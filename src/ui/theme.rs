//! Palette and widget tokens for the egui settings window.
//!
//! Ported from `iced-ui/theme.rs` (the Iced specification) with names and
//! values preserved so reviewers can diff the two files. The behavioural
//! contract is `docs/architecture/ui.md`; nothing here changes what the UI
//! does, only how its tokens are expressed.
//!
//! # Porting deltas (values unchanged, representation adapted)
//!
//! - Colours are [`egui::Color32`], not Iced's `f32` `Color`. Every channel
//!   is the same 8-bit value the `rgb()` helper produced; the f32 alphas the
//!   Iced file attached to a few literals (shadows, washes, the selection
//!   tint) are quantized to their nearest byte (`0.04 -> 10`, `0.05 -> 13`,
//!   `0.06 -> 15`, `0.12 -> 31`, `0.15 -> 38`, `0.25 -> 64`, `0.4 -> 102`,
//!   `0.7 -> 179`, `0.9 -> 230`, `0.95 -> 242`). The screen only ever showed
//!   8 bits of them.
//! - Iced's `Padding` is `egui::epaint::MarginF32` here (egui 0.36 removed
//!   `Padding`); the field names are the same and every value is identical.
//!   Frame margins in egui are i8-quantized at the call site; the tokens stay
//!   f32 so a `3.5` chip inset does not silently round in this file.
//! - Corner radii are [`egui::CornerRadius`] (u8). The Iced file's two
//!   "fully round" literals (`999.0`, `9999.0`) become [`CIRCLE_RADIUS`]:
//!   egui clamps a radius to half the box, so the maximum byte draws the same
//!   circle.
//! - Fonts: egui selects a face by [`egui::FontFamily`] and registered
//!   variant, and eframe's `default_fonts` ships no bold/semibold/italic
//!   faces, so `UI_FONT_BOLD`, `UI_FONT_SEMIBOLD`, `UI_FONT_ITALIC`,
//!   `UI_FONT_BLACK` and `CHIP_FONT` have no direct counterpart. The generic
//!   families they were built from stay here ([`UI_FONT`], [`MONO_FONT`]);
//!   emphasis is a size/colour decision at the call site until a weighted
//!   face is registered, which is a behaviour question to raise, not to fix
//!   in passing (migration constraint 5).
//! - The Iced file's style *closures* (`primary_button`, `keycap`,
//!   `accent_slider`, ...) fed Iced's per-widget style hook. egui styles
//!   widgets at the call site from `Style`/`Visuals` plus per-widget builders,
//!   so the closures are not ported as functions; their state-independent
//!   values are exposed here as the constants and [`egui::Frame`] builders
//!   the view modules compose. Each one's disposition is noted at its old
//!   name below. One exception, assigned by the Master after R2 round 2
//!   issue 2: [`secondary_button`] and the [`Icon`]/[`paint_icon`] set it
//!   needs are ported here as a shared paint-only control, so the outlined
//!   secondary action both cards render has one owner. Consumer call sites
//!   migrate to it in their own wave; until then the mapping module's local
//!   copy is what it draws with, and the two must not diverge.
//! - Constants the Iced file kept private are public here: with the style
//!   closures gone, the view modules are the consumers, and re-inlining a
//!   hex there would duplicate the contract this file owns.
//! - A few tokens the Iced spec held inside *widget* code rather than
//!   `theme.rs` (`iced-ui/widgets.rs`, `iced-ui/icons.rs`) are canonicalised
//!   here too, each citing its source line, so the view modules import one
//!   named value instead of repeating the literal per call site.

use crate::settings::SocdMode;
use egui::epaint::MarginF32;
use egui::{
    Color32, CornerRadius, FontId, Frame, Galley, Painter, Pos2, Rect, Response, Sense, Shadow,
    Shape, Stroke, StrokeKind, TextStyle, Ui, Vec2, WidgetInfo, WidgetType,
};
use std::f32::consts::PI;
use std::sync::Arc;

const fn rgb(red: u8, green: u8, blue: u8) -> Color32 {
    Color32::from_rgb(red, green, blue)
}

/// Straight-alpha restatement of an opaque colour: Iced stored `a` as an
/// f32 on the same struct, egui keeps it as the fourth byte.
const fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(color.r(), color.g(), color.b(), alpha)
}

pub const CANVAS: Color32 = Color32::WHITE;
pub const SURFACE: Color32 = Color32::WHITE;
pub const INSET: Color32 = rgb(0xf9, 0xfb, 0xfd);
/// `border-slate-200`. The reference draws every neutral hairline and every
/// outlined control edge with this one class, so the card frame, the control
/// outline, and the dropdown border read it instead of keeping three
/// near-duplicate slates that drift apart.
pub const BORDER: Color32 = rgb(0xe2, 0xe8, 0xf0);
/// `text-slate-900`.
pub const BODY_TEXT: Color32 = rgb(0x0f, 0x17, 0x2b);
/// `text-slate-500`.
pub const MUTED_TEXT: Color32 = rgb(0x62, 0x74, 0x8e);
/// `text-amber-600`; the fill of an amber control is the same class colour.
pub const WARN_TEXT: Color32 = rgb(0xe1, 0x71, 0x00);
pub const AMBER_BUTTON: Color32 = rgb(0xe1, 0x71, 0x00);
/// `bg-amber-700` / `border-amber-700`, and the dirty badge's ink.
pub const AMBER_DARK: Color32 = rgb(0xbb, 0x4d, 0x00);
pub const RELEASE_TEXT: Color32 = rgb(0x8b, 0x5c, 0xf6);
pub const MIX_TEXT: Color32 = rgb(0x6d, 0x51, 0xee);

// Two colour lineages. The reference styles the DOM with Tailwind classes but
// passes raw hex to its icon and canvas components. Tailwind v4 re-specified
// the scale in oklch, so a class and the v3 hex of the same name no longer
// agree: beside `bg-indigo-600` (`#4f39f6`) sits `CanvasIcon color="#4f46e5"`,
// indigo-600 as v3 defined it. Nothing the reference paints with a class
// should take a drawn constant, and nothing it draws should take a DOM one.
/// Drawn indigo-600 (reference literal `#4f46e5`): icon strokes and fills.
pub const PRIMARY_TEXT: Color32 = rgb(0x4f, 0x46, 0xe5);
/// `bg-indigo-50`: the panel close button's hover wash, and the base of the
/// Press Delay slot card. The rest of the indigo and violet ramps follow, for
/// the two delay-mode cards.
///
/// Tailwind v4 re-specified its scale in oklch, so these are the values the
/// reference actually renders, read back from its own computed styles rather
/// than the v3 hexes that share the names. Only the steps the slot cards use
/// are listed; the middle steps were read back from a rendered card edge, which
/// is the only place the reference paints them.
pub const INDIGO_50: Color32 = rgb(0xee, 0xf2, 0xff);
pub const INDIGO_100: Color32 = rgb(0xe0, 0xe7, 0xff);
pub const INDIGO_200: Color32 = rgb(0xc6, 0xd2, 0xff);
pub const INDIGO_300: Color32 = rgb(0xa3, 0xb3, 0xff);
pub const INDIGO_400: Color32 = rgb(0x7c, 0x86, 0xff);
pub const VIOLET_50: Color32 = rgb(0xf5, 0xf3, 0xff);
pub const VIOLET_100: Color32 = rgb(0xed, 0xe9, 0xfe);
pub const VIOLET_200: Color32 = rgb(0xdd, 0xd6, 0xff);
pub const VIOLET_300: Color32 = rgb(0xc4, 0xb3, 0xff);
pub const VIOLET_400: Color32 = rgb(0xa6, 0x84, 0xff);
/// `bg-indigo-600` / `text-indigo-600` / `border-indigo-600`.
pub const INDIGO_600: Color32 = rgb(0x4f, 0x39, 0xf6);
/// `bg-indigo-700` / `hover:bg-indigo-700` / `border-indigo-700`.
pub const INDIGO_700: Color32 = rgb(0x43, 0x2d, 0xd7);
/// `bg-indigo-800` / `hover:bg-indigo-800`.
pub const INDIGO_800: Color32 = rgb(0x37, 0x30, 0xa3);
/// `bg-emerald-500`: the connected status dot.
pub const EMERALD_500: Color32 = rgb(0x00, 0xbc, 0x7d);
/// `text-emerald-600`: notices and the latency table's median figures.
pub const EMERALD_600: Color32 = rgb(0x00, 0x99, 0x66);
/// `text-emerald-700`: the "all keys unique" footer.
pub const EMERALD_700: Color32 = rgb(0x00, 0x7a, 0x55);
/// `text-emerald-600/80`: the synchronized badge.
pub const EMERALD_SYNC: Color32 = rgb(0x33, 0xad, 0x84);
/// `bg-amber-500`: the physical-overlap legend dot.
pub const AMBER_500: Color32 = rgb(0xfe, 0x9a, 0x00);
/// `bg-red-500`: the near-simultaneous legend dot.
pub const RED_500: Color32 = rgb(0xfb, 0x2c, 0x36);
/// `text-red-600`: error and validation copy.
pub const RED_600: Color32 = rgb(0xe7, 0x00, 0x0b);
/// `bg-slate-100`: slider rails and the step-badge well.
pub const SLATE_100: Color32 = rgb(0xf1, 0xf5, 0xf9);
/// The Immediate accent. The reference hardcodes `#3a55e8` (its own blend of
/// blue-600 and indigo-600) to mark the only delay-free mode.
pub const IMMEDIATE_ACCENT: Color32 = rgb(0x3a, 0x55, 0xe8);
/// `bg-purple-600`: the preview's D keycap, the one place the reference uses
/// purple rather than the release delay's violet.
pub const PURPLE_600: Color32 = rgb(0x98, 0x10, 0xfa);
/// `bg-violet-500`: the preview's overlap dash.
pub const VIOLET_500: Color32 = rgb(0x8b, 0x5c, 0xf6);
/// `text-violet-600`: the release-delay range label.
pub const VIOLET_600: Color32 = rgb(0x7f, 0x22, 0xfe);
/// Drawn emerald-500 (reference literal `#10b981`) for check and warning
/// glyphs, which the reference colours with hex rather than a class.
pub const OK_TEXT: Color32 = rgb(0x10, 0xb9, 0x81);
/// Drawn red-600 (reference literal `#dc2626`) for warning glyphs.
pub const ERROR_TEXT: Color32 = rgb(0xdc, 0x26, 0x26);
/// Drawn green-600 (reference literal `#16a34a`), the unique-assignment check.
pub const GREEN_CHECK: Color32 = rgb(0x16, 0xa3, 0x4a);
/// Muted icon ink from the reference (`#94a3b8` for idle chevrons and the
/// power-off state, `#475569` for restore affordances). Text keeps
/// [`MUTED_TEXT`]; icons and the keycap sub-legends take this slate-400 so
/// they do not render darker than drawn.
pub const ICON_MUTED: Color32 = rgb(0x94, 0xa3, 0xb8);
pub const ICON_SECONDARY: Color32 = rgb(0x47, 0x55, 0x69);
/// `text-violet-700` (release-delay mode label). The card tint keeps
/// [`RELEASE_TEXT`]; only the label uses this darker ink.
pub const RELEASE_LABEL: Color32 = rgb(0x70, 0x08, 0xe7);
/// `text-slate-700` (keycap chip text). Chips stay neutral white so the
/// card's mode tint is the only color signal.
pub const CHIP_TEXT: Color32 = rgb(0x31, 0x41, 0x58);
/// `border-slate-300`: keycap chips, the slot confirm overlay, and the
/// disabled outline. The reference reuses the one class for all three.
pub const SLATE_300: Color32 = rgb(0xca, 0xd5, 0xe2);
/// `bg-slate-600` at full strength: the ink the reference's icons carry.
pub const SLATE_600: Color32 = rgb(0x45, 0x55, 0x6c);
/// `hover:border-indigo-300` on outlined controls and the profile name box.
pub const NAME_HOVER_BORDER: Color32 = rgb(0xa3, 0xb3, 0xff);

