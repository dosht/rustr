# P0 discovery: shared brief

Read `docs/prd.md` sections 2, 6, 7, 9 first. This folder holds the detailed findings of Phase 0, one file per topic. The assembled documents (`../inventory.md`, `../rules.md`, `../keep-throw.md`, `../parity.md`, `../research.md`) are built from these files, so every row here must be complete and sourced.

## Ground rules for every topic file

- Read only. Nothing in `/home/mu/Business/Transgate/**` is modified. Nothing is run that writes anywhere except this folder (and `transgate/fixtures/` for the languages topic).
- Every claim has a source: `path:line` relative to the Transgate repo (`frontend/src/...`, `frontend/supabase/migrations/...`, `supabase-routines/DDL.sql`, `Infra/...`). A migration counts as a source. Line numbers are required for code, optional for whole migrations.
- Business rules are copied as they are today. Do not improve them. If today's behavior looks like a defect, record it in the Defects table with current behavior and intended behavior, and a proposed decision. Only mu decides.
- Plain English, short sentences, no long dashes anywhere (a hook blocks the write).
- Facts over opinions. Counts and lists are exact, taken from the code, not from marketing docs. `docs/important-files/TRANSGATE-PRODUCT-REFERENCE.txt` and `docs/product/feature-specifications.md` are hints, the code is the truth.
- When something is unknowable without a secret or production access, write it under a `## Needs mu` heading with the workaround assumed.

## Write as you go

Usage limits can stop an agent at any moment. So write the topic file early and grow it:

1. Within your first few reads, write the file with all nine headings and `pending` under each.
2. After each area you finish reading, replace that section's `pending` with its content (Edit or a small heredoc append). Never hold more than one section's worth of findings only in memory.
3. If you are resumed after a stop, read your own topic file first and continue from the first `pending` section.

## Row formats

Rules ledger rows (prefix per topic, three digits, gapless):

```
| ID | Rule | Source | pgTAP | Rust test | Status |
| BIL-001 | A package is open when now is between valid_from and valid_to and remaining minutes are above zero | supabase-routines/DDL.sql:41 | 020-balance-functions-tests.sql:67 | tbd | ledger |
```

Status is always `ledger` in P0. `pgTAP` is `none` when no SQL test covers the rule. Every pgTAP assertion in `frontend/supabase/tests/*.sql` must appear in at least one row of the topic that owns it (billing owns 020, 030, 040, 060, 080, 090; data model owns 000, 050). Cite the exact assertion start line or an explicit inclusive line range. A file name alone or a nearby line does not prove coverage.

Prefixes: BIL billing and packages, FRE freemium, CPN coupons and currency, JOB job lifecycle, UPL upload and sources, TRX transcription, TRL translation, EDT transcript editing and speaker labels, EXP exports, AI ai features, PRV privacy and retention, IDN identity and auth, API public API, RTL rate limits, LNG languages, EML emails, CRN schedules, FLG flags that change business behavior, SEO static site rules.

DAT is reserved for data preservation and test infrastructure obligations. These rows classify legacy schema/setup assertions rather than requiring the new product to copy table names or install pgTAP in production.

Defects table:

```
| ID | Current behavior | Intended behavior | Proposal |
```

Keep/throw rows:

```
| Item | Where today | Verdict | Reason | Lands in |
```

Verdict is one of `keep-knowledge` (a rule, lesson or ADR to carry, no code), `port` (code or shape to rewrite in Rust), `simplify` (port with a named reduction), `throw` (dropped, with the reason), `gap` (something the new system will not have at first, named so mu can see it).

Parity rows (user flows, endpoints, emails, crons, locales):

```
| ID | Flow | Surface | Today | Test | Status |
```

Surface is one of `web-desktop`, `web-phone`, `cli`, `tui`, `android`, `ios`, `api`, `email`, `cron`, `locale`. Prefix flow ids with the topic (`WEB-`, `API-`, `EML-`, `CRN-`, `LOC-`). Test is `tbd` and Status is `inventoried` in P0. One flow that exists on desktop and phone is two rows.

## File layout of a topic file

1. `# <Topic>` and a three line summary of what exists today.
2. `## Inventory` tables (screens, routes, tables, whatever the topic owns), with sources.
3. `## Rules` ledger rows.
4. `## Defects`.
5. `## Keep / throw`.
6. `## Parity rows`.
7. `## Research` answers for the research questions the topic owns (R1 to R5).
8. `## Needs mu`.
9. `## Open questions for the design` (short, for the P1+ agents).

## Completion check

Run `python3 scripts/assemble-discovery.py --check`. It requires all ten topics, all eight nonempty sections, no standalone `pending`, valid ledger/parity rows, accessible SQL sources and zero unmapped pgTAP assertions. Missing topics or coverage return a nonzero exit status. A successful check proves discovery structure and traceability, not that the port exists or the old SQL tests pass.

Run `python3 scripts/assemble-discovery.py` to generate the four consolidated files only after input validates. Invalid input never overwrites previous reports. Regression checks: `python3 -B -m unittest discover -s scripts -p test_assemble_discovery.py`.

The lead owns the generated files, `inventory.md`, `research.md`, the board and book. Workers own only their assigned topic files. For this handover, the visible backend worker owns jobs/API/AI and the surfaces worker owns screens/static/platform. Report completion to the lead; do not start another discovery survey.
