# P7 cross-critique of the Opus proposal (Fable)

`w23:p27`, anthropic/claude-fable-5-1, MEDIUM, 2026-10-26. Read `opus/proposal.md` (3AFC) after my own was frozen. Source checks cited inline; neither proposal is edited. Verdict up front: the two proposals agree on the architecture (core `api` module, `can`, Postgres limiter, outbox-backed deliveries, committed spec, no new module). Opus is more source-accurate in five places and I adopt them; Opus is wrong or unsupported in four places.

## 1. Where Opus is wrong or unsupported

| # | Claim | Evidence | Disposition |
|---|---|---|---|
| O1 | Reuse `transgate_jobs.rate_windows` with a new "calendar-aligned mode" (§2.3) | `migrations/0001_jobs.sql:205-208`: PK is `key_hash` alone, one window per key, first-hit semantics in `rate.rs:36-43`. Two independent windows per key need a `(key, kind, window_start)` key; that is a new table or a breaking PK change to a P3 immutable migration | New table `transgate_api.rate_windows` (my §2); keep `PgRateLimiter` untouched for RTL-004/005 |
| O2 | Scope strings cited at `dashboard/api-keys.tsx:37,242-244` | The scope option literals are at `api-keys.tsx:50-69` (`transcriptions:read`, `transcriptions:create`, comma-joined pair). Lines 37/242 were not checked by me; the values are correct, the citation is loose | Cite 50-69 |
| O3 | `api_key_uses` one row per authenticated request with IP, UA, duration (§2.2, §2.4) | API-003 says verification "logs use"; API-005 shows the list returns no usage; nothing in source UI or API reads the audit rows (`api.md:31`, endpoint column empty per `production-facts.md:9`). A per-request insert on the hot path is unconsumed data plus a privacy retention question (DP-6) | Over-built. `last_used_at` only; record API-003's logging clause as replaced. If the lead wants parity, one row without IP/UA |
| O4 | Same-key different-body returns **422** (§2.7) | P4's accepted precedent for `Idempotency-Key` disagreement is **400** (`hosts.md:47`: "disagreement with body requestId is 400"); my proposal said 409. Neither is source; consistency with P4 matters more | 400 for malformed/disagreeing key use, consistent with P4; not 422/409 |

Minor: Opus §2.6 cites `webhook-handler.ts:33-45` under `services/`; the file is `services/unified-transcription/utils/webhook-handler.ts:41-45`. The claim itself (completed only) is confirmed.

## 2. What Opus gets right that I missed, and adopt

| # | Opus point | Why it is right | Adopt |
|---|---|---|---|
| A1 | Fan-out inside the existing relay, `callback_deliveries.event_id UNIQUE`, `INSERT … ON CONFLICT DO NOTHING` (§2.6) | My design had a second loop calling `claim_events`; `store.rs:48-51` leases each event once, so a second consumer would steal events from the mail relay (`workers.rs:165-168`). Fan-out in the one relay is the only correct shape | Yes |
| A2 | Keep `sk_` + base64 format for new keys (§2.2) | API-002 parity; my `tg_<prefix>_` format changed the customer-visible shape for no functional gain, since SHA-256 lookup works on the whole token either way | Yes; store an 8-char hint, show behind a flag as Opus says |
| A3 | Replay is admin-only (§2.6) | Customer self-service replay and owner delivery listing are new features; PRD §2 forbids new capabilities beyond the named ones | Yes; drop my `GET …/callbacks` and owner replay |
| A4 | Bounded summary wait, 15 min then send without (DP-5) | My unbounded wait would hold a callback forever on a stuck summary; source sent without summary on failure | Yes |
| A5 | Failed jobs get no callback, cited and confirmed (`webhook-handler.ts:41-45`) | Resolves my [unverified] P7-D8 | Yes |
| A6 | Parser/schema agreement test (§2.8) | True whichever generator is used: the hand parsers over `Value` (`api/src/lib.rs:1-8`) are the enforcement, a schema is only a description | Yes, regardless of DP below |
| A7 | `options.webhookUrl` validate-and-ignore (`route.ts:699-717`) | Confirmed; I had not addressed it | Yes |
| A8 | Carry `expiration_policy`, `metadata`, unknown scope strings | Migration fidelity | Yes |

What Opus missed from mine: limiter order (authenticate → `can` → consume; a malformed body or refused scope should not spend quota) and the P4-consistent 400; both small.

## 3. Open decisions, one-line dispositions

| Decision | Disposition | Deciding evidence |
|---|---|---|
| Shim for old callers | Keep the thin v1-beta shim with API-013 headers until the two owners confirm (Opus DP-1), **not** my no-shim default | `production-facts.md:9-11`: version used unknown, two owners break without it; PRD §2 allows a conscious shim |
| Callback signing | Per-account HMAC-SHA256 `t=,v1=`, header always sent, secret UI flag-off (both agree) | API-D06 correction; receivers ignore unknown headers |
| Retry schedule | +1 m, 5 m, 30 m, 2 h, 12 h, 6 attempts, retry only on network/timeout/408/429/5xx, other 4xx final (Opus refinement) | API-D07; 4xx finality avoids hammering a rejecting receiver |
| Idempotency-Key | Optional header, per account, 24 h, same body replays stored 201, disagreement 400 | P4 precedent `hosts.md:47`; source has none so absence is unchanged |
| Summary wait | Postpone while `summary_outcome` is `None`, bound 15 min, then send without | P4 D5 ordering seam; source sent without summary on failure |
| Failed-job callbacks | None | `webhook-handler.ts:41-45` |
| Retention | Clear `callback_deliveries.payload` with the history rule; no per-request usage rows; audit log not migrated (export kept read-only) | O3 above; 30-day history rule vs API-017 forever is a contradiction the privacy owner must record |
| Spec generation | `OPERATIONS` table + `schemars` + router-built-from-table + agreement test (Opus), not utoipa | utoipa path macros bind docs to Axum handler signatures and still would not describe the hand parsers; the table makes "no undocumented route" a compile-time property |
| Rate-limit windows | Two independent windows, calendar-aligned (source `date_trunc`), new `transgate_api.rate_windows`, one statement per window consumed only when both allow, denied not counted | O1; API-D03 |
| Fail-closed | 503 on limiter store error (both agree) | same database as keys and jobs; fail-open cannot serve anyway |
| Legacy bcrypt rehash | Digest miss → bcrypt scan of active legacy rows → write digest, null bcrypt (both agree); pure-Rust `bcrypt` crate pinned by lead | `remote_schema.sql:1217-1227` scans every row today; bounded by 39 rows and shrinking |
| Crate choice | No new crate group; `transgate/core/src/api`, `jobs-postgres` migration, `transgate-http` routes, worker in `Workers::spawn` (both agree) | PRD §5 names only `billing` and `cms` as modules |
| Docs rendering | Static render from the committed spec, no Try-it, curl per operation (Opus DP-8a = my §10); keep `/openapi/openapi.yaml` URL | `package.json:121` swagger-ui-react conflicts with no-React; P6 static design; `api-reference.tsx:26,39` references the YAML URL |
| Key format | Keep `sk_` (A2) | API-002 |
| Replay | Admin-only (A3) | PRD §2 no new features |

Net: adopt the Opus proposal as the base with O1, O3, O4 corrected and my limiter ordering added. No remaining architectural disagreement.
