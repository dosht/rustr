//! Checkout and confirmation against a real database with a scripted gateway.

mod support;

use std::sync::Arc;

use billing_core::app::{BillingError, OrderRequest};
use billing_core::calendar::{add_months, utc};
use billing_core::grants::PaymentStatus;
use billing_core::ports::{BillingTx, CheckoutKind, CheckoutResult, GatewayError};
use billing_core::presentation::AnalyticsIntent;
use billing_core::types::{AccountId, Micros};
use billing_postgres::PgBillingTx;
use serde_json::json;
use support::*;

fn order(hours: i64) -> OrderRequest {
    OrderRequest {
        pay_as_you_go_hours: Some(hours),
        ui_locale: "en".into(),
        gateway: "iyzico".into(),
        analytics: json!({"utm_source": "test"}),
        ..OrderRequest::default()
    }
}

fn yearly_plan() -> OrderRequest {
    OrderRequest { package_id: Some(1), ui_locale: "en".into(), gateway: "iyzico".into(), analytics: json!({}), ..OrderRequest::default() }
}

async fn payments_of(f: &Fixture, account: AccountId) -> i64 {
    count(&f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1", account).await
}

async fn events(f: &Fixture, account: AccountId, kind: &'static str) -> i64 {
    let sql: &'static str = match kind {
        "PaymentCompleted" => "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'PaymentCompleted'",
        "PaymentFailed" => "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'PaymentFailed'",
        _ => panic!("add the query"),
    };
    count(&f.pool, sql, account).await
}

