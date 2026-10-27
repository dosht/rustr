# Platform services

Transgate uses Resend email, several schedulers, local feature flags, PostHog analytics and Sentry error reporting.
The frontend declares two Railway replicas; public free tools require a separate FFmpeg processor and temporary-file lifecycle.
This inventory describes checked-in behavior, not verified live deployment settings or successful external delivery.

## Inventory

Paths are relative to Transgate. No production calls, secrets, builds or tests were used. The earlier production snapshot is `transgate/docs/production-facts.md`; it explicitly could not inspect pg_cron registration.

### Outgoing email inventory

All application messages use `sendEmail(to, subject, html)`, lazy Resend initialization, sender `Transgate <notifications@transgate.ai>`, and error propagation from Resend. There is no locale argument or text alternative in that API. Source: `frontend/src/emails/mailer.ts:1-29`. Templates use React-email and shared chrome, with homepage/contact links and optional Get Started CTA: `frontend/src/emails/Email/Email.tsx:10-13,38-73`.

| Message | Trigger and recipients | Payload/template | Source |
|---|---|---|---|
| Welcome to the Transgate Family! | Valid authenticated public.users INSERT webhook; inserted user's address | Name or `there`, English welcome, homepage/contact links | `frontend/src/pages/api/hooks/user-inserted.ts:29-68`; `frontend/src/emails/welcome.tsx:10-25` |
| Woohoo! Your Transgate purchase is complete! | Payment webhook with old pending and new completed; payer, skipped if no address | Name and purchase confirmation template | `frontend/src/pages/api/hooks/payment-updated.ts:47-74` |
| Payment Completed | Same payment transition; sales mailbox | Payment record, user id/address, sales template | `frontend/src/pages/api/hooks/payment-updated.ts:77-82` |
| Failed File Detected | Job webhook whose current status is failed; user, skipped if no address | Name and failed-job template | `frontend/src/pages/api/hooks/job-updated.ts:38-46,77-93` |
| Job Failed | Same failed job; support mailbox | Job id/error/user/language, admin failed-job template | `frontend/src/pages/api/hooks/job-updated.ts:96-104` |
| Job Partial Errors | Completed job with non-null error; support only | Same admin template | `frontend/src/pages/api/hooks/job-updated.ts:47-50,81-82,107-115` |
| SUPPORT: user address, jobId | POST support; support mailbox | Submitted message HTML and job context | `frontend/src/pages/api/support.ts:14-31` |
| We've received your message! | After support mail, signed-in user's address | SupportAutoReplyEmail with profile name or `there` | `frontend/src/pages/api/support.ts:27-31` |
| Contact subject prefixed by submitted email | POST contact-us; contact-us mailbox | Submitted message HTML | `frontend/src/pages/api/contact-us.ts:12-19` |
| We've received your message! | After contact-us mail, submitted address | SupportAutoReplyEmail with `there` | `frontend/src/pages/api/contact-us.ts:17-19` |
| New Newsletter Subscriber | POST subscribe; newsletter mailbox | Submitted address in HTML, not a confirmed subscription campaign | `frontend/src/pages/api/subscribe.ts:10-18` |
| !!!CANCEL SUBSCRIPTION!!! | After cancellation updates payment timestamp; support mailbox | Payment id | `frontend/src/pages/api/cancel-subscription.ts:17-29` |
| Iyzico callback | Subscription callback diagnostic to maintainer mailbox | Callback message in preformatted HTML | `frontend/src/pages/api/subscription-callback.ts:143-147` |
| ALERT: Provider NAME marked UNHEALTHY | Cached healthy-to-unhealthy transition; ops mailbox | Provider, rates, consecutive/server failures, total jobs and last-failure time; nonblocking send | `frontend/src/services/unified-transcription/services/health-monitor.ts:47-68,219-249` |

The two outreach templates `trans_agancies.email.tsx` and `translators.email.tsx` are content assets, not evidence of scheduled sends. Auth verification/recovery emails are an identity-provider responsibility; their live SMTP/template configuration was not inspected. Do not invent a new successful-transcription email from the failure hook. Sources: `frontend/src/emails/trans_agancies.email.tsx:1`; `frontend/src/emails/translators.email.tsx:1`; `frontend/src/pages/signin.tsx:151-158`.

### Schedules and asynchronous platform work

