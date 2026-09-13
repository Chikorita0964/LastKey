//! Bounded, opt-in display history of the engine's monitor events. Never persisted.
//!
//! The graph mirrors the reference Gantt canvas: two axis pairs (vertical
//! pair, then horizontal pair) split by a center time ruler, a keycap per
//! lane, per-key signal blocks carrying their own hold duration, overlap
//! columns with duration badges, and a `NOW` playhead. Every element is
//! cached canvas geometry — the keycaps, rounded lanes, and in-block labels
//! are not expressible as quads, so splitting the pass would only scatter
//! one coordinate system across two renderers.

use crate::protocol::{KeySlot, MonitorDecision, MonitorEdge, MonitorSnapshot};
use iced::{
    Color, Element, Event, Font, Length, Point, Rectangle, Size, Theme, Vector,
    advanced::{
        Layout, Renderer as _, Shell, Widget,
        graphics::geometry::Renderer as _,
        layout, mouse,
        text::{Alignment, Ellipsis, LineHeight, Shaping, Wrapping},
        widget::{Tree, tree},
    },
    alignment::Vertical,
    widget::canvas::{Cache, Frame, LineCap, LineJoin, Path, Stroke, Text},
    window,
};
use std::{
    cell::Cell,
    collections::VecDeque,
    hash::{DefaultHasher, Hash, Hasher},
    time::{Duration, Instant},
};

/// Which way a lane's keycap arrow points.
#[derive(Clone, Copy)]
enum Arrow {
    Up,
    Down,
    Left,
    Right,
}

/// One lane's display identity from the reference canvas theme: the signal
/// color plus the tint, border, and ring it takes while its key is held.
struct Lane {
    signal: Color,
    held_fill: Color,
    held_border: Color,
    held_ring: Color,
    arrow: Arrow,
}

const LANES: [Lane; 4] = [
    Lane {
        signal: Color::from_rgb8(37, 99, 235),
        held_fill: Color::from_rgba8(37, 99, 235, 0.12),
        held_border: Color::from_rgb8(29, 78, 216),
        held_ring: Color::from_rgba8(191, 219, 254, 0.90),
        arrow: Arrow::Up,
    },
    Lane {
        signal: Color::from_rgb8(124, 58, 237),
        held_fill: Color::from_rgba8(139, 92, 246, 0.12),
        held_border: Color::from_rgb8(109, 40, 217),
        held_ring: Color::from_rgba8(221, 214, 254, 0.90),
        arrow: Arrow::Down,
    },
    Lane {
        signal: Color::from_rgb8(79, 70, 229),
        held_fill: Color::from_rgba8(99, 102, 241, 0.12),
        held_border: Color::from_rgb8(67, 56, 202),
        held_ring: Color::from_rgba8(199, 210, 254, 0.90),
        arrow: Arrow::Left,
    },
    Lane {
        signal: Color::from_rgb8(147, 51, 234),
        held_fill: Color::from_rgba8(147, 51, 234, 0.12),
        held_border: Color::from_rgb8(126, 34, 206),
        held_ring: Color::from_rgba8(233, 213, 255, 0.90),
        arrow: Arrow::Right,
    },
];

const LANE_IDLE: Color = Color::from_rgb8(248, 250, 252);
const LANE_BORDER: Color = Color::from_rgb8(226, 232, 240);
const CAP_SURFACE: Color = Color::WHITE;
const CAP_SHADOW: Color = Color::from_rgba8(0, 0, 0, 0.05);
const CAP_TEXT: Color = Color::from_rgb8(15, 23, 42);
const OVERLAP_FILL: Color = Color::from_rgba8(99, 102, 241, 0.18);
/// A still-open overlap reads stronger than a settled one.
const OVERLAP_FILL_LIVE: Color = Color::from_rgba8(99, 102, 241, 0.26);
const OVERLAP_EDGE: Color = Color::from_rgb8(79, 70, 229);
const BADGE_FILL: Color = Color::from_rgb8(30, 27, 75);
const BADGE_EDGE: Color = Color::from_rgb8(129, 140, 248);
const RULER_LINE: Color = Color::from_rgb8(203, 213, 225);
const RULER_TEXT: Color = Color::from_rgb8(71, 85, 105);
const NEEDLE: Color = Color::from_rgb8(79, 70, 229);

