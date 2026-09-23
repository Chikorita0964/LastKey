//! The `update` and pure-helper tests. The widget-tree assertions belong to
//! the view layer and live there as `egui_kittest` tests (T8).

use crate::{
    core::PhysicalKey,
    protocol::{DisplayKey, UiCommand, UiEvent, UiSnapshot, UiView},
    settings::{Settings, SocdMode, TimingSettings},
};

use super::{
    message::{KeyPress, Message, TimingField, timing_pair_invalid},
    state::{
        Effect, INVALID_TIMING_TEXT, State, TimingInputs, format_ms, format_rate, millis_to_micros,
        parse_ms_text, parse_press_rate_text, parse_rate_text, requested_view_from, update,
    },
};

fn baseline_snapshot() -> UiSnapshot {
    let keys = std::array::from_fn(|_| DisplayKey {
        physical: PhysicalKey::new(0x11, false),
        name: "W".into(),
    });
    UiSnapshot {
        filter_enabled: true,
        saved: Settings::default(),
        draft: Settings::default(),
        keys,
        capture_slot: None,
        measurement_active: false,
        measurement: None,
    }
}

/// A state already synchronized with the runtime, which is where most
/// interaction tests start.
fn test_state() -> State {
    let mut state = State::default();
    let _ = update(
        &mut state,
        Message::Ipc(super::message::IpcEvent::Message(Box::new(
            UiEvent::Snapshot(baseline_snapshot()),
        ))),
    );
    state
}

fn press(character: &str, physical: &str) -> KeyPress {
    KeyPress {
        character: Some(character.into()),
        physical: Some(physical.into()),
    }
}

#[test]
fn slider_values_are_rounded_to_tenth_milliseconds() {
    assert_eq!(millis_to_micros(1.94), 1_900);
    assert_eq!(millis_to_micros(1.95), 2_000);
    assert_eq!(millis_to_micros(1_001.0), 1_000_000);
}

#[test]
fn view_argument_requires_the_named_flag_and_value() {
    assert_eq!(
        requested_view_from(["lastkey-settings", "--view", "measurement"]),
        UiView::Measurement
    );
    assert_eq!(
        requested_view_from(["lastkey-settings", "measurement"]),
        UiView::Settings
    );
    assert_eq!(
        requested_view_from(["lastkey-settings", "--view", "unknown"]),
        UiView::Settings
    );
}

#[test]
fn typed_millisecond_values_round_and_clamp_like_sliders() {
    assert_eq!(parse_ms_text("2.0"), Some(2_000));
    assert_eq!(parse_ms_text("1.95"), Some(2_000));
    assert_eq!(parse_ms_text(""), None);
    assert_eq!(parse_ms_text("abc"), None);
    assert_eq!(parse_ms_text("1.2.3"), None);
    assert_eq!(parse_ms_text("2000"), Some(1_000_000));
    assert_eq!(format_ms(2_000), "2.0");
}

#[test]
fn typed_preservation_rates_clamp_to_whole_percent() {
    assert_eq!(parse_rate_text("50"), Some(50));
    assert_eq!(parse_rate_text("49.6"), Some(50));
    assert_eq!(parse_rate_text("0"), Some(1));
    assert_eq!(parse_rate_text("1"), Some(1));
    assert_eq!(parse_rate_text("99"), Some(99));
    assert_eq!(parse_rate_text("100"), Some(99));
    assert_eq!(parse_rate_text("101"), Some(99));
    assert_eq!(parse_rate_text(""), None);
    assert_eq!(format_rate(50), "50");
}

#[test]
fn both_ratio_boxes_share_the_rate_band() {
    // The press box edits the same 1..=99 band, so it shares the parser; the
    // complement is applied by the commit.
    for input in ["0", "1", "99", "100", "101", ""] {
        assert_eq!(
            parse_press_rate_text(input),
            parse_rate_text(input),
            "{input}"
        );
    }
}

