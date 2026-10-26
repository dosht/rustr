# P5 web third review (Fable)

Reviewer `w23:p27`, anthropic/claude-fable-5-1, HIGH reasoning, 2026-10-25. Single bounded review of the frozen `opus/proposal.md` and `sol/proposal.md` against `brief.md`. No sibling critiques read, no coordination, no source edits, no builds. Five targeted source reads were used to settle concrete claims; they are cited inline. Everything else is read from the two proposals; unverified claims are marked **[unverified]**.

## Verdict

The proposals agree on the architecture that matters: separate `transgate/{client,wasm,web}`, existing Rust reducers plus new pure presentation, server-derived capabilities, feedback/support/`me` as real host routes, HTML confirm over the single `confirm_checkout` result, polling instead of a push channel, generated fixtures, two Playwright projects, Axum-served image. Reconcile on the seven disagreements below; otherwise take the union of their inventories. Opus is tighter on seams and ownership; Sol is more accurate on source parity. Three Sol parity claims that contradict Opus are confirmed by source.

## Material disagreements

| # | Topic | Opus | Sol | Evidence | Decision | Severity |
|---|---|---|---|---|---|---|
| 1 | Phone history filters | Renders "cards, filters, swipe delete, pull to refresh"; slice D has "phone filters" adapters | Filter sheet not mounted; props unused | `MobileJobListView.tsx:44-51,172-175` destructures `filter/sort/onFilterChange/onSortChange` and never mounts a control; header comment `:10` is stale. `JobListContainer.tsx:63-64,153` sets `disablePullToRefresh` on the all-transcripts view | **Sol.** No filter UI. Pull-to-refresh only on the sidebar list, not `?view=all-transcripts`. Lead records the ledger clarification | High: Opus would ship an invented control as parity |
| 2 | Phone export semantics | "Action-sheet download; source omits toggles (UI-01)" | Phone detail filters translation lines; TranscriptPage semantics | `MobileJobDetailsContainer.tsx:111-115` keeps only `translation_status === 'translation'` lines for translation jobs | **Sol.** Phone detail export = TranscriptPage request with translation filter and source-omitted toggles. UI-01 stays a toggles-only correction. EXP-013/core comment correction goes to the lead | Medium |
| 3 | Analysis/invoice document bytes | Server Rust: new unnamed pure-Rust PDF writer with embedded fonts, `POST /api/ai/jobs/{job}/analysis/export`, `GET /api/billing/invoices/{payment}.pdf` plus admin POST | Rust document model, browser `docx`/`jspdf` byte adapters, self-hosted fonts, same encoder for invoices | Source renders both client-side. Opus's route set touches the P4 AI host and billing HTTP mid-integration; the PDF crate is unnamed and Arabic needs shaping, which no proposal addresses | **Sol**, with the shared Rust document model both propose (`InvoiceDoc`, analysis export model). No new routes. P8 reuse is speculative; move the encoder server-side only when a P8 row needs bytes without a browser | Medium |
| 4 | Catalog runtime | Hand-written TS resolver over `Intl.PluralRules` | i18next core, Svelte binding | Source catalogs use i18next v4 semantics (`_one/_other`, `{{var}}`, and whatever nesting/context the 6 namespaces contain **[unverified]**). A hand resolver must reproduce them exactly and is tested only by the keys it happens to hit | **Sol runtime + Opus manifest.** i18next core with `fallbackLng: false` and a missing-key handler that fails tests; `fallback-manifest.json` is the only permitted English fallback and must equal the LNG-D08 baseline | Medium |
| 5 | Locale gap closure | Manifest equal to baseline; filling the 152 `ai` keys deferred to mu | Translate every reachable key before locale acceptance | Brief: no silent English fallback. A manifest makes the fallback explicit but a localized screen showing English cannot close its LOC row | Manifest now (mechanism). LOC rows stay amber until the manifest is empty for P5 screens. This is the one real mu decision: authorize filling reachable keys inside the shell/localization slice, mu reviews meaning | Medium |
| 6 | Job start route and the P7 credential boundary | Keep `/api/v1/transcriptions` (session JWT) | New `POST /api/jobs/start`, migrate consumers, reserve `/api/v1/transcriptions` for P7 keys | No fact dispute. The cutover edits a P3 host route while P3/P4 integrate | **Opus.** No cutover in P5. Record a P7 obligation: when key auth mounts, credentials are typed by structure (JWT vs key prefix) and verified independently; that is not a permissive fallback. If P7 prefers separate paths, P7 migrates then | Low |
| 7 | AI start transport | P5-D4: additive `Accept: application/json` start answer (new P4 host work) | Capture `x-generation-id` from the start response | `transgate/http/src/ai.rs:279-280` returns `text/event-stream` unconditionally with `x-generation-id`; no negotiation exists. Dispatch at `:100-103` runs independently of the response body **[inference: spawn body not read]** | **Sol.** Client reads the header, cancels the start body, attaches `/watch` with the existing decoders. No host change; D4 is optional later | Low |

