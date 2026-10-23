# Thoughts

Running brainstorm. Decisions are proposals until they land in the book.

## Goal

Launch a new MVP very quickly from a solid, tested pattern. Start from a CLI/TUI and grow to a full SaaS with native mobile, desktop or an Obsidian plugin as shells.

Scope to cover over time: static pages with SEO and a blog, an authenticated area with RBAC, cloud-native config (CRDs, operators), PostgreSQL, realtime and streaming, microservices and CQRS, ETL/ELT, analytics and conversion funnels, and the areas plain MVVM/DDD does not cover (navigation and more).

## What the Transgate docs taught us

Source: `/home/mu/Business/Transgate/frontend-voice-actor/docs/arc42`.

- The ViewModel and DDD layering is the same as ours. They write each ViewModel as a React hook, we share one Rust state machine.
- Check our design against master/detail ViewModels (ADR-020) and navigation as infrastructure (ADR-029).
- Never persist anything that expires, such as pre-signed URLs (ADR-025).
- Pluggable providers with fallback (ADR-021, RISK-001): ports with several adapters, test failover.
- Structured logs with a correlation id, user id and commit SHA (ADR-018). Our telemetry port only has JSON lines today.
- Interface duplication causes drift (ADR-028). Watch `taskr-ffi` against `taskr-wasm/src/dto.rs`.
- Their risk and debt register (section 11) is worth copying. Feature flag sprawl became debt.
- We have no auth, error taxonomy, retention, flags or i18n in the core yet.

## What the Transgate code taught us (2026-10-22)

Source: the production repos under `/home/mu/Business/Transgate` (`frontend`, `frontend/supabase`, `supabase-routines`, `Infra`, `runpod-whisper-x`, `voice-actor`), read as "what does a product need after a few years that taskr does not show yet". Billing, email, flags, analytics, SEO and i18n, the blog, jobs and uploads, realtime, offline and sync are already roadmap cards, so they only appear below where Transgate's version is harder than the card suggests.

### Stored procedures and the pattern

Stored procedures do not break the pattern by themselves. What breaks it is a decision living in the database. The test: if Postgres were swapped for the JSON file store, would behavior change? If yes, a rule leaked.

- Transgate's `deduct_minutes` and `request_breakdown_over_packages` (`supabase-routines/DDL.sql`) are the billing rule in plpgsql, called straight from the shell via `rpc('deduct_minutes')` in `src/utils/deepgram.ts` and `soniox.ts`. The `insert_new_packages_on_payment_completed` trigger is a use case hiding in a trigger. The `test_*` SQL functions exist because the DB was the only place the rule could be tested. `deduct_minutes` also takes no lock, so two parallel jobs can both pass the balance check.
- Fine in the DB: constraints, RLS, `updated_at` triggers, cleanup jobs, read-only views and SQL functions. They compute or refuse, they do not decide. A guardrail trigger that rejects a row breaking an invariant is defense in depth, not logic.
- The taskr way: a pure `allocate(requested, open_packages) -> Result<Vec<Deduction>, Insufficient>` in the domain, a port with "load open packages" and "record deductions", and a Postgres adapter that runs both in one transaction with `SELECT ... FOR UPDATE` or an advisory lock per user. When one round trip matters, keep a dumb procedure such as `apply_deductions(jsonb)` that executes a plan Rust already computed; it lives in the postgres crate's migrations and is tested there. Hot reads keep a `minutes_remaining` column maintained in the same transaction (the CQRS read model). The payment trigger becomes an `on_payment_completed` use case fed by the outbox.

### Topics not covered by the code or the roadmap

