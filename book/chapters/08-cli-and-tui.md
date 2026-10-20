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
        let code = match &e {
            TasksError::Rejected(_) => EXIT_REJECTED,
            TasksError::Access(a) => access_code(a),
            TasksError::Storage(_) => EXIT_STORAGE,
        };
        Failure { message: describe(&e), code }
    }
}
```

`1` means "the rules said no" (including "your role doesn't allow it"), `3` means "storage is broken" and `4` means "you are not signed in" (`2` is clap's usage error). A script can tell "you already have that task" from "the disk is full" without parsing text.

### Reusing the screen logic in a CLI

`taskr list` without `--json` reuses the full presentation layer. It sends three intents to the same controller the TUI uses and prints the `TaskListVm` as a plain table:

```{.rust file="crates/taskr-cli/src/main.rs"}
            let screen = TaskListController::new(t.api, t.workspace, clock);
            screen.dispatch(Intent::ScopeChanged(scope.into())).await;
            screen.dispatch(Intent::FilterChanged(filter.unwrap_or_default())).await;
            let vm = screen.dispatch(Intent::Opened).await;
```

So the CLI's empty-state messages, tab counts, filtering and columns are exactly the TUI's and the web's.

`t` is the command's target: the local files and `LocalTasks` when signed out, or a server workspace and the HTTP client after `taskr login`. The same three lines list either one.

### Two modes, same commands

```sh
taskr add Buy milk                     # local file
taskr login --server http://localhost:8787/api --auth-url http://localhost:9999 --email ada@example.com
taskr add Buy milk                     # now on the server, in your first workspace
taskr -w Team list                     # another workspace, by name or id
taskr members set bob@example.com viewer
taskr logout                           # back to the local file
```

`login` saves the server and the session next to the data, so the TUI opens signed in too.

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
    /// Only bound when signed in to a server (workspaces, members, sign out).
    pub remote_only: bool,
}
```

```{.rust file="crates/taskr-tui/src/keymap.rs"}
pub fn active(
    table: &'static [Binding],
    signed_in: bool,
) -> impl Iterator<Item = &'static Binding> {
    table.iter().filter(move |b| signed_in || !b.remote_only)
}

pub fn action_for(table: &'static [Binding], signed_in: bool, key: Key) -> Option<Action> {
    active(table, signed_in).find(|b| b.keys.contains(&key)).map(|b| b.action)
}
```

There is one table per screen (`TASKS_KEYS`, `MEMBERS_KEYS`). The hint bar and the help overlay are generated from the active table, so a key that only exists signed in never shows up in the local TUI.

The bindings follow vim: `j`/`k` (and arrows) move, `g`/`G` jump to top and bottom, `ctrl-d`/`ctrl-u` move half a page, `/` filters, `esc` clears. Then the app's own: `space` toggles done, `a` adds, `1 2 3` and `tab` switch scope, `r` reloads, `?` shows every key, `q` quits. Signed in, `w` cycles workspaces, `m` and `t` switch between members and tasks, and on the members screen `o`/`e`/`v` change a role and `d` removes.

A test makes sure no key is bound twice, so adding a binding can't silently shadow another.

### Modes

`shell.rs` has a mode per kind of typing:

```{.rust file="crates/taskr-tui/src/shell.rs"}
pub enum Mode {
    Normal,
    Adding(String),
    Filtering(String),
    AddingMember(String),
    SignInEmail(String),
    SignInPassword { email: String, password: String },
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

### Signed in

The shell holds one of two backends: a local `TaskListController`, or the shared `AppController` from chapter 13 with a single-threaded tokio runtime to drive its HTTP calls. Task intents go to whichever it has; signed in, they are wrapped as `AppIntent::Tasks`. Members, workspaces and sign-in are new screens, but no new decisions: who may change a role, which workspace is open and what an expired session does are all answered by the app controller, exactly as in the browser. Network calls block the loop while they run, which is what k9s does too.

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
