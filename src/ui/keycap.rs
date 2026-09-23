//! The Key mappings D-pad keycap, and the shared keycap chrome every other
//! keycap surface in the window paints through ([`Surface`], [`paint_surface`]).
//!
//! A keycap is drawn directly rather than composed from widgets: the box, the
//! glow, the legend, and both label lines are painted onto the layer, and the
//! interactive node comes from the response -- a custom-drawn widget is
//! invisible to AccessKit, so `ui.interact` plus a labelled [`WidgetInfo`] is
//! what lets `egui_kittest` and the inspection tools find the keycap by name.
//!
//! Colours and shadows come from [`super::theme`]. Every direction's arrow ink,
//! fill accent, edge, and ring step is a theme token too; the direction table
//! below only says which of them each direction takes.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontId, Galley, Id, Painter, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, epaint::Shadow,
};

use crate::protocol::KeySlot;

use super::{message::Message, motion, theme};

/// The keycap's square box, and the reference's size for every D-pad cap.
pub const KEYCAP_SIZE: f32 = 80.0;
/// The keycap's inner padding, which is what positions both text rows.
const PADDING: f32 = 10.0;
/// The sub-legend line: a 10px label at the reference's 1.3 line height.
const LEGEND_SIZE: f32 = 10.0;
const LEGEND_HEIGHT: f32 = 13.0;
/// The key-name hero region's lift above the legend row's bottom edge.
///
/// The reference's hero is `w-full flex-1 flex items-center justify-center
/// -mt-1`, so it starts at the legend row's bottom *pulled up 4px* and runs to
/// the content box's bottom edge. That is what puts the reference's key name
/// slightly below the keycap's own centre rather than on it, and why the hero
/// region is bottom-weighted: the 4px lift is subtracted from the top while the
/// bottom stays at the content edge.
///
/// Measured on the reference (`w-18 h-18` cap, 80px, `p-2.5` + `border-2`):
/// content 12..68, legend row 12..27, hero 23..68, hero centre 45.5 against the
/// box centre 40 -- 5.5px low. This port's content box is 10..70 and its legend
/// row 10..23 (the reference's padding and 13px line height, both pinned by
/// tests), so the same rule lands the hero centre at 44.5, 4.5px low.
const HERO_LIFT: f32 = 4.0;
/// The legend's arrow glyph, as `icons::icon(arrow, 12.0, ...)` drew it.
const ARROW_SIZE: f32 = 12.0;
/// How far a glow or drop shadow reaches past the keycap's own box: the
/// widest `Shadow::blur` here is [`theme::KEYCAP_REBIND_GLOW_BLUR`], and
/// epaint's penumbra extends half the blur in each direction.
const SHADOW_REACH: f32 = 12.0;
/// The pressed keycap's tactile compression: the reference's `scale-95`. This
/// is a paint-time shrink about the box centre; the allocated box is
/// untouched, which is what a CSS transform does to its layout box.
///
/// The reference reaches it through `transition-all duration-75`, so the
/// compression is interpolated over 75 ms rather than snapping -- see
/// [`super::motion`] and [`press_scale`].
pub const PRESS_SCALE: f32 = 0.95;
/// The rebinding keycap's `scale-105` lift, animated the same way.
const REBIND_SCALE: f32 = 1.05;
/// The hover edit badge: the reference's `w-5 h-5` circle sitting 4px past the
/// box's top-right corner (`-top-1 -right-1`).
const BADGE_SIZE: f32 = 20.0;
const BADGE_OFFSET: f32 = 4.0;
/// The badge's pencil glyph size.
const BADGE_ICON: f32 = 12.0;
/// The pressed keycap's inset shadow (`shadow-inner`). epaint has no inset
/// shadow primitive, so two translucent inside strokes -- a wider, fainter
/// band and a tighter one -- approximate the CSS `inset 0 2px 4px
/// rgb(0 0 0 / 0.05)` darkening along the box's inner edge.
pub const INNER_SHADOW_WIDE: f32 = 3.0;
pub const INNER_SHADOW_TIGHT: f32 = 1.5;