async fn granted(f: &Fixture, account: AccountId) -> Vec<billing_core::ledger::PackageBalance> {
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    let mut all = billing.packages(account).await.unwrap();
    all.retain(|p| p.package.payment_id.is_some());
    all
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_043_044_059_060_067_070_completion_grants_once_with_one_event_and_a_repeat_changes_nothing() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    let checkout_time = f.clock.now();
    gateway.will_start(Ok(started("tok-1")));
    let view = f.billing.start_checkout(account, &order(10)).await.unwrap();
    assert_eq!((view.token.as_str(), view.amount.to_string().as_str(), view.currency.as_str()), ("tok-1", "14.90", "USD"));
    assert!(view.form_content.is_some());
    {
        let sent = gateway.started.lock().unwrap();
        assert_eq!(sent[0].kind, CheckoutKind::PayAsYouGo);
        assert_eq!(sent[0].amount.to_string(), "14.90");
        // The return address is configured, not taken from a request header.
        assert_eq!(sent[0].callback_url, "https://billing.example.test/checkout/confirm");
        assert!(!sent[0].reference.is_empty());
    }
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1 AND status = 'pending'", account).await, 1);

    f.clock.set(Micros(checkout_time.0 + 20 * 60 * 1_000_000)); // twenty minutes on the Iyzico form
    gateway.will_answer(Ok(success("cust-1", None)));
    let confirmed = f.billing.confirm_checkout("iyzico", "tok-1").await.unwrap();
    assert!(confirmed.changed && confirmed.success);
    assert_eq!(confirmed.analytics[0], AnalyticsIntent::PaymentCompleted);
    assert_eq!(confirmed.redirect_after_seconds, 5);

    let packages = granted(&f, account).await;
    assert_eq!(packages.len(), 1);
    let p = &packages[0].package;
    assert_eq!((p.name.as_deref(), p.minutes_included), (Some("Pay-as-you-Go"), 600));
    assert_eq!(p.start_at, checkout_time, "the window starts at the checkout row's creation, not the confirmation");
    assert_eq!(p.end_at, add_months(checkout_time, 12).unwrap());
    assert_eq!(events(&f, account, "PaymentCompleted").await, 1);

    let again = f.billing.confirm_checkout("iyzico", "tok-1").await.unwrap();
    assert!(!again.changed && again.success && again.analytics.is_empty());
    assert_eq!(gateway.result_calls.lock().unwrap().len(), 1, "a completed payment is not asked about again");
    assert_eq!(granted(&f, account).await.len(), 1);
    assert_eq!(events(&f, account, "PaymentCompleted").await, 1);
    // An unknown token shows the failure page; it changes nothing.
    assert_eq!(f.billing.confirm_checkout("iyzico", "no-such-token").await.unwrap_err(), BillingError::NotFound);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_067_no_row_is_written_when_the_provider_does_not_answer_and_no_form_is_returned_for_an_unsaved_token() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;

    gateway.will_start(Err(GatewayError::Rejected { code: "1".into(), message: "no".into() }));
    assert!(matches!(f.billing.start_checkout(account, &order(10)).await, Err(BillingError::Gateway(_))));
    gateway.will_start(Err(GatewayError::Uncertain("timeout".into())));
    assert!(matches!(f.billing.start_checkout(account, &order(10)).await, Err(BillingError::Uncertain(_))));
    assert_eq!(payments_of(&f, account).await, 0);

    // The insert fails (the provider token is already recorded): no payable form comes back.
    gateway.will_start(Ok(started("tok-same")));
    f.billing.start_checkout(account, &order(10)).await.unwrap();
    gateway.will_start(Ok(started("tok-same")));
    assert!(matches!(f.billing.start_checkout(account, &order(10)).await, Err(BillingError::Conflict(_))));
    assert_eq!(payments_of(&f, account).await, 1);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_009_the_server_refuses_hours_outside_5_to_9999_before_calling_the_provider() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    for hours in [0, 4, -1, 10_000] {
        assert!(matches!(f.billing.start_checkout(account, &order(hours)).await, Err(BillingError::Order(_))), "{hours}");
    }
    assert!(gateway.started.lock().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_045_048_055_069_a_yearly_plan_grants_twelve_packages_and_binds_the_provider_subscription() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    gateway.will_start(Ok(started("tok-plan-1")));
    gateway.will_start(Ok(started("tok-plan-2")));
    let t0 = f.clock.now();
    f.billing.start_checkout(account, &yearly_plan()).await.unwrap();
    f.billing.start_checkout(account, &yearly_plan()).await.unwrap();
    {
        let sent = gateway.started.lock().unwrap();
        assert_eq!(sent[0].kind, CheckoutKind::Subscription);
        assert_eq!(sent[0].pricing_plan_id.as_deref(), Some("plan-usd-premium-yearly"));
        assert_ne!(sent[0].reference, sent[1].reference, "each initialization carries its own conversation id");
    }
    gateway.will_answer(Ok(success("cust-9", Some("sub-9"))));
    f.billing.confirm_checkout("iyzico", "tok-plan-1").await.unwrap();

    let packages = granted(&f, account).await;
    assert_eq!(packages.len(), 12);
    assert_eq!(packages[0].package.start_at, t0);
    assert_eq!(packages[1].package.start_at, add_months(t0, 1).unwrap());
    assert_eq!(packages[11].package.start_at, add_months(t0, 11).unwrap(), "2026-01-15 steps stay on the 15th");
    assert!(packages.iter().all(|p| p.package.minutes_included == 1200));

    let owner = owner().await;
    let row: (String, Option<String>, Option<String>, String) = sqlx::query_as(
        "SELECT subscription_ref, customer_ref, first_order_ref, currency FROM billing.subscriptions WHERE account_id = $1",
    )
    .bind(account.0)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!((row.0.as_str(), row.1.as_deref(), row.2, row.3.as_str()), ("sub-9", Some("cust-9"), None, "USD"));
    let stored_ref: Option<String> =
        sqlx::query_scalar("SELECT subscription_ref FROM billing.payments WHERE account_id = $1 AND token = 'tok-plan-1'")
            .bind(account.0)
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(stored_ref.as_deref(), Some("sub-9"));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_004_a_payment_that_failed_once_and_then_completes_grants_its_packages_exactly_once() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    gateway.will_start(Ok(started("tok-f")));
    f.billing.start_checkout(account, &order(5)).await.unwrap();

    gateway.will_answer(Ok(CheckoutResult::Failure { raw_status: "FAILURE".into() }));
    let failed = f.billing.confirm_checkout("iyzico", "tok-f").await.unwrap();
    assert!(failed.changed && !failed.success);
    assert_eq!(failed.analytics, vec![AnalyticsIntent::PaymentFailed]);
    assert_eq!(granted(&f, account).await.len(), 0);
    assert_eq!(events(&f, account, "PaymentFailed").await, 1);

    gateway.will_answer(Ok(success("cust-f", None)));
    let done = f.billing.confirm_checkout("iyzico", "tok-f").await.unwrap();
    assert!(done.changed && done.success);
    assert_eq!(granted(&f, account).await.len(), 1);
    assert_eq!(events(&f, account, "PaymentCompleted").await, 1);

    // A late failure cannot undo the completion, and a repeat grants nothing.
    gateway.will_answer(Ok(CheckoutResult::Failure { raw_status: "FAILURE".into() }));
    let late = f.billing.confirm_checkout("iyzico", "tok-f").await.unwrap();
    assert!(!late.changed && late.success);
    assert_eq!(granted(&f, account).await.len(), 1);
    assert_eq!(events(&f, account, "PaymentFailed").await, 1, "one event per genuine transition");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_005_transport_uncertainty_leaves_the_payment_pending_until_an_admin_reconciles_it() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    gateway.will_start(Ok(started("tok-u")));
    let view = f.billing.start_checkout(account, &order(5)).await.unwrap();

    gateway.will_answer(Err(GatewayError::Uncertain("connection reset".into())));
    assert!(matches!(f.billing.confirm_checkout("iyzico", "tok-u").await, Err(BillingError::Uncertain(_))));
    let status: String = sqlx::query_scalar("SELECT status FROM billing.payments WHERE id = $1")
        .bind(view.payment_id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(status, "pending", "an unknown outcome is not a payment failure");
    assert_eq!(events(&f, account, "PaymentFailed").await, 0);

    gateway.will_answer(Ok(success("cust-u", None)));
    let reconciled = f.billing.reconcile_payment(view.payment_id).await.unwrap();
    assert!(reconciled.success && reconciled.changed);
    assert_eq!(granted(&f, account).await.len(), 1);
    // Reconciliation refuses a payment that is already done.
    assert!(matches!(f.billing.reconcile_payment(view.payment_id).await, Err(BillingError::InvalidRequest(_))));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_030_two_parallel_confirmations_make_one_transition_one_event_and_one_package_set() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f: Arc<Fixture> = Arc::new(fixture(vec![gateway.clone()]).await);
    let account = provisioned(&f, &unique_email()).await;
    gateway.will_start(Ok(started("tok-race")));
    f.billing.start_checkout(account, &order(5)).await.unwrap();
    gateway.will_answer(Ok(success("cust-r", None)));
    gateway.will_answer(Ok(success("cust-r", None)));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let (f, barrier) = (f.clone(), barrier.clone());
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            f.billing.confirm_checkout("iyzico", "tok-race").await.unwrap()
        }));
    }
    let results: Vec<_> = futures_join(handles).await;
    assert_eq!(results.iter().filter(|r| r.changed).count(), 1, "exactly one confirmation moved the status");
    assert!(results.iter().all(|r| r.success));
    assert_eq!(granted(&f, account).await.len(), 1);
    assert_eq!(events(&f, account, "PaymentCompleted").await, 1);
}

