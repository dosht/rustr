from pathlib import Path
import runpy
import subprocess
import tempfile
import unittest
from unittest.mock import patch

GATE = runpy.run_path(str(Path(__file__).with_name("check-prd.py")))


class GateTests(unittest.TestCase):
    def test_successful_cargo_with_zero_or_ignored_tests_is_not_evidence(self):
        evidence = GATE["cargo_evidence"]
        for output in ("running 0 tests\ntest result: ok. 0 passed\n", "test rule::boundary ... ignored\n"):
            with self.subTest(output=output), patch.dict(evidence.__globals__, run=lambda *args: subprocess.CompletedProcess([], 0, output, "")):
                self.assertIn("did not pass", evidence({"cargo:billing-core:rule::boundary"})[0])
        with patch.dict(evidence.__globals__, run=lambda *args: subprocess.CompletedProcess([], 1, "test rule::boundary ... ok\n", "")):
            self.assertIn("tests failed", evidence({"cargo:billing-core:rule::boundary"})[0])

    def test_all_obligation_ids_are_parsed_without_guessing_their_shape(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "parity.md"
            path.write_text("| Surface | Rows |\n|---|---|\n| api | 2 |\n\n| ID | Flow | Surface | Today | Test | Status | Topic |\n|---|---|---|---|---|---|---|\n| API-UPLOAD | source a \\| b | api | route | tbd | inventoried | api |\n| WEB-01 | flow | web-phone | page | tbd | inventoried | screens |\n")
            parsed = list(GATE["rows"](path))
            self.assertEqual([row[0] for row in parsed], ["API-UPLOAD", "WEB-01"])
            self.assertEqual(len(parsed[0]), 7)

    def metadata(self):
        paths = {
            "billing": "modules/billing/core/Cargo.toml",
            "cms": "modules/cms/core/Cargo.toml",
            "helper": "modules/billing/postgres/Cargo.toml",
            "taskr": "crates/taskr-server/Cargo.toml",
            "transgate": "transgate/server/Cargo.toml",
        }
        names = {"billing": "billing-core", "cms": "cms-core", "helper": "billing-postgres", "taskr": "taskr-server", "transgate": "transgate-server"}
        edges = {"billing": [], "cms": [], "helper": ["billing"], "taskr": ["helper", "cms"], "transgate": ["billing", "cms"]}
        return {
            "workspace_root": "/repo", "workspace_members": list(paths),
            "packages": [{"id": key, "name": names[key], "manifest_path": "/repo/" + path, "dependencies": []} for key, path in paths.items()],
            "resolve": {"nodes": [{"id": key, "deps": [{"pkg": target, "name": "renamed_dependency"} for target in targets]} for key, targets in edges.items()]},
        }

    def test_transitive_host_dependency_is_rejected_even_when_renamed(self):
        metadata = self.metadata()
        self.assertEqual(GATE["module_graph"](metadata)[0], [])
        metadata["resolve"]["nodes"][0]["deps"] = [{"pkg": "helper", "name": "adapter"}]
        metadata["resolve"]["nodes"][2]["deps"] = [{"pkg": "transgate", "name": "innocent_name"}]
        errors, _ = GATE["module_graph"](metadata)
        self.assertTrue(any("billing-core depends on host app: transgate-server" in error for error in errors), errors)

    def test_single_consumer_does_not_satisfy_module_reuse(self):
        metadata = self.metadata()
        metadata["resolve"]["nodes"][-1]["deps"] = [{"pkg": "billing", "name": "billing"}]
        errors, _ = GATE["module_graph"](metadata)
        self.assertEqual(errors, ["transgate does not consume modules/cms"])

    def test_billing_scope_does_not_require_cms_or_ignore_its_boundary_violations(self):
        metadata = self.metadata()
        metadata["workspace_members"].remove("cms")
        errors, packages = GATE["module_graph"](metadata, ("billing",))
        self.assertEqual(errors, [])
        self.assertEqual(packages, ["billing-core", "billing-postgres"])
        self.assertIn("module not implemented: modules/cms", GATE["module_graph"](metadata)[0])
        metadata["workspace_members"].append("cms")
        metadata["resolve"]["nodes"][1]["deps"] = [{"pkg": "taskr", "name": "hidden_host"}]
        errors, _ = GATE["module_graph"](metadata, ("billing",))
        self.assertTrue(any("cms-core depends on host app: taskr-server" in error for error in errors), errors)

    def test_billing_scope_requires_every_named_rule_without_claiming_other_topics(self):
        coverage = GATE["coverage"]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ledger = root / "transgate/docs/rules.md"
            ledger.parent.mkdir(parents=True)
            header = "| ID | Rule | Source | Legacy test | Rust test | Status |\n|---|---|---|---|---|---|\n"
            obligations = [
                f"| {prefix}-{number:03} | behavior | source | legacy | tbd | ledger |\n"
                for prefix, total in (("BIL", 79), ("FRE", 25), ("CPN", 19))
                for number in range(1, total + 1)
            ]
            ledger.write_text(header + "".join(obligations) + "| JOB-001 | job | source | legacy | tbd | ledger |\n")
            with patch.dict(coverage.__globals__, ROOT=root, ledger_integrity=lambda: []):
                errors = coverage("billing")
                self.assertEqual(len(errors), 123)
                self.assertFalse(any("JOB-001" in error for error in errors))
                ledger.write_text(header + "".join(obligations[:-1]))
                errors = coverage("billing")
                self.assertTrue(any("exactly BIL-001..079" in error for error in errors), errors)

    def test_voice_scope_disposition_is_narrow_and_requires_the_recorded_decision(self):
        coverage = GATE["coverage"]
        reference = "decision:docs/plans/p4-ai/decisions.md#d6-exclude-only-the-inventoried-voice-demo"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            docs = root / "transgate/docs"
            docs.mkdir(parents=True)
            decision = root / "docs/plans/p4-ai/decisions.md"
            decision.parent.mkdir(parents=True)
            decision.write_text("# P4 decisions\n\n## D6 Exclude only the inventoried voice demo\n\nAI-027 and API-F44 are excluded under the delegated scope decision.\n")
            rules_header = "| ID | Rule | Source | Legacy test | Rust test | Status |\n|---|---|---|---|---|---|\n"
            parity_header = "| ID | Flow | Surface | Today | Test | Status | Topic |\n|---|---|---|---|---|---|---|\n"

            def write_rows(rule="AI-027", flow="API-F44", evidence=reference):
                (docs / "rules.md").write_text(rules_header + f"| {rule} | Voice demo | source:1 | none | {evidence} | excluded |\n")
                (docs / "parity.md").write_text(parity_header + f"| {flow} | Voice demo | api | source:1 | {evidence} | dropped | api |\n")

            with patch.dict(coverage.__globals__, ROOT=root, ledger_integrity=lambda: []):
                write_rows()
                self.assertEqual(coverage("transgate"), [])
                for rule, flow, evidence in (
                    ("AI-026", "API-F44", reference),
                    ("AI-027", "API-F43", reference),
                    ("AI-027", "API-F44", "decision:unapproved.md#anything"),
                ):
                    with self.subTest(rule=rule, flow=flow, evidence=evidence):
                        write_rows(rule, flow, evidence)
                        self.assertTrue(coverage("transgate"))
                write_rows()
                decision.write_text("# Unrelated decision\n")
                self.assertTrue(coverage("transgate"))
                decision.unlink()
                self.assertTrue(coverage("transgate"))

    def test_a_scope_decision_cannot_replace_an_executable_test_for_implemented_work(self):
        coverage = GATE["coverage"]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            docs = root / "transgate/docs"
            docs.mkdir(parents=True)
            reference = "decision:docs/plans/p4-ai/decisions.md#d6-exclude-only-the-inventoried-voice-demo"
            (docs / "rules.md").write_text(
                "| ID | Rule | Source | Legacy test | Rust test | Status |\n|---|---|---|---|---|---|\n"
                f"| AI-027 | Voice demo | source:1 | none | {reference} | tested |\n"
            )
            (docs / "parity.md").write_text(
                "| ID | Flow | Surface | Today | Test | Status | Topic |\n|---|---|---|---|---|---|---|\n"
                f"| API-F44 | Voice demo | api | source:1 | {reference} | tested | api |\n"
            )
            with patch.dict(coverage.__globals__, ROOT=root, ledger_integrity=lambda: []):
                errors = coverage("transgate")
            self.assertEqual(sum("missing or invalid executable test reference" in error for error in errors), 2)


if __name__ == "__main__":
    unittest.main()
