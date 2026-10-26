use super::*;

const ACCOUNT: AccountId = AccountId(uuid::Uuid::from_u128(1));

fn at(n: i64) -> Micros {
    Micros(n)
}

// D3: refusal fences in-flight work (its epoch is stale) but keeps completed artifacts, which
// a regrant can show again; the regrant never reauthorizes the old in-flight writer.
#[test]
fn refusal_fences_writers_but_regrant_restores_reads() {
    let granted = decide(None, ACCOUNT, true, at(1));
    let admitted = granted.epoch;
    assert!(may_write(Some(&granted), admitted) && may_read(Some(&granted), admitted));
    let refused = decide(Some(granted), ACCOUNT, false, at(2));
    assert!(!may_read(Some(&refused), admitted) && !may_write(Some(&refused), admitted));
    assert_eq!((refused.consent_date, refused.revoked_date), (None, Some(at(2))));
    let regranted = decide(Some(refused), ACCOUNT, true, at(3));
    assert!(may_read(Some(&regranted), admitted), "a completed artifact is readable again");
    assert!(!may_write(Some(&regranted), admitted), "the old worker stays fenced");
}

// D3, AI-023: DELETE covers every epoch up to the current one, including legacy (0) and
// artifacts of earlier refusal/grant cycles; newer artifacts after a regrant stay readable.
#[test]
fn delete_cuts_off_every_older_epoch_for_good() {
    let mut r = decide(None, ACCOUNT, true, at(1));
    r = decide(Some(r), ACCOUNT, false, at(2));
    r = decide(Some(r), ACCOUNT, true, at(3));
    let before = r.epoch;
    let revoked = revoke(Some(r), ACCOUNT, at(4));
    assert_eq!((revoked.purge_cutoff, revoked.purge_pending, revoked.has_consented), (Some(before), true, false));
    let regranted = decide(Some(revoked), ACCOUNT, true, at(5));
    for old in [0, 1, before] {
        assert!(!may_read(Some(&regranted), old), "epoch {old} stays purged after a regrant");
    }
    assert!(may_read(Some(&regranted), regranted.epoch));
    // A second DELETE never moves the cutoff backwards.
    let again = revoke(Some(regranted.clone()), ACCOUNT, at(6));
    assert_eq!(again.purge_cutoff, Some(regranted.epoch));
}

// AI-022, D3: no row reads as never consented with the source defaults; a DELETE without a
// row still records the cutoff over legacy artifacts.
#[test]
fn no_row_is_no_consent_and_delete_still_covers_legacy() {
    let v = view(None);
    assert_eq!((v.has_consented, v.can_revoke, v.consent_version.as_str(), v.provider_name.as_str()), (false, false, "1.0", "deepseek"));
    let revoked = revoke(None, ACCOUNT, at(1));
    assert!(covered(Some(&revoked), 0));
    assert!(view(Some(&revoked)).cleanup_pending);
}
