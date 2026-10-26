# PREP-1: lead preparation and continuation

This is the current lead checkpoint, not a replacement for the PRD, phase decisions or verification receipts.

## Authority and objective

Mu authorized preparation and a bounded continuation on 2026-10-26. P5 remains the current implementation phase. P2-P4 acceptance gaps remain open; P6 design is recorded but implementation is not dispatched. P7-P10 remain future scope. Book work is last.

PREP-1 replaces PAUSE-RETRO-1 only for the preparation below. Product execution stays held until the lead records preparation acceptance and releases candidate3 with a new batch token. Existing P5 C1/C2/C3 and live-provider/deployment gates are not implicitly authorized.

## Preparation owners

- Lead, w23:p2B (tab w23:tX): authority, checkpoint, session todos and evidence reconciliation. No product implementation. w23:p1E ("rustr-lead-old") is mu's backup only; never send it work.
- Existing OMP/Astra p22, w23:p22: explicit-loaded lead-only OMP reminder extension under scripts/lead-workflow/lead-guard.ts, its regression sources and docs/plans/lead-workflow-hook.md. No ambient/global installation.
- Existing OMP/Astra p1V, w23:p1V: separate notify files/tests under scripts/lead-workflow and docs/plans/lead-workflow-notifications.md. Deterministic Hermes Telegram delivery; no autonomous model loop or cron.
- Independent tester p1Y, w23:p1Y: validate both preparation slices, actual disposable-session smoke, one verified-recipient Telegram test, cleanup and receipts. Candidate3 execution remains held until release.
- All other original owners remain held. Native Claude p1R is read-only billing consultation; new billing repairs require an explicit OMP-writer assignment.

No new agents, worktrees, product edits or global settings changes are required for this preparation.

## Model and session policy

Preserve lead session /home/mu/.omp/agent/sessions/-src-rustr/2026-10-24T06-22-20-764Z_01a16c93-145c-7687-afe2-1878eb005742.jsonl and pane w23:p1E. Perform a focused in-place handoff, then use Opus medium for lead coordination, high for consequential unresolved questions. Verify the actual selected provider/model and effort; do not infer selection from a sent command. Fable is bounded review at medium only. OMP/Astra implementation sessions retain ownership and provenance. Native Codex/Astra and Claude/Opus remain independent advisors.

The lead currently remains Astra pending the transition. Prewalk and attached advisor were observed disabled. The configured inactive advisor role was Astra xhigh, so it must not be enabled blindly. Automatic model fallback has been disabled for this lead session only; persistent saving awaits mu's answer. No automatic provider hopping on quota limits. Preserve the existing Claude usage pane w23:p26 / w23:tV.

## Notification contract

Use the configured Hermes Telegram home recipient, verified before sending. Coalesce meaningful batch/status changes to about one delivery per hour, rather than per-agent messages. No unchanged/idle spam. Include a short result/blocker/next-action summary and observed Codex/Claude quota windows and reset times with observation timestamp/timezone. Missing, stale or failed observations are explicitly unavailable/stale, not estimates. Never include credentials, raw logs or account identifiers. Use Hermes send, not a custom Telegram transport. Do not spend saved quota resets. A failed send must not count as a successful delivery.

The lead invokes notifications at meaningful boundaries and before lengthy waits; this is not a promise of an exact hourly scheduler while idle. Y owns the first real preparation delivery and records the receipt so the lead does not duplicate it.

## Durable sources and drift checks

- Goal and gates: docs/prd.md.
- Current project status: WORKING-ON.md.
- Architecture: docs/architecture.md and each phase's accepted decision/implementation-contract files.
- Current phase: docs/plans/p5-web/decisions.md and implementation-contract.md. P5-DOCS-FINAL-A supersedes earlier crossed document-design messages. D17/D19 checkout, D20 asset ownership, D21/D22 consent and D23 invoice-preview rules remain binding.
- Actual evidence and resume sequence: docs/plans/p5-web/implementation/integration.md section 5a and its immutable receipts.
- Session todos: current batch only, governed by skill://todo-state-machine; they are not another full roadmap.

After startup, compaction, handoff or conflicting messages: reconcile objective, decision revision, authority token, owner, evidence level and next action. Old messages cannot override a newer release. A summary or source-ready report is not execution evidence. Reminder hooks never automatically mark acceptance or rewrite the plan.

## Preserved verification boundary

Candidate3: /home/mu/.cache/rustr-integration/s1-20261026151947.
Source manifest: daad27b809bfe082f836f0cc3aca95e73085aef56a1dfc20fa0d25fad635e037.
Stale gate and native host/FFI builds passed. The last FFI child completed exit0; wrapper exit148 represents intentional pause. logs/user-pause.json records no owned running jobs/containers and stages not started.

The prior candidate preserves AppState23 PASS, client44 PASS/1 FAIL with all12 export-consumer cases PASS, Transgate wasm check/release and native composed-state PASS. The RFC test-vector repair is source-verified and captured in candidate3, not yet behavior-verified. Preserve all failed snapshots. P5-GEN-RELEASE-5 remains valid; generated manifest 5ff92934aa5daa730a35e6a7fd694c2a43dbeb40d6715bccc0e231edbfa2bb28 and fixtures 602865cdf1c9da5786b5234a8287f036912d09a91715e1ef40aa6eb31a9137b2.

After preparation acceptance, Y resumes only unfinished authorized work: taskr wasm/type drift; exact publication-integrity regression; AppState/client/Transgate wasm/composed-state checks; immutable bindings and served-package hashes; P4 Transgate process scenarios; taskr browser4/4 and generated Linux Swift proof. Do not repeat already-passed native/FFI builds merely to restart the wrapper. Exact selector:

    cargo test --locked -p transgate-core --features ts --lib presentation::fixtures::web::export_integrity_preserves_outputs_on_rejection -- --exact

Two unsuccessful repairs of the same failure require reassessment, not assertion weakening or scope expansion. Material policy changes return to mu. Independent authorized work can continue around a blocker; later phase execution does not follow merely because it exists in the roadmap.

## PREP-1 notification acceptance

Y independently passed 15 notifier unit cases and real CLI scenarios covering coalescing, restart, failure, redaction and six-process serialization. Exactly one live Hermes delivery succeeded at 2026-10-26T14:21:06.409Z. Shared default state has no pending update; next attempt is not before 2026-10-26T15:21:06.409Z. Do not duplicate the acceptance message after handoff.

