# P4 AI: independent SOL proposal

2026-10-24. Advisory only, not an implementation contract or verification report. Read `/tmp/rustr-p4-advisors.md`, PRD, progress board, the complete AI discovery note, related discovery/parity clauses, P3 implementation contract/decisions, and targeted source. No sibling proposal read; no builds, tests, provider transactions, secrets, implementation or book edits.

## 1. Recommendation and evidence

Keep Transgate's product policy in `transgate/core`. Add two narrow reusable crates, `llm-core` and `llm-http`, for completion transport and pure streaming state. Do not create `modules/ai`, a provider plugin framework, retrieval, tools, a second billing system or a second auth system. Billing and CMS remain the PRD's reusable bounded contexts.

Use the existing DeepSeek selection, `deepseek-chat`, temperature 0.3, output cap 1000. Keep the OpenAI HTTP adapter as the existing alternative, not an automatic fallback. Preserve the six specialized shapes, separate general summary, whole transcript, source sanitation, untruncated caller history, source language outcomes and paid bypass. Correct generation races, privileged history roles, final-answer reconciliation, quota races/reset initialization and revocation resurrection explicitly, not under an unsupported claim of exact parity.

For the conflicting analysis framing, the smallest correction is **retain the exact multi-document prompt and omit `response_format=json_object` on the live analysis request**. This changes one incompatible request option rather than redesigning every prompt into a new envelope. Parse complete JSON documents separated by an unquoted delimiter line. Retain the legacy YAML *count header*, not a general YAML interpreter. Keep the captured old request separately for E4.

### Source anchors used

All `frontend/...` references below are read-only files under `/home/mu/Business/Transgate`.

| Evidence | Implication |
|---|---|
| `frontend/src/lib/ai/ai-service-manager.ts:74-149,152-293,355-380`; `deepseek-provider.ts:46-125`; `openai-provider.ts:37-113` | Actual completion path is fetch, not the Axios availability client or an assumed installed SDK. Active constructors choose DeepSeek explicitly. Analysis/general-summary language options are ignored; chat uses job language. |
| `frontend/src/lib/ai/schemas.ts:24-301`; `prompt-templates.ts:93-146`; `pages/api/v1/ai/analyze.ts:77-96,207-295` | Six distinct shapes; delimiter framing; JSON-only section parsing. Discovery's phrase “legacy YAML fallback” is broader than the code: YAML fallback only extracts `documentCount`. Source does not schema-validate sections. |
| `frontend/src/lib/ai/chat-validator.ts:80-289`; `chat-config.ts:37-74`; `content-processor.ts:44-57,77-193` | Question-only admission; question+sanitized-answer settlement; JS string-length units; explicit sanitation; separate checks/increments. |
| `frontend/supabase/migrations/20251130113249_remote_schema.sql:656-725` | Missing-row `SELECT INTO` can overwrite the calculated reset date; stored max is used for SQL question checks. This is source evidence, not a newly reproduced production failure. |
| `frontend/src/utils/user.ts:21-44`; `modules/billing/core/src/freemium.rs:106-132` | Use billing facet `no_unexpired_paid_package` / `is_freemium_app2`, not the open-package balance predicate. A future-start paid package with unexpired end already bypasses chat limits. |
| `frontend/src/lib/ai/analysis-service.ts:200-406`; `features/aiAnalysis/viewmodels/useAnalysisSummaryViewModel.ts:105-279`; `useChatViewModel.ts:101-310` | Cache and partial states, created-at staleness, client-owned races, answer-visible save failure, and retry behavior are observable requirements. |
| `frontend/src/lib/ai/consent-service.ts:9-11,55-167`; `features/ai-consent/services/ConsentService.ts:60-160`; `pages/api/ai/consent.ts:7-54`; `lib/ai/ai-config.ts:134-137` | Active writes use version 1.0 and `/legal/privacy-policy-en`; config declarations do not establish runtime version enforcement. Analysis checks a different predicate from chat/summary. Both browser and server cleanup paths omit analysis and ignore returned delete errors. |
| `transgate/core/src/jobs/ports.rs:117-238`; `jobs/events.rs:22-43`; `privacy/app.rs`; P3 contract and decisions D9-D15 | Attach AI persistence to the host transaction/pool and surviving erasure barrier. Completion event already identifies P4 general-summary and P7 callback consumers. P3 interfaces remain under repair. |
| `crates/taskr-work/src/work.rs:171-185`; `crates/taskr-wasm/src/lib.rs:109-131`; `crates/taskr-ffi/src/app.rs:24-40` | Taskr already has an owned completed audio-work transcript and presenter listener conventions; no change to dependency-free taskr-core is necessary. |

