//! The monitor timeline's data model. Framework-free on purpose: the painter
//! that draws it (T5) is added beside this, and the state layer depends only
//! on what is here.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use egui::{
    Color32, CornerRadius, FontId, Id, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2,
    WidgetInfo, WidgetType,
};

use super::{keycap, language::Language, message::Message, theme};
use crate::protocol::{KeySlot, MonitorDecision, MonitorEdge, MonitorSnapshot};

/// Which way a lane's arrow points: the shared direction type, so the four
/// lanes and the D-pad's four caps are the same four directions.
use keycap::Arrow;

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

/// One lane's display identity from the reference canvas theme: the signal
/// color plus the tint, border, and ring it takes while its key is held.
struct Lane {
    signal: Color32,
    held_fill: Color32,
    held_border: Color32,
    held_ring: Color32,
    arrow: Arrow,
}

// Alphas are stored as premultiplied bytes, so each one is rounded to the
// nearest byte (0.12 * 255 = 30.6 -> 31).
//
// Every value is a Tailwind step and every one of them resolves to a `theme.rs`
// token, so the lanes read the theme rather than restating it. `held_fill` and
// `held_ring` are the base at a straight alpha, so they take
// [`theme::with_alpha`] over the ramp step.
const LANES: [Lane; 4] = [
    Lane {
        signal: theme::BLUE_600,
        held_fill: theme::with_alpha(theme::BLUE_600, 31),
        held_border: theme::BLUE_700,
        held_ring: theme::with_alpha(theme::BLUE_200, 230),
        arrow: Arrow::Up,
    },
    Lane {
        signal: theme::VIOLET_600,
        held_fill: theme::with_alpha(theme::VIOLET_500, 31),
        held_border: theme::VIOLET_700,
        held_ring: theme::with_alpha(theme::VIOLET_200, 230),
        arrow: Arrow::Down,
    },
    Lane {
        signal: theme::INDIGO_600,
        held_fill: theme::with_alpha(theme::INDIGO_500, 31),
        held_border: theme::INDIGO_700,
        held_ring: theme::with_alpha(theme::INDIGO_200, 230),
        arrow: Arrow::Left,
    },
    Lane {
        signal: theme::PURPLE_600,
        held_fill: theme::with_alpha(theme::PURPLE_600, 31),
        held_border: theme::PURPLE_700,
        held_ring: theme::with_alpha(theme::PURPLE_200, 230),
        arrow: Arrow::Right,
    },
];

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
    draw_graph(ui, &painter, rect, timeline, &names, now);

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
    ui: &Ui,
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
        draw_keycap(ui, rect, &axis, row, lane, names[row], held);
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
///
/// A resting lane is [`theme::WHITE`] on the frame's slate inset, so the
/// canvas reads as content sitting in a recess rather than as a grey panel on
/// a white one -- the layering the timing and measurement cards already use.
fn draw_track(painter: &Painter, rect: Rect, axis: &Axis, row: usize, lane: &Lane, held: bool) {
    let track = axis.track(rect, row);
    let radius = CornerRadius::same(LANE_RADIUS as u8);
    painter.rect_filled(
        track,
        radius,
        if held { lane.held_fill } else { theme::WHITE },
    );
    let clipped = painter.with_clip_rect(track);
    let grid = Stroke::new(1.0, theme::SLATE_200);
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
            if held {
                lane.held_border
            } else {
                theme::SLATE_200
            },
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
        // The reference's block radius is 5.5; egui stores integer corner
        // radii, so it rounds to 6.
        let block = Rect::from_min_size(
            Pos2::new(x0, track.top() + 4.0),
            Vec2::new(width, track.height() - 8.0),
        );
        let radius = theme::CHIP_RADIUS;
        clipped.rect_filled(block, radius, lane.signal);
        if open {
            clipped.rect_stroke(
                block,
                radius,
                Stroke::new(1.5, theme::WHITE),
                egui::StrokeKind::Middle,
            );
        }
        let label = millis(end.saturating_sub(start));
        let size = 9.0;
        let galley = theme::line_galley(&clipped, &label, size);
        if width >= galley.size().x + 6.0 {
            let center = Pos2::new(x0 + width / 2.0, track.center().y);
            theme::stamp_galley(
                &clipped,
                center - galley.size() / 2.0,
                &galley,
                theme::WHITE,
                size,
            );
        }
    }
}

