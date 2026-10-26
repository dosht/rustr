//! LemonSqueezy hosted checkout and paid-order handling against a real database.
//! The raw-byte signature check lives in billing-lemonsqueezy; here the verifier
//! is scripted and the trusted-quote validation is under test.

mod support;

use std::sync::Arc;

use billing_core::app::{BillingError, NotificationOutcome, OrderRequest};
use billing_core::grants::PaymentStatus;
use billing_core::model::reasons;
use billing_core::ports::{CheckoutStarted, GatewayError, OrderNotification, WebhookError};
use billing_core::types::AccountId;
use serde_json::json;
use support::*;

struct Shop {
    f: Fixture,
    gateway: Arc<ScriptedGateway>,
    hook: Arc<ScriptedOrderWebhook>,
    account: AccountId,
}

async fn shop() -> Shop {
    let gateway = ScriptedGateway::new("lemonsqueezy", "test");
    let hook = ScriptedOrderWebhook::new("test");
    let h = hook.clone();
    let f = fixture_with(vec![gateway.clone()], move |b| b.with_order_webhook(h)).await;
    let account = provisioned(&f, &unique_email()).await;
    Shop { f, gateway, hook, account }
}

fn lemon_order(hours: i64) -> OrderRequest {
    OrderRequest {
        pay_as_you_go_hours: Some(hours),
        ui_locale: "en".into(),
        gateway: "lemonsqueezy".into(),
        analytics: json!({}),
        ..OrderRequest::default()
    }
}

fn hosted(url: &str) -> CheckoutStarted {
    CheckoutStarted { token: format!("co-{}", uuid::Uuid::new_v4().simple()), form_content: None, url: Some(url.into()) }
}

impl Shop {
    /// A pending 10 hour payment (USD 14.90, 1490 cents) with its hosted checkout.
    async fn pending(&self) -> i64 {
        self.gateway.will_start(Ok(hosted("https://pay.example.test/checkout/abc")));
        self.f.billing.start_checkout(self.account, &lemon_order(10)).await.unwrap().payment_id
    }

    async fn deliver(&self, order: OrderNotification) -> Result<NotificationOutcome, BillingError> {
        self.hook.deliver(order);
        self.f.billing.process_order_webhook(&vec![], b"{}").await
    }

    async fn status(&self, payment_id: i64) -> String {
        sqlx::query_scalar("SELECT status FROM billing.payments WHERE id = $1")
            .bind(payment_id)
            .fetch_one(&self.f.pool)
            .await
            .unwrap()
    }

    async fn packages(&self) -> i64 {
        count(&self.f.pool, "SELECT count(*) FROM billing.packages WHERE account_id = $1 AND payment_id IS NOT NULL", self.account).await
    }
}