async fn futures_join(handles: Vec<tokio::task::JoinHandle<billing_core::app::ConfirmView>>) -> Vec<billing_core::app::ConfirmView> {
    let mut out = Vec::new();
    for h in handles {
        out.push(h.await.unwrap());
    }
    out
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_057_a_provider_identity_is_unique_per_environment() {
    let f = fixture(vec![]).await;
    let account = provisioned(&f, &unique_email()).await;
    let new = |environment: &str, token: &str| billing_core::model::NewPayment {
        account,
        amount: "14.90".parse().unwrap(),
        currency: "USD".into(),
        gateway: "iyzico".into(),
        environment: environment.into(),
        token: Some(token.into()),
        created: utc(2026, 1, 15, 12, 0, 0, 0),
        status: PaymentStatus::Pending,
        metadata: json!({}),
        coupon_id: None,
        pricing_plan_id: None,
        provider_order_ref: None,
        subscription_ref: None,
        customer_id: None,
    };
    let token = format!("tok-{}", account);
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    billing.lock_account(account).await.unwrap();
    let first = billing.insert_payment(&new("live", &token)).await.unwrap();
    let second = billing.insert_payment(&new("live", &token)).await.unwrap();
    let other_env = billing.insert_payment(&new("sandbox", &token)).await.unwrap();
    assert!(matches!(first, billing_core::model::PaymentInsert::Inserted(_)));
    assert!(matches!(second, billing_core::model::PaymentInsert::Duplicate(_)), "the same live token is the same payment");
    assert!(matches!(other_env, billing_core::model::PaymentInsert::Inserted(_)), "another environment is another identity");
    tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_045_an_invalid_package_interval_rolls_back_the_whole_completion() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    let token = format!("tok-bad-{account}");
    let mut tx = f.pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    billing.lock_account(account).await.unwrap();
    billing
        .insert_payment(&billing_core::model::NewPayment {
            account,
            amount: "14.90".parse().unwrap(),
            currency: "USD".into(),
            gateway: "iyzico".into(),
            environment: gateway.environment.into(),
            token: Some(token.clone()),
            created: utc(2026, 1, 15, 12, 0, 0, 0),
            status: PaymentStatus::Pending,
            metadata: json!({"package": {"name": "x", "minutes": 60, "count": 2, "interval": "week", "frequency": 1, "expiry": 1}}),
            coupon_id: None,
            pricing_plan_id: None,
            provider_order_ref: None,
            subscription_ref: None,
            customer_id: None,
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();

    gateway.will_answer(Ok(success("cust-bad", None)));
    let error = f.billing.confirm_checkout("iyzico", &token).await.unwrap_err();
    assert!(matches!(&error, BillingError::InvalidRequest(m) if m.contains("Invalid duration interval (week)")), "{error:?}");
    let status: String = sqlx::query_scalar("SELECT status FROM billing.payments WHERE token = $1")
        .bind(&token)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(status, "pending", "the status change rolled back with the failed grant");
    assert_eq!(granted(&f, account).await.len(), 0);
    assert_eq!(events(&f, account, "PaymentCompleted").await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_005_a_provider_error_envelope_on_the_lookup_says_nothing_about_payment_and_leaves_it_pending() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    gateway.will_start(Ok(started("tok-env")));
    let view = f.billing.start_checkout(account, &order(5)).await.unwrap();
    // A wrong key or a system error answers with a failure envelope: not a payment verdict.
    gateway.will_answer(Err(GatewayError::Rejected { code: "1000".into(), message: "invalid signature".into() }));
    assert!(matches!(f.billing.confirm_checkout("iyzico", "tok-env").await, Err(BillingError::Uncertain(_))));
    let status: String = sqlx::query_scalar("SELECT status FROM billing.payments WHERE id = $1")
        .bind(view.payment_id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(status, "pending");
    assert_eq!(events(&f, account, "PaymentFailed").await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn def_004_two_pending_payments_cannot_both_bind_and_grant_for_one_provider_subscription() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    let tag = account.to_string();
    gateway.will_start(Ok(started(&format!("tok-a-{tag}"))));
    gateway.will_start(Ok(started(&format!("tok-b-{tag}"))));
    f.billing.start_checkout(account, &yearly_plan()).await.unwrap();
    f.billing.start_checkout(account, &yearly_plan()).await.unwrap();
    let subscription = format!("sub-dup-{tag}");
    gateway.will_answer(Ok(success("cust-dup", Some(&subscription))));
    f.billing.confirm_checkout("iyzico", &format!("tok-a-{tag}")).await.unwrap();
    assert_eq!(granted(&f, account).await.len(), 12);

    // The second purchase claims the same provider subscription identity: conflict, nothing granted.
    gateway.will_answer(Ok(success("cust-dup", Some(&subscription))));
    let error = f.billing.confirm_checkout("iyzico", &format!("tok-b-{tag}")).await.unwrap_err();
    assert!(matches!(error, BillingError::Conflict(_)), "{error:?}");
    assert_eq!(granted(&f, account).await.len(), 12, "one schedule for one subscription identity");
    let second_status: String = sqlx::query_scalar("SELECT status FROM billing.payments WHERE token = $1")
        .bind(format!("tok-b-{tag}"))
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(second_status, "pending", "the whole second confirmation rolled back");
    assert_eq!(events(&f, account, "PaymentCompleted").await, 1);
    // Confirming the first payment again is an idempotent repeat, not a conflict.
    assert!(!f.billing.confirm_checkout("iyzico", &format!("tok-a-{tag}")).await.unwrap().changed);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn bil_043_058_an_authenticated_completion_recovers_an_imported_canceled_payment_and_grants_exactly_once() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let account = provisioned(&f, &unique_email()).await;
    let token = format!("tok-canceled-{account}");
    // Written as the import role, the only authority that may preserve a historical status.
    let owner = owner().await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE billing_import").execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO billing.payments (account_id, amount, currency, payment_gateway, environment, status, token, created, metadata)
         VALUES ($1, 14.90, 'USD', 'iyzico', $3, 'canceled', $2, now(),
                 '{\"package\": {\"name\": \"Pay-as-you-Go\", \"minutes\": 600, \"count\": 1, \"interval\": \"year\", \"frequency\": 1, \"expiry\": 1}}'::jsonb)",
    )
    .bind(account.0)
    .bind(&token)
    .bind(gateway.environment)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    // A historical canceled row grants nothing by itself and is never backfilled.
    assert_eq!(granted(&f, account).await.len(), 0);
    gateway.will_answer(Ok(success("cust-c", None)));
    let done = f.billing.confirm_checkout("iyzico", &token).await.unwrap();
    assert!(done.changed && done.success);
    assert_eq!(granted(&f, account).await.len(), 1);
    assert!(!f.billing.confirm_checkout("iyzico", &token).await.unwrap().changed);
    assert_eq!(granted(&f, account).await.len(), 1);
}
