use media_ports::StoreKind;

use crate::ai::GenerationId;

use super::*;

const NOW: Micros = Micros(1_800_000_000_000_000);

fn object(store: StoreKind, key: &str) -> ObjectRef {
    ObjectRef { store, bucket: "b".into(), key: key.into(), version: None }
}

fn item(target: ItemTarget, state: ItemState) -> ErasureItem {
    ErasureItem { id: uuid::Uuid::new_v4(), erasure: ErasureId(uuid::Uuid::nil()), target, state, attempts: 0 }
}

fn record(state: ErasureState) -> ErasureRecord {
    ErasureRecord {
        id: ErasureId(uuid::Uuid::nil()),
        account: AccountId(uuid::Uuid::nil()),
        requested_at: NOW,
        state,
        local_erased_at: None,
        identity_deleted_at: None,
        closed_at: None,
    }
}

fn upload(key: &str, expires_at: Micros) -> IssuedUpload {
    IssuedUpload { account: AccountId(uuid::Uuid::nil()), job: JobId(1), object: object(StoreKind::Gcs, key), expires_at }
}

fn resources(media: Vec<ObjectRef>, archived_payloads: Vec<ArchivedPayload>, issued_uploads: Vec<IssuedUpload>) -> AccountResources {
    AccountResources {
        jobs: vec![JobId(1)], media, archived_payloads, provider_copies: vec![],
        unresolved_submissions: vec![], issued_uploads, llm_dispatches: vec![],
    }
}

fn deletes(items: &[ErasureItem]) -> Vec<&str> {
    items
        .iter()
        .filter_map(|i| match &i.target {
            ItemTarget::StorageObject { object } | ItemTarget::LegacyStorageObject { object } => Some(object.key.as_str()),
            _ => None,
        })
        .collect()
}

fn resweeps(items: &[ErasureItem]) -> Vec<(&str, Micros)> {
    items
        .iter()
        .filter_map(|i| match &i.target {
            ItemTarget::UploadResweep { object, not_before } => Some((object.key.as_str(), *not_before)),
            _ => None,
        })
        .collect()
}

fn in_flight(items: &[ErasureItem]) -> Vec<&str> {
    items
        .iter()
        .filter_map(|i| match &i.target {
            ItemTarget::ArchiveInFlight { object } => Some(object.key.as_str()),
            _ => None,
        })
        .collect()
}

fn archive(key: &str, write: ArchiveWrite) -> ArchivedPayload {
    ArchivedPayload { account: AccountId(uuid::Uuid::nil()), job: JobId(1), object: object(StoreKind::Gcs, key), write }
}

/// Every item done except those `keep` selects, which take `state`.
fn settle(items: Vec<ErasureItem>, keep: impl Fn(&ItemTarget) -> bool, state: ItemState) -> Vec<ErasureItem> {
    items
        .into_iter()
        .map(|mut i| {
            i.state = if keep(&i.target) { state.clone() } else { ItemState::Absent { at: NOW } };
            i
        })
        .collect()
}

// D2, F9: everything in the legacy Supabase store, its account folder included,
// runs before the identity; the rest waits for the identity.
#[test]
fn manifest_phases() {
    let mut resources = resources(vec![object(StoreKind::Gcs, "u/1/a.mp3"), object(StoreKind::Supabase, "old/a.mp3")], vec![], vec![]);
    resources.unresolved_submissions = vec![(Provider::Soniox, "job_1".into())];
    let items = build_items(ErasureId(uuid::Uuid::nil()), &resources, NOW);
    let first = runnable(&items, NOW);
    assert_eq!(first.len(), 2);
    assert!(first.iter().any(|i| matches!(i.target, ItemTarget::LegacyStorageObject { .. })));
    assert!(first.iter().any(|i| i.target == ItemTarget::LegacyAccountInventory { bucket: LEGACY_JOBS_BUCKET.into() }));
    assert!(items.iter().any(|i| i.target == ItemTarget::Identity));
}

