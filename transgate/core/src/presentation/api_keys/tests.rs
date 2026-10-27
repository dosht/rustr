use super::*;
use crate::presentation::fixtures;

const NOW: Micros = fixtures::NOW;

fn key(n: u128, name: &str, scopes: &[&str], expires_at: Option<Micros>, active: bool) -> KeyView {
    KeyView {
        id: KeyId(uuid::Uuid::from_u128(n)),
        name: name.into(),
        created_at: Micros(NOW.0 - MICROS_PER_DAY),
        expires_at,
        active,
        scopes: scopes.iter().map(|s| s.to_string()).collect(),
        expiration_policy: ExpirationPolicy::Never,
        hint: None,
    }
}

fn opened(keys: Vec<KeyView>) -> ApiKeysState {
    let mut s = ApiKeysState::default();
    assert_eq!(s.update(Event::Opened), vec![Effect::LoadKeys, Effect::LoadSigningSecret]);
    assert!(s.view(NOW).loading);
    s.update(Event::KeysLoaded { result: Ok(keys) });
    s.update(Event::SecretLoaded { result: Ok("whsec".into()) });
    s
}

// WEB-045/046, API-005/019: rows show scope labels (unknown strings raw, none as "No
// Permissions"), the created instant, expiry and status; only active rows can be revoked.
#[test]
fn web_045_rows_show_scopes_expiry_and_status() {
    let s = opened(vec![
        key(1, "ci", &["transcriptions:read", "transcriptions:create"], None, true),
        key(2, "old", &[], Some(Micros(NOW.0 - 1)), false),
        key(3, "legacy", &["admin"], Some(Micros(NOW.0 + 3 * MICROS_PER_DAY)), true),
    ]);
    let vm = s.view(NOW);
    assert!(!vm.loading && !vm.empty && vm.error.is_none());
    assert_eq!(vm.rows[0].scopes, vec![ScopeLabel::Known { label: common("Read Only") }, ScopeLabel::Known { label: common("Create Only") }]);
    assert_eq!((vm.rows[0].expiry.clone(), vm.rows[0].expiry_tone, vm.rows[0].status.clone(), vm.rows[0].can_revoke), (ExpiryLabel::Never, Tone::Neutral, common("Active"), true));
    assert_eq!(vm.rows[0].created_at_ms, (NOW.0 - MICROS_PER_DAY) / 1000);
    assert!(vm.rows[1].scopes.is_empty());
    assert_eq!((vm.rows[1].expiry.clone(), vm.rows[1].status.clone(), vm.rows[1].can_revoke), (ExpiryLabel::Expired, common("Revoked"), false));
    assert_eq!(vm.rows[2].scopes, vec![ScopeLabel::Raw { value: "admin".into() }]);
    assert_eq!((vm.rows[2].expiry.clone(), vm.rows[2].expiry_tone), (ExpiryLabel::Days { days: 3 }, Tone::Progress));
    // `api-keys.tsx:495-500`: the status badge is green when active, red when revoked;
    // an expired key keeps its own red expiry badge.
    assert_eq!((vm.rows[0].status_tone, vm.rows[1].status_tone), (Tone::Success, Tone::Danger));
    assert_eq!(vm.signing_secret.as_deref(), Some("whsec"));
    // D10: the secret section's copy is core's (the source had no such section).
    assert_eq!(
        (vm.secret_title.clone(), vm.secret_description.clone(), vm.secret_copy_label.clone()),
        (SECRET_TITLE, SECRET_DESCRIPTION, SECRET_COPY),
    );
    assert!(opened(vec![]).view(NOW).empty);
}

// `getExpirationStatus`: whole days rounded up; at most 7 warns, more is success.
#[test]
fn expiry_rounds_up_whole_days_and_warns_within_seven() {
    assert_eq!(expiry(None, NOW), (ExpiryLabel::Never, Tone::Neutral));
    assert_eq!(expiry(Some(Micros(NOW.0 - 1)), NOW), (ExpiryLabel::Expired, Tone::Danger));
    assert_eq!(expiry(Some(NOW), NOW), (ExpiryLabel::Days { days: 0 }, Tone::Progress), "not before now");
    assert_eq!(expiry(Some(Micros(NOW.0 + 1)), NOW), (ExpiryLabel::Days { days: 1 }, Tone::Progress));
    assert_eq!(expiry(Some(Micros(NOW.0 + 7 * MICROS_PER_DAY)), NOW), (ExpiryLabel::Days { days: 7 }, Tone::Progress));
    assert_eq!(expiry(Some(Micros(NOW.0 + 7 * MICROS_PER_DAY + 1)), NOW), (ExpiryLabel::Days { days: 8 }, Tone::Success));
}

