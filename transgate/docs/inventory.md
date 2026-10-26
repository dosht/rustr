# Transgate inventory

This is the entry point to P0's sourced inventories. Topic files contain the individual rows and source locations; this index deliberately does not duplicate their full tables. The baseline is the source tree under `/home/mu/Business/Transgate`, with dated production observations in [production-facts.md](production-facts.md). Discovery status is tracked in `WORKING-ON.md` and `docs/milestones/p0-discovery.md`.

## Product surfaces

| Inventory | Canonical detail | Port destination |
|---|---|---|
| Authenticated screens, desktop/phone variants, elements, intents and user flows | [Screens](discovery/screens.md#inventory) | Transgate Svelte/wasm shell and shared presentation state |
| 40 Next API paths: 33 Pages handlers and 7 App routes; keys, scopes, expiry, rate limiting and callbacks | [API](discovery/api.md#inventory) | Versioned Axum API and identity/rate-limit/callback ports |
| 4527 unique sitemap URLs across 16 locales, including 157 published article directories under the current JavaScript date rule | [Static pages](discovery/static-pages.md#inventory) | `modules/cms` and separate static shell; preserve intentional SEO pruning |
| Billing dialogs, checkout, payment confirmation, history and subscription management | [Billing](discovery/billing.md#inventory) | `modules/billing`, Iyzico and minimal LemonSqueezy adapters |
| Job lists, upload, source resolution, provider routing, callbacks, retries, editing and exports | [Jobs and transcription](discovery/jobs-transcription.md#inventory) | Job aggregate, use cases, provider/storage ports and worker shells |
| Summary/analysis, highlights, transcript chat, streaming and quotas | [AI](discovery/ai.md#inventory) | AI use cases and provider adapters; streaming presentation events |
| Sign-in methods, refresh, profile, consents, account deletion and retention | [Privacy and identity](discovery/privacy-identity.md#inventory) | Existing Identity port, GoTrue adapter, deletion saga and retention use cases |
| Emails, schedules, flags, analytics, errors and deployment operations | [Platform](discovery/platform.md#inventory) | Mailer, Scheduler, Lock, flags and telemetry ports; server/worker shells |
| 40 processor method/path endpoints: 39 media-tool endpoints plus health, separate from the 40 Next paths | [Static pages R4](discovery/static-pages.md#research) and [API](discovery/api.md#inventory) | Runtime server/provider adapters, not static CMS alone; current CLIENT_WASM_ENABLED=false makes these flows server-first |

The CLI, TUI, Android and iOS are new Transgate renderers of applicable flows, not existing product surfaces. Their obligations come from PRD P8 and the parity inventory; no claim is made that they already exist.

## Data and languages

| Inventory | Observed source scope | Canonical detail |
|---|---|---|
| Database | 32 tables, 50 views, 11 enums, 52 routines, 14 triggers, 59 RLS policies and 53 migrations in the inspected source | [Data model](discovery/data-model.md#inventory), including source locations and keep/throw ownership |
| UI locales | 16 locale codes | [Languages](discovery/languages.md#inventory), `../fixtures/languages/ui-locales.json` |
| Transcription languages | 61 offered language names/codes | `../fixtures/languages/transcription-languages.json` |
| Translation targets | 53, through Soniox one-way translation | `../fixtures/languages/translation-targets.json` |
| Provider routing | Soniox/Soniox-v5, Deepgram, and legacy RunPod capability declarations | `../fixtures/languages/provider-routing.json` and [jobs inventory](discovery/jobs-transcription.md#inventory) |

Database counts describe the migration/type inventory, not proof of the live schema. The production probe confirms drift: four language values absent from migrations and nine jobs in deprecated statuses. The new schema must represent the actual data rather than assume generated types are the production catalog.

## Boundaries and exclusions

- Billing is a reusable context with two consumers, taskr and Transgate. Its rules are not reimplemented in shells.
- CMS is a reusable static-content context. Public pages are not coupled to the Rust app's authenticated rendering loop.
- Iyzico recurring plans remain in scope. The absence of rows in the old Stripe-shaped tables does not mean subscriptions are absent.
- LemonSqueezy is hosted checkout plus an order webhook only. It does not replace Iyzico subscriptions.
- Paraşüt e-invoicing is excluded by the PRD; existing paid-package, payment and invoice-display behavior must be distinguished from that external integration.
- RunPod is removed from active routing for now. Preserve legacy provider values when importing old jobs, and retain a routing seam for a future adapter.
- Voice demo is presumed excluded by the PRD. Free tools and YouTube/URL sources require the R4 inventory; they are not silently discarded as marketing pages.
- Existing live API clients and callback consumers require an explicit cutover decision. New API design can proceed without claiming that old clients will automatically remain compatible.

## How to use this inventory

The consolidated rules ledger is generated from each topic's Rules section. Keep/throw decisions and parity rows are generated from their corresponding sections. Research answers live in `research.md`; counts, sources and assumptions stay in topic files rather than being independently maintained in multiple summaries.

A row marked `ledger` or `inventoried` is an obligation for later implementation, not a passing Rust or browser test. P0's checker establishes discovery completeness and SQL traceability only. The PRD's Rustr, module and Transgate gates are separate implementation gates.
