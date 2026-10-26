# PREP-1 independent validation

Owner: p1Y. Implementation owners: p22 (`scripts/lead-workflow/lead-guard.ts` and its tests/handoff), p1V (`notify*` and notification handoff). **Independent PREP-1 validation passed within the limits below; the lead has read and accepted the evidence.** Real-lead transition remains gated by an explicit TRANSITION token, and candidate3/product execution remains held.

## Boundary

PREP-1 authorizes preparation only. Candidate3 at `/home/mu/.cache/rustr-integration/s1-20261026151947`, prior failed snapshots, run5 publication and saved product smoke sources remain untouched. No product command, new product snapshot, C1/C2/C3, cron, recurring model call, global configuration change or saved-owner-session mutation is permitted. Product execution requires a later explicit NEXT token from the lead.

## Checks

| Surface | Independent evidence required |
|---|---|
| Guard units/scenarios | Owner-published targeted selectors; exact source identities, exits and behavioral results. Failures return to the original owner, not tester implementation takeover. |
| Actual OMP startup | Explicitly load the coherent extension in an owned disposable process with isolated HOME/config/session storage; observe its actual startup output/state. Record installed OMP version and invocation. No existing session is resumed. |
| Reminder | Exercise the published reminder boundary; inspect actual message/state rather than only registration or callback echoes. |
| Compaction or handoff | Exercise the actual lifecycle where practical without model/provider calls. Otherwise inject controlled lifecycle events through the real extension and label the missing real-compaction/handoff proof. |
| Deduplication | Repeat the triggering event and reload/resume only owned disposable state; check duplicate prevention and legitimate subsequent transitions. |
| Pause/no resumption | Enter the published pause state and deliver completion/reminder events. Observe that they cannot authorize or launch another work phase. Report the limit of an advisory reminder versus enforced runtime blocking. |
| Non-lead isolation | Explicitly load the same extension in a disposable non-lead session; confirm no lead-only messages, commands or state mutations beyond the published contract. |
| Notifier coalescing | Multiple meaningful updates within the hourly window; delivery contains the intended aggregate and does not emit per-agent acknowledgment noise. Use controlled clocks rather than waiting an hour. |
| Notifier restart | Persist pending/sent state, restart a disposable notifier process, and inspect retained pending work and deduplication. |
| Notifier failure | Simulate transport errors and ambiguous outcomes; inspect durable state and published retry behavior without weakening delivery assertions. |
| Notifier concurrency | Competing processes share disposable state; check serialization and duplicate/lost-update boundaries. |
| Notifier redaction | Synthetic secret/recipient-like inputs and malformed usage fields must not leak into transport, logs or reports. Real credentials/recipient identifiers are never printed. |
| Real Hermes delivery | Verify the configured user recipient through the published prerequisite, then send exactly one short preparation update through Hermes. No guessed recipient, direct Telegram bypass or periodic scheduler. Record the sanitized delivery receipt and exact limits. |
| Usage | Include only observed Codex/Claude usage/limit/reset facts with observation time and source; otherwise explicitly state unavailable. No estimated quota or inferred reset. |

## Isolation and execution approach

Installed runtime help reports OMP `18.12.6`. Its documented `--no-extensions` flag still permits explicit `-e`; RPC mode supports startup, state/message inspection and extension commands without an agent turn. RPC disables automatic title calls and exits normally on stdin closure after draining output. `--no-tools`, `--no-lsp`, `--no-skills`, `--no-rules`, an isolated HOME/agent directory and a disposable working directory will keep the smoke away from saved owners and ambient configuration. Exact flags and event inputs will be bound to the coherent implementation, not guessed now.


## Lead transition verification

`docs/plans/lead-preparation.md` is the current authority. The intended selector is exactly `anthropic/claude-opus-5-5` with `medium` thinking. Test its acceptance and returned runtime state only in a disposable OMP session; do not substitute another model. Catalog/selection acceptance is not a live provider-completion claim.

