# P7 cross-critique: Opus on Fable

2026-10-26, medium effort. I read `fable/proposal.md`, re-read the source behind every disputed claim, and left both proposals unchanged. No builds, tests or provider calls.

## 1. Where Fable is wrong or unsupported

| # | Fable claim | Evidence | Correction |
|---|---|---|---|
| W1 | Failed-job callbacks are "[unverified] … call site not inspected" (§6) | Callbacks are sent only from `WebhookHandler.handleJobCompletion`. It returns early when `event.status !== 'completed'` (`frontend/src/services/unified-transcription/utils/webhook-handler.ts:42-45`) and again when `job.job_status !== 'completed'` (`:64-67`). Both callers pass `status: 'completed'` (`frontend/src/utils/deepgram.ts:600-603`, `frontend/src/utils/soniox.ts:537-540`) | Settled: **Completed only**. A callback for failed jobs would be a new feature |
| W2 | The payload carries an optional `general_summary` (§6) | Source field is `payload.summary = { text }` (`callback-delivery.ts:123,135,172`, the `summary` key on `CallbackPayload`) | Use `summary: { text }`. Renaming it breaks live receivers |
| W3 | `User-Agent: Transgate-Callback/2.0` (§6) | Source sends `Transgate-Callback/1.0` (`callback-delivery.ts:199`, API-016) | Keep 1.0. Receivers may filter on the user agent, and nothing justifies a bump |
| W4 | New key format `tg_<prefix8>_<secret43>` (§3) | Source is `'sk_' \|\| encode(gen_random_bytes(32),'base64')` (`remote_schema.sql:400`, API-002). The public docs and the 2 active owners know `sk_` | Keep the `sk_` format and look keys up by SHA-256 digest. A visible prefix needs no format change: store an 8-character hint. A new format is an observable change with no corrected defect behind it |
| W5 | Stop the audit log; `last_used_at` and records replace it (§3, P7-D4) | The source verifier writes a row labelled "HIPAA audit log for API request" for every verification (`remote_schema.sql:1271-1274`). Production holds 872 rows (`production-facts.md:9`) | Dropping a compliance-labelled record is not an "audit duplicate" simplification. Keep one usage row per request (my `api_key_uses`) and give its IP retention to the privacy owner. Do not decide it in P7 alone |
| W6 | Customer-facing `GET /api/v1/transcriptions/{id}/callbacks`, `POST /api/v1/callbacks/{delivery}/replay` and `GET /api/keys/callback-secret` (§2, §6) | The source has no delivery read, no replay and no secret UI. The PRD forbids new capabilities and puts UI improvements behind an off-by-default flag (`docs/prd.md:46-49`) | Replay is operator-only (admin allow-list, IDN-032). Secret display ships behind an off-by-default flag until mu enables it. No new public endpoints besides `/api/v1/openapi.json` |
| W7 | `utoipa::ToSchema` derives on `StartRequest`, `UrlRequest`, `UrlItem`, `StartAnswer` (§8) | These are **core** types (`transgate_core::jobs::app`, imported at `api/src/lib.rs:15`). Core holds the domain. The public contract is the hand-written parser over `Value`, which keeps the source 400 messages (`api/src/lib.rs:1-8`) | Deriving on core couples core to an OpenAPI crate, and a schema derived from core types does not describe what the parser accepts. Put the DTOs in `transgate-api` with an agreement test against the parser (§3, "Spec generation") |
| W8 | Default **no shim**: drop F03/F04 unless mu objects (P7-D1) | The audit endpoint column is empty, so the version the 2 live owners use is unknown (`production-facts.md:9-11`; api.md R3: "Assume compatibility is required until mu chooses") | The default should be the reverse. Keep the shim until the owners confirm. Dropping it can break a paying customer at cutover |
| W9 | The limiter order means "malformed body does not consume"; the source charged before validation [INFERENCE] | Source: `check_rate_limit` runs inside key verification, before routing (`v1-beta/transcriptions/auth.ts:113-147`) | Fine as a correction, but mark it as a change. It needs the body parsed before the limit, which lets malformed floods bypass the limiter. I recommend authenticate, then `can`, then consume, then parse |

## 2. What Fable gets right that I missed (adopt)

- **WEB-045/046.** The key screen rows exist in `parity.md:98,198`. I listed only API-F08/F09. Both pairs close jointly with P5.
- **API-F06 needs a `dropped` record.** P5 D5 replaced `/api/transcripts`. `http/src/lib.rs:9-11` says it is "Not mounted". I wrongly listed it as not P7.
- **`ApiTx` on the same `JobUow` connection**, following the `AiTx` pattern (`core/src/ai/ports.rs:1-9`). It is cleaner than my separate stores and keeps account-lock order and the erasure barrier in one transaction.
- **Delivery payloads with summaries join the P4 DELETE purge.** The P4 port says "stored callback copies (P7) join this purge" (`core/src/ai/ports.rs:392-399`). I covered retention but missed consent purge.
- **`audio_url` is never stored in the record; it is re-signed at dispatch.** This is better than storing a signed URL that expires.
- **The attempt row is written before the network call.** That gives at-least-once delivery, and the delivery id is the receiver's dedupe key. It is more precise than my single delivery row.
- **DEF-017 stays mu's.** A deferred-charge failure at completion still completes the job; P7 ports this as is.
- **`can` closes the scope gap, and key actors never manage keys.** Same as mine; Fable's explicit action list is complete.
- **Packaged drift check.** Comparing the spec hash rendered into the P6 docs with `/api/v1/openapi.json` on the server is a good check. I adopt serving the spec at that path too.

