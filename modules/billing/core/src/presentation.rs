//! Presentation policies: what the screens decide, as pure functions. The
//! shells (P5) render these; nothing here draws.

use serde::Serialize;

pub mod confirm;
pub mod fixtures;
pub mod invoice;
pub mod invoices;
pub mod purchase;
pub mod subscription;

use crate::calendar::{Interval, add_interval, format_dmy};
use crate::decimal::{Decimal, Rounding};
use crate::grants::{PackageSpec, PaymentStatus};
use crate::freemium::is_freemium_app1;
use crate::ledger::{Package, PackageBalance, balance_or_zero, open_at};
use crate::model::Payment;
use crate::pricing::{Currency, PlanInterval, analytics_usd};
use crate::types::Micros;

pub const BALANCE_REFRESH_SECONDS: u32 = 30;
pub const CONFIRM_COUNTDOWN_SECONDS: u32 = 5;
pub const SIGN_IN_MESSAGE: &str = "Please sign in and try again.";
pub const CHECKOUT_START_FAILED: &str = "We couldn't start your checkout";
pub const PAYG_WILL_NOT_RENEW: &str = "Pay-as-you-go will not renew";
pub const CANCELED_MESSAGE: &str = "Your subscription is canceled, but you can still use your minutes.";

/// Under 60 minutes as `N min`, otherwise `H hr , M min`; missing shows `-`.
pub fn balance_text(balance: Option<i64>) -> String {
    match balance {
        None => "-".into(),
        Some(m) if m < 60 => format!("{m} min"),
        Some(m) => format!("{} hr , {} min", m / 60, m % 60),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StartAnswerMeta {
    pub new_balance: i64,
    pub is_freemium: bool,
}

/// BIL-074: what a started job answers with, derived from the account's packages after the
/// debit, as the source reads its balance view and `Balances.freemium()`: the balance in whole
/// minutes over the packages open at `at` (0 when none), and the application's first
/// definition of freemium (FRE-013): exactly one package ever, named freemium, any dates.
/// The caller reads every package row inside the debiting transaction.
pub fn start_answer_meta(all_after_debit: &[PackageBalance], at: Micros) -> StartAnswerMeta {
    let packages: Vec<Package> = all_after_debit.iter().map(|p| p.package.clone()).collect();
    StartAnswerMeta {
        new_balance: balance_or_zero(&open_at(all_after_debit, at)),
        is_freemium: is_freemium_app1(&packages),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum RenewalNotice {
    InDays(u32),
    WillNotRenew,
}

impl RenewalNotice {
    pub fn text(&self) -> Option<&'static str> {
        match self {
            RenewalNotice::InDays(_) => None,
            RenewalNotice::WillNotRenew => Some(PAYG_WILL_NOT_RENEW),
        }
    }
}

/// The modal's copy uses fixed day counts. It is not the calendar date of the
/// next payment (`next_payment_date`): the two stay separate source rules.
pub fn renewal_notice(interval: Option<PlanInterval>) -> RenewalNotice {
    match interval {
        Some(PlanInterval::Monthly) => RenewalNotice::InDays(30),
        Some(PlanInterval::Yearly) => RenewalNotice::InDays(365),
        None => RenewalNotice::WillNotRenew,
    }
}

/// Payment created plus `count` months or years; empty when canceled, when the
/// metadata is missing or for any other interval (BIL-063).
pub fn next_payment_date(created: Micros, spec: Option<&PackageSpec>, canceled: bool) -> Option<Micros> {
    if canceled {
        return None;
    }
    let spec = spec?;
    let interval = Interval::parse(&spec.interval)?;
    add_interval(created, interval, i64::from(spec.count)).ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SubscriptionView {
    pub message: Option<&'static str>,
    pub show_next_payment: bool,
}

pub fn subscription_view(canceled: bool) -> SubscriptionView {
    if canceled {
        SubscriptionView { message: Some(CANCELED_MESSAGE), show_next_payment: false }
    } else {
        SubscriptionView { message: None, show_next_payment: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum CurrentPlan {
    Plan { name: String, minutes: i64, ends_at: Micros },
    NoPackage,
}

/// The first open package named exactly `PREMIUM` or `BUSINESS`.
pub fn current_plan(open: &[Package]) -> CurrentPlan {
    let mut ordered: Vec<&Package> = open.iter().collect();
    ordered.sort_by_key(|p| p.id);
    ordered
        .into_iter()
        .find(|p| matches!(p.name.as_deref(), Some("PREMIUM") | Some("BUSINESS")))
        .map(|p| CurrentPlan::Plan {
            name: p.name.clone().unwrap_or_default(),
            minutes: p.minutes_included,
            ends_at: p.end_at,
        })
        .unwrap_or(CurrentPlan::NoPackage)
}

/// `TRAN` plus the payment id padded to eight digits.
pub fn invoice_id(payment_id: i64) -> String {
    format!("TRAN{payment_id:08}")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HistoryRow {
    pub payment_id: i64,
    pub invoice_id: String,
    pub date: String,
    pub amount_text: String,
    pub package_name: String,
    pub gateway: String,
    pub currency: String,
    pub receipt_url: Option<String>,
}

pub fn history_row(payment: &Payment) -> HistoryRow {
    let amount = payment
        .amount
        .quantize(2, Rounding::HalfUp)
        .map(|a| a.to_string())
        .unwrap_or_else(|_| payment.amount.to_string());
    HistoryRow {
        payment_id: payment.id,
        invoice_id: invoice_id(payment.id),
        date: format_dmy(payment.created),
        amount_text: format!("{amount} {}", payment.currency),
        package_name: payment.package_name().unwrap_or_else(|| "N/A".into()),
        gateway: payment.gateway.clone(),
        currency: payment.currency.clone(),
        receipt_url: payment.receipt_url.clone(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum AnalyticsIntent {
    PaymentCompleted,
    PaymentFailed,
    SubscriptionStarted,
    Purchase { value_usd: Decimal, currency: &'static str },
}

/// Events for a confirmation. `changed_to` is `Some` only when this call
/// really moved the status, so a repeat or a parallel confirm emits nothing.
pub fn confirm_analytics(changed_to: Option<PaymentStatus>, payment: &Payment, rate: &Decimal) -> Vec<AnalyticsIntent> {
    match changed_to {
        Some(PaymentStatus::Completed) => {
            let mut events = vec![AnalyticsIntent::PaymentCompleted];
            if payment.pricing_plan_id.is_some() && !payment.is_renewal() {
                events.push(AnalyticsIntent::SubscriptionStarted);
            }
            let currency = Currency::parse(&payment.currency).unwrap_or(Currency::Usd);
            events.push(AnalyticsIntent::Purchase {
                value_usd: analytics_usd(&payment.amount, currency, rate),
                currency: "USD",
            });
            events
        }
        Some(PaymentStatus::Failed) => vec![AnalyticsIntent::PaymentFailed],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests;
