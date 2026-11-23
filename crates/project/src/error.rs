//! Project errors.

use std::fmt;

#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    /// The file is not valid project JSON.
    Format(serde_json::Error),
    /// The file is structurally valid JSON but not a usable project.
    Invalid(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Format(e) => write!(f, "Invalid project data: {e}"),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<std::io::Error> for ProjectError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ProjectError {
    fn from(value: serde_json::Error) -> Self {
        Self::Format(value)
    }
}