Evidence: /home/mu/.cache/rustr-integration/rustr-prep1-bju4vezq/live-delivery.json, live-status.json, notifier-unit.json and notifier-cli/report.json. Lead read the successful delivery receipt and status. Usage observation time is 14:17:51Z, not the send time: one available Claude observation reported 5h 7/100 used, reset15:10Z, 7d 29/100 and Fable7d 4/100 resetting2026-11-01T13:00Z; one additional Claude observation was unavailable. Codex primary reported58/100 and reset2026-11-02T06:00:45Z. These are historical observed windows, not current promises or estimates of this session's consumption.

Hook acceptance now passed: 12 regressions and the actual disposable OMP lifecycle/selection smoke. Receipts are `rustr-prep1-bju4vezq/guard-unit.json`, `logs/guard-unit.log`, `omp-run1/report.json` and `omp-run1/observation-receipt.json` under the integration cache. The smoke verified Opus medium, all four effective overlay settings, preserved ambient customizations/history, pinned-main activation, unchanged-reminder deduplication and nonlead inactivity. It observed zero model turns and zero extension errors. Real model-generated compaction was not exercised; controlled lifecycle regression coverage is not that claim. Passive notices cannot prove compliance.

PREP-1 is accepted: guard, notifier and the single Telegram delivery passed.

## Usage collector information, not a new assignment

Mu supplied Omarchy collector findings during PREP-1. Existing `omarchy agent usage-claude --limits-only` and `usage-codex --limits-only` expose subscription limits, but percent is a consumed fraction (0-1), cache writes may occur, Claude may silently reuse old limits, and record `updatedAt` does not establish successful-fetch freshness. Claude's successful fetch time is `fetchedAtMs` in its separate limits cache; expired windows becoming zero/empty are not a fresh refill confirmation. Codex's legacy collector omits some `rateLimitsByLimitId` buckets. The local `~/.local/bin/codex` wrapper can invoke `mise use -g`, so do not invoke it merely to gather quota information.

This is information only. Keep PREP-1's verified OMP usage collector, original observation timestamps and explicit unavailable/stale reporting. Do not start another monitor, restart the old sleep script, substitute the historical 14:29 UTC readings as current, or expand implementation in response to this note.

## TRANSITION-PREP-1

Lead authorizes Y to transition this exact saved lead after its current turn reaches a genuine idle boundary. First perform the focused in-place handoff, preserving this document, current OMP todo state, source ownership and all execution/evidence limits. Wait for completion. Then gracefully stop the idle CLI, confirm the old writer has exited, and resume the exact journal with the command in `docs/plans/lead-workflow-validation.md`. Preserve ambient customizations and the existing environment. No fork, new session, concurrent writer, hard kill of active work or provider fallback.

Y must verify actual activation rather than command submission: same session/history, Opus 5.5 medium, the explicit lead guard and four effective overlay values. Real ambient conflicts or unavailable model access mean stop and report, not remove customizations or substitute models. The resumed lead's first work is to read this checkpoint and the board, reconcile current-batch todos and verify effective settings. Candidate3 remains held until that verified lead issues a separate NEXT token for only the remaining verification listed above. The transition permits that one explicit continuation, not a recurring wakeup or new product batch.

Independent PREP evidence and the safe procedure are consolidated in `docs/plans/lead-workflow-validation.md` and the integration-cache `rustr-prep1-bju4vezq/report.json`. Superseded by the actual outcome below. Preserve the single Telegram delivery/cooldown; no second preparation send.

### Actual transition outcome (2026-10-26)

The planned in-place restart did not happen. Y's interactive double-Ctrl+C did not exit the old lead (pid 448402); Y launched no replacement. Mu then relaunched the lead himself in a new tab, pane w23:p2B, on a **copy** of the original session: new ID `01a178dd-4b69-7054-9304-2744cd5817f8`, journal `2026-10-26T15-38-51-113Z_01a178dd-4b69-7054-9304-2744cd5817f8.jsonl`. The original journal `01a16c93-...` is preserved and its pane w23:p1E is a backup only.

The lead verified from the actual process argv and effective settings: `anthropic/claude-opus-5-5`, thinking `medium`, explicit `lead-guard.ts` pinned to the new session ID with both P5 decision files, and `advisor.enabled=false`, `prewalk.enabled=false`, `prewalk.auto=off`, `retry.modelFallback=false`, all four from the `--config` overlay. Ambient customizations were not removed. `/lead-guard` is a user-typed command; the lead cannot invoke it on itself.

Lessons: to exit an OMP pane through herdr, type `/exit`; double Ctrl+C did not work. Do not press F5 in the old pane: its retry reruns an old launch command.

## NEXT-S1-C3

Lead releases to Y only candidate3's remaining checks, in order: (a) taskr wasm and type drift; (b) the exact export-integrity regression above; (c) AppState selector; (d) repaired transgate-client suite; (e) Transgate wasm check and release; (f) native composed-state smoke; (g) stage bindings and served packages with hashes; (h) P4 Transgate process scenarios; (i) taskr browser 4/4; (j) generated Linux Swift listener; (k) consolidated evidence and owned-resource cleanup. No repeat of native/FFI builds, no change to captured source, no C1/C2/C3 or new scope. Failures go to the owning OMP agent; after two failed repairs of the same failure, escalate to the lead.

**Result, accepted by lead 2026-10-26:** all checks (a)-(k) passed on unchanged candidate3, with no native/FFI rebuild and owned resources cleaned up. Evidence: `s1-20261026151947/logs/next-s1-c3/checks.json`; summary on `WORKING-ON.md`. Limits: synthetic providers only; capability-wait closure and PG-only quota reconciliation not exercised; Transgate container log unavailable after cleanup. No new batch follows automatically.

## NEXT-P5-C

Mu chose P5 next (2026-10-26). C1/C2/C3 are Y's verification checkpoints (`docs/plans/p5-web/implementation/integration.md` §1), not implementation batches. Lead releases to Y, in order, on the same candidate3 copies: **C1** generated-fixture screens (static Storybook build; captures in light/dark/system with all palettes, axe, keyboard focus, dialog focus return, long strings, RTL, landscape notice); **C2** complete wasm flows on Desktop Chrome and Pixel 7 over the owned runtime with all 16 locales; **C3** packaged app (image build per §5 after its stale gate, then the C2 subset and container checks). Same rules as before: captured source unchanged, no live providers/OAuth/payment/SMTP claims, failures preserved and never rerun until they pass. A failure in one checkpoint does not stop independent later checks; a missing entry condition does stop that checkpoint. Failures route through the lead to the owning OMP agent; two failed repairs return to the lead. Repairs need a new candidate, released separately.

