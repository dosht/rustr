# P2-P4 whole-clause reconciliation (CLAUSE-RECON)

Author: p21, for lead w23:p2B, 2026-10-27. This is a read-only analysis: no source, ledger, status or test changes, and no checks were run. It only classifies existing evidence. The lead owns every canonical `rules.md` status edit.

## Method and scope

- **Open clauses in scope**:
  - P2: the six rows the billing gate still rejects (`docs/plans/p2-billing/integration-notes.md:80`).
  - P3: every row marked Mixed or Missing in the P3 rule map (`docs/plans/p3-transcription/opus/product-rule-evidence.md`), except the P2 rows above. Processor rows are excluded because WORKING-ON says they are reconciled.
  - P4: AI-001..026, the 23 rows still `ledger` in `transgate/docs/rules.md`, plus the AI clauses inside P3 rows (PRV-038..044, LNG-030, JOB-033, TRX-039).
  - FLG rows are not in any of these open lists and are not classified here.
- **Classes**:
  - **Proven**: every clause of the row has executed evidence (test name or candidate receipt).
  - **Later**: the remaining clause is owned by P6/P7/P8/P9, with a reference.
  - **Missing**: a clause in this phase's scope has no executed evidence. The row names the test that would close it.

  Grouped IDs count as one entry.
- **Evidence keys**:
  - `C187`, `PG18`, `CF5`, `B244`, `T27`, `S7`, `MT` and `AX21` are the P3 suite runs defined in the P3 rule map. Its `J:`/`Q:`/`TV:`/`SX:` etc. prefixes are the test selectors listed there.
  - `P3smoke`: the actual Transgate process smoke, PASS 1207.62 s on binary `85fea3ad`. Log: `/tmp/rustr-p3-auth-sol/transgate-resume-timed/full-run.log`.
  - `c5`/`c7`/`c8`/`gap`: the P5 Playwright receipts (Desktop Chrome and Pixel 7) in `docs/plans/p5-web/implementation/parity-disposition.md`. The gap run is `s1-20261026215323/logs/next-p5-gap/actions.json`; candidate8 is `s1-20261027073103/logs/next-p5-c/`.
  - `A`/`S`/`P`/`Q`/`C`, `H-auth`, `H-resume`, `W-summary` and `E4`: the P4 selectors in `docs/plans/p4-ai/implementation/content.md:113-119`.
  - `P4-GAPS`: the tests in `docs/plans/p4-ai/implementation/policy.md:171-180`. `realPG207` is the P4 real-Postgres run.

## Counts

| Phase | Entries | Proven | Later | Missing |
|---|---|---|---|---|
| P2 | 6 | 5 | 0 | 1 |
| P3 | 90 | 31 | 14 | 45 |
| P4 | 23 | 23 | 0 | 0 |

P4: five of the 23 proven rows are proven under an approved correction. They need ledger status `changed`, not `tested`: AI-004 (D1/D8), AI-006 and AI-021 (D1), AI-019 (D2) and AI-023 (D3). AI-003 also needs the ledger wording correction in `content.md:150-156` before it can be marked.

## P2 billing (six rows)

