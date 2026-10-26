# PRD: Transgate on Rustr

Written 2026-10-23 from mu's brief. This is the contract for the work. `WORKING-ON.md` at the repo root tracks progress against it.

## 1. Outcome

Three deliverables, built in parallel in this repository, no branches, no worktrees, sub-directories only.

1. **Transgate is re-written in the Core and Shells pattern.** Same product, same business rules, same user experience for uploading files and getting transcriptions and translations, plus two things it does not have today: native iOS and Android apps, and LemonSqueezy as a second payment gateway. Fewer moving parts than today, a modern but very simple design with an E-Ink inspired palette, packaged as a Docker image.
2. **Rustr is improved into a reusable template, skill and knowledge base** that covers every Transgate use case, so the next SaaS (SEO site, native apps, billing) starts from decided answers and spends its time on business.
3. **Generic example other than the template** to demo Rustr with very simple functionality enough to demo the each arch and design case.

The repository ends with three sections:

| Section                | Where                       | What it is                                                                              |
| ---------------------- | --------------------------- | --------------------------------------------------------------------------------------- |
| The template           | `book/`, `docs/`, `skills/` | The pattern, the decisions, the how-to, a skill an agent can follow to start a new SaaS |
| The minimalist example | `crates/`, `apps/` (taskr)  | One tiny app that exercises every scenario in the catalog (section 6)                   |
| The full working copy  | `transgate/`                | Transgate itself, production ready                                                      |

Reusable bounded contexts that both the example and Transgate consume live in `modules/` (section 5).

## 2. Guardrails

### Zero change (the core business)

These rules are copied, not redesigned. No additions, no reductions, no new restrictions, no new allowances. Every rule gets a line in the rules ledger (section 7.2) with its source in the current code and a Rust test.

1. Billing, payments, packages and freemium: minute packages with time windows, the usage ledger, balance derivation, how a yearly plan expands into packages, the freemium package (20 minutes, 2 months), the freemium first-job bypass, deferred billing when duration is unknown, coupons, currencies, the Iyzico flow. The Turkish e-invoice (Paraşüt) flow is out of scope and is not ported.
2. Transcription and translation: sources (file upload, URL, YouTube), job states as the user sees them, speaker labels, line editing, translation targets, exports.
3. AI functionality: summary, highlights, chat over a transcript, streaming, chat limits, AI consent.
4. Data privacy and retention: account deletion and anonymization, per-user retention opt-out, the 30-day history rule and the `unlimited_history` exception, the consents, the cleanup schedule.
5. Freemium limits everywhere they apply.
6. Every language stays supported: all 16 UI locales and every transcription and translation language the current app offers.

### Allowed to change

- Bugs and architecture flaws. A bug fix that changes a business outcome is recorded in the ledger as "current behavior / intended behavior / decision" and the intended behavior is used only when the current one is clearly a defect (for example the unlocked balance check that lets two parallel jobs both pass).
- Architecture complexity. Over-engineered areas get simplified: the transcription provider plugin framework and health scoring, the three schedulers, the webhook tables nothing processes, the Stripe-shaped leftover tables, legacy Redux and DAO layers. It must however support mutlipe transcript pipelines with the same routing logic based on language and models support including at least Deepgram and Sonios. You can drop Rundpod for now.
- The public API is rewritten. Our own API keys, not Supabase JWTs. One versioned API with generated OpenAPI docs. If production has active third-party API keys at migration time, a compatibility shim for the old `v1-beta` shape is a conscious decision, not a default.
- Design: dark and light mode, a design system with tokens, an E-Ink inspired palette (paper whites, warm grays, ink black, one accent) that feels familiar to academics and students. Super modern, very simple.
- Stack: not tied to Supabase, Railway or Next.js. Tailwind stays.

### Not allowed

- No placeholders, no "coming soon", no stubs shipped as features.
- No new features or capabilities beyond the two named (native apps, TUI, CLI, LemonSqueezy). LemonSqueezy full integration is out of scope.
- No UI/UX improvisation in the app. The upload, transcribe, translate and AI flows keep the same steps and feel. It must not look like other apps.
- UI/UX creative improvements may be built, but only behind a feature flag that is off by default, each listed in the milestone note so mu can switch them on one by one.
- No premature optimization and no over-engineering. The current app works at the current scale; design for that scale with clean seams, not for ten times it.
- No branches or worktrees for this work. Sub-directories only.

