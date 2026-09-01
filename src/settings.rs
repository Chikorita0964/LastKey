use std::{collections::HashSet, env, fmt, fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::{LogicalKey, PhysicalKey};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Settings {
    pub bindings: [PhysicalKey; 4],
    #[serde(default)]
    pub timing: TimingSettings,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimingSettings {
    pub transition_min_ms: u32,
    pub transition_max_ms: u32,
    pub overlap_min_ms: u32,
    pub overlap_max_ms: u32,
    pub overlap_probability: u8,
    pub full_overlap: bool,
}

#[derive(Debug)]
pub enum SettingsError {
    DuplicateBinding,
    EmptyBinding,
    InvalidTimingRange,
    InvalidOverlapProbability,
    Io(io::Error),
    MissingAppData,
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateBinding => write!(formatter, "each pair key must be unique"),
            Self::EmptyBinding => write!(formatter, "a key binding cannot be empty"),
            Self::InvalidTimingRange => {
                write!(formatter, "a timing minimum cannot exceed its maximum")
            }
            Self::InvalidOverlapProbability => {
                write!(formatter, "overlap probability must be between 0 and 100")
            }
            Self::Io(error) => write!(formatter, "settings I/O failed: {error}"),
            Self::MissingAppData => write!(formatter, "APPDATA is unavailable"),
            Self::Parse(error) => write!(formatter, "settings are invalid: {error}"),
            Self::Serialize(error) => write!(formatter, "settings serialization failed: {error}"),
        }
    }
}

impl std::error::Error for SettingsError {}

impl From<io::Error> for SettingsError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bindings: [
                PhysicalKey::new(0x11, false), // W
                PhysicalKey::new(0x1F, false), // S
                PhysicalKey::new(0x1E, false), // A
                PhysicalKey::new(0x20, false), // D
            ],
            timing: TimingSettings::default(),
        }
    }
}

impl Settings {
    pub fn binding(&self, key: LogicalKey) -> PhysicalKey {
        self.bindings[key.index()]
    }

    pub fn set_binding(&mut self, key: LogicalKey, physical: PhysicalKey) {
        self.bindings[key.index()] = physical;
    }

    pub fn logical_key_for(&self, physical: PhysicalKey) -> Option<LogicalKey> {
        LogicalKey::ALL
            .into_iter()
            .find(|logical| self.binding(*logical) == physical)
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.bindings.iter().any(|binding| binding.scan_code == 0) {
            return Err(SettingsError::EmptyBinding);
        }

        let unique: HashSet<PhysicalKey> = self.bindings.iter().copied().collect();
        if unique.len() != self.bindings.len() {
            return Err(SettingsError::DuplicateBinding);
        }
        if self.timing.transition_min_ms > self.timing.transition_max_ms
            || self.timing.overlap_min_ms > self.timing.overlap_max_ms
        {
            return Err(SettingsError::InvalidTimingRange);
        }
        if self.timing.overlap_probability > 100 {
            return Err(SettingsError::InvalidOverlapProbability);
        }
        Ok(())
    }
}

pub fn load() -> Result<Settings, SettingsError> {
    let path = settings_path()?;
    match fs::read_to_string(path) {
        Ok(contents) => {
            let settings: Settings = toml::from_str(&contents).map_err(SettingsError::Parse)?;
            settings.validate()?;
            Ok(settings)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(SettingsError::Io(error)),
    }
}

pub fn save(settings: &Settings) -> Result<(), SettingsError> {
    settings.validate()?;
    let path = settings_path()?;
    let parent = path.parent().expect("settings path always has a parent");
    fs::create_dir_all(parent)?;
    let contents = toml::to_string_pretty(settings).map_err(SettingsError::Serialize)?;
    fs::write(path, contents)?;
    Ok(())
}

fn settings_path() -> Result<PathBuf, SettingsError> {
    let app_data = env::var_os("APPDATA").ok_or(SettingsError::MissingAppData)?;
    Ok(PathBuf::from(app_data)
        .join("LastKey")
        .join("settings.toml"))
}
