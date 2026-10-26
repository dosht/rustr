use super::*;
use crate::calendar::utc;

fn spec(name: &str, minutes: i64, count: u32, interval: &str, frequency: u32, expiry: u32) -> PackageSpec {
    PackageSpec { name: name.into(), minutes, count, interval: interval.into(), frequency, expiry }
}

#[test]
fn bil_046_monthly_example_is_one_package_for_two_months() {
    let granted = expand_grant(utc(2023, 1, 1, 0, 0, 0, 0), &spec("monthly", 20, 1, "month", 1, 2)).unwrap();
    assert_eq!(granted.len(), 1);
    assert_eq!(granted[0].start_at, utc(2023, 1, 1, 0, 0, 0, 0));
    assert_eq!(granted[0].end_at, utc(2023, 3, 1, 0, 0, 0, 0));
    assert_eq!((granted[0].name.as_str(), granted[0].minutes, granted[0].sequence), ("monthly", 20, 1));
}

#[test]
fn bil_047_pay_as_you_go_example_is_one_year() {
    let granted = expand_grant(utc(2023, 1, 1, 0, 0, 0, 0), &spec("pay-as-you-go", 20, 1, "year", 1, 1)).unwrap();
    assert_eq!(granted.len(), 1);
    assert_eq!(granted[0].start_at, utc(2023, 1, 1, 0, 0, 0, 0));
    assert_eq!(granted[0].end_at, utc(2024, 1, 1, 0, 0, 0, 0));
}

#[test]
fn bil_044_048_049_yearly_example_is_twelve_packages_one_month_apart_with_one_name() {
    let granted = expand_grant(utc(2023, 1, 1, 0, 0, 0, 0), &spec("yearly", 20, 12, "month", 1, 2)).unwrap();
    assert_eq!(granted.len(), 12);
    for (i, g) in granted.iter().enumerate() {
        let month = (i + 1) as u8;
        assert_eq!(g.sequence, i as i32 + 1);
        assert_eq!(g.start_at, utc(2023, month, 1, 0, 0, 0, 0), "package {} start", i + 1);
        let end_month_index = i as i64 + 1 + 2; // two months after its own start
        let (year, m) = (2023 + ((end_month_index - 1) / 12) as i32, ((end_month_index - 1) % 12 + 1) as u8);
        assert_eq!(g.end_at, utc(year, m, 1, 0, 0, 0, 0), "package {} end", i + 1);
        assert_eq!((g.name.as_str(), g.minutes), ("yearly", 20), "all packages of one payment share name and minutes");
    }
    assert_eq!(granted[0].end_at, utc(2023, 3, 1, 0, 0, 0, 0));
    assert_eq!(granted[11].end_at, utc(2024, 2, 1, 0, 0, 0, 0));
}

#[test]
fn bil_045_frequency_steps_are_added_one_at_a_time_from_the_previous_start() {
    // Created on the 31st: starts are 31 Jan, 28 Feb, 28 Mar, 28 Apr ... (DEF-014 kept).
    let granted = expand_grant(utc(2023, 1, 31, 0, 0, 0, 0), &spec("m", 1, 4, "month", 1, 1)).unwrap();
    let starts: Vec<Micros> = granted.iter().map(|g| g.start_at).collect();
    assert_eq!(
        starts,
        vec![
            utc(2023, 1, 31, 0, 0, 0, 0),
            utc(2023, 2, 28, 0, 0, 0, 0),
            utc(2023, 3, 28, 0, 0, 0, 0),
            utc(2023, 4, 28, 0, 0, 0, 0)
        ]
    );
    // Each end is its own start plus one month, clamped on its own.
    assert_eq!(granted[0].end_at, utc(2023, 2, 28, 0, 0, 0, 0));
    // A leap year keeps the 29th on the second step.
    let leap = expand_grant(utc(2024, 1, 31, 0, 0, 0, 0), &spec("m", 1, 3, "month", 1, 1)).unwrap();
    assert_eq!(leap[1].start_at, utc(2024, 2, 29, 0, 0, 0, 0));
    assert_eq!(leap[2].start_at, utc(2024, 3, 29, 0, 0, 0, 0));
}