/// Left gutter holding the per-lane keycaps, matching the reference's 62 px
/// label column.
const LABEL_GUTTER: f32 = 62.0;
const RIGHT_PAD: f32 = 12.0;
const CAP_SIZE: f32 = 40.0;
const CAP_RADIUS: f32 = 10.0;
/// Lane metrics: the reference uses 46 px tracks with a 6 px gap, the center
/// ruler block between the two axis pairs, and top/bottom margins that hold
/// the overlap badges drawn outside the first and last track.
const LANE_HEIGHT: f32 = 46.0;
const LANE_GAP: f32 = 6.0;
const LANE_RADIUS: f32 = 8.0;
const PAIR_HEIGHT: f32 = 2.0 * LANE_HEIGHT + LANE_GAP;
const RULER_BLOCK: f32 = 41.0;
const TOP_MARGIN: f32 = 24.0;
const BOTTOM_PAD: f32 = 24.0;
const GRAPH_HEIGHT: f32 = TOP_MARGIN + 2.0 * PAIR_HEIGHT + RULER_BLOCK + BOTTOM_PAD;
const BADGE_HEIGHT: f32 = 14.0;
/// Ruler ticks every 200 ms across the 1 s window.
const RULER_STEP_MICROS: u64 = 200_000;

const WINDOW_MICROS: u64 = 1_000_000;
const MAX_INTERVALS: usize = 512;

#[derive(Default)]
pub enum MonitorState {
    #[default]
    Stopped,
    Starting,
    Recording(Timeline),
    Stopping(Timeline),
}

impl MonitorState {
    pub fn timeline(&self) -> Option<&Timeline> {
        match self {
            Self::Recording(timeline) | Self::Stopping(timeline) => Some(timeline),
            _ => None,
        }
    }
    pub fn resynchronize(&mut self) {
        if let Self::Recording(timeline) | Self::Stopping(timeline) = self {
            timeline.clear();
        }
    }
}

#[derive(Clone, Copy)]
struct Interval {
    key: usize,
    start: u64,
    end: u64,
}

pub struct Timeline {
    intervals: VecDeque<Interval>,
    held_since: [Option<u64>; 4],
    anchor: Option<(Instant, u64)>,
    pub decision: MonitorDecision,
    pub physical: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            intervals: VecDeque::with_capacity(MAX_INTERVALS),
            held_since: [None; 4],
            anchor: None,
            decision: MonitorDecision::Immediate,
            physical: false,
        }
    }
}

impl Timeline {
    pub fn clear(&mut self) {
        self.intervals.clear();
        self.held_since = [None; 4];
        self.anchor = None;
        self.decision = MonitorDecision::Immediate;
    }
    pub fn accept(&mut self, event: MonitorSnapshot, measuring: bool, received: Instant) {
        let physical = !event.filter_enabled || measuring;
        if self.physical != physical {
            self.clear();
        }
        self.physical = physical;
        // Keep backend deltas intact; arrival time only positions the first event on screen.
        if self.anchor.is_none() {
            self.anchor = Some((received, event.elapsed_micros));
        }
        self.decision = event.decision;
        if physical {
            if let Some(edge) = event.physical {
                self.edge(edge, event.elapsed_micros);
            }
        } else {
            for edge in event.outputs {
                self.edge(edge, event.elapsed_micros);
            }
        }
        let cutoff = event.elapsed_micros.saturating_sub(WINDOW_MICROS);
        while self
            .intervals
            .front()
            .is_some_and(|interval| interval.end < cutoff)
        {
            self.intervals.pop_front();
        }
    }
    fn edge(&mut self, edge: MonitorEdge, at: u64) {
        let key = index(edge.key);
        if edge.pressed {
            self.held_since[key].get_or_insert(at);
        } else if let Some(start) = self.held_since[key].take() {
            if self.intervals.len() == MAX_INTERVALS {
                self.intervals.pop_front();
            }
            self.intervals.push_back(Interval {
                key,
                start,
                end: at,
            });
        }
    }
    fn now(&self) -> u64 {
        self.anchor.map_or(0, |(arrival, offset)| {
            offset.saturating_add(arrival.elapsed().as_micros() as u64)
        })
    }
    pub fn held(&self, key: KeySlot) -> bool {
        self.held_since[index(key)].is_some()
    }
    fn held_mask(&self) -> [bool; 4] {
        std::array::from_fn(|key| self.held_since[key].is_some())
    }
    /// Visible `(start, end, still_held)` spans for one lane. A held key ends
    /// at `now`, which is what makes its block and any overlap grow live.
    fn spans(&self, key: usize, now: u64) -> Vec<(u64, u64, bool)> {
        let mut spans: Vec<(u64, u64, bool)> = self
            .intervals
            .iter()
            .filter(|interval| {
                interval.key == key && now.saturating_sub(interval.end) <= WINDOW_MICROS
            })
            .map(|interval| (interval.start, interval.end, false))
            .collect();
        if let Some(start) = self.held_since[key] {
            spans.push((start, now, true));
        }
        spans
    }
    /// Last-press-wins resolution between two opposing keys, for the D-pad's
    /// center dot: when both are held, the key pressed later takes it.
    pub fn winner(&self, first: KeySlot, second: KeySlot) -> Option<KeySlot> {
        match (
            self.held_since[index(first)],
            self.held_since[index(second)],
        ) {
            (Some(a), Some(b)) => Some(if b >= a { second } else { first }),
            (Some(_), None) => Some(first),
            (None, Some(_)) => Some(second),
            (None, None) => None,
        }
    }
}

