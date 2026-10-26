use super::*;
use crate::calendar::utc;
use crate::ledger::{Package, PackageBalance};
use crate::types::AccountId;
use uuid::Uuid;

fn pkg(id: i64, name: Option<&str>, included: i64, consumed: i64, end_month: u8) -> PackageBalance {
    PackageBalance {
        package: Package {
            id,
            account: AccountId(Uuid::from_u128(1)),
            payment_id: None,
            name: name.map(str::to_string),
            minutes_included: included,
            start_at: utc(2023, 1, 1, 0, 0, 0, 0),
            end_at: utc(2023, end_month, 1, 0, 0, 0, 0),
        },
        consumed,
    }
}

fn job(subject: i64, duration: Option<i64>, completed: u64) -> JobStart {
    JobStart { subject, duration, other_completed_jobs: completed }
}

fn free(consumed: i64) -> PackageBalance {
    pkg(1, Some("freemium"), 20, consumed, 3)
}

fn paid(minutes: i64) -> PackageBalance {
    pkg(2, Some("PREMIUM"), minutes, 0, 6)
}

#[test]
fn bil_025_026_079_only_draft_ready_uploaded_to_not_started_bills() {
    for from in ["draft", "ready", "uploaded"] {
        assert!(is_billable_transition(from, "not_started"), "{from} starts a job");
    }
    // Later states, a completed job sent back, and every other target charge nothing.
    for from in ["not_started", "pending", "running", "completed", "failed", "canceled"] {
        assert!(!is_billable_transition(from, "not_started"), "{from} must not bill again");
    }
    assert!(!is_billable_transition("draft", "running"));
}

#[test]
fn bil_027_the_whole_duration_is_charged_in_one_decision() {
    let open = vec![paid(100)];
    match decide_job_start(&job(1, Some(37), 5), &open).unwrap() {
        StartCharge::Strict(rows) => assert_eq!(rows.iter().map(|r| r.minutes).sum::<i64>(), 37),
        other => panic!("expected a strict charge, got {other:?}"),
    }
}

#[test]
fn bil_028_unknown_duration_defers_without_a_balance_check() {
    assert_eq!(decide_job_start(&job(1, None, 0), &[]).unwrap(), StartCharge::Deferred);
    assert_eq!(decide_job_start(&job(1, None, 9), &[paid(0)]).unwrap(), StartCharge::Deferred);
}

#[test]
fn bil_029_030_031_paid_path_errors_and_exact_messages() {
    // No open package at all.
    assert_eq!(decide_job_start(&job(1, Some(5), 3), &[]), Err(StartRefusal::NoAvailablePackage));
    // Balance below the duration.
    assert_eq!(
        decide_job_start(&job(1, Some(60), 3), &[paid(59)]),
        Err(StartRefusal::NotEnoughBalance { required: 60, available: 59 })
    );
    // Exactly enough passes.
    assert!(decide_job_start(&job(1, Some(59), 3), &[paid(59)]).is_ok());
    let refusal = StartRefusal::NotEnoughBalance { required: 60, available: 59 };
    assert_eq!(refusal.code(), "insufficient_balance");
    assert_eq!(refusal.http_status(), 402);
    assert_eq!(refusal.message(false), "Not enough balance: required=60 min, available=59 min");
    assert_eq!(
        refusal.message(true),
        "Not enough balance. Please upgrade to continue. (required=60 min, available=59 min)"
    );
    assert_eq!(StartRefusal::NoAvailablePackage.message(false), "No available package");
}

#[test]
fn bil_032_jobs_started_one_after_another_each_deduct_their_own_duration() {
    let open = vec![pkg(1, Some("PREMIUM"), 100, 0, 6)];
    let plan = plan_batch(&[job(1, Some(20), 4), job(2, Some(30), 4), job(3, Some(40), 4)], &open).unwrap();
    let written: i64 = plan
        .iter()
        .map(|(_, c)| match c {
            StartCharge::Strict(rows) => rows.iter().map(|r| r.minutes).sum::<i64>(),
            _ => 0,
        })
        .sum();
    assert_eq!(written, 90);
    assert_eq!(plan.len(), 3, "three decisions, three ledger rows");
    // The fourth no longer fits the 10 left.
    let again = plan_batch(&[job(1, Some(20), 4), job(2, Some(30), 4), job(3, Some(40), 4), job(4, Some(11), 4)], &open);
    assert_eq!(again, Err(StartRefusal::NotEnoughBalance { required: 11, available: 10 }));
}

