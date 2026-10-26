# P7 lead decisions

Lead technical decisions after the independent Opus (p10) and Fable (p27) proposals and their cross-critiques (`{opus,fable}/{proposal,critique}.md`). Codex/Sol was unavailable, so Fable took the second advisor seat; there is no separate Fable review this time. These are design dispositions, not execution evidence. The implementation batch is dispatched separately. Items marked **mu** wait for mu.

Base: the Opus proposal, with the corrections below. Both advisors agree there is no remaining architectural disagreement except D6 and D11, decided here.

## Accepted contract

| # | Decision | Evidence and conditions |
|---|---|---|
| D1 | No new crate group. Core `transgate/core/src/api` (actor, scopes, `can(actor, action, resource)`, limits, callbacks, idempotency) with an `ApiTx` on the existing `JobUow`; a `jobs-postgres` migration in a new `transgate_api` schema; routes in `transgate-http`; the delivery worker inside `Workers::spawn`. | PRD section 5 names only `billing` and `cms` as modules. Order is authenticate, then `can`, then consume the limiter. Key actors never manage keys. |
| D2 | Keep the `sk_` key format. Store a SHA-256 digest plus an 8-character hint (hint display behind a flag). Legacy bcrypt rows: on a digest miss, bcrypt-scan only active, unexpired, not-yet-upgraded rows; on success write the digest and null the bcrypt hash. Pure-Rust `bcrypt` crate, version pinned by the implementer after a test proves it verifies pgcrypto `$2a$` hashes. | Source scans every row (`remote_schema.sql:1217-1227`); 39 active keys today. API-002. |
| D3 | Rate limits: calendar-aligned minute and hour windows (60 and 1000 per key) in a new `transgate_api.rate_windows` keyed `(key_id, kind, window_start)`; both windows are consumed in one statement only when both allow; denied requests are not counted. Over limit is 429 with the source header names; a limiter store error is 503 (fail closed). | The existing `transgate_jobs.rate_windows` cannot be reused (`0001_jobs.sql:205-208` key-hash primary key, first-hit semantics in `rate.rs:36-43`). Source `date_trunc` windows (`remote_schema.sql:306-307`), API-D03/D04, RTL-002, header names `authentication.ts:274-284`. |
| D4 | Optional `Idempotency-Key` header on public starts, per account, 24 hours, written in the start transaction. Same body returns the stored 201; a different body returns **409 `request_conflict`**. | Matches the P4 request-key precedent exactly (`ai/mod.rs:110,128`: reused id with a different request is 409). Fable's 400 citation (`hosts.md:47`) does not describe idempotency. The source has none, so a retried POST charges twice today (`jobs/app.rs:717-770`). |
| D5 | Callbacks: fan-out inside the existing outbox relay, idempotent on `event_id` (UNIQUE), not a second `claim_events` consumer. An attempt row is written before each send; the delivery id is a header and the receiver's dedupe key. Per-account HMAC-SHA256 signature header (`t=,v1=`) always sent; the secret-viewing UI ships behind a flag that stays off until mu enables it. 6 attempts at +0, 1 m, 5 m, 30 m, 2 h, 12 h; retry on network error, timeout, 408, 429 and 5xx; other 4xx are final; then abandoned. User-Agent stays `Transgate-Callback/1.0`; payload keys match the source (`summary{text}`). SSRF checks are pinned at send time. | A second relay loop would steal events from the mail relay. API-D06 (no signature today; receivers ignore extra headers), API-D07 (one attempt today). |
| D6 | Callbacks only for **completed** jobs, as the source. | `webhook-handler.ts:41-45,64-67`; provider call sites pass completed only (`deepgram.ts:600`, `soniox.ts:537`). |
| D7 | Wait for the summary while `summary_outcome` is `None`, bounded at 15 minutes, then send without it. | Source sends without the summary on failure (`webhook-handler.ts:76-95`); `summary_outcome` is the P4 boundary (`ai/app.rs:837-846`). |
| D8 | Replay is admin-only and appends an attempt to the same delivery id. Customer list/replay/secret endpoints are new features and are not added. | PRD section 2 (no new features); IDN-032 admin allow-list. |
| D9 | OpenAPI 3.1 from a `transgate_api` `OPERATIONS` table that also mounts the router, DTOs with `schemars`, plus a parser/schema agreement test. Not `utoipa` on core types. The spec is committed, served at `/openapi/openapi.yaml` (unchanged URL) and `/api/v1/openapi.json`; P6 renders static docs from it with a pinned hash, and the packaged check compares the hash. No Swagger "Try it"; one curl example per operation. | The hand parser is the real contract (`api/src/lib.rs:1-8`); table mounting makes an undocumented route impossible. Source spec is 3.1.0. `swagger-ui-react` would bring React. |
| D10 | Stored callback payloads exclude `audio_url` (re-signed at send), are cleared by the history rule, join the P4 consent purge and are deleted on erasure. | P4 ports note `ai/ports.rs:392-399`. |
| D11 | **Usage audit rows: keep one row per request (source parity) for now.** Dropping or trimming them, migrating the 872 existing rows and IP retention are privacy decisions for mu. | The table is labelled as a HIPAA audit (`remote_schema.sql:1271`). Fable's `last_used_at`-only alternative is recorded as the proposed simplification. |
| D12 | Keep the thin v1-beta compatibility shim with the source sunset headers (API-013) and the unified 301. Retire it only after both live API owners confirm. | `production-facts.md:9-11`: two live owners, version unknown. Both advisors converged here. |
| D13 | Parity rows WEB-045/046 (key screens) close jointly with P5 views. API-F06 gets a `dropped` record (replaced by P5 D5 `/api/jobs/start`). No new billing metering: usage visibility is `last_used_at`, audit rows and job history. DEF-017 (job completes despite a failed deferred charge) is ported as is and stays mu's open decision. | |

## Waiting for mu

- **Shim retirement (D12):** contact the two live API owners, or confirm we keep the shim indefinitely.
- **Usage audit (D11):** keep per-request rows (default), or reduce to `last_used_at`; migrate the 872 rows or not; IP retention period.
- **Signing secret UI (D5):** when to turn the flag on.
- **DEF-017:** unchanged, still open.

## Next

Implementation batch for existing owners: p1X core `api` module; p1T migration, routes, worker and OpenAPI; p22 client against the generated contract; p1V key screens (WEB-045/046). Y verifies with real-Postgres tests, actual-process scenarios with loopback callback receivers, OpenAPI validation and a compiled client.
