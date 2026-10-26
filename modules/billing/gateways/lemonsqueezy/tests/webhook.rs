//! `X-Signature` verification over raw bytes, pinned by the independently
//! computed literal vector (docs/plans/p2-billing/signature-vectors.json, which
//! includes a non-ASCII character so byte handling matters), and the order
//! payload mapping. Payloads are shaped from the public documentation; none is
//! a recording of a real store.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use billing_core::ports::{Headers, OrderWebhook, WebhookError};
use billing_lemonsqueezy::{LemonClient, LemonConfig, LemonWebhook, signature_hex};
use serde_json::{Value, json};

const SECRET: &str = "test-secret-not-a-credential";

fn verifier_at(base_url: &str, timeout: Duration) -> LemonWebhook {
    LemonClient::new(LemonConfig {
        base_url: base_url.into(),
        api_key: "fixture-api-key".into(),
        store_id: "1".into(),
        variant_id: "7".into(),
        signing_secret: SECRET.into(),
        environment: "test".into(),
        timeout,
    }).unwrap().webhook()
}

fn verifier() -> LemonWebhook {
    verifier_at("http://127.0.0.1:1", Duration::from_secs(1))
}

fn signed(body: &[u8]) -> Headers {
    vec![("X-Signature".into(), signature_hex(SECRET, body))]
}

fn order(overrides: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut payload = json!({
        "meta": {
            "event_name": "order_created",
            "custom_data": {"payment_id": "42", "account_id": "11111111-2222-3333-4444-555555555555"}
        },
        "data": {
            "type": "orders",
            "id": "5001",
            "attributes": {
                "store_id": 1,
                "currency": "USD",
                "subtotal": 1490,
                "discount_total": 0,
                "tax": 298,
                "total": 1788,
                "tax_inclusive": false,
                "status": "paid",
                "test_mode": true,
                "urls": {"receipt": "https://app.lemonsqueezy.com/my-orders/abc"},
                "first_order_item": {"id": 8001, "order_id": 5001, "variant_id": 7, "price": 1490, "quantity": 1}
            }
        }
    });
    overrides(&mut payload);
    serde_json::to_vec(&payload).unwrap()
}

