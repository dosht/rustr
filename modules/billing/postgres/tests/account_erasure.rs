//! Account erasure, billing side (P3: decisions D5, D9 and D10), against a real
//! database through the restricted runtime login.
//!
//! Run with `BILLING_TEST_DATABASE_URL=... cargo test -p billing-postgres -- --include-ignored`.

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use billing_core::app::{BillingError, ChargeError, OrderRequest, charge_strict};
use billing_core::calendar::add_months;
use billing_core::charge::StartRefusal;
use billing_core::erasure::{
    CheckoutClosure, ErasureError, SubscriptionTermination, TerminationEvidence, TerminationTarget, erase_account_billing,
    open_account_erasure, terminate_subscriptions,
};
use billing_core::ledger::{Allocation, balance, open_at};
use billing_core::model::NewPackage;
use billing_core::ports::{BillingTx, CheckoutGateway, CheckoutKind, CheckoutResult, CheckoutStart, CheckoutStarted, GatewayError, StoreError};
use billing_core::types::{AccountId, BoxFuture, Micros};
use billing_postgres::{PgBillingStore, PgBillingTx};
use sqlx::PgPool;
use support::*;
use tokio::sync::oneshot;
use uuid::Uuid;

const NOW: Micros = Micros(1_768_478_400_000_000); // 2026-01-15T12:00:00Z

async fn new_account(f: &Fixture) -> AccountId {
    let account = provisioned(f, &unique_email()).await;
    f.clock.set(NOW);
    account
}

