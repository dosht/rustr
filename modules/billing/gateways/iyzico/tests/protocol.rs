//! The production Iyzico adapter against a loopback fixture server. The server
//! recomputes the IYZWSv2 signature from the bytes it received, so a request
//! signed over different bytes than were sent fails here. Response bodies are
//! documentation- and source-derived, not recordings of real transactions
//! (see fixtures/PROVENANCE.md).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use billing_core::ports::{
    Buyer, CheckoutGateway, CheckoutKind, CheckoutResult, CheckoutStart, GatewayError, SubscriptionGateway,
};
use billing_core::types::AccountId;
use billing_iyzico::sign::hmac_hex;
use billing_iyzico::{IyzicoClient, IyzicoConfig, segment};
use serde_json::{Value, json};
use uuid::Uuid;

const API_KEY: &str = "api-test";
const SECRET: &str = "test-secret-not-a-credential";

struct Reply {
    status: u16,
    body: String,
    delay: Duration,
}

fn ok(body: Value) -> Reply {
    Reply { status: 200, body: body.to_string(), delay: Duration::ZERO }
}

#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    path: String,
    body: Vec<u8>,
    signature_ok: bool,
}

#[derive(Clone, Default)]
struct Fixture {
    recorded: Arc<Mutex<Vec<Recorded>>>,
    script: Arc<Mutex<VecDeque<Reply>>>,
}

impl Fixture {
    fn then(&self, reply: Reply) {
        self.script.lock().unwrap().push_back(reply);
    }

    fn last(&self) -> Recorded {
        self.recorded.lock().unwrap().last().cloned().expect("a request was recorded")
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.last().body).unwrap()
    }
}

fn signature_matches(request: &axum::http::request::Parts, path: &str, body: &[u8]) -> bool {
    let Some(authorization) = request.headers.get("authorization").and_then(|v| v.to_str().ok()) else { return false };
    let Some(encoded) = authorization.strip_prefix("IYZWSv2 ") else { return false };
    let Ok(plain) = STANDARD.decode(encoded) else { return false };
    let plain = String::from_utf8_lossy(&plain).to_string();
    let fields: Vec<&str> = plain.split('&').collect();
    let get = |prefix: &str| fields.iter().find_map(|f| f.strip_prefix(prefix)).map(str::to_string);
    let (Some(key), Some(nonce), Some(signature)) = (get("apiKey:"), get("randomKey:"), get("signature:")) else {
        return false;
    };
    let header_nonce = request.headers.get("x-iyzi-rnd").and_then(|v| v.to_str().ok()).unwrap_or_default();
    // A GET without a model is signed over `{}`, a POST over the bytes it sent.
    let signed = if body.is_empty() { "{}".to_string() } else { String::from_utf8_lossy(body).to_string() };
    key == API_KEY && nonce == header_nonce && signature == hmac_hex(SECRET, &format!("{nonce}{path}{signed}"))
}

