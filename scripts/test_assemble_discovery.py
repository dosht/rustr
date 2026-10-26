"""Behavioral checks for the discovery gate; no production database required."""
import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "discovery", Path(__file__).with_name("assemble-discovery.py")
)
discovery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(discovery)


class DiscoveryGateTests(unittest.TestCase):
    def test_implemented_rules_require_evidence_and_known_status(self):
        for status, reference, valid in (
            ("ledger", "tbd", True),
            ("tested", "cargo:billing-core:allocation::exact_balance", True),
            ("changed", "cargo:billing-core:payment::recovered_once", True),
            ("tested", "tbd", False),
            ("changed", "", False),
            ("done", "cargo:billing-core:allocation::exact_balance", False),
        ):
            with self.subTest(status=status, reference=reference):
                problems = []
                discovery.lint_rules(
                    [("billing", ["BIL-001", "Rule", "source:1", "none", reference, status])],
                    problems,
                )
                self.assertEqual(not problems, valid, problems)

    def test_only_the_named_voice_rule_can_use_a_scope_exclusion(self):
        reference = "decision:docs/plans/p4-ai/decisions.md#d6-exclude-only-the-inventoried-voice-demo"
        rules = [
            ("ai", [f"AI-{number:03}", "Rule", "source:1", "none", "tbd", "ledger"])
            for number in range(1, 28)
        ]
        rules[-1][1][4:] = [reference, "excluded"]
        problems = []
        discovery.lint_rules(rules, problems)
        self.assertEqual(problems, [])
        rules[-1][1][4] = "decision:unapproved.md#anything"
        problems = []
        discovery.lint_rules(rules, problems)
        self.assertTrue(problems)
        rules[-1][1][4:] = ["tbd", "ledger"]
        rules[-2][1][4:] = [reference, "excluded"]
        problems = []
        discovery.lint_rules(rules, problems)
        self.assertTrue(problems)

    def test_incomplete_input_fails_without_overwriting_reports(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            topics = root / "topics"
            topics.mkdir()
            (topics / "ai.md").write_text("# AI\n\n## Inventory\n\npending\n")
            sql = root / "sql"
            sql.mkdir()
            (sql / "000-setup.sql").write_text("SELECT ok(true, 'unmapped');\n")
            report = root / "rules.md"
            report.write_text("previous valid report\n")
            with patch.multiple(discovery, DISCOVERY=str(topics), PGTAP_DIR=str(sql), OUT=str(root)), patch("sys.argv", ["assemble-discovery.py"]), contextlib.redirect_stdout(io.StringIO()):
                result = discovery.main()
            self.assertEqual(result, 1)
            self.assertEqual(report.read_text(), "previous valid report\n")
            self.assertFalse((root / "parity.md").exists())

    def test_missing_sql_source_cannot_be_green(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(discovery, "PGTAP_DIR", str(Path(directory) / "absent")):
                with self.assertRaises(ValueError):
                    discovery.pgtap_assertions()

    def test_descriptions_do_not_bleed_between_calls(self):
        sql = "SELECT has_table('public', 'jobs', 'Jobs exist');\nSELECT has_table('public', 'users', 'Users exist');"
        start = sql.index("(") + 1
        self.assertEqual(discovery.assertion_description(sql, start), "Jobs exist")

    def test_nested_sql_and_quotes_do_not_end_assertion(self):
        sql = "SELECT throws_ok($body$ SELECT '('; DO $$ BEGIN NULL; END $$; $body$, 'P0001', 'Expected', 'Owner''s error');"
        self.assertEqual(discovery.assertion_description(sql, sql.index("(") + 1), "Owner's error")
        with self.assertRaises(ValueError):
            discovery.assertion_description("SELECT ok(true, 'unfinished'", 10)

    def test_schema_and_deprecated_assertions_are_included(self):
        with tempfile.TemporaryDirectory() as directory:
            Path(directory, "000-setup.sql").write_text(
                "SELECT has_extension('pgtap', 'Available');\n"
                "SELECT col_is_fk('public', 'jobs', 'user_id', 'Owned');\n"
                "SELECT pass('Deprecated behavior');\n"
            )
            with patch.object(discovery, "PGTAP_DIR", directory):
                assertions = discovery.pgtap_assertions()
        self.assertEqual([(r[1], r[2], r[3]) for r in assertions], [
            (1, "has_extension", "Available"),
            (2, "col_is_fk", "Owned"),
            (3, "pass", "Deprecated behavior"),
        ])

    def test_adjacent_and_file_only_references_do_not_claim_coverage(self):
        rules = [
            ("billing", ["BIL-001", "Rule", "source:1", "050-schema.sql:10", "tbd", "ledger"]),
            ("billing", ["BIL-002", "Rule", "source:2", "050-schema.sql", "tbd", "ledger"]),
            ("billing", ["BIL-003", "Rule", "source:3", "050-schema.sql:20-22", "tbd", "ledger"]),
        ]
        assertions = [("050-schema.sql", n, "ok", "Description") for n in (10, 11, 20, 22, 23)]
        mapped = discovery.pgtap_map(rules, assertions)
        self.assertEqual([row[4] for row in mapped], [["BIL-001"], [], ["BIL-003"], ["BIL-003"], []])


if __name__ == "__main__":
    unittest.main()