#[test]
fn bil_033_a_job_larger_than_the_first_package_draws_from_two() {
    let open = vec![pkg(1, Some("PREMIUM"), 30, 0, 3), pkg(2, Some("PREMIUM"), 50, 0, 6)];
    match decide_job_start(&job(1, Some(40), 2), &open).unwrap() {
        StartCharge::Strict(rows) => {
            assert_eq!(
                rows,
                vec![Allocation { package_id: 1, minutes: 30 }, Allocation { package_id: 2, minutes: 10 }]
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn bil_034_035_provider_lengths_round_up_and_missing_or_zero_means_nothing() {
    assert_eq!(provider_minutes_deepgram(Some(61.0)), Some(2));
    assert_eq!(provider_minutes_deepgram(Some(60.0)), Some(1));
    assert_eq!(provider_minutes_deepgram(Some(0.0)), None);
    assert_eq!(provider_minutes_deepgram(None), None);
    assert_eq!(provider_minutes_soniox(Some(61_000.0), Some(1.0)), Some(2));
    // Missing or zero duration falls back to the largest token end.
    assert_eq!(provider_minutes_soniox(None, Some(125_000.0)), Some(3));
    assert_eq!(provider_minutes_soniox(Some(0.0), Some(59_000.0)), Some(1));
    assert_eq!(provider_minutes_soniox(Some(0.0), None), None);
    assert_eq!(provider_minutes_soniox(None, Some(0.0)), None);
}

#[test]
fn bil_034_036_deferred_charge_is_strict_and_a_failure_is_only_reported() {
    let open = vec![paid(10)];
    assert!(decide_deferred(10, &open).is_ok());
    // The caller logs this and still delivers the transcript (BIL-036): the
    // decision itself only reports the refusal.
    assert_eq!(decide_deferred(11, &open), Err(StartRefusal::NotEnoughBalance { required: 11, available: 10 }));
    assert_eq!(decide_deferred(5, &[]), Err(StartRefusal::NoAvailablePackage));
}

#[test]
fn fre_019_the_deferred_charge_never_uses_the_first_job_bypass() {
    // Freemium-only with no completed job: a start would bypass, a callback does not.
    let open = vec![free(0)];
    assert!(matches!(decide_job_start(&job(1, Some(36), 0), &open), Ok(StartCharge::Bypass(_))));
    assert_eq!(decide_deferred(36, &open), Err(StartRefusal::NotEnoughBalance { required: 36, available: 20 }));
}

#[test]
fn bil_037_a_shorter_provider_length_only_lowers_the_duration() {
    assert_eq!(duration_after_provider(10, 7), Some(7));
    assert_eq!(duration_after_provider(10, 10), None);
    assert_eq!(duration_after_provider(10, 12), None, "a longer provider length charges nothing more");
}

#[test]
fn bil_038_web_start_waits_for_every_duration() {
    assert!(web_start_allowed(&[Some(3), Some(4)]));
    assert!(!web_start_allowed(&[Some(3), None]));
    assert!(!web_start_allowed(&[]));
}

#[test]
fn bil_039_040_web_refuses_below_zero_and_api_at_zero_or_less() {
    assert!(web_precheck(Some(0)), "a balance of exactly 0 passes the web check");
    assert!(!web_precheck(Some(-1)));
    assert!(web_precheck(None), "a missing balance counts as 0");
    assert!(!api_precheck(Some(0)));
    assert!(!api_precheck(None));
    assert!(api_precheck(Some(1)));
}

#[test]
fn bil_041_database_messages_map_to_402_in_any_letter_case() {
    for text in [
        "Not enough balance: required=5 min, available=1 min",
        "NO AVAILABLE PACKAGE for user x",
        "Failed to deduct minutes for user x: no packages available or balance insufficient",
    ] {
        assert_eq!(classify_error_message(text), ErrorClass::InsufficientBalance, "{text}");
    }
    assert_eq!(classify_error_message("deadlock detected"), ErrorClass::Internal);
}

#[test]
fn fre_003_freemium_only_means_no_open_package_with_another_name() {
    assert!(freemium_only(&[]), "no open package counts as freemium-only");
    assert!(freemium_only(&[free(0)]));
    assert!(!freemium_only(&[free(0), paid(10)]));
    // An exact-name rule: a gift package named differently is a paid package.
    assert!(!freemium_only(&[pkg(3, Some("freemium-gift"), 5, 0, 6)]));
    // A null name never matches `name != 'freemium'` in SQL, so it does not end freemium.
    assert!(freemium_only(&[pkg(4, None, 5, 0, 6)]));
}

#[test]
fn fre_004_005_first_job_bypasses_and_writes_one_full_row_on_the_earliest_package() {
    let open = vec![free(0)];
    match decide_job_start(&job(1, Some(36), 0), &open).unwrap() {
        StartCharge::Bypass(a) => {
            assert_eq!(a, Allocation { package_id: 1, minutes: 36 });
        }
        other => panic!("{other:?}"),
    }
    // 20 - 36 = -16: the balance may go below zero.
    let mut after = open.clone();
    if let StartCharge::Bypass(a) = decide_job_start(&job(1, Some(36), 0), &open).unwrap() {
        crate::ledger::apply(&mut after, &[a]);
    }
    assert_eq!(crate::ledger::balance(&after), Some(-16));
}

#[test]
fn fre_006_eligibility_is_completed_jobs_so_two_unfinished_first_jobs_both_bypass() {
    let open = vec![free(0)];
    let plan = plan_batch(&[job(1, Some(36), 0), job(2, Some(36), 0)], &open).unwrap();
    assert!(plan.iter().all(|(_, c)| matches!(c, StartCharge::Bypass(_))));
    // Both land on the single package: 20 - 72.
    let total: i64 = plan
        .iter()
        .map(|(_, c)| if let StartCharge::Bypass(a) = c { a.minutes } else { 0 })
        .sum();
    assert_eq!(total, 72);
}

#[test]
fn fre_007_later_freemium_jobs_are_strict() {
    assert_eq!(decide_job_start(&job(1, Some(5), 1), &[]), Err(StartRefusal::NoAvailablePackage));
    assert_eq!(
        decide_job_start(&job(1, Some(11), 1), &[free(10)]),
        Err(StartRefusal::NotEnoughBalance { required: 11, available: 10 })
    );
    assert!(decide_job_start(&job(1, Some(5), 1), &[free(10)]).is_ok());
}

#[test]
fn fre_008_009_a_second_job_that_fits_exactly_passes_and_exhaustion_refuses_everything() {
    let after_first = vec![free(10)];
    assert!(decide_job_start(&job(1, Some(10), 1), &after_first).is_ok(), "exactly the 10 left");
    let exhausted = vec![free(20)];
    for minutes in [5, 13] {
        assert!(decide_job_start(&job(1, Some(minutes), 2), &exhausted).is_err());
    }
}

#[test]
fn fre_010_after_an_overdrawn_first_job_the_next_one_is_refused() {
    let overdrawn = vec![free(36)];
    assert_eq!(
        decide_job_start(&job(1, Some(1), 1), &overdrawn),
        Err(StartRefusal::NotEnoughBalance { required: 1, available: -16 })
    );
    assert!(!web_precheck(Some(-16)), "the app refuses before the database does");
}

#[test]
fn fre_012_an_expired_free_package_refuses_a_first_and_a_later_job() {
    assert_eq!(decide_job_start(&job(1, Some(5), 0), &[]), Err(StartRefusal::NoActivePackage));
    assert_eq!(decide_job_start(&job(1, Some(5), 1), &[]), Err(StartRefusal::NoAvailablePackage));
    assert_eq!(StartRefusal::NoActivePackage.code(), "insufficient_balance");
}

#[test]
fn bil_077_no_fraud_gate_exists_a_tiny_first_job_is_charged_normally() {
    // A freemium-only account starting a 1 minute job is not blocked by any rule.
    assert!(matches!(decide_job_start(&job(1, Some(1), 0), &[free(0)]), Ok(StartCharge::Bypass(_))));
}

#[test]
fn batch_decisions_are_made_in_ascending_subject_order_whatever_the_request_order() {
    let open = vec![pkg(1, Some("PREMIUM"), 50, 0, 6)];
    let forward = plan_batch(&[job(1, Some(30), 2), job(2, Some(30), 2)], &open);
    let reverse = plan_batch(&[job(2, Some(30), 2), job(1, Some(30), 2)], &open);
    assert_eq!(forward, reverse);
    assert_eq!(forward, Err(StartRefusal::NotEnoughBalance { required: 30, available: 20 }));
}

#[test]
fn fre_011_known_duration_freemium_jobs_write_rows_totaling_their_duration_and_unknown_ones_defer() {
    // Known durations: the rows written for each job sum to that job's duration.
    let open = vec![free(0)];
    let rows = |charge: StartCharge| match charge {
        StartCharge::Bypass(a) => vec![a],
        StartCharge::Strict(rows) => rows,
        StartCharge::Deferred => vec![],
    };
    let first = rows(decide_job_start(&job(1, Some(36), 0), &open).unwrap());
    assert_eq!(first.iter().map(|r| r.minutes).sum::<i64>(), 36);
    let later = rows(decide_job_start(&job(2, Some(7), 1), &[free(0)]).unwrap());
    assert_eq!(later.iter().map(|r| r.minutes).sum::<i64>(), 7);
    // This is not universal: a NULL duration writes nothing now, and a deduction
    // error at the callback is reported while the transcript is still delivered.
    assert_eq!(decide_job_start(&job(3, None, 0), &open).unwrap(), StartCharge::Deferred);
    assert!(decide_deferred(100, &open).is_err());
}

#[test]
fn fre_025_and_bil_079_partial_transcription_and_resume_are_not_modeled() {
    // Removed feature, not ported. A job sent back to `not_started` is never billed twice.
    for from in ["completed", "failed", "canceled", "pending", "running"] {
        assert!(!is_billable_transition(from, "not_started"), "{from} is not a start");
    }
    // Charging 36 minutes is one decision for 36 minutes, not a 20-minute slice plus a remainder.
    let charge = decide_job_start(&job(2, Some(36), 1), &[paid(100)]).unwrap();
    assert!(matches!(&charge, StartCharge::Strict(rows) if rows.iter().map(|r| r.minutes).sum::<i64>() == 36));
}