#[test]
fn apply_stops_before_ipc_when_typed_text_is_invalid() {
    let mut state = State {
        draft: Some(Settings::default()),
        inputs: TimingInputs::from_timing(&Settings::default().timing),
        ..State::default()
    };
    state.inputs.transition_minimum = "abc".into();

    let effects = update(&mut state, Message::Apply);

    assert_eq!(state.error.as_deref(), Some(INVALID_TIMING_TEXT));
    assert_eq!(state.inputs.transition_minimum, "2.0");
    assert!(
        effects.is_empty(),
        "no command may leave while the gate is closed"
    );
}

#[test]
fn apply_is_blocked_by_local_validation_before_any_ipc() {
    let mut invalid = Settings::default();
    invalid.timing.socd_transition_min_micros = 4_000;
    invalid.timing.socd_transition_max_micros = 2_000;
    let mut state = State {
        inputs: TimingInputs::from_timing(&invalid.timing),
        draft: Some(invalid),
        ..State::default()
    };

    let effects = update(&mut state, Message::Apply);

    assert_eq!(
        state.error.as_deref(),
        Some("a timing minimum cannot exceed its maximum")
    );
    assert!(
        effects.is_empty(),
        "no command may leave while the gate is closed"
    );
}

#[test]
fn apply_sends_the_draft_then_the_command() {
    let mut state = test_state();
    let effects = update(&mut state, Message::Apply);
    let draft = state.draft.clone().expect("draft is kept");
    assert_eq!(
        effects,
        vec![
            Effect::Send(UiCommand::UpdateDraft(draft)),
            Effect::Send(UiCommand::Apply),
        ]
    );
}

#[test]
fn successful_submit_preserves_an_unrelated_server_error() {
    let mut state = test_state();
    state.error = Some("boom".into());

    let _ = update(
        &mut state,
        Message::TimingTextSubmitted(TimingField::TransitionMinimum),
    );
    assert_eq!(state.error.as_deref(), Some("boom"));

    // A locally produced parse error still clears on the next success.
    state.inputs.transition_minimum = "abc".into();
    let _ = update(
        &mut state,
        Message::TimingTextSubmitted(TimingField::TransitionMinimum),
    );
    assert_eq!(state.error.as_deref(), Some(INVALID_TIMING_TEXT));
    let _ = update(
        &mut state,
        Message::TimingTextSubmitted(TimingField::TransitionMinimum),
    );
    assert_eq!(state.error, None);
}

#[test]
fn drags_for_a_hidden_group_leave_the_draft_untouched() {
    // A drag queued before the mode switch arrives after its slider is
    // unmounted; the update arm must drop it so the hidden value stays put.
    let mut state = test_state();
    let _ = update(&mut state, Message::ModeSelected(SocdMode::PressDelay));
    let _ = update(
        &mut state,
        Message::TimingSliderChanged(TimingField::TransitionMinimum, 9.0),
    );
    assert_eq!(
        state
            .draft
            .as_ref()
            .expect("draft is kept")
            .timing
            .socd_transition_min_micros,
        9_000
    );

    let _ = update(&mut state, Message::ModeSelected(SocdMode::Immediate));
    let _ = update(
        &mut state,
        Message::TimingSliderChanged(TimingField::TransitionMinimum, 3.0),
    );
    let draft = state.draft.as_ref().expect("draft is kept");
    assert_eq!(draft.timing.socd_transition_min_micros, 9_000);
    assert_eq!(state.inputs.transition_minimum, "9.0");
}

#[test]
fn inflight_snapshot_preserves_newer_local_timing_edit() {
    let mut state = test_state();
    // A syncing request went out with the baseline values; before its reply
    // arrives, the user flips a timing control.
    let _ = update(&mut state, Message::ModeSelected(SocdMode::PressDelay));
    let _ = update(
        &mut state,
        Message::Ipc(super::message::IpcEvent::Message(Box::new(
            UiEvent::Snapshot(baseline_snapshot()),
        ))),
    );
    assert_eq!(
        state.draft.as_ref().expect("draft is kept").timing.mode,
        SocdMode::PressDelay
    );
}