Browser rule (mu, 2026-10-26): no evidence may depend on Hyprland tiling sizes. Use headless Playwright with explicit viewport/device emulation, or AI Brave (CDP 127.0.0.1:9225) with an explicit CDP viewport, in agent-created tabs only. Each report records the actual viewport and DPR. A capture taken from a tiled visible window is not evidence.

### C1 result (Y, 2026-10-26): FAIL, routed as REPAIR-P5-1

Static Storybook build passed. Headless run: 246 stories, 2952 cases; Desktop 1280x800 DPR1, Pixel 7 412x839 DPR2.625. Evidence `s1-20261026151947/logs/next-p5-c/c1-summary.json`, screenshots `c1/*`, `manual/*-headless.png`; earlier visible captures excluded. Lead classification:

| # | Finding | Class | Owner |
|---|---|---|---|
| 1 | `e.subscribe is not a function`: PlayerControls desktop+mobile (72 cases), SignedInFrame phone-job (12) | product render failure | p1Z (player/media), p1V frame seam |
| 2 | Desktop sidebar renders New Transcript / All Transcripts / Recent twice | product | p1V (frame), p1Z seam |
| 3 | axe `link-in-text-block`, AI consent settings (71 cases; link 1.22:1 vs text, no underline) | product a11y | p21 |
| 4 | axe `color-contrast`, transcript desktop+mobile (22) | product a11y | p1Z |
| 5 | axe `scrollable-region-focusable`, mobile purchase (11) | product a11y | p1V |
| 6 | "Axe is already running" (70 cases, unevaluated) | harness | Y |
| 7 | Missing coverage: RTL transcript and long-string fixture variants | missing fixtures/stories | p1X generator, p1Z stories |
| 8 | Missing App-root story for landscape notice with inert app | missing story | p1V |
| 9 | Native dialog focus return not provable with fixed-VM stories | coverage limit; C2 interaction proves it | Y in C2 |
| 10 | C3 entry missing: `transgate/Dockerfile` is server-only; the accepted §5 web stage and ignore re-includes were never applied | missing packaging | p1T |

Repairs land in source; Y re-runs C1, C2 and C3 only on a new candidate released by the lead. Two failed repairs of the same finding return to the lead.

### C2 and C3 (Y, 2026-10-26): BLOCKED at entry

- **C2:** candidate3 `npm run build` exit 1, svelte-check 64 errors / 12 warnings in 20 files (`logs/next-p5-c/c2-app-build.log`); no runtime, browser or locale flow started. Routed: transcript/media files (most errors, likely same root as finding 1) to p1Z; PurchaseDialog, Turnstile, SplitPane, TextField story, i18n, App container, `platform/browser.ts` (`Window.ClipboardItem`/`URL` typing) and telemetry test to p1V; `core/fixtures.ts` generated-type/fixture mismatch (e.g. `TranscriptVm.completed.fades`, `freemium`) to p1X.
- **C3:** `logs/next-p5-c/c3-summary.json`; same cause as finding 10. Lead decision: p1T writes the Dockerfile web stage and ignore files from Y's accepted §5 proposal; Y stays the independent checker and does not write packaging.
- C1 classification corrected by Y: 84 product render-error cases, 70 harness collisions, 104 axe-violating cases. Y fixes its harness collision in its own files.

Next: when all REPAIR-P5-1 owners report source-ready, the lead releases a new candidate (S1 candidate4) for C1, C2 and C3.

Routing corrections (2026-10-26): `platform/browser.ts` and `platform/telemetry.test.ts` belong to p1V (`web-shell.md`), not p22; p22's assignment was cancelled. One writer per file: PlayerControls and transcript/media files are p1Z; p1V owns SignedInFrame and the frame. p1V may run `npm ci` once against the committed lock in the source `transgate/web` for source tests, shared with p1Z and p21; no lock change. p1Z root cause: local identifiers shadow the `$state` rune in three components. Lead decision on the transcript fade: faded rows are inside the free cut (FRE-020), so text, speaker and time stay at full semantic foreground; the core `fades` cue stays visible through a decorative layer that never drops text below 4.5:1; banner unchanged; recorded as an accessibility deviation from the source visual.

Shared `transgate/web/node_modules` was damaged by two concurrent `npm ci` runs (p1V authorized; p1Z overlapping, exit 217 ENOTEMPTY). Rule from now on: p1V is the only installer in that directory. Lead authorized one serialized corrective `npm ci` by p1V; p1Z and p21 hold npm, Vitest and build there until p1V announces completion.

### REPAIR-P5-1 source-ready; NEXT-P5-C4 released (2026-10-26)

All owners reported source-ready: p1Z (`$state` shadowing, decorative-only fade, RTL/long-string stories; `workspace-transcript.md`), p1V (duplicate nav, focusable overflow region with one scoped ignore, App-root landscape story, all owned diagnostics; `web-shell.md`), p21 (consent link underline; `ai.md`), p1X (typed fixture companion, RTL/long-string fixtures; `core.md`), p22 (`fixtures.ts` loader cutover; `client-wasm.md`), p1T (Dockerfile web stage and identical ignore files; `hosts.md`), Y (axe harness). Source `npm run build` passed: svelte-check 1414 files, 0 errors, 0 warnings. Root `.gitignore` now excludes `transgate/web/{dist,src/lib/core/pkg}`.

Lead releases to Y: freeze S1 candidate4 from the current source with `scripts/p5-web/s1.sh`, run the stale gate and only the builds C1/C2/C3 need, then run C1, C2, C3 in order on candidate4 under the NEXT-P5-C rules and browser rule. Candidate3 and all its evidence stay preserved. The P4 re-proof is not repeated unless the candidate3-to-candidate4 manifest diff touches P4-relevant source outside the REPAIR-P5-1 files (core non-fixture code, http/server/api, jobs, billing, processor, taskr); in that case Y reports the diff before continuing.

