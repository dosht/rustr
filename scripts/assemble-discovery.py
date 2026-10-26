#!/usr/bin/env python3
"""Builds transgate/docs/{rules,keep-throw,parity,pgtap-map}.md from the topic
files in transgate/docs/discovery/ and lints them.

Usage: scripts/assemble-discovery.py [--check]

--check validates topic completeness, row formats and pgTAP coverage without
writing. Without it, valid input rewrites the four files; invalid input fails.
"""
import os
import re
import sys
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DISCOVERY = os.path.join(ROOT, "transgate", "docs", "discovery")
OUT = os.path.join(ROOT, "transgate", "docs")
PGTAP_DIR = "/home/mu/Business/Transgate/frontend/supabase/tests"

LEDGER_ID = re.compile(r"^[A-Z]{2,4}-\d{3}$")
LONG_DASHES = "–—―"
VOICE_DEMO_DECISION = "decision:docs/plans/p4-ai/decisions.md#d6-exclude-only-the-inventoried-voice-demo"
RULE_PREFIXES = {
    "BIL": "Billing and packages", "FRE": "Freemium", "CPN": "Coupons and currency",
    "JOB": "Job lifecycle", "UPL": "Upload and sources", "TRX": "Transcription",
    "TRL": "Translation", "EDT": "Editing and speaker labels", "EXP": "Exports",
    "AI": "AI features", "PRV": "Privacy and retention", "IDN": "Identity and auth",
    "API": "Public API", "RTL": "Rate limits", "LNG": "Languages", "EML": "Emails",
    "CRN": "Schedules", "FLG": "Flags", "SEO": "Static site",
    "DAT": "Data preservation and test infrastructure",
}
PARITY_SURFACES = ["web-desktop", "web-phone", "cli", "tui", "android", "ios", "api", "email", "cron", "locale"]
REQUIRED_TOPICS = {
    "screens", "static-pages", "api", "billing", "jobs-transcription",
    "languages", "ai", "privacy-identity", "data-model", "platform",
}
REQUIRED_SECTIONS = {
    "Inventory", "Rules", "Defects", "Keep / throw", "Parity rows",
    "Research", "Needs mu", "Open questions for the design",
}


def split_cells(line):
    s = line.strip()
    if s.startswith("|"):
        s = s[1:]
    if s.endswith("|"):
        s = s[:-1]
    return [c.strip() for c in s.split("|")]


def sections(text):
    """Yields (heading, lines) for every ## section; heading '' for the preamble."""
    out, head, buf = [], "", []
    for line in text.splitlines():
        if line.startswith("## "):
            out.append((head, buf))
            head, buf = line[3:].strip(), []
        else:
            buf.append(line)
    out.append((head, buf))
    return out


def table_rows(lines):
    rows = []
    for i, line in enumerate(lines):
        if not line.strip().startswith("|"):
            continue
        cells = split_cells(line)
        if all(re.fullmatch(r":?-{2,}:?", c) for c in cells if c) and any(cells):
            continue
        rows.append((i, cells))
    return rows


def is_header(cells):
    first = cells[0].lower() if cells else ""
    return first in ("id", "item", "test file", "need", "name", "table", "route")


def load_topics():
    topics = {}
    for name in sorted(os.listdir(DISCOVERY)):
        if not name.endswith(".md") or name == "README.md":
            continue
        with open(os.path.join(DISCOVERY, name), encoding="utf-8") as f:
            topics[name[:-3]] = f.read()
    return topics


def lint_topics(topics, problems):
    for topic in sorted(REQUIRED_TOPICS - topics.keys()):
        problems.append(f"missing topic: {topic}.md")
    for topic, text in topics.items():
        content = dict(sections(text))
        for heading in sorted(REQUIRED_SECTIONS):
            if not any(line.strip() for line in content.get(heading, [])):
                problems.append(f"{topic}.md: missing or empty section {heading}")
        if re.search(r"^\s*pending\s*$", text, re.M | re.I):
            problems.append(f"{topic}.md: unfinished pending section")


def collect(topics):
    rules, defects, keep, parity, problems = [], [], [], [], []
    for topic, text in topics.items():
        for ch in LONG_DASHES:
            if ch in text:
                problems.append(f"{topic}.md: contains a long dash (U+{ord(ch):04X})")
        for head, lines in sections(text):
            h = head.lower()
            rows = [(i, c) for i, c in table_rows(lines) if not is_header(c)]
            if "parity" in h:
                for i, c in rows:
                    if len(c) != 6:
                        problems.append(f"{topic}.md: parity row with {len(c)} cells near line offset {i}: {c[0]}")
                        continue
                    parity.append((topic, c))
            elif "defect" in h:
                for i, c in rows:
                    if len(c) != 4:
                        problems.append(f"{topic}.md: defect row with {len(c)} cells: {c[0]}")
                        continue
                    defects.append((topic, c))
            elif "keep" in h and "throw" in h:
                for i, c in rows:
                    if len(c) != 5:
                        problems.append(f"{topic}.md: keep/throw row with {len(c)} cells: {c[0]}")
                        continue
                    keep.append((topic, c))
            elif h.startswith("rules") or h == "rules":
                for i, c in rows:
                    if not LEDGER_ID.match(c[0]):
                        continue
                    if len(c) != 6:
                        problems.append(f"{topic}.md: rule row {c[0]} has {len(c)} cells, expected 6")
                        continue
                    rules.append((topic, c))
    return rules, defects, keep, parity, problems


