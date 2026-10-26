# P5 integration, packaging and independent test ownership (p1Y)

Owner: w23:p1Y, the preserved independent integration/testing session. Opus-authored source and evidence are retained; Astra continuation in the same session is user-authorized. No new agents or change of ownership.

Status: **NEXT-S1-C3 executed on candidate3; all authorized remaining checks passed.** Native host/FFI builds were not repeated and captured source was unchanged. Full receipts are under `/home/mu/.cache/rustr-integration/s1-20261026151947/logs/next-s1-c3/`. P4 process/browser/Swift re-proof is complete within the fixture limits below; P5 C1/C2/C3 remain unauthorized and unclaimed.

## 1. Role and checkpoints

I am the independent tester of P5. Implementers do not certify their own UI. The lead authorizes each checkpoint and records the result; the lead does not drive the browser. Core behaviour tests (Rust and Vitest, owned by the implementers) stay separate from rendered evidence. A passing unit test is never reported as a rendered or browser result.

| Checkpoint | Entry condition (lead authorizes) | What I execute and report |
|---|---|---|
| C1 generated-fixture screens | Core's `presentation::fixtures::web::generate` has generated the Rust fixtures and TS; stories render them for desktop and phone; locale catalogs and generated tokens exist | A static Storybook build, then: <ul><li>visual capture of every story in light, dark and system (system via `emulateMedia` colorScheme), with Paper Moss plus the other palettes;</li><li>axe results;</li><li>keyboard focus order and visible focus;</li><li>native dialog focus return;</li><li>long strings;</li><li>RTL transcript;</li><li>the landscape notice with the app beneath `inert`.</li></ul> |
| C2 complete wasm flows | `transgate/web` builds against the real wasm; the host routes in the contract are mounted; the owned runtime (§2) starts | Playwright on the Desktop Chrome and Pixel 7 (`hasTouch`) projects over the real Axum host, owned PG, GoTrue, Mailpit and loopback providers, covering every applicable WEB flow:<ul><li>PKCE sign-up confirmation, magic link, recovery, expired code, and two tabs with rotation;</li><li>upload with a signed PUT, the player with ranged GET, editing, and export bytes;</li><li>AI start, header, watch and reattach;</li><li>the checkout handoff and HTML confirm;</li><li>support, feedback, invoices and the erasure receipt.</li></ul>A separate visual, keyboard and interaction inspection uses owned tabs. All 16 locales are exercised. `data-layout` proves which branch mounted |
| C3 packaged app | The Docker image from §5 builds | A non-root, read-only container serving the SPA and API from one origin. The C2 subset runs against the container: sign-in, upload, transcript, AI, checkout return and erasure receipt. Also checked: an SPA deep link, the API JSON 404, liveness and readiness, and that fonts and assets are same-origin with no CDN request |

Each checkpoint report contains:
- exact binary, wasm and image hashes;
- the command used;
- pass and fail per project and locale;
- screenshots;
- fixture versus product classification;
- explicit limits.

Live OAuth, captcha, payment, provider, storage, telemetry and SMTP acceptance are never claimed from local fixtures.

Document bytes (DOCX/PDF) produced in the browser are parsed by an independent reader, not the writer, and compared per shape with the source-derived references owned by the capture owner. The wasm32 compile of the krilla stack is proved at the first authorized build; it is not claimed before then.

## 2. Owned runtime: `scripts/p5-web/runtime.py`

```sh
# lead-authorized only; immutable snapshot root with target/debug -> ../bin
systemd-inhibit --what=sleep:idle python3 scripts/p5-web/runtime.py --root /tmp/<snapshot> \
  --web-dir /tmp/<snapshot>/web-dist -- sh -c 'cd transgate/web && npx playwright test'
```

The launcher never pulls, builds or installs anything. Every resource it creates carries the label `rustr.p5.web=<run>`, and the label is verified before removal.

| Component | Instance | Wiring |
|---|---|---|
| Network | `rustr-p5-net-<run>` | Lets GoTrue reach PG and Mailpit by container name |
| PostgreSQL | `postgres:17-alpine`. The owner DB is set up with the proven P3/P4 sequence:<ul><li>TCP readiness plus `SELECT 1`;</li><li>`billing-provision`;</li><li>restricted runtime login.</li></ul> | GoTrue's `auth` schema and `supabase_auth_admin` role are created on the same database, using the `deploy/dev/init-auth.sql` statements |
| GoTrue | `public.ecr.aws/supabase/gotrue:v2.197.0` (cached) | <ul><li>HS256 secret shared with the host's `SUPABASE_JWT_SECRET`.</li><li>Autoconfirm is off: confirmation, magic-link and recovery mail go to Mailpit.</li><li>`SITE_URL` and the allow-list are the app origin.</li><li>Google is enabled with synthetic credentials, so `/authorize` can be checked to redirect with the S256 challenge. The Google leg is never contacted.</li></ul> |
| `/auth/v1` proxy | Owned loopback | Maps `/auth/v1/*` onto the GoTrue root. The host needs it: `SUPABASE_URL` is a root, and the host appends `/auth/v1/admin/users/{id}` for the directory and for erasure. The browser `TransgateConfig.authUrl` is `P5_AUTH_URL` (p22: the client appends `/authorize`, `/token`, `/otp`, `/recover`, `/user` and `/logout` to the URL it is given) |
| Mailpit | `public.ecr.aws/supabase/mailpit:v1.31.3` (cached) | Receives SMTP from GoTrue. Specs read the links from its HTTP API at `P5_MAILPIT_URL` |
| Providers | The P4 fixtures, imported read-only from `scripts/p4-ai/smoke-transgate.py` | <ul><li>R2 SigV4 storage: an app-origin-only bucket CORS rule, preflight for GET, PUT and HEAD, and single-range `206` responses for media playback.</li><li>Soniox, with a default plan for browser uploads and automatic signed callback delivery.</li><li>Controlled DeepSeek: D1 request checks, plus plans keyed by marker and mode.</li><li>Lemon Squeezy, product mail, PostHog and the legacy bucket.</li><li>Any unplanned call fails the run.</li></ul> |
| Spec control | `P5_FIXTURE_URL/__p5/*` | <ul><li>`POST llm-plan` with `{marker, mode, steps}`, where a step is `delta`, `hold <name>` or `finish`.</li><li>`POST release/<hold>`.</li><li>`POST soniox-default` with `{text, duration_ms}`.</li><li>`GET calls` returns the LLM calls, mail, analytics and fixture failures.</li></ul> |
| Host | Immutable `transgate-server` and `billing-provision` copies | The P4 env (the AI flags plus a freemium chat cap of 2), with the real GoTrue directory through the proxy. `--web-dir` sets p1T's published `TRANSGATE_WEB_DIR`, `TRANSGATE_WEB_SITE_URL` (the loopback app origin) and `TRANSGATE_WEB_AUTH_URL` (`P5_AUTH_URL`). `apiUrl` is fixed at the same-origin `/api`. The optional `TRANSGATE_WEB_AUTH_KEY`, Turnstile and all-or-none PostHog/Sentry variables stay unset unless a spec sets them with `--host-env`. `TRANSGATE_FEATURE_FLAGS` is absent, so core defaults apply. The CSP storage origin comes from `MEDIA_R2_ENDPOINT`. |
| Browser | `@playwright/test` `1.64`, cached `chromium-1248` | Request interception allows only the owned loopback origins. The Lemon checkout URL `https://checkout.example.test/s` is intercepted, never fetched.<br>**YouTube.** Lead decision, 2026-10-25: keep the documented source behaviour, with no new consent gate and no switch to the nocookie origin in P5. The source (`YouTubePlayerLayoutContainer.tsx:51`, `MobileYouTubePlayerLayoutContainer.tsx:48`, `YouTubeMount.tsx:27,97`) mounts the embed with no click and no consent as soon as a YouTube-sourced transcript renders with a `videoId`. D12 consent covers only PostHog and Sentry. Opening a YouTube transcript is therefore a **recorded third-party contact**, and P6 privacy or cookie copy must not claim that every third party is blocked until consent. In C2/C3 no request reaches YouTube:<ul><li>Playwright serves `https://www.youtube.com/iframe_api` from a controlled loopback fixture that implements the documented IFrame API surface the adapter calls (`YT.Player`, `onReady`, `onStateChange`, `playVideo`, `pauseVideo`, `seekTo`, `getCurrentTime`, `getDuration`), driven by the spec.</li><li>The embed document request is answered with a blank local page.</li><li>Every other `youtube.com` request is aborted and recorded.</li></ul>Assertions:<ul><li>no `youtube.com` request before a YouTube-sourced job is opened;</li><li>exactly one `iframe_api` request per page;</li><li>one `[data-testid=youtube-player]` iframe per mounted layout;</li><li>the transcript follows the fixture-reported time.</li></ul>This is fixture evidence: it never claims live playback, which stays a named live limit.<br>**Storage origin.** The signed PUT (`connect-src`) and the streaming GET (`media-src`) go to `P5_FIXTURE_URL` in C2. p1T's CSP must derive that origin from the configured storage endpoint, never a wildcard. C2 records every CSP violation.<br>**Planned test ids** (the final list is in p1Z's `workspace-transcript.md`): `player-waveform`, `player-time`, `player-toggle`, `youtube-player`, `transcript-line-{id}[data-active]` |

