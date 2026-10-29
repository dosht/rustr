# Manual acceptance: current Transgate shells

This is mu's review guide for the current P2, P3, P4, P5 and P8 work. Start with the web app on desktop, then a narrow phone viewport. Android and terminal checks follow. P6 public-site content is outside this review.

## What this review can confirm

The application, sign-in service, database, editing and exports are real. Soniox, DeepSeek, storage, payment and analytics services are local fixtures. Audio produces the sample transcript below, not a transcription of your recording. AI produces preset answers, not an assessment of the transcript.

Use this to judge whether the product flows, wording, navigation and results make sense. It cannot confirm transcription accuracy, AI quality, real payment handling, external Google sign-in, provider retention or production delivery. Those still need live acceptance.

The automated baseline passed nine Android checks, native/CLI/TUI and Linux Swift checks, plus the recorded web checks. Your approval is separate: an automated pass does not mean the experience meets your expectations. Actual iOS UI compilation and interaction remain unverified without Xcode.

## Easiest way to start

Ask Codex in this repository:

> Start the manual acceptance environment from docs/runbooks/manual-acceptance.md using current sources. Configure the sample provider responses, give me the web and local inbox URLs, and keep it running while I review. Do not use live providers. Start a visible Android emulator if I request Android. Record my feedback without changing behavior until I approve the fix.

There is no running review environment left from acceptance. Old localhost links and APK endpoint settings are not reusable after its disposable backend stops. Each new run has a new database, ports and account list.

## Self-service web startup

These commands are for the existing Linux development machine, from `/home/mu/src/rustr`. They require working Docker, Rust, Node/npm, Python3, and the repository's wasm toolchain. The runtime itself never installs or pulls anything. The initial build can take several minutes.

In terminal 1:

```bash
cd /home/mu/src/rustr
cargo build --locked -p transgate-server -p billing-provision -p transgate-cli -p transgate-tui
TASKR_WASM_BUILD_STD=1 ./scripts/build-transgate-wasm.sh
(cd transgate/web && npm ci && npm run build)
docker image inspect postgres:17-alpine public.ecr.aws/supabase/gotrue:v2.197.0 public.ecr.aws/supabase/mailpit:v1.31.3 >/dev/null
review_dir=$(mktemp -d /tmp/rustr-manual-review.XXXXXX)
mkdir -p "$review_dir/target/debug"
cp target/debug/transgate-server target/debug/billing-provision "$review_dir/target/debug/"
cp -a transgate/web/dist "$review_dir/web"
printf 'Review directory: %s\n' "$review_dir"
python3 -u scripts/p5-web/runtime.py --root "$review_dir" --web-dir "$review_dir/web" --log-dir "$review_dir/evidence"
```

On a rustup installation with the wasm target already installed, omit `TASKR_WASM_BUILD_STD=1`. If a prerequisite fails, stop at that failure instead of continuing with older binaries. If the Docker image check fails, the missing images must be pulled before starting the runtime.

Leave terminal 1 running. It prints `P5_APP_URL`, `P5_AUTH_URL`, `P5_MAILPIT_URL` and `P5_FIXTURE_URL`. In terminal 2, copy those four lines and prefix each with `export`, for example `export P5_APP_URL=http://127.0.0.1:12345`. Use the actual printed values, not that example port.

### Sample transcript and AI responses

Run this once in terminal 2 before uploading. It prepares a small silent audio file, a deterministic transcript, a known YouTube fixture and ten responses each for analysis and chat. The marker `zqmanualzq` deliberately appears in the transcript so the local AI fixture can select its response.

