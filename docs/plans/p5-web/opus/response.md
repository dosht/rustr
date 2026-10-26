# P5 cross-critique: Opus response and convergence

Reviewer `w23:p1V`, preserved Opus session, 2026-10-25. This response answers Sol's direct reply to `opus/critique.md`. Both original proposals stay frozen for the independent Fable review. Read-only: no implementation, checks, browser or provider calls.

## Outcome

Sol and I converged on every point. Sol's conditions on H2 and H4 are correct and sharpen the dispatch contract. Further source evidence supports Sol's H4 condition, so I correct my own proposal there (below). No open disagreement remains between the advisors; section 3 lists the lead's decisions.

## 1. Sol's conditions, accepted

**H1, AI start.** The client posts to the existing source route with a client-generated `requestId` UUID and reads `x-generation-id` (set in `transgate/http/src/ai.rs:279-280`). It then drops the body promptly, which releases the per-account watch slot taken by the POST attachment (P4 cap of 4 per account), and attaches the canonical `/watch`. There is one decoder per kind: `ApplicationDecoder` with `StreamState` for text, and named analysis events over `SseDecoder`.

- No `Accept: application/json` host change and no legacy chat-body decoder.
- Network loss never re-starts generation. If the header was received, reattach `/watch?after=<durable_seq>`. If the POST outcome is unknown, re-POST with the **same** `requestId`. P4 guarantees an exact supplied-key replay returns the same generation without dispatching (`hosts.md:47`). A user retry is new metered work with a new key.

**H2, document bytes.** The direction is shared host bytes, so P8 reuses them and there is no duplicate browser pipeline. The existing transcript OOXML writer does not implement the six analysis shapes; I make no such claim. Dispatch must specify:

- **Writer.** A format-neutral `AnalysisDocument` (and `InvoiceDoc`) model in Rust. DOCX is written with the existing `transgate_export` OOXML parts, extended for headings, lists and tables. PDF uses one pure-Rust writer chosen by the lead (D-1).
- **Layout.** Section order, headings, rating and star text, and file names follow the source exporters (`docx-exporter.ts`, `pdf-exporter.ts`, the `InvoicePDF` component in `billing/index.tsx:155-338`). Equivalence is judged per shape, not "a PDF exists".
- **Fonts and glyphs.** Embedded Noto Sans (Latin, Greek, Cyrillic, which covers all 16 UI locales) and Noto Sans Arabic. Arabic shaping and right-to-left order must match a reference document produced by the source exporter. If the chosen writer cannot shape Arabic correctly, record the gap before dispatch; do not ship broken glyphs. CJK coverage stays at source level, which loads only an Arabic font. This is recorded, not expanded.
- **Evidence.** An independent parser (not the writer) reads text and structure from each of the six analysis shapes, one invoice and one admin Bill To invoice. These are compared with source-generated reference outputs from the read-only repository, run by the E4-style capture owner.

**H3, locales.** Keep an explicit source fallback manifest equal to the LNG-D08 baseline. Repair the `de/app.json` parse error. A new or unlisted fallback fails the test. All 16 locales are exercised on both Playwright projects. New translations need recorded owner approval and then shrink the manifest.

**H4, auth.** Sol's condition reflects the actual source auth flow, and my proposal was wrong to say "parsing of the redirect fragment session". The source browser client is `@supabase/ssr` `createBrowserClient` (`src/utils/supabase/client.ts:2,36`). Its installed version 0.3.0 sets `flowType: "pkce"`, `detectSessionInUrl` and `autoRefreshToken` (`node_modules/@supabase/ssr/dist/index.js:154-156`). OAuth (`signin.tsx:157`, `SimpleSignInPopup.tsx:60`), magic link and recovery therefore return with `?code=`, exchanged with a stored code verifier.

The Rust `GoTrueAuth` in `transgate-client` must implement:
- the PKCE S256 verifier and challenge;
- `/authorize?provider=google&code_challenge=…&redirect_to=…`;
- `/otp` and `/recover` carrying the challenge;
- `POST /token?grant_type=pkce` exchange;
- the `PASSWORD_RECOVERY` → `/new-password` update path;
- `refresh_token` rotation.

The browser platform port supplies:
- persistent storage for the verifier across the redirect;
- `navigator.locks` so a single tab refreshes, plus a `storage` event so the other tabs adopt the rotated session;
- navigation for redirects;
- callback URL parsing, after which the `code` is removed from history.

