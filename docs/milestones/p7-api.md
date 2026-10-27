# P7: public API

Status: **gate met in rows and evidence**, 2026-10-27. All 37 P7-owned rows are tested (14), changed by decision (18) or dropped (5); none missing ([disposition](../plans/p7-api/parity-disposition.md)). The API-BIL-012 product fix and its test are committed with the P3-CLOSE batch and re-verified on candidate9.

## What was built

API keys (`sk_` format, SHA-256 digests, legacy bcrypt rows upgraded on use), scopes checked by a pure `can(actor, action, resource)` in core, calendar-aligned Postgres rate limits that fail closed, idempotent public starts (`Idempotency-Key`, 409 on conflict), completed-job callbacks signed with HMAC-SHA256 with an attempt record before each send, retries and admin replay, and OpenAPI 3.1 generated from the same `OPERATIONS` table that mounts the router. Key management screens on desktop and phone. No compatibility shim: there are no API users.

Decisions: [P7 decisions](../plans/p7-api/decisions.md), after independent Opus and Fable proposals and cross-critiques.

## Exercised evidence (Y, frozen `a353d1e`)

| Check | Observed result |
|---|---|
| Real Postgres | `jobs-postgres` api 13/13, billing runtime `api::` 4/4, server callback worker 2/2, crypto 4/4 (bcrypt verifies pgcrypto `$2a$`) |
| Actual process | Keys per scope, 201 with request and rate headers, idempotent replay and 409, 403 by scope, 401 unknown key, 60/min then 429 with `Retry-After`, limiter outage 503, signed callback verified with the owner's secret, retry schedule, loopback callback URL refused, admin replay and its refusals |
| OpenAPI | Both URLs byte-identical to the committed spec; valid OpenAPI 3.1; every mounted public route documented; keys refused on undocumented routes |
| Browser | Key screens on Desktop Chrome and Pixel 7, all seven states; committed spec `transgate/web/e2e/api-keys.spec.ts` 12/12 in three runs |
| Zero balance | `api_bil_012_a_key_with_zero_balance_gets_402_and_nothing_is_created` (red, then fixed) |

## Limits

Callbacks were verified against a loopback receiver only; no live customer endpoint. Rate limits and replay are local-process evidence; production behavior behind a proxy is P9.
