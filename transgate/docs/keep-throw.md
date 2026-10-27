# Keep / throw

What of Transgate is knowledge to keep, code to port, something to simplify, or something to throw. Assembled by `scripts/assemble-discovery.py` from `discovery/`; edit there. PRD section 11 asks for this before anything is ported.

Verdicts: `keep-knowledge` (a rule, lesson or ADR, no code), `port` (rewrite in Rust or Svelte with the same behavior), `simplify` (port with a named reduction), `throw` (dropped, with the reason), `gap` (the new system will not have it at first; mu sees it here).

## Summary

| Verdict | Rows |
|---|---|
| keep-knowledge | 30 |
| port | 74 |
| simplify | 39 |
| throw | 51 |
| gap | 12 |
| other | 1 |
| Total | 207 |

## ai

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Six analysis schemas and prompts | frontend/src/lib/ai/schemas.ts:24-301; frontend/src/lib/ai/prompt-templates.ts:93-146 | port | These are product output shapes, not interchangeable summaries | AI core schemas and prompt fixtures |
| DeepSeek/OpenAI HTTP shape | frontend/src/lib/ai/deepseek-provider.ts:46-126; frontend/src/lib/ai/openai-provider.ts:37-113 | simplify | Plain streaming adapter suffices; E4 must compare identical prompts/models | LLM port |
| Chat limits and paid bypass | frontend/src/lib/ai/ai-service-manager.ts:204-282 | port | Preserve commercial rule, fix atomicity separately | AI use case with quota port |
| Consent rules and revocation lesson | frontend/src/lib/ai/consent-service.ts:55-153 | keep-knowledge | Avoid partial deletion and post-revoke writes | Consent/deletion saga |
| Browser DB orchestration | frontend/src/lib/ai/analysis-service.ts:259-406 | simplify | Shared state machine should not make each shell its own persistence coordinator | Core generation state and server storage |
| Legacy summary/highlights storage | frontend/src/lib/ai/analysis-service.ts:224-229; frontend/src/lib/ai/consent-service.ts:131-147 | keep-knowledge | Needed for migration/cleanup, not a second live analysis engine | Migration mapping and privacy cleanup |
| Voice demo | frontend/src/pages/api/voice-demo/deepseek.ts:19-81 | throw | Lead confirms the PRD exclusion under mu's delegated authority: P4 D6, AI-027 and API-F44 remain inventoried | No port; docs/plans/p4-ai/decisions.md#d6-exclude-only-the-inventoried-voice-demo |

## api

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Opaque key, expiry and scope knowledge | frontend/supabase/migrations/20251130113249_remote_schema.sql:387-458,1211-1284 | keep-knowledge | Already own keys, despite JWT-shaped internal client plumbing | Identity/API core |
| Supabase JWT impersonation plumbing | frontend/src/services/unified-transcription/services/authentication.ts:82-95 | throw | New server resolves Actor directly; no database JWT needed for API auth | API-key adapter to Actor |
| Three transcription entry shapes | frontend/src/app/api/v1/transcriptions/route.ts:564-721; frontend/src/services/unified-transcription/middleware/backward-compatibility.ts:197-269 | simplify | One generated versioned contract; compatibility only by migration decision | Public API and optional boundary shim |
| Legacy v1-beta and unified transcription shapes | frontend/src/app/api/v1-beta/transcriptions/url/route.ts:1-14; frontend/src/app/api/v1-beta/transcriptions/[id]/route.ts; frontend/src/app/api/unified/transcriptions/route.ts | throw | mu approved 2026-10-27 (P7 D12 revised: there are no API users). API-F03, API-F04 and API-F05 are not served; neither are the shapes in rules API-011 to API-014. The public API ships only the current contract. The router mounts only the OpenAPI operation table. | none |
| Callback payload and delivery history | frontend/src/services/unified-transcription/services/callback-delivery.ts:83-175,272-317 | port | Customer callbacks are used in production | Callback port and delivery records |
| Generic key webhook placeholder handlers | frontend/src/app/api/webhooks/route.ts:53-90 | throw | Only log, do not deliver the promised lifecycle actions | Domain events only if actually consumed |
| SQL limiter implementation | frontend/supabase/migrations/20251130113249_remote_schema.sql:296-345 | simplify | Preserve configured limits, remove duplicate counters and races | Atomic shared limiter |
| Public monitoring and diagnostic endpoints | frontend/src/app/api/unified/monitoring/route.ts:11-14,100-103; frontend/src/pages/api/test-ffmpeg.ts:18-20 | simplify | Operational concerns, not product contract | Authenticated operations surface plus health |
| RunPod-specific API options | frontend/src/services/unified-transcription/middleware/backward-compatibility.ts:232-255 | gap | Remaining providers do not implement all Whisper options | Explicit migration compatibility gap |
| Free-tool request/result contracts | frontend/services/free-tools-processor/src/index.ts:44-50 | port | Existing server-first functionality separate from billed jobs | Free-tool shell and processor adapter |
| Process-local ephemeral sessions | frontend/services/free-tools-processor/src/lib/sessionStore.ts:20-70 | keep-knowledge | Restart loses sessions and counters; replicas need deliberate routing | Platform deployment constraint |

