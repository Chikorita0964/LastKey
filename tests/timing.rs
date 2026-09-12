use std::time::{Duration, Instant};

use lastkey::{
    core::{KeyAction, LogicalKey, MonitorDecision, OutputEmitter, TimingController},
    settings::{SocdMode, TimingSettings},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Attempt(LogicalKey, KeyAction);

#[derive(Default)]
struct Emitter(Vec<Attempt>);

impl OutputEmitter for Emitter {
    fn emit(&mut self, key: LogicalKey, action: KeyAction) -> bool {
        self.0.push(Attempt(key, action));
        true
    }
}

struct FailingEmitter {
    results: Vec<bool>,
    attempts: Vec<Attempt>,
}

impl OutputEmitter for FailingEmitter {
    fn emit(&mut self, key: LogicalKey, action: KeyAction) -> bool {
        self.attempts.push(Attempt(key, action));
        self.results.remove(0)
    }
}

/// Timing settings in whole milliseconds. The mode is passed in rather than
/// derived from the ranges: every mode is independent, so a test states the
/// behavior it exercises instead of implying it.
fn timing(
    mode: SocdMode,
    press_delay: (u32, u32),
    release_delay: (u32, u32),
    mix_ratio: u8,
) -> TimingSettings {
    timing_micros(
        mode,
        (press_delay.0 * 1_000, press_delay.1 * 1_000),
        (release_delay.0 * 1_000, release_delay.1 * 1_000),
        mix_ratio,
    )
}

fn timing_micros(
    mode: SocdMode,
    press_delay: (u32, u32),
    release_delay: (u32, u32),
    mix_ratio: u8,
) -> TimingSettings {
    TimingSettings {
        mode,
        socd_transition_min_micros: press_delay.0,
        socd_transition_max_micros: press_delay.1,
        overlap_preservation_rate: mix_ratio,
        preserved_overlap_min_micros: release_delay.0,
        preserved_overlap_max_micros: release_delay.1,
    }
}

#[test]
fn disabled_timing_uses_the_immediate_path_without_a_deadline() {
    let start = Instant::now();
    let mut controller = TimingController::new(TimingSettings::default());
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    assert_eq!(
        emitter.0,
        [
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Down),
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Up),
            Attempt(LogicalKey::HorizontalSecond, KeyAction::Down)
        ]
    );
    assert_eq!(controller.next_deadline(), None);
}

#[test]
fn immediate_mode_ignores_every_configured_delay() {
    let start = Instant::now();
    let mut controller = TimingController::new(timing(SocdMode::Immediate, (2, 4), (2, 6), 100));
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start,
        &mut emitter,
    );

    assert_eq!(
        emitter.0,
        [
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Down),
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Up),
            Attempt(LogicalKey::HorizontalSecond, KeyAction::Down),
        ]
    );
    assert_eq!(controller.next_deadline(), None);
}

#[test]
fn transition_releases_then_presses_after_the_configured_delay() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::PressDelay, (10, 10), (0, 0), 50), 1);
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.poll(start + Duration::from_millis(9), &mut emitter);
    assert_eq!(emitter.0.len(), 2);
    controller.poll(start + Duration::from_millis(10), &mut emitter);
    assert_eq!(
        emitter.0,
        [
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Down),
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Up),
            Attempt(LogicalKey::HorizontalSecond, KeyAction::Down)
        ]
    );
}

#[test]
fn transition_supports_tenth_millisecond_delays() {
    let start = Instant::now();
    let mut controller = TimingController::with_seed(
        timing_micros(SocdMode::PressDelay, (1_500, 1_500), (0, 0), 50),
        1,
    );
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.poll(start + Duration::from_micros(1_499), &mut emitter);
    assert_eq!(emitter.0.len(), 2);
    controller.poll(start + Duration::from_micros(1_500), &mut emitter);
    assert_eq!(
        emitter.0.last(),
        Some(&Attempt(LogicalKey::HorizontalSecond, KeyAction::Down))
    );
}

