//! Transgate's customer directory: host-owned records with a trusted account
//! creation instant and verified contact facts.
//!
//! Where the facts come from. A configured trusted auth directory (Supabase's admin
//! user endpoint) answers, for an authenticated account that has no record yet,
//! when the identity system really created it and which address it verified. That
//! lookup runs on the account's own authenticated access, before provisioning, so the
//! free trial is anchored to the real signup and never to a token time or a request.
//! The host's own signup system may also report through the service-token route.
//!
//! Rules that hold on every path:
//! - `created_at` is written once and never changes: a refresh can change an address,
//!   it cannot move the trial window or grant another one;
//! - only a verified address is adopted, and a newer verified address replaces an older
//!   one only when the auth directory's `updated_at` is newer than what is stored, so a
//!   stale signup replay cannot undo a verified email change;
//! - an auth lookup that cannot be answered is an error, never "not found", never a
//!   guessed creation time, and never a reason to serve contact facts that are due for
//!   re-confirmation: the account stays due and the next access retries.

use std::sync::Arc;
use std::time::Duration;

use billing_core::calendar::parse_rfc3339;
use billing_core::ports::{Customer, CustomerDirectory, DirectoryError};
use billing_core::types::{AccountId, BoxFuture, Micros};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// What the trusted auth directory says about one account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthUser {
    pub account: AccountId,
    pub email: String,
    pub email_verified: bool,
    pub full_name: Option<String>,
    /// The source profile's Joined on, not account creation or generic confirmation.
    pub email_confirmed_at: Option<Micros>,
    /// When the identity system created the account.
    pub created_at: Micros,
    /// The auth directory's own last-change time: the order of contact facts.
    pub updated_at: Micros,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthLookupError(pub String);

impl std::fmt::Display for AuthLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub trait AuthDirectory: Send + Sync {
    /// `Ok(None)` only when the directory positively says there is no such user.
    fn lookup<'a>(&'a self, account: AccountId) -> BoxFuture<'a, Result<Option<AuthUser>, AuthLookupError>>;
}

/// Supabase Auth's admin API: `GET {url}/auth/v1/admin/users/{id}` with the service
/// key. The key never leaves this struct and never appears in an error.
pub struct SupabaseAdmin {
    http: reqwest::Client,
    base: String,
    service_key: String,
}

impl SupabaseAdmin {
    pub fn new(url: &str, service_key: &str, timeout: Duration) -> Result<SupabaseAdmin, String> {
        let base = url.trim().trim_end_matches('/').to_string();
        if base.is_empty() || service_key.trim().is_empty() {
            return Err("the Supabase auth directory needs SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY".into());
        }
        let http = reqwest::Client::builder().timeout(timeout).build().map_err(|e| e.to_string())?;
        Ok(SupabaseAdmin { http, base, service_key: service_key.trim().to_string() })
    }
}

/// What the admin API said about deleting one user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthDeletion {
    Deleted,
    /// There was no such user: already gone.
    Absent,
}

impl SupabaseAdmin {
    /// `DELETE {url}/auth/v1/admin/users/{id}` with the service key. Only a documented success or a
    /// 404 is an answer; any other status or a transport failure is an error, never a deletion.
    pub async fn delete_user(&self, account: AccountId) -> Result<AuthDeletion, AuthLookupError> {
        let response = self
            .http
            .delete(format!("{}/auth/v1/admin/users/{}", self.base, account))
            .header("apikey", &self.service_key)
            .header("authorization", format!("Bearer {}", self.service_key))
            .send()
            .await
            .map_err(|e| AuthLookupError(format!("the auth directory is unreachable: {}", e.without_url())))?;
        match response.status() {
            status if status.is_success() => Ok(AuthDeletion::Deleted),
            reqwest::StatusCode::NOT_FOUND => Ok(AuthDeletion::Absent),
            status => Err(AuthLookupError(format!("the auth directory answered HTTP {}", status.as_u16()))),
        }
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string).filter(|s| !s.trim().is_empty())
}

/// Reads one admin-API user object. The answer must name exactly the requested account.
pub fn parse_auth_user(account: AccountId, body: &Value) -> Result<AuthUser, AuthLookupError> {
    let bad = |why: &str| AuthLookupError(format!("the auth directory answer is unusable: {why}"));
    let id = text(body, "id").and_then(|id| AccountId::parse(&id)).ok_or_else(|| bad("no user id"))?;
    if id != account {
        return Err(bad("it describes another user"));
    }
    let email = text(body, "email").ok_or_else(|| bad("no email"))?;
    let created_at = text(body, "created_at").and_then(|t| parse_rfc3339(&t)).ok_or_else(|| bad("no creation time"))?;
    let updated_at = text(body, "updated_at").and_then(|t| parse_rfc3339(&t)).unwrap_or(created_at);
    let verified = text(body, "email_confirmed_at").is_some() || text(body, "confirmed_at").is_some();
    let email_confirmed_at = match body.get("email_confirmed_at") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text.trim().is_empty() => None,
        Some(Value::String(text)) => Some(parse_rfc3339(text).ok_or_else(|| bad("invalid email confirmation time"))?),
        Some(_) => return Err(bad("invalid email confirmation time")),
    };
    let metadata = body.get("user_metadata").unwrap_or(&Value::Null);
    let full_name = text(metadata, "full_name").or_else(|| text(metadata, "name"));
    Ok(AuthUser { account: id, email, email_verified: verified, full_name, email_confirmed_at, created_at, updated_at })
}

