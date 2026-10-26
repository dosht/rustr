//! Account erasure: the billing side of the deletion saga (P3, decisions D5,
//! D9 and D10).
//!
//! Stopping an external recurring subscription is erasure-only. The ordinary
//! cancellation request stays the captured manual-support flow and never calls
//! a provider. Here the adapter reports only what the provider authenticated;
//! whether that proves the subscription no longer renews is decided once, in
//! `termination_confirmed`.

use std::fmt;

use crate::ports::{BillingStore, BillingTx, GatewayError, StoreError};
use crate::types::{AccountId, BoxFuture, Micros, PaymentId, SubjectRef};

/// A subscription bound to the erased account by an earlier verified
/// confirmation: the identity the provider must answer for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminationTarget {
    pub gateway: String,
    pub environment: String,
    pub subscription_ref: String,
    /// The bound customer. A legacy binding without one cannot be confirmed
    /// from a detail answer and stays pending.
    pub customer_ref: Option<String>,
}

/// What an authenticated provider exchange showed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminationEvidence {
    /// The authenticated cancel request for this subscription returned the
    /// documented success.
    CancelSucceeded,
    /// The cancel request did not succeed; the authenticated detail answer
    /// for the requested subscription, as reported.
    Detail { subscription_ref: String, customer_ref: Option<String>, status: Option<String> },
}

/// Erasure-only termination of one bound external subscription. `Err` is
/// never a confirmation: a timeout, an unreadable or ambiguous answer, an
/// error envelope without a usable detail, or missing configuration leaves
/// the erasure step pending for retry or reconciliation.
pub trait SubscriptionTermination: Send + Sync {
    fn gateway(&self) -> &str;
    fn environment(&self) -> &str;
    fn terminate<'a>(&'a self, target: &'a TerminationTarget) -> BoxFuture<'a, Result<TerminationEvidence, GatewayError>>;
}

/// The subscription statuses that no longer renew.
pub const NOT_RENEWING: [&str; 2] = ["CANCELED", "EXPIRED"];

/// D10: a documented cancel success, or an authenticated detail that names
/// exactly the bound subscription and customer with a status that no longer
/// renews. Anything else keeps the step pending.
pub fn termination_confirmed(target: &TerminationTarget, evidence: &TerminationEvidence) -> bool {
    match evidence {
        TerminationEvidence::CancelSucceeded => true,
        TerminationEvidence::Detail { subscription_ref, customer_ref, status } => {
            *subscription_ref == target.subscription_ref
                && target.customer_ref.is_some()
                && *customer_ref == target.customer_ref
                && status.as_deref().is_some_and(|s| NOT_RENEWING.contains(&s))
        }
    }
}

/// An operator's verified closure of a plan checkout that keeps an erasure
/// pending (D10, D13). It is never a risk waiver: the operator attests, from
/// provider evidence kept at `evidence_ref`, both that the checkout can no
/// longer complete and that no subscription created from it still renews. An
/// unpaid snapshot proves neither, so either attestation missing refuses the
/// closure and the checkout stays pending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutClosure {
    pub resolved_by: String,
    /// Verified with the provider: the checkout cannot be paid or completed any more.
    pub checkout_cannot_complete: bool,
    /// Verified with the provider: no subscription from this checkout still renews.
    pub no_renewing_subscription: bool,
    /// Where the provider evidence is retained (ticket, provider record).
    pub evidence_ref: String,
    pub rationale: String,
}

impl CheckoutClosure {
    /// Every attestation and the evidence must be present; the reason otherwise.
    pub fn refusal(&self) -> Option<&'static str> {
        if self.resolved_by.trim().is_empty() {
            return Some("the closing operator must be named");
        }
        if !self.checkout_cannot_complete {
            return Some("the operator must verify that the checkout can no longer complete");
        }
        if !self.no_renewing_subscription {
            return Some("the operator must verify that no subscription from the checkout still renews");
        }
        if self.evidence_ref.trim().is_empty() {
            return Some("a reference to the retained provider evidence is required");
        }
        if self.rationale.trim().is_empty() {
            return Some("a rationale is required");
        }
        None
    }
}

// ---- the erasure saga, billing side -------------------------------------------
//
// 1. `open_account_erasure` (its own transaction): the durable barrier and a
//    pending termination for every bound subscription. From here on ordinary
//    billing (`lock_account`) refuses the account: no grant, charge,
//    provisioning or callback application can recreate or change it.
// 2. `terminate_subscriptions` (network outside any transaction): D10.
// 3. `erase_account_billing` (first billing call inside the host's deletion
//    transaction): the captured D5 cascade, once every termination is confirmed.
// The barrier stays after completion; only identifiers, instants and outcomes
// are kept (D9).

/// Where an account's billing erasure stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErasureStatus {
    pub started_at: Micros,
    /// Set once the local cascade has run.
    pub billing_erased_at: Option<Micros>,
    pub pending: Vec<TerminationTarget>,
    pub confirmed: Vec<TerminationTarget>,
    /// Plan checkouts of the account, whatever their local status, that no
    /// termination target covers: a lost confirmation or a form still payable
    /// may have created a subscription nothing here can stop, so the erasure
    /// stays pending until each is mapped to a target or explicitly excepted.
    pub unbound_plan_payments: Vec<PaymentId>,
}

impl ErasureStatus {
    pub fn subscriptions_stopped(&self) -> bool {
        self.pending.is_empty() && self.unbound_plan_payments.is_empty()
    }
}

