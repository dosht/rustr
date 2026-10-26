# P4 content handoff

Owner: w23:p23. Status: **allocation-free finish migration source-ready, unexecuted**. Content owner ran no builds, tests, lint, formatters or provider calls and made no source-repository writes. Historical lead evidence predates this API migration; no new pass claim. Lead owns coherent execution and acceptance.

## Owned source

- `transgate/core/src/ai/analysis.rs`: public result/progress/errors, bounds, test declarations.
- `analysis/types.rs`: all six exact source shapes and nested types; optional absent fields remain absent, explicit null rejects, action status defaults to pending, ratings permit fractional numbers in [1,5], enums retain source casing. Unknown object properties are ignored as source Zod objects do.
- `analysis/parser.rs`: one unfinished document and bounded pre-type sections, incremental quote/escape/depth tracking; no per-token transcript copies/reparse. D8 recognizes `---` anywhere outside strings/containers, including surrounding whitespace, adjacent next document and split markers. Only a first, single-line `documentCount: N` header accepts restricted YAML. Header optional; total unknown without it. `max(N-2,0)` advisory content total; mismatch warns, never fabricates optional sections. Finite numeric confidence has no added range gate; missing/non-number confidence becomes zero. Pre-type specialized values are deferred under the same raw aggregate/document/depth bounds, then applied in arrival order when type is known; only validated typed sections enter public progress. Source JS `data || items || content` precedence/truthiness is retained (empty arrays/objects are truthy), with D8's explicitly approved interruption for all-falsy, schema-invalid and inapplicable sections. Typed identical duplicates are ignored after defaulting; conflicting/invalid/incomplete content interrupts with valid partials retained. Every type requires the common usable summary; generic mirrors it in its source optional summary field.
- `prompts.rs`: literal unified system/user template and exact generated six-schema documentation, general concise/detailed summary builder, chat system prompt and complete source locale map. Analysis uses raw canonical speaker/line text, all lines, newline separated. D8 explicitly approves literal transcript insertion (no JS replacement-directive expansion) and the actual-transcript UTF-16 input estimate helper. Chat/general summary use ContentProcessor's nonblank trimmed lines and space joining plus input sanitation. Only chat uses language instructions; unknown exact locale codes remain raw source fallback.
- `sanitize.rs`: distinct source input/output filters, ECMAScript whitespace/ASCII word/case semantics, script-block versus tag-only differences, SQL/XSS and card/SSN/email/phone/key patterns, UTF-16 limits, raw route and optionally enabled question validation. Not a zero-PII or remote-retention guarantee; rendering stays plain text. Prose checkpoints/finals must sanitize cumulative raw text, never individual deltas.
- `analysis/tests.rs`, `analysis/sanitize_tests.rs`: 29 domain/regression test functions. Existing six-shape/schema/sanitation/D8 cases remain. Finish migration adds retained valid final-section/error/warning-once behavior and successful move/terminal-reuse rejection; older partial-error cases now consume retained progress explicitly.
- `analysis/fixtures/{meeting,lecture,sales,support,interview,generic}.txt`: six complete synthetic UTF-8 streams, common summary plus every applicable section and nested field. Quoted `---` and café exercise framing/Unicode. No real transcript/customer content.
- `analysis/fixtures/d8-order-framing.txt`: synthetic pre-type action items/empty next steps/common summary, whitespace/adjacent separators, quoted markers and confidence 101; also supplied directly to E4.

All paths above except this note are relative to `transgate/core/src/ai/`. No edits to ai/mod.rs, jobs/privacy, PG, manifests/lock, canonical ledgers or book.

## Public interfaces (published directly to p1X, p21, p1T, p24)

