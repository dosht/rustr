# Data model

Production database of the Transgate app (Supabase Postgres 15): 4 schemas of its own (public, accounting, analytics, audit; the misspelled anayltics was dropped), 32 tables, 50 views, 11 enums, 52 routines (51 functions and 1 procedure, 15 of them test code), 14 triggers (3 on auth.users, 3 are HTTP calls to production), 3 pg_cron jobs and 59 RLS policies.
53 migrations (one squashed baseline of 5,839 lines, 12 rewrites of the job billing trigger function, a six file fix-the-fix day on 2025-12-30, a four file analytics schema rename), and `types.gen.ts` matches them exactly (CI regenerates it from the migrations, so this says nothing about production drift).
13 of the 32 tables are not migrated (11 dead, rate limit counters, the cleanup log); the money and balance rules sit in 7 functions (3 of them triggers); every function and table is granted to `anon`, which leaves 7 high severity defects (DM-01 to DM-06 and DM-11).

## Inventory

### 2a. Schemas and extensions

Final state after all 53 migrations, cross checked with `frontend/src/types.gen.ts` (CI regenerates it from a local database built from the migrations and fails on any diff, `.github/workflows/ci.yml:82-85`, so it proves migrations equal types, not that production equals migrations).

Schemas created by the app's migrations:

| Schema | Created in | State | Contents |
|---|---|---|---|
| public | Supabase default | live | 30 tables, 5 views, 52 routines, 11 enums (all app data) |
| accounting | baseline `:15` | live, nothing reads it | 1 table (`Invoices`, Paraşüt invoice records) and 2 sales views |
| analytics | baseline `:21` | live, no app code reads it | 39 reporting views, no tables |
| anayltics (misspelled) | baseline `:27` | dropped by `20260222092041` (CASCADE) | had 26 views, recreated in `analytics` by `20260222092040` |
| audit | `20251222100000` | live | 1 table (`cleanup_audit_log`), 4 views |

Supabase managed schemas the app depends on: `auth` (users, identities; the app has 3 triggers on `auth.users` and 15 foreign keys into it), `storage` (4 policies on `storage.objects`), `realtime` (publication `supabase_realtime` holds `public.jobs` and `public.transcripts`, baseline `:4741-4745`; the browser listens with `postgres_changes` in `src/dao/JobRepository.ts:440-477` and `src/dao/TranscriptLineRepository.ts:65-79`), `supabase_functions` (the `http_request` function used by 3 triggers), `cron` (pg_cron tables), `extensions`, `graphql`.

Extensions:

| Extension | Schema | Added in | Used for | Needed in new system |
|---|---|---|---|---|
| pg_net | extensions | baseline `:33` | Backs `supabase_functions.http_request`, so the `welcome`, `purchased` and `failed_job` triggers can call HTTP | No. Emails move to use cases. |
| pgsodium | default | baseline `:40` | Supabase default, no app use found | No |
| pg_graphql | graphql | baseline `:56` | Supabase default, no app use found (code uses PostgREST client) | No |
| pg_stat_statements | extensions | baseline `:63` | Supabase default, query stats | Optional |
| pgcrypto | extensions | baseline `:70` | `crypt`, `gen_salt('bf')` (API key bcrypt), `gen_random_bytes` | Only if API key hashes keep being verified in SQL. Rust verifies bcrypt itself. |
| uuid-ossp | extensions | baseline `:77` | `uuid_generate_v4()` defaults on 3 tables, `ai_analysis.id` | No (use `gen_random_uuid()`) |
| wrappers | extensions | baseline `:84` | Supabase foreign data wrappers, no use found | No |
| pg_cron | extensions | `20251221200005` | 3 cleanup jobs (see pg_cron section) | Yes, housekeeping only (PRD 3) |
| pgtap | test database only | `supabase test db` | 8 SQL test files | Yes in the new repo, for constraint and view tests |

`supabase/config.toml` is local development only: PostgREST exposes schema `public` only (`schemas = ["public"]`, line 13), Postgres major version 15 (line 30), storage upload limit 50 MiB (line 83), signup enabled with confirmations off, minimum password length 6, MFA TOTP enabled, no OAuth provider enabled in the file. The real production settings (exposed schemas, Google OAuth, password rules) live in the Supabase dashboard and are not in the repo. See Needs mu.

### 2b. Tables

32 base tables: 30 in `public`, 1 in `accounting`, 1 in `audit`. Counts are exact. `types.gen.ts` lists the same 30 public tables. "Src sites" counts literal `.from('table')` calls in `frontend/src` (a multi-line aware scan, tests and stories excluded; there is no test file hit at all), then calls that reach the table through an rpc function or a view. `scripts` and e2e `tests` are named separately. A table nobody touches is marked throw candidate. Ownership column "user" is the column that holds the account id. All tables in `public` also carry `GRANT ALL` to `anon`, `authenticated` and `service_role` (default privileges, baseline `:5725-5751`, plus explicit GRANT ALL lines at `:4756-5724`), so RLS is the only barrier. Policy names are shortened. RLS is on for 31 of the 32 tables (only the archived consent table has it off) and there are 59 policies on app tables.

Bounded contexts used below: identity (shared kernel), transcription (core), billing (module), cms, ai, api (public API), privacy, analytics, dead.

| Table | Purpose | Key columns (ids, FKs, status, JSON, timestamps) | User column | RLS and policies | Src sites | New context | Verdict |
|---|---|---|---|---|---|---|---|
| public.users | Profile mirror of auth.users plus user settings | id uuid PK, FK to auth.users cascade; email, full_name, avatar_url; deleted_monthly bool default true; blocked bool; auto_delete_transcripts bool NOT NULL default true (`20251221200001`); unlimited_history bool NOT NULL default false (`20260707120000`). No timestamps | id | on; 2: select own, update own (any column, so the owner can flip blocked and unlimited_history) | 15 | identity (settings columns go to privacy and transcription settings) | migrate rows; email, full_name, avatar_url derive from auth.users |
| public.jobs | One transcription request: queue, state machine and audit in one row | id bigint identity PK; user_id FK users cascade; job_status job_status; language, translate_to language[]; duration smallint (minutes); transcription_provider (default deepgram), soniox_transcription_id, recovery_attempts int NOT NULL default 0, retry_count; storage_provider (default gcs), file_path, callback_url, youtube_video_id, youtube_url, transcript_source; error jsonb, whisper_params jsonb; created timestamp (UTC, no zone), started_at, updated_at timestamptz (BEFORE UPDATE trigger); speakers, fix_grammer, size, cost, request_time, worker_selector, assigned_worker (FK to jobs.id, wrong target), job_name | user_id | on; 3: ALL own ("Can operate own jobs data."), update own, select own. The owner can write every column, including duration and job_status that the billing trigger trusts | 102 (+5 scripts, +9 e2e); also written by runpod-whisper-x (`supabase_utils.py:442`) and Infra gcf-process-uploaded-file | transcription | migrate rows; drop worker_selector, assigned_worker, request_time, whisper_params after moving YouTube keys; keep callback_url only for external_url rows |
| public.transcripts | One row per transcript line, source or translated | id bigint identity PK; job_id FK jobs RESTRICT (`20251222000001`); user_id FK auth.users cascade; start_time, end_time text (SRT style strings, not intervals); line, corrected_text, speaker, file, split_on_speaker_change; language, translation_status (default original); created timestamptz | user_id | on; 3: ALL own, update own, select own | 15 (+1 script, +6 e2e); runpod worker inserts (`supabase_utils.py:390`) | transcription | migrate rows (largest table: 2,142 MB of 3,634 MB in Jan 2026, ADR-033); times to numeric; the `file` column is null in the seed rows, its use was not checked |
| public.payments | One checkout attempt per row | id serial PK; user_id FK users cascade (nullable); amount numeric, currency text (3 letters not enforced), payment_gateway text, status payment_status NOT NULL; token text (gateway token); coupon_id FK coupon(id); pricing_plan_id, customer_id, subscription_canceled_at (live Iyzico subscription fields); metadata jsonb (package shape read by the payment trigger); created (timestamp without zone), updated_at | user_id | on; 1: select own. No insert or update policy, so only the service role writes | 10 | billing | migrate rows, including the three live subscription fields; see the keep/throw source references |
| public.packages | One minute package with a time window | id serial PK; user_id FK users cascade; payment_id FK payments (no cascade); name text (the string 'freemium' is a business key); minutes_included; created, start_at, end_at timestamptz NOT NULL; metadata jsonb | user_id | on; 1: select own (authenticated) | 5 (+1 script); read by user_balance_view and 3 functions | billing | migrate rows |
| public.usage_history | The usage ledger: minutes consumed per package per job | id serial PK; user_id FK users cascade; package_id FK packages cascade; job_id FK jobs cascade; usage_date timestamptz; minutes_consumed int | user_id | on; 3: select own, INSERT own (authenticated, any values), update for service_role | 0 direct (+1 script); written by deduct_minutes; read by get_user_packages_with_balance | billing | migrate rows. Note ON DELETE CASCADE from jobs: deleting a job erases its ledger rows |
| public.coupon | Coupon codes | id text PK (the code); amount real; discount_type; expires_at (default now()); filter jsonb; created_at | none | on; 1: select for everyone including anon (`USING true`) | 2 | billing | migrate rows |
| public.billing_info | Customer invoice details (company, address, tax id) | id bigint identity PK; user_id FK users set null; parasut_contact_id; address, company_name, country, full_name, tax_id; created_at | user_id | on; 2: insert own, select own | 2 (billing page, admin customer-invoices) | billing | migrate rows; parasut_contact_id is archive only |
| public.Invoices | Paraşüt invoice link per payment | id bigint PK; payment_id FK payments set null; user_id FK users set null; billing_info_id FK; parasut_invoice_id; issued_at | user_id | on; 0 policies (service role only) | 0 | dead (Paraşüt is out of scope) | archive rows, do not migrate |
| accounting.Invoices | Older invoice record table | id bigint PK; payment_id FK payments RESTRICT; invoice_number, invoice_date, invoice_type, invoice_status, amount, vat real | none | on; 0 policies | 0 | dead | archive, do not migrate |
| public.generic_contacts | Paraşüt contact id per currency, for invoices without billing info | currency text PK; parasut_contact_id | none | on; 0 policies | 0 (read by view invoice_requests) | dead | throw |
| public.prices | Stripe style price list | id text PK; product_id FK products; unit_amount, currency (CHECK length 3), type pricing_type, interval, trial_period_days, metadata | none | on; 1: select for everyone | 0 | dead | throw (prices live in code and translation strings today) |
| public.products | Stripe style product list | id text PK; name, description, image, active, metadata | none | on; 1: select for everyone | 0 | dead | throw |
| public.subscriptions | Stripe style subscription state | id text PK; user_id FK auth.users (no cascade); status subscription_status; price_id FK prices; period and trial timestamps; metadata | user_id | on; 1: select own | 1 (`src/hooks/useUserIdentification.ts:158`, analytics only) | dead | throw after confirming the table is empty |
| public.ai_consent | AI consent per user | id bigint PK; user_id UNIQUE FK auth.users cascade; has_consented, consent_version (default '1.0'), consent_date, revoked_date, privacy_policy_url, provider_name (default deepseek); created_at, updated_at (trigger) | user_id | on; 4: select, insert, update, delete own | 7 | privacy (consent) with ai | migrate rows (legal record) |
| public.translation_beta_consent_archived | Renamed beta consent table, read only | same shape as ai_consent without privacy_policy_url; user_id UNIQUE FK auth.users cascade | user_id | RLS disabled, all grants revoked, `SELECT` only for service_role (`20251218000000`) | 0 | privacy | migrate rows as historic consents |
| public.ai_analysis | Current AI result per job (summary, highlights, typed analysis) | id uuid PK; job_id FK jobs cascade; user_id FK auth.users cascade; UNIQUE(job_id, user_id); summary text; highlights jsonb; analysis_type varchar default 'generic'; analysis_data jsonb; status text CHECK pending/partial/interrupted/completed default completed; ai_provider; tokens_used; processing_time_ms; auto_generated; created_at, updated_at (no trigger, so updated_at is set by app) | user_id | on; 1: ALL own (authenticated). Created outside migrations in production, documented retroactively (`20260305120001`) | 7 | ai | migrate rows |
| public.summaries | Older AI summary store | id bigint PK; job_id FK jobs cascade; user_id FK auth.users cascade; UNIQUE(job_id, summary_type); summary_text, summary_type default 'general', content_hash varchar(64), ai_provider; tokens_used; created_at, updated_at (trigger) | user_id | on; 4 own | 6 sites, all reads or deletes: no code inserts any row | ai | migrate rows (still read for the callback payload, `callback-delivery.ts:126`); no new writers |
| public.highlights | Older AI highlights store | id bigint PK; job_id UNIQUE FK jobs cascade; user_id FK auth.users cascade; highlights jsonb; content_hash; ai_provider; timestamps (trigger) | user_id | on; 4 own | 3, all deletes (consent revocation and cleanup) | ai | migrate rows or drop; superseded by ai_analysis. Mu decides |
| public.chat_messages | AI chat turns per job | id bigint PK; job_id FK jobs cascade; user_id FK auth.users cascade; role CHECK user/assistant/system; content; message_order int, UNIQUE(job_id, message_order); ai_provider default deepseek; tokens_used; processing_time_ms; created_at | user_id | on; 4 own | 5 (+ get_next_message_order rpc) | ai | migrate rows |
| public.chat_limits | Counters for AI chat limits | id bigint PK; user_id FK auth.users cascade; job_id FK jobs cascade (nullable); UNIQUE(user_id, job_id, limit_type, limit_period); limit_type CHECK 4 values; limit_period CHECK daily/weekly/monthly/transcript; current_count, max_limit, reset_date; created_at, updated_at | user_id | on; 3: select, insert, update own (so the owner can reset a counter) | 0 direct, 12 rpc (8 get_or_create_chat_limit, 4 increment_chat_limit) | ai | migrate rows (transcript level limits never reset) |
| public.api_keys | Public API keys, bcrypt hashed | id uuid PK default gen_random_uuid(); user_id FK auth.users cascade; name, UNIQUE(name, user_id); key_hash text (pgcrypto bcrypt); scopes jsonb default []; metadata jsonb; expires_at, last_used_at; is_active default true; expiration_policy enum default never; created_at | user_id | on; 4 own (the owner can insert a row with any scopes and any hash) | 2 (+ rpc create 1, revoke 1, verify 1, policies list 1) | api | migrate rows (hashes kept so old keys keep working; see Research) |
| public.api_key_audit_logs | Audit trail of key use | id uuid PK (uuid_generate_v4); api_key_id FK api_keys set null; user_id FK auth.users set null; action; details jsonb; ip_address inet, user_agent, endpoint, method, response_status, request_duration_ms; created_at | user_id | on; 1: select own | 1 (`src/pages/dashboard/audit-logs.tsx:44`) + rpc log_api_request 1 + verify_api_key writes one row per call | api | migrate recent rows or archive (HIPAA claim in function comments) |
| public.api_key_rate_limits | Per key window counters | id uuid PK; api_key_id FK api_keys cascade; user_id FK auth.users cascade; window_start timestamptz; request_count; created_at, updated_at. No unique key on (api_key_id, window_start) although check_rate_limit uses ON CONFLICT on it | user_id | on; 3: select, insert, update own | 0 direct, 1 rpc | api | throw (24 hour data, rate limits become a port) |
| public.callback_deliveries | Delivery log of customer callbacks (job finished) | id bigint PK; job_id FK jobs cascade; callback_url; status CHECK pending/success/failed; request_payload jsonb; response_status, response_body, error_message; created_at, delivered_at | via jobs.user_id | on; 2: select own via jobs (authenticated), ALL service_role | 2 (`callback-delivery.ts:287,334`) | api | migrate last 30 days or throw; cleanup function for 30 days is never called |
| public.webhook_notifications | Outbound notification queue, never processed | id uuid PK; user_id FK auth.users cascade; event_type; payload jsonb; status default pending; retry_count; next_retry_at; delivered_at; created_at, updated_at | user_id | on; 3 own | 0 direct (written by send_webhook_notification) | dead | throw |
| public.job_feedback | Rating after a job | id bigint PK; job_id integer FK jobs cascade (integer vs bigint); user_id FK auth.users cascade; rating CHECK in (0, 5, 10); feedback; created_at | user_id | on; 1: ALL own (authenticated) | 1 | transcription | migrate rows |
| public.feature_flags | Per user flag; only column is runpod text | user_id PK FK users cascade; runpod text | user_id | on; 1: select own | 2 (RunPod provider only) | dead (RunPod dropped) | throw |
| public.provider_health_metrics | Health scoring rows per provider | id bigint PK; provider; is_healthy; success_rate; average_latency; error_rate; consecutive_failures; totals; recorded_at; created_at | none | on; 2: select for authenticated, ALL for service_role | 4 | dead (no health scoring, PRD 2) | throw |
| public.provider_plugin_configs | Plugin framework config per provider | id bigint PK; provider_name UNIQUE; enabled; priority; fallback_chain jsonb; configuration jsonb; health_check_interval; retry_policy jsonb; created_at, updated_at (trigger) | none | on; 2: read authenticated, ALL service_role | 3 (`provider-plugin-system.ts`) | dead (no plugin configs, PRD 2) | throw |
| public.workers | RunPod worker registry | id bigint identity PK; label; available_time; heartbeat; running_job FK jobs set null; started_at | none | on; 0 policies | 0 | dead | throw |
| audit.cleanup_audit_log | One row per retention job run | id bigserial PK; job_name; table_name; retention_days; rows_deleted; execution_time_ms; status CHECK success/failure/partial; error_message; users_opted_out_count; executed_at; executed_by default 'pg_cron' | none | on; 1: select for every authenticated user (`USING true`); no write policy | 0 (4 views over it) | privacy (housekeeping log) | derive: the new retention job logs its own runs; old rows are not needed |

