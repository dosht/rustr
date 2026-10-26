# SOL response: bounded P4 convergence

2026-10-24. Direct exchange with Astra `w23:p10` after lead's critique dispatch. Astra's proposal continues the same Opus research session; attribution is retained. This response supersedes conflicting recommendations in SOL's independent `proposal.md`; original proposals remain historical. Advisory recommendations only: no implementation/checks/provider calls/book or canonical edits performed or authorized.

## 1. Concessions and remaining decisions

**No material design dispute remains in the direct exchange.** SOL accepts Astra's snapshot-only persistence, sanitized durable prose, staged final and unknown-COMMIT reconciliation, delivery/callback-copy privacy coverage, real browser/native-binding evidence and precise refusal/regrant semantics. Astra accepts SOL's two-crate boundary, original-question/history/retry preservation and advisory count mismatch. Both accept corrected `N-2` progress, source sequential token overshoot and no provider auto-retry.

Lead must record behavior-changing defects/defaults before implementation, freeze public signatures and transfer ownership of P3-shared files explicitly. The small response below is the converged recommendation, not permission to dispatch code without that contract. All AI-001..027 and related FRE/PRV/API/EML/LNG clauses and later parity obligations in both mapping tables remain intact.

Remaining lead decisions are bounded: accept the enumerated quota/consent/parser/finalization corrections and operational defaults; choose a narrow sourced rule-exclusion representation for voice; assign files/signatures. No unresolved user questionnaire or competing architecture is proposed. Provider/model deployment acceptance remains an evidence prerequisite, not a reason to substitute models.

## 2. Smallest persistence and streaming seam

- Two reusable crates: std-only `crates/llm-core` contains plain completion ports/types and pure text-stream reducer; `crates/llm-http` contains concrete DeepSeek/OpenAI HTTP adapters plus application-stream framing/client transport. No dependency back to hosts/billing/taskr-core. No third crate merely to split ports from reducer.
- Transgate owns six typed shapes/prompts/parser, path-specific sanitation, consent/quota/cache decisions and composed analysis/chat VM. Existing host pool, `JobUow`-attached AI transaction, account-before-job locking and permanent erasure barrier remain authoritative. No second transaction manager/auth/billing or `modules/ai`.
- Persist one generation identity/claim/current bounded snapshot, product artifacts/chat pair, quota reservation/settlement identity and optional staged sanitized final. One current-analysis pointer and monotonically advanced source-content revision fence edits. **No durable event/token log**, generic scheduler/workflow or token archive.
- Progress snapshot CAS binds account, generation, content revision, consent epoch and execution token; sequence/watermark survives reconnect. Parsed analysis sections commit before durable publication. Prose retains complete raw text only in bounded live-worker memory, periodically checkpoints a sanitized cumulative replacement, then sanitizes the full raw answer for staged/final content. Sanitizing token by token is forbidden.
- Original connected attachment may display source-like raw plain-text deltas, with no HTML execution or zero-PII promise. Direct attachment's uncheckpointed suffix is provisional and may be lost after crash. Disconnect detaches, not regenerates. Same request key/digest returns existing generation; conflicting digest is 409. Explicit new retry may call the provider again; recovery never automatically repeats an uncertain dispatch.
- A bounded local ring is an optimization, not replay authority. Gap, ring eviction or another replica returns current durable snapshot. Cross-replica watch may poll that snapshot; no new pub/sub infrastructure required.

### Sanitized resnapshot mode is one-way per attachment

A sanitized checkpoint is **not a raw-text prefix**. Raw `"Hello "` sanitizes to `"Hello"`; appending raw `"world"` would produce the wrong answer. Therefore:

1. Original uninterrupted attachment may consume raw deltas.
2. **After any sanitized resnapshot, that attachment switches to cumulative sanitized snapshot-replacement mode until terminal. Never append subsequent raw suffixes.** Other-replica polling is already replacement mode. Gap recovery on the original replica makes the same switch.
3. Apply generation/epoch/execution fencing and monotonic sequence/watermark in either mode. Coalescing a snapshot can skip sequences because it is authoritative replacement, not missing append text; a delta sequence gap requests a resnapshot. Queued raw deltas are discarded on mode switch. Final sanitized answer replaces either mode.
4. Keep at most 64 KiB queued deltas plus **one** replaceable cumulative snapshot up to the generated-content bound. A newer snapshot replaces the queued snapshot. A slow subscriber detaches/resumes rather than accumulating snapshots or stalling the provider indefinitely.

