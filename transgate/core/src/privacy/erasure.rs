//! Account erasure (PRV-001..010 intended behavior, PRV-D01/D02, D2, D5, D9, D10).
//!
//! The saga follows billing's (`billing_core::erasure`): the request writes the
//! host barrier and opens billing's erasure in one transaction; verified
//! recurring subscriptions are then stopped (D10) outside any transaction; once
//! billing reports them stopped, one transaction captures the resource manifest
//! and runs billing's cascade first (D9, D5), then the host cascade. Remote work
//! then runs item by item. Nothing is reported as erased until it is confirmed
//! or absent, or an explicitly approved exception is recorded (C-11). The
//! barrier outlives completion (D9): it keeps the account UUID, timestamps and
//! outcomes only. It is pseudonymous security state, not anonymized data.

use billing_core::erasure::ErasureStatus;
use media_ports::{ObjectRef, RemoteIdentity};
use serde::{Deserialize, Serialize};

use crate::ai::ports::{DispatchDisposition, LlmDispatch};
use crate::ids::{AccountId, ErasureId, JobId, Micros};
use crate::jobs::domain::Provider;
use crate::jobs::ports::{ArchiveWrite, ArchivedPayload, IssuedUpload};

/// Everything the manifest must capture before the cascade removes the rows
/// that name it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountResources {
    pub jobs: Vec<JobId>,
    /// Upload and staged media objects.
    pub media: Vec<ObjectRef>,
    pub archived_payloads: Vec<ArchivedPayload>,
    /// Every attempt's remote identity, current and superseded, with its product provider.
    pub provider_copies: Vec<(Provider, RemoteIdentity)>,
    /// Attempts whose acceptance is unknown: no remote id yet, so only the
    /// client reference can find them later.
    pub unresolved_submissions: Vec<(Provider, String)>,
    pub issued_uploads: Vec<IssuedUpload>,
    /// Payload-free provider dispatch evidence, retained even after consent
    /// revocation or hard deletion removes its generation and content.
    pub llm_dispatches: Vec<LlmDispatch>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemTarget {
    /// Legacy Supabase `jobs` bucket artifacts go first: owned Storage objects
    /// can block GoTrue user deletion (D2).
    LegacyStorageObject { object: ObjectRef },
    /// Every object under the account's folder of the legacy Supabase bucket
    /// (F9, D2): imported archives exist only as objects. Done only when an
    /// authenticated, complete listing comes back empty.
    LegacyAccountInventory { bucket: String },
    /// The GoTrue identity.
    Identity,
    StorageObject { object: ObjectRef },
    ProviderCopy { provider: Provider, remote: RemoteIdentity },
    /// A submission whose acceptance is unknown; reconciled by client reference.
    UnresolvedSubmission { provider: Provider, client_reference: String },
    /// An upload URL signed for this object (F1), live, expired or for a removed
    /// draft. After the URL expires the object is swept again; a client PUT has
    /// no acknowledgement, so a miss never proves that none is still landing:
    /// the item stays unresolved, swept again with backoff, until an approved
    /// exception closes it.
    UploadResweep { object: ObjectRef, not_before: Micros },
    /// Our archive PUT (F2) whose outcome is not durably acknowledged. Resolved
    /// only by evidence: the acknowledgement (`acknowledge_archive`), or the
    /// object found present and deleted. A miss proves nothing.
    ArchiveInFlight { object: ObjectRef },
    /// Retention of a possibly dispatched LLM request is not observable through
    /// a provider delete API. Keep every distinct observed identity; only a
    /// definitive nondispatch or explicit operator approval can close it (D9).
    LlmDispatch {
        generation: uuid::Uuid,
        provider: String,
        remote_ids: Vec<String>,
        #[serde(default)]
        unkept_id_observed: bool,
    },
}

impl ItemTarget {
    /// Execution phase: lower phases must finish (or be approved exceptions)
    /// before a higher phase starts. Everything in the legacy Supabase store is
    /// phase 0, ahead of the identity it can block (D2).
    pub fn phase(&self) -> u8 {
        match self {
            ItemTarget::LegacyStorageObject { .. } | ItemTarget::LegacyAccountInventory { .. } => 0,
            ItemTarget::UploadResweep { object, .. } | ItemTarget::ArchiveInFlight { object }
                if object.store == media_ports::StoreKind::Supabase =>
            {
                0
            }
            ItemTarget::Identity => 1,
            _ => 2,
        }
    }
}

/// The legacy Supabase Storage bucket of imported jobs (D2, F9).
pub const LEGACY_JOBS_BUCKET: &str = "jobs";

