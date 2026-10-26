//! D17 checkout document POST: the two form fields p1R's `POST /checkout`
//! accepts (billing.md section 6). The browser submits them as one hidden
//! same-origin form (target `_self`); the access token travels only in the
//! form body, never in a URL, log or view. Limits are shared with the server so an
//! oversized order is refused here instead of being sent and rejected.
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use billing_core::presentation::purchase::{
    CHECKOUT_FORM_MAX_BYTES, CHECKOUT_ORDER_MAX_BYTES, CHECKOUT_TOKEN_MAX_BYTES, OrderBody, PurchaseFailure,
};
use serde_json::{Map, Value};

pub const CHECKOUT_PATH: &str = "/checkout";

/// The encoded fields, in the names the server reads.
#[derive(Clone, PartialEq, Eq)]
pub struct CheckoutForm {
    /// base64url without padding (RFC 4648 section 5) of the `OrderBody` JSON.
    pub order: String,
    pub access_token: String,
}

// Never print the token.
impl std::fmt::Debug for CheckoutForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckoutForm").field("order", &self.order).field("access_token", &"<redacted>").finish()
    }
}

/// Sets the attribution object on the order (the purchase leaf leaves it unset)
/// and encodes both fields within the server's limits. A refusal means nothing is
/// sent; the caller reports it as `CheckoutNotSubmitted`.
pub fn checkout_form(order: &OrderBody, attribution: Map<String, Value>, access_token: &str) -> Result<CheckoutForm, PurchaseFailure> {
    let token_ok = !access_token.is_empty() && access_token.len() <= CHECKOUT_TOKEN_MAX_BYTES && access_token.bytes().all(|b| b.is_ascii_graphic());
    if !token_ok {
        return Err(PurchaseFailure::Unauthenticated);
    }
    let mut order = order.clone();
    order.analytics = Some(Value::Object(attribution));
    let json = serde_json::to_vec(&order).map_err(|_| PurchaseFailure::Refused { code: "invalid_request".into() })?;
    if json.len() > CHECKOUT_ORDER_MAX_BYTES {
        return Err(PurchaseFailure::Refused { code: "payload_too_large".into() });
    }
    let form = CheckoutForm { order: URL_SAFE_NO_PAD.encode(&json), access_token: access_token.to_string() };
    if encoded_len(&form) > CHECKOUT_FORM_MAX_BYTES {
        return Err(PurchaseFailure::Refused { code: "payload_too_large".into() });
    }
    Ok(form)
}

/// The `application/x-www-form-urlencoded` body length the browser will send.
pub fn encoded_len(form: &CheckoutForm) -> usize {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("order", &form.order)
        .append_pair("access_token", &form.access_token)
        .finish()
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(coupon: &str) -> OrderBody {
        OrderBody {
            package_id: None,
            pay_as_you_go_hours: Some(10),
            currency: "TRY".into(),
            locale: "tr".into(),
            coupon_id: Some(coupon.into()),
            gateway: "iyzico".into(),
            analytics: None,
        }
    }

    fn attribution() -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("utm_campaign".into(), Value::String("güz-kampanyası".into()));
        map.insert("device_type".into(), Value::String("mobile".into()));
        map
    }

    #[test]
    fn the_order_round_trips_as_unpadded_base64url_with_unicode_kept_inside_the_json() {
        let form = checkout_form(&order("İNDİRİM"), attribution(), "header.payload.signature").unwrap();
        assert!(form.order.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'), "RFC 4648 section 5 alphabet, no padding");
        let decoded: OrderBody = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&form.order).unwrap()).unwrap();
        assert_eq!(decoded.coupon_id.as_deref(), Some("İNDİRİM"));
        assert_eq!(decoded.analytics.unwrap()["utm_campaign"], "güz-kampanyası");
        assert_eq!(form.access_token, "header.payload.signature");
    }

    #[test]
    fn a_missing_or_malformed_token_is_not_submitted() {
        for token in ["", "with space", "line\nbreak", &"x".repeat(CHECKOUT_TOKEN_MAX_BYTES + 1)] {
            assert_eq!(checkout_form(&order("A"), attribution(), token), Err(PurchaseFailure::Unauthenticated), "{token:.20}");
        }
    }

    #[test]
    fn an_order_over_the_server_limit_is_refused_before_sending() {
        let huge = "x".repeat(CHECKOUT_ORDER_MAX_BYTES);
        assert_eq!(checkout_form(&order(&huge), attribution(), "t"), Err(PurchaseFailure::Refused { code: "payload_too_large".into() }));
    }

    #[test]
    fn the_token_never_appears_in_debug_output() {
        let form = checkout_form(&order("A"), attribution(), "secret.jwt.value").unwrap();
        assert!(!format!("{form:?}").contains("secret.jwt.value"));
    }
}
