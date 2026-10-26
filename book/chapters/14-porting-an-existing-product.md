# Porting an existing product

The chapters so far grow a product from nothing. This one starts from the other end: a product that already runs, with paying users, years of fixes and rules nobody wrote down. Transgate (transcription, translation and AI over recordings) is the sample. The goal is a rewrite in the pattern with zero change to the business, so the first phase produces no code at all. It produces five documents and a set of fixtures, and every later phase is measured against them. The contract is `docs/prd.md`, sections 7 and 8.

## Why discovery before code

A rule that lives in a trigger, a cron, a React hook and a marketing page is four rules until someone proves they agree. Transgate's billing deduction was rewritten twelve times as a trigger because the rule had no other home and no test above the database. Porting it means naming it once, pointing at where it lives today, and writing the Rust test that will own it. You cannot port what you cannot name, and you cannot claim parity for a flow you never listed.

So discovery answers four questions, each with its own document:

| Question | Document | Row |
|---|---|---|
| What exists? | `transgate/docs/inventory.md` | a screen, route, email, schedule, table or flag, with its source file |
| What must stay exactly the same? | `transgate/docs/rules.md` | one business rule, its source, the SQL test that proves it today, the Rust test that will |
| What do we carry, port, simplify or drop? | `transgate/docs/keep-throw.md` | one item with a verdict and a reason |
| How will we know the new one matches? | `transgate/docs/parity.md` | one user flow per surface, with the test that will prove it |

Open questions that need facts (which languages each provider covers, which models run which prompts, whether any third party still calls the old API) are research questions in the PRD, answered in `transgate/docs/research.md`. Lists that must not shrink (UI locales, transcription languages, translation targets) become JSON fixtures under `transgate/fixtures/`, and a Rust test later asserts the new lists are supersets.

## Where the truth is

Sources disagree, so record both provenance and confidence:

1. Read-only production observations establish the particular values inspected at that instant. A REST probe does not establish grants, triggers or cron configuration.
2. Database migrations, functions, constraints and tests describe the repository's behavior. They are not proof the same schema was deployed, and a test's existence does not prove it passes.
3. Server and client code establish reachable paths. Follow an option to its consumer: a computed language or timeout that is ignored downstream is not implemented behavior.
4. Docs, ADRs, epics and support tickets explain intent. Marketing claims must be checked against the reachable product.

Transgate demonstrates why the distinction matters. Production contains four language enum values absent from migrations. Its analysis route computes an output language, but the generator ignores the option. Its public media tools are server-first despite browser-WASM code being present. Each would produce a wrong port if a convenient declaration were treated as the whole truth.

Every row carries a `path:line` or a migration name so a reviewer can open it. A row without a source is not a row.

## The rules ledger

The ledger copies the current behavior, including the odd parts. It does not improve anything. Three columns do the work: the rule in plain English with exact numbers, the source, and the test that proves it. In P0 the Rust test column says `tbd` and status says `ledger`. The gate for the port (`scripts/gate-transgate.sh`) fails while any row has no passing Rust test.

Ids have a prefix per area (`BIL` billing, `FRE` freemium, `JOB` lifecycle, `PRV` privacy, `IDN` identity, `API`, `LNG` languages, `EML` emails, `CRN` schedules, and so on) and gapless numbers, so an area can be owned by one agent and merged without collisions.

What looks like a defect goes in a separate Defects table: current behavior, intended behavior, a proposal. The current behavior is what gets ported unless the owner picks the proposal. The one everyone agrees on in Transgate is the unlocked balance check that lets two parallel jobs both pass; the ledger still records today's behavior and the proposal next to it.

Existing SQL tests are rules in disguise. Every pgTAP assertion maps to at least one ledger row (`transgate/docs/pgtap-map.md` is generated to prove it), and each one becomes a Rust test in the phase that ports its area.

## Keep, port, simplify, throw, gap

Before anything is ported, each piece gets one of five verdicts with a reason:

- `keep-knowledge`: a lesson, an ADR, a runbook. Carried as text, not code.
- `port`: the behavior is rewritten in Rust or Svelte, unchanged.
- `simplify`: ported with a named reduction (one scheduler instead of three, one routing rule instead of a plugin framework with health scores).
- `throw`: dropped, and the reason says why nobody will miss it (tables nothing reads, a provider that caused more trouble than it solved, a legacy state layer).
- `gap`: the new system will not have it at first, and the owner sees it named rather than discovering it in production (the languages only one dropped provider served).

The list is what makes "fewer moving parts" a decision rather than an accident.

## The parity suite

One row per flow the current app has, per surface it runs on: the web desktop and phone layouts are two rows, the CLI, TUI, Android and iOS shells are added where the flow applies, and API endpoints, emails, schedules and locales get rows of their own. Each row names the Playwright spec or Rust test that will prove it. In P0 every row says `inventoried`; the gate fails on any row without a green test, and a row can only leave the suite with a `throw` verdict in the keep/throw list.

