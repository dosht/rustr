//! A loopback Transgate host for the actual-process tests: GoTrue (`/auth/v1`) and the
//! API (`/api`) on one origin, backed by in-memory state a test can read afterwards.
//! The product host (owned runtime) is a separate, Y-owned run of the same binary.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde_json::{Value, json};
use transgate_core::jobs::app::JobDetail;
use transgate_core::jobs::domain::{Job, JobStatus};
use transgate_core::languages::Language;
use transgate_core::presentation::fixtures::{self, session_profile};
use transgate_core::transcript::{Line, TranslationStatus};
use transgate_native::Vault;

pub const EMAIL: &str = "ada@example.test";
pub const GOOD_CODE: &str = "good-code";

pub struct State_ {
    pub jobs: Vec<Job>,
    pub lines: HashMap<i64, Vec<Line>>,
    pub next_id: i64,
    pub puts: Vec<(String, usize, Option<String>)>,
    pub started: Vec<Vec<i64>>,
    pub deleted: Vec<i64>,
    pub exports: Vec<Value>,
    pub options: Vec<Value>,
    pub insufficient: bool,
    pub requests: Vec<String>,
}

pub struct Host {
    pub origin: String,
    pub state: Arc<Mutex<State_>>,
    _runtime: tokio::runtime::Runtime,
}

type Shared = Arc<Mutex<State_>>;

fn lines_of(job: i64) -> Vec<Line> {
    vec![
        fixtures::line(job * 10 + 1, job, "00:00:00", "00:00:02", " Hello there ", Some("SPEAKER_00")),
        fixtures::line(job * 10 + 2, job, "00:00:02", "00:00:04", "Hi, \"Ada\"", Some("Ada")),
    ]
}

/// Job 1: completed. Job 2: still running. Job 3: completed translation to Turkish.
fn seed() -> (Vec<Job>, HashMap<i64, Vec<Line>>) {
    let one = fixtures::job(1, JobStatus::Completed, "talk.mp3");
    let two = fixtures::job(2, JobStatus::Running, "later.mp3");
    let mut three = fixtures::job(3, JobStatus::Completed, "ceviri.mp4");
    three.translate_to = vec![Language::from_name("Turkish").unwrap()];
    let mut lines = HashMap::new();
    lines.insert(1, lines_of(1));
    lines.insert(2, vec![]);
    let mut originals = lines_of(3);
    for l in &mut originals {
        l.language = Language::from_name("English");
        l.translation_status = Some(TranslationStatus::Original);
    }
    let mut turkish = vec![fixtures::line(39, 3, "00:00:00", "00:00:02", "Merhaba", Some("SPEAKER_00"))];
    for l in &mut turkish {
        l.language = Language::from_name("Turkish");
        l.translation_status = Some(TranslationStatus::Translation);
    }
    originals.extend(turkish);
    lines.insert(3, originals);
    (vec![three, two, one], lines)
}

fn session_json(expires_in: i64) -> Value {
    json!({"access_token": "tok", "refresh_token": "ref", "expires_in": expires_in, "user": {"id": fixtures::account().0.to_string(), "email": EMAIL}})
}

async fn token(Query(query): Query<HashMap<String, String>>, Json(body): Json<Value>) -> Response {
    let rejected = |code: &str| (StatusCode::BAD_REQUEST, Json(json!({"error_code": code, "msg": code}))).into_response();
    match query.get("grant_type").map(String::as_str) {
        Some("password") if body["password"] == "correct" && body["email"] == EMAIL => Json(session_json(3600)).into_response(),
        Some("password") => rejected("invalid_credentials"),
        Some("pkce") if body["auth_code"] == GOOD_CODE && body["code_verifier"].as_str().is_some_and(|v| !v.is_empty()) => Json(session_json(3600)).into_response(),
        Some("pkce") => rejected("flow_state_not_found"),
        Some("refresh_token") => Json(session_json(3600)).into_response(),
        _ => rejected("validation_failed"),
    }
}

fn job_json(job: &Job) -> Value {
    serde_json::to_value(job).unwrap()
}

async fn list(State(state): State<Shared>) -> Json<Value> {
    let s = state.lock().unwrap();
    let jobs: Vec<Value> = s.jobs.iter().filter(|j| j.status != JobStatus::Draft).map(job_json).collect();
    Json(json!({"jobs": jobs, "next_cursor": null}))
}

async fn drafts(State(state): State<Shared>) -> Json<Value> {
    let s = state.lock().unwrap();
    Json(Value::Array(s.jobs.iter().filter(|j| j.status == JobStatus::Draft).map(job_json).collect()))
}

async fn create_draft(State(state): State<Shared>, Json(body): Json<Value>) -> Response {
    let mut s = state.lock().unwrap();
    s.next_id += 1;
    let mut job = fixtures::job(s.next_id, JobStatus::Draft, body["name"].as_str().unwrap_or("upload.mp3"));
    job.speakers = body["speakers"].as_bool().unwrap_or(false);
    s.jobs.push(job.clone());
    (StatusCode::CREATED, Json(job_json(&job))).into_response()
}

async fn options(State(state): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let mut s = state.lock().unwrap();
    s.options.push(body);
    Json(json!([]))
}

