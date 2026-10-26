# P3 decisions

Recorded 2026-10-24. The PRD remains the product contract. Final rule and parity obligations are not reduced by phase sequencing. Public documentation and synthetic fixtures are not live provider acceptance.

## D1 Free-tool processor belongs to P3 backend work

Lead decision: implement the server-first FFmpeg processor/session/download backend in P3, with a separate runtime service as captured in P0. Its pages/rendering remain P5/P6. Static CMS must not become the owner of runtime processing. All seven families, 39 tool endpoints plus health and their cleanup obligations remain inventoried. No browser-only substitution, no fake endpoint scaffolds. This is sequencing, not new product scope.

## D2 Legacy Supabase storage is a real erasure target

Lead decision: include the narrow legacy `jobs`-bucket storage adapter in P3 rather than declaring erasure complete while deferring those objects. Use configured service authority and real documented storage APIs, with loopback protocol fixtures. P9 still supplies the actual object inventory/migration evidence. An unconfigured, retained or failed external target stays an explicit unresolved saga item, never a fabricated successful deletion.

The adapter must be reachable from erasure: fully enumerate the configured account prefix and persist its objects before completion. Incomplete or unavailable listing cannot be interpreted as an empty prefix. P9 supplies deployed inventory/cutover proof, not a substitute for wiring and exercising this capability in P3.

For this Transgate migration, `TRANSGATE_JOBS_ENABLED=1` requires the dedicated `LEGACY_SUPABASE_URL` and `LEGACY_SUPABASE_SERVICE_ROLE_KEY` pair, without silently borrowing auth credentials. Taskr is unchanged. Removing that dependency requires P9 evidence that legacy resources were migrated or erased and old writers drained; the core still reports a missing inventory adapter as unresolved.

## D3 Deepgram model-improvement opt-out

Mu explicitly selected **Opt out**. Every Deepgram transcription request sends `mip_opt_out=true`. This intentionally changes the captured omitted-flag behavior. Primary documentation says audio/transcripts are retained only for processing; asynchronous callback results remain until delivery/retry exhaustion, and request metadata/usage logs remain. Do not call this removal of all provider records. Pricing impact and actual provider-account behavior remain unverified before deployment.

Source: https://developers.deepgram.com/trust-security/your-data

## D4 Global Whisper is permitted; EU-only claim is not

Mu explicitly selected **Allow global Whisper** to preserve the current provider path and all 61 offered transcription languages. Deepgram documents no Whisper support on the EU endpoint. The rewrite must not claim all processing is EU-only. All eight disputed language codes remain supported inputs/routing obligations; their live provider acceptance remains a prerequisite, not a fabricated fixture result. The privacy/legal copy must be reconciled in its owning phase.

Sources: https://developers.deepgram.com/docs/deepgram-whisper-cloud and https://developers.deepgram.com/trust-security/your-data

## D5 Financial erasure preserves captured behavior

Mu explicitly selected **Preserve captured behavior**: account deletion cascades payments, packages and usage; billing-info fields survive with the owner link removed. This is not anonymization. Do not replace it with an invented accounting archive or silently erase the remaining billing profile. Record the privacy-policy discrepancy and keep retained-data reporting truthful. This choice does not by itself settle active external subscription cancellation or deletion-barrier retention.

## D6 Preserve Soniox observation-origin money

Lead applies the PRD zero-change default: webhook and recovery paths retain their captured duration precedence. A positive webhook-reported audio duration may be used; absent data stays absent and follows the source token-end fallback. A status GET must not silently enrich webhook billable minutes. Recovery keeps its own polled observation. Unifying these may change money and is not approved. The source-internal `metadata.duration_ms` name is not proof of a provider webhook field.

## D7 Preserve last-write-wins line editing

Source last-write-wins remains the user-visible rule. Internal account/job locks and lifecycle checks prevent edits from resurrecting erased data; updates never become inserts/upserts. Do not add expected-revision conflicts to the editor without a separate decision. Job lifecycle and provider-attempt fencing still use revisions where required.

## D8 Cutover has no unsigned callback bypass

Lead's P9 default is to drain or explicitly reconcile legacy Deepgram/RunPod attempts before routing callbacks to the new service. P3 callbacks require the configured per-attempt authentication. No permanent unsigned compatibility path. Actual in-flight inventory and drain evidence remain P9 prerequisites.

## D9 Erasure extension and surviving barrier

The billing account-erasure seam is P3 module-extension work, not an unimplemented P2 billing rule. It discovers the union of affected owners before taking any lock, locks in sorted order and rechecks, then performs the complete captured cascade in the host-owned transaction. Do not loop the one-subject deletion seam and invert lock order. Both Transgate and the taskr audio-work sample consume it.

Mu delegated the remaining choices to the lead. Keep a minimal durable erasure barrier for the lifetime of this deployment's identity namespace; do not expire it on a guessed callback/token window. Retain only the identifiers, timestamps and outcomes necessary to reject recreation or finish outstanding cleanup, not names, addresses, transcripts or raw callback bodies. Completed cleanup drops its no-longer-needed target details. This is pseudonymous security state, not anonymized data; privacy copy must say so. No general retention framework is added.

## D10 Stop future subscription charges before completing erasure

Lead decision under mu's delegation: fence new work first, then automatically cancel each verified external recurring subscription as part of account erasure, outside database transactions. This is an explicit correction to the unsafe outcome of erasing an account while its subscription can keep charging. Ordinary subscription cancellation remains the captured manual-support flow; no refund policy is introduced.