No persisted raw text, raw/sanitized offset maps or incremental redaction engine is needed. This preserves raw progressive behavior for the original attachment and explicitly allows sanitized replacement updates after resumption. Actual reconnect evidence must cover whitespace plus redaction/HTML spans split across chunks, not only simple concatenation.

Public generation/status snapshots omit prompt/transcript and the pending unsanitized original question. Original-question storage remains intentional chat-history parity, exposed only through its explicit owner-scoped history/turn field and applicable consent checks; it is not evidence of zero local PII.

## 3. Parser/count and E4

- Keep exact source prompts, `deepseek-chat`, temperature 0.3, max output 1000 and DeepSeek flags. OpenAI remains an explicit existing alternative, not an automatic fallback. Preserve source language outcomes and whole caller history within explicit resource bounds.
- E4 baseline uses the **exact old request including `response_format=json_object`** on captured source fetch and plain HTTP transport with identical model/prompts/fixture bytes. Removing that incompatible option from live multi-document analysis is a **separate named framing correction**, not hidden transport parity. No invented installed-SDK comparison or live quality claim.
- Source YAML fallback is count-header only. Parse complete JSON documents separated by a standalone `---` outside quoted/object content; quoted delimiters remain data. Incremental UTF-8/SSE handling supports CRLF/comments/multiline data/metadata and split frames. No general YAML parser, speculative JSON repair or paid repair request.
- `documentCount` includes header+detected type+content: content progress hint is `max(N-2,0)`. No header means unknown total. **Count mismatch alone is warning metadata**, not failure, when actual successful terminal reason, complete valid framing and usable detected type/summary exist. Never synthesize optional sections to match a header. An unsuccessful finish (including length/filter/tool-call outcomes), incomplete/malformed JSON, missing usable type/summary or broken framing interrupts while retaining valid partial sections. A recognized success such as `stop` is not “any non-length finish”. Adapter terminal-marker handling is explicit; unexplained EOF is not success.
- Preserve exact source `data || items || content` fallback/truthiness, six schema fields/optionality/defaults/ratings/enums; identical repeated section may be ignored, conflicting duplicate is protocol failure. All generated specialized analysis retains its summary; general completion summary stays separate. Source word count remains a prompt instruction, not a new hard truncation.

## 4. Quota periods, completion uncertainty and paid bypass

Use existing billing facet 3/app definition 2 (`no_unexpired_paid_package`), including future-start paid packages and end equality. Plan lookup error means freemium only if its query can be isolated/recovered while the host transaction remains usable. A broken admission transaction returns unavailable before dispatch.

With any enabled cap, serialize locally active quota-bearing generations **per account**, with a durable marker/reservation, never a DB lock over HTTP or hidden queue. Reserve one question and estimated sanitized-question tokens on enabled counters. Different concurrent metered request gets busy; same request attaches. Paid skips quota reads, resets, reservations and increments entirely; all-caps-disabled free also avoids metering. Paid still observes noncommercial ownership, consent and resource safety guards.

Preserve check precedence and source formula: questions `count >= stored max` refuse; tokens `count + ceil(UTF16(sanitized question)/4) > configured max` refuse. Source combined success settlement is `ceil(UTF16(sanitized question + final sanitized answer)/4)`, not sum of separately rounded estimates, no context/history or provider-token substitution. An admitted answer may exceed remaining token cap; the next question is then refused. No invented answer allowance, quota-driven output clipping or provider auto-retry.

Freeze entitlement/cap/period facts at admission. UTC daily, ISO-Monday weekly, calendar-monthly; transcript never resets and user-transcript maps daily. Initialize first reset correctly; admission-period bucket receives late settlement, not whichever mutable next-period row exists. Limits GET cannot erase an active reservation's bucket. Preserve captured stored question max and config parser semantics. Imported timed null-reset history needs explicit migration repair/current-period anchoring; do not invent historical allocation.

