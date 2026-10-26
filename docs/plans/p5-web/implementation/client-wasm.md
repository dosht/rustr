# P5 transgate/client and transgate/wasm (p22)

Owner w23:p22, same saved session: Opus authored the original implementation and regression sources; user-authorized Astra continuation consumes the published billing boundaries and adds invoice consumer regressions. Opus source and evidence provenance are retained. Original handoffs were source-only, with no owner-run builds, tests, lint, formatters, browser runs or provider calls. Y's later frozen-S1 evidence, the separately unverified working-source repair and the subsequent owner-run REPAIR-P5-1 loader smoke are recorded below. The shared-limit cutover and billing-composition migration were saved in Y's explicitly opened pre-S1 windows; later repairs touch working source, never frozen candidate snapshots.

## Published interfaces

- **TS facade** `transgate/web/src/lib/core/index.ts`:
  - `loadTransgate(config: TransgateConfig, platform: Platform): Promise<TransgateCore>`.
  - `TransgateCore { dispatch(intent: AppIntent): AppVm; subscribe(listener): () => void; current(): AppVm; dispose(): void }`. `dispatch` is synchronous; later views go to subscribers outside the Rust state borrow.
  - `TransgateConfig { apiUrl, authUrl, authKey?: string | null, siteUrl, captcha: boolean, documentAssets: DocumentAssetUrls }`. The first five come from host `tg-config` (`captcha` is true when Turnstile is configured; it feeds core `AppConfig.captcha`). Relative URLs resolve against the page. `documentAssets` holds the same-origin `?url` imports of `transgate/export/{fonts,assets}` (p1V), with fallback keys equal to the `FallbackFont` wire values. Under D20 those served URLs are V's `/_app/*` output. No client or wasm source contains an asset prefix.
  - Viewport changes: the container dispatches `{"type":"viewport_changed","intent":{"viewport":{"touch":bool,"width":number}}}` (confirmed to p1V); the presenter measures the first one at `start`.
- **Platform port** `transgate/web/src/lib/core/platform.ts`: `measure`, `put`, `forget`, `player`, `copy`, `save`, `preview(bytes, filename, mime)`, `share`, `track(event, Record<string, string | number | boolean | null>)`, `resetTelemetry()` (PostHog `reset()` only if started, never loads the SDK; agreed with p1V).
  - Browser objects only. HTTP, auth, storage, randomness, locks, navigation and the D17 checkout form POST stay in Rust.
  - `mountCheckoutForm` was removed by agreement with p1V (D17).
  - Implementers: p1Z (files/media), p1V (assembly and the rest).
  - `preview` is the agreed synchronous invoice-modal capability: Blob/object-URL failures throw like `save`; never a download or popup fallback. V owns the root-scoped URL resource, revocation on replacement/close/route exit/session loss/disposal, modal binding and CSP. Rust checks the core-owned preview ticket, invoice context, selection, session and disposal before invoking it.
- **Generated outputs:**
  - `transgate/web/src/lib/core/types/*.ts`, `fixtures.json` (`{"<VmType>": {"<name>": VM}}`) and the contextually typed `fixtures.generated.ts` companion come only from p1X's core generator (`presentation::fixtures::web::generate`, feature `ts`). The companion derives from the identical serialized JSON payload. transgate-wasm has no exporter and no ts-rs dependency.
  - `transgate/web/src/lib/core/fixtures.ts` (p22): unchanged `fixture(type, name)` / `fixtureNames(type)` consumer API, now importing the generated `fixtures` value and `Fixtures` interface directly. The generated contract owns all 21 roots, including `SessionProfile`; the accessor has no duplicated type map, raw JSON assertion or generated-file writes.
  - `pkg/transgate_wasm.js` from `scripts/build-transgate-wasm.sh` (wasm build plus wasm-bindgen only).
