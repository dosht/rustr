//! P3 closing tests over real HTTP, the real routes and the restricted PostgreSQL role. Every
//! test names the rule it closes. External providers (object signing, video metadata, captions)
//! are scripted fakes at the port; nothing here calls a provider.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

use media_ports::{
    AccessMethod, CaptionCue, CaptionDocument, CaptionError, DeleteOutcome, ObjectMetadata, ObjectRef, ObjectStore, PaidCaptions, PutObject,
    SensitiveUrl, StoreKind, TemporaryAccess, TransportError, VideoMetadata, VideoMetadataSource,
};
use transgate_core::AccountId;
use transgate_core::jobs::app::{Jobs, JobsConfig, JobsServices};
use transgate_core::jobs::domain::{JobSource, JobStatus, NewJob, StorageProvider};
use transgate_core::jobs::ports::{BoxFuture, JobStore};
use transgate_core::jobs::routing::{ModelConfig, RoutingConfig};
use transgate_core::{Language, Micros as CoreMicros};
use transgate_http::TransgateHttp;
use transgate_jobs_postgres::PgJobStore;

use super::*;

/// The upload bucket: signs a URL, or fails when the test says the signing service is down.
struct Signing {
    down: AtomicBool,
}
impl ObjectStore for Signing {
    fn kind(&self) -> StoreKind {
        StoreKind::Gcs
    }
    fn bucket(&self) -> &str {
        "uploads"
    }
    fn sign<'a>(&'a self, object: &'a ObjectRef, method: AccessMethod, now: media_ports::Micros, ttl: u32) -> BoxFuture<'a, Result<TemporaryAccess, TransportError>> {
        Box::pin(async move {
            if self.down.load(Ordering::SeqCst) {
                return Err(TransportError::Unavailable { status: Some(503), operation: "sign".into() });
            }
            Ok(TemporaryAccess {
                object: object.clone(),
                method,
                url: SensitiveUrl::new(format!("https://storage.example.test/{}", object.key)),
                expires_at: media_ports::Micros(now.0 + i64::from(ttl) * 1_000_000),
                headers: vec![],
            })
        })
    }
    fn head<'a>(&'a self, _: &'a ObjectRef) -> BoxFuture<'a, Result<Option<ObjectMetadata>, TransportError>> {
        Box::pin(async { Ok(None) })
    }
    fn put(&self, _: PutObject) -> BoxFuture<'_, Result<ObjectMetadata, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("not used".into())) })
    }
    fn delete<'a>(&'a self, _: &'a ObjectRef) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async { Ok(DeleteOutcome::Absent) })
    }
}

/// Video metadata answers, consumed in order; once empty, a normal video.
#[derive(Default)]
struct Video(Mutex<VecDeque<Result<VideoMetadata, CaptionError>>>);
impl Video {
    fn then(&self, answer: Result<VideoMetadata, CaptionError>) {
        self.0.lock().unwrap().push_back(answer);
    }
}
fn video(id: &str, seconds: f64) -> VideoMetadata {
    VideoMetadata { video_id: id.into(), title: format!("Video {id}"), duration_seconds: Some(seconds), language: Some("en".into()), raw: json!({}) }
}
impl VideoMetadataSource for Video {
    fn fetch<'a>(&'a self, video_id: &'a str) -> BoxFuture<'a, Result<VideoMetadata, CaptionError>> {
        let next = self.0.lock().unwrap().pop_front();
        let id = video_id.to_string();
        Box::pin(async move { next.unwrap_or_else(|| Ok(video(&id, 120.0))) })
    }
}

/// Paid captions: two cues, always.
struct Captions;
impl PaidCaptions for Captions {
    fn fetch<'a>(&'a self, video_id: &'a str) -> BoxFuture<'a, Result<CaptionDocument, CaptionError>> {
        let video_id = video_id.to_string();
        Box::pin(async move {
            Ok(CaptionDocument {
                video_id,
                language: Some("en".into()),
                cues: vec![
                    CaptionCue { start: 0.0, duration: 2.5, text: "Hello there".into() },
                    CaptionCue { start: 2.5, duration: 2.5, text: "General Kenobi".into() },
                ],
                metadata: json!({}),
            })
        })
    }
}

