use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use billing_core::ledger::Package;
use media_ports::StoreKind;

use super::*;
use crate::api::callbacks::{CallbackDelivery, DeliveryState, SendOutcome};
use crate::api::keys::ExpirationPolicy;
use crate::api::limits::RateCounts;
use crate::api::{Scope, keys::KEY_PREFIX};
use crate::ids::MICROS_PER_DAY;
use crate::jobs::app::{JobsConfig, JobsServices, NewDraft, StartRequest};
use crate::jobs::domain::{JobStatus, StorageProvider};
use crate::jobs::routing::{ModelConfig, Preferences, RoutingConfig};
use crate::jobs::testing::{CountingLimiter, FakeObjects, FakeTranscriber, MemStore, NoSleep, TestClock};
use crate::languages::Language;
use crate::presentation::fixtures;

const NOW: Micros = Micros(1_800_000_000_000_000);

fn run<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Deterministic stand-ins: the digest is the hex of the input, the "bcrypt"
/// hash of `k` is `$2a$06$k`, and the HMAC shows its inputs.
#[derive(Default)]
struct Crypto {
    tokens: AtomicU32,
    digests: AtomicU32,
    legacy_checks: AtomicU32,
}

impl ApiCrypto for Crypto {
    fn random_token(&self) -> String {
        format!("{:0>43}=", self.tokens.fetch_add(1, Ordering::SeqCst) + 1)
    }
    fn sha256_hex(&self, data: &[u8]) -> String {
        self.digests.fetch_add(1, Ordering::SeqCst);
        hex(data)
    }
    fn hmac_sha256_hex(&self, secret: &[u8], message: &[u8]) -> String {
        format!("hmac({},{})", hex(secret), hex(message))
    }
    fn verify_legacy<'a>(&'a self, key: &'a str, bcrypt_hash: &'a str) -> BoxFuture<'a, bool> {
        self.legacy_checks.fetch_add(1, Ordering::SeqCst);
        let ok = bcrypt_hash == format!("$2a$06${key}");
        Box::pin(async move { ok })
    }
}

/// The D3 reference semantics over calendar windows.
#[derive(Default)]
struct Limiter {
    minute: Mutex<BTreeMap<(KeyId, Micros), u32>>,
    hour: Mutex<BTreeMap<(KeyId, Micros), u32>>,
    fail: AtomicBool,
}

impl ApiRateLimiter for Limiter {
    fn consume(&self, key: KeyId, windows: RateWindows, limits: RateLimits) -> BoxFuture<'_, Result<RateCounts, StoreError>> {
        if self.fail.load(Ordering::SeqCst) {
            return Box::pin(async { Err(StoreError::Unavailable("limiter down".into())) });
        }
        let mut m = self.minute.lock().unwrap();
        let mut h = self.hour.lock().unwrap();
        let mc = m.entry((key, windows.minute_start)).or_default();
        let hc = h.entry((key, windows.hour_start)).or_default();
        let counts = limits::consume(limits, *mc, *hc);
        (*mc, *hc) = (counts.minute, counts.hour);
        Box::pin(async move { Ok(counts) })
    }
}

struct World {
    store: MemStore,
    clock: Arc<TestClock>,
    crypto: Arc<Crypto>,
    limiter: Arc<Limiter>,
    soniox: Arc<FakeTranscriber>,
    jobs: Arc<Jobs>,
}

