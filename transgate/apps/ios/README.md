# Transgate iOS (P8-IOS)

A SwiftUI shell over the generated `TransgateCore.swift` (UniFFI, `transgate-ffi`). Core and
Shells: Rust decides everything; this app renders `AppVm`, forwards `AppIntent`s and executes
the few device effects core asks for through `NativePorts`. Decisions: `docs/plans/p8-shells/decisions.md`.

## Layout

| Path | What | Builds on |
|---|---|---|
| `Generated/` | UniFFI output (`scripts/gen-bindings.sh`); never edit | Linux and macOS |
| `Transgate/Core/` | Everything that does not draw: `AppModel` (presenter lane, restart on auth return), `TransgateNativePorts`, `HeldFiles` (picked files, measure, PUT), `SecretStore` (Keychain), `Support` (settings, `Copy` over `translate()`, auth return), `Labels` (view-owned catalog keys) | **Linux Swift and macOS** |
| `Transgate/Platform/` | Apple adaptors: `ASWebAuthenticationSession`, AVFoundation, UIKit share/clipboard, bundle assets | macOS (Xcode) |
| `Transgate/UI/` | SwiftUI screens | macOS (Xcode) |
| `Transgate/DesignTokens.swift` | Generated from the root `design-tokens.json` (`scripts/gen-tokens.py`); semantic tones only | macOS (Xcode) |
| `smoke/` | Linux/macOS smoke (`Smoke.swift`) and the UI contract compile check (`UiContract.swift`) | Linux and macOS |
| `project.yml` | XcodeGen spec; no `.xcodeproj` is committed | macOS |

## What the app does (D5, D6)

Signed-in phone flows, all from core's view models: sign in, sign up, magic link, recovery and
sign out (WEB-052); upload with language, translation and speaker choices, plus YouTube sources
(WEB-056/060); history, transcript lines with speakers and times, editing and audio playback
(WEB-064); export through the share sheet (WEB-068); AI consent, analysis and chat (WEB-070);
settings, retention, consent and account deletion (WEB-074); feedback and support (WEB-076).

**Balance only, no purchases (D6).** The account sheet shows the balance as text. The app never
sends `purchase_opened`; billing history and API key administration are rows that open the web
app in the system browser: the address is core's `web_url(TransgateWebUrl, route, locale)` (the https web origin, never the auth-callback scheme). If core ever routes to billing or API keys inside
the app, a screen offers the same browser handoff.

**YouTube (D11).** No SDK: tapping the video or a line opens the video at that time in the
YouTube app or the browser.

## Where each rule lives

- Copy: only `translate()` (via `Copy` and `Labels`). `Labels.all` is translated in all sixteen locales by the smoke test.
- Session: core stores it through `NativePorts.storage_*`; the Keychain item is `ThisDeviceOnly`, readable after first unlock (D8).
- Auth (D7): core starts GoTrue PKCE and asks the shell to `leave(url)`. A URL under `TransgateAuthUrl` opens an `ASWebAuthenticationSession` whose callback scheme comes from `TransgateSiteUrl`; its return URL (`transgate://app/<locale>/home?code=...`) restarts the presenter from that location, as a page load does on the web. Magic links, signup confirmation and recovery arriving through `onOpenURL` take the same path.
- Native telemetry is off (D14): there is no analytics code.

## Linux check (D14; runs in the pinned `swift:6.2-noble` image)

```sh
transgate/scripts/check-swift.sh   # prints P8_IOS_PASS <steps>
```

Compiles `Generated/TransgateCore.swift`, `Transgate/Core/*.swift`, the UI contract and the smoke
against the host build of `transgate-ffi` and runs it with the real core: translation in 16
locales, the label registry, the PUT upload (200 only, monotonic progress, refusal and unreachable
host), the vault, the ports, a locale-carrying start URL, a stored session restoring against a
local fake host, a picked file measured through the port, settings, sign-out clearing the vault.
`UiContract.swift` names every intent and view-model field the SwiftUI layer uses, so core drift
that would break the UI is a Linux compile error. SwiftUI itself is not compiled on Linux.

Optional real-host run: set `TRANSGATE_SMOKE_API`, `_AUTH`, `_KEY`, `_EMAIL`, `_PASSWORD`.
If the host libc is newer than the image's, the script runs the Swift binary with the host libc
(the toolchain stays the image's).

## Mac build

```sh
brew install xcodegen
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
scripts/build-transgate-ios.sh      # bindings, static libs, TransgateCore.xcframework, Transgate.xcodeproj
open transgate/apps/ios/Transgate.xcodeproj
```

In the target's build settings set `TRANSGATE_API_URL` (`https://<host>/api`), `TRANSGATE_AUTH_URL`
(`https://<host>/auth/v1`), `TRANSGATE_AUTH_KEY` (the GoTrue anon key), `TRANSGATE_WEB_URL`
(`https://<host>`) and, to change it, `TRANSGATE_SITE_URL` (default `transgate://app`). Add
`transgate://app/**` to GoTrue's allowed redirect URLs, or sign-in by Google, magic link and
recovery cannot return to the app. A missing address shows a configuration error; the app never
guesses a host. The simulator reaches a Mac host at `localhost`.

## Limits

- SwiftUI and the Apple adaptors are syntax-checked on Linux (`swiftc -parse`) but have not been compiled, run or visually reviewed; that needs Xcode and a device or simulator.
- Drawn from the existing view models (not yet seen on a device): the five per-type analysis sections with their copy buttons (`AnalysisSections.swift`), the playing-line highlight (the player's clock via `AVPlayerControl` snapshots and `Formatting.activeLine`, the web rule), chat usage limits, and the special offer (message and code, no purchase link; D6).
