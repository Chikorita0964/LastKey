//! Palette and widget styles for the settings window.
//!
//! Values follow the React reference named by `docs/architecture/ui.md`.
//! Everything here uses generic system font families and iced's
//! own style structs, so the resident dependency tree is unaffected.

use iced::{
    Background, Border, Color, Font, Padding, Shadow, Theme, Vector,
    font::Weight,
    widget::{button, container, rule, slider, text_input, toggler},
};

const fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color::from_rgb(
        red as f32 / 255.0,
        green as f32 / 255.0,
        blue as f32 / 255.0,
    )
}

const CANVAS: Color = Color::WHITE;
const SURFACE: Color = Color::WHITE;
const INSET: Color = rgb(0xf9, 0xfb, 0xfd);
/// `border-slate-200`. The reference draws every neutral hairline and every
/// outlined control edge with this one class, so the card frame, the control
/// outline, and the dropdown border read it instead of keeping three
/// near-duplicate slates that drift apart.
const BORDER: Color = rgb(0xe2, 0xe8, 0xf0);
/// `text-slate-900`.
pub const BODY_TEXT: Color = rgb(0x0f, 0x17, 0x2b);
/// `text-slate-500`.
pub const MUTED_TEXT: Color = rgb(0x62, 0x74, 0x8e);
/// `text-amber-600`; the fill of an amber control is the same class colour.
pub const WARN_TEXT: Color = rgb(0xe1, 0x71, 0x00);
pub const AMBER_BUTTON: Color = rgb(0xe1, 0x71, 0x00);
/// `bg-amber-700` / `border-amber-700`, and the dirty badge's ink.
pub const AMBER_DARK: Color = rgb(0xbb, 0x4d, 0x00);
pub const RELEASE_TEXT: Color = rgb(0x8b, 0x5c, 0xf6);
pub const MIX_TEXT: Color = rgb(0x6d, 0x51, 0xee);