Naming only: Opus `GET /api/me` vs Sol `GET /api/session`. Pick `/api/me`.

## Missing end-to-end dependencies

| Gap | Who misses it | Evidence | Decision |
|---|---|---|---|
| Payment status read for URL-only checkout returns | Opus `checkout` machine confirms only via the form POST | `modules/billing/http/src/routes.rs:104` already mounts `GET /billing/payments/{id}/status`. Sol's "uncertain stays unconfirmed, offer a status read" needs it | Add `PaymentStatus` read to the `checkout` state machine and client; e2e scenario for a hosted return without the POST |
| Invoice facts in the owner history DTO | Both, more acute under client-side PDF | Bill To/VAT fields must be present in `/api/billing/history` or an owner read; neither proposal verifies the current DTO **[unverified]** | Billing owner checks `history_row` and the history DTO first; additive owner fields only if missing. No admin authority for owners |
| List refresh for running jobs not open | Opus polls only the open job (`RUNNING_POLL_MS`) and drafts | Source realtime updated list rows; the keep-throw gap is a web shell topic | Sol's visibility-scoped list poll while any loaded row is non-terminal; dedupe by id; cursor not reset |
| Captcha token | Opus "when configured"; Sol claims source sends no verified token **[unverified]** | GoTrue accepts a captcha token on signup/token/otp/recover | Pass the token to GoTrue when the site key is configured; no widget and no local boolean otherwise. Live enforcement stays a deployment gap |
| Telemetry timing change | Both end consent-gated; Sol notes source loads GA/Hotjar/NR unconditionally **[unverified]** | Changed script timing is a UX/legal change | Lead records it as a correction, like UI-01..05 |
| `dropped` records | Opus names them (WEB-BIL-018/019, UI-02, voice-agent-test, duplicate all-transcripts route) | `docs/gates.md` requires `mu approved` | Lead action before any P5 gate run |
| `footer` namespace | Opus: requested by `confirm.tsx:122`, absent in `en` | Confirm page copy | Put the two keys in `common`; record |
| Token roles `success`/`progress` (P5-D7) | Both defer to lead | `Tone::{Success,Progress}` exist in core | Add the two roles with contrast checks; Paper Moss default unchanged |
| Session presence for P6 `/` | Opus D9 localStorage; Sol wants root middleware coordination | Source redirects signed-in visitors server-side | Accept D9; record "server redirect → client-side redirect" as a change; same-origin is a P9 prerequisite, not a P5 proof |

## Unnecessarily broad abstractions

| Item | Proposal | Decision |
|---|---|---|
| Owner erasure status `GET` (fenced owner reads receipt after sign-out) | Sol 2.3 #5 | **Drop.** No WEB row needs it: the POST returns the `ErasureView`, the view renders it before the established sign-out boundary, and `Requested` closes access. A "last observed receipt after auth expiry" is local memory, not a route |
| `Msg{ns,key,args}` migration of every P3/P4 copy-bearing VM field | Opus 2.2 | **Narrow.** New modules emit keys plus typed args from day one. Existing modules get additive `*_key`/args fields written by their P3/P4 owners; English fields are not removed in P5. Removal is a P8/P10 cutover once no shell reads them |
| Server analysis export route and admin invoice POST | Opus 2.4 | Dropped by disagreement #3 |
| Nine implementation slices with one core policy owner (A) feeding four view owners | Opus 5 | **Hybrid, seven owners:** (1) core/client/binding root: `app`, `route`, session, `msg`, `flags`, presenter, generated contracts; (2) host: `/api/me`, feedback, support mail, SPA serving, Docker web stage; (3) billing: `purchase`, `InvoiceDoc`, HTML confirm, payment-status wiring; (4) workspace: upload/history/transcript/player views plus their platform adapters; (5) AI: `ai_consent`, `analysis_panel`, `chat_panel` presentation **and** views; (6) auth/settings/commerce views plus shell/localization/theme/Storybook; (7) verification runtime (`playwright.config`, e2e, loopback fixtures, extended runtime smoke). Feature owners write their own Rust presentation leaves (Sol), the root owner integrates exported-symbol changes |
| `/api/jobs/start` transport cutover | Sol 2.3 #2 | Dropped by disagreement #6 |

