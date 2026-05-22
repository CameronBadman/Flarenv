use crate::control::InMemoryMetadata;
use crate::error::{FlarenvError, Result};
use crate::ids::{AgentId, PolicyId, SnapshotId, WorkspaceId};
use crate::model::{ResourceLimits, Workspace, WorkspaceSnapshot, WorkspaceState};
use crate::network::NetworkPolicy;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileMetadataStore {
    path: PathBuf,
}

impl FileMetadataStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn save(&self, metadata: &InMemoryMetadata) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut lines = Vec::new();
        lines.push("flarenv-metadata-v1".to_string());
        for policy in metadata.policies() {
            lines.push(encode_policy(policy));
        }
        for workspace in metadata.workspaces() {
            lines.push(encode_workspace(workspace));
        }
        for snapshot in metadata.snapshots() {
            lines.push(encode_snapshot(snapshot));
        }

        fs::write(&self.path, lines.join("\n") + "\n")?;
        Ok(())
    }

    pub fn load(&self) -> Result<InMemoryMetadata> {
        let content = fs::read_to_string(&self.path)?;
        decode_metadata(&content)
    }
}

fn encode_policy(policy: &NetworkPolicy) -> String {
    match policy {
        NetworkPolicy::DenyAll { id } => format!("policy\t{}\tdeny", id.as_str()),
        NetworkPolicy::AllowEgress { id, cidrs } => {
            format!("policy\t{}\tallow\t{}", id.as_str(), cidrs.join(","))
        }
    }
}

fn encode_workspace(workspace: &Workspace) -> String {
    format!(
        "workspace\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        workspace.id.as_str(),
        workspace.agent_id.as_str(),
        workspace.root_path.display(),
        workspace
            .parent_snapshot
            .as_ref()
            .map(SnapshotId::as_str)
            .unwrap_or("-"),
        workspace.limits.cpu_weight,
        workspace.limits.memory_max_bytes,
        workspace.limits.pids_max,
        workspace.limits.disk_max_bytes,
        workspace.network_policy.as_str(),
        encode_state(&workspace.state),
        encode_time(workspace.created_at),
        encode_time(workspace.last_accessed_at),
    )
}

fn encode_snapshot(snapshot: &WorkspaceSnapshot) -> String {
    format!(
        "snapshot\t{}\t{}\t{}\t{}",
        snapshot.id.as_str(),
        snapshot.workspace_id.as_str(),
        snapshot.root_path.display(),
        encode_time(snapshot.created_at),
    )
}

fn decode_metadata(content: &str) -> Result<InMemoryMetadata> {
    let mut lines = content.lines();
    match lines.next() {
        Some("flarenv-metadata-v1") => {}
        _ => {
            return Err(FlarenvError::InvalidInput(
                "metadata header is missing or unsupported".into(),
            ));
        }
    }

    let mut workspaces = BTreeMap::new();
    let mut snapshots = BTreeMap::new();
    let mut policies = BTreeMap::new();

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        match fields.first().copied() {
            Some("policy") => {
                let policy = decode_policy(&fields)?;
                policies.insert(policy.id().clone(), policy);
            }
            Some("workspace") => {
                let workspace = decode_workspace(&fields)?;
                workspaces.insert(workspace.id.clone(), workspace);
            }
            Some("snapshot") => {
                let snapshot = decode_snapshot(&fields)?;
                snapshots.insert(snapshot.id.clone(), snapshot);
            }
            Some(kind) => {
                return Err(FlarenvError::InvalidInput(format!(
                    "unknown metadata record {kind}"
                )));
            }
            None => {}
        }
    }

    Ok(InMemoryMetadata::from_parts(
        workspaces, snapshots, policies,
    ))
}

