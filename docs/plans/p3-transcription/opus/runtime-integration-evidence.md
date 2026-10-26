# P3 runtime integration evidence (Opus integration owner)

Owner files: `transgate/Dockerfile`, `transgate/Dockerfile.dockerignore`, `transgate/processor/deploy/*`, `scripts/smoke-transgate-runtime.py`, `/tmp/rustr-p3-taskr-smoke.py`.
Saved logs: `/tmp/rustr-p3-integration-logs/`.

## Docker context boundary

Cause (from the before-fix probe, `/tmp/rustr-context-probe-before.tar`): Docker re-includes everything beneath a directory exception, so `!crates/`, `!transgate/` and `!crates/*/` let `crates/sample/README.md`, `tests/fixture.json` and `transgate/docs/production-facts.md` into the context. The processor file had the same shape (`!crates/ !modules/ !transgate/`) plus a deny list.

Fix: one identical whitelist in both `transgate/Dockerfile.dockerignore` and `transgate/processor/deploy/Dockerfile.dockerignore`. Only file patterns are re-included: root `Cargo.toml`/`Cargo.lock`, and for every member directory (`crates/*`, `modules/billing/*`, `modules/billing/gateways/*`, `transgate/*`) its `Cargo.toml`, `src/**/*.rs` and `migrations/*.sql`. The final `.env`/`.env.*`/`*.pem`/`*.key` exclusions stay as a security boundary, each with a `/**` form. Both Dockerfiles now `COPY . ./` the bounded context, because Cargo loads every member (the server image previously lacked core, processor, api, http, export and jobs-postgres). Rust 1.99.1, `--locked`, the separate FFmpeg processor runtime, non-root user, health checks and compose hardening are unchanged. The root jobs-postgres member/lock entry is owned by the lead.

Second finding (canary-proved): with only `**/.env`, `**/*.pem`, `**/*.key`, the source exceptions still let `src/.env/leak.rs`, `src/cert.pem/leak.rs` and `src/creds.key/leak.rs` through, because the deny matches only the parent directory. The old files had the same deny lines and the same gap. The `/**` forms close it.

Why this is enough for a release build: no member has `build.rs` or compile-time `query!` macros; `sqlx::migrate!("./migrations")` reads only `migrations/*.sql`; the only `include_str!` of non-source files (`tests/fixtures/*.pem`, `transgate/fixtures/languages/*.json`) are under `#[cfg(test)]`; every explicit `[[bin]] path` is under `src/`.

### Exercised

| Check | Result |
|---|---|
| Synthetic canary probe, actual `docker buildx build` COPY + tar export, both Dockerfile paths (`/tmp/rustr-context-probe-after-tmqp31xw`, canary map `probe-canaries.json`) | server and processor: 24/24 positives present, 0 of 46 negatives leaked, identical ignore files (`cmp` 0) |
| Negative canaries in that probe | excluded: READMEs, `tests/**` (incl. `.rs`, `.pem`, `fixtures/`), `src/.env.private`, `src/*.pem`, `src/*.md`, `.env.local.rs`/`.env.rs`/`.env.prod.rs` sources, `.rs` inside `.env/`, `.env.prod/`, `cert.pem/`, `creds.key/`, `b.key/c/` dirs, `migrations/.env.sql`, `migrations/.env.d/*.sql`, `target/**/*.rs`, `node_modules/**/*.rs`, nested non-migration SQL, `transgate/docs/**` (incl. `.rs`), `transgate/fixtures`, `transgate/core/scripts`, `transgate/processor/deploy/*` (incl. `.env`), Dockerfiles, `apps/`, `book/`, `docs/`, `scripts/`, `.git`, root `.env`. Included look-alikes: `src/env_settings.rs`, `src/keys.rs` |
| Real repository context listing with the same ignore file, export, names compared to the filesystem (`real-context-listing.txt`) | 246 files = 246 expected over 28 members (includes `transgate/jobs-postgres`), 0 missing, 0 extra, nothing other than `.rs`/`.sql`/manifests/lock |

