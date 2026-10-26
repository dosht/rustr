# P5 Transgate host/API/server (p1T)

## Status and ownership

**COHERENT — source handoff for transgate-server / transgate-http / transgate-api.** D17 uses R's actual published seam, V's blank-document markup and all 16 common catalogs; D19 is enforced only on that document; D20 app assets use only `/_app/*`. Source is saved and unverified: no worker build/test/lint/format/browser/provider calls were executed. Y owns conditional S1 verification. Earlier P4 strict Clippy, offline 805 tests, affected real-PG 207 tests and E4 81 tests remain separate snapshot evidence, not proof for this P5 source or binary791.

p1T owns Transgate API/HTTP/server and the narrow host identity additions in `transgate/billing/{directory.rs,identity.rs,lib.rs}`. p1R owns `modules/billing/{core,http}` and jobs-postgres; p1X owns account use cases, flags, route recognition and presentation. Y owns coherent root dependency/lock registration and verification. No taskr or book edits.

## Session/bootstrap

`GET /api/session?locale=<UI locale>` uses ordinary authentication. Missing/invalid credentials are 401; a permanent account erasure barrier is 410 before profile delivery; actual directory failures remain 503. No erasure authentication bypass applies.

200 is the direct serialized `transgate_core::account::SessionProfile`:

```json
{"account":"<uuid>","email":"ada@example.test","full_name":"Ada","display_name":"Ada","joined_on":1700000000000000,"capabilities":{"ai":true,"ai_chat":false,"youtube":true,"microphone":false,"google_drive":false,"onedrive":false,"show_original":false,"mobile_ux_jobs":true,"mobile_expanded_player":false,"mobile_ux_ai":true,"billing_history":true,"customer_lookup":false}}
```

`full_name` and `joined_on` are nullable. `joined_on` is UTC microseconds since epoch from GoTrue `email_confirmed_at`, never `created_at` or generic `confirmed_at`. The trusted directory supplies profile facts; the core computes display name and effective flag values. No email/user/locale allow-lists, private keys, balance, plan or consent are in this response. Billing lookup still enforces its server authorization independently.

Source seams: `account::VerifiedProfile`, `account::session_profile`, `flags::FlagConfig` and `flags::Capabilities`. `transgate/billing/src/session.rs` verifies the JWT, fences before/after the trusted directory lookup and reuses its fetched facts for adoption/refresh. It never substitutes JWT metadata when the directory is absent/unavailable. The separate receipt method verifies credentials but performs only the barrier query. The existing first-request deletion enrollment and normal/admin 410 paths are unchanged.

## Feedback and support

`POST /api/jobs/{id}/feedback`, ordinary authentication:

```json
{"rating":5,"feedback":"Optional comment as typed"}
```

`rating` is the core `FeedbackRating` numeric value 0, 5 or 10, not a UI index. Omitted/null `feedback` becomes the empty comment; supplied text is not trimmed. Account/user/email authority is not accepted from the body. 201 returns the committed core `JobFeedback` directly: `{id,job,account,rating,comment,created_at}`, with numeric job and microsecond timestamp. Missing/foreign job is the same 404; barrier 410; invalid JSON/types/rating 400; store failure 500 `database_error`, store conflict 409 `conflict`. Calls `Account::submit_feedback`.

`POST /api/support`, ordinary authentication:

```json
{"message":"Message as typed","jobId":123}
```

`jobId` is optional/null, an integer context only: it does not authorize or read another account's job. The verified account is never taken from the body. Calls `Account::request_support`; only after its transaction commits, 202 returns `{"request_id":"<uuid>","status":"queued"}`. This acknowledges durable acceptance, not delivery; the shell must not claim that either mail has already been sent. Storage failure follows existing `JobsError`: 500 `database_error`, not a new 503 mapping. No automatic submission retry is introduced.

Delivery uses `account::ports::SupportOutbox` implemented by `PgJobStore`: claim, two ordered mails from `SupportMail::ORDER`, then ack; failure releases the claim for the established worker retry policy. Recipient facts come from `CustomerDirectory`, not client fields. Support receives the exact message first; the owner acknowledgement follows, using core subjects/idempotency keys and the real source auto-reply template. No network inside a database transaction. X's final ack/release take `&ClaimedSupport` and return a bool fenced by attempts; a stale worker logs false and moves on.