```bash
cd /home/mu/src/rustr
python3 - <<'PY'
import json
import os
from pathlib import Path
import urllib.request
import wave

origin = os.environ['P5_FIXTURE_URL']
def plan(path, body):
    request = urllib.request.Request(origin + '/__p5/' + path,
        data=json.dumps(body).encode(), headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=30) as response:
        assert response.status == 200

plan('soniox-default', {'duration_ms': 30000, 'tokens': [
    {'text': 'zqmanualzq We agreed to review the launch on Friday.', 'start_ms': 0, 'end_ms': 4000, 'speaker': '1'},
    {'text': 'Ada will prepare the customer notes.', 'start_ms': 5000, 'end_ms': 8000, 'speaker': '2'}
]})
plan('youtube', {'video': 'manualdemo1', 'title': 'Manual review sample', 'duration': 'PT30S',
    'cues': [{'text': 'zqmanualzq We agreed to review the launch on Friday.', 'start': 0, 'duration': 4}]})
analysis = Path('transgate/core/src/ai/analysis/fixtures/meeting.txt').read_text()
for _ in range(10):
    plan('llm-plan', {'marker': 'manual', 'mode': 'analysis',
        'steps': [['delta', analysis], ['finish', 'stop']]})
    plan('llm-plan', {'marker': 'manual', 'mode': 'chat',
        'steps': [['delta', 'This is a preset test answer. Ada will prepare the customer notes.'], ['finish', 'stop']]})
with wave.open('/tmp/rustr-manual-review.wav', 'wb') as audio:
    audio.setnchannels(1)
    audio.setsampwidth(2)
    audio.setframerate(16000)
    audio.writeframes(bytes(16000 * 2 * 30))
print('Ready: /tmp/rustr-manual-review.wav')
print('YouTube fixture: https://www.youtube.com/watch?v=manualdemo1')
PY
```

If you exhaust the planned AI replies, run this preparation again. Keep the marker in at least one transcript line while testing AI. Arbitrary YouTube links are not configured. Audio playback of the generated file is silent by design; use a small non-sensitive recording to judge playback controls, while expecting the same preset transcript.

Open `P5_APP_URL/home` in the browser. Create a new account such as `mu-review@example.test` with a disposable password. Open `P5_MAILPIT_URL` to read its confirmation email, then confirm and sign in in the same browser. Mailpit is the local inbox; no real mailbox is needed. Use a second test account for destructive checks.

## Review route: about 30 to 45 minutes

Record each row as Accepted, Needs change, Failed or Not tested. Desktop and phone can receive different verdicts. A browser phone viewport tests responsive web, not the Android or iOS app.

| Area | What to do | What should be true |
| --- | --- | --- |
| First visit and account | Sign up, confirm through the local inbox, sign in, refresh, sign out and back in. | The next action is clear. Refresh restores the account. Sign-out removes access to private screens. |
| Upload | Upload `/tmp/rustr-manual-review.wav`, choose language and speaker options, then start transcription. | Progress and status are understandable. The job finishes with the preset transcript. No duplicate job appears. |
| History | Leave the transcript, find it in history, reopen it, then refresh the browser. | The same job, status and transcript remain available. |
| Transcript | Edit a line, save it, leave and reopen. Make another edit and cancel it. | Saved text persists; cancelled text does not. Speaker labels and timestamps remain understandable. |
| Playback | Try play/pause and jumping to a line with a non-sensitive recording. | Controls respond and the playback position is clear. Preset timestamps need not match your recording. |
| Exports | Download transcript/subtitle formats offered by the screen. Open the files. | Files are readable and contain the expected text; names and formats are useful. |
| AI consent | Inspect the consent explanation before enabling AI, then request analysis and ask a chat question. | Consent is understandable. Preset results appear without a stuck loading state. You can distinguish transcript content from AI output. |
| AI documents | Open the meeting analysis and download its PDF. | Sections and text are readable, including non-ASCII text. The PDF opens and matches the displayed preset result. |
| YouTube | Add `https://www.youtube.com/watch?v=manualdemo1`. | The configured sample becomes a usable job. The title and transcript are fixtures, not a real video. |
| Settings and language | Open, close and reopen Settings. Change language. Refresh or reopen the shell. | Settings does not open by itself. Navigation remains usable and the selected language is retained where supported. |
| Balance and billing | Inspect balance, plans and billing history. | Units, prices, empty states and actions are understandable. This is a UI review, not a payment acceptance. |
| Privacy | On a disposable account, inspect retention/consent controls and delete a sample job. | Warnings match the action and deleted content is no longer accessible. Record questionable promises for P6 review. |
| Phone web | Repeat sign-in, upload, edit, Settings and export at a narrow viewport. | Buttons, dialogs, keyboard focus and scrolling remain usable without hidden actions or unintended horizontal scrolling. |

