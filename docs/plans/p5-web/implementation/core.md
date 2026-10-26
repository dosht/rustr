# P5 core: published contracts and message keys

Owner: w23:p1X (original Opus P5 provenance; same saved session resumed as user-authorized Astra high). Slice: core composed app, session, route, settings, feedback, support, flags and message presentation; fixtures and TS generation; core module declarations. Status: **workspace leaves, AppState/epoch, billing composition and complete exporter written; pre-S1, unrun.** No ownership transfer. Checks remain with p1Y on the lead's authorization; source-ready is not a green execution claim.

## 1. Message keys (D10)

`transgate_core::presentation::msg::{Msg, Ns, Arg}`. Wire shape:

```json
{ "ns": "app", "key": "p5.erasure.local_data_erased", "args": { "count": 2 } }
```

- `ns` is one of `common`, `app`, `ai`, `auth`. Language display names are `common` keys (the English language name), as the source pickers used them. No `transcription`, `translation`, `languages` or `dashboard` key is reachable from P5.
- `key` is the exact catalog key: the source's own key where one exists (natural English or flattened dotted auth labels), else a key listed below.
- `args` is always present (`{}` without arguments). Values are numbers, strings, or a nested `Msg` that the shell translates first. A numeric `count` selects i18next plurals.

### 1.1 New keys (no source text): English meaning to translate in all 16 locales

| ns | key | args | English meaning |
|---|---|---|---|
| app | `p5.support.send_failed` | none | Your message could not be sent. Please try again. |
| app | `p5.erasure.requested` | none | Your account is closed. Your data will be erased once every active subscription is confirmed stopped. |
| app | `p5.erasure.local_data_erased` | `count` (remaining remote steps) | Your data has been erased. Remote cleanup is still in progress ({{count}} remaining). |
| app | `p5.erasure.fully_erased` | none | Your account and all of its data have been erased. |
| app | `p5.erasure.erased_with_exceptions` | none | Your data has been erased, but some items could not be removed automatically. Our team is handling them. |
| app | `p5.erasure.request_failed` | none | Your account deletion could not be started. Please try again. |
| app | `p5.session.account_erased` | none | This account is unavailable due to account erasure. |
| app | `p5.session.unavailable` | none | Your account could not be loaded. Please try again. |
| app | `p5.balance.none` | none | - |
| app | `p5.balance.minutes` | `minutes` | {{minutes}} min |
| app | `p5.balance.hours_minutes` | `hours`, `minutes` | {{hours}} hr , {{minutes}} min |
| app | `p5.auth.invalid_credentials` | none | Invalid login credentials |
| app | `p5.auth.email_not_confirmed` | none | Email not confirmed |
| app | `p5.auth.rate_limited` | none | Too many requests. Please try again later. |
| app | `p5.auth.weak_password` | none | Password should be at least 6 characters. |
| app | `p5.auth.user_exists` | none | User already registered |
| app | `p5.auth.invalid_or_expired_code` | none | This link is invalid or has expired. Please request a new one. |
| app | `p5.auth.captcha_failed` | none | Captcha verification failed. Please try again. |
| app | `p5.auth.network` | none | Network error. Check your connection and try again. |
| app | `p5.auth.unavailable` | none | Sign-in is temporarily unavailable. Please try again later. |

The balance formats repeat the source's `Balance.tsx` text shape (`N min`, `H hr , M min`, `-`), which the source did not translate.

### 1.1a AI consent disclosure (D22)

These replace the source modal's unsupported promises (provider permanence, browser-only storage) and the settings confirmation that did not state its scope. The existing mapping stays (P4 D3, `presentation/ai/consent.rs`): declining in the modal is the refusal (`POST` consent false), which keeps completed results; the settings action is `DELETE` (revoke plus account-side purge), labelled as both turning AI off and deleting saved AI data. Provider deletion stays unverified (D9). `cleanup_pending` from the consent read is shown as still deleting, never as done. The renderer shows these instead of, never beside, the replaced sentences. Superseded and unpublished: `turn_off_keeps`, `deletion_scope`, `disable_confirm`, `delete_confirm`, `delete_action`, `delete_title`, `delete_button`.

