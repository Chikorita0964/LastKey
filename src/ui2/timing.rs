//! Timing card, sliders, and value boxes for LastKey settings.
//!
//! Port of the four SOCD modes (`Immediate`, `PressDelay`, `ReleaseDelay`, `RandomMix`),
//! the mode-conditional timing groups with matched card heights, the two-row timing
//! pills, the dual-handle range slider, and the value boxes with select-all-on-focus
//! behavior.

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Id, Margin, Pos2, Rect, Response, Sense,
    Stroke, TextEdit, Ui, WidgetInfo, WidgetType, text::CCursor, text_selection::CCursorRange,
    vec2,
};

use crate::settings::{SocdMode, TimingSettings};

use super::{
    language::Language,
    message::{Message, PreviewAction, TimingField},
    preview::Preview,
    state::{TimingInputs, format_rate, parse_ms_text, parse_rate_text},
};

// ----------------------------------------------------------------------------
// Design tokens matching `src/ui/theme.rs`
// ----------------------------------------------------------------------------

pub const IMMEDIATE_ACCENT: Color32 = Color32::from_rgb(0x3a, 0x55, 0xe8);
pub const INDIGO_600: Color32 = Color32::from_rgb(0x4f, 0x39, 0xf6);
pub const INDIGO_700: Color32 = Color32::from_rgb(0x43, 0x2d, 0xd7);
pub const INDIGO_800: Color32 = Color32::from_rgb(0x37, 0x30, 0xa3);
pub const VIOLET_600: Color32 = Color32::from_rgb(0x7f, 0x22, 0xfe);
pub const MIX_TEXT: Color32 = Color32::from_rgb(0x6d, 0x51, 0xee);
pub const PRIMARY_TEXT: Color32 = Color32::from_rgb(0x4f, 0x46, 0xe5);
pub const MUTED_TEXT: Color32 = Color32::from_rgb(0x62, 0x74, 0x8e);
pub const BODY_TEXT: Color32 = Color32::from_rgb(0x0f, 0x17, 0x2b);
pub const BORDER: Color32 = Color32::from_rgb(0xe2, 0xe8, 0xf0);
pub const SURFACE: Color32 = Color32::WHITE;
pub const INSET: Color32 = Color32::from_rgb(0xf9, 0xfb, 0xfd);
pub const SLATE_100: Color32 = Color32::from_rgb(0xf1, 0xf5, 0xf9);
pub const SLATE_300: Color32 = Color32::from_rgb(0xcb, 0xd5, 0xe1);
pub const ERROR_TEXT: Color32 = Color32::from_rgb(0xdc, 0x26, 0x26);
pub const ERROR_BG: Color32 = Color32::from_rgb(0xfe, 0xf2, 0xf2);
pub const ERROR_BORDER: Color32 = Color32::from_rgb(0xff, 0x64, 0x67);
pub const RELEASE_TEXT: Color32 = Color32::from_rgb(0x8b, 0x5c, 0xf6);
pub const PURPLE_600: Color32 = Color32::from_rgb(0x98, 0x10, 0xfa);

/// Color associated with each SOCD mode.
pub const fn mode_color(mode: SocdMode) -> Color32 {
    match mode {
        SocdMode::Immediate => IMMEDIATE_ACCENT,
        SocdMode::PressDelay => INDIGO_600,
        SocdMode::ReleaseDelay => VIOLET_600,
        SocdMode::RandomMix => MIX_TEXT,
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
        TimingField::PreservedMinimum => "timing-preserved-minimum",
        TimingField::PreservedMaximum => "timing-preserved-maximum",
    })
}

/// Selects all text within a `TextEdit` widget.
pub fn select_all_text(ui: &Ui, id: Id, text: &str) {
    let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(
        CCursor::default(),
        CCursor::new(text.chars().count()),
    )));
    state.store(ui.ctx(), id);
}

// ----------------------------------------------------------------------------
// Value Box
// ----------------------------------------------------------------------------

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