// F9: the legacy folder is in every manifest, also for an account without jobs.
#[test]
fn the_legacy_folder_is_always_inventoried() {
    let items = build_items(ErasureId(uuid::Uuid::nil()), &resources(vec![], vec![], vec![]), NOW);
    assert!(items.iter().any(|i| i.target == ItemTarget::LegacyAccountInventory { bucket: LEGACY_JOBS_BUCKET.into() }));
}

// D9: content/job deletion cannot establish provider retention. Every possible
// dispatch remains open, with the generation/provider and all observed ids.
#[test]
fn possible_llm_dispatches_keep_erasure_open_and_name_every_observed_identity() {
    let generation = GenerationId(uuid::Uuid::from_u128(77));
    for (disposition, remote_ids) in [
        (DispatchDisposition::Possible, vec![]),
        (DispatchDisposition::Possible, vec!["request-id".to_string(), "completion-id".to_string()]),
        // Even contradictory settlement cannot erase an observed dispatch.
        (DispatchDisposition::NotDispatched, vec!["observed-before-settlement".to_string()]),
    ] {
        let mut resources = resources(vec![], vec![], vec![]);
        resources.jobs.clear();
        resources.llm_dispatches.push(LlmDispatch {
            account: AccountId(uuid::Uuid::nil()),
            generation,
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            disposition,
            remote_ids: remote_ids.clone(),
            unkept_id_observed: false,
            created_at: NOW,
        });
        let items = build_items(ErasureId(uuid::Uuid::nil()), &resources, NOW);
        let llm = items.iter().find(|i| matches!(i.target, ItemTarget::LlmDispatch { .. })).unwrap();
        assert_eq!(llm.target, ItemTarget::LlmDispatch {
            generation: generation.0, provider: "deepseek".into(), remote_ids: remote_ids.clone(),
            unkept_id_observed: false,
        });
        assert_eq!(llm.target.phase(), 2);
        assert!(matches!(&llm.state, ItemState::Unresolved { reason, .. } if reason == LLM_RETENTION_UNVERIFIED));
        let unresolved = llm.state.clone();
        let items = settle(items, |t| matches!(t, ItemTarget::LlmDispatch { .. }), unresolved);
        assert_eq!(closure(&items), None, "known remote ids do not establish erasure");
        let ErasureView::LocalDataErased { pending, exceptions } = view(&record(ErasureState::LocalErased), &items, None) else {
            panic!("possible provider retention must remain visible and open")
        };
        assert_eq!(pending, 1);
        assert!(exceptions.iter().any(|message| message.contains("deepseek")
            && message.contains(&generation.0.to_string())
            && message.contains(LLM_RETENTION_UNVERIFIED)
            && remote_ids.iter().all(|id| message.contains(id))), "{exceptions:?}");
    }
}

// Only a definitive pre-dispatch outcome establishes absence. No remote API
// or timeout assumption is needed, and the manifest still records the outcome.
#[test]
fn a_definitively_nondispatched_llm_request_is_absent_in_the_manifest() {
    let mut resources = resources(vec![], vec![], vec![]);
    resources.llm_dispatches.push(LlmDispatch {
        account: AccountId(uuid::Uuid::nil()),
        generation: GenerationId(uuid::Uuid::from_u128(78)),
        provider: "deepseek".into(),
        model: "deepseek-chat".into(),
        disposition: DispatchDisposition::NotDispatched,
        remote_ids: vec![],
        unkept_id_observed: false,
        created_at: NOW,
    });
    let items = build_items(ErasureId(uuid::Uuid::nil()), &resources, NOW);
    let llm = items.iter().find(|i| matches!(i.target, ItemTarget::LlmDispatch { .. })).unwrap();
    assert_eq!(llm.state, ItemState::Absent { at: NOW });
    let items = settle(items, |t| matches!(t, ItemTarget::LlmDispatch { .. }), ItemState::Absent { at: NOW });
    assert_eq!(closure(&items), Some(ErasureState::FullyErased));
}