/// The keycap in the left gutter: the shared keycap chrome
/// ([`keycap::paint_surface`]) at the canvas's 40px cap size, carrying the
/// key's label over its direction arrow.
///
/// The reference's canvas draws this cap inline and expresses its held state
/// as a 1.5px descent rather than a `scale-95`. The window's keycaps are one
/// component, though -- the D-pad, the preview, and this gutter all paint the
/// same compression, glow, ring, and inset shadow -- so a press reads
/// identically wherever it lands. Only the cap's size, radius, edge weight,
/// ring width, and content are local to this surface.
fn draw_keycap(ui: &Ui, rect: Rect, axis: &Axis, row: usize, lane: &Lane, name: &str, held: bool) {
    let x = rect.left() + ((LABEL_GUTTER - CAP_SIZE) / 2.0).round();
    let y = axis.track(rect, row).top() + ((LANE_HEIGHT - CAP_SIZE) / 2.0).round();
    let cap = Rect::from_min_size(Pos2::new(x, y), Vec2::splat(CAP_SIZE));
    let scale = keycap::press_scale(ui, Id::new("ui-timeline-cap-scale").with(row), held);
    let ink = if held { theme::WHITE } else { theme::SLATE_900 };
    keycap::paint_surface(
        ui,
        &keycap::Surface {
            rect: cap,
            radius: CornerRadius::same(CAP_RADIUS as u8),
            fill: if held { lane.signal } else { theme::WHITE },
            edge: Stroke::new(
                if held { 1.5 } else { 1.0 },
                if held {
                    lane.held_border
                } else {
                    theme::SLATE_200
                },
            ),
            shadow: if held {
                keycap::glow(lane.held_ring, theme::KEYCAP_PRESSED_GLOW_BLUR)
            } else {
                theme::SHADOW_XS
            },
            inner_shadow: held,
            // The canvas draws its ring 2.5px wide, heavier than the other
            // two surfaces' `ring-2`; the width stays local, the alignment
            // does not. It belongs to the same element as the box, so the
            // `scale-95` compresses it the way it compresses the other two.
            ring: held.then_some((2.5 * scale, lane.held_ring)),
            scale,
        },
        |painter, box_rect| {
            let center_x = box_rect.center().x;
            let lines = keycap::key_lines(name);
            // Shrink the label until the widest line clears the cap's 4 px
            // padding, so a long key name stays inside the cap instead of
            // bleeding over it.
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
                    .any(|line| theme::line_width(painter, line, size) > CAP_SIZE - 8.0)
            {
                size -= 0.5;
            }
            let baseline = box_rect.top() + 16.5;
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
            keycap::paint_arrow(
                painter,
                Rect::from_center_size(
                    Pos2::new(center_x, box_rect.top() + 29.5),
                    Vec2::splat(9.0),
                ),
                lane.arrow,
                ink,
            );
        },
    );
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
                        theme::INDIGO_500_25
                    } else {
                        theme::INDIGO_500_20
                    },
                );
                let edge = Stroke::new(1.0, theme::INDIGO_600);
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
                let long_edge = Stroke::new(1.5, theme::INDIGO_600);
                for y in [top + 0.5, top + PAIR_HEIGHT - 0.5] {
                    painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], long_edge);
                }
                let label = millis(end.saturating_sub(start));
                let size = 9.0;
                let galley = theme::line_galley(painter, &label, size);
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
                    let rule = Stroke::new(1.0, theme::INDIGO_600);
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
                let radius = theme::BADGE_RADIUS;
                painter.rect_filled(badge, radius, theme::INDIGO_950);
                painter.rect_stroke(
                    badge,
                    radius,
                    Stroke::new(1.0, theme::INDIGO_400),
                    egui::StrokeKind::Middle,
                );
                theme::stamp_galley(
                    painter,
                    Pos2::new(center.round(), badge_y + BADGE_HEIGHT / 2.0) - galley.size() / 2.0,
                    &galley,
                    theme::WHITE,
                    size,
                );
                if live {
                    let dot_y = if badge_above {
                        top - 7.0
                    } else {
                        top + PAIR_HEIGHT + 7.0
                    };
                    painter.circle_filled(Pos2::new(x1, dot_y), 2.5, theme::INDIGO_600);
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
        Stroke::new(1.5, theme::INDIGO_600),
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
        let text_w = theme::line_width(painter, &label, 10.0);
        let (left, color, bold) = if at == 0 {
            (axis.now_x - 14.0 - text_w, theme::INDIGO_600, true)
        } else if at == WINDOW_MICROS {
            (axis.left + 2.0, theme::SLATE_600, false)
        } else {
            let center = axis.left + axis.width * (1.0 - at as f32 / WINDOW_MICROS as f32);
            (center - text_w / 2.0, theme::SLATE_600, false)
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
    let rule = Stroke::new(1.0, theme::SLATE_300);
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

/// The card heading: the stamp-weighted title the reference renders bold.
/// egui's bundled faces ship one weight, so `RichText::strong` alone renders
/// thin; every card title stamps instead. Painted text still publishes its
/// node so label queries keep finding it.
fn card_title(ui: &mut Ui, content: &str) {
    let galley = theme::line_galley(ui.painter(), content, theme::HEADING_SIZE);
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), content));
    theme::stamp_galley(
        ui.painter(),
        rect.min,
        &galley,
        theme::SLATE_900,
        theme::HEADING_SIZE,
    );
}