#[test]
fn bil_045_frequency_greater_than_one_and_year_intervals() {
    let granted = expand_grant(utc(2023, 1, 1, 0, 0, 0, 0), &spec("q", 5, 3, "month", 3, 3)).unwrap();
    assert_eq!(granted[1].start_at, utc(2023, 4, 1, 0, 0, 0, 0));
    assert_eq!(granted[2].end_at, utc(2023, 10, 1, 0, 0, 0, 0));
}

#[test]
fn bil_045_any_other_interval_raises_and_names_it() {
    let err = expand_grant(utc(2023, 1, 1, 0, 0, 0, 0), &spec("x", 5, 1, "week", 1, 1)).unwrap_err();
    assert_eq!(err, GrantError::InvalidInterval("week".into()));
    assert_eq!(err.to_string(), "Invalid duration interval (week)");
}

#[test]
fn bil_055_a_renewal_expands_from_the_renewal_row_time() {
    let renewal = expand_grant(utc(2023, 6, 15, 8, 0, 0, 0), &spec("PREMIUM", 1200, 12, "month", 1, 1)).unwrap();
    assert_eq!(renewal.len(), 12);
    assert_eq!(renewal[0].start_at, utc(2023, 6, 15, 8, 0, 0, 0));
    let monthly = expand_grant(utc(2023, 6, 15, 8, 0, 0, 0), &spec("PREMIUM", 1200, 1, "month", 1, 1)).unwrap();
    assert_eq!(monthly.len(), 1);
}

#[test]
fn bil_043_a_package_set_is_granted_on_pending_or_failed_to_completed_only() {
    use PaymentStatus::*;
    assert_eq!(transition(Pending, Observation::Completed), Transition::Apply { to: Completed, grants: true });
    // DEF-004: failed to completed also grants, exactly once.
    assert_eq!(transition(Failed, Observation::Completed), Transition::Apply { to: Completed, grants: true });
    assert_eq!(transition(Completed, Observation::Completed), Transition::Ignore);
    // The source only refuses completed and same-status rows: a canceled row can still complete, once.
    assert_eq!(transition(Canceled, Observation::Completed), Transition::Apply { to: Completed, grants: true });
}

#[test]
fn bil_058_a_late_failure_never_undoes_a_completion_and_repeats_change_nothing() {
    use PaymentStatus::*;
    assert_eq!(transition(Completed, Observation::Failed), Transition::Ignore);
    assert_eq!(transition(Failed, Observation::Failed), Transition::Ignore);
    assert_eq!(transition(Pending, Observation::Failed), Transition::Apply { to: Failed, grants: false });
    assert_eq!(transition(Canceled, Observation::Failed), Transition::Apply { to: Failed, grants: false });
}

#[test]
fn bil_078_canceled_is_a_known_status_that_no_use_case_produces() {
    assert_eq!(PaymentStatus::parse("canceled"), Some(PaymentStatus::Canceled));
    assert_eq!(PaymentStatus::parse("completed"), Some(PaymentStatus::Completed));
    assert_eq!(PaymentStatus::parse("weird"), None);
    // Applying either observation to a pending payment never yields Canceled.
    for observation in [Observation::Completed, Observation::Failed] {
        assert!(!matches!(
            transition(PaymentStatus::Pending, observation),
            Transition::Apply { to: PaymentStatus::Canceled, .. }
        ));
    }
}

#[test]
fn package_spec_round_trips_the_legacy_metadata_block() {
    let json = r#"{"name":"yearly","minutes":20,"count":12,"interval":"month","frequency":1,"expiry":2}"#;
    let parsed: PackageSpec = serde_json::from_str(json).unwrap();
    assert_eq!(parsed, spec("yearly", 20, 12, "month", 1, 2));
    assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
}
