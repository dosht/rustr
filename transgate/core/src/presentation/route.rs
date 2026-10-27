//! App routes (LNG-002, IDN-022, MOBL-116). Parsing and formatting are core;
//! the history API is the shell's. The host uses [`is_app_path`] to decide
//! which paths serve the app shell: everything else is either `/api/*`
//! (JSON 404 when unmatched) or a public P6 page, never a blanket SPA 200.
//!
//! Locales follow Next.js: the default `en` has no prefix, every other UI
//! locale prefixes the path (`/de/home`). The language switcher does a full
//! page load of the same route under the new prefix and writes no cookie.

use serde::{Deserialize, Serialize};

use crate::ids::JobId;
use crate::languages::{UI_LOCALES, default_ui_locale};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", content = "job", rename_all = "snake_case")]
pub enum HomeView {
    /// `/home`: the new-transcription screen on desktop; the phone list.
    #[default]
    Default,
    /// `/home?view=new`
    New,
    /// `/home?view=all-transcripts` (alias `/home/all-transcripts`).
    AllTranscripts,
    /// `/home/{jobId}`
    Job(JobId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "page", content = "view", rename_all = "snake_case")]
pub enum Route {
    Home(HomeView),
    /// `/signin` (alias `/new-signin`).
    SignIn,
    /// The single update-password screen the recovery link returns to (IDN-017, IDN-D05).
    NewPassword,
    /// The completed-payments history page (BIL-075, `BILLING_HISTORY`).
    Billing,
    /// The provider return page. A browser POST is answered by the host from
    /// the one trusted confirmation; a GET shows the source's failure page.
    CheckoutConfirm,
    /// `/dashboard/api-keys`: the owner's API keys and callback signing secret (WEB-045/046).
    ApiKeys,
}

impl Route {
    /// IDN-022: pages that need a signed-in user. Sign-in, the recovery
    /// screen and the provider return page do not.
    pub fn protected(self) -> bool {
        matches!(self, Route::Home(_) | Route::Billing | Route::ApiKeys)
    }

    pub fn job(self) -> Option<JobId> {
        match self {
            Route::Home(HomeView::Job(id)) => Some(id),
            _ => None,
        }
    }
}

/// A parsed browser location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub locale: &'static str,
    pub route: Route,
    /// GoTrue PKCE callback `?code=`: exchanged once, then removed from history.
    pub auth_code: Option<String>,
    /// GoTrue callback failure `?error_description=` (or `?error=`).
    pub auth_error: Option<String>,
    /// Whether the location should be replaced by its canonical form
    /// (an alias path, or a consumed callback code).
    pub canonical: bool,
}

fn locale_code(segment: &str) -> Option<&'static str> {
    UI_LOCALES.iter().find(|l| !l.default && l.code == segment).map(|l| l.code)
}

/// Splits a leading locale off the path. The default locale has no prefix;
/// an explicit `/en/...` is accepted as an alias of the unprefixed path.
fn split_locale(path: &str) -> (&'static str, &str, bool) {
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    let (first, rest) = trimmed.split_once('/').unwrap_or((trimmed, ""));
    let default = default_ui_locale().code;
    match locale_code(first) {
        Some(code) => (code, rest, false),
        None if first == default => (default, rest, true),
        None => (default, trimmed, false),
    }
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// `application/x-www-form-urlencoded` value decoding; a malformed escape
/// stays literal.
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(high), Some(low)) => {
                    out.push(high << 4 | low);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_value(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == name).then(|| percent_decode(v))
    })
}

fn app_route(rest: &str, query: &str) -> Option<(Route, bool)> {
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let route = match segments.as_slice() {
        ["home"] => match query_value(query, "view").as_deref() {
            Some("new") => (Route::Home(HomeView::New), false),
            Some("all-transcripts") => (Route::Home(HomeView::AllTranscripts), false),
            _ => (Route::Home(HomeView::Default), false),
        },
        ["home", "all-transcripts"] => (Route::Home(HomeView::AllTranscripts), true),
        ["home", id] => (Route::Home(HomeView::Job(JobId(id.parse().ok()?))), false),
        ["signin"] => (Route::SignIn, false),
        ["new-signin"] => (Route::SignIn, true),
        ["new-password"] => (Route::NewPassword, false),
        ["billing"] => (Route::Billing, false),
        ["checkout", "confirm"] => (Route::CheckoutConfirm, false),
        ["dashboard", "api-keys"] => (Route::ApiKeys, false),
        _ => return None,
    };
    Some(route)
}

