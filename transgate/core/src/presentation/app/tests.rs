use super::*;
use crate::ai::ConsentView;
use crate::presentation::fixtures;
use billing_core::presentation::confirm::ConfirmKind;

const NOW: Micros = fixtures::NOW;

fn config() -> AppConfig {
    AppConfig { site_url: "https://app.example.test/".into(), captcha: false }
}

fn facts(user: u128) -> SessionFacts {
    SessionFacts { user_id: Uuid::from_u128(user), email: "ada@example.test".into(), expires_at: Micros(NOW.0 + 3_600_000_000) }
}

fn desktop() -> Viewport {
    Viewport { touch: false, width: 1440 }
}

fn start(url: &str, viewport: Viewport) -> (AppState, Vec<AppEffect>) {
    let mut s = AppState::new(config());
    let effects = s.update(AppEvent::User(AppIntent::Started { url: url.into(), viewport, utc_offset_minutes: 180 }), NOW);
    (s, effects)
}

/// Signed in with the session profile loaded on `url`.
fn signed_in(url: &str, viewport: Viewport) -> (AppState, Vec<AppEffect>) {
    let (mut s, _) = start(url, viewport);
    let epoch = s.epoch();
    s.update(AppEvent::Auth { epoch, event: AuthEvent::Restored { facts: Some(facts(1)) } }, NOW);
    let epoch = s.epoch();
    let effects = s.update(AppEvent::SessionLoaded { epoch, result: Ok(fixtures::session_profile()) }, NOW);
    (s, effects)
}

fn navigations(effects: &[AppEffect]) -> Vec<(String, bool)> {
    effects
        .iter()
        .filter_map(|e| match e {
            AppEffect::Navigate { path, replace } => Some((path.clone(), *replace)),
            _ => None,
        })
        .collect()
}

fn consent_view(granted: bool) -> ConsentView {
    ConsentView {
        has_consented: granted,
        consent_version: "1".into(),
        consent_date: granted.then_some(NOW),
        revoked_date: None,
        privacy_policy_url: "/legal/privacy-policy-en".into(),
        provider_name: "DeepSeek".into(),
        can_revoke: granted,
        cleanup_pending: false,
    }
}

#[test]
fn a_tab_restores_its_session_or_exchanges_a_callback_code_once() {
    let (s, effects) = start("/de/home", desktop());
    assert_eq!(effects, vec![AppEffect::Auth { epoch: s.epoch(), effect: AuthEffect::RestoreSession }]);
    assert_eq!(s.view(NOW), AppVm::Restoring { locale: "de".into() });

    let (s, effects) = start("/home?code=abc", desktop());
    assert_eq!(
        effects,
        vec![
            AppEffect::Navigate { path: "/home".into(), replace: true },
            AppEffect::Auth { epoch: s.epoch(), effect: AuthEffect::ExchangeCallback { code: "abc".into() } },
        ],
        "the code leaves the address bar and is exchanged once"
    );
}

// IDN-022: a protected page without a user is replaced by /signin.
#[test]
fn no_session_on_a_protected_page_goes_to_sign_in() {
    let (mut s, _) = start("/home/42", desktop());
    let epoch = s.epoch();
    let effects = s.update(AppEvent::Auth { epoch, event: AuthEvent::Restored { facts: None } }, NOW);
    assert_eq!(navigations(&effects), vec![("/signin".to_string(), true)]);
    assert!(matches!(s.view(NOW), AppVm::SignedOut(vm) if vm.route == Route::SignIn));
}

#[test]
fn a_profile_starts_every_screen_and_the_route_job() {
    let (s, effects) = signed_in("/home/42", desktop());
    let epoch = s.epoch();
    for expected in [AppEffect::LoadBalance { epoch }, AppEffect::LoadQueue { epoch }, AppEffect::LoadSettings { epoch }, AppEffect::LoadJob { epoch, job: JobId(42) }] {
        assert!(effects.contains(&expected), "{expected:?} in {effects:?}");
    }
    assert!(effects.contains(&AppEffect::Jobs { epoch, effect: job_list::Effect::LoadPage { before: None } }));
    assert!(effects.contains(&AppEffect::Consent { epoch, effect: consent::Effect::LoadConsent }));
    assert!(effects.contains(&AppEffect::Subscription { epoch, effect: SubscriptionEffect::Load }));
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    assert_eq!(vm.route, Route::Home(HomeView::Job(JobId(42))));
    assert_eq!(vm.joined_on_text.as_deref(), Some("3-10-2025"), "90 days before the fixture instant, at UTC+3");
}

