# P4: Astra convergence response

2026-10-24. Astra continuation of the SAME Opus research session. This document records direct critique/convergence with SOL `w23:p21` after the lead dispatched cross-reading. It supersedes conflicting recommendations in `astra/proposal.md`; it does not rewrite the independent proposal or accept implementation evidence that does not exist. `astra/critique.md` records findings and source-backed concessions.

## Verdict and remaining decisions

**No material advisor design disagreement remains.** SOL explicitly accepted the count, snapshot, consent, quota-uncertainty, taskr/bindings, bounds and sanitized-resume resolutions below. This is a recommendation to the lead, not self-authorization to implement.

The lead still needs to record these exact decisions in its eventual contract:

1. **Approve named behavioral corrections:** omit analysis json_object mode while keeping prompts; N-2 advisory count; explicit malformed/unsuccessful completion; strict user/assistant roles; authoritative final sanitation; atomic server persistence and revision/epoch fences; consistent true-and-not-revoked consent gate; quota serialization/fail-closed storage/reset/settlement corrections. These are defects corrected, not a claim of byte-for-byte source parity.
2. **Approve the commercial concurrency choice:** one locally active quota-bearing generation/account, question-only reservation, source sequential answer overshoot, original admission-period settlement, entitlement frozen at admission. Paid skips checking AND incrementing; freemium with no enabled caps also skips the quota lane. No hard answer allowance or automatic provider retries.
3. **Approve explicit operational defaults:** 16 MiB decoded input including history; 1 MiB generated UTF-8 output; 256 KiB provider SSE/document buffer; JSON depth 32; 64 KiB subscriber delta queue plus at most one coalesced cumulative snapshot <=1 MiB; 8 active provider dispatches/process; 60s total request deadline for all generation kinds. No separate 30s summary default, source 60-RPM assertion or invented retry count. Reject, never silently truncate. These bounds are proposed safety restrictions, not source-enforced settings.
4. **Record narrow voice exclusion:** AI-027 and API-F44 remain inventoried with the PRD voice-demo exclusion and a source/decision-backed final gate disposition. Do not silently remove rows or make a general-purpose “skip rule” mechanism.
5. **Freeze integration ownership and evidence scope:** two shared crates and one host transaction attachment, P3 content-revision/barrier seam ownership, real taskr browser/wasm + UniFFI evidence, and P4 summary producer/outcome versus P7 outbound-delivery ownership. Preserve every named final clause and live/cutover gap. Names may be adjusted to existing conventions, but the responsibilities below must not shrink.

No further policy questionnaire is recommended. These are decisions already delegated to the lead. Provider availability/retention, imported counter history, rendered parity and deployment cutover remain evidence prerequisites, not unresolved advisor votes.

## Smallest complete contract

### 1. Two shared crates, no event engine

- `llm-core`: std-only completion request/message/error/usage types, pull stream port and pure text-stream reducer. No Transgate/taskr/billing dependencies, SQL, authentication, product prompts or generic JSON plugins. Define BoxFuture from std rather than importing a host or billing solely for an alias.
- `llm-http`: two concrete DeepSeek/OpenAI HTTP adapters and narrow native/wasm application-stream codecs. Browser/native callers connect to the authenticated host; provider credentials and upstream clients stay server-only. Shared reducer is usable through both bindings without making taskr-core depend on anything new.
- Transgate owns six schemas/prompts, parsing/sanitation, quota/consent/cache policy and typed analysis presentation. Existing jobs-postgres owns AI persistence on the same pool/transaction/account-lock domain as jobs/billing/privacy. Existing hosts own runtime tasks and session routes.
- One generation row/current snapshot, staged sanitized final and canonical result/pair storage are sufficient. No durable token/event log, workflow engine or second transaction manager. A bounded local ring is optional optimization, not reconnect correctness.

Concrete coordination seam: `Llm::open(request)`, `CompletionStream::next/cancel`, generation start/read/watch/cancel, and `JobUow::ai() -> AiTx` borrowing the SAME transaction. `AiTx` covers consent/epoch/cutoff, claim/idempotency, snapshot CAS, reservation/settlement, atomic pair/general-summary save and purge. Public fences include account, job content revision, consent epoch, generation and worker token. Compare/update operations never recreate missing owners/jobs. Same request key + different operation/payload is conflict, not replay success.

### 2. Parser, exact prompts and E4

Keep source model `deepseek-chat`, temperature 0.3, max_tokens 1000, request privacy flags, entire canonical transcript and route-specific language/sanitation. OpenAI remains the explicit alternate adapter, not fallback. Do not substitute a currently advertised model to make a fixture pass.