## billing

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| `request_breakdown_over_packages` | `frontend/supabase/migrations/20251230190000_fix_request_breakdown_user_id_again.sql:21-74` | port | It is the allocation rule: take from the package that ends first. Becomes a pure `allocate(requested, open_packages) -> Result<Vec<Deduction>, Insufficient>` with a tie break (DEF-016) | `modules/billing/core` |
| `deduct_minutes` (three overloads) | `frontend/supabase/migrations/20260115162255_fix_freemium_first_job_bypass.sql:25-91` and the two older overloads in the baseline | port | Becomes one function in the use case: check, allocate, write rows, in one transaction with a user lock (DEF-001). The skip flag becomes the explicit first-job bypass | `modules/billing/core`, `modules/billing/postgres` |
| `get_user_packages_with_balance`, `get_user_balance`, `user_balance_view` | `frontend/supabase/migrations/20251130113249_remote_schema.sql:733-778`, `:3739-3744` | simplify | The open-window and remaining-minutes rules move into the core (`balance(open_packages)`); the adapter reads one user's rows. A read-only SQL view may stay for reports. The old function names are what the dry run compares against (see Research) | `modules/billing/core`, `modules/billing/postgres` |
| `insert_usage_history_on_job_start` and trigger `job_status_trigger` | `frontend/supabase/migrations/20260408120000_fix_null_duration_api_job_trigger.sql:25-125` | port | This is a use case hiding in a trigger (`docs/thoughts.md`). Becomes `StartJob` billing step: freemium bypass, strict check, unknown-duration deferral. No trigger in the new database | `modules/billing/core` |
| `insert_new_packages_on_payment_completed` and trigger `payment_status_trigger` | `frontend/supabase/migrations/20251130113249_remote_schema.sql:853-905`, `:4222` | port | Becomes `GrantPackages(payment)`, idempotent by payment id, run from the outbox event `PaymentCompleted` (fixes DEF-004). The date rules (BIL-045) are copied exactly | `modules/billing/core`, `modules/billing/postgres` |
| `insert_freemium_package` and trigger | `frontend/supabase/migrations/20251130113249_remote_schema.sql:839-848`, `:4214` | port | Becomes `GrantFreemium(user)` on the `UserCreated` event: 20 minutes, 2 months, once per user | `modules/billing/core` |
| Webhook triggers `purchased`, `welcome`, `failed_job` | `frontend/supabase/migrations/20251130113249_remote_schema.sql:4210-4242` | throw | HTTP calls from the database, with a secret committed in the file. Replaced by the outbox and the `Mailer` port | outbox in `modules/billing/postgres` |
| `process_payment(text)`, `accounting."Invoices"`, `generic_contacts`, view `invoice_requests`, `public."Invoices"` | `frontend/supabase/migrations/20251130113249_remote_schema.sql:996-1045`, `:1295-1306`, `:3176-3185`, `:3401-3404`, `:3441-3451` | throw | Dead code, a function for a table that does not exist, and the Paraşüt bookkeeping that is out of scope. Keep the rows in the snapshot but do not recreate the tables | none |
| Stripe-shaped tables `products`, `prices`, `subscriptions` and their enums | `frontend/supabase/migrations/20251130113249_remote_schema.sql:204-235`, `:3529-3558`, `:3644-3661` | throw | Never written; the catalog is code | none |
| Analytics views over payments | `frontend/supabase/migrations/20260222092040_migrate_anayltics_to_analytics.sql:62-183` | throw | Reporting with fixed rates 35 and 36; the new app reports through analytics events | none |
| RLS insert policy on `usage_history` | `frontend/supabase/migrations/20251130113249_remote_schema.sql:4455` | throw | Lets users write their own ledger (DEF-002) | none |
| Python tests `test_balance.py`, `test_payment.py`, `fixtures.py`, `Pipfile` | `supabase-routines/` (archived 2025-12-30, `supabase-routines/README.md:1-30`) | keep-knowledge | Every case becomes a Rust test: `test_get_balance` (4 balance checks), `test_calculate_balance` (4 cases), `test_request_breakdown_over_packages` (6 cases, BIL-013 to BIL-018), `test_deduct_minutes` (5 checks), `test_deduct_minutes_from_earlier_package` (5 checks), `test_insert_new_packages_on_payment_completed` (3 cases). The pytest and testgres harness is thrown | `modules/billing/core` tests |
| pgTAP suite `020`, `030`, `040`, `060`, `080`, `090` | `frontend/supabase/tests/` (117 assertions: 23, 39, 16, 11, 14, 14) | keep-knowledge | Each assertion is referenced by a ledger row and becomes a Rust test. `000` and `050` belong to the data model topic. Weak spots to fix when porting: the 0 to 5 minute tolerance in `020:244` and `020:257`, the legacy overload in `020`, the dropped breakdown cases | `modules/billing/core` and `postgres` tests |
| `FREEMIUM-COVERAGE-MAP.md` | `frontend/supabase/tests/FREEMIUM-COVERAGE-MAP.md` | throw | Describes the removed resume feature and a deleted test file | none |
| Iyzico checkout, confirm and renewal code | `frontend/src/pages/checkout/index.tsx`, `confirm.tsx`, `frontend/src/pages/api/subscription-callback.ts`, `frontend/src/utils/iyzicoRenewal.ts` | port | This is the real adapter. Becomes the Rust Iyzico `PaymentGateway` adapter with a fake and recorded fixtures. Fix DEF-003, DEF-004, DEF-005, DEF-007, DEF-008 while porting; keep the field values in BIL-069 to BIL-071 | `modules/billing/gateways/iyzico`, `modules/billing/http` |
| `Infra/gcf-iyzico` | `Infra/gcf-iyzico/src/index.ts` | throw | An unused prototype with dummy data and no database access; nothing in `frontend` calls it | none |
| Gateway interfaces and `IyzicoGatewayService.ts` | `frontend/src/dao/IPaymentGateway.ts`, `frontend/src/features/billing/services/IyzicoGatewayService.ts` | simplify | The interface is the right idea (the `PaymentGateway` port); the service is an unimplemented skeleton that throws. Keep the port shape, drop the skeleton | `modules/billing/core` (port) |
| Package catalog | `frontend/src/domain/packages.ts` | port | Prices and plan codes become data owned by the module; the pricing pages read the same data (PRD phase P6) | `modules/billing/core` |
| Coupon, currency, subscription date helpers | `frontend/src/domain/coupon.ts`, `currency.ts`, `subscriptions.ts` and their tests | port | Pure functions with tests already; rules CPN-001 to CPN-014, BIL-063 | `modules/billing/core` |
| Freemium start check and lists | `frontend/src/utils/freemiumLimits.ts` | port | The 20 and 45 minute rule and both exact lists (FRE-015 to FRE-018). The stale test is thrown | `modules/billing/core` |
| The three freemium definitions | `frontend/src/dao/Balances.ts:19-31`, `frontend/src/utils/user.ts:21-45`, the trigger | simplify | One `is_freemium` (DEF-019) | `modules/billing/core` |
| Deferred billing calls in provider callbacks | `frontend/src/utils/deepgram.ts:324-420`, `frontend/src/utils/soniox.ts:205-305` | port | Becomes the use case `ChargeOnProviderAnswer(job, provider_minutes)` called by the transcription worker; the duration rounding stays (BIL-035) | `modules/billing/core`, called from the jobs module |
| Subscription settings tab (live) | `frontend/src/features/subscription/**`, mounted from `frontend/src/features/settings/containers/SettingsContainer.tsx` | port | Port the live behavior once: plan box, next payment, cancel (BIL-062 to BIL-065) | `modules/billing/web` |
| Old settings components, dead `BillingInfo` mock tab, `/dashboard/billing` placeholder | `frontend/src/components/Settings/SubscriptionInfo/SubscriptionInfo.tsx`, `frontend/src/components/Settings/BillingInfo/BillingInfo.tsx`, `frontend/src/pages/dashboard/billing.tsx` | throw | Not imported anywhere (or a placeholder); the mock tab even shows invented customer data | none |
| Billing modal, pay-as-you-go card, plan section, summary, mobile bottom sheet | `frontend/src/features/billing/**` | port | Same screens and steps; the view model rules move into the core and the shared state machine; desktop and phone views as in the web conventions. The PRD forbids new UX | `modules/billing/web` |
| `/billing` history and PDF invoice page | `frontend/src/pages/billing/index.tsx` | port | Live page; keeps the invoice id format, the TRY VAT rule and the admin lookup (BIL-075, BIL-076, CPN-018). How the PDF is drawn is a P5 decision | `modules/billing/web`, `modules/billing/http` |
| `NewBilling` page | `frontend/src/pages/NewBilling/index.tsx` | throw | A static Flowbite demo card ("Standard plan, 49 per month"); not linked or wired. `/billing` is the live page | none |
| `InvoiceForm` page | `frontend/src/pages/InvoiceForm/index.tsx` | throw | Its submit only logs to the console; nothing links to it. `billing_info` keeps its existing rows | none |
| `billing_info` table | `frontend/supabase/migrations/20251130113249_remote_schema.sql:3284-3295` | port | Customer invoice identity read by the PDF and the admin lookup; carried by the migration | `modules/billing/postgres` |
| Paraşüt (Turkish e-invoice) | `frontend/src/utils/parasut/**`, `frontend/faturas/`, `frontend/BRAINSTORM-parasut-fatura.md`, `frontend/docs/product/epics/FTRA-1-fatura-automation/FTRA-101-fatura-draft-automation.md`, `billing_info.parasut_contact_id`, `public."Invoices"` | throw | Out of scope and not ported (PRD section 2). The column `parasut_contact_id` is carried as data so nothing is lost | none |
| `fraud.ts` | `frontend/src/fraud.ts` | throw | Disabled (returns false). The old rule text is kept in BIL-077 | none |
| Special client list and alert | `frontend/src/constants/specialClients.ts`, `frontend/src/components/SpecialClientDiscountAlert` | simplify | Move the 725 ids and the percentage into data and show one coupon alert; fix the 25 vs 33 text (DEF-023) | `modules/billing/core`, `web` |
| Balance badge, not-enough-balance alert | `frontend/src/components/Balance/Balance.tsx`, `frontend/src/components/NotEnoughBalanceAlert` | port | Become view-model fields (balance text, "add minutes" action) in the shared state machine | `modules/billing/web` |
| `/api/admin/customer-invoices` | `frontend/src/pages/api/admin/customer-invoices.ts` | port | An admin use case behind an `admin` role through the `can()` check, not an email flag | `modules/billing/http` |
| `/api/cancel-subscription` | `frontend/src/pages/api/cancel-subscription.ts` | port | With owner check and a gateway call (DEF-006) | `modules/billing/http` |
| `/api/hooks/payment-updated`, purchase email, sales email | `frontend/src/pages/api/hooks/payment-updated.ts`, `frontend/src/emails/purchase-email.tsx`, `frontend/src/emails/backoffice/sales_email.tsx` | port | Two emails sent from the `PaymentCompleted` event through the `Mailer` port; same subjects and fields | `modules/billing/core`, `http` |
| `/api/subscribe` | `frontend/src/pages/api/subscribe.ts` | throw | A newsletter sign-up that only emails the team; not billing (belongs to the email topic) | none |
| `PriceCountdown` banner | `frontend/src/components/ui/PriceCountdown/PriceCountdown.tsx` | throw | Ended on 2025-09-30 and renders nothing | none |
| "billing new design" folder | `billing new design/` (4 image files, April 2024) | throw | Mock-ups of the modal that BILL-105 already implemented, with old prices. Keep only as a visual reference; not needed for the port | none |
| `BILL-107-FIX-SUMMARY.md` and the BILL epic stories 107 to 111 | `frontend/BILL-107-FIX-SUMMARY.md`, `frontend/docs/product/epics/BILL-1-billing-checkout/` | keep-knowledge | Lesson: check the balance before deducting; the resume feature was tried and reverted. Feed the book chapter; do not port the feature | `book/` |
| `billing-and-pricing.md` appendix | `frontend/docs/arch42/appendix/billing-and-pricing.md` | keep-knowledge | The best written map; two statements are wrong (refund, failed renewal) and must be corrected in the book | `book/` |
| LemonSqueezy | not in Transgate | gap | New adapter: hosted checkout for a package plus the order webhook only. No subscriptions, no customer portal, no license keys (PRD section 3) | `modules/billing/gateways/lemonsqueezy` |
| Iyzico recurring for international cards | `frontend/docs/product/epics/BILL-1-billing-checkout/BILL-114-flaky-subscription-checkout-international.md` | gap | Known to fail for some foreign cards today; the port inherits it unless the data fix in DEF-025 works | `modules/billing/gateways/iyzico` |
| Refund and adjustment | none today | gap | There is no refund path today and none is added; the ledger keeps room for an adjustment row (DEF-018) | `modules/billing/core` |