/// `hover:bg-indigo-50/60` on the profile name box, composited over the panel
/// the way [`over`] does for the cards: a browser blends an alpha in sRGB and
/// would land paler than a raw alpha channel resolves here.
pub const NAME_HOVER_FILL: Color32 = over(INDIGO_50, 0.60, SURFACE);
/// `bg-red-50` and `border-red-400` for invalid values and duplicate keys.
pub const ERROR_BG: Color32 = rgb(0xfe, 0xf2, 0xf2);
pub const ERROR_BORDER: Color32 = rgb(0xff, 0x64, 0x67);

// Values the Iced file held inside its style closures, kept as named tokens
// so the view modules compose them instead of re-inlining hexes.

/// The main card's edge (`border-indigo-200/80` as the reference draws it),
/// distinct from the neutral [`BORDER`] hairline of the inset frames.
pub const CARD_BORDER: Color32 = rgb(0xd2, 0xdb, 0xff);
/// The hover wash the outlined controls share (`hover:bg-indigo-50/60`
/// composited, which the reference's own edge read-back rounds to `#f5f7ff`):
/// secondary buttons, mode segments, nav buttons, the preview pill, and the
/// language rows all take it.
pub const HOVER_WASH: Color32 = rgb(0xf5, 0xf7, 0xff);
/// Keycap hover fill (`hover:bg-slate-50`).
pub const KEYCAP_HOVER_BG: Color32 = rgb(0xf8, 0xfa, 0xfc);
/// Keycap hover edge (`hover:border-indigo-400` as drawn).
pub const KEYCAP_HOVER_BORDER: Color32 = rgb(0x81, 0x8c, 0xf8);
/// The rebinding keycap's white inner ring (`rgba(255,255,255,0.4)`).
pub const KEYCAP_REBIND_EDGE: Color32 = with_alpha(Color32::WHITE, 102);
/// The selected language row's fill and hover fill (`bg-indigo-50/80` and
/// `hover:bg-indigo-100/80` as the reference renders them); its edge is
/// [`INDIGO_200`].
pub const ACTIVE_OPTION_BG: Color32 = rgb(0xf1, 0xf5, 0xff);
pub const ACTIVE_OPTION_HOVER_BG: Color32 = rgb(0xe6, 0xeb, 0xff);
/// Text selection over an accent (`Color { a: 0.25, ..INDIGO_600 }`).
pub const TEXT_SELECTION: Color32 = with_alpha(INDIGO_600, 64);
/// The dirty badge's wash and edge (`bg-amber-50` at 0.7, `border-amber-200`
/// at 0.9).
pub const DIRTY_BADGE_BG: Color32 = with_alpha(rgb(0xff, 0xfc, 0xf1), 179);
pub const DIRTY_BADGE_EDGE: Color32 = with_alpha(rgb(0xfe, 0xe8, 0x91), 230);
/// The confirm overlay's wash (`bg-white/95`).
pub const CONFIRM_OVERLAY_FILL: Color32 = with_alpha(SURFACE, 242);

/// System UI face, left generic on purpose: [`egui::FontFamily::Proportional`]
/// resolves whatever the platform context calls its default sans and walks its
/// fallback chain for glyphs that face lacks. The Iced file's reasoning for
/// not naming a family applies unchanged.
pub const UI_FONT: egui::FontFamily = egui::FontFamily::Proportional;
/// Generic monospace, for the profile slot keycaps. The reference puts its
/// `font-code` stack on every `<kbd>` (`[&_kbd]:font-code` on `<body>`), which
/// is what keeps all four chips the same width.
pub const MONO_FONT: egui::FontFamily = egui::FontFamily::Monospace;

/// Body text size from the preview; headings sit just above it.
pub const BODY_TEXT_SIZE: f32 = 13.0;
pub const HEADING_SIZE: f32 = 15.0;

pub const PAGE_PADDING: f32 = 16.0;
pub const SECTION_GAP: f32 = 16.0;
pub const ROW_GAP: f32 = 8.0;
pub const CARD_PADDING: f32 = 20.0;
pub const GROUP_PADDING: f32 = 14.0;

// Control padding. The reference sizes controls with CSS classes under
// `box-sizing: border-box` (`px-3 py-1.5` on outlined actions, `px-4 py-1.5`
// on filled ones), so its padding starts *inside* the 1px border and a
// standard action measures 30px tall: 16px line + 2*6 padding + 2*1 border.
//
// The `+ 1.0` in these constants encodes the same correction the Iced file
// derived for its layout: a control's drawn box is `content + 2 * padding`,
// and the edge sits inside it. egui's own widget margins are smaller by
// default, so the view modules pass these as explicit button padding.

/// Outlined and filled action controls: `py-1.5 px-3` plus the 1px edge.
pub const BUTTON_PADDING: MarginF32 = MarginF32 {
    left: 13.0,
    right: 13.0,
    top: 7.0,
    bottom: 7.0,
};
/// Filled primary controls, which the reference widens to `px-4`.
pub const BUTTON_PADDING_WIDE: MarginF32 = MarginF32 {
    left: 17.0,
    right: 17.0,
    top: 7.0,
    bottom: 7.0,
};
/// Icon-only header buttons. The reference writes `px-2.5 py-1.5` plus an
/// explicit `h-[29px]`, so the height comes from that literal rather than from
/// padding: `py-1.5` plus the 1px edge would give 30px, and the reference
/// clips two of them back off. Match the literal, not the padding sum.
pub const HEADER_ICON_PADDING: MarginF32 = MarginF32 {
    left: 11.0,
    right: 11.0,
    top: 7.0,
    bottom: 7.0,
};
pub const HEADER_ICON_HEIGHT: f32 = 29.0;
/// Mode segments. `py-1.5 px-2` with no border, so nothing is added for an
/// edge and the segment is 28px tall in the reference.
pub const MODE_PADDING: MarginF32 = MarginF32 {
    left: 8.0,
    right: 8.0,
    top: 7.0,
    bottom: 7.0,
};

// Profile slot panel. The `+ 1` reading the Iced file derived for a nested
// box applies the same way here: the reference's card is `box-sizing:
// border-box`, so its padding and its 1px border both sit inside the box it
// measures.

