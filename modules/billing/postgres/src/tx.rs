//! `BillingTx` over a Postgres connection.
//!
//! One implementation serves both seams: a transaction billing opened itself
//! (`PgBillingTx<Transaction<'static, Postgres>>`, a `UnitOfWork`) and a
//! connection a host already holds inside its own transaction
//! (`PgBillingTx<&mut PgConnection>`, the seam P3 composes job status
//! compare-and-swap and charges through). Whoever opened the transaction
//! commits it.

use std::ops::DerefMut;

use billing_core::decimal::Decimal;
use billing_core::erasure::{CheckoutClosure, ErasureCounts, ErasureStatus, TerminationTarget};
use billing_core::grants::PaymentStatus;
use billing_core::ledger::{Package, PackageBalance};
use billing_core::model::{
    BillingEvent, BillingInfo, NewPackage, NewPayment, NewSubscription, NewUnresolved, NewUsage, Payment,
    PaymentInsert, PaymentUpdate, Subscription, UnresolvedNotification,
};
use billing_core::ports::{BillingTx, StoreError, StoreResult, UnitOfWork};
use billing_core::pricing::Coupon;
use billing_core::types::{AccountId, BoxFuture, Micros, PackageId, PaymentId, SubjectRef};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::types::Json;
use sqlx::{PgConnection, Postgres, Row, Transaction};
use uuid::Uuid;

pub struct PgBillingTx<C>(pub(crate) C);

pub(crate) fn db(e: sqlx::Error) -> StoreError {
    if let Some(d) = e.as_database_error() && let Some(code) = d.code() && matches!(code.as_ref(), "23505" | "23503" | "23514" | "23502") {
        return StoreError::Conflict(d.message().to_string());
    }
    StoreError::Unavailable(e.to_string())
}

fn corrupt(what: &str, e: impl std::fmt::Display) -> StoreError {
    StoreError::Unavailable(format!("stored {what} is unreadable: {e}"))
}

impl<'c> PgBillingTx<&'c mut PgConnection> {
    /// Joins billing to a transaction the host already opened. Refuses a
    /// connection that is not inside an explicit transaction: the account lock
    /// would end with its own statement and every later write would commit on
    /// its own, so a late failure could leave a partial charge. It also refuses
    /// a transaction that is not READ COMMITTED: the account lock only
    /// serializes correctly when each statement takes a fresh snapshot.
    pub async fn attach(connection: &'c mut PgConnection) -> Result<Self, StoreError> {
        // Outside a transaction every statement is its own transaction with its
        // own id; inside one the id is stable.
        let first: String = sqlx::query_scalar("SELECT pg_current_xact_id()::text")
            .fetch_one(&mut *connection)
            .await
            .map_err(db)?;
        let second: String = sqlx::query_scalar("SELECT pg_current_xact_id()::text")
            .fetch_one(&mut *connection)
            .await
            .map_err(db)?;
        if first != second {
            return Err(StoreError::Conflict(
                "billing must join an explicit transaction: this connection is in autocommit".into(),
            ));
        }
        let level: String = sqlx::query_scalar("SELECT current_setting('transaction_isolation')")
            .fetch_one(&mut *connection)
            .await
            .map_err(db)?;
        if level != "read committed" {
            return Err(StoreError::Conflict(format!(
                "billing requires a READ COMMITTED transaction, found {level}"
            )));
        }
        Ok(PgBillingTx(connection))
    }
}

impl<C: DerefMut<Target = PgConnection> + Send> PgBillingTx<C> {
    fn conn(&mut self) -> &mut PgConnection {
        self.0.deref_mut()
    }
}

impl UnitOfWork for PgBillingTx<Transaction<'static, Postgres>> {
    fn tx(&mut self) -> &mut dyn BillingTx {
        self
    }

    fn commit(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>> {
        Box::pin(async move { self.0.commit().await.map_err(db) })
    }

    fn rollback(self: Box<Self>) -> BoxFuture<'static, Result<(), StoreError>> {
        Box::pin(async move { self.0.rollback().await.map_err(db) })
    }
}

macro_rules! payment_cols {
    () => {
        "id, account_id, amount::text AS amount, currency, payment_gateway AS gateway, environment, status, \
         token, billing.ts_to_us(created) AS created_us, metadata, coupon_id, pricing_plan_id, customer_id, \
         subscription_ref, provider_order_ref, billing.ts_to_us(subscription_canceled_at) AS canceled_us, \
         observed, receipt_url"
    };
}

macro_rules! select_payment {
    ($tail:literal) => {
        concat!("SELECT ", payment_cols!(), " FROM billing.payments ", $tail)
    };
}

macro_rules! subscription_cols {
    () => {
        "id, account_id, payment_gateway AS gateway, environment, subscription_ref, customer_ref, \
         pricing_plan_id, currency, amount::text AS amount, spec, initial_payment_id, first_order_ref, \
         first_order_resolved_by, first_order_rationale, billing.ts_to_us(created_at) AS created_us, \
         billing.ts_to_us(cancel_requested_at) AS cancel_us"
    };
}

macro_rules! select_subscription {
    ($tail:literal) => {
        concat!("SELECT ", subscription_cols!(), " FROM billing.subscriptions ", $tail)
    };
}

macro_rules! unresolved_cols {
    () => {
        "id, payment_gateway AS gateway, environment, event_type, subscription_ref, order_ref, customer_ref, \
         payment_id, reason, payload, billing.ts_to_us(received_at) AS received_us, status"
    };
}

macro_rules! select_unresolved {
    ($tail:literal) => {
        concat!("SELECT ", unresolved_cols!(), " FROM billing.unresolved_notifications ", $tail)
    };
}

fn decimal(text: &str, what: &str) -> Result<Decimal, StoreError> {
    Decimal::parse(text).map_err(|e| corrupt(what, e))
}