impl World {
    fn new() -> World {
        let store = MemStore::default();
        store.with(|d| {
            d.accounts.insert(fixtures::account());
            d.accounts.insert(other());
            d.packages.push(Package {
                id: 1,
                account: fixtures::account(),
                payment_id: None,
                name: Some("Pro".into()),
                minutes_included: 100,
                start_at: Micros(NOW.0 - 10 * MICROS_PER_DAY),
                end_at: Micros(NOW.0 + 30 * MICROS_PER_DAY),
            });
        });
        let clock = Arc::new(TestClock(Mutex::new(NOW)));
        let soniox = Arc::new(FakeTranscriber::new(media_ports::Provider::Soniox));
        let jobs = Arc::new(Jobs::new(
            JobsServices {
                store: Arc::new(store.clone()),
                clock: clock.clone(),
                objects: vec![Arc::new(FakeObjects::default())],
                transcribers: vec![soniox.clone(), Arc::new(FakeTranscriber::new(media_ports::Provider::Deepgram))],
                stager: None,
                sleeper: Arc::new(NoSleep),
                rate_limiter: Arc::new(CountingLimiter { hits: Mutex::new(BTreeMap::new()) }),
                paid_captions: None,
                video_metadata: None,
            },
            JobsConfig {
                routing: RoutingConfig { soniox_max_duration_minutes: 200 },
                models: ModelConfig { soniox: "stt-async-v5".into(), soniox_v5: "stt-async-v5".into(), deepgram: "whisper-large".into(), deepgram_account_overrides: vec![] },
                upload_storage: StorageProvider::Gcs,
                upload_bucket: "bucket".into(),
                archive_store: StoreKind::Gcs,
                archive_bucket: "bucket".into(),
                staging_store: StoreKind::Gcs,
                staging_bucket: "bucket".into(),
                callback_base_url: "https://transgate.test/".into(),
                soniox_auto_delete: false,
                ssrf_protection: true,
                freemium_lists: Default::default(),
            },
        ));
        World { store, clock, crypto: Arc::new(Crypto::default()), limiter: Arc::new(Limiter::default()), soniox, jobs }
    }

    fn api(&self) -> Api {
        Api::new(ApiServices {
            store: Arc::new(self.store.clone()),
            jobs: self.jobs.clone(),
            crypto: self.crypto.clone(),
            limiter: self.limiter.clone(),
            clock: self.clock.clone(),
            outbox: Arc::new(self.store.clone()),
            limits: RateLimits::default(),
        })
    }

    fn key(&self, id: u128, scopes: &[&str], digest_of: Option<&str>, legacy_of: Option<&str>) -> ApiKey {
        let key = ApiKey {
            id: KeyId(Uuid::from_u128(id)),
            account: fixtures::account(),
            name: "ci".into(),
            created_at: Micros(NOW.0 - MICROS_PER_DAY),
            expires_at: None,
            last_used_at: None,
            active: true,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            expiration_policy: ExpirationPolicy::Never,
            metadata: serde_json::json!({}),
            digest: digest_of.map(|k| hex(k.as_bytes())),
            legacy_bcrypt: legacy_of.map(|k| format!("$2a$06${k}")),
            hint: None,
        };
        self.store.with(|d| d.api.keys.push(key.clone()));
        key
    }

    fn stored_key(&self, id: u128) -> ApiKey {
        self.store.with(|d| d.api.keys.iter().find(|k| k.id == KeyId(Uuid::from_u128(id))).unwrap().clone())
    }

    fn usage(&self) -> i64 {
        self.store.with(|d| d.usage.iter().map(|u| u.minutes).sum())
    }

    fn draft(&self, minutes: i32) -> Job {
        let draft = NewDraft { name: "talk.mp3".into(), size_bytes: Some(10), language: Language::from_name("English"), speakers: false, duration_minutes: Some(minutes) };
        run(self.jobs.create_draft(fixtures::account(), draft)).unwrap()
    }
}

fn other() -> AccountId {
    AccountId(Uuid::from_u128(0xbad))
}

fn secret(n: u32) -> String {
    format!("{KEY_PREFIX}{n:0>43}=")
}

fn both() -> [&'static str; 2] {
    [Scope::TranscriptionsRead.as_str(), Scope::TranscriptionsCreate.as_str()]
}

// API-001, API-002: a value without the key shape is refused before any store read or digest.
#[test]
fn api_001_002_a_malformed_bearer_is_refused_without_a_lookup() {
    let w = World::new();
    for bad in ["", "Bearer x", "eyJhbGciOiJIUzI1NiJ9.e30.sig", "sk_short"] {
        assert_eq!(run(w.api().authenticate(bad)), Err(ApiError::Unauthorized(keys::INVALID_KEY)), "{bad:?}");
    }
    assert_eq!(w.crypto.digests.load(Ordering::SeqCst), 0);
}

