use crate::config::DaemonConfig;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightReport {
    pub checks: Vec<HostCheck>,
}

impl PreflightReport {
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|check| check.ok)
    }
}

pub fn run_preflight(config: &DaemonConfig) -> PreflightReport {
    let path = env::var_os("PATH").unwrap_or_default();
    let checks = vec![
        binary_check("btrfs", &path),
        binary_check("systemd-nspawn", &path),
        path_check("state parent", state_parent(&config.state_root)),
        path_check("nix store", &config.nix_profile.store_path),
        path_check("nix profile", &config.nix_profile.profile_path),
    ];

    PreflightReport { checks }
}

fn binary_check(name: &str, path: &std::ffi::OsStr) -> HostCheck {
    match find_in_path(name, path) {
        Some(found) => HostCheck {
            name: format!("binary:{name}"),
            ok: true,
            detail: found.display().to_string(),
        },
        None => HostCheck {
            name: format!("binary:{name}"),
            ok: false,
            detail: "not found in PATH".into(),
        },
    }
}

fn path_check(name: &str, path: impl AsRef<Path>) -> HostCheck {
    let path = path.as_ref();
    HostCheck {
        name: format!("path:{name}"),
        ok: path.exists(),
        detail: path.display().to_string(),
    }
}

fn state_parent(state_root: &Path) -> &Path {
    state_root.parent().unwrap_or_else(|| Path::new("/"))
}

fn find_in_path(binary: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    env::split_paths(path)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::{find_in_path, PreflightReport};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn report_ok_requires_all_checks_to_pass() {
        let report = PreflightReport {
            checks: vec![
                super::HostCheck {
                    name: "a".into(),
                    ok: true,
                    detail: "".into(),
                },
                super::HostCheck {
                    name: "b".into(),
                    ok: false,
                    detail: "".into(),
                },
            ],
        };

        assert!(!report.ok());
    }

    #[test]
    fn finds_binary_in_explicit_path() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("flarenv-preflight-{nonce}"));
        let binary = dir.join("btrfs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&binary, b"").unwrap();
        let mut perms = fs::metadata(&binary).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary, perms).unwrap();

        assert_eq!(find_in_path("btrfs", dir.as_os_str()), Some(binary));

        fs::remove_dir_all(dir).unwrap();
    }
}
