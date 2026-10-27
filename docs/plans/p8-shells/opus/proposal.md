# P8 mobile, CLI and TUI shells: Opus proposal

2026-10-27, medium effort. Independent design under `docs/plans/p8-shells/brief.md`. I did not read the Fable proposal. Reads only. No implementation, builds, emulators or provider calls. Claims I did not observe are marked [INFERENCE].

## 0. Recommendation

1. **Move the effect driver out of wasm.** The composed session presenter already exists in core: `presentation::app::{AppState, AppIntent, AppEffect, AppEvent, AppVm}` (`core/src/presentation/app.rs:116-504`). Only its effect driver is browser-bound: `transgate/wasm/src/driver.rs` (1,155 lines, `Rc`, `js_sys::Date`, JS platform calls). P8's main change is to move the platform-neutral half of that driver into `transgate-client::driver`, generic over a `ShellPorts` trait. wasm, the new `transgate-ffi`, `transgate-tui` and `transgate-cli` then all execute the same effects. That gives one execution path, not four.
2. **Three new Rust crates and two native apps:**

   | Path | Kind |
   |---|---|
   | `transgate/ffi` (`transgate-ffi`, UniFFI) | crate |
   | `transgate/tui` (`transgate-tui`, ratatui) | crate |
   | `transgate/cli` (`transgate-cli`, clap) | crate |
   | `transgate/apps/android` (Compose) | app |
   | `transgate/apps/ios` (SwiftUI, XcodeGen) | app |

   They copy the taskr pattern: `crates/taskr-ffi` exports callback interfaces such as `SessionVault` and `AppViewListener` (`taskr-ffi/src/app.rs:26-38`). taskr's smoke scripts, the Linux Swift compile and the Waydroid flow carry over too.
3. **Shell scope:**
   - **Mobile:** every signed-in flow, WEB-051 to 076.
   - **CLI/TUI:** the PRD's smaller set: sign-in, upload and configuration, YouTube source, list/status/fetch/delete, and export (WEB-049/050, 053/054, 057/058, 061/062, 065/066).
   - **Browser handoff:** purchase, invoices, API keys and admin go to an authenticated browser, not native screens (screens.md:94; WEB-071/072, 075/076).
4. **Authentication:**
   - **Native:** the existing Rust GoTrue PKCE client (`client/src/auth.rs`). The system browser handles Google, magic link and recovery: Android Custom Tabs, iOS `ASWebAuthenticationSession`. The redirect is a custom scheme; PKCE protects the code. Password sign-in and sign-up stay native forms.
   - **CLI/TUI:** password sign-in, plus Google via an RFC 8252 loopback redirect.
5. **Bulk bytes never cross FFI.** Measure, PUT, play, save and share are shell ports. Core decides order and rules, as P5 already does for the web.

## 1. Row families and mapping

Every row maps to existing core presenters. The shell supplies only the ports.

| Rows | Flow | Surfaces | Core presenter (existing) | Shell ports |
|---|---|---|---|---|
| WEB-049/050 | sign in, sign out | CLI, TUI | `AuthEffect::{PasswordSignIn, GoogleRedirect, ExchangeCallback, SignOut}` (`app.rs:116-129`) | session storage, loopback listener, open URL |
| WEB-051/052 | sign in, signup, recovery, sign out | Android, iOS | same plus `SignUp`, `MagicLink`, `Recover`, `UpdatePassword` | session vault, auth browser session, deep link |
| WEB-053 to 056 | upload and configuration (file, language, target, speakers, progress, errors) | all four | `upload_queue` (ordering is P5-proven: measure, create, options, sign, PUT, Start) | file pick, `measure(handle)`, `put(handle, url, progress)`, `forget` |
| WEB-057/058 | YouTube add and resolve, no player | CLI, TUI | `upload_queue` YouTube path | none |
| WEB-059/060 | YouTube plus playback handoff | Android, iOS | same plus the transcript `player` intents | `open_external(youtube url)` |
| WEB-061/062 | list, status, fetch transcript/translation, delete | CLI, TUI | `job_list`, `transcript` | stdout and terminal |
| WEB-063/064 | list and detail, edits and speakers, freemium and consent states, playback | Android, iOS | `job_list`, `transcript` | player (`play`, `pause`, `seek`, `is_playing`, matching the wasm `PlayerPort`, `platform.rs:38-44`) |
| WEB-065 to 068 | export with explicit options | all four | transcript export intent; bytes from `transgate-export` in Rust | `save(bytes, name, mime)`, `share(...)` |
| WEB-069/070 | AI consent, analysis, highlights, streamed chat | Android, iOS | `presentation::ai::{consent, analysis, chat}`; native SSE through `llm-http`/`transgate-client::ai` | copy; document save |
| WEB-071/072 | balance, purchase and invoice browser handoff | Android, iOS | `LoadBalance`/`BalanceView`; purchase becomes a handoff | `open_external(app billing URL)` |
| WEB-073/074 | settings, retention, consent, account deletion | Android, iOS | `data_management`, settings, erasure receipt | confirm dialogs (UI only) |
| WEB-075/076 | feedback, support, API admin handoff | Android, iOS | `feedback`, `support`; API keys handed off | `open_external(/dashboard/api-keys)` |