async fn give_package(pool: &PgPool, account: AccountId, minutes: i64) -> i64 {
    let mut tx = pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    billing.lock_account(account).await.unwrap();
    let id = billing
        .insert_package(&NewPackage {
            account,
            payment_id: None,
            payment_seq: None,
            name: "PREMIUM".into(),
            minutes,
            start_at: Micros(NOW.0 - 86_400_000_000),
            end_at: add_months(NOW, 3).unwrap(),
            created: NOW,
            signup_grant: false,
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();
    id
}

async fn open_balance(pool: &PgPool, account: AccountId) -> Option<i64> {
    let mut tx = pool.begin().await.unwrap();
    let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
    balance(&open_at(&billing.packages(account).await.unwrap(), NOW))
}

/// Imported ledger rows `(row account, package, subject, minutes)`, as the import role.
async fn import_usage(rows: &[(AccountId, Option<i64>, i64, i32)]) {
    let owner = owner().await;
    let base = (Uuid::new_v4().as_u128() as i64) & 0x0fff_ffff_ffff_f000;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE billing_import").execute(&mut *tx).await.unwrap();
    for (n, (row_account, package_id, subject, minutes)) in rows.iter().enumerate() {
        sqlx::query(
            "INSERT INTO billing.legacy_usage (id, account_id, package_id, subject_ref, minutes, used_at)
             VALUES ($1, $2, $3, $4, $5, now())",
        )
        .bind(base + n as i64 + 1)
        .bind(row_account.0)
        .bind(*package_id)
        .bind(*subject)
        .bind(*minutes)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

/// A completed plan payment for `account`, optionally bound to a verified
/// subscription, written as the import role. Returns the payment id.
async fn import_plan_payment(account: AccountId, status: &str, bound_to: Option<(&str, &str)>) -> i64 {
    let owner = owner().await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE billing_import").execute(&mut *tx).await.unwrap();
    let token = format!("tok-{}", Uuid::new_v4().simple());
    let payment: i64 = sqlx::query_scalar(
        "INSERT INTO billing.payments (account_id, amount, currency, payment_gateway, environment, status, token, created,
                                       pricing_plan_id, customer_id, subscription_ref)
         VALUES ($1, 120, 'USD', 'iyzico', 'sandbox', $2, $3, now(), 'plan-usd-premium-yearly', $4, $5) RETURNING id",
    )
    .bind(account.0)
    .bind(status)
    .bind(&token)
    .bind(bound_to.map(|(_, customer)| customer))
    .bind(bound_to.map(|(subscription, _)| subscription))
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    if let Some((subscription, customer)) = bound_to {
        sqlx::query(
            "INSERT INTO billing.subscriptions (account_id, payment_gateway, environment, subscription_ref, customer_ref,
                                                pricing_plan_id, currency, amount, spec, initial_payment_id, created_at)
             VALUES ($1, 'iyzico', 'sandbox', $2, $3, 'plan-usd-premium-yearly', 'USD', 120,
                     '{\"name\":\"PREMIUM\",\"minutes\":1200,\"count\":12,\"interval\":\"month\",\"frequency\":1,\"expiry\":1}'::jsonb,
                     $4, now())",
        )
        .bind(account.0)
        .bind(subscription)
        .bind(customer)
        .bind(payment)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    payment
}

/// Answers a scripted sequence; records what it was asked. A deliberate test adapter.
struct ScriptedTermination {
    answers: Mutex<Vec<Result<TerminationEvidence, GatewayError>>>,
    asked: Mutex<Vec<TerminationTarget>>,
}

impl ScriptedTermination {
    fn new(answers: Vec<Result<TerminationEvidence, GatewayError>>) -> ScriptedTermination {
        ScriptedTermination { answers: Mutex::new(answers.into_iter().rev().collect()), asked: Mutex::default() }
    }
}

impl SubscriptionTermination for ScriptedTermination {
    fn gateway(&self) -> &str {
        "iyzico"
    }

    fn environment(&self) -> &str {
        "sandbox"
    }

    fn terminate<'a>(&'a self, target: &'a TerminationTarget) -> BoxFuture<'a, Result<TerminationEvidence, GatewayError>> {
        self.asked.lock().unwrap().push(target.clone());
        let answer = self.answers.lock().unwrap().pop().unwrap_or(Err(GatewayError::Uncertain("nothing scripted".into())));
        Box::pin(std::future::ready(answer))
    }
}

async fn erase_in_host_tx(pool: &PgPool, account: AccountId, subjects: &[i64]) -> Result<billing_core::erasure::ErasureReport, ErasureError> {
    let mut tx = pool.begin().await.unwrap();
    let result = {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        erase_account_billing(&mut billing, account, subjects, NOW).await
    };
    match &result {
        Ok(_) => tx.commit().await.unwrap(),
        Err(_) => tx.rollback().await.unwrap(),
    }
    result
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d9_erasure_locks_every_affected_owner_and_cascades_signed_cross_owner_and_null_package_rows() {
    let f = fixture(vec![]).await;
    let owner = owner().await;
    let (a, b, c) = (new_account(&f).await, new_account(&f).await, new_account(&f).await);
    let package_a = give_package(&f.pool, a, 80).await;
    let package_b = give_package(&f.pool, b, 50).await;
    let package_c = give_package(&f.pool, c, 40).await;
    let job = job_base() + 1;
    let unrelated = job + 1;
    import_usage(&[
        (a, Some(package_a), job, 30),       // the account's own usage
        (a, Some(package_b), unrelated, 10), // the account's usage drawn on B's package: B is affected
        (b, Some(package_c), job, -15),      // the account's job, naming B, on C's package: C is affected
        (a, None, job, 5),                   // no package at all
        (b, Some(package_a), unrelated, 7),  // B's row on the account's package: goes with that package
        (b, Some(package_b), unrelated, 4),  // B's own row on B's package: stays
    ])
    .await;
    let mut tx = f.pool.begin().await.unwrap();
    {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        charge_strict(&mut billing, a, job, 10, NOW).await.unwrap();
    }
    tx.commit().await.unwrap();
    // Personal billing profile and a payment, as imported data.
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE billing_import").execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO billing.billing_info (account_id, full_name) VALUES ($1, 'Kept Profile')")
        .bind(a.0)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO billing.payments (account_id, amount, currency, payment_gateway, status, created)
         VALUES ($1, 14.90, 'USD', 'iyzico', 'completed', now())",
    )
    .bind(a.0)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(open_balance(&f.pool, b).await, Some(70 - 10 - 4));
    assert_eq!(open_balance(&f.pool, c).await, Some(60 + 15));

    let status = open_account_erasure(&*f.store, a, NOW).await.unwrap();
    assert!(status.subscriptions_stopped() && status.billing_erased_at.is_none());
    // From here ordinary billing refuses the account: no charge, no grant, no new trial.
    let mut tx = f.pool.begin().await.unwrap();
    let refused = {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        charge_strict(&mut billing, a, job + 2, 1, NOW).await
    };
    drop(tx);
    assert!(matches!(&refused, Err(ChargeError::Store(StoreError::AccountErased(r))) if *r == a), "{refused:?}");

    let mut expected = vec![a, b, c];
    expected.sort();
    let report = erase_in_host_tx(&f.pool, a, &[job]).await.unwrap();
    assert_eq!(report.owners_locked, expected, "the account and every owner whose package a removed row drew on, ascending");
    assert_eq!(report.counts.imported_usage, 5, "every imported row of the account, its job and its packages");
    assert_eq!(report.counts.runtime_usage, 1);
    assert_eq!(report.counts.payments, 1);
    assert_eq!(report.counts.packages, 2, "the free package and the paid one");
    assert_eq!(report.counts.billing_info_detached, 1);
    assert!(report.counts.outbox_events >= 1, "at least the signup grant event");

    assert_eq!(open_balance(&f.pool, b).await, Some(70 - 4), "B gets back what the account drew from it");
    assert_eq!(open_balance(&f.pool, c).await, Some(60), "the negative row's credit leaves C with the account's job");
    for (sql, expected) in [
        ("SELECT count(*) FROM billing.accounts WHERE account_id = $1", 0i64),
        ("SELECT count(*) FROM billing.payments WHERE account_id = $1", 0),
        ("SELECT count(*) FROM billing.packages WHERE account_id = $1", 0),
        ("SELECT count(*) FROM billing.usage WHERE account_id = $1", 0),
        ("SELECT count(*) FROM billing.legacy_usage WHERE account_id = $1", 0),
        ("SELECT count(*) FROM billing.outbox WHERE account_id = $1", 0),
        ("SELECT count(*) FROM billing.erasure_barriers WHERE account_id = $1 AND billing_erased_at IS NOT NULL", 1),
    ] {
        assert_eq!(count(&owner, sql, a).await, expected, "{sql}");
    }
    let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM billing.billing_info WHERE full_name = 'Kept Profile' AND account_id IS NULL")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert!(kept >= 1, "D5: the billing profile stays, without its owner link");

    // The barrier survives: provisioning is refused as erased and never grants again, and a
    // repeated step is a no-op.
    assert_eq!(f.billing.provision(customer(a, &unique_email(), NOW)).await.unwrap_err(), BillingError::AccountErased(a));
    assert_eq!(count(&owner, "SELECT count(*) FROM billing.packages WHERE account_id = $1", a).await, 0);
    let repeat = erase_in_host_tx(&f.pool, a, &[job]).await.unwrap();
    assert!(repeat.owners_locked.is_empty() && repeat.counts == Default::default());
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d9_def_001_a_debit_of_an_affected_owner_waits_for_the_erasure_cascade_and_spends_what_it_returns() {
    let f = fixture(vec![]).await;
    let owner = owner().await;
    let (a, b) = (new_account(&f).await, new_account(&f).await);
    let package_b = give_package(&f.pool, b, 80).await; // B holds 100
    import_usage(&[(a, Some(package_b), job_base() + 3, 30)]).await; // the account drew 30 from B
    assert_eq!(open_balance(&f.pool, b).await, Some(70), "a debit of 90 would be refused before the erasure");
    open_account_erasure(&*f.store, a, NOW).await.unwrap();

    let (erased_tx, erased_rx) = oneshot::channel();
    let (commit_tx, commit_rx) = oneshot::channel::<()>();
    let pool = f.pool.clone();
    let erasure = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let report = {
            let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
            erase_account_billing(&mut billing, a, &[], NOW).await.unwrap()
        };
        erased_tx.send(report).unwrap();
        commit_rx.await.unwrap();
        tx.commit().await.unwrap();
    });
    let report = erased_rx.await.unwrap();
    assert!(report.owners_locked.contains(&b));

    let (pid_tx, pid_rx) = oneshot::channel();
    let pool = f.pool.clone();
    let debit = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *tx).await.unwrap();
        pid_tx.send(pid).unwrap();
        let result: Result<Vec<Allocation>, ChargeError> = {
            let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
            charge_strict(&mut billing, b, job_base() + 4, 90, NOW).await
        };
        match &result {
            Ok(_) => tx.commit().await.unwrap(),
            Err(_) => tx.rollback().await.unwrap(),
        }
        result
    });
    let pid = pid_rx.await.unwrap();
    let mut waiting = false;
    for _ in 0..100 {
        waiting = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE pid = $1 AND wait_event_type = 'Lock')")
            .bind(pid)
            .fetch_one(&owner)
            .await
            .unwrap();
        if waiting {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    commit_tx.send(()).unwrap();
    erasure.await.unwrap();
    let taken = debit.await.unwrap();
    assert!(waiting, "the affected owner's debit waits on its account row, locked by the erasure");
    assert_eq!(taken.unwrap().iter().map(|t| t.minutes).sum::<i64>(), 90, "after the cascade B holds 100 again");
    assert_eq!(open_balance(&f.pool, b).await, Some(10));
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_the_cascade_waits_until_every_verified_subscription_is_confirmed_stopped() {
    let f = fixture(vec![]).await;
    let owner = owner().await;
    let a = new_account(&f).await;
    let tag = Uuid::new_v4().simple().to_string();
    let (subscription, customer) = (format!("sub-{tag}"), format!("cust-{tag}"));
    import_plan_payment(a, "completed", Some((&subscription, &customer))).await;

    let status = open_account_erasure(&*f.store, a, NOW).await.unwrap();
    assert_eq!(status.pending.len(), 1);
    assert_eq!(status.pending[0].subscription_ref, subscription);
    assert!(matches!(erase_in_host_tx(&f.pool, a, &[]).await, Err(ErasureError::SubscriptionsNotStopped { .. })));

    // No configured gateway: pending. A timeout, then a still-renewing detail: pending.
    let none: [&dyn SubscriptionTermination; 0] = [];
    assert_eq!(terminate_subscriptions(&*f.store, &none, a, NOW).await.unwrap().pending.len(), 1);
    let iyzico = ScriptedTermination::new(vec![
        Err(GatewayError::Uncertain("timeout".into())),
        Ok(TerminationEvidence::Detail { subscription_ref: subscription.clone(), customer_ref: Some(customer.clone()), status: Some("ACTIVE".into()) }),
        Ok(TerminationEvidence::CancelSucceeded),
    ]);
    let terminators: [&dyn SubscriptionTermination; 1] = [&iyzico];
    assert_eq!(terminate_subscriptions(&*f.store, &terminators, a, NOW).await.unwrap().pending.len(), 1);
    assert_eq!(terminate_subscriptions(&*f.store, &terminators, a, NOW).await.unwrap().pending.len(), 1);
    assert!(matches!(erase_in_host_tx(&f.pool, a, &[]).await, Err(ErasureError::SubscriptionsNotStopped { .. })));
    // The documented cancel success confirms it.
    let status = terminate_subscriptions(&*f.store, &terminators, a, NOW).await.unwrap();
    assert!(status.subscriptions_stopped());
    assert_eq!(iyzico.asked.lock().unwrap().len(), 3, "only pending subscriptions are asked, once per pass");
    let (attempts, outcome): (i32, String) = sqlx::query_as(
        "SELECT attempts, last_outcome FROM billing.erasure_terminations WHERE subscription_ref = $1",
    )
    .bind(&subscription)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!((attempts, outcome.as_str()), (4, "confirmed"));

    let report = erase_in_host_tx(&f.pool, a, &[]).await.unwrap();
    assert_eq!(report.counts.subscriptions, 1);
    let kept: (Option<String>, String) =
        sqlx::query_as("SELECT customer_ref, status FROM billing.erasure_terminations WHERE subscription_ref = $1")
            .bind(&subscription)
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(kept, (None, "confirmed".to_string()), "only identifiers and outcomes survive (D9)");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_every_plan_checkout_no_target_covers_keeps_erasure_pending_whatever_its_local_status() {
    let f = fixture(vec![]).await;
    let a = new_account(&f).await;
    let tag = Uuid::new_v4().simple().to_string();
    import_plan_payment(a, "completed", Some((&format!("sub-{tag}"), &format!("cust-{tag}")))).await;
    // A lost or uncertain confirmation, a later authenticated completion of a failed or
    // canceled checkout, or a form still payable may each have created a renewing subscription.
    let mut unbound = vec![
        import_plan_payment(a, "pending", None).await,
        import_plan_payment(a, "failed", None).await,
        import_plan_payment(a, "canceled", None).await,
        import_plan_payment(a, "completed", None).await,
    ];
    unbound.sort();
    open_account_erasure(&*f.store, a, NOW).await.unwrap();
    let iyzico = ScriptedTermination::new(vec![Ok(TerminationEvidence::CancelSucceeded)]);
    let terminators: [&dyn SubscriptionTermination; 1] = [&iyzico];
    let status = terminate_subscriptions(&*f.store, &terminators, a, NOW).await.unwrap();
    assert!(status.pending.is_empty(), "the known subscription is confirmed stopped");
    assert_eq!(status.unbound_plan_payments, unbound, "every checkout without a covering target");
    match erase_in_host_tx(&f.pool, a, &[]).await {
        Err(ErasureError::SubscriptionsNotStopped { unbound_plan_payments, .. }) => assert_eq!(unbound_plan_payments, unbound),
        other => panic!("{other:?}"),
    }
    let deleted = sqlx::query("DELETE FROM billing.accounts WHERE account_id = $1").bind(a.0).execute(&f.pool).await.unwrap();
    assert_eq!(deleted.rows_affected(), 0, "row security also refuses while a checkout is uncovered");
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.accounts WHERE account_id = $1", a).await, 1, "nothing was erased");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_a_binding_that_appears_after_the_barrier_is_pending_until_its_own_termination_is_confirmed() {
    let f = fixture(vec![]).await;
    let a = new_account(&f).await;
    let status = open_account_erasure(&*f.store, a, NOW).await.unwrap();
    assert!(status.subscriptions_stopped());
    let tag = Uuid::new_v4().simple().to_string();
    let (subscription, customer) = (format!("sub-{tag}"), format!("cust-{tag}"));
    import_plan_payment(a, "completed", Some((&subscription, &customer))).await;

    let none: [&dyn SubscriptionTermination; 0] = [];
    let status = terminate_subscriptions(&*f.store, &none, a, NOW).await.unwrap();
    assert_eq!(status.pending.iter().map(|t| t.subscription_ref.as_str()).collect::<Vec<_>>(), vec![subscription.as_str()]);
    assert!(status.unbound_plan_payments.is_empty(), "its payment is covered by the adopted target");
    assert!(matches!(erase_in_host_tx(&f.pool, a, &[]).await, Err(ErasureError::SubscriptionsNotStopped { .. })));
    let deleted = sqlx::query("DELETE FROM billing.accounts WHERE account_id = $1").bind(a.0).execute(&f.pool).await.unwrap();
    assert_eq!(deleted.rows_affected(), 0, "presence in billing.subscriptions is not coverage");

    let iyzico = ScriptedTermination::new(vec![Ok(TerminationEvidence::CancelSucceeded)]);
    let terminators: [&dyn SubscriptionTermination; 1] = [&iyzico];
    assert!(terminate_subscriptions(&*f.store, &terminators, a, NOW).await.unwrap().subscriptions_stopped());
    assert_eq!(iyzico.asked.lock().unwrap()[0].customer_ref.as_deref(), Some(customer.as_str()));
    assert_eq!(erase_in_host_tx(&f.pool, a, &[]).await.unwrap().counts.subscriptions, 1);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_an_open_barrier_refuses_a_new_checkout_payment() {
    let f = fixture(vec![]).await;
    let a = new_account(&f).await;
    open_account_erasure(&*f.store, a, NOW).await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    let refused = {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing
            .insert_payment(&billing_core::model::NewPayment {
                account: a,
                amount: "120".parse().unwrap(),
                currency: "USD".into(),
                gateway: "iyzico".into(),
                environment: "sandbox".into(),
                token: Some(format!("tok-{}", Uuid::new_v4().simple())),
                created: NOW,
                status: billing_core::grants::PaymentStatus::Pending,
                metadata: serde_json::json!({}),
                coupon_id: None,
                pricing_plan_id: Some("plan-usd-premium-yearly".into()),
                provider_order_ref: None,
                subscription_ref: None,
                customer_id: None,
            })
            .await
    };
    drop(tx);
    assert_eq!(refused, Err(StoreError::AccountErased(a)), "a typed refusal, not a storage conflict");
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1", a).await, 0);
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_a_callback_waiting_on_the_account_lock_sees_the_barrier_committed_while_it_waited() {
    let f = fixture(vec![]).await;
    let owner = owner().await;
    let a = new_account(&f).await;

    let mut opening = f.pool.begin().await.unwrap();
    {
        let mut billing = PgBillingTx::attach(&mut opening).await.unwrap();
        billing.open_erasure(a, NOW).await.unwrap();
    }

    let (pid_tx, pid_rx) = oneshot::channel();
    let pool = f.pool.clone();
    let callback = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *tx).await.unwrap();
        pid_tx.send(pid).unwrap();
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        billing.lock_account(a).await
    });
    let pid = pid_rx.await.unwrap();
    let mut waiting = false;
    for _ in 0..100 {
        waiting = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE pid = $1 AND wait_event_type = 'Lock')")
            .bind(pid)
            .fetch_one(&owner)
            .await
            .unwrap();
        if waiting {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    opening.commit().await.unwrap();
    let locked = callback.await.unwrap();
    assert!(waiting, "the callback's account lock waits behind the opening erasure");
    assert_eq!(
        locked,
        Err(StoreError::AccountErased(a)),
        "no ordinary change may follow a barrier committed while the lock was awaited"
    );
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d9_the_runtime_can_delete_an_account_or_its_outbox_only_behind_a_barrier_and_cannot_clear_a_completed_one() {
    let f = fixture(vec![]).await;
    let a = new_account(&f).await;
    // No barrier: the runtime's DELETE right reaches nothing.
    for sql in ["DELETE FROM billing.accounts WHERE account_id = $1", "DELETE FROM billing.outbox WHERE account_id = $1"] {
        let deleted = sqlx::query(sql).bind(a.0).execute(&f.pool).await.unwrap().rows_affected();
        assert_eq!(deleted, 0, "{sql}");
    }
    // A barrier with a pending termination still protects the account row.
    let tag = Uuid::new_v4().simple().to_string();
    import_plan_payment(a, "completed", Some((&format!("sub-{tag}"), &format!("cust-{tag}")))).await;
    open_account_erasure(&*f.store, a, NOW).await.unwrap();
    let deleted = sqlx::query("DELETE FROM billing.accounts WHERE account_id = $1").bind(a.0).execute(&f.pool).await.unwrap();
    assert_eq!(deleted.rows_affected(), 0, "not while a subscription may renew");

    // A finished erasure's stamp cannot be cleared to reopen the account for recreation.
    let b = new_account(&f).await;
    open_account_erasure(&*f.store, b, NOW).await.unwrap();
    erase_in_host_tx(&f.pool, b, &[]).await.unwrap();
    let cleared = sqlx::query("UPDATE billing.erasure_barriers SET billing_erased_at = NULL WHERE account_id = $1")
        .bind(b.0)
        .execute(&f.pool)
        .await;
    assert!(
        matches!(&cleared, Ok(r) if r.rows_affected() == 0) || cleared.is_err(),
        "the completion stamp is write-once: {cleared:?}"
    );
    let recreate = sqlx::query("INSERT INTO billing.accounts (account_id, created_at) VALUES ($1, now())").bind(b.0).execute(&f.pool).await;
    assert!(recreate.is_err(), "row security refuses recreating an erased account");
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.erasure_barriers WHERE account_id = $1 AND billing_erased_at IS NOT NULL", b).await, 1);
    // The barrier cannot be removed at all.
    let removed = sqlx::query("DELETE FROM billing.erasure_barriers WHERE account_id = $1").bind(b.0).execute(&f.pool).await;
    let code = removed.unwrap_err().as_database_error().and_then(|d| d.code().map(|c| c.to_string())).unwrap_or_default();
    assert_eq!(code, "42501");
}

#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d9_erasure_needs_an_open_barrier_and_refuses_an_unknown_order_of_steps() {
    let f = fixture(vec![]).await;
    let a = new_account(&f).await;
    assert!(matches!(erase_in_host_tx(&f.pool, a, &[]).await, Err(ErasureError::NotOpened)));
    // An account billing never provisioned still gets a barrier, so it can never be provisioned later.
    let stranger = account();
    let status = open_account_erasure(&*f.store, stranger, NOW).await.unwrap();
    assert!(status.subscriptions_stopped());
    erase_in_host_tx(&f.pool, stranger, &[]).await.unwrap();
    f.directory.add(customer(stranger, &unique_email(), NOW));
    // Recreation is refused as erased, before and after the cascade, never answered as a quiet no-grant.
    assert_eq!(
        f.billing.provision(customer(stranger, &unique_email(), NOW)).await.unwrap_err(),
        BillingError::AccountErased(stranger)
    );
    assert_eq!(f.billing.ensure_provisioned(stranger).await.unwrap_err(), BillingError::AccountErased(stranger));
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.packages WHERE account_id = $1", stranger).await, 0);
    let _unused: Option<StartRefusal> = None;
    let _arc: Option<Arc<()>> = None;
}

fn yearly_plan() -> OrderRequest {
    OrderRequest { package_id: Some(1), ui_locale: "en".into(), gateway: "iyzico".into(), ..OrderRequest::default() }
}

fn lemon_hours(hours: i64) -> OrderRequest {
    OrderRequest { pay_as_you_go_hours: Some(hours), ui_locale: "en".into(), gateway: "lemonsqueezy".into(), ..OrderRequest::default() }
}

/// Opens the account's erasure while the provider initialization is in flight, and
/// optionally runs the whole cascade too, then answers with a payable form. A
/// deliberate test adapter.
struct OpensErasureMidStart {
    name: &'static str,
    environment: &'static str,
    armed: Mutex<Option<Armed>>,
    starts: Mutex<u32>,
}

#[derive(Clone)]
struct Armed {
    store: Arc<PgBillingStore>,
    pool: PgPool,
    account: AccountId,
    complete_erasure: bool,
}

impl OpensErasureMidStart {
    fn new(name: &'static str, environment: &'static str) -> Arc<OpensErasureMidStart> {
        Arc::new(OpensErasureMidStart { name, environment, armed: Mutex::default(), starts: Mutex::default() })
    }

    fn arm(&self, f: &Fixture, account: AccountId, complete_erasure: bool) {
        *self.armed.lock().unwrap() = Some(Armed { store: f.store.clone(), pool: f.pool.clone(), account, complete_erasure });
    }
}

impl CheckoutGateway for OpensErasureMidStart {
    fn gateway(&self) -> &str {
        self.name
    }

    fn environment(&self) -> &str {
        self.environment
    }

    fn start<'a>(&'a self, _request: &'a CheckoutStart) -> BoxFuture<'a, Result<CheckoutStarted, GatewayError>> {
        Box::pin(async move {
            *self.starts.lock().unwrap() += 1;
            let armed = self.armed.lock().unwrap().clone().expect("the adapter is armed with the account");
            open_account_erasure(&*armed.store, armed.account, NOW).await.unwrap();
            if armed.complete_erasure {
                erase_in_host_tx(&armed.pool, armed.account, &[]).await.unwrap();
            }
            Ok(CheckoutStarted {
                token: format!("tok-{}", Uuid::new_v4().simple()),
                form_content: Some("<form/>".into()),
                url: Some("https://pay.example.test/checkout".into()),
            })
        })
    }

    fn result<'a>(&'a self, _token: &'a str, _kind: CheckoutKind) -> BoxFuture<'a, Result<CheckoutResult, GatewayError>> {
        Box::pin(std::future::ready(Err(GatewayError::Uncertain("not scripted".into()))))
    }
}

// D10 admission: a barrier already open refuses checkout before any provider initialization.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_an_open_barrier_refuses_checkout_before_the_provider_is_initialized() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let a = new_account(&f).await;
    open_account_erasure(&*f.store, a, NOW).await.unwrap();
    gateway.will_start(Ok(started(&format!("tok-{}", Uuid::new_v4().simple()))));
    assert_eq!(f.billing.start_checkout(a, &yearly_plan()).await.unwrap_err(), BillingError::AccountErased(a));
    assert!(gateway.started.lock().unwrap().is_empty(), "no provider initialization behind a barrier");
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1", a).await, 0);
}

// D10 admission race: an Iyzico form initialized while the barrier opened, or while the
// whole erasure ran and removed the account row, is neither stored nor exposed, and the
// answer is the erasure refusal in both cases, never an unknown account.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_an_iyzico_form_initialized_while_the_barrier_opened_is_neither_stored_nor_exposed() {
    let gateway = OpensErasureMidStart::new("iyzico", test_env("sandbox"));
    let g = gateway.clone();
    let f = fixture_with(vec![], move |b| b.with_checkout(g)).await;
    let a = new_account(&f).await;
    gateway.arm(&f, a, false);
    assert_eq!(f.billing.start_checkout(a, &yearly_plan()).await.unwrap_err(), BillingError::AccountErased(a));
    assert_eq!(*gateway.starts.lock().unwrap(), 1);
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1", a).await, 0);

    let b = new_account(&f).await;
    gateway.arm(&f, b, true);
    assert_eq!(f.billing.start_checkout(b, &yearly_plan()).await.unwrap_err(), BillingError::AccountErased(b));
    assert_eq!(*gateway.starts.lock().unwrap(), 2);
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.accounts WHERE account_id = $1", b).await, 0, "the cascade ran");
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1", b).await, 0);
}