/// Panel width (reference `w-96`), and the width of the language menu
/// (reference `w-48`).
pub const PROFILE_PANEL_WIDTH: f32 = 384.0;
pub const LANGUAGE_PANEL_WIDTH: f32 = 192.0;
/// The panel's own inset, standing in for the border it strokes inside itself.
pub const PANEL_PADDING: MarginF32 = MarginF32 {
    left: 1.0,
    right: 1.0,
    top: 1.0,
    bottom: 1.0,
};
/// Header block inset (reference `px-4 pt-4 pb-2` on the slot dialog).
pub const PROFILE_HEADER_PADDING: MarginF32 = MarginF32 {
    left: 16.0,
    right: 16.0,
    top: 16.0,
    bottom: 8.0,
};
/// Card scroller inset (reference `p-2.5`), and the tighter `p-1.5` the
/// reference uses around the language rows.
pub const PROFILE_SCROLLER_PADDING: MarginF32 = MarginF32 {
    left: 10.0,
    right: 10.0,
    top: 10.0,
    bottom: 10.0,
};
pub const LANGUAGE_SCROLLER_PADDING: MarginF32 = MarginF32 {
    left: 6.0,
    right: 6.0,
    top: 6.0,
    bottom: 6.0,
};
/// Gap between slot cards (reference `space-y-1.5`) and between language rows
/// (`space-y-0.5`).
pub const SLOT_GAP: f32 = 6.0;
pub const LANGUAGE_ROW_GAP: f32 = 2.0;
/// Between a card's name row and its keycap row (reference `mt-2.5`). The
/// reference's 10 would grow the card by 4px with a square chip, so 2 of those
/// pixels move here and the card grows by 2.
pub const SLOT_ROW_GAP: f32 = 8.0;
/// Slot card inset: `p-3` plus the 1px edge the reference counts inside it.
pub const SLOT_CARD_PADDING: f32 = 12.0 + 1.0;
/// Slot name box: reference `pl-2 pr-1 py-1` plus the same 1px edge, so the
/// padding is asymmetric -- the pencil side is tighter than the text side.
pub const SLOT_NAME_PADDING: MarginF32 = MarginF32 {
    left: 9.0,
    right: 5.0,
    top: 5.0,
    bottom: 5.0,
};
/// Between the name and its pencil (reference `gap-1`).
pub const SLOT_NAME_GAP: f32 = 4.0;
/// The rename box that replaces the name box in place: reference `px-2 py-1`
/// plus the same 1px edge, symmetric because a field has no trailing icon. The
/// left inset matches the name box's, so the name itself does not move when the
/// box becomes editable, and the box takes the text's own width rather than
/// filling the row, so it grows to the right as the name is typed.
pub const SLOT_NAME_INPUT_PADDING: MarginF32 = MarginF32 {
    left: 9.0,
    right: 9.0,
    top: 5.0,
    bottom: 5.0,
};
/// Heading block: the title row and its subtitle are `gap-1`, tighter than the
/// `gap-2` inside the title row itself.
pub const PROFILE_HEADER_GAP: f32 = 4.0;
/// Keycap chip: a square box holding one 10px label. The box is pinned rather
/// than derived from its padding because the reference's `leading-none` line is
/// shorter than the line box the shaper gives the same label, and only the box
/// decides the drawn size.
pub const CHIP_SIZE: f32 = 20.0;
/// Between the two chips of one axis pair (reference `gap-1`). Two pixels wider
/// than the reference because the square chip is 4px taller, and the pair needs
/// that much air to keep reading as two separate keys.
pub const CHIP_GAP: f32 = 6.0;
/// Chip inset. Both pairs are symmetric, and that symmetry is what puts the
/// label on the chip's centre line. The vertical insets are
/// `(CHIP_SIZE - 13) / 2`, thirteen being the 10px label's 1.3 line box. That
/// half pixel is deliberate: it centres the *line box*, which is what can be
/// positioned -- the glyph ink inside it is a font metric with no API. Verify
/// again if the chip font changes.
pub const CHIP_PADDING: MarginF32 = MarginF32 {
    left: 7.0,
    right: 7.0,
    top: 3.5,
    bottom: 3.5,
};
/// The chip's floor applies to its *content*: this is the square minus both
/// horizontal insets, and it is what makes a single-glyph chip exactly
/// [`CHIP_SIZE`] wide while a longer label -- the computed `SC:xx` fallback for
/// a key the wire does not name -- grows its own chip sideways. The height
/// stays [`CHIP_SIZE`] for every chip, so one long label cannot make the row
/// taller.
pub const CHIP_CONTENT_MIN: f32 = CHIP_SIZE - CHIP_PADDING.left - CHIP_PADDING.right;

/// Corner radii, named after the controls that take them. The Iced file
/// carried these inside its style closures; egui needs them at the call site.
///
/// [`CIRCLE_RADIUS`] stands for the reference's `rounded-full`: egui clamps a
/// corner radius to half the box, so the maximum representable byte draws the
/// same circle the Iced file's `999.0` and `9999.0` did.
pub const CARD_RADIUS: CornerRadius = CornerRadius::same(16);
pub const GROUP_RADIUS: CornerRadius = CornerRadius::same(12);
pub const CONTROL_RADIUS: CornerRadius = CornerRadius::same(12);
pub const SEGMENT_RADIUS: CornerRadius = CornerRadius::same(8);
pub const PILL_RADIUS: CornerRadius = CornerRadius::same(16);
pub const CHIP_RADIUS: CornerRadius = CornerRadius::same(6);
pub const BADGE_RADIUS: CornerRadius = CornerRadius::same(4);
pub const DOT_RADIUS: CornerRadius = CornerRadius::same(3);
pub const CIRCLE_RADIUS: CornerRadius = CornerRadius::same(255);

/// Slider geometry from the Iced `accent_slider`: a 10pt rail rounded to 5, a
/// hollow handle that swells from 7 to 8 while hovered or grabbed, ringed 3pt
/// in the accent.
pub const SLIDER_RAIL_WIDTH: f32 = 10.0;
pub const SLIDER_RAIL_RADIUS: CornerRadius = CornerRadius::same(5);
pub const SLIDER_HANDLE_RADIUS: f32 = 7.0;
pub const SLIDER_HANDLE_RADIUS_DRAG: f32 = 8.0;
pub const SLIDER_HANDLE_BORDER: f32 = 3.0;
/// The range slider's disabled ink (`iced-ui/widgets.rs:234`,
/// `Color::from_rgb8(203, 213, 225)`), which replaces the per-mode accent on
/// both the filled rail segment and the thumb ring while the slider is
/// disabled (`iced-ui/widgets.rs:235`). Deliberately NOT [`SLATE_300`]
/// (`#cad5e2`, from the Iced theme): the two are distinct source colours one
/// byte apart, and conflating them recolours the disabled rail (R2 finding
/// 4). The rail's base track stays [`SLATE_100`]
/// (`iced-ui/widgets.rs:242`).
pub const SLIDER_RAIL_DISABLED: Color32 = rgb(203, 213, 225);

// The D-pad centre tile's dots and glow (`iced-ui/icons.rs`, `DpadTile`). The
// tile's fill, edge, and guide ring are already canonical here
// ([`SLATE_100`], [`BORDER`]); these are the values the Iced widget painted
// inline.

/// The resting centre guide dot (`iced-ui/icons.rs:215`,
/// `Color::from_rgba(0.80, 0.84, 0.88, 0.6)`). The Iced comment calls it
/// "slate-300/60", but its decimals resolve to `#ccd6e0` at 60% -- not
/// [`SLATE_300`] `#cad5e2`; the rendered bytes are kept.
pub const DPAD_GUIDE_DOT: Color32 = Color32::from_rgba_unmultiplied_const(204, 214, 224, 153);
/// The 24px halo behind the active dot (`iced-ui/icons.rs:224`,
/// `Color::from_rgba(0.23, 0.33, 0.91, 0.25)`). The Iced source wrote the
/// glow's base as rounded f32 decimals of the Immediate accent, so its bytes
/// are `#3b54e8` -- one LSB off [`IMMEDIATE_ACCENT`]'s `#3a55e8` in two
/// channels. The port keeps what the reference rendered rather than
/// "correcting" it (migration constraint 5); the near-miss is a finding, not
/// a licence.
pub const DPAD_ACTIVE_GLOW: Color32 = Color32::from_rgba_unmultiplied_const(59, 84, 232, 64);
/// The shadow under the active dot, one pixel below it
/// (`iced-ui/icons.rs:229`, same `#3b54e8` base at 0.30 -> 77).
pub const DPAD_ACTIVE_DOT_SHADOW: Color32 = Color32::from_rgba_unmultiplied_const(59, 84, 232, 77);
/// The shadow under the resting dot (`iced-ui/icons.rs:240`,
/// `Color::from_rgba(0.0, 0.0, 0.0, 0.08)`).
pub const DPAD_IDLE_DOT_SHADOW: Color32 = Color32::from_black_alpha(20);
/// The resting dot itself (`iced-ui/icons.rs:245`,
/// `Color::from_rgba(0.58, 0.64, 0.72, 0.8)`) -- [`ICON_MUTED`] at 80%.
pub const DPAD_IDLE_DOT: Color32 = with_alpha(ICON_MUTED, 204);

/// Shadows, named after the surfaces that cast them. Iced's f32 shadow alphas
/// are the same colours quantized to a byte (see the module note); the offsets
/// and blur widths are egui's integer units now, which the reference's values
/// all already sat on.
pub const SHADOW_CARD: Shadow = Shadow {
    offset: [0, 1],
    blur: 2,
    spread: 0,
    color: Color32::from_black_alpha(10),
};
pub const SHADOW_SLOT: Shadow = Shadow {
    offset: [0, 1],
    blur: 2,
    spread: 0,
    color: Color32::from_black_alpha(13),
};
pub const SHADOW_NAME_BOX: Shadow = Shadow {
    offset: [0, 1],
    blur: 1,
    spread: 0,
    color: Color32::from_black_alpha(10),
};
pub const SHADOW_MODE_SELECTED: Shadow = Shadow {
    offset: [0, 1],
    blur: 2,
    spread: 0,
    color: Color32::from_black_alpha(15),
};
pub const SHADOW_BANNER: Shadow = Shadow {
    offset: [0, 4],
    blur: 6,
    spread: 0,
    color: Color32::from_black_alpha(38),
};
pub const SHADOW_PANEL: Shadow = Shadow {
    offset: [0, 8],
    blur: 24,
    spread: 0,
    color: Color32::from_black_alpha(38),
};
/// The keycap's resting lift, and the two glows: the 8pt rebinding ring and
/// the 3pt pressed shadow. The ring's colour is the per-direction accent's
/// glow (`ring` at the Iced call site), so those two are composed at the call
/// site from [`SHADOW_KEYCAP`]'s shape with the ring colour swapped in.
pub const SHADOW_KEYCAP: Shadow = Shadow {
    offset: [0, 1],
    blur: 2,
    spread: 0,
    color: Color32::from_black_alpha(13),
};
pub const KEYCAP_REBIND_GLOW_BLUR: u8 = 8;
pub const KEYCAP_PRESSED_GLOW_BLUR: u8 = 3;

/// Page background behind the cards. The Iced style also set the body text
/// colour; egui takes that globally from [`style`]'s `override_text_color`.
pub fn canvas_style() -> Frame {
    Frame::new().fill(CANVAS)
}

