use super::*;
use crate::calendar::utc;
use uuid::Uuid;

fn account(n: u128) -> AccountId {
    AccountId(Uuid::from_u128(n))
}

fn pb(id: i64, owner: AccountId, included: i64, consumed: i64, start: Micros, end: Micros) -> PackageBalance {
    PackageBalance {
        package: Package {
            id,
            account: owner,
            payment_id: None,
            name: Some("PREMIUM".into()),
            minutes_included: included,
            start_at: start,
            end_at: end,
        },
        consumed,
    }
}

/// The captured Python breakdown fixture: two packages, 60 minutes with 15
/// used, so 45 left each; the first ends 2023-08-31, the second 2023-10-31.
fn captured_pair() -> Vec<PackageBalance> {
    let a = account(1);
    vec![
        pb(1, a, 60, 15, utc(2023, 7, 31, 0, 0, 0, 0), utc(2023, 8, 31, 0, 0, 0, 0)),
        pb(2, a, 60, 15, utc(2023, 8, 31, 0, 0, 0, 0), utc(2023, 10, 31, 0, 0, 0, 0)),
    ]
}

fn captured_at() -> Micros {
    utc(2023, 8, 31, 0, 0, 0, 0)
}

fn rows(request: i64) -> Vec<(i64, i64, i64)> {
    let open = open_at(&captured_pair(), captured_at());
    breakdown(request, &open)
        .into_iter()
        .map(|b| (b.package_id, b.request_minutes, b.carry_over_minutes))
        .collect()
}

#[test]
fn bil_001_002_003_remaining_is_included_minus_the_sum_of_usage() {
    let p = pb(7, account(1), 60, 0, utc(2023, 1, 1, 0, 0, 0, 0), utc(2023, 2, 1, 0, 0, 0, 0));
    assert_eq!(p.remaining(), 60, "a package without usage rows has used nothing");
    let used = pb(7, account(1), 60, 15, utc(2023, 1, 1, 0, 0, 0, 0), utc(2023, 2, 1, 0, 0, 0, 0));
    assert_eq!(used.remaining(), 45);
}

#[test]
fn bil_004_007_balance_is_the_sum_of_remaining_over_open_packages() {
    let a = account(1);
    let all = vec![
        pb(1, a, 60, 0, utc(2023, 1, 1, 0, 0, 0, 0), utc(2023, 3, 1, 0, 0, 0, 0)),
        pb(2, a, 60, 0, utc(2023, 1, 1, 0, 0, 0, 0), utc(2023, 3, 1, 0, 0, 0, 0)),
    ];
    let open = open_at(&all, utc(2023, 2, 1, 0, 0, 0, 0));
    assert_eq!(balance(&open), Some(120), "two open packages of 60 give 120");
}

#[test]
fn bil_005_both_window_ends_are_inclusive_to_the_microsecond() {
    let a = account(1);
    let start = utc(2023, 1, 1, 0, 0, 0, 0);
    let end = utc(2023, 2, 1, 0, 0, 0, 0);
    let all = vec![pb(1, a, 10, 0, start, end)];
    assert_eq!(open_at(&all, start).len(), 1, "start instant is open");
    assert_eq!(open_at(&all, end).len(), 1, "end instant is open");
    assert!(open_at(&all, Micros(start.0 - 1)).is_empty(), "one microsecond before the start is closed");
    assert!(open_at(&all, Micros(end.0 + 1)).is_empty(), "one microsecond after the end is closed");
}

#[test]
fn bil_005_adjacent_yearly_windows_share_a_boundary_and_add_up_there() {
    let a = account(1);
    let boundary = utc(2023, 2, 1, 0, 0, 0, 0);
    let all = vec![
        pb(1, a, 1200, 0, utc(2023, 1, 1, 0, 0, 0, 0), boundary),
        pb(2, a, 1200, 0, boundary, utc(2023, 3, 1, 0, 0, 0, 0)),
    ];
    assert_eq!(balance(&open_at(&all, boundary)), Some(2400));
}

