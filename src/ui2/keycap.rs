//! The Key mappings D-pad keycap.
//!
//! The Iced original is `fn keycap` in `src/ui/app.rs`, which took its paint
//! from `src/ui/theme.rs` (`theme::keycap` / `theme::KeycapMode`). egui paints
//! the box, the glow, the legend, and both label lines directly, and the
//! interactive node comes from the response: a custom-drawn widget is
//! invisible to AccessKit, so `ui.interact` plus a labelled [`WidgetInfo`] is
//! what lets `egui_kittest` and the inspection tools find the keycap by name.
//!
//! Every colour and shadow comes from [`super::theme`]; this module keeps only
//! metrics that no theme owner defines.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontId, Galley, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2, WidgetInfo, WidgetType, epaint::Shadow,
};

use crate::protocol::KeySlot;

use super::{message::Message, theme};

/// The keycap's square box. The Iced button pinned `width(80).height(80)`,
/// and the reference sizes every D-pad cap the same way.
pub const KEYCAP_SIZE: f32 = 80.0;
/// The Iced button's inner padding, which is what positions both text rows.
const PADDING: f32 = 10.0;
/// `rounded-xl` on the Iced keycap; the reference caps the pad with a 12px
/// radius and fills the border inside those bounds.
const RADIUS: u8 = 12;
/// The sub-legend line: a 10px label at iced's 1.3 line height.
const LEGEND_SIZE: f32 = 10.0;
const LEGEND_HEIGHT: f32 = 13.0;
/// The Iced column's `spacing(4)` between the legend row and the key label.
const LEGEND_GAP: f32 = 4.0;
/// The legend's arrow glyph, as `icons::icon(arrow, 12.0, ...)` drew it.
const ARROW_SIZE: f32 = 12.0;
/// How far a glow or drop shadow reaches past the keycap's own box: the
/// widest `Shadow::blur` here is [`theme::KEYCAP_REBIND_GLOW_BLUR`], and
/// epaint's penumbra extends half the blur in each direction.
const SHADOW_REACH: f32 = 12.0;

/// One D-pad direction's display identity: its capture slot, sub-legend,
/// arrow, accent colour, and outer ring glow colour. Ported unchanged from the
/// Iced `Direction` constants; `docs/architecture/ui.md` fixes the mapping
/// (UP = Immediate, LEFT = Press Delay, RIGHT = Random Mix, DOWN = Release
/// Delay).
pub struct Direction {
    pub slot: KeySlot,
    pub label: &'static str,
    pub arrow: Arrow,
    pub accent: Color32,
    pub ring: Color32,
}

/// Which way a direction's legend arrow points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Arrow {
    Up,
    Down,
    Left,
    Right,
}

pub const UP: Direction = Direction {
    slot: KeySlot::VerticalFirst,
    label: "UP",
    arrow: Arrow::Up,
    accent: theme::IMMEDIATE_ACCENT,
    ring: Color32::from_rgb(0x93, 0xc5, 0xfd),
};
pub const DOWN: Direction = Direction {
    slot: KeySlot::VerticalSecond,
    label: "DOWN",
    arrow: Arrow::Down,
    accent: theme::VIOLET_600,
    ring: Color32::from_rgb(0xc4, 0xb5, 0xfd),
};
pub const LEFT: Direction = Direction {
    slot: KeySlot::HorizontalFirst,
    label: "LEFT",
    arrow: Arrow::Left,
    accent: theme::INDIGO_600,
    ring: Color32::from_rgb(0xa5, 0xb4, 0xfc),
};
pub const RIGHT: Direction = Direction {
    slot: KeySlot::HorizontalSecond,
    label: "RIGHT",
    arrow: Arrow::Right,
    accent: theme::MIX_TEXT,
    ring: Color32::from_rgb(0xd8, 0xb4, 0xfe),
};

/// The keycap's paint state. [`super::theme::KeycapMode`] is the one owner
/// (it came from the Iced `theme::keycap` style function); this re-export
/// keeps the widget's own surface stable for the card and its tests.
pub use super::theme::KeycapMode;

