use crate::ids::{AgentId, PolicyId, SnapshotId, WorkspaceId};
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceState {
    Ready,
    Deleted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceLimits {
    pub cpu_weight: u16,
    pub memory_max_bytes: u64,
    pub pids_max: u32,
    pub disk_max_bytes: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpu_weight: 100,
            memory_max_bytes: 2 * 1024 * 1024 * 1024,
            pids_max: 256,
            disk_max_bytes: 10 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub agent_id: AgentId,
    pub root_path: PathBuf,
    pub parent_snapshot: Option<SnapshotId>,
    pub limits: ResourceLimits,
    pub network_policy: PolicyId,
    pub state: WorkspaceState,
    pub created_at: SystemTime,
    pub last_accessed_at: SystemTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSnapshot {
    pub id: SnapshotId,
    pub workspace_id: WorkspaceId,
    pub root_path: PathBuf,
    pub created_at: SystemTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRequest {
    pub command: Option<Vec<String>>,
    pub tty: bool,
}

impl Default for SessionRequest {
    fn default() -> Self {
        Self {
            command: None,
            tty: true,
        }
    }
}
