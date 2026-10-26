# Astra P3 transport source handoff

Source complete; **not compiled or executed by this owner**. Lead owns integrated checks/runtime. No root Cargo/lock/ledger edits, hidden agents, source Transgate edits or provider/customer API calls. Public docs and local synthetic fixtures only.

## Deliverables and integration

- `crates/media-ports/{Cargo.toml,src/lib.rs}`: shared Send+Sync/BoxFuture ports, explicit identities/origins, object capabilities, nullable source token fields, full provider DTOs with unknown-field retention and redacted Debug. No product routing/normalization/billing.
- `crates/media-adapters/src/{http,transcription,storage,fetch,captions}.rs`: real reqwest providers, local HMAC GCS/R2, erasure-only legacy Supabase, pinned-DNS external staging/customer POST, separate paid/public captions and metadata.
- `modules/billing/gateways/iyzico/src/client.rs`: implements PG-owned `billing_core::erasure::SubscriptionTermination`; only a bound signed cancel or authenticated detail is reported. Core decides D10. Non-2xx contradictory success is uncertain; redirects/retries disabled.
- Exact manifests are saved and coherent. Adapter test-only additions: `hmac=0.12`, `sha2=0.10` for an independent SigV4 verifier. No billing dependency in shared media crates.
- Constructors/config structs are exported from `media_adapters`; adapters read no environment. Host receives config/env mapping directly through herdr. Required GCS credentials are HMAC XML keys, not an implicit service-account/IAM call.
- Final callback seam: `CallbackBinding { attempt, provider, project, expected_remote_id: Option<String>, secret }`. `None` supports callback-before-ack; core adopts or rejects staleness. Host maps Unauthorized/IdentityMismatch/Malformed to 401/409/400 and returns 2xx only after durable accepted/duplicate handling. `complete_callback` performs asynchronous Soniox transcript retrieval separately.

## Rule / regression map

Paths below are under `crates/media-adapters` except Iyzico. These are **written regression cases**, not a claim of passing runs.

| Rules / decision | Test and substantive assertions |
|---|---|
| TRX-015..019, TRL-002, D11 | `tests/transcription.rs::soniox_real_post_keeps_source_reference_and_explicit_v5_language_translation_auth`: actual HTTP model v5, source job_N, selected lower-case hint or absent auto hint, diarization, one-way target, signed audio capability and callback authentication. |
| TRX-024..030, TRX-034, D6 | `d6_soniox_callback_preserves_received_duration_and_full_tokens_without_status_enrichment`: absent and positive webhook durations unchanged; original/translation, string/numeric speaker, null times, audio events and unknown fields retained; only transcript GETs observed. `recovery_poll_owns_its_duration_and_mismatched_transcript_identity_is_rejected`: separate poll origin/duration and wrong-id rejection. Normalization/billing assertions belong core. |
| D8, callback-before-ack W3 | `callbacks_authenticate_before_parse_and_bind_provider_project_remote_with_full_deepgram_body`: unauthorized malformed bytes rejected before parsing, duplicate headers rejected, post-ack id/project/provider mismatch rejected; pre-ack authenticated identity returned; full Deepgram metadata/channels/alternatives/words/paragraphs survives durable serialization. |
| TRX-012/020/023, D3/D4 | `d3_d4_deepgram_global_whisper_request_is_opted_out_and_basic_authenticated`: all 61 captured offered codes serialized through actual HTTP; Whisper, mandatory opt-out and Basic callback credentials. Live code support is unproved. Unsupported recovery/deletion is explicit. |
| Submit uncertainty, recovery identity, TRX-041/PRV-037 | `post_ambiguity_does_not_become_rejection_or_retry_and_explicit_429_is_rejected`; `soniox_listing_preserves_all_ambiguous_matches_and_delete_processing_is_not_success`: lost/malformed/5xx response stays uncertain with one POST, explicit 429 rejection; all paged matching identities retained for core reconciliation; processing delete not success. |
| UPL-010..012/016, PRV-035 | `tests/storage.rs::gcs_and_r2_signed_access_binds_key_method_expiry_without_content_type_or_resumable_upload`: independent signature verifier rejects method/key tampering, 900/3600 expiry and no content-type pin. `real_storage_put_head_and_versioned_delete_keep_observed_object_identity`: actual bytes and generation survive PUT/HEAD/DELETE. `retention_absence_permission_and_provider_failures_are_not_all_deleted`: retention, absence and failures remain distinct. |
| D2 legacy storage erasure | `d2_legacy_supabase_inventory_and_delete_use_service_authority_and_confirm_empty_success`: account-prefix inventory, folder distinction, authenticated fixed jobs bucket delete; exact distinction between authenticated absence, unconfirmed deletion of an existing object, failed lookup and malformed Info. `d2_legacy_supabase_info_reads_root_storage_fields_not_user_metadata`: official root size/type/etag with empty, null and misleading user metadata; zero size and nullable optional fields. |
| UPL-030, controlled external staging | `src/fetch/tests.rs`: public/private/reserved/tunnel address boundaries; actual pinned GET → anonymous spool → private PUT; direct and redirected private destinations denied before connect/upload; bounded redirect loop; customer POST payload never forwarded on redirect. No release-build private-IP override. |
| UPL-026/027 | `tests/captions.rs::paid_caption_wire_is_authenticated_timestamped_and_never_uses_public_fallback`: real paid HTTP options/cues and no-captions/age/rate/network/unknown classes; never calls public service. |
| UPL-029/038 transport subset | `public_srv3_milliseconds_and_legacy_seconds_are_parsed_by_protocol_not_duration_guess`; `public_language_missing_rate_limit_and_hostile_track_url_do_not_fall_through_to_paid_or_arbitrary_hosts`: distinct public protocol, exact cue units, entities, requested track, forbidden upstream destinations and no paid fallback. `public_innertube_status_and_parse_failures_use_one_watch_fallback`, `public_innertube_network_timeout_uses_one_watch_fallback`, `public_fallback_preserves_final_watch_and_caption_failures`: bounded source-defined discovery and final error propagation. Host owns cache/rate/formatting. |
| UPL-023/024 language and metadata transport | `metadata_preserves_raw_language_precedence_and_provider_duration_without_product_fallbacks`: raw audio-language wins, default-language fallback, original metadata retained, inaccessible video classification. Core owns English/base-tag mapping and minutes. |
| D10 erasure-only recurring cancellation | `modules/billing/gateways/iyzico/tests/protocol.rs` three `d10_*` cases: independent IYZWSv2 verification of encoded cancel POST, environment binding, authenticated CANCELED/EXPIRED versus ACTIVE/wrong-customer detail, ambiguous detail, lost/malformed/5xx/401 contradictions; no hidden repeat POST. |