| Work | Cadence and execution | Boundary | Source |
|---|---|---|---|
| Deepgram queue/recovery | Every three minutes in production Node runtime | Recover jobs with 30-minute threshold, then process pending/not_started; scheduler guard is process-local | `frontend/src/instrumentation.ts:1-5`; `frontend/src/scheduler/index.ts:9-45,58-61` |
| Soniox recovery | Every three minutes in production Node runtime | Recover at 30 minutes, cleanup in finally | `frontend/src/scheduler/index.ts:47-66` |
| HTTP cron entry points | Deepgram and Soniox endpoints require Bearer CRON_SECRET | Same recovery work; endpoints are callable independently of node-cron | `frontend/src/pages/api/cron/process-deepgram-queue.ts:7-57`; `frontend/src/pages/api/cron/process-soniox-recovery.ts:7-35` |
| Legacy Vercel schedule | Deepgram HTTP endpoint every three minutes | Declaration remains; current external execution unknown | `frontend/vercel.json:13-18` |
| Transcript cleanup | Daily 02:00 UTC, cleanup_old_transcripts(30) | Privacy topic owns selection/opt-out rules | `frontend/supabase/migrations/20251221200006_schedule_cleanup_jobs.sql:11-15` |
| Provider-health metrics cleanup | Daily 03:00 UTC, cleanup_old_health_metrics(30) | Housekeeping for current health subsystem | `frontend/supabase/migrations/20251221200006_schedule_cleanup_jobs.sql:23-27` |
| Cron history cleanup | Sunday 04:00 UTC, cleanup_cron_job_history(90) | pg_cron execution history retention | `frontend/supabase/migrations/20251221200006_schedule_cleanup_jobs.sql:35-39` |
| Provider alert condition monitor | One-minute timer per instantiated HealthMonitor | Actual health checks are event-triggered; old 30-second health-check field is unused | `frontend/src/services/unified-transcription/services/health-monitor.ts:86-98,160-167` |
| Free-tools cleanup | Every ten minutes in processor | Purge expired sessions and orphan files in all seven families | `frontend/services/free-tools-processor/src/index.ts:102-135` |
| GCS upload completion | Eventarc storage object finalized, not a clock schedule | Duration worker; job logic owned by backend discovery | `Infra/gcf-process-uploaded-file/terraform/function.tf:59-68`; `Infra/gcf-process-uploaded-file/src-v2/index.ts:20-35` |

Processor TTL is one hour for trim-audio, convert-audio and convert-video; two hours for reduce-audio, remove-noise, video-to-audio and compress-video. Session expiry compares createdAt; orphan expiry compares file mtime. Sources: `frontend/services/free-tools-processor/src/routes/trim-audio.ts:54,429-458`; `frontend/services/free-tools-processor/src/routes/convert-audio.ts:60,274-303`; `frontend/services/free-tools-processor/src/routes/convert-video.ts:76,168-197`; `frontend/services/free-tools-processor/src/routes/reduce-audio.ts:76,357-386`; `frontend/services/free-tools-processor/src/routes/remove-noise.ts:63,262-291`; `frontend/services/free-tools-processor/src/routes/video-to-audio.ts:79,347-376`; `frontend/services/free-tools-processor/src/routes/compress-video.ts:83,528-557`. API and privacy topics own request limits and retained content policy; CRN here records when cleanup runs.

### Flags and effective scope

Source for all local product flags: `frontend/src/utils/featureFlags.ts:18-117`. A global true wins immediately; otherwise email, user id or locale match is sufficient. Browser hook requires a loaded signed-in user first; utility check does not. Thus a globally enabled flag is not itself authorization. Server customer-invoice lookup separately authenticates and evaluates the same flag: `frontend/src/pages/api/admin/customer-invoices.ts:23-25`.

| Flags | Current configuration | Consumer/applicability |
|---|---|---|
| AI_FEATURES | true | Transcript AI visibility; server AI config and consent remain separate |
| TRANSCRIPTION_SOURCE_MICROPHONE | false, no exceptions | Workspace source hidden; independent public recorder still exists |
| TRANSCRIPTION_SOURCE_YOUTUBE | true | Signed-in YouTube add/resolve |
| TRANSCRIPTION_SOURCE_GOOGLE_DRIVE, TRANSCRIPTION_SOURCE_ONEDRIVE | false, no exceptions | No shipped enabled cloud-drive source |
| SHOW_ORIGINAL_TRANSCRIPT | false, one email exception | Original/translation tab visibility |
| MOBILE_UX_JOBS | true | Touch-phone list/detail/navigation |
| MOBILE_EXPANDED_AUDIO_PLAYER | false, two email exceptions | Expanded mobile player; `frontend/src/features/transcript-header/views/mobile/MobileAudioPlayer.tsx:72` |
| MOBILE_UX_AI | true | Phone AI layout |
| VOICE_DEMO | true | Public landing uses utility without user, so available to guests; `frontend/src/components/LandingPage/VoiceDemo/VoiceDemoSection.tsx:27-29` |
| BILLING_HISTORY | true | Invoice history page |
| BILLING_CUSTOMER_LOOKUP | false, two email exceptions | Privileged invoice lookup, enforced by server too |
| DASHBOARD_API_KEYS | true | Enabled dashboard capability |
| DASHBOARD_AUDIT_LOGS/BILLING/JOBS/ANALYTICS/SETTINGS/PROFILE | all false | ComingSoon siblings, not working settings or billing screens |
| API_RATE_LIMITING/AUDIT_LOGGING/WEBHOOKS/KEY_EXPIRATION | all true in legacy config | Configuration declarations; backend owns actual enforcement call sites |
| Legacy AI_FEATURES_ENABLED | false | Separate compatibility constant, not current AI_FEATURES truth |