impl AuthDirectory for SupabaseAdmin {
    fn lookup<'a>(&'a self, account: AccountId) -> BoxFuture<'a, Result<Option<AuthUser>, AuthLookupError>> {
        Box::pin(async move {
            let response = self
                .http
                .get(format!("{}/auth/v1/admin/users/{}", self.base, account))
                .header("apikey", &self.service_key)
                .header("authorization", format!("Bearer {}", self.service_key))
                .send()
                .await
                .map_err(|e| AuthLookupError(format!("the auth directory is unreachable: {}", e.without_url())))?;
            let status = response.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !status.is_success() {
                return Err(AuthLookupError(format!("the auth directory answered HTTP {}", status.as_u16())));
            }
            let body: Value = response
                .json()
                .await
                .map_err(|e| AuthLookupError(format!("the auth directory answered unreadably: {}", e.without_url())))?;
            parse_auth_user(account, &body).map(Some)
        })
    }
}

#[derive(Clone)]
pub struct PgCustomerDirectory {
    pool: PgPool,
    auth: Option<Arc<dyn AuthDirectory>>,
    refresh_after_secs: f64,
}

impl PgCustomerDirectory {
    pub fn new(pool: PgPool) -> PgCustomerDirectory {
        PgCustomerDirectory { pool, auth: None, refresh_after_secs: 900.0 }
    }

    /// Whether a trusted auth directory is configured.
    pub fn has_auth(&self) -> bool {
        self.auth.is_some()
    }

    /// Adopts accounts that have no record yet from `auth`, and re-confirms the contact
    /// facts of a stored account when they were last confirmed longer ago than `refresh_after`.
    pub fn with_auth(mut self, auth: Arc<dyn AuthDirectory>, refresh_after: Duration) -> PgCustomerDirectory {
        self.auth = Some(auth);
        self.refresh_after_secs = refresh_after.as_secs_f64();
        self
    }
}

fn fail(e: sqlx::Error) -> DirectoryError {
    let unique = e.as_database_error().and_then(|d| d.code()).is_some_and(|c| c == "23505");
    if unique {
        DirectoryError("that email already belongs to another account".into())
    } else {
        DirectoryError(e.to_string())
    }
}

type Row = (Uuid, String, Option<String>, i64, bool);

fn customer(row: Row) -> Customer {
    Customer { account: AccountId(row.0), email: row.1, full_name: row.2, created_at: Micros(row.3) }
}

impl PgCustomerDirectory {
    /// The stored record and whether its contact facts are due for re-confirmation.
    async fn stored(&self, account: AccountId) -> Result<Option<Row>, DirectoryError> {
        sqlx::query_as(
            "SELECT account_id, email, full_name, billing.ts_to_us(created_at),
                    (contact_checked_at IS NULL OR contact_checked_at < now() - make_interval(secs => $2)) AS due
             FROM transgate_billing.customers WHERE account_id = $1",
        )
        .bind(account.0)
        .bind(self.refresh_after_secs)
        .fetch_optional(&self.pool)
        .await
        .map_err(fail)
    }

    /// An account with no record: ask the trusted auth directory. Only a verified address
    /// is adopted; the creation instant is the identity system's own.
    async fn adopt(&self, auth: &dyn AuthDirectory, account: AccountId) -> Result<Option<Customer>, DirectoryError> {
        let found = auth.lookup(account).await.map_err(|e| DirectoryError(e.0))?;
        let Some(user) = found.filter(|u| u.email_verified) else { return Ok(None) };
        self.adopt_user(&user).await
    }

