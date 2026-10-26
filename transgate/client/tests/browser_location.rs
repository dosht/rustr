use transgate_client::location::app_location;
use transgate_core::presentation::app::{
    AppConfig, AppEffect, AppEvent, AppIntent, AppState, AppVm, AuthEffect, AuthEvent, SessionFacts,
};
use transgate_core::presentation::fixtures::{NOW, session_profile};
use transgate_core::presentation::layout::Viewport;
use transgate_core::presentation::route::{HomeView, Route};
use transgate_core::{JobId, Micros};

fn start(href: &str, viewport: Viewport) -> (AppState, Vec<AppEffect>) {
    let mut state = AppState::new(AppConfig { site_url: "http://127.0.0.1:37363".into(), captcha: false });
    let effects = state.update(AppEvent::User(AppIntent::Started {
        url: app_location(href.into()), viewport, utc_offset_minutes: 0,
    }), NOW);
    (state, effects)
}

#[test]
fn absolute_callback_url_reaches_core_exchange() {
    let (mut state, effects) = start("http://127.0.0.1:37363/home?code=flow%2B1", Viewport::default());
    assert_eq!(effects, vec![
        AppEffect::Navigate { path: "/home".into(), replace: true },
        AppEffect::Auth { epoch: state.epoch(), effect: AuthEffect::ExchangeCallback { code: "flow+1".into() } },
    ]);
    assert!(state.update(AppEvent::User(AppIntent::Started {
        url: app_location("http://127.0.0.1:37363/home?code=flow%2B1".into()),
        viewport: Viewport::default(), utc_offset_minutes: 0,
    }), NOW).is_empty(), "startup cannot exchange the same callback twice");
}

#[test]
fn absolute_phone_deep_link_opens_requested_job_after_session_restore() {
    let (mut state, _) = start("http://127.0.0.1:37363/home/1", Viewport { touch: true, width: 412 });
    let profile = session_profile();
    state.update(AppEvent::Auth { epoch: state.epoch(), event: AuthEvent::Restored { facts: Some(SessionFacts {
        user_id: profile.account.0, email: profile.email.clone(), expires_at: Micros(NOW.0 + 3_600_000_000),
    }) } }, NOW);
    let effects = state.update(AppEvent::SessionLoaded { epoch: state.epoch(), result: Ok(profile) }, NOW);
    let AppVm::SignedIn(vm) = state.view(NOW) else { panic!("restored session must be signed in") };
    assert_eq!(vm.route, Route::Home(HomeView::Job(JobId(1))));
    assert!(effects.contains(&AppEffect::LoadJob { epoch: state.epoch(), job: JobId(1) }));

    let effects = state.update(AppEvent::User(AppIntent::RouteChanged {
        url: app_location("http://127.0.0.1:37363/home/2".into()),
    }), NOW);
    assert!(effects.contains(&AppEffect::LoadJob { epoch: state.epoch(), job: JobId(2) }),
        "popstate must pass the same path-only boundary as startup");
}

#[test]
fn absolute_locale_callback_preserves_locale_and_query_route() {
    let (state, effects) = start(
        "https://app.example.test/tr/home?view=all-transcripts&code=locale%2Bflow#ignored",
        Viewport::default(),
    );
    assert_eq!(state.view(NOW), AppVm::Restoring { locale: "tr".into() });
    assert_eq!(effects, vec![
        AppEffect::Navigate { path: "/tr/home?view=all-transcripts".into(), replace: true },
        AppEffect::Auth { epoch: state.epoch(), effect: AuthEffect::ExchangeCallback { code: "locale+flow".into() } },
    ]);
}