Candidate4: `s1-20261026195749`, manifest `a1a7d000...`. Diff has no P4 runtime change outside REPAIR-P5-1; stale gate and Storybook build passed. C1 stopped fail-closed after 60 cases: Y's harness precondition (effective a11y.manual global) could not be read (`c1-harness-block.json`). Not a product finding. Lead: Y repairs its harness (second repair of the axe-collision finding), proves it on a one-story-per-project smoke (guard sees manual, one axe run, a known violation still reported), then runs the full C1 matrix; a failed smoke returns to the lead. C2/C3 entry checks continue meanwhile.

Candidate4 C3 (Y): stale gate and isolated context export passed (0/6 canary leaks); real image build failed in the web `npm run build`: `DesktopSidebar` imports `../../art/logo_0.png?url`, omitted by the ignore allow-list (`logs/next-p5-c/c3-image-build.log`). Routed as REPAIR-P5-2 to p1T: derive the re-includes from all real web build inputs, not one file, and record the check. When source-ready, Y freezes candidate5; if its diff from candidate4 is only the two ignore files (and hosts.md), C1/C2 results on candidate4 stand and only C3 runs on candidate5. C1 guard smoke passed (manual=true, one axe run per project, injected image-alt canary detected); full C1 matrix running. C2 real-wasm app build and owned runtime entry passed.

Candidate4 C2 (Y): real-wasm signup sends PKCE and Mailpit receives the confirmation, but the GoTrue link points at proxy-root `/verify`, outside the owned `/auth/v1` prefix proxy. Lead classification: fixture/runtime config, not product. GoTrue resolves absolute mailer paths against `API_EXTERNAL_URL` and drops its path; fix is `GOTRUE_MAILER_URLPATHS_{CONFIRMATION,INVITE,RECOVERY,EMAIL_CHANGE}=/auth/v1/verify` in Y's `runtime.py` (as Supabase self-hosting does), no link rewriting. Signed-in C2 flows then run on candidate4. P9 staging/production GoTrue config must carry the same setting.

Candidate4 C2 progress (Y): after the mailer-path fix, the delivered `/auth/v1/verify` link succeeds. Passing: password login, signed PUT upload, `POST /api/jobs/start` 201, completed transcript, edit, export 200, AI consent. Product findings, preserved, no retries:
- **REPAIR-P5-3a, p22:** after confirmation the app lands on `/signin` with the PKCE verifier still stored and no session; the code exchange does not happen.
- **REPAIR-P5-3b, p1T:** `POST /api/v1/ai/chat` returns 400 "No transcript content found for this job" despite a visible saved transcript. p1T diagnoses; core (p1X) or client (p22) only if the evidence points there.

REPAIR-P5-2 source-ready (p1T): both ignore files cover web-wide image/font/text assets with generated/test/dependency exclusions and secrets last; inventory 451 paths = 365 required + 86 excluded, 0 omissions; commands in `hosts.md` §160. Note: `transgate/web` is not tracked in git at all (`git ls-files` empty); the inventory used cached plus untracked files. Candidate manifests capture untracked files, so evidence is unaffected, but the P5 web source is uncommitted.

Lead decision: candidate5 waits for REPAIR-P5-3a/3b, so one candidate carries all three repairs. On candidate5 Y runs C3 in full, the C2 flows that failed or were blocked on candidate4, and any C1/C2 surface whose files changed; untouched candidate4 results stand.

REPAIR-P5-3b diagnosis (p1T, preliminary): the host passes the job unchanged; core `start_chat` returns the same `NoTranscript` error for empty lines and for `validate_chat_context` failures, and `sanitize.rs` requires at least 50 UTF-16 units. The C2 edited transcript was 32 chars, so too-short is the likely cause, shown as a misleading "no transcript" message. Now owned by p1X: check the Transgate source for the rule and its user message; separate the errors only if the source does, else keep parity with file:line evidence. Y's candidate5 AI flow uses a transcript of at least 50 units plus a separate short-transcript case. Y confirms the stored line length.

Confirmed by Y (read-only): the stored line is exactly "Candidate four edited transcript." = 33 UTF-16 units, `corrected_text` NULL. The chat 400 is the 50-unit minimum applied to a too-short test transcript, not a broken route. Remaining product question is only the user-visible message (p1X, source parity).

Candidate4 C2 (Y): phone password login passes, but authenticated direct navigation to `/home/1` lands on `/home?view=new` instead of the stored transcript (`c2-independent-report.json`, `c2-phone-deep-link.png`). Likely the same start-route root as REPAIR-P5-3a; p22 leads one diagnosis for both, handing core evidence to p1X if route parsing or the post-sign-in decision is at fault. A locale-harness assertion failed on a guessed catalog key before navigation: harness error, preserved, not a product finding; Y now asserts source keys such as `sign_in.button_label`.

REPAIR-P5-3b decision (p1X, approved): the source enforces 50..512000 UTF-16 units (`content-processor.ts:85-98`), shows "No transcript content found" for zero rows (`chat.ts:91-93`) and maps every rejected sanitized context to "No transcript content available for chat" (`ai-service-manager.ts:189-199`). Core `ai/app.rs:575-576` wrongly reuses the analysis-style `NoTranscript`. Fix: context rejection uses the existing `AiError::Invalid` with the exact source copy; no new message, API or threshold. Failing admission regression first.

REPAIR-P5-3a root cause (p22, approved): the wasm presenter (`presenter.rs:261,272`) forwards the absolute `browser::href()` into core `route::parse`, which expects a path (`route.rs:148-152`). An absolute URL parses as `None`, so core drops the PKCE callback and defaults to `/home`, then `/signin` when signed out; the same bug sends authenticated `/home/1` to `/home?view=new`. Fix at the adapter: normalize `Started`/`RouteChanged` input to path+search, keep the absolute href only for API base resolution. Failing real-AppState regressions for the callback, the deep link and a locale-prefixed path; check popstate and post-sign-in paths for the same bug.

**Candidate4 C2 partial FAIL** (`logs/next-p5-c/c2-report.json`). Exercised and passing: real-wasm password login on both projects; desktop upload, ranged playback, transcription, edit, ZIP export, support, feedback, erasure 202; other-tab sign-out after erasure. Failing or not exercised, with lead classification:
- PKCE callback and phone deep link: product, REPAIR-P5-3a (p22).
- Non-en locales: en 2 PASS, 30 non-en FAIL (UI stays English). Product; p22 checks whether the same start-URL bug drops the locale prefix, p1V checks i18n read-only.
- AI: 33-unit refusal is the fixture minimum; positive AI runs on candidate5.
- Checkout: Y's runtime has no Iyzico loopback route, so checkout handoff and HTML confirm were not exercised. Y adds a loopback fixture for the documented adapter calls for candidate5; no live calls.
- Erasure initiating tab lands on public root 404: P6 dependency (public root), not a P5 failure.
No live acceptance claimed.