**Not in P8 shells:** waveform, pointer gestures, browser capture, SEO, gateway forms and promotional UI (screens.md:94). No terminal payment, and no new in-app subscription (screens.md:183).

## 2. Architecture

### 2.1 Shared driver (p22, the largest seam)

- **Move:** `transgate-client::driver::Driver<P: ShellPorts>` holds what is now in `wasm/src/driver.rs`:
  - the epoch and token fences: a late effect never borrows a newer user's session, and clipboard, files, share and checkout run only if `is_current(epoch)`
  - 401 maps to `Unauthorized`, 410 to `AccountErased`
  - watch, poll and timer lifecycles, and dispose
- **Ports:** `ShellPorts` uses the `MaybeSend`/`MaybeSync` split that `transgate-client` already has (`client/src/future.rs`). Its methods:
  - `spawn`
  - `now`
  - `sleep`
  - `storage`
  - `random`
  - `session_lock`
  - `measure`, `put` and `forget` for media
  - `player`
  - `copy`
  - `save`
  - `share`
  - `open_external`
  - `navigate`
  - `telemetry`
- **Stays in `transgate-wasm`:** the JS `Platform` adapter and the `wasm-bindgen` glue. Web behavior must be identical. The existing web Vitest and Playwright suites are the regression gate.
- **Why not a native copy of the driver:** a copy would duplicate the fences, and those are the hardest-won P5 correctness.

### 2.2 `transgate-ffi` (p22)

- **Presenter object:** one `uniffi::Object` `TransgatePresenter`:
  - `new(config, ports: Box<dyn NativePorts>, listener: Box<dyn ViewListener>)`
  - `dispatch(intent)`
  - `current()`
  - `dispose()`

  It owns a tokio multi-thread runtime, as taskr does. Callback interfaces are `NativePorts`, the native subset of `ShellPorts`, and `ViewListener`.
- **View-model types:** core must not depend on uniffi, the same rule taskr follows. Use **UniFFI remote types** (`#[uniffi::remote(Record/Enum)]` over the core VM types), which re-declare the shapes in the ffi crate. The compiler rejects drift. [INFERENCE: available in uniffi 0.32; p22 confirms.] Export only the VMs the mobile rows render.
  - **Rejected, JSON over FFI:** Kotlin and Swift would hand-write the decoders, a second contract.
- **Messages:** VMs carry message keys and typed arguments (P5 D10). Add `translate(locale, key, args) -> String` in Rust over the **same** `transgate/web/locales` catalogs, embedded at build. TUI and CLI use it too. Interpolation and plural semantics are tested against the i18next fixtures. Without this, Android and iOS would each need their own i18next semantics.
- **Bindings:** generated by `scripts/gen-bindings.sh` (extended with the transgate library), `--check` in the gate.
- **Android HTTPS:** rustls with the platform verifier needs JNI initialization on Android (`docs/milestones/m1-multi-user.md:116`). The ffi crate needs an Android init entry point, or use webpki roots (DP-5).

### 2.3 Platform ports per shell