Preserve EXACT source prompts and generated schema documentation. Capture baseline fetch request including json_object and replay identical responses through the plain HTTP adapter for E4. Then test omitted analysis response mode as a named separate correction. No SDK comparison is invented, and synthetic responses do not prove live model acceptance, quality, multilingual output or retention.

Parser: JSON documents with an unquoted standalone `---` delimiter; restricted JSON or YAML count header only, no arbitrary YAML parser. Complete validated type/summary and relevant optional sections; no invented optional content or hard summary word cap. N counts header + detected type + content, so content hint is max(N-2,0). Mismatch alone warns. Ignore semantically identical duplicate sections; conflicting duplicates, malformed/incomplete nonempty documents or unusable result interrupt. Require a successful finish such as stop: length/content_filter/tool_calls are not success just because `[DONE]` appeared. Preserve valid partials on error.

### 3. Durable snapshots and sanitized reconnect

Original direct attachment may receive raw provisional text deltas with natural spacing. Persist only bounded sanitized provisional snapshots and the staged sanitized final, never a raw token log. Analysis persists typed sections. Final authoritative text replaces draft before the client displays durable success. Unsaved final may remain visible with a sync warning; no false saved status.

**After ANY sanitized resnapshot, that subscription switches one-way to cumulative sanitized snapshot REPLACEMENT until terminal.** Never append a raw suffix to a transformed checkpoint. Other replicas can poll current snapshots with monotonically increasing watermarks. No offset map or raw-prefix reconstruction is required. One coalesced snapshot plus bounded deltas is the entire queue allowance; detach a stalled subscriber. Same generation/sequence/epoch guards apply in both modes, and final replaces either.

Required boundary: original raw `Hello ` -> persisted sanitized `Hello` -> provider `world`; reconnect must show `Hello world`, not `Helloworld`. Also exercise split HTML and email redaction. Source `content-processor.ts:132-149` proves why raw-prefix append is invalid after trim/whitespace/redaction.

Disconnect detaches, not regenerates. Reattach/retry-lost-response uses the same generation/request identity. Explicit new retry is new work. Crash after dispatch means local interrupted/remote uncertain unless durable successful final can be reconciled; no automatic provider replay. Local timeout/abort does not prove remote cancellation.

### 4. Quota and commit uncertainty

Preserve source billing facet 3, check precedence, enabled-counter semantics, UTF-16 character estimates, question gate and question-only token admission. Settle +1 questions and ceil(UTF16(question+sanitized_answer)/4), not separate ceilings or model token counts. Context/history excluded. Reserve original period and frozen entitlement; reset at UTC boundary, with Monday weeks/calendar months and nonresetting transcript counters. Preserve stored imported question max; configured token max remains token gate.

One locally active metered generation/account serializes unknown answer spend without holding a database lock during provider HTTP. An admitted answer may overshoot sequential token cap exactly as source; the next admission observes that spend. No hidden queue or hard allowance. Plan lookup error means freemium only if its savepoint leaves the transaction usable. Actual quota-store failure refuses before dispatch.

Stage final sanitized result durably; atomically settle counters, pair and final generation state. If commit outcome is unknown, re-read generation/pair/settlement identity before either retrying settlement or releasing reservation. A lost database response is not rollback evidence. Staged final can be locally settled without another provider call. No durable final after recovery means classify/fence local interruption before release, not synthesize success from last delta. Local successful-chat charging and remote unknown provider cost are separate records; releasing local quota cannot clear provider uncertainty.

### 5. Refusal is not DELETE; write epochs are not artifact TTLs

POST(false) preserves source metadata behavior, increments epoch, blocks AI-content reads/dispatch and fences running work, but does not purge completed artifacts. A later grant may expose those retained completions unless a DELETE purge cutoff covers them. Active/staged writes require exact current epoch; retained completed artifact reads require current valid consent, ownership, no erasure and no covering DELETE cutoff, NOT artifact.creation_epoch equality with the latest grant epoch. No old in-flight response is reauthorized by regrant.

Consent and limits metadata APIs retain their own source authentication/ownership gates. Existing valid consent is not a prerequisite to reading/granting consent or reading limit metadata. The content-access predicate must not create a consent bootstrap deadlock.

DELETE first commits false + new epoch + a durable purge cutoff/obligation under account lock. Purge includes ALL artifact epochs <= cutoff, including migrated null/legacy epochs, not only the immediately prior epoch. Read/attach/cache must deny cutoff-covered rows even while deletion is pending or a new grant exists. Newer content survives delayed old cleanup. A deletion failure leaves revoked/cleanup-pending; it cannot restore permission or report successful cleanup.

