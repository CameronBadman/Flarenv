use crate::control::ControlPlane;
use crate::error::{FlarenvError, Result};
use crate::executor::Executor;
use crate::frontend::{AllowListAuthorizer, SessionAuthorizer};
use crate::ids::{SessionId, WorkspaceId};
use crate::model::SessionRequest;
use crate::storage::StorageBackend;
use rand::rng;
use russh::keys::{Algorithm, PrivateKey, PublicKey};
use russh::server::{Auth, Config, Msg, Server as _, Session};
use russh::{server, Channel, ChannelId};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SshWireConfig {
    pub listen_addr: SocketAddr,
    pub workspace_id: WorkspaceId,
    pub inactivity_timeout: Duration,
}

impl SshWireConfig {
    pub fn new(listen_addr: SocketAddr, workspace_id: WorkspaceId) -> Self {
        Self {
            listen_addr,
            workspace_id,
            inactivity_timeout: Duration::from_secs(3600),
        }
    }
}

pub struct FlarenvSshServer<S, E> {
    config: SshWireConfig,
    control_plane: Arc<Mutex<ControlPlane<S, E>>>,
    authorizer: AllowListAuthorizer,
    session_counter: Arc<AtomicU64>,
    authenticated_principal: Option<String>,
}

impl<S, E> Clone for FlarenvSshServer<S, E> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            control_plane: self.control_plane.clone(),
            authorizer: self.authorizer.clone(),
            session_counter: self.session_counter.clone(),
            authenticated_principal: self.authenticated_principal.clone(),
        }
    }
}

impl<S, E> FlarenvSshServer<S, E>
where
    S: StorageBackend + Send + 'static,
    E: Executor + Send + 'static,
{
    pub fn new(
        config: SshWireConfig,
        control_plane: ControlPlane<S, E>,
        authorizer: AllowListAuthorizer,
    ) -> Self {
        Self {
            config,
            control_plane: Arc::new(Mutex::new(control_plane)),
            authorizer,
            session_counter: Arc::new(AtomicU64::new(1)),
            authenticated_principal: None,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let ssh_config = Arc::new(Config {
            inactivity_timeout: Some(self.config.inactivity_timeout),
            auth_rejection_time: Duration::from_secs(1),
            auth_rejection_time_initial: Some(Duration::ZERO),
            keys: vec![PrivateKey::random(&mut rng(), Algorithm::Ed25519)
                .map_err(|err| FlarenvError::Execution(err.to_string()))?],
            ..Default::default()
        });
        let listener = TcpListener::bind(self.config.listen_addr).await?;
        self.run_on_socket(ssh_config, &listener)
            .await
            .map_err(|err| FlarenvError::Execution(err.to_string()))
    }

    fn next_session_id(&self) -> Result<SessionId> {
        let id = self.session_counter.fetch_add(1, Ordering::Relaxed);
        SessionId::new(format!("ssh_{id}"))
    }

    async fn open_flarenv_session(
        &self,
        channel: ChannelId,
        command: Option<Vec<String>>,
        tty: bool,
        session: &mut Session,
    ) -> std::result::Result<(), russh::Error> {
        if self.authenticated_principal.is_none() {
            session.channel_failure(channel)?;
            return Ok(());
        }

        let session_id = match self.next_session_id() {
            Ok(id) => id,
            Err(err) => {
                session.extended_data(channel, 1, err.to_string())?;
                session.channel_failure(channel)?;
                return Ok(());
            }
        };
        let request = SessionRequest { command, tty };
        let exit = {
            let mut control_plane = self.control_plane.lock().await;
            control_plane.open_session(&self.config.workspace_id, session_id, request)
        };

        match exit {
            Ok(exit) => {
                session.channel_success(channel)?;
                session.exit_status_request(channel, exit.code.max(0) as u32)?;
                session.eof(channel)?;
                session.close(channel)?;
            }
            Err(err) => {
                session.extended_data(channel, 1, err.to_string())?;
                session.channel_failure(channel)?;
            }
        }
        Ok(())
    }
}

impl<S, E> server::Server for FlarenvSshServer<S, E>
where
    S: StorageBackend + Send + 'static,
    E: Executor + Send + 'static,
{
    type Handler = Self;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self {
        let mut handler = self.clone();
        handler.authenticated_principal = None;
        handler
    }
}

impl<S, E> server::Handler for FlarenvSshServer<S, E>
where
    S: StorageBackend + Send + 'static,
    E: Executor + Send + 'static,
{
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> std::result::Result<Auth, Self::Error> {
        let principal = match public_key.to_openssh() {
            Ok(key) => key,
            Err(_) => {
                return Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                });
            }
        };
        if self
            .authorizer
            .can_open(&principal, &self.config.workspace_id)
        {
            self.authenticated_principal = Some(format!("{user}:{principal}"));
            Ok(Auth::Accept)
        } else {
            Ok(Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        _session: &mut Session,
    ) -> std::result::Result<bool, Self::Error> {
        Ok(self.authenticated_principal.is_some())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        self.open_flarenv_session(channel, None, true, session)
            .await
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data).into_owned();
        self.open_flarenv_session(
            channel,
            Some(vec!["/bin/sh".into(), "-lc".into(), command]),
            false,
            session,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::SshWireConfig;
    use crate::ids::WorkspaceId;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    #[test]
    fn ssh_wire_config_sets_workspace_and_timeout() {
        let workspace_id = WorkspaceId::new("workspace_a").unwrap();
        let config = SshWireConfig::new(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 2222),
            workspace_id.clone(),
        );

        assert_eq!(config.workspace_id, workspace_id);
        assert_eq!(config.inactivity_timeout, Duration::from_secs(3600));
    }
}
