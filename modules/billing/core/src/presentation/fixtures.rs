//! Named billing screen states for previews, Storybook and snapshot tests. Every
//! state comes from real transitions of its state machine; only the server
//! answers fed to it are written out. Data is synthetic. Transgate's fixture
//! export folds each list in under its view model type name.

use serde_json::json;
use uuid::Uuid;

use crate::app::CancelView;
use crate::calendar::utc;
use crate::decimal::Decimal;
use crate::grants::{PackageSpec, PaymentStatus};
use crate::model::Payment;
use crate::ports::Customer;
use crate::presentation::confirm::{ConfirmAnalytic, ConfirmEvent, ConfirmPageState, ConfirmPayload, ConfirmVm, ConfirmedPayment};
use crate::presentation::invoice::{InvoiceContext, invoice_context};
use crate::presentation::invoices::{AdminLookupReply, InvoicesEvent, InvoicesState, InvoicesVm, LookedUpCustomer};
use crate::presentation::purchase::{
    CatalogPlan, CatalogView, PaygCardView, PlanCardView, PurchaseEffect, PurchaseEvent, PurchaseFailure, PurchaseState, PurchaseVm,
    QuoteView,
};
use crate::presentation::subscription::{SubscriptionEvent, SubscriptionFacts, SubscriptionPayment, SubscriptionState, SubscriptionVm};
use crate::pricing::{CouponLookup, Currency, PlanInterval, Quote};
use crate::types::{AccountId, Micros};

pub const NOW: Micros = Micros(1_768_478_400_000_000);

fn d(s: &str) -> Decimal {
    Decimal::parse(s).expect("a fixture decimal")
}

fn card(name: &str, interval: PlanInterval, usd_per_month: &str, struck_per_month: &str) -> PlanCardView {
    let badge = if interval == PlanInterval::Yearly { "Save %10" } else { "" };
    PlanCardView { name: name.into(), interval, usd_per_month: d(usd_per_month), struck_per_month: d(struck_per_month), badge: badge.into() }
}

pub fn catalog() -> CatalogView {
    CatalogView {
        plans: vec![
            CatalogPlan { package_id: 0, card: card("PREMIUM", PlanInterval::Monthly, "10.00", "12.00") },
            CatalogPlan { package_id: 1, card: card("BUSINESS", PlanInterval::Monthly, "25.00", "30.00") },
            CatalogPlan { package_id: 2, card: card("PREMIUM", PlanInterval::Yearly, "9.00", "12.00") },
            CatalogPlan { package_id: 3, card: card("BUSINESS", PlanInterval::Yearly, "22.50", "30.00") },
        ],
        pay_as_you_go: Some(PaygCardView { price_text: "$1.49 per 1 hour".into(), struck_per_hour: d("2.00") }),
    }
}

fn payg_quote(amount: &str, currency: Currency, display: &str) -> QuoteView {
    QuoteView {
        quote: Quote {
            product_name: "Pay-as-you-Go".into(),
            amount: d(amount),
            currency,
            coupon_id: None,
            discount_applied: false,
            pricing_plan_id: None,
            spec: PackageSpec { name: "Pay-as-you-Go".into(), minutes: 600, count: 1, interval: "year".into(), frequency: 1, expiry: 1 },
            hours: Some(10),
            usd_amount: d("14.90"),
            rate: d("44"),
        },
        display_amount: d(display),
    }
}

fn plan_quote() -> QuoteView {
    QuoteView {
        quote: Quote {
            product_name: "PREMIUM".into(),
            amount: d("108.00"),
            currency: Currency::Usd,
            coupon_id: None,
            discount_applied: false,
            pricing_plan_id: Some("plan-premium-yearly".into()),
            spec: PackageSpec { name: "PREMIUM".into(), minutes: 1200, count: 1, interval: "year".into(), frequency: 1, expiry: 1 },
            hours: None,
            usd_amount: d("108.00"),
            rate: d("44"),
        },
        display_amount: d("108.00"),
    }
}

fn answer(state: &mut PurchaseState, effects: Vec<PurchaseEffect>, quote: QuoteView) {
    let request = effects
        .iter()
        .rev()
        .find_map(|e| match e {
            PurchaseEffect::FetchQuote { request, .. } => Some(*request),
            _ => None,
        })
        .expect("a quote request");
    state.update(PurchaseEvent::QuoteAnswered { request, result: Ok(quote) }, NOW);
}

