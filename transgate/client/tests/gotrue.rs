//! GoTrue PKCE regressions against a loopback GoTrue that enforces the rules the
//! client depends on: S256 verifier check, single-use flow codes and single-use
//! rotated refresh tokens. Real GoTrue/Mailpit proof is the lead's runtime.
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{post, put};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use transgate_client::auth::{
    AuthErrorKind, AuthOutcome, AuthPorts, AuthStorage, Callback, Clock, GoTrue, GoTrueConfig, LockGuard, OsRandom,
    SESSION_KEY, SessionLock, VERIFIER_KEY, challenge, parse_callback, strip_callback,
};
use transgate_client::future::BoxFuture;
use uuid::Uuid;

#[derive(Default)]
struct Memory(Mutex<HashMap<String, String>>);
impl AuthStorage for Memory {
    fn get(&self, key: &str) -> Option<String> { self.0.lock().unwrap().get(key).cloned() }
    fn set(&self, key: &str, value: &str) { self.0.lock().unwrap().insert(key.into(), value.into()); }
    fn remove(&self, key: &str) { self.0.lock().unwrap().remove(key); }
}

/// Stands in for navigator.locks: one holder at a time across "tabs".
#[derive(Clone, Default)]
struct TabLock(Arc<tokio::sync::Mutex<()>>);
impl SessionLock for TabLock {
    fn acquire(&self) -> BoxFuture<'_, LockGuard> {
        let mutex = self.0.clone();
        Box::pin(async move {
            let held = mutex.lock_owned().await;
            LockGuard::new(move || drop(held))
        })
    }
}

struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn now_secs(&self) -> i64 { self.0.load(Ordering::SeqCst) }
}

#[derive(Default)]
struct Fake {
    flows: HashMap<String, (String, String)>,
    refresh: HashMap<String, (String, bool)>,
    access: HashMap<String, String>,
    users: HashMap<String, Uuid>,
    calls: Vec<(String, String, Value, Option<String>)>,
    next: u64,
    refresh_calls: usize,
    hold_refresh: bool,
    fail_held_refresh: bool,
    logout_status: Option<u16>,
}

#[derive(Clone)]
struct Server {
    state: Arc<Mutex<Fake>>,
    refresh_arrived: Arc<Notify>,
    refresh_release: Arc<Notify>,
}

fn error(status: StatusCode, code: &str, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"code": status.as_u16(), "error_code": code, "msg": msg})))
}

impl Server {
    fn issue(fake: &mut Fake, email: &str) -> Value {
        fake.next += 1;
        let n = fake.next;
        let count = fake.users.len() as u128 + 1;
        let id = *fake.users.entry(email.into()).or_insert_with(|| Uuid::from_u128(count));
        let (access, refresh) = (format!("access-{n}"), format!("refresh-{n}"));
        fake.refresh.insert(refresh.clone(), (email.into(), false));
        fake.access.insert(access.clone(), email.into());
        json!({"access_token": access, "token_type": "bearer", "expires_in": 3600, "refresh_token": refresh,
               "user": {"id": id, "email": email}})
    }
    fn flow(fake: &mut Fake, body: &Value) {
        fake.next += 1;
        let code = format!("code-{}", fake.next);
        fake.flows.insert(code, (body["code_challenge"].as_str().unwrap().into(), body["email"].as_str().unwrap().into()));
    }
}

fn record(server: &Server, path: &str, query: &HashMap<String, String>, body: &Bytes, headers: &HeaderMap) -> Value {
    let body: Value = if body.is_empty() { Value::Null } else { serde_json::from_slice(body).unwrap() };
    let query = serde_json::to_string(query).unwrap();
    let bearer = headers.get("authorization").map(|v| v.to_str().unwrap().to_string());
    server.state.lock().unwrap().calls.push((path.into(), query, body.clone(), bearer));
    body
}

