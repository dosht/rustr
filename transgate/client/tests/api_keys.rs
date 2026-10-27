//! The key-screen transport (WEB-045/046): the session routes under `/api/keys`,
//! served here by the REAL host DTO builders (`transgate_api::public`), and the
//! results driven through the real `AppState`. The create body is parsed with the
//! host's own `parse_new_key`, so a request the host would refuse fails here.
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde_json::json;
use transgate_api::public::{CreatedKeyResponse, KeyListResponse, RevokedKeyResponse, SigningSecretResponse, key_view, parse_new_key};
use transgate_client::api::HttpApi;
use transgate_client::auth::{AuthPorts, AuthStorage, Clock, GoTrue, GoTrueConfig, LockGuard, OsRandom, SESSION_KEY, SessionLock};
use transgate_client::future::BoxFuture;
use transgate_core::api::keys::{CreatedKey, ExpirationPolicy, KeyView, NewKey};
use transgate_core::api::{KeyId, Scope};
use transgate_core::Micros;
use transgate_core::presentation::api_keys::{Effect, Event};
use transgate_core::presentation::app::{AppConfig, AppEffect, AppEvent, AppIntent, AppState, AppVm, AuthEvent, SessionFacts};
use transgate_core::presentation::fixtures::{NOW, session_profile};
use transgate_core::presentation::layout::Viewport;
use uuid::Uuid;

#[derive(Default)]
struct Memory(Mutex<HashMap<String, String>>);
impl AuthStorage for Memory {
    fn get(&self, key: &str) -> Option<String> { self.0.lock().unwrap().get(key).cloned() }
    fn set(&self, key: &str, value: &str) { self.0.lock().unwrap().insert(key.into(), value.into()); }
    fn remove(&self, key: &str) { self.0.lock().unwrap().remove(key); }
}
struct NoContention;
impl SessionLock for NoContention {
    fn acquire(&self) -> BoxFuture<'_, LockGuard> { Box::pin(async { LockGuard::new(|| {}) }) }
}
struct Fixed(AtomicI64);
impl Clock for Fixed {
    fn now_secs(&self) -> i64 { self.0.load(Ordering::SeqCst) }
}

const USER: Uuid = Uuid::from_u128(1);
/// Whole milliseconds: the wire is `toISOString()` text.
const CREATED: Micros = Micros(1_800_000_123_000);
const EXPIRES: Micros = Micros(1_802_592_123_000);

fn key(id: u128, name: &str, active: bool, expires_at: Option<Micros>, scopes: &[&str]) -> KeyView {
    KeyView {
        id: KeyId(Uuid::from_u128(id)),
        name: name.into(),
        created_at: CREATED,
        expires_at,
        active,
        scopes: scopes.iter().map(|s| s.to_string()).collect(),
        expiration_policy: if expires_at.is_some() { ExpirationPolicy::Days30 } else { ExpirationPolicy::Never },
        hint: Some("sk_abcd".into()),
    }
}

fn stored() -> Vec<KeyView> {
    vec![
        key(10, "ci", true, Some(EXPIRES), &["transcriptions:read", "transcriptions:create"]),
        // A migrated row keeps a scope string this build does not know.
        key(11, "legacy", false, None, &["legacy:all"]),
    ]
}

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<NewKey>>>);