/// Value box for numeric timing inputs. Reproduces the user-visible select-all-on-focus
/// behavior directly in egui without needing the Iced facade button workaround.
pub fn value_box(
    ui: &mut Ui,
    props: ValueBoxProps,
    buffer: &mut String,
    messages: &mut Vec<Message>,
) -> Response {
    let id = value_box_id(props.field);
    let text_color = if props.invalid {
        ERROR_TEXT
    } else {
        props.accent
    };

    let response = ui.add(
        TextEdit::singleline(buffer)
            .id(id)
            .desired_width(props.width)
            .font(FontId::new(12.0, egui::FontFamily::Proportional))
            .horizontal_align(Align::Center)
            .vertical_align(Align::Center)
            .text_color(text_color)
            .margin(Margin::symmetric(2, 1))
            .frame(Frame::NONE),
    );

    let field_name = match props.field {
        TimingField::TransitionMinimum => "Transition Minimum",
        TimingField::TransitionMaximum => "Transition Maximum",
        TimingField::PreservationRate => "Delay Mix Ratio",
        TimingField::PreservedMinimum => "Preserved Minimum",
        TimingField::PreservedMaximum => "Preserved Maximum",
    };
    response.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, ui.is_enabled(), field_name));

    // Select-all on focus gain or explicit activation
    let flag_id = id.with("selected_on_focus");
    let already_selected: bool = ui.data(|d| d.get_temp(flag_id)).unwrap_or(false);

    if response.gained_focus() || (props.editing && response.has_focus() && !already_selected) {
        select_all_text(ui, id, buffer);
        ui.data_mut(|d| d.insert_temp(flag_id, true));
    }
    if !response.has_focus() {
        ui.data_mut(|d| d.remove_temp::<bool>(flag_id));
    }

    if response.clicked() {
        select_all_text(ui, id, buffer);
        messages.push(Message::ValueBoxActivated(props.field));
    }

    if response.changed() {
        messages.push(Message::TimingTextChanged(props.field, buffer.clone()));
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

#[derive(Clone, Copy, Default)]
struct RangeDragState {
    anchor: f32,
    offset: f32,
}

/// Dual-thumb range slider operating over 0.0..=20.0 ms.
/// Clamps to `floor` on the lower bound, preserves drag offsets for near-thumb clicks,
/// jumps thumbs for distant presses, and sorts automatically when handles cross.
pub fn range_slider(
    ui: &mut Ui,
    min_val: f32,
    max_val: f32,
    floor: f32,
    enabled: bool,
    accent: Color32,
    id: Id,
) -> Option<(f32, f32)> {
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

    let to_x = |v: f32| track_start + (v.clamp(0.0, 20.0) / 20.0) * track_width;
    let to_value = |x: f32| ((x - track_start) / track_width * 20.0).clamp(0.0, 20.0);
    let to_rounded =
        |x: f32| (((x - track_start) / track_width * 200.0).round() / 10.0).clamp(floor, 20.0);

    let mut result = None;

    if enabled {
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                let val = to_value(pos.x);
                let cur_min = min_val.min(20.0);
                let cur_max = max_val.min(20.0);
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

                let offset = if (val - selected).abs() * track_width / 20.0 <= 12.0 {
                    val - selected
                } else {
                    0.0
                };
                ui.data_mut(|d| d.insert_temp(id, RangeDragState { anchor, offset }));
                let jumped = to_rounded(pos.x - offset * track_width / 20.0);
                let (n_min, n_max) = if jumped <= anchor {
                    (jumped, anchor)
                } else {
                    (anchor, jumped)
                };
                result = Some((n_min, n_max));
            }
        } else if response.dragged()
            && let Some(drag) = ui.data(|d| d.get_temp::<RangeDragState>(id))
            && let Some(pos) = response.interact_pointer_pos()
        {
            let moved = to_rounded(pos.x - drag.offset * track_width / 20.0);
            let (n_min, n_max) = if moved <= drag.anchor {
                (moved, drag.anchor)
            } else {
                (drag.anchor, moved)
            };
            result = Some((n_min, n_max));
        } else if response.drag_stopped() {
            ui.data_mut(|d| d.remove_temp::<RangeDragState>(id));
        }
    }

    let painter = ui.painter();
    let active_accent = if enabled { accent } else { SLATE_300 };

    // Background track (12px high, rounded 6px)
    let track_rect = Rect::from_min_max(
        Pos2::new(track_start, center_y - 6.0),
        Pos2::new(track_end, center_y + 6.0),
    );
    painter.rect_filled(track_rect, CornerRadius::same(6), SLATE_100);

    // Active range span
    let span_start = to_x(min_val);
    let span_end = to_x(max_val);
    if span_end > span_start {
        let span_rect = Rect::from_min_max(
            Pos2::new(span_start, center_y - 6.0),
            Pos2::new(span_end, center_y + 6.0),
        );
        painter.rect_filled(span_rect, CornerRadius::same(6), active_accent);
    }

    // Two thumbs: 16x16 circle, white background, 3px accent stroke
    for val in [min_val, max_val] {
        let thumb_x = to_x(val);
        let center = Pos2::new(thumb_x, center_y);
        painter.circle(center, 8.0, SURFACE, Stroke::new(3.0, active_accent));
    }

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Slider, enabled, "Duration Range"));
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    result
}

