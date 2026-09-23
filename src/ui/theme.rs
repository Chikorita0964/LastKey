//! Palette and widget tokens for the egui settings window.
//!
//! This file is the single owner of the window's visual contract: every colour,
//! radius, shadow, spacing, and frame the view modules paint comes from here,
//! and `docs/architecture/design.md` is the human-readable index of what those
//! tokens mean. Nothing here changes what the UI does, only how its tokens are
//! expressed.
//!
//! # Representation
//!
//! - Colours are [`egui::Color32`]. Translucent values are the 8-bit alpha the
//!   screen can actually show, quantized from whatever f32 the design source
//!   carried. The alpha scale is Tailwind's own and is closed (`/5`, `/10`,
//!   `/20`, `/25`, `/30`, `/40`, `/50`, `/60`, `/70`, `/80`, `/90`, `/95`); the
//!   byte each one quantizes to is `0.05 -> 13`, `0.10 -> 26`, `0.20 -> 51`,
//!   `0.25 -> 64`, `0.30 -> 77`, `0.40 -> 102`, `0.50 -> 128`, `0.60 -> 153`,
//!   `0.70 -> 179`, `0.80 -> 204`, `0.90 -> 230`, `0.95 -> 242`.
//! - Padding is `egui::epaint::MarginF32` (egui 0.36 removed `Padding`). Frame
//!   margins in egui are i8-quantized at the call site; the tokens stay f32 so
//!   a `3.5` chip inset does not silently round in this file.
//! - Corner radii are [`egui::CornerRadius`] (u8). "Fully round" is
//!   [`CIRCLE_RADIUS`]: egui clamps a radius to half the box, so the maximum
//!   byte draws a circle from any size.
//! - Fonts: egui selects a face by [`egui::FontFamily`] and registered variant,
//!   and eframe's `default_fonts` ships no bold/semibold/italic faces, so
//!   `UI_FONT_BOLD`, `UI_FONT_SEMIBOLD`, `UI_FONT_ITALIC`, `UI_FONT_BLACK` and
//!   `CHIP_FONT` have no counterpart. The generic families stay here
//!   ([`UI_FONT`], [`MONO_FONT`]), and [`fonts`] registers the native Windows UI
//!   face ahead of the bundled faces (F24), with separate Han and Hangul
//!   fallbacks behind it because neither script's faces cover the other.
//!   Emphasis is a size/colour decision at the call site until egui can select
//!   a face by weight and not by family alone (migration constraint 5).
//! - Styles are composed at the call site from `Style`/`Visuals` plus
//!   per-widget builders, so per-widget *state* lives in the view modules while
//!   every state-independent value is exposed here as a constant or an
//!   [`egui::Frame`] builder. [`secondary_button`] and the [`Icon`]/[`paint_icon`]
//!   set are the shared paint-only controls, so the outlined secondary action
//!   both cards render has one owner.
//! - Tokens that once lived inside widget code are canonicalised here too, so
//!   the view modules import one named value instead of repeating a literal per
//!   call site.

use crate::settings::SocdMode;
use egui::epaint::{CubicBezierShape, MarginF32};
use egui::{
    Color32, CornerRadius, FontId, Frame, Galley, Painter, Pos2, Rect, Response, Sense, Shadow,
    Shape, Stroke, StrokeKind, TextStyle, Ui, Vec2, WidgetInfo, WidgetType,
};
use std::f32::consts::PI;
use std::sync::Arc;

const fn rgb(red: u8, green: u8, blue: u8) -> Color32 {
    Color32::from_rgb(red, green, blue)
}

/// Straight-alpha restatement of an opaque colour: the design source stored
/// `a` as an f32 on the same struct, egui keeps it as the fourth byte.
///
/// Public because the preview's highlighted delay badge carries the
/// reference's `ring-*-200/90` and `ring-[#3a55e8]/25` alphas, which are
/// translucency rather than a composite over a known surface: the ring sits
/// on the group's inset fill, so epaint must blend it against whatever is
/// actually behind it.
pub const fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(color.r(), color.g(), color.b(), alpha)
}

// ===========================================================================
// Palette
// ===========================================================================
//
// Every colour the window paints is a Tailwind v4 step, held as the sRGB bytes
// that step resolves to. These ramps are the whole colour vocabulary: a view
// module names a step and nothing else, so where a colour comes from reads off
// the call site.
//
//   slate   ⌞50  ⌞100 ⌞200 ⌞300 ⌞400 ⌞500 ⌞600 ⌞700 ⌞800 ⌞900
//   amber   ⌞100 ⌞200 ⌞500 ⌞600 ⌞700
//   red     ⌞50  ⌞400 ⌞500 ⌞600
//   emerald ⌞100 ⌞500 ⌞600
//   indigo  ⌞50  ⌞100 ⌞200 ⌞300 ⌞400 ⌞500 ⌞600 ⌞700 ⌞800 ⌞950
//   blue    ⌞200 ⌞300 ⌞600 ⌞700
//   violet  ⌞50  ⌞100 ⌞200 ⌞300 ⌞400 ⌞500 ⌞600 ⌞700
//   purple  ⌞200 ⌞300 ⌞600 ⌞700
//
// `white` and `black` are the achromatic pair and have no ramp: [`WHITE`] sits
// in the surfaces section below, and the alphas built on either colour sit in
// the slash-alpha and translucent sections.

// -- slate: surfaces, neutral hairlines, and the ink ladder --

/// `bg-slate-50`: the keycap hover fill.
pub const SLATE_50: Color32 = rgb(0xf8, 0xfa, 0xfc);
/// `bg-slate-100`: slider rails, the step-badge well, and the D-pad tile.
pub const SLATE_100: Color32 = rgb(0xf1, 0xf5, 0xf9);
/// `border-slate-200`: every neutral hairline, control outline, and dropdown
/// edge. One class covers the card frame, the control outline, and the dropdown
/// border, so the port names it once rather than keeping three near-duplicate
/// slates that drift apart.
pub const SLATE_200: Color32 = rgb(0xe2, 0xe8, 0xf0);
/// `border-slate-300`: keycap chips, the slot confirm overlay, the disabled
/// outline, and the slider's disabled ink.
pub const SLATE_300: Color32 = rgb(0xca, 0xd5, 0xe2);
/// `text-slate-400`: idle chevrons, the pencil, and disabled figures.
pub const SLATE_400: Color32 = rgb(0x90, 0xa1, 0xb9);
/// `text-slate-500`: secondary copy and card subtitles.
pub const SLATE_500: Color32 = rgb(0x62, 0x74, 0x8e);
/// `text-slate-600`: the unselected mode segment, the icon ink, and the restore
/// affordance's label.
pub const SLATE_600: Color32 = rgb(0x45, 0x55, 0x6c);
/// `text-slate-700`: the keycap chip label. Chips stay neutral white so the
/// card's mode tint is the only colour signal.
pub const SLATE_700: Color32 = rgb(0x31, 0x41, 0x58);
/// `text-slate-800`: the "How it works" header, one step lighter than
/// [`SLATE_900`]. The reference keeps both in the same block -- a slate-800
/// title over slate-600 step copy -- so the two steps stay distinct rather than
/// collapsing to the body ink.
pub const SLATE_800: Color32 = rgb(0x1d, 0x29, 0x3d);
/// `text-slate-900`: every label and paragraph on a light surface.
pub const SLATE_900: Color32 = rgb(0x0f, 0x17, 0x2b);

// -- amber: the warning control and the dirty badge --

/// `ring-amber-100`: the measuring status dot's ring.
pub const AMBER_100: Color32 = rgb(0xfe, 0xf3, 0xc6);
/// `border-amber-200`: the dirty badge's edge step. Defined but unread -- the
/// badge's edge and its ink both take [`AMBER_200_90`], which is the same step
/// at 90% and the only one a painter ever sees.
pub const AMBER_200: Color32 = rgb(0xfe, 0xe6, 0x85);
/// `bg-amber-500`: the physical-overlap legend dot and the overlap figure.
pub const AMBER_500: Color32 = rgb(0xfe, 0x9a, 0x00);
/// `bg-amber-600`: the amber control's fill.
pub const AMBER_600: Color32 = rgb(0xe1, 0x71, 0x00);
/// `bg-amber-700` / `border-amber-700`: the amber control's edge and hover
/// fill, and the dirty badge's ink.
pub const AMBER_700: Color32 = rgb(0xbb, 0x4d, 0x00);

// -- red: invalid values, error copy, and the near-simultaneous legend --

/// `bg-red-50`: the invalid value-box fill.
pub const RED_50: Color32 = rgb(0xfe, 0xf2, 0xf2);
/// `border-red-400`: the invalid value-box edge.
pub const RED_400: Color32 = rgb(0xff, 0x64, 0x67);
/// `bg-red-500`: the near-simultaneous legend dot.
pub const RED_500: Color32 = rgb(0xfb, 0x2c, 0x36);
/// `text-red-600`: error and validation copy, the warning and duplicate-key
/// glyphs beside it, and the indistinguishable-overlap figure.
pub const RED_600: Color32 = rgb(0xe7, 0x00, 0x0b);

// -- emerald: the connected state and notice feedback --

/// `ring-emerald-100`: the synchronized status dot's ring.
pub const EMERALD_100: Color32 = rgb(0xd0, 0xfa, 0xe5);
/// `bg-emerald-500`: the step every emerald **mark** takes -- the connected
/// status dot, and each check glyph (beside a notice, on the Synchronized
/// badge, and on the assignment footer).
pub const EMERALD_500: Color32 = rgb(0x00, 0xbc, 0x7d);
/// `text-emerald-600`: notice feedback copy.
pub const EMERALD_600: Color32 = rgb(0x00, 0x99, 0x66);

