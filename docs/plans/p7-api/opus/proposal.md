# P7 public API: Opus proposal

2026-10-26. Independent design under `docs/plans/p7-api/brief.md`, at medium effort. I did not read the Fable proposal. Reads were limited to source, the rustr tree and the canonical docs. No implementation, stubs, builds, tests, browser runs or provider calls. Claims I did not observe are marked [INFERENCE].

## 0. Recommendation

1. **Most of P7 already has a home.** P3 built the URL-job use case (`JobsApp::submit_urls`, `core/src/jobs/app.rs:717`) and the request parser (`transgate_api::parse_transcriptions`). It also left the HTTP branch deliberately waiting for P7 (`http/src/lib.rs:237-238`: "The URL mode is for API customers: it waits for P7's API-key authenticator"). The completion outbox already carries `callback_url` for "P7 (customer callback)" (`core/src/jobs/events.rs:24-33`). P4 published the callback ordering boundary `Ai::summary_outcome` (`ai/app.rs:837-846`). P7 adds what is missing around these pieces:
   - an API-key actor
   - scope checks
   - a per-key limiter
   - durable callback deliveries
   - one generated OpenAPI document
2. **Keys stay in Transgate core, not a new module.** PRD §5 scopes only `billing` and `cms` as modules. The new code is `transgate/core/src/api/` (domain, `can`, ports), `jobs-postgres` tables, `transgate-http` routes and a dispatcher worker.
3. **The live contract stays exactly as it is.** `/api/v1/transcriptions` POST and GET keep the source shape: it is live, it is documented in the source `openapi.yaml`, and two owners used it in the last 90 days (`production-facts.md:5-11`). The v1-beta and unified aliases are a mu decision. My recommendation is a thin v1-beta shim with its sunset headers (§6, DP-1).
4. **Corrections ship as named decisions:**
   - scope enforcement (API-D01)
   - owner-checked revocation (API-D02)
   - an atomic limiter (API-D03)
   - 429 instead of 401 (API-D04)
   - SSRF pinning at dispatch (API-D06)
   - durable retry (API-D07)

   Callback signing, retries, `Idempotency-Key` and replay change what customers observe, so they are listed with their visible effect (§6).
5. **Billing gets no new authority.** API jobs are charged exactly like any job, through `charge_and_start` and the deferred-duration path that `submit_urls` already calls. There is no per-request price.

## 1. Row inventory (P7-owned vs already proven)

Statuses come from `transgate/docs/parity.md` and `rules.md` as read on 2026-10-26.

| Family | Rows | Today | P7 must make green |
|---|---|---|---|
| Public transcription | API-F01 POST, API-F02 GET; API-007, 008, 009 (URL validation, already in `sources::check_url`), 010 | inventoried / ledger. Parser and use case exist (P3); no API-key route | yes |
| Legacy shapes | API-F03, F04 (v1-beta), F05 unified 301; API-011, 012, 013, 014; defect API-D05 | inventoried | yes, after DP-1 (shim or explicit drop) |
| Authentication | API-001 to 004, IDN-028; defect API-D01 | ledger | yes |
| Key management | API-F07 list, F08/F09 web desktop/phone (joint with P5 views), API-005, 006, 019; defect API-D02 | inventoried | yes. F08/F09 close only together with P5's rendered screen |
| Rate limits | RTL-001, 002, 003; defects API-D03, D04 | ledger | yes |
| Callbacks | API-F10, API-015, 016, 017; defects API-D06, D07 | ledger | yes |
| Key-event webhooks | API-F11, API-018 (keep-throw verdict "throw") | inventoried | disposition row: dropped by decision, route returns 404 |
| Docs | baseline sitemap `/api-docs`, `/api-reference` × 16 (32 URLs), `/openapi/openapi.yaml` | P6 marks them P7-owned | yes, joint with P6 |
| **Not P7** | API-F17, API-020/021 public captions (P3 route exists); RTL-004/005 (P3 `PgRateLimiter`); API-F45 to F84 and API-022 to 030 processor (tested or changed, P3); API-F44 voice demo (dropped, P4 D6); API-F06, F12 to F16 session routes (P3/P5, D5); F18 to F21 AI (P4); F22 to F30 provider, cron and hooks (P3); F31 to F43 ops, platform, billing, privacy | some tested | no; P7 does not reopen them |