/// Everything one keycap needs to draw itself this frame.
#[derive(Clone, Copy, Debug)]
pub struct Keycap<'a> {
    /// The bound key's display name from the snapshot.
    pub name: &'a str,
    pub mode: KeycapMode,
    /// Whether another slot shares this key (`duplicate_slots`'s flag).
    pub duplicate: bool,
}

/// Draws one keycap and returns the message its press produced.
///
/// The keycap reserves its box and interacts with `Sense::click`, then
/// publishes an AccessKit node labelled by [`keycap_label`]. Clicking a
/// rebinding keycap cancels the capture; any other state starts one for its
/// slot, which is the Iced `on_press` mapping unchanged.
pub fn keycap(ui: &mut Ui, direction: &Direction, state: Keycap<'_>) -> Option<Message> {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(KEYCAP_SIZE), Sense::click());
    let label = keycap_label(direction, state.mode, state.name);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), &label));

    paint(
        ui,
        rect,
        direction,
        &state,
        response.hovered(),
        response.is_pointer_button_down_on(),
    );

    if response.clicked() {
        Some(match state.mode {
            KeycapMode::Rebinding => Message::CancelCapture,
            _ => Message::Capture(direction.slot),
        })
    } else {
        None
    }
}

/// The AccessKit label for a keycap. The direction keeps the four labels
/// unique, and the rebinding state replaces the key name, so a semantic test
/// can observe the capture transition through labels alone.
pub fn keycap_label(direction: &Direction, mode: KeycapMode, name: &str) -> String {
    match mode {
        KeycapMode::Rebinding => format!("{} keycap: rebinding", direction.label),
        KeycapMode::Pressed => format!("{} keycap: {name} (pressed)", direction.label),
        KeycapMode::Normal => format!("{} keycap: {name}", direction.label),
    }
}

/// How a key name is drawn in the keycap: its lines and the size both take.
/// Ported unchanged from the Iced `format_key_for_display`, except that the
/// font weight is gone with egui's single-weight bundled faces.
pub struct KeyDisplay {
    pub lines: Vec<String>,
    pub size: f32,
}

/// Splits a key name into at most two display lines and picks the size the
/// longest line takes. Compound names (`Numpad 8`, `Arrow Up`, `Backspace`)
/// are the case this exists for.
///
/// Lengths are counted in characters, never bytes: a name may carry non-ASCII
/// text, and slicing a UTF-8 string at a byte offset that falls inside a
/// character panics. The `starts_with` guards are ASCII prefixes, so their
/// byte offsets are always char boundaries.
pub fn format_key_for_display(key: &str) -> KeyDisplay {
    if key.is_empty() {
        return KeyDisplay {
            lines: vec!["-".to_string()],
            size: 16.0,
        };
    }
    let upper = key.trim().to_ascii_uppercase();
    let chars = upper.chars().count();
    let lines: Vec<String> = if upper.starts_with("ARROW") && chars > 5 {
        vec!["ARROW".to_string(), upper[5..].trim().to_string()]
    } else if upper.starts_with("NUMPAD") && chars > 6 {
        vec!["NUMPAD".to_string(), upper[6..].trim().to_string()]
    } else if upper.starts_with("NUM ") && chars > 4 {
        vec!["NUM".to_string(), upper[4..].trim().to_string()]
    } else if upper.starts_with("PAGE") && chars > 4 {
        vec!["PAGE".to_string(), upper[4..].trim().to_string()]
    } else if upper == "BACKSPACE" {
        vec!["BACK".to_string(), "SPACE".to_string()]
    } else if upper == "CAPSLOCK" {
        vec!["CAPS".to_string(), "LOCK".to_string()]
    } else if upper.starts_with("LEFT") && chars > 4 {
        vec!["LEFT".to_string(), upper[4..].trim().to_string()]
    } else if upper.starts_with("RIGHT") && chars > 5 {
        vec!["RIGHT".to_string(), upper[5..].trim().to_string()]
    } else if upper.contains(' ') {
        let parts: Vec<&str> = upper.split_whitespace().collect();
        if parts.len() == 2 {
            vec![parts[0].to_string(), parts[1].to_string()]
        } else if parts.len() > 2 {
            vec![parts[0].to_string(), parts[1..].join(" ")]
        } else {
            vec![upper.clone()]
        }
    } else if chars >= 7 {
        if let Some(pos) = upper.find(|c: char| c.is_ascii_digit())
            && pos > 0
        {
            // `find` reports a byte index on a char boundary, so this split is
            // safe for any preceding text.
            vec![upper[..pos].to_string(), upper[pos..].to_string()]
        } else {
            let split = char_boundary_at_half(&upper);
            vec![upper[..split].to_string(), upper[split..].to_string()]
        }
    } else {
        vec![upper.clone()]
    };

    if lines.len() > 1 {
        let max_len = lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        let size = if max_len <= 4 {
            12.0
        } else if max_len <= 6 {
            11.0
        } else {
            9.5
        };
        KeyDisplay { lines, size }
    } else {
        let size = if chars <= 2 {
            18.0
        } else if chars <= 4 {
            14.0
        } else {
            11.0
        };
        KeyDisplay { lines, size }
    }
}