const fn index(key: KeySlot) -> usize {
    match key {
        KeySlot::VerticalFirst => 0,
        KeySlot::VerticalSecond => 1,
        KeySlot::HorizontalFirst => 2,
        KeySlot::HorizontalSecond => 3,
    }
}

pub fn graph<'a, Message: 'a>(
    timeline: Option<&'a Timeline>,
    names: [&'a str; 4],
) -> Element<'a, Message> {
    Element::new(Graph { timeline, names })
}

struct Graph<'a> {
    timeline: Option<&'a Timeline>,
    names: [&'a str; 4],
}

/// What the cached geometry was drawn for: the 60 Hz frame, which keys were
/// held, the track width, and a hash of the key names.
type CacheKey = (u64, [bool; 4], u64, u64);

#[derive(Default)]
struct GraphState {
    cache: Cache,
    key: Cell<Option<CacheKey>>,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Graph<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<GraphState>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(GraphState::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, GRAPH_HEIGHT.into())
    }
    fn layout(
        &mut self,
        _: &mut Tree,
        _: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, GRAPH_HEIGHT)
    }
    fn update(
        &mut self,
        _: &mut Tree,
        event: &Event,
        _: Layout<'_>,
        _: mouse::Cursor,
        _: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        _: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && let Some(timeline) = self.timeline
        {
            let animate = timeline.held_since.iter().any(Option::is_some)
                || timeline.intervals.back().is_some_and(|interval| {
                    timeline.now().saturating_sub(interval.end) < WINDOW_MICROS
                });
            if animate {
                shell.request_redraw_at(*now + Duration::from_millis(16));
            }
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _: &Theme,
        _: &iced::advanced::renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<GraphState>();
        // Cache on the visible window: the playhead moves every frame while
        // recording, so geometry is re-tessellated on motion and reused when
        // the window is idle. Held keys and the key names join the cache key
        // so a press or a rebind repaints instead of reusing stale geometry.
        let now = self.timeline.map_or(0, Timeline::now);
        let held = self
            .timeline
            .map_or([false; 4], |timeline| timeline.held_mask());
        let mut hasher = DefaultHasher::new();
        self.names.hash(&mut hasher);
        let key = (now / 16_666, held, bounds.width as u64, hasher.finish());
        if state.key.replace(Some(key)) != Some(key) {
            state.cache.clear();
        }
        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            draw_graph(frame, self.timeline, &self.names, now);
        });
        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            renderer.draw_geometry(geometry);
        });
    }
}

/// Y offset of a lane inside the graph: two lanes, the ruler block, then two
/// more lanes (reference WS / ruler / AD grouping).
fn lane_y(row: usize) -> f32 {
    let pair_top = TOP_MARGIN + (row / 2) as f32 * (PAIR_HEIGHT + RULER_BLOCK);
    pair_top + (row % 2) as f32 * (LANE_HEIGHT + LANE_GAP)
}

/// Horizontal geometry shared by every pass: the track box and the
/// microsecond-to-x mapping that anchors `now` at the right edge.
struct Axis {
    left: f32,
    width: f32,
    now_x: f32,
    now: u64,
}

impl Axis {
    fn new(canvas_width: f32, now: u64) -> Self {
        let left = LABEL_GUTTER;
        let width = (canvas_width - LABEL_GUTTER - RIGHT_PAD).max(80.0);
        Self {
            left,
            width,
            now_x: left + width - 1.0,
            now,
        }
    }
    fn x(&self, at: u64) -> f32 {
        self.left
            + self.width
                * (1.0 - self.now.saturating_sub(at) as f32 / WINDOW_MICROS as f32).clamp(0.0, 1.0)
    }
    /// The visible span of an interval, or `None` once it has scrolled out.
    fn visible(&self, start: u64, end: u64) -> Option<(f32, f32)> {
        let x0 = self.x(start).max(self.left);
        let x1 = self.x(end).min(self.now_x);
        (x1 > self.left && x0 < self.now_x).then_some((x0, x1))
    }
    fn track(&self, row: usize) -> Rectangle {
        Rectangle {
            x: self.left,
            y: lane_y(row),
            width: self.width,
            height: LANE_HEIGHT,
        }
    }
}