/// Canvas text centered on a point, drawn with the fake-bold stamp the port
/// uses wherever the design calls for bold.
fn centered_text(painter: &Painter, center: Pos2, content: &str, size: f32, color: Color32) {
    theme::centered_line(painter, center, content, size, color, true);
}

fn centered_text_with(
    painter: &Painter,
    center: Pos2,
    content: &str,
    size: f32,
    color: Color32,
    bold: bool,
) {
    theme::centered_line(painter, center, content, size, color, bold);
}

fn millis(micros: u64) -> String {
    format!("{:.1}ms", micros as f32 / 1_000.0)
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
                        theme::paint_icon(
                            ui.painter(),
                            icon_rect,
                            theme::Icon::Target,
                            theme::INDIGO_600,
                        );
                        card_title(ui, language.text("Key Input Timeline"));
                    });
                    ui.colored_label(
                        theme::SLATE_500,
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

/// The card's monitor toggle: a 48x24 pill, padding
/// `round(0.1 * 24) = 2`, and a 20px round knob that sits 2px from the left
/// when off and 2px from the right when on. The track takes [`theme::SLATE_300`]
/// until recording, then [`theme::INDIGO_600`]; the knob stays
/// [`theme::WHITE`]. Paint-only: it publishes the checkbox node and hands
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
    painter.rect_filled(rect, theme::CONTROL_RADIUS, track);
    let knob_x = if recording {
        rect.right() - 22.0
    } else {
        rect.left() + 2.0
    };
    painter.circle_filled(
        Pos2::new(knob_x + 10.0, rect.center().y),
        10.0,
        theme::WHITE,
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

    /// Every `Shape::Circle` this frame painted, flattened out of `Shape::Vec`.
    fn painted_circles<State>(harness: &Harness<'_, State>) -> Vec<egui::epaint::CircleShape> {
        fn collect(shape: &egui::Shape, out: &mut Vec<egui::epaint::CircleShape>) {
            match shape {
                egui::Shape::Circle(circle) => out.push(*circle),
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

    /// The canvas layers the way every other card does: a white card body, a
    /// slate inset panel, and white content on it. A resting lane is therefore
    /// **white** -- not the `SLATE_50` it used to be, which put the grey on the
    /// lane and left the panel white and made the timeline the one card in the
    /// window whose background read inverted.
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
            .find(|rect| rect.rect.height() == LANE_HEIGHT && rect.rect.width() > 100.0)
            .expect("a stopped graph still paints its idle lanes");
        assert_eq!(
            lane.fill,
            theme::WHITE,
            "a resting lane is white on the frame's slate inset, like every \
             other card's content"
        );
        assert_eq!(lane.corner_radius, CornerRadius::same(LANE_RADIUS as u8));

        assert!(
            painted_rects(&harness)
                .iter()
                .all(|rect| rect.fill != theme::SLATE_50),
            "the lane must not carry the grey the panel owns"
        );

        let caps: Vec<_> = painted_rects(&harness)
            .into_iter()
            .filter(|rect| rect.fill == theme::WHITE && rect.rect.width() == CAP_SIZE)
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

    /// The frame the canvas sits in is the card's **recessed panel**, so it
    /// takes `group_style`'s slate inset -- the grey step -- and the lanes on
    /// it are white. `the_stopped_graph_keeps_its_lanes_keycaps_and_ruler`
    /// calls `graph()` directly and so never renders this frame; without this
    /// test the panel's fill could flip back to white unnoticed.
    #[test]
    fn the_canvas_frame_is_the_recessed_slate_panel() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .build_ui(|ui| {
                theme::graph_frame().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.set_min_height(GRAPH_HEIGHT);
                });
            });
        harness.run();

        let panel = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.corner_radius == theme::CARD_RADIUS && rect.rect.width() > 100.0)
            .expect("the canvas frame paints its panel");
        assert_eq!(
            panel.fill,
            theme::SLATE_50_80,
            "the canvas panel is the slate inset, not white -- the timeline must \
             layer like every other card (white body, slate inset, white content)"
        );
        assert_eq!(
            panel.stroke.color,
            theme::SLATE_200,
            "the panel keeps its slate-200 hairline"
        );
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
        assert_eq!(block.corner_radius, theme::CHIP_RADIUS);
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
                rect.stroke == Stroke::new(1.5, theme::WHITE)
                    && rect.rect.height() == LANE_HEIGHT - 8.0
            }),
            "the open block carries the white outline as its own stroke shape"
        );
    }

    /// The gutter cap is one of the window's three keycap surfaces, so its held
    /// state must be the shared press effect -- a `scale-95` compression about
    /// the box centre, an accent glow, a ring, and the `shadow-inner` band --
    /// not the canvas's own 1.5px descent.
    #[test]
    fn a_held_keycap_compresses_through_the_shared_press_effect() {
        let held = |pressed: bool| {
            let mut timeline = Timeline::default();
            if pressed {
                timeline.accept(output(KeySlot::VerticalFirst, true, 0), false, backdated());
            }
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
            harness.run_steps(20);
            harness
        };

        let resting = held(false);
        let resting_cap = painted_rects(&resting)
            .into_iter()
            .find(|rect| rect.fill == theme::WHITE && rect.rect.width() == CAP_SIZE)
            .expect("the resting gutter cap paints at its full size");

        let pressed = held(true);
        let pressed_cap = painted_rects(&pressed)
            .into_iter()
            .find(|rect| rect.fill == LANES[0].signal && rect.rect.width() < CAP_SIZE)
            .expect("the held gutter cap fills with its lane accent");

        assert!(
            (pressed_cap.rect.width() - CAP_SIZE * keycap::PRESS_SCALE).abs() < 0.5,
            "the held cap must compress to the shared scale: {}",
            pressed_cap.rect.width()
        );
        assert!(
            (pressed_cap.rect.center() - resting_cap.rect.center()).length() < 0.01,
            "the compression shrinks about the box centre, not downward"
        );
        assert_eq!(pressed_cap.corner_radius, resting_cap.corner_radius);
        assert!(
            painted_rects(&pressed).iter().any(|rect| {
                rect.stroke.color == theme::BLACK_5 && rect.stroke_kind == egui::StrokeKind::Inside
            }),
            "the held cap carries the shared inset shadow"
        );
        assert!(
            painted_rects(&pressed)
                .iter()
                .any(|rect| rect.stroke.color == LANES[0].held_ring),
            "the held cap carries its lane ring"
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
            .find(|rect| rect.fill == theme::INDIGO_500_25)
            .expect("two live spans paint the stronger overlap fill");
        assert_eq!(column.rect.height(), PAIR_HEIGHT);

        let badge = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.fill == theme::INDIGO_950)
            .expect("the overlap carries its duration badge");
        assert_eq!(badge.rect.height(), BADGE_HEIGHT);
        assert!(
            painted_rects(&harness).iter().any(|rect| {
                rect.stroke == Stroke::new(1.0, theme::INDIGO_400)
                    && rect.rect.height() == BADGE_HEIGHT
            }),
            "the badge paints its own edge stroke"
        );
    }

    // The data-model tests: the painter above
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

    /// R2 round-2 finding 2: the monitor switch's advertised geometry (the
    /// report's 48x24 pill, radius 12, 20px knob with 2px padding) was
    /// unverified. The values are the reference toggler at `.size(24)`: track
    /// 2N x N, border radius height / 2, knob height - 2 * round(0.1 * height),
    /// offset `round(0.1 * 24) = 2`.
    #[test]
    fn the_monitor_switch_paints_the_toggler_geometry() {
        // Off: SLATE_300 track, the knob 2px from the left.
        let harness = section_harness(MonitorState::Stopped);
        let track = painted_rects(&harness)
            .into_iter()
            .find(|rect| {
                rect.fill == theme::SLATE_300 && rect.corner_radius == theme::CONTROL_RADIUS
            })
            .expect("the off switch paints its SLATE_300 pill");
        assert_eq!(
            track.rect.size(),
            Vec2::new(48.0, 24.0),
            "the track is the 24px pill"
        );
        let knob = painted_circles(&harness)
            .into_iter()
            .find(|circle| circle.fill == theme::WHITE && circle.radius == 10.0)
            .expect("the off switch paints its 20px knob");
        assert_eq!(
            knob.center,
            Pos2::new(track.rect.left() + 12.0, track.rect.center().y),
            "the off knob sits 2px from the left edge"
        );
        assert_eq!(
            knob.center.x - knob.radius - track.rect.left(),
            2.0,
            "2px padding"
        );
        assert_eq!(
            knob.center.y - knob.radius - track.rect.top(),
            2.0,
            "2px padding"
        );

        // On: INDIGO_600 track, the knob 2px from the right.
        let harness = section_harness(MonitorState::Recording(Timeline::default()));
        let track = painted_rects(&harness)
            .into_iter()
            .find(|rect| {
                rect.fill == theme::INDIGO_600 && rect.corner_radius == theme::CONTROL_RADIUS
            })
            .expect("the recording switch paints its INDIGO_600 pill");
        assert_eq!(track.rect.size(), Vec2::new(48.0, 24.0));
        let knob = painted_circles(&harness)
            .into_iter()
            .find(|circle| circle.fill == theme::WHITE && circle.radius == 10.0)
            .expect("the recording switch paints its 20px knob");
        assert_eq!(
            knob.center.x,
            track.rect.right() - 12.0,
            "the on knob sits 2px from the right edge"
        );
        assert_eq!(
            track.rect.right() - (knob.center.x + knob.radius),
            2.0,
            "2px padding"
        );
    }
}
