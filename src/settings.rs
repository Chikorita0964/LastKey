use std::{
    collections::HashSet,
    env, fmt, fs,
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};

use crate::core::{LogicalKey, PhysicalKey};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Settings {
    pub bindings: [PhysicalKey; 4],
    #[serde(default)]
    pub timing: TimingSettings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimingSettings {
    pub socd_transition_delay_enabled: bool,
    pub socd_transition_min_micros: u32,
    pub socd_transition_max_micros: u32,
    pub preserve_overlap: bool,
    pub overlap_preservation_rate: u8,
    pub preserved_overlap_min_micros: u32,
    pub preserved_overlap_max_micros: u32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredMilliseconds {
    Integer(u32),
    Decimal(f64),
}

impl StoredMilliseconds {
    fn into_micros<E>(self) -> Result<u32, E>
    where
        E: serde::de::Error,
    {
        match self {
            Self::Integer(milliseconds) => milliseconds
                .checked_mul(1_000)
                .ok_or_else(|| E::custom("timing value is too large")),
            Self::Decimal(milliseconds) if milliseconds.is_finite() && milliseconds >= 0.0 => {
                let tenths = (milliseconds * 10.0).round();
                if tenths > f64::from(u32::MAX / 100) {
                    return Err(E::custom("timing value is too large"));
                }
                Ok(tenths as u32 * 100)
            }
            Self::Decimal(_) => Err(E::custom("timing value must be a finite positive number")),
        }
    }
}

/// Stored timing settings use only the current field names. Aliases from
/// unreleased development builds (`transition_min_ms`, `overlap_probability`,
/// `full_overlap`, and friends) are not recognized: no released build ever
/// wrote a settings file, so there is nothing to migrate. Unknown fields are
/// ignored and missing fields take the defaults below; anything else must
/// pass `validate()` when the file is loaded.
#[derive(Default, Deserialize)]
struct StoredTimingSettings {
    #[serde(default)]
    socd_transition_delay_enabled: Option<bool>,
    #[serde(default)]
    socd_transition_min_ms: Option<StoredMilliseconds>,
    #[serde(default)]
    socd_transition_max_ms: Option<StoredMilliseconds>,
    #[serde(default)]
    preserve_overlap: Option<bool>,
    #[serde(default)]
    overlap_preservation_rate: Option<u8>,
    #[serde(default)]
    preserved_overlap_min_ms: Option<StoredMilliseconds>,
    #[serde(default)]
    preserved_overlap_max_ms: Option<StoredMilliseconds>,
}

impl<'de> Deserialize<'de> for TimingSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let stored = StoredTimingSettings::deserialize(deserializer)?;
        Ok(Self {
            socd_transition_delay_enabled: stored.socd_transition_delay_enabled.unwrap_or(false),
            socd_transition_min_micros: stored
                .socd_transition_min_ms
                .map(StoredMilliseconds::into_micros)
                .transpose()?
                .unwrap_or(2_000),
            socd_transition_max_micros: stored
                .socd_transition_max_ms
                .map(StoredMilliseconds::into_micros)
                .transpose()?
                .unwrap_or(4_000),
            preserve_overlap: stored.preserve_overlap.unwrap_or(false),
            overlap_preservation_rate: stored.overlap_preservation_rate.unwrap_or(50),
            preserved_overlap_min_micros: stored
                .preserved_overlap_min_ms
                .map(StoredMilliseconds::into_micros)
                .transpose()?
                .unwrap_or(2_000),
            preserved_overlap_max_micros: stored
                .preserved_overlap_max_ms
                .map(StoredMilliseconds::into_micros)
                .transpose()?
                .unwrap_or(6_000),
        })
    }
}

impl Serialize for TimingSettings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("TimingSettings", 7)?;
        state.serialize_field(
            "socd_transition_delay_enabled",
            &self.socd_transition_delay_enabled,
        )?;
        state.serialize_field(
            "socd_transition_min_ms",
            &millis_decimal(self.socd_transition_min_micros),
        )?;
        state.serialize_field(
            "socd_transition_max_ms",
            &millis_decimal(self.socd_transition_max_micros),
        )?;
        state.serialize_field("preserve_overlap", &self.preserve_overlap)?;
        state.serialize_field("overlap_preservation_rate", &self.overlap_preservation_rate)?;
        state.serialize_field(
            "preserved_overlap_min_ms",
            &millis_decimal(self.preserved_overlap_min_micros),
        )?;
        state.serialize_field(
            "preserved_overlap_max_ms",
            &millis_decimal(self.preserved_overlap_max_micros),
        )?;
        state.end()
    }
}

