use crate::error::{FlarenvError, Result};
use crate::executor::{ExecRequest, Executor, SessionExit};
use crate::gc::GcAction;
use crate::ids::{AgentId, PolicyId, SessionId, SnapshotId, WorkspaceId};
use crate::model::{ResourceLimits, SessionRequest, Workspace, WorkspaceSnapshot, WorkspaceState};
use crate::network::NetworkPolicy;
use crate::nix_profile::FixedNixProfile;
use crate::storage::StorageBackend;
use std::collections::BTreeMap;
use std::time::SystemTime;

#[derive(Debug, Default)]
pub struct InMemoryMetadata {
    workspaces: BTreeMap<WorkspaceId, Workspace>,
    snapshots: BTreeMap<SnapshotId, WorkspaceSnapshot>,
    policies: BTreeMap<PolicyId, NetworkPolicy>,
}

impl InMemoryMetadata {
    pub fn from_parts(
        workspaces: BTreeMap<WorkspaceId, Workspace>,
        snapshots: BTreeMap<SnapshotId, WorkspaceSnapshot>,
        policies: BTreeMap<PolicyId, NetworkPolicy>,
    ) -> Self {
        Self {
            workspaces,
            snapshots,
            policies,
        }
    }

    pub fn insert_policy(&mut self, policy: NetworkPolicy) {
        self.policies.insert(policy.id().clone(), policy);
    }

    pub fn workspaces(&self) -> impl Iterator<Item = &Workspace> {
        self.workspaces.values()
    }

    pub fn snapshots(&self) -> impl Iterator<Item = &WorkspaceSnapshot> {
        self.snapshots.values()
    }

    pub fn policies(&self) -> impl Iterator<Item = &NetworkPolicy> {
        self.policies.values()
    }

    pub fn workspace(&self, id: &WorkspaceId) -> Option<&Workspace> {
        self.workspaces.get(id)
    }

    pub fn snapshot(&self, id: &SnapshotId) -> Option<&WorkspaceSnapshot> {
        self.snapshots.get(id)
    }
}

#[derive(Debug)]
pub struct ControlPlane<S, E> {
    metadata: InMemoryMetadata,
    storage: S,
    executor: E,
    nix_profile: FixedNixProfile,
    default_policy: PolicyId,
}

