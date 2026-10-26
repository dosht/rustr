use super::*;
use crate::grants::PackageSpec;

const NOW: Micros = Micros(1_768_478_400_000_000);

fn d(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

fn card(name: &str, interval: PlanInterval) -> PlanCardView {
    PlanCardView { name: name.into(), interval, usd_per_month: d("10.00"), struck_per_month: d("12.00"), badge: "Save %10".into() }
}

/// Source indices: 0 monthly Premium, 1 monthly Business, 2 yearly Premium, 3 yearly Business.
fn catalog() -> CatalogView {
    CatalogView {
        plans: vec![
            CatalogPlan { package_id: 0, card: card("PREMIUM", PlanInterval::Monthly) },
            CatalogPlan { package_id: 1, card: card("BUSINESS", PlanInterval::Monthly) },
            CatalogPlan { package_id: 2, card: card("PREMIUM", PlanInterval::Yearly) },
            CatalogPlan { package_id: 3, card: card("BUSINESS", PlanInterval::Yearly) },
        ],
        pay_as_you_go: Some(PaygCardView { price_text: "$1.49 per 1 hour".into(), struck_per_hour: d("2.00") }),
    }
}

fn quote(amount: &str, currency: Currency, display: &str, rate: &str) -> QuoteView {
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
            usd_amount: d(amount),
            rate: d(rate),
        },
        display_amount: d(display),
    }
}

fn last_request(effects: &[PurchaseEffect]) -> (u64, OrderBody) {
    effects
        .iter()
        .rev()
        .find_map(|e| match e {
            PurchaseEffect::FetchQuote { request, order } => Some((*request, order.clone())),
            _ => None,
        })
        .expect("a quote request")
}

fn opened(locale: &str) -> (PurchaseState, u64) {
    let (state, effects) = PurchaseState::new(catalog(), locale);
    let (request, _) = last_request(&effects);
    (state, request)
}

// WEB-BIL-001..004: the modal opens on pay as you go with 10 hours on the yearly tab
// and asks the server for its quote at once.
#[test]
fn opening_asks_for_the_default_pay_as_you_go_quote_in_the_locale_currency() {
    let (state, effects) = PurchaseState::new(catalog(), "en");
    let (request, order) = last_request(&effects);
    assert_eq!(request, 1);
    assert_eq!(
        order,
        OrderBody {
            package_id: None,
            pay_as_you_go_hours: Some(10),
            currency: "USD".into(),
            locale: "en".into(),
            coupon_id: None,
            gateway: "iyzico".into(),
            analytics: None
        }
    );
    let vm = state.view();
    assert!(vm.pay_as_you_go_selected && vm.yearly && vm.quote_pending && !vm.can_pay);
    assert_eq!(vm.plans.iter().map(|p| p.package_id).collect::<Vec<_>>(), vec![2, 3], "the yearly tab shows the yearly cards");
    assert_eq!(vm.renewal, RenewalNotice::WillNotRenew);
    let (_, tr) = PurchaseState::new(catalog(), "tr");
    assert_eq!(last_request(&tr).1.currency, "TRY", "CPN-001");
}

// P5 D8, DEF-010: the summary shows display_amount and the charge stays the quote; a late
// answer to an older request never overwrites a newer one.
#[test]
fn the_shown_total_and_the_charged_quote_stay_separate_and_stale_answers_are_dropped() {
    let (mut state, first) = opened("id");
    let second = last_request(&state.update(PurchaseEvent::StepHours(1), NOW)).0;
    state.update(PurchaseEvent::QuoteAnswered { request: second, result: Ok(quote("7.45", Currency::Usd, "7.40", "44")) }, NOW);
    state.update(PurchaseEvent::QuoteAnswered { request: first, result: Ok(quote("99.00", Currency::Usd, "99.00", "44")) }, NOW);
    let vm = state.view();
    assert_eq!(vm.display_total, Some(d("7.40")));
    assert_eq!(vm.charged_quote, Some(ChargedQuote { amount: d("7.45"), currency: Currency::Usd }));
    assert!(!vm.quote_pending);
    assert_eq!(vm.try_display, None, "only the Turkish locale shows a TL line");

    let (mut tr, request) = opened("tr");
    tr.update(PurchaseEvent::QuoteAnswered { request, result: Ok(quote("655.60", Currency::Try, "14.90", "44")) }, NOW);
    assert_eq!(tr.view().try_display, Some(d("655.60")), "14.90 * 44, half up");
}

