# Lead workflow notifications — PREP-1

## Ownership and evidence

p1V owns only `scripts/lead-workflow/notify.py`, `notify_recipient.py`, `notify_test.py` and this note. p22's `lead-guard.ts` is independent and never invokes the notifier or external transports. Existing product/shell source is unchanged by this assignment.

Status: **implementation and regression source ready; no worker tests, smoke runs, or delivery executed**. Y owns independent unit/scenario/runtime validation and exactly one authorized short preparation delivery. No delivery is claimed here. No agent, daemon, cron, watcher, gateway, browser or model loop was started.

Read-only discovery on 2026-10-26:

- Actual `~/.local/bin/hermes send --help` documents `send --to telegram --file - --json`, configured home resolution, stdin input, and exits 0/1/2.
- Actual `hermes send --list telegram --json` exited 0. Configured home matched a discovered Telegram DM and an explicitly allowed user. Recipient values, names, credentials and raw results were not printed or persisted in this note.
- Actual `omp usage --json --redact --provider anthropic` and `--provider openai-codex` both exited 0. JSON has `reports[].provider`, `fetchedAt`, `limits[].id`, `amount.{used,limit,unit}`, and `window.resetsAt`; timestamps are epoch milliseconds. Account metadata and reset credits are not quota windows.
- Installed Hermes source confirms its send loader's `.env`/config precedence (`hermes_cli/send_cmd.py`, `_load_hermes_env`) and effective `HomeChannel.chat_id/thread_id` (`gateway/config.py`). CLI exit 0 can also mean `skipped`; notifier requires JSON `success: true` without `error` or `skipped` before advancing delivery.

Discovery snapshots are not current usage claims. The notifier obtains fresh observations only through the commands below. It never calls `omp usage reset`, spends a reset credit, scrapes native TUIs, or touches the Claude `/usage` pane.

## Commands for the lead and Y

Python 3 stdlib on Linux. The recipient probe uses the already-installed Hermes Python environment, not a new dependency installation. Default paths:

- Hermes CLI: `~/.local/bin/hermes`
- Hermes package/interpreter: `~/.hermes/hermes-agent` and its `venv/bin/python`
- Private durable state: `~/.local/state/rustr-lead-notify` (0700), `notify.json` and `notify.lock` (0600)

Global options `--state-dir`, `--hermes`, `--hermes-root`, `--omp` precede the subcommand. Normal/live invocations must share the same durable state directory. A separate state directory is an independent cap, so never rotate it to force a send.

From the repository root, define a shell convenience array (no service or persistent configuration):

```bash
notify=(python3 scripts/lead-workflow/notify.py)
```

**Recipient verification, no send:** Y first confirms the configured home is the intended user's recipient, then runs:

```bash
"${notify[@]}" verify-recipient --confirm-configured-home
```

The probe uses Hermes's own config loader and channel directory. It requires an enabled Telegram configuration, a discovered DM matching the configured home, and that home explicitly present in `TELEGRAM_ALLOWED_USERS`. It outputs only a boolean and an opaque target fingerprint to the parent; notifier prints only `configured-recipient-verified`. No chat ID is copied into argv, reports, or notifier state. The same probe runs immediately before each eligible send; a changed home/thread fingerprint requires explicit verification again. Missing config/venv, unsupported config API, undiscovered home, group target, or absent explicit allowlist is a missing prerequisite, not permission to guess another recipient. Never run Hermes's unredacted target list directly into a public report.

**Normal lead-invoked batches:** create a curated one-line UTF-8 status file, not a log. For example:

```bash
printf '%s\n' 'Preparation tooling source saved; independent validation remains in progress. Product work stays paused.' > /tmp/rustr-preparation-status.txt
"${notify[@]}" queue --status-file /tmp/rustr-preparation-status.txt
"${notify[@]}" observe
"${notify[@]}" status
"${notify[@]}" flush
```

`queue` stores/replaces the latest meaningful status and never sends. The lead decides what is meaningful; no LLM summarizes logs. Repeating the queued or last-delivered text does not create another notification. Returning to the last-delivered text cancels an obsolete pending update. `observe` reads both provider snapshots without sending; usage changes alone never queue a message. `status` shows a sanitized preview, timestamped quota observations, pending flag, delivery time and next attempt eligibility. `flush` sends the pending message only when eligible. Deferred work stays queued until a later lead invocation; there is no timer or autonomous wakeup.

**Y's one authorized live test, distinct command:** after recipient verification and local acceptance, use the same production state directory and a short preparation status file:

```bash
"${notify[@]}" send-once --status-file /tmp/rustr-preparation-status.txt
```

`send-once` performs queue → observe → flush once, then exits. It does not bypass the hourly cap, create a recurring task or send on unchanged/idle state. Use **either** the earlier `flush` example **or** this one-shot command for the single live acceptance delivery, not both. Y must inspect the result and record actual receipt/delivery evidence without recipient IDs. `deferred`, `idle` and `unchanged` are explicitly not delivery evidence.

## Delivery and privacy contract

