//! The PostHog capture adapter against a local fixture server. The expected event ids
//! were calculated independently (Python `uuid.uuid5` over the source's namespace and
//! `JSON.stringify([event, kind, id])`), not by this code. No PostHog account or host
//! was contacted.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use billing_core::calendar::utc;
use billing_core::notify::AnalyticsEvent;
use billing_core::ports::AnalyticsSink;
use serde_json::{Map, Value, json};
use transgate_billing::analytics::{PostHog, capture_body};

fn event(name: &'static str, kind: &str, id: &str, timestamp: bool) -> AnalyticsEvent {
    let mut properties = Map::new();
    properties.insert("payment_id".into(), json!(42));
    AnalyticsEvent {
        name,
        distinct_id: "11111111-2222-4333-8444-555555555555".into(),
        timestamp: timestamp.then(|| utc(2026, 1, 15, 12, 30, 45, 120_000)),
        dedup_name: json!([name, kind, id]).to_string(),
        properties,
    }
}

#[test]
fn event_ids_are_the_sources_uuid_v5_over_the_stable_event_name() {
    let id = |e: &AnalyticsEvent| capture_body("key", e)["uuid"].as_str().unwrap().to_string();
    assert_eq!(id(&event("payment_completed", "payments", "42", true)), "22cd3fb0-670e-588a-9330-19bf31d8ce57");
    assert_eq!(id(&event("subscription_started", "payments", "42", true)), "9d546fea-e3ef-5fae-b0d9-cded7d614e99");
    assert_eq!(
        id(&event("user_signed_up", "users", "11111111-2222-4333-8444-555555555555", false)),
        "751f7406-cef8-54ee-8e22-f1e04785b868"
    );
}

#[test]
fn the_body_carries_the_documented_fields_suppresses_geoip_and_omits_an_unknown_timestamp() {
    let body = capture_body("phc_key", &event("payment_completed", "payments", "42", true));
    assert_eq!(body["api_key"], "phc_key");
    assert_eq!(body["event"], "payment_completed");
    assert_eq!(body["distinct_id"], "11111111-2222-4333-8444-555555555555");
    assert_eq!(body["timestamp"], "2026-01-15T12:30:45.120000Z");
    assert_eq!(body["properties"], json!({"payment_id": 42, "$geoip_disable": true}));
    let signup = capture_body("phc_key", &event("user_signed_up", "users", "x", false));
    assert!(signup.get("timestamp").is_none(), "the source sends no timestamp for a signup");
    assert_eq!(signup["properties"]["$geoip_disable"], true);
}

#[derive(Clone, Default)]
struct Fixture {
    seen: Arc<Mutex<Vec<(String, Value)>>>,
    status: Arc<Mutex<u16>>,
}

async fn handler(State(f): State<Fixture>, request: Request) -> Response {
    let path = request.uri().path().to_string();
    let bytes = to_bytes(request.into_body(), 1 << 20).await.unwrap();
    f.seen.lock().unwrap().push((path, serde_json::from_slice(&bytes).unwrap_or(Value::Null)));
    let status = *f.status.lock().unwrap();
    (StatusCode::from_u16(status).unwrap(), Json(json!({"status": 1}))).into_response()
}

use axum::Json;

async fn serve(f: &Fixture) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new().fallback(handler).with_state(f.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{address}/")
}

#[tokio::test]
async fn an_event_is_posted_to_the_capture_endpoint_and_a_refusal_is_an_error_without_the_key() {
    let fixture = Fixture::default();
    *fixture.status.lock().unwrap() = 200;
    let host = serve(&fixture).await;
    let sink = PostHog::new(&host, "phc_secret_key", Duration::from_secs(3)).unwrap();
    let sent = event("payment_completed", "payments", "42", true);
    sink.send(&sent, "event:analytics:payment_completed").await.unwrap();
    {
        let seen = fixture.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "/i/v0/e/", "a trailing slash on the host does not double it");
        assert_eq!(seen[0].1, capture_body("phc_secret_key", &sent));
    }

    *fixture.status.lock().unwrap() = 503;
    let error = sink.send(&sent, "k").await.unwrap_err();
    assert!(error.0.contains("503"), "{error}");
    assert!(!error.0.contains("phc_secret_key"));

    // An unreachable host is also an error the relay retries; the URL is not echoed.
    let down = PostHog::new("http://127.0.0.1:9", "phc_secret_key", Duration::from_millis(500)).unwrap();
    let error = down.send(&sent, "k").await.unwrap_err();
    assert!(!error.0.contains("127.0.0.1:9"), "{error}");
    assert!(PostHog::new(" ", "key", Duration::from_secs(1)).is_err());
    assert!(PostHog::new("http://h", " ", Duration::from_secs(1)).is_err());
}