### First Sol image snapshot (packaging evidence, superseded)

Both product Docker builds now pass with Rust 1.99.1 and `cargo build --locked --release`. The server Docker runtime smoke passes: non-root/read-only, healthy process liveness, billing readiness remains 503 when unconfigured, product routes remain 404, and graceful stop. Processor runtime also passes: non-root/read-only with writable private `/tmp`, healthy `/health`, actual FFmpeg and ffprobe 5.1.9, graceful stop. These are packaging/liveness checks, not configured-provider acceptance.

## Auth-repaired Sol snapshot (second handoff)

After the observed stale-JWT 503, lead reported a clean rebuild with the contact/auth repair and 11/11 isolated transgate-billing tests (artifact542). Copied binaries to a new immutable `/tmp/rustr-p3-auth-sol/bin/`; the first snapshot and its failure logs remain intact.

| Binary | SHA-256 |
|---|---|
| transgate-server | `85fea3ad98e87e7ab09aff8f82c3858ee84bcd85b6da5de89c2f4b075009028d` |
| taskr-server | `fa5b79b37cbc3763ddc3f01a5a22c88e1b3d7173c004b3750e4ef5ed5de19b0a` |
| billing-provision | `92ba0cce51ba8f29c6d53d36971875fe8b4a7dc260961a1b3519ac9e9f4356d3` |

At this handoff the unchanged full Transgate smoke started with real `--capability-wait` (`/tmp/rustr-p3-auth-sol/transgate-run1/`), the repaired server image rebuild started, and the processor rebuild was held for W's child-survives invalidation repair (artifact544). Later outcomes are recorded in the pause, resume and completed-verification sections below; the first processor image is historical packaging/liveness evidence only.

## First Sol verification handoff

The same saved session resumed directly on Sol after Claude quota/auth blocked it; earlier findings and logs remain retained. Lead supplied a coherent successful build (artifact504) after jobs-postgres and retention repairs. Before testing, copied all three binaries into `/tmp/rustr-p3-final-sol/bin/`; smoke `--root /tmp/rustr-p3-final-sol` uses only those immutable copies, not a scratch Cargo workspace or concurrent build outputs.

| Binary | SHA-256 |
|---|---|
| transgate-server | `73b0c2cc6254474b4e01f9abf8e0840a95ac3401fa3d251986b45561e7dd1aea` |
| taskr-server | `c395d072e1e2a78269eb20116673293faa4a900365b560ba37401f7f2c2cbb51` |
| billing-provision | `182f72852fc0efeb4ed8f4622e315d22616628d40c31310b8e771cc600301885` |

| Exercised check | Actual result |
|---|---|
| Local runtime, `--binary /tmp/rustr-p3-final-sol/bin/transgate-server` | PASS: both signals, bind refusals, liveness/readiness separation, jobs config/legacy-key/missing-billing refusals |
| Taskr full short smoke, owned postgres:17-alpine cluster | PASS, including 410 account_erased, zero-row checks, before/after-ACK callbacks and restart |
| Taskr real expiry extension | PASS in 908.28s: after real signed-PUT expiry, DELETE returns 202 complete:false, known late re-PUT removed, object remains explicitly unresolved; zero local rows remain zero. `taskr-real-expiry.log` |
| Transgate current run1 | Fixture error: Carol's 30-minute job correctly routed to Deepgram, but smoke waited for Soniox. Fixed fixture provider/payload; money assertions unchanged |
| Transgate current run2 | PRODUCT FAILURE at post-erasure `GET /api/jobs`: expected 410, actual 503 identity_unavailable; fixture healthy |
| Server Docker build + runtime | PASS; release build 2m54s, overall 186s |
| Processor Docker build + runtime | PASS; build 117s; actual health/FFmpeg/ffprobe/graceful stop exercised |

