//! The Key mappings D-pad keycap.
//!
//! The Iced original is `fn keycap` in `iced-ui/app.rs`, which took its paint
//! from `iced-ui/theme.rs` (`theme::keycap` / `theme::KeycapMode`). egui paints
//! the box, the glow, the legend, and both label lines directly, and the
//! interactive node comes from the response: a custom-drawn widget is
//! invisible to AccessKit, so `ui.interact` plus a labelled [`WidgetInfo`] is
//! what lets `egui_kittest` and the inspection tools find the keycap by name.
//!
//! Colours and shadows come from [`super::theme`] wherever the theme owns the
//! token. The reference draws its keycap glyphs with hex literals
//! (`CanvasIcon color=...`) that sit on a different palette lineage than the
//! theme's class-based tokens, so the per-direction arrow inks and ring steps
//! stay beside the direction table they belong to; everything else is
//! theme-owned.

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
/// The pressed keycap's tactile compression: the reference's `scale-95`. This
/// is a paint-time shrink about the box centre; the allocated [`KEYCAP_SIZE`]
/// square is untouched, which is what a CSS transform does to its layout box.
const PRESS_SCALE: f32 = 0.95;
/// The hover edit badge: the reference's `w-5 h-5` circle sitting 4px past the
/// box's top-right corner (`-top-1 -right-1`).
const BADGE_SIZE: f32 = 20.0;
const BADGE_OFFSET: f32 = 4.0;
/// The badge's pencil, `CanvasIcon name="edit" size={12}`.
const BADGE_ICON: f32 = 12.0;
/// Drawn indigo-700 (reference literal `#4338ca`), the badge pencil's ink.
/// Not [`theme::INDIGO_700`]: that token is the rendered `bg-indigo-700`
/// class (`#432dd7`), a different lineage than the reference's drawn hex.
const BADGE_PENCIL: Color32 = Color32::from_rgb(0x43, 0x38, 0xca);
/// The pressed keycap's inset shadow (`shadow-inner`). epaint has no inset
/// shadow primitive, so two translucent inside strokes -- a wider, fainter
/// band and a tighter one -- approximate the CSS `inset 0 2px 4px
/// rgb(0 0 0 / 0.05)` darkening along the box's inner edge.
const INNER_SHADOW: Color32 = Color32::from_black_alpha(8);
const INNER_SHADOW_WIDE: f32 = 3.0;
const INNER_SHADOW_TIGHT: f32 = 1.5;

/// One D-pad direction's display identity: its capture slot, sub-legend,
/// arrow, fill accent, arrow ink, and outer ring glow colour. Ported from the
/// Iced `Direction` constants; `docs/architecture/ui.md` fixes the fill
/// mapping (UP = Immediate, LEFT = Press Delay, RIGHT = Random Mix, DOWN =
/// Release Delay).
pub struct Direction {
    pub slot: KeySlot,
    pub label: &'static str,
    pub arrow: Arrow,
    /// The direction's SOCD mode colour: the accent fill for the pressed and
    /// rebinding states.
    pub accent: Color32,
    /// The legend arrow's resting ink. The reference draws it with its own hex
    /// literal (`KeyMappingsCard.tsx` `accentColor`), a different value than
    /// the CSS-class mode colour the fill takes.
    pub ink: Color32,
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
    ink: Color32::from_rgb(0x25, 0x63, 0xeb),
    ring: Color32::from_rgb(0x93, 0xc5, 0xfd),
};
pub const DOWN: Direction = Direction {
    slot: KeySlot::VerticalSecond,
    label: "DOWN",
    arrow: Arrow::Down,
    accent: theme::VIOLET_600,
    ink: Color32::from_rgb(0x7c, 0x3a, 0xed),
    ring: Color32::from_rgb(0xc4, 0xb5, 0xfd),
};
pub const LEFT: Direction = Direction {
    slot: KeySlot::HorizontalFirst,
    label: "LEFT",
    arrow: Arrow::Left,
    accent: theme::INDIGO_600,
    ink: Color32::from_rgb(0x63, 0x66, 0xf1),
    ring: Color32::from_rgb(0xa5, 0xb4, 0xfc),
};
pub const RIGHT: Direction = Direction {
    slot: KeySlot::HorizontalSecond,
    label: "RIGHT",
    arrow: Arrow::Right,
    accent: theme::MIX_TEXT,
    ink: Color32::from_rgb(0x93, 0x33, 0xea),
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
    // The pressed box is the reference's `scale-95`: a physical keypress
    // compresses the painted square about its centre while the allocated box
    // -- and the D-pad grid that measures it -- stays put, exactly as a CSS
    // transform leaves the layout box alone.
    let box_rect = painted_box(rect, state.mode);
    // The ring and drop shadows paint outside the keycap's own box, so they
    // take a clip expanded by the blur; clipping them to `rect` would cut the
    // rebind glow off at the keycap's edge and leave only the Iced behaviour's
    // inner half. Box and label still clip to the keycap; the hover badge
    // sits outside the box too and is painted on this painter last.
    let halo = ui.painter_at(rect.expand(SHADOW_REACH));
    halo.add(shadow.as_shape(box_rect, radius));
    let painter = ui.painter_at(rect);
    painter.rect_filled(box_rect, radius, fill);
    if state.mode == KeycapMode::Pressed {
        paint_inner_shadow(&painter, box_rect, radius);
    }
    painter.rect_stroke(box_rect, radius, Stroke::new(2.0, edge), StrokeKind::Inside);

    let content = box_rect.shrink(PADDING);
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
        direction.ink
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

    if hovered {
        paint_edit_badge(&halo, box_rect);
    }
}