## 2. Architecture

### 2.1 Core: `transgate/core/src/api/` (owner p1X)

- **`Actor`:** `Session { account }` or `ApiKey { account, key: KeyId, scopes }`. HTTP builds it, and the route determines which kind is allowed. Public routes accept only `ApiKey` (P5 D5). There is no fallback from an invalid key to a session, and no credential-shape sniffing.
- **`Scope`:** `TranscriptionsRead` or `TranscriptionsCreate`, using the source strings `transcriptions:read` and `transcriptions:create` (`dashboard/api-keys.tsx:37,242-244`). Unknown stored strings are kept and ignored, so migration can carry them.
- **Authorization:** `pub fn can(actor: &Actor, action: Action, resource: Resource) -> bool` is pure. Its action table:

  | Action | Allowed actor |
  |---|---|
  | `TranscriptionCreate` | `ApiKey` with `create` |
  | `TranscriptionRead` | `ApiKey` with `read`, where `Resource::Job { owner }` matches the key's account |
  | `KeysManage` | `Session` whose account owns the key |

  This fixes API-D01 and API-D02. Use cases call `can` first; the HTTP layer never decides.
- **Key domain:** `NewKey { name, scopes, expiry: Never | Days(30 | 90 | 365) | Custom(days) }`, matching the source `expiration_policy` (API-019). `ApiKey { id, account, name, created_at, expires_at, last_used_at, active, scopes, metadata, expiration_policy, secret: KeySecret }`. Verification rules: active, and `expires_at` strictly after `now` (API-003).
- **Ports:** `ApiKeyStore`, `CallbackStore` and `CallbackTransport`, with the same `StoreResult`/`BoxFuture` conventions as P3. All writes go through the existing `JobUow` transaction, so key creation takes the billing account lock like every account mutation, and erasure barriers apply.
- **Use cases:**
  - `ApiKeys::{create, list, revoke, authenticate}`
  - `Callbacks::{enqueue_for(event), dispatch_pass, replay}`
  - `callback_payload(job) -> CallbackPayload`, which assembles the API-015 body from the job, its lines (including `corrected_text`, `transcript/mod.rs:55`), the P4 completion summary visible under consent, and an audio URL produced by the existing P3 storage signer

### 2.2 Key format and storage

- New keys keep the source format, `sk_` + base64 of 32 random bytes (API-002). Clients and docs do not change.
- 256-bit secrets need no slow hash. Store `digest = SHA-256(key)`, unique-indexed, compare in constant time (`sha2`, `subtle` are in `Cargo.lock`). Lookup is one indexed read.
- **Legacy bcrypt keys (P9):** the 46 rows carry their bcrypt hashes into a `legacy_bcrypt` column. The source verifies by scanning every row, `crypt(provided, key_hash)` (`remote_schema.sql:1217-1227`).
  - Verification: digest miss, then bcrypt-check only the active, unexpired, not-yet-upgraded legacy rows (39 today). This is the same scan cost as the source and shrinks over time.
  - On the first successful match, write the digest and null the bcrypt hash (rehash on use; no plaintext is ever needed).
  - Needs the pure-Rust `bcrypt` crate, a new dependency that the integration owner pins.
- **Prefix display:** the source list shows no prefix (API-005). Store an 8-character `hint` for new and upgraded keys. Showing it in the UI is a flag-off improvement (PRD §2).
- **Rotation:** the source has none. Rotation is create plus revoke, unchanged.
- **Revocation:** sets `active = false`, owner-checked (API-D02).
- **Usage records:** `last_used_at` updates on each successful auth. One `api_key_uses` row per authenticated request (endpoint, method, status, ip, user agent, duration). The source wrote up to two audit rows per call (api.md:31); one row is the correction.

### 2.3 Rate limits (state in Postgres)