// D10 post-network save: a LemonSqueezy URL obtained after the barrier opened is not saved or exposed.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_a_lemonsqueezy_url_obtained_after_the_barrier_opened_is_neither_saved_nor_exposed() {
    let gateway = OpensErasureMidStart::new("lemonsqueezy", test_env("test"));
    let g = gateway.clone();
    let f = fixture_with(vec![], move |b| b.with_checkout(g)).await;
    let a = new_account(&f).await;
    gateway.arm(&f, a, false);
    assert_eq!(f.billing.start_checkout(a, &lemon_hours(5)).await.unwrap_err(), BillingError::AccountErased(a));
    assert_eq!(*gateway.starts.lock().unwrap(), 1);
    // The payment written before the call stays pending as correlation, with no provider token saved.
    assert_eq!(
        count(&f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1 AND status = 'pending' AND token IS NULL", a).await,
        1
    );
}

// D10 late evidence: an unfinished plan checkout stays unresolved through failure and
// uncertainty; an authenticated success during erasure becomes a termination target
// with no grant, no completion and no binding, and then the usual cancellation finishes it.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_a_late_authenticated_plan_result_during_erasure_becomes_a_termination_target_without_any_grant() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let f = fixture(vec![gateway.clone()]).await;
    let owner = owner().await;
    let a = new_account(&f).await;
    let tag = Uuid::new_v4().simple().to_string();
    let (token, subscription, customer) = (format!("tok-{tag}"), format!("sub-{tag}"), format!("cust-{tag}"));
    gateway.will_start(Ok(started(&token)));
    let view = f.billing.start_checkout(a, &yearly_plan()).await.unwrap();
    let status = open_account_erasure(&*f.store, a, NOW).await.unwrap();
    assert_eq!(status.unbound_plan_payments, vec![view.payment_id], "the exposed form may still create a subscription");

    // A payment-status failure and an uncertain lookup prove nothing about the form: still unresolved.
    gateway.will_answer(Ok(CheckoutResult::Failure { raw_status: "FAILURE".into() }));
    assert_eq!(f.billing.confirm_checkout("iyzico", &token).await.unwrap_err(), BillingError::AccountErased(a));
    gateway.will_answer(Err(GatewayError::Uncertain("timeout".into())));
    assert!(f.billing.confirm_checkout("iyzico", &token).await.is_err());
    let none: [&dyn SubscriptionTermination; 0] = [];
    let status = terminate_subscriptions(&*f.store, &none, a, NOW).await.unwrap();
    assert_eq!(status.unbound_plan_payments, vec![view.payment_id]);
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1 AND status = 'pending'", a).await, 1);

    // The admin reconciliation obtains the authenticated success: a target, nothing else.
    gateway.will_answer(Ok(success(&customer, Some(&subscription))));
    assert_eq!(f.billing.reconcile_payment(view.payment_id).await.unwrap_err(), BillingError::AccountErased(a));
    let status = terminate_subscriptions(&*f.store, &none, a, NOW).await.unwrap();
    assert!(status.unbound_plan_payments.is_empty(), "the checkout is now covered by its target");
    assert_eq!(
        status.pending,
        vec![TerminationTarget {
            gateway: "iyzico".into(),
            environment: "sandbox".into(),
            subscription_ref: subscription.clone(),
            customer_ref: Some(customer.clone()),
        }]
    );
    for sql in [
        "SELECT count(*) FROM billing.packages WHERE account_id = $1 AND payment_id IS NOT NULL",
        "SELECT count(*) FROM billing.subscriptions WHERE account_id = $1",
        "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind IN ('PaymentCompleted', 'PaymentFailed')",
    ] {
        assert_eq!(count(&f.pool, sql, a).await, 0, "{sql}");
    }
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1 AND status = 'pending'", a).await, 1);
    assert!(matches!(erase_in_host_tx(&f.pool, a, &[]).await, Err(ErasureError::SubscriptionsNotStopped { .. })));

    let iyzico = ScriptedTermination::new(vec![Ok(TerminationEvidence::CancelSucceeded)]);
    let terminators: [&dyn SubscriptionTermination; 1] = [&iyzico];
    assert!(terminate_subscriptions(&*f.store, &terminators, a, NOW).await.unwrap().subscriptions_stopped());
    assert_eq!(iyzico.asked.lock().unwrap()[0].subscription_ref, subscription);
    erase_in_host_tx(&f.pool, a, &[]).await.unwrap();
    let kept: Option<String> = sqlx::query_scalar("SELECT customer_ref FROM billing.erasure_terminations WHERE subscription_ref = $1")
        .bind(&subscription)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(kept, None, "only identifiers and outcomes survive (D9)");
}

