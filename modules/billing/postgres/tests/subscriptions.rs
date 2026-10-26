//! Iyzico subscription notifications against a real database: unbound events
//! held, the unproven initial order (decision D4), explicit admin identification
//! of that order, and exactly-once grants afterwards.
//!
//! The signature itself is proved against literal vectors in billing-iyzico;
//! here the verifier is scripted and only the use case is under test. Every
//! test makes its own provider identities, so reruns never collide.

mod support;

use std::sync::Arc;

use billing_core::app::{BillingError, NotificationOutcome, OrderRequest};
use billing_core::grants::PaymentStatus;
use billing_core::model::{UnresolvedNotification, reasons};
use billing_core::ports::{GatewayError, SubscriptionDetail, SubscriptionNotification, SubscriptionOutcome};
use billing_core::types::{AccountId, Micros};
use serde_json::json;
use support::*;
use uuid::Uuid;

struct World {
    f: Arc<Fixture>,
    hook: Arc<ScriptedSubscriptionWebhook>,
    provider: Arc<ScriptedSubscriptionGateway>,
    account: AccountId,
    tag: String,
    sub: String,
    customer: String,
}

impl World {
    fn order(&self, name: &str) -> String {
        format!("{}-{name}", self.tag)
    }

    fn note(&self, outcome: SubscriptionOutcome, order: &str) -> SubscriptionNotification {
        SubscriptionNotification {
            outcome,
            raw_event_type: match outcome {
                SubscriptionOutcome::Success => "subscription.order.success".into(),
                SubscriptionOutcome::Failure => "subscription.order.failure".into(),
            },
            order_ref: self.order(order),
            subscription_ref: self.sub.clone(),
            customer_ref: self.customer.clone(),
        }
    }

    fn detail(&self, orders: &[&str]) -> SubscriptionDetail {
        SubscriptionDetail {
            subscription_ref: self.sub.clone(),
            customer_ref: Some(self.customer.clone()),
            pricing_plan_ref: Some("plan-usd-premium-yearly".into()),
            status: Some("ACTIVE".into()),
            order_refs: orders.iter().map(|o| self.order(o)).collect(),
        }
    }

    async fn deliver(&self, n: SubscriptionNotification) -> Result<NotificationOutcome, BillingError> {
        self.hook.deliver(n);
        self.f.billing.process_subscription_webhook(&vec![], b"{}").await
    }

    async fn resolve(&self, order: &str, why: &str) -> Result<billing_core::app::ResolveView, BillingError> {
        self.f.billing.resolve_first_order("admin@example.test", &self.sub, &self.order(order), why).await
    }

    async fn packages(&self) -> i64 {
        count(&self.f.pool, "SELECT count(*) FROM billing.packages WHERE account_id = $1 AND payment_id IS NOT NULL", self.account).await
    }

    async fn payments(&self) -> i64 {
        count(&self.f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1", self.account).await
    }

    async fn completions(&self) -> i64 {
        count(&self.f.pool, "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'PaymentCompleted'", self.account).await
    }

    async fn held(&self) -> Vec<UnresolvedNotification> {
        self.f
            .billing
            .list_unresolved(Some("held"))
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.subscription_ref.as_deref() == Some(self.sub.as_str()))
            .collect()
    }
}

