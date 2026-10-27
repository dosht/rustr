# P8 cross-critique of the Opus proposal (Fable)

`w23:p27`, anthropic/claude-fable-5-1, MEDIUM, 2026-10-27. Read `opus/proposal.md` (34BA) after mine was frozen. Four targeted fact checks cited inline. Neither proposal is edited.

Verdict: the proposals agree on every structural point (driver out of wasm into `transgate-client`, one UniFFI presenter, browser handoff for purchase/keys, system-browser PKCE, PRD-only CLI/TUI set, three token outputs). Opus is more complete in two places I missed outright (message translation on native, lazy fonts) and I adopt them. Opus has three weak spots.

## 1. Where Opus is wrong, unsupported or heavier than needed

| # | Claim | Evidence | Disposition |
|---|---|---|---|
| O1 | `driver.rs` is "1,155 lines" (§0.1) | `grep -c . transgate/wasm/src/driver.rs` = 1105 non-blank | Cosmetic; the point stands |
| O2 | DP-1 asymmetric purchase: Android browser handoff, iOS balance-only | Google Play's payments policy constrains steering to external purchase for digital goods as Apple's 3.1.1 does; the asymmetry is unsupported by cited evidence (none given). WEB-071/072 are parallel rows | Symmetric: balance shown, purchase text "buy on the web", **no link on either platform** until mu decides store posture. Simpler, and one decision instead of two. Record the WEB-071/072 "handoff" wording as pending that decision |
| O3 | DP-5 `rustls-platform-verifier` with JNI init on Android | `m1-multi-user.md:116` says the platform verifier needs JNI init, or webpki roots. Both client crates build reqwest 0.13 with only `rustls` (`transgate/client/Cargo.toml:26`); which root store that selects is **unverified** here | Prefer webpki roots (no JNI entry point, no per-platform init path). p22 checks the actual reqwest 0.13 feature; if `rustls` already means webpki roots, nothing to do |
| O4 | TUI pty process test (§6.6) | PRD P8 asks for "TUI render tests green"; the pty run adds a flaky layer for no row | Drop; `TestBackend` renders plus keymap tests against the real driver with a fake API |
| O5 | §2.2 "tokio multi-thread runtime, as taskr does" | Not verified by me; taskr-ffi `dispatch` is documented as blocking (`taskr-ffi/src/app.rs:6-7`) | p22 states the actual runtime shape when publishing; no design impact |

## 2. What Opus gets right that I missed, and adopt

| # | Point | Why |
|---|---|---|
| A1 | `translate(locale, key, args)` in Rust over the same `transgate/web/locales` catalogs (§2.2) | VMs carry `Msg` keys by P5 D10 (`core/src/presentation/msg.rs:1-7`); the web resolves them with strict i18next, no fallback (`web/src/lib/i18n/index.svelte.ts:1-6,49-51`). My proposal ignored translation on native; without a Rust formatter Kotlin, Swift and ratatui would each reimplement `{{var}}` and `_one/_other`. Placement: pure formatter in core `presentation::msg` (core already depends on serde), catalogs embedded with `include_str!` in `transgate-client::i18n`, tested against the same `locales/config.json` policy |
| A2 | UniFFI remote types over the core VMs (§2.2) | `uniffi 0.32.2` is in `Cargo.lock`; remote records/enums re-declare shapes and the compiler rejects drift, removing taskr's hand `to_view` layer (`taskr-ffi/src/app.rs:295-390`). Adopt where field types are plain; fall back to a manual mirror for any VM holding `serde_json::Value`. p22 confirms coverage |
| A3 | DP-7 fonts: bundle the base set, fetch heavy scripts lazily from the app origin | Reuses the existing `DocumentAssetSource::{base,extra}` seam (`client/src/export.rs:23-26`, P5 D18); bundling CJK is tens of MB. My "bundle everything" was heavier |
| A4 | `Started { url: "/home", viewport: Phone }` with no new intent (§3) | `url` is parsed into a `Route`; `/home` is real. Simpler than my `StartedNative` variant. p1X confirms no presenter branches on platform |
| A5 | Paths under `transgate/{ffi,tui,cli,apps/android,apps/ios}` | Matches the `transgate/` subdirectory rule; mine put apps under `apps/` |
| A6 | CLI exit codes 0/1/2/3/4 and `jobs --watch` | Concrete process-test contract |
| A7 | `ShellPorts` includes `spawn/now/sleep` | Needed to make the driver truly target-neutral (`js_sys::Date`, `Rc` today); I had only product ports |

Mine kept: the `Player` port maps `PausePlayer/ResumePlayer/Seek` with shell-local current time; log sinks (`LogcatSink`/`OSLogSink`) as diagnostics even with no product analytics; `--no-browser` CLI fallback that prints the URL.

## 3. Dispositions, one line each

| Decision | Disposition | Deciding evidence |
|---|---|---|
| Driver extraction shape and name | `transgate-client::driver::Driver<P: ShellPorts>` (Opus name), ports incl. `spawn/now/sleep`; wasm keeps only the JS adapter | `wasm/src/driver.rs:50-145` is the only browser-bound half; taskr precedent (controller in `taskr-client`) |
| Binding approach | UniFFI remote types; manual mirror only where a field type is unsupported | `Cargo.lock` uniffi 0.32.2; compile-time drift check; fewer lines than `to_view` |
| Locale translation | Rust formatter in core `msg`, catalogs embedded in `transgate-client`, shared by ffi/TUI/CLI | `msg.rs:1-7`, strict no-fallback policy `index.svelte.ts:49-51`; one semantics, not three |
| Rows per shell | Mobile WEB-051..076; CLI/TUI 049/050, 053/054, 057/058, 061/062, 065/066 (both agree) | PRD P8 row; `screens.md:146-173` |
| PKCE per platform | Android Custom Tabs + `transgate://auth/callback`; iOS `ASWebAuthenticationSession`; CLI/TUI RFC 8252 loopback with `--no-browser` fallback; password forms in-app; verifier/exchange in `client::auth` unchanged (both agree) | `auth.rs:3-6,29-32`; GoTrue allow-list is P9 config |
| Purchase / invoice / API-key handoff | Invoices and API keys: open authenticated web URL on both platforms; purchase: balance-only, no link, pending mu's store decision (O2) | WEB-071/072/075/076 wording; equal evidence → simpler symmetric rule |
| Secure storage | Android EncryptedSharedPreferences/DataStore with Keystore key; iOS Keychain this-device; CLI/TUI 0600 file + `flock` (both agree) | taskr `PrefsSessionVault.kt`, `KeychainSessionVault.swift`, `taskr-infra::session_file` |
| Android HTTPS | webpki roots unless p22 shows reqwest 0.13 `rustls` already does; no JNI init path | `m1-multi-user.md:116`; simpler |
| PDF fonts on native | Base bundled, extras lazy via `DocumentAssetSource::extra` (Opus) | `export.rs:23-26`, P5 D18 |
| Telemetry on native | No product analytics (D12 optional); OS log sinks only | taskr `LogcatSink`/`OSLogSink`; source has no native app |
| YouTube on mobile | Open external at seeked time; no player SDK (both agree) | WEB-059/060 "playback handoff" |
| CLI duration | `ffprobe` if present else duration-unknown path (both agree) | UPL-006 fallback already in core |
| Notifications | None (both agree) | source has none |
| TUI tests | `TestBackend` + keymap; no pty (O4) | PRD "render tests" |

Net: adopt Opus as base with O2, O3, O4 applied. No architectural disagreement remains.