// Two colour lineages. The reference styles the DOM with Tailwind classes but
// passes raw hex to its icon and canvas components. Tailwind v4 re-specified
// the scale in oklch, so a class and the v3 hex of the same name no longer
// agree: beside `bg-indigo-600` (`#4f39f6`) sits `CanvasIcon color="#4f46e5"`,
// indigo-600 as v3 defined it. Nothing the reference paints with a class
// should take a drawn constant, and nothing it draws should take a DOM one.
/// Drawn indigo-600 (reference literal `#4f46e5`): icon strokes and fills.
pub const PRIMARY_TEXT: Color = rgb(0x4f, 0x46, 0xe5);
/// `bg-indigo-50`: the panel close button's hover wash.
const INDIGO_50: Color = rgb(0xee, 0xf2, 0xff);
/// `bg-indigo-600` / `text-indigo-600` / `border-indigo-600`.
pub const INDIGO_600: Color = rgb(0x4f, 0x39, 0xf6);
/// `bg-indigo-700` / `hover:bg-indigo-700` / `border-indigo-700`.
pub const INDIGO_700: Color = rgb(0x43, 0x2d, 0xd7);
/// `bg-emerald-500`: the connected status dot.
pub const EMERALD_500: Color = rgb(0x00, 0xbc, 0x7d);
/// `text-emerald-600`: notices and the latency table's median figures.
pub const EMERALD_600: Color = rgb(0x00, 0x99, 0x66);
/// `text-emerald-700`: the "all keys unique" footer.
pub const EMERALD_700: Color = rgb(0x00, 0x7a, 0x55);
/// `text-emerald-600/80`: the synchronized badge.
pub const EMERALD_SYNC: Color = rgb(0x33, 0xad, 0x84);
/// `bg-amber-500`: the physical-overlap legend dot.
pub const AMBER_500: Color = rgb(0xfe, 0x9a, 0x00);
/// `bg-red-500`: the near-simultaneous legend dot.
pub const RED_500: Color = rgb(0xfb, 0x2c, 0x36);
/// `text-red-600`: error and validation copy.
pub const RED_600: Color = rgb(0xe7, 0x00, 0x0b);
/// `bg-slate-100`: slider rails and the step-badge well.
pub const SLATE_100: Color = rgb(0xf1, 0xf5, 0xf9);
/// The Immediate accent. The reference hardcodes `#3a55e8` (its own blend of
/// blue-600 and indigo-600) to mark the only delay-free mode.
pub const IMMEDIATE_ACCENT: Color = rgb(0x3a, 0x55, 0xe8);
/// `bg-purple-600`: the preview's D keycap, the one place the reference uses
/// purple rather than the release delay's violet.
pub const PURPLE_600: Color = rgb(0x98, 0x10, 0xfa);
/// `bg-violet-500`: the preview's overlap dash.
pub const VIOLET_500: Color = rgb(0x8b, 0x5c, 0xf6);
/// `text-violet-600`: the release-delay range label.
pub const VIOLET_600: Color = rgb(0x7f, 0x22, 0xfe);
/// Drawn emerald-500 (reference literal `#10b981`) for check and warning
/// glyphs, which the reference colours with hex rather than a class.
pub const OK_TEXT: Color = rgb(0x10, 0xb9, 0x81);
/// Drawn red-600 (reference literal `#dc2626`) for warning glyphs.
pub const ERROR_TEXT: Color = rgb(0xdc, 0x26, 0x26);
/// Drawn green-600 (reference literal `#16a34a`), the unique-assignment check.
pub const GREEN_CHECK: Color = rgb(0x16, 0xa3, 0x4a);
/// Muted icon ink from the reference (`#94a3b8` for idle chevrons and the
/// power-off state, `#475569` for restore affordances). Text keeps
/// `MUTED_TEXT`; icons and the keycap sub-legends take this slate-400 so
/// they do not render darker than drawn.
pub const ICON_MUTED: Color = rgb(0x94, 0xa3, 0xb8);
pub const ICON_SECONDARY: Color = rgb(0x47, 0x55, 0x69);
/// `text-violet-700` (release-delay mode label). The card tint keeps
/// `RELEASE_TEXT`; only the label uses this darker ink.
pub const RELEASE_LABEL: Color = rgb(0x70, 0x08, 0xe7);
/// `text-slate-700` (keycap chip text). Chips stay neutral white so the
/// card's mode tint is the only color signal.
pub const CHIP_TEXT: Color = rgb(0x31, 0x41, 0x58);
/// `border-slate-300`: keycap chips, the slot confirm overlay, and the
/// disabled outline. The reference reuses the one class for all three.
pub const SLATE_300: Color = rgb(0xca, 0xd5, 0xe2);
/// `bg-slate-600` at full strength: the ink the reference's icons carry.
pub const SLATE_600: Color = rgb(0x45, 0x55, 0x6c);
/// `hover:border-indigo-300` on outlined controls and the profile name box.
const NAME_HOVER_BORDER: Color = rgb(0xa3, 0xb3, 0xff);
/// `bg-red-50` and `border-red-400` for invalid values and duplicate keys.
const ERROR_BG: Color = rgb(0xfe, 0xf2, 0xf2);
const ERROR_BORDER: Color = rgb(0xff, 0x64, 0x67);

/// System UI face, left generic on purpose. `Font::DEFAULT` is
/// `Family::SansSerif`, so the shaper resolves whatever the OS calls its
/// default sans and then walks its own fallback chain for glyphs that face
/// lacks — Hangul and CJK included. Naming a family instead (`Segoe UI`)
/// pins a face that does not exist on every target and reintroduces the
/// tofu the reference's font stack exists to avoid. Nothing is bundled
/// either way, so the runtime feature set is unchanged.
pub const UI_FONT: Font = Font::DEFAULT;
/// Bold system UI face for the status line and action-bar feedback.
pub const UI_FONT_BOLD: Font = Font {
    weight: Weight::Bold,
    ..UI_FONT
};
/// Semibold system UI face for the action bar's labels and status line
/// (reference `font-semibold`), one step below `UI_FONT_BOLD`.
pub const UI_FONT_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..UI_FONT
};
/// Italic system UI face for the draft-edit hint (reference `italic`).
pub const UI_FONT_ITALIC: Font = Font {
    style: iced::font::Style::Italic,
    ..UI_FONT
};
/// Heaviest system UI face for the D-pad keycap heroes (reference
/// `font-black`, weight 900).
pub const UI_FONT_BLACK: Font = Font {
    weight: Weight::Black,
    ..UI_FONT
};
/// Generic monospace, which Windows resolves without requiring a specific
/// family to be installed.
pub const MONO_FONT: Font = Font::MONOSPACE;

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
// iced lays a button out with `layout::padded` from padding alone -- it never
// consults `Border::width` -- and strokes the hairline inside those bounds.
// A control's box is therefore `content + 2 * padding`, and the drawn edge
// sits `border_width` outside the padding. Matching the reference's drawn
// edge means `iced_padding = css_padding + border_width`, which is what the
// `+ 1.0` in these constants encodes. Dropping it back to the CSS numbers
// shrinks every bordered control by 2px per axis.
//
// Verified: native action button measured 26px (16 + 2*5 from iced's
// `DEFAULT_PADDING`) against the reference's 30px, and the reference's
// border-top-width reports 1px there.

