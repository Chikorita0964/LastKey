//! Timing card, sliders, and value boxes for LastKey settings.
//!
//! Port of the four SOCD modes (`Immediate`, `PressDelay`, `ReleaseDelay`, `RandomMix`),
//! the mode-conditional timing groups with matched card heights, the two-row timing
//! pills, the dual-handle range slider, and the value boxes with select-all-on-focus
//! behavior.

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Id, Margin, Pos2, Rect, Response, Sense,
    Stroke, TextEdit, Ui, WidgetInfo, WidgetType, text::CCursor, vec2,
};

use crate::settings::{SocdMode, TimingSettings};

use super::{
    language::Language,
    message::{Message, TimingField},
    preview::{self, Preview},
    state::{TimingInputs, parse_ms_text, parse_press_rate_text, parse_rate_text},
    theme::{
        self, BODY_TEXT, CARD_PADDING, ERROR_TEXT, GROUP_PADDING, HEADING_SIZE, IMMEDIATE_ACCENT,
        INDIGO_600, INSET, MIX_TEXT, MUTED_TEXT, PRIMARY_TEXT, RELEASE_TEXT, SECTION_GAP,
        SLATE_100, SLIDER_HANDLE_BORDER, SLIDER_RAIL_DISABLED, SURFACE, VIOLET_600,
    },
};

// ----------------------------------------------------------------------------
// Mode Metadata
// ----------------------------------------------------------------------------