REPAIR-P5-3b source-ready (p1X): `ai/app.rs` now returns `Invalid("No transcript content found")` for zero rows and `Invalid("No transcript content available for chat")` for rejected context, both HTTP 400 `invalid_request`; analysis `NoTranscript` and the 50..512000 rule unchanged. Failing admission tests first; `ai::app::tests` 22 PASS, including 49/50 and no admission/quota/provider effect on refusal; read-only legacy `ContentProcessor` smoke matches (0 and 49 reject, 50 accept). Display trace with file:line in `implementation/policy.md`.

Locale diagnosis (p1V, read-only): i18n loading is not the cause. `App.svelte` sets `documentElement.lang` from `vm.locale` before catalogs load, and C2 shows `lang=en` on tr/pt pages, so the AppVm locale itself stayed en: same start-URL root as REPAIR-P5-3a; p22's locale-prefixed regression asserts `AppVm.locale`. p1V also found a latent race: overlapping `i18n.use` calls lack a latest-request fence. Routed as REPAIR-P5-4 to p1V: fence with a failing test first, plus a test that loads every real catalog and checks a known key is translated.

REPAIR-P5-3a progress (p22): all three symptoms (callback, deep link, locale prefix) follow the same absolute-href `parse` → `None` branch; the locale regression asserts `AppVm` locale for `/tr/home`. Lead decision: the pure href-to-path+search normalization lives in `transgate-client` (no web-sys, no new dependencies) and the wasm presenter calls it, so regressions are ordinary client tests against real AppState rather than a `#[path]` include. Known gap, not widened: native `cargo test -p transgate-wasm` fails on a pre-existing `JsFuture` `!Send` versus `BoxFuture + Send` mismatch in `NavigatorLock`.

Y's candidate5 fixture work is source-ready, unrun: `scripts/p5-web/iyzico_fixture.py` and `runtime.py`, contract in `integration.md` §7. It verifies the adapter's exact request signature and plans PAYG/subscription init, result, detail and cancel; a synthetic nonce-script form submits to the real `/checkout/confirm`; no live calls. Mailer-path fix kept; AI positive (≥50 units) and short-transcript cases and the P6 erasure dependency recorded. Receipt: `candidate5-fixture-source-handoff.json`. Candidate4 C1 full matrix still running.

REPAIR-P5-4 source-ready (p1V): request-identity fences in `i18n/index.svelte.ts` stop a stale locale load publishing; `App.svelte` effect cleanup drops stale readiness callbacks. Failing-first test (delayed tr overtaking de) showed tr; now only de is ever published. Real-glob test loads all 16 locales and 5 namespaces, and every non-en `sign_in.button_label` differs from English. Scoped Vitest 24 PASS; `npm run build` PASS (1415 files, 0 errors, 0 warnings). Limit: no dedicated out-of-order test for the App-root cleanup.

REPAIR-P5-3a source-ready (p22): `transgate-client::location::app_location` used by the wasm presenter on start and popstate; absolute href kept for API base only. Three real-AppState regressions failed before and pass after (callback exchanges; phone deep link keeps job 1 and history job 2; `/tr/home` gives locale tr); native runtime smoke passed. This explains all 30 non-en C2 failures. Selector: `cargo test --locked -p transgate-client --test browser_location`. Handoff `client-wasm.md` §REPAIR-P5-3a.

### NEXT-P5-C5 released (2026-10-26)

Candidate5 carries REPAIR-P5-2 (ignore files), 3a (start URL), 3b (chat refusal copy), 4 (i18n race) and Y's fixtures (Iyzico loopback, mailer paths, AI ≥50). Y lets the running candidate4 C1 matrix finish and reports it, then freezes candidate5. Because 3b changes core AI code, candidate5 also runs the P4 Transgate actual-process scenarios plus the AppState and transgate-client suites; taskr browser and Swift are not repeated if the diff leaves taskr untouched. Then C1, C2 and C3 in full on candidate5, under the same rules.

**Candidate4 C1 result** (2247.77 s; `c1-summary.json`): 251 stories, 3012 cases (desktop 1872, phone 1140); 3010 audited with **0 axe violations, 0 harness collisions, 0 render errors**, against candidate3's 104 violation cases, 70 collisions and 84 render errors. Two cases could not be prepared (dynamic module fetch failure: AI mobile "Copied" and Root PhoneLandscape); the guard refused to audit them, so C1 is not a clean PASS. Headless RTL, long-string and landscape-inert inspections captured; native dialog focus return observed in C2. Candidate5 C1 must show whether those two are product (missing or broken chunk) or harness (static server/load) with the actual fetch error, without retrying them into a pass.

**Candidate5 progress (Y):** AppState 23, transgate-client 48 and P4 Transgate actual-process scenarios PASS; Docker image build PASS. C2: the unmodified email-verify PKCE callback now passes (session stored, verifier cleared); locale-prefix labels 32/32 PASS; positive AI (237 UTF-16 units) start 200, watch 200, completion PASS. New failure, preserved, no retry: a second tab on the same job during an in-flight answer loads the transcript and `GET` chat 200 but shows no held partial answer and opens no second watch (`c2-action-receipts.json`, `c2-network.json`, `c2-ai-reattach-missing.png`). Routed as REPAIR-P5-5: p1X diagnoses whether core emits the attach effect (source parity read-only), then p22 if it is client execution; p21 on standby.

**REPAIR-P5-5 decision (lead):** root cause (p1X, real AppState diagnostic): chat history returns only saved pairs, so an in-flight answer cannot be represented, and the chat leaf never watches; the legacy app never reattached either. P5-D6 requires reload/layout switch without redispatch, and a second tab is the same case, so this is a deliberate correction beyond legacy. Contract: the same session `GET` chat-history route returns `{messages, in_flight: Option<{question, generation: GenerationView}>}`; core reads the authorized active chat via the existing `AiTx::active_for_job`; the leaf puts the held partial into `lines[]` as a streaming assistant line and emits Watch only (no POST, quota, provider call or new key). Clean cutover, failing tests first. Owners: p1T DTO and route (publishes first), p1X core and leaf plus generated types, p22 client transport and wasm; p21 no view change.