def lint_rules(rules, problems):
    seen = {}
    by_prefix = defaultdict(list)
    for topic, c in rules:
        rid = c[0]
        if rid in seen:
            problems.append(f"duplicate rule id {rid} in {topic}.md and {seen[rid]}.md")
        seen[rid] = topic
        prefix, num = rid.split("-")
        if prefix not in RULE_PREFIXES:
            problems.append(f"{topic}.md: unknown prefix in {rid}")
        by_prefix[prefix].append(int(num))
        if not c[2] or c[2].lower() in ("tbd", "none", "?"):
            problems.append(f"{topic}.md: {rid} has no source")
        status = c[5].lower()
        if status == "excluded":
            if rid != "AI-027" or c[4] != VOICE_DEMO_DECISION:
                problems.append(f"{topic}.md: {rid} has no named voice-demo scope exclusion")
        elif status not in {"ledger", "tested", "changed"}:
            problems.append(f"{topic}.md: {rid} has unsupported status '{c[5]}'")
        elif status != "ledger" and c[4].lower() in {"", "tbd", "none", "?"}:
            problems.append(f"{topic}.md: {rid} claims '{status}' without test evidence")
    for prefix, nums in by_prefix.items():
        nums = sorted(nums)
        missing = sorted(set(range(1, nums[-1] + 1)) - set(nums))
        if missing:
            problems.append(f"{prefix}: numbering gaps at {missing[:10]}{' ...' if len(missing) > 10 else ''}")


def lint_parity(parity, problems):
    seen = {}
    for topic, c in parity:
        pid = c[0]
        if pid in seen:
            problems.append(f"duplicate parity id {pid} in {topic}.md and {seen[pid]}.md")
        seen[pid] = topic
        if c[2] not in PARITY_SURFACES:
            problems.append(f"{topic}.md: parity row {pid} has surface '{c[2]}'")


def assertion_description(sql, start):
    """Read a call without confusing nested SQL, quoted text or adjacent calls."""
    token = re.compile(
        r"--[^\n]*|/\*[\s\S]*?\*/|'(?:[^']|'')*'|"
        r"(?P<tag>\$(?:[A-Za-z_]\w*)?\$)[\s\S]*?(?P=tag)|[()]"
    )
    depth, description = 1, ""
    for match in token.finditer(sql, start):
        value = match.group()
        if value == "(":
            depth += 1
        elif value == ")":
            depth -= 1
            if depth == 0:
                return description
        elif value.startswith("'") and depth == 1:
            description = value[1:-1].replace("''", "'")
    raise ValueError("unterminated pgTAP assertion")


def pgtap_assertions():
    """Every assertion in the pgTAP files: (file, line, kind, description)."""
    out = []
    if not os.path.isdir(PGTAP_DIR):
        raise ValueError(f"pgTAP source directory is unavailable: {PGTAP_DIR}")
    kinds = r"(ok|is|isnt|throws_ok|lives_ok|results_eq|has_table|has_column|has_function|has_view|is_empty|cmp_ok|has_index|col_is_null|col_not_null|has_extension|col_is_fk|pass|fail)"
    pat = re.compile(r"^[ \t]*SELECT\s+" + kinds + r"\s*\(", re.I | re.M)
    for name in sorted(os.listdir(PGTAP_DIR)):
        if not name.endswith(".sql"):
            continue
        path = os.path.join(PGTAP_DIR, name)
        with open(path, encoding="utf-8") as f:
            sql = f.read()
        for match in pat.finditer(sql):
            line = sql.count("\n", 0, match.start()) + 1
            desc = assertion_description(sql, match.end())
            out.append((name, line, match.group(1).lower(), desc))
    return out


