//! AI start and watch transport over the unchanged P4 session routes (P5 D6).
//!
//! A start POST answers `x-generation-id` with a stream body; the body is dropped
//! at once (which ends the request and releases the watch slot) and the panel
//! attaches the canonical watch. No transport loss is ever answered by minting a
//! new request: the caller replays the same key or reattaches.
use futures_util::StreamExt;
use llm_core::{ApplicationFrame, Limits, Persistence, StreamStatus, TextEvent};
use llm_http::{ApplicationDecoder, SseDecoder, SseEvent};
use reqwest::Method;
use serde_json::{Value, json};
use transgate_core::JobId;
use transgate_core::ai::ports::ChatMessage;
use transgate_core::ai::{ConsentView, GenerationId, GenerationView, LimitUsage, LimitsView};
use transgate_core::jobs::correlate::parse_rfc3339;
use transgate_core::presentation::ai::chat::HistoryEntry;
use transgate_core::presentation::ai::{AnalysisWire, RequestKey, StartRejection, WatchFailure, WatchKind};
use transgate_core::presentation::failure::ApiFailure;
use uuid::Uuid;

use crate::api::{ApiError, HttpApi};

impl ApiError {
    /// The core view of a failure. `SignedOut` has none: it is an auth event.
    pub fn failure(&self) -> Option<ApiFailure> {
        match self {
            ApiError::SignedOut => None,
            ApiError::Network(_) => Some(ApiFailure::network()),
            ApiError::Http { status, code, .. } => Some(ApiFailure::http(*status, code.as_deref())),
            ApiError::Decode(_) => Some(ApiFailure { status: None, code: Some("unexpected_answer".into()) }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum StartOutcome {
    Accepted(GenerationId),
    /// The host answered and refused; nothing runs for this key.
    Rejected { status: u16, rejection: StartRejection },
    /// No session for the user: nothing was sent.
    SignedOut,
    /// No answer, or an answer without a generation id: the caller replays the same key.
    Unknown,
}

/// One decoded watch item, typed by the decoder the watch was attached with.
#[derive(Clone, Debug, PartialEq)]
pub enum WatchItem {
    Text(ApplicationFrame),
    Analysis(AnalysisWire),
}

fn terminal_text(frame: &ApplicationFrame) -> bool {
    matches!(
        &frame.event,
        TextEvent::Final { persistence: Persistence::Saved, .. }
            | TextEvent::Interrupted(_)
            | TextEvent::Revoked
            | TextEvent::Snapshot { status: StreamStatus::Completed | StreamStatus::Interrupted | StreamStatus::Cancelled | StreamStatus::Revoked, .. }
    )
}

/// The host's named analysis events (`transgate/http/src/ai.rs` `analysis_bytes`).
pub fn analysis_wire(event: &SseEvent) -> Option<AnalysisWire> {
    let data: Value = serde_json::from_str(&event.data).ok()?;
    let text = |key: &str| data.get(key).and_then(Value::as_str).map(str::to_string);
    let int = |key: &str| data.get(key).and_then(Value::as_i64);
    Some(match event.event.as_str() {
        "sectionCount" => AnalysisWire::SectionCount { total: usize::try_from(int("total")?).ok()? },
        "typeDetected" => AnalysisWire::TypeDetected {
            kind: serde_json::from_value(data.get("type")?.clone()).ok()?,
            confidence: data.get("confidence")?.as_f64()?,
        },
        "sectionReady" => AnalysisWire::SectionReady { section_type: text("sectionType")?, data: data.get("data")?.clone() },
        "complete" => AnalysisWire::Complete { tokens_used: int("tokensUsed").unwrap_or(0), processing_time_ms: int("processingTime").unwrap_or(0) },
        "error" => AnalysisWire::Error { code: text("code")?, message: text("message").unwrap_or_default() },
        _ => return None,
    })
}

impl HttpApi {
    async fn start(&self, user: Uuid, path: &str, body: Value) -> StartOutcome {
        match self.send(user, Method::POST, path, Some(&body)).await {
            Ok(response) => {
                let id = response.headers().get("x-generation-id").and_then(|v| v.to_str().ok()).and_then(|v| Uuid::parse_str(v).ok());
                // Dropping the answer ends the attachment the POST opened (its body is a stream).
                drop(response);
                id.map(|id| StartOutcome::Accepted(GenerationId(id))).unwrap_or(StartOutcome::Unknown)
            }
            Err(ApiError::SignedOut) => StartOutcome::SignedOut,
            Err(ApiError::Http { status, body, .. }) => {
                let rejection = body.and_then(|b| serde_json::from_value(b).ok()).unwrap_or_default();
                StartOutcome::Rejected { status, rejection }
            }
            Err(ApiError::Network(_) | ApiError::Decode(_)) => StartOutcome::Unknown,
        }
    }

    /// `POST /api/v1/ai/analyze {jobId, requestId}`.
    pub async fn start_analysis(&self, user: Uuid, job: JobId, key: RequestKey) -> StartOutcome {
        self.start(user, "/v1/ai/analyze", json!({ "jobId": job.0, "requestId": key })).await
    }

    /// `POST /api/v1/ai/chat {jobId, message, history, requestId}`; history is the
    /// caller's ordered `{role, content}` turns, sent as given (AI-010).
    pub async fn start_chat(&self, user: Uuid, job: JobId, key: RequestKey, message: &str, history: &[HistoryEntry]) -> StartOutcome {
        let history = serde_json::to_value(history).unwrap_or(Value::Array(vec![]));
        self.start(user, "/v1/ai/chat", json!({ "jobId": job.0, "message": message, "history": history, "requestId": key })).await
    }

    pub async fn consent(&self, user: Uuid) -> Result<ConsentView, ApiError> {
        let response = self.send(user, Method::GET, "/ai/consent", None).await?;
        consent_view(&read_json(response).await?)
    }

    /// `POST /api/ai/consent {hasConsented}`: accept or decline (decline is not DELETE).
    pub async fn record_consent(&self, user: Uuid, granted: bool) -> Result<ConsentView, ApiError> {
        let response = self.send(user, Method::POST, "/ai/consent", Some(&json!({ "hasConsented": granted }))).await?;
        consent_view(&read_json(response).await?)
    }

    /// `DELETE /api/ai/consent`: revoke and purge (D3).
    pub async fn revoke_consent(&self, user: Uuid) -> Result<ConsentView, ApiError> {
        let response = self.send(user, Method::DELETE, "/ai/consent", None).await?;
        consent_view(&read_json(response).await?)
    }

    pub async fn chat_limits(&self, user: Uuid, job: JobId) -> Result<LimitsView, ApiError> {
        let response = self.send(user, Method::GET, &format!("/ai/chat-limits?jobId={}", job.0), None).await?;
        limits_view(&read_json(response).await?)
    }

    /// The job's saved analysis generation, if any (`{"analysis": …}`).
    pub async fn analysis(&self, user: Uuid, job: JobId) -> Result<Option<GenerationView>, ApiError> {
        let mut answer = read_json(self.send(user, Method::GET, &format!("/ai/jobs/{}/analysis", job.0), None).await?).await?;
        decode(answer.get_mut("analysis").map(Value::take).unwrap_or(Value::Null))
    }

    /// The job's persisted chat in order (`{"messages": […]}`).
    pub async fn chat_history(&self, user: Uuid, job: JobId) -> Result<Vec<ChatMessage>, ApiError> {
        let mut answer = read_json(self.send(user, Method::GET, &format!("/ai/jobs/{}/chat", job.0), None).await?).await?;
        decode(answer.get_mut("messages").map(Value::take).ok_or_else(|| ApiError::Decode("missing messages".into()))?)
    }

    /// `GET /api/ai/generations/{id}`: the authoritative snapshot after a non-success terminal.
    pub async fn generation(&self, user: Uuid, generation: GenerationId) -> Result<GenerationView, ApiError> {
        decode(read_json(self.send(user, Method::GET, &format!("/ai/generations/{}", generation.0), None).await?).await?)
    }

    /// `POST /api/ai/generations/{id}/retry {requestId}`: new metered work with a new key.
    pub async fn retry_generation(&self, user: Uuid, generation: GenerationId, key: RequestKey) -> StartOutcome {
        let path = format!("/ai/generations/{}/retry", generation.0);
        match self.send(user, Method::POST, &path, Some(&json!({ "requestId": key }))).await {
            Ok(response) => {
                let Ok(text) = response.text().await else { return StartOutcome::Unknown };
                let started: Option<Value> = serde_json::from_str(&text).ok();
                let id = started.as_ref().and_then(|v| v.pointer("/generation/id")).and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok());
                id.map(|id| StartOutcome::Accepted(GenerationId(id))).unwrap_or(StartOutcome::Unknown)
            }
            Err(ApiError::SignedOut) => StartOutcome::SignedOut,
            Err(ApiError::Http { status, body, .. }) => {
                StartOutcome::Rejected { status, rejection: body.and_then(|b| serde_json::from_value(b).ok()).unwrap_or_default() }
            }
            Err(ApiError::Network(_) | ApiError::Decode(_)) => StartOutcome::Unknown,
        }
    }

    /// Attaches the canonical watch and hands every decoded item to `on_item`
    /// as it arrives. `Ok` once a terminal item was delivered; an attachment that
    /// ends earlier is `Disconnected` (reattach; never restart). Dropping the
    /// future closes the attachment.
    pub async fn watch(
        &self,
        user: Uuid,
        generation: GenerationId,
        kind: WatchKind,
        after: Option<u64>,
        mut on_item: impl FnMut(WatchItem),
    ) -> Result<(), WatchFailure> {
        let mut path = format!("/ai/generations/{}/watch", generation.0);
        if let Some(after) = after {
            path.push_str(&format!("?after={after}"));
        }
        let response = match self.send(user, Method::GET, &path, None).await {
            Ok(response) => response,
            Err(ApiError::Http { status, code, .. }) => return Err(WatchFailure::Http { failure: ApiFailure::http(status, code.as_deref()) }),
            Err(ApiError::SignedOut) => return Err(WatchFailure::Http { failure: ApiFailure::http(401, None) }),
            Err(_) => return Err(WatchFailure::Disconnected),
        };
        let limits = Limits::default();
        let mut text = ApplicationDecoder::new(limits);
        let mut named = SseDecoder::new(limits.sse_bytes);
        let mut terminal = false;
        let mut bytes = response.bytes_stream();
        while let Some(chunk) = bytes.next().await {
            let Ok(chunk) = chunk else { return Err(WatchFailure::Disconnected) };
            let mut offset = 0;
            while offset < chunk.len() {
                let rest = &chunk[offset..];
                let used = match kind {
                    WatchKind::Text => {
                        let (used, frame) = text.push(rest).map_err(|_| WatchFailure::Protocol)?;
                        if let Some(frame) = frame {
                            terminal |= terminal_text(&frame);
                            on_item(WatchItem::Text(frame));
                        }
                        used
                    }
                    WatchKind::Analysis => {
                        let (used, event) = named.push(rest).map_err(|_| WatchFailure::Protocol)?;
                        if let Some(event) = event {
                            let wire = analysis_wire(&event).ok_or(WatchFailure::Protocol)?;
                            terminal |= matches!(wire, AnalysisWire::Complete { .. } | AnalysisWire::Error { .. });
                            on_item(WatchItem::Analysis(wire));
                        }
                        used
                    }
                };
                if used == 0 {
                    return Err(WatchFailure::Protocol);
                }
                offset += used;
            }
            if terminal {
                return Ok(());
            }
        }
        if terminal { Ok(()) } else { Err(WatchFailure::Disconnected) }
    }
}

async fn read_json(response: reqwest::Response) -> Result<Value, ApiError> {
    let text = response.text().await.map_err(|e| ApiError::Network(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| ApiError::Decode(e.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ApiError> {
    serde_json::from_value(value).map_err(|e| ApiError::Decode(e.to_string()))
}

/// `toISOString()` text or null, as `transgate-api::ai::timestamp` writes it.
fn instant(value: Option<&Value>) -> Result<Option<transgate_core::Micros>, ApiError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => parse_rfc3339(text).map(Some).ok_or_else(|| ApiError::Decode(format!("bad timestamp {text}"))),
        Some(_) => Err(ApiError::Decode("timestamp must be a string".into())),
    }
}

/// The inverse of `transgate_api::ai::consent_response`.
pub fn consent_view(v: &Value) -> Result<ConsentView, ApiError> {
    let missing = |key: &str| ApiError::Decode(format!("missing {key}"));
    let flag = |key: &str| v.get(key).and_then(Value::as_bool).ok_or_else(|| missing(key));
    let text = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| missing(key));
    Ok(ConsentView {
        has_consented: flag("hasConsented")?,
        consent_version: text("consentVersion")?,
        consent_date: instant(v.get("consentDate"))?,
        revoked_date: instant(v.get("revokedDate"))?,
        privacy_policy_url: text("privacyPolicyUrl")?,
        provider_name: text("providerName")?,
        can_revoke: flag("canRevoke")?,
        cleanup_pending: flag("cleanupPending")?,
    })
}

/// The inverse of `transgate_api::ai::limits_response`: a cap absent from `limits` is not configured.
pub fn limits_view(v: &Value) -> Result<LimitsView, ApiError> {
    let usage = |name: &str| -> Result<Option<LimitUsage>, ApiError> {
        let Some(u) = v.get("limits").and_then(|l| l.get(name)) else { return Ok(None) };
        let int = |key: &str| u.get(key).and_then(Value::as_i64).ok_or_else(|| ApiError::Decode(format!("missing {name}.{key}")));
        Ok(Some(LimitUsage {
            current: int("current")?,
            max: int("max")?,
            exceeded: u.get("exceeded").and_then(Value::as_bool).ok_or_else(|| ApiError::Decode(format!("missing {name}.exceeded")))?,
            reset_at: instant(u.get("resetDate"))?,
            reset_period: match u.get("resetPeriod") {
                None | Some(Value::Null) => None,
                Some(period) => Some(decode(period.clone())?),
            },
        }))
    };
    Ok(LimitsView {
        enabled: v.get("enabled").and_then(Value::as_bool).ok_or_else(|| ApiError::Decode("missing enabled".into()))?,
        has_limits: v.get("hasLimits").and_then(Value::as_bool).ok_or_else(|| ApiError::Decode("missing hasLimits".into()))?,
        questions_per_transcript: usage("questionsPerTranscript")?,
        questions_per_user: usage("questionsPerUser")?,
        tokens_per_transcript: usage("tokensPerTranscript")?,
        tokens_per_user: usage("tokensPerUser")?,
    })
}

