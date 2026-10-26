#!/usr/bin/env python3
"""Lead-invoked notifications only. Python stdlib; Hermes owns the transport."""
import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

HOUR = 3600
PROVIDERS = {"anthropic": "Claude", "openai-codex": "Codex"}
WINDOWS = {"anthropic:5h": "5h", "anthropic:7d": "7d", "anthropic:7d:fable": "7d Fable",
           "openai-codex:primary": "primary", "openai-codex:secondary": "secondary"}


class NotifyError(Exception):
    pass


def timestamp(seconds):
    return datetime.fromtimestamp(seconds, timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and value >= 0


def status_text(text):
    text = text.strip()
    if not text or len(text) > 500 or any(ord(c) < 32 for c in text):
        raise NotifyError("status must be one curated line of 1..500 characters, not logs")
    # Reject rather than silently send potentially sensitive text. Not a universal DLP filter.
    if re.search(r"(?i)(https?://|www\.|[\w.+-]+@[\w.-]+|\b(?:token|secret|password|api[_ -]?key|account[_ -]?id|chat[_ -]?id)\b|\b\d{6,}\b|\b[0-9a-f]{24,}\b|\b(?:sk-|ghp_|eyJ)[\w.-]+|MEDIA:)", text):
        raise NotifyError("status contains a prohibited identifier, link or credential pattern")
    return text


def run_json(argv, timeout=30, body=None):
    try:
        result = subprocess.run(argv, input=body, text=True, capture_output=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired):
        raise NotifyError("command unavailable or timed out; raw output suppressed") from None
    if result.returncode:
        raise NotifyError("command failed; raw output suppressed")
    try:
        return json.loads(result.stdout)
    except (ValueError, TypeError):
        raise NotifyError("command returned invalid JSON; raw output suppressed") from None


def usage_snapshot(raw, provider, now):
    """Allowlist numeric quota fields; never persist raw reports or account metadata."""
    if not isinstance(raw, dict) or not isinstance(raw.get("reports"), list):
        raise NotifyError("usage snapshot schema unavailable")
    observations = []
    for report in raw["reports"]:
        if not isinstance(report, dict) or report.get("provider") != provider:
            continue
        fetched = report.get("fetchedAt")
        if not number(fetched) or fetched <= 0 or fetched / 1000 > now + 60:
            continue
        windows = []
        limits = report.get("limits")
        for item in limits if isinstance(limits, list) else []:
            if not isinstance(item, dict) or item.get("id") not in WINDOWS:
                continue
            amount, window = item.get("amount"), item.get("window")
            if not isinstance(amount, dict) or not isinstance(window, dict):
                continue
            used, limit = amount.get("used"), amount.get("limit")
            if not number(used) or not number(limit) or amount.get("unit") != "percent":
                continue
            reset = window.get("resetsAt")
            windows.append({"name": WINDOWS[item["id"]], "used": used, "limit": limit,
                            "reset_at": reset / 1000 if number(reset) and reset > 0 else None})
        observations.append({"observed_at": fetched / 1000, "windows": windows})
    missing = raw.get("accountsWithoutUsage", [])
    return {"checked_at": now, "available": bool(observations), "observations": observations,
            "unavailable_accounts": len(missing) if isinstance(missing, list) else 0}


def usage_lines(usage, now):
    lines = []
    for provider, label in PROVIDERS.items():
        snapshot = usage.get(provider, {})
        observations = snapshot.get("observations", [])
        if not observations:
            lines.append(f"{label}: usage/limit/next reset unavailable (checked {timestamp(snapshot['checked_at']) if snapshot.get('checked_at') else 'never'}).")
            continue
        if not snapshot.get("available"):
            lines.append(f"{label}: latest fetch unavailable; last observations follow.")
        for index, observation in enumerate(observations, 1):
            observed = observation["observed_at"]
            stale = now - observed >= HOUR or observed > now
            state = "stale" if stale else "observed"
            name = label if len(observations) == 1 else f"{label} observation {index}"
            details = []
            for window in observation["windows"]:
                reset = window["reset_at"]
                reset_text = timestamp(reset) if reset else "unavailable"
                if reset and reset <= now:
                    reset_text += " (past; refresh needed)"
                details.append(f"{window['name']} {window['used']:g}/{window['limit']:g}% used; reset {reset_text}")
            lines.append(f"{name} [{state} {timestamp(observed)}]: " + ("; ".join(details) or "usage/limit/next reset unavailable") + ".")
        if snapshot.get("unavailable_accounts"):
            lines.append(f"{label}: {snapshot['unavailable_accounts']} additional account observation(s) unavailable.")
    return lines


class Notifier:
    def __init__(self, directory, hermes=None, omp="omp", hermes_root=None, clock=time.time, runner=run_json):
        self.directory = Path(directory)
        self.hermes = str(hermes or Path.home() / ".local/bin/hermes")
        self.hermes_root = Path(hermes_root or Path.home() / ".hermes/hermes-agent")
        self.omp, self.clock, self.runner = omp, clock, runner

    @contextmanager
    def locked(self):
        self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        if self.directory.is_symlink() or self.directory.stat().st_uid != os.getuid():
            raise NotifyError("state directory must be private and owned by this user")
        if self.directory.stat().st_mode & 0o077:
            raise NotifyError("state directory must have mode 0700")
        fd = os.open(self.directory / "notify.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            path = self.directory / "notify.json"
            try:
                state = json.loads(path.read_text())
            except FileNotFoundError:
                state = {"version": 1, "usage": {}}
            except (ValueError, OSError):
                raise NotifyError("state unreadable; refusing to reset delivery history") from None
            if not isinstance(state, dict) or state.get("version") != 1:
                raise NotifyError("unsupported state; refusing to reset delivery history")
            yield state

    def save(self, state):
        fd, name = tempfile.mkstemp(prefix="notify-", dir=self.directory)
        try:
            with os.fdopen(fd, "w") as stream:
                json.dump(state, stream, ensure_ascii=False)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(name, self.directory / "notify.json")
            directory_fd = os.open(self.directory, os.O_DIRECTORY)
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
        finally:
            if os.path.exists(name):
                os.unlink(name)

    def recipient(self):
        probe = Path(__file__).with_name("notify_recipient.py")
        result = self.runner([str(self.hermes_root / "venv/bin/python"), str(probe), str(self.hermes_root)])
        if not isinstance(result, dict) or result.get("verified") is not True or not re.fullmatch(r"[a-f0-9]{64}", str(result.get("fingerprint", ""))):
            raise NotifyError("configured Telegram home is not an explicitly allowed recipient")
        return result["fingerprint"]

    def verify_recipient(self):
        with self.locked() as state:
            state["recipient"] = self.recipient()
            self.save(state)
        return {"result": "configured-recipient-verified"}

    def queue(self, text):
        text = status_text(text)
        digest = hashlib.sha256(text.encode()).hexdigest()
        with self.locked() as state:
            # The newest meaningful status wins, including returning to the delivered status.
            if state.get("delivered_digest") == digest:
                state.pop("pending", None)
                outcome = "unchanged"
            elif state.get("pending", {}).get("digest") == digest:
                return {"result": "unchanged"}
            else:
                state["pending"] = {"text": text, "digest": digest, "queued_at": self.clock()}
                outcome = "queued"
            self.save(state)
        return {"result": outcome}

    def observe(self):
        with self.locked() as state:
            usage = state.setdefault("usage", {})
            for provider in PROVIDERS:
                now = self.clock()
                try:
                    raw = self.runner([self.omp, "usage", "--json", "--redact", "--provider", provider])
                    usage[provider] = usage_snapshot(raw, provider, now)
                except NotifyError:
                    prior = usage.get(provider, {"observations": []})
                    usage[provider] = {**prior, "available": False, "checked_at": now}
            self.save(state)
            return {"result": "observed", "usage": usage_lines(usage, self.clock())}

    def message(self, state):
        pending = state.get("pending")
        if not pending:
            return None
        lines = ["Rustr preparation update", status_text(pending["text"]), *usage_lines(state.get("usage", {}), self.clock())]
        message = "\n".join(lines)
        if len(message) > 3500:
            raise NotifyError("report exceeds short-message limit; no send attempted")
        return message

    def status(self):
        with self.locked() as state:
            now = self.clock()
            last = max(state.get("last_attempt_at", 0), state.get("delivered_at", 0))
            return {"result": "status", "recipient_verified": bool(state.get("recipient")),
                    "pending": bool(state.get("pending")), "delivered_at": state.get("delivered_at"),
                    "next_attempt_at": last + HOUR if last else None,
                    "attempt_outcome": state.get("attempt_outcome"), "preview": self.message(state),
                    "usage": usage_lines(state.get("usage", {}), now)}

    def flush(self):
        with self.locked() as state:
            if not state.get("pending"):
                return {"result": "idle"}
            now = self.clock()
            last = max(state.get("last_attempt_at", 0), state.get("delivered_at", 0))
            if last and now < last + HOUR:
                return {"result": "deferred", "next_attempt_at": last + HOUR}
            if not state.get("recipient") or self.recipient() != state["recipient"]:
                raise NotifyError("recipient unverified or changed; explicit verification required")
            message = self.message(state)
            # Reserve BEFORE external I/O. Crash/timeout cannot release the hourly attempt cap.
            state["last_attempt_at"], state["attempt_outcome"] = now, "uncertain"
            self.save(state)
            try:
                receipt = self.runner([self.hermes, "send", "--to", "telegram", "--file", "-", "--json"], body=message)
                if not isinstance(receipt, dict) or receipt.get("success") is not True or receipt.get("error") or receipt.get("skipped"):
                    raise NotifyError("Hermes did not confirm a delivery")
            except NotifyError:
                state["attempt_outcome"] = "failed-or-uncertain"
                self.save(state)
                raise NotifyError("Hermes delivery failed or uncertain; pending retained and hourly attempt cap preserved") from None
            state["delivered_digest"] = state["pending"]["digest"]
            state["delivered_at"], state["attempt_outcome"] = self.clock(), "delivered"
            state.pop("pending")
            self.save(state)
            return {"result": "delivered", "delivered_at": state["delivered_at"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, default=Path.home() / ".local/state/rustr-lead-notify")
    parser.add_argument("--hermes", default=str(Path.home() / ".local/bin/hermes"))
    parser.add_argument("--hermes-root", type=Path, default=Path.home() / ".hermes/hermes-agent")
    parser.add_argument("--omp", default="omp")
    commands = parser.add_subparsers(dest="command", required=True)
    verify = commands.add_parser("verify-recipient", help="read-only config probe; stores local consent, never sends")
    verify.add_argument("--confirm-configured-home", action="store_true", required=True)
    for name in ("queue", "send-once"):
        cmd = commands.add_parser(name)
        cmd.add_argument("--status-file", type=Path, required=True)
    for name in ("observe", "status", "flush"):
        commands.add_parser(name)
    args = parser.parse_args()
    notifier = Notifier(args.state_dir, args.hermes, args.omp, args.hermes_root)
    try:
        if args.command == "verify-recipient":
            result = notifier.verify_recipient()
        elif args.command in ("queue", "send-once"):
            result = notifier.queue(args.status_file.read_text())
            if args.command == "send-once":
                notifier.observe()
                result = notifier.flush()
        else:
            result = getattr(notifier, args.command)()
        print(json.dumps(result, ensure_ascii=False))
        return 0
    except NotifyError as error:
        # NotifyError messages are fixed, deliberately sanitized boundary errors.
        print(json.dumps({"result": "error", "reason": str(error)}))
        return 1
    except (OSError, ValueError, TypeError, KeyError, OverflowError):
        # Even paths/exceptions can contain sensitive input. Keep stdout machine-readable.
        print(json.dumps({"result": "error", "reason": "operation failed; inspect recipient verification, private state permissions, input and command availability; no raw output retained"}))
        return 1


if __name__ == "__main__":
    sys.exit(main())
