//! Copy as message keys (P5 D10). Presentation never returns display text:
//! a view model carries a [`Msg`] naming a key in one of the source's i18next
//! namespaces, plus typed interpolation arguments, and each shell translates
//! it with its catalog (`transgate/web/locales/<locale>/<ns>.json`).
//!
//! Keys are the source's own keys: natural-language English where the source
//! used English as its key, dotted paths where it nested. New keys for states
//! the source never had live in [`Ns::App`] and are listed with their English
//! meaning in `docs/plans/p5-web/implementation/core.md`.
//!
//! Wire shape: `{"ns":"app","key":"…","args":{"name":"Ada","count":3}}`;
//! `args` is always present, `{}` without arguments. A numeric `count` drives the source's plural
//! variants. [`Arg::Msg`] is translated by the shell first and the result is
//! interpolated, so a language or status name stays a key too.

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The catalog files P5 renders, by file stem. `common`, `app` and `ai` are
/// source catalogs; `auth` holds the source sign-in labels that `signin.tsx`
/// declared inline. Language display names are `common` keys, as the source
/// pickers used them. P6-only catalogs (`howto`, `tools`, SEO, keyword and
/// language-page files) are not reachable from the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Ns {
    Common,
    App,
    Ai,
    Auth,
}

impl Ns {
    pub const ALL: [Ns; 4] = [Ns::Common, Ns::App, Ns::Ai, Ns::Auth];

    /// The catalog file stem.
    pub fn file(self) -> &'static str {
        match self {
            Ns::Common => "common",
            Ns::App => "app",
            Ns::Ai => "ai",
            Ns::Auth => "auth",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum Arg {
    Number(#[cfg_attr(feature = "ts", ts(type = "number"))] i64),
    Text(String),
    Msg(Box<Msg>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Msg {
    pub ns: Ns,
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub key: Cow<'static, str>,
    /// Always present; empty without arguments.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "BTreeMap<String, Arg>"))]
    pub args: BTreeMap<Cow<'static, str>, Arg>,
}

impl Msg {
    /// A key without arguments.
    pub const fn new(ns: Ns, key: &'static str) -> Msg {
        Msg { ns, key: Cow::Borrowed(key), args: BTreeMap::new() }
    }

    pub fn text(mut self, name: &'static str, value: impl Into<String>) -> Msg {
        self.args.insert(Cow::Borrowed(name), Arg::Text(value.into()));
        self
    }

    pub fn number(mut self, name: &'static str, value: i64) -> Msg {
        self.args.insert(Cow::Borrowed(name), Arg::Number(value));
        self
    }

    pub fn nested(mut self, name: &'static str, value: Msg) -> Msg {
        self.args.insert(Cow::Borrowed(name), Arg::Msg(Box::new(value)));
        self
    }

    /// The `count` argument i18next uses to pick a plural variant.
    pub fn count(self, n: i64) -> Msg {
        self.number("count", n)
    }
}

pub const fn common(key: &'static str) -> Msg {
    Msg::new(Ns::Common, key)
}

pub const fn app(key: &'static str) -> Msg {
    Msg::new(Ns::App, key)
}

pub const fn ai(key: &'static str) -> Msg {
    Msg::new(Ns::Ai, key)
}

pub const fn auth(key: &'static str) -> Msg {
    Msg::new(Ns::Auth, key)
}

/// A language's display name: its English name as a `common` key
/// (`LanguageDropdown`, `MobileLanguageSheet`, `LanguageSwitcher`).
pub fn language(name: &'static str) -> Msg {
    Msg::new(Ns::Common, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_serializes_as_namespace_key_and_named_arguments() {
        let m = app("Welcome back, {{name}}").text("name", "Ada").count(2);
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json, serde_json::json!({"ns": "app", "key": "Welcome back, {{name}}", "args": {"count": 2, "name": "Ada"}}));
        assert_eq!(serde_json::from_value::<Msg>(json).unwrap(), m);
    }

    #[test]
    fn a_message_without_arguments_has_empty_args() {
        let json = serde_json::to_value(common("Cancel")).unwrap();
        assert_eq!(json, serde_json::json!({"ns": "common", "key": "Cancel", "args": {}}));
        assert_eq!(serde_json::from_value::<Msg>(serde_json::json!({"ns": "common", "key": "Cancel"})).unwrap(), common("Cancel"));
    }

    #[test]
    fn a_nested_message_stays_a_key_for_the_shell_to_translate_first() {
        let m = app("Translated to {{language}}").nested("language", language("German"));
        let back: Msg = serde_json::from_value(serde_json::to_value(&m).unwrap()).unwrap();
        assert_eq!(back.args.get("language"), Some(&Arg::Msg(Box::new(language("German")))));
    }
}