Current Transgate run2 passed everything before erasure: exact persisted archive intent `{account}/{job}/{write-uuid}/{file}` in the archive bucket, one PUT without overwrite and matching provider payload identity; corrected F6 divergent callback 409 (duplicate 200, state unchanged); stable mail retry across restart; complete ZIP/OOXML output; FRE-022 Carol driven to **-10 minutes** through HTTP first-job freemium bypass, complete txt/Word/subtitle/CSV download without debit, then another start refused 402. Cross-account capability attribution now matches the caller (the read-only assertion passed).

**Observed first-snapshot blocker, repaired in the second handoff:** after `DELETE /api/delete-account` returned 202 local_data_erased, the customer row was gone while legacy phase-0 inventory was still running. `ContactRefresh::authenticate` (`transgate/billing/src/lib.rs`) called `PgCustomerDirectory::by_account`; the absent row led to `adopt` (`directory.rs`) and an INSERT rejected by the erasure fence, mapped to `AuthError::Unavailable`. The valid stale JWT therefore got **503** `{"code":"identity_unavailable","error":"The sign-in service is not reachable."}` before the jobs barrier could return 410. Repeated self-erasure continuation being blocked was a **source inference**, not a second observed request: the run stopped at GET /api/jobs. No authentication workaround, assertion weakening, SQL product mutation or Rust edit was used. Reported to lead and p1T, whose fix separates ordinary/admin erased typed410 from a signature-verified account-only self-erasure continuation method. The later saved-snapshot run verified the repaired jobs410/billing410 and continuation path.

Logs retained under `/tmp/rustr-p3-final-sol/`: `transgate-run1/`, `transgate-run2/` (server logs and fixture traces), `taskr-short.log`, `server-docker-runtime.log`, `processor-docker-build.log`, `processor-runtime.log`. Processor image manifest list: `sha256:727887b93b3f6fbd5ff1f44bc73e02468eebb25e03991d1b21ffa6e692a23f13`; server: `sha256:e9b01105082033af3e1554532e2bc363324b237d33176d501caa5fde42dabbcd`.

## Baseline process smokes (2026-10-24, lead-built binaries, not final P3 acceptance)

Binaries (`/tmp/rustr-p3-integration-logs/binaries.txt`, with sha256): `transgate-server` 20:58:39, `taskr-server` 21:01:02, `billing-provision` 21:00:53. They predate the history-retention slot (`Schedule::ALL`, media migration 0002) and the missing-row 410 edge; rerun after the rebuild. Every run used its own labeled `postgres:17-alpine` cluster, removed afterwards.

| Smoke | Result | Log |
|---|---|---|
| `scripts/smoke-transgate-runtime.py` (local) | PASS: SIGINT/SIGTERM graceful, liveness vs 503 billing readiness, job routes 404 without jobs config, bind refusals, jobs refusing incomplete config or missing billing without echoing values. The script's own `cargo build` was skipped: at run time it failed on an in-flight `retention.rs` edit, so the lead-built binary was exercised | `runtime-smoke-local.log` |
| `/tmp/rustr-p3-taskr-smoke.py --postgres-image postgres:17-alpine` | PASS, first run, including 410 `account_erased` for stale billing enroll/sample | `taskr-smoke.log` |
| `/tmp/rustr-p3-transgate-smoke.py` run3 | FAIL on product findings only; every scenario executed | `transgate-smoke-run3.log`, `/tmp/rustr-p3-transgate-smoke-run3/` |