// API-019: the form defaults to Read & Create and never expiring; a name is required; the custom
// policy needs 1-3650 days; create sends the choice once and the new key is shown once, then the
// list reloads and the form resets.
#[test]
fn api_019_create_validates_sends_once_and_shows_the_key_once() {
    let mut s = opened(vec![]);
    assert_eq!(s.view(NOW).form_toggle, common("Generate New Key"));
    s.update(Event::FormToggled);
    let form = s.view(NOW).form.expect("open");
    assert_eq!(s.view(NOW).form_toggle, common("Cancel"));
    assert!(form.scopes.iter().any(|o| o.value == ScopeChoice::ReadAndCreate && o.selected));
    assert!(form.policies.iter().any(|o| o.value == ExpirationPolicy::Never && o.selected));
    assert_eq!((form.custom_days, form.can_create), (None, false), "a name is required");
    assert!(s.update(Event::CreateRequested).is_empty());
    s.update(Event::NameChanged { text: "  ".into() });
    assert!(!s.view(NOW).form.unwrap().can_create);
    s.update(Event::NameChanged { text: "ci".into() });
    s.update(Event::ScopeChosen { scope: ScopeChoice::Read });
    s.update(Event::PolicyChosen { policy: ExpirationPolicy::Custom });
    assert_eq!(s.view(NOW).form.unwrap().custom_days.as_deref(), Some("30"));
    for bad in ["0", "3651", "x", ""] {
        s.update(Event::CustomDaysChanged { text: bad.into() });
        let form = s.view(NOW).form.unwrap();
        assert!(form.custom_days_invalid && !form.can_create, "{bad:?}");
    }
    s.update(Event::CustomDaysChanged { text: "45".into() });
    let effects = s.update(Event::CreateRequested);
    let expected = NewKey { name: "ci".into(), scopes: vec![Scope::TranscriptionsRead], expiration_policy: ExpirationPolicy::Custom, custom_days: Some(45) };
    assert_eq!(effects, vec![Effect::CreateKey { key: expected }]);
    assert!(s.view(NOW).form.unwrap().creating);
    assert!(s.update(Event::CreateRequested).is_empty(), "one create at a time");

    let created = CreatedKey { key: key(9, "ci", &["transcriptions:read"], None, true), api_key: "sk_secret=".into() };
    assert_eq!(s.update(Event::Created { result: Ok(created) }), vec![Effect::LoadKeys]);
    let vm = s.view(NOW);
    assert_eq!((vm.created_key.as_deref(), vm.form.is_none(), vm.notice), (Some("sk_secret="), true, Some(common("API key created successfully"))));
    s.update(Event::FormToggled);
    let form = s.view(NOW).form.unwrap();
    assert!(form.name.is_empty() && form.scopes.iter().any(|o| o.value == ScopeChoice::ReadAndCreate && o.selected), "reset to defaults");
}

#[test]
fn a_failed_create_keeps_the_form_and_says_so() {
    let mut s = opened(vec![]);
    s.update(Event::FormToggled);
    s.update(Event::NameChanged { text: "ci".into() });
    s.update(Event::CreateRequested);
    assert!(s.update(Event::Created { result: Err(ApiFailure::http(500, None)) }).is_empty());
    let vm = s.view(NOW);
    assert_eq!((vm.created_key, vm.notice), (None, Some(common("Failed to create API key"))));
    let form = vm.form.unwrap();
    assert!(form.name == "ci" && !form.creating && form.can_create);
}

