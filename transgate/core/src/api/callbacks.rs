//! Customer callbacks (API-015/016/017, D5/D6/D7/D10): which events deliver,
//! when an attempt is retried or final, when to stop waiting for the summary,
//! the payload and the signature format. The dispatcher applies these with
//! the host's outbox and transport.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::ai::ports::SummaryOutcome;
use crate::ids::{AccountId, JobId, MICROS_PER_SECOND, Micros};
use crate::jobs::domain::Job;
use crate::jobs::events::JobEvent;
use crate::transcript::Line;

/// API-016.
pub const USER_AGENT: &str = "Transgate-Callback/1.0";
pub const TIMEOUT_SECONDS: u64 = 10;
/// D5: stable across retries; the receiver's dedupe key.
pub const DELIVERY_ID_HEADER: &str = "Transgate-Delivery-Id";
/// D5: `t=<unix seconds>,v1=<hex HMAC-SHA256(secret, "<t>." + body)>`.
pub const SIGNATURE_HEADER: &str = "Transgate-Signature";
/// D5: delay before attempt n+1 after attempt n (attempt 1 at +0): six attempts.
pub const SCHEDULE_SECONDS: [i64; 6] = [0, 60, 300, 1_800, 7_200, 43_200];
pub const MAX_ATTEMPTS: u32 = SCHEDULE_SECONDS.len() as u32;
/// D7: the longest wait for the general summary after completion.
pub const SUMMARY_WAIT: i64 = 15 * 60 * MICROS_PER_SECOND;
/// How often a waiting delivery looks at the summary again.
pub const SUMMARY_RECHECK: i64 = 30 * MICROS_PER_SECOND;
/// API-017: the response excerpt kept, as the source logs.
pub const EXCERPT_CHARS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeliveryId(pub Uuid);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Pending,
    Delivered,
    /// A final refusal (non-retryable 4xx, redirect, or a refused destination).
    Failed,
    /// Every scheduled attempt failed, or the job is gone.
    Abandoned,
}

/// One delivery per completed event with a callback URL. `event_id` is unique,
/// so a relay retry inserts nothing new (D5). `payload` is the last body sent,
/// without `audio_url` (D10).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CallbackDelivery {
    pub id: DeliveryId,
    pub account: AccountId,
    pub job: JobId,
    pub event_id: i64,
    pub url: String,
    pub state: DeliveryState,
    /// Attempts begun so far.
    pub attempts: u32,
    pub next_attempt_at: Micros,
    /// The completion instant (D7 counts the summary wait from it).
    pub completed_at: Micros,
    pub created_at: Micros,
    pub delivered_at: Option<Micros>,
    pub payload: Option<Value>,
}

/// D6: only a completion with a non-empty callback URL delivers (the source
/// called the handler for completed jobs only, `webhook-handler.ts:41-45,64-67`).
pub fn delivery_for(id: DeliveryId, event_id: i64, event: &JobEvent, now: Micros) -> Option<CallbackDelivery> {
    let JobEvent::Completed { job, account, callback_url: Some(url), at, .. } = event else { return None };
    if url.is_empty() {
        return None;
    }
    Some(CallbackDelivery {
        id,
        account: *account,
        job: *job,
        event_id,
        url: url.clone(),
        state: DeliveryState::Pending,
        attempts: 0,
        next_attempt_at: now,
        completed_at: *at,
        created_at: now,
        delivered_at: None,
        payload: None,
    })
}

/// What one POST produced, as the host transport saw it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendOutcome {
    /// An HTTP answer; `excerpt` is at most [`EXCERPT_CHARS`] of the body.
    Status { code: u16, excerpt: String },
    /// No answer within [`TIMEOUT_SECONDS`].
    Timeout,
    /// Connection or DNS failure.
    Network { error: String },
    /// The destination failed the send-time SSRF check (API-D06): private,
    /// loopback or otherwise disallowed address. Never sent.
    Refused { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Delivered,
    Retry,
    Final,
}

