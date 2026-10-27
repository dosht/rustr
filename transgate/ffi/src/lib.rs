//! The mobile facade of the Transgate core (P8 D2, D3). One presenter object over
//! the shared `AppDriver`, the core view models and intents as UniFFI remote types
//! (core stays UniFFI-free), and one `translate()`. Treat it as a public, versioned
//! API: Kotlin and Swift are generated from it by `scripts/gen-bindings.sh`.

mod custom;
mod ports;
mod presenter;
mod remote;

pub use custom::*;
pub use ports::*;
pub use presenter::*;

uniffi::setup_scaffolding!();

/// Translates a view-model message into `locale` with the web catalogs, strictly:
/// a missing key, argument or locale is an error, never a fallback (D3).
#[uniffi::export]
pub fn translate(locale: String, message: transgate_core::presentation::msg::Msg) -> Result<String, FfiError> {
    transgate_i18n::translate(&locale, &message).map_err(|e| FfiError::Translate { reason: e.to_string() })
}

/// The UI locales, in catalog order.
#[uniffi::export]
pub fn ui_locales() -> Vec<String> {
    transgate_i18n::LOCALES.iter().map(|l| l.to_string()).collect()
}

/// The absolute web URL of an app route, for handing over to the web app (billing,
/// API keys: D6). Core owns the path and the locale prefix; the shell supplies only
/// the web origin and opens the result in a browser.
#[uniffi::export]
pub fn web_url(site_url: String, route: transgate_core::presentation::route::Route, locale: String) -> String {
    transgate_core::presentation::route::web_url(&site_url, route, &locale)
}
