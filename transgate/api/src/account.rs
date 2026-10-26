//! Signed-in account requests. Identity comes from authentication, never JSON.

use serde_json::{Map, Value, json};
use transgate_core::account::{FeedbackRating, SupportId};
use transgate_core::JobId;

use super::{Invalid, object, whole_number};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedbackRequest {
    pub rating: FeedbackRating,
    pub comment: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupportRequest {
    pub job: Option<JobId>,
    pub message: String,
}

fn fields(map: &Map<String, Value>, allowed: &[&str]) -> Result<(), Invalid> {
    if map.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("The request contains an unsupported field".into());
    }
    Ok(())
}

pub fn parse_feedback(body: &[u8]) -> Result<FeedbackRequest, Invalid> {
    let mut map = object(body)?;
    fields(&map, &["rating", "feedback"])?;
    let rating = whole_number(map.get("rating"))
        .and_then(|n| i16::try_from(n).ok())
        .ok_or("rating must be a number")?;
    let rating = FeedbackRating::try_from(rating)?;
    let comment = match map.remove("feedback") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text,
        Some(_) => return Err("feedback must be a string".into()),
    };
    Ok(FeedbackRequest { rating, comment })
}

pub fn parse_support(body: &[u8]) -> Result<SupportRequest, Invalid> {
    let mut map = object(body)?;
    fields(&map, &["message", "jobId"])?;
    let message = match map.remove("message") {
        Some(Value::String(text)) => text,
        _ => return Err("message must be a string".into()),
    };
    let job = match map.get("jobId") {
        None | Some(Value::Null) => None,
        Some(value) => Some(JobId(whole_number(Some(value)).ok_or("jobId must be an integer")?)),
    };
    Ok(SupportRequest { job, message })
}

pub fn support_accepted(id: SupportId) -> Value {
    json!({"request_id": id, "status": "queued"})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_a_real_rating_and_json_numeric_spelling_does_not_change_it() {
        assert_eq!(parse_feedback(br#"{"rating":0}"#).unwrap().rating, FeedbackRating::Disappointed);
        assert_eq!(parse_feedback(br#"{"rating":5.0}"#).unwrap().rating, FeedbackRating::Satisfied);
        for body in [br#"{"rating":false}"#.as_slice(), br#"{"rating":"5"}"#, br#"{"rating":5.5}"#, br#"{"rating":1}"#, br#"{"rating":null}"#] {
            assert!(parse_feedback(body).is_err(), "{}", String::from_utf8_lossy(body));
        }
    }

    #[test]
    fn client_supplied_identity_cannot_cross_either_account_request_boundary() {
        assert!(parse_feedback(br#"{"rating":10,"user_id":"another-account"}"#).is_err());
        assert!(parse_support(br#"{"message":"help","email":"victim@example.test"}"#).is_err());
    }

    #[test]
    fn support_context_is_optional_but_a_malformed_id_is_not_silently_dropped() {
        assert_eq!(parse_support(br#"{"message":"help","jobId":null}"#).unwrap().job, None);
        assert_eq!(parse_support(br#"{"message":"help","jobId":42}"#).unwrap().job, Some(JobId(42)));
        assert!(parse_support(br#"{"message":"help","jobId":"42"}"#).is_err());
        assert!(parse_support(br#"{"message":"help","jobId":1.5}"#).is_err());
        assert!(parse_support(br#"{"jobId":42}"#).is_err());
    }
}
