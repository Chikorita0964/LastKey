use lastkey::{
    core::PhysicalKey,
    settings::{Settings, SettingsError, SocdMode, TimingSettings},
};

#[test]
fn default_settings_are_valid() {
    let settings = Settings::default();

    assert!(settings.validate().is_ok());
    assert_eq!(settings.timing.mode, SocdMode::Immediate);
    assert_eq!(settings.timing.socd_transition_min_micros, 2_000);
    assert_eq!(settings.timing.socd_transition_max_micros, 4_000);
    assert_eq!(settings.timing.overlap_preservation_rate, 50);
    assert_eq!(settings.timing.preserved_overlap_min_micros, 2_000);
    assert_eq!(settings.timing.preserved_overlap_max_micros, 6_000);
    assert_eq!(settings.timing.release_delay_share(), 0);
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
fn a_release_delay_below_the_duration_floor_is_rejected_in_every_mode() {
    let mut settings = Settings::default();
    settings.timing.preserved_overlap_min_micros = 0;

    // The floor is unconditional, so a stored value cannot turn invalid later
    // by switching to the mode that uses it.
    for mode in SocdMode::ALL {
        settings.timing.mode = mode;
        assert!(matches!(
            settings.validate(),
            Err(SettingsError::InvalidPreservedOverlapDuration)
        ));
    }

    settings.timing.preserved_overlap_min_micros = 100;
    assert!(settings.validate().is_ok());
}

#[test]
fn every_mode_round_trips_through_toml() {
    for mode in SocdMode::ALL {
        let mut settings = Settings::default();
        settings.timing.mode = mode;

        let text = toml::to_string_pretty(&settings).expect("settings serialize");
        let restored: Settings = toml::from_str(&text).expect("settings deserialize");

        assert_eq!(restored, settings);
    }
}

#[test]
fn each_mode_answers_the_release_delay_share_on_its_own() {
    let mut timing = TimingSettings {
        overlap_preservation_rate: 35,
        ..TimingSettings::default()
    };

    let shares = SocdMode::ALL.map(|mode| {
        timing.mode = mode;
        timing.release_delay_share()
    });

    // Ordered as SocdMode::ALL: Immediate, PressDelay, RandomMix, ReleaseDelay.
    assert_eq!(shares, [0, 0, 35, 100]);
}

#[test]
fn pre_mode_files_migrate_to_the_behavior_that_shipped() {
    // v1.0.0 through v1.0.2 stored a master switch plus a sub-switch. Overlap
    // preservation with the master switch off never reached the engine, so it
    // migrates to Immediate rather than to the mode it looks like.
    let cases = [
        (
            r#"
socd_transition_delay_enabled = true
preserve_overlap = true
overlap_preservation_rate = 100
"#,
            SocdMode::ReleaseDelay,
        ),
        (
            r#"
socd_transition_delay_enabled = true
preserve_overlap = true
overlap_preservation_rate = 40
"#,
            SocdMode::RandomMix,
        ),
        (
            r#"
socd_transition_delay_enabled = true
preserve_overlap = false
"#,
            SocdMode::PressDelay,
        ),
        (
            r#"
socd_transition_delay_enabled = false
preserve_overlap = true
"#,
            SocdMode::Immediate,
        ),
        ("", SocdMode::Immediate),
    ];

    for (stored, expected) in cases {
        let timing: TimingSettings = toml::from_str(stored).expect("stored timing settings");
        assert_eq!(timing.mode, expected, "stored: {stored}");
    }
}

#[test]
fn a_saved_file_drops_the_pre_mode_switches() {
    let mut settings = Settings::default();
    settings.timing.mode = SocdMode::RandomMix;

    let text = toml::to_string_pretty(&settings).expect("settings serialize");

    assert!(text.contains("mode = \"RandomMix\""));
    assert!(!text.contains("socd_transition_delay_enabled"));
    assert!(!text.contains("preserve_overlap"));
}

#[test]
fn decimal_millisecond_settings_round_trip_at_tenth_millisecond_precision() {
    let mut settings = Settings::default();
    settings.timing.socd_transition_min_micros = 1_900;
    settings.timing.socd_transition_max_micros = 4_000;
    settings.timing.mode = SocdMode::RandomMix;
    settings.timing.overlap_preservation_rate = 50;
    settings.timing.preserved_overlap_min_micros = 2_000;
    settings.timing.preserved_overlap_max_micros = 6_000;

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
    settings.timing.mode = SocdMode::Immediate;
    settings.timing.socd_transition_min_micros = 0;
    settings.timing.socd_transition_max_micros = 0;
    assert!(settings.validate().is_ok());

    let json = serde_json::to_string(&settings).expect("settings serialize");
    let restored: Settings = serde_json::from_str(&json).expect("settings deserialize");

    assert_eq!(restored.timing.socd_transition_min_micros, 0);
    assert_eq!(restored.timing.socd_transition_max_micros, 0);
    assert_eq!(restored.timing.mode, SocdMode::Immediate);
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

#[test]
fn legacy_profile_migration_keeps_current_configuration_and_round_trips_all_slots() {
    let mut legacy = Settings::default();
    legacy.bindings[0] = PhysicalKey::new(0x21, false);
    legacy.timing.socd_transition_max_micros = 900_000;
    let migrated = legacy.select_profile(2).expect("factory profile is valid");
    assert_eq!(migrated.timing.mode, SocdMode::RandomMix);
    let restored = migrated.select_profile(0).expect("legacy slot is valid");
    assert_eq!(restored.bindings, legacy.bindings);
    assert_eq!(restored.timing, legacy.timing);
    let encoded = toml::to_string(&migrated).expect("profile bank serializes");
    assert_eq!(
        toml::from_str::<Settings>(&encoded).expect("profile bank loads"),
        migrated
    );
}

#[test]
fn invalid_profile_metadata_and_inactive_slot_settings_are_rejected() {
    let mut settings = Settings::default().select_profile(1).expect("valid slot");
    assert!(settings.select_profile(4).is_err());
    settings.profiles.as_mut().unwrap().slots[3].name = "\n".into();
    assert!(matches!(
        settings.validate(),
        Err(SettingsError::InvalidProfile)
    ));
    settings.profiles.as_mut().unwrap().slots[3].name = "Valid name".into();
    settings.profiles.as_mut().unwrap().slots[3].bindings[0] = settings.bindings[1];
    assert!(matches!(
        settings.validate(),
        Err(SettingsError::DuplicateBinding)
    ));
}
