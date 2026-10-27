//! The public API contract (P7): request parsing and the response DTOs of the key-authenticated
//! transcription routes and the session key-management routes. Every response is built from a
//! DTO that derives `JsonSchema`, so the OpenAPI document is generated from the very types that
//! produce the bodies (D9). No rule lives here: scopes, limits, idempotency and key lifecycle are
//! core's (`transgate_core::api`).

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use transgate_core::api::ApiError;
use transgate_core::api::keys::{ExpirationPolicy, KeyView, NewKey};
use transgate_core::api::limits::ApiRate;
use transgate_core::api::Scope;
use transgate_core::ai::SummaryView;
use transgate_core::ids::{JobId, Micros};
use transgate_core::jobs::app::{JobResult, PublicAnswer, PublicStart, StartMeta};
use transgate_core::jobs::domain::Job;
use transgate_core::transcript::Line;

use crate::{Invalid, Transcriptions, iso8601, object, parse_transcriptions};

// ---- requests -------------------------------------------------------------------

/// `POST /api/v1/transcriptions`: exactly one of `jobIds` and `audioUrls`.
#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct TranscriptionsRequest {
    /// Draft jobs to start (positive integers). Cannot be combined with `audioUrls`.
    pub job_ids: Option<Vec<i64>>,
    /// Audio to fetch and transcribe. Cannot be combined with `jobIds`.
    pub audio_urls: Option<Vec<AudioUrlRequest>>,
    /// The caller's locale, used for the freemium exception rules.
    pub locale: Option<String>,
    pub options: Option<RequestOptions>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct AudioUrlRequest {
    /// An absolute `http` or `https` URL of the audio.
    pub audio_url: String,
    /// Receives a signed POST with the result when the job completes.
    pub callback_url: Option<String>,
    /// A language name such as `English`; English when absent.
    pub language: Option<String>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct RequestOptions {
    pub webhook_url: Option<String>,
    pub preferred_provider: Option<String>,
    pub fallback_enabled: Option<bool>,
}

/// The two public start shapes as core's `PublicStart`.
pub fn parse_public_start(body: &[u8]) -> Result<PublicStart, Invalid> {
    Ok(match parse_transcriptions(body)? {
        Transcriptions::JobIds(request) => PublicStart::Jobs(request),
        Transcriptions::AudioUrls(request) => PublicStart::Urls(request),
    })
}

/// `GET /api/v1/transcriptions?jobId=`.
pub fn parse_job_id(raw: Option<&str>) -> Result<JobId, Invalid> {
    let raw = raw.filter(|r| !r.is_empty()).ok_or("jobId parameter is required")?;
    // `parseInt(value, 10)`: a leading integer is the id, anything non-numeric is invalid.
    let digits: String = raw.trim().chars().enumerate().take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && matches!(c, '-' | '+'))).map(|(_, c)| c).collect();
    digits.parse::<i64>().ok().filter(|n| *n > 0).map(JobId).ok_or_else(|| "Invalid jobId parameter".to_string())
}

/// `POST /api/keys`.
#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct NewKeyRequest {
    /// A label for the dashboard.
    pub name: String,
    /// `transcriptions:read`, `transcriptions:create` or both.
    pub scopes: Vec<String>,
    pub expiration_policy: ExpirationPolicyView,
    /// Days until expiry; read only when `expirationPolicy` is `custom`.
    pub custom_days: Option<i32>,
}

/// The source `expiration_policy_type` values.
#[derive(JsonSchema, Serialize)]
pub enum ExpirationPolicyView {
    #[serde(rename = "never")]
    Never,
    #[serde(rename = "30_days")]
    Days30,
    #[serde(rename = "90_days")]
    Days90,
    #[serde(rename = "1_year")]
    OneYear,
    #[serde(rename = "custom")]
    Custom,
}

pub fn parse_new_key(body: &[u8]) -> Result<NewKey, Invalid> {
    let map = object(body)?;
    let name = map.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).ok_or("name is required")?.to_string();
    let scopes = map
        .get("scopes")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or("scopes must be a nonempty array")?
        .iter()
        .map(|s| serde_json::from_value::<Scope>(s.clone()).map_err(|_| format!("Unsupported scope: {}", s.as_str().unwrap_or("?"))))
        .collect::<Result<Vec<_>, _>>()?;
    let policy = map.get("expirationPolicy").cloned().unwrap_or(Value::String("never".into()));
    let expiration_policy = serde_json::from_value::<ExpirationPolicy>(policy).map_err(|_| "Unsupported expirationPolicy".to_string())?;
    let custom_days = match map.get("customDays") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_i64().and_then(|n| i32::try_from(n).ok()).ok_or("customDays must be a whole number")?),
    };
    Ok(NewKey { name, scopes, expiration_policy, custom_days })
}

