# Reference {.unnumbered}

## Glossary {-}

| Term | Meaning in this book |
|---|---|
| Adapter | an implementation of a port for one environment (JSON file, `localStorage`, logcat) |
| Aggregate | a cluster of objects changed as one unit, guarded by its root (`TaskList`) |
| Bounded context | a part of the product with its own consistent model and vocabulary (`tasks`) |
| Composition root | the one place a program chooses real adapters and wires them (`main.rs`, `MainActivity`, `TaskrApp`, `App.svelte`) |
| Container | a platform's thin holder of the latest ViewModel that forwards intents (`TasksViewModel.kt`) |
| Contract test | one test suite run against every implementation of a port, fakes included |
| Controller | the small runtime that runs effects and feeds results back (`TaskListController`) |
| Cross-cutting concern | something every context and shell needs, handled by a shared-kernel port (telemetry, time, identity) |
| Effect | a description of work the state machine wants done, as data (`Effect::Add`) |
| Entity | an object with an identity that persists while its attributes change (`Task`) |
| Event | something the state machine reacts to: a user intent or an effect's result |
| Facade | the boundary crate that translates types for a platform (`taskr-ffi`, `taskr-wasm`) |
| Fake | a working in-memory implementation of a port, used in tests |
| Fixture | a named screen state used for previews and snapshot tests (`fixtures::sample`) |
| Intent | what the user meant, sent by a shell (`Intent::ToggleRequested`) |
| Invariant | a rule that must always hold for an aggregate |
| Policy | a context's authorization rules, checked in use cases (`TaskPolicy`) |
| Port | an interface owned by the core for something it needs from outside |
| Read model | plain data returned by use cases instead of entities (`TaskSummary`) |
| Shell | the platform-specific code that renders and forwards input |
| Snapshot | plain data used to persist an aggregate without serde in the domain |
| Ubiquitous language | the shared vocabulary of users and team, used verbatim in code |
| UI facade | a screen that composes several contexts and owns no rules (settings, dashboard) |
| Unit of work | load, apply one domain operation, save, as one step (`Tasks::change`) |
| Use case | an operation the outside can ask the core for (`Tasks::add`) |
| Value object | an immutable value, valid by construction (`Title`) |
| ViewModel | everything a renderer needs, already decided (`TaskListVm`) |
| Actor | a principal inside one workspace, with a role (`Actor`) |
| API port | what a client may ask for, one workspace at a time (`TasksApi`, `WorkspacesApi`) |
| Outbox | domain events written in the same transaction as the change, for later consumers |
| Principal | a signed-in person as the identity provider vouches for them |
| Revision | the optimistic concurrency token a repository hands out on load |

## Decision log {-}

Each decision here was made for taskr, with the main alternative and why it lost. Revisit them when the reason stops being true.

| Decision | Alternative | Why |
|---|---|---|
| Share screen behavior in Rust, not just the domain | a ViewModel per platform | otherwise presentation logic exists five times and drifts |
| Pure state machine plus a controller | ViewModels that call use cases directly | effects as data make screen logic testable with no fakes |
| Web runs the core as WebAssembly | thin client over HTTP | same controller everywhere, offline for free, one API to version |
| Ports are `Arc<dyn Trait>` | generics | FFI and wasm can't export generics; adapters are chosen at runtime |
| Ports return `BoxFuture` (since M1) | `async fn` in traits, `async-trait` | object safe, no dependency in the core; the controller was the only presentation code that changed |
| API ports between presentation and use cases | an HTTP repository adapter | rules and permissions stay on the server; local and remote look the same to the controller |
| Role policy is one pure `can(role, action)` | database row-level security | every shell and test gets the same rules; RLS can come later as a safety net |
| Membership keyed by email | user ids plus invitations | invite before sign-up with no extra tables; revisit if emails change often |
| Outbox in the same transaction | publish events after commit | no lost or phantom events; consumers can be added later |
| Supabase only as an adapter | Supabase client SDKs in the shells | JWTs and plain Postgres keep the provider swappable |
| `taskr-core` has zero dependencies | serde derives on domain types | the core compiles for every target and can't do I/O by accident |
| Whole `TaskList` is the aggregate | `Task` as aggregate | the invariants span tasks, and the data is small; revisit with a server database |
| Fakes and contract tests | mocking libraries | tests describe behavior and survive refactoring |
| Columns and keys as data | hardcoded per shell | one generic table and generated help, as k9s does |
| UniFFI, one object per screen | many fine-grained FFI calls | a coarse, versionable, thread-safe boundary |
| XcodeGen spec in git | a checked-in `.xcodeproj` | the project is generated on a Mac; Linux can edit the spec |
| TypeScript types generated by `ts-rs` | hand-written TS types | the Svelte build fails when the Rust contract changes |