Public primary documentation read: [DeepSeek JSON Output](https://api-docs.deepseek.com/guides/json_mode) explicitly warns about empty/truncated output; [Chat Completions](https://api-docs.deepseek.com/api/create-chat-completion) documents SSE `[DONE]`, finish reasons and usage. The currently served reference lists `deepseek-flash`/`deepseek-v4-pro`, not the captured `deepseek-chat`. **This is a compatibility evidence gap, not permission to substitute models.** Retain the source model in code/fixtures; authorized deployment acceptance must establish its availability or record a separate model decision. Neither documentation nor `store=false`/`ephemeral=true` proves zero retention.

## 2. Clause disposition

P = preserve product outcome; C = explicit defect correction recommended below; L = rendered evidence remains later. Every row still needs a real rule test or approved exclusion in the final ledger. Proposed test labels here are not existing test claims.

| Clause | P4 requirement and proposed evidence | Disposition |
|---|---|---|
| AI-001 | Session, explicit owned job, consent, transcript and exact feature flag gates; preserve pre-stream 401/404/403/400/503, plus source input/DB errors. `analysis_admission_matrix`. | P; consent inconsistency C |
| AI-002 | Detect all six types; objective/relevant optional sections and under-300-word prompt; no fabricated sections or replacement generic summary. Six typed fixture cases. | P |
| AI-003 | `sectionCount`, `typeDetected`, `sectionReady`, `complete`, `error` meanings; count header JSON or legacy YAML, JSON sections. Chunk-boundary/malformed/truncation cases. | P/C framing |
| AI-004 | Entire canonical speaker/text transcript, DeepSeek. Preserve ignored language outcome; explicitly remove incompatible single-object flag and enforce local deadline. | P/C request framing/timeout |
| AI-005 | DeepSeek model/0.3/1000/stream/store=false/ephemeral=true; OpenAI configured model or source default, same temperature/cap, no DeepSeek-only flags. Captured HTTP requests. | P; provider acceptance unproved |
| AI-006 | Server now owns replacement, pending, partial, completed/interrupted and metadata writes. Real PG partial visible while upstream remains open. | C ownership, P states |
| AI-007 | Newest matching generation; null/empty completed legacy data is a miss; pending/partial/interrupted are returned even empty. Cache read failure may show idle but cannot bypass durable admission. | P/C safe admission |
| AI-008 | Created-at age: partial `>=5min`, pending `>5min` interrupted; pending `>1min` warns. Completed populated cache succeeds. Pure boundary tests. | P; L actual views |
| AI-009 | Explicit retry replaces old analysis content and creates one new generation; duplicate clicks/tabs attach rather than duplicate calls. Retry/supersession race. | P/C cross-tab fence |
| AI-010 | Session, jobId, nonblank string, raw 100000-character cap, history array and captured script/URL/SQL-like deny patterns. Strict history roles additionally required. | P/C roles |
| AI-011 | Owned nonempty transcript, exact AI/chat flags, consent true and no revocation. Read-time checks are not replaced with frontend gating. | P |
| AI-012 | Sanitized context 50..512000 characters inclusive; enabled validation rejects sanitized questions >10000; preserve the distinct route cap. Emoji/whitespace boundary cases. | P |
| AI-013 | Source transcript-only prompt, job/source-language instruction, sanitized complete caller history without a 50-message truncation. Role injection and long-history fixture. | P/C roles only |
| AI-014 | Billing facet 3; paid skips both limit checks and increments; plan lookup failure treated freemium, without changing auth/barrier checks. Real path with exhausted counters. | P |
| AI-015 | Question `count >= max` refuses; tokens `count + estimate(question) > max` refuses; no speculative answer reserve. Serialize metered account generations instead of admitting overlapping unknown answer spends. | P formula/C concurrency |
| AI-016 | Successful answer: enabled question counters +1; tokens `ceil(UTF16_len(sanitized_question + sanitized_answer)/4)`; no context/history. Idempotent transactional settlement. | P/C atomicity |
| AI-017 | Source cap parsing, absent/unparseable = absent; zero disabled; daily default, weekly/monthly/transcript recognized; user transcript maps daily; transcript counters never reset. | P |
| AI-018 | First timed row gets actual next UTC boundary; reset at `now >= reset_at`; preserve existing stored question max until explicit migration/config policy. Midnight/month/year boundary cases. | P/C missing-row defect |
| AI-019 | Explicitly replace lookup fail-open with unavailable-before-provider; replace lost increments with durable same-generation settlement, never another provider request. Already-visible answer survives a save error with warning. | C, not parity |
| AI-020 | Preserve natural raw token spacing in provisional plain text; terminal event includes authoritative sanitized answer replacing draft. EOF without valid terminal completion is interrupted, not success. | C final reconciliation |
| AI-021 | Server saves whole pair in one transaction, stable pair order, provider metadata. Display answer with sync warning if persistence fails; retry persistence does not regenerate. Reload proves pair once. | P/C ownership/order |
| AI-022 | No row=false; grant/refusal timestamps and fixed version/provider/path; no new mandatory version match. Epoch fencing is separate from public consent version. | P |
| AI-023 | Revocation also clears specialized analysis, pending drafts and in-flight outputs; all local deletes checked. Preserve chat-limit counters to prevent revoke/regrant resetting quota; no unrelated feedback deletion on revoke. | C omitted data/errors |
| AI-024 | General summary is separate prose, sanitized input/output, concise unless requested detailed, under-300-word prompt, no chat quota. Always use real account consent in server path. | P/C remove nullable bypass |
| AI-025 | Exact AI flag, nonempty transcript; disabled/failed summary gives absent summary and never fails completed transcription or invents one. Completion outbox consumer and callback handoff exercised. | P |
| AI-026 | Port exact path-specific sanitation, not an improved universal scrubber: chat/summary sanitize, analysis joins raw speaker/line. Render all drafts/sections as text, never HTML. | P; no zero-PII claim |
| AI-027 | Public voice demo has no job/consent link in source. Excluded under PRD R4 assumption; lead records an explicit excluded decision in final rule/parity gate, not a fake passing test or silent row removal. | Excluded, not implemented |

### Cross-ledger obligations

- **FRE-024:** P4 closes actual admission AND successful increment bypass, not just parsing. Use existing billing facet; future-start/nonexpired paid, expired paid, missing package and failed plan lookup cases. Keep the four observed deployed caps as configuration evidence (50/20000 per transcript, 500/50000 per user/day), not hard-coded new defaults or proof of enforcement. FRE-020/021 are view cuts, not AI transcript-context truncation; FRE-022 download remains unrestricted. Existing distinct billing definitions must remain distinct.
- **PRV-005:** enroll consent, analyses, summaries, highlights, chat, counters, generations, stream replay/drafts and pending settlement in account cascade; complete erasure blocks future writes. PRV-011 soft-delete semantics remain distinct from hard deletion; do not silently delete AI history on ordinary soft delete. PRV-014..017: transcript retention still deletes lines only, not saved AI artifacts; it must fence active generations tied to removed lines without erasing preserved completed derived content. PRV-032..034 history list filtering is not an AI authorization TTL.
- **PRV-038..043:** defaults, write metadata, grant/refusal/revoke, path-specific source differences and session API clauses belong to P4. Unify true+not-revoked as an explicit correction for inconsistent imported rows; do not invent version-based invalidation. PRV-044 flag-aware settings state is P4; actual settings tab is P5. PRV-045 remains archived, not a revived language consent feature. PRV-053..059 legal/retention/region promises remain final copy/evidence obligations; do not infer they are satisfied by local deletion.
- **LNG-030:** preserve current no-language-instruction behavior for specialized analysis/general summary. Preserve chat job/source-language mapping, not translation target. Missing AI map entries must not shrink 61 transcription languages or 16 UI locales. LNG-031 remains no translation-beta consent; speech translation remains Soniox, not an LLM call.
- **API-F18..21, API-IDN-002, API-BIL-011:** session-owned analysis/chat/consent/limits contracts are implemented in P4, even though their route names include v1. No API-key access is added. P7 owns final public contract/OpenAPI/compatibility decisions, not these backend behaviors. API-015..017: expose latest saved general summary to the existing result/callback read boundary; missing/failed generation omits it and does not add a callback retry scheme. Final outbound HTTP callback verification remains P7.
- **EML-004/005:** no AI failure email is invented. A failed/disabled summary must not transition the transcription job to failed, cause user/support failure mail, or erase its existing partial-error mail. EML-001 transport and EML-012 shared template behavior remain unchanged. Completion-consumer failure isolation must be exercised with the existing outbox, not assumed from a pure test.
- **Final rendered parity remains:** AI-F01..10; WEB-027..030, WEB-041/042, WEB-PRV-009/010 in P5; native WEB-069/070 and consent settings in P8. Copy/export/rating/confirmation mechanics retain their inventoried obligations. P4 supplies meaningful view state and working server operations; it does not mark rendered rows green. Book/scenario documentation waits until the user-authorized final book pass.

## 3. Boundaries and public seams

These are proposed interfaces for coordination, not claims those APIs already exist. Reuse async `BoxFuture` conventions, UTC microseconds, UUID accounts/generations and signed Transgate job ids. No provider call inside a database transaction.

| Location | Owns | Must not own |
|---|---|---|
| `crates/llm-core/src/{ports,completion,stream}.rs` | Plain request/message/delta/error/usage types; narrow stream port; dependency-free pure stream reducer, sequence/draft/final replacement. | Billing, SQL, auth, Transgate schemas/prompts, provider selection policy. |
| `crates/llm-http/src/{deepseek,openai,sse,client}.rs` | Two concrete upstream HTTP adapters; bounded SSE decoder; shared application-stream client codec, native and wasm client paths. Server credentials/adapters compile only on server targets. | Product quota/consent/cache/retries; no dependency back to hosts, taskr-core or billing. |
| `transgate/core/src/ai/{domain,prompts,parse,sanitize,quota,ports,app}.rs` | Six schemas, source prompts and path-specific text rules, consent predicates, generation/cache decisions, admission/settlement and orchestration. | HTTP implementations and SQL. |
| `transgate/core/src/presentation/ai.rs` | Analysis/chat/consent state, retry/attach effects, stale thresholds, quota errors, sync warning, language-independent semantic labels/tones. Composes shared stream reducer. | Browser persistence or database queries. |
| `transgate/jobs-postgres/src/ai.rs`, its migrations | Generation/consent/counters/chat/summary persistence; same pool and attached transaction as jobs/billing; unique constraints and fencing. | Business calculations in SQL triggers/procedures. |
| `transgate/api`, `transgate/http/src/ai.rs`, server composition | Typed request/snapshot/events, session auth, HTTP/SSE mapping, actual consumer workers and bounded streaming runtime. | Duplicate product decisions. |
| `crates/taskr-work/src/ai.rs`, existing store/http/compose; taskr-client/wasm/ffi | Small audio-work summary consumer with own storage and host barrier; reuse llm transport/reducer. | Six Transgate schemas, copied quotas or new authentication. |

### Shared completion seam

- `Llm::open(request: CompletionRequest) -> BoxFuture<Result<Box<dyn CompletionStream>, LlmError>>`; `CompletionStream::next(&mut self) -> BoxFuture<Result<Option<CompletionEvent>, LlmError>>`; explicit `cancel(&mut self)` stops the local transport only. Request is owned for the request lifetime; borrow immutable prompt/transcript buffers where practical rather than repeatedly copying them.
- `CompletionRequest`: messages with typed `System|User|Assistant` roles, server-selected provider/model, temperature, max output tokens, response mode, request correlation id, local absolute deadline and resource bounds. DeepSeek privacy flags are explicit adapter behavior, not a portable retention guarantee. Do not forward account email/name or caller-supplied provider URLs/keys.
- `CompletionEvent`: response identity when observed; exact content delta; finish reason; optional provider-reported input/output usage; terminal marker. `LlmError`: definite HTTP refusal with status/retry-after, auth/config, protocol, local resource bound, timeout, transport loss, canceled. Record whether dispatch may have reached the provider. A retryable hint is not an automatic retry instruction.
- `StreamState::update(StreamEvent) -> effects`, `view() -> StreamVm`: generation id, sequence, draft, authoritative final, status, persistence state, sanitized public error. Sequence gaps request a snapshot; duplicates are ignored; old generations cannot alter the active view. Product payloads such as six section types stay in the Transgate reducer, not a generic JSON plugin.

### Host transaction seam

Extend the existing `JobUow` with `ai(&mut self) -> StoreResult<Box<dyn AiTx + '_>>`, analogous to its existing billing attachment. Extend query surface for owner-scoped generation snapshot/history/limits. Do not introduce a second pool or a nested independently committing transaction.

`AiTx` operations sufficient for separate developers:

1. `lock_consent(account) -> ConsentRecord` (missing defaults false), `save_consent(expected_epoch, decision)`; epoch monotonically changes on every decision.
2. `load_generation_for_update(account, generation)`; `claim_generation(NewGeneration, expected_active) -> Created|Existing|Conflict`. Unique `(account, client_request_id)` binds immutable operation/payload digest; mismatched reuse is 409. One active specialized analysis per owned job; replace under the job lock.
3. `quota_snapshot(account, job, periods)`; `reserve(ReservationPlan)` and `settle(SettlementPlan)` implement core-calculated changes, idempotent by generation. Store original period ids and frozen admission metering policy.
4. `append_progress(Fence, seq, typed_event)` and `finish(Fence, TerminalArtifact)`; fence includes account, consent epoch, job content revision, generation and worker token. Neither may upsert a missing/erased account or job.
5. `save_chat_pair(generation, question, answer, allocated_pair_order)`; `save_general_summary(completion_identity, content_revision, summary)`; `purge_consent_content(account, invalidated_epoch)` and `erase_account_ai(account)` in the host transaction. Every delete error propagates.

Add a **transcript-content revision** to jobs or an equivalent persisted revision advanced on line/speaker edits, translation replacement and retention deletion. A lifecycle revision alone is insufficient: the current line edits are update-only LWW and must remain so. Hash/version actual canonical content plus effective language policy, prompt version, schema version and provider/model into the cache key; do not cache by job id alone. Protect the content snapshot with account/job locks, then release before dispatch. P3 integration owner must publish the concrete attach/barrier/edit seams before parallel code changes; the proposal does not assume its repair is frozen.

Product payload inventory (nested field types/defaults remain those in the cited `schemas.ts`, not a new flattened DTO): meeting `actionItems/decisions/discussionPoints/nextSteps`; lecture `topics/concepts/reviewQuestions/studyGuide`; sales `callMetrics/coachingTips/customerInsights/nextSteps`; support `issueResolution/sentiment/insights/kbUpdates/followUpActions`; interview `assessment/skillsAssessment/notableResponses/nextSteps`; generic `summary/highlights`. Summary is also a separate streamed section for specialized types; general completion summary is a separate saved artifact. Preserve optional sections, timestamp units, owner/deadline, status/priority/assessment enums and rating bounds.

## 4. Generation, reconnect and persistence protocol

### Admission and identities

Client sends a random `clientRequestId` before first dispatch. Session is resolved server-side; never accept an owner from the body. In one short transaction: account lock, fresh erasure-barrier check, job lock, fresh owned content/consent/flags, idempotency and cache resolution, metering admission where applicable, durable generation claim. Commit, then call provider. If durable admission fails, no provider call, even when the UI's cache read failed open.

Use the existing analysis/chat POST surfaces with additive request id/retry metadata and existing event meanings. Add owner-scoped generation snapshot and `GET /api/v1/ai/generations/{id}/events?after={seq}` for attach. Fetch streaming works with session headers on web/native; do not rely on browser EventSource carrying custom auth. Consent and limits retain their inventoried session surfaces; P7 may consolidate route naming only with deliberate contract migration.

Generation states: `Pending -> Streaming/Partial -> Completed|Interrupted|Superseded|Revoked`. Record request dispatched/response identity/local termination/remote outcome separately. A local status is not proof of provider cancellation or retention deletion. Before-dispatch claims can resume safely after a crash; a persisted dispatch intent without a definite refusal is uncertain and must not automatically call the provider again.

### Stream events

Each durable application event has protocol version, generation id, monotonically increasing `seq`, and kind. Analysis keeps the five source named events; chat keeps token/error/done and adds final answer plus persistence outcome to done. Snapshot conveys cumulative draft/sections/status and watermark. Final content replacement is explicit, not a concatenated token. Terminal completed events are persisted before announcing durable success. If that commit fails, a separate non-replayable, generation/worker-fenced control notification can carry the visible final answer and unsaved warning without advancing the durable cursor; reconnect resumes only from the last committed watermark.

Persist bounded progress batches before publishing their durable sequence. Analysis writes at section boundaries; chat batches adjacent content deltas without changing bytes or spacing. A small generation-local event table supports reconnect; compact terminal events into the canonical snapshot/artifact, then serve a snapshot when an old cursor predates retained events. This is not a generic event bus or workflow engine. Page replay; do not load every historical turn or event to reconnect one generation.

A subscriber disconnect does **not** start a new provider call or cancel an otherwise valid generation: source analysis already continues outside its mounted hook. On reconnect/reload/native resume, load snapshot then resume after its watermark; other replicas use the same durable state. Lost response to admission is retried with the same request id. Same id after completion replays the result; a new explicit retry gets a new id and may cost another provider call.

Worker restart never fabricates completion from the last delta. Expired worker claim can be fenced and marked interrupted; no automatic remote resubmission. A late old worker cannot append after a new worker token/generation/epoch/content revision. Provider uncertainty remains observable even after local progress stops.

### Cache, edits and retry

- Latest populated completed *matching-key* result is success; completed legacy null/empty is a miss. Pending/partial/interrupted remain visible even empty. No background reanalysis merely because a user opened a stale or edited transcript.
- Preserve created-at thresholds exactly, not last-write activity. At the stale boundary, a server transition fences the prior generation before presenting interrupted. The view's tick event handles warning/age without each shell reimplementing it.
- Edit advances content revision. Keep the old result viewable as stale, not reusable as a current cache hit; fence any in-flight generation on that old snapshot. Do not resurrect removed lines from its answer. Retry uses the latest content and replaces old analysis rows/content atomically, preserving the source clean-slate outcome rather than silently serving the old analysis as new.
- Chat accepts sanitized caller history with only user/assistant roles, preserving order and every supplied turn within the explicit resource envelope. Server-owned persistence is **not** permission to replace caller history with a different unbounded query or to truncate to nominal 50 turns. Omitted history remains empty, as source.
- Persist the original accepted user question for displayed/reloaded history, as `ChatMessageRepository.ts:31-58` does; use a separate sanitized question for provider input and token accounting. The authoritative assistant answer is sanitized under the explicit AI-D07 correction. Do not accidentally redact or replace the user's saved message just because provider input is sanitized.
- Source chat retry builds context up to the replaced answer (including the preceding user question), then sends that question again; it replaces the visible assistant message but saves another pair. Preserve this request/history and append-pair outcome initially; don't silently “deduplicate” history or delete later turns. The new generation is metered as a new successful answer. Source failed-stream UI and EOF-as-success are not persistence/charging proofs; show interrupted drafts explicitly, with no completed pair for an unsuccessful stream.
- A final persistence failure leaves already-delivered content visible with `persistence=unsaved` and a sync warning, never a false saved checkmark. Retry-saving uses the same generation and settlement id. If storage remains unavailable and the process dies, that unsaved tail may be lost; do not promise durable recovery of bytes never committed. Preserve the reservation until recovery fences/classifies that generation, then settle once or release it as locally interrupted.

## 5. Parser, sanitation and resource behavior

### Upstream SSE

Decode UTF-8 incrementally across arbitrary byte boundaries; accept LF/CRLF and SSE comments/keepalives, concatenate multiple data lines within an event and parse only at blank-line dispatch. Handle metadata-only chunks, empty deltas, optional usage and `[DONE]`. Do not confuse an HTTP chunk with a token or an SSE record. A partial JSON data event, invalid UTF-8, unexpected choice/tool content, malformed completion shape, transport EOF without terminal evidence or `finish_reason=length` is an explicit interrupted/protocol outcome, not skipped content followed by success. Keep any valid prior partial sections. A provider refusal does not silently switch models/providers.

### Analysis documents

Keep source prompts and full speaker/line input (analysis newline join and fallback speaker differ from chat's space join). Omit JSON-object response mode for this path only. Stateful scanner tracks JSON quoting/escape/nesting; a line containing exactly `---` outside a string separates documents. `---` inside quoted content is literal text. Ignore whitespace-only documents. Accept the first count object or the restricted legacy `documentCount: <nonnegative integer>` header; do not parse YAML sections, aliases or tags. Header count is advisory, not evidence all content arrived.

Accept only one detected type from the six and finite confidence 0..100, then summary and relevant type-specific sections. Extract payload with the source `data/items/content` precedence; preserve accepted empty collections. Validate complete sections against the source field types, optionality, enums, interview 1..5 bounds and defaults (meeting status defaults pending). Do not infer missing content, synthesize other analysis types or silently drop malformed nonempty documents. Duplicate/conflicting type/section documents are a protocol error; retain already valid partial output. Process a final complete JSON document without a trailing delimiter. A lone final detectedType must not become empty success.

Preserve source `sectionCount = max(0, documentCount - 1)` as an advisory progress hint, and source progress shape for existing clients. The prompt says count includes its own header, so this hint can overcount actual content sections; it must never decide terminal success. Completed means transport terminal evidence, valid detected type and nonempty valid content, not a guessed section-count match. A count mismatch is visible metadata, not fabricated extra sections. Summary length remains a prompt instruction, not a new hard output rejection/truncation rule.

### Text policy and bounds

Port sanitation literally with fixtures, including HTML/script removal differences, SQL word replacement, whitespace normalization and card/SSN/email/phone/key-like redactions. Match JavaScript UTF-16 code-unit lengths, not Rust byte length or Unicode scalar count, for route/content limits and estimated tokens. Estimate provider-reported usage separately; do not substitute it into commercial counters. Analysis metadata estimate uses the **actual submitted canonical analysis text**, correcting AI-D03, with source round-to-nearest behavior explicitly identified as estimated input, not chat billing.

Raw chat deltas are provisional **plain text**, never HTML/Markdown execution; final sanitized content replaces them and is what persistence/settlement uses. No per-token trim/scrub that breaks spacing or misses cross-token PII. This preserves source transient exposure, not a zero-PII promise. Analysis remains path-specific raw input rather than acquiring an unapproved new redaction rule.

Recommended explicit operational guardrails (new resource-safety limits, not source business caps): 16 MiB encoded completion input including full history, 4 MiB total output/event payload per generation, 256 KiB maximum SSE record/document parser buffer, 64 nested JSON levels, per-subscriber queue bounded by 128 events AND 256 KiB, replay pages at most 64 KiB. Reject oversize input before provider dispatch; terminate oversize provider output as interrupted; never silently truncate a transcript or history. Keep counters independent from these byte bounds. These defaults require an explicit lead decision because source history/analysis input is otherwise unbounded; do not disguise them as `AI_MAX_CHAT_HISTORY` enforcement.

Use one bounded producer/consumer path, one HTTP reader, and bounded persistence batches, not detached per-token tasks. Slow subscriber loses its attachment and resumes from durable snapshot; it cannot stall every subscriber or create an unbounded queue. Database backpressure pauses upstream reading, bounded by the request deadline; a DB outage eventually interrupts locally rather than buffering indefinitely. Persisted terminal history is paged, not copied into every view update.

Enforce a real configured total request deadline (recommend 60 seconds), cancellation and body limits; those were not active source guarantees. No automatic provider retry, even for 429 or a nominal three-retry setting. Preserve retry-after as advice; explicit user retry is a new attempt. Abort/timeout means local reading stopped, not that remote generation stopped or retained data disappeared. Provider errors and bodies are mapped/redacted; never log transcript/prompt/token bodies, keys, auth headers or raw provider diagnostic payloads.

## 6. Quota: serialize unknown answer spend, do not invent an allowance

Recommendation: one **locally active metered chat generation per account** when any cap is enabled. This is a persisted active marker/reservation, not a database lock held across HTTP. It prevents concurrent calls from all spending the same remaining unknown answer allowance while preserving single-request admission and post-success settlement exactly. Other simultaneous metered chats receive 409 `chat_in_progress` and may retry after the first locally terminates; no hidden queue or provider retry. Paid users bypass the quota subsystem entirely; they do not receive this metering-only restriction. Ordinary per-generation idempotency/fencing still applies to everyone.

In account-first/job-then-counter order, derive the billing facet, initialize/reset applicable counters, check in source precedence (transcript questions, user questions, transcript tokens, user tokens), and reserve 1 question plus estimated sanitized question tokens. Do not reserve the 1000-token model output cap: model tokens and source character estimates are not the same unit. Reservation participates in admission, is not publicly reported as settled usage, and is released on definite pre-provider refusal or locally unsuccessful termination.

On successful terminal answer, one transaction replaces reservation with actual +1 and `ceil(UTF16_len(sanitized_question+sanitized_answer)/4)` on enabled counters, stores the pair and generation terminal state, and clears the active marker. Every effect is idempotent by generation; double terminal delivery cannot spend twice. Paid-at-admission remains unmetered even if a package expires during the stream. Free-at-admission remains metered if a package is bought during the stream. This explicitly freezes a request's entitlement snapshot; subsequent admissions read fresh facts.

| Concrete boundary | Captured sequential outcome | Recommended outcome/difference |
|---|---|---|
| Transcript question count 49, max 50, two concurrent sends | Both can pass and finish at 51. | First reserves; second busy. First success settles 50; next send refuses. Explicit race correction. |
| Token count 19999, max 20000; question length 4 and sanitized answer length 8 | Admission allows 1; settlement adds 3, ending 20002. | Same 20002, then further positive-cost questions refuse. No hard output truncation or invented allowance. |
| Token count 20000, question estimate 1 | Rejects `current+1 > max`. | Same refusal before provider call. |
| Max questions exhausted but an unexpired paid package starts tomorrow | Source paid predicate ignores start; no checks or increments. | Same bypass, even with stored exhausted counters. |
| Plan lookup error; quota DB healthy | Treats freemium. | Same. Quota read failure is distinct: 503 before provider, not source fail-open. |
| Provider fails after some deltas | No successful-service settlement; browser can still treat an EOF as success. | No settled question/tokens; draft interrupted, reservation released after local worker fencing; remote provider cost remains unknown. |
| Reservation at 23:59, completion at 00:01 UTC | Source increment may land in whichever existing row/reset timing races win. | Settle original admission-period bucket; new day's counter remains independent. Explicit reset-race correction. |
| Completion/pair transaction fails after visible answer | Separate source increment/save can fail independently and be lost. | Unsaved warning; reservation remains, same-generation settlement retry only; no provider call. |

No claim that counters can never exceed token cap: source permits the admitted answer to overshoot, and retaining that rule is deliberate. Serialization bounds this to one locally successful admitted answer at a time. Strict no-overshoot would require a separately approved commercial change (output truncation or an answer allowance); neither is recommended.

Use UTC next-midnight, Monday week start, calendar-month boundary; transcript counters have no reset, user `transcript` period maps daily. Store period identity in reservation so stale settlement cannot decrement/reset today's unrelated row. New timed rows get a calculated reset date without reusing loaded nullable variables. Preserve stored SQL question max for imported rows; do not silently overwrite it with new environment caps. Token admission uses the current configured token max as in source. Record any imported question-max/environment mismatch for migration rather than claiming they were always equivalent.

Plan-query failure can become freemium only if the host transaction remains usable (isolate that query with the existing savepoint pattern). A lost connection or failed savepoint cannot continue as a fabricated healthy quota transaction: rollback and return unavailable before dispatch.

Reuse the established optional-cap parser, including source JS signed decimal-prefix parsing: absent/blank/unparseable means absent, zero is disabled, negative parsed values retain their source truthy/rejection semantics until a deliberate config correction. Do not strengthen config validation opportunistically. With all caps absent, freemium also has no quota reservations/serialization/increments.

Local cancellation/crash recovery fences worker writes before releasing local reservations. It does not settle a late answer as a success, and does not clear external-retention uncertainty. These are different records. A persistence-outcome-unknown generation is reconciled from committed generation/pair/settlement identity first; never assume “failed response” means the transaction rolled back.

## 7. Consent, revocation and account erasure

Keep public version 1.0, DeepSeek provider and captured privacy URL. Preserve recorded imported metadata, including different historical versions/paths; do not rewrite it to make every grant appear current. Configuration declarations and `isConsentVersionCurrent` helpers do not prove active re-consent enforcement. No version-match requirement is added. Unifying analysis to true+not-revoked fixes only contradictory true-with-revoked rows and is recorded against AI-001/PRV-042.

Migration must assign existing derived rows to a known initial consent epoch (or an explicit legacy epoch covered by cleanup) without re-authorizing an absent/revoked grant. Null/missing epoch cannot become a cleanup exemption. Refusal followed by regrant must not make previously hidden retained artifacts accessible as newly consented output.

Every generation captures consent epoch after fresh admission checks. Every append/section/final/summary/pair/save-retry transaction reacquires the account lock, checks the permanent account-erasure barrier and current consent epoch, then locks job and generation. A stale token/session cannot supply valid consent, clear a barrier or upsert an erased owner. Read/attach/history/cache endpoints also check current account/consent, not just write endpoints.

Revocation uses two durable stages when cleanup must be retried:

1. Under account lock, set false+revoked timestamp and increment epoch; enqueue the exact local cleanup obligation atomically. Mark old generations revoked, invalidate worker fences and prevent further authorization to dispatch. Granting again increments epoch and cannot authorize the old generation.
2. Delete all old-epoch specialized analysis, general summaries, highlights, chat pairs, drafts/replay events and pending result bodies. A small local transaction can complete this immediately; if it fails after the fence committed, state remains `revoked, cleanup_pending`, with retryable durable work. Never report content deletion succeeded because a delete returned an error object. New-epoch content after a regrant must not be swept by a delayed old-epoch cleanup.

Keep chat counters through revoke/refusal and regrant, as captured; deleting them is an unapproved quota reset. Refusal POST(false) preserves source metadata behavior and immediately fences in-flight processing; DELETE revoke additionally owns content purge. A failed cleanup does not restore permission. No-row refusal/revoke must still create the false/epoch state necessary for a durable fence.

Account erasure joins P3's existing barrier/manifest/cascade, with billing's affected-owner discovery/ascending account locks and account-before-job ordering. Add all AI local content/consent/counters/generations and unsaved settlement bodies to that cascade before acknowledging local deletion. Preserve only minimal account/generation correlation, provider/request identity, timestamps and unresolved cleanup outcomes needed by the erasure saga, never prompt/question/answer/name/email in the surviving barrier. Confirm the P3 schema roles allow neither stale background intake nor client direct writes around this boundary.

Check authorization immediately before dispatch as well as at claim time. There is still a narrow check-to-network race: remote dispatch is not atomic with revocation. A durable dispatch intent lets revoke identify it; cancel locally and reject all subsequent content writes. Do not claim the request could not have reached the provider. A late provider identity may update the bounded privacy resource manifest even after local cascade, never reinsert its content. No callback/completion path can create AI content without its original fence.

LLM completion HTTP does not supply a demonstrated per-request erase API here. Report `local content deleted` separately from `provider retention unknown/unsupported`. Missing identity, interrupted stream or dropped socket is not confirmed deletion. Unresolved obligations stay runnable/pending or an explicitly approved documented retained/unsupported exception; no auto-approval or guessed timeout/grace. Taskr uses the same honesty. Provider records/backups and legal copy remain deployment evidence gaps, not blockers to exercising local correctness with fixtures.

Transcript retention differs: preserve completed AI artifacts under PRV-014, fence generations whose source lines were deleted, and refuse new chat/generation when no transcript remains. An extant saved analysis may still load. Ordinary soft deletion remains captured state-only behavior; hard job/account deletion includes derived content. These distinctions must be explicit in tests and UI state.

## 8. Separate general-summary completion consumer

Consume the existing `JobEvent::Completed` outbox event idempotently using completion identity plus content revision; do not infer a new transcription completion from an AI status. Fresh account/barrier/consent/feature checks precede the call; collect the prose stream, sanitize, save as general summary distinct from specialized `ai_analysis`. Disabled/no-consent/no-lines/provider-failure means an explicit absent/failed/skipped summary outcome, never job failure.

Expose a current saved general-summary read seam to result and customer-callback assembly; do not forward a draft or copy specialized analysis's summary into that field. P4 must exercise the real completion event-to-worker-to-save chain and failure isolation. Publish completion outcome so P7 can order callback assembly after summary attempt settlement when that source path expects it; do not rely on independent racing consumers or claim callback delivery is already verified. P4 implements the consumer/outcome/read boundary, P7 implements actual callback HTTP semantics without a new unapproved delivery policy. Duplicate outbox delivery must not rerun a successful or remotely uncertain attempt. No automatic provider retry is introduced by an outbox retry.

## 9. Minimal real taskr consumer and shared presentation

Add one operation: **summarize an owned completed audio-work transcript**. Existing `Work.transcript` supplies meaningful source content; do not expose an unauthenticated arbitrary-prompt echo endpoint. The sample host supplies a short summary prompt, calls the same `Llm` adapter and uses the same generation/event protocol/reducer for actual partial text, final replacement, error, detach/resume and persistence state. Save the summary in taskr's own schema with source revision and enroll it/drafts in its existing account erasure hook. Use existing taskr sessions/ownership and barrier. No Transgate dependency, six-shape policy clone or sample commercial AI quota system.

Keep taskr-core unchanged: ordinary tasks remain no-I/O/no-dependencies and unmetered. Put the example consumer/presenter beside audio-work; `llm-core` contains the reusable pure reducer, not a taskr business dependency. Requesting the sample summary explicitly authorizes that sample operation; document external processing and retain account-erasure coverage, without pretending the sample's operation is Transgate's consent policy.

Expose typed `StreamIntent`, `StreamView`, `StreamEvent` and generation/cursor through existing wasm/UniFFI wrapper conventions. wasm dispatch invokes listeners on meaningful partial changes, not just before/after one final Promise. UniFFI listener emits off the main thread; release reducer mutex before callbacks, marshal UI updates in the shell, and detach safely on suspend/drop. No provider key in wasm/native app; both stream from the authenticated host. Session loss clears view/attachment and prevents late events from another user/job updating it.

Provide a runnable taskr CLI consumer (`summarize-work <id>`, reconnect by generation) as the minimal actual P4 surface. Real loopback upstream sends at least two content chunks with a barrier proving the first view arrives before completion. Exercise the same sequence/final-replacement fixtures through wasm and UniFFI presenters, including native suspension and restart attachment; do not count DTO conversion or token-echo tests as scenario 10. Full Transgate web/native screens remain P5/P8, not these shared transport/presenter guarantees.

## 10. E4: compare a real captured fetch shape, not invented live runs

Two lanes, with identical synthetic transcript/history fixtures, actual source prompts, provider/model and supplied response bytes:

1. Capture source request construction from the **current fetch path**, including exact messages, endpoint, headers with fake credentials, flags and response mode. A throwaway recording harness runs it only against loopback; do not needlessly install an SDK or call availability endpoints at a real provider.
2. Run Rust plain HTTP adapter against the same loopback responses. Compare normalized outbound bodies/message ordering, concatenated prose, six typed section fixtures, errors, finish/usage handling and failure classification. Fix time/ids in captures; never normalize away prompts/models/content or final sanitation differences.

Mark every fixture's provenance: source file+line/content fingerprint and timestamp; synthetic provider transcript versus an actual authorized recording; expected semantic result and intentional delta. Include English, Turkish, a non-Latin/RTL case, emoji accounting, delimiter text inside a JSON string, split UTF-8/SSE records, final no-newline chunk, empty JSON mode response, length finish, malformed section and provider 429/401/5xx.

Run baseline lane with original `json_object` request and candidate transport with the **same request** first. Separately compare the proposed framing correction (omitted response mode) as a named defect experiment; do not hide that request difference in E4 output-parity numbers. Replay fixtures cannot establish whether that change affects real model quality, six-shape adherence or acceptance. Keep any revised prompt/model experiment separate rather than contaminate the same-prompts requirement.

Count actual direct/transitive dependency footprint for each executed path after implementation; distinguish Axios availability/dead imports from dependencies actually used by streaming. Fewer dependencies wins only after fixture output parity plus declared defect outcomes. This proposal recommends plain HTTP, not an unmeasured verdict or an installed-SDK comparison. Live availability/model-alias behavior, latency, cost, retention and multilingual generation quality remain named authorized acceptance gaps.

## 11. Acceptance and dispatch

Implementation should start with domain rule regressions, then one integrated verification run after all slices are source-ready. No helper/default/constant/mock-echo tests as business proof. Use isolated PostgreSQL databases for each runtime suite, stable per-test clocks and unique request/provider identities across reruns. A global worker must never consume another test's fixtures. Evidence records exact command, executable revision, environment without secrets, observations and rule ids; no green claim from this proposal.

| Scenario | Required observed behavior and plausible failing-before defect |
|---|---|
| Admission matrix on actual server | Missing/foreign account/job, contradictory revoked row, empty transcript, flags, input/history security and source size boundaries map correctly; no upstream request on refusal. |
| Six shapes over real local streaming HTTP | Type and validated section become visible/persisted before terminal; reload matches each typed field/default, summary and generic highlights. Chunked parser must not split a quoted `---`. |
| Final sanitation | Cross-token email/HTML and spacing are visible only as safe provisional text; done replaces draft with exact sanitized persisted final, which also determines charged estimate. Old route omitted that content. |
| Two processes, one last metered slot | One admitted, one busy; after settlement second gets quota error; one pair/counter increment. Baseline check/increment permits both. Include same account different jobs. |
| Paid bypass and lookup failures | Exhausted free counters do not block paid; inspect counters unchanged after a real answer; future-start paid bypasses. Plan error is free; quota-store error is unavailable with zero upstream calls. |
| Reset/settlement | First counter has reset deadline; equality at day/week/month boundary resets; old-period completion cannot reset/spend new bucket; DB commit-success/response-loss settles once. |
| Disconnect/restart/idempotency | Drop stream after partial, reattach on second replica and retrieve correct snapshot/final with one provider request; crash after dispatch yields interrupted/uncertain, never automatic duplicate. |
| Cache/edit/retry | Legacy empty completed misses; five-minute boundaries use createdAt; line edit remains LWW but invalidates cache/fences old stream; duplicate retry id generates once; new retry clears old analysis. |
| Persistence failure | Inject real PG failure after partial/answer; view retains answer with unsaved warning, no false durable completion; restore DB and retry save by generation without provider duplication. |
| Revoke/regrant during blocked upstream | Revoke committed before final; release upstream and prove no old content/pair/counters resurrect. Delayed cleanup cannot erase newly granted epoch content. Failed deletes remain pending. |
| Account erasure/stale credentials | Existing token, reopened stream, late worker identity and settlement after cascade cannot recreate content; minimal unresolved external resource state remains honest. No inversion versus billing/job locks. |
| Retention distinction | Lines removed by actual sweep; completed AI artifact retained, active old generation fenced, new chat refused without context; revoke/account erase subsequently removes retained derived artifact. |
| Completion event and mail isolation | Actual completion worker saves general summary once or records absent outcome; transcription stays completed and existing failure/partial-error mail behavior unchanged. P7 reads this result, not a racing draft. |
| Resource pressure | Slow subscriber, provider flood/large frame, missing terminal, database stall, deadline and shutdown give bounded memory and interrupted state; no “EOF means success”. |
| Taskr process and binding consumers | Actual owned audio-work summary produces partial view before final, save/reload and reconnect; same behavior through wasm and UniFFI listener paths, not merely matching DTO strings. |

Suggested implementation ownership, after lead reconciles proposals: shared transport/reducer plus E4; Transgate pure schemas/prompts/quota/presentation; persistence+privacy attach; both host compositions/taskr wrappers; lead integration, canonical ledgers and actual-process acceptance. Shared prerequisite is frozen public interfaces and write ownership, not copying whole proposals into parallel agents. Existing P3 jobs/erasure files need one integration writer, not simultaneous AI and repair writers.

## 12. Material decisions for the lead

Recommendations under the user's delegation, not extra approval ceremonies:

1. **Accept minimal framing correction:** preserve prompts and multi-JSON documents; remove incompatible JSON-object mode from analysis only. Retain only real legacy count fallback. Record AI-D01 and robust-parser changes separately from same-request E4.
2. **Accept serialized metered chat with source overshoot:** reserve question-only, one metered generation/account, successful exact source settlement, original admission period. No answer allowance, hard output clipping, automatic provider retry or new paid restriction.
3. **Accept quota failure correction:** fail unavailable before provider if admission cannot be durably determined; keep visible unsaved answer and idempotent settlement if persistence later fails. This changes AI-019 deliberately.
4. **Accept safety fences:** strict user/assistant history, authoritative sanitized final, content revision/generation/worker/consent epochs, account erasure integration, and true+not-revoked for inconsistent analysis consent rows. Preserve version 1.0 validity regardless of current-version equality.
5. **Preserve actual language/sanitation/commercial outcomes:** no new analysis/general-summary language instruction, history truncation, universal redaction, quota deletion on revoke or changed billing facet. These are not prerequisites to correcting the recorded defects.
6. **Accept explicit runtime bounds/deadline:** proposed byte/depth/queue bounds and real 60-second deadline are resource corrections, not source-enforced defaults. Any chosen alternatives must be written before implementation, not hidden in adapters.
7. **Keep provider truth explicit:** source DeepSeek model and flags remain; current docs/model drift and external retention are named evidence gaps. No synthetic fixture establishes live model acceptance, zero retention, guaranteed cancel or E4 quality parity.
8. **Record voice exclusion explicitly:** AI-027 excluded under PRD assumption in final gate/ledger, not removed or marked exercised. P5/P7/P8 rendered/public/native evidence remains outstanding by name; no backend clause is deferred to a screen phase.

Independent handoff: lead performs critique dispatch; SOL reads the sibling proposal only after that instruction. No implementation or canonical ledger changes are authorized by this advisory document.