// ---- responses ------------------------------------------------------------------

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobResultView {
    pub job_id: i64,
    /// `not_started`, `pending`, `running`, `completed`, `failed` and the other job statuses.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_completion_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartMetaView {
    pub new_balance: i64,
    pub is_freemium: bool,
    pub total_jobs: usize,
    pub successful_jobs: usize,
    pub failed_jobs: usize,
    pub queued_jobs: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_completion: Option<String>,
    /// `jobIds` or `audioUrls`.
    pub request_type: String,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartData {
    pub success: bool,
    pub results: Vec<JobResultView>,
    pub metadata: StartMetaView,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResponseMetadata {
    pub request_id: String,
    /// RFC 3339 UTC.
    pub timestamp: String,
    /// Milliseconds spent on the request.
    pub processing_time: u64,
    pub client_type: String,
}

/// 201 of `POST /api/v1/transcriptions`.
#[derive(Serialize, JsonSchema)]
pub struct StartResponse {
    pub success: bool,
    pub data: StartData,
    pub metadata: ResponseMetadata,
}

pub fn result_view(result: &JobResult) -> JobResultView {
    JobResultView {
        job_id: result.job_id.0,
        status: result.status.as_str().into(),
        estimated_completion_time: result.estimated_completion.map(iso8601),
        error: result.error.clone(),
    }
}

pub fn meta_view(meta: &StartMeta) -> StartMetaView {
    StartMetaView {
        new_balance: meta.new_balance,
        is_freemium: meta.is_freemium,
        total_jobs: meta.total_jobs,
        successful_jobs: meta.successful_jobs,
        failed_jobs: meta.failed_jobs,
        queued_jobs: meta.queued_jobs,
        estimated_completion: meta.estimated_completion.map(iso8601),
        request_type: meta.request_type.clone(),
    }
}

pub fn metadata(request_id: &str, at: Micros, processing_ms: u64, client_type: &str) -> ResponseMetadata {
    ResponseMetadata { request_id: request_id.into(), timestamp: iso8601(at), processing_time: processing_ms, client_type: client_type.into() }
}

pub fn public_start_response(answer: &PublicAnswer, request_id: &str, at: Micros, processing_ms: u64) -> StartResponse {
    StartResponse {
        success: true,
        data: StartData {
            success: true,
            results: answer.answer.results.iter().map(result_view).collect(),
            metadata: meta_view(&answer.answer.meta),
        },
        metadata: metadata(request_id, at, processing_ms, "api"),
    }
}

#[derive(Serialize, JsonSchema)]
pub struct JobView {
    pub id: i64,
    pub status: String,
    /// Whole minutes, when known.
    pub duration: Option<i32>,
    pub language: Option<String>,
    /// A short-lived signed URL of the stored audio; null when none is stored.
    pub audio_url: Option<String>,
    pub fix_grammer: Option<bool>,
}

#[derive(Serialize, JsonSchema)]
pub struct ResultLine {
    pub start_time: String,
    pub end_time: String,
    pub text: String,
    pub corrected_text: Option<String>,
    pub speaker: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct SummaryText {
    pub text: String,
}

#[derive(Serialize, JsonSchema)]
pub struct StatusData {
    pub job: JobView,
    /// Present once the job is completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Vec<ResultLine>>,
    /// The general summary, when one was made and its owner may read it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SummaryText>,
    /// The job's error record, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Value>,
}

/// 200 of `GET /api/v1/transcriptions`.
#[derive(Serialize, JsonSchema)]
pub struct StatusResponse {
    pub success: bool,
    pub data: StatusData,
    pub metadata: ResponseMetadata,
}

pub fn status_response(
    job: &Job,
    lines: &[Line],
    summary: Option<&SummaryView>,
    audio_url: Option<String>,
    request_id: &str,
    at: Micros,
    processing_ms: u64,
) -> StatusResponse {
    let completed = job.status == transgate_core::jobs::domain::JobStatus::Completed;
    StatusResponse {
        success: true,
        data: StatusData {
            job: JobView {
                id: job.id.0,
                status: job.status.as_str().into(),
                duration: job.duration_minutes,
                language: job.language.map(|l| l.name().to_string()),
                audio_url,
                fix_grammer: job.fix_grammer,
            },
            result: completed.then(|| {
                lines
                    .iter()
                    .map(|l| ResultLine {
                        start_time: l.start_time.clone(),
                        end_time: l.end_time.clone(),
                        text: l.line.clone(),
                        corrected_text: l.corrected_text.clone(),
                        speaker: l.speaker.clone(),
                    })
                    .collect()
            }),
            summary: completed.then(|| summary.map(|s| SummaryText { text: s.text.clone() })).flatten(),
            errors: job.error.clone(),
        },
        metadata: metadata(request_id, at, processing_ms, "api"),
    }
}

// ---- keys -----------------------------------------------------------------------

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyView {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub is_active: bool,
    pub scopes: Vec<String>,
    pub expiration_policy: ExpirationPolicyView,
    /// The first characters of the key, for recognising it in a list.
    pub hint: Option<String>,
}

pub fn key_view(key: &KeyView) -> ApiKeyView {
    ApiKeyView {
        id: key.id.0.to_string(),
        name: key.name.clone(),
        created_at: iso8601(key.created_at),
        expires_at: key.expires_at.map(iso8601),
        is_active: key.active,
        scopes: key.scopes.clone(),
        expiration_policy: match key.expiration_policy {
            ExpirationPolicy::Never => ExpirationPolicyView::Never,
            ExpirationPolicy::Days30 => ExpirationPolicyView::Days30,
            ExpirationPolicy::Days90 => ExpirationPolicyView::Days90,
            ExpirationPolicy::OneYear => ExpirationPolicyView::OneYear,
            ExpirationPolicy::Custom => ExpirationPolicyView::Custom,
        },
        hint: key.hint.clone(),
    }
}

/// 200 of `GET /api/keys`.
#[derive(Serialize, JsonSchema)]
pub struct KeyListResponse {
    pub keys: Vec<ApiKeyView>,
}

/// 201 of `POST /api/keys`: the plaintext key is returned exactly once.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedKeyResponse {
    pub key: ApiKeyView,
    pub api_key: String,
}

/// 200 of `DELETE /api/keys/{id}`.
#[derive(Serialize, JsonSchema)]
pub struct RevokedKeyResponse {
    pub key: ApiKeyView,
}

/// The per-account callback signing secret an owner reads on the key screen.
#[derive(Serialize, JsonSchema)]
pub struct SigningSecretResponse {
    pub secret: String,
}

/// 200 of `POST /api/admin/callbacks/{id}/replay`.
#[derive(Serialize, JsonSchema)]
pub struct ReplayResponse {
    /// `queued`: the next dispatch pass appends an attempt to the same delivery.
    pub status: String,
}

// ---- errors and rate headers -----------------------------------------------------

/// Every failure body: a message and a stable code.
#[derive(Serialize, JsonSchema)]
pub struct ErrorBody {
    pub error: String,
    pub code: String,
}

pub fn error_body(error: &ApiError) -> ErrorBody {
    ErrorBody { error: error.message(), code: error.code().into() }
}

/// RTL-003: the limits, what remains and when each window resets. On a refusal also `Retry-After`
/// in whole seconds until the exhausted window resets.
pub fn rate_headers(rate: &ApiRate, now: Micros, refused: bool) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("X-RateLimit-Limit-Minute", rate.limits.per_minute.to_string()),
        ("X-RateLimit-Limit-Hour", rate.limits.per_hour.to_string()),
        ("X-RateLimit-Remaining-Minute", rate.remaining_minute.to_string()),
        ("X-RateLimit-Remaining-Hour", rate.remaining_hour.to_string()),
        ("X-RateLimit-Reset-Minute", iso8601(rate.reset_minute)),
        ("X-RateLimit-Reset-Hour", iso8601(rate.reset_hour)),
    ];
    if refused {
        let reset = if rate.remaining_minute == 0 { rate.reset_minute } else { rate.reset_hour };
        headers.push(("Retry-After", ((reset.0 - now.0).max(0) / 1_000_000 + 1).to_string()));
    }
    headers
}

#[cfg(test)]
mod tests;