async fn handle(State(fixture): State<Fixture>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 1 << 20).await.unwrap().to_vec();
    let path = parts.uri.path().to_string();
    fixture.recorded.lock().unwrap().push(Recorded {
        method: parts.method.to_string(),
        path: path.clone(),
        body: bytes.clone(),
        signature_ok: signature_matches(&parts, &path, &bytes),
    });
    let reply = fixture.script.lock().unwrap().pop_front();
    match reply {
        Some(reply) => {
            tokio::time::sleep(reply.delay).await;
            (StatusCode::from_u16(reply.status).unwrap(), [("content-type", "application/json")], reply.body).into_response()
        }
        None => (StatusCode::NOT_FOUND, "nothing scripted").into_response(),
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

fn client(base_url: &str, timeout: Duration) -> IyzicoClient {
    IyzicoClient::new(IyzicoConfig {
        base_url: base_url.into(),
        api_key: API_KEY.into(),
        secret_key: SECRET.into(),
        environment: "sandbox".into(),
        timeout,
    })
    .unwrap()
}

fn buyer() -> Buyer {
    Buyer { account: AccountId(Uuid::from_u128(0xabc)), email: "ada.lovelace@example.test".into(), full_name: Some("Ada Lovelace".into()) }
}

fn start(kind: CheckoutKind) -> CheckoutStart {
    CheckoutStart {
        reference: "conversation-1".into(),
        kind,
        amount: "14.90".parse().unwrap(),
        currency: "USD".into(),
        product_name: "Pay-as-you-Go".into(),
        pricing_plan_id: matches!(kind, CheckoutKind::Subscription).then(|| "plan-code-1".to_string()),
        locale: "tr".into(),
        callback_url: "https://billing.example.test/checkout/confirm".into(),
        buyer: buyer(),
    }
}

#[tokio::test]
async fn bil_070_071_pay_as_you_go_start_and_result_use_the_captured_fields_and_a_valid_signature() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    fixture.then(ok(json!({"status": "success", "token": "tok-1", "checkoutFormContent": "<script>x</script>", "tokenExpireTime": 1800})));
    let started = gateway.start(&start(CheckoutKind::PayAsYouGo)).await.unwrap();
    assert_eq!(started.token, "tok-1");
    assert_eq!(started.form_content.as_deref(), Some("<script>x</script>"));

    let recorded = fixture.last();
    assert_eq!((recorded.method.as_str(), recorded.path.as_str()), ("POST", "/payment/iyzipos/checkoutform/initialize/auth/ecom"));
    assert!(recorded.signature_ok, "the signed bytes are the transmitted bytes");
    let body = fixture.json();
    assert_eq!(body["price"], "14.9");
    assert_eq!(body["paidPrice"], "14.9");
    assert_eq!(body["currency"], "USD");
    assert_eq!(body["locale"], "tr");
    assert_eq!(body["conversationId"], "conversation-1");
    assert_eq!(body["basketId"], "B67832");
    assert_eq!(body["paymentGroup"], "PRODUCT");
    assert_eq!(body["enabledInstallments"], json!([2, 3, 6, 9]));
    assert_eq!(body["callbackUrl"], "https://billing.example.test/checkout/confirm");
    assert_eq!(body["buyer"]["name"], "Ada");
    assert_eq!(body["buyer"]["surname"], "Lovelace");
    assert_eq!(body["buyer"]["identityNumber"], "11111111111");
    assert_eq!(body["buyer"]["email"], "ada.lovelace@example.test");
    assert_eq!(body["basketItems"][0]["category1"], "Pay-as-you-Go");
    assert_eq!(body["basketItems"][0]["itemType"], "VIRTUAL");
    assert_eq!(body["basketItems"][0]["price"], "14.9");

    // Result lookup: English, the legacy fixed conversation id, the token.
    fixture.then(ok(json!({"status": "success", "paymentStatus": "SUCCESS", "paidPrice": "14.9", "currency": "USD", "paymentId": 8123})));
    let result = gateway.result("tok-1", CheckoutKind::PayAsYouGo).await.unwrap();
    let CheckoutResult::Success { observed_amount, observed_currency, provider_payment_id, .. } = result else { panic!("{result:?}") };
    assert_eq!(observed_amount.map(|a| a.to_string()).as_deref(), Some("14.9"));
    assert_eq!((observed_currency.as_deref(), provider_payment_id.as_deref()), (Some("USD"), Some("8123")));
    let recorded = fixture.last();
    assert_eq!(recorded.path, "/payment/iyzipos/checkoutform/auth/ecom/detail");
    assert!(recorded.signature_ok);
    assert_eq!(fixture.json(), json!({"locale": "en", "conversationId": "123456789", "token": "tok-1"}));
}