// SubscriptionSection.tsx: the interval tabs select the same plan kind in that interval,
// the section header restores the last kind, and a card click selects that card.
#[test]
fn plan_selection_follows_the_source_tabs_header_and_cards() {
    let (mut state, _) = opened("en");
    let order = last_request(&state.update(PurchaseEvent::IntervalChosen { yearly: false }, NOW)).1;
    assert_eq!(order.package_id, Some(0), "from pay as you go the monthly tab selects Premium");
    assert_eq!(order.pay_as_you_go_hours, None);
    state.update(PurchaseEvent::SelectPlan { package_id: 1 }, NOW);
    let order = last_request(&state.update(PurchaseEvent::IntervalChosen { yearly: true }, NOW)).1;
    assert_eq!(order.package_id, Some(3), "Business stays Business in the yearly tab");
    let vm = state.view();
    assert_eq!(vm.renewal, RenewalNotice::InDays(365));
    assert!(vm.plans.iter().any(|p| p.package_id == 3 && p.selected));

    state.update(PurchaseEvent::SelectPayAsYouGo, NOW);
    let order = last_request(&state.update(PurchaseEvent::PlanSectionClicked, NOW)).1;
    assert_eq!(order.package_id, Some(3), "the header restores the last kind for the current tab");
    assert!(state.update(PurchaseEvent::PlanSectionClicked, NOW).is_empty(), "a selected plan is kept");
    state.update(PurchaseEvent::SelectPlan { package_id: 99 }, NOW);
    assert_eq!(last_request(&state.update(PurchaseEvent::StepHours(0), NOW)).1.package_id, Some(3), "an unknown card selects nothing");
}

// CPN-005: typed hours under five or not a number reset to five with the warning; steps
// and presets clamp to 5..9999 and clear it.
#[test]
fn hours_follow_the_source_control() {
    let (mut state, _) = opened("en");
    state.update(PurchaseEvent::HoursTyped("3".into()), NOW);
    assert_eq!((state.view().hours, state.view().minimum_warning), (5, true));
    let order = last_request(&state.update(PurchaseEvent::HoursTyped("12.7".into()), NOW)).1;
    assert_eq!((order.pay_as_you_go_hours, state.view().minimum_warning), (Some(12), false));
    state.update(PurchaseEvent::StepHours(-100), NOW);
    assert_eq!(state.view().hours, 5);
    state.update(PurchaseEvent::PresetHours(25), NOW);
    assert_eq!(state.view().hours, 25);
    assert_eq!(state.view().hour_presets, [5, 10, 25]);
}

// CPN-008..013: applying asks the server once per code; an expired or unknown coupon shows
// its notice and is not sent; an applied coupon locks the field and joins every order.
#[test]
fn a_coupon_is_looked_up_once_shown_by_code_and_locked_when_applied() {
    let (mut state, _) = opened("en");
    state.update(PurchaseEvent::CouponTyped("OLD".into()), NOW);
    assert_eq!(state.update(PurchaseEvent::CouponApplied, NOW), vec![PurchaseEffect::FetchCoupon("OLD".into())]);
    assert!(state.update(PurchaseEvent::CouponApplied, NOW).is_empty(), "the same code is not fetched twice");
    let order = last_request(&state.update(PurchaseEvent::CouponAnswered(CouponLookup::Expired), NOW)).1;
    assert_eq!((state.view().coupon_notice, order.coupon_id), (Some(CouponMessage::Expired), None));

    state.update(PurchaseEvent::CouponTyped("HALF".into()), NOW);
    state.update(PurchaseEvent::CouponApplied, NOW);
    let applied = crate::pricing::Coupon { id: "HALF".into(), amount: Some(d("0.5")), expires_at: None, filter_product: Some("Pay-as-you-Go".into()), discount_type: None };
    let order = last_request(&state.update(PurchaseEvent::CouponAnswered(CouponLookup::Found(applied)), NOW)).1;
    assert_eq!(order.coupon_id.as_deref(), Some("HALF"));
    let vm = state.view();
    assert!(vm.coupon_locked && vm.coupon_notice.is_none());
    state.update(PurchaseEvent::CouponTyped("OTHER".into()), NOW);
    assert_eq!(state.view().coupon_code, "HALF", "a locked field ignores typing");

    state.update(PurchaseEvent::CouponAnswered(CouponLookup::Failed), NOW);
    assert_eq!(state.view().coupon_notice, Some(CouponMessage::Error));
}