| ns | key | args | English meaning | Replaces |
|---|---|---|---|---|
| app | `p5.consent.provider_processing` | `provider` | Your transcript content and chat questions are sent to {{provider}}, our AI provider, to create AI results. | modal processing bullets |
| app | `p5.consent.provider_retention` | none | We cannot verify how long the AI provider keeps this data or whether it deletes it. | modal "no permanent storage" bullet |
| app | `p5.consent.account_results` | none | Saved AI results, such as summaries, analyses and chat history, are stored with your account, not only in this browser. | modal "local caching" bullet |
| app | `p5.consent.decline_keeps` | none | If you decline, nothing new is sent to the AI provider. AI results already saved stay in your account. | (modal refusal, `POST` false) |
| app | `p5.consent.settings_delete` | none | You can later turn off AI and delete your saved AI data in Settings. This removes it from your account; it does not confirm deletion by the AI provider. | modal `ai` "You can revoke consent and delete AI-generated content at any time" |
| app | `p5.consent.turn_off_delete_action` | none | Turn off AI and delete saved AI data | settings action label |
| app | `p5.consent.turn_off_delete_title` | none | Turn off AI and delete your saved AI data? | settings confirmation title |
| app | `p5.consent.turn_off_delete_confirm` | none | AI features will be turned off and your saved AI results (summaries, analyses and chat history) will be deleted from your account. This does not confirm deletion by the AI provider. This cannot be undone. | settings confirm body `common` "This will disable all AI features and delete your AI-generated content (summaries, highlights, chat history). This action cannot be undone." |
| app | `p5.consent.turn_off_delete_button` | none | Turn off and delete | settings confirm button |
| app | `p5.consent.cleanup_pending` | none | Your saved AI data is still being deleted from your account. | (`cleanup_pending` after the `DELETE`) |

### 1.1b More app keys added by the leaf revision (English = key, source literals)

`app`: `Untitled`, `2+ speakers`, `No more jobs to load` (source `tapp`), `All Transcripts`, `My Transcripts` (phone header literals), YouTube add dialog server texts `Invalid YouTube URL`, `Account suspended`, `Video not found or is private`, `Too many requests. Please try again in a minute.`, `Failed to fetch video metadata`, `Failed to create transcript job`; new `p5.duration.minutes` with `minutes`: "{{minutes}} min" (queue file durations).

`common`: `Password updated successfully!` (moved from `app`; the catalog holds it in `common`).

AI panel keys supplied by p21 (English text is the key, `app` namespace):

| key | meaning |
|---|---|
| `Export failed` | A TXT or MD analysis export failed. |
| `Failed to load chat history` | The chat history request failed. |
| `The AI request could not be confirmed. Check your connection and retry.` | An AI start whose outcome stayed unknown after the one same-key replay. |
| `This answer could not be saved. It may disappear after refresh.` | The shown answer reached a not-saved terminal; it is visible now, not durable. It replaces the removed source key "Chat saved locally but failed to sync. Your messages may not appear after refresh." (no alias). |

### 1.2 Source literals the source rendered untranslated (now catalog keys; English = key)

`app`: the five YouTube resolver messages (`upload_queue::youtube_error_message`), `No YouTube video found in queue`, `An unexpected error occurred. Please try again.`, `Audio is not available.`, `N/A`, the twelve upper-case status labels (`DRAFT`, `NOT STARTED`, `PENDING`, `RUNNING`, `COMPLETED`, `FAILED`, `CANCELED`, `DELETED`, `SPEAKER LABELING`, `UPLOADED`, `PROCESSING`, `READY`), and p21's consent failures still in use (`Failed to load consent preferences`, `Failed to record consent decision`, `Failed to enable AI features. Please try again.`, `Failed to disable AI features. Please try again.`), `Failed to export as DOCX`, `Failed to export as PDF`. Withdrawn by D21, no alias: `Failed to record consent`, `Failed to revoke consent`.

`common`: `Please select an option and provide your feedback.`, `An error occurred while submitting your feedback. Please try again.`

`ai` (used by the source, missing from its catalog): `Analysis in progress…`, `Analysis in progress… taking longer than expected`, `Analysis in progress… (you may have had another tab open)`.

