use std::time::{Duration, Instant};

use lastkey::core::{KeyAction, LogicalKey, MeasurementSession, RecommendedTimingRange, recommend};

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
    assert_eq!(statistics.sample_count, 1);
    assert_eq!(statistics.transition.count, 1);
    assert_eq!(statistics.overlap.count, 0);
    assert_eq!(statistics.transition.min_micros, Some(5_000));
    assert_eq!(statistics.transition.max_micros, Some(5_000));
    assert_eq!(statistics.transition.latest_micros, Some(5_000));
    assert_eq!(statistics.transition.p10_micros, Some(5_000));
    assert_eq!(statistics.transition.median_micros, Some(5_000));
    assert_eq!(statistics.transition.p90_micros, Some(5_000));
    assert_eq!(statistics.overlap.min_micros, None);
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
    assert_eq!(statistics.overlap.min_micros, Some(6_000));
    assert_eq!(statistics.overlap.max_micros, Some(6_000));
    assert_eq!(statistics.overlap.latest_micros, Some(6_000));
    assert_eq!(statistics.overlap.p10_micros, Some(6_000));
    assert_eq!(statistics.overlap.median_micros, Some(6_000));
    assert_eq!(statistics.overlap.p90_micros, Some(6_000));
    assert_eq!(recommend(statistics).preserved_overlap, None);
}

#[test]
fn separates_near_simultaneous_edges_from_timing_distributions() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    session.observe(LogicalKey::HorizontalFirst, KeyAction::Down, start);
    session.observe(
        LogicalKey::HorizontalFirst,
        KeyAction::Up,
        start + Duration::from_millis(10),
    );
    let statistics = session
        .observe(
            LogicalKey::HorizontalSecond,
            KeyAction::Down,
            start + Duration::from_micros(10_999),
        )
        .expect("near-simultaneous sample");

    assert_eq!(statistics.sample_count, 1);
    assert_eq!(statistics.near_simultaneous_count, 1);
    assert_eq!(statistics.transition.count, 0);
    assert_eq!(statistics.overlap.count, 0);
    assert_eq!(statistics.transition.min_micros, None);
    assert_eq!(recommend(statistics).socd_transition, None);
}

#[test]
fn tracks_minimum_maximum_and_latest_values_for_each_sample_type() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();

    session.observe(LogicalKey::HorizontalFirst, KeyAction::Down, start);
    session.observe(
        LogicalKey::HorizontalFirst,
        KeyAction::Up,
        start + Duration::from_millis(10),
    );
    session.observe(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start + Duration::from_millis(15),
    );
    session.observe(
        LogicalKey::HorizontalSecond,
        KeyAction::Up,
        start + Duration::from_millis(20),
    );
    session.observe(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start + Duration::from_millis(28),
    );

    session.observe(
        LogicalKey::VerticalFirst,
        KeyAction::Down,
        start + Duration::from_millis(40),
    );
    session.observe(
        LogicalKey::VerticalSecond,
        KeyAction::Down,
        start + Duration::from_millis(44),
    );
    session.observe(
        LogicalKey::VerticalFirst,
        KeyAction::Up,
        start + Duration::from_millis(50),
    );
    session.observe(
        LogicalKey::VerticalFirst,
        KeyAction::Down,
        start + Duration::from_millis(60),
    );
    session.observe(
        LogicalKey::VerticalSecond,
        KeyAction::Up,
        start + Duration::from_millis(63),
    );

    let statistics = session.statistics();
    assert_eq!(statistics.transition.min_micros, Some(5_000));
    assert_eq!(statistics.transition.max_micros, Some(8_000));
    assert_eq!(statistics.transition.latest_micros, Some(8_000));
    assert_eq!(statistics.transition.p10_micros, Some(5_300));
    assert_eq!(statistics.transition.median_micros, Some(6_500));
    assert_eq!(statistics.transition.p90_micros, Some(7_700));
    assert_eq!(statistics.overlap.min_micros, Some(3_000));
    assert_eq!(statistics.overlap.max_micros, Some(6_000));
    assert_eq!(statistics.overlap.latest_micros, Some(3_000));
    assert_eq!(statistics.overlap.p10_micros, Some(3_300));
    assert_eq!(statistics.overlap.median_micros, Some(4_500));
    assert_eq!(statistics.overlap.p90_micros, Some(5_700));
}

#[test]
fn recommends_p10_to_median_after_ten_transition_samples() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    let mut elapsed = Duration::ZERO;
    let mut current = LogicalKey::HorizontalFirst;
    session.observe(current, KeyAction::Down, start);

    for gap_ms in (10..=100).step_by(10) {
        elapsed += Duration::from_millis(1);
        session.observe(current, KeyAction::Up, start + elapsed);
        elapsed += Duration::from_millis(gap_ms);
        current = if current == LogicalKey::HorizontalFirst {
            LogicalKey::HorizontalSecond
        } else {
            LogicalKey::HorizontalFirst
        };
        session.observe(current, KeyAction::Down, start + elapsed);
    }

    let recommendation = recommend(session.statistics());
    assert_eq!(
        recommendation.socd_transition,
        Some(RecommendedTimingRange {
            min_micros: 19_000,
            max_micros: 55_000,
        })
    );
    assert_eq!(recommendation.preserved_overlap, None);
}

