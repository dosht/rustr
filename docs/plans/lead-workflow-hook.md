# PREP-1: passive lead workflow extension

Owner: existing OMP/Astra p22, preserving the saved session and prior P5/Opus source provenance. Authority: [lead-preparation.md](lead-preparation.md). Product/candidate3 work remains held. **Source-authored only: p22 has not executed tests, loaded the extension, changed a running session, or claimed runtime acceptance. Independent Y owns validation and the safe same-session transition recipe.**

## Scope and design

Owned files:

- `scripts/lead-workflow/lead-guard.ts`
- `scripts/lead-workflow/lead-guard.test.ts`
- `scripts/lead-workflow/lead-session.yml` (subsequently authorized session overlay)
- this document

No `hooks.json`, legacy `HookAPI`, ambient extension directory, global config, product source, generated output, telemetry, model calls, new agents or notifier integration. V owns `notify*` and `lead-workflow-notifications.md`; the guard neither imports nor invokes them. Notification queue/status/flush remains standalone and lead-invoked.

The extension uses the current `ExtensionAPI` default factory. It reads content hashes of `WORKING-ON.md`, the authoritative `docs/plans/lead-preparation.md`, explicitly designated current decision/contract files, and canonical current-branch OMP todos. It reminds reconciliation of objective, decisions, authority token, owner, evidence level and next action through `skill://todo-state-machine`. Session todos are the current batch, not a second full roadmap.

It never determines acceptance, rewrites files/todos, or interprets PAUSED/PREP as permission to execute. Changed bytes are an observation worth checking, not proof of semantic disagreement. Unchanged status needs no forced edit. The footer always says **alignment unverified**, including after a reminder; unreadable/missing inputs produce explicit warnings, not an aligned/complete badge. `mtime` is never used as content evidence.

## Explicit activation and designation

Required runtime identity checks are `ctx.agent.kind === "main"` **and** exact equality of `--lead-guard-session` with `ctx.sessionManager.getSessionId()`. A pane name, session title, depth, inherited environment variable, copied journal or inherited flag alone does not authorize a child/advisor or another main session. No environment variable grants lead status. A non-designated main displays an inactive status; subagents/advisors make no UI or journal changes and perform no file observation.

The guard is loaded explicitly with `--extension`; nothing is registered ambiently. The real saved lead retains its existing customizations and ambient extensions, adding only the explicit guard and session overlay. Use `--no-extensions` **only for the disposable smoke**: there it excludes ambient extension discovery while retaining the explicitly supplied module. It is not a sandbox for unrelated OMP discovery systems.

After Y validates, the **lead/Y**, not this implementation worker, can apply the following argument shape to the preserved lead session. Do not run a second process against a session still owned by a live writer. Y must publish the actual stop/restart/resume sequence and verify provider, model, effort and session identity rather than infer them from command submission:

```sh
omp --session /home/mu/.omp/agent/sessions/-src-rustr/2026-10-24T06-22-20-764Z_01a16c93-145c-7687-afe2-1878eb005742.jsonl \
  --config scripts/lead-workflow/lead-session.yml \
  --extension scripts/lead-workflow/lead-guard.ts \
  --lead-guard-session 01a16c93-145c-7687-afe2-1878eb005742 \
  --lead-guard-decisions docs/plans/p5-web/decisions.md,docs/plans/p5-web/implementation-contract.md \
  --model anthropic/claude-opus-5-5 --thinking medium
```

Run from the repository root. Decision files are comma-separated repository-relative paths. They are chosen explicitly; neither latest mtime nor a roadmap phase silently selects a decision. A missing designation warns. Add other accepted decision files explicitly when they become relevant to the current batch. The two fixed checkpoint files are always observed. A missing, unreadable, non-regular or larger-than-1-MiB input warns without blocking conversation. No document content is copied into an instruction; reminders contain only paths, hashes and guard-authored guidance.

The overlay sets `advisor.enabled: false`, `prewalk.enabled: false`, `prewalk.auto: "off"`, `retry.modelFallback: false`, `retry.waitForUsageReset: true`, and loads the Claude usage monitor (below) through `extensions`. It does not replace `modelRoles`, memory, auth or unrelated settings, and it does not set a model itself. It is read via `--config` for this process, never saved globally. Settings precedence still applies: an existing environment/runtime override can outrank an overlay, so Y must inspect **effective** values. The explicit CLI model/effort does not authorize provider fallback. Same-model retries and normal host compaction are not disabled by `retry.modelFallback`.

## Claude usage limit handling

Blocking until a usage reset is done by OMP itself: `retry.waitForUsageReset: true` in `lead-session.yml` and `worker-session.yml`. When the provider reports a usage limit with an authoritative reset time, the turn waits until the reset (abortable with Esc) instead of failing. `retry.modelFallback: false` keeps it on the same model.

`scripts/lead-workflow/claude-usage-gate.ts` is a monitor, not a gate. Before each Anthropic request it reads `omarchy agent usage claude --limits-only` (at most once a minute; freshness from `fetchedAtMs` in `~/.cache/omarchy/agent-usage/claude-limits.json`, not `updatedAt`), shows the most constraining window and its reset time in the status line, and warns once per state at the threshold (default 0.98; `--claude-usage-threshold`). The 5-hour and weekly windows apply to every Claude model; the Fable weekly window to Fable only. Unreadable or stale (over 15 minutes) usage shows as unknown.

