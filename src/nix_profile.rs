use crate::error::{FlarenvError, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedNixProfile {
    pub store_path: PathBuf,
    pub profile_path: PathBuf,
}

impl FixedNixProfile {
    pub fn new(store_path: impl Into<PathBuf>, profile_path: impl Into<PathBuf>) -> Self {
        Self {
            store_path: store_path.into(),
            profile_path: profile_path.into(),
        }
    }

    pub fn validate_paths(&self) -> Result<()> {
        validate_absolute("store_path", &self.store_path)?;
        validate_absolute("profile_path", &self.profile_path)?;
        Ok(())
    }
}

impl Default for FixedNixProfile {
    fn default() -> Self {
        Self::new("/nix/store", "/nix/var/nix/profiles/flarenv/global")
    }
}

fn validate_absolute(name: &str, path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(FlarenvError::InvalidInput(format!("{name} must be absolute")));
    }
    Ok(())
}
