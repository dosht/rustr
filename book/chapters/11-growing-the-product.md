# Growing the product

taskr is small on purpose. This chapter takes the questions that come up as a real product grows (more platforms, shell-only features, a real database, users and permissions) and answers each one in terms of the pattern. Code here is a sketch unless it has a source path.

## More platforms: macOS, Windows, Linux desktop

The rule from chapter 3 decides where a new shell lives: **Rust shells go in `crates/`, everything else in `apps/`**. Then pick the toolkit by how native it has to feel:

| Option | Lives in | Talks to the core through | Good for |
|---|---|---|---|
| SwiftUI for macOS | `apps/ios` or `apps/macos` | the same `taskr-ffi` and Swift bindings, plus a macOS slice in the xcframework | a fully native Mac app, almost free if you have iOS |
| Tauri | `crates/taskr-desktop` + reuse `apps/web` | direct Rust calls (no wasm, no FFI) behind Tauri commands | one desktop shell for macOS, Windows and Linux, reusing the Svelte UI |
| gtk4-rs, Slint, iced | `crates/taskr-gtk` etc. | direct Rust calls | native-looking Linux (Wayland) apps with no JS |
| WinUI (C#) | `apps/windows` | a C# binding generator for UniFFI (third-party) | a fully native Windows app |

The core never changes. What each new shell adds:

- a **composition root** choosing adapters
- a **telemetry adapter** for the platform's log (journald on Linux, the Windows Event Log, os_log on macOS)
- a **storage location**: `dirs::data_dir()` already returns the right folder on every desktop OS
- **packaging**: `.dmg` and notarization, MSIX or an installer, Flatpak or AppImage

::: tip
Tauri is the cheapest route to all three desktops: the Svelte views from `apps/web` are reused as-is, and the `TaskCore` interface gets a second implementation that calls Tauri commands instead of wasm. That's the payoff of the web shell having its own port.
:::

## Shell-only features: the sidebar question

Say the web app needs a collapsible sidebar with navigation. It's not a domain feature, and it's not infrastructure every app shares. Where does it go?

Use the test from chapter 2 (would a stakeholder describe it in business terms?) and then ask *how many platforms have it*:

1. **Only the web has it, and it's pure mechanics** (open, closed, which item is highlighted). It's shell code: `apps/web/src/lib/shell/Sidebar.svelte` with local state. No Rust, no ports. This is the React guide's "navigation is infrastructure" case.
2. **It shows data from several contexts** (task counts, an unread badge from another context). It's a **UI facade**: it composes existing ViewModels and adds no rules. Still shell code, but it reads from the core.
3. **Several platforms need the same items and rules** ("Billing appears only for admins", "badges show unread counts"). Now it's product behavior. Move it into the core as a presentation module, for example `presentation/navigation.rs` with a `NavigationVm { items, badges }`. It's still not domain: there's no aggregate and no repository.

Start at 1 and move down only when a second platform needs it. Moving shell code into the core later is cheap. Pulling wrongly-shared code back out is not.

## A real database

The database connects at exactly one place: an **adapter that implements a port** owned by the application layer. Domain and use cases never know it exists.

```text
crates/
  taskr-infra/            JSON file (today)
  taskr-infra-sqlite/     rusqlite: CLI, TUI, mobile (local)
  taskr-infra-postgres/   sqlx: the server
```

Each adapter owns its migrations and passes the same `task_list_repository_contract` as every other repository. If it passes, every use case test is valid for it.

Three things change when the data gets real.

### The aggregate gets smaller

Loading the whole list to add one task is fine for a few hundred tasks, and wrong for a team with a million. The fix is a domain change, not a database trick:

- `Task` becomes the aggregate. The repository loads and saves one task.
- The cross-task rule ("open titles are unique") moves to two places: a **domain check** in the use case (`repo.open_title_exists(&title)`) for a good error message, and a **unique index** in the database (on lowercased title where status is open) as the real guarantee against races.
- The "at most 50 open tasks" rule becomes a counted query, or gets rethought with the product owner. Rules that need the whole data set are expensive, and that's worth a conversation.

### Ports become async

Network databases are async in Rust. The port changes shape:

```rust
pub trait TaskRepository: Send + Sync {
    fn get(&self, id: TaskId) -> impl Future<Output = Result<Option<Task>, StorageError>> + Send;
    fn save(&self, task: &Task) -> impl Future<Output = Result<(), StorageError>> + Send;
}
```

Use cases become `async fn`. The presentation state machine **doesn't change**: it already treats "load requested" and "loaded" as separate events. Only the controller (now awaiting effects) and the facades (now exposing async functions) change. UniFFI supports async functions that become Kotlin `suspend` functions and Swift `async` functions.

### Concurrent writers need versions

With many writers, last-writer-wins loses data. Add a `version` to each aggregate, and make `save` conditional (`UPDATE ... WHERE id = ? AND version = ?`). A mismatch becomes a `StorageError::Conflict`, and the presentation layer turns it into "Someone else changed this task. Reload?".

## Identity, RBAC and the super admin

Authorization splits cleanly into two questions, and they live in different layers.

### Who is this? (cross-cutting infrastructure)

Identity is a port in the shared kernel. Every adapter produces the same thing:

```rust
pub struct Actor {
    pub id: ActorId,
    pub roles: Vec<Role>,
    pub tenant: Option<TenantId>,
}

pub trait Identity: Send + Sync {
    fn current(&self) -> Result<Actor, AuthError>;
}
```

You can have several adapters at once, which answers the "normal users plus me as super admin" question:

| Who | How they prove it | Adapter |
|---|---|---|
| App users | sign up and log in with Supabase Auth (or any OIDC provider); the app sends a JWT | verify the JWT against the provider's JWKS, map claims to `Actor` |
| You, super admin | an mTLS client certificate, with a kubeconfig-style context file | the server checks the certificate's subject and maps it to `roles: [SuperAdmin]` |

Guidelines for the super admin path:

- Copy kubeconfig's **format** (named contexts with a server URL and a client certificate), but don't reuse real Kubernetes admin credentials to log into an app.
- Use **short-lived certificates** (cert-manager or step-ca) and a **separate admin hostname** that only accepts mTLS.
- **Audit everything**: every super admin action records a telemetry event with the actor, the action and the target.

### What may they do? (business rules)

Permissions are part of each bounded context, because "who may complete a task" is a product rule. Each context has a policy:

```rust
pub enum TaskAction { View, Create, Complete, Reopen, Delete }

pub struct TaskPolicy;

impl TaskPolicy {
    pub fn allows(actor: &Actor, action: TaskAction, task: Option<&Task>) -> bool {
        if actor.has(Role::SuperAdmin) {
            return true;
        }
        match action {
            TaskAction::View | TaskAction::Create => actor.has(Role::Member),
            _ => task.is_some_and(|t| t.owner() == actor.id) || actor.has(Role::TeamLead),
        }
    }
}
```

and **use cases check it before touching the aggregate**:

```rust
pub async fn complete(&self, actor: &Actor, id: u32) -> Result<TaskSummary, TasksError> {
    let task = self.repo.get(TaskId(id)).await?.ok_or(TaskError::NotFound(TaskId(id)))?;
    if !TaskPolicy::allows(actor, TaskAction::Complete, Some(&task)) {
        return Err(TasksError::Forbidden);
    }
    // ...
}
```

Three rules keep this sound:

1. **Enforce in use cases, always.** Shells and facades never decide permissions.
2. **Reuse the policy for display.** The presentation layer calls the same `TaskPolicy` to set `can_complete` on each row, so buttons hide or disable correctly on every platform. That's convenience, not security.
3. **The server enforces again.** A client can be modified. When there's a server, it runs the same use cases with the same policies, and it's the one that counts.

A super admin is just a role the policies allow, never a code path that skips the use cases. That's what keeps super admin actions audited and validated like everyone else's.

A shared `access` crate holds the `Role` enum, the role-to-permission mapping and test helpers, so every app in a product family speaks the same RBAC language (chapter 12).

::: exercise
Sketch `Identity` for the CLI: `taskr login` runs the OIDC **device flow** (show a code, the user approves in a browser), stores the token in the OS keyring, and `Identity::current()` reads it. Which parts are adapters, which part is a port, and which part is a use case?
:::
