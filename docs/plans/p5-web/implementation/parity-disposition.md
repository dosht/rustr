# P5 parity disposition

Owner: p1Y (independent tester). Row-by-row disposition of every WEB-*, WEB-BIL-*, WEB-IDN-*, WEB-PRV-* and LOC-* row. Updated after NEXT-P5-GAP; the lead decides acceptance.

## Evidence

| Tag | Source | Receipts |
|---|---|---|
| c7 | candidate7 `/home/mu/.cache/rustr-integration/s1-20261026215323`, manifest `c3c3d5afbdeb…`, image `sha256:1efb4e84821a…` | `logs/next-p5-c/report.json`, `c2-action-receipts.json`, `c2-locale-final.json` |
| gap | NEXT-P5-GAP on the same frozen c7 binaries and web build, owned loopback runtime | `logs/next-p5-gap/actions.json`, `network.json`, `external.json`, `fixture-setup.sql`, `host-env-*.json`, `runtime-*.py`, screenshots, parsed PDFs/ZIPs |
| c5 | candidate5 `/home/mu/.cache/rustr-integration/s1-20261026204512` | `logs/next-p5-c/c2-action-receipts.json` (only where the path is unchanged in c7) |

Rules:
- **PROVEN**: every clause exercised in a real headless browser flow on the row's platform (Desktop Chrome 1280×800, or Pixel 7 for `web-phone`) against the real host, wasm and owned loopback runtime. Failures are preserved, never rerun until pass.
- **FAIL**: exercised, product failure, routed by the lead.
- **LIVE-P9**: every locally reachable clause is proven; the remainder needs a live provider or physical device. The reason is in the Remaining column.
- **P6 / P7 / P8**: owned by another phase (D13; opus proposal §1.1, §1.3).
- **DROPPED**: lead decision recorded in the row.
- C1 fixture screenshots/axe support rendering only; loopback fixtures never establish live OAuth, captcha, payment, provider, SMTP or telemetry acceptance.

Test-only setup used by the gap run (owned files, recorded): coupon rows and blocked/unlimited_history settings and back-dated jobs via owner-role setup SQL (`fixture-setup.sql`); host env for special clients, the customer-lookup flag allow-list and loopback PostHog/Sentry config; runtime fixture routes for YouTube metadata/transcript, browser PostHog/Sentry, Soniox token lists and an Iyzico `init_rejected` outcome (`scripts/p5-web/runtime.py`, `iyzico_fixture.py`).

## Counts

| Disposition | Rows |
|---|---|
| PROVEN | 114 |
| FAIL | 5 |
| LIVE-P9 | 6 |
| P6 | 8 |
| P6/P7 | 2 |
| P7 | 2 |
| P8 | 28 |
| DROPPED | 2 |
| **Total** | **167** |

P5-owned rows: 125; proven 114, FAIL 5, LIVE-P9 6, open GAP 0. The P5 gate is not met until the FAIL rows are repaired and re-proven.

| FAIL row | Routing |
|---|---|
| WEB-011/012 | YouTube resolve succeeds but UI shows an error and does not navigate (REPAIR-P5-10, p22) |
| WEB-015 | desktop all-transcripts table cannot scroll; no load-more (REPAIR-P5-12, p1Z) |
| WEB-BIL-024/025 | 402 alert button "Add Balance" vs source "Add minutes" (`NotEnoughBalanceAlert.tsx:34`), awaiting routing |

Other findings (not row failures): new job not shown in sidebar/all-transcripts until reload (no list polling, D11) — reported in the billing group; seek slider accessible name "Show Time" (REPAIR-P5-11). Live-only limits beyond the LIVE-P9 rows: invoice-preview pixels (headless), YouTube embed, physical-device landscape, live Iyzico.

## Rows

