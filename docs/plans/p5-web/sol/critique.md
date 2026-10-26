# P5 cross-critique: Sol on Opus

Preserved Sol session `w23:p24`. Reviewed `opus/proposal.md` and discussed concrete differences with Opus `w23:p1V`. Both original proposals remain frozen for Fable's independent review. This file is advisory, not implementation authorization or passing evidence. No builds, tests, lint, browser or provider calls were performed. Only `sol/critique.md` and `sol/response.md` are owned in this round.

## Verdict

**Changes needed before dispatch; resolved design deltas are recorded in `response.md`.** Architecture agreement is strong: separate Transgate app/client/wasm, composed Rust presentation, pure desktop/phone views, existing billing policies and streaming codecs, generated fixtures, one packaged Axum app. The important corrections concern real source controls, request boundaries and document/auth semantics, not a competing architecture.

## Findings against the frozen Opus proposal

### C1. Preserve mounted phone behavior, not stale ledger descriptions

Opus section 3 adds phone filters and unrestricted pull-to-refresh. The actual `frontend/src/features/job-management/views/mobile/MobileJobListView.tsx` only declares/destructures filter/sort props; its rendered tree has no filter control. `JobListContainer.tsx:63-64,153` disables pull-to-refresh on all-transcripts. `MobileJobDetailsContainer.tsx:108-118` already filters translation lines, contrary to stale EXP-013/core comments.

Keep cards/swipe/FAB and the actual refresh exception; do not introduce a filter/search sheet. Phone detail uses TranscriptPage filtering semantics. UI-01 remains the separate omitted timestamp/speaker-option correction, without a new all-formats phone action. Opus conceded all three source clarifications. Lead records inventory corrections, not an unexplained behavior change.

### C2. Separate session start from the public-key transcription boundary

Opus originally leaves current server routes unchanged without resolving P7 credential precedence. `transgate/http/src/lib.rs:208-221` currently starts job IDs through session JWT authentication; source IDN-028 reserves Bearer on the public transcription route for API keys. Guessing credential type or falling back after invalid key verification is not acceptable.

Adopt session `POST /api/jobs/start` over the existing parser/use case, migrate affected current consumers, and reserve the public transcription credential boundary for P7. **This is not a transfer of every `/api/v1/*` route to P7:** existing session-owned `/api/v1/ai/{analyze,chat}` remain unchanged. Opus accepted the split and narrowed its later namespace wording.

### C3. Neither a new JSON AI start mode nor a legacy-chat decoder is necessary

Opus section 2.4 adds `Accept: application/json`; my original approach consumed the legacy POST bodies. Opus identified a smaller existing seam, which I accept:

- POST with a stable supplied request UUID, read `x-generation-id`, promptly drop/cancel that response body, then attach the canonical generation watch.
- Text watches use `ApplicationDecoder`/`StreamState`; analysis uses named events over `SseDecoder`.

Actual evidence: `transgate/http/src/ai.rs:106-131,218-280`, especially the retained watch slot in the body, initial durable resnapshot and response header. `implementation/hosts.md:45-51` publishes the same contracts. Drop must release the POST's slot before another attachment; detach does not cancel generation.

I also correct my initial overbroad “no re-POST” statement: when the POST outcome is unknown, an exact same-requestId replay is the published reconciliation mechanism, not new work (`hosts.md:47`). A deliberate user retry uses a new key. Never generate a replacement key merely because a connection failed.

### C4. Export must preserve the displayed partial, not silently fetch the newest completed analysis

The frozen Opus proposal exports by job and format alone. Source `DesktopSummaryView.tsx:160-182` exposes export whenever `hasData`, including loading/interrupted partials. `useExportAnalysis.ts:60-72` captures current displayed summary/highlights/specialized data and export time. A job-only route can export a different generation or newer sections than the user selected, or reject an export the source permits.

The final recommendation is **P5-DOCS-FINAL-A**: one in-memory Rust byte adapter in/alongside `transgate/export`, called from wasm over the captured displayed typed document and click-time, reused through native/P8 bindings. The shell supplies font assets and saves/shares bytes; there is no host analysis-export route or duplicate JS encoder. This preserves partial exports without historical snapshot storage or an extra document-upload API. Real invoice context still comes from the authenticated host, with financial facts/VAT from billing core and temporary Bill To kept separate. Earlier host-payload and sequence-conflict alternatives were considered and superseded; `response.md` records the final contract and its wasm/native writer prerequisite.

My original browser `docx`/`jspdf` direction is withdrawn in favor of a single reusable Rust byte implementation for P8. However, existing `transgate_export::docx_bytes` is **not** an analysis implementation: `WordDocument` currently has paragraphs/runs, and `transgate/export/src/lib.rs:161-219` writes that transcript shape. Six analysis layouts, nested sections, invoice layout, fonts and Arabic shaping remain real work. A new PDF dependency and licensed font assets need a concrete lead-owned selection and independent source-reference proof, not “a PDF exists.” No generic document engine or historical snapshot store is needed.

### C5. Fragment-only auth omits the source's actual PKCE flow

