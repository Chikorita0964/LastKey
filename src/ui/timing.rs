//! Timing card, sliders, and value boxes for LastKey settings.
//!
//! Port of the four SOCD modes (`Immediate`, `PressDelay`, `ReleaseDelay`, `RandomMix`),
//! the mode-conditional timing groups with matched card heights, the two-row timing
//! pills, the dual-handle range slider, and the value boxes with select-all-on-focus
//! behavior.

use egui::{
    Align, Align2, Color32, FontId, Frame, Id, Margin, Pos2, Rect, Response, Sense, Stroke,
    TextEdit, Ui, WidgetInfo, WidgetType, text::CCursor, vec2,
};

use crate::settings::{SocdMode, TimingSettings};

use super::{
    language::Language,
    message::{Message, TimingField},
    preview::{self, Preview},
    state::{TimingInputs, parse_ms_text, parse_press_rate_text, parse_rate_text},
    theme::{
        self, BLUE_600, CARD_PADDING, GROUP_PADDING, HEADING_SIZE, INDIGO_600, PURPLE_600, RED_600,
        SECTION_GAP, SLATE_100, SLATE_300, SLATE_500, SLATE_900, SLIDER_HANDLE_BORDER, VIOLET_500,
        VIOLET_600, WHITE,
    },
};

// ----------------------------------------------------------------------------
// Mode Metadata
// ----------------------------------------------------------------------------

/// Geometry shared by every slider in the timing card (F08): a 12px rail
/// rounded to 6 -- the reference's `h-3` + `rounded-full` -- and a thumb
/// whose *drawn box* is the reference's `w-4 h-4` (16px), a size the status
/// does not change.
///
/// The thumb's radius is the circle's path radius, not the box's. epaint
/// paints a `CircleShape`'s stroke entirely **outside** the path
/// (`tessellate_circle` calls `PathStroke::from(stroke).outside()`), while a
/// CSS border is drawn inside its box. The two boxes therefore differ:
///
/// | | outer | core |
/// |---|---|---|
/// | reference (`w-4 h-4` + `border-[3px]`) | `16` | `16 - 2 * 3 = 10` |
/// | epaint | `2 * (radius + stroke)` | `2 * radius` |
///
/// Matching both puts the path at `8 - 3 = 5`, which draws the reference's
/// 16px box around a 10px core. The path radius is *not* `8 - 3/2`: that would
/// assume the stroke straddles the path, and the resulting 6.5 drew a 19px
/// handle -- a visibly heavier control than the reference's, which is exactly
/// what the request named.
///
/// The theme's single-handle `SLIDER_RAIL_*` / `SLIDER_HANDLE_RADIUS*` tokens
/// are no longer consumed by this card; the Random Mix mixer takes this
/// geometry too. The rail's radius is [`theme::CHIP_RADIUS`] at the paint call,
/// and the thumb's ring is [`SLIDER_HANDLE_BORDER`].
const RAIL_WIDTH: f32 = 12.0;
const THUMB_RADIUS: f32 = 5.0;

/// Color associated with each SOCD mode.
///
/// Every arm names the mode's Tailwind `-600` base directly, the same token its
/// card tint, keycap fill, and preview accent read, so the four arms are
/// visibly the four ramp steps the mode table in `theme.rs` lists.
pub const fn mode_color(mode: SocdMode) -> Color32 {
    match mode {
        SocdMode::Immediate => BLUE_600,
        SocdMode::PressDelay => INDIGO_600,
        SocdMode::ReleaseDelay => VIOLET_600,
        SocdMode::RandomMix => PURPLE_600,
    }
}

/// Localized label for an SOCD mode.
pub fn mode_label(mode: SocdMode, language: Language) -> &'static str {
    language.text(match mode {
        SocdMode::Immediate => "Immediate",
        SocdMode::PressDelay => "Press Delay",
        SocdMode::RandomMix => "Random Mix",
        SocdMode::ReleaseDelay => "Release Delay",
    })
}

/// Stable widget id per value box, used by the activate-to-select-all flow.
pub fn value_box_id(field: TimingField) -> Id {
    Id::new(match field {
        TimingField::TransitionMinimum => "timing-transition-minimum",
        TimingField::TransitionMaximum => "timing-transition-maximum",
        TimingField::PreservationRate => "timing-preservation-rate",
        TimingField::PressRate => "timing-press-rate",
        TimingField::PreservedMinimum => "timing-preserved-minimum",
        TimingField::PreservedMaximum => "timing-preserved-maximum",
    })
}

/// Selects all text within a `TextEdit` widget.
pub fn select_all_text(ui: &Ui, id: Id, text: &str) {
    let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text_selection::CCursorRange::two(
            CCursor::default(),
            CCursor::new(text.chars().count()),
        )));
    state.store(ui.ctx(), id);
}

// ----------------------------------------------------------------------------
// Value Box
// ----------------------------------------------------------------------------

/// The manual entry box's text size (`text-xs`, the reference's 12px).
const VALUE_TEXT_SIZE: f32 = 12.0;

/// Configuration properties for rendering a value box.
#[derive(Clone, Copy, Debug)]
pub struct ValueBoxProps {
    pub field: TimingField,
    pub editing: bool,
    pub invalid: bool,
    pub width: f32,
    pub accent: Color32,
}

impl ValueBoxProps {
    pub const fn new(
        field: TimingField,
        editing: bool,
        invalid: bool,
        width: f32,
        accent: Color32,
    ) -> Self {
        Self {
            field,
            editing,
            invalid,
            width,
            accent,
        }
    }
}

/// Value box for numeric timing inputs.
///
/// Gates select-all on the armed state: the first click selects all text and arms the box;
/// subsequent clicks within the armed box fall through to native caret placement.
/// Strictly read-only with respect to `TimingInputs`; buffer changes emit
/// `Message::TimingTextChanged` for `state::update` to apply as the single writer.
///
/// **The figure paints at normal weight -- the edit's own single pass, not a
/// [`theme::stamp_galley`] second one.** The reference writes its entry boxes
/// `font-mono font-bold`, but a *monospace bold face* does that work and the
/// port cannot select one (egui's bundled faces ship a single weight). The
/// stamp standing in for it offsets its second pass by `max(12 * 0.04, 0.35)`
/// = 0.48 px, which at this size reads as a smeared figure rather than a
/// heavier one -- and, worse, it leaves the value a different weight from the
/// `~`, `:`, `ms` and `%` glyphs sharing its pill, so one pill carries two
/// weights. One pass keeps the pill uniform, which is the property a reader
/// actually sees; the reference's bold buys no separation here anyway, because
/// the unit sits right beside the figure either way.
pub fn value_box(
    ui: &mut Ui,
    props: ValueBoxProps,
    buffer: &str,
    messages: &mut Vec<Message>,
) -> Response {
    let id = value_box_id(props.field);
    let text_color = if props.invalid { RED_600 } else { props.accent };

    // Frame-local buffer so the view remains read-only with respect to TimingInputs.
    let mut local_text = buffer.to_owned();
    let output = TextEdit::singleline(&mut local_text)
        .id(id)
        .desired_width(props.width)
        .font(FontId::new(VALUE_TEXT_SIZE, egui::FontFamily::Proportional))
        .horizontal_align(Align::Center)
        .vertical_align(Align::Center)
        .text_color(text_color)
        .margin(Margin::symmetric(2, 1))
        .frame(Frame::NONE)
        .show(ui);
    let response = output.response.response;

    let field_name = match props.field {
        TimingField::TransitionMinimum => "Transition Minimum",
        TimingField::TransitionMaximum => "Transition Maximum",
        // The reference labels the two halves of the ratio pill separately
        // (`t.pressDelayPercentage` / `t.releaseDelayPercentage`), so each box
        // names its own side.
        TimingField::PreservationRate => "Release Delay Percentage",
        TimingField::PressRate => "Press Delay Percentage",
        TimingField::PreservedMinimum => "Preserved Minimum",
        TimingField::PreservedMaximum => "Preserved Maximum",
    };
    response.widget_info(|| {
        let mut info = WidgetInfo::labeled(WidgetType::TextEdit, ui.is_enabled(), field_name);
        info.current_text_value = Some(buffer.to_owned());
        info
    });

    if !props.editing && (response.gained_focus() || response.clicked()) {
        select_all_text(ui, id, buffer);
    }

    if response.clicked() {
        messages.push(Message::ValueBoxActivated(props.field));
    }

    if response.changed() {
        messages.push(Message::TimingTextChanged(props.field, local_text));
    }

    if (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
    {
        messages.push(Message::TimingTextSubmitted(props.field));
    }

    response
}

// ----------------------------------------------------------------------------
// Range Slider (Dual Thumb)
// ----------------------------------------------------------------------------

/// Configuration properties for a dual-thumb range slider.
#[derive(Clone, Debug)]
pub struct RangeSliderProps<'a> {
    pub min_val: f32,
    pub max_val: f32,
    pub floor: f32,
    pub enabled: bool,
    pub accent: Color32,
    pub id: Id,
    pub accessible_name: &'a str,
}

#[derive(Clone, Copy, Default)]
struct RangeDragState {
    anchor: f32,
    offset: f32,
}

/// The duration rail's upper bound, in milliseconds.
///
/// The reference's rail stops at `DELAY_MS_MAX = 20` (`domain/socd.ts:12`) and
/// keeps larger values reachable only through the numeric editors. The rail is
/// asked to reach the settings layer's own ceiling instead, so a long delay can
/// be dragged rather than typed. [`crate::settings::MAX_TIMING_MICROS`] is the
/// single upper bound every timing entry path already shares (the numeric
/// editors clamp at it, and `Settings::validate` rejects anything past it), so
/// the rail reads it rather than carrying a second literal that could drift.
const RAIL_MAX_MS: f32 = crate::settings::MAX_TIMING_MICROS as f32 / 1_000.0;

