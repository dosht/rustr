#!/usr/bin/env python3
"""Y-owned execution: python3 -m unittest discover -s scripts/lead-workflow -p 'notify_test.py' -v"""
import json
import multiprocessing
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from notify import HOUR, Notifier, NotifyError, run_json, status_text, usage_lines, usage_snapshot

FINGERPRINT = "a" * 64
NOW = 1793023524


def provider_report(provider="anthropic", observed=NOW):
    return {"reports": [{"provider": provider, "fetchedAt": observed * 1000,
                         "metadata": {"accountId": "PRIVATE_ACCOUNT", "email": "private@example.test"},
                         "resetCredits": {"availableCount": 99},
                         "limits": [{"id": "anthropic:5h" if provider == "anthropic" else "openai-codex:primary",
                                     "label": "PRIVATE_LABEL", "status": "PRIVATE_STATUS",
                                     "amount": {"used": 7, "limit": 100, "unit": "percent"},
                                     "window": {"resetsAt": (NOW + 7200) * 1000}}]}],
            "accountsWithoutUsage": [{"accountId": "PRIVATE_ACCOUNT"}]}


class Backend:
    def __init__(self):
        self.sent = []
        self.fingerprint = FINGERPRINT
        self.failure = False
        self.receipt = {"success": True, "chat_id": "PRIVATE_RECIPIENT"}

    def __call__(self, argv, timeout=30, body=None):
        if "notify_recipient.py" in " ".join(argv):
            return {"verified": True, "fingerprint": self.fingerprint}
        if "usage" in argv:
            if self.failure:
                raise NotifyError("synthetic unavailable")
            return provider_report(argv[-1])
        if self.failure:
            raise NotifyError("synthetic transport failure")
        self.sent.append(body)
        return self.receipt


def concurrent_flush(directory, fake, counter, barrier, results):
    def runner(argv, timeout=30, body=None):
        if body is None:
            return {"verified": True, "fingerprint": FINGERPRINT}
        return run_json([sys.executable, fake, counter], body=body)
    notifier = Notifier(directory, runner=runner, clock=lambda: NOW)
    barrier.wait(timeout=10)
    results.put(notifier.flush()["result"])


class NotifierTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.directory = Path(self.tmp.name) / "state"
        self.backend = Backend()
        self.now = NOW
        self.notifier = self.restart()

    def restart(self):
        return Notifier(self.directory, runner=self.backend, clock=lambda: self.now)

    def prepare(self, text="Preparation implementation saved; validation remains with Y."):
        self.notifier.verify_recipient()
        self.notifier.queue(text)

    def test_latest_meaningful_status_survives_restart_and_idle_never_sends(self):
        self.assertEqual(self.notifier.flush(), {"result": "idle"})
        self.prepare("First meaningful batch saved.")
        self.notifier.queue("Latest meaningful batch saved.")
        self.assertEqual(self.restart().flush()["result"], "delivered")
        self.assertIn("Latest meaningful batch saved.", self.backend.sent[0])
        self.assertNotIn("First meaningful batch", self.backend.sent[0])
        self.now += HOUR * 2
        self.assertEqual(self.restart().flush(), {"result": "idle"})
        self.assertEqual(self.restart().queue("Latest meaningful batch saved."), {"result": "unchanged"})
        self.assertEqual(self.restart().flush(), {"result": "idle"})
        self.assertEqual(len(self.backend.sent), 1)

    def test_hourly_boundary_and_restart_preserve_coalesced_latest(self):
        self.prepare("First batch completed.")
        self.notifier.flush()
        self.now += HOUR - 1
        self.notifier.queue("Intermediate batch completed.")
        self.notifier.queue("Final batch completed.")
        self.assertEqual(self.restart().flush()["result"], "deferred")
        self.now += 1
        self.assertEqual(self.restart().flush()["result"], "delivered")
        self.assertEqual(len(self.backend.sent), 2)
        self.assertIn("Final batch completed.", self.backend.sent[-1])

    def test_return_to_delivered_status_cancels_obsolete_pending_status(self):
        self.prepare("Preparation complete.")
        self.notifier.flush()
        self.notifier.queue("Additional work in progress.")
        self.notifier.queue("Preparation complete.")
        self.now += HOUR
        self.assertEqual(self.notifier.flush(), {"result": "idle"})

    def test_failure_retains_pending_without_advancing_delivery(self):
        self.prepare()
        self.backend.failure = True
        with self.assertRaises(NotifyError):
            self.notifier.flush()
        state = self.restart().status()
        self.assertTrue(state["pending"])
        self.assertIsNone(state["delivered_at"])
        self.backend.failure = False
        self.assertEqual(self.restart().flush()["result"], "deferred")
        self.now += HOUR
        self.assertEqual(self.restart().flush()["result"], "delivered")

    def test_skipped_or_unconfirmed_transport_is_not_delivered(self):
        self.prepare()
        for receipt in ({"skipped": True}, {}, {"success": True, "error": "private"}):
            self.backend.receipt = receipt
            with self.assertRaises(NotifyError):
                self.notifier.flush()
            self.assertIsNone(self.notifier.status()["delivered_at"])
            self.assertTrue(self.notifier.status()["pending"])
            self.now += HOUR

    def test_crash_after_reservation_preserves_cap_and_pending(self):
        self.prepare()
        real_save = self.notifier.save
        def crash_after_save(state):
            real_save(state)
            if state.get("attempt_outcome") == "uncertain":
                raise RuntimeError("synthetic process crash before transport")
        with patch.object(self.notifier, "save", crash_after_save), self.assertRaises(RuntimeError):
            self.notifier.flush()
        self.assertEqual(self.restart().flush()["result"], "deferred")
        self.assertEqual(self.backend.sent, [])
        self.assertTrue(self.restart().status()["pending"])

    def test_clock_rollback_cannot_bypass_cap(self):
        self.prepare()
        self.notifier.flush()
        self.notifier.queue("Next batch saved.")
        self.now -= HOUR
        self.assertEqual(self.restart().flush()["result"], "deferred")

    def test_unverified_or_changed_recipient_cannot_send(self):
        self.notifier.queue("Preparation saved.")
        with self.assertRaises(NotifyError):
            self.notifier.flush()
        self.notifier.verify_recipient()
        self.backend.fingerprint = "b" * 64
        with self.assertRaises(NotifyError):
            self.notifier.flush()
        self.assertEqual(self.backend.sent, [])

    def test_corrupt_state_is_not_silently_reset(self):
        self.prepare()
        (self.directory / "notify.json").write_text("broken")
        with self.assertRaises(NotifyError):
            self.restart().flush()
        self.assertEqual(self.backend.sent, [])

    def test_concurrent_processes_share_one_delivery_cap(self):
        self.prepare()
        fake = Path(self.tmp.name) / "fake_send.py"
        counter = Path(self.tmp.name) / "deliveries.jsonl"
        fake.write_text("import json,sys\nfrom pathlib import Path\n"
                        "with Path(sys.argv[1]).open('a') as f: f.write(json.dumps(sys.stdin.read())+'\\n')\n"
                        "print(json.dumps({'success':True}))\n")
        context = multiprocessing.get_context("fork")
        barrier, results = context.Barrier(6), context.Queue()
        workers = [context.Process(target=concurrent_flush,
                                   args=(str(self.directory), str(fake), str(counter), barrier, results)) for _ in range(6)]
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join(15)
            if worker.is_alive():
                worker.kill()
                worker.join()
            self.assertEqual(worker.exitcode, 0)
        outcomes = [results.get(timeout=2) for _ in workers]
        self.assertEqual(outcomes.count("delivered"), 1)
        self.assertEqual(outcomes.count("idle"), 5)
        self.assertEqual(len(counter.read_text().splitlines()), 1)

    def test_provider_observations_are_timestamped_and_never_identity_or_credit_data(self):
        self.prepare()
        self.notifier.observe()
        preview = self.notifier.status()["preview"]
        self.assertIn("7/100% used", preview)
        self.assertIn("reset ", preview)
        state = (self.directory / "notify.json").read_text()
        for forbidden in ("PRIVATE", "private@example.test", "resetCredits", "availableCount", "chat_id"):
            self.assertNotIn(forbidden, preview + state)
        self.now += HOUR
        self.backend.failure = True
        self.notifier.observe()
        preview = self.restart().status()["preview"]
        self.assertIn("stale", preview)
        self.assertIn("latest fetch unavailable", preview)

    def test_missing_reset_is_unavailable_not_a_reset_credit(self):
        raw = provider_report()
        raw["reports"][0]["limits"][0]["window"].pop("resetsAt")
        snapshot = usage_snapshot(raw, "anthropic", NOW)
        rendered = "\n".join(usage_lines({"anthropic": snapshot}, NOW))
        self.assertIn("reset unavailable", rendered)
        self.assertNotIn("99", rendered)
        self.assertIn("Codex: usage/limit/next reset unavailable", rendered)

    def test_future_observation_and_unknown_windows_do_not_invent_usage(self):
        raw = provider_report(observed=NOW + HOUR)
        self.assertFalse(usage_snapshot(raw, "anthropic", NOW)["available"])
        raw = provider_report()
        raw["reports"][0]["limits"][0]["id"] = "private-unknown-meter"
        rendered = "\n".join(usage_lines({"anthropic": usage_snapshot(raw, "anthropic", NOW)}, NOW))
        self.assertIn("usage/limit/next reset unavailable", rendered)
        self.assertNotIn("7/100", rendered)

    def test_status_rejects_logs_credentials_identifiers_and_media_directives(self):
        for text in ("line one\nraw log", "token=private", "person@example.test", "account_id abc",
                     "chat id 123456789", "https://private.example", "MEDIA:/tmp/private", "x" * 501):
            with self.subTest(text=text), self.assertRaises(NotifyError):
                status_text(text)
        self.assertEqual(status_text("Batch saved; two blockers remain."), "Batch saved; two blockers remain.")

    def test_subprocess_errors_never_relay_raw_output(self):
        completed = subprocess.CompletedProcess([], 1, "PRIVATE_ACCOUNT", "PRIVATE_SECRET")
        with patch("notify.subprocess.run", return_value=completed), self.assertRaises(NotifyError) as error:
            run_json(["synthetic"])
        self.assertNotIn("PRIVATE", str(error.exception))
        with patch("notify.subprocess.run", side_effect=subprocess.TimeoutExpired("synthetic", 1, output="PRIVATE")), self.assertRaises(NotifyError) as error:
            run_json(["synthetic"])
        self.assertNotIn("PRIVATE", str(error.exception))


if __name__ == "__main__":
    unittest.main()
