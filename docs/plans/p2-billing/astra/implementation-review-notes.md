# P2 implementation review notes

Read-only review of the approved contract and available source, not a verdict on unfinished developer work. No billing implementation files were visible in the targeted paths when reviewed. No code, builds or tests run. Iyzico order-list identity is excluded. Priorities below are false-coverage risks to close during implementation, not new framework requests.

## P1: Customer directory and actual notification delivery

Rules: BIL-066, BIL-072, BIL-075/076, CPN-007/019; EML-BIL-001/002/003.

Evidence: `crates/taskr-server/src/identity.rs` returns only subject/email; legacy `frontend/src/pages/api/hooks/payment-updated.ts:27-38,62-83` resolves the payment owner's customer record before sending buyer and sales emails. `admin/customer-invoices.ts:39-52` looks up another customer by email. The approved contract names configuration and an outbox but does not specify a working customer lookup or mail-delivery composition.

Required: Transgate must provide a real host-owned customer lookup by ID/email, with trusted name/email and eligibility data, usable after the checkout JWT is gone. A server callback cannot obtain the buyer from its caller identity or trust posted recipient fields. Mount the authorized admin lookup and return the actual customer's completed history/latest billing info. Wire committed completion to buyer and sales templates, plus cancellation to support, through a runnable configured delivery adapter. Preserve documented subjects. A declared event, template-only test or logging sink does not prove delivery behavior. Local HTTP mail fixtures can prove the actual runtime path without a live send. Do not acknowledge the completion event as delivered when one required destination failed; retries remain at-least-once. Assert the server analytics dispatch too where BIL-072 is claimed, not just an event-name constant.

## P1: Real identity adapters and stable account mapping

Rules: BIL-019, BIL-062, BIL-068, BIL-075/076; FRE-001.

Evidence: `crates/taskr-server/src/identity.rs:19-32` defines verified claims/principal; its DevIdentity returns `UserId(format!("dev-{email}"))`, not a UUID. Taskr composes this identity in `src/main.rs`; Transgate's current runtime mounts only `/health`. Contract requires UUID-shaped billing accounts and real configurable JWT verification.

Required: Mount billing with the actual configured identity in each binary, not an unrelated test-only identity. Preserve real UUID subjects; define deterministic host-owned dev-subject mapping so the documented dev smoke can use billing and retries reach the same account. Dev mapping must not replace real-token verification or become an anonymous fallback. Verify missing/invalid/expired credentials, cross-account history/cancel requests and non-admin lookup against the composed router. Admin and special-offer eligibility come from verified identity and host data, never request email/admin booleans. Health-only success and a router tested solely with an injected fake cannot cover these rules.

## P1: Import permissions must apply to the running database connection

Rules: BIL-001/002/003/019/024; DEF-002.

Evidence: contract line 67 requires a separate import role and runtime prohibition on origin writes. Existing taskr main connects with DATABASE_URL and reuses that pool for runtime repositories; the P2 local database credential is an administrative test credential. Column grants on a restricted role do not restrict a server still connected as the table owner/superuser, and inherited/table-level INSERT grants can restore access to origin.

Required: Separate migration/import authority from the effective runtime database role. Prove the actual runtime role cannot set origin (including imported zero/negative rows), update it, gain import authority, or create new cross-account package references. Test default-origin negative rejection and explicit-origin rejection separately. Use the configured restricted runtime connection for a consumer smoke; running only SET ROLE assertions while the server still uses an owner pool is false security evidence. Imported owner anomalies remain preserved and counted by referenced package, with source-versus-normalized differences reported.

## P1: Work retry must compare payload before returning a saved result

Rules: BIL-019/020/021/026 and the approved taskr sample contract, line 35.

Evidence: this sample has no Job CAS, and the contract requires stable work identity, saved result and payload conflict. A shortcut `if done { return success }` without checking the original payload makes reuse of the same ID with different units look successful without doing or charging that work.

Required: Scope work identity to its authenticated account; persist immutable requested units/operation data and the successful response. Compare payload before replaying a done row. Concurrent identical requests produce one debit and the same stored result; changed units conflict without a write; another account cannot inspect or complete that row. Insufficient funds roll back both debit and pending-to-done transition, so the rejected operation can succeed after purchasing credits. Replay returns its saved outcome, not a newly computed balance. Exercise this through the mounted taskr HTTP route and real transaction.

## P2: Freemium provisioning must retain signup time and cannot replenish

Rules: FRE-001/002/012, BIL-005/006.

Evidence: FRE-001's source is the user-row INSERT trigger; its two-month window starts on signup. The contract requires provisioning-race evidence but does not spell out the runtime signup-to-billing connection. Neither existing server source provides a billing signup consumer yet.

Required: Implement an idempotent host account-provisioning path using a trusted account creation instant. A user created January 1 who first opens billing April 1 must not receive a fresh April-to-June trial merely because billing is lazy-initialized. JWT issued-at is a login/token timestamp, not signup time. Repeated sign-in, duplicate provisioning and expired trial reads must not grant again. Prove the path used by the real host with delayed and duplicate provisioning, not only an isolated grant helper.

These five items are actionable implementation/evidence requirements. They do not reopen settled policy, demand a general identity/event framework, or claim missing unfinished files are developer regressions.
