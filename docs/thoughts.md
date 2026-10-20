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
