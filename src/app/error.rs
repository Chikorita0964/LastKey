use std::fmt;

use crate::settings::SettingsError;

#[derive(Debug)]
pub enum AppControllerError {
    InvalidSettings(SettingsError),
    Persistence(String),
    Runtime(String),
    RuntimeWithRollbackFailure {
        runtime: String,
        rollback: String,
    },
    /// The Apply acknowledgement timed out and the engine state could not be
    /// confirmed afterwards. The previous file was restored, but a wedged
    /// input thread may still activate the candidate later.
    RuntimeUnconfirmed {
        runtime: String,
        fence: String,
    },
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
            Self::RuntimeUnconfirmed { runtime, fence } => write!(
                formatter,
                "runtime apply failed: {runtime}; the engine state could not be confirmed: {fence}"
            ),
        }
    }
}

impl std::error::Error for AppControllerError {}
