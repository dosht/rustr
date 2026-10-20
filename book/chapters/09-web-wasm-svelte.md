# The web shell: WebAssembly and Svelte

The web is the platform where the decision matters most. You can make it a thin client over an HTTP API, or you can run the core in the browser. taskr runs the core in the browser.

## Why the core runs in the browser

A thin client over HTTP looks simpler, but it has costs:

- The presentation logic (tabs, counts, filters, messages) has to be rewritten in TypeScript, which is the five-copies problem again.
- Nothing works offline.
- There are now two APIs to version: HTTP for web and UniFFI for mobile.

With WebAssembly, the browser gets the same `TaskListController` as every other shell. Storage is `localStorage` behind the same repository port, and the server, when it exists, becomes a sync backend rather than the web app's only brain. The cost is a 200 KB `.wasm` file (81 KB gzipped) and a build step.

## The wasm facade

`taskr-wasm` is the composition root for the browser. It plugs in browser adapters and exposes a small class with `wasm-bindgen`:

```{.rust file="crates/taskr-wasm/src/lib.rs"}
#[wasm_bindgen]
impl TaskListPresenter {
    #[wasm_bindgen(constructor)]
    pub fn new() -> TaskListPresenter {
        let telemetry: Arc<dyn Telemetry> = Arc::new(ConsoleTelemetry);
        telemetry.record(LogEvent::info("app.started").field("shell", "web"));
        let tasks =
            Tasks::new(Arc::new(LocalStorageTaskListRepository), Arc::new(BrowserClock), telemetry);
        TaskListPresenter { controller: Rc::new(TaskListController::local(tasks)) }
    }

    /// Takes a `TaskIntent`, resolves to a `TaskListView` (see the generated TS types).
    pub fn dispatch(&self, intent: JsValue) -> Result<js_sys::Promise, JsError> {
        let intent: TaskIntent = serde_wasm_bindgen::from_value(intent)?;
        let controller = self.controller.clone();
        Ok(future_to_promise(async move {
            let vm = controller.dispatch(intent.into()).await;
            to_js(&TaskListView::from(vm)).map_err(JsValue::from)
        }))
    }
```

Same controller as every other shell, three browser adapters, and a `dispatch` that takes plain JS objects and returns a Promise of one. The Promise is there because the core's ports are async (chapter 5); `future_to_promise` turns a Rust future into a JS Promise. The controller is shared through an `Rc` so the future can outlive the call.

The three browser adapters are tiny. `localStorage` is reached through the global object rather than `window`, so the same build also runs under Node for tests:

```{.rust file="crates/taskr-wasm/src/adapters.rs"}
impl TaskListRepository for LocalStorageTaskListRepository {
    fn load<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<(TaskList, Revision), StorageError>> {
        Box::pin(async move { Self::read(&key_for(workspace)) })
    }
```

The future is finished the moment it is created; `localStorage` is synchronous. On wasm32, `BoxFuture` drops the `Send` bound, because browser futures (like `fetch`) are never `Send` and there is only one thread.

It reuses `snapshot_json` from `taskr-infra` with the `native` feature off, so the browser stores exactly the same JSON format as the CLI.

## Types that can't drift

Values cross the boundary as plain JS objects through `serde-wasm-bindgen`. The facade has its own DTO types (it never exposes core types directly), and `ts-rs` generates TypeScript from them when `cargo test -p taskr-wasm` runs:

```{.rust file="crates/taskr-wasm/src/dto.rs"}
#[derive(Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type")]
#[ts(export)]
pub enum TaskIntent {
```

becomes, in `apps/web/src/lib/core/types/TaskIntent.ts`:

```typescript
export type TaskIntent =
  | { "type": "Opened" }
  | { "type": "AddSubmitted", title: string }
  | { "type": "ToggleRequested", id: number }
  | ...
```

A tagged union in TypeScript, checked by the compiler. Rename a field in Rust and the Svelte code stops compiling.

## A Svelte 5 primer

