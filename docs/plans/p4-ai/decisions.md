# P4 decisions

Lead decisions under mu's delegation, 2026-10-24. Inputs: the independent Sol proposal and the original Opus research session continued on Astra after Claude session-limit/403 responses; both direct convergence responses supersede their original disagreements. A credential defect is not established. No implementation or provider acceptance is implied. Book work remains deferred to the very end.

## D1 Correct framing, completion and ownership defects

Keep exact source prompts, six schemas, language behavior, `deepseek-chat`, temperature 0.3, max_tokens 1000 and captured privacy flags. E4 first compares the exact old fetch request, including `response_format=json_object`, against the plain HTTP adapter on recorded fixtures. Separately approve omitting that mode for live multi-document analysis. Do not substitute a model to conceal compatibility drift.

Approve N-2 advisory content counts; mismatch warns rather than inventing sections or rejecting otherwise complete successful output. Count-header YAML only, strict complete JSON documents thereafter. Identical sections may repeat; conflicting duplicates, malformed/incomplete documents, unusable type/summary and unsuccessful terminal reasons interrupt with valid partials retained. Validate caller roles as user/assistant. Final sanitized text authoritatively replaces provisional text. Move persistence to account-owned, revision/epoch-fenced server transactions; do not present an unsaved answer as durably complete.

## D2 Preserve sequential quota outcomes, serialize concurrency

One locally active quota-bearing generation per account, with no hidden queue and no database lock held during HTTP. Reserve one question and the sanitized question's UTF-16 estimate on enabled counters. Preserve source check precedence, stored question maxima, question-only admission and combined question-plus-final-answer rounding. An admitted answer may overshoot its token cap; the next admission observes the spend. No answer clipping or automatic provider retry.

Freeze entitlement and admission-period facts. Settle the original UTC day/ISO week/calendar month; transcript counters do not reset. Paid callers skip all quota checks, resets, reservations and increments; all-caps-disabled free callers avoid the quota lane too. Plan-query failure means freemium only if its savepoint preserves a usable transaction. Actual quota-store failure refuses before dispatch. Stage sanitized final, then atomically save pair/usage/completion. An unknown COMMIT is reconciled before settlement is retried or a reservation released; no durable successful final means fenced interruption, not invented success or provider replay. Imported null-reset history remains a migration prerequisite.

## D3 Refusal, revocation and retained artifacts are distinct

Use true-and-not-revoked consent consistently, retaining source version metadata semantics. Consent/limits metadata endpoints do not require prior AI consent. POST(false) advances the write epoch and blocks content access/dispatch without purging completed artifacts; regrant can expose them again. Active/staged writes require the exact current epoch. Completed reads require current valid consent, ownership, no erasure and no covering purge cutoff, not equality to the newest grant's epoch.

DELETE commits false, a new epoch and durable purge cutoff first. Purge every artifact at/below that cutoff, including legacy/null epochs, snapshots, staged finals and callback copies; delayed cleanup must spare newer content. Failure leaves revoked/cleanup-pending. Content-free quota survives refusal/revoke, but not account erasure. Preserve completed derived artifacts across source transcript-only retention and soft deletion while fencing active work when its source disappears. Hard deletion/erasure removes the appropriate children. Local abort/timeout never proves remote cancellation or deletion.

## D4 Bound memory and dispatch explicitly

Defaults: 16 MiB decoded input including history; 1 MiB generated UTF-8 output; 256 KiB SSE record/analysis document; JSON depth 32; 64 KiB queued subscriber deltas plus one replaceable cumulative snapshot of at most 1 MiB; eight dispatched generations per process; a 60-second wall-clock network deadline for every completion kind. These are explicit safety restrictions, not captured commercial limits. Refuse/interrupt rather than silently truncate. Capacity refusal before dispatch releases local reservation without spending it. Already-received buffered frames remain parseable after a slow consumer resumes; check the deadline before fetching new bytes and enforce it during the read. This does not authorize another request or extend network work.

The 1 MiB snapshot bound is decoded content: serialized structured analysis counts as that content. Provider SSE records and individual analysis documents remain 256 KiB. Application SSE has a separate JSON-escaping envelope ceiling of `6 * max(output_bytes, 256 + 4096) + 4096`, accommodating bounded control-error code/message fields even with a small output limit. Default output limits are unchanged; this does not authorize more generated/decoded content or multiple queued snapshots.

Long-lived watch attachments are capped at 32 per process and four per authenticated account, with immediate explicit capacity refusal and release on disconnect; no queue and no generation retry. Unchanged polling uses read-only state checks, not repeated billing/account/job write locks. A 250 ms fence cadence applies while waiting for provider headers or further content. These are local resource bounds, not commercial quotas; the existing global eight-dispatch ceiling remains and no new per-account generation quota is introduced.

Persist only sanitized cumulative checkpoints and staged final, not a raw token/event log. After any sanitized resnapshot, that attachment permanently uses cumulative sanitized replacements until terminal; never append a raw suffix to a transformed checkpoint. Discard queued deltas on that switch. Detach/resume stalled subscribers instead of accumulating snapshots. An uninterrupted attachment may show bounded raw provisional plain text; this is not a zero-PII guarantee.

## D5 Keep source chat and summary semantics

