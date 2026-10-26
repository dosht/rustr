//! Transgate's captured catalog and freemium lists against the discovery
//! ledger's literal values, priced through the billing core.

use std::collections::BTreeSet;

use billing_core::calendar::{add_months, utc};
use billing_core::decimal::Decimal;
use billing_core::freemium::{Draft, web_guard};
use billing_core::grants::expand_grant;
use billing_core::pricing::{
    Currency, OrderInput, PlanInterval, Product, SAVE_BADGE, payg_card, plan_card, price_order,
};
use transgate_billing::catalog::{EXCEPTION_LANGUAGES, EXCEPTION_LOCALES, catalog, freemium_lists};

fn d(text: &str) -> Decimal {
    text.parse().unwrap()
}

/// Iyzico plan reference codes (TRY, USD) per plan index, copied from the discovery ledger's
/// plan table (`transgate/docs/discovery/billing.md`, BIL-050..053), not from the catalog.
pub const SOURCE_PLAN_CODES: [(&str, &str); 4] = [
    ("60fb9b3d-c6aa-4357-aa9f-463804d8a169", "9aee031c-1cf3-4921-b4b2-b7c337b94f79"),
    ("502aa78e-b325-40fb-9eef-36cf9239d863", "d52214fc-f41d-41d3-a0a3-c4824c1ae35c"),
    ("77666eba-6655-4888-8502-7c829759569b", "b36a6079-57d6-45a5-aad1-a1fa7a03c158"),
    ("4b7bdf78-8265-475c-aef7-bc824e962b56", "23f69d4b-6c64-4f3f-b25c-d67c6a491128"),
];

#[test]
fn bil_050_051_052_053_the_four_plans_are_the_captured_packages() {
    let c = catalog();
    let expected = [
        ("PREMIUM", PlanInterval::Monthly, 1200, 1, "14", "40"),
        ("BUSINESS", PlanInterval::Monthly, 2400, 1, "21", "60"),
        ("PREMIUM", PlanInterval::Yearly, 1200, 12, "120", "30"),
        ("BUSINESS", PlanInterval::Yearly, 2400, 12, "180", "55"),
    ];
    assert_eq!(c.plans.len(), 4);
    for (plan, (name, interval, minutes, count, usd, struck)) in c.plans.iter().zip(expected) {
        assert_eq!((plan.name.as_str(), plan.interval), (name, interval));
        assert_eq!((plan.spec.minutes, plan.spec.count), (minutes, count));
        assert_eq!((plan.spec.interval.as_str(), plan.spec.frequency, plan.spec.expiry), ("month", 1, 1), "each package is valid for one month");
        assert_eq!((plan.usd_price, plan.struck_price), (d(usd), d(struck)));
    }
    // The provider plan codes are the ledger's, in order, TRY then USD.
    for (plan, (try_code, usd_code)) in c.plans.iter().zip(SOURCE_PLAN_CODES) {
        assert_eq!((plan.plan_code_try.as_deref(), plan.plan_code_usd.as_deref()), (Some(try_code), Some(usd_code)));
    }
}

#[test]
fn bil_054_056_pay_as_you_go_is_149_per_hour_for_a_year_under_the_stored_name() {
    let c = catalog();
    let payg = c.payg.as_ref().unwrap();
    assert_eq!((payg.stored_name.as_str(), payg.catalog_label.as_str()), ("Pay-as-you-Go", "PAY AS YOU GO"));
    assert_eq!((payg.usd_per_hour, payg.struck_per_hour, payg.min_hours, payg.max_hours, payg.minutes_per_hour), (d("1.49"), d("2"), 5, 9999, 60));
    let quote = price_order(
        &c,
        &OrderInput { product: Product::PayAsYouGo { hours: 10 }, currency: Currency::Usd, ui_locale: "en", coupon: None, special_offer: false, rate: &d("44"), now: utc(2026, 1, 1, 0, 0, 0, 0) },
    )
    .unwrap();
    assert_eq!((quote.spec.minutes, quote.spec.count, quote.spec.interval.as_str(), quote.spec.expiry), (600, 1, "year", 1));
    assert_eq!(quote.amount.to_string(), "14.90");
    let created = utc(2026, 2, 28, 10, 0, 0, 0);
    let granted = expand_grant(created, &quote.spec).unwrap();
    assert_eq!(granted[0].end_at, add_months(created, 12).unwrap());
}

#[test]
fn bil_055_a_yearly_renewal_grants_twelve_packages_and_a_monthly_one() {
    let c = catalog();
    let at = utc(2026, 6, 15, 9, 0, 0, 0);
    assert_eq!(expand_grant(at, &c.plans[2].spec).unwrap().len(), 12);
    assert_eq!(expand_grant(at, &c.plans[3].spec).unwrap().len(), 12);
    assert_eq!(expand_grant(at, &c.plans[0].spec).unwrap().len(), 1);
}