| Row | Class | Evidence | To close / owner |
|---|---|---|---|
| BIL-068 | Proven | `billing-http:bil_068_every_protected_route_refuses_missing_and_invalid_sessions_and_checkout_is_post_only`; `d17_the_checkout_document_post_starts_once_and_carries_its_own_policy` (a cold `GET /checkout` gets 303 to `/`); `d17_a_checkout_document_post_refuses_cross_site_mixed_credentials_and_malformed_forms`; browser POST checkout WEB-BIL-009/010 (c7, gap) | The redirect target is `/` (P5 D17). The locale-specific public home behind `/` is P6. |
| BIL-072 | Missing | Server events: `notify::tests::bil_072_*` and `transgate-billing:bil_072_purchase_and_signup_events_reach_posthog_once…`. Success/failure pages with the 5 s countdown: WEB-BIL-011..014 (gap). | (1) A C2 Playwright assertion on both projects that, after cookie Accept, Pay emits client `checkout_started` with attribution and Cancel emits `subscription_cancelled` to loopback PostHog. Neither event appears in the gap or c8 receipts; core only asserts the event names (`purchase_tests.rs:200`, `subscription_tests.rs:95`). (2) Google Analytics `purchase`: the app loads no GA tracker (PRV-049, PRV-D13). This needs a lead decision: drop it, or assign site-wide trackers to P6. |
| BIL-073 | Proven | `billing-core:presentation::tests::bil_073_balance_text_is_minutes_under_an_hour_then_hours_and_minutes`; `billing-http:bil_008_073_…_no_balance_value`; WEB-BIL-001 gap: `balance refreshes within 30 s without reload` | |
| BIL-075 | Proven | `bil_075_history_rows_carry_…`, `billing-postgres:bil_075_076_…`, `transgate-billing:bil_066_075_076_…`; browser history and parsed PDF in WEB-BIL-015/016 (c5, gap) | Invoice-preview pixels are LIVE-P9; they are not part of the rule. |
| FRE-022 | Proven | `P3smoke`: "FRE-022: freemium first-job bypass drove an account to -10 min; its complete all-formats download is served without a debit while a new start is refused 402". `J:export_use_case`; `TV:freemium_cut` (download stays enabled in the view model); browser export WEB-025/026 | |
| FRE-024 | Proven | `fre_024_unparseable_or_missing_chat_caps_mean_no_limit`; `P::paid_callers_bypass_the_quota_lane`; `Q::admission_keeps_source_precedence_and_maxima`; `Q::settlement_charges_the_combined_estimate`; P4-GAPS `paid_callers_bypass_a_failing_quota_store_but_a_failed_plan_lookup_is_freemium`; real-PG `jobs-postgres/tests/ai.rs` (realPG207); browser limits in WEB-029/030 | Production cap values are deployment configuration (P9). |

## P3 transcription and privacy