## Read-only erasure receipt

`GET /api/account/erasure/status` verifies credentials without normal contact refresh/adoption. It yields only an account id, never email/admin roles. Unbarred verified callers receive 404 without hydration/provisioning. Barred callers call only `Privacy::erasure_status(account)`, whose read transaction rolls back. The endpoint never requests, advances or retries erasure.

200 is the existing core `ErasureView`, tagged by `status`: `requested{waiting_for}`, `local_data_erased{pending,exceptions}`, `fully_erased`, or `erased_with_exceptions{exceptions}`. No request is 404 `not_found`; invalid credentials 401 `unauthorized`; verification/barrier-query outages 503 `identity_unavailable`; receipt read-store failures retain 500 `database_error`. Other ordinary/admin routes retain 410 for the same erased identity. A local saved receipt is only last-observed after sign-out/expiry, not a fresh server result.

The existing `authenticate_for_erasure` is explicitly not this path: its unbarred branch intentionally preserves contact enrollment for first-request deletion. That behavior remains intact.

## Session start clean cutover

`POST /api/jobs/start` replaces the session-only `POST /api/v1/transcriptions`. Same parser, owned `Jobs::start` use case and 201 response; no invented 202 envelope:

```json
{"success":true,"data":{"success":true,"results":[],"metadata":{"newBalance":0,"isFreemium":true,"totalJobs":0,"successfulJobs":0,"failedJobs":0,"queuedJobs":0,"requestType":"jobIds"}},"metadata":{"requestId":"<uuid>","timestamp":"<UTC ISO8601>","processingTime":0,"clientType":"web"}}
```

The example illustrates the envelope, not an accepted empty input. Existing `jobIds`/locale/options parsing and per-job result fields stay authoritative in `transgate-api`. Session callers still cannot submit `audioUrls`. Every old session caller, test and host note must migrate; the old route is removed rather than aliased. P7 public/key intake and P4 `/api/v1/ai/*` are not part of this cutover.

Start body: `{"jobIds":[123],"locale":"en","options":{"preferredProvider":"soniox","fallbackEnabled":true}}`; optional locale/options retain existing parser semantics. Language/translation/speaker changes are written through existing `PATCH /api/jobs/options`, not invented start fields. 402 is `{"error":string,"code":"insufficient_balance"}` without a numeric balance: `upload_queue::Event::StartFailed.balance` is a bool, discriminated by status/code, never prose.

`GET /api/jobs/drafts` is the restoration/poll source: 200 direct `Vec<Job>` from the actual `Jobs::drafts` use case, every owned draft, oldest first, no history window or 20-row paging. `GET /api/jobs` is the separate listed-history source and excludes drafts. Normal 401/410 and existing JobsError mapping apply. `GET /api/languages/usage` supplies the separate preferred language/target lists.

## Billing HTML confirmation and invoice boundary

R owns quote `display_amount` (USD two-decimal display, separate from charged economics), `GET /api/billing/invoices` owner context and the existing admin invoice route's additional `invoice_context`.

Accepted billing seam: `ConfirmPage::render(&ConfirmOutcome) -> String`, installed optionally on `BillingHttp`. R's confirm handler executes `confirm_checkout` exactly once. A form-urlencoded POST that accepts HTML and has a renderer gets the HTML adapter; otherwise existing JSON is retained. Failed status and pending/uncertain outcome remain truthful. The host never confirms a second time and never purchases on cold GET/reload.

V's index marker is `<!--tg-inject-->`. The host injects public `tg-config`; HTML confirm also injects the trusted single serialized `ConfirmOutcome` as `tg-confirm`: `{"outcome":"confirmed","view":ConfirmView}` or `{"outcome":"failed","status":u16,"code":string,"message":string}`. This is R's published source shape, not a raw `ConfirmView` or a second host result.

### D17/D19 checkout start document

