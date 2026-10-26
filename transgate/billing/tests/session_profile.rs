//! Host identity-provider facts: Joined on is specifically email confirmation.

use billing_core::types::{AccountId, Micros};
use serde_json::json;
use transgate_billing::directory::parse_auth_user;
use uuid::Uuid;

#[test]
fn joined_on_is_email_confirmation_and_never_creation_or_generic_confirmation() {
    let account = AccountId(Uuid::from_u128(17));
    let mut user = json!({
        "id": account.0,
        "email": "ada@example.test",
        "created_at": "2020-01-01T00:00:00Z",
        "confirmed_at": "2021-01-01T00:00:00Z",
        "email_confirmed_at": "2025-01-01T00:00:00Z"
    });
    let profile = parse_auth_user(account, &user).unwrap();
    assert_eq!(profile.email_confirmed_at, Some(Micros(1_735_689_600_000_000)));
    user.as_object_mut().unwrap().remove("email_confirmed_at");
    let profile = parse_auth_user(account, &user).unwrap();
    assert!(profile.email_verified, "legacy generic confirmation still verifies the address");
    assert_eq!(profile.email_confirmed_at, None, "neither legacy confirmation nor account creation is Joined on");
}

#[test]
fn malformed_email_confirmation_is_not_reported_as_a_trusted_missing_date() {
    let account = AccountId(Uuid::from_u128(18));
    let user = json!({
        "id": account.0,
        "email": "ada@example.test",
        "created_at": "2020-01-01T00:00:00Z",
        "email_confirmed_at": "not-a-timestamp"
    });
    assert!(parse_auth_user(account, &user).is_err());
}