fn millis_decimal(micros: u32) -> f64 {
    f64::from(micros) / 1_000.0
}

impl Default for TimingSettings {
    fn default() -> Self {
        Self {
            socd_transition_delay_enabled: false,
            socd_transition_min_micros: 2_000,
            socd_transition_max_micros: 4_000,
            preserve_overlap: false,
            overlap_preservation_rate: 50,
            preserved_overlap_min_micros: 2_000,
            preserved_overlap_max_micros: 6_000,
        }
    }
}

impl TimingSettings {
    pub fn effective_overlap_preservation_rate(&self) -> u8 {
        if self.socd_transition_delay_enabled && self.preserve_overlap {
            self.overlap_preservation_rate
        } else {
            0
        }
    }
}

/// Upper bound shared by every timing entry path: the settings UI clamps
/// typed values at 1000 ms and sliders at 20 ms, so anything larger can only
/// arrive from a hand-edited file or a skewed recommendation.
pub const MAX_TIMING_MICROS: u32 = 1_000_000;

#[derive(Debug)]
pub enum SettingsError {
    DuplicateBinding,
    EmptyBinding,
    InvalidTimingRange,
    InvalidTimingMaximum,
    InvalidOverlapPreservationRate,
    InvalidPreservedOverlapDuration,
    InvalidTimingPrecision,
    Io(io::Error),
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
            Self::InvalidTimingMaximum => {
                write!(formatter, "a timing value cannot exceed 1000 ms")
            }
            Self::InvalidOverlapPreservationRate => {
                write!(
                    formatter,
                    "configured overlap preservation rate must be between 1 and 100"
                )
            }
            Self::InvalidPreservedOverlapDuration => {
                write!(
                    formatter,
                    "preserved overlap duration must be at least 0.1 ms when overlap preservation is active"
                )
            }
            Self::InvalidTimingPrecision => {
                write!(formatter, "timing values must use 0.1 ms increments")
            }
            Self::Io(error) => write!(formatter, "settings I/O failed: {error}"),
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
        if self.timing.socd_transition_min_micros > self.timing.socd_transition_max_micros
            || self.timing.preserved_overlap_min_micros > self.timing.preserved_overlap_max_micros
        {
            return Err(SettingsError::InvalidTimingRange);
        }
        if [
            self.timing.socd_transition_min_micros,
            self.timing.socd_transition_max_micros,
            self.timing.preserved_overlap_min_micros,
            self.timing.preserved_overlap_max_micros,
        ]
        .into_iter()
        .any(|micros| micros > MAX_TIMING_MICROS)
        {
            return Err(SettingsError::InvalidTimingMaximum);
        }
        if !(1..=100).contains(&self.timing.overlap_preservation_rate) {
            return Err(SettingsError::InvalidOverlapPreservationRate);
        }
        if self.timing.preserved_overlap_min_micros < 100 {
            return Err(SettingsError::InvalidPreservedOverlapDuration);
        }
        if [
            self.timing.socd_transition_min_micros,
            self.timing.socd_transition_max_micros,
            self.timing.preserved_overlap_min_micros,
            self.timing.preserved_overlap_max_micros,
        ]
        .into_iter()
        .any(|micros| micros % 100 != 0)
        {
            return Err(SettingsError::InvalidTimingPrecision);
        }
        Ok(())
    }
}

pub fn load() -> Result<Settings, SettingsError> {
    let paths = settings_paths()?;
    match load_from(&paths.primary) {
        Err(SettingsError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            if paths.legacy != paths.primary {
                match load_from(&paths.legacy) {
                    Err(SettingsError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                        Ok(Settings::default())
                    }
                    result => result,
                }
            } else {
                Ok(Settings::default())
            }
        }
        result => result,
    }
}

