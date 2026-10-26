//! Session AI request shapes and source field names. Admission, validation of
//! transcript/question content, consent, flags and quotas remain core policy.
use serde_json::{Value, json};
use transgate_core::ai::{ConsentView, LimitUsage, LimitsView};
use transgate_core::ai::app::{ChatRequest, HistoryTurn};
use transgate_core::{JobId, Micros};
use uuid::Uuid;

use crate::{Invalid, object, whole_number};

pub struct AnalysisRequest { pub job: JobId, pub request: Option<Uuid> }
pub struct ChatStart { pub job: JobId, pub request: Option<Uuid>, pub chat: ChatRequest }

fn request_key(map: &serde_json::Map<String, Value>) -> Result<Option<Uuid>, Invalid> {
    match map.get("requestId") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Uuid::parse_str(s).map(Some).map_err(|_| "requestId must be a UUID".into()),
        Some(_) => Err("requestId must be a UUID".into()),
    }
}

fn job(map: &serde_json::Map<String, Value>, missing: &str) -> Result<JobId, Invalid> {
    whole_number(map.get("jobId")).filter(|id| *id > 0).map(JobId).ok_or_else(|| missing.into())
}

/// Old callers omit requestId and get a fresh attempt, as before; new callers
/// provide it to replay the same operation without another provider dispatch.
pub fn parse_analysis(body: &[u8]) -> Result<AnalysisRequest, Invalid> {
    let map = object(body)?;
    Ok(AnalysisRequest { job: job(&map, "jobId is required")?, request: request_key(&map)? })
}

pub fn parse_chat(body: &[u8]) -> Result<ChatStart, Invalid> {
    let map = object(body)?;
    let job = job(&map, "Job ID is required")?;
    let message = map.get("message").and_then(Value::as_str).ok_or("Message is required")?.to_string();
    let history = match map.get("history") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(turns)) => turns.iter().map(|turn| {
            let role = turn.get("role").and_then(Value::as_str).ok_or("Invalid chat history format")?;
            let content = turn.get("content").and_then(Value::as_str).ok_or("Invalid chat history format")?;
            Ok(HistoryTurn { role: role.to_string(), content: content.to_string() })
        }).collect::<Result<Vec<_>, Invalid>>()?,
        Some(_) => return Err("Invalid chat history format".into()),
    };
    Ok(ChatStart { job, request: request_key(&map)?, chat: ChatRequest { message, history } })
}

pub fn parse_consent(body: &[u8]) -> Result<bool, Invalid> {
    object(body)?.get("hasConsented").and_then(Value::as_bool).ok_or_else(|| "hasConsented must be a boolean".into())
}

pub fn parse_retry(body: &[u8]) -> Result<Uuid, Invalid> {
    request_key(&object(body)?)?.ok_or_else(|| "requestId must be a UUID".into())
}

fn timestamp(at: Option<Micros>) -> Value {
    at.map(|at| json!(crate::iso8601(at))).unwrap_or(Value::Null)
}

pub fn consent_response(v: &ConsentView) -> Value {
    json!({
        "hasConsented": v.has_consented,
        "consentVersion": v.consent_version,
        "consentDate": timestamp(v.consent_date),
        "revokedDate": timestamp(v.revoked_date),
        "privacyPolicyUrl": v.privacy_policy_url,
        "providerName": v.provider_name,
        "canRevoke": v.can_revoke,
        "cleanupPending": v.cleanup_pending
    })
}

fn usage(v: &LimitUsage) -> Value {
    let mut out = json!({"current":v.current,"max":v.max,"exceeded":v.exceeded});
    if let Some(period) = &v.reset_period {
        out["resetDate"] = timestamp(v.reset_at);
        out["resetPeriod"] = json!(period);
    }
    out
}

pub fn limits_response(v: &LimitsView) -> Value {
    let mut limits = serde_json::Map::new();
    for (name, value) in [
        ("questionsPerTranscript", &v.questions_per_transcript),
        ("questionsPerUser", &v.questions_per_user),
        ("tokensPerTranscript", &v.tokens_per_transcript),
        ("tokensPerUser", &v.tokens_per_user),
    ] {
        if let Some(value) = value { limits.insert(name.to_string(), usage(value)); }
    }
    json!({"enabled":v.enabled,"hasLimits":v.has_limits,"limits":limits})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_identity_or_history_cannot_be_coerced_into_another_request() {
        for value in [json!(null), json!("true"), json!(1), json!({})] {
            assert!(parse_consent(&serde_json::to_vec(&json!({"hasConsented":value})).unwrap()).is_err());
        }
        assert!(!parse_consent(br#"{"hasConsented":false}"#).unwrap());
        assert!(parse_analysis(br#"{"jobId":1,"requestId":"not-a-uuid"}"#).is_err());
        for history in [json!({}), json!([{"role":"user","content":1}]), json!([{"role":1,"content":"hello"}])] {
            assert!(parse_chat(&serde_json::to_vec(&json!({"jobId":1,"message":"hello","history":history})).unwrap()).is_err());
        }
    }

    #[test]
    fn chat_forwards_complete_ordered_input_to_the_single_core_validator() {
        let history: Vec<_> = (0..60).map(|i| json!({"role":if i % 2 == 0 {"user"} else {"assistant"},"content":format!("  turn {i}  ")})).collect();
        let parsed = parse_chat(&serde_json::to_vec(&json!({"jobId":42,"message":"  What happened?  ","history":history})).unwrap()).unwrap();
        assert_eq!(parsed.chat.message, "  What happened?  ");
        assert_eq!(parsed.chat.history.iter().map(|t| t.content.clone()).collect::<Vec<_>>(), (0..60).map(|i| format!("  turn {i}  ")).collect::<Vec<_>>());
        assert!(parsed.request.is_none());
    }
}