pub const UPLOAD_OUTCOME_UNKNOWN: &str = "an upload begun with a URL issued before erasure may still land";
pub const ARCHIVE_OUTCOME_UNKNOWN: &str = "the archive write's outcome is not acknowledged";
pub const LLM_RETENTION_UNVERIFIED: &str = "LLM provider retention is unverified";

/// The state of an `UploadResweep` or `ArchiveInFlight` item after one sweep,
/// `found` telling whether the object was present (and is now deleted). Only
/// our own single archive PUT is resolved by finding its object; nothing is
/// resolved by a miss.
pub fn after_sweep(target: &ItemTarget, found: bool, attempts: i32, now: Micros) -> ItemState {
    match target {
        ItemTarget::ArchiveInFlight { .. } if found => ItemState::Confirmed { at: now },
        ItemTarget::ArchiveInFlight { .. } => ItemState::Unresolved { reason: ARCHIVE_OUTCOME_UNKNOWN.into(), next_at: next_retry(attempts, now) },
        _ => ItemState::Unresolved { reason: UPLOAD_OUTCOME_UNKNOWN.into(), next_at: next_retry(attempts, now) },
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ItemState {
    Pending,
    Confirmed { at: Micros },
    Absent { at: Micros },
    /// The store refuses before a date (GCS retention policy): retried after it.
    BlockedUntil { until: Option<Micros>, reason: String },
    /// A transient failure; retried with backoff.
    Retrying { attempts: i32, last_error: String, next_at: Micros },
    /// Evidence is incomplete (ambiguous ownership, several matches); stays runnable.
    Unresolved { reason: String, next_at: Micros },
    /// No delete capability exists (Deepgram). Finishes only as an approved exception.
    Unsupported { reason: String },
    /// An exception approved by policy or an operator, recorded with its reason.
    ApprovedException { reason: String, approved_by: String, at: Micros },
}

impl ItemState {
    pub fn is_done(&self) -> bool {
        matches!(self, ItemState::Confirmed { .. } | ItemState::Absent { .. } | ItemState::ApprovedException { .. })
    }

    pub fn is_exception(&self) -> bool {
        matches!(self, ItemState::ApprovedException { .. })
    }

    /// Runnable now: pending, or a retry/blocked/unresolved item whose time came.
    pub fn is_runnable(&self, now: Micros) -> bool {
        match self {
            ItemState::Pending => true,
            ItemState::Retrying { next_at, .. } | ItemState::Unresolved { next_at, .. } => now.0 >= next_at.0,
            ItemState::BlockedUntil { until, .. } => until.is_none_or(|u| now.0 >= u.0),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErasureItem {
    pub id: uuid::Uuid,
    pub erasure: ErasureId,
    pub target: ItemTarget,
    pub state: ItemState,
    pub attempts: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErasureState {
    /// Barrier written and billing's erasure opened; subscriptions are being
    /// stopped and the local cascade has not run. The data is already unreachable.
    Requested,
    /// Manifest and local cascade committed; remote items run.
    LocalErased,
    /// Every item confirmed or absent.
    FullyErased,
    /// Every remaining item is an approved exception; listed, never "fully erased".
    ErasedWithExceptions,
}

/// The barrier and the saga record in one row. It survives completion (D9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErasureRecord {
    pub id: ErasureId,
    pub account: AccountId,
    pub requested_at: Micros,
    pub state: ErasureState,
    pub local_erased_at: Option<Micros>,
    pub identity_deleted_at: Option<Micros>,
    pub closed_at: Option<Micros>,
}

/// What the account owner sees (PRV-D01 intended: honest, never a false success).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ErasureView {
    /// Access is closed; local data is erased once every verified subscription is
    /// confirmed stopped. `waiting_for` names what still blocks it.
    Requested { waiting_for: Vec<String> },
    /// Local data is gone; remote work continues. `exceptions` lists blocked,
    /// unresolved or unsupported items so the answer stays truthful.
    LocalDataErased { pending: u32, exceptions: Vec<String> },
    FullyErased,
    ErasedWithExceptions { exceptions: Vec<String> },
}

/// The manifest, captured with the cascade (C-11). Every object the account
/// may own is deleted at once, including the object of every upload URL ever
/// issued, expired or for a removed draft (F1). A write that may still land
/// keeps its own item: an upload URL until an approved exception, our archive
/// PUT until its outcome is acknowledged or its object is found (F2).
pub fn build_items(erasure: ErasureId, resources: &AccountResources, now: Micros) -> Vec<ErasureItem> {
    let mut targets: Vec<ItemTarget> = Vec::new();
    let deletes = resources
        .archived_payloads
        .iter()
        .map(|p| &p.object)
        .chain(&resources.media)
        .chain(resources.issued_uploads.iter().map(|u| &u.object));
    let mut seen: Vec<&ObjectRef> = Vec::new();
    for object in deletes {
        if !seen.contains(&object) {
            seen.push(object);
            targets.push(delete_target(object));
        }
    }
    targets.push(ItemTarget::LegacyAccountInventory { bucket: LEGACY_JOBS_BUCKET.into() });
    targets.push(ItemTarget::Identity);
    for (provider, remote) in &resources.provider_copies {
        targets.push(ItemTarget::ProviderCopy { provider: *provider, remote: remote.clone() });
    }
    for (provider, reference) in &resources.unresolved_submissions {
        targets.push(ItemTarget::UnresolvedSubmission { provider: *provider, client_reference: reference.clone() });
    }
    let mut in_flight: Vec<&ObjectRef> = Vec::new();
    for payload in resources.archived_payloads.iter().filter(|p| p.write == ArchiveWrite::InFlight) {
        if !in_flight.contains(&&payload.object) {
            in_flight.push(&payload.object);
            targets.push(ItemTarget::ArchiveInFlight { object: payload.object.clone() });
        }
    }
    // One re-sweep per object, after the last of its URLs expires.
    let mut uploads: Vec<(&ObjectRef, Micros)> = Vec::new();
    for upload in &resources.issued_uploads {
        match uploads.iter_mut().find(|(o, _)| *o == &upload.object) {
            Some((_, latest)) => *latest = Micros(latest.0.max(upload.expires_at.0)),
            None => uploads.push((&upload.object, upload.expires_at)),
        }
    }
    for (object, expires_at) in uploads {
        targets.push(ItemTarget::UploadResweep { object: object.clone(), not_before: expires_at });
    }
    targets
        .into_iter()
        .map(|target| {
            let state = match &target {
                ItemTarget::UploadResweep { not_before, .. } if not_before.0 > now.0 => {
                    ItemState::BlockedUntil { until: Some(*not_before), reason: "its upload URL is still valid".into() }
                }
                _ => ItemState::Pending,
            };
            new_item(erasure, target, state)
        })
        .chain(resources.llm_dispatches.iter().map(|dispatch| {
            let target = ItemTarget::LlmDispatch {
                generation: dispatch.generation.0,
                provider: dispatch.provider.clone(),
                remote_ids: dispatch.remote_ids.clone(),
                unkept_id_observed: dispatch.unkept_id_observed,
            };
            let state = match dispatch.disposition {
                DispatchDisposition::NotDispatched
                    if dispatch.remote_ids.is_empty() && !dispatch.unkept_id_observed => ItemState::Absent { at: now },
                DispatchDisposition::Possible | DispatchDisposition::NotDispatched => ItemState::Unresolved {
                    reason: LLM_RETENTION_UNVERIFIED.into(),
                    next_at: next_retry(0, now),
                },
            };
            new_item(erasure, target, state)
        }))
        .collect()
}

fn delete_target(object: &ObjectRef) -> ItemTarget {
    if object.store == media_ports::StoreKind::Supabase {
        ItemTarget::LegacyStorageObject { object: object.clone() }
    } else {
        ItemTarget::StorageObject { object: object.clone() }
    }
}

fn new_item(erasure: ErasureId, target: ItemTarget, state: ItemState) -> ErasureItem {
    ErasureItem { id: uuid::Uuid::new_v4(), erasure, target, state, attempts: 0 }
}

/// What a durable archive acknowledgement changes in a saga that already
/// captured its manifest (F2): items to save, items to insert, and whether a
/// closed saga reopens.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArchiveAck {
    pub save: Vec<ErasureItem>,
    pub insert: Vec<ErasureItem>,
    pub reopen: bool,
}

/// F2: the known outcome of our archive PUT, reported after the local cascade
/// removed its row. The in-flight item is resolved; a completed write gets a
/// fresh delete, which reopens a saga closed meanwhile (by an approved exception).
pub fn acknowledge_archive(record: &ErasureRecord, items: &[ErasureItem], object: &ObjectRef, write: ArchiveWrite, now: Micros) -> ArchiveAck {
    let mut ack = ArchiveAck::default();
    if write == ArchiveWrite::InFlight {
        return ack;
    }
    for item in items {
        if matches!(&item.target, ItemTarget::ArchiveInFlight { object: o } if o == object) && !item.state.is_done() {
            let mut resolved = item.clone();
            resolved.state = ItemState::Confirmed { at: now };
            // A sweep that loaded the item before this acknowledgement must not
            // write its older outcome over it: `attempts` marks the change.
            resolved.attempts += 1;
            ack.save.push(resolved);
        }
    }
    if write == ArchiveWrite::Written {
        ack.insert.push(new_item(record.id, delete_target(object), ItemState::Pending));
        ack.reopen = matches!(record.state, ErasureState::FullyErased | ErasureState::ErasedWithExceptions);
    }
    ack
}

/// Items that may run now, respecting the phases.
pub fn runnable(items: &[ErasureItem], now: Micros) -> Vec<&ErasureItem> {
    let lowest_open = items.iter().filter(|i| !i.state.is_done()).map(|i| i.target.phase()).min();
    let Some(phase) = lowest_open else { return vec![] };
    items.iter().filter(|i| i.target.phase() == phase && i.state.is_runnable(now)).collect()
}

/// Retry backoff for remote steps: 1, 2, 4 ... minutes, capped at 6 hours.
pub fn next_retry(attempts: i32, now: Micros) -> Micros {
    let minutes = 1i64 << attempts.clamp(0, 9);
    Micros(now.0 + minutes.min(360) * crate::ids::MICROS_PER_MINUTE)
}

/// Closure rule (C-11): fully erased only when every item is confirmed or
/// absent; with exceptions only when every remaining item is an approved one.
pub fn closure(items: &[ErasureItem]) -> Option<ErasureState> {
    if !items.iter().all(|i| i.state.is_done()) {
        return None;
    }
    if items.iter().any(|i| i.state.is_exception()) {
        Some(ErasureState::ErasedWithExceptions)
    } else {
        Some(ErasureState::FullyErased)
    }
}

fn describe(item: &ErasureItem) -> Option<String> {
    let what = match &item.target {
        ItemTarget::LegacyStorageObject { object } | ItemTarget::StorageObject { object } => {
            format!("stored object {}/{}", object.bucket, object.key)
        }
        ItemTarget::Identity => "sign-in identity".into(),
        ItemTarget::ProviderCopy { provider, remote } => format!("{provider} copy {}", remote.id),
        ItemTarget::UnresolvedSubmission { provider, client_reference } => format!("{provider} submission {client_reference}"),
        ItemTarget::LegacyAccountInventory { bucket } => format!("legacy storage folder in bucket {bucket}"),
        ItemTarget::UploadResweep { object, .. } => format!("upload to {}/{}", object.bucket, object.key),
        ItemTarget::ArchiveInFlight { object } => format!("archive write to {}/{}", object.bucket, object.key),
        ItemTarget::LlmDispatch { generation, provider, remote_ids, unkept_id_observed } => {
            let unkept = if *unkept_id_observed { "; additional provider ids were observed but not retained" } else { "" };
            format!("{provider} LLM dispatch {generation} (remote ids: {remote_ids:?}){unkept}")
        }
    };
    match &item.state {
        ItemState::BlockedUntil { reason, .. }
        | ItemState::Unresolved { reason, .. }
        | ItemState::Unsupported { reason }
        | ItemState::ApprovedException { reason, .. } => Some(format!("{what}: {reason}")),
        ItemState::Retrying { last_error, .. } => Some(format!("{what}: retrying ({last_error})")),
        _ => None,
    }
}

/// Billing's blockers before the local cascade (D10), in owner-readable words.
pub fn billing_blockers(status: &ErasureStatus) -> Vec<String> {
    let subscriptions = status
        .pending
        .iter()
        .map(|t| format!("subscription {} at {}: not yet confirmed stopped", t.subscription_ref, t.gateway));
    let payments = status
        .unbound_plan_payments
        .iter()
        .map(|p| format!("plan payment {p}: no verified subscription binding to stop"));
    subscriptions.chain(payments).collect()
}

/// `billing` is billing's erasure status, read in the same transaction.
pub fn view(record: &ErasureRecord, items: &[ErasureItem], billing: Option<&ErasureStatus>) -> ErasureView {
    let exceptions: Vec<String> = items.iter().filter_map(describe).collect();
    match record.state {
        ErasureState::Requested => ErasureView::Requested { waiting_for: billing.map(billing_blockers).unwrap_or_default() },
        ErasureState::FullyErased => ErasureView::FullyErased,
        ErasureState::ErasedWithExceptions => ErasureView::ErasedWithExceptions { exceptions },
        ErasureState::LocalErased => ErasureView::LocalDataErased {
            pending: items.iter().filter(|i| !i.state.is_done()).count() as u32,
            exceptions,
        },
    }
}

/// Exceptions the deployment pre-approves by policy, by reason code. Empty by
/// default: nothing is approved implicitly.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExceptionPolicy {
    pub approved_unsupported: Vec<String>,
}

impl ExceptionPolicy {
    pub fn approves(&self, reason: &str) -> bool {
        self.approved_unsupported.iter().any(|r| r == reason)
    }
}

/// Reason code for provider copies without a delete API (Deepgram, D3).
pub const NO_PROVIDER_DELETE_API: &str = "provider_has_no_delete_api";

#[cfg(test)]
mod tests;