Tests must cover callback, code exchange, recovery, expired code and two tabs with rotation, not only the password happy path. P8 reuses the same Rust flows with native storage and browser-handoff ports.

**Smaller agreements.**
- Analysis cancel is in scope (`DesktopSummaryView` `onCancel`); chat has no cancel (`useChatViewModel.ts:30-34`).
- `GET /api/session`, canonical redirects for duplicate list routes, billing presentation with the billing module owner, and Sol's six feature-owner slices with one integration owner.

## 2. Converged design delta against both originals

| Area | Converged |
|---|---|
| Crates and app | `transgate/client` (native and wasm, including GoTrue PKCE), `transgate/wasm` (one composed presenter per session, ts-rs types, fixture export), `transgate/web` (Svelte 5, Vite, Tailwind v4, i18next core). No taskr change |
| New pure presentation | app/session/route, settings, consent, analysis and chat panels, feedback, support, checkout, invoices, flags, and `Msg` keys (my §2.2). Billing `purchase` and `InvoiceDoc` live in `modules/billing` |
| New host seams | `GET /api/session`; `POST /api/jobs/start` (browser session only, same parser and use case; P7 owns only the public-API/key boundary such as `/api/v1/transcriptions`, not the session-owned `/api/v1/ai/*` starts, which stay unchanged per H1); `POST /api/jobs/{id}/feedback`; `POST /api/support` with ordered mails; owner erasure-status GET; owner invoice context including latest `billing_info`; invoice and analysis document bytes (§4 R-2); HTML adapter on browser `POST /checkout/confirm` over the single confirmation; SPA serving with explicit app routes and an API JSON 404; Docker web stage |
| Unchanged host | AI start routes (H1), billing checkout start, all P2-P4 policy |
| Device and layout | Touch and width < 1024 px with the mobile flags. One mounted branch, one player and one watch. Landscape notice made accessible |
| Phone baseline corrections | No filter controls; pull-to-refresh disabled on all-transcripts; detail export filters translation lines. All three are inventory corrections for the lead to record |
| Realtime | Source polling cadence, visibility-scoped. No push channel |
| Tokens | Generated through `gen-tokens.py` into `transgate/web`, Paper Moss default, additive `success`/`progress` roles through the lead |
| Phase boundaries | P6: public, legal, pricing, homepage popup, tools. P7: key backend delivered early, with views on the P5 shell and no entry until then; API docs; the public-API/key route boundary. P8: WEB-049..076 consume the same client. P9/P10: live origin, OAuth, payment and migration. **P6 session presence:** `localStorage['transgate.session']` is only a hint. Before a signed-in redirect from `/` or suppressing the popup, P6 verifies it with `GET /api/session`. A 401 or an expired session clears the hint and stays on the public page, with no redirect loop |

## 3. Lead decisions, recommendations agreed by both advisors

| ID | Decision | Joint recommendation |
|---|---|---|
| D-1 | Document bytes location and writer | **Joint, final (token `P5-DOCS-FINAL-A`):** option (a) in §4 R-2. One in-memory Rust writer adapter in or beside `transgate/export`, over the core `AnalysisDocument` and `InvoiceDoc` models, building for wasm32 and native. The browser runs it through wasm; P8 runs the same crate through its binding. It works on the captured typed model on screen plus the click time. Shell ports supply fonts and save or share only. It embeds or bundles licensed fonts (Noto Sans, Noto Sans Arabic) and shapes Arabic, proven per shape against six source reference outputs with an independent parser. No analysis-export host route, no JS `docx`/`jspdf`, no generic document engine and no snapshot history |
| D-2 | Locale gaps | Baseline fallback manifest, `de/app.json` repair, all 16 tested; translation filling only with recorded owner approval |
| D-3 | Auth | Rust GoTrue PKCE adapter shared with P8, browser lock/storage/navigation ports |
| D-4 | Browser job start | New session route `POST /api/jobs/start`, migrating current consumers, no credential-format fallback on `/api/v1/transcriptions` |
| D-5 | Corrections | Record UI-01 (phone honors toggles; no new all-formats action), UI-03 (suppress popup when signed in, P6 surface), UI-04 (typed start and support errors), UI-05 (actual provider or explicit unknown) |
| D-6 | Inventory corrections | Phone filters unmounted; all-transcripts pull-to-refresh disabled; phone detail export filters translations (EXP-013 stale) |
| D-7 | Trust items | Turnstile: preserve placement and label it decorative unless the token is wired to GoTrue as a recorded correction. Browser telemetry: PostHog and Sentry behind cookie consent; GA, Hotjar and New Relic need an explicit keep or drop record, with a default of drop |
| D-8 | P7 sequencing | Key service delivered before the P5 key screen closes; no menu entry, route or fake backend before then |
| D-9 | Thrown rows | `dropped` records with `mu approved` for WEB-BIL-018/019, disabled dashboards and prototypes (UI-02); voice demo already excluded (P4 D6) |
| D-10 | Cancel and display-total refinements | No analysis or chat cancel in the baseline (§4 R-1). The purchase screen keeps a separate `display_total` and `charged_quote` (§4 R-3), with `display_amount` added to the quote response |