Transgate run3 passed: non-HTTPS callback origin and incomplete storage group refused before listening; startup migration with a restricted runtime login; SigV4 signed PUT (tampered signature 403), signed player GET, provider GET; another owner gets only their own folder and 404s; Soniox deferred start charged from the webhook's 150 s (3 min) over the 1 s token end, one transcript fetch, auto-delete, refusals 401 (secret, unknown attempt), 409 (job, transport, identity), 400 (malformed), duplicate and divergent deliveries acknowledged without change; Deepgram by duration override, strict start charge, Basic-auth callback after a SIGTERM restart, 61 s lowers duration to 2 without refund; failed job mails the user, the support outage survives restart and is delivered after the retry delay with the same idempotency keys; owner-only line edit appears in downloads; all-formats ZIP (txt, OOXML Word, srt, vtt, csv) and WYSIWYG variant, no charge, `transcript_downloaded` to PostHog; retention opt-out persists across SIGINT restart; cron secret gate; erasure answers 202 `local_data_erased`, deletes the GoTrue identity, 410 `account_deletion_in_progress` on job routes and 410 `account_erased` on billing (also after restart), erased callback binding 401, upload objects and Soniox copies deleted, zero rows in the jobs, customer and billing tables, other account intact, erasure stays open for the Deepgram copy and for upload re-checks while a capability is valid.

Product findings (sent to lead; owners named):

1. JOB-044 archive payloads never reach the archive bucket. `Jobs::archive_payload` (`transgate/core/src/jobs/app.rs`) picks `object_store(archive_store)` by kind, which is the upload-bucket adapter; `S3Store` rejects bucket `archive-*` and the best-effort error is dropped. Fixture trace: no archive PUT. Fix direction: `object_store_for(&object)`.
2. Cross-account upload capability. `record_issued_upload` (`transgate/jobs-postgres`) takes `account_id` from the job row, so Bob's `init-upload` with Ada's job id records `userId=<bob>/...` under Ada. Ada's erasure manifest then lists `upload re-check userId=<bob>/jobId=1/steal.wav`, and Bob's own erasure would miss it. The insert also refuses a nonexistent job id, contrary to the core `init_upload` doc.

Not exercised by the smoke: 30-day retention run (02:00 UTC slot), weekly history retention, lease/stuck windows, live providers, TLS, JWKS.

Run4 `--capability-wait` used the same baseline binary and a real 16.5-minute wait; it took 1082 s (`transgate-smoke-run4-capwait.log`, `/tmp/rustr-p3-transgate-smoke-run4-capwait/fixture-trace.json`). After the capabilities expired, the owner's erasure request ran a signed HEAD on every recorded upload key, then deleted the late re-PUT of `jobId=1/soniox-smoke.wav`. That binary then dropped the expired holds from the exception list. This is observed old behaviour, not proof that no write can land later: under the resource-review policy, an expired presigned URL plus an absent HEAD does not fence an earlier accepted PUT. The final smoke asserts that the HEAD re-sweep and the late-object deletion still happen, and that issued uploads stay pending after expiry. Erasure stayed open for the Deepgram copy; the run failed only on findings 1 and 2. The trace also shows the impact of finding 2: Ada's erasure sent `HEAD /uploads-*/userId=<bob>/jobId=1/steal.wav`, a key in Bob's folder.

## `scripts/smoke-transgate-runtime.py`

Updated for the actual host composition in `transgate/server/src/main.rs`. Without the jobs configuration, the real job routes `/api/v1/transcriptions` and `/api/jobs` must return 404; this replaces the invented `/api/transcriptions`. `TRANSGATE_JOBS_ENABLED=1` must make the server exit non-zero before listening, with the named setting or billing message and without echoing values, in two cases: an incomplete jobs group, and a complete synthetic jobs group with no billing. `/health` liveness and `/api/billing/ready` 503 readiness stay separate.

## `/tmp/rustr-p3-taskr-smoke.py`

Requires HTTP 410 and `error.code == "account_erased"` for stale `POST /api/billing/enroll` and `/api/billing/sample/work`, followed by zero-row SELECTs. Run with `--postgres-image postgres:17-alpine` (`postgres:17` is not cached).

## `/tmp/rustr-p3-transgate-smoke.py`

New, owned by this slice. Env and routes come from `transgate/server/src/host.rs` and `transgate/http`. Three distinct buckets (upload, archive, staging). Product mutations go through HTTP only. SQL is limited to database creation, read-only catalog checks and zero-row checks. Mid-run product defects are recorded as findings, the remaining scenarios continue, and the run fails at the end.

