//! The checkout confirmation page (WEB-BIL-011..014, BIL-059/060/072, P5 D8) as a
//! pure state machine over the one confirmation result the host embedded in the
//! page. A definitive result counts down to the locale root; an uncertain one stays
//! visibly unconfirmed and never counts down to an implied outcome.

use serde::{Deserialize, Serialize};

use crate::decimal::Decimal;
use crate::grants::PaymentStatus;
use crate::presentation::CONFIRM_COUNTDOWN_SECONDS;

/// One analytics intent as the confirmation JSON carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfirmAnalytic {
    PaymentCompleted,
    PaymentFailed,
    SubscriptionStarted,
    Purchase { value_usd: Decimal, currency: String },
}

/// The `ConfirmView` JSON of a confirmation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmedPayment {
    pub payment_id: i64,
    pub status: PaymentStatus,
    pub success: bool,
    /// False for a repeat: nothing changed and there is nothing to report.
    pub changed: bool,
    pub analytics: Vec<ConfirmAnalytic>,
    pub redirect_after_seconds: u32,
}

/// The JSON embedded in the page (`billing_http::ConfirmOutcome`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ConfirmPayload {
    Confirmed { view: ConfirmedPayment },
    Failed { status: u16, code: String, message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum ConfirmKind {
    Succeeded,
    /// A definitive failure: declined, unknown checkout, refused or wrong method.
    Failed,
    /// The provider or the store could not say: not a success, not a decline.
    Unconfirmed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ConfirmEffect {
    /// Report these intents once, subject to the shell's consent gate.
    Track(Vec<ConfirmAnalytic>),
    /// The countdown ended: go to the locale root.
    NavigateHome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmEvent {
    /// One second passed.
    Tick,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ConfirmVm {
    pub kind: ConfirmKind,
    /// Seconds left before going home; `None` for an unconfirmed result.
    pub countdown: Option<u32>,
    pub payment_id: Option<i64>,
}

pub struct ConfirmPageState {
    kind: ConfirmKind,
    countdown: Option<u32>,
    payment_id: Option<i64>,
    navigated: bool,
}

impl ConfirmPageState {
    /// `None` is a page reached without a confirmation (a GET): the source failure page.
    pub fn new(payload: Option<ConfirmPayload>) -> (ConfirmPageState, Vec<ConfirmEffect>) {
        let (kind, countdown, payment_id, effects) = match payload {
            Some(ConfirmPayload::Confirmed { view }) => {
                let kind = match view.status {
                    _ if view.success => ConfirmKind::Succeeded,
                    PaymentStatus::Failed | PaymentStatus::Canceled => ConfirmKind::Failed,
                    _ => ConfirmKind::Unconfirmed,
                };
                // The server lists intents only for a confirmation that changed the payment.
                let effects = if view.changed && !view.analytics.is_empty() { vec![ConfirmEffect::Track(view.analytics)] } else { vec![] };
                let countdown = (kind != ConfirmKind::Unconfirmed).then_some(view.redirect_after_seconds);
                (kind, countdown, Some(view.payment_id), effects)
            }
            // 504 provider_uncertain and storage failures cannot tell what happened.
            Some(ConfirmPayload::Failed { status, code, .. }) if status >= 500 && code != "checkout_failed" => {
                (ConfirmKind::Unconfirmed, None, None, vec![])
            }
            Some(ConfirmPayload::Failed { .. }) | None => (ConfirmKind::Failed, Some(CONFIRM_COUNTDOWN_SECONDS), None, vec![]),
        };
        (ConfirmPageState { kind, countdown, payment_id, navigated: false }, effects)
    }

    pub fn update(&mut self, event: ConfirmEvent) -> Vec<ConfirmEffect> {
        match event {
            ConfirmEvent::Tick => {
                let Some(left) = self.countdown else { return vec![] };
                let left = left.saturating_sub(1);
                self.countdown = Some(left);
                if left == 0 && !self.navigated {
                    self.navigated = true;
                    return vec![ConfirmEffect::NavigateHome];
                }
                vec![]
            }
        }
    }

    pub fn view(&self) -> ConfirmVm {
        ConfirmVm { kind: self.kind, countdown: self.countdown, payment_id: self.payment_id }
    }
}

#[cfg(test)]
#[path = "confirm_tests.rs"]
mod tests;
