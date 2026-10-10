# taskr

A small task list that runs as a CLI, a TUI, a web app, an Android app and an iOS app, all driven by one Rust core. It is the reference implementation of the **Core and Shells** pattern described in [docs/architecture.md](docs/architecture.md).

- Domain: `tasks` (the `TaskList` aggregate: unique open titles, an open-task limit, complete and reopen rules)
- Cross-cutting infrastructure: `telemetry` (one port, five destinations: stderr, a log file, the browser console, logcat, os_log)

## Layout

```text
crates/
  taskr-core/     domain, use cases, ports, shared ViewModel state machine (no I/O, no deps)
  taskr-infra/    native adapters: JSON file storage, system clock, JSON-lines telemetry
  taskr-cli/      `taskr` binary (clap)
  taskr-tui/      `taskr-tui` binary (ratatui, vim keys, k9s-style hints)
  taskr-ffi/      UniFFI facade for Kotlin and Swift
  taskr-wasm/     wasm-bindgen facade for the browser, generates the TypeScript types
apps/
  web/            Svelte 5 + TypeScript + Vite, runs the core as wasm
  android/        Kotlin + Jetpack Compose
  ios/            Swift + SwiftUI, XcodeGen spec
scripts/          build and smoke-test scripts for each platform
docs/             architecture.md
```

## Run it

The CLI and TUI share data in `$TASKR_HOME` (default: the platform data dir, for example `~/.local/share/taskr`).

```sh
cargo test --workspace                      # 61 tests: domain, use cases, presentation, adapters, shells

cargo run -p taskr-cli -- add Buy milk
cargo run -p taskr-cli -- list --scope all
cargo run -p taskr-cli -- done 1
cargo run -p taskr-cli -- -v list --json    # -v prints telemetry to stderr

cargo run -p taskr-tui                      # ? shows every key
```

### Web

```sh
./scripts/build-web.sh                      # needs wasm32-unknown-unknown and wasm-bindgen-cli 0.2.128
cd apps/web && npm install
npm run smoke                               # the real wasm core under Node
npm run dev                                 # http://localhost:5173
npm run build                               # svelte-check + production build
```

### Android

```sh
./scripts/gen-bindings.sh                   # Kotlin bindings, works on Linux without the SDK
JNA_JAR=path/to/jna-5.18.1.jar ./scripts/smoke-kotlin.sh   # bindings on the JVM
./scripts/build-android.sh                  # needs the NDK and cargo-ndk
```

Then open `apps/android` in Android Studio. The Gradle `preBuild` step runs `build-android.sh`.

### iOS

On Linux (before Xcode):

```sh
./scripts/gen-bindings.sh                   # Swift bindings + C header + modulemap
./scripts/smoke-swift.sh                    # bindings + TasksModel with a Linux Swift toolchain
```

On a Mac:

```sh
./scripts/build-ios.sh                      # static libs, xcframework, xcodegen
open apps/ios/Taskr.xcodeproj
```

## What was verified where

| Shell | Verified on this Linux machine |
|---|---|
| CLI | unit and process-level tests, manual run |
| TUI | unit and render tests, driven in a real pty, shares data with the CLI |
| Web | wasm built, Node smoke test, svelte-check, production build, driven in headless Chromium |
| Android | Kotlin bindings compiled and run on the JVM against the core; Compose UI and Gradle not built (no SDK) |
| iOS | Swift bindings and `TasksModel` compiled and run with Swift 6.4 on Linux; SwiftUI and the Xcode project not built (needs macOS) |