Keys found in the source catalogs are used unchanged: for example `app`: `Transcribing audio...`, `Error loading jobs`, `Translation`, `Transcript`; `common`: `Uploading`, `Preparing`, `Ready`, `Upload Failed`, `Profile`, `Subscription`, `Manage Data`, `Settings`, `Disappointed`, `Satisfied`, `Outstanding`, `Auto-detect`, `min`, `No transcript lines available`, `Error loading transcripts:`, the job status values (`completed`, `speaker_labeling`, ...), the YouTube dialog validation texts, the Manage Data texts and the language switcher names; `ai`: `AI Features`; `auth`: the Supabase Auth UI labels such as `sign_up.confirmation_text`.

Language names in pickers, list subtitles and transcript tabs are the stored English metadata name, not a catalog key (`LanguageDropdown.tsx:122-127`, `MobileLanguageSheet.tsx:99-104`); only auto-detect is translated. The settings language switcher names are `common` keys (`t(currentLang.name)`).

## 2. Leaf contracts (source)

| Module | Symbols |
|---|---|
| `presentation::upload_queue` | `PickedFile{handle,name,size,mime}`, `Event::{Picked{files, frame}, TabChanged{mode, frame}, YouTubeDialogOpened, YouTubeDialogClosed, YouTubeUrlChanged, YouTubeSubmitted, YouTubeAddFailed{failure}}`, `Effect::{MeasureDuration{file,handle}, Upload{file,handle,job,name}, AddYouTube, Telemetry{event}}`, `phone_accepts`, `UploadQueueVm{files_ready, total_minutes: Option<i64>, youtube_error, youtube_dialog, auto_detect_label, LanguageOption lists, phone_preferred, rows: FileRow{label, phone_label, progress, desktop_size, phone_size, duration_text}}` |
| `presentation::transcript` | `Media`, `Event::{MediaUrlLoaded, MediaUrlDue, NoAudioAlertExpired, NoAudioAlertDismissed, LinesFailed}`, `Effect::{LoadMediaUrl, ScheduleMediaUrl, ScheduleNoAudioAlert, Export{job, request}}`, `LineRow{time_text, start_seconds, end_seconds, speaker_value}`, `TranscriptVm{title, duration_minutes, minutes_unit, status_badge, is_translation, translation_flag, loading_message, empty, load_error, can_copy, freemium, show_tabs, tabs: TabOption, lines_tab_label, lines_tab_flag, speaker_options, phone_speaker_toggle, media, no_audio_alert}` |
| `presentation::job_list` | `view(selected, utc_offset_minutes)`, `JobRow{display_name, status_label, is_translation, can_download, can_feedback, duration, speakers, language (raw English), date_text, created_at_ms, selected, downloading, sidebar_name, sidebar_feedback}`, `JobListVm{initial_loading, loading_more, end_of_list, error, error_detail, not_available, untitled, auto_detect, recent: RecentRow (first 20, drawer), recent_empty}`, row download `Event::{DownloadRequested, DownloadFinished}`, `Effect::Export{job, request}` (server ZIP, EXP-016 recorded server-side) |
| `presentation::data_management` | all copy as `Msg`; `erasure_message(&ErasureView)`; `Event::ErasureFailed{failure}` |
| `presentation::{settings, feedback, support}` | `SettingsState/SettingsVm` (tabs, AI tab only with `ai`), `FeedbackState/FeedbackVm`, `SupportState/SupportVm` (UI-04 visible failure) |
| `presentation::{layout, route, dates, failure, telemetry, msg}` | `Viewport`, `layouts() -> Layouts{frame, transcript, youtube, phone_ai}`; `parse`, `href`, `is_app_path`, `public_root`; `job_date`, `day_month_year`; `ApiFailure`; `TelemetryEvent`; `Msg` |
| `presentation::app` | `AppState::{new(AppConfig{site_url, captcha}), update(AppEvent, now) -> Vec<AppEffect>, view(now) -> AppVm, is_current(Epoch), epoch(), ai_epoch()}`; `Epoch{session, consent: Option}`; `AppIntent` (adjacent `{type, intent}`); `AppVm::{Restoring, SignedOut, SignedIn, CheckoutConfirm, Erased}`. Signed-in billing fields and exact confirmation/invoice driver contracts are in section 4. |
| `presentation::billing` | `BillingMessages` maps purchase/coupon/renewal/subscription/invoice codes to `Msg`; `renewal_at_ms` is core-computed, formatted as a locale date by the shell. `ConfirmMessages{title,status: Option<Msg>,message,detail,redirect}` maps the confirmation outcome, including honest unconfirmed/no-countdown copy. |
| `account` | `session_profile`, `SessionProfile`, `Account::{submit_feedback, request_support}`, `AccountTx`, `SupportOutbox` (attempts-fenced) |
| `flags` | `FlagConfig`, `Capabilities` |
| `jobs::app::Jobs::drafts` | every own draft, oldest first |