fn payment_from(row: &PgRow) -> Result<Payment, StoreError> {
    let status_text: String = row.try_get("status").map_err(db)?;
    let status = PaymentStatus::parse(&status_text).ok_or_else(|| corrupt("payment status", &status_text))?;
    let amount: String = row.try_get("amount").map_err(db)?;
    let metadata: Json<Value> = row.try_get("metadata").map_err(db)?;
    let observed: Option<Json<Value>> = row.try_get("observed").map_err(db)?;
    let canceled: Option<i64> = row.try_get("canceled_us").map_err(db)?;
    Ok(Payment {
        id: row.try_get("id").map_err(db)?,
        account: AccountId(row.try_get::<Uuid, _>("account_id").map_err(db)?),
        amount: decimal(&amount, "payment amount")?,
        currency: row.try_get("currency").map_err(db)?,
        gateway: row.try_get("gateway").map_err(db)?,
        status,
        token: row.try_get("token").map_err(db)?,
        created: Micros(row.try_get("created_us").map_err(db)?),
        environment: row.try_get("environment").map_err(db)?,
        metadata: metadata.0,
        coupon_id: row.try_get("coupon_id").map_err(db)?,
        pricing_plan_id: row.try_get("pricing_plan_id").map_err(db)?,
        customer_id: row.try_get("customer_id").map_err(db)?,
        subscription_ref: row.try_get("subscription_ref").map_err(db)?,
        provider_order_ref: row.try_get("provider_order_ref").map_err(db)?,
        subscription_canceled_at: canceled.map(Micros),
        observed: observed.map(|o| o.0),
        receipt_url: row.try_get("receipt_url").map_err(db)?,
    })
}

fn subscription_from(row: &PgRow) -> Result<Subscription, StoreError> {
    let amount: String = row.try_get("amount").map_err(db)?;
    let spec: Json<Value> = row.try_get("spec").map_err(db)?;
    let cancel: Option<i64> = row.try_get("cancel_us").map_err(db)?;
    Ok(Subscription {
        id: row.try_get("id").map_err(db)?,
        account: AccountId(row.try_get::<Uuid, _>("account_id").map_err(db)?),
        gateway: row.try_get("gateway").map_err(db)?,
        environment: row.try_get("environment").map_err(db)?,
        subscription_ref: row.try_get("subscription_ref").map_err(db)?,
        customer_ref: row.try_get("customer_ref").map_err(db)?,
        pricing_plan_id: row.try_get("pricing_plan_id").map_err(db)?,
        currency: row.try_get("currency").map_err(db)?,
        amount: decimal(&amount, "subscription amount")?,
        spec: serde_json::from_value(spec.0).map_err(|e| corrupt("subscription spec", e))?,
        initial_payment_id: row.try_get("initial_payment_id").map_err(db)?,
        first_order_ref: row.try_get("first_order_ref").map_err(db)?,
        first_order_resolved_by: row.try_get("first_order_resolved_by").map_err(db)?,
        first_order_rationale: row.try_get("first_order_rationale").map_err(db)?,
        created_at: Micros(row.try_get("created_us").map_err(db)?),
        cancel_requested_at: cancel.map(Micros),
    })
}

fn unresolved_from(row: &PgRow) -> Result<UnresolvedNotification, StoreError> {
    let payload: Json<Value> = row.try_get("payload").map_err(db)?;
    Ok(UnresolvedNotification {
        id: row.try_get("id").map_err(db)?,
        gateway: row.try_get("gateway").map_err(db)?,
        environment: row.try_get("environment").map_err(db)?,
        event_type: row.try_get("event_type").map_err(db)?,
        subscription_ref: row.try_get("subscription_ref").map_err(db)?,
        order_ref: row.try_get("order_ref").map_err(db)?,
        customer_ref: row.try_get("customer_ref").map_err(db)?,
        payment_id: row.try_get("payment_id").map_err(db)?,
        reason: row.try_get("reason").map_err(db)?,
        payload: payload.0,
        received_at: Micros(row.try_get("received_us").map_err(db)?),
        status: row.try_get("status").map_err(db)?,
    })
}

type PackageRow = (i64, Uuid, Option<i64>, Option<String>, i64, i64, i64, i64);

async fn lock_unbarred(conn: &mut PgConnection, account: AccountId) -> Result<(), StoreError> {
    let locked = sqlx::query("SELECT 1 FROM billing.accounts WHERE account_id = $1 FOR UPDATE")
        .bind(account.0)
        .fetch_optional(&mut *conn)
        .await
        .map_err(db)?;
    // A separate statement after the lock: under READ COMMITTED a statement that waited
    // on the row lock keeps its older snapshot of other tables, so a barrier committed
    // by the erasure holding the lock would be missed if read in the same statement. It
    // runs before the missing-row answer too: a row the completed cascade removed while
    // this waited is an erased account, not an unknown one.
    let erasing: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM billing.erasure_barriers WHERE account_id = $1)")
        .bind(account.0)
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    if erasing {
        return Err(StoreError::AccountErased(account));
    }
    if locked.is_none() {
        return Err(StoreError::AccountMissing(account));
    }
    Ok(())
}

/// A transaction-scoped lock on one provider subscription identity, taken by both
/// ordinary binding and erasure adoption before either checks the other's table,
/// so the two can never both pass. It is a leaf lock: no account lock is taken
/// while it is held.
async fn lock_identity(conn: &mut PgConnection, gateway: &str, environment: &str, subscription_ref: &str) -> Result<(), StoreError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("billing.subscription|{gateway}|{environment}|{subscription_ref}"))
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    Ok(())
}