The saved lead session and pane must not be touched by validation. Test an idle-boundary recipe on an owned persisted disposable session: confirm no streaming/pending work, preserve its session identity/history, explicitly attach the extension through the supported continuation/reload mechanism, and verify the extension is loaded on the same saved session. Report whether that requires a process restart versus an in-process reload. Do not use `/new`, `/clear`, a fork, or a replacement session as proof of same-session continuation.

The lead's inactive advisor currently points to Astra xhigh; never enable it implicitly. Prewalk stays disabled. `retry.modelFallback=false` is currently lead-session-only and persistence awaits mu; the disposable test must disable fallback without saving global configuration. Actual lead handoff/model transition requires a separate explicit transition token after preparation acceptance, and `/handoff` must not be sent while the lead is streaming.

A real model-generated compaction is not required to invent a green result: if that path cannot be exercised without an extra model call, controlled-event evidence will be reported separately. The same distinction applies to fake Hermes transports used in scenario tests versus the single authorized real delivery.

## Handoff requirements

The consolidated result records source identities, exact commands, unit/scenario counts, real-process observations, controlled-event limits, sanitized messaging/usage evidence, concrete owner-routed failures, and removal of owned processes/terminals/state watchers. Candidate3 remains held regardless of preparation success; only the lead may issue NEXT.

## Executed notifier evidence — 2026-10-26

Evidence root: `/home/mu/.cache/rustr-integration/rustr-prep1-bju4vezq`. `notifier-source.json` pins the tested source/handoff hashes; `candidate3-boundary.json` records the untouched product evidence boundary.

- Owner's exact unittest command: **15 passed**, 0.065 s test time (0.209 s process time), exit 0. `logs/notifier-unit.log` and `notifier-unit.json`.
- Independent actual CLI processes: **passed** latest-state coalescing, restart persistence, hourly deferral, definite transport failure with retained pending/reservation, six-process serialization (one delivered/five idle), and status/output/state redaction. `notifier-cli/report.json` records every CLI invocation. Hermes/config/usage were synthetic for these scenarios; the hourly exact-boundary and crash injection are unit-scenario evidence.
- Actual configured-home probe: **passed**, using the shared production state. No recipient identifier or secret was printed. `recipient-verification.json`.
- **Exactly one authorized real preparation delivery:** `send-once --status-file /tmp/rustr-preparation-status.txt`, exit 0, Hermes-confirmed `delivered` at **2026-10-26T14:21:06.409232Z**. Shared durable state remains `~/.local/state/rustr-lead-notify`; no pending update. Next eligible attempt is **2026-10-26T15:21:06.409232Z**, not an instruction to send automatically. `live-send-attempt.json`, `live-delivery.json`, `live-status.json`.
- Usage attached by the actual notifier retained provider observation time **2026-10-26T14:17:51Z**, rather than replacing it with collection time. Claude: 5h 7/100 used, reset 15:10Z; 7d 29/100 and 7d Fable 4/100, reset 2026-11-01T13:00Z; one additional observation explicitly unavailable. Codex primary: 58/100 used, reset 2026-11-02T06:00:45Z. These are historical observed values, not current quota promises.

No notifier repair was required. No recurring job, model turn, cron, live product provider, browser or saved-owner-session action ran. The live delivery confirms Hermes's receipt, not that the user has read it; the documented uncertain-send/exactly-once limitation remains.

## Executed guard and actual OMP evidence

