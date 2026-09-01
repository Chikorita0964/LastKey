use lastkey::{
    core::PhysicalKey,
    settings::{Settings, SettingsError},
};

#[test]
fn default_settings_are_valid() {
    assert!(Settings::default().validate().is_ok());
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
    settings.timing.transition_min_ms = 2;
    settings.timing.transition_max_ms = 1;
    assert!(matches!(
        settings.validate(),
        Err(SettingsError::InvalidTimingRange)
    ));

    settings.timing.transition_min_ms = 0;
    settings.timing.transition_max_ms = 0;
    settings.timing.overlap_probability = 101;
    assert!(matches!(
        settings.validate(),
        Err(SettingsError::InvalidOverlapProbability)
    ));
}
