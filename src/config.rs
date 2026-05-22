use crate::control::ControlPlane;
use crate::error::Result;
use crate::executor::NspawnExecutor;
use crate::ids::PolicyId;
use crate::network::NetworkPolicy;
use crate::nix_profile::FixedNixProfile;
use crate::storage::BtrfsStorage;
use std::env;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonConfig {
    pub state_root: PathBuf,
    pub nix_profile: FixedNixProfile,
    pub machine_prefix: String,
    pub default_network_policy: NetworkPolicy,
}

impl DaemonConfig {
    pub fn from_env() -> Result<Self> {
        let state_root = env_path("FLARENV_STATE_ROOT", "/var/lib/flarenv");
        let store_path = env_path("FLARENV_NIX_STORE", "/nix/store");
        let profile_path = env_path(
            "FLARENV_NIX_PROFILE",
            "/nix/var/nix/profiles/flarenv/global",
        );
        let machine_prefix =
            env::var("FLARENV_MACHINE_PREFIX").unwrap_or_else(|_| "flarenv".into());

        Ok(Self {
            state_root,
            nix_profile: FixedNixProfile::new(store_path, profile_path),
            machine_prefix,
            default_network_policy: NetworkPolicy::DenyAll {
                id: PolicyId::new("deny")?,
            },
        })
    }

    pub fn build_host_control_plane(&self) -> Result<ControlPlane<BtrfsStorage, NspawnExecutor>> {
        ControlPlane::new(
            BtrfsStorage::new(&self.state_root),
            NspawnExecutor::new(&self.machine_prefix),
            self.nix_profile.clone(),
            self.default_network_policy.clone(),
        )
    }
}

fn env_path(name: &str, default: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

#[cfg(test)]
mod tests {
    use super::DaemonConfig;

    #[test]
    fn default_config_builds_host_control_plane() {
        let config = DaemonConfig::from_env().unwrap();
        config.build_host_control_plane().unwrap();
    }
}
