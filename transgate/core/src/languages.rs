//! The language lists of the current app (PRD 7.4, LNG rules).
//!
//! `languages_data.rs` is generated from `transgate/fixtures/languages`; the
//! tests below assert the generated tables are supersets of those fixtures.
//! Jobs store the language name (the database enum value), providers receive
//! the lower case ISO code (LNG-017, TRX-023).

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[path = "languages_data.rs"]
mod data;

pub use data::{LANGUAGES, UI_LOCALES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanguageEntry {
    /// The stored database enum name, for example `Chinese`.
    pub name: &'static str,
    /// Lower case ISO code sent to every provider.
    pub code: &'static str,
    /// Declared Soniox (and Soniox v5) support (LNG-015, TRX-011).
    pub soniox: bool,
    /// Declared Deepgram support (LNG-016, TRX-012). A source capability claim,
    /// not provider acceptance evidence.
    pub deepgram: bool,
    /// One of the 53 translation languages (LNG-012, TRL-003).
    pub translation: bool,
    pub flag: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiLocale {
    pub code: &'static str,
    pub name: &'static str,
    pub default: bool,
}

/// One of the 61 offered languages. Serialized as its stored name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "string"))]
pub struct Language(u8);

impl Language {
    /// Exact stored name, as `jobs.language` holds it.
    pub fn from_name(name: &str) -> Option<Language> {
        LANGUAGES.iter().position(|l| l.name == name).map(|i| Language(i as u8))
    }

    /// `toTransgateLanguage`: an ISO code in any letter case to the stored name.
    /// Empty or unknown codes give `None`.
    pub fn from_code(code: &str) -> Option<Language> {
        if code.is_empty() {
            return None;
        }
        let lower = code.to_lowercase();
        LANGUAGES.iter().position(|l| l.code == lower).map(|i| Language(i as u8))
    }

    pub fn entry(self) -> &'static LanguageEntry {
        &LANGUAGES[self.0 as usize]
    }

    pub fn name(self) -> &'static str {
        self.entry().name
    }

    pub fn code(self) -> &'static str {
        self.entry().code
    }

    pub fn soniox(self) -> bool {
        self.entry().soniox
    }

    pub fn deepgram(self) -> bool {
        self.entry().deepgram
    }

    pub fn translation_capable(self) -> bool {
        self.entry().translation
    }

    pub fn flag(self) -> &'static str {
        self.entry().flag
    }

    pub fn is_rtl(self) -> bool {
        is_rtl(Some(self.name()))
    }

    pub fn all() -> impl Iterator<Item = Language> {
        (0..LANGUAGES.len()).map(|i| Language(i as u8))
    }

    /// The 53 languages offered as translation source and target (LNG-012).
    pub fn translation_languages() -> impl Iterator<Item = Language> {
        Language::all().filter(|l| l.translation_capable())
    }
}

impl fmt::Debug for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Serialize for Language {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Language::from_name(&name).ok_or_else(|| serde::de::Error::custom(format!("unknown language {name:?}")))
    }
}

/// `isRTL` (LNG-007): case-insensitive, trimmed; `farsi` stays as the source
/// alias even though no stored language has that name (LNG-D12).
pub fn is_rtl(language: Option<&str>) -> bool {
    match language {
        None => false,
        Some(l) => {
            let normalized = l.trim().to_lowercase();
            matches!(normalized.as_str(), "arabic" | "hebrew" | "persian" | "farsi" | "urdu")
        }
    }
}

pub fn ui_locale(code: &str) -> Option<&'static UiLocale> {
    UI_LOCALES.iter().find(|l| l.code == code)
}

pub fn default_ui_locale() -> &'static UiLocale {
    UI_LOCALES.iter().find(|l| l.default).expect("one default locale")
}

#[cfg(test)]
mod tests;