## 4. Addendum: three source refinements from Sol, all accepted

**R-1. Analysis cancel is not baseline. Withdrawn.** I repeated the same unused-prop mistake I had just pointed out for the phone filters. `DesktopSummaryView.tsx:55,65` and `MobileSummaryView.tsx:58,72` only declare and destructure `onCancel`. The only rendered cancel button is `AnalysisProgress` reached through `AnalysisViewPresentation:84`, inside `AnalysisContainer`. The container is marked "legacy MVVM container, active path uses useAIFeatures" (`features/aiAnalysis/containers/AnalysisContainer.tsx:17`). `AnalysisView` is imported by no page; only its stories import it. The mounted path is `Transcript.tsx:250-271` over `DesktopSummaryView`/`MobileSummaryView`, and it has no cancel control. Baseline therefore has no analysis cancel and no chat cancel. The P4 server cancel route stays a capability that P5 does not surface. Supersedes the analysis-cancel line in §1.

**R-2. Analysis export must use the analysis on screen, not the latest saved one.** The source shows copy and export whenever `hasData`, including partial, loading and interrupted states (`DesktopSummaryView.tsx:160-182`). It exports the data currently on screen, stamped with the time of the export (`useExportAnalysis.ts:60-72`). A host route that loads "the job's analysis" would silently export the latest completed one, or something other than what the user is looking at. Final joint choice is (a), token `P5-DOCS-FINAL-A`. After crossed messages, this supersedes every earlier (c) wording:

- **(a) Chosen: in-shell Rust writer adapter.** At click time, core builds a typed `AnalysisDocument` from the typed progress or result on screen, plus title, analysis type, job id and export time. One in-memory adapter in or beside `transgate/export` writes DOCX, PDF, TXT and MD bytes. It builds for wasm32 and native: the browser calls it through `transgate-wasm`, and P8 calls the same crate through its binding. The shell only supplies font bytes or assets and the save or share action; there is no second policy pipeline. Nothing is sent to or stored on the host for analysis export.
- **(b) Rejected: host bytes keyed by generation and displayed sequence.** It cannot export a partial that loading or interruption left only in view state.
- **(c) Considered, not chosen: host bytes from a captured payload.** It would need a new host route and validation of an uploaded payload, for data the shell already holds.
- **Invoices.** The owner's invoice context is a real API. All financial document facts, including VAT, are computed by billing core on the server and returned in that DTO. The adapter only lays them out. The temporary admin Bill To is the only user-supplied text, is kept apart from the financial facts and is never stored.

In every option the writer, layout, font and Arabic-shaping evidence contract in §1 H2 still applies, and partial or interrupted content is exported exactly as shown.

**R-3. The purchase screen shows two different PAYG totals on purpose.** My original wording, "prices always come from `/api/billing/quote`", needs correcting. Under DEF-010, the price shown for `id`/`pt` locales is `payg_display_total` (`modules/billing/core/src/pricing.rs:296-307`). The charged amount is the quote: for 10 hours in `id`, the screen shows 7.40 and the charge is 7.45 (`pricing/tests.rs:173-180`). P2 D2 explicitly keeps this display-versus-charged distinction. The `purchase` presentation must therefore carry two separate fields:

- `display_total`, from `payg_display_total`;
- `charged_quote`, from the quote.

It must never collapse them or compute either in JS. `payg_display_total` is only in core today and is not exposed over HTTP. Recommended: add `display_amount` to the quote response, computed by the host. This keeps the locale-discount list on the server. It is additive and decides nothing new.
