# Resource safety slice (F1, F2, F7, F9 model side): handoff

Owner: p1W (Opus). The source is written but has not been built or tested. Every claim below is a source claim until the lead runs the suites.

Lead rulings applied:
- F2 uses a durable write state, with no clock or margin.
- F1 follows option (b): an upload stays unresolved in flight until an operator approves an exception. Nothing is approved automatically.
- An archive key is unique per intent.

There are no clocks, grace periods or windows in this slice. Upload finality research is in `upload-finality-evidence.md` (p1Z).

## Protocol

```mermaid
sequenceDiagram
  participant A as apply tx
  participant P as permit tx
  participant S as object store
  participant K as ack tx
  participant E as erasure
  A->>A: lock_barrier none, record intent in_flight (unique key)
  A-->>P: commit, then
  P->>P: lock_barrier none = permission (else ack not_written, no PUT)
  P->>S: one PUT, never retried
  S-->>K: Ok = written, Rejected = not_written, other = unknown (no ack)
  K->>K: lock_barrier: before manifest, settle row; after cascade, resolve ArchiveInFlight item, insert delete if written, reopen closed saga
  E->>E: manifest: delete every archive object; in_flight adds ArchiveInFlight
  E->>S: sweep ArchiveInFlight: present, then delete, resolves (key unique to this PUT); a miss stays Unresolved forever
```

## What p1W changed (own files)

### `transgate/core/src/privacy/erasure.rs`

- `ItemTarget` gains two variants:
  - `ArchiveInFlight { object }`;
  - `LegacyAccountInventory { bucket }`, with `LEGACY_JOBS_BUCKET = "jobs"`.
  - `UploadResweep { object, not_before }` keeps its name and fields.
- `phase()`:
  - both legacy targets are phase 0;
  - an `UploadResweep` or `ArchiveInFlight` on a Supabase object is also phase 0.
- `build_items`:
  - Every archived payload, media object and issued upload gets one immediate delete, deduplicated by `ObjectRef`. This includes expired uploads and uploads for removed drafts (F1).
  - Each object of an issued upload gets one `UploadResweep`. Its `not_before` is the object's latest `expires_at`. The item is `BlockedUntil` that time while a URL is still valid, else `Pending`.
  - Each `in_flight` archive gets one `ArchiveInFlight`, `Pending`.
  - `LegacyAccountInventory` is always added.
- `after_sweep(target, found, attempts, now) -> ItemState`. This is the only finality rule, and p1X's `run_item` delegates to it:
  - `ArchiveInFlight` found: `Confirmed`.
  - `ArchiveInFlight` not found: `Unresolved(ARCHIVE_OUTCOME_UNKNOWN)`, swept again with backoff.
  - `UploadResweep`, found or not: always `Unresolved(UPLOAD_OUTCOME_UNKNOWN)`, swept again with backoff, so late landings keep getting deleted.
- `acknowledge_archive(record, items, object, write, now) -> ArchiveAck { save, insert, reopen }`:
  - `written` resolves `ArchiveInFlight` (with `attempts + 1`), inserts a fresh delete, and sets `reopen` when the saga is `FullyErased` or `ErasedWithExceptions`;
  - `not_written` only resolves the item;
  - `in_flight` is a no-op.

### `transgate/core/src/jobs/app/resources.rs`

- `streaming_url` runs `guard` again after signing. A URL signed while the barrier committed is never returned (F7).
- `archive_intent(&Job, file) -> ArchivedPayload`:
  - The intent is `in_flight`.
  - The key comes from `ingest::archive_key(job, Uuid::new_v4(), file)`, so it is unique per intent.
- `archive_payload(&intent, body)`:
  - First `permit_archive`: its own transaction, which proceeds only when `lock_barrier` returns `None`. If permission is refused or the store is not configured, the write is acknowledged `NotWritten` and nothing is sent.
  - Then one PUT:
    - `Ok` is acknowledged `Written`;
    - `Err(Rejected)` is acknowledged `NotWritten`;
    - any other error sends no acknowledgement, so the intent stays in flight.
- `acknowledge_archive`:
  - It is one transaction, with no network inside.
  - It runs `lock_barrier` first. With no barrier, or a barrier still `Requested`, it calls `settle_archived_payload`. Otherwise it applies `erasure::acknowledge_archive` with `save_erasure_item`, `insert_erasure_items` and `save_erasure`, and reopens the saga as `LocalErased` with `closed_at = None`.
  - If the acknowledgement fails, the intent stays in flight. That state is still honest.
