# P4 shell source handoff

Owner w23:p22, 2026-10-25. Source authored by p22 without running anything; **all execution below was performed by lead w23:p1E**. Regression sources preceded their corresponding implementation; no reproduced-before/passed-after claim.

## Executed evidence (lead runs)

| Check | Result | Where |
|---|---|---|
| Native client, ffi, wasm and taskr-work Rust suites | PASS 104 tests / 10 suites | artifact604 |
| `taskr-client --test audio_summary` after the `watch_limit` mapping | PASS 7/7 | artifact650 |
| Actual Arch build-std wasm build; generated JS/wasm, TS types, fixtures | PASS | artifact604 |
| Actual wasm Playwright, desktop + Pixel 7, `playwright.p4.config.ts`, on the owned `runtime.py` runtime with immutable taskr `2f2a5fea…` | PASS 4/4 in 10.0s | `/tmp/rustr-p4-shell-lead-3dugm256/logs/playwright.log` |
| Generated UniFFI Swift listener in the pinned `swift:6.2-noble` amd64 container (`sha256:c58ddae4…`), same runtime, immutable matching `libtaskr_ffi.so` (max GLIBC_2.38; Noble 2.39; `ldd` clean; no Rust rebuild) | PASS: `P4_UNIFFI_PASS progressive-before-final authoritative-replacement detach/resume canonical-reload cancellation session-loss one-dispatch-per-start` | `/tmp/rustr-p4-shell-lead-3dugm256/logs/swift-container.log` |

**Scope of the Swift proof:** Linux Swift compiled against the real generated UniFFI bindings and callbacks. It is not iOS, SwiftUI, Kotlin/Android, an Apple SDK, or a device or simulator. In the container only the compile and run steps of `uniffi.sh` ran, because the image has no cargo; bindgen ran on the host from the same library build.

**Visual inspection (lead):** in actual Brave, the lead inspected sign-in, the Audio summaries section, work selection, the idle disclosure and the disabled Generate button. The lead also inspected the saved Playwright PNGs of the desktop partial state and the phone. Layouts were readable with no overlap observed. The attached-Brave tool's label-based Email fill did not take effect, while the exact `input[type=email]` selector worked. Screenshots needed CDP `fromSurface:false`. These are tooling notes; no product defect was inferred. p22 did not inspect the images itself.

**Runtime cleanup (lead):** `runtime.py` exited 0 after SIGINT. `sentinel.json` was `[]`, so the summary flow never reached the storage, transcriber or billing-provider origins. Docker inventories for labels `rustr.p4.shell` and `rustr.p4.swift=3dugm256` were empty afterwards. The Swift proof manifest is `/tmp/rustr-p4-shell-lead-3dugm256/logs/swift-proof.json`.

## Files and public seams

- `crates/taskr-client/src/{lib.rs,audio_summary.rs,audio_summary/{transport,fixtures}.rs}` and `tests/audio_summary.rs`: additive authenticated client and shared Rust shell presentation. `SummaryController::{refresh,select,start,watch,suspend,cancel,view,dispose}`. Exact p1T direct snapshots plus `{summary,generation}` discovery, including `execution_id`; no guessed routes/envelopes. Native uses p21 `ApplicationClient`; wasm uses existing reqwest/fetch ReadableStream bytes into `ApplicationDecoder`; both use real `StreamState`.
- Identity validates generation/source revision/neutral taskr epoch 0/execution against authenticated snapshot. Cursor is `durable_seq`, never transient `last_seq`. `identity_unavailable`/`unavailable` controls detach with Resume, not durable failure/new generation. Unsuccessful terminals read owner-scoped metadata for reason/remote uncertainty. Revoked/session-changed/disposed clears content and fences late publication. Lost start response retains request identity. Suspend/dispose stop attachment only; cancel calls host API, never claims remote deletion.
- `crates/taskr-wasm/src/{summary,summary_dto,lib}.rs`: `AudioSummaryPresenter::new(api_url)`, `dispatch(intent,on_view?) -> Promise<AudioSummaryView>`, `current`, synchronous `dispose`. Select/start/resume publish progressive callbacks before settlement. Existing fixture exporter now generates summary views and TypeScript contracts.
- `crates/taskr-ffi/src/{summary,lib,app}.rs`: UniFFI `AudioSummaryPresenter::{new(api_url,SessionVault),refresh,select(work_id),start(request_id,accepted),watch(AudioSummaryListener),suspend,cancel,current,dispose}`. `AudioSummaryListener::on_view(AudioSummaryView)` is foreign callback; blocking methods run off UI thread. Callbacks are outside locks; current/suspend/dispose can re-enter. Existing `VaultStore` became crate-visible for reuse; old public APIs unchanged.
- `apps/web/src/{App.svelte,lib/app/{AppContainer,SignedInShell}.svelte,lib/core/{index,fixtures}.ts,lib/audio-summary/**}`: actual authenticated Audio summaries surface, pure desktop and phone views, disclosure checkbox, escaped plain text, Rust-derived action flags/statuses, fixture-backed stories/component sources. Existing task/member surfaces remain. No taskr-core/navigation/policy edits or copied Transgate quota/consent.
- Evidence source: `scripts/p4-shell/{provider.mjs,seed.mjs,uniffi.sh}`, `apps/web/{playwright.p4.config.ts,p4-e2e/summary.spec.ts}`, `apps/ios/p4-summary/main.swift`.

