# P1: runnable foundation and honest gates

Status: complete on 2026-10-24. All three gates executed end to end. P1 permits product readiness gates to fail for unfinished later phases; those failures remain visible.

## What was built

- `transgate/server` is an actual Axum runtime in the existing Cargo workspace, with the shared lockfile. `GET /health` reports service, version, `alive` and `process-liveness`. Product paths remain 404. `TRANSGATE_BIND` defaults to `127.0.0.1:8788`; invalid config and occupied addresses fail explicitly. SIGINT/SIGTERM trigger graceful draining.
- `transgate/Dockerfile` uses a Rust 1.99.1 builder and Debian slim runtime. Its scoped context excludes env files, keys, build outputs and the legacy source repositories. Runtime UID/GID 65532, a real curl healthcheck and `deploy/dev/transgate.compose.yaml` provide local packaging. Compose restricts the published port to localhost, drops capabilities, uses a read-only filesystem and grants ten seconds to stop.
- `modules/README.md` records the billing/CMS dependency boundary without creating empty crates. Actual modules and their two consumers remain P2/P6 work. No additional workspace was introduced.
- `design-tokens.json` is the source for Paper Moss, Paper Indigo and Paper Ochre, each light/dark. The stdlib generator produces five outputs consumed by web, Android, Swift and TUI. It validates contrast and has a read-only drift check. Paper Moss remains provisional default; CLI behavior is unchanged.
- Storybook `Pages/App/Palettes` has 36 addressable examples: three palettes, two modes, two platforms and three existing taskr screens. Core-generated fixtures are reused unchanged. A single screen per canvas avoids conflicting global keyboard handlers and fixed phone bars.
- `gate-rustr.sh`, `gate-modules.sh` and `gate-transgate.sh` run every stage and propagate any failed or missing requirement. CI exposes all three gates with no failure suppression. `check-prd.py` rejects unverified rows and missing/ignored tests, checks generated ledgers against their sources, and traverses the resolved Cargo graph for transitive/renamed host dependencies and both consumers.
- `gen-bindings.sh --check` generates into temporary directories and compares checked-in Kotlin/Swift outputs without rewriting them. Cargo uses the lockfile in both generation modes. `docs/scenarios.md` registers all 17 required scenarios without turning historical implementation claims into unearned passing evidence.

## Verification exercised

| Surface | Observed result |
|---|---|
| Python checker regressions | 10 passed, including false-green prevention for zero/ignored Cargo tests, transitive renamed host dependencies, missing consumers, source parsing and discovery completeness |
| Rust workspace | `cargo test --locked --workspace`: 157 reported passed, zero failed; no `TASKR_TEST_DATABASE_URL` supplied, so this is not a fresh Postgres-backed validation |
| Clippy | Workspace and all targets passed with `-D warnings` |
| Workspace consumers | `cargo build --locked --workspace --all-targets` passed |
| Host server smoke | Real HTTP JSON/content type/version, unknown-route 404, invalid/occupied bind failures, SIGINT and SIGTERM all passed |
| Docker smoke | Image built; non-root/read-only container reached healthy; HTTP/404 checked; graceful stop exited zero; only the smoke container was removed |
| Native bindings | Generated Kotlin/Swift binding comparison passed |
| Tokens | Five generated outputs current; all declared text/border/focus contrast checks passed. Worker also exercised missing/stale output detection without modifying checked files |
| Svelte | `npm run check`: 566 files, zero errors/warnings |
| Storybook | All 36 stories rendered with expected palette/mode values, controls present, no horizontal surface overflow and no browser errors |
| Real web app | Wasm-backed local app created and persisted a task across reload, rendered desktop/phone, used system light/dark tokens and let explicit light override system dark |
| Android | x86_64 Rust core and Kotlin theme consumers compiled using the existing distro-Rust build-std path |
| TUI | Launched in a real herdr terminal with isolated TASKR_HOME, rendered, added a task and persisted it; quit normally |
| Book | Listings passed after updating the workspace snippet: 16 chapters, 10 diagrams |