- **Limits:** 60 per minute and 1000 per hour per key (RTL-001), shared by every replica.
- **Mechanism:** reuse `transgate_jobs.rate_windows` with **calendar-aligned** windows, as in the source `date_trunc('minute'|'hour')` (`remote_schema.sql:306-307`). This is a new aligned mode beside `PgRateLimiter`'s first-hit window. Both windows are consumed in one statement, or in one transaction with locks taken in a fixed order, and only when both allow. That fixes API-D03: one request costs one unit per window, there is no collision at hour boundaries, and there is no read-then-increment race.
- **Denial:** answers **429** with the source header names, which also appear on success (RTL-003, `authentication.ts:274-284`): `X-RateLimit-{Limit,Remaining,Reset}-{Minute,Hour}`. The source answers 401 (API-D04); 429 is the correction.
- **Outage:** the source fails open (RTL-002). The limiter lives in the same Postgres as keys and jobs, so fail-open cannot serve the request anyway. Recommend 503 on a limiter store error (DP-4).
- **Order:** authenticate, then limit, then authorize scope. A denied request does not reach the use case.

### 2.4 Persistence (jobs-postgres migration `0005_api.sql`; owner R/p1R as existing jobs-postgres writer, lead to confirm)

| Table | Contents |
|---|---|
| `api_keys` | id (source uuid kept), account_id, name, digest unique nullable, legacy_bcrypt nullable, hint, created_at, expires_at, last_used_at, active, scopes jsonb, metadata jsonb, expiration_policy |
| `api_key_uses` | key_id, account_id, at, endpoint, method, status, ip, user_agent, duration_ms. Retention is a privacy decision (DP-6) |
| `callback_deliveries` | id uuid (the `Transgate-Delivery-Id`), account_id, job_id, event_id **unique** (outbox fan-out idempotency), url, state (`pending`, `delivering`, `delivered`, `failed`, `abandoned`), attempts, next_attempt_at, lease token/until, payload jsonb, created_at, delivered_at |
| `callback_attempts` | delivery_id, n, started_at, status_code, error, response excerpt (200 chars, as the source logs), duration_ms |
| `idempotency_keys` (only if DP-3 is approved) | account_id, key, request digest, response, created_at, unique (account_id, key) |

- Grants are column-level, as 0001 to 0004 are.
- Account erasure deletes every row in these tables.
- History retention clears `callback_deliveries.payload` together with the transcript lines it copied. The source keeps the payload forever (API-017), which contradicts the 30-day rule. Privacy owner decision (DP-6).

### 2.5 Routes (`transgate-http`, owner p1T)

| Route | Auth | Use case |
|---|---|---|
| `POST /api/v1/transcriptions` | API key only, `create` | `jobIds` keeps the existing `start`. `audioUrls` maps to `submit_urls` with each item's `callbackUrl`. `options.webhookUrl` is validated only, as in the source (`route.ts:699-716`); its use is not wired anywhere in the source [observed], so parity is validate and ignore. Answers 201 with `start_response(…, client_type "api")` |
| `GET /api/v1/transcriptions?jobId=` | API key only, `read` | Owner-scoped detail plus lines plus summary in the source shape (`transcription.ts:719-790`) |
| `POST /api/v1-beta/transcriptions/url`, `GET /api/v1-beta/transcriptions/{id}` | API key only | Compatibility mapping (API-011, 012, 013). API-D05: derive the audio URL from the storage adapter. Only if DP-1 approves |
| `GET`/`POST /api/unified/transcriptions` | none | 301 `ENDPOINT_MOVED`, `Location /api/v1/transcriptions` (API-014) |
| `GET /api/keys/list` | session | Source path kept (API-005) |
| `POST /api/keys`, `POST /api/keys/{id}/revoke` | session | Replace the browser Supabase RPC calls. Plaintext is returned once on create |
| `/api/webhooks` | none | Not mounted; 404 (API-018 throw) |

The session-only `POST /api/jobs/start` (P5 D5) is untouched. The HTTP layer reads the key from `Authorization: Bearer` on public routes only. A request without the header gets 401. A request with a session JWT gets 401 because it is not a key: no fallback.

### 2.6 Callbacks (outbox to delivery to attempts)

