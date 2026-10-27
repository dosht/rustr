//! The authenticated session API, typed with the core types the host serializes.
//! Every call carries the access token of an explicit user, so a request issued
//! for one account can never run with another account's token.
use std::sync::Arc;

use reqwest::{Method, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use transgate_core::account::{FeedbackRating, JobFeedback, SessionProfile};
use transgate_core::jobs::app::{CalendarContext, DraftOptions, ExportRequest, JobDetail, JobPage, NewDraft, ResolveAnswer};
use transgate_core::jobs::domain::Job;
use transgate_core::jobs::ports::LanguageUsage;
use transgate_core::privacy::erasure::ErasureView;
use transgate_core::privacy::settings::UserSettings;
use transgate_core::transcript::{Line, LineEdit};
use transgate_core::{JobId, Language};
use uuid::Uuid;

use crate::auth::GoTrue;

#[derive(Clone, Debug, PartialEq)]
pub enum ApiError {
    /// `user` has no usable session in this origin (signed out, refresh rejected,
    /// or another account signed in). Nothing was sent.
    SignedOut,
    /// No HTTP answer: offline, aborted or timed out. The outcome is unknown.
    Network(String),
    /// An answer outside 2xx. `code`/`message` come from the host's
    /// `{error, code}` body; a bare string body (the three source text 401s) is the message.
    Http { status: u16, code: Option<String>, message: Option<String>, body: Option<Value> },
    /// A 2xx whose body does not match the published contract.
    Decode(String),
}

impl ApiError {
    pub fn status(&self) -> Option<u16> {
        match self {
            ApiError::Http { status, .. } => Some(*status),
            _ => None,
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::SignedOut => write!(f, "no session"),
            ApiError::Network(why) => write!(f, "network: {why}"),
            ApiError::Http { status, code, .. } => write!(f, "HTTP {status} {}", code.as_deref().unwrap_or("")),
            ApiError::Decode(why) => write!(f, "unexpected answer: {why}"),
        }
    }
}

impl std::error::Error for ApiError {}

async fn http_error(response: Response) -> ApiError {
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    let body: Option<Value> = serde_json::from_str(&text).ok();
    // transgate-http answers `{error, code}`; billing-http `{error: {code, message}}`.
    let text_of = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
    let (code, message) = match &body {
        Some(Value::Object(map)) => match map.get("error") {
            Some(Value::Object(nested)) => (text_of(nested.get("code")), text_of(nested.get("message"))),
            flat => (text_of(map.get("code")), text_of(flat)),
        },
        Some(Value::String(message)) => (None, Some(message.clone())),
        _ => (None, (!text.is_empty()).then_some(text)),
    };
    ApiError::Http { status, code, message, body }
}

fn network(e: reqwest::Error) -> ApiError {
    ApiError::Network(e.to_string())
}

/// Bytes the host produced for download, with its chosen file name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// The job detail plus the optional completion summary the host appends.
#[derive(Clone, Debug, PartialEq)]
pub struct JobDetailView {
    pub detail: JobDetail,
    pub summary: Option<String>,
}

/// `filename*=UTF-8''…` (RFC 6266) when present, else the quoted ASCII fallback.
pub fn disposition_filename(header: &str) -> Option<String> {
    let parts = header.split(';').map(str::trim);
    let mut fallback = None;
    for part in parts {
        if let Some(encoded) = part.strip_prefix("filename*=UTF-8''") {
            let decoded: Vec<u8> = percent_decode(encoded)?;
            return String::from_utf8(decoded).ok();
        }
        if let Some(quoted) = part.strip_prefix("filename=") {
            fallback = Some(quoted.trim_matches('"').to_string());
        }
    }
    fallback
}

fn percent_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

pub struct HttpApi {
    http: reqwest::Client,
    base: String,
    auth: Arc<GoTrue>,
}

/// Most list pages (20 rows each) one D11 poll reads: 500 rows. Older loaded rows
/// simply keep their last state until the next full load.
const POLL_PAGE_LIMIT: usize = 25;

/// The `{ status, jobId, error? }` answer of `/api/youtube/resolve`
/// (`transgate_api::resolve_response`): camelCase, the failure detail nested.
#[derive(serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ResolveWire {
    Completed {
        #[serde(rename = "jobId")]
        job_id: JobId,
    },
    Failed {
        #[serde(rename = "jobId")]
        job_id: JobId,
        error: ResolveFailure,
    },
}

#[derive(serde::Deserialize)]
struct ResolveFailure {
    code: String,
    message: String,
}

