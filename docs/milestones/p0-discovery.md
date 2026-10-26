# P0: discovery before the Transgate port

Status: complete on 2026-10-24. The product is not implemented by this milestone. The contract is `docs/prd.md`; the phase board is `WORKING-ON.md`.

## Handover and ownership

On 2026-10-24 Astra took over from the quota-stopped Claude session. Existing discovery files, four language fixtures and `transgate/docs/production-facts.md` were retained. Historical lead/subagent transcripts remain available locally; completed topics are not being rediscovered.

Two visible Astra High agents completed the remaining discovery in herdr in the same checkout, without branches or worktrees:

- `p0-backend`: jobs/transcription, API and AI discovery.
- `p0-surfaces`: screens, static pages and platform discovery.
- Lead: reconciliation, discovery checker, consolidated outputs, research summary and book.

Source repositories and production are read-only for this phase. No new production request was made during the Astra handover. Claude review is deferred until quota resets; it is not a prerequisite for finishing discovery artifacts.

## Verification exercised

- `python3 -B -m unittest discover -s scripts -p test_assemble_discovery.py -v`: six tests passed. These cover refusing incomplete inputs without overwriting reports, missing SQL sources, adjacent assertion descriptions, nested/dollar-quoted SQL, omitted assertion kinds, and exact line/range coverage.
- Before completion, `python3 -B scripts/assemble-discovery.py --check` correctly exited 1 for missing/unfinished topics. After reconciliation, both generation and `--check` exited 0: 10 topics, 526 ledger obligations, 152 proposed defects, 206 keep/throw decisions, 388 parity rows, 143 pgTAP assertions and zero unmapped assertions.
- A read-only fixture/source comparison passed for all 16 UI locale codes, 61 transcription language names/codes, 53 translation targets and provider membership for all 61 languages. Source: `frontend/next-i18next.config.js`, `src/domain/languages.ts` and provider `capabilities.ts`. This does not verify live provider support.
- The final ledger scan found all 253 referenced source paths. Seven out-of-bounds citation ranges found during integration were corrected against the source, including a comma-separated second range. Existence and bounds checks do not prove every claim's semantics.
- Each extracted SQL assertion count matches its file's declared plan, across eight files. The 143 assertions were inventoried, not executed against a database.
- A read-only source/data count confirmed 4527 unique sitemap snapshot URLs, 33 Pages API handlers plus seven App routes, and 39 processor family endpoints. The processor also has health. The surfaces worker reconciled the source/config-derived sitemap set against the snapshot with zero normalized set differences.
- `./scripts/build-book.sh --check`: passed, reporting `snippets ok, 16 chapters, 10 diagrams`. Chapter 14 now records the evidence hierarchy, safe assembly and discovered runtime boundary.

## Corrections to the discovery process

The original checker returned success despite unfinished sections and unmapped SQL assertions. It also omitted `has_extension`, `col_is_fk` and `pass`, read descriptions from a fixed 12-line window, and accepted nearby line numbers as coverage.

The checker now requires all ten topics and eight nonempty sections, rejects standalone `pending`, counts those assertion kinds, extracts descriptions within a balanced call, and maps explicit assertion start lines or inclusive ranges only. Unmapped assertions or inaccessible SQL sources fail. Invalid input never overwrites consolidated reports.

DAT ledger rows classify legacy schema/setup obligations separately from business rules. They preserve migration entities, ownership and balance comparison requirements, but do not mandate old table names, cross-module foreign keys or pgTAP in production.

## Reconciled observations

The production probe from the prior session supersedes several code-only assumptions:

- The four language values absent from migrations exist in production. This is schema drift, not a confirmed production language-save failure.
- Soniox auto delete is enabled.
- Observed chat limits are 50 questions/20000 tokens per transcript and 500 questions/50000 tokens per user per day.
- Four users are blocked, four have unlimited history and sixteen opted out of auto delete.
- Deprecated job statuses have nine live rows; the migration must preserve them rather than assume none exist.
- API keys and callbacks were recently used. API version usage remains unknown, so compatibility is a cutover decision, not a justified default deletion.

These are dated observations, not permanent constants. Grants, triggers, cron state, provider-column defaults and complete auth settings were not established by the REST probe.

## Deliverables and limits

The ten source topics live in `transgate/docs/discovery/`. Their integrated entry points are `inventory.md`, `rules.md`, `keep-throw.md`, `parity.md`, `pgtap-map.md` and `research.md` under `transgate/docs/`. R1 through R5 are answered to the available source and production-probe evidence; four language fixtures are retained.

Discovery is complete, not product parity. Ledger and parity test columns intentionally remain `tbd` until their implementation phases. Proposed defect corrections still need implementation and, where business behavior is ambiguous, an explicit decision. No Rust, web, provider or migration gate is claimed green.

The eight non-Soniox languages need real-provider acceptance evidence before cutover; source capability lists are not that evidence. Active API users need a compatibility or migration decision. A database snapshot and deployed settings are still required for migration parity, grants/triggers, storage, cron, auth mail and provider defaults. Current REST observations cannot replace them.

The media-tool processor remains a runtime obligation with seven families, 39 endpoints plus health and temporary-file cleanup. A static CMS build cannot replace it. Voice demo remains excluded under the PRD assumption; intentional SEO pruning is preserved rather than silently redesigned.

The next phase is P1 foundation. Its gates must report missing implementation honestly rather than reuse the discovery check as proof of production readiness.