## Working in parallel without a merge fight

Discovery is split by topic (screens, static pages, API, billing, jobs, languages, AI, privacy and identity, data model, platform), with one writer per file under `transgate/docs/discovery/`. A worker may own several related files; the shared brief fixes row formats and prefixes. Workers save small completed sections so a usage limit does not erase a whole survey. The lead reconciles cross-topic findings and owns the generated documents.

The same split works for the port itself: each topic file ends with the open questions its design phase must answer, so the agent who ports billing starts from the billing discovery, not from the whole product.

The assembly script checks all ten topics and eight nonempty sections, rejects unfinished `pending` sections, and checks row formats and exact SQL assertion mappings. Missing SQL sources or unmapped assertions fail. Generation refuses invalid inputs rather than replacing good reports with incomplete ones. A nearby line number is not coverage; a whole-file citation is not proof of an individual assertion.

P0 captures 526 ledger obligations, 388 parity rows and all 143 pgTAP assertions. Those are discovery artifacts, not passing implementation tests. Schema/setup obligations are classified separately under DAT: the new system needs equivalent ownership and migration evidence, not the old public table names or pgTAP functions installed in production.

The findings also constrain the architecture before code starts. The static site has 4527 sitemap URLs with deliberate pruning, not every possible locale/matrix combination. Its free-media tools need a runtime processor with upload, processing, progress, download and temporary-file cleanup. Keep static content in CMS while preserving runtime behavior behind ports. A simpler deployment must not silently remove a working product surface.

::: tip
Read the inventory of a product you think you know. Transgate's owner expected a few dozen SEO pages and a handful of schedules; the counts in `transgate/docs/inventory.md` are the ones to plan with.
:::

## Build a foundation that cannot masquerade as a finished port

The first Transgate crate is an HTTP shell in the existing Cargo workspace. Its `/health` response explicitly says `process-liveness`: it proves the process can answer HTTP, not that billing, transcription, migration or provider access works. Unknown product paths return 404. Configuration comes from `TRANSGATE_BIND`; invalid configuration and an occupied address fail startup. The container runs without root privileges on a read-only filesystem and drains on SIGTERM, with a ten-second Compose stop grace.

That small runtime is useful because it is real and testable. The smoke command starts the actual executable, exercises HTTP and failure paths, sends SIGINT/SIGTERM, and separately builds and stops the real container. It is not a fake API standing in for unimplemented use cases.

The same distinction applies to gates. `gate-rustr.sh`, `gate-modules.sh` and `gate-transgate.sh` run every stage and exit nonzero if any required implementation or evidence is absent. A Rust test reference must actually appear as passed in the test output; a successful command that selected zero tests is not evidence. Module dependency checks traverse the resolved graph, including renamed and transitive dependencies, and require both applications as consumers. Empty module crates would conceal missing work, so P1 records their boundary without creating them.

## One palette source, several renderers

`design-tokens.json` holds three E-Ink candidates, each in light and dark. `scripts/gen-tokens.py` generates CSS, TypeScript palette metadata, Kotlin Material colors, Swift dynamic colors and Rust TUI constants. Its read-only check detects stale outputs and rejects low-contrast token pairs. Business states remain semantic; no palette knowledge enters the Rust domain.

Storybook renders the candidates through existing taskr presentation views and core-generated fixtures. This tests colors on real controls without pretending a Transgate screen has already been ported. The first palette, Paper Moss, is the provisional default; the choice is mu's. The web respects system preference and explicit light/dark attributes, mobile uses system appearance, and the TUI keeps its previous dark-mode default.

Cross-platform generation is not cross-platform runtime proof. In P1 the web was inspected in a browser, Android consumers compiled, and the TUI was exercised in a real terminal. Swift colors were generated and wired, but UIKit/SwiftUI execution still needs an Apple environment. Keep that verification limit visible rather than counting a generated file as a native app.

## Port billing as a module, not a collection of handlers

The billing implementation separates five packages under `modules/billing`: pure rules and use cases, PostgreSQL, HTTP, Iyzico and LemonSqueezy. Transgate supplies its catalog, customer directory and delivery policy. taskr supplies a much smaller catalog: ten free credits, a paid credit pack and explicitly metered sample work. Ordinary task creation is not charged. Both applications consume the module; none of its packages may depend back on either application, even transitively.

The transaction boundary is more important than the directory names. A host begins a transaction, attaches the billing adapter, locks the account and reads fresh facts. Its aggregate transition, deductions or grants, and committed events succeed together. The host owns commit. The adapter rejects an autocommit attachment rather than giving the caller an apparently transactional object that has already committed its first statement.

