use serde_json::Value;

use super::*;

fn fixture(name: &str) -> Vec<Value> {
    let text = match name {
        "transcription" => include_str!("../../../fixtures/languages/transcription-languages.json"),
        "translation" => include_str!("../../../fixtures/languages/translation-targets.json"),
        "locales" => include_str!("../../../fixtures/languages/ui-locales.json"),
        _ => unreachable!(),
    };
    serde_json::from_str(text).expect("fixture parses")
}

// PRD 7.4, LNG-008, LNG-033: every offered transcription language with the same name and code.
#[test]
fn transcription_languages_are_a_superset_of_the_fixture() {
    let rows = fixture("transcription");
    assert_eq!(rows.len(), 61);
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let code = row["code"].as_str().unwrap();
        let language = Language::from_name(name).unwrap_or_else(|| panic!("{name} missing"));
        assert_eq!(language.code(), code, "{name}");
        let providers: Vec<&str> = row["providers"].as_array().unwrap().iter().map(|p| p["provider"].as_str().unwrap()).collect();
        assert_eq!(language.soniox(), providers.contains(&"soniox"), "{name} soniox");
        assert_eq!(language.deepgram(), providers.contains(&"deepgram"), "{name} deepgram");
        // LNG-017: every provider receives the same lower case code.
        for p in row["providers"].as_array().unwrap() {
            assert_eq!(p["code_sent"].as_str().unwrap(), code);
        }
    }
}

// PRD 7.4, LNG-012, TRL-003: the 53 translation languages, all of them Soniox languages.
#[test]
fn translation_targets_are_a_superset_of_the_fixture() {
    let rows = fixture("translation");
    assert_eq!(rows.len(), 53);
    for row in &rows {
        let language = Language::from_code(row["code"].as_str().unwrap()).unwrap();
        assert_eq!(language.name(), row["name"].as_str().unwrap());
        assert!(language.translation_capable());
        assert!(language.soniox());
    }
    assert_eq!(Language::translation_languages().count(), 53);
}

// LNG-001: exactly 16 UI locales with `en` as the only default.
#[test]
fn ui_locales_match_the_fixture() {
    let rows = fixture("locales");
    assert_eq!(rows.len(), 16);
    for row in &rows {
        let locale = ui_locale(row["code"].as_str().unwrap()).unwrap();
        assert_eq!(locale.default, row["default"].as_bool().unwrap());
    }
    assert_eq!(default_ui_locale().code, "en");
}

// LNG-015/TRX-011: the eight Soniox exclusions are exactly the Deepgram-only languages.
#[test]
fn eight_languages_are_deepgram_only() {
    let mut only: Vec<&str> = Language::all().filter(|l| !l.soniox()).map(|l| l.code()).collect();
    only.sort_unstable();
    assert_eq!(only, ["hy", "is", "ka", "la", "lb", "mi", "ne", "uz"]);
    assert!(Language::all().all(|l| l.deepgram()));
}

// toTransgateLanguage is case-insensitive and refuses empty or unknown codes.
#[test]
fn codes_map_back_to_stored_names() {
    assert_eq!(Language::from_code("ZH").unwrap().name(), "Chinese");
    assert_eq!(Language::from_code("no").unwrap().name(), "Norwegian");
    assert!(Language::from_code("").is_none());
    assert!(Language::from_code("bn").is_none());
}

// TRX-023, LNG-013: one table maps names to codes and every translation target has a
// non-empty lower case code, so a job cannot hold a target "without a code".
#[test]
fn trx_023_every_translation_target_has_a_code() {
    for language in Language::all().filter(|l| l.translation_capable()) {
        let code = language.code();
        assert!(!code.is_empty() && code == code.to_lowercase(), "{}: {code:?}", language.name());
        assert_eq!(Language::from_code(code), Some(language), "{code}");
    }
}

// LNG-007: the RTL list compares case-insensitively and trims.
#[test]
fn rtl_detection_follows_the_source_list() {
    assert!(is_rtl(Some(" Arabic ")));
    assert!(is_rtl(Some("farsi")));
    assert!(!is_rtl(None));
    assert!(!is_rtl(Some("Turkish")));
    let rtl: Vec<&str> = Language::all().filter(|l| l.is_rtl()).map(|l| l.name()).collect();
    assert_eq!(rtl, ["Arabic", "Hebrew", "Persian", "Urdu"]);
}
