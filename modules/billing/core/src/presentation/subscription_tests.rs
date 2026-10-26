use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::calendar::utc;
use crate::decimal::Decimal;
use crate::grants::PaymentStatus;
use crate::types::AccountId;

const NOW: Micros = Micros(1_768_478_400_000_000);

fn account() -> AccountId {
    AccountId(Uuid::from_u128(7))
}

fn package(id: i64, name: &str, payment_id: Option<i64>) -> Package {
    Package {
        id,
        account: account(),
        payment_id,
        name: Some(name.into()),
        minutes_included: 1200,
        start_at: Micros(NOW.0 - 86_400_000_000),
        end_at: Micros(NOW.0 + 30 * 86_400_000_000),
    }
}

fn payment(id: i64, canceled_at: Option<Micros>) -> Payment {
    Payment {
        id,
        account: account(),
        amount: Decimal::parse("14.00").unwrap(),
        currency: "USD".into(),
        gateway: "iyzico".into(),
        status: PaymentStatus::Completed,
        token: Some("tok".into()),
        created: utc(2026, 1, 1, 12, 0, 0, 0),
        environment: "live".into(),
        metadata: json!({"package": {"name": "PREMIUM", "minutes": 1200, "count": 1, "interval": "month", "frequency": 1, "expiry": 1}}),
        coupon_id: None,
        pricing_plan_id: Some("plan".into()),
        customer_id: None,
        subscription_ref: None,
        provider_order_ref: None,
        subscription_canceled_at: canceled_at,
        observed: None,
        receipt_url: None,
    }
}

// SubscriptionService.getCurrentPackage + getPaymentById: the open Premium/Business package
// and the payment that bought it; another account's payment is never attached.
#[test]
fn the_facts_are_the_open_plan_with_its_own_payment_and_the_next_payment_date() {
    let open = vec![package(3, "Pay-as-you-Go", Some(1)), package(5, "PREMIUM", Some(9))];
    let facts = subscription_facts(&open, |id| (id == 9).then(|| payment(9, None))).unwrap();
    assert_eq!((facts.package_name.as_str(), facts.minutes_included), ("PREMIUM", 1200));
    let paid = facts.payment.unwrap();
    assert_eq!((paid.payment_id, paid.canceled_at), (9, None));
    assert_eq!(paid.next_payment_at, Some(utc(2026, 2, 1, 12, 0, 0, 0)), "BIL-063: created plus one month");

    let canceled = subscription_facts(&open, |_| Some(payment(9, Some(NOW)))).unwrap().payment.unwrap();
    assert_eq!(canceled.next_payment_at, None, "no next payment once canceled");
    let foreign = subscription_facts(&open, |_| Some(Payment { account: AccountId(Uuid::from_u128(8)), ..payment(9, None) })).unwrap();
    assert_eq!(foreign.payment, None);
    assert_eq!(subscription_facts(&[package(3, "Pay-as-you-Go", None)], |_| None), None, "no plan, no subscription");
}

fn loaded(canceled_at: Option<Micros>) -> SubscriptionState {
    let (mut state, effects) = SubscriptionState::new();
    assert_eq!(effects, vec![SubscriptionEffect::Load]);
    let facts = subscription_facts(&[package(5, "PREMIUM", Some(9))], |_| Some(payment(9, canceled_at)));
    state.update(SubscriptionEvent::Loaded(Ok(facts)), NOW);
    state
}

// WEB-BIL-022/023, BIL-062: confirm, then cancel over the owner route; the stored canceled
// instant replaces the next payment with the keep-your-minutes message, and telemetry follows.
#[test]
fn cancel_is_confirmed_then_sent_once_and_the_stored_instant_is_shown() {
    let mut state = loaded(None);
    let vm = state.view();
    assert!(vm.show_cancel && !vm.canceled && vm.next_payment.is_some() && !vm.loading);
    assert!(state.update(SubscriptionEvent::CancelConfirmed, NOW).is_empty(), "nothing is sent before the confirmation opens");
    state.update(SubscriptionEvent::CancelClicked, NOW);
    assert!(state.view().confirming);
    assert_eq!(state.update(SubscriptionEvent::CancelConfirmed, NOW), vec![SubscriptionEffect::Cancel { payment_id: 9 }]);
    assert!(state.update(SubscriptionEvent::CancelConfirmed, NOW).is_empty(), "a double click sends once");
    state.update(SubscriptionEvent::CancelDismissed, NOW);
    assert!(state.view().confirming, "the dialog stays while the request runs");

    let effects = state.update(SubscriptionEvent::CancelAnswered(Ok(CancelView { payment_id: 9, canceled_at: NOW })), NOW);
    let days = (NOW.0 - utc(2026, 1, 1, 12, 0, 0, 0).0) / 86_400_000_000;
    assert_eq!(effects, vec![SubscriptionEffect::Analytics(SubscriptionCancelled { plan: "PREMIUM".into(), days_subscribed: Some(days) })]);
    assert_eq!(SubscriptionCancelled::EVENT, "subscription_cancelled");
    let vm = state.view();
    assert!(vm.canceled && !vm.show_cancel && !vm.confirming && vm.next_payment.is_none());
}

// A failed cancel keeps the confirmation open and shows the failure; a plan bought without a
// payment refuses before any request; an already canceled plan offers no cancel.
#[test]
fn failures_and_missing_payments_never_claim_a_cancellation() {
    let mut state = loaded(None);
    state.update(SubscriptionEvent::CancelClicked, NOW);
    state.update(SubscriptionEvent::CancelConfirmed, NOW);
    assert!(state.update(SubscriptionEvent::CancelAnswered(Err(PurchaseFailure::Unavailable)), NOW).is_empty());
    let vm = state.view();
    assert!(vm.confirming && !vm.canceled && !vm.cancelling);
    assert_eq!(vm.failure, Some(SubscriptionFailure::Request(PurchaseFailure::Unavailable)));

    let (mut unpaid, _) = SubscriptionState::new();
    unpaid.update(SubscriptionEvent::Loaded(Ok(subscription_facts(&[package(5, "BUSINESS", None)], |_| None))), NOW);
    unpaid.update(SubscriptionEvent::CancelClicked, NOW);
    assert!(unpaid.update(SubscriptionEvent::CancelConfirmed, NOW).is_empty());
    assert_eq!(unpaid.view().failure, Some(SubscriptionFailure::NoActiveSubscription));

    let mut canceled = loaded(Some(NOW));
    canceled.update(SubscriptionEvent::CancelClicked, NOW);
    assert!(!canceled.view().confirming && !canceled.view().show_cancel);

    let (mut none, _) = SubscriptionState::new();
    none.update(SubscriptionEvent::Loaded(Err(PurchaseFailure::Unavailable)), NOW);
    assert_eq!((none.view().plan, none.view().loading), (None, false));
    assert_eq!(none.update(SubscriptionEvent::UpgradeClicked, NOW), vec![SubscriptionEffect::OpenPurchase]);
}