async fn token(State(s): State<Server>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, raw: Bytes) -> (StatusCode, Json<Value>) {
    let body = record(&s, "/token", &q, &raw, &headers);
    match q.get("grant_type").map(String::as_str) {
        Some("password") => {
            if body["password"] != "right" {
                return error(StatusCode::BAD_REQUEST, "invalid_credentials", "Invalid login credentials");
            }
            let mut fake = s.state.lock().unwrap();
            (StatusCode::OK, Json(Server::issue(&mut fake, body["email"].as_str().unwrap())))
        }
        Some("pkce") => {
            let mut fake = s.state.lock().unwrap();
            let Some((challenge, email)) = fake.flows.remove(body["auth_code"].as_str().unwrap()) else {
                return error(StatusCode::NOT_FOUND, "flow_state_not_found", "invalid flow state, no valid flow state found");
            };
            let digest = URL_SAFE_NO_PAD.encode(Sha256::digest(body["code_verifier"].as_str().unwrap().as_bytes()));
            if digest != challenge {
                return error(StatusCode::FORBIDDEN, "bad_code_verifier", "code challenge does not match previously saved code verifier");
            }
            (StatusCode::OK, Json(Server::issue(&mut fake, &email)))
        }
        Some("refresh_token") => {
            let hold = {
                let mut fake = s.state.lock().unwrap();
                fake.refresh_calls += 1;
                fake.hold_refresh
            };
            if hold {
                s.refresh_arrived.notify_one();
                s.refresh_release.notified().await;
            }
            let mut fake = s.state.lock().unwrap();
            if hold && fake.fail_held_refresh {
                return error(StatusCode::BAD_REQUEST, "refresh_token_not_found", "Invalid Refresh Token: Refresh Token Not Found");
            }
            let presented = body["refresh_token"].as_str().unwrap().to_string();
            match fake.refresh.get_mut(&presented) {
                Some((email, used)) if !*used => {
                    *used = true;
                    let email = email.clone();
                    (StatusCode::OK, Json(Server::issue(&mut fake, &email)))
                }
                _ => error(StatusCode::BAD_REQUEST, "refresh_token_already_used", "Invalid Refresh Token: Already Used"),
            }
        }
        _ => error(StatusCode::BAD_REQUEST, "validation_failed", "unsupported grant_type"),
    }
}

async fn otp(State(s): State<Server>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, raw: Bytes) -> (StatusCode, Json<Value>) {
    let body = record(&s, "/otp", &q, &raw, &headers);
    Server::flow(&mut s.state.lock().unwrap(), &body);
    (StatusCode::OK, Json(json!({})))
}
async fn recover(State(s): State<Server>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, raw: Bytes) -> (StatusCode, Json<Value>) {
    let body = record(&s, "/recover", &q, &raw, &headers);
    Server::flow(&mut s.state.lock().unwrap(), &body);
    (StatusCode::OK, Json(json!({})))
}
async fn signup(State(s): State<Server>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, raw: Bytes) -> (StatusCode, Json<Value>) {
    let body = record(&s, "/signup", &q, &raw, &headers);
    let mut fake = s.state.lock().unwrap();
    Server::flow(&mut fake, &body);
    // Email confirmation on: the answer is the unconfirmed user, no session.
    (StatusCode::OK, Json(json!({"id": Uuid::from_u128(99), "email": body["email"]})))
}
async fn logout(State(s): State<Server>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, raw: Bytes) -> StatusCode {
    record(&s, "/logout", &q, &raw, &headers);
    StatusCode::from_u16(s.state.lock().unwrap().logout_status.unwrap_or(204)).unwrap()
}
async fn user(State(s): State<Server>, Query(q): Query<HashMap<String, String>>, headers: HeaderMap, raw: Bytes) -> (StatusCode, Json<Value>) {
    record(&s, "/user", &q, &raw, &headers);
    let fake = s.state.lock().unwrap();
    let token = headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    match token.and_then(|t| fake.access.get(t)) {
        Some(email) => (StatusCode::OK, Json(json!({"id": fake.users[email], "email": email}))),
        None => error(StatusCode::UNAUTHORIZED, "bad_jwt", "invalid JWT"),
    }
}

