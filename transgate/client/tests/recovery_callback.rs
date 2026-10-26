//! A password-recovery callback ends on the update-password route through the real
//! client exchange and the real `AppState`. The session exists (the person is
//! authenticated to change the password), so the view model is `SignedIn` on
//! `Route::NewPassword`: the shell must render the new-password form from
//! `SignedInVm.new_password`, not the signed-in home (REPAIR-P5-6).
use transgate_core::presentation::app::{AppConfig, AppEffect, AppEvent, AppIntent, AppState, AppVm, AuthEffect, AuthEvent, SessionFacts};
use transgate_core::presentation::fixtures::NOW;
use transgate_core::presentation::layout::Viewport;
use transgate_core::presentation::route::Route;
use transgate_core::Micros;
use transgate_client::location::app_location;
use uuid::Uuid;

#[test]
fn a_recovery_callback_lands_on_the_new_password_route() {
    let mut s = AppState::new(AppConfig { site_url: "http://127.0.0.1:37363".into(), captcha: false });
    let effects = s.update(AppEvent::User(AppIntent::Started {
        url: app_location("http://127.0.0.1:37363/new-password?code=rec".into()), viewport: Viewport::default(), utc_offset_minutes: 0,
    }), NOW);
    assert!(effects.iter().any(|e| matches!(e, AppEffect::Auth { effect: AuthEffect::ExchangeCallback { .. }, .. })));
    let facts = SessionFacts { user_id: Uuid::from_u128(1), email: "ada@example.test".into(), expires_at: Micros(NOW.0 + 3_600_000_000) };
    s.update(AppEvent::Auth { epoch: s.epoch(), event: AuthEvent::PasswordRecovery { facts } }, NOW);
    match s.view(NOW) {
        AppVm::SignedIn(vm) => assert_eq!(vm.route, Route::NewPassword),
        AppVm::SignedOut(vm) => assert_eq!(vm.route, Route::NewPassword),
        other => panic!("{other:?}"),
    }
}