| Rows | Class | Evidence | To close / owner |
|---|---|---|---|
| JOB-001 | Later | JD transition tests (C187) | Importing deprecated production statuses: P9 |
| JOB-002, JOB-005 | Proven | `J:start_charges_and_hands_over`, `Q:file_flow_to_ready` (C187); browser upload/language/Start WEB-007/008 (c7) | |
| JOB-003 | Missing | `Q:start_needs_a_language_choice`; draft restore with language reset WEB-009/010 (gap) | A core presentation test that restores a draft whose upload failed and aggregates the all-speakers choice across restored drafts |
| JOB-006, LNG-029 | Missing | `J:negative_balance_refuses_all_drafts`, `BC:fre_018_…` (C187/B244) | A PgJobStore consumer test, beside `consumer_failures`: one Start batch mixing an exempt-language freemium draft and a normal draft at zero or negative balance, asserting which start and the single charge |
| JOB-008 | Later | `BC:bil_025_026_079_…`, `BL:bil_025_026_027_032_…` | Imported ready/uploaded jobs: P9 migration |
| JOB-009, JOB-010, JOB-043 | Later | `J:deferred_charge_from_the_result`, API parsers (AX21), MS/MF signing and fetch (S7/MT) | API-key URL entry, callback_url fallback: P7 |
| JOB-018 | Missing | Capacity locks (PG18/F8PG) | A worker-pass test asserting queue order, at most five per pass, 1 s spacing and stop on the first 429 |
| JOB-020, JOB-021 | Missing | `JD:stuck_deepgram_resets_to_pending`, PG F4 | A test where the same job goes stuck repeatedly and is reset every pass without a limit and without a second charge |
| JOB-022..024 | Missing | JD/RT/MP recovery units (C187/MT) | A Transgate running-job Soniox recovery-poll integration covering the exhausted, missing-id, working and error branches |
| JOB-033 | Later | Summary proven by AI-024/025 (below) | Customer callback delivery: P7 |
| JOB-035 | Missing | `JD:youtube_completes_from_draft`; resolve in a real browser WEB-011/012 (c8, C3) | Assert the balance is unchanged across a YouTube resolve (C2 balance read before/after, or a core test), the "zero billing" clause |
| JOB-036 | Proven | JD/J soft delete, PG F4; Delete All WEB-PRV-003/004, list delete WEB-015/016 (gap, c8) | |
| JOB-039 | Proven | 20-row window, `unlimited_history` and `?before=` paging WEB-PRV-013/014, WEB-015 (gap, c8) | |
| UPL-001 | Proven | `Q:file_flow_to_ready`; browser measure and upload WEB-007/008 | |
| UPL-002 | Missing | Browser pick-file (c7) | A Vitest test per platform on the upload views asserting the picker `accept` MIME list and `multiple` |
| UPL-003, UPL-004 | Missing | `Q:drop_filters_and_cap` | A Playwright drop test (desktop): m4a accepted, rejected MIME ignored, drops after ten ignored, button visibility |
| UPL-005 | Missing | No positive test | A consumer test that a large, long upload is accepted with no product size/duration refusal |
| UPL-006 | Missing | `Q:file_flow_to_ready` (61 s to 2 min); the 5-minute unmeasurable alert (gap) | A Vitest or Playwright test of the decode fallback, the 5 s/10 s timeouts and the 100 MB decode boundary |
| UPL-007 | Proven | Q readiness tests; Start enabled after language WEB-007/008 | |
| UPL-009 | Missing | AX21 parser | An HTTP route test of init-upload: 401, 400, 201, 500 |
| UPL-012 | Proven | `J:the_payload_archive_uses_the_archive_buckets_adapter`; P3smoke exact archive intent in the archive bucket | Deployed bucket authority: P9 |
| UPL-013, UPL-014 | Missing | `Q:file_flow_to_ready`; signed PUT (c7) | A test of a failed PUT: the draft is kept, marked failed, no automatic retry; progress shown and only HTTP 200 counts as success |
| UPL-015 | Missing | `JR:an_expired_upload_of_a_removed_draft_is_deleted_and_swept_again` | A presentation test that an expired removed draft leaves the visible queue |
| UPL-016 | Proven | `JR:no_capability_is_returned_across_the_erasure_barrier`; P3smoke owner GET and foreign denial; playback WEB-017/018 | Deployed signing: P9 |
| UPL-017 | Proven | `Q:start_needs_a_language_choice`; picker in a real browser WEB-007/008 | |
| UPL-018, UPL-019 | Proven | `Q:translation_languages`; EN to TR job WEB-013/014 (gap) | |
| UPL-020, LNG-024 | Missing | `Q:file_flow_to_ready` | A browser toggle of the speaker option applied to all queued drafts and persisted, on both projects. WEB-007 notes the toggle was not exercised. |
| UPL-021 | Missing | API ordering test (AX21) | A real-PG test of language-usage aggregation and ordering for one account |
| UPL-023 | Missing | `J:youtube_add_creates_the_draft`; blocked add 403 WEB-PRV-015/016 | HTTP tests for add: 10/min limit, 401, 404 and 502 |
| UPL-025 | Proven | `Q:youtube_queue`; WEB-011/012 | |
| UPL-026 | Missing | Resolve in a real browser WEB-011/012 (c8) | A test of the 5/min resolve limit |
| UPL-027 | Missing | MC public/paid wire tests | A matrix test of paid caption failure codes against the displayed suggestion message |
| UPL-028 | Missing | `Q:youtube_queue` | A browser test of a mixed file plus YouTube queue where a failed YouTube draft is removed and files proceed |
| UPL-031, TRX-004, TRX-022, LNG-025, LNG-027, LNG-028 | Later | Parsers and routing (AX21/C187) | API-key options and legacy public contract: P7. Imported echoed options: P9. |
| UPL-033 | Proven | P3smoke by-id owner read and foreign denial; deep link `/home/N` (c7 `phone login deep link`, WEB-IDN-004) | |
| TRX-014 | Missing | `J:rate_limit_fails_over`, `uncertain_submission_does_not_fail_over` | A process-level hard-error failover-chain matrix: definite refusal fails over, an uncertain response does not |
| TRX-023, LNG-013 | Missing | Code-map tests (C187/MT) | A test that an invalid stored translation target or code mapping fails the job |
| TRX-024 | Missing | SX segmentation tests | Exact-boundary unit tests at 300 ms and 2000 ms gaps and for each punctuation class |
| TRX-025 | Missing | `SX:short_lines_merge`, `leading_punctuation_only_line_is_dropped` | Tests of the language/status merge guards, neighbour-edge repair and zero-length remainder |
| TRX-028, TRL-004 | Missing | `SX:translation_pairs_share_windows` | A test of same-start window subdivision with the default 2 s |
| TRX-033, TRX-034 | Proven | J/DU/MP callback and duration tests (C187/MT); CF5 Soniox deferred-billing failure paths; P3smoke | |
| TRX-038 | Missing | MC cue timing; resolve in a real browser (c8) | A core test that every cue field (start, end, text, speaker) lands on the saved line through `youtube_resolve` |
| TRX-039 | Later | Owner status and lines (P3smoke); summary AI-024 | Public status API: P7 |
| TRX-041, PRV-035, PRV-037, LNG-017, LNG-022 | Later | MP/MS synthetic provider tests | Live provider retention, GCS lifecycle, live language/model acceptance: P9 |
| TRX-042 | Proven | `TV:translation_page`; original gated by flag WEB-013/014, WEB-021/022 | |
| TRL-003 | Proven | `Q:translation_languages`; picker in a real browser WEB-013/014 | |
| EDT-001 | Proven | `TV:edit_flow`; edit save and Escape cancel WEB-019/020 (gap) | |
| EDT-005, EDT-006 | Missing | `PG:d7_line_edits_…`; rename persisted WEB-019/020 | A real-PG test of two concurrent multi-line speaker renames on one job ending atomic, with no mixed rows |
| EDT-008, EDT-010 | Missing | `TV:speaker_display`, `translation_page` | Matrix unit tests: speaker_labeling placeholder, YouTube job, no speakers |
| EDT-012, EDT-013, LNG-007 | Missing | `TX:language_tabs_order`, `L:rtl_detection_…`; tabs WEB-013/014; RTL fixtures (C1) | A test that text direction switches correctly when a streaming job completes |
| EDT-014 | Proven | `TV:time_click_seeks`; timestamp seek in a real browser WEB-017 (gap) | |
| EXP-007, EXP-009, EXP-010 | Missing | EX format tests; all-formats ZIP parsed WEB-025 (gap) | A unit test of translated headers and CSV fields containing newlines for each extension |
| EXP-008 | Missing | `EX:long_lines_split_for_subtitles` | Unit tests at exactly the 6 s boundary and of `-->` escaping |
| EXP-015 | Missing | Phone ZIP download without Web Share (c5 WEB-026) | A phone test exporting a transcript with empty lines through the share-unavailable fallback |
| EXP-016 | Missing | `EX:download_event_properties` | A C2 assertion that `transcript_downloaded` reaches loopback PostHog after consent, with word count, media and flags; absent from the gap/c8 receipts |
| LNG-001 | Proven | `L:ui_locales_match_the_fixture`; 16 locales by path on both projects LOC-001..016 (c7) | |
| LNG-002..006 | Missing | Locale path prefix and `lang` persisted WEB-039/040, LOC rows | A browser test of first-visit locale detection from cookie and Accept-Language, plus the fallback-translation case. Public static pages: P6. |
| LNG-008 | Proven | Routing and roster tests; language picker WEB-007/008 | |
| LNG-011, LNG-012 | Proven | `Q:translation_languages`; WEB-013/014 | |
| LNG-030 | Proven | `S::chat_language_instructions_keep_source_exact_code_lookup_and_raw_unknown_fallback`; E4 `chat-ar-…`, `chat-tr-…`, `chat-fr-…` | Live answer quality: P9 |
| PRV-001 | Proven | P3smoke own erasure and stale 410; erasure c7, C3 packaged 202 | TLS/JWKS: P9 |
| PRV-002, PRV-003 | Proven | DM tests; Delete All WEB-PRV-003/004 | |
| PRV-005 | Later | Jobs/billing cascade (PG18/B244); AI purge `P::refusal_keeps_artifacts_and_delete_purges_older_epochs` | API keys and audit: P7. Historical archives: P9. |
| PRV-008, PRV-009 | Later | | API audit anonymization: P7. Historical data: P9. |
| PRV-010 | Missing | DM test; other tab signs out WEB-PRV-005/006, WEB-043/044 | A C2 assertion that the analytics identity is reset (PostHog reset) after deletion and sign-out |
| PRV-011, PRV-012 | Proven | JD/J tests; list and Delete All WEB-015/016, WEB-PRV-003/004 | |
| PRV-014, PRV-015 | Missing | PA, PG 31-day and taskr retention tests | A Transgate retention sweep over mixed statuses and jobs with AI artifacts, asserting retained statuses, metadata and AI rows survive |
| PRV-017 | Missing | | A real-PG fixture with `unlimited_history = true` whose old lines are still swept |
| PRV-019 | Missing | `PA:history_retention_skips_and_aborts_like_the_source` | A real-PG all-or-nothing sweep at 100000 versus 100001 rows |
| PRV-021 | Missing | PA/PG audit-row tests | Assert every audit field (failure, error, elapsed, executor), or record a lead decision retiring the monitoring views |
| PRV-023 | Missing | `RE:plan_counts_only_opted_in_lines` | A use-case `retention_plan` test: same predicate as the sweep, zero audit or deletion writes |
| PRV-026 | Missing | PG D7 cascade | A real-PG test of the orphan and RESTRICT foreign-key behaviour. Imported orphans: P9. |
| PRV-027 | Missing | Confirm and persist WEB-PRV-001/002 | A test that a failed PATCH of the cleanup switch reverts and shows an error |
| PRV-028, PRV-053..060 | Later | | Legal and marketing copy and promises: P6. Deployed retention and IP/backup claims: P9. |
| PRV-029, CRN-005, CRN-007 | Missing | `SC:retention_slot_is_two_am_utc`; PA/PG sweeps | A process test with a controlled clock: the daily 02:00 UTC sweep runs and resumes after a restart. P3smoke states no day-retention windows. |
| PRV-032 | Proven | TM arithmetic; history window and paging WEB-PRV-013/014 | |
| PRV-033 | Later | PG authority test; list behaviour WEB-PRV-013/014 | Imported values: P9 |
| PRV-034 | Missing | | A test that the list refetches when `unlimited_history` changes without a reload (D11 polling) |
| PRV-036 | Proven | Legacy switch persisted WEB-PRV-007/008 | Deployment data: P9 |
| PRV-038..043 | Proven | `C::*`, `P::refusal_during_streaming_fences_the_answer`, `P::refusal_keeps_artifacts_and_delete_purges_older_epochs`, `P::a_failed_purge_stays_pending_and_covered_rows_stay_hidden`, H-auth (P4 D3); grant and DELETE WEB-PRV-009/010 | |
| PRV-044 | Proven | AI Features tab WEB-041/042, WEB-PRV-009/010 | |
| PRV-046..048 | Proven | Cookie group WEB-PRV-011/012 (gap): no telemetry before a choice, Reject keeps none, Accept loads once | |
| PRV-049 | Later | | GA, Hotjar and New Relic are absent from the app. Gate or drop is PRV-D13; site trackers: P6 |
| PRV-050 | Proven | `transgate-billing:bil_072_purchase_and_signup_events_reach_posthog_once…` | Deployed sinks: P9 |
| PRV-051 | Missing | Blocked add 403 WEB-PRV-015/016 | A test that a blocked account's resolve is denied too |
| BIL-038..041 | Later | `BC:bil_039_040_…`, Q readiness | API-key zero boundary and legacy public errors: P7 |
| BIL-042 | Proven | `BL:bil_042_*` (B244); JR remove-draft (C187) | Manual refund terms are content: P6 |
| FRE-020, FRE-021, FRE-023 | Proven | `BC:fre_020_…`, `fre_021_…`, `fre_023_…`; `TV:freemium_cut`; 22-minute cut and banner WEB-023/024, WEB-BIL-026/027 (gap) | |
| EML-004, EML-005 | Missing | EV and PG18 tests; P3smoke support-mail retry | A process test delivering the partial-error email, and a durable per-target sent mark surviving a resend |
| API-001..019, API-F01..F05/F10, RTL-001..003 | Later | | P7 public API |