// D9: an id too large to retain still proves a possible dispatch. A later
// nondispatch cannot turn an empty retained-id list into proof of absence.
#[test]
fn an_unkept_llm_id_keeps_erasure_unresolved_without_retained_ids() {
    for disposition in [DispatchDisposition::Possible, DispatchDisposition::NotDispatched] {
        let mut resources = resources(vec![], vec![], vec![]);
        resources.llm_dispatches.push(LlmDispatch {
            account: AccountId(uuid::Uuid::nil()),
            generation: GenerationId(uuid::Uuid::from_u128(79)),
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            disposition,
            remote_ids: vec![],
            unkept_id_observed: true,
            created_at: NOW,
        });
        let items = build_items(ErasureId(uuid::Uuid::nil()), &resources, NOW);
        let llm = items.iter().find(|i| matches!(i.target, ItemTarget::LlmDispatch { .. })).unwrap();
        assert!(matches!(&llm.target, ItemTarget::LlmDispatch { remote_ids, unkept_id_observed: true, .. } if remote_ids.is_empty()));
        assert!(matches!(&llm.state, ItemState::Unresolved { reason, .. } if reason == LLM_RETENTION_UNVERIFIED));
        let unresolved = llm.state.clone();
        let items = settle(items, |t| matches!(t, ItemTarget::LlmDispatch { .. }), unresolved);
        assert_eq!(closure(&items), None, "unkept observed evidence must survive manifest capture");
        let ErasureView::LocalDataErased { pending, exceptions } = view(&record(ErasureState::LocalErased), &items, None) else {
            panic!("unkept dispatch evidence must remain visible and open")
        };
        assert_eq!(pending, 1);
        assert!(exceptions.iter().any(|message| message.contains("additional provider ids were observed but not retained")
            && message.contains(LLM_RETENTION_UNVERIFIED)), "{exceptions:?}");
    }
}

// F1: every issued upload, live, expired or for a removed draft, is deleted at
// once and swept again after its last URL expires, however long ago that was.
#[test]
fn every_issued_upload_is_deleted_and_swept_after_its_last_url_expires() {
    let live = upload("u/1/live.mp3", Micros(NOW.0 + 1));
    let again = upload("u/1/live.mp3", Micros(NOW.0 + 9));
    let old = upload("u/2/old.mp3", Micros(NOW.0 - 365 * crate::ids::MICROS_PER_DAY));
    let media = vec![object(StoreKind::Gcs, "u/1/live.mp3")];
    let items = build_items(ErasureId(uuid::Uuid::nil()), &resources(media, vec![], vec![live, again, old]), NOW);
    assert_eq!(deletes(&items), vec!["u/1/live.mp3", "u/2/old.mp3"]);
    assert_eq!(resweeps(&items), vec![("u/1/live.mp3", Micros(NOW.0 + 9)), ("u/2/old.mp3", Micros(NOW.0 - 365 * crate::ids::MICROS_PER_DAY))]);
    let state = |key: &str| items.iter().find(|i| matches!(&i.target, ItemTarget::UploadResweep { object, .. } if object.key == key)).unwrap().state.clone();
    assert!(!state("u/1/live.mp3").is_runnable(Micros(NOW.0 + 8)));
    assert!(state("u/1/live.mp3").is_runnable(Micros(NOW.0 + 9)));
    assert_eq!(state("u/2/old.mp3"), ItemState::Pending);
}

// F1: a client PUT is never acknowledged, so no sweep resolves an upload:
// found or missed, it stays unresolved and the saga stays open.
#[test]
fn no_sweep_resolves_an_upload() {
    let target = ItemTarget::UploadResweep { object: object(StoreKind::Gcs, "u/1/a.mp3"), not_before: NOW };
    for found in [false, true] {
        let state = after_sweep(&target, found, 3, NOW);
        assert_eq!(state, ItemState::Unresolved { reason: UPLOAD_OUTCOME_UNKNOWN.into(), next_at: next_retry(3, NOW) });
    }
    let items = build_items(ErasureId(uuid::Uuid::nil()), &resources(vec![], vec![], vec![upload("u/1/a.mp3", NOW)]), NOW);
    let missed = after_sweep(&target, false, 0, NOW);
    assert_eq!(closure(&settle(items, |t| matches!(t, ItemTarget::UploadResweep { .. }), missed)), None);
}

