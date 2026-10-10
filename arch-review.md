# Architecture Review: Rust Multiplatform Application

Reviewed: `rust-multiplatform-application-architecture.md`
Compared against: `~/Knowledge/frontend/developer-guide/` (MVVM + DDD + props DI + test pyramid, Next.js)
Date: 2026-10-09
Status: initial thoughts, open for discussion

## Overall

The direction is right. Hexagonal core, thin adapters, native UI per platform, UniFFI as a deliberate public API, and offline/sync treated as product behavior are all good calls. The doc is strongest on boundaries and weakest on the presentation layer: it says "MVVM at UI boundaries" but never says where a ViewModel lives, what it contains, or how it is tested. With five UIs (web, Android, iOS, TUI, CLI) that is the decision with the most duplication risk, and it is the part where the developer guide has the most to teach.

## 1. What the developer guide does, translated

| Guide concept (TS/React) | Role | Rust equivalent |
|---|---|---|
| `features/<feature>/` bounded context | Vertical slice | One module (or crate) per bounded context, not per layer |
| `domain/` entities, VOs, aggregates | Business rules | `domain` module inside the context |
| `services/I*Repository.ts` | Port | Trait owned by the context's application layer |
| `services/*Repository.ts` | Adapter | Struct in an infra crate implementing the trait |
| ViewModel | State + orchestration | Split in two, see section 2 |
| View (pure, props only) | Render | SwiftUI / Compose / Svelte / ratatui widget rendering a ViewModel struct |
| Container | Wiring VM to View | Platform shell (Activity/Scene/Svelte route/TUI app loop) |
| Page (creates clients + repos) | Composition root | `main.rs` of each binary, and the UniFFI facade constructor |
| Props-based DI, no service locator | Explicit deps | Constructor injection, no globals, no `lazy_static` registries |
| Infrastructure vs domain rule | Don't DDD the plumbing | Same rule; navigation, theming, logging stay out of `domain` |
| UI facade (settings, dashboard) | Composes contexts, no rules | Facade ViewModel that composes use cases from several contexts |
| Storybook | View in isolation | Previews + snapshot tests per platform driven by fixture ViewModels |
| Jest VM tests with interface mocks | Fast behavior tests | Pure state-transition tests, no mocks needed (section 4) |

Almost everything carries over. Two things need to change, below.

## 2. The ViewModel must be split, and probably shared

In the guide, the ViewModel holds "business logic + state". That works with one UI. Here it would mean writing the same logic in Swift, Kotlin, TypeScript and Rust (TUI). The Rust doc already avoids that by pushing business rules into domain/application, so the old ViewModel splits into:

1. **Use case** (shared Rust, application layer): business decisions, transactions, ports.
2. **Presentation logic**: loading/error/empty states, pagination, form state, field-level validation messages, selection, derived display flags, what a button does in which state.

Item 2 is still a lot of code, and it is identical on every platform. If it lives in each shell we get four copies that drift. Proposal: a **shared presentation core in Rust**.

```rust
pub struct ProjectListVm {
    pub rows: Vec<ProjectRow>,
    pub status: LoadStatus,
    pub can_create: bool,
    pub error_banner: Option<String>,
}

pub enum ProjectListEvent {
    Opened,
    Loaded(Result<Vec<ProjectSummary>, LoadError>),
    CreateTapped { name: String },
}

pub enum Effect {
    LoadProjects,
    CreateProject(CreateProjectCommand),
    Navigate(Route),
}

impl ProjectListState {
    pub fn update(&mut self, event: ProjectListEvent) -> Vec<Effect> { /* pure */ }
    pub fn view(&self) -> ProjectListVm { /* pure */ }
}
```

The state machine is pure: no async, no ports, no I/O. A small runtime executes `Effect`s by calling use cases and feeds results back as events. Shells only render `ProjectListVm` and forward user input as events. This is the Elm / Crux model (redbadger/crux does exactly this for Rust + Swift + Kotlin + web). We don't have to adopt Crux, but the shape is worth copying.

What stays native in each shell, matching the doc's "keep platform-native UI behavior native": navigation stack mechanics, lifecycle, permissions, animations, platform widgets, accessibility wiring. `Effect::Navigate(Route)` says *where*, the shell decides *how*.

Mapping back to the guide: `ProjectListVm` is the "props" of the pure View, the shell is the Container, the binary's `main` / UniFFI facade is the Page.

**Tradeoff to discuss:** a shared presentation core means every screen change touches Rust and regenerates bindings. Teams who want SwiftUI/Compose devs to own screens end to end may push back. A middle ground: shared presentation core only for screens with real logic (forms, lists with filters, sync status), plain native VMs for trivial screens.

## 3. Web is the odd one out

The doc has Svelte talk to Axum over HTTP. That means:

- Web cannot reuse the shared presentation core (section 2), so web gets its own ViewModels in TS.
- Web gets no offline, while the doc makes offline/sync a first-class product concern.
- There are two API surfaces to version: HTTP for web and UniFFI for mobile.

Options:

- **A. Thin web client** (current doc). Simple, but accept duplicated presentation logic and online-only web. Generate TS types from Rust (`ts-rs` or `specta`) so at least the DTOs don't drift.
- **B. Core in WASM** (`wasm-bindgen`), same presentation core as mobile, local storage via SQLite-on-OPFS or IndexedDB adapter, Axum becomes the sync server. One behavior model everywhere, but bigger bundle and more complex build.