/// The byte offset of the halfway character boundary, rounding up, so
/// `text[..split]` and `text[split..]` are valid halves of `text`.
fn char_boundary_at_half(text: &str) -> usize {
    let half = text.chars().count().div_ceil(2);
    text.char_indices()
        .nth(half)
        .map_or(text.len(), |(index, _)| index)
}

fn paint(
    ui: &Ui,
    rect: Rect,
    direction: &Direction,
    state: &Keycap<'_>,
    hovered: bool,
    held: bool,
) {
    // `active` is the Iced outer test -- rebinding or a live press -- and it
    // drives the legend and arrow inks. The mouse-down tint is a Normal-mode
    // status and does not recolor them.
    let active = state.mode != KeycapMode::Normal;
    let (fill, edge, shadow, ink) = match state.mode {
        KeycapMode::Rebinding => (
            direction.accent,
            theme::KEYCAP_REBIND_EDGE,
            glow(direction.ring, theme::KEYCAP_REBIND_GLOW_BLUR),
            Color32::WHITE,
        ),
        KeycapMode::Pressed => (
            direction.accent,
            direction.accent,
            glow(direction.ring, theme::KEYCAP_PRESSED_GLOW_BLUR),
            Color32::WHITE,
        ),
        KeycapMode::Normal => {
            let fill = if held {
                direction.accent
            } else if state.duplicate {
                theme::ERROR_BG
            } else if hovered {
                theme::KEYCAP_HOVER_BG
            } else {
                theme::SURFACE
            };
            let ink = if held {
                Color32::WHITE
            } else if state.duplicate {
                theme::ERROR_TEXT
            } else {
                theme::BODY_TEXT
            };
            let edge = if state.duplicate {
                theme::ERROR_BORDER
            } else if held {
                direction.accent
            } else if hovered {
                theme::KEYCAP_HOVER_BORDER
            } else {
                theme::BORDER
            };
            (fill, edge, theme::SHADOW_KEYCAP, ink)
        }
    };

    let radius = CornerRadius::same(RADIUS);
    // The ring and drop shadows paint outside the keycap's own box, so they
    // take a clip expanded by the blur; clipping them to `rect` would cut the
    // rebind glow off at the keycap's edge and leave only the Iced behaviour's
    // inner half. Box and label still clip to the keycap.
    ui.painter_at(rect.expand(SHADOW_REACH))
        .add(shadow.as_shape(rect, radius));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(rect, radius, Stroke::new(2.0, edge), StrokeKind::Inside);

    let content = rect.shrink(PADDING);
    let legend = Rect::from_min_size(content.min, Vec2::new(content.width(), LEGEND_HEIGHT));
    let legend_ink = if active {
        // The Iced keycap's legend: `Color::from_rgba(1.0, 1.0, 1.0, 0.8)`.
        Color32::from_white_alpha(204)
    } else {
        theme::ICON_MUTED
    };
    // Text is laid out with `PLACEHOLDER` so the paint-time colour below is
    // what actually renders; a galley laid out with a real colour ignores the
    // colour handed to `Painter::galley` (see `secondary_button`'s regression
    // test in `mapping.rs`).
    let legend_galley = painter.layout_no_wrap(
        direction.label.to_owned(),
        FontId::proportional(LEGEND_SIZE),
        Color32::PLACEHOLDER,
    );
    stamp_galley(
        &painter,
        legend.left_top(),
        &legend_galley,
        legend_ink,
        LEGEND_SIZE,
    );
    let arrow_ink = if active {
        Color32::WHITE
    } else {
        direction.accent
    };
    paint_arrow(
        &painter,
        Rect::from_center_size(
            Pos2::new(legend.right() - ARROW_SIZE / 2.0, legend.center().y),
            Vec2::splat(ARROW_SIZE),
        ),
        direction.arrow,
        arrow_ink,
    );

    let center = Rect::from_min_max(
        Pos2::new(legend.left(), legend.bottom() + LEGEND_GAP),
        content.max,
    )
    .center();
    match state.mode {
        KeycapMode::Rebinding => {
            paint_text_block(
                &painter,
                center,
                &["...".to_owned()],
                18.0,
                0.0,
                Color32::WHITE,
            );
        }
        _ => {
            let display = format_key_for_display(state.name);
            let gap = if display.lines.len() > 1 { 1.0 } else { 0.0 };
            paint_text_block(&painter, center, &display.lines, display.size, gap, ink);
        }
    }
}