/// The whole graph in one cached canvas pass. A stopped timeline still draws
/// its lanes, keycaps, and ruler so the card does not flash empty between
/// the start request and the first monitor event.
fn draw_graph(frame: &mut Frame, timeline: Option<&Timeline>, names: &[&str; 4], now: u64) {
    let axis = Axis::new(frame.width(), now);
    for (row, lane) in LANES.iter().enumerate() {
        let held = timeline.is_some_and(|timeline| timeline.held_since[row].is_some());
        draw_track(frame, &axis, row, lane, held);
        draw_keycap(frame, &axis, row, lane, names[row], held);
    }
    if let Some(timeline) = timeline {
        for (row, lane) in LANES.iter().enumerate() {
            draw_blocks(frame, &axis, timeline, row, lane);
        }
        draw_overlaps(frame, &axis, timeline);
    }
    draw_needle(frame, &axis);
    draw_ruler(frame, &axis);
}

/// One lane's rounded background, its clipped time grid, and its border.
/// A held lane takes the reference's accent tint and a heavier border.
fn draw_track(frame: &mut Frame, axis: &Axis, row: usize, lane: &Lane, held: bool) {
    let track = axis.track(row);
    let shape = || {
        Path::new(|path| {
            path.rounded_rectangle(
                Point::new(track.x, track.y),
                Size::new(track.width, track.height),
                LANE_RADIUS.into(),
            );
        })
    };
    // Inside the clip on purpose: iced appends unclipped fills after every
    // pasted subframe, so an unclipped track fill would cover the blocks.
    frame.with_clip(track, |clipped| {
        clipped.fill(&shape(), if held { lane.held_fill } else { LANE_IDLE });
        let grid = Stroke::default().with_color(LANE_BORDER).with_width(1.0);
        let mut tick = RULER_STEP_MICROS;
        while tick < WINDOW_MICROS {
            let x = axis.left + axis.width * (1.0 - tick as f32 / WINDOW_MICROS as f32);
            let line = Path::new(|path| {
                path.move_to(Point::new(x, track.y));
                path.line_to(Point::new(x, track.y + track.height));
            });
            clipped.stroke(&line, grid);
            tick += RULER_STEP_MICROS;
        }
    });
    frame.stroke(
        &shape(),
        Stroke::default()
            .with_color(if held { lane.held_border } else { LANE_BORDER })
            .with_width(if held { 1.5 } else { 1.0 }),
    );
}

/// Signal blocks for one lane, clipped to its track, each carrying its hold
/// duration when the block is wide enough to hold the label.
fn draw_blocks(frame: &mut Frame, axis: &Axis, timeline: &Timeline, row: usize, lane: &Lane) {
    let track = axis.track(row);
    frame.with_clip(track, |clipped| {
        for (start, end, open) in timeline.spans(row, axis.now) {
            let Some((x0, x1)) = axis.visible(start, end) else {
                continue;
            };
            let width = (x1 - x0).max(4.0);
            let block = Path::new(|path| {
                path.rounded_rectangle(
                    Point::new(x0, track.y + 4.0),
                    Size::new(width, track.height - 8.0),
                    5.5.into(),
                );
            });
            clipped.fill(&block, lane.signal);
            if open {
                clipped.stroke(
                    &block,
                    Stroke::default().with_color(Color::WHITE).with_width(1.5),
                );
            }
            let label = millis(end.saturating_sub(start));
            if width >= text_width(&label, 9.0) + 6.0 {
                clipped.fill_text(centered(
                    label,
                    Point::new(x0 + width / 2.0, track.y + track.height / 2.0),
                    9.0,
                    Color::WHITE,
                    crate::ui::theme::UI_FONT_BOLD,
                ));
            }
        }
    });
}