```rust
AnalysisParser::new() -> AnalysisParser // also Default
AnalysisParser::push(&mut self, text: &str) -> Result<(), AnalysisError>
AnalysisParser::progress(&self) -> &AnalysisProgress
AnalysisParser::finish(&mut self, successful_stop: bool) -> Result<AnalysisResult, AnalysisError>
AnalysisParser::into_progress(self) -> AnalysisProgress
// true means actual successful provider stop, NOT EOF/[DONE]/length/filter/tool/cancel.
analysis::sections(progress: &AnalysisProgress) -> Vec<(&'static str, serde_json::Value)>
// Source section names/data in schema order, summary first; arrival order not tracked.
// Absent optional fields omitted, present empty sections retained, generic summary once.
// AnalysisType serializes to generic/lecture/meeting/sales/support/interview (lowercase).

AnalysisResult { detected: DetectedType, summary: String, analysis: Analysis, warnings: Vec<AnalysisWarning> }
DetectedType { kind: AnalysisType, confidence: f64 }
AnalysisProgress {
    detected: Option<DetectedType>, summary: Option<String>, analysis: Option<Analysis>,
    content_sections: usize, expected_content_sections: Option<usize>, warnings: Vec<AnalysisWarning>
}
AnalysisFailure { error: AnalysisError, partial: AnalysisProgress }
// Plain caller-built DTO: combine finish's small error with parser.into_progress().
AnalysisFailure::message(&self) -> &'static str // also Display + Error
// Result/progress/failure/nested content: Clone, Debug, PartialEq, Serialize, Deserialize.

prompts::PromptMessages { system: String, user: String }
prompts::analysis_messages(lines: &[crate::transcript::Line]) -> PromptMessages
prompts::analysis_input_estimated_tokens(lines: &[crate::transcript::Line]) -> usize
prompts::summary_messages(lines: &[crate::transcript::Line], detailed: bool) -> PromptMessages
prompts::chat_system_message(sanitized_context: &str, language: Option<&str>) -> String
sanitize::input(content: &str) -> String
sanitize::output(content: &str) -> String
sanitize::transcript_text(lines: &[crate::transcript::Line], include_speakers: bool) -> String
sanitize::validate_chat_context(content: &str) -> Result<(), ContentError>
sanitize::validate_input_security(content: &str) -> Result<(), ContentError>
sanitize::validate_raw_question(raw: &str) -> Result<(), QuestionError>
sanitize::validate_question(sanitized_question: &str, raw_context: &str) -> Result<(), QuestionError>
QuestionError::message(self) -> &'static str
QuestionError::suggestion(self) -> Option<&'static str>
```

Policy owns invocation/precedence, role validation and durable transitions: raw required/100000 UTF-16/unsafe-pattern checks are unconditional; sanitized input security cap is 512000; context is 50..512000 inclusive; optional question validation (nonempty, <=10000, context available) follows quota precedence only when enabled. Preserve original accepted question separately; provider/accounting use its sanitized copy.

Parser limits remain **1 MiB cumulative raw generated UTF-8 (including deferred sections/separators), 256 KiB unfinished document, JSON depth 32**, refusing rather than truncating; the framer holds at most two additional pending marker bytes. Pre-type raw values remain bounded worker memory until type establishes applicability; no raw deferred content is added to persisted/public progress. These limits do not replace p1X's <=1 MiB **serialized** snapshot/final envelope check: source defaults and generic summary mirroring can expand typed encoding relative to raw JSON. Input/history/queue/deadline/concurrency/SSE limits remain their assigned owners' responsibility. `prompts::analysis_input_estimated_tokens` supplies nonnegative JS round(UTF16(actual submitted canonical analysis transcript)/4), without allocating another transcript or including prompt/history; policy was notified. This D8 metadata correction is unmetered, distinct from chat ceiling/settlement.

## Exact inspected legacy references

Read-only `/home/mu/Business/Transgate/frontend/src/`:

- `lib/ai/schemas.ts:24-335`; `schema-to-prompt.ts:240-338`; `prompt-templates.ts:93-146`.
- `lib/ai/ai-service-manager.ts:74-133,178-198,245-278,355-380`; `locale-map.ts:5-28`.
- `lib/ai/content-processor.ts:44-57,104-193`; `chat-validator.ts:46-74`.
- `pages/api/v1/ai/analyze.ts:77-96,207-295`; `chat.ts:47-71`.
- `lib/ai/deepseek-provider.ts:46-76` for model/options provenance; transport implementation belongs to p21.
- `lib/ai/validation.ts:35-61,80-121` for source null-return/invalid-item filtering, used by `components/AIAnalysis/specialized/sales/CoachingTips.tsx:79-85`. Old analyze route can emit an all-falsy terminal operand if it is not undefined; rendering subsequently hides invalid data. Do not collapse those separate old outcomes into an invented route-level skip.