struct World {
    app: App,
    store: PgJobStore,
    signing: Arc<Signing>,
    video: Arc<Video>,
    soniox: Arc<Scripted>,
    deepgram: Arc<Scripted>,
}

type Answer = Result<media_ports::Submission, TransportError>;

/// A provider that answers each submission from a script and counts how often it was asked.
struct Scripted {
    provider: media_ports::Provider,
    answers: Mutex<VecDeque<Answer>>,
    asked: std::sync::atomic::AtomicUsize,
}
impl Scripted {
    fn new(provider: media_ports::Provider) -> Scripted {
        Scripted { provider, answers: Mutex::new(VecDeque::new()), asked: Default::default() }
    }
    /// Replaces the script and the count.
    fn script(&self, answers: Vec<Answer>) {
        *self.answers.lock().unwrap() = answers.into();
        self.asked.store(0, Ordering::SeqCst);
    }
    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}
impl media_ports::Transcriber for Scripted {
    fn provider(&self) -> media_ports::Provider {
        self.provider
    }
    fn project(&self) -> &str {
        "project"
    }
    fn submit<'a>(&'a self, _: &'a media_ports::TranscriptionRequest) -> BoxFuture<'a, Result<media_ports::Submission, TransportError>> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let next = self.answers.lock().unwrap().pop_front().unwrap_or(Err(TransportError::Unavailable { status: None, operation: "unscripted".into() }));
        Box::pin(async move { next })
    }
    fn status<'a>(&'a self, _: &'a media_ports::RemoteIdentity) -> BoxFuture<'a, Result<media_ports::ProviderObservation, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("status".into())) })
    }
    fn result<'a>(&'a self, _: &'a media_ports::RemoteIdentity) -> BoxFuture<'a, Result<media_ports::ProviderObservation, TransportError>> {
        Box::pin(async { Err(TransportError::Unsupported("result".into())) })
    }
    fn delete_remote<'a>(&'a self, _: &'a media_ports::RemoteIdentity) -> BoxFuture<'a, Result<DeleteOutcome, TransportError>> {
        Box::pin(async { Ok(DeleteOutcome::Absent) })
    }
    fn find_by_reference<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Vec<media_ports::RemoteRecord>, TransportError>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn verify_callback(&self, _: &media_ports::CallbackBinding, _: &media_ports::Headers, _: &[u8]) -> Result<media_ports::ProviderObservation, media_ports::CallbackError> {
        Err(media_ports::CallbackError::Malformed)
    }
    fn complete_callback(&self, observation: media_ports::ProviderObservation) -> BoxFuture<'_, Result<media_ports::ProviderObservation, TransportError>> {
        Box::pin(async move { Ok(observation) })
    }
}