fn opened(locale: &str, quote: QuoteView) -> PurchaseState {
    let (mut state, effects) = PurchaseState::new(catalog(), locale);
    answer(&mut state, effects, quote);
    state
}

fn pay_as_you_go() -> PurchaseState {
    opened("en", payg_quote("14.90", Currency::Usd, "14.90"))
}

pub fn purchase() -> Vec<(&'static str, PurchaseVm)> {
    let (quote_pending, _) = PurchaseState::new(catalog(), "en");

    let mut plan = pay_as_you_go();
    let effects = plan.update(PurchaseEvent::SelectPlan { package_id: 2 }, NOW);
    answer(&mut plan, effects, plan_quote());

    let mut coupon_expired = pay_as_you_go();
    coupon_expired.update(PurchaseEvent::CouponTyped("SPRING".into()), NOW);
    coupon_expired.update(PurchaseEvent::CouponApplied, NOW);
    let effects = coupon_expired.update(PurchaseEvent::CouponAnswered(CouponLookup::Expired), NOW);
    answer(&mut coupon_expired, effects, payg_quote("14.90", Currency::Usd, "14.90"));

    let mut ready = pay_as_you_go();
    ready.update(PurchaseEvent::TermsAccepted(true), NOW);
    let mut processing = pay_as_you_go();
    processing.update(PurchaseEvent::TermsAccepted(true), NOW);
    processing.update(PurchaseEvent::PayRequested, NOW);
    let mut failed = pay_as_you_go();
    failed.update(PurchaseEvent::TermsAccepted(true), NOW);
    failed.update(PurchaseEvent::PayRequested, NOW);
    failed.update(PurchaseEvent::CheckoutNotSubmitted(PurchaseFailure::Unavailable), NOW);

    vec![
        ("quote_pending", quote_pending.view()),
        ("pay_as_you_go", pay_as_you_go().view()),
        ("display_differs_from_charge", opened("id", payg_quote("7.45", Currency::Usd, "7.40")).view()),
        ("turkish_lira", opened("tr", payg_quote("655.60", Currency::Try, "655.60")).view()),
        ("yearly_plan", plan.view()),
        ("coupon_expired", coupon_expired.view()),
        ("ready_to_pay", ready.view()),
        ("processing", processing.view()),
        ("failed", failed.view()),
    ]
}

fn subscription_facts() -> SubscriptionFacts {
    SubscriptionFacts {
        package_name: "PREMIUM".into(),
        minutes_included: 1200,
        valid_until: utc(2026, 2, 1, 12, 0, 0, 0),
        payment: Some(SubscriptionPayment {
            payment_id: 9,
            created: utc(2026, 1, 1, 12, 0, 0, 0),
            canceled_at: None,
            next_payment_at: Some(utc(2026, 2, 1, 12, 0, 0, 0)),
        }),
    }
}

fn subscribed() -> SubscriptionState {
    let (mut state, _) = SubscriptionState::new();
    state.update(SubscriptionEvent::Loaded(Ok(Some(subscription_facts()))), NOW);
    state
}

pub fn subscription() -> Vec<(&'static str, SubscriptionVm)> {
    let (loading, _) = SubscriptionState::new();
    let (mut no_plan, _) = SubscriptionState::new();
    no_plan.update(SubscriptionEvent::Loaded(Ok(None)), NOW);
    let mut confirming = subscribed();
    confirming.update(SubscriptionEvent::CancelClicked, NOW);
    let mut cancelling = subscribed();
    cancelling.update(SubscriptionEvent::CancelClicked, NOW);
    cancelling.update(SubscriptionEvent::CancelConfirmed, NOW);
    let mut canceled = subscribed();
    canceled.update(SubscriptionEvent::CancelClicked, NOW);
    canceled.update(SubscriptionEvent::CancelConfirmed, NOW);
    canceled.update(SubscriptionEvent::CancelAnswered(Ok(CancelView { payment_id: 9, canceled_at: NOW })), NOW);
    let mut failed = subscribed();
    failed.update(SubscriptionEvent::CancelClicked, NOW);
    failed.update(SubscriptionEvent::CancelConfirmed, NOW);
    failed.update(SubscriptionEvent::CancelAnswered(Err(PurchaseFailure::Unavailable)), NOW);

    vec![
        ("loading", loading.view()),
        ("active", subscribed().view()),
        ("no_plan", no_plan.view()),
        ("confirming", confirming.view()),
        ("cancelling", cancelling.view()),
        ("canceled", canceled.view()),
        ("cancel_failed", failed.view()),
    ]
}