**Candidate5 C1 accepted** (`s1-20261026204512`, 2421 s, `c1-summary.json`): 251 stories, 3012 cases, 3011 audited, 0 axe violations, 0 collisions, 0 render errors. One case (SignedInFrame Special Offer, indigo light) unprepared: its request failures are `net::ERR_NETWORK_CHANGED` on chunks that exist and were audited in 71 other cases, so it is classified as environment transport, not product. The candidate4 unprepared pair did not recur. C1 is accepted with that one recorded environment gap; no retry.

REPAIR-P5-5 p1T source-ready: `http/ai.rs` serializes core `ChatHistory {messages, in_flight}` directly (`in_flight` always present on the wire, `null` when idle). Real-PG `ai_http_chat_history_reports_the_held_answer_to_a_second_replica_and_only_to_its_owner` red first, then 4 PASS with p1X's core: idle null, held partial via a second replica with the question as typed, foreign 404, unauthenticated 401, one provider call, completion returns null plus the saved pair. Handoff `hosts.md` §REPAIR-P5-5. Waiting on p1X core/leaf and p22 green run.

REPAIR-P5-5 p22 source-ready: `client/src/ai.rs` decodes the whole `ChatHistory` with no messages-only fallback; new `client/tests/chat_reattach.rs` (in_flight gives exactly one Watch and no start or retry; null gives none). `cargo test --locked -p transgate-client` all green; wasm32 `cargo check -Zbuild-std -p transgate-wasm` PASS (driver passes the type through). Not run: wasm release/bindgen and the browser second-tab proof. Only p1X's core/leaf handoff remains before candidate6.

REPAIR-P5-5 p1X source-ready: core `ai::{ChatHistory, InFlightChat}`; `Ai::chat_history` adds `in_flight` via `active_for_job`, owner-fenced, with the original question; the chat leaf hydrates the held partial as a streaming assistant line and emits exactly one keyless Watch (no start, retry, key or quota). Four failing-first regressions including a real-AppState second-tab test; `transgate-core --features ts --lib` 394 PASS; p1T's PG route 4/4 against this core. New fixture `ChatPanelVm.reattached`; no generated type change.

### NEXT-P5-C6 released (2026-10-26)

Candidate6 adds REPAIR-P5-5 (p1T route, p1X core/leaf, p22 client) to candidate5. Candidate5 C1 stands unless the diff touches web views or stories beyond the new fixture; Y then reruns only the affected C1 stories. Y reruns: AppState and transgate-client suites, the P4 Transgate actual-process scenarios (core AI changed), wasm release, C2 in full (including the second-tab reattach case: partial shown, exactly one watch, no second start) and C3 on the new image. Same rules.

**Candidate5 C2 and C3 accepted** (`c2-report.json`, `c3-report.json`). C2 single product failure: second-tab AI reattach (fixed in candidate6 by REPAIR-P5-5). C2 PASS: PKCE verify callback, phone deep link, 32/32 locale prefixes, positive AI desktop and phone, short-transcript refusal (exact copy, 400 `invalid_request`), Iyzico D17 handoff and HTML confirm, invoice PDF parsed by Poppler, magic link, two-tab rotation, invalid code, support 202, feedback 201, erasure 202 with other-tab sign-out. Password recovery not exercised: GoTrue's mail frequency limit (fixture config; Y sets loopback throttles and runs it on candidate6). **C3 PASS:** image `sha256:2aa565e6...` runs non-root (65532) read-only, 0 canary leaks, D20 MIME types and caching, API JSON 404, health, and packaged sign-up, upload, AI, checkout confirm and erasure. Runtimes cleaned.

**Candidate6 early result** (`s1-20261026213245`): stale gate, AppState, transgate-client 48, host build, wasm and P4 scenarios PASS; PKCE and upload PASS. **Regression (REPAIR-P5-5b):** the original tab's own AI chat shows no user line, partial or answer although start 200, one watch 200 and the provider finished; a fresh tab after completion shows the saved pair. Candidate5 rendered this correctly, so REPAIR-P5-5 caused it; the second-tab case cannot be judged. Routed to p1X (chat leaf, leads) with p22 (client ordering and watch decode); real-AppState own-tab sequence test red first; own-tab start must not be treated as a reattach. Y continues the remaining C2/C3.

REPAIR-P5-5b root cause (p1X, approved): the chat leaf allowed a send while history was loading. In candidate6 the history `GET` happened later (after analysis), the send landed first, and `HistoryLoaded` replaced `lines[]`, dropping the local user line and leaving `pending.index` out of range, so the answer never rendered. No second watch happened. The race predates REPAIR-P5-5, which only changed request timing. Fix: send refused and `can_send=false` while history loads (source parity: `useChatViewModel.ts:67`, `DesktopChatView.tsx:142,307`, `MobileChatInput.tsx:73-109`), plus an invariant that a history load arriving while a request is pending never drops it. Real-AppState tests red first for the own-tab sequence, the late-history case and the second-tab reattach.

p22 (read-only) confirms from `c2-network.json`: the original tab sent `GET` chat, `POST` chat, one watch, then chat-limits, with no second history request and no decode error, so frames reached the leaf; a history response landing after the send fits the cause. The new route does more work, so the window probably widened [INFERENCE: timing not measured]. Fix stays in the leaf; the driver gets no request ordering because the leaf must tolerate any arrival order. p22 adds a client regression with a delayed history response.

**Candidate6 report** (`report.json`). PASS: stale gate, AppState, client, host and wasm builds, P4 Transgate, PKCE callback, phone deep link, 32/32 locales, short-transcript refusal, Iyzico handoff and HTML confirm, magic link, recovery mail delivered (throttle fix), two-tab rotation, invalid code, support, feedback, erasure; C3 image `sha256:d5d3085cca85...` non-root read-only, static serving, packaged subset including unheld AI. C1 not rerun (only fixtures changed). Product failures:
1. Own-tab AI stream renders nothing (REPAIR-P5-5b, p1X with p22, in progress); second-tab reattach not yet judgeable.
2. **REPAIR-P5-6:** the recovery link lands on `/new-password` but shows the signed-in home without the password form. p22 leads; core route decision goes to p1X if implicated.
3. **REPAIR-P5-7:** the product requests `https://cpp.iyzipay.com` (a logo; blocked). p1V finds the source: our markup becomes a local `/_app` asset (D20); Iyzico's own injected form is an expected D19 provider contact.