async fn world() -> World {
    let clock: Arc<dyn Clock> = Arc::new(transgate_billing::SystemClock);
    let mut app = start_in(clock.clone(), runtime_fixture().await).await;
    transgate_jobs_postgres::migrate(&app.owner).await.unwrap();
    transgate_jobs_postgres::verify_runtime(&app.transgate.pool).await.unwrap();
    let pool = app.transgate.pool.clone();
    let store = PgJobStore::new(pool.clone());
    let limiter: Arc<dyn transgate_core::jobs::ports::RateLimiter> = Arc::new(transgate_jobs_postgres::PgRateLimiter::new(pool.clone()));
    let signing = Arc::new(Signing { down: AtomicBool::new(false) });
    let video = Arc::new(Video::default());
    let soniox = Arc::new(Scripted::new(media_ports::Provider::Soniox));
    let deepgram = Arc::new(Scripted::new(media_ports::Provider::Deepgram));
    let jobs = Arc::new(Jobs::new(
        JobsServices {
            store: Arc::new(store.clone()),
            clock: clock.clone(),
            objects: vec![signing.clone()],
            transcribers: vec![soniox.clone(), deepgram.clone()],
            stager: None,
            sleeper: Arc::new(super::erasure::RuntimeSleeper),
            rate_limiter: limiter.clone(),
            paid_captions: Some(Arc::new(Captions)),
            video_metadata: Some(video.clone()),
        },
        JobsConfig {
            routing: RoutingConfig { soniox_max_duration_minutes: 200 },
            models: ModelConfig { soniox: "m".into(), soniox_v5: "m".into(), deepgram: "m".into(), deepgram_account_overrides: vec![] },
            upload_storage: StorageProvider::Gcs,
            upload_bucket: "uploads".into(),
            archive_store: StoreKind::Gcs,
            archive_bucket: "archive".into(),
            staging_store: StoreKind::Gcs,
            staging_bucket: "staging".into(),
            callback_base_url: "https://transgate.example.test".into(),
            soniox_auto_delete: true,
            ssrf_protection: true,
            freemium_lists: transgate_billing::catalog::freemium_lists(),
        },
    ));
    let privacy = Arc::new(transgate_core::privacy::app::Privacy::new(transgate_core::privacy::app::PrivacyServices {
        store: Arc::new(store.clone()),
        clock: clock.clone(),
        locks: Arc::new(transgate_jobs_postgres::PgLocks::new(pool.clone())),
        objects: vec![],
        inventories: vec![],
        transcribers: vec![],
        identity: Arc::new(super::erasure::IdentityDeletion::default()),
        billing: Arc::new(billing_postgres::PgBillingStore::new(pool.clone())),
        terminators: vec![],
        exceptions: transgate_core::privacy::erasure::ExceptionPolicy::default(),
    }));
    let state = Arc::new(TransgateHttp {
        ai: None,
        jobs,
        privacy,
        public_transcript: Arc::new(transgate_core::public_transcript::PublicTranscript::new(Arc::new(super::erasure::NoPublicCaptions), limiter, clock.clone())),
        queries: Arc::new(store.clone()),
        transcribers: vec![],
        identity: app.transgate.identity.clone(),
        clock,
        cron_secret: None,
        downloads: None,
        api: None,
        callbacks: None,
    });
    let server = serve(app.transgate.router.clone().merge(transgate_http::router(state))).await;
    app.base = server.url.clone();
    app._servers.push(server);
    World { app, store, signing, video, soniox, deepgram }
}

/// A signed-in account created now, so its free package is open.
async fn signed_in(app: &App) -> User {
    let person = user();
    assert_eq!(app.signup(&person, now_micros()).await.0, 200);
    assert_eq!(app.call("GET", "/api/billing/overview", Some(&person.jwt), None).await.0, 200);
    person
}

#[derive(Clone, Default)]
struct Draft {
    duration: Option<i32>,
    size: Option<i64>,
    language: Option<&'static str>,
    translate_to: Option<&'static str>,
}

async fn draft(store: &PgJobStore, person: &User, spec: Draft) -> i64 {
    let mut uow = store.begin().await.unwrap();
    let job = uow
        .insert_job(&NewJob {
            account: AccountId(person.id),
            status: JobStatus::Draft,
            name: Some("talk.mp3".into()),
            source: JobSource::Upload { storage: StorageProvider::Gcs, object: None },
            language: spec.language.and_then(Language::from_name),
            translate_to: spec.translate_to.and_then(Language::from_name).into_iter().collect(),
            speakers: false,
            fix_grammer: None,
            duration_minutes: spec.duration,
            size_bytes: spec.size,
            callback_url: None,
            whisper_params: None,
            created_at: CoreMicros(transgate_billing::SystemClock.now().0),
        })
        .await
        .unwrap();
    uow.commit().await.unwrap();
    job.id.0
}