/// `None` for anything that is not an app page: the public site (P6), the
/// API, assets and unknown paths.
pub fn parse(url: &str) -> Option<Location> {
    let without_fragment = url.split_once('#').map_or(url, |(u, _)| u);
    let (path, query) = without_fragment.split_once('?').unwrap_or((without_fragment, ""));
    let (locale, rest, explicit_default) = split_locale(path);
    let (route, alias) = app_route(rest, query)?;
    let auth_code = query_value(query, "code").filter(|c| !c.is_empty());
    let auth_error = query_value(query, "error_description").or_else(|| query_value(query, "error")).filter(|e| !e.is_empty());
    let canonical = alias || explicit_default || auth_code.is_some() || auth_error.is_some();
    Some(Location { locale, route, auth_code, auth_error, canonical })
}

/// Whether the host serves the app shell for this path (query ignored).
pub fn is_app_path(path: &str) -> bool {
    let (_, rest, _) = split_locale(path.split_once('?').map_or(path, |(p, _)| p));
    app_route(rest, "").is_some()
}

/// The canonical path of a route under a UI locale.
pub fn href(route: Route, locale: &str) -> String {
    let prefix = match locale_code(locale) {
        Some(code) => format!("/{code}"),
        None => String::new(),
    };
    let path = match route {
        Route::Home(HomeView::Default) => "/home".to_string(),
        Route::Home(HomeView::New) => "/home?view=new".to_string(),
        Route::Home(HomeView::AllTranscripts) => "/home?view=all-transcripts".to_string(),
        Route::Home(HomeView::Job(id)) => format!("/home/{}", id.0),
        Route::SignIn => "/signin".to_string(),
        Route::NewPassword => "/new-password".to_string(),
        Route::Billing => "/billing".to_string(),
        Route::CheckoutConfirm => "/checkout/confirm".to_string(),
        Route::ApiKeys => "/dashboard/api-keys".to_string(),
    };
    format!("{prefix}{path}")
}

/// The public root the source sends a signed-out visitor and a finished
/// password change to (`/`, locale-prefixed). It is a P6 page, not an app route.
pub fn public_root(locale: &str) -> String {
    match locale_code(locale) {
        Some(code) => format!("/{code}"),
        None => "/".to_string(),
    }
}

/// The absolute web URL of an app route under `site_url` (the host's origin),
/// for shells that hand over to the web app (P8: billing, API keys).
pub fn web_url(site_url: &str, route: Route, locale: &str) -> String {
    format!("{}{}", site_url.trim_end_matches('/'), href(route, locale))
}

/// LNG-003 (Next.js detection on `/`): a valid `NEXT_LOCALE` cookie, then the
/// best `Accept-Language` match, then the default locale. Preferences are taken
/// in `q` order (ties keep header order, `q=0` is refused); each matches an
/// offered locale exactly, else by its primary subtag. The host applies it to
/// the public root; the app never writes the cookie (LNG-002).
pub fn first_visit_locale(cookie: Option<&str>, accept_language: Option<&str>) -> &'static str {
    let offered = |tag: &str| UI_LOCALES.iter().find(|l| l.code.eq_ignore_ascii_case(tag)).map(|l| l.code);
    if let Some(code) = cookie.and_then(|c| offered(c.trim())) {
        return code;
    }
    let mut preferences: Vec<(&str, f64)> = accept_language
        .unwrap_or("")
        .split(',')
        .filter_map(|part| {
            let mut fields = part.split(';');
            let tag = fields.next()?.trim();
            let q = fields
                .find_map(|f| f.trim().strip_prefix("q=").map(|v| v.trim().parse::<f64>().unwrap_or(0.0)))
                .unwrap_or(1.0);
            (!tag.is_empty() && q > 0.0).then_some((tag, q))
        })
        .collect();
    preferences.sort_by(|a, b| b.1.total_cmp(&a.1));
    preferences
        .iter()
        .find_map(|(tag, _)| offered(tag).or_else(|| offered(tag.split('-').next().unwrap_or(tag))))
        .unwrap_or(default_ui_locale().code)
}

#[cfg(test)]
mod tests;
