use crate::control::ControlPlane;
use crate::error::{FlarenvError, Result};
use crate::executor::{Executor, SessionExit};
use crate::ids::{SessionId, WorkspaceId};
use crate::model::SessionRequest;
use crate::storage::StorageBackend;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshSessionRequest {
    pub principal: String,
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub session: SessionRequest,
}

pub trait SessionAuthorizer {
    fn can_open(&self, principal: &str, workspace_id: &WorkspaceId) -> bool;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AllowListAuthorizer {
    grants: BTreeMap<String, BTreeSet<WorkspaceId>>,
}

impl AllowListAuthorizer {
    pub fn grant(&mut self, principal: impl Into<String>, workspace_id: WorkspaceId) {
        self.grants
            .entry(principal.into())
            .or_default()
            .insert(workspace_id);
    }
}

impl SessionAuthorizer for AllowListAuthorizer {
    fn can_open(&self, principal: &str, workspace_id: &WorkspaceId) -> bool {
        self.grants
            .get(principal)
            .is_some_and(|workspaces| workspaces.contains(workspace_id))
    }
}

pub struct SshSessionRouter<A> {
    authorizer: A,
}

impl<A> SshSessionRouter<A>
where
    A: SessionAuthorizer,
{
    pub fn new(authorizer: A) -> Self {
        Self { authorizer }
    }

    pub fn open_session<S, E>(
        &self,
        control_plane: &mut ControlPlane<S, E>,
        request: SshSessionRequest,
    ) -> Result<SessionExit>
    where
        S: StorageBackend,
        E: Executor,
    {
        if !self
            .authorizer
            .can_open(&request.principal, &request.workspace_id)
        {
            return Err(FlarenvError::PreconditionFailed(format!(
                "principal {} is not authorized for workspace {}",
                request.principal, request.workspace_id
            )));
        }

        control_plane.open_session(&request.workspace_id, request.session_id, request.session)
    }
}

#[cfg(test)]
mod tests {
    use super::{AllowListAuthorizer, SshSessionRequest, SshSessionRouter};
    use crate::control::ControlPlane;
    use crate::executor::RecordingExecutor;
    use crate::ids::{AgentId, PolicyId, SessionId, WorkspaceId};
    use crate::model::SessionRequest;
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
    fn authorizes_ssh_principal_before_opening_session() {
        let mut cp = control_plane();
        let workspace_id = WorkspaceId::new("workspace_a").unwrap();
        cp.create_workspace(workspace_id.clone(), AgentId::new("agent_a").unwrap())
            .unwrap();

        let mut authorizer = AllowListAuthorizer::default();
        authorizer.grant("agent-key-a", workspace_id.clone());
        let router = SshSessionRouter::new(authorizer);

        let exit = router
            .open_session(
                &mut cp,
                SshSessionRequest {
                    principal: "agent-key-a".into(),
                    workspace_id,
                    session_id: SessionId::new("session_a").unwrap(),
                    session: SessionRequest::default(),
                },
            )
            .unwrap();

        assert_eq!(exit.code, 0);
    }

    #[test]
    fn denies_unauthorized_principal() {
        let mut cp = control_plane();
        let workspace_id = WorkspaceId::new("workspace_a").unwrap();
        cp.create_workspace(workspace_id.clone(), AgentId::new("agent_a").unwrap())
            .unwrap();

        let router = SshSessionRouter::new(AllowListAuthorizer::default());
        let result = router.open_session(
            &mut cp,
            SshSessionRequest {
                principal: "agent-key-a".into(),
                workspace_id,
                session_id: SessionId::new("session_a").unwrap(),
                session: SessionRequest::default(),
            },
        );

        assert!(result.is_err());
    }
}