// D9: provisioning and reads of an erasing account answer the typed refusal, never a grant.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d9_an_open_barrier_refuses_ordinary_billing_access_with_the_typed_error() {
    let f = fixture(vec![]).await;
    let a = new_account(&f).await;
    open_account_erasure(&*f.store, a, NOW).await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        assert_eq!(billing.lock_account(a).await, Err(StoreError::AccountErased(a)));
    }
    drop(tx);
    let mut tx = f.pool.begin().await.unwrap();
    {
        let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
        assert_eq!(billing.create_account(a, NOW).await, Err(StoreError::AccountErased(a)));
    }
    drop(tx);
    assert_eq!(f.billing.overview(a).await.unwrap_err(), BillingError::AccountErased(a));
    let stranger = account();
    open_account_erasure(&*f.store, stranger, NOW).await.unwrap();
    f.directory.add(customer(stranger, &unique_email(), NOW));
    assert_eq!(f.billing.ensure_provisioned(stranger).await.unwrap_err(), BillingError::AccountErased(stranger));
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.packages WHERE account_id = $1", stranger).await, 0);
}

fn verified_closure() -> CheckoutClosure {
    CheckoutClosure {
        resolved_by: "ops@example.test".into(),
        checkout_cannot_complete: true,
        no_renewing_subscription: true,
        evidence_ref: "IYZ-TICKET-42".into(),
        rationale: "provider record: token expired, no subscription for the customer".into(),
    }
}

