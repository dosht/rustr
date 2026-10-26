# P7 independent design brief

P5 checkpoints passed on candidate7 and P5 closure is in progress. PRD section 8 lets P2-P8 overlap and P9 waits for P5 and P7, so P7 design starts now. This is independent design only, not an implementation authorization. The lead keeps decisions and canonical gates.

Codex is over its weekly limit, so the usual Opus/Sol pair is replaced by two Claude advisors: the saved Opus advisor session (p10) and the saved Fable session (p27), both at medium effort. Each writes one proposal, then cross-critiques only on explicit dispatch. No nested agents, book work, builds, tests, browser runs, provider calls or source-repository writes.

## Required outcome

Read `docs/prd.md` (P7 row and sections 2-8), `transgate/docs/discovery/{api,platform,privacy-identity,billing,jobs-transcription,ai}.md` as needed, then the exact read-only source paths behind your claims (`/home/mu/Business/Transgate`, read-only). Produce only your assigned `docs/plans/p7-api/{opus,fable}/proposal.md`. Do not read the sibling proposal before cross-critique.

- PRD P7 scope: API keys, scopes, rate limits, callbacks with delivery records, OpenAPI docs. Done condition: parity rows for API green.
- Inventory every API parity row and rule family that is P7-owned (public transcription routes, key management, callbacks, rate limits, docs), separating rows already proven in P3/P4/P5 from those P7 must make green. Cite row IDs.
- Respect accepted decisions: P5-D5 reserves Bearer on public transcription routes for API keys and moved browser job start to session-only `POST /api/jobs/start`; no credential-shape multiplexing, no fallback from an invalid key to a session. P4 D6 voice-demo exclusion (AI-027, API-F44) stays as is. Outbox from day one for callbacks (AGENTS direction); RBAC as a pure `can(actor, action, resource)` in core.
- Key storage (hashing, prefix display, rotation, revocation), scope model, rate-limit model and where its state lives (Postgres vs process), callback signing, retry schedule, delivery records and replay, idempotency for public starts, and OpenAPI generation from the `transgate-api` contract without a second source of truth.
- Billing interaction: how API usage is metered and charged through `modules/billing` without a second pricing authority.
- P6 overlap: public API docs routes and their static/SEO treatment.
- P9 overlap: what migration must carry (existing keys, callbacks, delivery history) and cutover constraints.

## Proposal format

Give source-backed row families, the smallest concrete architecture (crates, ports, tables, routes), exact shared seams with existing P3/P4/P5 code, bounded ownership and order for existing OMP owners (p1T host/API/HTTP, p1X core, p22 client, p1W documents, R billing consultation), decision recommendations with observable changes separated from source parity, and an executable acceptance plan run by the independent test agent (Y): real-Postgres tests, actual-process scenarios with loopback callback receivers, OpenAPI validation, and a client compiled against the generated contract.

No implementation or interface stubs during this design slice. Contact owners only for factual seam questions.
