//! UI-independent application coordination.

mod controller;
mod error;
mod ports;
mod state;

pub use controller::AppController;
pub use error::AppControllerError;
pub use ports::{FileSettingsStore, RuntimeService, SettingsStore};
pub use state::{AppSnapshot, CapturedKey, MeasurementUpdate};