## Acceptance holes

| Hole | Required proof |
|---|---|
| Arabic/RTL in browser-encoded PDF | e2e downloads an Arabic analysis PDF and an invoice; independent extraction must match the fixture text. Failure here is the only trigger to revisit server-side PDF |
| Confirm page | One browser form POST to `/checkout/confirm` renders HTML with escaped embedded `ConfirmView`, countdown fires once, reload does not reconfirm, GET gives the source failure page. Both proposals state parts; require all four in one spec |
| Uncertain payment | Hosted return without a POST shows unconfirmed state and a status read against `payments/{id}/status`; never success from a query parameter |
| Phone list | Assert absence of filter controls and absence of pull-to-refresh on `?view=all-transcripts` (negative assertions prevent regression toward disagreement #1) |
| Phone export | ZIP from phone detail of a translation job contains translation lines only; desktop same job with toggles differs per UI-01 until recorded |
| Analysis start | Generation attaches via header id with the start body cancelled; the same generation is observed after reload (no double dispatch from layout remount) |
| Locale | Per locale, both projects: catalog loads, representative screens render no key from outside the manifest; `de/app.json` parses (LNG-D07) |
| Landscape blocker | Preserve rotation condition; notice is reachable by assistive tech and the hidden app is not focusable (Sol). Opus keeps `aria-hidden` verbatim; adopt Sol's a11y fix as a recorded correction |
| SPA fallback | Unknown `/api/*` stays JSON 404; `/checkout/confirm` POST, `/health` and provider routes precede the fallback; P6 public paths are not blanket 200 |

Both proposals correctly separate synthetic proof from live gaps (OAuth, Turnstile, SMTP/Resend, Iyzico 3-DS, storage CORS, PostHog/Sentry, YouTube quotas, provider quality, deployment origin). Keep Opus's list as the canonical live-gap record.

## Smallest correct combined plan

1. Opus sections 1, 2.1, 2.3, 2.5 and 3 as the inventory, seams and step order, corrected by disagreements #1 and #2 and the Sol parity clarifications (phone filters unmounted, all-transcripts refresh exception, phone translation filter).
2. Opus 2.4 backend gaps minus the analysis export route, invoice PDF routes and JSON AI start; plus the payment-status read already mounted.
3. Sol 3.3 document handling: Rust document models, browser `docx`/`jspdf` encoders, self-hosted fonts, chat TXT export with share fallback.
4. Sol i18next core plus Opus fallback manifest and catalog tests; LOC rows amber until the manifest is empty for P5 screens.
5. Opus decisions P5-D1 (API keys with P7), D2 (HTML confirm, no PRG), D3 (new GoTrue adapter, taskr untouched), D5 replaced by item 3, D6 (polling, with Sol's list poll), D7, D8 (as item 4), D9, D10, D11, D12. Drop D4.
6. Seven-owner split above; lead prerequisites as Opus 5 (workspace members, lockfile, `transgate/web/package.json`, `gen-tokens.py` outputs, locale vendoring script and manifest baseline, runtime harness env contract).
7. Verification: Opus 6 table plus Sol 7.1 scenario list plus the acceptance holes above. Sol's rule "a pair may share a scenario, never one platform's pass" is the counting rule.

## Decisions for the lead

1. Confirm #1, #2 as ledger clarifications; approve negative assertions in e2e.
2. Approve client-side document bytes (#3) and i18next core with manifest (#4).
3. Decide locale filling (#5): recommend authorizing reachable-key translation in the shell/localization slice, mu reviews meaning.
4. Keep `/api/v1/transcriptions` for P5; record the P7 typed-credential obligation (#6).
5. Drop P5-D4 and the erasure status GET; narrow the `Msg` migration to additive fields.
6. Record `dropped` entries, UI-01..05, telemetry timing, landscape a11y and `footer` key placement before gates run.