impl<S, E> ControlPlane<S, E>
where
    S: StorageBackend,
    E: Executor,
{
    pub fn new(
        storage: S,
        executor: E,
        nix_profile: FixedNixProfile,
        default_policy: NetworkPolicy,
    ) -> Result<Self> {
        nix_profile.validate_paths()?;
        let default_policy_id = default_policy.id().clone();
        let mut metadata = InMemoryMetadata::default();
        metadata.insert_policy(default_policy);

        Ok(Self {
            metadata,
            storage,
            executor,
            nix_profile,
            default_policy: default_policy_id,
        })
    }

    pub fn metadata(&self) -> &InMemoryMetadata {
        &self.metadata
    }

    pub fn replace_metadata(&mut self, metadata: InMemoryMetadata) {
        self.metadata = metadata;
    }

    pub fn create_workspace(
        &mut self,
        workspace_id: WorkspaceId,
        agent_id: AgentId,
    ) -> Result<Workspace> {
        if self.metadata.workspaces.contains_key(&workspace_id) {
            return Err(FlarenvError::AlreadyExists(format!(
                "workspace {workspace_id}"
            )));
        }

        let root_path = self.storage.create_workspace(&workspace_id)?;
        let now = SystemTime::now();
        let workspace = Workspace {
            id: workspace_id.clone(),
            agent_id,
            root_path,
            parent_snapshot: None,
            limits: ResourceLimits::default(),
            network_policy: self.default_policy.clone(),
            state: WorkspaceState::Ready,
            created_at: now,
            last_accessed_at: now,
        };
        self.storage.set_quota(&workspace_id, &workspace.limits)?;
        self.metadata
            .workspaces
            .insert(workspace_id, workspace.clone());
        Ok(workspace)
    }

    pub fn snapshot_workspace(
        &mut self,
        workspace_id: &WorkspaceId,
        snapshot_id: SnapshotId,
    ) -> Result<WorkspaceSnapshot> {
        let source_workspace_id = self.ready_workspace(workspace_id)?.id.clone();
        if self.metadata.snapshots.contains_key(&snapshot_id) {
            return Err(FlarenvError::AlreadyExists(format!(
                "snapshot {snapshot_id}"
            )));
        }

        let root_path = self
            .storage
            .snapshot_workspace(workspace_id, &snapshot_id)?;
        let snapshot = WorkspaceSnapshot {
            id: snapshot_id.clone(),
            workspace_id: source_workspace_id,
            root_path,
            created_at: SystemTime::now(),
        };
        self.metadata
            .snapshots
            .insert(snapshot_id, snapshot.clone());
        Ok(snapshot)
    }

    pub fn branch_workspace(
        &mut self,
        snapshot_id: &SnapshotId,
        workspace_id: WorkspaceId,
        agent_id: AgentId,
    ) -> Result<Workspace> {
        if self.metadata.workspaces.contains_key(&workspace_id) {
            return Err(FlarenvError::AlreadyExists(format!(
                "workspace {workspace_id}"
            )));
        }
        if !self.metadata.snapshots.contains_key(snapshot_id) {
            return Err(FlarenvError::NotFound(format!("snapshot {snapshot_id}")));
        }

        let root_path = self.storage.clone_workspace(snapshot_id, &workspace_id)?;
        let now = SystemTime::now();
        let workspace = Workspace {
            id: workspace_id.clone(),
            agent_id,
            root_path,
            parent_snapshot: Some(snapshot_id.clone()),
            limits: ResourceLimits::default(),
            network_policy: self.default_policy.clone(),
            state: WorkspaceState::Ready,
            created_at: now,
            last_accessed_at: now,
        };
        self.storage.set_quota(&workspace_id, &workspace.limits)?;
        self.metadata
            .workspaces
            .insert(workspace_id, workspace.clone());
        Ok(workspace)
    }

    pub fn delete_workspace(&mut self, workspace_id: &WorkspaceId) -> Result<()> {
        let workspace = self
            .metadata
            .workspaces
            .get_mut(workspace_id)
            .ok_or_else(|| FlarenvError::NotFound(format!("workspace {workspace_id}")))?;

        if workspace.state == WorkspaceState::Deleted {
            return Ok(());
        }

        workspace.state = WorkspaceState::Deleted;
        Ok(())
    }

    pub fn purge_workspace(&mut self, workspace_id: &WorkspaceId) -> Result<()> {
        let workspace = self
            .metadata
            .workspaces
            .get(workspace_id)
            .ok_or_else(|| FlarenvError::NotFound(format!("workspace {workspace_id}")))?;
        if workspace.state != WorkspaceState::Deleted {
            return Err(FlarenvError::PreconditionFailed(format!(
                "workspace {workspace_id} must be soft-deleted before purge"
            )));
        }

        self.storage.delete_workspace(workspace_id)?;
        self.metadata.workspaces.remove(workspace_id);
        Ok(())
    }

    pub fn purge_snapshot(&mut self, snapshot_id: &SnapshotId) -> Result<()> {
        if self
            .metadata
            .workspaces
            .values()
            .any(|workspace| workspace.parent_snapshot.as_ref() == Some(snapshot_id))
        {
            return Err(FlarenvError::PreconditionFailed(format!(
                "snapshot {snapshot_id} is still used by a branch"
            )));
        }
        if !self.metadata.snapshots.contains_key(snapshot_id) {
            return Err(FlarenvError::NotFound(format!("snapshot {snapshot_id}")));
        }

        self.storage.delete_snapshot(snapshot_id)?;
        self.metadata.snapshots.remove(snapshot_id);
        Ok(())
    }

    pub fn execute_gc_actions(&mut self, actions: &[GcAction]) -> Result<()> {
        for action in actions {
            match action {
                GcAction::DeleteWorkspace { workspace_id } => {
                    self.purge_workspace(workspace_id)?;
                }
                GcAction::DeleteSnapshot { snapshot_id } => {
                    self.purge_snapshot(snapshot_id)?;
                }
            }
        }
        Ok(())
    }

    pub fn set_limits(&mut self, workspace_id: &WorkspaceId, limits: ResourceLimits) -> Result<()> {
        self.ready_workspace(workspace_id)?;
        self.storage.set_quota(workspace_id, &limits)?;
        self.metadata
            .workspaces
            .get_mut(workspace_id)
            .expect("ready_workspace already checked existence")
            .limits = limits;
        Ok(())
    }

    pub fn set_network_policy(
        &mut self,
        workspace_id: &WorkspaceId,
        policy: NetworkPolicy,
    ) -> Result<()> {
        self.ready_workspace(workspace_id)?;
        let policy_id = policy.id().clone();
        self.metadata.insert_policy(policy);
        self.metadata
            .workspaces
            .get_mut(workspace_id)
            .expect("ready_workspace already checked existence")
            .network_policy = policy_id;
        Ok(())
    }

    pub fn open_session(
        &mut self,
        workspace_id: &WorkspaceId,
        session_id: SessionId,
        session: SessionRequest,
    ) -> Result<SessionExit> {
        let mut workspace = self.ready_workspace(workspace_id)?.clone();
        let policy = self
            .metadata
            .policies
            .get(&workspace.network_policy)
            .ok_or_else(|| {
                FlarenvError::PreconditionFailed(format!(
                    "network policy {} is missing",
                    workspace.network_policy
                ))
            })?
            .clone();

        workspace.last_accessed_at = SystemTime::now();
        self.metadata
            .workspaces
            .insert(workspace_id.clone(), workspace.clone());

        self.executor.open_session(ExecRequest {
            session_id,
            workspace,
            session,
            nix_profile: self.nix_profile.clone(),
            network_policy: policy,
        })
    }

    fn ready_workspace(&self, workspace_id: &WorkspaceId) -> Result<&Workspace> {
        let workspace = self
            .metadata
            .workspaces
            .get(workspace_id)
            .ok_or_else(|| FlarenvError::NotFound(format!("workspace {workspace_id}")))?;
        if workspace.state != WorkspaceState::Ready {
            return Err(FlarenvError::PreconditionFailed(format!(
                "workspace {workspace_id} is not ready"
            )));
        }
        Ok(workspace)
    }
}

