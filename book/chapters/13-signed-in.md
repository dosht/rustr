# Signed in: many people, one core

Until now taskr ran on one device for one person. This chapter is milestone M1 of the roadmap: people sign in, get a workspace, invite others with a role, and every change is also written as a domain event for later consumers. It is the first real test of the pattern's main claim, that the core survives a big product change. It did: the domain was untouched, the screen state machines gained one field each, and every shell kept working signed out.

```mermaid
%% caption: The signed-in mode. Clients run the same state machines; the server runs the same use cases.
flowchart LR
    subgraph Client["Client shells"]
        Web["Web (wasm)"]
        Cli["CLI, TUI"]
        Mobile["Android, iOS"]
    end
    subgraph CoreC["taskr-core, client side"]
        App["AppController<br/>AppState"]
        Ports["TasksApi, WorkspacesApi<br/>AuthClient, SessionStore"]
    end
    subgraph Net["taskr-client"]
        Http["HttpApi"]
        Auth["SupabaseAuth, DevAuth"]
    end
    subgraph Server["taskr-server"]
        Routes["Axum routes"]
        InProc["InProcessApi<br/>(one per request)"]
        UC["Tasks, Workspaces<br/>use cases"]
    end
    Pg[("Postgres<br/>+ outbox")]
    GoTrue["Supabase Auth"]
    Web --> App
    Cli --> App
    Mobile --> App
    App --> Ports
    Http -.implements.-> Ports
    Auth -.implements.-> Ports
    Http -->|"JSON (taskr-api)"| Routes
    Auth --> GoTrue
    Routes --> InProc --> UC --> Pg
```

## Async ports without a runtime in the core

A network call can't return immediately, so every port now returns a future. The core still has no dependencies, so it defines its own boxed future type:

```{.rust file="crates/taskr-core/src/shared/future.rs"}
#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
```