## 3. Decisions already made

These stand unless a dead end forces a rethink, which is recorded in `WORKING-ON.md`.

| Area                    | Decision                                                                                                                                                                                                                          | Why                                                                                          |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Language and pattern    | Rust core, Core and Shells as built in taskr: domain, use cases, ports, shared presentation state machine, thin shells                                                                                                            | The whole point                                                                              |
| Database                | PostgreSQL via `sqlx`, no NoSQL                                                                                                                                                                                                   | Transgate's data is already in Postgres, the adapter exists, one moving part, no vendor lock |
| Logic in the database   | Constraints, RLS as a safety net, housekeeping and read-only views only. No business rule in a trigger or procedure. See `docs/thoughts.md`                                                                                       | Rules must live once, in the domain, with tests                                              |
| Identity                | Self-hostable GoTrue (Supabase Auth) behind the `Identity` port as the default. Account migration keeps user ids. A Rust-native adapter is an experiment (E2), not the plan                                                       | Already built in M1, open source, one container, keeps Google OAuth and password hashes      |
| Payments                | `modules/billing` with a `PaymentGateway` port. Adapters: Iyzico (ported) and LemonSqueezy (new, minimal: hosted checkout for a package plus the order webhook, no subscriptions, no customer portal, no license keys). Fakes for both. | Zero rule changes, two gateways, module is splittable                                        |
| Transcription providers | A `Transcriber` port with at least Soniox and Deepgram, one routing rule shared by all pipelines (language and model support, then duration), one retry policy, no health scoring, no plugin configs. RunPod/WhisperX is dropped for now; its routing slot stays so it can return | Simplify what caused more problems than it solved                                            |
| Jobs                    | A `Job` aggregate with explicit transitions, compare-and-swap saves, one scheduler port, one `Lock` port (Postgres advisory locks). Workers are shells that call use cases                                                        | The lesson from Transgate's 12 trigger rewrites                                              |
| Web app                 | Svelte plus the wasm core, Tailwind, desktop and phone views per screen, as in taskr                                                                                                                                              | Pattern parity across shells                                                                 |
| Static site and blog    | `modules/cms`: content as Markdown in git, built to static HTML with sitemap, hreflang and 16 locales. Framework chosen by experiment E1                                                                                          | SEO needs static pages; git is enough CMS for now                                            |
| Mobile                  | Kotlin and Swift shells over UniFFI, as in taskr. Android verified on Waydroid here, iOS compiled on Linux Swift and built on a Mac when available                                                                                | Pattern parity, Linux-first                                                                  |
| CLI and TUI             | Rust shells over the same core and state machines, as in taskr: upload, list jobs, fetch a transcript or translation, sign in                                                                                                     | Two more shells for the price of a renderer; the TUI is the fastest way to exercise flows    |
| Email                   | Resend behind a `Mailer` port, templates in Rust, same emails as today                                                                                                                                                            | One language, no TypeScript build step                                                       |
| Analytics and errors    | PostHog and Sentry behind ports, same events as today                                                                                                                                                                             | Keep what works                                                                              |
| Packaging               | One Docker image: the Axum server serving the built web app, plus the static site as a second image. Deployable on Railway now, Kubernetes later                                                                                  | Asked for                                                                                    |
| Design tokens           | One tokens file at the repo root, generated per shell, semantic tones in the state machine                                                                                                                                        | Already agreed                                                                               |
| Multi-tenancy           | Not now. `modules/billing` and `modules/cms` are bounded contexts with their own schema prefix, their own ports and no dependency on Transgate crates, so they can be split later                                                 | Asked for: splittable, not multi-tenant                                                      |

## 4. Decisions by experiment

Where experience beats hypothesis, build two or three small versions, measure, then pick. Each experiment has a question, a bounded build, a verdict rule and a deadline in effort, not calendar time.