## P4 AI

Every row has core, real-PG (realPG207) or process evidence, and browser evidence for the WEB-027..030 flows. The runtime selectors H-auth, H-resume and W-summary come from `content.md:117-119`. Remaining live or quality claims are P9 notes, not open P4 clauses.

| Row | Class | Evidence |
|---|---|---|
| AI-001 | Proven | H-auth `ai_http_auth_consent_and_owned_job_refusals_happen_before_any_provider_dispatch`; `P::analysis_streams_typed_sections_in_source_event_order`; WEB-027/028 |
| AI-003 | Proven* | `A::sections_are_available_before_provider_finish_…`, `A::only_a_single_restricted_count_header_is_yaml_…`, `A::incomplete_bad_schema_…`; E4 D8 parser cases. *Apply the ledger wording correction first (`content.md:150-156`). |
| AI-004 | Proven (changed D1/D8) | `S::canonical_analysis_is_raw_…`, `S::analysis_substitutes_transcript_literally_…`, `S::analysis_estimate_uses_…`; E4 `meeting-deepseek-coalesced-D1-omitted-mode` |
| AI-005 | Proven | E4 `meeting-deepseek-coalesced` exact request; `*-openai-explicit-alternate`. Live model availability: P9. |
| AI-006 | Proven (changed D1) | H-resume `ai_http_disconnect_cross_replica_resnapshot_and_saved_final_never_redispatch`; WEB-027/028 interrupted to Retry |
| AI-007 | Proven | `presentation::ai::analysis::tests::cached_completed_analysis_succeeds_and_legacy_empty_rows_are_a_miss`, `a_cache_read_failure_allows_generation_through_the_server`; WEB-027 |
| AI-008 | Proven | `presentation::ai::analysis::tests::a_running_cached_analysis_is_watched_with_ai008_staleness_from_creation`; `ai::presentation::staleness` |
| AI-009 | Proven | `P::a_request_key_replays_and_never_redispatches`; analysis `RetryRequested` leaf tests; Retry in WEB-027/028 |
| AI-010 | Proven | `S::raw_route_rejections_precede_sanitation_…`; `transgate-api::ai::tests::malformed_identity_or_history_cannot_be_coerced_…`; H-auth |
| AI-011 | Proven | H-auth; `P::refusal_during_streaming_fences_the_answer`; `C::refusal_fences_writers_but_regrant_restores_reads` |
| AI-013 | Proven | `S::chat_language_instructions_…`; E4 chat language cases; `transgate-api::ai::tests::chat_forwards_complete_ordered_input_…`. Live answer quality: P9. |
| AI-014 | Proven | `P::paid_callers_bypass_the_quota_lane`; P4-GAPS `paid_callers_bypass_a_failing_quota_store_but_a_failed_plan_lookup_is_freemium` plus p1T's real-PG twin |
| AI-015 | Proven | `Q::admission_keeps_source_precedence_and_maxima`; `P::freemium_caps_and_the_single_metered_lane_refuse_before_dispatch`; real-PG `jobs-postgres/tests/ai.rs` |
| AI-016 | Proven | `Q::settlement_charges_the_combined_estimate`; `P::a_freemium_answer_saves_the_pair_and_settles_once` |
| AI-017 | Proven | `Q::configured_caps_follow_the_source_parser`, `Q::periods_reset_on_utc_day_iso_week_and_calendar_month` |
| AI-018 | Proven | `Q::late_settlement_never_touches_a_later_period`, `P::an_answer_settled_after_midnight_lands_in_its_admission_day`; real-PG rollover `jobs-postgres/tests/ai.rs:730` |
| AI-019 | Proven (changed D2, fail-closed) | P4-GAPS `a_failing_quota_store_refuses_freemium_admission_before_anything_is_dispatched`, `a_failing_quota_store_at_settlement_keeps_the_final_for_one_local_completion`; real-PG unknown-COMMIT `jobs-postgres/tests/ai.rs:974` |
| AI-020 | Proven | `S::cumulative_sanitation_handles_html_email_and_spacing_…`; `P::a_quiet_provider_still_checkpoints_the_partial_before_the_final`; H-resume; streaming WEB-029/030 |
| AI-021 | Proven (changed D1) | `P::a_freemium_answer_saves_the_pair_and_settles_once`; `presentation::ai::chat::tests::an_interrupted_stream_is_read_back_before_deciding_failure_or_unsaved_answer` (unsaved answer stays visible with the sync warning) |
| AI-022 | Proven | `C::no_row_is_no_consent_and_delete_still_covers_legacy` (version 1.0, provider deepseek); `P::consent_and_limit_metadata_need_no_prior_consent`; grant/DELETE WEB-PRV-009/010 |
| AI-023 | Proven (changed D3) | `P::refusal_keeps_artifacts_and_delete_purges_older_epochs`; `P::a_failed_purge_stays_pending_and_covered_rows_stay_hidden`; `C::delete_cuts_off_every_older_epoch_for_good` |
| AI-024 | Proven | `S::canonical_analysis_is_raw_while_summary_and_chat_drop_blank_lines_and_sanitize`; E4 `summary-{concise,detailed}-…`; `P::the_completion_summary_releases_callback_ordering_with_an_explicit_outcome`. Outbound delivery: P7. |
| AI-025 | Proven | W-summary `a_held_then_failed_ai_summary_cannot_hold_partial_error_mail_or_fail_completed_transcription`; `P::the_completion_summary_releases_callback_ordering_…` |