/// Dual-thumb range slider operating over `0.0..=`[`RAIL_MAX_MS`] ms in 0.1 ms
/// units -- the precision `Settings::validate` enforces for every timing field.
/// Clamps to `floor` on the lower bound, preserves drag offsets for near-thumb clicks,
/// jumps thumbs for distant presses, and sorts automatically when handles cross.
pub fn range_slider(ui: &mut Ui, props: RangeSliderProps<'_>) -> Option<(f32, f32)> {
    let desired_size = vec2(ui.available_width(), 28.0);
    let (rect, response) = ui.allocate_exact_size(
        desired_size,
        if props.enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );

    let track_margin = 8.0;
    let track_start = rect.min.x + track_margin;
    let track_end = rect.max.x - track_margin;
    let track_width = (track_end - track_start).max(1.0);
    let center_y = rect.center().y;

    let to_x = |v: f32| track_start + (v.clamp(0.0, RAIL_MAX_MS) / RAIL_MAX_MS) * track_width;
    let to_value = |x: f32| ((x - track_start) / track_width * RAIL_MAX_MS).clamp(0.0, RAIL_MAX_MS);
    // `* 10.0` then `/ 10.0` quantizes to the 0.1 ms unit the settings layer
    // stores (`micros % 100 == 0`), independent of the rail's span.
    let to_rounded = |x: f32| {
        (((x - track_start) / track_width * RAIL_MAX_MS * 10.0).round() / 10.0)
            .clamp(props.floor, RAIL_MAX_MS)
    };

    let mut result = None;

    if props.enabled {
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                let val = to_value(pos.x);
                let cur_min = props.min_val.min(RAIL_MAX_MS);
                let cur_max = props.max_val.min(RAIL_MAX_MS);
                let select_min = if cur_min == cur_max {
                    val <= cur_min
                } else {
                    (val - cur_min).abs() <= (val - cur_max).abs()
                };
                let (selected, anchor) = if select_min {
                    (cur_min, cur_max)
                } else {
                    (cur_max, cur_min)
                };

                // A press within 12px of a thumb drags it with its offset
                // preserved; a distant press jumps the nearer thumb.
                let offset = if (val - selected).abs() * track_width / RAIL_MAX_MS <= 12.0 {
                    val - selected
                } else {
                    0.0
                };
                ui.data_mut(|d| d.insert_temp(props.id, RangeDragState { anchor, offset }));
                let jumped = to_rounded(pos.x - offset * track_width / RAIL_MAX_MS);
                let (n_min, n_max) = if jumped <= anchor {
                    (jumped, anchor)
                } else {
                    (anchor, jumped)
                };
                result = Some((n_min, n_max));
            }
        } else if response.dragged()
            && let Some(drag) = ui.data(|d| d.get_temp::<RangeDragState>(props.id))
            && let Some(pos) = response.interact_pointer_pos()
        {
            let moved = to_rounded(pos.x - drag.offset * track_width / RAIL_MAX_MS);
            let (n_min, n_max) = if moved <= drag.anchor {
                (moved, drag.anchor)
            } else {
                (drag.anchor, moved)
            };
            result = Some((n_min, n_max));
        } else if response.drag_stopped() {
            ui.data_mut(|d| d.remove_temp::<RangeDragState>(props.id));
        }
    }

    let painter = ui.painter();
    let active_accent = if props.enabled {
        props.accent
    } else {
        SLATE_300
    };
    let rail_half = RAIL_WIDTH / 2.0;

    // Background track: SLATE_100, the card's shared 12px rail rounded to 6.
    let track_rect = Rect::from_min_max(
        Pos2::new(track_start, center_y - rail_half),
        Pos2::new(track_end, center_y + rail_half),
    );
    painter.rect_filled(track_rect, theme::CHIP_RADIUS, SLATE_100);

    // Active range span
    let span_start = to_x(props.min_val);
    let span_end = to_x(props.max_val);
    if span_end > span_start {
        let span_rect = Rect::from_min_max(
            Pos2::new(span_start, center_y - rail_half),
            Pos2::new(span_end, center_y + rail_half),
        );
        painter.rect_filled(span_rect, theme::CHIP_RADIUS, active_accent);
    }

    // Two thumbs: white background, 3px accent stroke, the shared constant
    // 16px drawn box (the reference thumb has no status-dependent size).
    // epaint draws the stroke outside the path, so the box is
    // `2 * THUMB_RADIUS + SLIDER_HANDLE_BORDER`.
    for val in [props.min_val, props.max_val] {
        let thumb_x = to_x(val);
        let center = Pos2::new(thumb_x, center_y);
        painter.circle(
            center,
            THUMB_RADIUS,
            WHITE,
            Stroke::new(SLIDER_HANDLE_BORDER, active_accent),
        );
    }

    let name = props.accessible_name.to_owned();
    response
        .widget_info(move || WidgetInfo::labeled(WidgetType::Slider, props.enabled, name.clone()));
    if props.enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    result
}

// ----------------------------------------------------------------------------
// Mixer Slider (Random Mix Rail)
// ----------------------------------------------------------------------------

/// Random Mix ratio rail slider: the card's shared 12px rail rounded to 6 and
/// a constant 16px handle ringed in [`PURPLE_600`], the same geometry the
/// duration rail uses (F08; the reference draws both with an `h-3` rail and a
/// `w-4 h-4` thumb). The rail is filled [`INDIGO_600`] left of the handle and
/// [`VIOLET_500`] right of it, so one glance shows the split the two delay
/// modes produce.
pub fn mixer_slider(
    ui: &mut Ui,
    press_share: f32,
    enabled: bool,
    language: Language,
) -> Option<f32> {
    let desired_size = vec2(ui.available_width(), 28.0);
    let (rect, response) = ui.allocate_exact_size(
        desired_size,
        if enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );

    let track_margin = 8.0;
    let track_start = rect.min.x + track_margin;
    let track_end = rect.max.x - track_margin;
    let track_width = (track_end - track_start).max(1.0);
    let center_y = rect.center().y;

    let to_x = |v: f32| track_start + ((v.clamp(1.0, 99.0) - 1.0) / 98.0) * track_width;
    let to_val = |x: f32| (((x - track_start) / track_width * 98.0 + 1.0).round()).clamp(1.0, 99.0);

    let mut result = None;

    if enabled
        && (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let val = to_val(pos.x);
        if (val - press_share).abs() >= 0.5 {
            result = Some(val);
        }
    }

    let painter = ui.painter();
    let rail_half = RAIL_WIDTH / 2.0;

    // Rail split at the handle: INDIGO_600 to the left, VIOLET_500 to the
    // right, both on the shared 12px rounded-6 rail.
    let handle_x = to_x(press_share);
    for (start, end, fill) in [
        (track_start, handle_x, INDIGO_600),
        (handle_x, track_end, VIOLET_500),
    ] {
        let (start, end) = (start.min(end), start.max(end));
        if end > start {
            let span_rect = Rect::from_min_max(
                Pos2::new(start, center_y - rail_half),
                Pos2::new(end, center_y + rail_half),
            );
            painter.rect_filled(span_rect, theme::CHIP_RADIUS, fill);
        }
    }

    // Handle thumb: white fill, 3.0 border in PURPLE_600, the shared constant
    // 16px drawn box that hover and drag do not change (the reference thumb
    // is status-independent).
    painter.circle(
        Pos2::new(handle_x, center_y),
        THUMB_RADIUS,
        WHITE,
        Stroke::new(SLIDER_HANDLE_BORDER, PURPLE_600),
    );

    response.widget_info(|| {
        WidgetInfo::slider(
            enabled,
            press_share as f64,
            language.text("Delay Mix Ratio"),
        )
    });

    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    result
}

// ----------------------------------------------------------------------------
// Mode Selector
// ----------------------------------------------------------------------------

/// The card heading: the stamp-weighted title the reference renders bold.
/// egui's bundled faces ship one weight, so `RichText::strong` alone renders
/// thin; every card title stamps instead (`Key mappings`, `Input timing
/// measurement`, `Measured Input Transitions`, `Suggested delays` all do).
/// Painted text still publishes its node so label queries keep finding it.
fn card_title(ui: &mut Ui, content: &str) {
    let galley = ui.painter().layout_no_wrap(
        content.to_owned(),
        FontId::new(HEADING_SIZE, egui::FontFamily::Proportional),
        Color32::PLACEHOLDER,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), content));
    theme::stamp_galley(ui.painter(), rect.min, &galley, SLATE_900, HEADING_SIZE);
}

/// Horizontal 4-mode segment picker for the timing card.
///
/// The strip is the reference's `grid grid-cols-4 gap-1 p-1 bg-white
/// border border-slate-200 rounded-xl shadow-2xs`: a white card with a slate
/// hairline, its segments inset by [`theme::MODE_STRIP_PADDING`] and spaced by
/// [`theme::MODE_STRIP_GAP`]. It is deliberately *not* a [`theme::group_style`]
/// surface -- that frame is the card's slate-50 inset, which is the surface the
/// strip sits *on* rather than the one it is.
///
/// The selected segment is `bg-indigo-600` with white bold text; every other
/// segment is transparent with `text-slate-600` ink and no edge (`border: 0`).
/// The chip colour is the reference's own literal and does **not** follow the
/// mode accent: `renderModeSegmented` hardcodes `bg-indigo-600` for all four
/// modes, so Release Delay selects the same indigo chip the other three do
/// rather than turning violet.
pub fn mode_selector(
    ui: &mut Ui,
    selected: SocdMode,
    language: Language,
    messages: &mut Vec<Message>,
) {
    Frame::NONE
        .fill(WHITE)
        .stroke(Stroke::new(1.0, theme::SLATE_200))
        .corner_radius(theme::CONTROL_RADIUS)
        .shadow(theme::SHADOW_2XS)
        .inner_margin(Margin::same(theme::MODE_STRIP_PADDING as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = theme::MODE_STRIP_GAP;
            ui.columns(4, |cols| {
                for (i, mode) in SocdMode::ALL.iter().copied().enumerate() {
                    let col = &mut cols[i];
                    let is_active = mode == selected;
                    let label = mode_label(mode, language);
                    let ink = if is_active {
                        theme::WHITE
                    } else {
                        theme::SLATE_600
                    };

                    // The reference segment is `rounded-lg` (8px) on a 28px
                    // box. The selected one takes the solid indigo fill and
                    // `shadow-xs`; the others stay transparent and carry no
                    // edge at all (`border: 0`), which is what the retired
                    // `CHIP_RADIUS` chip was drawing a hairline on top of.
                    let btn_frame = if is_active {
                        Frame::NONE
                            .fill(INDIGO_600)
                            .corner_radius(theme::SEGMENT_RADIUS)
                            .shadow(theme::SHADOW_XS)
                            .inner_margin(theme::MODE_PADDING)
                    } else {
                        Frame::NONE
                            .fill(Color32::TRANSPARENT)
                            .corner_radius(theme::SEGMENT_RADIUS)
                            .inner_margin(theme::MODE_PADDING)
                    };

                    let res = btn_frame
                        .show(col, |ui| {
                            ui.set_width(ui.available_width());
                            ui.vertical_centered(|ui| {
                                ui.colored_label(
                                    ink,
                                    egui::RichText::new(label)
                                        .font(FontId::new(12.0, egui::FontFamily::Proportional))
                                        .strong(),
                                );
                            });
                        })
                        .response
                        .interact(Sense::click());

                    res.widget_info(|| {
                        WidgetInfo::selected(WidgetType::Button, col.is_enabled(), is_active, label)
                    });

                    if res.clicked() {
                        messages.push(Message::ModeSelected(mode));
                    }
                }
            });
        });
}

// ----------------------------------------------------------------------------
// Duration Range Component
// ----------------------------------------------------------------------------

/// Configuration properties for a two-row duration range group.
#[derive(Clone, Copy, Debug)]
pub struct DurationRangeProps {
    pub minimum: TimingField,
    pub maximum: TimingField,
    pub label: &'static str,
    pub accent: Color32,
}

impl DurationRangeProps {
    pub const fn new(
        minimum: TimingField,
        maximum: TimingField,
        label: &'static str,
        accent: Color32,
    ) -> Self {
        Self {
            minimum,
            maximum,
            label,
            accent,
        }
    }
}

