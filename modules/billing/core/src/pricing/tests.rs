use super::*;
use crate::calendar::utc;

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn spec(name: &str, minutes: i64, count: u32, interval: &str, expiry: u32) -> PackageSpec {
    PackageSpec { name: name.into(), minutes, count, interval: interval.into(), frequency: 1, expiry }
}

fn catalog() -> Catalog {
    Catalog {
        plans: vec![
            PlanEntry {
                name: "PREMIUM".into(),
                interval: PlanInterval::Monthly,
                spec: spec("PREMIUM", 1200, 1, "month", 1),
                usd_price: d("14"),
                struck_price: d("40"),
                plan_code_try: Some("try-premium-monthly".into()),
                plan_code_usd: Some("usd-premium-monthly".into()),
            },
            PlanEntry {
                name: "PREMIUM".into(),
                interval: PlanInterval::Yearly,
                spec: spec("PREMIUM", 1200, 12, "month", 1),
                usd_price: d("120"),
                struck_price: d("30"),
                plan_code_try: Some("try-premium-yearly".into()),
                plan_code_usd: Some("usd-premium-yearly".into()),
            },
            PlanEntry {
                name: "BUSINESS".into(),
                interval: PlanInterval::Yearly,
                spec: spec("BUSINESS", 2400, 12, "month", 1),
                usd_price: d("180"),
                struck_price: d("55"),
                plan_code_try: None,
                plan_code_usd: Some("usd-business-yearly".into()),
            },
        ],
        payg: Some(PaygEntry {
            stored_name: "Pay-as-you-Go".into(),
            catalog_label: "PAY AS YOU GO".into(),
            usd_per_hour: d("1.49"),
            struck_per_hour: d("2"),
            min_hours: 5,
            max_hours: 9999,
            minutes_per_hour: 60,
        }),
        locale_discount_locales: ["id".to_string(), "pt".to_string()].into_iter().collect(),
        special_offer_try_per_hour: Some(d("14.99")),
    }
}

fn coupon(id: &str, amount: Option<&str>, product: Option<&str>) -> Coupon {
    Coupon {
        id: id.into(),
        amount: amount.map(d),
        expires_at: Some(utc(2030, 1, 1, 0, 0, 0, 0)),
        filter_product: product.map(str::to_string),
        discount_type: Some("percentage_discount".into()),
    }
}

fn now() -> Micros {
    utc(2026, 10, 24, 0, 0, 0, 0)
}

fn payg(hours: i64, currency: Currency, locale: &str, coupon: Option<&Coupon>, special: bool) -> Result<Quote, OrderError> {
    price_order(
        &catalog(),
        &OrderInput {
            product: Product::PayAsYouGo { hours },
            currency,
            ui_locale: locale,
            coupon,
            special_offer: special,
            rate: &d("44"),
            now: now(),
        },
    )
}

fn amount(q: Result<Quote, OrderError>) -> String {
    q.unwrap().amount.to_string()
}

#[test]
fn cpn_001_currency_follows_the_locale_and_only_usd_and_try_exist() {
    assert_eq!(currency_for_locale("tr"), Currency::Try);
    assert_eq!(currency_for_locale("en"), Currency::Usd);
    assert_eq!(currency_for_locale("tr-TR"), Currency::Usd, "exactly tr, nothing broader");
    assert_eq!(Currency::parse("try"), Some(Currency::Try), "the request currency is upper-cased");
    assert_eq!(Currency::parse("usd"), Some(Currency::Usd));
    assert_eq!(Currency::parse("EUR"), None);
}

#[test]
fn cpn_002_the_rate_falls_back_to_44_and_try_is_rounded_once_not_left_as_a_float_tail() {
    assert_eq!(parse_rate(Some("44.5")), d("44.5"));
    assert_eq!(parse_rate(Some("0")), d("44"));
    assert_eq!(parse_rate(Some("abc")), d("44"));
    assert_eq!(parse_rate(Some("")), d("44"));
    assert_eq!(parse_rate(None), d("44"));
    // Legacy printed 393.35999999999996 for six hours in TRY.
    assert_eq!(amount(payg(6, Currency::Try, "tr", None, false)), "393.36");
}

