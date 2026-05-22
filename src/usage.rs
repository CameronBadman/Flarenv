use crate::control::InMemoryMetadata;
use crate::error::Result;
use crate::ids::WorkspaceId;
use crate::model::WorkspaceState;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceUsage {
    pub workspace_id: WorkspaceId,
    pub state: WorkspaceState,
    pub logical_bytes: u64,
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
    pub quota_bytes: u64,
    pub memory_limit_bytes: u64,
    pub pids_limit: u64,
}

pub trait UsageProbe {
    fn logical_bytes(&self, path: &Path) -> Result<u64>;
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
        workspaces.push(WorkspaceUsage {
            workspace_id: workspace.id.clone(),
            state: workspace.state.clone(),
            logical_bytes,
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

#[cfg(test)]
mod tests {
    use super::{measure_usage, PathUsageProbe};
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
        assert_eq!(report.workspaces[0].workspace_id, workspace_id);

        fs::remove_dir_all(root).unwrap();
    }
}