1. **Job lifecycle as a domain concept.** Transgate's `jobs` table is queue, state machine and audit trail at once: a 12-value status enum, `retry_count` and `recovery_attempts`, a cron that resets jobs stuck in `running` for 30 minutes (`src/utils/deepgramRetry.ts`), compare-and-swap status updates so a late provider callback cannot overwrite a completed job (`src/pages/api/soniox/callback.ts`), recovery claims capped at 3 (`src/utils/sonioxRetry.ts`), a provider fallback chain scored by `provider_health_metrics`. The billing trigger `insert_usage_history_on_job_start` was redefined 12 times. M6 needs a Job aggregate with explicit transitions, idempotency, and workers as shells. The Python GPU worker (`runpod-whisper-x`) writes to Supabase with the service key: the stored-procedure problem in another language.
2. **A public API product.** API keys with hashes, scopes, expiration policies, audit logs and per-key rate limits (`api_keys`, `api_key_audit_logs`, `api_key_rate_limits`), customer callback URLs with delivery records (`callback_deliveries`), Swagger docs, `v1`, `v1-beta` and `unified` versions. In the pattern: a second `Identity` adapter (principal from key), scopes feeding `can()`, outbound webhooks as an outbox consumer. `webhook_notifications` has retry columns but nothing processes them.
3. **Schema and contract evolution.** 53 migrations with fix-the-fix chains (20251230160000 to 190000, including a revert to "buggy but working"), enum additions in their own transaction, no down migrations, a squashed baseline, orphan purges, dropped columns, test code shipped as migrations, and a CI check that `src/types.gen.ts` is committed. taskr has one migration and no CI. Needed: versioned snapshots, additive-only API changes, and a version skew story (cached wasm bundle or month-old mobile app against a newer server; mobile cannot be force-updated).
4. **More than one replica.** Two Railway replicas broke the in-process rate limiter (`src/utils/userRateLimit.ts` assumes a single dyno) and the in-process scheduler; there are three schedulers (Vercel cron, node-cron, pg_cron). No staging environment, secrets on disk, CD only pushes the DB. Nothing on the roadmap about deploy, health checks, graceful shutdown, config, leader election. Proposal: a `Scheduler` port and a `Lock` port with Postgres advisory-lock adapters.
5. **Data lifecycle and GDPR.** `anonymize_user_data` on auth user delete, `public.users` mirrored from `auth.users` by triggers, per-user retention opt-out (`auto_delete_transcripts`), pg_cron cleanup with dry run and `audit.cleanup_audit_log`, 30-day history unless `unlimited_history`, three consents (AI, translation beta, cookies), a `blocked` flag. Account deletion is a saga across aggregates, the storage bucket, the STT provider, the payment gateway and PostHog.
6. **Screens the state machine has not met.** Keyset pagination and infinite scroll in a master-detail list (ADR-019, ADR-020); taskr loads the whole aggregate, max 50 tasks. Streaming LLM output through one primitive (ADR-037) with per-user per-job daily `chat_limits`. Upload progress, presigned URLs computed on demand and never stored (ADR-025), browser-side ffmpeg, a 300 s request cap. All need partial results and progress as events through `update()`, and across UniFFI that means callback streams.
7. **Billing details.** Ledger with derived balance, deferred billing when duration is unknown until the provider answers (NULL-duration fix 20260408), freemium first-job bypass changed five times, coupons, currency, Turkish e-invoicing through Paraşüt, no refund path. Prices live twice: the static pricing page renders translation strings, packages live in the DB.
8. **Admin and support.** Nearly absent: one customer-invoices endpoint, `transfer_job_by_email` (granted to `anon` in the baseline), a runbook that replays failed jobs by pointing a laptop at production. "Replay a failed job" should be a use case behind an admin role.
9. **Auth edge cases.** Google OAuth, Cloudflare Turnstile, a Supabase `over_request_rate_limit` hit by calling `getSession()` in middleware, no token refresh, no MFA, `next-auth` leftovers. Plus the refresh question already open from M1.
10. **Observability beyond logs.** Sentry on client, server and edge, memory monitoring after timer leaks (ADR-014), provider health endpoints, analytics views for hanging and failed jobs. taskr has request ids only. Missing: an error-tracking adapter, health of ports, the runbook as a document type.
11. **Replacing parts of the architecture.** Redux phase-out, DAO removal, Pages to App Router, interface duplication drift (ADR-028). The reference needs a story for swapping a shell and for retiring a bounded context.
12. **Agent-verifiable testing.** The voice-actor eval lab (`voice-actor/docs/product/evaluation-lab/`): deterministic replay with logical time, provider fixtures matched to requests, `verdict.json` plus a hashed `manifest.json` so tampered artifacts are refused, exit codes that mean "report produced" rather than "passed". pgTAP moved into the repo after an external test repo drifted (ADR-024). taskr has fixtures and contract tests but no recorded provider fixtures, no mock provider server, and no CI drift check for the generated bindings.

### Challenges for the pattern

- Who may write to the database besides the core: pg_cron, triggers, foreign-language workers. Either they call use cases (worker as shell) or they are limited to housekeeping and guards.
- Where shared-but-not-business state lives with N replicas: rate limits, schedules, locks, caches. Ports with distributed adapters, never process memory.
- Rules that change weekly (freemium bypass, billing trigger) must be cheap to change and test, and may be gated by flags. Flags read inside use cases risk the sprawl Transgate logged as DEBT-002.
- Contracts evolve under version skew across wasm, mobile and server. Additive changes only, codes not strings, a compatibility test per release.
- Streams and progress cross the shell boundary: wasm promises, UniFFI callbacks, TUI ticks, one design.
- Long-running work needs a Job aggregate and idempotency keys; status compare-and-swap alone leaves gaps (Transgate has no idempotency table).
- Deleting a person is a cross-context saga with external systems, not a cascade.
- Lists do not fit one aggregate: paged read models with cursors in presentation state.

## Architecture ladder

The core never changes as the product grows. Each rung adds adapters and shells.

1. CLI/TUI with a file
2. PostgreSQL behind an HTTP API
3. Web with auth and RBAC
4. Native mobile
5. Events, realtime
6. Analytics and ETL

## Milestones to reach Transgate's feature set