D1/D4/D8 preserve ordinary prompt bytes while explicitly correcting framing/count/finish, strict invalid-section completion, literal substitution and analysis metadata. No hard summary word cap or generic YAML parser. E4 baseline always uses source-produced messages including JS expansion and retains `response_format=json_object`; corrected literal messages and D1 mode omission are separate named comparisons, not a generic ignore.

## Allocation-free finish migration

Concrete reported failure: lead clippy `result_large_err` at parser finish, with a 360-byte AnalysisFailure Err. No rerun by content owner. `finish` now borrows mutably and returns only AnalysisError; successful typed state moves into AnalysisResult via mem::take, while failure retains the finalized valid partial for progress()/into_progress(). The last document is still parsed once before terminal validation, preserving prior semantics. Repeated failed finish returns the original error without reparsing or duplicating count warnings; push/finish after success returns AlreadyFinished. No Box, partial clone, lint suppression or compatibility shim.

LSP references identified the product caller in ai/app.rs and E4 example e4_prompts.rs, plus owned domain cases. Exact contract sent directly to both owners before editing. p1X confirms product migration to Some(mut p)/finish/Err(error)+into_progress; E4 example source at lines 82-87 now constructs AnalysisFailure { error, partial: parser.into_progress() }, preserving its failure JSON. Owned domain callers are migrated. AnalysisFailure remains a serializable plain DTO, not a Result Err. No host/PG/root/example edits by content owner. Exported error-enum reference lookup twice returned LSP content-modified errors; targeted source lookup found no other exhaustive match beyond its owned message implementation.

Reported **pre-migration** evidence: offline E4 81 PASS in 5.41s, `target/p4-e4/run-odHWCQ/report.json`; initial core 239 PASS included D8 content tests. Later D9 core 243 PASS / four expected RED were reported as non-content failures. This evidence does not verify the finish API migration and establishes no live acceptance.

## Execution and dependencies

Lead-only selectors, **not executed by this owner**:

```sh
cargo test --locked -p transgate-core ai::analysis::tests::
cargo test --locked -p transgate-core ai::analysis::sanitize_tests::
```

No environment/provider/database required for those pure cases. Dependency: `regex = "1"` in transgate-core (requested through p1X/lead; no worker manifest/lock edits). p1X owns module declarations/root integration. Lead runs runtime/loopback six-schema coverage after coherent integration; test source alone is not runtime proof.

E4 p24 reports the final independent Rust runner at lead-authorized `transgate/core/examples/e4_prompts.rs`. It calls actual builders/sanitation on the same canonical Lines as legacy TS, compares ordinary analysis/chat/summary arrays, then feeds Rust-produced messages through p21 transport. D8 coordination supplied real parser progress/result/failure serde interfaces, source confidence and render-filter helper references, the D8 core fixture, and the actual-transcript estimate API. p24 owns differential fixtures: actual old JS replacement expansion versus a source-template callback-literal oracle for all four directives; confidence/order/framing parity versus separately named strict invalid/all-falsy interruptions. Existing source-order history/retry and `fr`/French versus `fr-CA` cases remain. No execution claim; E4 details/ownership are in `implementation/e4.md`.

p24 reports D8 differential source now authored: the owned example invokes real AnalysisParser, stops on push error and calls finish(false) for retained serde partial/error. Actual route streaming statements/helpers are AST-extracted without auth/DB/HTTP, and actual `validation.ts` is loaded. Cases distinguish all-falsy fallback ending undefined (old route omits) from terminal false (old route emits; actual validateArrayForRender(ActionItemSchema, false) returns []). Confidence normalization/out-of-range values, pre-type action items, whitespace/adjacent markers and old quoted-marker loss are covered. Literal-four-directive and Unicode actual-canonical metadata comparisons remain separately named, unmetered corrections. This is p24's source-authoring report, not executed evidence.

Clauses: content portions of AI-002/003/004/010/012/013/020/024/026, LNG-030 and D1/D4/D5/D8. No full-clause or green-gate claim. Live model/prompt quality, multilingual generation, provider privacy-flag support/retention/region and deployment provenance remain unproved.

## Next owners

- p1X: consume published content APIs, apply policy/serialized snapshot bounds and fences, integrate modules.
- p24: independent source/Rust prompt comparison plus baseline-versus-correction E4.
- Lead p1E: integrate dependency/lock, execute pure selectors and real transport/host surfaces, route any diagnostics back to content owner. No execution delegated to this worker.