async fn host(seen: Seen) -> String {
    let list = || async { Json(KeyListResponse { keys: stored().iter().map(key_view).collect() }) };
    let secret = || async { Json(SigningSecretResponse { secret: "whsec_signing".into() }) };
    let create = move |body: Bytes| {
        let seen = seen.clone();
        async move {
            let Ok(new) = parse_new_key(&body) else { return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid", "code": "invalid_request"}))).into_response() };
            seen.0.lock().unwrap().push(new.clone());
            let created = KeyView { id: KeyId(Uuid::from_u128(12)), name: new.name, ..key(12, "", true, None, &["transcriptions:read"]) };
            (StatusCode::CREATED, Json(CreatedKeyResponse { key: key_view(&created), api_key: "sk_plaintext_once".into() })).into_response()
        }
    };
    let revoke = |Path(id): Path<Uuid>| async move {
        let revoked = KeyView { active: false, ..key(id.as_u128(), "ci", true, None, &["transcriptions:read"]) };
        Json(RevokedKeyResponse { key: key_view(&revoked) })
    };
    let routes = Router::new()
        .route("/api/keys", get(list).post(create))
        .route("/api/keys/signing-secret", get(secret))
        .route("/api/keys/{id}", delete(revoke));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
    base
}

fn api(base: &str) -> HttpApi {
    let storage = Arc::new(Memory::default());
    storage.set(SESSION_KEY, &json!({"access_token": "t", "refresh_token": "r", "expires_at": 4_000_000_000i64,
                                    "user_id": USER, "email": "ada@example.test"}).to_string());
    let ports = AuthPorts { storage, random: Arc::new(OsRandom), clock: Arc::new(Fixed(AtomicI64::new(1_800_000_000))), lock: Arc::new(NoContention) };
    HttpApi::new(base, Arc::new(GoTrue::new(GoTrueConfig { url: "http://127.0.0.1:9".into(), api_key: None }, ports)))
}

#[tokio::test]
async fn the_list_and_secret_read_what_the_host_dtos_write() {
    let api = api(&host(Seen::default()).await);
    assert_eq!(api.list_keys(USER).await.unwrap(), stored());
    assert_eq!(api.signing_secret(USER).await.unwrap(), "whsec_signing");
}

#[tokio::test]
async fn a_create_sends_a_body_the_host_parses_back_to_the_same_request() {
    let seen = Seen::default();
    let api = api(&host(seen.clone()).await);
    let request = NewKey {
        name: "deploy".into(),
        scopes: vec![Scope::TranscriptionsRead, Scope::TranscriptionsCreate],
        expiration_policy: ExpirationPolicy::Custom,
        custom_days: Some(45),
    };
    let CreatedKey { key, api_key } = api.create_key(USER, &request).await.unwrap();
    assert_eq!(*seen.0.lock().unwrap(), vec![request], "the host's own parser read exactly what was sent");
    assert_eq!((key.name.as_str(), api_key.as_str()), ("deploy", "sk_plaintext_once"));
}

#[tokio::test]
async fn a_revoke_returns_the_inactive_key() {
    let api = api(&host(Seen::default()).await);
    let revoked = api.revoke_key(USER, KeyId(Uuid::from_u128(10))).await.unwrap();
    assert_eq!((revoked.id, revoked.active), (KeyId(Uuid::from_u128(10)), false));
}

/// The real client answers drive the real `AppState` from opening the route to a
/// shown-once key; the plaintext leaves state when the route does.
#[tokio::test]
async fn the_key_screen_loads_and_creates_through_the_real_client() {
    let api = api(&host(Seen::default()).await);
    let mut s = AppState::new(AppConfig { site_url: "http://127.0.0.1:37363".into(), captcha: false });
    s.update(AppEvent::User(AppIntent::Started { url: "/dashboard/api-keys".into(), viewport: Viewport::default(), utc_offset_minutes: 0 }), NOW);
    let profile = session_profile();
    let facts = SessionFacts { user_id: profile.account.0, email: profile.email.clone(), expires_at: Micros(NOW.0 + 3_600_000_000) };
    s.update(AppEvent::Auth { epoch: s.epoch(), event: AuthEvent::Restored { facts: Some(facts) } }, NOW);
    let opened = s.update(AppEvent::SessionLoaded { epoch: s.epoch(), result: Ok(profile) }, NOW);
    let wanted: Vec<_> = opened.iter().filter_map(|e| match e { AppEffect::ApiKeys { effect, .. } => Some(effect.clone()), _ => None }).collect();
    assert_eq!(wanted, vec![Effect::LoadKeys, Effect::LoadSigningSecret], "{opened:?}");

    let epoch = s.epoch();
    let keys = api.list_keys(USER).await.map_err(|e| e.failure().unwrap());
    s.update(AppEvent::ApiKeys { epoch, event: Event::KeysLoaded { result: keys } }, NOW);
    let secret = api.signing_secret(USER).await.map_err(|e| e.failure().unwrap());
    s.update(AppEvent::ApiKeys { epoch, event: Event::SecretLoaded { result: secret } }, NOW);
    let AppVm::SignedIn(vm) = s.view(NOW) else { panic!("signed in") };
    let screen = vm.api_keys.expect("the screen exists while its route is open");
    assert_eq!(screen.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["ci", "legacy"]);
    assert_eq!(screen.signing_secret.as_deref(), Some("whsec_signing"));
}