R owns one `Billing::start_checkout` for the authenticated hidden form `POST /checkout`, and the 303 hosted-HTTPS redirect branch. Its only fields are ASCII `order` (unpadded base64url of existing OrderBody JSON) and `access_token`; the form/token/decoded-order caps are 32768/8192/16384 bytes. Existing credential verification, same-origin/TLS, competing-method rejection and JSON economic validation remain R's. No API-init then second-init, srcdoc/blob, token URL/log/response/VM echo or host network call is introduced.

`billing_http::CheckoutPage::render(&CheckoutOutcome) -> RenderedCheckout {html,content_security_policy}` is implemented in `transgate/server/src/web/checkout.rs`. A fresh 32-byte OS-random hex nonce is applied to every provider script with the actual lol_html parser, including replacing pre-existing nonce attributes; script raw text is not substring-rewritten. V's separate template has no app bootstrap, `tg-config`, telemetry, WASM or style nonce. Failure labels come from the selected common catalog, never Failed.message; 401 selects the sign-in copy. All 16 catalogs must deserialize at startup, with no missing-key English fallback.

The returned CSP is exactly D19: default none; script nonce + strict-dynamic; styles HTTPS + unsafe-inline; fonts/images HTTPS + data; connect/frame/form-action HTTPS; base/object/frame-ancestors none. This deliberately trusts arbitrary HTTPS provider/issuer destinations, not a fabricated origin allow-list. Only the exact configured HTTP loopback site and `IYZIPAY_URI` origins extend destination directives for local fixtures; auth/storage origins do not become checkout exceptions. Ordinary app and confirmation policies are unchanged. No actual Iyzico/sandbox/3DS compatibility is claimed.

`transgate_billing::Pages {confirm,checkout}` is passed optionally to `compose`/`compose_with`. The host installs R's `with_checkout_page` with a normalized configured site origin; existing billing-only runtime callers still pass None. R exclusively owns GET `/checkout` (303 `/`, no start) and POST `/checkout`; T's duplicate GET registration was removed. On HTML parser refusal, no raw markup executes or reaches logs: a localized generic failure body is returned, payment facts stay stored and no second initialization runs. The HTTP status still belongs to R's typed result.

## Static serving and runtime config

Static root setting: `TRANSGATE_WEB_DIR`. Core `presentation::route::is_app_path` remains the only SPA allow-list; public roots, dashboard, unknown routes and assets are never blanket SPA 200, and unmatched `/api/*` stays JSON 404. POST confirmation belongs to billing; GET confirmation serves the application without purchasing.

D20 mapping is exact: URL `/_app/<file>` -> physical `dist/_app/<file>` (flat Vite output). Every generated asset is immutable except `/_app/theme-init.js`, which is no-cache. Index/HTML responses are no-store. The app owns no `/assets/*` or root `/theme-init.js` alias. Existing encoded traversal, canonical confinement, symlink refusal and range behavior are retained. Explicit MIME: wasm application/wasm, ttf font/ttf, otf font/otf, txt text/plain; charset=utf-8, svg image/svg+xml, jpg/jpeg image/jpeg.

Public config is injected at serve time with V's `<!--tg-inject-->` marker: apiUrl, authUrl, nullable authKey, siteUrl, captcha and telemetry. `checkoutNonce` was cleanly removed from the app config and matching baseline tests. JSON script bodies escape `<`, `>`, `&`, U+2028/U+2029. No flags or secrets are embedded.

The ordinary app/confirmation CSP retains its same-origin/wasm baseline and exact auth/telemetry/Turnstile/YouTube/storage extensions, with `frame-src blob:` for the root-owned invoice PDF preview. This does not grant Blob script execution: script-src has no blob source, and object-src stays none. D19's nonce/scheme/style permissions remain confined to the blank checkout response; its policy is unchanged. Middleware preserves the route-specific checkout CSP rather than overwriting it with the app policy.

## Verification handoff

No builds/tests/lint/format/browser/provider calls were executed. Source selectors for the lead, after coherent dependencies and explicit authorization:

```sh
cargo test -p transgate-api account::
cargo test -p transgate-billing --test session_profile
BILLING_TEST_DATABASE_URL=... cargo test -p transgate-billing --test runtime erasure::account:: -- --include-ignored
TRANSGATE_SERVER_TEST_DATABASE_URL=... cargo test -p transgate-server workers::pg_tests::support:: -- --include-ignored
cargo test -p transgate-server web::
```

