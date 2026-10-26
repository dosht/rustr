# P5 parity disposition

Owner: p1Y (independent tester). Read-only analysis of existing receipts; no new runs. Lead decides acceptance.

## Evidence

| Tag | Snapshot | Receipts |
|---|---|---|
| c7 | candidate7 `/home/mu/.cache/rustr-integration/s1-20261026215323`, manifest `c3c3d5afbdeb…` | `logs/next-p5-c/report.json`, `c2-action-receipts.json`, `c2-network.json`, `c2-locale-final.json`, C3 packaged image `sha256:1efb4e84821a…`, screenshots |
| c5 | candidate5 `/home/mu/.cache/rustr-integration/s1-20261026204512` | `logs/next-p5-c/c2-action-receipts.json`, `c2-invoice-text.txt`, `c2-phone-export-inspection.json` |
| c4 | candidate4 `/home/mu/.cache/rustr-integration/s1-20261026195749` | cited only as stale context, never as proof |

Rules applied:
- **PROVEN**: every clause of the row was exercised in a real browser flow on the row's own platform (Desktop Chrome 1280×800 for `web-desktop`, Pixel 7 for `web-phone`) against the real host, wasm and owned loopback runtime, on c7, or on c5 when no input on that path changed afterwards.
- **GAP**: any clause not exercised. Partially exercised rows stay GAP; the evidence column says what was proven.
- **P6 / P7 / P8**: owned by another phase per D13 and opus proposal §1.1/§1.3.
- **THROWN**: proposal §1.3 and D14; red until the canonical `dropped` record with mu approval exists.
- C1 generated-fixture screenshots and axe results (c5 full matrix 3011/3012 audited, c7 targeted 1116/1116, 0 violations) support rendering only; they never prove a flow.
- Local loopback fixtures never establish live OAuth, captcha, payment, provider, SMTP or telemetry acceptance.

## Counts

| Disposition | Rows |
|---|---|
| PROVEN | 31 |
| GAP | 94 |
| P6 | 8 |
| P6/P7 | 2 |
| P7 | 2 |
| P8 | 28 |
| THROWN | 2 |
| **Total** | **167** |

P5-owned rows: 125; proven 31, gap 94. The PRD P5 gate (web parity green on both Playwright projects) is **not met**.

## Rows