#[tokio::test]
async fn bil_060_only_success_and_failure_are_final_payment_statuses() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    for status in ["SUCCESS", "success", "Success"] {
        fixture.then(ok(json!({"status": "success", "paymentStatus": status, "token": "t"})));
        assert!(matches!(gateway.result("t", CheckoutKind::PayAsYouGo).await.unwrap(), CheckoutResult::Success { .. }));
    }
    for status in ["FAILURE", "failure", "Failure"] {
        fixture.then(ok(json!({"status": "success", "paymentStatus": status, "token": "t"})));
        assert_eq!(
            gateway.result("t", CheckoutKind::PayAsYouGo).await.unwrap(),
            CheckoutResult::Failure { raw_status: status.into() }
        );
    }
    // CF-Retrieve and retrieve-payment document intermediate 3DS and wallet states.
    for status in ["INIT_THREEDS", "CALLBACK_THREEDS", "BKM_POS_SELECTED", "CALLBACK_PECCO", "NEW_UNKNOWN_STATE"] {
        fixture.then(ok(json!({"status": "success", "paymentStatus": status})));
        assert!(matches!(
            gateway.result("t", CheckoutKind::PayAsYouGo).await,
            Err(GatewayError::Uncertain(message)) if message.contains(status)
        ), "{status}");
    }
}

#[tokio::test]
async fn def_005_lookup_error_envelopes_never_establish_payment_failure() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    // Synthetic codes are deliberately not a guessed list of payment declines.
    for kind in [CheckoutKind::PayAsYouGo, CheckoutKind::Subscription] {
        for (code, message) in [("1000", "token expired"), ("auth", "invalid credentials"), ("system", "unavailable"), ("2", "declined")] {
            fixture.then(ok(json!({"status": "failure", "errorCode": code, "errorMessage": message})));
            assert!(matches!(gateway.result("t", kind).await, Err(GatewayError::Uncertain(_))));
        }
    }
}

#[tokio::test]
async fn a_returned_token_must_match_before_any_payment_outcome_is_accepted() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    for token in [json!("another-token"), json!(""), json!(null), json!(42)] {
        for status in ["SUCCESS", "FAILURE"] {
            fixture.then(ok(json!({"status": "success", "paymentStatus": status, "token": token})));
            assert!(matches!(gateway.result("t", CheckoutKind::PayAsYouGo).await, Err(GatewayError::Uncertain(_))));
        }
    }
}

#[tokio::test]
async fn provider_prices_follow_sdk_decimal_format_without_changing_the_quote() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    for (quote, wire) in [("14.90", "14.9"), ("616.00", "616.0"), ("393.36", "393.36")] {
        let mut request = start(CheckoutKind::PayAsYouGo);
        request.amount = quote.parse().unwrap();
        fixture.then(ok(json!({"status": "success", "token": "t"})));
        gateway.start(&request).await.unwrap();
        let body = fixture.json();
        for amount in [&body["price"], &body["paidPrice"], &body["basketItems"][0]["price"]] {
            assert_eq!(amount, wire);
            assert_eq!(amount.as_str().unwrap().parse::<billing_core::decimal::Decimal>().unwrap(), request.amount);
        }
        assert!(fixture.last().signature_ok);
    }
}

