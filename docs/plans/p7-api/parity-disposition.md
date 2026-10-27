# P7 parity disposition

Owner: p21, for lead w23:p2B, 2026-10-27. Row-by-row disposition of every API-*, API-F*, API-IDN-*, API-BIL-* and WEB-045/046 row. P7 ownership comes from the Opus proposal §1 (accepted as the base by `decisions.md`) and the lead decisions D1–D13 with their 2026-10-27 revisions. The lead decides acceptance.

## Evidence

| Tag | Source |
|---|---|
| y7 | Y's P7 verification on the frozen candidate `s1-20261027094048` (`git archive a353d1e`, manifest `98f08b5c…`), `logs/next-p7-v1/report.json`. Real-PG suites all exit 0: jobs-postgres `api`, billing runtime `api::`, callbacks filter, server `workers::pg_tests::callbacks`, crypto bcrypt/pgcrypto. Actual-process receipts `api-{start,callback,retry,replay,ratelimit,limiter-down}.json`. `openapi-report.json`: served YAML equals JSON equals the committed file, `openapi-spec-validator` 3.1 valid. Browser `actions.json` and `keys-*.png` on Desktop Chrome and Pixel 7. Two browser selector timeouts were retried and passed. The `export_tests` failure is REPAIR-P5-13 (lopdf pin), outside P7. |
| cargo | The cargo tests named in each `rules.md`/`parity.md` row. Every name was resolved against `cargo test -- --list --include-ignored`. |

Rules:
- **TESTED:** every clause exercised; cites cargo tests and/or a y7 receipt.
- **CHANGED:** the source behaviour was replaced under a named decision; the new behaviour is tested.
- **DROPPED:** not served, by decision.
- **OTHER:** owned by another phase; P7 does not reopen it.
- **MISSING:** a P7 clause without evidence; the closing test is named.

## Counts

| Disposition | P7 rules | P7 parity | Total |
|---|---|---|---|
| TESTED | 7 | 6 | 13 |
| CHANGED | 16 | 2 | 18 |
| DROPPED | 0 | 5 | 5 |
| MISSING | 0 | 1 | 1 |
| **P7-owned** | **23** | **14** | **37** |
| OTHER (not P7) | 13 rules + RTL-004..008 | 48 parity rows | — |

Canonical ledger: all 23 P7 rules and 13 of the 14 P7 parity rows are applied.
- F01/F02 `tested`, F07/F10 `changed`.
- F03/F04/F05 `dropped` through the mu-approved keep/throw row.
- F06/F11 `dropped` through keep/throw rows reading "mu approved: decisions delegated to the lead on 2026-10-27", with the decision reference (lead instruction).
- F08/F09 and WEB-045/046 `tested`, citing all six tests of Y's committed `transgate/web/e2e/api-keys.spec.ts`. Each test passes on both projects; Y's runs were 12/12, three times on `9d76d0d` and once on `5ad7082` with the retitle. F08/F09 are the full create/copy/list/revoke flow per platform, so they cite all six tests and not Y's suggested split.

The only open P7 row is API-BIL-012 (MISSING).

## Rules (P7-owned)