/// The keycap in the left gutter: the reference's white cap that fills with
/// the lane accent, sinks 1.5 px, and gains a ring while its key is held.
fn draw_keycap(frame: &mut Frame, axis: &Axis, row: usize, lane: &Lane, name: &str, held: bool) {
    let x = ((LABEL_GUTTER - CAP_SIZE) / 2.0).round();
    let y = axis.track(row).y + ((LANE_HEIGHT - CAP_SIZE) / 2.0).round();
    let surface_y = if held { y + 1.5 } else { y };
    let cap = |top: f32, inset: f32, radius: f32| {
        Path::new(|path| {
            path.rounded_rectangle(
                Point::new(x - inset, top - inset),
                Size::new(CAP_SIZE + 2.0 * inset, CAP_SIZE + 2.0 * inset),
                radius.into(),
            );
        })
    };
    if held {
        frame.stroke(
            &cap(surface_y, 2.0, CAP_RADIUS + 2.0),
            Stroke::default().with_color(lane.held_ring).with_width(2.5),
        );
        frame.fill(&cap(surface_y, 0.0, CAP_RADIUS), lane.signal);
        frame.stroke(
            &cap(surface_y, 0.0, CAP_RADIUS),
            Stroke::default()
                .with_color(lane.held_border)
                .with_width(1.5),
        );
    } else {
        frame.fill(&cap(y + 1.5, 0.0, CAP_RADIUS), CAP_SHADOW);
        frame.fill(&cap(y, 0.0, CAP_RADIUS), CAP_SURFACE);
        frame.stroke(
            &cap(y, 0.0, CAP_RADIUS),
            Stroke::default().with_color(LANE_BORDER).with_width(1.0),
        );
    }
    let ink = if held { Color::WHITE } else { CAP_TEXT };
    let center_x = x + CAP_SIZE / 2.0;
    let lines = key_label_lines(name);
    // Shrink the label until the widest line clears the cap's 4 px padding,
    // so a long key name stays inside the cap instead of bleeding over it.
    let mut size = if lines.len() > 1 {
        10.0
    } else if name.chars().count() <= 2 {
        14.0
    } else {
        11.0
    };
    while size > 6.0
        && lines
            .iter()
            .any(|line| text_width(line, size) > CAP_SIZE - 8.0)
    {
        size -= 0.5;
    }
    let baseline = surface_y + 16.5;
    let bold = crate::ui::theme::UI_FONT_BOLD;
    if let [single] = lines.as_slice() {
        frame.fill_text(centered(
            single.clone(),
            Point::new(center_x, baseline),
            size,
            ink,
            bold,
        ));
    } else {
        let step = (size + 0.5) / 2.0;
        for (index, line) in lines.iter().enumerate() {
            let offset = if index == 0 { -step } else { step };
            frame.fill_text(centered(
                line.clone(),
                Point::new(center_x, baseline + offset),
                size,
                ink,
                bold,
            ));
        }
    }
    draw_arrow(
        frame,
        Point::new(center_x, surface_y + 29.5),
        lane.arrow,
        ink,
    );
}

/// The keycap's direction glyph: a shaft with a two-stroke head.
fn draw_arrow(frame: &mut Frame, center: Point, arrow: Arrow, color: Color) {
    let (dx, dy) = match arrow {
        Arrow::Up => (0.0, -1.0),
        Arrow::Down => (0.0, 1.0),
        Arrow::Left => (-1.0, 0.0),
        Arrow::Right => (1.0, 0.0),
    };
    let tip = Point::new(center.x + dx * 3.2, center.y + dy * 3.2);
    let tail = Point::new(center.x - dx * 3.2, center.y - dy * 3.2);
    // The head's wings sit perpendicular to the shaft, so the normal is the
    // direction vector rotated a quarter turn.
    let path = Path::new(|path| {
        path.move_to(tail);
        path.line_to(tip);
        for side in [-1.0_f32, 1.0] {
            path.move_to(Point::new(
                tip.x - dx * 2.5 + dy * 2.5 * side,
                tip.y - dy * 2.5 + dx * 2.5 * side,
            ));
            path.line_to(tip);
        }
    });
    frame.stroke(
        &path,
        Stroke::default()
            .with_color(color)
            .with_width(1.4)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round),
    );
}