```mermaid
flowchart LR
  T[completion transition] -->|same tx| E[events outbox: JobEvent::Completed{callback_url}]
  E -->|relay, idempotent on event_id| D[callback_deliveries pending]
  D -->|lease, SKIP LOCKED| W[dispatcher]
  W -->|summary_outcome None| D
  W -->|POST| R[receiver]
  W --> A[callback_attempts]
```

- **Fan-out:** the existing event relay (`server/src/workers.rs:163-194`) first runs `INSERT … ON CONFLICT (event_id) DO NOTHING` for completed events with a callback URL, then sends its mails. A mail failure releases the event and the retry re-inserts nothing. The delivery intent is as durable as the outbox row, and the P3 transition code does not change.
- **Events covered:** completed only, as in the source. Failed jobs get no callback today (`webhook-handler.ts:33-45`, callers pass `'completed'` only).
- **Dispatcher:** `Callbacks::dispatch_pass` runs in the worker tick (scenario 7: leases, safe with two replicas).
  - Wait for summary: while `summary_outcome(job)` is `None`, postpone. The source generated the summary synchronously before sending and sent without it on failure. Recommend a bound of 15 minutes before sending without a summary (DP-5).
  - Payload assembly at dispatch: the source fetches fresh data when it sends. The assembled body is stored on the delivery.
- **Transport:** a `reqwest` adapter (in the lock):
  - `POST`, `Content-Type: application/json`, `User-Agent: Transgate-Callback/1.0`, 10 s timeout, any 2xx is success (API-016).
  - SSRF fix (API-D06): resolve and validate at dispatch with `sources::check_url` rules, connect to the validated address, follow no redirects, and reject when DNS fails at dispatch (the source allows it).
- **Additions** (observable; DP-2):
  - `Transgate-Delivery-Id`, stable across retries
  - `Transgate-Signature: t=<unix>,v1=<hex HMAC-SHA256(secret, t + "." + body)>` with a per-account signing secret (`hmac` is in the lock)
  - Retry on network error, timeout, 408, 429 or 5xx at +1 min, 5 min, 30 min, 2 h and 12 h (6 attempts in total), then `abandoned`. Other 4xx responses are final.
- **Replay:** admin-only `POST /api/admin/callbacks/{id}/replay`, gated by the existing admin allow-list (IDN-032). It appends attempts to the same delivery id with a freshly assembled payload. Customer self-service replay would be a new feature; not proposed.

### 2.7 Idempotent public starts (DP-3)

The source has none. A retried POST creates and charges a second job.

Recommendation: honor an optional `Idempotency-Key` header, scoped per account and kept for 24 h. The same key with the same body returns the stored 201. The same key with a different body returns 422. Without the header, behavior is unchanged. This reuses the P4 request-key pattern: `UNIQUE (account, key)`, rollback, then a fresh read (`ai/app.rs:596-627`). The record is written in the same transaction as `charge_and_start`.

### 2.8 OpenAPI from `transgate-api` (owner p1W, with p1T)

Today the parsers are hand-written over `serde_json::Value` on purpose, to keep the source 400 messages (`api/src/lib.rs:1-8`). A derive-generated schema would therefore not describe what is enforced.

Recommended single-source design:

- **Operations table:** `transgate_api::public::OPERATIONS`, one entry per method, path, scope, typed request and response DTOs, and status codes.
  - `transgate-http` **builds the public router by iterating this table**, so no route can exist undocumented.
  - A generator binary emits `openapi.json` and `openapi.yaml`, version 3.1 (the source is already 3.1.0).
- **Schemas:** the DTOs derive `schemars::JsonSchema` (a new dependency: JSON Schema 2020-12, which 3.1 uses natively). The responses that `start_response` and the status reader build become these DTOs, serialized with serde instead of `json!`.
- **Committed artifact:** the generated file is committed with a `--check` mode, like `gen-bindings.sh --check`.
- **Agreement test:** a table of valid and invalid request bodies runs through both the hand parser and the JSON Schema validator. The test fails if one accepts what the other rejects. Exceptions such as message-only differences are listed explicitly.
- **Docs truth:** the source `openapi.yaml` promises features and fields not checked against behavior. The generated document replaces it, which is an observable docs change (DP-7).

### 2.9 P6 docs overlap

