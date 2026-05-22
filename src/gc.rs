use crate::control::InMemoryMetadata;
use crate::ids::{SnapshotId, WorkspaceId};
use crate::model::WorkspaceState;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GcPolicy {
    pub retain_snapshots_per_workspace: usize,
    pub delete_deleted_workspaces_after: Duration,
}

impl Default for GcPolicy {
    fn default() -> Self {
        Self {
            retain_snapshots_per_workspace: 3,
            delete_deleted_workspaces_after: Duration::from_secs(7 * 24 * 60 * 60),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GcAction {
    DeleteWorkspace { workspace_id: WorkspaceId },
    DeleteSnapshot { snapshot_id: SnapshotId },
}

pub fn plan_gc(metadata: &InMemoryMetadata, now: SystemTime, policy: &GcPolicy) -> Vec<GcAction> {
    let mut actions = Vec::new();

    for workspace in metadata.workspaces() {
        if workspace.state == WorkspaceState::Deleted
            && age(now, workspace.last_accessed_at) >= policy.delete_deleted_workspaces_after
        {
            actions.push(GcAction::DeleteWorkspace {
                workspace_id: workspace.id.clone(),
            });
        }
    }

    let protected_snapshots: BTreeSet<_> = metadata
        .workspaces()
        .filter_map(|workspace| workspace.parent_snapshot.clone())
        .collect();
    let mut snapshots_by_workspace: BTreeMap<WorkspaceId, Vec<_>> = BTreeMap::new();
    for snapshot in metadata.snapshots() {
        snapshots_by_workspace
            .entry(snapshot.workspace_id.clone())
            .or_default()
            .push(snapshot);
    }

    for snapshots in snapshots_by_workspace.values_mut() {
        snapshots.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.created_at));
        for snapshot in snapshots.iter().skip(policy.retain_snapshots_per_workspace) {
            if !protected_snapshots.contains(&snapshot.id) {
                actions.push(GcAction::DeleteSnapshot {
                    snapshot_id: snapshot.id.clone(),
                });
            }
        }
    }

    actions
}

fn age(now: SystemTime, then: SystemTime) -> Duration {
    now.duration_since(then).unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod tests {
    use super::{plan_gc, GcAction, GcPolicy};
    use crate::control::InMemoryMetadata;
    use crate::ids::{AgentId, PolicyId, SnapshotId, WorkspaceId};
    use crate::model::{ResourceLimits, Workspace, WorkspaceSnapshot, WorkspaceState};
    use crate::network::NetworkPolicy;
    use std::collections::BTreeMap;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn plans_old_deleted_workspaces_and_old_snapshots() {
        let workspace_id = WorkspaceId::new("workspace_a").unwrap();
        let keep_snapshot = SnapshotId::new("snap_keep").unwrap();
        let delete_snapshot = SnapshotId::new("snap_delete").unwrap();
        let mut workspaces = BTreeMap::new();
        workspaces.insert(
            workspace_id.clone(),
            Workspace {
                id: workspace_id.clone(),
                agent_id: AgentId::new("agent_a").unwrap(),
                root_path: "/tmp/workspace_a".into(),
                parent_snapshot: None,
                limits: ResourceLimits::default(),
                network_policy: PolicyId::new("deny").unwrap(),
                state: WorkspaceState::Deleted,
                created_at: UNIX_EPOCH,
                last_accessed_at: UNIX_EPOCH,
            },
        );

        let mut snapshots = BTreeMap::new();
        snapshots.insert(
            keep_snapshot.clone(),
            WorkspaceSnapshot {
                id: keep_snapshot.clone(),
                workspace_id: workspace_id.clone(),
                root_path: "/tmp/snap_keep".into(),
                created_at: UNIX_EPOCH + Duration::from_secs(10),
            },
        );
        snapshots.insert(
            delete_snapshot.clone(),
            WorkspaceSnapshot {
                id: delete_snapshot.clone(),
                workspace_id,
                root_path: "/tmp/snap_delete".into(),
                created_at: UNIX_EPOCH,
            },
        );

        let mut policies = BTreeMap::new();
        policies.insert(
            PolicyId::new("deny").unwrap(),
            NetworkPolicy::DenyAll {
                id: PolicyId::new("deny").unwrap(),
            },
        );
        let metadata = InMemoryMetadata::from_parts(workspaces, snapshots, policies);

        let actions = plan_gc(
            &metadata,
            UNIX_EPOCH + Duration::from_secs(100),
            &GcPolicy {
                retain_snapshots_per_workspace: 1,
                delete_deleted_workspaces_after: Duration::from_secs(50),
            },
        );

        assert!(actions.contains(&GcAction::DeleteWorkspace {
            workspace_id: WorkspaceId::new("workspace_a").unwrap()
        }));
        assert!(actions.contains(&GcAction::DeleteSnapshot {
            snapshot_id: delete_snapshot
        }));
        assert!(!actions.contains(&GcAction::DeleteSnapshot {
            snapshot_id: keep_snapshot
        }));
    }
}
