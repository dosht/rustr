//! Transgate's captured billing data: the package catalog and the freemium
//! exception lists. These are Transgate's data, not constants of the billing
//! module. Source: the billing discovery ledger (`transgate/docs/discovery/billing.md`,
//! BIL-050..056, CPN-015/016, FRE-016/017), taken from `frontend/src/domain/packages.ts`
//! and `frontend/src/utils/freemiumLimits.ts`.
//!
//! The Iyzico plan reference codes are provider configuration identifiers that
//! the discovery ledger records. No customer list, credential or seller
//! detail is stored here.

use std::collections::BTreeSet;

use billing_core::decimal::Decimal;
use billing_core::freemium::FreemiumLists;
use billing_core::grants::PackageSpec;
use billing_core::pricing::{Catalog, PaygEntry, PlanEntry, PlanInterval};

fn d(text: &str) -> Decimal {
    text.parse().expect("catalog decimal literal")
}

fn monthly_spec(name: &str, minutes: i64, count: u32) -> PackageSpec {
    // Every plan package is valid for one calendar month; a yearly plan
    // creates 12 of them, one month apart.
    PackageSpec { name: name.into(), minutes, count, interval: "month".into(), frequency: 1, expiry: 1 }
}

pub fn catalog() -> Catalog {
    Catalog {
        // The array index is the `packageId` the checkout form sends.
        plans: vec![
            PlanEntry {
                name: "PREMIUM".into(),
                interval: PlanInterval::Monthly,
                spec: monthly_spec("PREMIUM", 1200, 1),
                usd_price: d("14"),
                struck_price: d("40"),
                plan_code_try: Some("60fb9b3d-c6aa-4357-aa9f-463804d8a169".into()),
                plan_code_usd: Some("9aee031c-1cf3-4921-b4b2-b7c337b94f79".into()),
            },
            PlanEntry {
                name: "BUSINESS".into(),
                interval: PlanInterval::Monthly,
                spec: monthly_spec("BUSINESS", 2400, 1),
                usd_price: d("21"),
                struck_price: d("60"),
                plan_code_try: Some("502aa78e-b325-40fb-9eef-36cf9239d863".into()),
                plan_code_usd: Some("d52214fc-f41d-41d3-a0a3-c4824c1ae35c".into()),
            },
            PlanEntry {
                name: "PREMIUM".into(),
                interval: PlanInterval::Yearly,
                spec: monthly_spec("PREMIUM", 1200, 12),
                usd_price: d("120"),
                struck_price: d("30"),
                plan_code_try: Some("77666eba-6655-4888-8502-7c829759569b".into()),
                plan_code_usd: Some("b36a6079-57d6-45a5-aad1-a1fa7a03c158".into()),
            },
            PlanEntry {
                name: "BUSINESS".into(),
                interval: PlanInterval::Yearly,
                spec: monthly_spec("BUSINESS", 2400, 12),
                usd_price: d("180"),
                struck_price: d("55"),
                plan_code_try: Some("4b7bdf78-8265-475c-aef7-bc824e962b56".into()),
                plan_code_usd: Some("23f69d4b-6c64-4f3f-b25c-d67c6a491128".into()),
            },
        ],
        payg: Some(PaygEntry {
            // The checkout stores `Pay-as-you-Go`; the catalog constant says `PAY AS YOU GO`.
            stored_name: "Pay-as-you-Go".into(),
            catalog_label: "PAY AS YOU GO".into(),
            usd_per_hour: d("1.49"),
            struck_per_hour: d("2"),
            min_hours: 5,
            max_hours: 9999,
            minutes_per_hour: 60,
        }),
        locale_discount_locales: BTreeSet::from(["id".to_string(), "pt".to_string()]),
        // TRY 14.99 per hour for the two configured special-offer buyers.
        special_offer_try_per_hour: Some(d("14.99")),
    }
}

/// FRE-016: the 48 exception UI locales. English is not among them.
pub const EXCEPTION_LOCALES: [&str; 48] = [
    "tr", "de", "fr", "es", "it", "pt", "zh", "ja", "ko", "sv", "no", "da", "fi", "pl", "cs", "hu", "ro", "sk", "sl",
    "et", "lv", "lt", "mk", "ms", "mr", "mi", "ne", "fa", "sr", "bs", "sq", "ca", "gl", "ur", "bn", "ta", "te", "kn",
    "ml", "gu", "pa", "sw", "af", "is", "cy", "ga", "mt", "lb",
];

/// FRE-017: the 37 exception language names, lower case; the same list serves
/// the job language and the translation target.
pub const EXCEPTION_LANGUAGES: [&str; 37] = [
    "turkish", "german", "french", "spanish", "italian", "portuguese", "chinese", "japanese", "korean", "swedish",
    "norwegian", "danish", "finnish", "polish", "czech", "hungarian", "romanian", "slovak", "slovenian", "estonian",
    "latvian", "lithuanian", "macedonian", "malay", "marathi", "maori", "nepali", "persian", "russian", "serbian",
    "swahili", "tagalog", "tamil", "thai", "ukrainian", "vietnamese", "welsh",
];

pub fn freemium_lists() -> FreemiumLists {
    FreemiumLists::new(&EXCEPTION_LOCALES, &EXCEPTION_LANGUAGES)
}