Legacy dashboard/API values: `frontend/src/utils/featureFlags.ts:229-248`. Other runtime switches: ENABLE_AI_CHAT must equal `true`, AI_CHAT_VALIDATION_ENABLED defaults on unless `false` (`frontend/src/lib/ai/chat-config.ts:43-45`); CLIENT_WASM_ENABLED is false (`frontend/src/utils/free-tools/clientWasmEnabled.ts:5`); YouTube related-tool links are locally off (`frontend/src/pages/tools/youtube-transcript-generator.tsx:51-53`). Business values for chat limits remain AI-owned.

### Analytics, errors and logging

| Concern | Current behavior | Source |
|---|---|---|
| Browser PostHog | Requires public key/host; initial `id` locale skips initialization. Managed proxy, identified-only profiles, manual pageviews, pageleave/autocapture, default opt-out | `frontend/src/components/PostHogProvider/PostHogProvider.tsx:15-62,70-83` |
| Consent | Accepted/rejected cookie lasts 365 days; accepted enables PostHog and GA analytics_storage; rejected opts PostHog out | `frontend/src/components/CookieConsent/CookieConsent.tsx:15-49` |
| Browser event families | user_identified, user_logged_out, user_properties_updated; transcription_started/completed/failed/downloaded; checkout_started, subscription_cancelled; ai_chat_message_sent/rated/copied/retried | `frontend/src/utils/analytics/types.ts:29,61-65,108-110,178-182` |
| Additional events | `$pageview`, cookie_consent_accepted; error_occurred with scrubbed Sentry details; pageleave/autocapture SDK events | `frontend/src/components/PostHogProvider/PostHogProvider.tsx:38-40,99-104`; `frontend/sentry.client.config.ts:173-180` |
| Server events | user_signed_up, payment_completed, payment_failed, subscription_started. Stable UUID v5 per source record/event; signup sends internal id only. Subscription start only initial completed plan payment | `frontend/src/utils/analytics/server.ts:6,81-123` |
| Payment attribution | Server payment metadata supplies whitelisted UTM/referrer/device/browser fields for initial or renewal origin. No card details. Client checkout uses corresponding fields | `frontend/src/utils/analytics/server.ts:49-69`; `frontend/src/utils/analytics/events/payment.ts:64-80` |
| Attribution storage | First touch in transgate_utm_data localStorage, no overwrite if already set; storage failures ignored | `frontend/src/utils/analytics/utmCapture.ts:22,35-78` |
| Best effort | Browser wrapper checks initialized and not opted out, catches SDK errors. Server ingestion is nonblocking with 3-second timeout, zero retries, sanitized diagnostic failures and no GeoIP | `frontend/src/utils/analytics/client.ts:77-99,139-156`; `frontend/src/utils/analytics/server.ts:14-42,81-123` |
| Session correlation | Browser optionally sends X-PostHog-Session-ID; server Sentry extracts it for replay links/tags | `frontend/src/utils/analytics/sessionHeaders.ts:26-45`; `frontend/sentry.server.config.ts:91-145` |
| Browser Sentry | DSN required; production beforeSend requires accepted consent, development bypasses; sensitive string scrubbing and top-level transcript content removal; trace 0.1, replay error 1.0, replay session 0.1, text/media masked | `frontend/sentry.client.config.ts:51-117,136-224` |
| Server and edge Sentry | Server DSN-gated, trace 0.1 and scrubEvent; edge config separately initializes with fixed DSN and trace 1.0, without the same hooks | `frontend/sentry.server.config.ts:124-165`; `frontend/sentry.edge.config.ts:8-15` |
| Other telemetry | GA and Hotjar are mounted globally alongside PostHog. Source presence does not establish consent equivalence | `frontend/src/pages/_app.tsx:52-58` |
| Structured logs | Request/cron wrappers and job-scoped logs surround platform operations; provider/processor errors have separate loggers | `frontend/src/pages/api/cron/process-deepgram-queue.ts:1-4,61`; `frontend/src/pages/api/hooks/job-updated.ts:26-29`; `frontend/services/free-tools-processor/src/index.ts:60-76` |

### Deployment and dependency boundary