#[test]
fn natural_neutral_transitions_are_not_changed() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::PressDelay, (10, 10), (0, 0), 50), 1);
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Up,
        start + Duration::from_millis(1),
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start + Duration::from_millis(5),
        &mut emitter,
    );

    assert_eq!(
        emitter.0,
        [
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Down),
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Up),
            Attempt(LogicalKey::HorizontalSecond, KeyAction::Down),
        ]
    );
    assert_eq!(controller.next_deadline(), None);
}

#[test]
fn press_delay_ignores_the_mix_ratio() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::PressDelay, (4, 4), (20, 20), 100), 1);
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start,
        &mut emitter,
    );

    assert_eq!(
        emitter.0,
        [
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Down),
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Up),
        ]
    );
    controller.poll(start + Duration::from_millis(4), &mut emitter);
    assert_eq!(
        emitter.0.last(),
        Some(&Attempt(LogicalKey::HorizontalSecond, KeyAction::Down))
    );
}

#[test]
fn release_delay_keeps_the_previous_key_held_past_the_new_press() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::ReleaseDelay, (0, 0), (7, 7), 1), 1);
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::VerticalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::VerticalSecond,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    assert_eq!(
        emitter.0,
        [
            Attempt(LogicalKey::VerticalFirst, KeyAction::Down),
            Attempt(LogicalKey::VerticalSecond, KeyAction::Down)
        ]
    );
    controller.poll(start + Duration::from_millis(7), &mut emitter);
    assert_eq!(
        emitter.0.last(),
        Some(&Attempt(LogicalKey::VerticalFirst, KeyAction::Up))
    );
}

#[test]
fn random_mix_draws_both_delays_across_repeated_overlaps() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::RandomMix, (4, 4), (7, 7), 50), 1);
    let mut emitter = Emitter::default();
    let mut press_delayed = 0;
    let mut release_delayed = 0;

    for step in 0..32 {
        // Each round starts from neutral, overlaps once, then flushes the
        // pending half so the next round sees a clean axis.
        let now = start + Duration::from_millis(step * 100);
        controller.process(
            LogicalKey::HorizontalFirst,
            KeyAction::Down,
            now,
            &mut emitter,
        );
        emitter.0.clear();
        controller.process(
            LogicalKey::HorizontalSecond,
            KeyAction::Down,
            now,
            &mut emitter,
        );
        match emitter.0.first().copied() {
            // Press delay drops the previous key now and schedules the press.
            Some(Attempt(LogicalKey::HorizontalFirst, KeyAction::Up)) => press_delayed += 1,
            // Release delay sends the new key now and schedules the release.
            Some(Attempt(LogicalKey::HorizontalSecond, KeyAction::Down)) => release_delayed += 1,
            other => panic!("unexpected overlap output: {other:?}"),
        }
        controller.poll(now + Duration::from_millis(20), &mut emitter);
        for key in [LogicalKey::HorizontalFirst, LogicalKey::HorizontalSecond] {
            controller.process(
                key,
                KeyAction::Up,
                now + Duration::from_millis(30),
                &mut emitter,
            );
        }
        emitter.0.clear();
    }

    assert!(
        press_delayed > 0 && release_delayed > 0,
        "expected both delays, got {press_delayed} press and {release_delayed} release"
    );
}

#[test]
fn a_new_input_cancels_stale_delayed_work_for_its_axis_only() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::PressDelay, (10, 10), (0, 0), 50), 1);
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalSecond,
        KeyAction::Up,
        start + Duration::from_millis(1),
        &mut emitter,
    );
    controller.poll(start + Duration::from_millis(20), &mut emitter);
    assert_eq!(
        emitter.0,
        [
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Down),
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Up),
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Down)
        ]
    );
}