## Notes for the lead

- The first pass of this analysis made no canonical edits. The canonical edits that followed are recorded in the next section.
- BIL-072: lead decision 2026-10-27. The GA `purchase` event is dropped (P6 D14, GA omitted). The PostHog `checkout_started` and `subscription_cancelled` events stay, and Y verifies them in a browser.
- P3's 45 missing entries are listed by owner below.

## Canonical ledger edits (2026-10-27, p21)

Edited in the topic files `transgate/docs/discovery/{ai,billing,jobs-transcription,languages,privacy-identity}.md`, then regenerated `transgate/docs/rules.md` with `scripts/assemble-discovery.py`. `--check` passes (526 rules, 143 pgTAP assertions, 0 unmapped).

Static checks: `check-prd.py billing` reports one unmet row (BIL-072), and every AI rule is accepted. Every new cargo reference was resolved against `cargo test -- --list --include-ignored` output, so the names exist. The named tests were not executed by `check-prd.py`; that run needs the PG test databases, which the integration owner holds.

| Phase | Rows edited | Result |
|---|---|---|
| P4 | AI-001, 003, 005, 007, 008, 009, 010, 011, 013..018, 020, 022, 024, 025 → `tested`; AI-004, 006, 019, 021, 023 → `changed` (D1/D8, D1, D2, D1, D3) | AI-003 already carried the corrected wording. AI: 21 tested + 5 changed + AI-027 excluded. P4 rule rows are complete. |
| P2 | BIL-068, 073, 075, FRE-022, 024 → `tested`, adding the cargo tests named above | Billing 122/123; BIL-072 stays `ledger`. The browser and process receipts are cited in the billing topic prose. |
| P3 | JOB-002, 005, 036, 039; UPL-001, 007, 012, 016, 017, 018, 019, 025, 033; TRX-033, 034, 042; TRL-003; EDT-001, 014; LNG-001, 008, 011, 012, 030; PRV-001, 002, 011, 012, 032, 038, 039, 041, 042, 044, 050 → `tested`; PRV-040 → `changed` (P4 D3) | BIL-042 and FRE-020/021/023 were already `tested`. The browser receipts covering browser clauses are cited in each topic's prose. |

