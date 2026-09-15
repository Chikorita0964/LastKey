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
    message::{Message, PreviewAction, TimingField},
    preview::Preview,
    state::{TimingInputs, format_rate, parse_ms_text, parse_rate_text},
    theme::{
        self, BODY_TEXT, BORDER, ERROR_TEXT, HEADING_SIZE, IMMEDIATE_ACCENT, INDIGO_600, INSET,
        MIX_TEXT, MUTED_TEXT, PRIMARY_TEXT, PURPLE_600, RELEASE_TEXT, SECTION_GAP, SLATE_100,
        SLIDER_HANDLE_BORDER, SLIDER_HANDLE_RADIUS, SLIDER_HANDLE_RADIUS_DRAG,
        SLIDER_RAIL_DISABLED, SLIDER_RAIL_RADIUS, SLIDER_RAIL_WIDTH, SURFACE, VIOLET_600,
    },
};

// ----------------------------------------------------------------------------
// Mode Metadata
// ----------------------------------------------------------------------------

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
        TimingField::PreservationRate => "Delay Mix Ratio",
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
    let rail_half = SLIDER_RAIL_WIDTH / 2.0;

    // Background track (SLATE_100, 10px high, rounded 5px per theme::SLIDER_RAIL_*)
    let track_rect = Rect::from_min_max(
        Pos2::new(track_start, center_y - rail_half),
        Pos2::new(track_end, center_y + rail_half),
    );
    painter.rect_filled(track_rect, SLIDER_RAIL_RADIUS, SLATE_100);

    // Active range span
    let span_start = to_x(props.min_val);
    let span_end = to_x(props.max_val);
    if span_end > span_start {
        let span_rect = Rect::from_min_max(
            Pos2::new(span_start, center_y - rail_half),
            Pos2::new(span_end, center_y + rail_half),
        );
        painter.rect_filled(span_rect, SLIDER_RAIL_RADIUS, active_accent);
    }

    // Two thumbs: white background, 3px accent stroke
    let thumb_radius = if response.dragged() {
        SLIDER_HANDLE_RADIUS_DRAG
    } else {
        SLIDER_HANDLE_RADIUS
    };
    for val in [props.min_val, props.max_val] {
        let thumb_x = to_x(val);
        let center = Pos2::new(thumb_x, center_y);
        painter.circle(
            center,
            thumb_radius,
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

/// Random Mix ratio rail slider: 10pt rail rounded to 5, hollow handle that swells
/// from 7 to 8 while grabbed, ringed in MIX_TEXT with a RELEASE_TEXT filled rail.
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
    let rail_half = SLIDER_RAIL_WIDTH / 2.0;

    // Background track (SLATE_100, 10px high, rounded 5px)
    let track_rect = Rect::from_min_max(
        Pos2::new(track_start, center_y - rail_half),
        Pos2::new(track_end, center_y + rail_half),
    );
    painter.rect_filled(track_rect, SLIDER_RAIL_RADIUS, SLATE_100);

    // Filled span from start to handle (RELEASE_TEXT, rounded 5px)
    let handle_x = to_x(press_share);
    if handle_x > track_start {
        let span_rect = Rect::from_min_max(
            Pos2::new(track_start, center_y - rail_half),
            Pos2::new(handle_x, center_y + rail_half),
        );
        painter.rect_filled(span_rect, SLIDER_RAIL_RADIUS, RELEASE_TEXT);
    }

    // Handle thumb: Circle at handle_x, center_y, radius 7.0 (8.0 while dragged/hovered),
    // white fill, 3.0 border in MIX_TEXT.
    let handle_radius = if response.dragged() || response.hovered() {
        SLIDER_HANDLE_RADIUS_DRAG
    } else {
        SLIDER_HANDLE_RADIUS
    };
    painter.circle(
        Pos2::new(handle_x, center_y),
        handle_radius,
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
pub fn mode_selector(
    ui: &mut Ui,
    selected: SocdMode,
    language: Language,
    messages: &mut Vec<Message>,
) {
    theme::group_style().show(ui, |ui| {
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
                        .corner_radius(theme::CHIP_RADIUS)
                        .inner_margin(Margin::symmetric(8, 5))
                } else {
                    Frame::NONE
                        .fill(Color32::TRANSPARENT)
                        .corner_radius(theme::CHIP_RADIUS)
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
    editing: &[bool; 5],
    language: Language,
    messages: &mut Vec<Message>,
) {
    let min_micros = props.minimum.micros(timing).unwrap_or(0);
    let max_micros = props.maximum.micros(timing).unwrap_or(0);
    let min_val = min_micros as f32 / 1000.0;
    let max_val = max_micros as f32 / 1000.0;
    let invalid = props.minimum.pair_invalid(timing);

    theme::slot_style().show(ui, |ui| {
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
                theme::pill_style(invalid).show(ui, |ui| {
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
    editing: &[bool; 5],
    language: Language,
    messages: &mut Vec<Message>,
) {
    let press_share = 100u8.saturating_sub(timing.overlap_preservation_rate);
    let rate_str = inputs.buffer(TimingField::PreservationRate);
    let invalid = parse_rate_text(rate_str).is_none();

    theme::slot_style().show(ui, |ui| {
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
                theme::pill_style(invalid).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                    ui.colored_label(MIX_TEXT, "%");

                    // R1 issue 10: RELEASE_TEXT is intentional for the preservation rate box
                    // per src/ui/app.rs:3317 (matching the release-delay accent).
                    let rate_props = ValueBoxProps::new(
                        TimingField::PreservationRate,
                        editing[TimingField::PreservationRate.index()],
                        invalid,
                        32.0,
                        RELEASE_TEXT,
                    );
                    value_box(ui, rate_props, rate_str, messages);

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

        // Row 2: custom mixer rail (RELEASE_TEXT filled rail, MIX_TEXT handle ring)
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

    theme::slot_style().show(ui, |ui| {
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
// Immediate Mode Illustrative Preview
// ----------------------------------------------------------------------------

/// Illustrative preview section displayed only in Immediate mode.
///
/// Note: Ownership of preview animation and interactive graph belongs to T5 per
/// the EGUI_MIGRATION.md work split. This section provides the Immediate mode mount
/// point until coordinated with T5's `preview::preview_card`.
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

    // R1 issue 6: Bounds check access to prevent panic on arbitrary example values
    let mode = [
        SocdMode::Immediate,
        SocdMode::PressDelay,
        SocdMode::ReleaseDelay,
    ]
    .get(p.example % 3)
    .copied()
    .unwrap_or(SocdMode::Immediate);

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

    theme::slot_style().show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 8.0);

        // Controls header: Previous / Play-Pause Pill / Next
        // With accessible labels for label-driven testing
        ui.horizontal(|ui| {
            let prev_btn = ui.button("⏴");
            prev_btn.widget_info(|| {
                WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), "Previous example")
            });
            if prev_btn.clicked() {
                messages.push(Message::Preview(PreviewAction::Previous));
            }

            let pill_text = format!(
                "{} {} {}",
                language.text("Preview"),
                mode_label(mode, language),
                if p.playing { "⏸" } else { "▶" }
            );
            let pill_btn = ui.add(
                egui::Button::new(
                    egui::RichText::new(pill_text)
                        .font(FontId::new(11.0, egui::FontFamily::Proportional))
                        .strong(),
                )
                .corner_radius(theme::CONTROL_RADIUS),
            );
            pill_btn.widget_info(|| {
                WidgetInfo::labeled(
                    WidgetType::Button,
                    ui.is_enabled(),
                    if p.playing {
                        "Pause preview"
                    } else {
                        "Play preview"
                    },
                )
            });
            if pill_btn.clicked() {
                messages.push(Message::Preview(PreviewAction::Toggle));
            }

            let next_btn = ui.button("⏵");
            next_btn.widget_info(|| {
                WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), "Next example")
            });
            if next_btn.clicked() {
                messages.push(Message::Preview(PreviewAction::Next));
            }

            // 3-example position dots
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                for i in 0..3 {
                    let dot_color = if (p.example % 3) == i {
                        mode_color(mode)
                    } else {
                        BORDER
                    };
                    theme::example_dot(dot_color).show(ui, |ui| {
                        ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
                    });
                }
            });
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
                    .corner_radius(theme::CHIP_RADIUS)
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

            // Phase 1 highlights delay range per ui.md line 143
            let delay_bg = if p.phase == 1 {
                theme::step_badge(Some(mode_color(mode)))
            } else {
                Frame::NONE.fill(INSET)
            };
            delay_bg.show(ui, |ui| {
                ui.colored_label(MUTED_TEXT, format!("Delay: {delay}"));
            });

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
    inputs: &TimingInputs,
    editing: &[bool; 5],
    language: Language,
    preview: Option<&Preview>,
    messages: &mut Vec<Message>,
) {
    theme::card_style().show(ui, |ui| {
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
                    egui::RichText::new(language.text("How opposite-direction overlaps resolve."))
                        .font(FontId::new(12.0, egui::FontFamily::Proportional)),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let restore_btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new(language.text("Restore timing defaults"))
                            .font(FontId::new(12.0, egui::FontFamily::Proportional)),
                    )
                    .corner_radius(theme::CHIP_RADIUS),
                );
                if restore_btn.clicked() {
                    messages.push(Message::RestoreTimingDefaults);
                }
            });
        });

        // Card content container
        theme::group_style().show(ui, |ui| {
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
    });
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{settings::Settings, ui2::state::State};

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
        let editing = [false; 5];
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
        let editing = [false; 5];
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
        let editing = [false; 5];
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
        let editing = [false; 5];
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
    }

    impl FakeRuntime {
        fn frame(&mut self, ui: &mut Ui) {
            let mut messages = Vec::new();
            if let Some(draft) = &self.state.draft {
                timing_card(
                    ui,
                    &draft.timing,
                    &self.state.inputs,
                    &self.state.editing,
                    self.state.language,
                    Some(&self.state.preview),
                    &mut messages,
                );
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
                FakeRuntime {
                    state: test_state(),
                    sent: Vec::new(),
                },
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
                FakeRuntime {
                    state: test_state(),
                    sent: Vec::new(),
                },
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
                FakeRuntime {
                    state: test_state(),
                    sent: Vec::new(),
                },
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
}