Not in migrations but referenced: `auth.users`, `auth.identities` (GoTrue, kept), `storage.objects` (see storage section), `cron.job` and `cron.job_run_details` (pg_cron).

Data volume hints from the repo (Jan 2026, ADR-033, `frontend/docs/arch42/decisions/ADR-033-database-io-optimization-strategy.md` and the `_backup_orphans` migration header): database 3,634 MB; transcripts table 2,142 MB (about 3.5 million rows, from 163K dead rows being 4.65%); jobs about 137K rows (22K dead rows being 16.11%); indexes 850 MB; packages table scanned 1.67 million times. These are the only size numbers in the repo. Current counts need a snapshot.

### 2c. Enums

11 enum types, all in `public`. Postgres cannot drop enum values, so unused values stay for ever. Source: `20251130113249_remote_schema.sql:91-244` and the later migrations named in the table. Cross check: `frontend/src/types.gen.ts:1830-1925` (generated 2026-10-23, commit 511bf8de) lists the same 11 enums and the same values, so there is no drift between migrations and the generated types.

| Enum | Values | Added in | Used by | Note |
|---|---|---|---|---|
| discount_type | fixed_discount, percentage_discount, product_discount | baseline | coupon.discount_type | Owned by billing (CPN). |
| expiration_policy_type | never, 30_days, 90_days, 1_year, custom | baseline | api_keys.expiration_policy, calculate_expiration_date, create_api_key_with_expiration | Public API. |
| job_status | draft, not_started, pending, running, completed, failed, canceled, deleted, speaker_labeling, uploaded, processing, ready | baseline has 9; `20251223140321` adds uploaded, processing, ready | jobs.job_status | The last three are unused since `20260107013917` (comment says deprecated). 12 values in the enum, 9 in real use. |
| language | Afrikaans, Arabic, Armenian, Azerbaijani, Belarusian, Bosnian, Bulgarian, Catalan, Chinese, Croatian, Czech, Danish, Dutch, English, Estonian, Finnish, French, Galician, German, Greek, Hebrew, Hindi, Hungarian, Icelandic, Indonesian, Italian, Japanese, Kannada, Kazakh, Korean, Latvian, Lithuanian, Macedonian, Malay, Maori, Marathi, Nepali, Norwegian, Persian, Polish, Portuguese, Romanian, Russian, Serbian, Slovak, Slovenian, Spanish, Swahili, Swedish, Tagalog, Tamil, Thai, Turkish, Ukrainian, Urdu, Vietnamese, Welsh | baseline | jobs.language, jobs.translate_to (array), transcripts.language | 57 values. English names, not ISO codes. Adding a language needs an enum migration. |
| payment_status | pending, completed, failed, canceled | baseline | payments.status | The payment trigger only reacts to pending to completed. |
| pricing_plan_interval | day, week, month, year | baseline | prices.interval | Stripe shaped leftover. |
| pricing_type | one_time, recurring | baseline | prices.type | Stripe shaped leftover. |
| storage_provider_type | gcs, cloudflare, external_url | `20251221000000` | jobs.storage_provider (default gcs, NOT NULL) | external_url jobs keep the URL in jobs.callback_url (`20251225212352` comment) or in file_path for YouTube. |
| subscription_status | trialing, active, canceled, incomplete, incomplete_expired, past_due, unpaid | baseline | subscriptions.status | Stripe shaped leftover. |
| transcript_provider | runpod, deepgram, soniox, soniox_v5 | baseline has runpod, deepgram; `20251130114025` soniox; `20260619100001` soniox_v5 | jobs.transcription_provider (default deepgram) | New system keeps Soniox and Deepgram; runpod value stays only for old rows. |
| translation_status | original, translation | `20251130114026` | transcripts.translation_status (default original) | A transcript line is either the source text or a translation into transcripts.language. |

Text columns that behave like enums (CHECK constraints, not Postgres enums) are in the "Rules in DB constraints" table: chat_limits.limit_type, chat_limits.limit_period, chat_messages.role, callback_deliveries.status, ai_analysis.status, audit.cleanup_audit_log.status, job_feedback.rating.

### 2d. Views and materialized views

50 views, 0 materialized views. Public views are read by the app. The 39 analytics views, 2 accounting views and 4 audit views have no reader in the app (no `.from('analytics...')`, no `.schema('analytics')` anywhere in `frontend/src`); they were made for hand queries in the Supabase dashboard. Source for the analytics set: baseline `:1453-3173` (14 in `analytics`, 26 in the misspelled `anayltics`), `20260222092040` (recreates the 26 into `analytics`, 2 of them replace existing names `daily_summary` and `user_in_numbers`), `20260222092042` and `20260222114054` (add `daily_summary_30d`). 27 of the 39 analytics views (the 26 recreated in `20260222092040` and `daily_summary_30d`) carry an explicit `GRANT SELECT` to `authenticated`; the other 12 have none. All 39 are owned by postgres without `security_invoker`, so they read `auth.users` and all payments as the owner. The schema has no `GRANT USAGE` in any migration, so exposure depends on dashboard settings (Needs mu).

Public and audit views:

| View | One line definition | Readers | Notes |
|---|---|---|---|
| public.user_balance_view | sum of remaining_minutes per user from get_user_packages_with_balance(now in UTC) | `src/dao/Balances.ts:12`; trigger insert_usage_history_on_job_start; pgTAP 050, 020, 080 | security_invoker on, so the caller sees only own rows; this is the "balance per user" the migration tool must match |
| public.user_languages | count of jobs per source language | `src/dao/JobRepository.ts:370` | security_invoker on |
| public.user_translate_to_languages | count of jobs per translation target (unnest of jobs.translate_to), filtered by user_id = auth.uid() (`20251130114027`) | `JobRepository.ts:389`, `TranslationPreferenceService.ts:18` | Not security_invoker; the filter is inside the view. Granted to authenticated and service_role (plus default grants) |
| public.invoice_requests | payments completed in a hard coded week (2025-03-07 to 2025-03-14) with no Invoices row, grouped per Paraşüt contact and currency | none | security_invoker on. One time Paraşüt batch, dead |
| public.user_opt_out_stats | counts of users with auto_delete_transcripts true and false and percentages | none | `20251222100000`. Not security_invoker, granted to authenticated: any signed in user reads global counts |
| audit.cleanup_metrics_summary | per job and table: runs, rows deleted, success rate | none | granted to authenticated; reads audit.cleanup_audit_log as owner |
| audit.cleanup_job_health | latest run per job with 7 day failures and a health label (CRITICAL, WARNING, DEGRADED, STALE, HEALTHY) | none | same |
| audit.cleanup_daily_stats | per day, job, table over the last 90 days | none | same |
| audit.cleanup_audit_recent | last 100 audit rows | none | same |

Accounting views (owner postgres, no grants in migrations):

| View | One line definition | Readers |
|---|---|---|
| accounting.sales_per_month | monthly completed sales per currency, excluding 4 hard coded internal test accounts (emails in the SQL) | none |
| accounting.usd_sales_per_month | the same in USD using a fixed 36 TRY per USD rate | none |

Analytics views (39). Readers: none in the app for all of them.