/// Paints one galley, stamping it twice so it reads heavier.
///
/// egui's bundled faces ship a single weight, so the Iced port's
/// `UI_FONT_BOLD`/`UI_FONT_BLACK` families cannot be selected here, and
/// `RichText::strong` only recolours. A sub-pixel second stamp is the closest
/// the default stack gets without pinning a named system face, which
/// `docs/architecture/ui.md` forbids. A shared weight strategy belongs to the
/// theme module once T2 lands; this is T3's local stand-in.
pub(super) fn stamp_galley(
    painter: &Painter,
    pos: Pos2,
    galley: &Arc<Galley>,
    color: Color32,
    size: f32,
) {
    painter.galley(pos, galley.clone(), color);
    painter.galley(
        pos + Vec2::new((size * 0.04).max(0.35), 0.0),
        galley.clone(),
        color,
    );
}

/// Paints one or two centred lines as a block, with the same weight
/// approximation as [`stamp_galley`].
fn paint_text_block(
    painter: &Painter,
    center: Pos2,
    lines: &[String],
    size: f32,
    gap: f32,
    color: Color32,
) {
    // `PLACEHOLDER` keeps the layout colour-neutral: the paint-time `color`
    // below is the one that renders.
    let galleys: Vec<Arc<Galley>> = lines
        .iter()
        .map(|line| {
            painter.layout_no_wrap(
                line.clone(),
                FontId::proportional(size),
                Color32::PLACEHOLDER,
            )
        })
        .collect();
    let height: f32 = galleys.iter().map(|galley| galley.size().y).sum::<f32>()
        + gap * galleys.len().saturating_sub(1) as f32;
    let mut y = center.y - height / 2.0;
    for galley in &galleys {
        stamp_galley(
            painter,
            Pos2::new(center.x - galley.size().x / 2.0, y),
            galley,
            color,
            size,
        );
        y += galley.size().y + gap;
    }
}

