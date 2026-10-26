//! Exact base-10 numbers for money, rates and coupon fractions.
//!
//! Legacy checkout arithmetic ran in binary floating point and produced prices
//! such as 393.35999999999996. Quotes now use this type so each documented
//! rounding stage happens once, on exact values. Historical stored amounts are
//! read and printed digit for digit and are never rewritten.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "string"))]
pub struct Decimal {
    units: i128,
    scale: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    /// Ties go toward positive infinity, exactly like JavaScript `Math.round`.
    HalfUp,
    /// Toward negative infinity, like `Math.floor`.
    Floor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecimalError {
    Malformed(String),
    Overflow,
    DivisionByZero,
}

impl fmt::Display for DecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecimalError::Malformed(s) => write!(f, "not a decimal number: {s:?}"),
            DecimalError::Overflow => write!(f, "decimal arithmetic overflow"),
            DecimalError::DivisionByZero => write!(f, "division by zero"),
        }
    }
}

impl std::error::Error for DecimalError {}

const MAX_SCALE: u32 = 38;

fn pow10(exp: u32) -> Result<i128, DecimalError> {
    if exp > MAX_SCALE {
        return Err(DecimalError::Overflow);
    }
    Ok(10i128.pow(exp))
}

impl Decimal {
    pub const fn new(units: i128, scale: u32) -> Decimal {
        Decimal { units, scale }
    }

    pub const fn zero() -> Decimal {
        Decimal { units: 0, scale: 0 }
    }

    pub fn units(&self) -> i128 {
        self.units
    }

    pub fn scale(&self) -> u32 {
        self.scale
    }

    pub fn is_zero(&self) -> bool {
        self.units == 0
    }

    pub fn is_negative(&self) -> bool {
        self.units < 0
    }

