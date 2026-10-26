//! taskr's host-owned pieces: the enrollment directory and the metered sample
//! operation. Ordinary task creation is never metered; this is an additive,
//! separate route.
//!
//! Sample work has no job compare-and-swap, so its host table is the
//! compare-and-swap: under the account lock the work row is locked, its stored
//! request is compared with this one (a different payload is a conflict even
//! when the work is done), a pending row is debited and marked done in one
//! transaction, and a done row replays its saved result without another charge.

use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use billing_core::app::{Billing, BillingError, ChargeError, charge_strict, start_answer};
use billing_core::ports::{BillingTx, Customer, CustomerDirectory, DirectoryError, StoreError};
use billing_core::types::{AccountId, BoxFuture, Micros};
use billing_http::BillingIdentity;
use billing_postgres::PgBillingTx;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::SystemClock;

#[derive(Clone)]
pub struct PgCustomerDirectory {
    pool: PgPool,
}

impl PgCustomerDirectory {
    pub fn new(pool: PgPool) -> PgCustomerDirectory {
        PgCustomerDirectory { pool }
    }
}

fn directory_error(e: sqlx::Error) -> DirectoryError {
    DirectoryError(e.to_string())
}

type Row4 = (Uuid, String, Option<String>, i64);

fn customer(row: Row4) -> Customer {
    Customer { account: AccountId(row.0), email: row.1, full_name: row.2, created_at: Micros(row.3) }
}

impl CustomerDirectory for PgCustomerDirectory {
    fn by_account(&self, account: AccountId) -> BoxFuture<'_, Result<Option<Customer>, DirectoryError>> {
        Box::pin(async move {
            let row: Option<Row4> = sqlx::query_as(
                "SELECT account_id, email, full_name, billing.ts_to_us(created_at)
                 FROM taskr_billing.customers WHERE account_id = $1",
            )
            .bind(account.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(directory_error)?;
            Ok(row.map(customer))
        })
    }

    fn by_email<'a>(&'a self, email: &'a str) -> BoxFuture<'a, Result<Option<Customer>, DirectoryError>> {
        Box::pin(async move {
            let row: Option<Row4> = sqlx::query_as(
                "SELECT account_id, email, full_name, billing.ts_to_us(created_at)
                 FROM taskr_billing.customers WHERE lower(email) = lower($1)",
            )
            .bind(email)
            .fetch_optional(&self.pool)
            .await
            .map_err(directory_error)?;
            Ok(row.map(customer))
        })
    }

    fn register<'a>(&'a self, new: &'a Customer) -> BoxFuture<'a, Result<Customer, DirectoryError>> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO taskr_billing.customers (account_id, email, full_name, created_at)
                 VALUES ($1, $2, $3, billing.us_to_ts($4))
                 ON CONFLICT (account_id) DO UPDATE SET email = EXCLUDED.email, full_name = EXCLUDED.full_name",
            )
            .bind(new.account.0)
            .bind(&new.email)
            .bind(&new.full_name)
            .bind(new.created_at.0)
            .execute(&self.pool)
            .await
            .map_err(directory_error)?;
            let row: Row4 = sqlx::query_as(
                "SELECT account_id, email, full_name, billing.ts_to_us(created_at)
                 FROM taskr_billing.customers WHERE account_id = $1",
            )
            .bind(new.account.0)
            .fetch_one(&self.pool)
            .await
            .map_err(directory_error)?;
            Ok(customer(row))
        })
    }
}

pub struct WorkState {
    pub billing: Arc<Billing>,
    pub identity: Arc<dyn BillingIdentity>,
    pub pool: PgPool,
}

type HttpError = (StatusCode, Json<Value>);

fn error(status: StatusCode, code: &str, message: impl Into<String>) -> HttpError {
    (status, Json(json!({"error": {"code": code, "message": message.into()}})))
}

fn billing_error(e: BillingError) -> HttpError {
    let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let message = match &e {
        BillingError::Store(_) | BillingError::Directory(_) | BillingError::Internal(_) => {
            eprintln!("taskr billing: {e}");
            "Billing storage is unavailable.".to_string()
        }
        other => other.to_string(),
    };
    error(status, e.code(), message)
}