// The session fence: a late answer for an earlier session is dropped, even for the same owner.
#[test]
fn answers_of_an_earlier_session_are_dropped_after_sign_out_and_sign_in() {
    let (mut s, _) = signed_in("/home", desktop());
    let old = s.epoch();
    s.update(AppEvent::User(AppIntent::SignOutRequested), NOW);
    s.update(AppEvent::Auth { epoch: old, event: AuthEvent::SignedOut }, NOW);
    let epoch = s.epoch();
    s.update(AppEvent::Auth { epoch, event: AuthEvent::SignedIn { facts: facts(1) } }, NOW);
    assert!(!s.is_current(old), "the same owner signed in again: a new session");
    let late = AppEvent::Jobs { epoch: old, event: job_list::Event::PageLoaded { jobs: vec![fixtures::job(9, JobStatus::Completed, "late.mp3")], next_cursor: None, replace: true } };
    assert!(s.update(late, NOW).is_empty());
    let epoch = s.epoch();
    s.update(AppEvent::SessionLoaded { epoch, result: Ok(fixtures::session_profile()) }, NOW);
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    assert!(vm.jobs.rows.is_empty());
}

// IDN-024: sign-out records loggedOut, resets telemetry, signs out once, then leaves for `/`.
#[test]
fn sign_out_reports_once_and_leaves_for_the_public_root() {
    let (mut s, _) = signed_in("/de/home", desktop());
    let epoch = s.epoch();
    let effects = s.update(AppEvent::User(AppIntent::SignOutRequested), NOW);
    assert_eq!(
        effects,
        vec![
            AppEffect::Telemetry { event: TelemetryEvent::new("loggedOut") },
            AppEffect::TelemetryReset,
            AppEffect::Auth { epoch, effect: AuthEffect::SignOut },
        ]
    );
    assert!(s.update(AppEvent::User(AppIntent::SignOutRequested), NOW).is_empty(), "a second click is ignored");
    let effects = s.update(AppEvent::Auth { epoch, event: AuthEvent::SignedOut }, NOW);
    assert_eq!(effects, vec![AppEffect::Leave { url: "/de".into() }]);
}

// A sign-out the identity service could not complete leaves the tab signed in and retryable.
#[test]
fn a_failed_sign_out_can_be_retried() {
    let (mut s, _) = signed_in("/home", desktop());
    let epoch = s.epoch();
    s.update(AppEvent::User(AppIntent::SignOutRequested), NOW);
    assert!(s.update(AppEvent::Auth { epoch, event: AuthEvent::Failed { kind: AuthFailure::Network } }, NOW).is_empty());
    assert!(s.is_current(epoch) && matches!(s.view(NOW), AppVm::SignedIn(vm) if !vm.signing_out));
    assert!(matches!(s.update(AppEvent::User(AppIntent::SignOutRequested), NOW).last(), Some(AppEffect::Auth { effect: AuthEffect::SignOut, .. })));
}

// The consent fence: a held AI answer or export is dropped once the decision changes.
#[test]
fn a_consent_change_fences_ai_work_but_not_session_work() {
    let (mut s, _) = signed_in("/home/42", desktop());
    let epoch = s.epoch();
    s.update(AppEvent::Consent { epoch, event: consent::Event::Loaded { result: Ok(consent_view(true)) } }, NOW);
    let granted = s.ai_epoch();
    s.update(AppEvent::User(AppIntent::Consent(ConsentIntent::DisableConfirmed)), NOW);
    s.update(AppEvent::Consent { epoch, event: consent::Event::DecisionSaved { result: Ok(consent_view(false)) } }, NOW);
    assert!(!s.is_current(granted), "AI work admitted under the old decision is stale");
    assert!(s.is_current(epoch), "session work continues");
    assert!(s.update(AppEvent::Analysis { epoch: granted, job: JobId(42), event: analysis::Event::Hidden }, NOW).is_empty());
}