#[test]
fn cpn_003_try_plan_quote_is_usd_price_times_rate_with_the_plan_code_for_the_currency() {
    let order = |index, currency| {
        price_order(
            &catalog(),
            &OrderInput {
                product: Product::Plan { index },
                currency,
                ui_locale: "tr",
                coupon: None,
                special_offer: false,
                rate: &d("44"),
                now: now(),
            },
        )
    };
    let monthly = order(0, Currency::Try).unwrap();
    assert_eq!(monthly.amount.to_string(), "616.00");
    assert_eq!(monthly.pricing_plan_id.as_deref(), Some("try-premium-monthly"));
    assert_eq!(order(1, Currency::Try).unwrap().amount.to_string(), "5280.00");
    let usd = order(0, Currency::Usd).unwrap();
    assert_eq!(usd.amount.to_string(), "14.00");
    assert_eq!(usd.pricing_plan_id.as_deref(), Some("usd-premium-monthly"));
    // A plan without a code for the requested currency cannot be bought in it.
    assert_eq!(order(2, Currency::Try).unwrap_err(), OrderError::NoPlanCode);
    assert_eq!(order(9, Currency::Usd).unwrap_err(), OrderError::UnknownPlan(9));
}

#[test]
fn cpn_004_pay_as_you_go_is_149_cents_an_hour_exactly() {
    assert_eq!(amount(payg(10, Currency::Usd, "en", None, false)), "14.90");
    assert_eq!(amount(payg(5, Currency::Usd, "en", None, false)), "7.45");
    assert_eq!(amount(payg(9999, Currency::Usd, "en", None, false)), "14898.51");
}

#[test]
fn def_009_the_server_enforces_whole_hours_five_to_9999() {
    for hours in [0, 4, -3, 10_000] {
        assert!(matches!(payg(hours, Currency::Usd, "en", None, false), Err(OrderError::HoursOutOfRange { .. })), "{hours}");
    }
    assert!(payg(5, Currency::Usd, "en", None, false).is_ok());
    assert!(payg(9999, Currency::Usd, "en", None, false).is_ok());
}

#[test]
fn cpn_005_the_hours_control_clamps_steps_and_warns() {
    assert_eq!(normalize_hours("12"), HoursInput { hours: 12, minimum_warning: false });
    assert_eq!(normalize_hours("3"), HoursInput { hours: 5, minimum_warning: true });
    assert_eq!(normalize_hours(""), HoursInput { hours: 5, minimum_warning: true });
    assert_eq!(normalize_hours("abc"), HoursInput { hours: 5, minimum_warning: true });
    assert_eq!(normalize_hours("20000"), HoursInput { hours: 9999, minimum_warning: false });
    assert_eq!(normalize_hours("7.9").hours, 7, "whole hours only");
    assert_eq!(step_hours(5, -1), 5, "the down arrow stops at the minimum");
    assert_eq!(step_hours(9999, 1), 9999);
    assert_eq!(step_hours(10, 1), 11);
    assert_eq!(HOUR_PRESETS, [5, 10, 25]);
    assert_eq!(DEFAULT_HOURS, 10);
}

#[test]
fn cpn_006_the_locale_discount_halves_the_whole_total_and_the_page_halves_the_hourly_price() {
    // Charged: floor of half the whole total. Shown: half the hourly price times hours (DEF-010).
    assert_eq!(amount(payg(10, Currency::Usd, "id", None, false)), "7.45");
    assert_eq!(payg_display_total(&catalog(), 10, "id").unwrap().to_string(), "7.40");
    assert_eq!(amount(payg(5, Currency::Usd, "pt", None, false)), "3.72");
    assert_eq!(payg_display_total(&catalog(), 5, "pt").unwrap().to_string(), "3.70");
    assert_eq!(amount(payg(100, Currency::Usd, "id", None, false)), "74.50");
    assert_eq!(payg_display_total(&catalog(), 100, "id").unwrap().to_string(), "74.00");
    // Other locales show what they are charged.
    assert_eq!(payg_display_total(&catalog(), 10, "en").unwrap().to_string(), "14.90");
}

