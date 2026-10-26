use super::*;

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

#[test]
fn parses_and_prints_without_changing_scale_or_digits() {
    // Historical amounts are preserved verbatim, including binary-float artifacts.
    assert_eq!(d("393.35999999999996").to_string(), "393.35999999999996");
    assert_eq!(d("616").to_string(), "616");
    assert_eq!(d("40.2300").to_string(), "40.2300");
    assert_eq!(d("-0.5").to_string(), "-0.5");
    assert_eq!(d("0.05").to_string(), "0.05");
}

#[test]
fn equality_is_by_value_not_by_scale() {
    assert_eq!(d("616"), d("616.00"));
    assert!(d("1.49") < d("1.5"));
    assert!(d("-1") < d("0"));
}

#[test]
fn float4_shortest_text_with_an_exponent_is_read_exactly() {
    // A coupon stored as real can be printed by Postgres as 1e-05.
    assert_eq!(d("1e-05"), d("0.00001"));
    assert_eq!(d("2.5E1"), d("25"));
    assert_eq!(d("0.25"), Decimal::new(25, 2));
}

#[test]
fn rejects_malformed_text() {
    for bad in ["", "-", ".", "1.", ".5", "1..2", "abc", "1e", "1e+", "1 2", "+"] {
        assert!(bad.parse::<Decimal>().is_err(), "{bad:?} must not parse");
    }
}

#[test]
fn quantize_half_up_matches_math_round_for_positive_and_negative_ties() {
    assert_eq!(d("0.125").quantize(2, Rounding::HalfUp).unwrap().to_string(), "0.13");
    assert_eq!(d("0.124").quantize(2, Rounding::HalfUp).unwrap().to_string(), "0.12");
    // Math.round rounds ties toward positive infinity.
    assert_eq!(d("-0.125").quantize(2, Rounding::HalfUp).unwrap().to_string(), "-0.12");
    assert_eq!(d("2.5").quantize(0, Rounding::HalfUp).unwrap().to_string(), "3");
}

#[test]
fn quantize_floor_and_widening_scale() {
    assert_eq!(d("40.239").quantize(2, Rounding::Floor).unwrap().to_string(), "40.23");
    assert_eq!(d("-0.001").quantize(2, Rounding::Floor).unwrap().to_string(), "-0.01");
    assert_eq!(d("5").quantize(2, Rounding::HalfUp).unwrap().to_string(), "5.00");
}

#[test]
fn multiplication_is_exact() {
    // 6 hours at 1.49 is 8.94 exactly, times 44 is 393.36 with no float tail.
    let usd = d("8.94");
    let try_amount = usd.mul(&d("44")).unwrap();
    assert_eq!(try_amount.to_string(), "393.36");
    assert_eq!(d("0.1").mul(&d("0.2")).unwrap(), d("0.02"));
}

#[test]
fn division_quantizes_once() {
    // TRY 1770.12 at rate 44 is exactly 40.23 USD.
    assert_eq!(d("1770.12").div(&d("44"), 2, Rounding::HalfUp).unwrap().to_string(), "40.23");
    assert!(d("1").div(&d("0"), 2, Rounding::HalfUp).is_err());
}

#[test]
fn overflow_is_an_error_not_a_wrap() {
    let big = Decimal::new(i128::MAX / 2, 0);
    assert!(big.mul(&Decimal::new(3, 0)).is_err());
    assert!(Decimal::new(1, 0).quantize(60, Rounding::Floor).is_err());
}

#[test]
fn minor_units_need_scale_two_or_less() {
    assert_eq!(d("14.99").to_minor_units(), Some(1499));
    assert_eq!(d("14.9").to_minor_units(), Some(1490));
    assert_eq!(d("14").to_minor_units(), Some(1400));
    assert_eq!(d("14.999").to_minor_units(), None);
}