// REPAIR-P5-5, P5 D6: a second tab opening a job whose chat answer is running elsewhere shows the
// held partial and attaches exactly one watch; nothing starts, retries or meters.
#[test]
fn a_second_tab_reattaches_a_running_chat_answer_without_starting_one() {
    use crate::ai::{ChatHistory, ChatRole, Draft, GenerationId, GenerationKind, GenerationStatus, GenerationView, InFlightChat};
    use crate::presentation::ai::WatchKind;
    let (mut s, _) = signed_in("/home/42", desktop());
    let epoch = s.epoch();
    let effects = s.update(AppEvent::Consent { epoch, event: consent::Event::Loaded { result: Ok(consent_view(true)) } }, NOW);
    let ai = s.ai_epoch();
    assert!(effects.contains(&AppEffect::Chat { epoch: ai, job: JobId(42), effect: chat::Effect::LoadHistory { job: JobId(42) } }));
    let g = GenerationId(Uuid::from_u128(77));
    let generation = GenerationView {
        id: g, job: JobId(42), kind: GenerationKind::Chat, status: GenerationStatus::Running, sequence: 3,
        source_revision: 1, consent_epoch: 1, draft: Draft::Text { text: "The team reviewed".into() },
        final_content: None, durable: true, remote_outcome_unknown: false, execution: Some(Uuid::from_u128(5)),
        tokens_used: None, interruption: None, created_at: NOW, finished_at: None,
    };
    let history = ChatHistory { messages: vec![], in_flight: Some(InFlightChat { question: "What did the team agree to review?".into(), generation }) };
    let effects = s.update(AppEvent::Chat { epoch: ai, job: JobId(42), event: chat::Event::HistoryLoaded { job: JobId(42), result: Ok(history) } }, NOW);
    assert_eq!(effects, vec![AppEffect::Chat { epoch: ai, job: JobId(42), effect: chat::Effect::Watch { generation: g, kind: WatchKind::Text, after: None } }]);
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    let chat = vm.chat.expect("chat on a job route");
    let last = chat.lines.last().unwrap();
    assert_eq!((last.role, last.content.as_str(), last.streaming), (ChatRole::Assistant, "The team reviewed", true));
    assert!(!chat.can_send);
}

// REPAIR-P5-5b: the tab's own question is never replaced by its history load. As in the source
// (`DesktopChatView.tsx:142,307`), nothing is sent while history is loading; afterwards the own
// question streams in place with one watch, never treated as a reattach.
#[test]
fn the_own_tab_question_keeps_its_lines_through_history_and_stream() {
    use crate::ai::{ChatHistory, ChatRole, GenerationId};
    use crate::presentation::ai::WatchKind;
    use llm_core::{ApplicationFrame, Persistence, StreamIdentity, StreamStatus, TextEvent};
    let (mut s, _) = signed_in("/home/42", desktop());
    let epoch = s.epoch();
    s.update(AppEvent::Consent { epoch, event: consent::Event::Loaded { result: Ok(consent_view(true)) } }, NOW);
    let ai = s.ai_epoch();
    let job = JobId(42);
    let chat_vm = |s: &AppState| match s.view(NOW) {
        AppVm::SignedIn(vm) => vm.chat.expect("chat on a job route"),
        _ => panic!("signed in"),
    };
    let send = AppIntent::Chat(ChatIntent::SendRequested { text: "What did the team agree to review?".into() });
    assert!(!chat_vm(&s).can_send, "history is still loading");
    let early = s.update(AppEvent::User(send.clone()), NOW);
    assert!(!early.iter().any(|e| matches!(e, AppEffect::Chat { effect: chat::Effect::StartChat { .. }, .. })), "{early:?}");

    s.update(AppEvent::Chat { epoch: ai, job, event: chat::Event::HistoryLoaded { job, result: Ok(ChatHistory { messages: vec![], in_flight: None }) } }, NOW);
    let started = s.update(AppEvent::User(send), NOW);
    let key = started.iter().find_map(|e| match e {
        AppEffect::Chat { effect: chat::Effect::StartChat { key, .. }, .. } => Some(*key),
        _ => None,
    }).expect("the question is sent once history arrived");
    let g = GenerationId(Uuid::from_u128(88));
    let watched = s.update(AppEvent::Chat { epoch: ai, job, event: chat::Event::StartAccepted { key, generation: g } }, NOW);
    assert_eq!(watched, vec![AppEffect::Chat { epoch: ai, job, effect: chat::Effect::Watch { generation: g, kind: WatchKind::Text, after: None } }]);
    let identity = StreamIdentity { generation: g.to_string(), content_revision: 1, consent_epoch: 1, execution: "e".into() };
    let frame = |seq, event| AppEvent::Chat { epoch: ai, job, event: chat::Event::WatchFrame { generation: g, frame: ApplicationFrame { identity: identity.clone(), seq, event } } };
    s.update(frame(1, TextEvent::Snapshot { text: "The team".into(), status: StreamStatus::Streaming, persistence: Persistence::Provisional }), NOW);
    let vm = chat_vm(&s);
    let shown: Vec<_> = vm.lines.iter().map(|l| (l.role, l.content.as_str(), l.streaming)).collect();
    assert_eq!(shown, vec![(ChatRole::User, "What did the team agree to review?", false), (ChatRole::Assistant, "The team", true)]);
    s.update(frame(2, TextEvent::Final { text: "The team reviewed accessibility.".into(), persistence: Persistence::Saved }), NOW);
    let vm = chat_vm(&s);
    let shown: Vec<_> = vm.lines.iter().map(|l| (l.role, l.content.as_str(), l.streaming)).collect();
    assert_eq!(shown, vec![(ChatRole::User, "What did the team agree to review?", false), (ChatRole::Assistant, "The team reviewed accessibility.", false)]);
    assert!(vm.can_send);
}

