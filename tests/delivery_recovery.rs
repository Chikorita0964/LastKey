use std::time::Instant;

use lastkey::{
    core::{
        DeliveryState, EventDisposition, KeyAction, LogicalKey, OutputEmitter, TimingController,
    },
    settings::TimingSettings,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OutputAttempt(LogicalKey, KeyAction);

struct TestEmitter {
    results: Vec<bool>,
    attempts: Vec<OutputAttempt>,
}

impl TestEmitter {
    fn new(results: impl Into<Vec<bool>>) -> Self {
        Self {
            results: results.into(),
            attempts: Vec::new(),
        }
    }
}

impl OutputEmitter for TestEmitter {
    fn emit(&mut self, key: LogicalKey, action: KeyAction) -> bool {
        self.attempts.push(OutputAttempt(key, action));
        self.results.remove(0)
    }
}

fn disabled_controller() -> TimingController {
    TimingController::new(TimingSettings {
        socd_transition_delay_enabled: false,
        ..Default::default()
    })
}

fn process(
    controller: &mut TimingController,
    emitter: &mut TestEmitter,
    key: LogicalKey,
    action: KeyAction,
) -> EventDisposition {
    controller.process(key, action, Instant::now(), emitter)
}

#[test]
fn last_input_wins_and_releasing_it_restores_the_opposing_key() {
    let mut controller = disabled_controller();
    let mut emitter = TestEmitter::new([true, true, true, true, true]);

    assert_eq!(
        process(
            &mut controller,
            &mut emitter,
            LogicalKey::HorizontalFirst,
            KeyAction::Down
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        process(
            &mut controller,
            &mut emitter,
            LogicalKey::HorizontalSecond,
            KeyAction::Down
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        process(
            &mut controller,
            &mut emitter,
            LogicalKey::HorizontalSecond,
            KeyAction::Up
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        emitter.attempts,
        [
            OutputAttempt(LogicalKey::HorizontalFirst, KeyAction::Down),
            OutputAttempt(LogicalKey::HorizontalFirst, KeyAction::Up),
            OutputAttempt(LogicalKey::HorizontalSecond, KeyAction::Down),
            OutputAttempt(LogicalKey::HorizontalSecond, KeyAction::Up),
            OutputAttempt(LogicalKey::HorizontalFirst, KeyAction::Down),
        ]
    );
    assert_eq!(
        controller.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::SyntheticHeld
    );
}

#[test]
fn axes_are_independent() {
    let mut controller = disabled_controller();
    let mut emitter = TestEmitter::new([true, true]);

    process(
        &mut controller,
        &mut emitter,
        LogicalKey::VerticalFirst,
        KeyAction::Down,
    );
    process(
        &mut controller,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );

    assert_eq!(
        controller.output_state(LogicalKey::VerticalFirst),
        DeliveryState::SyntheticHeld
    );
    assert_eq!(
        controller.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::SyntheticHeld
    );
}

#[test]
fn failed_initial_down_falls_back_to_the_physical_event() {
    let mut controller = disabled_controller();
    let mut emitter = TestEmitter::new([false, false]);

    assert_eq!(
        process(
            &mut controller,
            &mut emitter,
            LogicalKey::HorizontalFirst,
            KeyAction::Down
        ),
        EventDisposition::PassThrough
    );
    assert_eq!(
        controller.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::PhysicalPassThroughHeld
    );
    assert_eq!(
        process(
            &mut controller,
            &mut emitter,
            LogicalKey::HorizontalFirst,
            KeyAction::Up
        ),
        EventDisposition::PassThrough
    );
    assert_eq!(
        controller.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::NotHeld
    );
}

#[test]
fn failed_release_blocks_the_opposing_down() {
    let mut controller = disabled_controller();
    let mut emitter = TestEmitter::new([true, false]);

    process(
        &mut controller,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );
    assert_eq!(
        process(
            &mut controller,
            &mut emitter,
            LogicalKey::HorizontalSecond,
            KeyAction::Down
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        controller.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::SyntheticHeld
    );
    assert_eq!(
        controller.output_state(LogicalKey::HorizontalSecond),
        DeliveryState::NotHeld
    );
}

#[test]
fn failed_down_after_a_successful_release_stays_neutral() {
    let mut controller = disabled_controller();
    let mut emitter = TestEmitter::new([true, true, false]);

    process(
        &mut controller,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );
    assert_eq!(
        process(
            &mut controller,
            &mut emitter,
            LogicalKey::HorizontalSecond,
            KeyAction::Down
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        controller.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::NotHeld
    );
    assert_eq!(
        controller.output_state(LogicalKey::HorizontalSecond),
        DeliveryState::NotHeld
    );
}

#[test]
fn repeated_down_does_not_change_priority_or_emit_again() {
    let mut controller = disabled_controller();
    let mut emitter = TestEmitter::new([true, true, true]);

    process(
        &mut controller,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );
    process(
        &mut controller,
        &mut emitter,
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
    );
    assert_eq!(
        process(
            &mut controller,
            &mut emitter,
            LogicalKey::HorizontalFirst,
            KeyAction::Down
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        controller.output_state(LogicalKey::HorizontalSecond),
        DeliveryState::SyntheticHeld
    );
    assert_eq!(emitter.attempts.len(), 3);
}

#[test]
fn untracked_key_up_passes_through() {
    let mut controller = disabled_controller();
    let mut emitter = TestEmitter::new([]);

    assert_eq!(
        process(
            &mut controller,
            &mut emitter,
            LogicalKey::HorizontalFirst,
            KeyAction::Up
        ),
        EventDisposition::PassThrough
    );
    assert!(emitter.attempts.is_empty());
}

#[test]
fn shutdown_releases_all_held_output() {
    let mut controller = disabled_controller();
    let mut emitter = TestEmitter::new([true, true, true, true]);

    process(
        &mut controller,
        &mut emitter,
        LogicalKey::VerticalFirst,
        KeyAction::Down,
    );
    process(
        &mut controller,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );
    controller.release_all(&mut emitter);

    assert_eq!(
        emitter.attempts,
        [
            OutputAttempt(LogicalKey::VerticalFirst, KeyAction::Down),
            OutputAttempt(LogicalKey::HorizontalFirst, KeyAction::Down),
            OutputAttempt(LogicalKey::VerticalFirst, KeyAction::Up),
            OutputAttempt(LogicalKey::HorizontalFirst, KeyAction::Up),
        ]
    );
}

#[test]
fn reset_state_clears_output_and_physical_state() {
    let mut controller = disabled_controller();
    let mut emitter = TestEmitter::new([true, true]);

    process(
        &mut controller,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );
    controller.reset_state(&mut emitter);

    assert_eq!(
        emitter.attempts,
        [
            OutputAttempt(LogicalKey::HorizontalFirst, KeyAction::Down),
            OutputAttempt(LogicalKey::HorizontalFirst, KeyAction::Up),
        ]
    );
    assert_eq!(
        process(
            &mut controller,
            &mut emitter,
            LogicalKey::HorizontalFirst,
            KeyAction::Up,
        ),
        EventDisposition::PassThrough
    );
}
