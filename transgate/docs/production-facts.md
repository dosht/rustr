# Production facts (read only, 2026-10-24)

Collected with `railway run` on the Transgate production frontend service, using Supabase REST GET and HEAD only. Counts, dates and config values only; no keys, emails or URLs. Script: the session scratchpad `probe/prod_probe.py`.

## R3: public API use

- 46 API keys, 40 active, 39 active and not expired, 31 owners.
- Only 2 keys (2 owners) were used in the last 90 days, the most recent use was 2026-10-24. 16 keys were used in the last year.
- Audit log: 872 rows, mostly 2025-09 to 2026-04, 17 rows in 2026-10, 18 rows in 90 days from 4 users and 5 keys. The endpoint column is empty, so the version used (v1, v1-beta, unified) is unknown.
- Callbacks: 49 deliveries in total, 2 successful in the last 90 days, the latest on 2026-10-16. 37 jobs carry a customer callback URL (12 hosts, 5 users).
- Verdict for mu: the API is small but live. Two owners would break at cutover without a shim. Options: a v1-beta shim, or contacting those two owners.

## Config that rules read

AI chat: 50 questions and 20000 tokens per transcript, 500 questions and 50000 tokens per user per day, reset daily. AI provider deepseek (api.deepseek.com), OPENAI_MODEL gpt-3.5-turbo, AI consent required, version 1.0, 60 requests per minute, 3 retries, 60 s timeouts. Soniox max duration 200 minutes, Soniox auto delete on. USD_TRY_RATE 44. RunPod MODEL large-v3.

## Data

| Table | Rows |
|---|---|
| users | 98383 |
| jobs | 163316 |
| transcripts (planner estimate) | 9837462 |
| payments | 6512 (2054 completed, 4354 pending, 103 failed, 1 canceled) |
| packages | 100525 (98359 freemium) |
| usage_history | 132143 |
| ai_consent | 1859 |
| ai_analysis | 3050 |
| chat_limits | 117595 |
| coupon | 31 |
| subscriptions, prices, products | 0 |
| webhook_notifications | 285 |
| provider_health_metrics | 6948 |

- Completed payments: 2052 Iyzico, 1 bank, 1 Iyzico IBAN. 1222 in USD, 832 in TRY.
- Packages open now: 2650 freemium, 686 Pay-as-you-Go, 14 PREMIUM, 8 BUSINESS, 3 Master, plus single rows named Pay-as-you-Go-Add, pay-as-you-go, freemium-gift and one with no name.
- Jobs in the last 90 days: 9629. Provider: Soniox 7741, none set 1774, Deepgram 114. Source: 690 from `transcriptapi`, 773 with storage `external_url`. 58 languages used, led by English, Urdu, Kazakh, Hindi and Turkish.
- 9 jobs sit in the deprecated statuses uploaded, processing or ready (the data model note expected none).
- Georgian, Latin, Uzbek and Luxembourgish do exist in the production `language` enum. The languages defect is drift between the migrations and production, not a missing value.
- Users: 4 blocked, 4 with unlimited_history, 16 opted out of auto delete.
- Railway has a `staging` environment next to `production`.

## Not verified

- Anon exposure of SECURITY DEFINER functions: the PostgREST schema endpoint answered 401 to the anon key, so this stays open.
- pg_cron jobs, grants, triggers and the latest payment date: not reachable through REST.
