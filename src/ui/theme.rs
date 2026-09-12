//! Palette and widget styles for the settings window.
//!
//! Values follow the React reference named by `docs/architecture/ui.md`.
//! Everything here uses generic system font families and iced's
//! own style structs, so the resident dependency tree is unaffected.

use iced::{
    Background, Border, Color, Font, Shadow, Theme, Vector,
    font::Weight,
    widget::{button, container, rule, slider, text_input},
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
const INSET: Color = rgb(0xf8, 0xfa, 0xfc);
const BORDER: Color = rgb(0xe1, 0xe4, 0xe8);
pub const BODY_TEXT: Color = rgb(0x0f, 0x17, 0x2a);
pub const MUTED_TEXT: Color = rgb(0x64, 0x74, 0x8b);
pub const PRIMARY_TEXT: Color = rgb(0x4f, 0x46, 0xe5);
pub const RELEASE_TEXT: Color = rgb(0x8b, 0x5c, 0xf6);
pub const MIX_TEXT: Color = rgb(0x6d, 0x51, 0xee);
pub const WARN_TEXT: Color = rgb(0xca, 0x8a, 0x04);
pub const OK_TEXT: Color = rgb(0x10, 0xb9, 0x81);
pub const ERROR_TEXT: Color = rgb(0xdc, 0x26, 0x26);

const PRIMARY_DARK: Color = rgb(0x43, 0x38, 0xca);
const HOVER_SURFACE: Color = rgb(0xf8, 0xfa, 0xfc);
const KBD_BORDER: Color = rgb(0xd1, 0xd5, 0xdb);
const RAIL_INACTIVE: Color = rgb(0xcb, 0xd5, 0xe1);
const ERROR_BG: Color = rgb(0xfe, 0xf2, 0xf2);
const ERROR_BORDER: Color = rgb(0xfc, 0xa5, 0xa5);

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

/// Page background behind the cards.
pub fn canvas_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(CANVAS)),
        text_color: Some(BODY_TEXT),
        ..container::Style::default()
    }
}

/// White card on the canvas background.
pub fn card_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            color: rgb(0xc7, 0xd2, 0xfe),
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

/// Surface tile inside a group, used for a single key row.
pub fn slot_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE)),
        border: Border {
            width: 0.0,
            radius: 6.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
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

pub fn mode_button(status: button::Status, selected: bool, accent: Color) -> button::Style {
    if selected {
        button::Style {
            background: Some(accent.into()),
            ..tab_selected(&Theme::Light, status)
        }
    } else {
        tab_unselected(&Theme::Light, status)
    }
}

pub fn mixer_slider(theme: &Theme, status: slider::Status) -> slider::Style {
    let mut style = accent_slider(theme, status);
    style.rail.backgrounds.1 = RELEASE_TEXT.into();
    style.handle.border_color = MIX_TEXT;
    style
}

/// Filled accent button for the one primary action on a screen.
pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => PRIMARY_DARK,
        button::Status::Active => PRIMARY_TEXT,
        button::Status::Disabled => RAIL_INACTIVE,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: SURFACE,
        border: Border {
            color: PRIMARY_DARK,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..button::Style::default()
    }
}

/// Outlined button for secondary actions.
pub fn secondary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => (rgb(0xee, 0xf2, 0xff), PRIMARY_TEXT),
        button::Status::Active => (SURFACE, BODY_TEXT),
        button::Status::Disabled => (SURFACE, MUTED_TEXT),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..button::Style::default()
    }
}

/// The selected tab in the view switch: a raised pill inside the inset well.
pub fn tab_selected(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(PRIMARY_TEXT)),
        text_color: SURFACE,
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.06),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 2.0,
        },
        ..button::Style::default()
    }
}

/// The unselected tab: flat against the well until hovered.
pub fn tab_unselected(_theme: &Theme, status: button::Status) -> button::Style {
    let text_color = match status {
        button::Status::Hovered | button::Status::Pressed => BODY_TEXT,
        _ => MUTED_TEXT,
    };
    button::Style {
        background: None,
        text_color,
        border: Border {
            width: 0.0,
            radius: 6.0.into(),
            ..Border::default()
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
                Background::Color(RAIL_INACTIVE),
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

/// Muted slider for a disabled timing group. The slider widget has no
/// disabled state, so this gray rendering pairs with `update` ignoring its
/// drag messages while the group is off. The thumb keeps showing the stored
/// value instead of disappearing with the control.
pub fn muted_slider(_theme: &Theme, _status: slider::Status) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(RAIL_INACTIVE),
                Background::Color(RAIL_INACTIVE),
            ),
            width: 4.0,
            border: Border {
                radius: 2.0.into(),
                ..Border::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 7.0 },
            background: Background::Color(RAIL_INACTIVE),
            border_width: 2.0,
            border_color: SURFACE,
        },
    }
}

/// Press-to-edit facade mirroring a live value box (`value_input` Active
/// visuals). Only enabled boxes get a facade; pressing it reveals the real
/// input already focused and selected.
pub fn facade_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => HOVER_SURFACE,
        button::Status::Pressed => INSET,
        button::Status::Active | button::Status::Disabled => SURFACE,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: PRIMARY_TEXT,
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..button::Style::default()
    }
}

/// Press-to-edit facade over invalid input. Mirrors `facade_button` on a
/// light-red background so an untouched box still flags a bad value.
pub fn facade_button_error(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Pressed => INSET,
        _ => ERROR_BG,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: ERROR_TEXT,
        border: Border {
            color: ERROR_BORDER,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..button::Style::default()
    }
}

/// Numeric entry box paired with a slider, or docked to the preserve toggle.
pub fn value_input(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let (background, border_color, value) = match status {
        text_input::Status::Focused { .. } => (SURFACE, PRIMARY_TEXT, PRIMARY_TEXT),
        text_input::Status::Hovered => (SURFACE, KBD_BORDER, PRIMARY_TEXT),
        text_input::Status::Active => (SURFACE, BORDER, PRIMARY_TEXT),
        text_input::Status::Disabled => (INSET, BORDER, MUTED_TEXT),
    };
    text_input::Style {
        background: Background::Color(background),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 6.0.into(),
        },
        placeholder: MUTED_TEXT,
        value,
        selection: Color {
            a: 0.25,
            ..PRIMARY_TEXT
        },
    }
}

/// Value box holding invalid input: light-red background and border with a
/// red value. Only used while the group is enabled; disabled boxes keep the
/// gray `Disabled` look of `value_input`.
pub fn value_input_error(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let background = match status {
        text_input::Status::Disabled => INSET,
        _ => ERROR_BG,
    };
    text_input::Style {
        background: Background::Color(background),
        border: Border {
            color: ERROR_BORDER,
            width: 1.0,
            radius: 6.0.into(),
        },
        placeholder: MUTED_TEXT,
        value: ERROR_TEXT,
        selection: Color {
            a: 0.25,
            ..ERROR_TEXT
        },
    }
}