## data-model

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Stripe shaped tables `prices`, `products`, `subscriptions` and enums `pricing_type`, `pricing_plan_interval`, `subscription_status` | baseline `:3529-3663`; 1 analytics read | throw | No writer, no price read; prices live in code and translation strings. Check empty on the snapshot | none |
| Iyzico subscription columns `payments.pricing_plan_id`, `customer_id`, `subscription_canceled_at` | `frontend/src/pages/checkout/confirm.tsx:168,213`; `frontend/src/dao/Payments.ts:50-56`; `frontend/src/pages/api/cancel-subscription.ts:22-23` | keep | Live subscription behavior: choose subscription confirmation, find renewal customer payments, and record cancellation requests. Their Stripe-shaped names do not make them unused. Preserve values during migration; billing owns the replacement subscription identity and state | `modules/billing/postgres`, `billing` schema |
| Webhook tables: `webhook_notifications`, `webhook_payloads` (already dropped) and `send_webhook_notification` | baseline, `20251130114026` | throw | Rows are written, nothing ever sends them (`retry_count`, `next_retry_at` are unused) | none. Outbound callbacks come from the API module outbox |
| `callback_deliveries` | `20251130114026` | simplify | The delivery record is wanted (PRD scenario 6); keep the shape, drop the unused cleanup function, add retention | `modules` public API crate, `api` schema |
| `provider_health_metrics` and its cleanup job and function | baseline, `20251221200006` | throw | Health scoring is dropped (PRD section 2) | none |
| `provider_plugin_configs` | `20251130114026` | throw | Plugin configs are dropped (PRD section 2) | none |
| `workers`, `feature_flags` (runpod flag), `jobs.worker_selector`, `assigned_worker`, `request_time` | baseline | throw | RunPod is dropped (a known gap for R1); its routing slot stays in the `Transcriber` port | transcription core (gap noted for R1) |
| Analytics schema, 39 views | `20260222092040` and earlier | simplify | No app reader. Keep 5 read only ops views (daily_jobs_count, daily_summary_30d without hard coded emails, hanging_jobs, failed_jobs, user_payments_pending) in an `ops` schema readable only by staff; throw the other 34 | `ops` views in the transcription and billing migrations |
| `accounting` schema (2 views, `Invoices`) | baseline | throw | Paraşüt flow is out of scope; fixed FX rate and hard coded emails. Archive the rows | none (archive) |
| `audit` schema: `cleanup_audit_log` and 4 views | `20251222100000` | simplify | A record of retention runs is useful for privacy evidence; keep one table `retention_runs` with a closed status enum and one health view; throw the other 3 views | privacy module |
| Cleanup rule inside `cleanup_old_transcripts` (31 days, opt out aware, 100,000 row guard, advisory lock) | `20251222100001` | port | It is a privacy rule (PRD section 2 item 4), so it is a Rust use case on the `Scheduler` and `Lock` ports; the guard values become constants with tests | privacy module, rules PRV |
| `cleanup_dry_run` | `20251222000002` | simplify | Becomes a `dry_run` flag of the same use case | privacy module |
| `cleanup_old_health_metrics` | `20251222100001` | throw | Table dropped | none |
| `cleanup_cron_job_history` and its pg_cron job | `20251221200006` | port | Dumb housekeeping of pg_cron's own table; stays in the database (allowed by PRD section 3) | `deploy` migrations |
| `cleanup_old_audit_logs`, `cleanup_old_rate_limits`, `cleanup_old_callback_deliveries`, `expire_expired_keys` | baseline, `20251130114026` | simplify | Never scheduled. Retention of delivery and audit rows becomes part of the API module sweep; rate limit rows disappear with the shared limiter | API module |
| Tests inside migrations (15 functions) and `_backup_orphans` | `20251221200008`, `20251222000003`, `_backup_orphans/` | throw | Test code belongs in the test suite. Keep the boundary, NULL, parameter and threshold cases as Rust tests | privacy module tests |
| pgTAP files 000 to 090 | `frontend/supabase/tests/` | keep-knowledge | Each assertion maps to a ledger row (see pgTAP mapping); the files are not copied | `transgate/docs/rules.md` |
| API key tables `api_keys`, `api_key_audit_logs` | baseline | port | Keys keep working across the cut over (R3 decides the shim); keep hashes as they are | API module |
| `api_key_rate_limits` | baseline | throw | 24 hour counters, probably not enforced today (DM-04); rate limit becomes a port with a shared store | API module (RTL) |
| API key functions `create_api_key_with_expiration`, `verify_api_key`, `validate_api_key`, `revoke_api_key`, `get_expiration_policies`, `calculate_expiration_date`, `log_api_request` | baseline | port | Rules: key format `sk_` plus 32 random bytes, bcrypt hash, expiry policies, scope check, audit rows. Written in Rust, not as SQL functions | API module |
| Balance and deduction: `get_user_packages_with_balance`, `get_user_balance`, `request_breakdown_over_packages`, 3 `deduct_minutes`, `user_balance_view` | baseline, `20251230*`, `20260108130000`, `20260115162255` | port | Business rule (BIL). A pure `allocate(requested, open_packages)`, locked per user; the view becomes a read model | `modules/billing/core` |
| Payment trigger `insert_new_packages_on_payment_completed` | baseline `:853` | port | Plan expansion (yearly plan into packages) is a rule; becomes an `on_payment_completed` use case with the unique Iyzico token key kept as a constraint | `modules/billing/core` |
| Freemium trigger `insert_freemium_package` | baseline `:839` | port | 20 minutes, 2 months; runs from the account provisioning use case | `modules/billing/core` (FRE) |
| Job start trigger `insert_usage_history_on_job_start` | `20260408120000` | port | The deepest rule set (first job bypass, deferred billing, strict later jobs). 13 versions; the final one is the contract. Becomes the job start use case with compare and swap | `modules/billing` and transcription core |
| User mirroring triggers `handle_new_user`, `handle_update_user` | baseline `:784`, `:798` | simplify | A dumb copy of id, email, name is housekeeping and may stay as a trigger on a GoTrue `auth.users`, or the profile row is created on first sign in. Mu decides (open question) | identity |
| `anonymize_user_data` | baseline `:247` | throw | Likely a no-op (DM-15). Deletion becomes an explicit saga | privacy module |
| DB webhook triggers `welcome`, `purchased`, `failed_job` | baseline `:4210-4242` | throw | Emails are use cases fed by an outbox, secrets stay out of SQL | EML, Mailer port |
| `updated_at` triggers (jobs, ai_consent, highlights, summaries, provider_plugin_configs) and the two trigger functions | baseline, `20251223140321` | port | Pure housekeeping; stays in the database as one generic function | `deploy` migrations |
| RLS policies (59 on app tables, 4 on storage.objects) | baseline `:4436-4729` and later | simplify | Keep owner scoped SELECT policies as a safety net. Drop owner write policies on usage_history, jobs, users, chat_limits, api_keys (DM-05 to DM-09). The server connects as a trusted service | each module's migrations |
| pg_cron | `20251221200005` | port | Stays in the database for housekeeping only (cron history). Business schedules go to the `Scheduler` port | `deploy` |
| Extensions pg_net, pgsodium, pg_graphql, wrappers, uuid-ossp, pg_stat_statements | baseline | throw | No use found in the new design | none |
| Realtime publication on jobs and transcripts | baseline `:4741` | gap | The browser change feed has no equivalent until the push channel in the web app topic exists | web shell topic |
| Storage bucket `jobs` and its 4 policies | baseline `:5802` | throw | Unused; audio is in GCS or R2 | none |
| `billing_info`, `coupon` | baseline | port | Rows are used by the billing page and checkout | `modules/billing` |
| `Invoices`, `generic_contacts`, `invoice_requests`, `billing_info.parasut_contact_id` | baseline, `20250308143806` | throw | Paraşüt is out of scope. Archive rows (tax records), do not migrate | none (archive) |
| `ai_consent`, `translation_beta_consent_archived` | baseline, `20251208095855` | port | Consent evidence; keep rows. Cookie consent is a browser cookie, not a row | privacy module |
| `ai_analysis`, `chat_messages`, `chat_limits` | `20260305120001`, baseline | port | Core of the AI feature | AI module |
| `summaries`, `highlights` | baseline | simplify | No writer left; `summaries` is still read for callbacks. Copy rows into ai_analysis shape or keep read only; mu decides | AI module |
| `job_feedback` | baseline | port | Small, in use | transcription core |
| `transcripts` and `jobs` | baseline and later | port | See open question on rows against JSON | transcription core |
| `users` | baseline `:1347` | port | Settings columns move to a profile; account columns stay with GoTrue | identity (shared kernel) |
| `supabase/seed.sql`, `seed-ai-insights.sql` | frontend/supabase | keep-knowledge | Fixture ideas (freemium user, paid user, payments for invoices); not copied | `transgate/fixtures` |
| `supabase/config.toml` | frontend/supabase | keep-knowledge | Local defaults only (password length 6, MFA TOTP, 50 MiB uploads); production values are in the dashboard | docs |
| CI check that `types.gen.ts` is committed | `ci.yml:82-85` | keep-knowledge | Same idea as `./scripts/gen-bindings.sh --check` | `scripts/gate-*.sh` |
| Migration discipline: baseline squash, enum value in its own migration, `IF NOT EXISTS` guards, no down migrations | migrations | keep-knowledge | Lessons for the migration tool and the module migration folders | `docs` and the book |