## Bounded AI-001..026 evidence map (content ownership)

Coordinated with p1X before appending here; policy claims remain in [policy.md](policy.md), host scenarios in [hosts.md](hosts.md), E4 provenance in [e4.md](e4.md). This is a source-to-evidence index, **not whole-clause acceptance**. No product source, canonical row or phase status changed; no execution, provider call or new agent.

Current execution pointers supersede the older historical pointers above: latest offline E4 report is `target/p4-e4/run-dd8Tk7/report.json`, status `PASSED-OFFLINE-ONLY`, 81 result rows (lead artifact787). The full offline **workspace805** run includes the migrated core parser; 805 is not a core-only count. Reading existing evidence is not a new run. Runtime selectors below identify substantive scenario source; they do not independently assert execution. No live model/quality/privacy acceptance, browser rendering, binding parity or commercial whole-clause signoff is owned here.

Selector prefixes: `A` = `transgate-core::ai::analysis::tests` (`analysis/tests.rs`); `S` = `transgate-core::ai::analysis::sanitize_tests` (`analysis/sanitize_tests.rs`); `P` = `transgate-core::ai::app::tests`; `Q` = `transgate-core::ai::quota::tests`; `C` = `transgate-core::ai::consent::tests`. Core paths are under `transgate/core/src/ai/`. E4 names below are literal `results[].name` values in that report; brace sets enumerate the named cases, not an extra execution claim.

Runtime scenario locators (other owners):

- **H-auth**: `transgate/billing/tests/runtime/ai.rs::ai_http_auth_consent_and_owned_job_refusals_happen_before_any_provider_dispatch`.
- **H-resume**: same file, `ai_http_disconnect_cross_replica_resnapshot_and_saved_final_never_redispatch`.
- **W-summary**: `transgate/server/src/workers/pg_tests/ai.rs::a_held_then_failed_ai_summary_cannot_hold_partial_error_mail_or_fail_completed_transcription`.

