# P5 web advisory brief

## Outcome and ownership

Implement PRD P5 without changing the established product flows: Svelte 5 plus the shared Rust/wasm presentation, pure desktop and phone views, all 16 UI locales, generated root tokens and light/dark/system themes. Paper Moss remains the default. The book waits until final delivery.

Independent proposals: Opus in `opus/proposal.md` (w23:p1V), Sol in `sol/proposal.md` (w23:p24). Each reads the same evidence but not the other's proposal until the lead dispatches cross-critique. Proposals only: no implementation, builds, tests, formatters, browser or provider calls. Preserve the saved agent sessions and accurate model provenance.

## Existing evidence to reuse

- `docs/prd.md`, especially sections 2, 3, 7 and P5-P10.
- `transgate/docs/discovery/screens.md`: 76 WEB parity rows, including explicitly separate native/terminal obligations. Preserve every applicable desktop/phone flow; do not claim P5 verifies the other shells.
- Other discovery ledgers for billing, AI, jobs, privacy, languages, platform and API. The original repository `/home/mu/Business/Transgate` is read-only. Use the cited live source paths, not an invented screen inventory.
- P2/P3/P4 decisions and implementation contracts. In particular: server-first FFmpeg tools, no RunPod, exact legacy upload order, D15 unresolved external writes, D9 payload-free LLM retention facts and explicit operator exceptions, D5 original chat question, D8 analysis corrections, and bounded shared streaming.
- Existing `transgate/core/src/presentation`, `transgate/api`, `transgate/http`, billing ports and server composition. Reuse actual published contracts; do not propose a second business state machine in TypeScript.
- `apps/web` is Svelte 5/Vite/Storybook with pure desktop/phone views and Rust-generated fixtures. `taskr-client`, wasm and UniFFI have an executed shared streaming consumer. Keep taskr-core dependency-free and avoid unnecessary changes to taskr.

## Required proposal

1. A complete, bounded clause/flow inventory referencing all applicable WEB rows and cross-ledger obligations, with explicit P6/P7/P8 dependencies rather than omitted screens or fake placeholders.
2. The smallest client/presentation/binding/view split, actual reusable symbols and proposed public seams. Business meaning and authorization never move into a shell. Describe auth/session loss, routing, upload/media progress, editor/player mechanics, streaming, destructive operations and external checkout handoff.
3. Exact source-preserving desktop/phone step order. Enumerate any proposed defect corrections separately, with source evidence; do not silently fix or expand the UX.
4. Localization, accessibility, tokens/theme and fixture/Storybook strategy. Identify existing translation resources and avoid silently falling back to English for a supported locale.
5. Dependency choices and bounded parallel implementation ownership. Name shared prerequisites and integration owners. No generic engine, speculative facade, compatibility shim, fake backend or placeholder screen.
6. Executable verification: core transitions, generated contracts, component tests for both platforms, real wasm/browser flows against an owned server/database with loopback external fixtures, actual media/export bytes, and packaged Axum serving of the app. Separate synthetic/local proof from live OAuth/captcha/payment/provider and deployment gaps.
7. A short decision list with recommendations. User has delegated normal decisions to the lead; do not create unnecessary questions or reopen settled P2-P4 policy.

## Integration boundaries

P3/P4 are still being integrated and their canonical evidence is not globally green. Current taskr browser4/4 and real Linux Swift UniFFI listener pass; this does not prove Transgate web or an iOS UI. Full P4 Transgate actual-process run5 passed on its immutable snapshot, with D9/D15 exception-labelled erasure; later source cleanup still requires verification. Existing API-key administration/public-API dependencies must be implemented honestly or explicitly sequenced with P7, never rendered as working over an absent endpoint. Public/static/SEO and free-tool page dependencies must be distinguished from P5 and handed to P6 without losing their final gate obligations.

The lead owns root manifests/lockfile, canonical discovery/rules/parity status, decisions, gates and integrated execution. Advisors own only their proposal folders. After independent proposals, one direct cross-critique and a reconciled dispatch contract precede implementation.

## Bounded Fable review and usage decision

At mu's request, Fable5.1 HIGH performed one independent review of both frozen proposals (`fable/review.md`). No xhigh run. Claude `/usage` immediately before/after showed session22%→32%, weekly all models3%→5%, weekly Fable0%→1%. These rounded account-wide readings overlap concurrent Opus work, not an isolated attribution to Fable.

Lead decision: use **medium** for subsequent Fable work and reserve it for bounded reviews, not bulk implementation. No second review is assigned. Raw readings: `/tmp/rustr-p5-fable-usage-{before,after}.txt`. The Claude usage screen remains open in herdr `w23:tV`, pane `w23:p26`, label `claude-usage-keep-open`, for mu to inspect. Fable's preserved review session is `2026-10-25T14-58-44-614Z_01a17392-3706-71e3-aea6-5806911382de`.