async fn post(app: &App, path: &str, person: Option<&User>, body: Value) -> (u16, Value) {
    app.call("POST", path, person.map(|p| p.jwt.as_str()), Some(body)).await
}

// UPL-009: init-upload answers 401 without a session, 400 for wrong types, 201 with the signed
// URL as the body, 500 on a signing error.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn upl_009_init_upload_answers_401_400_201_and_500() {
    let w = world().await;
    let person = signed_in(&w.app).await;
    let job = draft(&w.store, &person, Draft::default()).await;

    assert_eq!(post(&w.app, "/api/init-upload", None, json!({"jobId": job, "fileName": "a.mp3"})).await.0, 401, "no session");
    for body in [json!({"jobId": "7", "fileName": "a.mp3"}), json!({"jobId": job, "fileName": 5}), json!({"jobId": job}), json!({})] {
        assert_eq!(post(&w.app, "/api/init-upload", Some(&person), body.clone()).await.0, 400, "{body}");
    }
    let (status, url) = post(&w.app, "/api/init-upload", Some(&person), json!({"jobId": job, "fileName": "a.mp3"})).await;
    assert_eq!(status, 201);
    assert!(url.as_str().is_some_and(|u| u.starts_with("https://storage.example.test/") && u.contains(&person.id.to_string())), "the signed URL is the body: {url}");

    w.signing.down.store(true, Ordering::SeqCst);
    let (status, body) = post(&w.app, "/api/init-upload", Some(&person), json!({"jobId": job, "fileName": "a.mp3"})).await;
    assert_eq!(status, 500, "a signing failure is a 500: {body}");
}

fn youtube_url(n: u32) -> String {
    format!("https://www.youtube.com/watch?v=vid{n:08}")
}

async fn block(w: &World, person: &User) {
    sqlx::query("INSERT INTO transgate_jobs.settings (account_id, blocked) VALUES ($1, true) ON CONFLICT (account_id) DO UPDATE SET blocked = true")
        .bind(person.id)
        .execute(&w.app.owner)
        .await
        .unwrap();
}

// UPL-023: YouTube add needs a session (401), refuses a blocked user (403), allows 10 requests per
// user per minute (429 after), a bad URL is 400, a video that is gone or private is 404 and a
// metadata API failure is 502.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn upl_023_youtube_add_answers_401_403_400_404_502_and_the_eleventh_request_in_a_minute_is_429() {
    async fn add(app: &App, n: u32, person: Option<&User>) -> (u16, Value) {
        post(app, "/api/youtube/add", person, json!({"youtubeUrl": youtube_url(n)})).await
    }
    let w = world().await;
    let person = signed_in(&w.app).await;
    assert_eq!(add(&w.app, 1, None).await.0, 401, "a refused session spends nothing of the limit");
    assert_eq!(post(&w.app, "/api/youtube/add", Some(&person), json!({"youtubeUrl": "https://example.test/nope"})).await.0, 400);
    w.video.then(Err(CaptionError::VideoUnavailable));
    assert_eq!(add(&w.app, 2, Some(&person)).await.0, 404, "a private or missing video");
    w.video.then(Err(CaptionError::Network));
    assert_eq!(add(&w.app, 3, Some(&person)).await.0, 502, "the metadata API failed");
    let (status, body) = add(&w.app, 4, Some(&person)).await;
    assert_eq!((status, body["job"]["status"].clone()), (200, json!("draft")), "{body}");
    // Four requests so far (the 401 never reached the limiter): six more fit the ten per minute.
    for n in 5..11 {
        assert_eq!(add(&w.app, n, Some(&person)).await.0, 200, "request {n} is inside the limit");
    }
    assert_eq!(add(&w.app, 11, Some(&person)).await.0, 429, "the eleventh request in the minute");

    // A blocked user is refused before the limiter, with 403.
    let blocked = signed_in(&w.app).await;
    block(&w, &blocked).await;
    let (status, body) = add(&w.app, 1, Some(&blocked)).await;
    assert_eq!(status, 403, "{body}");
}

