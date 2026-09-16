//! The monitor timeline's data model. Framework-free on purpose: the painter
//! that draws it (T5) is added beside this, and the state layer depends only
//! on what is here.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use egui::{
    Color32, CornerRadius, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2,
    WidgetInfo, WidgetType,
};

use super::{language::Language, message::Message, theme};
use crate::protocol::{KeySlot, MonitorDecision, MonitorEdge, MonitorSnapshot};

/// How much history the graph keeps on screen.
pub const WINDOW_MICROS: u64 = 1_000_000;
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

    pub fn now(&self) -> u64 {
        self.anchor.map_or(0, |(arrival, offset)| {
            offset.saturating_add(arrival.elapsed().as_micros() as u64)
        })
    }

    pub fn held(&self, key: KeySlot) -> bool {
        self.held_since[index(key)].is_some()
    }

    pub fn held_mask(&self) -> [bool; 4] {
        std::array::from_fn(|key| self.held_since[key].is_some())
    }

    /// Visible `(start, end, still_held)` spans for one lane. A held key ends
    /// at `now`, which is what makes its block and any overlap grow live.
    pub fn spans(&self, key: usize, now: u64) -> Vec<(u64, u64, bool)> {
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

pub const fn index(key: KeySlot) -> usize {
    match key {
        KeySlot::VerticalFirst => 0,
        KeySlot::VerticalSecond => 1,
        KeySlot::HorizontalFirst => 2,
        KeySlot::HorizontalSecond => 3,
    }
}

// ---------------------------------------------------------------------------
// Graph painter
// ---------------------------------------------------------------------------

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
    signal: Color32,
    held_fill: Color32,
    held_border: Color32,
    held_ring: Color32,
    arrow: Arrow,
}

// The Iced source writes float alphas (`Color::from_rgba8(.., 0.12)`); egui
// stores premultiplied bytes, so each alpha is rounded to the nearest byte
// (0.12 * 255 = 30.6 -> 31). The colour channels are the Iced bytes verbatim.
const LANES: [Lane; 4] = [
    Lane {
        signal: Color32::from_rgb(37, 99, 235),
        held_fill: Color32::from_rgba_unmultiplied_const(37, 99, 235, 31),
        held_border: Color32::from_rgb(29, 78, 216),
        held_ring: Color32::from_rgba_unmultiplied_const(191, 219, 254, 230),
        arrow: Arrow::Up,
    },
    Lane {
        signal: Color32::from_rgb(124, 58, 237),
        held_fill: Color32::from_rgba_unmultiplied_const(139, 92, 246, 31),
        held_border: Color32::from_rgb(109, 40, 217),
        held_ring: Color32::from_rgba_unmultiplied_const(221, 214, 254, 230),
        arrow: Arrow::Down,
    },
    Lane {
        signal: Color32::from_rgb(79, 70, 229),
        held_fill: Color32::from_rgba_unmultiplied_const(99, 102, 241, 31),
        held_border: Color32::from_rgb(67, 56, 202),
        held_ring: Color32::from_rgba_unmultiplied_const(199, 210, 254, 230),
        arrow: Arrow::Left,
    },
    Lane {
        signal: Color32::from_rgb(147, 51, 234),
        held_fill: Color32::from_rgba_unmultiplied_const(147, 51, 234, 31),
        held_border: Color32::from_rgb(126, 34, 206),
        held_ring: Color32::from_rgba_unmultiplied_const(233, 213, 255, 230),
        arrow: Arrow::Right,
    },
];

const LANE_IDLE: Color32 = Color32::from_rgb(248, 250, 252);
const LANE_BORDER: Color32 = Color32::from_rgb(226, 232, 240);
const CAP_SURFACE: Color32 = Color32::WHITE;
const CAP_SHADOW: Color32 = Color32::from_rgba_unmultiplied_const(0, 0, 0, 13);
const CAP_TEXT: Color32 = Color32::from_rgb(15, 23, 42);
const OVERLAP_FILL: Color32 = Color32::from_rgba_unmultiplied_const(99, 102, 241, 46);
/// A still-open overlap reads stronger than a settled one.
const OVERLAP_FILL_LIVE: Color32 = Color32::from_rgba_unmultiplied_const(99, 102, 241, 66);
const OVERLAP_EDGE: Color32 = Color32::from_rgb(79, 70, 229);
const BADGE_FILL: Color32 = Color32::from_rgb(30, 27, 75);
const BADGE_EDGE: Color32 = Color32::from_rgb(129, 140, 248);
const RULER_LINE: Color32 = Color32::from_rgb(203, 213, 225);
const RULER_TEXT: Color32 = Color32::from_rgb(71, 85, 105);
const NEEDLE: Color32 = Color32::from_rgb(79, 70, 229);

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