fn order_id() -> String {
    format!("order-{}", uuid::Uuid::new_v4().simple())
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn the_quote_is_persisted_before_the_hosted_checkout_is_created_and_correlates_by_payment_id() {
    let s = shop().await;
    let payment_id = s.pending().await;
    {
        let sent = s.gateway.started.lock().unwrap();
        assert_eq!(sent[0].reference, payment_id.to_string(), "custom_data correlates by our payment id");
        assert_eq!(sent[0].amount.to_string(), "14.90");
        assert_eq!(sent[0].currency, "USD");
    }
    assert_eq!(s.status(payment_id).await, "pending");
    let (token, env): (Option<String>, String) =
        sqlx::query_as("SELECT token, environment FROM billing.payments WHERE id = $1")
            .bind(payment_id)
            .fetch_one(&s.f.pool)
            .await
            .unwrap();
    assert!(token.is_some_and(|t| t.starts_with("co-")), "the hosted checkout id is stored");
    assert_eq!(env, "test");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn a_definite_creation_failure_records_failed_and_an_uncertain_one_does_not_claim_a_rejection() {
    let s = shop().await;
    s.gateway.will_start(Err(GatewayError::Rejected { code: "422".into(), message: "bad variant".into() }));
    assert!(matches!(s.f.billing.start_checkout(s.account, &lemon_order(10)).await, Err(BillingError::Gateway(_))));
    let failed = count(&s.f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1 AND status = 'failed'", s.account).await;
    assert_eq!(failed, 1);
    assert_eq!(count(&s.f.pool, "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'PaymentFailed'", s.account).await, 1);

    s.gateway.will_start(Err(GatewayError::Uncertain("timeout".into())));
    assert!(matches!(s.f.billing.start_checkout(s.account, &lemon_order(10)).await, Err(BillingError::Uncertain(_))));
    let pending = count(&s.f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1 AND status = 'pending'", s.account).await;
    assert_eq!(pending, 1, "an unknown outcome is not a provider rejection");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn the_gateway_only_sells_pay_as_you_go_hours_in_usd() {
    let s = shop().await;
    let plan = OrderRequest { package_id: Some(0), ui_locale: "en".into(), gateway: "lemonsqueezy".into(), ..OrderRequest::default() };
    assert!(matches!(s.f.billing.start_checkout(s.account, &plan).await, Err(BillingError::InvalidRequest(_))));
    let try_order = OrderRequest { currency: Some("TRY".into()), ..lemon_order(10) };
    assert!(matches!(s.f.billing.start_checkout(s.account, &try_order).await, Err(BillingError::InvalidRequest(_))));
    assert!(s.gateway.started.lock().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn a_verified_paid_order_completes_the_payment_once_and_keeps_provider_amounts_apart_from_the_quote() {
    let s = shop().await;
    let payment_id = s.pending().await;
    let id = order_id();
    // Tax exclusive: 14.90 plus 20 percent tax. The pre-tax item amount is the quote.
    let mut taxed = paid_order(payment_id, s.account, &id, 1490);
    taxed.tax_cents = 298;
    taxed.total_cents = 1788;
    let outcome = s.deliver(taxed.clone()).await.unwrap();
    assert!(matches!(outcome, NotificationOutcome::Applied { status: PaymentStatus::Completed, changed: true, .. }), "{outcome:?}");
    assert_eq!(s.packages().await, 1);
    let (amount, order, total, receipt): (String, String, String, Option<String>) = sqlx::query_as(
        "SELECT amount::text, provider_order_ref, observed->>'total_cents', receipt_url FROM billing.payments WHERE id = $1",
    )
    .bind(payment_id)
    .fetch_one(&s.f.pool)
    .await
    .unwrap();
    assert_eq!((amount.as_str(), order.as_str(), total.as_str()), ("14.90", id.as_str(), "1788"));
    assert_eq!(receipt.as_deref(), Some("https://receipts.example.test/r/1"), "the receipt link is kept distinct");

    // A repeat delivery changes nothing.
    let again = s.deliver(taxed).await.unwrap();
    assert!(matches!(again, NotificationOutcome::Ignored { .. }), "{again:?}");
    assert_eq!(s.packages().await, 1);
    assert_eq!(count(&s.f.pool, "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'PaymentCompleted'", s.account).await, 1);

    // Tax inclusive (documented sample: subtotal 15.00, tax 2.50, total 15.00): the item
    // price is the subtotal in both tax modes, so the quote still matches it.
    let second = s.pending().await;
    let mut inclusive = paid_order(second, s.account, &order_id(), 1490);
    inclusive.tax_cents = 248;
    inclusive.total_cents = 1490;
    inclusive.tax_inclusive = true;
    assert!(matches!(s.deliver(inclusive).await.unwrap(), NotificationOutcome::Applied { changed: true, .. }));
    assert_eq!(s.packages().await, 2);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn every_forged_or_mismatched_order_is_held_with_its_reason_and_grants_nothing() {
    let s = shop().await;
    let payment_id = s.pending().await;
    let good = |id: &str| paid_order(payment_id, s.account, id, 1490);
    type Mutation = fn(&mut OrderNotification);
    let cases: Vec<(&str, Mutation)> = vec![
        (reasons::ACCOUNT_MISMATCH, |o| o.account_id = Some(account().to_string())),
        (reasons::ACCOUNT_MISMATCH, |o| o.account_id = None),
        (reasons::ENVIRONMENT_MISMATCH, |o| o.test_mode = false),
        (reasons::STORE_MISMATCH, |o| o.store_id = "someone-elses-store".into()),
        (reasons::VARIANT_MISMATCH, |o| o.variant_id = "a-cheaper-variant".into()),
        (reasons::QUANTITY_MISMATCH, |o| o.quantity = 2),
        (reasons::CURRENCY_MISMATCH, |o| o.currency = "EUR".into()),
        (reasons::DISCOUNT_PRESENT, |o| o.discount_cents = 100),
        (reasons::AMOUNT_MISMATCH, |o| o.subtotal_cents = 1000),
        (reasons::AMOUNT_MISMATCH, |o| {
            // A consistent inclusive order whose item subtotal is not the quote: the total is never the comparison.
            o.subtotal_cents = 1192;
            o.tax_cents = 298;
            o.total_cents = 1192;
            o.tax_inclusive = true;
        }),
        // Exclusive: the total must be subtotal plus tax, so an untaxed total with tax present is refused.
        (reasons::AMOUNT_MISMATCH, |o| o.tax_cents = 298),
        // Inclusive: the total must equal the subtotal, so a total that adds the tax is refused.
        (reasons::AMOUNT_MISMATCH, |o| {
            o.tax_cents = 248;
            o.total_cents = 1738;
            o.tax_inclusive = true;
        }),
        (reasons::AMOUNT_INVALID, |o| o.tax_cents = -5),
        (reasons::AMOUNT_INVALID, |o| o.subtotal_cents = -1490),
        (reasons::PAYMENT_UNKNOWN, |o| o.payment_id = Some(i64::MAX - 7)),
        (reasons::PAYMENT_UNKNOWN, |o| o.payment_id = None),
    ];
    for (reason, mutate) in cases {
        let mut forged = good(&order_id());
        mutate(&mut forged);
        let outcome = s.deliver(forged).await.unwrap();
        assert!(
            matches!(&outcome, NotificationOutcome::Held { reason: r, .. } if r == reason),
            "expected {reason}, got {outcome:?}"
        );
        assert_eq!(s.status(payment_id).await, "pending");
        assert_eq!(s.packages().await, 0);
    }
    // A missing or invalid signature answers 401 and stores nothing.
    *s.hook.next.lock().unwrap() = Some(Err(WebhookError::Rejected("signature mismatch".into())));
    let rejected = s.f.billing.process_order_webhook(&vec![], b"{}").await.unwrap_err();
    assert_eq!(rejected.http_status(), 401);
    // The honest order still completes afterwards.
    assert!(matches!(s.deliver(good(&order_id())).await.unwrap(), NotificationOutcome::Applied { changed: true, .. }));
    assert_eq!(s.packages().await, 1);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn a_conflicting_second_order_for_a_completed_payment_and_a_reused_order_id_are_held_not_granted() {
    let s = shop().await;
    let first = s.pending().await;
    let order = order_id();
    s.deliver(paid_order(first, s.account, &order, 1490)).await.unwrap();
    assert_eq!(s.packages().await, 1);

    // A different order id naming the already-completed payment.
    let conflict = s.deliver(paid_order(first, s.account, &order_id(), 1490)).await.unwrap();
    assert!(matches!(&conflict, NotificationOutcome::Held { reason, .. } if reason == reasons::CONFLICTING_ORDER), "{conflict:?}");

    // The completed order's id reused for another pending payment: the unique identity refuses the grant.
    let second = s.pending().await;
    let reused = s.deliver(paid_order(second, s.account, &order, 1490)).await.unwrap();
    assert!(matches!(&reused, NotificationOutcome::Held { reason, .. } if reason == reasons::CONFLICTING_ORDER), "{reused:?}");
    assert_eq!(s.status(second).await, "pending");
    assert_eq!(s.packages().await, 1, "still one set");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn orders_that_are_not_paid_and_other_events_are_ignored() {
    let s = shop().await;
    let payment_id = s.pending().await;
    let mut unpaid = paid_order(payment_id, s.account, &order_id(), 1490);
    unpaid.status = "pending".into();
    assert!(matches!(s.deliver(unpaid).await.unwrap(), NotificationOutcome::Ignored { .. }));
    let mut refunded = paid_order(payment_id, s.account, &order_id(), 1490);
    refunded.event_name = "order_refunded".into();
    assert!(matches!(s.deliver(refunded).await.unwrap(), NotificationOutcome::Ignored { .. }));
    assert_eq!(s.status(payment_id).await, "pending");
    assert_eq!(s.packages().await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn conflicting_verified_evidence_for_one_order_is_kept_apart_while_an_exact_retry_is_the_same_row() {
    let s = shop().await;
    let order = order_id();
    let held = |outcome: NotificationOutcome| match outcome {
        NotificationOutcome::Held { held_id, .. } => held_id,
        other => panic!("{other:?}"),
    };
    let first = held(s.deliver(paid_order(i64::MAX - 11, s.account, &order, 1490)).await.unwrap());
    let retry = held(s.deliver(paid_order(i64::MAX - 11, s.account, &order, 1490)).await.unwrap());
    let other = held(s.deliver(paid_order(i64::MAX - 12, s.account, &order, 1490)).await.unwrap());
    assert_eq!(first, retry, "an exact retry is idempotent");
    assert_ne!(first, other, "a different claimed payment is a different observation, not swallowed");
}
