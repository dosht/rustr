//! Process configuration: `TRANSGATE_HOST`, `TRANSGATE_AUTH_KEY`, `TRANSGATE_LOCALE`
//! (or `--locale`) and `--out`. Parsing is pure so a test feeds it an argument list
//! and an environment lookup.
use std::path::PathBuf;

/// Syntax only, no prose: every user-facing sentence of the shell comes from the
/// message catalogs, and the catalogs carry no usage text.
pub const USAGE: &str = "TRANSGATE_HOST=<origin> [TRANSGATE_AUTH_KEY=<key>] [TRANSGATE_LOCALE=<code>] transgate-tui [--locale <code>] [--out <dir>]";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub host: String,
    pub auth_key: Option<String>,
    pub locale: String,
    pub out: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// `TRANSGATE_HOST` is unset or empty.
    MissingHost,
    /// An argument that is not `--locale` or `--out`.
    UnknownArgument(String),
    /// `--locale` or `--out` without its value.
    MissingValue(&'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::MissingHost => write!(f, "TRANSGATE_HOST"),
            ConfigError::UnknownArgument(arg) => write!(f, "{arg}"),
            ConfigError::MissingValue(flag) => write!(f, "{flag}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// A locale code the catalogs carry, else `en`.
pub fn locale_or_default(code: Option<&str>) -> String {
    match code {
        Some(code) if transgate_i18n::LOCALES.contains(&code) => code.to_string(),
        _ => "en".to_string(),
    }
}

pub fn parse(args: impl IntoIterator<Item = String>, env: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
    let mut locale = env("TRANSGATE_LOCALE").filter(|v| !v.is_empty());
    let mut out: Option<PathBuf> = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag.to_string(), Some(value.to_string())),
            _ => (arg.clone(), None),
        };
        let name: &'static str = match flag.as_str() {
            "--locale" => "--locale",
            "--out" => "--out",
            _ => return Err(ConfigError::UnknownArgument(arg)),
        };
        let value = inline.or_else(|| args.next()).ok_or(ConfigError::MissingValue(name))?;
        match name {
            "--locale" => locale = Some(value),
            _ => out = Some(PathBuf::from(value)),
        }
    }
    let host = env("TRANSGATE_HOST").filter(|v| !v.is_empty()).ok_or(ConfigError::MissingHost)?;
    Ok(Config {
        host,
        auth_key: env("TRANSGATE_AUTH_KEY").filter(|v| !v.is_empty()),
        locale: locale_or_default(locale.as_deref()),
        out: out.unwrap_or_else(|| PathBuf::from(".")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_host_is_required() {
        assert_eq!(parse(args(&[]), env(&[])), Err(ConfigError::MissingHost));
        assert_eq!(parse(args(&[]), env(&[("TRANSGATE_HOST", "")])), Err(ConfigError::MissingHost));
    }

    #[test]
    fn defaults_are_english_and_the_working_directory() {
        let config = parse(args(&[]), env(&[("TRANSGATE_HOST", "http://h")])).unwrap();
        assert_eq!(config, Config { host: "http://h".into(), auth_key: None, locale: "en".into(), out: PathBuf::from(".") });
    }

    #[test]
    fn the_flag_beats_the_environment_and_unknown_codes_fall_back_to_english() {
        let vars = [("TRANSGATE_HOST", "http://h"), ("TRANSGATE_LOCALE", "de"), ("TRANSGATE_AUTH_KEY", "k")];
        assert_eq!(parse(args(&[]), env(&vars)).unwrap().locale, "de");
        assert_eq!(parse(args(&["--locale", "tr"]), env(&vars)).unwrap().locale, "tr");
        assert_eq!(parse(args(&["--locale=xx"]), env(&vars)).unwrap().locale, "en");
        assert_eq!(parse(args(&[]), env(&vars)).unwrap().auth_key.as_deref(), Some("k"));
    }

    #[test]
    fn out_takes_a_directory_and_bad_arguments_are_usage_errors() {
        let vars = [("TRANSGATE_HOST", "http://h")];
        assert_eq!(parse(args(&["--out", "/tmp/x"]), env(&vars)).unwrap().out, PathBuf::from("/tmp/x"));
        assert_eq!(parse(args(&["--out"]), env(&vars)), Err(ConfigError::MissingValue("--out")));
        assert_eq!(parse(args(&["--wat"]), env(&vars)), Err(ConfigError::UnknownArgument("--wat".into())));
        assert_eq!(parse(args(&["x"]), env(&vars)), Err(ConfigError::UnknownArgument("x".into())));
    }
}