These are commands to run, not pass evidence. Existing `workers::pg_tests::` (including F8 and held-AI/mail independence) remains applicable; its disposable scratch fixture was moved unchanged from the AI child into the shared parent so support uses the same isolated production-order migrations.

## Public environment and dependency registration

- `TRANSGATE_WEB_DIR`: built Vite distribution; when enabled, the server also requires the real jobs/billing host.
- Required with it: `TRANSGATE_WEB_SITE_URL` (origin) and `TRANSGATE_WEB_AUTH_URL` (`/auth/v1` or an absolute GoTrue base).
- Optional public values: `TRANSGATE_WEB_AUTH_KEY`, `TRANSGATE_WEB_TURNSTILE_SITE_KEY`. No service-role/JWT-secret fallback.
- Optional all-or-none groups: `TRANSGATE_WEB_POSTHOG_KEY` + `TRANSGATE_WEB_POSTHOG_HOST`; `TRANSGATE_WEB_SENTRY_DSN` + `TRANSGATE_WEB_SENTRY_ENVIRONMENT`. Standard Sentry public-key usernames are allowed; passwords/query/fragment are not.
- Public `apiUrl` is `/api`, same-origin. HTTPS required except actual loopback HTTP fixtures.
- Private `TRANSGATE_FEATURE_FLAGS`: full serialized core `FlagConfig`, absent means its source defaults; malformed JSON refuses startup. Its allow-lists never enter `tg-config` or the session answer.

Y registered the `transgate-server` dependencies: direct billing-http path, percent-encoding 2, url 2, tower 0.5 (`default-features=false`, `util`), tower-http 0.6 (`default-features=false`, `fs`) and Tokio `fs`. Y reports lock resolution to tower-http 0.6.11, tower 0.5.3, url 2.5.8 and percent-encoding 2.3.2. `web.rs` needs no tower-http feature beyond `fs`: Axum middleware sets headers directly. Registration is not compile/runtime evidence; no root manifest/lock was edited by T.

Y has registered native `getrandom = "0.3"` and `lol_html = "2"`, reporting cargo metadata exit 0, getrandom 0.3.4, lol_html 2.9.0 and lock SHA prefix `8a823553`. getrandom supplies 256 bits of per-response entropy; lol_html rewrites actual script attributes without modifying JavaScript string contents. Y also confirmed that all 19 include_str! inputs exist and the production host route is `/_app` without an app `/assets` route. These are registration/static observations, not compile/runtime proof.

### Frozen compile inputs for S1/image

No build.rs or custom rerun-if-changed was added. The child `transgate/server/src/web/checkout.rs` uses rustc-tracked include_str! paths:

- `../../../web/checkout/document.html` and `../../../web/checkout/failure.html`.
- `../../../web/src/tokens.css`.
- `../../../web/locales/<code>/common.json` for en, tr, pt, es, de, id, it, fr, el, ru, da, sv, no, fi, kk and az.

These resolve to `transgate/web/...` in the repository and must be identical in native S1 and image builds. Vite does not process the checkout templates. The five required common keys are Checkout, We couldn't start your checkout, Back to plans, Please sign in and try again., and Something went wrong while preparing your payment. No charge was made — please try again. All labels are HTML-escaped.

## Saved source

- `transgate/api/src/account.rs`: existing Value-parser convention, numeric rating boundary, no client identity fields, durable queued reply.
- `transgate/http/src/account.rs`: four account routes, compact error boundary and private no-store responses; `http/src/lib.rs`: start cutover/drafts and shared error mapping.
- `transgate/billing/src/{directory,session,lib}.rs`: trusted confirmation timestamp/profile, verify-only receipt identity and optional Pages composition; all current constructor callers are migrated.
- `transgate/server/src/{account,host,main,web}.rs` and `web/checkout.rs`: real actor/identity/flags composition, static loader, separate checkout and confirmation renderers.
- `transgate/server/src/workers/support.rs`: ordered durable support sends, attempts-fenced ack/release, observed-barrier refusal; an independent joined support loop cannot hold media/job mail/AI.
- Regression source: account/API/profile/runtime and support files above, `transgate/server/src/web/tests.rs` (D20 confinement/cache/MIME/app policy) and `web/checkout/tests.rs` (nonce/header agreement, raw-script preservation, label privacy/escaping, D19 destinations).