/// The keycap's drawn box: the allocated square, compressed by [`PRESS_SCALE`]
/// about its centre while a physical key is down (the reference's
/// `scale-95`). Every other mode draws the allocated box.
fn painted_box(rect: Rect, mode: KeycapMode) -> Rect {
    if mode == KeycapMode::Pressed {
        Rect::from_center_size(rect.center(), rect.size() * PRESS_SCALE)
    } else {
        rect
    }
}

/// The pressed keycap's inset shadow (`shadow-inner`): the two translucent
/// inside strokes described at [`INNER_SHADOW`], drawn on the deflated box so
/// the band follows the pressed geometry.
fn paint_inner_shadow(painter: &Painter, rect: Rect, radius: CornerRadius) {
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(INNER_SHADOW_WIDE, INNER_SHADOW),
        StrokeKind::Inside,
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(INNER_SHADOW_TIGHT, INNER_SHADOW),
        StrokeKind::Inside,
    );
}

/// The hover edit badge: a 20px white circle with an [`theme::INDIGO_200`]
/// hairline and the drawn edit glyph, 4px past the box's top-right corner
/// (`-top-1 -right-1`). It sits outside the keycap's own clip, so it takes the
/// halo painter the ring shadows use.
fn paint_edit_badge(painter: &Painter, box_rect: Rect) {
    let badge = Rect::from_min_size(
        Pos2::new(
            box_rect.right() + BADGE_OFFSET - BADGE_SIZE,
            box_rect.top() - BADGE_OFFSET,
        ),
        Vec2::splat(BADGE_SIZE),
    );
    painter.add(theme::SHADOW_KEYCAP.as_shape(badge, theme::CIRCLE_RADIUS));
    painter.circle_filled(badge.center(), BADGE_SIZE / 2.0, theme::SURFACE);
    painter.circle_stroke(
        badge.center(),
        BADGE_SIZE / 2.0 - 0.5,
        Stroke::new(1.0, theme::INDIGO_200),
    );
    theme::paint_icon(
        painter,
        Rect::from_center_size(badge.center(), Vec2::splat(BADGE_ICON)),
        theme::Icon::Edit,
        BADGE_PENCIL,
    );
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

/// The direction legend's arrow: the reference `CanvasIcon` line-and-chevron
/// geometry (a 0.15s padding, a 45-degree chevron, `lineCap`/`lineJoin` round),
/// drawn from the same coordinate tables so the two ports stay comparable.
fn paint_arrow(painter: &Painter, rect: Rect, arrow: Arrow, color: Color32) {
    let size = rect.width().min(rect.height());
    let stroke = Stroke::new((size * 0.1).max(1.2), color);
    let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
    let line = |coordinates: &[(f32, f32)]| {
        let points: Vec<Pos2> = coordinates.iter().map(|&(x, y)| point(x, y)).collect();
        painter.add(Shape::line(points.clone(), stroke));
        // The reference strokes with `lineCap = 'round'` and `lineJoin =
        // 'round'`; epaint has neither, so a disc at every vertex rounds the
        // open ends and fills the chevron's point.
        for vertex in points {
            painter.circle_filled(vertex, stroke.width / 2.0, color);
        }
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

    use egui::epaint::ColorMode;
    use egui_kittest::{Harness, kittest::Queryable};

    /// Renders one `UP` keycap in `mode` and runs the first frame.
    fn render_keycap(mode: KeycapMode) -> Harness<'static> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 200.0))
            .build_ui(move |ui| {
                keycap(
                    ui,
                    &UP,
                    Keycap {
                        name: "W",
                        mode,
                        duplicate: false,
                    },
                );
            });
        harness.run();
        harness
    }

    /// Every top-level shape the last frame painted.
    fn painted_shapes(harness: &Harness<'_>) -> Vec<Shape> {
        harness
            .output()
            .shapes
            .iter()
            .map(|clipped| clipped.shape.clone())
            .collect()
    }

    /// The solid colour of a path stroke, if it has one.
    fn solid_color(color: &ColorMode) -> Option<Color32> {
        match color {
            ColorMode::Solid(color) => Some(*color),
            _ => None,
        }
    }

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

    #[test]
    fn direction_arrow_inks_are_the_reference_drawn_literals() {
        // The `CanvasIcon color=...` arguments in the reference's
        // `renderKeycap` calls (KeyMappingsCard.tsx): blue, indigo, purple and
        // violet drawn hexes. They are a different lineage than the
        // class-based SOCD mode colours the accent holds.
        let channels = |color: Color32| [color.r(), color.g(), color.b()];
        assert_eq!(channels(UP.ink), [0x25, 0x63, 0xeb]);
        assert_eq!(channels(LEFT.ink), [0x63, 0x66, 0xf1]);
        assert_eq!(channels(RIGHT.ink), [0x93, 0x33, 0xea]);
        assert_eq!(channels(DOWN.ink), [0x7c, 0x3a, 0xed]);
        // The fill accent and the arrow ink are distinct concepts with
        // distinct values.
        for direction in [&UP, &DOWN, &LEFT, &RIGHT] {
            assert_ne!(direction.ink, direction.accent);
        }
    }

    #[test]
    fn the_pressed_box_deflates_to_scale_95() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::splat(KEYCAP_SIZE));
        let pressed = painted_box(rect, KeycapMode::Pressed);
        assert!((pressed.width() - 76.0).abs() < 0.01);
        assert!((pressed.height() - 76.0).abs() < 0.01);
        assert!((pressed.center() - rect.center()).length() < 0.01);
        // The compression is paint-time only: normal and rebinding keep the
        // allocated box, so the D-pad grid never reflows.
        assert_eq!(painted_box(rect, KeycapMode::Normal), rect);
        assert_eq!(painted_box(rect, KeycapMode::Rebinding), rect);
    }

    #[test]
    fn the_pressed_keycap_paints_the_deflated_box_and_its_inner_shadow() {
        let harness = render_keycap(KeycapMode::Pressed);
        let rect = harness.get_by_label("UP keycap: W (pressed)").rect();
        let shapes = painted_shapes(&harness);

        let box_shape = shapes
            .iter()
            .find_map(|shape| match shape {
                Shape::Rect(rect_shape) if rect_shape.fill == UP.accent => Some(rect_shape.clone()),
                _ => None,
            })
            .expect("the pressed fill is the direction accent");
        assert!((box_shape.rect.center() - rect.center()).length() < 0.01);
        assert!(
            (box_shape.rect.width() - rect.width() * PRESS_SCALE).abs() < 0.01,
            "the pressed box must shrink about its centre"
        );

        let bands: Vec<egui::epaint::RectShape> = shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Rect(rect_shape) if rect_shape.stroke.color == INNER_SHADOW => {
                    Some(rect_shape.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            bands.len(),
            2,
            "the inset shadow is the two-layer approximation"
        );
        for band in &bands {
            assert_eq!(band.rect, box_shape.rect);
            assert_eq!(band.stroke_kind, StrokeKind::Inside);
        }
    }

    #[test]
    fn the_arrow_keeps_the_canvas_icon_geometry_with_round_caps() {
        let harness = render_keycap(KeycapMode::Normal);
        let shapes = painted_shapes(&harness);

        let arrow: Vec<egui::epaint::PathShape> = shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Path(path) if solid_color(&path.stroke.color) == Some(UP.ink) => {
                    Some(path.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(arrow.len(), 2, "the arrow is a stem and a chevron");
        let stem = arrow
            .iter()
            .find(|path| path.points.len() == 2)
            .expect("the stem is the two-point path");
        let chevron = arrow
            .iter()
            .find(|path| path.points.len() == 3)
            .expect("the chevron is the three-point path");
        // The reference proportions: a stem between the 0.15s paddings and a
        // 45-degree chevron whose arms reach 0.28s.
        assert!(((stem.points[1] - stem.points[0]).length() - 0.7 * ARROW_SIZE).abs() < 0.01);
        assert!(
            ((chevron.points[2].x - chevron.points[0].x).abs() - 0.56 * ARROW_SIZE).abs() < 0.01
        );
        assert!(
            ((chevron.points[0].y - chevron.points[1].y).abs() - 0.28 * ARROW_SIZE).abs() < 0.01
        );
        assert_eq!(
            chevron.points[1], stem.points[1],
            "the chevron meets the stem's tip"
        );

        // `CanvasIcon` strokes with `lineCap`/`lineJoin` round; epaint has no
        // cap option, so every vertex carries a disc of the stroke's radius.
        let caps: Vec<egui::epaint::CircleShape> = shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Circle(circle) if circle.fill == UP.ink => Some(*circle),
                _ => None,
            })
            .collect();
        let vertices: usize = arrow.iter().map(|path| path.points.len()).sum();
        assert_eq!(caps.len(), vertices, "one cap disc per arrow vertex");
        for path in &arrow {
            for vertex in &path.points {
                assert!(
                    caps.iter().any(|cap| (cap.center - *vertex).length() < 0.01
                        && (cap.radius - path.stroke.width / 2.0).abs() < 0.01),
                    "vertex {vertex:?} has no cap disc"
                );
            }
        }
    }

    #[test]
    fn the_edit_badge_hangs_off_the_top_right_corner_on_hover() {
        let mut harness = render_keycap(KeycapMode::Normal);
        let rect = harness.get_by_label("UP keycap: W").rect();
        let find_badge = |shapes: &[Shape]| {
            shapes.iter().find_map(|shape| match shape {
                Shape::Circle(circle)
                    if circle.fill == theme::SURFACE
                        && (circle.radius - BADGE_SIZE / 2.0).abs() < 0.01 =>
                {
                    Some(*circle)
                }
                _ => None,
            })
        };
        assert!(
            find_badge(&painted_shapes(&harness)).is_none(),
            "the badge is hover-only"
        );

        harness.get_by_label("UP keycap: W").hover();
        harness.run();
        let shapes = painted_shapes(&harness);
        let badge = find_badge(&shapes).expect("hovering raises the badge");
        let expected = Pos2::new(
            rect.right() - BADGE_SIZE / 2.0 + BADGE_OFFSET,
            rect.top() + BADGE_SIZE / 2.0 - BADGE_OFFSET,
        );
        assert!(
            (badge.center - expected).length() < 0.01,
            "badge {badge:?} must hang off the top-right corner"
        );
        assert!(
            shapes
                .iter()
                .any(|shape| matches!(shape, Shape::Circle(circle)
                if circle.stroke.color == theme::INDIGO_200)),
            "the badge keeps its indigo-200 hairline"
        );
        assert!(
            shapes.iter().any(|shape| matches!(shape, Shape::Path(path)
                if solid_color(&path.stroke.color) == Some(BADGE_PENCIL))),
            "the badge carries the drawn pencil"
        );
    }
}
