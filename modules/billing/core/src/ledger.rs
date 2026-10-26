//! The derived ledger: packages with a window, usage rows against them, a
//! balance that is never stored, and the allocation of a debit.
//!
//! Allocation follows decision D1. Admission checks the signed aggregate of
//! the open packages. A successful debit then writes exactly the requested
//! minutes over the packages that still have positive capacity, ordered by
//! `(end_at, package_id)`. A package that is already overdrawn is neither
//! drained a second time nor does it enlarge the amount taken from the next
//! one (the legacy prefix formula overcharged there, DEF-034), and packages
//! sharing an expiry are no longer ambiguous (DEF-016).

use serde::{Deserialize, Serialize};

use crate::types::{AccountId, Micros, PackageId, PaymentId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub id: PackageId,
    pub account: AccountId,
    pub payment_id: Option<PaymentId>,
    /// Free text and possibly null in imported data; rules compare the exact
    /// text `freemium`, nothing else.
    pub name: Option<String>,
    pub minutes_included: i64,
    pub start_at: Micros,
    pub end_at: Micros,
}

/// A package with the sum of every usage row that references it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageBalance {
    pub package: Package,
    pub consumed: i64,
}

impl PackageBalance {
    pub fn remaining(&self) -> i64 {
        self.package.minutes_included - self.consumed
    }
}

/// `start_at <= at <= end_at`: both ends are included (BIL-005).
pub fn is_open(package: &Package, at: Micros) -> bool {
    package.start_at <= at && at <= package.end_at
}

pub fn open_at(all: &[PackageBalance], at: Micros) -> Vec<PackageBalance> {
    all.iter().filter(|p| is_open(&p.package, at)).cloned().collect()
}

pub fn for_account(all: &[PackageBalance], account: AccountId) -> Vec<PackageBalance> {
    all.iter().filter(|p| p.package.account == account).cloned().collect()
}

/// `None` is "no open package": the legacy function and view return no row.
pub fn balance(open: &[PackageBalance]) -> Option<i64> {
    if open.is_empty() {
        None
    } else {
        Some(open.iter().map(PackageBalance::remaining).sum())
    }
}

pub fn balance_or_zero(open: &[PackageBalance]) -> i64 {
    balance(open).unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allocation {
    pub package_id: PackageId,
    pub minutes: i64,
}

/// One row of the legacy `request_breakdown_over_packages` result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Breakdown {
    pub package_id: PackageId,
    pub request_minutes: i64,
    pub carry_over_minutes: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebitRefusal {
    /// Balance minus the request is below zero. Nothing is written.
    InsufficientBalance { balance: i64, requested: i64 },
    /// Zero minutes, or no capacity to write on: the legacy "failed to deduct".
    NothingDeducted,
    /// The bypass found no open package at all.
    NoActivePackage,
    InvalidMinutes,
}

impl DebitRefusal {
    /// The number the legacy function returned: balance minus the request.
    pub fn balance_after(&self) -> Option<i64> {
        match self {
            DebitRefusal::InsufficientBalance { balance, requested } => Some(balance - requested),
            _ => None,
        }
    }
}

fn ordered(open: &[PackageBalance]) -> Vec<&PackageBalance> {
    let mut rows: Vec<&PackageBalance> = open.iter().collect();
    rows.sort_by_key(|p| (p.package.end_at, p.package.id));
    rows
}

/// Split of `requested` over the open packages, with the legacy carry value:
/// the part of the request not yet covered after this package.
pub fn breakdown(requested: i64, open: &[PackageBalance]) -> Vec<Breakdown> {
    let mut running_capacity = 0i64;
    let mut still_needed = requested.max(0);
    ordered(open)
        .into_iter()
        .map(|p| {
            let capacity = p.remaining().max(0);
            running_capacity += capacity;
            let take = capacity.min(still_needed);
            still_needed -= take;
            Breakdown {
                package_id: p.package.id,
                request_minutes: take,
                carry_over_minutes: (running_capacity - requested).min(0),
            }
        })
        .collect()
}

/// The strict debit: refuse when the signed balance cannot cover the request,
/// otherwise allocate exactly the request over positive capacities.
pub fn plan_strict(requested: i64, open: &[PackageBalance]) -> Result<Vec<Allocation>, DebitRefusal> {
    if requested < 0 {
        return Err(DebitRefusal::InvalidMinutes);
    }
    // With no open package the source skips the balance check (NULL < 0 is not
    // true) and fails to deduct: BIL-022.
    if open.is_empty() {
        return Err(DebitRefusal::NothingDeducted);
    }
    let available = balance_or_zero(open);
    if available - requested < 0 {
        return Err(DebitRefusal::InsufficientBalance { balance: available, requested });
    }
    let allocations: Vec<Allocation> = breakdown(requested, open)
        .into_iter()
        .filter(|b| b.request_minutes > 0)
        .map(|b| Allocation { package_id: b.package_id, minutes: b.request_minutes })
        .collect();
    if allocations.is_empty() {
        return Err(DebitRefusal::NothingDeducted);
    }
    Ok(allocations)
}

/// First-job bypass: the whole amount lands on the earliest-ending open
/// package even when it holds less. A zero amount is a real zero-minute write.
pub fn plan_bypass(requested: i64, open: &[PackageBalance]) -> Result<Allocation, DebitRefusal> {
    if requested < 0 {
        return Err(DebitRefusal::InvalidMinutes);
    }
    let first = ordered(open).into_iter().next().ok_or(DebitRefusal::NoActivePackage)?;
    Ok(Allocation { package_id: first.package.id, minutes: requested })
}

/// The single successor of the three legacy `deduct_minutes` overloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebitRequest {
    pub minutes: i64,
    pub skip_balance_check: bool,
}

pub fn plan_debit(request: &DebitRequest, open: &[PackageBalance]) -> Result<Vec<Allocation>, DebitRefusal> {
    if request.skip_balance_check {
        plan_bypass(request.minutes, open).map(|a| vec![a])
    } else {
        plan_strict(request.minutes, open)
    }
}

/// Applies planned allocations to an in-memory snapshot, so later decisions in
/// one batch see earlier ones.
pub fn apply(open: &mut [PackageBalance], allocations: &[Allocation]) {
    for allocation in allocations {
        if let Some(p) = open.iter_mut().find(|p| p.package.id == allocation.package_id) {
            p.consumed += allocation.minutes;
        }
    }
}

/// Whole minutes for a file: rounded up, never down (BIL-010).
pub fn minutes_from_seconds(seconds: f64) -> i64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    (seconds / 60.0).ceil() as i64
}

/// Each file is rounded first, then the rounded minutes are summed.
pub fn batch_minutes(file_seconds: &[f64]) -> i64 {
    file_seconds.iter().map(|s| minutes_from_seconds(*s)).sum()
}

#[cfg(test)]
mod tests;