/// White card on the canvas background. Reference: `bg-white border-2
/// border-indigo-200/80 rounded-2xl shadow-sm hover:border-indigo-300`.
pub fn card_style() -> Frame {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke {
            width: 2.0,
            color: CARD_BORDER,
        })
        .corner_radius(CARD_RADIUS)
        .shadow(SHADOW_CARD)
}

/// Inset frame grouping related slots or slider sets.
pub fn group_style() -> Frame {
    Frame::new()
        .fill(INSET)
        .stroke(Stroke {
            width: 1.0,
            color: BORDER,
        })
        .corner_radius(GROUP_RADIUS)
}

/// The D-pad's stage: the same inset surface as a group but with the
/// reference's larger corner radius (`rounded-2xl`) and roomier padding
/// (`p-5`), which is applied at the call site.
pub fn stage_style() -> Frame {
    group_style().corner_radius(CARD_RADIUS)
}

/// The capture-mode banner: a solid indigo bar (reference `bg-indigo-600`)
/// with white text and a soft elevation shadow. The white ink is set at the
/// call site (egui labels take their colour from the widget, not the frame).
pub fn rebind_banner_style() -> Frame {
    Frame::new()
        .fill(INDIGO_600)
        .corner_radius(GROUP_RADIUS)
        .shadow(SHADOW_BANNER)
}

/// Surface tile inside a group: the white rounded boxes that hold a timing
/// group, the preview, or the mechanism block (reference: rounded, thin
/// border, soft shadow).
pub fn slot_style() -> Frame {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke {
            width: 1.0,
            color: BORDER,
        })
        .corner_radius(GROUP_RADIUS)
        .shadow(SHADOW_SLOT)
}

/// Neutral rounded frame around the timeline canvas (reference:
/// `rounded-2xl border-slate-200/70`). The canvas paints its own white
/// background, so this only supplies the border and the clipping radius.
pub fn graph_frame() -> Frame {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke {
            width: 1.0,
            color: BORDER,
        })
        .corner_radius(CARD_RADIUS)
}

/// Small bordered keycap chip in a slot card (reference: `rounded-md border
/// border-slate-300 bg-white/80`). The fill is opaque white rather than the
/// reference's white wash: 80% white over a card tint this pale resolves within
/// one step of white, and keeping it opaque stops linear compositing from
/// tinting the one part of the chip that must stay neutral.
pub fn chip_style(ink: SlotInk) -> Frame {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke {
            width: 1.0,
            color: ink.chip_edge,
        })
        .corner_radius(CHIP_RADIUS)
}

/// Dropdown panel holding the profile slot cards (reference: `rounded-2xl
/// border-slate-200 shadow-2xl`). Main cards keep [`card_style`] with its
/// indigo border; the dropdown is neutral so the slot tints carry the color.
pub fn profile_panel() -> Frame {
    Frame::new()
        .fill(SURFACE)
        .stroke(Stroke {
            width: 1.0,
            color: BORDER,
        })
        .corner_radius(CARD_RADIUS)
        .shadow(SHADOW_PANEL)
}

/// Confirm overlay covering a slot card (reference: `absolute inset-0
/// rounded-xl bg-white/95 border-slate-300`). Sits on top of the card so the
/// card never changes height while confirming.
pub fn profile_confirm_overlay() -> Frame {
    Frame::new()
        .fill(CONFIRM_OVERLAY_FILL)
        .stroke(Stroke {
            width: 1.0,
            color: SLATE_300,
        })
        .corner_radius(GROUP_RADIUS)
}

/// Profile slot card tinted by its mode and state.
pub fn tinted_slot(tint: SlotTint) -> Frame {
    Frame::new()
        .fill(tint.fill)
        .stroke(Stroke {
            width: 1.0,
            color: tint.border,
        })
        .corner_radius(GROUP_RADIUS)
}

/// Bordered pill grouping the bare value editors on a timing row.
pub fn pill_style(invalid: bool) -> Frame {
    Frame::new()
        .fill(if invalid { ERROR_BG } else { SURFACE })
        .stroke(Stroke {
            width: 1.0,
            color: if invalid { ERROR_BORDER } else { BORDER },
        })
        .corner_radius(GROUP_RADIUS)
}

/// Numbered step badge in the "How it works" block. The steps that carry
/// the mode's delay behavior are accent-tinted (`bg-<accent>/12`); the rest
/// stay neutral. The Iced style also carried the text colour: the accent
/// itself, or [`MUTED_TEXT`] -- take it at the label.
pub fn step_badge(accent: Option<Color32>) -> Frame {
    Frame::new().fill(match accent {
        Some(color) => with_alpha(color, 31),
        None => INSET,
    })
}

/// Small circle for the preview's example-position indicator.
pub fn example_dot(color: Color32) -> Frame {
    Frame::new().fill(color).corner_radius(DOT_RADIUS)
}

/// Amber badge marking uncommitted draft edits in the action bar. Its ink is
/// [`AMBER_DARK`], taken at the label.
pub fn dirty_badge() -> Frame {
    Frame::new()
        .fill(DIRTY_BADGE_BG)
        .stroke(Stroke {
            width: 1.0,
            color: DIRTY_BADGE_EDGE,
        })
        .corner_radius(GROUP_RADIUS)
}

/// Small filled circle used for status and legend marks.
pub fn dot_style(color: Color32) -> Frame {
    Frame::new().fill(color).corner_radius(BADGE_RADIUS)
}

/// The hairline dividing the two keycap pairs inside a slot card (reference:
/// `border-l border-slate-200`). Drawn as a filled box rather than a rule:
/// the reference sizes a `border-l` to its content, which is the 16px of a
/// keycap chip, and egui rules stretch to the layout instead.
pub fn pair_divider(ink: SlotInk) -> Frame {
    Frame::new().fill(ink.hairline)
}

/// Interaction state of a profile slot card.
///
/// The reference selects between its three card classes with CSS alone: the
/// loaded slot always takes the active class, a card under the pointer takes
/// its hover class, and every other card carries `opacity-75`. The active class
/// has no hover variant, so the active card does not change under the pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotState {
    Active,
    Hovered,
    Idle,
}

/// A card's drawn fill and edge, flattened to opaque sRGB.
#[derive(Clone, Copy, Debug)]
pub struct SlotTint {
    pub fill: Color32,
    pub border: Color32,
}

/// The ink a card's own contents draw with. The name box keeps an opaque white
/// fill in every state, so it is not part of this set.
#[derive(Clone, Copy, Debug)]
pub struct SlotInk {
    /// Slot name, inside the name box.
    pub name: Color32,
    /// The rename pencil beside the name. It sits one slate step below the name
    /// in the reference (`text-slate-400` against `text-slate-900`) and follows
    /// the box's `currentColor`, so it dims with the rest of an idle card and
    /// reaches indigo-600 whenever the box is hovered.
    pub pencil: Color32,
    /// Keycap label.
    pub chip_label: Color32,
    /// Keycap outline.
    pub chip_edge: Color32,
    /// Name box edge, and the hairline between the two key pairs.
    pub hairline: Color32,
}

/// Keycap interaction mode, from the Iced `keycap` style function. egui has no
/// style hook to carry it, so it stays as the plain state marker the keycap
/// widget composes its own visuals from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeycapMode {
    Normal,
    Pressed,
    Rebinding,
}

/// sRGB composite of `top` at `alpha` over `bottom`, rounded to the 8-bit steps
/// a screen shows.
///
/// The reference's browser blends in sRGB, so a tint composited here matches
/// what the reference draws instead of landing paler under a linear blend.
const fn over(top: Color32, alpha: f32, bottom: Color32) -> Color32 {
    Color32::from_rgb(
        round8(top.r() as f32 * alpha + bottom.r() as f32 * (1.0 - alpha)),
        round8(top.g() as f32 * alpha + bottom.g() as f32 * (1.0 - alpha)),
        round8(top.b() as f32 * alpha + bottom.b() as f32 * (1.0 - alpha)),
    )
}

/// Rounds one channel, already in 0..255 units, to the byte the screen will
/// show, so a composite stacked on another composite matches the browser's own
/// 8-bit arithmetic.
const fn round8(value: f32) -> u8 {
    (value + 0.5) as u8
}

/// The reference's `opacity-75` on an idle card: the whole card, ink included,
/// blended toward the white panel behind it.
const fn dim(color: Color32) -> Color32 {
    over(color, 0.75, Color32::WHITE)
}

/// Multiplies a color's RGB channels, for hover darkening of filled pills
/// (the Iced `preview_pill` used factors 0.88 and 0.8).
pub fn shade(color: Color32, factor: f32) -> Color32 {
    let channel = |value: u8| ((value as f32 * factor) + 0.5) as u8;
    Color32::from_rgba_unmultiplied(
        channel(color.r()),
        channel(color.g()),
        channel(color.b()),
        color.a(),
    )
}

/// Immediate and Random Mix draw their card from one accent and two alpha
/// classes: `bg-…/10` with `border-…/50` when active, `bg-…/5` with
/// `border-…/20` at `opacity-75` when idle, and `bg-…/10` with `border-…/40`
/// on hover.
///
/// The edge composites over the fill rather than over the panel, because a CSS
/// `border-box` background reaches under its own border. On the Immediate card
/// that is 9 points of red -- the difference between `#93A2F3` and `#9CAAF3` --
/// so it is modelled rather than rounded away.
const fn accent_tint(accent: Color32, state: SlotState) -> SlotTint {
    let (fill_alpha, border_alpha) = match state {
        SlotState::Active => (0.10, 0.50),
        SlotState::Hovered => (0.10, 0.40),
        SlotState::Idle => (0.05, 0.20),
    };
    let fill = over(accent, fill_alpha, Color32::WHITE);
    let border = over(accent, border_alpha, fill);
    match state {
        SlotState::Idle => SlotTint {
            fill: dim(fill),
            border: dim(border),
        },
        _ => SlotTint { fill, border },
    }
}