Svelte is a compiler. You write components in `.svelte` files with a `<script>`, markup and `<style>`, and Svelte compiles them to small, direct DOM updates, with no virtual DOM. Svelte 5 uses **runes**, special functions that mark reactive state:

| Rune | Meaning | React equivalent |
|---|---|---|
| `$state(value)` | reactive variable; assigning to it updates the UI | `useState` |
| `$derived(expr)` | value computed from other state | `useMemo` |
| `$effect(() => ...)` | run side effects when dependencies change | `useEffect` |
| `$props()` | the component's inputs | props |

Markup uses `{#if}`, `{#each items as item (item.id)}` and `{#await promise}` blocks. Events are plain attributes: `onclick={...}`. Styles in a component are scoped to it automatically.

Runes only work in `.svelte` files and in modules named `*.svelte.ts`, which is why the model file has that name.

## Container and view, Svelte style

The web shell follows the React developer guide's layering, adapted:

| Guide | Web shell file | Job |
|---|---|---|
| Page | `App.svelte` | load wasm, build the model, render the screen |
| Repository interface | `TaskCore` in `lib/core/index.ts` | the shell's port into the core |
| Model | `taskListModel.svelte.ts`, `appModel.svelte.ts` | hold the latest view, forward intents |
| Container | `TaskListContainer.svelte`, `AppContainer.svelte` | pick the desktop or the phone view, wire it to the model |
| View | `views/desktop/*.svelte`, `views/mobile/*.svelte` | pure: props in, intents out |

The `TaskCore` interface is the web shell's own port. Components depend on it, not on wasm, so a story or a test can pass a fake:

```{.typescript file="apps/web/src/lib/core/index.ts"}
export interface TaskCore {
  dispatch(intent: TaskIntent): Promise<TaskListView>;
  current(): TaskListView;
}
```

The container is a handful of lines. The state machine is in Rust, so there's nothing else to do:

```{.typescript file="apps/web/src/lib/tasks/taskListModel.svelte.ts"}
export async function createTaskListModel(core: TaskCore): Promise<TaskListModel> {
  let view = $state<TaskListView>(await core.dispatch({ type: "Opened" }));
  return {
    get view() {
      return view;
    },
    async send(intent) {
      await core.dispatch(intent);
      // The latest state, not this dispatch's, in case an earlier slow one finished last.
      view = core.current();
    },
  };
}
```

The composition root waits for the wasm module, then hands the model to the view. With `VITE_TASKR_API` set it builds the signed-in app instead (chapter 13); without it, tasks stay in this browser:

```{.html file="apps/web/src/App.svelte"}
{#if app}
  {#await app}
    <p class="boot">Loading…</p>
  {:then model}
    <AppContainer {model} mobile={viewport.mobile} devSignIn={!config?.authUrl} />
  {:catch error}
    <p class="boot">Could not start: {error.message}</p>
  {/await}
{:else if local}
  {#await local}
    <p class="boot">Loading…</p>
  {:then model}
    <TaskListContainer {model} mobile={viewport.mobile} />
  {:catch error}
    <p class="boot">Could not start: {error.message}</p>
  {/await}
{/if}
```

### Desktop and phone views

Each feature has two sets of pure views, as in the production app this pattern was checked against:

```text
lib/tasks/views/desktop/TaskListDesktop.svelte    a table, vim keys like the TUI
lib/tasks/views/mobile/TaskListMobile.svelte      tappable cards, an add bar at the bottom
lib/members/views/desktop|mobile/...              a list with pickers; one card per member
lib/app/views/desktop/DesktopFrame.svelte         sidebar: workspaces, sections, account
lib/app/views/mobile/MobileFrame.svelte           title bar, drawer for workspaces, bottom tabs
lib/app/views/SignInView.svelte                   one responsive view, it is a single form
```

They are separate components, not one component with breakpoints, because a phone is a different interaction (thumbs, drawers, bottom bars), not a narrower desktop. They render the **same** view model and send the **same** intents, so the difference is purely visual: a viewer's read-only list, a duplicate-title error and the members an owner can manage come from Rust either way.

The container decides which one to show from a viewport store (`lib/platform/viewport.svelte.ts`, a `matchMedia` query). Screen size is a shell mechanic, like focus, so it never reaches the core.