#[test]
fn rounds_recommended_ranges_to_the_nearest_tenth_millisecond() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    let mut elapsed = Duration::ZERO;
    let mut current = LogicalKey::HorizontalFirst;
    session.observe(current, KeyAction::Down, start);

    for _ in 0..10 {
        elapsed += Duration::from_millis(1);
        session.observe(current, KeyAction::Up, start + elapsed);
        elapsed += Duration::from_micros(1_499);
        current = if current == LogicalKey::HorizontalFirst {
            LogicalKey::HorizontalSecond
        } else {
            LogicalKey::HorizontalFirst
        };
        session.observe(current, KeyAction::Down, start + elapsed);
    }

    assert_eq!(
        recommend(session.statistics()).socd_transition,
        Some(RecommendedTimingRange {
            min_micros: 1_400,
            max_micros: 1_400,
        })
    );
}

#[test]
fn recommends_p10_to_median_after_ten_overlap_samples() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    let mut elapsed = Duration::ZERO;

    for overlap_ms in (10..=100).step_by(10) {
        session.observe(LogicalKey::VerticalFirst, KeyAction::Down, start + elapsed);
        elapsed += Duration::from_millis(1);
        session.observe(LogicalKey::VerticalSecond, KeyAction::Down, start + elapsed);
        elapsed += Duration::from_millis(overlap_ms);
        session.observe(LogicalKey::VerticalFirst, KeyAction::Up, start + elapsed);
        elapsed += Duration::from_millis(1);
        session.observe(LogicalKey::VerticalSecond, KeyAction::Up, start + elapsed);
        elapsed += Duration::from_millis(1);
    }

    let recommendation = recommend(session.statistics());
    assert_eq!(
        recommendation.preserved_overlap,
        Some(RecommendedTimingRange {
            min_micros: 19_000,
            max_micros: 55_000,
        })
    );
}

#[test]
fn second_pressed_release_ends_overlap_without_over_reporting() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    session.observe(LogicalKey::HorizontalFirst, KeyAction::Down, start);
    session.observe(
        LogicalKey::HorizontalSecond,
        KeyAction::Down,
        start + Duration::from_millis(10),
    );
    let statistics = session
        .observe(
            LogicalKey::HorizontalSecond,
            KeyAction::Up,
            start + Duration::from_millis(20),
        )
        .expect("overlap ends at first release");
    assert_eq!(statistics.overlap.count, 1);
    assert_eq!(statistics.overlap.min_micros, Some(10_000));
    assert_eq!(statistics.overlap.max_micros, Some(10_000));

    assert_eq!(
        session.observe(
            LogicalKey::HorizontalFirst,
            KeyAction::Up,
            start + Duration::from_millis(500),
        ),
        None
    );
    assert_eq!(session.statistics().overlap.count, 1);
    assert_eq!(session.statistics().sample_count, 1);
}

#[test]
fn auto_repeat_downs_and_duplicate_ups_do_not_inflate_the_edge_count() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    session.observe(LogicalKey::HorizontalFirst, KeyAction::Down, start);
    session.observe(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start + Duration::from_millis(33),
    );
    session.observe(
        LogicalKey::HorizontalFirst,
        KeyAction::Down,
        start + Duration::from_millis(66),
    );
    assert_eq!(session.edge_count(), 1);
    session.observe(
        LogicalKey::HorizontalFirst,
        KeyAction::Up,
        start + Duration::from_millis(100),
    );
    session.observe(
        LogicalKey::HorizontalFirst,
        KeyAction::Up,
        start + Duration::from_millis(110),
    );
    assert_eq!(session.edge_count(), 2);
}

#[test]
fn same_direction_repress_does_not_create_a_phantom_transition() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    for (key, action, ms) in [
        (LogicalKey::HorizontalFirst, KeyAction::Down, 0),
        (LogicalKey::HorizontalFirst, KeyAction::Up, 10),
        (LogicalKey::HorizontalFirst, KeyAction::Down, 20),
        (LogicalKey::HorizontalSecond, KeyAction::Down, 30),
        (LogicalKey::HorizontalFirst, KeyAction::Up, 40),
        (LogicalKey::HorizontalSecond, KeyAction::Up, 50),
        (LogicalKey::HorizontalSecond, KeyAction::Down, 60),
    ] {
        session.observe(key, action, start + Duration::from_millis(ms));
    }
    assert_eq!(session.statistics().overlap.count, 1);
    assert_eq!(session.statistics().transition.count, 0);
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
    assert_eq!(session.statistics().sample_count, 0);
}

#[test]
fn minute_long_holds_record_no_overlap_sample() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    session.observe(LogicalKey::VerticalFirst, KeyAction::Down, start);
    session.observe(
        LogicalKey::VerticalSecond,
        KeyAction::Down,
        start + Duration::from_secs(1),
    );
    assert_eq!(
        session.observe(
            LogicalKey::VerticalFirst,
            KeyAction::Up,
            start + Duration::from_secs(62)
        ),
        None
    );
    let statistics = session.statistics();
    assert_eq!(statistics.sample_count, 0);
    assert_eq!(statistics.overlap.count, 0);
}

#[test]
fn overlap_at_exactly_the_pair_gap_is_still_recorded() {
    let start = Instant::now();
    let mut session = MeasurementSession::new();
    session.observe(LogicalKey::VerticalFirst, KeyAction::Down, start);
    session.observe(
        LogicalKey::VerticalSecond,
        KeyAction::Down,
        start + Duration::from_secs(1),
    );
    let statistics = session
        .observe(
            LogicalKey::VerticalFirst,
            KeyAction::Up,
            start + Duration::from_secs(2),
        )
        .expect("boundary overlap sample");
    assert_eq!(statistics.sample_count, 1);
    assert_eq!(statistics.overlap.count, 1);
}
