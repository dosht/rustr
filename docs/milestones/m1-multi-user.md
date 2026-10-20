# M1: Multi-user with an outbox

Status: built, waiting for review. Nothing is committed.

## What M1 adds

taskr can now run as a multi-user app. People sign in, get a workspace, invite others with a role, and every change writes a domain event to an outbox in the same transaction. The core still has no dependencies and no I/O, and the signed-out shells (CLI, TUI, mobile, local web) work as before.

| Piece | Where |
|---|---|
| Async ports (`BoxFuture`, std-only `block_on`) | `taskr-core/src/shared/future.rs` |
| Principal, roles, the `can()` policy, the `Identity` port | `taskr-core/src/shared/access.rs` |
| Revisions (optimistic concurrency), `SaveError::Conflict` | `taskr-core/src/shared/storage.rs` |
| Domain events for the outbox | `taskr-core/src/shared/events.rs` |
| Workspaces context: aggregate, use cases, members screen | `taskr-core/src/workspaces/` |
| Session context: auth ports, `AppState` (sign-in, navigation, expiry) | `taskr-core/src/session/` |
| `InProcessApi`: the API ports run in process for one principal | `taskr-core/src/api.rs` |
| HTTP contract: paths, bodies, error codes | `crates/taskr-api` (new) |
| Postgres adapters + outbox | `crates/taskr-postgres` (new) |
| HTTP client + Supabase Auth client + dev auth | `crates/taskr-client` (new, native and wasm) |
| Axum server shell | `crates/taskr-server` (new) |
| CLI remote mode: `login`, `logout`, `whoami`, `members`, `-w` | `crates/taskr-cli/src/remote.rs` |
| Web sign-in, sidebar frame, members screen, read-only list | `apps/web/src/lib/app`, `apps/web/src/lib/members` |
| Local Postgres + GoTrue stack | `deploy/dev/compose.yaml` |

## Decisions to review

1. **Ports return `BoxFuture`, not `async fn` in traits.** That keeps them object safe (`Arc<dyn Port>`) with no `async-trait` crate. On wasm32 the future is not `Send`, because browser futures never are.

2. **Two API ports for clients: `TasksApi` and `WorkspacesApi`.** The presentation layer talks to these, never to repositories. `LocalTasks` runs them in process for the signed-out shells, `InProcessApi` runs them on the server for one principal, and `HttpApi` runs them over HTTP. So the same `TaskListController` drives a local file and a remote server.

3. **The server is a thin HTTP mapping of `InProcessApi`.** Each request verifies the token, builds an `InProcessApi` with request-scoped telemetry, and calls the trait method. There is no business code in the server crate.

4. **RBAC is a pure `can(role, action)` in the core.** Owner: everything. Editor: view and change tasks, view members. Viewer: view only. Use cases enforce it, and the state machines read it to hide what a role cannot do (`can_change`, `can_manage`). The e2e test calls the server directly as a viewer to prove the server refuses, not just the UI.

5. **Membership is keyed by email.** You can invite someone before they have an account; they see the workspace on first sign-in. The cost: if a person changes their email in Supabase, they lose access until re-invited. The alternative is user-id membership plus pending invites (more tables, more flows).

6. **Everyone owns a workspace.** On sign-in, a person who owns no workspace gets "<name>'s workspace", even if they were invited somewhere first. (The first version skipped it for invited people, who then could not keep tasks of their own; found in review and fixed.) It is named after the owner because invited members see the same name.

7. **Unknown and foreign workspaces both answer `not_a_member` (404).** Ids leak nothing.

8. **Optimistic concurrency everywhere.** Every aggregate load returns a `Revision`; saves with a stale one fail with `Conflict`, and the use case reloads and retries up to 3 times. The JSON file and localStorage adapters check it too, which narrows the old CLI/TUI last-writer-wins race (it does not close it, there is no file lock).

9. **Outbox in the same transaction.** Postgres writes the aggregate rows and the `outbox` rows in one transaction. The file and localStorage adapters drop events (single device, no consumer). Nothing reads the outbox yet; M4 adds the relay.

10. **Supabase is only an adapter.** The server verifies Supabase JWTs (HS256 secret, or JWKS for projects on asymmetric keys). The client signs in through GoTrue's REST API. Storage is plain `sqlx` against Postgres, so Supabase, Neon or a Docker Postgres all work. No RLS, no PostgREST.

11. **Dev auth is opt-in on both sides.** `TASKR_DEV_AUTH=1` on the server accepts `dev:<email>` tokens; `taskr login --dev` and a web build without `VITE_TASKR_AUTH_URL` produce them. Without the flag the server refuses them (checked).