#[test]
fn axes_keep_independent_pending_transitions() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::PressDelay, (5, 5), (0, 0), 50), 1);
    let mut emitter = Emitter::default();
    for key in [
        LogicalKey::VerticalFirst,
        LogicalKey::VerticalSecond,
        LogicalKey::HorizontalFirst,
        LogicalKey::HorizontalSecond,
    ] {
        controller.process(key, KeyAction::Down, start, &mut emitter);
    }
    controller.poll(start + Duration::from_millis(5), &mut emitter);
    assert_eq!(
        emitter.0,
        [
            Attempt(LogicalKey::VerticalFirst, KeyAction::Down),
            Attempt(LogicalKey::VerticalFirst, KeyAction::Up),
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Down),
            Attempt(LogicalKey::HorizontalFirst, KeyAction::Up),
            Attempt(LogicalKey::VerticalSecond, KeyAction::Down),
            Attempt(LogicalKey::HorizontalSecond, KeyAction::Down),
        ]
    );
}

#[test]
fn measurement_boundary_reset_clears_physical_repeat_state() {
    let start = Instant::now();
    let mut controller = TimingController::with_seed(
        TimingSettings {
            mode: SocdMode::Immediate,
            ..TimingSettings::default()
        },
        1,
    );
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    assert_eq!(emitter.0.len(), 1);

    // Repeat without release is consumed without new output.
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start + Duration::from_millis(1),
        &mut emitter,
    );
    assert_eq!(emitter.0.len(), 1);

    // Measurement boundaries must clear physical state plus output.
    controller.reset_state(&mut emitter);
    assert_eq!(
        emitter.0.last(),
        Some(&Attempt(LogicalKey::HorizontalFirst, KeyAction::Up))
    );

    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start + Duration::from_millis(2),
        &mut emitter,
    );
    assert_eq!(
        emitter.0.last(),
        Some(&Attempt(LogicalKey::HorizontalFirst, KeyAction::Down))
    );
}

#[test]
fn failed_overlap_release_attempts_to_restore_a_non_conflicting_output() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::ReleaseDelay, (0, 0), (1, 1), 1), 1);
    let mut emitter = FailingEmitter {
        results: vec![true, true, false, true],
        attempts: Vec::new(),
    };
    controller.process(
        LogicalKey::VerticalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::VerticalSecond,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.poll(start + Duration::from_millis(1), &mut emitter);
    assert_eq!(
        emitter.attempts,
        [
            Attempt(LogicalKey::VerticalFirst, KeyAction::Down),
            Attempt(LogicalKey::VerticalSecond, KeyAction::Down),
            Attempt(LogicalKey::VerticalFirst, KeyAction::Up),
            Attempt(LogicalKey::VerticalSecond, KeyAction::Up),
        ]
    );
}

#[test]
fn last_decision_reports_immediate_for_uncontended_input() {
    let start = Instant::now();
    let mut controller = TimingController::new(TimingSettings::default());
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );

    assert_eq!(controller.last_decision(), MonitorDecision::Immediate);
}

#[test]
fn last_decision_reports_press_delay_with_its_duration() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::PressDelay, (10, 10), (0, 0), 50), 1);
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start,
        &mut emitter,
    );

    // Min and max coincide, so the reported delay is exact and seed-free.
    assert_eq!(
        controller.last_decision(),
        MonitorDecision::PressDelayed {
            delay_micros: 10_000
        }
    );
    controller.poll(start + Duration::from_millis(10), &mut emitter);
    assert_eq!(
        controller.last_decision(),
        MonitorDecision::PressDelayed {
            delay_micros: 10_000
        }
    );
}

#[test]
fn last_decision_reports_release_delay_with_its_duration() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::ReleaseDelay, (0, 0), (7, 7), 100), 1);
    let mut emitter = Emitter::default();
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start,
        &mut emitter,
    );
    controller.process(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start,
        &mut emitter,
    );

    assert_eq!(
        controller.last_decision(),
        MonitorDecision::ReleaseDelayed {
            delay_micros: 7_000
        }
    );
}

#[test]
fn last_decision_is_immediate_for_early_returns() {
    let start = Instant::now();
    let mut controller =
        TimingController::with_seed(timing(SocdMode::PressDelay, (10, 10), (0, 0), 50), 1);
    let mut emitter = Emitter::default();
    // Up on nothing held: passes through with nothing delayed.
    controller.process(
        LogicalKey::HorizontalFirst,
        KeyAction::Up,
        start,
        &mut emitter,
    );

    assert_eq!(controller.last_decision(), MonitorDecision::Immediate);
}