/// The two delay modes take their fill and edge from two different steps of a
/// Tailwind ramp (`bg-indigo-50/40` inside `border-indigo-200/70`) rather than
/// from one accent with two alphas, so each state names its own pair.
const fn ramp_tint(
    fill: Color32,
    fill_alpha: f32,
    edge: Color32,
    edge_alpha: f32,
    dimmed: bool,
) -> SlotTint {
    let base = over(fill, fill_alpha, Color32::WHITE);
    let border = over(edge, edge_alpha, base);
    if dimmed {
        SlotTint {
            fill: dim(base),
            border: dim(border),
        }
    } else {
        SlotTint { fill: base, border }
    }
}

const PRESS_ACTIVE: SlotTint = ramp_tint(INDIGO_100, 0.70, INDIGO_400, 1.0, false);
const PRESS_HOVER: SlotTint = ramp_tint(INDIGO_50, 0.80, INDIGO_300, 1.0, false);
const PRESS_IDLE: SlotTint = ramp_tint(INDIGO_50, 0.40, INDIGO_200, 0.70, true);
const RELEASE_ACTIVE: SlotTint = ramp_tint(VIOLET_100, 0.70, VIOLET_400, 1.0, false);
const RELEASE_HOVER: SlotTint = ramp_tint(VIOLET_50, 0.80, VIOLET_300, 1.0, false);
const RELEASE_IDLE: SlotTint = ramp_tint(VIOLET_50, 0.40, VIOLET_200, 0.70, true);

/// The fill and edge a slot card draws for one mode and state.
///
/// This is a separate table from the accent the timeline and preview use
/// (`app::mode_color`). The two agree on Immediate and Random Mix, which share
/// their accent constant, and differ on the two delay modes, whose cards sit one
/// ramp step off the accent. That is the reference's own arrangement.
pub const fn slot_tint(mode: SocdMode, state: SlotState) -> SlotTint {
    match mode {
        SocdMode::Immediate => accent_tint(IMMEDIATE_ACCENT, state),
        SocdMode::RandomMix => accent_tint(MIX_TEXT, state),
        SocdMode::PressDelay => match state {
            SlotState::Active => PRESS_ACTIVE,
            SlotState::Hovered => PRESS_HOVER,
            SlotState::Idle => PRESS_IDLE,
        },
        SocdMode::ReleaseDelay => match state {
            SlotState::Active => RELEASE_ACTIVE,
            SlotState::Hovered => RELEASE_HOVER,
            SlotState::Idle => RELEASE_IDLE,
        },
    }
}

/// The ink a card's contents draw with. Idle is the reference's `opacity-75`
/// applied to the same inks, so the name, the keycaps and the hairline all
/// lighten together -- measured at `#4B5160` for a slate-900 name where the
/// active card draws `#0F172B`.
///
/// `hovering` is the name box's own hover, not the card's: the reference gives
/// the box `hover:*` classes on its own element, so pointing anywhere else in
/// the card leaves the box at rest. It is how the pencil, which has to be
/// coloured where the icon is built, learns what the name box already knows
/// about the state it sits in.
pub fn slot_ink(state: SlotState) -> SlotInk {
    slot_ink_hovering(state, false)
}

/// [`slot_ink`] with the name box's hover folded in. Only the name box is
/// affected: its fill, edge, name and pencil all move together, so the box is
/// the one part of a card whose ink is not a function of the card's state
/// alone.
pub fn slot_ink_hovering(state: SlotState, hovering: bool) -> SlotInk {
    let ink = SlotInk {
        name: BODY_TEXT,
        pencil: ICON_MUTED,
        chip_label: CHIP_TEXT,
        chip_edge: SLATE_300,
        hairline: BORDER,
    };
    let ink = match state {
        SlotState::Idle => SlotInk {
            name: dim(ink.name),
            pencil: dim(ink.pencil),
            chip_label: dim(ink.chip_label),
            chip_edge: dim(ink.chip_edge),
            hairline: dim(ink.hairline),
        },
        _ => ink,
    };
    // The hover replaces the idle dim rather than stacking on it: the reference's
    // `group-hover/slot:text-indigo-600` is one class at full strength, and an
    // idle card under the pointer draws the same indigo as an active one.
    if hovering {
        SlotInk {
            name: INDIGO_600,
            pencil: INDIGO_600,
            ..ink
        }
    } else {
        ink
    }
}

/// The mode label's ink on a slot card. The immediate and random-mix labels use
/// the same colour as their card tint; the two delay labels reach one step
/// darker (`text-indigo-700`, `text-violet-700`), which is what the reference's
/// `MODE_TEXT` table states and what its rendered pixels show.
pub fn slot_mode_ink(mode: SocdMode, state: SlotState) -> Color32 {
    let base = match mode {
        SocdMode::Immediate => IMMEDIATE_ACCENT,
        SocdMode::PressDelay => INDIGO_700,
        SocdMode::RandomMix => MIX_TEXT,
        SocdMode::ReleaseDelay => RELEASE_LABEL,
    };
    match state {
        SlotState::Idle => dim(base),
        _ => base,
    }
}

/// The global egui style: the reference's palette and type sizes expressed as
/// `Visuals`, `Spacing`, and text styles.
///
/// This is the baseline the window installs; the view modules override per
/// widget where the reference styles a single control differently (a filled
/// primary button is indigo, the global button is the outlined control). The
/// mapping mirrors the Iced closures' shared defaults:
///
/// - `panel_fill` is the canvas behind the cards; [`INSET`] is the deep well
///   (`extreme_bg_color`) and [`SLATE_100`] the faint wash behind separators
///   and disabled rails.
/// - The interactive widget states carry the outlined-control pattern the
///   reference repeats across secondary buttons, mode segments, nav buttons,
///   and language rows: white shell and [`BORDER`] edge at rest, [`HOVER_WASH`]
///   fill, [`NAME_HOVER_BORDER`] edge and [`INDIGO_600`] ink on hover *and*
///   press (the Iced closures grouped `Hovered | Pressed`). egui 0.36 has no
///   disabled colour set -- it fades disabled widgets via `Visuals::
///   disabled_alpha` -- so the reference's disabled pair ([`SLATE_300`] ink on
///   a [`SURFACE`] shell, kept as named constants) must be applied by the
///   view modules where a control can actually be disabled.
/// - `noninteractive` carries body ink, so unstyled labels land on
///   `text-slate-900` the way the Iced containers' `text_color` did.
///
/// Dispositions of the Iced closures that are *not* expressed here, because
/// egui styles them at the call site: `primary_button`, `warning_button`,
/// `banner_cancel_button`, `profile_close_button`, `active_option`,
/// `language_option`, `profile_name_button`, `mode_button`, `preview_pill`,
/// `nav_button`, `keycap` (their state colours are all named constants above,
/// plus [`SEGMENT_RADIUS`]/[`CONTROL_RADIUS`]/[`PILL_RADIUS`]/[`CIRCLE_RADIUS`]
/// and the shadow constants); `accent_slider`, `mixer_slider` (geometry above;
/// the rails are [`PRIMARY_TEXT`]/[`SLATE_100`], the mixer instead splits
/// [`PRIMARY_TEXT`] left of the handle and [`RELEASE_TEXT`] right of it, with
/// no neutral [`SLATE_100`] track, ring [`MIX_TEXT`], the handle [`SURFACE`]
/// ringed 3pt, and the disabled ink [`SLIDER_RAIL_DISABLED`]);
/// `monitor_toggler` (track [`INDIGO_600`]/[`SLATE_300`], knob [`SURFACE`]);
/// `value_input`, `facade_button`, `profile_name_input` (fills and edges above;
/// the selection tint is [`TEXT_SELECTION`]); `table_rule` (a 1px [`BORDER`]
/// hairline, which `ui.separator()` draws from the mapped `bg_stroke`).
/// `secondary_button` *is* ported -- as the shared paint-only control
/// [`secondary_button`] below, the one exception the Master assigned after
/// R2 round 2 issue 2.
pub fn style() -> egui::Style {
    // egui 0.36 has no `Style::light()`; the light scheme is a `Visuals`
    // constructor, and the rest of the default style (spacing, interaction,
    // text styles) is scheme-independent.
    let mut style = egui::Style {
        visuals: egui::Visuals::light(),
        ..Default::default()
    };

    // Fonts: the Iced file left the face generic on purpose, and egui's
    // default font set is a bundled proportional/monospace pair; naming the
    // families keeps the same split (UI text proportional, keycap chips and
    // numbers monospace) without pinning a face that may not exist.
    style
        .text_styles
        .insert(TextStyle::Body, egui::FontId::new(BODY_TEXT_SIZE, UI_FONT));
    style.text_styles.insert(
        TextStyle::Button,
        egui::FontId::new(BODY_TEXT_SIZE, UI_FONT),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        egui::FontId::new(BODY_TEXT_SIZE, MONO_FONT),
    );
    style
        .text_styles
        .insert(TextStyle::Heading, egui::FontId::new(HEADING_SIZE, UI_FONT));

    style.spacing.item_spacing = Vec2::new(ROW_GAP, ROW_GAP);
    style.spacing.button_padding = Vec2::new(BUTTON_PADDING.left, BUTTON_PADDING.top);

    let visuals = &mut style.visuals;
    visuals.panel_fill = CANVAS;
    visuals.extreme_bg_color = INSET;
    visuals.faint_bg_color = SLATE_100;
    visuals.override_text_color = Some(BODY_TEXT);
    visuals.selection.bg_fill = TEXT_SELECTION;
    visuals.selection.stroke = Stroke::NONE;

    let rest = &mut visuals.widgets.noninteractive;
    rest.bg_fill = SURFACE;
    rest.weak_bg_fill = SURFACE;
    rest.bg_stroke = Stroke {
        width: 1.0,
        color: BORDER,
    };
    rest.fg_stroke = Stroke {
        width: 1.0,
        color: BODY_TEXT,
    };
    rest.corner_radius = CONTROL_RADIUS;

    for state in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        state.bg_fill = SURFACE;
        state.weak_bg_fill = SURFACE;
        state.bg_stroke = Stroke {
            width: 1.0,
            color: BORDER,
        };
        state.fg_stroke = Stroke {
            width: 1.0,
            color: SLATE_600,
        };
        state.corner_radius = CONTROL_RADIUS;
    }
    // The reference's hover and pressed states are one and the same class set
    // (`hover:bg-…` fires while pressed too), so both take the wash, the
    // indigo-300 edge, and the indigo-600 ink. `open` keeps the resting
    // visuals: the reference has no persistent open state for the plain
    // outlined control, and the panels that do open draw their own chrome.
    for state in [&mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        state.weak_bg_fill = HOVER_WASH;
        state.bg_stroke = Stroke {
            width: 1.0,
            color: NAME_HOVER_BORDER,
        };
        state.fg_stroke = Stroke {
            width: 1.0,
            color: INDIGO_600,
        };
    }

    style
}

