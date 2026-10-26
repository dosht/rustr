//! The JavaScript number and value conversions the captured routes depend on
//! (`Number`, `parseFloat`, `parseInt`, truthiness, `JSON.stringify` of
//! numbers), and an insertion-ordered JSON object for responses.

use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::Value;

/// `Number(text)`.
pub fn number(text: &str) -> f64 {
    let t = text.trim();
    if t.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = t.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix).map_or(f64::NAN, |v| v as f64);
        }
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if !t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// `parseFloat(text)`: the longest decimal prefix after leading whitespace.
pub fn parse_float(text: &str) -> f64 {
    let t = text.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    if t[i..].starts_with("Infinity") {
        return if b.first() == Some(&b'-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    if i < b.len() && b[i] == b'.' {
        let frac_start = i + 1;
        let mut j = frac_start;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        digits += j - frac_start;
        if digits > 0 {
            i = j;
        }
    }
    if digits == 0 {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    t[..i].trim_end_matches('.').parse().unwrap_or(f64::NAN)
}

/// `parseInt(text, 10)`.
pub fn parse_int(text: &str) -> f64 {
    let t = text.trim_start();
    let (negative, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits: &str = &rest[..rest.bytes().take_while(u8::is_ascii_digit).count()];
    if digits.is_empty() {
        return f64::NAN;
    }
    let value: f64 = digits.parse().unwrap_or(f64::NAN);
    if negative { -value } else { value }
}

/// `String(value)` for the JSON values ffprobe produces.
pub fn string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Array(items) => items
            .iter()
            .map(|v| if v.is_null() { String::new() } else { string(v) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `x && !isNaN(x) ? x : null` over an already converted number.
pub fn nonzero(x: f64) -> Option<f64> {
    (x != 0.0 && !x.is_nan()).then_some(x)
}

/// A number written into an FFmpeg argument, as a JS template literal would.
pub fn arg(x: f64) -> String {
    format!("{x}")
}

/// A JSON response value whose objects keep insertion order and whose numbers
/// serialize as `JSON.stringify` does (integral values without a fraction,
/// non-finite values as `null`).
#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
}

impl Serialize for J {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            J::Null => s.serialize_unit(),
            J::Bool(b) => s.serialize_bool(*b),
            J::Num(n) if !n.is_finite() => s.serialize_unit(),
            J::Num(n) if n.fract() == 0.0 && n.abs() < 1e21 => s.serialize_i128(*n as i128),
            J::Num(n) => s.serialize_f64(*n),
            J::Str(text) => s.serialize_str(text),
            J::Arr(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            J::Obj(fields) => {
                let mut map = s.serialize_map(Some(fields.len()))?;
                for (k, v) in fields {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl From<f64> for J {
    fn from(n: f64) -> J {
        J::Num(n)
    }
}
impl From<u64> for J {
    fn from(n: u64) -> J {
        J::Num(n as f64)
    }
}
impl From<bool> for J {
    fn from(b: bool) -> J {
        J::Bool(b)
    }
}
impl From<&str> for J {
    fn from(s: &str) -> J {
        J::Str(s.to_string())
    }
}
impl From<String> for J {
    fn from(s: String) -> J {
        J::Str(s)
    }
}
impl From<&String> for J {
    fn from(s: &String) -> J {
        J::Str(s.clone())
    }
}
impl<T: Into<J>> From<Option<T>> for J {
    fn from(v: Option<T>) -> J {
        v.map_or(J::Null, Into::into)
    }
}
impl From<Vec<f64>> for J {
    fn from(v: Vec<f64>) -> J {
        J::Arr(v.into_iter().map(J::Num).collect())
    }
}

/// `obj! { "key" => value, ... }` builds an ordered [`J::Obj`].
#[macro_export]
macro_rules! obj {
    ($($k:literal => $v:expr),* $(,)?) => {
        $crate::js::J::Obj(vec![$(($k, $crate::js::J::from($v))),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_parsing_follows_javascript() {
        assert_eq!(number(" 2 "), 2.0);
        assert_eq!(number(""), 0.0);
        assert_eq!(number("0x10"), 16.0);
        assert!(number("inf").is_nan());
        assert!(number("2 threads").is_nan());
        assert_eq!(parse_float("4.000000\n"), 4.0);
        assert!(parse_float("N/A").is_nan());
        assert_eq!(parse_float("  1.5e3xyz"), 1500.0);
        assert_eq!(parse_float("7."), 7.0);
        assert_eq!(parse_float(".5"), 0.5);
        assert_eq!(parse_float("1e"), 1.0);
        assert_eq!(parse_int("44100"), 44100.0);
        assert_eq!(parse_int("12.9"), 12.0);
        assert!(parse_int("x1").is_nan());
    }

    #[test]
    fn json_numbers_print_like_json_stringify_and_objects_keep_order() {
        let value = crate::obj! { "z" => 100.0, "a" => 0.5, "n" => f64::NAN, "o" => None::<f64> };
        assert_eq!(serde_json::to_string(&value).unwrap(), r#"{"z":100,"a":0.5,"n":null,"o":null}"#);
    }
}