| Row | Platform | Flow | Disposition | Evidence | Remaining |
|---|---|---|---|---|---|
| WEB-001 | web-desktop | Password sign-in, signup, magic link, Google and recovery (screen) | LIVE-P9 | password, signup, magic link, recovery proven on the project (c7 + gap); Google local half proven | Google completion LIVE-P9 |
| WEB-002 | web-phone | Password sign-in, signup, magic link, Google and recovery (screen) | LIVE-P9 | password, signup, magic link, recovery proven on the project (c7 + gap); Google local half proven | Google completion LIVE-P9 |
| WEB-003 | web-desktop | Set new password (screen) | PROVEN | c7 recovery: form, submit, result `Password updated successfully!` | redirect after save not asserted |
| WEB-004 | web-phone | Set new password (screen) | PROVEN | gap phone New Password form, save, sign-in |  |
| WEB-005 | web-desktop | First-visit sign-in and cookie prompts (screen) | PROVEN | gap cookie group (app part) | homepage popup is P6 (D13) |
| WEB-006 | web-phone | First-visit cookie prompt (screen) | PROVEN | gap cookie group (app part) | homepage popup is P6 (D13) |
| WEB-007 | web-desktop | Upload files and start transcription (screen) | PROVEN | c7 `upload transcript`: pick file, signed PUT, language, Start, navigate to `/home/1`; C3 packaged repeat | speaker toggle not exercised |
| WEB-008 | web-phone | Upload files and start transcription (screen) | PROVEN | c7 `phone short refusal` flow: phone upload, Ready, language, Start, navigate `/home/2` |  |
| WEB-009 | web-desktop | Restore drafts and preparation/error states (screen) | PROVEN | gap: draft restored after reload with language reset; 5-minute "Processing Timeout" alert for an unmeasurable file; 402 balance alert, both |  |
| WEB-010 | web-phone | Restore drafts and preparation/error states (screen) | PROVEN | gap: draft restored after reload with language reset; 5-minute "Processing Timeout" alert for an unmeasurable file; 402 balance alert, both |  |
| WEB-011 | web-desktop | Add YouTube and start (screen) | FAIL | gap: add/metadata/resolve 200, job completed, `/home/N` renders transcript | UI shows "An unexpected error occurred" and does not navigate (REPAIR-P5-10) |
| WEB-012 | web-phone | Add YouTube and start (screen) | FAIL | gap: add/metadata/resolve 200, job completed, `/home/N` renders transcript | UI shows "An unexpected error occurred" and does not navigate (REPAIR-P5-10) |
| WEB-013 | web-desktop | Translate source to target (screen) | PROVEN | gap: EN→TR translation job, translation tab, original gated (show_original_transcript off), both |  |
| WEB-014 | web-phone | Translate source to target (screen) | PROVEN | gap: EN→TR translation job, translation tab, original gated (show_original_transcript off), both |  |
| WEB-015 | web-desktop | List, page, select, refresh and delete jobs (screen) | FAIL | gap: row download, delete confirm (204), view; sidebar paging | table cannot scroll past the fold, no load-more (REPAIR-P5-12) |
| WEB-016 | web-phone | List, filter, select, refresh and delete jobs (screen) | PROVEN | gap: phone cards, touch swipe delete + confirm (204), infinite scroll `?before=` | no filter sheet / no all-transcripts pull-refresh per D9 and source `disablePullToRefresh` |
| WEB-017 | web-desktop | View live transcript and playback (screen) | PROVEN | gap: play, timestamp seek (→12.5 s), pause; Show Time / Show Speaker toggles; speaker labels | YouTube player embed is LIVE-P9; seek slider name "Show Time" routed (REPAIR-P5-11) |
| WEB-018 | web-phone | View live transcript and playback (screen) | PROVEN | gap: phone play/seek/pause, tabs, action sheet |  |
| WEB-019 | web-desktop | Edit line and speaker labels (screen) | PROVEN | gap: line edit save + Escape cancel, speaker reassign (PATCH kind=speaker) and rename (rename_speaker), persisted, both |  |
| WEB-020 | web-phone | Edit line and speaker labels (screen) | PROVEN | gap: line edit save + Escape cancel, speaker reassign (PATCH kind=speaker) and rename (rename_speaker), persisted, both |  |
| WEB-021 | web-desktop | View translation and gated original (screen) | PROVEN | gap translation view; original gated by flag, both |  |
| WEB-022 | web-phone | View translation and gated original (screen) | PROVEN | gap translation view; original gated by flag, both |  |
| WEB-023 | web-desktop | Freemium transcript fade and upgrade (screen) | PROVEN | gap freemium cut + upgrade banner, both |  |
| WEB-024 | web-phone | Freemium transcript fade and upgrade (screen) | PROVEN | gap freemium cut + upgrade banner, both |  |
| WEB-025 | web-desktop | Export TXT, Word, SRT, VTT and CSV ZIP (screen) | PROVEN | gap all-formats ZIP parsed: txt, doc, srt, vtt, Speakers Timestamps Text.csv; toggles |  |
| WEB-026 | web-phone | Export transcript/translation ZIP (screen) | PROVEN | c5 `phone transcript edit and export download`: ZIP parsed independently, edited text present (export path unchanged c5→c7) |  |
| WEB-027 | web-desktop | AI consent, analysis, highlights, retry and export (screen) | PROVEN | gap: consent + analysis sections incl. generic "Key Highlights"; interrupted → Retry; copy; DOCX/PDF/TXT/MD parsed | highlights are analysis sections in source (screens.md:45; HighlightsView removed, aiAnalysis/index.ts:7) |
| WEB-028 | web-phone | AI consent, analysis, highlights, retry and export (screen) | PROVEN | gap phone: consent, sections incl. Key Highlights, interrupted ("Analysis incomplete") → Retry, PDF export parsed |  |
| WEB-029 | web-desktop | AI chat, limits, copy, retry, rating and export (screen) | PROVEN | c7 own-tab stream + reattach; gap: limits, Copy message/chat, Download chat, Retry, rating → ai_chat_message_rated after consent |  |
| WEB-030 | web-phone | AI chat and limits (screen) | PROVEN | c7 phone short refusal; gap phone held stream partial → answer, copy/download/retry |  |
| WEB-031 | web-desktop | Job feedback (screen) | PROVEN | c7 `support feedback` (POST feedback 201, Thank You) |  |
| WEB-032 | web-phone | Job feedback (screen) | PROVEN | lead decision: source phone mounts no feedback trigger (`JobListContainer.tsx:136-156`); app matches |  |
| WEB-033 | web-desktop | Contact support (screen) | PROVEN | c7 `support feedback` (POST `/api/support` 202, dialog closes); focus returns to Support (c5) |  |
| WEB-034 | web-phone | Contact support (screen) | PROVEN | gap phone support POST 202 |  |
| WEB-035 | web-desktop | Balance, choose package, coupon and checkout (screen) | PROVEN | c7 + gap desktop balance, package, coupon, checkout |  |
| WEB-036 | web-phone | Balance, choose package, coupon and checkout (screen) | PROVEN | gap phone sheet, coupon, purchase |  |
| WEB-037 | web-desktop | Invoice history, PDF and privileged lookup (screen) | PROVEN | c5 + gap desktop history, PDF, admin lookup | preview pixels LIVE-P9 |
| WEB-038 | web-phone | Invoice history and PDF (screen) | PROVEN | gap phone invoices + PDF |  |
| WEB-039 | web-desktop | Profile, theme and locale settings (screen) | PROVEN | gap: profile, theme Dark/Light/System (data-theme, stored, system follows prefers-color-scheme), locale change → `/de/…` lang=de persisted, both | no palette selector exists in app settings; palettes covered by C1 (D10) |
| WEB-040 | web-phone | Profile, theme and locale settings (screen) | PROVEN | gap: profile, theme Dark/Light/System (data-theme, stored, system follows prefers-color-scheme), locale change → `/de/…` lang=de persisted, both | no palette selector exists in app settings; palettes covered by C1 (D10) |
| WEB-041 | web-desktop | AI consent, subscription and data settings (screen) | PROVEN | gap AI Features, Subscription, Manage Data tabs, both |  |
| WEB-042 | web-phone | AI consent, subscription and data settings (screen) | PROVEN | gap AI Features, Subscription, Manage Data tabs, both |  |
| WEB-043 | web-desktop | Delete account and sign out (screen) | PROVEN | c7 erasure + gap desktop sign out |  |
| WEB-044 | web-phone | Delete account and sign out (screen) | PROVEN | gap phone delete account + Logout |  |
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
| WEB-BIL-001 | web-desktop | View the balance in the user area and on the all-transcripts page (minutes under 60, else hours and minutes, r | PROVEN | c7 c2: user-area balance (min / hr+min, negative); gap: `balance refreshes within 30 s without reload` (18 min -> 10 hr 18 min in 26 s) | all-transcripts-page clause DROPPED with excluded legacy route `pages/home/all-transcripts/all-transcripts.tsx:108` (lead decision) |
| WEB-BIL-002 | web-phone | View the balance in the drawer and bottom sheet | PROVEN | gap: phone drawer `Balance 20 min`, account sheet `Balance -5 min` |  |
| WEB-BIL-003 | web-desktop | Open the billing dialog and view packages (pay-as-you-go card, monthly or yearly toggle, Premium and Business  | PROVEN | c7 checkout; gap: desktop dialog PAYG/monthly-yearly/Premium/Business/summary | TRY card note not asserted (USD flow) |
| WEB-BIL-004 | web-phone | Open the billing bottom sheet and view packages | PROVEN | gap: phone `Add Balance` sheet, PAYG and Subscription sections (bil-phone-sheet.png, bil-phone-sub.png) |  |
| WEB-BIL-005 | web-desktop | Choose pay-as-you-go hours (presets 5, 10, 25, typed value, arrows, minimum 5 warning) | PROVEN | gap `hours presets/arrows/typed/min` both: 25 → $37.25, +1/−1, 5, typed 3 clamps to 5 with minimum warning and Pay disabled, typed 12 → $17.88 |  |
| WEB-BIL-006 | web-phone | Choose pay-as-you-go hours | PROVEN | gap `hours presets/arrows/typed/min` both: 25 → $37.25, +1/−1, 5, typed 3 clamps to 5 with minimum warning and Pay disabled, typed 12 → $17.88 |  |
| WEB-BIL-007 | web-desktop | Apply a coupon (error tooltip, green check when applied) | PROVEN | gap `coupon invalid/expired/valid` both: NOPE "Coupon is not valid", P5OLD "Coupon has expired", P5GOOD $14.90 → $11.18 (owned setup SQL seed) |  |
| WEB-BIL-008 | web-phone | Apply a coupon | PROVEN | gap `coupon invalid/expired/valid` both: NOPE "Coupon is not valid", P5OLD "Coupon has expired", P5GOOD $14.90 → $11.18 (owned setup SQL seed) |  |
| WEB-BIL-009 | web-desktop | Buy a package: accept terms, press Pay with Card, land on Iyzico's hosted form, pay | PROVEN | c7 checkout + gap: PAYG and Premium subscription through D17 document and signed loopback Iyzico form | live Iyzico is LIVE-P9 |
| WEB-BIL-010 | web-phone | Buy a package from the bottom sheet | PROVEN | gap `phone purchase success + countdown`, phone Business subscription purchase |  |
| WEB-BIL-011 | web-desktop | Checkout success page, countdown, return home | PROVEN | gap: visible "Successful!" copy and 5 s countdown (desktop subscription, phone PAYG) | countdown target is the public root `/` (P6, 404 today) |
| WEB-BIL-012 | web-phone | Checkout success page, countdown, return home | PROVEN | gap `phone purchase success + countdown` | countdown target `/` (P6) |
| WEB-BIL-013 | web-desktop | Checkout failure page and the "We couldn't start your checkout" page with a back-to-plans link | PROVEN | gap: desktop "Failed" page + countdown; unconfirmed page (no countdown, do-not-pay-again); start-failure page "We couldn't start your checkout" | Back to plans targets `/#pricing` (P6) |
| WEB-BIL-014 | web-phone | Checkout failure page and the start-failure page | PROVEN | gap: phone "Failed" page + countdown; phone start-failure page | Back to plans `/#pricing` (P6) |
| WEB-BIL-015 | web-desktop | Billing history list, invoice preview and PDF download (no link from the live UI, reached by address) | PROVEN | c5 + gap: desktop list, preview modal (Blob iframe), PDF download parsed by pdftotext (PREMIUM 120.00) | preview pixel rendering is LIVE-P9 (headless Chromium has no PDF viewer) |
| WEB-BIL-016 | web-phone | Billing history cards and PDF download | PROVEN | gap `phone invoice cards + PDF download`, pdftotext parsed |  |
| WEB-BIL-017 | web-desktop | Admin: look up a customer's invoices by email and type a custom "Bill To" block (flag `BILLING_CUSTOMER_LOOKUP | PROVEN | gap `admin lookup`: not-found message, found customer list, custom Bill To replaces Bill To in parsed PDF (lookup flag via TRANSGATE_FEATURE_FLAGS allow-list) |  |
| WEB-BIL-018 | web-desktop | Invoice information form (personal or business, tax id, address); today it saves nothing and is not linked. Ve | DROPPED | lead decision 2026-10-26: source form saves nothing and is unlinked (`frontend/src/pages/InvoiceForm/index.tsx:48-52`); P5 D14 |  |
| WEB-BIL-019 | web-phone | Invoice information form (same page, same stub) | DROPPED | lead decision 2026-10-26: source form saves nothing and is unlinked (`frontend/src/pages/InvoiceForm/index.tsx:48-52`); P5 D14 |  |
| WEB-BIL-020 | web-desktop | View the current plan (name, total minutes, valid until, next payment) in settings | PROVEN | gap: current plan (PREMIUM 1200 / BUSINESS 2400 min, valid until, next payment) desktop and phone |  |
| WEB-BIL-021 | web-phone | View the current plan | PROVEN | gap: current plan (PREMIUM 1200 / BUSINESS 2400 min, valid until, next payment) desktop and phone |  |
| WEB-BIL-022 | web-desktop | Cancel the subscription (confirm dialog, then the canceled message) | PROVEN | gap: cancel confirm → POST cancel 200 → "Your subscription is canceled…", Next Payment `--`, both |  |
| WEB-BIL-023 | web-phone | Cancel the subscription | PROVEN | gap: cancel confirm → POST cancel 200 → "Your subscription is canceled…", Next Payment `--`, both |  |
| WEB-BIL-024 | web-desktop | Not-enough-balance alert with an "Add minutes" button when a job is refused (HTTP 402) | FAIL | gap: 402 alert both projects ("Not enough balance." + text), action opens billing | button label "Add Balance" differs from source "Add minutes" (`NotEnoughBalanceAlert.tsx:34`); routed to lead |
| WEB-BIL-025 | web-phone | Not-enough-balance alert | FAIL | gap: 402 alert both projects ("Not enough balance." + text), action opens billing | button label "Add Balance" differs from source "Add minutes" (`NotEnoughBalanceAlert.tsx:34`); routed to lead |
| WEB-BIL-026 | web-desktop | Freemium upgrade banner and cut transcript (22 minutes, fade on three lines) | PROVEN | gap `freemium 22-minute cut` both (tr UI, Turkish job, 25 min): lines after 1320 s hidden, upgrade banner |  |
| WEB-BIL-027 | web-phone | Freemium upgrade banner and cut transcript | PROVEN | gap `freemium 22-minute cut` both (tr UI, Turkish job, 25 min): lines after 1320 s hidden, upgrade banner |  |
| WEB-BIL-028 | web-desktop | Special client discount alert (inside the billing dialog and on transcript pages) | PROVEN | gap: special-client alert on home and in purchase dialog/sheet with copy code, both (TRANSGATE_SPECIAL_CLIENTS env) |  |
| WEB-BIL-029 | web-phone | Special client discount alert | PROVEN | gap: special-client alert on home and in purchase dialog/sheet with copy code, both (TRANSGATE_SPECIAL_CLIENTS env) |  |
| WEB-BIL-030 | web-desktop | Pricing section on the landing page and the pricing page (cards from the catalog, monthly or yearly toggle, st | P6 | D13; proposal §1.3 (pricing is a public route) |  |
| WEB-BIL-031 | web-phone | Pricing section on the landing page and the pricing page | P6 | D13; proposal §1.3 (pricing is a public route) |  |
| WEB-IDN-001 | web-desktop | Sign up with email and password | PROVEN | c7 `PKCE signup unmodified verify` (challenge sent, unmodified `/auth/v1/verify` link, session stored, freemium balance); C3 `C3 signup confirm` | welcome mail content not asserted |
| WEB-IDN-002 | web-phone | Sign up with email and password | PROVEN | gap `signup PKCE (phone)` (twice) |  |
| WEB-IDN-003 | web-desktop | Sign in with email and password | PROVEN | c7 `save new password then sign in with it` |  |
| WEB-IDN-004 | web-phone | Sign in with email and password | PROVEN | c7 `phone login deep link` |  |
| WEB-IDN-005 | web-desktop | Sign in with a magic link | PROVEN | c7 `magic link` (unmodified local mail link, session) |  |
| WEB-IDN-006 | web-phone | Sign in with a magic link | PROVEN | gap phone `magic link` |  |
| WEB-IDN-007 | web-desktop | Sign in with Google from the sign in page | LIVE-P9 | gap: Google button → local `/auth/v1/authorize` provider=google, S256 challenge, redirect_to `/home`; GoTrue redirects to accounts.google.com (intercepted) both | Google OAuth completion needs a live Google client (D4) |
| WEB-IDN-008 | web-phone | Sign in with Google from the sign in page | LIVE-P9 | gap: Google button → local `/auth/v1/authorize` provider=google, S256 challenge, redirect_to `/home`; GoTrue redirects to accounts.google.com (intercepted) both | Google OAuth completion needs a live Google client (D4) |
| WEB-IDN-009 | web-desktop | Home page Google sign in popup | P6 | D13; proposal §1.3 (homepage popup on `/`) |  |
| WEB-IDN-010 | web-phone | Home page Google sign in popup | P6 | D13; proposal §1.3 (homepage popup on `/`) |  |
| WEB-IDN-011 | web-desktop | Forgot password: request the reset email | PROVEN | c7 `recovery mail to new-password form and save` (request text and mail delivered) |  |
| WEB-IDN-012 | web-phone | Forgot password: request the reset email | PROVEN | gap phone `forgot password + new password form + save + sign in` |  |
| WEB-IDN-013 | web-desktop | Set a new password after the reset link | PROVEN | c7 recovery link reached `/new-password` form, PUT `/auth/v1/user` 200, `Password updated successfully!`, then sign-in with the new password |  |
| WEB-IDN-014 | web-phone | Set a new password after the reset link | PROVEN | gap phone `forgot password + new password form + save + sign in` |  |
| WEB-IDN-015 | web-desktop | Signed in visitor on the sign in page or on `/` | PROVEN | gap `signed-in visit to /signin` → `/home` both | `/` redirect is P6 (D13) |
| WEB-IDN-016 | web-phone | Signed in visitor on the sign in page or on `/` | PROVEN | gap `signed-in visit to /signin` → `/home` both | `/` redirect is P6 (D13) |
| WEB-IDN-017 | web-desktop | Protected page without a session | PROVEN | gap `protected route without session`: `/home/1` and `/billing` → `/signin` both |  |
| WEB-IDN-018 | web-phone | Protected page without a session | PROVEN | gap `protected route without session`: `/home/1` and `/billing` → `/signin` both |  |
| WEB-IDN-019 | web-desktop | Sign out from the user menu | PROVEN | gap sign out: desktop menu "Sign Out", phone account sheet "Logout"; session cleared, `/signin` stays | post-logout navigation to `/` (P6) |
| WEB-IDN-020 | web-phone | Sign out from the user menu | PROVEN | gap sign out: desktop menu "Sign Out", phone account sheet "Logout"; session cleared, `/signin` stays | post-logout navigation to `/` (P6) |
| WEB-IDN-021 | web-desktop | Settings, Profile tab | PROVEN | gap: Profile email + Joined on, both |  |
| WEB-IDN-022 | web-phone | Settings, Profile tab | PROVEN | gap: Profile email + Joined on, both |  |
| WEB-IDN-023 | web-desktop | Captcha on the sign in page (production only) | LIVE-P9 |  | Turnstile widget/token acceptance needs a deployed site key (D4, D12) |
| WEB-IDN-024 | web-phone | Captcha on the sign in page (production only) | LIVE-P9 |  | Turnstile widget/token acceptance needs a deployed site key (D4, D12) |
| WEB-PRV-001 | web-desktop | Settings, Manage Data: automatic transcript cleanup switch | PROVEN | gap: cleanup switch with "Disable Cleanup" confirm, PATCH autoDelete=false, persisted after reload, both |  |
| WEB-PRV-002 | web-phone | Settings, Manage Data: automatic transcript cleanup switch | PROVEN | gap: cleanup switch with "Disable Cleanup" confirm, PATCH autoDelete=false, persisted after reload, both |  |
| WEB-PRV-003 | web-desktop | Settings, Manage Data: Delete All transcripted files | PROVEN | gap: Delete All confirm → DELETE /api/jobs 200, list empty, both |  |
| WEB-PRV-004 | web-phone | Settings, Manage Data: Delete All transcripted files | PROVEN | gap: Delete All confirm → DELETE /api/jobs 200, list empty, both |  |
| WEB-PRV-005 | web-desktop | Settings, Manage Data: Delete Account | PROVEN | c7 `erasure` (POST `/api/delete-account` 202, other tab signs out); C3 packaged 202 | receipt view not reachable: the initiating tab goes to the public root (P6 dependency, lead classification) |
| WEB-PRV-006 | web-phone | Settings, Manage Data: Delete Account | PROVEN | gap phone `delete account`: confirm → 202, second phone tab → `/signin` | receipt navigation goes to `/` (P6) |
| WEB-PRV-007 | web-desktop | Settings, Manage Data: legacy "Delete my data automatically" switch | PROVEN | gap: legacy switch PATCH deletedMonthly=false, persisted, both |  |
| WEB-PRV-008 | web-phone | Settings, Manage Data: legacy "Delete my data automatically" switch | PROVEN | gap: legacy switch PATCH deletedMonthly=false, persisted, both |  |
| WEB-PRV-009 | web-desktop | Settings, AI Features: give or revoke AI consent | PROVEN | gap: Enable AI (POST consent), "Turn off AI and delete saved AI data" with inline confirm → DELETE consent 200, "Consent revoked", both |  |
| WEB-PRV-010 | web-phone | Settings, AI Features: give or revoke AI consent | PROVEN | gap: Enable AI (POST consent), "Turn off AI and delete saved AI data" with inline confirm → DELETE consent 200, "Consent revoked", both |  |
| WEB-PRV-011 | web-desktop | Cookie consent banner: accept or reject | PROVEN | gap cookie group both: 0 telemetry before choice; Reject → none incl. reload; Accept → SDK chunks once, PostHog config/flags, POST /ph/e events, Sentry envelope (loopback telemetry config) | harness masks navigator.webdriver/HeadlessChrome UA only so PostHog does not drop events as bot |
| WEB-PRV-012 | web-phone | Cookie consent banner: accept or reject | PROVEN | gap cookie group both: 0 telemetry before choice; Reject → none incl. reload; Accept → SDK chunks once, PostHog config/flags, POST /ph/e events, Sentry envelope (loopback telemetry config) | harness masks navigator.webdriver/HeadlessChrome UA only so PostHog does not drop events as bot |
| WEB-PRV-013 | web-desktop | Job list history window and `unlimited_history` | PROVEN | gap history seed (24 recent + 3 at 40 days): window hides old jobs, unlimited_history shows them; phone list and desktop sidebar, `?before=` paging |  |
| WEB-PRV-014 | web-phone | Job list history window and `unlimited_history` | PROVEN | gap history seed (24 recent + 3 at 40 days): window hides old jobs, unlimited_history shows them; phone list and desktop sidebar, `?before=` paging |  |
| WEB-PRV-015 | web-desktop | Blocked user adds or resolves a YouTube link | PROVEN | gap: blocked user (setup SQL) YouTube add → 403 "Account suspended" alert, both |  |
| WEB-PRV-016 | web-phone | Blocked user adds or resolves a YouTube link | PROVEN | gap: blocked user (setup SQL) YouTube add → 403 "Account suspended" alert, both |  |
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