#[test]
fn cpn_003_try_plan_amounts_are_the_usd_price_times_the_rate_and_each_plan_and_currency_selects_its_source_plan_code() {
    let c = catalog();
    let try_amounts = ["616.00", "924.00", "5280.00", "7920.00"];
    for (index, expected) in try_amounts.iter().enumerate() {
        let order = |currency| {
            price_order(&c, &OrderInput { product: Product::Plan { index }, currency, ui_locale: "tr", coupon: None, special_offer: false, rate: &d("44"), now: utc(2026, 1, 1, 0, 0, 0, 0) })
                .unwrap()
        };
        let try_quote = order(Currency::Try);
        assert_eq!(try_quote.amount.to_string(), *expected);
        // Compared with the ledger's own identifiers, never with the catalog's own fields.
        assert_eq!(try_quote.pricing_plan_id.as_deref(), Some(SOURCE_PLAN_CODES[index].0), "plan {index} TRY");
        assert_eq!(order(Currency::Usd).pricing_plan_id.as_deref(), Some(SOURCE_PLAN_CODES[index].1), "plan {index} USD");
    }
}

#[test]
fn cpn_015_016_displayed_prices_are_per_month_with_the_captured_reference_prices_and_the_fixed_badge() {
    let c = catalog();
    let shown: Vec<(String, String)> = c.plans.iter().map(|p| {
        let card = plan_card(p);
        (card.usd_per_month.to_string(), card.struck_per_month.to_string())
    }).collect();
    assert_eq!(
        shown,
        vec![("14".into(), "40".into()), ("21".into(), "60".into()), ("10.00".into(), "30".into()), ("15.00".into(), "55".into())]
    );
    assert_eq!(plan_card(&c.plans[2]).badge, SAVE_BADGE);
    assert_eq!(SAVE_BADGE, "Save %10");
    let card = payg_card(&c).unwrap();
    assert_eq!((card.price_text.as_str(), card.struck_per_hour), ("$1.49 per 1 hour", d("2")));
}

#[test]
fn cpn_006_007_the_locale_discount_and_the_special_offer_are_catalog_data() {
    let c = catalog();
    assert_eq!(c.locale_discount_locales, BTreeSet::from(["id".to_string(), "pt".to_string()]));
    assert_eq!(c.special_offer_try_per_hour, Some(d("14.99")));
}

const LEDGER_LOCALES: &str = "tr de fr es it pt zh ja ko sv no da fi pl cs hu ro sk sl et lv lt mk ms mr mi ne fa sr bs sq ca gl ur bn ta te kn ml gu pa sw af is cy ga mt lb";
const LEDGER_LANGUAGES: &str = "turkish german french spanish italian portuguese chinese japanese korean swedish norwegian danish finnish polish czech hungarian romanian slovak slovenian estonian latvian lithuanian macedonian malay marathi maori nepali persian russian serbian swahili tagalog tamil thai ukrainian vietnamese welsh";

#[test]
fn fre_016_017_the_exception_lists_are_exactly_the_ledgers_48_locales_and_37_languages() {
    let locales: BTreeSet<&str> = LEDGER_LOCALES.split(' ').collect();
    let languages: BTreeSet<&str> = LEDGER_LANGUAGES.split(' ').collect();
    assert_eq!((locales.len(), languages.len()), (48, 37));
    assert_eq!(EXCEPTION_LOCALES.iter().copied().collect::<BTreeSet<_>>(), locales);
    assert_eq!(EXCEPTION_LANGUAGES.iter().copied().collect::<BTreeSet<_>>(), languages);
    let lists = freemium_lists();
    assert!(!lists.locale_is_exception("en"), "English is not an exception");
    assert_eq!(lists.ui_locales().len(), 48);
    assert_eq!(lists.languages().len(), 37);
}

#[test]
fn fre_015_018_the_real_lists_drive_the_web_guard_by_the_lowest_draft_id() {
    let lists = freemium_lists();
    let draft = |id, minutes, language: &str| Draft { id, duration: Some(minutes), language: Some(language.into()), first_translation_target: None };
    // 30 minutes: refused for English, allowed when the first draft is Turkish, a locale exception also allows it.
    assert!(web_guard(&[draft(1, 30, "English")], "en", &lists).is_err());
    assert!(web_guard(&[draft(1, 30, "Turkish")], "en", &lists).is_ok());
    assert!(web_guard(&[draft(1, 30, "English")], "tr", &lists).is_ok());
    // The lowest id decides, however the request lists them.
    let (turkish, english) = (draft(1, 15, "Turkish"), draft(2, 15, "English"));
    assert!(web_guard(&[english.clone(), turkish.clone()], "en", &lists).is_ok());
    assert!(web_guard(&[turkish, english], "en", &lists).is_ok());
    // Inclusive 45.
    assert!(web_guard(&[draft(1, 45, "Turkish")], "en", &lists).is_ok());
    assert!(web_guard(&[draft(1, 46, "Turkish")], "en", &lists).is_err());
}
