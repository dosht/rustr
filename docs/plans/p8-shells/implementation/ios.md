# P8-IOS: SwiftUI shell (p1V)

Source: `transgate/apps/ios/` (README there has the layout, build steps and limits), `transgate/scripts/check-swift.sh` (the D14 Linux gate script `gate-transgate.sh` already requires), `scripts/build-transgate-ios.sh` (Mac half).

## What exists

- **Non-UI half (Linux and macOS):** `Transgate/Core/*` over the generated bindings. `AppModel` owns the `TransgatePresenter`, forwards intents in order and restarts the presenter from a custom-scheme auth return (core accepts `Started` once, and consumes `?code=` from it). `TransgateNativePorts` forwards every `NativePorts` method to small protocols (`SecretStore`, `PlayerControl`, `OutputSink`, `ExternalLauncher`, `AssetSource`) and `HeldFiles` (hold, measure, one PUT with progress, release). `Copy`/`Labels` are the only text path (`translate()`).
- **Apple half (Xcode):** `Platform/Adapters.swift` (Keychain `ThisDeviceOnly`, `ASWebAuthenticationSession`, AVFoundation, UIKit share/clipboard, bundle assets) and `UI/*` (auth, new transcript, history, transcript with playback and editing, AI consent/analysis/chat, settings, feedback, support, signed-in frame). Balance as text only; billing history and API keys are browser handoffs (D6).
- **Gate:** `transgate/scripts/check-swift.sh` builds `transgate-ffi`, compiles the generated Swift, `Core/*`, `smoke/UiContract.swift` and `smoke/Smoke.swift` in the pinned `swift:6.2-noble` image, and runs them with the real core against a local fake host and PUT receiver.

## Evidence (executed by p1V)

`transgate/scripts/check-swift.sh` exit 0, printing `P8_IOS_PASS translate-16-locales-strict label-registry-16-locales auth-return-and-picker-accept ui-contract-intents-and-fields-compile vault-round-trip-and-permissions put-200-only-with-progress held-files-measure-release native-ports-forwarding restore-signed-out-and-auth-intents custom-scheme-return-restarts-core locale-from-start-url stored-session-restores-and-lists-jobs pick-measure-remove-through-core settings-round-trip vault-restore-and-sign-out`. `swiftc -parse` accepts every `UI/` and `Platform/` file in the same image; `bash -n` accepts both scripts.

Two Linux-run adaptations: the library built on this machine needs glibc 2.43 and the image has 2.39, so the script runs the image's binary under the host's libc (toolchain and runtime stay the image's; a library built in an Ubuntu 24.04 container would not need that). Failures found and fixed while running: `UploadDelegate` must use delegate callbacks (not a completion handler) for progress to fire on Linux; core releases held handles on dispose, not on row removal.

## Not proven

SwiftUI and the Apple adaptors have not been compiled, run or reviewed visually (no Xcode). Waydroid is Android's. No real GoTrue, Google or provider run; `ASWebAuthenticationSession` and the Keychain are untested outside the fakes. No simulator, device or TestFlight build.

## For the lead

1. `TranscriptVm.status_badge` has no tone; the web view maps a color from the status (`TranscriptHeader.svelte:17`). That is a shell rule; core should emit the tone (iOS draws it neutral until then).
2. `/billing` and `/dashboard/api-keys` are constants in the shell (`WebHandoff`). If core should own those paths, expose them.
3. Gaps listed in the README limits (per-type analysis cards, active-line highlight, chat limits, special offer).
4. `gate-transgate.sh` line "Linux Swift bindings and views compile" can now call the script; Y owns D14 execution (set `SWIFT_IMAGE`/`TRANSGATE_FFI_LIB_DIR` for an immutable library if desired).
