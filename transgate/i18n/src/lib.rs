//! Strict translation of core [`Msg`]s for the native and terminal shells (P8 D3).
//!
//! View models carry message keys and typed arguments; the web translates them with
//! i18next over `transgate/web/locales/<locale>/<ns>.json`. This crate embeds the
//! same catalogs and applies the same policy (`locales/config.json`): **no fallback
//! language**, literal namespace and key lookup (no key or namespace separators),
//! `{{name}}` interpolation without escaping, a numeric `count` choosing the
//! `<key>_<category>` plural variant (then the plain key), and nested message
//! arguments translated first. Anything else is an error, never a quiet default:
//! a missing key or argument is a bug the catalog validator or a test must catch.

mod plural;

use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use transgate_core::presentation::msg::{Arg, Msg, Ns};

pub use plural::category as plural_category;

/// The UI locales, in `locales/config.json` order.
pub const LOCALES: [&str; 16] = ["en", "tr", "pt", "es", "de", "id", "it", "fr", "el", "ru", "da", "sv", "no", "fi", "kk", "az"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranslateError {
    UnknownLocale(String),
    MissingKey { locale: String, ns: &'static str, key: String },
    MissingArgument { locale: String, ns: &'static str, key: String, name: String },
}

impl fmt::Display for TranslateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TranslateError::UnknownLocale(locale) => write!(f, "unknown locale {locale:?}"),
            TranslateError::MissingKey { locale, ns, key } => write!(f, "missing translation {locale}/{ns}: {key:?}"),
            TranslateError::MissingArgument { locale, ns, key, name } => {
                write!(f, "missing argument {name:?} for {locale}/{ns}: {key:?}")
            }
        }
    }
}

impl std::error::Error for TranslateError {}

type Catalog = HashMap<String, String>;

macro_rules! sources {
    ($($locale:literal),* $(,)?) => {
        fn raw(locale: &str, ns: Ns) -> Option<&'static str> {
            match locale {
                $($locale => Some(match ns {
                    Ns::Common => include_str!(concat!("../../web/locales/", $locale, "/common.json")),
                    Ns::App => include_str!(concat!("../../web/locales/", $locale, "/app.json")),
                    Ns::Ai => include_str!(concat!("../../web/locales/", $locale, "/ai.json")),
                    Ns::Auth => include_str!(concat!("../../web/locales/", $locale, "/auth.json")),
                }),)*
                _ => None,
            }
        }
    };
}

sources!("en", "tr", "pt", "es", "de", "id", "it", "fr", "el", "ru", "da", "sv", "no", "fi", "kk", "az");

static CATALOGS: LazyLock<HashMap<(&'static str, Ns), Catalog>> = LazyLock::new(|| {
    let mut all = HashMap::new();
    for locale in LOCALES {
        for ns in Ns::ALL {
            let text = raw(locale, ns).expect("every UI locale embeds every namespace");
            let catalog: Catalog = serde_json::from_str(text).unwrap_or_else(|e| panic!("{locale}/{}.json is not a flat string map: {e}", ns.file()));
            all.insert((locale, ns), catalog);
        }
    }
    all
});

fn catalog(locale: &str, ns: Ns) -> Result<&'static Catalog, TranslateError> {
    let locale = LOCALES.iter().find(|l| **l == locale).ok_or_else(|| TranslateError::UnknownLocale(locale.to_string()))?;
    Ok(&CATALOGS[&(*locale, ns)])
}

/// Whether `key` is in the catalog of `locale` and `ns` exactly as written (no plural suffix).
pub fn has_key(locale: &str, ns: Ns, key: &str) -> Result<bool, TranslateError> {
    Ok(catalog(locale, ns)?.contains_key(key))
}

/// Translates `msg` into `locale`, or says exactly what is missing.
pub fn translate(locale: &str, msg: &Msg) -> Result<String, TranslateError> {
    let catalog = catalog(locale, msg.ns)?;
    let missing = || TranslateError::MissingKey { locale: locale.to_string(), ns: msg.ns.file(), key: msg.key.to_string() };
    // i18next tries the plural variant first, then the plain key.
    let template = match msg.args.get("count") {
        Some(Arg::Number(count)) => {
            let plural = format!("{}_{}", msg.key, plural::category(locale, *count));
            catalog.get(&plural).or_else(|| catalog.get(msg.key.as_ref()))
        }
        _ => catalog.get(msg.key.as_ref()),
    }
    .ok_or_else(missing)?;
    interpolate(locale, msg, template)
}

fn interpolate(locale: &str, msg: &Msg, template: &str) -> Result<String, TranslateError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}") else { break };
        out.push_str(&rest[..open]);
        let name = rest[open + 2..open + 2 + close].trim();
        let arg = msg.args.get(name).ok_or_else(|| TranslateError::MissingArgument {
            locale: locale.to_string(),
            ns: msg.ns.file(),
            key: msg.key.to_string(),
            name: name.to_string(),
        })?;
        match arg {
            Arg::Number(n) => out.push_str(&n.to_string()),
            Arg::Text(text) => out.push_str(text),
            Arg::Msg(nested) => out.push_str(&translate(locale, nested)?),
        }
        rest = &rest[open + 2 + close + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The `{{name}}` placeholder names of `template`, trimmed, in order of appearance.
pub fn placeholders(template: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}") else { break };
        names.push(rest[open + 2..open + 2 + close].trim());
        rest = &rest[open + 2 + close + 2..];
    }
    names
}

/// Every `(locale, ns, key, template)` of the embedded catalogs, for validators and tests.
pub fn entries() -> impl Iterator<Item = (&'static str, Ns, &'static str, &'static str)> {
    LOCALES.into_iter().flat_map(|locale| {
        Ns::ALL.into_iter().flat_map(move |ns| CATALOGS[&(locale, ns)].iter().map(move |(key, template)| (locale, ns, key.as_str(), template.as_str())))
    })
}

#[cfg(test)]
mod tests;
