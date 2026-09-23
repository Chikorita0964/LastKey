//! Shared paint-time motion helpers for the settings window.
//!
//! The reference animates several controls with CSS `transition-all
//! duration-75` and `transition-all duration-150`, and a CSS transition is a
//! *paint-time* interpolation: the layout box never moves, only what is drawn
//! between the two states. egui is immediate-mode and has no transition
//! primitive, so a widget that must animate keeps its in-flight segment in the
//! frame's temp memory and asks for repaints until the segment settles.
//!
//! [`Transition`] is that state machine. It interpolates one scalar per
//! widget and is deliberately pure: [`Transition::poll`] takes the current time
//! as an argument, so tests drive it with explicit times instead of sleeping.

use egui::{Context, Id};
use std::time::Duration;

/// The reference's `duration-75`: a keycap press, a hover, or the D-pad dot's
/// travel.
pub const DURATION_75_SECS: f64 = 0.075;
/// The reference's `duration-150`: the delay badge's state change.
pub const DURATION_150_SECS: f64 = 0.150;
/// The cadence of the intermediate frames a transition asks for while it is in
/// flight, the 16 ms the timeline playhead uses. The final request of a
/// segment is shortened to its remaining time so the last frame lands exactly
/// on the settled value.
const FRAME_STEP_SECS: f64 = 0.016;

/// CSS `ease` (`cubic-bezier(0.25, 0.1, 0.25, 1)`), Tailwind's default curve.
///
/// Evaluated for a linear progress in `0..=1` by inverting the bezier's x with
/// a bisection. The curve's x is monotonic on `0..=1`, so the bisection
/// converges; 24 halvings resolve it well past display precision.
fn ease(progress: f32) -> f32 {
    cubic_bezier(progress, (0.25, 0.1), (0.25, 1.0))
}

/// CSS `ease-out` (`cubic-bezier(0, 0, 0.58, 1)`), the curve the mapping card's
/// `duration-75 ease-out` names.
fn ease_out(progress: f32) -> f32 {
    cubic_bezier(progress, (0.0, 0.0), (0.58, 1.0))
}

/// Tailwind's default transition timing function,
/// `cubic-bezier(0.4, 0, 0.2, 1)`.
///
/// This is Material Design's *standard* curve, not a symmetric ease-in-out: it
/// leaves the origin more slowly than it arrives, so its midpoint sits well
/// past 0.5. Both keycap cards' `transition-all duration-75` names no easing
/// utility, so this is what their press animation actually runs.
fn standard(progress: f32) -> f32 {
    cubic_bezier(progress, (0.4, 0.0), (0.2, 1.0))
}

/// A CSS cubic-bezier timing function with control points `(0, 0)`, `p1`,
/// `p2`, `(1, 1)`, evaluated at a linear progress in `0..=1`.
///
/// The curve is parametric: `x(s)` and `y(s)` are both cubics in `s`, and a CSS
/// timing function's value is `y(s)` where `s` solves `x(s) == progress`. `x` is
/// monotonic for the control points Tailwind's named easings use, so bisecting
/// `s` finds it.
fn cubic_bezier(progress: f32, p1: (f32, f32), p2: (f32, f32)) -> f32 {
    let x = progress.clamp(0.0, 1.0);
    if x <= 0.0 || x >= 1.0 {
        return x;
    }
    // A cubic bezier with endpoints (0, 0) and (1, 1):
    //   v(s) = 3 * s * (1 - s)^2 * c1 + 3 * s^2 * (1 - s) * c2 + s^3
    let axis = |s: f32, c1: f32, c2: f32| {
        let t = 1.0 - s;
        3.0 * s * t * t * c1 + 3.0 * s * s * t * c2 + s * s * s
    };
    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    for _ in 0..24 {
        let mid = (low + high) / 2.0;
        if axis(mid, p1.0, p2.0) < x {
            low = mid;
        } else {
            high = mid;
        }
    }
    axis((low + high) / 2.0, p1.1, p2.1)
}

/// The easing curve a transition follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Easing {
    /// `cubic-bezier(0.4, 0, 0.2, 1)` -- Tailwind's default, which is what
    /// `transition-all` resolves to.
    #[default]
    Standard,
    /// `cubic-bezier(0, 0, 0.58, 1)` -- an explicit `ease-out`.
    EaseOut,
    /// `cubic-bezier(0.25, 0.1, 0.25, 1)` -- an explicit `ease`.
    Ease,
}