| Port | Android | iOS | TUI/CLI |
|---|---|---|---|
| Session vault (`AuthStorage`) | Keystore-wrapped key plus encrypted DataStore | Keychain (`kSecClassGenericPassword`, this-device-only) | XDG file, mode 0600, as taskr does (`taskr-infra::session_file`); OS keyring is DP-3 |
| Session lock (cross-process refresh) | in-process mutex (one process) | in-process mutex | file lock (TUI and CLI may run together) |
| Auth browser | Custom Tabs plus intent filter `transgate://auth/callback` | `ASWebAuthenticationSession`, callback scheme `transgate` | loopback `127.0.0.1:<random>/callback`, opened via `open`/`xdg-open` |
| File pick, measure | Storage Access Framework URI; `MediaMetadataRetriever` duration | `UIDocumentPicker` security-scoped URL; `AVURLAsset` duration | path argument; `ffprobe`, or the duration-unknown path (DP-4) |
| PUT with progress | OkHttp streaming from `ContentResolver`, progress callback | `URLSession` upload task | reqwest streaming from file (Rust, no FFI) |
| Player | Media3 ExoPlayer | AVPlayer | none |
| Save and share | SAF create document / share sheet | `UIActivityViewController` / file exporter | write file to `--out`, or stdout for text |
| Open external | Intent `ACTION_VIEW` | `UIApplication.open` | print URL plus `xdg-open` |
| Fonts for PDF export (D18) | bundled app assets via a font port | bundled | read from install dir |
| Telemetry | none (DP-6) | none | none |

Presigned uploads: core produces the signed URL. The shell streams the file to it, and the core never holds media bytes (PRD scenario 11).

### 2.4 TUI and CLI

- **`transgate-tui`:** ratatui over `Driver` with native ports. It renders a subset of `AppVm`:
  - sign-in
  - queue (add a file by path, options, start, progress)
  - job list and detail
  - transcript reader
  - export
  - Keymap and help come from the taskr pattern (`taskr-tui/src/{keymap,view}.rs`). Render tests use `TestBackend` (`taskr-tui/src/view.rs:271-320`).
- **`transgate-cli`:** clap subcommands. Each one **drives the same presenter headlessly**: dispatch intents, run effects until the presenter is idle, print the result. Upload ordering, freemium bounds and errors are never reimplemented.
  - `login [--google]`, `logout`, `whoami`
  - `upload <file> [--language] [--translate-to] [--speakers]`
  - `youtube <url>`
  - `jobs [--watch]`, `show <id>`, `delete <id>`
  - `transcript <id> [--translation <lang>]`
  - `export <id> --format <txt|srt|vtt|docx|pdf|md> [--out]`
  - Exit codes: 0 success, 1 domain refusal, 2 usage, 3 auth, 4 network. Messages go to stderr.

### 2.5 Mobile apps

- **Android** (`transgate/apps/android`): Compose screens per row family. A ViewModel only holds the presenter and maps `ViewListener` updates to state. `DesignTokens.kt` is generated.
- **iOS** (`transgate/apps/ios`): SwiftUI, an `@Observable` model over the presenter, generated `DesignTokens.swift`, and XcodeGen `project.yml` copied from taskr.
- **Layout:** phone layouts follow the P5 phone views (source mobile order, D9). Tablet uses the phone layout; there is no new tablet design.
- **Business logic:** no business `if` in Kotlin or Swift. Visibility, enablement and tones come from VM fields.

### 2.6 Design tokens

- Extend `scripts/gen-tokens.py`, which already emits taskr Kotlin, Swift and TUI outputs (`gen-tokens.py:180-188`), with three Transgate outputs: `transgate/apps/android/.../DesignTokens.kt`, `transgate/apps/ios/Transgate/DesignTokens.swift` and `transgate/tui/src/design_tokens.rs`.
- Same root `design-tokens.json`, Paper Moss default, and the contrast checks already in the script.
- VMs speak semantic tones only, including the P5 success and progress roles.

## 3. Seams with existing code

| Seam | Owner | Change |
|---|---|---|
| `wasm/src/driver.rs` → `transgate-client::driver` | p22 | Move, generic over ports, with no behavior change for web |
| `AppIntent::Started{url, viewport, utc_offset}` (`app.rs:397-402`) | p1X | Native passes an app path (`/home`) and `Viewport::Phone`. Add a `Platform::{Web, Native, Terminal}` field only if a presenter branches on it. Today none must [INFERENCE]; p1X checks |
| `AppEffect::Leave{url}`, `Navigate` | p1X/p22 | Native maps `Navigate` to its nav stack (Route is core) and `Leave` to `open_external` |
| Purchase effects on native | p1X | Native purchase intents resolve to a handoff VM state (open the web billing page). No `checkout_form` on native (P5 D17 is browser-only) |
| Auth redirect URLs | p1X/p22 | `redirect` strings in `AuthEffect` come from config per shell. GoTrue `URI_ALLOW_LIST` must include `transgate://auth/callback` and `http://127.0.0.1` (deploy config, P9 and the local compose) |
| `translate()` over the locale catalogs | p1X (or the locale owner) | A pure Rust message formatter shared by the ffi, TUI and CLI |
| Host routes | p1T | None missing [INFERENCE from route list `http/src/lib.rs:70-92` and P5/P7 routes]. Universal or app links would need `/.well-known` files; avoided by the custom scheme |

