use std::collections::BTreeSet;

use serde_json::Value;
use transgate_core::presentation::fixtures;
use transgate_core::presentation::msg::{Msg, Ns, app, common};

use super::*;

#[test]
fn plural_categories_follow_cldr_for_whole_numbers() {
    let table: &[(&str, &[(i64, &str)])] = &[
        ("en", &[(0, "other"), (1, "one"), (2, "other"), (-1, "one")]),
        ("tr", &[(1, "one"), (5, "other")]),
        ("id", &[(1, "other"), (7, "other")]),
        ("fr", &[(0, "one"), (1, "one"), (2, "other"), (1_000_000, "many")]),
        ("pt", &[(0, "one"), (1, "one"), (2, "other"), (2_000_000, "many")]),
        ("es", &[(0, "other"), (1, "one"), (1_000_000, "many")]),
        ("ru", &[(1, "one"), (2, "few"), (4, "few"), (5, "many"), (11, "many"), (12, "many"), (21, "one"), (22, "few"), (100, "many")]),
    ];
    for (locale, cases) in table {
        for (count, expected) in *cases {
            assert_eq!(plural_category(locale, *count), *expected, "{locale} {count}");
        }
    }
}

#[test]
fn a_count_picks_the_plural_variant_and_interpolates_it() {
    let seconds = |n| common("redirectMessage").count(n);
    assert_eq!(translate("en", &seconds(1)).unwrap(), "You will be redirected to the home page in 1 second.");
    assert_eq!(translate("en", &seconds(5)).unwrap(), "You will be redirected to the home page in 5 seconds.");
    // Russian has its own few/many variants in the catalog.
    assert!(translate("ru", &seconds(3)).unwrap().contains('3'));
    assert_ne!(translate("ru", &seconds(3)).unwrap(), translate("ru", &seconds(5)).unwrap());
}

#[test]
fn arguments_interpolate_in_any_catalog_and_nested_messages_translate_first() {
    let discount = common("special_client_discount_message").text("percentage", "25");
    assert_eq!(translate("en", &discount).unwrap(), "Because you are an old client you have a discount code 25%!");
    let nested = app("p5.erasure.local_data_erased").count(2);
    assert!(translate("tr", &nested).unwrap().contains('2'));
    let outer = common("And {{count}} more transcripts...").nested("count", common("Cancel"));
    assert_eq!(translate("en", &outer).unwrap(), "And Cancel more transcripts...");
}

#[test]
fn there_is_no_fallback_a_missing_key_argument_or_locale_is_an_error() {
    assert_eq!(
        translate("tr", &common("this key does not exist")),
        Err(TranslateError::MissingKey { locale: "tr".into(), ns: "common", key: "this key does not exist".into() })
    );
    assert!(matches!(
        translate("en", &common("special_client_discount_message")),
        Err(TranslateError::MissingArgument { name, .. }) if name == "percentage"
    ));
    assert_eq!(translate("xx", &common("Cancel")), Err(TranslateError::UnknownLocale("xx".into())));
    // A region-tagged or differently cased code is not the catalog's code: no guessing.
    assert_eq!(translate("tr-TR", &common("Cancel")), Err(TranslateError::UnknownLocale("tr-TR".into())));
    assert_eq!(translate("EN", &common("Cancel")), Err(TranslateError::UnknownLocale("EN".into())));
    // The same key in another namespace is not found: lookups are literal, never searched.
    assert!(translate("en", &Msg::new(Ns::Auth, "Cancel")).is_err());
}

/// A translation may reword but never lose or invent an argument: every locale
/// uses exactly the placeholders of the English text (plural variants excepted:
/// a language may say "one second" without the number).
#[test]
fn every_translation_uses_the_placeholders_of_its_english_source() {
    let english: std::collections::HashMap<(Ns, &str), BTreeSet<&str>> =
        entries().filter(|(locale, ..)| *locale == "en").map(|(_, ns, key, template)| ((ns, key), placeholders(template).into_iter().collect())).collect();
    let mut drift = Vec::new();
    for (locale, ns, key, template) in entries() {
        let is_plural_variant = ["_zero", "_one", "_two", "_few", "_many", "_other"].iter().any(|s| key.ends_with(s));
        if locale == "en" || is_plural_variant {
            continue;
        }
        let Some(expected) = english.get(&(ns, key)) else { continue };
        let got: BTreeSet<&str> = placeholders(template).into_iter().collect();
        if &got != expected {
            drift.push(format!("{locale}/{}: {key:?} uses {got:?}, English {expected:?}", ns.file()));
        }
    }
    assert!(drift.is_empty(), "{} translations disagree with English placeholders:\n{}", drift.len(), drift.join("\n"));
}

/// Every message the state machines emit (the fixtures cover each screen state,
/// with their real arguments) translates in all 16 locales.
#[test]
fn every_message_core_emits_translates_in_all_sixteen_locales() {
    fn walk(value: &Value, out: &mut Vec<Msg>) {
        match value {
            Value::Object(map) => {
                if map.contains_key("ns")
                    && map.contains_key("key")
                    && let Ok(msg) = serde_json::from_value::<Msg>(value.clone())
                {
                    out.push(msg);
                    return;
                }
                map.values().for_each(|v| walk(v, out));
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut emitted = Vec::new();
    walk(&fixtures::export_json(), &mut emitted);
    assert!(emitted.len() > 200, "the fixtures exercise many messages: {}", emitted.len());
    let mut failures = Vec::new();
    for msg in &emitted {
        for locale in LOCALES {
            if let Err(error) = translate(locale, msg) {
                failures.push(error.to_string());
            }
        }
    }
    failures.sort();
    failures.dedup();
    assert!(failures.is_empty(), "{} messages do not translate:\n{}", failures.len(), failures.join("\n"));
}

/// The web's reachability manifest lists every key a screen can show; each is in
/// every locale (the strict catalog policy the validator also enforces).
#[test]
fn every_reachable_key_exists_in_every_locale() {
    let manifest: Value = serde_json::from_str(include_str!("../../web/locales/reachable-keys.json")).unwrap();
    let mut missing = Vec::new();
    for (ns_name, keys) in manifest["namespaces"].as_object().unwrap() {
        let Some(ns) = Ns::ALL.into_iter().find(|ns| ns.file() == ns_name) else { continue };
        for key in keys.as_object().unwrap().keys() {
            for locale in LOCALES {
                // A plural family has only suffixed keys, never the plain one.
                let family = ["_one", "_other"].iter().any(|s| has_key(locale, ns, &format!("{key}{s}")).unwrap());
                if !has_key(locale, ns, key).unwrap() && !family {
                    missing.push(format!("{locale}/{ns_name}: {key:?}"));
                }
            }
        }
    }
    assert!(missing.is_empty(), "{} reachable keys missing:\n{}", missing.len(), missing.join("\n"));
}