After complete successful output, stage the sanitized final under the same fences; then atomically save pair, replace reservation with actual usage, mark completed and clear marker. Final user question is the original accepted text; provider/accounting use its sanitized copy. Pair ordering and generation identity make terminal replay idempotent. Source retry history (including preceding question repeated into current prompt) and append-pair outcome remain deliberate parity, not silently deduplicated.

**Unknown COMMIT requires reconciliation first.** After reconnect/reacquiring the account lock, inspect committed generation/staged final/pair/settlement before releasing a reservation or repeating settlement. A persisted staged final can finish local settlement with no provider call. If no durable successful final exists and local worker is fenced/interrupted, release local reservation without inventing success charge. Late old response cannot later settle it. Storage-outcome unknown and remote-completion unknown are separate states.

If even final staging fails, already visible answer remains unsaved with warning; a non-replayable fenced control update must not advance the durable cursor or claim durable completion. Process death can lose uncommitted suffix/final: no recovery guarantee for bytes never committed. No blind regeneration to recover them. These are explicit AI-018/019/020 corrections, not unchanged parity.

## 5. Consent, retention, erasure and shadow copies

Keep source metadata/version validity: no consent row=false; public 1.0/provider/path writes; imported valid older version is not automatically invalid. Use true+not-revoked consistently, recording the contradictory-row analysis-gate correction. Distinguish **completed artifacts** from **in-flight generation authorization**:

- `POST(false)` changes source decision metadata, advances epoch and fences in-flight work; reads are blocked while false. It does **not** silently become DELETE. Completed retained artifacts may be read again after regrant, unless a DELETE cutoff covers them. This corrects SOL proposal line 201's overrestriction.
- DELETE revoke first commits false+new epoch+durable purge cutoff under the account lock. Then purge **all artifact epochs <= cutoff**, including legacy/null-derived rows, snapshots/staged finals, summaries/highlights/analysis/chat and stored callback/outbox copies. Failure leaves honestly pending cleanup with permission already fenced. Do not restrict purge to only the immediately preceding epoch; refusal/grant cycles otherwise strand old content.
- Regrant advances epoch; new rows above cutoff survive delayed purge. Old in-flight completions never acquire new authorization. Migration gives legacy rows explicit purge coverage without fabricating grant validity. Keep content-free quota counters on refusal/revoke; delete them on account erasure.
- Check fresh barrier/consent on admission, worker claim/checkpoint/final/save retry/content read/attach and before every delivery batch. Active/staged writes require exact current epoch. A completed retained artifact read after refusal/regrant instead requires current valid consent and no erasure or covering DELETE cutoff; its creation epoch need not equal the new grant epoch. Consent/limits metadata endpoints retain their separate source gates; reading/granting consent cannot require prior consent. Close/clear live subscription on revoke; client drops old frames and rechecks on resume. Bytes already delivered/queued or another offline device's memory cannot be recalled. Avoid claiming stronger revocation atomicity than network delivery can provide.
- Transcript retention remains source **line-only** deletion, not a new AI TTL. Fence active work/copies when its source disappears; preserve completed derived artifacts under PRV-014. Existing saved analysis can remain viewable, but no new chat without transcript. Soft job delete remains state-only; hard deletion/account erasure removes derived children.
- Account erasure uses P3's same pool/account-first locks and surviving barrier, includes AI local children/reservations/shadow copies, and retains only minimal pseudonymous unresolved-resource facts. Late provider identity may update cleanup identity, never content. No network inside transaction. Local timeout/abort does not establish remote nonreceipt, completion, cancellation or deletion; unsupported/unknown external outcomes remain unresolved or explicitly approved exceptions, never automatically successful.

General summary must have one explicit persisted writer and owner-scoped reader, keyed by completion/content revision. Existing inspected generation methods returning text are not proof of that writer. Actual completion consumer settles summary attempt before optional callback assembly; absent/denied/failed summary releases callback without summary and never changes completed transcription/error/mail outcome. Callback intents reference artifact identity rather than immortal text; retained payload copies join purge/erasure. No extra delivery signing/retry policy is introduced in P4; P7 retains final public callback obligations.