Opus section 2.3 describes fragment-session parsing. Actual source `frontend/src/utils/supabase/client.ts:2,25-39` constructs `@supabase/ssr`'s browser client. Its installed 0.3.0 implementation sets `flowType: "pkce"`, URL session detection and persistence (`node_modules/@supabase/ssr/dist/index.js:147-158`). Source callback/code exchange cannot be replaced with an assumed fragment happy path.

I accept Opus's Rust GoTrue transport recommendation for P8 reuse, replacing my preference for an auth-only browser SDK. That acceptance includes S256/verifier persistence, code exchange, recovery intent, refresh rotation and localized redirects, with browser storage/navigation/locking as capability ports. Before refresh, acquire the cross-tab lock and reread the latest session; do not refresh a stale rotated token or clear a newer account's session on a late failure. P8 supplies native storage/browser-handoff ports later. This is a transport implementation, not a new identity system or a second JS auth state machine.

For P6, stored presence is only a hint. Verify the session bootstrap before signed-in redirect/popup suppression; expired state must not create a public-root/home redirect loop. Opus accepted this condition.

### C6. My translation expansion exceeded the recorded decision boundary

Opus's explicit baseline fallback manifest is the correct default. `languages.md:305` explicitly leaves filling the missing AI keys to mu; the brief prohibits *silent* fallback, not a documented source-equivalent fallback. I withdraw the full new translation requirement in my frozen proposal.

Use i18next core, source resources/auth labels and all 16 locales. Repair invalid German JSON; inventory actual reachable fallback resolutions with source provenance; fail new/unlisted fallback. Locale evidence states which strings still use approved English fallback. New translated content needs a recorded owner decision and shrinks that manifest. Opus conceded its hand-written resolver; no new resolver engine is needed.

### C7. Purchase display and charged quote remain different contracts

Opus's purchase row says prices always come from the quote. P2 D2 explicitly preserves DEF-010's display-versus-charged distinction. `billing_core::pricing::payg_display_total` documents and implements it at `pricing.rs:296-306`; card helpers and `normalize_hours` also already exist.

Carry separate `display_amount` and charged-quote facts, computed using existing Rust policies, never client financial arithmetic or forced equality. An additive display field on the quote response is a small host integration, owned by billing. Preserve source presets/clamp/coupon/terms and P2's decimal correction. Opus accepted; no P2 policy decision is reopened.

### C8. An optional onCancel prop is not a mounted cancel action

Opus's critique initially treated `DesktopSummaryView`'s `onCancel` prop as proof of an analysis cancel button. Direct source inspection finds it only at the interface/destructure (`DesktopSummaryView.tsx:55,65`), likewise in `MobileSummaryView.tsx:58,72`. The mounted desktop rendered tree exposes retry/copy/export but no cancel control. Opus traced the remaining rendered cancel to an unmounted legacy analysis path and withdrew the claim.

Do not add analysis or chat cancel UI merely because the backend has a cancel API. Keep terminal/interrupted behavior, existing retry, detach semantics and cancellation handling required by the shared protocol. Any newly mounted user cancel action is a separately approved UX change.

### C9. Fill actual host seams and keep phase assignments explicit

Opus accepted owner erasure-status read and own invoice `billing_info` context. `privacy::app::erasure_status` already supplies the authoritative saga view; the route must permit only the fenced owner to read that receipt, not reopen normal provisioning/content access. Read-only profile facts include the source `email_confirmed_at` Joined on value. Billing/consent remain in their existing reads rather than duplicated bootstrap policy.

P7 must provide the real key service before key screen acceptance. It is valid to integrate an app subset before that dependency; it is not valid to claim WEB-045/046 green through an absent route, hidden menu or fixture story. P6 retains public/legal/pricing/tools/docs-navigation obligations. P8 rows remain separate. All are retained in the final coverage map, not silently omitted.

## Bounded integration notes

- Use `GET /api/session` consistently; keep allow-lists and privileged credentials server-side. Adopt one composed presenter/session epoch and one mounted layout/player/watch.
- Billing presentation, owner invoice facts and checkout HTML response stay with the billing owner. The host owns feedback/support persistence/mail and erasure inclusion; the client integrates published DTOs rather than importing server internals.
- Publish the file-handle seam before view wiring. Current `PickedFile` contains only name/size/MIME (`upload_queue.rs:98-104`); duplicate names must bind to the existing stable file IDs via an explicit opaque platform handle, not filename lookup, speculative next-ID arithmetic or post-filter array zipping. No full file bytes cross JSON/wasm.
- Shared `Msg`/DTO changes affect existing P3/P4 presentation and require owner coordination. Reuse reducers, not a claim that all affected public shapes stay unchanged.
- P4 AI flags already gate admission; new general capabilities must reuse those facts, not establish a second conflicting AI policy. General visibility flags do not become authorization by hiding a button.
- `cargo test -p transgate-core presentation:: flags::` in Opus section 6 supplies two positional filters; replace with the whole crate command or separate valid filtered invocations. No command was executed here. Storybook/type checks supplement, not replace, actual declared gates and browser evidence.

## Handoff

Both advisors agreed on the source refinements and final design, subject to lead recording the dispatch decisions in `response.md`. Neither critique changes canonical rows, accepts a phase, authorizes implementation, or overrides the independent review of the frozen originals.
