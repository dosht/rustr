# Billing erasure safety review

Read-only source review; no builds, tests, database checks, credentials or provider API calls. Public documentation only. Counterexamples below are **[INFERENCE: source/transaction interleavings, not executed]**. Three P1 gaps remain in D10; correct cancellation evidence for a known subscription does not close them.

## 1. P1 — Unfinished plan checkouts can survive a falsely completed erasure

`modules/billing/postgres/src/tx.rs:955-1007` inventories existing subscription bindings, then blocks only **completed** unbound plan payments. `modules/billing/core/src/erasure.rs:92-95,242-249` interprets empty pending/unbound lists as permission to erase. The cascade deletes payments/tokens/subscriptions and stamps completion (`tx.rs:1124-1147`).

Concrete counterexamples:

- Iyzico form/token is persisted and exposed (`core/src/app.rs:649-677`). Customer completes the form; provider creates an ACTIVE subscription. Callback/result lookup is lost or uncertain, so local payment stays pending and has no subscription binding (`app.rs:751-775`; `gateways/iyzico/src/client.rs:318-331`). Erasure sees neither a termination target nor an unbound **completed** payment and finishes without canceling that subscription.
- Same state, but the exposed form is still payable and completes **after** local erasure. No minimum duration is needed: erasure can start immediately after form exposure. Confirmation now cannot find its token. Deleting local rows did not revoke the provider form.
- A successful authenticated result arrives after the barrier but before the cascade. `observe_in` calls ordinary `lock_account` before storing the subscription/customer (`app.rs:777-831`); with a visible barrier it rejects the result. `reconcile_payment` invokes that same path (`896-906`). There is no erasure-only adoption of this late identity. A subscription webhook instead holds an unbound notification without an account/payment association (`982-993`); it neither identifies the erased owner nor creates a cancellation target. D4 forbids guessing that association.

Local failed/canceled status is also insufficient: `core/src/grants.rs:126-139` explicitly allows later authenticated completion; P2 D5 preserves uncertainty rather than inferring a final decline. `postgres/tests/account_erasure.rs:380-392` pins the unsafe exclusion (line 387), not provider non-renewal. Its completed row prevents deletion, so it never exercises pending/failed-only false completion.

**Smallest safe correction:** block erasure for every unresolved plan checkout, regardless of local status; retain its checkout-token/account/gateway/environment correlation while blocked. Add a narrow erasure-only authenticated checkout reconciliation path: under the account lock, retain the verified subscription/customer as a termination target, without granting/provisioning, then use existing D10 cancellation. Completion must check that all such correlations and every bound subscription are covered by confirmed termination evidence. Keep uncertainty visibly pending. A currently empty/error lookup or local failed status cannot prove that an exposed form will never complete. Mirror the unresolved-target invariant in database deletion enforcement: `0002_account_erasure.sql:57-60` currently checks only existing termination rows, not unbound checkouts.

**Required regression:** pending-only checkout with successful provider result/local response loss cannot erase; pending live form completing during erasure produces a durable cancellation target with no grant; failed/canceled imported plan remains unresolved until verified; one canceled known subscription plus a second unresolved checkout still cannot finish. Replace the line-387 exclusion assertion.

## 2. P1 — Checkout admission bypasses the erasure barrier

`core/src/app.rs:484-489` treats account existence as provisioned, including an account retained during erasure. `start_checkout` then initializes Iyzico and inserts the payment without calling `lock_account` (`580-585,649-667`). `postgres/src/tx.rs:400-428` is a plain INSERT; neither migration adds a payment barrier guard. The account FK only establishes existence.

**Counterexample:** barrier is already committed, a known subscription's cancellation is still pending, and the account/contact still exists (or a checkout captured the customer before erasure). A new plan checkout initializes, inserts and exposes another payable form despite the fence. Even fixing finding 1 would allow continued creation of new unresolved work and prevent erasure convergence.