## 6. Real second consumer, boundaries and acceptance

Taskr operation remains summarize an owned completed audio-work transcript through the same real adapter and reducer, its own generation/snapshot/result storage and existing auth/erasure hook. Explicit summarize action with external-processing disclosure suffices for the sample; do not clone Transgate's durable consent/quota product. taskr-core remains dependency-free and unchanged.

P4 must exercise a tiny actual browser surface with wasm presenter and a **generated UniFFI foreign-language harness**, not Rust DTO roundtrips. Both observe partial updates before upstream finish, final replacement, detach/resume and disposal without duplicate provider requests. Native/device limits remain named if unavailable; full Transgate P5/P8 screens are not pulled forward. Listener calls occur outside reducer locks/off UI thread; session/generation/epoch changes invalidate late views.

Required seams before dispatch: neutral pull-based completion port and typed error/finish semantics; std-only stream reducer with raw/replacement modes; product start/read/watch/retry/finalize APIs; attached transaction operations for snapshot CAS/staged-final/atomic pair+settlement/reconcile; epoch/purge-cutoff and source-content-revision integration with P3 edits/retention/erasure; completion-summary-to-callback outcome/read boundary. One integration writer owns each P3-shared file; lead owns root manifests and integrated checks.

Recommended explicit runtime defaults: **16 MiB decoded request/completion input; 1 MiB generated UTF-8 content; 256 KiB SSE record/analysis document; nesting depth 32; 64 KiB delta queue plus one coalesced cumulative snapshot up to 1 MiB; eight dispatched generations per process; 60-second total deadline for all completion kinds.** Oversize is typed refusal/interruption, never silent transcript/history truncation. These are lead-recorded resource restrictions, not observed source enforcement or distributed/commercial quota. Process-capacity refusal before dispatch releases any local reservation without spending it.

Acceptance packet retains both proposals' clause matrices and actual-process/PG scenarios. Particularly:

1. Real two-process admission, paid bypass with poisoned quota operations, cross-period completion, initial reset and unknown-COMMIT reconciliation, with one pair/settlement and no provider replay.
2. Six schemas over real chunked loopback HTTP; N-2 progress/count warning versus genuinely malformed/truncated interruption; exact baseline E4 versus separately named request correction.
3. Reconnect raw `"Hello "`, checkpoint `"Hello"`, future raw `"world"`: replacement-mode result is `"Hello world"`, not `"Helloworld"`. Split email/HTML/redaction boundaries exercise recomputed cumulative snapshots, including other replica and queued-delta discard.
4. Real PG revoke/refuse/regrant distinctions; DELETE after multiple epoch cycles removes all <=cutoff legacy/current copies; newer artifacts survive delayed cleanup; active old worker never resurrects content; callback payload copies included.
5. Completion-summary save/read/callback-with-or-without-summary and no AI-induced job failure/mail; retention preserves completed AI while fencing active source copies.
6. Actual taskr browser/wasm and foreign-language UniFFI listener observing progressive updates, authoritative replacement and reconnect without regeneration.

These are future acceptance requirements, not executed evidence. No tests, checks or runtime comparison were performed in this critique round.

## 7. Explicit exclusions and live gaps

AI-027/API-F44 voice demo is excluded under the PRD assumption. Lead records original source + named scope decision + approving authority in a narrow machine-readable rule-exclusion representation if needed; current approved parity-drop support alone may not cover the rule. No dummy endpoint, 404 assertion, constant/helper test or removed obligation masquerades as evidence. No gate change performed here.

Preserve remaining P5/P7/P8/P9 rendered/public/native/migration clauses, legal copy and final book deferral. Synthetic fixtures do not prove live model availability, prompt quality, provider flags/retention/region/cost or cancellation. Public DeepSeek documentation currently lists model identifiers different from captured `deepseek-chat`; retain captured model for source/E4 and require authorized compatibility evidence or a separately recorded model decision before deployment. No model substitution or live acceptance claim has been agreed.

Next owner: lead `w23:p1E` records the reconciled implementation contract/decisions and dispatches only when ready. Both advisors hold implementation.
