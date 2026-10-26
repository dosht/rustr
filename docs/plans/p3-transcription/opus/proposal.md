# P3 transcription and privacy: Opus proposal

Independent proposal for `docs/plans/p3-transcription/brief.md`. Design only: nothing here was built, compiled or run. Source paths under `frontend/` are relative to `/home/mu/Business/Transgate` (read only). Protocol claims link primary documentation fetched on 2026-10-24. "Fixture" always means synthetic or documentation-derived data, never a captured provider transaction.

## 0. Summary of the decisions

1. P3 owns the JOB, UPL, TRX, TRL, EDT, EXP rows, the transcription LNG rows and the non-AI, non-legal PRV rows, plus CRN-001..005/007 and EML-004/005. Rows that belong to P4/P5/P6/P9 are named, not silently counted (section 1). The PRD's "privacy rows green" wording needs a phase-aware gate.
2. New crates under `transgate/`: a pure `core` (with the generated `languages.rs` the PRD names), `postgres`, `api` (contract), `http`, `export`, `storage`, `providers/{soniox,deepgram}`, `youtube`. No generic workflow engine, no shared "platform" crate: each host implements its own small advisory-lock and S3 presign adapter over established crates.
3. Every job mutation follows one order: billing account lock, then job rows `FOR UPDATE` in ascending id, then a revision compare-and-swap. No network inside any transaction. A provider call is always preceded by a committed attempt claim and followed by a guarded result write.
4. Routing is one pure function: pinned provider, translation capability, the duration and `qta` overrides, preferred provider, then Soniox, then the fixed chain. Health scoring goes (PRD section 2 allows it); failover after a hard failure finally works.
5. Billing is composed through the existing P2 seam (`PgBillingTx::attach`, `charge_job_starts`, `charge_strict`, `release_subject_charges`). **New billing prerequisite:** account erasure. P2 has no erase path and the runtime role cannot delete accounts, packages or payments (section 5). Reported to lead.
6. Deletion is a durable saga: local erasure and billing erasure in one database transaction, identity deletion next, remote objects and provider copies tracked item by item. It never reports a deletion it has not seen succeed; GCS retention refusals and Deepgram's lack of a delete API are recorded as such.
7. taskr gets **task attachments with a malware-scan job**: a presigned upload, a scan job aggregate with retries and stuck recovery, two real scanner adapters (ClamAV `clamd`, VirusTotal hash lookup) with a consent-gated fallback, a retention sweep with opt-out and an account deletion saga. Ordinary task creation does not change.

## 1. Rule scope and phase boundary

### 1.1 Rows P3 must turn green