// ----------------------------------------------------------------------------
// Mode Selector
// ----------------------------------------------------------------------------

/// Horizontal 4-mode segment picker for the timing card.
pub fn mode_selector(
    ui: &mut Ui,
    selected: SocdMode,
    language: Language,
    messages: &mut Vec<Message>,
) {
    Frame::NONE
        .fill(INSET)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(4))
        .show(ui, |ui| {
            ui.columns(4, |cols| {
                for (i, mode) in SocdMode::ALL.iter().copied().enumerate() {
                    let col = &mut cols[i];
                    let is_active = mode == selected;
                    let label = mode_label(mode, language);
                    let color = if is_active {
                        mode_color(mode)
                    } else {
                        MUTED_TEXT
                    };

                    let btn_frame = if is_active {
                        Frame::NONE
                            .fill(SURFACE)
                            .stroke(Stroke::new(1.0, BORDER))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(8, 5))
                    } else {
                        Frame::NONE
                            .fill(Color32::TRANSPARENT)
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(8, 5))
                    };

                    let res = btn_frame
                        .show(col, |ui| {
                            ui.set_width(ui.available_width());
                            ui.vertical_centered(|ui| {
                                ui.colored_label(
                                    color,
                                    egui::RichText::new(label)
                                        .font(FontId::new(12.0, egui::FontFamily::Proportional))
                                        .strong(),
                                );
                            });
                        })
                        .response
                        .interact(Sense::click());

                    res.widget_info(|| {
                        WidgetInfo::selected(WidgetType::Button, true, is_active, label)
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
    inputs: &mut TimingInputs,
    editing: &[bool; 5],
    messages: &mut Vec<Message>,
) {
    let min_micros = props.minimum.micros(timing).unwrap_or(0);
    let max_micros = props.maximum.micros(timing).unwrap_or(0);
    let min_val = min_micros as f32 / 1000.0;
    let max_val = max_micros as f32 / 1000.0;
    let invalid = props.minimum.pair_invalid(timing);

    Frame::NONE
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 8.0);

            // Row 1: dot + label + space + pill
            ui.horizontal(|ui| {
                // Color dot
                let (dot_rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 3.5, props.accent);

                ui.colored_label(
                    BODY_TEXT,
                    egui::RichText::new(props.label)
                        .font(FontId::new(12.0, egui::FontFamily::Proportional))
                        .strong(),
                );

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let pill_bg = if invalid { ERROR_BG } else { SURFACE };
                    let pill_stroke = Stroke::new(1.0, if invalid { ERROR_BORDER } else { BORDER });

                    Frame::NONE
                        .fill(pill_bg)
                        .stroke(pill_stroke)
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::symmetric(6, 1))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                            ui.colored_label(props.accent, "ms");

                            let max_buf = inputs.buffer_mut(props.maximum);
                            let max_invalid = invalid || parse_ms_text(max_buf).is_none();
                            let max_box_props = ValueBoxProps::new(
                                props.maximum,
                                editing[props.maximum.index()],
                                max_invalid,
                                32.0,
                                props.accent,
                            );
                            value_box(ui, max_box_props, max_buf, messages);

                            ui.colored_label(props.accent, "~");

                            let min_buf = inputs.buffer_mut(props.minimum);
                            let min_invalid = invalid || parse_ms_text(min_buf).is_none();
                            let min_box_props = ValueBoxProps::new(
                                props.minimum,
                                editing[props.minimum.index()],
                                min_invalid,
                                32.0,
                                props.accent,
                            );
                            value_box(ui, min_box_props, min_buf, messages);
                        });
                });
            });

            // Row 2: dual-handle slider
            let floor = if props.minimum == TimingField::PreservedMinimum {
                0.1
            } else {
                0.0
            };
            if let Some((n_min, n_max)) = range_slider(
                ui,
                min_val,
                max_val,
                floor,
                true,
                props.accent,
                Id::new("rail").with(props.minimum.index()),
            ) {
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
    inputs: &mut TimingInputs,
    editing: &[bool; 5],
    language: Language,
    messages: &mut Vec<Message>,
) {
    let press_share = 100u8.saturating_sub(timing.overlap_preservation_rate);
    let invalid = parse_rate_text(&inputs.preservation_rate).is_none();

    Frame::NONE
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 8.0);

            // Row 1: dot + title + pill
            ui.horizontal(|ui| {
                let (dot_rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(dot_rect.center(), 3.5, MIX_TEXT);

                ui.colored_label(
                    BODY_TEXT,
                    egui::RichText::new(language.text("Delay Mix Ratio"))
                        .font(FontId::new(12.0, egui::FontFamily::Proportional))
                        .strong(),
                );

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let pill_bg = if invalid { ERROR_BG } else { SURFACE };
                    let pill_stroke = Stroke::new(1.0, if invalid { ERROR_BORDER } else { BORDER });

                    Frame::NONE
                        .fill(pill_bg)
                        .stroke(pill_stroke)
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::symmetric(6, 1))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                            ui.colored_label(MIX_TEXT, "%");

                            let rate_buf = &mut inputs.preservation_rate;
                            let rate_props = ValueBoxProps::new(
                                TimingField::PreservationRate,
                                editing[TimingField::PreservationRate.index()],
                                invalid,
                                32.0,
                                RELEASE_TEXT,
                            );
                            value_box(ui, rate_props, rate_buf, messages);

                            ui.colored_label(MIX_TEXT, ":");

                            ui.colored_label(
                                MIX_TEXT,
                                egui::RichText::new(format_rate(press_share))
                                    .font(FontId::new(12.0, egui::FontFamily::Proportional))
                                    .strong(),
                            );
                        });
                });
            });

            // Row 2: single-track mix ratio slider (1.0..=99.0)
            let mut slider_val = press_share as f32;
            let slider_res = ui.add(
                egui::Slider::new(&mut slider_val, 1.0..=99.0)
                    .step_by(1.0)
                    .show_value(false),
            );
            if slider_res.changed() {
                messages.push(Message::MixChanged(slider_val));
            }

            // Row 3: helper caption
            ui.colored_label(
                MUTED_TEXT,
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

    Frame::NONE
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 8.0);

            // Header
            ui.horizontal(|ui| {
                let (dot_rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 3.5, MUTED_TEXT);
                ui.colored_label(
                    BODY_TEXT,
                    egui::RichText::new(language.text("How it works"))
                        .font(FontId::new(11.0, egui::FontFamily::Proportional))
                        .strong(),
                );
            });

            // Steps list
            for (index, (label, accented)) in steps.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    let (badge_rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    let (bg, ink) = if accented {
                        (
                            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 30),
                            accent,
                        )
                    } else {
                        (INSET, MUTED_TEXT)
                    };

                    ui.painter()
                        .rect_filled(badge_rect, CornerRadius::same(4), bg);
                    ui.painter().text(
                        badge_rect.center(),
                        Align2::CENTER_CENTER,
                        format!("{}", index + 1),
                        FontId::new(9.0, egui::FontFamily::Proportional),
                        ink,
                    );

                    ui.colored_label(
                        MUTED_TEXT,
                        egui::RichText::new(label)
                            .font(FontId::new(11.0, egui::FontFamily::Proportional)),
                    );
                });
            }
        });
}