// API-003, D2: a digest key verifies with its stored scopes and updates last_used_at; an
// inactive key, an expired key and an unknown key are refused with the source messages.
#[test]
fn api_003_digest_keys_verify_touch_and_refuse_inactive_or_expired() {
    let w = World::new();
    w.key(1, &both(), Some(&secret(1)), None);
    let actor = run(w.api().authenticate(&secret(1))).unwrap();
    assert_eq!(actor, Actor::ApiKey { account: fixtures::account(), key: KeyId(Uuid::from_u128(1)), scopes: both().iter().map(|s| s.to_string()).collect() });
    assert_eq!(w.stored_key(1).last_used_at, Some(NOW));
    w.store.with(|d| d.api.keys[0].active = false);
    assert_eq!(run(w.api().authenticate(&secret(1))), Err(ApiError::Unauthorized(keys::INVALID_KEY)));
    w.store.with(|d| {
        d.api.keys[0].active = true;
        d.api.keys[0].expires_at = Some(Micros(NOW.0 - 1));
    });
    assert_eq!(run(w.api().authenticate(&secret(1))), Err(ApiError::Unauthorized(keys::EXPIRED_KEY)));
    assert_eq!(run(w.api().authenticate(&secret(2))), Err(ApiError::Unauthorized(keys::INVALID_KEY)));
}

// D2: a migrated bcrypt key verifies on a digest miss, then is upgraded (digest and hint written,
// bcrypt nulled) and never scanned again; expired or inactive legacy rows are not candidates.
#[test]
fn d2_a_legacy_bcrypt_key_upgrades_on_first_use() {
    let w = World::new();
    w.key(1, &both(), None, Some(&secret(1)));
    let expired = w.key(2, &both(), None, Some(&secret(2)));
    w.store.with(|d| d.api.keys.iter_mut().find(|k| k.id == expired.id).unwrap().expires_at = Some(Micros(NOW.0 - 1)));
    assert!(matches!(run(w.api().authenticate(&secret(1))), Ok(Actor::ApiKey { .. })));
    let upgraded = w.stored_key(1);
    assert_eq!((upgraded.digest, upgraded.legacy_bcrypt, upgraded.hint.as_deref()), (Some(hex(secret(1).as_bytes())), None, Some("sk_00000")));
    assert_eq!(w.crypto.legacy_checks.load(Ordering::SeqCst), 1, "the expired legacy row is not scanned");
    assert!(run(w.api().authenticate(&secret(1))).is_ok());
    assert_eq!(w.crypto.legacy_checks.load(Ordering::SeqCst), 1, "the digest answers from now on");
    assert_eq!(run(w.api().authenticate(&secret(2))), Err(ApiError::Unauthorized(keys::INVALID_KEY)));
}

// D1, API-D01: a refused scope answers 403 and is not counted. D3, RTL-001, API-D04: the 61st
// request in a minute is 429 with the header values, and a limiter outage is 503, not fail-open.
#[test]
fn d1_d3_scope_before_the_limiter_then_429_and_503() {
    let w = World::new();
    let api = w.api();
    let reader = Actor::ApiKey { account: fixtures::account(), key: KeyId(Uuid::from_u128(1)), scopes: vec![Scope::TranscriptionsRead.as_str().into()] };
    let job = Resource::Job { id: JobId(1), owner: fixtures::account() };
    assert_eq!(run(api.authorize(&reader, Action::TranscriptionCreate, Resource::Account(fixtures::account()))), Err(ApiError::Forbidden));
    for n in 1..=60 {
        let rate = run(api.authorize(&reader, Action::TranscriptionRead, job)).unwrap();
        assert_eq!(rate.remaining_minute, 60 - n, "the refused create was not counted");
    }
    match run(api.authorize(&reader, Action::TranscriptionRead, job)) {
        Err(ApiError::RateLimited(rate)) => assert_eq!((rate.allowed, rate.remaining_minute, rate.remaining_hour), (false, 0, 940)),
        other => panic!("{other:?}"),
    }
    assert_eq!(ApiError::RateLimited(limits::decide(RateWindows::at(NOW), RateLimits::default(), RateCounts { allowed: false, minute: 60, hour: 60 })).http_status(), 429);
    w.limiter.fail.store(true, Ordering::SeqCst);
    let down = run(api.authorize(&reader, Action::TranscriptionRead, job));
    assert!(matches!(down, Err(ApiError::Unavailable(_))), "{down:?}");
    assert_eq!(down.unwrap_err().http_status(), 503);
}