/// One direction's display identity: its capture slot, sub-legend, arrow,
/// fill accent, edge, and outer ring colour.
///
/// The window's one direction table. The D-pad's four caps read it, and so do
/// the preview card's two example keys -- A is [`LEFT`] and D is [`RIGHT`] --
/// so a direction's colours are written once and the same key reads the same
/// on both surfaces. The fill mapping is fixed (UP = Immediate, LEFT = Press
/// Delay, RIGHT = Random Mix, DOWN = Release Delay).
pub struct Direction {
    pub slot: KeySlot,
    /// The direction's own name, which is the D-pad cap's sub-legend row. The
    /// preview's example keys print their key letter instead, because the
    /// letter is what the reference draws there.
    pub label: &'static str,
    pub arrow: Arrow,
    /// The direction's mode base: the accent fill for the pressed and
    /// rebinding states, and the resting arrow's ink. Every direction reads
    /// its mode's Tailwind `-600` step (see `theme`'s mode-base block).
    pub accent: Color32,
    /// The held fill's one-step-darker edge, `-700`. Only the preview's
    /// example keys draw it: the D-pad cap takes its own accent as the edge.
    pub edge: Color32,
    /// The held and rebinding states' outer glow and ring: the mode's `-300`
    /// step.
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
    accent: theme::BLUE_600,
    edge: theme::BLUE_700,
    ring: theme::BLUE_300,
};
pub const DOWN: Direction = Direction {
    slot: KeySlot::VerticalSecond,
    label: "DOWN",
    arrow: Arrow::Down,
    accent: theme::VIOLET_600,
    edge: theme::VIOLET_700,
    ring: theme::VIOLET_300,
};
pub const LEFT: Direction = Direction {
    slot: KeySlot::HorizontalFirst,
    label: "LEFT",
    arrow: Arrow::Left,
    accent: theme::INDIGO_600,
    edge: theme::INDIGO_700,
    ring: theme::INDIGO_300,
};
pub const RIGHT: Direction = Direction {
    slot: KeySlot::HorizontalSecond,
    label: "RIGHT",
    arrow: Arrow::Right,
    accent: theme::PURPLE_600,
    edge: theme::PURPLE_700,
    ring: theme::PURPLE_300,
};

/// The keycap's paint state. [`super::theme::KeycapMode`] is the one owner
/// (it came from the keycap's own style function); this re-export
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
/// slot, which is the reference's `on_press` mapping unchanged.
pub fn keycap(ui: &mut Ui, direction: &Direction, state: Keycap<'_>) -> Option<Message> {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(KEYCAP_SIZE), Sense::click());
    let label = keycap_label(direction, state.mode, state.name);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), &label));

    // The reference's `transition-all duration-75` covers the whole keycap,
    // but only the transform is a continuous quantity here: the fills and inks
    // are theme tokens with no meaningful midpoint. One scale per keycap, keyed
    // by its direction label so the four caps animate independently.
    let scale = mode_scale(
        ui,
        Id::new("ui-keycap-scale").with(direction.label),
        state.mode,
    );

    paint(
        ui,
        rect,
        direction,
        &state,
        response.hovered(),
        response.is_pointer_button_down_on(),
        scale,
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

/// Splits a key name into at most two display lines. Compound names
/// (`Numpad 8`, `Arrow Up`, `Backspace`) are the case this exists for.
///
/// The window's one owner of the split. Every surface that draws a key name
/// reads its lines from here -- the D-pad cap, the preview's example keys, and
/// the timeline's gutter caps -- so one key reads the same wherever it lands.
///
/// Lengths are counted in characters, never bytes: a name may carry non-ASCII
/// text, and slicing a UTF-8 string at a byte offset that falls inside a
/// character panics. The `starts_with` guards are ASCII prefixes, so their
/// byte offsets are always char boundaries.
pub fn key_lines(key: &str) -> Vec<String> {
    let upper = key.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return vec!["-".to_owned()];
    }
    let chars = upper.chars().count();
    if upper.starts_with("ARROW") && chars > 5 {
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
    }
}

/// A key name's display lines and the size the D-pad cap draws them at.
/// The reference's `format_key_for_display`, with one difference: the
/// font weight is gone with egui's single-weight bundled faces.
pub struct KeyDisplay {
    pub lines: Vec<String>,
    pub size: f32,
}

/// [`key_lines`] plus the D-pad cap's own size for them: a compound name takes
/// the size its longest line needs, a single line the size its length needs.
///
/// The preview and timeline caps do not use this size. Their caps are 52px and
/// 40px against this one's 80px, so each carries its own size policy while
/// reading the same lines.
pub fn format_key_for_display(key: &str) -> KeyDisplay {
    let lines = key_lines(key);
    let size = if key.trim().is_empty() {
        16.0
    } else if lines.len() > 1 {
        let max_len = lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        if max_len <= 4 {
            12.0
        } else if max_len <= 6 {
            11.0
        } else {
            9.5
        }
    } else {
        let chars = lines[0].chars().count();
        if chars <= 2 {
            18.0
        } else if chars <= 4 {
            14.0
        } else {
            11.0
        }
    };
    KeyDisplay { lines, size }
}