#[test]
fn revert_resets_the_draft_and_buffers_at_click_time() {
    let mut state = test_state();
    let _ = update(&mut state, Message::ModeSelected(SocdMode::PressDelay));

    let effects = update(&mut state, Message::Revert);

    let draft = state.draft.as_ref().expect("draft is kept");
    assert_eq!(draft.timing.mode, SocdMode::Immediate);
    assert_eq!(state.inputs, TimingInputs::from_timing(&draft.timing));
    assert_eq!(effects, vec![Effect::Send(UiCommand::Revert)]);
}

#[test]
fn older_snapshot_after_revert_leaves_reverted_values_in_place() {
    let mut state = test_state();
    let _ = update(&mut state, Message::ModeSelected(SocdMode::PressDelay));
    let _ = update(&mut state, Message::Revert);
    // A stale reply to an earlier request arrives after the revert.
    let mut stale = baseline_snapshot();
    stale.draft.timing.mode = SocdMode::PressDelay;
    let _ = update(
        &mut state,
        Message::Ipc(super::message::IpcEvent::Message(Box::new(
            UiEvent::Snapshot(stale),
        ))),
    );
    assert_eq!(
        state.draft.as_ref().expect("draft is kept").timing.mode,
        SocdMode::Immediate
    );
}

#[test]
fn stale_measurement_update_does_not_revive_a_stopped_session() {
    use crate::protocol::MeasurementSnapshot;
    let mut state = State {
        snapshot: Some(baseline_snapshot()),
        ..State::default()
    };
    let measurement = MeasurementSnapshot {
        observed_event_count: 9,
        ..MeasurementSnapshot::default()
    };
    let _ = update(
        &mut state,
        Message::Ipc(super::message::IpcEvent::Message(Box::new(
            UiEvent::MeasurementUpdated(measurement),
        ))),
    );

    let snapshot = state.snapshot.as_ref().expect("snapshot is kept");
    assert!(!snapshot.measurement_active);
    assert!(snapshot.measurement.is_none());

    state
        .snapshot
        .as_mut()
        .expect("snapshot is kept")
        .measurement_active = true;
    let _ = update(
        &mut state,
        Message::Ipc(super::message::IpcEvent::Message(Box::new(
            UiEvent::MeasurementUpdated(measurement),
        ))),
    );
    let snapshot = state.snapshot.as_ref().expect("snapshot is kept");
    assert!(snapshot.measurement_active);
    assert_eq!(snapshot.measurement, Some(measurement));
}

#[test]
fn uncommitted_text_counts_as_dirty() {
    let timing = TimingSettings::default();
    let mut inputs = TimingInputs::from_timing(&timing);

    assert_eq!(inputs, TimingInputs::from_timing(&timing));
    inputs.transition_minimum = "9.9".into();
    assert_ne!(inputs, TimingInputs::from_timing(&timing));
}

#[test]
fn input_buffers_round_trip_through_draft_precision() {
    let timing = TimingSettings {
        socd_transition_min_micros: 1_900,
        overlap_preservation_rate: 35,
        ..TimingSettings::default()
    };
    let inputs = TimingInputs::from_timing(&timing);

    assert_eq!(inputs.transition_minimum, "1.9");
    assert_eq!(inputs.preservation_rate, "35");
}

#[test]
fn restore_timing_defaults_resets_only_timing() {
    let mut state = test_state();
    state.draft.as_mut().expect("draft is kept").timing.mode = SocdMode::PressDelay;
    let _ = update(
        &mut state,
        Message::TimingSliderChanged(TimingField::TransitionMinimum, 9.0),
    );
    let _ = update(&mut state, Message::RestoreTimingDefaults);

    let draft = state.draft.as_ref().expect("draft is kept");
    assert_eq!(draft.timing, TimingSettings::default());
    assert_eq!(draft.bindings, Settings::default().bindings);
    assert_eq!(
        state.inputs,
        TimingInputs::from_timing(&TimingSettings::default())
    );
}

