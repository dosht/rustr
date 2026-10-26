//! Cancellation, history, admin lookup, coupons, eligibility data and the
//! outbox relay against a real database.

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use billing_core::app::{AdminLookup, BillingError, OrderRequest};
use billing_core::model::BillingEvent;
use billing_core::ports::BillingStore;
use billing_core::pricing::SpecialClientOffer;
use billing_core::types::{AccountId, Micros};
use serde_json::json;
use support::*;
use uuid::Uuid;

fn payg(hours: i64, gateway: &str) -> OrderRequest {
    OrderRequest {
        pay_as_you_go_hours: Some(hours),
        ui_locale: "en".into(),
        gateway: gateway.into(),
        analytics: json!({}),
        ..OrderRequest::default()
    }
}

/// Starts and confirms a pay-as-you-go purchase; returns the payment id.
async fn buy(f: &Fixture, gateway: &Arc<ScriptedGateway>, account: AccountId, hours: i64) -> i64 {
    let token = format!("tok-{}", Uuid::new_v4().simple());
    gateway.will_start(Ok(started(&token)));
    let view = f.billing.start_checkout(account, &payg(hours, "iyzico")).await.unwrap();
    gateway.will_answer(Ok(success("cust", None)));
    f.billing.confirm_checkout("iyzico", &token).await.unwrap();
    view.payment_id
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_062_only_the_owner_can_request_cancellation_which_keeps_the_minutes_and_queues_the_support_mail() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let f = fixture(vec![gateway.clone()]).await;
    let (owner_account, other) = (provisioned(&f, &unique_email()).await, provisioned(&f, &unique_email()).await);
    let payment = buy(&f, &gateway, owner_account, 10).await;

    // Someone else's payment looks exactly like a missing one.
    assert_eq!(f.billing.cancel_subscription(other, payment).await.unwrap_err(), BillingError::NotFound);
    assert_eq!(f.billing.cancel_subscription(owner_account, i64::MAX - 3).await.unwrap_err(), BillingError::NotFound);
    let stamped: Option<i64> = sqlx::query_scalar("SELECT billing.ts_to_us(subscription_canceled_at) FROM billing.payments WHERE id = $1")
        .bind(payment)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(stamped, None, "a rejected request stamps nothing");

    let before = f.billing.overview(owner_account).await.unwrap().balance;
    let view = f.billing.cancel_subscription(owner_account, payment).await.unwrap();
    assert_eq!(view.payment_id, payment);
    let stamped: Option<i64> = sqlx::query_scalar("SELECT billing.ts_to_us(subscription_canceled_at) FROM billing.payments WHERE id = $1")
        .bind(payment)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(stamped, Some(view.canceled_at.0));
    assert_eq!(f.billing.overview(owner_account).await.unwrap().balance, before, "bought minutes stay usable");
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'CancelRequested'", owner_account).await, 1);
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'CancelRequested'", other).await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_075_076_history_lists_completed_payments_newest_first_and_admin_lookup_returns_the_real_customer() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let f = fixture(vec![gateway.clone()]).await;
    let email = unique_email();
    let account = provisioned(&f, &email).await;
    let first = buy(&f, &gateway, account, 10).await;
    f.clock.set(Micros(f.clock.now().0 + 3_600_000_000));
    let second = buy(&f, &gateway, account, 5).await;
    // A pending checkout never shows in the history.
    gateway.will_start(Ok(started(&format!("tok-{}", Uuid::new_v4().simple()))));
    f.billing.start_checkout(account, &payg(7, "iyzico")).await.unwrap();

    let history = f.billing.history(account).await.unwrap();
    assert_eq!(history.iter().map(|h| h.payment_id).collect::<Vec<_>>(), vec![second, first]);
    assert_eq!(history[0].invoice_id, format!("TRAN{second:08}"));
    assert_eq!(history[0].amount_text, "7.45 USD");
    assert_eq!(history[0].package_name, "Pay-as-you-Go");
    assert_eq!(history[0].date, "15/01/2026");

    // Billing info is read-only data: written as the import role, read for the lookup.
    let owner = owner().await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE billing_import").execute(&mut *tx).await.unwrap();
    for (name, days) in [("Old Name", 10), ("New Name", 1)] {
        sqlx::query("INSERT INTO billing.billing_info (account_id, full_name, created_at) VALUES ($1, $2, now() - ($3::int * interval '1 day'))")
            .bind(account.0)
            .bind(name)
            .bind(days)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();

    let lookup: AdminLookup = f.billing.admin_customer_invoices(&email.to_uppercase()).await.unwrap();
    assert_eq!(lookup.customer.account, account);
    assert_eq!(lookup.payments.iter().map(|p| p.id).collect::<Vec<_>>(), vec![second, first]);
    assert_eq!(lookup.billing_info.and_then(|b| b.full_name).as_deref(), Some("New Name"));
    assert_eq!(
        f.billing.admin_customer_invoices("nobody@example.test").await.unwrap_err(),
        BillingError::NotFound
    );
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn cpn_008_to_013_coupons_are_read_as_stored_matched_exactly_and_the_id_is_kept_when_it_did_not_apply() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    let tag = Uuid::new_v4().simple().to_string();
    let owner = owner().await;
    for (id, amount, product, expires) in [
        (format!("TEN-{tag}"), "0.1", Some("Pay-as-you-Go"), "2099-01-01"),
        (format!("OTHER-{tag}"), "0.5", Some("PREMIUM"), "2099-01-01"),
        (format!("OLD-{tag}"), "0.5", Some("Pay-as-you-Go"), "2020-01-01"),
    ] {
        sqlx::query("INSERT INTO billing.coupons (id, amount, filter, expires_at) VALUES ($1, $2::text::real, $3::text::jsonb, $4::text::date)")
            .bind(id)
            .bind(amount)
            .bind(product.map(|p| json!({"product": p}).to_string()))
            .bind(expires)
            .execute(&owner)
            .await
            .unwrap();
    }
    let quote = |coupon: String| {
        let f = &f;
        async move {
            let request = OrderRequest { coupon_id: Some(coupon), ..payg(10, "iyzico") };
            f.billing.quote(account, &request).await.unwrap()
        }
    };
    // float4 0.1 is read as the decimal 0.1, not 0.10000000149011612.
    let ten = quote(format!("TEN-{tag}")).await;
    assert_eq!((ten.amount.to_string().as_str(), ten.discount_applied), ("13.41", true));
    // Matching is exact: another case is another code, which does not exist.
    let lower = quote(format!("ten-{tag}")).await;
    assert_eq!((lower.amount.to_string().as_str(), lower.coupon_id), ("14.90", None));
    // An existing, unexpired coupon for another product: full price, id still kept.
    let other = quote(format!("OTHER-{tag}")).await;
    assert_eq!((other.amount.to_string().as_str(), other.discount_applied), ("14.90", false));
    assert_eq!(other.coupon_id.as_deref(), Some(format!("OTHER-{tag}").as_str()));
    let expired = quote(format!("OLD-{tag}")).await;
    assert_eq!((expired.amount.to_string().as_str(), expired.coupon_id), ("14.90", None));
    // The coupon is saved on the checkout row.
    gateway.will_start(Ok(started(&format!("tok-{tag}"))));
    let request = OrderRequest { coupon_id: Some(format!("OTHER-{tag}")), ..payg(10, "iyzico") };
    let view = f.billing.start_checkout(account, &request).await.unwrap();
    assert_eq!(view.coupon_id.as_deref(), Some(format!("OTHER-{tag}").as_str()));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn cpn_007_019_eligibility_comes_from_host_data_never_from_the_request() {
    let special = unique_email();
    let loyal = account();
    let mut cfg = config();
    cfg.audience.special_offer_emails = BTreeSet::from([special.to_lowercase()]);
    cfg.audience.special_clients = BTreeSet::from([loyal]);
    cfg.special_client_offer = Some(SpecialClientOffer { code: "LOYAL".into(), percent: 33, message: "loyalty text as captured".into() });
    let f = fixture_config(vec![], cfg, |b| b).await;
    let offered = provisioned(&f, &special).await;
    let ordinary = provisioned(&f, &unique_email()).await;
    f.directory.add(customer(loyal, &unique_email(), f.clock.now()));

    // The same request, TRY rate and all, gets the offer only for the listed email.
    let request = OrderRequest { currency: Some("USD".into()), ..payg(10, "iyzico") };
    let offer = f.billing.quote(offered, &request).await.unwrap();
    assert_eq!((offer.currency.as_str(), offer.amount.to_string().as_str()), ("TRY", "149.90"));
    assert!(offer.coupon_id.is_none());
    let usual = f.billing.quote(ordinary, &request).await.unwrap();
    assert_eq!((usual.currency.as_str(), usual.amount.to_string().as_str()), ("USD", "14.90"));

    assert!(f.billing.overview(loyal).await.unwrap().special_client_alert.is_some_and(|a| a.percent == 33));
    assert!(f.billing.overview(ordinary).await.unwrap().special_client_alert.is_none());
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn fre_013_014_the_overview_keeps_the_three_freemium_facets_apart() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    let free = f.billing.overview(account).await.unwrap();
    assert!(free.is_freemium);
    assert!(free.facets.database_no_open_paid_package && free.facets.historical_single_free_package && free.facets.no_unexpired_paid_package);
    buy(&f, &gateway, account, 5).await;
    let paid = f.billing.overview(account).await.unwrap();
    assert!(!paid.is_freemium, "any purchase ever ends the single-package definition");
    assert!(!paid.facets.database_no_open_paid_package);
    assert_eq!(paid.balance, Some(20 + 300));
    assert_eq!(paid.balance_text, "5 hr , 20 min");
}

async fn wait_for(mut check: impl FnMut() -> bool) {
    for _ in 0..80 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("condition not reached");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn eml_bil_001_002_003_committed_events_are_delivered_to_buyer_sales_and_support_with_stable_keys() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let mailer = Arc::new(RecordingMailer::default());
    let m = mailer.clone();
    let f = isolated_fixture_with(vec![gateway.clone()], move |b| b.with_mailer(m)).await;
    let email = unique_email();
    let account = provisioned(&f, &email).await;
    let payment = buy(&f, &gateway, account, 10).await;
    f.billing.cancel_subscription(account, payment).await.unwrap();

    let mine = |mailer: &RecordingMailer| -> Vec<(String, String, String)> {
        mailer
            .sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| m.to == email || m.text.contains(&account.to_string()) || (m.to == "support@example.test" && m.text == payment.to_string()))
            .map(|(m, key)| (m.to.clone(), m.subject.clone(), key.clone()))
            .collect()
    };
    wait_for_relay(&f, || mine(&mailer).len() >= 3).await;
    let sent = mine(&mailer);
    assert_eq!(sent.len(), 3, "buyer, sales and support, once each");
    let subjects: BTreeSet<_> = sent.iter().map(|(_, s, _)| s.as_str()).collect();
    assert_eq!(subjects, BTreeSet::from(["Woohoo! Your Transgate purchase is complete!", "Payment Completed", "!!!CANCEL SUBSCRIPTION!!!"]));
    assert!(sent.iter().all(|(_, _, key)| key.contains(':')), "every send carries a stable per-event idempotency key");

    // Delivered events are acknowledged: another pass sends nothing more.
    f.billing.relay_outbox(1000, 60).await.unwrap();
    assert_eq!(mine(&mailer).len(), 3);
    let unpublished: i64 = count(
        &f.pool,
        "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND published_at IS NULL AND kind IN ('PaymentCompleted', 'CancelRequested')",
        account,
    )
    .await;
    assert_eq!(unpublished, 0);
}

async fn wait_for_relay(f: &Fixture, mut done: impl FnMut() -> bool) {
    for _ in 0..40 {
        f.billing.relay_outbox(1000, 60).await.unwrap();
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    wait_for(done).await;
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn a_failed_recipient_keeps_the_event_unacknowledged_and_a_retry_does_not_resend_the_buyer() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let mailer = Arc::new(RecordingMailer::default());
    let m = mailer.clone();
    let f = isolated_fixture_with(vec![gateway.clone()], move |b| b.with_mailer(m)).await;
    let email = unique_email();
    let account = provisioned(&f, &email).await;
    mailer.failing.lock().unwrap().push("sales@example.test".into());
    buy(&f, &gateway, account, 10).await;

    let buyer_mails = |mailer: &RecordingMailer| mailer.sent.lock().unwrap().iter().filter(|(m, _)| m.to == email).count();
    wait_for_relay(&f, || buyer_mails(&mailer) >= 1).await;
    assert_eq!(buyer_mails(&mailer), 1);
    let pending: i64 = count(
        &f.pool,
        "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'PaymentCompleted' AND published_at IS NULL",
        account,
    )
    .await;
    assert_eq!(pending, 1, "one failed request means the completion is not acknowledged as delivered");
    let attempts: i32 = sqlx::query_scalar("SELECT attempts FROM billing.outbox WHERE account_id = $1 AND kind = 'PaymentCompleted'")
        .bind(account.0)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert!(attempts >= 1);

    // Later, with the recipient reachable again, only the missing message is sent.
    mailer.failing.lock().unwrap().clear();
    f.clock.set(Micros(f.clock.now().0 + 24 * 3_600_000_000));
    wait_for_relay(&f, || {
        mailer.sent.lock().unwrap().iter().any(|(m, _)| m.to == "sales@example.test" && m.text.contains(&account.to_string()))
    })
    .await;
    assert_eq!(buyer_mails(&mailer), 1, "the buyer is not mailed twice");
    let pending: i64 = count(
        &f.pool,
        "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'PaymentCompleted' AND published_at IS NULL",
        account,
    )
    .await;
    assert_eq!(pending, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn two_concurrent_consumers_never_hold_the_same_event() {
    // A database of its own: the real, global claim sees exactly these 8 events.
    let f = isolated_fixture_with(vec![], |b| b).await;
    {
        let mut unit = f.store.begin().await.unwrap();
        for n in 0..8 {
            unit.tx()
                .emit(
                    &BillingEvent::NotificationHeld {
                        gateway: "test".into(),
                        reason: format!("claim-test-{n}"),
                        order_ref: None,
                        subscription_ref: None,
                    },
                    f.clock.now(),
                )
                .await
                .unwrap();
        }
        unit.commit().await.unwrap();
    }
    let (a, b) = (f.store.clone(), f.store.clone());
    let now = f.clock.now();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let (b1, b2) = (barrier.clone(), barrier);
    let first = tokio::spawn(async move {
        b1.wait().await;
        a.claim_outbox(1000, 1, now).await.unwrap()
    });
    let second = tokio::spawn(async move {
        b2.wait().await;
        b.claim_outbox(1000, 1, now).await.unwrap()
    });
    let (first, second) = (first.await.unwrap(), second.await.unwrap());
    let ids: BTreeSet<i64> = first.iter().map(|e| e.id).collect();
    assert!(second.iter().all(|e| !ids.contains(&e.id)), "an active claim is exclusive");
    assert_eq!(first.len() + second.len(), 8, "every event is claimed by exactly one consumer");
    for event in first.iter().chain(second.iter()) {
        f.store.release_outbox(event, "claim test", now).await.unwrap();
    }
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn mail_that_is_not_configured_is_an_error_not_a_silent_acknowledgement() {
    let f = fixture(vec![]).await;
    assert!(matches!(f.billing.relay_outbox(10, 60).await, Err(BillingError::NotConfigured(_))));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn outbox_after_a_lease_expires_a_new_claim_wins_and_the_stale_claimant_can_neither_acknowledge_nor_release() {
    // A database of its own, so this test's leases never hide another test's events.
    let f = isolated_fixture_with(vec![], |b| b).await;
    let t0 = f.clock.now();
    let ours = |events: &[billing_core::model::OutboxEvent], tag: &str| {
        events
            .iter()
            .find(|e| matches!(&e.event, BillingEvent::NotificationHeld { reason, .. } if reason == tag))
            .cloned()
    };
    // Other tests' relays may deliver any queued event; make our own until we claim it first.
    let (tag, a) = {
        let mut found = None;
        for _ in 0..10 {
            let tag = format!("lease-{}", Uuid::new_v4().simple());
            let mut unit = f.store.begin().await.unwrap();
            unit.tx()
                .emit(
                    &BillingEvent::NotificationHeld { gateway: "test".into(), reason: tag.clone(), order_ref: None, subscription_ref: None },
                    t0,
                )
                .await
                .unwrap();
            unit.commit().await.unwrap();
            let claimed = f.store.claim_outbox(1000, 10, t0).await.unwrap();
            if let Some(event) = ours(&claimed, &tag) {
                found = Some((tag, event));
                break;
            }
        }
        found.expect("claimed our own event")
    };

    // While A's lease is active nobody else gets it.
    let during = f.store.claim_outbox(1000, 10, Micros(t0.0 + 5_000_000)).await.unwrap();
    assert!(ours(&during, &tag).is_none());
    // After the lease expires B claims it with a new token.
    let later = Micros(t0.0 + 11_000_000);
    let b = ours(&f.store.claim_outbox(1000, 10, later).await.unwrap(), &tag).expect("B claims after expiry");
    assert_eq!(a.id, b.id);
    assert_ne!(a.claim_token, b.claim_token);
    // The stale claimant cannot acknowledge or release B's claim.
    assert!(!f.store.ack_outbox(&a, later).await.unwrap());
    f.store.release_outbox(&a, "stale release", later).await.unwrap();
    let unpublished: Option<bool> = sqlx::query_scalar("SELECT published_at IS NULL FROM billing.outbox WHERE id = $1")
        .bind(b.id)
        .fetch_optional(&f.pool)
        .await
        .unwrap();
    assert_eq!(unpublished, Some(true));
    // B still owns it and can finish. This is at-least-once, not exactly-once.
    assert!(f.store.ack_outbox(&b, later).await.unwrap());
    assert!(!f.store.ack_outbox(&b, later).await.unwrap(), "an acknowledged event is not acknowledged twice");

    // A claim needs a positive limit and a positive lease.
    assert!(f.store.claim_outbox(0, 10, t0).await.is_err());
    assert!(f.store.claim_outbox(10, 0, t0).await.is_err());
}

/// Starts and confirms a pay-as-you-go purchase with a coupon; returns the
/// payment id and the quoted amount and coupon id the checkout answered with.
async fn buy_with_coupon(
    f: &Fixture,
    gateway: &Arc<ScriptedGateway>,
    account: AccountId,
    hours: i64,
    code: &str,
) -> (i64, String, Option<String>) {
    let token = format!("tok-{}", Uuid::new_v4().simple());
    gateway.will_start(Ok(started(&token)));
    let request = OrderRequest { coupon_id: Some(code.to_string()), ..payg(hours, "iyzico") };
    let view = f.billing.start_checkout(account, &request).await.unwrap();
    gateway.will_answer(Ok(success("cust", None)));
    assert!(f.billing.confirm_checkout("iyzico", &token).await.unwrap().success);
    (view.payment_id, view.amount.to_string(), view.coupon_id)
}

/// CPN-014: a coupon has no usage counter, per-account limit or one-use rule. One
/// code applies again for the same account after a completed purchase, and for
/// another account, at the same percentage; every checkout row keeps the code.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn cpn_014_one_coupon_is_used_again_by_the_same_account_and_by_another_with_the_same_discount_and_saved_id() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let f = fixture(vec![gateway.clone()]).await;
    let (first, second) = (provisioned(&f, &unique_email()).await, provisioned(&f, &unique_email()).await);
    let code = format!("QUARTER-{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO billing.coupons (id, amount, filter, expires_at)
         VALUES ($1, '0.25'::real, $2::text::jsonb, '2099-01-01'::date)",
    )
    .bind(&code)
    .bind(json!({"product": "Pay-as-you-Go"}).to_string())
    .execute(&owner().await)
    .await
    .unwrap();

    // Hours x 1.49 USD less 25 percent, rounded half up to cents:
    // 14.90 x 0.75 = 11.175, 7.45 x 0.75 = 5.5875, 37.25 x 0.75 = 27.9375.
    for (account, hours, expected) in [(first, 10, "11.18"), (first, 5, "5.59"), (second, 25, "27.94")] {
        let (payment_id, quoted, coupon) = buy_with_coupon(&f, &gateway, account, hours, &code).await;
        assert_eq!(quoted, expected, "{hours} hours");
        assert_eq!(coupon.as_deref(), Some(code.as_str()));
        let (amount, saved, status): (String, Option<String>, String) =
            sqlx::query_as("SELECT amount::text, coupon_id, status FROM billing.payments WHERE id = $1")
                .bind(payment_id)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!((amount.as_str(), saved.as_deref(), status.as_str()), (expected, Some(code.as_str()), "completed"), "{hours} hours");
    }
}