#[test]
fn cpn_007_the_special_offer_is_try_per_hour_and_ignores_coupon_locale_and_rate() {
    let c = coupon("SAVE", Some("0.5"), Some("Pay-as-you-Go"));
    let quote = payg(10, Currency::Usd, "id", Some(&c), true).unwrap();
    assert_eq!(quote.currency, Currency::Try);
    assert_eq!(quote.amount.to_string(), "149.90");
    assert_eq!(quote.coupon_id.as_deref(), Some("SAVE"), "the id is kept as the source does, though the offer price ignores it");
    assert!(!quote.discount_applied);
    // Plans are not affected by the offer.
    let plan = price_order(
        &catalog(),
        &OrderInput {
            product: Product::Plan { index: 0 },
            currency: Currency::Usd,
            ui_locale: "en",
            coupon: None,
            special_offer: true,
            rate: &d("44"),
            now: now(),
        },
    )
    .unwrap();
    assert_eq!(plan.amount.to_string(), "14.00");
    assert_eq!(plan.currency, Currency::Usd);
}

#[test]
fn cpn_008_009_010_a_coupon_is_a_percentage_for_the_exactly_named_product_only() {
    let matching = coupon("QUARTER", Some("0.25"), Some("Pay-as-you-Go"));
    let quote = payg(10, Currency::Usd, "en", Some(&matching), false).unwrap();
    assert_eq!(quote.amount.to_string(), "11.18", "14.90 * 0.75 = 11.175 rounds half up");
    assert!(quote.discount_applied);
    // discount_type is stored but never read: a fixed type still acts as a percentage.
    let mut fixed = matching.clone();
    fixed.discount_type = Some("fixed_discount".into());
    assert_eq!(amount(payg(10, Currency::Usd, "en", Some(&fixed), false)), "11.18");
    // Product name must match exactly, case included.
    let wrong_case = coupon("X", Some("0.25"), Some("pay-as-you-go"));
    assert_eq!(amount(payg(10, Currency::Usd, "en", Some(&wrong_case), false)), "14.90");
    // No filter means the coupon applies to nothing (plans pass no name either).
    let no_filter = coupon("X", Some("0.25"), None);
    assert_eq!(amount(payg(10, Currency::Usd, "en", Some(&no_filter), false)), "14.90");
    // A null amount counts as zero.
    let null_amount = coupon("X", None, Some("Pay-as-you-Go"));
    assert_eq!(amount(payg(10, Currency::Usd, "en", Some(&null_amount), false)), "14.90");
}

#[test]
fn cpn_011_a_coupon_expires_strictly_after_its_instant_and_null_never_expires() {
    let mut c = coupon("X", Some("0.1"), Some("Pay-as-you-Go"));
    c.expires_at = Some(now());
    assert!(!c.is_expired(now()), "the instant itself is still valid");
    assert!(c.is_expired(Micros(now().0 + 1)));
    c.expires_at = None;
    assert!(!c.is_expired(utc(2999, 1, 1, 0, 0, 0, 0)));
}

#[test]
fn cpn_013_the_server_keeps_an_existing_unexpired_coupon_id_even_when_it_did_not_apply() {
    let other_product = coupon("OTHER", Some("0.5"), Some("PREMIUM"));
    let quote = payg(10, Currency::Usd, "en", Some(&other_product), false).unwrap();
    assert_eq!(quote.amount.to_string(), "14.90", "the full price is charged");
    assert_eq!(quote.coupon_id.as_deref(), Some("OTHER"), "but the id is still saved");
    assert!(!quote.discount_applied);
    // A missing or expired coupon yields no coupon id.
    assert_eq!(payg(10, Currency::Usd, "en", None, false).unwrap().coupon_id, None);
    let mut expired = coupon("OLD", Some("0.5"), Some("Pay-as-you-Go"));
    expired.expires_at = Some(utc(2020, 1, 1, 0, 0, 0, 0));
    let quote = payg(10, Currency::Usd, "en", Some(&expired), false).unwrap();
    assert_eq!((quote.coupon_id, quote.amount.to_string()), (None, "14.90".to_string()));
    // A plan never carries a coupon.
    let plan = price_order(
        &catalog(),
        &OrderInput {
            product: Product::Plan { index: 0 },
            currency: Currency::Usd,
            ui_locale: "en",
            coupon: Some(&coupon("Q", Some("0.5"), Some("PREMIUM"))),
            special_offer: false,
            rate: &d("44"),
            now: now(),
        },
    )
    .unwrap();
    assert_eq!((plan.coupon_id, plan.amount.to_string()), (None, "14.00".to_string()));
}

