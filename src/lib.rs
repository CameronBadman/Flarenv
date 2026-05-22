//! Control-plane foundation for Flarenv.
//!
//! The crate keeps host-specific work behind traits so lifecycle behavior can
//! be tested without root privileges, btrfs, or systemd.

pub mod config;
pub mod control;
pub mod error;
pub mod executor;
pub mod frontend;
pub mod ids;
pub mod layout;
pub mod model;
pub mod network;
pub mod nix_profile;
pub mod preflight;
pub mod storage;

pub use config::DaemonConfig;
pub use control::{ControlPlane, InMemoryMetadata};
pub use error::{FlarenvError, Result};
pub use executor::{ExecRequest, Executor, NspawnExecutor, SessionExit};
pub use frontend::{AllowListAuthorizer, SessionAuthorizer, SshSessionRequest, SshSessionRouter};
pub use ids::{AgentId, PolicyId, SessionId, SnapshotId, WorkspaceId};
pub use layout::{initialize_workspace_root, GUEST_PROFILE_PATH};
pub use model::{ResourceLimits, SessionRequest, Workspace, WorkspaceSnapshot, WorkspaceState};
pub use network::NetworkPolicy;
pub use nix_profile::FixedNixProfile;
pub use preflight::{run_preflight, HostCheck, PreflightReport};
pub use storage::{BtrfsStorage, InMemoryStorage, StorageBackend};