| Row | Platform | Flow | Disposition | Evidence | Remaining |
|---|---|---|---|---|---|
| WEB-001 | web-desktop | Password sign-in, signup, magic link, Google and recovery (screen) | GAP | c7 desktop password, signup, magic link, recovery all PROVEN (see WEB-IDN rows) | Google not exercised |
| WEB-002 | web-phone | Password sign-in, signup, magic link, Google and recovery (screen) | GAP | c7 phone password sign-in | phone signup, magic link, recovery, Google not exercised |
| WEB-003 | web-desktop | Set new password (screen) | PROVEN | c7 recovery: form, submit, result `Password updated successfully!` | redirect after save not asserted |
| WEB-004 | web-phone | Set new password (screen) | GAP |  | phone new-password screen not exercised |
| WEB-005 | web-desktop | First-visit sign-in and cookie prompts (screen) | GAP | cookie reject proven desktop (c5) | cookie accept not exercised; homepage popup part is P6 (D13) |
| WEB-006 | web-phone | First-visit cookie prompt (screen) | GAP | phone Reject clicked (c7) | accept and telemetry gating not asserted on phone |
| WEB-007 | web-desktop | Upload files and start transcription (screen) | PROVEN | c7 `upload transcript`: pick file, signed PUT, language, Start, navigate to `/home/1`; C3 packaged repeat | speaker toggle not exercised |
| WEB-008 | web-phone | Upload files and start transcription (screen) | PROVEN | c7 `phone short refusal` flow: phone upload, Ready, language, Start, navigate `/home/2` |  |
| WEB-009 | web-desktop | Restore drafts and preparation/error states (screen) | GAP |  | draft restore and preparation/error states not exercised |
| WEB-010 | web-phone | Restore drafts and preparation/error states (screen) | GAP |  | draft restore and preparation/error states not exercised |
| WEB-011 | web-desktop | Add YouTube and start (screen) | GAP |  | YouTube add/resolve not exercised |
| WEB-012 | web-phone | Add YouTube and start (screen) | GAP |  | YouTube add/resolve not exercised |
| WEB-013 | web-desktop | Translate source to target (screen) | GAP |  | translation job not exercised |
| WEB-014 | web-phone | Translate source to target (screen) | GAP |  | translation job not exercised |
| WEB-015 | web-desktop | List, page, select, refresh and delete jobs (screen) | GAP | c4 desktop selected a sidebar history item | paging, refresh, delete confirm not exercised |
| WEB-016 | web-phone | List, filter, select, refresh and delete jobs (screen) | GAP |  | phone list filters, swipe delete, pull refresh not exercised |
| WEB-017 | web-desktop | View live transcript and playback (screen) | GAP | c7 completed transcript rendered with player; c4 fixture recorded a signed ranged GET | seek, waveform, timestamp and speaker toggles not exercised |
| WEB-018 | web-phone | View live transcript and playback (screen) | GAP | c7 phone deep link rendered transcript and player | phone playback, tabs swipe, action sheet not exercised |
| WEB-019 | web-desktop | Edit line and speaker labels (screen) | GAP | c4 desktop line edit and save (stale) | speaker label edit/rename not exercised; no c7 desktop line edit |
| WEB-020 | web-phone | Edit line and speaker labels (screen) | GAP | c5 phone line edit and save | speaker labels not exercised |
| WEB-021 | web-desktop | View translation and gated original (screen) | GAP |  | translation view and gated original not exercised |
| WEB-022 | web-phone | View translation and gated original (screen) | GAP |  | translation view and gated original not exercised |
| WEB-023 | web-desktop | Freemium transcript fade and upgrade (screen) | GAP |  | freemium fade not exercised |
| WEB-024 | web-phone | Freemium transcript fade and upgrade (screen) | GAP |  | freemium fade not exercised |
| WEB-025 | web-desktop | Export TXT, Word, SRT, VTT and CSV ZIP (screen) | GAP | c4 desktop ZIP (Full Text.txt, .doc) parsed | SRT/VTT/CSV "all formats" and toggles not exercised; c4 stale |
| WEB-026 | web-phone | Export transcript/translation ZIP (screen) | PROVEN | c5 `phone transcript edit and export download`: ZIP parsed independently, edited text present (export path unchanged c5→c7) |  |
| WEB-027 | web-desktop | AI consent, analysis, highlights, retry and export (screen) | GAP |  | AI analysis, highlights, retry, export not exercised |
| WEB-028 | web-phone | AI consent, analysis, highlights, retry and export (screen) | GAP |  | AI analysis, highlights, retry, export not exercised |
| WEB-029 | web-desktop | AI chat, limits, copy, retry, rating and export (screen) | GAP | c7 own-tab question, streaming partial, answer, second-tab reattach (+1 watch, 0 extra start, 1 provider call), limits `1 / 2` | copy, retry, rating, export not exercised |
| WEB-030 | web-phone | AI chat and limits (screen) | GAP | c7 phone short-transcript refusal (exact 400 copy); c5 phone positive stream predates the c6/c7 chat-leaf change | positive phone stream not re-run on c7 |
| WEB-031 | web-desktop | Job feedback (screen) | PROVEN | c7 `support feedback` (POST feedback 201, Thank You) |  |
| WEB-032 | web-phone | Job feedback (screen) | GAP |  | phone feedback not exercised |
| WEB-033 | web-desktop | Contact support (screen) | PROVEN | c7 `support feedback` (POST `/api/support` 202, dialog closes); focus returns to Support (c5) |  |
| WEB-034 | web-phone | Contact support (screen) | GAP |  | phone support not exercised |
| WEB-035 | web-desktop | Balance, choose package, coupon and checkout (screen) | GAP | balance, package, checkout proven desktop (c7) | coupon not exercised |
| WEB-036 | web-phone | Balance, choose package, coupon and checkout (screen) | GAP |  | phone purchase not exercised |
| WEB-037 | web-desktop | Invoice history, PDF and privileged lookup (screen) | GAP | history and PDF download proven desktop (c5) | privileged lookup not exercised; preview content unproven headless |
| WEB-038 | web-phone | Invoice history and PDF (screen) | GAP |  | phone invoices not exercised |
| WEB-039 | web-desktop | Profile, theme and locale settings (screen) | GAP | locale-prefixed routes proven (LOC rows) | theme/palette switching and settings locale change not exercised |
| WEB-040 | web-phone | Profile, theme and locale settings (screen) | GAP | locale-prefixed routes proven (LOC rows) | theme/palette switching and settings locale change not exercised |
| WEB-041 | web-desktop | AI consent, subscription and data settings (screen) | GAP |  | settings AI consent, subscription, data tabs not exercised |
| WEB-042 | web-phone | AI consent, subscription and data settings (screen) | GAP |  | settings AI consent, subscription, data tabs not exercised |
| WEB-043 | web-desktop | Delete account and sign out (screen) | GAP | delete account proven desktop (c7) | sign out not exercised |
| WEB-044 | web-phone | Delete account and sign out (screen) | GAP |  | phone delete account and sign out not exercised |
| WEB-045 | web-desktop | API-key list, create, copy and revoke (screen) | P7 | D13; proposal §1.3 (API keys) |  |
| WEB-046 | web-phone | API-key list, create, copy and revoke (screen) | P7 | D13; proposal §1.3 (API keys) |  |
| WEB-047 | web-desktop | API docs, contact, certificate and error navigation (screen) | P6/P7 | D13; proposal §1.3 (API docs P7; contact, certificate, public 404 P6) | P5 app-route 404 fallback not separately asserted; C3 c7 proved `/api/*` JSON 404 and `/assets/*` 404 |
| WEB-048 | web-phone | API docs, contact, certificate and error navigation (screen) | P6/P7 | D13; proposal §1.3 (API docs P7; contact, certificate, public 404 P6) | P5 app-route 404 fallback not separately asserted; C3 c7 proved `/api/*` JSON 404 and `/assets/*` 404 |
| WEB-049 | cli | Sign in and sign out | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-050 | tui | Sign in and sign out | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-051 | android | Sign in, signup, recovery and sign out | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-052 | ios | Sign in, signup, recovery and sign out | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-053 | cli | Upload and transcription/translation configuration | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-054 | tui | Upload and transcription/translation configuration | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-055 | android | Upload and transcription/translation configuration | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-056 | ios | Upload and transcription/translation configuration | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-057 | cli | YouTube source submission | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-058 | tui | YouTube source submission | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-059 | android | YouTube source and playback handoff | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-060 | ios | YouTube source and playback handoff | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-061 | cli | List jobs, status, fetch transcript/translation and delete | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-062 | tui | List jobs, status, fetch transcript/translation and delete | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-063 | android | List jobs, status, transcript editing and playback | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-064 | ios | List jobs, status, transcript editing and playback | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-065 | cli | Transcript/translation export | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-066 | tui | Transcript/translation export | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-067 | android | Transcript/translation export | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-068 | ios | Transcript/translation export | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-069 | android | AI consent, analysis, highlights and streamed chat | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-070 | ios | AI consent, analysis, highlights and streamed chat | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-071 | android | Balance, purchase and invoice browser handoff | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-072 | ios | Balance, purchase and invoice browser handoff | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-073 | android | Settings, retention, consent and account deletion | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-074 | ios | Settings, retention, consent and account deletion | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-075 | android | Feedback, support and API administration handoff | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-076 | ios | Feedback, support and API administration handoff | P8 | proposal §1.1 and §1.3 (native/CLI/TUI shells) |  |
| WEB-BIL-001 | web-desktop | View the balance in the user area and on the all-transcripts page (minutes under 60, else hours and minutes, r | GAP | c7 desktop user area shows balance in minutes and hours (`19 min`, `10 hr , 19 min` after purchase) | all-transcripts page balance and 30 s refresh not observed |
| WEB-BIL-002 | web-phone | View the balance in the drawer and bottom sheet | GAP | c5 phone drawer showed `Balance 10 hr , 19 min` | bottom-sheet balance not observed; drawer evidence predates c7 |
| WEB-BIL-003 | web-desktop | Open the billing dialog and view packages (pay-as-you-go card, monthly or yearly toggle, Premium and Business  | PROVEN | c7 `Iyzico handoff HTML confirm` opened the dialog (PAYG, monthly/yearly, Premium/Business, summary, quote `$14.90`); C3 packaged run repeated it | Turkish card note (TRY only) not asserted |
| WEB-BIL-004 | web-phone | Open the billing bottom sheet and view packages | GAP |  | phone bottom sheet never opened |
| WEB-BIL-005 | web-desktop | Choose pay-as-you-go hours (presets 5, 10, 25, typed value, arrows, minimum 5 warning) | GAP | default 10 h quote only | presets, typed value, arrows, minimum-5 warning not exercised |
| WEB-BIL-006 | web-phone | Choose pay-as-you-go hours | GAP | default 10 h quote only | presets, typed value, arrows, minimum-5 warning not exercised |
| WEB-BIL-007 | web-desktop | Apply a coupon (error tooltip, green check when applied) | GAP |  | coupon never applied (needs a seeded coupon) |
| WEB-BIL-008 | web-phone | Apply a coupon | GAP |  | coupon never applied (needs a seeded coupon) |
| WEB-BIL-009 | web-desktop | Buy a package: accept terms, press Pay with Card, land on Iyzico's hosted form, pay | PROVEN | c7 `Iyzico handoff HTML confirm` and C3 `C3 checkout confirm erasure no iyzipay`: terms, Pay with Card, D17 document, signed loopback Iyzico form, payment | loopback fixture only; live Iyzico is a deployed gate |
| WEB-BIL-010 | web-phone | Buy a package from the bottom sheet | GAP |  | phone purchase not exercised |
| WEB-BIL-011 | web-desktop | Checkout success page, countdown, return home | GAP | c7 embedded `tg-confirm` = `confirmed`, analytics once | visible success copy and 5 s countdown not asserted; return home targets the public root (P6, 404 today) |
| WEB-BIL-012 | web-phone | Checkout success page, countdown, return home | GAP |  | phone confirmation page not exercised |
| WEB-BIL-013 | web-desktop | Checkout failure page and the "We couldn't start your checkout" page with a back-to-plans link | GAP | c4 desktop showed the start-failure page with Back to plans (c4 checkout renderer since changed; not still-valid) | Iyzico `failure`/`unconfirmed` outcomes and start failure not run on c7 |
| WEB-BIL-014 | web-phone | Checkout failure page and the start-failure page | GAP | c4 desktop showed the start-failure page with Back to plans (c4 checkout renderer since changed; not still-valid) | Iyzico `failure`/`unconfirmed` outcomes and start failure not run on c7 |
| WEB-BIL-015 | web-desktop | Billing history list, invoice preview and PDF download (no link from the live UI, reached by address) | GAP | c5 `paid invoice context on actual billing route`, `invoice PDF actual download` (Poppler parsed text); preview modal opened with a Blob iframe | headless Chromium shows no PDF in the iframe, so preview content is unproven; web.rs/App.svelte changed after c5 |
| WEB-BIL-016 | web-phone | Billing history cards and PDF download | GAP |  | phone invoice cards and PDF not exercised |
| WEB-BIL-017 | web-desktop | Admin: look up a customer's invoices by email and type a custom "Bill To" block (flag `BILLING_CUSTOMER_LOOKUP | GAP |  | admin lookup and custom Bill To not exercised |
| WEB-BIL-018 | web-desktop | Invoice information form (personal or business, tax id, address); today it saves nothing and is not linked. Ve | THROWN | proposal §1.3, D14 | needs the canonical `dropped` record with mu approval; renders nothing |
| WEB-BIL-019 | web-phone | Invoice information form (same page, same stub) | THROWN | proposal §1.3, D14 | needs the canonical `dropped` record with mu approval; renders nothing |
| WEB-BIL-020 | web-desktop | View the current plan (name, total minutes, valid until, next payment) in settings | GAP | c4 desktop Subscription tab showed `No Package` | active plan view not exercised (needs a subscription purchase) |
| WEB-BIL-021 | web-phone | View the current plan | GAP | c4 desktop Subscription tab showed `No Package` | active plan view not exercised (needs a subscription purchase) |
| WEB-BIL-022 | web-desktop | Cancel the subscription (confirm dialog, then the canceled message) | GAP |  | subscription cancel not exercised |
| WEB-BIL-023 | web-phone | Cancel the subscription | GAP |  | subscription cancel not exercised |
| WEB-BIL-024 | web-desktop | Not-enough-balance alert with an "Add minutes" button when a job is refused (HTTP 402) | GAP |  | 402 not-enough-balance alert not exercised |
| WEB-BIL-025 | web-phone | Not-enough-balance alert | GAP |  | 402 not-enough-balance alert not exercised |
| WEB-BIL-026 | web-desktop | Freemium upgrade banner and cut transcript (22 minutes, fade on three lines) | GAP |  | freemium 22-minute cut and banner not exercised (fixture transcripts were 1 min) |
| WEB-BIL-027 | web-phone | Freemium upgrade banner and cut transcript | GAP |  | freemium 22-minute cut and banner not exercised (fixture transcripts were 1 min) |
| WEB-BIL-028 | web-desktop | Special client discount alert (inside the billing dialog and on transcript pages) | GAP | C1 fixture `special_offer` only | not exercised against a configured special-client audience |
| WEB-BIL-029 | web-phone | Special client discount alert | GAP | C1 fixture `special_offer` only | not exercised against a configured special-client audience |
| WEB-BIL-030 | web-desktop | Pricing section on the landing page and the pricing page (cards from the catalog, monthly or yearly toggle, st | P6 | D13; proposal §1.3 (pricing is a public route) |  |
| WEB-BIL-031 | web-phone | Pricing section on the landing page and the pricing page | P6 | D13; proposal §1.3 (pricing is a public route) |  |
| WEB-IDN-001 | web-desktop | Sign up with email and password | PROVEN | c7 `PKCE signup unmodified verify` (challenge sent, unmodified `/auth/v1/verify` link, session stored, freemium balance); C3 `C3 signup confirm` | welcome mail content not asserted |
| WEB-IDN-002 | web-phone | Sign up with email and password | GAP |  | phone signup not exercised |
| WEB-IDN-003 | web-desktop | Sign in with email and password | PROVEN | c7 `save new password then sign in with it` |  |
| WEB-IDN-004 | web-phone | Sign in with email and password | PROVEN | c7 `phone login deep link` |  |
| WEB-IDN-005 | web-desktop | Sign in with a magic link | PROVEN | c7 `magic link` (unmodified local mail link, session) |  |
| WEB-IDN-006 | web-phone | Sign in with a magic link | GAP |  | phone magic link not exercised |
| WEB-IDN-007 | web-desktop | Sign in with Google from the sign in page | GAP |  | local `/authorize` redirect with S256 not checked; the Google leg is a deployed gate (D4) |
| WEB-IDN-008 | web-phone | Sign in with Google from the sign in page | GAP |  | local `/authorize` redirect with S256 not checked; the Google leg is a deployed gate (D4) |
| WEB-IDN-009 | web-desktop | Home page Google sign in popup | P6 | D13; proposal §1.3 (homepage popup on `/`) |  |
| WEB-IDN-010 | web-phone | Home page Google sign in popup | P6 | D13; proposal §1.3 (homepage popup on `/`) |  |
| WEB-IDN-011 | web-desktop | Forgot password: request the reset email | PROVEN | c7 `recovery mail to new-password form and save` (request text and mail delivered) |  |
| WEB-IDN-012 | web-phone | Forgot password: request the reset email | GAP |  | phone forgot-password not exercised |
| WEB-IDN-013 | web-desktop | Set a new password after the reset link | PROVEN | c7 recovery link reached `/new-password` form, PUT `/auth/v1/user` 200, `Password updated successfully!`, then sign-in with the new password |  |
| WEB-IDN-014 | web-phone | Set a new password after the reset link | GAP |  | phone new password not exercised |
| WEB-IDN-015 | web-desktop | Signed in visitor on the sign in page or on `/` | GAP |  | signed-in visit to `/signin` not asserted; the `/` half is P6 (D13, P5 publishes the session-presence contract) |
| WEB-IDN-016 | web-phone | Signed in visitor on the sign in page or on `/` | GAP |  | signed-in visit to `/signin` not asserted; the `/` half is P6 (D13, P5 publishes the session-presence contract) |
| WEB-IDN-017 | web-desktop | Protected page without a session | GAP | c7 `invalid code` (desktop `/home?code=` without session ends on `/signin`); c7 erasure second tab returned to sign-in | plain protected route without a session not asserted on either project |
| WEB-IDN-018 | web-phone | Protected page without a session | GAP | c7 `invalid code` (desktop `/home?code=` without session ends on `/signin`); c7 erasure second tab returned to sign-in | plain protected route without a session not asserted on either project |
| WEB-IDN-019 | web-desktop | Sign out from the user menu | GAP |  | sign out not exercised |
| WEB-IDN-020 | web-phone | Sign out from the user menu | GAP |  | sign out not exercised |
| WEB-IDN-021 | web-desktop | Settings, Profile tab | GAP | c4 desktop Profile tab showed email and Joined on (stale) | not exercised on c7 |
| WEB-IDN-022 | web-phone | Settings, Profile tab | GAP |  | phone profile tab not exercised |
| WEB-IDN-023 | web-desktop | Captcha on the sign in page (production only) | GAP |  | Turnstile not configured locally; widget acceptance is a deployed gate (D4, D12) |
| WEB-IDN-024 | web-phone | Captcha on the sign in page (production only) | GAP |  | Turnstile not configured locally; widget acceptance is a deployed gate (D4, D12) |
| WEB-PRV-001 | web-desktop | Settings, Manage Data: automatic transcript cleanup switch | GAP | c4 desktop Manage Data tab rendered the switch | toggle not exercised |
| WEB-PRV-002 | web-phone | Settings, Manage Data: automatic transcript cleanup switch | GAP | c4 desktop Manage Data tab rendered the switch | toggle not exercised |
| WEB-PRV-003 | web-desktop | Settings, Manage Data: Delete All transcripted files | GAP |  | Delete All not exercised |
| WEB-PRV-004 | web-phone | Settings, Manage Data: Delete All transcripted files | GAP |  | Delete All not exercised |
| WEB-PRV-005 | web-desktop | Settings, Manage Data: Delete Account | PROVEN | c7 `erasure` (POST `/api/delete-account` 202, other tab signs out); C3 packaged 202 | receipt view not reachable: the initiating tab goes to the public root (P6 dependency, lead classification) |
| WEB-PRV-006 | web-phone | Settings, Manage Data: Delete Account | GAP |  | phone delete account not exercised |
| WEB-PRV-007 | web-desktop | Settings, Manage Data: legacy "Delete my data automatically" switch | GAP |  | legacy switch not exercised |
| WEB-PRV-008 | web-phone | Settings, Manage Data: legacy "Delete my data automatically" switch | GAP |  | legacy switch not exercised |
| WEB-PRV-009 | web-desktop | Settings, AI Features: give or revoke AI consent | GAP | consent given through the in-panel Start Chatting (c7) | Settings AI Features give/revoke (`DELETE /api/ai/consent`) not exercised |
| WEB-PRV-010 | web-phone | Settings, AI Features: give or revoke AI consent | GAP | consent given through the in-panel Start Chatting (c7) | Settings AI Features give/revoke (`DELETE /api/ai/consent`) not exercised |
| WEB-PRV-011 | web-desktop | Cookie consent banner: accept or reject | GAP | c5 `telemetry absent before and after reject` (0 telemetry requests) | accept path not exercised: PostHog/Sentry public config unset |
| WEB-PRV-012 | web-phone | Cookie consent banner: accept or reject | GAP | c7 phone clicked Reject | phone telemetry absence and accept path not asserted |
| WEB-PRV-013 | web-desktop | Job list history window and `unlimited_history` | GAP |  | history window and `unlimited_history` not exercised |
| WEB-PRV-014 | web-phone | Job list history window and `unlimited_history` | GAP |  | history window and `unlimited_history` not exercised |
| WEB-PRV-015 | web-desktop | Blocked user adds or resolves a YouTube link | GAP |  | blocked-user YouTube 403 not exercised |
| WEB-PRV-016 | web-phone | Blocked user adds or resolves a YouTube link | GAP |  | blocked-user YouTube 403 not exercised |
| WEB-PRV-017 | web-desktop | Privacy policy page (EN and TR) | P6 | D13; proposal §1.3 (legal pages) |  |
| WEB-PRV-018 | web-phone | Privacy policy page (EN and TR) | P6 | D13; proposal §1.3 (legal pages) |  |
| WEB-PRV-019 | web-desktop | Terms and conditions page (EN and TR) | P6 | D13; proposal §1.3 (legal pages) |  |
| WEB-PRV-020 | web-phone | Terms and conditions page (EN and TR) | P6 | D13; proposal §1.3 (legal pages) |  |
| LOC-001 | locale | App renders in locale en (English) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-002 | locale | App renders in locale tr (Turkish) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-003 | locale | App renders in locale pt (Portuguese) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-004 | locale | App renders in locale es (Spanish) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-005 | locale | App renders in locale de (German) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-006 | locale | App renders in locale id (Indonesian) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-007 | locale | App renders in locale it (Italian) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-008 | locale | App renders in locale fr (French) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-009 | locale | App renders in locale el (Greek) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-010 | locale | App renders in locale ru (Russian) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-011 | locale | App renders in locale da (Danish) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-012 | locale | App renders in locale sv (Swedish) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-013 | locale | App renders in locale no (Norwegian) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-014 | locale | App renders in locale fi (Finnish) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-015 | locale | App renders in locale kk (Kazakh) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |
| LOC-016 | locale | App renders in locale az (Azerbaijani) | PROVEN | c7 `c2-locale-final.json`: 32/32, each locale by path prefix on Desktop Chrome and Pixel 7, exact catalog label `sign_in.button_label`; gate locale validator PASS | only the sign-in surface is asserted per locale; signed-in screens in non-en not asserted |

## Proposed minimal gap run (candidate7, no product change)

One lead-authorized C2 pass on the frozen c7 binaries, image and runtime, both projects unless noted, no live calls, failures preserved, no rerun-until-pass. Prerequisites are tester-owned runtime/spec additions, listed so the lead can authorize them.

| # | Scenario (both projects) | Closes | Prerequisite |
|---|---|---|---|
| 1 | Phone signup, magic link, forgot password, new-password save | WEB-IDN-002/006/012/014, WEB-002/004 | none |
| 2 | Sign out from the user menu; protected route without a session; signed-in visit to `/signin` | WEB-IDN-015..020, WEB-043/044 (with 9) | none |
| 3 | Google button: local GoTrue `/authorize` redirect carries S256 challenge; Google host intercepted, never contacted | WEB-IDN-007/008 local half, WEB-001/002 | none; Google completion stays a deployed gate |
| 4 | Cookie accept and reject with loopback PostHog/Sentry public config: no telemetry before consent, reject keeps chunks absent, accept loads each once | WEB-PRV-011/012, WEB-005/006 app part | runtime `--host-env` telemetry keys pointing to the fixture |
| 5 | Settings: Profile; theme light/dark/system and palette; locale change; AI Features revoke (`DELETE`) and regrant; Manage Data cleanup and legacy switches, Delete All | WEB-IDN-021/022, WEB-039..042, WEB-PRV-001..004/007..010 | none |
| 6 | Billing: phone bottom sheet; hours presets/typed/arrows/min-5; Iyzico `failure` and `unconfirmed` plans; visible confirm copy and countdown; start-failure page; phone purchase | WEB-BIL-004..006/010..014, WEB-036 | none (fixture supports outcomes) |
| 7 | Coupon valid and invalid | WEB-BIL-007/008, WEB-035 | coupon seed via setup SQL (setup only) |
| 8 | Subscription purchase (Iyzico subscription plan), current-plan view, cancel | WEB-BIL-020..023 | none |
| 9 | Invoices: phone cards and PDF; admin lookup and temporary Bill To (admin email from `P5_ADMIN_EMAILS`); preview bytes read in-page via the iframe document, not `fetch(blob:)` | WEB-BIL-015..017, WEB-037/038 | none |
| 10 | 402 not-enough-balance; freemium 22-minute cut; special-client alert; all-transcripts balance | WEB-BIL-001/002/024..029, WEB-023/024 | long-duration Soniox plan; special-client audience env via `--host-env` |
| 11 | Jobs: draft restore after reload, preparation error, translation job, list paging/refresh/delete, phone filters/swipe/pull, history window | WEB-009/010/013..016/021/022, WEB-PRV-013/014 | translation provider plan in fixture |
| 12 | YouTube add/resolve and blocked-user 403 (D12 embed is recorded third-party contact; iframe origin intercepted) | WEB-011/012, WEB-PRV-015/016 | confirm P4 fixture covers YouTube metadata/transcript API; else add tester fixture routes |
| 13 | Playback seek, timestamp/speaker toggles, speaker rename; desktop line edit on c7; export all formats with toggles | WEB-017..020, WEB-025 | none |
| 14 | AI analysis start/retry/copy/export (document bytes parsed independently), highlights; chat copy/retry/rating/export; phone positive stream on c7 | WEB-027..030 | analysis LLM plan |
| 15 | Phone feedback, support, delete account | WEB-032/034, WEB-PRV-006, WEB-044 | none |

Not closable locally, recorded as explicit limits: Turnstile captcha (WEB-IDN-023/024) and Google OAuth completion (deployed gates per D4/D12); landscape on a physical device (headless covers the published orientation fixture only); invoice preview pixels in headless Chromium (PDF viewer absent); erasure receipt navigation (public root, P6).

Out of P5: P6 rows (WEB-BIL-030/031, WEB-IDN-009/010, WEB-PRV-017..020, public parts of WEB-005/047/048, `/` part of WEB-IDN-015/016), P7 rows (WEB-045/046, docs part of WEB-047/048), P8 rows (WEB-049..076). WEB-BIL-018/019 need the `dropped` record.