/// Geometry shared by every slider in the timing card (F08): a 12px rail
/// rounded to 6 -- the reference's `h-3` + `rounded-full` -- and a constant
/// 16px (radius 8) thumb, the reference's `w-4 h-4`, whose size does not
/// depend on the status. The theme's single-handle `SLIDER_RAIL_*` /
/// `SLIDER_HANDLE_RADIUS*` tokens are no longer consumed by this card; the
/// Random Mix mixer takes this geometry too instead of the Iced
/// `accent_slider` one.
const RAIL_WIDTH: f32 = 12.0;
const RAIL_RADIUS: CornerRadius = CornerRadius::same(6);
const THUMB_RADIUS: f32 = 8.0;

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
pub fn value_box(
    ui: &mut Ui,
    props: ValueBoxProps,
    buffer: &str,
    messages: &mut Vec<Message>,
) -> Response {
    let id = value_box_id(props.field);
    let text_color = if props.invalid {
        ERROR_TEXT
    } else {
        props.accent
    };

    // Frame-local buffer so the view remains read-only with respect to TimingInputs.
    let mut local_text = buffer.to_owned();
    let response = ui.add(
        TextEdit::singleline(&mut local_text)
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

/// Dual-thumb range slider operating over 0.0..=20.0 ms.
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

    let to_x = |v: f32| track_start + (v.clamp(0.0, 20.0) / 20.0) * track_width;
    let to_value = |x: f32| ((x - track_start) / track_width * 20.0).clamp(0.0, 20.0);
    let to_rounded = |x: f32| {
        (((x - track_start) / track_width * 200.0).round() / 10.0).clamp(props.floor, 20.0)
    };

    let mut result = None;

    if props.enabled {
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                let val = to_value(pos.x);
                let cur_min = props.min_val.min(20.0);
                let cur_max = props.max_val.min(20.0);
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
                ui.data_mut(|d| d.insert_temp(props.id, RangeDragState { anchor, offset }));
                let jumped = to_rounded(pos.x - offset * track_width / 20.0);
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
            let moved = to_rounded(pos.x - drag.offset * track_width / 20.0);
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
        SLIDER_RAIL_DISABLED
    };
    let rail_half = RAIL_WIDTH / 2.0;

    // Background track: SLATE_100, the card's shared 12px rail rounded to 6.
    let track_rect = Rect::from_min_max(
        Pos2::new(track_start, center_y - rail_half),
        Pos2::new(track_end, center_y + rail_half),
    );
    painter.rect_filled(track_rect, RAIL_RADIUS, SLATE_100);

    // Active range span
    let span_start = to_x(props.min_val);
    let span_end = to_x(props.max_val);
    if span_end > span_start {
        let span_rect = Rect::from_min_max(
            Pos2::new(span_start, center_y - rail_half),
            Pos2::new(span_end, center_y + rail_half),
        );
        painter.rect_filled(span_rect, RAIL_RADIUS, active_accent);
    }

    // Two thumbs: white background, 3px accent stroke, the shared constant
    // 16px size (the reference thumb has no status-dependent size)
    for val in [props.min_val, props.max_val] {
        let thumb_x = to_x(val);
        let center = Pos2::new(thumb_x, center_y);
        painter.circle(
            center,
            THUMB_RADIUS,
            SURFACE,
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
/// a constant 16px (radius 8) handle ringed in MIX_TEXT, the same geometry the
/// duration rail uses (F08; the reference draws both with an `h-3` rail and a
/// `w-4 h-4` thumb). The rail is filled `PRIMARY_TEXT` left of the handle and
/// `RELEASE_TEXT` right of it, the split the Iced `theme::mixer_slider` sets
/// by overriding `accent_slider`'s second rail background
/// (`iced-ui/theme.rs:1116-1121`, iced `widget/src/slider.rs:455-481`).
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

    // Rail split at the handle: PRIMARY_TEXT to the left, RELEASE_TEXT to the
    // right, both on the shared 12px rounded-6 rail.
    let handle_x = to_x(press_share);
    for (start, end, fill) in [
        (track_start, handle_x, PRIMARY_TEXT),
        (handle_x, track_end, RELEASE_TEXT),
    ] {
        let (start, end) = (start.min(end), start.max(end));
        if end > start {
            let span_rect = Rect::from_min_max(
                Pos2::new(start, center_y - rail_half),
                Pos2::new(end, center_y + rail_half),
            );
            painter.rect_filled(span_rect, RAIL_RADIUS, fill);
        }
    }

    // Handle thumb: white fill, 3.0 border in MIX_TEXT, the shared constant
    // 16px size that hover and drag do not change (the reference thumb is
    // status-independent).
    painter.circle(
        Pos2::new(handle_x, center_y),
        THUMB_RADIUS,
        SURFACE,
        Stroke::new(SLIDER_HANDLE_BORDER, MIX_TEXT),
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

/// Horizontal 4-mode segment picker for the timing card.
///
/// The strip insets its segments by 4 (`iced-ui/app.rs:3123`,
/// `container(segments).padding(4)`), and each segment is padded by
/// [`theme::MODE_PADDING`] (`iced-ui/app.rs:3117`). The selected segment is a
/// solid mode-accent chip with white bold text (F07); the others stay
/// transparent with muted ink.
pub fn mode_selector(
    ui: &mut Ui,
    selected: SocdMode,
    language: Language,
    messages: &mut Vec<Message>,
) {
    theme::group_style()
        .inner_margin(Margin::same(4))
        .show(ui, |ui| {
            ui.columns(4, |cols| {
                for (i, mode) in SocdMode::ALL.iter().copied().enumerate() {
                    let col = &mut cols[i];
                    let is_active = mode == selected;
                    let label = mode_label(mode, language);
                    let ink = if is_active {
                        Color32::WHITE
                    } else {
                        MUTED_TEXT
                    };

                    let btn_frame = if is_active {
                        Frame::NONE
                            .fill(mode_color(mode))
                            .corner_radius(theme::CHIP_RADIUS)
                            .inner_margin(theme::MODE_PADDING)
                    } else {
                        Frame::NONE
                            .fill(Color32::TRANSPARENT)
                            .corner_radius(theme::CHIP_RADIUS)
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
                // Color dot
                let (dot_rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot_rect.center(), 3.5, props.accent);

                ui.colored_label(
                    BODY_TEXT,
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

    // The Iced ratio group is the one slot in this card padded with
    // `theme::GROUP_PADDING` rather than the 12 the duration ranges use
    // (`iced-ui/app.rs:3180`).
    theme::slot_style()
        .inner_margin(Margin::same(GROUP_PADDING as i8))
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
                    theme::pill_style(invalid)
                        .inner_margin(Margin::symmetric(6, 1))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                            ui.colored_label(MIX_TEXT, "%");

                            // R1 issue 10: RELEASE_TEXT is intentional for the preservation rate box
                            // per iced-ui/app.rs:3317 (matching the release-delay accent).
                            let rate_props = ValueBoxProps::new(
                                TimingField::PreservationRate,
                                editing[TimingField::PreservationRate.index()],
                                rate_invalid,
                                32.0,
                                RELEASE_TEXT,
                            );
                            value_box(ui, rate_props, rate_str, messages);

                            ui.colored_label(MIX_TEXT, ":");

                            // F10: the press share is the mirror box, so it takes the
                            // mixer's left-rail ink (drawn `#4f46e5`) where the release
                            // box takes its right-rail ink.
                            let press_props = ValueBoxProps::new(
                                TimingField::PressRate,
                                editing[TimingField::PressRate.index()],
                                press_invalid,
                                32.0,
                                PRIMARY_TEXT,
                            );
                            value_box(ui, press_props, press_str, messages);
                        });
                });
            });

            // Row 2: custom mixer rail (PRIMARY_TEXT left of the handle,
            // RELEASE_TEXT right of it, MIX_TEXT handle ring)
            if let Some(new_share) = mixer_slider(ui, press_share as f32, true, language) {
                messages.push(Message::MixChanged(new_share));
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

    theme::slot_style()
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
                        .rect_filled(badge_rect, theme::BADGE_RADIUS, bg);
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
/// carries the procedural transport glyphs, the 850 ms phase clock, and the
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

            // Card title row
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.colored_label(
                        BODY_TEXT,
                        egui::RichText::new(language.text("Input timings"))
                            .font(FontId::new(HEADING_SIZE, egui::FontFamily::Proportional))
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

                    // 2. Immediate mode illustrative preview (T5's card)
                    if timing.mode == SocdMode::Immediate
                        && let Some(mount) = preview
                    {
                        preview::preview_card(
                            ui,
                            timing,
                            mount.preview,
                            language,
                            mount.awake,
                            mount.clock_mounted,
                            messages,
                        );
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
                        duration_range(ui, props, timing, inputs, editing, language, messages);
                    }

                    // 5. Previous Key Release Delay duration group
                    if matches!(timing.mode, SocdMode::ReleaseDelay | SocdMode::RandomMix) {
                        let props = DurationRangeProps::new(
                            TimingField::PreservedMinimum,
                            TimingField::PreservedMaximum,
                            "Previous Key Release Delay",
                            VIOLET_600,
                        );
                        duration_range(ui, props, timing, inputs, editing, language, messages);
                    }

                    // 6. Mechanism steps ("How it works")
                    if timing.mode != SocdMode::RandomMix {
                        mechanism_steps(ui, timing.mode, timing, language);
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
                    accent: PRIMARY_TEXT,
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

    /// R2 round-3: the strip insets its segments by 4 and each segment is padded
    /// by `theme::MODE_PADDING`. Iced wraps the segments as
    /// `container(segments).padding(4)` (`iced-ui/app.rs:3123`) and pads each
    /// segment with `theme::MODE_PADDING` (`iced-ui/app.rs:3117`). Rendered rather
    /// than restated, so a flattened margin fails here.
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

        // The strip is the only `group_style` frame in this render: inset fill,
        // 1px border, `GROUP_RADIUS` corners.
        let strip = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.corner_radius == theme::GROUP_RADIUS && rect.fill == theme::INSET)
            .expect("the mode strip paints its group frame");

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

            // MODE_PADDING contributes 7px vertically over a 14px label, so a
            // segment is at least the reference's 28px. The literal
            // `symmetric(8, 5)` this replaces measured 24. The bound rather than
            // an equality keeps this about the padding.
            let height = segment.height();
            assert!(
                height >= 28.0,
                "{label}: segment height must follow theme::MODE_PADDING (measured {height})"
            );
        }
    }

    /// F08: both sliders in the timing card share one geometry -- a 12px rail
    /// rounded to 6 and constant 16px (radius 8) thumbs -- and the mixer
    /// handle keeps its size under hover and drag, as the reference does.
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
                        accent: PRIMARY_TEXT,
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
        let duration_thumbs = painted_circle_radii(&duration, PRIMARY_TEXT);

        // The Random Mix rail's geometry.
        let mut mixer = Harness::builder()
            .with_size(egui::vec2(600.0, 200.0))
            .build_ui(|ui| {
                let _ = mixer_slider(ui, 50.0, true, Language::English);
            });
        mixer.run();
        let span = painted_rects(&mixer)
            .into_iter()
            .find(|rect| rect.fill == PRIMARY_TEXT)
            .expect("the mixer paints its left span");
        let mixer_rail = (span.rect.height(), span.corner_radius);
        let mixer_handles = painted_circle_radii(&mixer, MIX_TEXT);

        assert_eq!(
            duration_rail,
            (12.0, CornerRadius::same(6)),
            "the duration rail keeps the reference's 12px rounded-6 track"
        );
        assert_eq!(
            mixer_rail, duration_rail,
            "the Random Mix rail must share the duration rail's dimensions"
        );
        assert_eq!(
            duration_thumbs,
            vec![8.0, 8.0],
            "both duration thumbs keep the constant 16px diameter"
        );
        assert_eq!(
            mixer_handles,
            vec![8.0],
            "the mixer handle must share the 16px diameter"
        );

        // The reference handle has no status-dependent size: hovering and
        // dragging the mixer must leave it at 16px.
        let slider = mixer.get_by_role_and_label(egui::accesskit::Role::Slider, "Delay Mix Ratio");
        let rect = slider.rect();
        slider.hover();
        mixer.run();
        assert_eq!(
            painted_circle_radii(&mixer, MIX_TEXT),
            vec![8.0],
            "hovering must not resize the mixer handle"
        );
        mixer.drag_at(rect.center());
        mixer.run();
        assert_eq!(
            painted_circle_radii(&mixer, MIX_TEXT),
            vec![8.0],
            "dragging must not resize the mixer handle"
        );
        mixer.drop_at(rect.center());
    }

    /// F07: the active mode segment takes a solid mode-accent fill and white
    /// bold text; every other segment stays transparent with muted ink. The
    /// old white surface chip with a hairline border must not survive.
    #[test]
    fn test_mode_selector_active_segment_takes_the_mode_accent() {
        use egui_kittest::Harness;

        for mode in SocdMode::ALL {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(600.0, 200.0))
                .build_ui(move |ui| {
                    let mut messages = Vec::new();
                    mode_selector(ui, mode, Language::English, &mut messages);
                });
            harness.run();

            let chip = painted_rects(&harness)
                .into_iter()
                .find(|rect| rect.fill == mode_color(mode))
                .unwrap_or_else(|| {
                    panic!("{mode:?}: the active segment must paint a solid accent chip")
                });
            assert!(
                chip.rect.height() >= 28.0,
                "{mode:?}: the accent chip must keep the segment's padded height"
            );

            let active = mode_label(mode, Language::English);
            assert_eq!(
                painted_text_color(&harness, active),
                Some(Color32::WHITE),
                "{active}: the active segment's label must render white"
            );
            for other in SocdMode::ALL {
                if other == mode {
                    continue;
                }
                let label = mode_label(other, Language::English);
                assert_eq!(
                    painted_text_color(&harness, label),
                    Some(MUTED_TEXT),
                    "{label}: inactive segments keep the muted ink"
                );
            }
        }
    }

    /// R2 round-3: the Iced `mixer_slider` fills the rail `PRIMARY_TEXT` left of
    /// the handle and `RELEASE_TEXT` right of it (`iced-ui/theme.rs:1116-1121`).
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
            .filter(|rect| rect.corner_radius == RAIL_RADIUS)
            .collect();
        assert_eq!(rail.len(), 2, "the rail paints two spans: {rail:?}");

        let left = rail
            .iter()
            .find(|rect| rect.fill == PRIMARY_TEXT)
            .expect("the span left of the handle is PRIMARY_TEXT");
        let right = rail
            .iter()
            .find(|rect| rect.fill == RELEASE_TEXT)
            .expect("the span right of the handle is RELEASE_TEXT");

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

    /// R2 round-3: the duration rail is the Iced `widgets::RangeSlider`, not the
    /// single-handle `accent_slider`: a 12px rail rounded to 6 with constant
    /// 16px thumbs (radius 8) that do not change with the status
    /// (`iced-ui/widgets.rs:238-285`). F08 unified the mixer onto this same
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
                        accent: PRIMARY_TEXT,
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
            track.corner_radius, RAIL_RADIUS,
            "the duration rail is rounded to 6, not the retired accent_slider 5"
        );

        assert_eq!(
            painted_circle_radii(&harness, PRIMARY_TEXT),
            vec![THUMB_RADIUS, THUMB_RADIUS],
            "both thumbs keep the constant 16px size"
        );

        // The Iced widget has no status-dependent thumb size, so a drag must
        // leave the geometry alone.
        harness.drag_at(egui::pos2(300.0, 100.0));
        harness.run();
        harness.hover_at(egui::pos2(340.0, 100.0));
        harness.run();
        assert_eq!(
            painted_circle_radii(&harness, PRIMARY_TEXT),
            vec![THUMB_RADIUS, THUMB_RADIUS],
            "dragging must not resize the two-handle thumbs"
        );
        harness.drop_at(egui::pos2(340.0, 100.0));
    }

    /// R2 round-3: the Random Mix slot is padded with `theme::GROUP_PADDING` (14),
    /// not the 12 the duration groups use (`iced-ui/app.rs:3180`).
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
        // both the SURFACE fill and the GROUP_RADIUS corners.
        let rail = harness
            .get_by_role_and_label(egui::accesskit::Role::Slider, "Delay Mix Ratio")
            .rect();
        let slot = painted_rects(&harness)
            .into_iter()
            .find(|rect| {
                rect.corner_radius == theme::GROUP_RADIUS
                    && rect.fill == SURFACE
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
        assert_eq!(timing_rest, Some(theme::ICON_SECONDARY));
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
                rect.corner_radius == RAIL_RADIUS && slider_rect.contains_rect(rect.rect)
            })
            .collect();
        let press_span = spans
            .iter()
            .find(|rect| rect.fill == PRIMARY_TEXT)
            .expect("the mixer paints its press share span");
        let release_span = spans
            .iter()
            .find(|rect| rect.fill == RELEASE_TEXT)
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
}