1. Multi-user with an outbox: async ports, Postgres adapter, Axum server shell, `Identity` port (Supabase Auth), RBAC `can()`, domain events through an outbox so analytics is only an adapter later.
2. Static site with SEO, design system and i18n: i18n comes here because SEO pages need locales from the start (hreflang, translated slugs).
3. Billing, emails and feature flags: freemium, entitlements, receipts.
4. Analytics and funnels: an adapter on the domain events from milestone 1.
5. Blog as a second app: first cross-app integration, multi-tenant, white label.
6. Background jobs and uploads: long-running work, webhooks, provider fallback. Transgate's hardest ADRs came from here.
7. Realtime and offline: hardest and least needed for an MVP, so last.

M1 was built on 2026-10-17; see `docs/milestones/m1-multi-user.md` for decisions and open questions.

## Q&A

**Which Lambda alternative did we discuss?** Kappa: one event log you can replay, instead of Lambda's separate batch and stream paths. Not sure it is the one mentioned earlier.

**Is a Kubernetes operator a shell or a port?** If k8s drives the app, it is a shell: a reconcile loop that turns CRD changes into use case calls (`kube-rs`). If the core only reads or creates k8s resources, it is a port with a k8s adapter.

**Supabase auth?** An `Identity` port returning an actor with roles. Keep Supabase, do not rebuild it.

**OPA and Rego?** Start with a pure Rust `can()`. Use OPA when policies must change without a deploy or be shared with non-Rust services. Cedar is a Rust-native option in between.

**Security policies in k8s?** Deployment config in an infra folder, not core.

**DevCycle?** A `FeatureFlags` port. Start with a config file, add DevCycle when targeting is needed.

**Iyzico and LemonSqueezy?** A `Billing` port. The core owns subscription and entitlement state, providers only send events in.

**React Email and Resend?** Resend is a `Mailer` adapter. React Email adds a TypeScript build step, so Rust templates (`mrml` or `askama`) keep it one language.

**LLM translation?** A build-time tool that writes reviewed translation files, not a runtime call.

**Tailwind?** Web and static shells only, never core.

**Where does the design system live?** One tokens file at the repo root. A generator emits the Tailwind theme, Compose theme, SwiftUI colors and TUI palette. The state machine only says semantic tones such as `Tone::Danger`.

**Where do platform features go (browser storage, mobile offline storage, keychain, notifications)?** Ports implemented by the shell. Telemetry already works this way with `LogcatSink`.

**A device goes offline and comes back?** Local-first. Each device writes locally and keeps a queue of changes. On reconnect it sends the queue and asks for everything since its last cursor. Realtime is a faster version of the same pull.

**Conflicts?** Rules per aggregate in the domain. Last-writer-wins per field for simple data, a business rule for cases like duplicate titles, CRDTs only for collaborative text.

**Where does the code live?** Rust. Sync logic is a state machine in the core, only the transport is a port. The server is Rust too.

**Supabase and Postgres, why both?** Supabase is a hosted Postgres with extras. Our adapter is plain `sqlx` against that Postgres, so the core only sees standard Postgres. Supabase Auth issues a JWT, the server verifies it, and the `Identity` adapter maps it to an actor with roles. That is the only Supabase-specific code. Authorization stays in the core's `can()`, the server connects as a trusted service, every repository query takes the tenant id, and RLS can come later as a safety net. Tests use plain Postgres in Docker with a fake `Identity`.

**Is there a Postgres extension for auth like Supabase?** No. Full auth (signup, OAuth, email flows, MFA) runs as a separate service that stores users in Postgres. Supabase Auth is GoTrue, a small Go service that can be self-hosted next to any Postgres. Similar services: Ory Kratos, Zitadel, Logto, SuperTokens, Keycloak. Inside Postgres there are only building blocks (`pgcrypto`, `pgjwt`). Rust has pieces (`axum-login`, `oauth2`), no full solution. Pick: Supabase Auth behind the `Identity` port.

**Postgres extensions from "I replaced my entire tech stack with Postgres"?** Rule: an extension is fine when it is an implementation detail inside an adapter, not when it exposes the database to clients and bypasses the core.

- Keep: JSONB for flexible fields; `pg_cron` for database housekeeping like retention cleanup (business schedules stay in Rust); `tsvector` full-text search as the first `Search` adapter; `pgvector` if a product needs AI or RAG.
- Evaluate for sync: ElectricSQL streams Postgres data to clients (the "pull since cursor" half), writes still go through our API.
- Maybe later: unlogged tables as a cache; `pg_mooncake` as a sink for the Kappa event log (does not replace PostHog funnels).
- Skip: `pgcrypto` plus `pgjwt` auth (no flows, ruled out); RLS as the main rules; PostgREST and `pg_graphql` (bypass the core); `pgai`; HTML or React in the database.

## Open

- Which concepts to design first (picked per phase).
- Exact shape of the overnight agent workflow and its output format.