// REPAIR-P5-5b invariant: a history answer landing while this tab's own question is pending (a
// late duplicate, or an answer reporting this same generation in flight) keeps the question and
// the streaming answer, adds no watch, and the answer still completes in place.
#[test]
fn history_landing_while_a_question_is_pending_keeps_both_lines() {
    use crate::ai::{ChatHistory, ChatRole, Draft, GenerationId, GenerationKind, GenerationStatus, GenerationView, InFlightChat};
    use llm_core::{ApplicationFrame, Persistence, StreamIdentity, StreamStatus, TextEvent};
    let (mut s, _) = signed_in("/home/42", desktop());
    let epoch = s.epoch();
    s.update(AppEvent::Consent { epoch, event: consent::Event::Loaded { result: Ok(consent_view(true)) } }, NOW);
    let ai = s.ai_epoch();
    let job = JobId(42);
    let history = |in_flight| AppEvent::Chat { epoch: ai, job, event: chat::Event::HistoryLoaded { job, result: Ok(ChatHistory { messages: vec![], in_flight }) } };
    s.update(history(None), NOW);
    let started = s.update(AppEvent::User(AppIntent::Chat(ChatIntent::SendRequested { text: "Q".into() })), NOW);
    let Some(AppEffect::Chat { effect: chat::Effect::StartChat { key, .. }, .. }) = started.first() else { panic!("{started:?}") };
    let g = GenerationId(Uuid::from_u128(99));
    s.update(AppEvent::Chat { epoch: ai, job, event: chat::Event::StartAccepted { key: *key, generation: g } }, NOW);
    let identity = StreamIdentity { generation: g.to_string(), content_revision: 1, consent_epoch: 1, execution: "e".into() };
    let frame = |seq, event| AppEvent::Chat { epoch: ai, job, event: chat::Event::WatchFrame { generation: g, frame: ApplicationFrame { identity: identity.clone(), seq, event } } };
    s.update(frame(1, TextEvent::Snapshot { text: "Partial".into(), status: StreamStatus::Streaming, persistence: Persistence::Provisional }), NOW);

    let own = GenerationView {
        id: g, job, kind: GenerationKind::Chat, status: GenerationStatus::Running, sequence: 1, source_revision: 1, consent_epoch: 1,
        draft: Draft::Text { text: "Partial".into() }, final_content: None, durable: true, remote_outcome_unknown: false,
        execution: None, tokens_used: None, interruption: None, created_at: NOW, finished_at: None,
    };
    for late in [history(None), history(Some(InFlightChat { question: "Q".into(), generation: own }))] {
        assert!(s.update(late, NOW).is_empty(), "no reattach or second watch for the tab's own question");
    }
    let lines = |s: &AppState| match s.view(NOW) {
        AppVm::SignedIn(vm) => vm.chat.unwrap().lines.into_iter().map(|l| (l.role, l.content, l.streaming)).collect::<Vec<_>>(),
        _ => panic!("signed in"),
    };
    assert_eq!(lines(&s), vec![(ChatRole::User, "Q".into(), false), (ChatRole::Assistant, "Partial".into(), true)]);
    s.update(frame(2, TextEvent::Final { text: "Done.".into(), persistence: Persistence::Saved }), NOW);
    assert_eq!(lines(&s), vec![(ChatRole::User, "Q".into(), false), (ChatRole::Assistant, "Done.".into(), false)]);
}

// MOBL-116: a phone landing on /home is replaced by the new-transcript view.
#[test]
fn a_phone_on_home_is_sent_to_the_new_transcript_view() {
    let phone = Viewport { touch: true, width: 390 };
    let (s, effects) = signed_in("/home", phone);
    assert_eq!(navigations(&effects), vec![("/home?view=new".to_string(), true)]);
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    assert_eq!((vm.route, vm.layouts.frame), (Route::Home(HomeView::New), Frame::Phone));
    let (_, effects) = signed_in("/home", desktop());
    assert!(navigations(&effects).is_empty());
}