## Comments and source handoff

- D17/R, markup/V, localization/p24 and D19 policy dependencies are published and consumed; Y has registered the native dependencies. There is no outstanding host policy/interface/source-registration blocker. Conditional S1/image/runtime verification remains Y's work.
- Y reports the prior run5 copy preserved. Historical discovery ledgers and immutable prior P4 evidence intentionally retain their source endpoint names; T did not overwrite Y's harness.
- Self-review limit: errors and regressions are authored against source, not exercised. New API writes preserve the existing store error mapping (500 database_error/409 conflict), not a fabricated 503. Receipt verification/barrier failures remain 503; ordinary erased access remains 410.
- Both internal leaves were `openai-codex/gpt-6.1-sol`, not Claude. They ended on `turn_rate_limit_exceeded`; static baseline source was preserved and only T's existing Sol host scope was continued. No Claude work was absorbed, no model/provider hop, no new leaf launched after hold.
- No owned smoke processes, containers, watchers or scheduled work were started. Earlier internal jobs are terminal; no new agent, replacement session or model transfer was used after resumption.
- Source-ready is not acceptance or compile/runtime evidence. Y's authorized checks must cover the real one-start POST document, fresh nonce/header agreement, unchanged ordinary policy and D20 asset routing before any compatibility claim.

## REPAIR-P5-1: C3 image entry (2026-10-26)

The three packaging sources now implement the accepted integration §5 split:

- `transgate/Dockerfile`: Rust 1.99.1 builds the server and a fresh wasm32 module; wasm-bindgen CLI is pinned to 0.2.128. The digest-pinned Node 24 stage runs `npm ci` against the committed lock, consumes that in-image WASM/glue, and runs the existing web build. Only the server binary and built `dist` enter the Debian runtime; `TRANSGATE_WEB_DIR=/srv/transgate-web`, UID/GID 65532, existing liveness probe and shutdown signal remain. There is no runtime build/download or writable asset-directory requirement.
- `transgate/Dockerfile.dockerignore` and `transgate/processor/deploy/Dockerfile.dockerignore` share identical file-only allow-lists. They include checkout templates, generated tokens/types/fixtures, all 16 catalogs, actual Vite/Svelte/TypeScript configs, `public/_app/theme-init.js`, feedback SVGs, the complete D18 font/license set and invoice logo/provenance. Host `pkg`, web tests/stories/specs and test setup are excluded; `.env*`, `.pem` and `.key` exclusions remain last.
- Finalization against actual files: there is one `tsconfig.json`, no PostCSS config, and no additional public asset directory. Committed generated TypeScript/fixtures are consumed, not regenerated in the image. Font/CJK expansion follows the real `documents/assets.ts` imports rather than the older eight-file example in integration §5.

Verification boundary: this changes packaging, not component behavior, so no source-text/wiring regression was added. The two ignore files compare byte-identical (`cmp`, exit 0). Owned host regression command `env -u TRANSGATE_SERVER_TEST_DATABASE_URL -u BILLING_TEST_DATABASE_URL cargo test --locked --offline -p transgate-server web::` passed: **13 passed, 0 failed, 14 filtered**, output artifact383. It exercises app routing, asset confinement/ranges/MIME/cache, injected config and checkout document/CSP behavior; it does not prove image context or container behavior.

Packaging source is ready for Y's stale gate, Docker context-canary export, image build and actual non-root/read-only C3 proof in the lead-authorized window. No image build, browser/Storybook certification, provider call or candidate3 mutation was performed by T. The owned test process has exited; no owned smoke processes/containers or cleanup blockers remain.

## REPAIR-P5-2: complete web input classes

Candidate4 C3's `c3-image-build.log:1418-1481` shows five unresolved imports of the two app/auth PNG logos. The earlier allow-list covered code and SVG feedback art but not all Vite input classes. This supersedes REPAIR-P5-1's web-input completeness claim, not its retained native host test result.