| View | One line definition |
|---|---|
| analytics._speaker_daily_job_stats | per day: completed jobs with, without and unknown speaker setting, with percentages |
| analytics.all_jobs | every job with user email and a "has payment" flag |
| analytics.clients | per user email and currency: total completed payments |
| analytics.clients_last_payment | last payment per user |
| analytics.clients_with_transcripts_5 | paying users with payment count, total amount and the text of their latest job's first lines (string_agg over transcripts) |
| analytics.completed_jobs | completed jobs with user email |
| analytics.completed_jobs_by_language | completed jobs per language for paying users |
| analytics.daily_job_duration | per day: summed duration of completed jobs in minutes and hours |
| analytics.daily_job_errors | per day: completed jobs, jobs with error, error rate |
| analytics.daily_jobs_by_languages | per day: completed jobs in columns, one hard coded column per language |
| analytics.daily_jobs_by_languages_with_errors | the same with error columns (590 lines) |
| analytics.daily_jobs_count | per day: completed, draft, failed and total jobs |
| analytics.daily_payments | per day: completed and pending payment counts |
| analytics.daily_summary | per day: users, payments, pending payments, jobs, duration, payment amount (hard coded internal emails excluded) |
| analytics.daily_summary_30d | the same for the last 30 days with CTEs on base tables and a fixed 35 TRY per USD rate (`20260222114054`) |
| analytics.daily_users | per day: signups from auth.users |
| analytics.draft_jobs | draft jobs with user email |
| analytics.failed_jobs | failed jobs (all job columns) |
| analytics.failed_jobs_ | failed jobs joined to user email |
| analytics.free_user_completed_jobs_by_language | jobs per language created after 2025-06-30 |
| analytics.hanging_jobs | jobs stuck in not_started, pending or running (all job columns) |
| analytics.job_completion | per job: last transcript end time against duration |
| analytics.lines_without_speakers | transcript lines with no speaker for recent jobs |
| analytics.monthly_clients | per month: distinct paying users |
| analytics.new_users | users who got a freemium package in the last day |
| analytics.null_speaker_ratio | share of transcript lines with null speaker since 2025-06-21 |
| analytics.null_speaker_ratio_by_language | the same per language |
| analytics.pending_jobs | pending jobs with user email |
| analytics.total_revenue | sum of completed payment amounts (all currencies added together) |
| analytics.transcription_hours_by_payment_status | per day: hours and jobs for paying and non paying users |
| analytics.user_analytics | per user: completed and draft jobs and payments (long CTE) |
| analytics.user_in_numbers | per user: first and last job date, job count, minutes, hours |
| analytics.user_payments | completed payments joined to auth.users (email, token, metadata) |
| analytics.user_payments_pending | pending payments with email, excluding hard coded internal accounts |
| analytics.user_weekly_payments | per week: summed payment amount |
| analytics.users_ | users excluding hard coded internal emails |
| analytics.users_index | numbered list of users by signup date |
| analytics.weekly_client_payments_count | per week: distinct paying users |
| analytics.weekly_users | per week: signups excluding internal emails |

### 2e. Functions

52 routines in `public`: 51 functions (43 callable plus 8 trigger functions) and 1 procedure, 50 distinct names (`deduct_minutes` has 3 overloads). Final bodies come from the last migration that replaced each one (named in the Source column). No function pins `search_path`; 17 are SECURITY DEFINER.

Grants: every function and the procedure is executable by `anon`, `authenticated` and `service_role`. The 30 baseline routines carry explicit `GRANT ALL` lines (baseline `:4756-5724`) and every later function gets the same through default privileges for the postgres role (`:5725-5751`). No migration ever revokes a function grant. So "anon: yes" below is true for every row; the column "anon risk" flags the ones that do something harmful when called by anyone holding the public anon key through `/rest/v1/rpc/<name>`. Trigger functions cannot be called through rpc.

Class: decides = holds a business rule; computes = pure read or arithmetic; refuses = raises on a bad state; housekeeps = deletes, mirrors or logs.

| Function | Args | Purpose | Class | Caller | Definer | anon risk | Source |
|---|---|---|---|---|---|---|---|
| anonymize_user_data | () trigger | On auth.users delete: set api_keys.user_id and api_key_audit_logs.user_id to the all zero uuid | housekeeps | trigger on_user_deleted | no | none (trigger) | baseline `:247`. Likely a no-op: the FK cascade and set null run first, and the zero uuid is not in auth.users |
| calculate_expiration_date | (policy, custom_days) | Expiry timestamp from an API key policy (30/90 days, 1 year, custom days) | computes | create_api_key_with_expiration | no | none | baseline `:267`. Marked IMMUTABLE but calls NOW() |
| check_rate_limit | (api_key_id, user_id, per_minute 60, per_hour 1000) | Read counters, upsert minute and hour counters, return allowed and remaining | decides | rpc `src/app/api/v1-beta/transcriptions/auth.ts:119` | yes | writes counter rows for any key and user id | baseline `:296`. ON CONFLICT (api_key_id, window_start) has no matching unique index, so it errors (per the migrations); the caller only logs the error (`auth.ts:129`) |
| cleanup_old_audit_logs | () | Delete api_key_audit_logs older than 90 days | housekeeps | nobody | yes | deletes audit rows | baseline `:351` |
| cleanup_old_rate_limits | () | Delete api_key_rate_limits older than 24 hours | housekeeps | nobody | yes | deletes counters | baseline `:369` |
| cleanup_old_callback_deliveries | () | Delete callback_deliveries older than 30 days | housekeeps | nobody | yes | deletes delivery log | `20251130114026` |
| create_api_key_with_expiration | (name, scopes, policy, custom_days, metadata) | Make key 'sk_' + base64(32 random bytes), bcrypt hash with pgcrypto, insert api_keys row, audit row, webhook row; return the plain key once | decides | rpc `src/pages/dashboard/api-keys.tsx:179` | yes | fails for anon (auth.uid() is null) | baseline `:387` |
| deduct_minutes | (user_id, minutes, time) | Old 3 argument form: refuses when balance would go below zero, else allocates minutes over packages and writes usage_history (no job id) | decides | nobody | no | none beyond RLS | baseline `:464` |
| deduct_minutes | (user_id, minutes, job_id, time) | Same with job id | decides | nobody (superseded) | no | none | baseline `:495`. Overlaps the 5 argument form on default arguments |
| deduct_minutes | (user_id, minutes, job_id, time, skip_balance_check false) | Final form. With skip true: insert the full amount into the earliest ending open package (balance may go negative). With false: refuses if balance too small, else allocates with request_breakdown_over_packages and raises if no row was written | decides | trigger job_status_trigger; rpc `src/utils/deepgram.ts:372`, `src/utils/soniox.ts:261` | no | A caller can deduct for any user id allowed by RLS on usage_history insert (own id) | `20260115162255` |
| deduplicate_transcript_lines | (job_id) | Delete lines of a job with identical text, keep the lowest id | housekeeps | nobody | no | RLS limits it to own rows | baseline `:526` |
| expire_expired_keys | () | Deactivate expired active keys, audit row and webhook row per key | housekeeps | nobody (no cron calls it) | yes | anyone can run it | baseline `:550` |
| get_expiration_policies | () | Fixed list of the 5 expiry options with labels | computes | rpc `api-keys.tsx:116` | no | none | baseline `:598` |
| get_next_message_order | (job_id) | max(message_order)+1 for chat_messages | computes | rpc `src/features/aiAnalysis/services/ChatMessageRepository.ts:37` | no | none | baseline `:639`. Not locked; UNIQUE(job_id, message_order) is the guard |
| get_or_create_chat_limit | (user_id, job_id, type, period, max_limit) | Find or create the counter, reset it when the period has passed, report exceeded | decides | rpc x8: `src/pages/api/ai/chat-limits.ts:54,72,92,110`, `src/lib/ai/chat-validator.ts:94,125,153,178` | no | writes limits for any user id that RLS allows (own) | baseline `:656` |
| get_user_balance | (time) | Sum of remaining minutes per user over open packages | computes | deduct_minutes; pgTAP | no | none | `20251230180000`. The comment says it returns 0 for users without packages, but with no open package there is no row at all |
| get_user_packages_with_balance | (time) | Packages open at the time (start_at <= time <= end_at) with consumed and remaining minutes | computes | user_balance_view, get_user_balance, request_breakdown_over_packages | no | none | baseline `:748` |
| handle_new_user | () trigger | Insert public.users from auth.users (id, email, full_name, avatar_url) | housekeeps | trigger on_auth_user_created | yes | none (trigger) | baseline `:784` |
| handle_update_user | () trigger | Copy email, full_name, avatar_url to public.users on auth.users update | housekeeps | trigger on_auth_user_updated | yes | none (trigger) | baseline `:798` |
| increment_chat_limit | (user_id, job_id, type, period, increment 1) | Add to the counter | decides | rpc x4: `chat-validator.ts:236,249,262,275` | no | owner can call it with a negative increment | baseline `:816` |
| insert_freemium_package | () trigger | Insert package 'freemium', 20 minutes, ends now + 2 months | decides | trigger insert_freemium_package_trigger (AFTER INSERT on public.users) | no | none (trigger) | baseline `:839` |
| insert_new_packages_on_payment_completed | () trigger | When payment goes pending to completed: read metadata.package (interval, frequency, count, expiry, name, minutes) and insert `count` packages, each starting one frequency later, end = start + expiry; raise on an interval other than month or year | decides | trigger payment_status_trigger | no | none (trigger) | baseline `:853` |
| insert_usage_history_on_job_start | () trigger | On job_status draft, ready or uploaded to not_started (or insert as not_started): skip when duration is null (deferred API billing); freemium only user and no completed job: deduct with skip_balance_check; other freemium: balance check then deduct; paying users: balance check then deduct. Raises 'No available package' and 'Not enough balance' | decides, refuses | trigger job_status_trigger (AFTER UPDATE on jobs) | no | none (trigger) | `20260408120000` (the 13th definition: baseline plus 12 rewrites, see history). The INSERT branch is dead because the trigger is AFTER UPDATE only |
| log_api_request | (api_key_id, user_id, action, endpoint, method, ip, user_agent, status, duration_ms, details) | Insert an api_key_audit_logs row, return its id | housekeeps | rpc `auth.ts:155` | yes | anyone can forge audit rows for any key or user | baseline `:954` |
| process_payment | (token) | Old balance based payment completion; reads table `balances` that does not exist | dead | nobody | no | none | baseline `:996`, restated `20250308143806`. Always fails at the missing table |
| request_breakdown_over_packages | (requested_minutes, time) | Split a request over open packages ordered by end date, earliest first; returns minutes per package and carry over | decides | deduct_minutes; pgTAP | no | none | `20251230190000` (original in baseline `:1050`, fix attempted in `20251230160002`, reverted by `20251230170000`, re-applied) |
| revoke_api_key | (key_id) | Set is_active false and write an audit row. No owner check | decides | rpc `api-keys.tsx:214` | yes | anyone who knows a key id can revoke the key | baseline `:1088` |
| send_webhook_notification | (user_id, event_type, payload) | Insert a pending webhook_notifications row | housekeeps | rpc `auth.ts:177`; create_api_key_with_expiration; expire_expired_keys | yes | anyone can queue a row for any user id | baseline `:1112` |
| transfer_job_by_email | PROCEDURE (job_id, to_email) | Move a job and its transcripts to another user found by email | decides | nobody (manual support SQL) | no | not reachable through rpc (procedure) | baseline `:1142` |
| update_updated_at_column | () trigger | Set NEW.updated_at = now() | housekeeps | triggers on ai_consent, highlights, summaries, jobs | no | none | baseline `:1174`, redefined `20251223140321` |
| update_provider_plugin_configs_updated_at | () trigger | Same for provider_plugin_configs | housekeeps | trigger trigger_update_provider_plugin_configs_updated_at | no | none | `20251130114026` |
| validate_api_key | (key) | Wrapper around verify_api_key that adds the key name | computes | nobody (types only) | yes | key guessing oracle | baseline `:1187` |
| verify_api_key | (key, required_scope) | Find the active key whose bcrypt hash matches (scans all active keys), check expiry and scope, update last_used_at, write an audit row | decides, refuses | rpc `auth.ts:73` | yes | key guessing oracle with a bcrypt scan per call; writes audit and last_used_at | baseline `:1211` |
| cleanup_old_transcripts | (retention_days 31) | Delete transcript lines of completed jobs older than retention + 1 day for users with auto_delete_transcripts true (null counts as false). Refuses a retention outside 7 to 365 or more than 100,000 rows. Advisory lock. Writes audit.cleanup_audit_log | decides, housekeeps | pg_cron `cleanup-old-transcripts` | yes | anyone can delete old lines of every opted in user | `20251222100001` |
| cleanup_old_health_metrics | (retention_days 30) | Same pattern for provider_health_metrics.recorded_at | housekeeps | pg_cron `cleanup-old-health-metrics` | yes | anyone can run | `20251222100001` |
| cleanup_dry_run | (retention_days 30) | Report counts that the two jobs above would delete | computes | nobody | yes | read only, leaks counts | `20251222000002` |
| cleanup_cron_job_history | (retention_days 90) | Delete cron.job_run_details older than retention | housekeeps | pg_cron `cleanup-cron-job-history` | yes | anyone can run | `20251222100001` |

The 15 test functions in `public` (never dropped; all executable by anon; types.gen lists them): `run_cleanup_test_suite`, `test_create_user`, `test_create_job`, `test_create_transcript`, `test_create_health_metric`, `test_cleanup_test_data`, `test_cleanup_respects_opt_out`, `test_cleanup_health_metrics`, `test_dry_run_accuracy` (from `20251221200008`), and `test_boundary_condition_exact_30_days`, `test_null_opt_out_preference`, `test_parameter_validation`, `test_safety_threshold_documentation`, `test_concurrent_execution_documentation`, `test_dry_run_uses_delete_logic` (from `20251222000003`, which also redefines `run_cleanup_test_suite`). The helper `test_cleanup_test_data()` deletes rows with fixed marker values from three tables as the caller.

Supabase side function used by triggers: `supabase_functions.http_request(url, method, headers, params, timeout)` (pg_net), not part of the app schema.

Count check: 29 baseline functions (28 names, `deduct_minutes` twice) + 1 baseline procedure + 2 functions from `20251130114026` + 4 cleanup functions + 15 test functions + 1 new `deduct_minutes` overload = 52 routines, of which 8 are trigger functions and 1 is the procedure.