REPAIR-P5-5b p1X source-ready: send and retry refused with `can_send=false` while history loads, no queueing; history loads only on selection or consent grant, both of which reset requests, and late duplicates are ignored once loaded, so a load never lands over a pending question. Real-AppState tests: own-tab sequence red first then green; late history while pending keeps both lines (documents the invariant); second-tab reattach still exactly one Watch. `transgate-core --features ts --lib` 396 PASS; fixture `ChatPanelVm.loading_history` now `can_send=false`. p22's client regression uses the same contract and should turn green.

p22: slow-history client regression green against p1X's fix (red before); transgate-client suite all PASS. REPAIR-P5-5b is source-ready.

REPAIR-P5-6 root cause (p22, read-only): client exchange and core are correct: a recovery code yields `PasswordRecovery`, core signs in (a session is needed to set the password) and routes to `NewPassword` with `SignedInVm.new_password` populated. The shell loses it: `App.svelte:86-105` renders `NewPasswordView` only when signed out; signed in always renders `SignedInFrame`. Routed to p1V (App.svelte owner): render the password form from `vm.new_password` when core's route is new-password, matching source layout; Vitest red first. p22's `client/tests/recovery_callback.rs` documents the core contract.

REPAIR-P5-7 finding (p1V, read-only): the `cpp.iyzipay.com/img/guvenli_odeme_logo_seridi.svg` request is our own checkout footer `<img>` (`transgate/web/checkout/document.html:35`, copied from the source), not Iyzico-injected. Lead decision (D20): p1V adds it as the unhashed local `/_app/iyzico-secure-payment.svg` with provenance and changes the reference; the checkout CSP keeps `img-src https:` because Iyzico's own form still needs it (D19); p1T confirms host content type and unhashed-file caching. Y's C3 D20 checks gain this file.

REPAIR-P5-7 p1T source-ready: the host already serves `/_app/*.svg` as `image/svg+xml`, but its no-cache rule was hard-coded to `theme-init.js`, so the badge would have been cached as immutable for a year. Now an explicit `UNHASHED_ASSETS` list (`theme-init.js`, `iyzico-secure-payment.svg`) gets `no-cache`; hashed files stay immutable. Host test red first, `web::` 13 PASS; checkout CSP unchanged. Handoff `hosts.md` §200. Waiting on p1V's asset and REPAIR-P5-6.

REPAIR-P5-6 and 7 p1V source-ready. P5-6: the source new-password page is standalone on desktop and phone; `App.svelte` now renders `NewPasswordView` from `vm.new_password` when signed in on core's new-password route, otherwise `SignedInFrame`; failing-first Vitest (`App.test.ts`, desktop and phone) red then green. P5-7: badge now `/_app/iyzico-secure-payment.svg` (656139 bytes, sha256 `39233f34...c9405`, fetched once, no scripts or external references), provenance in `transgate/web/checkout/PROVENANCE.md`. Scoped Vitest 13 PASS; `npm run build` PASS (0 errors, 0 warnings), badge present in `dist/_app`. Lead decision on p1V's caveat: the checkout CSP `img-src https: data:` lacks `'self'`, so the badge would be blocked on plain-HTTP runs; p1T adds `'self'` only (REPAIR-P5-7b).

REPAIR-P5-7b p1T source-ready: checkout `img-src` is now `'self' https: data:` plus fixture origins, no other directive changed; test red first, `web::` 13 PASS.

### NEXT-P5-C7 released (2026-10-26)

Candidate7 adds REPAIR-P5-5b (chat send refused while history loads), 6 (signed-in new-password page), 7 and 7b (local badge, unhashed no-cache, CSP `'self'`). Committed as product and harness commits. Y reruns: AppState, transgate-client and core AI/presentation suites, P4 Transgate scenarios, wasm release; C1 only for stories whose files changed (App root, checkout-adjacent views) plus the new `ChatPanelVm` fixtures; C2 in full, including the own-tab chat, the second-tab reattach (partial shown, exactly one extra watch, no second start, provider calls 1) and password recovery reaching the form and saving a new password; C3 on the new image including the badge served from `/_app` as `image/svg+xml`, `no-cache`, and no request to `cpp.iyzipay.com`. Same rules.

**Candidate7 accepted: C1, C2 and C3 PASS** (`s1-20261026215323`, `report.json`). Suites, builds and P4 Transgate PASS. C1 targeted 93 stories / 1116 cases, all audited, 0 axe violations. C2: PKCE, own-tab chat, second-tab reattach (partial shown, one extra watch, no second start, provider calls 1), recovery to the new-password form and sign-in with the new password, phone deep link, 32/32 locales, short refusal, Iyzico handoff and HTML confirm, magic link, rotation, invalid code, support, feedback, erasure. C3: image `sha256:1efb4e84821a...` non-root read-only, 0/6 canaries, badge served from `/_app` and loaded, 0 requests to `iyzipay.com`, 0 non-loopback requests, packaged flows PASS. This is checkpoint acceptance, not the P5 phase gate: the gate is every applicable WEB parity row green on both projects.

### Parallel batch (mu: parallelize, Claude only, medium effort), 2026-10-26

- **P5-CLOSE (Y):** row-by-row disposition of WEB, WEB-BIL, WEB-IDN, WEB-PRV and LOC rows against the accepted evidence (proven, other phase, or gap) in `docs/plans/p5-web/implementation/parity-disposition.md`, then a proposed minimal gap run.
- **P7 design (p10 Opus, p27 Fable):** independent proposals per `docs/plans/p7-api/brief.md`; Codex/Sol is unavailable, so Fable replaces Sol as the second advisor. Cross-critique only on dispatch.
- **P4-GAPS (p1T):** read-only diagnosis of capability-wait closure and PG-only quota reconciliation, the two unexercised P4 paths.
- **Held:** P6 S0 needs Y after P5-CLOSE (independent test owner); P6 implementation stays undispatched.
- All active Claude workers were relaunched at medium effort with the Claude usage gate (`--extension .../claude-usage-gate.ts`).

P4-GAPS diagnosis (p1T, `/tmp/p4gaps-p1t.txt`): (1) capability-wait closure is a harness-flag gap: it passed once (1207 s on `85fea3ad`) and unit logic exists, but the normal run skips it. (2) Of PG quota reconciliation, rollover (`jobs-postgres/tests/ai.rs:730`) and unknown-COMMIT (`:974`) are already covered by real-PG tests; the untested path is quota-store failure: fail-closed admission (AI-019) and paid bypass with a failing store (AI-014). Routed: p1T adds a deterministic real-PG closure test with a past-expiry seed, three real-PG store-failure tests and the HTTP 503 case; p1X adds failure injection to the core fake with core tests; they agree the error mapping first.

