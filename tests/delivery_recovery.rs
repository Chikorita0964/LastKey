use lastkey::core::{
    DeliveryState, EventDisposition, InputRouter, KeyAction, LogicalKey, OutputEmitter,
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

fn process(
    router: &mut InputRouter,
    emitter: &mut TestEmitter,
    key: LogicalKey,
    action: KeyAction,
) -> EventDisposition {
    router.process(key, action, emitter)
}

#[test]
fn last_input_wins_and_releasing_it_restores_the_opposing_key() {
    let mut router = InputRouter::new();
    let mut emitter = TestEmitter::new([true, true, true, true, true]);

    assert_eq!(
        process(
            &mut router,
            &mut emitter,
            LogicalKey::HorizontalFirst,
            KeyAction::Down
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        process(
            &mut router,
            &mut emitter,
            LogicalKey::HorizontalSecond,
            KeyAction::Down
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        process(
            &mut router,
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
        router.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::SyntheticHeld
    );
}

#[test]
fn axes_are_independent() {
    let mut router = InputRouter::new();
    let mut emitter = TestEmitter::new([true, true]);

    process(
        &mut router,
        &mut emitter,
        LogicalKey::VerticalFirst,
        KeyAction::Down,
    );
    process(
        &mut router,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );

    assert_eq!(
        router.output_state(LogicalKey::VerticalFirst),
        DeliveryState::SyntheticHeld
    );
    assert_eq!(
        router.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::SyntheticHeld
    );
}

#[test]
fn failed_initial_down_falls_back_to_the_physical_event() {
    let mut router = InputRouter::new();
    let mut emitter = TestEmitter::new([false, false]);

    assert_eq!(
        process(
            &mut router,
            &mut emitter,
            LogicalKey::HorizontalFirst,
            KeyAction::Down
        ),
        EventDisposition::PassThrough
    );
    assert_eq!(
        router.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::PhysicalPassThroughHeld
    );
    assert_eq!(
        process(
            &mut router,
            &mut emitter,
            LogicalKey::HorizontalFirst,
            KeyAction::Up
        ),
        EventDisposition::PassThrough
    );
    assert_eq!(
        router.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::NotHeld
    );
}

#[test]
fn failed_release_blocks_the_opposing_down() {
    let mut router = InputRouter::new();
    let mut emitter = TestEmitter::new([true, false]);

    process(
        &mut router,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );
    assert_eq!(
        process(
            &mut router,
            &mut emitter,
            LogicalKey::HorizontalSecond,
            KeyAction::Down
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        router.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::SyntheticHeld
    );
    assert_eq!(
        router.output_state(LogicalKey::HorizontalSecond),
        DeliveryState::NotHeld
    );
}

#[test]
fn failed_down_after_a_successful_release_stays_neutral() {
    let mut router = InputRouter::new();
    let mut emitter = TestEmitter::new([true, true, false]);

    process(
        &mut router,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );
    assert_eq!(
        process(
            &mut router,
            &mut emitter,
            LogicalKey::HorizontalSecond,
            KeyAction::Down
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        router.output_state(LogicalKey::HorizontalFirst),
        DeliveryState::NotHeld
    );
    assert_eq!(
        router.output_state(LogicalKey::HorizontalSecond),
        DeliveryState::NotHeld
    );
}

#[test]
fn repeated_down_does_not_change_priority_or_emit_again() {
    let mut router = InputRouter::new();
    let mut emitter = TestEmitter::new([true, true, true]);

    process(
        &mut router,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );
    process(
        &mut router,
        &mut emitter,
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
    );
    assert_eq!(
        process(
            &mut router,
            &mut emitter,
            LogicalKey::HorizontalFirst,
            KeyAction::Down
        ),
        EventDisposition::Consume
    );
    assert_eq!(
        router.output_state(LogicalKey::HorizontalSecond),
        DeliveryState::SyntheticHeld
    );
    assert_eq!(emitter.attempts.len(), 3);
}

#[test]
fn untracked_key_up_passes_through() {
    let mut router = InputRouter::new();
    let mut emitter = TestEmitter::new([]);

    assert_eq!(
        process(
            &mut router,
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
    let mut router = InputRouter::new();
    let mut emitter = TestEmitter::new([true, true, true, true]);

    process(
        &mut router,
        &mut emitter,
        LogicalKey::VerticalFirst,
        KeyAction::Down,
    );
    process(
        &mut router,
        &mut emitter,
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
    );
    router.release_all(&mut emitter);

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