#[tokio::test]
async fn def_005_malformed_answers_server_errors_timeouts_and_refused_connections_are_uncertain_never_failures() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_millis(300));
    // Not JSON.
    fixture.then(Reply { status: 200, body: "<html>bad gateway</html>".into(), delay: Duration::ZERO });
    assert!(matches!(gateway.result("t", CheckoutKind::PayAsYouGo).await, Err(GatewayError::Uncertain(_))));
    // HTTP 500, even with a JSON body.
    fixture.then(Reply { status: 500, body: json!({"status": "failure"}).to_string(), delay: Duration::ZERO });
    assert!(matches!(gateway.result("t", CheckoutKind::PayAsYouGo).await, Err(GatewayError::Uncertain(_))));
    // Success envelope without a payment status.
    fixture.then(ok(json!({"status": "success"})));
    assert!(matches!(gateway.result("t", CheckoutKind::PayAsYouGo).await, Err(GatewayError::Uncertain(_))));
    // No status at all.
    fixture.then(ok(json!({"unexpected": true})));
    assert!(matches!(gateway.result("t", CheckoutKind::PayAsYouGo).await, Err(GatewayError::Uncertain(_))));
    // Slower than the client's timeout.
    fixture.then(Reply { status: 200, body: json!({"status": "success", "paymentStatus": "SUCCESS"}).to_string(), delay: Duration::from_secs(2) });
    assert!(matches!(gateway.result("t", CheckoutKind::PayAsYouGo).await, Err(GatewayError::Uncertain(_))));
    // Nothing listening.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    assert!(matches!(
        client(&closed, Duration::from_secs(2)).result("t", CheckoutKind::PayAsYouGo).await,
        Err(GatewayError::Uncertain(_))
    ));
    // A failed initialization is a rejection that creates nothing.
    let (fixture, base) = serve().await;
    fixture.then(ok(json!({"status": "failure", "errorCode": "5", "errorMessage": "invalid price", "errorGroup": "G"})));
    assert_eq!(
        client(&base, Duration::from_secs(5)).start(&start(CheckoutKind::PayAsYouGo)).await.unwrap_err(),
        GatewayError::Rejected { code: "5".into(), message: "invalid price".into() }
    );
}

#[tokio::test]
async fn bil_069_subscription_start_sends_the_plan_code_active_status_locale_and_unique_conversation_id() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    fixture.then(ok(json!({"status": "success", "data": {"token": "sub-tok", "checkoutFormContent": "<form/>"}})));
    let mut request = start(CheckoutKind::Subscription);
    request.locale = "id".into();
    let started = gateway.start(&request).await.unwrap();
    assert_eq!(started.token, "sub-tok");
    let recorded = fixture.last();
    assert_eq!((recorded.method.as_str(), recorded.path.as_str()), ("POST", "/v2/subscription/checkoutform/initialize"));
    assert!(recorded.signature_ok);
    let body = fixture.json();
    assert_eq!(body["pricingPlanReferenceCode"], "plan-code-1");
    assert_eq!(body["subscriptionInitialStatus"], "ACTIVE");
    assert_eq!(body["locale"], "en", "anything but tr is English");
    assert_eq!(body["conversationId"], "conversation-1");
    assert_eq!(body["customer"]["identityNumber"], "11111111111");
    assert_eq!(body["customer"]["billingAddress"]["district"], "altunizade");
    // A plan checkout without a plan code never reaches the provider.
    let mut missing = start(CheckoutKind::Subscription);
    missing.pricing_plan_id = None;
    let before = fixture.recorded.lock().unwrap().len();
    assert!(matches!(gateway.start(&missing).await, Err(GatewayError::Rejected { .. })));
    assert_eq!(fixture.recorded.lock().unwrap().len(), before);
}

#[tokio::test]
async fn def_007_the_subscription_form_result_yields_the_subscription_reference_and_a_get_signs_empty_braces() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    fixture.then(ok(json!({
        "status": "success",
        "data": {"referenceCode": "sub-ref-1", "customerReferenceCode": "cust-1", "parentReferenceCode": "x"}
    })));
    let result = gateway.result("form tok/1", CheckoutKind::Subscription).await.unwrap();
    let CheckoutResult::Success { customer_ref, subscription_ref, .. } = result else { panic!("{result:?}") };
    assert_eq!((customer_ref.as_deref(), subscription_ref.as_deref()), (Some("cust-1"), Some("sub-ref-1")));
    let recorded = fixture.last();
    assert_eq!(recorded.method, "GET");
    assert_eq!(recorded.path, "/v2/subscription/checkoutform/form%20tok%2F1", "the token stays one path segment");
    assert!(recorded.body.is_empty() && recorded.signature_ok, "no body is sent and `{{}}` is what is signed");

    // A success envelope with no subscription reference cannot be bound.
    fixture.then(ok(json!({"status": "success", "data": {"customerReferenceCode": "cust-1"}})));
    assert!(matches!(gateway.result("t", CheckoutKind::Subscription).await, Err(GatewayError::Uncertain(_))));
    // An error envelope does not prove a subscription payment decline.
    fixture.then(ok(json!({"status": "failure", "errorCode": "2", "errorMessage": "declined"})));
    assert!(matches!(gateway.result("t", CheckoutKind::Subscription).await, Err(GatewayError::Uncertain(_))));
}