- Exact guard selector on the pinned official Node24 image: **12 passed**, zero failures, 127.023 ms test-runner duration. `guard-unit.json` and `logs/guard-unit.log`. Covered main/session pinning, depth-zero advisor exclusion, content/todo identity, durable/branch-aware deduplication, cancelled preparation, compaction/handoff events, pause/aborted stop, observation races and visible persistence failures.
- Actual OMP `18.12.6`: **four clean disposable process lifecycles** in `omp-run1/report.json`, 9.17 s overall. Baseline → clean shutdown → attach guard to the same journal → clean shutdown/resume → mismatched-pin non-lead. The existing journal remained a byte-prefix of later history, and a separate ambient customization remained available before and after guard attachment.
- Actual startup notice and explicit `/lead-guard handoff` behaved correctly: one notice for changed synthetic content, no duplicate for unchanged content or resumed context. An abort followed by 3.12 s bounded quiescence showed no resumption; all runtime frames contained **zero agent starts and zero extension errors**.
- A separate actual no-pin process also passed: inactive status, no reminder, zero guard journal entries and zero model turns. `omp-unpinned/report.json`. Sub/advisor identities and real-compaction event payloads were controlled unit scenarios, not spawned advisor processes.
- Actual runtime state accepted exactly **`anthropic/claude-opus-5-5`, `medium`**. A read-only registry probe verified `advisor.enabled=false`, `prewalk.enabled=false`, `prewalk.auto="off"` and `retry.modelFallback=false` both before and after attachment. No credentials were copied and no inference request was issued. This is selector/effective-settings acceptance, not live provider entitlement or completion proof.
- No actual model-generated `/handoff` or compaction ran. The actual command exercised was the guard's local handoff check; automatic compaction/handoff handling has controlled-event coverage. A passive reminder does not enforce human/model compliance, establish semantic alignment or grant authority.

### Same-saved-lead transition recipe — not executed

The installed loading contract requires restart for an extension not imported at startup; toggling settings does not dynamically import it. The validated method is **graceful idle shutdown, then exact-file continuation with the added extension**, not a fork/new/clear and not a second concurrent writer.

Read-only Herdr process metadata showed the current real launch as `omp --model astra`, cwd `/home/mu/src/rustr`. No existing custom launch flags were removed. Global auth, memory, model-role and ambient-extension configuration was not changed or copied into the test.

Only after an explicit TRANSITION token:

1. Reach a genuine idle boundary: no streaming, compaction, queued input or owned background work. The lead performs its focused in-place handoff separately, then waits for completion. Do not send `/handoff` while busy.
2. Gracefully exit that CLI and confirm it has relinquished the journal before starting another writer. RPC EOF/drain/exit 0 was tested; interactive double-`Ctrl+C` is the documented TUI exit, not an exercised action on the real lead. Do not hard-kill an active writer.
3. From the same pane, existing environment/profile and repository cwd, use:

```sh
omp --session /home/mu/.omp/agent/sessions/-src-rustr/2026-10-24T06-22-20-764Z_01a16c93-145c-7687-afe2-1878eb005742.jsonl \
  --config /home/mu/src/rustr/scripts/lead-workflow/lead-session.yml \
  --extension /home/mu/src/rustr/scripts/lead-workflow/lead-guard.ts \
  --lead-guard-session 01a16c93-145c-7687-afe2-1878eb005742 \
  --lead-guard-decisions docs/plans/p5-web/decisions.md,docs/plans/p5-web/implementation-contract.md \
  --model anthropic/claude-opus-5-5 --thinking medium
```

There is deliberately **no `--no-extensions`** or disposable isolation flag in this real-lead command. Existing ambient customizations remain enabled. The overlay changes only the four documented safety settings; it does not persist global values or enable the inactive Astra-xhigh advisor role. If the launch/environment changes before transition, preserve its unrelated options rather than mechanically replacing them with this earlier inventory.

4. Verify actual session ID/file/history, selected provider/model/medium and effective settings after launch; command submission alone is not proof. `/lead-guard` is a local inspection command. Keep “alignment unverified” honest. Inspect any real extension/setting conflict and stop/report it rather than disabling ambient extensions, changing auth/memory/roles, choosing a fallback or creating another session.
5. Remain held until the lead separately issues the product batch NEXT token. No additional Telegram preparation test is needed.

The real lead's current ambient extensions were not reloaded during verification. Preservation was exercised with an owned ambient customization; real-lead conflict acceptance remains an observation at the authorized transition, not a claim from the fixture.

### Cleanup and retained boundaries

All five disposable OMP processes exited cleanly; owned runtime home/workspace/native-cache directories and synthetic transport executables were removed. No pane, recurring job or cron was created, and the labeled Node container is gone. Logs, source identities, reports and synthetic journal evidence remain under the evidence root. `cleanup.json` records the unchanged candidate3 source-manifest and pause-receipt identities. Shared production notifier state remains intact, with the single delivery receipt and hourly cap preserved. No product command or saved-lead mutation occurred.
