//! API keys (API-002/003/005/019, D2): the `sk_` format, expiry policies and
//! the verification rules. Hashing is the host's ([`crate::api::ports::ApiCrypto`]).

use serde::{Deserialize, Serialize};

use crate::api::{KeyId, Scope};
use crate::ids::{AccountId, MICROS_PER_DAY, Micros};
use crate::time::{civil_from_days, days_from_civil};

/// API-002: `sk_` plus base64 of 32 random bytes.
pub const KEY_PREFIX: &str = "sk_";
/// D2: the stored display hint is the key's first eight characters.
pub const HINT_LEN: usize = 8;

pub const INVALID_KEY: &str = "Invalid or inactive API key";
pub const EXPIRED_KEY: &str = "API key has expired";

/// The source `expiration_policy_type` values (API-019).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpirationPolicy {
    #[serde(rename = "never")]
    Never,
    #[serde(rename = "30_days")]
    Days30,
    #[serde(rename = "90_days")]
    Days90,
    #[serde(rename = "1_year")]
    OneYear,
    #[serde(rename = "custom")]
    Custom,
}

/// A key the dashboard asks for. `custom_days` is read only for `Custom`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewKey {
    pub name: String,
    pub scopes: Vec<Scope>,
    pub expiration_policy: ExpirationPolicy,
    pub custom_days: Option<i32>,
}

/// A stored key. Exactly one of `digest` and `legacy_bcrypt` identifies it
/// once verified; a migrated row has only `legacy_bcrypt` until first use (D2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: KeyId,
    pub account: AccountId,
    pub name: String,
    pub created_at: Micros,
    pub expires_at: Option<Micros>,
    pub last_used_at: Option<Micros>,
    pub active: bool,
    /// Stored strings, unknown ones kept (migration).
    pub scopes: Vec<String>,
    pub expiration_policy: ExpirationPolicy,
    pub metadata: serde_json::Value,
    /// Lowercase hex SHA-256 of the full key.
    pub digest: Option<String>,
    /// A migrated source bcrypt hash, nulled when the digest is written.
    pub legacy_bcrypt: Option<String>,
    pub hint: Option<String>,
}

/// The key list row (API-005): never the digest, hash or plaintext.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyView {
    pub id: KeyId,
    pub name: String,
    pub created_at: Micros,
    pub expires_at: Option<Micros>,
    pub active: bool,
    pub scopes: Vec<String>,
    pub expiration_policy: ExpirationPolicy,
    /// D2: stored for every new or upgraded key; showing it is a flag-off UI choice.
    pub hint: Option<String>,
}

impl From<&ApiKey> for KeyView {
    fn from(k: &ApiKey) -> Self {
        KeyView {
            id: k.id,
            name: k.name.clone(),
            created_at: k.created_at,
            expires_at: k.expires_at,
            active: k.active,
            scopes: k.scopes.clone(),
            expiration_policy: k.expiration_policy,
            hint: k.hint.clone(),
        }
    }
}

/// The create answer: the plaintext is returned exactly once (API-002).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedKey {
    pub key: KeyView,
    pub api_key: String,
}