After run3, the smoke was aligned with the source contract for the next source-current binary. These changes are not yet run:
- F6: a divergent callback now expects 409. A duplicate still expects 200. Job and balance must be unchanged.
- `LEGACY_SUPABASE_URL` and its key now point at a fixture of the legacy Storage `jobs` bucket (list, info, prefix delete). Ada and Bob each have one seeded legacy object. Erasure must list only Ada's folder, delete her object, and keep Bob's.
- D2: the legacy Supabase store uses dedicated credentials, `LEGACY_SUPABASE_URL` and `LEGACY_SUPABASE_SERVICE_ROLE_KEY`; the fixture key is separate from the GoTrue admin key. Both smokes require startup to fail and name the missing `LEGACY_SUPABASE_SERVICE_ROLE_KEY`. The deletion case lists objects at depth (`{owner}/legacy-7/…`). Erasure completes on the empty listing after the 2-minute retry.
- Erasure hold checks no longer depend on wording. Every upload key of the erased account must appear as an exception while its capability is valid, and the GoTrue deletion must target exactly that identity.
- FRE-022 exhausted account (Carol, locale `tr`): the first-job freemium bypass of grant+10 minutes drives the balance negative. After that, the complete all-formats download (txt and Word text checked) does not change the balance, and a new start is refused with 402 `insufficient_balance`.

## User pause handoff

User requested an immediate pause until morning and explicit lead resume. No more builds/checks/retries are scheduled. The only running owned job, auth-repaired Transgate `--capability-wait`, was cancelled via SIGTERM; its handler retained diagnostics and cleaned up its processes/container (exit130 at 690.17s, no fixture failures, no remaining labeled Transgate smoke containers). No cleanup blocker.

The repaired run reached the final real-expiry wait, so all preceding assertions—including stale-JWT jobs410, repeated self-erasure through legacy inventory/identity/object phases, local zero-row checks and restart—passed. The final real-expiry assertions were **not reached**, so this run is **cancelled, not PASS**. Logs: `/tmp/rustr-p3-auth-sol/transgate-run1/`. The repaired server Docker rebuild/runtime passed in 138.79s; log `/tmp/rustr-p3-auth-sol/server-docker-runtime.log`, image manifest list `sha256:1bd164164daf09941a7f2efa870edf60e05d27b7be7c6e1e827773b9bfe64a59`. Processor rebuild remains held for W's invalidation repair. All earlier snapshots/findings remain intact; lead w23:p1E has the pause report.

## Morning resume verification

User explicitly resumed verification. Saved auth-snapshot binary hashes were rechecked unchanged: Transgate `85fea3ad98e87e7ab09aff8f82c3858ee84bcd85b6da5de89c2f4b075009028d`, taskr `fa5b79b37cbc3763ddc3f01a5a22c88e1b3d7173c004b3750e4ef5ed5de19b0a`, provision `92ba0cce51ba8f29c6d53d36971875fe8b4a7dc260961a1b3519ac9e9f4356d3`. The deliberately cancelled Transgate run restarted unchanged with `--capability-wait` on an owned isolated cluster (`/tmp/rustr-p3-auth-sol/transgate-resume/`). It reached final assertions after1204.90s but exposed a fixture timing defect: the scheduled worker had already done all three HEADs and deleted the late re-PUT (trace lines200–218), whereas the assertion demanded an additional HEAD inside the final owner DELETE. The corrected smoke records wall-clock event times, checks each exact owned key's HEAD after its actual signed-URL expiry and a post-expiry DELETE of the late re-PUT, accepts either scheduler or owner trigger, and still requires every upload hold explicitly present plus honest pending state. The subsequent timed run passed; details follow below. Preserved taskr real-expiry PASS908.28s and repaired server Docker PASS138.79s; neither rerun.