/// Overlap columns per axis pair, with a millisecond badge above the
/// vertical pair and below the horizontal pair (reference placement).
fn draw_overlaps(frame: &mut Frame, axis: &Axis, timeline: &Timeline) {
    let width = frame.width();
    for (pair, badge_above) in [(0usize, true), (2usize, false)] {
        let top = lane_y(pair);
        for (first_start, first_end, first_open) in timeline.spans(pair, axis.now) {
            for (second_start, second_end, second_open) in timeline.spans(pair + 1, axis.now) {
                let start = first_start.max(second_start);
                let end = first_end.min(second_end);
                if end.saturating_sub(start) < 1_000 {
                    continue;
                }
                let Some((x0, x1)) = axis.visible(start, end) else {
                    continue;
                };
                let span = (x1 - x0).max(3.0);
                let live = first_open && second_open;
                frame.fill_rectangle(
                    Point::new(x0, top),
                    Size::new(span, PAIR_HEIGHT),
                    if live {
                        OVERLAP_FILL_LIVE
                    } else {
                        OVERLAP_FILL
                    },
                );
                let edge = Stroke::default().with_color(OVERLAP_EDGE);
                frame.stroke(
                    &Path::new(|path| {
                        path.move_to(Point::new(x0 + 0.5, top));
                        path.line_to(Point::new(x0 + 0.5, top + PAIR_HEIGHT));
                        path.move_to(Point::new(x1 - 0.5, top));
                        path.line_to(Point::new(x1 - 0.5, top + PAIR_HEIGHT));
                    }),
                    edge.with_width(1.0),
                );
                frame.stroke(
                    &Path::new(|path| {
                        path.move_to(Point::new(x0, top + 0.5));
                        path.line_to(Point::new(x1, top + 0.5));
                        path.move_to(Point::new(x0, top + PAIR_HEIGHT - 0.5));
                        path.line_to(Point::new(x1, top + PAIR_HEIGHT - 0.5));
                    }),
                    edge.with_width(1.5),
                );
                let label = millis(end.saturating_sub(start));
                let badge_w = text_width(&label, 9.0) + 10.0;
                // Keep the badge on screen when the overlap sits at an edge.
                let center =
                    ((x0 + x1) / 2.0).clamp(badge_w / 2.0 + 2.0, width - badge_w / 2.0 - 2.0);
                let badge_x = (center - badge_w / 2.0).round();
                let badge_y = if badge_above {
                    (top - BADGE_HEIGHT - 4.0).round()
                } else {
                    (top + PAIR_HEIGHT + 4.0).round()
                };
                if span >= 48.0 {
                    let dimension_y = (badge_y + BADGE_HEIGHT / 2.0).round();
                    let rule = Stroke::default().with_color(OVERLAP_EDGE).with_width(1.0);
                    for (from, to) in [
                        (x0 + 2.0, badge_x - 2.0),
                        (badge_x + badge_w + 2.0, x1 - 2.0),
                    ] {
                        if to - from > 5.0 {
                            frame.stroke(
                                &Path::new(|path| {
                                    path.move_to(Point::new(from, dimension_y));
                                    path.line_to(Point::new(to, dimension_y));
                                }),
                                rule,
                            );
                        }
                    }
                }
                let badge = Path::new(|path| {
                    path.rounded_rectangle(
                        Point::new(badge_x, badge_y),
                        Size::new(badge_w, BADGE_HEIGHT),
                        4.0.into(),
                    );
                });
                frame.fill(&badge, BADGE_FILL);
                frame.stroke(
                    &badge,
                    Stroke::default().with_color(BADGE_EDGE).with_width(1.0),
                );
                frame.fill_text(centered(
                    label,
                    Point::new(center.round(), badge_y + BADGE_HEIGHT / 2.0),
                    9.0,
                    Color::WHITE,
                    crate::ui::theme::UI_FONT_BOLD,
                ));
                if live {
                    let dot_y = if badge_above {
                        top - 7.0
                    } else {
                        top + PAIR_HEIGHT + 7.0
                    };
                    frame.fill(&Path::circle(Point::new(x1, dot_y), 2.5), NEEDLE);
                }
            }
        }
    }
}

/// The `NOW` playhead, spanning the tracks without entering the badge margins.
fn draw_needle(frame: &mut Frame, axis: &Axis) {
    frame.stroke(
        &Path::new(|path| {
            path.move_to(Point::new(axis.now_x, TOP_MARGIN));
            path.line_to(Point::new(axis.now_x, lane_y(3) + LANE_HEIGHT));
        }),
        Stroke::default()
            .with_color(NEEDLE)
            .with_width(1.5)
            .with_line_cap(LineCap::Round),
    );
}

