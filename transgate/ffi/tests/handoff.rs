//! What the native shells read from core without a constant of their own: the web
//! URL of a route. Billing and API keys are browser handoffs (D6); core owns the path
//! and the locale prefix, the shell supplies only the web origin.
use transgate_core::presentation::route::Route;
use transgate_ffi::web_url;

#[test]
fn a_handoff_url_is_cores_path_under_the_web_origin_with_the_locale_prefix() {
    assert_eq!(web_url("https://app.example".into(), Route::ApiKeys, "de".into()), "https://app.example/de/dashboard/api-keys");
    assert_eq!(web_url("https://app.example/".into(), Route::Billing, "en".into()), "https://app.example/billing");
}