// The new key and the signing secret copy through the shell; the outcome shows "Copied!" or the
// source's failure notice.
#[test]
fn copying_the_key_or_secret_reports_the_outcome() {
    let mut s = opened(vec![]);
    s.update(Event::FormToggled);
    s.update(Event::NameChanged { text: "ci".into() });
    s.update(Event::CreateRequested);
    s.update(Event::Created { result: Ok(CreatedKey { key: key(9, "ci", &[], None, true), api_key: "sk_k=".into() }) });
    assert_eq!(s.update(Event::CopyRequested { what: Copied::Key }), vec![Effect::CopyText { text: "sk_k=".into(), what: Copied::Key }]);
    s.update(Event::CopyAnswered { what: Copied::Key, ok: true });
    let vm = s.view(NOW);
    assert!(vm.key_copied && !vm.secret_copied);
    assert_eq!(vm.notice, Some(common("API key copied to clipboard")));
    assert_eq!(s.update(Event::CopyRequested { what: Copied::Secret }), vec![Effect::CopyText { text: "whsec".into(), what: Copied::Secret }]);
    s.update(Event::CopyAnswered { what: Copied::Secret, ok: true });
    let vm = s.view(NOW);
    assert!(vm.secret_copied && !vm.key_copied);
    assert_eq!(vm.notice, Some(SECRET_COPIED), "the secret is not an API key");
    s.update(Event::CopyAnswered { what: Copied::Secret, ok: false });
    let vm = s.view(NOW);
    assert!(!vm.secret_copied);
    assert_eq!(vm.notice, Some(SECRET_COPY_FAILED));
    let mut none = opened(vec![]);
    assert!(none.update(Event::CopyRequested { what: Copied::Key }).is_empty(), "nothing to copy before a create");
}

// API-019: revoke asks first; cancel changes nothing; confirming revokes one key at a time and reloads.
#[test]
fn api_019_revoke_asks_first_then_reloads() {
    let id = KeyId(uuid::Uuid::from_u128(1));
    let mut s = opened(vec![key(1, "ci", &["transcriptions:read"], None, true), key(2, "off", &[], None, false)]);
    assert!(s.update(Event::RevokeRequested { id: KeyId(uuid::Uuid::from_u128(2)) }).is_empty(), "a revoked key cannot be revoked");
    assert_eq!(s.view(NOW).confirm_revoke, None);
    assert!(s.update(Event::RevokeRequested { id }).is_empty());
    assert_eq!(s.view(NOW).confirm_revoke, Some(id));
    assert!(s.update(Event::RevokeCanceled).is_empty());
    assert_eq!(s.view(NOW).confirm_revoke, None);
    s.update(Event::RevokeRequested { id });
    assert_eq!(s.update(Event::RevokeConfirmed), vec![Effect::RevokeKey { id }]);
    let vm = s.view(NOW);
    assert!(vm.rows[0].revoking && !vm.rows[0].can_revoke && vm.confirm_revoke.is_none());
    let revoked = key(1, "ci", &["transcriptions:read"], None, false);
    assert_eq!(s.update(Event::Revoked { id, result: Ok(revoked) }), vec![Effect::LoadKeys]);
    let vm = s.view(NOW);
    assert_eq!((vm.rows[0].active, vm.rows[0].revoking, vm.notice), (false, false, Some(common("API key revoked successfully"))));
    s.update(Event::RevokeRequested { id: KeyId(uuid::Uuid::from_u128(1)) });
    assert!(s.view(NOW).confirm_revoke.is_none());
}

#[test]
fn failures_show_without_losing_loaded_rows() {
    let id = KeyId(uuid::Uuid::from_u128(1));
    let mut s = opened(vec![key(1, "ci", &[], None, true)]);
    s.update(Event::RevokeRequested { id });
    s.update(Event::RevokeConfirmed);
    s.update(Event::Revoked { id, result: Err(ApiFailure::network()) });
    let vm = s.view(NOW);
    assert_eq!((vm.rows[0].active, vm.rows[0].revoking, vm.notice), (true, false, Some(common("Failed to revoke API key"))));
    s.update(Event::KeysLoaded { result: Err(ApiFailure::http(503, None)) });
    s.update(Event::SecretLoaded { result: Err(ApiFailure::http(503, None)) });
    let vm = s.view(NOW);
    assert_eq!((vm.rows.len(), vm.error, vm.secret_error), (1, Some(LOAD_FAILED), Some(SECRET_FAILED)));
}