// UPL-026: YouTube resolve allows 5 requests per user per minute (the sixth is 429); only the
// owner's draft resolves (another account sees 404, a resolved job is 409); the captions become
// lines without a speaker and the job completes with its transcript source, never billed.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn upl_026_youtube_resolve_is_limited_to_five_per_minute_owner_and_draft_only() {
    let w = world().await;
    let person = signed_in(&w.app).await;
    let stranger = signed_in(&w.app).await;
    let mut jobs = Vec::new();
    for n in 1..=5 {
        let (status, body) = post(&w.app, "/api/youtube/add", Some(&person), json!({"youtubeUrl": youtube_url(n)})).await;
        assert_eq!(status, 200, "{body}");
        jobs.push(body["job"]["id"].as_i64().unwrap());
    }
    let extra = post(&w.app, "/api/youtube/add", Some(&person), json!({"youtubeUrl": youtube_url(6)})).await.1["job"]["id"].as_i64().unwrap();
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM billing.usage WHERE account_id = $1").bind(person.id).fetch_one(&w.app.owner).await.unwrap();

    assert_eq!(post(&w.app, "/api/youtube/resolve", Some(&stranger), json!({"jobId": jobs[0]})).await.0, 404, "another account's draft");
    let (status, body) = post(&w.app, "/api/youtube/resolve", Some(&person), json!({"jobId": jobs[0]})).await;
    assert_eq!((status, body["status"].clone()), (200, json!("completed")), "{body}");
    assert_eq!(post(&w.app, "/api/youtube/resolve", Some(&person), json!({"jobId": jobs[0]})).await.0, 409, "no longer a draft");
    for job in &jobs[1..4] {
        assert_eq!(post(&w.app, "/api/youtube/resolve", Some(&person), json!({"jobId": job})).await.0, 200);
    }
    assert_eq!(post(&w.app, "/api/youtube/resolve", Some(&person), json!({"jobId": extra})).await.0, 429, "the sixth request in the minute");

    let lines: Vec<(String, Option<String>)> = sqlx::query_as("SELECT line, speaker FROM transgate_jobs.lines WHERE job_id = $1 ORDER BY start_time")
        .bind(jobs[0])
        .fetch_all(&w.app.owner)
        .await
        .unwrap();
    assert_eq!(lines, vec![("Hello there".to_string(), None), ("General Kenobi".to_string(), None)], "lines are saved without a speaker");
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM billing.usage WHERE account_id = $1").bind(person.id).fetch_one(&w.app.owner).await.unwrap();
    assert_eq!(after, before, "a YouTube resolve is never billed");
}

// PRV-051: a blocked account is refused by YouTube add and resolve with 403, and by nothing else.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn prv_051_a_blocked_account_is_refused_by_youtube_add_and_resolve_only() {
    let w = world().await;
    let person = signed_in(&w.app).await;
    let (status, body) = post(&w.app, "/api/youtube/add", Some(&person), json!({"youtubeUrl": youtube_url(1)})).await;
    assert_eq!(status, 200, "{body}");
    let job = body["job"]["id"].as_i64().unwrap();
    block(&w, &person).await;

    for (path, request) in [("/api/youtube/add", json!({"youtubeUrl": youtube_url(2)})), ("/api/youtube/resolve", json!({"jobId": job}))] {
        let (status, body) = post(&w.app, path, Some(&person), request).await;
        assert_eq!(status, 403, "{path}: {body}");
        assert!(body.to_string().to_lowercase().contains("suspended"), "{path}: {body}");
    }
    assert_eq!(w.app.call("GET", "/api/jobs", Some(&person.jwt), None).await.0, 200, "nothing else checks the flag");
    assert_eq!(w.app.call("GET", "/api/settings", Some(&person.jwt), None).await.0, 200);
    assert_eq!(post(&w.app, "/api/youtube/metadata", Some(&person), json!({"videoId": youtube_url(3)})).await.0, 200, "metadata does not check the flag either");
}

