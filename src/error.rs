//! Error types for Aster.

use std::fmt;

#[derive(Debug)]
pub enum AsterError {
    Io(std::io::Error),
    Json(serde_json::Error),
    NotFound(String),
    Validation(String),
    Runtime(String),
    Dependency(String),
    Http(String),
    Other(String),
}

impl fmt::Display for AsterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsterError::Io(e) => write!(f, "{e}"),
            AsterError::Json(e) => write!(f, "{e}"),
            AsterError::NotFound(m) => write!(f, "{m}"),
            AsterError::Validation(m) => write!(f, "{m}"),
            AsterError::Runtime(m) => write!(f, "{m}"),
            AsterError::Dependency(m) => write!(f, "{m}"),
            AsterError::Http(m) => write!(f, "{m}"),
            AsterError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for AsterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AsterError::Io(e) => Some(e),
            AsterError::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AsterError {
    fn from(e: std::io::Error) -> Self {
        AsterError::Io(e)
    }
}

impl From<serde_json::Error> for AsterError {
    fn from(e: serde_json::Error) -> Self {
        AsterError::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, AsterError>;