async fn gotrue() -> (Server, String) {
    let server = Server { state: Arc::default(), refresh_arrived: Arc::default(), refresh_release: Arc::default() };
    let routes = Router::new()
        .route("/token", post(token))
        .route("/otp", post(otp))
        .route("/recover", post(recover))
        .route("/signup", post(signup))
        .route("/logout", post(logout))
        .route("/user", put(user))
        .with_state(server.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
    (server, base)
}

struct Tab {
    auth: Arc<GoTrue>,
    storage: Arc<Memory>,
}

fn tab(base: &str, storage: Arc<Memory>, lock: TabLock, clock: Arc<TestClock>) -> Tab {
    let ports = AuthPorts { storage: storage.clone(), random: Arc::new(OsRandom), clock, lock: Arc::new(lock) };
    Tab { auth: Arc::new(GoTrue::new(GoTrueConfig { url: base.into(), api_key: None }, ports)), storage }
}

fn new_tab(base: &str) -> Tab {
    tab(base, Arc::default(), TabLock::default(), Arc::new(TestClock(AtomicI64::new(1_800_000_000))))
}

fn last_call(server: &Server, path: &str) -> (Value, HashMap<String, String>, Option<String>) {
    let fake = server.state.lock().unwrap();
    let (_, query, body, bearer) = fake.calls.iter().rev().find(|c| c.0 == path).unwrap().clone();
    (body, serde_json::from_str(&query).unwrap(), bearer)
}

fn pending_code(server: &Server, email: &str) -> String {
    server.state.lock().unwrap().flows.iter().find(|(_, (_, e))| e == email).unwrap().0.clone()
}

const HOME: &str = "http://127.0.0.1:5173/en/home";

#[test]
fn pkce_challenge_matches_rfc7636_appendix_b() {
    // Verifier and expected challenge copied verbatim from RFC 7636 Appendix B:
    // https://www.rfc-editor.org/rfc/rfc7636.html#appendix-B
    assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
}

#[test]
fn callback_parsing_reads_code_or_error_and_strip_keeps_other_state() {
    assert_eq!(parse_callback("http://a.test/en/home?code=abc&view=new"), Callback::Code("abc".into()));
    assert_eq!(
        parse_callback("http://a.test/en/home#error=access_denied&error_code=otp_expired&error_description=Email+link+is+invalid"),
        Callback::Error { code: Some("otp_expired".into()), description: Some("Email link is invalid".into()) }
    );
    assert_eq!(parse_callback("http://a.test/en/home?view=new"), Callback::None);
    assert_eq!(strip_callback("http://a.test/en/home?code=abc&view=new"), "http://a.test/en/home?view=new");
    assert_eq!(strip_callback("http://a.test/en/home?error=x&error_description=y#keep"), "http://a.test/en/home#keep");
}

#[tokio::test]
async fn magic_link_callback_exchanges_the_stored_verifier_and_signs_in() {
    let (server, base) = gotrue().await;
    let t = new_tab(&base);
    assert_eq!(t.auth.magic_link("ada@example.test", HOME, None).await.unwrap(), AuthOutcome::EmailSent);
    let (body, query, _) = last_call(&server, "/otp");
    assert_eq!(query.get("redirect_to").map(String::as_str), Some(HOME));
    assert_eq!(body["code_challenge_method"], "s256");
    assert_eq!(body["create_user"], true);
    let verifier = t.storage.get(VERIFIER_KEY).expect("verifier persisted across the redirect");
    assert_eq!(body["code_challenge"], challenge(&verifier));

    let code = pending_code(&server, "ada@example.test");
    let Callback::Code(code) = parse_callback(&format!("{HOME}?code={code}")) else { panic!("code expected") };
    let AuthOutcome::SignedIn(session) = t.auth.exchange_code(&code).await.unwrap() else { panic!("signed in expected") };
    assert_eq!(session.email, "ada@example.test");
    assert_eq!(t.storage.get(VERIFIER_KEY), None, "a verifier is single use");
    assert_eq!(t.auth.current(), Some(session));
    assert!(t.storage.get(SESSION_KEY).is_some());
}

#[tokio::test]
async fn recovery_link_yields_password_recovery_then_updates_the_password() {
    let (server, base) = gotrue().await;
    let t = new_tab(&base);
    assert_eq!(t.auth.recover("ada@example.test", HOME, None).await.unwrap(), AuthOutcome::EmailSent);
    assert!(t.storage.get(VERIFIER_KEY).unwrap().ends_with("/PASSWORD_RECOVERY"));
    let (body, _, _) = last_call(&server, "/recover");
    let verifier = t.storage.get(VERIFIER_KEY).unwrap();
    assert_eq!(body["code_challenge"], challenge(verifier.trim_end_matches("/PASSWORD_RECOVERY")));

    let code = pending_code(&server, "ada@example.test");
    let AuthOutcome::PasswordRecovery(session) = t.auth.exchange_code(&code).await.unwrap() else { panic!("recovery expected") };
    let AuthOutcome::PasswordUpdated(updated) = t.auth.update_password("n3w-secret").await.unwrap() else { panic!("updated expected") };
    assert_eq!(updated.user_id, session.user_id);
    let (body, _, bearer) = last_call(&server, "/user");
    assert_eq!(body["password"], "n3w-secret");
    assert!(bearer.unwrap().starts_with("Bearer access-"));
}

#[tokio::test]
async fn wrong_or_expired_code_fails_without_a_session_and_consumes_the_verifier() {
    let (_server, base) = gotrue().await;
    let t = new_tab(&base);
    t.auth.magic_link("ada@example.test", HOME, None).await.unwrap();
    let error = t.auth.exchange_code("not-issued").await.unwrap_err();
    assert_eq!(error.kind, AuthErrorKind::InvalidOrExpiredCode);
    assert_eq!(t.storage.get(VERIFIER_KEY), None);
    assert_eq!(t.auth.current(), None);
}

#[tokio::test]
async fn callback_without_a_stored_verifier_is_refused_without_calling_gotrue() {
    let (server, base) = gotrue().await;
    let t = new_tab(&base);
    let error = t.auth.exchange_code("code-1").await.unwrap_err();
    assert_eq!(error.kind, AuthErrorKind::InvalidOrExpiredCode);
    assert!(server.state.lock().unwrap().calls.is_empty());
}

#[tokio::test]
async fn signup_with_confirmation_sends_challenge_and_reports_email_sent() {
    let (server, base) = gotrue().await;
    let t = new_tab(&base);
    assert_eq!(t.auth.sign_up("new@example.test", "s3cret-pass", HOME, None).await.unwrap(), AuthOutcome::EmailSent);
    let (body, query, _) = last_call(&server, "/signup");
    assert_eq!(query.get("redirect_to").map(String::as_str), Some(HOME));
    assert_eq!(body["code_challenge"], challenge(&t.storage.get(VERIFIER_KEY).unwrap()));
    assert_eq!(t.auth.current(), None);
}

#[tokio::test]
async fn google_authorize_url_carries_provider_redirect_and_challenge() {
    let (_server, base) = gotrue().await;
    let t = new_tab(&base);
    let url = url::Url::parse(&t.auth.google_authorize_url(HOME)).unwrap();
    assert_eq!(url.path(), "/authorize");
    let q: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(q["provider"], "google");
    assert_eq!(q["redirect_to"], HOME);
    assert_eq!(q["code_challenge_method"], "s256");
    assert_eq!(q["code_challenge"], challenge(&t.storage.get(VERIFIER_KEY).unwrap()));
}

#[tokio::test]
async fn invalid_credentials_keep_gotrue_message_and_captcha_is_forwarded_only_when_present() {
    let (server, base) = gotrue().await;
    let t = new_tab(&base);
    let error = t.auth.sign_in_password("ada@example.test", "wrong", Some("turnstile-token")).await.unwrap_err();
    assert_eq!(error.kind, AuthErrorKind::InvalidCredentials);
    assert_eq!(error.message.as_deref(), Some("Invalid login credentials"));
    let (body, query, _) = last_call(&server, "/token");
    assert_eq!(query["grant_type"], "password");
    assert_eq!(body["gotrue_meta_security"]["captcha_token"], "turnstile-token");

    t.auth.sign_in_password("ada@example.test", "right", None).await.unwrap();
    let (body, _, _) = last_call(&server, "/token");
    assert!(body.get("gotrue_meta_security").is_none());
}

#[tokio::test]
async fn two_tabs_refresh_once_and_both_use_the_rotated_session() {
    let (server, base) = gotrue().await;
    let storage = Arc::new(Memory::default());
    let lock = TabLock::default();
    let clock = Arc::new(TestClock(AtomicI64::new(1_800_000_000)));
    let a = tab(&base, storage.clone(), lock.clone(), clock.clone());
    let b = tab(&base, storage.clone(), lock, clock.clone());
    let AuthOutcome::SignedIn(session) = a.auth.sign_in_password("ada@example.test", "right", None).await.unwrap() else { panic!() };
    let first = a.auth.access_token(session.user_id).await.unwrap().unwrap();
    clock.0.fetch_add(3600, Ordering::SeqCst);

    let (from_a, from_b) = tokio::join!(a.auth.access_token(session.user_id), b.auth.access_token(session.user_id));
    let (from_a, from_b) = (from_a.unwrap().unwrap(), from_b.unwrap().unwrap());
    assert_ne!(from_a, first);
    assert_eq!(from_a, from_b, "the second tab adopts the rotated session instead of reusing the old refresh token");
    assert_eq!(server.state.lock().unwrap().refresh_calls, 1);
    assert_eq!(b.auth.current().unwrap().user_id, session.user_id);
}

#[tokio::test]
async fn a_token_the_host_rejected_is_renewed_on_demand_before_its_expiry() {
    let (server, base) = gotrue().await;
    let t = new_tab(&base);
    let AuthOutcome::SignedIn(session) = t.auth.sign_in_password("ada@example.test", "right", None).await.unwrap() else { panic!() };
    let rejected = t.auth.access_token(session.user_id).await.unwrap().unwrap();

    let renewed = t.auth.renew(session.user_id).await.unwrap().expect("ada still has a session");
    assert_eq!(renewed.user_id, session.user_id);
    assert_eq!(server.state.lock().unwrap().refresh_calls, 1);
    assert_ne!(t.auth.access_token(session.user_id).await.unwrap().unwrap(), rejected);
}

#[tokio::test]
async fn two_tabs_renewing_the_same_rejected_token_rotate_it_once() {
    let (server, base) = gotrue().await;
    let storage = Arc::new(Memory::default());
    let lock = TabLock::default();
    let clock = Arc::new(TestClock(AtomicI64::new(1_800_000_000)));
    let a = tab(&base, storage.clone(), lock.clone(), clock.clone());
    let b = tab(&base, storage.clone(), lock, clock);
    let AuthOutcome::SignedIn(session) = a.auth.sign_in_password("ada@example.test", "right", None).await.unwrap() else { panic!() };

    let (from_a, from_b) = tokio::join!(a.auth.renew(session.user_id), b.auth.renew(session.user_id));
    assert_eq!(from_a.unwrap(), from_b.unwrap(), "the waiting tab adopts the rotation instead of spending the used refresh token");
    assert_eq!(server.state.lock().unwrap().refresh_calls, 1);
}

#[tokio::test]
async fn renewing_for_a_user_who_is_no_longer_signed_in_here_sends_nothing() {
    let (server, base) = gotrue().await;
    let t = new_tab(&base);
    t.auth.sign_in_password("bob@example.test", "right", None).await.unwrap();

    assert_eq!(t.auth.renew(Uuid::from_u128(999)).await.unwrap(), None);
    assert_eq!(server.state.lock().unwrap().refresh_calls, 0);
}

#[tokio::test]
async fn a_late_refresh_failure_never_clears_a_newer_accounts_session() {
    let (server, base) = gotrue().await;
    let storage = Arc::new(Memory::default());
    let clock = Arc::new(TestClock(AtomicI64::new(1_800_000_000)));
    let a = tab(&base, storage.clone(), TabLock::default(), clock.clone());
    let b = tab(&base, storage.clone(), TabLock::default(), clock.clone());
    let AuthOutcome::SignedIn(ada) = a.auth.sign_in_password("ada@example.test", "right", None).await.unwrap() else { panic!() };
    clock.0.fetch_add(3600, Ordering::SeqCst);
    {
        let mut fake = server.state.lock().unwrap();
        fake.hold_refresh = true;
        fake.fail_held_refresh = true;
    }
    let pending = tokio::spawn({
        let auth = a.auth.clone();
        async move { auth.access_token(ada.user_id).await }
    });
    server.refresh_arrived.notified().await;
    server.state.lock().unwrap().hold_refresh = false;
    let AuthOutcome::SignedIn(bob) = b.auth.sign_in_password("bob@example.test", "right", None).await.unwrap() else { panic!() };
    server.refresh_release.notify_one();

    assert_eq!(pending.await.unwrap().unwrap(), None, "ada's request must not proceed under another account");
    assert_eq!(a.auth.current(), Some(bob));
}

#[tokio::test]
async fn a_late_refresh_success_never_overwrites_a_newer_accounts_session() {
    let (server, base) = gotrue().await;
    let storage = Arc::new(Memory::default());
    let clock = Arc::new(TestClock(AtomicI64::new(1_800_000_000)));
    let a = tab(&base, storage.clone(), TabLock::default(), clock.clone());
    let b = tab(&base, storage.clone(), TabLock::default(), clock.clone());
    let AuthOutcome::SignedIn(ada) = a.auth.sign_in_password("ada@example.test", "right", None).await.unwrap() else { panic!() };
    clock.0.fetch_add(3600, Ordering::SeqCst);
    server.state.lock().unwrap().hold_refresh = true;
    let pending = tokio::spawn({
        let auth = a.auth.clone();
        async move { auth.access_token(ada.user_id).await }
    });
    server.refresh_arrived.notified().await;
    server.state.lock().unwrap().hold_refresh = false;
    let AuthOutcome::SignedIn(bob) = b.auth.sign_in_password("bob@example.test", "right", None).await.unwrap() else { panic!() };
    server.refresh_release.notify_one();

    assert_eq!(pending.await.unwrap().unwrap(), None);
    assert_eq!(a.auth.current(), Some(bob));
}

#[tokio::test]
async fn sign_out_with_an_already_invalid_token_still_clears_the_local_session() {
    let (server, base) = gotrue().await;
    let t = new_tab(&base);
    t.auth.sign_in_password("ada@example.test", "right", None).await.unwrap();
    server.state.lock().unwrap().logout_status = Some(401);
    assert_eq!(t.auth.sign_out().await.unwrap(), AuthOutcome::SignedOut);
    assert_eq!(t.auth.current(), None);
    let (_, query, bearer) = last_call(&server, "/logout");
    assert_eq!(query.get("scope").map(String::as_str), Some("global"));
    assert!(bearer.is_some());
}
