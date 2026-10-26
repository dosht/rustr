# P5 cross-critique: Sol response and convergence

Preserved Sol session `w23:p24`, following direct discussion with Opus `w23:p1V`. Both original proposals remain frozen for Fable's independent review. Only this response and `sol/critique.md` were written in this round. No implementation, builds, tests, lint, browser or provider calls. No canonical, book, code or root-manifest edits.

## Outcome

**Converged advisory recommendation, not implementation approval.** Opus acknowledged the final document-design token **P5-DOCS-FINAL-A**. Earlier crossed messages proposing host bytes are superseded; the final contract below is deliberately explicit. Source findings and citations are in `critique.md`; no runtime evidence is claimed.

## 1. Changes I accept against my frozen proposal

1. **Locale completeness means explicit source parity, not unapproved translation work.** Keep all 16 locales and source/auth resources with i18next core. Repair German JSON syntax. Produce a provenance-backed manifest of actual source English fallbacks; new or unlisted fallback fails. Do not certify every string as translated. Filling gaps requires a recorded content-owner decision (`languages.md:305`); my original mandatory new translations are withdrawn.
2. **GoTrue transport belongs in Rust for reuse.** Implement the source's actual PKCE flows in `transgate-client`, not an assumed fragment-only flow and not a second browser SDK session owner. Browser ports supply storage, cross-tab locks/events and navigation. Include S256/verifier persistence, OTP/recovery challenge and code exchange, password recovery/update and rotated refresh tokens. Lock, reread current session, then refresh; epoch-fence late results. P8 later substitutes native storage/handoff ports. Source installed `@supabase/ssr` configuration is evidence for PKCE, not proof the new adapter works.
3. **Use the existing AI header-to-watch seam.** POST with the logical request's supplied UUID; read `x-generation-id`; promptly cancel/drop the body to release its watch slot; attach the canonical watch. Text uses the application codec/StreamState, analysis its named-event decoder. No JSON-start extension and no legacy chat-body decoder. Known generation: reattach. Unknown POST outcome: exact same-key replay per the published P4 contract, never an automatically minted replacement request. A deliberate new retry retains its existing new-work meaning.
4. **One Rust document byte implementation, not browser docx/jspdf.** The final location and snapshot semantics are specified in section 2. I withdraw my browser encoder recommendation.
5. **Tighten ownership.** Keep billing purchase/invoice presentation, display/charge DTOs and checkout HTML integration with the billing module owner. Use the six bounded feature/integration slices from my proposal, with existing P3/P4 owners coordinating shared public-shape changes. There is no giant new generic framework or taskr conversion.

## 2. Final document contract: P5-DOCS-FINAL-A

- **Location:** one in-memory Rust byte adapter in or alongside existing `transgate/export`, usable on wasm32 and native. The web calls it through wasm; P8 reuses the same crate through its binding. This is an adapter outside I/O-free core, not a new service or generic document engine.
- **Input:** the pure core creates the format-specific document model from the **captured displayed** typed analysis progress/result, selected format, title/labels and export click-time. Preserve six shapes, common summary, nested/optional sections, metadata, section ordering, names and source export availability while loading/interrupted. Do not substitute the latest saved analysis for the clicked partial.
- **Invoice authority:** fetch real owner invoice/payment/billing_info context through the authenticated API; privileged lookup stays server-authorized. Billing core derives invoice/VAT facts and the document model. Temporary Bill To is separate user text, unstored. No client-submitted document becomes a financial or durable AI fact.
- **Shell mechanics:** load bundled licensed font assets, supply font bytes to the adapter, preview the resulting PDF and save/share bytes. No business formatting rules or extra docx/jspdf pipeline in TypeScript. Reuse font data rather than loading/copying it per streamed section.
- **Real remaining work:** extend the existing paragraph/run OOXML support for the source analysis/invoice document structures; select one PDF writer with the necessary layout/font/shaping support. Existing transcript OOXML is reuse, not a completed six-shape exporter. Fonts must cover the required UI glyphs and source Arabic/RTL behavior; source-level CJK limitations are recorded, not silently expanded or declared solved.
- **Pre-dispatch contract:** lead records the actual PDF dependency, wasm32/native target support, font assets/licenses and ownership. The writer must support the specified document behavior; do not dispatch a pretend completed exporter around an unspecified no-op backend. If a dependency cannot meet the contract, that is an explicit dependency/design issue, not permission to ship corrupted documents.
- **Later proof:** lead-run source-derived reference capture on synthetic data only, independent parsing of generated DOCX/PDF/text, all six analysis shapes including a partial, invoice/admin Bill To examples, pagination and non-Latin/Arabic visual evidence. Verify actual browser save/share and native binding consumption in their own phases. This round runs none of it.

**Not chosen:** job-only host export, host payload formatting API, displayed-sequence conflict retries, snapshot-history persistence, and duplicate per-shell encoders. There is **no new host analysis-export route** in this recommendation. No further document-location switch is pending between the advisors.

## 3. Source refinements accepted by both advisors