12. **Navigation is state in Rust, looks are per shell.** `AppState` owns "which workspace, which screen" and composes the task list and members state machines (the parent/child pattern from Transgate's ADR-020). Results are tagged with their workspace and their sign-in (`Epoch`), so a slow answer for a workspace you already left, or for an earlier session, is dropped. The web shows it as a sidebar on wide screens and a drawer on phones.

13. **Errors cross the wire as codes.** `taskr-api` maps every core error to a status and code and back, with a round-trip test per variant, so `describe()` reads the same in the CLI, the browser and over curl.

14. **Requests are traceable.** Every log line of a request carries `request_id`, `user` and `build` (the Transgate ADR-018 lesson). The response has an `x-request-id` header.

15. **Concurrency fixes from the second review** (each has a regression test):
    - Postgres first saves are a plain `insert`, later saves `update … where revision = $expected`; anything but one row is `Conflict`. The old `select … for update` + upsert let two first saves both commit, silently dropping one acknowledged task. Both repositories had it.
    - Membership changes re-check the caller's role in the loaded workspace on every attempt, so a removed or demoted owner's in-flight request cannot change members (it could re-add itself). Task writes keep the role resolved at request start: separate aggregates, a one-request window, accepted.
    - Screen intents carry the workspace the shell showed (`SignedInVm::workspace`); intents for a replaced screen are dropped. `AppController::dispatch_with` publishes the pending view before requests run, so the old screen does not stay interactive.
    - Every `/me`, task and member request carries the sign-in `Epoch`; answers from an earlier session are dropped, so a late `401` cannot sign out a newer session and a previous user's `/me` cannot show up in the next one.
    - The role follows the server: loading members updates it from your own row (navigation label, `can_manage`, `can_change`); a `403`/`404` reloads `/me`, and losing the open workspace opens the next one with a message.
    - The personal workspace id is stable per person (`personal_id`, UUIDv5 in Postgres), so racing first visits create one workspace.

## How it was verified

- 133 Rust tests (61 before). New: policy table, workspace aggregate, use cases with RBAC, conflicts and outbox, `AppState` navigation and expiry, the wire codec, GoTrue error shapes, JWT verification (valid, wrong secret, wrong audience, expired).
- The repository contracts run against the in-memory fakes, the JSON files, and Postgres (`TASKR_TEST_DATABASE_URL`).
- `taskr-server/tests/http.rs`: two people, real HTTP, the shared `AppController` and `HttpApi`, on memory and on Postgres. Covers onboarding, adding, duplicate rejection, inviting a viewer, server-side refusals, strangers, last-owner rule, and an invalid token signing the person out.
- Manual runs of the real binaries: the CLI against the server with dev auth, then against GoTrue in Docker (wrong password, sign-up, duplicate sign-up, sign-in, add, list).
- Browser runs with Playwright against the server serving the built web app: owner flow on desktop, viewer flow on a phone viewport (read-only list, drawer, no member controls), reload keeps the session, sign-out. Then sign-up and sign-in through real GoTrue.
- `svelte-check`, `vite build`, the Node wasm smoke test, and `cargo clippy --workspace --all-targets` are clean.

## How to run it

```sh
# Throwaway: in-memory storage, dev sign-in
TASKR_STORAGE=memory TASKR_DEV_AUTH=1 cargo run -p taskr-server
taskr login --server http://localhost:8787/api --email ada@example.com --dev
cd apps/web && VITE_TASKR_API=/api npm run dev      # sign in with any email

# Real: Postgres + Supabase Auth (GoTrue) in Docker
docker compose -f deploy/dev/compose.yaml up -d
DATABASE_URL=postgres://postgres:postgres@localhost:54322/taskr \
SUPABASE_JWT_SECRET=super-secret-jwt-token-with-at-least-32-characters-long \
cargo run -p taskr-server
TASKR_PASSWORD=secret123 taskr login --server http://localhost:8787/api \
  --auth-url http://localhost:9999 --email ada@example.com --sign-up
cd apps/web && VITE_TASKR_API=/api VITE_TASKR_AUTH_URL=/auth npm run dev

# Hosted Supabase: point DATABASE_URL at the project's Postgres, set
# SUPABASE_JWKS_URL=https://<project>.supabase.co/auth/v1/.well-known/jwks.json,
# and VITE_TASKR_AUTH_URL / --auth-url to https://<project>.supabase.co/auth/v1
# with the anon key in VITE_TASKR_AUTH_KEY / TASKR_SUPABASE_KEY.

# Postgres tests
TASKR_TEST_DATABASE_URL=postgres://postgres:postgres@localhost:54322/taskr cargo test --workspace
```

The server can also serve the built web app (`TASKR_WEB_DIR=apps/web/dist`), so one process is the whole deploy.

## Open questions for the review

1. Email-keyed membership (decision 5): keep, or move to user ids plus invites now?
2. Token refresh is not done. Supabase access tokens last an hour; after that the app signs the person out with "Your session has ended." Refresh belongs in `AuthClient` plus one retry in `HttpApi`. M1 or later?
3. Should creating extra workspaces (and renaming) be in M1? The aggregate supports a name; there is no use case or screen yet.
4. Deep links: the web URL does not change with the workspace or screen. Adding it means the shell maps `location` to `WorkspaceSelected` / `ScreenSelected` and back. Worth doing before M2 (SEO pages need real URLs anyway).
5. `TASKR_STORAGE=memory` ships the in-memory fakes in the server binary (the `testing` feature). Fine for a throwaway mode, or move them to a separate crate?
6. ~~The TUI and the mobile apps stay signed out.~~ Done after the first review: the TUI opens signed in after `taskr login` (sign-in prompt, `w` workspaces, `m`/`t` members and tasks), and Android and iOS have a signed-in `AppPresenter` with a platform `SessionVault` (Android preferences, iOS Keychain). Android is built and was run on Waydroid against the dev stack, signing in through GoTrue; iOS needs a Mac.

## Not done in M1

- The outbox relay (M4), realtime (M7), offline sync for the signed-in mode (M7).
- Token refresh, password reset, OAuth providers (GoTrue supports them; only email and password are wired).
- A file lock for the local JSON files.
- iOS was not built. Its signed-in screens are written (SwiftUI: split view, sign-in, members, Keychain vault) but not compiled here. Android is built (`scripts/setup-android-toolchain.sh`, then `build-android.sh` with `TASKR_ANDROID_BUILD_STD=1 TASKR_ANDROID_ABIS=x86_64` and Gradle with `-Ptaskr.apiUrl`) and runs on Waydroid.
- HTTPS from Android: reqwest's rustls uses the platform verifier, which needs JNI initialisation on Android. Plain http (dev, Waydroid) works; production HTTPS needs that init or webpki roots.
- The book is updated: new chapter 13 (signed in), chapters 2 and 4 to 12 brought in line, reference renumbered to 14.