- **Why not a hook that sleeps:** the first version held `before_provider_request` until the reset. On 2026-10-27 it hit the 5-hour limit in a worker and OMP cut the handler off every time with "handler timed out after 30000ms": extension handlers have a 30-second budget (`omp://extensions.md`, reliability rules), so a hook cannot hold a request for hours.
- **Loading:** the lead via `lead-session.yml`; workers via `--config /home/mu/src/rustr/scripts/lead-workflow/worker-session.yml`. Settings files load only at process start.
- **Evidence:** `node --experimental-strip-types --test scripts/lead-workflow/claude-usage-gate.test.ts` 8 PASS (no request is held over 1 s; one warning per state; once-a-minute polling; window, freshness and probe rules). Actual `omp -p` with Haiku and `--claude-usage-threshold 0.01` through `worker-session.yml` answered `OK` in 10 s. The reset wait itself has not been observed at a real limit yet.

## Actual API/event seams

| Seam | Behavior |
|---|---|
| `session_start`, `session_switch`, `session_branch`, `session_tree` | Re-read the active branch and observed files; passive UI reminder only for a new observed revision. Invalidate outstanding observations on session/branch changes. |
| `session_before_compact`, `session_compact` | Check before requested compaction and after its new persisted context revision. Aborted pre-compaction is ignored. No compaction is initiated by this extension. |
| `auto_compaction_start` with `action === "handoff"` | Observe the documented handoff action; dedupe the same revision. There is no invented generic `handoff` event. |
| `session_before_switch` | Passive material-change reminder before a session handoff. Never cancels switching. |
| `before_agent_start` with `reason === "prompt"` and nonempty text/images | Return one hidden custom message to an **already requested** turn. No prompt rewriting, input interception, system-prompt replacement or added turn. |
| `session_stop` | Only if material content/todo state differs from the last observation/reminder, notify before yield. An aborted signal is ignored. Always returns no decision/continuation. |
| `/lead-guard` | Explicit operator inspection of current observation/warnings; never starts agent work. |
| `/lead-guard handoff` | Explicit material-change handoff check; unchanged state is quiet. |

Lifecycle and yield reminders are UI notifications, not instructions that wake an idle model. The next nonempty user-work preparation receives model context through the supported return value. This deliberately cannot force a lead to repair a stale board in the same turn after its final response. Forcing that would require a continuation/block, contrary to the passive requirement. There is no model-based classifier for “meaningful”: nonempty prepared user work is the boundary, and the reminder explicitly exempts ordinary conversation and unchanged status from bookkeeping edits.

`sendMessage`, `sendUserMessage`, stop continuations/blocks, timers/watchers, shell execution, provider calls and settings mutations are absent from the implementation. It neither wakes a paused session nor authorizes held work. Headless UI may discard status/notifications; the next actual user-work preparation still gets the hidden reminder. No UI receipt is described as proof the model reconciled anything.

## State, deduplication and branch safety

- `pi.appendEntry("rustr.lead-guard.observation.v1", observation)` persists **observation/reminder metadata only**, never an acceptance bit. Accepted returned messages persist as `custom_message` with `customType: "rustr.lead-guard.reminder.v1"`.
- Reconstruct only from `ctx.sessionManager.getBranch()`, not every entry across abandoned branches. Resume preserves ancestor receipts. A rewind does not import later receipts/todos from an abandoned branch.
- Material identity hashes configured paths, actual file bytes/error states, normalized canonical todo content/status/blockers, and warnings. Context identity additionally includes session ID and the latest compaction/branch-summary/reset entry ID. File timestamps, unrelated journal entries and tool-view echoes do not imply progress.
- Todo sources match the documented runtime: `custom/user_todo_edit` snapshots and successful `message/toolResult` records named `todo`, excluding `op: "view"`. Invalid shapes are ignored; absent canonical state is explicitly unknown, not an invented empty/completed batch.
- Prompt reminders also key the preceding user entry and prepared prompt/image count. Only an **accepted persisted custom message** is a receipt. Cancelled/retried `before_agent_start` preparation does not stamp a delivered reminder; identical re-entry returns the same candidate for the host to accept or discard.
- UI lifecycle receipts dedupe unchanged state across reload/resume. A new compaction context can refresh the reminder without pretending material progress. Before-yield comparison uses material identity only.
- A fork/branch with a **new session ID becomes inactive** until the operator explicitly designates that ID on a subsequent load. Its copied historical state is preserved but does not grant authority or suppress its first new-session reminder. `/tree` within the same pinned session remains active and reads only that branch.
- Generation/session/cwd fences discard an observation that awaited filesystem reads across a session/branch change. Factory-local state prevents child bindings from sharing mutable guard state.
- File failures produce warnings. Unexpected observation/persistence failures produce an unavailable indicator and warning, never a blocked conversation or silently verified state. The host retains control of session persistence and compaction.