impl<C: DerefMut<Target = PgConnection> + Send> BillingTx for PgBillingTx<C> {
    fn create_account(&mut self, account: AccountId, created_at: Micros) -> StoreResult<'_, bool> {
        let conn = self.conn();
        Box::pin(async move {
            // An account whose erasure has begun is never created or provisioned again.
            let result = sqlx::query(
                "INSERT INTO billing.accounts (account_id, created_at)
                 SELECT $1, billing.us_to_ts($2)
                 WHERE NOT EXISTS (SELECT 1 FROM billing.erasure_barriers WHERE account_id = $1)
                 ON CONFLICT (account_id) DO NOTHING",
            )
            .bind(account.0)
            .bind(created_at.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            // A fresh statement on both branches: the INSERT's own barrier check used its
            // starting snapshot, so an erasure that opened and completed while it waited could
            // be missed. A barrier seen now refuses, and the caller rolls the insert back.
            let erasing: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM billing.erasure_barriers WHERE account_id = $1)")
                .bind(account.0)
                .fetch_one(&mut *conn)
                .await
                .map_err(db)?;
            if erasing {
                return Err(StoreError::AccountErased(account));
            }
            Ok(result.rows_affected() == 1)
        })
    }

    fn lock_account(&mut self, account: AccountId) -> StoreResult<'_, ()> {
        let conn = self.conn();
        Box::pin(async move {
            // The erasure barrier fences every ordinary billing change of the account:
            // grants, charges, provisioning and provider callbacks alike.
            lock_unbarred(conn, account).await
        })
    }

    fn account_created_at(&mut self, account: AccountId) -> StoreResult<'_, Option<Micros>> {
        let conn = self.conn();
        Box::pin(async move {
            let at: Option<i64> = sqlx::query_scalar(
                "SELECT billing.ts_to_us(created_at) FROM billing.accounts WHERE account_id = $1",
            )
            .bind(account.0)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
            Ok(at.map(Micros))
        })
    }

    fn packages(&mut self, account: AccountId) -> StoreResult<'_, Vec<PackageBalance>> {
        let conn = self.conn();
        Box::pin(async move {
            // Consumption is counted by the package a usage row references,
            // across both ledger tables, whoever the row says it belongs to.
            let rows: Vec<PackageRow> = sqlx::query_as(
                "SELECT p.id, p.account_id, p.payment_id, p.name, p.minutes_included::bigint,
                        billing.ts_to_us(p.start_at), billing.ts_to_us(p.end_at),
                        (COALESCE((SELECT sum(u.minutes) FROM billing.usage u WHERE u.package_id = p.id), 0)
                       + COALESCE((SELECT sum(l.minutes) FROM billing.legacy_usage l WHERE l.package_id = p.id), 0))::bigint
                 FROM billing.packages p WHERE p.account_id = $1 ORDER BY p.end_at, p.id",
            )
            .bind(account.0)
            .fetch_all(&mut *conn)
            .await
            .map_err(db)?;
            Ok(rows
                .into_iter()
                .map(|(id, owner, payment_id, name, included, start, end, consumed)| PackageBalance {
                    package: Package {
                        id,
                        account: AccountId(owner),
                        payment_id,
                        name,
                        minutes_included: included,
                        start_at: Micros(start),
                        end_at: Micros(end),
                    },
                    consumed,
                })
                .collect())
        })
    }

    fn insert_package<'a>(&'a mut self, package: &'a NewPackage) -> StoreResult<'a, PackageId> {
        let conn = self.conn();
        Box::pin(async move {
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO billing.packages
                   (account_id, payment_id, payment_seq, signup_grant, name, minutes_included, created, start_at, end_at)
                 VALUES ($1, $2, $3, $4, $5, $6, billing.us_to_ts($7), billing.us_to_ts($8), billing.us_to_ts($9))
                 RETURNING id",
            )
            .bind(package.account.0)
            .bind(package.payment_id)
            .bind(package.payment_seq)
            .bind(package.signup_grant)
            .bind(&package.name)
            .bind(package.minutes)
            .bind(package.created.0)
            .bind(package.start_at.0)
            .bind(package.end_at.0)
            .fetch_one(&mut *conn)
            .await
            .map_err(db)?;
            Ok(id)
        })
    }

    fn insert_usage<'a>(&'a mut self, rows: &'a [NewUsage]) -> StoreResult<'a, ()> {
        let conn = self.conn();
        Box::pin(async move {
            for row in rows {
                sqlx::query(
                    "INSERT INTO billing.usage (account_id, package_id, subject_ref, minutes, used_at)
                     VALUES ($1, $2, $3, $4, billing.us_to_ts($5))",
                )
                .bind(row.account.0)
                .bind(row.package_id)
                .bind(row.subject)
                .bind(row.minutes)
                .bind(row.used_at.0)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            }
            Ok(())
        })
    }

    fn release_subject(&mut self, account: AccountId, subject: SubjectRef) -> StoreResult<'_, u64> {
        let conn = self.conn();
        Box::pin(async move {
            let usage = sqlx::query("DELETE FROM billing.usage WHERE account_id = $1 AND subject_ref = $2")
                .bind(account.0)
                .bind(subject)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            // Imported subject ids are the legacy job ids, unique across accounts, and the host
            // has authorized this subject. The legacy foreign key cascaded every usage row of the
            // job whoever it named and whichever package it drew on, signed or not, so all of
            // them go. The caller has locked every package owner (`imported_release_owners`).
            let imported = sqlx::query("DELETE FROM billing.legacy_usage WHERE subject_ref = $1")
                .bind(subject)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            Ok(usage.rows_affected() + imported.rows_affected())
        })
    }

    fn imported_release_owners(&mut self, account: AccountId, subject: SubjectRef) -> StoreResult<'_, Vec<AccountId>> {
        let conn = self.conn();
        Box::pin(async move {
            // Balances count usage by referenced package, so the package owners are the accounts
            // a release changes; a row's own `account_id` changes nobody's balance.
            let mut owners: Vec<Uuid> = sqlx::query_scalar(
                "SELECT DISTINCT p.account_id FROM billing.legacy_usage l
                 JOIN billing.packages p ON p.id = l.package_id
                 WHERE l.subject_ref = $1",
            )
            .bind(subject)
            .fetch_all(&mut *conn)
            .await
            .map_err(db)?;
            owners.push(account.0);
            owners.sort();
            owners.dedup();
            Ok(owners.into_iter().map(AccountId).collect())
        })
    }

    fn insert_payment<'a>(&'a mut self, payment: &'a NewPayment) -> StoreResult<'a, PaymentInsert> {
        let conn = self.conn();
        Box::pin(async move {
            // Checkout admission does not take the account lock itself; a payment is new
            // payable work, so an erasing account refuses it here.
            lock_unbarred(&mut *conn, payment.account).await?;
            let inserted = sqlx::query(concat!(
                "INSERT INTO billing.payments
                   (account_id, amount, currency, payment_gateway, environment, status, token, created,
                    metadata, coupon_id, pricing_plan_id, provider_order_ref, subscription_ref, customer_id)
                 VALUES ($1, $2::text::numeric, $3, $4, $5, $6, $7, billing.us_to_ts($8), $9, $10, $11, $12, $13, $14)
                 ON CONFLICT DO NOTHING
                 RETURNING ",
                payment_cols!()
            ))
            .bind(payment.account.0)
            .bind(payment.amount.to_string())
            .bind(&payment.currency)
            .bind(&payment.gateway)
            .bind(&payment.environment)
            .bind(payment.status.as_str())
            .bind(&payment.token)
            .bind(payment.created.0)
            .bind(Json(&payment.metadata))
            .bind(&payment.coupon_id)
            .bind(&payment.pricing_plan_id)
            .bind(&payment.provider_order_ref)
            .bind(&payment.subscription_ref)
            .bind(&payment.customer_id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
            if let Some(row) = inserted {
                return Ok(PaymentInsert::Inserted(payment_from(&row)?));
            }
            // A unique provider identity already exists: hand back that row.
            let existing = match (&payment.token, &payment.provider_order_ref) {
                (Some(token), _) => {
                    sqlx::query(select_payment!("WHERE payment_gateway = $1 AND environment = $2 AND token = $3"))
                        .bind(&payment.gateway)
                        .bind(&payment.environment)
                        .bind(token)
                        .fetch_optional(&mut *conn)
                        .await
                        .map_err(db)?
                }
                (None, Some(order)) => {
                    sqlx::query(select_payment!(
                        "WHERE payment_gateway = $1 AND environment = $2 AND provider_order_ref = $3"
                    ))
                    .bind(&payment.gateway)
                    .bind(&payment.environment)
                    .bind(order)
                    .fetch_optional(&mut *conn)
                    .await
                    .map_err(db)?
                }
                (None, None) => None,
            };
            match existing {
                Some(row) => Ok(PaymentInsert::Duplicate(payment_from(&row)?)),
                None => Err(StoreError::Conflict("the payment conflicts with an existing row".into())),
            }
        })
    }

    fn payment(&mut self, id: PaymentId, lock: bool) -> StoreResult<'_, Option<Payment>> {
        let conn = self.conn();
        Box::pin(async move {
            let sql = if lock {
                select_payment!("WHERE id = $1 FOR UPDATE")
            } else {
                select_payment!("WHERE id = $1")
            };
            let row = sqlx::query(sql).bind(id).fetch_optional(&mut *conn).await.map_err(db)?;
            row.as_ref().map(payment_from).transpose()
        })
    }

    fn payment_by_token<'a>(
        &'a mut self,
        gateway: &'a str,
        environment: &'a str,
        token: &'a str,
        lock: bool,
    ) -> StoreResult<'a, Option<Payment>> {
        let conn = self.conn();
        Box::pin(async move {
            let sql = if lock {
                select_payment!("WHERE payment_gateway = $1 AND environment = $2 AND token = $3 FOR UPDATE")
            } else {
                select_payment!("WHERE payment_gateway = $1 AND environment = $2 AND token = $3")
            };
            let row = sqlx::query(sql)
                .bind(gateway)
                .bind(environment)
                .bind(token)
                .fetch_optional(&mut *conn)
                .await
                .map_err(db)?;
            row.as_ref().map(payment_from).transpose()
        })
    }

    fn payment_by_order_ref<'a>(
        &'a mut self,
        gateway: &'a str,
        environment: &'a str,
        order_ref: &'a str,
        lock: bool,
    ) -> StoreResult<'a, Option<Payment>> {
        let conn = self.conn();
        Box::pin(async move {
            let sql = if lock {
                select_payment!("WHERE payment_gateway = $1 AND environment = $2 AND provider_order_ref = $3 FOR UPDATE")
            } else {
                select_payment!("WHERE payment_gateway = $1 AND environment = $2 AND provider_order_ref = $3")
            };
            let row = sqlx::query(sql)
                .bind(gateway)
                .bind(environment)
                .bind(order_ref)
                .fetch_optional(&mut *conn)
                .await
                .map_err(db)?;
            row.as_ref().map(payment_from).transpose()
        })
    }

    fn set_payment_status(&mut self, id: PaymentId, from: PaymentStatus, to: PaymentStatus) -> StoreResult<'_, bool> {
        let conn = self.conn();
        Box::pin(async move {
            let result = sqlx::query("UPDATE billing.payments SET status = $3 WHERE id = $1 AND status = $2")
                .bind(id)
                .bind(from.as_str())
                .bind(to.as_str())
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            Ok(result.rows_affected() == 1)
        })
    }

    fn update_payment<'a>(&'a mut self, id: PaymentId, update: &'a PaymentUpdate) -> StoreResult<'a, ()> {
        let conn = self.conn();
        Box::pin(async move {
            sqlx::query(
                "UPDATE billing.payments SET
                   token = COALESCE($2, token),
                   customer_id = COALESCE($3, customer_id),
                   subscription_ref = COALESCE($4, subscription_ref),
                   provider_order_ref = COALESCE($5, provider_order_ref),
                   observed = COALESCE($6::jsonb, observed),
                   receipt_url = COALESCE($7, receipt_url)
                 WHERE id = $1",
            )
            .bind(id)
            .bind(&update.token)
            .bind(&update.customer_id)
            .bind(&update.subscription_ref)
            .bind(&update.provider_order_ref)
            .bind(update.observed.as_ref().map(Json))
            .bind(&update.receipt_url)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn stamp_cancellation(&mut self, id: PaymentId, at: Micros) -> StoreResult<'_, ()> {
        let conn = self.conn();
        Box::pin(async move {
            sqlx::query("UPDATE billing.payments SET subscription_canceled_at = billing.us_to_ts($2) WHERE id = $1")
                .bind(id)
                .bind(at.0)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            Ok(())
        })
    }

    fn completed_payments(&mut self, account: AccountId) -> StoreResult<'_, Vec<Payment>> {
        let conn = self.conn();
        Box::pin(async move {
            let rows = sqlx::query(select_payment!(
                "WHERE account_id = $1 AND status = 'completed' ORDER BY created DESC, id DESC"
            ))
            .bind(account.0)
            .fetch_all(&mut *conn)
            .await
            .map_err(db)?;
            rows.iter().map(payment_from).collect()
        })
    }

    fn insert_subscription<'a>(&'a mut self, subscription: &'a NewSubscription) -> StoreResult<'a, Subscription> {
        let conn = self.conn();
        Box::pin(async move {
            // Serialize with an erasure adoption of the same identity, then refuse an identity
            // another account's erasure already holds as its cancellation target.
            lock_identity(&mut *conn, &subscription.gateway, &subscription.environment, &subscription.subscription_ref).await?;
            let target: Option<(Uuid, Option<String>)> = sqlx::query_as(
                "SELECT account_id, customer_ref FROM billing.erasure_terminations
                 WHERE payment_gateway = $1 AND environment = $2 AND subscription_ref = $3",
            )
            .bind(&subscription.gateway)
            .bind(&subscription.environment)
            .bind(&subscription.subscription_ref)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
            if let Some((owner, customer)) = target {
                let other_customer = matches!((&customer, &subscription.customer_ref), (Some(a), Some(b)) if a != b);
                if owner != subscription.account.0 || other_customer {
                    return Err(StoreError::Conflict("the subscription reference is another account's erasure target".into()));
                }
            }
            let spec = serde_json::to_value(&subscription.spec).map_err(|e| corrupt("subscription spec", e))?;
            let inserted = sqlx::query(concat!(
                "INSERT INTO billing.subscriptions
                   (account_id, payment_gateway, environment, subscription_ref, customer_ref, pricing_plan_id,
                    currency, amount, spec, initial_payment_id, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8::text::numeric, $9, $10, billing.us_to_ts($11))
                 ON CONFLICT (payment_gateway, environment, subscription_ref) DO NOTHING
                 RETURNING ",
                subscription_cols!()
            ))
            .bind(subscription.account.0)
            .bind(&subscription.gateway)
            .bind(&subscription.environment)
            .bind(&subscription.subscription_ref)
            .bind(&subscription.customer_ref)
            .bind(&subscription.pricing_plan_id)
            .bind(&subscription.currency)
            .bind(subscription.amount.to_string())
            .bind(Json(&spec))
            .bind(subscription.initial_payment_id)
            .bind(subscription.created_at.0)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
            if let Some(row) = inserted {
                return subscription_from(&row);
            }
            let row = sqlx::query(select_subscription!(
                "WHERE payment_gateway = $1 AND environment = $2 AND subscription_ref = $3"
            ))
            .bind(&subscription.gateway)
            .bind(&subscription.environment)
            .bind(&subscription.subscription_ref)
            .fetch_one(&mut *conn)
            .await
            .map_err(db)?;
            let existing = subscription_from(&row)?;
            // The same provider identity must mean the same binding: a second
            // initial payment, customer, plan or schedule is a conflict, not a
            // second successful grant for one subscription.
            let same = existing.account == subscription.account
                && existing.initial_payment_id == subscription.initial_payment_id
                && existing.customer_ref == subscription.customer_ref
                && existing.pricing_plan_id == subscription.pricing_plan_id
                && existing.currency == subscription.currency
                && existing.amount == subscription.amount
                && existing.spec == subscription.spec;
            if !same {
                return Err(StoreError::Conflict("the subscription reference is already bound to a different purchase".into()));
            }
            Ok(existing)
        })
    }

    fn subscription<'a>(
        &'a mut self,
        gateway: &'a str,
        environment: &'a str,
        subscription_ref: &'a str,
        lock: bool,
    ) -> StoreResult<'a, Option<Subscription>> {
        let conn = self.conn();
        Box::pin(async move {
            let sql = if lock {
                select_subscription!("WHERE payment_gateway = $1 AND environment = $2 AND subscription_ref = $3 FOR UPDATE")
            } else {
                select_subscription!("WHERE payment_gateway = $1 AND environment = $2 AND subscription_ref = $3")
            };
            let row = sqlx::query(sql)
                .bind(gateway)
                .bind(environment)
                .bind(subscription_ref)
                .fetch_optional(&mut *conn)
                .await
                .map_err(db)?;
            row.as_ref().map(subscription_from).transpose()
        })
    }

    fn set_first_order_ref<'a>(
        &'a mut self,
        id: i64,
        order_ref: &'a str,
        admin: &'a str,
        rationale: &'a str,
    ) -> StoreResult<'a, bool> {
        let conn = self.conn();
        Box::pin(async move {
            let result = sqlx::query(
                "UPDATE billing.subscriptions
                 SET first_order_ref = $2, first_order_resolved_by = $3, first_order_rationale = $4
                 WHERE id = $1 AND first_order_ref IS NULL",
            )
            .bind(id)
            .bind(order_ref)
            .bind(admin)
            .bind(rationale)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            Ok(result.rows_affected() == 1)
        })
    }

    fn stamp_subscription_cancel<'a>(
        &'a mut self,
        gateway: &'a str,
        environment: &'a str,
        subscription_ref: &'a str,
        at: Micros,
    ) -> StoreResult<'a, ()> {
        let conn = self.conn();
        Box::pin(async move {
            sqlx::query(
                "UPDATE billing.subscriptions SET cancel_requested_at = billing.us_to_ts($4)
                 WHERE payment_gateway = $1 AND environment = $2 AND subscription_ref = $3",
            )
            .bind(gateway)
            .bind(environment)
            .bind(subscription_ref)
            .bind(at.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn hold_notification<'a>(
        &'a mut self,
        notification: &'a NewUnresolved,
    ) -> StoreResult<'a, (UnresolvedNotification, bool)> {
        let conn = self.conn();
        Box::pin(async move {
            let inserted = sqlx::query(concat!(
                "INSERT INTO billing.unresolved_notifications
                   (payment_gateway, environment, event_type, subscription_ref, order_ref, customer_ref,
                    payment_id, reason, payload, received_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, billing.us_to_ts($10))
                 ON CONFLICT (payment_gateway, environment, event_type, COALESCE(order_ref, ''),
                              COALESCE(subscription_ref, ''), COALESCE(customer_ref, ''), COALESCE(payment_id, -1))
                 DO UPDATE SET reason = EXCLUDED.reason
                 WHERE billing.unresolved_notifications.status = 'held'
                   AND billing.unresolved_notifications.reason <> EXCLUDED.reason
                   AND billing.unresolved_notifications.payload = EXCLUDED.payload
                 RETURNING ",
                unresolved_cols!(),
                ", (xmax = 0) AS inserted"
            ))
            .bind(&notification.gateway)
            .bind(&notification.environment)
            .bind(&notification.event_type)
            .bind(&notification.subscription_ref)
            .bind(&notification.order_ref)
            .bind(&notification.customer_ref)
            .bind(notification.payment_id)
            .bind(&notification.reason)
            .bind(Json(&notification.payload))
            .bind(notification.received_at.0)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
            if let Some(row) = inserted {
                let fresh: bool = row.try_get("inserted").map_err(db)?;
                return Ok((unresolved_from(&row)?, fresh));
            }
            // An exact retry (same identity, same reason, or no longer held): the existing row.
            let row = sqlx::query(select_unresolved!(
                "WHERE payment_gateway = $1 AND environment = $2 AND event_type = $3
                   AND COALESCE(order_ref, '') = $4 AND COALESCE(subscription_ref, '') = $5
                   AND COALESCE(customer_ref, '') = $6 AND COALESCE(payment_id, -1) = $7"
            ))
            .bind(&notification.gateway)
            .bind(&notification.environment)
            .bind(&notification.event_type)
            .bind(notification.order_ref.clone().unwrap_or_default())
            .bind(notification.subscription_ref.clone().unwrap_or_default())
            .bind(notification.customer_ref.clone().unwrap_or_default())
            .bind(notification.payment_id.unwrap_or(-1))
            .fetch_one(&mut *conn)
            .await
            .map_err(db)?;
            let existing = unresolved_from(&row)?;
            // Same claimed identity, different signed evidence: not a retry, and the
            // first observation must not be silently replaced by or merged with it.
            if existing.payload != notification.payload {
                return Err(StoreError::Conflict(format!(
                    "held notification {} already records different evidence for the same identity",
                    existing.id
                )));
            }
            Ok((existing, false))
        })
    }

    fn unresolved(&mut self, id: i64, lock: bool) -> StoreResult<'_, Option<UnresolvedNotification>> {
        let conn = self.conn();
        Box::pin(async move {
            let sql = if lock {
                select_unresolved!("WHERE id = $1 FOR UPDATE")
            } else {
                select_unresolved!("WHERE id = $1")
            };
            let row = sqlx::query(sql).bind(id).fetch_optional(&mut *conn).await.map_err(db)?;
            row.as_ref().map(unresolved_from).transpose()
        })
    }

    fn list_unresolved<'a>(&'a mut self, status: Option<&'a str>) -> StoreResult<'a, Vec<UnresolvedNotification>> {
        let conn = self.conn();
        Box::pin(async move {
            let rows = sqlx::query(select_unresolved!("WHERE ($1::text IS NULL OR status = $1) ORDER BY id"))
                .bind(status)
                .fetch_all(&mut *conn)
                .await
                .map_err(db)?;
            rows.iter().map(unresolved_from).collect()
        })
    }

    fn mark_unresolved<'a>(
        &'a mut self,
        id: i64,
        status: &'a str,
        payment_id: Option<PaymentId>,
        at: Micros,
    ) -> StoreResult<'a, ()> {
        let conn = self.conn();
        Box::pin(async move {
            sqlx::query(
                "UPDATE billing.unresolved_notifications
                 SET status = $2, resolved_payment_id = $3, resolved_at = billing.us_to_ts($4) WHERE id = $1",
            )
            .bind(id)
            .bind(status)
            .bind(payment_id)
            .bind(at.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn coupon<'a>(&'a mut self, id: &'a str) -> StoreResult<'a, Option<Coupon>> {
        let conn = self.conn();
        Box::pin(async move {
            type CouponRow = (String, Option<String>, Option<i64>, Option<String>, Option<String>);
            // `real` is read through its shortest decimal text, never widened to float8.
            let row: Option<CouponRow> = sqlx::query_as(
                "SELECT id, amount::text, billing.ts_to_us(expires_at), filter->>'product', discount_type
                 FROM billing.coupons WHERE id = $1",
            )
            .bind(id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
            row.map(|(id, amount, expires, product, discount_type)| -> Result<Coupon, StoreError> {
                Ok(Coupon {
                    id,
                    amount: amount.map(|a| decimal(&a, "coupon amount")).transpose()?,
                    expires_at: expires.map(Micros),
                    filter_product: product,
                    discount_type,
                })
            })
            .transpose()
        })
    }

    fn latest_billing_info(&mut self, account: AccountId) -> StoreResult<'_, Option<BillingInfo>> {
        let conn = self.conn();
        Box::pin(async move {
            type InfoRow = (i64, Option<Uuid>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64);
            let row: Option<InfoRow> = sqlx::query_as(
                "SELECT id, account_id, full_name, company_name, address, country, tax_id, parasut_contact_id,
                        billing.ts_to_us(created_at)
                 FROM billing.billing_info WHERE account_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1",
            )
            .bind(account.0)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
            Ok(row.map(|(id, owner, full_name, company_name, address, country, tax_id, parasut, created)| BillingInfo {
                id,
                account: owner.map(AccountId),
                full_name,
                company_name,
                address,
                country,
                tax_id,
                parasut_contact_id: parasut,
                created_at: Micros(created),
            }))
        })
    }

    fn emit<'a>(&'a mut self, event: &'a BillingEvent, at: Micros) -> StoreResult<'a, ()> {
        let conn = self.conn();
        Box::pin(async move {
            let payload = serde_json::to_value(event).map_err(|e| corrupt("event", e))?;
            sqlx::query(
                "INSERT INTO billing.outbox (account_id, kind, payload, occurred_at)
                 VALUES ($1, $2, $3, billing.us_to_ts($4))",
            )
            .bind(event.account().map(|a| a.0))
            .bind(event.kind())
            .bind(Json(&payload))
            .bind(at.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn lock_account_for_erasure(&mut self, account: AccountId) -> StoreResult<'_, ()> {
        let conn = self.conn();
        Box::pin(async move {
            let row = sqlx::query("SELECT 1 FROM billing.accounts WHERE account_id = $1 FOR UPDATE")
                .bind(account.0)
                .fetch_optional(&mut *conn)
                .await
                .map_err(db)?;
            row.map(|_| ()).ok_or(StoreError::AccountMissing(account))
        })
    }

    fn open_erasure(&mut self, account: AccountId, at: Micros) -> StoreResult<'_, ()> {
        let conn = self.conn();
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO billing.erasure_barriers (account_id, started_at) VALUES ($1, billing.us_to_ts($2))
                 ON CONFLICT (account_id) DO NOTHING",
            )
            .bind(account.0)
            .bind(at.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            // The lock anchor, also for an account billing never provisioned, unless the
            // local cascade already ran and removed it for good.
            sqlx::query(
                "INSERT INTO billing.accounts (account_id, created_at)
                 SELECT $1, billing.us_to_ts($2)
                 WHERE NOT EXISTS (SELECT 1 FROM billing.erasure_barriers
                                   WHERE account_id = $1 AND billing_erased_at IS NOT NULL)
                 ON CONFLICT (account_id) DO NOTHING",
            )
            .bind(account.0)
            .bind(at.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            sqlx::query("SELECT 1 FROM billing.accounts WHERE account_id = $1 FOR UPDATE")
                .bind(account.0)
                .fetch_optional(&mut *conn)
                .await
                .map_err(db)?;
            sqlx::query(
                "INSERT INTO billing.erasure_terminations
                   (account_id, payment_gateway, environment, subscription_ref, customer_ref, updated_at)
                 SELECT s.account_id, s.payment_gateway, s.environment, s.subscription_ref, s.customer_ref,
                        billing.us_to_ts($2)
                 FROM billing.subscriptions s WHERE s.account_id = $1
                 ON CONFLICT (payment_gateway, environment, subscription_ref) DO NOTHING",
            )
            .bind(account.0)
            .bind(at.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            Ok(())
        })
    }

    fn erasure_status(&mut self, account: AccountId) -> StoreResult<'_, Option<ErasureStatus>> {
        let conn = self.conn();
        Box::pin(async move {
            let barrier: Option<(i64, Option<i64>)> = sqlx::query_as(
                "SELECT billing.ts_to_us(started_at), billing.ts_to_us(billing_erased_at)
                 FROM billing.erasure_barriers WHERE account_id = $1",
            )
            .bind(account.0)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
            let Some((started, erased)) = barrier else { return Ok(None) };
            // Coverage, not presence, and only by this account's own targets: a binding that
            // appeared after the barrier opened, or whose reference only another account's
            // target names, is reported pending until a target of this account is confirmed.
            let rows: Vec<(String, String, String, Option<String>, String)> = sqlx::query_as(
                "SELECT payment_gateway, environment, subscription_ref, customer_ref, status
                 FROM billing.erasure_terminations WHERE account_id = $1
                 UNION ALL
                 SELECT s.payment_gateway, s.environment, s.subscription_ref, s.customer_ref, 'pending'
                 FROM billing.subscriptions s
                 WHERE s.account_id = $1
                   AND NOT EXISTS (SELECT 1 FROM billing.erasure_terminations t
                                   WHERE t.account_id = s.account_id
                                     AND t.payment_gateway = s.payment_gateway AND t.environment = s.environment
                                     AND t.subscription_ref = s.subscription_ref)
                 ORDER BY 1, 2, 3",
            )
            .bind(account.0)
            .fetch_all(&mut *conn)
            .await
            .map_err(db)?;
            let (mut pending, mut confirmed) = (Vec::new(), Vec::new());
            for (gateway, environment, subscription_ref, customer_ref, status) in rows {
                let target = TerminationTarget { gateway, environment, subscription_ref, customer_ref };
                if status == "confirmed" { confirmed.push(target) } else { pending.push(target) }
            }
            // A plan checkout no termination target covers, whatever its local status: a lost
            // or uncertain confirmation, or a form still payable, may have created a
            // subscription the provider renews, and nothing here can stop it. Only an
            // operator's audited resolution closes one without a target.
            let unbound_plan_payments: Vec<i64> = sqlx::query_scalar(
                "SELECT p.id FROM billing.payments p
                 WHERE p.account_id = $1 AND p.pricing_plan_id IS NOT NULL
                   AND NOT EXISTS (SELECT 1 FROM billing.erasure_terminations t
                                   WHERE t.account_id = p.account_id
                                     AND t.payment_gateway = p.payment_gateway AND t.environment = p.environment
                                     AND t.subscription_ref = p.subscription_ref)
                   AND NOT EXISTS (SELECT 1 FROM billing.erasure_checkout_resolutions r
                                   WHERE r.payment_id = p.id AND r.account_id = p.account_id)
                 ORDER BY p.id",
            )
            .bind(account.0)
            .fetch_all(&mut *conn)
            .await
            .map_err(db)?;
            Ok(Some(ErasureStatus {
                started_at: Micros(started),
                billing_erased_at: erased.map(Micros),
                pending,
                confirmed,
                unbound_plan_payments,
            }))
        })
    }

    fn record_termination<'a>(
        &'a mut self,
        account: AccountId,
        target: &'a TerminationTarget,
        confirmed: bool,
        outcome: &'a str,
        at: Micros,
    ) -> StoreResult<'a, ()> {
        let conn = self.conn();
        Box::pin(async move {
            let result = sqlx::query(
                "UPDATE billing.erasure_terminations
                 SET attempts = attempts + 1, last_outcome = $4, updated_at = billing.us_to_ts($5),
                     status = CASE WHEN $6 THEN 'confirmed' ELSE status END
                 WHERE payment_gateway = $1 AND environment = $2 AND subscription_ref = $3 AND account_id = $7",
            )
            .bind(&target.gateway)
            .bind(&target.environment)
            .bind(&target.subscription_ref)
            .bind(outcome)
            .bind(at.0)
            .bind(confirmed)
            .bind(account.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            if result.rows_affected() == 1 {
                return Ok(());
            }
            // A binding of this account that appeared after the barrier opened: its first
            // attempt records it. Another account's target for the reference is a conflict.
            let adopted = sqlx::query(
                "INSERT INTO billing.erasure_terminations
                   (account_id, payment_gateway, environment, subscription_ref, customer_ref,
                    status, attempts, last_outcome, updated_at)
                 SELECT s.account_id, s.payment_gateway, s.environment, s.subscription_ref, s.customer_ref,
                        CASE WHEN $6 THEN 'confirmed' ELSE 'pending' END, 1, $4, billing.us_to_ts($5)
                 FROM billing.subscriptions s
                 JOIN billing.erasure_barriers b ON b.account_id = s.account_id AND b.billing_erased_at IS NULL
                 WHERE s.payment_gateway = $1 AND s.environment = $2 AND s.subscription_ref = $3 AND s.account_id = $7
                 ON CONFLICT (payment_gateway, environment, subscription_ref) DO NOTHING",
            )
            .bind(&target.gateway)
            .bind(&target.environment)
            .bind(&target.subscription_ref)
            .bind(outcome)
            .bind(at.0)
            .bind(confirmed)
            .bind(account.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            if adopted.rows_affected() == 1 {
                Ok(())
            } else {
                Err(StoreError::Conflict("no termination of this account for that subscription".into()))
            }
        })
    }

    fn adopt_erasure_target<'a>(&'a mut self, account: AccountId, target: &'a TerminationTarget, at: Micros) -> StoreResult<'a, ()> {
        let conn = self.conn();
        Box::pin(async move {
            // Serialize with ordinary binding of the same identity, then check both tables. A
            // verified binding is the trusted owner of a subscription identity: another
            // account's binding, or this account's binding with another customer, is a
            // collision, never a target to cancel (the same rule insert_subscription keeps).
            lock_identity(&mut *conn, &target.gateway, &target.environment, &target.subscription_ref).await?;
            let bound: Option<(Uuid, Option<String>)> = sqlx::query_as(
                "SELECT account_id, customer_ref FROM billing.subscriptions
                 WHERE payment_gateway = $1 AND environment = $2 AND subscription_ref = $3",
            )
            .bind(&target.gateway)
            .bind(&target.environment)
            .bind(&target.subscription_ref)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
            match bound {
                Some((owner, _)) if owner != account.0 => {
                    return Err(StoreError::Conflict("the subscription is bound to another account".into()));
                }
                Some((_, Some(customer))) if target.customer_ref.as_ref().is_some_and(|reported| *reported != customer) => {
                    return Err(StoreError::Conflict("the subscription's binding names another customer".into()));
                }
                _ => {}
            }
            let inserted = sqlx::query(
                "INSERT INTO billing.erasure_terminations
                   (account_id, payment_gateway, environment, subscription_ref, customer_ref, updated_at)
                 SELECT $1, $2, $3, $4, $5, billing.us_to_ts($6)
                 FROM billing.erasure_barriers b WHERE b.account_id = $1 AND b.billing_erased_at IS NULL
                 ON CONFLICT (payment_gateway, environment, subscription_ref) DO NOTHING",
            )
            .bind(account.0)
            .bind(&target.gateway)
            .bind(&target.environment)
            .bind(&target.subscription_ref)
            .bind(&target.customer_ref)
            .bind(at.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            if inserted.rows_affected() == 1 {
                return Ok(());
            }
            // Already recorded: the same account and customer is the same identity, left as it
            // is (status and attempts included); anything else is a collision.
            let existing: Option<(Uuid, Option<String>)> = sqlx::query_as(
                "SELECT account_id, customer_ref FROM billing.erasure_terminations
                 WHERE payment_gateway = $1 AND environment = $2 AND subscription_ref = $3",
            )
            .bind(&target.gateway)
            .bind(&target.environment)
            .bind(&target.subscription_ref)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
            match existing {
                Some((owner, customer)) if owner == account.0 && customer == target.customer_ref => Ok(()),
                Some((owner, _)) if owner != account.0 => {
                    Err(StoreError::Conflict("the subscription is another account's termination target".into()))
                }
                Some(_) => Err(StoreError::Conflict("the subscription's termination target names another customer".into())),
                None => Err(StoreError::Conflict("no open erasure for the account".into())),
            }
        })
    }

    fn resolve_erasure_checkout<'a>(
        &'a mut self,
        account: AccountId,
        payment_id: PaymentId,
        closure: &'a CheckoutClosure,
        at: Micros,
    ) -> StoreResult<'a, bool> {
        let conn = self.conn();
        Box::pin(async move {
            let written = sqlx::query(
                "INSERT INTO billing.erasure_checkout_resolutions
                   (account_id, payment_id, payment_gateway, environment, token, resolved_by,
                    checkout_cannot_complete, no_renewing_subscription, evidence_ref, rationale, resolved_at)
                 SELECT p.account_id, p.id, p.payment_gateway, p.environment, p.token, $3, $4, $5, $6, $7, billing.us_to_ts($8)
                 FROM billing.payments p
                 JOIN billing.erasure_barriers b ON b.account_id = p.account_id AND b.billing_erased_at IS NULL
                 WHERE p.id = $2 AND p.account_id = $1 AND p.pricing_plan_id IS NOT NULL
                   AND NOT EXISTS (SELECT 1 FROM billing.erasure_terminations t
                                   WHERE t.account_id = p.account_id
                                     AND t.payment_gateway = p.payment_gateway AND t.environment = p.environment
                                     AND t.subscription_ref = p.subscription_ref)
                 ON CONFLICT (payment_id) DO NOTHING",
            )
            .bind(account.0)
            .bind(payment_id)
            .bind(&closure.resolved_by)
            .bind(closure.checkout_cannot_complete)
            .bind(closure.no_renewing_subscription)
            .bind(&closure.evidence_ref)
            .bind(&closure.rationale)
            .bind(at.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            Ok(written.rows_affected() == 1)
        })
    }

    fn erasure_owners<'a>(&'a mut self, account: AccountId, subjects: &'a [SubjectRef]) -> StoreResult<'a, Vec<AccountId>> {
        let conn = self.conn();
        Box::pin(async move {
            // Balances count usage by referenced package, so the accounts the cascade changes
            // are the owners of the packages the removed imported rows draw on.
            let mut owners: Vec<Uuid> = sqlx::query_scalar(
                "SELECT DISTINCT p.account_id FROM billing.legacy_usage l
                 JOIN billing.packages p ON p.id = l.package_id
                 WHERE l.account_id = $1 OR l.subject_ref = ANY($2)",
            )
            .bind(account.0)
            .bind(subjects)
            .fetch_all(&mut *conn)
            .await
            .map_err(db)?;
            owners.push(account.0);
            owners.sort();
            owners.dedup();
            Ok(owners.into_iter().map(AccountId).collect())
        })
    }

    fn erase_account_rows<'a>(
        &'a mut self,
        account: AccountId,
        subjects: &'a [SubjectRef],
        at: Micros,
    ) -> StoreResult<'a, ErasureCounts> {
        let conn = self.conn();
        Box::pin(async move {
            let (payments, packages, subscriptions, billing_info): (i64, i64, i64, i64) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM billing.payments WHERE account_id = $1),
                        (SELECT count(*) FROM billing.packages WHERE account_id = $1),
                        (SELECT count(*) FROM billing.subscriptions WHERE account_id = $1),
                        (SELECT count(*) FROM billing.billing_info WHERE account_id = $1)",
            )
            .bind(account.0)
            .fetch_one(&mut *conn)
            .await
            .map_err(db)?;
            // Imported rows the source cascade removed with the user, with the user's jobs
            // and with the user's packages, signed or not, whichever account they name.
            let imported = sqlx::query(
                "DELETE FROM billing.legacy_usage
                 WHERE account_id = $1 OR subject_ref = ANY($2)
                    OR package_id IN (SELECT id FROM billing.packages WHERE account_id = $1)",
            )
            .bind(account.0)
            .bind(subjects)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            let runtime = sqlx::query("DELETE FROM billing.usage WHERE account_id = $1")
                .bind(account.0)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            sqlx::query(
                "DELETE FROM billing.outbox_deliveries d USING billing.outbox o
                 WHERE d.event_id = o.event_id AND o.account_id = $1",
            )
            .bind(account.0)
            .execute(&mut *conn)
            .await
            .map_err(db)?;
            let outbox = sqlx::query("DELETE FROM billing.outbox WHERE account_id = $1")
                .bind(account.0)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            // The foreign keys remove payments, packages and subscriptions and detach
            // billing_info (D5). Row security allows this only behind a barrier whose
            // terminations are all confirmed.
            let removed = sqlx::query("DELETE FROM billing.accounts WHERE account_id = $1")
                .bind(account.0)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            if removed.rows_affected() != 1 {
                return Err(StoreError::Conflict(format!(
                    "billing account {account} was not erased: no open barrier with every subscription stopped"
                )));
            }
            sqlx::query("UPDATE billing.erasure_barriers SET billing_erased_at = billing.us_to_ts($2) WHERE account_id = $1")
                .bind(account.0)
                .bind(at.0)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            sqlx::query("UPDATE billing.erasure_terminations SET customer_ref = NULL WHERE account_id = $1")
                .bind(account.0)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
            Ok(ErasureCounts {
                runtime_usage: runtime.rows_affected(),
                imported_usage: imported.rows_affected(),
                outbox_events: outbox.rows_affected(),
                payments: payments as u64,
                packages: packages as u64,
                subscriptions: subscriptions as u64,
                billing_info_detached: billing_info as u64,
            })
        })
    }
}
