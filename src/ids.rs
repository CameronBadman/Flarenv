use crate::error::{FlarenvError, Result};
use std::fmt;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                validate_id(stringify!($name), &value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(AgentId);
id_type!(WorkspaceId);
id_type!(SnapshotId);
id_type!(PolicyId);
id_type!(SessionId);

fn validate_id(kind: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(FlarenvError::InvalidInput(format!(
            "{kind} cannot be empty"
        )));
    }

    let valid = value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
    if !valid {
        return Err(FlarenvError::InvalidInput(format!(
            "{kind} must contain only ASCII letters, numbers, '-' or '_'"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::WorkspaceId;

    #[test]
    fn rejects_path_like_ids() {
        assert!(WorkspaceId::new("../escape").is_err());
        assert!(WorkspaceId::new("workspace/one").is_err());
    }
}
