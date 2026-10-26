//! Request signing and webhook signatures.
//!
//! Both algorithms are pinned by literal known-answer vectors computed
//! independently with Python's stdlib (`docs/plans/p2-billing/signature-vectors.json`),
//! not by this code. They are synthetic inputs, never provider recordings.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

fn mac(key: &[u8]) -> HmacSha256 {
    // HMAC accepts a key of any length.
    HmacSha256::new_from_slice(key).expect("hmac key")
}

pub fn hmac_hex(key: &str, message: &str) -> String {
    let mut mac = mac(key.as_bytes());
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// `Authorization: IYZWSv2 <base64>` where the base64 encodes
/// `apiKey:<key>&randomKey:<nonce>&signature:<hex>` and the signature is
/// HMAC-SHA256(secret, nonce + request path + body). The same bytes that are
/// signed here must be the bytes transmitted. A request without a model (the
/// subscription GETs) signs `{}` exactly like the installed SDK.
pub fn authorization_v2(api_key: &str, secret: &str, nonce: &str, path: &str, body: &str) -> String {
    let signature = hmac_hex(secret, &format!("{nonce}{path}{body}"));
    let plain = format!("apiKey:{api_key}&randomKey:{nonce}&signature:{signature}");
    format!("IYZWSv2 {}", STANDARD.encode(plain))
}

/// The published subscription sample formula:
/// HEX(HMAC-SHA256(secret, merchantId + secret + eventType +
/// subscriptionReferenceCode + orderReferenceCode + customerReferenceCode)).
/// The documentation's prose lists the secret before the merchant id, unlike
/// every published code sample; the code samples are pinned here. It signs
/// identifiers and the event type, never an amount.
pub fn subscription_v3(
    secret: &str,
    merchant_id: &str,
    event_type: &str,
    subscription_ref: &str,
    order_ref: &str,
    customer_ref: &str,
) -> String {
    let message = format!("{merchant_id}{secret}{event_type}{subscription_ref}{order_ref}{customer_ref}");
    hmac_hex(secret, &message)
}

/// Constant-time check of a hex signature (either letter case).
pub fn subscription_v3_matches(
    claimed_hex: &str,
    secret: &str,
    merchant_id: &str,
    event_type: &str,
    subscription_ref: &str,
    order_ref: &str,
    customer_ref: &str,
) -> bool {
    let Ok(claimed) = hex::decode(claimed_hex.trim()) else { return false };
    let message = format!("{merchant_id}{secret}{event_type}{subscription_ref}{order_ref}{customer_ref}");
    let mut mac = mac(secret.as_bytes());
    mac.update(message.as_bytes());
    mac.verify_slice(&claimed).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // From docs/plans/p2-billing/signature-vectors.json. Literal values.
    const SECRET: &str = "test-secret-not-a-credential";

    #[test]
    fn iyzico_subscription_v3_known_answer() {
        let signed = subscription_v3(
            SECRET,
            "merchant-test",
            "subscription.order.success",
            "sub-test",
            "order-test",
            "customer-test",
        );
        assert_eq!(signed, "3a46ec191b74a7e029176525db0f6d1c5389946c70432753442b17aef87e44ea");
        assert!(subscription_v3_matches(
            "3A46EC191B74A7E029176525DB0F6D1C5389946C70432753442B17AEF87E44EA",
            SECRET,
            "merchant-test",
            "subscription.order.success",
            "sub-test",
            "order-test",
            "customer-test"
        ));
    }

    #[test]
    fn iyzico_subscription_v3_rejects_any_changed_identifier_and_the_other_field_order() {
        let good = "3a46ec191b74a7e029176525db0f6d1c5389946c70432753442b17aef87e44ea";
        let check = |event: &str, sub: &str, order: &str, customer: &str| {
            subscription_v3_matches(good, SECRET, "merchant-test", event, sub, order, customer)
        };
        assert!(check("subscription.order.success", "sub-test", "order-test", "customer-test"));
        assert!(!check("subscription.order.failure", "sub-test", "order-test", "customer-test"));
        assert!(!check("subscription.order.success", "sub-other", "order-test", "customer-test"));
        assert!(!check("subscription.order.success", "sub-test", "order-other", "customer-test"));
        assert!(!check("subscription.order.success", "sub-test", "order-test", "customer-other"));
        assert!(!subscription_v3_matches("", SECRET, "merchant-test", "subscription.order.success", "sub-test", "order-test", "customer-test"));
        assert!(!subscription_v3_matches("not hex", SECRET, "merchant-test", "subscription.order.success", "sub-test", "order-test", "customer-test"));
        assert!(!subscription_v3_matches(good, "another-secret", "merchant-test", "subscription.order.success", "sub-test", "order-test", "customer-test"));
        // The prose order (secret before the merchant id) gives a different digest, so it is not accepted.
        let prose = hmac_hex(SECRET, "test-secret-not-a-credentialmerchant-testsubscription.order.successsub-testorder-testcustomer-test");
        assert_ne!(prose, good);
    }

    #[test]
    fn iyzwsv2_get_without_a_model_signs_empty_braces_known_answer() {
        let authorization =
            authorization_v2("api-test", SECRET, "nonce-test", "/v2/subscription/subscriptions/sub-test", "{}");
        assert_eq!(
            authorization,
            "IYZWSv2 YXBpS2V5OmFwaS10ZXN0JnJhbmRvbUtleTpub25jZS10ZXN0JnNpZ25hdHVyZTo5ZDY5MDQ2MmI2Yzc5MjMxOWE3MzJhMDEyOWFiOWUzODIwZTA4MGY1MTQ3OGMzNTk3ODNiZjM2NzkwMGY4NWU2"
        );
        // An empty body would sign a different string and must not match the SDK behavior.
        assert_ne!(
            authorization,
            authorization_v2("api-test", SECRET, "nonce-test", "/v2/subscription/subscriptions/sub-test", "")
        );
    }
}