/// One duration group in the two-row grouping: label and numeric pill on top,
/// two-handle rail slider below.
pub fn duration_range(
    ui: &mut Ui,
    props: DurationRangeProps,
    timing: &TimingSettings,
    inputs: &TimingInputs,
    editing: &[bool; 6],
    language: Language,
    messages: &mut Vec<Message>,
) {
    let min_micros = props.minimum.micros(timing).unwrap_or(0);
    let max_micros = props.maximum.micros(timing).unwrap_or(0);
    let min_val = min_micros as f32 / 1000.0;
    let max_val = max_micros as f32 / 1000.0;
    let invalid = props.minimum.pair_invalid(timing);

    theme::slot_style()
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 8.0);

            // Row 1: dot + label + space + pill
            ui.horizontal(|ui| {
                // The mark centres on the label's line box (the reference's
                // `items-center`) and takes the reference's `gap-1.5` ahead of
                // it, so it is handed the label's own line rather than a box
                // of its own.
                ui.spacing_mut().item_spacing.x = theme::LEGEND_GAP;
                theme::legend_mark(ui, props.accent, 3.5, theme::line_height(ui, 12.0));

                ui.colored_label(
                    SLATE_900,
                    egui::RichText::new(language.text(props.label))
                        .font(FontId::new(12.0, egui::FontFamily::Proportional))
                        .strong(),
                );

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    theme::pill_style(invalid)
                        .inner_margin(Margin::symmetric(6, 1))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                            ui.colored_label(props.accent, "ms");

                            let max_str = inputs.buffer(props.maximum);
                            let max_invalid = invalid || parse_ms_text(max_str).is_none();
                            let max_box_props = ValueBoxProps::new(
                                props.maximum,
                                editing[props.maximum.index()],
                                max_invalid,
                                32.0,
                                props.accent,
                            );
                            value_box(ui, max_box_props, max_str, messages);

                            ui.colored_label(props.accent, "~");

                            let min_str = inputs.buffer(props.minimum);
                            let min_invalid = invalid || parse_ms_text(min_str).is_none();
                            let min_box_props = ValueBoxProps::new(
                                props.minimum,
                                editing[props.minimum.index()],
                                min_invalid,
                                32.0,
                                props.accent,
                            );
                            value_box(ui, min_box_props, min_str, messages);
                        });
                });
            });

            // Row 2: dual-handle slider with unambiguous group-identifying accessible name
            let floor = if props.minimum == TimingField::PreservedMinimum {
                0.1
            } else {
                0.0
            };
            let accessible_name = format!("{} duration range", language.text(props.label));
            let slider_props = RangeSliderProps {
                min_val,
                max_val,
                floor,
                enabled: true,
                accent: props.accent,
                id: Id::new("rail").with(props.minimum.index()),
                accessible_name: &accessible_name,
            };
            if let Some((n_min, n_max)) = range_slider(ui, slider_props) {
                messages.push(Message::TimingSliderChanged(props.minimum, n_min));
                messages.push(Message::TimingSliderChanged(props.maximum, n_max));
            }
        });
}

// ----------------------------------------------------------------------------
// Rate Group (Random Mix)
// ----------------------------------------------------------------------------

/// Split between the two delays for Random Mix, shown as one `press : release`
/// pill on the title row, with a slider controlling the complementary shares.
pub fn rate_group(
    ui: &mut Ui,
    timing: &TimingSettings,
    inputs: &TimingInputs,
    editing: &[bool; 6],
    language: Language,
    messages: &mut Vec<Message>,
) {
    let press_share = 100u8.saturating_sub(timing.overlap_preservation_rate);
    let rate_str = inputs.buffer(TimingField::PreservationRate);
    let press_str = inputs.buffer(TimingField::PressRate);
    let rate_invalid = parse_rate_text(rate_str).is_none();
    let press_invalid = parse_press_rate_text(press_str).is_none();
    let invalid = rate_invalid || press_invalid;

    // The ratio group is the one slot in this card padded with
    // `theme::GROUP_PADDING` rather than the 12 the duration ranges use.
    theme::slot_style()
        .inner_margin(Margin::same(GROUP_PADDING as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 8.0);

            // Row 1: dot + title + pill
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::LEGEND_GAP;
                theme::legend_mark(ui, PURPLE_600, 3.5, theme::line_height(ui, 12.0));

                ui.colored_label(
                    SLATE_900,
                    egui::RichText::new(language.text("Delay Mix Ratio"))
                        .font(FontId::new(12.0, egui::FontFamily::Proportional))
                        .strong(),
                );

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    theme::pill_style(invalid)
                        .inner_margin(Margin::symmetric(6, 1))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                            ui.colored_label(PURPLE_600, "%");

                            // R1 issue 10: VIOLET_500 is intentional for the preservation rate
                            // box (matching the release-delay accent).
                            let rate_props = ValueBoxProps::new(
                                TimingField::PreservationRate,
                                editing[TimingField::PreservationRate.index()],
                                rate_invalid,
                                32.0,
                                VIOLET_500,
                            );
                            value_box(ui, rate_props, rate_str, messages);

                            ui.colored_label(PURPLE_600, ":");

                            // F10: the press share is the mirror box, so it takes the
                            // mixer's left-rail ink where the release box takes its
                            // right-rail ink.
                            let press_props = ValueBoxProps::new(
                                TimingField::PressRate,
                                editing[TimingField::PressRate.index()],
                                press_invalid,
                                32.0,
                                INDIGO_600,
                            );
                            value_box(ui, press_props, press_str, messages);
                        });
                });
            });

            // Row 2: custom mixer rail (INDIGO_600 left of the handle,
            // VIOLET_500 right of it, PURPLE_600 handle ring)
            if let Some(new_share) = mixer_slider(ui, press_share as f32, true, language) {
                messages.push(Message::MixChanged(new_share));
            }

            // Row 3: helper caption
            ui.colored_label(
                SLATE_500,
                egui::RichText::new(
                    language.text("Each overlap randomly picks one of the two delays below."),
                )
                .font(FontId::new(12.0, egui::FontFamily::Proportional)),
            );
        });
}

// ----------------------------------------------------------------------------
// Mechanism Steps ("How it works")
// ----------------------------------------------------------------------------

/// Explains the selected mode as numbered steps with the configured range embedded.
pub fn mechanism_steps(ui: &mut Ui, mode: SocdMode, timing: &TimingSettings, language: Language) {
    if mode == SocdMode::RandomMix {
        return;
    }

    let range = |min_micros: u32, max_micros: u32| {
        format!(
            "{:.1}~{:.1} ms",
            min_micros as f32 / 1000.0,
            max_micros as f32 / 1000.0
        )
    };
    let step = |key: &str| language.text(key).to_owned();
    let step_with_range = |key: &str, range: String| language.text(key).replace("{range}", &range);

    let steps: Vec<(String, bool)> = match mode {
        SocdMode::Immediate => vec![
            (step("Detect an opposite-direction overlap."), false),
            (step("Release the previous key output immediately."), false),
            (step("Send the new key immediately. 0 ms added delay"), true),
        ],
        SocdMode::PressDelay => vec![
            (step("Detect an opposite-direction overlap."), false),
            (step("Release the previous key output immediately."), false),
            (
                step_with_range(
                    "Wait a random time within {range}. This gap sends no input",
                    range(
                        timing.socd_transition_min_micros,
                        timing.socd_transition_max_micros,
                    ),
                ),
                true,
            ),
            (step("Send the new key once the wait ends."), true),
        ],
        SocdMode::ReleaseDelay => vec![
            (step("Detect an opposite-direction overlap."), false),
            (step("Send the new key immediately."), false),
            (
                step_with_range(
                    "Wait a random time within {range}. The overlap stays live",
                    range(
                        timing.preserved_overlap_min_micros,
                        timing.preserved_overlap_max_micros,
                    ),
                ),
                true,
            ),
            (
                step("Release the previous key output once the wait ends."),
                true,
            ),
        ],
        SocdMode::RandomMix => vec![],
    };

    let accent = mode_color(mode);
    // The reference (`InputTimingsCard.tsx` `renderMechanism`) is a white
    // floating card (`bg-white px-3.5 py-3 rounded-2xl border
    // border-slate-200/70 shadow-2xs`), not a dark grey inset slot. The
    // reference badges are subtle rounded rectangles (`bg-slate-100
    // text-slate-500` for the neutral steps, the mode accent tint for the
    // delay steps); step text is readable body copy (`text-slate-600`), not
    // the faint `SLATE_500`. The block sits directly under the controls it
    // explains, and in the delay modes nothing follows it, so the row's
    // leftover collects above it in the stretched card -- the reference's
    // `mt-auto` bottom pin, expressed by order rather than by a spacer the
    // height-matching pass would measure back. Immediate mode appends the
    // preview after it, and the picture takes that pin instead.
    Frame::NONE
        .fill(WHITE)
        .stroke(Stroke::new(1.0, theme::SLATE_200))
        .corner_radius(theme::CARD_RADIUS)
        .shadow(theme::SHADOW_2XS)
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            // The reference block is a block-level `div`, so it fills the
            // column's content width (`mt-auto bg-white ... rounded-2xl`)
            // rather than shrinking to its text. `Frame` sizes its content
            // rect to `min_rect`, and every child here is a shrink-to-fit
            // row, so without this claim the card would hug the longest step
            // and sit against the column's left edge. `set_min_width` is the
            // house pattern for a frame that must span its parent
            // (`mapping.rs`'s cards, the mode selector's segments).
            ui.set_min_width(ui.available_width());
            // The reference stacks the header and the list with `space-y-2`
            // (8px) and the list items with `space-y-1.5` (6px). The two gaps
            // differ, so both are placed explicitly and the frame's own item
            // spacing is zeroed.
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            // Every row here is text or a fixed-size badge, so the layout's
            // "assume something interactive" floor (egui's 18px
            // `interact_size.y`) would inflate each row past the reference's
            // 16px content box and stretch the block by 2px per row.
            ui.spacing_mut().interact_size.y = 16.0;

            // Header: the reference's `w-2 h-2 rounded-full bg-slate-300`
            // mark, then the `text-[11px] font-bold text-slate-800` title.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                let (dot_rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 4.0, theme::SLATE_300);
                ui.colored_label(
                    theme::SLATE_800,
                    egui::RichText::new(language.text("How it works"))
                        .font(FontId::new(11.0, egui::FontFamily::Proportional))
                        .strong(),
                );
            });

            // The header-to-list gap (the reference's `space-y-2`).
            ui.add_space(8.0);

            // Steps list: the reference's `space-y-1.5` (6px) between rows.
            for (index, (label, accented)) in steps.into_iter().enumerate() {
                if index > 0 {
                    ui.add_space(6.0);
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                    let (badge_rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    let (bg, ink) = if accented {
                        (
                            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 30),
                            accent,
                        )
                    } else {
                        // The reference neutral badge is `bg-slate-100
                        // text-slate-500`: the badge's digit takes
                        // [`SLATE_500`] (the slate-500 ink), a step darker
                        // than the step copy beside it. The `inset`/
                        // `SLATE_500` pair this replaces was the dark-slot
                        // leftover; the fill was the wrong half of it.
                        (SLATE_100, SLATE_500)
                    };

                    // The reference badge is `rounded-md` (6px), the same
                    // radius the keycap chips use.
                    ui.painter().rect_filled(badge_rect, theme::CHIP_RADIUS, bg);
                    ui.painter().text(
                        badge_rect.center(),
                        Align2::CENTER_CENTER,
                        format!("{}", index + 1),
                        FontId::new(9.0, egui::FontFamily::Proportional),
                        ink,
                    );

                    ui.colored_label(
                        theme::SLATE_600,
                        egui::RichText::new(label)
                            .font(FontId::new(11.0, egui::FontFamily::Proportional)),
                    );
                });
            }
        });
}