// One refresh after a 401; a failed refresh signs the tab out.
#[test]
fn an_unauthorized_answer_refreshes_once_then_signs_out() {
    let (mut s, _) = signed_in("/home", desktop());
    let epoch = s.epoch();
    assert_eq!(s.update(AppEvent::Unauthorized { epoch }, NOW), vec![AppEffect::Auth { epoch, effect: AuthEffect::Refresh }]);
    assert!(s.update(AppEvent::Unauthorized { epoch }, NOW).is_empty());
    let effects = s.update(AppEvent::Auth { epoch, event: AuthEvent::Failed { kind: AuthFailure::InvalidOrExpiredCode } }, NOW);
    assert_eq!(navigations(&effects), vec![("/signin".to_string(), true)]);
    assert!(matches!(s.view(NOW), AppVm::SignedOut(_)));
}

#[test]
fn a_rotated_token_for_the_same_user_keeps_the_session_and_another_user_replaces_it() {
    let (mut s, _) = signed_in("/home", desktop());
    let epoch = s.epoch();
    s.update(AppEvent::Unauthorized { epoch }, NOW);
    assert!(s.update(AppEvent::Auth { epoch, event: AuthEvent::SessionAdopted { facts: facts(1) } }, NOW).is_empty());
    assert!(s.is_current(epoch));
    let effects = s.update(AppEvent::Auth { epoch, event: AuthEvent::SessionAdopted { facts: facts(2) } }, NOW);
    assert!(!s.is_current(epoch), "another account adopted from another tab");
    assert!(matches!(effects.as_slice(), [AppEffect::LoadSession { .. }]));
}

// P3 D13: an erased account is terminal for the tab.
#[test]
fn an_erased_account_is_terminal() {
    let (mut s, _) = signed_in("/home", desktop());
    let epoch = s.epoch();
    let effects = s.update(AppEvent::AccountErased { epoch }, NOW);
    assert!(matches!(effects.as_slice(), [AppEffect::Auth { effect: AuthEffect::SignOut, .. }]));
    let after = s.epoch();
    assert!(s.update(AppEvent::Auth { epoch: after, event: AuthEvent::SignedOut }, NOW).is_empty());
    assert_eq!(s.view(NOW), AppVm::Erased { locale: "en".into(), message: ACCOUNT_ERASED });
    assert!(!s.is_current(epoch) && !s.is_current(after));
}

// FRE-020 needs the freemium answer: a loaded job waits for it before it is shown.
#[test]
fn a_job_is_shown_once_both_it_and_the_freemium_answer_arrived() {
    let (mut s, _) = signed_in("/home/11", desktop());
    let epoch = s.epoch();
    let detail = JobDetail { job: fixtures::job(11, JobStatus::Completed, "podcast.mp3"), lines: vec![fixtures::line(1, 11, "00:00:00", "00:00:02", "hi", None)] };
    assert!(s.update(AppEvent::JobLoaded { epoch, job: JobId(11), result: Ok(detail) }, NOW).is_empty());
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    assert!(vm.transcript.is_none());
    let effects = s.update(AppEvent::Balance { epoch, result: Ok(BalanceView { balance: Some(75), is_freemium: false, special_client_alert: None }) }, NOW);
    assert!(effects.contains(&AppEffect::Transcript { epoch, job: JobId(11), effect: transcript::Effect::LoadMediaUrl { job: JobId(11), file_name: "podcast.mp3".into() } }));
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    assert_eq!(vm.transcript.map(|t| t.rows.len()), Some(1));
    assert_eq!(vm.balance, app("p5.balance.hours_minutes").number("hours", 1).number("minutes", 15));
    let other = AppEvent::JobLoaded { epoch, job: JobId(12), result: Ok(JobDetail { job: fixtures::job(12, JobStatus::Completed, "x.mp3"), lines: vec![] }) };
    assert!(s.update(other, NOW).is_empty(), "an answer for a job no longer open is dropped");
}

#[test]
fn support_is_sent_with_the_open_job_and_start_navigates_to_the_first_job() {
    let (mut s, _) = signed_in("/home/11", desktop());
    let epoch = s.epoch();
    s.update(AppEvent::User(AppIntent::Support(SupportIntent::Opened)), NOW);
    s.update(AppEvent::User(AppIntent::Support(SupportIntent::MessageChanged { text: "help".into() })), NOW);
    let effects = s.update(AppEvent::User(AppIntent::Support(SupportIntent::Submitted)), NOW);
    assert_eq!(effects, vec![AppEffect::Support { epoch, effect: support::Effect::Send { message: "help".into(), job: Some(JobId(11)) } }]);
    let effects = s.update(AppEvent::Upload { epoch, event: upload_queue::Event::StartSucceeded { first_job: JobId(77) } }, NOW);
    assert_eq!(navigations(&effects), vec![("/home/77".to_string(), false)]);
    assert!(effects.contains(&AppEffect::LoadJob { epoch, job: JobId(77) }));
}