| Id | Question | Build | Verdict rule |
|---|---|---|---|
| E1 | Which static site tool for `modules/cms`: Astro, SvelteKit prerender, or a Rust generator (Askama templates over Markdown)? | The pricing page, one SEO landing page and one blog post, in 2 locales with hreflang and a sitemap, in each candidate | Fewest moving parts that passes Lighthouse SEO 100 on both pages, builds the full page count in under 2 minutes, and lets Tailwind tokens come from the shared tokens file. Tie goes to the one with no second JavaScript framework |
| E2 | Keep GoTrue or own the auth flows in Rust? | Only if GoTrue blocks something concrete (account migration, OAuth, rate limits). A Rust adapter with email and password, Google OAuth, password reset | Pick Rust only if it removes a container and its tests cover every flow Transgate uses today |
| E3 | E-Ink palette | Three token sets rendered in Storybook across the main screens, light and dark | mu picks. Until then the first set is used |
| E4 | Translation and AI provider calls: keep the current LLM providers or route through one gateway? | Same prompts, same models, through the current SDK shape and through a plain HTTP adapter | Output parity on the recorded fixtures, fewer dependencies wins |

## 5. Reusable modules

Each module is a crate group under `modules/<name>/` with `core` (pure), `postgres`, `http` (routes and contract) and, where needed, `web` (Svelte views). Rules for a module:

- No dependency on `transgate/*` or `crates/taskr-*`. Checked by a script over `cargo metadata`.
- Two consumers from day one: taskr uses a minimal slice, Transgate uses all of it. That is the test that it is reusable.
- Its own Postgres schema prefix and migrations folder, so a split is a copy, not a rewrite.
- Talks to the host app only through ports and domain events, never by sharing tables.

Modules in scope: `billing` (packages, ledger, gateways, coupons, freemium) and `cms` (static pages, blog, SEO metadata, locales). `identity` stays in the shared kernel as it is today.

## 6. Scenario catalog for the minimalist example

taskr must show every scenario Transgate needs, in the smallest form. A scenario is done when taskr has it with a test and the book has a section for it.

1. Domain, use cases, ports, presentation state machine, five shells (done)
2. Multi-user, RBAC, outbox, revision-checked saves (done)
3. Long-running job aggregate: states, retries, stuck recovery, a worker shell, idempotency
4. External provider port with two adapters, a simple fallback rule, recorded fixtures
5. Billing through `modules/billing`: a paid package, a freemium limit, one gateway webhook
6. Public API keys with scopes, per-key rate limit, outbound callbacks with delivery records
7. Scheduler and lock ports, safe with two replicas
8. Account deletion as a saga, retention sweep with opt-out, consents
9. Paged list with cursors in presentation state
10. Streaming results through the state machine, across wasm and UniFFI
11. File upload with progress and presigned URLs the core never stores
12. Static site and blog through `modules/cms`, 2 locales, sitemap, hreflang
13. Design tokens generated per shell, light and dark, E-Ink palette
14. Email, analytics and error tracking as ports with fakes
15. Contract evolution: a versioned API change with an old client still passing
16. Docker image, health endpoint, graceful shutdown, config from environment
17. Data migration tool pattern: read old shape, write new, verify counts

## 7. Verification: how we know a goal is reached

Everything below is a command or a checklist file. No goal is "done" by opinion.

### 7.1 Gates

| Gate | Command | Passes when |
|---|---|---|
| Rustr | `./scripts/gate-rustr.sh` | `cargo test --workspace`, `cargo clippy --workspace --all-targets` clean, `./scripts/build-book.sh --check`, generated bindings match (`./scripts/gen-bindings.sh --check`), every scenario in section 6 is marked done in `docs/scenarios.md` with a test id that exists |
| Modules | `./scripts/gate-modules.sh` | Dependency check passes, each module's tests pass, both consumers (taskr and Transgate) build against it |
| Transgate | `./scripts/gate-transgate.sh` | Rules ledger all green, parity suite all green, web Vitest and Playwright on desktop and phone, Docker images build and answer `/health`, migration dry run reports parity, Android APK runs the signed-in flow on Waydroid, Swift bindings and views compile on Linux |

### 7.2 Rules ledger: `transgate/docs/rules.md`