/// Paint the graph into the current layout and return its container response.
///
/// Display-only: the whole canvas publishes one container-level accessible
/// label (the card title) so a tree dump can find it, and carries no
/// interactive node. The graph renders its lanes, keycaps, and ruler even
/// without a timeline, so the card does not flash empty between the start
/// request and the first monitor event; blocks, overlaps, and the needle
/// mount on top once data arrives. `awake` mirrors the window's focus: a
/// deactivated window requests no animation frames.
pub fn graph(
    ui: &mut Ui,
    timeline: Option<&Timeline>,
    names: [&str; 4],
    language: Language,
    awake: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), GRAPH_HEIGHT),
        Sense::hover(),
    );
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Other,
            ui.is_enabled(),
            language.text("Key Input Timeline"),
        )
    });

    let now = timeline.map_or(0, Timeline::now);
    let painter = ui.painter_at(rect);
    draw_graph(&painter, rect, timeline, &names, now);

    let animate = timeline.is_some_and(|timeline| {
        timeline.held_since.iter().any(Option::is_some)
            || timeline
                .intervals
                .back()
                .is_some_and(|interval| now.saturating_sub(interval.end) < WINDOW_MICROS)
    });
    if awake && animate {
        ui.ctx().request_repaint_after(Duration::from_millis(16));
    }
    response
}