// ----------------------------------------------------------------------------
// Full Timing Card
// ----------------------------------------------------------------------------

/// The Immediate-mode preview mount: the preview state plus the two gates the
/// app owns. `awake` is the window focus and `clock_mounted` is the view's
/// clock mount condition (`matches!(state.profiles, ProfileDialog::Closed)`),
/// both forwarded verbatim to [`preview::preview_card`].
#[derive(Clone, Copy)]
pub struct PreviewMount<'a> {
    pub preview: &'a Preview,
    pub awake: bool,
    pub clock_mounted: bool,
}

/// The full Input Timings card containing the mode selector, conditional duration
/// ranges, Random Mix slider, and mechanism steps.
///
/// The Immediate-mode preview is [`preview::preview_card`] (T5's owner): it
/// carries the transport glyphs, the 850 ms phase clock, and the
/// viewport/dialog gates. The card itself stays state-free; the mount carries
/// the gates in.
///
/// [`super::app::stretch_card`] grows the frame to the row's shared height
/// (the reference's `h-full` inside its grid) while keeping this card's
/// content top-aligned, so the leftover collects below it.
pub fn timing_card(
    ui: &mut Ui,
    timing: &TimingSettings,
    inputs: &TimingInputs,
    editing: &[bool; 6],
    language: Language,
    preview: Option<PreviewMount<'_>>,
    messages: &mut Vec<Message>,
) -> Response {
    theme::card_style()
        .inner_margin(Margin::same(CARD_PADDING as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, SECTION_GAP);

            // Card title row: the leading timer mark plus the stamped heading
            // (`stamp_galley` is the port's bold: egui's bundled faces ship
            // one weight, so `RichText::strong` alone renders thin).
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                let (icon_rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                theme::paint_icon(ui.painter(), icon_rect, theme::Icon::Timer, INDIGO_600);
                ui.vertical(|ui| {
                    card_title(ui, language.text("Input timings"));
                    ui.colored_label(
                        SLATE_500,
                        egui::RichText::new(
                            language.text("How opposite-direction overlaps resolve."),
                        )
                        .font(FontId::new(12.0, egui::FontFamily::Proportional)),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    // One owner for the outlined secondary action (R2 round-2
                    // issue 2): the same theme::secondary_button the mapping
                    // card renders, not a local copy and not a global egui::Button.
                    let restore_btn = theme::secondary_button(
                        ui,
                        theme::Icon::Restore,
                        language.text("Restore timing defaults"),
                    );
                    if restore_btn.clicked() {
                        messages.push(Message::RestoreTimingDefaults);
                    }
                });
            });

            // Card content container
            theme::group_style()
                .inner_margin(Margin::same(GROUP_PADDING as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 12.0);

                    // 1. Mode selector
                    mode_selector(ui, timing.mode, language, messages);

                    // 2. Random Mix ratio group
                    if timing.mode == SocdMode::RandomMix {
                        rate_group(ui, timing, inputs, editing, language, messages);
                    }

                    // 3. New Key Press Delay duration group
                    if matches!(timing.mode, SocdMode::PressDelay | SocdMode::RandomMix) {
                        let props = DurationRangeProps::new(
                            TimingField::TransitionMinimum,
                            TimingField::TransitionMaximum,
                            "New Key Press Delay",
                            INDIGO_600,
                        );
                        duration_range(ui, props, timing, inputs, editing, language, messages);
                    }

                    // 4. Previous Key Release Delay duration group
                    if matches!(timing.mode, SocdMode::ReleaseDelay | SocdMode::RandomMix) {
                        let props = DurationRangeProps::new(
                            TimingField::PreservedMinimum,
                            TimingField::PreservedMaximum,
                            "Previous Key Release Delay",
                            VIOLET_600,
                        );
                        duration_range(ui, props, timing, inputs, editing, language, messages);
                    }

                    // 5. Mechanism steps ("How it works"). It sits directly
                    // under the controls it explains, and takes the stretched
                    // card's bottom pin in the delay modes, where nothing
                    // follows it.
                    if timing.mode != SocdMode::RandomMix {
                        mechanism_steps(ui, timing.mode, timing, language);
                    }

                    // 6. Immediate mode illustrative preview (T5's card), the
                    // mode's last child, so the stretched card's leftover
                    // collects above it and the picture holds the bottom pin
                    // the mechanism block holds in the delay modes. The card
                    // illustrates fixed demo ranges rather than the draft, so
                    // the timing settings stay out of the call: it mounts only
                    // in Immediate mode, where the delay fields it would
                    // otherwise read belong to other modes.
                    if timing.mode == SocdMode::Immediate
                        && let Some(mount) = preview
                    {
                        preview::preview_card(
                            ui,
                            mount.preview,
                            language,
                            mount.awake,
                            mount.clock_mounted,
                            messages,
                        );
                    }
                });
            // The reference keeps the stretched card's content top-aligned
            // (`flex flex-col h-full`), so the row's leftover height collects
            // below the content; the row's helper measures the natural height
            // before growing the frame.
            super::app::stretch_card(ui, "timing");
        })
        .response
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{settings::Settings, ui::state::State};

    #[test]
    fn test_mode_labels_and_colors() {
        let lang = Language::English;
        for mode in SocdMode::ALL {
            assert!(!mode_label(mode, lang).is_empty());
            let color = mode_color(mode);
            assert!(color.a() > 0);
        }
    }

    #[test]
    fn test_value_box_ids_unique() {
        let mut ids = std::collections::HashSet::new();
        for field in TimingField::ALL {
            let id = value_box_id(field);
            assert!(ids.insert(id), "Duplicate ID for {field:?}");
        }
    }

    #[test]
    fn test_value_box_select_all_text() {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let id = value_box_id(TimingField::TransitionMinimum);
            let text = "12.5";
            select_all_text(ui, id, text);

            let state = TextEdit::load_state(ui.ctx(), id).expect("state loaded");
            let range = state.cursor.char_range().expect("range exists");
            let [start, end] = range.sorted_cursors();
            assert_eq!(start.index.0, 0);
            assert_eq!(end.index.0, text.chars().count());
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn test_mode_selector_emits_message() {
        let ctx = egui::Context::default();
        let mut messages = Vec::new();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            mode_selector(ui, SocdMode::PressDelay, Language::English, &mut messages);
        });
        output.drop_without_applying_deltas();
        assert!(messages.is_empty());
    }

    #[test]
    fn test_timing_card_immediate_mode() {
        let ctx = egui::Context::default();
        let timing = TimingSettings {
            mode: SocdMode::Immediate,
            ..Default::default()
        };
        let inputs = TimingInputs::from_timing(&timing);
        let editing = [false; 6];
        let mut messages = Vec::new();

        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            timing_card(
                ui,
                &timing,
                &inputs,
                &editing,
                Language::English,
                None,
                &mut messages,
            );
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn test_timing_card_press_delay_mode() {
        let ctx = egui::Context::default();
        let timing = TimingSettings {
            mode: SocdMode::PressDelay,
            ..Default::default()
        };
        let inputs = TimingInputs::from_timing(&timing);
        let editing = [false; 6];
        let mut messages = Vec::new();

        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            timing_card(
                ui,
                &timing,
                &inputs,
                &editing,
                Language::English,
                None,
                &mut messages,
            );
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn test_timing_card_release_delay_mode() {
        let ctx = egui::Context::default();
        let timing = TimingSettings {
            mode: SocdMode::ReleaseDelay,
            ..Default::default()
        };
        let inputs = TimingInputs::from_timing(&timing);
        let editing = [false; 6];
        let mut messages = Vec::new();

        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            timing_card(
                ui,
                &timing,
                &inputs,
                &editing,
                Language::English,
                None,
                &mut messages,
            );
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn test_timing_card_random_mix_mode() {
        let ctx = egui::Context::default();
        let timing = TimingSettings {
            mode: SocdMode::RandomMix,
            ..Default::default()
        };
        let inputs = TimingInputs::from_timing(&timing);
        let editing = [false; 6];
        let mut messages = Vec::new();

        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            timing_card(
                ui,
                &timing,
                &inputs,
                &editing,
                Language::English,
                None,
                &mut messages,
            );
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn test_range_slider_draw_and_clamping() {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let res = range_slider(
                ui,
                RangeSliderProps {
                    min_val: 2.0,
                    max_val: 8.0,
                    floor: 0.0,
                    enabled: true,
                    accent: INDIGO_600,
                    id: Id::new("test-slider"),
                    accessible_name: "Test duration range",
                },
            );
            assert_eq!(res, None);
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn test_mixer_slider_draw() {
        let ctx = egui::Context::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let res = mixer_slider(ui, 50.0, true, Language::English);
            assert_eq!(res, None);
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn test_mechanism_steps_generation() {
        let ctx = egui::Context::default();
        let timing = TimingSettings::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            mechanism_steps(ui, SocdMode::Immediate, &timing, Language::English);
            mechanism_steps(ui, SocdMode::PressDelay, &timing, Language::English);
            mechanism_steps(ui, SocdMode::ReleaseDelay, &timing, Language::English);
            mechanism_steps(ui, SocdMode::RandomMix, &timing, Language::English);
        });
        output.drop_without_applying_deltas();
    }

    /// The reference (`renderMechanism`) floats the block as a white card
    /// (`bg-white border-slate-200/70 rounded-2xl`), not a dark grey inset
    /// slot; neutral badges are `bg-slate-100` with a slate-500 digit, and
    /// step text is `text-slate-600`.
    #[test]
    fn test_mechanism_steps_use_the_floating_white_card() {
        use egui_kittest::Harness;

        let timing = TimingSettings::default();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build_ui(move |ui| {
                mechanism_steps(ui, SocdMode::Immediate, &timing, Language::English);
            });
        harness.run();

        // The white floating frame around the steps: WHITE fill, hairline
        // edge, the card radius (`rounded-2xl`), slot shadow.
        let card = painted_rects(&harness)
            .into_iter()
            .find(|rect| {
                rect.fill == WHITE
                    && rect.stroke.color == theme::SLATE_200
                    && rect.corner_radius == theme::CARD_RADIUS
            })
            .expect("the mechanism block paints the white floating card");
        // No dark-slot fill may wrap the steps.
        assert!(
            !painted_rects(&harness)
                .iter()
                .any(|rect| rect.fill == theme::SLATE_50_80 && rect.rect.width() > 200.0),
            "the dark inset slot frame must be gone"
        );
        // The neutral step badges sit on SLATE_100 with the chip radius
        // (`rounded-md`), the reference's 6px.
        assert!(
            painted_rects(&harness)
                .iter()
                .any(|rect| rect.fill == SLATE_100 && rect.corner_radius == theme::CHIP_RADIUS),
            "neutral step badges keep the bg-slate-100 fill and rounded-md corners"
        );
        // Step text renders at slate-600, the reference's step copy ink.
        assert_eq!(
            painted_text_color(&harness, "Detect an opposite-direction overlap."),
            Some(theme::SLATE_600),
            "step text keeps the readable slate-600 ink"
        );
        let _ = card;
    }

    /// The reference stacks the block as `space-y-2` (header to list) over
    /// `space-y-1.5` (between steps): the two gaps differ, so a single item
    /// spacing cannot express both. Each step row is a 16px content box
    /// (`leading-snug` over `text-[11px]`), and egui's 18px
    /// `interact_size.y` floor would inflate every row by 2px.
    #[test]
    fn test_mechanism_steps_keep_the_reference_row_pitch() {
        use egui_kittest::{Harness, kittest::Queryable};

        let timing = TimingSettings::default();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build_ui(move |ui| {
                mechanism_steps(ui, SocdMode::Immediate, &timing, Language::English);
            });
        harness.run();

        let header = harness.get_by_label("How it works").rect();
        let first = harness
            .get_by_label("Detect an opposite-direction overlap.")
            .rect();
        let second = harness
            .get_by_label("Release the previous key output immediately.")
            .rect();
        let third = harness
            .get_by_label("Send the new key immediately. 0 ms added delay")
            .rect();

        // The rhythm is measured top-to-top: each row is a 16px content box,
        // so the header-to-list gap (the reference's `space-y-2`) shows up as
        // a 24px pitch and the between-step gap (`space-y-1.5`) as 22px.
        // Measuring tops rather than bottoms keeps the assertion independent
        // of how the shaper sizes each label's line box.
        assert!(
            (first.top() - header.top() - 24.0).abs() < 1.0,
            "header to first step must pitch 24px, got {}",
            first.top() - header.top()
        );
        assert!(
            (second.top() - first.top() - 22.0).abs() < 1.0,
            "step pitch must be 22px, got {}",
            second.top() - first.top()
        );
        assert!(
            (third.top() - second.top() - 22.0).abs() < 1.0,
            "step pitch must be 22px, got {}",
            third.top() - second.top()
        );
    }

    /// The reference's header mark is `w-2 h-2 rounded-full bg-slate-300`
    /// beside a `text-slate-800` title, not the slate-500 ink the dark-slot
    /// leftover carried.
    #[test]
    fn test_mechanism_header_keeps_the_slate_mark_and_title_ink() {
        use egui_kittest::Harness;

        let timing = TimingSettings::default();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 300.0))
            .build_ui(move |ui| {
                mechanism_steps(ui, SocdMode::Immediate, &timing, Language::English);
            });
        harness.run();

        let mark = harness
            .output()
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Circle(circle) if circle.fill == theme::SLATE_300 => {
                    Some(circle.radius)
                }
                _ => None,
            })
            .expect("the header paints the slate-300 mark");
        assert!(
            (mark - 4.0).abs() < 0.01,
            "the mark is the reference's 8px circle, got radius {mark}"
        );
        assert_eq!(
            painted_text_color(&harness, "How it works"),
            Some(theme::SLATE_800),
            "the header title takes the reference's slate-800 ink"
        );
    }

    struct FakeRuntime {
        state: State,
        sent: Vec<crate::protocol::UiCommand>,
        card_rect: Rect,
    }

    impl FakeRuntime {
        fn new(state: State) -> Self {
            Self {
                state,
                sent: Vec::new(),
                card_rect: Rect::NOTHING,
            }
        }

        fn frame(&mut self, ui: &mut Ui) {
            let mut messages = Vec::new();
            if let Some(draft) = &self.state.draft {
                let resp = timing_card(
                    ui,
                    &draft.timing,
                    &self.state.inputs,
                    &self.state.editing,
                    self.state.language,
                    Some(PreviewMount {
                        preview: &self.state.preview,
                        awake: true,
                        clock_mounted: true,
                    }),
                    &mut messages,
                );
                self.card_rect = resp.rect;
            }
            for message in messages {
                for effect in super::super::state::update(&mut self.state, message) {
                    if let super::super::state::Effect::Send(command) = effect {
                        self.sent.push(command);
                    }
                }
            }
        }
    }

    fn test_state() -> State {
        let mut state = State {
            connected: true,
            draft: Some(Settings::default()),
            inputs: TimingInputs::from_timing(&TimingSettings::default()),
            ..State::default()
        };
        state.draft.as_mut().unwrap().timing.mode = SocdMode::PressDelay;
        state
    }

    #[test]
    fn test_value_box_edit_and_read_back() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(test_state()),
            );

        // 1. Find box by accessible name ("Transition Minimum")
        assert_eq!(
            harness
                .get_by_label("Transition Minimum")
                .value()
                .as_deref(),
            Some("2.0")
        );

        // 2. Click and type new value through harness
        harness.get_by_label("Transition Minimum").click();
        harness.run();
        harness.get_by_label("Transition Minimum").type_text("15.5");
        harness.run();

        // 3. Submit with Enter
        harness.key_press(egui::Key::Enter);
        harness.run();

        // 4. Verify state::update committed the value
        let runtime_state = &harness.state().state;
        assert_eq!(
            runtime_state
                .draft
                .as_ref()
                .unwrap()
                .timing
                .socd_transition_min_micros,
            15500
        );
        assert_eq!(runtime_state.inputs.transition_minimum, "15.5");

        // And verify box reads back the committed value by label
        assert_eq!(
            harness
                .get_by_label("Transition Minimum")
                .value()
                .as_deref(),
            Some("15.5")
        );
    }

    #[test]
    fn test_value_box_second_click_places_caret() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(test_state()),
            );

        let id = value_box_id(TimingField::TransitionMinimum);

        // First click: gains focus and selects all text
        harness.get_by_label("Transition Minimum").click();
        harness.run();

        let state1 =
            TextEdit::load_state(&harness.ctx, id).expect("state loaded after first click");
        let range1 = state1.cursor.char_range().expect("range exists");
        let [s1, e1] = range1.sorted_cursors();
        assert_ne!(s1, e1, "first click must select all text");

        // Second click: inside already-armed box
        harness.get_by_label("Transition Minimum").click();
        harness.run();

        let state2 =
            TextEdit::load_state(&harness.ctx, id).expect("state loaded after second click");
        let range2 = state2
            .cursor
            .char_range()
            .expect("range exists after second click");
        let [s2, e2] = range2.sorted_cursors();
        assert_eq!(
            s2, e2,
            "second click must place a collapsed caret, not re-select all"
        );
    }

    #[test]
    fn test_value_box_rearm_after_focus_move() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(test_state()),
            );

        let id = value_box_id(TimingField::TransitionMinimum);

        // Click first box twice (places caret)
        harness.get_by_label("Transition Minimum").click();
        harness.run();
        harness.get_by_label("Transition Minimum").click();
        harness.run();

        let state_collapsed = TextEdit::load_state(&harness.ctx, id).expect("state loaded");
        let [sc, ec] = state_collapsed
            .cursor
            .char_range()
            .unwrap()
            .sorted_cursors();
        assert_eq!(sc, ec, "caret placed");

        // Move focus elsewhere: click "Transition Maximum" box
        harness.get_by_label("Transition Maximum").click();
        harness.run();

        // Click Transition Minimum again: must select all again (rearmed via track_box_focus)
        harness.get_by_label("Transition Minimum").click();
        harness.run();

        let state_rearmed = TextEdit::load_state(&harness.ctx, id).expect("state loaded");
        let [sr, er] = state_rearmed.cursor.char_range().unwrap().sorted_cursors();
        assert_ne!(
            sr, er,
            "press after focus moved elsewhere must select all again"
        );
    }

    /// Every `Shape::Rect` this frame painted, flattened out of `Shape::Vec`.
    fn painted_rects<State>(
        harness: &egui_kittest::Harness<'_, State>,
    ) -> Vec<egui::epaint::RectShape> {
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

    /// Radii of the painted circles ringed in `ink`, in paint order.
    fn painted_circle_radii<State>(
        harness: &egui_kittest::Harness<'_, State>,
        ink: Color32,
    ) -> Vec<f32> {
        harness
            .output()
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Circle(circle) if circle.stroke.color == ink => Some(circle.radius),
                _ => None,
            })
            .collect()
    }

    /// The reference's slider handle is a `w-4 h-4` box with a 3px border drawn
    /// *inside* it, so the drawn handle is 16px across around a 10px white
    /// core. epaint paints the stroke entirely outside the circle's path
    /// instead, so the drawn box is `2 * (radius + stroke)` and the path radius
    /// has to be `8 - 3 = 5`. The `8 - 3/2` this replaces assumed a straddling
    /// stroke and drew a 19px handle -- visibly heavier than the reference.
    #[test]
    fn test_slider_thumb_draws_the_reference_16px_box() {
        use egui_kittest::Harness;

        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 200.0))
            .build_ui(|ui| {
                let _ = range_slider(
                    ui,
                    RangeSliderProps {
                        min_val: 2.0,
                        max_val: 8.0,
                        floor: 0.0,
                        enabled: true,
                        accent: INDIGO_600,
                        id: Id::new("thumb-box-test"),
                        accessible_name: "Thumb box range",
                    },
                );
            });
        harness.run();

        let drawn = painted_circle_radii(&harness, INDIGO_600);
        assert_eq!(drawn, vec![THUMB_RADIUS, THUMB_RADIUS]);

        // epaint's `CircleShape::visual_bounding_rect` is
        // `radius * 2.0 + stroke.width` (the stroke is painted `outside()`),
        // so this is the box the user actually sees.
        let box_size = 2.0 * (THUMB_RADIUS + SLIDER_HANDLE_BORDER);
        assert!(
            (box_size - 16.0).abs() < 0.01,
            "the drawn handle must be the reference's 16px `w-4 h-4` box, got {box_size}"
        );
        // The white core the border encloses: the reference's `16 - 2 * 3`.
        let core = 2.0 * THUMB_RADIUS;
        assert!(
            (core - 10.0).abs() < 0.01,
            "the handle's white core must match the reference's 10px, got {core}"
        );
    }

    /// Every manual input figure paints **one** pass, so a pill carries one
    /// weight.
    ///
    /// The reference writes its entry boxes `font-mono font-bold`, which is a
    /// *face* selection the port cannot make (egui's bundled faces ship a
    /// single weight), so the stamp standing in for it offsets a second pass by
    /// 0.48 px at this size -- a smear, not a heavier figure. What a reader
    /// actually sees is that the figure disagreed with the `~`, `:`, `ms` and
    /// `%` glyphs beside it in the same pill. A stamp creeping back in here is
    /// the regression this pins.
    #[test]
    fn test_value_box_figure_paints_one_pass() {
        use egui_kittest::Harness;

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(test_state()),
            );
        harness.run();

        // `test_state` is Press Delay, whose defaults are a 2.0~4.0 ms
        // transition range, so both boxes are on screen.
        for value in ["2.0", "4.0"] {
            let passes = harness
                .output()
                .shapes
                .iter()
                .filter(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => text.galley.text() == value,
                    _ => false,
                })
                .count();
            assert_eq!(
                passes, 1,
                "{value} must paint exactly one pass (the edit's own); \
                 a second means the stamp is back and the figure disagrees \
                 with the unit beside it"
            );
        }
    }

    /// The figure and the unit sharing its pill are the same weight, measured
    /// the way a reader sees it: the *number of paint passes* per glyph.
    ///
    /// This is the property the request named. Both `2.0` (a `TextEdit`) and
    /// `ms` (a plain label) resolve to a single pass, so neither reads heavier
    /// than the other. A stamp on the figure alone -- whatever its ink or size
    /// -- makes the counts disagree, which is exactly the mixed-weight pill.
    #[test]
    fn test_value_and_unit_in_a_pill_share_one_weight() {
        use egui_kittest::Harness;

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(test_state()),
            );
        harness.run();

        let passes = |needle: &str| {
            harness
                .output()
                .shapes
                .iter()
                .filter(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => text.galley.text() == needle,
                    _ => false,
                })
                .count()
        };

        for (figure, unit) in [("2.0", "ms"), ("4.0", "ms")] {
            assert_eq!(
                passes(figure),
                passes(unit),
                "the {figure} figure and the {unit} unit share a pill and must \
                 share a weight: {figure} paints {} pass(es), {unit} paints {}",
                passes(figure),
                passes(unit)
            );
        }
    }

    /// The strip is the reference's own card, not a `group_style` surface: a
    /// white fill with a slate hairline and `shadow-2xs`, its segments inset by
    /// `MODE_STRIP_PADDING` and spaced by `MODE_STRIP_GAP` (`grid grid-cols-4
    /// gap-1 p-1 bg-white border border-slate-200 rounded-xl shadow-2xs`).
    /// Rendered rather than restated, so a flattened margin fails here.
    #[test]
    fn test_mode_selector_strip_inset_and_segment_padding() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 200.0))
            .build_ui(|ui| {
                let mut messages = Vec::new();
                mode_selector(ui, SocdMode::PressDelay, Language::English, &mut messages);
            });
        harness.run();

        // The strip is the only white `CONTROL_RADIUS` frame in this render.
        // Asserting the fill separates it from the `group_style` slate inset
        // the old strip used, and the `SHADOW_2XS` paint is checked below.
        let strip = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.corner_radius == theme::CONTROL_RADIUS && rect.fill == WHITE)
            .expect("the mode strip paints its white card frame");
        assert_eq!(
            strip.stroke.color,
            theme::SLATE_200,
            "the strip's hairline is `border-slate-200`, not the card's indigo edge"
        );

        // `shadow-2xs` is a zero-blur drop; the strip must carry it and not the
        // blurred `SHADOW_XS` that the selected segment takes.
        let lift = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.fill == theme::SHADOW_2XS.color)
            .expect("the strip paints its shadow-2xs lift");
        assert_eq!(lift.blur_width, 0.0, "`shadow-2xs` carries no blur");

        let mut previous_right: Option<f32> = None;
        for mode in SocdMode::ALL {
            let label = mode_label(mode, Language::English);
            let segment = harness
                .get_by_role_and_label(egui::accesskit::Role::Button, label)
                .rect();

            // 4px strip inset + the frame's own 1px stroke.
            assert!(
                segment.min.x - strip.rect.min.x >= 4.0,
                "{label}: strip must inset the segments by 4 (left gap {})",
                segment.min.x - strip.rect.min.x
            );
            assert!(
                segment.min.y - strip.rect.min.y >= 4.0,
                "{label}: strip must inset the segments by 4 (top gap {})",
                segment.min.y - strip.rect.min.y
            );

            // `gap-1`: consecutive segments are 4 apart. `columns` derives its
            // gap from `item_spacing.x`, so a flattened spacing collapses the
            // segments against each other and fails here.
            if let Some(right) = previous_right {
                let gap = segment.min.x - right;
                assert!(
                    (gap - theme::MODE_STRIP_GAP).abs() < 0.01,
                    "{label}: segments must be spaced by `gap-1` (measured {gap})"
                );
            }
            previous_right = Some(segment.max.x);

            // The segment is the label's own line box plus `py-1.5` on each
            // side. Asserting the relationship rather than an absolute height
            // is deliberate: the harness lays the label out with egui's bundled
            // faces (14px) while the running window uses the native UI face
            // (16px), so the reference's 28px is `16 + 2 * 6` there and
            // `14 + 2 * 6` here. A hardcoded 28 would pass only in the window
            // and a hardcoded 26 only in the harness -- and the earlier `7`
            // padding was calibrated to exactly that mistake, drawing a 30px
            // segment on screen.
            let label_rect = harness
                .get_by_role_and_label(egui::accesskit::Role::Label, label)
                .rect();
            let expected = label_rect.height() + 2.0 * theme::MODE_PADDING.top;
            assert!(
                (segment.height() - expected).abs() < 0.01,
                "{label}: segment height must be the label plus `py-1.5` on each side \
                 (label {}, expected {expected}, measured {})",
                label_rect.height(),
                segment.height()
            );
        }
    }

    /// F08: both sliders in the timing card share one geometry -- a 12px rail
    /// rounded to 6 and constant 16px thumbs -- and the mixer handle keeps its
    /// size under hover and drag, as the reference does.
    #[test]
    fn test_sliders_share_one_geometry() {
        use egui_kittest::{Harness, kittest::Queryable};

        // The duration rail's geometry.
        let mut duration = Harness::builder()
            .with_size(egui::vec2(600.0, 200.0))
            .build_ui(|ui| {
                let _ = range_slider(
                    ui,
                    RangeSliderProps {
                        min_val: 2.0,
                        max_val: 8.0,
                        floor: 0.0,
                        enabled: true,
                        accent: INDIGO_600,
                        id: Id::new("shared-geometry-range"),
                        accessible_name: "Shared geometry range",
                    },
                );
            });
        duration.run();
        let track = painted_rects(&duration)
            .into_iter()
            .find(|rect| rect.fill == SLATE_100)
            .expect("the duration rail paints its SLATE_100 track");
        let duration_rail = (track.rect.height(), track.corner_radius);
        let duration_thumbs = painted_circle_radii(&duration, INDIGO_600);

        // The Random Mix rail's geometry.
        let mut mixer = Harness::builder()
            .with_size(egui::vec2(600.0, 200.0))
            .build_ui(|ui| {
                let _ = mixer_slider(ui, 50.0, true, Language::English);
            });
        mixer.run();
        let span = painted_rects(&mixer)
            .into_iter()
            .find(|rect| rect.fill == INDIGO_600)
            .expect("the mixer paints its left span");
        let mixer_rail = (span.rect.height(), span.corner_radius);
        let mixer_handles = painted_circle_radii(&mixer, PURPLE_600);

        assert_eq!(
            duration_rail,
            (12.0, theme::CHIP_RADIUS),
            "the duration rail keeps the reference's 12px rounded-6 track"
        );
        assert_eq!(
            mixer_rail, duration_rail,
            "the Random Mix rail must share the duration rail's dimensions"
        );
        assert_eq!(
            duration_thumbs,
            vec![THUMB_RADIUS, THUMB_RADIUS],
            "both duration thumbs keep the constant 16px drawn box"
        );
        assert_eq!(
            mixer_handles,
            vec![THUMB_RADIUS],
            "the mixer handle must share the 16px drawn box"
        );

        // The reference handle has no status-dependent size: hovering and
        // dragging the mixer must leave it at 16px.
        let slider = mixer.get_by_role_and_label(egui::accesskit::Role::Slider, "Delay Mix Ratio");
        let rect = slider.rect();
        slider.hover();
        mixer.run();
        assert_eq!(
            painted_circle_radii(&mixer, PURPLE_600),
            vec![THUMB_RADIUS],
            "hovering must not resize the mixer handle"
        );
        mixer.drag_at(rect.center());
        mixer.run();
        assert_eq!(
            painted_circle_radii(&mixer, PURPLE_600),
            vec![THUMB_RADIUS],
            "dragging must not resize the mixer handle"
        );
        mixer.drop_at(rect.center());
    }

    /// The selected segment is `bg-indigo-600` with white bold text and
    /// `shadow-xs`; every other segment is transparent with `text-slate-600`
    /// ink. The chip does **not** follow the mode accent -- the reference
    /// hardcodes the class for all four modes, so Release Delay selects the same
    /// indigo chip the rest do.
    #[test]
    fn test_mode_selector_active_segment_is_the_fixed_indigo_chip() {
        use egui_kittest::{Harness, kittest::Queryable};

        for mode in SocdMode::ALL {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(600.0, 200.0))
                .build_ui(move |ui| {
                    let mut messages = Vec::new();
                    mode_selector(ui, mode, Language::English, &mut messages);
                });
            harness.run();

            let active = mode_label(mode, Language::English);
            let chip = painted_rects(&harness)
                .into_iter()
                .find(|rect| rect.fill == INDIGO_600)
                .unwrap_or_else(|| {
                    panic!("{mode:?}: the active segment must paint the indigo-600 chip")
                });
            assert_eq!(
                chip.corner_radius,
                theme::SEGMENT_RADIUS,
                "{mode:?}: the chip keeps the segment's `rounded-lg` corners"
            );
            // The chip covers the whole segment, so its height is the label's
            // line box plus `py-1.5` on each side (see the strip test for why
            // this is a relationship rather than the reference's literal 28).
            let label_rect = harness
                .get_by_role_and_label(egui::accesskit::Role::Label, active)
                .rect();
            let expected = label_rect.height() + 2.0 * theme::MODE_PADDING.top;
            assert!(
                (chip.rect.height() - expected).abs() < 0.01,
                "{mode:?}: the chip must span the segment's padded height \
                 (expected {expected}, measured {})",
                chip.rect.height()
            );

            // The chip's own `shadow-xs` lift, distinct from the strip's
            // zero-blur `shadow-2xs` underneath it.
            assert!(
                painted_rects(&harness).into_iter().any(|rect| {
                    rect.fill == theme::SHADOW_XS.color
                        && rect.blur_width == theme::SHADOW_XS.blur as f32
                        && rect.rect.intersects(chip.rect)
                }),
                "{mode:?}: the selected segment carries the `shadow-xs` lift"
            );

            let active = mode_label(mode, Language::English);
            assert_eq!(
                painted_text_color(&harness, active),
                Some(theme::WHITE),
                "{active}: the active segment's label must render white"
            );
            for other in SocdMode::ALL {
                if other == mode {
                    continue;
                }
                let label = mode_label(other, Language::English);
                assert_eq!(
                    painted_text_color(&harness, label),
                    Some(theme::SLATE_600),
                    "{label}: inactive segments keep the `text-slate-600` ink"
                );
            }
        }
    }

    /// The timing card's vertical order: the mode strip, then the controls the
    /// mode owns, then "How it works", and -- in Immediate mode, the one mode
    /// that mounts it -- the preview last.
    ///
    /// The preview is the card's bottom pin in Immediate mode the way the
    /// mechanism block is in the delay modes: it is the last child, so the
    /// stretched card's leftover collects above it. Both orders are asserted
    /// as the relationship between the two blocks' own rects, because the
    /// card stretches to the row's shared height and no absolute y is stable.
    #[test]
    fn test_timing_card_puts_the_preview_after_the_mechanism_block() {
        use egui_kittest::{Harness, kittest::Queryable};

        let timing = TimingSettings::default();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 900.0))
            .build_ui(move |ui| {
                let inputs = TimingInputs::from_timing(&timing);
                let editing = [false; 6];
                let mut messages = Vec::new();
                let preview = Preview::default();
                timing_card(
                    ui,
                    &timing,
                    &inputs,
                    &editing,
                    Language::English,
                    Some(PreviewMount {
                        preview: &preview,
                        awake: true,
                        clock_mounted: true,
                    }),
                    &mut messages,
                );
            });
        harness.ctx.all_styles_mut(|style| *style = theme::style());
        harness.ctx.set_fonts(theme::fonts());
        harness.run();
        harness.run();

        let steps = harness.get_by_label("How it works").rect();
        let pill = harness.get_by_label("Play preview").rect();
        assert!(
            pill.top() > steps.top(),
            "the preview must follow the mechanism block \
             (steps top {}, preview top {})",
            steps.top(),
            pill.top()
        );

        // The strip still leads both: the mode selector is the first control,
        // so nothing may be pushed above it.
        let strip = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.corner_radius == theme::CONTROL_RADIUS && rect.fill == WHITE)
            .expect("the mode strip paints its white card frame");
        assert!(
            steps.top() > strip.rect.top() && pill.top() > strip.rect.top(),
            "the mode strip must stay the card's first control \
             (strip top {}, steps top {}, preview top {})",
            strip.rect.top(),
            steps.top(),
            pill.top()
        );
    }

    /// R2 round-3: the mixer fills the rail `INDIGO_600` left of the handle
    /// and `VIOLET_500` right of it.
    #[test]
    fn test_mixer_rail_splits_primary_left_release_right() {
        use egui_kittest::Harness;

        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 200.0))
            .build_ui(|ui| {
                let _ = mixer_slider(ui, 50.0, true, Language::English);
            });
        harness.run();

        let rail: Vec<_> = painted_rects(&harness)
            .into_iter()
            .filter(|rect| rect.corner_radius == theme::CHIP_RADIUS)
            .collect();
        assert_eq!(rail.len(), 2, "the rail paints two spans: {rail:?}");

        let left = rail
            .iter()
            .find(|rect| rect.fill == INDIGO_600)
            .expect("the span left of the handle is INDIGO_600");
        let right = rail
            .iter()
            .find(|rect| rect.fill == VIOLET_500)
            .expect("the span right of the handle is VIOLET_500");

        assert!(
            left.rect.max.x <= right.rect.min.x + f32::EPSILON,
            "the two spans must meet at the handle: left {:?}, right {:?}",
            left.rect,
            right.rect
        );
        assert!(
            !rail.iter().any(|rect| rect.fill == SLATE_100),
            "the mixer rail carries no neutral SLATE_100 track"
        );
        assert_eq!(
            left.rect.height(),
            RAIL_WIDTH,
            "the rail takes the card's shared 12px height"
        );
    }

    /// R2 round-3: the duration rail is a two-handle range slider, not the
    /// single-handle `accent_slider`: a 12px rail rounded to 6 with constant
    /// 16px thumbs that do not change with the status
    /// (F08 unified the mixer onto this same
    /// geometry.
    #[test]
    fn test_duration_rail_uses_the_range_geometry() {
        use egui_kittest::Harness;

        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 200.0))
            .build_ui(|ui| {
                let res = range_slider(
                    ui,
                    RangeSliderProps {
                        min_val: 2.0,
                        max_val: 8.0,
                        floor: 0.0,
                        enabled: true,
                        accent: INDIGO_600,
                        id: Id::new("rail-geometry-test"),
                        accessible_name: "Rail geometry range",
                    },
                );
                assert_eq!(res, None);
            });
        harness.run();

        let track = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.fill == SLATE_100)
            .expect("the rail paints its SLATE_100 track");
        assert_eq!(
            track.rect.height(),
            RAIL_WIDTH,
            "the duration rail is 12px tall, not the retired accent_slider 10"
        );
        assert_eq!(
            track.corner_radius,
            theme::CHIP_RADIUS,
            "the duration rail is rounded to 6, not the retired accent_slider 5"
        );

        assert_eq!(
            painted_circle_radii(&harness, INDIGO_600),
            vec![THUMB_RADIUS, THUMB_RADIUS],
            "both thumbs keep the constant 16px size"
        );

        // The rail has no status-dependent thumb size, so a drag must
        // leave the geometry alone.
        harness.drag_at(egui::pos2(300.0, 100.0));
        harness.run();
        harness.hover_at(egui::pos2(340.0, 100.0));
        harness.run();
        assert_eq!(
            painted_circle_radii(&harness, INDIGO_600),
            vec![THUMB_RADIUS, THUMB_RADIUS],
            "dragging must not resize the two-handle thumbs"
        );
        harness.drop_at(egui::pos2(340.0, 100.0));
    }

    /// The timing card's two slot frames are the reference's white tile
    /// (`p-3 rounded-2xl border-slate-200/70 shadow-2xs`), so both take
    /// [`theme::SHADOW_2XS`] -- a **zero-blur** drop. The port drew them with a
    /// separate `SHADOW_SLOT` at a 2px blur until the two were folded, which is
    /// the one pixel change this refactor carries: the tiles now sit on the
    /// reference's own hairline drop rather than a blurred one.
    #[test]
    fn test_slot_frames_use_the_zero_blur_hairline_drop() {
        use egui_kittest::{Harness, kittest::Queryable};

        let timing = TimingSettings {
            mode: SocdMode::RandomMix,
            ..Default::default()
        };
        let inputs = TimingInputs::from_timing(&timing);
        let editing = [false; 6];
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 500.0))
            .build_ui(move |ui| {
                let mut messages = Vec::new();
                rate_group(
                    ui,
                    &timing,
                    &inputs,
                    &editing,
                    Language::English,
                    &mut messages,
                );
            });
        harness.run();

        // The ratio slot is the frame wrapping the mixer, identified by
        // containment the way the padding test does.
        let rail = harness
            .get_by_role_and_label(egui::accesskit::Role::Slider, "Delay Mix Ratio")
            .rect();
        let slot = painted_rects(&harness)
            .into_iter()
            .find(|rect| {
                rect.fill == WHITE
                    && rect.corner_radius == theme::CONTROL_RADIUS
                    && rect.rect.contains_rect(rail)
            })
            .expect("the ratio group paints its white slot frame");
        assert!(
            slot.rect.contains_rect(rail),
            "the slot frame wraps its mixer"
        );

        // Every drop this card paints is the zero-blur `shadow-2xs`: the slot
        // tiles, the ratio pill, and the mixer's own value pill. The blurred
        // `SHADOW_XS` is what the retired `SHADOW_SLOT` carried, so any 2px
        // blur in this render is the regression.
        let drops: Vec<_> = painted_rects(&harness)
            .into_iter()
            .filter(|rect| rect.fill == theme::SHADOW_2XS.color)
            .collect();
        assert!(
            !drops.is_empty(),
            "the slot paints its `shadow-2xs` drop behind the frame"
        );
        for drop in &drops {
            assert_eq!(
                drop.blur_width, 0.0,
                "every drop in the timing card takes `shadow-2xs`, which carries no blur"
            );
        }
        assert!(
            drops
                .iter()
                .any(|drop| drop.corner_radius == theme::CONTROL_RADIUS),
            "the slot tile's own drop follows the frame's radius"
        );
    }

    /// R2 round-3: the Random Mix slot is padded with `theme::GROUP_PADDING` (14),
    /// not the 12 the duration groups use.
    #[test]
    fn test_rate_group_uses_group_padding() {
        use egui_kittest::{Harness, kittest::Queryable};

        let timing = TimingSettings {
            mode: SocdMode::RandomMix,
            ..Default::default()
        };
        let inputs = TimingInputs::from_timing(&timing);
        let editing = [false; 6];
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .build_ui(move |ui| {
                let mut messages = Vec::new();
                rate_group(
                    ui,
                    &timing,
                    &inputs,
                    &editing,
                    Language::English,
                    &mut messages,
                );
            });
        harness.run();

        // The mixer's own rect is unique in this render, and the slot is the one
        // `slot_style` frame that contains it. Selecting by containment rather
        // than by fill+radius alone keeps this unambiguous: the ratio pill shares
        // both the WHITE fill and the CONTROL_RADIUS corners.
        let rail = harness
            .get_by_role_and_label(egui::accesskit::Role::Slider, "Delay Mix Ratio")
            .rect();
        let slot = painted_rects(&harness)
            .into_iter()
            .find(|rect| {
                rect.corner_radius == theme::CONTROL_RADIUS
                    && rect.fill == WHITE
                    && rect.rect.contains_rect(rail)
            })
            .expect("the ratio group paints the slot frame around its mixer");

        // The slot's content starts GROUP_PADDING + the frame's 1px stroke in.
        // The 12px-padded duration groups measure 13 here instead.
        assert_eq!(
            rail.min.x - slot.rect.min.x,
            GROUP_PADDING + 1.0,
            "the ratio group is padded by GROUP_PADDING + its stroke"
        );
    }

    #[test]
    fn test_timing_card_has_non_zero_frame_padding() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(test_state()),
            );

        harness.run();

        // The first child widget inside timing_card is the "Input timings" section title
        let title_node = harness.get_by_label("Input timings");
        let title_rect = title_node.rect();
        let card_rect = harness.state().card_rect;

        assert_ne!(card_rect, Rect::NOTHING, "card rect must be recorded");

        let left_gap = title_rect.min.x - card_rect.min.x;
        let top_gap = title_rect.min.y - card_rect.min.y;

        assert!(
            left_gap >= CARD_PADDING - 1.0,
            "left gap must be at least CARD_PADDING: left_gap={left_gap}, CARD_PADDING={CARD_PADDING}"
        );
        assert!(
            top_gap >= CARD_PADDING - 1.0,
            "top gap must be at least CARD_PADDING: top_gap={top_gap}, CARD_PADDING={CARD_PADDING}"
        );
    }

    /// State with both a draft (timing card) and a runtime snapshot (mapping card)
    /// so one harness can render the two cards side by side.
    fn dual_card_state() -> State {
        use crate::{
            core::PhysicalKey,
            protocol::{DisplayKey, UiSnapshot},
            settings::Settings,
        };

        let keys = std::array::from_fn(|index| DisplayKey {
            physical: PhysicalKey::new(0x11 + index as u16, false),
            name: ["W", "S", "A", "D"][index].into(),
        });
        let mut state = State {
            connected: true,
            draft: Some(Settings::default()),
            snapshot: Some(UiSnapshot {
                filter_enabled: true,
                saved: Settings::default(),
                draft: Settings::default(),
                keys,
                capture_slot: None,
                measurement_active: false,
                measurement: None,
            }),
            inputs: TimingInputs::from_timing(&TimingSettings::default()),
            ..State::default()
        };
        state.draft.as_mut().unwrap().timing.mode = SocdMode::PressDelay;
        state
    }

    /// The colour a label actually renders in: the galley's own layout colour,
    /// falling back to the paint call's colour only when the layout used
    /// [`Color32::PLACEHOLDER`]. A label laid out in a real colour renders in
    /// that colour regardless of the paint call, which is the trap both cards'
    /// restore controls must avoid.
    fn painted_text_color<State>(
        harness: &egui_kittest::Harness<'_, State>,
        needle: &str,
    ) -> Option<Color32> {
        harness.output().shapes.iter().find_map(|clipped| {
            let egui::Shape::Text(text) = &clipped.shape else {
                return None;
            };
            if !text.galley.text().contains(needle) {
                return None;
            }
            let layout_color = text
                .galley
                .job
                .sections
                .first()
                .map(|section| section.format.color)
                .unwrap_or(Color32::PLACEHOLDER);
            Some(if layout_color == Color32::PLACEHOLDER {
                text.fallback_color
            } else {
                layout_color
            })
        })
    }

    /// R2 round-2 issue 2: the timing card's Restore control must be the same
    /// outlined secondary action the mapping card renders -- same accessible
    /// role, same rest ink, same hover ink.
    #[test]
    fn test_restore_control_matches_the_mapping_card_control() {
        use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 1600.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| {
                    runtime.frame(ui);
                    if runtime.state.snapshot.is_some() {
                        let _ = super::super::mapping::key_mappings_card(ui, &runtime.state);
                    }
                },
                FakeRuntime::new(dual_card_state()),
            );
        harness.run();

        // Same accessible role: both controls publish a labelled button node.
        let timing_role = harness
            .get_by_label("Restore timing defaults")
            .accesskit_node()
            .role();
        let mapping_role = harness
            .get_by_label("Restore mapping defaults")
            .accesskit_node()
            .role();
        assert_eq!(
            timing_role, mapping_role,
            "both cards' restore controls must report the same accessible role"
        );

        // Same rest ink.
        let timing_rest = painted_text_color(&harness, "Restore timing defaults");
        assert_eq!(timing_rest, Some(theme::SLATE_600));
        assert_eq!(
            timing_rest,
            painted_text_color(&harness, "Restore mapping defaults"),
            "both cards' restore labels must render the same rest ink"
        );

        // Same hover ink: hover each control and compare.
        harness.get_by_label("Restore timing defaults").hover();
        harness.run();
        let timing_hover = painted_text_color(&harness, "Restore timing defaults");
        assert_eq!(timing_hover, Some(theme::INDIGO_600));

        harness.get_by_label("Restore mapping defaults").hover();
        harness.run();
        let mapping_hover = painted_text_color(&harness, "Restore mapping defaults");
        assert_eq!(
            timing_hover, mapping_hover,
            "both cards' restore labels must render the same hover ink"
        );
        assert_eq!(
            painted_text_color(&harness, "Restore timing defaults"),
            timing_rest,
            "the timing control must return to its rest ink when the pointer leaves"
        );
    }

    fn rate_of(harness: &egui_kittest::Harness<'_, FakeRuntime>) -> u8 {
        harness
            .state()
            .state
            .draft
            .as_ref()
            .unwrap()
            .timing
            .overlap_preservation_rate
    }

    fn release_delay_state() -> State {
        let mut state = test_state();
        state.draft.as_mut().unwrap().timing.mode = SocdMode::ReleaseDelay;
        state
    }

    /// F10: both sides of the Random Mix ratio are editable boxes and either
    /// side writes the reciprocal preservation rate, so the pill and the
    /// mixer slider always show `press + release = 100`.
    #[test]
    fn test_mix_ratio_boxes_edit_both_sides_reciprocally() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut state = test_state();
        state.draft.as_mut().unwrap().timing.mode = SocdMode::RandomMix;
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(state),
            );

        assert_eq!(
            harness
                .get_by_label("Press Delay Percentage")
                .value()
                .as_deref(),
            Some("50"),
            "the press side mirrors the default rate"
        );
        assert_eq!(
            harness
                .get_by_label("Release Delay Percentage")
                .value()
                .as_deref(),
            Some("50")
        );

        // Typing into the press box stores the complement as the rate.
        harness.get_by_label("Press Delay Percentage").click();
        harness.run();
        harness
            .get_by_label("Press Delay Percentage")
            .type_text("30");
        harness.key_press(egui::Key::Enter);
        harness.run();

        assert_eq!(rate_of(&harness), 70, "press 30 must store a 70% rate");
        assert_eq!(
            harness
                .get_by_label("Press Delay Percentage")
                .value()
                .as_deref(),
            Some("30")
        );
        assert_eq!(
            harness
                .get_by_label("Release Delay Percentage")
                .value()
                .as_deref(),
            Some("70"),
            "the release box must follow the reciprocal"
        );
        // The mixer rail splits where the slider puts the press share: 30% in.
        let slider_rect = harness
            .get_by_role_and_label(egui::accesskit::Role::Slider, "Delay Mix Ratio")
            .rect();
        let spans: Vec<_> = painted_rects(&harness)
            .into_iter()
            .filter(|rect| {
                rect.corner_radius == theme::CHIP_RADIUS && slider_rect.contains_rect(rect.rect)
            })
            .collect();
        let press_span = spans
            .iter()
            .find(|rect| rect.fill == INDIGO_600)
            .expect("the mixer paints its press share span");
        let release_span = spans
            .iter()
            .find(|rect| rect.fill == VIOLET_500)
            .expect("the mixer paints its release share span");
        let share = (press_span.rect.max.x - press_span.rect.min.x)
            / (release_span.rect.max.x - press_span.rect.min.x);
        assert!(
            (share - 0.30).abs() < 0.02,
            "the rail split must sit at the 30% press share (measured {share})"
        );

        // The release box writes the rate directly.
        harness.get_by_label("Release Delay Percentage").click();
        harness.run();
        harness
            .get_by_label("Release Delay Percentage")
            .type_text("80");
        harness.key_press(egui::Key::Enter);
        harness.run();

        assert_eq!(rate_of(&harness), 80);
        assert_eq!(
            harness
                .get_by_label("Press Delay Percentage")
                .value()
                .as_deref(),
            Some("20")
        );
        assert_eq!(
            harness
                .get_by_label("Release Delay Percentage")
                .value()
                .as_deref(),
            Some("80")
        );
    }

    /// F25: the release-delay manual input holds the engine's 0.1 ms floor.
    /// 0.0 ms is rejected by the validator with its message and never reaches
    /// the runtime; 0.1 ms is accepted and applies.
    #[test]
    fn test_release_delay_manual_input_keeps_the_floor() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(release_delay_state()),
            );
        harness.get_by_label("Preserved Minimum").click();
        harness.run();
        harness.get_by_label("Preserved Minimum").type_text("0.0");
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(
            harness
                .state()
                .state
                .draft
                .as_ref()
                .unwrap()
                .timing
                .preserved_overlap_min_micros,
            0,
            "the typed 0.0 ms is what the validator must reject"
        );
        let effects = super::super::state::update(&mut harness.state_mut().state, Message::Apply);
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, super::super::state::Effect::Send(_))),
            "a below-floor release delay must not reach the runtime"
        );
        assert_eq!(
            harness.state().state.error.as_deref(),
            Some("preserved overlap duration must be at least 0.1 ms"),
            "Apply must name the 0.1 ms floor"
        );

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(release_delay_state()),
            );
        harness.get_by_label("Preserved Minimum").click();
        harness.run();
        harness.get_by_label("Preserved Minimum").type_text("0.1");
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(
            harness
                .state()
                .state
                .draft
                .as_ref()
                .unwrap()
                .timing
                .preserved_overlap_min_micros,
            100
        );
        let effects = super::super::state::update(&mut harness.state_mut().state, Message::Apply);
        assert_eq!(
            harness.state().state.error,
            None,
            "0.1 ms is the floor and must pass validation"
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, super::super::state::Effect::Send(_))),
            "a valid draft must reach the runtime"
        );
    }

    /// F25: the release-delay rail clamps a drag to the left edge at 0.1 ms.
    #[test]
    fn test_release_delay_slider_keeps_the_floor() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(release_delay_state()),
            );

        let rail = harness.get_by_role_and_label(
            egui::accesskit::Role::Slider,
            "Previous Key Release Delay duration range",
        );
        let rect = rail.rect();
        let edge = egui::pos2(rect.left() + 1.0, rect.center().y);
        harness.drag_at(rect.center());
        harness.run();
        harness.hover_at(edge);
        harness.run();
        harness.hover_at(edge);
        harness.run();
        harness.drop_at(edge);
        harness.run();

        assert_eq!(
            harness
                .state()
                .state
                .draft
                .as_ref()
                .unwrap()
                .timing
                .preserved_overlap_min_micros,
            100,
            "the rail must floor the release delay at 0.1 ms"
        );
    }

    /// The rail reaches the settings layer's shared ceiling, not the
    /// reference's 20 ms: a long delay must be draggable, and the value it
    /// produces must be one `Settings::validate` accepts.
    #[test]
    fn test_range_slider_reaches_the_settings_ceiling() {
        use egui_kittest::{Harness, kittest::Queryable};

        assert_eq!(
            RAIL_MAX_MS, 1000.0,
            "the rail reads the settings layer's 1000 ms ceiling"
        );

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime::new(test_state()),
            );

        let rail = harness.get_by_role_and_label(
            egui::accesskit::Role::Slider,
            "New Key Press Delay duration range",
        );
        let rect = rail.rect();

        // Drag the upper thumb to the rail's right edge.
        let edge = egui::pos2(rect.right() - 1.0, rect.center().y);
        harness.drag_at(rect.center());
        harness.run();
        harness.hover_at(edge);
        harness.run();
        harness.hover_at(edge);
        harness.run();
        harness.drop_at(edge);
        harness.run();

        let timing = &harness.state().state.draft.as_ref().unwrap().timing;
        assert_eq!(
            timing.socd_transition_max_micros,
            crate::settings::MAX_TIMING_MICROS,
            "the rail's right edge must reach the settings ceiling"
        );
        assert!(
            timing.socd_transition_max_micros > 20_000,
            "the ceiling must be past the retired 20 ms rail stop"
        );
    }
}
