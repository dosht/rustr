use super::*;
use crate::calendar::utc;
use crate::ledger::Package;
use crate::types::AccountId;
use uuid::Uuid;

fn lists() -> FreemiumLists {
    FreemiumLists::new(&["tr", "de", "pt"], &["turkish", "german", "russian"])
}

fn draft(id: i64, minutes: Option<i64>, language: Option<&str>, target: Option<&str>) -> Draft {
    Draft {
        id,
        duration: minutes,
        language: language.map(str::to_string),
        first_translation_target: target.map(str::to_string),
    }
}

fn package(name: Option<&str>, start_month: u8, end_month: u8) -> Package {
    Package {
        id: 1,
        account: AccountId(Uuid::from_u128(1)),
        payment_id: None,
        name: name.map(str::to_string),
        minutes_included: 20,
        start_at: utc(2023, start_month, 1, 0, 0, 0, 0),
        end_at: utc(2023, end_month, 1, 0, 0, 0, 0),
    }
}

#[test]
fn fre_015_limits_are_inclusive_and_exceptions_raise_the_cap_to_45() {
    let l = lists();
    // 20 is allowed, 21 is not, for an English draft with an English locale.
    assert!(web_guard(&[draft(1, Some(20), Some("English"), None)], "en", &l).is_ok());
    assert_eq!(
        web_guard(&[draft(1, Some(21), Some("English"), None)], "en", &l),
        Err(GuardRefusal { total_minutes: 21 })
    );
    // Locale exception: 45 inclusive, 46 refused.
    assert!(web_guard(&[draft(1, Some(45), Some("English"), None)], "de", &l).is_ok());
    assert!(web_guard(&[draft(1, Some(46), Some("English"), None)], "de", &l).is_err());
    // Missing, zero and unknown totals are allowed.
    assert!(web_guard(&[], "en", &l).is_ok());
    assert!(web_guard(&[draft(1, Some(0), None, None)], "en", &l).is_ok());
    assert!(web_guard(&[draft(1, None, None, None)], "en", &l).is_ok());
    assert_eq!(GuardRefusal { total_minutes: 21 }.message(), "Not enough balance.");
}

#[test]
fn fre_015_language_and_first_translation_target_are_lower_cased_exceptions() {
    let l = lists();
    assert!(web_guard(&[draft(1, Some(30), Some("Turkish"), None)], "en", &l).is_ok());
    assert!(web_guard(&[draft(1, Some(30), Some("English"), Some("GERMAN"))], "en", &l).is_ok());
    assert!(web_guard(&[draft(1, Some(30), Some("English"), Some("French"))], "en", &l).is_err());
}

#[test]
fn fre_016_017_the_english_locale_is_not_an_exception() {
    let l = lists();
    assert!(!l.locale_is_exception("en"));
    assert!(l.locale_is_exception("tr"));
    assert!(l.language_is_exception("Russian"));
}

#[test]
fn fre_018_only_the_first_draft_by_id_decides_the_exception_in_any_request_order() {
    let l = lists();
    let turkish_first = draft(1, Some(20), Some("Turkish"), None);
    let english_second = draft(2, Some(10), Some("English"), None);
    // 30 minutes: allowed only because the lowest-id draft is Turkish.
    let forward = web_guard(&[turkish_first.clone(), english_second.clone()], "en", &l);
    let reversed = web_guard(&[english_second.clone(), turkish_first.clone()], "en", &l);
    assert!(forward.is_ok());
    assert_eq!(forward, reversed, "request array order must not change the decision");
    // And the other way round: the lowest id is English, so no exception.
    let english_first = draft(1, Some(20), Some("English"), None);
    let turkish_second = draft(2, Some(10), Some("Turkish"), None);
    let a = web_guard(&[english_first.clone(), turkish_second.clone()], "en", &l);
    let b = web_guard(&[turkish_second, english_first], "en", &l);
    assert_eq!(a, Err(GuardRefusal { total_minutes: 30 }));
    assert_eq!(a, b);
}

#[test]
fn fre_013_app_definition_one_is_exactly_one_package_row_named_freemium_any_dates() {
    assert!(is_freemium_app1(&[package(Some("freemium"), 1, 2)]), "an expired lone free package still counts");
    assert!(!is_freemium_app1(&[package(Some("freemium"), 1, 2), package(Some("PREMIUM"), 1, 2)]), "any purchase ever ends it");
    assert!(!is_freemium_app1(&[]));
    assert!(!is_freemium_app1(&[package(Some("PREMIUM"), 1, 2)]));
}