Only client manifest changed: sibling llm-core/llm-http path deps, futures-util 0.3, reqwest 0.13 stream; dev axum 0.8/Tokio 1 macros/rt-multi-thread/net/sync. No root/lock/book/canonical/host/policy/source-Transgate edits. Native/wasm manifests require no direct new deps.

## Lead integration commands

After coherent shared/host source and lead lock refresh:

```sh
cargo test --locked -p taskr-client --test audio_summary
cargo test --locked -p taskr-wasm
TASKR_WASM_BUILD_STD=1 ./scripts/build-web.sh
./scripts/gen-bindings.sh
(cd apps/web && npm run test:ci)
```

Seven client regressions: disclosure-before-dispatch; unfinished transcript; old-session late response; real chunked HTTP partial/detach/replacement/final with one start and disposal fencing; lost-start identity reuse; saved seq1/control seq2 reconnect after1; p1Z's 429 `watch_limit` refusal stays resumable with the host message and no new generation. Component selectors: `src/lib/audio-summary/views/{desktop/AudioSummaryDesktop,mobile/AudioSummaryMobile}.test.ts`.

Wasm cargo tests generate `types/AudioSummary{View,Intent}.ts`, `AudioWorkRowView.ts` and `fixtures.json` summary section; wasm build generates real JS/wasm export; gen-bindings regenerates Kotlin/Swift. **Generated outputs deliberately await lead execution, not handwritten substitutes. Generate before frontend checks.**

## Actual browser and foreign-listener proof sources

`scripts/p4-shell/runtime.py` supplies the whole owned runtime. It follows p1Y's proven P3 taskr smoke: TCP readiness, provisioning order, restricted login and a clean env, with no shared simulator code. Prerequisites: immutable copies of the lead-built `taskr-server` and `billing-provision` in one directory; the actual wasm pkg (`build-web.sh`) and generated summary types (`cargo test -p taskr-wasm`); `apps/web/node_modules`; a cached `postgres:17-alpine`; host `docker`, `node` and `psql`. It never builds, pulls or contacts a non-loopback origin.

It starts a labeled cluster, runs `CREATE DATABASE` and `billing-provision`, then `provider.mjs` on a free loopback port. A sentinel listener stands in for the `MEDIA_R2_*`, `SONIOX_*` and `LEMONSQUEEZY_*` origins; the summary flow must never reach it, and any request it records fails the run. Then the actual `taskr-server` starts with dev auth, the audio-work sample and summaries (`TASKR_SUMMARY_PROVIDER=deepseek`, `TASKR_SUMMARY_API_KEY=p4-synthetic-key`, `TASKR_SUMMARY_BASE_URL=<provider>/v1`; the base URL is never omitted). The runtime login is asserted non-elevated. Finally the actual Vite/wasm shell starts with `VITE_TASKR_API=/api` proxied to the host.