#[test]
fn an_out_of_range_mix_ratio_clamps_without_leaving_the_mode() {
    // "Off" is a mode of its own now, so a zero in the ratio box no longer
    // flips a hidden switch: it clamps to the lowest usable share and Random
    // Mix stays selected.
    let mut state = test_state();
    state.draft.as_mut().expect("draft is kept").timing.mode = SocdMode::RandomMix;
    state.inputs.preservation_rate = "0".into();
    let _ = update(&mut state, Message::Apply);

    let draft = state.draft.as_ref().expect("draft is kept");
    assert_eq!(draft.timing.mode, SocdMode::RandomMix);
    assert_eq!(draft.timing.overlap_preservation_rate, 1);
    assert_eq!(state.inputs.preservation_rate, "1");
    assert_eq!(state.inputs.press_rate, "99");

    // The old ceiling is out of range now, the mirror of the zero case: 100
    // clamps to the highest usable share and the press box takes the mirror.
    state.inputs.preservation_rate = "100".into();
    let _ = update(&mut state, Message::Apply);

    let draft = state.draft.as_ref().expect("draft is kept");
    assert_eq!(draft.timing.mode, SocdMode::RandomMix);
    assert_eq!(draft.timing.overlap_preservation_rate, 99);
    assert_eq!(state.inputs.preservation_rate, "99");
    assert_eq!(state.inputs.press_rate, "1");
}

#[test]
fn recommendations_open_settings_with_results_in_the_draft() {
    use crate::protocol::{MeasurementSnapshot, TimingRange};
    let mut state = test_state();
    state
        .snapshot
        .as_mut()
        .expect("snapshot is kept")
        .measurement = Some(MeasurementSnapshot {
        recommended_transition: Some(TimingRange {
            min_micros: 2_100,
            max_micros: 3_000,
        }),
        ..MeasurementSnapshot::default()
    });
    let effects = update(&mut state, Message::ApplyRecommendations);

    let draft = state.draft.as_ref().expect("draft is kept");
    assert_eq!(draft.timing.socd_transition_min_micros, 2_100);
    assert_eq!(draft.timing.socd_transition_max_micros, 3_000);
    assert!(state.notice.is_some());
    assert_eq!(
        effects,
        vec![Effect::ShowSection {
            view: UiView::Settings,
            focus: false,
        }]
    );
}

#[test]
fn recommendations_without_samples_only_report() {
    let mut state = test_state();
    let effects = update(&mut state, Message::ApplyRecommendations);
    assert!(effects.is_empty());
    assert!(state.status.contains("samples"));
}

#[test]
fn measurement_updates_keep_apply_feedback() {
    use crate::protocol::MeasurementSnapshot;
    let mut state = test_state();
    let mut snapshot = baseline_snapshot();
    snapshot.measurement_active = true;
    snapshot.measurement = Some(MeasurementSnapshot::default());
    let _ = update(
        &mut state,
        Message::Ipc(super::message::IpcEvent::Message(Box::new(
            UiEvent::ApplySucceeded(snapshot),
        ))),
    );
    assert_eq!(state.error, None);
    assert!(state.notice.is_some());
    let _ = update(
        &mut state,
        Message::Ipc(super::message::IpcEvent::Message(Box::new(
            UiEvent::MeasurementUpdated(MeasurementSnapshot::default()),
        ))),
    );
    assert_eq!(state.notice.as_deref(), Some("Settings applied."));
}

#[test]
fn a_monitor_start_failure_stops_the_monitor_and_asks_the_runtime_to_stop() {
    use crate::protocol::ErrorView;
    let mut state = test_state();
    let effects = update(
        &mut state,
        Message::Ipc(super::message::IpcEvent::Message(Box::new(
            UiEvent::RuntimeError(ErrorView {
                code: "monitor-start-failed".into(),
                message: "raw input registration failed".into(),
                recoverable: true,
            }),
        ))),
    );
    assert_eq!(
        state.error.as_deref(),
        Some("raw input registration failed")
    );
    assert_eq!(effects, vec![Effect::Send(UiCommand::StopMonitor)]);
}