// ---------------------------------------------------------------------------
// Mode ramps
// ---------------------------------------------------------------------------
//
// Each SOCD mode owns one Tailwind `-600` step, and every mode-coloured signal
// derives from it:
//
//   mode bases
//   ⌞blue-600    — Immediate
//   ⌞indigo-600  — Press Delay
//   ⌞purple-600  — Random Mix
//   ⌞violet-600  — Release Delay
//
//   | Consumer | Reads |
//   |---|---|
//   | `slot_tint` (card wash + edge) | the base, through `accent_tint` or `ramp_tint` |
//   | `slot_mode_ink` (mode label) | the base; the two delay modes reach one step darker (`-700`) |
//   | `mode_color` (preview, timeline accent) | the base |
//   | Keycap fill, arrow ink | the base |
//   | Keycap held/release glow | the ramp's `-300` step |
//
// A mode's base IS its ramp step, so a call site reads [`BLUE_600`] or
// [`VIOLET_600`] directly. A role name holding the same bytes
// (`IMMEDIATE_ACCENT`, `MIX_TEXT`) only made a `mode_color` match look like it
// drew from four unrelated sources.

// -- indigo: Press Delay, and every generic interactive signal --

/// `bg-indigo-50`: the Press Delay slot card's base, and the panel close
/// button's hover wash.
pub const INDIGO_50: Color32 = rgb(0xee, 0xf2, 0xff);
/// `bg-indigo-100`: the Press Delay slot card's active wash, taken at 70%.
pub const INDIGO_100: Color32 = rgb(0xe0, 0xe7, 0xff);
/// `border-indigo-200`: the Press Delay card's idle edge, taken at 70%.
pub const INDIGO_200: Color32 = rgb(0xc6, 0xd2, 0xff);
/// `ring-indigo-300` on a held keycap, and `hover:border-indigo-300` on every
/// outlined control. The reference writes the first as a ring class and the
/// second as a hover edge, but both are this one step, so the port names it
/// once.
pub const INDIGO_300: Color32 = rgb(0xa3, 0xb3, 0xff);
/// `border-indigo-400`: the keycap hover edge and the timeline badge's fill.
pub const INDIGO_400: Color32 = rgb(0x7c, 0x86, 0xff);
/// `bg-indigo-500`: the timeline overlap wash and the rebinding keycap step.
pub const INDIGO_500: Color32 = rgb(0x61, 0x5f, 0xff);
/// `bg-indigo-600` / `text-indigo-600` / `border-indigo-600`: the Press Delay
/// mode base, and the generic interactive accent every other control takes.
pub const INDIGO_600: Color32 = rgb(0x4f, 0x39, 0xf6);
/// `bg-indigo-700` / `border-indigo-700`: the primary control's hover fill and
/// edge, and the hover badge's pencil.
pub const INDIGO_700: Color32 = rgb(0x43, 0x2d, 0xd7);
/// `bg-indigo-800` / `hover:bg-indigo-800`: the Escape chip's hover fill.
pub const INDIGO_800: Color32 = rgb(0x37, 0x2a, 0xac);
/// `bg-indigo-950`: the timeline's overlap badge well, the darkest step any
/// surface uses.
pub const INDIGO_950: Color32 = rgb(0x1e, 0x1a, 0x4d);

// -- blue: Immediate --

/// `bg-blue-200`: the UP lane's held ring in the timeline canvas, carried as a
/// straight alpha over the step rather than a composite.
pub const BLUE_200: Color32 = rgb(0xbe, 0xdb, 0xff);
/// `ring-blue-300`: the UP keycap's held and rebinding glow.
pub const BLUE_300: Color32 = rgb(0x8e, 0xc5, 0xff);
/// `bg-blue-600`: the Immediate mode base, and the UP keycap's held fill.
pub const BLUE_600: Color32 = rgb(0x15, 0x5d, 0xfc);
/// `border-blue-700`: the Immediate badge's edge, the counterpart of the two
/// delay examples' `border-<ramp>-700/50`.
pub const BLUE_700: Color32 = rgb(0x14, 0x47, 0xe6);

// -- violet: Release Delay --

/// `bg-violet-50`: the Release Delay slot card's base.
pub const VIOLET_50: Color32 = rgb(0xf5, 0xf3, 0xff);
/// `bg-violet-100`: the Release Delay slot card's active wash, taken at 70%.
pub const VIOLET_100: Color32 = rgb(0xed, 0xe9, 0xfe);
/// `border-violet-200`: the Release Delay card's idle edge, taken at 70%.
pub const VIOLET_200: Color32 = rgb(0xdd, 0xd6, 0xff);
/// `border-violet-300`: the Release Delay card's hover edge.
pub const VIOLET_300: Color32 = rgb(0xc4, 0xb4, 0xff);
/// `border-violet-400`: the Release Delay card's active edge.
pub const VIOLET_400: Color32 = rgb(0xa6, 0x84, 0xff);
/// `bg-violet-500`: the preview's overlap dash and its selected example dot,
/// the Random Mix mixer's right rail, and the release-side value box. One step
/// lighter than the mode base ([`VIOLET_600`]), which is the reference's own
/// arrangement.
pub const VIOLET_500: Color32 = rgb(0x8e, 0x51, 0xff);
/// `text-violet-600`: the Release Delay mode base, and the DOWN keycap's fill.
pub const VIOLET_600: Color32 = rgb(0x7f, 0x22, 0xfe);
/// `text-violet-700`: the release-delay mode label's ink, one step darker than
/// the mode base ([`VIOLET_600`]). The release card's tint keeps [`VIOLET_500`];
/// only the label uses this darker step.
pub const VIOLET_700: Color32 = rgb(0x70, 0x08, 0xe7);

// -- purple: Random Mix --

/// `bg-purple-200`: the Random Mix lane's held ring in the timeline canvas.
pub const PURPLE_200: Color32 = rgb(0xe9, 0xd4, 0xff);
/// `ring-purple-300`: the held D keycap's outer ring, the purple counterpart
/// of [`INDIGO_300`].
pub const PURPLE_300: Color32 = rgb(0xda, 0xb2, 0xff);
/// `bg-purple-600`: the Random Mix mode base, the D keycap's held fill, and the
/// mixer control's ink.
pub const PURPLE_600: Color32 = rgb(0x98, 0x10, 0xfa);
/// `border-purple-700`: the held D keycap's edge, the purple counterpart of
/// the held A keycap's [`INDIGO_700`].
pub const PURPLE_700: Color32 = rgb(0x82, 0x00, 0xdb);

// ===========================================================================
// Surfaces
// ===========================================================================

/// `bg-white`: the page background behind the cards, and every card, control
/// shell, and slot tile. The page and the surfaces on it are the same white,
/// so one token names both rather than two role names for one value.
pub const WHITE: Color32 = rgb(0xff, 0xff, 0xff);
/// The group frame inside a card; the deep well (`bg-slate-50/80`).
pub const SLATE_50_80: Color32 = rgb(0xf9, 0xfb, 0xfd);

// ===========================================================================
// Slash-alpha tokens
// ===========================================================================
//
// A step under a slash alpha (`bg-indigo-50/60`) has no step name of its own,
// so the token takes the step's name and the alpha: `INDIGO_50_60` is
// `indigo-50/60`. A call site then reads the class it paints.
//
// The alpha scale is Tailwind's own, and it is closed. A slash alpha is one of
// these and nothing else:
//
//   /5  /10  /20  /25  /30  /40  /50  /60  /70  /80  /90  /95
//
// An alpha off that scale is drift, not a new value -- `/18` and `/26` were a
// hand-picked pair for the timeline's overlap wash and rounded to `/20` and
// `/25`. The achromatic pair uses the same scale (`white/40`, `black/5`), so
// there is one rule for every slash alpha in the window.
//
// A token whose alpha is 100 is not written here: an opaque step is just the
// step, so `slate-50/80` flattens to `SLATE_50_80` while a bare `slate-400` at
// 80% over white is [`SLATE_400_80`].

/// The main card's 2px edge (`border-indigo-200/80`).
pub const INDIGO_200_80: Color32 = rgb(0xd2, 0xdb, 0xff);
/// The hover wash every outlined control shares (`hover:bg-indigo-50/60`):
/// secondary buttons, mode segments, nav buttons, the preview pill, the
/// language rows, and the profile name box.
pub const INDIGO_50_60: Color32 = rgb(0xf5, 0xf7, 0xff);
/// The selected language row's fill (`bg-indigo-50/80`); its edge is
/// [`INDIGO_200`].
pub const INDIGO_50_80: Color32 = rgb(0xf1, 0xf5, 0xff);
/// The selected language row, hovered (`hover:bg-indigo-100/80`).
pub const INDIGO_100_80: Color32 = rgb(0xe6, 0xeb, 0xff);
/// The ink of the label beside an emerald mark: the "Synchronized" badge and
/// the assignment footer (`text-emerald-600/80`).
pub const EMERALD_600_80: Color32 = rgb(0x33, 0xad, 0x85);
/// The dirty badge's wash and edge (`bg-amber-50/70`, `border-amber-200/90`).
pub const AMBER_50_70: Color32 = with_alpha(rgb(0xff, 0xfc, 0xf1), 179);
pub const AMBER_200_90: Color32 = with_alpha(rgb(0xfe, 0xe8, 0x91), 230);

// ===========================================================================
// Translucent tokens
// ===========================================================================
//
// [`with_alpha`] is only for genuine translucency over a surface that is not
// known at definition time. These are the only such tokens.

/// The rebinding keycap's white inner ring (`white/40`).
pub const WHITE_40: Color32 = with_alpha(WHITE, 102);
/// Text selection over an accent (`indigo-600/25`).
pub const INDIGO_600_25: Color32 = with_alpha(INDIGO_600, 64);
/// The confirm overlay's wash (`white/95`).
pub const WHITE_95: Color32 = with_alpha(WHITE, 242);
/// The pressed keycap's `shadow-inner` band (`black/5`). epaint has no inset
/// shadow primitive, so two translucent inside strokes approximate the CSS
/// `inset 0 2px 4px rgb(0 0 0 / 0.05)` darkening along the box's inner edge.
pub const BLACK_5: Color32 = Color32::from_black_alpha(13);
/// The timeline's settled overlap wash, and a still-open one: a live overlap
/// reads stronger than a settled one (`indigo-500/20` and `indigo-500/25`).
pub const INDIGO_500_20: Color32 = with_alpha(INDIGO_500, 51);
pub const INDIGO_500_25: Color32 = with_alpha(INDIGO_500, 64);