impl From<ResolveWire> for ResolveAnswer {
    fn from(wire: ResolveWire) -> Self {
        match wire {
            ResolveWire::Completed { job_id } => ResolveAnswer::Completed { job_id },
            ResolveWire::Failed { job_id, error } => ResolveAnswer::Failed { job_id, code: error.code, message: error.message },
        }
    }
}

impl HttpApi {
    /// `base` is the API root, e.g. `https://app.example/api`.
    pub fn new(base: impl Into<String>, auth: Arc<GoTrue>) -> Self {
        Self { http: crate::http::http_client(), base: base.into().trim_end_matches('/').to_string(), auth }
    }

    pub fn auth(&self) -> &Arc<GoTrue> {
        &self.auth
    }

    /// Sends one authenticated request and returns the raw answer on 2xx.
    pub async fn send(&self, user: Uuid, method: Method, path: &str, body: Option<&Value>) -> Result<Response, ApiError> {
        let token = self.auth.access_token(user).await.map_err(|e| ApiError::Network(e.to_string()))?.ok_or(ApiError::SignedOut)?;
        let mut request = self.http.request(method, format!("{}{path}", self.base)).bearer_auth(token);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(network)?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(http_error(response).await)
        }
    }

    pub(crate) async fn json<T: DeserializeOwned>(&self, user: Uuid, method: Method, path: &str, body: Option<&Value>) -> Result<T, ApiError> {
        let response = self.send(user, method, path, body).await?;
        let text = response.text().await.map_err(network)?;
        serde_json::from_str(&text).map_err(|e| ApiError::Decode(e.to_string()))
    }

    async fn empty(&self, user: Uuid, method: Method, path: &str) -> Result<(), ApiError> {
        self.send(user, method, path, None).await.map(drop)
    }

    fn value(value: impl Serialize) -> Result<Value, ApiError> {
        serde_json::to_value(value).map_err(|e| ApiError::Decode(e.to_string()))
    }

    pub async fn list_jobs(&self, user: Uuid, before: Option<JobId>, calendar: CalendarContext) -> Result<JobPage, ApiError> {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        if let Some(before) = before {
            query.append_pair("before", &before.0.to_string());
        }
        query.append_pair("offsetNow", &calendar.offset_now_minutes.to_string());
        query.append_pair("offsetThen", &calendar.offset_then_minutes.to_string());
        self.json(user, Method::GET, &format!("/jobs?{}", query.finish()), None).await
    }

    /// D11 list poll: the owned list newest first, page after page, until the page
    /// that holds `through` (the oldest id this tab has loaded; `None` = just the
    /// first page). Bounded by `POLL_PAGE_LIMIT`: rows beyond it keep their loaded
    /// state, and a failure on any page fails the whole poll so a partial answer
    /// can never be merged as if complete.
    pub async fn poll_jobs(&self, user: Uuid, through: Option<JobId>, calendar: CalendarContext) -> Result<Vec<Job>, ApiError> {
        let mut jobs = Vec::new();
        let mut before = None;
        for _ in 0..POLL_PAGE_LIMIT {
            let page = self.list_jobs(user, before, calendar).await?;
            let covered = through.is_none_or(|t| page.jobs.iter().any(|j| j.id <= t));
            jobs.extend(page.jobs);
            match page.next_cursor {
                Some(cursor) if !covered => before = Some(cursor),
                _ => break,
            }
        }
        Ok(jobs)
    }

    pub async fn job_detail(&self, user: Uuid, job: JobId) -> Result<JobDetailView, ApiError> {
        let mut value: Value = self.json(user, Method::GET, &format!("/jobs/{job}"), None).await?;
        let summary = value.as_object_mut().and_then(|m| m.remove("summary")).and_then(|s| s.as_str().map(str::to_string));
        let detail = serde_json::from_value(value).map_err(|e| ApiError::Decode(e.to_string()))?;
        Ok(JobDetailView { detail, summary })
    }

    pub async fn job_lines(&self, user: Uuid, job: JobId) -> Result<Vec<Line>, ApiError> {
        self.json(user, Method::GET, &format!("/jobs/{job}/lines"), None).await
    }

    pub async fn create_draft(&self, user: Uuid, draft: &NewDraft) -> Result<Job, ApiError> {
        self.json(user, Method::POST, "/jobs", Some(&Self::value(draft)?)).await
    }

    /// `PATCH /api/jobs/options`: an absent field is left alone, `language: null` is auto-detect.
    pub async fn set_draft_options(&self, user: Uuid, jobs: &[JobId], options: &DraftOptions) -> Result<Vec<Job>, ApiError> {
        let mut body = Map::new();
        body.insert("jobIds".into(), Self::value(jobs)?);
        match &options.language {
            None => {}
            Some(None) => {
                body.insert("language".into(), Value::Null);
            }
            Some(Some(language)) => {
                body.insert("language".into(), Value::String(language.name().to_string()));
            }
        }
        if let Some(targets) = &options.translate_to {
            body.insert("translateTo".into(), Value::Array(targets.iter().map(|l| Value::String(l.name().into())).collect()));
        }
        if let Some(speakers) = options.speakers {
            body.insert("speakers".into(), Value::Bool(speakers));
        }
        self.json(user, Method::PATCH, "/jobs/options", Some(&Value::Object(body))).await
    }

    pub async fn remove_draft(&self, user: Uuid, job: JobId) -> Result<(), ApiError> {
        self.empty(user, Method::DELETE, &format!("/jobs/{job}/draft")).await
    }

    pub async fn delete_job(&self, user: Uuid, job: JobId) -> Result<(), ApiError> {
        self.empty(user, Method::DELETE, &format!("/jobs/{job}")).await
    }

    /// Soft-deletes every job; answers how many.
    pub async fn delete_all(&self, user: Uuid) -> Result<u64, ApiError> {
        let value: Value = self.json(user, Method::DELETE, "/jobs", None).await?;
        value.get("deleted").and_then(Value::as_u64).ok_or_else(|| ApiError::Decode("missing deleted".into()))
    }

    /// Last write wins (P3 D7); answers the host's `updated` value.
    pub async fn edit_line(&self, user: Uuid, edit: &LineEdit) -> Result<Value, ApiError> {
        let value: Value = self.json(user, Method::PATCH, "/jobs/lines", Some(&Self::value(edit)?)).await?;
        Ok(value.get("updated").cloned().unwrap_or(Value::Null))
    }

    pub async fn export(&self, user: Uuid, job: JobId, request: &ExportRequest) -> Result<Download, ApiError> {
        let response = self.send(user, Method::POST, &format!("/jobs/{job}/export"), Some(&Self::value(request)?)).await?;
        let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        let filename = header("content-disposition").as_deref().and_then(disposition_filename).ok_or_else(|| ApiError::Decode("missing file name".into()))?;
        let content_type = header("content-type").unwrap_or_else(|| "application/zip".into());
        let bytes = response.bytes().await.map_err(network)?.to_vec();
        Ok(Download { filename, content_type, bytes })
    }

    /// The signed PUT URL for a draft's file (the answer is a bare JSON string).
    pub async fn init_upload(&self, user: Uuid, job: JobId, file_name: &str) -> Result<String, ApiError> {
        self.json(user, Method::POST, "/init-upload", Some(&json!({ "jobId": job.0, "fileName": file_name }))).await
    }

    /// The signed playback URL (UPL-016).
    pub async fn streaming_url(&self, user: Uuid, job: JobId, file_name: &str) -> Result<String, ApiError> {
        self.json(user, Method::POST, "/streaming-url", Some(&json!({ "jobId": job.0, "fileName": file_name }))).await
    }

    pub async fn youtube_add(&self, user: Uuid, url: &str, language: Option<Language>) -> Result<(Job, media_ports::VideoMetadata), ApiError> {
        let mut body = json!({ "youtubeUrl": url });
        if let Some(language) = language {
            body["language"] = Value::String(language.name().into());
        }
        let mut value: Value = self.json(user, Method::POST, "/youtube/add", Some(&body)).await?;
        let decode = |v: Option<Value>| v.ok_or_else(|| ApiError::Decode("missing field".into()));
        let job = serde_json::from_value(decode(value.get_mut("job").map(Value::take))?).map_err(|e| ApiError::Decode(e.to_string()))?;
        let metadata = serde_json::from_value(decode(value.get_mut("metadata").map(Value::take))?).map_err(|e| ApiError::Decode(e.to_string()))?;
        Ok((job, metadata))
    }

    /// `POST /api/youtube/resolve`: the host's `{status, jobId, error?}` envelope,
    /// decoded through its typed wire shape (`ResolveWire`), never core's own serde
    /// shape of `ResolveAnswer`.
    pub async fn youtube_resolve(&self, user: Uuid, job: JobId) -> Result<ResolveAnswer, ApiError> {
        let wire: ResolveWire = self.json(user, Method::POST, "/youtube/resolve", Some(&json!({ "jobId": job.0 }))).await?;
        Ok(ResolveAnswer::from(wire))
    }

    pub async fn language_usage(&self, user: Uuid) -> Result<LanguageUsage, ApiError> {
        let value: Value = self.json(user, Method::GET, "/languages/usage", None).await?;
        let rows = |key: &str| -> Result<Vec<(Language, u64)>, ApiError> {
            value
                .get(key)
                .and_then(Value::as_array)
                .ok_or_else(|| ApiError::Decode(format!("missing {key}")))?
                .iter()
                .map(|row| {
                    let language = row.get("language").and_then(Value::as_str).and_then(Language::from_name);
                    let count = row.get("count").and_then(Value::as_u64);
                    language.zip(count).ok_or_else(|| ApiError::Decode(format!("bad {key} row")))
                })
                .collect()
        };
        Ok(LanguageUsage { languages: rows("languages")?, targets: rows("targets")? })
    }

    pub async fn settings(&self, user: Uuid) -> Result<UserSettings, ApiError> {
        self.json(user, Method::GET, "/settings", None).await
    }

    /// PRV-027 auto-delete and the stored legacy monthly toggle; nothing else is writable.
    pub async fn update_settings(&self, user: Uuid, auto_delete: Option<bool>, deleted_monthly: Option<bool>) -> Result<UserSettings, ApiError> {
        let mut body = Map::new();
        if let Some(value) = auto_delete {
            body.insert("autoDelete".into(), Value::Bool(value));
        }
        if let Some(value) = deleted_monthly {
            body.insert("deletedMonthly".into(), Value::Bool(value));
        }
        self.json(user, Method::PATCH, "/settings", Some(&Value::Object(body))).await
    }

    /// `GET /api/jobs/drafts`: every own draft, oldest first (upload restore and reload).
    pub async fn drafts(&self, user: Uuid) -> Result<Vec<Job>, ApiError> {
        self.json(user, Method::GET, "/jobs/drafts", None).await
    }

    /// `GET /api/session?locale=`: the verified profile and effective capabilities.
    pub async fn session(&self, user: Uuid, locale: &str) -> Result<SessionProfile, ApiError> {
        let query = url::form_urlencoded::Serializer::new(String::new()).append_pair("locale", locale).finish();
        self.json(user, Method::GET, &format!("/session?{query}"), None).await
    }

    /// `POST /api/jobs/start` (D5): the session start after options were rewritten.
    /// Answers the existing 201 envelope; a refusal keeps its status and code
    /// (402 `insufficient_balance`), never inferred from the message text.
    pub async fn start_jobs(&self, user: Uuid, jobs: &[JobId], locale: Option<&str>) -> Result<Value, ApiError> {
        let mut body = json!({ "jobIds": Self::value(jobs)? });
        if let Some(locale) = locale {
            body["locale"] = Value::String(locale.into());
        }
        self.json(user, Method::POST, "/jobs/start", Some(&body)).await
    }

    /// `POST /api/jobs/{id}/feedback`: the comment is sent as typed.
    pub async fn submit_feedback(&self, user: Uuid, job: JobId, rating: FeedbackRating, comment: &str) -> Result<JobFeedback, ApiError> {
        let body = json!({ "rating": Self::value(rating)?, "feedback": comment });
        self.json(user, Method::POST, &format!("/jobs/{job}/feedback"), Some(&body)).await
    }

    /// `POST /api/support`: 202 means durably queued, not that any mail was sent.
    pub async fn request_support(&self, user: Uuid, message: &str, job: Option<JobId>) -> Result<Uuid, ApiError> {
        let body = json!({ "message": message, "jobId": job.map(|j| j.0) });
        let answer: Value = self.json(user, Method::POST, "/support", Some(&body)).await?;
        answer
            .get("request_id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| ApiError::Decode("missing request_id".into()))
    }

    /// `POST /api/delete-account`: starts the saga; 200 only when already fully gone, else 202.
    pub async fn request_erasure(&self, user: Uuid) -> Result<ErasureView, ApiError> {
        self.json(user, Method::POST, "/delete-account", None).await
    }

    /// `GET /api/account/erasure/status`: the barred owner's receipt; never advances erasure.
    pub async fn erasure_status(&self, user: Uuid) -> Result<ErasureView, ApiError> {
        self.json(user, Method::GET, "/account/erasure/status", None).await
    }
}