Use Iyzico's authenticated `POST /v2/subscription/subscriptions/{subscriptionReferenceCode}/cancel`. A documented success or authenticated, correctly bound `CANCELED`/`EXPIRED` detail proves it no longer renews. Timeout, failure, unknown status, ambiguous legacy ownership or missing configuration leaves erasure visibly pending for retry/reconciliation. Never guess a subscription, treat a generic error as cancellation, or claim the account is fully erased while billing is unresolved. The captured local financial cascade then remains D5.

Primary protocol: https://docs.iyzico.com/en/getting-started/preliminaries/api-reference-beta/subscription/subscription/subscription-transactions

## D11 Use the current explicit Soniox async model

New submissions default to configured `stt-async-v5`. The primary model page lists it as active and documents the retired v3-to-v4 and v4-to-v5 redirects. Keep historical requested/observed model names readable; do not claim a retired v3 identifier proves v3 execution parity. This is a recorded provider-compatibility correction, not a language expansion, duration-threshold change or live quality/price acceptance.

Source: https://soniox.com/docs/stt/models

## D12 Narrow processor failure and resource corrections

Lead accepts applying the existing family upload-size limit to the previously uncapped octet-stream path, prefixing temporary files so crash leftovers are purgeable, and returning an explicit error when an already-checked download cannot be opened. Mark a ZIP downloaded only after its stream is exhausted, not before streaming starts; this is not proof that a remote client received every byte. These correct resource-limit, cleanup and hanging-request defects without changing valid tool options or quotas.

Preserve valid compressed JSON support from the installed source middleware. Invalid explicit configuration must fail visibly rather than silently select another port. On a trim failure, cancel and reap the sibling child instead of leaving it running or delaying the error for its full run. These are implementation acceptance decisions; source-ready code and synthetic fixtures alone do not prove them.

Keep the processor's bounded stored two-entry ZIP layout with maintained CRC32: it preserves exact length and local-header compatibility without buffering or copying a whole archive. The general document exporter uses the maintained ZIP crate. Omit Express branding and weak ETags as framework metadata; preserve the source's actual download semantics, which do not implement Range requests.

Invalidation must cancel and reap the session's active children, fence work that has not started, and prevent late outputs/publication before reporting success. The captured implementation removes only currently registered files; the real gated-child HTTP regression failed with two trim children still alive (lead artifact 544). Correct this resource leak without changing valid options, quota charging, or another session's work. Broader invalidation and timer tests remain unverified until their integrated run.

## D13 Erasure requires a complete recurring-billing inventory

A local pending, failed or canceled checkout is not proof that no subscription exists or can complete later. Preserve its correlation and keep erasure pending until authenticated reconciliation resolves it. Every bound subscription needs confirmed termination coverage. Late authenticated checkout success during erasure may register cleanup identity, never grant packages or revive the account. No risk waiver is approved: operator closure requires explicit attestation of verified checkout non-payability and no remaining renewing subscription, with an evidence reference. An unpaid-at-this-moment snapshot or audit rationale alone is insufficient. If either fact cannot be established, erasure stays pending; do not invent provider session expiry or cancel-session support.

Checkout persistence and ordinary billing operations must check the barrier in a fresh statement after obtaining the account lock, including after external initialization. Expected refusal uses HTTP 410 and `account_erased`; that code means the billing surface is unavailable due to erasure, not that all retained data or external resources are already gone. The saga view remains the authority on completion.

## D14 Preserve retention defaults and adapt operational history

An absent settings row means the source default, automatic transcript deletion enabled; only an explicit false is an opt-out. Preserve the source's 90-day history setting and extra-day cutoff, Sunday 04:00 UTC schedule, range/threshold guards, single-run lock and honest audit. The replacement has no pg_cron: prune finished rows of its own named scheduler slots instead, leaving active claims untouched. This supersedes discovery's tentative pg_cron-specific housekeeping suggestion, not the retention obligation.

## D15 Expiry is not proof that an accepted write finished

A signed URL's expiry prevents new authorized requests; it does not prove that an already accepted PUT cannot commit later. No arbitrary 24-hour grace, local timeout or archive landing margin is approved as erasure proof. Both consumers delete/recheck known objects but retain explicit unresolved upload uncertainty until a supported mechanism or evidence resolves it. Exceptions require explicit approval, never automatic promotion to fully erased.

Server archives persist an intent before dispatch and durably acknowledge known completion even after local cascade, reopening cleanup when needed. Unknown completion remains pending. JOB-044 changes from an upsert at a shared job key to one PUT at `{account}/{job}/{write-id}/{file}` per durable intent. Only that unique object identity or equivalent version evidence can identify the write; an older object at a reused key is insufficient.

An operator may explicitly accept residual in-flight-write risk for an unresolved `UploadResweep` or `ArchiveInFlight`, with a nonempty rationale. The result is `ErasedWithExceptions`, never `FullyErased`; automatic exception policies cannot approve these items. This does not waive D13 billing evidence or unresolved legacy inventory. A later known archive completion reopens cleanup even after an exception-labeled closure.

[R2 documents that a negative read does not wait for or cancel an earlier write, which may still commit afterward](https://developers.cloudflare.com/r2/reference/consistency/). Neither R2 nor GCS supplied a documented hard completion bound in the primary-source review (`opus/upload-finality-evidence.md`). Conditional writes/tombstones are not adopted without both-provider evidence and authorized probes; GCS soft-delete/retention configuration remains deployment evidence, not something synthetic tests prove.

## D16 Providers without reconciliation listings

Preserve bounded supersession after 30 minutes for an unresolved submission only when the provider has no reconciliation listing, currently Deepgram. This can duplicate remote work; it does not permit another local charge/completion or losing a late authenticated remote identity. Soniox listing failure, no match or ambiguous matches remain unresolved instead of triggering this fallback. No remote exactly-once guarantee is claimed.