// UPL-021: the offered languages are the 61 supported ones; the account's most used languages
// and translation targets come first, ordered by use, and only this account's jobs count.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn upl_021_language_usage_orders_this_accounts_languages_and_targets_by_use() {
    let w = world().await;
    let person = signed_in(&w.app).await;
    let other = signed_in(&w.app).await;
    assert_eq!(Language::all().count(), 61, "the supported language list");
    for (language, target, times) in [("English", Some("French"), 2), ("Turkish", Some("Spanish"), 1), ("German", Some("French"), 1), ("Turkish", None, 2)] {
        for _ in 0..times {
            draft(&w.store, &person, Draft { language: Some(language), translate_to: target, ..Draft::default() }).await;
        }
    }
    for _ in 0..9 {
        draft(&w.store, &other, Draft { language: Some("Japanese"), translate_to: Some("Korean"), ..Draft::default() }).await;
    }
    let (status, usage) = w.app.call("GET", "/api/languages/usage", Some(&person.jwt), None).await;
    assert_eq!(status, 200);
    let rows = |key: &str| usage[key].as_array().unwrap().iter().map(|r| (r["language"].as_str().unwrap().to_string(), r["count"].as_u64().unwrap())).collect::<Vec<_>>();
    assert_eq!(rows("languages"), vec![("Turkish".into(), 3), ("English".into(), 2), ("German".into(), 1)]);
    assert_eq!(rows("targets"), vec![("French".into(), 3), ("Spanish".into(), 1)]);
}

// UPL-005: the host sets no product size or duration refusal on a draft or its upload.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn upl_005_a_very_large_and_very_long_upload_is_accepted_without_a_product_limit() {
    let w = world().await;
    let person = signed_in(&w.app).await;
    let job = draft(&w.store, &person, Draft { size: Some(40 * 1024 * 1024 * 1024), duration: Some(24 * 60), language: Some("English"), ..Draft::default() }).await;
    let (status, url) = post(&w.app, "/api/init-upload", Some(&person), json!({"jobId": job, "fileName": "ten-hours.wav"})).await;
    assert_eq!(status, 201, "{url}");
    let (size, minutes): (i64, i32) = sqlx::query_as("SELECT size_bytes, duration_minutes FROM transgate_jobs.jobs WHERE id = $1").bind(job).fetch_one(&w.app.owner).await.unwrap();
    assert_eq!((size, minutes), (40 * 1024 * 1024 * 1024, 24 * 60), "nothing clamped the size or the duration");
}

async fn overdraw(w: &World, person: &User) {
    sqlx::query("INSERT INTO billing.usage (account_id, package_id, subject_ref, minutes, used_at) SELECT $1, id, 999999, minutes_included + 5, now() FROM billing.packages WHERE account_id = $1")
        .bind(person.id)
        .execute(&w.app.owner)
        .await
        .unwrap();
}

async fn statuses(w: &World, jobs: &[i64]) -> Vec<String> {
    let mut out = Vec::new();
    for job in jobs {
        out.push(sqlx::query_scalar("SELECT status FROM transgate_jobs.jobs WHERE id = $1").bind(job).fetch_one(&w.app.owner).await.unwrap());
    }
    out
}