## 3. Disposition of every open decision

| Decision | Recommendation | Deciding evidence |
|---|---|---|
| Shim for old callers (v1-beta, unified) | Keep the v1-beta shim plus the unified 301, retiring the shim only after both owners confirm; mu decides | `production-facts.md:9-11`: 2 live owners, version unknown. api.md:257 says to assume compatibility. The source already sends sunset headers (API-013) |
| Signing | Per-account HMAC-SHA256 header, default on. The secret is readable only behind a flag-off UI until mu enables it | API-D06: the source sends no signature header (`callback-delivery.ts:197-200`). Receivers ignore extra headers. A new UI needs a flag (`prd.md:49`) |
| Retry schedule | 6 attempts (+0, 1 min, 5 min, 30 min, 2 h, 12 h). Retry on network error, timeout, 408, 429 and 5xx; other 4xx are final; then abandoned. Delivery id in a header | API-D07: one attempt today, record written after the call. Both proposals chose the same schedule independently |
| Idempotency-Key | Optional header, per account, 24 h. Same body returns the stored 201; a different body returns 409. Written in the start transaction | The source has none, so a retried POST charges twice (`charge_and_start` in `jobs/app.rs:717-770`). This reuses the P4 request-key pattern (`ai/app.rs:596-627`). I move from 422 to Fable's 409 to match P4's conflict semantics |
| Summary wait | Wait while `summary_outcome` is `None`, bounded at 15 minutes, then send without the summary | The source generates the summary synchronously and sends without it on failure (`webhook-handler.ts:76-95`). `summary_outcome` is the published boundary (`ai/app.rs:837-846`) |
| Failed-job callbacks | None (parity) | W1: `webhook-handler.ts:42-45,64-67`; callers pass completed only |
| Retention | Payload (minus `audio_url`) is cleared by the history rule, joins the P4 consent purge, and is deleted on erasure. Usage rows are kept, with IP retention decided by the privacy owner | 30-day rule vs API-017's stored payload. P4 ports note at `ai/ports.rs:392-399`. HIPAA-labelled audit at `remote_schema.sql:1271` |
| Spec generation | `transgate_api::public` DTOs with `schemars`, plus an `OPERATIONS` table that both mounts the router and emits OpenAPI 3.1, plus a parser/schema agreement test. Not `utoipa` on core types and handlers | W7: the parser is the real contract (`api/src/lib.rs:1-8`). Table-driven mounting makes undocumented routes impossible without split annotations. Source spec is 3.1.0 (`public/openapi/openapi.yaml:1`) |
| Rate-limit windows | Calendar-aligned minute and hour windows per key (60 and 1000) in Postgres. Each window is consumed in one conditional upsert; denied requests are not counted | The source uses `date_trunc` minute/hour (`remote_schema.sql:306-307`). API-D03 shows the shared-key collision and double count (`:306-341`). Replicas are plausible under P9. Fable's `(key_id, kind, window_start)` key is the right shape; adopt it |
| Fail-closed | Limiter store error returns 503. Over limit returns 429 with the source header names | RTL-002/API-D04 (`auth.ts:129-144`). The limiter shares the Postgres that serves keys and jobs, so fail-open buys nothing. Header names at `authentication.ts:274-284` |
| Legacy bcrypt rehash and crate | Keep `legacy_bcrypt`. Bcrypt-scan only active, unexpired, not-yet-upgraded `sk_` rows; on success write the SHA-256 digest and null the hash. Use the pure-Rust `bcrypt` crate, pinned by the integration owner, after verifying that the version supports `$2a$` hashes produced by pgcrypto | The source scans every row with `crypt(provided, key_hash)` (`remote_schema.sql:1217-1227`), so the cost is parity. There are 39 active unexpired keys today. pgcrypto `gen_salt('bf')` emits `$2a$`, which must be confirmed against the chosen crate |
| Docs rendering | Static HTML from the committed spec, built by P6. Drop the Swagger "Try it"; keep a curl example per operation. `/openapi/openapi.yaml` stays at the same URL; also serve `/api/v1/openapi.json`. The site pins the spec hash, and the packaged check compares it | `api-reference.tsx:26,39` fetches `/openapi/openapi.yaml` (external links). `swagger-ui-react` (`package.json:121`) would bring React. There are 32 baseline sitemap URLs. Both proposals agree on static rendering |
| Key format and display | Keep `sk_`; store an 8-character hint, displayed behind a flag | W4; the API-005 list shows no prefix today |
| Audit log continuation | Continue one usage row per request; migrating the 872 rows is a privacy decision | W5 |
| Replay | Admin-only, appending an attempt to the same delivery id | W6; IDN-032 admin allow-list |

## 4. Net effect on my proposal

Superseded on these points, without editing it:

- add WEB-045/046 and the API-F06 `dropped` record
- `ApiTx` on `JobUow` instead of separate stores
- an attempt row written before the send
- `audio_url` excluded from stored payloads
- P4 purge coverage for stored payloads
- 409 for an idempotency conflict
- `(key_id, kind, window_start)` limiter rows
- serving `/api/v1/openapi.json`
- recording DEF-017 as mu's

Everything else stands.
