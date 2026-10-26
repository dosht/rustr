//! The settings Subscription tab as a pure state machine (WEB-BIL-022/023,
//! BIL-062/063). The server derives the plan, its payment and the next payment
//! date; the shell renders codes and instants, never English.

use serde::{Deserialize, Serialize};

use crate::app::CancelView;
use crate::grants::PackageSpec;
use crate::ledger::Package;
use crate::model::Payment;
use crate::presentation::next_payment_date;
use crate::presentation::purchase::PurchaseFailure;
use crate::types::Micros;

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// The current plan's payment, as `GET /api/billing/subscription` returns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionPayment {
    pub payment_id: i64,
    pub created: Micros,
    pub canceled_at: Option<Micros>,
    /// BIL-063: created plus the plan's count of intervals; none when canceled or unknown.
    pub next_payment_at: Option<Micros>,
}

/// The open Premium or Business package and its payment, if any.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionFacts {
    pub package_name: String,
    pub minutes_included: i64,
    pub valid_until: Micros,
    pub payment: Option<SubscriptionPayment>,
}

/// The first open package named exactly `PREMIUM` or `BUSINESS` (the same choice
/// as `current_plan`) with the payment that bought it, when that payment is the account's.
pub fn subscription_facts(open: &[Package], payment: impl FnOnce(i64) -> Option<Payment>) -> Option<SubscriptionFacts> {
    let mut ordered: Vec<&Package> = open.iter().collect();
    ordered.sort_by_key(|p| p.id);
    let plan = ordered.into_iter().find(|p| matches!(p.name.as_deref(), Some("PREMIUM") | Some("BUSINESS")))?;
    let payment = plan.payment_id.and_then(payment).filter(|p| p.account == plan.account).map(|p| {
        let spec: Option<PackageSpec> = p.spec();
        SubscriptionPayment {
            payment_id: p.id,
            created: p.created,
            canceled_at: p.subscription_canceled_at,
            next_payment_at: next_payment_date(p.created, spec.as_ref(), p.subscription_canceled_at.is_some()),
        }
    });
    Some(SubscriptionFacts {
        package_name: plan.name.clone().unwrap_or_default(),
        minutes_included: plan.minutes_included,
        valid_until: plan.end_at,
        payment,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum SubscriptionFailure {
    /// Cancel was confirmed with no payment to cancel (source "No active subscription to cancel").
    NoActiveSubscription,
    Request(PurchaseFailure),
}

/// The source `paymentEvents.subscriptionCancelled` intent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SubscriptionCancelled {
    pub plan: String,
    pub days_subscribed: Option<i64>,
}

impl SubscriptionCancelled {
    pub const EVENT: &'static str = "subscription_cancelled";

    pub fn properties(&self) -> serde_json::Value {
        serde_json::json!({ "plan": self.plan, "days_subscribed": self.days_subscribed })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum SubscriptionEffect {
    /// `GET /api/billing/subscription`.
    Load,
    /// `POST /api/billing/cancel-subscription` with `{payment_id}`.
    Cancel { payment_id: i64 },
    Analytics(SubscriptionCancelled),
    /// "Upgrade Now" opens the purchase modal.
    OpenPurchase,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubscriptionEvent {
    Loaded(Result<Option<SubscriptionFacts>, PurchaseFailure>),
    Refresh,
    UpgradeClicked,
    CancelClicked,
    CancelDismissed,
    CancelConfirmed,
    CancelAnswered(Result<CancelView, PurchaseFailure>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PlanSummary {
    pub name: String,
    pub minutes_included: i64,
    pub valid_until: Micros,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SubscriptionVm {
    pub loading: bool,
    /// `None` shows "No Package" and the upgrade action.
    pub plan: Option<PlanSummary>,
    /// `None` prints `--`.
    pub next_payment: Option<Micros>,
    /// BIL-062: canceled shows the keep-your-minutes message instead of the cancel action.
    pub canceled: bool,
    pub show_cancel: bool,
    pub confirming: bool,
    pub cancelling: bool,
    pub failure: Option<SubscriptionFailure>,
}

#[derive(Default)]
pub struct SubscriptionState {
    loading: bool,
    facts: Option<SubscriptionFacts>,
    confirming: bool,
    cancelling: bool,
    failure: Option<SubscriptionFailure>,
}

impl SubscriptionState {
    pub fn new() -> (SubscriptionState, Vec<SubscriptionEffect>) {
        (SubscriptionState { loading: true, ..SubscriptionState::default() }, vec![SubscriptionEffect::Load])
    }

    fn canceled(&self) -> bool {
        self.facts.as_ref().and_then(|f| f.payment.as_ref()).is_some_and(|p| p.canceled_at.is_some())
    }

    pub fn update(&mut self, event: SubscriptionEvent, now: Micros) -> Vec<SubscriptionEffect> {
        match event {
            SubscriptionEvent::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok(facts) => {
                        self.facts = facts;
                        self.failure = None;
                    }
                    // The source shows "No Package" when loading fails; the failure stays visible.
                    Err(failure) => {
                        self.facts = None;
                        self.failure = Some(SubscriptionFailure::Request(failure));
                    }
                }
                vec![]
            }
            SubscriptionEvent::Refresh => {
                self.loading = true;
                vec![SubscriptionEffect::Load]
            }
            SubscriptionEvent::UpgradeClicked => vec![SubscriptionEffect::OpenPurchase],
            SubscriptionEvent::CancelClicked => {
                if self.facts.is_some() && !self.canceled() {
                    self.confirming = true;
                    self.failure = None;
                }
                vec![]
            }
            SubscriptionEvent::CancelDismissed => {
                if !self.cancelling {
                    self.confirming = false;
                }
                vec![]
            }
            SubscriptionEvent::CancelConfirmed => {
                if !self.confirming || self.cancelling {
                    return vec![];
                }
                match self.facts.as_ref().and_then(|f| f.payment.as_ref()) {
                    // The confirmation stays open, as when the source's cancel throws.
                    None => {
                        self.failure = Some(SubscriptionFailure::NoActiveSubscription);
                        vec![]
                    }
                    Some(payment) => {
                        self.cancelling = true;
                        self.failure = None;
                        vec![SubscriptionEffect::Cancel { payment_id: payment.payment_id }]
                    }
                }
            }
            SubscriptionEvent::CancelAnswered(result) => {
                if !self.cancelling {
                    return vec![];
                }
                self.cancelling = false;
                match result {
                    Ok(view) => {
                        self.confirming = false;
                        let Some(facts) = self.facts.as_mut() else { return vec![] };
                        let plan = facts.package_name.clone();
                        let Some(payment) = facts.payment.as_mut().filter(|p| p.payment_id == view.payment_id) else { return vec![] };
                        // The server's stored instant, not a local guess.
                        payment.canceled_at = Some(view.canceled_at);
                        payment.next_payment_at = None;
                        let days = (now.0 - payment.created.0).div_euclid(MICROS_PER_DAY);
                        vec![SubscriptionEffect::Analytics(SubscriptionCancelled {
                            plan: if plan.is_empty() { "unknown".into() } else { plan },
                            days_subscribed: Some(days),
                        })]
                    }
                    Err(failure) => {
                        self.failure = Some(SubscriptionFailure::Request(failure));
                        vec![]
                    }
                }
            }
        }
    }

    pub fn view(&self) -> SubscriptionVm {
        let canceled = self.canceled();
        SubscriptionVm {
            loading: self.loading,
            plan: self.facts.as_ref().map(|f| PlanSummary {
                name: f.package_name.clone(),
                minutes_included: f.minutes_included,
                valid_until: f.valid_until,
            }),
            next_payment: self.facts.as_ref().and_then(|f| f.payment.as_ref()).and_then(|p| p.next_payment_at),
            canceled,
            show_cancel: self.facts.is_some() && !canceled,
            confirming: self.confirming,
            cancelling: self.cancelling,
            failure: self.failure.clone(),
        }
    }
}

#[cfg(test)]
#[path = "subscription_tests.rs"]
mod tests;