## jobs-transcription

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Job transitions and billing boundary | frontend/src/services/unified-transcription/services/transcription.ts:395-484 | port | Keep user states and trigger outcomes without SQL business logic | Job aggregate and billing port |
| Claim and callback lessons | frontend/src/utils/deepgram.ts:459-581 | keep-knowledge | Existing guards do not enclose effects | Revision-checked saves and ingestion transaction |
| Soniox and Deepgram adapters | frontend/src/services/unified-transcription/providers/capabilities.ts:13-85 | port | Preserve routing and declared language coverage | Transcriber port adapters |
| Provider health scoring and plugin configuration | frontend/src/services/unified-transcription/services/provider-manager.ts:1112-1183,1392-1518 | simplify | PRD chooses language/model/duration routing and one retry policy | Core routing policy |
| RunPod GPU pipeline | runpod-whisper-x/src/whisperx_worker/rp_handler.py:279-413 | throw | PRD explicitly drops RunPod for now | Reserved provider slot only |
| RunPod grammar correction, custom Whisper options and extra fallback | frontend/src/services/unified-transcription/providers/capabilities.ts:88-104; frontend/src/services/unified-transcription/services/url-job-creator.ts:230-250 | gap | Not supplied by remaining adapters; no declared language is uniquely RunPod-only | Named migration gap |
| Upload signing and external URL sources | frontend/src/pages/api/init-upload.ts:20-49; frontend/src/services/unified-transcription/services/url-job-creator.ts:130-155 | port | Keep direct-to-storage uploads and deferred duration | Storage port and upload state machine |
| YouTube captions versus free transcript tool | frontend/src/pages/api/youtube/resolve.ts:105-158; frontend/src/pages/api/youtube/public-transcript.ts:33-146 | port | Distinct signed-in and anonymous outcomes | Source adapter and static tool shell |
| Editing, segmentation and export choices | frontend/src/utils/transcriptFileSaver.ts:101-183; frontend/src/features/transcriptLines/services/TranscriptLineService.ts:76-115 | port | Preserve text, speaker labels and user-facing format choices | Transcript domain and exporter port |
| Multiple scheduling mechanisms | frontend/src/scheduler/index.ts:26-66 | simplify | Shared scheduler/lock port is already agreed | Platform-owned schedule inventory |

