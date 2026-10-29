# P8 Mobile, CLI and TUI

## Scope and outcome

The Android, iOS, CLI and TUI shells share the Transgate core. This closure work repairs sign-in recovery, session isolation, transcript interaction and terminal upload behavior. P6 remains reserved for a separate session.

Independent native, CLI, TUI and Linux Swift verification passed on 2026-10-28. Android acceptance passed all nine tests on the official API29 emulator, authorized by mu instead of Waydroid. The complete run reported `OK (9 tests)` with no failure or crash markers.

## Repairs

- Terminal credentials, PKCE state and refresh locks are deployment-scoped. Concurrent commands serialize refresh and vault updates; lock failures become visible errors.
- CLI upload and YouTube commands start their own batch without deleting restored server drafts. The core fences stale polling results.
- Android consumes authentication callbacks through a fresh presenter and shows Settings only while the core open flag is true.
- iOS publishes current presenter state through one ordered lane and retains the locale chosen through core intents.

## Independent evidence

`/tmp/rustr-codex-lead/tester-final-report.md` records current-source workspace verification with real PostgreSQL: 1619 parent-suite tests plus five vault child workers; CLI19, native15 and TUI93; generated-binding checks; and Linux Swift with real GoTrue password readiness. The source-reference capture test was excluded because its independent fixture directory was unavailable.

Source review receipts are `review-native-final-report.md`, `review-ios-report.md`, `review-final-web-android-report.md` and `review-android-settings-report.md` in the same directory. Completed receipts are also archived under `/home/mu/.cache/rustr-integration/codex-closure-20261028`.

## Android evidence and limits

API36 software emulation suffered startup ANRs before app assertions. API29 executed the suite and exposed the Settings visibility defect. Subsequent testing verified the Settings repair and identified two test-environment problems: a callback fixture omitted the core home route, and the emulator keyboard requested contacts access during editing. The fixture now uses the generated core URL; contacts permission remains denied. Earlier failed receipts remain preserved.

Successful callback acceptance uses a local one-use PKCE fixture backed by a real GoTrue password session. It verifies callback delivery, verifier consumption, persisted session and activity recreation. It does not establish live Google OAuth acceptance, which remains P9.

Linux Swift compiles and exercises the shared native integration. SwiftUI and Apple-specific adapters have not been compiled or run with Xcode. Mac build instructions remain in `transgate/apps/ios/README.md`; no Apple device acceptance is claimed.

## Final Android acceptance

The final build delegates task-clearing callbacks to the replacement activity. A diagnostic trace proved the outgoing and incoming activities both handled the one-use code; the old presenter completed successfully after disposal while the new screen showed an invalid-link error. Decision D21 records the Android lifecycle contract and repair. Temporary diagnostics were removed.

The independent full-suite run passed nine of nine tests in222.655 seconds, with unchanged assertions and timeouts. It verified cold and warm callback exchange, verifier consumption, session persistence and restoration, invalid callbacks, signed-in balance, saved and cancelled edits, Settings open/close and sign-out. No new failure capture or crash marker was produced.

Final app SHA256: `14f46ffaf85acf320f08d1d4e959a7070d1cc1ef972e0ceaa946ac247ee107f0`. Unchanged test APK SHA256: `a5a79508376765e5a00d96a366a0603550b5e2c5d03b57f95b788cc1e3dd6aba`. Native library SHA256: `df40ccce0e29c7059804dcc8b5bfa9e2562164dcd9217d1216b8ab1cecad25a7`. Input and installed artifacts were independently verified.

Final report: `/tmp/rustr-codex-lead/android-harness5-report.md`. Runtime receipts: `/tmp/rustr-android-harness5`. Earlier failed and diagnostic runs remain separate evidence. The emulator keyboard was disabled with contacts permission denied during acceptance to prevent an unrelated system permission dialog.

Independent source review approved the bounded lifecycle repair. The owned emulator, local backend/proxy, disposable containers/network and tester tab were stopped after acceptance. Final and earlier failed receipts are preserved in the closure archive. Mu authorized committing the accepted repairs and manual review runbook on 2026-10-29.