#[cfg(test)]
mod tests {
    use super::ControlPlane;
    use crate::executor::RecordingExecutor;
    use crate::ids::{AgentId, PolicyId, SessionId, SnapshotId, WorkspaceId};
    use crate::model::{ResourceLimits, SessionRequest};
    use crate::network::NetworkPolicy;
    use crate::nix_profile::FixedNixProfile;
    use crate::storage::InMemoryStorage;

    fn control_plane() -> ControlPlane<InMemoryStorage, RecordingExecutor> {
        ControlPlane::new(
            InMemoryStorage::new("/tmp/flarenv-test"),
            RecordingExecutor::default(),
            FixedNixProfile::default(),
            NetworkPolicy::DenyAll {
                id: PolicyId::new("deny").unwrap(),
            },
        )
        .unwrap()
    }

    #[test]
    fn creates_snapshots_and_branches_workspaces() {
        let mut cp = control_plane();
        let workspace_id = WorkspaceId::new("workspace_a").unwrap();
        let agent_id = AgentId::new("agent_a").unwrap();
        let snapshot_id = SnapshotId::new("snap_a").unwrap();

        let workspace = cp
            .create_workspace(workspace_id.clone(), agent_id.clone())
            .unwrap();
        assert_eq!(workspace.id, workspace_id);

        let snapshot = cp
            .snapshot_workspace(&workspace_id, snapshot_id.clone())
            .unwrap();
        assert_eq!(snapshot.workspace_id, workspace_id);

        let branch_id = WorkspaceId::new("workspace_branch").unwrap();
        let branch = cp
            .branch_workspace(&snapshot_id, branch_id.clone(), agent_id)
            .unwrap();
        assert_eq!(branch.id, branch_id);
        assert_eq!(branch.parent_snapshot, Some(snapshot_id));
    }

    #[test]
    fn updates_limits_and_network_policy_before_session() {
        let mut cp = control_plane();
        let workspace_id = WorkspaceId::new("workspace_a").unwrap();
        cp.create_workspace(workspace_id.clone(), AgentId::new("agent_a").unwrap())
            .unwrap();

        cp.set_limits(
            &workspace_id,
            ResourceLimits {
                cpu_weight: 50,
                memory_max_bytes: 1024,
                pids_max: 8,
                disk_max_bytes: 2048,
            },
        )
        .unwrap();
        cp.set_network_policy(
            &workspace_id,
            NetworkPolicy::AllowEgress {
                id: PolicyId::new("egress").unwrap(),
                cidrs: vec!["10.0.0.0/8".into()],
            },
        )
        .unwrap();

        let exit = cp
            .open_session(
                &workspace_id,
                SessionId::new("session_a").unwrap(),
                SessionRequest {
                    command: Some(vec!["/bin/true".into()]),
                    tty: false,
                },
            )
            .unwrap();
        assert_eq!(exit.code, 0);
        assert_eq!(
            cp.metadata()
                .workspace(&workspace_id)
                .unwrap()
                .network_policy
                .as_str(),
            "egress"
        );
    }

    #[test]
    fn deleted_workspaces_cannot_open_sessions() {
        let mut cp = control_plane();
        let workspace_id = WorkspaceId::new("workspace_a").unwrap();
        cp.create_workspace(workspace_id.clone(), AgentId::new("agent_a").unwrap())
            .unwrap();
        cp.delete_workspace(&workspace_id).unwrap();

        let result = cp.open_session(
            &workspace_id,
            SessionId::new("session_a").unwrap(),
            SessionRequest::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn gc_actions_purge_soft_deleted_workspaces_and_snapshots() {
        let mut cp = control_plane();
        let workspace_id = WorkspaceId::new("workspace_a").unwrap();
        let snapshot_id = SnapshotId::new("snap_a").unwrap();
        cp.create_workspace(workspace_id.clone(), AgentId::new("agent_a").unwrap())
            .unwrap();
        cp.snapshot_workspace(&workspace_id, snapshot_id.clone())
            .unwrap();
        cp.delete_workspace(&workspace_id).unwrap();

        cp.execute_gc_actions(&[
            crate::gc::GcAction::DeleteSnapshot {
                snapshot_id: snapshot_id.clone(),
            },
            crate::gc::GcAction::DeleteWorkspace {
                workspace_id: workspace_id.clone(),
            },
        ])
        .unwrap();

        assert!(cp.metadata().workspace(&workspace_id).is_none());
        assert!(cp.metadata().snapshot(&snapshot_id).is_none());
    }
}