Provisioning is not an admission guard: `create_account` returns false for a visible barrier (`tx.rs:223-239`), but `provision_in` returns successful `granted:false`, and `ensure_provisioned` discards that distinction (`app.rs:457-460,491-493`). A completed cascade's FK eventually prevents insertion; it does not fence the open-barrier interval or avoid the external initialization.

**Smallest safe correction:** explicitly reject barred checkout admission before provider initialization; after provider initialization, acquire the account lock and recheck the barrier before persistence/exposure in the existing init → persist → expose flow. Do not hold a transaction across provider I/O. A preflight check alone loses the in-flight initialization race. Previously exposed forms still require finding 1's unresolved-checkout gate.

**Required regression:** already-open barrier denies checkout without provider initialization; an initialization response arriving after barrier opening cannot persist/expose a new payable form. Preserve ordinary unbarred P2 checkout ordering.

## 3. P1 — A waiting callback can pass a stale barrier predicate and create an uninventoried binding

`postgres/src/tx.rs:242-260` evaluates barrier `EXISTS` in the **same statement** as `SELECT ... FOR UPDATE OF a`. `open_erasure` inserts the barrier, locks the account and snapshots bindings (`924-968`). READ COMMITTED refreshes snapshots between statements, not while that statement waits on a row lock.

Counterexample schedule:

1. Erasure transaction inserts its uncommitted barrier, holds the account lock, and copies the currently known bindings into termination rows.
2. Confirmation has an authenticated success. Its `lock_account` statement starts before erasure commits, sees no committed barrier, and waits on that account lock.
3. Erasure commits. The waiting statement retains its old snapshot of the separate barrier table and returns false. Confirmation completes the payment and inserts a new subscription binding.
4. A later erasure-status read has no termination row for that new binding. The payment is no longer "unbound" because the subscription now exists. It therefore does not block the cascade. Broadening the payment-status filter alone does not fix this schedule.

**Smallest safe correction:** first acquire the account row lock, then query the barrier in a separate READ COMMITTED statement while retaining the lock. Make checkout persistence participate in that guard. Final erasure reconciliation should also reject any bound subscription not covered by a confirmed termination target, rather than treating presence in `billing.subscriptions` alone as coverage. Late authenticated observations rejected from ordinary application still need finding 1's cancellation-only reconciliation path.

**Required regression:** two-connection schedule with confirmation demonstrably blocked behind `open_erasure`, then barrier commit. Assert no ordinary grant/binding can commit afterward and no uncanceled identity is lost. A sequential "barrier already exists" test cannot catch this.

## Confirmed-detail boundary and primary evidence

No additional defect found in the **known-target** D10 evidence check: `core/src/erasure.rs:48-63` requires the exact subscription, an existing matching customer binding, and exactly CANCELED/EXPIRED; missing customer, ACTIVE/PENDING/UNPAID and mismatches do not confirm. `iyzico/src/client.rs:397-420` accepts a documented successful cancel envelope, uses authenticated detail after explicit rejection, and leaves transport/parse uncertainty uncertain. This proves only that target, not the completeness of the account's subscription inventory.