/// The byte offset of the halfway character boundary, rounding up, so
/// `text[..split]` and `text[split..]` are valid halves of `text`.
fn char_boundary_at_half(text: &str) -> usize {
    let half = text.chars().count().div_ceil(2);
    text.char_indices()
        .nth(half)
        .map_or(text.len(), |(index, _)| index)
}

/// The shared keycap surface.
///
/// Every keycap in the window paints its box through [`paint_surface`]: the
/// D-pad's 80px mapping keycap, the preview's 52px example key, and the
/// timeline gutter's 40px cap. Size, radius, edge width, and content differ
/// between the three; the press effect does not, and owning it here is what
/// makes that true by construction rather than by three parallel
/// implementations that drift.
///
/// The split is deliberate. This type carries the *chrome* -- the box, its
/// lift, the compression, the glow, the ring, the inset shadow, and the edge
/// -- and each caller draws its own content into the compressed box, because
/// the three surfaces differ in exactly that way: the timeline cap draws a
/// label and a compact arrow, the preview keys draw a letter over an arrow,
/// and the D-pad caps draw a sub-legend row over a hero.
#[derive(Clone, Copy)]
pub struct Surface {
    /// The box the keycap occupies. The compression shrinks the *painted* box
    /// about this rect's centre and leaves the rect itself where it is, which
    /// is what a CSS `transform: scale()` does to its layout box.
    pub rect: Rect,
    /// The box's corner radius.
    pub radius: CornerRadius,
    /// The fill painted inside the edge.
    pub fill: Color32,
    /// The edge, drawn inside the box.
    pub edge: Stroke,
    /// The drop shadow under the box: the resting lift, or a state's glow.
    pub shadow: Shadow,
    /// Whether the pressed `shadow-inner` band is drawn.
    pub inner_shadow: bool,
    /// The held outer ring (the reference's `ring-2`): its stroke width and
    /// colour, drawn outside the compressed box. `None` where a surface
    /// renders its ring as [`Surface::shadow`]'s glow alone.
    pub ring: Option<(f32, Color32)>,
    /// The press compression, already interpolated by [`motion`].
    pub scale: f32,
}

/// What [`paint_surface`] hands back to its caller.
pub struct Painted {
    /// The box's own clip expanded by [`SHADOW_REACH`]: the painter for
    /// anything a caller draws *outside* the keycap, such as the D-pad's
    /// hover badge.
    pub halo: Painter,
    /// The compressed box the content was drawn into.
    pub box_rect: Rect,
}

/// Paints one keycap's chrome, then hands the compressed box to `content`.
///
/// This is the single owner of the press effect: the `scale-95` compression
/// about the box's centre, the accent glow, the outer ring, the
/// `shadow-inner` band, and the edge. The scale arrives already interpolated
/// (see [`press_scale`]), so a press and its release glide rather than snap.
pub fn paint_surface(ui: &Ui, surface: &Surface, content: impl FnOnce(&Painter, Rect)) -> Painted {
    let Surface {
        rect,
        radius,
        fill,
        edge,
        shadow,
        inner_shadow,
        ring,
        scale,
    } = *surface;
    let box_rect = Rect::from_center_size(rect.center(), rect.size() * scale);
    // The ring and the shadows paint outside the keycap's own box, so they
    // take a clip expanded by the blur; clipping them to `rect` would cut a
    // glow off at the keycap's edge and leave only its inner half. The box
    // and its content still clip to the keycap.
    let halo = ui.painter_at(rect.expand(SHADOW_REACH));
    halo.add(shadow.as_shape(box_rect, radius));
    let painter = ui.painter_at(rect);
    if let Some((width, color)) = ring {
        // The ring strokes *outside* the box, and at rest the box fills the
        // allocated rect, so it has no room inside it: on `painter` it would
        // be clipped in half wherever the scale is 1.
        halo.rect_stroke(
            box_rect,
            radius,
            Stroke::new(width, color),
            StrokeKind::Outside,
        );
    }
    painter.rect_filled(box_rect, radius, fill);
    if inner_shadow {
        paint_inner_shadow(&painter, box_rect, radius);
    }
    painter.rect_stroke(box_rect, radius, edge, StrokeKind::Inside);
    content(&painter, box_rect);
    Painted { halo, box_rect }
}

