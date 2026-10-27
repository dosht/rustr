# P5: web app

Status: **gate met for every row that can be proven locally**, 2026-10-27. Six rows need live services and move to the P9 staging gate.

## What was built

The Transgate web app is a Svelte shell over the wasm core (`transgate/web`, `transgate/wasm`, `transgate/client`). Every screen has a desktop and a phone view; containers pick one from core layout decisions. Business meaning lives in `transgate-core` presentation state machines; views render view models and forward intents. 16 locales, light/dark/system themes, generated design tokens, Storybook stories from generated fixtures. One Docker image serves the SPA under `/_app` and the API from one origin (non-root, read-only).

Decisions: [P5 decisions](../plans/p5-web/decisions.md) (D1-D23) and the [implementation contract](../plans/p5-web/implementation-contract.md). Lead log of checkpoints and repairs: [lead-preparation.md](../plans/lead-preparation.md).

## Exercised evidence

| Check | Observed result |
|---|---|
| Parity rows | 167 rows: 119 proven on both Playwright projects (Desktop Chrome, Pixel 7), 6 live-only, 40 owned by P6/P7/P8, 2 dropped. P5-owned 125 = 119 + 6 ([disposition](../plans/p5-web/implementation/parity-disposition.md)) |
| C1 fixture screens | Candidate5 full matrix 3011/3012 audited, 0 axe violations; candidate8 changed stories 935/936 audited, 0 violations (one environment network glitch each) |
| C2 browser flows | Candidate8 all pass: PKCE sign-up, magic link, recovery, Google authorize (local half), billing and Iyzico loopback checkout, invoices, AI analysis and chat with second-tab reattach, YouTube, export, history paging, list polling (D11), settings, consent-gated telemetry, erasure |
| C3 packaged image | `sha256:3ba423f92231...` non-root, read-only, 0/6 context canaries, D20 asset types and caching, packaged sign-up, upload, AI, YouTube, checkout and erasure; only non-loopback request is the recorded YouTube contact (D12) |
| Suites on candidate8 | `transgate-core --features ts --lib` 433, all `transgate-client` suites, P4 Transgate actual-process scenarios, wasm release |

Candidate8 was frozen read-only from commit `3e5f41d`: `/home/mu/.cache/rustr-integration/s1-20261027073103/logs/next-p5-c/report.json`.

## Repairs found by the checkpoints

Candidates 3-7 found and fixed, each with a failing-first test: start-URL parsing (PKCE callback, deep links, locale prefixes), own-tab chat race and second-tab reattach, recovery page, local checkout badge and unhashed caching, i18n switch race, YouTube resolve decode, missing D11 list polling, desktop list scroll, labels and accessible names, Docker web packaging, and several harness and fixture gaps.

## What stays open

- **LIVE-P9:** Google sign-in completion (WEB-001/002, WEB-IDN-007/008) and Turnstile (WEB-IDN-023/024).
- Live providers, payment, mail and telemetry are proven only against loopback fixtures.
- Earlier phases (P2-P4) keep their own whole-clause gates; P6 public root and P7 API rows are listed in the disposition.

**Correction after closure (2026-10-27):** the five `transgate-export` document tests had failed since the first commit and were not part of any accepted run. Fixing the test reader exposed a real product bug, fixed in REPAIR-P5-13: analysis PDFs with a sentiment progression failed because no bundled font had the arrow (U+2192). Candidate8 contains that bug; the fix ships in the next candidate, which must include an analysis PDF export with sentiment.
