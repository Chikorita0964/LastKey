use std::fmt;

use crate::settings::SettingsError;

#[derive(Debug)]
pub enum AppControllerError {
    InvalidSettings(SettingsError),
    Persistence(String),
    Runtime(String),
    RuntimeWithRollbackFailure { runtime: String, rollback: String },
}

impl fmt::Display for AppControllerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSettings(error) => write!(formatter, "invalid settings: {error}"),
            Self::Persistence(error) => write!(formatter, "settings persistence failed: {error}"),
            Self::Runtime(error) => write!(formatter, "runtime operation failed: {error}"),
            Self::RuntimeWithRollbackFailure { runtime, rollback } => write!(
                formatter,
                "runtime operation failed: {runtime}; restoring the previous settings also failed: {rollback}"
            ),
        }
    }
}

impl std::error::Error for AppControllerError {}