def pgtap_map(rules, assertions):
    """Map explicit assertion lines/ranges; nearby lines are not coverage."""
    refs = []
    for topic, c in rules:
        cell = c[3]
        if not cell or cell.lower() == "none":
            continue
        for part in re.split(r"[;,]\s*", cell):
            m = re.search(r"(\d{3}-[a-z0-9-]+\.sql)(?::(\d+)(?:-(\d+))?)?", part)
            if m:
                refs.append((c[0], m.group(1), int(m.group(2)) if m.group(2) else None, int(m.group(3)) if m.group(3) else None, part))
            else:
                refs.append((c[0], None, None, None, part))
    rows = []
    for name, line, kind, desc in assertions:
        ids = sorted({
            r[0] for r in refs
            if r[1] == name and r[2] is not None
            and r[2] <= line <= (r[3] if r[3] is not None else r[2])
        })
        how = "line" if ids else "unmapped"
        rows.append((name, line, kind, desc, ids, how))
    return rows


def write_rules(rules, defects, topics_order):
    by_prefix = defaultdict(list)
    for topic, c in rules:
        by_prefix[c[0].split("-")[0]].append((topic, c))
    lines = []
    lines.append("# Rules ledger")
    lines.append("")
    lines.append("One row per business rule of Transgate, copied from the current code, not redesigned. Assembled by `scripts/assemble-discovery.py` from the topic files in `discovery/`; edit the topic file, then rerun the script. PRD section 7.2 is the contract: implemented rules need executable Rust evidence. The named AI-027 voice-demo scope exclusion retains its row and decision instead of pretending to be implemented.")
    lines.append("")
    lines.append("Columns: ID, the rule in plain English, the source in the current code (path relative to `/home/mu/Business/Transgate`, with line, or a migration), the pgTAP test that proves it today (`none` when no SQL test exists), executable Rust evidence (`tbd` until written) or the named scope decision, and status. Status values: `ledger` (captured in P0), `tested` (a Rust test exists and passes), `changed` (documented approved outcome correction), and `excluded` (only AI-027 under P4 D6; not implemented behavior).")
    lines.append("")
    lines.append("## Summary")
    lines.append("")
    lines.append("| Prefix | Area | Rows | Topic file |")
    lines.append("|---|---|---|---|")
    total = 0
    for prefix in sorted(by_prefix, key=lambda p: list(RULE_PREFIXES).index(p) if p in RULE_PREFIXES else 99):
        rows = by_prefix[prefix]
        total += len(rows)
        files = sorted({t for t, _ in rows})
        lines.append(f"| {prefix} | {RULE_PREFIXES.get(prefix, '?')} | {len(rows)} | {', '.join(f'`discovery/{t}.md`' for t in files)} |")
    lines.append(f"| | Total | {total} | |")
    lines.append("")
    for prefix in sorted(by_prefix, key=lambda p: list(RULE_PREFIXES).index(p) if p in RULE_PREFIXES else 99):
        lines.append(f"## {prefix}: {RULE_PREFIXES.get(prefix, prefix)}")
        lines.append("")
        lines.append("| ID | Rule | Source | pgTAP | Rust test | Status |")
        lines.append("|---|---|---|---|---|---|")
        for topic, c in sorted(by_prefix[prefix], key=lambda tc: tc[1][0]):
            lines.append("| " + " | ".join(c) + " |")
        lines.append("")
    lines.append("## Defects")
    lines.append("")
    lines.append("Current behavior that looks like a defect. The current behavior stays unless mu picks the proposal; a picked proposal moves the related rows to status `changed`.")
    lines.append("")
    lines.append("| ID | Current behavior | Intended behavior | Proposal | Topic |")
    lines.append("|---|---|---|---|---|")
    for topic, c in defects:
        lines.append("| " + " | ".join(c) + f" | {topic} |")
    lines.append("")
    return "\n".join(lines) + "\n"


def write_keep(keep):
    by_topic = defaultdict(list)
    counts = defaultdict(int)
    for topic, c in keep:
        by_topic[topic].append(c)
        counts[c[2].split()[0].lower()] += 1
    lines = ["# Keep / throw", "", "What of Transgate is knowledge to keep, code to port, something to simplify, or something to throw. Assembled by `scripts/assemble-discovery.py` from `discovery/`; edit there. PRD section 11 asks for this before anything is ported.", "", "Verdicts: `keep-knowledge` (a rule, lesson or ADR, no code), `port` (rewrite in Rust or Svelte with the same behavior), `simplify` (port with a named reduction), `throw` (dropped, with the reason), `gap` (the new system will not have it at first; mu sees it here).", "", "## Summary", "", "| Verdict | Rows |", "|---|---|"]
    for v in ["keep-knowledge", "port", "simplify", "throw", "gap"]:
        lines.append(f"| {v} | {counts.get(v, 0)} |")
    other = sum(n for v, n in counts.items() if v not in ("keep-knowledge", "port", "simplify", "throw", "gap"))
    if other:
        lines.append(f"| other | {other} |")
    lines.append(f"| Total | {len(keep)} |")
    lines.append("")
    for topic in sorted(by_topic):
        lines.append(f"## {topic}")
        lines.append("")
        lines.append("| Item | Where today | Verdict | Reason | Lands in |")
        lines.append("|---|---|---|---|---|")
        for c in by_topic[topic]:
            lines.append("| " + " | ".join(c) + " |")
        lines.append("")
    return "\n".join(lines) + "\n"