| Prefix | P3 owns | Status `changed` (decision recorded, test proves the new outcome) | Owned elsewhere |
|---|---|---|---|
| JOB (46) | 001-031, 034-040, 043-045 | 032, 046 (health checks, PRD 2); 041, 042 (RunPod worker, PRD 2) | 033: P3 emits the completion event and proves a consumer failure never fails the job; the row closes when P4 (summary) and P7 (customer callback) consume it |
| UPL (33) | 001-028, 030-033 | none | 029 (anonymous public YouTube tool) to P6 free tools |
| TRX (42) | 001-006, 008, 009, 011, 012, 014-034, 038-042 | 007, 010 (health sort and thresholds); 013, 035, 036, 037 (RunPod) | 022 shared with P7: P3 stores `whisper_params` opaquely and proves no remaining provider acts on them; P7 owns the request surface |
| TRL (4) | all | none | none |
| EDT (14) | all; 001 and 014 as presentation intents, P5 renders | none | none |
| EXP (17) | 001-016 | none | 017 (public tool formats) to P6 |
| PRV (60) | 001-023, 026-029, 032-037, 051, 052 | 024, 030 (health-metrics table dropped); 025, 031 (pg_cron history becomes the scheduler's own run history, same 90-day weekly rule) | 038-044 to P4 (AI consent); 045 to P9 (archived table, optional export); 046-049 to P5 (cookie banner, browser trackers); 050 to the P2 analytics/signup owner; 053-060 to P6 (legal and marketing copy) |
| LNG (33) | 007-028, 031, 033 | none | 001-006, 032 to P5; 029 to P2 (freemium lists); 030 to P4 |
| CRN (10) | 001-005, 007 | 006, 010 (health) | 008, 009 (free-tools processor) to P6 |
| EML (12) | 004, 005 | 011 (provider-unhealthy mail goes with health scoring) | rest already P2 or platform |
| DAT | schema preserves DAT-005 (owner, all 12 statuses, nullable duration) | none | rows close in P9 |

The `changed` rows are PRD-authorized simplifications ("the transcription provider plugin framework and health scoring", RunPod dropped), not silent removals. Each still needs a decision reference and a test of the new outcome (deterministic routing, a RunPod-pinned job routes through the shared rule).

Browser-mechanics rows (UPL-002/003/004/006/007/008/013/014/017-020/025/028, JOB-003, EDT-008-013, EXP-012-016) are proven in P3 as pure presentation decisions with tests, the way P2 proved its presentation policies. Their rendered parity rows stay P5. UPL-006 splits the same way: rounding and the failure policy are core, the HTML5 probe adapter is P5.

### 1.2 Inventory contradictions to correct (lead integration)

1. **Soniox duration field.** BIL-035 and TRX-034 say Soniox minutes come from `metadata.duration_ms`. That object is source-internal. `buildSonioxPayload` copies `reported.audio_duration_ms` into it (`frontend/src/utils/soniox.ts:150-172`). The webhook path passes the webhook body's `audio_duration_ms` (`frontend/src/pages/api/soniox/callback.ts:139-143`), but Soniox documents the webhook body as `{id, status}` only ([webhooks](https://soniox.com/docs/stt/async/webhooks)). `audio_duration_ms` exists on `GET /v1/transcriptions/{id}` ([get transcription](https://soniox.com/docs/api-reference/stt/transcriptions/get_transcription)). So the live webhook path almost certainly bills by the largest token `end_ms`, while the recovery sweep, which polls, uses `audio_duration_ms`. The ledger wording should say this. The outcome is decision M2.
2. **Deepgram coverage.** TRX-012 and LNG-016 say Deepgram handles all 61 languages. Deepgram's Whisper Cloud page accepts every code in Whisper's tokenizer list, which includes all eight non-Soniox codes (hy, is, mi, ne, ka, la, uz, lb). Its "officially supported" list names Armenian, Icelandic, Maori and Nepali but not Georgian, Latin, Uzbek or Luxembourgish ([Whisper Cloud](https://developers.deepgram.com/docs/deepgram-whisper-cloud)). The same page says Whisper is unavailable on the EU endpoint and every request has a 10-minute processing timeout (504). That timeout matters for TRX-002, which sends audio over 200-250 minutes to Deepgram.
3. **Soniox models.** TRX-015/019 pin `stt-async-v3`/`stt-async-v5`. Soniox retired v3 on 2026-02-28 with automatic routing to v4, and v4 is now an alias of v5 ([models](https://soniox.com/docs/stt/models)). v3 no longer appears in the current alias table. Production still completes Soniox jobs (`transgate/docs/production-facts.md:37`), so v3 is still accepted somewhere, but this is unverified (decision M5).
4. **Soniox language list.** Soniox's current list has 60 languages ([supported languages](https://soniox.com/docs/stt/concepts/supported-languages)). All 53 app-declared Soniox languages are in it and none of the eight exclusions are, so the routing data (LNG-015) stays correct.
5. **Phase gate.** The PRD P3 done condition says "Ledger rows for transcription and privacy green", but AI consent is P4's deliverable, cookie consent and trackers are web-shell behavior, and legal copy is CMS content. Proposed: `check-prd.py transcription` takes the exact ID set in 1.1, and every moved row names its owning phase.
6. **Unauthenticated queue endpoints.** Section 2.6 of `jobs-transcription.md` lists `/api/queue/status` and `/api/queue/trigger` without authentication, but no rule row covers them. P3 keeps only the CRON_SECRET routes (CRN-004) and records the drop as a security correction.

### 1.3 Boundary with later phases

P3 delivers real core and HTTP behavior and its tests. It does not claim web views (P5), the CMS or free tools (P6), API keys, rate limits for keys or customer callback delivery (P7), native/CLI/TUI shells (P8), or migration and cutover (P9). Where a later phase consumes P3 it gets a stable contract: the `transgate-api` DTOs, outbox events `job.completed`, `job.failed` and `job.partial_errors`, and presentation state machines with fixtures. The URL-job use case (UPL-030/031, deferred billing) is implemented and Postgres-tested in P3. Its HTTP exposure needs an API-key principal, so the route mounts only when P7 supplies that authenticator. Nothing pretends it exists before then.

Free tools stay server-first with `CLIENT_WASM_ENABLED=false` and a separate FFmpeg processor (FLG-008, CRN-008/009, API-022..030, RTL-006..008). P3 neither builds nor deletes that obligation. It belongs to P6's shell work with the processor's session-affinity question (`transgate/docs/discovery/platform.md:97,217`).

## 2. Crates, aggregates, ports and transaction ownership

| Crate | Content | Dependencies |
|---|---|---|
| `transgate/core` (`transgate-core`) | `languages.rs` (generated from `transgate/fixtures/languages/*.json`, PRD 7.4), `jobs` (Job aggregate, transitions, routing, retry and recovery policies, errors), `transcript` (lines, Soniox/Deepgram/caption segmentation, time formatting, speaker labels, edit rules), `export` model (file list, names, txt/csv/srt/vtt text, Word paragraph model), `privacy` (retention plan, deletion saga state machine, preferences), `presentation` (state machines and fixtures), ports, use cases | `billing-core` (types, `Clock`, charge and freemium policies), `serde`, `uuid`. No I/O |
| `transgate/postgres` | schema `transgate` with its own history table (same SQLx pattern as billing), repositories, `JobUow` over one `Transaction`, advisory-lock adapter, schedule-slot claims, rate-limit windows, outbox relay, retention apply, local erasure | `transgate-core`, `billing-postgres` (attach seam), `sqlx` 0.9 |
| `transgate/api` | HTTP DTOs both the server and P8 clients compile against (the taskr-api pattern) | `serde` |
| `transgate/http` | Axum 0.8 routes, identity extractor reusing `transgate_billing::identity::JwtIdentity`, callback verification wiring | core, api |
| `transgate/export` | zip and Word bytes from the core export model; usable from wasm (P5) and CLI (P8) | `zip`, `docx-rs`; developer confirms both are pure Rust with acceptable licenses |
| `transgate/storage` | S3-compatible presign, HEAD, DELETE and PUT for `gcs` (HMAC V4 interop), `cloudflare` (R2) and `s3` (MinIO, dev only) | `rusty-s3` ([sans-IO SigV4](https://docs.rs/rusty-s3)), `reqwest`/Rustls |
| `transgate/providers/soniox`, `.../deepgram` | real HTTP adapters, callback authentication, typed parse into core structs | `reqwest`/Rustls, `serde_json` |
| `transgate/youtube` | YouTube Data API v3 metadata and TranscriptAPI.com captions | `reqwest` |
| `transgate/server` | composition, scheduler task, CRON_SECRET routes; the existing binary grows | all of the above |

`taskr-core` stays dependency-free and unchanged except for the additive taskr scenario in section 7.

### 2.1 Job aggregate

The persisted `job_status` keeps all 12 source values (JOB-001): `draft, not_started, pending, running, speaker_labeling, completed, failed, canceled, deleted, uploaded, processing, ready`. The last four have no runtime producer. They are imported, readable by id and soft-deletable, nothing more. The language column keeps all 61 names, including the four that drifted out of the migrations (LNG-033, LNG-D01).

Fields beyond the source columns (all on the job row, no side framework):

- `revision bigint`: the compare-and-swap token.
- `source`: `upload {storage_provider, key derived from user/job/name}`, `url {audio_url, callback_url}` or `youtube {video_id, url, transcript_source}`.
- `pinned_provider` (imported or set column value, honored by TRX-001) and `requested_provider` (API `preferredProvider`): separate fields (JOB-D05) so routing can tell an override from a choice.
- `attempt_no int`, `attempt_provider`, `attempt_phase` (`none | claimed | accepted | unknown`), `attempt_claimed_at`, `attempt_lease_until`, `remote_id` (Soniox transcription id, Deepgram `request_id`), `remote_deleted_at`. At most one live attempt.
- `deferred_charge` (`none | pending | charged | refused | no_duration`).
- `retry_count`, `recovery_attempts`, `started_at`, `error jsonb`, `next_dispatch_at`.

Transcript lines live in `transgate.transcript_lines`: the source column set, with `start_time`/`end_time` kept as zero-padded `HH:MM:SS` text so text ordering (EDT-011) and migration parity hold. The `job_id` foreign key stays `ON DELETE RESTRICT` (PRV-026). Lines are not part of the job revision. Edits stay last-write-wins per line, as EDT-002 requires; adding a line revision check would be a new restriction.

User preferences live in `transgate.user_settings (account_id, auto_delete_transcripts default true NOT NULL, unlimited_history default false, blocked, deleted_monthly default true)`. Only `auto_delete_transcripts` is user-writable (PRV-D10 security correction). The rest are operator or imported fields.

### 2.2 Ports (frozen before fan-out)

All ports are `Send + Sync` trait objects returning the BoxFuture alias, the house style.

- `JobStore::begin() -> Box<dyn JobUow>`. `JobUow` methods:
  - `billing() -> Box<dyn BillingTx + '_>`, built with `PgBillingTx::attach(&mut *tx)` (`modules/billing/postgres/src/tx.rs:45-79`), so billing joins the host transaction;
  - `lock_jobs(account, ids) -> Vec<Job>` (`FOR UPDATE`, ascending id) and `job(id, lock)`;
  - `completed_job_count(account)` (status `completed`, FRE-006), read after the lock;
  - `insert_job`, `save_job(&Job, expected_revision)`, `insert_lines`, `delete_lines`, `hard_delete_job`;
  - `provider_slot_lock(provider)` and `inflight(provider)`;
  - `emit(JobEvent)`, `commit`, `rollback`.
- `JobQueries`: page of jobs (status allow-list, window, id cursor), job by id, lines, preferred languages (UPL-021).
- `Transcriber` (one per provider):
  - `submit(&Dispatch) -> Accepted{remote_id} | RateLimited | Rejected{class, status, message} | Unknown{message}`;
  - `status(remote_id) -> Queued | Processing | Completed | Error{message} | Unsupported`;
  - `result(remote_id) -> ProviderResult`;
  - `delete_remote(remote_id) -> Deleted | Absent | StillProcessing | Unsupported | Failed`;
  - `find_by_reference(job_id, since) -> Option<remote_id>`;
  - `verify_callback(headers, query, body) -> CallbackEnvelope`.

  Parsing yields core types (`SonioxToken`, `DeepgramParagraph`, `DeepgramWord`); segmentation stays in core.
- `ObjectStore`: `presign_put(key, ttl)`, `presign_get(key, ttl)`, `head(key)`, `put(key, bytes)`, `delete(key) -> Deleted | Absent | Retained{until} | Failed`.
- `CaptionSource`: `metadata(video_id)` and `captions(url) -> Cues | no_captions | age_restricted | rate_limited | network_error | unknown_error` (UPL-027).
- `IdentityAdmin::delete_user(account) -> Deleted | Absent | Unavailable`.
- `RateLimiter::hit(key, limit, window) -> {allowed, remaining, reset}`.
- `Lock::try_acquire(name) -> Option<Guard>`, plus `claim_slot(schedule, instant) -> bool`.
- Reused rather than duplicated: `billing_core::types::Clock` and `billing_core::ports::Mailer` with the existing `transgate_billing::mail::HttpMailer` adapter.

### 2.3 Use cases and who owns the transaction

Use cases live in `transgate-core::jobs::app` and `privacy::app`, and each opens exactly one `JobUow`. Every use case:

- commits or rolls back its own unit of work;
- calls adapters only between transactions;
- loads host facts (completed count, balances, slots) after its locks, never from a request.

HTTP and the scheduler are shells: they call use cases and translate errors. No `if` with business meaning lives in `transgate-http`.

### 2.4 Presentation state machines (consumed by P5/P8)

Each is a pure `update(event) -> effects` plus `view()`, with named fixtures.

- **`UploadQueueState`**. File entries get a stable local id instead of a name key (JOB-D07). Entry states: uploading with progress, preparing (no duration yet), ready, upload failed, duration failed. Further rules:
  - two drop filters exactly as built and an unvalidated picker (UPL-003);
  - a 10-file cap that a multi-pick can exceed (UPL-004);
  - Start enabled only when every file is uploaded with a duration (UPL-007, BIL-038);
  - translation mode clears the opposite language and offers the 53 targets (UPL-018);
  - auto-detect pinned first in transcription mode only (LNG-008/011);
  - drafts restored on load (JOB-003);
  - YouTube-only submission for mixed queues (UPL-028);
  - the duration poll schedule as timer effects (UPL-008).
- **`JobListState`**: cursor paging by 20, the status allow-list, badges and copy (section 2.1 table of the discovery), in-progress set.
- **`TranscriptState`**:
  - previous-speaker fallback (EDT-009) and translation tabs with the `SHOW_ORIGINAL_TRANSCRIPT` allow-list (EDT-012, TRX-042);
  - RTL (LNG-007, EDT-013), speaker labels hidden before completion (EDT-008), the edit and rename intents (EDT-001-006);
  - export toggles (EXP-012), download only when completed (EXP-001);
  - the P2 freemium display cut (`billing_core::freemium::display_cut`, `modules/billing/core/src/freemium.rs:146`);
  - a 2-second status poll while running. Polling replaces Supabase Realtime, matching the agreed "realtime is an optimization of the same pull".
- **`DataManagementState`**: auto-delete toggle with the confirm-on-disable copy (PRV-027/028), delete-all confirm (PRV-011), delete-account confirm (PRV-002), and deletion progress that signs the user out only once the identity is gone (PRV-D01 intended behavior).

## 3. Transitions, concurrency and recovery

### 3.1 Transition table

| From | Trigger | To | Guard and effects (same transaction unless marked) |
|---|---|---|---|
| none | create draft (upload, UPL-001; YouTube add, UPL-024; API URL, UPL-031) | draft | owner from session; URL jobs go draft then not_started inside one request (observably the same as T03+T04) |
| draft | set options (JOB-005, UPL-019/020) | draft | owner; draft only; writes language, `translate_to` and speakers only |
| draft | remove from queue (UPL-015) | row deleted | `release_subject_charges` first (BIL-042), then the job lock; no object delete (JOB-038 as built) |
| draft (batch) | Start (JOB-004..012) | not_started | the start protocol in section 5; any refusal rolls back every job; YouTube drafts refused here |
| draft (YouTube) | resolve succeeded (UPL-026) | completed | lines plus status in one transaction; never billed (JOB-035) |
| draft (YouTube) | resolve failed | failed | `{code, message}`; the client deletes the draft (UPL-027/028) |
| not_started, pending | dispatch claim | unchanged, attempt `claimed` | revision CAS; Deepgram slot rule; commit before any HTTP |
| claimed | provider accepted (JOB-014) | running | `started_at`, provider set to the accepting one, `remote_id`; CAS on `attempt_no` |
| claimed or running | 429 or rate-limit message (JOB-015) | pending | `retry_count + 1`, error with attempt time, `next_dispatch_at` |
| claimed | hard rejection, candidates remain (TRX-014) | not_started or pending, next candidate claimed | DEF-06 correction: an attempt failure is not terminal while failover candidates exist and `fallbackEnabled` is not false |
| claimed | hard rejection, no candidate (JOB-016) | failed | coded error (JOB-017 classes) |
| claimed | transport outcome unknown | unchanged, attempt `unknown` | resolved by section 3.3 recovery |
| running, pending | result ingested (JOB-028) | completed | lines, duration rule, deferred charge, outbox event; JOB-027 completed-is-no-op |
| running, pending | provider error, no tokens, no speech, insert failure (JOB-031, TRX-040, TRX-032) | failed | coded error; `no_speech_detected` |
| running (Deepgram) | started ≥ 30 min ago (JOB-020/021) | pending | `retry_count + 1`, `stuck_recovery` error; no maximum (M4) |
| running (Soniox) | stuck: recovery claim (JOB-022..024) | running, `recovery_attempts + 1` | completed or error ingested; queued, processing or lookup failure gives the claim back |
| running (Soniox) | attempts ≥ 3 or no `remote_id` (JOB-023) | failed | `recovery_attempts_exhausted` or `recovery_unavailable` |
| running, pending | Soniox ingestion task threw after the 200 reply (JOB-029) | failed | `callback_ingestion_error`, only from running or pending |
| not_started, pending, running | cancel (JOB-040) | canceled | use case only; no route and no screen, as built |
| any | soft delete, delete all (JOB-036, PRV-011) | deleted | account lock (it changes the completed-job predicate); charges kept |
| completed | retention sweep (PRV-014) | completed | lines removed, job row kept |
| deleted, canceled, failed, completed | late provider result | unchanged | audit `late_result_ignored`, no lines (JOB-D01 correction); remote delete if Soniox auto-delete is on |
| uploaded, processing, ready, speaker_labeling (imported) | none at runtime | none | only soft delete; P9 drains in-flight RunPod work before cutover |

### 3.2 Invariants

1. **One lock order.** Billing account row, then job rows ascending, then revision CAS. Every job save takes the account lock, because entering or leaving `completed` changes the freemium predicate P2 must serialize (`docs/plans/p2-billing/implementation-contract.md:29`).
2. **At most one live attempt per job.** `attempt_no` only grows, and a provider response or callback for an older attempt is ignored. Soniox gets `webhook_url ...?job_id=N&attempt=K` (query metadata is documented, [webhooks](https://soniox.com/docs/stt/async/webhooks)) and `client_reference_id = job_N` (TRX-015 preserved). Deepgram gets `job_id` and `attempt` in the callback URL, and `request_id` is matched.
3. **One start charge per job.** The draft-to-not_started CAS and the usage insert commit together. The deferred charge happens at most once through the `deferred_charge` CAS in the completion transaction.
4. **Lines appear only at completion.** They are written in the completion or YouTube-resolve transaction and never for a job that is no longer running or pending.
5. **Deepgram concurrency stays at most 4.** Claimed plus running Deepgram attempts never exceed `5 - 1` (JOB-018). The claim transaction locks a single `transgate.provider_slots` row for the provider, so two replicas cannot both take the last slot. This is cost-relevant: Deepgram bills 429 responses (source ADR-017 note in JOB-018 inventory).
6. **No transaction spans network I/O.** That covers providers, storage, identity and mail. Mail and later consumers run from the outbox after commit, at least once, with stable event ids (P2 pattern).
7. **Locks are an optimization; row CAS and slot claims decide.** Two replicas running the same sweep remain correct.

### 3.3 Crash windows and bounded recovery

| Window | What can be left | Recovery | Bound |
|---|---|---|---|
| W1 Start committed, process dies before dispatch | `not_started`, no attempt | dispatch pass (every 3 min, CRN-002 cadence) takes not_started and pending jobs with no live attempt, oldest first | one pass plus backlog |
| W2 attempt claimed, no HTTP sent, or sent and the reply was lost | `claimed` with an expired lease and no `remote_id` | Soniox: `find_by_reference` pages `GET /v1/transcriptions` (bounded by the documented 2,000-transcription cap, [limits](https://soniox.com/docs/stt/async/limits-and-quotas)) for `client_reference_id = job_N` created after the claim; found means adopt, not found means redispatch. Deepgram has no lookup: wait up to the 30-minute stuck threshold for a callback carrying this attempt, then reset to pending and redispatch | Soniox: no duplicate except within the listing's visibility lag. Deepgram: at most one duplicate purchase per stuck period, the source's own behavior (JOB-021) |
| W3 callback arrives before the accepted write commits | `claimed`, callback names this attempt | callback adopts the remote id if the attempt number matches; Soniox is verified through `GET` and `client_reference_id` | none |
| W4 Soniox 200 sent, ingestion task dies (JOB-029) | `running` | stuck sweep after 30 min polls Soniox (JOB-022) | at most 3 recovery attempts (JOB-023) |
| W4' Deepgram ingestion fails | `running` | the handler is synchronous, as in the source, so a failure answers non-2xx and Deepgram retries up to 10 times 30 s apart ([callback](https://developers.deepgram.com/docs/callback)). Results are deleted after the last retry and cannot be retrieved ([your data](https://developers.deepgram.com/trust-security/your-data)), so the stuck reset re-purchases | the source's behavior |
| W5 Soniox remote delete after completion fails | `remote_deleted_at` null | remote-cleanup pass retries; Soniox deletes files and transcriptions after 30 days anyway ([limits](https://soniox.com/docs/stt/async/limits-and-quotas)) | 30 days |
| W6 deletion saga interrupted | durable saga record and items | saga pass resumes from the recorded step (section 6) | until each item is done, absent or explicitly unsupported |
| W7 retention interrupted | nothing or everything, since apply is one transaction | next daily run | one day |

The scheduler is one in-process ticker per replica with named schedules. Each run inserts `(schedule, slot instant)` into `transgate.schedule_runs` with a unique key, so two replicas cannot both run the 02:00 retention for the same day. It then takes `pg_try_advisory_lock(schedule)` on a dedicated pooled connection to stop overlap (a dropped connection releases the lock) and records finish, status and counts. This replaces CRN-001's per-process flag. Schedules:

- dispatch and recovery every 3 minutes (CRN-002/003);
- retention at 02:00 UTC (CRN-005);
- run-history purge on Sundays at 04:00 UTC, 90 days (CRN-007 adapted);
- remote cleanup every 10 minutes.

The CRON_SECRET routes run the same passes on demand (CRN-004).

## 4. Provider, storage and translation protocols

### 4.1 Protocol plan

| Concern | Plan | Evidence |
|---|---|---|
| Soniox submit | `POST /v1/transcriptions` with the source body (TRX-015..018): model, `audio_url` (presigned GET, 3600 s, JOB-043), `webhook_url` with `job_id` and `attempt`, `webhook_auth_header_name: X-Webhook-Secret`, `client_reference_id: job_N`, language identification, single `language_hints` only when chosen, diarization, `translation {type: one_way, target_language}` from the first target | source `frontend/src/services/unified-transcription/providers/soniox.ts:123-196`; [create](https://soniox.com/docs/api-reference/stt/transcriptions/create_transcription) |
| Soniox callback | body `{id, status}` with status `completed` or `error`; the secret is now mandatory (fail closed). The source compared it only when configured, JOB-030; this is a security correction. Fetch `GET /v1/transcriptions/{id}/transcript` for tokens and `GET /v1/transcriptions/{id}` for `error_message` and `audio_duration_ms` | [webhooks](https://soniox.com/docs/stt/async/webhooks), [get transcription](https://soniox.com/docs/api-reference/stt/transcriptions/get_transcription) |
| Soniox delete | `DELETE /v1/transcriptions/{id}` after completion when auto-delete is configured (TRX-041, PRV-037; on in production, `transgate/docs/production-facts.md:15`); a processing transcription cannot be deleted | [delete](https://soniox.com/docs/api-reference/stt/transcriptions/delete_transcription) |
| Soniox limits | 300-minute fixed file limit, 100 pending, 2,000 total, 30-day auto-deletion | [limits](https://soniox.com/docs/stt/async/limits-and-quotas) |
| Deepgram submit | `POST /v1/listen` with the TRX-020 query (`model=whisper-large`, `smart_format`, `punctuate`, `paragraphs`, `diarize`, `language` when chosen) and body `{url}`. The callback URL carries basic-auth credentials plus `job_id` and `attempt`. That closes the unauthenticated callback (JOB-030), a security correction. The `nova-2` hard-coded user (LNG-D03) becomes a model configuration value, not a user id | [callback auth](https://developers.deepgram.com/docs/callback), [Whisper Cloud](https://developers.deepgram.com/docs/deepgram-whisper-cloud) |
| Deepgram retention | default requests join the Model Improvement Program and are retained; `mip_opt_out=true` means zero retention (decision M3) | [your data](https://developers.deepgram.com/trust-security/your-data) |
| Storage | presigned PUT for 900 s with no content-type pin (UPL-011) and GET for 900 s for the player (UPL-016) and 3600 s for providers; key `userId=<uid>/jobId=<id>/<fileName>` (UPL-010). GCS accepts `AWS4-HMAC-SHA256` with HMAC keys ([GCS signatures](https://cloud.google.com/storage/docs/authentication/signatures)); R2 is S3-compatible ([R2 presigned URLs](https://developers.cloudflare.com/r2/api/s3/presigned-urls/)). The storage provider is required configuration, never a silent default; each job records its provider (UPL-012) | source `frontend/src/pages/api/init-upload.ts:20-49` |
| Raw payload | best-effort PUT of `jobs/{user}/{job}/{provider}_transcript.json` after the result is fetched; failure is logged (JOB-044) | source `frontend/src/utils/soniox.ts:386-405` |
| Translation | Soniox one-way speech translation only; paired original and translated lines (TRX-028, TRL-004). No LLM translation is introduced (AI R2, `transgate/docs/discovery/ai.md:132`) | source `frontend/src/utils/soniox/transcript-processor.ts:178-386` |
| YouTube | Data API v3 `videos?part=snippet,contentDetails` and TranscriptAPI.com `GET transcript?video_url=..&format=json&include_timestamp=true` with bearer key, the error mapping of UPL-027, the `blocked` 403 on add and resolve only (PRV-051), and the per-user limits through the shared `RateLimiter`. That replaces the per-process map (IDN-D03) and is a correction, since the source comment assumed a single dyno | source `frontend/src/pages/api/youtube/*.ts`, `frontend/src/features/transcription/services/youtube/TranscriptApiService.ts:53-123` |

### 4.2 Evidence classes

| Class | What P3 can claim |
|---|---|
| Public documentation | request and response shapes, callback authentication, limits, retention statements, the language lists above |
| Synthetic fixtures | doc-example payloads, plus source-shaped token and paragraph payloads (Soniox punctuation, Arabic and CJK, 300 ms and 2000 ms gaps, speaker change, translation pairs, missing paragraphs, zero-length tokens, translation tokens with `end_ms: 0`). Each fixture file carries a provenance header (doc URL or source file:line). Loopback fixture servers implement the documented endpoints, including firing callbacks with credentials |
| Unavailable live acceptance | any real Soniox or Deepgram transcription, transcription quality, the eight Deepgram-only languages, Whisper processing of audio over 200 minutes within Deepgram's 10-minute timeout, GCS's exact retention-refusal response, R2 lifecycle, whether `stt-async-v3` still resolves. Listed as Needs mu; no fixture invents a success for them |

### 4.3 Languages

All 61 transcription languages and 53 translation targets stay offered (PRD 2.6). Auto-detect is Soniox-only (LNG-010). The eight Soniox exclusions route to Deepgram:

| Language | Code | Deepgram documentation level |
|---|---|---|
| Armenian, Icelandic, Maori, Nepali | hy, is, mi, ne | in the "officially supported" list |
| Georgian, Latin, Uzbek, Luxembourgish | ka, la, uz, lb | accepted codes in the Whisper tokenizer list, not in the official list |

Fixtures for these eight prove the request (language parameter, model, routing), never a transcript. Removing any of them would be a new restriction; acceptance stays a Needs mu item with authorized keys and consented sample audio.

### 4.4 Provider-pinned and legacy jobs

| Stored provider | Behavior |
|---|---|
| `soniox` | used as-is (TRX-001) |
| `soniox_v5` | Soniox adapter with the v5 model (TRX-019) |
| `deepgram` | used as-is with no capability check, even for auto-detect (TRX-001 as built) |
| `runpod` | no adapter: routed by the shared rule and recorded as a PRD-2 consequence of dropping RunPod. Running or `speaker_labeling` RunPod rows are a P9 drain item |
| null | shared routing |

New drafts are never pinned: the provider is written only when an attempt is accepted (JOB-014), so the baseline default question (LNG-D04) cannot recur. Imported in-flight Soniox and Deepgram jobs keep working after cutover because the callback paths `/api/soniox/callback` and `/api/deepgram/callback?job_id=` are preserved. P9 decides whether legacy unauthenticated Deepgram callbacks for imported in-flight jobs are drained before cutover or accepted once by stored provenance.

### 4.5 Routing (pure, deterministic)

`route(job, request, availability) -> candidates`:

1. A pinned provider (except `runpod`) is the only candidate.
2. A translation job may only use translation-capable providers (Soniox); none available fails with the TRX-009 message, and the duration and `qta` overrides are skipped.
3. Duration above `SONIOX_MAX_DURATION_MINUTES` (required configuration; production 200, code default 250) sends a non-translation job to Deepgram with no availability check (TRX-002).
4. A `qta` extension sends a non-translation job to Deepgram (TRX-003).
5. A valid `preferredProvider` is used (TRX-004). Valid means it exists, can handle the job, supports translation when needed, and is available: key configured and, for Deepgram, a slot.
6. Soniox if valid. The language map and client-type preference collapse into this step because they never changed an outcome (LNG-D06).
7. Chain soniox, deepgram (the RunPod slot is reserved but empty): first valid.
8. Last resort: the first that can handle the job and is available, else Soniox for a non-translation job (TRX-008).

Failover follows the remaining chain (TRX-014). The 7-language map collapsing into step 6 changes no outcome; health sorting (TRX-007/010) is replaced by the fixed order (LNG-D05).

## 5. Billing integration

| Flow | Transaction | P2 functions used |
|---|---|---|
| Start (web, job ids) | `BEGIN` (READ COMMITTED) → attach billing → `lock_account` → `lock_jobs` ascending → JOB-004 checks ("No draft jobs found", 400 "No jobs were updated") → `web_precheck`, balance below 0 refuses (BIL-039) → `web_guard` over drafts in ascending id, so the request-array order is irrelevant (`modules/billing/core/src/freemium.rs:83`) → `completed_job_count` → `charge_job_starts` (`modules/billing/core/src/app.rs:1553`) → CAS every draft to not_started with `deferred_charge` pending for null durations → `COMMIT`. Then dispatch sequentially outside any transaction (JOB-011/012) and answer 201 with per-job results and `start_answer_meta` (BIL-074) | `web_precheck`, `web_guard`, `charge_job_starts`, `start_answer_meta` |
| API URL jobs | the same transaction with `api_precheck` (balance must be above 0, BIL-040); duration null means `Deferred` (BIL-028) | `api_precheck`, `charge_job_starts` |
| Completion with deferred charge | `BEGIN` → `lock_account` → job `FOR UPDATE` → attempt matches → if pending and provider minutes are known, set duration and `charge_strict` (strict, never the bypass; `skip_balance_check=false`). A refusal is logged and stored as `refused`, and the job still completes (BIL-034..036). Missing minutes means `no_duration` with no charge (BIL-035). A shorter provider length lowers `duration` only (BIL-037) → lines → completed → outbox → `COMMIT` | `provider_minutes_soniox`, `provider_minutes_deepgram`, `charge_strict`, `duration_after_provider` |
| Draft hard delete | `release_subject_charges` as the first billing call, so it locks every affected owner including imported cross-owner usage (`modules/billing/core/src/app.rs:1594-1620`), then job lock and delete | `release_subject_charges` |
| Soft delete, cancel, fail | account lock for the eligibility predicate; usage untouched (BIL-042) | `lock_account` |
| Account erasure | new prerequisite, below | not yet provided |

Billing never reads job tables (subjects are `i64` job ids, `modules/billing/core/src/types.rs:53-54`); P3 never reads billing tables. Shells get decided outcomes (`StartRefusal::message` and the freemium path text) and contain no balance logic.

**New billing prerequisite (reported to lead).** PRV-005/006 require account deletion to delete payments, packages and usage, and PRV-007 requires billing info to survive with a null owner. The source cascades usage by `user_id`, `job_id` and `package_id` (`frontend/supabase/migrations/20251130113249_remote_schema.sql:4382,4387,4392`), so deleting account A also returns minutes to another owner B whose package A's legacy rows drew from, and removes B's rows that reference A's jobs. P2 has no erase use case (no match for erase or delete-account in `modules/billing/`). Its runtime role has no DELETE on `accounts`, `packages`, `payments` or `subscriptions` (`modules/billing/postgres/migrations/0001_billing.sql:238-251`), and `transgate_billing.customers` grants only SELECT and INSERT (`transgate/billing/migrations/0001_customers.sql:22`).

Proposed seam: `erase_account(tx, account, subjects: &[SubjectRef]) -> ErasureReport`. It:

1. discovers affected owners, locks them ascending and re-checks the owner set, as `release_subject_charges` already does;
2. deletes usage and legacy rows by account, by subject and by the account's packages;
3. deletes packages, payments, subscriptions and the account; `billing_info` is nulled by its existing `ON DELETE SET NULL`;
4. returns counts for the audit.

It must be the first billing call of the erasure transaction. Grants are added for the runtime role only for these deletes. A separate erasure login is possible but adds a role for one path; my recommendation is plain runtime grants with a permission test. Whether undelivered billing outbox rows for the account are dropped or still sent is a billing decision to record. The P2 PostgreSQL owner should implement it, or delegate it explicitly. P3 slice B consumes it, and taskr's deletion scenario is the second consumer.

## 6. Retention and privacy saga

**Retention sweep (PRV-014..023, CRN-005).**

- `RetentionDays` is a typed value 7..=365 (PRV-018).
- `plan(now, days)` and `apply` share one predicate: lines of `completed` jobs created at or before `now - days - 1 day` whose owner has `auto_delete_transcripts` true. Null means keep (PRV-014..017), and `unlimited_history` changes nothing (PRV-017, decision M8).
- `apply` runs in one transaction: count, then abort above 100,000 rows with an `aborted` audit and no deletion (PRV-019), otherwise delete and write a `success` audit with `users_opted_out_count` (PRV-021/022).
- A concurrent run writes `skipped` (PRV-020). A failure writes `failure` in a separate transaction and re-raises.
- The audit table `transgate.cleanup_runs` allows `success`, `failure`, `partial`, `skipped` and `aborted` (PRV-D08 correction) and has no public grants (PRV-D09 correction). The `cleanup_job_health` and `user_opt_out_stats` views stay (keep-throw).
- The dry run returns the plan and writes no audit (PRV-023). It is an operator subcommand of the server binary, not a public route.
- Audio and raw payloads are not swept (preserved); bucket lifecycle remains their guard (PRV-035).

**Deletion saga (PRV-001..010, PRV-D01/D02/D04/D18).**

| Step | Mechanism | Failure handling |
|---|---|---|
| 1 Request | the authenticated subject deletes only itself (PRV-001). One transaction: create `account_deletions` (state `requested`); soft-delete all jobs (PRV-003, server side now); collect remote items (audio keys per storage provider, raw payload keys, Soniox transcription ids not yet deleted, Deepgram request ids marked unsupported) into `account_deletion_items`; hard-delete lines, jobs, settings and the customer directory row; call billing `erase_account` | all or nothing; the answer is honest |
| 2 Access guard | while a deletion record is open, the identity extractor refuses that subject (410 `account_deletion_in_progress`), so a lingering identity cannot re-provision a trial or recreate data | none needed |
| 3 Identity | GoTrue `DELETE /admin/users/{id}` ([admin API](https://supabase.com/docs/reference/self-hosting-auth/deletes-a-user)) through `IdentityAdmin`; 404 means already absent | retried by the saga pass; state stays `identity_pending`; never reported as done |
| 4 Remote items | storage `delete`. `Retained{until}` is recorded when the bucket retention policy refuses it: the GCS bucket has an unlocked 31-day retention policy (`Infra/infrastructure/terraform/main.tf:28-39` at the Transgate root, PRV-035), and GCS refuses deletion before the period ends ([bucket lock](https://cloud.google.com/storage/docs/bucket-lock)). Soniox `delete_remote` covers ids that are still processing. Deepgram items are `unsupported`: no delete API exists, and retention depends on `mip_opt_out` | per-item attempts and last error; `retained` items are re-verified by HEAD after `until` |
| 5 Close | the record closes when every item is done, absent, verified expired or unsupported; it keeps account id, timestamps, counts and item outcomes (the PRV-D04 tombstone stance) | none needed |

Other systems: PostHog and Sentry person deletion is not added. ADR-001/002 put it out of scope, and adding it would be a new capability without mu. Iyzico subscriptions are not called (P2 D3 manual cancellation stands). AI tables (P4) and API keys (P7) register their row deleters into the step-1 transaction when they exist. P3 provides the hook as a list of host-owned deleters in the same `JobUow`, not a plugin framework.

**Opt-outs and consents.** The auto-delete opt-out is the only user-writable privacy field (PRV-013/027, PRV-D10). `deleted_monthly` stays stored with no effect (PRV-036, decision M11). AI consent (P4) and cookie consent (P5) are outside P3. Their rows are deleted by step 1 once they exist, and the P3 test registers a fake deleter to prove the hook runs in the same transaction.

## 7. taskr consumer scenario

Additive "task attachments", covering catalog scenarios 3, 4, 7, 8 and 11 with no change to ordinary task creation:

- **Upload (11).** `POST /attachments` creates an `Attachment` awaiting upload and returns a presigned PUT; the core stores only the key. Completion verifies the object with HEAD. A core state machine models progress, and a `taskr-cli attach` command performs the real PUT with progress output.
- **Long-running job (3).** The attachment embeds a scan job: `awaiting_upload → uploaded → scanning(attempt) → clean | infected | scan_failed`. It has an attempt claim before any scanner call, a bounded retry policy on unavailability, a stuck scan reclaimed after a lease, and idempotent completion. Download URLs exist only for `clean`, a taskr business rule in core.
- **Provider port (4).**
  - `ClamdScanner` speaks the documented `zINSTREAM` framing to a real `clamav/clamav` container ([clamd protocol](https://docs.clamav.net/manual/Usage/ClamdProtocol.html)) and is smoke-tested with the standard EICAR test string.
  - `VirusTotalHashLookup` calls `GET /api/v3/files/{sha256}` ([file info](https://docs.virustotal.com/reference/file-info)) and never uploads content.
  - Pure fallback rule: clamd first; VirusTotal only when clamd is unavailable and the workspace consented to sharing file hashes; an unknown hash stays unscanned.
  - VirusTotal's live acceptance is unavailable without a key, and its fixtures say so.
- **Scheduler and lock (7).** `taskr-server` runs the same slot-claim plus advisory-lock scheduler; a two-process test proves one scan per attachment and one sweep per slot.
- **Retention and deletion (8).** A daily sweep deletes attachments of tasks done 30 days or more ago unless the workspace opted out, with an audit row. A durable account-deletion saga removes sole-member workspaces and their objects, leaves shared workspaces (refusing with "transfer ownership first" when the user is the only owner of a shared workspace), erases the taskr billing sample account through the same billing `erase_account`, and deletes the GoTrue user in the dev compose stack.

Placement:

- `taskr-core::attachments` (domain, app, presentation) plus shared-kernel `Lock`, `ObjectStore` and `Scanner` ports, all dependency-free;
- `taskr-postgres` (repositories, lock);
- `taskr-infra` or a new `taskr-scan` crate for the two scanners and the S3 adapter;
- `taskr-server` (routes, worker);
- the dev compose stack gains MinIO and clamd (lead-owned deploy file).

The scenario stays generic SaaS and imports nothing from Transgate.

## 8. Verification plan (not executed in design)

**Failing-first domain tests (transgate-core):**

- the full status × command transition matrix, with refusals asserted, not only successes;
- routing over all 61 languages × auto-detect × translation × duration at, below and above the configured threshold × `qta` × preferred provider × availability (Deepgram slots full) × pins (including `runpod`);
- failover after a hard rejection (DEF-06 regression);
- segmentation fixtures for every TRX-024..031 branch, plus the Deepgram word-fallback gap (JOB-D03 failing-before test);
- line time formatting with the Soniox 500 ms offset (TRX-026);
- speaker numbering (TRX-027/030);
- export golden outputs: names, sanitization (EXP-003), SRT splitting above 6 s, CSV quoting, VTT header, the video-extension gate, all-formats naming;
- retention plan boundaries (exactly 30 days kept, 31 deleted), threshold and validation;
- deletion saga transitions under every item outcome;
- every presentation state machine with fixtures.

**PostgreSQL (`TRANSGATE_TEST_DATABASE_URL`, restricted runtime login, ignored offline, failing when explicitly run without it).** Random accounts, no truncation.

- Whole-batch Start rollback on refusal (statuses unchanged, no usage), with both request permutations.
- Two concurrent Starts of the same drafts.
- Two replicas racing to claim one dispatch.
- Deepgram slot ceiling under 8 parallel claims.
- Duplicate concurrent callbacks producing one set of lines and one deferred charge.
- Callback versus soft delete (no resurrection) and callback versus retention.
- Deferred refusal still completes.
- Draft hard delete releasing imported cross-owner usage while the affected owner debits concurrently.
- Account erasure with signed and cross-owner legacy usage.
- Retention `skipped` and `aborted` audits.
- Schedule-slot uniqueness across two processes.
- Permission tests: the runtime cannot write imported-origin or audit rows and cannot read the audit through public grants.

**Real binary and loopback smokes.** Run `transgate-server` against the disposable PostgreSQL, MinIO, Soniox and Deepgram fixture servers, the dev GoTrue container and the mail fixture:

1. create draft, presign, PUT with curl, Start;
2. the Soniox fixture fires a webhook, lines appear;
3. edit, rename, export via the export crate;
4. a Deepgram job with basic-auth callback; an unauthenticated callback is refused;
5. a 429, then a dispatch pass;
6. kill the server between claim and accept, restart, recover;
7. soft delete, retention run, account deletion: GoTrue user gone, MinIO objects gone, Soniox DELETE received, billing rows gone;
8. two server processes on one database to prove one dispatch per job and one retention run per slot.

**taskr:** the EICAR smoke on real clamd and a two-process scan race.

**Rule mapping and gate.** Developers write `docs/plans/p3-transcription/implementation-evidence.md` with exact test references per row, plus decision links for `changed` rows. Proposed lead additions:

- `scripts/gate-p3-transcription.sh`, requiring the database URL;
- `check-prd.py transcription` with the exact ID set from 1.1;
- a workspace build;
- a runtime smoke script.

`gate-transgate.sh`, the module gate (CMS) and the Rustr scenario gate remain honestly red for P4-P10.

## 9. Developer slices

Fixed before fan-out (slice A writes them first and freezes them; changes only through lead): section 2.2 port signatures, the job row shape and status enum, core event names, the `transgate-api` DTO names, and the billing `erase_account` signature.

| Slice | Owns | Acceptance |
|---|---|---|
| A Core and presentation | `transgate/core` (languages generator and superset test, Job aggregate, routing, transcript, export model, privacy, presentation and fixtures, use cases against port fakes) | domain matrix and fixtures green; every P3 rule mapped to a core or presentation test or explicitly to B, C, D or E |
| B Persistence and transactions | `transgate/postgres`, migrations, `JobUow`, billing composition, locks, slots, rate limiter, outbox relay, retention apply, local erasure; coordinates `erase_account` with the P2 PostgreSQL owner | the PostgreSQL race and permission suite under the restricted login |
| C Providers and captions | `providers/soniox`, `providers/deepgram`, `youtube`, loopback fixture servers with provenance | protocol tests over real TCP: authentication accepted and refused, 429, timeout, malformed bodies, stale attempt, `find_by_reference` |
| D Storage, identity, mail, export bytes | `transgate/storage` (rusty-s3; GCS, R2, MinIO), retention-refusal mapping, GoTrue admin adapter, EML-004/005 templates on the existing `HttpMailer`, `transgate/export` | MinIO presign round trip, export zip and Word inspection, GoTrue delete against the dev container |
| E HTTP, server, scheduler, smoke | `transgate/api`, `transgate/http`, `transgate/server`, CRON_SECRET routes, scheduler, smoke harness | the section 8 runtime smoke and two-process runs |
| F taskr scenario | section 7 crates and tests | EICAR, two-process and deletion smokes; ordinary task tests unchanged |

Order: A's interface freeze, then B to F in parallel against fakes. The billing prerequisite blocks only B's erasure tests and F's deletion step. Root manifests, gates, ledgers and the deploy compose file stay lead-owned.

## 10. Decisions

### 10.1 Documented corrections under PRD section 2

Each needs a ledger record (current, intended, decision) and a failing-before test.

| Id | Current behavior | Correction |
|---|---|---|
| C1 | callbacks ingest into deleted, failed or canceled jobs; lines and status are written separately (JOB-027/028, JOB-D01) | one transaction; no lines unless running or pending |
| C2 | provider HTTP before any guard (JOB-D02) | committed attempt claim before the call |
| C3 | failover cannot submit after a hard failure (DEF-06) | attempts are not terminal while candidates remain |
| C4 | Soniox 429 and orphaned not_started jobs never retried (JOB-026, JOB-D04) | one dispatch pass for every provider; Deepgram slot, batch and spacing rules kept |
| C5 | unauthenticated Deepgram callback; optional Soniox secret (JOB-030) | both mandatory |
| C6 | browser writes any job column and inserts lines; users can set `blocked` and `unlimited_history` (JOB-045, EDT-003, PRV-052) | server-owned writes; edits only |
| C7 | failure mail on every update of a failed row (JOB-034, EML-004) | one mail per transition into failed |
| C8 | per-process YouTube limiter doubled by two replicas (IDN-D03) | shared windows, same fixed-window semantics |
| C9 | delete-account answers 200 on failure (PRV-004, PRV-D01) | durable saga, honest state |
| C10 | `skipped` and `aborted` violate the audit check; audit public (PRV-D08/D09) | recorded statuses, no public grants |
| C11 | Deepgram word fallback compares `parseFloat("HH:MM:SS")` (JOB-D03) | numeric seconds |
| C12 | health-scored routing and alerts (TRX-007/010, JOB-032/046, EML-011, CRN-006/010, PRV-024/030) | fixed order (PRD 2) |
| C13 | per-process scheduler duplicate guard, unauthenticated queue routes (CRN-001) | slot claim and advisory lock; secured routes only |
| C14 | pg_cron history purge (PRV-025/031, CRN-007) | own run-history purge, same schedule and 90 days |

### 10.2 Needs mu

The default shown is what P3 implements until mu decides.

| Id | Question | Default |
|---|---|---|
| M1 | Browser-measured minutes are trusted at Start; a user can understate and the callback never raises (TRX-033, JOB-D10, BIL-037). Server probe or upward reconciliation? | preserve |
| M2 | Soniox webhook path bills by max token `end_ms`; the recovery path uses `audio_duration_ms` (contradiction 1). Unify on `audio_duration_ms`, as the source comment intended? This can raise deferred charges | preserve per-path behavior |
| M3 | Deepgram `mip_opt_out=true` (zero retention; possible price impact) | preserve: not sent |
| M4 | Deepgram stuck reset has no maximum (JOB-021). Cap at 3 like Soniox? | preserve: no cap |
| M5 | Soniox model names: `stt-async-v3` is gone from the current table. Configure `stt-async-v5` (aliased today)? | send the configured value, defaulting to the source names |
| M6 | Delete storage objects and Soniox copies at account deletion. The source never did, but the policy promises erasure (PRV-D02, PRV-053) | attempt, and record retained or unsupported honestly |
| M7 | Hard-delete data of `deleted` jobs in the sweep (PRV-D05) | preserve: completed only |
| M8 | `unlimited_history` exempts the sweep (PRV-D06) | preserve: no |
| M9 | Financial records at account deletion (PRV-D03) | preserve: delete; billing info nulled |
| M10 | `blocked` scope (PRV-D11) | preserve: two YouTube routes |
| M11 | `deleted_monthly` toggle (PRV-D15) | preserve: stored, no effect |
| M12 | `.doc` holding docx; whole-second subtitles (JOB-D09) | preserve |
| M13 | YouTube jobs unbilled; mixed queue (JOB-035, JOB-D08) | preserve |
| M14 | taskr scenario choice: attachments with malware scan (section 7). The alternative considered was title translation through two translation APIs, which gives weaker local live proof | lead or mu |
| M15 | Live acceptance: authorized Soniox and Deepgram smoke, the eight Deepgram-only languages, multi-hour Whisper within the 10-minute timeout, GCS refusal shape, R2 lifecycle | Needs mu (keys, consented audio, bucket access) |