#[test]
fn cpn_013_a_valid_coupon_cuts_the_usd_amount_before_the_try_conversion() {
    let c = coupon("HALF", Some("0.5"), Some("Pay-as-you-Go"));
    // 14.90 * 0.5 = 7.45 USD, times 44 = 327.80 TRY.
    assert_eq!(amount(payg(10, Currency::Try, "tr", Some(&c), false)), "327.80");
}

#[test]
fn d2_money_evidence_examples_use_exact_arithmetic_and_legacy_differences_are_classified() {
    let quarter = coupon("Q", Some("0.25"), Some("Pay-as-you-Go"));
    let third = coupon("T", Some("0.33"), Some("Pay-as-you-Go"));
    // Legacy float result versus the exact result, per documented example.
    struct Case {
        hours: i64,
        locale: &'static str,
        coupon: Option<Coupon>,
        currency: Currency,
        legacy: &'static str,
        exact: &'static str,
    }
    let cases = [
        Case { hours: 33, locale: "id", coupon: Some(quarter.clone()), currency: Currency::Usd, legacy: "18.43", exact: "18.44" },
        Case { hours: 34, locale: "en", coupon: Some(quarter), currency: Currency::Usd, legacy: "37.99", exact: "38.00" },
        Case { hours: 50, locale: "en", coupon: Some(third), currency: Currency::Usd, legacy: "49.91", exact: "49.92" },
        Case { hours: 54, locale: "id", coupon: None, currency: Currency::Usd, legacy: "40.22", exact: "40.23" },
        Case { hours: 54, locale: "id", coupon: None, currency: Currency::Try, legacy: "1769.68", exact: "1770.12" },
    ];
    for case in cases {
        let quote = payg(case.hours, case.currency, case.locale, case.coupon.as_ref(), false).unwrap();
        let exact = quote.amount.quantize(2, Rounding::HalfUp).unwrap().to_string();
        assert_eq!(exact, case.exact, "{} h {}", case.hours, case.locale);
        assert_ne!(exact, case.legacy, "this is a documented, intended price change");
    }
}

#[test]
fn d2_a_fractional_configured_rate_rounds_exactly_once_half_up() {
    let quote = price_order(
        &catalog(),
        &OrderInput {
            product: Product::PayAsYouGo { hours: 5 },
            currency: Currency::Try,
            ui_locale: "tr",
            coupon: None,
            special_offer: false,
            rate: &d("44.37"),
            now: now(),
        },
    )
    .unwrap();
    // 7.45 * 44.37 = 330.5565 -> 330.56
    assert_eq!(quote.amount.to_string(), "330.56");
}

#[test]
fn cpn_015_016_plan_cards_show_monthly_usd_struck_prices_and_the_fixed_badge() {
    let cat = catalog();
    let monthly = plan_card(&cat.plans[0]);
    assert_eq!(monthly.usd_per_month.to_string(), "14");
    assert_eq!(monthly.struck_per_month.to_string(), "40");
    let yearly = plan_card(&cat.plans[1]);
    assert_eq!(yearly.usd_per_month.to_string(), "10.00", "120 / 12 with two decimals");
    assert_eq!(yearly.struck_per_month.to_string(), "30");
    assert_eq!(plan_card(&cat.plans[2]).usd_per_month.to_string(), "15.00");
    assert_eq!(yearly.badge, "Save %10", "a fixed string, kept as it is (DEF-022)");
    assert_eq!(payg_card(&cat).unwrap().price_text, "$1.49 per 1 hour");
    assert_eq!(payg_card(&cat).unwrap().struck_per_hour.to_string(), "2");
    assert_eq!(try_total_text(&d("14.90"), &d("44")), "= TL 655.60");
}