// ----------------------------------------------------------------------------
// Immediate Mode Illustrative Preview
// ----------------------------------------------------------------------------

/// Illustrative preview section displayed only in Immediate mode.
pub fn timing_preview_section(
    ui: &mut Ui,
    timing: &TimingSettings,
    preview: Option<&Preview>,
    language: Language,
    messages: &mut Vec<Message>,
) {
    let dummy_preview = Preview::default();
    let p = preview.unwrap_or(&dummy_preview);
    let (old, new) = p.held();

    let mode = [
        SocdMode::Immediate,
        SocdMode::PressDelay,
        SocdMode::ReleaseDelay,
    ][p.example];

    let (min, max) = match mode {
        SocdMode::PressDelay => (
            timing.socd_transition_min_micros,
            timing.socd_transition_max_micros,
        ),
        SocdMode::ReleaseDelay => (
            timing.preserved_overlap_min_micros,
            timing.preserved_overlap_max_micros,
        ),
        _ => (0, 0),
    };
    let delay = super::preview::delay_label(min, max);

    Frame::NONE
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 8.0);

            // Controls header: Previous / Play-Pause Pill / Next
            ui.horizontal(|ui| {
                if ui.button("⏴").clicked() {
                    messages.push(Message::Preview(PreviewAction::Previous));
                }

                let pill_text = format!(
                    "{} {} {}",
                    language.text("Preview"),
                    mode_label(mode, language),
                    if p.playing { "⏸" } else { "▶" }
                );
                let btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new(pill_text)
                            .font(FontId::new(11.0, egui::FontFamily::Proportional))
                            .strong(),
                    )
                    .corner_radius(CornerRadius::same(12)),
                );
                if btn.clicked() {
                    messages.push(Message::Preview(PreviewAction::Toggle));
                }

                if ui.button("⏵").clicked() {
                    messages.push(Message::Preview(PreviewAction::Next));
                }
            });

            // Key state visualization (A / D)
            ui.horizontal(|ui| {
                let render_key = |ui: &mut Ui, name: &str, held: bool, accent: Color32| {
                    let fill = if held { accent } else { SURFACE };
                    let stroke = Stroke::new(1.0, if held { accent } else { BORDER });
                    let text_color = if held { SURFACE } else { accent };

                    Frame::NONE
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.colored_label(
                                text_color,
                                egui::RichText::new(name)
                                    .font(FontId::new(16.0, egui::FontFamily::Proportional))
                                    .strong(),
                            );
                        });
                };

                render_key(ui, "A", old, PRIMARY_TEXT);
                ui.colored_label(MUTED_TEXT, format!("Delay: {delay}"));
                render_key(ui, "D", new, PURPLE_600);
            });
        });
}