// D10 verified closure: no risk waiver. An unpaid snapshot or either attestation alone is
// refused and the checkout stays pending; only a closure verifying both facts with
// retained evidence, for an account under erasure and an uncovered plan checkout, closes it.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_only_a_verified_operator_closure_closes_an_abandoned_checkout_and_nothing_else() {
    let gateway = ScriptedGateway::new("iyzico", test_env("sandbox"));
    let f = fixture(vec![gateway.clone()]).await;
    let owner = owner().await;
    let (a, b) = (new_account(&f).await, new_account(&f).await);
    let tag = Uuid::new_v4().simple().to_string();
    gateway.will_start(Ok(started(&format!("tok-a-{tag}"))));
    let abandoned = f.billing.start_checkout(a, &yearly_plan()).await.unwrap().payment_id;
    gateway.will_start(Ok(started(&format!("tok-b-{tag}"))));
    let not_erasing = f.billing.start_checkout(b, &yearly_plan()).await.unwrap().payment_id;

    // No barrier: nothing to close, and nothing changes.
    assert!(matches!(f.billing.resolve_erasure_checkout(not_erasing, &verified_closure()).await, Err(BillingError::Conflict(_))));
    open_account_erasure(&*f.store, a, NOW).await.unwrap();
    let never_paid = CheckoutClosure {
        checkout_cannot_complete: false,
        no_renewing_subscription: false,
        evidence_ref: String::new(),
        rationale: "Iyzico console: form never paid".into(),
        ..verified_closure()
    };
    for refused in [
        never_paid,
        CheckoutClosure { checkout_cannot_complete: false, ..verified_closure() },
        CheckoutClosure { no_renewing_subscription: false, ..verified_closure() },
        CheckoutClosure { evidence_ref: " ".into(), ..verified_closure() },
        CheckoutClosure { resolved_by: String::new(), ..verified_closure() },
    ] {
        assert!(matches!(f.billing.resolve_erasure_checkout(abandoned, &refused).await, Err(BillingError::InvalidRequest(_))), "{refused:?}");
    }
    let none: [&dyn SubscriptionTermination; 0] = [];
    assert_eq!(terminate_subscriptions(&*f.store, &none, a, NOW).await.unwrap().unbound_plan_payments, vec![abandoned], "still pending");

    let resolved = f.billing.resolve_erasure_checkout(abandoned, &verified_closure()).await.unwrap();
    assert!(resolved.remaining_unbound.is_empty() && resolved.billing_blockers_cleared, "{resolved:?}");
    // Repeating it is refused: the checkout is no longer unresolved.
    assert!(matches!(f.billing.resolve_erasure_checkout(abandoned, &verified_closure()).await, Err(BillingError::Conflict(_))));
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.packages WHERE account_id = $1 AND payment_id IS NOT NULL", a).await, 0);
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.payments WHERE account_id = $1 AND status = 'pending'", a).await, 1);

    erase_in_host_tx(&f.pool, a, &[]).await.unwrap();
    // The closure outlives the cascade with identifiers, attestations and the statement only.
    let audit: (i64, String, bool, bool, String) = sqlx::query_as(
        "SELECT payment_id, resolved_by, checkout_cannot_complete, no_renewing_subscription, evidence_ref
         FROM billing.erasure_checkout_resolutions WHERE account_id = $1",
    )
    .bind(a.0)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!(audit, (abandoned, "ops@example.test".into(), true, true, "IYZ-TICKET-42".into()));
    // The database itself refuses an unverified closure row.
    let unverified = sqlx::query(
        "INSERT INTO billing.erasure_checkout_resolutions
           (account_id, payment_id, payment_gateway, environment, resolved_by, checkout_cannot_complete,
            no_renewing_subscription, evidence_ref, rationale, resolved_at)
         VALUES ($1, -1, 'iyzico', 'sandbox', 'ops', false, true, 'x', 'never paid', now())",
    )
    .bind(a.0)
    .execute(&f.pool)
    .await;
    assert!(unverified.is_err(), "{unverified:?}");
    // The runtime cannot rewrite or remove the audit.
    for sql in [
        "UPDATE billing.erasure_checkout_resolutions SET rationale = 'x' WHERE account_id = $1",
        "DELETE FROM billing.erasure_checkout_resolutions WHERE account_id = $1",
    ] {
        let changed = sqlx::query(sql).bind(a.0).execute(&f.pool).await;
        assert!(matches!(&changed, Ok(r) if r.rows_affected() == 0) || changed.is_err(), "{sql}: {changed:?}");
    }
}