async fn authenticate(state: &WorkState, headers: &HeaderMap) -> Result<billing_http::Caller, HttpError> {
    state.identity.authenticate(headers).await.map_err(|e| match e {
        billing_http::AuthError::Unauthenticated(_) => {
            error(StatusCode::UNAUTHORIZED, "unauthenticated", "Your session has ended. Sign in again.")
        }
        billing_http::AuthError::AccountErased(_) => error(StatusCode::GONE, "account_erased", "This account is unavailable due to account erasure."),
        billing_http::AuthError::Unavailable(_) => {
            error(StatusCode::SERVICE_UNAVAILABLE, "identity_unavailable", "The sign-in service is not reachable.")
        }
    })
}

async fn barrier_exists(pool: &PgPool, account: AccountId) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM billing.erasure_barriers WHERE account_id = $1)").bind(account.0).fetch_one(pool).await.unwrap_or(false)
}

/// The fallback for failures that carry no typed erased-account refusal, such as a write the sample's
/// own directory trigger refused: if the account's billing erasure barrier exists the answer is 410,
/// not a storage outage a client would retry. The typed refusal (`StoreError::AccountErased`,
/// `BillingError::AccountErased`) is mapped directly where it arises and never depends on this
/// second query; a failed query here only leaves the original answer in place.
async fn erased_or(state: &WorkState, account: AccountId, response: HttpError) -> Response {
    if !response.0.is_success() && response.0 != StatusCode::GONE && barrier_exists(&state.pool, account).await {
        return error(StatusCode::GONE, "account_erased", "This account is unavailable due to account erasure.").into_response();
    }
    response.into_response()
}

/// A storage failure of the billing module as an answer. The typed refusal for an erased account is
/// the agreed contract and is mapped here directly, never inferred from a second query that could
/// itself fail; everything else is an outage.
fn store_failure(e: &StoreError) -> HttpError {
    match e {
        StoreError::AccountErased(_) => error(StatusCode::GONE, "account_erased", "This account is unavailable due to account erasure."),
        other => {
            eprintln!("taskr billing: {other}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "storage", "Billing storage is unavailable.")
        }
    }
}

/// Explicit enrollment in the billing sample. The enrollment instant is the
/// server's clock at this call and is the account's creation instant for the
/// free credits; repeating the call never grants again and never moves it.
pub async fn enroll(State(state): State<Arc<WorkState>>, headers: HeaderMap) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    let customer = Customer {
        account: caller.account,
        email: caller.email,
        full_name: caller.full_name,
        created_at: billing_core::types::Clock::now(&SystemClock),
    };
    let account = customer.account;
    match state.billing.provision(customer).await {
        Ok(provisioned) => Json(provisioned).into_response(),
        Err(e) => erased_or(&state, account, billing_error(e)).await,
    }
}

#[derive(Deserialize)]
struct WorkBody {
    work_id: String,
    units: i64,
    operation: String,
}

enum Outcome {
    Done(Value, bool),
    Refused(HttpError),
}