/// System UI face, left generic on purpose: [`egui::FontFamily::Proportional`]
/// resolves whatever the platform context calls its default sans and walks its
/// fallback chain for glyphs that face lacks. The design source's reasoning for
/// not naming a family applies unchanged.
pub const UI_FONT: egui::FontFamily = egui::FontFamily::Proportional;
/// Generic monospace, for the profile slot keycaps. The reference puts its
/// `font-code` stack on every `<kbd>` (`[&_kbd]:font-code` on `<body>`), which
/// is what keeps all four chips the same width.
pub const MONO_FONT: egui::FontFamily = egui::FontFamily::Monospace;

/// Registered name of the native UI face: the Windows shell face, Segoe UI.
const NATIVE_UI_FACE: &str = "segoe-ui";
/// The regular-weight Segoe UI file inside the Windows font directory.
const NATIVE_UI_FACE_FILE: &str = "segoeui.ttf";

/// The Han fallback's registered name, and the Windows font files that can
/// supply it, most preferred first.
///
/// **Segoe UI carries no CJK glyphs**, and eframe's bundled `default_fonts`
/// (Ubuntu-Light, NotoEmoji, Hack) have none either, so a Chinese string --
/// `Language::name()`'s `中文`, or any of `zh.rs`'s translated values -- renders
/// as tofu/mojibake once Segoe UI leads the family. epaint walks a family's
/// face list in order and uses the first face that has the glyph, so a
/// CJK-capable face placed *behind* Segoe UI fixes the CJK strings without
/// touching the Latin ones.
///
/// The candidates are the Windows-shipped Simplified Chinese faces, then the
/// Traditional and Japanese ones: `msyh` (Microsoft YaHei, the shell's own
/// Simplified Chinese UI face and the best match for `zh.rs`), `msjh`
/// (JhengHei, Traditional), `simsun`, and `YuGoth` (Japanese). The first one
/// that reads wins; a machine with none of them keeps the pre-existing
/// behaviour rather than failing.
const NATIVE_HAN_FACE: &str = "native-han";
const NATIVE_HAN_FACE_FILES: &[&str] = &[
    "msyh.ttc",
    "msyh.ttf",
    "msjh.ttc",
    "simsun.ttc",
    "YuGothR.ttc",
];

/// The Hangul fallback's registered name, and the Windows font files that can
/// supply it, most preferred first.
///
/// This is a **second, independent** fallback rather than another entry in
/// [`NATIVE_HAN_FACE_FILES`], because the Han faces ship no Hangul at all:
/// Microsoft YaHei, JhengHei, SimSun and Yu Gothic carry roughly 29k glyphs
/// and not one of the 11,172 Hangul syllables. A single "first readable file
/// wins" list therefore let `msyh.ttc` shadow the Korean face, and every
/// Hangul string fell through to epaint's `◻` replacement glyph -- the tofu
/// the window showed for a Korean `io::Error` message.
///
/// Hangul arrives from the OS rather than the translation tables: on a Korean
/// Windows the runtime's `io::Error` text is Korean (`지정된 파일을 찾을 수
/// 없습니다. (os error 2)`), and the port renders that error verbatim, so the
/// window needs the glyphs whatever `Language` is selected. `malgun` is
/// Microsoft's own Hangul UI face and the only Windows face that carries both
/// Hangul and Han, so it is the whole list; a machine without it keeps the
/// pre-existing behaviour rather than failing.
const NATIVE_HANGUL_FACE: &str = "native-hangul";
const NATIVE_HANGUL_FACE_FILES: &[&str] = &["malgun.ttf", "batang.ttc", "gulim.ttc"];

/// The settings window's fonts: the native system UI face first, the Han and
/// Hangul fallbacks behind it, then eframe's bundled `default_fonts` faces
/// (F24).
///
/// egui has no OS font lookup, so the faces are read from `%WINDIR%\Fonts` when
/// this is called; a missing or unreadable file leaves the bundled set alone,
/// which is what the window rendered with before F24. epaint walks a family's
/// font list in order and uses the first face that has the glyph, so the bundled
/// faces behind the native ones stay the per-glyph fallback.
///
/// The bold face is deliberately not loaded: egui picks a face by
/// [`egui::FontFamily`], not by weight, so a second weight has no selection
/// path until the text styles can name a weighted family; emphasis keeps
/// [`stamp_galley`]'s approximation until then.
pub fn fonts() -> egui::FontDefinitions {
    compose_fonts(
        native_ui_font_bytes(),
        native_han_font_bytes(),
        native_hangul_font_bytes(),
    )
}

/// Composes the bundled definitions with the optional native faces. Split from
/// [`fonts`] so every branch is testable without the real system files.
///
/// Order within [`UI_FONT`] is the whole contract: the Latin face, then the Han
/// fallback, then the Hangul fallback, then the bundled faces. A CJK face ahead
/// of Segoe UI would take the Latin glyphs too, changing every measurement in
/// the window; the two CJK faces are kept apart because neither covers the
/// other's script (see [`NATIVE_HANGUL_FACE_FILES`]).
fn compose_fonts(
    native: Option<Vec<u8>>,
    han: Option<Vec<u8>>,
    hangul: Option<Vec<u8>>,
) -> egui::FontDefinitions {
    let mut definitions = egui::FontDefinitions::default();
    let mut lead: Vec<String> = Vec::new();
    for (name, bytes) in [
        (NATIVE_UI_FACE, native),
        (NATIVE_HAN_FACE, han),
        (NATIVE_HANGUL_FACE, hangul),
    ] {
        let Some(bytes) = bytes else {
            continue;
        };
        definitions
            .font_data
            .insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        lead.push(name.to_owned());
    }
    if !lead.is_empty() {
        let family = definitions.families.entry(UI_FONT).or_default();
        for (index, face) in lead.into_iter().enumerate() {
            family.insert(index, face);
        }
    }
    definitions
}

/// The Windows font directory, or `None` off Windows.
#[cfg(windows)]
fn windows_font_dir() -> std::path::PathBuf {
    std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
        .join("Fonts")
}

/// The native UI face's bytes from the Windows font directory, or `None` on
/// another platform or an install that does not ship Segoe UI.
fn native_ui_font_bytes() -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        std::fs::read(windows_font_dir().join(NATIVE_UI_FACE_FILE)).ok()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The first Han-capable face that reads from the Windows font directory, or
/// `None` on another platform or an install that ships none of the candidates.
///
/// The candidates are `.ttc` collections on most installs; epaint's
/// [`egui::FontData`] carries a face index for exactly that case, and face 0 is
/// the collection's regular weight.
fn native_han_font_bytes() -> Option<Vec<u8>> {
    first_readable_face(NATIVE_HAN_FACE_FILES)
}

/// The first Hangul-capable face that reads from the Windows font directory,
/// or `None` on another platform or an install that ships none of the
/// candidates. Kept apart from [`native_han_font_bytes`] because the Han faces
/// carry no Hangul, so one list cannot serve both scripts.
fn native_hangul_font_bytes() -> Option<Vec<u8>> {
    first_readable_face(NATIVE_HANGUL_FACE_FILES)
}

/// The first candidate file that reads, or `None` when none do.
fn first_readable_face(files: &[&str]) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        let dir = windows_font_dir();
        files
            .iter()
            .find_map(|file| std::fs::read(dir.join(file)).ok())
    }
    #[cfg(not(windows))]
    {
        let _ = files;
        None
    }
}

/// Body text size: the reference's `text-sm`, which is what its card copy and
/// the global body style both resolve to.
pub const BODY_TEXT_SIZE: f32 = 14.0;
/// Heading size: the reference's `sm:text-base`. The settings window is wider
/// than the `sm` breakpoint, so every card heading renders at this step.
pub const HEADING_SIZE: f32 = 16.0;

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
// The `+ 1.0` in these constants encodes the same correction the design source
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
/// edge and the segment is 28px tall in the reference: a 16px line box
/// (`leading-4`) over `2 * 6` padding.
///
/// The vertical padding is 6, the class's own value. It was briefly 7, on the
/// premise that a shaped galley at 12px measures 14 and the missing 2px had to
/// be folded into the padding to reach 28. That premise is stale: the label now
/// lays out at its full 16px, so the extra pixel on each side drew a 30px
/// segment -- 2px taller than the reference, and taller than the 28 this
/// constant exists to hold.
pub const MODE_PADDING: MarginF32 = MarginF32 {
    left: 8.0,
    right: 8.0,
    top: 6.0,
    bottom: 6.0,
};
/// The mode strip's own inset (`p-1`) and its segment gap (`gap-1`), both 4.
/// The strip is the one control the reference does not lay out with padding
/// classes on a `group_style` surface: it is `grid grid-cols-4 gap-1 p-1` on a
/// white card with a slate hairline and `shadow-2xs`, so the port reads the
/// inset from here rather than borrowing [`GROUP_PADDING`], which is the
/// card's 14.
pub const MODE_STRIP_PADDING: f32 = 4.0;
pub const MODE_STRIP_GAP: f32 = 4.0;

// Profile slot panel. The `+ 1` reading the design source derived for a nested
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
/// [`CHIP_SIZE`] wide while a longer label -- the `Scan code 0x{:02X}` fallback
/// from `physical_key_name` for a key the wire does not name -- grows its own
/// chip sideways. The height stays [`CHIP_SIZE`] for every chip, so one long
/// label cannot make the row taller.
pub const CHIP_CONTENT_MIN: f32 = CHIP_SIZE - CHIP_PADDING.left - CHIP_PADDING.right;