## 3. Generation (feature `ts`)

`cargo test --locked -p transgate-core --features ts --lib presentation::fixtures::web::generate` writes `transgate/web/src/lib/core/types/*.ts`, `fixtures.json` (`{"<VmType>": {"<name>": vm}}`) and `fixtures.generated.ts`, explicit paths only, `large_int` as `number`. The typed companion contains the same serialized data as a contextually typed literal plus generated `Fixtures`/`FixtureFile` mappings, preserving tuple and discriminated-union types without assertions or `any`; JSON remains available for receipts. Core's `ts` feature enables `billing-core/ts`. The exporter includes all previous leaf roots plus `AppVm`, `SignedInVm`, `PurchaseVm`, `SubscriptionVm`, `InvoicesVm`, `ConfirmVm` and their reachable types. `ConfirmVm` is the billing presentation type, not HTTP `ConfirmView`. REPAIR-P5-1 authorizes working-source regeneration by p1X; Y still owns candidate capture and certification.

Before any output write, the exporter traverses all roots and their reachable exportable dependencies with one shared `ExportIntegrity` visitor. Distinct canonical Rust types may not share a TypeScript identifier or output path; a collision fails with both Rust identities. Generic instantiations share their canonical declaration but each concrete dependency graph is traversed. The same root list drives preflight and export. The selected command also includes `generate_rejects_colliding_nested_names` and `generate_rejects_colliding_root_paths`; these synthetic collision regressions never write files.

The focused publication-boundary regression is `presentation::fixtures::web::export_integrity_preserves_outputs_on_rejection`. It uses the same `publish_roots!` path as the real generator, first publishes two roots sharing one dependency plus a repeated root, then attempts conflicting nested names and root paths. Each rejected graph includes an earlier nonconflicting new type; the regression compares the entire existing output tree byte-for-byte, including both `fixtures.json` and `fixtures.generated.ts`, and rejects premature added files. Its temporary directory is isolated and removed even when an assertion panics.

Exact standalone selector (included in the lead-accepted NEXT-S1-C3 evidence; rerun with the typed companion in REPAIR-P5-1):

```sh
cargo test --locked -p transgate-core --features ts --lib presentation::fixtures::web::export_integrity_preserves_outputs_on_rejection -- --exact
```

This selector is deliberately outside the existing `presentation::fixtures::web::generate` filter. No new product API, serde change, output edit, or warning suppression accompanies it.

Auth fixtures are present in `export_json`: `AuthVm` (`sign_in`, `sign_up`, `magic_link`, `forgotten_password`, `busy`, `notice`, `error`, `captcha_required`) and `NewPasswordVm` (`empty`, `busy`, `error`, `success`). These are source entries, not a claim that generated files have been written.

`SignedInVm` fixtures: `desktop_new`, `phone_job`, `settings_open`, `billing_owner`, `purchase_open`, `special_offer`. `AppVm` includes these plus `restoring`, `signed_out`, `erased`, `confirmation_succeeded`, `confirmation_declined`, `confirmation_pending`. All are real state transitions with synthetic input data.

Billing fixture keys/names from `billing_core::presentation::fixtures::{purchase,subscription,invoices,confirm}` are folded unchanged under `PurchaseVm`, `SubscriptionVm`, `InvoicesVm`, `ConfirmVm`. `ConfirmMessages` uses the confirmation fixture names unchanged; `BillingMessages` uses `purchase_<name>`, `subscription_<name>`, `invoices_<name>`, projected from those same billing states. Stories do not hand-write copy VMs.

p23's analysis projection dependency is source-ready: `ai/analysis.rs` projects the public wrappers; `ai/analysis/types.rs` projects the six-shape graph with omission-only option properties, numeric `AnalysisRating`, and `AnalysisDecision`. The latter TS-only names avoid collisions with chat `Rating` and consent `Decision`; Rust names and serde behavior are unchanged. `AnalysisProgress.expected_content_sections` remains required and nullable.

## 4. Composed billing contract and remaining execution