// D4: the same Idempotency-Key and body replays the 201 without another job or charge; the same
// key with another body is 409 request_conflict and changes nothing; no key keeps the source behavior.
#[test]
fn d4_an_idempotent_start_charges_once_and_conflicts_on_another_body() {
    let w = World::new();
    let api = w.api();
    let creator = Actor::ApiKey { account: fixtures::account(), key: KeyId(Uuid::from_u128(1)), scopes: vec![Scope::TranscriptionsCreate.as_str().into()] };
    let a = w.draft(12);
    let b = w.draft(5);
    let request = |id: JobId| PublicStart::Jobs(StartRequest { job_ids: vec![id], locale: None, preferences: Preferences::default() });
    w.soniox.accept("tx-1");
    let first = run(api.start(&creator, request(a.id), Some("retry-1".into()))).unwrap();
    let first = first.result.unwrap();
    assert!(!first.replayed);
    assert_eq!((first.answer.results[0].job_id, w.usage()), (a.id, 12));
    let again = run(api.start(&creator, request(a.id), Some("retry-1".into()))).unwrap().result.unwrap();
    assert!(again.replayed);
    assert_eq!(again.answer, first.answer, "the stored 201");
    assert_eq!(w.usage(), 12, "charged once");
    let other = run(api.start(&creator, request(b.id), Some("retry-1".into()))).unwrap().result;
    assert_eq!(other.as_ref().map_err(|e| (e.http_status(), e.code())), Err((409, "request_conflict")));
    assert_eq!((w.usage(), w.store.with(|d| d.jobs[&b.id.0].status)), (12, JobStatus::Draft), "nothing changed");
    assert!(matches!(run(api.start(&creator, request(b.id), Some(String::new()))).unwrap().result, Err(ApiError::Invalid(_))));
    // Remembered for 24 hours only.
    w.clock.advance_minutes(24 * 60 + 1);
    w.soniox.accept("tx-2");
    let later = run(api.start(&creator, request(b.id), Some("retry-1".into()))).unwrap().result.unwrap();
    assert!(!later.replayed);
    assert_eq!(w.usage(), 17);
}

// API-002/005/019, API-D02: the owner's session creates (plaintext once, digest stored, expiry
// applied), lists newest first without secrets, and revokes; another account cannot revoke, and
// a key actor cannot manage keys at all.
#[test]
fn api_005_019_d02_keys_are_managed_by_their_owners_session_only() {
    let w = World::new();
    let api = w.api();
    let owner = Actor::Session { account: fixtures::account() };
    let new = NewKey { name: "ci".into(), scopes: vec![Scope::TranscriptionsRead], expiration_policy: ExpirationPolicy::Days30, custom_days: None };
    let created = run(api.create_key(&owner, new.clone())).unwrap();
    assert!(keys::well_formed(&created.api_key));
    assert_eq!((created.key.scopes.clone(), created.key.expires_at, created.key.active), (vec!["transcriptions:read".to_string()], Some(Micros(NOW.0 + 30 * MICROS_PER_DAY)), true));
    let stored = w.stored_key(created.key.id.0.as_u128());
    assert_eq!((stored.digest, stored.legacy_bcrypt), (Some(hex(created.api_key.as_bytes())), None));
    assert!(run(api.authenticate(&created.api_key)).is_ok(), "the plaintext authenticates");
    w.clock.advance_minutes(1);
    let second = run(api.create_key(&owner, NewKey { name: "ops".into(), ..new.clone() })).unwrap();
    let listed: Vec<String> = run(api.list_keys(&owner)).unwrap().into_iter().map(|k| k.name).collect();
    assert_eq!(listed, vec!["ops".to_string(), "ci".to_string()], "newest first");
    let foreign = Actor::Session { account: other() };
    assert_eq!(run(api.revoke_key(&foreign, second.key.id)), Err(ApiError::NotFound));
    assert!(w.stored_key(second.key.id.0.as_u128()).active);
    assert!(run(api.list_keys(&foreign)).unwrap().is_empty());
    let key_actor = run(api.authenticate(&created.api_key)).unwrap();
    assert_eq!(run(api.create_key(&key_actor, new)), Err(ApiError::Forbidden));
    assert_eq!(run(api.revoke_key(&key_actor, second.key.id)), Err(ApiError::Forbidden));
    assert!(!run(api.revoke_key(&owner, second.key.id)).unwrap().active);
    assert_eq!(run(api.authenticate(&second.api_key)), Err(ApiError::Unauthorized(keys::INVALID_KEY)));
}

