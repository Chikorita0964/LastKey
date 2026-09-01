use std::time::{Duration, Instant};

use lastkey::core::{KeyAction, LogicalKey, MeasurementSession, recommend};

#[test]
fn measures_a_positive_neutral_transition_from_physical_edges() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    assert_eq!(
        session.observe(LogicalKey::HorizontalFirst, KeyAction::Down, start),
        None
    );
    assert_eq!(
        session.observe(
            LogicalKey::HorizontalFirst,
            KeyAction::Up,
            start + Duration::from_millis(10)
        ),
        None
    );
    let statistics = session
        .observe(
            LogicalKey::HorizontalSecond,
            KeyAction::Down,
            start + Duration::from_millis(15),
        )
        .expect("transition sample");
    assert_eq!(statistics.sample_count(), 1);
    assert_eq!(statistics.transition_count(), 1);
    assert_eq!(statistics.overlap_count(), 0);
    assert_eq!(statistics.average_transition_micros(), Some(5_000));
}

#[test]
fn measures_a_negative_overlap_from_physical_edges() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    session.observe(LogicalKey::VerticalFirst, KeyAction::Down, start);
    assert_eq!(
        session.observe(
            LogicalKey::VerticalSecond,
            KeyAction::Down,
            start + Duration::from_millis(4)
        ),
        None
    );
    let statistics = session
        .observe(
            LogicalKey::VerticalFirst,
            KeyAction::Up,
            start + Duration::from_millis(10),
        )
        .expect("overlap sample");
    assert_eq!(statistics.average_overlap_micros(), Some(-6_000));
    assert_eq!(recommend(statistics).overlap_micros, Some(6_000));
}

#[test]
fn ignores_repeats_and_edges_outside_the_pairing_window() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    session.observe(LogicalKey::HorizontalFirst, KeyAction::Down, start);
    assert_eq!(
        session.observe(
            LogicalKey::HorizontalFirst,
            KeyAction::Down,
            start + Duration::from_millis(1)
        ),
        None
    );
    session.observe(
        LogicalKey::HorizontalFirst,
        KeyAction::Up,
        start + Duration::from_millis(2),
    );
    assert_eq!(
        session.observe(
            LogicalKey::HorizontalSecond,
            KeyAction::Down,
            start + Duration::from_secs(2)
        ),
        None
    );
    assert_eq!(session.statistics().sample_count(), 0);
}