## Lead commands and remaining integration authority

Run after all owners' source handoffs, not as parallel mid-flight checks:

```sh
cargo test -p media-ports -p media-adapters -p billing-iyzico
```

Then lead's integrated workspace suite and real taskr/Transgate binary scenarios. Exercise both providers via synthetic HTTP, signed upload/provider access, callback-before-ack and duplicate/mismatch, external staging/SSRF rejection, public/paid captions, and erasure outcomes through the hosts. No production acceptance should be inferred from fixture passes.

## Comments

- Known live acceptance/configuration limits and primary sources are in `crates/media-adapters/PROVENANCE.md`; successful synthetic discovery does not prove deployed availability.
- Public YouTube XML units are read from the actual schema instead of the source wrapper's heuristic. Explicit InnerTube → watch-page discovery runs once when no track is advertised or InnerTube fails (network, non-2xx, malformed JSON), matching the captured dependency. Final watch-page/track errors propagate; no endpoint retry loop or paid-service fallback.
- HTTP(S) signed audio capabilities are accepted (including controlled local storage fixtures); provider callback URLs require authenticated HTTPS and exactly one `attempt` query parameter. Deepgram callback ports follow its documented allowlist.
- D9 authorization/barrier/manifest sequencing, public success-cache TTL/500-entry cap, per-user/IP limits, scheduling, normalization, routing, retention completion and adoption of early callback identities remain their assigned core/PG/host responsibilities, not unimplemented transport work.

## Transport review repairs — source-ready, execution pending

Both findings in `astra/transport-review.md` are implemented. Supabase Info now reads documented root fields, not user metadata. Public discovery restores the captured bounded fallback instead of the previously unsupported narrowing. No public signature, dependency, manifest or lock changes. Reviewer findings remain historical evidence; this section is the repair handoff, not a claim that lead verification has passed.

Changed source/tests: `crates/media-adapters/src/{storage,captions}.rs`, `crates/media-adapters/tests/{storage,captions}.rs`. Provenance updated in `crates/media-adapters/PROVENANCE.md`.

Exact new/changed regression tests:

- `tests/storage.rs`: `d2_legacy_supabase_info_reads_root_storage_fields_not_user_metadata`
- `tests/storage.rs`: `d2_legacy_supabase_inventory_and_delete_use_service_authority_and_confirm_empty_success`
- `tests/captions.rs`: `public_innertube_status_and_parse_failures_use_one_watch_fallback`
- `tests/captions.rs`: `public_innertube_network_timeout_uses_one_watch_fallback`
- `tests/captions.rs`: `public_fallback_preserves_final_watch_and_caption_failures`
- `tests/captions.rs`: `public_language_missing_rate_limit_and_hostile_track_url_do_not_fall_through_to_paid_or_arbitrary_hosts`

The timeout regression uses a local delayed response beyond a one-second client deadline, then successful watch-page discovery and caption retrieval. The other fixtures assert exact final error classes and bounded request sequences; deletion reconciliation no longer passes merely because any parsing error occurred.

Lead verification command (not run by this owner):

```sh
cargo test --locked -p media-adapters --test storage --test captions
```

No builds, tests, formatters or provider/customer calls were made for these repairs. Public documentation and captured local source were read; integrated execution and smoke proof remain with lead.