/// An account with an older pay-as-you-go payment for the same customer code,
/// then a confirmed yearly subscription bound to its provider reference. The
/// provider lists `orders` (names, prefixed per world) for the subscription.
async fn world(orders: &[&str]) -> World {
    let checkout = ScriptedGateway::new("iyzico", "sandbox");
    let hook = ScriptedSubscriptionWebhook::new("sandbox");
    let tag = Uuid::new_v4().simple().to_string();
    let provider = ScriptedSubscriptionGateway::new(
        "sandbox",
        Ok(SubscriptionDetail {
            subscription_ref: format!("sub-{tag}"),
            customer_ref: Some(format!("cust-{tag}")),
            pricing_plan_ref: Some("plan-usd-premium-yearly".into()),
            status: Some("ACTIVE".into()),
            order_refs: orders.iter().map(|o| format!("{tag}-{o}")).collect(),
        }),
    );
    let (h, p) = (hook.clone(), provider.clone());
    let f = Arc::new(fixture_with(vec![checkout.clone()], move |b| b.with_subscription_webhook(h).with_subscription_gateway(p)).await);
    let account = provisioned(&f, &unique_email()).await;
    let (sub, customer) = (format!("sub-{tag}"), format!("cust-{tag}"));
    let request = |package_id: Option<usize>, hours: Option<i64>| OrderRequest {
        package_id,
        pay_as_you_go_hours: hours,
        ui_locale: "en".into(),
        gateway: "iyzico".into(),
        analytics: json!({}),
        ..OrderRequest::default()
    };
    // The oldest payment of this customer: a small pay-as-you-go purchase.
    checkout.will_start(Ok(started(&format!("tok-{tag}-payg"))));
    f.billing.start_checkout(account, &request(None, Some(5))).await.unwrap();
    checkout.will_answer(Ok(success(&customer, None)));
    f.billing.confirm_checkout("iyzico", &format!("tok-{tag}-payg")).await.unwrap();
    // The subscription.
    checkout.will_start(Ok(started(&format!("tok-{tag}-plan"))));
    f.billing.start_checkout(account, &request(Some(1), None)).await.unwrap();
    checkout.will_answer(Ok(success(&customer, Some(&sub))));
    f.billing.confirm_checkout("iyzico", &format!("tok-{tag}-plan")).await.unwrap();
    World { f, hook, provider, account, tag, sub, customer }
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_003_a_notification_that_fails_signature_verification_creates_nothing() {
    let w = world(&["ord-1"]).await;
    let (payments, packages) = (w.payments().await, w.packages().await);
    w.hook.reject();
    let error = w.f.billing.process_subscription_webhook(&vec![], b"{}").await.unwrap_err();
    assert!(matches!(error, BillingError::Webhook(_)) && error.http_status() == 401);
    assert_eq!((w.payments().await, w.packages().await), (payments, packages));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_007_an_unbound_subscription_is_held_durably_and_never_matched_to_the_customers_oldest_payment() {
    let w = world(&["ord-1"]).await;
    let (payments, packages) = (w.payments().await, w.packages().await);
    // Same customer code as an existing payment, but a subscription nobody bound.
    let mut stranger = w.note(SubscriptionOutcome::Success, "ord-legacy");
    stranger.subscription_ref = format!("sub-unbound-{}", w.tag);
    let outcome = w.deliver(stranger.clone()).await.unwrap();
    let NotificationOutcome::Held { reason, held_id } = &outcome else { panic!("{outcome:?}") };
    let held_id = *held_id;
    assert_eq!(reason, reasons::SUBSCRIPTION_UNBOUND);
    assert_eq!((w.payments().await, w.packages().await), (payments, packages), "no payment, no grant");

    // Redelivery is the same held row; a failure is a distinct observation.
    let again = w.deliver(stranger.clone()).await.unwrap();
    assert!(matches!(again, NotificationOutcome::Held { held_id: id, .. } if id == held_id));
    let mut failure = stranger.clone();
    failure.outcome = SubscriptionOutcome::Failure;
    w.deliver(failure).await.unwrap();
    let rows = w.f.billing.list_unresolved(Some("held")).await.unwrap();
    let mine: Vec<_> = rows.iter().filter(|r| r.subscription_ref.as_deref() == Some(stranger.subscription_ref.as_str())).collect();
    assert_eq!(mine.len(), 2, "one success row and one failure row, however often each arrived");
    let held_events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing.outbox WHERE kind = 'NotificationHeld' AND payload->>'subscription_ref' = $1",
    )
    .bind(&stranger.subscription_ref)
    .fetch_one(&w.f.pool)
    .await
    .unwrap();
    assert_eq!(held_events, 2, "one event per stored notification, not per delivery");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d4_while_the_initial_order_is_unproven_every_verified_order_is_held() {
    let w = world(&["ord-1", "ord-2"]).await;
    let (payments, packages, completions) = (w.payments().await, w.packages().await, w.completions().await);
    for n in [
        w.note(SubscriptionOutcome::Success, "ord-1"),
        w.note(SubscriptionOutcome::Failure, "ord-1"),
        w.note(SubscriptionOutcome::Success, "ord-2"), // a later-looking order is not auto-granted either
    ] {
        let outcome = w.deliver(n).await.unwrap();
        assert!(
            matches!(&outcome, NotificationOutcome::Held { reason, .. } if reason == reasons::INITIAL_ORDER_UNPROVEN),
            "{outcome:?}"
        );
    }
    let mut wrong = w.note(SubscriptionOutcome::Success, "ord-3");
    wrong.customer_ref = "cust-evil".into();
    let outcome = w.deliver(wrong).await.unwrap();
    assert!(matches!(&outcome, NotificationOutcome::Held { reason, .. } if reason == reasons::CUSTOMER_MISMATCH), "{outcome:?}");
    assert_eq!((w.payments().await, w.packages().await, w.completions().await), (payments, packages, completions));
    assert_eq!(w.held().await.len(), 4);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d4_an_admin_identifies_the_initial_order_against_the_provider_list_once_and_replay_grants_each_later_order_once() {
    let w = world(&["ord-1", "ord-2"]).await;
    let mut wrong = w.note(SubscriptionOutcome::Success, "ord-3");
    wrong.customer_ref = "cust-evil".into();
    for n in [
        w.note(SubscriptionOutcome::Success, "ord-1"),
        w.note(SubscriptionOutcome::Failure, "ord-1"),
        w.note(SubscriptionOutcome::Success, "ord-2"),
        wrong,
    ] {
        w.deliver(n).await.unwrap();
    }
    let (payments, packages) = (w.payments().await, w.packages().await);

    // A reference the provider does not list is refused, and so is an unreachable provider.
    assert!(matches!(w.resolve("ord-x", "looked it up").await, Err(BillingError::InvalidRequest(_))));
    *w.provider.detail.lock().unwrap() = Err(GatewayError::Uncertain("timeout".into()));
    assert!(matches!(w.resolve("ord-1", "looked it up").await, Err(BillingError::Uncertain(_))));
    *w.provider.detail.lock().unwrap() = Ok(w.detail(&["ord-1", "ord-2"]));
    assert!(matches!(w.resolve("ord-1", "").await, Err(BillingError::InvalidRequest(_))), "a rationale is required");
    assert_eq!((w.payments().await, w.packages().await), (payments, packages), "nothing changed so far");

    let view = w.resolve("ord-1", "the earliest order in the dashboard").await.unwrap();
    assert!(!view.already_resolved);
    // ord-1 success and failure are aliases of the initial purchase; ord-2 is a new order.
    assert_eq!(view.replayed, 3);
    assert_eq!(w.payments().await, payments + 1, "only ord-2 became a payment; the initial order grants nothing again");
    assert_eq!(w.packages().await, packages + 12);
    assert_eq!(w.completions().await, 3, "the pay-as-you-go, the subscription and exactly one renewal");
    let held = w.held().await;
    assert_eq!(held.len(), 1, "the customer mismatch stays held for a human");
    assert_eq!(held[0].reason, reasons::CUSTOMER_MISMATCH);

    let (by, why, first): (String, String, String) = sqlx::query_as(
        "SELECT first_order_resolved_by, first_order_rationale, first_order_ref FROM billing.subscriptions WHERE account_id = $1",
    )
    .bind(w.account.0)
    .fetch_one(&owner().await)
    .await
    .unwrap();
    assert_eq!((by.as_str(), why.as_str(), first), ("admin@example.test", "the earliest order in the dashboard", w.order("ord-1")));

    // Idempotent for the same reference, immutable against another.
    assert!(w.resolve("ord-1", "again").await.unwrap().already_resolved);
    assert!(matches!(w.resolve("ord-2", "changed my mind").await, Err(BillingError::Conflict(_))));
    assert_eq!(w.packages().await, packages + 12);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_057_058_061_after_identification_distinct_orders_grant_once_and_failure_and_success_are_distinct_observations() {
    let w = world(&["ord-1", "ord-3", "ord-4", "ord-5"]).await;
    w.resolve("ord-1", "identified").await.unwrap();
    let base_packages = w.packages().await;

    // The initial alias is a no-op in both directions.
    let alias = w.deliver(w.note(SubscriptionOutcome::Success, "ord-1")).await.unwrap();
    assert!(matches!(alias, NotificationOutcome::Ignored { .. }));
    assert_eq!(w.packages().await, base_packages);

    // ord-3: repeated delivery grants once.
    let first = w.deliver(w.note(SubscriptionOutcome::Success, "ord-3")).await.unwrap();
    let NotificationOutcome::Applied { payment_id, status, changed } = first else { panic!("{first:?}") };
    assert_eq!((status, changed), (PaymentStatus::Completed, true));
    let repeat = w.deliver(w.note(SubscriptionOutcome::Success, "ord-3")).await.unwrap();
    assert!(matches!(repeat, NotificationOutcome::Applied { changed: false, .. }));
    assert_eq!(w.packages().await, base_packages + 12, "a yearly renewal grants 12 more packages once");

    // The renewal repeats the bound plan at the bound amount, not the customer's oldest payment.
    let row: (String, String, String, Option<String>) = sqlx::query_as(
        "SELECT amount::text, metadata->'analytics'->>'payment_origin', token, pricing_plan_id FROM billing.payments WHERE id = $1",
    )
    .bind(payment_id)
    .fetch_one(&w.f.pool)
    .await
    .unwrap();
    assert_eq!(
        (row.0.as_str(), row.1.as_str(), row.2, row.3.as_deref()),
        ("120.00", "renewal", w.order("ord-3"), Some("plan-usd-premium-yearly"))
    );

    // ord-4: a failure, then the same order succeeding. Distinct observations; one grant.
    let failed = w.deliver(w.note(SubscriptionOutcome::Failure, "ord-4")).await.unwrap();
    assert!(matches!(failed, NotificationOutcome::Applied { status: PaymentStatus::Failed, changed: true, .. }));
    let failed_again = w.deliver(w.note(SubscriptionOutcome::Failure, "ord-4")).await.unwrap();
    assert!(matches!(failed_again, NotificationOutcome::Applied { changed: false, .. }));
    assert_eq!(w.packages().await, base_packages + 12);
    let recovered = w.deliver(w.note(SubscriptionOutcome::Success, "ord-4")).await.unwrap();
    assert!(matches!(recovered, NotificationOutcome::Applied { status: PaymentStatus::Completed, changed: true, .. }));
    assert_eq!(w.packages().await, base_packages + 24, "failed then completed grants exactly once");
    let late = w.deliver(w.note(SubscriptionOutcome::Failure, "ord-4")).await.unwrap();
    assert!(matches!(late, NotificationOutcome::Applied { status: PaymentStatus::Completed, changed: false, .. }));

    // ord-5: success then a late failure leaves it completed.
    w.deliver(w.note(SubscriptionOutcome::Success, "ord-5")).await.unwrap();
    let late = w.deliver(w.note(SubscriptionOutcome::Failure, "ord-5")).await.unwrap();
    assert!(matches!(late, NotificationOutcome::Applied { status: PaymentStatus::Completed, changed: false, .. }));
    assert_eq!(w.packages().await, base_packages + 36);
    assert_eq!(w.completions().await, 2 + 3, "pay-as-you-go, subscription, and three renewals");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_003_parallel_duplicate_deliveries_of_one_order_grant_once() {
    let w = Arc::new(world(&["ord-1", "ord-6"]).await);
    w.resolve("ord-1", "identified").await.unwrap();
    let base = w.packages().await;
    w.hook.deliver(w.note(SubscriptionOutcome::Success, "ord-6"));
    let barrier = Arc::new(tokio::sync::Barrier::new(4));
    let mut handles = Vec::new();
    for _ in 0..4 {
        let (w, barrier) = (w.clone(), barrier.clone());
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            w.f.billing.process_subscription_webhook(&vec![], b"{}").await.unwrap()
        }));
    }
    let mut changed = 0;
    for h in handles {
        if let NotificationOutcome::Applied { changed: true, .. } = h.await.unwrap() {
            changed += 1;
        }
    }
    assert_eq!(changed, 1);
    assert_eq!(w.packages().await, base + 12);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_045_055_a_replayed_renewal_starts_its_packages_when_its_notification_arrived_not_when_it_was_resolved() {
    let w = world(&["ord-1", "ord-2"]).await;
    let day_one = w.f.clock.now();
    let held = w.deliver(w.note(SubscriptionOutcome::Success, "ord-2")).await.unwrap();
    assert!(matches!(held, NotificationOutcome::Held { .. }));
    // Five days later an admin identifies the initial order and the held renewal replays.
    w.f.clock.set(Micros(day_one.0 + 5 * 86_400 * 1_000_000));
    w.resolve("ord-1", "identified on day six").await.unwrap();
    let starts: Vec<i64> = sqlx::query_scalar(
        "SELECT billing.ts_to_us(start_at) FROM billing.packages
         WHERE payment_id = (SELECT id FROM billing.payments WHERE token = $1) ORDER BY payment_seq",
    )
    .bind(w.order("ord-2"))
    .fetch_all(&w.f.pool)
    .await
    .unwrap();
    assert_eq!(starts.len(), 12);
    assert_eq!(starts[0], day_one.0, "the first renewal window starts on the day the notification arrived");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d4_resolution_fails_closed_when_the_provider_detail_does_not_name_the_bound_customer() {
    let w = world(&["ord-1"]).await;
    let mut nameless = w.detail(&["ord-1"]);
    nameless.customer_ref = None;
    *w.provider.detail.lock().unwrap() = Ok(nameless);
    assert!(matches!(w.resolve("ord-1", "the detail has no customer").await, Err(BillingError::Uncertain(_))));
    let mut other = w.detail(&["ord-1"]);
    other.customer_ref = Some("cust-someone-else".into());
    *w.provider.detail.lock().unwrap() = Ok(other);
    assert!(matches!(w.resolve("ord-1", "wrong customer").await, Err(BillingError::Conflict(_))));
    let stored: Option<String> = sqlx::query_scalar("SELECT first_order_ref FROM billing.subscriptions WHERE account_id = $1")
        .bind(w.account.0)
        .fetch_one(&w.f.pool)
        .await
        .unwrap();
    assert_eq!(stored, None, "nothing was stored");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d4_an_order_that_already_belongs_to_another_subscription_is_held_and_never_transitioned_under_the_wrong_lock() {
    let a = world(&["ord-1", "ord-3"]).await;
    let b = world(&["ord-1"]).await;
    a.resolve("ord-1", "identified").await.unwrap();
    b.resolve("ord-1", "identified").await.unwrap();
    // A's renewal exists, granted once.
    a.deliver(a.note(SubscriptionOutcome::Success, "ord-3")).await.unwrap();
    let (a_packages, b_packages) = (a.packages().await, b.packages().await);

    // A verified notification for B's subscription that names A's order reference.
    let mut stolen = b.note(SubscriptionOutcome::Failure, "ord-1");
    stolen.order_ref = a.order("ord-3");
    let outcome = b.deliver(stolen).await.unwrap();
    assert!(matches!(&outcome, NotificationOutcome::Held { reason, .. } if reason == reasons::CONFLICTING_ORDER), "{outcome:?}");
    assert_eq!((a.packages().await, b.packages().await), (a_packages, b_packages));
    let status: String = sqlx::query_scalar("SELECT status FROM billing.payments WHERE token = $1")
        .bind(a.order("ord-3"))
        .fetch_one(&a.f.pool)
        .await
        .unwrap();
    assert_eq!(status, "completed", "A's payment was not touched by B's notification");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn a_notification_held_as_unbound_is_the_same_row_when_it_is_held_again_for_the_initial_order() {
    let checkout = ScriptedGateway::new("iyzico", "sandbox");
    let hook = ScriptedSubscriptionWebhook::new("sandbox");
    let h = hook.clone();
    let f = fixture_with(vec![checkout.clone()], move |b| b.with_subscription_webhook(h)).await;
    let account = provisioned(&f, &unique_email()).await;
    let tag = Uuid::new_v4().simple().to_string();
    let (sub, customer) = (format!("sub-late-{tag}"), format!("cust-late-{tag}"));
    let n = SubscriptionNotification {
        outcome: SubscriptionOutcome::Success,
        raw_event_type: "subscription.order.success".into(),
        order_ref: format!("{tag}-ord-9"),
        subscription_ref: sub.clone(),
        customer_ref: customer.clone(),
    };
    hook.deliver(n.clone());
    let first = f.billing.process_subscription_webhook(&vec![], b"{}").await.unwrap();
    let NotificationOutcome::Held { reason, held_id } = &first else { panic!("{first:?}") };
    let held_id = *held_id;
    assert_eq!(reason, reasons::SUBSCRIPTION_UNBOUND);

    // The notification arrived before the confirmation bound the subscription.
    checkout.will_start(Ok(started(&format!("tok-late-{tag}"))));
    f.billing
        .start_checkout(account, &OrderRequest { package_id: Some(1), ui_locale: "en".into(), gateway: "iyzico".into(), analytics: json!({}), ..OrderRequest::default() })
        .await
        .unwrap();
    checkout.will_answer(Ok(success(&customer, Some(&sub))));
    f.billing.confirm_checkout("iyzico", &format!("tok-late-{tag}")).await.unwrap();

    let replayed = f.billing.replay_unresolved(held_id).await.unwrap();
    assert!(matches!(&replayed, NotificationOutcome::Held { held_id: id, reason } if *id == held_id && reason == reasons::INITIAL_ORDER_UNPROVEN), "{replayed:?}");
    let rows: Vec<_> = f
        .billing
        .list_unresolved(Some("held"))
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.subscription_ref.as_deref() == Some(sub.as_str()))
        .collect();
    assert_eq!(rows.len(), 1, "one row, with its reason updated, not a stale duplicate");
    assert_eq!(rows[0].reason, reasons::INITIAL_ORDER_UNPROVEN);
}

/// Held evidence is never silently replaced: an exact retry returns the same row,
/// a changed reason with the same evidence updates it, and the same claimed
/// identity carrying different signed evidence is a conflict that leaves the first
/// observation exactly as it was.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn held_notifications_keep_the_first_evidence_and_refuse_a_different_payload_for_the_same_identity() {
    use billing_core::model::NewUnresolved;
    use billing_core::ports::{BillingTx, StoreError};
    use billing_postgres::PgBillingTx;

    let f = fixture(vec![]).await;
    let order = format!("ls-order-{}", Uuid::new_v4().simple());
    let held = |reason: &str, total_cents: i64| NewUnresolved {
        gateway: "lemonsqueezy".into(),
        environment: "test".into(),
        event_type: "success".into(),
        subscription_ref: None,
        order_ref: Some(order.clone()),
        customer_ref: None,
        payment_id: Some(4_242),
        reason: reason.into(),
        payload: json!({"order_id": order, "total_cents": total_cents}),
        received_at: Micros(1_768_478_400_000_000),
    };
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    let (first, inserted) = billing.hold_notification(&held(reasons::PAYMENT_UNKNOWN, 745)).await.unwrap();
    assert!(inserted);
    let (retry, inserted) = billing.hold_notification(&held(reasons::PAYMENT_UNKNOWN, 745)).await.unwrap();
    assert_eq!((retry.id, inserted), (first.id, false), "an exact retry is the same row");
    let (moved, inserted) = billing.hold_notification(&held(reasons::AMOUNT_MISMATCH, 745)).await.unwrap();
    assert_eq!((moved.id, moved.reason.as_str(), inserted), (first.id, reasons::AMOUNT_MISMATCH, false));
    let conflicting = billing.hold_notification(&held(reasons::CONFLICTING_ORDER, 999)).await;
    assert!(matches!(&conflicting, Err(StoreError::Conflict(m)) if m.contains("different evidence")), "{conflicting:?}");
    tx.rollback().await.unwrap();

    // The refusal changed nothing in a committed run either.
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    billing.hold_notification(&held(reasons::PAYMENT_UNKNOWN, 745)).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    assert!(billing.hold_notification(&held(reasons::PAYMENT_UNKNOWN, 999)).await.is_err());
    tx.rollback().await.unwrap();
    let rows: Vec<UnresolvedNotification> = f
        .billing
        .list_unresolved(None)
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.order_ref.as_deref() == Some(order.as_str()))
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].payload["total_cents"], 745, "the first evidence is kept exactly");
}