// JOB-006, LNG-029: a freemium start over 20 minutes is allowed up to 45 only when the UI locale
// or the language (or first translation target) of the lowest-id draft is on the list; a balance
// below zero refuses the whole batch; either way a refusal moves nothing and a start is charged once.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn job_006_lng_029_a_start_batch_mixing_an_exempt_and_a_normal_draft_follows_the_lowest_id_draft() {
    let w = world().await;
    let usage = |person: &User| {
        let pool = w.app.owner.clone();
        let id = person.id;
        async move { sqlx::query_scalar::<_, i64>("SELECT COALESCE(sum(minutes), 0)::bigint FROM billing.usage WHERE account_id = $1").bind(id).fetch_one(&pool).await.unwrap() }
    };

    // The lowest-id draft is Turkish (exempt): 30 minutes in total is allowed and charged once.
    let person = signed_in(&w.app).await;
    let (turkish, english) = (
        draft(&w.store, &person, Draft { duration: Some(15), language: Some("Turkish"), ..Draft::default() }).await,
        draft(&w.store, &person, Draft { duration: Some(15), language: Some("English"), ..Draft::default() }).await,
    );
    let (status, body) = post(&w.app, "/api/jobs/start", Some(&person), json!({"jobIds": [english, turkish], "locale": "en"})).await;
    assert_eq!(status, 201, "the request order is irrelevant, the lowest id decides: {body}");
    assert!(statuses(&w, &[turkish, english]).await.iter().all(|s| s != "draft"), "both started");
    let charged = usage(&person).await;
    assert!(charged > 0 && charged <= 30, "one charge for the batch: {charged}");

    // The lowest-id draft is English: no exception, so the same 30 minutes are refused and nothing moves.
    let person = signed_in(&w.app).await;
    let (english, turkish) = (
        draft(&w.store, &person, Draft { duration: Some(15), language: Some("English"), ..Draft::default() }).await,
        draft(&w.store, &person, Draft { duration: Some(15), language: Some("Turkish"), ..Draft::default() }).await,
    );
    let (status, body) = post(&w.app, "/api/jobs/start", Some(&person), json!({"jobIds": [english, turkish], "locale": "en"})).await;
    assert_eq!(status, 402, "{body}");
    assert!(body.to_string().contains("Not enough balance"), "{body}");
    assert_eq!(statuses(&w, &[english, turkish]).await, vec!["draft", "draft"], "a refusal moves nothing");
    assert_eq!(usage(&person).await, 0, "and charges nothing");
    // A listed UI locale makes the same batch allowed; the 45 minute ceiling still holds.
    let (status, body) = post(&w.app, "/api/jobs/start", Some(&person), json!({"jobIds": [english, turkish], "locale": "tr"})).await;
    assert_eq!(status, 201, "{body}");
    let person = signed_in(&w.app).await;
    let long = draft(&w.store, &person, Draft { duration: Some(46), language: Some("Turkish"), ..Draft::default() }).await;
    assert_eq!(post(&w.app, "/api/jobs/start", Some(&person), json!({"jobIds": [long], "locale": "tr"})).await.0, 402, "over 45 minutes is refused even with an exception");
    // Twenty minutes or less is never blocked.
    let short = draft(&w.store, &person, Draft { duration: Some(20), language: Some("English"), ..Draft::default() }).await;
    assert_eq!(post(&w.app, "/api/jobs/start", Some(&person), json!({"jobIds": [short], "locale": "en"})).await.0, 201);

    // JOB-006: a balance below zero refuses the whole batch, exempt language or not.
    let person = signed_in(&w.app).await;
    let (turkish, english) = (
        draft(&w.store, &person, Draft { duration: Some(5), language: Some("Turkish"), ..Draft::default() }).await,
        draft(&w.store, &person, Draft { duration: Some(5), language: Some("English"), ..Draft::default() }).await,
    );
    overdraw(&w, &person).await;
    let before = usage(&person).await;
    let (status, body) = post(&w.app, "/api/jobs/start", Some(&person), json!({"jobIds": [turkish, english], "locale": "tr"})).await;
    assert_eq!(status, 402, "{body}");
    assert_eq!(statuses(&w, &[turkish, english]).await, vec!["draft", "draft"]);
    assert_eq!(usage(&person).await, before, "nothing was charged");
}

