//! Control-plane foundation for Flarenv.
//!
//! The crate keeps host-specific work behind traits so lifecycle behavior can
//! be tested without root privileges, btrfs, or systemd.

pub mod control;
pub mod error;
pub mod executor;
pub mod ids;
pub mod model;
pub mod network;
pub mod nix_profile;
pub mod storage;

pub use control::{ControlPlane, InMemoryMetadata};
pub use error::{FlarenvError, Result};
pub use executor::{ExecRequest, Executor, NspawnExecutor, SessionExit};
pub use ids::{AgentId, PolicyId, SessionId, SnapshotId, WorkspaceId};
pub use model::{ResourceLimits, SessionRequest, Workspace, WorkspaceSnapshot, WorkspaceState};
pub use network::NetworkPolicy;
pub use nix_profile::FixedNixProfile;
pub use storage::{BtrfsStorage, InMemoryStorage, StorageBackend};
