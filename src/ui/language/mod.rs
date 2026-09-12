//! Session-local UI language. Call sites pass their English string and a
//! language file answers with the localized form or nothing. Runtime
//! diagnostics and user-defined names are never translated.
//!
//! One file per language, `en.rs` included: it maps every string to itself,
//! so it doubles as the canonical inventory to diff a new language against.
//!
//! Adding a language is one file plus two lines. Copy `en.rs` to `<code>.rs`,
//! translate the values, add a variant to [`Language`], and add its arm to
//! `translate`. A string the new file omits falls back to the English source
//! rather than rendering blank, so a partial translation ships safely.

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
