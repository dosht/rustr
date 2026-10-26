//! Hosted checkout creation against a loopback fixture. The request shape comes
//! from the public create-checkout documentation; the answers are synthetic.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use billing_core::ports::{Buyer, CheckoutGateway, CheckoutKind, CheckoutStart, GatewayError};
use billing_core::types::AccountId;
use billing_lemonsqueezy::{LemonClient, LemonConfig};
use serde_json::{Value, json};
use uuid::Uuid;

struct Reply(u16, String, Duration);
type RecordedRequest = (String, String, Vec<(String, String)>, Value);

#[derive(Clone, Default)]
struct Fixture {
    seen: Arc<Mutex<Vec<RecordedRequest>>>,
    script: Arc<Mutex<VecDeque<Reply>>>,
}

async fn handle(State(fixture): State<Fixture>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 1 << 20).await.unwrap();
    let headers = parts
        .headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect();
    fixture.seen.lock().unwrap().push((
        parts.method.to_string(),
        parts.uri.path().to_string(),
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    ));
    let reply = fixture.script.lock().unwrap().pop_front();
    match reply {
        Some(Reply(status, body, delay)) => {
            tokio::time::sleep(delay).await;
            (StatusCode::from_u16(status).unwrap(), [("content-type", "application/vnd.api+json")], body).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn serve() -> (Fixture, String) {
    let fixture = Fixture::default();
    let app = Router::new().fallback(handle).with_state(fixture.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (fixture, format!("http://{address}"))
}

fn client(base: &str, timeout: Duration) -> LemonClient {
    LemonClient::new(LemonConfig {
        base_url: base.into(),
        api_key: "key-not-a-credential".into(),
        store_id: "store-1".into(),
        variant_id: "7".into(),
        signing_secret: "secret-not-a-credential".into(),
        environment: "test".into(),
        timeout,
    })
    .unwrap()
}

fn request(amount: &str, kind: CheckoutKind) -> CheckoutStart {
    CheckoutStart {
        reference: "42".into(),
        kind,
        amount: amount.parse().unwrap(),
        currency: "USD".into(),
        product_name: "Pay-as-you-Go".into(),
        pricing_plan_id: None,
        locale: "en".into(),
        callback_url: "https://billing.example.test/checkout/confirm".into(),
        buyer: Buyer { account: AccountId(Uuid::from_u128(9)), email: "ada@example.test".into(), full_name: None },
    }
}

fn created(id: &str) -> Reply {
    Reply(
        201,
        json!({"data": {"type": "checkouts", "id": id, "attributes": {"url": format!("https://pay.example.test/checkout/{id}")}}}).to_string(),
        Duration::ZERO,
    )
}

#[tokio::test]
async fn a_priced_order_creates_a_one_off_hosted_checkout_with_custom_price_and_correlation() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    fixture.script.lock().unwrap().push_back(created("abc-123"));
    let started = gateway.start(&request("14.90", CheckoutKind::PayAsYouGo)).await.unwrap();
    assert_eq!(started.token, "abc-123");
    assert_eq!(started.url.as_deref(), Some("https://pay.example.test/checkout/abc-123"));
    assert!(started.form_content.is_none());

    let (method, path, headers, body) = fixture.seen.lock().unwrap()[0].clone();
    assert_eq!((method.as_str(), path.as_str()), ("POST", "/v1/checkouts"));
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    assert_eq!(header("authorization").as_deref(), Some("Bearer key-not-a-credential"));
    assert_eq!(header("accept").as_deref(), Some("application/vnd.api+json"));
    assert_eq!(header("content-type").as_deref(), Some("application/vnd.api+json"));
    let attributes = &body["data"]["attributes"];
    assert_eq!(body["data"]["type"], "checkouts");
    assert_eq!(attributes["custom_price"], 1490, "positive integer cents");
    assert_eq!(attributes["product_options"]["enabled_variants"], json!([7]), "restricted to the configured variant");
    assert_eq!(attributes["checkout_options"]["discount"], false, "no second discount from the provider");
    assert_eq!(attributes["checkout_data"]["custom"]["payment_id"], "42");
    assert_eq!(attributes["checkout_data"]["custom"]["account_id"], AccountId(Uuid::from_u128(9)).to_string());
    assert_eq!(attributes["checkout_data"]["email"], "ada@example.test");
    assert_eq!(attributes["test_mode"], true);
    assert_eq!(body["data"]["relationships"]["store"]["data"], json!({"type": "stores", "id": "store-1"}));
    assert_eq!(body["data"]["relationships"]["variant"]["data"], json!({"type": "variants", "id": "7"}));
}

#[tokio::test]
async fn amounts_that_are_not_whole_positive_cents_and_plans_never_reach_the_provider() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    for amount in ["14.905", "0.00", "-1.00"] {
        assert!(matches!(gateway.start(&request(amount, CheckoutKind::PayAsYouGo)).await, Err(GatewayError::Rejected { .. })), "{amount}");
    }
    assert!(matches!(gateway.start(&request("14.90", CheckoutKind::Subscription)).await, Err(GatewayError::Rejected { .. })));
    assert!(fixture.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_provider_rejection_is_definite_and_everything_else_is_uncertain() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_millis(300));
    let queue = |reply: Reply| fixture.script.lock().unwrap().push_back(reply);
    queue(Reply(
        422,
        json!({"errors": [{"status": "422", "title": "Unprocessable Entity", "detail": "variant is not a one-off product"}]}).to_string(),
        Duration::ZERO,
    ));
    match gateway.start(&request("14.90", CheckoutKind::PayAsYouGo)).await {
        Err(GatewayError::Rejected { code, message }) => {
            assert_eq!(code, "422");
            assert!(message.contains("one-off"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    for reply in [
        Reply(500, "{}".into(), Duration::ZERO),
        Reply(200, "not json".into(), Duration::ZERO),
        Reply(201, json!({"data": {"attributes": {}}}).to_string(), Duration::ZERO),
        Reply(201, json!({"data": {"id": "x", "attributes": {}}}).to_string(), Duration::ZERO),
        Reply(201, created("late").1, Duration::from_secs(2)),
    ] {
        queue(reply);
        assert!(matches!(gateway.start(&request("14.90", CheckoutKind::PayAsYouGo)).await, Err(GatewayError::Uncertain(_))));
    }
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    assert!(matches!(
        client(&closed, Duration::from_secs(2)).start(&request("14.90", CheckoutKind::PayAsYouGo)).await,
        Err(GatewayError::Uncertain(_))
    ));
}

#[test]
fn an_incomplete_or_ambiguous_configuration_is_refused_not_replaced_by_a_fake() {
    let base = LemonConfig {
        base_url: "https://api.example.test".into(),
        api_key: "k".into(),
        store_id: "1".into(),
        variant_id: "2".into(),
        signing_secret: "s".into(),
        environment: "test".into(),
        timeout: Duration::from_secs(1),
    };
    assert!(LemonClient::new(base.clone()).is_ok());
    assert!(LemonClient::new(LemonConfig { api_key: String::new(), ..base.clone() }).is_err());
    assert!(LemonClient::new(LemonConfig { signing_secret: String::new(), ..base.clone() }).is_err());
    assert!(LemonClient::new(LemonConfig { environment: "staging".into(), ..base }).is_err());
}