#[test]
fn bil_006_expired_and_future_packages_add_nothing() {
    let a = account(1);
    let all = vec![
        pb(1, a, 60, 0, utc(2022, 1, 1, 0, 0, 0, 0), utc(2022, 2, 1, 0, 0, 0, 0)),
        pb(2, a, 60, 0, utc(2024, 1, 1, 0, 0, 0, 0), utc(2024, 2, 1, 0, 0, 0, 0)),
    ];
    assert_eq!(balance(&open_at(&all, utc(2023, 1, 1, 0, 0, 0, 0))), None);
}

#[test]
fn bil_008_no_open_package_has_no_balance_row_and_display_shows_zero() {
    assert_eq!(balance(&[]), None);
    assert_eq!(balance_or_zero(&[]), 0);
}

#[test]
fn bil_009_balance_goes_negative_only_through_the_bypass() {
    let a = account(1);
    let open = vec![pb(1, a, 20, 0, utc(2023, 1, 1, 0, 0, 0, 0), utc(2023, 3, 1, 0, 0, 0, 0))];
    // A strict debit of 36 on a balance of 20 is refused.
    assert!(matches!(plan_strict(36, &open), Err(DebitRefusal::InsufficientBalance { .. })));
    // The bypass writes the whole 36 on the only package: 20 - 36 = -16.
    let allocation = plan_bypass(36, &open).unwrap();
    let mut after = open.clone();
    apply(&mut after, &[allocation]);
    assert_eq!(balance(&after), Some(-16));
}

#[test]
fn bil_010_files_round_up_to_whole_minutes_and_batches_sum_rounded_files() {
    assert_eq!(minutes_from_seconds(61.0), 2, "61 seconds costs 2 minutes");
    assert_eq!(minutes_from_seconds(60.0), 1);
    assert_eq!(minutes_from_seconds(0.2), 1);
    assert_eq!(minutes_from_seconds(0.0), 0);
    assert_eq!(batch_minutes(&[6.0; 10]), 10, "ten 6 second files cost 10 minutes");
}

#[test]
fn bil_011_012_allocation_takes_from_the_package_that_ends_first() {
    let a = account(1);
    // Listed out of expiry order on purpose.
    let open = vec![
        pb(2, a, 30, 0, utc(2023, 1, 1, 0, 0, 0, 0), utc(2023, 6, 1, 0, 0, 0, 0)),
        pb(1, a, 50, 0, utc(2023, 1, 1, 0, 0, 0, 0), utc(2023, 5, 1, 0, 0, 0, 0)),
    ];
    let taken = plan_strict(60, &open).unwrap();
    assert_eq!(taken, vec![Allocation { package_id: 1, minutes: 50 }, Allocation { package_id: 2, minutes: 10 }]);
}

#[test]
fn bil_013_breakdown_request_10() {
    assert_eq!(rows(10), vec![(1, 10, 0), (2, 0, 0)]);
}

#[test]
fn bil_014_breakdown_request_45() {
    assert_eq!(rows(45), vec![(1, 45, 0), (2, 0, 0)]);
}

#[test]
fn bil_015_breakdown_request_60() {
    assert_eq!(rows(60), vec![(1, 45, -15), (2, 15, 0)]);
}

#[test]
fn bil_016_breakdown_request_90() {
    assert_eq!(rows(90), vec![(1, 45, -45), (2, 45, 0)]);
}

#[test]
fn bil_017_breakdown_request_100_shortfall_shows_only_as_carry_and_is_refused() {
    assert_eq!(rows(100), vec![(1, 45, -55), (2, 45, -10)]);
    let open = open_at(&captured_pair(), captured_at());
    assert_eq!(
        plan_strict(100, &open),
        Err(DebitRefusal::InsufficientBalance { balance: 90, requested: 100 })
    );
}

#[test]
fn bil_018_breakdown_with_no_open_package_returns_no_rows() {
    let open = open_at(&captured_pair(), utc(2023, 12, 31, 0, 0, 0, 0));
    assert!(breakdown(10, &open).is_empty());
}

#[test]
fn bil_019_a_debit_never_touches_another_accounts_packages() {
    let (a, b) = (account(1), account(2));
    let all = vec![
        pb(1, a, 60, 0, utc(2023, 1, 1, 0, 0, 0, 0), utc(2023, 6, 1, 0, 0, 0, 0)),
        pb(2, b, 60, 0, utc(2023, 1, 1, 0, 0, 0, 0), utc(2023, 2, 1, 0, 0, 0, 0)),
    ];
    let mine = for_account(&all, a);
    let open = open_at(&mine, utc(2023, 1, 15, 0, 0, 0, 0));
    // B's package ends first, but it is not A's to spend.
    let taken = plan_strict(10, &open).unwrap();
    assert_eq!(taken, vec![Allocation { package_id: 1, minutes: 10 }]);
}