- Fact: the adapters never retry a PUT (`reqwest::retry::never()` at `crates/media-adapters/src/http.rs:30`). That is why there is one dispatch per intent.

### Tests, written first and driven through the real use cases with scripted schedules

`transgate/core/src/jobs/app/resources/tests.rs`:

| Test | What it proves |
|---|---|
| `an_archive_put_completing_after_the_cascade_is_acknowledged_and_deleted` | The PUT hangs while erasure runs every phase and sweeps. The PUT then completes, its acknowledgement reopens cleanup, and the object is deleted before `FullyErased`. |
| `a_sweep_racing_the_acknowledgement_keeps_the_acknowledged_state` | A sweep loads the item and its HEAD misses. Before the sweep saves, the PUT completes and its acknowledgement commits. The miss must not overwrite the acknowledgement, and the saga reaches `FullyErased` once the inserted delete runs. This covers p1X's stored-`attempts` check in `run_remote`. |
| `a_late_archive_completion_reopens_an_exception_labeled_closure_without_losing_identity` | D15 follow-up, saved 2026-10-25 and **not run by p1W**: a gated PUT remains unresolved through the real cascade, `OperatorApproval` closes it as `ErasedWithExceptions`, then the PUT's real `Written` acknowledgement reopens the same erasure id as `LocalErased` and inserts a pending delete for the exact persisted `ObjectRef`. It retains the approved item's object/audit identity, account barrier, and identity-deletion item/timestamps. After cleanup the object is absent and the explicit exception audit remains `ErasedWithExceptions`, never silently `FullyErased`. Lead selector: `cargo test --locked -p transgate-core jobs::app::resources::tests::a_late_archive_completion_reopens_an_exception_labeled_closure_without_losing_identity -- --exact`. |
| `an_unacknowledged_archive_write_stays_pending_until_its_object_is_found` | The PUT's answer is lost. A month of misses never closes the saga. An older settled archive at the source's shared key is deleted, and it does not resolve the new write. The landed object is found, deleted, and only then is the saga closed. |
| `the_archive_intent_precedes_the_put_and_outlives_its_job` | The intent is `in_flight` when the PUT starts and `written` after it. The object is still erased after its job row is gone. |
| `an_expired_upload_of_a_removed_draft_is_deleted_and_swept_again` | The upload object is deleted at once, and a late write is swept again. The saga stays open with `UPLOAD_OUTCOME_UNKNOWN`. |
| `no_capability_is_returned_across_the_erasure_barrier` | The barrier commits during signing. `streaming_url` returns `ErasureInProgress`, and `init_upload` refuses. |

`transgate/core/src/privacy/erasure/tests.rs`:

- `manifest_phases`
- `the_legacy_folder_is_always_inventoried`
- `every_issued_upload_is_deleted_and_swept_after_its_last_url_expires`
- `no_sweep_resolves_an_upload`
- `an_unacknowledged_archive_write_is_resolved_only_by_evidence`
- `an_archive_acknowledgement_after_the_cascade_reopens_cleanup`

## Integrated by p1X (source only, not built)

p1X integrated `ArchiveWrite`, `ArchivedPayload.write`, `settle_archived_payload`, the apply call sites, the fake, `run_item` through `after_sweep`, and the inventory. He also did the three follow-ups below. He added `privacy::app::tests::an_unobservable_upload_closes_only_by_operator_approval`.

1. `jobs/ingest.rs`: `pub fn archive_key(job: &Job, write: uuid::Uuid, file: &str) -> String`, producing `{account}/{job}/{write}/{file}`. Update its test in `ingest/tests.rs:81`, and `the_payload_archive_uses_the_archive_buckets_adapter`, which asserts the old key; read the key from `d.archived` instead.
2. `privacy/app.rs` `run_remote` stale write-back. Items are loaded before the network, and an acknowledgement can commit in between. Inside the save transaction, skip any loaded item whose stored `attempts` differs from the loaded value. Otherwise a sweep that missed before the acknowledgement would overwrite `Confirmed` with `Unresolved`.
3. `privacy/app.rs` `approve_exception`. The lead keeps operator-approved exceptions explicit, but it currently accepts only `Unsupported` and `BlockedUntil`. Also accept `Unresolved` items whose target is `UploadResweep` or `ArchiveInFlight`. Without that, those sagas can never close. The `ExceptionPolicy` pre-approval must not apply to them.

## Needed from p1R

- Confirm that `lock_barrier` takes the same erasures row lock (FOR UPDATE) as `lock_erasure`. The acknowledgement and the manifest capture serialize on it.
- When no barrier row exists, the permit transaction only reads. If an erasure opens after the permit, its manifest sees the `in_flight` row. That is safe.