One row per business rule: id, plain-English rule, source in the current code (file and line or migration), the Rust test that proves it, status. Written in Phase 0 before any code. Every pgTAP test in `frontend/supabase/tests/` maps to at least one row. The gate fails if any row has no test or a failing test.

### 7.3 Parity suite: `transgate/docs/parity.md`

One row per user flow in the current app (desktop and phone, and the same flow in the CLI, TUI, Android and iOS shells where it applies), per API endpoint kept, per email sent, per cron job, per locale. Each row names its Playwright spec or Rust test. Flows come from an inventory of the current app's screens and routes, written in Phase 0. The gate fails on any row without a green test.

### 7.4 Languages

`transgate/core/src/languages.rs` is generated from the current app's language lists. A test asserts the new lists are supersets of the extracted fixtures for UI locales, transcription languages and translation targets.

### 7.5 Data migration

`transgate/tools/migrate` reads a snapshot of the current Postgres and writes the new schema. Dry run prints a parity report: users, jobs, transcripts, payments, packages, usage rows, balances per user (old function versus new derivation must match to the minute), API keys, consents. The gate requires zero mismatches on the latest snapshot.

### 7.6 Template usability

A scripted run: an agent with only the skill in `skills/` and the book scaffolds a new app named `demo` with one aggregate, one screen, billing and a static page, and its gate passes. This is the test that Rustr became a template, not a sample.

## 8. Phases

Each phase records a milestone note and a `WORKING-ON.md` entry. Per mu's 2026-10-24 instruction, book edits and builds are deferred to the very end rather than repeated at each phase. Rustr code improvements still happen inside every phase.

| Phase | Deliverable | Done when |
|---|---|---|
| P0 Discovery | Inventory of screens, routes, emails, crons, tables; rules ledger with sources; keep/throw list with reasons; language fixtures; research R1 to R4 answered | The three documents exist and mu can read them in the morning |
| P1 Foundation | Repo layout (`transgate/`, `modules/`), one workspace, tokens file and E-Ink palette (E3), Docker image and `/health`, CI in `.github/workflows`, gate scripts (failing is fine) | Gates run end to end |
| P2 Billing module | `modules/billing` with every ledger rule, Iyzico and LemonSqueezy adapters with fakes and recorded fixtures, taskr slice | Ledger rows for billing green, module gate green |
| P3 Transcription core | `Job` aggregate, upload with presigned URLs, Soniox and Deepgram, translation, transcript lines and editing, exports, retention and deletion saga | Ledger rows for transcription and privacy green, server tests green |
| P4 AI | Summary, highlights, chat with streaming, limits, consent | Ledger rows for AI green, streaming scenario in taskr |
| P5 Web app | Every screen as desktop and phone views, 16 locales, light and dark, same flows as today | Parity rows for web green on both Playwright projects |
| P6 Static site and blog | `modules/cms`, all current SEO pages and blog posts migrated, sitemap, hreflang, pricing from the billing module's catalog | Page count equals today's sitemap, Lighthouse SEO 100 on a sample |
| P7 Public API | Keys, scopes, rate limits, callbacks with delivery records, OpenAPI docs | Parity rows for API green |
| P8 Mobile, CLI and TUI | Android and iOS shells for the Transgate flows; CLI and TUI shells over the same core | APK on Waydroid passes the signed-in flow, Swift compiles on Linux, Mac build steps documented; CLI process tests and TUI render tests green |
| P9 Migration and deploy | Migration tool, dry run on a snapshot, staging deploy on Railway from the Docker image, cutover runbook | Migration gate green, staging answers, runbook reviewed |
| P10 Template | Skill, book chapters for every scenario, `docs/scenarios.md` complete, the template usability run (7.6) | Rustr gate green, usability run passes |

Phases P2 to P8 can overlap when agents work in parallel. P9 waits for P5 and P7. P10 is continuous and closes last.

## 9. Research questions to answer in P0

