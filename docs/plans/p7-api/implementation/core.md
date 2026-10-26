# P7 core API handoff (p1X)

Source: `transgate/core/src/api/` plus small changes to `jobs/{app,ports,testing}.rs` and `ai/ports.rs`. Decisions: `docs/plans/p7-api/decisions.md`. No HTTP, SQL or OpenAPI here; p1T owns the migration, PG adapters, routes, worker and transport.

## Contract

| Module | Contents |
|---|---|
| `api/mod.rs` | `KeyId`; `ApiError` with `http_status`/`code`/`message`: 401 `unauthorized`, 403 `forbidden`, 404, 400 `invalid_request`, 429 `rate_limited` (carries `ApiRate`), 409 `request_conflict`, 410 `account_deletion_in_progress`, 503 `unavailable`, or the wrapped `JobsError` |
| `auth.rs` | `Scope` (source strings `transcriptions:read`/`create`), `Actor::{Session, ApiKey}`, `Action`, `Resource`, pure `can` |
| `keys.rs` | `ApiKey`, `KeyView` (never digest/hash/plaintext), `NewKey`, `CreatedKey`, `ExpirationPolicy` (source values), `well_formed`, `hint`, `expires_at`, `check` |
| `limits.rs` | `RateWindows::at` (calendar minute/hour, UTC), `RateLimits` (60/1000), `RateCounts`, `ApiRate` (RTL-003 header values), `decide`, reference `consume` |
| `idempotency.rs` | `IdempotencyClaim`, `IdempotencyRecord`, `lookup`, 24 h `RETENTION` |
| `callbacks.rs` | `CallbackDelivery`, `DeliveryState`, `SendOutcome`, `classify`, `settle`, `summary_gate`, `payload`, `with_audio_url`, `signed_message`, `signature`, header/UA/schedule constants |
| `ports.rs` | `ApiTx` (on `JobUow::api()`), `KeyUse`, `ApiCrypto`, `ApiRateLimiter`, `CallbackOutbox` (token-fenced), `CallbackAttempt`, `CallbackRequest`, `CallbackTransport`, `CallbackContent` |
| `app.rs` | `Api::{authenticate, authorize, start, record_use, create_key, list_keys, revoke_key}`, `Admitted<T>`; `Callbacks::{fan_out, dispatch_pass}`, `CallbackPass`; `JobsCallbackContent` over the job store, `Jobs` and `Ai` |

Jobs: `Jobs::start_public(account, PublicStart::{Jobs, Urls}, Option<IdempotencyClaim>) -> PublicAnswer { answer, replayed }`. `start` and `submit_urls` are now thin callers of it (no behavior change). `JobsError::RequestConflict` (409). `Jobs::callback_audio_url` signs a stored object for the callback body (no staging).

## Semantics chosen in core

- **Order (D1):** `authenticate`, then `authorize` = `can`, then one limiter unit. A refused scope is 403 and not counted. Sessions never pass `authorize`; keys never manage keys.
- **Authentication (API-001..003, D2):** a malformed bearer is 401 without any store read. Digest lookup first. On a miss, legacy candidates (active, unexpired, no digest yet) are verified outside the transaction, then the match is upgraded under its row lock (digest + hint written, bcrypt hash nulled); a concurrent upgrade rereads by digest. `expires_at < now` is expired (equal still verifies, as `remote_schema.sql:1245`; the Opus proposal's "strictly after" was not the source). Success updates `last_used_at`; an erasure barrier is 410.
- **Limiter (D3):** store error 503; denial 429 with `ApiRate`. Remaining is `limit − count`, never negative.
- **Idempotency (D4):** the key is 1..=255 characters (else 400). The request digest is the host SHA-256 of the canonical JSON of the parsed `PublicStart`. Lookup happens after the billing account lock in the start transaction; the record (job ids) is inserted in that same transaction; the first answer is saved after dispatch (best effort). A replay returns the stored answer, or rebuilds it from the same jobs' current status if saving the answer had failed. Records expire after 24 h; an expired record is replaced.
- **Keys:** create needs the owner's session and takes the erasure barrier and billing account lock; the plaintext `sk_` + `random_token()` is returned once. Revoking another account's key is 404 (no enumeration) and changes nothing.
- **Callbacks (D5..D7, D10):** fan-out from the relay's transaction, idempotent on `event_id`. Per claim: summary gate (wait, rechecking every 30 s, at most 15 min after completion; any decided outcome sends). Then the content is read (job gone: `Abandoned`, nothing sent). The signing secret is obtained before the attempt, so a missing secret never burns an attempt. Then `begin_attempt` (durable attempt row and stored body without `audio_url`), the signed POST, and `finish_attempt`. The schedule delays are counted from the previous attempt (+0, 1 m, 5 m, 30 m, 2 h, 12 h; the sixth retryable failure abandons). 2xx delivers; network, timeout, 408, 429 and 5xx retry; any other answer and a refused destination are final. The body is rebuilt from current content on every attempt, so clearing stored payloads is always safe.
- **D10 purges:** `JobUow::delete_lines_of` also nulls the payloads of those jobs' deliveries; `AiTx::purge_ai_artifacts` nulls every payload of the account; `erase_account_rows` deletes all API rows. Mirrored in the memory fake; p1T implemented and tested them in PG.

## Not in this slice

- D8 admin replay: no use case yet. The decision allows appending an attempt to the same delivery id; it needs an outbox `reopen` and the admin route.
- `GET /api/v1/transcriptions` shape and the v1-beta shim (p1T). Their authorization uses `Api::authorize(actor, TranscriptionRead, Resource::Job{..})` after loading the owned job.
- P5 key-screen presenter (WEB-045/046) is not built here.

## Evidence (executed by p1X)

- Rule tests, red first (stub bodies panicked), then green: `api::{auth,keys,limits,idempotency,callbacks}::tests` (16), `api::app::tests` (12): API-001/002/003/005/019, API-D01/D02/D03/D04, RTL-001/003, D1..D7, D10, D11.
- `cargo test --locked -p transgate-core --features ts --lib`: 432 passed.
- p1T reports `jobs-postgres` real-PG API tests 13/13, including the D10 purges, against these ports.
- Not claimed: HTTP routes, the real transport and SSRF pinning, the worker loop, OpenAPI, and any process or browser run.
