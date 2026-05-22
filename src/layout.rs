use crate::error::Result;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

pub const GUEST_PROFILE_PATH: &str = "/run/current-system/sw";

pub fn initialize_workspace_root(root: &Path) -> Result<()> {
    create_dirs(root)?;
    create_profile_symlink(root.join("bin"), "bin")?;
    create_profile_symlink(root.join("usr").join("bin"), "bin")?;
    create_profile_symlink(root.join("lib"), "lib")?;
    Ok(())
}

fn create_dirs(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("home").join("agent"))?;
    fs::create_dir_all(root.join("run"))?;
    fs::create_dir_all(root.join("tmp"))?;
    fs::create_dir_all(root.join("usr"))?;
    fs::create_dir_all(root.join("var").join("tmp"))?;
    Ok(())
}

fn create_profile_symlink(link: PathBuf, profile_child: &str) -> Result<()> {
    if link.exists() {
        return Ok(());
    }

    let target = Path::new(GUEST_PROFILE_PATH).join(profile_child);
    symlink(target, link)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{initialize_workspace_root, GUEST_PROFILE_PATH};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn initializes_linux_like_root_layout() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("flarenv-layout-{nonce}"));

        initialize_workspace_root(&root).unwrap();

        assert!(root.join("home").join("agent").is_dir());
        assert!(root.join("tmp").is_dir());
        assert_eq!(
            fs::read_link(root.join("bin")).unwrap(),
            std::path::Path::new(GUEST_PROFILE_PATH).join("bin")
        );

        fs::remove_dir_all(root).unwrap();
    }
}