- **Storage keys:** session `transgate.session` (also P6's presence hint); PKCE verifier `transgate.session.code-verifier` (`/PASSWORD_RECOVERY` suffix for recovery); `navigator.locks` name `transgate.session`; first-touch attribution `transgate_utm_data` (source key).

## transgate/client

| Module | Content |
|---|---|
| `auth` | `GoTrue` over ports `AuthStorage`/`Random`/`Clock`/`SessionLock` (native `OsRandom`, `SystemClock`). Password sign-in; signup, magic link (`create_user`), recovery and the Google authorize URL with S256 PKCE and `redirect_to`. `exchange_code` (recovery verifier yields `PasswordRecovery`), `update_password`, global `sign_out` (401/403/404 still clear locally). `access_token(user)` refreshes when due; `renew(user)` refreshes now (the host rejected the token). Both take the cross-tab lock, reread, adopt a rotation another tab made meanwhile, and write only if storage still holds the session they rotated. Captcha goes in `gotrue_meta_security`. Helpers `parse_callback`, `strip_callback`, `challenge` |
| `api` | `HttpApi::new(base, Arc<GoTrue>)`; every call names its user, so another account's token is never sent. Jobs, drafts, detail plus summary, lines, draft create/options (absent vs `null`), remove draft, delete/delete all, edit line, export bytes (RFC 6266 UTF-8 name), init-upload, streaming-url, YouTube add/resolve, language usage, settings. P5 host routes: `session(locale)`, `start_jobs` (402 `insufficient_balance` stays status/code), `submit_feedback`, `request_support` (202 = queued, not mailed), `request_erasure`, `erasure_status`. `ApiError::{SignedOut, Network, Http{status, code, message, body}, Decode}` reads both the transgate `{error, code}` and the billing `{error: {code, message}}` bodies; `failure() -> Option<ApiFailure>` |
| `billing` | The authenticated billing routes under `/api` with billing-core DTOs: `catalog`, `balance` (typed projection of `Overview.balance`, `is_freemium`, `special_client_alert`, preserving the configured alert verbatim and no longer `Copy`), `quote`, `coupon`, `subscription`, `cancel_subscription`, `invoices`, `customer_invoices` (`POST /billing/admin/customer-invoices` with `{email}`, decoded as R's `AdminLookupReply`). Temporary Bill To never enters an HTTP request. `ApiError::purchase_failure()` maps 401/no session to `Unauthenticated`, 410 to `Erased`, other 4xx with a code to `Refused{code}`, the rest to `Unavailable` |
| `ai` | `start_analysis`/`start_chat(history)` (requestId, header, then body drop), `retry_generation`. `StartOutcome::{Accepted, Rejected{status, StartRejection}, SignedOut, Unknown}`. `consent`/`record_consent`/`revoke_consent` (D22: POST `false` is the nondestructive refusal, DELETE the settings turn-off with the account-side purge; `cleanup_pending` decoded), `chat_limits`, `analysis`, `chat_history`, `generation`. `watch(user, generation, WatchKind, after, on_item)` returns `Ok` only after a terminal item, else `WatchFailure::Disconnected` |
| `attribution` | Source `utmCapture.ts`/`deviceDetection.ts`: first-touch `capture`, `device_type`, `browser`, `checkout_attribution` (the `OrderBody.analytics` object and the `checkout_started` extra properties) |
| `export` | Consumer of W's frozen adapter. `DocumentFonts`: one lazily created `DocumentWriter`, base assets fetched only on the first PDF, an extra family only when `analysis_fonts`/`invoice_fonts` asks, kept across sessions as public assets. `export_analysis` and `export_invoice` accept an already captured document, a `current` predicate and a delivery callback, returning `Delivery::{Delivered(Result), Failed(ExportFailure), Stale}`. Both check the fence before rendering and after the renderer await, immediately before byte delivery. Preview adds X's request/context predicate; downloads keep the session/disposal fence. |
| `checkout` | D17. `checkout_form(&OrderBody, attribution, token) -> Result<CheckoutForm{order, access_token}, PurchaseFailure>`: unpadded base64url JSON; oversize is `Refused{payload_too_large}`, an empty or non-visible-ASCII token `Unauthenticated`. `Debug` redacts the token. Imports `CHECKOUT_FORM_MAX_BYTES`, `CHECKOUT_TOKEN_MAX_BYTES`, `CHECKOUT_ORDER_MAX_BYTES` directly from `billing_core::presentation::purchase` (32768/8192/16384); local copies and names removed, including in the existing boundary tests. Encoding, validation and refusal behavior are unchanged. |

**Tests** (`transgate/client/tests/`):
- `gotrue.rs`: loopback GoTrue enforcing S256, single-use flow codes and single-use rotated refresh tokens. Callback, recovery, wrong/expired code, two-tab single refresh, late refresh failure/success versus a newer account. New for `renew`:
  - `a_token_the_host_rejected_is_renewed_on_demand_before_its_expiry`
  - `two_tabs_renewing_the_same_rejected_token_rotate_it_once`
  - `renewing_for_a_user_who_is_no_longer_signed_in_here_sends_nothing`
- `api.rs`: per-user token isolation, option absent/null, UTF-8 disposition, text 401.
- `ai.rs`: start without waiting for the stream body plus observed body drop, quota rejection, missing header is `Unknown`, split analysis events, disconnect before terminal.
- `attribution` and `checkout` unit tests in-module.
- `export.rs`: the delivery fence driven by **real core `AppState` transitions** (no counter): the ticket is `ai_epoch()` as the analysis effect carries it, checked with `AppState::is_current` plus disposal; real `DocumentWriter`, W's partial fixture, licensed fonts, held base asset load. `cargo test --locked -p transgate-client --test export`:
  - `sign_out_then_same_owner_relogin_during_a_held_font_load_saves_nothing` (SignOutRequested, SignedOut, SignedIn same user, regrant)
  - `an_account_switch_during_a_held_font_load_never_delivers_the_previous_capture` (SessionAdopted other user, who consented)
  - `a_consent_refusal_then_regrant_leaves_an_older_capture_stale` (POST false, then enable)
  - `turning_ai_off_with_delete_then_regrant_leaves_an_older_capture_stale` (D22 DELETE with `cleanup_pending`, then enable)
  - `a_token_rotation_for_the_same_owner_still_delivers_the_clicked_capture` (a refresh must not eat an export)
  - `disposal_during_a_held_load_saves_nothing_and_the_public_fonts_stay_cached`
  - `an_unchanged_session_delivers_the_clicked_snapshot_exactly_once`
  - `a_failed_save_is_reported_as_a_save_failure`
  - `font_free_formats_render_without_fetching_any_pdf_asset`
  - `an_invoice_held_across_same_owner_relogin_is_never_delivered`
  - `an_invoice_uses_the_session_fence_not_ai_consent`
  - `closing_and_reopening_the_same_invoice_fences_held_bytes_and_old_failure`: captures tickets from actual core effects, holds the real invoice renderer's font load, closes/reopens the **same** payment, injects the old `DocumentFailed`, rejects the old PDF delivery, and then delivers the new request using cached public fonts. No local surrogate ticket or hand-advanced epoch.

## transgate/wasm

- `presenter.rs`: `TransgatePresenter::{new(config, platform), listen, start, dispatch, current, dispose}`. Builds `GoTrue` with browser ports, `HttpApi`, `DocumentFonts`, `AppState::new(AppConfig{site_url, captcha})`. `start` captures attribution, wires `popstate` to `RouteChanged` and `storage` (session key or clear) to other-tab adoption, then sends `Started`. `apply` binds the user named by an accepted auth event to the session core holds after it, before that event's effects run.
  - Locale projection also handles `AppVm::CheckoutConfirm`. `is_current_invoice_preview(epoch, ticket, payment_id)` adds disposal and `AppState::is_current(epoch)` to X's allocation-free `AppState::is_current_invoice_preview(ticket, payment_id)`.
- `driver.rs`: every `AppEffect` (table below). Core fences each event by epoch. The driver adds: a request runs only with the token of its epoch's user (`Inner::user(epoch)`); clipboard, file save, share, player and checkout form act only while `Inner::is_current(epoch)` holds immediately before; any 401 also sends `Unauthorized{epoch}`, any 410 `account_deletion_in_progress` `AccountErased{epoch}`, and a local `SignedOut` (no session for that user any more) re-reads storage as an other-tab change.
- `browser.rs`, `platform.rs`, `tasks.rs` (keyed abortable watches/timers), `assets.rs` (`FetchedAssets`; nothing fetched at load, no CJK for Latin), `lib.rs`.

| Effect | Work | Answer |
|---|---|---|
| `Auth` | GoTrue flow; `GoogleRedirect` assigns the authorize URL; `RestoreSession` = stored session refreshed when due; `Refresh` = `renew(epoch user)` | `AuthEvent` (`SessionAdopted` / `SignedOut` / `Failed`) |
| `LoadSession`, `LoadBalance`, `LoadJob`, `LoadCatalog`, `LoadSettings` | `/session?locale=`, `/billing/overview`, `/jobs/{id}`, `/billing/catalog`, `/settings` | `SessionLoaded`, `Balance`, `JobLoaded`, `Catalog`, `ManageData{Loaded}` |
| `LoadQueue` | drafts plus language usage (usage only orders menus) | `Upload{Restored}` |
| `Navigate` / `Leave` / `Timer` | history push/replace / `location.assign` / keyed sleep | — / — / `Timer` |
| Upload | measure, create draft, options, init-upload + PUT with progress, delete draft, start (options, then `/jobs/start` with locale), YouTube add/resolve, draft poll with elapsed time from poll start | `Measured`, `DraftCreated/Failed`, `UploadProgress/Finished`, `StartSucceeded/StartFailed{balance: 402 or insufficient_balance}`, `YouTubeAdded/AddFailed`, `YouTubeResolved/ResolveCrashed`, `DraftsReloaded` |
| Transcript | save edit, delayed poll/lines, player pause/resume/seek, server ZIP export, copy, media URL and its timers | `JobPolled`, `LinesArrived/LinesFailed`, `PlayerState`, `MediaUrlLoaded`, `MediaUrlDue`, `NoAudioAlertExpired` |
| Jobs | page with calendar offsets, delete, server ZIP export | `PageLoaded/LoadFailed`, `Refresh` after a refused delete, `DownloadFinished` after every export |
| ManageData | settings patch, delete all, erasure request | `Saved`, `Jobs{Refresh}` after delete all, `ErasureState/ErasureFailed` |
| Feedback / Support | POST | `Answered` |
| Consent | GET, POST `{hasConsented}`, DELETE | `Loaded`, `DecisionSaved` |
| Analysis / Chat | load, start/retry with `RequestKey`, keyed watch (replaced per generation, aborted on `Detach`), generation, copy, analysis document export (fenced `export_analysis`), chat text download (share on touch, a dismissal ends it) | `AnalysisLoaded`, `StartAccepted/Rejected/Unknown`, `WatchAnalysis`/`WatchFrame`, `WatchEnded`, `GenerationLoaded`, `ChatCopied`, `ExportFinished` (none when stale) |
| Purchase | quote, coupon, D17 submit (token for the epoch user, `checkout_form`, fence, hidden form POST), `checkout_started` with attribution | `QuoteAnswered`, `CouponAnswered` (`Failed` on error), `CheckoutNotSubmitted` |
| Subscription | GET, cancel, `subscription_cancelled` | `Loaded`, `CancelAnswered` |
| Invoices `{epoch,ticket,effect}` | owner GET, administrator lookup POST, `InvoiceDocument::capture` once from effect facts before awaiting the existing renderer; Preview calls `platform.preview`, Download calls `platform.save` | `Loaded`, `LookupAnswered`, or `DocumentFailed{payment_id, failure: Unavailable}`, always echoing the originating effect ticket |
| `ReadConfirmation{epoch,sequence}` | only the embedded `script#tg-confirm[type="application/json"]`; missing/unreadable data is `None`; no auth, checkout or confirmation HTTP | `ConfirmationLoaded{epoch,sequence,payload}` |
| `Confirmation` / confirmation timer | `Track` forwards `payment_completed`, `payment_failed`, `subscription_started` with empty properties through existing consent-gated PostHog; normal keyed timer echoes `Timer::Confirmation{sequence}` | No telemetry answer; core handles navigation and repeat-payload fencing |
| `Telemetry` / `TelemetryReset` | `platform.track` / `platform.resetTelemetry` | — |

Driver choices accepted by p1X: a failed poll read re-runs `PollJob` (source interval kept polling); a failed draft reread is a `DraftsReloaded` tick with no drafts (UPL-008 timeout still applies); `SessionMissing` maps to `InvalidOrExpiredCode`, GoTrue `Rejected` to `Unavailable`; an offline restore is `Auth{Failed}`. Picked-file handles are released at dispose.

Refused writes without a failure event reread the stored truth (p1X rule, no new leaf events): `SetAutoDelete`/`SetDeletedMonthly` → `GET /settings` → `ManageData{Loaded}` (toggle returns to the stored value); `SaveEdit` → `GET /jobs/{id}/lines` → `Transcript{LinesArrived}` (merge by id restores the stored text); `SetOptions` → `GET /jobs/drafts` → `Upload{DraftsReloaded{drafts, elapsed_ms: 0}}`; `DeleteJob`, `DeleteDraft`, `DeleteAllJobs` → `Jobs{Refresh}` (delete all also refreshes on success). No reread when the session itself failed (no session, 401, 410 erased): that is already escalated and a reread would be refused the same way.

Resolved in core by p1X: a failed sign-out clears `signing_out`, so it can be retried (`a_failed_sign_out_can_be_retried`).

Invoice preview delivery combines the original session epoch/disposal fence with the **ticket carried on `AppEffect::Invoices`**, never a later ticket reread. X invalidates the ticket on preview/close, invoice-context changes, lookup, temporary Bill To changes, route changes and session reset. Core also rejects stale `DocumentFailed` tickets so a failed old render cannot close a new preview of the same payment. Capture, font, PDF, preview and save errors become R's `DocumentFailed { payment_id, failure: PurchaseFailure::Unavailable }`; rows stay usable, and X maps `document_failure` to app `An unexpected error occurred. Please try again.` No success is invented.

D12 confirmation analytics precision, agreed with V: `ConfirmAnalytic::Purchase` was the source GA conversion and is intentionally omitted because GA is not carried over. No new PostHog purchase event, conversion transport or server-analytics change. Confirmation navigation remains core-owned (`Leave`), not an HTTP confirmation.

## Manifests (registered by p1Y; lock 339b85e1, no new packages)

- **client** `transgate/client/Cargo.toml`: base64 0.22; billing-core, llm-core, llm-http, media-ports, transgate-core, transgate-export (path); futures-util 0.3; reqwest 0.13 df=false [json, stream] (+rustls native); serde/serde_json (workspace); sha2 0.10 df=false; url 2.5; uuid 1 df=false [serde]; native-only getrandom 0.3; dev axum 0.8, tokio 1 [macros, rt-multi-thread, net, sync, time]. Test-only compile-time inputs: `tests/export.rs` `include_bytes!` the base fonts and logo from `transgate/export/{fonts,assets}` and `include_str!` `transgate/core/src/documents/fixtures/analysis.json`.
- **wasm** `transgate/wasm/Cargo.toml` (`cdylib` + `rlib`): billing-core, transgate-client, transgate-core, transgate-export (path); futures-util 0.3; js-sys, serde, serde_json, serde-wasm-bindgen, wasm-bindgen =0.2.128 (workspace); wasm-bindgen-futures 0.4; reqwest 0.13 df=false (asset fetch); url 2.5; uuid 1 df=false; web-sys 0.3 [Crypto, Document, Element, Event, EventTarget, History, HtmlElement, HtmlFormElement, HtmlInputElement, Location, Navigator, Node, Storage, StorageEvent, Window]; on wasm32-unknown uuid [js]. No `build.rs`, no production compile-time non-Rust inputs.
  - Static resolution by p1Y: the wasm32 graph has only getrandom 0.4.3 (via zip, `wasm_js` on), so no `getrandom_backend` cfg is expected. `[INFERENCE]` until the lead's wasm build compiles it.
- `scripts/build-transgate-wasm.sh` mirrors `build-web.sh`.

## Integration evidence and remaining verification

- R's portable D17 limits and the published X/R billing composition are consumed. Billing-composition window changed: `transgate/client/src/{billing,export}.rs`, `transgate/client/tests/export.rs`, `transgate/wasm/src/{browser,driver,platform,presenter}.rs`, `transgate/web/src/lib/core/{platform,fixtures}.ts`, this handoff. The preceding window changed `transgate/client/src/checkout.rs` for shared limits. Manifests and root files unchanged.
- Corrected generated contracts were explicitly released by Y as P5-GEN-RELEASE-5: `/tmp/rustr-p5-tsgen/run5/report.json`, exit 0, three generator tests passed, 144 types and 21 fixture roots. Generated manifest SHA256 `5ff92934aa5daa730a35e6a7fd694c2a43dbeb40d6715bccc0e231edbfa2bb28`. Actual publication was relayed to Z/V; earlier failed/unreleased generation evidence remains preserved.
- V reported the agreed PDF-only modal preview capability, root-scoped URL lifecycle and disposal integration source-ready, using corrected run5 contracts. This is source integration, not browser/runtime proof.
- Y executed the already-authorized client tests, Arch build-std wasm check and release/bindgen build on frozen `s1-20261026144505`. Client compilation passed (34.47 s), behavior **44 passed / 1 failed**. The sole failure was the Appendix B test-vector discrepancy below; all 12 export cases, including held same-payment bytes/old failure, passed. Evidence: `/home/mu/.cache/rustr-integration/s1-20261026144505/logs/p5-client-test.log` (read by p22, not rerun).
- Y reports wasm32 check passed (42.18 s), release plus bindgen passed (about 61 s), no getrandom cfg/override. Frozen wasm SHA256 `00da14d5ea81f17754489717ccfd2d5a6d21a1744c92c774af5197af66aea2ae`. These are build results, not app/browser behavior proof.

## Bounded S1 working-source repair

Root cause: `pkce_challenge_matches_rfc7636_appendix_b` contained **two mistranscribed literals**, not the [RFC 7636 Appendix B](https://www.rfc-editor.org/rfc/rfc7636.html#appendix-B) verifier/challenge pair. The frozen test used verifier `dBjftJeZ4CVP-mJ0kz2-wIl4_n8d3NQS0ZH1HKyFG9w` and asserted `E9Melhoa2OwvFrEMTJguCHaoeK1t8URWuGJ3WuiTnUM`; neither is the RFC example. Production `challenge` implements the specified unpadded base64url of SHA-256 over the verifier's ASCII bytes and is unchanged.

The working-source test now uses the normative literals verbatim:

- verifier: `dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk`
- challenge: `E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM`

The exact-equality assertion remains; it is not pinned to the observed failing digest. The owned unused `Value` import in `transgate/client/src/billing.rs` was removed with the language-server import quick fix. Repair paths: `transgate/client/tests/gotrue.rs`, `transgate/client/src/billing.rs`, this handoff. No production auth, root, dependency or generated-file change. Frozen S1 artifacts and failure evidence are untouched. **Repair saved but not rerun by p22; Y's subsequent verification is pending.**

## REPAIR-P5-1 typed fixture loader

Consumed X's published `fixtures.generated.ts` exports; removed the loader's duplicated interface/import list and `raw as FixtureFile` assertion. `fixture` and `fixtureNames` retain their behavior. No external `Fixtures` type imports were found, so no obsolete compatibility re-export remains. Platform/browser and telemetry-test ownership was corrected back to V before any p22 edits; those files are untouched by this slice.

Owner runtime smoke (Bun 1.4.3 Eval) imported the actual migrated loader and checked every returned name and complete serialized payload against the retained JSON: **21 roots / 170 fixtures passed**, including `SessionProfile.signed_in`; an unknown fixture name still threw. The first strict-object comparison failed because imported objects and Eval-parsed JSON have different realm prototypes (explicitly inspected); payload serialization matched. A standalone Bun smoke could not launch because `bun` is absent from PATH. The completed Eval smoke compared the full JSON-value contract without altering source or fixture data. No permanent test, Vitest suite or owner-run npm build is claimed.

Z subsequently reports `npm run build` reached svelte-check: **1,412 files, two errors, zero warnings; no fixture-loader/generated-module diagnostics**. Remaining errors are missing `core/pkg/transgate_wasm.js`/declarations and V's `TextField.stories.ts` union complexity. Overall build exited 1; no app-build acceptance is claimed. Z released the build slot to V. The source package prerequisite and final build remain with their assigned owners; candidate3 and generated files were not modified by p22.

## REPAIR-P5-3a initial browser location

**Root cause: an adapter contract mismatch, not GoTrue's redirect target or core's post-sign-in route choice.** Candidate4's `presenter.rs` passed absolute `location.href` to `AppIntent::Started` and `RouteChanged`; core `route::parse` expects a path with query. Consequently every absolute startup URL returned `None`, selected default English/home and discarded the callback code or job route.

Candidate4 receipts under `/home/mu/.cache/rustr-integration/s1-20261026195749/logs/next-p5-c/`:

- `c2-pkce-observations.json`: the delivered `/auth/v1/verify` link targets `http://127.0.0.1:37363/home`; the browser initially reached `/home?code=…`, then `/signin`, retaining its verifier without a session. No HAR, exact 302 Location or PKCE token-request trace was captured; **request absence is not claimed**, and the verification link was not replayed.
- `c2-independent-report.json`: after password login, the phone's job-1 deep link ended at `/home?view=new`. Core received default home instead of job 1; its normal phone-home rule then selected the new-transcript page.
- `c2-report.json`: two English locale assertions passed and all 30 non-English assertions failed with `html lang=en`. The same startup fallback explains this: core's locale stayed English, before catalog loading. V independently reported that the shell sets `html lang` from `vm.locale` before catalog loading; no catalog-loading change is needed for this cause.

The approved fix is `transgate_client::location::app_location`: serialized HTTP(S) browser href becomes path + search, preserving encoded values for core to decode once and removing the fragment. It reuses the input `String`; no new dependency, web-sys dependency or product decision. Wasm calls it at **both startup and popstate**. The constructor retains absolute href to resolve API/auth endpoints and document assets. All other production href/location inputs were traced: core builds auth redirect targets from the site origin plus a locale path; `Navigate` uses core's path; `Leave` performs full navigation. `begin_session` redirects only an actual sign-in route, and profile loading re-enters the retained route. No host/core source was changed.

Ordinary native regressions in `transgate/client/tests/browser_location.rs` use the published client helper and real `AppState`, without cross-crate path includes:

- `absolute_callback_url_reaches_core_exchange`: `/home?code=flow%2B1` emits canonical `/home` plus `ExchangeCallback("flow+1")`, not restoration; repeated startup cannot exchange again.
- `absolute_phone_deep_link_opens_requested_job_after_session_restore`: the restored phone session retains job 1 and emits `LoadJob(1)`; subsequent history navigation loads job 2.
- `absolute_locale_callback_preserves_locale_and_query_route`: `/tr/home?view=all-transcripts&code=…` produces **`AppVm::Restoring { locale: "tr" }`**, preserves the selected query route, and exchanges the code.

Executed `cargo test --locked -p transgate-client --test browser_location -- --nocapture`: **all three failed before normalization**, reproducing `RestoreSession`, `Home(New)` and locale `en`; **all three passed after**. The initial extraction used the unchanged wasm helper through a test include; lead then moved the helper into client, and the final tests import its public module. Assertions were not weakened. A disposable native CLI smoke ran the actual client helper and `AppState` with the three URLs: observed exchange effect, signed-in job-1 route/load, and Turkish VM/query route. Its source was removed after success. Logs copied from actual executed output are `/tmp/rustr-p5-3a-p22/{before,after-and-smoke}.log`.

Known verification gap: the initial native `cargo test -p transgate-wasm --lib browser::tests::` could not compile because `NavigatorLock` awaits a non-`Send` `JsFuture` while native `BoxFuture` requires `Send` (`browser.rs:70–92`). This pre-existing native-only boundary was not changed or bypassed in production; evidence is `/tmp/rustr-p5-3a-p22/native-wasm-gap.log`. No new wasm32 build, bindgen output, browser/auth replay or visual acceptance is claimed for this repair. Y/lead owns the fresh built-browser callback, phone deep-link and full locale re-proof.

Final source paths: `transgate/client/src/{lib,location}.rs`, `transgate/client/tests/browser_location.rs`, `transgate/wasm/src/presenter.rs`, this handoff. Temporary wasm module/browser edits were removed; manifests, dependencies, generated contracts, candidates and other owners' files are unchanged. Original Opus/P5 and frozen failure evidence remain intact. Source-ready; hold for integration.