/// Rows removed or detached by the local cascade.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ErasureCounts {
    pub runtime_usage: u64,
    pub imported_usage: u64,
    pub outbox_events: u64,
    pub payments: u64,
    pub packages: u64,
    pub subscriptions: u64,
    /// Billing profiles kept with their owner link removed (D5).
    pub billing_info_detached: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErasureReport {
    /// Every account locked, in the order they were locked.
    pub owners_locked: Vec<AccountId>,
    pub counts: ErasureCounts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErasureError {
    Store(StoreError),
    /// `open_account_erasure` has not run for this account.
    NotOpened,
    /// A subscription may still renew: the cascade waits (D10).
    SubscriptionsNotStopped { pending: Vec<TerminationTarget>, unbound_plan_payments: Vec<PaymentId> },
    /// The affected owners changed between discovery and locking; retry.
    OwnersChanged,
}

impl fmt::Display for ErasureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErasureError::Store(e) => write!(f, "{e}"),
            ErasureError::NotOpened => write!(f, "the account's erasure has not been opened"),
            ErasureError::SubscriptionsNotStopped { pending, unbound_plan_payments } => write!(
                f,
                "{} subscription(s) not confirmed stopped and {} unbound plan payment(s): erasure stays pending",
                pending.len(),
                unbound_plan_payments.len()
            ),
            ErasureError::OwnersChanged => write!(f, "the affected owners changed while they were being locked; retry"),
        }
    }
}

impl std::error::Error for ErasureError {}

impl From<StoreError> for ErasureError {
    fn from(e: StoreError) -> Self {
        ErasureError::Store(e)
    }
}

async fn finish<T>(uow: Box<dyn crate::ports::UnitOfWork>, result: Result<T, StoreError>) -> Result<T, StoreError> {
    match result {
        Ok(value) => {
            uow.commit().await?;
            Ok(value)
        }
        Err(e) => {
            let _ = uow.rollback().await;
            Err(e)
        }
    }
}

/// Step 1. Idempotent: reopening an open or finished erasure changes nothing
/// and returns its status.
pub async fn open_account_erasure(store: &dyn BillingStore, account: AccountId, at: Micros) -> Result<ErasureStatus, StoreError> {
    let mut uow = store.begin().await?;
    let result = async {
        uow.tx().open_erasure(account, at).await?;
        uow.tx().erasure_status(account).await?.ok_or_else(|| StoreError::Unavailable("the erasure barrier was not written".into()))
    }
    .await;
    finish(uow, result).await
}

/// Step 2 (D10). Asks each pending subscription's gateway to stop it, with no
/// transaction open during the call, then records the outcome under the
/// account lock. A target whose gateway is not configured stays pending.
pub async fn terminate_subscriptions(
    store: &dyn BillingStore,
    terminators: &[&dyn SubscriptionTermination],
    account: AccountId,
    at: Micros,
) -> Result<ErasureStatus, StoreError> {
    let status = {
        let mut uow = store.begin().await?;
        let status = uow.tx().erasure_status(account).await;
        let _ = uow.rollback().await;
        status?.ok_or_else(|| StoreError::Conflict("the account's erasure has not been opened".into()))?
    };
    for target in &status.pending {
        let terminator = terminators
            .iter()
            .find(|t| t.gateway() == target.gateway && t.environment() == target.environment);
        let (confirmed, outcome) = match terminator {
            None => (false, "gateway_not_configured".to_string()),
            Some(t) => match t.terminate(target).await {
                Ok(evidence) if termination_confirmed(target, &evidence) => (true, "confirmed".to_string()),
                Ok(_) => (false, "not_confirmed".to_string()),
                Err(e) => (false, format!("uncertain: {e}")),
            },
        };
        let mut uow = store.begin().await?;
        let result = async {
            uow.tx().lock_account_for_erasure(account).await?;
            uow.tx().record_termination(account, target, confirmed, &outcome, at).await
        }
        .await;
        finish(uow, result).await?;
    }
    let mut uow = store.begin().await?;
    let status = uow.tx().erasure_status(account).await;
    let _ = uow.rollback().await;
    status?.ok_or_else(|| StoreError::Unavailable("the erasure barrier disappeared".into()))
}

/// Step 3. The first billing call of the host's deletion transaction (D9):
/// discovers every account the cascade changes before taking any lock (the
/// erased account, owners of packages that its imported usage or its erased
/// subjects drew on), locks them in ascending order, rediscovers, then runs the
/// captured cascade (D5). `subjects` are every host subject (job, sample work)
/// of the account being erased.
pub async fn erase_account_billing(
    tx: &mut dyn BillingTx,
    account: AccountId,
    subjects: &[SubjectRef],
    at: Micros,
) -> Result<ErasureReport, ErasureError> {
    // A repeated saga step after completion: the account row is gone, nothing to lock or erase.
    if tx.erasure_status(account).await?.ok_or(ErasureError::NotOpened)?.billing_erased_at.is_some() {
        return Ok(ErasureReport { owners_locked: vec![], counts: ErasureCounts::default() });
    }
    let owners = tx.erasure_owners(account, subjects).await?;
    for owner in &owners {
        tx.lock_account_for_erasure(*owner).await?;
    }
    if tx.erasure_owners(account, subjects).await? != owners {
        return Err(ErasureError::OwnersChanged);
    }
    let status = tx.erasure_status(account).await?.ok_or(ErasureError::NotOpened)?;
    if !status.subscriptions_stopped() {
        return Err(ErasureError::SubscriptionsNotStopped {
            pending: status.pending,
            unbound_plan_payments: status.unbound_plan_payments,
        });
    }
    let counts = tx.erase_account_rows(account, subjects, at).await?;
    Ok(ErasureReport { owners_locked: owners, counts })
}

#[cfg(test)]
mod tests;