fn payment(id: i64, amount: &str, currency: &str, package: &str, minutes: i64, created: Micros) -> Payment {
    Payment {
        id,
        account: AccountId(Uuid::from_u128(7)),
        amount: d(amount),
        currency: currency.into(),
        gateway: "iyzico".into(),
        status: PaymentStatus::Completed,
        token: None,
        created,
        environment: "live".into(),
        metadata: json!({"package": {"name": package, "minutes": minutes}}),
        coupon_id: None,
        pricing_plan_id: None,
        customer_id: None,
        subscription_ref: None,
        provider_order_ref: None,
        subscription_canceled_at: None,
        observed: None,
        receipt_url: None,
    }
}

fn customer(email: &str, full_name: &str) -> Customer {
    Customer { account: AccountId(Uuid::from_u128(7)), email: email.into(), full_name: Some(full_name.into()), created_at: Micros(0) }
}

/// Newest first, as the owner invoice route answers.
pub fn invoice_context_sample() -> InvoiceContext {
    invoice_context(
        &customer("ada@example.test", "Ada Customer"),
        None,
        &[
            payment(42, "120.00", "TRY", "PREMIUM", 1200, utc(2026, 1, 1, 12, 0, 0, 0)),
            payment(7, "14.90", "USD", "Pay-as-you-Go", 600, utc(2025, 11, 5, 9, 30, 0, 0)),
        ],
    )
}

fn invoices_loaded(is_admin: bool) -> InvoicesState {
    let (mut state, _) = InvoicesState::new(is_admin);
    state.update(InvoicesEvent::Loaded(Ok(invoice_context_sample())));
    state
}

fn looked_up(answer: Result<AdminLookupReply, PurchaseFailure>) -> InvoicesState {
    let mut state = invoices_loaded(true);
    state.update(InvoicesEvent::LookupEmailTyped("grace@example.test".into()));
    state.update(InvoicesEvent::LookupRequested);
    state.update(InvoicesEvent::LookupAnswered(answer));
    state
}

pub fn invoices() -> Vec<(&'static str, InvoicesVm)> {
    let (loading, _) = InvoicesState::new(false);
    let (mut empty, _) = InvoicesState::new(false);
    empty.update(InvoicesEvent::Loaded(Ok(invoice_context(&customer("ada@example.test", "Ada Customer"), None, &[]))));
    let (mut failed, _) = InvoicesState::new(false);
    failed.update(InvoicesEvent::Loaded(Err(PurchaseFailure::Unavailable)));
    let mut preview = invoices_loaded(false);
    preview.update(InvoicesEvent::Preview { payment_id: 42 });
    let mut document_failed = invoices_loaded(false);
    document_failed.update(InvoicesEvent::Preview { payment_id: 42 });
    document_failed.update(InvoicesEvent::DocumentFailed { payment_id: 42, failure: PurchaseFailure::Unavailable });

    let mut admin_looking_up = invoices_loaded(true);
    admin_looking_up.update(InvoicesEvent::LookupEmailTyped("grace@example.test".into()));
    admin_looking_up.update(InvoicesEvent::LookupRequested);
    let mut admin_customer = looked_up(Ok(AdminLookupReply {
        customer: LookedUpCustomer { email: "grace@example.test".into() },
        invoice_context: invoice_context(
            &customer("grace@example.test", "Grace Customer"),
            None,
            &[payment(51, "25.00", "USD", "BUSINESS", 3000, utc(2026, 1, 10, 8, 0, 0, 0))],
        ),
    }));
    admin_customer.update(InvoicesEvent::TemporaryBillToTyped("Grace Ltd\nLondon".into()));

    vec![
        ("loading", loading.view()),
        ("owner", invoices_loaded(false).view()),
        ("empty", empty.view()),
        ("failed", failed.view()),
        ("preview", preview.view()),
        ("document_failed", document_failed.view()),
        ("admin", invoices_loaded(true).view()),
        ("admin_looking_up", admin_looking_up.view()),
        ("admin_customer", admin_customer.view()),
        ("admin_not_found", looked_up(Err(PurchaseFailure::Refused { code: "not_found".into() })).view()),
    ]
}