- Frontend Railway config: Nixpacks, yarn build, two replicas, failure restart maximum three, `/api/health`, 300-second health timeout, Next start with 1536 MiB old-space cap. Source: `frontend/railway.json:3-14`. Health is a GET memory/uptime response; over 95 percent heap utilization returns 503, degraded still 200, not a database/provider readiness probe: `frontend/src/pages/api/health.ts:31-78`.
- Processor: Node 20 image with system FFmpeg, port 8080. Compose defines Caddy TLS edge, processor resource limits 1.5 CPU/2 GiB, persistent disk `/tmp`, 30-second stop grace and health polling. Caddy removes request-body timeout and allows 30-minute upstream response headers. This checked-in deployment is not proof of its live host. Sources: `frontend/services/free-tools-processor/Dockerfile:1-23`; `frontend/services/free-tools-processor/deploy/docker-compose.yml:3-50`; `frontend/services/free-tools-processor/deploy/Caddyfile:4-27`.
- Processor sessions and rate limits are in-memory; disk payloads alone do not restore sessions. Do not deploy it like the two-replica frontend without a session-affinity/storage decision. Sources: `frontend/services/free-tools-processor/src/lib/sessionStore.ts:20-69`; `frontend/services/free-tools-processor/deploy/docker-compose.yml:32-37`.
- Core source dependencies include Supabase auth/database, GCS upload events, provider callbacks, Iyzico, Resend, PostHog and Sentry. Domain specifics are in billing/jobs/API/privacy discovery. GCS and Cloud Function declarations are retained as migration inputs, not a requirement to keep the old cloud topology: `Infra/infrastructure/terraform/main.tf:15-18,46-54`; `Infra/gcf-process-uploaded-file/terraform/function.tf:59-68`. The GKE cluster resource file is commented out: `Infra/infrastructure/terraform/cluster.tf:13-31`.
- Configuration names, never values: RESEND_API_KEY, CRON_SECRET, NODE_ENV/NEXT_RUNTIME, public PostHog key/host/project id, server POSTHOG_API_KEY/POSTHOG_HOST, NEXT_PUBLIC_SENTRY_DSN; processor PORT/ALLOWED_ORIGINS/REQUEST_TIMEOUT_MS/FREE_TOOLS_TRUSTED_PROXY_COUNT/FFMPEG_THREADS. Frontend chooses its processor through NEXT_PUBLIC_FREE_TOOLS_PROCESSOR_URL, default localhost:8080 (`frontend/src/utils/free-tools/processorUrl.ts:7-8`). Sources for other settings are the mailer, cron, telemetry and deployment references above. Provider/storage/payment secrets belong to their adapter inventories.

## Rules

Rules below own delivery, cadence and flag semantics only. Billing, identity/privacy, job transitions, API/RTL and AI outcomes remain in their owning topics.