/// Outlined and filled action controls: `py-1.5 px-3` plus the 1px edge.
pub const BUTTON_PADDING: Padding = Padding {
    top: 7.0,
    bottom: 7.0,
    right: 13.0,
    left: 13.0,
};
/// Filled primary controls, which the reference widens to `px-4`.
pub const BUTTON_PADDING_WIDE: Padding = Padding {
    top: 7.0,
    bottom: 7.0,
    right: 17.0,
    left: 17.0,
};
/// Icon-only header buttons. The reference writes `px-2.5 py-1.5` plus an
/// explicit `h-[29px]`, so the height comes from that literal rather than from
/// padding: `py-1.5` plus the 1px edge would give 30px, and the reference
/// clips two of them back off. Match the literal, not the padding sum.
pub const HEADER_ICON_PADDING: Padding = Padding {
    top: 7.0,
    bottom: 7.0,
    right: 11.0,
    left: 11.0,
};
pub const HEADER_ICON_HEIGHT: f32 = 29.0;
/// Mode segments. `py-1.5 px-2` with no border, so nothing is added for an
/// edge and the segment is 28px tall in the reference. iced's rendered text
/// box for a 12px label is 14px here, so the vertical padding carries the
/// remaining 14px; measured 26px with 6, which is what this corrects.
pub const MODE_PADDING: Padding = Padding {
    top: 7.0,
    bottom: 7.0,
    right: 8.0,
    left: 8.0,
};

// Profile slot panel. The `+ 1` reading above applies to the panel's inner
// boxes too, and for a nested box it also has to reach the box's own edge: the
// reference's card is `box-sizing: border-box`, so its `p-3` and its 1px border
// both sit inside the 78px it measures, while iced adds only the padding to the
// content. `css_padding + border_width` is what reproduces the drawn box, which
// is why the card takes 13 rather than 12 -- and that same 13 is what lands its
// content on the reference's 336px row width inside a 362px card.
//
// An explicit width runs the other way. `layout::positioned` resolves the fixed
// length first and then expands the content box by the padding, so a declared
// width already includes its padding and takes no correction. The panel is 384
// for the same reason the reference's dialog is, and its 1px of padding is the
// border's 1px: the reference's header block sits exactly one pixel inside the
// dialog it belongs to.
/// Panel width (reference `w-96`), and the width of the language menu
/// (reference `w-48`).
pub const PROFILE_PANEL_WIDTH: f32 = 384.0;
pub const LANGUAGE_PANEL_WIDTH: f32 = 192.0;
/// The panel's own inset, standing in for the border it strokes inside itself.
pub const PANEL_PADDING: Padding = Padding {
    top: 1.0,
    right: 1.0,
    bottom: 1.0,
    left: 1.0,
};
/// Header block inset (reference `px-4 pt-4 pb-2` on the slot dialog).
pub const PROFILE_HEADER_PADDING: Padding = Padding {
    top: 16.0,
    right: 16.0,
    bottom: 8.0,
    left: 16.0,
};
/// Card scroller inset (reference `p-2.5`), and the tighter `p-1.5` the
/// reference uses around the language rows.
pub const PROFILE_SCROLLER_PADDING: Padding = Padding {
    top: 10.0,
    bottom: 10.0,
    right: 10.0,
    left: 10.0,
};
pub const LANGUAGE_SCROLLER_PADDING: Padding = Padding {
    top: 6.0,
    bottom: 6.0,
    right: 6.0,
    left: 6.0,
};
/// Gap between slot cards (reference `space-y-1.5`) and between language rows
/// (`space-y-0.5`).
pub const SLOT_GAP: f32 = 6.0;
pub const LANGUAGE_ROW_GAP: f32 = 2.0;
/// Between a card's name row and its keycap row (reference `mt-2.5`).
pub const SLOT_ROW_GAP: f32 = 10.0;
/// Slot card inset: `p-3` plus the 1px edge the reference counts inside it.
pub const SLOT_CARD_PADDING: f32 = 12.0 + 1.0;
/// Slot name box: reference `pl-2 pr-1 py-1` plus the same 1px edge, so the
/// padding is asymmetric -- the pencil side is tighter than the text side.
pub const SLOT_NAME_PADDING: Padding = Padding {
    top: 5.0,
    bottom: 5.0,
    right: 5.0,
    left: 9.0,
};
/// Heading block: the title row and its subtitle are `gap-1`, tighter than the
/// `gap-2` inside the title row itself.
pub const PROFILE_HEADER_GAP: f32 = 4.0;
/// Keycap chip (reference `px-1.5 py-0.5 leading-none` around a 10px label).
/// The chip box is pinned rather than derived from its padding because the
/// reference's `leading-none` line is shorter than the line box iced gives the
/// same label, and only the box decides the drawn height.
pub const CHIP_HEIGHT: f32 = 16.0;
pub const CHIP_PADDING: Padding = Padding {
    top: 0.0,
    bottom: 0.0,
    right: 7.0,
    left: 7.0,
};