Both ignore files now allow PNG/JPG/JPEG/SVG/WebP, WOFF/WOFF2/TTF/OTF and TXT assets throughout the web tree, alongside the inventoried source/config/catalog/template inputs. Output/dependency/test/story directories and named tests/stories/specs are explicitly excluded after those asset rules; the unchanged secret exclusions are last. Asset files and locations were not changed.

Exact inventory/comparison commands run from repository root:

```sh
git ls-files transgate/web
git ls-files --cached --others --exclude-standard -- transgate/web
cmp transgate/Dockerfile.dockerignore transgate/processor/deploy/Dockerfile.dockerignore
```

The index-only command lists **0** files in this checkout; the second command is necessary because this product source is currently untracked. Its **451** paths classify as **365** required build inputs and **86** excluded test/story/tooling inputs. Required extensions: CSS 2, HTML 3, JS 2, JSON 88, PNG 2, Svelte 90, SVG 3, TS 175. No tracked-input coverage was claimed from the empty first command.

A read-only `python3 -B` check consumed the NUL-delimited (`-z`) forms of those Git lists and compared every required/excluded path against the ordered ignore rules, matching paths and parents using the literal/`*`/`**` subset present in these files: **0 missing required inputs; 0 admitted excluded inputs**. Additional static probes included **30/30** asset-class paths and excluded **190/190** output/test/secret paths; byte equality and the final eight secret guards passed. This is an inventory/static rule check, not Docker's actual context-canary export or an image build. No image/candidate changes or background jobs were made; Y retains the actual C3 build/proof.

## REPAIR-P5-3b: candidate4 chat content diagnosis

Evidence is read-only from `/home/mu/.cache/rustr-integration/s1-20261026195749/`: `logs/next-p5-c/c2-ai-observations.json:4-26` records the persisted edited line `Candidate four edited transcript.`, blank following speaker/corrected-text fields, successful lines/edit/export HTTP statuses, chat 400, the displayed error, and **zero provider calls**. The saved text measures **33 UTF-16 units**, not the 32 estimated in the preliminary relay. Exact POST/response bodies were **not captured**; neither their jobId/account nor an exact JSON envelope is reconstructed here. No live PG query, endpoint rerun or candidate write was performed.

The immutable `src/` sources establish the content path:

- `transgate/http/src/ai.rs:120-131` authenticates normally, parses the API jobId/message/history, and passes them unchanged to `Ai::start_chat`; no host-side transcript text is supplied.
- `transgate/core/src/ai/app.rs:557-576` verifies owner visibility, reads `JobStore::lines(job)`, then builds sanitized canonical `line` text (including nonblank speakers). It maps both an empty row list and a failed context validation to `AiError::NoTranscript`. `ai/mod.rs:108,121` maps this to HTTP 400 and the observed message; host `http/src/ai.rs:54` maps the code to `no_transcript`.
- `transgate/core/src/ai/sanitize.rs:106-130` uses `Line.line`, not `corrected_text` or an archived provider transcript, and requires **50..=512000 UTF-16 units** after sanitation. The saved plain ASCII line remains 33 units. This lower bound is explicitly preserved by P4 `implementation/content.md:59,134` (AI-012), not newly inferred policy.
- `transgate/jobs-postgres/src/store.rs:136-160` checks account/job ownership and reads `transgate_jobs.lines WHERE job_id=$1`. Wrong/foreign ownership returns NotFound before context validation; an exact submitted jobId still cannot be asserted without the missing request body.
- Completion inserts canonical line/speaker/corrected_text fields into that table (`jobs-postgres/src/uow.rs:423-447`; core `jobs/app.rs:1633-1640`). Editing updates the **line** column and advances the AI source revision (`uow.rs:462-470`; `jobs/app.rs:2022-2044`). Detail, lines and export use the same reader (`jobs/app.rs:2075-2086,2111-2118`); export does not impose the chat-context minimum.