Lead authorized the final processor image after W's expanded suite passed53 (artifact562), following the observed invalidation failure544. Before P4 writers could affect Cargo membership, froze the actual allowlisted Docker context via COPY/export into `/tmp/rustr-p3-processor-resume-q64d_sdu/context` (255 files including Dockerfile/ignore, no P4 manifests). `snapshot.sha256` fingerprints every input; `context-export.log` retains export proof. No product source or root manifest was changed.

Corrected processor Docker build **PASS60.49s**, Rust1.99.1 `cargo build --locked --release --package transgate-processor`. Image manifest list `sha256:0dc1c424c5aa3b0f63a488d1888cbf793269fda07b41ae7aaee776ce2f4da899`.

Actual corrected container runtime **PASS6.88s**: UID/GID65532, read-only root, isolated owned disk volume at `/tmp`, healthcheck + `/health`; multipart three-second WAV upload; actual FFmpeg conversion to Opus with 8000Hz request forced to 48000Hz; downloaded Ogg stream verified by real ffprobe as Opus/48000Hz/stereo; consumed session410; explicit invalidation200 then410; graceful exit0 without OOM. Container and volume were removed. This proves packaging and the exercised codec/session flow, not the full53-case invalidation suite. Logs: `product-build.log` and `runtime-smoke.log` in the processor snapshot directory.

## Completed saved-snapshot verification

Corrected full Transgate `--capability-wait` **PASS1207.62s** on saved `85fea3ad…` auth-repaired binary, with no product source changes or clock injection. `/tmp/rustr-p3-auth-sol/transgate-resume-timed/full-run.log` retains complete output; `fixture-trace.json` retains timestamped exact-key requests and completed scenario reports. The failed trigger-specific run remains preserved separately in `transgate-resume/`.

Final assertions proved each exact owned upload key received a HEAD at/after its real signed-URL expiry; the late known re-PUT received a DELETE after both the late PUT and expiry; that object was absent; every issued-upload key remained explicitly represented in exceptions; pending count still covered uploads plus unsupported Deepgram cleanup. Thus no expiry/absent-HEAD finality waiver. All preceding owner isolation, local zero-row checks, JWT jobs410/billing410, repeated self-erasure through legacy inventory then identity/object phases, restart, original-origin charging, exact archive intent/payload, negative-balance FRE-022 ZIP/OOXML and stable mail assertions passed. Fixture errors were empty.

The successful script prints a post-cleanup attempt to collect PostgreSQL logs (`No such container`); the cluster had already been removed by its cleanup. This is a diagnostics-order limitation, not a failed product assertion or cleanup failure. Retained server logs/fixture evidence and complete PASS output remain available.

Final cleanup inventory: no containers with the owned taskr, Transgate or processor smoke labels; no owned processor volume. Saved binary SHA-256 values rechecked unchanged. Corrected processor build/runtime PASS60.49s/6.88s, repaired server Docker PASS138.79s and taskr real-expiry PASS908.28s remain the independent evidence described above. No live-provider calls or host workspace compilation were performed in this resumed verification.

**Deferred separately:** p1T's newly added D15 admin exception HTTP endpoint is source-only relative to saved `85fea3ad…`; lead explicitly requires a new coherent binary before its approval smoke. It was not invoked, no operator approval was applied, and it is not covered by these PASS results.

## P4 Transgate AI process harness

The same saved session resumed on Opus. The P3 snapshot evidence above is unchanged. The harness is `scripts/p4-ai/smoke-transgate.py`; it was source-only until the lead's binary handoff, and its executed result is recorded below. It runs only on a coherent handed binary:

`python3 scripts/p4-ai/smoke-transgate.py --postgres-image postgres:17-alpine --root /tmp/<snapshot> [--log-dir /tmp/<new>] [--capability-wait]`