#[test]
fn the_language_switcher_reloads_the_home_page_in_the_chosen_locale() {
    let (mut s, _) = signed_in("/home/11", desktop());
    assert_eq!(s.update(AppEvent::User(AppIntent::LocaleChosen { code: "de".into() }), NOW), vec![AppEffect::Leave { url: "/de/home".into() }]);
    assert_eq!(s.update(AppEvent::User(AppIntent::LocaleChosen { code: "en".into() }), NOW), vec![AppEffect::Leave { url: "/home".into() }]);
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    assert_eq!(vm.locales.len(), 16);
    assert!(vm.locales.iter().filter(|l| l.current).all(|l| l.code == "en"));
}

#[test]
fn sign_in_forms_send_redirects_to_the_locale_home_and_recovery_to_new_password() {
    let (mut s, _) = start("/de/signin", desktop());
    let epoch = s.epoch();
    s.update(AppEvent::Auth { epoch, event: AuthEvent::Restored { facts: None } }, NOW);
    let epoch = s.epoch();
    let user = |s: &mut AppState, i: AuthIntent| s.update(AppEvent::User(AppIntent::Auth(i)), NOW);
    user(&mut s, AuthIntent::EmailChanged { text: "ada@example.test".into() });
    user(&mut s, AuthIntent::ModeChosen { mode: AuthMode::ForgottenPassword });
    assert_eq!(
        user(&mut s, AuthIntent::Submitted),
        vec![AppEffect::Auth {
            epoch,
            effect: AuthEffect::Recover { email: "ada@example.test".into(), redirect: "https://app.example.test/de/new-password".into(), captcha: None },
        }]
    );
    s.update(AppEvent::Auth { epoch, event: AuthEvent::EmailSent }, NOW);
    let AppVm::SignedOut(vm) = s.view(NOW) else { panic!("signed out") };
    assert_eq!(vm.auth.notice, Some(auth("forgotten_password.confirmation_text")));
    assert_eq!(
        user(&mut s, AuthIntent::GoogleChosen),
        vec![AppEffect::Auth { epoch, effect: AuthEffect::GoogleRedirect { redirect: "https://app.example.test/de/home".into() } }]
    );
}

// D12: with Turnstile configured, a form submits only with a token.
#[test]
fn a_configured_captcha_gates_submission() {
    let mut s = AppState::new(AppConfig { captcha: true, ..config() });
    s.update(AppEvent::User(AppIntent::Started { url: "/signin".into(), viewport: desktop(), utc_offset_minutes: 0 }), NOW);
    let epoch = s.epoch();
    s.update(AppEvent::Auth { epoch, event: AuthEvent::Restored { facts: None } }, NOW);
    assert!(s.update(AppEvent::User(AppIntent::Auth(AuthIntent::Submitted)), NOW).is_empty());
    s.update(AppEvent::User(AppIntent::Auth(AuthIntent::CaptchaSolved { token: "t".into() })), NOW);
    let effects = s.update(AppEvent::User(AppIntent::Auth(AuthIntent::Submitted)), NOW);
    assert!(matches!(effects.as_slice(), [AppEffect::Auth { effect: AuthEffect::PasswordSignIn { captcha: Some(_), .. }, .. }]));
}

// IDN-017: success shows the notice and goes to the public root after two seconds.
#[test]
fn a_password_update_goes_to_the_public_root_after_its_notice() {
    let (mut s, _) = start("/new-password?code=rec", desktop());
    let epoch = s.epoch();
    let effects = s.update(AppEvent::Auth { epoch, event: AuthEvent::PasswordRecovery { facts: facts(1) } }, NOW);
    assert!(navigations(&effects).contains(&("/new-password".to_string(), true)));
    let epoch = s.epoch();
    s.update(AppEvent::User(AppIntent::NewPassword(NewPasswordIntent::PasswordChanged { text: "s3cret!".into() })), NOW);
    let effects = s.update(AppEvent::User(AppIntent::NewPassword(NewPasswordIntent::Submitted)), NOW);
    assert_eq!(effects, vec![AppEffect::Auth { epoch, effect: AuthEffect::UpdatePassword { password: "s3cret!".into() } }]);
    let effects = s.update(AppEvent::Auth { epoch, event: AuthEvent::PasswordUpdated }, NOW);
    assert_eq!(effects, vec![AppEffect::Timer { epoch, timer: Timer::PasswordUpdated, after_ms: PASSWORD_UPDATED_REDIRECT_MS }]);
    assert_eq!(s.update(AppEvent::Timer { epoch, timer: Timer::PasswordUpdated }, NOW), vec![AppEffect::Leave { url: "/".into() }]);
}

