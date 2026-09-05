use std::sync::mpsc::Receiver;

use crate::settings::{self, Settings};

use super::{CapturedKey, MeasurementUpdate};

pub trait SettingsStore {
    fn save(&self, settings: &Settings) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FileSettingsStore;

impl SettingsStore for FileSettingsStore {
    fn save(&self, settings: &Settings) -> Result<(), String> {
        settings::save(settings).map_err(|error| error.to_string())
    }
}

pub trait RuntimeService {
    fn apply(&self, settings: Settings) -> Result<(), String>;

    /// Returns the settings the runtime is currently running. Queued behind
    /// any in-flight Apply, so the answer confirms whether it activated.
    fn active_settings(&self) -> Result<Settings, String>;

    fn begin_key_capture(&self) -> Result<Receiver<CapturedKey>, String>;

    fn cancel_key_capture(&self) -> Result<(), String>;

    fn start_measurement(&self) -> Result<Receiver<MeasurementUpdate>, String>;

    fn stop_measurement(&self) -> Result<Option<MeasurementUpdate>, String>;
}