fn confirmed(status: PaymentStatus, success: bool, analytics: Vec<ConfirmAnalytic>) -> ConfirmPageState {
    let view = ConfirmedPayment { payment_id: 7, status, success, changed: true, analytics, redirect_after_seconds: 5 };
    ConfirmPageState::new(Some(ConfirmPayload::Confirmed { view })).0
}

pub fn confirm() -> Vec<(&'static str, ConfirmVm)> {
    let succeeded = || {
        confirmed(
            PaymentStatus::Completed,
            true,
            vec![ConfirmAnalytic::PaymentCompleted, ConfirmAnalytic::Purchase { value_usd: d("14.90"), currency: "USD".into() }],
        )
    };
    let mut counting_down = succeeded();
    counting_down.update(ConfirmEvent::Tick);
    counting_down.update(ConfirmEvent::Tick);
    let uncertain =
        ConfirmPayload::Failed { status: 504, code: "provider_uncertain".into(), message: "The payment provider did not answer.".into() };

    vec![
        ("succeeded", succeeded().view()),
        ("counting_down", counting_down.view()),
        ("declined", confirmed(PaymentStatus::Failed, false, vec![ConfirmAnalytic::PaymentFailed]).view()),
        ("pending", confirmed(PaymentStatus::Pending, false, vec![]).view()),
        ("unconfirmed", ConfirmPageState::new(Some(uncertain)).0.view()),
        ("no_confirmation", ConfirmPageState::new(None).0.view()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::confirm::ConfirmKind;

    // Each named state shows what its name says.
    #[test]
    fn named_billing_screens_render() {
        let purchase = purchase();
        let get = |name: &str| purchase.iter().find(|(n, _)| *n == name).map(|(_, vm)| vm.clone()).unwrap();
        assert!(get("quote_pending").quote_pending);
        let shown = get("display_differs_from_charge");
        assert_eq!((shown.display_total, shown.charged_quote.map(|q| q.amount)), (Some(d("7.40")), Some(d("7.45"))));
        assert!(get("yearly_plan").plans.iter().any(|p| p.selected && p.package_id == 2));
        assert!(get("coupon_expired").coupon_notice.is_some());
        assert!(get("ready_to_pay").can_pay && get("processing").processing);
        assert_eq!(get("failed").failure, Some(PurchaseFailure::Unavailable));

        let subscription = subscription();
        assert!(subscription.iter().any(|(n, vm)| *n == "canceled" && vm.canceled));
        assert!(subscription.iter().any(|(n, vm)| *n == "cancel_failed" && vm.failure.is_some()));

        let invoices = invoices();
        let owner = &invoices.iter().find(|(n, _)| *n == "owner").unwrap().1;
        assert_eq!(owner.rows.iter().map(|r| r.amount_text.as_str()).collect::<Vec<_>>(), vec!["120.00 TRY", "14.90 USD"]);
        assert!(owner.admin.is_none());
        let customer = invoices.iter().find(|(n, _)| *n == "admin_customer").unwrap().1.admin.clone().unwrap();
        assert_eq!(customer.viewing_customer.as_deref(), Some("grace@example.test"));
        assert!(invoices.iter().any(|(n, vm)| *n == "admin_not_found" && vm.admin.as_ref().is_some_and(|a| a.not_found)));

        let kinds: Vec<_> = confirm().into_iter().map(|(n, vm)| (n, vm.kind, vm.countdown)).collect();
        assert!(kinds.contains(&("counting_down", ConfirmKind::Succeeded, Some(3))));
        assert!(kinds.contains(&("unconfirmed", ConfirmKind::Unconfirmed, None)));
        assert!(kinds.contains(&("pending", ConfirmKind::Unconfirmed, None)));
    }
}