### 2f. Triggers

14 triggers in the final state: 11 on `public` tables and 3 on `auth.users`. The three `auth.users` triggers are created in the baseline (`:5797-5801`) and again in `supabase/seed.sql:1-11` (drop and recreate, for local resets). Removed on the way: `insert_usage_history_trigger` (made in `20251230160001`, dropped in `20251230160002`/`20251230170000`), `update_translation_beta_consent_updated_at` (dropped `20251218000000`), `job_uploaded_webhook` (drop in `20260105120000`, never created by these migrations).

| # | Table | Event | Function | Purpose | Class | Source |
|---|---|---|---|---|---|---|
| 1 | auth.users | AFTER INSERT | public.handle_new_user() | Mirror the new account into public.users | housekeeping (user mirroring) | baseline `:5797` |
| 2 | auth.users | AFTER UPDATE | public.handle_update_user() | Copy email, full_name, avatar_url on change | housekeeping | baseline `:5799` |
| 3 | auth.users | AFTER DELETE | public.anonymize_user_data() | Point api_keys and api_key_audit_logs at the zero uuid | housekeeping, likely a no-op | baseline `:5801` |
| 4 | public.users | AFTER INSERT | public.insert_freemium_package() | Give the 'freemium' package: 20 minutes, 2 months | business rule in a trigger | baseline `:4214` |
| 5 | public.users | AFTER INSERT | supabase_functions.http_request(.../api/hooks/user-inserted) | Call the production site, which sends the welcome email (`src/pages/api/hooks/user-inserted.ts`) | business effect in a trigger | baseline `:4242` (name `welcome`) |
| 6 | public.payments | AFTER UPDATE | public.insert_new_packages_on_payment_completed() | pending to completed: create the purchased packages | business rule in a trigger | baseline `:4222` (name `payment_status_trigger`) |
| 7 | public.payments | AFTER UPDATE | supabase_functions.http_request(.../api/hooks/payment-updated) | Call the production site for the purchase email and the admin sale email (`payment-updated.ts`) | business effect in a trigger | baseline `:4226` (name `purchased`) |
| 8 | public.jobs | AFTER UPDATE | public.insert_usage_history_on_job_start() | Charge minutes when a job starts | business rule in a trigger | baseline `:4218` (name `job_status_trigger`), restored `20251230170000` |
| 9 | public.jobs | AFTER UPDATE | supabase_functions.http_request(.../api/hooks/job-updated) | Call the production site on every job update; the handler sends failed job emails and an admin mail for completed jobs with an error (`job-updated.ts:43-53`) | business effect in a trigger | baseline `:4210` (name `failed_job`) |
| 10 | public.jobs | BEFORE UPDATE | public.update_updated_at_column() | Keep jobs.updated_at | updated_at | `20251223140321` (name `update_jobs_updated_at`) |
| 11 | public.ai_consent | BEFORE UPDATE | public.update_updated_at_column() | Keep updated_at | updated_at | baseline `:4230` |
| 12 | public.highlights | BEFORE UPDATE | public.update_updated_at_column() | Keep updated_at | updated_at | baseline `:4234` |
| 13 | public.summaries | BEFORE UPDATE | public.update_updated_at_column() | Keep updated_at | updated_at | baseline `:4238` |
| 14 | public.provider_plugin_configs | BEFORE UPDATE | public.update_provider_plugin_configs_updated_at() | Keep updated_at | updated_at | `20251130114026` (name `trigger_update_provider_plugin_configs_updated_at`) |

Order and interaction facts:

- Triggers 5, 7 and 9 post to a hard coded production URL (`https://transgate.ai/api/hooks/...`) with a shared secret written in the migration text, a 1000 ms timeout and no retry (ADR-001 context: fire and forget; ADR-002 notes the hard coded URL and secret). A staging or local database therefore calls production. The secret value is in the baseline at `:4210`, `:4226` and `:4242`; it is not repeated in this document and must be rotated before the file is shared.
- Trigger 9 fires on every UPDATE of `jobs`, including retries, callbacks and soft deletes. The handler returns "No action needed" for most calls (`job-updated.ts:53`).
- Trigger 8 only reacts to `draft` (or `ready`, `uploaded`) to `not_started`. The database does not stop a job from entering `not_started` a second time (for example after going back to `draft`); usage_history has no unique key on job_id, so a second entry would charge again. Whether the app ever does that is not shown by the code read for this topic.
- On signup the chain is: GoTrue inserts `auth.users`, then trigger 1 inserts `public.users`, then triggers 4 and 5 fire on that insert.

### 2g. pg_cron jobs

3 jobs, all created in `20251221200006_schedule_cleanup_jobs.sql` (ADR-022). All run SQL as the postgres role, UTC. Nothing else is scheduled in the database: no job calls `expire_expired_keys`, `cleanup_old_audit_logs`, `cleanup_old_rate_limits` or `cleanup_old_callback_deliveries`, so those tables grow without limit. The other two schedulers (node-cron in `src/scheduler/index.ts` for the Deepgram queue and Soniox recovery, and `vercel.json` crons) live in the app, not here.

| Job name | Schedule | Command | What it does |
|---|---|---|---|
| cleanup-old-transcripts | `0 2 * * *` (daily 02:00 UTC) | `SELECT * FROM public.cleanup_old_transcripts(30)` | Deletes transcript lines (not jobs) of completed jobs created more than 31 days ago (30 days plus 1) for users whose auto_delete_transcripts is true. At most 100,000 rows per run; above that the run aborts and logs |
| cleanup-old-health-metrics | `0 3 * * *` (daily 03:00 UTC) | `SELECT * FROM public.cleanup_old_health_metrics(30)` | Deletes provider_health_metrics older than 31 days |
| cleanup-cron-job-history | `0 4 * * 0` (Sundays 04:00 UTC) | `SELECT * FROM public.cleanup_cron_job_history(90)` | Deletes cron.job_run_details older than 91 days |

Facts to carry: the job rows are rows in `cron.job` of the production database, so they exist only if the migration ran there; a restored dump without the `cron` schema loses them. The 30 day rule is applied twice in different places: this job deletes lines, and the job list query hides jobs older than a month unless `users.unlimited_history` is true (`src/pages/home/[[...jobId]].tsx:74`, `JobListContainer.tsx:86`). The privacy topic owns both rules.

### 2h. Storage buckets and policies

Buckets: no migration creates a bucket. The only trace is a bucket named `jobs` in four policy definitions on `storage.objects` (baseline `:5802-5839`). `supabase/config.toml` has no bucket block (the example is commented out, `:88`). No application code calls `supabase.storage` (the only hit is a test mock), and audio lives in Google Cloud Storage or Cloudflare R2 behind the `StorageProvider` layer (`src/services/storage`, ADR-021, `jobs.storage_provider`). So the Supabase bucket `jobs` is probably an unused leftover; its existence and contents need a look in the dashboard.

| Policy (on storage.objects) | Command | Role | Rule |
|---|---|---|---|
| Give users access to own folder 1y1cm_0 | SELECT | public | bucket_id = 'jobs' and auth.uid() = first folder name of the object path |
| Give users access to own folder 1y1cm_1 | INSERT | public | same, WITH CHECK |
| Give users access to own folder 1y1cm_2 | UPDATE | public | same |
| Give users access to own folder 1y1cm_3 | DELETE | public | same |

Upload limit: 50 MiB locally (`config.toml:83`). The 300 second request cap and presigned URL flow are in the jobs topic. For account deletion the storage objects are not in the database: `anonymize_user_data` does not touch them.

### 2i. Sensitive grants

The model is "grant everything to everyone, let RLS decide". That is Supabase's default and it was never tightened. Items marked HIGH can be abused with only the public anon key or a normal signed in account. They are described as read from the SQL; none was run.

| # | Grant or exposure | Roles | Objects | Why it matters | Source | Level |
|---|---|---|---|---|---|---|
| 1 | GRANT ALL on every function and the procedure | anon, authenticated, service_role | 30 baseline routines explicitly; every later function through default privileges | Anyone with the anon key can call, through `/rest/v1/rpc/`, the 15 callable SECURITY DEFINER functions (17 in all, minus the two mirror trigger functions). Worst cases: `cleanup_old_transcripts(7)` deletes old transcript lines of every opted in user (up to 100,000 per call; default is opted in); `revoke_api_key(uuid)` has no owner check; `verify_api_key` and `validate_api_key` work as a key guessing oracle and each call scans every active key with bcrypt; `log_api_request` and `send_webhook_notification` accept any user id | baseline `:4756-5724`, `:5725-5751`; no REVOKE in any migration | HIGH |
| 2 | Table privileges SELECT, INSERT, UPDATE, DELETE, TRUNCATE, TRIGGER, REFERENCES | anon, authenticated, service_role | 27 tables and 3 views in `public` (baseline) plus `callback_deliveries`, `provider_plugin_configs` (explicit) and every later table by default | RLS is the only barrier. TRUNCATE and TRIGGER are not row security checked; REST does not expose them but the grant is wider than any need | baseline `:4756-5724`, `20251130114026` | MEDIUM |
| 3 | RLS lets the owner write ledger and state columns | authenticated | `usage_history` (INSERT own rows with any minutes, including negative), `jobs` (ALL own: duration, job_status, provider, error), `users` (UPDATE own: blocked, unlimited_history, auto_delete_transcripts), `chat_limits` (UPDATE own counters), `api_key_rate_limits` (INSERT, UPDATE own), `api_keys` (INSERT, UPDATE own: scopes, hash, expiry), `transcripts` (ALL own) | The billing ledger and the checks that protect it are client writable | baseline `:4455`, `:4463`, `:4479`, `:4586`, `:4570-4602`, `:4530-4582` | HIGH |
| 4 | Read for everyone, anon included | public role | `coupon` (all rows, `USING true`), `prices`, `products` | Every coupon code and its discount can be listed by anyone | baseline `:4439-4443`, `:4515` | MEDIUM |
| 5 | SELECT on 27 of the 39 analytics views | authenticated | `analytics.*` views over `auth.users`, `payments`, `jobs`; owner postgres, no security_invoker | If the `analytics` schema is exposed or granted USAGE (not in migrations), any signed in user reads all emails, payment tokens and metadata. `analytics.user_payments` includes `token` and `metadata` | `20260222092040` | HIGH if exposed, unknown |
| 6 | USAGE on schema audit; SELECT on 4 audit views and `audit.cleanup_audit_log` | authenticated | audit schema | Any signed in user can read retention job logs; policy is `USING (true)` and a comment says "restrict to admin roles in production" | `20251222100000` | LOW |
| 7 | SELECT on `public.user_opt_out_stats` | authenticated | view | Global opt out counts visible to any signed in user (view runs as owner) | `20251222100000` | LOW |
| 8 | Webhook secret and production URL inside trigger definitions | any role that can read `pg_trigger` or the migrations | triggers `welcome`, `purchased`, `failed_job` | The shared secret that authenticates the hook handlers is in git history and in the database catalog | baseline `:4210`, `:4226`, `:4242` | HIGH (rotate) |
| 9 | Revokes done | anon, authenticated, service_role | `webhook_payloads` (dropped later), `translation_beta_consent_archived` (all revoked, then SELECT for service_role) | These two are the only tightened objects in 53 migrations | `20251130114026`, `20251218000000` | info |
| 10 | Policies for service_role only | service_role | `usage_history` UPDATE, `callback_deliveries` ALL, `provider_health_metrics` ALL, `provider_plugin_configs` ALL | Fine; the service role bypasses RLS anyway | baseline `:4713`, `20251130114026`, `20251130114027` | info |
| 11 | Tables with RLS on and no policy | all but the service role are blocked | `accounting.Invoices`, `public.Invoices`, `generic_contacts`, `workers` | Effective deny, good | baseline `:4436`, `:4527`, `:4677`, `:4729` | info |
| 12 | Realtime publication | per RLS | `jobs`, `transcripts` in `supabase_realtime` | Browser change feed for job status and lines; the new system needs its own push | baseline `:4741-4745` | info |
| 13 | Storage policies for bucket `jobs` | public role, effectively authenticated owner | `storage.objects` | Own folder only; bucket probably unused | baseline `:5802-5839` | LOW |

### 2j. Migration history

53 migrations in `frontend/supabase/migrations/` (no down migrations; applied to production by `supabase db push` in `.github/workflows/cd.yml:26`), plus the folder `_backup_orphans/` with 9 files that are not applied. Flags: T = ships test code, D = data fix or data move, X = destructive, R = rewrites an earlier migration's object (fix-the-fix).