| ID | Rule | Source | pgTAP | Rust test | Status |
|---|---|---|---|---|---|
| EML-001 | Application emails use the common Resend sender and HTML payload; delivery errors propagate | `frontend/src/emails/mailer.ts:13-29` | none | cargo:billing-postgres:mail_that_is_not_configured_is_an_error_not_a_silent_acknowledgement; cargo:transgate-billing:mail_delivery_is_required_to_start_and_a_partial_group_names_what_is_missing | tested |
| EML-002 | Welcome requires authenticated valid public.users INSERT with nonempty id/email and null old_record | `frontend/src/pages/api/hooks/user-inserted.ts:29-68` | none | tbd | ledger |
| EML-003 | Purchase email and sales notification require payment pending to completed; missing payer email skips only user mail | `frontend/src/pages/api/hooks/payment-updated.ts:47-82` | none | cargo:billing-core:notify::tests::eml_bil_001_002_completion_sends_the_buyer_and_the_sales_email_with_the_documented_subjects; cargo:billing-postgres:eml_bil_001_002_003_committed_events_are_delivered_to_buyer_sales_and_support_with_stable_keys | tested |
| EML-004 | Current failed job status sends user and support failure messages; no previous-status transition check | `frontend/src/pages/api/hooks/job-updated.ts:38-46,77-104` | none | cargo:transgate-server:workers::pg_tests::support::eml_004_a_failed_job_mails_the_user_then_support_and_a_retry_never_repeats_a_sent_mail | tested |
| EML-005 | Completed job with non-null error sends support partial-error notification only | `frontend/src/pages/api/hooks/job-updated.ts:47-50,81-82,107-115` | none | cargo:transgate-server:workers::pg_tests::support::eml_005_a_completion_with_an_error_mails_support_only_and_a_clean_completion_mails_nobody | tested |
| EML-006 | Support POST sends support message first, then personalized acknowledgement to user | `frontend/src/pages/api/support.ts:14-37` | none | cargo:transgate-core:account::tests::support_mails_go_to_support_first_then_acknowledge_the_owner; cargo:transgate-server:workers::pg_tests::support::support_failure_preserves_order_and_only_both_successful_mails_ack_the_durable_request | tested |
| EML-007 | Contact POST sends contact mailbox message first, then acknowledgement to supplied email | `frontend/src/pages/api/contact-us.ts:12-25` | none | tbd | ledger |
| EML-008 | Newsletter POST sends an operator notification containing submitted address | `frontend/src/pages/api/subscribe.ts:10-21` | none | tbd | ledger |
| EML-009 | Cancellation sends support the payment id after updating the payment cancellation timestamp | `frontend/src/pages/api/cancel-subscription.ts:17-32` | none | cargo:billing-core:notify::tests::eml_bil_003_a_cancellation_request_goes_to_support_with_the_payment_id_as_the_body; cargo:billing-postgres:bil_062_only_the_owner_can_request_cancellation_which_keeps_the_minutes_and_queues_the_support_mail | tested |
| EML-010 | Subscription callback sends a diagnostic email; mail failure is caught and logged | `frontend/src/pages/api/subscription-callback.ts:143-147` | none | tbd | ledger |
| EML-011 | Provider unhealthy notification is sent on cached healthy-to-unhealthy transition, nonblocking; error is logged | `frontend/src/services/unified-transcription/services/health-monitor.ts:47-68,219-249` | none | tbd | ledger |
| EML-012 | Shared email chrome defaults to Get Started CTA and homepage email UTM links; templates receive name but no locale | `frontend/src/emails/Email/Email.tsx:10-13,60-65`; `frontend/src/emails/welcome.tsx:6-10` | none | tbd | ledger |
| CRN-001 | Node scheduler starts only in production Node runtime and suppresses duplicate start only within the process | `frontend/src/instrumentation.ts:1-5`; `frontend/src/scheduler/index.ts:9-23` | none | tbd | ledger |
| CRN-002 | Deepgram queue processing and 30-minute stuck recovery run every three minutes | `frontend/src/scheduler/index.ts:26-44,58-61` | none | tbd | ledger |
| CRN-003 | Soniox 30-minute stuck recovery runs every three minutes | `frontend/src/scheduler/index.ts:47-66` | none | tbd | ledger |
| CRN-004 | HTTP recovery endpoints compare Authorization to Bearer CRON_SECRET and return unauthorized on mismatch | `frontend/src/pages/api/cron/process-deepgram-queue.ts:8-13`; `frontend/src/pages/api/cron/process-soniox-recovery.ts:8-12` | none | cargo:transgate-http:tests::cron_accepts_only_the_exact_bearer_secret_and_refuses_everything_when_none_is_set | tested |
| CRN-005 | Daily 02:00 UTC transcript cleanup calls cleanup_old_transcripts with 30 days | `frontend/supabase/migrations/20251221200006_schedule_cleanup_jobs.sql:11-15` | none | cargo:transgate-server:workers::pg_tests::schedule::prv_029_crn_005_the_daily_0200_sweep_runs_once_per_slot_and_resumes_after_a_restart | tested |
| CRN-006 | Daily 03:00 UTC provider-health cleanup keeps 30 days | `frontend/supabase/migrations/20251221200006_schedule_cleanup_jobs.sql:23-27` | none | tbd | ledger |
| CRN-007 | Sunday 04:00 UTC cron-history cleanup keeps 90 days | `frontend/supabase/migrations/20251221200006_schedule_cleanup_jobs.sql:35-39` | none | cargo:transgate-server:workers::pg_tests::schedule::crn_007_the_sunday_0400_sweep_keeps_ninety_days_of_scheduler_history | tested |
| CRN-008 | Free-tools processor calls all seven session/orphan cleanup pairs every ten minutes | `frontend/services/free-tools-processor/src/index.ts:102-135` | none | cargo:transgate-processor:acceptance::cleanup_timer_waits_ten_minutes_then_sweeps_all_seven_orphan_prefixes | tested |
| CRN-009 | Processor session/orphan cleanup uses per-family one-hour or two-hour TTL, based on createdAt or mtime respectively | `frontend/services/free-tools-processor/src/routes/trim-audio.ts:54,429-458`; `frontend/services/free-tools-processor/src/routes/compress-video.ts:83,528-557`; remaining family references in Inventory | none | cargo:transgate-processor:acceptance::every_family_purges_only_past_its_session_and_orphan_ttl; cargo:transgate-processor:cleanup_purges_sessions_past_their_ttl_and_orphans_by_age_and_prefix | tested |
| CRN-010 | HealthMonitor checks alert conditions every minute; provider health itself is event-triggered | `frontend/src/services/unified-transcription/services/health-monitor.ts:86-98,160-167` | none | tbd | ledger |
| FLG-001 | Global true enables immediately; otherwise any matching email, user id or locale enables the feature | `frontend/src/utils/featureFlags.ts:188-222` | none | tbd | ledger |
| FLG-002 | Browser flag hook stays false without a user, waits for user loading and fails closed on evaluation error; utility has no user gate | `frontend/src/utils/featureFlags.ts:122-180,188-222` | none | tbd | ledger |
| FLG-003 | AI, YouTube source, mobile jobs, mobile AI, voice demo and billing history are globally on | `frontend/src/utils/featureFlags.ts:19-20,46-47,65-66,85-96,105-106` | none | tbd | ledger |
| FLG-004 | Microphone, Google Drive and OneDrive workspace source flags are off without exceptions | `frontend/src/utils/featureFlags.ts:43-54` | none | tbd | ledger |
| FLG-005 | Original transcript, expanded phone player and customer invoice lookup are off except configured allowlists | `frontend/src/utils/featureFlags.ts:55-64,75-84,108-116` | none | tbd | ledger |
| FLG-006 | API-key dashboard is enabled; six other dashboard capabilities are disabled; legacy API switches are true | `frontend/src/utils/featureFlags.ts:234-248` | none | tbd | ledger |
| FLG-007 | Customer invoice lookup uses server-side flag authorization as well as UI visibility | `frontend/src/pages/api/admin/customer-invoices.ts:23-25`; `frontend/src/pages/billing/index.tsx:358-359` | none | tbd | ledger |
| FLG-008 | Free-tools browser WASM is disabled, making server processing the current default | `frontend/src/utils/free-tools/clientWasmEnabled.ts:5`; `frontend/src/components/TrimAudio/TrimAudioTool.tsx:154-158,243-246` | none | tbd | ledger |
| FLG-009 | AI chat is enabled only by exact true; chat validation is enabled unless exact false | `frontend/src/lib/ai/chat-config.ts:43-45` | none | tbd | ledger |