- R1 Which transcription and translation languages does each provider cover, and which languages today reach only RunPod/WhisperX? Those are listed in the keep/throw list as a known gap since RunPod is dropped for now.
- R2 Which LLM providers and models do translation and AI analysis use today, with which prompts and limits?
- R3 Are there active third-party API keys and callback URLs in production? Decides the compatibility shim.
- R4 Which parts of the free tools, YouTube and URL sources, and the voice demo are part of the product the user pays for? Voice demo is presumed out of scope.
- R5 What does the current sitemap contain (page count per locale) and which pages were pruned on purpose?

## 10. Known blockers and workarounds

Work never stops on a missing key. Every external port has a fake and recorded fixtures, so all gates except real-provider smoke runs pass without secrets. Missing items are listed in `WORKING-ON.md` under "Needs mu" and picked up when provided. You can read .env.local in transgate for any API access.

| Need | Workaround until provided |
|---|---|
| Soniox, Deepgram, LLM keys | Recorded fixtures from the current app's shapes; sandbox keys found in Transgate's local env files are used for read-only smoke runs, never committed |
| Iyzico sandbox | The sandbox credentials already used by Transgate CI |
| LemonSqueezy test-mode store | Fake adapter plus recorded webhook payloads from the LemonSqueezy docs; real smoke when a store exists |
| Resend, PostHog, Sentry | Fakes; real smoke optional |
| Production Postgres snapshot | Migration tool developed against a seeded copy of the baseline schema; parity run needs one `pg_dump` from mu |
| Google OAuth client, Turnstile | Dev auth and a fake captcha port locally |
| Mac, Apple developer account, Play console | iOS compiled on Linux Swift; Android on Waydroid; store steps documented, not executed |
| Railway token, DNS | Docker image and compose file verified locally; staging deploy when the token exists |

## 11. Working agreements

- mu is not watching. No questions and no blockers until done. Obvious questions are answered here or in `WORKING-ON.md` with the assumption taken.
- Dead ends are allowed. When a decision proves wrong, stop that thread, record why in `WORKING-ON.md` under "Rethink", and continue elsewhere. mu decides in the morning.
- Decide alone where possible. When unsure, build two or three variants (section 4) and let the result decide.
- Effort xhigh. Sub-agents, teams, herdr tabs and panes, OMP (sonnet, opus, fable, gpt-6.1-sol, Astra), Codex and Claude are all fair game, each used for what it is good at: planning, advising, LSP-aware edits, reviews. Context is compacted and effort changed by the agent's own judgment.
- AI-Brave and walkthrough videos are used for visual checks and for evidence in milestone notes.
- Logging: `WORKING-ON.md` holds the phase board and a dated high-level log, one line per unit of work, so any agent can pick up. Detail goes in per-phase files under `docs/milestones/` so one file never explodes.
- Knowledge import: before porting anything from Transgate, decide and write down whether it is knowledge to keep (a rule, a lesson, an ADR), code to port, or something to throw away, in `transgate/docs/keep-throw.md`.
- Comments only where a hidden assumption must be explained. No long dashes anywhere.
## 12. Hints from Mu experience
- Codex / Astra is good for implementing big chunks of code,  but draft from original goal after multiple prompts and long context
- Both Opus and Fable are good for critical thinking and don't just agree
- OMP is good for debugging and testing and yolo mode with structured TODOs and code integrations
- Claude is safer and can also implement tasks very well
- effort mode changes are very useful based on task:
	- Low for short obvious tasks and implements exactly what you say
	- Medium spend more effort and retries that I use if I'm not sure exactly what I prompt to Low will work
	- High will work for longer and makes more reasoning and corrections
	- XHigh and Max for larger tasks and auto-pilot mode, but it requires a clear verifiable (verifiable is important) goal
	- OMP has more features that I didn't experiment all myself. 
- Watch limits - I use Anthropic and OpenAI OAuth. OpenAI is more forgiving because it has a full week reset, but Anthropic has session limits.
- My recommendation is to start with Fabel and Astra and with high or xhigh or even ultra thinking and then move for lower models if lmits are about to reach. You can check limits from time to time, but don't be paranoid. At worse, I can wait for reset.
- You can assign roles to herdr agents and keep track of them, so you don't have to start context from scratch every time.
- You are still encouraged to use sub-agents and teams of agents
- All of those hints are only recommendations, but not required. You might find better hints during working