impl Easing {
    fn apply(self, progress: f32) -> f32 {
        match self {
            Self::Standard => standard(progress),
            Self::EaseOut => ease_out(progress),
            Self::Ease => ease(progress),
        }
    }
}

/// One animated scalar, interpolated between its observed targets the way a CSS
/// transition interpolates.
///
/// A CSS transition starts when the target changes and runs for its duration;
/// a change mid-flight restarts from the *displayed* value, not from the
/// previous target, so a rapidly toggled control never jumps. The first
/// observation adopts its target without a segment, which is why a freshly
/// mounted widget does not animate in from a default.
#[derive(Clone, Copy, Debug)]
pub struct Transition {
    /// Whether a target has been observed before.
    seen: bool,
    in_flight: bool,
    from: f32,
    to: f32,
    start: f64,
    duration: f64,
    easing: Easing,
}

impl Default for Transition {
    fn default() -> Self {
        Self {
            seen: false,
            in_flight: false,
            from: 0.0,
            to: 0.0,
            start: 0.0,
            duration: DURATION_75_SECS,
            easing: Easing::default(),
        }
    }
}

impl Transition {
    /// A transition on the reference's `duration-75` and `transition-all`
    /// easing, which is what both keycap cards' press animation uses.
    pub fn new() -> Self {
        Self::default()
    }

    /// A transition with an explicit duration and curve.
    pub fn with(duration: f64, easing: Easing) -> Self {
        Self {
            duration,
            easing,
            ..Self::default()
        }
    }

    /// Advances to `now` (egui's monotonic input time, seconds) and returns the
    /// value to draw for `target`.
    pub fn poll(&mut self, now: f64, target: f32) -> f32 {
        if !self.seen {
            self.seen = true;
            self.to = target;
            self.from = target;
            return target;
        }
        if !self.in_flight {
            if target == self.to {
                return self.to;
            }
            self.start_segment(now, self.to, target);
            return self.from;
        }
        let progress = (now - self.start) / self.duration;
        // The deadline comparison carries an epsilon: a caller's `now` and the
        // `start` it recorded rarely differ by exactly `duration` in f64, and a
        // progress of 0.9999999999999995 must settle rather than leaving the
        // segment in flight with a zero-length repaint request.
        if progress >= 1.0 - 1e-9 {
            self.in_flight = false;
            self.from = self.to;
            return self.to;
        }
        let displayed = self.from + (self.to - self.from) * self.easing.apply(progress as f32);
        if target != self.to {
            // Retarget from what is on screen, the way a CSS transition
            // restarts from its current computed value.
            self.start_segment(now, displayed, target);
            return displayed;
        }
        displayed
    }

    fn start_segment(&mut self, now: f64, from: f32, to: f32) {
        self.from = from;
        self.to = to;
        self.start = now;
        self.in_flight = true;
    }

    /// Seconds until the next repaint is useful, or `None` once settled.
    pub fn remaining(&self, now: f64) -> Option<f64> {
        if !self.in_flight {
            return None;
        }
        let left = (self.duration - (now - self.start)).max(0.0);
        Some(left.min(FRAME_STEP_SECS))
    }
}

/// Drives one [`Transition`] stored in the frame's temp memory under `id` and
/// requests the repaints it needs.
///
/// This is the whole egui-side idiom in one place: read the state, advance it,
/// and ask for the next frame only while a segment is in flight. A widget calls
/// it with a stable `id` (one per animated element) and the target for this
/// frame, and draws with the returned value.
pub fn animate(ui: &egui::Ui, id: Id, target: f32, duration: f64, easing: Easing) -> f32 {
    let now = ui.input(|input| input.time);
    let (value, repaint_in) = ui.data_mut(|data| {
        let transition =
            data.get_temp_mut_or_insert_with(id, || Transition::with(duration, easing));
        (transition.poll(now, target), transition.remaining(now))
    });
    if let Some(repaint_in) = repaint_in {
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(repaint_in));
    }
    value
}