#[test]
fn fre_014_app_definition_two_ignores_start_dates_and_counts_a_query_error_as_freemium() {
    let now = utc(2023, 6, 1, 0, 0, 0, 0);
    // A paid package that has not started but has not ended counts as paid.
    let future_paid = package(Some("PREMIUM"), 8, 9);
    assert!(!is_freemium_app2(Some(&[future_paid]), now));
    // An expired paid package does not.
    assert!(is_freemium_app2(Some(&[package(Some("PREMIUM"), 1, 2)]), now));
    // end_at >= now is inclusive.
    assert!(!is_freemium_app2(Some(&[package(Some("PREMIUM"), 1, 6)]), now));
    assert!(is_freemium_app2(None, now), "on a query error the user counts as freemium");
}

#[test]
fn def_019_the_three_facets_are_kept_separate_for_one_user() {
    // Paid package expired, free package still open: database freemium and app 2 freemium,
    // but app definition 1 (more than one row ever) says not freemium.
    let now = utc(2023, 1, 15, 0, 0, 0, 0);
    let packages = vec![
        package(Some("PREMIUM"), 1, 1),
        Package { end_at: utc(2023, 3, 1, 0, 0, 0, 0), id: 2, ..package(Some("freemium"), 1, 3) },
    ];
    let f = facets(&packages, now);
    assert!(f.database_no_open_paid_package, "paid window 01-01..01-01 has closed by 01-15");
    assert!(!f.historical_single_free_package);
    assert!(f.no_unexpired_paid_package);
}

#[test]
fn fre_020_display_cut_keeps_lines_up_to_22_minutes_and_fades_the_last_three() {
    let starts = [0.0, 600.0, 1200.0, 1319.0, 1320.0, 1321.0, 1500.0];
    let cut = display_cut(true, 30, &starts);
    assert_eq!(cut.visible, 5, "lines starting at or before 1320 seconds");
    assert_eq!(cut.fades, vec![(2, 50), (3, 30), (4, 10)]);
    assert!(cut.show_upgrade_banner);
    // Not freemium, or a job of 20 minutes or less: untouched.
    assert_eq!(display_cut(false, 30, &starts).visible, starts.len());
    let short = display_cut(true, 20, &starts);
    assert_eq!(short.visible, starts.len());
    assert!(!short.show_upgrade_banner);
    // Fewer than three visible lines fade from the end.
    assert_eq!(display_cut(true, 30, &[0.0, 5000.0]).fades, vec![(0, 10)]);
}

#[test]
fn fre_021_load_cut_keeps_lines_to_21_minutes_plus_three_and_stops_live_updates() {
    let starts: Vec<f64> = (0..40).map(|i| i as f64 * 60.0).collect(); // 0, 60 ... 2340
    let cut = load_cut(true, &starts);
    // Lines at 0..=1260 are 22 lines (0 to 21 minutes), plus 3 preview lines.
    assert_eq!(cut.keep, 25);
    assert!(cut.stop_live);
    let short = load_cut(true, &[0.0, 300.0]);
    assert_eq!(short.keep, 2);
    assert!(!short.stop_live);
    assert_eq!(load_cut(false, &starts).keep, starts.len());
}

#[test]
fn fre_023_the_banner_progress_is_capped_at_one_hundred_percent() {
    assert_eq!(banner_progress(10, 20), 50.0);
    assert_eq!(banner_progress(40, 20), 100.0);
    assert_eq!(banner_progress(5, DEFAULT_BANNER_MAX), 25.0);
}

#[test]
fn fre_024_unparseable_or_missing_chat_caps_mean_no_limit() {
    let caps = ChatCaps::parse(Some("50"), Some("20000"), Some("500"), Some("not a number"));
    assert_eq!(caps.per_transcript_questions, Some(50));
    assert_eq!(caps.per_transcript_tokens, Some(20000));
    assert_eq!(caps.per_user_daily_questions, Some(500));
    assert_eq!(caps.per_user_daily_tokens, None);
    assert_eq!(ChatCaps::parse(None, Some(""), Some("-3"), Some("0")).per_transcript_questions, None);
}