| Row | Disposition | Evidence | Decision / note |
|---|---|---|---|
| IDN-028 | CHANGED | `api::auth::tests::api_001_public_routes_are_key_only_and_owner_scoped`, `api_001_002_a_malformed_bearer_is_refused_without_a_lookup`; y7 `openapi-report` key probes: session routes answer 401 to a key | P5 D5, P7 D1: public routes are key-only, no cookie path |
| API-001 | CHANGED | as IDN-028, plus `transgate-billing:api::an_owner_manages_keys_and_a_key_reads_only_its_own_accounts_transcription`; y7 `unknown_key` 401 | Invalid keys never fall back (kept); no cookie auth on public routes (D1) |
| API-002 | CHANGED | `api::keys::tests::api_002_only_the_source_key_shape_is_well_formed`, `crypto::tests::{a_new_token_is_a_fresh_api_002_key_body,digests_match_their_published_vectors,bcrypt_verifies_hashes_generated_by_pgcrypto}`, `d2_a_legacy_bcrypt_key_upgrades_on_first_use`, PG `keys_round_trip_and_the_legacy_scan_set_…`; y7 keys `sk_…` shown once | D2: SHA-256 digest; legacy bcrypt verified then upgraded |
| API-003 | CHANGED | `api_003_digest_keys_verify_touch_and_refuse_inactive_or_expired`, `keys::tests::api_003_inactive_and_strictly_past_expiry_refuse` | D11 reversed: `last_used_at` only, no per-request usage rows |
| API-004 | CHANGED | `api_d01_public_operations_require_their_scope`, `d1_d3_scope_before_the_limiter_then_429_and_503`; y7 create-only key reading status gets 403 | API-D01: the enforcement gap is closed |
| API-005 | TESTED | `api_005_019_d02_keys_are_managed_by_their_owners_session_only`, `public::tests::keys_never_expose_secrets_and_name_their_fields_in_camel_case`, billing runtime owner test; y7 list after reload | |
| API-006 | CHANGED | `api_d02_only_the_owners_session_manages_keys`, billing runtime (a stranger's DELETE gets 404); y7 revoke with confirm | API-D02: revoke is owner-checked |
| API-007 | TESTED | `transgate-api:tests::a_job_ids_request_needs_positive_integers_and_never_both_modes`, `api_001_public_routes_are_key_only_and_owner_scoped`; y7 `audioUrls` start 201 | |
| API-008 | TESTED | `tests::an_audio_urls_request_is_checked_item_by_item` | |
| API-009 | CHANGED | `an_audio_urls_request_is_checked_item_by_item`, `media-adapters:callback::tests::private_loopback_link_local_and_metadata_destinations_are_refused_before_anything_is_sent`, `server:callbacks::tests::without_the_development_switch_a_loopback_receiver_is_refused_…`; y7 `literal_loopback_callback_intake` 400 | D5: SSRF pinned at send time |
| API-010 | TESTED | `public::tests::the_public_start_body_is_the_session_start_body_with_the_api_client_type`, `tests::the_start_answer_has_the_wrapped_shape_the_clients_read`; y7 `api-start` 201 with `success/data/metadata`, request id, `clientType: api` | |
| API-011..014 | CHANGED | `openapi::tests::the_document_is_openapi_3_1_and_lists_exactly_the_operation_table`, `transgate-billing:api::the_document_is_served_from_the_same_table_that_mounts_the_routes`; y7 documented operations (no v1-beta or unified paths) | D12 revised (mu, 2026-10-27): no shim, no 301 |
| API-015 | TESTED | `callbacks::tests::api_015_payload_matches_the_source_shape`, `server:callbacks::tests::the_d5_headers_and_the_signed_body_reach_the_receiver_unchanged`; y7 payload keys `job,result,errors[,summary]`, `audio_url` signed at send and absent from the stored payload (D10) | |
| API-016 | CHANGED | `d5_schedule_has_six_attempts_then_abandons`, `d5_retryable_and_final_outcomes`, `d5_signature_covers_timestamp_and_body`, `d5_retries_follow_the_schedule_and_4xx_is_final`, server PG callbacks dispatcher; y7 HMAC `t=,v1=` valid, retries at +0/+61 s | D5: signed, durable, 6-attempt schedule |
| API-017 | CHANGED | `d5_the_attempt_is_durable_before_the_signed_post`, PG `the_outbox_leases_fences_and_records_an_attempt_before_the_post`, server PG "no delivery for a failed job, final refusal not retried"; y7 stored delivery and attempts | D5: delivery and attempt rows |
| API-018 | CHANGED | operation-table tests (as API-011) | Accepted proposal §1: `/api/webhooks` placeholder not served (keep/throw "throw") |
| API-019 | TESTED | `keys::tests::api_019_expiry_policies_follow_the_source_function`, `public::tests::a_new_key_needs_a_name_known_scopes_and_a_known_policy`; y7 create Read & Create / Read Only / Create Only, revoke with confirm (desktop dialog, phone sheet) | |
| RTL-001 | CHANGED | `limits::tests::rtl_001_windows_are_calendar_aligned_minute_and_hour`, `api_d03_both_windows_or_neither_and_denials_do_not_count`, PG `the_limiter_consumes_both_windows_or_neither_…`, `concurrent_requests_never_overshoot_a_limit`; y7 61st request gets 429, another key unaffected | D3: same 60/1000, calendar-aligned, both or neither |
| RTL-002 | CHANGED | `d1_d3_scope_before_the_limiter_then_429_and_503`, billing runtime `the_limit_refuses_with_retry_after_…`; y7 `api-limiter-down` 503, no job created | D3: fail closed (503), over limit is 429, not 401 |
| RTL-003 | TESTED | `rtl_003_remaining_and_resets_for_the_headers`, `public::tests::rate_headers_carry_the_windows_and_retry_after_only_on_a_refusal`, `openapi::tests::every_key_route_documents_the_rate_limit_headers_…`; y7 headers on 201 and 429 | |

## Parity (P7-owned)

| Row | Disposition | Evidence | Decision / note |
|---|---|---|---|
| API-F01 `POST /api/v1/transcriptions` | TESTED | billing runtime owner test, `d4_an_idempotent_start_charges_once_and_conflicts_on_another_body`, PG `an_idempotency_key_is_live_for_a_day_…`; y7 201, same-key replay 201 with `idempotency-replayed`, different body 409 | D4 adds Idempotency-Key |
| API-F02 `GET /api/v1/transcriptions?jobId` | TESTED | billing runtime owner test, `api_001_public_routes_are_key_only_and_owner_scoped`; y7 `status_read` 200 with lines | |
| API-F03 v1-beta URL | DROPPED | `keep-throw:Legacy v1-beta and unified transcription shapes` | mu, D12 revised |
| API-F04 v1-beta status | DROPPED | same | mu, D12 revised |
| API-F05 unified redirect | DROPPED | same | mu, D12 revised |
| API-F06 browser `/api/transcripts` | DROPPED | `keep-throw:Browser legacy \`/api/transcripts\` submission`; replaced by the P5 D5 session route `/api/jobs/start`, proven in P5 WEB-007/008 | Lead D13, mu-delegated |
| API-F07 `GET /api/keys/list` | CHANGED | billing runtime owner test, `api_005_019_d02_…`; y7 list | Now `GET /api/keys` from the OpenAPI operation table (D9) |
| API-F08 key screen, desktop | TESTED | `playwright:e2e/api-keys.spec.ts#api keys …`, all six tests (menu/URL entry; empty + signing secret; each scope shown once, copies, hidden after reload; revoke with confirm; loading; failed load); y7 receipts | |
| API-F09 key screen, phone | TESTED | the same six tests on the `mobile` project (reach by URL, revoke via sheet); y7 Pixel 7 receipts | |
| API-F10 customer callback delivery | CHANGED | server PG dispatcher test, `d5_d6_fan_out_is_idempotent_per_completed_event`, `an_admin_replay_appends_an_attempt_…`, PG `a_relay_retry_inserts_no_second_delivery_…`; y7 callback (3 attempts, delivered), retry, admin replay with the same delivery id and valid HMAC | D5/D6/D7/D8 |
| API-F11 `/api/webhooks` | DROPPED | `keep-throw:Generic key webhook placeholder handlers`; not in the operation table (operation-table tests) | Accepted proposal §1, mu-delegated |
| API-BIL-012 public 402 and balance metadata | MISSING | y7 start answer carries `newBalance` and `isFreemium`; the core rule is BIL-039/040 (`bil_039_040_web_refuses_below_zero_and_api_at_zero_or_less`) | Closing test (p1T): a billing-runtime HTTP test where a key with balance 0 starts `audioUrls` and gets 402 `insufficient_balance`, with no job and no charge |
| WEB-045 key screen, desktop | TESTED | as API-F08 | D13: closes jointly with P5 views |
| WEB-046 key screen, phone | TESTED | as API-F09 | Same |

## Other phases (not reopened by P7)

| Rows | Owner phase | Current state |
|---|---|---|
| API-020, API-021, API-F17 (public captions); RTL-004, RTL-005 (YouTube limits) | P3 | ledger/inventoried; open P3 entries in `docs/plans/clause-reconciliation.md` (UPL-023/026) |
| API-022..030, RTL-006..008, API-F45..F84 (free-tools processor) | P3 | tested/changed |
| API-F44 voice demo | P4 D6 | dropped |
| API-F12..F16 (init-upload, streaming-url, YouTube add/metadata/resolve) | P3 (session routes, P5 D5) | browser-proven in P5 (WEB-007/011/017); canonical rows inventoried |
| API-F18..F21, API-BIL-011, API-IDN-002 (AI SSE, consent, chat limits) | P4 | AI rules all reconciled; parity rows inventoried, browser-proven in P5 WEB-027..030, WEB-PRV-009/010 |
| API-F22..F30 (provider callbacks, internal hooks, cron, queue) | P3 | process-proven in the P3 smoke; inventoried |
| API-F25, API-IDN-003 (user-inserted hook), API-F26, API-BIL-006 (payment-updated, replaced by outbox) | P2/P3 | inventoried |
| API-F31..F35 (monitoring, health, memory, test-ffmpeg) | P9 operations (keep/throw "simplify") | inventoried |
| API-F36, API-F37 (contact-us, subscribe) | P6 | inventoried (subscribe "throw") |
| API-F38 (support), API-F42, API-IDN-001 (delete account), API-F43 (download) | P5/P3 | browser-proven in P5 (WEB-033/034, WEB-PRV-005/006, WEB-025/026) |
| API-F39..F41, API-BIL-001..010 (checkout, confirm, cancel, subscription callback, admin invoices, browser reads) | P2/P5 | billing rules 122/123; browser-proven in P5 WEB-BIL rows |
