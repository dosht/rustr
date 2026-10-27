# P8 independent design brief

P5 is done and P7 is being implemented. PRD section 8 lets P2-P8 overlap, so P8 design starts now. This is independent design only, not an implementation authorization. The lead keeps decisions and gates. Two Claude advisors (Opus p10, Fable p27, medium effort) each write one proposal; cross-critique only on dispatch. No nested agents, builds, tests, emulators, provider calls or source-repository writes.

## Required outcome

Read `docs/prd.md` (P8 row, sections 2-8), `docs/architecture.md`, `transgate/docs/discovery/screens.md` (WEB-049..076 are the P8 rows) and the taskr shells as the reference pattern (`crates/taskr-ffi`, `crates/taskr-tui`, `apps/android`, `apps/ios`, the generated Linux Swift listener). Produce only `docs/plans/p8-shells/{opus,fable}/proposal.md`. Do not read the sibling proposal before cross-critique.

- PRD P8 scope: Android and iOS shells for the Transgate flows over UniFFI; CLI and TUI shells over the same core (upload, list jobs, fetch a transcript or translation, sign in). Done condition: APK on Waydroid passes the signed-in flow, Swift compiles on Linux, Mac build steps documented, CLI process tests and TUI render tests green.
- Core and Shells: every shell renders `transgate-core` view models and forwards intents; no business rule in Kotlin, Swift, CLI or TUI code. Reuse `transgate-client` (the same client the wasm shell uses) through a `transgate-ffi` crate, as taskr does.
- Inventory WEB-049..076 and any native/terminal obligations; map each to a shell and to existing core presenters. Name core gaps (presenters the web uses that native cannot, platform ports such as file picking, keychain, notifications, audio playback) as ports implemented by the shell.
- Which screens each shell carries (mobile: the signed-in flows; CLI/TUI: the PRD's smaller set), auth on native (PKCE with system browser vs in-app), secure session storage per platform, uploads with presigned URLs, playback.
- Design tokens generated per shell from the root file; semantic tones only.
- Executable acceptance run by Y: Waydroid APK signed-in flow, Linux Swift compile plus the generated listener, CLI actual-process tests against the owned runtime, TUI render tests.

## Proposal format

Source-backed row families, the smallest concrete architecture (crates, bindings, generated code, ports), exact seams with existing code, bounded ownership and order for existing OMP owners (p1X core, p22 client/ffi, p1T host if any route is missing, view owners), decision recommendations, and the acceptance plan. No implementation or stubs.