## Defects

| ID | Current behavior | Intended behavior | Proposal |
|---|---|---|---|
| OPS-D01 | Two frontend replicas can each start local cron, with a legacy external cron declaration too; stopScheduler only resets a boolean, not timer handles (`frontend/railway.json:8`; `frontend/src/scheduler/index.ts:16-23,58-74`; `frontend/vercel.json:13-18`) | One logical scheduler execution with safe replica coordination | Use agreed Scheduler/Lock ports; backend owns job-level idempotency. This is a duplicate-invocation risk, not proof of duplicate completed jobs |
| OPS-D02 | Job failure hook checks current failed state, not transition; mailer passes no idempotency key (`frontend/src/pages/api/hooks/job-updated.ts:77-79`; `frontend/src/emails/mailer.ts:16-21`) | Avoid duplicate notifications on webhook replay | Mu approves durable delivery identity/outbox semantics, retaining subjects and recipients |
| OPS-D03 | Initial Indonesian locale skips PostHog init, but effect has no locale dependency; UTM storage is captured on SDK load outside accepted-consent branch (`frontend/src/components/PostHogProvider/PostHogProvider.tsx:28-68`) | Consistent explicit tracking policy | Privacy owner and mu decide locale/consent policy; do not silently call current behavior compliant |
| OPS-D04 | Browser and server Sentry use scrubbing/0.1 trace sample, edge uses separate fixed configuration/1.0 (`frontend/sentry.client.config.ts:136-224`; `frontend/sentry.server.config.ts:124-165`; `frontend/sentry.edge.config.ts:8-15`) | Deliberate consistent privacy and sampling policy | Keep evidence; reconcile adapter config before deployment |
| OPS-D05 | Source next.config calls withSentryConfig without assigning its return; condition uses OR between not-build and not-dev and is always true (`frontend/next.config.js:65-67`) | Build instrumentation follows intended lifecycle | Record source defect; actual build impact not exercised here |
| OPS-D06 | Processor stores sessions in memory while files persist on disk (`frontend/services/free-tools-processor/src/lib/sessionStore.ts:20-47`; `frontend/services/free-tools-processor/deploy/docker-compose.yml:32-37`) | Deployment must preserve the intended download/session lifecycle | Keep single-instance constraint or explicitly change storage; do not assume a stateless two-replica image |