Do not approve payment settlement by entering a real card. Payment success/failure, quotas, provider errors and expiry require deliberate fixture plans. Ask Codex to stage the specific scenario before reviewing it. Google completion, real email delivery, real transcription/AI quality and P6 public pages are not part of this local walkthrough.

## CLI and TUI

The terminal shells expect API and auth under one origin. In terminal 3, copy the actual `P5_APP_URL` and `P5_AUTH_URL` lines from terminal 1, prefix both with `export`, then keep this local proxy running:

```bash
cd /home/mu/src/rustr
python3 transgate/apps/android/test-support/auth_proxy.py --app "$P5_APP_URL" --auth "$P5_AUTH_URL" --port 44921
```

This proxy is sufficient for local password sign-in and API requests. Do not use it as the web app URL or for Google/magic-link/recovery acceptance. Those require a separate callback setup.

In terminal 4:

```bash
cd /home/mu/src/rustr
export TRANSGATE_HOST=http://127.0.0.1:44921
./target/debug/transgate login --email mu-review@example.test
./target/debug/transgate whoami
./target/debug/transgate jobs
./target/debug/transgate upload /tmp/rustr-manual-review.wav --language en --speakers
./target/debug/transgate jobs
./target/debug/transgate-tui
```

The CLI prompts for the disposable password. Use the job ID from `jobs` with `transgate show ID`, `transgate transcript ID`, and `transgate export ID --out /tmp/rustr-manual-exports`. Replace `ID` with the actual number.

Check whether results and failures are understandable, exports are easy to find, and the TUI's displayed keyboard help lets you navigate without guessing. Confirm that a new CLI upload does not start unrelated drafts left in the web app. Use `transgate logout` when finished.

## Android and iOS

For Android, ask Codex to start a visible emulator against this same running backend. The previously accepted APK points at stopped services and should not be treated as ready for this new run. The setup must rebuild endpoint configuration and reverse the current app, auth and fixture ports so uploads and playback can reach their URLs. Start with password sign-in using the account created in the web app.

Repeat sign-in, history, saved/cancelled edits, Settings open/close, app relaunch and sign-out. Then explore upload, playback, AI and exports and report any differences from web. Purchases and API-key administration intentionally hand off to the web; native displays balance rather than a native purchase flow. Google and cold/warm callback acceptance need the scripted fixture described in `transgate/apps/android/test-support/README.md`, not a live Google account.

Android's keyboard may show an unrelated contacts permission dialog on a fresh emulator. Deny it before judging the app; record if it interferes. The automated run used a disabled software keyboard, so actual touch-keyboard usability is still valuable manual feedback.

For iOS, use the Mac/Xcode instructions in `transgate/apps/ios/README.md` with endpoints reachable from that Mac. Linux Swift checks are not an interactive iOS build. Record iOS as Not tested until it actually runs on an Apple simulator or device.

## Feedback to send back

Copy this once per issue, or give the same information in chat:

```text
Shell and viewport/device:
Area and verdict: Accepted / Needs change / Failed / Not tested
What I tried:
What I expected:
What happened:
Screenshot or recording:
Job ID and approximate time:
Does it happen every time?
Priority: blocks review / important / polish
```

Feedback on wording or product behavior is useful even when nothing crashes. Say whether it is a defect or a preference if you know. Do not include passwords, tokens or real personal recordings in the report.

At the end, send: "I accept [areas]. Please change [issues]. I have not tested [areas]." This records your product approval without treating untested areas as approved.

## Stop and preserve the review

Save screenshots and feedback before stopping. Press Ctrl+C in terminal 3 to stop the proxy, then Ctrl+C in terminal 1 to stop the disposable backend and remove its owned containers/network. Close the emulator if one was started. Stopping removes the test database and its accounts/jobs; restarting does not resume them. Export anything you want to keep first.

The runtime prints its evidence directory on exit. Keep that path with the review feedback. If startup or a flow fails, share the step and that directory rather than repeatedly restarting and losing the failing state.

## Verification of this runbook

Commands and fixture payloads were checked against the current runtime, CLI arguments and provider fixture handlers. This document was not itself executed as a fresh end-to-end manual review. Previous acceptance receipts remain in `docs/milestones/p8-shells.md`, `docs/milestones/p5-web.md` and the local closure archive.
