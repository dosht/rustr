# P5 cross-critique: Opus on Sol

Reviewer `w23:p1V`, preserved Opus session, 2026-10-25. Read `sol/proposal.md` in full against my frozen `opus/proposal.md`. Source checks for disputed claims are cited below. Read-only: no implementation, checks, browser or provider calls.

## Summary

The two proposals agree on the architecture. Both put `transgate/web` over a new `transgate/client` and `transgate/wasm`, reuse the existing reducers, billing presentation and P4 codecs, and keep views pure per platform. Both choose touch plus width under 1024 px for the phone layout, polling instead of realtime, a browser checkout confirmation rendered as HTML from one `confirm_checkout` result with escaped embedded JSON, a server-derived capability and profile read, real feedback and support routes, P7 backend before the key screen, P6 for public pages and P8 for native rows. Sol is more precise in several places, and I adopt those points. The real differences are below, ordered by consequence.

## Sol is right; I concede

1. **Phone filters are not mounted.** `MobileJobListView.tsx:44-51,172-175` declares `filter`/`sort` props and the file header promises "Bottom sheet filters", but the component never renders a filter control. `screens.md` and my §3 table ("Cards, filters, swipe delete, pull to refresh") copied the inaccurate ledger. Baseline has no phone filter UI.
2. **The all-transcripts view disables pull-to-refresh on phone.** `JobListContainer.tsx:63-64,153` passes `disablePullToRefresh={isAllTranscriptsView}`. I missed this exception.
3. **Phone detail export already filters translation lines.** `MobileJobDetailsContainer.tsx:111-117` filters them, so EXP-013 and the core comments are stale for phone. Use TranscriptPage semantics for phone and record an inventory correction, not a behavior change. UI-01 (toggle options) remains the only phone export difference.
4. **The browser start route needs a P7 boundary.** `transgate/http/src/lib.rs:208-221` authenticates `/api/v1/transcriptions` with the session JWT, while source IDN-028 reserves Bearer for API keys on that route. A separate session route `POST /api/jobs/start` over the same parser and use case is smaller than credential-format precedence rules, and it leaves P7 free to own `/api/v1/*`. I did not address this; adopt Sol's recommendation as a lead decision.
5. **An owner erasure status read is needed.** `Privacy::erasure_status` exists ("What the owner (or the sign-in shell) sees", `privacy/app.rs:289-306`), but no route exposes it. The post-delete screen needs it to show Requested, LocalDataErased or ErasedWithExceptions honestly. Adopt it.
6. **Owner invoice context.** The source `InvoicePDF` takes `billingInfo` (`billing/index.tsx:155`), and the only route returning it today is the admin lookup. My `InvoiceDoc` therefore needs a narrow owner read of the latest `billing_info`. Adopt it.
7. **i18next core instead of a hand-written resolver.** The source catalogs use i18next v4 plural and interpolation semantics. Using the library is lower risk than reimplementing it. I withdraw my "no i18next" choice and keep the rest of my locale plan (see D-2).
8. **Additions I missed, all accepted.** Cross-tab refresh ownership, IME composition not saving an edit, a single mounted player or watch on layout switch, an accessible landscape notice, the telemetry consent mismatch (GA, Hotjar and New Relic load regardless of cookie), and decorative Turnstile recorded as a trust issue rather than parity protection.

## Where I hold, with reasons

### H-1. AI start transport: no new host route; Sol's tap is also unnecessary

My proposal added `Accept: application/json` to the AI POST routes. Sol consumes the legacy POST SSE bodies and decodes source chat and analysis events. Both are larger than needed. `transgate/http/src/ai.rs` sends `x-generation-id` as a response header (handoff `hosts.md:45-46`), and detaching never cancels or regenerates (P4 D4 and host Watch). The smallest client:

1. POST the source route.
2. Read `x-generation-id`.
3. Drop the body.
4. Attach `GET /api/ai/generations/{id}/watch`: text through `ApplicationDecoder` and `StreamState`, analysis through named events over `SseDecoder`.

There is one decoder path per kind and no legacy chat-event decoder. **Withdraw my `Accept` change.**

### H-2. Analysis and invoice document bytes: Rust, not browser `docx`/`jspdf`

Sol keeps a Rust format model but encodes bytes in the browser with lazy `docx` and `jspdf`, and invoices with the same `jspdf`. I hold server-side Rust bytes:

- **P8 needs the same documents.** WEB-069/070 carry "copy/export states" for Android and iOS. Browser encoders force a second implementation per native shell. Rust bytes on the host are reused unchanged by every shell.
- **One writer already exists.** `transgate_export` writes OOXML, used today for the transcript `.doc`. The source analysis DOCX is a structured document of headings, paragraphs and runs, so the new work is mostly the PDF writer.
- **Fonts.** The source fetches Noto Arabic from a CDN at runtime (`pdf-exporter.ts:40-48`). Embedding fonts in the server binary removes that runtime fetch, which Sol also wants removed.
- **No PDF engine in the browser bundle.** The invoice uses `@react-pdf` today; neither proposal keeps it.

**Cost:** one pure-Rust PDF dependency plus font assets, with a P9 image-size impact. Lead decision D-1.

### H-3. Locale completeness: keep the baseline, do not translate in P5

Sol recommends translating every reachable P5 key into all 16 locales before locale acceptance. PRD §2.1 is zero change: "every language stays supported". LNG-D08 says "Parity target is the current state (English fallback) ... Mu decides whether to fill the gaps", and `languages.md:305` lists it under Needs mu. About 152 AI keys times 15 locales of new translated product copy:

- has no source of truth;
- needs a human or verified translation owner;
- is a content change, not a port.

My proposal keeps the gaps explicit and closes the defect Sol worries about (silently certifying English as parity):

- a generated fallback manifest equal to the baseline;
- tests that fail on any new or unlisted fallback;
- the `de/app.json` parse fix;
- every catalog parses.

Filling the gaps stays a separate, recorded decision that closes the manifest when someone owns the translations. Lead decision D-2.

### H-4. Auth adapter in Rust, shared with P8

Sol allows a thin `@supabase/ssr` or GoTrue-JS browser adapter. WEB-051/052 require sign-in, signup, recovery and sign-out natively too. Rust GoTrue flows in `transgate-client` cover all six flows for every shell from one implementation. Sol's cross-tab rotation point is real: GoTrue rotates refresh tokens. The browser `platform` port therefore supplies a lock (`navigator.locks` under the `transgate.session` key) and a `storage` event, so one tab refreshes and the others adopt the result. Only the Google redirect step and the fragment session parse touch the DOM. Lead decision D-3.

## Remaining differences that need no lead decision

| Topic | Resolution |
|---|---|
| `/api/me` vs `/api/session` | Same contract; take `GET /api/session` (Sol's name). Contents per my §2.4 table: verified profile including `email_confirmed_at`, capabilities derived server-side, no allow-lists; billing stays in `overview`, consent in its own API |
| Duplicate list routes | Canonical redirects, not aliases (Sol) |
| Checkout initialization | Presenter calls existing `POST /api/billing/checkout`, hands `CheckoutView` to `/checkout`; cold reload follows the source redirect; never initializes on GET (Sol's wording, same as my §2.5) |
| Slice ownership | Sol's six feature-owner slices with one integration owner for client, wasm and root presenter. Two amendments: billing presentation (`purchase`, `InvoiceDoc`, owner invoice read, confirm HTML adapter) stays with the billing module owner; Msg-key migration of existing P3/P4 presentation is coordinated through their owners |
| Key screen views | P7 delivers the key service early; the commerce/settings owner renders the views on the P5 shell (Sol). No menu entry or route until the backend exists (both) |
| UI-01/03/04/05 | Identical recommendations; Sol adds a typed support outcome to UI-04. Agree |
| Tones | My additive `success`/`progress` token roles through the lead (Sol says only genuinely missing roles; these are) |
| Captcha | Preserve placement. Pass the real Turnstile token to GoTrue only as a recorded correction; otherwise label it decorative. Live proof stays external (Sol) |
| Telemetry | PostHog and Sentry behind cookie consent (PRD names these). GA, Hotjar and New Relic need an explicit keep or drop record (keep-throw "Needs mu"); default is to drop them, not load them unconditionally |

## Smallest gaps in Sol's text

- §7.2 includes `build-storybook` and `check` scripts in the gate set. These are fine as owner checks, but they are not gate rows. `gate-transgate.sh` names only `test:ci` and `e2e`.
- §3.3 "explicit cancel only where the source exposes a cancel control": `DesktopSummaryView` exposes `onCancel` (props at `:62-65`). Analysis cancel is therefore in scope; chat cancel is not, because `useChatViewModel` has no cancel action in `ChatActions:30-34`. Name both explicitly.