The `--root` directory must hold immutable copies of `transgate-server` and `billing-provision` under `target/debug/`. The harness reuses the P3 setup: its own labeled cluster and restricted login, loopback fixtures (R2 with SigV4 checks, Soniox, GoTrue, mail, PostHog, the legacy Supabase jobs bucket), and HTTP-only mutations with read-only SQL evidence. It adds a Lemon Squeezy fixture (a real HTTP purchase makes one account paid) and a controlled DeepSeek-compatible `/chat/completions`. That endpoint checks the D1 request: `deepseek-chat`, temperature 0.3, max_tokens 1000, stream, store=false, ephemeral=true, no `response_format`. It classifies each call by its source system prompt, routes it by a per-job transcript marker, and serves only planned answers, with the default general summary counted separately. The six analysis answers are the core fixtures in `transgate/core/src/ai/analysis/fixtures/`.

Minimal host configuration added to the P3 jobs/billing env: `ENABLE_AI_FEATURES=true`, `ENABLE_AI_CHAT=true`, `DEEPSEEK_API_KEY=p4-synthetic-key`, `DEEPSEEK_BASE_URL=http://127.0.0.1:<fixture>` and `AI_CHAT_MAX_QUESTIONS_PER_TRANSCRIPT=2`. `TRANSGATE_ADMIN_EMAILS` lists two synthetic admins.

Scenarios, each a hard assertion:

- **Startup and consent bootstrap.** Blank or missing-key DeepSeek configuration refuses startup by name. Consent and chat-limits metadata are readable before consent. A request before consent gets 403, a foreign request 404, and a coerced boolean, `system` history or disagreeing Idempotency-Key gets 400, all with no provider call.
- **Six typed analysis shapes.** One mid-string chunk split, and a provider held after three documents: `sectionCount` (documentCount-2), `typeDetected` and `sectionReady` must arrive before `complete`. The saved typed result has the camelCase keys of each shape. An exact replay attaches to the same generation without redispatch. The D8 invalid section interrupts and keeps valid partials.
- **Chat, quota and paid bypass.** Chat sends cumulative snapshots, and the saved final `Hello world` replaces them. The original question is persisted. Exact replay returns the same generation; a conflicting key gets 409. Retry (202, then 200 on replay) repeats the question and appends a pair. The transcript cap returns 429 `TRANSCRIPT_QUESTIONS_LIMIT_EXCEEDED` for both chat and retry without dispatch. A freemium account gets 409 `ai_busy` while another of its chats runs. Cancel before save wins and releases the reservation. A Lemon-paid account shows `hasLimits:false`, gets past the cap and has no quota counter rows.
- **Held provider.** Reattaching gives a resnapshot and then the saved replacement, with one provider request. A future cursor gets 400 and a foreign watch is refused. Four watches fill the account; the fifth watch and a legacy start get 503 before admission (read-only SQL shows no row). D4 idle disconnect: after those four observers close while the generation is still unchanged, a new watch must be admitted within 5 s. A miss is recorded as a product finding (slot retention plus graceful-SIGTERM delay), the remaining scenarios continue, and the run fails at the end. Slots return after EOF. Refusal interrupts a held generation as `revoked`, keeps completed chat, and regrant re-exposes it. The 60 s deadline gives `deadline` with the remote outcome unknown. SIGTERM restart mid-stream gives `worker_lost` with the remote outcome unknown, the replay returns the same id, and there is no redispatch. A transcript edit gives `source_changed`.
- **Completion summary and mail.** Every completed job saves exactly one general summary; owner reads go through `/api/ai/jobs/{job}/summary` and job detail. An unconsented account is `denied` with no call. While one completion summary is held, another job's failure mail (user and support) is still delivered. A provider 500 records `failed`, the job stays completed, and no transcription mail mentions it.
- **Consent DELETE.** DELETE purges all generation epochs (404 afterwards), regrant shows nothing old, and dispatch intents survive.
- **D9 dispatch intent.** The payload-free intent (provider, model, observed id) survives consent DELETE and job deletion. Once the local erasure cascade has committed, it exists only as an unresolved `LLM provider retention is unverified` manifest item, and the manifest is payload-free. Zero account rows remain in `generations`, `chat_messages`, `completion_summaries`, `summary_requests`, `quota_counters`, `llm_dispatches` and `ai_consent`. The status is never `fully_erased`.
- **D15 admin exception, on the new binary only.** A non-admin gets 403 even for a non-JSON body. A forged `operator` field or a non-JSON body from an admin gets 400. An unknown item gets 404, and a done or retrying item gets 409 with code `conflict` (the message is not pinned). A trusted admin gets 200, with `approved_by` taken from the session email, and the status is still not `fully_erased`. An erased admin gets 410. With `--capability-wait`, once every unresolved item is approved after real upload expiry, the closure must be `erased_with_exceptions`.

