use lastkey::{
    core::PhysicalKey,
    settings::{Settings, SettingsError, TimingSettings},
};

#[test]
fn default_settings_are_valid() {
    let settings = Settings::default();

    assert!(settings.validate().is_ok());
    assert!(!settings.timing.socd_transition_delay_enabled);
    assert_eq!(settings.timing.socd_transition_min_micros, 2_000);
    assert_eq!(settings.timing.socd_transition_max_micros, 4_000);
    assert!(!settings.timing.preserve_overlap);
    assert_eq!(settings.timing.overlap_preservation_rate, 50);
    assert_eq!(settings.timing.preserved_overlap_min_micros, 2_000);
    assert_eq!(settings.timing.preserved_overlap_max_micros, 6_000);
    assert_eq!(settings.timing.effective_overlap_preservation_rate(), 0);
}

#[test]
fn duplicate_bindings_are_rejected() {
    let mut settings = Settings::default();
    settings.bindings[3] = settings.bindings[2];

    assert!(matches!(
        settings.validate(),
        Err(SettingsError::DuplicateBinding)
    ));
}

#[test]
fn empty_bindings_are_rejected() {
    let mut settings = Settings::default();
    settings.bindings[0] = PhysicalKey::new(0, false);

    assert!(matches!(
        settings.validate(),
        Err(SettingsError::EmptyBinding)
    ));
}

#[test]
fn settings_round_trip_through_toml() {
    let settings = Settings::default();
    let text = toml::to_string_pretty(&settings).expect("settings serialize");
    let restored: Settings = toml::from_str(&text).expect("settings deserialize");

    assert_eq!(restored, settings);
}

#[test]
fn invalid_timing_settings_are_rejected() {
    let mut settings = Settings::default();
    settings.timing.socd_transition_min_micros = 2_000;
    settings.timing.socd_transition_max_micros = 1_000;
    assert!(matches!(
        settings.validate(),
        Err(SettingsError::InvalidTimingRange)
    ));

    settings.timing.socd_transition_min_micros = 0;
    settings.timing.socd_transition_max_micros = 0;
    settings.timing.overlap_preservation_rate = 101;
    assert!(matches!(
        settings.validate(),
        Err(SettingsError::InvalidOverlapPreservationRate)
    ));
}

#[test]
fn active_overlap_preservation_requires_a_nonzero_duration() {
    let mut settings = Settings::default();
    settings.timing.socd_transition_delay_enabled = true;
    settings.timing.preserve_overlap = true;
    settings.timing.overlap_preservation_rate = 50;
    settings.timing.preserved_overlap_min_micros = 0;

    assert!(matches!(
        settings.validate(),
        Err(SettingsError::InvalidPreservedOverlapDuration)
    ));

    settings.timing.preserve_overlap = false;
    settings.timing.preserved_overlap_min_micros = 100;
    assert!(settings.validate().is_ok());
}

#[test]
fn disabled_transition_delay_preserves_overlap_preference_without_applying_it() {
    let mut settings = Settings::default();
    settings.timing.preserve_overlap = true;

    assert!(settings.validate().is_ok());
    assert!(settings.timing.preserve_overlap);
    assert_eq!(settings.timing.effective_overlap_preservation_rate(), 0);

    let text = toml::to_string_pretty(&settings).expect("settings serialize");
    let restored: Settings = toml::from_str(&text).expect("settings deserialize");

    assert!(restored.timing.preserve_overlap);
    assert_eq!(restored.timing.effective_overlap_preservation_rate(), 0);

    settings.timing.socd_transition_delay_enabled = true;
    assert_eq!(settings.timing.effective_overlap_preservation_rate(), 50);
}

#[test]
fn decimal_millisecond_settings_round_trip_at_tenth_millisecond_precision() {
    let mut settings = Settings::default();
    settings.timing.socd_transition_min_micros = 1_900;
    settings.timing.socd_transition_max_micros = 4_000;
    settings.timing.socd_transition_delay_enabled = true;
    settings.timing.preserve_overlap = true;
    settings.timing.overlap_preservation_rate = 50;
    settings.timing.preserved_overlap_min_micros = 2_000;
    settings.timing.preserved_overlap_max_micros = 6_000;
    assert_eq!(settings.timing.effective_overlap_preservation_rate(), 50);

    let text = toml::to_string_pretty(&settings).expect("settings serialize");
    assert!(text.contains("socd_transition_min_ms = 1.9"));
    let restored: Settings = toml::from_str(&text).expect("settings deserialize");

    assert_eq!(restored, settings);
}

#[test]
fn unrecognized_alias_fields_are_ignored_instead_of_migrated() {
    let legacy = r#"
transition_min_ms = 15
overlap_probability = 35
full_overlap = true
"#;

    let timing: TimingSettings = toml::from_str(legacy).expect("legacy timing settings");

    assert_eq!(timing, TimingSettings::default());
}

#[test]
fn explicit_zero_transition_is_preserved_from_file() {
    let explicit = r#"
socd_transition_delay_enabled = false
socd_transition_min_ms = 0
socd_transition_max_ms = 0
"#;

    let timing: TimingSettings = toml::from_str(explicit).expect("explicit zero timing");

    assert_eq!(timing.socd_transition_min_micros, 0);
    assert_eq!(timing.socd_transition_max_micros, 0);
}

#[test]
fn timing_precision_below_one_tenth_millisecond_is_rejected() {
    let mut settings = Settings::default();
    settings.timing.socd_transition_min_micros = 100;
    settings.timing.socd_transition_max_micros = 150;

    assert!(matches!(
        settings.validate(),
        Err(SettingsError::InvalidTimingPrecision)
    ));
}

#[test]
fn explicit_zero_transition_survives_ipc_round_trip() {
    let mut settings = Settings::default();
    settings.timing.socd_transition_delay_enabled = false;
    settings.timing.socd_transition_min_micros = 0;
    settings.timing.socd_transition_max_micros = 0;
    assert!(settings.validate().is_ok());

    let json = serde_json::to_string(&settings).expect("settings serialize");
    let restored: Settings = serde_json::from_str(&json).expect("settings deserialize");

    assert_eq!(restored.timing.socd_transition_min_micros, 0);
    assert_eq!(restored.timing.socd_transition_max_micros, 0);
    assert!(!restored.timing.socd_transition_delay_enabled);
}

#[test]
fn timing_values_above_one_second_are_rejected() {
    let mut settings = Settings::default();
    settings.timing.preserved_overlap_max_micros = 2_000_000;

    assert!(matches!(
        settings.validate(),
        Err(SettingsError::InvalidTimingMaximum)
    ));

    settings.timing.preserved_overlap_max_micros = 1_000_000;
    assert!(settings.validate().is_ok());
}
