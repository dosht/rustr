# Android callback acceptance fixture

`auth_proxy.py` runs only on loopback and forwards normal API/auth requests to a dedicated local acceptance backend. Its test endpoint obtains a real GoTrue password session for the supplied dedicated account, then issues a one-use authorization code bound to the supplied SHA256 PKCE challenge. The callback exchange rejects a wrong verifier and replay. This verifies Android callback routing, verifier preservation and session persistence, without claiming live Google OAuth acceptance.

The backend account needs a completed newest job with transcript lines for the existing edit/persistence tests. Do not point the fixture at production or another acceptance runner's account.

```sh
python transgate/apps/android/test-support/auth_proxy.py --app "$APP_ORIGIN" --auth "$AUTH_ORIGIN/auth/v1" --port 44921
adb -s emulator-5556 reverse tcp:44921 tcp:44921
```

Build both APKs with `transgate.apiUrl=http://127.0.0.1:44921/api`, `transgate.authUrl=http://127.0.0.1:44921/auth/v1`, and `transgate.webUrl=http://127.0.0.1:44921`. Run the instrumentation runner with `-e email`, `-e password`, and `-e pkceFixture http://127.0.0.1:44921`. Keep account credentials in local evidence files.

An official Android emulator can run the suite instead of Waydroid. Use the assigned ADB serial and wait for `sys.boot_completed=1` plus available package and activity services before installing. The reverse mapping above makes the host's loopback fixture reachable from the emulator; update the serial for your device. Software emulation without KVM can be slow or unstable, so a successful build or connected ADB alone does not establish runtime acceptance.

The suite contains nine tests. The successful callback selectors are `SignedInFlowTest#warmAuthReturnExchangesPkceAndRestoresSession` and `SignedInFlowTest#coldAuthReturnExchangesPkceAndRestoresSession`. Both assert signed-in UI, encrypted vault session, consumed verifier, a single exchange, and session restoration after recreating the activity. Callback URLs come from the generated core `webUrl(SITE_URL, Route.Home(HomeView.Default), "en")` before adding the code, matching the real sign-in redirect. With the default origin this is `dev.transgate.app://auth/home?code=...`; the bare auth origin is not an app route.

`SignedInFlowTest#settingsOpenAndCloseReturnsToTheSignedInFrame` verifies that Settings opens on request, closes back to the signed-in frame, and can be reopened and dismissed again. The other six tests cover invalid callbacks, balance, editing/persistence, cancelling an edit and sign-out. Require an `OK (9 tests)` result with no failures or process crash; the ADB command's exit status alone is insufficient.

On failure, the test watcher captures a PNG screenshot and XML UI hierarchy before pressing Home. Files are stored under `/sdcard/Android/data/dev.transgate.android/files/acceptance-failures`, named with the test and timestamp; the `TransgateAcceptance` log tag reports their prefix. Preserve these with instrumentation output and logcat, and distinguish the current run's timestamps from earlier captures. Evidence can contain the dedicated test account's email or other test data and should remain local.
