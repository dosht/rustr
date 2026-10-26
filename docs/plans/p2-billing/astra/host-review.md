# Final host composition review

Read-only source review of the developer handoff, 2026-10-24. No builds, tests, formatters or implementation edits. Gateway protocols remain Opus's slice. Findings below are source counterexamples, not observed test failures.

## P1: imported cross-account hard-delete retention is an unapproved policy change

Rules: BIL-042, BIL-003/019; D3 preserves refund/delete policy.

`modules/billing/postgres/src/tx.rs:344`, `release_subject`, now removes imported rows only when the referenced package belongs to the supplied account (`:357`). The handoff explicitly admits the exception at `docs/plans/p2-billing/implementation-evidence.md:56`. Counterexample: A owns subject J; imported usage for J consumes 10 minutes from B's package. Hard-deleting J previously cascaded that usage row away and returned B's 10 minutes. This implementation retains the row and the charge indefinitely. An anomaly report does not preserve the deletion outcome.

Required: restore cascade-by-authorized-subject behavior with every affected package owner participating in deterministic account locking before the host deletion and ledger mutation. If that requires a distinct host deletion seam, expose the narrow seam and prove the anomaly case. Do not mark BIL-042 fully covered by own-package tests. Any deliberate retention requires an explicit lead/user policy decision and recorded rule difference; this review does not approve that scope reduction.

## P1: cached JWT signing keys never expire while their kid is reused

Rules: BIL-019/068/075/076 and the actual authenticated-runtime contract.

`transgate/billing/src/identity.rs:83`, `JwtIdentity::key_for`, returns a cached key before checking its age. `crates/taskr-server/src/identity.rs:59`, `SupabaseJwt::key_for`, has the same behavior and is the verifier injected into taskr billing. After a key is removed from the configured JWKS, a running process continues accepting newly signed, unexpired tokens using that key indefinitely unless another request happens to force a refresh. A stolen retired key can therefore mint billing identities and, in Transgate, an allow-listed admin email. The 60-second constant currently throttles misses; it does not bound trust in a cached hit.

Required: apply a bounded cache lifetime to hits as well as misses, refresh stale sets, and reject a removed kid after refresh. Handle refresh failure explicitly rather than accepting stale keys forever. Add a local JWKS rotation/removal case through both actual verifier paths. Current new host tests exercise HS256 and dev identity, not this configured production path.

## P1: Transgate's trusted signup receiver has no production producer

Rules: FRE-001/002/012; the approved consumer-composition closure explicitly requires a real host invocation path.

`transgate/billing/src/directory.rs:35` reads only the new host table. `settings.rs:203` makes its signup service token optional. The only repository calls to `/api/billing/internal/signup` are test clients; there is no auth signup hook, trusted auth-directory fetch, or supplied executable integration that populates `transgate_billing.customers`. `tests/runtime.rs:264` manually reports a signup, and `:321` directly inserts a customer. A valid newly registered production JWT therefore reaches `ensure_provisioned` with no directory record and gets `account_not_provisioned`, despite billing readiness succeeding.

Required: supply and exercise the minimal trusted registration integration, preserving the original identity creation instant, or a trusted host lookup that supplies those facts on delayed access. A documented endpoint alone does not satisfy the already-approved invocation requirement. Do not replace the missing producer with JWT `iat`, request time or client-posted signup facts.

## P1: the allow-listed runtime role's own attributes are not checked

Rules: DEF-002 and the restricted-runtime-authority acceptance condition.

Both compositions correctly call `connect_runtime`. However, `modules/billing/postgres/src/lib.rs:79`, `authority_problems`, checks attributes only for the login being examined, while explicitly excluding `billing_runtime` from the other-role check (`:103`). The migration keeps an existing `billing_runtime` role unchanged (`migrations/0001_billing.sql:220`). Counterexample: an existing group role has `CREATEROLE`; the ordinary login inherits only its normal table grants and passes the check, then can `SET ROLE billing_runtime` and obtain that prohibited authority. Existing contaminated-login tests do not contaminate the allow-listed group itself.

