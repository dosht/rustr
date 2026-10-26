//! Charging a job: the decision the legacy `job_status_trigger` and the
//! provider callbacks made, as pure functions of facts the host supplies.
//!
//! Three rules stay deliberately separate (DEF-019, DEF-020): the web
//! 20/45-minute guard in `freemium`, the completed-job bypass eligibility
//! decided here, and the strict deferred deduction after a provider answer.

use serde::{Deserialize, Serialize};

use crate::ledger::{
    Allocation, DebitRefusal, PackageBalance, apply, balance, plan_bypass, plan_strict,
};
use crate::types::SubjectRef;

pub const FREEMIUM_NAME: &str = "freemium";

/// SQL `name != 'freemium'`: a null name is never "not freemium".
pub fn is_paid_name(name: &Option<String>) -> bool {
    matches!(name, Some(n) if n != FREEMIUM_NAME)
}

/// Database definition (FRE-003): no open package carries another name.
pub fn freemium_only(open: &[PackageBalance]) -> bool {
    !open.iter().any(|p| is_paid_name(&p.package.name))
}

/// Only these moves bill, and only once at start (BIL-025, BIL-026, BIL-079).
pub fn is_billable_transition(from: &str, to: &str) -> bool {
    to == "not_started" && matches!(from, "draft" | "ready" | "uploaded")
}

/// Facts about one job at start. The host loads `other_completed_jobs` inside
/// the transaction, after the account lock, never from a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobStart {
    pub subject: SubjectRef,
    pub duration: Option<i64>,
    pub other_completed_jobs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartCharge {
    /// Unknown duration: nothing charged, nothing checked (BIL-028).
    Deferred,
    Strict(Vec<Allocation>),
    Bypass(Allocation),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartRefusal {
    NoAvailablePackage,
    NotEnoughBalance { required: i64, available: i64 },
    NoActivePackage,
    NothingDeducted,
    InvalidMinutes,
}

impl StartRefusal {
    /// The app turns every one of these into the insufficient-balance answer.
    pub fn code(&self) -> &'static str {
        "insufficient_balance"
    }

    pub fn http_status(&self) -> u16 {
        402
    }

    /// Legacy wording without the user id; `freemium_path` selects the FRE-007 text.
    pub fn message(&self, freemium_path: bool) -> String {
        match self {
            StartRefusal::NoAvailablePackage => "No available package".into(),
            StartRefusal::NoActivePackage => "No active package found".into(),
            StartRefusal::NothingDeducted => {
                "Failed to deduct minutes: no packages available or balance insufficient".into()
            }
            StartRefusal::InvalidMinutes => "Invalid minutes".into(),
            StartRefusal::NotEnoughBalance { required, available } if freemium_path => format!(
                "Not enough balance. Please upgrade to continue. (required={required} min, available={available} min)"
            ),
            StartRefusal::NotEnoughBalance { required, available } => {
                format!("Not enough balance: required={required} min, available={available} min")
            }
        }
    }
}

fn from_debit(refusal: DebitRefusal) -> StartRefusal {
    match refusal {
        DebitRefusal::InsufficientBalance { balance, requested } => {
            StartRefusal::NotEnoughBalance { required: requested, available: balance }
        }
        DebitRefusal::NothingDeducted => StartRefusal::NothingDeducted,
        DebitRefusal::NoActivePackage => StartRefusal::NoActivePackage,
        DebitRefusal::InvalidMinutes => StartRefusal::InvalidMinutes,
    }
}

fn strict(minutes: i64, open: &[PackageBalance]) -> Result<Vec<Allocation>, StartRefusal> {
    if balance(open).is_none() {
        return Err(StartRefusal::NoAvailablePackage);
    }
    plan_strict(minutes, open).map_err(from_debit)
}

/// The start decision over the packages open at the command instant.
pub fn decide_job_start(job: &JobStart, open: &[PackageBalance]) -> Result<StartCharge, StartRefusal> {
    let Some(duration) = job.duration else { return Ok(StartCharge::Deferred) };
    if freemium_only(open) && job.other_completed_jobs == 0 {
        return plan_bypass(duration, open).map(StartCharge::Bypass).map_err(from_debit);
    }
    strict(duration, open).map(StartCharge::Strict)
}

/// Deferred billing after a provider answer: always strict, never the bypass.
pub fn decide_deferred(minutes: i64, open: &[PackageBalance]) -> Result<Vec<Allocation>, StartRefusal> {
    strict(minutes, open)
}

/// All-or-nothing batch. Decisions are made in ascending subject id, whatever
/// the order the caller listed them in, and each sees the earlier ones.
pub fn plan_batch(
    jobs: &[JobStart],
    open: &[PackageBalance],
) -> Result<Vec<(SubjectRef, StartCharge)>, StartRefusal> {
    let mut ordered: Vec<JobStart> = jobs.to_vec();
    ordered.sort_by_key(|j| j.subject);
    let mut snapshot = open.to_vec();
    let mut plan = Vec::with_capacity(ordered.len());
    for job in &ordered {
        let charge = decide_job_start(job, &snapshot)?;
        match &charge {
            StartCharge::Strict(rows) => apply(&mut snapshot, rows),
            StartCharge::Bypass(row) => apply(&mut snapshot, std::slice::from_ref(row)),
            StartCharge::Deferred => {}
        }
        plan.push((job.subject, charge));
    }
    Ok(plan)
}

/// Normal web Start stays disabled until every file has a duration (BIL-038).
pub fn web_start_allowed(durations: &[Option<i64>]) -> bool {
    !durations.is_empty() && durations.iter().all(Option::is_some)
}

/// The web refuses below zero; a balance of exactly zero is the database's call.
pub fn web_precheck(balance: Option<i64>) -> bool {
    balance.unwrap_or(0) >= 0
}

/// The URL path knows no duration, so zero or less is refused up front.
pub fn api_precheck(balance: Option<i64>) -> bool {
    balance.unwrap_or(0) > 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorClass {
    InsufficientBalance,
    Internal,
}

pub fn classify_error_message(text: &str) -> ErrorClass {
    let lower = text.to_lowercase();
    if ["not enough balance", "no available package", "failed to deduct minutes"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        ErrorClass::InsufficientBalance
    } else {
        ErrorClass::Internal
    }
}

/// Deepgram `metadata.duration` in seconds, rounded up; missing or zero is nothing.
pub fn provider_minutes_deepgram(duration_seconds: Option<f64>) -> Option<i64> {
    match duration_seconds {
        Some(s) if s.is_finite() && s > 0.0 => Some((s / 60.0).ceil() as i64),
        _ => None,
    }
}

/// Soniox `metadata.duration_ms`, else the largest token `end_ms`.
pub fn provider_minutes_soniox(duration_ms: Option<f64>, max_token_end_ms: Option<f64>) -> Option<i64> {
    let positive = |v: Option<f64>| v.filter(|m| m.is_finite() && *m > 0.0);
    positive(duration_ms)
        .or_else(|| positive(max_token_end_ms))
        .map(|ms| (ms / 60_000.0).ceil() as i64)
}

/// A shorter provider length lowers the job duration only; the ledger is untouched.
pub fn duration_after_provider(current: i64, provider: i64) -> Option<i64> {
    (provider < current).then_some(provider)
}

#[cfg(test)]
mod tests;