// D10 coverage is per account: another account's termination target for the same
// subscription reference never covers this account's legacy plan payment.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_a_foreign_accounts_target_never_covers_this_accounts_plan_reference() {
    let f = fixture(vec![]).await;
    let owner = owner().await;
    let (a, b) = (new_account(&f).await, new_account(&f).await);
    let tag = Uuid::new_v4().simple().to_string();
    let (subscription, customer) = (format!("sub-{tag}"), format!("cust-{tag}"));
    import_plan_payment(b, "completed", Some((&subscription, &customer))).await;
    // A legacy, ambiguous row of A that names B's subscription.
    let ambiguous = import_plan_payment(a, "completed", None).await;
    sqlx::query("UPDATE billing.payments SET subscription_ref = $2 WHERE id = $1")
        .bind(ambiguous)
        .bind(&subscription)
        .execute(&owner)
        .await
        .unwrap();
    open_account_erasure(&*f.store, b, NOW).await.unwrap();
    let iyzico = ScriptedTermination::new(vec![Ok(TerminationEvidence::CancelSucceeded)]);
    let terminators: [&dyn SubscriptionTermination; 1] = [&iyzico];
    assert!(terminate_subscriptions(&*f.store, &terminators, b, NOW).await.unwrap().subscriptions_stopped());

    let status = open_account_erasure(&*f.store, a, NOW).await.unwrap();
    assert_eq!(status.unbound_plan_payments, vec![ambiguous], "B's confirmed target is not A's coverage");
    assert!(matches!(erase_in_host_tx(&f.pool, a, &[]).await, Err(ErasureError::SubscriptionsNotStopped { .. })));
    let deleted = sqlx::query("DELETE FROM billing.accounts WHERE account_id = $1").bind(a.0).execute(&f.pool).await.unwrap();
    assert_eq!(deleted.rows_affected(), 0, "row security also requires a target of the same account");
}

async fn token_of(owner: &PgPool, payment: i64) -> String {
    sqlx::query_scalar("SELECT token FROM billing.payments WHERE id = $1").bind(payment).fetch_one(owner).await.unwrap()
}