## Y validation handoff

No worker execution has occurred. Node's built-in test runner with TypeScript stripping is sufficient; the OMP package import is type-only and adds no runtime dependency/package changes:

```sh
node --experimental-strip-types --test scripts/lead-workflow/lead-guard.test.ts
```

Use Node 24 or another installed release supporting type stripping. The exact file selector contains twelve behavior regressions: pinned/main-only activation including depth-zero advisors; durable resume dedupe; changed bytes with restored mtimes and unchanged bytes with changed mtimes; missing inputs; committed todo precedence; cancelled preparation/accepted-message dedupe; rewind/new-session isolation; compaction and handoff; PAUSED/PREP and aborted stop; observation/session-switch race; persistence failure; invalid or undesignated inputs. Filesystem observations use isolated temporary directories, not mocked document bytes. The handler harness controls lifecycle/session entries and traps any attempted turn/model/exec/timer action. This is regression coverage source, not a claimed real-host smoke.

Useful exact focused selectors (Y only):

```sh
node --experimental-strip-types --test --test-name-pattern='^lead activation requires an exact session pin and main identity, including depth-zero advisors$' scripts/lead-workflow/lead-guard.test.ts
node --experimental-strip-types --test --test-name-pattern='^PAUSED and PREP stay passive and aborted stop cannot resume or block the agent$' scripts/lead-workflow/lead-guard.test.ts
node --experimental-strip-types --test --test-name-pattern='^cancelled policy preparation is not a receipt and accepted reminders dedupe on retry$' scripts/lead-workflow/lead-guard.test.ts
```

### Safe disposable actual-host procedure

Y owns this execution and its receipt. Do **not** modify or resume the saved lead/worker sessions for testing.

1. Create a fresh temporary cwd containing synthetic `WORKING-ON.md`, `docs/plans/lead-preparation.md` and `docs/plans/decisions.md`. Use a separate temporary default-profile agent directory (`PI_CODING_AGENT_DIR`) and no copied auth/global config. Use absolute paths for the real guard/overlay. Keep stdout draining throughout RPC shutdown.
2. Create a disposable session journal using the documented session header (`type: "session"`, `version: 3`, unique `id`, ISO `timestamp`, temporary `cwd`). This is a synthetic fixture, not an edit/copy of an owner journal. Pin that exact ID. Confirm the actual `get_state` sessionId/sessionFile after loading, rather than trusting the filename.
3. Launch from the temporary cwd with `--profile default --mode rpc --session <temporary-journal> --config <absolute-lead-session.yml> --no-extensions --extension <absolute-lead-guard.ts> --lead-guard-session <fixture-id> --lead-guard-decisions docs/plans/decisions.md --model anthropic/claude-opus-5-5 --thinking medium`. Do not pass `--no-ui`, which drops the status/notify frames. Startup/command inspection needs no model completion. If the isolated environment cannot resolve the named model, report that exact prerequisite instead of selecting a fallback or copying credentials silently.
4. Observe actual `extension_ui_request` status/notify frames, no extension error, and idle `get_state`. Inspect effective overlay settings through the installed registry handles (`lookup(id).get(pi.pi.settings)`) in a disposable test harness or the supported read-only config interface; check effective provider/model/medium in `get_state`. No `config set` or global write. Unrelated roles/memory/auth must remain untouched.
5. Exercise `get_entries`, `set_todos`, `prompt` with **only** `/lead-guard` or `/lead-guard handoff`, and `abort`. Extension commands complete locally (`agentInvoked: false`); never submit ordinary text to a provider for this smoke. Change only a synthetic checkpoint/todo snapshot, invoke handoff twice, and observe one material-change notice. Resume the same disposable journal and verify dedupe. Repeat with a mismatched pin and without the flag; no observations/journal writes or reminder context may be attributed to those non-leads.
6. While idle/paused, observe that no `agent_start`, model request, timer-driven continuation or background resumption originates from the guard. Use the regression harness for sub/advisor identity and controlled compaction/handoff if actual host paths would require inference; report the boundary honestly. Only Y may decide/run an additionally authorized real compaction smoke, not this module.
7. Close RPC stdin, drain output and observe clean exit. Remove only the temporary sessions/cwd/agent directory Y created. Preserve receipts. Publish the safe same-session model/extension transition recipe only after these observations; preparation success is not a product batch release.

API sources consumed: [current extension guide](omp://extensions.md), [loading guide](omp://extension-loading.md), [session storage](omp://session.md), [RPC protocol](omp://rpc.md), [settings](omp://settings.md), [configuration layering](omp://config-usage.md), and the current public source definitions for [extension types](https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/extensibility/extensions/types.ts), [shared events](https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/extensibility/shared-events.ts), [canonical todo restoration](https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/tools/todo.ts) and [registered prewalk/retry settings](https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/session/settings.ts). Remote main is supporting source evidence, not a substitute for Y validating the installed host API.

## Next owner

- [ ] @tester Y execute the bounded regressions and disposable extension/overlay smoke; publish exact results and safe transition procedure. No product release or acceptance tick is delegated to this extension.