- `/openapi/openapi.yaml` stays at the same URL; it is externally referenced by `api-reference.tsx:26,39`. The P6 site build copies the generated P7 artifact into it.
- `/api-docs` (editorial, 786 lines) is P6 content. Its code samples are extracted and run against the spec in a P7 test: method, path, auth header and body validate.
- `/api-reference` uses `swagger-ui-react` (`package.json:121`), which conflicts with the no-React rule and the P6 static design. Choose (DP-8):
  - (a) P6 renders the spec into static, indexable HTML and drops "Try it out", or
  - (b) a `swagger-ui-dist` island (no React) keeps Try-it, with CSP `connect-src` set to the API origin.

  Recommend (a), with a curl example per operation.
- Both doc pages stay indexable across the 16 locales, per the P6 baseline.

## 3. Billing

- **No new metering and no second pricing authority.** `submit_urls` already locks the billing account, applies `billing_core::charge::api_precheck` and charges through `charge_and_start`, deferring duration-unknown jobs (`app.rs:717-770`, `charge.rs:152`). `jobIds` mode uses the P3 start path.
- API requests themselves cost nothing, as in the source.
- The rate limit is protection, not billing.
- R consultation only: confirm that no BIL/FRE rule distinguishes API jobs beyond `api_precheck`. If one does, it belongs in billing core.

## 4. P9 migration and cutover

- **Carry:**
  - `api_keys`: all 46 rows with their ids, owners, names, bcrypt hashes, dates, active flag, scopes, metadata and expiration policy
  - `callback_deliveries`: 49 rows, as history with the `delivered` or `failed` state; their attempt rows are synthesized from status code, error and `delivered_at`
  - `jobs.callback_url`: already part of the jobs migration
- **Decision (DP-6):** `api_key_audit_logs`, 872 rows including IPs. Default is to carry them as usage history only if privacy approves.
- **Not carried:** `api_key_rate_limits` (ephemeral) and the webhook notification queue (thrown).
- **Dry-run parity:** key counts by (active, expired, scope set), counts of keys whose scopes would newly deny an operation under API-D01, and delivery counts.
- **Cutover:**
  - Keys authenticate on the new host from the first request, through the legacy bcrypt path.
  - Jobs in flight at cutover complete on whichever system holds them. Only completions on the new system create deliveries. Old failed callbacks are not retried, because the source never retried.
  - The two active owners are told about any v1-beta change before DNS moves (DP-1).
- P9 also owns the edge trusted-proxy count for `api_key_uses.ip`, and live TLS for callbacks.

## 5. Ownership and order

| Order | Owner | Slice |
|---|---|---|
| 1 | p1X | `core/src/api`: Actor, Scope, `can`, key and callback domain and ports, `callback_payload`, use cases, key-screen presentation state for P5 (create dialog, copy once, list, revoke confirm, expiry choices). Domain tests first |
| 1 | R (p1R) | `0005_api.sql`, PG adapters for the ports, aligned-window limiter, legacy bcrypt upgrade, erasure and retention hooks. Real-PG tests |
| 2 | p1T | Public, legacy and alias routes, key session routes, bearer-key extractor, rate headers, worker wiring (relay fan-out, dispatcher), reqwest transport with pinning |
| 2 | p1W | `transgate_api::public` DTOs and `OPERATIONS`, OpenAPI generator and check, parser/schema agreement test, docs sample validator, artifact handoff to P6 |
| 3 | p22 | `transgate-client::public_api`: typed Rust client over the same DTOs (used by CLI/TUI in P8), and the key-admin calls used by the P5 screen |
| 3 | P5 view owners | `/dashboard/api-keys` desktop and phone views over p1X's presenter. `route.rs` gains the app route (p1X) |
| 4 | Y | Acceptance (§7) |

No two owners edit one file. Root Cargo and lock changes (`bcrypt`, `schemars`) go through the integration owner. Implementers do not run checks mid-flight.

## 6. Decisions: parity vs observable changes