## 4. Ownership and order

| Order | Owner | Slice |
|---|---|---|
| 1 | p22 | Driver extraction plus web regression; `transgate-ffi` with ports, remote VM types, `translate` export; bindings generation |
| 1 | p1X | `Started` and native intents check, purchase handoff state, redirect config, the `translate` formatter |
| 2 | TUI/CLI owner (lead assigns; p22 or a P5 view owner) | `transgate-tui`, `transgate-cli` with render tests and process tests |
| 2 | Android view owner | Compose app, Kotlin ports |
| 2 | iOS view owner | SwiftUI app, Swift ports, Linux-compilable models and listener |
| 2 | token owner (p1V) | gen-tokens outputs |
| 3 | Y | Acceptance |

Root Cargo and lock changes go through the integration owner. No mid-flight builds.

## 5. Decisions

| ID | Question | Recommendation | Observable |
|---|---|---|---|
| DP-1 | iOS purchase link (store rules) | Android: browser handoff to web billing. iOS: balance only, no purchase link, until mu checks App Store rules | iOS users buy on the web |
| DP-2 | Native Google sign-in | System browser PKCE, no native Google SDK | none |
| DP-3 | CLI/TUI session storage | 0600 file (taskr parity); OS keyring later | none |
| DP-4 | CLI duration measurement | Use `ffprobe` if present, else the source duration-unknown path (deferred charge) | billing via deferred path |
| DP-5 | Android HTTPS roots | `rustls-platform-verifier` with JNI init | none |
| DP-6 | Native telemetry | None initially (D12 optional); record it | no product analytics on native |
| DP-7 | PDF fonts on mobile | Bundle Latin, Cyrillic, Greek and Turkish. Fetch CJK and other heavy scripts lazily from the app origin and cache them (D18 spirit) | first PDF export in CJK needs network |
| DP-8 | Notifications | None (the source has none); job status by polling (D11) | none |

## 6. Acceptance (Y)

1. **Bindings:** `gen-bindings.sh --check` (Kotlin and Swift) and `gen-tokens.py --check`. Web suites stay green after the driver move, both projects.
2. **Linux Swift:** `smoke-swift.sh` extended. It compiles the generated `TransgateCore.swift`, the iOS models and the listener with `swiftc` on Linux, links `libtransgate_ffi`, and runs a signed-in smoke against the owned runtime (server, PG, GoTrue): sign in, list jobs, read a transcript, sign out. SwiftUI files are not compiled; the Mac steps are documented in `scripts/build-ios.sh`.
3. **Kotlin JVM smoke:** like `apps/android/smoke/BindingsSmoke.kt`, against the same runtime.
4. **Waydroid APK:** `build-android.sh` (`x86_64`, build-std). Install and drive with UiAutomator:
   - password sign-in
   - upload a fixture file pushed with `adb` and picked via SAF
   - start
   - see the job complete through the loopback provider fixture
   - open the transcript, play and seek
   - export TXT and share
   - sign out

   Screenshots recorded. A manual run is accepted only where SAF automation fails, and is recorded as such.
5. **CLI process tests:** the built binary against the owned runtime:
   - `login`
   - `upload` (bytes reach storage; Start is charged per P3 rules)
   - `jobs` until completed
   - `transcript`
   - `export --format srt|docx` (bytes read by an independent parser)
   - `delete`
   - `logout`
   - Bad credentials exit 3; a refusal exits 1.
6. **TUI render tests:** `TestBackend` snapshots of sign-in, queue, list, detail and export, from Rust fixtures in both themes. Keymap and help tests. One process test drives the TUI through a pty.
7. **Auth:** native PKCE callback exchange with a wrong or expired code (P5 fences); loopback login on the CLI.

**Open beyond P8:** a real Mac/Xcode build, real devices, store review, and production HTTPS and GoTrue redirect configuration (P9).