- [Iyzico initialize/retrieve subscription checkout](https://docs.iyzico.com/en/getting-started/preliminaries/api-reference-beta/subscription/subscription/initialize-subscription.md): initialization returns form/token; result lookup after form completion returns the created subscription/customer references. ACTIVE/PENDING is provider subscription state, not local payment confirmation. Documentation has `tokenExpireTime`, but the adapter does not retain it; its existence is not proof that a subscription was never created or that a late result can safely be forgotten. No checkout revocation capability or assumed expiry interval is used in this review.
- [Iyzico subscription transactions](https://docs.iyzico.com/en/getting-started/preliminaries/api-reference-beta/subscription/subscription/subscription-transactions.md): cancellation addresses a known subscription reference; EXPIRED means no further charges, and CANCELED is canceled. This does not revoke other outstanding checkout forms.
- [PostgreSQL READ COMMITTED semantics](https://www.postgresql.org/docs/current/transaction-iso.html#XACT-READ-COMMITTED): a locking statement may wait for another transaction while retaining its snapshot of other rows; a subsequent statement sees the newly committed state. This supports finding 3; it is not execution proof.

Factual cross-check requested from persistence owner p1R regarding any additional guard/reconciliation path; no such path was found in the reviewed core, PostgreSQL migrations or billing HTTP entrypoints. Scope excludes host identity/contact erasure and ordinary manual cancellation policy.

## Repair seam re-review — p1X ownership

Verdict: the revised late-result adoption **plan is sound**; saved coverage repairs address the original races at source level. Remaining account-binding coverage and operator-closure evidence issues below prevent unconditional approval. No execution performed; planned application changes are not claimed implemented.

### Accepted source and seam

- `postgres/src/tx.rs:222-242` now locks first and reads the barrier in a separate statement; `insert_payment:411-416` uses that guard. `erasure_status:998-1032` inventories uncovered bindings and all plan-payment statuses; `record_termination:1075-1099` adopts an uncovered binding on its first cancellation attempt. Migrations 0003/0004 also require coverage before deletion.
- Updating `payment.subscription_ref` is the right checkout-to-target correlation: the coverage query uses that field. p1X's revised plan acquires account then payment locks, rereads the payment, and atomically writes customer/subscription/observation plus a pending target. It returns an internal successful outcome, commits, **then** returns `AccountErased`; passing that error into `finish` would roll back the evidence.
- p1X explicitly accepted checking erasure before the completed shortcut, including completed imported uncovered plan payments in authenticated reconciliation, refusing a changed stored subscription reference, and rejecting existing-target account/customer collisions. Exact duplicate adoption preserves status/attempts; it does not overwrite or reset another target. No normal subscription binding, grant, payment-status transition or billing event is required for cancellation-only adoption.
- Preflight plus post-network account locking is appropriate for admission. Neither preflight alone nor the presence of an account is a fence. Existing exposed checkouts remain unresolved until their target is canceled or independently justified closure is available.

### Remaining concrete issues

1. **Account-scoped coverage is missing in saved SQL.** `tx.rs:1007-1009,1028-1030,1121-1123` and migration `0004_erasure_checkout_resolutions.sql:38-45` match gateway/environment/subscription reference but omit the target account. An imported ambiguous payment referencing another account's target is classified covered; its owner has no corresponding pending target. If the foreign target is confirmed, the deletion policy also accepts it. Adoption collision checks cannot help when an already-covered shortcut skips adoption. Require `t.account_id = p.account_id` / `s.account_id` consistently in status, resolution eligibility and deletion coverage. Preserve ambiguity as unresolved, not borrowed cancellation evidence. Sent directly to p1X.

2. **An audit row is not by itself proof an exposed form cannot create a subscription.** The operator tests at `postgres/tests/account_erasure.rs:787-806` and `http/tests/api.rs:743-751` accept “form never paid”, report `subscriptions_stopped`, and (Postgres test) erase. That is a past-state observation, compatible with a still-payable form completing afterward. The saved API requires only nonblank rationale (`core/src/app.rs:1230-1267`), while the port comment promises a statement that the checkout **cannot renew**. Retaining its token in the audit does not route a late confirmation: current lookup uses live payments. Requested lead clarification whether this is an explicitly approved risk waiver or a proof-backed operator attestation. Under existing D10, mere unpaid status must not authorize completion. Do not invent an expiry interval or revocation API to close this gap.

### Regression assessment and handoff

The new tests are behavioral, not wiring-only: `account_erasure.rs:380-511` checks every status, missing target coverage, restricted-role deletion and an actual blocked lock waiter; `676-733` checks durable adoption despite the returned error, no grants/bindings/events, subsequent cancellation and cascade. The event-kind predicates agree with `BillingEvent::kind`; there is no false-green naming mismatch. The HTTP tail checks typed refusal/no provider initialization and operator authorization. None was run by this reviewer.

Add focused regressions for the revised plan's uncertain edges: completed imported uncovered checkout reconciliation; duplicate adoption preserving existing termination state/attempt count; reference/customer/cross-account collisions rolling back all payment-field changes; a foreign-account termination never covering this account; and HTTP late-result 410 with the target demonstrably persisted. Operator closure fixtures must establish the approved non-renewal evidence, not normalize “unpaid so far” into future safety.

Next owner: p1X implements the agreed seam and remaining coverage correction; lead settles operator-closure evidence and owns execution. This re-review is source/plan acceptance only, not a passing-test or live-provider claim.

## Source-ready re-review — current repair

Reviewed application snapshot `5CE6`, erasure `1B25`, migration 0004 `E76E`, HTTP tests `3C45`, and persistence/test corrections `B10E`/`447F`. No builds, tests or provider calls. **Changes needed only for the remaining bidirectional identity collision below; execution is still lead-owned.**

The earlier account-scoped coverage and closure-evidence findings are addressed in source. Coverage and final SQL policy now require the same account. `CheckoutClosure::refusal` requires both provider-evidence attestations, evidence reference and named operator; SQL CHECKs enforce those fields, and HTTP derives the operator from authenticated admin identity. Tests now reject the bare “never paid” request. This validates an operator-attestation boundary, not the truth of any live provider evidence.

`confirm_checkout`/`keep_erasure_target` preserve authenticated late references atomically and commit before returning 410; the completed-import shortcut no longer blocks reconciliation. Tests exercise unchanged payment status, absent grants/events/bindings, subsequent cancellation, repeated target identity, collision rollback and HTTP durable-target behavior. The same-account coverage fixture also exercises restricted-role deletion.

Two additional defects found during this pass were promptly corrected by p1X and source-rechecked:

- Adoption now checks existing trusted subscription bindings, not only termination rows (`tx.rs:1123-1147`). The live-other-account regression explicitly leaves B **not erasing**, verifies conflict, unchanged payment references, no cancellation target and A still unresolved (`account_erasure.rs:996-1020`).
- Account creation now freshly checks the barrier after a successful INSERT as well as a zero-row INSERT (`tx.rs:246-273`), before reporting success and permitting a signup grant. This closes the success-side stale-statement-snapshot concern; the proposed interleaving was not executed here.

### Remaining P1 — Ownership checks are one-directional across two tables

`adopt_erasure_target` now checks `billing.subscriptions`, but normal `insert_subscription` still checks only that same table (`tx.rs:616-669` in the reviewed implementation). A cancellation-only target intentionally has no normal subscription binding.

**[INFERENCE: unexecuted source schedule]** A's erasure authenticates reference R and commits a termination target without a normal binding. B's ordinary confirmation later authenticates R; its `insert_subscription` sees no normal binding and can grant/bind R to B despite A's existing target. A's cancellation can then terminate B's newly accepted subscription. The reverse ordering is now refused, but this ordering is not. Concurrent adoption/binding can likewise both pass independent checks on separate tables. This is the same ambiguous-provider-reference boundary already exercised by collision fixtures, not a proposal to trust a new provider mapping.

Smallest coherent correction: serialize normal binding and erasure adoption by the same `(gateway, environment, subscription_ref)` identity, then check the counterpart table after acquiring that lock in **both** paths. Keep account-first ordering and never lock the other account's row merely to reject its identity. A transaction-scoped identity lock can avoid a new registry/schema. Preserve legitimate same-owner/customer idempotency and reject foreign/conflicting identities before any grant commits.

Regression: an existing A cancellation-only target followed by B normal confirmation must conflict with no B grant/binding/event; exercise both concurrent orderings as well. Sent directly to p1X. No production/provider request is needed.

### Callback status disposition

**410 `account_erased` is appropriate** for the permanent ordinary-write fence; it is not a temporary conflict that retrying can remove. Authenticated late evidence commits before that response; uncertain provider lookup still returns 504, identity conflict remains 409, and signature failures remain authentication failures. No provider retry behavior is inferred from the status choice. The HTTP late-browser-return test checks 410 plus persistent target/no grant; no execution claim is made.