Deleting an imported subject adds another constraint: old usage may refer to another account's package, or contain signed adjustments. Discover all affected owners before locking, lock them in a stable order and recheck the set. Removing only today's well-formed usage would retain a different balance from the captured cascade. A module boundary must preserve this operation, not force the host to reach around it with arbitrary SQL.

### Similar names do not imply one rule

Transgate has three meanings of freemium:

| Decision | Captured predicate |
|---|---|
| Database start/bypass | No open package whose name differs from `freemium` |
| Start answers and transcript limits | Exactly one package row ever, named `freemium`; dates ignored |
| Chat limits and header badge | No non-free package ending at or after now; its start date is ignored |

An expired paid package distinguishes the last two. Combining them behind a convenient `is_free` flag changes the product. The post-start answer therefore derives its balance and its particular freemium facet from the real post-debit rows. A test that supplies `(74, true)` to a forwarding helper and receives `(74, true)` proves neither decision.

### Correct a defect without inventing parity

The captured allocator can see -16 minutes in an early package and 100 in a later one, admit a ten-minute request against balance 84, then deduct 26 and leave 58. The corrected allocator checks signed admission balance but allocates exactly ten over positive capacities. It leaves 74 now and 90 after the negative package expires. That last value is also an outcome change; it cannot be hidden behind “the return value is now correct.”

Exact decimal pricing has the same obligation. Removing binary rounding changed some new quotes by a cent, with corresponding exchange-rate differences. The decision preserves the pricing stages and historical amounts while recording the measured differences. The old source wording stays in the ledger; a `changed` row links the accepted correction rather than rewriting history as though the new behavior always existed.

### A provider answer is an observation, not a command

An unfinished authentication status or failed lookup cannot prove a payment failed. The adapter reports uncertainty; later authenticated confirmation or manual reconciliation may settle it. It does not quietly retry a financial POST or grant because a plausible customer record exists.

The Iyzico initial-order problem demonstrates why “idempotent” needs an identity. Checkout retrieval identifies the subscription, but the available evidence does not prove which order is its initial payment. The implementation grants the confirmed initial checkout once, holds verified subscription orders until an admin identifies that initial order, and replays through the ordinary transition. The initial alias grants nothing again; a distinct renewal grants once. Array order, first arrival and a conversation-id echo are not substitutes for proof.

External delivery happens after commit. Mail recipients and analytics targets have separate durable delivery marks and stable keys. A process restart retries the failed sales message without resending an already acknowledged buyer message. This is not a promise that every external service provides unlimited exactly-once delivery. The real process smoke exercises the restart and recorded marks instead of treating an in-memory outbox assertion as delivery evidence.

### Keep verification at the right boundary

The runtime database login is restricted and verified. Tests do not substitute the schema owner for it. A global outbox relay also needs genuinely isolated test databases: filtering the production claim API by test account would hide the very global behavior being tested. Provider references must be unique across repeated runs, not merely across two cases in one process.

Passing module policies does not finish their consumers. Freemium cap parsing cannot prove that an AI request skips both the check and increment for paid users. A constant allowing downloads cannot prove a real download path. Browser redirects, periodic refresh, client analytics and invoice PDFs likewise need their owning phase. The ledger keeps those clauses unverified while phases overlap; the final gate is not weakened to manufacture a checkpoint.

Commands, exercised results and remaining limits live in `docs/plans/p2-billing/integration-notes.md`. The phase gate is `scripts/gate-p2-billing.sh`; its missing-database and incomplete-rule failures are intentional.

## Make privacy decisions explicit before wiring deletion

Deletion is not synonymous with removing a user row. The agreed P3 contract stops future subscription charges before reporting erasure complete, preserves the captured financial cascade, and retains a minimal pseudonymous barrier against stale credentials and late callbacks. Retained billing-profile fields with a null owner link are not anonymous data.

A pending or failed checkout is not evidence that it can never create a subscription. Erasure keeps its correlation until authenticated reconciliation supplies a termination target or an operator explicitly attests verified non-payability and no remaining recurring obligation, with an evidence reference. An unpaid snapshot is insufficient. Late confirmation may add cleanup identity but cannot grant or revive an account. The barrier is read after obtaining the account lock in a fresh statement; a predicate captured before waiting can miss a concurrently committed barrier.

HTTP 410 identifies a permanently fenced application surface, not proof that every external resource has been erased. The saga reports unresolved or retained resources separately. Cleanup defaults must also agree across reads and sweeps: a missing settings row cannot mean “enabled” in the UI but “opted out” in the deletion query.

Provider policy also constrains the architecture. The selected contract sends Deepgram's model-improvement opt-out and permits global Whisper to retain the offered languages; it does not claim EU-only processing or live acceptance of every language. These decisions are recorded in `docs/plans/p3-transcription/decisions.md`. They are implementation requirements, not evidence that a deployed provider or storage policy has been verified.
