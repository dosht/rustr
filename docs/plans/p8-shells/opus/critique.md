# P8 cross-critique: Opus on Fable

2026-10-27, medium effort. I read `fable/proposal.md` and verified the disputed points against source. Both proposals are unchanged. Where the evidence is equal, the simpler option wins (mu's instruction).

## 1. Where Fable is wrong or unsupported

| # | Fable claim | Evidence | Correction |
|---|---|---|---|
| W1 | Mobile checkout "opens the web `/checkout` in the system browser" (§1, D6) | `GET /checkout` answers `303 Location: /` (`modules/billing/http/src/routes.rs:128,407-409`). `POST /checkout` needs the form `access_token` plus the typed order (P5 D17; `routes.rs:411-414`). A bare browser open cannot reach checkout | The handoff opens the web app's billing page (`/{locale}/billing` or `/home`, where the purchase modal lives). The user signs in there; no token is transferred |
| W2 | "taskr already did this correctly: the controller lives in `taskr-client` and both wrap it" (§0) | `crates/taskr-client/src` holds only `audio_summary`, `auth` and `config`. The taskr app presenter is in `taskr-ffi/src/app.rs:217-262`, not in the client | Only the P4 audio-summary stream was lifted. The Transgate driver move is new work, not a copy of a taskr precedent. It is still the right move: both proposals agree |
| W3 | No plan for message text (§3: "`Locale.getDefault()` → one of 16") | VMs carry message keys and typed arguments (P5 D10). The catalogs live in `transgate/web/locales/{16}/*.json`, and only the web loads them, through i18next | Without a shared formatter, Kotlin, Swift and the terminal each reimplement i18next interpolation and plurals. One Rust `translate(locale, key, args)` over the same catalogs is the simpler option |
| W4 | Hand-written flat UniFFI mirrors with a `to_view`/`to_intent` layer, the taskr `app.rs:295-390` style (§2.2) | UniFFI 0.32.2 ships `#[uniffi::remote(...)]`, which "generates the same code that `#[derive(uniffi::<kind>)]` would" for types defined in another crate (`uniffi_macros-0.32.2/src/lib.rs:161-167`). The Transgate `AppVm` is far larger than taskr's | Remote types are less code and compile-checked against the core definitions. The mapping layer would be a second, hand-maintained contract |
| W5 | Android `AuthStorage` = `EncryptedSharedPreferences` (§3) | [INFERENCE] `androidx.security:security-crypto` is deprecated upstream. The taskr precedent is plain `getSharedPreferences(MODE_PRIVATE)` (`apps/android/.../PrefsSessionVault.kt:11`) | Wrap the token with an Android Keystore AES key and store it in private preferences. That is no extra dependency and not deprecated |
| W6 | iOS Keychain `kSecAttrAccessibleAfterFirstUnlock` (§3) | Without `ThisDeviceOnly`, refresh tokens can migrate through encrypted backups to another device | Use `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` |
| W7 | New `AppIntent::StartedNative` (D1) | `route::parse` already accepts a bare app path such as `/home` or `/de/home` (`core/src/presentation/route.rs:146-155`). `Viewport::Phone` is an existing value | A documented app path through the existing `Started` needs no core change. Prefer it |
| W8 | Android HTTPS not addressed | reqwest's rustls platform verifier needs JNI init on Android; "production HTTPS needs that init or webpki roots" (`docs/milestones/m1-multi-user.md:116`) | This is a required P8 item, or production sign-in fails on device |
| W9 | App paths `apps/transgate-android`, `apps/transgate-ios` (§2.4, §6) | PRD §1: `transgate/` is the full working copy, and `apps/` belongs to the taskr example | Use `transgate/apps/{android,ios}` |

## 2. Adopted from Fable

- **YouTube handoff at the clicked time** (D4). This is better than my plain open; there is no embedded player SDK.
- **CLI `--no-browser`:** print the auth URL and read a pasted code. This is the simplest fallback for headless machines.
- **A pinned Linux Swift container, `swift:6.2-noble`** (`sha256:c58ddae4…`, `scripts/p5-web/s1.sh:17`). Like the P4 harness, it prints a named pass marker for each step.
- **A line edit in the Waydroid flow, and comparing CLI export bytes with the server ZIP.** Both are stronger evidence than my list.
- **Bundling all D18 PDF fonts on mobile** (`DocumentAssetSource = bundle`). I drop my lazy CJK fetch: it adds network and cache code for a size saving mu did not ask for. Simpler wins.
- **Terminal and log sinks only, no product analytics on native.** Same as mine.
- **Explicit transcript effects** for player ports (`transcript.rs:99-112`) and the 900 s streaming URL for playback.

## 3. Disposition of each open decision

| Decision | Recommendation | Deciding evidence |
|---|---|---|
| Driver extraction shape and name | `transgate-client::driver::AppDriver` over one `trait Platform` (Fable's name), using the existing `MaybeSend`/`MaybeSync` split. wasm becomes a wrapper. Web Playwright on both projects is the regression gate | `wasm/src/driver.rs` holds the epoch and token fences P5 proved. A native copy would duplicate them. `auth.rs:38-56` bounds already allow both targets |
| Bindings approach | UniFFI remote types over the core VMs the mobile rows render. Core stays free of UniFFI | W4; `uniffi_macros-0.32.2/src/lib.rs:161-167` |
| Locale translation | One Rust `translate()` over the web catalogs, embedded at build. Tests compare it with i18next on the existing fixtures. Used by the ffi, TUI and CLI | W3; P5 D10 message keys |
| Native `Started` | Existing `Started` with a documented app path and `Viewport::Phone`; no new intent | W7; `route.rs:146-155` |
| Rows per shell | Mobile: WEB-051 to 076. CLI/TUI: 049/050, 053/054, 057/058, 061/062, 065/066, with no AI, billing or settings | `screens.md:94`; both proposals agree |
| PKCE flows | Mobile: in-app email and password forms; Google, magic link and recovery through Custom Tabs / `ASWebAuthenticationSession` with `transgate://auth/callback`; never a webview. CLI/TUI: password plus a loopback redirect, with `--no-browser` fallback. The GoTrue allow-list is P9 config | `AuthEffect` set at `app.rs:116-129`; both proposals agree |
| Purchase handoff | Open the web billing page (`/{locale}/billing`) in the system browser, never `/checkout`. iOS store policy goes to mu as a recorded risk; until decided, iOS shows balance only (DP-1) | W1; `routes.rs:407-409` |
| Invoice handoff | Browser handoff to the web billing page, as the row says (WEB-071/072 "invoice browser handoff"). No native invoice screen | `screens.md` WEB-071 text. Simpler than wiring the billing-owner invoice context to native |
| API-key handoff | Open `/{locale}/dashboard/api-keys` in the browser | WEB-075/076 "not new native admin scope" |
| Secure storage | Android: Keystore key wrapping a token in private preferences. iOS: Keychain, `AfterFirstUnlockThisDeviceOnly`. CLI/TUI: `$TRANSGATE_HOME` or the XDG session file at mode 0600 with `flock` (taskr parity) | W5, W6; `taskr-infra::session_file` |
| Android HTTPS | `rustls-platform-verifier` with the Android JNI init entry point in `transgate-ffi`. If its init proves fragile on Waydroid, the fallback is webpki roots | W8 |
| PDF fonts on native | Bundle the full D18 set in the app; lazily parse and cache faces in memory | Simpler; offline; D18 forbids runtime CDN fonts |
| Telemetry on native | None. A local log sink only; record the omission against D12 | D12 makes analytics optional; both proposals agree |
| YouTube on mobile | Open the YouTube app or browser at the clicked time; no SDK | Fable D4; no source native player |
| CLI duration | Use `ffprobe` if present, else `Measured{None}` so the existing server-side duration path applies | UPL-006 fallback; both proposals agree |
| CLI output | A human table only. Defer `--json` until a consumer needs it | PRD names no machine-output requirement; simpler |
| Notifications | None | Not in source; both proposals agree |

## 4. Net effect on my proposal

Superseded on these points, without editing it:

- Fable's `AppDriver` and `Platform` naming
- YouTube at the clicked time
- `--no-browser`
- the pinned Swift container and pass markers
- the line edit on Waydroid, and the ZIP comparison for CLI export
- bundling all fonts (replacing my DP-7)
- invoices as a browser handoff

Everything else stands.