- One shared `flock` covers queue/state mutations and the whole send transaction across processes. Atomic same-directory replacement plus file/directory `fsync` persists state; unreadable or unknown-version state fails closed rather than clearing the cap.
- A durable attempt reservation is saved **before** invoking Hermes. A send attempt is eligible only 3600 seconds after both the last attempt and the last confirmed delivery. Thus failure, timeout, restart and concurrent callers cannot release the cap; clock rollback delays rather than accelerates eligibility.
- Success clears pending and advances `delivered_digest`/`delivered_at` only after Hermes's positive JSON receipt. Failure/timeout/skip keeps pending and leaves the delivered marker unchanged. Conservative tradeoff: even a definite failed send occupies the attempt cap for one hour. No automatic retries.
- Transport is exclusively `hermes send --to telegram --file - --json`, message over stdin, timeout 30 seconds. No Telegram HTTP client, provider inference call, or gateway dependency. Raw subprocess stdout/stderr is captured and discarded, never logged or saved. Operational errors return nonzero with sanitized diagnostics.
- CLI result JSON uses `queued`, `unchanged`, `idle`, `deferred`, `observed`, `status`, `configured-recipient-verified`, `delivered`, or `error`. Exit 0 means the requested operation completed, not necessarily delivery; only `result: delivered` means Hermes confirmed it. Exit 1 is an operational/input failure; argparse usage errors exit 2.
- Status is one curated line, max 500 characters; links, email addresses, long numeric/hex identifiers, common credential labels/prefixes, control characters and `MEDIA:` directives are rejected. No raw logs are accepted as status. This is a guard, **not a universal secret detector**: lead must never supply confidential text or arbitrary account IDs. Reports over 3500 characters fail before sending rather than splitting into spam.

## Usage observation schema

Only this sanitized projection is stored per provider; no account IDs, emails, organization names, raw labels, provider errors, endpoints or reset-credit fields survive:

```json
{
  "checked_at": 1793023524,
  "available": true,
  "observations": [
    {
      "observed_at": 1793023332.196,
      "windows": [
        {"name": "5h", "used": 7, "limit": 100, "reset_at": 1793027400.086}
      ]
    }
  ],
  "unavailable_accounts": 1
}
```

This is the shape of the actual observed Claude snapshot, not a current quota promise. Multiple returned reports remain separate anonymous observations; no guessed account association or aggregate quota. Known meter IDs are Claude 5h/7d/7d Fable and Codex primary/secondary; numeric `percent` amounts are copied directly as used/limit, never estimated from session tokens. Unknown meter/amount shapes render unavailable, not invented percentages. Missing reset is `unavailable`, never replaced by a credit count or computed from duration. Future observation timestamps are refused; observations one hour old are marked stale. A past reset is displayed as past/refresh-needed, not treated as an automatic quota refill. Failed reads keep the last timestamped observation and explicitly mark latest fetch unavailable; with no prior observation everything is unavailable.

`observe` invokes exactly:

```bash
omp usage --json --redact --provider anthropic
omp usage --json --redact --provider openai-codex
```

OMP may return a cached `fetchedAt`; notifier preserves that original timestamp rather than presenting collection time as provider freshness. `flush` does not silently refresh quota; lead runs `observe` at a meaningful batch or uses `send-once`.

## Independent acceptance handoff

Y runs:

```bash
python3 -m unittest discover -s scripts/lead-workflow -p 'notify_test.py' -v
```

Regression source covers latest-status coalescing/restart, no-change/idle suppression, exact hourly boundary, reverting to delivered status, failed/unconfirmed/skipped sends, crash after reservation, clock rollback, unverified/changed recipient refusal, corrupted-state refusal, six concurrent processes sharing one local fake transport, usage privacy/staleness/missing reset/unknown windows, sensitive-status rejection, subprocess timeout/error redaction. Tests use temporary state and synthetic transport; none sends externally.

Y additionally owns actual CLI scenarios (queue/status/observe/flush and send-once), read-only effective-recipient verification, real OMP observation parsing, restart/failure/concurrency checks, and the one authorized Hermes delivery. Source readiness is not runtime acceptance. No build, test, formatter or notifier CLI was executed by p1V; only read-only discovery commands described above were executed. No owned running jobs remain at source handoff.

## Limits and failure recovery

Exactly-once external delivery is impossible with Hermes's CLI receipt and a separate local state file. A process can die after Telegram accepted the message but before the local success marker is saved. The reservation still blocks attempts for an hour, and `attempt_outcome: uncertain` signals ambiguity. A later explicit flush may duplicate that message after the cap expires; inspect Telegram first rather than blindly retrying. Never delete state to resolve ambiguity. No receipt/account identifier is retained automatically.

Recipient verification and send read the same Hermes configuration via separate processes; do not change Hermes profiles/home/credentials concurrently with a flush. Local state assumes the invoking OS user and a local filesystem supporting `flock`/atomic replace/fsync; malicious same-user mutation and cross-machine shared state are not supported. The probe intentionally depends on the installed Hermes config API and fails closed if that API changes. All source/product/browser work remains held.