Required: validate the role attributes and effective authority of the allow-listed group too, including authority available after SET ROLE. Refuse a contaminated group at provisioning/startup. The prior direct column-grant findings are addressed in source; this remaining path prevents calling the authority boundary complete.

## P1: missing mail configuration still reports ready and accepts cancellation

Rules: BIL-062/066; EML-BIL-001/002/003.

`transgate/billing/src/settings.rs:162` permits the entire mail group to be absent. `lib.rs:148` then installs neither mailer nor relay. `modules/billing/http/src/routes.rs:128`, `ready`, reports success on a database ping alone. A configured account can buy or receive a successful cancellation response while no buyer/sales/support email can leave this process. Manual cancellation depends on the support email; retaining its outbox row is recoverable storage, not delivery.

Required: make Transgate's required mail configuration part of readiness/startup acceptance, with a usable recovery path for already-pending events after configuration is repaired. Prove the composed HTTP mailer failing, restarting/recovering, and delivering outstanding recipients with stable keys. Current `transgate/billing/tests/runtime.rs:404` explicitly calls the relay and uses an always-successful mail fixture; the Postgres fake-mailer/lease tests do not prove that composed recovery path.

taskr assessment: `crates/taskr-billing/src/lib.rs:143` installs no mailer or relay at all. Its purchase outbox rows therefore remain pending when taskr runs alone. The minimal sample contract does not itself specify Transgate-style taskr emails, so I am not inventing that product policy. The evidence must nevertheless not describe both consumers as exercising outbox delivery/recovery. Supply the intended sample event consumer if that behavior is claimed. Do not use a Transgate relay on the same database as a substitute: it claims the global outbox with Transgate's directory and notification configuration.

## P2: server analytics is still only a presentation value

Rules: BIL-072; prior implementation-review-notes requirement remains open.

The source sends server events via `capturePayment` in `frontend/src/pages/checkout/confirm.tsx:196,220,225`. New `modules/billing/core/src/app.rs:707` only returns `AnalyticsIntent` values; the relay at `:1422` dispatches mail only. No host analytics dispatcher is composed. `implementation-evidence.md:143` maps BIL-072 only to a pure presentation test. Closing the browser after the successful callback therefore loses the server event; completing mail can acknowledge the outbox without analytics ever being sent.

Required: connect the captured server events to an actual host-configured dispatch path with transition deduplication and local-fixture evidence. Keep browser rendering/countdown and client GA delivery separately bounded by P5; those exclusions do not remove the existing server events.

## P2: customer contact details are frozen along with signup time

Rules: BIL-066/075/076; trusted-directory acceptance condition.

`transgate/billing/src/directory.rs:64`, `register`, and `crates/taskr-billing/src/work.rs:81` use `ON CONFLICT(account_id) DO NOTHING` for every field. A trusted later report of a verified email change leaves the old address in the directory. Transgate purchases still email that old inbox and admin lookup by the new address returns no customer. Keeping creation time immutable does not require keeping contact details stale.

Required: provide a trusted contact-refresh path that preserves original creation time and never grants another trial; do not trust arbitrary user-posted recipients. Cover an updated verified address, lookup by the updated address and purchase delivery after refresh. An out-of-order signup replay must not overwrite newer verified contact facts.

## Prior findings addressed in source, pending lead execution

The actual main compositions mount the billing routers. taskr passes its existing `Identities` instance to billing, preserves UUID subjects and deterministically maps dev subjects; Transgate verifies configured JWTs and gates dev tokens explicitly. The protected admin route resolves the target directory account, completed history and latest billing info. Trusted creation time is stored once, and delayed access uses it rather than JWT time. The missing production population path is the finding above.

`crates/taskr-billing/src/work.rs:211` compares units/operation before its done-row replay, returns the saved result and commits work/debit together under the account lock. Mounted HTTP tests cover conflict, account scoping, concurrency and saved-balance replay. `PgBillingTx::attach` now rejects autocommit by checking transaction IDs, and restricted-role verification covers direct column grants. New Postgres tests cover deferred savepoint rollback and stale outbox claim tokens. These source corrections are not a claim that integrated tests passed. Historical statuses remain the four agreed enum values; the withdrawn unknown-status finding stays withdrawn.