Exported to the command: `P5_APP_URL`, `P5_AUTH_URL`, `P5_MAILPIT_URL`, `P5_FIXTURE_URL`, `P5_ADMIN_EMAILS`, `P5_EVIDENCE_DIR`, `P5_PG_CONTAINER`, `P5_PG_DATABASE`, `P5_RUN`. The evidence directory keeps:
- the host log;
- the last 300 lines of each container log;
- `fixture-trace.json`.

**Browser-visible keys** (from p22): `localStorage["transgate.session"]` (also the P6 presence hint), `localStorage["transgate.session.code-verifier"]` (`<verifier>` or `<verifier>/PASSWORD_RECOVERY`), and the `navigator.locks` name `transgate.session`. Other tabs adopt a rotated session through the `storage` event. The theme is stored in `localStorage["theme"]`.

## 3. Ownership of test files

| Path | Owner |
|---|---|
| `transgate/web/playwright.config.ts`, `transgate/web/e2e/**` (C2/C3 specs and the C1 Storybook capture spec), `scripts/p5-web/**` | p1Y |
| `transgate/web/package.json`: devDependency `@playwright/test` `1.64`, script `e2e` | p1V (both present) |
| `transgate/web/package-lock.json` | Generated by `npm install` only in the authorized integration window, then `npm ci` everywhere after |

The web runtime dependencies (p1V, as of 2026-10-25) are `@lucide/svelte ^0.539.0`, `@sentry/browser ^10.39.0`, `i18next ^24.2.3`, `paneforge 1.0.2` (exact; its Svelte 5 peer requirement is `^5.29`), `posthog-js ^1.352.0` and `wavesurfer.js ^7.8.4` (p1Z). The PostHog and Sentry SDKs are dynamically imported chunks that load only after cookie consent. C2 asserts, on both projects, that no telemetry chunk or telemetry-origin request happens before consent, that declining keeps them absent, and that accepting loads each one once (D12). It does this with loopback or blocked telemetry endpoints and makes no live provider calls.

Locators follow `web-shell.md` §3: role plus accessible name taken from the `en` catalog, or the same key's translated name in another locale. `data-testid` is used only where an element has no role, and only as listed in the owning view's handoff. Branch and theme are asserted through `data-layout` on the mounted branch root and `data-theme` on `<html>`.

## 4. Package registration (announced so far)

A crate joins the root members only when its manifest and its source root both exist and agree with the announcement below. There is no partial member.