| # | Migration | What it does | Flags |
|---|---|---|---|
| 1 | 20250308143806_billing-info | Adds address, company_name, country, full_name, tax_id to billing_info; restates process_payment; adds 2 billing_info policies. Sorts before the baseline although it touches tables the baseline creates | R |
| 2 | 20251130113249_remote_schema | Squashed baseline of production on 2025-11-30: 3 new schemas (accounting, analytics, anayltics), 7 extensions, 9 enums, 28 tables, 29 functions, 1 procedure, 45 views (14 in analytics, 26 in anayltics, 2 in accounting, 3 in public), 12 triggers (9 on public tables, 3 on auth.users), RLS and policies, grants, storage policies | baseline |
| 3 | 20251130113250_restore_search_path | `SET search_path = public, extensions` because the baseline clears it | |
| 4 | 20251130114025_add_soniox_provider | Enum value soniox (own migration, enum rule) | |
| 5 | 20251130114026_new_changes | translation_status enum; drops webhook_payloads; creates callback_deliveries and provider_plugin_configs; adds jobs.translate_to, transcripts.language and translation_status; indexes; view user_translate_to_languages; 2 functions; also overwrites insert_usage_history_on_job_start with an older body that has no freemium bypass and reads user_balance_view with no user filter (it looks like a schema diff artifact) | R, X |
| 6 | 20251130114027_add_rls_callback_deliveries_and_fix_view | RLS and 2 policies on callback_deliveries; filters user_translate_to_languages by auth.uid() | R |
| 7 | 20251208095855_add_translation_beta_consent | translation_beta_consent table, policies, updated_at trigger | |
| 8 | 20251218000000_archive_translation_beta_consent | Renames it to _archived, drops trigger, policies, function, revokes grants | X |
| 9 | 20251221000000_add_storage_provider_column | Enum storage_provider_type and jobs.storage_provider | |
| 10 | 20251221200001_add_user_cleanup_preferences | users.auto_delete_transcripts default true, partial index | |
| 11 | 20251221200002_create_cleanup_audit_log | public.cleanup_audit_log with a status CHECK of success, failure, partial | |
| 12 | 20251221200003_add_cleanup_indexes | Indexes on jobs, transcripts, provider_health_metrics for cleanup | |
| 13 | 20251221200004_create_cleanup_functions | cleanup_old_transcripts, cleanup_old_health_metrics, cleanup_dry_run, cleanup_cron_job_history (v1) | |
| 14 | 20251221200005_enable_pg_cron | CREATE EXTENSION pg_cron, EXECUTE grants to postgres | |
| 15 | 20251221200006_schedule_cleanup_jobs | 3 cron.schedule calls | |
| 16 | 20251221200007_create_monitoring_views | 5 monitoring views in public, SELECT to authenticated | |
| 17 | 20251221200008_cleanup_test_suite | 9 test functions in public | T |
| 18 | 20251222000000_cleanup_orphaned_transcripts | Deletes transcripts whose job does not exist, then verifies zero left | D, X |
| 19 | 20251222000001_add_transcripts_fk_constraint | transcripts.job_id FK to jobs ON DELETE RESTRICT | |
| 20 | 20251222000002_fix_cleanup_functions_safety | Rewrites the 4 cleanup functions: boundary fix, parameter limits 7 to 365, 100,000 row threshold, advisory lock | R |
| 21 | 20251222000003_comprehensive_test_suite | 6 more test functions, redefines run_cleanup_test_suite | T, R |
| 22 | 20251222100000_move_cleanup_audit_to_audit_schema | Creates schema audit, DROPs public.cleanup_audit_log CASCADE and recreates it in audit (old rows lost), 4 audit views, drops 5 public views and makes user_opt_out_stats again | R, X |
| 23 | 20251222100001_update_cleanup_functions_audit_schema | Recreates 3 cleanup functions to write to audit (cleanup_dry_run is not touched) | R |
| 24 | 20251223140321_add_job_statuses_and_columns | job_status +uploaded, processing, ready; jobs.updated_at, is_partial, original_duration, processed_duration, resumable, original_audio_url; update_jobs_updated_at trigger; redefines update_updated_at_column | |
| 25 | 20251224151735_add_resume_tracking | jobs.resumed_at; trigger function resume path | R |
| 26 | 20251225212352_remove_audio_url_columns | Drops jobs.audio_url and original_audio_url (URLs computed on demand, ADR-025) | X |
| 27 | 20251227094010_bill108_resume_usage_calculation | Trigger function: charge original minus processed duration on resume | R |
| 28 | 20251228145206_bill109_resume_detection | Trigger function: detect resume by resumed_at | R |
| 29 | 20251230160000_restore_balance_check_enforcement | Trigger function: always check balance, no freemium bypass; get_user_balance with COALESCE | R |
| 30 | 20251230160001_add_usage_history_trigger | Creates insert_usage_history_trigger (a second trigger on the same function) | R |
| 31 | 20251230160002_fix_request_breakdown_user_id | Fixes the user_id partition in request_breakdown_over_packages; drops job_status_trigger | R |
| 32 | 20251230170000_revert_to_original_schema | Reverts 29 to 31 to "buggy but working": old request_breakdown, old trigger function, old trigger name | R |
| 33 | 20251230180000_restore_usage_history_trigger | Trigger function with always check balance again; get_user_balance | R |
| 34 | 20251230190000_fix_request_breakdown_user_id_again | The 31 fix of request_breakdown again | R |
| 35 | 20260101143000_fix_freemium_duration_check | Trigger function charges processed_duration for partial jobs | R |
| 36 | 20260105120000_cleanup_webhook_infrastructure | Drops trigger job_uploaded_webhook, function notify_job_uploaded, index idx_jobs_uploaded_processing (none created by these migrations) | X |
| 37 | 20260107013917_revert_freemium_to_simple | Resets all partial jobs (UPDATE jobs), comments 3 deprecated enum values, restores the freemium bypass trigger function | D, R |
| 38 | 20260108000000_drop_partial_columns | Drops is_partial, original_duration, processed_duration, resumable, resumed_at | X |
| 39 | 20260108130000_fix_freemium_bypass_in_deduct_minutes | New 5 argument deduct_minutes with skip_balance_check; trigger function uses it | R |
| 40 | 20260115162255_fix_freemium_first_job_bypass | deduct_minutes inserts directly when skipping; trigger counts completed jobs: only the first job of a freemium only user bypasses | R |
| 41 | 20260222092040_migrate_anayltics_to_analytics | Recreates the 26 views of the misspelled schema in analytics, grants SELECT to authenticated | R |
| 42 | 20260222092041_drop_anayltics_schema | DROP SCHEMA anayltics CASCADE | X |
| 43 | 20260222092042_optimize_analytics_daily_summary | New view daily_summary_30d | |
| 44 | 20260222114054_fix_daily_summary_30d_full_table_scans | Rewrites it with date bounded CTEs | R |
| 45 | 20260305120001_add_ai_analysis_table | Retroactive: ai_analysis was created in production outside migrations | |
| 46 | 20260305120002_add_ai_analysis_type_column | analysis_type, analysis_data | |
| 47 | 20260319100001_add_ai_analysis_status | status with CHECK | |
| 48 | 20260408120000_fix_null_duration_api_job_trigger | Trigger function returns early on NULL duration (deferred API billing) | R |
| 49 | 20260410120000_add_youtube_columns | jobs.youtube_video_id, youtube_url, transcript_source; UPDATE copies them out of whisper_params | D |
| 50 | 20260619100001_add_soniox_v5_provider | Enum value soniox_v5 | |
| 51 | 20260707120000_add_unlimited_history | users.unlimited_history | |
| 52 | 20260922120000_add_soniox_recovery_columns | jobs.soniox_transcription_id, recovery_attempts NOT NULL default 0 | |
| 53 | 20261022120000_add_iyzico_payment_token_unique_index | Partial unique index payments(token) where gateway is iyzico and token is not null (duplicate callbacks) | |

`_backup_orphans/` (not applied): `20251221_01` to `20251221_08` are the cleanup migrations 10 to 17 under short names Supabase could not order (04 differs only in the default retention 30 against 31; 06 adds a COMMENT ON SCHEMA cron). `20260116170000_optimize_database_io_performance.sql` (ADR-033) adds `idx_packages_user_id`, drops `idx_transcripts_job_id`, `idx_transcripts_job_language`, `idx_transcripts_created` and runs two VACUUMs (which cannot run inside a migration transaction). Whether it was run by hand in production is not recorded. The baseline has no index on packages.user_id.

Fix-the-fix chains:

1. 2025-12-30 (migrations 29 to 34, six files in one day). 29 removed the freemium bypass and made the balance check universal. It forgot the trigger, so the function never ran (30 added a second trigger, which then ran the function twice with the old trigger). 31 fixed a window partition bug in request_breakdown_over_packages (the request row had no user_id, so request_minutes came out 0 and no usage row was written) and dropped the duplicate trigger. 32 reverted all of it ("buggy but working"). 33 and 34 re-applied the function and the partition fix. End state: trigger job_status_trigger (AFTER UPDATE on jobs), fixed request_breakdown, universal balance check, and a balance read with `WHERE user_id = NEW.user_id` (the body left by migration 5 read `user_balance_view` with no user filter, which is only correct when RLS limits the view to the caller).
2. Freemium and resume, 2025-12-23 to 2026-01-15 (migrations 24 to 28, 35 to 40). A "partial transcription with resume" feature (BILL-107 to 111) added 3 statuses, 6 columns and a webhook trigger, then was backed out on 2026-01-07 with a data reset (migration 37) and a column drop (38). The three status values stay in the enum for good. The freemium rule then moved from the application to the trigger again and changed four more times: bypass for freemium only users (37), explicit skip_balance_check (39), first job only (40), null duration skip (48). End state: the function in migration 48.
3. Analytics, 2026-02-22 (migrations 41 to 44). A misspelled schema `anayltics` was rebuilt as `analytics` and dropped with CASCADE. `daily_summary_30d` was written twice because the first version scanned whole tables. End state: 39 views in analytics.
4. Cleanup, 2025-12-21 to 2025-12-22 (migrations 10 to 23). Built, hardened, moved to a new schema and rebuilt within two days, with a test suite stored as migrations. The audit table CHECK was never widened for the statuses `skipped` and `aborted` that the hardened functions write.

Tests and data inside migrations: test code in 17 and 21 (15 functions that remain in production); data changes in 18 (delete orphan transcripts), 37 (reset partial jobs), 49 (copy YouTube fields); `seed.sql` and `seed-ai-insights.sql` are local only.

### 2k. Rules in DB constraints

No ledger ids here; the owner topic turns a row into a ledger row if it is a real rule. Constraints on tables marked dead in the table inventory are listed so nothing is lost. Primary keys are left out unless they encode a rule. Owner prefix is a suggestion from the README prefix list.

CHECK constraints

| Table.column | Rule | Source | Owner |
|---|---|---|---|
| chat_limits.limit_period | Period is one of daily, weekly, monthly, transcript | baseline `:3322`, redone in `20251130114026` | AI |
| chat_limits.limit_type | Type is one of questions_per_transcript, questions_per_user, tokens_per_transcript, tokens_per_user | same | AI |
| chat_messages.role | Role is user, assistant or system | baseline `:3356`, redone in `20251130114026` | AI |
| ai_analysis.status | Status is pending, partial, interrupted or completed; old rows default to completed | `20260319100001` | AI |
| job_feedback.rating | Rating is 0, 5 or 10 | baseline `:3464` | JOB |
| callback_deliveries.status | Status is pending, success or failed | `20251130114026` | API |
| prices.currency | Currency code has 3 characters (table is dead) | baseline `:3541` | CPN (dead) |
| audit.cleanup_audit_log.status | Status is success, failure or partial. The hardened functions write skipped and aborted, which this CHECK rejects | `20251222100000` | PRV |

Unique keys and unique indexes

| Table | Key | Rule | Source | Owner |
|---|---|---|---|---|
| payments | partial unique index `payments_iyzico_token_key` on (token) where payment_gateway = 'iyzico' and token is not null | A repeated Iyzico callback cannot create a second payment, so it cannot create a second set of packages | `20261022120000` | BIL |
| ai_consent | UNIQUE(user_id) | One consent record per user | baseline `:3868` | AI |
| translation_beta_consent_archived | UNIQUE(user_id) | Same, historic | `20251208095855` | PRV |
| api_keys | UNIQUE(name, user_id) | Key names are unique per user | baseline `:3948` | API |
| chat_limits | UNIQUE(user_id, job_id, limit_type, limit_period) | One counter per user, job, type and period. NULL job ids do not collide in Postgres, so the user level counters can repeat | baseline `:3898` | AI |
| chat_messages | UNIQUE(job_id, message_order) | Messages of a job have unique order numbers; guards the unlocked max+1 | baseline `:3903` | AI |
| highlights | UNIQUE(job_id) | One highlights row per job | baseline `:3928` | AI |
| summaries | UNIQUE(job_id, summary_type) | One summary per job and type | baseline `:3983` | AI |
| ai_analysis | UNIQUE(job_id, user_id) | One analysis per job and user | `20260305120001` | AI |
| provider_plugin_configs | UNIQUE(provider_name) | One config per provider (dead) | `20251130114026` | TRX (dead) |
| feature_flags | PK(user_id) | One flag row per user (dead) | baseline | FLG (dead) |

Missing uniqueness worth noting (rules the code relies on but the database does not enforce): usage_history has no unique key on (job_id, package_id) so a job can be charged twice; api_key_rate_limits has no unique key on (api_key_id, window_start) although check_rate_limit uses ON CONFLICT on it; packages has no unique key on (payment_id, start_at); users.email is only indexed.