/// The whole graph in one pass. A stopped timeline still draws its lanes,
/// keycaps, and ruler so the card does not flash empty between the start
/// request and the first monitor event.
fn draw_graph(
    painter: &Painter,
    rect: Rect,
    timeline: Option<&Timeline>,
    names: &[&str; 4],
    now: u64,
) {
    let axis = Axis::new(rect, now);
    for (row, lane) in LANES.iter().enumerate() {
        let held = timeline.is_some_and(|timeline| timeline.held_since[row].is_some());
        draw_track(painter, rect, &axis, row, lane, held);
        draw_keycap(painter, rect, &axis, row, lane, names[row], held);
    }
    if let Some(timeline) = timeline {
        for (row, lane) in LANES.iter().enumerate() {
            draw_blocks(painter, rect, &axis, timeline, row, lane);
        }
        draw_overlaps(painter, rect, &axis, timeline);
    }
    draw_needle(painter, rect, &axis);
    draw_ruler(painter, rect, &axis);
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
    fn new(rect: Rect, now: u64) -> Self {
        let left = rect.left() + LABEL_GUTTER;
        let width = (rect.width() - LABEL_GUTTER - RIGHT_PAD).max(80.0);
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

    fn track(&self, rect: Rect, row: usize) -> Rect {
        Rect::from_min_size(
            Pos2::new(self.left, rect.top() + lane_y(row)),
            Vec2::new(self.width, LANE_HEIGHT),
        )
    }
}

/// One lane's rounded background, its clipped time grid, and its border.
/// A held lane takes the reference's accent tint and a heavier border.
fn draw_track(painter: &Painter, rect: Rect, axis: &Axis, row: usize, lane: &Lane, held: bool) {
    let track = axis.track(rect, row);
    let radius = CornerRadius::same(LANE_RADIUS as u8);
    painter.rect_filled(track, radius, if held { lane.held_fill } else { LANE_IDLE });
    let clipped = painter.with_clip_rect(track);
    let grid = Stroke::new(1.0, LANE_BORDER);
    let mut tick = RULER_STEP_MICROS;
    while tick < WINDOW_MICROS {
        let x = axis.left + axis.width * (1.0 - tick as f32 / WINDOW_MICROS as f32);
        clipped.line_segment(
            [Pos2::new(x, track.top()), Pos2::new(x, track.bottom())],
            grid,
        );
        tick += RULER_STEP_MICROS;
    }
    painter.rect_stroke(
        track,
        radius,
        Stroke::new(
            if held { 1.5 } else { 1.0 },
            if held { lane.held_border } else { LANE_BORDER },
        ),
        egui::StrokeKind::Middle,
    );
}

/// Signal blocks for one lane, clipped to its track, each carrying its hold
/// duration when the block is wide enough to hold the label.
fn draw_blocks(
    painter: &Painter,
    rect: Rect,
    axis: &Axis,
    timeline: &Timeline,
    row: usize,
    lane: &Lane,
) {
    let track = axis.track(rect, row);
    let clipped = painter.with_clip_rect(track);
    for (start, end, open) in timeline.spans(row, axis.now) {
        let Some((x0, x1)) = axis.visible(start, end) else {
            continue;
        };
        let width = (x1 - x0).max(4.0);
        // The Iced block radius is 5.5; egui stores integer corner radii.
        let block = Rect::from_min_size(
            Pos2::new(x0, track.top() + 4.0),
            Vec2::new(width, track.height() - 8.0),
        );
        let radius = CornerRadius::same(6);
        clipped.rect_filled(block, radius, lane.signal);
        if open {
            clipped.rect_stroke(
                block,
                radius,
                Stroke::new(1.5, Color32::WHITE),
                egui::StrokeKind::Middle,
            );
        }
        let label = millis(end.saturating_sub(start));
        let size = 9.0;
        let galley = clipped.layout_no_wrap(
            label.clone(),
            FontId::new(size, theme::UI_FONT),
            Color32::PLACEHOLDER,
        );
        if width >= galley.size().x + 6.0 {
            let center = Pos2::new(x0 + width / 2.0, track.center().y);
            theme::stamp_galley(
                &clipped,
                center - galley.size() / 2.0,
                &galley,
                Color32::WHITE,
                size,
            );
        }
    }
}

/// The keycap in the left gutter: the reference's white cap that fills with
/// the lane accent, sinks 1.5 px, and gains a ring while its key is held.
fn draw_keycap(
    painter: &Painter,
    rect: Rect,
    axis: &Axis,
    row: usize,
    lane: &Lane,
    name: &str,
    held: bool,
) {
    let x = rect.left() + ((LABEL_GUTTER - CAP_SIZE) / 2.0).round();
    let y = axis.track(rect, row).top() + ((LANE_HEIGHT - CAP_SIZE) / 2.0).round();
    let surface_y = if held { y + 1.5 } else { y };
    let cap = |top: f32, inset: f32| {
        Rect::from_min_size(
            Pos2::new(x - inset, top - inset),
            Vec2::splat(CAP_SIZE + 2.0 * inset),
        )
    };
    let radius = CornerRadius::same(CAP_RADIUS as u8);
    if held {
        // The ring is the rounded rect's stroke offset 2px outward.
        painter.rect_stroke(
            cap(surface_y, 2.0),
            CornerRadius::same((CAP_RADIUS + 2.0) as u8),
            Stroke::new(2.5, lane.held_ring),
            egui::StrokeKind::Middle,
        );
        painter.rect_filled(cap(surface_y, 0.0), radius, lane.signal);
        painter.rect_stroke(
            cap(surface_y, 0.0),
            radius,
            Stroke::new(1.5, lane.held_border),
            egui::StrokeKind::Middle,
        );
    } else {
        painter.rect_filled(cap(y + 1.5, 0.0), radius, CAP_SHADOW);
        painter.rect_filled(cap(y, 0.0), radius, CAP_SURFACE);
        painter.rect_stroke(
            cap(y, 0.0),
            radius,
            Stroke::new(1.0, LANE_BORDER),
            egui::StrokeKind::Middle,
        );
    }
    let ink = if held { Color32::WHITE } else { CAP_TEXT };
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
            .any(|line| text_width(painter, line, size) > CAP_SIZE - 8.0)
    {
        size -= 0.5;
    }
    let baseline = surface_y + 16.5;
    if let [single] = lines.as_slice() {
        centered_text(painter, Pos2::new(center_x, baseline), single, size, ink);
    } else {
        let step = (size + 0.5) / 2.0;
        for (index, line) in lines.iter().enumerate() {
            let offset = if index == 0 { -step } else { step };
            centered_text(
                painter,
                Pos2::new(center_x, baseline + offset),
                line,
                size,
                ink,
            );
        }
    }
    draw_arrow(
        painter,
        Pos2::new(center_x, surface_y + 29.5),
        lane.arrow,
        ink,
    );
}

/// The keycap's direction glyph: a shaft with a two-stroke head.
fn draw_arrow(painter: &Painter, center: Pos2, arrow: Arrow, color: Color32) {
    let (dx, dy) = match arrow {
        Arrow::Up => (0.0, -1.0),
        Arrow::Down => (0.0, 1.0),
        Arrow::Left => (-1.0, 0.0),
        Arrow::Right => (1.0, 0.0),
    };
    let tip = Pos2::new(center.x + dx * 3.2, center.y + dy * 3.2);
    let tail = Pos2::new(center.x - dx * 3.2, center.y - dy * 3.2);
    let stroke = Stroke::new(1.4, color);
    painter.line_segment([tail, tip], stroke);
    // The head's wings sit perpendicular to the shaft, so the normal is the
    // direction vector rotated a quarter turn.
    for side in [-1.0_f32, 1.0] {
        painter.line_segment(
            [
                Pos2::new(
                    tip.x - dx * 2.5 + dy * 2.5 * side,
                    tip.y - dy * 2.5 + dx * 2.5 * side,
                ),
                tip,
            ],
            stroke,
        );
    }
}

/// Overlap columns per axis pair, with a millisecond badge above the
/// vertical pair and below the horizontal pair (reference placement).
fn draw_overlaps(painter: &Painter, rect: Rect, axis: &Axis, timeline: &Timeline) {
    let width = rect.width();
    for (pair, badge_above) in [(0usize, true), (2usize, false)] {
        let top = rect.top() + lane_y(pair);
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
                painter.rect_filled(
                    Rect::from_min_size(Pos2::new(x0, top), Vec2::new(span, PAIR_HEIGHT)),
                    CornerRadius::ZERO,
                    if live {
                        OVERLAP_FILL_LIVE
                    } else {
                        OVERLAP_FILL
                    },
                );
                let edge = Stroke::new(1.0, OVERLAP_EDGE);
                painter.line_segment(
                    [
                        Pos2::new(x0 + 0.5, top),
                        Pos2::new(x0 + 0.5, top + PAIR_HEIGHT),
                    ],
                    edge,
                );
                painter.line_segment(
                    [
                        Pos2::new(x1 - 0.5, top),
                        Pos2::new(x1 - 0.5, top + PAIR_HEIGHT),
                    ],
                    edge,
                );
                let long_edge = Stroke::new(1.5, OVERLAP_EDGE);
                for y in [top + 0.5, top + PAIR_HEIGHT - 0.5] {
                    painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], long_edge);
                }
                let label = millis(end.saturating_sub(start));
                let size = 9.0;
                let galley = painter.layout_no_wrap(
                    label.clone(),
                    FontId::new(size, theme::UI_FONT),
                    Color32::PLACEHOLDER,
                );
                let badge_w = galley.size().x + 10.0;
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
                    let rule = Stroke::new(1.0, OVERLAP_EDGE);
                    for (from, to) in [
                        (x0 + 2.0, badge_x - 2.0),
                        (badge_x + badge_w + 2.0, x1 - 2.0),
                    ] {
                        if to - from > 5.0 {
                            painter.line_segment(
                                [Pos2::new(from, dimension_y), Pos2::new(to, dimension_y)],
                                rule,
                            );
                        }
                    }
                }
                let badge = Rect::from_min_size(
                    Pos2::new(badge_x, badge_y),
                    Vec2::new(badge_w, BADGE_HEIGHT),
                );
                let radius = CornerRadius::same(4);
                painter.rect_filled(badge, radius, BADGE_FILL);
                painter.rect_stroke(
                    badge,
                    radius,
                    Stroke::new(1.0, BADGE_EDGE),
                    egui::StrokeKind::Middle,
                );
                theme::stamp_galley(
                    painter,
                    Pos2::new(center.round(), badge_y + BADGE_HEIGHT / 2.0) - galley.size() / 2.0,
                    &galley,
                    Color32::WHITE,
                    size,
                );
                if live {
                    let dot_y = if badge_above {
                        top - 7.0
                    } else {
                        top + PAIR_HEIGHT + 7.0
                    };
                    painter.circle_filled(Pos2::new(x1, dot_y), 2.5, NEEDLE);
                }
            }
        }
    }
}

