# P5 implementation contract

Lead coordination under mu's delegation. Read `decisions.md`, final Opus/Sol responses and the inventory union from both proposals. `P5-DOCS-FINAL-A` is final; older A/C relays are superseded. Fable's independent review is advisory, with dispositions in decisions.md. No implementation or green status is implied by this contract.

## Ownership

| Owner | Exclusive slice | Publish before consumers depend on it |
|---|---|---|
| p1X, Opus | Existing/new core composed app/session/route/settings/feedback/support/flags/message presentation and fixture integration; sole core module-declaration owner. Integrate leaf modules supplied by other owners. No billing policy or document-byte duplication. | Concrete Event/Effect/View/port contracts and shared message-key representation; coordinate migration of existing presentation consumers. |
| p22, Opus | New transgate/client and transgate/wasm; actual Rust GoTrue PKCE, native/wasm HTTP, browser capability ports, effect execution, shared stream decoding, generated TS/fixture exporter. | Presenter/platform interfaces, auth/session lifecycle and exact dependency manifests. No taskr product changes. |
| p1T, Sol | transgate/api/http/server host composition, session bootstrap, feedback/support persistence coordination, fenced receipt read, browser session start cutover, static serving and HTML checkout adapter with billing owner. | Exact DTOs/routes and ownership/erasure semantics. Coordinate shared files before editing; no AI/billing policy in handlers. |
| p1R, Opus | Billing purchase/invoice presentation, owner invoice context, display-versus-charge DTO and billing HTTP changes; required feedback/support persistence changes in jobs-postgres against published core ports. | Purchase/document facts and migrations; financial facts remain server-derived. Coordinate shared host handlers with p1T. |
| p1W, preserved processor/document owner | Shared pure analysis/invoice document model leaf and in-memory document byte adapter in/alongside transgate/export; source reference capture and independent-reader regression source. | Actual PDF/font/shaping dependency selection with wasm/native support, typed bounded document interface and font port. No host analysis-export route, JS encoders or generic document engine. |
| p21, Opus | New AI panel presentation leaves and desktop/phone AI views, stories and component regressions; consent/cache/watch/history/copy/export/rating source flows. | Panel interfaces to p1X and document use to p1W. Existing llm-core/http and commercial AI policy stay unchanged absent a concrete diagnosed defect. |
| p1Z, Opus | Desktop/phone workspace/upload/history/transcript/player views, stories, component regressions and browser media/file adapters. | Mechanical platform needs to p22; missing business transitions to p1X, never duplicate them in TS. |
| p1V, Opus | Web package/app frame/auth/settings/commerce views, shared accessible UI primitives, theme/route containers, Storybook/build configuration. Delegated root token generator outputs/roles, coordinating exact edit window with integration owner. | Shared UI primitives, layout contracts and requested locale keys. No handwritten VM fixtures. |
| p24, Sol | Locale import/validation and reachable P5 translations under transgate/web/locales plus scripts/p5-locales; preserve source provenance and fallback inventory. | Catalog/key/argument contract to p1X/p1V. All16 locales, no English-copy filler. Source repository read-only. |
| p1Y, Opus | Integration runtime/browser harness, packaged Docker/web integration, root workspace member/lock integration when manifests are coherent. | Owned local runtime/env and package registration contract. No builds or smokes until lead authorizes the coherent execution slot. |
| Lead | Decisions, ownership/conflict resolution, integrated verification, evidence/ledger/gate/status updates and final review dispatch. | No product implementation. |

No two owners edit shared declarations or migrations concurrently. Root Cargo/lock and Docker changes have one integration owner. Existing P2-P4 source is frozen during the remaining verification; new unlinked P5 leaves may be authored, but owners must announce metadata/integration holds before creating partial workspace members. Do not create a Cargo.toml without a coherent source root.

## Required complete behavior

- Preserve the union of WEB and cross-ledger flows in both proposals. P6/P7/P8 dependencies stay visible and prevent whole-row closure; never ship placeholders or fake key administration.
- Existing Rust queue/history/transcript/privacy/billing/LLM behavior is reused. One session presenter; every asynchronous result is fenced by session and selection. One mounted device branch, player and watch; source touch/width/flag selection.
- File measurement/create/options/sign/PUT/Start ordering, draft restoration, YouTube resolver, history cursors, editing/seek and translated/freemium export distinctions are preserved. Browser media stays in File/Blob handles rather than copied through JSON.
- GoTrue PKCE covers password/signup/magic-link/Google/recovery/password-update, secure verifier storage, code exchange and cross-tab refresh. No privileged browser keys or browser database access.
- Existing AI start header then watch, same-key uncertain replay, explicit new-key retry, typed analysis decoder and shared text replacement reducer. No invented cancel controls or automatic model retry.
- Documents export captured displayed content, including partial/interrupted analysis, at click time. Real six-shape DOCX/PDF/TXT/MD and invoice bytes, locally supplied licensed fonts and Arabic shaping. No fake PDF or latest-result substitution.
- Commerce retains separate display and charge totals, coupons/terms/provider handoff and truthful uncertainty. Real HTML provider return, owner status read, invoice/admin isolation, manual ordinary cancellation.
- Erasure and consent remain their real state machines: no full-erasure claim for unresolved writes/LLM retention; no end-user exception control or contact recreation. Receipt read is owned and read-only.
- Complete reachable locale catalogs, generated tokens, both themes, keyboard/focus/IME/dialog/status semantics and source-order views. Message keys replace old presentation copy with every caller migrated, no compatibility aliases.

## Execution and acceptance

Workers write substantive domain-first regressions and platform components with their feature; no builds/tests/lint/format/browser/provider calls mid-flight. Publish source-ready handoffs with exact selectors, dependencies and unverified boundaries. Lead runs coherent checks and routes concrete failures to the saved owner. No source-only claim is labelled reproduced or passed.

Proof must include generated Rust fixtures/TS and actual wasm, both browser projects over owned Axum/PG/GoTrue and loopback external services, actual uploaded/played/exported bytes, independent PDF/DOCX readers, all16 locales, session/ownership races, and the packaged nonroot app. Stories and backend tests do not substitute for complete browser flows. No live/production provider calls are authorized.

Final acceptance receives a bounded independent Fable medium or Claude review after coherent evidence exists. Review fixes return to implementation owners. Lead records remaining cross-phase/live gates without weakening them. Book edits/builds remain deferred to the very end.

## Browser and UI/UX checkpoints

Per mu's explicit instruction, lead chooses readiness/timing; the independent testing agent p1Y executes the browser and UI/UX checks. Checkpoints: coherent generated-fixture screens for visual/accessibility mechanics; complete real desktop/phone wasm flows over owned services; then packaged-app navigation, playback, downloads and checkout. Implementers do not self-certify their UI, and lead does not substitute its own browser inspection. Record screenshots, keyboard/focus behavior, source-order differences, actual outcomes and all live/device limits.