    async fn adopt_user(&self, user: &AuthUser) -> Result<Option<Customer>, DirectoryError> {
        let account = user.account;
        sqlx::query(
            "INSERT INTO transgate_billing.customers
               (account_id, email, full_name, created_at, contact_version, contact_checked_at)
             VALUES ($1, $2, $3, billing.us_to_ts($4), billing.us_to_ts($5), now())
             ON CONFLICT (account_id) DO NOTHING",
        )
        .bind(account.0)
        .bind(&user.email)
        .bind(&user.full_name)
        .bind(user.created_at.0)
        .bind(user.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(fail)?;
        Ok(self.stored(account).await?.map(customer))
    }

    /// Re-confirms contact facts against the trusted directory, or fails and leaves the account
    /// due. Only a positive answer confirms: the directory must know the account, name a
    /// verified address, and that address must be the one stored once a newer verified change
    /// has been applied. A lookup that cannot be answered, an unknown account, an unverified
    /// address or a disagreement that cannot be resolved are all errors, never a reason to keep
    /// serving the stored recipient for another interval.
    async fn refresh(&self, auth: &dyn AuthDirectory, account: AccountId) -> Result<(), DirectoryError> {
        let user = auth
            .lookup(account)
            .await
            .map_err(|e| DirectoryError(e.0))?
            .ok_or_else(|| DirectoryError("the auth directory does not know this account, so its contact facts cannot be re-confirmed".into()))?;
        self.refresh_user(&user).await
    }

    async fn refresh_user(&self, user: &AuthUser) -> Result<(), DirectoryError> {
        let account = user.account;
        if !user.email_verified {
            return Err(DirectoryError("the auth directory has no verified address for this account".into()));
        }
        sqlx::query(
            "UPDATE transgate_billing.customers
             SET email = $2, full_name = $3, contact_version = billing.us_to_ts($4)
             WHERE account_id = $1 AND COALESCE(contact_version, created_at) < billing.us_to_ts($4)",
        )
        .bind(account.0)
        .bind(&user.email)
        .bind(&user.full_name)
        .bind(user.updated_at.0)
        .execute(&self.pool)
        .await
        .map_err(fail)?;
        let confirmed = sqlx::query(
            "UPDATE transgate_billing.customers SET contact_checked_at = now()
             WHERE account_id = $1 AND lower(email) = lower($2)",
        )
        .bind(account.0)
        .bind(&user.email)
        .execute(&self.pool)
        .await
        .map_err(fail)?;
        if confirmed.rows_affected() == 0 {
            return Err(DirectoryError("the stored address differs from the auth directory's and could not be reconciled".into()));
        }
        Ok(())
    }

    /// The session needs the exact email-confirmation timestamp, which is not stored in the
    /// customer row. Fetch once and reuse those same verified facts for adoption/refresh.
    pub(super) async fn verified_contact(&self, account: AccountId) -> Result<Option<(Customer, Option<Micros>)>, DirectoryError> {
        let Some(auth) = &self.auth else { return Ok(None) };
        let user = auth.lookup(account).await.map_err(|e| DirectoryError(e.0))?
            .ok_or_else(|| DirectoryError("the auth directory does not know this account".into()))?;
        if !user.email_verified {
            return Err(DirectoryError("the auth directory has no verified address for this account".into()));
        }
        let contact = if self.stored(account).await?.is_none() {
            self.adopt_user(&user).await?
        } else {
            self.refresh_user(&user).await?;
            self.stored(account).await?.map(customer)
        }.ok_or_else(|| DirectoryError("the verified customer record is unavailable".into()))?;
        Ok(Some((contact, user.email_confirmed_at)))
    }
}

impl CustomerDirectory for PgCustomerDirectory {
    fn by_account(&self, account: AccountId) -> BoxFuture<'_, Result<Option<Customer>, DirectoryError>> {
        Box::pin(async move {
            let stored = self.stored(account).await?;
            let Some(auth) = &self.auth else { return Ok(stored.map(customer)) };
            match stored {
                None => self.adopt(&**auth, account).await,
                Some(row) if row.4 => {
                    // Contact facts are due for re-confirmation. If the trusted directory cannot
                    // answer, nothing here may be served as if it had: purchase mail or a lookup
                    // could reach an address that has since changed. The stored row stays, still
                    // due, so the next access retries and recovers.
                    self.refresh(&**auth, account).await?;
                    Ok(self.stored(account).await?.map(customer))
                }
                Some(row) => Ok(Some(customer(row))),
            }
        })
    }

    fn by_email<'a>(&'a self, email: &'a str) -> BoxFuture<'a, Result<Option<Customer>, DirectoryError>> {
        Box::pin(async move {
            let row: Option<Row> = sqlx::query_as(
                "SELECT account_id, email, full_name, billing.ts_to_us(created_at), false
                 FROM transgate_billing.customers WHERE lower(email) = lower($1)",
            )
            .bind(email)
            .fetch_optional(&self.pool)
            .await
            .map_err(fail)?;
            Ok(row.map(customer))
        })
    }

    fn register<'a>(&'a self, customer_in: &'a Customer) -> BoxFuture<'a, Result<Customer, DirectoryError>> {
        Box::pin(async move {
            // The first record wins: a later report, however stale, never moves the creation
            // instant and never replaces stored contact facts. Newer verified facts arrive
            // only through the auth directory refresh above.
            sqlx::query(
                "INSERT INTO transgate_billing.customers (account_id, email, full_name, created_at)
                 VALUES ($1, $2, $3, billing.us_to_ts($4))
                 ON CONFLICT (account_id) DO NOTHING",
            )
            .bind(customer_in.account.0)
            .bind(&customer_in.email)
            .bind(&customer_in.full_name)
            .bind(customer_in.created_at.0)
            .execute(&self.pool)
            .await
            .map_err(fail)?;
            let row = self
                .stored(customer_in.account)
                .await?
                .ok_or_else(|| DirectoryError("the customer record disappeared after registration".into()))?;
            Ok(customer(row))
        })
    }
}