P4-GAPS p1X source-ready: core behavior was already correct; tests now document it, no product change. Mapping agreed with p1T: a quota-store failure at chat admission is `AiError::Unavailable`, HTTP 503 `unavailable` "AI storage is unavailable", transaction rolled back (P4 D2 fail-closed correction of the source's AI-019 fail-open). Core fake gains `fail_counters` and `fail_packages`. Tests: freemium admission with the store down returns 503 with no generation, counter, dispatch or provider call, and the same key admits after recovery; AI-014 paid users are admitted, answered and unmetered while the store is down, and a failed plan lookup falls back to freemium (metered, so 503 while down); a settlement-time failure keeps the staged final and settles exactly once after lease expiry with one provider call. `transgate-core --lib` 395 PASS. p1T's real-PG and HTTP side pending.

P4-GAPS p1T source-ready on real Postgres; product already correct, tests document it. Capability-wait closure: `billing/tests/runtime/erasure.rs` drives real routes with an expired upload capability; erasure stays blocked while the URL is valid, the post-expiry sweep leaves a clean HEAD unresolved, a late PUT is found and deleted but stays unresolved, non-admins get 403, and closure is only `erased_with_exceptions` with the admin's approval, never `fully_erased`. Quota-store failure: three `jobs-postgres` tests (freemium refused before anything is durable; paid callers never touch the store; settlement failure reconciled locally with one provider call) and the HTTP 503 case. `jobs-postgres` ai 20 PASS, billing runtime `ai_http_` 5 PASS. Both P4 gaps are closed by tests.

Flag from p1T: pre-existing failure `erasure::account::feedback_is_owned_...` (500 "billing account does not exist" for a signed-in stranger). Lead decision REPAIR-P4-8: the test is right; an owned-job route for a user without a billing account answers 404, not 500, without provisioning as a side effect. p1T fixes the shared lookup.

**NEXT-P5-GAP billing group PASS** on candidate7, both projects (`next-p5-gap/actions.json`): hour presets, arrows and typed clamp; phone sheet with subscription section; coupons (invalid, expired, 25% applied); Iyzico failure with countdown, unconfirmed without countdown, phone success; start-failure page; subscription purchase, current plan and cancel; invoices on phone and desktop with Poppler-parsed PDFs; admin lookup and custom Bill To; special-client alerts; 402; freemium 22-minute cut with upgrade banner; balance surfaces including negative balance. Y added owned fixtures (coupon SQL, Iyzico rejected init, special-client and lookup env). Product findings routed: **REPAIR-P5-8** new job missing from the sidebar and all-transcripts lists for over 40 s with no list polling (D11), p22 diagnoses; **REPAIR-P5-9** 402 button says "Add Balance" where the source says "Add minutes" (`NotEnoughBalanceAlert.tsx:34`), p1Z diagnoses. Observation for p1T: yearly Premium shows validity +1 month and next payment +1 year; parity check requested.

NEXT-P5-GAP auth group PASS on candidate7, both projects: phone sign-up, magic link and full recovery with sign-in on the new password; signed-in `/signin` redirects home; sign-out from both menus clears the local session; protected routes redirect to `/signin` without a session; Google button sends PKCE S256 to the local authorize endpoint and GoTrue redirects to Google (intercepted; completion stays LIVE-P9).

REPAIR-P5-8 root cause (p22, read-only): D11 owned-list polling was never built. `job_list` has no poll effect and `JobChanged` has no producer, core timers cover only balance, password and confirmation, there is no visibility intent, and nothing refreshes the list after a start. It is a missing core feature, not a client bug. Lead: p1X does the core slice ahead of P7 (visibility intent, poll timer while visible and signed in, source cadence, cursor-safe merge, refresh on start; failing real-AppState tests first), then p22 wires the timer and the `visibilitychange` listener.

**NEXT-P5-GAP settings group PASS** (both projects): profile, theme incl. system, language switch persisted, AI enable and revoke with deletion, manage-data switches, delete all. No palette selector exists in app settings (palette is token-level, Paper Moss default per D10/E3); C1 covers palettes.

**NEXT-P5-GAP jobs group** (both projects): PASS translation with original gated, speaker recognition and rename, playback and seek, line edit and cancel, show-time/speaker toggles, export of every format (ZIP parsed), phone export sheet, all-transcripts download/delete/view, phone swipe delete, draft restore, processing timeout alert, suspended-account YouTube 403. Product findings routed: **REPAIR-P5-10** YouTube add and resolve succeed (`{"jobId":N,"status":"completed"}`) but the UI shows "An unexpected error occurred" and stays on `/home`, p22 diagnoses; **REPAIR-P5-11** the seek slider's accessible name is "Show Time", p1Z. Y adds back-dated job seeds for list paging and the history window and fixes the phone speaker-reassign harness step.

REPAIR-P5-8 is source-ready in core and client (committed `f4a6768`); the DOM visibility listener and timer still need a browser check on candidate8 (new job visible within one cadence, no polling while hidden).

**NEXT-P5-GAP AI group PASS** (both projects, loopback DeepSeek): analysis with consent, copy and DOCX/PDF/TXT/MD exports parsed by independent readers, interrupted analysis with partial summary and retry, chat answer, limits, copy, download and retry; phone held stream completes. Rating buttons only emit a consent-gated analytics event in the source (`ChatContainer.tsx:78-80`), checked with cookies. Highlights are not assumed covered: Y checks the source rows.

REPAIR-P5-10 p22 source-ready (uncommitted until P7 lands with the shared `Cargo.lock`): the client decodes the YouTube resolve reply through a private typed wire enum mapped to core `ResolveAnswer`; tests built from the real `transgate_api::resolve_response` failed before and pass now, including a real AppState navigate. A sweep of 38 client decodes found no other mismatch; `client/tests/host_contracts.rs` now checks consent, limits, support and catalog cards against the real host producers. **Follow-up (p1T after P7):** `DELETE /api/jobs`, `PATCH /api/jobs/lines`, the billing catalog envelope and the coupon reply are built inline in routes, so they have no producer to test against; make them reusable producers with contract tests.
