use super::*;

#[test]
fn bil_045_month_addition_clamps_to_the_last_day_and_keeps_time_of_day() {
    let jan31 = utc(2023, 1, 31, 10, 30, 15, 123_456);
    assert_eq!(add_months(jan31, 1).unwrap(), utc(2023, 2, 28, 10, 30, 15, 123_456));
    // Leap year keeps the 29th.
    let jan31_leap = utc(2024, 1, 31, 0, 0, 0, 0);
    assert_eq!(add_months(jan31_leap, 1).unwrap(), utc(2024, 2, 29, 0, 0, 0, 0));
    assert_eq!(add_months(utc(2023, 3, 31, 1, 2, 3, 4), 1).unwrap(), utc(2023, 4, 30, 1, 2, 3, 4));
}

#[test]
fn year_steps_are_twelve_months_and_leap_day_clamps() {
    let leap_day = utc(2024, 2, 29, 12, 0, 0, 0);
    assert_eq!(add_interval(leap_day, Interval::Year, 1).unwrap(), utc(2025, 2, 28, 12, 0, 0, 0));
    assert_eq!(add_interval(utc(2023, 1, 1, 0, 0, 0, 0), Interval::Year, 1).unwrap(), utc(2024, 1, 1, 0, 0, 0, 0));
}

#[test]
fn december_wraps_into_the_next_year_and_large_counts_work() {
    assert_eq!(add_months(utc(2023, 12, 15, 0, 0, 0, 0), 1).unwrap(), utc(2024, 1, 15, 0, 0, 0, 0));
    assert_eq!(add_months(utc(2023, 1, 1, 0, 0, 0, 0), 25).unwrap(), utc(2025, 2, 1, 0, 0, 0, 0));
}

#[test]
fn microsecond_boundaries_survive_a_month_step() {
    let at = utc(2023, 8, 31, 23, 59, 59, 999_999);
    let next = add_months(at, 2).unwrap();
    assert_eq!(next, utc(2023, 10, 31, 23, 59, 59, 999_999));
    assert_eq!(next.0 - utc(2023, 10, 31, 23, 59, 59, 999_998).0, 1);
}

#[test]
fn dates_before_the_epoch_and_formatting() {
    assert_eq!(format_dmy(utc(2024, 3, 5, 23, 59, 59, 0)), "05/03/2024");
    assert_eq!(format_dmy(utc(1969, 12, 31, 23, 0, 0, 0)), "31/12/1969");
}

#[test]
fn interval_text_is_month_or_year_only() {
    assert_eq!(Interval::parse("month"), Some(Interval::Month));
    assert_eq!(Interval::parse("year"), Some(Interval::Year));
    assert_eq!(Interval::parse("week"), None);
    assert_eq!(Interval::parse("Month"), None);
}

#[test]
fn rfc3339_timestamps_parse_with_fractions_and_offsets_into_one_instant() {
    let expected = utc(2026, 1, 15, 12, 30, 45, 0);
    assert_eq!(parse_rfc3339("2026-01-15T12:30:45Z"), Some(expected));
    assert_eq!(parse_rfc3339("2026-01-15T12:30:45z"), Some(expected));
    assert_eq!(parse_rfc3339("2026-01-15T12:30:45.123456Z"), Some(utc(2026, 1, 15, 12, 30, 45, 123_456)));
    // More than microsecond precision is truncated, never rounded up across a second.
    assert_eq!(parse_rfc3339("2026-01-15T12:30:45.999999999Z"), Some(utc(2026, 1, 15, 12, 30, 45, 999_999)));
    assert_eq!(parse_rfc3339("2026-01-15T15:30:45+03:00"), Some(expected));
    assert_eq!(parse_rfc3339("2026-01-15T07:00:45-05:30"), Some(expected));
    assert_eq!(parse_rfc3339("2026-01-15T12:30:45.5+00:00"), Some(utc(2026, 1, 15, 12, 30, 45, 500_000)));
    // An offset can move the instant across a date boundary.
    assert_eq!(parse_rfc3339("2026-03-01T01:00:00+03:00"), Some(utc(2026, 2, 28, 22, 0, 0, 0)));
}

#[test]
fn rfc3339_rejects_anything_that_is_not_an_exact_timestamp() {
    for bad in [
        "",
        "garbage",
        "2026-13-01T00:00:00Z",
        "2026-02-30T00:00:00Z",
        "2026-01-15 12:30:45Z",
        "2026-01-15T12:30:45",
        "2026-01-15T24:00:00Z",
        "2026-01-15T12:60:00Z",
        "2026-01-15T12:30:60Z",
        "2026-01-15T12:30:45.Z",
        "2026-01-15T12:30:45+3:00",
        "2026-01-15T12:30:45+03:60",
        "2026-01-15T12:30:45Z extra",
        "26-01-15T12:30:45Z",
    ] {
        assert_eq!(parse_rfc3339(bad), None, "{bad:?}");
    }
}

#[test]
fn rfc3339_output_is_utc_with_microseconds_and_parses_back_to_the_same_instant() {
    let at = utc(2026, 1, 5, 3, 4, 5, 60_007);
    assert_eq!(format_rfc3339(at), "2026-01-05T03:04:05.060007Z");
    assert_eq!(parse_rfc3339(&format_rfc3339(at)), Some(at));
    let epoch_before = utc(1969, 12, 31, 23, 59, 59, 999_999);
    assert_eq!(parse_rfc3339(&format_rfc3339(epoch_before)), Some(epoch_before));
}
