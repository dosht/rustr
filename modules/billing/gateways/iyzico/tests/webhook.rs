//! V3 subscription notification verification, end to end over a raw body and
//! header, with the independently computed literal signature from
//! docs/plans/p2-billing/signature-vectors.json. Synthetic inputs, no provider
//! recording and no claim of live acceptance.

use billing_core::ports::{Headers, SubscriptionOutcome, SubscriptionWebhook, WebhookError};
use billing_iyzico::IyzicoWebhook;
use billing_iyzico::sign::subscription_v3;
use serde_json::json;

const SECRET: &str = "test-secret-not-a-credential";
const VECTOR: &str = "3a46ec191b74a7e029176525db0f6d1c5389946c70432753442b17aef87e44ea";

fn verifier() -> IyzicoWebhook {
    IyzicoWebhook::new("merchant-test", SECRET, "sandbox").unwrap()
}

fn body(event: &str, sub: &str, order: &str, customer: &str) -> Vec<u8> {
    json!({
        "iyziEventType": event,
        "subscriptionReferenceCode": sub,
        "orderReferenceCode": order,
        "customerReferenceCode": customer,
        "iyziReferenceCode": "iyzi-ref",
        "iyziEventTime": 1_700_000_000_000i64,
        // A claimed amount in the body is never read: the signature does not cover it.
        "price": "0.01",
    })
    .to_string()
    .into_bytes()
}

fn signed(signature: &str) -> Headers {
    vec![("x-iyz-signature-v3".into(), signature.into())]
}

#[test]
fn def_003_a_correctly_signed_success_is_accepted_against_the_literal_vector() {
    let raw = body("subscription.order.success", "sub-test", "order-test", "customer-test");
    let n = verifier().verify(&signed(VECTOR), &raw).unwrap();
    assert_eq!(n.outcome, SubscriptionOutcome::Success);
    assert_eq!((n.subscription_ref.as_str(), n.order_ref.as_str(), n.customer_ref.as_str()), ("sub-test", "order-test", "customer-test"));
}

#[test]
fn def_003_missing_wrong_or_tampered_signatures_are_rejected_before_anything_is_read() {
    let raw = body("subscription.order.success", "sub-test", "order-test", "customer-test");
    let rejected = |headers: Headers, raw: &[u8]| matches!(verifier().verify(&headers, raw), Err(WebhookError::Rejected(_)));
    assert!(rejected(vec![], &raw), "no header: fail closed, whatever the deployment's activation state");
    assert!(rejected(signed(""), &raw));
    assert!(rejected(signed("not hex at all"), &raw));
    assert!(rejected(signed(&VECTOR.replace('3', "4")), &raw));
    // Valid signature, but for another identifier set.
    assert!(rejected(signed(VECTOR), &body("subscription.order.success", "sub-test", "order-FORGED", "customer-test")));
    assert!(rejected(signed(VECTOR), &body("subscription.order.success", "sub-other", "order-test", "customer-test")));
    assert!(rejected(signed(VECTOR), &body("subscription.order.success", "sub-test", "order-test", "customer-other")));
    // Another merchant's verifier does not accept it either.
    let other = IyzicoWebhook::new("merchant-other", SECRET, "sandbox").unwrap();
    assert!(matches!(other.verify(&signed(VECTOR), &raw), Err(WebhookError::Rejected(_))));
    // Malformed bodies and missing identifiers.
    assert!(rejected(signed(VECTOR), b"not json"));
    assert!(rejected(signed(VECTOR), br#"{"iyziEventType":"subscription.order.success"}"#));
    assert!(rejected(signed(VECTOR), &body("subscription.order.success", "", "order-test", "customer-test")));
}

#[test]
fn def_003_the_documented_failure_name_and_the_source_name_are_both_accepted_explicitly() {
    for event in ["subscription.order.failure", "subscription.order.failed"] {
        let signature = subscription_v3(SECRET, "merchant-test", event, "sub-test", "order-test", "customer-test");
        let n = verifier().verify(&signed(&signature), &body(event, "sub-test", "order-test", "customer-test")).unwrap();
        assert_eq!(n.outcome, SubscriptionOutcome::Failure, "{event}");
        assert_eq!(n.raw_event_type, event);
    }
}

#[test]
fn an_authentic_event_of_another_type_is_acknowledged_and_ignored_not_treated_as_a_failure() {
    let event = "subscription.order.refunded";
    let signature = subscription_v3(SECRET, "merchant-test", event, "sub-test", "order-test", "customer-test");
    assert!(matches!(
        verifier().verify(&signed(&signature), &body(event, "sub-test", "order-test", "customer-test")),
        Err(WebhookError::Unsupported(_))
    ));
}

#[test]
fn a_verifier_without_a_merchant_id_or_secret_is_refused_at_construction() {
    assert!(IyzicoWebhook::new("", SECRET, "sandbox").is_err());
    assert!(IyzicoWebhook::new("merchant-test", "", "sandbox").is_err());
}