| Area | Final recommendation |
|---|---|
| Product split | `transgate/client`, `transgate/wasm`, `transgate/web`; one composed presenter and session epoch; pure desktop/phone views and Rust-generated types/fixtures. No taskr-core dependency or unnecessary taskr changes |
| Bootstrap | `GET /api/session` for verified source profile/capabilities; Joined on remains email-confirmed date. No allow-list leakage; billing and consent remain in their existing authoritative reads |
| Public/session start | New session `POST /api/jobs/start`, existing parser/use case, migrate current consumers. P7 owns public transcription/key auth, not every `/api/v1/*` route. Existing session AI starts stay unchanged |
| Missing real operations | Owned feedback persistence/erasure, support mail and honest outcome, owner invoice context, fenced-owner erasure receipt read. No mock route or reopened access after erasure |
| Phone history/export | No mounted filter controls; all-transcripts pull-refresh disabled; phone detail already filters translation lines. UI-01 separately makes current phone timestamp/speaker toggles effective; no new all-formats action |
| Cancel | Neither analysis nor chat gets a new cancel button from an unused prop/API. Mounted summary views have no cancel action; the discovered rendered control belongs to an unmounted legacy path. Preserve retry/interruption/detach semantics |
| Price presentation | Separate source `display_amount` from charged quote via existing Rust pricing helpers, preferably an additive billing-owned response field. Preserve P2 D2/DEF-010, never JS financial arithmetic or forced equality |
| Layout/mechanics | Touch and width below 1024 plus existing mobile flags; one mounted layout/player/watch. IME-safe editing, stable file/opaque-handle association, source polling with visibility/disposal cleanup, accessible landscape notice |
| Checkout | Existing authenticated checkout POST -> real CheckoutView -> trusted handoff. Cold GET/reload never initializes/repurchases. Browser provider POST confirmation renders HTML from one confirm result with safe embedded JSON; JSON consumers remain supported. Uncertainty is not success or decline |
| Copy/localization/tokens | Typed message keys/arguments, i18next core and explicit fallback manifest; source locale navigation; generated root tokens light/dark/system and Paper Moss default. Any missing semantic roles go through the lead's root-token integration, not shell-local colors |

## 4. Phase and gate boundaries

**P5:** real Rust/browser flows for applicable desktop and phone rows. Compile/generate/type-check/Storybook checks are useful but do not replace declared gates, actual wasm/browser/database exercise, bytes/media proof or packaged serving. Correct the invalid two-filter Cargo command in the Opus original before copying commands into dispatch. Existing P3/P4 integration and E4 proof retain their original attribution, not automatic P5 acceptance.

**P6:** public/static/SEO/legal/pricing/contact/certificates/free tools and actual homepage popup mount. P5 publishes shared theme/cookie/session widgets and app return targets; P6 mounts real pages and preserves the server-first P3 processor/tool contracts. Same-origin session presence is an untrusted storage hint until session verification. On expiry/refusal clear it and resolve guest state; no public-root/home redirect loop or guest popup racing restoration. Public routes are not blanket SPA 200s.

**P7:** deliver the real session-owned key service before the key screen/menu can be accepted; P5 commerce/settings renders its two views against that published service. Public-key scopes/rates/callback delivery/OpenAPI and API docs remain P7. It is valid to integrate an app subset first, not to declare WEB-045/046 or dependent docs rows complete over hidden/absent endpoints. Final coverage retains every dependency-backed row.

**P8:** WEB-049..076 consume the same Rust client/presentation/document machinery with platform adapters. P5 does not prove native UI, native browser OAuth, store policy or terminal interaction. Do not create a second document/auth business implementation merely to finish P5.

**P9/P10:** actual hosted origins, OAuth/SMTP/captcha/payment/provider/storage/telemetry acceptance, session-storage/cutover migration, operations and final book/template work remain named. Local GoTrue/PG and loopback fixtures cannot certify external services or deployment. No book work now.

## 5. Concise lead decisions before implementation dispatch

| Decision | Joint recommendation / action |
|---|---|
| Architecture and ownership | Adopt the product split and six bounded slices; billing owns its presentation/DTO/checkout changes, existing owners coordinate P3/P4 interfaces. Lead owns root manifests/locks/gates/canonical status/integrated execution |
| Document dependency | Record **P5-DOCS-FINAL-A**, actual wasm32/native PDF dependency, licensed fonts and document evidence contract before exporter dispatch |
| Auth and session boundaries | Adopt Rust GoTrue PKCE with platform locks/storage/navigation; `/api/session`; clean session job-start cutover; publish P6 verified-presence and P7 credential contracts |
| Locale policy | Preserve explicit measured fallback baseline, fix invalid German JSON, all 16 exercised; translated-content expansion only after recorded owner approval |
| Named corrections | Record UI-01/03/04/05, honest support failure, landscape accessibility and the inventory clarifications; keep settled P2-P4 policies unchanged |
| Trust and telemetry | Real GoTrue captcha-token integration requires a recorded correction and later live proof; do not claim decorative Turnstile protects auth. PostHog/Sentry follow consent; GA/Hotjar/New Relic need explicit keep/drop records, recommended drop rather than unconditional loading |
| Phase sequencing and drops | Publish early P7 key dependency and P6 joint acceptance obligations. Lead records approved thrown/prototype rows using the required canonical approval provenance; neither advisor silently drops a row |

No unresolved advisor design objection remains after **P5-DOCS-FINAL-A** acknowledgement. Fable's independent review of both originals is still a separate input; this agreement does not preempt it or authorize implementation. Ready for lead reconciliation, then holding.