/// Whether a bearer value has the API-002 shape: `sk_` and the 44-character
/// padded base64 of 32 bytes. Anything else is refused without a store lookup.
pub fn well_formed(secret: &str) -> bool {
    let Some(body) = secret.strip_prefix(KEY_PREFIX) else { return false };
    let bytes = body.as_bytes();
    bytes.len() == 44
        && bytes[43] == b'='
        && bytes[..43].iter().all(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/')
}

/// D2 display hint.
pub fn hint(secret: &str) -> String {
    secret.chars().take(HINT_LEN).collect()
}

/// `calculate_expiration_date` (`remote_schema.sql:267-291`). `1_year` is the
/// PostgreSQL calendar year (Feb 29 becomes Feb 28); a custom count that is
/// absent or not positive never expires.
pub fn expires_at(policy: ExpirationPolicy, custom_days: Option<i32>, now: Micros) -> Option<Micros> {
    let days = |n: i64| Some(Micros(now.0 + n * MICROS_PER_DAY));
    match policy {
        ExpirationPolicy::Never => None,
        ExpirationPolicy::Days30 => days(30),
        ExpirationPolicy::Days90 => days(90),
        ExpirationPolicy::Custom => custom_days.filter(|d| *d > 0).and_then(|d| days(i64::from(d))),
        ExpirationPolicy::OneYear => {
            let day = now.0.div_euclid(MICROS_PER_DAY);
            let time = now.0.rem_euclid(MICROS_PER_DAY);
            let (y, m, d) = civil_from_days(day);
            // PostgreSQL clamps to the month's last day instead of overflowing.
            let last = civil_from_days(days_from_civil(y + 1, i64::from(m) + 1, 1) - 1).2;
            Some(Micros(days_from_civil(y + 1, i64::from(m), i64::from(d.min(last))) * MICROS_PER_DAY + time))
        }
    }
}

/// API-003 (`remote_schema.sql:1229-1255`): only an active key, and an
/// `expires_at` strictly before `now` is expired (equal still verifies).
pub fn check(key: &ApiKey, now: Micros) -> Result<(), &'static str> {
    if !key.active {
        return Err(INVALID_KEY);
    }
    if key.expires_at.is_some_and(|e| e < now) {
        return Err(EXPIRED_KEY);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::iso8601;

    const NOW: Micros = Micros(1_767_225_600_000_000); // 2026-01-01T00:00:00Z

    fn key(active: bool, expires_at: Option<Micros>) -> ApiKey {
        ApiKey {
            id: KeyId(uuid::Uuid::from_u128(1)),
            account: AccountId(uuid::Uuid::from_u128(2)),
            name: "ci".into(),
            created_at: NOW,
            expires_at,
            last_used_at: None,
            active,
            scopes: vec![],
            expiration_policy: ExpirationPolicy::Never,
            metadata: serde_json::json!({}),
            digest: None,
            legacy_bcrypt: None,
            hint: None,
        }
    }

    // API-002: `sk_` + padded base64 of exactly 32 bytes.
    #[test]
    fn api_002_only_the_source_key_shape_is_well_formed() {
        let good = format!("sk_{}=", "A".repeat(43));
        assert!(well_formed(&good));
        assert!(well_formed("sk_q8+/0aZ9q8+/0aZ9q8+/0aZ9q8+/0aZ9q8+/0aZ9q8+=") );
        for bad in [
            String::new(),
            "sk_".into(),
            format!("pk_{}=", "A".repeat(43)),
            format!("sk_{}", "A".repeat(44)),
            format!("sk_{}==", "A".repeat(42)),
            format!("sk_{}=", "A".repeat(42)),
            format!("sk_{}-=", "A".repeat(42)),
            format!("sk_{} =", "A".repeat(42)),
            "eyJhbGciOiJIUzI1NiJ9.e30.sig".into(),
        ] {
            assert!(!well_formed(&bad), "{bad:?}");
        }
        assert_eq!(hint(&good), "sk_AAAAA");
    }

    // API-019 with `calculate_expiration_date`: never, 30/90 days, the calendar year,
    // and a custom day count where absent or non-positive means never.
    #[test]
    fn api_019_expiry_policies_follow_the_source_function() {
        assert_eq!(expires_at(ExpirationPolicy::Never, Some(5), NOW), None);
        assert_eq!(expires_at(ExpirationPolicy::Days30, None, NOW), Some(Micros(NOW.0 + 30 * MICROS_PER_DAY)));
        assert_eq!(expires_at(ExpirationPolicy::Days90, Some(1), NOW), Some(Micros(NOW.0 + 90 * MICROS_PER_DAY)));
        assert_eq!(expires_at(ExpirationPolicy::OneYear, None, NOW).map(iso8601).as_deref(), Some("2027-01-01T00:00:00.000Z"));
        let leap = Micros(1_709_210_096_000_000); // 2024-02-29T12:34:56Z
        assert_eq!(expires_at(ExpirationPolicy::OneYear, None, leap).map(iso8601).as_deref(), Some("2025-02-28T12:34:56.000Z"));
        assert_eq!(expires_at(ExpirationPolicy::Custom, Some(7), NOW), Some(Micros(NOW.0 + 7 * MICROS_PER_DAY)));
        assert_eq!(expires_at(ExpirationPolicy::Custom, Some(0), NOW), None);
        assert_eq!(expires_at(ExpirationPolicy::Custom, Some(-3), NOW), None);
        assert_eq!(expires_at(ExpirationPolicy::Custom, None, NOW), None);
    }

    // API-003: inactive refuses; `expires_at < now` refuses; equal still verifies.
    #[test]
    fn api_003_inactive_and_strictly_past_expiry_refuse() {
        assert_eq!(check(&key(true, None), NOW), Ok(()));
        assert_eq!(check(&key(false, None), NOW), Err(INVALID_KEY));
        assert_eq!(check(&key(true, Some(NOW)), NOW), Ok(()), "equal is not before now");
        assert_eq!(check(&key(true, Some(Micros(NOW.0 - 1))), NOW), Err(EXPIRED_KEY));
        assert_eq!(check(&key(false, Some(Micros(NOW.0 - 1))), NOW), Err(INVALID_KEY), "inactive is checked first");
    }
}