Six proven P3 rows stay `ledger`, because the ledger accepts only cargo references and these have none:

| Rows | Executed evidence | What closes the ledger row | Owner |
|---|---|---|---|
| PRV-003 | Account deletion in a real browser (WEB-PRV-005/006) | **Done:** `changed` by lead decision 2026-10-27. There is no browser soft-delete pre-step; server-side deletion is authoritative. Cargo evidence cites the erasure barrier, manifest and cascade tests. | — |
| PRV-036 | Legacy "Delete my data automatically" switch persisted (WEB-PRV-007/008) | A `presentation::data_management` test: confirming `DeleteMonthly` emits `SetDeletedMonthly`, and a reload reflects the stored value | p1X |
| PRV-043 | Consent GET/POST/DELETE in a real browser (WEB-PRV-009/010) | An HTTP route test: a non-boolean `hasConsented` gets 400, no session gets 401 | p1T |
| PRV-046..048 | Committed spec `transgate/web/e2e/cookie-consent.spec.ts`: 5 tests × desktop/mobile, 10/10 passed in each of Y's 3 runs on a candidate8 copy | **Done:** PRV-046 `tested`; PRV-047/048 `changed` under P5 D12 (no SDK before consent; GTM/gtag and `opt_out_capturing_by_default` not carried over). `check-prd.py` resolves the `playwright:` references to that spec; regression `test_a_browser_only_rule_resolves_its_playwright_spec_and_still_needs_a_passing_run`. | — |