## Keep / throw

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Resend message subjects, recipients and content | `frontend/src/emails/mailer.ts:13-29`; email inventory | port | Same transactional messages, no React-email build dependency | Mailer port and Rust templates |
| Outreach templates | `frontend/src/emails/trans_agancies.email.tsx:1`; `frontend/src/emails/translators.email.tsx:1` | keep-knowledge | Content assets, no discovered automated campaign | Content archive, not new scheduler |
| Provider-health scoring and alert machinery | `frontend/src/services/unified-transcription/services/health-monitor.ts:79-98` | simplify | PRD drops health scoring; retain operational failure-notification requirement explicitly | Simple provider-error telemetry/ops decision |
| Multiple scheduler entry points | `frontend/src/scheduler/index.ts:58-66`; `frontend/vercel.json:13-18` | simplify | Preserve work/cadence, remove competing orchestrators | One Scheduler plus Lock port |
| Database housekeeping | `frontend/supabase/migrations/20251221200006_schedule_cleanup_jobs.sql:11-38` | keep-knowledge | Retention and execution-history cleanup must not disappear | Scheduler/housekeeping adapter; privacy use cases |
| Free-tools runtime and cleanup | `frontend/services/free-tools-processor/src/index.ts:34-50,102-135` | port | Seven live processing families cannot be replaced by CMS HTML | Runtime processor adapter |
| Processor proxy/timeouts and disk limits | `frontend/services/free-tools-processor/deploy/Caddyfile:4-27`; `frontend/services/free-tools-processor/deploy/docker-compose.yml:32-50` | keep-knowledge | Upload duration and temporary storage are real deployment constraints | Docker deployment configuration |
| Local flags and legacy dashboards | `frontend/src/utils/featureFlags.ts:18-117,229-248` | simplify | Keep effective entitlements and off states, remove duplicate flag systems and fake dashboard pages | Flag port plus explicit presentation capabilities |
| PostHog and Sentry | `frontend/src/utils/analytics/server.ts:81-123`; `frontend/sentry.client.config.ts:136-224` | port | Keep event contracts, nonblocking failure behavior and privacy decisions | Analytics and ErrorReporter ports |
| GA, Hotjar and mixed instrumentation | `frontend/src/pages/_app.tsx:52-58` | keep-knowledge | PRD names PostHog/Sentry; extra trackers require an explicit retain/drop decision | Needs mu |
| Old cloud deployment topology | `Infra/gcf-process-uploaded-file/terraform/function.tf:59-68`; `Infra/infrastructure/terraform/cluster.tf:13-31` | simplify | Keep upload-event behavior, not dormant Kubernetes/Vercel topology | Rust workers and storage port |

## Parity rows

Email and cron surfaces are shared backend obligations across every shell. Native/CLI/TUI do not send their own duplicate transactional emails or run recovery schedules. Product flag semantics apply wherever the corresponding use case exists; web-only layout flags do not become native business capabilities. Browser pageviews/replay/autocapture do not imply native or terminal session replay.