// The HTTP host spawns these futures on a multi-threaded runtime: each must be `Send`
// (an `AsyncFnOnce(&mut dyn JobUow)` body made them unprovable; p1T, E0277).
#[test]
fn the_host_facing_use_case_futures_are_send() {
    fn send<T: Send>(_: T) {}
    let w = World::new();
    let api = w.api();
    let actor = Actor::Session { account: fixtures::account() };
    send(api.authenticate("sk_x"));
    let request = crate::jobs::app::StartRequest { job_ids: vec![], locale: None, preferences: Default::default() };
    send(api.start(&actor, PublicStart::Jobs(request), None));
    send(api.create_key(&actor, NewKey { name: "ci".into(), scopes: vec![], expiration_policy: ExpirationPolicy::Never, custom_days: None }));
    send(api.list_keys(&actor));
    send(api.revoke_key(&actor, KeyId(Uuid::from_u128(1))));
    send(api.signing_secret(&actor));
}

// D5 (lead 2026-10-27): the account owner's session sees the signing secret on the key screen; it is
// created once and stable; a key actor or another account never reads it.
#[test]
fn d5_only_the_owners_session_reads_the_signing_secret() {
    let w = World::new();
    let api = w.api();
    let owner = Actor::Session { account: fixtures::account() };
    let first = run(api.signing_secret(&owner)).unwrap();
    assert!(!first.is_empty());
    assert_eq!(run(api.signing_secret(&owner)).unwrap(), first, "created once");
    assert_eq!(w.store.with(|d| d.api.secrets[&fixtures::account()].clone()), first, "the secret callbacks are signed with");
    let key = Actor::ApiKey { account: fixtures::account(), key: KeyId(Uuid::from_u128(1)), scopes: both().iter().map(|s| s.to_string()).collect() };
    assert_eq!(run(api.signing_secret(&key)), Err(ApiError::Forbidden));
    let other_owner = Actor::Session { account: other() };
    assert_ne!(run(api.signing_secret(&other_owner)).unwrap(), first, "per account");
}

// ---- callbacks -----------------------------------------------------------------------------

#[derive(Default)]
struct Transport {
    answers: Mutex<VecDeque<SendOutcome>>,
    /// Each request with the attempt numbers already recorded when it was sent.
    sent: Mutex<Vec<(CallbackRequest, Vec<u32>)>>,
    store: Mutex<Option<MemStore>>,
}

impl CallbackTransport for Transport {
    fn post<'a>(&'a self, request: &'a CallbackRequest) -> BoxFuture<'a, SendOutcome> {
        let recorded = self.store.lock().unwrap().as_ref().map(|s| s.with(|d| d.api.attempts.iter().map(|a| a.n).collect())).unwrap_or_default();
        self.sent.lock().unwrap().push((request.clone(), recorded));
        let answer = self.answers.lock().unwrap().pop_front().unwrap_or(SendOutcome::Status { code: 200, excerpt: String::new() });
        Box::pin(async move { answer })
    }
}

struct Content {
    outcome: Mutex<Option<SummaryOutcome>>,
    facts: Mutex<Option<CallbackContentFacts>>,
}

impl CallbackContent for Content {
    fn summary_outcome(&self, _job: JobId) -> BoxFuture<'_, Result<Option<SummaryOutcome>, String>> {
        let o = self.outcome.lock().unwrap().clone();
        Box::pin(async move { Ok(o) })
    }
    fn content(&self, _account: AccountId, _job: JobId) -> BoxFuture<'_, Result<Option<CallbackContentFacts>, String>> {
        let f = self.facts.lock().unwrap().clone();
        Box::pin(async move { Ok(f) })
    }
    fn audio_url<'a>(&'a self, _job: &'a Job) -> BoxFuture<'a, Option<String>> {
        Box::pin(async { Some("https://signed.example.test/audio".into()) })
    }
}

struct Delivery {
    w: World,
    transport: Arc<Transport>,
    content: Arc<Content>,
    callbacks: Callbacks,
}

fn completed_event(callback_url: Option<&str>) -> JobEvent {
    JobEvent::Completed { job: JobId(3), account: fixtures::account(), provider: None, language: None, error: None, callback_url: callback_url.map(str::to_string), at: NOW }
}