#[test]
fn balance_texts_follow_the_source_shapes() {
    assert_eq!(balance_text(None), app("p5.balance.none"));
    assert_eq!(balance_text(Some(59)), app("p5.balance.minutes").number("minutes", 59));
    assert_eq!(balance_text(Some(125)), app("p5.balance.hours_minutes").number("hours", 2).number("minutes", 5));
}

#[test]
fn confirmation_reads_the_embedded_result_once_and_never_restarts_checkout() {
    let (mut s, effects) = start("/de/checkout/confirm", desktop());
    let epoch = s.epoch();
    let [AppEffect::ReadConfirmation { sequence, .. }] = effects.as_slice() else { panic!("{effects:?}") };
    let sequence = *sequence;
    let payload = ConfirmPayload::Confirmed { view: billing_core::presentation::confirm::ConfirmedPayment {
        payment_id: 42, status: billing_core::grants::PaymentStatus::Completed, success: true,
        changed: true, analytics: vec![billing_core::presentation::confirm::ConfirmAnalytic::PaymentCompleted],
        redirect_after_seconds: 2,
    }};
    let effects = s.update(AppEvent::ConfirmationLoaded { epoch, sequence, payload: Some(payload.clone()) }, NOW);
    assert!(effects.iter().any(|e| matches!(e, AppEffect::Confirmation { effect: ConfirmEffect::Track(_), .. })));
    assert!(s.update(AppEvent::ConfirmationLoaded { epoch, sequence, payload: Some(payload) }, NOW).is_empty());
    assert!(matches!(s.view(NOW), AppVm::CheckoutConfirm { vm: Some(ConfirmVm { kind: ConfirmKind::Succeeded, .. }), .. }));
    s.update(AppEvent::Timer { epoch, timer: Timer::Confirmation { sequence } }, NOW);
    assert_eq!(s.update(AppEvent::Timer { epoch, timer: Timer::Confirmation { sequence } }, NOW), vec![AppEffect::Leave { url: "/de".into() }]);
    assert!(s.update(AppEvent::Timer { epoch, timer: Timer::Confirmation { sequence } }, NOW).is_empty());
}

#[test]
fn uncertain_confirmation_has_no_countdown_and_old_route_answers_are_discarded() {
    let (mut s, effects) = start("/checkout/confirm", desktop());
    let epoch = s.epoch();
    let [AppEffect::ReadConfirmation { sequence, .. }] = effects.as_slice() else { panic!("{effects:?}") };
    let sequence = *sequence;
    assert!(s.update(AppEvent::ConfirmationLoaded { epoch, sequence, payload: Some(ConfirmPayload::Failed {
        status: 503, code: "storage".into(), message: "private detail".into(),
    }) }, NOW).is_empty());
    assert!(matches!(s.view(NOW), AppVm::CheckoutConfirm { vm: Some(ConfirmVm { kind: ConfirmKind::Unconfirmed, countdown: None, .. }), .. }));
    s.update(AppEvent::User(AppIntent::Navigate { route: Route::SignIn }), NOW);
    assert!(s.update(AppEvent::Timer { epoch, timer: Timer::Confirmation { sequence } }, NOW).is_empty());
    s.update(AppEvent::User(AppIntent::Navigate { route: Route::CheckoutConfirm }), NOW);
    assert!(s.update(AppEvent::ConfirmationLoaded { epoch, sequence, payload: None }, NOW).is_empty());
    assert!(matches!(s.view(NOW), AppVm::CheckoutConfirm { vm: None, .. }));
}