| Clause | Exact substantive content test / E4 / runtime scenario | Scope left outside content ownership |
|---|---|---|
| AI-001 | `P::analysis_streams_typed_sections_in_source_event_order`; H-auth. | **Not owned**: session/owner/consent/feature admission and pre-stream HTTP statuses (p1X/p1T). |
| AI-002 | `A::six_shapes_survive_every_character_boundary_with_typed_optional_content`; `A::ratings_null_and_cross_type_sections_are_validated`; E4 `{meeting,lecture,sales,support,interview,generic}-deepseek-fragmented`. | Owns shapes, relevant optional fields and exact prompt instructions, not live accuracy/objectivity, generated word count or P5 specialized rendering. Under 300 words is a prompt, not a new hard truncation. |
| AI-003 | `A::sections_are_available_before_provider_finish_and_count_is_advisory_n_minus_two`; `A::only_a_single_restricted_count_header_is_yaml_and_json_trim_matches_source_js`; `A::incomplete_bad_schema_yaml_and_unsuccessful_terminal_preserve_valid_partial`; `P::analysis_streams_typed_sections_in_source_event_order`. E4 `D8-parser-{actionItems-before-detectedType,whitespace-around-separator,adjacent-next-document,quoted-separator-data,all-falsy-section-interrupts,all-falsy-terminal-value-emitted-then-filtered}-fragmented`; `protocol-unexplained-EOF`, `protocol-length-not-success`. | Owns framing/count/schema/retained-partial mechanics; product event delivery/committed success and UI progress remain p1X/p1T/P5. YAML correction below is required before a whole-clause mapping. |
| AI-004 | `S::canonical_analysis_is_raw_while_summary_and_chat_drop_blank_lines_and_sanitize`; `S::analysis_substitutes_transcript_literally_instead_of_expanding_js_replacement_directives`; `S::analysis_estimate_uses_actual_raw_transcript_utf16_rounding_not_bytes_prompt_or_chat_ceiling`. E4 `meeting-deepseek-coalesced`, `meeting-deepseek-coalesced-D1-omitted-mode`, `D8-literal-transcript-directives-fragmented-{old-messages,D8-literal}`. | Owns complete canonical text/literal substitution/estimate. Active DeepSeek selection and deadline belong to policy/host; omitted json_object is D1, literal substitution/metadata are D8, not unchanged-source parity. |
| AI-005 | E4 `meeting-deepseek-coalesced` compares exact request model/0.3/1000/stream/store=false/ephemeral=true; `meeting-openai-explicit-alternate` and `chat-en-openai-explicit-alternate` exercise the explicit alternate. | Request evidence is offline and shared-transport-owned (p21/p24); no model availability, flag acceptance, retention/region/cost, configured deployment model or automatic fallback claim. |
| AI-006 | `P::analysis_streams_typed_sections_in_source_event_order`; H-resume identifies committed-state/reload behavior. | **Not owned**: D1 server-owned pending/partial/final persistence, PG fencing and removal of client-owned writes (p1X/p1R/p1T/P5). |
| AI-007 | No content test proves newest-cache selection, null/empty imported completed rows or cache-read failure disposition; consult policy evidence. | **Not owned**: cache/source revision/imported rows, regeneration admission and browser state. A parser result alone is not cache evidence. |
| AI-008 | No content test proves createdAt age thresholds or browser warning/interruption; policy `presentation::staleness` is the implementation locator, not an executed scenario claim here. | **Not owned**: pending/partial staleness, warning timing and rendered state (p1X/P5). |
| AI-009 | `P::a_request_key_replays_and_never_redispatches`; H-resume. | **Not owned**: retry clean-slate/current-pointer transitions and browser duplicate-click guard. Same-key replay evidence is not a substitute for explicit new retry evidence. |
| AI-010 | `S::raw_route_rejections_precede_sanitation_and_keep_source_substring_semantics`; `transgate-api::ai::tests::malformed_identity_or_history_cannot_be_coerced_into_another_request`; H-auth. | Owns UTF-16 raw 100000/required/unsafe-pattern rules; auth/job/history-container/role enforcement and HTTP surface are p1X/p1T. |
| AI-011 | H-auth; `P::refusal_during_streaming_fences_the_answer`; `C::refusal_fences_writers_but_regrant_restores_reads`. | **Not owned**: visible transcript, both exact flags, fresh consent/epoch and dispatch barriers (p1X/p1R/p1T). E4 bypasses these policies deliberately. |
| AI-012 | `S::transcript_context_and_question_security_use_source_utf16_boundaries`; `S::optional_question_validation_is_separate_from_unconditional_route_cap`. | Owns 50..512000 context and optional 10000-question boundaries; calling them in source quota/config precedence, including disabled validation, remains p1X/host whole-clause evidence. |
| AI-013 | `S::chat_language_instructions_keep_source_exact_code_lookup_and_raw_unknown_fallback`; E4 `chat-ar-history-deepseek-fragmented`, `chat-tr-retry-deepseek-fragmented`, `chat-fr-exact-locale-deepseek-coalesced`, `chat-fr-CA-raw-fallback-deepseek-coalesced`; `transgate-api::ai::tests::chat_forwards_complete_ordered_input_to_the_single_core_validator`. | Owns exact transcript-only/language prompt and sanitation; role authorization is policy-owned. No live answer relevance/multilingual correctness or browser retry/history acceptance. |
| AI-014 | `P::paid_callers_bypass_the_quota_lane`; policy evidence also owns the plan-query/savepoint boundary. | **Not owned**: commercial entitlement, paid check/increment bypass and plan-error handling; PG proof remains p1X/p1R. |
| AI-015 | `Q::admission_keeps_source_precedence_and_maxima`; `P::freemium_caps_and_the_single_metered_lane_refuse_before_dispatch`. | **Not owned**: commercial admission, question-only reservation, concurrency/overshoot and actual PG two-process behavior. |
| AI-016 | `Q::settlement_charges_the_combined_estimate`; `P::a_freemium_answer_saves_the_pair_and_settles_once`. | **Not owned**: combined question+answer commercial settlement, enabled counters and durable exactly-once charging. D8 analysis estimate must not be used as this evidence. |
| AI-017 | `Q::configured_caps_follow_the_source_parser`; `Q::periods_reset_on_utc_day_iso_week_and_calendar_month`. | **Not owned**: env cap parsing, period policy and stored commercial counters (p1X/p1R). |
| AI-018 | `Q::admission_keeps_source_precedence_and_maxima`; `Q::late_settlement_never_touches_a_later_period`; `P::an_answer_settled_after_midnight_lands_in_its_admission_day`. | **Not owned**: PG reset/maxima, original-period settlement and imported null-reset history. |
| AI-019 | `P::recovery_completes_a_staged_final_without_the_provider` is the local-reconciliation locator; unavailable quota/unknown-COMMIT boundaries require p1X/p1R evidence. | **Not owned**: D2 fail-closed storage/atomic settlement corrections, not preserved old fail-open behavior; no storage failure proof supplied by content tests. |
| AI-020 | `S::cumulative_sanitation_handles_html_email_and_spacing_across_provider_boundaries`; `P::a_quiet_provider_still_checkpoints_the_partial_before_the_final`; H-resume. | Owns cumulative source sanitation; raw/replacement subscription modes, persisted final and actual browser replacement are p21/p1X/p1T/P5. |
| AI-021 | `P::a_freemium_answer_saves_the_pair_and_settles_once`; `P::recovery_completes_a_staged_final_without_the_provider`; H-resume. | **Not owned**: D1 server-owned pair ordering/atomic save, unknown commit, visible unsaved-answer/sync-warning browser behavior. |
| AI-022 | `C::no_row_is_no_consent_and_delete_still_covers_legacy`; `C::refusal_fences_writers_but_regrant_restores_reads`; `P::consent_and_limit_metadata_need_no_prior_consent`. | **Not owned**: exact consent metadata/version/path/provider upsert and grant/refusal endpoint/UI evidence (p1X/p1R/p1T/P5). |
| AI-023 | `P::refusal_keeps_artifacts_and_delete_purges_older_epochs`; `P::a_failed_purge_stays_pending_and_covered_rows_stay_hidden`. | **Not owned**: D3 all-epoch purge, callback copies, quota retention and PG revocation races; old incomplete cleanup is explicitly corrected, not reproduced as desired behavior. |
| AI-024 | `S::canonical_analysis_is_raw_while_summary_and_chat_drop_blank_lines_and_sanitize`; E4 `summary-{concise,detailed}-ignored-language-deepseek-fragmented`; `P::the_completion_summary_releases_callback_ordering_with_an_explicit_outcome`. | Owns separate exact summary prompt/input-output rules and ignored language. Durable consent/producer/save/read and callback ordering remain p1X/p1R/p1T; P7 owns outbound delivery; no live summary-quality claim. |
| AI-025 | `P::the_completion_summary_releases_callback_ordering_with_an_explicit_outcome`; W-summary (real worker/mail isolation scenario source). | **Not owned**: enabled/transcript gates, absent/denied/failed outcome, transcription/mail isolation and actual runtime proof. |
| AI-026 | `S::source_input_and_output_are_distinct_paths_not_token_filters`; `S::source_js_whitespace_and_ascii_case_folding_are_not_unicode_generalizations`; `S::cumulative_sanitation_handles_html_email_and_spacing_across_provider_boundaries`; E4 `chat-sanitized-deepseek-fragmented` plus AI-004/024 cases above. | Owns path-specific input versus output filters and raw analysis join. Output does not acquire the input SQL/script-block filter. No zero-PII, plain-text browser rendering or provider nonretention guarantee. |

### Exact AI-003 ledger wording correction (lead only)

Targets: `transgate/docs/rules.md:363` and its inventoried duplicate `transgate/discovery/ai.md:51`. Replace the sentence **“It accepts JSON documents and a legacy YAML fallback, split on three hyphens.”** with:

> Content documents are JSON-only. Legacy YAML syntax is accepted only for the `documentCount: N` count header; the source splits accumulated output on literal `---`.

Extend that row's source citation to `frontend/src/pages/api/v1/ai/analyze.ts:77-96,207-303` (especially `:213-227` for the count fallback). `parseDocument` requires `{` and uses JSON.parse; the sole YAML-style extraction is the documentCount regex. This corrects the **historical source inventory**, not a claim that arbitrary YAML sections were implemented. D1 separately records N-2 advisory count/terminal validation; D8 records quote-aware top-level framing and strict invalid/inapplicable/all-falsy interruption. `A::only_a_single_restricted_count_header_is_yaml_and_json_trim_matches_source_js` and `A::incomplete_bad_schema_yaml_and_unsuccessful_terminal_preserve_valid_partial` are the substantive regression locators. Lead retains all canonical/status edits; none made here.