Diagnosis: the known persisted job1 content explains the observed refusal as the preserved **TooShort** gate, not a missing edited/original transcript source, corrected-text mismatch, storage outage or provider error. The exact request-target limitation remains explicit. No host repair or test expecting admission of this 33-unit context is justified. The existing core regression source `ai/analysis/sanitize_tests.rs:25-31` rejects 49 units and accepts 50 UTF-16 units; it was not rerun. A changed minimum or distinct short-context error would be X-owned policy work, not an HTTP patch.

Lead handoff: for a positive C2 AI flow, Y should use an edited canonical context meeting the published minimum on the next authorized candidate and retain short-context refusal as a negative case. No failing host regression was invented for behavior that already conforms to the accepted rule. Only this diagnosis note changed for 3b; no core/client/host product source, DB or candidate was edited.

## REPAIR-P5-5: chat history reports the held answer

`GET /api/ai/jobs/{job}/chat` (`transgate/http/src/ai.rs`) now serializes core's `transgate_core::ai::ChatHistory` directly: `{"messages":[ChatMessage...],"in_flight":null|{"question":<as typed>,"generation":<GenerationView>}}`. `in_flight` is always present; the held partial is `generation.draft`. Wire names are core serde (snake_case), as the previous `messages` already were; there is no `transgate-api` wrapper (the client parses `Value`, p22). Auth, consent (403), owner scope (foreign account 404, no session 401) and covered-epoch fencing are core's and unchanged. The route is read-only: no POST, quota, provider call or key. Clean cutover: the old `{messages}`-only shape has no fallback.

Evidence: failing first, `transgate/billing/tests/runtime/ai.rs::ai_http_chat_history_reports_the_held_answer_to_a_second_replica_and_only_to_its_owner` failed on the missing explicit `in_flight: null`, then on `in_flight: null` while a generation ran. After p1X's core landed, `cargo test --locked --offline -p transgate-billing --test runtime ai_http_ -- --include-ignored` against an owned disposable `postgres:17-alpine`: **4 passed, 0 failed** (the new test covers no-in-flight null, a held partial `"Hello"` read through a second replica sharing only the database, the question as typed, foreign 404, unauthenticated 401, one provider call total, and `in_flight: null` plus the saved pair after completion).

## REPAIR-P5-7: second unhashed app file (checkout badge)

Read-only finding: `/_app/<file>` is served from `TRANSGATE_WEB_DIR` with `image/svg+xml` for `.svg` already, but the revalidation rule was the single literal `/_app/theme-init.js`; any other unhashed file would have received `immutable` for a year. Fixed in `transgate/server/src/web.rs` with the explicit set `UNHASHED_ASSETS = [theme-init.js, iyzico-secure-payment.svg]` (a filename-pattern heuristic was rejected: 8-letter lowercase words such as `checkout` look like Vite hashes). Both get `Cache-Control: no-cache`; hashed Vite outputs stay `public, max-age=31536000, immutable`. Test first red (`immutable` on the badge), then `cargo test --locked --offline -p transgate-server web::` 13 passed. Checkout CSP is unchanged: `img-src https: data:` already admits the same-origin https badge, and the exact local-http fixture origin is the existing exception. Adding a future unhashed file means adding its name to that set.

## REPAIR-P5-7b: checkout `img-src 'self'`

Lead decision: the checkout CSP `img-src` is now `'self' https: data:` (plus the existing exact fixture origins), so the local `/_app/iyzico-secure-payment.svg` badge loads on plain-HTTP runs regardless of whether the browser origin matches `TRANSGATE_WEB_SITE_URL`. This supersedes the REPAIR-P5-7 note that the existing policy already sufficed (it only did when the configured http site origin equalled the browser origin). `'self'` adds only the page's own origin; `https:` already admitted every HTTPS image source. No other directive changed (script-src nonce/strict-dynamic, style/font/connect/frame/form-action, default-src none). Test first red (`checkout_policy_is_enforced_...` expected `img-src 'self' https: data:`), then `cargo test --locked --offline -p transgate-server web::` green. The badge file is present at `transgate/web/public/_app/iyzico-secure-payment.svg` (Vite copies `public/` unchanged to `dist/_app/`, observed in the existing dist); the host serves it `image/svg+xml`, `no-cache` (REPAIR-P5-7).