Gaps this harness does not cover:

- **Owned PG tests:** quota original-period rollover, unknown-COMMIT reconciliation, quota-store failure and imported history.
- **Live prerequisites:** DeepSeek model acceptance, retention, region and cost.
- **Other slices:** taskr, browser and Swift runtime (p22).

p1T answered the five factual questions:

- The committed account cascade deletes `ai_consent` and `llm_dispatches`; the harness now asserts zero rows.
- A nonapprovable item returns 409 with code `conflict`.
- An idle disconnected watch is not intended to keep its slot, but runtime release is unknown; the harness now probes it and records a finding.
- Retry after a transcript edit is allowed; the edit scenario stays last as a conservative choice.
- `summary_requests.outcome` is the source of truth. Evidence is scoped by account, job and newest `source_revision`, and NULL means pending.

### Executed on the lead's coherent binary (2026-10-25)

Snapshot `/tmp/rustr-p4-transgate-lead-yjn_v9wg`: `transgate-server` `f1ca960c984bff8a0011c4a7ad1115a4d8b32e57af1a4698f87811e84ee94ed7`, `billing-provision` `10f9e6033cc8461108d6ecc35a32118c0c3f2fe3c03bbab66a5589af81e65fc0` (rechecked unchanged after the run). Own `postgres:17-alpine` cluster each run, all removed.

| Run | Result | Cause |
|---|---|---|
| run1 | stopped in setup | Fixture expectation. `main.rs:142` reads whitespace-only env values as unset, so a blank `DEEPSEEK_API_KEY` is refused by name with "is required when DEEPSEEK_BASE_URL is configured". The `must not be blank` branch of `Settings::from_env` cannot be reached from a process env. |
| run2 (run3 stale) | stopped in bootstrap | Fixture expectation. `GET /api/ai/jobs/{job}/summary` without consent returns 200 `{summary:null}` (core `completion_summary` returns no content, app.rs:804). It is still a hard check that no content is exposed, the outcome is `denied` and nothing was dispatched. |
| run4 | every assertion before the real wait passed, then 401 | Environment. The laptop suspended for about 2.4 h during the wait (journal resume 17:36:03, trace gap 8687 s) and the 2 h synthetic JWTs expired. JWT lifetime unchanged. |
| **run5** | **PASS, 1187.25 s** | Unchanged harness under `systemd-inhibit --what=sleep:idle` (process-scoped, no persistent power config). Trace span 1178 s, largest gap 997 s (the deliberate wait). |

Run5 proved every scenario listed above with fixture failures `[]` and no product findings. The D4 idle-disconnect probe admitted a new watch within 5 s. There were 32 controlled provider calls (13 chat, 12 summary, 7 analysis), each planned or the counted default summary. After real upload expiry and explicit operator approvals, erasure closed as `erased_with_exceptions`, never `fully_erased`. Logs and traces: `/tmp/rustr-p4-transgate-run{1..5}/` (`full-run.log`, `fixture-trace.json`, `transgate-*.log`).

Still not covered here: quota original-period rollover, unknown-COMMIT reconciliation and quota-store failure (PG tests); live DeepSeek model acceptance, retention, region and cost (P9). Synthetic loopback output only, so this is not a P4 green claim.