fn accepted(provider: media_ports::Provider, id: &str) -> Answer {
    Ok(media_ports::Submission::Accepted { remote: media_ports::RemoteIdentity { provider, project: "project".into(), id: id.into() } })
}
fn rejected(status: u16) -> Answer {
    Ok(media_ports::Submission::Rejected { status: Some(status), code: "refused".into() })
}

// TRX-014: after a submit that was definitely refused (rate limited or failed) the job fails over to the
// next provider in the chain that can take it, unless the request turns fallback off or a translation
// job has no translation-capable provider left; an answer that may have been accepted (uncertain) never
// fails over, so one job is never submitted to two providers. Run through the real route and database.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn trx_014_a_definite_refusal_fails_over_and_an_uncertain_answer_never_does() {
    use media_ports::{Provider, Submission};
    let w = world().await;
    struct Case {
        name: &'static str,
        soniox: Vec<Answer>,
        deepgram: Vec<Answer>,
        options: Value,
        translate_to: Option<&'static str>,
        status: &'static str,
        provider: Option<&'static str>,
        asked: (usize, usize),
    }
    let cases = vec![
        Case { name: "rate limited", soniox: vec![rejected(429)], deepgram: vec![accepted(Provider::Deepgram, "dg-1")], options: json!({}), translate_to: None, status: "running", provider: Some("deepgram"), asked: (1, 1) },
        Case { name: "failed (a definite refusal)", soniox: vec![rejected(400)], deepgram: vec![accepted(Provider::Deepgram, "dg-2")], options: json!({}), translate_to: None, status: "running", provider: Some("deepgram"), asked: (1, 1) },
        Case { name: "uncertain answer", soniox: vec![Ok(Submission::Uncertain { status: None, operation: "submit".into() })], deepgram: vec![accepted(Provider::Deepgram, "dg-3")], options: json!({}), translate_to: None, status: "not_started", provider: None, asked: (1, 0) },
        Case { name: "fallback turned off", soniox: vec![rejected(429)], deepgram: vec![accepted(Provider::Deepgram, "dg-4")], options: json!({"fallbackEnabled": false}), translate_to: None, status: "pending", provider: None, asked: (1, 0) },
        Case { name: "every provider refuses", soniox: vec![rejected(429)], deepgram: vec![rejected(400)], options: json!({}), translate_to: None, status: "failed", provider: None, asked: (1, 1) },
        Case { name: "translation: Deepgram cannot translate", soniox: vec![rejected(429)], deepgram: vec![accepted(Provider::Deepgram, "dg-6")], options: json!({}), translate_to: Some("German"), status: "pending", provider: None, asked: (1, 0) },
    ];
    for case in cases {
        let person = signed_in(&w.app).await;
        let job = draft(&w.store, &person, Draft { duration: Some(5), language: Some("English"), translate_to: case.translate_to, ..Draft::default() }).await;
        w.soniox.script(case.soniox);
        w.deepgram.script(case.deepgram);
        let (status, body) = post(&w.app, "/api/jobs/start", Some(&person), json!({"jobIds": [job], "locale": "en", "options": case.options})).await;
        assert_eq!(status, 201, "{}: {body}", case.name);
        assert_eq!(body["data"]["results"][0]["status"], case.status, "{}: {body}", case.name);
        let stored: (String, Option<String>) =
            sqlx::query_as("SELECT status, provider FROM transgate_jobs.jobs WHERE id = $1").bind(job).fetch_one(&w.app.owner).await.unwrap();
        assert_eq!(stored.0, case.status, "{}", case.name);
        if let Some(provider) = case.provider {
            assert_eq!(stored.1.as_deref(), Some(provider), "{}", case.name);
        }
        assert_eq!((w.soniox.asked(), w.deepgram.asked()), case.asked, "{}: how many submissions each provider saw", case.name);
    }
}