| ID | Flow | Surface | Today | Test | Status |
|---|---|---|---|---|---|
| EML-P001 | Welcome after signup | email | Authenticated validated users INSERT sends English welcome; EML-002 | tbd | inventoried |
| EML-P002 | Completed purchase confirmation | email | Payer notification for pending-to-completed payment; EML-003 | tbd | inventoried |
| EML-P003 | Completed purchase sales notification | email | Sales mailbox receives payment/user details; EML-003 | tbd | inventoried |
| EML-P004 | Failed job user notification | email | Current failed job sends named failure template if address exists; EML-004 | tbd | inventoried |
| EML-P005 | Failed job operator notification | email | Support gets job/error/user/language; EML-004 | tbd | inventoried |
| EML-P006 | Partial job errors | email | Completed job with error sends support only; EML-005 | tbd | inventoried |
| EML-P007 | Support request delivery | email | Submitted message and job context sent to support; EML-006 | tbd | inventoried |
| EML-P008 | Support acknowledgement | email | Personalized reply after support send; EML-006 | tbd | inventoried |
| EML-P009 | Public contact delivery | email | Submitted message sent to contact-us; EML-007 | tbd | inventoried |
| EML-P010 | Public contact acknowledgement | email | Reply to supplied address after contact send; EML-007 | tbd | inventoried |
| EML-P011 | Newsletter interest notification | email | Operator receives supplied address, no campaign inferred; EML-008 | tbd | inventoried |
| EML-P012 | Cancellation support notification | email | Payment id after timestamp update; EML-009 | tbd | inventoried |
| EML-P013 | Subscription callback diagnostic | email | Maintainer notification, send failure swallowed after logging; EML-010 | tbd | inventoried |
| EML-P014 | Provider unhealthy operations alert | email | Cached healthy-to-unhealthy transition; scoring removal decision required; EML-011 | tbd | inventoried |
| CRN-P001 | Recover/process Deepgram queue | cron | Three-minute production scheduler, 30-minute stuck threshold; CRN-001/002 | tbd | inventoried |
| CRN-P002 | Recover Soniox jobs | cron | Three-minute production scheduler, 30-minute stuck threshold; CRN-001/003 | tbd | inventoried |
| CRN-P003 | Authorized HTTP Deepgram recovery | api | Independent HTTP entry, Bearer comparison; CRN-004 | tbd | inventoried |
| CRN-P004 | Authorized HTTP Soniox recovery | api | Independent HTTP entry, Bearer comparison; CRN-004 | tbd | inventoried |
| CRN-P005 | Daily transcript cleanup | cron | 02:00 UTC, 30-day argument; privacy owns content selection; CRN-005 | tbd | inventoried |
| CRN-P006 | Daily health-metrics cleanup | cron | 03:00 UTC, 30 days; tied to scoring-retirement decision; CRN-006 | tbd | inventoried |
| CRN-P007 | Weekly execution-history cleanup | cron | Sunday 04:00 UTC, 90 days; CRN-007 | tbd | inventoried |
| CRN-P008 | Expired free-tool sessions | cron | Seven processor families every ten minutes, one/two-hour TTL; CRN-008/009 | cargo:transgate-processor:acceptance::every_family_purges_only_past_its_session_and_orphan_ttl; cargo:transgate-processor:acceptance::cleanup_timer_waits_ten_minutes_then_sweeps_all_seven_orphan_prefixes | tested |
| CRN-P009 | Orphan processor files after restart | cron | Same sweep, file mtime expiry even without in-memory session; CRN-008/009 | cargo:transgate-processor:the_deployable_binary_loses_sessions_on_restart_keeps_their_files_and_stops_on_sigterm; cargo:transgate-processor:acceptance::every_family_purges_only_past_its_session_and_orphan_ttl; cargo:transgate-processor:acceptance::cleanup_timer_waits_ten_minutes_then_sweeps_all_seven_orphan_prefixes | tested |
| CRN-P010 | Provider alert-condition timer | cron | One-minute timer, health data itself event-driven; CRN-010 | tbd | inventoried |
| OPS-P001 | Effective feature visibility | web-desktop | User-gated hook versus guest utility and server authorization; FLG-001 to FLG-009 | tbd | inventoried |
| OPS-P002 | Effective phone feature visibility | web-phone | Mobile layout flags plus shared capability rules; FLG-001 to FLG-009 | tbd | inventoried |
| OPS-P003 | Consent and browser telemetry | web-desktop | Accepted/rejected cookie, default opt-out, events, Sentry correlation; `frontend/src/components/PostHogProvider/PostHogProvider.tsx:15-62,99-114` | tbd | inventoried |
| OPS-P004 | Consent and phone-browser telemetry | web-phone | Same browser policy and schema, not a second analytics implementation; `frontend/src/components/CookieConsent/CookieConsent.tsx:15-49` | tbd | inventoried |
| OPS-P005 | Server signup/payment analytics | api | Stable event ids, no payment failure due to analytics failure; `frontend/src/utils/analytics/server.ts:81-123` | tbd | inventoried |
| OPS-P006 | Application health response | api | GET memory/uptime; unhealthy 503, otherwise 200; `frontend/src/pages/api/health.ts:31-78` | tbd | inventoried |
| OPS-P007 | Native feature/telemetry applicability | android | No current native shell; carry shared capability/event meaning, not browser replay or layout flags; Inventory flag/analytics contracts | tbd | inventoried |
| OPS-P008 | Native feature/telemetry applicability | ios | No current native shell; carry shared capability/event meaning, not browser replay or layout flags; Inventory flag/analytics contracts | tbd | inventoried |
| OPS-P009 | Terminal feature/telemetry applicability | cli | No current terminal shell; server outcome events shared, local consent/telemetry decision explicit; Inventory flag/analytics contracts | tbd | inventoried |
| OPS-P010 | Interactive terminal applicability | tui | No current terminal shell; same capability rules and server outcomes, no browser replay; Inventory flag/analytics contracts | tbd | inventoried |

## Research

R4 runtime dependency is fully enumerated in static-pages.md: 39 processor family endpoints plus health, active server-first configuration, and local/browser-only capture distinguished from paid jobs. Backend worker owns API/RTL contracts. Production pg_cron and deployment values remain unverified; the prior production facts only establish that Railway has staging alongside production (`transgate/docs/production-facts.md:41-46`). No additional research question is owned by this topic.

## Needs mu

- Confirm actual Railway services, processor host, externally registered cron and pg_cron jobs at cutover. Assumption: preserve declared work and use local/fake adapters; do not delete a scheduler based only on absent production access.
- Confirm auth-provider SMTP/templates, Resend sender/domain readiness and operator recipients. Assumption: keep inspected English transactional content; no invented 16-locale email campaign.
- Resolve OPS-D01 to OPS-D06 and decide what provider-unhealthy notification becomes after health scoring is removed. Assumption: preserve operational alert intent, not the old scoring framework.
- Decide whether GA/Hotjar, Indonesian analytics exclusion, attribution storage and the browser/server consent distinction remain. Assumption: PostHog/Sentry are the target ports, with privacy policy resolved before real delivery.
- Confirm whether processor stays separate or joins the application image as a worker. Assumption: image simplification must retain FFmpeg, long uploads, temp-file cleanup and session affinity.

## Open questions for the design

- Give email delivery stable identity and an outbox without changing billing/job transition rules.
- Assign scheduler ownership and locks explicitly across web replicas and workers; timer cancellation and cleanup on failure need real shutdown semantics.
- Separate semantic telemetry events from browser-specific replay, and define allowed event properties centrally.
- Treat public-tool runtime, provider callbacks and storage events as shells over ports, not hidden exceptions to the static-site architecture.