NOT NULL and defaults that encode a rule

| Table.column | Rule | Source | Owner |
|---|---|---|---|
| users.auto_delete_transcripts | NOT NULL default true: retention deletion is opt out; every account is included until it switches off | `20251221200001` | PRV |
| users.unlimited_history | NOT NULL default false: the 30 day history window applies unless the flag is set | `20260707120000` | PRV |
| users.deleted_monthly | default true, nullable (older "delete monthly" setting; the cron job does not read it) | baseline | PRV |
| users.blocked | nullable boolean, no default | baseline | IDN |
| jobs.recovery_attempts | NOT NULL default 0, load bearing: the recovery sweep claims a job with compare and swap on this column, and a NULL would never match (migration comment) | `20260922120000` | JOB |
| jobs.retry_count | default 0 | baseline | JOB |
| jobs.storage_provider | NOT NULL default gcs | `20251221000000` | UPL |
| jobs.transcription_provider | default deepgram | baseline | TRX |
| jobs.job_status | NOT NULL, no default (the app always sets it) | baseline | JOB |
| jobs.duration | smallint minutes (max 32,767); NULL means unknown until the provider answers (deferred billing) | baseline, `20260408120000` | BIL |
| jobs.speakers | default false | baseline | TRX |
| transcripts.translation_status | default original | `20251130114026` | TRL |
| packages.start_at, end_at | NOT NULL, default now(): a package row with no dates is open for zero time | baseline | BIL |
| packages.minutes_included | nullable (a NULL package has no balance) | baseline | BIL |
| payments.status, amount, currency, payment_gateway | NOT NULL | baseline | BIL |
| coupon.expires_at | default now(): a coupon inserted without an expiry is already expired | baseline | CPN |
| ai_consent.has_consented | NOT NULL default false; consent_version default '1.0'; provider_name default 'deepseek' | baseline | AI |
| summaries.ai_provider, chat_messages.ai_provider, highlights.ai_provider | default deepseek | baseline | AI |
| api_keys.is_active, scopes, expiration_policy | default true, empty array, never | baseline | API |
| audit.cleanup_audit_log.executed_by | default pg_cron | `20251222100000` | PRV |

Foreign key behavior that decides what disappears (this is where deletion rules hide)

| FK | Behavior | Effect | Source | Owner |
|---|---|---|---|---|
| public.users.id to auth.users | ON DELETE CASCADE | Deleting the GoTrue account deletes the profile | baseline `:4397` | IDN |
| jobs.user_id, packages.user_id, payments.user_id, usage_history.user_id to public.users | ON DELETE CASCADE | Account deletion removes jobs, packages, payments and the whole ledger. Payments are financial records | baseline `:4362-4397` | PRV |
| usage_history.job_id to jobs | ON DELETE CASCADE | Deleting a job deletes its ledger rows, so the balance goes back up | baseline `:4382` | BIL |
| usage_history.package_id to packages | ON DELETE CASCADE | same for packages | baseline `:4387` | BIL |
| packages.payment_id to payments | no action | A payment with packages cannot be deleted alone | baseline `:4327` | BIL |
| payments.coupon_id to coupon | no action | A used coupon cannot be deleted | baseline `:4332` | CPN |
| transcripts.job_id to jobs | ON DELETE RESTRICT (added `20251222000001`, comment: "use explicit cleanup functions") | A job with lines cannot be deleted until lines are deleted | `20251222000001` | PRV |
| transcripts.user_id to auth.users | ON DELETE CASCADE | Account deletion removes lines | baseline `:4422` | PRV |
| chat_*, highlights, summaries, ai_analysis, job_feedback, callback_deliveries to jobs | ON DELETE CASCADE | AI results and chats go with the job | baseline, `20260305120001` | AI |
| ai_consent, chat_limits, chat_messages, highlights, summaries, ai_analysis, api_keys, api_key_rate_limits, webhook_notifications, job_feedback, transcripts, translation_beta_consent_archived to auth.users | ON DELETE CASCADE | Account deletion removes them | baseline | PRV |
| api_key_audit_logs.user_id and api_key_id | ON DELETE SET NULL | Audit rows survive account deletion with no owner | baseline `:4257-4262` | API |
| subscriptions.user_id to auth.users | no action | A leftover subscription row blocks deleting the auth user (table is dead) | baseline `:4407` | IDN (dead) |
| billing_info.user_id, Invoices.user_id and payment_id | ON DELETE SET NULL | Invoice details survive without an owner | baseline | BIL |
| jobs.assigned_worker to jobs(id) | ON DELETE CASCADE | Wrong target (should be workers). Deleting a job deletes every job pointing at it as worker | baseline `:4362` | JOB (defect) |
| workers.running_job to jobs | ON DELETE SET NULL | dead | baseline | dead |
| accounting.Invoices.payment_id to payments | RESTRICT | dead | baseline | dead |

### pgTAP mapping

Both files are flat pgTAP scripts (no named test functions). Each is wrapped in BEGIN and ROLLBACK. The data model topic owns them (README). 000-setup.sql has 3 assertions (plan 3) and 050-view-schema-tests.sql has 23 (plan 23). Where an assertion checks a billing rule (balance derivation), the Owner topic says billing and the BIL ledger row must cite this file. The other six pgTAP files (020, 030, 040, 060, 080, 090) belong to billing; this topic only notes which objects they touch (see below the table).

| Test file | Line | Object | Property checked | Owner topic |
|---|---|---|---|---|
| 000-setup.sql | 24 | extension pgtap | `has_extension('pgtap')`: pgTAP is installed in the test database | data model (test infrastructure) |
| 000-setup.sql | 168 | function create_test_user(text, text) and trigger on_auth_user_created | The helper inserts into auth.users and returns a non-null uuid. It relies on the auth.users trigger that mirrors the row into public.users (the helper then updates full_name on that row) | data model |
| 000-setup.sql | 177 | function create_test_package(uuid, int, timestamptz, timestamptz, text) and table packages | The helper inserts a package and returns a non-null id | data model |
| 050-view-schema-tests.sql | 51 | view user_balance_view | After one 100 minute package that is open now, balance for the user is 100 | billing |
| 050-view-schema-tests.sql | 71 | view user_balance_view, table usage_history | After a 30 minute usage row, balance is 70 (100 minus 30) | billing |
| 050-view-schema-tests.sql | 77 | view user_balance_view | Exactly one row per user | billing |
| 050-view-schema-tests.sql | 86 | table public.users | table exists | data model |
| 050-view-schema-tests.sql | 87 | table public.packages | table exists | data model |
| 050-view-schema-tests.sql | 88 | table public.payments | table exists | data model |
| 050-view-schema-tests.sql | 89 | table public.jobs | table exists | data model |
| 050-view-schema-tests.sql | 90 | table public.usage_history | table exists | data model |
| 050-view-schema-tests.sql | 91 | table auth.users | table exists (the dependency on the identity schema) | data model |
| 050-view-schema-tests.sql | 97 | column packages.user_id | column exists | data model |
| 050-view-schema-tests.sql | 98 | column packages.minutes_included | column exists | data model |
| 050-view-schema-tests.sql | 99 | column packages.start_at | column exists | data model |
| 050-view-schema-tests.sql | 100 | column packages.end_at | column exists | data model |
| 050-view-schema-tests.sql | 102 | column usage_history.user_id | column exists | data model |
| 050-view-schema-tests.sql | 103 | column usage_history.minutes_consumed | column exists | data model |
| 050-view-schema-tests.sql | 104 | column usage_history.package_id | column exists | data model |
| 050-view-schema-tests.sql | 105 | column usage_history.job_id | column exists | data model |
| 050-view-schema-tests.sql | 107 | column jobs.user_id | column exists | data model |
| 050-view-schema-tests.sql | 108 | column jobs.job_status | column exists | data model |
| 050-view-schema-tests.sql | 109 | column jobs.duration | column exists | data model |
| 050-view-schema-tests.sql | 115 | FK packages.user_id | `col_is_fk`: the column is a foreign key (to public.users, cascade) | data model |
| 050-view-schema-tests.sql | 116 | FK usage_history.user_id | `col_is_fk` (to public.users, cascade) | data model |
| 050-view-schema-tests.sql | 117 | FK jobs.user_id | `col_is_fk` (to public.users, cascade) | data model |

Notes on these two files:

- 050 lines 22-49 and 58-69 are fixture code (DO blocks) that insert into auth.users and rely on the insert_freemium_package trigger, then delete the freemium package (`DELETE FROM packages`) to get a known balance. So the freemium trigger and the mirror trigger are exercised by the fixture, not asserted.
- 000 line 157 (`create_test_job`) inserts a text id into jobs.id, which is bigint. The helper is never called in 000, so the broken helper does not fail the file. See Defects.
- No assertion in 000 or 050 checks an enum, an RLS policy, a CHECK constraint, a grant or a pg_cron job. Those objects have no pgTAP cover at all.

Objects touched by the other pgTAP files (billing owns the assertions; counts are the plan() numbers):

| File | Objects touched |
|---|---|
| 020-balance-functions-tests.sql (23) | get_user_balance, deduct_minutes, request_breakdown_over_packages, user_balance_view, packages, usage_history, users |
| 030-payment-trigger-tests.sql (39) | trigger insert_new_packages_on_payment_completed (payment_status_trigger), payments, packages, users |
| 040-job-trigger-tests.sql (16) | trigger insert_usage_history_on_job_start (job_status_trigger), get_user_balance, deduct_minutes, jobs, usage_history, packages |
| 060-regression-tests.sql (11) | request_breakdown_over_packages, get_user_packages_with_balance, get_user_balance, jobs, usage_history, packages |
| 080-freemium-first-job-bypass-tests.sql (14) | job trigger, request_breakdown_over_packages, user_balance_view, jobs, usage_history, packages |
| 090-null-duration-api-job-tests.sql (14) | job trigger, deduct_minutes, get_user_balance, jobs, usage_history, packages |

No pgTAP file touches: the cleanup functions and their audit table, the pg_cron jobs, the API key functions, chat limits, AI tables, callback_deliveries, the analytics views, RLS policies, the storage policies, or the auth mirror and anonymize triggers directly. The tests in `20251221200008_cleanup_test_suite.sql` and `20251222000003_comprehensive_test_suite.sql` are the only tests for the cleanup functions, and they live in migrations.

## Rules

The business behavior remains owned by its topic. DAT rows below preserve the schema/test obligations from pgTAP without requiring the new database to copy public table names or trigger implementations. Test infrastructure is not a product feature: replace pgTAP-specific setup with the Rust/Postgres test harness, not a runtime dependency.

| ID | Rule | Source | pgTAP | Rust test | Status |
|---|---|---|---|---|---|
| DAT-001 | Legacy tests require pgTAP and isolated user/package fixture creation with non-null identifiers. Preserve the ability to construct these fixtures in the replacement test harness; do not install test functions in production. | frontend/supabase/tests/000-setup.sql:15-27; frontend/supabase/tests/000-setup.sql:168-194 | 000-setup.sql:24; 000-setup.sql:168; 000-setup.sql:177 | tbd | ledger |
| DAT-002 | The source schema contains identity users, public user profiles, packages, payments, jobs and usage history. Migration must account for all six entities even when their destination tables move to module schemas. | frontend/supabase/tests/050-view-schema-tests.sql:86-91 | 050-view-schema-tests.sql:86-91 | tbd | ledger |
| DAT-003 | A migrated package preserves its owner, included minutes and inclusive validity window. Exact billing semantics belong to BIL-001 and BIL-005. | frontend/supabase/tests/050-view-schema-tests.sql:97-100; frontend/supabase/migrations/20251130113249_remote_schema.sql:3010-3023 | 050-view-schema-tests.sql:97-100 | tbd | ledger |
| DAT-004 | A migrated usage row preserves its owner, consumed minutes, package reference and job reference. Allocation and deletion behavior belong to billing and privacy. | frontend/supabase/tests/050-view-schema-tests.sql:102-105; frontend/supabase/migrations/20251130113249_remote_schema.sql:3710-3718 | 050-view-schema-tests.sql:102-105 | tbd | ledger |
| DAT-005 | A migrated job preserves its owner, status and nullable duration. Status transitions and deferred billing belong to jobs and billing. | frontend/supabase/tests/050-view-schema-tests.sql:107-109 | 050-view-schema-tests.sql:107-109 | tbd | ledger |
| DAT-006 | Legacy packages, usage rows and jobs reference a user through foreign keys. Migration must preserve ownership and detect orphan references; destination cross-context enforcement can use ports rather than shared-table foreign keys. | frontend/supabase/tests/050-view-schema-tests.sql:115-117 | 050-view-schema-tests.sql:115-117 | tbd | ledger |
| DAT-007 | The legacy balance view returns one row for a user with an open package: 100 minutes before usage and 70 after a 30-minute deduction. Migration compares this result with the Rust balance derivation, not a cached or separately stored balance. | frontend/supabase/tests/050-view-schema-tests.sql:51-80; frontend/supabase/migrations/20251130113249_remote_schema.sql:3739-3744 | 050-view-schema-tests.sql:51; 050-view-schema-tests.sql:71; 050-view-schema-tests.sql:77 | tbd | ledger |