| ID | Question | Recommendation | Observable change |
|---|---|---|---|
| DP-1 | v1-beta and unified compatibility | Keep a thin v1-beta shim with the source deprecation and sunset headers (API-013) plus the unified 301. Remove the shim only after the two active owners confirm | none if kept |
| API-D01 | Scope enforcement | Enforce | A read-only key can no longer create. The migration report counts affected keys |
| API-D02 | Revocation by non-owner | Owner only | none for legitimate use |
| API-D03 | Limiter arithmetic | Atomic, independent windows | Hourly counts no longer double |
| API-D04 | Over-limit status | 429 | 401 becomes 429 |
| DP-4 | Limiter outage | 503 (fail closed) | none in practice (same database) |
| API-D06 | Callback destination safety | Pin the validated address, no redirects, DNS failure rejects | Callbacks to private or unresolvable hosts stop |
| API-D07 / DP-2 | Retries, delivery id, signature | Adopt all three | Receivers can see duplicates (dedupe by delivery id) and extra headers. Signing secret UI is flag-off until mu approves showing it |
| DP-3 | `Idempotency-Key` | Optional header | Only when the header is sent |
| DP-5 | Summary wait bound | 15 min, then send without | Comparable to the source's synchronous wait |
| DP-6 | Retention of payloads, usage IPs, audit migration | Payload cleared with the history rule; IP kept 30 days; audit carried if privacy approves | Privacy |
| DP-7 | Generated spec replaces the hand-written YAML | Adopt | Docs text changes to the true contract |
| DP-8 | `/api-reference` Try-it | Static render (a) | Try-it removed |
| — | Key prefix hint, rotation UI | Hint stored, shown behind a flag; no rotation feature | none by default |

## 7. Acceptance (independent test agent Y, after lead authorization)

1. **Real Postgres** (`TRANSGATE_JOBS_TEST_DATABASE_URL`):
   - Key create, list and revoke, with owner isolation and another account refused.
   - Digest lookup.
   - Legacy bcrypt rows verify, upgrade on first use, and no longer scan afterward.
   - Expiry is strict at `now`.
   - Inactive keys are refused.
   - The aligned limiter under two connections: exactly 60 per minute and 1000 per hour, both windows consumed atomically, and the hour boundary crossed correctly.
   - Fan-out is idempotent under a relay retry.
   - Two dispatcher processes never deliver one attempt twice.
   - Erasure and retention clear every API row and payload.
2. **Actual process** (transgate-server plus PG plus GoTrue plus loopback provider fixtures plus a **loopback callback receiver**):
   - A key submits `audioUrls` and gets 201 with `client_type: "api"`.
   - The job completes through the fixture provider.
   - Billing debit and deferred duration match P3 rules.
   - The receiver gets exactly one 2xx-acknowledged POST with the API-015 body (lines with `corrected_text`, summary after the P4 outcome, an audio URL), `User-Agent`, delivery id and a valid signature.
   - The receiver then answers 500, 500 and 200, and the attempts follow the schedule under a test clock.
   - A receiver that hangs times out at 10 s.
   - Redirect and private-IP targets are refused.
   - Admin replay appends an attempt to the same delivery.
   - Bearer session JWT on public routes is 401. Missing scope is 403. Over limit is 429 with headers. Legacy v1-beta round-trip, if kept. Unified 301.
   - A same-key `Idempotency-Key` retry does not charge twice.
3. **OpenAPI:**
   - The `--check` artifact matches a fresh generation.
   - The document validates with a pinned OpenAPI 3.1 validator.
   - The router's mounted public routes equal `OPERATIONS`.
   - The parser/schema agreement table passes.
   - `/api-docs` samples validate against the spec.
4. **Generated client:** a client generated from `openapi.json` by a pinned generator with 3.1 support (Y and p22 pin it; no guess here) compiles and runs scenario 2's submit and status against the live process. The `transgate-client` Rust client passes the same scenario.
5. **Browser:** `/dashboard/api-keys` on desktop and phone: create with each scope and expiry, copy once, list, revoke with confirm. This closes API-F08/F09 jointly with P5.
6. **Ledger:** API-001 to 019, RTL-001 to 003 and IDN-028 each name their Rust or process test. API-F11 and API-018 record the dropped disposition.

**Live and deployment gates stay open (P9):** real customer receivers, TLS, DNS, trusted proxy, and notification of the two active API owners.