fn delivery() -> Delivery {
    let w = World::new();
    let transport = Arc::new(Transport::default());
    *transport.store.lock().unwrap() = Some(w.store.clone());
    let mut job = fixtures::job(3, JobStatus::Completed, "call.mp3");
    job.duration_minutes = Some(2);
    let facts = CallbackContentFacts { job, lines: vec![fixtures::line(1, 3, "00:00:00", "00:00:02", "Hi", None)], summary: Some("Sum".into()) };
    let content = Arc::new(Content { outcome: Mutex::new(Some(SummaryOutcome::Saved { generation: crate::ai::GenerationId(Uuid::from_u128(5)) })), facts: Mutex::new(Some(facts)) });
    let callbacks = Callbacks::new(CallbackServices {
        outbox: Arc::new(w.store.clone()),
        transport: transport.clone(),
        content: content.clone(),
        crypto: w.crypto.clone(),
        clock: w.clock.clone(),
    });
    let mut uow = run(w.store.begin()).unwrap();
    assert!(run(Callbacks::fan_out(uow.as_mut(), 41, &completed_event(Some("https://hooks.example.test/t")), NOW)).unwrap());
    run(uow.commit()).unwrap();
    Delivery { w, transport, content, callbacks }
}

fn stored(d: &Delivery) -> CallbackDelivery {
    d.w.store.with(|x| x.api.deliveries[0].clone())
}

// D5, D6: fan-out is one delivery per completed event with a URL, idempotent on the event id.
#[test]
fn d5_d6_fan_out_is_idempotent_per_completed_event() {
    let d = delivery();
    let mut uow = run(d.w.store.begin()).unwrap();
    assert!(!run(Callbacks::fan_out(uow.as_mut(), 41, &completed_event(Some("https://hooks.example.test/t")), NOW)).unwrap(), "relay retry");
    assert!(!run(Callbacks::fan_out(uow.as_mut(), 42, &completed_event(None), NOW)).unwrap());
    let failed = JobEvent::Failed { job: JobId(3), account: fixtures::account(), language: None, error: None, at: NOW };
    assert!(!run(Callbacks::fan_out(uow.as_mut(), 43, &failed, NOW)).unwrap());
    run(uow.commit()).unwrap();
    assert_eq!(d.w.store.with(|x| x.api.deliveries.len()), 1);
}

// D5, API-015/016, D10: the attempt is recorded before the POST; the body carries the signed audio
// URL while the stored payload does not; the delivery id and the HMAC signature header are sent.
#[test]
fn d5_the_attempt_is_durable_before_the_signed_post() {
    let d = delivery();
    let pass = run(d.callbacks.dispatch_pass(10));
    assert_eq!((pass.claimed, pass.delivered), (1, 1));
    let (request, recorded) = d.transport.sent.lock().unwrap()[0].clone();
    assert_eq!(recorded, vec![1], "attempt 1 existed before the send");
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!((body["job"]["audio_url"].as_str(), body["summary"]["text"].as_str()), (Some("https://signed.example.test/audio"), Some("Sum")));
    let delivery = stored(&d);
    assert_eq!(request.delivery, delivery.id);
    assert_eq!(delivery.payload.as_ref().unwrap()["job"]["audio_url"], serde_json::Value::Null, "D10");
    assert_eq!((delivery.state, delivery.attempts, delivery.delivered_at), (DeliveryState::Delivered, 1, Some(NOW)));
    let secret = d.w.store.with(|x| x.api.secrets[&fixtures::account()].clone());
    let t = NOW.0 / MICROS_PER_SECOND;
    let expected = callbacks::signature(t, &d.w.crypto.hmac_sha256_hex(secret.as_bytes(), &callbacks::signed_message(t, &request.body)));
    assert_eq!(request.signature, expected);
    assert_eq!(run(d.callbacks.dispatch_pass(10)).claimed, 0, "delivered is final");
}