/// Page background behind the cards.
pub fn canvas_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(CANVAS)),
        text_color: Some(BODY_TEXT),
        ..container::Style::default()
    }
}

/// White card on the canvas background. Reference: `bg-white border-2
/// border-indigo-200/80 rounded-2xl shadow-sm hover:border-indigo-300`.
pub fn card_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            color: rgb(0xd2, 0xdb, 0xff),
            width: 2.0,
            radius: 16.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.04),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 2.0,
        },
        ..container::Style::default()
    }
}

/// Inset frame grouping related slots or slider sets.
pub fn group_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(INSET)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// The D-pad's stage: the same inset surface as a group but with the
/// reference's larger corner radius (`rounded-2xl`) and roomier padding
/// (`p-5`), which is applied at the call site.
pub fn stage_style() -> container::Style {
    container::Style {
        border: Border {
            radius: 16.0.into(),
            ..group_style().border
        },
        ..group_style()
    }
}

/// The capture-mode banner: a solid indigo bar (reference `bg-indigo-600`)
/// with white text.
pub fn rebind_banner_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(INDIGO_600)),
        text_color: Some(Color::WHITE),
        border: Border {
            radius: 12.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Surface tile inside a group: the white rounded boxes that hold a timing
/// group, the preview, or the mechanism block (reference: rounded, thin
/// border, soft shadow).
pub fn slot_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.05),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 2.0,
        },
        ..container::Style::default()
    }
}

/// Neutral rounded frame around the timeline canvas (reference:
/// `rounded-2xl border-slate-200/70`). The canvas paints its own white
/// background, so this only supplies the border and the clipping radius.
pub fn graph_frame() -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 16.0.into(),
        },
        ..container::Style::default()
    }
}

/// The timeline's start/stop switch (reference: indigo when on, slate when
/// off, white knob). Disabled during the monitor's start and stop round trip.
pub fn monitor_toggler(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let track = match status {
        toggler::Status::Disabled { .. } => SLATE_300,
        toggler::Status::Active { is_toggled } | toggler::Status::Hovered { is_toggled } => {
            if is_toggled {
                INDIGO_600
            } else {
                SLATE_300
            }
        }
    };
    toggler::Style {
        background: track.into(),
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: SURFACE.into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        ..toggler::default(theme, status)
    }
}

/// Small bordered keycap chip in a profile slot card. The reference keeps
/// chips neutral white so the card's mode tint is the only color signal.
pub fn chip_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(Color { a: 0.8, ..SURFACE })),
        border: Border {
            color: SLATE_300,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..container::Style::default()
    }
}

/// Dropdown panel holding the profile slot cards (reference: `rounded-2xl
/// border-slate-200 shadow-2xl`). Main cards keep `card_style` with its
/// indigo border; the dropdown is neutral so the slot tints carry the color.
pub fn profile_panel() -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.15),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        ..container::Style::default()
    }
}