## Checklists {-}

### Adding a feature {-}

1. Name it in the ubiquitous language. If nobody outside engineering would name it, it's not domain.
2. Domain: add the rule to the aggregate, failing test first.
3. Application: add or extend a use case. New outside need? Add a port, a fake and contract cases.
4. Presentation: add the intent, effect, state change and ViewModel field. Add a fixture for a new screen state, and the message to `describe()`.
5. Facades: map new types in `taskr-ffi` and `taskr-wasm/src/dto.rs`, then regenerate (`scripts/gen-bindings.sh`, `cargo test -p taskr-wasm`).
6. Shells: render the new field, send the new intent. If a shell needs an `if` about business meaning, go back to step 4.

### Adding a platform {-}

1. Rust shell? `crates/`. Otherwise `apps/`.
2. Write the composition root: storage, clock, telemetry, identity adapters.
3. Pick the facade: direct Rust, `taskr-ffi`, or `taskr-wasm`.
4. Write a container (latest view, forwards intents, threading) and a pure view.
5. Add a smoke test that drives the real core through the real boundary.
6. Confirm `taskr-core` didn't change.

### Reviewing a shell change {-}

- Does any `if` depend on business meaning rather than a ViewModel field?
- Is any user-facing string built here instead of in the core?
- Does any core call happen on the UI thread?
- Is new local state really platform mechanics?

## Commands {-}

```sh
cargo test --workspace                 # all Rust tests
cargo clippy --workspace --all-targets
cargo run -p taskr-cli -- --help
cargo run -p taskr-tui

./scripts/build-web.sh                 # wasm core into apps/web (TASKR_WASM_BUILD_STD=1 without rustup)
(cd apps/web && npm run smoke && npm run build)
(cd apps/web && npm run test:ci && npm run e2e)   # Vitest, then Playwright on desktop and phone
(cd apps/web && npm run storybook)                # every screen, desktop and mobile

./scripts/gen-bindings.sh              # Kotlin and Swift bindings
./scripts/smoke-kotlin.sh              # needs kotlinc, java, JNA_JAR
./scripts/smoke-swift.sh               # needs a Swift toolchain
./scripts/setup-android-toolchain.sh   # JDK, Gradle, SDK, NDK, cargo-ndk for this user
./scripts/build-android.sh             # TASKR_ANDROID_BUILD_STD=1 TASKR_ANDROID_ABIS=x86_64 for Waydroid
./scripts/build-ios.sh                 # macOS only

docker compose -f deploy/dev/compose.yaml up -d                 # Postgres + Supabase Auth
TASKR_STORAGE=memory TASKR_DEV_AUTH=1 cargo run -p taskr-server # throwaway server
TASKR_TEST_DATABASE_URL=postgres://... cargo test --workspace    # include the Postgres tests

./scripts/build-book.sh                # this book, EPUB and PDF
./scripts/build-book.sh --check        # code listings still match the source
```

## Further reading {-}

- Alistair Cockburn, *Hexagonal Architecture* (the original ports and adapters article)
- Eric Evans, *Domain-Driven Design*, and Vaughn Vernon, *Implementing Domain-Driven Design* (aggregates in practice)
- Robert C. Martin, *Clean Architecture* (the dependency rule)
- The Elm guide, "The Elm Architecture" chapter, and the Crux documentation (shared Rust cores for mobile)
- *The Rust Programming Language* (the official book), chapters on ownership, enums, error handling and traits
- The UniFFI user guide, the wasm-bindgen guide, and the Svelte 5 runes documentation
- The k9s source code, especially its resource registry and key actions

## Changelog {-}

### First edition {-}

- The Core and Shells pattern, with taskr as the sample: one `tasks` context, telemetry as the cross-cutting concern, and CLI, TUI, web, Android and iOS shells.
- Primers on Rust, Svelte 5, Kotlin and Swift.
- Part four: databases, identity and RBAC, desktop platforms, servers on Kubernetes, realtime, messaging, analytics, static sites, content, and a family of apps on one platform.

### Web testing {-}

- Desktop and phone views for every web screen, Storybook grouped by `DesktopViews` and `MobileViews`, Vitest component tests, and Playwright end-to-end tests on both layouts (chapter 9).

### M1: signed in {-}

- New chapter 13: async ports, roles and the `can()` policy, workspaces, API ports, the Axum server, Supabase Auth, Postgres with an outbox, and `AppState` for sign-in and navigation.
- Chapters 2 and 4 to 12 updated for async ports, workspaces, revisions and the signed-in shells.

