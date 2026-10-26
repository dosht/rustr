# P3 Opus critique of the Astra proposal, with convergence contract

Design review only. Nothing was built or run. I read `astra/proposal.md` and discussed it directly with Astra (pane w23:p1S). Lead constraints from the dispatch are applied throughout. Section 1 corrects my own proposal, section 2 reviews Astra's, section 3 is the convergence contract, and section 4 lists the conflicts that only lead or mu can decide.

## 1. Corrections to my own proposal

1. **Soniox duration wording.** My section 1.2 item 1 said the live webhook path "almost certainly" bills by the largest token end. Here is the corrected statement, with source facts kept apart from documentation and from inference:
   - Source fact: `buildSonioxPayload` uses the reported `audio_duration_ms` when it is positive, otherwise the largest token `end_ms` (`frontend/src/utils/soniox.ts:150-172`).
   - Source fact: the webhook route passes the webhook body's `audio_duration_ms` (`frontend/src/pages/api/soniox/callback.ts:139-143`). The recovery sweep passes its status poll.
   - External documentation lists only `id` and `status` in the webhook body ([webhooks](https://soniox.com/docs/stt/async/webhooks)).
   - Whether Soniox actually sends the field at runtime is an inference until authorized evidence exists.
   - The new system preserves the observation origin per path. A GET made to authenticate or fetch a webhook result must never supply billable minutes. M2 stays a mu decision.
2. **Pins.** My "pinned provider is the only candidate" contradicts `frontend/src/services/unified-transcription/services/transcription.ts:542-558`. When a submit fails and fallback is enabled, `handleFailover` runs even for a pinned job. Corrected rule in contract C-4.
3. **taskr scanner scenario.** Withdrawn per the lead constraint. ClamAV and VirusTotal are unrelated products. Replaced by contract C-9.
4. **No inbox.** Withdrawn. A narrow durable callback inbox is adopted (C-6). It removes the 30-minute recovery latency and the source's permanent failure of paid jobs on transient errors, at small cost.
5. **Account erasure as a "P2 prerequisite".** Reclassified (C-8). It is a P3 dependency on a billing module extension, not an unimplemented P2 BIL row. BIL-042 covers one subject's hard delete, which `release_subject_charges` already handles. Account-level deletion of payments, packages and usage comes from PRV-005/006/007.
6. **Phase manifest.** My section 1.1 counted browser-mechanics rows as P3-green after presentation tests. That over-claims, so I adopt Astra's clause-level manifest (C-1).

## 2. Review of the Astra proposal

### Adopted from Astra

- Clause-level phase manifest. Rows with P5, P6, P7 or P9 clauses are not green at P3.
- History window as an explicit calendar computation with a supplied time zone and calendar context. Golden cases cover month-end overflow; UTC clamping would change behavior.
- Retention transaction:
  - locks candidate accounts in sorted order and rereads preferences under the lock;
  - preference writes take the same account lock;
  - exactly 100,000 lines deletes, more than 100,000 aborts;
  - the run record is created before the work, and failure, skip and abort outcomes are written in an independent transaction.
- Savepoint around the deferred debit, so a storage error in the debit cannot poison transcript delivery. The `charged` or uncharged outcome commits with completion.
- Lock order: capacity lock, then account, then job for submit claims. Callbacks never take the capacity lock.
- Unconditional Soniox last resort replaced by capacity wait or an explicit compatibility failure, recorded.
- Erasure coverage:
  - covers legacy raw payloads in the Supabase `jobs` bucket;
  - covers outstanding presigned PUT capabilities through a re-sweep after the longest presign lifetime;
  - Soniox 409 means "pending cleanup", and a 404 counts as absence only for verified project credentials.
- Deepgram Nova-2 exception kept as host configuration with a synthetic fixture, rather than discarded under LNG-D03.
- Explicit Soniox model configuration, recording both the requested and the response model.

### Objections raised with Astra, and how each ended

1. **Free-tool processor phase: still open (D-1).** Astra places API-022..030, CRN-008/009 and an FFmpeg processor backend in P3, citing the brief's obligation wording. I prefer P6, next to the public tool pages (with UPL-029 and EXP-017): the PRD P3 deliverables and done condition list none of them, and these are API/CRN/FLG rows rather than transcription or privacy rows. Both placements keep every obligation in the manifest as unverified until it is built. Lead chooses the sequencing.
2. **SSRF staging fetch of external audio: resolved, Astra conceded.** Our server never fetches customer audio; the provider does, so staging would add our own SSRF and bandwidth surface. Agreed:
   - keep direct provider fetch;
   - keep the source URL validation for audio URLs;
   - record the DNS-failure policy and the unproved provider redirect/DNS boundary as explicit evidence limits;
   - fail-closed validation with connect-time address pinning applies to outbound requests our own server makes (P7 customer callbacks).
3. **Immutable submission copy: resolved, Astra conceded.** No copy requirement. Record the object generation where the backend exposes it, and re-sweep for recreated objects after the longest presigned PUT lifetime.
4. **AI consent record: resolved, Astra conceded.** The consent API belongs to P4, and its clauses stay visible as unfinished. P4 must participate in erasure for real; a fake consumer does not count.
5. **Draft removal deleting objects: resolved.** JOB-038's no-file-delete outcome stays unless a correction is recorded. Account erasure does delete objects.

### Correctness requirements from Astra, all accepted

- **Unsupported remote deletion.** A saga with any unsupported, retained or unresolved item closes as "erased with recorded exceptions", listing each item. It is never labeled fully erased. Deepgram content without an opt-out and unexpired GCS-retained objects are the expected cases.
- **Combined owner discovery.** Account erasure discovers the union of affected owners before taking any lock. It never loops `release_subject_charges` across jobs (C-8).
- **Supabase artifacts before identity.** Legacy Supabase `jobs` bucket artifacts are deleted through the storage API, with per-object outcomes, before GoTrue deletion: Supabase documents that user-owned Storage objects can block user deletion. The same rule applies to GCS and R2 items that a deployment ties to the identity.

## 3. Convergence contract

These items fix the shared interfaces and behavior before developer fan-out. Each item is agreed with Astra unless marked otherwise.

**C-1 Scope and gates.**
- A clause-level P3 manifest lists every JOB/UPL/TRX/TRL/EDT/EXP/PRV/LNG/CRN/EML clause as one of: P3-proved, recorded correction or retirement (a PRD section 2 reference plus a test of the new outcome), or owned by a later phase.
- `check-prd.py transcription` checks the exact manifest. It passes only on executable evidence, with database tests included and runtime smoke required.
- The global 526-row ledger, parity, module, Rustr and Transgate gates stay strict and red until every clause is proved. Sequencing is not deletion.

**C-2 Crates.**
- `transgate/core`: pure product code, including `languages.rs` (PRD 7.4).
- A Transgate Postgres crate: schema `transgate`, its own migration history, restricted runtime role.
- `crates/media-ports`: provider and storage protocol value types and ports only. No job statuses, retry policy, language picker, billing or privacy policy.
- `crates/media-adapters`: Soniox, Deepgram, GCS/R2/S3 presign, and legacy Supabase object access if lead keeps it (D-2).
- One small shared Postgres lock and schedule-slot adapter crate (agreed with Astra; no scheduler engine). taskr exercises it and the billing `erase_account` extension on its own database, with no `taskr-core` change.
- `transgate/server`, `crates/taskr-work`, export bytes, and the YouTube/TranscriptAPI adapters.
- No generic workflow engine, plugin registry or reusable transcription module. Final crate names are a lead naming pass.

**C-3 Job aggregate.**
- All 12 persisted statuses with no folding. Imported `uploaded/processing/ready/speaker_labeling` and RunPod rows are readable and soft-deletable, with no runtime producer.
- Fields: revision, owner, source, options, duration, `pinned_provider` separate from `requested_provider`, and one live attempt (number, provider, phase, lease, remote id).
- `deferred_charge` takes the values `none | pending | charged | refused | no_duration`.
- Imported data enters only through import authority.

**C-4 Routing.** One pure router:
1. A stored pin is the first attempt, executed as-is (TRX-001).
2. Otherwise apply, in order: the non-translation duration override (required configuration; production 200, code default 250), the non-translation `.qta` override, a valid API preference, Soniox, then the chain.
3. Translation stays Soniox-only with a known source and the first target.
4. Auto-detect stays Soniox-only.
5. A failed or unavailable attempt fails over to compatible candidates unless `fallbackEnabled` is false (TRX-014, DEF-06 corrected so failover can actually submit). A removed RunPod pin counts as an unavailable first attempt.
6. A capability-incompatible pin, such as a translation job pinned to a provider that cannot translate, is treated as a failed attempt. That is a recorded correction.
7. If no compatible configured provider exists, the job fails with an explicit compatibility error. If only capacity is missing, it waits as pending.
8. Health scoring is retired (PRD section 2).

**C-5 Concurrency.**
- Lock order is capacity lock (submit claims only), then billing account, then job rows ascending, then revision compare-and-swap.
- Every job save takes the account lock, because entering or leaving `completed` changes freemium eligibility.
- No network call inside any transaction.
- The attempt claim commits before the provider call. Results for stale attempts are fenced.
- Deepgram claimed plus live attempts stay at or below 4.
- We promise one local terminal application and one charge per eligible transition. We do not promise exactly one provider bill, because Deepgram has no lookup API.

**C-6 Callbacks.**
- Authentication is mandatory: the Soniox custom header and Deepgram basic-auth credentials per attempt, both recorded security corrections.
- The authenticated envelope commits to a narrow inbox before the 2xx: Soniox `id`, `status`, attempt, the optional webhook-reported `audio_duration_ms` exactly as received (absent stays absent), and its observation origin (`webhook` or `recovery_poll`); the Deepgram body is retained only until ingestion commits. A retried ingestion therefore bills exactly as C-7 requires.
- The intake transaction locks the account first and checks the erasure barrier.
- Ingestion is one transaction: lines, duration rule, deferred charge, completion and outbox.
- Transient failures retry. Deterministic failures keep the source coded `failed` outcomes, including `callback_ingestion_error`. The JOB-029 change is recorded.
- Late results for deleted, canceled, failed or completed jobs write no lines and only contribute remote identities to cleanup.
- Raw payload archival stays best effort (JOB-044) and is listed in the erasure manifest.

**C-7 Money.**
- Soniox keeps source origin precedence: on the webhook path, a positive reported `audio_duration_ms`, else the largest token end; on the recovery path, the poll's observation. A GET never enriches webhook billable minutes.
- Deepgram uses `metadata.duration`.
- Deferred debits use `charge_strict` (no bypass), inside a savepoint.
- A shorter provider length lowers the duration only; a longer one changes nothing.
- YouTube jobs are never billed.
- Start composes `web_precheck`/`api_precheck`, `web_guard` and `charge_job_starts` in one transaction with the status compare-and-swap.

**C-8 Erasure.**
- P3 depends on a billing module extension, `erase_account(tx, account, subjects)`. As one combined first billing call, it discovers every affected owner from three sets: rows by account, rows by the account's subjects, and rows on the account's packages. It locks the owners ascending, rechecks the set, then deletes usage, legacy usage, packages, payments and subscriptions, and finally the account. `billing_info` keeps its source `SET NULL` (PRV-007).
- Calling `release_subject_charges` per job is not sufficient. It must be the first billing call, so repeated calls could lock a lower-id owner after a higher one.
- The extension needs the module's two consumers: Transgate account deletion and taskr's sample account erasure.
- Grants for these deletes and for the host customer directory are part of the extension. P2's BIL rows are not reopened.

**C-9 taskr.**
- Explicit "audio work" sample: upload URL, start with a persisted work id and expected revision, provider submission and authenticated callback, lease takeover, a two-adapter fallback, scheduler locking across two processes, sample retention, and local erasure with a failed and then retried external deletion.
- It uses the same media ports and adapters against local fixture endpoints, with its own small `Work` aggregate in `crates/taskr-work`, a separate database, and no `taskr-core` change.
- Ordinary task creation is unchanged. The existing `taskr-billing/src/work.rs` contract stays as it is.
- No language picker, exports, YouTube, AI or free tools.

**C-10 Editing.**
- Source last-write-wins per line (EDT-002) is preserved as the explicit outcome decision: no expected-revision 409 for users.
- Internally, every edit runs under the account and job lock with a lifecycle check. It updates only existing rows, never inserts or upserts, so an edit after retention or erasure cannot resurrect content.
- Optimistic line revisions would be a new user-visible outcome and need mu.

**C-11 Erasure barrier.**
- A minimal tombstone (account UUID, timestamps, item outcomes) persists after the saga closes. It keeps no name, email or content. The account UUID is still a linkable identifier, so the tombstone is pseudonymous, never evidence of anonymization or of holding no personal data. It rejects stale JWTs, delayed callbacks and provisioning or directory refreshes.
- It must outlive the JWT lifetime, the presign re-sweep and the provider callback retry windows.
- Removing it is an explicit operating-policy decision, never automatic.
- Saga order:
  1. Barrier and local erasure in one transaction, including the billing extension.
  2. Legacy Supabase `jobs` artifacts deleted through the storage API.
  3. GoTrue deletion: accepted or pending, never a false success.
  4. Storage, provider and late-bound items, each confirmed, absent, `blocked_until`, unsupported or retrying.
  5. The user-facing state may read "local data erased, with exceptions" while remote work is pending. Blocked, retrying and unresolved items stay durably runnable, and nothing auto-closes them away. Only an approved unsupported or retained exception, recorded with its reason, can finish an item without confirmation. The saga is "fully erased" only when every item is confirmed or absent; otherwise it finishes as "erased with recorded exceptions" once every remaining item is an approved exception.

**C-12 Retention.**
- Source predicate and schedule (02:00 UTC; days 7..365; `now - days - 1 day`; completed jobs only; owner opt-in true; null means keep; `unlimited_history` is not an exemption).
- Plan and dry run share the predicate.
- Audit statuses include `skipped` and `aborted`, with no public grants.
- The scheduler's own run history replaces the pg_cron history purge (Sunday 04:00 UTC, 90 days).
- One slot claim per UTC slot plus an advisory lock on a dedicated connection.

**C-13 Storage.**
- Store an opaque object reference, never a presigned URL. Keys keep source paths and TTLs (900 s for upload and player, 3600 s for providers).
- The storage provider is required configuration.
- GCS uses HMAC V4 interop; R2 uses SigV4.
- A GCS retention-policy refusal is recorded as `blocked_until`, never as success.

**C-14 Developer slices.** Five, matching Astra's table: domain/presentation; Postgres/privacy runtime; protocol adapters (media crates); hosts/taskr/integration; and the processor only if lead places it in P3 (D-1). Interfaces C-2..C-8 are frozen before fan-out.

## 4. Conflicts requiring lead or mu

| Id | Conflict | Opus position | Owner |
|---|---|---|---|
| D-1 | Free-tool FFmpeg processor backend (API-022..030, CRN-008/009, FLG-008) in P3 or P6 | Astra prefers P3 (brief obligations); Opus prefers P6, with the tool pages, UPL-029 and EXP-017. Either way all clauses stay unverified in the manifest until built | lead |
| D-2 | Legacy Supabase `jobs` bucket: keep a Supabase storage adapter, or migrate objects in P9 | The adapter may wait only if it is explicitly sequenced behind P9 artifact migration plus erasure proof. The affected final clauses cannot pass on synthetic host-only deletion. Before cutover, these items are API-deleted before GoTrue deletion and are an erasure manifest kind | lead (P9 dependency) |
| D-3 | Deepgram `mip_opt_out=true` (zero retention, possible price change) | Astra recommends it as a privacy correction; Opus defaults to preserve | mu |
| D-4 | EU residency promise (PRV-057) versus Whisper's absence from the EU endpoint for the eight Deepgram-only languages | keep all 61 languages and claim no EU Whisper processing | mu |
| D-5 | Retention promise reconciliation (14 days, one month, 30 days, 31 days, GCS 31-day lock); subscription and `billing_info` handling at erasure; how long the erasure tombstone is kept | preserve source outcomes; keep the tombstone until a policy is set | mu |
| D-6 | Soniox webhook versus GET duration unification (M2; money) | preserve per-path origin | mu |
| D-7 | Line edits: source last-write-wins (agreed default) versus optimistic per-line revisions | preserve last-write-wins | mu, only if a change is wanted |
| D-8 | Cutover: unsigned legacy Deepgram attempts and imported RunPod in-flight rows | drain before cutover; no permanent unsigned bypass | lead (P9) |
