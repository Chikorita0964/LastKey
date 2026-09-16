//! Session-local UI language: the message strings and the language panel.
//!
//! Call sites pass their English string and a language file answers with the
//! localized form or nothing. Runtime diagnostics and user-defined names are
//! never translated.
//!
//! One file per language, `en.rs` included: it maps every string to itself,
//! so it doubles as the canonical inventory to diff a new language against.
//!
//! Adding a language is one file plus two lines. Copy `en.rs` to `<code>.rs`,
//! translate the values, add a variant to [`Language`], and add its arm to
//! `translate`. A string the new file omits falls back to the English source
//! rather than rendering blank, so a partial translation ships safely.
//!
//! # Why this is `language.rs` and not `language/mod.rs`
//!
//! T6 owns the language panel, and its file list names `src/ui2/language.rs`.
//! Rust cannot have both `language.rs` and `language/mod.rs`, so the module
//! entry moved into this file; the per-language files stay under
//! `src/ui2/language/`. `src/ui2/mod.rs` already declares `pub mod language;`,
//! so the move needs no registration change and this file compiles on the
//! task branch before the integration step registers `header`/`profiles`.

use egui::{
    Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo,
    WidgetType,
};

use super::{message::Message, state::State, theme};

mod en;
mod es;
mod zh;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Language {
    #[default]
    English,
    Chinese,
    Spanish,
}

impl Language {
    pub const ALL: [Self; 3] = [Self::English, Self::Chinese, Self::Spanish];

    /// Endonym, shown in the language list.
    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Chinese => "中文",
            Self::Spanish => "Español",
        }
    }

    pub fn text(self, source: &str) -> &str {
        self.translate(source).unwrap_or(source)
    }

    fn translate(self, source: &str) -> Option<&'static str> {
        match self {
            Self::English => en::text(source),
            Self::Chinese => zh::text(source),
            Self::Spanish => es::text(source),
        }
    }
}

/// Draws the language panel's rows, one full-width option per [`Language::ALL`]
/// with the session's language carrying the trailing check mark. Port of
/// `profile_slots`'s languages arm (src/ui/app.rs:1615-1649): the scroller's
/// `space-y-0.5` gap, the selected row's active-option pair, and the plain row
/// that stays open on the panel surface.
pub fn language_rows(ui: &mut Ui, state: &State) -> Vec<Message> {
    let mut messages = Vec::new();
    ui.spacing_mut().item_spacing.y = theme::LANGUAGE_ROW_GAP;
    for language in Language::ALL {
        if language_row(ui, state, language).clicked() {
            messages.push(Message::SelectLanguage(language));
        }
    }
    messages
}

/// One row: the reference's `px-2.5 py-2` insets around a 12px bold label and
/// a 12px check, with a 6px gap at the trailing edge. The label is painted so
/// the row keeps one owner for its fill, edge and ink; the double-stamp
/// follows the port's bold approximation (`theme::stamp_galley`), which the
/// open font-weight decision owns.
fn language_row(ui: &mut Ui, state: &State, language: Language) -> Response {
    /// `px-2.5` plus the 1px edge the reference counts inside the box.
    const PAD_X: f32 = 10.0;
    /// `py-2`.
    const PAD_Y: f32 = 8.0;
    const LABEL_SIZE: f32 = 12.0;
    const CHECK: f32 = 12.0;

    let selected = state.language == language;
    let galley = layout_label(ui, language.name(), LABEL_SIZE);
    let height = galley.size().y + 2.0 * PAD_Y;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    response
        .widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), language.name()));

    let hovered = response.contains_pointer();
    let (fill, edge, ink) = if selected {
        let fill = if hovered {
            theme::ACTIVE_OPTION_HOVER_BG
        } else {
            theme::ACTIVE_OPTION_BG
        };
        (fill, theme::INDIGO_200, theme::INDIGO_700)
    } else if hovered {
        (
            theme::HOVER_WASH,
            theme::NAME_HOVER_BORDER,
            theme::INDIGO_600,
        )
    } else {
        (theme::SURFACE, Color32::TRANSPARENT, theme::SLATE_600)
    };

    let painter = ui.painter();
    painter.rect(
        rect,
        theme::CONTROL_RADIUS,
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    let text_pos = Pos2::new(rect.left() + PAD_X, rect.center().y - galley.size().y / 2.0);
    theme::stamp_galley(painter, text_pos, &galley, ink, LABEL_SIZE);
    if selected {
        let check_rect = Rect::from_center_size(
            Pos2::new(rect.right() - PAD_X - CHECK / 2.0, rect.center().y),
            Vec2::splat(CHECK),
        );
        theme::paint_icon(painter, check_rect, theme::Icon::Check, theme::PRIMARY_TEXT);
    }
    response
}

/// One measured line at the shared proportional family, laid out in
/// `PLACEHOLDER` so the paint colour is the one handed to the painter.
fn layout_label(ui: &Ui, text: &str, size: f32) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::proportional(size),
        Color32::PLACEHOLDER,
    )
}