/// One ruler label per tick, placed inline on the center axis with connecting
/// segments drawn only in the gaps between them (reference: `-1000ms` pinned
/// to the track start, `NOW (0ms)` parked just before the playhead).
fn draw_ruler(frame: &mut Frame, axis: &Axis) {
    struct Tick {
        label: String,
        left: f32,
        right: f32,
        color: Color,
        font: Font,
    }
    let y = TOP_MARGIN + PAIR_HEIGHT + RULER_BLOCK / 2.0;
    let mut ticks: Vec<Tick> = Vec::new();
    let mut at = WINDOW_MICROS;
    loop {
        let label = if at == 0 {
            "NOW (0ms)".to_owned()
        } else {
            format!("-{}ms", at / 1_000)
        };
        let text_w = text_width(&label, 10.0);
        let (left, color, font) = if at == 0 {
            (
                axis.now_x - 14.0 - text_w,
                NEEDLE,
                crate::ui::theme::UI_FONT_BOLD,
            )
        } else if at == WINDOW_MICROS {
            (axis.left + 2.0, RULER_TEXT, crate::ui::theme::UI_FONT)
        } else {
            let center = axis.left + axis.width * (1.0 - at as f32 / WINDOW_MICROS as f32);
            (center - text_w / 2.0, RULER_TEXT, crate::ui::theme::UI_FONT)
        };
        ticks.push(Tick {
            label,
            left,
            right: left + text_w,
            color,
            font,
        });
        if at == 0 {
            break;
        }
        at = at.saturating_sub(RULER_STEP_MICROS);
    }
    let rule = Stroke::default().with_color(RULER_LINE).with_width(1.0);
    let mut segment = |from: f32, to: f32| {
        if to > from {
            frame.stroke(
                &Path::new(|path| {
                    path.move_to(Point::new(from, y));
                    path.line_to(Point::new(to, y));
                }),
                rule,
            );
        }
    };
    const LABEL_PAD: f32 = 8.0;
    for pair in ticks.windows(2) {
        segment(pair[0].right + LABEL_PAD, pair[1].left - LABEL_PAD);
    }
    if let Some(last) = ticks.last() {
        segment(last.right + LABEL_PAD, axis.now_x);
    }
    for tick in ticks {
        let center = ((tick.left + tick.right) / 2.0).round();
        frame.fill_text(centered(
            tick.label,
            Point::new(center, y),
            10.0,
            tick.color,
            tick.font,
        ));
    }
}

/// Canvas text centered on a point. Advanced shaping keeps Hangul and CJK key
/// names legible through the generic UI font's fallback chain.
fn centered(content: String, position: Point, size: f32, color: Color, font: Font) -> Text {
    Text {
        content,
        position,
        max_width: f32::INFINITY,
        color,
        size: size.into(),
        line_height: LineHeight::Relative(1.0),
        font,
        align_x: Alignment::Center,
        align_y: Vertical::Center,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::None,
        ellipsis: Ellipsis::None,
    }
}

fn millis(micros: u64) -> String {
    format!("{:.1}ms", micros as f32 / 1_000.0)
}

/// Advance estimate for canvas text, which the pinned `Frame` cannot measure.
/// Used only to decide whether a label fits, so an approximation that never
/// underestimates the common case is enough; wide CJK glyphs count double.
fn text_width(content: &str, size: f32) -> f32 {
    content
        .chars()
        .map(|glyph| if glyph.is_ascii() { 0.58 } else { 1.0 })
        .sum::<f32>()
        * size
}