/// Corner radii, named after the controls that take them. A style closure
/// cannot carry them in egui, so they live here and the call site names one.
///
/// [`CIRCLE_RADIUS`] stands for the reference's `rounded-full`: egui clamps a
/// corner radius to half the box, so the maximum representable byte draws the
/// same circle any `999`-style literal would.
///
/// The 12 is the reference's `rounded-xl`, one class serving both the group
/// frames and the outlined controls. The port carried a separate
/// `GROUP_RADIUS` for it until the two were folded: one class, one name.
pub const CARD_RADIUS: CornerRadius = CornerRadius::same(16);
pub const CONTROL_RADIUS: CornerRadius = CornerRadius::same(12);
pub const SEGMENT_RADIUS: CornerRadius = CornerRadius::same(8);
pub const PILL_RADIUS: CornerRadius = CornerRadius::same(16);
pub const CHIP_RADIUS: CornerRadius = CornerRadius::same(6);
/// The timeline's overlap badge well, and the preview's delay badge: the
/// reference's `rounded` on a small status mark.
pub const BADGE_RADIUS: CornerRadius = CornerRadius::same(4);
pub const CIRCLE_RADIUS: CornerRadius = CornerRadius::same(255);

/// The slider handle's ring width, 3pt in the accent (or [`SLATE_300`] while
/// disabled). The single-handle rail width, rail radius, and handle radii are
/// retired with T12: the timing card draws its own two-handle rail and mixer
/// geometry and no longer consumes them.
pub const SLIDER_HANDLE_BORDER: f32 = 3.0;

// The D-pad centre tile's dots and glow. The tile's own fill, edge, and guide
// ring are already canonical here ([`SLATE_100`], [`SLATE_200`]); these are
// the values the tile painted inline.

/// The resting centre guide dot, `slate-300/60` flattened to opaque sRGB over
/// the tile.
///
/// The reference paints it from a literal (`bg-slate-300/60`), so the port
/// composites in sRGB to the bytes a browser shows ([`over`]'s rule) rather
/// than handing epaint a raw alpha.
pub const SLATE_300_60: Color32 = rgb(0xdf, 0xe6, 0xee);
/// The halo behind the active dot: `blue-600/25` flattened over white.
pub const BLUE_600_25: Color32 = rgb(0xc5, 0xd7, 0xfe);
/// The shadow under the active dot, one pixel below it: `blue-600/30`
/// flattened over white.
pub const BLUE_600_30: Color32 = rgb(0xb9, 0xce, 0xfe);
/// The shadow under the resting dot (`black/10`), a straight alpha because
/// the dot's own fill sits under it.
pub const BLACK_10: Color32 = Color32::from_black_alpha(26);
/// The resting dot itself: `slate-400/80` flattened over white.
pub const SLATE_400_80: Color32 = rgb(0xa6, 0xb4, 0xc7);

