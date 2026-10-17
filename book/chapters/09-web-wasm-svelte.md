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
        TaskListPresenter { controller: TaskListController::new(tasks) }
    }

    /// Takes a `TaskIntent`, returns a `TaskListView` (see the generated TS types).
    pub fn dispatch(&mut self, intent: JsValue) -> Result<JsValue, JsError> {
        let intent: TaskIntent = serde_wasm_bindgen::from_value(intent)?;
        to_js(&TaskListView::from(self.controller.dispatch(intent.into())))
    }
```

Same controller as every other shell, three browser adapters, and a `dispatch` that takes and returns plain JS objects.

The three browser adapters are tiny. `localStorage` is reached through the global object rather than `window`, so the same build also runs under Node for tests:

```{.rust file="crates/taskr-wasm/src/adapters.rs"}
impl TaskListRepository for LocalStorageTaskListRepository {
    fn load(&self) -> Result<TaskList, StorageError> {
        match Self::storage()?.get_item(STORAGE_KEY) {
            Ok(Some(text)) => snapshot_json::decode(&text),
            Ok(None) => Ok(TaskList::new()),
            Err(_) => Err(StorageError("could not read localStorage".into())),
        }
    }
```

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
| Container | `taskListModel.svelte.ts` | hold the latest view, forward intents |
| View | `TaskListScreen.svelte` | pure: props in, intents out |

The `TaskCore` interface is the web shell's own port. Components depend on it, not on wasm, so a story or a test can pass a fake:

```{.typescript file="apps/web/src/lib/core/index.ts"}
export interface TaskCore {
  dispatch(intent: TaskIntent): TaskListView;
  current(): TaskListView;
}
```

The container is a handful of lines. The state machine is in Rust, so there's nothing else to do:

```{.typescript file="apps/web/src/lib/tasks/taskListModel.svelte.ts"}
export function createTaskListModel(core: TaskCore): TaskListModel {
  let view = $state<TaskListView>(core.dispatch({ type: "Opened" }));
  return {
    get view() {
      return view;
    },
    send(intent) {
      view = core.dispatch(intent);
    },
  };
}
```

The composition root waits for the wasm module, then hands the model to the view:

```{.html file="apps/web/src/App.svelte"}
{#await model}
  <p class="boot">Loading…</p>
{:then model}
  <TaskListScreen view={model.view} onIntent={model.send} />
{:catch error}
  <p class="boot">Could not start: {error.message}</p>
{/await}
```

`TaskListScreen.svelte` renders tabs, the add form, the filter, the message banner and a table built from `view.columns` and `row.cells`. Its only local state is the add draft and a keyboard cursor. It even has the TUI's keys: `j`/`k` move, `x` toggles, `/` focuses the filter, `a` focuses the add box, `1 2 3` switch tabs.

## Building it

```sh
./scripts/build-web.sh   # cargo build --target wasm32-unknown-unknown, then wasm-bindgen
cd apps/web
npm run dev              # Vite dev server
npm run build            # svelte-check, then a production build
```

`wasm-bindgen --target web` writes a JS module that exports an `init()` function and the classes. Vite understands its `new URL("...wasm", import.meta.url)` pattern and bundles the `.wasm` file as an asset.

::: note
The `wasm-bindgen` CLI version must match the `wasm-bindgen` crate version exactly. taskr pins the crate with `=0.2.128` in the workspace so the two can't drift apart silently.
:::

## Testing the web shell

Two checks, both against the real wasm build:

- **Node smoke test.** `npm run smoke` loads the `.wasm` file with `initSync`, runs a scenario (add, duplicate rejected, complete, reload from `localStorage`) and checks the fixtures. Node has a global `localStorage` when started with `--localstorage-file`, which is why the adapter looks it up on the global object.
- **Browser run.** A headless Chromium session fills the form, presses `j` and `x`, reloads, and checks that the data survived. That's the web's end-to-end test.

::: exercise
Make the web shell installable and fully offline: add a web app manifest and a small service worker that caches `index.html`, the JS bundle and the `.wasm` file. The core already stores everything locally, so nothing else needs to change. Then explain why that's true, in terms of ports.
:::
