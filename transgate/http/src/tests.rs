use axum::http::HeaderMap;

use super::*;

fn bearer(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, HeaderValue::from_str(value).unwrap());
    headers
}

#[test]
fn cron_accepts_only_the_exact_bearer_secret_and_refuses_everything_when_none_is_set() {
    let secret = SecretString::new("s3cret-value");
    assert!(cron_allowed(Some(&secret), &bearer("Bearer s3cret-value")).is_ok());
    for wrong in ["Bearer s3cret-valuf", "Bearer s3cret-value ", "Bearer", "s3cret-value", "Basic s3cret-value", "Bearer "] {
        let refused = cron_allowed(Some(&secret), &bearer(wrong)).unwrap_err();
        assert_eq!(refused.0, StatusCode::UNAUTHORIZED, "{wrong:?}");
    }
    assert_eq!(cron_allowed(Some(&secret), &HeaderMap::new()).unwrap_err().0, StatusCode::UNAUTHORIZED);
    // The source compared against the literal `Bearer undefined` when the variable was unset.
    assert_eq!(cron_allowed(None, &bearer("Bearer undefined")).unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(cron_allowed(None, &HeaderMap::new()).unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
    let empty = SecretString::new("");
    assert_eq!(cron_allowed(Some(&empty), &bearer("Bearer ")).unwrap_err().0, StatusCode::UNAUTHORIZED, "an empty secret matches nothing");
}

#[test]
fn the_client_address_is_the_first_forwarded_entry_else_the_socket() {
    let peer: SocketAddr = "203.0.113.9:4000".parse().unwrap();
    let mut headers = HeaderMap::new();
    assert_eq!(client_ip(&headers, &peer), "203.0.113.9");
    headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.7, 10.0.0.1"));
    assert_eq!(client_ip(&headers, &peer), "198.51.100.7");
    headers.insert("x-forwarded-for", HeaderValue::from_static("  "));
    assert_eq!(client_ip(&headers, &peer), "203.0.113.9", "an empty header falls back to the socket");
}

#[test]
fn rate_limit_headers_carry_the_reset_only_on_a_refusal() {
    let decision = RateDecision { allowed: false, limit: 10, remaining: 0, reset_at: Micros(12_500_000) };
    let mut denied = StatusCode::TOO_MANY_REQUESTS.into_response();
    rate_headers(&mut denied, &decision, Micros(10_000_000), true);
    assert_eq!(denied.headers()["x-ratelimit-limit"], "10");
    assert_eq!(denied.headers()["x-ratelimit-remaining"], "0");
    assert_eq!(denied.headers()["x-ratelimit-reset"], "3", "whole seconds, rounded up");

    let mut allowed = StatusCode::OK.into_response();
    rate_headers(&mut allowed, &RateDecision { allowed: true, limit: 10, remaining: 9, reset_at: Micros(70_000_000) }, Micros(10_000_000), false);
    assert_eq!(allowed.headers()["x-ratelimit-remaining"], "9");
    assert!(allowed.headers().get("x-ratelimit-reset").is_none());

    let mut past = StatusCode::TOO_MANY_REQUESTS.into_response();
    rate_headers(&mut past, &decision, Micros(99_000_000), true);
    assert_eq!(past.headers()["x-ratelimit-reset"], "0", "a window that already ended never goes negative");
}

#[test]
fn a_download_name_keeps_its_exact_utf8_form_beside_a_safe_ascii_fallback() {
    assert_eq!(content_disposition("talk_transcript.zip"), "attachment; filename=\"talk_transcript.zip\"; filename*=UTF-8''talk_transcript.zip");
    let tricky = content_disposition("Röportaj \"1\"; x%.zip");
    assert!(tricky.contains("filename=\"R_portaj _1_; x_.zip\""), "{tricky}");
    assert!(tricky.contains("filename*=UTF-8''R%C3%B6portaj%20%221%22%3B%20x%25.zip"), "{tricky}");
    assert!(!tricky.contains('\n') && !tricky.contains('\r'));
}