#[test]
fn invoice_documents_and_answers_are_scoped_to_the_signed_in_owner() {
    let (mut s, effects) = signed_in("/billing", desktop());
    let epoch = s.epoch();
    assert!(effects.contains(&AppEffect::Invoices { epoch, ticket: s.invoice_preview_ticket(), effect: InvoicesEffect::LoadOwn }));
    let context = billing_core::presentation::fixtures::invoice_context_sample();
    let payment_id = context.invoices[0].payment_id;
    s.update(AppEvent::Invoices { epoch, ticket: s.invoice_preview_ticket(), event: InvoicesEvent::Loaded(Ok(context.clone())) }, NOW);
    let effects = s.update(AppEvent::User(AppIntent::Invoices(InvoicesIntent::Download { payment_id })), NOW);
    assert!(matches!(effects.as_slice(), [AppEffect::Invoices { epoch: e, effect: InvoicesEffect::Document { purpose: billing_core::presentation::invoices::DocumentPurpose::Download, .. }, .. }] if *e == epoch));
    s.update(AppEvent::Invoices { epoch, ticket: s.invoice_preview_ticket(), event: InvoicesEvent::DocumentFailed {
        payment_id, failure: billing_core::presentation::purchase::PurchaseFailure::Unavailable,
    } }, NOW);
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    assert!(vm.billing_messages.document_failure.is_some());
    assert_eq!(vm.invoices.unwrap().rows[0].payment_id, payment_id, "failed delivery keeps history usable");
    s.update(AppEvent::Auth { epoch, event: AuthEvent::SessionAdopted { facts: facts(2) } }, NOW);
    assert!(!s.is_current(epoch), "a held document cannot be delivered to another owner");
    s.update(AppEvent::Invoices { epoch, ticket: 0, event: InvoicesEvent::Loaded(Ok(context)) }, NOW);
    s.update(AppEvent::Invoices { epoch, ticket: 0, event: InvoicesEvent::DocumentFailed {
        payment_id, failure: billing_core::presentation::purchase::PurchaseFailure::Unavailable,
    } }, NOW);
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    assert!(vm.invoices.is_none());
    assert!(vm.billing_messages.document_failure.is_none(), "a previous owner's delivery failure is stale");
}

#[test]
fn special_client_host_copy_is_preserved_and_cleared_on_account_switch() {
    let (mut s, _) = signed_in("/home", desktop());
    let epoch = s.epoch();
    let alert = SpecialClientAlert { code: "LOYAL".into(), percent: 25, message: "Host says 33 percent".into() };
    s.update(AppEvent::Balance { epoch, result: Ok(BalanceView { balance: Some(75), is_freemium: false, special_client_alert: Some(alert.clone()) }) }, NOW);
    assert!(matches!(s.view(NOW), AppVm::SignedIn(vm) if vm.special_client_alert == Some(alert)));
    s.update(AppEvent::Auth { epoch, event: AuthEvent::SessionAdopted { facts: facts(2) } }, NOW);
    assert!(matches!(s.view(NOW), AppVm::SignedIn(vm) if vm.special_client_alert.is_none()));
}

#[test]
fn closing_and_reopening_the_same_invoice_rejects_old_preview_delivery_and_failure() {
    let (mut s, _) = signed_in("/billing", desktop());
    let epoch = s.epoch();
    let context = billing_core::presentation::fixtures::invoice_context_sample();
    let payment_id = context.invoices[0].payment_id;
    s.update(AppEvent::Invoices { epoch, ticket: s.invoice_preview_ticket(), event: InvoicesEvent::Loaded(Ok(context.clone())) }, NOW);
    s.update(AppEvent::User(AppIntent::Invoices(InvoicesIntent::Preview { payment_id })), NOW);
    let old = s.invoice_preview_ticket();
    assert!(s.is_current_invoice_preview(old, payment_id));
    s.update(AppEvent::User(AppIntent::Invoices(InvoicesIntent::ClosePreview)), NOW);
    s.update(AppEvent::User(AppIntent::Invoices(InvoicesIntent::Preview { payment_id })), NOW);
    let current = s.invoice_preview_ticket();
    assert_ne!(old, current);
    assert!(!s.is_current_invoice_preview(old, payment_id));
    s.update(AppEvent::Invoices { epoch, ticket: old, event: InvoicesEvent::DocumentFailed {
        payment_id, failure: billing_core::presentation::purchase::PurchaseFailure::Unavailable,
    } }, NOW);
    assert!(s.is_current_invoice_preview(current, payment_id), "a stale failure must not close a newer preview");
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    assert!(vm.billing_messages.document_failure.is_none());
    s.update(AppEvent::Invoices { epoch, ticket: current, event: InvoicesEvent::Loaded(Ok(context)) }, NOW);
    assert!(!s.is_current_invoice_preview(current, payment_id), "a changed invoice context invalidates held rendering");
    s.update(AppEvent::User(AppIntent::Invoices(InvoicesIntent::Preview { payment_id })), NOW);
    let current = s.invoice_preview_ticket();
    s.update(AppEvent::User(AppIntent::Navigate { route: Route::Home(HomeView::New) }), NOW);
    assert!(!s.is_current_invoice_preview(current, payment_id), "leaving history invalidates held rendering");
}