It seeds one native owner with `seed.mjs` and checks that the summary routes are mounted and empty (`{summary:null,generation:null}`), with no dispatch. It then prints and writes `<logs>/p4.env` with `TASKR_P4_API`, `TASKR_P4_WEB_URL`, `TASKR_P4_PROVIDER_CONTROL`, `TASKR_P4_FIXTURE_DATABASE_URL`, `TASKR_P4_EMAIL` and `TASKR_P4_WORK_ID`, and holds until Ctrl-C. Alternatively, pass a command after `--`. Run the browser and Swift suites sequentially, not concurrently, because both count provider dispatches.

```sh
python3 scripts/p4-shell/runtime.py --bin-dir /tmp/<snapshot>/bin -- \
  sh -c 'cd apps/web && npx playwright test --config playwright.p4.config.ts'
python3 scripts/p4-shell/runtime.py --bin-dir /tmp/<snapshot>/bin -- ./scripts/p4-shell/uniffi.sh
```

Teardown runs in reverse start order: Vite (closing proxied SSE), the host on SIGINT, then the provider. It writes `sentinel.json` and `postgres.log`, then removes only the container whose label `rustr.p4.shell` equals this run. Logs default to `/tmp/rustr-p4-shell-<run>/` (`taskr-server.log`, `provider.log`, `vite.log`, `binaries.json` with SHA-256). A forced kill, a sentinel hit or a cleanup refusal makes the exit status nonzero.

Dedicated suite requires env, never skips. Actual desktop+Pixel 7 browser/wasm/auth/host/SSE, no page-route mocks. Fresh owner enroll + `scripts/p4-fixtures/taskr-completed.sql` with published UUIDv5/dev subject mapping, then an owner-scoped GET confirms the completed work before the UI is touched (taskr summary host source now owned by p1Z; DTO drift is coordinated with p22 directly). Provider holds `Hello `: assert visible trimmed `Hello` before upstream finish; detach/resume and page reload without redispatch; final saved replacement `Hello world`, not `Helloworld`/trailing spaces; canonical reload; late cancelled response cannot save; sign-out removes old surface. Partial/final screenshots, traces and reports are captured by the run. The runtime removes the owned cluster afterward; no external object-cleanup calls are added. Under the per-account watch cap of 4, test 1 opens up to 3 sequential attachments per owner (pause, resume, reload); a closed attachment must release its slot promptly.

Foreign proof: the runtime already seeds a fresh owner and exports `TASKR_P4_EMAIL` and `TASKR_P4_WORK_ID`; each runtime run supports one Swift run. To repeat within a held runtime, export a fresh email and work UUID, then run `node scripts/p4-shell/seed.mjs`, then:

```sh
./scripts/p4-shell/uniffi.sh
```

Seed enrolls exact owner, uses same SQL and verifies readable completed transcript. Launcher builds cdylib/bindgen, generates real Swift into `target/p4-shell-uniffi/generated`, compiles it with `apps/ios/p4-summary/main.swift`, runs listener. Existing AppPresenter sign-in/SessionVault; background foreign callbacks re-enter current/suspend. Asserts partial-before-final, same-generation resume, authoritative replacement, fresh-presenter saved read, cancel preserving prior canonical artifact despite late response, session loss/disposal clearing and no late callbacks, exactly one provider dispatch for each of three explicit starts. Success prints `P4_UNIFFI_PASS ...`; failure/missing fixtures/timeouts exit nonzero.

## Acceptance boundary

Source implements D4/D7, PRD 6.10 and contract taskr streaming/session/cancellation responsibilities. Shared tests own split whitespace/redaction/HTML protocol cases; taskr sanitation remains its own complete-answer trimming, not Transgate policy. Full Transgate P5/P8 rendering/device work remains separate. Executed coverage is exactly the table above: actual wasm browser on desktop + phone and a generated-UniFFI Linux Swift listener over the actual taskr host with synthetic loopback completions. Not proved: iOS/SwiftUI/Android/device or Mac acceptance, live model availability/quality/region/retention/cost, and rendered Transgate surfaces. Final P4 acceptance is the lead's.

Next: lead w23:p1E records acceptance; return any concrete failures to w23:p22. No known source blocker.