Android command:

```sh
TASKR_ANDROID_BUILD_STD=1 TASKR_ANDROID_ABIS=x86_64 \
  mise exec java@temurin-21.0.12+8.0.LTS gradle@8.14.6 -- \
  gradle :app:compileDebugKotlin --offline --no-daemon
```

The first Android attempt omitted build-std and failed because Arch's compiler has no prebuilt Android standard library. The existing build-std switch resolved it; no toolchain or application workaround was added. This phase did not install or run a new APK on Waydroid.

## Gate outcomes and meaning

- Modules: exit 1 because billing and CMS are not implemented; workspace consumer compilation/linking passes.
- Transgate: exit 1 with six failed requirements: 914 unimplemented ledger/parity obligations, missing web component suite, missing web e2e suite, migration parity, Android signed-in verification and Swift compilation. Host and Docker runtime stages pass. The empty server unit suite is not counted as product proof; the process smoke supplies runtime evidence.
- Rustr: final integrated run exits 1 only for the 17 scenario rows without complete registered evidence. All other stages pass: ten Python regressions, 157 reported Rust tests, Clippy, book listings, native bindings and tokens. The earlier stale workspace listing was corrected; the complete rerun confirms that failure is gone.

P1 readiness is that these commands execute end to end and tell the truth, not that P2-P10 suddenly pass. No provider, payment, production migration or production mutation was exercised. Hosted GitHub Actions was not run from this local session; the workflow invokes the exercised gate commands.

## Visual evidence and accessibility

The retained walkthrough is `/home/mu/Videos/AI-recordings/rustr-p1-palettes/rustr-p1-palettes-final.webm`: 71.666 seconds, 1365 by 1000, full ffmpeg decode passed. Its contact sheet shows six palette/mode pairs, actual phone drawer open/close and the sign-in error fixture. Metadata and the helper are alongside it. No credentials or customer data appear.

A first visible-Wayland recording produced a static capture and could not deliver reliable click events. It is retained as limited exploration, not accepted evidence. The final pass used a separate headless AI Brave profile and left existing user tabs untouched.

Browser audits found and corrected landmark nesting/duplicate-banner problems introduced by the preview wrapper. Keyboard focus is visible and the real phone menu works. A pre-existing task-table empty header remains a minor best-practice finding; some phone contrast auditing requires manual review. Token contrast validation and visual inspection passed, but no universal accessibility conformance claim is made.

Swift dynamic colors are generated and wired to existing views. They are not compiled or run here: no Swift compiler is currently installed and UIKit/SwiftUI needs an Apple environment. That remains an explicit verification limit, not a green native gate.

## Decisions and next work

E3 is built and inspectable, awaiting mu's palette choice; the first set is used meanwhile as the PRD requires. No upload, billing, transcription or AI workflow was redesigned. Runtime liveness is intentionally distinct from product readiness, and the separate media processor obligation from P0 remains intact.

P2 is the next substantial feasibility checkpoint: billing rules implemented once, consumed by both taskr and Transgate, with real transaction/concurrency evidence and gateway fixtures. A working health route and a palette preview do not establish the cost or correctness of that port.

## Resumable ownership

The visible herdr workers reused their discovery sessions for P1. Closing a pane does not delete its session or findings:

- Runtime: `/home/mu/.omp/agent/sessions/-src-rustr/2026-10-24T06-49-07-212Z_01a16cab-978c-7029-893e-6f453cd66c24.jsonl`.
- Palettes: `/home/mu/.omp/agent/sessions/-src-rustr/2026-10-24T06-49-07-030Z_01a16cab-96d6-75d8-8e96-a203c9bb3427.jsonl`.

The lead owns integration, gates, milestone/book updates and the next phase plan. No hidden subagents were used for P0 completion or P1 implementation.