The desktop list renders tabs, the add form, the filter, the message banner and a table built from `view.columns` and `row.cells`. Its only local state is the add draft and a keyboard cursor. It even has the TUI's keys: `j`/`k` move, `x` toggles, `/` focuses the filter, `a` focuses the add box, `1 2 3` switch tabs.

## Building it

```sh
./scripts/build-web.sh   # cargo build --target wasm32-unknown-unknown, then wasm-bindgen
cd apps/web
npm run dev              # Vite dev server
npm run build            # svelte-check, then a production build
```

On a distro Rust without rustup (Arch, for example), the official wasm32 standard library does not match the compiler. `TASKR_WASM_BUILD_STD=1 ./scripts/build-web.sh` builds std from `rust-src` instead.

`wasm-bindgen --target web` writes a JS module that exports an `init()` function and the classes. Vite understands its `new URL("...wasm", import.meta.url)` pattern and bundles the `.wasm` file as an asset.

::: note
The `wasm-bindgen` CLI version must match the `wasm-bindgen` crate version exactly. taskr pins the crate with `=0.2.128` in the workspace so the two can't drift apart silently.
:::

## Testing the web shell

The web shell has the same test pyramid as the production app this pattern was checked against, with one difference that makes it cheaper: every screen state comes from Rust.

**Fixtures from the core.** `cargo test -p taskr-wasm` writes `lib/core/fixtures.json`: every named task screen (`sample`, `empty`, `error`, `read-only`, ...) and every app screen (`sign-in`, `sign-in-error`, `owner-tasks`, `viewer-tasks`, `owner-members`, ...), exactly as the core produces them. Stories and component tests import them, so they never drift from what the app really shows, and they need no wasm.

**Component tests (Vitest and Testing Library).** Vitest has Jest's API (`describe`, `it`, `expect`, `vi.fn()`) and runs Svelte through Vite with no extra setup. Each view gets a test per platform: it renders a fixture and checks that clicks become the right intents, that a viewer gets no add form, that the core's messages appear verbatim:

```{.typescript file="apps/web/src/lib/tasks/views/mobile/TaskListMobile.test.ts"}
  it("is read only for viewers", () => {
    render(TaskListMobile, { view: taskScreen("read-only"), onIntent: vi.fn() });
    expect(screen.queryByLabelText("New task title")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Write the architecture doc, open" })).toBeDisabled();
  });
```

The models (`appModel`, `taskListModel`) are tested with a fake `AppCore` or `TaskCore`, including the case where an older dispatch finishes after a newer one.

**Storybook.** Stories are grouped like the production app's: `Features/<Feature>/DesktopViews/...`, `Features/<Feature>/MobileViews/...` (opened in a phone viewport) and `Pages/App/...` for the full frames. Every story is a fixture. A Vitest test composes every story file and renders each story, so a broken story fails the test run instead of waiting for someone to open Storybook.

**End to end (Playwright).** `npm run e2e` starts a throwaway `taskr-server` (memory storage, dev sign-in) and two Vite servers, signed in and local, then runs every spec twice: on a desktop browser and on a Pixel 7 profile. Small helpers hide the layout difference (the sidebar on a desktop, the drawer and bottom tabs on a phone), so one scenario covers both sets of views: an owner adds tasks and hits the duplicate rule, a viewer sees the shared list read-only and still has a workspace of their own, a session survives a reload, the local mode keeps tasks across reloads.

**Node smoke test.** `npm run smoke` loads the real `.wasm` with `initSync` and runs a scenario under Node, the cheapest check that the wasm build and its bindings work.

```sh
npm run test:ci     # Vitest: views, models, every story
npm run e2e         # Playwright: desktop and phone, against a real server
npm run storybook   # browse every screen state
```

::: exercise
Make the web shell installable and fully offline: add a web app manifest and a small service worker that caches `index.html`, the JS bundle and the `.wasm` file. The core already stores everything locally, so nothing else needs to change. Then explain why that's true, in terms of ports.
:::