## Observable difference from the source

JOB-044 in the source writes `jobs/{user_id}/{job_id}/{file}` with upsert. The archive is now written to the configured archive bucket at `{account}/{job}/{write-id}/{file}`, one immutable object per result, with no upsert.

An archive is visible only to support and operators; no product screen reads it. The rule row JOB-044 needs this recorded as a decided difference.

## Residual risk, stated and not hidden

- An upload URL has no acknowledgement. Every account that was issued an upload URL stays `LocalDataErased`, with the named upload items, until an operator approves an exception. That ends at `ErasedWithExceptions`.
- An archive PUT whose answer is lost, and whose object never appears, stays `Unresolved` until an operator approves an exception. If its acknowledgement arrives later, cleanup reopens. If it lands without an acknowledgement after the approval, that is exactly the residual risk the operator approved.

## D9 LLM dispatch manifest seam (2026-10-25, source only)

Lead released the integration hold after run307 passed 239 and authorized the
payload-free dispatch inventory. p1W changed only
`transgate/core/src/privacy/erasure.rs` and its own manifest tests for this seam.
No check or formatter was run by p1W; no passing result is claimed for it.

- `AccountResources.llm_dispatches: Vec<ai::ports::LlmDispatch>`.
- `ItemTarget::LlmDispatch { generation: uuid::Uuid, provider: String, remote_ids: Vec<String>, unkept_id_observed: bool }`;
  serde kind `llm_dispatch`, execution phase 2; the evidence bit has `#[serde(default)]`.
- `LLM_RETENTION_UNVERIFIED` is `"LLM provider retention is unverified"`.
- Every dispatch is captured, even without a surviving job/generation.
  `Possible` becomes `Unresolved` with the named retention reason and retry
  scheduling; `NotDispatched` becomes `Absent` **only when `remote_ids` is empty
  and `unkept_id_observed` is false**. Retained ids or the evidence bit keep
  even contradictory nondispatch state unresolved. The owner view names the
  provider, generation and retained ids, and explicitly reports unretained
  observed ids when the bit is true; never prompt/question/transcript.
- p1X owns the ports/fake, worker and privacy-app integration; p1R owns the
  durable inventory and restricted late-target update. The ports retain at
  most `MAX_DISPATCH_REMOTE_IDS = 8` distinct ids, each nonempty and at most
  `MAX_DISPATCH_REMOTE_ID_BYTES = 256` bytes, preserving first-seen order.
  The first nonempty observed id that cannot be retained sets the durable,
  monotonic `unkept_id_observed` bit. Later `NotDispatched, None` cannot clear
  this evidence. The manifest copies both the complete retained list and bit;
  no latest-id overwrite, invented id or truncation.
- Lead D9 allows **explicit operator approval only**, producing
  `ErasedWithExceptions`. No `ExceptionPolicy` approval or provider deletion API
  is invented. p1X owns that approval/run-item wiring, not p1W.
- Late reconciliation must persist target/state/attempts under `lock_barrier`
  through the restricted `save_erasure_item_target`, preserve previously seen
  ids, and bump attempts against stale sweep write-back. A nondispatch outcome
  cannot erase known dispatch evidence or recreate deleted content.

Manifest tests were added before the model:

- `privacy::erasure::tests::possible_llm_dispatches_keep_erasure_open_and_name_every_observed_identity`
  covers no observed id, distinct request/completion ids, and a contradictory
  `NotDispatched` with an observed id; no surviving jobs, retained identities,
  open closure and owner-visible uncertainty are checked.
- `privacy::erasure::tests::a_definitively_nondispatched_llm_request_is_absent_in_the_manifest`
  covers the definitive absence outcome without blocking otherwise-complete erasure.
- `privacy::erasure::tests::an_unkept_llm_id_keeps_erasure_unresolved_without_retained_ids`
  covers empty retained ids with the evidence bit true, for both `Possible`
  and contradictory `NotDispatched`: manifest propagation, no closed outcome,
  one pending item and owner-visible unretained evidence.

Sol completed this bounded marker-model repair over the existing Opus resource
implementation. p1X/p1R received the COHERENT/FROZEN source handoff; source is
held for the lead's integrated core+PG run. No p1W check was run.

Evidence boundary (lead-reported): existing red641 and regression-first source
remain retained. Attempts662/663 stopped at compilation, so overflow BEFORE-FIX
behavior is **UNOBSERVED**; neither attempt executed migration0006. The lead
superseded the capture/revert holds and requested the complete correction,
not another manufactured red capture. No passing result is claimed here.