## languages

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Master-translate-language-az---.json, -id---.json, -kk---.json | frontend/public/locales/en/ | throw | Three identical 14090 byte copies of one Swedish to English page template. No code, script, page or test in the Transgate tree names them (searched the whole tree without node_modules, .next, .git). | nowhere |
| reverse-translation/ folders | frontend/public/locales/*/reverse-translation/ | keep-knowledge | Only the page template shape is knowledge. en has 81 files, 56 are loaded by `translation/[...slug].tsx:725`; 25 are unused (albanian, amharic, basque, bengali, cebuano, chichewa, corsican, esperanto, filipino, georgian, gujarati, haitian-creole, hausa, hawaiian, hmong, igbo, irish, javanese, khmer, kinyarwanda, kurdish, kyrgyz, lao, latin, luxembourgish). The 15 other locales hold 1 file each and no path generates them (reverse paths are built for en only, `:535-566`). Static site topic decides. | modules/cms |
| supportedSlugs SEO list | frontend/src/config/languages.ts | keep-knowledge | 226 SEO slugs for 56 languages. Reference only; owned by the static site topic. Not a product capability list. | modules/cms |
| translate_languages_page.py | frontend/translate_languages_page.py | throw | One off script that writes `public/locales/en/languages.json` (5600 SEO keys) from a language list of 56 names. The output is already committed; the new static site generates pages from content files. | nowhere |
| Unified provider plugin framework language fields | frontend/src/services/unified-transcription/framework/ | throw | `supportedLanguages` per plugin duplicates `capabilities.ts`; PRD drops the plugin framework. | Transcriber port |
| utils/transcriptionProvider.ts selectTranscriptionProvider | frontend/src/utils/transcriptionProvider.ts | throw | No importers. The live selector is the provider manager. | nowhere |
| Dead language helpers | frontend/src/utils/languageContext.ts: createLanguageContext, getJobLanguage, getEffectiveLanguage, getEffectiveLanguageWithFallback, mapLanguageToCodeSafe, getUserLocale, mapDatabaseLanguageToCodeSafe, getLanguageErrorMessage, logLanguageError | throw | No non-test importer. The live ones are getJobAILanguageWithErrorHandling, getJobLanguageWithErrorHandling, mapDatabaseLanguageToCode and handleLanguageError. | nowhere |
| Language lists, names, initials, flags | frontend/src/domain/languages.ts | port | Source of `transgate/core/src/languages.rs`; generated from the fixtures. | transgate/core |
| Soniox set of 53, translation targets of 53, RTL set, freemium language sets, AI fallback to English | capabilities.ts, textDirection.ts, freemiumLimits.ts, languageErrorHandler.ts | port | Rules LNG-007, 015, 029, 030 as data and pure functions. | transgate/core |
| RunPod/WhisperX path | utils/runpod.ts, providers/runpod.ts, runpod-whisper-x/ | gap | Dropped for now (PRD). No language of the 61 depends on it (see Research). The routing slot stays. | Transcriber port |
| Languages outside the 61 that WhisperX could take | runpod-whisper-x/.../utils.py:9-110 | gap | 39 Whisper codes the app never offers. No product impact. | nowhere |
| Locale fallback to English and per locale gaps | public/locales/* | gap | 15 locales miss 152 AI keys and some common keys; German `app` does not parse. The new app copies the English fallback unless mu fills them. | modules/cms and web app |

## platform

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

## privacy-identity

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Transcript retention rule (31 day cutoff, completed jobs, opt-out, NULL means keep) | `cleanup_old_transcripts` in `20251222100001_update_cleanup_functions_audit_schema.sql` | port | It decides what happens to user data and has an opt-out. It must live once with tests (PRV-014 to PRV-017). Runs as a use case behind the Scheduler port with a Lock port, not in a function. | core: `RetentionSweep` use case; postgres: repository query; taskr scenario 8 |
| Dry run | `cleanup_dry_run` in `20251222000002_fix_cleanup_functions_safety.sql` | port | Same predicates must produce the plan and the delete; one function returns a plan that apply then executes. | core: `RetentionSweep::plan` |
| Sweep safety checks (7 to 365 range, 100000 threshold, advisory lock, audit row) | same functions | simplify | Range check becomes a typed value. Threshold, single run lock and audit row stay. Statuses `skipped` and `aborted` really exist this time. | core (threshold, audit), Lock port (Postgres advisory lock) |
| Health metrics cleanup | `cleanup_old_health_metrics` | throw | The provider health scoring tables are dropped by the PRD (health scoring removed). | none |
| pg_cron history purge | `cleanup_cron_job_history` and cron entry | simplify | P3 D14 adapts this obligation to finished rows of the replacement scheduler's own run history; preserve cutoff, range/threshold guards, single-run lock and durable audit. No pg_cron dependency. | core: history-retention use case; media-postgres: named finished schedule slots |
| Schedules (02:00 daily sweep, weekly history purge) | `20251221200006_schedule_cleanup_jobs.sql` | port | Business schedules live in Rust through the Scheduler port; the exact times and UTC are kept (CRN rows). History cleanup follows P3 D14 on the replacement scheduler. | core: schedule table; infra: replicas safe via Lock |
| Cleanup audit table | `audit.cleanup_audit_log` | simplify | Keep one run record table (job, counts, status, error, time, trigger). Add real statuses. No public grants. | postgres: migration, `audit` schema kept |
| Monitoring views | `audit.cleanup_metrics_summary`, `cleanup_job_health`, `cleanup_daily_stats`, `cleanup_audit_recent`, `public.user_opt_out_stats` | simplify | Read only views are allowed. Keep `cleanup_job_health` (stale or failing sweep) and `user_opt_out_stats`; drop the other three, they are plain queries on the run table. | postgres: views |
| Cleanup tests shipped as migrations | `20251221200008_cleanup_test_suite.sql`, `20251222000003_comprehensive_test_suite.sql` | throw | Test code in the live schema, deletes real data, helpers violate the FK. Their 9 behaviors are ledger rows PRV-014 to PRV-025 and become Rust tests. | core and postgres tests |
| Orphan purge migration | `20251222000000_cleanup_orphaned_transcripts.sql` | throw | One time data fix. The migration tool drops orphans and counts them in the parity report. | tools/migrate |
| `transcripts.job_id` RESTRICT FK | `20251222000001_add_transcripts_fk_constraint.sql` | simplify | Keep the integrity rule (a line belongs to a job). The deletion saga deletes children first so RESTRICT can stay. | postgres: schema |
| `auto_delete_transcripts` and its toggle | `users` column, `ManageDataView` | port | Opt-out is a guardrail item. Same default `true`, same confirm text, same write semantics. | core: preference; web: settings tab |
| `unlimited_history` | `users` column, `JobRepository` list queries | port | Read-side rule copied (PRV-032, PRV-033). No UI added. | core: list query rule; postgres |
| Legacy `deleted_monthly` flag and toggle | `users` column, `ManageDataView` | throw | Nothing reads it (PRV-036). Do not migrate the column. Confirm with mu because the toggle is user visible. | none |
| Account deletion | `delete-account.ts`, `anonymize_user_data`, `on_user_deleted`, FK cascades | port | Becomes the deletion saga (scenario 8) with ordered, recorded steps and honest errors. The trigger and function are dropped (PRV-D04). | core: `DeleteAccount` saga; taskr scenario 8 |
| Soft delete of jobs ("Delete All") | `JobRepository.softDeleteAllJobs` | port | Keep status `deleted` and the list filter (PRV-011, PRV-012). Hard delete only if mu accepts PRV-D05. | core |
| Storage lifecycle (30 day delete, 31 day retention) | `Infra/infrastructure/terraform/main.tf` | keep-knowledge | The two numbers are part of the 30 day promise. The new storage adapter documents them and the saga deletes files on account deletion. | infra; core port `ObjectStore::delete` |
| User mirroring triggers (`handle_new_user`, `handle_update_user`) | `20251130113249_remote_schema.sql:784-811` | simplify | GoTrue stays and user ids are kept (PRD). The mirror is a copy of id, email, name, avatar. Keep it as dumb copy triggers or a first-request upsert (open question). No business work in them. | postgres: migration |
| `insert_freemium_package_trigger` | `20251130113249_remote_schema.sql:839-848` | port | A rule in a trigger. Becomes part of the signup use case in one transaction (freemium topic owns the numbers). | core: signup use case; modules/billing |
| `welcome` pg_net trigger and `user-inserted` hook | `20251130113249_remote_schema.sql:4242`, `user-inserted.ts` | simplify | Same email and event, delivered by an outbox consumer with retry instead of an HTTP call with a secret. | core: outbox consumer; Mailer port |
| `blocked` flag | `users` column, two YouTube routes | port | Field kept, but enforced in the use case layer. Scope is mu's decision (PRV-D11). | core: Identity actor and guard |
| AI consent record and gate | `ai_consent`, `consent-service.ts`, `analyze.ts`, `ai-service-manager.ts` | port | Guardrail item. One server side consent use case; revoke deletes all AI tables including `ai_analysis`. The AI topic owns the features. | core: consent use case; postgres |
| Translation beta consent | `translation_beta_consent_archived` | throw | No effect anywhere. mu decides whether the archive is exported once for compliance. | tools/migrate (optional export) |
| Cookie consent banner and cookie | `CookieConsent.tsx`, `PostHogProvider.tsx` | port | Platform feature behind a port implemented by the shell (agreed direction). Same cookie name, 365 days, accept and reject effects. | web shell; core port `Consent` |
| Google Analytics, Hotjar and New Relic browser scripts | `_app.tsx:56-57`, `_document.tsx:15` | gap | Not in the PRD list of kept integrations (PostHog and Sentry only). Name it so mu can decide. | none |
| GoTrue (Supabase Auth) as identity | hosted Supabase project | keep-knowledge | PRD keeps GoTrue behind the `Identity` port; ids kept. Settings in "E2 inputs" below must be exported from the hosted project. | postgres/infra: compose |
| Auth UI library screens (sign in, sign up, magic link, reset, Google) | `signin.tsx`, `new-signin.tsx`, `@supabase/auth-ui-react` | port | Same flows and copy in 16 locales as web desktop and phone views; the library is replaced by views that call the Identity port. | web: views and containers; android and ios shells |
| Home sign in popup | `SimpleSignInPopup.tsx` | port | Live behavior: desktop only, `/` only, 2 seconds, Google plus guest. | web |
| Unmounted `AutoSignInModal` and config | `AutoSignInModal.tsx`, `config/autoSignInModal.ts`, `AutoSignInModalDebug.tsx` | throw | Not rendered anywhere. | none |
| Turnstile | `signin.tsx:176` | port | PRD section 10 keeps it behind a captcha port with a fake. Today it is decorative (IDN-D01); the new one verifies the token on the server (GoTrue captcha or a use case). Needs mu: is it enabled in the Supabase project. | core: `Captcha` port; web |
| `fraud.ts` and its test | `src/fraud.ts`, `src/fraud.test.ts` | throw | Disabled stub. Rules kept as knowledge in IDN-031. | none (knowledge) |
| In-process rate limiter | `userRateLimit.ts` | simplify | Same window rule as a `RateLimiter` port with a shared adapter, safe with two replicas (RTL topic). | core port; postgres adapter |
| Middleware | `src/middleware.ts` | simplify | Keep request id and correlation id headers and the `/` to `/home` redirect for signed in users. Drop the dead `blocked` comment and the TODO. | server (Axum layer); web |
| `withUserProfile` guard | `utils/withUserProfile.tsx` | simplify | A guard in the web shell plus a server side extractor on API routes. | web; server |
| API key to Supabase JWT minting | `utils/supabase/jwt.ts` | throw | PRD: own API keys, not Supabase JWTs. | API topic |
| Service role admin client | `utils/supabase/admin*.ts` | throw | The server is a trusted service connecting with its own credentials. | postgres |
| next-auth packages | `package.json` | throw | Unused. | none |
| Dashboard `profile` and `settings` pages | `pages/dashboard/*.tsx` | throw | "Coming soon" shells. | none |
| `new-password.tsx` | `src/pages/new-password.tsx` | simplify | One update password screen with a confirmation field, used after the recover link. | web |
| Legal pages (privacy, terms) | `src/pages/legal/*.tsx` | keep-knowledge | Static content for the cms module; the conflicting numbers are listed in PRV-D14. | modules/cms |
| ADR-022, TECH-103 notes | `docs/arch42/decisions/ADR-022-*`, `docs/product/epics/TECH-1-technical-improvements/TECH-103-*` | keep-knowledge | Lessons: dry run, lock, threshold, audit, and "marked done but never deployed". | book chapter on retention |
| Webhook secret header | `src/webhooks.ts` | simplify | Internal events replace the trigger hook. External provider callbacks keep their own verification (jobs and billing topics). | core; server |

## screens

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Workspace order and split views | `frontend/src/pages/home/[[...jobId]].tsx:339-521` | keep-knowledge | Keep familiar upload and transcript flow, not React implementation | Presentation state machine plus Svelte desktop/phone views |
| Feature VMs, signals, contexts and DAOs | `frontend/src/features/transcription/viewmodels/useTranscriptionViewModel.ts:139`; `frontend/src/pages/_app.tsx:43-78` | simplify | One core state/effect boundary instead of duplicated browser business logic and direct database calls | Rust presentation and ports |
| Redux audio slice | `frontend/src/redux/store.ts:1`; `frontend/src/redux/audioPlayerSlice.ts:1` | simplify | Player mechanics stay shell-local; domain state must not fork across Redux/signals | Shell media adapter |
| Sidebar variants | `frontend/src/components/SideBar/SideBar.tsx:163-330`; `frontend/src/infrastructure/navigation/components/MobileNavigationContainer.tsx:307-343` | simplify | One navigation model, desktop sidebar and phone drawer; no parallel legacy NewSidebar framework | Shared navigation VM |
| Working purchase and invoice history | `frontend/src/features/billing/containers/BillingModalContainer.tsx:1`; `frontend/src/pages/billing/index.tsx:374-438,639-688` | port | Keep package selection, payment and invoices; Paraşüt excluded by PRD | Billing module and shell |
| NewBilling, InvoiceForm, disabled dashboards | `frontend/src/pages/NewBilling/index.tsx:3-8`; `frontend/src/pages/InvoiceForm/index.tsx:48-52`; `frontend/src/utils/featureFlags.ts:234-241` | throw | Prototypes and unavailable features are not parity requirements | No replacement fake pages |
| Landscape blocker and popup | `frontend/src/pages/_app.tsx:53-74` | keep-knowledge | Record current mechanics; do not infer new native landscape restriction | Web-shell design review |
| Presentation and KenzNote promotion | `frontend/src/components/Presentation/index.tsx:1-7`; `frontend/src/components/UserArea/UserArea.tsx:159-267` | keep-knowledge | Marketing/promotion is not transcription domain behavior | Public shell/content decision |
| Voice experiment | `frontend/src/pages/voice-agent-test.tsx:1` | gap | PRD presumes voice demo out of scope; R4 records confirmation needed | No transcription-core port |
| Existing story fixtures and tests | `frontend/src/features/settings/views/SettingsView.stories.tsx:1`; `frontend/src/features/billing/__tests__/useBillingViewModel.test.ts:1` | keep-knowledge | Useful behavior examples, not evidence that the Rust rewrite passes | Future acceptance scenarios |

## static-pages

| Item | Where today | Verdict | Reason | Lands in |
|---|---|---|---|---|
| Home, editorial, personas, features, FAQ, legal and contact copy | `frontend/src/pages/pricing.tsx:1`; `frontend/src/config/features.ts:1`; route inventory above | port | Markdown content plus reusable data templates; preserve URLs and locale coverage | modules/cms |
| Root keyword, language and translation matrices | `frontend/src/config/seoIndexing.ts:245-298` | keep-knowledge | Keep intentional pruning, traffic exceptions and redirect/canonical lessons, not duplicate React pages | CMS URL manifest and templates |
| Blog Markdown, images and metadata | `frontend/src/pages/blog/[[...articleSlug]].tsx:162-225` | port | Existing git content fits CMS; migrate metadata without losing slugs | modules/cms |
| Pricing | `frontend/src/components/LandingPage/PricingCard/PricingCard.tsx:29-37` | simplify | Template reads billing catalog; no second price authority | CMS plus billing catalog |
| trim-audio, audio-trimmer and video-trimmer | `frontend/src/utils/free-tools/trimAudioServer.ts:60-132` | port | Shared trim UI, runtime processor, files and downloads | Public interactive shell plus runtime service |
| convert-audio | `frontend/src/utils/free-tools/convertAudioServer.ts:37-64` | port | Real conversion, not Markdown | Public interactive shell plus runtime service |
| reduce-audio | `frontend/src/utils/free-tools/reduceAudioServer.ts:37-73` | port | Real compression and progress | Public interactive shell plus runtime service |
| remove-noise | `frontend/src/utils/free-tools/removeNoiseServer.ts:34-76` | port | Real denoise and progress | Public interactive shell plus runtime service |
| video-to-audio | `frontend/src/utils/free-tools/videoToAudioServer.ts:40-89` | port | Real extraction and progress | Public interactive shell plus runtime service |
| compress-video | `frontend/src/utils/free-tools/compressVideoServer.ts:33-77` | port | Real compression and progress | Public interactive shell plus runtime service |
| convert-video | `frontend/src/utils/free-tools/convertVideoServer.ts:44-96` | port | Real conversion and progress | Public interactive shell plus runtime service |
| Recorder and tab capture | `frontend/src/pages/tools/record-audio-online.tsx:195-230`; `frontend/src/components/TabAudioCapture/DownloadControls.tsx:59-96` | port | Browser capture/download islands, not transcription jobs | Public web shell |
| Free YouTube captions | `frontend/src/components/features/youtube-transcript/YouTubeTranscriptTool.tsx:136-139` | port | Public endpoint distinct from paid transcription | Public web plus API adapter |
| Voice experiment | `frontend/src/pages/voice-agent-test.tsx:3-10,28-31`; `frontend/src/features/voice-demo/hooks/usePersonaPlex.ts:147-162` | gap | Separate websocket voice product; presumed excluded by PRD R4 | No port unless mu changes scope |
| Generated documentation site | `frontend/site/index.html`; `frontend/site/arc42/index.html` | keep-knowledge | Architecture documentation, not the production public page router | Reference material only |