/// The `NOW` playhead, spanning the tracks without entering the badge margins.
fn draw_needle(painter: &Painter, rect: Rect, axis: &Axis) {
    painter.line_segment(
        [
            Pos2::new(axis.now_x, rect.top() + TOP_MARGIN),
            Pos2::new(axis.now_x, rect.top() + lane_y(3) + LANE_HEIGHT),
        ],
        Stroke::new(1.5, NEEDLE),
    );
}

/// One ruler label per tick, placed inline on the center axis with connecting
/// segments drawn only in the gaps between them (reference: `-1000ms` pinned
/// to the track start, `NOW (0ms)` parked just before the playhead).
fn draw_ruler(painter: &Painter, rect: Rect, axis: &Axis) {
    struct Tick {
        label: String,
        left: f32,
        right: f32,
        color: Color32,
        bold: bool,
    }
    let y = rect.top() + TOP_MARGIN + PAIR_HEIGHT + RULER_BLOCK / 2.0;
    let mut ticks: Vec<Tick> = Vec::new();
    let mut at = WINDOW_MICROS;
    loop {
        let label = if at == 0 {
            "NOW (0ms)".to_owned()
        } else {
            format!("-{}ms", at / 1_000)
        };
        let text_w = text_width(painter, &label, 10.0);
        let (left, color, bold) = if at == 0 {
            (axis.now_x - 14.0 - text_w, NEEDLE, true)
        } else if at == WINDOW_MICROS {
            (axis.left + 2.0, RULER_TEXT, false)
        } else {
            let center = axis.left + axis.width * (1.0 - at as f32 / WINDOW_MICROS as f32);
            (center - text_w / 2.0, RULER_TEXT, false)
        };
        ticks.push(Tick {
            label,
            left,
            right: left + text_w,
            color,
            bold,
        });
        if at == 0 {
            break;
        }
        at = at.saturating_sub(RULER_STEP_MICROS);
    }
    let rule = Stroke::new(1.0, RULER_LINE);
    let segment = |from: f32, to: f32| {
        if to > from {
            painter.line_segment([Pos2::new(from, y), Pos2::new(to, y)], rule);
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
        centered_text_with(
            painter,
            Pos2::new(center, y),
            &tick.label,
            10.0,
            tick.color,
            tick.bold,
        );
    }
}

/// A 16px header mark matching `icons::Name::Target`: an outer ring at 0.35
/// of the box with a filled centre at 35% of that radius. The shared
/// `theme::Icon` set does not carry it, and this card is its only consumer.
fn paint_target_icon(painter: &Painter, rect: Rect, color: Color32) {
    let size = rect.width().min(rect.height());
    painter.circle_stroke(
        rect.center(),
        size * 0.35,
        Stroke::new((size * 0.1).max(1.2), color),
    );
    painter.circle_filled(rect.center(), size * 0.35 * 0.35, color);
}

/// Canvas text centered on a point, drawn with the fake-bold stamp the port
/// uses where the Iced source selected `UI_FONT_BOLD`.
fn centered_text(painter: &Painter, center: Pos2, content: &str, size: f32, color: Color32) {
    centered_text_with(painter, center, content, size, color, true);
}

fn centered_text_with(
    painter: &Painter,
    center: Pos2,
    content: &str,
    size: f32,
    color: Color32,
    bold: bool,
) {
    let galley = painter.layout_no_wrap(
        content.to_owned(),
        FontId::new(size, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    let pos = center - galley.size() / 2.0;
    if bold {
        theme::stamp_galley(painter, pos, &galley, color, size);
    } else {
        painter.galley(pos, galley, color);
    }
}

/// The measured advance of `content` at `size`. The Iced canvas could not
/// measure text and used an approximation; egui's font layout can, so the
/// fit checks use the real advance.
fn text_width(painter: &Painter, content: &str, size: f32) -> f32 {
    painter
        .layout_no_wrap(
            content.to_owned(),
            FontId::new(size, theme::UI_FONT),
            Color32::PLACEHOLDER,
        )
        .size()
        .x
}

fn millis(micros: u64) -> String {
    format!("{:.1}ms", micros as f32 / 1_000.0)
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

// ---------------------------------------------------------------------------
// Key Input Timeline card
// ---------------------------------------------------------------------------

/// The Key Input Timeline card: header with the lifecycle subtitle and the
/// monitor switch, collapsed to just those while stopped and mounting the
/// graph only while recording (ui.md: a stopped timeline collapses to its
/// title, subtitle, and start control). `names` are the four mapped key
/// names, in slot order; the caller supplies them from the snapshot.
pub fn timeline_section(
    ui: &mut Ui,
    monitor: &MonitorState,
    names: [&str; 4],
    language: Language,
    awake: bool,
    messages: &mut Vec<Message>,
) -> Response {
    let label = match monitor {
        MonitorState::Stopped => "Start timeline",
        MonitorState::Starting => "Starting…",
        MonitorState::Recording(_) => "Stop timeline",
        MonitorState::Stopping(_) => "Stopping…",
    };
    let ready = matches!(monitor, MonitorState::Stopped | MonitorState::Recording(_));
    let recording = matches!(monitor, MonitorState::Recording(_));
    // The subtitle carries the Starting / Stopping lifecycle the toggle
    // itself cannot express.
    let subtitle = if ready {
        "Shows how long each key is held and where it overlaps its opposite, live."
    } else {
        label
    };

    theme::card_style()
        .inner_margin(egui::Margin::same(theme::CARD_PADDING as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 12.0);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
                        let (icon_rect, _) =
                            ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                        paint_target_icon(ui.painter(), icon_rect, theme::PRIMARY_TEXT);
                        ui.colored_label(
                            theme::BODY_TEXT,
                            egui::RichText::new(language.text("Key Input Timeline"))
                                .font(FontId::new(theme::HEADING_SIZE, theme::UI_FONT))
                                .strong(),
                        );
                    });
                    ui.colored_label(
                        theme::MUTED_TEXT,
                        egui::RichText::new(language.text(subtitle))
                            .font(FontId::new(12.0, theme::UI_FONT)),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if monitor_switch(ui, recording, ready, language.text(label)) {
                        messages.push(Message::ToggleMonitor);
                    }
                });
            });

            if recording {
                theme::graph_frame().show(ui, |ui| {
                    graph(ui, monitor.timeline(), names, language, awake);
                });
            }
        })
        .response
}

