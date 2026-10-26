# P3 transcription and privacy advisor brief

P2 source is implemented but under integration repair. It is not yet green. P3 design can proceed independently; no P3 implementation starts from unreconciled proposals. The user approved independent Opus/Astra proposals, cross-critique and then visible developers, and has explicitly authorized more Opus implementation capacity. No code-complete prototype, builds, tests, formatters, hidden subagents, branches or worktrees during this design step.

## Outcome and fixed boundaries

Read PRD sections 2, 3, 6, 7 and P3 in section 8. Deliver P3: a revision-checked Job aggregate, upload/presigned URLs, actual Soniox and Deepgram adapters, shared routing, translation, transcript lines/editing, exports, retention/deletion saga; minimal taskr scenarios for long-running work, provider adapters, scheduler/locks, upload and deletion/retention. Business meaning stays in pure Rust, shells only render or translate protocols. No dependency added to taskr-core. Reuse the existing core/ports/state-machine pattern and billing module, rather than a second framework.

Reusable modules remain billing and CMS; do not casually add a generic workflow engine or move product rules into a reusable abstraction. Decide the smallest reusable scenario seam for taskr versus the product's full implementation. P4 AI, P5 web renderers, P6 CMS, P7 API keys/rate limits/callback delivery, P8 native/CLI/TUI shells and P9 live migration/deploy remain their own phases. P3 must expose real tested core/API behavior those shells consume, not placeholders claiming those phases are done.

## Sources to consult

- `transgate/docs/discovery/jobs-transcription.md`: job statuses/transitions, routing, provider callbacks, billing coupling, uploads, editing and exports.
- `transgate/docs/discovery/privacy-identity.md`: PRV rules, retention, opt-out, account deletion, source defects and identity boundary.
- `transgate/docs/discovery/languages.md`, `transgate/fixtures/languages/`: all current locales/language lists, source capability gaps.
- `transgate/docs/discovery/ai.md`: translation and shared LLM transport boundary only; no accidental P4 scope expansion.
- `transgate/docs/discovery/api.md`, `platform.md`, `data-model.md`: endpoint/service split, jobs schedules and schemas as needed.
- `docs/plans/p2-billing/implementation-contract.md`, `decisions.md`, `repair-contract.md`: shared atomic billing contract and current repairs.
- Existing taskr core/postgres/server patterns and `docs/architecture.md`; targeted source under `/home/mu/Business/Transgate` is read-only. Never read secret env files or call production/provider APIs. Public primary documentation is allowed.

P0 findings to preserve: twelve job status values including nine observed legacy-state rows; source billing trigger is UPDATE-only; batch order is ascending draft id; source Soniox duration lives in metadata.duration_ms. URL callbacks use skip_balance_check=false; unknown duration has explicit deferred billing. Free tools are server-first with CLIENT_WASM_ENABLED=false and a separate FFmpeg processor service, not browser-only. P3 must not erase those obligations even if their shells deploy later. RunPod is excluded by PRD, but eight source-listed languages lack proved non-RunPod acceptance; investigate current primary provider capabilities without silently dropping them or inventing success fixtures.

## Proposal shape

Each advisor writes only its own `docs/plans/p3-transcription/<opus|astra>/proposal.md`, then a critique/response when dispatched. Propose the same complete P3 solution independently. Keep it decision-oriented, not a giant code listing:

1. Exact P3 rule-ID scope and boundary to later phases, including any existing inventory contradiction.
2. Crates/files, pure aggregates and presentation intents/view models, ports and ownership of transactions.
3. Transition table and concurrency/idempotency invariants: submission, duration discovery, provider start/callback, retries, cancellation, editing, deletion, retention and crash recovery. Network never inside the billing transaction; explain the unavoidable crash windows and bounded durable recovery, not a generic framework.
4. Real provider/storage/translation protocol plan and fixture provenance. Separate public docs evidence, synthetic fixture proof and unavailable live acceptance. Explicitly handle all current language choices and provider-pinned legacy jobs.
5. Billing integration: account lock then ordered host rows, exact-once start/deferred charges, hard-delete refund including imported signed/cross-owner usage, aggregate ownership, no new shell business rules.
6. Retention/privacy saga with cross-resource failure recovery and durable audit, source opt-outs/consents, remote retention uncertainty and no fictional successful deletion.
7. Minimal taskr consumer scenario that proves the pattern without turning taskr into Transgate or changing ordinary task creation.
8. Runnable verification plan (not executed during design): domain failing-first tests, restricted PostgreSQL/two-replica races, real binary/loopback protocol smokes, exact rule mapping and phase gate. Final global gates must remain honestly red for unfinished later phases.
9. Concrete independent developer slices with shared interfaces fixed before fan-out. Keep enough work per slice to justify visible agents.
10. Decisions requiring mu versus clearly documented bug corrections under PRD section 2; do not silently normalize source behavior.

Do not modify existing ledgers, board, gates, core or manifests. Report source-backed contradictions and proposed corrections for lead integration. Avoid speculative abstractions, invented scopes, no-op ports and happy-path-only tests. Every claim about current code needs a source location; every external protocol claim needs a primary link. Source unavailable means a named prerequisite, not fake completeness.

## Handoff

Send a concise ready/blocker message to lead pane w23:p1E with your proposal path. Remain available for the other proposal's critique. P2 implementation owners continue separately; their current files are not yours to edit.
