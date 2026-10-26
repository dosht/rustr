# P2 billing implementation and exercised evidence

The developer handoff was source-only. Lead subsequently passed 454 workspace tests across 39 test binaries with the database-backed cases enabled, then exercised both actual server binaries against restricted PostgreSQL logins and real local HTTP services, including restart/replay/delivery. Details and limits: `integration-notes.md`. This is the verified P2 baseline, not a claim that the P3 changes now in progress are tested or that every mixed-phase ledger clause is complete. Postgres, gateway and identity repair notes retain their original source-only provenance.

## Packages

| Package | Path | Role |
|---|---|---|
| billing-core | modules/billing/core | rules, use cases (`Billing`), presentation policies, ports. No I/O. |
| billing-postgres | modules/billing/postgres | schema `billing`, transactions, outbox with leases, runtime-role verification, `billing-provision` |
| billing-http | modules/billing/http | Axum contract: `authenticated` (relative `/billing/...`) and `provider` (absolute provider paths) |
| billing-iyzico / billing-lemonsqueezy | modules/billing/gateways/* | provider adapters (fixtures: `fixtures/PROVENANCE.md`) |
| transgate-billing | transgate/billing | catalog, customer directory with trusted auth lookup, JWT identity, HTTP mailer, PostHog adapter, settings, composition, relay |
| taskr-billing | crates/taskr-billing | taskr sample: enrollment, 10 free credits, metered sample work, one LemonSqueezy credit pack |

Host composition: `transgate/server` (`main.rs`, `Cargo.toml`) and `crates/taskr-server` (`main.rs`, `Cargo.toml`, `tests/billing.rs`).

## Repairs from the host review, in my slice

| Review item | What now exists |
|---|---|
| Async order verification | `OrderWebhook::verify` returns a `BoxFuture`; `WebhookError::Unavailable` maps to a retryable 504 `provider_uncertain`, never a 401 or a grant. Core awaits verification before opening a transaction. |
| BIL-042 cascade | `release_subject_charges` discovers affected package owners (`imported_release_owners`), locks every one in ascending account id, rediscovers, refuses with a conflict if the set changed, then deletes. Must be the first billing call of the host's deletion transaction. Postgres side and its tests: Opus. |
| LemonSqueezy return URL | Provider redirect is `<public>/api/billing/payments/{our payment id}/status`; Iyzico stays `<public>/checkout/confirm`. `GET /api/billing/payments/{id}/status` is session-authenticated, owner-scoped (foreign or unknown id is 404), answers `{"payment_id","gateway","status"}` from the stored payment, never grants and never calls a provider. |
| Lemon total consistency | `order_net`: inclusive `total = subtotal - discount`, exclusive `total = subtotal - discount + tax`; negative, zero-subtotal or overflowing amounts hold as `amount_invalid`, a nonzero provider discount holds as `discount_present`, an inconsistent total holds as `amount_mismatch`. The quote is still compared to the pre-tax net. Cases: 745/124/745 inclusive and 1500/300/1800 exclusive. |
| Trusted signup and contact producer | Production Transgate requires `SUPABASE_URL` + `SUPABASE_SERVICE_ROLE_KEY` (unless `TRANSGATE_DEV_AUTH=1`). On the account's own authenticated access the directory asks `GET {url}/auth/v1/admin/users/{id}` for the identity system's `created_at`, verified address and `updated_at`, then provisions from them. Only a verified address is adopted. `created_at` is never updated; a newer verified address (by `updated_at`) replaces an older one; the service-token signup route only creates and never overwrites. When contact facts are due (15 minutes) and the trusted directory cannot answer, the request is an explicit 503 and mail for that account is retried, never served from stale facts. The service-token route is an extra reporting path, not the producer. |
| Re-confirmation is strict | Due contact facts are confirmed only by a positive answer: the auth directory must know the account, name a verified address, and that address must match the stored one after any newer verified change is applied. An outage, an unknown account, an unverified address or an unresolvable disagreement fail the request with 503 (and make the mail lookup fail so the event is retried); the account stays due and nothing is served from stale facts for another interval. An account reported only through the service-token route must therefore also exist in the auth directory. Tests: `a_verified_email_change_reaches_lookup_and_purchase_mail_without_a_second_trial_or_a_stale_replay_overwrite`, `readiness_identity_and_provisioning_through_the_composed_runtime`. |
| taskr contact | Enrollment is explicit and repeatable; a re-enrollment refreshes email and name only (migration `0002_contact_refresh.sql` grants UPDATE on those two columns); creation time and credits never change. |
| Required mail | Transgate settings refuse to start without the mail group; `Billing::requires_mail` makes `/api/billing/ready` fail without a mailer. taskr has no mailer and no relay: **taskr does not claim mail or outbox delivery.** |
| Independent deliveries | `relay_outbox` attempts every recipient and every analytics event independently, each with its own durable mark, and retries only what is missing with the same `<event id>:<target>` key. |
| Server analytics | PostHog, as the source (`frontend/src/utils/analytics/server.ts`): `POST <POSTHOG_HOST>/i/v0/e/` with `api_key`, `event`, `distinct_id` (account UUID), `properties`, `timestamp`, `uuid = uuidv5(JSON.stringify([event,"payments",id]), 4d407b2d-2266-45ca-87fc-b13c44f70f26)` and `$geoip_disable`. Events `payment_completed`, `payment_failed`, `subscription_started` (first plan payment only) with the source properties, and `user_signed_up` from account creation. Optional all-or-none group `POSTHOG_HOST` + `POSTHOG_API_KEY`. Client GA purchase and countdown stay with P5. |
| Fixture identities | Provider tokens, subscription refs and order ids in the Transgate, taskr and HTTP fixtures carry a random per-instance or per-call component; the Postgres fixtures use a fresh environment per test (Opus). Production uniqueness is unchanged. |
| Zero-minute overview (BIL-008/073) | An account with no open package keeps `balance: null` and shows `0 min`; `-` is only for an unavailable read. Test: `bil_008_073_an_account_whose_only_package_expired_shows_zero_minutes_and_no_balance_value` (HTTP, Postgres). |
| Post-start answer (BIL-074) | `billing_core::app::start_answer(tx, account, at)` reads every package row inside the debiting transaction and derives `new_balance` (open packages, 0 when none) and `is_freemium` (the source's first definition, FRE-013: exactly one package ever, named freemium). It is called only by hosts that build that answer; charging reads nothing extra. Consumed by the taskr sample route (`balance_after`, `is_freemium`). Tests: `presentation::tests::bil_074_a_started_answer_is_derived_from_the_state_after_the_debit` (first-job overdraft -16, paid 83, expired-paid history 15), the taskr-server mounted test, and Opus's Postgres test. |
| Removed non-evidence | `observation_from_status` and its legacy `INIT_THREEDS => Failed` test (D5: the adapter decides), the FRE-022 constant, the FRE-024 identity helper, the FRE-019 absence test, the BIL-067/068 string-constant test and the CPN-014 duplicate pure call. FRE-022 is unverified until the P3 download path exists; FRE-024's AI bypass until P4. |
| Plan and currency binding (BIL-050..053, CPN-003) | The expected provider plan codes and TRY/USD prices are literals copied from the ledger's plan table. `bil_050_053_cpn_003_each_plan_and_currency_sends_its_source_plan_code_and_price_to_the_provider` drives actual checkouts for all four plans in both currencies and asserts what the Iyzico fixture received; `data.rs` compares the catalog and quotes with the same literals instead of with themselves. |

## Runtime configuration

| Host | Variables |
|---|---|
| Transgate | `TRANSGATE_DATABASE_URL` (restricted `billing_runtime` login, an owner is refused), `TRANSGATE_MIGRATION_DATABASE_URL`, `TRANSGATE_PUBLIC_URL`, `IYZIPAY_{URI,API_KEY,SECRET_KEY,MERCHANT_ID,ENVIRONMENT}`, `LEMONSQUEEZY_{API_URL,API_KEY,STORE_ID,VARIANT_ID,WEBHOOK_SECRET,ENVIRONMENT}`, `USD_TRY_RATE`, `SUPABASE_JWT_SECRET`, `SUPABASE_JWKS_URL`, `TRANSGATE_DEV_AUTH=1`, `TRANSGATE_ADMIN_EMAILS`, **required** `SUPABASE_URL` + `SUPABASE_SERVICE_ROLE_KEY`, **required** `TRANSGATE_MAIL_{URL,API_KEY,FROM}` + `TRANSGATE_SALES_EMAIL` + `TRANSGATE_SUPPORT_EMAIL`, optional `POSTHOG_HOST` + `POSTHOG_API_KEY`, optional `TRANSGATE_SIGNUP_TOKEN`, `TRANSGATE_SPECIAL_*`. Each provider group is all-or-none. |
| taskr | `TASKR_BILLING_DATABASE_URL`, `TASKR_BILLING_MIGRATION_DATABASE_URL`, `TASKR_PUBLIC_URL`, `LEMONSQUEEZY_*` |

## Handoff commands

```sh
cargo test --locked -p billing-core -p billing-iyzico -p billing-lemonsqueezy -p billing-http -p transgate-billing -p taskr-billing
BILLING_TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:32769/rustr_billing_p2 \
  cargo test --locked -p billing-postgres -p billing-http -p transgate-billing -p taskr-billing -p taskr-server -- --include-ignored
```

Run the Postgres role-contamination tests (group role is cluster-wide) and the host smokes sequentially, and give each host its own database, because every composed runtime relays the global outbox. The Transgate runtime tests share one mail, PostHog and auth fixture per process for that reason and assert by unique recipient or account.

## Remaining verification limits

- P3 extensions are being implemented after the passing P2 baseline; their own integrated verification is still required. Strict Clippy cleanup has not yet had a final clean run.
- Live provider, mail, PostHog and Supabase behavior remains unverified: fixtures are source- or documentation-derived local servers. The documented PostHog single-event protocol at `/i/v0/e/` and stable UUIDs were exercised locally; this is not acceptance by a deployed PostHog service.
- The Supabase admin user response shape (`id`, `email`, `email_confirmed_at`, `created_at`, `updated_at`, `user_metadata`) is from its public documentation, not a recording.
- Iyzico lookup responses without an authoritative outcome stay uncertain under D5. The policy is settled; its final not-confirmed web surface belongs to P5.
- Held-notification evidence: the same identity with a different signed payload is now a conflict (409) instead of a silent update; a provider whose retries change the body would see 409s.

## Rule-to-test table

Ledger form: `cargo:<package>:<libtest name>`. References below were observed passing in the P2 workspace run; they prove only their stated assertions, not every clause of a mixed-phase row. BIL-066 includes actual mail delivery/restart recovery and BIL-072 includes server-event delivery. FRE-022 awaits the P3 download path; FRE-024 currently proves cap parsing, not P4 chat check/increment bypass. The canonical ledger remains the authority for full-row status, including later browser behavior.

| Rule | Tests |
|---|---|
| BIL-001 | `cargo:billing-core:ledger::tests::bil_001_002_003_remaining_is_included_minus_the_sum_of_usage`<br>`cargo:billing-postgres:bil_001_002_003_a_charge_writes_one_usage_row_per_package_and_remaining_is_derived` |
| BIL-002 | `cargo:billing-core:ledger::tests::bil_001_002_003_remaining_is_included_minus_the_sum_of_usage`<br>`cargo:billing-postgres:bil_001_002_003_a_charge_writes_one_usage_row_per_package_and_remaining_is_derived` |
| BIL-003 | `cargo:billing-core:ledger::tests::bil_001_002_003_remaining_is_included_minus_the_sum_of_usage`<br>`cargo:billing-postgres:bil_001_002_003_a_charge_writes_one_usage_row_per_package_and_remaining_is_derived` |
| BIL-004 | `cargo:billing-core:ledger::tests::bil_004_007_balance_is_the_sum_of_remaining_over_open_packages`<br>`cargo:billing-postgres:bil_004_to_008_stored_windows_read_back_exactly_in_a_non_utc_session` |
| BIL-005 | `cargo:billing-core:ledger::tests::bil_005_adjacent_yearly_windows_share_a_boundary_and_add_up_there`<br>`cargo:billing-core:ledger::tests::bil_005_both_window_ends_are_inclusive_to_the_microsecond` |
| BIL-006 | `cargo:billing-core:ledger::tests::bil_006_expired_and_future_packages_add_nothing` |
| BIL-007 | `cargo:billing-core:ledger::tests::bil_004_007_balance_is_the_sum_of_remaining_over_open_packages` |
| BIL-008 | `cargo:billing-core:ledger::tests::bil_008_no_open_package_has_no_balance_row_and_display_shows_zero`<br>`cargo:billing-http:bil_008_073_an_account_whose_only_package_expired_shows_zero_minutes_and_no_balance_value` |
| BIL-009 | `cargo:billing-core:ledger::tests::bil_009_balance_goes_negative_only_through_the_bypass`<br>`cargo:billing-postgres:bil_009_024_a_first_job_overdraft_is_persisted_as_one_full_row_and_zero_minutes_is_a_real_write` |
| BIL-010 | `cargo:billing-core:ledger::tests::bil_010_files_round_up_to_whole_minutes_and_batches_sum_rounded_files` |
| BIL-011 | `cargo:billing-core:ledger::tests::bil_011_012_allocation_takes_from_the_package_that_ends_first` |
| BIL-012 | `cargo:billing-core:ledger::tests::bil_011_012_allocation_takes_from_the_package_that_ends_first` |
| BIL-013 | `cargo:billing-core:ledger::tests::bil_013_breakdown_request_10` |
| BIL-014 | `cargo:billing-core:ledger::tests::bil_014_breakdown_request_45` |
| BIL-015 | `cargo:billing-core:ledger::tests::bil_015_breakdown_request_60` |
| BIL-016 | `cargo:billing-core:ledger::tests::bil_016_breakdown_request_90` |
| BIL-017 | `cargo:billing-core:ledger::tests::bil_017_breakdown_request_100_shortfall_shows_only_as_carry_and_is_refused` |
| BIL-018 | `cargo:billing-core:ledger::tests::bil_018_breakdown_with_no_open_package_returns_no_rows` |
| BIL-019 | `cargo:billing-core:ledger::tests::bil_019_a_debit_never_touches_another_accounts_packages`<br>`cargo:billing-http:bil_019_a_client_cannot_name_another_account_claim_admin_or_set_an_amount`<br>`cargo:billing-postgres:bil_019_accounts_are_isolated_and_a_cross_account_usage_reference_is_rejected` |
| BIL-020 | `cargo:billing-core:ledger::tests::bil_020_insufficient_balance_returns_the_negative_number_and_plans_no_write`<br>`cargo:billing-postgres:bil_020_021_022_strict_refusal_writes_nothing_and_exact_fit_leaves_exactly_zero` |
| BIL-021 | `cargo:billing-core:ledger::tests::bil_021_exact_fit_leaves_exactly_zero_not_zero_to_five`<br>`cargo:billing-postgres:bil_020_021_022_strict_refusal_writes_nothing_and_exact_fit_leaves_exactly_zero` |
| BIL-022 | `cargo:billing-core:ledger::tests::bil_022_zero_minutes_or_no_package_in_the_normal_path_raises_nothing_deducted`<br>`cargo:billing-postgres:bil_020_021_022_strict_refusal_writes_nothing_and_exact_fit_leaves_exactly_zero` |
| BIL-023 | `cargo:billing-core:ledger::tests::bil_023_one_function_covers_both_modes_with_five_named_inputs` |
| BIL-024 | `cargo:billing-core:ledger::tests::bil_024_bypass_writes_the_full_amount_on_the_earliest_ending_open_package`<br>`cargo:billing-postgres:bil_009_024_a_first_job_overdraft_is_persisted_as_one_full_row_and_zero_minutes_is_a_real_write` |
| BIL-025 | `cargo:billing-core:charge::tests::bil_025_026_079_only_draft_ready_uploaded_to_not_started_bills`<br>`cargo:billing-postgres:bil_025_026_027_032_jobs_are_billed_once_at_start_and_a_repeat_start_charges_nothing` |
| BIL-026 | `cargo:billing-core:charge::tests::bil_025_026_079_only_draft_ready_uploaded_to_not_started_bills`<br>`cargo:billing-postgres:bil_025_026_027_032_jobs_are_billed_once_at_start_and_a_repeat_start_charges_nothing` |
| BIL-027 | `cargo:billing-core:charge::tests::bil_027_the_whole_duration_is_charged_in_one_decision`<br>`cargo:billing-postgres:bil_025_026_027_032_jobs_are_billed_once_at_start_and_a_repeat_start_charges_nothing` |
| BIL-028 | `cargo:billing-core:charge::tests::bil_028_unknown_duration_defers_without_a_balance_check`<br>`cargo:billing-postgres:bil_028_033_034_036_a_deferred_charge_that_fails_still_commits_delivery_and_repeats_charge_once` |
| BIL-029 | `cargo:billing-core:charge::tests::bil_029_030_031_paid_path_errors_and_exact_messages` |
| BIL-030 | `cargo:billing-core:charge::tests::bil_029_030_031_paid_path_errors_and_exact_messages`<br>`cargo:billing-postgres:bil_030_031_a_refused_batch_leaves_every_job_status_and_the_ledger_untouched` |
| BIL-031 | `cargo:billing-core:charge::tests::bil_029_030_031_paid_path_errors_and_exact_messages` |
| BIL-032 | `cargo:billing-core:charge::tests::bil_032_jobs_started_one_after_another_each_deduct_their_own_duration`<br>`cargo:billing-postgres:bil_025_026_027_032_jobs_are_billed_once_at_start_and_a_repeat_start_charges_nothing` |
| BIL-033 | `cargo:billing-core:charge::tests::bil_033_a_job_larger_than_the_first_package_draws_from_two` |
| BIL-034 | `cargo:billing-core:charge::tests::bil_034_035_provider_lengths_round_up_and_missing_or_zero_means_nothing`<br>`cargo:billing-core:charge::tests::bil_034_036_deferred_charge_is_strict_and_a_failure_is_only_reported`<br>`cargo:billing-postgres:bil_028_033_034_036_a_deferred_charge_that_fails_still_commits_delivery_and_repeats_charge_once` |
| BIL-035 | `cargo:billing-core:charge::tests::bil_034_035_provider_lengths_round_up_and_missing_or_zero_means_nothing` |
| BIL-036 | `cargo:billing-core:charge::tests::bil_034_036_deferred_charge_is_strict_and_a_failure_is_only_reported`<br>`cargo:billing-postgres:bil_028_033_034_036_a_deferred_charge_that_fails_still_commits_delivery_and_repeats_charge_once`<br>`cargo:billing-postgres:bil_036_a_sql_failure_inside_a_deferred_debit_is_isolated_by_a_savepoint_and_delivery_still_commits` |
| BIL-037 | `cargo:billing-core:charge::tests::bil_037_a_shorter_provider_length_only_lowers_the_duration` |
| BIL-038 | `cargo:billing-core:charge::tests::bil_038_web_start_waits_for_every_duration` |
| BIL-039 | `cargo:billing-core:charge::tests::bil_039_040_web_refuses_below_zero_and_api_at_zero_or_less` |
| BIL-040 | `cargo:billing-core:charge::tests::bil_039_040_web_refuses_below_zero_and_api_at_zero_or_less` |
| BIL-041 | `cargo:billing-core:charge::tests::bil_041_database_messages_map_to_402_in_any_letter_case` |
| BIL-042 | `cargo:billing-postgres:bil_042_a_hard_delete_cascades_every_imported_row_of_the_subject_across_accounts_and_signs`<br>`cargo:billing-postgres:bil_042_a_hard_delete_returns_the_minutes_and_failed_jobs_keep_their_charge`<br>`cargo:billing-postgres:bil_042_def_001_a_debit_of_an_affected_owner_waits_for_a_negative_imported_row_to_go_and_cannot_overspend`<br>`cargo:billing-postgres:bil_042_def_001_a_debit_of_an_affected_owner_waits_for_an_imported_refund_and_spends_it` |
| BIL-043 | `cargo:billing-core:grants::tests::bil_043_a_package_set_is_granted_on_pending_or_failed_to_completed_only`<br>`cargo:billing-postgres:bil_043_044_059_060_067_070_completion_grants_once_with_one_event_and_a_repeat_changes_nothing`<br>`cargo:billing-postgres:bil_043_058_an_authenticated_completion_recovers_an_imported_canceled_payment_and_grants_exactly_once` |
| BIL-044 | `cargo:billing-core:grants::tests::bil_044_048_049_yearly_example_is_twelve_packages_one_month_apart_with_one_name`<br>`cargo:billing-postgres:bil_043_044_059_060_067_070_completion_grants_once_with_one_event_and_a_repeat_changes_nothing` |
| BIL-045 | `cargo:billing-core:calendar::tests::bil_045_month_addition_clamps_to_the_last_day_and_keeps_time_of_day`<br>`cargo:billing-core:grants::tests::bil_045_any_other_interval_raises_and_names_it`<br>`cargo:billing-core:grants::tests::bil_045_frequency_greater_than_one_and_year_intervals`<br>`cargo:billing-core:grants::tests::bil_045_frequency_steps_are_added_one_at_a_time_from_the_previous_start`<br>`cargo:billing-postgres:bil_045_048_055_069_a_yearly_plan_grants_twelve_packages_and_binds_the_provider_subscription`<br>`cargo:billing-postgres:bil_045_055_a_replayed_renewal_starts_its_packages_when_its_notification_arrived_not_when_it_was_resolved`<br>`cargo:billing-postgres:bil_045_an_invalid_package_interval_rolls_back_the_whole_completion` |
| BIL-046 | `cargo:billing-core:grants::tests::bil_046_monthly_example_is_one_package_for_two_months` |
| BIL-047 | `cargo:billing-core:grants::tests::bil_047_pay_as_you_go_example_is_one_year` |
| BIL-048 | `cargo:billing-core:grants::tests::bil_044_048_049_yearly_example_is_twelve_packages_one_month_apart_with_one_name`<br>`cargo:billing-postgres:bil_045_048_055_069_a_yearly_plan_grants_twelve_packages_and_binds_the_provider_subscription` |
| BIL-049 | `cargo:billing-core:grants::tests::bil_044_048_049_yearly_example_is_twelve_packages_one_month_apart_with_one_name` |
| BIL-050 | `cargo:transgate-billing:bil_050_051_052_053_the_four_plans_are_the_captured_packages`<br>`cargo:transgate-billing:bil_050_053_cpn_003_each_plan_and_currency_sends_its_source_plan_code_and_price_to_the_provider` |
| BIL-051 | `cargo:transgate-billing:bil_050_051_052_053_the_four_plans_are_the_captured_packages` |
| BIL-052 | `cargo:transgate-billing:bil_050_051_052_053_the_four_plans_are_the_captured_packages` |
| BIL-053 | `cargo:transgate-billing:bil_050_051_052_053_the_four_plans_are_the_captured_packages`<br>`cargo:transgate-billing:bil_050_053_cpn_003_each_plan_and_currency_sends_its_source_plan_code_and_price_to_the_provider` |
| BIL-054 | `cargo:transgate-billing:bil_054_056_pay_as_you_go_is_149_per_hour_for_a_year_under_the_stored_name` |
| BIL-055 | `cargo:billing-core:grants::tests::bil_055_a_renewal_expands_from_the_renewal_row_time`<br>`cargo:billing-postgres:bil_045_048_055_069_a_yearly_plan_grants_twelve_packages_and_binds_the_provider_subscription`<br>`cargo:billing-postgres:bil_045_055_a_replayed_renewal_starts_its_packages_when_its_notification_arrived_not_when_it_was_resolved`<br>`cargo:transgate-billing:bil_055_a_yearly_renewal_grants_twelve_packages_and_a_monthly_one` |
| BIL-056 | `cargo:billing-core:presentation::tests::bil_056_065_the_plan_box_shows_only_premium_or_business`<br>`cargo:transgate-billing:bil_054_056_pay_as_you_go_is_149_per_hour_for_a_year_under_the_stored_name` |
| BIL-057 | `cargo:billing-postgres:bil_057_058_061_after_identification_distinct_orders_grant_once_and_failure_and_success_are_distinct_observations`<br>`cargo:billing-postgres:bil_057_a_provider_identity_is_unique_per_environment` |
| BIL-058 | `cargo:billing-core:grants::tests::bil_058_a_late_failure_never_undoes_a_completion_and_repeats_change_nothing`<br>`cargo:billing-postgres:bil_043_058_an_authenticated_completion_recovers_an_imported_canceled_payment_and_grants_exactly_once`<br>`cargo:billing-postgres:bil_057_058_061_after_identification_distinct_orders_grant_once_and_failure_and_success_are_distinct_observations` |
| BIL-059 | `cargo:billing-http:bil_059_060_the_browser_return_accepts_a_form_or_json_and_a_missing_or_unknown_token_is_the_failure_page`<br>`cargo:billing-postgres:bil_043_044_059_060_067_070_completion_grants_once_with_one_event_and_a_repeat_changes_nothing` |
| BIL-060 | `cargo:billing-http:bil_059_060_the_browser_return_accepts_a_form_or_json_and_a_missing_or_unknown_token_is_the_failure_page`<br>`cargo:billing-iyzico:bil_060_only_success_and_failure_are_final_payment_statuses`<br>`cargo:billing-postgres:bil_043_044_059_060_067_070_completion_grants_once_with_one_event_and_a_repeat_changes_nothing` |
| BIL-061 | `cargo:billing-postgres:bil_057_058_061_after_identification_distinct_orders_grant_once_and_failure_and_success_are_distinct_observations` |
| BIL-062 | `cargo:billing-core:presentation::tests::bil_062_a_canceled_subscription_keeps_its_minutes_and_hides_the_next_payment`<br>`cargo:billing-http:bil_062_cancellation_over_http_is_owner_only_and_validates_its_body`<br>`cargo:billing-postgres:bil_062_only_the_owner_can_request_cancellation_which_keeps_the_minutes_and_queues_the_support_mail` |
| BIL-063 | `cargo:billing-core:presentation::tests::bil_063_next_payment_is_created_plus_count_intervals_and_empty_when_it_cannot_be_known` |
| BIL-064 | `cargo:billing-core:presentation::tests::bil_064_renewal_text_is_30_days_monthly_365_yearly_and_none_for_pay_as_you_go` |
| BIL-065 | `cargo:billing-core:presentation::tests::bil_056_065_the_plan_box_shows_only_premium_or_business` |
| BIL-066 | `cargo:transgate-billing:bil_066_075_076_checkout_history_admin_lookup_and_actual_buyer_sales_and_support_mail_over_http`<br>`cargo:transgate-billing:bil_066_failed_buyer_mail_recovers_after_a_restart_sending_only_the_missing_recipient_with_the_same_key` |
| BIL-067 | `cargo:billing-postgres:bil_043_044_059_060_067_070_completion_grants_once_with_one_event_and_a_repeat_changes_nothing`<br>`cargo:billing-postgres:bil_067_no_row_is_written_when_the_provider_does_not_answer_and_no_form_is_returned_for_an_unsaved_token` |
| BIL-068 | `cargo:billing-http:bil_068_every_protected_route_refuses_missing_and_invalid_sessions_and_checkout_is_post_only` |
| BIL-069 | `cargo:billing-iyzico:bil_069_subscription_start_sends_the_plan_code_active_status_locale_and_unique_conversation_id`<br>`cargo:billing-postgres:bil_045_048_055_069_a_yearly_plan_grants_twelve_packages_and_binds_the_provider_subscription` |
| BIL-070 | `cargo:billing-iyzico:bil_070_071_pay_as_you_go_start_and_result_use_the_captured_fields_and_a_valid_signature`<br>`cargo:billing-postgres:bil_043_044_059_060_067_070_completion_grants_once_with_one_event_and_a_repeat_changes_nothing` |
| BIL-071 | `cargo:billing-iyzico:bil_070_071_pay_as_you_go_start_and_result_use_the_captured_fields_and_a_valid_signature` |
| BIL-072 | `cargo:billing-core:notify::tests::bil_072_analytics_a_failed_payment_carries_its_reason_and_plan`<br>`cargo:billing-core:notify::tests::bil_072_analytics_a_first_subscription_payment_adds_subscription_started_and_a_renewal_does_not`<br>`cargo:billing-core:notify::tests::bil_072_analytics_a_pay_as_you_go_completion_is_one_payment_completed_event_with_the_source_properties`<br>`cargo:billing-core:notify::tests::bil_072_analytics_account_creation_is_one_user_signed_up_event_and_other_events_plan_none`<br>`cargo:billing-core:presentation::tests::bil_072_events_follow_a_real_status_change_only_and_the_countdown_is_five_seconds`<br>`cargo:transgate-billing:bil_072_purchase_and_signup_events_reach_posthog_once_with_the_sources_ids_and_a_refusing_service_is_retried_with_the_same_identity` |
| BIL-073 | `cargo:billing-core:presentation::tests::bil_073_balance_text_is_minutes_under_an_hour_then_hours_and_minutes`<br>`cargo:billing-http:bil_008_073_an_account_whose_only_package_expired_shows_zero_minutes_and_no_balance_value` |
| BIL-074 | `cargo:billing-core:presentation::tests::bil_074_a_started_answer_is_derived_from_the_state_after_the_debit`<br>`cargo:billing-postgres:bil_074_started_answers_carry_the_balance_after_the_debit_and_the_single_free_package_flag` |
| BIL-075 | `cargo:billing-core:presentation::tests::bil_075_history_rows_carry_the_invoice_id_date_amount_package_and_gateway`<br>`cargo:billing-postgres:bil_075_076_history_lists_completed_payments_newest_first_and_admin_lookup_returns_the_real_customer`<br>`cargo:transgate-billing:bil_066_075_076_checkout_history_admin_lookup_and_actual_buyer_sales_and_support_mail_over_http` |
| BIL-076 | `cargo:billing-http:bil_076_admin_lookup_answers_401_403_400_404_and_200_with_the_customers_own_history`<br>`cargo:billing-postgres:bil_075_076_history_lists_completed_payments_newest_first_and_admin_lookup_returns_the_real_customer`<br>`cargo:transgate-billing:bil_066_075_076_checkout_history_admin_lookup_and_actual_buyer_sales_and_support_mail_over_http` |
| BIL-077 | `cargo:billing-core:charge::tests::bil_077_no_fraud_gate_exists_a_tiny_first_job_is_charged_normally` |
| BIL-078 | `cargo:billing-core:grants::tests::bil_078_canceled_is_a_known_status_that_no_use_case_produces`<br>`cargo:billing-postgres:bil_078_historical_free_text_gateways_and_canceled_status_are_preserved_on_read` |
| BIL-079 | `cargo:billing-core:charge::tests::bil_025_026_079_only_draft_ready_uploaded_to_not_started_bills` |
| CPN-001 | `cargo:billing-core:pricing::tests::cpn_001_currency_follows_the_locale_and_only_usd_and_try_exist` |
| CPN-002 | `cargo:billing-core:pricing::tests::cpn_002_the_rate_falls_back_to_44_and_try_is_rounded_once_not_left_as_a_float_tail` |
| CPN-003 | `cargo:billing-core:pricing::tests::cpn_003_try_plan_quote_is_usd_price_times_rate_with_the_plan_code_for_the_currency`<br>`cargo:transgate-billing:cpn_003_try_plan_amounts_are_the_usd_price_times_the_rate_and_each_plan_and_currency_selects_its_source_plan_code` |
| CPN-004 | `cargo:billing-core:pricing::tests::cpn_004_pay_as_you_go_is_149_cents_an_hour_exactly` |
| CPN-005 | `cargo:billing-core:pricing::tests::cpn_005_the_hours_control_clamps_steps_and_warns` |
| CPN-006 | `cargo:billing-core:pricing::tests::cpn_006_the_locale_discount_halves_the_whole_total_and_the_page_halves_the_hourly_price`<br>`cargo:transgate-billing:cpn_006_007_the_locale_discount_and_the_special_offer_are_catalog_data` |
| CPN-007 | `cargo:billing-core:pricing::tests::cpn_007_the_special_offer_is_try_per_hour_and_ignores_coupon_locale_and_rate`<br>`cargo:billing-postgres:cpn_007_019_eligibility_comes_from_host_data_never_from_the_request`<br>`cargo:transgate-billing:cpn_006_007_the_locale_discount_and_the_special_offer_are_catalog_data` |
| CPN-008 | `cargo:billing-core:pricing::tests::cpn_008_009_010_a_coupon_is_a_percentage_for_the_exactly_named_product_only`<br>`cargo:billing-postgres:cpn_008_to_013_coupons_are_read_as_stored_matched_exactly_and_the_id_is_kept_when_it_did_not_apply` |
| CPN-009 | `cargo:billing-core:pricing::tests::cpn_008_009_010_a_coupon_is_a_percentage_for_the_exactly_named_product_only` |
| CPN-010 | `cargo:billing-core:pricing::tests::cpn_008_009_010_a_coupon_is_a_percentage_for_the_exactly_named_product_only` |
| CPN-011 | `cargo:billing-core:pricing::tests::cpn_011_a_coupon_expires_strictly_after_its_instant_and_null_never_expires` |
| CPN-012 | `cargo:billing-core:pricing::tests::cpn_012_the_coupon_field_never_fetches_twice_clears_on_empty_and_locks_on_success`<br>`cargo:billing-http:cpn_012_the_coupon_route_reports_the_screen_messages` |
| CPN-013 | `cargo:billing-core:pricing::tests::cpn_013_a_valid_coupon_cuts_the_usd_amount_before_the_try_conversion`<br>`cargo:billing-core:pricing::tests::cpn_013_the_server_keeps_an_existing_unexpired_coupon_id_even_when_it_did_not_apply` |
| CPN-014 | `cargo:billing-postgres:cpn_014_one_coupon_is_used_again_by_the_same_account_and_by_another_with_the_same_discount_and_saved_id` |
| CPN-015 | `cargo:billing-core:pricing::tests::cpn_015_016_plan_cards_show_monthly_usd_struck_prices_and_the_fixed_badge`<br>`cargo:transgate-billing:cpn_015_016_displayed_prices_are_per_month_with_the_captured_reference_prices_and_the_fixed_badge` |
| CPN-016 | `cargo:billing-core:pricing::tests::cpn_015_016_plan_cards_show_monthly_usd_struck_prices_and_the_fixed_badge`<br>`cargo:transgate-billing:cpn_015_016_displayed_prices_are_per_month_with_the_captured_reference_prices_and_the_fixed_badge` |
| CPN-017 | `cargo:billing-core:pricing::tests::cpn_017_analytics_reports_try_payments_in_usd_with_the_same_rate` |
| CPN-018 | `cargo:billing-core:pricing::tests::cpn_018_try_invoices_treat_the_amount_as_vat_inclusive_at_20_percent` |
| CPN-019 | `cargo:billing-core:pricing::tests::cpn_019_the_special_client_alert_shows_the_host_supplied_text_verbatim`<br>`cargo:billing-postgres:cpn_007_019_eligibility_comes_from_host_data_never_from_the_request` |
| FRE-001 | `cargo:billing-http:fre_001_signup_provisioning_is_service_authenticated_and_anchored_to_the_reported_instant`<br>`cargo:billing-postgres:fre_001_002_first_access_months_after_signup_gets_the_original_expired_window_not_a_new_trial`<br>`cargo:billing-postgres:fre_001_a_user_with_no_trusted_signup_record_is_refused_not_granted_a_trial`<br>`cargo:billing-postgres:fre_001_concurrent_provisioning_races_grant_exactly_one_package` |
| FRE-002 | `cargo:billing-postgres:fre_001_002_first_access_months_after_signup_gets_the_original_expired_window_not_a_new_trial` |
| FRE-003 | `cargo:billing-core:charge::tests::fre_003_freemium_only_means_no_open_package_with_another_name` |
| FRE-004 | `cargo:billing-core:charge::tests::fre_004_005_first_job_bypasses_and_writes_one_full_row_on_the_earliest_package`<br>`cargo:billing-postgres:fre_004_006_two_first_jobs_started_before_either_completes_both_bypass_in_turn` |
| FRE-005 | `cargo:billing-core:charge::tests::fre_004_005_first_job_bypasses_and_writes_one_full_row_on_the_earliest_package` |
| FRE-006 | `cargo:billing-core:charge::tests::fre_006_eligibility_is_completed_jobs_so_two_unfinished_first_jobs_both_bypass`<br>`cargo:billing-postgres:fre_004_006_two_first_jobs_started_before_either_completes_both_bypass_in_turn` |
| FRE-007 | `cargo:billing-core:charge::tests::fre_007_later_freemium_jobs_are_strict`<br>`cargo:billing-postgres:fre_007_010_after_a_completed_job_the_next_start_is_strict_and_refused_when_overdrawn` |
| FRE-008 | `cargo:billing-core:charge::tests::fre_008_009_a_second_job_that_fits_exactly_passes_and_exhaustion_refuses_everything` |
| FRE-009 | `cargo:billing-core:charge::tests::fre_008_009_a_second_job_that_fits_exactly_passes_and_exhaustion_refuses_everything` |
| FRE-010 | `cargo:billing-core:charge::tests::fre_010_after_an_overdrawn_first_job_the_next_one_is_refused`<br>`cargo:billing-postgres:fre_007_010_after_a_completed_job_the_next_start_is_strict_and_refused_when_overdrawn` |
| FRE-011 | `cargo:billing-core:charge::tests::fre_011_known_duration_freemium_jobs_write_rows_totaling_their_duration_and_unknown_ones_defer` |
| FRE-012 | `cargo:billing-core:charge::tests::fre_012_an_expired_free_package_refuses_a_first_and_a_later_job` |
| FRE-013 | `cargo:billing-core:freemium::tests::fre_013_app_definition_one_is_exactly_one_package_row_named_freemium_any_dates`<br>`cargo:billing-postgres:fre_013_014_the_overview_keeps_the_three_freemium_facets_apart` |
| FRE-014 | `cargo:billing-core:freemium::tests::fre_014_app_definition_two_ignores_start_dates_and_counts_a_query_error_as_freemium`<br>`cargo:billing-postgres:fre_013_014_the_overview_keeps_the_three_freemium_facets_apart` |
| FRE-015 | `cargo:billing-core:freemium::tests::fre_015_language_and_first_translation_target_are_lower_cased_exceptions`<br>`cargo:billing-core:freemium::tests::fre_015_limits_are_inclusive_and_exceptions_raise_the_cap_to_45`<br>`cargo:transgate-billing:fre_015_018_the_real_lists_drive_the_web_guard_by_the_lowest_draft_id` |
| FRE-016 | `cargo:billing-core:freemium::tests::fre_016_017_the_english_locale_is_not_an_exception`<br>`cargo:transgate-billing:fre_016_017_the_exception_lists_are_exactly_the_ledgers_48_locales_and_37_languages` |
| FRE-017 | `cargo:billing-core:freemium::tests::fre_016_017_the_english_locale_is_not_an_exception`<br>`cargo:transgate-billing:fre_016_017_the_exception_lists_are_exactly_the_ledgers_48_locales_and_37_languages` |
| FRE-018 | `cargo:billing-core:freemium::tests::fre_018_only_the_first_draft_by_id_decides_the_exception_in_any_request_order`<br>`cargo:transgate-billing:fre_015_018_the_real_lists_drive_the_web_guard_by_the_lowest_draft_id` |
| FRE-019 | `cargo:billing-core:charge::tests::fre_019_the_deferred_charge_never_uses_the_first_job_bypass` |
| FRE-020 | `cargo:billing-core:freemium::tests::fre_020_display_cut_keeps_lines_up_to_22_minutes_and_fades_the_last_three` |
| FRE-021 | `cargo:billing-core:freemium::tests::fre_021_load_cut_keeps_lines_to_21_minutes_plus_three_and_stops_live_updates` |
| FRE-023 | `cargo:billing-core:freemium::tests::fre_023_the_banner_progress_is_capped_at_one_hundred_percent` |
| FRE-024 | `cargo:billing-core:freemium::tests::fre_024_unparseable_or_missing_chat_caps_mean_no_limit` |
| FRE-025 | `cargo:billing-core:charge::tests::fre_025_and_bil_079_partial_transcription_and_resume_are_not_modeled` |