async fn payment_refs(owner: &PgPool, payment: i64) -> (String, Option<String>, Option<String>) {
    sqlx::query_as("SELECT status, subscription_ref, customer_id FROM billing.payments WHERE id = $1")
        .bind(payment)
        .fetch_one(owner)
        .await
        .unwrap()
}

async fn termination_row(owner: &PgPool, subscription: &str) -> (Uuid, Option<String>, String, i32) {
    sqlx::query_as("SELECT account_id, customer_ref, status, attempts FROM billing.erasure_terminations WHERE subscription_ref = $1")
        .bind(subscription)
        .fetch_one(owner)
        .await
        .unwrap()
}

// D10: a completed imported plan payment no target covers is looked up during erasure,
// and a second checkout naming the same subscription adopts nothing new and keeps the
// confirmed outcome and its attempts.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_a_completed_imported_plan_is_looked_up_and_a_repeated_identity_adopts_idempotently() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let f = fixture(vec![gateway.clone()]).await;
    let owner = owner().await;
    let a = new_account(&f).await;
    let tag = Uuid::new_v4().simple().to_string();
    let (subscription, customer) = (format!("sub-{tag}"), format!("cust-{tag}"));
    let imported = import_plan_payment(a, "completed", None).await;
    let retry_token = format!("tok-retry-{tag}");
    gateway.will_start(Ok(started(&retry_token)));
    let retried = f.billing.start_checkout(a, &yearly_plan()).await.unwrap().payment_id;
    open_account_erasure(&*f.store, a, NOW).await.unwrap();

    gateway.will_answer(Ok(success(&customer, Some(&subscription))));
    assert_eq!(f.billing.reconcile_payment(imported).await.unwrap_err(), BillingError::AccountErased(a));
    let imported_token = token_of(&owner, imported).await;
    assert_eq!(gateway.result_calls.lock().unwrap()[0].0, imported_token, "the completed row was asked about");
    assert_eq!(payment_refs(&owner, imported).await, ("completed".into(), Some(subscription.clone()), Some(customer.clone())));
    let iyzico = ScriptedTermination::new(vec![Ok(TerminationEvidence::CancelSucceeded)]);
    let terminators: [&dyn SubscriptionTermination; 1] = [&iyzico];
    let status = terminate_subscriptions(&*f.store, &terminators, a, NOW).await.unwrap();
    assert_eq!(status.unbound_plan_payments, vec![retried]);
    assert_eq!(termination_row(&owner, &subscription).await, (a.0, Some(customer.clone()), "confirmed".into(), 1));

    // The retried checkout turns out to be the same subscription and customer.
    gateway.will_answer(Ok(success(&customer, Some(&subscription))));
    assert_eq!(f.billing.confirm_checkout("iyzico", &retry_token).await.unwrap_err(), BillingError::AccountErased(a));
    assert_eq!(termination_row(&owner, &subscription).await, (a.0, Some(customer.clone()), "confirmed".into(), 1), "untouched");
    let status = terminate_subscriptions(&*f.store, &terminators, a, NOW).await.unwrap();
    assert!(status.subscriptions_stopped(), "{status:?}");
    assert_eq!(iyzico.asked.lock().unwrap().len(), 1, "a confirmed target is never asked again");
}

// D10: an identity the provider reports that collides with another account's target, a
// different customer on this account's target, or a different stored reference is refused,
// and the payment's correlation fields stay exactly as they were.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_colliding_late_identities_are_refused_and_change_nothing() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let f = fixture(vec![gateway.clone()]).await;
    let owner = owner().await;
    let (a, b) = (new_account(&f).await, new_account(&f).await);
    let tag = Uuid::new_v4().simple().to_string();
    let (foreign_sub, foreign_cust) = (format!("sub-b-{tag}"), format!("cust-b-{tag}"));
    let (own_sub, own_cust) = (format!("sub-a-{tag}"), format!("cust-a-{tag}"));
    import_plan_payment(b, "completed", Some((&foreign_sub, &foreign_cust))).await;
    open_account_erasure(&*f.store, b, NOW).await.unwrap();
    import_plan_payment(a, "completed", Some((&own_sub, &own_cust))).await;
    let stored = import_plan_payment(a, "failed", None).await;
    sqlx::query("UPDATE billing.payments SET subscription_ref = $2 WHERE id = $1")
        .bind(stored)
        .bind(format!("sub-stored-{tag}"))
        .execute(&owner)
        .await
        .unwrap();
    let mut tokens = Vec::new();
    let mut pending = Vec::new();
    for n in 0..2 {
        let token = format!("tok-{n}-{tag}");
        gateway.will_start(Ok(started(&token)));
        pending.push(f.billing.start_checkout(a, &yearly_plan()).await.unwrap().payment_id);
        tokens.push(token);
    }
    open_account_erasure(&*f.store, a, NOW).await.unwrap();

    // Another account's subscription.
    gateway.will_answer(Ok(success(&foreign_cust, Some(&foreign_sub))));
    assert!(matches!(f.billing.confirm_checkout("iyzico", &tokens[0]).await, Err(BillingError::Conflict(_))));
    assert_eq!(payment_refs(&owner, pending[0]).await, ("pending".into(), None, None));
    assert_eq!(termination_row(&owner, &foreign_sub).await.0, b.0);
    // This account's subscription, another customer.
    gateway.will_answer(Ok(success("cust-other", Some(&own_sub))));
    assert!(matches!(f.billing.confirm_checkout("iyzico", &tokens[1]).await, Err(BillingError::Conflict(_))));
    assert_eq!(payment_refs(&owner, pending[1]).await, ("pending".into(), None, None));
    assert_eq!(termination_row(&owner, &own_sub).await.1, Some(own_cust.clone()));
    // A stored reference the provider contradicts.
    gateway.will_answer(Ok(success(&own_cust, Some(&own_sub))));
    assert!(matches!(f.billing.reconcile_payment(stored).await, Err(BillingError::Conflict(_))));
    assert_eq!(payment_refs(&owner, stored).await, ("failed".into(), Some(format!("sub-stored-{tag}")), None));

    let none: [&dyn SubscriptionTermination; 0] = [];
    let mut unbound = vec![stored, pending[0], pending[1]];
    unbound.sort();
    assert_eq!(terminate_subscriptions(&*f.store, &none, a, NOW).await.unwrap().unbound_plan_payments, unbound);
}