// D5: a retryable answer schedules the next attempt by the schedule; a non-retryable 4xx is final.
#[test]
fn d5_retries_follow_the_schedule_and_4xx_is_final() {
    let d = delivery();
    d.transport.answers.lock().unwrap().extend([SendOutcome::Status { code: 503, excerpt: "busy".into() }, SendOutcome::Status { code: 404, excerpt: String::new() }]);
    assert_eq!(run(d.callbacks.dispatch_pass(10)).retrying, 1);
    assert_eq!((stored(&d).state, stored(&d).next_attempt_at), (DeliveryState::Pending, Micros(NOW.0 + 60 * MICROS_PER_SECOND)));
    assert_eq!(run(d.callbacks.dispatch_pass(10)).claimed, 0, "not due yet");
    d.w.clock.advance_minutes(1);
    assert_eq!(run(d.callbacks.dispatch_pass(10)).failed, 1);
    assert_eq!((stored(&d).state, stored(&d).attempts), (DeliveryState::Failed, 2));
    let attempts = d.w.store.with(|x| x.api.attempts.clone());
    assert_eq!(attempts.iter().map(|a| (a.n, a.outcome.clone())).collect::<Vec<_>>(), vec![
        (1, Some(SendOutcome::Status { code: 503, excerpt: "busy".into() })),
        (2, Some(SendOutcome::Status { code: 404, excerpt: String::new() })),
    ]);
}

// D7: while the summary is undecided nothing is sent and no attempt is counted; after 15 minutes
// the callback goes without it.
#[test]
fn d7_the_summary_wait_is_bounded() {
    let d = delivery();
    *d.content.outcome.lock().unwrap() = None;
    d.content.facts.lock().unwrap().as_mut().unwrap().summary = None;
    let pass = run(d.callbacks.dispatch_pass(10));
    assert_eq!((pass.waiting, d.transport.sent.lock().unwrap().len(), stored(&d).attempts), (1, 0, 0));
    assert_eq!(stored(&d).next_attempt_at, Micros(NOW.0 + callbacks::SUMMARY_RECHECK));
    d.w.clock.advance_minutes(15);
    assert_eq!(run(d.callbacks.dispatch_pass(10)).delivered, 1);
    let body: serde_json::Value = serde_json::from_slice(&d.transport.sent.lock().unwrap()[0].0.body).unwrap();
    assert!(body.get("summary").is_none());
}

// A delivery whose job is gone (erased, hard deleted) is abandoned without a send.
#[test]
fn a_delivery_for_a_missing_job_is_abandoned_unsent() {
    let d = delivery();
    *d.content.facts.lock().unwrap() = None;
    assert_eq!(run(d.callbacks.dispatch_pass(10)).abandoned, 1);
    assert_eq!((stored(&d).state, d.transport.sent.lock().unwrap().len()), (DeliveryState::Abandoned, 0));
}

// D8: an admin replay appends one attempt to the same delivery id with a freshly built body;
// a pending delivery is not replayed, an unknown one is not found, and a replay needs the operator.
#[test]
fn d8_an_admin_replay_appends_one_attempt_to_the_same_delivery() {
    let d = delivery();
    d.transport.answers.lock().unwrap().push_back(SendOutcome::Status { code: 404, excerpt: String::new() });
    run(d.callbacks.dispatch_pass(10));
    let id = stored(&d).id;
    assert_eq!(stored(&d).state, DeliveryState::Failed);
    assert_eq!(run(d.callbacks.replay("", id)), Err(ApiError::Forbidden), "the host's verified operator is required");
    assert_eq!(run(d.callbacks.replay("ops@example.test", DeliveryId(Uuid::from_u128(99)))), Err(ApiError::NotFound));
    d.w.clock.advance_minutes(5);
    run(d.callbacks.replay("ops@example.test", id)).unwrap();
    assert_eq!((stored(&d).state, stored(&d).next_attempt_at), (DeliveryState::Pending, Micros(NOW.0 + 5 * 60 * MICROS_PER_SECOND)));
    assert!(matches!(run(d.callbacks.replay("ops@example.test", id)), Err(ApiError::Jobs(_)) | Err(ApiError::Invalid(_))), "already pending");
    assert_eq!(run(d.callbacks.dispatch_pass(10)).delivered, 1);
    let sent = d.transport.sent.lock().unwrap();
    assert_eq!((sent.len(), sent[1].0.delivery, sent[1].1.clone()), (2, id, vec![1, 2]), "same id, attempt 2 recorded first");
    assert_eq!((stored(&d).state, stored(&d).attempts), (DeliveryState::Delivered, 2));
}