#[test]
fn cpn_017_analytics_reports_try_payments_in_usd_with_the_same_rate() {
    assert_eq!(analytics_usd(&d("1770.12"), Currency::Try, &d("44")).to_string(), "40.23");
    assert_eq!(analytics_usd(&d("40.23"), Currency::Usd, &d("44")).to_string(), "40.23");
}

#[test]
fn cpn_018_try_invoices_treat_the_amount_as_vat_inclusive_at_20_percent() {
    let tax = invoice_tax(&d("120.00"), Currency::Try);
    assert_eq!(tax.subtotal.to_string(), "100.00");
    assert_eq!(tax.vat.to_string(), "20.00");
    assert_eq!(tax.label, Some("VAT (20%)"));
    let odd = invoice_tax(&d("616.00"), Currency::Try);
    assert_eq!(odd.subtotal.add(&odd.vat).unwrap(), d("616.00"), "subtotal plus VAT is exactly the amount");
    let usd = invoice_tax(&d("14.00"), Currency::Usd);
    assert_eq!((usd.subtotal.to_string(), usd.vat.is_zero(), usd.label), ("14.00".to_string(), true, None));
}

#[test]
fn cpn_012_the_coupon_field_never_fetches_twice_clears_on_empty_and_locks_on_success() {
    let mut field = CouponField::default();
    assert_eq!(field.submit("SAVE"), CouponEffect::Fetch("SAVE".into()));
    assert_eq!(field.submit("SAVE"), CouponEffect::None, "the same code is not fetched twice");
    field.lookup_result(CouponLookup::NotFound, now());
    assert_eq!(field.message(), Some("Coupon is not valid"));
    assert_eq!(field.submit("OLD"), CouponEffect::Fetch("OLD".into()));
    let mut expired = coupon("OLD", Some("0.1"), Some("Pay-as-you-Go"));
    expired.expires_at = Some(utc(2020, 1, 1, 0, 0, 0, 0));
    field.lookup_result(CouponLookup::Found(expired), now());
    assert_eq!(field.message(), Some("Coupon has expired"));
    assert!(field.applied().is_none());
    assert_eq!(field.submit("BOOM"), CouponEffect::Fetch("BOOM".into()));
    field.lookup_result(CouponLookup::Failed, now());
    assert_eq!(field.message(), Some("Error"));
    assert_eq!(field.submit("GOOD"), CouponEffect::Fetch("GOOD".into()));
    field.lookup_result(CouponLookup::Found(coupon("GOOD", Some("0.1"), Some("Pay-as-you-Go"))), now());
    assert!(field.is_locked(), "after success the field locks and a green check replaces Apply");
    assert_eq!(field.message(), None);
    assert_eq!(field.submit("OTHER"), CouponEffect::None);
    // An empty code clears everything (only possible by unlocking through clear).
    field.clear();
    assert_eq!(field.submit(""), CouponEffect::None);
    assert!(!field.is_locked());
}

#[test]
fn cpn_019_the_special_client_alert_shows_the_host_supplied_text_verbatim() {
    let offer = SpecialClientOffer { code: "LOYAL".into(), percent: 33, message: "Use LOYAL for 25% off".into() };
    let alert = special_client_alert(true, &offer).unwrap();
    // DEF-023 stays as captured: the percentage is data, the text is shown unchanged.
    assert_eq!((alert.percent, alert.message.as_str(), alert.code.as_str()), (33, "Use LOYAL for 25% off", "LOYAL"));
    assert!(special_client_alert(false, &offer).is_none());
}

#[test]
fn order_amounts_the_documented_tax_examples_are_consistent_and_the_quote_is_compared_before_tax() {
    // Tax inclusive: the buyer pays the quoted price and tax is part of it.
    assert_eq!(order_net(745, 0, 124, 745, true), Ok(745));
    // Tax exclusive: tax is added on top, the quote is still the pre-tax subtotal.
    assert_eq!(order_net(1500, 0, 300, 1800, false), Ok(1500));
    assert_eq!(order_net(1490, 0, 0, 1490, false), Ok(1490));
}