Purge covers analysis, summaries, legacy highlights, chat, draft snapshots, staged finals and saved callback payload copies. Numeric quota counters survive refusal/revoke; they disappear on account erasure. Completed derived artifacts survive ordinary soft delete/transcript-only retention as captured; active work is fenced when source disappears. Hard job/account erasure deletes the corresponding children. Public generation snapshots omit the original unsanitized question except in its intentional owner-scoped chat-history field.

Every claim/write/final/settlement/save retry and delivery batch uses fresh account-first barrier/epoch checks; no network inside DB transaction. Regrant and old cleanup must race safely. Late provider identity may update minimal outstanding privacy-resource state after local cascade, never recreate content. External dispatch/retention uncertainty remains honestly pending or an explicitly approved exception, not auto-closed by timeout/flags.

### 6. Source chat behavior and general summary

Persist original accepted user question; provider input/accounting uses its separate sanitized version. Preserve source chat retry context through the preceding user question, resend that question, replace visible assistant answer, append another durable pair. Do not silently deduplicate caller history, delete subsequent turns or truncate to declared 50 messages. Retry-success is a new metered answer; replay of the same generation is not.

General summary remains separate, unmetered by chat, driven idempotently by existing completion identity/content revision. Explicit failed/disabled/absent outcome releases callback ordering without changing completed job status or triggering AI-induced EML-004/005 mail. P4 must prove actual completion-to-summary-save/outcome/read; P7 remains responsible for final outbound callback delivery semantics. Stored callback copies cannot bypass revoke/erasure. Independent racing consumers are not evidence of source summary-before-callback ordering.

### 7. Meaningful second consumer and acceptance

Taskr summarizes its owned completed audio-work transcript through the SAME adapter/stream reducer. Explicit action + processing disclosure, existing session/ownership/barrier and account erasure suffice; no clone of Transgate quota/consent policy. Save/reload/reconnect operates on real sample content. taskr-core remains unchanged and dependency-free.

P4 requires an actual tiny taskr browser/wasm surface and an actual UniFFI callback harness: partial view before provider finish, authoritative replacement, reconnect snapshot mode, cancel/suspend/session-loss and no repeated provider call. Rust DTO equality or CLI alone is insufficient. Release reducer locks before native listeners; shells own UI-thread dispatch, not business transitions.

Acceptance packet retains six schema cases, real PG two-process quota/idempotency/period/unknown-COMMIT races, real host stream/fault/privacy scenarios, bounded slow-client behavior, completion/mail isolation, exact-source E4 then changed framing, and actual binding consumers. Before/after claims must distinguish source counterexamples from executed reproductions. No verification was run for this advisory exchange.

## Source-backed concessions and preserved gaps

Astra withdraws fatal count mismatch, the third shared crate, copied taskr consent, separate summary timeout, and all-or-nothing revoke cleanup. It accepts SOL's original-question/retry semantics, savepoint treatment and unknown-COMMIT reconciliation. Sources: `analyze.ts:77-96,213-225`; `prompt-templates.ts:111-125`; `ChatMessageRepository.ts:31-58`; `useChatViewModel.ts:176-179,216-295`; `consent-service.ts:55-110`; `content-processor.ts:132-149`.

SOL accepts N-2 advisory count/identical duplicate handling, snapshots rather than durable events, staged sanitized final, callback-copy cleanup, actual wasm/UniFFI evidence, refusal/regrant versus DELETE cutoff precision and sanitized-resume replacement. These acceptances were received directly during discussion; original proposals remain historical inputs, not silently rewritten agreement.

Final ledger coverage is the UNION of both proposals, not their intersection: AI-001..027; FRE-014/024 and unchanged display/download boundaries; PRV consent/deletion/retention/history/legal clauses; LNG-030/031; FLG-009; API-F18..21/API-IDN-002/API-BIL-011, summary API-015..017/JOB-033/TRX-039; EML-004/005 and unchanged mail transport/template clauses. All AI-F01..10 and named WEB/phone/native parity rows remain required in their owning phases. Backend shared-state behavior is P4, not deferred rendered work.

Live provider/model/flag compatibility, actual model output quality/cost, retention/region guarantees, imported reset history, deployed prompt/config provenance, rendered Transgate surfaces and migration/callback cutover remain named gaps. No live or green-gate claim follows from agreement. Next: lead records the five decisions above and dispatches one concrete implementation contract; this advisor has made no implementation/check/book/canonical changes.