/// The direction legend's arrow, tracing the Iced `icons::draw_icon` geometry
/// so the two ports stay comparable.
fn paint_arrow(painter: &Painter, rect: Rect, arrow: Arrow, color: Color32) {
    let size = rect.width().min(rect.height());
    let stroke = Stroke::new((size * 0.1).max(1.2), color);
    let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
    let line = |coordinates: &[(f32, f32)]| {
        painter.add(Shape::line(
            coordinates.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    match arrow {
        Arrow::Up => {
            line(&[(0.5, 0.85), (0.5, 0.15)]);
            line(&[(0.22, 0.43), (0.5, 0.15), (0.78, 0.43)]);
        }
        Arrow::Down => {
            line(&[(0.5, 0.15), (0.5, 0.85)]);
            line(&[(0.22, 0.57), (0.5, 0.85), (0.78, 0.57)]);
        }
        Arrow::Left => {
            line(&[(0.85, 0.5), (0.15, 0.5)]);
            line(&[(0.43, 0.22), (0.15, 0.5), (0.43, 0.78)]);
        }
        Arrow::Right => {
            line(&[(0.15, 0.5), (0.85, 0.5)]);
            line(&[(0.72, 0.22), (0.85, 0.5), (0.72, 0.78)]);
        }
    }
}

/// The Iced pressed/rebinding ring: a zero-offset glow in the direction's own
/// ring colour, at the theme-owned blur for the state.
fn glow(color: Color32, blur: u8) -> Shadow {
    Shadow {
        offset: [0, 0],
        blur,
        spread: 0,
        color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_key_for_display_splits_compound_keys() {
        let single = format_key_for_display("W");
        assert_eq!(single.lines, vec!["W"]);
        assert_eq!(single.size, 18.0);

        let numpad = format_key_for_display("Numpad 8");
        assert_eq!(numpad.lines, vec!["NUMPAD", "8"]);

        let arrow = format_key_for_display("Arrow Up");
        assert_eq!(arrow.lines, vec!["ARROW", "UP"]);

        let backspace = format_key_for_display("Backspace");
        assert_eq!(backspace.lines, vec!["BACK", "SPACE"]);

        let capslock = format_key_for_display("CapsLock");
        assert_eq!(capslock.lines, vec!["CAPS", "LOCK"]);
    }

    #[test]
    fn compound_lines_autoscale_to_their_longest_line() {
        assert_eq!(format_key_for_display("Num 5").size, 12.0);
        assert_eq!(format_key_for_display("Page Down").size, 12.0);
        assert_eq!(format_key_for_display("Right Shift").size, 11.0);
        assert_eq!(format_key_for_display("Numpad Pagedown").size, 9.5);
        assert_eq!(format_key_for_display("F1").size, 18.0);
        assert_eq!(format_key_for_display("Tab").size, 14.0);
        assert_eq!(format_key_for_display("Enter").size, 11.0);
    }

    #[test]
    fn an_empty_key_name_draws_a_dash() {
        let empty = format_key_for_display("");
        assert_eq!(empty.lines, vec!["-"]);
        assert_eq!(empty.size, 16.0);
    }

    #[test]
    fn multi_byte_names_split_without_panicking() {
        // Seven characters of three bytes each: a byte-based midpoint lands
        // inside a character and panics.
        let long = format_key_for_display("가나다라마바사");
        assert_eq!(long.lines, vec!["가나다라", "마바사"]);
        assert_eq!(long.size, 12.0);

        // Five characters, fifteen bytes: below the seven-*character* split
        // threshold even though it exceeds it in bytes.
        let five = format_key_for_display("가나다라마");
        assert_eq!(five.lines, vec!["가나다라마"]);
        assert_eq!(five.size, 11.0);

        // Two characters, four bytes: the size classification counts
        // characters too, so this keeps the two-character size.
        let short = format_key_for_display("F한");
        assert_eq!(short.lines, vec!["F한"]);
        assert_eq!(short.size, 18.0);

        // A digit after multi-byte text splits on the digit's own char
        // boundary.
        let digit = format_key_for_display("가나다라마바4");
        assert_eq!(digit.lines, vec!["가나다라마바", "4"]);
    }

    #[test]
    fn keycap_labels_name_the_direction_and_key() {
        assert_eq!(keycap_label(&UP, KeycapMode::Normal, "W"), "UP keycap: W");
        assert_eq!(
            keycap_label(&DOWN, KeycapMode::Pressed, "S"),
            "DOWN keycap: S (pressed)"
        );
        assert_eq!(
            keycap_label(&RIGHT, KeycapMode::Rebinding, "D"),
            "RIGHT keycap: rebinding"
        );
    }

    #[test]
    fn directions_keep_the_reference_accents() {
        assert_eq!(UP.accent, theme::IMMEDIATE_ACCENT);
        assert_eq!(LEFT.accent, theme::INDIGO_600);
        assert_eq!(RIGHT.accent, theme::MIX_TEXT);
        assert_eq!(DOWN.accent, theme::VIOLET_600);
        // The ring is the mode accent's lighter step and must differ per
        // direction, or the rebind glow loses its identity.
        let rings = [UP.ring, DOWN.ring, LEFT.ring, RIGHT.ring];
        for (index, ring) in rings.iter().enumerate() {
            assert!(
                !rings
                    .iter()
                    .enumerate()
                    .any(|(other, value)| other != index && value == ring),
                "ring colour must be unique"
            );
        }
    }
}