#[test]
fn bil_020_insufficient_balance_returns_the_negative_number_and_plans_no_write() {
    let open = open_at(&captured_pair(), captured_at());
    let refusal = plan_strict(91, &open).unwrap_err();
    assert_eq!(refusal, DebitRefusal::InsufficientBalance { balance: 90, requested: 91 });
    assert_eq!(refusal.balance_after(), Some(-1));
}

#[test]
fn bil_021_exact_fit_leaves_exactly_zero_not_zero_to_five() {
    let mut open = open_at(&captured_pair(), captured_at());
    let taken = plan_strict(90, &open).unwrap();
    assert_eq!(taken.iter().map(|t| t.minutes).sum::<i64>(), 90);
    apply(&mut open, &taken);
    assert_eq!(balance(&open), Some(0));
}

#[test]
fn bil_022_zero_minutes_or_no_package_in_the_normal_path_raises_nothing_deducted() {
    let open = open_at(&captured_pair(), captured_at());
    assert_eq!(plan_strict(0, &open), Err(DebitRefusal::NothingDeducted));
    // With no open package the balance check is skipped (NULL is not below zero) and the debit fails to write.
    assert_eq!(plan_strict(5, &[]), Err(DebitRefusal::NothingDeducted));
    assert_eq!(plan_strict(0, &[]), Err(DebitRefusal::NothingDeducted));
}

#[test]
fn bil_023_one_function_covers_both_modes_with_five_named_inputs() {
    // The legacy five arguments: account, minutes, subject, time, skip check.
    let open = open_at(&captured_pair(), captured_at());
    let strict = plan_debit(&DebitRequest { minutes: 10, skip_balance_check: false }, &open).unwrap();
    let bypass = plan_debit(&DebitRequest { minutes: 500, skip_balance_check: true }, &open).unwrap();
    assert_eq!(strict, vec![Allocation { package_id: 1, minutes: 10 }]);
    assert_eq!(bypass, vec![Allocation { package_id: 1, minutes: 500 }]);
}

#[test]
fn bil_024_bypass_writes_the_full_amount_on_the_earliest_ending_open_package() {
    let open = open_at(&captured_pair(), captured_at());
    assert_eq!(plan_bypass(120, &open).unwrap(), Allocation { package_id: 1, minutes: 120 });
    assert_eq!(plan_bypass(5, &[]), Err(DebitRefusal::NoActivePackage));
    // A zero minute bypass is a real, preserved write.
    assert_eq!(plan_bypass(0, &open).unwrap(), Allocation { package_id: 1, minutes: 0 });
}

#[test]
fn def_016_equal_expiry_allocates_exactly_the_request_in_package_id_order() {
    let a = account(1);
    let end = utc(2023, 6, 1, 0, 0, 0, 0);
    let start = utc(2023, 1, 1, 0, 0, 0, 0);
    // Listed in reverse id order; both end at the same instant.
    let open = vec![pb(9, a, 10, 0, start, end), pb(4, a, 10, 0, start, end)];
    assert_eq!(plan_strict(5, &open).unwrap(), vec![Allocation { package_id: 4, minutes: 5 }]);
    assert_eq!(
        plan_strict(15, &open).unwrap(),
        vec![Allocation { package_id: 4, minutes: 10 }, Allocation { package_id: 9, minutes: 5 }]
    );
    assert_eq!(plan_strict(20, &open).unwrap().iter().map(|t| t.minutes).sum::<i64>(), 20);
}

#[test]
fn d1_a_negative_prefix_cannot_overcharge_a_later_package() {
    let a = account(1);
    let start = utc(2023, 1, 1, 0, 0, 0, 0);
    // Earlier package is overdrawn by 16, the later one has 100: balance 84.
    let mut open = vec![
        pb(1, a, 20, 36, start, utc(2023, 3, 1, 0, 0, 0, 0)),
        pb(2, a, 100, 0, start, utc(2023, 6, 1, 0, 0, 0, 0)),
    ];
    assert_eq!(balance(&open), Some(84));
    let taken = plan_strict(10, &open).unwrap();
    // The legacy prefix formula wrote 26 here and left 58 while returning 74.
    assert_eq!(taken, vec![Allocation { package_id: 2, minutes: 10 }]);
    apply(&mut open, &taken);
    assert_eq!(balance(&open), Some(74), "persisted balance equals the derived outcome");
}