// D10: a late result naming a subscription bound to another, live account (no erasure, no
// target) is a collision: no target is created for it, so nothing can cancel B's subscription,
// and A's payment keeps its correlation fields.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_a_late_result_naming_a_live_accounts_bound_subscription_adopts_nothing() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let f = fixture(vec![gateway.clone()]).await;
    let owner = owner().await;
    let (a, b) = (new_account(&f).await, new_account(&f).await);
    let tag = Uuid::new_v4().simple().to_string();
    let (live_sub, live_cust) = (format!("sub-live-{tag}"), format!("cust-live-{tag}"));
    import_plan_payment(b, "completed", Some((&live_sub, &live_cust))).await;
    let token = format!("tok-{tag}");
    gateway.will_start(Ok(started(&token)));
    let pending = f.billing.start_checkout(a, &yearly_plan()).await.unwrap().payment_id;
    open_account_erasure(&*f.store, a, NOW).await.unwrap();

    gateway.will_answer(Ok(success(&live_cust, Some(&live_sub))));
    assert!(matches!(f.billing.confirm_checkout("iyzico", &token).await, Err(BillingError::Conflict(_))));
    assert_eq!(payment_refs(&owner, pending).await, ("pending".into(), None, None));
    let targets: i64 = sqlx::query_scalar("SELECT count(*) FROM billing.erasure_terminations WHERE subscription_ref = $1")
        .bind(&live_sub)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(targets, 0, "B's live subscription never becomes a cancellation target");
    assert_eq!(count(&f.pool, "SELECT count(*) FROM billing.erasure_barriers WHERE account_id = $1", b).await, 0);
    let none: [&dyn SubscriptionTermination; 0] = [];
    assert_eq!(terminate_subscriptions(&*f.store, &none, a, NOW).await.unwrap().unbound_plan_payments, vec![pending]);
}

// D10, reverse direction: once A's erasure holds a cancellation-only target for a
// subscription, an ordinary confirmation by another account naming it is a conflict:
// nothing is granted, completed or bound, and A's target stays A's.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_an_ordinary_confirmation_naming_another_accounts_erasure_target_binds_and_grants_nothing() {
    let gateway = ScriptedGateway::new("iyzico", "sandbox");
    let f = fixture(vec![gateway.clone()]).await;
    let owner = owner().await;
    let (a, b) = (new_account(&f).await, new_account(&f).await);
    let tag = Uuid::new_v4().simple().to_string();
    let (subscription, customer) = (format!("sub-{tag}"), format!("cust-{tag}"));
    let (token_a, token_b) = (format!("tok-a-{tag}"), format!("tok-b-{tag}"));
    gateway.will_start(Ok(started(&token_a)));
    f.billing.start_checkout(a, &yearly_plan()).await.unwrap();
    gateway.will_start(Ok(started(&token_b)));
    let b_payment = f.billing.start_checkout(b, &yearly_plan()).await.unwrap().payment_id;
    open_account_erasure(&*f.store, a, NOW).await.unwrap();
    gateway.will_answer(Ok(success(&customer, Some(&subscription))));
    assert_eq!(f.billing.confirm_checkout("iyzico", &token_a).await.unwrap_err(), BillingError::AccountErased(a));
    assert_eq!(termination_row(&owner, &subscription).await.0, a.0);

    gateway.will_answer(Ok(success(&customer, Some(&subscription))));
    assert!(matches!(f.billing.confirm_checkout("iyzico", &token_b).await, Err(BillingError::Conflict(_))));
    assert_eq!(payment_refs(&owner, b_payment).await, ("pending".into(), None, None));
    for sql in [
        "SELECT count(*) FROM billing.packages WHERE account_id = $1 AND payment_id IS NOT NULL",
        "SELECT count(*) FROM billing.subscriptions WHERE account_id = $1",
        "SELECT count(*) FROM billing.outbox WHERE account_id = $1 AND kind = 'PaymentCompleted'",
    ] {
        assert_eq!(count(&f.pool, sql, b).await, 0, "{sql}");
    }
    assert_eq!(termination_row(&owner, &subscription).await, (a.0, Some(customer.clone()), "pending".into(), 0));
}

// D10, concurrent schedules: an ordinary binding by B and an erasure adoption by A of the
// same subscription serialize on the shared identity lock, so whichever commits first wins
// and the other is a conflict; the identity never ends up both bound to B and A's target.
#[tokio::test]
#[ignore = "needs BILLING_TEST_DATABASE_URL"]
async fn d10_concurrent_binding_and_adoption_of_one_identity_never_both_commit() {
    let f = fixture(vec![]).await;
    let owner = owner().await;
    for binding_first in [true, false] {
        let (a, b) = (new_account(&f).await, new_account(&f).await);
        let tag = Uuid::new_v4().simple().to_string();
        let (subscription, customer) = (format!("sub-{tag}"), format!("cust-{tag}"));
        let b_payment = import_plan_payment(b, "pending", None).await;
        open_account_erasure(&*f.store, a, NOW).await.unwrap();
        let target = TerminationTarget {
            gateway: "iyzico".into(),
            environment: "sandbox".into(),
            subscription_ref: subscription.clone(),
            customer_ref: Some(customer.clone()),
        };
        let binding = billing_core::model::NewSubscription {
            account: b,
            gateway: "iyzico".into(),
            environment: "sandbox".into(),
            subscription_ref: subscription.clone(),
            customer_ref: Some(customer.clone()),
            pricing_plan_id: Some("plan-usd-premium-yearly".into()),
            currency: "USD".into(),
            amount: "120".parse().unwrap(),
            spec: serde_json::from_value(serde_json::json!(
                {"name": "PREMIUM", "minutes": 1200, "count": 12, "interval": "month", "frequency": 1, "expiry": 1}
            ))
            .unwrap(),
            initial_payment_id: b_payment,
            created_at: NOW,
        };

        // The first transaction takes the identity lock and holds it until it commits.
        let mut first = f.pool.begin().await.unwrap();
        {
            let mut billing = PgBillingTx::attach(&mut first).await.unwrap();
            if binding_first {
                billing.insert_subscription(&binding).await.unwrap();
            } else {
                billing.lock_account_for_erasure(a).await.unwrap();
                billing.adopt_erasure_target(a, &target, NOW).await.unwrap();
            }
        }
        let (pid_tx, pid_rx) = oneshot::channel();
        let pool = f.pool.clone();
        let (second_binding, second_target) = (binding.clone(), target.clone());
        let second = tokio::spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *tx).await.unwrap();
            pid_tx.send(pid).unwrap();
            let result = {
                let mut billing = PgBillingTx::attach(&mut tx).await.unwrap();
                if binding_first {
                    billing.adopt_erasure_target(a, &second_target, NOW).await.map(|_| ())
                } else {
                    billing.insert_subscription(&second_binding).await.map(|_| ())
                }
            };
            if result.is_ok() {
                tx.commit().await.unwrap();
            }
            result
        });
        let pid = pid_rx.await.unwrap();
        let mut waiting = false;
        for _ in 0..100 {
            waiting = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE pid = $1 AND wait_event_type = 'Lock')")
                .bind(pid)
                .fetch_one(&owner)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        first.commit().await.unwrap();
        let second = second.await.unwrap();
        assert!(waiting, "the second writer waits on the identity lock (binding first: {binding_first})");
        assert!(matches!(&second, Err(StoreError::Conflict(_))), "binding first: {binding_first}: {second:?}");
        let bound: i64 = sqlx::query_scalar("SELECT count(*) FROM billing.subscriptions WHERE subscription_ref = $1")
            .bind(&subscription)
            .fetch_one(&owner)
            .await
            .unwrap();
        let targets: i64 = sqlx::query_scalar("SELECT count(*) FROM billing.erasure_terminations WHERE subscription_ref = $1")
            .bind(&subscription)
            .fetch_one(&owner)
            .await
            .unwrap();
        assert_eq!((bound, targets), if binding_first { (1, 0) } else { (0, 1) }, "binding first: {binding_first}");
    }
}