| Crate | Path / lib | Dependencies announced | wasm32 | Compile-time non-`.rs` inputs |
|---|---|---|---|---|
| `transgate-client` | `transgate/client` (source-coherent per p22; registered together with `transgate-wasm` on p22's single COHERENT message) | <ul><li>`base64 0.22`;</li><li>`media-ports` and `transgate-core` (path);</li><li>`reqwest 0.13`, `default-features=false`, `[json, stream]`, plus `rustls` on native only;</li><li>`serde`, `serde_json` (workspace);</li><li>`sha2 0.10`, `default-features=false`;</li><li>`url 2.5`;</li><li>`uuid 1`, `default-features=false`, `[serde]`;</li><li>`getrandom 0.3`, native only.</li></ul>Dev: `axum 0.8`; `tokio 1` with `[macros, rt-multi-thread, net, sync]`. Tests: `tests/gotrue.rs` and `tests/api.rs` | Yes. `media-ports` is checked statically: it uses only `bytes`, `futures-core`, serde and `uuid[serde]`, with no tokio, fs, net, clock or RNG. It has no RNG on wasm | None |
| `transgate-wasm` | `transgate/wasm`, lib `transgate_wasm`, `["cdylib","rlib"]` | <ul><li>`wasm-bindgen =0.2.128`;</li><li>`wasm-bindgen-futures 0.4`;</li><li>`js-sys 0.3`;</li><li>`web-sys 0.3`, features `[Window, Storage, StorageEvent, Crypto, Location, History, Navigator, console]`;</li><li>`serde-wasm-bindgen 0.6`;</li><li>`serde`, `serde_json`;</li><li>wasm32-only `uuid 1` with `[js]`.</li></ul>No `ts-rs` and no exporter. `scripts/build-transgate-wasm.sh` runs only the wasm build and wasm-bindgen | It is the wasm target | None. The generated `transgate/web/src/lib/core/types/*.ts` and `fixtures.json` come only from core: `cargo test -p transgate-core --features ts --lib presentation::fixtures::web::generate` (p1X), which writes to an explicit path, never `TS_RS_EXPORT_DIR` |
| document adapter (p1W) | In or beside `transgate/export` | <ul><li>`krilla 0.8.2`, `default-features=false`, plus `raster-images` (invoice logo);</li><li>`rustybuzz 0.20.1`, `default-features=false`, `[std]`;</li><li>`unicode-bidi 0.3.18`, `default-features=false`, `[std, hardcoded-data]`;</li><li>`unicode-linebreak 0.1.5`;</li><li>`unicode-segmentation 1.12.0`.</li></ul>Transitives: `subsetter 0.2.6` `[variable-fonts]`, `pdf-writer 0.15.0`, `skrifa 0.42.0` | Pure Rust, unproved until the first build | Fonts are supplied through a port and are not `include_bytes!` in the adapter. Only the dev regression test embeds them. Files: `transgate/export/fonts/{NotoSans.ttf, NotoSans-Italic.ttf, NotoSansArabic.ttf, NotoSansSymbols2-Regular.ttf}`, `*-OFL.txt`, `provenance.json`, and `transgate/export/assets/invoice-logo.jpg` |

**Registered `transgate-server` additions (p1T coherent handoff; resolution evidence in §5b):**
- **Dependencies:** direct native `getrandom 0.3`, used for a 32-byte OS nonce with no fallback, and `lol_html 2`, which rewrites the HTML script nonce. No `base64` (the nonce is hex) and no extra tower features.
- **Files the checkout renderer `transgate/server/src/web/checkout.rs` embeds with `include_str!`:**
  - `transgate/web/checkout/{document,failure}.html`;
  - `transgate/web/src/tokens.css`;
  - `transgate/web/locales/{en,tr,pt,es,de,id,it,fr,el,ru,da,sv,no,fi,kk,az}/common.json`.

  There is no `build.rs`. The Docker allow-list already covers every one of these paths (`checkout/*.html`, `src/**/*.css`, `locales/*/*.json`).

Open before the lock window:
1. **uuid on wasm32. Lead decision, proposal sent to p22 and p1X:**
   - **Locked crate behaviour:** `Cargo.lock` holds `uuid 1.27.0`. On `wasm32-unknown-unknown` it does not use getrandom; its `getrandom`/`rand` dependencies are for non-wasm targets only. It raises `compile_error!` (`src/rng.rs:104`) unless `js`, `rng-getrandom` or `rng-rand` is set. The `js` feature calls `globalThis.crypto.getRandomValues` through wasm-bindgen (`src/rng.rs:210-304`), which is a cryptographically secure source.
   - **Manifest change:** only in `transgate/wasm/Cargo.toml` (p22), using the existing target-specific convention (`taskr-client`, `llm-http`): `[target.'cfg(all(target_arch = "wasm32", target_os = "unknown"))'.dependencies] uuid = { version = "1", default-features = false, features = ["js"] }`. Feature unification then covers `transgate-core`'s v4.
   - **Unchanged:** no change to `taskr-core` or `transgate-core`, no `RUSTFLAGS` and no custom fallback.
   - **PKCE randomness:** comes from web-sys `Crypto.getRandomValues` in `transgate-wasm`. `transgate-client` uses `uuid` for serde only and has no RNG on wasm; its `getrandom 0.3` is native only.
   - **Confirmed by p22:** it adds exactly this target block when the `transgate/wasm` crate root is coherent, and sends the full manifest then.
   - **Confirmed by p1X:** core's only randomness is `uuid` v4, used for request keys from the app composer and for existing ids. Clocks are injected (`Clock`, `view(now)`). There is no `SystemTime` or getrandom anywhere else in core. A future ts export would be an optional `ts-rs` feature enabled only by `transgate-wasm`, announced before it is added.
   - **Residual risk:** another dependency in the wasm graph that uses getrandom directly. Only the first authorized wasm build proves there is none.
2. **Font delivery to the browser.** Resolved: single-copy Vite `?url` imports; see §5.
3. **p1T host contract.** The final source-coherent handoff is published in `hosts.md`; runtime acceptance is still pending.
   - **Static serving:** `TRANSGATE_WEB_DIR`. The only SPA allow-list is core's `presentation::route::is_app_path`: `/home`, `/home/{i64}`, `/home/all-transcripts`, `/signin`, `/new-signin`, `/new-password`, `/billing` and `/checkout/confirm`, each also under a recognized locale prefix, including an explicit `/en`. `/`, bare locale roots, `/checkout`, `/dashboard/*`, assets and unknown paths are not served the SPA. `/api/*` returns a JSON 404. C3 asserts that every listed path returns the index and that every unlisted path does not.
   - **Runtime config:** injected at serve time, never baked into the build. The `<!--tg-inject-->` marker is replaced with the `tg-config` script, holding `apiUrl`, `authUrl`, a nullable `authKey`, `siteUrl`, and nullable Turnstile, PostHog, Sentry and `checkoutNonce` values. It carries no flags and no secrets. Inside the JSON script bodies, `<`, `>`, `&`, U+2028 and U+2029 are escaped. C2 parses the script and asserts it has exactly these keys and no secret-shaped value. The env-to-config mapping has not been published yet.
   - **Caching:** hashed assets are cached as immutable. The index page and the theme initializer are `no-cache`. `.wasm` files are served as `application/wasm`. C3 asserts these headers.
   - **D20, app assets under `/_app/*`** (lead decision): the Vite output, the app-owned fonts and logo, and the theme bootstrap move in one clean cutover. There is no `/assets` alias and no catch-all ownership; the source's public `/assets/*` (blog, OG, certificate and logo URLs) stays with P6. Native asset files and taskr are unchanged. Checks:
     - **S1 gate:** static checks of the current host and web source. It fails on any app-owned `/assets` route, Vite `assetsDir` or reference, and requires `/_app` in both the host route and the Vite config.
     - **C3:** checks the D20 output tree p1V published.
       - **Layout:** `dist/index.html`, served only at app routes, plus a flat `dist/_app/<name>-<hash>.<ext>` holding the JS and CSS chunks, `transgate_wasm_bg-<hash>.wasm`, the fonts, the OFL and COPYRIGHT notices, the invoice logo and the rating SVGs.
       - **Caching:** every hashed file is `immutable`. The one unhashed file, `/_app/theme-init.js`, is `no-cache`.
       - **Content types:** `.wasm` `application/wasm`, `.ttf` `font/ttf`, `.otf` `font/otf`, `.txt` `text/plain; charset=utf-8`, `.svg` `image/svg+xml`, `.jpg` `image/jpeg`.
       - **Not served:** `assetsInlineLimit 0` means no `data:` inlining. The app returns 404 for `/assets/*`. Built `index.html` and JS contain no `/assets/` URL. The checkout templates are not in `dist`.
   - **CSP:** the ordinary-app baseline is `script-src 'self' 'wasm-unsafe-eval'`, `style-src 'self'`, same-origin `connect`, `media`, `img` and `font` sources with `data:` and `blob:` where needed, and `frame-ancestors 'none'`. Its extensions name GoTrue, captcha, telemetry and YouTube origins individually. Storage `connect`/`media` origins derive from the configured endpoint or public bucket URL; loopback `MEDIA_R2_ENDPOINT` needs no extra origin variable. Checkout instead uses the distinct D19 enforced nonce/`strict-dynamic` policy in §5d, explicitly trusting arbitrary HTTPS issuer/provider destinations rather than an origin allow-list. Unexpected CSP violations fail C2/C3; deliberate negative-test blocks are recorded as expected evidence, never hidden.
   - **Invoice-preview delta (p1T, source-inspected only):** ordinary app and confirmation `frame-src` additionally permits `blob:` for the root-owned PDF preview iframe. This does not add Blob scripts: `script-src` has no `blob:`, and `object-src` remains `'none'`. D19 checkout policy is unchanged. `web.rs` and the updated CSP regression in `web/tests.rs` must be included in the eventual S1 source manifest; neither has been exercised by this inspection.
   - **`POST /api/jobs/start`:** session only. It reuses the current JobIds parser and `Jobs.start` and returns 201 with the existing `start_response` envelope. It rejects `audioUrls`. The old session `/api/v1/transcriptions` is removed at release.
   - **Invoices:** `GET /api/billing/invoices` returns `InvoiceContext{bill_to, invoices}`, or 401 or 410. The admin lookup adds `invoice_context` and returns 403 for non-admins (`billing.md`).
   - **Checkout confirm:** only a form POST that sends `Accept: text/html` gets HTML, built from a single confirm call. Every other request gets the existing JSON. The `tg-confirm` payload is either `{outcome:"confirmed", view: ConfirmView}` or `{outcome:"failed", status, code, message}`, with the same HTTP status as the JSON response. C2 asserts that the HTML and JSON outcomes match for the same confirmation, and that reloading never confirms again.
   - **DTOs and public configuration:** `hosts.md` is the published source boundary for bootstrap, feedback, support, receipt and exact environment mapping. These are source-coherent, not HTTP acceptance claims. The launcher only receives host variables through `--host-env`.
4. **Tokens.** The generator and `--check` ran with lead authorization. See `/tmp/rustr-p5-tokens-1792942460`:
   - **Result:** pass.
   - **New files:** `transgate/web/src/tokens.css` and `transgate/web/src/lib/theme/palettes.ts`.
   - **Taskr changes:** additive only, in the CSS, Swift and TUI outputs.
   - **Contrast minimums:** success 5.18, progress 5.74.

Resolved (p24): catalogs are flat maps at `transgate/web/locales/<code>/<ns>.json`, 16 source codes. The initial reachable namespaces are `common`, `app`, `ai`, `dashboard` and `auth`. Root metadata lives in `config.json`, `reachable-keys.json`, `fallback-manifest.json` and `source-provenance.json`. The manifest's `active` list is empty under D3, so C2 asserts that a reachable key never falls back at runtime in any locale. The baseline is evidence, not permitted fallback. No Rust crate reads catalogs at compile time. The validators live in `scripts/p5-locales/` and are run by the lead or me at an authorized window.

The document contract is in `documents.md` (p1W). Production fonts and the logo are supplied through a port; only tests use `include_bytes!`. Actual wasm compile, Arabic rendering and independent-reader proof remain unverified.

## 5. Docker: proposed edits, not applied

Lead decision (2026-10-25):
- Network is allowed only in controlled build stages, for official pinned toolchains and packages.
- The build is reproducible from source: no opaque wasm artifacts, `cargo --locked`, `npm ci` against the committed lock.
- Fonts are versioned local inputs with license, source and checksum.
- No runtime downloads or secrets.
- The secret and path exclusions are kept.

Resolved Node image, an official Node 24 LTS (`NODE_VERSION=24.22.1` on linux/amd64, resolved 2026-10-25 with `docker buildx imagetools inspect`):
`node:24-bookworm-slim@sha256:55fb5f5a527ea2c5c4a95f963e48ef90aa38c7b346054fdc8ee9957a2b55d205` (index digest). Acquired for the authorized PRE-S1 dependency window; actual containers report Node `v24.22.1`, npm `11.19.1`, linux/x64. This is toolchain/dependency preparation, not a product image build.

Proposed `transgate/Dockerfile`:

```dockerfile
FROM rust:1.99.1-bookworm AS toolchain
# Network: official rustup target and crates.io wasm-bindgen-cli, exact pin = Cargo.lock's wasm-bindgen.
RUN rustup target add wasm32-unknown-unknown \
    && cargo install --locked wasm-bindgen-cli --version =0.2.128
WORKDIR /build
COPY . ./

FROM toolchain AS server
RUN cargo build --locked --release --package transgate-server

FROM toolchain AS wasm
RUN cargo build --locked --release --package transgate-wasm --target wasm32-unknown-unknown \
    && wasm-bindgen --target web --out-dir /pkg target/wasm32-unknown-unknown/release/transgate_wasm.wasm

FROM node:24-bookworm-slim@sha256:55fb5f5a527ea2c5c4a95f963e48ef90aa38c7b346054fdc8ee9957a2b55d205 AS web
WORKDIR /build/transgate/web
COPY transgate/web/package.json transgate/web/package-lock.json ./
RUN npm ci
WORKDIR /build
COPY transgate/web transgate/web
COPY transgate/export/fonts transgate/export/fonts
COPY transgate/export/assets transgate/export/assets
COPY --from=wasm /pkg transgate/web/src/lib/core/pkg
RUN cd transgate/web && npm run build

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends libgcc-s1 curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=server /build/target/release/transgate-server /usr/local/bin/transgate-server
COPY --from=web /build/transgate/web/dist /srv/transgate-web
USER 65532:65532
ENV TRANSGATE_BIND=0.0.0.0:8788
ENV TRANSGATE_WEB_DIR=/srv/transgate-web
EXPOSE 8788
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["curl", "--fail", "--silent", "--show-error", "--max-time", "2", "http://127.0.0.1:8788/health"]
ENTRYPOINT ["/usr/local/bin/transgate-server"]
```

The generated TS types and `fixtures.json` are committed outputs; the image consumes them and does not regenerate them. Their drift check (rerun the core generator, then diff) is a separate gate.

**Confirmed (p1W, p1V, p1Y):** the assets ship as single-copy Vite `?url` imports.

- **Imports:** `transgate/web/src/lib/documents/assets.ts` (p1V; URLs only) imports exactly eight files. From `transgate/export/fonts/`: `NotoSans.ttf`, `NotoSans-Italic.ttf`, `NotoSansArabic.ttf`, `NotoSansSymbols2-Regular.ttf`, `NotoSans-OFL.txt`, `NotoSansArabic-OFL.txt` and `NotoSansSymbols2-OFL.txt`. From `transgate/export/assets/`: `invoice-logo.jpg`.
- **Output:** Vite emits them hashed into `dist/_app` (D20), with the licenses next to the fonts. There is no duplicate copy under `transgate/web/public`.
- **Dev serving:** `vite.config.ts` sets `server.fs.allow` to the workspace root and `../export`.
- **Byte fetching:** the export call site does this, not `assets.ts` (p22's font port, used by p21's and p1R's document views).
- **`provenance.json`:** the bundle does not import it. It stays in the build context as the license record, but does not reach the runtime image (only `dist` does). C3 reads it from the repository to check the served bytes.
- **Tests:** `transgate/export/tests/documents.rs` uses `include_bytes!("../fonts/…")`, which only native test builds compile. The image builds no tests, and the context excludes `tests/**`.
- **C3 checks:** the hashes of the served font and logo bytes match `transgate/export/fonts/provenance.json` and `transgate/export/assets/provenance.json`, and each font has its OFL notice served.

**Accepted by the lead:** the pinned Node proposal. The image build runs only after a stale-check gate passes, all in the authorized window:

1. Source, manifest and lock coherence: `cargo metadata --locked`, `npm ci --dry-run` against the committed lock, or `npm ci` in the window.
2. `python3 -B scripts/gen-tokens.py --check`.
3. The generated TS types and `fixtures.json` from the core generator (`presentation::fixtures::web::generate`), compared with the committed files.
4. The p24 locale validator.
5. Font and logo bytes against both `provenance.json` files.

Any stale output fails the gate. The image is never built from stale generated contracts.

Proposed additions to both identical ignore files (`transgate/Dockerfile.dockerignore` and `transgate/processor/deploy/Dockerfile.dockerignore`). Re-includes are file patterns only:

```gitignore
!transgate/export/fonts/*.ttf
!transgate/export/fonts/*.otf
!transgate/export/fonts/*-OFL.txt
!transgate/export/fonts/*-COPYRIGHT.txt
!transgate/export/fonts/provenance.json
!transgate/export/assets/invoice-logo.jpg
!transgate/export/assets/provenance.json
# compile-time inputs of transgate-server (p1T include_str!, p1V files): the HTML checkout pages and their style source
!transgate/web/checkout/document.html
!transgate/web/checkout/failure.html
!transgate/web/package.json
!transgate/web/package-lock.json
!transgate/web/index.html
!transgate/web/public/_app/theme-init.js
!transgate/web/src/**/*.svg
!transgate/web/vite.config.ts
!transgate/web/svelte.config.js
!transgate/web/tsconfig*.json
!transgate/web/src/**/*.svelte
!transgate/web/src/**/*.ts
!transgate/web/src/**/*.css
!transgate/web/src/**/*.json
!transgate/web/locales/*.json
!transgate/web/locales/*/*.json
# never from the host: generated wasm glue is rebuilt in the image; tests and stories are not shipped
transgate/web/src/lib/core/pkg/**
transgate/web/**/*.test.ts
transgate/web/**/*.stories.ts
transgate/web/**/*.spec.ts
```

The existing `.env`, `.pem` and `.key` exclusions stay last, so they still win over the new re-includes.

- **Exact list finalized against the real files.** The final web file list (for example `public/**`, a `postcss`/Tailwind config, a static font directory) is taken from the files once p1V's package exists.
- **Canary export proves it.** The list is then proved with the P3 method, a `docker buildx build --output type=tar` canary export: required files present, forbidden files absent (`node_modules`, `dist`, `pkg`, `.env*`, keys, tests, stories, `e2e`).
- **Processor image.** The processor image shares the ignore file, so its context also gains these few web files. It does not use them. Keeping the two files identical is the P3 invariant.

## 5a. First coherent integration snapshot (S1) and the P4 re-proof

Fable's P4 final review (`docs/plans/p4-ai/fable/final-review.md`, finding 1, blocking) asks for fresh actual-process and binding proof on the post-cleanup tree. The run5 and shell proofs ran on binaries built before the parser-`finish`, private-error and Clippy cleanup. Run5 is retained as historical evidence. Binary 791 is not a P4 handoff and is never used as one.

S1 is the first snapshot built after the root members and the lock are coherent. It runs only when the lead authorizes it, and nothing is built mid-flight. Steps:

**Recovered readiness, 2026-10-26:** W, T, R, X and p22 source-coherent handoffs are received. R's billing TS/fixtures/Msg contracts and portable D17 constants are published. p22 consumes the actual core invoice epoch/request-ticket predicate and preserves the original capture; all 12 export consumer regressions passed on the second S1 candidate, including same-payment close/reopen fencing. p24's final billing meanings are present in all 16 locales. X's generator and the resulting contract passed in run5 after the concrete repairs below. Rust source is held for versioned S1 capture.

**Prescribed gates:** the digest-pinned Node24 toolchain, clean-install-validated web lock and inspected generated contract are published. p24's validator runs offline in the pinned Node image during the S1 stale gate. The exporter is rerun in a separate source copy and compared with the snapshot, so a stale result cannot overwrite frozen evidence. Matching output and deliberate drift were exercised with synthetic Cargo/Docker commands at `/tmp/rustr-s1-stale-gate-3_jl9shn`: only matching output creates the pass stamp; both cases leave the source unchanged. This is harness-only proof, not an executed S1 gate.

**Runner-only verification:** `/tmp/rustr-s1-driver-boundary-49hppnsg/report.json` exercises failure propagation with throwaway command shims. The blocked gate and unapproved build return 78; injected process failure 47 is preserved by each Transgate, taskr-browser and Swift phase. This is harness evidence only: no Cargo check/build, product, database, container or browser ran. Historical run5 artifacts and the accepted token/D2 evidence are untouched.

**First S1 candidate, preserved failed gate:** `/home/mu/.cache/rustr-integration/s1-20261026142204`, source-manifest SHA256 `66bd624b35bfb76ff3048771f5f3297fe33ea00e5387b6c1568e2ca8775b7add`. Disk-backed storage avoids this workstation's 16 GiB `/tmp` tmpfs. Captured generator inputs and outputs match run5. The gate completed in 65.25 s and failed only the locale validator: `pt/common` keys `10`, `11`, `12`, `18`, `20`, `21` violate its nonempty-string rule. p24 owns the working-source repair; the failed snapshot is not changed.
- Observed on that snapshot: locked metadata and clean Node24 `npm ci` exited 0; token stale/contrast check passed; 24 font/license hashes and the invoice-logo hash matched; the D20 static check passed; the cloned exporter passed all three selected tests with no generated TS/fixture drift; frozen source hashes remained unchanged.
- There is no `gate.passed` stamp and no whole-host build. Evidence is `logs/gate-report.json`, `locales.log`, `generated-contract.log` and the zero-drift files. Both owned Node containers were removed.
- A subsequent user-requested publisher-boundary regression changes X's exporter test source. Its new selector, `presentation::fixtures::web::export_integrity_preserves_outputs_on_rejection`, is outside the existing `generate` filter and awaits explicit bounded execution approval. A fresh candidate follows its coherent closure and the locale repair, not a mutation of this one.

**Repaired S1 candidate:** `/home/mu/.cache/rustr-integration/s1-20261026144505`, source-manifest SHA256 `8443068d5c2da54efae16f3945cacf5241c5959caf35fd43905830d1e5156c2e`. p24's 25 changed payload/source hashes and the unchanged validator/importer hashes were verified before capture. This candidate also contains X's coherent shared-publication helper/regression source; generated types and fixtures still match the published run5 output.
- Actual stale gate **passed** in 64.69 s: all 16 locales, `issues: []`; token/output contrast check; 25 asset hashes; locked metadata; pinned Node24 clean install; D20 static check; cloned exporter 3 passed with zero TS/fixture drift; source integrity unchanged. Receipt: `logs/gate-report.json`.
- Selected host/taskr build **passed** in 214.24 s, followed by a successful full source/lock hash check. Evidence: `logs/build-report.json`, `native-build.jsonl`, `build.log`. Native/FFI commands used `--locked`; the historical taskr wrapper's internal Cargo calls did not, but the captured lock remained unchanged. The working wrapper now adds `--locked` for the next candidate.
- AppState selector **23 passed**. Client compilation passed; **44 tests passed, one failed**: `pkce_challenge_matches_rfc7636_appendix_b`, `tests/gotrue.rs:227`. The captured test's verifier and expected digest were both mistranscribed. All 12 export consumer cases passed. Client doc tests were not reached.
- Transgate wasm **check and release/glue build passed**, using `RUSTC_BOOTSTRAP=1`, `-Zbuild-std=std,panic_abort` and the repository's `TASKR_WASM_BUILD_STD=1` script. No getrandom override or toolchain substitution. Compiled wasm SHA256 `00da14d5ea81f17754489717ccfd2d5a6d21a1744c92c774af5197af66aea2ae`. This proves compilation, not browser execution.
- The native throwaway executable linked the exact host-built core/billing rlibs and **passed** confirmation succeeded/declined/unconfirmed, invoice capture and same-payment selection fencing, verbatim host alert/reset, and display/charge separation. `composed-smoke/report.json` records harness/library/binary hashes. This is pure composed-state evidence, not HTTP/browser/PDF-rendering/provider proof.
- `logs/p5-report.json` records the separate results; post-P5 full source integrity passed. No P4 process/browser/Swift runs occurred on this failed candidate.
- The separate publication-boundary regression was not executed by the `generate` filter (383 tests filtered out). The lead has now explicitly authorized its exact standalone selector on the next coherent candidate.

**Next candidate repair:** p22 closed the working-source window with RFC 7636 Appendix B's normative verifier/challenge and removal of one unused import. Integration independently read the [normative source](https://www.rfc-editor.org/rfc/rfc7636.html#appendix-B), checked the exact two-file delta and all three handoff hashes; no production auth change or observed-digest repinning. Receipt: `/tmp/rustr-s1-pkce-repair-810b8jnb/source-handoff.json`. The new candidate also includes X's shared publication-path regression, p24's locale repair, locked taskr Cargo calls and fail-closed artifact staging that hashes/read-protects the actual served wasm packages as well as their archives. Prior failed snapshots remain untouched.

**PAUSE-RETRO-1 safe handoff:** candidate3 `/home/mu/.cache/rustr-integration/s1-20261026151947` was captured before the pause, source-manifest SHA256 `daad27b809bfe082f836f0cc3aca95e73085aef56a1dfc20fa0d25fad635e037`. Static comparison with candidate2 found only the two published p22 Rust changes; Cargo inputs were unchanged. The gate passed in 65.64 s and native host build JSON reports success. At the pause, `cargo build --locked -p taskr-ffi --features cli` was already running; it finished successfully in 44.78 s. The parent/controller were stopped first, so no taskr wasm or next test phase began; after the child exited, both were terminated without resuming. Wrapper exit 148 is the intentional pause, not a product failure. `logs/user-pause.json` and `integration-sequence.json` preserve the boundary. Cleanup found no remaining controller/child PID or labeled Node container.

**Remaining, authorized-but-paused until explicit resume:** complete candidate3's taskr wasm build/type-drift portion without repeating its passed native/FFI builds; run the exact standalone publication-integrity regression, AppState selector, repaired client suite, Transgate wasm check/release and composed-state executable on that candidate; stage/hash bindings and actual served packages; then the P4 Transgate scenarios, taskr browser 4/4 and generated Linux Swift listener; publish the consolidated evidence/cleanup report. The standalone regression needs no further approval request. C1/C2/C3 remain separately unauthorized. Saved smoke sources `/tmp/rustr-s1-composed-state-smoke.rs` and `/tmp/rustr-s1-composed-state-driver.py`, all earlier failed snapshots and run5 publication remain intact.

**NEXT-S1-C3 actual completion, 2026-10-26:** lead w23:p2B released only the remaining checks. Candidate3 manifest remained `daad27b809bfe082f836f0cc3aca95e73085aef56a1dfc20fa0d25fad635e037`; final full source and staged-artifact checksum checks both exited 0.
- Taskr wasm build passed, with no generated-type drift (64.74 s). Prior successful native/FFI builds were retained, not repeated.
- Exact export-integrity regression: **1 passed**; AppState: **23 passed**; repaired client: **45 passed**, including the normative RFC PKCE vector and all 12 export-consumer cases. Doc tests completed with zero cases.
- Transgate wasm check and release/glue passed using the established Arch build-std invocation; wasm SHA256 `00da14d5ea81f17754489717ccfd2d5a6d21a1744c92c774af5197af66aea2ae`.
- Actual composed-state executable passed confirmation outcomes, captured invoice/request-ticket fencing, verbatim host text/reset and display/charge separation. `composed-smoke/report.json` identifies the actual libraries and executable. This remains pure state evidence, not HTTP/browser/rendered-PDF proof.
- Staged binaries, generated Swift bindings, archives and actual served wasm package files are hashed and read-only (18 files, `artifacts.sha256`).
- P4 Transgate actual-process scenarios passed (172.96 s); `fixture-trace.json` has `failures: []`. Only the documented start-endpoint migration differs from the preserved historical harness. **Evidence warning:** post-cleanup container-log retrieval reported “No such container”; product scenarios passed, but that container log was not captured. The preserved log reports no capability-wait closure, PG-only quota reconciliation or live-provider acceptance.
- Taskr actual wasm Playwright: **4/4 passed**, desktop and phone (9.3 s tests; 17.19 s wrapper), covering progressive/final replacement, detach/reconnect/reload, cancellation and session loss. This is the existing P4 scope, not P5 C1/C2/C3 or a new visual/a11y audit.
- Generated Linux Swift listener passed on Swift 6.2.4 (11.40 s wrapper), reporting progressive-before-final, authoritative replacement, detach/resume, canonical reload, cancellation, session loss and one dispatch per start.
- Shell/Swift forbidden-origin sentinels stayed empty. Owned container label inventories were empty after teardown; wrappers and build jobs completed. No new agent, live provider or Telegram send occurred.
- Consolidated receipt: `logs/next-s1-c3/checks.json`; detailed process/browser/Swift logs remain under `logs/`. Lead owns acceptance and any next authorization.

**Repair routing:** new billing implementation diagnostics go to the lead for an explicit OMP-writer handoff. R's native session is read-only billing consultation; it is not assigned new source repairs.

1. **Tree identity.** Record `git rev-parse HEAD`, plus a sorted sha256 manifest of every tracked and untracked source file passed to the build (Cargo manifests, `Cargo.lock`, `*.rs`, `*.sql`, the web package and lock, generated TS and fixtures, fonts and assets). Uncommitted trees are identified by this manifest.
2. **Stale gate** (§5): run it before any build.
3. **Immutable copies.** Build these binaries (lead-run, or by me if authorized):
   - `transgate-server` and `billing-provision` (debug, matching the run5 harness);
   - the taskr wasm package (`scripts/build-web.sh`);
   - taskr ffi and the Swift bindings (`scripts/gen-bindings.sh`).

   Copy binaries into `$S/bin`, with `$S/root/target/debug -> ../../bin`; record their SHA256 and freeze the wasm `pkg` as a tar copy. `$S` is the disk-backed snapshot path printed by `scripts/p5-web/s1.sh freeze`.
4. **P4 re-proof, on S1 before any P5 checkpoint.** Labeled "P4 post-cleanup re-proof on S1 (tree also contains P5 source at manifest `<sha>`)":
   - From `$S/src`: `systemd-inhibit --what=sleep:idle python3 scripts/p4-ai/smoke-transgate.py --root "$S/root" --postgres-image postgres:17-alpine`. Every run5 scenario and assertion is kept. Pass means every scenario passes, fixture failures are `[]`, and there are no product findings.
     - **Recorded S1 difference:** the P5 session cutover changes only the start endpoint. `smoke-transgate.py:794` now calls `POST /api/jobs/start`, which used to be `/api/v1/transcriptions`, and the liveness 404 list at `smoke-transgate-runtime.py:42` changed the same way. There is no compatibility route or shim, and the harness is **not** byte-identical to run5.
     - **Preserved originals:** the run5 harness is kept byte for byte, read-only, at `/tmp/rustr-p4-run5-harness/` (`smoke-transgate.py` sha256 `dd54069f…`, `smoke-transgate-runtime.py` `375616ce…`; both mtimes predate run5). The run5 logs at `/tmp/rustr-p4-transgate-run5/` are untouched.
   - From `$S/src`: `scripts/p4-shell/runtime.py --bin-dir "$S/bin"`, followed by `playwright.p4.config.ts`, which must pass 4/4 (desktop and mobile), and the generated Linux Swift listener in the pinned Swift image. The exact wrappers are `scripts/p5-web/s1.sh shell "$S"` and `swift "$S"`.
   - The report gives binary hashes, durations, logs, and the fixture-versus-product classification. A failure is preserved, never rerun until it passes.
5. **P5 checkpoints.** C1, C2 and C3 then run on the same S1 copies, each when the lead authorizes it.

Fable's finding 2 (the required real-PG race suites named in artifact 787) is the lead's citation, not part of this run.

## 5b. Executed so far (lead-authorized, bounded)

| Item | Result | Evidence |
|---|---|---|
| Token generation and `--check` | PASS. Success minimum contrast 5.18, progress 5.74 | `/tmp/rustr-p5-tokens-1792942460` |
| D2 consumer lost-COMMIT regression, `transgate-jobs-postgres --test ai ai_completes_once_when_the_completing_commit_answer_is_lost`, on an owned `postgres:17-alpine` | Compile exit 0, 0 warnings, 30.7 s. Behaviour: 1 passed, 0 failed, 1.51 s. Test binary sha256 `02e21a56…` | `/tmp/rustr-p5-d2-lostcommit/run1` |
| Root lock refresh (`cargo metadata --format-version 1`), after static checks of `transgate/export` (p1W coherent) and the `transgate/server` host deps (p1T) | Exit 0; 57 packages locked; `Cargo.lock` sha256 `b6597255…`. Resolved: krilla 0.8.2, pdf-writer 0.15.0, subsetter 0.2.6, skrifa 0.42.1, rustybuzz 0.20.1, lopdf 0.38.0 (dev, non-wasm), tower-http 0.6.11, tower 0.5.3, url 2.5.8, percent-encoding 2.3.2. **unicode-segmentation moved from 1.13.3 down to 1.12.0 across the workspace**, because export pins `=1.12.0`; this is with p1W. Static wasm note: `zip 9.1` already enables getrandom 0.4 `wasm_js` itself on wasm32 | `/tmp/rustr-p5-lock-1792990350` |

**PRE-S1 Node/npm dependency preparation:** `/tmp/rustr-p5-node24-lock-xwep1ocn` preserves frozen package manifests, toolchain identity and resolution evidence.
- **Attempt 1:** manifest `9518c7d3…`; `npm install --package-lock-only --ignore-scripts --no-audit --no-fund` exited 1, `ERESOLVE`: TypeScript `~6.0.3` conflicts with i18next `24.2.3`'s optional TypeScript `^5` peer. The terminal excerpt is preserved; the disposable container's internal debug log was not persisted.
- **Owner repair:** V changed only TypeScript to `~5.9.3`, manifest `977f23df…`.
- **Attempt 2:** fresh resolution exited 1, `ERESOLVE`: Storybook resolved `10.6.3`, while `@storybook/addon-a11y 10.6.4` requires Storybook `^10.6.4`. Full npm report/debug logs are retained under `attempt2/npm-cache/_logs/`.
- **Attempt 3, after V's coherent repair:** all five direct Storybook entries are exactly `10.6.2`; TypeScript remains `~5.9.3`; manifest SHA256 `52bf05e3c957304fd0026120f64a4b9202423a3a5f96cad0d40273e066f486dd`. Resolution exited 0 in 30.79 s. A separate clean `npm ci --no-audit --no-fund` exited 0 in 17.33 s, installing 231 packages.
- **Published output:** `transgate/web/package-lock.json`, SHA256 `10c9b625b8f7d13c3ba1baa448d76568ba9971d5f0bb0ad4f3cc7920af2dcbc8`, 354 entries including optional platform packages. `npm ci` did not change the lock, and the current manifest still matched the frozen input at publication. No `--force`, `--legacy-peer-deps` or peer overrides were used.
- **Install-script limit:** npm 11.19.1 withheld unapproved `core-js 3.51.0` and `esbuild 0.28.2` postinstall scripts. No approval or bypass was added. The installed esbuild executable returned version `0.28.2` successfully in a network-disabled container. This is dependency/tooling proof, not application build/type compatibility.
- Successful input/lock/install reports and npm logs are under `attempt3/`. No application build, Storybook, browser or S1 run occurred.

**PRE-S1 core TS/fixture generation:** exact selector `cargo test --locked -p transgate-core --features ts --lib presentation::fixtures::web::generate`. Each version has its own directory under `/tmp/rustr-p5-tsgen`; logs, input/output hash manifests and generated archives are retained. Captured core/billing/root inputs stayed unchanged during each run; taskr core outputs were untouched.
- **Lock preparation:** `cargo metadata --format-version 1` exited 0. The only added edge was `billing-core -> ts-rs`, with no new package/version. Lock SHA256 `6ff619f4388118a2e218dd00048c9e877c8d5af0df9a0f277ac27f55c6255bbb`; evidence `/tmp/rustr-p5-generation-lock-uyyhtc87`.
- **Run1:** compile exit 101, `E0502` in `presentation/ai/chat.rs:539`; no exporter test or generated output. p21 repaired the overlapping match-guard borrow without changing the replacement/append behavior.
- **Run2:** compile and exporter passed, 1 test, 142 TS files and 21 fixture roots, 35.29 s total. Inspection found `ActionItem.owner?: string | null` despite the actual omission-only wire contract. p23 added conditional `ts(optional_fields)` to the 17 relevant content structs, leaving runtime serialization and required nullable progress untouched.
- **Run3:** compile and exporter passed, 1 test, 13.77 s total. Actual optional content properties exclude `null`; numeric rating and ID overrides remain correct. Inspection found that analysis `Decision` and consent `Decision` shared `Decision.ts`: it contained the consent enum while `MeetingAnalysis` expected the analysis object. p23 gave the analysis projection its own name; X added a complete pre-write dependency/name/path guard and two collision regressions.
- **Run4:** compile passed; the two negative collision regressions passed, while the real exporter rejected another actual duplicate: data-management `ConfirmVm` versus billing `ConfirmVm`. No output was written. X renamed only the data-management TS projection and retained the guard.
- **Run5, published boundary:** compile and all three selected tests passed, 6.69 s total. Inspection confirms separate `AnalysisDecision`/consent `Decision`, separate `DataManagementConfirmVm`/billing `ConfirmVm`, correct dependency imports, 49 omission-only optional properties across the 17 content structs, numeric scalar overrides and resolvable local type imports. Actual output is 144 TS files plus 21 fixture roots under `transgate/web/src/lib/core`. Producing receipt: `/tmp/rustr-p5-tsgen/run5/report.json`; generated hash-manifest SHA256 `5ff92934aa5daa730a35e6a7fd694c2a43dbeb40d6715bccc0e231edbfa2bb28`; fixture SHA256 `602865cdf1c9da5786b5234a8287f036912d09a91715e1ef40aa6eb31a9137b2`. This is not an application TypeScript, HTTP or browser pass.
- The warnings about unsupported serde attributes remain in the logs. `AnalysisProgress` is not a reached exporter root; its required nullable source field is unchanged, not claimed as generated-output proof.

Registered in `transgate/server/Cargo.toml` (p1T coherent): `billing-http` (path), `percent-encoding 2`, `url 2`, `tower 0.5` (`default-features=false`, `[util]`), `tower-http 0.6` (`default-features=false`, `[fs]`), tokio `+fs`.

**Registered on p22's COHERENT message (2026-10-26).** `transgate/client` and `transgate/wasm` are now root members. I checked them statically: each module tree matches its files, and every external crate the code uses is declared.
- **Lock:** `cargo metadata` exit 0 with no new packages; `Cargo.lock` sha256 `339b85e1…`.
- **wasm32 resolution:** `cargo tree --target wasm32-unknown-unknown` resolves without compiling. The browser graph has no getrandom 0.3 or 0.2. Its only getrandom is 0.4.3, through `zip 9.1` and `transgate-export`, and that copy has `wasm_js` enabled. So no `getrandom_backend` cfg is expected; the first wasm compile still has to prove it.
- **Server dependencies (p1T):** `getrandom 0.3` and `lol_html 2` (2.9.0, which added 14 packages), lock sha256 `8a823553…`.

Evidence: `/tmp/rustr-p5-lock-d17-1792992476` and `/tmp/rustr-p5-lock-clientwasm-1792993050`.

**Added to S1 by the lead (2026-10-26), on the same frozen snapshot, after the host build and before the P4 runs** (`s1.sh p5`). This is targeted client/wasm proof, not C1/C2/C3. The installed rustc 1.99.1 (Arch) has no wasm32 std; only `x86_64-unknown-linux-gnu` is present under `lib/rustlib`. Both wasm steps therefore use the repository's build-std path, and the logs record the exact invocations:
1. `cargo test --locked -p transgate-client`
2. `RUSTC_BOOTSTRAP=1 CARGO_TARGET_DIR=target/wasm-buildstd cargo check --locked -Zbuild-std=std,panic_abort -p transgate-wasm --target wasm32-unknown-unknown`
3. `TASKR_WASM_BUILD_STD=1 ./scripts/build-transgate-wasm.sh`, with `wasm-bindgen 0.2.128` from `~/.cargo/bin`. The `pkg` hashes and a tar copy are kept.

Two further lead-authorized checks use the same immutable S1 inputs:
- `cargo test --locked -p transgate-core --features ts --lib presentation::app::tests`.
- A throwaway native executable outside the immutable source tree, linked to the actual composed `AppState`/billing artifacts. It exercises confirmed, declined and unconfirmed outcomes, invoice capture/selection fencing, verbatim special-client text, and displayed-versus-charged amounts. This is **pure composed-state smoke**, not HTTP, rendered UI or C2 proof. No product dependencies or root members are added.

No speculative getrandom cfg and no toolchain substitution. A failure keeps its snapshot and logs, and goes to the owner (p22). Any repair is built as a new versioned snapshot (`S1.1`); the failing one is never modified.

**Remaining P5 boundaries, separate from S1 proof:**
- X's composed session/billing source, R's shared limits and billing exports, and p22's ticket consumer are source-coherent. The generated boundary is published with the run5 receipt; every prior failure and rejected contract remains preserved.
- V's actual Blob-modal lifecycle implementation and subsequent view integration remain distinct from client/wasm proof. No C1/C2/C3 execution is authorized.
- All 16 locales are source-ready; `locales.md` records the final billing meanings and commands. No validator, import or capture run is claimed yet.

## 5c. Async document export fencing (consumer-boundary regression)

Requested from the documents integration: the displayed content and the click time are captured once. A lazy font load must not deliver that capture after logout or an account switch, after consent is refused or deleted, or after disposal. A late font completion must not replace the capture with newer data. The client/app effect owns the guard over owner, session and consent; the writer stays free of auth.

- **Consumer regression (owned by the implementer):** hold the font load, revoke or switch the session, release, and assert that nothing is saved or copied.
- **C2 browser check (p1Y):** a Playwright route holds the font asset request. The test signs out, switches account, or refuses applicable AI consent, then releases the font. It asserts no stale preview, download event or clipboard write. A second variant lets newer data arrive during the hold and asserts that the saved bytes match the content captured at click time.
- **Invoice Blob-preview lifecycle (C2, still authorization-gated):** exercise the actual Rust PDF → `Platform.preview` → root-owned modal iframe path. Verify the preview Blob URL is revoked on close, replacement and session disposal; replacement must not revoke the new active URL. A held async completion after owner/session invalidation must neither open a modal nor leave a live Blob URL. Inspect the actual app/confirmation CSP: Blob frames allowed, Blob scripts disallowed, objects disabled, and D19 checkout unaffected. These are planned checks, not observed browser results.
- **Same-payment reopen race:** hold request A's font load, close the invoice preview, reopen the same payment and start request B, then release A. Payment/account equality alone must not admit A: the core-owned context/request ticket must still identify the selected, open, latest request. Assert both A's PDF and A's failure are discarded; neither may reopen the modal, replace B, nor surface stale `DocumentFailed`. p22's final source handoff consumes the actual predicate and includes `closing_and_reopening_the_same_invoice_fences_held_bytes_and_old_failure`; the authorized client test run has not executed it yet.

**Lock follow-up (2026-10-26).** p1W changed his pin to `unicode-segmentation = "1.12"` (caret). I then ran `cargo update -p unicode-segmentation --precise 1.13.3` and `cargo metadata`, both exit 0. The workspace is back on 1.13.3, and `Cargo.lock` sha256 is `39ffe9c0…` (evidence in `/tmp/rustr-p5-lock-seg-1792990602`). The same refresh recorded two edits owners made to their own manifests. They add dependency links only; no new package or version enters the lock:
- `modules/billing/http` gained `base64 0.22` (p1R);
- `transgate-core` gained an optional `ts-rs` 12 `[uuid-impl, serde-json-impl]` behind feature `ts` (p1X). It is enabled only by `transgate-wasm`.

## 5d. D19 checkout CSP: browser acceptance checks (C2/C3, when authorized)

D19 (`decisions.md`; D17 contract in `billing.md` §6) applies an enforced checkout-only policy:
- `default-src 'none'`;
- `script-src 'nonce-<per response>' 'strict-dynamic'`;
- `style-src https: 'unsafe-inline'`;
- `font-src https: data:` and `img-src https: data:`;
- `connect-src https:`, `frame-src https:` and `form-action https:`;
- `base-uri 'none'`, `object-src 'none'` and `frame-ancestors 'none'`.

The checks against the owned runtime and local fixtures:
- The checkout response has exactly the D19 directives in an enforced `Content-Security-Policy` header, with no `Report-Only` header. There is no `unsafe-eval`, no script `'unsafe-inline'` and no `http:` source, except the exact configured loopback fixture origins that the runtime adds for local testing. Every added origin is listed in the report.
- The nonce is different in two consecutive checkout responses, and every executed script carries it. A script injected without the nonce is blocked, which shows up as a `securitypolicyviolation` event.
- The ordinary SPA and confirmation pages keep their own CSP unchanged, with none of the checkout directives present.
- Every `securitypolicyviolation` event is recorded. A blocked need on the checkout page is reported as evidence for the lead's decision; the policy is never relaxed to make a test pass.

Limits: local fixtures only. This makes no live or sandbox claim about Lemon Squeezy or iyzico compatibility, and makes no live provider call.

## 6. Candidate4 evidence boundary

Candidate4 is `/home/mu/.cache/rustr-integration/s1-20261026195749`, source-manifest SHA256 `a1a7d0006c5c33c579df9dbc7ae3b478753fdabdf8f3b4b50264f919102ff193`. Its source is immutable. Reports and tester harnesses are under `logs/next-p5-c`, outside captured source. The stale gate and static Storybook/real wasm/app builds passed. C1's guarded matrix is in progress; the failed 60-case harness precondition run is retained separately. C2 is partial FAIL (`c2-report.json`); C3 image build failed because imported app PNGs were excluded (`c3-report.json`). Neither is acceptance.

The actual C2 Mailpit link exposed a runtime configuration defect: GoTrue resolved `/verify` against the origin, dropping the proxy prefix. The working runtime now sets `GOTRUE_MAILER_URLPATHS_CONFIRMATION`, `_INVITE`, `_RECOVERY`, and `_EMAIL_CHANGE` to `/auth/v1/verify`. A separately recorded runtime copy proved the actual delivered link used that path without spec rewriting. P9 production/staging GoTrue must carry equivalent deployment settings. Candidate4 source was not edited.

The positive AI fixture must provide at least 50 UTF-16 units of canonical transcript content on the next authorized candidate. Candidate4's saved 33-unit line was correctly refused before provider dispatch; the generic NoTranscript wording is independently owned by the core repair. Preserve a separate short-input case, not a weakened minimum. Erasure's initiating tab reaches the public root, currently 404 until P6; that is a P6 dependency, not a P5 defect.

## 7. Candidate5 Iyzico loopback fixture contract (source-only)

`scripts/p5-web/iyzico_fixture.py` is integrated into the working runtime, not candidate4. It implements only the adapter's published protocols from `modules/billing/gateways/iyzico/src/client.rs` and `sign.rs`: PAYG initialize/detail POST, subscription initialize POST/result GET/detail GET/cancel POST. Every request must carry the exact IYZWSv2 signature over nonce + path + transmitted bytes (`{}` for GET); initialization requires an explicit queued `POST /__p5/iyzico-plan` with `{"kind":"payg"|"subscription","outcome":"success"|"failure"|"unconfirmed"}`. Unknown routes, missing plans, mismatched checkout kind, unbound subscriptions and invalid signatures fail the fixture.

The initialization answer contains a synthetic provider form with a token and a `Complete fixture payment` button, plus an inline script for the real host nonce renderer to process. It posts to the configured owned app origin's `/checkout/confirm`; only the known synthetic public origin is mapped to loopback, preserving callback path/query. Specs do not rewrite confirmation links or proxy paths. Browser submission therefore exercises the real form/Accept HTML branch and embedded confirmation result, not a copied reducer.

PAYG lookup echoes the exact charged amount/currency and a unique provider payment ID; plans produce SUCCESS, FAILURE or PENDING. Subscription success binds a unique customer/reference/plan, then supports authenticated detail and cancellation. A non-success subscription lookup remains uncertain as the actual adapter specifies. This is not a 3DS, issuer, sandbox or live-payment emulator. `iyzico-trace.json` records calls, remaining plans and checkout count without auth headers or credentials. No fixture execution or candidate5 acceptance is claimed yet; lead must release the next candidate.
