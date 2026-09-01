use std::time::{Duration, Instant};

use lastkey::{
    core::{KeyAction, LogicalKey, OutputEmitter, TimingController},
    settings::TimingSettings,
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

fn timing(
    transition: (u32, u32),
    overlap: (u32, u32),
    probability: u8,
    full_overlap: bool,
) -> TimingSettings {
    TimingSettings {
        transition_min_ms: transition.0,
        transition_max_ms: transition.1,
        overlap_min_ms: overlap.0,
        overlap_max_ms: overlap.1,
        overlap_probability: probability,
        full_overlap,
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
fn transition_releases_then_presses_after_the_configured_delay() {
    let start = Instant::now();
    let mut controller = TimingController::with_seed(timing((10, 10), (0, 0), 0, false), 1);
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
fn full_overlap_presses_then_releases_after_the_configured_delay() {
    let start = Instant::now();
    let mut controller = TimingController::with_seed(timing((0, 0), (7, 7), 0, true), 1);
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
fn a_new_input_cancels_stale_delayed_work_for_its_axis_only() {
    let start = Instant::now();
    let mut controller = TimingController::with_seed(timing((10, 10), (0, 0), 0, false), 1);
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
    let mut controller = TimingController::with_seed(timing((5, 5), (0, 0), 0, false), 1);
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
fn failed_overlap_release_attempts_to_restore_a_non_conflicting_output() {
    let start = Instant::now();
    let mut controller = TimingController::with_seed(timing((0, 0), (1, 1), 0, true), 1);
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