fn decode_policy(fields: &[&str]) -> Result<NetworkPolicy> {
    require_fields(fields, 3, "policy")?;
    let id = PolicyId::new(fields[1])?;
    match fields[2] {
        "deny" => Ok(NetworkPolicy::DenyAll { id }),
        "allow" => Ok(NetworkPolicy::AllowEgress {
            id,
            cidrs: fields
                .get(3)
                .copied()
                .unwrap_or("")
                .split(',')
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
        }),
        value => Err(FlarenvError::InvalidInput(format!(
            "unknown network policy kind {value}"
        ))),
    }
}

fn decode_workspace(fields: &[&str]) -> Result<Workspace> {
    require_fields(fields, 13, "workspace")?;
    let parent_snapshot = if fields[4] == "-" {
        None
    } else {
        Some(SnapshotId::new(fields[4])?)
    };

    Ok(Workspace {
        id: WorkspaceId::new(fields[1])?,
        agent_id: AgentId::new(fields[2])?,
        root_path: PathBuf::from(fields[3]),
        parent_snapshot,
        limits: ResourceLimits {
            cpu_weight: parse_field(fields[5], "cpu_weight")?,
            memory_max_bytes: parse_field(fields[6], "memory_max_bytes")?,
            pids_max: parse_field(fields[7], "pids_max")?,
            disk_max_bytes: parse_field(fields[8], "disk_max_bytes")?,
        },
        network_policy: PolicyId::new(fields[9])?,
        state: decode_state(fields[10])?,
        created_at: decode_time(fields[11])?,
        last_accessed_at: decode_time(fields[12])?,
    })
}

fn decode_snapshot(fields: &[&str]) -> Result<WorkspaceSnapshot> {
    require_fields(fields, 5, "snapshot")?;
    Ok(WorkspaceSnapshot {
        id: SnapshotId::new(fields[1])?,
        workspace_id: WorkspaceId::new(fields[2])?,
        root_path: PathBuf::from(fields[3]),
        created_at: decode_time(fields[4])?,
    })
}

fn require_fields(fields: &[&str], min_count: usize, kind: &str) -> Result<()> {
    if fields.len() < min_count {
        return Err(FlarenvError::InvalidInput(format!(
            "{kind} record has too few fields"
        )));
    }
    Ok(())
}

fn parse_field<T>(value: &str, name: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|err| FlarenvError::InvalidInput(format!("invalid {name}: {err}")))
}

fn encode_state(state: &WorkspaceState) -> &'static str {
    match state {
        WorkspaceState::Ready => "ready",
        WorkspaceState::Deleted => "deleted",
    }
}

fn decode_state(value: &str) -> Result<WorkspaceState> {
    match value {
        "ready" => Ok(WorkspaceState::Ready),
        "deleted" => Ok(WorkspaceState::Deleted),
        _ => Err(FlarenvError::InvalidInput(format!(
            "unknown workspace state {value}"
        ))),
    }
}

fn encode_time(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn decode_time(value: &str) -> Result<SystemTime> {
    Ok(UNIX_EPOCH + Duration::from_secs(parse_field(value, "timestamp")?))
}

#[cfg(test)]
mod tests {
    use super::FileMetadataStore;
    use crate::control::ControlPlane;
    use crate::executor::RecordingExecutor;
    use crate::ids::{AgentId, PolicyId, SnapshotId, WorkspaceId};
    use crate::network::NetworkPolicy;
    use crate::nix_profile::FixedNixProfile;
    use crate::storage::InMemoryStorage;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn saves_and_loads_metadata() {
        let mut cp = ControlPlane::new(
            InMemoryStorage::new("/tmp/flarenv-test"),
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
        cp.snapshot_workspace(&workspace_id, SnapshotId::new("snap_a").unwrap())
            .unwrap();

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("flarenv-metadata-{nonce}"));
        let store = FileMetadataStore::new(dir.join("metadata.tsv"));

        store.save(cp.metadata()).unwrap();
        let loaded = store.load().unwrap();

        assert!(loaded.workspace(&workspace_id).is_some());
        assert!(loaded
            .snapshot(&SnapshotId::new("snap_a").unwrap())
            .is_some());

        fs::remove_dir_all(dir).unwrap();
    }
}