def write_parity(parity):
    by_surface = defaultdict(list)
    for topic, c in parity:
        by_surface[c[2]].append((topic, c))
    lines = ["# Parity suite", "", "One row per user flow, endpoint, email, schedule and locale of the current app, per surface. Assembled by `scripts/assemble-discovery.py` from `discovery/`; edit there. PRD section 7.3 is the contract: the Transgate gate fails on any row without a green test.", "", "Columns: ID, the flow, the surface, where it lives today, the Playwright spec or Rust test that proves it (`tbd` until written), and status: `inventoried` (P0), `tested`, `dropped` (mu decided; the reason is in keep-throw.md). CLI, TUI, Android and iOS rows describe applicable current behavior to carry to new shells, not existing native or terminal implementations.", "", "## Summary", "", "| Surface | Rows |", "|---|---|"]
    for s in PARITY_SURFACES:
        if by_surface.get(s):
            lines.append(f"| {s} | {len(by_surface[s])} |")
    lines.append(f"| Total | {len(parity)} |")
    lines.append("")
    for s in PARITY_SURFACES:
        if not by_surface.get(s):
            continue
        lines.append(f"## {s}")
        lines.append("")
        lines.append("| ID | Flow | Surface | Today | Test | Status | Topic |")
        lines.append("|---|---|---|---|---|---|---|")
        for topic, c in sorted(by_surface[s], key=lambda tc: tc[1][0]):
            lines.append("| " + " | ".join(c) + f" | {topic} |")
        lines.append("")
    return "\n".join(lines) + "\n"


def write_pgtap(mapped):
    lines = ["# pgTAP map", "", "Every assertion in `frontend/supabase/tests/*.sql` and the ledger rows that cover it. PRD section 7.2: every pgTAP test maps to at least one row. `How` is `line` only when an explicit assertion start line or inclusive range covers it. Missing coverage fails the discovery check. Generated by `scripts/assemble-discovery.py`.", ""]
    by_file = defaultdict(list)
    for row in mapped:
        by_file[row[0]].append(row)
    lines += ["## Summary", "", "| File | Assertions | Mapped | Unmapped |", "|---|---|---|---|"]
    for name in sorted(by_file):
        rows = by_file[name]
        un = sum(1 for r in rows if not r[4])
        lines.append(f"| {name} | {len(rows)} | {len(rows) - un} | {un} |")
    lines.append(f"| Total | {len(mapped)} | {sum(1 for r in mapped if r[4])} | {sum(1 for r in mapped if not r[4])} |")
    lines.append("")
    for name in sorted(by_file):
        lines.append(f"## {name}")
        lines.append("")
        lines.append("| Line | Kind | Assertion | Rows | How |")
        lines.append("|---|---|---|---|---|")
        for _, line, kind, desc, ids, how in by_file[name]:
            lines.append(f"| {line} | {kind} | {desc.replace('|', '\\|')} | {', '.join(ids) if ids else ''} | {how} |")
        lines.append("")
    return "\n".join(lines) + "\n"


def main():
    check = "--check" in sys.argv
    topics = load_topics()
    rules, defects, keep, parity, problems = collect(topics)
    lint_topics(topics, problems)
    lint_rules(rules, problems)
    lint_parity(parity, problems)
    try:
        assertions = pgtap_assertions()
    except ValueError as error:
        problems.append(str(error))
        assertions = []
    if not assertions:
        problems.append("no pgTAP assertions found")
    mapped = pgtap_map(rules, assertions)
    unmapped = [r for r in mapped if not r[4]]
    for name, line, kind, _, _, _ in unmapped:
        problems.append(f"unmapped pgTAP assertion: {name}:{line} ({kind})")
    print(f"topics: {len(topics)}  rules: {len(rules)}  defects: {len(defects)}  keep/throw: {len(keep)}  parity: {len(parity)}  pgTAP assertions: {len(assertions)} (unmapped {len(unmapped)})")
    for p in problems:
        print("problem:", p)
    if problems:
        return 1
    if not check:
        with open(os.path.join(OUT, "rules.md"), "w", encoding="utf-8") as f:
            f.write(write_rules(rules, defects, list(topics)))
        with open(os.path.join(OUT, "keep-throw.md"), "w", encoding="utf-8") as f:
            f.write(write_keep(keep))
        with open(os.path.join(OUT, "parity.md"), "w", encoding="utf-8") as f:
            f.write(write_parity(parity))
        with open(os.path.join(OUT, "pgtap-map.md"), "w", encoding="utf-8") as f:
            f.write(write_pgtap(mapped))
        print("wrote rules.md, keep-throw.md, parity.md, pgtap-map.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