    pub fn parse(text: &str) -> Result<Decimal, DecimalError> {
        let bad = || DecimalError::Malformed(text.to_string());
        let (negative, rest) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (mantissa, exponent) = match rest.find(['e', 'E']) {
            Some(at) => {
                let exp: i32 = rest[at + 1..].parse().map_err(|_| bad())?;
                (&rest[..at], exp)
            }
            None => (rest, 0),
        };
        let (whole, fraction) = match mantissa.split_once('.') {
            Some((w, f)) => (w, f),
            None => (mantissa, ""),
        };
        let has_dot = mantissa.contains('.');
        if whole.is_empty()
            || (has_dot && fraction.is_empty())
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(bad());
        }
        let digits = format!("{whole}{fraction}");
        let mut units: i128 = digits.parse().map_err(|_| DecimalError::Overflow)?;
        let mut scale = fraction.len() as i64 - exponent as i64;
        if scale < 0 {
            units = units
                .checked_mul(pow10((-scale) as u32)?)
                .ok_or(DecimalError::Overflow)?;
            scale = 0;
        }
        if scale > MAX_SCALE as i64 {
            return Err(DecimalError::Overflow);
        }
        Ok(Decimal { units: if negative { -units } else { units }, scale: scale as u32 })
    }

    /// Both values at the larger scale, for comparison and subtraction.
    fn aligned(&self, other: &Decimal) -> Result<(i128, i128, u32), DecimalError> {
        let scale = self.scale.max(other.scale);
        let a = self
            .units
            .checked_mul(pow10(scale - self.scale)?)
            .ok_or(DecimalError::Overflow)?;
        let b = other
            .units
            .checked_mul(pow10(scale - other.scale)?)
            .ok_or(DecimalError::Overflow)?;
        Ok((a, b, scale))
    }

    pub fn cmp_value(&self, other: &Decimal) -> Ordering {
        match self.aligned(other) {
            Ok((a, b, _)) => a.cmp(&b),
            // Alignment only overflows for absurd magnitudes; fall back to sign.
            Err(_) => self.units.signum().cmp(&other.units.signum()),
        }
    }

    pub fn sub(&self, other: &Decimal) -> Result<Decimal, DecimalError> {
        let (a, b, scale) = self.aligned(other)?;
        Ok(Decimal { units: a.checked_sub(b).ok_or(DecimalError::Overflow)?, scale })
    }

    pub fn add(&self, other: &Decimal) -> Result<Decimal, DecimalError> {
        let (a, b, scale) = self.aligned(other)?;
        Ok(Decimal { units: a.checked_add(b).ok_or(DecimalError::Overflow)?, scale })
    }

    pub fn mul(&self, other: &Decimal) -> Result<Decimal, DecimalError> {
        let units = self.units.checked_mul(other.units).ok_or(DecimalError::Overflow)?;
        let scale = self.scale + other.scale;
        if scale > MAX_SCALE {
            return Err(DecimalError::Overflow);
        }
        Ok(Decimal { units, scale })
    }

    pub fn mul_int(&self, factor: i128) -> Result<Decimal, DecimalError> {
        Ok(Decimal {
            units: self.units.checked_mul(factor).ok_or(DecimalError::Overflow)?,
            scale: self.scale,
        })
    }

    /// Rescales to `scale` decimals. Narrowing rounds once with `mode`.
    pub fn quantize(&self, scale: u32, mode: Rounding) -> Result<Decimal, DecimalError> {
        if scale > MAX_SCALE {
            return Err(DecimalError::Overflow);
        }
        if scale >= self.scale {
            let units = self
                .units
                .checked_mul(pow10(scale - self.scale)?)
                .ok_or(DecimalError::Overflow)?;
            return Ok(Decimal { units, scale });
        }
        let divisor = pow10(self.scale - scale)?;
        Ok(Decimal { units: round_div(self.units, divisor, mode)?, scale })
    }

    /// `self / other`, rounded once to `scale` decimals.
    pub fn div(&self, other: &Decimal, scale: u32, mode: Rounding) -> Result<Decimal, DecimalError> {
        if other.units == 0 {
            return Err(DecimalError::DivisionByZero);
        }
        // (a / 10^sa) / (b / 10^sb) = a * 10^(sb + scale) / (b * 10^sa) at `scale` decimals.
        let numerator = self
            .units
            .checked_mul(pow10(other.scale + scale)?)
            .ok_or(DecimalError::Overflow)?;
        let denominator = other
            .units
            .checked_mul(pow10(self.scale)?)
            .ok_or(DecimalError::Overflow)?;
        let (n, d) = if denominator < 0 { (-numerator, -denominator) } else { (numerator, denominator) };
        Ok(Decimal { units: round_div(n, d, mode)?, scale })
    }

    /// Whole minor units (cents) when the value has at most two decimals.
    pub fn to_minor_units(&self) -> Option<i64> {
        if self.scale > 2 {
            return None;
        }
        let units = self.units.checked_mul(10i128.pow(2 - self.scale))?;
        i64::try_from(units).ok()
    }
}

fn round_div(numerator: i128, denominator: i128, mode: Rounding) -> Result<i128, DecimalError> {
    match mode {
        Rounding::Floor => Ok(numerator.div_euclid(denominator)),
        Rounding::HalfUp => {
            let doubled = numerator.checked_mul(2).ok_or(DecimalError::Overflow)?;
            let shifted = doubled.checked_add(denominator).ok_or(DecimalError::Overflow)?;
            let twice = denominator.checked_mul(2).ok_or(DecimalError::Overflow)?;
            Ok(shifted.div_euclid(twice))
        }
    }
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        self.cmp_value(other) == Ordering::Equal
    }
}

impl Eq for Decimal {}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_value(other)
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let digits = self.units.unsigned_abs().to_string();
        let sign = if self.units < 0 { "-" } else { "" };
        if self.scale == 0 {
            return write!(f, "{sign}{digits}");
        }
        let scale = self.scale as usize;
        let padded = if digits.len() <= scale {
            format!("{}{}", "0".repeat(scale + 1 - digits.len()), digits)
        } else {
            digits
        };
        let split = padded.len() - scale;
        write!(f, "{sign}{}.{}", &padded[..split], &padded[split..])
    }
}

impl FromStr for Decimal {
    type Err = DecimalError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Decimal::parse(s)
    }
}

impl Serialize for Decimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Decimal::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