/// The shared press transition: the reference's `duration-75` curve, keyed by
/// `id` so each keycap animates independently. Both entry points below are
/// this one call with a different target, so the duration and the easing are
/// written once for all three surfaces.
fn animate_scale(ui: &Ui, id: Id, target: f32) -> f32 {
    motion::animate(
        ui,
        id,
        target,
        motion::DURATION_75_SECS,
        motion::Easing::Standard,
    )
}

/// The D-pad cap's transition: the mode's painted scale, so the rebinding
/// `scale-105` lift and the press `scale-95` compression are one transition.
pub fn mode_scale(ui: &Ui, id: Id, mode: KeycapMode) -> f32 {
    animate_scale(ui, id, painted_scale(mode))
}

/// The binary surfaces' transition: `scale-95` while held, the allocated box
/// otherwise. The preview's example keys and the timeline's gutter caps have
/// no rebinding state, so this is the whole of their press effect.
pub fn press_scale(ui: &Ui, id: Id, held: bool) -> f32 {
    animate_scale(ui, id, if held { PRESS_SCALE } else { 1.0 })
}

fn paint(
    ui: &Ui,
    rect: Rect,
    direction: &Direction,
    state: &Keycap<'_>,
    hovered: bool,
    held: bool,
    scale: f32,
) {
    // `active` is the outer test -- rebinding or a live press -- and it drives
    // the legend and arrow inks. The mouse-down tint is a Normal-mode status
    // and does not recolor them.
    let active = state.mode != KeycapMode::Normal;
    let (fill, edge, shadow, ink) = match state.mode {
        KeycapMode::Rebinding => (
            direction.accent,
            theme::WHITE_40,
            glow(direction.ring, theme::KEYCAP_REBIND_GLOW_BLUR),
            theme::WHITE,
        ),
        KeycapMode::Pressed => (
            direction.accent,
            direction.accent,
            glow(direction.ring, theme::KEYCAP_PRESSED_GLOW_BLUR),
            theme::WHITE,
        ),
        KeycapMode::Normal => {
            let fill = if held {
                direction.accent
            } else if state.duplicate {
                theme::RED_50
            } else if hovered {
                theme::SLATE_50
            } else {
                theme::WHITE
            };
            let ink = if held {
                theme::WHITE
            } else if state.duplicate {
                theme::RED_600
            } else {
                theme::SLATE_900
            };
            let edge = if state.duplicate {
                theme::RED_400
            } else if held {
                direction.accent
            } else if hovered {
                theme::INDIGO_400
            } else {
                theme::SLATE_200
            };
            (fill, edge, theme::SHADOW_XS, ink)
        }
    };

    // The reference's `ring-2` while pressed and `ring-4` while rebinding, in
    // the direction's own ring step. The class also draws the soft halo
    // [`glow`] carries above, so the two are one effect: the glow is the
    // penumbra and the ring the hard edge inside it.
    let ring = match state.mode {
        KeycapMode::Normal => None,
        KeycapMode::Pressed => Some((theme::KEYCAP_RING_PRESSED * scale, direction.ring)),
        KeycapMode::Rebinding => Some((theme::KEYCAP_RING_REBIND * scale, direction.ring)),
    };
    let painted = paint_surface(
        ui,
        &Surface {
            rect,
            radius: theme::CONTROL_RADIUS,
            fill,
            edge: Stroke::new(2.0, edge),
            shadow,
            inner_shadow: state.mode == KeycapMode::Pressed,
            ring,
            scale,
        },
        |painter, box_rect| paint_content(painter, box_rect, direction, state, active, ink),
    );

    if hovered {
        paint_edit_badge(&painted.halo, painted.box_rect);
    }
}

