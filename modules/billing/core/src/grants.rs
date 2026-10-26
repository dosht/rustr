//! Turning a completed payment into packages, and the payment status
//! transitions that decide when that happens.
//!
//! The legacy trigger granted only on `pending -> completed`, so a payment that
//! failed once and later completed paid for nothing (DEF-004). Here a payment
//! grants its whole schedule on the first transition into `completed` from
//! `pending` or `failed`, in the same transaction as the transition, and a
//! unique `(payment, sequence)` constraint backs that up.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::calendar::{Interval, add_interval};
use crate::types::Micros;

/// The `metadata.package` block of a payment: the schedule it grants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSpec {
    pub name: String,
    pub minutes: i64,
    pub count: u32,
    /// Free text in stored metadata; only `month` and `year` are valid.
    pub interval: String,
    pub frequency: u32,
    pub expiry: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantedPackage {
    /// 1-based position inside the payment's schedule.
    pub sequence: i32,
    pub name: String,
    pub minutes: i64,
    pub start_at: Micros,
    pub end_at: Micros,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantError {
    InvalidInterval(String),
    Calendar(String),
}

impl fmt::Display for GrantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GrantError::InvalidInterval(x) => write!(f, "Invalid duration interval ({x})"),
            GrantError::Calendar(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for GrantError {}

/// Package 1 starts at the payment row's creation time. Each next package
/// starts `frequency` intervals after the previous start, added one step at a
/// time (so a 31st clamps once and stays clamped, DEF-014 kept). Each package
/// ends `expiry` intervals after its own start.
pub fn expand_grant(created: Micros, spec: &PackageSpec) -> Result<Vec<GrantedPackage>, GrantError> {
    let interval = Interval::parse(&spec.interval)
        .ok_or_else(|| GrantError::InvalidInterval(spec.interval.clone()))?;
    let calendar = |e: crate::calendar::CalendarError| GrantError::Calendar(e.to_string());
    let mut granted = Vec::with_capacity(spec.count as usize);
    let mut start = created;
    for sequence in 1..=spec.count {
        if sequence > 1 {
            start = add_interval(start, interval, i64::from(spec.frequency)).map_err(calendar)?;
        }
        let end = add_interval(start, interval, i64::from(spec.expiry)).map_err(calendar)?;
        granted.push(GrantedPackage {
            sequence: sequence as i32,
            name: spec.name.clone(),
            minutes: spec.minutes,
            start_at: start,
            end_at: end,
        });
    }
    Ok(granted)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentStatus {
    Pending,
    Completed,
    Failed,
    /// Exists in the legacy enum; no use case ever produces it (BIL-078).
    Canceled,
}

impl PaymentStatus {
    pub fn parse(text: &str) -> Option<PaymentStatus> {
        match text {
            "pending" => Some(PaymentStatus::Pending),
            "completed" => Some(PaymentStatus::Completed),
            "failed" => Some(PaymentStatus::Failed),
            "canceled" => Some(PaymentStatus::Canceled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PaymentStatus::Pending => "pending",
            PaymentStatus::Completed => "completed",
            PaymentStatus::Failed => "failed",
            PaymentStatus::Canceled => "canceled",
        }
    }
}

/// What a definite provider answer says about a payment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    Apply { to: PaymentStatus, grants: bool },
    Ignore,
}

/// Applies BIL-058 and DEF-004. As in the source, an outcome is applied only if
/// the row is not already completed and not already in the same status: a
/// completion is never undone by a late failure, a failed or canceled payment
/// can still complete (and then grants once, on that newly authenticated
/// completion, never backfilled for history), repeats do nothing.
pub fn transition(current: PaymentStatus, observation: Observation) -> Transition {
    match (current, observation) {
        (PaymentStatus::Pending | PaymentStatus::Failed | PaymentStatus::Canceled, Observation::Completed) => {
            Transition::Apply { to: PaymentStatus::Completed, grants: true }
        }
        (PaymentStatus::Pending | PaymentStatus::Canceled, Observation::Failed) => {
            Transition::Apply { to: PaymentStatus::Failed, grants: false }
        }
        _ => Transition::Ignore,
    }
}

#[cfg(test)]
mod tests;