// F2: every archive object is deleted at once; only an unacknowledged write
// keeps an in-flight item, resolved by finding its object, never by a miss.
#[test]
fn an_unacknowledged_archive_write_is_resolved_only_by_evidence() {
    let payloads = vec![
        archive("a/1/soniox.json", ArchiveWrite::InFlight),
        archive("a/2/soniox.json", ArchiveWrite::Written),
        archive("a/3/soniox.json", ArchiveWrite::NotWritten),
    ];
    let items = build_items(ErasureId(uuid::Uuid::nil()), &resources(vec![], payloads, vec![]), NOW);
    assert_eq!(deletes(&items), vec!["a/1/soniox.json", "a/2/soniox.json", "a/3/soniox.json"]);
    assert_eq!(in_flight(&items), vec!["a/1/soniox.json"]);

    let target = ItemTarget::ArchiveInFlight { object: object(StoreKind::Gcs, "a/1/soniox.json") };
    let later = Micros(NOW.0 + 365 * crate::ids::MICROS_PER_DAY);
    let missed = after_sweep(&target, false, 9, later);
    assert_eq!(missed, ItemState::Unresolved { reason: ARCHIVE_OUTCOME_UNKNOWN.into(), next_at: next_retry(9, later) });
    assert_eq!(closure(&settle(items.clone(), |t| matches!(t, ItemTarget::ArchiveInFlight { .. }), missed)), None, "a year of misses proves nothing");
    let found = after_sweep(&target, true, 9, later);
    assert_eq!(closure(&settle(items, |t| matches!(t, ItemTarget::ArchiveInFlight { .. }), found)), Some(ErasureState::FullyErased));
}

// F2: a known outcome arriving after the cascade resolves the in-flight item; a
// completed write also gets a fresh delete, reopening a saga closed meanwhile.
#[test]
fn an_archive_acknowledgement_after_the_cascade_reopens_cleanup() {
    let object = object(StoreKind::Gcs, "a/1/soniox.json");
    let flight = item(ItemTarget::ArchiveInFlight { object: object.clone() }, ItemState::Unresolved { reason: ARCHIVE_OUTCOME_UNKNOWN.into(), next_at: NOW });
    let other = item(ItemTarget::ArchiveInFlight { object: self::object(StoreKind::Gcs, "a/2/soniox.json") }, ItemState::Pending);
    let items = vec![flight.clone(), other];

    let ack = acknowledge_archive(&record(ErasureState::LocalErased), &items, &object, ArchiveWrite::Written, NOW);
    assert_eq!(ack.save.len(), 1);
    assert_eq!((ack.save[0].id, &ack.save[0].state), (flight.id, &ItemState::Confirmed { at: NOW }));
    assert_eq!(ack.insert.iter().map(|i| (&i.target, &i.state)).collect::<Vec<_>>(), vec![(&ItemTarget::StorageObject { object: object.clone() }, &ItemState::Pending)]);
    assert!(!ack.reopen);

    let closed = acknowledge_archive(&record(ErasureState::FullyErased), &[], &object, ArchiveWrite::Written, NOW);
    assert!(closed.reopen && closed.insert.len() == 1, "a late completion reopens a closed saga");

    let unsent = acknowledge_archive(&record(ErasureState::LocalErased), &items, &object, ArchiveWrite::NotWritten, NOW);
    assert_eq!((unsent.save.len(), unsent.insert.len(), unsent.reopen), (1, 0, false));
    assert_eq!(acknowledge_archive(&record(ErasureState::FullyErased), &items, &object, ArchiveWrite::InFlight, NOW), ArchiveAck::default());
}