#[tokio::test]
async fn d4_subscription_detail_accepts_only_a_uniquely_matched_documented_or_source_derived_shape() {
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    let subscription = |reference: &str| {
        json!({
            "referenceCode": reference, "customerReferenceCode": "cust-1", "pricingPlanReferenceCode": "plan-1",
            "subscriptionStatus": "ACTIVE",
            "orders": [
                {"referenceCode": "ord-b", "orderStatus": "SUCCESS", "startPeriod": 2},
                {"referenceCode": "ord-a", "orderStatus": "SUCCESS", "startPeriod": 1}
            ]
        })
    };
    // Documented shape: data.items[] with exactly the requested subscription. Order of orders is not interpreted.
    fixture.then(ok(json!({"status": "success", "data": {"totalCount": 1, "items": [subscription("sub-1")]}})));
    let detail = gateway.detail("sub-1").await.unwrap();
    assert_eq!(detail.order_refs, vec!["ord-b", "ord-a"], "listed as given, never reordered or picked");
    assert_eq!(detail.customer_ref.as_deref(), Some("cust-1"));
    let recorded = fixture.last();
    assert_eq!((recorded.method.as_str(), recorded.path.as_str()), ("GET", "/v2/subscription/subscriptions/sub-1"));
    assert!(recorded.signature_ok);

    // Source-derived shape: data is the subscription.
    fixture.then(ok(json!({"status": "success", "data": subscription("sub-1")})));
    assert_eq!(gateway.detail("sub-1").await.unwrap().order_refs.len(), 2);

    // Anything else is "cannot verify".
    for ambiguous in [
        json!({"status": "success", "data": {"items": []}}),
        json!({"status": "success", "data": {"items": [subscription("sub-1"), subscription("sub-1")]}}),
        json!({"status": "success", "data": {"items": [subscription("sub-other")]}}),
        json!({"status": "success", "data": subscription("sub-other")}),
        json!({"status": "success", "data": {"referenceCode": "sub-1", "items": [subscription("sub-1")]}}),
        json!({"status": "success", "data": {"items": [{"referenceCode": "sub-1", "orders": "nope"}]}}),
        json!({"status": "success"}),
    ] {
        fixture.then(ok(ambiguous.clone()));
        assert!(matches!(gateway.detail("sub-1").await, Err(GatewayError::Uncertain(_))), "{ambiguous}");
    }
}

#[tokio::test]
async fn a_path_identifier_cannot_change_the_request_uri() {
    assert_eq!(segment("sub-1_a.b~c"), "sub-1_a.b~c");
    assert_eq!(segment("../x?y#z/w"), "..%2Fx%3Fy%23z%2Fw");
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    fixture.then(ok(json!({"status": "success", "data": {"referenceCode": "a/b?c"}})));
    let _ = gateway.detail("a/b?c").await;
    let recorded = fixture.last();
    assert_eq!(recorded.path, "/v2/subscription/subscriptions/a%2Fb%3Fc");
    assert!(recorded.signature_ok);
}

#[test]
fn an_incomplete_configuration_is_refused_not_replaced_by_a_fake() {
    let missing = IyzicoClient::new(IyzicoConfig {
        base_url: "https://example.test".into(),
        api_key: String::new(),
        secret_key: "x".into(),
        environment: "sandbox".into(),
        timeout: Duration::from_secs(1),
    });
    assert!(missing.is_err());
}