Persist the original accepted user question separately from its sanitized provider/accounting copy. Preserve caller history and retry's repeated preceding question plus appended durable pair; explicit retry is new metered work, generation replay is not. General completion summary is a separate unmetered artifact, idempotent by completion/content revision. P4 proves actual completion -> summary outcome/save -> owner read and the ordering seam before callback assembly. Absent, denied or failed summary releases callback ordering without changing completed job status or producing transcription-failure mail. P7 still owns outbound callback delivery. Saved callback copies participate in consent cleanup.

## D6 Exclude only the inventoried voice demo

Confirm the PRD section 9/R4 presumption that the voice demo is out of scope. AI-027 and API-F44 stay inventoried with their original source and this lead decision under delegated authority. The final checker may recognize exactly these named exclusions; no generic skip-rule mechanism, removed rows, dummy endpoint or constant/404 test. All other 526-rule obligations and parity rows keep their existing strict disposition requirements.

## D7 Use two shared crates and real binding consumers

`llm-core` is std-only ports/types/text-stream state; `llm-http` is concrete DeepSeek/OpenAI transport and application-stream codecs. No shared dependency on Transgate, billing or taskr-core; no AI module framework, token event log or second transaction manager. Product policy stays in Transgate and uses the existing host transaction/account-lock domain.

Taskr summarizes its own completed audio work with the same transport/reducer, explicit action/disclosure, existing auth and erasure, not a copied consent/quota product. P4 must run a tiny actual browser/wasm surface and a generated UniFFI foreign-language listener harness through partial-before-final, authoritative replacement, reconnect and cancellation/session loss without a duplicate provider call. Full Transgate rendering/native apps remain in their named phases. Provider/model availability, real retention/region/cost and imported/cutover history remain explicit live prerequisites.

E4 result: retain the captured providers through plain HTTP, with no additional SDK or remote gateway. The actual legacy path is direct fetch, not an SDK completion call. Lead executed 81 offline comparison/differential cases in `target/p4-e4/run-odHWCQ`: actual source requests versus the real Rust adapter, independent product prompt builders, and separately named D1/D8 corrections. Existing reqwest suffices; no new provider SDK dependency is justified. Synthetic recorded fixtures do not establish live model acceptance or output quality.

## D8 Resolve the source-content review explicitly

Approve one validation outcome change: a schema-invalid or inapplicable section, including all-falsy section data, interrupts with valid partials retained instead of silently filtering/hiding invalid items and reporting a complete saved analysis. Keep strict typed schemas; do not manufacture optional sections. This is an explicit difference from the old render-time filtering, not a claim of exact parity.

Do not add unrelated confidence or ordering restrictions. A usable detected type keeps finite numeric confidence without a new range check, and missing/non-numeric confidence defaults to zero as in the source. Valid sections may precede the type within the existing aggregate/document bounds. Recognize separator markers outside JSON strings/containers, including surrounding whitespace or an immediately adjacent next document; quoted marker text remains data.

Approve literal transcript substitution instead of JavaScript replacement-string expansion of `$$`, `$&`, `$'` and backtick-prefix directives. E4 records these named differences separately from exact baseline cases. Correct analysis `tokensUsed` metadata to the UTF-16 estimate of the actual submitted canonical transcript, rather than the old nonexistent-column estimate; analysis remains unmetered and chat quota arithmetic is unchanged. These are observable corrections, not representation-only changes.

## D9 Keep LLM dispatch facts through every local content deletion

A possible dispatch remains an external-retention obligation even after consent DELETE, hard job deletion or local account cascade. Register a payload-free durable intent before provider I/O, independent of generation/content lifetime; retain trusted provider/model, request identity and bounded observed provider identifiers, never prompts, questions or transcripts. Account erasure inventories those facts before deleting local rows. Late metadata or a known NotDispatched result reconciles under the surviving barrier without recreating content. Preserve distinct request/completion identifiers rather than overwriting one with another.

Bound provider identifiers to eight distinct values of at most 256 bytes each. Refuse unrepresentable metadata without inventing or truncating an identifier, and preserve the fact that dispatch evidence was observed even when its value cannot be retained; a later NotDispatched report cannot erase that fact.

Possible remote retention remains explicitly unresolved: privacy flags, local cancellation, elapsed time and absent local rows are not remote-deletion proof. Only a verified operator's explicit named approval and rationale may close this item as ErasedWithExceptions. No deployment ExceptionPolicy pre-approval for this LLM obligation, and no invented provider deletion API. A locally proven NotDispatched result can resolve its intent as absent. Historical requests whose inventory cannot be established remain a P9 prerequisite, not implicitly verified by the new ledger.

## D10 Define taskr cancellation and retained partials

Cancellation may win over a staged answer that has not become canonical; a committed Completed answer is preserved. The account transaction orders cancellation against finalization, so reconnect cannot revive a canceled stage. This does not weaken unknown-COMMIT reconciliation.

Transcript retention clears non-completed generation text and staged content while retaining content-free terminal reasons/provider facts. Canonical saved summaries remain readable under the existing owner/erasure rules. A first execution claim receives enough lease time for the declared provider deadline; a delayed replay must not purchase a completion that its remaining admission-time lease makes unsavable. Exact request replay precedes new-admission disclosure/configuration validation, after authentication/owner/barrier checks; conflicting payload reuse still fails.
