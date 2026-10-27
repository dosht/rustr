use super::*;
use serde_json::json;
use transgate_core::api::KeyId;
use transgate_core::api::limits::RateLimits;
use transgate_core::jobs::app::{StartAnswer, StartMeta};
use transgate_core::jobs::domain::JobStatus;

fn rate(remaining_minute: u32, remaining_hour: u32) -> ApiRate {
    ApiRate {
        allowed: remaining_minute > 0 && remaining_hour > 0,
        limits: RateLimits::default(),
        remaining_minute,
        remaining_hour,
        reset_minute: Micros(1_767_225_660_000_000),
        reset_hour: Micros(1_767_229_200_000_000),
    }
}

// RTL-003: every response carries the six window headers; only a refusal adds Retry-After, counted
// to the window that is actually exhausted.
#[test]
fn rate_headers_carry_the_windows_and_retry_after_only_on_a_refusal() {
    let now = Micros(1_767_225_630_000_000);
    let ok = rate_headers(&rate(59, 999), now, false);
    let get = |headers: &[(&str, String)], name: &str| headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone());
    assert_eq!(get(&ok, "X-RateLimit-Limit-Minute").as_deref(), Some("60"));
    assert_eq!(get(&ok, "X-RateLimit-Limit-Hour").as_deref(), Some("1000"));
    assert_eq!(get(&ok, "X-RateLimit-Remaining-Minute").as_deref(), Some("59"));
    assert_eq!(get(&ok, "X-RateLimit-Remaining-Hour").as_deref(), Some("999"));
    assert_eq!(get(&ok, "X-RateLimit-Reset-Minute").as_deref(), Some("2026-01-01T00:01:00.000Z"));
    assert_eq!(get(&ok, "X-RateLimit-Reset-Hour").as_deref(), Some("2026-01-01T01:00:00.000Z"));
    assert_eq!(get(&ok, "Retry-After"), None);

    let minute_spent = rate_headers(&rate(0, 500), now, true);
    assert_eq!(get(&minute_spent, "Retry-After").as_deref(), Some("31"), "30 s to the minute reset, rounded up");
    let hour_spent = rate_headers(&rate(10, 0), now, true);
    assert_eq!(get(&hour_spent, "Retry-After").as_deref(), Some("3571"));
}

#[test]
fn a_job_id_is_a_positive_integer_as_parseint_reads_it() {
    assert_eq!(parse_job_id(Some("42")), Ok(JobId(42)));
    assert_eq!(parse_job_id(Some(" 42 ")), Ok(JobId(42)));
    assert_eq!(parse_job_id(Some("42abc")), Ok(JobId(42)), "parseInt reads the leading digits");
    for (raw, message) in [(None, "jobId parameter is required"), (Some(""), "jobId parameter is required"), (Some("abc"), "Invalid jobId parameter"), (Some("0"), "Invalid jobId parameter"), (Some("-3"), "Invalid jobId parameter")] {
        assert_eq!(parse_job_id(raw), Err(message.to_string()), "{raw:?}");
    }
}

#[test]
fn a_new_key_needs_a_name_known_scopes_and_a_known_policy() {
    let ok = parse_new_key(br#"{"name":" CI ","scopes":["transcriptions:read","transcriptions:create"],"expirationPolicy":"30_days"}"#).unwrap();
    assert_eq!((ok.name.as_str(), ok.scopes.len(), ok.expiration_policy, ok.custom_days), ("CI", 2, ExpirationPolicy::Days30, None));
    let defaulted = parse_new_key(br#"{"name":"k","scopes":["transcriptions:read"]}"#).unwrap();
    assert_eq!(defaulted.expiration_policy, ExpirationPolicy::Never);
    let custom = parse_new_key(br#"{"name":"k","scopes":["transcriptions:read"],"expirationPolicy":"custom","customDays":7}"#).unwrap();
    assert_eq!((custom.expiration_policy, custom.custom_days), (ExpirationPolicy::Custom, Some(7)));
    for body in [
        r#"{"scopes":["transcriptions:read"]}"#,
        r#"{"name":"  ","scopes":["transcriptions:read"]}"#,
        r#"{"name":"k"}"#,
        r#"{"name":"k","scopes":[]}"#,
        r#"{"name":"k","scopes":["admin"]}"#,
        r#"{"name":"k","scopes":["transcriptions:*"]}"#,
        r#"{"name":"k","scopes":["transcriptions:read"],"expirationPolicy":"forever"}"#,
        r#"{"name":"k","scopes":["transcriptions:read"],"expirationPolicy":"custom","customDays":"7"}"#,
        "not json",
    ] {
        assert!(parse_new_key(body.as_bytes()).is_err(), "{body}");
    }
}

fn answer() -> StartAnswer {
    StartAnswer {
        results: vec![JobResult { job_id: JobId(7), status: JobStatus::NotStarted, estimated_completion: Some(Micros(0)), error: None }],
        meta: StartMeta {
            new_balance: 10, is_freemium: false, total_jobs: 1, successful_jobs: 1, failed_jobs: 0, queued_jobs: 1,
            estimated_completion: Some(Micros(0)), request_type: "audioUrls".into(),
        },
    }
}

// One DTO feeds the session start answer and the public one: the bodies cannot drift apart.
#[test]
fn the_public_start_body_is_the_session_start_body_with_the_api_client_type() {
    let public = public_start_response(&PublicAnswer { answer: answer(), replayed: false }, "req", Micros(0), 5);
    let session = crate::start_response(&answer(), "req", Micros(0), 5, "api");
    assert_eq!(serde_json::to_value(public).unwrap(), session);
    assert_eq!(session["data"]["results"][0]["jobId"], 7);
    assert_eq!(session["data"]["metadata"]["requestType"], "audioUrls");
    assert_eq!(session["metadata"]["clientType"], "api");
}

#[test]
fn keys_never_expose_secrets_and_name_their_fields_in_camel_case() {
    let view = KeyView {
        id: KeyId(uuid::Uuid::from_u128(1)), name: "k".into(), created_at: Micros(0), expires_at: None, active: true,
        scopes: vec!["transcriptions:read".into()], expiration_policy: ExpirationPolicy::OneYear, hint: Some("sk_AbCdE".into()),
    };
    let body = serde_json::to_value(key_view(&view)).unwrap();
    assert_eq!(
        body,
        json!({"id":"00000000-0000-0000-0000-000000000001","name":"k","createdAt":"1970-01-01T00:00:00.000Z","expiresAt":null,
               "isActive":true,"scopes":["transcriptions:read"],"expirationPolicy":"1_year","hint":"sk_AbCdE"})
    );
}