/// Slot name box (reference: white `rounded-lg border-slate-200` pill with
/// the name in slate-900 and a slate-400 pencil; hover deepens the border
/// and recolors the name to indigo). The pencil keeps its muted ink so the
/// idle state shows both inks; the name follows the button text color.
pub fn profile_name_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (border, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (NAME_HOVER_BORDER, INDIGO_600),
        _ => (BORDER, BODY_TEXT),
    };
    button::Style {
        background: Some(Background::Color(SURFACE)),
        text_color,
        border: Border {
            color: border,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.04),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 1.0,
        },
        ..button::Style::default()
    }
}

/// Confirm overlay covering a slot card (reference: `absolute inset-0
/// rounded-xl bg-white/95 border-slate-300`). Sits on top of the card via
/// `stack` so the card never changes height while confirming.
pub fn profile_confirm_overlay() -> container::Style {
    container::Style {
        background: Some(Background::Color(Color { a: 0.95, ..SURFACE })),
        border: Border {
            color: SLATE_300,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// Profile slot card tinted by its mode accent; the active slot deepens the
/// same hue instead of switching color. The two alphas are the reference's own
/// card classes read as paint rather than intent: its active card composites to
/// 0.40 over white and its idle cards to 0.05, and the idle ones then take
/// `opacity-75`. The lower figure is the one that was measured; the alpha the
/// active class alone would suggest is half again too faint -- the hover wash on
/// the load row comes from that button's own `ghost_button` style.
pub fn tinted_slot(accent: Color, active: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: if active { 0.40 } else { 0.05 },
            ..accent
        })),
        border: Border {
            color: Color {
                a: if active { 0.50 } else { 0.25 },
                ..accent
            },
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// Bordered pill grouping the bare value editors on a timing row.
pub fn pill_style(invalid: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(if invalid { ERROR_BG } else { SURFACE })),
        border: Border {
            color: if invalid { ERROR_BORDER } else { BORDER },
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// The D-pad's center tile: an inset well holding the resting guide and the
/// moving dot.
pub fn dpad_center() -> container::Style {
    container::Style {
        background: Some(Background::Color(INSET)),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 16.0.into(),
        },
        ..container::Style::default()
    }
}

/// Stroke color of the dashed guide ring inside the D-pad's center tile.
pub const GUIDE_RING: Color = BORDER;

/// Numbered step badge in the "How it works" block. The steps that carry
/// the mode's delay behavior are accent-tinted; the rest stay neutral.
pub fn step_badge(accent: Option<Color>) -> container::Style {
    let (background, text_color) = match accent {
        Some(color) => (Color { a: 0.12, ..color }, color),
        None => (INSET, MUTED_TEXT),
    };
    container::Style {
        background: Some(Background::Color(background)),
        text_color: Some(text_color),
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Small circle for the preview's example-position indicator.
pub fn example_dot(color: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(color)),
        border: Border {
            radius: 3.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Multiplies a color's RGB channels, for hover darkening of filled pills.
const fn shade(color: Color, factor: f32) -> Color {
    Color {
        r: color.r * factor,
        g: color.g * factor,
        b: color.b * factor,
        a: color.a,
    }
}

/// Rounded preview play/pause pill: filled with the example's accent while
/// playing, an outlined neutral pill while paused.
pub fn preview_pill(
    _theme: &Theme,
    status: button::Status,
    accent: Color,
    playing: bool,
) -> button::Style {
    if playing {
        let background = match status {
            button::Status::Hovered | button::Status::Pressed => shade(accent, 0.88),
            _ => accent,
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: SURFACE,
            border: Border {
                color: shade(accent, 0.8),
                width: 1.0,
                radius: 16.0.into(),
            },
            ..button::Style::default()
        }
    } else {
        let background = match status {
            button::Status::Hovered | button::Status::Pressed => rgb(0xf5, 0xf7, 0xff),
            _ => SURFACE,
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: BODY_TEXT,
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: 16.0.into(),
            },
            ..button::Style::default()
        }
    }
}

/// Circular, chromeless navigation button (preview previous/next).
/// Reference: `text-slate-400 hover:text-indigo-600 hover:bg-indigo-50`.
pub fn nav_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => {
            (Some(Background::Color(rgb(0xf5, 0xf7, 0xff))), INDIGO_600)
        }
        _ => (None, ICON_MUTED),
    };
    button::Style {
        background,
        text_color,
        border: Border {
            radius: 999.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// Hairline divider for the latency table, matching the card border.
pub fn table_rule(_theme: &Theme) -> rule::Style {
    rule::Style {
        color: BORDER,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

/// Amber badge marking uncommitted draft edits in the action bar.
pub fn dirty_badge() -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.7,
            ..rgb(0xff, 0xfc, 0xf1)
        })),
        text_color: Some(AMBER_DARK),
        border: Border {
            color: Color {
                a: 0.9,
                ..rgb(0xfe, 0xe8, 0x91)
            },
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// Small filled circle used for status and legend marks.
pub fn dot_style(color: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(color)),
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

pub fn keycap(
    status: button::Status,
    selected: bool,
    duplicate: bool,
    accent: Color,
) -> button::Style {
    let active = selected || status == button::Status::Pressed;
    button::Style {
        background: Some(
            if active {
                accent
            } else if duplicate {
                ERROR_BG
            } else {
                SURFACE
            }
            .into(),
        ),
        text_color: if active {
            SURFACE
        } else if duplicate {
            ERROR_TEXT
        } else {
            BODY_TEXT
        },
        border: Border {
            color: if duplicate {
                ERROR_BORDER
            } else if active || status == button::Status::Hovered {
                accent
            } else {
                BORDER
            },
            width: 2.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.05),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 1.0,
        },
        ..button::Style::default()
    }
}

pub fn mode_button(status: button::Status, selected: bool) -> button::Style {
    if selected {
        button::Style {
            background: Some(Background::Color(INDIGO_600)),
            text_color: Color::WHITE,
            border: Border {
                radius: 8.0.into(),
                ..Border::default()
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.06),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 2.0,
            },
            ..button::Style::default()
        }
    } else {
        let (background, text_color) = match status {
            button::Status::Hovered | button::Status::Pressed => {
                (Some(Background::Color(rgb(0xf5, 0xf7, 0xff))), INDIGO_600)
            }
            _ => (None, SLATE_600),
        };
        button::Style {
            background,
            text_color,
            border: Border {
                radius: 8.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

pub fn mixer_slider(theme: &Theme, status: slider::Status) -> slider::Style {
    let mut style = accent_slider(theme, status);
    style.rail.backgrounds.1 = RELEASE_TEXT.into();
    style.handle.border_color = MIX_TEXT;
    style
}

/// Filled accent button for the one primary action on a screen. Reference:
/// `bg-indigo-600 hover:bg-indigo-700 text-white border-indigo-700`, and a
/// white outlined shell once the action is unavailable.
pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color, border_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (INDIGO_700, Color::WHITE, INDIGO_700),
        button::Status::Active => (INDIGO_600, Color::WHITE, INDIGO_700),
        button::Status::Disabled => (SURFACE, SLATE_300, BORDER),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..button::Style::default()
    }
}

/// Filled amber warning button for active measurement (stop action).
/// Reference: `bg-amber-600 hover:bg-amber-700 text-white border-amber-700`.
pub fn warning_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color, border_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (AMBER_DARK, Color::WHITE, AMBER_DARK),
        button::Status::Active => (AMBER_BUTTON, Color::WHITE, AMBER_DARK),
        button::Status::Disabled => (SURFACE, SLATE_300, BORDER),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..button::Style::default()
    }
}

/// The capture banner's ESC chip: a darker indigo fill (reference
/// `bg-indigo-700`) with white text, on top of the banner itself.
pub fn banner_cancel_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => INDIGO_700,
        _ => INDIGO_600,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::WHITE,
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// Outlined button for secondary actions. Reference: white shell, slate-200
/// outline, slate-600 label; hover turns the label indigo-600 and the shell
/// indigo-50/60 with an indigo-300 edge; disabled drops the label to
/// slate-300 and keeps the outline.
pub fn secondary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color, border_color) = match status {
        button::Status::Hovered | button::Status::Pressed => {
            (rgb(0xf5, 0xf7, 0xff), INDIGO_600, NAME_HOVER_BORDER)
        }
        button::Status::Active => (SURFACE, SLATE_600, BORDER),
        button::Status::Disabled => (SURFACE, SLATE_300, BORDER),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..button::Style::default()
    }
}

/// Transparent clickable region for the profile slot's load row: no chrome
/// of its own, just a faint accent wash on hover.
pub fn ghost_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Some(Background::Color(Color {
            a: 0.06,
            ..INDIGO_600
        })),
        _ => None,
    };
    button::Style {
        background,
        text_color: BODY_TEXT,
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// Panel close button: a borderless circle rather than the outlined secondary
/// button the rest of the UI uses. Reference: `rounded-full` on a transparent
/// shell, with a `bg-indigo-50/70` wash and indigo ink on hover. The reference
/// writes the size on the box (`h-9 w-9 p-2` on the slot dialog, `h-7 w-7` on
/// the language menu), so callers set it rather than the padding.
pub fn profile_close_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (
            Some(Background::Color(Color {
                a: 0.7,
                ..INDIGO_50
            })),
            INDIGO_600,
        ),
        button::Status::Active | button::Status::Disabled => (None, ICON_MUTED),
    };
    button::Style {
        background,
        text_color,
        border: Border {
            radius: 9999.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// The hairline dividing the two keycap pairs inside a slot card (reference:
/// `border-l border-slate-200` on the second pair). Drawn as a filled box rather
/// than `rule::vertical`: iced gives that a `Length::Fill` height, so it
/// stretches to the whole row and inflates the card from the reference's 78px to
/// 150px -- measured, before this replaced it. A `border-l` in CSS is sized to
/// its content instead, which is the 16px of a keycap chip.
pub fn pair_divider() -> container::Style {
    container::Style {
        background: Some(Background::Color(BORDER)),
        ..container::Style::default()
    }
}

/// The selected row in the language menu: a light accent fill instead of the
/// plain secondary outline. Reference: `border-indigo-200 bg-indigo-50/80
/// text-indigo-700 hover:bg-indigo-100/80 hover:border-indigo-300`.
pub fn active_option(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => rgb(0xe6, 0xeb, 0xff),
        _ => rgb(0xf1, 0xf5, 0xff),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: INDIGO_700,
        border: Border {
            color: rgb(0xc6, 0xd2, 0xff),
            width: 1.0,
            radius: 12.0.into(),
        },
        ..button::Style::default()
    }
}

/// The unselected row in the language menu: the shell stays open on the
/// panel surface with no edge of its own, and hover supplies the accent.
pub fn language_option(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color, border_color) = match status {
        button::Status::Hovered | button::Status::Pressed => {
            (rgb(0xf5, 0xf7, 0xff), INDIGO_600, NAME_HOVER_BORDER)
        }
        _ => (SURFACE, SLATE_600, Color::TRANSPARENT),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..button::Style::default()
    }
}