## Defects

Current behavior is read from the SQL and code, not run. Intended behavior and proposal are for mu to decide. Severity in brackets.

| ID | Current behavior | Intended behavior | Proposal |
|---|---|---|---|
| DM-01 [high] | 15 SECURITY DEFINER functions can be called with the public anon key because every function is granted to anon (`20251130113249:4756-5724`, no REVOKE in 53 migrations). `cleanup_old_transcripts(7)` deletes the transcript lines of every opted in user older than 8 days, up to 100,000 rows per call. `cleanup_old_audit_logs`, `cleanup_old_rate_limits`, `cleanup_old_callback_deliveries`, `expire_expired_keys`, `cleanup_cron_job_history` are open the same way | Only the service role runs housekeeping functions | Check the production catalog now (Needs mu) and, regardless of the migration, revoke EXECUTE from anon and authenticated on all of them. In the new system there is no rpc surface and no function granted to a client role |
| DM-02 [high] | `revoke_api_key(key_id)` is SECURITY DEFINER with no owner check (baseline `:1088`); anyone who knows a key uuid can revoke it | Only the owner revokes a key | Revoke becomes a use case with an ownership check |
| DM-03 [high] | `verify_api_key` and `validate_api_key` are open to anon and scan every active key with a bcrypt compare per call (`WHERE key_hash = crypt(provided_key, key_hash)`, baseline `:1211`). Each call also updates last_used_at and writes an audit row | Key checks are rate limited and cost one lookup | Keys get an id prefix in the new format (old keys keep the O(n) scan during a transition window). Needs the R3 answer on how many active keys exist |
| DM-04 [high] | `check_rate_limit` uses `ON CONFLICT (api_key_id, window_start)` but no unique index on those columns exists in any migration (only a PK on id). The call errors and `auth.ts:129-131` only logs the error, so API rate limiting is probably not enforced at all | Per key limits of 60 per minute and 1000 per hour are enforced (the intended rule in the function defaults) | Verify on the snapshot (`\d api_key_rate_limits`). Re-implement behind a `RateLimiter` port with a shared store (RTL) |
| DM-05 [high] | RLS policy `Can insert new usage` lets any signed in user insert rows into usage_history for themselves with any minutes_consumed and package_id (baseline `:4455`). A negative value adds minutes to the balance | The ledger is written only by the billing use case | Drop the policy; the new ledger accepts inserts only from the billing module. Keep the select policy |
| DM-06 [high] | `jobs` policy `Can operate own jobs data.` is FOR ALL for the owner (baseline `:4463`), so the owner can write duration, job_status, provider, cost and error. The billing trigger trusts `duration` and the status change | Duration and status come from the server (upload analysis and provider callbacks) | Job writes go through use cases only. RLS stays as a select safety net |
| DM-07 [medium] | `users` update policy has no column limit (baseline `:4479`): the owner can set `blocked` to false and `unlimited_history` to true | Settings columns are writable by the owner, `blocked` is not | Split settings from account state in the new profile table |
| DM-08 [medium] | `chat_limits` is updatable by its owner (baseline `:4586`), so the AI chat counters can be reset from the client | Counters change only on the server | Move the counters to a server only table or the AI use case |
| DM-09 [medium] | `api_keys` insert and update policies let an owner write any scopes, hash or expiry directly (baseline `:4530`, `:4582`) | Keys are created by the key use case | Remove owner write policies |
| DM-10 [medium] | Table `coupon` is readable by everyone including anon (`USING true`, baseline `:4515`), and `coupon.expires_at` defaults to now(), so a coupon inserted without an expiry is expired immediately | Coupon list is not public; expiry is explicit | Billing module owns coupons; no public read |
| DM-11 [high] | Three triggers (`welcome`, `purchased`, `failed_job`) call `https://transgate.ai/api/hooks/...` with a shared secret written in the SQL (baseline `:4210`, `:4226`, `:4242`). The secret is in git history. Staging and local databases call production. `failed_job` fires on every UPDATE of jobs | Emails come from use cases. Secrets are not in migrations | Rotate the secret. In the new system the three effects become outbox events handled by the Mailer port (EML). Do not copy these triggers |
| DM-12 [high if exposed] | 27 of the 39 analytics views are granted SELECT to `authenticated`, owned by postgres, no security_invoker, joining auth.users and payments (`20260222092040`). They hold personal email addresses of the team hard coded in 7 views (6 in analytics, 1 in accounting) and a fixed currency rate (35 or 36 TRY per USD) in 3 | Reporting reads only for staff, with no personal data in SQL | Check schema exposure and USAGE (Needs mu). New system: staff only `ops` views, no hard coded emails |
| DM-13 [medium] | `audit.cleanup_audit_log.status` CHECK allows success, failure, partial. The hardened cleanup functions insert `skipped` and `aborted` (`20251222100001:71,118,275,308,454,487`). The insert fails, the exception handler logs a failure row and re-raises | The log accepts every status the job writes | Fix in the new retention job table: status is a closed Rust enum |
| DM-14 [medium] | 15 test functions are created in production `public` and stay executable by anon (`20251221200008`, `20251222000003`); `test_cleanup_test_data()` deletes rows by fixed marker values. `create_test_job` in `tests/000-setup.sql:145-162` inserts a text id into jobs.id (bigint) and cannot work | Tests live outside the production schema | Drop all 15 now; new tests run in a throwaway database |
| DM-15 [medium] | `anonymize_user_data` (trigger on auth.users delete) sets api_keys.user_id and api_key_audit_logs.user_id to the zero uuid, which is not a row in auth.users. The FKs on those columns (cascade and set null) fire first, so the function probably updates nothing, or would fail the FK | Account deletion removes or anonymizes personal data as the privacy rules say | Privacy topic decides; the new deletion saga is explicit. Note payments, packages and usage rows are deleted by cascade from users (financial records) |
| DM-16 [medium] | `jobs.assigned_worker` has a foreign key to `jobs(id)` with ON DELETE CASCADE (baseline `:4362`), not to `workers` | The column is a worker id or unused | Drop the column (RunPod is dropped) |
| DM-17 [medium] | `deduct_minutes` has 3 overloads. The 4 argument and 5 argument forms both accept 3 or 4 arguments through defaults, so a call with fewer than 5 arguments can be ambiguous; the two callers pass all 5 and say so in a comment (`src/utils/deepgram.ts:370`, `src/utils/soniox.ts:259`). Also the balance check and the insert are not locked: two parallel jobs both pass (known, PRD section 2) | One signature; a locked allocation | Billing ledger rule BIL covers the lock; the new `allocate` is one function |
| DM-18 [medium] | No unique key on usage_history for a job: if a job re-enters not_started the trigger charges again; deleting a job deletes its ledger rows (cascade) and refunds the minutes | One charge per job; the ledger is append only | Unique (job_id, package_id) plus no cascade from jobs. Billing decides |
| DM-19 [medium] | `get_user_packages_with_balance` compares `input_time` (timestamp without zone) with timestamptz columns, so the result depends on the session TimeZone. Production sessions use UTC by default | Same answer in any session | The new code uses timestamptz everywhere; the migration dry run must run with `SET TIME ZONE 'UTC'` |
| DM-20 [low] | `get_user_balance` comment says it returns 0 for a user with no active package. It returns no row (the COALESCE is inside the SUM of an existing group) | Missing row means zero | The new derivation returns 0; the dry run compares with absent equal to 0 |
| DM-21 [low] | `process_payment` reads and updates a table `balances` that does not exist, and swallows the error | Not present | Throw |
| DM-22 [low] | 17 SECURITY DEFINER functions have no `SET search_path` | Pinned search path | Moot: functions go away |
| DM-23 [low] | Two retention flags: `users.deleted_monthly` (default true, edited in the Settings screen `ManageData.tsx:23`, `JobRepository.ts:409`) and `users.auto_delete_transcripts` (the only one the cron job reads). The first has no server job behind it | One flag | Privacy topic (PRV) decides which is real |
| DM-24 [low] | Migration 1 (`20250308143806`) sorts before the baseline and alters `billing_info`, which the baseline creates | Replay from zero works | The new repo replays from zero in CI. Verify how CI avoids the failure today |
| DM-25 [low] | `ai_analysis` was created in production by hand and documented later (`20260305120001`); `notify_job_uploaded` and `job_uploaded_webhook` were dropped in `20260105120000` though no migration creates them; `20260116170000_optimize_database_io_performance.sql` sits in `_backup_orphans` with unknown status | Migrations equal production | The pg_dump snapshot is the truth; compare its schema to the baseline plus migrations (Needs mu) |
| DM-26 [low] | Soft deleted jobs (`job_status = 'deleted'`, `JobRepository.ts:337,356`) keep their rows, and the cleanup job only deletes transcript lines of `completed` jobs, so lines of deleted jobs are never removed by the job | Deleted means gone, per the privacy rules | PRV decides; the dry run counts lines under deleted jobs |
| DM-27 [low] | `Invoices` tables, `billing_info.parasut_contact_id` and the view `invoice_requests` (hard coded week of March 2025) only serve the Paraşüt flow | Not ported | Archive; do not migrate |
| DM-28 [low] | Other writers to the database exist outside the app: the RunPod worker writes transcripts and jobs with the service key (`runpod-whisper-x/src/whisperx_worker/supabase_utils.py:390,442`) and the Cloud Function `Infra/gcf-process-uploaded-file/src/index.ts:115` updates jobs | One writer | RunPod is dropped. ADR-023 says the Cloud Function was folded into the app; confirm it is off |
| DM-29 [low] | `calculate_expiration_date` is marked IMMUTABLE but uses NOW(); `get_next_message_order` is an unlocked max+1 | Correct volatility | Moot in the new system |
| DM-30 [low] | `job_status` enum keeps `uploaded`, `processing`, `ready` that no code path uses since 2026-01-07 | 9 values | The new enum has 9 values; map nothing |

## Keep / throw

Rule used for every verdict (PRD section 3): constraints, RLS as a safety net, housekeeping and read only views stay in the database; a business rule in a trigger or function moves to Rust with a test. "Lands in" names the crate group or document.

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

## Parity rows

Data parity is the migration dry run; see Research.

## Research

### Migration tool inputs (7.5)

Scope: `transgate/tools/migrate` reads a `pg_dump` snapshot restored into a scratch Postgres, runs the counts below on the old side, writes the new schema, runs the matching counts on the new side and prints a parity report. The gate needs zero mismatches. All SQL is read only and meant for the scratch copy, never for production. Run every session with `SET TIME ZONE 'UTC';` (DM-19).

**What the dry run counts and compares**

| Subject | Old side SQL | Compare with new side |
|---|---|---|
| Users | `SELECT count(*) FROM auth.users;` `SELECT count(*) FROM public.users;` `SELECT count(*) FROM public.users u LEFT JOIN auth.users a ON a.id=u.id WHERE a.id IS NULL;` (expect 0) `SELECT count(*) FILTER (WHERE blocked IS TRUE), count(*) FILTER (WHERE auto_delete_transcripts), count(*) FILTER (WHERE unlimited_history), count(*) FILTER (WHERE deleted_monthly) FROM public.users;` `SELECT provider, count(*) FROM auth.identities GROUP BY 1;` | Same counts of accounts and identities by provider; same flag counts; every old uuid present |
| Jobs | `SELECT job_status, count(*), coalesce(sum(duration),0) FROM public.jobs GROUP BY 1;` `SELECT transcription_provider, storage_provider, count(*) FROM public.jobs GROUP BY 1,2;` `SELECT count(*) FROM public.jobs WHERE duration IS NULL;` | Count and summed minutes per status; same ids |
| Transcripts | `SELECT translation_status, count(*), count(DISTINCT job_id) FROM public.transcripts GROUP BY 1;` `SELECT language, count(*) FROM public.transcripts GROUP BY 1;` plus a content hash per job: `SELECT job_id, md5(string_agg(concat_ws(chr(31), line, start_time, speaker), chr(10) ORDER BY id)) FROM public.transcripts GROUP BY 1;` | Line count and per job hash (if stored as JSON, rebuild the same ordered string) |
| Payments | `SELECT payment_gateway, currency, status, count(*), sum(amount) FROM public.payments GROUP BY 1,2,3;` `SELECT count(*) FROM public.payments WHERE coupon_id IS NOT NULL;` | Count and money sum per gateway, currency and status |
| Packages | `SELECT name, count(*), sum(minutes_included), count(*) FILTER (WHERE now() BETWEEN start_at AND end_at) FROM public.packages GROUP BY 1;` `SELECT count(*) FROM public.packages WHERE payment_id IS NOT NULL;` | Count, minutes and windows per package name; payment link kept |
| Usage rows | `SELECT count(*), coalesce(sum(minutes_consumed),0), count(DISTINCT job_id), count(*) FILTER (WHERE job_id IS NULL) FROM public.usage_history;` `SELECT package_id, sum(minutes_consumed) FROM public.usage_history GROUP BY 1;` | Count, minutes total and per package |
| Balance per user | Old function and inputs: `public.get_user_balance(time_p timestamp)` returns `(user_id uuid, balance int)`; per package `public.get_user_packages_with_balance(input_time timestamp)`; current view `public.user_balance_view`. Run `SELECT user_id, balance FROM public.get_user_balance('<snapshot instant, UTC>');` and `SELECT * FROM public.get_user_packages_with_balance('<snapshot instant, UTC>');`. A user with no open package has no row | New derivation at the same instant, per user and per package; absent equals zero; negative balances (freemium first job) must match. Also run at snapshot plus 7 days to test window edges |
| API keys | `SELECT count(*), count(*) FILTER (WHERE is_active), count(*) FILTER (WHERE expires_at < now()), count(DISTINCT user_id) FROM public.api_keys;` `SELECT left(key_hash,7), count(*) FROM public.api_keys GROUP BY 1;` `SELECT scopes, count(*) FROM public.api_keys GROUP BY 1;` | Same counts; hash prefix preserved; a stored test key (only the owner knows real ones) verifies in the new code |
| Consents | `SELECT has_consented, consent_version, provider_name, count(*) FROM public.ai_consent GROUP BY 1,2,3;` `SELECT count(*), count(*) FILTER (WHERE has_consented) FROM public.translation_beta_consent_archived;` | Same counts and dates; the cookie consent is a browser cookie `cookieConsent` with a 1 year expiry (`src/components/CookieConsent/CookieConsent.tsx:31,38`), not in the database, so there is nothing to compare |
| Other | `coupon`, `billing_info`, `ai_analysis`, `summaries`, `highlights`, `chat_messages`, `chat_limits`, `job_feedback`, `callback_deliveries` (last 30 days), `api_key_audit_logs`: `SELECT count(*) FROM public.<table>;` | Count per table that is migrated |