#[test]
fn order_amounts_a_total_that_does_not_follow_from_the_tax_mode_is_refused() {
    // An exclusive order whose total ignores the tax, and an inclusive one that adds it.
    assert_eq!(order_net(1500, 0, 300, 1500, false), Err(OrderAmountProblem::TotalMismatch));
    assert_eq!(order_net(745, 0, 124, 869, true), Err(OrderAmountProblem::TotalMismatch));
    assert_eq!(order_net(1500, 0, 300, 1801, false), Err(OrderAmountProblem::TotalMismatch));
}

#[test]
fn order_amounts_negative_zero_or_overflowing_values_and_any_provider_discount_are_refused() {
    assert_eq!(order_net(-100, 0, 0, -100, false), Err(OrderAmountProblem::Invalid));
    assert_eq!(order_net(0, 0, 0, 0, false), Err(OrderAmountProblem::Invalid));
    assert_eq!(order_net(1000, 0, -1, 999, false), Err(OrderAmountProblem::Invalid));
    assert_eq!(order_net(1000, 0, 0, -1000, true), Err(OrderAmountProblem::Invalid));
    assert_eq!(order_net(i64::MAX, 0, 1, i64::MIN, false), Err(OrderAmountProblem::Invalid));
    assert_eq!(order_net(1000, -5, 0, 1005, false), Err(OrderAmountProblem::Invalid));
    // This sale never discounts at the provider: a nonzero discount is unsupported, not netted.
    assert_eq!(order_net(1000, 100, 0, 900, false), Err(OrderAmountProblem::DiscountPresent));
}

#[test]
fn d8_the_display_amount_is_the_page_total_and_stays_separate_from_the_charged_quote() {
    let cat = catalog();
    let at = now();
    let payg_10 = Product::PayAsYouGo { hours: 10 };
    // DEF-010: id shows 7.40 while the quote charges 7.45; neither replaces the other.
    assert_eq!(display_amount(&cat, payg_10, "id", None, at).unwrap().to_string(), "7.40");
    assert_eq!(amount(payg(10, Currency::Usd, "id", None, false)), "7.45");
    assert_eq!(display_amount(&cat, payg_10, "en", None, at).unwrap().to_string(), "14.90");
    // The page applies a coupon for the displayed product to the displayed total, half up.
    let half = coupon("HALF", Some("0.5"), Some("Pay-as-you-Go"));
    assert_eq!(display_amount(&cat, payg_10, "id", Some(&half), at).unwrap().to_string(), "3.70");
    let third = coupon("THIRD", Some("0.333"), Some("Pay-as-you-Go"));
    // 14.90 * 0.667 = 9.9383 -> 9.94
    assert_eq!(display_amount(&cat, payg_10, "en", Some(&third), at).unwrap().to_string(), "9.94");
    // Another product's coupon, or an expired one, leaves the shown total alone.
    let other = coupon("PLAN", Some("0.5"), Some("PREMIUM"));
    assert_eq!(display_amount(&cat, payg_10, "en", Some(&other), at).unwrap().to_string(), "14.90");
    let mut expired = half.clone();
    expired.expires_at = Some(Micros(at.0 - 1));
    assert_eq!(display_amount(&cat, payg_10, "en", Some(&expired), at).unwrap().to_string(), "14.90");
    // A plan shows its USD price; no coupon reaches a plan, as in its quote.
    let plan = Product::Plan { index: 0 };
    let shown = display_amount(&cat, plan, "id", Some(&half), at).unwrap();
    assert_eq!(shown, cat.plans[0].usd_price.quantize(2, Rounding::HalfUp).unwrap());
    // The special TRY offer changes the charge, never the shown USD total.
    assert_eq!(display_amount(&cat, payg_10, "tr", None, at).unwrap().to_string(), "14.90");
    // Out-of-range hours and unknown plans are refused like the quote.
    assert!(matches!(display_amount(&cat, Product::PayAsYouGo { hours: 4 }, "en", None, at), Err(OrderError::HoursOutOfRange { .. })));
    assert!(matches!(display_amount(&cat, Product::Plan { index: 99 }, "en", None, at), Err(OrderError::UnknownPlan(99))));
}