// Phase order: identity only after phase 0 is done, the rest after the identity.
#[test]
fn phases_gate_each_other() {
    let mut items = vec![
        item(ItemTarget::LegacyStorageObject { object: object(StoreKind::Supabase, "a") }, ItemState::Pending),
        item(ItemTarget::Identity, ItemState::Pending),
        item(ItemTarget::StorageObject { object: object(StoreKind::Gcs, "b") }, ItemState::Pending),
    ];
    assert_eq!(runnable(&items, NOW).len(), 1);
    items[0].state = ItemState::Absent { at: NOW };
    assert!(matches!(runnable(&items, NOW)[0].target, ItemTarget::Identity));
    items[1].state = ItemState::Confirmed { at: NOW };
    assert!(matches!(runnable(&items, NOW)[0].target, ItemTarget::StorageObject { .. }));
}

// C-11: blocked, retrying, unresolved and unsupported items keep the saga open;
// approved exceptions close it, but never as fully erased.
#[test]
fn closure_rule() {
    let target = || ItemTarget::StorageObject { object: object(StoreKind::Gcs, "k") };
    let done = item(target(), ItemState::Confirmed { at: NOW });
    for open in [
        ItemState::Pending,
        ItemState::BlockedUntil { until: None, reason: "retention policy".into() },
        ItemState::Retrying { attempts: 1, last_error: "503".into(), next_at: NOW },
        ItemState::Unresolved { reason: "two matches".into(), next_at: NOW },
        ItemState::Unsupported { reason: NO_PROVIDER_DELETE_API.into() },
    ] {
        assert_eq!(closure(&[done.clone(), item(target(), open.clone())]), None, "{open:?}");
    }
    assert_eq!(closure(&[done.clone(), item(target(), ItemState::Absent { at: NOW })]), Some(ErasureState::FullyErased));
    let approved = ItemState::ApprovedException { reason: "hold".into(), approved_by: "ops".into(), at: NOW };
    assert_eq!(closure(&[done, item(target(), approved)]), Some(ErasureState::ErasedWithExceptions));
}

// PRV-D01: the view names every open problem; before the cascade it names billing's blockers.
#[test]
fn honest_views() {
    let items = vec![item(
        ItemTarget::ProviderCopy {
            provider: Provider::Deepgram,
            remote: media_ports::RemoteIdentity { provider: media_ports::Provider::Deepgram, project: "p".into(), id: "r1".into() },
        },
        ItemState::Unsupported { reason: NO_PROVIDER_DELETE_API.into() },
    )];
    assert_eq!(
        view(&record(ErasureState::LocalErased), &items, None),
        ErasureView::LocalDataErased { pending: 1, exceptions: vec![format!("deepgram copy r1: {NO_PROVIDER_DELETE_API}")] }
    );
    let status = ErasureStatus {
        started_at: NOW,
        billing_erased_at: None,
        pending: vec![billing_core::erasure::TerminationTarget {
            gateway: "iyzico".into(),
            environment: "live".into(),
            subscription_ref: "sub-1".into(),
            customer_ref: Some("c".into()),
        }],
        confirmed: vec![],
        unbound_plan_payments: vec![42],
    };
    assert_eq!(
        view(&record(ErasureState::Requested), &[], Some(&status)),
        ErasureView::Requested {
            waiting_for: vec![
                "subscription sub-1 at iyzico: not yet confirmed stopped".into(),
                "plan payment 42: no verified subscription binding to stop".into(),
            ]
        }
    );
}

// Remote retries back off 1, 2, 4 ... minutes, capped at six hours.
#[test]
fn retry_backoff() {
    let minutes = |a| (next_retry(a, NOW).0 - NOW.0) / crate::ids::MICROS_PER_MINUTE;
    assert_eq!((minutes(0), minutes(1), minutes(3), minutes(20)), (1, 2, 8, 360));
}