async fn run_work(state: &WorkState, account: AccountId, body: &WorkBody) -> Result<Outcome, HttpError> {
    let internal = |e: &dyn std::fmt::Display| {
        eprintln!("taskr billing: {e}");
        error(StatusCode::INTERNAL_SERVER_ERROR, "storage", "Billing storage is unavailable.")
    };
    let now = billing_core::types::Clock::now(&SystemClock);
    let mut tx = state.pool.begin().await.map_err(|e| internal(&e))?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED").execute(&mut *tx).await.map_err(|e| internal(&e))?;
    {
        // The account lock first: the same serialization as every other billing write.
        let mut billing = PgBillingTx::attach(&mut tx).await.map_err(|e| store_failure(&e))?;
        billing.lock_account(account).await.map_err(|e| store_failure(&e))?;
    }
    sqlx::query(
        "INSERT INTO taskr_billing.sample_work (account_id, work_key, units, operation, status, created_at)
         VALUES ($1, $2, $3, $4, 'pending', billing.us_to_ts($5)) ON CONFLICT (account_id, work_key) DO NOTHING",
    )
    .bind(account.0)
    .bind(&body.work_id)
    .bind(body.units as i32)
    .bind(&body.operation)
    .bind(now.0)
    .execute(&mut *tx)
    .await
    .map_err(|e| internal(&e))?;
    let row = sqlx::query(
        "SELECT id, units, operation, status, result FROM taskr_billing.sample_work
         WHERE account_id = $1 AND work_key = $2 FOR UPDATE",
    )
    .bind(account.0)
    .bind(&body.work_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| internal(&e))?;
    let id: i64 = row.try_get("id").map_err(|e| internal(&e))?;
    let stored_units: i32 = row.try_get("units").map_err(|e| internal(&e))?;
    let stored_operation: String = row.try_get("operation").map_err(|e| internal(&e))?;
    let status: String = row.try_get("status").map_err(|e| internal(&e))?;
    // The stored request is immutable: compare it before anything is replayed.
    if i64::from(stored_units) != body.units || stored_operation != body.operation {
        return Ok(Outcome::Refused(error(
            StatusCode::CONFLICT,
            "work_conflict",
            "This work id was already used for a different request.",
        )));
    }
    if status == "done" {
        let saved: Option<sqlx::types::Json<Value>> = row.try_get("result").map_err(|e| internal(&e))?;
        let saved = saved.map(|j| j.0).unwrap_or(Value::Null);
        return Ok(Outcome::Done(saved, true));
    }
    let charged = {
        let mut billing = PgBillingTx::attach(&mut tx).await.map_err(|e| store_failure(&e))?;
        match charge_strict(&mut billing, account, id, body.units, now).await {
            // The answer is derived from the state after this debit, in the same transaction.
            Ok(_) => Ok(start_answer(&mut billing, account, now).await.map_err(|e| store_failure(&e))?),
            Err(ChargeError::Refused(refusal)) => Err(refusal),
            Err(ChargeError::Store(e)) => return Err(store_failure(&e)),
        }
    };
    let after = match charged {
        Ok(after) => after,
        // Dropping the transaction rolls back the debit attempt and the pending row together,
        // so the same work id succeeds after credits are bought.
        Err(refusal) => {
            return Ok(Outcome::Refused(error(
                StatusCode::from_u16(refusal.http_status()).unwrap_or(StatusCode::PAYMENT_REQUIRED),
                refusal.code(),
                refusal.message(false),
            )));
        }
    };
    let result = json!({
        "work_id": body.work_id,
        "units": body.units,
        "operation": body.operation,
        "charged": body.units,
        "balance_after": after.new_balance,
        "is_freemium": after.is_freemium,
    });
    sqlx::query("UPDATE taskr_billing.sample_work SET status = 'done', result = $2 WHERE id = $1 AND status = 'pending'")
        .bind(id)
        .bind(sqlx::types::Json(&result))
        .execute(&mut *tx)
        .await
        .map_err(|e| internal(&e))?;
    tx.commit().await.map_err(|e| internal(&e))?;
    Ok(Outcome::Done(result, false))
}

/// The metered sample operation: costs `units` credits, once per work id.
pub async fn sample_work(State(state): State<Arc<WorkState>>, headers: HeaderMap, body: Bytes) -> Response {
    let caller = match authenticate(&state, &headers).await {
        Ok(c) => c,
        Err(response) => return response.into_response(),
    };
    let body: WorkBody = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request", format!("invalid request body: {e}")).into_response(),
    };
    if body.work_id.is_empty() || body.work_id.len() > 100 || body.operation.is_empty() || body.operation.len() > 100 {
        return error(StatusCode::BAD_REQUEST, "invalid_request", "work_id and operation are required, at most 100 characters.").into_response();
    }
    if !(1..=1000).contains(&body.units) {
        return error(StatusCode::BAD_REQUEST, "invalid_request", "units must be from 1 to 1000.").into_response();
    }
    if let Err(e) = state.billing.ensure_provisioned(caller.account).await {
        return erased_or(&state, caller.account, billing_error(e)).await;
    }
    match run_work(&state, caller.account, &body).await {
        Ok(Outcome::Done(result, replayed)) => {
            let mut result = result;
            if let Value::Object(map) = &mut result {
                map.insert("replayed".into(), json!(replayed));
            }
            Json(result).into_response()
        }
        Ok(Outcome::Refused(response)) | Err(response) => erased_or(&state, caller.account, response).await,
    }
}