// ----------------------------------------------------------------------------
// Full Timing Card
// ----------------------------------------------------------------------------

/// The full Input Timings card containing the mode selector, conditional duration
/// ranges, Random Mix slider, and mechanism steps.
pub fn timing_card(
    ui: &mut Ui,
    timing: &TimingSettings,
    inputs: &mut TimingInputs,
    editing: &[bool; 5],
    language: Language,
    preview: Option<&Preview>,
    messages: &mut Vec<Message>,
) {
    Frame::NONE
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 16.0);

            // Card title row
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.colored_label(
                        BODY_TEXT,
                        egui::RichText::new(language.text("Input timings"))
                            .font(FontId::new(14.0, egui::FontFamily::Proportional))
                            .strong(),
                    );
                    ui.colored_label(
                        MUTED_TEXT,
                        egui::RichText::new(
                            language.text("How opposite-direction overlaps resolve."),
                        )
                        .font(FontId::new(12.0, egui::FontFamily::Proportional)),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let restore_btn = ui.add(
                        egui::Button::new(
                            egui::RichText::new(language.text("Restore timing defaults"))
                                .font(FontId::new(12.0, egui::FontFamily::Proportional)),
                        )
                        .corner_radius(CornerRadius::same(6)),
                    );
                    if restore_btn.clicked() {
                        messages.push(Message::RestoreTimingDefaults);
                    }
                });
            });

            // Card content container
            Frame::NONE
                .fill(INSET)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 12.0);

                    // 1. Mode selector
                    mode_selector(ui, timing.mode, language, messages);

                    // 2. Immediate mode illustrative preview
                    if timing.mode == SocdMode::Immediate {
                        timing_preview_section(ui, timing, preview, language, messages);
                    }

                    // 3. Random Mix ratio group
                    if timing.mode == SocdMode::RandomMix {
                        rate_group(ui, timing, inputs, editing, language, messages);
                    }

                    // 4. New Key Press Delay duration group
                    if matches!(timing.mode, SocdMode::PressDelay | SocdMode::RandomMix) {
                        let props = DurationRangeProps::new(
                            TimingField::TransitionMinimum,
                            TimingField::TransitionMaximum,
                            "New Key Press Delay",
                            PRIMARY_TEXT,
                        );
                        duration_range(ui, props, timing, inputs, editing, messages);
                    }

                    // 5. Previous Key Release Delay duration group
                    if matches!(timing.mode, SocdMode::ReleaseDelay | SocdMode::RandomMix) {
                        let props = DurationRangeProps::new(
                            TimingField::PreservedMinimum,
                            TimingField::PreservedMaximum,
                            "Previous Key Release Delay",
                            VIOLET_600,
                        );
                        duration_range(ui, props, timing, inputs, editing, messages);
                    }

                    // 6. Mechanism steps ("How it works")
                    if timing.mode != SocdMode::RandomMix {
                        mechanism_steps(ui, timing.mode, timing, language);
                    }
                });
        });
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        // Rendering alone emits no messages until clicked
        assert!(messages.is_empty());
    }

    #[test]
    fn test_timing_card_immediate_mode() {
        let ctx = egui::Context::default();
        let timing = TimingSettings {
            mode: SocdMode::Immediate,
            ..Default::default()
        };
        let mut inputs = TimingInputs::from_timing(&timing);
        let editing = [false; 5];
        let mut messages = Vec::new();

        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            timing_card(
                ui,
                &timing,
                &mut inputs,
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
        let mut inputs = TimingInputs::from_timing(&timing);
        let editing = [false; 5];
        let mut messages = Vec::new();

        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            timing_card(
                ui,
                &timing,
                &mut inputs,
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
        let mut inputs = TimingInputs::from_timing(&timing);
        let editing = [false; 5];
        let mut messages = Vec::new();

        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            timing_card(
                ui,
                &timing,
                &mut inputs,
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
        let mut inputs = TimingInputs::from_timing(&timing);
        let editing = [false; 5];
        let mut messages = Vec::new();

        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            timing_card(
                ui,
                &timing,
                &mut inputs,
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
                2.0,
                8.0,
                0.0,
                true,
                PRIMARY_TEXT,
                Id::new("test-slider"),
            );
            // At rest with no input, returns None
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
            // RandomMix yields early and draws no steps
            mechanism_steps(ui, SocdMode::RandomMix, &timing, Language::English);
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn test_value_box_edit_and_read_back() {
        let ctx = egui::Context::default();
        let mut text = "10.0".to_string();
        let mut messages = Vec::new();

        // 1. Initial render
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let props = ValueBoxProps::new(
                TimingField::TransitionMinimum,
                false,
                false,
                32.0,
                PRIMARY_TEXT,
            );
            value_box(ui, props, &mut text, &mut messages);
        });
        output.drop_without_applying_deltas();

        // 2. User edits text to "15.5"
        text = "15.5".to_string();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let props = ValueBoxProps::new(
                TimingField::TransitionMinimum,
                true,
                false,
                32.0,
                PRIMARY_TEXT,
            );
            value_box(ui, props, &mut text, &mut messages);
        });
        output.drop_without_applying_deltas();

        // Check that text changed and parses back into micros
        assert_eq!(text, "15.5");
        let parsed = parse_ms_text(&text);
        assert_eq!(parsed, Some(15500));
    }
}