// ---------------------------------------------------------------------------
// Shared paint-only controls (Master-assigned owner, R2 round 2 issue 2).
// ---------------------------------------------------------------------------

/// The action icon set, ported from `src/ui/mapping.rs` (whose glyphs trace
/// the Iced `iced-ui/icons.rs::draw_icon` paths). Paint-only: a kind, a rect,
/// an ink.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Icon {
    Keyboard,
    Restore,
    Edit,
    Check,
    Warning,
}

/// Trace one [`Icon`] into `rect` at `color`. The geometry is the mapping
/// card's, which is the geometry the Iced widget drew.
pub fn paint_icon(painter: &Painter, rect: Rect, kind: Icon, color: Color32) {
    let size = rect.width().min(rect.height());
    let stroke = Stroke::new((size * 0.1).max(1.2), color);
    let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
    let line = |coordinates: &[(f32, f32)]| {
        painter.add(Shape::line(
            coordinates.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    match kind {
        Icon::Keyboard => {
            painter.rect_stroke(
                Rect::from_min_max(point(0.15, 0.23), point(0.85, 0.78)),
                CornerRadius::same(1),
                stroke,
                StrokeKind::Middle,
            );
            line(&[(0.32, 0.6), (0.68, 0.6)]);
            for x in [0.28, 0.5, 0.72] {
                painter.circle_filled(point(x, 0.4), size * 0.04, color);
            }
        }
        Icon::Restore => {
            // The Iced arc runs PI * 0.2 -> PI * 1.8 around the centre at r
            // 0.32; sampled because epaint shapes are polylines.
            let arc: Vec<Pos2> = (0..=24)
                .map(|step| {
                    let angle = PI * 0.2 + PI * 1.6 * (step as f32 / 24.0);
                    point(0.5 + angle.cos() * 0.32, 0.5 + angle.sin() * 0.32)
                })
                .collect();
            painter.add(Shape::line(arc, stroke));
            let x = 0.5 + (PI * 1.8).cos() * 0.32;
            let y = 0.5 + (PI * 1.8).sin() * 0.32;
            line(&[(x - 0.15, y), (x, y), (x, y + 0.15)]);
        }
        Icon::Edit => line(&[
            (0.2, 0.8),
            (0.2, 0.65),
            (0.65, 0.2),
            (0.8, 0.35),
            (0.35, 0.8),
            (0.2, 0.8),
        ]),
        Icon::Check => line(&[(0.15, 0.52), (0.4, 0.78), (0.85, 0.25)]),
        Icon::Warning => {
            line(&[(0.5, 0.15), (0.85, 0.85), (0.15, 0.85), (0.5, 0.15)]);
            line(&[(0.5, 0.38), (0.5, 0.62)]);
            painter.circle_filled(point(0.5, 0.75), size * 0.05, color);
        }
    }
}

/// The icon ink rule from the Iced `icon_label` (iced-ui/app.rs:2411-2428):
/// a Restore glyph keeps the reference's slate-600 ink in every state, while
/// other action icons inherit the button's text colour so their hover states
/// keep working. The label itself follows the hover ink either way.
fn action_icon_ink(kind: Icon, label_ink: Color32) -> Color32 {
    if kind == Icon::Restore {
        ICON_SECONDARY
    } else {
        label_ink
    }
}

/// The outlined action's source dimensions: a 30px shell -- the reference's
/// `py-1.5` line (16px) plus 2*6 padding plus 2*1 border, the derivation the
/// Iced theme comment records beside [`BUTTON_PADDING`] -- with a 14px icon
/// and a 6px gap ahead of the 12px label. Horizontal padding is
/// [`BUTTON_PADDING`]'s 13pt per side; the corner is [`CONTROL_RADIUS`].
pub const BUTTON_HEIGHT: f32 = 30.0;
pub const BUTTON_TEXT_SIZE: f32 = 12.0;
pub const BUTTON_ICON: f32 = 14.0;
pub const BUTTON_ICON_GAP: f32 = 6.0;

/// The double-stamp weight approximation's offset: a second pass of the same
/// galley at `max(size * STAMP_OFFSET_FACTOR, STAMP_OFFSET_MIN)` px to the
/// right, the geometry `src/ui/keycap.rs`'s `stamp_galley` renders with.
pub const STAMP_OFFSET_FACTOR: f32 = 0.04;
pub const STAMP_OFFSET_MIN: f32 = 0.35;

/// Paint one galley twice at a sub-pixel offset so it reads heavier.
///
/// This is the single owner the R2 round-1 issue 8 asked for -- the copy
/// `keycap.rs` carries is the consumer wave's to delete, exactly as its
/// `KeycapMode` copy was this one's to absorb. It is **not** a blessing of
/// the approximation: egui's bundled faces ship a single weight, so the Iced
/// port's `UI_FONT_BOLD`/`UI_FONT_BLACK` families cannot be selected and
/// `RichText::strong` only recolours. Whether to register a weighted face
/// and drop this, or record the stamp as the accepted approach in
/// `docs/architecture/ui.md`, is an open Master decision (ui.md:112-119
/// requires bold key names and footer typography); no bold-weight parity is
/// claimed by anything here.
pub fn stamp_galley(painter: &Painter, pos: Pos2, galley: &Arc<Galley>, color: Color32, size: f32) {
    painter.galley(pos, galley.clone(), color);
    painter.galley(
        pos + Vec2::new((size * STAMP_OFFSET_FACTOR).max(STAMP_OFFSET_MIN), 0.0),
        galley.clone(),
        color,
    );
}

/// The outlined secondary action: white shell, [`BORDER`] edge,
/// [`ICON_SECONDARY`] label at rest; [`HOVER_WASH`] fill, [`NAME_HOVER_BORDER`]
/// edge and [`INDIGO_600`] label on hover; a disabled ui drops the label to
/// [`SLATE_300`] and keeps the outline (the Iced `secondary_button`
/// `Disabled` arm, iced-ui/theme.rs). egui additionally fades everything a
/// disabled ui paints by `Visuals::disabled_alpha`, so the rendered disabled
/// bytes are the spec pair under that fade -- a representation difference
/// against Iced's explicit-only disabled arm, not a value change.
/// Paint-only: it draws, publishes the
/// accessible node, and hands back the [`Response`] -- mapping a click to a
/// `Message` stays the caller's job (migration constraint 2).
///
/// Ported from the verified integrated rendering in `src/ui/mapping.rs`
/// (R2 round 2 issue 2), which is itself the Iced `theme::secondary_button`
/// closure plus `icon_label`'s Restore-only slate ink. The label galley is
/// laid out with [`Color32::PLACEHOLDER`] on purpose: `Painter::galley`'s
/// colour argument is a fallback only, and a galley laid out in a real colour
/// would pin it and ignore the per-state ink (the trap R2 measured at egui
/// 0.36 `painter.rs:527`, where the mapping card's white-on-white regression
/// lived).
pub fn secondary_button(ui: &mut Ui, kind: Icon, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(BUTTON_TEXT_SIZE),
        Color32::PLACEHOLDER,
    );
    let content = BUTTON_ICON + BUTTON_ICON_GAP + galley.size().x;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(content + 2.0 * BUTTON_PADDING.left, BUTTON_HEIGHT),
        Sense::click(),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    let (fill, edge, label_ink) = if !ui.is_enabled() {
        (SURFACE, BORDER, SLATE_300)
    } else if response.hovered() {
        (HOVER_WASH, NAME_HOVER_BORDER, INDIGO_600)
    } else {
        (SURFACE, BORDER, ICON_SECONDARY)
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CONTROL_RADIUS, fill);
    painter.rect_stroke(
        rect,
        CONTROL_RADIUS,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    let start = rect.center().x - content / 2.0;
    paint_icon(
        &painter,
        Rect::from_min_size(
            Pos2::new(start, rect.center().y - BUTTON_ICON / 2.0),
            Vec2::splat(BUTTON_ICON),
        ),
        kind,
        action_icon_ink(kind, label_ink),
    );
    stamp_galley(
        &painter,
        Pos2::new(
            start + BUTTON_ICON + BUTTON_ICON_GAP,
            rect.center().y - galley.size().y / 2.0,
        ),
        &galley,
        label_ink,
        BUTTON_TEXT_SIZE,
    );
    response
}

#[cfg(test)]
mod theme_tests {
    //! Slot-card tint tests, ported from `iced-ui/theme_tests.rs`.
    //!
    //! The values below are the reference's own rendered pixels, not its class
    //! names. A browser blends alpha in sRGB while a linear-space blend lands
    //! visibly paler (`#CED1F6` where the reference shows `#EBEEFD`, measured),
    //! so `slot_tint` flattens the composite to opaque sRGB, and these pin
    //! that.
    //!
    //! Every expectation was read off a screenshot of the running reference at
    //! the panel's card-fill and card-edge coordinates. Channel tolerance is
    //! one 8-bit step, because the browser keeps more precision between two
    //! stacked composites than this model does -- the model rounds to a byte at
    //! each step. The error it guards against is thirty steps, not one.
    //!
    //! Four entries are derived rather than measured and are marked as such:
    //! the reference was never captured with the Immediate card hovered or the
    //! Random Mix card loaded, and the name box's hover inks follow conclusions
    //! about the reference's DOM rather than pixels read off it (one
    //! `group-hover` class, so the name and the pencil move together; the box's
    //! ink sets `currentColor`, so the two must land on the same colour in both
    //! states).

    use super::{
        DPAD_ACTIVE_DOT_SHADOW, DPAD_ACTIVE_GLOW, DPAD_GUIDE_DOT, DPAD_IDLE_DOT,
        DPAD_IDLE_DOT_SHADOW, ICON_MUTED, IMMEDIATE_ACCENT, SLATE_300, SLIDER_RAIL_DISABLED,
        SlotState, rgb, slot_ink, slot_ink_hovering, slot_mode_ink, slot_tint,
    };
    use crate::settings::SocdMode;
    use egui::Color32;

    /// Splits a colour into its 8-bit channels, the way a screen shows it.
    fn channels(color: Color32) -> [u8; 3] {
        [color.r(), color.g(), color.b()]
    }

    /// Parses `#RRGGBB` into its 8-bit channels.
    fn parse(hex: &str) -> [u8; 3] {
        let byte =
            |i: usize| u8::from_str_radix(&hex[1 + i * 2..3 + i * 2], 16).expect("hex digits");
        [byte(0), byte(1), byte(2)]
    }

    /// Asserts a colour's channels match, one 8-bit step per channel.
    fn assert_close(color: Color32, expected: &str, what: &str) {
        let actual = channels(color);
        let want = parse(expected);
        for i in 0..3 {
            assert!(
                actual[i].abs_diff(want[i]) <= 1,
                "{what}: got {actual:?} want {expected} ({want:?})"
            );
        }
    }

    #[test]
    fn immediate_tints_match_the_reference() {
        let active = slot_tint(SocdMode::Immediate, SlotState::Active);
        assert_close(active.fill, "#EBEDFC", "Immediate active fill");
        assert_close(active.border, "#93A1F2", "Immediate active border");

        let idle = slot_tint(SocdMode::Immediate, SlotState::Idle);
        assert_close(idle.fill, "#F8F8FE", "Immediate idle fill");
        assert_close(idle.border, "#DCE0FB", "Immediate idle border");
    }

    #[test]
    fn random_mix_tints_match_the_reference() {
        let hovered = slot_tint(SocdMode::RandomMix, SlotState::Hovered);
        assert_close(hovered.fill, "#F0EDFD", "Random Mix hovered fill");
        assert_close(hovered.border, "#BBAEF7", "Random Mix hovered border");

        let idle = slot_tint(SocdMode::RandomMix, SlotState::Idle);
        assert_close(idle.fill, "#F9F8FE", "Random Mix idle fill");
        assert_close(idle.border, "#E5E0FC", "Random Mix idle border");

        // Derived: the active card was never captured, but it shares Immediate's
        // alphas on its own accent, and that pair was measured.
        let active = slot_tint(SocdMode::RandomMix, SlotState::Active);
        assert_close(active.fill, "#F0EDFD", "Random Mix active fill (derived)");
        assert_close(
            active.border,
            "#AF9FF6",
            "Random Mix active border (derived)",
        );
    }

    #[test]
    fn press_delay_tints_match_the_reference() {
        let active = slot_tint(SocdMode::PressDelay, SlotState::Active);
        assert_close(active.fill, "#E9EEFF", "Press Delay active fill");
        assert_close(active.border, "#7C86FF", "Press Delay active border");

        let hovered = slot_tint(SocdMode::PressDelay, SlotState::Hovered);
        assert_close(hovered.fill, "#F1F5FF", "Press Delay hovered fill");
        assert_close(hovered.border, "#A3B3FF", "Press Delay hovered border");

        let idle = slot_tint(SocdMode::PressDelay, SlotState::Idle);
        assert_close(idle.fill, "#FAFBFF", "Press Delay idle fill");
        assert_close(idle.border, "#E0E6FF", "Press Delay idle border");
    }

    #[test]
    fn release_delay_tints_match_the_reference() {
        let active = slot_tint(SocdMode::ReleaseDelay, SlotState::Active);
        assert_close(active.fill, "#F2F0FE", "Release Delay active fill");
        assert_close(active.border, "#A684FF", "Release Delay active border");

        let hovered = slot_tint(SocdMode::ReleaseDelay, SlotState::Hovered);
        assert_close(hovered.fill, "#F7F5FF", "Release Delay hovered fill");
        assert_close(hovered.border, "#C4B3FF", "Release Delay hovered border");

        let idle = slot_tint(SocdMode::ReleaseDelay, SlotState::Idle);
        assert_close(idle.fill, "#FCFBFF", "Release Delay idle fill");
        assert_close(idle.border, "#ECE8FF", "Release Delay idle border");
    }

    #[test]
    fn every_card_colour_is_opaque() {
        // The whole point of the model: a linear alpha blend lands off the
        // reference, so nothing may reach the painter with an alpha.
        for mode in SocdMode::ALL {
            for state in [SlotState::Active, SlotState::Hovered, SlotState::Idle] {
                let tint = slot_tint(mode, state);
                assert!(
                    tint.fill.is_opaque(),
                    "{mode:?}/{state:?} fill must be opaque"
                );
                assert!(
                    tint.border.is_opaque(),
                    "{mode:?}/{state:?} border must be opaque"
                );
            }
        }
    }

    #[test]
    fn idle_ink_is_the_reference_opacity_step() {
        // The reference dims an idle card's *contents*, not just its wash, so
        // the name it draws is slate-900 at `opacity-75`.
        assert_close(slot_ink(SlotState::Active).name, "#0F172B", "active name");
        assert_close(slot_ink(SlotState::Idle).name, "#4B5160", "idle name");
        assert_close(
            slot_ink(SlotState::Active).chip_edge,
            "#CAD5E2",
            "active chip edge",
        );
        // Hover is a full-strength card, so it takes the undimmed ink.
        assert_eq!(
            channels(slot_ink(SlotState::Hovered).name),
            channels(slot_ink(SlotState::Active).name)
        );
    }

    #[test]
    fn name_box_hover_moves_the_name_and_the_pencil_together() {
        // The box is the only part of a card whose ink is not a function of the
        // card's state: `group-hover/slot:text-indigo-600` recolours the name,
        // and the pencil draws in `currentColor` from that same declaration, so
        // both land on indigo-600. One class at full strength also means the
        // hover *replaces* the idle dim rather than stacking on it -- an idle
        // card under the pointer draws the same indigo a loaded one would.
        for state in [SlotState::Idle, SlotState::Hovered, SlotState::Active] {
            let resting = slot_ink(state);
            let hovered = slot_ink_hovering(state, true);

            assert_close(hovered.name, "#4F39F6", "hovered name");
            assert_close(hovered.pencil, "#4F39F6", "hovered pencil");
            assert_ne!(
                channels(hovered.name),
                channels(resting.name),
                "{state:?}: the hover has to be visible against the rest ink"
            );
            assert_eq!(
                channels(hovered.name),
                channels(hovered.pencil),
                "{state:?}: the pencil follows the name's colour on hover"
            );
            // The box's own ink is all that moves; the chips and the divider
            // are outside it and keep the card's state ink.
            assert_eq!(channels(hovered.chip_label), channels(resting.chip_label));
            assert_eq!(channels(hovered.chip_edge), channels(resting.chip_edge));
            assert_eq!(channels(hovered.hairline), channels(resting.hairline));
        }

        // At rest the two inks differ, which is why the box cannot be styled
        // through a single text colour.
        assert_ne!(
            channels(slot_ink(SlotState::Active).name),
            channels(slot_ink(SlotState::Active).pencil)
        );
        // Not hovering is exactly the resting table.
        for state in [SlotState::Idle, SlotState::Hovered, SlotState::Active] {
            assert_eq!(
                channels(slot_ink_hovering(state, false).name),
                channels(slot_ink(state).name),
                "{state:?}: `slot_ink` is `slot_ink_hovering` with no hover"
            );
        }
    }

    #[test]
    fn mode_label_ink_matches_the_reference_table() {
        // The two delay labels reach one ramp step darker than the cards they
        // sit on; the other two label in their card's own colour.
        assert_close(
            slot_mode_ink(SocdMode::Immediate, SlotState::Active),
            "#3A55E8",
            "Immediate label",
        );
        assert_close(
            slot_mode_ink(SocdMode::PressDelay, SlotState::Active),
            "#432DD7",
            "Press Delay label",
        );
        assert_close(
            slot_mode_ink(SocdMode::RandomMix, SlotState::Active),
            "#6D51EE",
            "Random Mix label",
        );
        assert_close(
            slot_mode_ink(SocdMode::ReleaseDelay, SlotState::Active),
            "#7008E7",
            "Release Delay label",
        );
        // An idle card dims its label with everything else.
        assert_close(
            slot_mode_ink(SocdMode::PressDelay, SlotState::Idle),
            "#7262E1",
            "idle Press Delay label",
        );
    }

    #[test]
    fn hover_changes_the_card_but_not_an_active_one() {
        // The reference's active class carries no hover variant, so the card
        // state never reports `Hovered` for the loaded slot. Its tints still
        // have to stay distinguishable, or this test would pass while the hover
        // was invisible.
        //
        // The pair is compared, not each channel: Immediate and Random Mix take
        // the same `bg-…/10` wash in both states and deepen only the edge, so
        // their fills are equal by design and only the border moves.
        for mode in SocdMode::ALL {
            let active = slot_tint(mode, SlotState::Active);
            let hovered = slot_tint(mode, SlotState::Hovered);
            assert_ne!(
                (channels(active.fill), channels(active.border)),
                (channels(hovered.fill), channels(hovered.border)),
                "{mode:?} must draw a different card when hovered than when loaded"
            );
        }
    }

    #[test]
    fn widget_sourced_tokens_match_their_iced_bytes() {
        // The tokens canonicalised from Iced *widget* code (not theme.rs)
        // are pinned to the exact straight-alpha bytes the Iced source
        // wrote. The pins go through `from_rgba_unmultiplied` because
        // `Color32` stores premultiplied bytes and the unmultiplied read
        // back is not an exact inverse (204 at alpha 153 reads back 203);
        // construction-argument equality is the exact claim.
        //
        // The disabled range-slider ink (iced-ui/widgets.rs:234) is
        // #cbd5e1; the theme's slate-300 (src/ui/theme.rs) is #cad5e2. A
        // dedup that merged them would recolour the disabled rail (R2
        // finding 4).
        assert_eq!(
            SLIDER_RAIL_DISABLED.to_srgba_unmultiplied(),
            [203, 213, 225, 255]
        );
        assert_eq!(SLATE_300.to_srgba_unmultiplied(), [0xca, 0xd5, 0xe2, 255]);
        assert_ne!(SLIDER_RAIL_DISABLED, SLATE_300);
        // The D-pad dots and glow (iced-ui/icons.rs:215-245).
        assert_eq!(
            DPAD_GUIDE_DOT,
            Color32::from_rgba_unmultiplied(204, 214, 224, 153)
        );
        assert_eq!(
            DPAD_ACTIVE_GLOW,
            Color32::from_rgba_unmultiplied(59, 84, 232, 64)
        );
        assert_eq!(
            DPAD_ACTIVE_DOT_SHADOW,
            Color32::from_rgba_unmultiplied(59, 84, 232, 77)
        );
        assert_eq!(DPAD_IDLE_DOT_SHADOW, Color32::from_black_alpha(20));
        assert_eq!(
            DPAD_IDLE_DOT,
            Color32::from_rgba_unmultiplied(148, 163, 184, 204)
        );
        // The resting dot's base is ICON_MUTED, and the glow base is the
        // Iced decimal rounding -- one LSB off the accent in two channels,
        // kept deliberately (see the constants' docs).
        assert_eq!(&ICON_MUTED.to_srgba_unmultiplied()[..3], &[148, 163, 184]);
        assert_ne!(rgb(59, 84, 232), IMMEDIATE_ACCENT);
    }
}

#[cfg(test)]
mod controls_tests {
    //! Render tests for the shared paint-only controls.
    //!
    //! The pattern follows the mapping card's integrated tests: read the
    //! painted shapes out of the harness output, so the layout-colour trap
    //! and the state inks are observed rather than asserted by construction.

    use super::{
        BORDER, BUTTON_HEIGHT, BUTTON_ICON, BUTTON_ICON_GAP, BUTTON_PADDING, HOVER_WASH,
        ICON_SECONDARY, INDIGO_600, Icon, NAME_HOVER_BORDER, SLATE_300, SURFACE, action_icon_ink,
        secondary_button,
    };
    use egui::{Color32, Rect, Shape};
    use egui_kittest::{Harness, kittest::Queryable};

    const LABEL: &str = "Restore timing defaults";

    /// The colours a painted label actually renders in, one per stamp pass.
    ///
    /// A galley carries the colour it was laid out with, and the colour
    /// handed to `Painter::galley` only reaches sections laid out with
    /// [`Color32::PLACEHOLDER`] -- reading both is what makes the
    /// layout-colour defect observable.
    fn painted_text_colors(harness: &Harness, needle: &str) -> Vec<Color32> {
        harness
            .output()
            .shapes
            .iter()
            .filter_map(|clipped| {
                let Shape::Text(text) = &clipped.shape else {
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
            .collect()
    }

    fn painted_text_width(harness: &Harness, needle: &str) -> Option<f32> {
        harness.output().shapes.iter().find_map(|clipped| {
            let Shape::Text(text) = &clipped.shape else {
                return None;
            };
            text.galley
                .text()
                .contains(needle)
                .then_some(text.galley.size().x)
        })
    }

    #[test]
    fn the_secondary_button_publishes_a_named_button_node() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 120.0))
            .build_ui(|ui| {
                secondary_button(ui, Icon::Restore, LABEL);
            });
        harness.run();
        // Panics unless a Button-role node with this exact name exists, which
        // is the accessible semantics the control must keep for kittest and
        // screen readers.
        let by_role = harness.get_by_role_and_label(egui::accesskit::Role::Button, LABEL);
        let by_label = harness.get_by_label(LABEL);
        assert_eq!(
            by_role.rect(),
            by_label.rect(),
            "both queries must hit the same node"
        );
    }

    #[test]
    fn the_secondary_button_paints_the_state_inks() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 120.0))
            .build_ui(|ui| {
                secondary_button(ui, Icon::Restore, LABEL);
            });
        harness.run();
        let rest = painted_text_colors(&harness, LABEL);
        assert!(!rest.is_empty(), "the label must render at all");
        assert!(
            rest.iter().all(|color| *color == ICON_SECONDARY),
            "at rest the label must render in the secondary ink, not in the colour it was laid out with: {rest:?}"
        );

        harness.get_by_label(LABEL).hover();
        harness.run();
        let hovered = painted_text_colors(&harness, LABEL);
        assert!(
            hovered.iter().all(|color| *color == INDIGO_600),
            "hover must move the label to the indigo ink: {hovered:?}"
        );
    }

    #[test]
    fn the_label_is_stamped_twice_by_the_shared_approximation() {
        // Pins the integrated rendering (two text passes, the same galley).
        // This records what the control does, not a bold-weight parity claim:
        // the weight strategy stays an open Master decision.
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 120.0))
            .build_ui(|ui| {
                secondary_button(ui, Icon::Restore, LABEL);
            });
        harness.run();
        let stamps = painted_text_colors(&harness, LABEL);
        assert_eq!(
            stamps.len(),
            2,
            "the label must carry exactly the two passes of the shared stamp"
        );
    }

    #[test]
    fn the_secondary_button_keeps_the_source_dimensions() {
        let captured: std::cell::RefCell<Option<Rect>> = std::cell::RefCell::new(None);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 120.0))
            .build_ui(|ui| {
                let response = secondary_button(ui, Icon::Restore, LABEL);
                *captured.borrow_mut() = Some(response.rect);
            });
        harness.run();
        let rect = *captured
            .borrow()
            .as_ref()
            .expect("the closure ran and captured the response rect");
        assert_eq!(
            rect.height(),
            BUTTON_HEIGHT,
            "the reference action is 30px tall"
        );
        let text_width = painted_text_width(&harness, LABEL).expect("the label rendered");
        let expected = text_width + BUTTON_ICON + BUTTON_ICON_GAP + 2.0 * BUTTON_PADDING.left;
        assert!(
            (rect.width() - expected).abs() < 0.01,
            "width must be icon + gap + label + the BUTTON_PADDING 13pt per side: rect {} vs expected {expected}",
            rect.width()
        );
    }

    #[test]
    fn the_disabled_button_takes_the_iced_disabled_pair() {
        // The Iced `secondary_button` Disabled arm: SURFACE shell, BORDER
        // outline, SLATE_300 ink. The integrated mapping copy had no disabled
        // branch (both current call sites are always enabled); this is the
        // spec the shared owner restores, inert for enabled consumers.
        //
        // The rendered bytes are additionally faded by egui's global
        // `Visuals::disabled_alpha` (0.5 in `Visuals::light()`, applied to
        // every shape a disabled ui paints) -- a difference of representation
        // against Iced, which had no automatic fade. The control's ink
        // SELECTION is what this pins; the fade sits on top of it.
        let mut harness = Harness::builder()
            .with_size(egui::vec2(400.0, 120.0))
            .build_ui(|ui| {
                ui.disable();
                secondary_button(ui, Icon::Restore, LABEL);
            });
        harness.run();
        // The fade primitive is `Visuals::faded` (egui style.rs:1179:
        // `color.gamma_multiply(disabled_alpha)`); 0.5 is the shipped default
        // in both schemes (style.rs:1560, and observed under the harness's
        // default style).
        let faded = SLATE_300.gamma_multiply(0.5);
        let inks = painted_text_colors(&harness, LABEL);
        assert!(
            !inks.is_empty() && inks.iter().all(|color| *color == faded),
            "a disabled control drops its label to slate-300 (under the global fade): {inks:?} vs {faded:?}"
        );
    }

    #[test]
    fn the_restore_icon_keeps_the_iced_slate_ink_rule() {
        // iced-ui/app.rs:2411-2428: Restore-only slate ink; other icons
        // inherit the button ink so hover reaches them.
        assert_eq!(action_icon_ink(Icon::Restore, INDIGO_600), ICON_SECONDARY);
        assert_eq!(action_icon_ink(Icon::Keyboard, INDIGO_600), INDIGO_600);
        // And the resting pair the control composes from:
        assert_eq!(
            action_icon_ink(Icon::Restore, ICON_SECONDARY),
            ICON_SECONDARY
        );
        assert_eq!(action_icon_ink(Icon::Check, ICON_SECONDARY), ICON_SECONDARY);
    }

    #[test]
    fn the_state_triples_are_the_named_tokens() {
        // The control composes its three states from the published constants
        // -- pin the pairs so a token edit cannot silently move one state.
        // (fill, edge, ink) rest / hover / disabled, as secondary_button
        // documents.
        let rest = (SURFACE, BORDER, ICON_SECONDARY);
        let hovered = (HOVER_WASH, NAME_HOVER_BORDER, INDIGO_600);
        let disabled = (SURFACE, BORDER, SLATE_300);
        assert_ne!(rest.2, hovered.2, "hover must actually move the ink");
        assert_ne!(rest.2, disabled.2, "disabled must actually dim the ink");
        assert_eq!(rest.0, disabled.0, "both keep the SURFACE shell");
    }
}
