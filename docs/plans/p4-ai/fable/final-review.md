# P4 final acceptance review (Fable)

Reviewer `w23:p27`, anthropic/claude-fable-5-1, MEDIUM reasoning, 2026-10-25. Independent of implementers. Read: `decisions.md`, `implementation-contract.md` (including the lead execution checkpoint), executed-evidence sections of `implementation/{e4,hosts,shells,taskr-summary}.md`, and the run5 section of `docs/plans/p3-transcription/opus/runtime-integration-evidence.md:171-180`. Two targeted source greps. Lead observations stated in the dispatch (strict Clippy PASS, offline 805/70/221 ignored, PG artifact787 207/20, E4 81 PASS `run-dd8Tk7`) are taken as ground truth and supersede stale prose. No builds, tests or edits.

## Verdict

**Not phase-wide green yet; no design obligation is unclosed.** The D1–D10 contract is implemented and evidenced end to end on synthetic/local surfaces, and the written evidence does not overclaim live or native acceptance. One coherence gap blocks green: the actual-process and binding proofs (run5, browser 4/4, Linux Swift) were produced on binaries that predate the parser-`finish` ownership / private-error / Clippy cleanup, and nothing executed since then exercises a real Transgate process or a real taskr shell. The post-cleanup reruns are unit/PG/E4 only. Closure is one bounded rerun, not new work.

## Findings

| # | Severity | Finding | Evidence | Needed closure |
|---|---|---|---|---|
| 1 | **Blocking** | Actual-process and binding evidence is stale relative to the accepted source | Contract checkpoint rows "Actual Transgate process" and "Taskr client/bindings/work" cite immutable `f1ca960c`/`10f9e603` and taskr `2f2a5fea…`; both rows state later cleanup is not verified by those binaries. The cleanup touched `transgate-core` parser API (linked into `transgate-server`) and taskr-work/llm error/checkpoint signatures (linked into wasm/ffi). E4 was rebuilt and rerun (`run-dd8Tk7`); the process and shells were not | Rebuild immutable `transgate-server`/`billing-provision` and taskr wasm/ffi from the post-cleanup tree; rerun run5 (unchanged harness, same scenario set) and the shell harness (`playwright.p4.config.ts` 4/4, `uniffi.sh` Swift listener). Record new hashes. A smaller scope is acceptable only if the lead records a file-level diff since each snapshot proving no linked crate changed, which the dispatch's own description of the cleanup makes unlikely |
| 2 | Medium | Ignored-test accounting must name the contract's required PG races as executed post-cleanup | Offline run reports 221 ignored. Contract §Acceptance requires two-process admission/idempotency/period/unknown-COMMIT races; the regression exists (`transgate/jobs-postgres/tests/ai.rs:274 after_an_unknown_commit_the_pair_and_settlement_are_found_and_never_written_twice`, `#[ignore]` without DB URL) | Cite artifact787's suite list showing `jobs-postgres` AI, `transgate-billing --test runtime ai::`, `transgate-server workers::pg_tests` and `taskr-work summary::` all ran with real PG after cleanup. If any is missing from the 20 suites, run it |
| 3 | Low | Stale handoff prose | `e4.md:7` still names `run-odHWCQ` as the executed artifact; `taskr-summary.md:62` says browser/UniFFI runs are still pending although `shells.md:12-13` records them | Lead refreshes pointers when recording final evidence; not a correctness issue |
| 4 | Low | D6 checker recognition not independently inspected | Contract checkpoint: canonical check passes with 526 rules; I did not read the checker | Lead confirms the checker names exactly AI-027/API-F44 and no generic skip rule |
| 5 | Medium | **Correction (lead-sourced, 2026-10-25):** finding 2 named `after_an_unknown_commit_the_pair_and_settlement_are_found_and_never_written_twice` as the unknown-COMMIT race regression on the strength of its name. Lead read `tests/ai.rs:274-334`: the test commits with `unwrap`, then manually rereads and attempts duplicate writes; it makes no `Ai` call and never returns a failed/uncertain COMMIT to the consumer. It proves real-PG adapter durability/uniqueness, not D2 consumer handling of a lost COMMIT (reconcile before retry/release, no provider replay, no double settlement) | My grep matched the function name only; I did not read the body. The artifact787 execution citation for the suite stays valid; the whole D2 consumer proof must not be inferred from it | R's narrow regression (real `Ai` over `PgJobStore`, post-commit uncertain answer, no replay, no double settlement) is source-only pending Y execution; not executed, not green. Finding 1's rerun and this regression's executed pass together close D2 evidence |

Checked and found consistent with decisions, no action: paid bypass regression (`transgate/core/src/ai/app/tests.rs:250`); D5 original/sanitized question and summary→callback ordering (`transgate/server/src/workers/pg_tests/ai.rs`, run5); D4 watch caps and idle-disconnect readmission (billing runtime `ai.rs`, run5 within 5 s); D9 dispatch inventory and D15 operator-approved `erased_with_exceptions` never `fully_erased` (run5); D10 cancel/lease/retention/remote-id repairs (taskr-summary RED→PASS list, 24 targeted checks); E4 baseline-versus-D1/D8 separation (81 cases, rebuilt).

## Overclaim check

None found in the texts read. `shells.md:15` correctly limits the Swift proof to Linux; the checkpoint labels every row as synthetic/loopback; run5 is labeled as covering only the listed scenarios with fixture failures `[]`. Counts are presented as non-additive. The dispatch's own caveat about pre-cleanup snapshots is the only place an overclaim could arise and is the subject of finding 1.

## Explicit dependencies, not P4 defects

- **P5:** render the cumulative `snapshot` replacement event and analysis section events; no raw-append consumer.
- **P7:** outbound callback delivery; `summary_outcome` ordering seam is in place.
- **P9:** imported null-reset quota history (D2); historical dispatch inventory that the new ledger cannot establish (D9); migration/cutover.
- **Live:** DeepSeek model compatibility and output quality with D1 live framing (no `json_object` for multi-document analysis); OpenAI as explicit alternate only; provider retention/region/cost; remote deletion remains unresolved by design (D9); real SMTP; iOS/SwiftUI/Android/device shells (P8).
- **Operator:** D15 exception approval is per-erasure human action; no policy pre-approval.

## Decision

Close P4 as green only after finding 1's rerun (and finding 2's citation) is recorded. Everything else is complete clause evidence under the contract's own scoping.

## Lead execution disposition, 2026-10-26

Finding5's consumer evidence gap is closed by independent owner Y's targeted execution of `ai_completes_once_when_the_completing_commit_answer_is_lost`:1 passed,0 failed,1.51s on an owned disposable PostgreSQL17 cluster. The case exercises both really-committed and rolled-back completion transactions whose answers are reported unavailable to the real `Ai` consumer. Lead read `/tmp/rustr-p5-d2-lostcommit/run1/test.log`; compile/source hashes and database logs are retained alongside it. This is new execution evidence, not a reinterpretation of artifact787 or a reviewer-executed run. Finding1's fresh immutable process/browser/Linux-Swift proof remains open; P4 is not declared green.