/// Shadows. Each name is the class the surface takes in the reference, because
/// that class is what decides the shape: `shadow-xs` for a resting control,
/// `shadow-2xs` for the faint hairline, and the two larger lifts for the banner
/// and the panel. The translucent alphas are the same colours quantized to a
/// byte (see the module note); the offsets and blur widths are egui's integer
/// units, which the reference's values all already sat on.
pub const SHADOW_CARD: Shadow = Shadow {
    offset: [0, 1],
    blur: 2,
    spread: 0,
    color: Color32::from_black_alpha(10),
};
/// Tailwind's `shadow-xs` (`0 1px 2px rgb(0 0 0 / 0.05)`): the lift every
/// resting control takes -- the keycaps on all three surfaces, the highlighted
/// delay badge, and the selected mode segment.
///
/// It is **not** [`SHADOW_2XS`]. That class is the reference's other hairline
/// drop and carries no blur; the two were told apart by reading back a rendered
/// shadow's own computed value, where a zero-blur drop shows one length rather
/// than two. Which class a surface takes is the surface's own reference class,
/// so these names follow the class and not the shape.
pub const SHADOW_XS: Shadow = Shadow {
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
/// Tailwind's `shadow-2xs` (`0 1px rgb(0 0 0 / 0.05)`): a **zero-blur** drop,
/// the reference's faintest hairline. The mode strip shell and its outlined
/// value pills carry it, and so do the white slot tiles the timing card groups
/// its ranges in (`p-3 rounded-2xl border-slate-200/70 shadow-2xs`) and the
/// "How it works" block.
///
/// Those last two are the elevation the port used to draw with a separate
/// `SHADOW_SLOT`: the same class, carried at the wrong blur. Folding the name
/// onto this one is what put the tiles back on the reference's own drop.
pub const SHADOW_2XS: Shadow = Shadow {
    offset: [0, 1],
    blur: 0,
    spread: 0,
    color: Color32::from_black_alpha(13),
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
/// The keycap's two glows: the 8pt rebinding ring and the 3pt pressed shadow.
/// The ring's colour is the per-direction accent's glow (`ring` at the call
/// site), so those two are composed there from [`SHADOW_XS`]'s shape with the
/// ring colour swapped in. The resting lift itself is [`SHADOW_XS`].
pub const KEYCAP_REBIND_GLOW_BLUR: u8 = 8;
pub const KEYCAP_PRESSED_GLOW_BLUR: u8 = 3;

/// The D-pad cap's outer ring widths: the reference's `ring-2` while pressed
/// and `ring-4` while rebinding, drawn in the direction's own ring step.
///
/// The same class draws the glow above, so the two are one effect -- the glow
/// is its penumbra and this is the hard edge inside it. The preview's 2px and
/// the timeline's 2.5px rings are local to their own references.
pub const KEYCAP_RING_PRESSED: f32 = 2.0;
pub const KEYCAP_RING_REBIND: f32 = 4.0;

/// White card on the canvas background. Reference: `bg-white border-2
/// border-indigo-200/80 rounded-2xl shadow-sm hover:border-indigo-300`.
/// [`CARD_STROKE`] is the frame's own stroke width, kept beside the frame so
/// layout reserves (the action bar's scroll-viewport reserve) derive from the
/// same constant instead of a second literal.
pub const CARD_STROKE: f32 = 2.0;
pub fn card_style() -> Frame {
    Frame::new()
        .fill(WHITE)
        .stroke(Stroke {
            width: 2.0,
            color: INDIGO_200_80,
        })
        .corner_radius(CARD_RADIUS)
        .shadow(SHADOW_CARD)
}

/// inset frame grouping related slots or slider sets.
pub fn group_style() -> Frame {
    Frame::new()
        .fill(SLATE_50_80)
        .stroke(Stroke {
            width: 1.0,
            color: SLATE_200,
        })
        .corner_radius(CONTROL_RADIUS)
}

/// Surface tile inside a group: the white rounded boxes that hold a timing
/// group, the preview, or the mechanism block (reference: rounded, thin
/// border, soft shadow).
pub fn slot_style() -> Frame {
    Frame::new()
        .fill(WHITE)
        .stroke(Stroke {
            width: 1.0,
            color: SLATE_200,
        })
        .corner_radius(CONTROL_RADIUS)
        .shadow(SHADOW_2XS)
}

/// Neutral rounded frame around the timeline canvas (reference:
/// `rounded-2xl border-slate-200/70`). It is the canvas's **recessed panel**,
/// so it takes [`group_style`]'s slate inset while the lanes drawn on it stay
/// white: the card body is white, the panel is the grey step, and the content
/// returns to white -- the same layering the timing and measurement cards use.
/// The lanes used to carry the grey and this frame the white, which read as
/// the one card in the window whose background was inverted.
pub fn graph_frame() -> Frame {
    Frame::new()
        .fill(SLATE_50_80)
        .stroke(Stroke {
            width: 1.0,
            color: SLATE_200,
        })
        .corner_radius(CARD_RADIUS)
}

/// Dropdown panel holding the profile slot cards (reference: `rounded-2xl
/// border-slate-200 shadow-2xl`). Main cards keep [`card_style`] with its
/// indigo border; the dropdown is neutral so the slot tints carry the color.
pub fn profile_panel() -> Frame {
    Frame::new()
        .fill(WHITE)
        .stroke(Stroke {
            width: 1.0,
            color: SLATE_200,
        })
        .corner_radius(CARD_RADIUS)
        .shadow(SHADOW_PANEL)
}

/// Profile slot card tinted by its mode and state.
pub fn tinted_slot(tint: SlotTint) -> Frame {
    Frame::new()
        .fill(tint.fill)
        .stroke(Stroke {
            width: 1.0,
            color: tint.border,
        })
        .corner_radius(CONTROL_RADIUS)
}

/// Bordered pill grouping the bare value editors on a timing row.
pub fn pill_style(invalid: bool) -> Frame {
    Frame::new()
        .fill(if invalid { RED_50 } else { WHITE })
        .stroke(Stroke {
            width: 1.0,
            color: if invalid { RED_400 } else { SLATE_200 },
        })
        .corner_radius(CONTROL_RADIUS)
}

/// Amber badge marking uncommitted draft edits in the action bar. Its ink is
/// [`AMBER_700`], taken at the label.
pub fn dirty_badge() -> Frame {
    Frame::new()
        .fill(AMBER_50_70)
        .stroke(Stroke {
            width: 1.0,
            color: AMBER_200_90,
        })
        .corner_radius(CONTROL_RADIUS)
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

/// Keycap interaction mode, from the keycap's own state machine. egui has no
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
/// [`over_card`] is the white-card special case; the preview badge's edge
/// composites a half-alpha `-700` step over its own fill instead.
pub const fn over(top: Color32, alpha: f32, bottom: Color32) -> Color32 {
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
    over(color, 0.75, WHITE)
}

/// A colour under the reference's `opacity-60`, composited toward the white
/// card it sits over.
///
/// Used by the preview's resting delay badge, whose whole pill -- fill, hairline
/// and ink -- carries `opacity-60`. Compositing in sRGB here matches what the
/// reference's browser renders; handing egui a raw 60% alpha would land paler
/// under a linear blend (the split [`over`] documents).
pub const fn over_card(color: Color32) -> Color32 {
    over(color, 0.60, WHITE)
}

/// Multiplies a color's RGB channels, for hover darkening of filled pills
/// (the reference's `preview_pill` used factors 0.88 and 0.8).
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
    let fill = over(accent, fill_alpha, WHITE);
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
    let base = over(fill, fill_alpha, WHITE);
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
        SocdMode::Immediate => accent_tint(BLUE_600, state),
        SocdMode::RandomMix => accent_tint(PURPLE_600, state),
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
        name: SLATE_900,
        pencil: SLATE_400,
        chip_label: SLATE_700,
        chip_edge: SLATE_300,
        hairline: SLATE_200,
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
        SocdMode::Immediate => BLUE_600,
        SocdMode::PressDelay => INDIGO_700,
        SocdMode::RandomMix => PURPLE_600,
        SocdMode::ReleaseDelay => VIOLET_700,
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
/// mapping mirrors the shared per-widget defaults:
///
/// - `panel_fill` is the canvas behind the cards; [`SLATE_50_80`] is the deep well
///   (`extreme_bg_color`) and [`SLATE_100`] the faint wash behind separators
///   and disabled rails.
/// - The interactive widget states carry the outlined-control pattern the
///   reference repeats across secondary buttons, mode segments, nav buttons,
///   and language rows: white shell and [`SLATE_200`] edge at rest, [`INDIGO_50_60`]
///   fill, [`INDIGO_300`] edge and [`INDIGO_600`] ink on hover *and*
///   press (the shared defaults grouped `Hovered | Pressed`). egui 0.36 has no
///   disabled colour set -- it fades disabled widgets via `Visuals::
///   disabled_alpha` -- so the reference's disabled pair ([`SLATE_300`] ink on
///   a [`WHITE`] shell, kept as named constants) must be applied by the
///   view modules where a control can actually be disabled.
/// - `noninteractive` carries body ink, so unstyled labels land on
///   `text-slate-900` the way the reference containers' `text_color` did.
///
/// Dispositions of those defaults that are *not* expressed here, because
/// egui styles them at the call site: `primary_button`, `warning_button`,
/// `banner_cancel_button`, `profile_close_button`, `active_option`,
/// `language_option`, `profile_name_button`, `mode_button`, `preview_pill`,
/// `nav_button`, `keycap` (their state colours are all named constants above,
/// plus [`SEGMENT_RADIUS`]/[`CONTROL_RADIUS`]/[`PILL_RADIUS`]/[`CIRCLE_RADIUS`]
/// and the shadow constants); `accent_slider`, `mixer_slider` (geometry above;
/// the rails are [`INDIGO_600`]/[`SLATE_100`], the mixer instead splits
/// [`INDIGO_600`] left of the handle and [`VIOLET_500`] right of it, with
/// no neutral [`SLATE_100`] track, ring [`PURPLE_600`], the handle [`WHITE`]
/// ringed 3pt, and the disabled ink [`SLATE_300`]);
/// `monitor_toggler` (track [`INDIGO_600`]/[`SLATE_300`], knob [`WHITE`]);
/// `value_input`, `facade_button`, `profile_name_input` (fills and edges above;
/// the selection tint is [`INDIGO_600_25`]); `table_rule` (a 1px [`SLATE_200`]
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

    // Fonts: the design source left the face generic on purpose, and egui's
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
    visuals.panel_fill = WHITE;
    visuals.extreme_bg_color = SLATE_50_80;
    visuals.faint_bg_color = SLATE_100;
    visuals.override_text_color = Some(SLATE_900);
    visuals.selection.bg_fill = INDIGO_600_25;
    // The selection's stroke is **not** an outline: `paint_text_selection`
    // reads its colour as the ink for the selected glyphs while painting
    // `bg_fill` behind them (`egui/src/text_selection/visuals.rs`). Leaving it
    // at [`Stroke::NONE`] therefore erased every selected character -- the wash
    // painted, the text turned transparent. Keep the ink the body colour so a
    // selection reads as highlighted text rather than a blank bar.
    //
    // `Stroke::NONE`'s *width* stays zero: egui also draws a selection outline
    // from this stroke in some widgets, and the reference has no outline.
    visuals.selection.stroke = Stroke::new(0.0, SLATE_900);

    let rest = &mut visuals.widgets.noninteractive;
    rest.bg_fill = WHITE;
    rest.weak_bg_fill = WHITE;
    rest.bg_stroke = Stroke {
        width: 1.0,
        color: SLATE_200,
    };
    rest.fg_stroke = Stroke {
        width: 1.0,
        color: SLATE_900,
    };
    rest.corner_radius = CONTROL_RADIUS;

    for state in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        state.bg_fill = WHITE;
        state.weak_bg_fill = WHITE;
        state.bg_stroke = Stroke {
            width: 1.0,
            color: SLATE_200,
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
        state.weak_bg_fill = INDIGO_50_60;
        state.bg_stroke = Stroke {
            width: 1.0,
            color: INDIGO_300,
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

/// The window's one glyph table. Paint-only: a kind, a rect, an ink.
///
/// Every icon the window paints is a variant here -- the action buttons, the
/// page headings, the header's three controls, the profile panel's close
/// circle, the measurement card's transport pair -- so a glyph is traced once
/// and a surface supplies only its box and its ink.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Icon {
    Keyboard,
    Restore,
    Edit,
    Check,
    Warning,
    Target,
    Timer,
    Revert,
    Chart,
    Measurement,
    Star,
    Play,
    Stop,
    ArrowForward,
    Layers,
    Languages,
    Power,
    Close,
}

/// The stroke a stroked glyph takes: one tenth of its box, floored at 1.2px so
/// a 12px glyph still reads. The direction arrows ([`super::keycap::paint_arrow`])
/// take the same rule.
pub fn icon_stroke(size: f32, color: Color32) -> Stroke {
    Stroke::new((size * 0.1).max(1.2), color)
}

/// The two plate glyphs' own step: the reference draws `Layers` and
/// `Languages` at 0.09 rather than 0.1.
fn icon_stroke_fine(size: f32, color: Color32) -> Stroke {
    Stroke::new((size * 0.09).max(1.2), color)
}

/// Trace one [`Icon`] into `rect` at `color`. The geometry is the mapping
/// card's, which is the geometry the icon widget drew.
pub fn paint_icon(painter: &Painter, rect: Rect, kind: Icon, color: Color32) {
    let size = rect.width().min(rect.height());
    let stroke = icon_stroke(size, color);
    let fine = icon_stroke_fine(size, color);
    let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
    let polyline = |stroke: Stroke, coordinates: &[(f32, f32)]| {
        painter.add(Shape::line(
            coordinates.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    let line = |coordinates: &[(f32, f32)]| polyline(stroke, coordinates);
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
            // The arc runs PI * 0.2 -> PI * 1.8 around the centre at r
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
        Icon::Target => {
            // The timeline card's heading mark: an outer ring at 0.35 of the
            // box with a filled centre at 35% of that radius.
            painter.circle_stroke(rect.center(), size * 0.35, stroke);
            painter.circle_filled(rect.center(), size * 0.35 * 0.35, color);
        }
        Icon::Timer => {
            // The timing card's heading mark: a stopwatch face with a top
            // button and two hands.
            line(&[(0.4, 0.0), (0.6, 0.0), (0.5, 0.2)]);
            painter.circle_stroke(point(0.5, 0.55), size * 0.35, stroke);
            line(&[(0.5, 0.55), (0.5, 0.55 - 0.35 * 0.6)]);
            line(&[(0.5, 0.55), (0.5 + 0.35 * 0.45, 0.55)]);
        }
        Icon::Revert => {
            // The turn-back arc is a cubic, and the arrowhead closes it.
            painter.add(Shape::CubicBezier(CubicBezierShape::from_points_stroke(
                [
                    point(0.85, 0.65),
                    point(0.85, 0.25),
                    point(0.4, 0.25),
                    point(0.25, 0.45),
                ],
                false,
                Color32::TRANSPARENT,
                stroke,
            )));
            line(&[(0.25, 0.25), (0.15, 0.45), (0.4, 0.55)]);
        }
        Icon::Chart => {
            // A baseline bar with three columns standing on it.
            let quad = |x: f32, y: f32, width: f32, height: f32| {
                painter.rect_filled(
                    Rect::from_min_size(point(x, y), Vec2::new(width * size, height * size)),
                    CornerRadius::ZERO,
                    color,
                );
            };
            quad(0.15, 0.8, 0.7, 0.1);
            for (x, height) in [(0.2095, 0.266), (0.423, 0.574), (0.6365, 0.406)] {
                quad(x, 0.85 - height, 0.154, height);
            }
        }
        Icon::Measurement => line(&[
            (0.15, 0.5),
            (0.3, 0.5),
            (0.42, 0.18),
            (0.6, 0.82),
            (0.72, 0.5),
            (0.85, 0.5),
        ]),
        Icon::Star => {
            // A ten-vertex star about the box centre: the outer points at 0.45
            // of the box, the inner ones at 0.2.
            let center = rect.center();
            let star = (0..10)
                .map(|index| {
                    let angle = -PI / 2.0 + index as f32 * PI / 5.0;
                    let radius = if index % 2 == 0 { 0.45 } else { 0.2 } * size;
                    Pos2::new(
                        center.x + angle.cos() * radius,
                        center.y + angle.sin() * radius,
                    )
                })
                .collect();
            painter.add(Shape::convex_polygon(star, color, Stroke::NONE));
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(
                vec![point(0.23, 0.15), point(0.85, 0.5), point(0.23, 0.85)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Stop => {
            painter.rect_filled(
                Rect::from_min_size(point(0.2, 0.2), Vec2::splat(0.6 * size)),
                CornerRadius::ZERO,
                color,
            );
        }
        Icon::ArrowForward => {
            line(&[(0.15, 0.5), (0.85, 0.5)]);
            line(&[(0.6, 0.25), (0.85, 0.5), (0.6, 0.75)]);
        }
        Icon::Layers => {
            // Three stacked plates: the top one closed, the lower two open
            // chevrons.
            polyline(
                fine,
                &[
                    (0.5, 0.12),
                    (0.88, 0.31),
                    (0.5, 0.5),
                    (0.12, 0.31),
                    (0.5, 0.12),
                ],
            );
            polyline(fine, &[(0.12, 0.5), (0.5, 0.69), (0.88, 0.5)]);
            polyline(fine, &[(0.12, 0.69), (0.5, 0.88), (0.88, 0.69)]);
        }
        Icon::Languages => {
            // The reference's translate glyph: six strokes in a 24-unit box.
            let parts: &[&[(f32, f32)]] = &[
                &[(8.0, 2.0), (8.0, 5.0)],
                &[(2.0, 5.0), (14.0, 5.0)],
                &[(4.0, 14.0), (10.0, 8.0), (12.0, 5.0)],
                &[(5.0, 8.0), (11.0, 14.0)],
                &[(12.0, 22.0), (17.0, 11.0), (22.0, 22.0)],
                &[(14.0, 18.0), (20.0, 18.0)],
            ];
            for part in parts {
                let coordinates: Vec<(f32, f32)> =
                    part.iter().map(|&(x, y)| (x / 24.0, y / 24.0)).collect();
                polyline(fine, &coordinates);
            }
        }
        Icon::Power => {
            // An arc open at the top, with the stem down its centre. The arc
            // is sampled because epaint shapes are polylines.
            let arc: Vec<Pos2> = (0..=24)
                .map(|step| {
                    let start = -PI / 2.0 + 0.6;
                    let end = -PI / 2.0 - 0.6 + 2.0 * PI;
                    let angle = start + (end - start) * (step as f32 / 24.0);
                    point(0.5 + angle.cos() * 0.3, 0.56 + angle.sin() * 0.3)
                })
                .collect();
            painter.add(Shape::line(arc, stroke));
            line(&[(0.5, 0.14), (0.5, 0.56)]);
        }
        Icon::Close => {
            line(&[(0.15, 0.15), (0.85, 0.85)]);
            line(&[(0.85, 0.15), (0.15, 0.85)]);
        }
    }
}

/// One line of canvas text, laid out colour-neutral at `size` in the UI face.
///
/// `PLACEHOLDER` is what makes the paint-time colour the one that renders: a
/// galley laid out with a real colour ignores the colour handed to
/// [`Painter::galley`].
pub fn line_galley(painter: &Painter, content: &str, size: f32) -> Arc<Galley> {
    painter.layout_no_wrap(
        content.to_owned(),
        FontId::new(size, UI_FONT),
        Color32::PLACEHOLDER,
    )
}

/// The measured advance of one line at `size`. egui's font layout provides the
/// real advance, so fit checks use it rather than an approximation.
pub fn line_width(painter: &Painter, content: &str, size: f32) -> f32 {
    line_galley(painter, content, size).size().x
}

/// One line painted centred on `center`; `bold` takes the [`stamp_galley`]
/// weight approximation.
pub fn centered_line(
    painter: &Painter,
    center: Pos2,
    content: &str,
    size: f32,
    color: Color32,
    bold: bool,
) {
    let galley = line_galley(painter, content, size);
    let pos = center - galley.size() / 2.0;
    if bold {
        stamp_galley(painter, pos, &galley, color, size);
    } else {
        painter.galley(pos, galley, color);
    }
}

/// The icon ink rule for a labelled action: the
/// a Restore glyph keeps the reference's slate-600 ink in every state, while
/// other action icons inherit the button's text colour so their hover states
/// keep working. The label itself follows the hover ink either way.
pub fn action_icon_ink(kind: Icon, label_ink: Color32) -> Color32 {
    if kind == Icon::Restore {
        SLATE_600
    } else {
        label_ink
    }
}

/// The outlined action's source dimensions: a 30px shell -- the reference's
/// `py-1.5` line (16px) plus 2*6 padding plus 2*1 border, the derivation the
/// the reference comment records beside [`BUTTON_PADDING`] -- with a 14px icon
/// and a 6px gap ahead of the 12px label. Horizontal padding is
/// [`BUTTON_PADDING`]'s 13pt per side; the corner is [`CONTROL_RADIUS`].
pub const BUTTON_HEIGHT: f32 = 30.0;
pub const BUTTON_TEXT_SIZE: f32 = 12.0;
pub const BUTTON_ICON: f32 = 14.0;
pub const BUTTON_ICON_GAP: f32 = 6.0;

/// The double-stamp weight approximation's offset: a second pass of the same
/// galley at `max(size * STAMP_OFFSET_FACTOR, STAMP_OFFSET_MIN)` px to the
/// right, the geometry [`stamp_galley`] renders with.
pub const STAMP_OFFSET_FACTOR: f32 = 0.04;
pub const STAMP_OFFSET_MIN: f32 = 0.35;

/// Paint one galley twice at a sub-pixel offset so it reads heavier.
///
/// This is the single owner the R2 round-1 issue 8 asked for: the local copy
/// `keycap.rs` carried was deleted in T12, and its call sites now resolve to
/// this function. It is **not** a blessing of
/// the approximation: egui selects a face by family, not by weight, so the
/// reference's bold/semibold families have no counterpart here (see the
/// module header) and `RichText::strong` only recolours. Whether to register
/// a weighted face and drop this, or record the stamp as the accepted
/// approach in `docs/architecture/ui.md`, is an open Master decision; no
/// bold-weight parity is claimed by anything here.
pub fn stamp_galley(painter: &Painter, pos: Pos2, galley: &Arc<Galley>, color: Color32, size: f32) {
    painter.galley(pos, galley.clone(), color);
    painter.galley(
        pos + Vec2::new((size * STAMP_OFFSET_FACTOR).max(STAMP_OFFSET_MIN), 0.0),
        galley.clone(),
        color,
    );
}

/// The drawn width of a dot row's mark: the reference's `w-2 h-2` span, which
/// is the same 8px circle in the measurement table and the timing groups.
pub const LEGEND_DOT: f32 = 8.0;

/// The gap between a dot row's mark and its label: the reference's `gap-1.5`,
/// which every `flex items-center gap-1.5` dot row in the source carries.
pub const LEGEND_GAP: f32 = 6.0;

/// The line box a label of `size` in [`UI_FONT`] occupies -- what a mark
/// beside it centres on, and what the row that holds both is sized by.
///
/// Read from the font rather than from a laid-out galley, so a caller that
/// only needs the line's height does not have to lay the text out to learn
/// it. The result carries the same physical-pixel rounding a galley's rows
/// get (`epaint::text::galley_from_rows`), so the two agree to the pixel
/// rather than to within half of one.
pub fn line_height(ui: &Ui, size: f32) -> f32 {
    let row_height = ui
        .ctx()
        .fonts_mut(|fonts| fonts.row_height(&FontId::new(size, UI_FONT)));
    let pixels_per_point = ui.ctx().pixels_per_point();
    egui::emath::GuiRounding::round_to_pixels(row_height, pixels_per_point)
}

/// Paint a dot row's mark: the small filled circle set ahead of the label,
/// into a box as tall as the label's line.
///
/// Every dot row in the reference is `flex items-center`, so the mark centres
/// on the **text's line box**, not on the row that holds the two. The two
/// agree only while the row is exactly one line tall; a row sized from the
/// mark itself, or one whose height has already collapsed, leaves the mark
/// high by half the difference between the mark and the line -- which is the
/// whole of the misalignment a reader sees. Sizing the box to the line puts
/// the mark's centre on the line whatever the row does.
///
/// `line_height` is the label's own line box: [`line_height`] for a caller
/// that has not laid the text out yet, or `galley.size().y` for one that has,
/// so the two agree to the pixel at every scale. The mark's **radius** stays
/// the caller's, because the table's 8px circle and the timing groups' 7px one
/// are deliberately different marks.
///
/// Returns the row it drew into, so a caller that needs to place something
/// else on the mark's centre line can read it rather than re-derive it.
pub fn legend_mark(ui: &mut Ui, color: Color32, radius: f32, line_height: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(LEGEND_DOT, line_height), Sense::hover());
    ui.painter().circle_filled(rect.center(), radius, color);
    rect
}

/// The outlined secondary action: white shell, [`SLATE_200`] edge,
/// [`SLATE_600`] label at rest; [`INDIGO_50_60`] fill, [`INDIGO_300`]
/// edge and [`INDIGO_600`] label on hover; a disabled ui drops the label to
/// [`SLATE_300`] and keeps the outline (the `secondary_button`
/// `Disabled` arm). egui additionally fades everything a
/// disabled ui paints by `Visuals::disabled_alpha`, so the rendered disabled
/// bytes are the spec pair under that fade -- a representation difference
/// against an explicit-only disabled arm, not a value change.
/// Paint-only: it draws, publishes the
/// accessible node, and hands back the [`Response`] -- mapping a click to a
/// `Message` stays the caller's job (migration constraint 2).
///
/// Ported from the verified integrated rendering in `src/ui/mapping.rs`
/// (R2 round 2 issue 2), which is itself the [`secondary_button`]
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
        (WHITE, SLATE_200, SLATE_300)
    } else if response.hovered() {
        (INDIGO_50_60, INDIGO_300, INDIGO_600)
    } else {
        (WHITE, SLATE_200, SLATE_600)
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
    //! Slot-card tint tests.
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
        BLACK_10, BLUE_600, BLUE_600_25, BLUE_600_30, SLATE_300, SLATE_300_60, SLATE_400,
        SLATE_400_80, SlotState, slot_ink, slot_ink_hovering, slot_mode_ink, slot_tint,
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
        assert_close(active.fill, "#E8EFFF", "Immediate active fill");
        assert_close(active.border, "#7FA6FE", "Immediate active border");

        let idle = slot_tint(SocdMode::Immediate, SlotState::Idle);
        assert_close(idle.fill, "#F6F9FF", "Immediate idle fill");
        assert_close(idle.border, "#D5E2FE", "Immediate idle border");
    }

    #[test]
    fn random_mix_tints_match_the_reference() {
        let hovered = slot_tint(SocdMode::RandomMix, SlotState::Hovered);
        assert_close(hovered.fill, "#F5E7FF", "Random Mix hovered fill");
        assert_close(hovered.border, "#D091FD", "Random Mix hovered border");

        let idle = slot_tint(SocdMode::RandomMix, SlotState::Idle);
        assert_close(idle.fill, "#FBF6FF", "Random Mix idle fill");
        assert_close(idle.border, "#ECD4FE", "Random Mix idle border");

        let active = slot_tint(SocdMode::RandomMix, SlotState::Active);
        assert_close(active.fill, "#F5E7FF", "Random Mix active fill");
        assert_close(active.border, "#C77CFD", "Random Mix active border");
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
        assert_close(hovered.border, "#C4B4FF", "Release Delay hovered border");

        let idle = slot_tint(SocdMode::ReleaseDelay, SlotState::Idle);
        assert_close(idle.fill, "#FCFBFF", "Release Delay idle fill");
        assert_close(idle.border, "#ECE9FF", "Release Delay idle border");
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
            "#155DFC",
            "Immediate label",
        );
        assert_close(
            slot_mode_ink(SocdMode::PressDelay, SlotState::Active),
            "#432DD7",
            "Press Delay label",
        );
        assert_close(
            slot_mode_ink(SocdMode::RandomMix, SlotState::Active),
            "#9810FA",
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
    fn widget_sourced_tokens_match_their_palette_bytes() {
        // The tokens canonicalised from widget code rather than the palette
        // table are pinned to the exact bytes their step renders.
        //
        // The disabled range-slider ink is the same `slate-300` the theme
        // carries: one ramp step, one token, no second value to drift.
        assert_eq!(SLATE_300.to_srgba_unmultiplied(), [0xca, 0xd5, 0xe2, 255]);
        // The D-pad dots and glow: `slate-300/60`, `slate-400/80`, and the
        // Immediate base at 25% and 30%, each composited in sRGB over white to
        // the bytes a screen shows.
        assert_eq!(
            SLATE_300_60.to_srgba_unmultiplied(),
            [0xdf, 0xe6, 0xee, 255]
        );
        assert_eq!(BLUE_600_25.to_srgba_unmultiplied(), [0xc5, 0xd7, 0xfe, 255]);
        assert_eq!(BLUE_600_30.to_srgba_unmultiplied(), [0xb9, 0xce, 0xfe, 255]);
        assert_eq!(BLACK_10, Color32::from_black_alpha(26));
        assert_eq!(
            SLATE_400_80.to_srgba_unmultiplied(),
            [0xa6, 0xb4, 0xc7, 255]
        );
        // The resting dot's base is `slate-400` at 80% over white, which is NOT
        // [`SLATE_400`]'s own value: the composite lightens it.
        assert_ne!(&SLATE_400.to_srgba_unmultiplied()[..3], &[0xa6, 0xb4, 0xc7]);
        // The Immediate base is the blue-600 step.
        assert_eq!(BLUE_600.to_srgba_unmultiplied(), [0x15, 0x5d, 0xfc, 255]);
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
        BODY_TEXT_SIZE, BUTTON_HEIGHT, BUTTON_ICON, BUTTON_ICON_GAP, BUTTON_PADDING, FontId,
        INDIGO_50_60, INDIGO_300, INDIGO_600, Icon, SLATE_200, SLATE_300, SLATE_600, UI_FONT,
        WHITE, action_icon_ink, icon_stroke, icon_stroke_fine, legend_mark, line_height,
        paint_icon, secondary_button,
    };
    use egui::{Color32, Rect, Sense, Shape, Vec2};
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

    /// A dot row's mark centres on the label's **line**, not on a box of its
    /// own. The two agree only while the row is exactly one line tall, which
    /// is what makes the mistake invisible in isolation and visible in the
    /// card: a row whose height has already collapsed leaves the mark half the
    /// difference between the mark and the line above the text. The mark is
    /// 8px and the line is a 14px one, so the retired box sat the full 4px
    /// high -- the offset this pins.
    ///
    /// The mark's own row is read back rather than assumed, so the assertion
    /// holds wherever the surrounding layout puts it.
    #[test]
    fn the_legend_mark_centres_on_the_line_it_is_given() {
        for line in [14.0_f32, 19.0, 20.5] {
            let row = std::cell::Cell::new(Rect::NOTHING);
            let mut harness = Harness::builder()
                .with_size(egui::vec2(200.0, 120.0))
                .build_ui(|ui| {
                    row.set(legend_mark(ui, INDIGO_600, 4.0, line));
                });
            harness.run();
            let row = row.get();

            let mark = harness
                .output()
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    Shape::Circle(circle) => Some(*circle),
                    _ => None,
                })
                .expect("the mark paints one circle");
            assert_eq!(mark.radius, 4.0, "the mark keeps the caller's radius");
            assert!(
                (row.height() - line).abs() <= 0.01,
                "the mark's row is the line it was given: {line}, got {}",
                row.height()
            );
            assert!(
                (mark.center.y - row.center().y).abs() <= 0.01,
                "the mark centres on its row: row centre {}, mark {}",
                row.center().y,
                mark.center.y
            );
        }
    }

    /// [`line_height`] is the height a label's galley will actually take, at
    /// the scale the window is rendering at. The check is against a real
    /// galley laid out at the same size, because that is the value the mark
    /// has to agree with; a font-metric read that skips the galley's own
    /// physical-pixel rounding lands up to half a pixel off it.
    #[test]
    fn line_height_matches_a_laid_out_galley() {
        for pixels_per_point in [1.0_f32, 1.5, 2.0] {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(400.0, 200.0))
                .with_pixels_per_point(pixels_per_point)
                .build_ui(move |ui| {
                    let read = line_height(ui, BODY_TEXT_SIZE);
                    let galley = ui.painter().layout_no_wrap(
                        "Neutral transition".to_owned(),
                        FontId::new(BODY_TEXT_SIZE, UI_FONT),
                        Color32::PLACEHOLDER,
                    );
                    assert!(
                        (read - galley.size().y).abs() <= 0.01,
                        "at {pixels_per_point} ppp: line_height read {read}, \
                         the galley takes {}",
                        galley.size().y
                    );
                });
            harness.run();
        }
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
            rest.iter().all(|color| *color == SLATE_600),
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
    fn the_disabled_button_takes_the_disabled_pair() {
        // The `secondary_button` Disabled arm: WHITE shell, SLATE_200
        // outline, SLATE_300 ink. The integrated mapping copy had no disabled
        // branch (both current call sites are always enabled); this is the
        // spec the shared owner restores, inert for enabled consumers.
        //
        // The rendered bytes are additionally faded by egui's global
        // `Visuals::disabled_alpha` (0.5 in `Visuals::light()`, applied to
        // every shape a disabled ui paints) -- a difference of representation
        // against an explicit-only disabled arm, which had no automatic fade.
        // The control's ink
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
    fn the_restore_icon_keeps_the_slate_ink_rule() {
        // Restore-only slate ink; other icons
        // inherit the button ink so hover reaches them.
        assert_eq!(action_icon_ink(Icon::Restore, INDIGO_600), SLATE_600);
        assert_eq!(action_icon_ink(Icon::Keyboard, INDIGO_600), INDIGO_600);
        // And the resting pair the control composes from:
        assert_eq!(action_icon_ink(Icon::Restore, SLATE_600), SLATE_600);
        assert_eq!(action_icon_ink(Icon::Check, SLATE_600), SLATE_600);
    }

    #[test]
    fn the_state_triples_are_the_named_tokens() {
        // The control composes its three states from the published constants
        // -- pin the pairs so a token edit cannot silently move one state.
        // (fill, edge, ink) rest / hover / disabled, as secondary_button
        // documents.
        let rest = (WHITE, SLATE_200, SLATE_600);
        let hovered = (INDIGO_50_60, INDIGO_300, INDIGO_600);
        let disabled = (WHITE, SLATE_200, SLATE_300);
        assert_ne!(rest.2, hovered.2, "hover must actually move the ink");
        assert_ne!(rest.2, disabled.2, "disabled must actually dim the ink");
        assert_eq!(rest.0, disabled.0, "both keep the WHITE shell");
    }

    /// Every glyph the window paints is a variant of [`Icon`], and each one
    /// traces something: a variant that painted nothing would be a silently
    /// blank control.
    #[test]
    fn every_icon_traces_a_shape() {
        for kind in [
            Icon::Keyboard,
            Icon::Restore,
            Icon::Edit,
            Icon::Check,
            Icon::Warning,
            Icon::Target,
            Icon::Timer,
            Icon::Revert,
            Icon::Chart,
            Icon::Measurement,
            Icon::Star,
            Icon::Play,
            Icon::Stop,
            Icon::ArrowForward,
            Icon::Layers,
            Icon::Languages,
            Icon::Power,
            Icon::Close,
        ] {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(40.0, 40.0))
                .build_ui(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    paint_icon(ui.painter(), rect, kind, INDIGO_600);
                });
            harness.run();
            let painted = harness.output().shapes.iter().any(|clipped| {
                fn any(shape: &Shape) -> bool {
                    match shape {
                        Shape::Vec(shapes) => shapes.iter().any(any),
                        Shape::Noop => false,
                        _ => true,
                    }
                }
                any(&clipped.shape)
            });
            assert!(painted, "{kind:?} must trace at least one shape");
        }
    }

    /// The stroke a glyph takes is one rule, so a 16px heading mark and a 12px
    /// pencil weigh the same at their own sizes and a small glyph never thins
    /// past the 1.2px floor.
    #[test]
    fn the_icon_stroke_follows_the_box_and_keeps_its_floor() {
        assert!((icon_stroke(20.0, INDIGO_600).width - 2.0).abs() < 0.001);
        assert!((icon_stroke(14.0, INDIGO_600).width - 1.4).abs() < 0.001);
        // Below 12px the tenth would drop under the floor.
        assert!((icon_stroke(8.0, INDIGO_600).width - 1.2).abs() < 0.001);
        // The two plate glyphs keep the reference's own 0.09 step.
        assert!((icon_stroke_fine(20.0, INDIGO_600).width - 1.8).abs() < 0.001);
    }
}

#[cfg(test)]
mod font_tests {
    //! F24: the theme leads with the native system face and keeps the bundled
    //! faces as its per-glyph fallback.

    use super::{
        NATIVE_HAN_FACE, NATIVE_HANGUL_FACE, NATIVE_UI_FACE, compose_fonts, fonts,
        native_han_font_bytes, native_hangul_font_bytes, native_ui_font_bytes,
    };
    use egui::{FontDefinitions, FontFamily, FontId, RawInput};
    use std::sync::Arc;

    /// The bundled set the fallback branch must reproduce.
    fn bundled() -> FontDefinitions {
        FontDefinitions::default()
    }

    /// Runs one frame so `set_fonts` takes effect; the atlas deltas are
    /// cleared because a context without a renderer never applies them.
    fn run_one_frame(ctx: &egui::Context) {
        let mut output = ctx.run_ui(RawInput::default(), |_ui| {});
        output.textures_delta.clear();
    }

    /// The bundled set with only the native face in the proportional family,
    /// so a live context can lay out with that face alone.
    fn native_only_definitions(bytes: Vec<u8>) -> FontDefinitions {
        let mut definitions = FontDefinitions::default();
        definitions.font_data.insert(
            NATIVE_UI_FACE.to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        definitions
            .families
            .insert(FontFamily::Proportional, vec![NATIVE_UI_FACE.to_owned()]);
        definitions
    }

    #[test]
    fn a_missing_native_face_leaves_the_bundled_set_untouched() {
        assert_eq!(compose_fonts(None, None, None), bundled());
    }

    #[test]
    fn the_native_face_leads_the_proportional_family_behind_the_bundled_fallback() {
        let definitions = compose_fonts(Some(vec![0, 1, 2, 3]), None, None);
        let proportional = &definitions.families[&FontFamily::Proportional];

        assert_eq!(
            proportional.first().map(String::as_str),
            Some(NATIVE_UI_FACE)
        );
        assert_eq!(
            &proportional[1..],
            bundled().families[&FontFamily::Proportional].as_slice(),
            "the bundled faces must stay in order behind the native one"
        );
        assert!(
            definitions.font_data.contains_key(NATIVE_UI_FACE),
            "the native face must be registered in font_data"
        );
    }

    /// Both CJK fallbacks belong *behind* the Latin face. Ahead of it, a CJK
    /// face would take every Latin glyph too and move every measurement in the
    /// window; absent, the Chinese and Korean strings render as tofu.
    #[test]
    fn the_cjk_fallbacks_sit_behind_the_latin_face_and_ahead_of_the_bundled_set() {
        let definitions = compose_fonts(
            Some(vec![0, 1, 2, 3]),
            Some(vec![4, 5, 6, 7]),
            Some(vec![8, 9, 10, 11]),
        );
        let proportional = &definitions.families[&FontFamily::Proportional];

        assert_eq!(
            proportional.first().map(String::as_str),
            Some(NATIVE_UI_FACE),
            "the Latin face must stay first"
        );
        assert_eq!(
            proportional.get(1).map(String::as_str),
            Some(NATIVE_HAN_FACE),
            "the Han fallback must be the second face"
        );
        assert_eq!(
            proportional.get(2).map(String::as_str),
            Some(NATIVE_HANGUL_FACE),
            "the Hangul fallback must be the third face"
        );
        assert_eq!(
            &proportional[3..],
            bundled().families[&FontFamily::Proportional].as_slice(),
            "the bundled faces must stay in order behind all three native ones"
        );
        assert!(
            definitions.font_data.contains_key(NATIVE_HAN_FACE),
            "the Han face must be registered in font_data"
        );
        assert!(
            definitions.font_data.contains_key(NATIVE_HANGUL_FACE),
            "the Hangul face must be registered in font_data"
        );
    }

    /// The two CJK fallbacks are independent of each other and of the Latin
    /// lead: the Hangul face must still register when the Han file is missing,
    /// because no Han face carries Hangul and one list cannot serve both.
    #[test]
    fn each_native_face_is_optional_and_independent() {
        let hangul_only = compose_fonts(None, None, Some(vec![8, 9, 10, 11]));
        assert_eq!(
            hangul_only.families[&FontFamily::Proportional]
                .first()
                .map(String::as_str),
            Some(NATIVE_HANGUL_FACE),
            "without the other faces the Hangul fallback still registers"
        );

        let han_only = compose_fonts(None, Some(vec![4, 5, 6, 7]), None);
        assert_eq!(
            han_only.families[&FontFamily::Proportional]
                .first()
                .map(String::as_str),
            Some(NATIVE_HAN_FACE),
            "without the Latin face the Han fallback still registers"
        );
        assert!(
            !han_only.font_data.contains_key(NATIVE_HANGUL_FACE),
            "without a Hangul file no Hangul face is registered"
        );

        let latin_only = compose_fonts(Some(vec![0, 1, 2, 3]), None, None);
        assert_eq!(
            latin_only.families[&FontFamily::Proportional]
                .first()
                .map(String::as_str),
            Some(NATIVE_UI_FACE)
        );
        assert!(
            !latin_only.font_data.contains_key(NATIVE_HAN_FACE),
            "without a Han file no Han face is registered"
        );
        assert!(
            !latin_only.font_data.contains_key(NATIVE_HANGUL_FACE),
            "without a Hangul file no Hangul face is registered"
        );
    }

    #[test]
    fn the_installed_definitions_lead_with_the_native_face() {
        let Some(bytes) = native_ui_font_bytes() else {
            // Without the system file the fallback branch is the contract.
            return;
        };

        let themed = egui::Context::default();
        themed.set_fonts(fonts());
        run_one_frame(&themed);

        let native_only = egui::Context::default();
        native_only.set_fonts(native_only_definitions(bytes));
        run_one_frame(&native_only);

        themed.fonts(|fonts| {
            assert_eq!(
                fonts.definitions().families[&FontFamily::Proportional]
                    .first()
                    .map(String::as_str),
                Some(NATIVE_UI_FACE),
                "the installed set must lead with the native family"
            );
        });

        // The native metrics are what layout uses: a glyph's advance equals a
        // context where only the native face is registered. A bundled first
        // face would land on the bundled metrics instead.
        let font = FontId::proportional(16.0);
        for glyph in ['W', 'i', 'M', 'g'] {
            assert_eq!(
                themed.fonts_mut(|fonts| fonts.glyph_width(&font, glyph)),
                native_only.fonts_mut(|fonts| fonts.glyph_width(&font, glyph)),
                "{glyph} must be laid out by the native face"
            );
        }
    }

    /// The point of the CJK fallback: a Chinese string must resolve to real
    /// glyphs, not the tofu box. Before the fallback every CJK codepoint fell
    /// through Segoe UI and the bundled faces to `.notdef`, which is what the
    /// Language window rendered as mojibake.
    ///
    /// The assertion is `has_glyph`, which is epaint's own answer to "can this
    /// face chain display this codepoint": it compares the face that resolves
    /// `c` against the chain's replacement face, so a codepoint no face
    /// carries reports `false`. `glyph_width` cannot stand in for it -- it
    /// returns `0.0` for a missing glyph rather than the `.notdef` box's
    /// advance, so a width comparison against a Latin-only context compares
    /// zero with zero and passes for the wrong reason.
    #[test]
    fn the_installed_definitions_resolve_cjk_glyphs() {
        if native_han_font_bytes().is_none() && native_hangul_font_bytes().is_none() {
            // A machine with none of the candidate faces keeps the old
            // behaviour, which is the documented fallback.
            return;
        }

        let themed = egui::Context::default();
        themed.set_fonts(fonts());
        run_one_frame(&themed);

        let font = FontId::proportional(16.0);
        for glyph in ['中', '文', '预', '览', '工', '作'] {
            assert!(
                themed.fonts_mut(|fonts| fonts.has_glyph(&font, glyph)),
                "{glyph} must resolve through the Han fallback rather than the tofu box"
            );
        }

        // And the Latin glyphs must be untouched: the fallback sits behind the
        // native face, so 'W' still measures as Segoe UI draws it.
        let Some(latin) = native_ui_font_bytes() else {
            return;
        };
        let latin_context = egui::Context::default();
        latin_context.set_fonts(native_only_definitions(latin));
        run_one_frame(&latin_context);
        for glyph in ['W', 'i', 'M', 'g'] {
            assert_eq!(
                themed.fonts_mut(|fonts| fonts.glyph_width(&font, glyph)),
                latin_context.fonts_mut(|fonts| fonts.glyph_width(&font, glyph)),
                "{glyph} must still be laid out by the native Latin face"
            );
        }
    }

    /// Hangul needs its own face, because no Han face carries a single Hangul
    /// syllable: Microsoft YaHei, JhengHei, SimSun and Yu Gothic all ship ~29k
    /// Han glyphs and zero Hangul. The old single "first readable file wins"
    /// list therefore stopped at `msyh.ttc` and every Korean string fell
    /// through to epaint's `◻` replacement glyph.
    ///
    /// Hangul is not a translation-table concern: on a Korean Windows the
    /// runtime's `io::Error` text is Korean, and the port renders that error
    /// verbatim, so this must hold whatever `Language` is selected.
    #[test]
    fn the_installed_definitions_resolve_hangul_glyphs() {
        if native_hangul_font_bytes().is_none() {
            // A machine with no Hangul face keeps the documented fallback.
            return;
        }

        let themed = egui::Context::default();
        themed.set_fonts(fonts());
        run_one_frame(&themed);

        // The literal the window showed as tofu: a Korean `io::Error` for
        // ERROR_FILE_NOT_FOUND, which is what a lone settings window renders.
        let font = FontId::proportional(16.0);
        for glyph in ['지', '정', '된', '파', '일', '없', '습', '니', '다'] {
            assert!(
                themed.fonts_mut(|fonts| fonts.has_glyph(&font, glyph)),
                "{glyph} must resolve through the Hangul fallback rather than the tofu box"
            );
        }
    }
}