async fn start(State(state): State<Shared>, Json(body): Json<Value>) -> Response {
    let mut s = state.lock().unwrap();
    if s.insufficient {
        return (StatusCode::PAYMENT_REQUIRED, Json(json!({"error": "Insufficient balance", "code": "insufficient_balance"}))).into_response();
    }
    let ids: Vec<i64> = body["jobIds"].as_array().unwrap().iter().filter_map(Value::as_i64).collect();
    for job in &mut s.jobs {
        if ids.contains(&job.id.0) {
            job.status = JobStatus::Running;
        }
    }
    for id in &ids {
        s.lines.entry(*id).or_default();
    }
    s.started.push(ids);
    (StatusCode::CREATED, Json(json!({"started": true}))).into_response()
}

async fn detail(State(state): State<Shared>, UrlPath(id): UrlPath<i64>) -> Response {
    let s = state.lock().unwrap();
    match s.jobs.iter().find(|j| j.id.0 == id) {
        Some(job) => Json(JobDetail { job: job.clone(), lines: s.lines.get(&id).cloned().unwrap_or_default() }).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn lines(State(state): State<Shared>, UrlPath(id): UrlPath<i64>) -> Json<Vec<Line>> {
    Json(state.lock().unwrap().lines.get(&id).cloned().unwrap_or_default())
}

async fn remove(State(state): State<Shared>, UrlPath(id): UrlPath<i64>) -> StatusCode {
    let mut s = state.lock().unwrap();
    s.jobs.retain(|j| j.id.0 != id);
    s.deleted.push(id);
    StatusCode::NO_CONTENT
}

async fn export(State(state): State<Shared>, UrlPath(id): UrlPath<i64>, Json(body): Json<Value>) -> Response {
    state.lock().unwrap().exports.push(body);
    ([(header::CONTENT_TYPE, "application/zip"), (header::CONTENT_DISPOSITION, "attachment; filename=\"Job Name_transcript.zip\"")], format!("PK-job-{id}").into_bytes()).into_response()
}

async fn init_upload(headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("127.0.0.1");
    let name = body["fileName"].as_str().unwrap_or("file").replace('/', "_");
    (StatusCode::CREATED, Json(format!("http://{host}/put/{}-{name}", body["jobId"]))).into_response()
}

async fn put_file(State(state): State<Shared>, UrlPath(name): UrlPath<String>, headers: HeaderMap, body: axum::body::Bytes) -> StatusCode {
    let kind = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_string);
    state.lock().unwrap().puts.push((name, body.len(), kind));
    StatusCode::OK
}

async fn youtube_add(State(state): State<Shared>, Json(body): Json<Value>) -> Response {
    let mut s = state.lock().unwrap();
    s.next_id += 1;
    let job = fixtures::job(s.next_id, JobStatus::Draft, "A talk");
    s.jobs.push(job.clone());
    let video = body["youtubeUrl"].as_str().unwrap_or("").rsplit(['=', '/']).next().unwrap_or("x").to_string();
    (StatusCode::CREATED, Json(json!({"job": job_json(&job), "metadata": {"video_id": video, "title": "A talk", "duration_seconds": 600.0, "language": "en", "raw": {}}}))).into_response()
}

async fn youtube_resolve(State(state): State<Shared>, Json(body): Json<Value>) -> Json<Value> {
    let id = body["jobId"].as_i64().unwrap();
    let mut s = state.lock().unwrap();
    for job in &mut s.jobs {
        if job.id.0 == id {
            job.status = JobStatus::Completed;
        }
    }
    s.lines.insert(id, lines_of(id));
    Json(json!({"status": "completed", "jobId": id}))
}

pub fn host() -> Host {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let (jobs, seeded_lines) = seed();
    let state: Shared = Arc::new(Mutex::new(State_ { jobs, lines: seeded_lines, next_id: 100, puts: vec![], started: vec![], deleted: vec![], exports: vec![], options: vec![], insufficient: false, requests: vec![] }));
    let routes = Router::new()
        .route("/auth/v1/token", post(token))
        .route("/auth/v1/logout", post(|| async { StatusCode::NO_CONTENT }))
        .route("/api/session", get(|| async { Json(session_profile()) }))
        .route("/api/billing/overview", get(|| async { Json(json!({"balance": 75, "is_freemium": false, "special_client_alert": null})) }))
        .route("/api/jobs", get(list).post(create_draft))
        .route("/api/jobs/drafts", get(drafts))
        .route("/api/jobs/options", patch(options))
        .route("/api/jobs/start", post(start))
        .route("/api/jobs/{id}", get(detail).delete(remove))
        .route("/api/jobs/{id}/draft", delete(remove))
        .route("/api/jobs/{id}/lines", get(lines))
        .route("/api/jobs/{id}/export", post(export))
        .route("/api/init-upload", post(init_upload))
        .route("/put/{name}", put(put_file))
        .route("/api/languages/usage", get(|| async { Json(json!({"languages": [], "targets": []})) }))
        .route("/api/settings", get(|| async { Json(transgate_core::privacy::settings::UserSettings::defaults(session_profile().account)) }))
        .route("/api/youtube/add", post(youtube_add))
        .route("/api/youtube/resolve", post(youtube_resolve))
        .with_state(state.clone());
    let origin = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
        origin
    });
    Host { origin, state, _runtime: runtime }
}

/// A vault file that already holds a valid session, as a finished `login` leaves it.
pub fn signed_in_vault(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("session.json");
    let stored = json!({"access_token": "tok", "refresh_token": "ref", "expires_at": 4_000_000_000i64, "user_id": fixtures::account().0.to_string(), "email": EMAIL});
    Vault::file(&path).set("transgate.session", &stored.to_string());
    path
}