R's final `billing.md` sections 7–10 are consumed. No remaining billing source dependency is identified; generated-contract publication remains gated by the evidence below. The leaf files remain R-owned; `presentation/app.rs`, `presentation/billing.rs`, `presentation/fixtures.rs`, core manifest and shared declarations remain p1X-owned.

### Confirmation

- `AppEffect::ReadConfirmation { epoch, sequence }` reads only the embedded `#tg-confirm` JSON; answer `AppEvent::ConfirmationLoaded { epoch, sequence, payload: Option<ConfirmPayload> }`. No HTTP confirmation or checkout call. `None` means a page with no embedded confirmation, the source GET failure page.
- Starting on `/checkout/confirm` bypasses auth restoration. `AppVm::CheckoutConfirm { locale, vm: Option<ConfirmVm>, messages: Option<ConfirmMessages> }` renders independently of a signed-in session. `vm: None` is loading, not success or failure.
- `AppEffect::Confirmation { epoch, effect: ConfirmEffect::Track(...) }` forwards billing analytics once, with the shell's consent gate. Core handles `NavigateHome` as `Leave` to the localized public root.
- `Timer::Confirmation { sequence }` ticks once per second while countdown is positive. A route change advances the sequence; old payloads/timers and duplicate payloads cannot restart the countdown or analytics. Pending/unconfirmed results schedule no countdown.

### Invoices and overview

- `/billing` creates `InvoicesState` after the verified session profile arrives; `customer_lookup` supplies its admin capability. `AppEffect::Invoices { epoch, ticket, effect }` and `AppEvent::Invoices { epoch, ticket, event }` wrap the published billing effects/events; replies echo the originating effect's ticket. Every reply and final document delivery uses the session epoch.
- `AppIntent::Invoices(InvoicesIntent)` supports `Preview { payment_id }`, `ClosePreview`, `Download { payment_id }`, `LookupEmailTyped { text }`, `LookupRequested`, `LookupCleared`, `TemporaryBillToTyped { text }`. User intents apply only on `/billing`; invoice data is discarded on session reset.
- `SignedInVm` adds `invoices: Option<InvoicesVm>`, `billing_messages: BillingMessages`, `special_client_alert: Option<SpecialClientAlert>`.
- `BalanceView` gains `special_client_alert`; p22 maps it from the existing overview response. The host message is verbatim, never recomputed from percent; session reset clears it. Dismissal stays UI-local per R section 10.
- `InvoicesEvent::DocumentFailed { payment_id, failure: PurchaseFailure::Unavailable }` is the capture/render/font/PDF/preview/save failure result. R's leaf clears that payment's preview, preserves usable rows and exposes `InvoicesVm.document_failure`; core maps it to `BillingMessages.document_failure` using `app` "An unexpected error occurred. Please try again." The next preview/download clears it. Do not substitute a download for preview or report failed delivery as successful.
- Held preview delivery additionally calls `AppState::is_current_invoice_preview(ticket, payment_id)` immediately before `Platform.preview`. It checks the current signed-in billing route, the exact ticket and R's allocation-free `is_previewing(payment_id)`. `invoice_preview_ticket()` exposes the current counter. Preview, close, lookup/context/Bill-To changes, route changes and session reset advance it; closing and reopening the same payment invalidates an old renderer. Old-ticket document failures cannot clear the newer preview. Downloads retain their captured document and session/disposal fence.

### Billing message meanings

`presentation/billing.rs` is the executable code-to-`Msg` mapping of R section 9. p24 owns all-locale resources. Four new `app` keys, no arguments:

| Key | English meaning |
|---|---|
| `p5.billing.no_active_subscription` | No active subscription to cancel |
| `p5.billing.payment_unconfirmed` | We could not confirm your payment yet. Do not pay again; check your billing history shortly. |
| `p5.billing.customer_not_found` | No customer found for that email |
| `p5.billing.lookup_failed` | Failed to load customer invoices |

Existing `common` keys stay exact to R section 9, including `redirectMessage` with `count`; renewal date arithmetic is core-owned, locale date rendering is a shell mechanism.

### Evidence boundary

