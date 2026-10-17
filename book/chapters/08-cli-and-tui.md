# Shells in the terminal: CLI and TUI

The terminal shells are the easiest place to see the pattern, because they're Rust all the way down. There's no FFI and no second language, just a composition root and a renderer.

## The CLI: scriptable first

A CLI has two audiences: people at a prompt, and scripts. Scripts care about three things more than anything else: **stable output, machine-readable output, and exit codes**.

```text
taskr add Buy milk              Added #1 Buy milk
taskr list --scope all          table, driven by the shared column spec
taskr list --json               JSON for scripts and jq
taskr done 1                    Done #1 Buy milk
taskr -v add x                  telemetry as JSON lines on stderr
```

Arguments are parsed with `clap`'s derive API. The enum of subcommands *is* the documentation:

```{.rust file="crates/taskr-cli/src/main.rs"}
#[derive(Subcommand)]
enum Command {
    /// Add a task
    Add { title: Vec<String> },
```

### Exit codes are an API

Errors map to exit codes in one place, and the message comes from the shared `describe`:

```{.rust file="crates/taskr-cli/src/main.rs"}
impl From<TasksError> for Failure {
    fn from(e: TasksError) -> Self {
        let code = match e {
            TasksError::Rejected(_) => EXIT_REJECTED,
            TasksError::Storage(_) => EXIT_STORAGE,
        };
        Failure { message: describe(&e), code }
    }
}
```

`1` means "the rules said no" and `3` means "storage is broken" (`2` is clap's usage error). A script can tell "you already have that task" from "the disk is full" without parsing text.

### Reusing the screen logic in a CLI

`taskr list` without `--json` reuses the full presentation layer. It sends three intents to the same controller the TUI uses and prints the `TaskListVm` as a plain table:

```{.rust file="crates/taskr-cli/src/main.rs"}
            let mut screen = TaskListController::new(tasks);
            screen.dispatch(Intent::ScopeChanged(scope.into()));
            screen.dispatch(Intent::FilterChanged(filter.unwrap_or_default()));
            let vm = screen.dispatch(Intent::Opened);
```

So the CLI's empty-state messages, tab counts, filtering and columns are exactly the TUI's and the web's.

### Testing the real binary

Cargo builds the binary for integration tests and tells you where it is through `CARGO_BIN_EXE_<name>`. Each test gets its own data directory:

```{.rust file="crates/taskr-cli/tests/cli.rs"}
#[test]
fn rejections_use_exit_code_1_and_shared_messages() {
    let h = home("reject");
    taskr(&h, &["add", "Buy milk"]);
    let o = taskr(&h, &["add", "buy MILK"]);
    assert_eq!(o.status.code(), Some(1));
```

## The TUI: lessons from k9s

k9s is a terminal UI for Kubernetes. It's worth studying because it's used for hours a day by people who live in the keyboard, and it has to show hundreds of resource types without hundreds of custom screens. Four lessons from its source carry straight over:

1. **Every resource has the same shape.** A registry maps each resource type to an accessor, a renderer and a view. The renderer returns a header (columns with attributes like alignment, "wide" and "time") and rows of strings. One generic table draws them all. taskr's `COLUMNS` and `cells` are this idea.
2. **Keys are data.** Each view builds a map of key to `{description, action, visible, dangerous}`. The hint bar and the help screen are generated from that map, so they can't drift from what the keys do.
3. **Modes are explicit.** `:` for commands, `/` to filter, `esc` to back out, `space` to mark. Normal keys never fire while you're typing.
4. **Navigation is a stack.** Drilling down pushes a page, `esc` pops it, and breadcrumbs show where you are.

### Three files, three jobs

The TUI is split the same way as the other shells, just in Rust:

| File | Job | Pure? |
|---|---|---|
| `keymap.rs` | which key means which action, as a table | yes, data |
| `shell.rs` | terminal-only state: cursor, input mode, help overlay | yes, no I/O itself |
| `view.rs` | draw `Shell` into a ratatui frame | yes, a function of state |
| `main.rs` | composition root and the event loop | no, this is where I/O lives |

### Keys as data

```{.rust file="crates/taskr-tui/src/keymap.rs"}
pub struct Binding {
    pub keys: &'static [Key],
    pub label: &'static str,
    pub description: &'static str,
    pub action: Action,
    pub in_hint_bar: bool,
}
```

```{.rust file="crates/taskr-tui/src/keymap.rs"}
pub fn action_for(key: Key) -> Option<Action> {
    NORMAL_MODE.iter().find(|b| b.keys.contains(&key)).map(|b| b.action)
}

pub fn hint_bar() -> impl Iterator<Item = &'static Binding> {
    NORMAL_MODE.iter().filter(|b| b.in_hint_bar)
}
```

The bindings follow vim: `j`/`k` (and arrows) move, `g`/`G` jump to top and bottom, `ctrl-d`/`ctrl-u` move half a page, `/` filters, `esc` clears. Then the app's own: `space` toggles done, `a` adds, `1 2 3` and `tab` switch scope, `r` reloads, `?` shows every key, `q` quits.

A test makes sure no key is bound twice, so adding a binding can't silently shadow another.

### Modes

`shell.rs` has three modes:

```{.rust file="crates/taskr-tui/src/shell.rs"}
pub enum Mode {
    Normal,
    Adding(String),
    Filtering(String),
}
```

In `Adding` and `Filtering`, letters are text, not commands, so typing "quit jogging" adds a task instead of quitting. Filtering is live: every keystroke sends `FilterChanged` to the core, and the table updates as you type.

### What the shell does with an action

Cursor movement stays in the shell. Anything with product meaning becomes an intent:

```{.rust file="crates/taskr-tui/src/shell.rs"}
            Action::Toggle => {
                if let Some(id) = self.selected_id() {
                    self.send(Intent::ToggleRequested(id));
                }
            }
```

After every intent the shell clamps the cursor to the new row count. That's UI mechanics, and it's the only "logic" the TUI has.

### Rendering

`view.rs` lays out five rows: tabs, the input line, the table, a message line and the hint bar. The table comes straight from the ViewModel's column spec: left-aligned columns fill the space, right-aligned ones get exactly the width of their widest cell. Done rows are dimmed and crossed out.

### Testing a TUI without a terminal

ratatui has a `TestBackend` that renders into a buffer, so a test can check what's on screen:

```{.rust file="crates/taskr-tui/src/view.rs"}
    #[test]
    fn empty_screen_explains_itself_and_shows_hints() {
        let screen = render(&shell());
        assert!(screen.contains("Nothing here yet. Add your first task."), "{screen}");
        assert!(screen.contains("<j/↓> Move down"), "{screen}");
        assert!(screen.contains("Open (0)"), "{screen}");
    }
```

### CLI and TUI share data

Both read `$TASKR_HOME/tasks.json` through the same adapter. Every write reloads first and replaces the file atomically (write a temp file, then rename), and the TUI reloads every three seconds. So a task added with `taskr add` shows up in a running TUI on its own. Two writers at the exact same moment are still last-writer-wins. That's acceptable for a personal tool, and chapter 11 shows what changes when it isn't.

::: exercise
Add a k9s-style **command mode**: `:` opens a prompt, `:open`, `:done` and `:all` switch scope, `:q` quits. Keep the command names in a table next to `NORMAL_MODE`, generate their help lines from it, and test that an unknown command shows a message instead of doing nothing.
:::