/// Drives one [`Transition`] for a widget that has no `Ui` handy (a paint-only
/// helper already holding a [`Context`]).
pub fn animate_in(
    ctx: &Context,
    id: Id,
    now: f64,
    target: f32,
    duration: f64,
    easing: Easing,
) -> f32 {
    let (value, repaint_in) = ctx.data_mut(|data| {
        let transition =
            data.get_temp_mut_or_insert_with(id, || Transition::with(duration, easing));
        (transition.poll(now, target), transition.remaining(now))
    });
    if let Some(repaint_in) = repaint_in {
        ctx.request_repaint_after(Duration::from_secs_f64(repaint_in));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_easing_curves_hit_their_endpoints_and_are_monotonic() {
        for easing in [Easing::Standard, Easing::EaseOut, Easing::Ease] {
            assert_eq!(easing.apply(0.0), 0.0, "{easing:?}");
            assert_eq!(easing.apply(1.0), 1.0, "{easing:?}");
            let mut previous = -1.0;
            for step in 0..=20 {
                let value = easing.apply(step as f32 / 20.0);
                assert!(value >= previous, "{easing:?} must not go backwards");
                previous = value;
            }
        }
    }

    /// The curves are distinct shapes, and Tailwind's default is Material's
    /// *standard* curve: it leaves the origin more slowly than it arrives, so
    /// its midpoint sits well past 0.5 rather than on it.
    #[test]
    fn the_curves_are_distinguishable() {
        assert!(
            ease_out(0.25) > standard(0.25),
            "ease-out must lead at the quarter point"
        );
        assert!(
            standard(0.5) > 0.6,
            "the standard curve is asymmetric and past halfway at its midpoint: {}",
            standard(0.5)
        );
        assert!(
            standard(0.5) < 1.0 && standard(0.75) > standard(0.5),
            "the standard curve keeps rising after its midpoint"
        );
        assert!(
            ease_out(0.5) > 0.6,
            "ease-out is past halfway at the midpoint: {}",
            ease_out(0.5)
        );
    }

    #[test]
    fn the_first_observation_adopts_its_target_without_a_segment() {
        let mut transition = Transition::new();
        assert_eq!(transition.poll(1.0, 0.95), 0.95);
        assert_eq!(transition.remaining(1.0), None);
    }

    #[test]
    fn a_target_change_glides_over_the_transition_duration() {
        let mut transition = Transition::new();
        assert_eq!(transition.poll(1.0, 1.0), 1.0);

        // The frame the segment starts still shows the previous value.
        assert_eq!(transition.poll(1.0, 0.95), 1.0);
        assert!(transition.remaining(1.0).is_some());

        let quarter = transition.poll(1.0 + DURATION_75_SECS / 4.0, 0.95);
        let half = transition.poll(1.0 + DURATION_75_SECS / 2.0, 0.95);
        assert!(
            (0.95..1.0).contains(&quarter),
            "a quarter in, the value is between the endpoints: {quarter}"
        );
        assert!(
            half < quarter,
            "the value must keep moving toward the target: {quarter} then {half}"
        );

        // At the deadline it has arrived, and the repaint requests stop.
        assert_eq!(
            transition.poll(1.0 + DURATION_75_SECS, 0.95),
            0.95,
            "the segment lands exactly on its target"
        );
        assert_eq!(transition.remaining(1.0 + DURATION_75_SECS), None);
        assert_eq!(transition.poll(1.0 + 2.0 * DURATION_75_SECS, 0.95), 0.95);
    }
    #[test]
    fn a_mid_flight_change_restarts_from_the_displayed_value() {
        let mut transition = Transition::new();
        assert_eq!(transition.poll(0.0, 1.0), 1.0);

        // Start the segment, then advance into it: the starting frame still
        // shows the previous value, so the first sample has to come later.
        assert_eq!(transition.poll(0.0, 0.95), 1.0);
        let halfway = transition.poll(DURATION_75_SECS / 2.0, 0.95);
        assert!(
            halfway < 1.0 && halfway > 0.95,
            "halfway into the segment the value is between the endpoints: {halfway}"
        );

        // Reversing must not jump to the old target first.
        let reversed = transition.poll(DURATION_75_SECS / 2.0, 1.0);
        assert_eq!(
            reversed, halfway,
            "the restart frame keeps showing what was on screen"
        );
        assert_eq!(
            transition.poll(DURATION_75_SECS / 2.0 + DURATION_75_SECS, 1.0),
            1.0
        );
    }

    #[test]
    fn an_explicit_curve_and_duration_are_honoured() {
        let mut transition = Transition::with(DURATION_150_SECS, Easing::EaseOut);
        assert_eq!(transition.poll(0.0, 0.0), 0.0);
        assert_eq!(transition.poll(0.0, 1.0), 0.0);
        // A quarter of the way into a 150ms ease-out is well past a quarter.
        let value = transition.poll(DURATION_150_SECS / 4.0, 1.0);
        assert!(value > 0.25, "ease-out leads: {value}");
        assert_eq!(transition.poll(DURATION_150_SECS, 1.0), 1.0);
    }
}
