#!/usr/bin/env python3
"""Check PRD evidence and execute the tests it names. Missing work is a failure."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import re
import runpy
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
DISCOVERY = runpy.run_path(str(ROOT / "scripts/assemble-discovery.py"))


def rows(path):
    active = False
    for line in path.read_text().splitlines():
        if not line.startswith("|"):
            active = False
            continue
        cells = [cell.strip() for cell in re.split(r"(?<!\\)\|", line.strip()[1:-1])]
        if cells[0] == "ID":
            active = cells[1] in {"Rule", "Flow", "Scenario"}
        elif active and not all(re.fullmatch(r"[-:]+", cell) for cell in cells):
            yield cells


def approved_drop(reference, identity):
    prefix = "keep-throw:"
    if not reference.startswith(prefix):
        return False
    for line in (ROOT / "transgate/docs/keep-throw.md").read_text().splitlines():
        cells = [cell.strip() for cell in re.split(r"(?<!\\)\|", line.strip()[1:-1])]
        if len(cells) == 5 and cells[0] == reference[len(prefix):] and cells[2] == "throw":
            return identity in cells[3] and "mu approved" in cells[3].lower()
    return False


def approved_voice_exclusion(reference, identity):
    """Only the two retained voice-demo rows have this delegated scope decision."""
    if identity not in {("rules.md", "AI-027"), ("parity.md", "API-F44")}:
        return False
    if reference != DISCOVERY["VOICE_DEMO_DECISION"]:
        return False
    decision = ROOT / "docs/plans/p4-ai/decisions.md"
    return decision.is_file() and "\n## D6 Exclude only the inventoried voice demo\n" in decision.read_text()


def ledger_integrity():
    assembly = DISCOVERY
    topics = assembly["load_topics"]()
    rules, _, _, parity, problems = assembly["collect"](topics)
    assembly["lint_topics"](topics, problems)
    assembly["lint_rules"](rules, problems)
    assembly["lint_parity"](parity, problems)
    expected = {
        "rules.md": [tuple(cells) for _, cells in rules],
        "parity.md": [tuple(cells + [topic]) for topic, cells in parity],
    }
    for name, source in expected.items():
        path = ROOT / "transgate/docs" / name
        if not path.is_file() or sorted(map(tuple, rows(path))) != sorted(source):
            problems.append(f"{name} is missing or differs from discovery source rows; regenerate it")
    return problems


def run(command, cwd=ROOT):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    if result.stdout:
        print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr)
    return result


def cargo_evidence(references, include_ignored=False):
    """A successful process with zero selected tests is not evidence."""
    by_package = defaultdict(set)
    errors = []
    for reference in references:
        match = re.fullmatch(r"cargo:([A-Za-z0-9_-]+):([^\s]+)", reference)
        if not match:
            errors.append(f"invalid Rust test reference: {reference}")
        else:
            by_package[match[1]].add(match[2])
    for package, expected in sorted(by_package.items()):
        command = ["cargo", "test", "--locked", "-p", package, "--", "--color", "never"]
        if include_ignored:
            command.append("--include-ignored")
        result = run(command)
        if result.returncode:
            errors.append(f"cargo tests failed: {package}")
        passed = set(re.findall(r"^test (\S+) \.\.\. ok$", result.stdout, re.MULTILINE))
        for name in sorted(expected - passed):
            errors.append(f"test did not pass (absent, ignored or failed): cargo:{package}:{name}")
    return errors


def playwright_evidence(references):
    directory = ROOT / "transgate/web"
    if not (directory / "package.json").is_file():
        return ["Transgate web shell is not implemented: transgate/web/package.json"]
    result = subprocess.run(
        ["npx", "--no-install", "playwright", "test", "--reporter=json"],
        cwd=directory, text=True, capture_output=True,
    )
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr)
    errors = []
    if result.returncode:
        errors.append("Transgate Playwright suite failed")
    try:
        report = json.loads(result.stdout)
    except json.JSONDecodeError:
        return errors + ["Playwright did not emit a JSON report"]
    outcomes = defaultdict(list)

    def visit(suite, parents):
        # File suites supply the path separately, not a describe-title prefix.
        titles = parents + ([suite["title"]] if suite.get("title") and suite.get("line", 0) else [])
        for spec in suite.get("specs", []):
            title = " ".join(titles + [spec["title"]])
            key = f"playwright:{spec['file']}#{title}"
            for test in spec.get("tests", []):
                outcomes[key].append((test.get("projectName"), test.get("expectedStatus"), test.get("status")))
        for child in suite.get("suites", []):
            visit(child, titles)

    for suite in report.get("suites", []):
        visit(suite, [])
    for reference in references:
        tests = outcomes.get(reference, [])
        projects = {project for project, _, _ in tests}
        if not {"desktop", "mobile"} <= projects or any(
            expected != "passed" or status != "expected" for _, expected, status in tests
        ):
            errors.append(f"test must pass on desktop and mobile without skips/expected failures: {reference}")
    return errors


def playwright_specs(references):
    """A browser-only rule names a committed Transgate spec and test title, as the
    Playwright JSON report keys it: `playwright:<spec path under transgate/web>#<describe
    titles and test title, space-joined>`. The file must exist and declare a test whose
    title ends the reference; execution is checked separately."""
    errors = []
    for reference in sorted(references):
        match = re.fullmatch(r"playwright:([^#]+\.spec\.ts)#(.+)", reference)
        if not match:
            errors.append(f"invalid Playwright reference: {reference}")
            continue
        spec = ROOT / "transgate/web" / match[1]
        if not spec.is_file():
            errors.append(f"Playwright spec is not committed: {reference}")
            continue
        declared = re.findall(r"""\btest(?:\.\w+)?\(\s*(["'`])(.+?)\1""", spec.read_text())
        if not any(match[2] == title or match[2].endswith(" " + title) for _, title in declared):
            errors.append(f"Playwright spec declares no test matching {match[2]!r}: {reference}")
    return errors


def coverage(kind):
    errors, rust, web = [], set(), set()
    count = 0
    paths = [ROOT / "docs/scenarios.md"] if kind == "scenarios" else [
        ROOT / "transgate/docs/rules.md", ROOT / "transgate/docs/parity.md"
    ]
    if kind == "billing":
        paths = [ROOT / "transgate/docs/rules.md"]
    ids = set()
    for path in paths:
        if not path.is_file():
            errors.append(f"missing {path.relative_to(ROOT)}")
            continue
        local_count = 0
        for cells in rows(path):
            if not cells or not cells[0]:
                errors.append(f"unnamed obligation in {path.relative_to(ROOT)}")
                continue
            if kind == "billing" and not cells[0].startswith(("BIL-", "FRE-", "CPN-")):
                continue
            required = 5 if kind == "scenarios" else (6 if path.name == "rules.md" else 7)
            if len(cells) != required:
                errors.append(f"{cells[0]}: expected {required} columns, got {len(cells)}")
                continue
            local_count += 1
            identity = (path.name, cells[0])
            if identity in ids:
                errors.append(f"duplicate row: {identity}")
            ids.add(identity)
            reference, status = (cells[3], cells[2]) if kind == "scenarios" else (cells[4], cells[5])
            if path.name == "rules.md" and status == "excluded":
                if not approved_voice_exclusion(reference, identity):
                    errors.append(f"{cells[0]}: excluded rule lacks the named voice-demo scope decision")
                continue
            if path.name == "parity.md" and status == "dropped":
                if not (approved_voice_exclusion(reference, identity) or approved_drop(reference, cells[0])):
                    errors.append(f"{cells[0]}: dropped flow lacks approved keep/throw evidence or the named voice-demo scope decision")
                continue
            accepted = {"done"} if kind == "scenarios" else {"tested", "changed"}
            if status not in accepted:
                errors.append(f"{cells[0]}: status is {status}, not implemented/verified")
                continue
            if kind == "scenarios":
                chapter = ROOT / cells[4].split("#", 1)[0]
                if not cells[4].startswith("book/chapters/") or not chapter.is_file():
                    errors.append(f"{cells[0]}: missing book chapter {cells[4]}")
            for test in reference.split("; "):
                if test.startswith("cargo:"):
                    rust.add(test)
                elif test.startswith("playwright:"):
                    web.add(test)
                else:
                    errors.append(f"{cells[0]}: missing or invalid executable test reference: {test}")
        count += local_count
        if not local_count:
            errors.append(f"no obligation rows found: {path.relative_to(ROOT)}")
    if kind == "scenarios":
        expected = {("scenarios.md", f"SC-{i:02}") for i in range(1, 18)}
        if ids != expected:
            errors.append("scenario catalog must contain exactly SC-01 through SC-17")
    else:
        errors += ledger_integrity()
        if kind == "billing":
            expected = {
                ("rules.md", f"{prefix}-{number:03}")
                for prefix, total in (("BIL", 79), ("FRE", 25), ("CPN", 19))
                for number in range(1, total + 1)
            }
            if ids != expected:
                errors.append("P2 requires exactly BIL-001..079, FRE-001..025 and CPN-001..019")
    print(f"{kind}: {count} rows, {len(errors)} unmet requirements before executing named tests")
    errors += cargo_evidence(rust, include_ignored=True)
    if web:
        errors += playwright_specs(web)
        errors += playwright_evidence(web)
    return errors


def module_graph(metadata, required=("billing", "cms")):
    packages = {p["id"]: p for p in metadata["packages"]}
    graph = {n["id"]: {d["pkg"] for d in n["deps"]} for n in metadata["resolve"]["nodes"]}
    root = Path(metadata["workspace_root"])
    groups = defaultdict(set)
    consumers = defaultdict(set)
    for ident in metadata["workspace_members"]:
        package = packages[ident]
        path = Path(package["manifest_path"]).relative_to(root)
        if path.parts[0] == "modules" and len(path.parts) > 2:
            groups[path.parts[1]].add(ident)
        elif path.parts[0] == "transgate":
            consumers["transgate"].add(ident)
        elif path.parts[0] == "crates" and package["name"].startswith("taskr-"):
            consumers["taskr"].add(ident)

    def reachable(start):
        seen, pending = set(), list(graph.get(start, ()))
        while pending:
            node = pending.pop()
            if node not in seen:
                seen.add(node)
                pending.extend(graph.get(node, ()))
        return seen

    forbidden = consumers["taskr"] | consumers["transgate"]
    errors = []
    for module in required:
        if not groups[module]:
            errors.append(f"module not implemented: modules/{module}")
        for consumer in ("taskr", "transgate"):
            if groups[module] and not any(reachable(p) & groups[module] for p in consumers[consumer]):
                errors.append(f"{consumer} does not consume modules/{module}")
    for module, members in groups.items():
        for member in members:
            violations = reachable(member) & forbidden
            if violations:
                errors.append(f"{packages[member]['name']} depends on host app: " + ", ".join(sorted(packages[p]["name"] for p in violations)))
    return errors, sorted(packages[p]["name"] for module in required for p in groups[module])


def modules(required=("billing", "cms")):
    result = subprocess.run(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT, text=True, capture_output=True)
    if result.returncode:
        print(result.stderr, file=sys.stderr)
        return ["cargo metadata failed"]
    errors, packages = module_graph(json.loads(result.stdout), required)
    for package in packages:
        if run(["cargo", "test", "--locked", "-p", package, "--", "--include-ignored"]).returncode:
            errors.append(f"module tests failed: {package}")
    print(f"modules: {len(packages)} workspace packages")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=("scenarios", "ledgers", "modules", "billing"))
    parser.add_argument("--module", choices=("billing", "cms"), help="Phase-specific module proof; default requires both modules")
    args = parser.parse_args()
    if args.module and args.kind != "modules":
        parser.error("--module is only valid with modules")
    try:
        required = (args.module,) if args.module else ("billing", "cms")
        errors = modules(required) if args.kind == "modules" else coverage(args.kind)
    except (OSError, ValueError, KeyError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    for error in errors[:20]:
        print(f"FAIL: {error}", file=sys.stderr)
    if len(errors) > 20:
        print(f"... {len(errors) - 20} more unmet requirements", file=sys.stderr)
    return int(bool(errors))


if __name__ == "__main__":
    sys.exit(main())
