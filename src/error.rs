use std::fmt;
use std::io;

pub type Result<T> = std::result::Result<T, FlarenvError>;

#[derive(Debug)]
pub enum FlarenvError {
    AlreadyExists(String),
    InvalidInput(String),
    Io(io::Error),
    NotFound(String),
    PreconditionFailed(String),
    Storage(String),
    Execution(String),
}

impl fmt::Display for FlarenvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists(msg) => write!(f, "already exists: {msg}"),
            Self::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::PreconditionFailed(msg) => write!(f, "precondition failed: {msg}"),
            Self::Storage(msg) => write!(f, "storage error: {msg}"),
            Self::Execution(msg) => write!(f, "execution error: {msg}"),
        }
    }
}

impl std::error::Error for FlarenvError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for FlarenvError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