/// The D-pad keycap's content: the sub-legend row with its direction glyph,
/// and the hero key name below it.
fn paint_content(
    painter: &Painter,
    box_rect: Rect,
    direction: &Direction,
    state: &Keycap<'_>,
    active: bool,
    ink: Color32,
) {
    let content = box_rect.shrink(PADDING);
    let legend = Rect::from_min_size(content.min, Vec2::new(content.width(), LEGEND_HEIGHT));
    let legend_ink = if active {
        // The legend row while a state is active: white at 80%.
        Color32::from_white_alpha(204)
    } else {
        theme::SLATE_400
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
    theme::stamp_galley(
        painter,
        legend.left_top(),
        &legend_galley,
        legend_ink,
        LEGEND_SIZE,
    );
    let arrow_ink = if active {
        theme::WHITE
    } else {
        direction.accent
    };
    paint_arrow(
        painter,
        Rect::from_center_size(
            Pos2::new(legend.right() - ARROW_SIZE / 2.0, legend.center().y),
            Vec2::splat(ARROW_SIZE),
        ),
        direction.arrow,
        arrow_ink,
    );

    // The hero region, as the reference lays it out: from the legend row's
    // bottom minus the `-mt-1` lift, down to the content box's bottom. The key
    // name centres in *that* region, which is deliberately not the keycap's own
    // centre -- see [`HERO_LIFT`].
    let center = Rect::from_min_max(
        Pos2::new(legend.left(), legend.bottom() - HERO_LIFT),
        content.max,
    )
    .center();
    match state.mode {
        KeycapMode::Rebinding => {
            paint_text_block(
                painter,
                center,
                &["...".to_owned()],
                18.0,
                0.0,
                theme::WHITE,
            );
        }
        _ => {
            let display = format_key_for_display(state.name);
            let gap = if display.lines.len() > 1 { 1.0 } else { 0.0 };
            paint_text_block(painter, center, &display.lines, display.size, gap, ink);
        }
    }
}

/// The scale a mode's painted box settles on: the reference's `scale-105`
/// while rebinding, `scale-95` while a physical key is down, and the allocated
/// box otherwise.
fn painted_scale(mode: KeycapMode) -> f32 {
    match mode {
        KeycapMode::Pressed => PRESS_SCALE,
        KeycapMode::Rebinding => REBIND_SCALE,
        KeycapMode::Normal => 1.0,
    }
}

/// The pressed keycap's inset shadow (`shadow-inner`): the two translucent
/// inside strokes described at [`theme::BLACK_5`], drawn on the compressed box
/// so the band follows the pressed geometry.
pub fn paint_inner_shadow(painter: &Painter, rect: Rect, radius: CornerRadius) {
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(INNER_SHADOW_WIDE, theme::BLACK_5),
        StrokeKind::Inside,
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(INNER_SHADOW_TIGHT, theme::BLACK_5),
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
    painter.add(theme::SHADOW_XS.as_shape(badge, theme::CIRCLE_RADIUS));
    painter.circle_filled(badge.center(), BADGE_SIZE / 2.0, theme::WHITE);
    painter.circle_stroke(
        badge.center(),
        BADGE_SIZE / 2.0 - 0.5,
        Stroke::new(1.0, theme::INDIGO_200),
    );
    theme::paint_icon(
        painter,
        Rect::from_center_size(badge.center(), Vec2::splat(BADGE_ICON)),
        theme::Icon::Edit,
        theme::INDIGO_700,
    );
}

/// Paints one or two centred lines as a block, with the same weight
/// approximation as [`theme::stamp_galley`].
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
        theme::stamp_galley(
            painter,
            Pos2::new(center.x - galley.size().x / 2.0, y),
            galley,
            color,
            size,
        );
        y += galley.size().y + gap;
    }
}

/// The direction arrow: a line-and-chevron glyph (0.15 padding, a 45-degree
/// chevron, round caps and joins), drawn from one coordinate table.
///
/// The window's one owner of the glyph. Every direction chevron on every
/// surface is this -- the D-pad cap's legend, the preview's example keys and
/// game-view indicator, the timeline's gutter caps -- so the four directions
/// stay mirror images of one another and the stroke weight follows the size
/// everywhere rather than being re-tuned per surface.
pub fn paint_arrow(painter: &Painter, rect: Rect, arrow: Arrow, color: Color32) {
    let size = rect.width().min(rect.height());
    let stroke = theme::icon_stroke(size, color);
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
            line(&[(0.57, 0.22), (0.85, 0.5), (0.57, 0.78)]);
        }
    }
}