// The coupon route's body maps to a lookup answer in shared Rust, never in a shell.
#[test]
fn the_coupon_reply_carries_the_servers_decision() {
    let reply = |json: serde_json::Value| serde_json::from_value::<CouponReply>(json).unwrap().lookup();
    assert!(matches!(
        reply(serde_json::json!({"valid": true, "message": null, "notice": null, "coupon": {"id": "HALF", "amount": "0.5", "product": "Pay-as-you-Go"}})),
        CouponLookup::Found(c) if c.id == "HALF" && c.filter_product.as_deref() == Some("Pay-as-you-Go")
    ));
    assert_eq!(reply(serde_json::json!({"valid": false, "message": "Coupon has expired", "notice": "Expired", "coupon": null})), CouponLookup::Expired);
    assert_eq!(reply(serde_json::json!({"valid": false, "message": "Coupon is not valid", "notice": "NotValid", "coupon": null})), CouponLookup::NotFound);
    assert_eq!(reply(serde_json::json!({"valid": false, "message": "Error", "notice": "Error", "coupon": null})), CouponLookup::Failed);
}

// WEB-BIL-010..014, D17: paying needs accepted terms and a current quote; it emits the
// analytics intent then one form submission, and nothing else changes while the page leaves.
#[test]
fn paying_needs_terms_and_a_quote_and_submits_the_checkout_form_once() {
    let (mut state, request) = opened("en");
    state.update(PurchaseEvent::QuoteAnswered { request, result: Ok(quote("14.90", Currency::Usd, "14.90", "44")) }, NOW);
    assert!(state.update(PurchaseEvent::PayRequested, NOW).is_empty(), "terms not accepted");
    state.update(PurchaseEvent::TermsAccepted(true), NOW);
    assert!(state.view().can_pay);
    let effects = state.update(PurchaseEvent::PayRequested, NOW);
    assert_eq!(
        effects[0],
        PurchaseEffect::Analytics(CheckoutStarted { plan: "pay-as-you-go".into(), amount: d("14.90"), currency: Currency::Usd, billing_cycle: None })
    );
    if let PurchaseEffect::Analytics(started) = &effects[0] {
        assert_eq!(CheckoutStarted::EVENT, "checkout_started");
        assert_eq!(started.properties(), serde_json::json!({"plan": "pay-as-you-go", "amount": 14.9, "currency": "USD", "billing_cycle": null}));
    }
    assert!(matches!(&effects[1], PurchaseEffect::SubmitCheckout(order) if order.pay_as_you_go_hours == Some(10) && order.analytics.is_none()));
    assert_eq!(effects.len(), 2);
    assert!(state.view().processing && !state.view().can_pay);
    assert!(state.update(PurchaseEvent::PayRequested, NOW).is_empty(), "a second click submits nothing");
    assert!(state.update(PurchaseEvent::StepHours(1), NOW).is_empty(), "nothing changes while the page leaves");

    // A submission the shell could not make returns control with its failure.
    state.update(PurchaseEvent::CheckoutNotSubmitted(PurchaseFailure::Unauthenticated), NOW);
    let vm = state.view();
    assert_eq!((vm.processing, vm.failure), (false, Some(PurchaseFailure::Unauthenticated)));
    assert!(vm.can_pay, "the buyer may try again");
}