Consistency checks on the old side (a mismatch here is old data debt, report it and do not count it as a migration failure): completed payments with no package `SELECT p.id FROM payments p LEFT JOIN packages k ON k.payment_id=p.id WHERE p.status='completed' AND k.id IS NULL;`; usage rows whose package belongs to another user `SELECT count(*) FROM usage_history u JOIN packages p ON p.id=u.package_id WHERE p.user_id<>u.user_id;` (must be 0, else the balance grouping splits); users with no freemium package; job lines under `deleted` jobs `SELECT count(*) FROM transcripts t JOIN jobs j ON j.id=t.job_id WHERE j.job_status='deleted';` (DM-26); duplicate Iyzico tokens (the unique index since 2026-10-22 forbids them).

**Copy order** (foreign keys; the new schema has fewer FKs across modules, but the order still keeps references valid):

1. GoTrue tables `auth.users` and `auth.identities` (and MFA factors if used). Password hashes and OAuth identities come with the rows.
2. identity profile (from `public.users`)
3. billing: `coupon`, `payments`, `packages`
4. transcription: `jobs`, then `usage_history` (needs packages and jobs), then `transcripts`, `job_feedback`
5. ai: `ai_consent`, `ai_analysis`, `summaries`, `highlights`, `chat_messages`, `chat_limits`
6. api: `api_keys`, `api_key_audit_logs`, `callback_deliveries`
7. privacy: archived beta consents
8. `billing_info`
9. Reset sequences after the copy (`setval` to max id plus one) for jobs, payments, packages, usage_history, transcripts.

When the staging copy uses the old schema, restore with `SET session_replication_role = replica` or the triggers fire: the freemium trigger adds a package for every inserted user, the payment trigger adds packages for every completed payment update, `failed_job`, `purchased` and `welcome` would call production. The new schema has no such triggers.

**Id strategy**: keep every existing id. Users, api_keys, ai_analysis and audit rows keep their uuids. `jobs.id` (bigint) stays because it appears in URLs, provider references (`client_reference_id`), callback payloads and storage paths built from `(user_id, job_id, job_name)`. `payments.id`, `packages.id` (serial) stay because packages link to payments and usage rows link to packages. `transcripts.id` stays so editing and exports are stable. New rows after cut over continue from the reset sequences.

**Columns to transform**

| Column | Transform |
|---|---|
| jobs.created, payments.created, payments.updated_at (timestamp without zone, UTC by default) | to timestamptz assuming UTC |
| transcripts.start_time, end_time (text `HH:MM:SS,mmm`) | parse to milliseconds (integer) or keep text if the design keeps rows as they are |
| jobs.duration (smallint minutes) | widen to integer |
| jobs.language, translate_to, transcripts.language (57 names in migrations; four more confirmed in production) | Map all 61 offered names to the LNG code type; preserve the four drifted values confirmed by `../production-facts.md:39` |
| jobs.callback_url when storage_provider = external_url | to a source url column (the column is overloaded: customer callback for API jobs, source URL for URL and YouTube jobs) |
| jobs.whisper_params, transcript_source, youtube_* | YouTube keys already copied to columns by `20260410120000`; drop the rest of whisper_params after checking what other keys exist |
| jobs.error (jsonb) | keep as is |
| payments.metadata (jsonb with `package`: interval, frequency, count, expiry, name, minutes) | keep as the stored snapshot; also expand into typed columns |
| users | split: identity profile (id, email, name, avatar) and settings (auto_delete_transcripts, unlimited_history, deleted_monthly) and `blocked` |
| api_keys.key_hash | keep byte for byte (bcrypt through pgcrypto `crypt` and `gen_salt('bf')`; expected prefix `$2a$06$`, confirm with the prefix count query above); the Rust bcrypt crate reads that format. Old keys have the form `sk_` plus base64 of 32 bytes with no id part, so verification must scan active keys until a new format with an id prefix exists |
| coupon.expires_at default now() | keep the stored values; do not copy the default |

**Data safe to drop (not migrated)**: `webhook_notifications`; `api_key_rate_limits`; `provider_health_metrics`; `provider_plugin_configs`; `workers`; `feature_flags`; `prices`, `products`, `subscriptions` (after the count shows zero rows); `generic_contacts`; `public.Invoices` and `accounting.Invoices` (archive); `audit.cleanup_audit_log`; `cron.job_run_details`; `callback_deliveries` older than 30 days (the retention that the unused function would have applied); jobs columns `worker_selector`, `assigned_worker`, `request_time`; the three unused `job_status` values (map nothing, they should have no rows: check `SELECT count(*) FROM jobs WHERE job_status IN ('uploaded','processing','ready');`).

**Size**: the only numbers in the repo are from January 2026: about 3.5 million transcript lines (2.1 GB), about 137,000 jobs. Use COPY in batches of 100,000 and a per job transaction for transcripts. The Python helper `frontend/migrate_job.py` (`docs/developer-guide-old/database-job-migration.md`) copies one job and its lines between two Supabase projects in batches of 100, keeps ids and rolls back on error; it is a reference for the single job path only.

**Pre-work from the schema**: replay the baseline plus 52 migrations into the scratch database with a stub `auth.users` and check against `types.gen.ts` (the stub is the workaround until a real snapshot exists, PRD section 10). `seed.sql` supplies a freemium user, a paid user with two payments and two jobs with lines for the first dry runs.

## Needs mu

Workaround assumed until these arrive (PRD section 10): the migration tool and the data model work use the baseline plus the 52 later migrations replayed into a scratch Postgres 15 with a stub `auth.users`, and the rows from `supabase/seed.sql`. Nothing in this file is verified against production data.

Reconciliation after the read-only probe: `../production-facts.md` supplies counts and selected values, not a schema dump. It confirms nine jobs in deprecated statuses; empty prices/products/subscriptions tables; 285 webhook_notifications rows; four language values absent from migrations; and recently active API keys/callbacks. These observations supersede assumptions of empty deprecated states or exact migration/production parity. Cron definitions, grants, triggers and the provider-column default remain unverified.

1. A `pg_dump` snapshot of production, taken by mu, with these parts:
   - Schema only for the whole database (all schemas, `--no-owner`), to compare against baseline plus migrations (DM-25, DM-04, DM-01 need it).
   - Data for `public`, `accounting` and `audit`.
   - Data for `auth.users` and `auth.identities` (and `auth.mfa_factors` if MFA is used). Yes, `auth.users` must be included: the PRD keeps user ids, password hashes and Google OAuth identities, and `public.users` and every user table point at it. It holds emails and bcrypt password hashes, so the dump stays outside git and outside shared folders.
   - Leave out `auth.sessions`, `auth.refresh_tokens`, `auth.audit_log_entries`, `auth.one_time_tokens`, `storage.*`, `realtime.*` and `cron.job_run_details`. Include the small table `cron.job` so the three schedules can be confirmed.
   - A note of the snapshot instant in UTC, used as the "as of" time for the balance comparison.
2. Read only answers from the production catalog (these are statements for mu or a Supabase SQL editor, I did not run them):
   - `SELECT jobid, jobname, schedule, command, active FROM cron.job;` (are the 3 jobs there and active)
   - `SELECT proname, proacl FROM pg_proc WHERE pronamespace='public'::regnamespace AND prosecdef;` (is anon still executing the cleanup and key functions, DM-01)
   - `\d public.api_key_rate_limits` (is there a unique index on api_key_id, window_start, DM-04)
   - `\dn+ analytics` and `\dp analytics.*`, and the "Exposed schemas" setting in the Supabase API settings (DM-12)
   - `SELECT enum_range(NULL::job_status);` and `SELECT count(*) FROM jobs WHERE job_status IN ('uploaded','processing','ready');`
   - `SELECT count(*) FROM prices; SELECT count(*) FROM products; SELECT count(*) FROM subscriptions; SELECT count(*) FROM workers; SELECT count(*) FROM webhook_notifications;` (confirm the throw list is empty or low)
   - `SELECT indexname FROM pg_indexes WHERE indexname = 'idx_packages_user_id';` (was the unapplied I/O migration run by hand)
   - Storage: does the bucket `jobs` exist and hold objects.
   - Auth settings from the dashboard: enabled providers, password length rule, email confirmation, JWT expiry.
   - Whether the RunPod worker and the Cloud Function `gcf-process-uploaded-file` still write to the database (DM-28).
3. Rotate the shared webhook secret that sits in three trigger definitions in git history (DM-11). This is an action for mu, not for the rewrite.
4. A decision on legal retention of payment and invoice records. Today an account delete cascades to payments, packages and usage rows (cascade chain in "Rules in DB constraints"), and the Paraşüt invoice tables are out of scope. Mu decides whether anything must be archived before the cut over.
5. The R3 numbers (active API keys) use the `api_keys` queries in Research; the API topic owns the answer.

## Open questions for the design

1. Schema per module with a prefix. `modules/billing` and `modules/cms` have their own schema prefix and migrations folder (PRD section 5). Options: one Postgres schema per module (`billing`, `cms`, with core in `app`), or table name prefixes in one schema. Per schema keeps a split a copy of one folder and allows a role per module. Either way the billing tables must not keep foreign keys into other modules: `packages.user_id` and `usage_history.job_id` become plain ids, and the cascades (account delete, job delete) become an explicit deletion saga. The current `usage_history.job_id ... ON DELETE CASCADE` also refunds minutes when a job is deleted, which a plain id removes.
2. Auth schema ownership with GoTrue. GoTrue owns `auth` and runs its own migrations as its own role. The old app put 3 triggers on `auth.users` and 15 foreign keys into it. The new app should create no trigger on `auth` and no foreign key into it: a profile row is created on first sign in (idempotent use case that also grants the freemium package), and user ids are plain uuids. Question for mu: accept the one lazily created profile (a small gap between signup and first request), or keep one dumb mirror trigger on `auth.users` and accept the coupling to GoTrue's schema.
3. Transcripts as JSON or rows. Today one row per line: about 3.5 million rows for about 137,000 jobs (Jan 2026), 2.1 GB, indexes on (user_id, job_id, id) and job_id, line level edits (`corrected_text`, `speaker`), realtime per row, retention deletes lines but keeps the job, translations are more rows with `translation_status`. One document per job and language would cut rows by about 25 times and make export and delete atomic, but line edits then rewrite a whole document, need a revision check (the pattern already has revision checked saves), and the realtime and hash comparisons change. Keeping rows is the lowest risk for the migration dry run; a document is the simpler read model. The design topic and mu decide before P3.
4. Balance read model: keep a view over packages and usage, or a `minutes_remaining` column kept in the same transaction (docs/thoughts.md). The dry run compares either to `get_user_balance`.
5. Enums or text. The 57 value `language` enum needs a migration for every new language and `ALTER TYPE ADD VALUE` needs its own transaction (migrations 4, 24, 50 show it). A text column checked against a generated list (LNG) avoids this. Same question for `job_status` (9 used of 12) and `transcript_provider`.
6. Soft deleted jobs. Keep the status `deleted` or remove rows (DM-26)? Privacy decides; it changes what the migration copies.
7. Timestamps and units. Use timestamptz everywhere and name units in columns (`duration_minutes`). Does a duration in minutes keep its current rounding (billing topic)?
8. Public ids. Keep `bigint` job ids as the public identifier (they are in URLs and callbacks) or add an opaque id for the API (API topic).
9. One retention flag instead of two (`deleted_monthly`, `auto_delete_transcripts`, DM-23).
10. Realtime replacement. The browser listens to row changes on `jobs` and `transcripts` through Supabase Realtime; the Rust server needs its own push channel (web shell topic).