/// A state's outer glow: a zero-offset shadow in the state's ring colour, at
/// the theme-owned blur for that state.
pub fn glow(color: Color32, blur: u8) -> Shadow {
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
    fn painted_shapes<State>(harness: &Harness<'_, State>) -> Vec<Shape> {
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

    /// Every `Shape::Rect` the frame painted, flattened out of `Shape::Vec`.
    fn painted_rects<State>(harness: &Harness<'_, State>) -> Vec<egui::epaint::RectShape> {
        fn collect(shape: &Shape, out: &mut Vec<egui::epaint::RectShape>) {
            match shape {
                Shape::Rect(rect) => out.push(rect.clone()),
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for shape in &painted_shapes(harness) {
            collect(shape, &mut out);
        }
        out
    }

    /// Every `Shape::Path` the frame painted, flattened out of `Shape::Vec`.
    fn painted_paths<State>(harness: &Harness<'_, State>) -> Vec<egui::epaint::PathShape> {
        fn collect(shape: &Shape, out: &mut Vec<egui::epaint::PathShape>) {
            match shape {
                Shape::Path(path) => out.push(path.clone()),
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for shape in &painted_shapes(harness) {
            collect(shape, &mut out);
        }
        out
    }

    /// One direction's arrow painted alone into a `ARROW_SIZE` square.
    fn render_arrow(arrow: Arrow) -> Harness<'static> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(120.0, 120.0))
            .build_ui(move |ui| {
                let painter = ui.painter().clone();
                paint_arrow(
                    &painter,
                    Rect::from_min_size(Pos2::ZERO, Vec2::splat(ARROW_SIZE)),
                    arrow,
                    theme::INDIGO_600,
                );
            });
        harness.run();
        harness
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

    /// T12: the weight stamp has one owner, `theme::stamp_galley`. The keycap
    /// legend must carry its two passes at the shared offset; a local copy
    /// with its own geometry would break this.
    #[test]
    fn the_legend_stamp_uses_the_shared_theme_offset() {
        let harness = render_keycap(KeycapMode::Normal);
        let mut passes: Vec<f32> = Vec::new();
        for shape in painted_shapes(&harness) {
            if let Shape::Text(text) = shape
                && text.galley.text() == "W"
            {
                passes.push(text.pos.x);
            }
        }
        passes.sort_by(f32::total_cmp);
        assert_eq!(passes.len(), 2, "the legend carries the two stamp passes");
        let expected = (18.0 * theme::STAMP_OFFSET_FACTOR).max(theme::STAMP_OFFSET_MIN);
        assert!(
            (passes[1] - passes[0] - expected).abs() < 1e-3,
            "the second pass sits at the shared offset: {passes:?} vs {expected}"
        );
    }

    #[test]
    fn directions_keep_the_reference_accents() {
        assert_eq!(UP.accent, theme::BLUE_600);
        assert_eq!(LEFT.accent, theme::INDIGO_600);
        assert_eq!(RIGHT.accent, theme::PURPLE_600);
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
    fn direction_accents_are_the_mode_bases() {
        // Every direction reads its mode's Tailwind `-600` step, which is both
        // the resting arrow's ink and the held fill, so the two are one value.
        let channels = |color: Color32| [color.r(), color.g(), color.b()];
        assert_eq!(channels(UP.accent), [0x15, 0x5d, 0xfc]);
        assert_eq!(channels(LEFT.accent), [0x4f, 0x39, 0xf6]);
        assert_eq!(channels(RIGHT.accent), [0x98, 0x10, 0xfa]);
        assert_eq!(channels(DOWN.accent), [0x7f, 0x22, 0xfe]);
    }

    /// The direction table is the window's one colour source for a direction:
    /// the preview's example keys read [`LEFT`] and [`RIGHT`] rather than
    /// carrying a second copy of indigo and purple, so every row must name a
    /// complete colour set and its own slot.
    #[test]
    fn every_direction_row_is_a_complete_colour_set() {
        let rows = [&UP, &DOWN, &LEFT, &RIGHT];
        for (index, row) in rows.iter().enumerate() {
            for (other_index, other) in rows.iter().enumerate() {
                if other_index == index {
                    continue;
                }
                assert_ne!(
                    row.accent, other.accent,
                    "{}: accents must differ",
                    row.label
                );
                assert_ne!(row.edge, other.edge, "{}: edges must differ", row.label);
                assert_ne!(row.ring, other.ring, "{}: rings must differ", row.label);
                assert_ne!(row.arrow, other.arrow, "{}: arrows must differ", row.label);
                assert_ne!(row.slot, other.slot, "{}: slots must differ", row.label);
            }
            // The edge is the accent's own ramp, one step darker, so a row can
            // never pair indigo with violet.
            assert_ne!(
                row.edge, row.accent,
                "{}: the edge is one step darker",
                row.label
            );
        }
    }

    #[test]
    fn the_pressed_box_deflates_to_scale_95() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::splat(KEYCAP_SIZE));
        let pressed = Rect::from_center_size(
            rect.center(),
            rect.size() * painted_scale(KeycapMode::Pressed),
        );
        assert!((pressed.width() - 76.0).abs() < 0.01);
        assert!((pressed.height() - 76.0).abs() < 0.01);
        assert!((pressed.center() - rect.center()).length() < 0.01);
        // The compression is paint-time only: normal keeps the allocated box,
        // so the D-pad grid never reflows.
        assert_eq!(painted_scale(KeycapMode::Normal), 1.0);
        // The reference lifts a rebinding keycap to `scale-105` instead.
        assert!((painted_scale(KeycapMode::Rebinding) - 1.05).abs() < 0.001);
        // The lift grows the box where the press shrinks it, so the two
        // states cannot be confused for one another.
        assert!(painted_scale(KeycapMode::Rebinding) > 1.0);
        assert!(painted_scale(KeycapMode::Pressed) < 1.0);
    }

    /// The reference's press is a `transition-all duration-75`, so the
    /// compression must glide: the frame a press starts still paints the
    /// resting box, and only later frames paint the `scale-95` one. A version
    /// that snapped would paint the deflated box on the press frame itself.
    ///
    /// The harness's default `step_dt` is 250 ms -- more than three times the
    /// transition -- so this drives it at 10 ms to resolve the curve.
    #[test]
    fn the_press_compression_glides_over_the_reference_duration() {
        /// The state the closure toggles, so a press can start mid-test.
        struct Held(bool);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 200.0))
            .with_step_dt(0.01)
            .build_ui_state(
                |ui, held: &mut Held| {
                    keycap(
                        ui,
                        &UP,
                        Keycap {
                            name: "W",
                            mode: if held.0 {
                                KeycapMode::Pressed
                            } else {
                                KeycapMode::Normal
                            },
                            duplicate: false,
                        },
                    );
                },
                Held(false),
            );

        let painted_width = |harness: &Harness<'_, Held>| {
            painted_shapes(harness)
                .iter()
                .find_map(|shape| match shape {
                    Shape::Rect(rect_shape) if rect_shape.fill == UP.accent => {
                        Some(rect_shape.rect.width())
                    }
                    _ => None,
                })
                .expect("the pressed fill is the direction accent")
        };

        // Resting: the fill is white, so there is no accent box at all.
        harness.run();
        assert!(
            !painted_shapes(&harness).iter().any(|shape| matches!(
                shape,
                Shape::Rect(rect_shape) if rect_shape.fill == UP.accent
            )),
            "a resting keycap paints no accent box"
        );

        // Press it. The frame the segment starts still shows the resting box,
        // so the accent appears at the full allocated width first.
        harness.state_mut().0 = true;
        harness.step();
        let mut widths = Vec::new();
        for _ in 0..10 {
            harness.step();
            widths.push(painted_width(&harness));
        }

        let first = widths[0];
        assert!(
            (first - KEYCAP_SIZE).abs() < 0.5,
            "the press frame shows the resting size before the transition runs: {first}"
        );
        let last = *widths.last().unwrap();
        assert!(
            (last - KEYCAP_SIZE * PRESS_SCALE).abs() < 0.01,
            "the transition settles on the pressed scale: {last}"
        );
        assert!(
            widths
                .iter()
                .any(|w| (w - KEYCAP_SIZE * PRESS_SCALE).abs() > 0.5
                    && (*w - KEYCAP_SIZE).abs() > 0.5),
            "some frame must show an intermediate width, not a snap: {widths:?}"
        );
        // Monotonic: the compression never reverses while the key is held.
        for pair in widths.windows(2) {
            assert!(
                pair[1] <= pair[0] + 0.01,
                "the compression must not reverse: {widths:?}"
            );
        }
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
                Shape::Rect(rect_shape) if rect_shape.stroke.color == theme::BLACK_5 => {
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
    fn the_four_arrows_are_mirror_images() {
        // Every direction is the same glyph turned a quarter turn: a shaft
        // spanning the 0.15 paddings, and a 45-degree chevron whose arms reach
        // 0.28 of the glyph back along the shaft and 0.28 of it sideways.
        //
        // The chevron's reach is measured along each direction's own axis
        // rather than read off a table, so a coordinate that is right for
        // three directions and wrong for the fourth -- a narrower head on one
        // arrow than on its mirror -- fails here.
        for (arrow, axis, normal) in [
            (Arrow::Up, Vec2::new(0.0, -1.0), Vec2::new(1.0, 0.0)),
            (Arrow::Down, Vec2::new(0.0, 1.0), Vec2::new(1.0, 0.0)),
            (Arrow::Left, Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0)),
            (Arrow::Right, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)),
        ] {
            let harness = render_arrow(arrow);
            let paths: Vec<egui::epaint::PathShape> = painted_paths(&harness)
                .into_iter()
                .filter(|path| solid_color(&path.stroke.color) == Some(theme::INDIGO_600))
                .collect();
            assert_eq!(paths.len(), 2, "{arrow:?}: a shaft and a chevron");
            let shaft = paths
                .iter()
                .find(|path| path.points.len() == 2)
                .expect("the shaft is the two-point path");
            let chevron = paths
                .iter()
                .find(|path| path.points.len() == 3)
                .expect("the chevron is the three-point path");
            let tip = chevron.points[1];
            assert_eq!(tip, shaft.points[1], "{arrow:?}: the chevron meets the tip");
            let reach = |from: Pos2| ((tip - from).dot(axis).abs(), (tip - from).dot(normal).abs());
            for wing in [chevron.points[0], chevron.points[2]] {
                let (back, side) = reach(wing);
                assert!(
                    (back - 0.28 * ARROW_SIZE).abs() < 0.01,
                    "{arrow:?}: the chevron arm reaches {back} back, not {}",
                    0.28 * ARROW_SIZE
                );
                assert!(
                    (side - 0.28 * ARROW_SIZE).abs() < 0.01,
                    "{arrow:?}: the chevron arm sits {side} to the side, not {}",
                    0.28 * ARROW_SIZE
                );
            }
            assert!(
                ((tip - shaft.points[0]).length() - 0.7 * ARROW_SIZE).abs() < 0.01,
                "{arrow:?}: the shaft spans the 0.15 paddings"
            );
        }
    }

    #[test]
    fn key_lines_is_the_windows_one_splitter() {
        // Every surface that draws a key name reads its lines from here -- the
        // D-pad cap, the preview's example keys, and the timeline's gutter
        // caps -- so these are the lines all three draw for a given key.
        for (name, expected) in [
            ("Arrow Up", vec!["ARROW", "UP"]),
            ("Numpad 8", vec!["NUMPAD", "8"]),
            ("Num 5", vec!["NUM", "5"]),
            ("Page Down", vec!["PAGE", "DOWN"]),
            ("Backspace", vec!["BACK", "SPACE"]),
            ("CapsLock", vec!["CAPS", "LOCK"]),
            ("Left Ctrl", vec!["LEFT", "CTRL"]),
            ("Right Shift", vec!["RIGHT", "SHIFT"]),
            ("Ctrl Shift", vec!["CTRL", "SHIFT"]),
            ("A B C", vec!["A", "B C"]),
            ("W", vec!["W"]),
            ("F13", vec!["F13"]),
            ("", vec!["-"]),
        ] {
            assert_eq!(key_lines(name), expected, "{name:?}");
        }
    }

    #[test]
    fn the_dpad_cap_paints_its_ring_in_the_active_states() {
        // The reference's `ring-2` while pressed and `ring-4` while rebinding,
        // in the direction's own ring step -- the same effect the preview and
        // timeline caps take, and the hard edge inside the glow.
        let resting = render_keycap(KeycapMode::Normal);
        assert!(
            !painted_rects(&resting)
                .iter()
                .any(|rect| rect.stroke.color == UP.ring),
            "a resting cap paints no ring"
        );
        for (mode, width) in [
            (KeycapMode::Pressed, theme::KEYCAP_RING_PRESSED),
            (KeycapMode::Rebinding, theme::KEYCAP_RING_REBIND),
        ] {
            let harness = render_keycap(mode);
            // The ring belongs to the same element as the box, so the mode's
            // scale compresses it too: the pressed `ring-2` measures 1.9px and
            // the rebinding `ring-4` measures 4.2px.
            let scaled = width * painted_scale(mode);
            assert!(
                painted_rects(&harness).iter().any(|rect| {
                    rect.stroke.color == UP.ring
                        && (rect.stroke.width - scaled).abs() < 0.01
                        && rect.stroke_kind == StrokeKind::Outside
                }),
                "{mode:?}: the cap paints its {scaled}px ring in the direction's ring step"
            );
        }
    }

    #[test]
    fn the_arrow_keeps_the_canvas_icon_geometry_with_round_caps() {
        let harness = render_keycap(KeycapMode::Normal);
        let shapes = painted_shapes(&harness);

        let arrow: Vec<egui::epaint::PathShape> = shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Path(path) if solid_color(&path.stroke.color) == Some(UP.accent) => {
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

        // Round line caps; epaint has no cap option, so every vertex carries a
        // disc of the stroke's radius.
        let caps: Vec<egui::epaint::CircleShape> = shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Circle(circle) if circle.fill == UP.accent => Some(*circle),
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
                    if circle.fill == theme::WHITE
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
                if solid_color(&path.stroke.color) == Some(theme::INDIGO_700))),
            "the badge carries the drawn pencil"
        );
    }
}
