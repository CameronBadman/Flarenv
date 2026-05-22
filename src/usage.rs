use crate::control::InMemoryMetadata;
use crate::error::{FlarenvError, Result};
use crate::ids::WorkspaceId;
use crate::model::WorkspaceState;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceUsage {
    pub workspace_id: WorkspaceId,
    pub state: WorkspaceState,
    pub logical_bytes: u64,
    pub retained_bytes: Option<u64>,
    pub disk_quota_bytes: u64,
    pub memory_limit_bytes: u64,
    pub pids_limit: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapacityReport {
    pub workspaces: Vec<WorkspaceUsage>,
    pub ready_workspaces: usize,
    pub deleted_workspaces: usize,
    pub logical_bytes: u64,
    pub retained_bytes: Option<u64>,
    pub quota_bytes: u64,
    pub memory_limit_bytes: u64,
    pub pids_limit: u64,
}

pub trait UsageProbe {
    fn logical_bytes(&self, path: &Path) -> Result<u64>;

    fn retained_bytes(&self, _path: &Path) -> Result<Option<u64>> {
        Ok(None)
    }
}

#[derive(Clone, Debug, Default)]
pub struct PathUsageProbe;

impl UsageProbe for PathUsageProbe {
    fn logical_bytes(&self, path: &Path) -> Result<u64> {
        directory_size(path)
    }
}

pub fn measure_usage(
    metadata: &InMemoryMetadata,
    probe: &impl UsageProbe,
) -> Result<CapacityReport> {
    let mut workspaces = Vec::new();

    for workspace in metadata.workspaces() {
        let logical_bytes = match probe.logical_bytes(&workspace.root_path) {
            Ok(bytes) => bytes,
            Err(crate::FlarenvError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => 0,
            Err(err) => return Err(err),
        };
        let retained_bytes = probe.retained_bytes(&workspace.root_path)?;
        workspaces.push(WorkspaceUsage {
            workspace_id: workspace.id.clone(),
            state: workspace.state.clone(),
            logical_bytes,
            retained_bytes,
            disk_quota_bytes: workspace.limits.disk_max_bytes,
            memory_limit_bytes: workspace.limits.memory_max_bytes,
            pids_limit: workspace.limits.pids_max,
        });
    }

    let ready_workspaces = workspaces
        .iter()
        .filter(|usage| usage.state == WorkspaceState::Ready)
        .count();
    let deleted_workspaces = workspaces
        .iter()
        .filter(|usage| usage.state == WorkspaceState::Deleted)
        .count();
    let logical_bytes = workspaces.iter().map(|usage| usage.logical_bytes).sum();
    let retained_bytes = sum_optional(workspaces.iter().map(|usage| usage.retained_bytes));
    let quota_bytes = workspaces.iter().map(|usage| usage.disk_quota_bytes).sum();
    let memory_limit_bytes = workspaces
        .iter()
        .map(|usage| usage.memory_limit_bytes)
        .sum();
    let pids_limit = workspaces
        .iter()
        .map(|usage| u64::from(usage.pids_limit))
        .sum();

    Ok(CapacityReport {
        workspaces,
        ready_workspaces,
        deleted_workspaces,
        logical_bytes,
        retained_bytes,
        quota_bytes,
        memory_limit_bytes,
        pids_limit,
    })
}

fn directory_size(path: &Path) -> Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(metadata.len());
    }

    let mut bytes = metadata.len();
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            bytes += directory_size(&entry?.path())?;
        }
    }
    Ok(bytes)
}

#[derive(Clone, Debug, Default)]
pub struct BtrfsUsageProbe;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BtrfsQgroupUsage {
    pub referenced_bytes: u64,
    pub exclusive_bytes: u64,
}

impl UsageProbe for BtrfsUsageProbe {
    fn logical_bytes(&self, path: &Path) -> Result<u64> {
        directory_size(path)
    }

    fn retained_bytes(&self, path: &Path) -> Result<Option<u64>> {
        Ok(Some(read_btrfs_qgroup_usage(path)?.exclusive_bytes))
    }
}

pub fn read_btrfs_qgroup_usage(path: &Path) -> Result<BtrfsQgroupUsage> {
    let output = Command::new("btrfs")
        .arg("qgroup")
        .arg("show")
        .arg("--raw")
        .arg("--referenced")
        .arg("--exclusive")
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(FlarenvError::Storage(format!(
            "btrfs qgroup show failed with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    parse_btrfs_qgroup_show(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_btrfs_qgroup_show(output: &str) -> Result<BtrfsQgroupUsage> {
    for line in output.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 3 || !fields[0].contains('/') || fields[0].starts_with('-') {
            continue;
        }

        return Ok(BtrfsQgroupUsage {
            referenced_bytes: parse_u64(fields[1], "referenced bytes")?,
            exclusive_bytes: parse_u64(fields[2], "exclusive bytes")?,
        });
    }

    Err(FlarenvError::Storage(
        "btrfs qgroup output did not contain a usage row".into(),
    ))
}

fn parse_u64(value: &str, name: &str) -> Result<u64> {
    value
        .parse()
        .map_err(|err| FlarenvError::Storage(format!("invalid {name}: {err}")))
}

fn sum_optional(values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    let mut saw_value = false;
    let mut total = 0;
    for value in values {
        if let Some(value) = value {
            saw_value = true;
            total += value;
        }
    }
    saw_value.then_some(total)
}

#[cfg(test)]
mod tests {
    use super::{measure_usage, parse_btrfs_qgroup_show, PathUsageProbe};
    use crate::control::ControlPlane;
    use crate::executor::RecordingExecutor;
    use crate::ids::{AgentId, PolicyId, WorkspaceId};
    use crate::network::NetworkPolicy;
    use crate::nix_profile::FixedNixProfile;
    use crate::storage::InMemoryStorage;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn measures_workspace_usage_without_following_symlink_targets() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("flarenv-usage-{nonce}"));
        let workspace_root = root.join("workspaces").join("workspace_a");
        fs::create_dir_all(&workspace_root).unwrap();
        fs::write(workspace_root.join("file.txt"), b"hello").unwrap();
        std::os::unix::fs::symlink("/nix/store/not-charged", workspace_root.join("bin")).unwrap();

        let mut cp = ControlPlane::new(
            InMemoryStorage::new(&root),
            RecordingExecutor::default(),
            FixedNixProfile::default(),
            NetworkPolicy::DenyAll {
                id: PolicyId::new("deny").unwrap(),
            },
        )
        .unwrap();
        let workspace_id = WorkspaceId::new("workspace_a").unwrap();
        cp.create_workspace(workspace_id.clone(), AgentId::new("agent_a").unwrap())
            .unwrap();

        let report = measure_usage(cp.metadata(), &PathUsageProbe).unwrap();

        assert_eq!(report.ready_workspaces, 1);
        assert!(report.logical_bytes >= 5);
        assert_eq!(report.retained_bytes, None);
        assert_eq!(report.workspaces[0].workspace_id, workspace_id);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_raw_btrfs_qgroup_usage() {
        let usage = parse_btrfs_qgroup_show(
            "qgroupid         rfer         excl\n\
             --------         ----         ----\n\
             0/257            4096         2048\n",
        )
        .unwrap();

        assert_eq!(usage.referenced_bytes, 4096);
        assert_eq!(usage.exclusive_bytes, 2048);
    }
}