/// The card's monitor toggle, ported from the Iced `toggler` at `.size(24)`
/// (iced rev f8127c8 `widget/src/toggler.rs`): a 48x24 pill, padding
/// `round(0.1 * 24) = 2`, and a 20px round knob that sits 2px from the left
/// when off and 2px from the right when on. The track takes [`theme::SLATE_300`]
/// until recording, then [`theme::INDIGO_600`]; the knob stays
/// [`theme::SURFACE`]. Paint-only: it publishes the checkbox node and hands
/// back whether it was clicked; the caller maps that to a `Message`.
fn monitor_switch(ui: &mut Ui, recording: bool, ready: bool, label: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(48.0, 24.0),
        if ready {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::Checkbox,
            ready && ui.is_enabled(),
            recording,
            label,
        )
    });
    if ready && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let track = if recording {
        theme::INDIGO_600
    } else {
        theme::SLATE_300
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(12), track);
    let knob_x = if recording {
        rect.right() - 22.0
    } else {
        rect.left() + 2.0
    };
    painter.circle_filled(
        Pos2::new(knob_x + 10.0, rect.center().y),
        10.0,
        theme::SURFACE,
    );
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{MonitorDecision, MonitorEdge, MonitorSnapshot};
    use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};

    fn output(key: KeySlot, pressed: bool, at: u64) -> MonitorSnapshot {
        MonitorSnapshot {
            elapsed_micros: at,
            filter_enabled: true,
            physical: None,
            outputs: vec![MonitorEdge {
                key,
                pressed,
                synthetic: true,
            }],
            decision: MonitorDecision::Immediate,
        }
    }

    /// Every `Shape::Rect` this frame painted, flattened out of `Shape::Vec`.
    fn painted_rects<State>(harness: &Harness<'_, State>) -> Vec<egui::epaint::RectShape> {
        fn collect(shape: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
            match shape {
                egui::Shape::Rect(rect) => out.push(rect.clone()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &harness.output().shapes {
            collect(&clipped.shape, &mut out);
        }
        out
    }

    /// Every text drawn this frame, in paint order.
    fn painted_texts<State>(harness: &Harness<'_, State>) -> Vec<String> {
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &harness.output().shapes {
            collect(&clipped.shape, &mut out);
        }
        out
    }

    #[test]
    fn the_stopped_graph_keeps_its_lanes_keycaps_and_ruler() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .build_ui(|ui| {
                graph(ui, None, ["W", "S", "A", "D"], Language::English, true);
            });
        harness.run();

        let lane = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.fill == LANE_IDLE)
            .expect("a stopped graph still paints its idle lanes");
        assert_eq!(lane.rect.height(), LANE_HEIGHT);
        assert_eq!(lane.corner_radius, CornerRadius::same(LANE_RADIUS as u8));

        let caps: Vec<_> = painted_rects(&harness)
            .into_iter()
            .filter(|rect| rect.fill == CAP_SURFACE && rect.rect.width() == CAP_SIZE)
            .collect();
        assert_eq!(caps.len(), 4, "one white keycap per lane: {caps:?}");

        assert!(
            painted_rects(&harness)
                .iter()
                .all(|rect| rect.fill != LANES[0].signal),
            "no signal block mounts without a timeline"
        );

        let texts = painted_texts(&harness);
        assert!(
            texts.iter().any(|text| text == "NOW (0ms)"),
            "texts: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "-1000ms"),
            "texts: {texts:?}"
        );

        // One container-level node, the size of the graph, and no more.
        let graph_node = harness.get_by_label("Key Input Timeline");
        assert_eq!(graph_node.rect().height(), GRAPH_HEIGHT);
    }

    /// The anchor `accept` takes fixes `now` at the first event's offset plus
    /// the wall-clock time since it arrived. Backdating the first arrival by a
    /// second parks `now` after the recorded spans without the test waiting.
    fn backdated() -> Instant {
        Instant::now() - Duration::from_secs(1)
    }

    #[test]
    fn recorded_spans_paint_blocks_in_their_lane() {
        let mut timeline = Timeline::default();
        timeline.accept(
            output(KeySlot::HorizontalFirst, true, 0),
            false,
            backdated(),
        );
        timeline.accept(
            output(KeySlot::HorizontalFirst, false, 100_000),
            false,
            Instant::now(),
        );
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .build_ui(move |ui| {
                graph(
                    ui,
                    Some(&timeline),
                    ["W", "S", "A", "D"],
                    Language::English,
                    true,
                );
            });
        harness.run_steps(2);

        let block = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.fill == LANES[2].signal)
            .expect("the released span paints a block in the A lane");
        assert_eq!(block.rect.height(), LANE_HEIGHT - 8.0);
        assert_eq!(block.corner_radius, CornerRadius::same(6));
        assert_eq!(block.stroke, Stroke::NONE, "a settled block has no stroke");
    }

    #[test]
    fn a_held_key_paints_a_held_lane_and_an_open_block() {
        let mut timeline = Timeline::default();
        timeline.accept(output(KeySlot::VerticalFirst, true, 0), false, backdated());
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .build_ui(move |ui| {
                graph(
                    ui,
                    Some(&timeline),
                    ["W", "S", "A", "D"],
                    Language::English,
                    true,
                );
            });
        harness.run_steps(2);

        assert!(
            painted_rects(&harness)
                .iter()
                .any(|rect| rect.fill == LANES[0].held_fill && rect.rect.height() == LANE_HEIGHT),
            "the held lane takes its accent tint"
        );
        let open = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.fill == LANES[0].signal && rect.rect.height() == LANE_HEIGHT - 8.0)
            .expect("the held span paints an open signal block");
        assert_eq!(open.stroke, Stroke::NONE);
        assert!(
            painted_rects(&harness).iter().any(|rect| {
                rect.stroke == Stroke::new(1.5, Color32::WHITE)
                    && rect.rect.height() == LANE_HEIGHT - 8.0
            }),
            "the open block carries the Iced white outline as its own stroke shape"
        );
    }

    #[test]
    fn opposing_overlaps_paint_a_live_column_and_badge() {
        let mut timeline = Timeline::default();
        timeline.accept(
            output(KeySlot::HorizontalFirst, true, 0),
            false,
            backdated(),
        );
        timeline.accept(
            output(KeySlot::HorizontalSecond, true, 50_000),
            false,
            Instant::now(),
        );
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .build_ui(move |ui| {
                graph(
                    ui,
                    Some(&timeline),
                    ["W", "S", "A", "D"],
                    Language::English,
                    true,
                );
            });
        harness.run_steps(2);

        let column = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.fill == OVERLAP_FILL_LIVE)
            .expect("two live spans paint the stronger overlap fill");
        assert_eq!(column.rect.height(), PAIR_HEIGHT);

        let badge = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.fill == BADGE_FILL)
            .expect("the overlap carries its duration badge");
        assert_eq!(badge.rect.height(), BADGE_HEIGHT);
        assert!(
            painted_rects(&harness).iter().any(|rect| {
                rect.stroke == Stroke::new(1.0, BADGE_EDGE) && rect.rect.height() == BADGE_HEIGHT
            }),
            "the badge paints its own edge stroke"
        );
    }

    // The data-model tests ported from `src/ui/timeline.rs`: the painter above
    // reads the same `now`/`spans`/`winner` contract.

    #[test]
    fn the_clock_keeps_running_after_the_last_key_is_released() {
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

    struct SectionState {
        monitor: MonitorState,
        messages: Vec<Message>,
    }

    fn section_harness(monitor: MonitorState) -> Harness<'static, SectionState> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .build_ui_state(
                |ui, state: &mut SectionState| {
                    let SectionState { monitor, messages } = state;
                    let _ = timeline_section(
                        ui,
                        monitor,
                        ["W", "S", "A", "D"],
                        Language::English,
                        true,
                        messages,
                    );
                },
                SectionState {
                    monitor,
                    messages: Vec::new(),
                },
            );
        harness.run();
        harness
    }

    #[test]
    fn the_section_collapses_while_stopped_and_mounts_while_recording() {
        let mut harness = section_harness(MonitorState::Stopped);
        assert!(
            harness
                .query_by_role_and_label(egui::accesskit::Role::Unknown, "Key Input Timeline")
                .is_none(),
            "a stopped card mounts no graph"
        );
        let switch = harness.get_by_label("Start timeline");
        assert_eq!(
            switch.accesskit_node().role(),
            egui::accesskit::Role::CheckBox
        );
        switch.click();
        harness.run();
        assert!(
            harness
                .state()
                .messages
                .iter()
                .any(|m| matches!(m, Message::ToggleMonitor))
        );

        let mut harness = section_harness(MonitorState::Recording(Timeline::default()));
        assert_eq!(
            harness
                .get_by_role_and_label(egui::accesskit::Role::Unknown, "Key Input Timeline")
                .rect()
                .height(),
            GRAPH_HEIGHT,
            "a recording card mounts the graph"
        );
        harness.get_by_label("Stop timeline").click();
        harness.run();
        assert!(
            harness
                .state()
                .messages
                .iter()
                .any(|m| matches!(m, Message::ToggleMonitor))
        );
    }

    #[test]
    fn a_starting_section_disables_the_switch() {
        let mut harness = section_harness(MonitorState::Starting);
        assert!(
            harness
                .query_by_role_and_label(egui::accesskit::Role::Unknown, "Key Input Timeline")
                .is_none(),
            "the starting card still shows the collapsed state"
        );
        harness
            .get_by_role_and_label(egui::accesskit::Role::CheckBox, "Starting…")
            .click();
        harness.run();
        assert!(
            harness.state().messages.is_empty(),
            "a disabled switch emits nothing"
        );
    }
}