#[test]
fn timing_field_indices_match_all_order() {
    for (index, field) in TimingField::ALL.iter().enumerate() {
        assert_eq!(field.index(), index);
    }
}

#[test]
fn timing_pair_flags_minimum_above_maximum() {
    assert!(timing_pair_invalid(4_000, 2_000));
    assert!(!timing_pair_invalid(2_000, 2_000));
    assert!(!timing_pair_invalid(2_000, 4_000));
}

#[test]
fn value_box_activation_marks_editing() {
    let mut state = test_state();
    assert!(!state.editing[TimingField::TransitionMinimum.index()]);

    let effects = update(
        &mut state,
        Message::ValueBoxActivated(TimingField::TransitionMinimum),
    );

    assert!(state.editing[TimingField::TransitionMinimum.index()]);
    assert!(!state.editing[TimingField::PreservationRate.index()]);
    assert_eq!(
        effects,
        vec![Effect::FocusValueBox(TimingField::TransitionMinimum)]
    );
}

#[test]
fn focus_moves_rearm_value_boxes() {
    let mut state = test_state();
    let _ = update(
        &mut state,
        Message::ValueBoxActivated(TimingField::TransitionMinimum),
    );
    assert!(state.editing[TimingField::TransitionMinimum.index()]);

    // Pressing another control moves focus away: the next press on any box
    // selects all again.
    let _ = update(&mut state, Message::ModeSelected(SocdMode::PressDelay));
    assert!(!state.editing[TimingField::TransitionMinimum.index()]);

    // Losing the window rearms as well.
    let _ = update(
        &mut state,
        Message::ValueBoxActivated(TimingField::TransitionMinimum),
    );
    let _ = update(&mut state, Message::WindowUnfocused);
    assert!(!state.editing[TimingField::TransitionMinimum.index()]);

    // Typing and scrolling leave the armed box alone.
    let _ = update(
        &mut state,
        Message::ValueBoxActivated(TimingField::TransitionMinimum),
    );
    let _ = update(
        &mut state,
        Message::TimingTextChanged(TimingField::TransitionMinimum, "9.9".into()),
    );
    assert!(state.editing[TimingField::TransitionMinimum.index()]);
}

#[test]
fn explicit_profile_load_replaces_local_edits_only_on_confirmed_success() {
    let mut state = test_state();
    let initial = baseline_snapshot();
    let _ = update(&mut state, Message::ModeSelected(SocdMode::PressDelay));
    let _ = update(&mut state, Message::LoadProfile(2));
    assert_eq!(state.snapshot.as_ref().unwrap().saved, initial.saved);
    assert_eq!(
        state.draft.as_ref().unwrap().timing.mode,
        SocdMode::PressDelay
    );
    let mut loaded = initial;
    loaded.saved = loaded.saved.select_profile(2).unwrap();
    loaded.draft = loaded.saved.clone();
    let expected = loaded.saved.clone();
    let _ = update(
        &mut state,
        Message::Ipc(super::message::IpcEvent::Message(Box::new(
            UiEvent::ProfileLoaded(loaded),
        ))),
    );
    assert_eq!(state.draft.as_ref(), Some(&expected));
    assert!(!state.is_dirty());
}

#[test]
fn a_dirty_draft_asks_before_loading_a_profile() {
    let mut state = test_state();
    let _ = update(&mut state, Message::ModeSelected(SocdMode::PressDelay));
    let effects = update(&mut state, Message::LoadProfile(2));
    assert!(effects.is_empty(), "the confirm banner opens first");
    assert!(matches!(
        state.profiles,
        super::state::ProfileDialog::Confirm(2)
    ));

    let effects = update(&mut state, Message::ConfirmProfile(2));
    assert_eq!(effects, vec![Effect::Send(UiCommand::LoadProfile(2))]);
}