fn load_from(path: &Path) -> Result<Settings, SettingsError> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let settings: Settings = toml::from_str(&contents).map_err(SettingsError::Parse)?;
            settings.validate()?;
            Ok(settings)
        }
        Err(error) => Err(SettingsError::Io(error)),
    }
}

pub fn save(settings: &Settings) -> Result<(), SettingsError> {
    settings.validate()?;
    let path = settings_paths()?.primary;
    let parent = path.parent().expect("settings path always has a parent");
    fs::create_dir_all(parent)?;
    let contents = toml::to_string_pretty(settings).map_err(SettingsError::Serialize)?;
    write_settings_file(&path, contents.as_bytes())?;
    Ok(())
}

fn write_settings_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "settings path has no name"))?;
    let mut temporary_name = file_name.to_os_string();
    temporary_name.push(".tmp");
    let temporary_path = path.with_file_name(temporary_name);

    let result = (|| {
        let mut temporary = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary_path)?;
        temporary.write_all(contents)?;
        temporary.sync_all()?;
        drop(temporary);
        replace_file(&temporary_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::HSTRING,
    };

    unsafe {
        MoveFileExW(
            &HSTRING::from(source.as_os_str()),
            &HSTRING::from(destination.as_os_str()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

struct SettingsPaths {
    primary: PathBuf,
    legacy: PathBuf,
}

fn settings_paths() -> Result<SettingsPaths, SettingsError> {
    let executable = env::current_exe()?;
    let local_app_data = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let portable = executable.with_file_name("lastkey.portable").is_file();
    Ok(settings_paths_for(
        &executable,
        local_app_data.as_deref(),
        portable,
    ))
}

fn settings_paths_for(
    executable: &Path,
    local_app_data: Option<&Path>,
    portable: bool,
) -> SettingsPaths {
    let legacy = executable.with_file_name("settings.toml");
    let primary = if portable {
        legacy.clone()
    } else if let Some(local_app_data) = local_app_data {
        local_app_data.join("LastKey").join("settings.toml")
    } else {
        legacy.clone()
    };
    SettingsPaths { primary, legacy }
}

#[cfg(test)]
mod tests {
    use super::{settings_paths_for, write_settings_file};
    use std::{fs, path::Path};

    #[test]
    fn installed_settings_use_per_user_local_application_data() {
        let paths = settings_paths_for(
            Path::new(r"C:\Program Files\WindowsApps\LastKey\LastKey.exe"),
            Some(Path::new(r"C:\Users\player\AppData\Local")),
            false,
        );
        assert_eq!(
            paths.primary,
            Path::new(r"C:\Users\player\AppData\Local\LastKey\settings.toml")
        );
        assert_eq!(
            paths.legacy,
            Path::new(r"C:\Program Files\WindowsApps\LastKey\settings.toml")
        );
    }

    #[test]
    fn portable_settings_remain_next_to_the_executable() {
        let paths = settings_paths_for(
            Path::new(r"C:\apps\LastKey\LastKey.exe"),
            Some(Path::new(r"C:\Users\player\AppData\Local")),
            true,
        );
        assert_eq!(paths.primary, Path::new(r"C:\apps\LastKey\settings.toml"));
        assert_eq!(paths.legacy, paths.primary);
    }

    #[test]
    fn platforms_without_local_application_data_keep_the_legacy_location() {
        let paths = settings_paths_for(Path::new("/opt/lastkey/lastkey"), None, false);
        assert_eq!(paths.primary, Path::new("/opt/lastkey/settings.toml"));
    }

    #[test]
    fn settings_file_replacement_does_not_leave_a_temporary_file() {
        let directory = std::env::temp_dir().join(format!(
            "lastkey-settings-save-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time is valid")
                .as_nanos()
        ));
        fs::create_dir_all(&directory).expect("test directory is created");
        let path = directory.join("settings.toml");

        write_settings_file(&path, b"first").expect("initial settings are written");
        write_settings_file(&path, b"second").expect("settings are replaced");

        assert_eq!(fs::read(&path).expect("settings are readable"), b"second");
        assert!(!directory.join("settings.toml.tmp").exists());
        fs::remove_dir_all(directory).expect("test directory is removed");
    }
}