#[test]
fn d1_reversed_expiry_zero_and_negative_capacities_mixed_with_positive_ones() {
    let a = account(1);
    let start = utc(2023, 1, 1, 0, 0, 0, 0);
    // Positive package ends first; the overdrawn one ends later.
    let open = vec![
        pb(1, a, 30, 0, start, utc(2023, 3, 1, 0, 0, 0, 0)),
        pb(2, a, 10, 25, start, utc(2023, 4, 1, 0, 0, 0, 0)),
        pb(3, a, 20, 20, start, utc(2023, 5, 1, 0, 0, 0, 0)),
        pb(4, a, 40, 0, start, utc(2023, 6, 1, 0, 0, 0, 0)),
    ];
    assert_eq!(balance(&open), Some(55));
    let taken = plan_strict(35, &open).unwrap();
    assert_eq!(
        taken,
        vec![Allocation { package_id: 1, minutes: 30 }, Allocation { package_id: 4, minutes: 5 }],
        "negative and exhausted packages are skipped, not drained twice"
    );
    // Admission uses the signed aggregate: 55 available, 56 is refused without writes.
    assert_eq!(
        plan_strict(56, &open),
        Err(DebitRefusal::InsufficientBalance { balance: 55, requested: 56 })
    );
}

#[test]
fn d1_conservation_over_many_mixed_capacities_written_minutes_equal_the_request() {
    let a = account(1);
    let start = utc(2023, 1, 1, 0, 0, 0, 0);
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..500 {
        let count = 1 + (next() % 6) as i64;
        let mut open = Vec::new();
        for id in 1..=count {
            let included = (next() % 120) as i64;
            let consumed = (next() % 160) as i64; // may overdraw
            let end = utc(2023, 1 + (next() % 11) as u8, 1 + (next() % 27) as u8, 0, 0, 0, 0);
            open.push(pb(id, a, included, consumed, start, end));
        }
        let available = balance(&open).unwrap();
        let request = (next() % 200) as i64;
        match plan_strict(request, &open) {
            Ok(taken) => {
                assert_eq!(taken.iter().map(|t| t.minutes).sum::<i64>(), request);
                let mut after = open.clone();
                apply(&mut after, &taken);
                assert_eq!(balance(&after), Some(available - request));
                assert!(taken.iter().all(|t| t.minutes > 0));
            }
            Err(DebitRefusal::InsufficientBalance { balance, requested }) => {
                assert_eq!((balance, requested), (available, request));
                assert!(available < request);
            }
            Err(DebitRefusal::NothingDeducted) => assert_eq!(request, 0),
            Err(other) => panic!("unexpected refusal {other:?}"),
        }
    }
}

#[test]
fn d1_negative_balances_stay_effective_until_their_package_expires() {
    let a = account(1);
    let start = utc(2023, 1, 1, 0, 0, 0, 0);
    let all = vec![
        pb(1, a, 20, 36, start, utc(2023, 3, 1, 0, 0, 0, 0)),
        pb(2, a, 100, 10, start, utc(2023, 6, 1, 0, 0, 0, 0)),
    ];
    let before = open_at(&all, utc(2023, 2, 1, 0, 0, 0, 0));
    assert_eq!(balance(&before), Some(74));
    let after_expiry = open_at(&all, Micros(utc(2023, 3, 1, 0, 0, 0, 0).0 + 1));
    assert_eq!(balance(&after_expiry), Some(90), "recorded outcome change: 74 before, 90 after the overdrawn package lapses");
}

#[test]
fn a_negative_request_is_invalid_not_a_credit() {
    let open = open_at(&captured_pair(), captured_at());
    assert_eq!(plan_strict(-5, &open), Err(DebitRefusal::InvalidMinutes));
    assert_eq!(plan_bypass(-5, &open), Err(DebitRefusal::InvalidMinutes));
}