#[test]
fn stopped_timeline_rejects_late_updates_and_filter_ack_clears_held_keys() {
    use crate::protocol::{KeySlot, MonitorDecision, MonitorEdge, MonitorSnapshot};
    let mut state = test_state();
    let event = MonitorSnapshot {
        elapsed_micros: 0,
        filter_enabled: true,
        physical: None,
        outputs: vec![MonitorEdge {
            key: KeySlot::HorizontalFirst,
            pressed: true,
            synthetic: true,
        }],
        decision: MonitorDecision::Immediate,
    };
    let monitor = |event: UiEvent| Message::Ipc(super::message::IpcEvent::Message(Box::new(event)));
    let _ = update(&mut state, monitor(UiEvent::MonitorStateChanged(true)));
    let _ = update(&mut state, monitor(UiEvent::MonitorUpdated(event.clone())));
    assert!(
        state
            .monitor
            .timeline()
            .unwrap()
            .held(KeySlot::HorizontalFirst)
    );
    let _ = update(&mut state, monitor(UiEvent::FilterChanged(false)));
    assert!(
        !state
            .monitor
            .timeline()
            .unwrap()
            .held(KeySlot::HorizontalFirst)
    );
    let _ = update(&mut state, monitor(UiEvent::MonitorStateChanged(false)));
    let _ = update(&mut state, monitor(UiEvent::MonitorUpdated(event)));
    assert!(state.monitor.timeline().is_none());
}

#[test]
fn keyboard_press_and_release_updates_state() {
    let mut state = test_state();
    assert!(!state.pressed_keys[0]);

    // Press "W" (default UP key)
    let _ = update(&mut state, Message::KeyboardPressed(press("w", "KeyW")));
    assert!(state.pressed_keys[0]);
    assert!(state.press_timestamps[0].is_some());

    let _ = update(&mut state, Message::KeyboardReleased(press("w", "KeyW")));
    assert!(!state.pressed_keys[0]);
    assert!(state.press_timestamps[0].is_none());
}

#[test]
fn arrow_and_space_keys_match_their_display_names() {
    let mut state = test_state();
    for key in state.snapshot.as_mut().unwrap().keys.iter_mut() {
        key.name = "Up Arrow".into();
    }
    let _ = update(
        &mut state,
        Message::KeyboardPressed(KeyPress {
            character: None,
            physical: Some("ArrowUp".into()),
        }),
    );
    assert!(state.pressed_keys[0]);

    for key in state.snapshot.as_mut().unwrap().keys.iter_mut() {
        key.name = "Space".into();
    }
    let _ = update(
        &mut state,
        Message::KeyboardReleased(KeyPress {
            character: None,
            physical: Some("ArrowUp".into()),
        }),
    );
    let _ = update(
        &mut state,
        Message::KeyboardPressed(KeyPress {
            character: None,
            physical: Some("Space".into()),
        }),
    );
    assert!(state.pressed_keys[0]);
}

#[test]
fn an_open_dialog_swallows_messages_that_are_not_its_own() {
    let mut state = test_state();
    let _ = update(&mut state, Message::OpenProfiles);
    let effects = update(&mut state, Message::Apply);
    assert!(effects.is_empty(), "the panel gates unrelated messages");
}

#[test]
fn a_disconnected_runtime_refuses_the_filter_toggle() {
    let mut state = test_state();
    assert!(!state.connected);
    let effects = update(&mut state, Message::ToggleFilter);
    assert!(effects.is_empty());
    assert_eq!(state.pending_filter, None);
}

#[test]
fn a_shutting_down_runtime_closes_the_window() {
    let mut state = test_state();
    let effects = update(
        &mut state,
        Message::Ipc(super::message::IpcEvent::Message(Box::new(
            UiEvent::RuntimeShuttingDown,
        ))),
    );
    assert_eq!(effects, vec![Effect::Close]);
    assert!(!state.connected);
}