/// Accent slider: filled rail up to the handle, hollow after it.
pub fn accent_slider(_theme: &Theme, status: slider::Status) -> slider::Style {
    let handle_radius = match status {
        slider::Status::Active => 7.0,
        slider::Status::Hovered | slider::Status::Dragged => 8.0,
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(PRIMARY_TEXT),
                Background::Color(SLATE_100),
            ),
            width: 10.0,
            border: Border {
                radius: 5.0.into(),
                ..Border::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle {
                radius: handle_radius,
            },
            background: Background::Color(SURFACE),
            border_width: 3.0,
            border_color: PRIMARY_TEXT,
        },
    }
}

/// Press-to-edit facade mirroring a live value box (`value_input` Active
/// visuals). Both are borderless: the surrounding `pill_style` container
/// owns the chrome. Pressing the facade reveals the real input already
/// focused and selected.
pub fn facade_button(
    _theme: &Theme,
    status: button::Status,
    accent: Color,
    invalid: bool,
) -> button::Style {
    let text_color = if invalid { ERROR_TEXT } else { accent };
    let background = match status {
        button::Status::Hovered => Some(Background::Color(Color { a: 0.08, ..accent })),
        button::Status::Pressed => Some(Background::Color(Color { a: 0.14, ..accent })),
        _ => None,
    };
    button::Style {
        background,
        text_color,
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

/// Numeric entry box inside a timing pill: borderless, accent-colored,
/// transparent over the pill's own chrome.
pub fn value_input(
    _theme: &Theme,
    _status: text_input::Status,
    accent: Color,
    invalid: bool,
) -> text_input::Style {
    let value = if invalid { ERROR_TEXT } else { accent };
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        placeholder: MUTED_TEXT,
        value,
        selection: Color { a: 0.25, ..value },
    }
}