Y's preserved `/tmp/rustr-p5-tsgen/run1` failed compilation at p21-owned `chat.rs:539` (E0502), with no generator execution or generated changes. After p21's repair, run2 passed the exporter and wrote 142 TS files plus 21 fixture roots. Run3 also passed the exporter and corrected optional property projections, but publication **failed** inspection: `MeetingAnalysis` imported consent's `Decision.ts` instead of the analysis struct. In run4, p23's `AnalysisDecision` rename and the exporter guard compiled; both collision regressions passed, and real generation failed before any write on the next collision: data-management and billing `ConfirmVm`. The data-management projection now has the TS-only name `DataManagementConfirmVm`, retaining billing's published `ConfirmVm` root and every Rust/serde name.

Y's run5 passed compilation and all three selected tests, then inspected and published 144 TS files and 21 fixture roots. Both repaired dependency names, all 49 omission-only properties across 17 content structs, numeric aliases and local type-import resolution were confirmed. Receipt: `/tmp/rustr-p5-tsgen/run5/report.json`; fixture SHA256 `602865cdf1c9da5786b5234a8287f036912d09a91715e1ef40aa6eb31a9137b2`. The later shared-publication-path refactor and standalone failure-boundary regression above are source-ready but unrun; run5 does not verify that later delta.

Y reports first S1 captured at `/home/mu/.cache/rustr-integration/s1-20261026142204` before the regression request, then stopped at the locale stale gate without a host build. That snapshot stays immutable; a new candidate waits for the requested regression authorization and p24's source repair. No browser pass or full app-regression pass is claimed.

Authored app regressions: `confirmation_reads_the_embedded_result_once_and_never_restarts_checkout`, `uncertain_confirmation_has_no_countdown_and_old_route_answers_are_discarded`, `invoice_documents_and_answers_are_scoped_to_the_signed_in_owner`, `special_client_host_copy_is_preserved_and_cleared_on_account_switch`, `closing_and_reopening_the_same_invoice_rejects_old_preview_delivery_and_failure`. Before-fix and after-fix execution are unobserved by this owner. Y owns generation and integrated execution; p22 owns driver migrations, V owns rendering consumers.

The consolidated `upload_queue`, `transcript`, `job_list` revisions and their leaf tests stay with p1X; no transfer or duplicate rewrite.

The sign-out retry source repair is present: `AuthEvent::Failed` while `signing_out` clears that flag without ending the session; regression `a_failed_sign_out_can_be_retried` is authored, not executed by this owner. p22 reports adopting rereads for refused leaf writes, skipping session failures (no session, 401, erasure 410). That relay is not execution evidence for failed-write recovery.

## 5. REPAIR-P5-1 fixture coverage and typing

- `TranscriptVm.rtl`: completed Arabic job and original Arabic lines, including mixed Latin product/version text and Arabic speakers. `TranscriptState::Loaded` computes `rtl = true`; no view-model field is patched by a story.
- `TranscriptVm.long_strings`: completed English job with long title, long spaced and unbroken speaker names, transcript paragraphs over 1,000 characters and an unbroken token over 200 characters. Both variants use the existing `Loaded`/`MediaUrlLoaded` fixture path and preserve real speaker controls.
- The reported `fixtures.ts` conversion failure was JSON-import inference, not an invalid tuple wire format: `number[][]` cannot prove `[number, number][]`, and inferred strings lose union discriminants. The generator now emits contextually typed `fixtures.generated.ts`; p22 owns the loader cutover, without weakening generated types or changing serde.
- Both new fixture regressions failed for missing keys before implementation. The publication-boundary regression also failed before the typed companion existed. After implementation, `cargo test --locked -p transgate-core --features ts --lib presentation::fixtures` passed all 8 tests and regenerated the working-source artifacts.
- p22 migrated the owned fixture accessor to import the generated data/type map, removing the raw JSON assertion. p22 reports runtime smoke passing for all 21 roots / 170 payloads, `SessionProfile` access and missing-name failure; this is not a Vitest-suite claim.
- The final serialized source `npm run build` by p1V passed after Y published source wasm glue: Svelte-check 1,414 files, zero errors/warnings; Vite transformed 954 modules and built in 2.26 s. Durable receipt: `web-shell.md`, “Final build update”. This supersedes the earlier missing-wasm/TextField failures and verifies the generated fixture consumers compile without weakened types.
- p1Z received the published `rtl` and `long_strings` keys and owns desktop/phone component cases and stories. No Vitest component files were edited by p1X. Candidate3 is untouched; no Storybook/browser/C1 certification is implied.