This choice changes the crate layout and the ports (storage adapter per target), so it should be made early. Also worth noting: the developer guide is React/Next; choosing Svelte means none of the guide's Storybook/RTL/hook patterns transfer directly. That's fine if intentional, just call it out.

## 4. TDD: how it looks in Rust

The guide's tests mock interfaces with `jest.fn()`. In Rust, the better default is **hand-written fakes over mock libraries**:

- Every port gets an in-memory implementation in a `testing` module (or `*-fakes` crate behind a feature flag).
- The doc's "contract tests against every adapter" become one test suite, generic over the trait, run against both the fake and the real adapter. That is what keeps the fake honest. This is the Rust replacement for "service tests with real Supabase local instance".
- `mockall` only where we need to assert a call happened that has no observable state (rare).

Suggested TDD loop per feature, outside in:

1. **Presentation test** (pure, sync): given state, send event, assert `view()` and emitted effects. Covers what the guide's `ViewModel.test.ts` covers (loading, error, selection, computed), with zero mocks because effects are data.
2. **Use case test**: in-memory fakes for ports, assert outcome and persisted state.
3. **Domain test**: invariants, plus `proptest` for value objects and state transitions.
4. **Adapter contract test**: same suite against fake and real (SQLite, HTTP).
5. **Shell tests**, few: render fixture ViewModels.

For point 5, the Storybook equivalent: keep a set of named fixture ViewModels in Rust (`ProjectListVm::loading()`, `::empty()`, `::with_error()`), export them through UniFFI, and use them in SwiftUI previews, Compose previews (Paparazzi for snapshots), Svelte Storybook, and ratatui `TestBackend` + `insta` snapshots. One fixture set, every platform renders the same states.

The doc's testing section is already good on adapters and sync. It's missing the presentation layer, which is where most UI bugs live.

## 5. Crate layout: slice by context, not by layer

The proposed `domain / application / ports / infrastructure` crates are horizontal layers. The guide organizes by feature (bounded context), and that's the better default here too:

- Layer crates mean every feature touches 4+ crates, and the `domain` crate becomes one big bag of every context.
- `ports` as its own crate separates traits from the use cases that own them. In hexagonal, the application layer owns its ports; putting them in a separate crate weakens that.

Suggested starting point (fewer crates, split later when compile time asks for it):

```text
crates/
  core/                 # pure: no tokio, no sqlx, no serde in domain types
    src/
      projects/         # bounded context
        domain.rs
        app.rs          # use cases + port traits
        presentation.rs # state machines + ViewModels (if section 2 is accepted)
      sync/             # sync rules that are business rules (conflict policy)
      shared_kernel/    # ids, time, money, etc.
  infra-sqlite/
  infra-http/
  runtime/              # effect executor, composition helpers
  server/  cli/  tui/  ffi/
apps/ web/ android/ ios/
```

Enforce the dependency direction with crate boundaries for the important rule (core has no I/O deps) and with module visibility (`pub(crate)`) for the rest.

## 6. Smaller issues and gaps

- **Code sample leaks domain types.** `CreateProject::execute` returns `Project`. The doc itself says don't expose domain objects across FFI. Return a DTO or an id, or have the facade map it, and say which.
- **Generics vs trait objects.** `CreateProject<R>` is fine inside Rust, but UniFFI can't export generics and native `async fn` in traits is not dyn-compatible. Pick a rule: e.g. use cases hold `Arc<dyn Port>` with `async_trait` (or boxed futures) at the boundary, and generics only inside hot paths.
- **Async runtime on mobile.** Who owns the tokio runtime inside the FFI library, how cancellation maps to Swift `Task` / Kotlin coroutines, and which thread callbacks arrive on. The doc lists these as things to define; they need concrete answers before the first screen.
- **Conflict resolution is domain, not infra.** The doc puts sync under infrastructure. The queue, transport and checkpoints are infra; "which edit wins" is a business rule and belongs in each context's domain, tested like any other rule.
- **IDs and time.** Offline creation needs client-generated IDs (UUIDv7) and a `Clock` port. Worth stating since everything else depends on it.
- **Domain events.** Not mentioned. Sync, notifications and cross-context reactions usually want them. Decide early whether aggregates emit events.
- **Unit of work.** "Transaction boundaries" is listed as a port but there's no model for multi-aggregate writes, especially across the local DB + outbound queue (outbox pattern would fit).
- **Error taxonomy.** Domain error, application error, and how each maps to HTTP status, CLI exit code, FFI exception and user-facing message. One table would prevent a lot of drift.
- **Infrastructure vs domain rule from the guide.** Worth copying in nearly verbatim: navigation, theming, TUI layout state, etc. are not domains and should not get aggregates or repositories.
- **Scope.** 10 crates and 6 delivery targets up front is a lot. Suggest a walking skeleton: one context, core + sqlite + CLI + one mobile platform end to end, then add targets.

## Open questions

1. Shared Rust presentation core (section 2): yes, no, or only for non-trivial screens?
2. Web: thin client over HTTP, or core in WASM with offline?
3. Is Svelte a deliberate break from the React-based guide, or open?
4. Slice crates by bounded context (section 5)?
5. What is the first bounded context / walking skeleton?