#[tokio::test]
async fn d10_erasure_cancel_is_signed_bound_and_distinct_from_manual_support_cancel() {
    use billing_core::erasure::{SubscriptionTermination, TerminationEvidence, TerminationTarget};
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    let target = TerminationTarget {
        gateway: "iyzico".into(), environment: "sandbox".into(),
        subscription_ref: "sub/a?b".into(), customer_ref: Some("customer-a".into()),
    };
    fixture.then(ok(json!({"status":"success"})));
    assert_eq!(gateway.terminate(&target).await.unwrap(), TerminationEvidence::CancelSucceeded);
    let recorded = fixture.last();
    assert_eq!((recorded.method.as_str(), recorded.path.as_str()),
        ("POST", "/v2/subscription/subscriptions/sub%2Fa%3Fb/cancel"));
    assert_eq!(recorded.body, b"{}");
    assert!(recorded.signature_ok);
    let wrong = TerminationTarget { environment: "live".into(), ..target };
    assert!(matches!(gateway.terminate(&wrong).await, Err(GatewayError::Uncertain(_))));
    assert_eq!(fixture.recorded.lock().unwrap().len(), 1, "wrong environment never reaches the provider");
}

#[tokio::test]
async fn d10_cancel_failure_reports_authenticated_detail_without_inventing_terminal_customer_binding() {
    use billing_core::erasure::{SubscriptionTermination, TerminationEvidence, TerminationTarget, termination_confirmed};
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_secs(5));
    let target = TerminationTarget {
        gateway: "iyzico".into(), environment: "sandbox".into(),
        subscription_ref: "sub-1".into(), customer_ref: Some("customer-a".into()),
    };
    for (status, customer, confirmed) in [
        ("CANCELED", "customer-a", true), ("EXPIRED", "customer-a", true),
        ("ACTIVE", "customer-a", false), ("CANCELED", "another-customer", false),
    ] {
        fixture.then(ok(json!({"status":"failure","errorCode":"already_processed"})));
        fixture.then(ok(json!({"status":"success","data":{"referenceCode":"sub-1",
            "customerReferenceCode":customer,"subscriptionStatus":status}})));
        let evidence = gateway.terminate(&target).await.unwrap();
        assert_eq!(evidence, TerminationEvidence::Detail {
            subscription_ref:"sub-1".into(), customer_ref:Some(customer.into()), status:Some(status.into()),
        });
        assert_eq!(termination_confirmed(&target, &evidence), confirmed);
        assert_eq!(fixture.last().method, "GET");
        assert!(fixture.last().signature_ok);
    }
    fixture.then(ok(json!({"status":"failure"})));
    fixture.then(ok(json!({"status":"success","data":{"items":[
        {"referenceCode":"sub-1","subscriptionStatus":"CANCELED"},
        {"referenceCode":"sub-1","subscriptionStatus":"EXPIRED"}]}})));
    assert!(matches!(gateway.terminate(&target).await, Err(GatewayError::Uncertain(_))));
}

#[tokio::test]
async fn d10_lost_malformed_and_contradictory_cancel_answers_stay_uncertain_without_hidden_retry() {
    use billing_core::erasure::{SubscriptionTermination, TerminationTarget};
    let (fixture, base) = serve().await;
    let gateway = client(&base, Duration::from_millis(50));
    let target = TerminationTarget {
        gateway:"iyzico".into(), environment:"sandbox".into(),
        subscription_ref:"sub-1".into(), customer_ref:Some("customer-a".into()),
    };
    for reply in [
        Reply { status:200, body:"not json".into(), delay:Duration::ZERO },
        Reply { status:500, body:json!({"status":"success"}).to_string(), delay:Duration::ZERO },
        Reply { status:401, body:json!({"status":"success"}).to_string(), delay:Duration::ZERO },
        Reply { status:200, body:json!({"status":"success"}).to_string(), delay:Duration::from_millis(250) },
    ] {
        fixture.then(reply);
        assert!(matches!(gateway.terminate(&target).await, Err(GatewayError::Uncertain(_))));
    }
    assert_eq!(fixture.recorded.lock().unwrap().len(), 4);
    assert!(fixture.recorded.lock().unwrap().iter().all(|request| request.method == "POST"));
}
