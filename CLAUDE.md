# rustr

## What we are building

A reusable architecture, "Core and Shells", for launching a new MVP very quickly from a solid, tested pattern. It starts as a CLI/TUI over a file and grows into a full SaaS (web, native mobile, desktop, even an Obsidian plugin) without rewriting the core. `taskr` is the reference sample app. The goal is a complete architecture system, not only a design pattern.

The core is Rust: domain, use cases, ports, and a shared presentation state machine (`TaskListState`: `update(event) -> effects`, `view() -> TaskListVm`). Shells (CLI, TUI, web via wasm, Android and iOS via UniFFI, and the Axum server) only render the view model, forward intents, or map HTTP to the API ports. Everything outside the core (storage, auth, billing, email, flags, analytics) is a port with swappable adapters.

## Where things are

- `docs/prd.md`: the goal and its gates (Transgate on Rustr). `WORKING-ON.md` at the root is the progress board
- `docs/architecture.md`: the pattern as built
- `docs/Roadmaap.md`: Obsidian kanban of concepts. Keep the `**In progress**` and `**Complete**` marker lines, Obsidian uses them. The filename typo is known.
- `docs/thoughts.md`: running brainstorm, questions and answers, decisions not yet in the book
- `docs/milestones/`: one note per milestone: what was built, decisions, how it was verified, open questions
- `book/`: the live study guide (EPUB and PDF). Build with `./scripts/build-book.sh`. Update it after decisions are discussed.
- `crates/taskr-core`: no I/O, no dependencies. Keep it that way. Ports return `shared::future::BoxFuture`.
- `crates/taskr-api`: the HTTP contract; server and client both compile against it, so change it there only
- `crates/taskr-server`, `crates/taskr-client`, `crates/taskr-postgres`: the signed-in mode (M1)
- `deploy/dev/compose.yaml`: local Postgres + Supabase Auth (GoTrue)
- `rust-multiplatform-application-architecture.md`, `arch-review.md`: early notes

## Commands

```sh
cargo test --workspace
TASKR_TEST_DATABASE_URL=postgres://postgres:postgres@localhost:54322/taskr cargo test --workspace   # with Postgres
./scripts/gen-bindings.sh      # after changing taskr-ffi or taskr-wasm DTOs
TASKR_WASM_BUILD_STD=1 ./scripts/build-web.sh   # Arch's rustc needs std built from rust-src for wasm32
./scripts/build-book.sh --check                 # book listings still match the code
(cd apps/web && npm run test:ci && npm run e2e)  # web: Vitest, then Playwright on desktop and phone
docker compose -f deploy/dev/compose.yaml up -d
TASKR_STORAGE=memory TASKR_DEV_AUTH=1 cargo run -p taskr-server
```

## Working mode

Concepts are explored in parallel by agents working overnight on best effort. Each concept gets a short design note and a decision proposal. mu reviews in the morning, then the book and roadmap are updated. Agents propose, mu decides. Do not mark roadmap cards done without evidence.

## Reference projects (read on demand, do not explore up front)

- `/home/mu/Business/Transgate/frontend`: primary. A mature SaaS in production: MVVM with separate mobile and desktop views, Storybook, tests, arc42 and ADRs in `docs/arc42`. Source of "what to validate or adapt".
- `/home/mu/Business/Transgate/voice-actor`: an evaluation lab where agents work end to end and validate with confidence. Inspiration for agent-verifiable workflows.
- `/home/mu/src/namaz`: a TUI example.
- Outside inspiration for TUIs (other languages): lazygit, lazydocker, k9s.

## Direction already agreed

- Local-first sync: local store plus change queue, pull since cursor, realtime is an optimization of the same pull. Conflict rules per aggregate in the domain.
- Modular monolith first, one crate per bounded context. CQRS starts in one process. Kappa-style event log (not Lambda) for ETL, with an outbox from day one.
- RBAC as a pure `can(actor, action, resource)` in the core. OPA/Rego or Cedar only if policies must change without a deploy.
- Blog, static pages and SEO are a separate static shell, not core.
- Design tokens live in one file at the repo root and are generated per shell. The state machine speaks in semantic tones, never colors.
- Platform features (browser storage, keychain, notifications) are ports implemented by the shell.
- A Kubernetes operator is a shell (`kube-rs`); reading or writing k8s resources from the core is a port.

## Web shell conventions

- Every screen has a pure desktop view and a pure phone view (`views/desktop`, `views/mobile`): props in, intents out, only UI mechanics as local state. Containers pick one by viewport.
- Stories are titled `Features/<Feature>/DesktopViews|MobileViews/...` or `Pages/App/...`, and render fixtures from `src/lib/core/fixtures.json` (written by `cargo test -p taskr-wasm`). Never hand-write a view model in a story.
- New behavior gets a component test per platform and, when it is a user flow, an e2e spec that runs on both projects.

## Rules for this repo

- Business meaning never lives in a shell. If a shell needs an `if` about it, move it to the presentation layer.
- Add a failing test first for domain rules.
- Prefer pure Rust for adapters where reasonable.
