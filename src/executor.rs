use crate::error::Result;
use crate::ids::{SessionId, WorkspaceId};
use crate::model::{ResourceLimits, SessionRequest, Workspace};
use crate::network::NetworkPolicy;
use crate::nix_profile::FixedNixProfile;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecRequest {
    pub session_id: SessionId,
    pub workspace: Workspace,
    pub session: SessionRequest,
    pub nix_profile: FixedNixProfile,
    pub network_policy: NetworkPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionExit {
    pub code: i32,
}

pub trait Executor {
    fn open_session(&mut self, request: ExecRequest) -> Result<SessionExit>;
}

#[derive(Debug, Default)]
pub struct RecordingExecutor {
    pub sessions: BTreeMap<SessionId, ExecRequest>,
}

impl Executor for RecordingExecutor {
    fn open_session(&mut self, request: ExecRequest) -> Result<SessionExit> {
        self.sessions.insert(request.session_id.clone(), request);
        Ok(SessionExit { code: 0 })
    }
}

#[derive(Clone, Debug)]
pub struct NspawnExecutor {
    machine_prefix: String,
}

impl NspawnExecutor {
    pub fn new(machine_prefix: impl Into<String>) -> Self {
        Self {
            machine_prefix: machine_prefix.into(),
        }
    }

    pub fn command_for(&self, request: &ExecRequest) -> Command {
        let mut command = Command::new("systemd-nspawn");
        command
            .arg("--quiet")
            .arg("--as-pid2")
            .arg("--directory")
            .arg(&request.workspace.root_path)
            .arg("--machine")
            .arg(format!(
                "{}-{}-{}",
                self.machine_prefix, request.workspace.id, request.session_id
            ))
            .arg("--bind-ro")
            .arg(format!(
                "{}:{}",
                request.nix_profile.store_path.display(),
                request.nix_profile.store_path.display()
            ))
            .arg("--bind-ro")
            .arg(format!(
                "{}:{}",
                request.nix_profile.profile_path.display(),
                "/run/current-system/sw"
            ));

        add_limit_args(&mut command, &request.workspace.limits);
        add_network_args(&mut command, &request.network_policy);

        if request.session.tty {
            command.arg("--console=interactive");
        } else {
            command.arg("--pipe");
        }

        command.arg("--");
        match &request.session.command {
            Some(argv) if !argv.is_empty() => {
                command.args(argv);
            }
            _ => {
                command.arg("/bin/sh");
            }
        }

        command
    }
}

impl Executor for NspawnExecutor {
    fn open_session(&mut self, request: ExecRequest) -> Result<SessionExit> {
        let status = self.command_for(&request).status()?;
        Ok(SessionExit {
            code: status.code().unwrap_or(128),
        })
    }
}

fn add_limit_args(command: &mut Command, limits: &ResourceLimits) {
    command
        .arg("--property")
        .arg(format!("CPUWeight={}", limits.cpu_weight))
        .arg("--property")
        .arg(format!("MemoryMax={}", limits.memory_max_bytes))
        .arg("--property")
        .arg(format!("TasksMax={}", limits.pids_max));
}

fn add_network_args(command: &mut Command, policy: &NetworkPolicy) {
    command.arg("--private-network");
    match policy {
        NetworkPolicy::DenyAll { .. } => {}
        NetworkPolicy::AllowEgress { id, .. } => {
            command.arg("--network-veth").arg("--network-zone").arg(id.as_str());
        }
    }
}

pub fn workspace_run_dir(workspace_id: &WorkspaceId) -> PathBuf {
    PathBuf::from("/run/flarenv").join(workspace_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::{ExecRequest, NspawnExecutor};
    use crate::ids::{AgentId, PolicyId, SessionId, WorkspaceId};
    use crate::model::{ResourceLimits, SessionRequest, Workspace, WorkspaceState};
    use crate::network::NetworkPolicy;
    use crate::nix_profile::FixedNixProfile;
    use std::time::SystemTime;

    #[test]
    fn nspawn_command_contains_root_profile_and_limits() {
        let workspace_id = WorkspaceId::new("workspace_1").unwrap();
        let request = ExecRequest {
            session_id: SessionId::new("session_1").unwrap(),
            workspace: Workspace {
                id: workspace_id,
                agent_id: AgentId::new("agent_1").unwrap(),
                root_path: "/var/lib/flarenv/workspaces/workspace_1".into(),
                parent_snapshot: None,
                limits: ResourceLimits::default(),
                network_policy: PolicyId::new("deny").unwrap(),
                state: WorkspaceState::Ready,
                created_at: SystemTime::UNIX_EPOCH,
                last_accessed_at: SystemTime::UNIX_EPOCH,
            },
            session: SessionRequest {
                command: Some(vec!["/bin/echo".into(), "ok".into()]),
                tty: false,
            },
            nix_profile: FixedNixProfile::default(),
            network_policy: NetworkPolicy::DenyAll {
                id: PolicyId::new("deny").unwrap(),
            },
        };

        let command = NspawnExecutor::new("flarenv").command_for(&request);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert!(args.contains(&"--directory".to_string()));
        assert!(args.contains(&"/var/lib/flarenv/workspaces/workspace_1".to_string()));
        assert!(args.contains(&"--private-network".to_string()));
        assert!(args.contains(&"--pipe".to_string()));
        assert!(args.contains(&"/bin/echo".to_string()));
    }
}
