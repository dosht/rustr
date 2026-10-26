//! AI consent (AI-011, AI-022, AI-023, D3), pure.
//!
//! Valid consent is granted and not revoked, on every path (the analysis
//! route's has-consented-only check is corrected). Every decision advances
//! the write epoch: in-flight work admitted under an older epoch can no
//! longer write. Refusal (POST false) keeps completed artifacts, readable
//! again after a regrant; DELETE also records a purge cutoff, and nothing at
//! or below it is ever readable again, whatever later grants say.

use crate::ai::ConsentView;
use crate::ai::ports::ConsentRecord;
use crate::ids::{AccountId, Micros};

pub const CONSENT_VERSION: &str = "1.0";
pub const PRIVACY_POLICY_URL: &str = "/legal/privacy-policy-en";
pub const PROVIDER_NAME: &str = "deepseek";

/// Granted and not revoked. No row is no consent.
pub fn is_valid(record: Option<&ConsentRecord>) -> bool {
    record.is_some_and(|r| r.has_consented && r.revoked_date.is_none())
}

/// The current epoch; 0 before any decision (legacy artifacts carry 0 too).
pub fn epoch(record: Option<&ConsentRecord>) -> i64 {
    record.map_or(0, |r| r.epoch)
}

/// Active and staged writes need valid consent at exactly their admission epoch.
pub fn may_write(record: Option<&ConsentRecord>, admitted_epoch: i64) -> bool {
    is_valid(record) && epoch(record) == admitted_epoch
}

/// A completed artifact is readable under current valid consent unless a
/// DELETE cutoff covers its epoch. Its epoch need not equal the current one.
pub fn may_read(record: Option<&ConsentRecord>, artifact_epoch: i64) -> bool {
    is_valid(record) && !covered(record, artifact_epoch)
}

/// Whether a DELETE cutoff covers an artifact epoch.
pub fn covered(record: Option<&ConsentRecord>, artifact_epoch: i64) -> bool {
    record.and_then(|r| r.purge_cutoff).is_some_and(|cutoff| artifact_epoch <= cutoff)
}

fn base(previous: Option<ConsentRecord>, account: AccountId, now: Micros) -> ConsentRecord {
    previous.unwrap_or(ConsentRecord {
        account,
        has_consented: false,
        version: CONSENT_VERSION.into(),
        provider: PROVIDER_NAME.into(),
        policy_url: PRIVACY_POLICY_URL.into(),
        consent_date: None,
        revoked_date: None,
        epoch: 0,
        purge_cutoff: None,
        purge_pending: false,
        updated_at: now,
    })
}

/// POST `{hasConsented}` (AI-022): upserts version, policy and provider; a
/// grant sets the consent date and clears revocation, a refusal does the
/// inverse. The epoch advances either way; cutoff and pending purge stay.
pub fn decide(previous: Option<ConsentRecord>, account: AccountId, granted: bool, now: Micros) -> ConsentRecord {
    let mut r = base(previous, account, now);
    r.has_consented = granted;
    r.version = CONSENT_VERSION.into();
    r.provider = PROVIDER_NAME.into();
    r.policy_url = PRIVACY_POLICY_URL.into();
    r.consent_date = granted.then_some(now);
    r.revoked_date = (!granted).then_some(now);
    r.epoch += 1;
    r.updated_at = now;
    r
}

/// DELETE (AI-023, D3): not consented, revoked now, a new epoch, and a purge
/// cutoff at the epoch every existing artifact belongs to. The cutoff only
/// moves forward. The purge itself runs after this commits.
pub fn revoke(previous: Option<ConsentRecord>, account: AccountId, now: Micros) -> ConsentRecord {
    let mut r = base(previous, account, now);
    let cutoff = r.epoch;
    r.has_consented = false;
    r.revoked_date = Some(now);
    r.purge_cutoff = Some(r.purge_cutoff.map_or(cutoff, |c| c.max(cutoff)));
    r.purge_pending = true;
    r.epoch += 1;
    r.updated_at = now;
    r
}

/// `GET /api/ai/consent`: the source's state, defaults for no row.
pub fn view(record: Option<&ConsentRecord>) -> ConsentView {
    match record {
        None => ConsentView {
            has_consented: false,
            consent_version: CONSENT_VERSION.into(),
            consent_date: None,
            revoked_date: None,
            privacy_policy_url: PRIVACY_POLICY_URL.into(),
            provider_name: PROVIDER_NAME.into(),
            can_revoke: false,
            cleanup_pending: false,
        },
        Some(r) => ConsentView {
            has_consented: r.has_consented,
            consent_version: r.version.clone(),
            consent_date: r.consent_date,
            revoked_date: r.revoked_date,
            privacy_policy_url: if r.policy_url.is_empty() { PRIVACY_POLICY_URL.into() } else { r.policy_url.clone() },
            provider_name: r.provider.clone(),
            can_revoke: is_valid(Some(r)),
            cleanup_pending: r.purge_pending,
        },
    }
}

#[cfg(test)]
#[path = "consent/tests.rs"]
mod tests;