Boxing keeps the ports object safe: use cases hold `Arc<dyn TaskListRepository>`, so one binary can choose an adapter at runtime. (`async fn` in traits exists in Rust now, but it isn't object safe.) On wasm32 the alias drops `Send`, because browser futures never are.

Adapters that finish immediately (a file, `localStorage`) return a future that is already done. For those, the core has a twenty-line `block_on` built on `std::thread::park`, which the TUI, the mobile facade and the tests use. Networked shells use a real runtime: tokio in the CLI, the server and the mobile `AppPresenter`, the browser's event loop on the web.

## Who is acting, and what may they do

The shared kernel gained an `access` module. A **principal** is a signed-in person as the identity provider vouches for them. An **actor** is that person inside one workspace, with a role:

```{.rust file="crates/taskr-core/src/shared/access.rs"}
pub struct Actor {
    pub principal: Principal,
    pub workspace: WorkspaceId,
    pub role: Role,
}
```

The whole role policy is one pure function:

```{.rust file="crates/taskr-core/src/shared/access.rs"}
pub fn can(role: Role, action: Action) -> bool {
    match action {
        Action::ViewTasks | Action::ViewMembers => true,
        Action::ChangeTasks => role >= Role::Editor,
        Action::ManageMembers => role == Role::Owner,
    }
}
```

Use cases enforce it before they touch an aggregate (`actor.ensure(Action::ChangeTasks)?`, chapter 5). Presentation reads it to decide what to show: `TaskListVm::can_change` and `MembersVm::can_manage` come from the same function, so a viewer never sees an add box, on any platform. That's convenience. The server runs the same use cases, so a modified client still gets `403`.

## Workspaces: a second bounded context

Membership has a rule that spans members, "a workspace always keeps at least one owner", so the workspace is an aggregate with its members inside:

```{.rust file="crates/taskr-core/src/workspaces/domain.rs"}
    pub fn set_member(&mut self, email: Email, role: Role) -> Result<(), WorkspaceError> {
        match self.members.iter().position(|m| m.email == email) {
            Some(i) => {
                if role != Role::Owner && self.is_last_owner(&email) {
                    return Err(WorkspaceError::LastOwner);
                }
                self.members[i].role = role;
            }
            None => self.members.push(Member { email, role }),
        }
        Ok(())
    }
```

It follows every pattern from chapter 5: a value object (`Email`, parsed and lowercased), `snapshot` and `restore` for storage, a repository port with revisions, use cases that load, apply, save and retry. On sign-in, anyone who owns no workspace yet gets one named after them, even if they were invited somewhere first, so everyone can keep their own tasks. That is the whole onboarding for now. (The first version skipped invited people; a review session found it, and the scenario test now says what a person expects.)

Two review findings shaped the use cases further, and both are about time passing between a check and a write.

**Permission is checked on the aggregate being saved.** The server resolves the actor when a request starts, but a membership change could land before that request saves. Checking the captured role once let a removed owner's in-flight request put them back. So membership changes check the caller's role in the workspace they just loaded, on every retry, and the save only lands on that revision:

```{.rust file="crates/taskr-core/src/workspaces/app.rs"}
            let role = ws.role_of(&actor.principal.email).ok_or(AccessError::NotAMember)?;
            Actor { role, ..actor.clone() }.ensure(Action::ManageMembers)?;
```

Task changes keep the role resolved at the start of the request: tasks and members are separate aggregates, so a member removed mid-request can still land that one write. That window is accepted, and written down here.

**Onboarding is idempotent.** Two first visits at once (two tabs) both saw "no workspace yet". The personal workspace now has a stable id per person (`WorkspaceRepository::personal_id`, a UUIDv5 of the user id in Postgres), so the second creation is a revision conflict and both requests answer with the same workspace.

Membership is keyed by email, so you can invite someone before they have an account. The cost is that changing your email loses access until an owner adds the new one; the alternative, user ids plus pending invitations, is a known follow-up.

## The API ports: one seam for local and remote

The presentation layer doesn't call use cases directly any more. It calls an **API port**, which says what a client may ask for, one workspace at a time, with the caller already known:

```{.rust file="crates/taskr-core/src/tasks/app.rs"}
pub trait TasksApi: Send + Sync {
    /// Every task in the workspace, ordered like `Tasks::list`.
    fn list<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Vec<TaskSummary>, TasksError>>;
```

Three implementations answer it:

| Implementation | Where | What it does |
|---|---|---|
| `LocalTasks` | core | runs the use cases in process as the owner of the local workspace (signed out) |
| `InProcessApi` | core | resolves the actor for a principal in a workspace, then runs the use cases (the server, and tests) |
| `HttpApi` | `taskr-client` | sends the same calls over HTTP with the session's bearer token |

`InProcessApi` is the server's whole business layer:

```{.rust file="crates/taskr-core/src/api.rs"}
impl TasksApi for InProcessApi {
    fn list<'a>(
        &'a self,
        ws: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Vec<TaskSummary>, TasksError>> {
        Box::pin(async move { self.tasks.list(&self.task_actor(ws).await?, Scope::All).await })
    }
```

## The server is a thin mapping

`taskr-server` verifies the bearer token through the `Identity` port, builds an `InProcessApi` for that person, and calls the matching method. A handler is three lines:

```{.rust file="crates/taskr-server/src/lib.rs"}
async fn add_task(
    caller: Caller,
    ws: Result<Path<String>, PathRejection>,
    body: Result<Json<AddTaskBody>, JsonRejection>,
) -> Result<(StatusCode, Json<TaskDto>), ApiError> {
    let ws = WorkspaceId(ws?.0);
    let task = caller.0.add(&ws, body?.0.title).await?;
    Ok((StatusCode::CREATED, Json(task.into())))
}
```

There are two `Identity` adapters. `SupabaseJwt` verifies Supabase access tokens, with the project's shared secret (HS256) or its published keys (JWKS). `DevIdentity` accepts `dev:<email>` and is only wired in when the server is started with `TASKR_DEV_AUTH=1`, so local work and tests need no auth server.

Every request gets an id, and every log line written while serving it carries the request id, the user id and the build, the lesson from a production app where one request's lines could not be found together. The response has an `x-request-id` header with the same id.

### One contract for both ends

`taskr-api` is a small crate both the server and the client compile against: paths, JSON bodies and error codes. Every core error maps to a status and a code, and back:

```{.rust file="crates/taskr-api/src/errors.rs"}
fn access_parts(e: &AccessError) -> (u16, &'static str, Option<String>) {
    match e {
        AccessError::Unauthenticated => (401, "unauthenticated", None),
        AccessError::NotAMember => (404, "not_a_member", None),
        AccessError::Forbidden(a) => (403, "forbidden", Some(action_name(*a).into())),
    }
}
```

A test round-trips every error variant through JSON. That's what lets `describe()` produce the same sentence in the CLI, the browser and the phone, with the server in between.

## Postgres and the outbox

`taskr-postgres` implements both repository ports with plain `sqlx`, so any Postgres works: Supabase's, Neon, RDS, or a Docker container. Aggregates are stored as rows, loaded in one statement (one consistent snapshot) and saved in one transaction. The transaction moves the revision, rewrites the rows, and writes the change's domain events to an `outbox` table.

Moving the revision is a plain `insert` for a first save and `update … where revision = $expected` afterwards. The first version locked the row with `select … for update` and then upserted, which looks safe but isn't: a row that doesn't exist yet can't be locked, so two first saves both passed and the second silently replaced the first. Now the second insert hits the primary key, and either statement touching anything other than exactly one row is a conflict the use case retries:

```{.rust file="crates/taskr-postgres/src/lib.rs"}
fn written_once(result: Result<PgQueryResult, sqlx::Error>) -> Result<(), SaveError> {
    match result.map_err(save_failed)?.rows_affected() {
        1 => Ok(()),
        _ => Err(SaveError::Conflict),
    }
}
```

A Postgres test holds one first save open in a transaction, starts another behind it, commits, and expects the conflict. The outbox:

```{.sql file="crates/taskr-postgres/migrations/0001_workspaces_tasks_outbox.sql"}
create table outbox (
    id bigserial primary key,
    workspace_id uuid not null,
    name text not null,
    actor text not null,
    occurred_at bigint not null,
    payload jsonb not null,
    published_at timestamptz
);
```

The outbox is the hinge for everything after M1. Analytics, realtime and ETL read it in order and mark rows published; none of them needs a use case changed. Because the events are written in the same transaction as the change, there is never an event for a change that didn't happen, or a change without its event.

Both repositories pass the same contract tests as the in-memory fakes (chapter 7).

## AppState: sign-in and navigation as state

The client side needs a parent state machine above the task list: are we signed in, which workspace is open, which screen is showing. That's `AppState`, and it composes the task list and members state machines as children, the way a production MVVM app composes a list and a detail view model under one coordinator.

Navigation here is state, not mechanics. `AppState` decides *which* workspace and screen are open; each shell decides *how* that looks: a sidebar on the web, a drawer on Android, a split view on iOS, key bindings in the TUI.

Four details show why this belongs in the core. They all come from one fact: answers arrive later than the state they were asked from.

**Late answers are dropped.** Every result carries the workspace it was for and the sign-in it was made under (an `Epoch`, counted up on every sign-in and sign-out). A slow load for a workspace you already left doesn't overwrite the one you're looking at, and an answer for the previous session, even a `401`, can't sign out or fill in the next one:

```{.rust file="crates/taskr-core/src/session/presentation.rs"}
            AppEvent::MeLoaded(epoch, _)
            | AppEvent::Tasks(epoch, ..)
            | AppEvent::Members(epoch, ..)
                if epoch != self.epoch =>
```

**Screen intents name their workspace.** `SignedInVm::workspace` says which workspace the screen shows, and shells send it back with every task or member intent. An intent from a screen that was already replaced is dropped, so a form still on screen while the next workspace loads can't write into it:

```{.rust file="crates/taskr-core/src/session/presentation.rs"}
            AppIntent::Tasks(ws, intent) => match self.open_in(&ws) {
```

Shells also render the pending screen right away: `AppController::dispatch_with` hands them the view each time requests are about to run, not only when they all answered.

**The role follows what the server says.** The role from sign-in can go stale: an owner demotes themselves, or someone else changes it. When the members list loads, your own row sets the role for the navigation and both screens. A refused request (`403`, `404`) reloads `/me`; if you were removed, the first remaining workspace opens with "You are no longer a member of …".

**An expired session signs you out from anywhere.** Any `Unauthenticated` answer for the current session, from any screen, clears the stored session and shows the sign-in screen with "Your session has ended. Sign in again." Written once, it behaves the same in the browser, the terminal and on the phone.

Sessions themselves sit behind two client ports: `AuthClient` (sign in and sign up, implemented by `SupabaseAuth` against Supabase Auth's REST API, and by `DevAuth`) and `SessionStore` (where the session lives between launches: `localStorage`, a file readable only by its owner, the Android preferences or the iOS Keychain).

## How it was verified

The milestone was built to be checked by an agent end to end, without a person clicking:

- **Contracts.** The repository contract tests run against the in-memory fakes, the JSON files and Postgres.
- **HTTP end to end.** `taskr-server/tests/http.rs` starts a real server on a random port and drives it with the real `HttpApi` and the shared `AppController`, as two people, on memory and on Postgres: onboarding, adding, a duplicate rejected with the shared message, inviting a viewer, the server refusing the viewer's writes even when the client is bypassed, strangers seeing `not_a_member`, and an invalid token signing the person out.
- **Real binaries.** The CLI against the server with dev auth, and against a GoTrue container (Supabase Auth) for wrong password, sign-up, duplicate sign-up and sign-in.
- **A real browser.** Playwright drives the built web app served by the server: the owner flow on a desktop viewport, the viewer flow on a phone viewport with the drawer, reload keeping the session, sign-out, and sign-up through GoTrue.
- **The TUI.** Driven in a real terminal against the live server, after the CLI's `login`.

## Running it

```sh
docker compose -f deploy/dev/compose.yaml up -d          # Postgres + Supabase Auth (GoTrue)
DATABASE_URL=postgres://postgres:postgres@localhost:54322/taskr \
SUPABASE_JWT_SECRET=super-secret-jwt-token-with-at-least-32-characters-long \
cargo run -p taskr-server
cd apps/web && VITE_TASKR_API=/api VITE_TASKR_AUTH_URL=/auth npm run dev
```

For a throwaway server with no database and no auth service: `TASKR_STORAGE=memory TASKR_DEV_AUTH=1 cargo run -p taskr-server`, then sign in with any email.

::: exercise
Add "rename a workspace". Owners only. Which layer gets the rule that names are 1 to 60 characters (it's already there), which gets the permission, and what changes in `taskr-api`? Write the presentation test first: after a rename, the sidebar shows the new name in every workspace list.
:::