#[tokio::test]
async fn lemonsqueezy_raw_utf8_known_answer() {
    let raw = "{\"meta\":{\"event_name\":\"order_created\"},\"data\":{\"id\":\"order-test\",\"note\":\"İstanbul\"}}";
    assert_eq!(
        signature_hex(SECRET, raw.as_bytes()),
        "f80ff34cee70d14288b8f68922991f374332363de7734927cdc2dfafdcff3dc7"
    );
    // The signature passes: the failure is the payload shape (no order attributes), not authenticity.
    let headers: Headers = vec![(
        "x-signature".into(),
        "f80ff34cee70d14288b8f68922991f374332363de7734927cdc2dfafdcff3dc7".into(),
    )];
    match verifier().verify(&headers, raw.as_bytes()).await {
        Err(WebhookError::Rejected(message)) => assert!(message.contains("unreadable order"), "{message}"),
        other => panic!("{other:?}"),
    }
    // One changed byte of the raw body (a different Unicode normalization) fails the signature.
    let altered = raw.replace('İ', "I\u{307}");
    match verifier().verify(&headers, altered.as_bytes()).await {
        Err(WebhookError::Rejected(message)) => assert_eq!(message, "signature mismatch"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_correctly_signed_paid_order_maps_every_trusted_field_and_separates_the_money() {
    let raw = order(|_| {});
    let n = verifier().verify(&signed(&raw), &raw).await.unwrap();
    assert_eq!((n.event_name.as_str(), n.order_id.as_str(), n.status.as_str()), ("order_created", "5001", "paid"));
    assert_eq!((n.store_id.as_str(), n.variant_id.as_str(), n.quantity), ("1", "7", 1));
    assert_eq!((n.subtotal_cents, n.discount_cents, n.tax_cents, n.total_cents), (1490, 0, 298, 1788));
    assert!(!n.tax_inclusive && n.test_mode);
    assert_eq!(n.currency, "USD");
    assert_eq!(n.payment_id, Some(42));
    assert_eq!(n.account_id.as_deref(), Some("11111111-2222-3333-4444-555555555555"));
    assert_eq!(n.receipt_url.as_deref(), Some("https://app.lemonsqueezy.com/my-orders/abc"));
}

#[tokio::test]
async fn missing_malformed_or_wrong_signatures_are_rejected_without_reading_the_payload() {
    let (fixture, base, server) = serve().await;
    let verifier = verifier_at(&base, Duration::from_secs(1));
    let raw = without_quantity(|_| {});
    let mut reformatted = raw.clone();
    reformatted.push(b' ');
    for (headers, body) in [
        (vec![], raw.clone()),
        (vec![("x-signature".into(), "".into())], raw.clone()),
        (vec![("x-signature".into(), "zz".into())], raw.clone()),
        (signed(&raw), reformatted),
        (vec![("x-signature".into(), signature_hex("another-secret", &raw))], raw),
        (vec![("x-signature".into(), "00".repeat(32))], b"{broken".to_vec()),
    ] {
        assert!(matches!(
            verifier.verify(&headers, &body).await,
            Err(WebhookError::Rejected(m)) if m == "signature mismatch" || m.starts_with("missing")
        ));
    }
    assert!(fixture.recorded.lock().unwrap().is_empty(), "invalid signatures must never cause outbound requests");
    server.abort();
}

#[tokio::test]
async fn events_other_than_order_created_are_acknowledged_and_ignored_and_unreadable_orders_are_not_swallowed() {
    let refund = order(|p| p["meta"]["event_name"] = json!("order_refunded"));
    assert!(matches!(verifier().verify(&signed(&refund), &refund).await, Err(WebhookError::Unsupported(_))));
    let subscription = order(|p| p["meta"]["event_name"] = json!("subscription_created"));
    assert!(matches!(verifier().verify(&signed(&subscription), &subscription).await, Err(WebhookError::Unsupported(_))));
    let no_total = order(|p| {
        p["data"]["attributes"].as_object_mut().unwrap().remove("total");
    });
    assert!(matches!(verifier().verify(&signed(&no_total), &no_total).await, Err(WebhookError::Rejected(_))));
    let not_json = b"{broken".to_vec();
    assert!(matches!(verifier().verify(&signed(&not_json), &not_json).await, Err(WebhookError::Rejected(_))));
}


fn without_quantity(overrides: impl FnOnce(&mut Value)) -> Vec<u8> {
    order(|payload| {
        payload["data"]["attributes"]["first_order_item"].as_object_mut().unwrap().remove("quantity");
        overrides(payload);
    })
}

fn item() -> Value {
    json!({"data": {
        "type": "order-items", "id": "8001",
        "attributes": {"order_id": 5001, "variant_id": 7, "price": 1490, "quantity": 1}
    }})
}

struct Reply {
    status: u16,
    body: String,
    delay: Duration,
}

#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    path: String,
    authorization: String,
    accept: String,
}

#[derive(Clone, Default)]
struct Fixture {
    recorded: Arc<Mutex<Vec<Recorded>>>,
    replies: Arc<Mutex<VecDeque<Reply>>>,
}

impl Fixture {
    fn then(&self, status: u16, body: String, delay: Duration) {
        self.replies.lock().unwrap().push_back(Reply { status, body, delay });
    }

    fn item(&self, value: Value) {
        self.then(200, value.to_string(), Duration::ZERO);
    }
}

async fn handle(State(fixture): State<Fixture>, request: Request) -> Response {
    let reply = {
        let (parts, _) = request.into_parts();
        let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        fixture.recorded.lock().unwrap().push(Recorded {
            method: parts.method.to_string(),
            path: parts.uri.to_string(),
            authorization: header("authorization"),
            accept: header("accept"),
        });
        let mut replies = fixture.replies.lock().unwrap();
        replies.pop_front()
    };
    match reply {
        Some(reply) => {
            tokio::time::sleep(reply.delay).await;
            (StatusCode::from_u16(reply.status).unwrap(), [("content-type", "application/vnd.api+json")], reply.body).into_response()
        }
        None => (StatusCode::NOT_FOUND, "no scripted item").into_response(),
    }
}

async fn serve() -> (Fixture, String, tokio::task::JoinHandle<()>) {
    let fixture = Fixture::default();
    let app = Router::new().fallback(handle).with_state(fixture.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (fixture, base, server)
}

#[tokio::test]
async fn documented_order_without_quantity_retrieves_and_binds_the_order_item() {
    let (fixture, base, server) = serve().await;
    let verifier = verifier_at(&base, Duration::from_secs(1));
    fixture.item(item());
    let raw = without_quantity(|_| {});
    let notification = verifier.verify(&signed(&raw), &raw).await.unwrap();
    assert_eq!((notification.order_id.as_str(), notification.variant_id.as_str(), notification.quantity), ("5001", "7", 1));
    let recorded = fixture.recorded.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1);
    assert_eq!((recorded[0].method.as_str(), recorded[0].path.as_str()), ("GET", "/v1/order-items/8001"));
    assert_eq!(recorded[0].authorization, "Bearer fixture-api-key");
    assert_eq!(recorded[0].accept, "application/vnd.api+json");
    // Preserve a real multi-item quantity for core's one-item policy; never default to one.
    let mut answer = item();
    answer["data"]["attributes"]["quantity"] = json!(2);
    fixture.item(answer);
    let raw = without_quantity(|p| p["data"]["attributes"]["subtotal"] = json!(2980));
    assert_eq!(verifier.verify(&signed(&raw), &raw).await.unwrap().quantity, 2);
    server.abort();
}

#[tokio::test]
async fn retrieved_item_must_match_signed_identity_variant_price_and_subtotal() {
    let (fixture, base, server) = serve().await;
    let verifier = verifier_at(&base, Duration::from_secs(1));
    let raw = without_quantity(|_| {});
    for (pointer, value) in [
        ("/data/type", json!("variants")),
        ("/data/id", json!("8002")),
        ("/data/attributes/order_id", json!(5002)),
        ("/data/attributes/variant_id", json!(8)),
        ("/data/attributes/price", json!(1491)),
        ("/data/attributes/quantity", json!(0)),
        ("/data/attributes/quantity", json!(-1)),
        ("/data/attributes/quantity", json!(1.5)),
        ("/data/attributes/quantity", json!("1")),
        ("/data/attributes/quantity", Value::Null),
        ("/data/attributes/quantity", json!(2)),
        ("/data/attributes/quantity", json!(i64::MAX)),
    ] {
        let mut answer = item();
        *answer.pointer_mut(pointer).unwrap() = value;
        fixture.item(answer);
        assert!(matches!(verifier.verify(&signed(&raw), &raw).await, Err(WebhookError::Unavailable(_))), "{pointer}");
    }
    let mut missing = item();
    missing["data"]["attributes"].as_object_mut().unwrap().remove("quantity");
    fixture.item(missing);
    assert!(matches!(verifier.verify(&signed(&raw), &raw).await, Err(WebhookError::Unavailable(_))));
    // A response matching the signed item price still cannot contradict the signed subtotal.
    fixture.item(item());
    let mismatched = without_quantity(|p| p["data"]["attributes"]["subtotal"] = json!(1491));
    assert!(matches!(verifier.verify(&signed(&mismatched), &mismatched).await, Err(WebhookError::Unavailable(_))));
    server.abort();
}

#[tokio::test]
async fn unavailable_order_item_lookup_is_retryable_and_a_later_delivery_can_succeed() {
    let (fixture, base, server) = serve().await;
    let verifier = verifier_at(&base, Duration::from_millis(300));
    let raw = without_quantity(|_| {});
    for status in [401, 404, 429, 500] {
        fixture.then(status, json!({"errors": [{"title": "unavailable"}]}).to_string(), Duration::ZERO);
        assert!(matches!(verifier.verify(&signed(&raw), &raw).await, Err(WebhookError::Unavailable(_))));
    }
    for body in ["not json", "{}", r#"{"data":{"type":"order-items","id":"8001"}}"#] {
        fixture.then(200, body.into(), Duration::ZERO);
        assert!(matches!(verifier.verify(&signed(&raw), &raw).await, Err(WebhookError::Unavailable(_))));
    }
    fixture.then(200, item().to_string(), Duration::from_secs(2));
    assert!(matches!(verifier.verify(&signed(&raw), &raw).await, Err(WebhookError::Unavailable(_))));
    fixture.item(item());
    assert_eq!(verifier.verify(&signed(&raw), &raw).await.unwrap().quantity, 1);
    server.abort();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    assert!(matches!(
        verifier_at(&closed, Duration::from_secs(1)).verify(&signed(&raw), &raw).await,
        Err(WebhookError::Unavailable(_))
    ));
}

#[tokio::test]
async fn present_quantity_is_validated_without_lookup_and_never_silently_replaced() {
    let (fixture, base, server) = serve().await;
    let verifier = verifier_at(&base, Duration::from_secs(1));
    for quantity in [json!(0), json!(-1), json!(1.5), json!("1"), Value::Null] {
        let raw = order(|p| p["data"]["attributes"]["first_order_item"]["quantity"] = quantity);
        assert!(matches!(verifier.verify(&signed(&raw), &raw).await, Err(WebhookError::Rejected(_))));
    }
    for quantity in [1, 2] {
        let raw = order(|p| p["data"]["attributes"]["first_order_item"]["quantity"] = json!(quantity));
        assert_eq!(verifier.verify(&signed(&raw), &raw).await.unwrap().quantity, quantity);
    }
    assert!(fixture.recorded.lock().unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn missing_or_malformed_money_fields_are_rejected_before_item_lookup() {
    let (fixture, base, server) = serve().await;
    let verifier = verifier_at(&base, Duration::from_secs(1));
    for field in ["discount_total", "tax", "tax_inclusive"] {
        for malformed in [None, Some(Value::Null), Some(json!("not-a-value"))] {
            let raw = without_quantity(|p| {
                let attrs = p["data"]["attributes"].as_object_mut().unwrap();
                match malformed {
                    None => { attrs.remove(field); }
                    Some(value) => { attrs.insert(field.into(), value); }
                }
            });
            assert!(matches!(verifier.verify(&signed(&raw), &raw).await, Err(WebhookError::Rejected(_))), "{field}");
        }
    }
    assert!(fixture.recorded.lock().unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn lookup_requires_signed_item_identity_and_the_configured_sale() {
    let (fixture, base, server) = serve().await;
    let verifier = verifier_at(&base, Duration::from_secs(1));
    for (pointer, value) in [
        ("/data/type", json!("checkouts")),
        ("/data/attributes/store_id", json!(2)),
        ("/data/attributes/test_mode", json!(false)),
        ("/data/attributes/first_order_item/id", json!("../../orders")),
        ("/data/attributes/first_order_item/order_id", json!(5002)),
        ("/data/attributes/first_order_item/variant_id", json!(8)),
        ("/data/attributes/first_order_item/price", json!(0)),
    ] {
        let raw = without_quantity(|p| *p.pointer_mut(pointer).unwrap() = value);
        assert!(matches!(verifier.verify(&signed(&raw), &raw).await, Err(WebhookError::Rejected(_))), "{pointer}");
    }
    assert!(fixture.recorded.lock().unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn primary_document_tax_examples_preserve_inclusive_and_exclusive_amounts() {
    let (fixture, base, server) = serve().await;
    let verifier = verifier_at(&base, Duration::from_secs(1));
    for (subtotal, tax, total, inclusive) in [(745, 124, 745, true), (1500, 300, 1800, false)] {
        let raw = without_quantity(|p| {
            let attrs = &mut p["data"]["attributes"];
            attrs["subtotal"] = json!(subtotal);
            attrs["tax"] = json!(tax);
            attrs["total"] = json!(total);
            attrs["tax_inclusive"] = json!(inclusive);
            attrs["first_order_item"]["price"] = json!(subtotal);
        });
        let mut answer = item();
        answer["data"]["attributes"]["price"] = json!(subtotal);
        fixture.item(answer);
        let n = verifier.verify(&signed(&raw), &raw).await.unwrap();
        assert_eq!((n.subtotal_cents, n.discount_cents, n.tax_cents, n.total_cents, n.tax_inclusive),
            (subtotal, 0, tax, total, inclusive));
    }
    server.abort();
}
