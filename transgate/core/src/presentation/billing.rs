//! Transgate catalog projections of billing-owned codes. Financial policy stays
//! in billing-core; these fields let shells render messages without matching codes.

use billing_core::presentation::RenewalNotice;
use billing_core::presentation::confirm::{ConfirmKind, ConfirmVm};
use billing_core::presentation::invoices::InvoicesVm;
use billing_core::presentation::purchase::{PurchaseFailure, PurchaseVm};
use billing_core::presentation::subscription::{SubscriptionFailure, SubscriptionVm};
use billing_core::pricing::CouponMessage;
use serde::Serialize;

use crate::ids::Micros;
use super::msg::{Msg, app, common};

pub const NO_ACTIVE_SUBSCRIPTION: Msg = app("p5.billing.no_active_subscription");
pub const PAYMENT_UNCONFIRMED: Msg = app("p5.billing.payment_unconfirmed");
pub const CUSTOMER_NOT_FOUND: Msg = app("p5.billing.customer_not_found");
pub const LOOKUP_FAILED: Msg = app("p5.billing.lookup_failed");

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BillingMessages {
    pub purchase_failure: Option<Msg>,
    pub coupon_notice: Option<Msg>,
    pub renewal: Option<Msg>,
    /// Format with the browser's locale date, after `renewal`; no date arithmetic in the shell.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub renewal_at_ms: Option<i64>,
    pub subscription_failure: Option<Msg>,
    pub invoice_failure: Option<Msg>,
    pub document_failure: Option<Msg>,
    pub customer_not_found: Option<Msg>,
}

fn purchase_failure(failure: &PurchaseFailure) -> Msg {
    match failure {
        PurchaseFailure::Unauthenticated => common("Please sign in and try again."),
        PurchaseFailure::Erased => super::app::ACCOUNT_ERASED,
        PurchaseFailure::Unavailable => common("Something went wrong while preparing your payment. No charge was made — please try again."),
        PurchaseFailure::Refused { .. } => common("We couldn't start your checkout"),
    }
}

pub fn messages(purchase: Option<&PurchaseVm>, subscription: &SubscriptionVm, invoices: Option<&InvoicesVm>, lookup_failed: bool, now: Micros) -> BillingMessages {
    BillingMessages {
        purchase_failure: purchase.and_then(|p| p.failure.as_ref()).map(purchase_failure),
        coupon_notice: purchase.and_then(|p| p.coupon_notice).map(|notice| common(match notice {
            CouponMessage::NotValid => "Coupon is not valid",
            CouponMessage::Expired => "Coupon has expired",
            CouponMessage::Error => "Error",
        })),
        renewal: purchase.map(|p| common(match p.renewal {
            RenewalNotice::WillNotRenew => "Pay-as-you-go will not renew",
            RenewalNotice::InDays(_) => "Renews on ",
        })),
        renewal_at_ms: purchase.and_then(|p| match p.renewal {
            RenewalNotice::WillNotRenew => None,
            RenewalNotice::InDays(days) => Some(now.0.div_euclid(1000).saturating_add(i64::from(days) * 86_400_000)),
        }),
        subscription_failure: subscription.failure.as_ref().map(|failure| match failure {
            SubscriptionFailure::NoActiveSubscription => NO_ACTIVE_SUBSCRIPTION,
            SubscriptionFailure::Request(PurchaseFailure::Unavailable | PurchaseFailure::Refused { .. }) => app("An unexpected error occurred. Please try again."),
            SubscriptionFailure::Request(failure) => purchase_failure(failure),
        }),
        invoice_failure: invoices.and_then(|i| i.failure.as_ref()).map(|_| if lookup_failed { LOOKUP_FAILED } else { common("This feature is currently unavailable") }),
        document_failure: invoices.and_then(|i| i.document_failure.as_ref()).map(|_| app("An unexpected error occurred. Please try again.")),
        customer_not_found: invoices.and_then(|i| i.admin.as_ref()).filter(|a| a.not_found).map(|_| CUSTOMER_NOT_FOUND),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ConfirmMessages {
    pub title: Msg,
    pub status: Option<Msg>,
    pub message: Msg,
    pub detail: Option<Msg>,
    pub redirect: Option<Msg>,
}

pub fn confirmation(vm: &ConfirmVm) -> ConfirmMessages {
    let (status, message, detail) = match vm.kind {
        ConfirmKind::Succeeded => (Some(common("Successful!")), common("Your order has been successfully processed."), Some(common("Thank you for transcibing with us!"))),
        ConfirmKind::Failed => (Some(common("Failed")), common("Oops! Something went wrong during the checkout process."), Some(common("Please try again later or contact customer support."))),
        ConfirmKind::Unconfirmed => (None, PAYMENT_UNCONFIRMED, None),
    };
    ConfirmMessages {
        title: common("Checkout"), status, message, detail,
        redirect: vm.countdown.map(|count| common("redirectMessage").number("count", i64::from(count))),
    }
}