## P3 missing entries by owner

Each test is the one named in the P3 table above. Y runs browser assertions; the owner writes the test.

| Owner | Entries | Closing test |
|---|---|---|
| p1X core | JOB-003 | A presentation test restoring a draft whose upload failed, and aggregating the all-speakers choice across restored drafts |
| p1X | JOB-035 | A core test that a YouTube resolve leaves the balance unchanged |
| p1X | UPL-015 | A presentation test that an expired removed draft leaves the visible queue |
| p1X | UPL-020, LNG-024 | A presentation test that the speaker toggle applies to every queued draft and persists. Y adds the browser toggle on both projects. |
| p1X | UPL-027 | A matrix test of paid caption failure codes against the displayed suggestion message |
| p1X | UPL-028 | A queue test: a failed YouTube draft is removed while the files in the same queue proceed |
| p1X | TRX-023, LNG-013 | An invalid stored translation target or code mapping fails the job |
| p1X | TRX-024 | Segmentation at exactly 300 ms and 2000 ms gaps, one case per punctuation class |
| p1X | TRX-025 | Language/status merge guards, neighbour-edge repair, zero-length remainder |
| p1X | TRX-028, TRL-004 | Same-start window subdivision with the default 2 s |
| p1X | TRX-038 | Every cue field (start, end, text, speaker) lands on the saved line through `youtube_resolve` |
| p1X | EDT-008, EDT-010 | Speaker display matrix: speaker_labeling placeholder, YouTube job, no speakers |
| p1X | EDT-012, EDT-013, LNG-007 | Text direction switches when a streaming job completes |
| p1X | LNG-002..006 | Route tests: first-visit locale from the cookie, then Accept-Language, then default; fallback translation; `lang`/`dir` in the view model. Y adds the browser check. Public static pages: P6. |
| p1X | PRV-023 | A use-case `retention_plan` test: same predicate as the sweep, zero audit and zero deletion writes |
| p1X | PRV-027 | A presentation test that a failed cleanup-switch PATCH reverts the switch and shows an error |
| p1X | PRV-034 | The job list refetches when `unlimited_history` changes, without a reload (D11 polling) |
| p1T host/http/server/processor | JOB-006, LNG-029 | A PgJobStore consumer test: one Start batch mixing an exempt-language freemium draft and a normal draft at zero or negative balance; assert which start and the single charge |
| p1T | JOB-018 | A worker pass: queue order, at most five per pass, 1 s spacing, stop on the first 429 |
| p1T | JOB-020, JOB-021 | The same job stuck repeatedly is reset on every pass with no limit and no second charge |
| p1T | JOB-022..024 | Running-job Soniox recovery poll: exhausted, missing-id, working and error branches |
| p1T | UPL-005 | A large, long upload is accepted with no product size or duration refusal |
| p1T | UPL-009 | Init-upload route: 401, 400, 201, 500 |
| p1T | UPL-021 | Real-PG language-usage aggregation and ordering for one account |
| p1T | UPL-023 | YouTube add route: the 10/min limit, 401, 404, 502 |
| p1T | UPL-026 | YouTube resolve: the 5/min limit |
| p1T | TRX-014 | A process-level failover matrix: definite refusal fails over, an uncertain response does not |
| p1T | EDT-005, EDT-006 | Real-PG: two concurrent multi-line speaker renames on one job end atomic, with no mixed rows |
| p1T | PRV-014, PRV-015 | A retention sweep over mixed statuses and jobs with AI artifacts; retained statuses, metadata and AI rows survive |
| p1T | PRV-017 | Real-PG: with `unlimited_history = true`, old lines are still swept |
| p1T | PRV-019 | Real-PG all-or-nothing sweep at 100000 versus 100001 rows |
| p1T | PRV-021 | Every audit field asserted (failure, error, elapsed, executor), or a lead decision retiring the monitoring views |
| p1T | PRV-026 | Real-PG orphan and RESTRICT foreign-key behaviour |
| p1T | PRV-029, CRN-005, CRN-007 | A process test with a controlled clock: the daily 02:00 UTC sweep runs and resumes after a restart |
| p1T | PRV-051 | A blocked account's YouTube resolve is denied (add is already proven) |
| p1T | EML-004, EML-005 | Process delivery of the partial-error email, and a durable per-target sent mark that survives a resend |
| p22 client | UPL-006 | Duration measurement: the decode fallback, the 5 s and 10 s timeouts, the 100 MB decode boundary |
| p22 | UPL-013, UPL-014 | A failed PUT keeps the draft marked failed with no automatic retry; progress shown; only HTTP 200 counts as success |
| p22 | EXP-015 | Phone export of a transcript with empty lines through the share-unavailable fallback |
| p22 | EXP-016 | `transcript_downloaded` with word count, media and flags reaches PostHog after consent. Y asserts delivery in C2. |
| p22 | PRV-010 | Sign-out and deletion reset the analytics identity (PostHog reset). Y asserts it in C2. |
| p1W documents | EXP-007, EXP-009, EXP-010 | Translated headers and CSV fields containing newlines for each export extension |
| p1W | EXP-008 | Subtitle split at exactly the 6 s boundary, and `-->` escaping |
| p1Z views | UPL-002 | A Vitest test per platform: the picker `accept` MIME list and `multiple` |
| p1Z | UPL-003, UPL-004 | Drop-zone view test: m4a accepted, rejected MIME ignored, drops after ten ignored, button visibility. Y adds the Playwright drop. |

Totals: p1X 17, p1T 19, p22 5, p1W 2, p1Z 2, p23 0 (no P3 content rows are open). 45 in all.