/// The reference's keycap label split: names that do not fit on one line
/// break at their prefix, so `Up Arrow` reads as `UP` over `ARROW`.
fn key_label_lines(name: &str) -> Vec<String> {
    let upper = name.trim().to_uppercase();
    if upper.is_empty() {
        return vec!["-".to_owned()];
    }
    for prefix in ["ARROW", "NUMPAD", "PAGE", "LEFT", "RIGHT"] {
        if let Some(rest) = upper.strip_prefix(prefix)
            && !rest.is_empty()
        {
            return vec![prefix.to_owned(), rest.trim().to_owned()];
        }
    }
    if let Some((head, tail)) = upper.split_once(char::is_whitespace) {
        return vec![head.to_owned(), tail.trim().to_owned()];
    }
    match upper.as_str() {
        "BACKSPACE" => vec!["BACK".to_owned(), "SPACE".to_owned()],
        "CAPSLOCK" => vec!["CAPS".to_owned(), "LOCK".to_owned()],
        _ => vec![upper],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_keeps_running_after_the_last_key_is_released() {
        let output = |key, pressed, at| MonitorSnapshot {
            elapsed_micros: at,
            filter_enabled: true,
            physical: None,
            outputs: vec![MonitorEdge {
                key,
                pressed,
                synthetic: true,
            }],
            decision: MonitorDecision::Immediate,
        };
        let key = KeySlot::HorizontalFirst;
        let mut timeline = Timeline::default();
        // The session started well before the first key: `elapsed_micros` is
        // already large when the anchor is taken.
        let arrival = Instant::now();
        timeline.accept(output(key, true, 9_000_000), false, arrival);
        timeline.accept(output(key, false, 9_100_000), false, arrival);
        let last_end = 9_100_000;
        assert!(
            timeline.now().saturating_sub(last_end) < WINDOW_MICROS,
            "a just-released span must still be inside the window, so the graph keeps animating"
        );
    }

    #[test]
    fn a_released_key_keeps_its_span_while_the_next_key_is_pressed() {
        let output = |key, pressed, at| MonitorSnapshot {
            elapsed_micros: at,
            filter_enabled: true,
            physical: None,
            outputs: vec![MonitorEdge {
                key,
                pressed,
                synthetic: true,
            }],
            decision: MonitorDecision::Immediate,
        };
        let mut timeline = Timeline::default();
        let arrival = Instant::now();
        let first = KeySlot::HorizontalFirst;
        let second = KeySlot::HorizontalSecond;
        timeline.accept(output(first, true, 0), false, arrival);
        timeline.accept(output(first, false, 100_000), false, arrival);
        timeline.accept(output(second, true, 150_000), false, arrival);
        let now = 200_000;
        assert_eq!(
            timeline.spans(index(first), now),
            vec![(0, 100_000, false)],
            "the released key keeps its completed span inside the window"
        );
        assert_eq!(
            timeline.spans(index(second), now),
            vec![(150_000, now, true)]
        );
    }

    #[test]
    fn a_socd_handover_keeps_both_lanes_visible() {
        // One engine event can carry the old key's release and the new key's
        // press together; both lanes must survive it.
        let handover = MonitorSnapshot {
            elapsed_micros: 150_000,
            filter_enabled: true,
            physical: None,
            outputs: vec![
                MonitorEdge {
                    key: KeySlot::HorizontalFirst,
                    pressed: false,
                    synthetic: true,
                },
                MonitorEdge {
                    key: KeySlot::HorizontalSecond,
                    pressed: true,
                    synthetic: true,
                },
            ],
            decision: MonitorDecision::Immediate,
        };
        let mut timeline = Timeline::default();
        let arrival = Instant::now();
        timeline.accept(
            MonitorSnapshot {
                elapsed_micros: 0,
                outputs: vec![MonitorEdge {
                    key: KeySlot::HorizontalFirst,
                    pressed: true,
                    synthetic: true,
                }],
                ..handover.clone()
            },
            false,
            arrival,
        );
        timeline.accept(handover, false, arrival);
        let now = 200_000;
        assert_eq!(
            timeline.spans(index(KeySlot::HorizontalFirst), now),
            vec![(0, 150_000, false)],
            "the handed-over key keeps its completed span"
        );
        assert_eq!(
            timeline.spans(index(KeySlot::HorizontalSecond), now),
            vec![(150_000, now, true)]
        );
    }

    #[test]
    fn display_uses_engine_output_and_switches_to_physical_only_when_requested() {
        let key = KeySlot::HorizontalFirst;
        let edge = MonitorEdge {
            key,
            pressed: true,
            synthetic: false,
        };
        let mut event = MonitorSnapshot {
            elapsed_micros: 50,
            filter_enabled: true,
            physical: Some(edge),
            outputs: vec![],
            decision: MonitorDecision::PressDelayed { delay_micros: 2000 },
        };
        let mut timeline = Timeline::default();
        let now = Instant::now();
        timeline.accept(event.clone(), false, now);
        assert!(
            !timeline.held(key),
            "a delayed physical press is not an output press"
        );
        event.elapsed_micros = 2050;
        event.physical = None;
        event.outputs.push(MonitorEdge {
            synthetic: true,
            ..edge
        });
        timeline.accept(event.clone(), false, now);
        assert!(timeline.held(key));
        timeline.clear();
        assert!(!timeline.held(key));
        event.outputs.clear();
        event.physical = Some(edge);
        event.filter_enabled = false;
        timeline.accept(event, false, now);
        assert!(timeline.held(key));
        assert!(timeline.physical);
    }

    #[test]
    fn the_later_press_wins_the_dpad_dot() {
        let press = |key, at| MonitorSnapshot {
            elapsed_micros: at,
            filter_enabled: false,
            physical: Some(MonitorEdge {
                key,
                pressed: true,
                synthetic: false,
            }),
            outputs: vec![],
            decision: MonitorDecision::Immediate,
        };
        let mut timeline = Timeline::default();
        let now = Instant::now();
        assert_eq!(
            timeline.winner(KeySlot::HorizontalFirst, KeySlot::HorizontalSecond),
            None
        );
        timeline.accept(press(KeySlot::HorizontalFirst, 50), false, now);
        assert_eq!(
            timeline.winner(KeySlot::HorizontalFirst, KeySlot::HorizontalSecond),
            Some(KeySlot::HorizontalFirst)
        );
        timeline.accept(press(KeySlot::HorizontalSecond, 80), false, now);
        assert_eq!(
            timeline.winner(KeySlot::HorizontalFirst, KeySlot::HorizontalSecond),
            Some(KeySlot::HorizontalSecond)
        );
        let mut release_first = press(KeySlot::HorizontalFirst, 120);
        release_first.physical.as_mut().unwrap().pressed = false;
        timeline.accept(release_first, false, now);
        assert_eq!(
            timeline.winner(KeySlot::HorizontalFirst, KeySlot::HorizontalSecond),
            Some(KeySlot::HorizontalSecond)
        );
    }
}