/// D5: any 2xx delivers; network error, timeout, 408, 429 and 5xx retry;
/// every other answer (other 4xx, 1xx, 3xx since redirects are not followed)
/// and a refused destination are final.
pub fn classify(outcome: &SendOutcome) -> Verdict {
    match outcome {
        SendOutcome::Status { code: 200..=299, .. } => Verdict::Delivered,
        SendOutcome::Status { code: 408 | 429 | 500..=599, .. } | SendOutcome::Timeout | SendOutcome::Network { .. } => Verdict::Retry,
        SendOutcome::Status { .. } | SendOutcome::Refused { .. } => Verdict::Final,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settlement {
    pub state: DeliveryState,
    pub next_attempt_at: Option<Micros>,
    pub delivered_at: Option<Micros>,
}

/// The delivery after attempt `n` (1-based) ended with `outcome` at `now`.
pub fn settle(n: u32, outcome: &SendOutcome, now: Micros) -> Settlement {
    let ended = |state| Settlement { state, next_attempt_at: None, delivered_at: None };
    match classify(outcome) {
        Verdict::Delivered => Settlement { state: DeliveryState::Delivered, next_attempt_at: None, delivered_at: Some(now) },
        Verdict::Final => ended(DeliveryState::Failed),
        Verdict::Retry => match SCHEDULE_SECONDS.get(n as usize) {
            Some(delay) => Settlement {
                state: DeliveryState::Pending,
                next_attempt_at: Some(Micros(now.0 + delay * MICROS_PER_SECOND)),
                delivered_at: None,
            },
            None => ended(DeliveryState::Abandoned),
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryGate {
    /// The summary is still undecided: look again at `recheck_at`.
    Wait { recheck_at: Micros },
    /// Send now (with the summary only if one was saved and is readable).
    Send,
}

/// D7: wait while the P4 outcome is undecided, at most [`SUMMARY_WAIT`] after
/// completion, then send without it (the source sent without on failure).
pub fn summary_gate(outcome: Option<&SummaryOutcome>, completed_at: Micros, now: Micros) -> SummaryGate {
    let deadline = completed_at.0 + SUMMARY_WAIT;
    if outcome.is_some() || now.0 >= deadline {
        return SummaryGate::Send;
    }
    SummaryGate::Wait { recheck_at: Micros((now.0 + SUMMARY_RECHECK).min(deadline)) }
}

/// API-015 body, as `GET /api/v1/transcriptions/{id}` (`callback-delivery.ts:114-173`):
/// `job{id,status,duration,language,audio_url,fix_grammer}`, `result[]` with
/// `corrected_text`, `errors`, and `summary{text}` only when there is one.
/// `audio_url` is null here; [`with_audio_url`] signs it in at send (D10).
pub fn payload(job: &Job, lines: &[Line], summary: Option<&str>) -> Value {
    let result: Vec<Value> = lines
        .iter()
        .map(|l| {
            json!({
                "start_time": l.start_time,
                "end_time": l.end_time,
                "text": l.line,
                "corrected_text": l.corrected_text,
                "speaker": l.speaker,
            })
        })
        .collect();
    let mut body = json!({
        "job": {
            "id": job.id.0,
            "status": job.status.as_str(),
            "duration": job.duration_minutes,
            "language": job.language.map(|l| l.name()),
            "audio_url": null,
            "fix_grammer": job.fix_grammer,
        },
        "result": result,
        "errors": job.error,
    });
    if let Some(text) = summary {
        body["summary"] = json!({ "text": text });
    }
    body
}

/// The stored payload with a freshly signed audio URL (or null when none could be made).
pub fn with_audio_url(payload: &Value, audio_url: Option<&str>) -> Value {
    let mut body = payload.clone();
    if let Some(job) = body.get_mut("job").and_then(Value::as_object_mut) {
        job.insert("audio_url".into(), audio_url.map_or(Value::Null, |u| Value::String(u.into())));
    }
    body
}

/// The bytes the HMAC covers: `"<t>." + body`.
pub fn signed_message(t_unix: i64, body: &[u8]) -> Vec<u8> {
    let mut message = format!("{t_unix}.").into_bytes();
    message.extend_from_slice(body);
    message
}

/// The header value for a computed HMAC.
pub fn signature(t_unix: i64, hex: &str) -> String {
    format!("t={t_unix},v1={hex}")
}

#[cfg(test)]
mod tests;
