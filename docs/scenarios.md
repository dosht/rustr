# Scenario catalog

PRD section 6 is the contract. `existing` records a previously built capability whose complete scenario evidence has not yet been registered here; `pending` is unfinished; `done` requires executable tests and a book chapter. The Rustr gate accepts only `done`, executes each named Rust package's tests, and checks that the exact named tests passed, rather than accepting zero selected tests.

Test references use `cargo:<package>:<fully-qualified-test-name>`. Multiple references are separated by `; `. A test reference is evidence of an exercised behavior, not a claim that a single test proves every aspect of the scenario. Review remains responsible for adequacy.

| ID | Scenario | Status | Tests | Book |
|---|---|---|---|---|
| SC-01 | Domain, use cases, ports, presentation state machine and five shells | existing | tbd | book/chapters/03-core-and-shells.md |
| SC-02 | Multi-user, RBAC, outbox and revision-checked saves | existing | tbd | book/chapters/13-signed-in.md |
| SC-03 | Long-running jobs, retries, stuck recovery, workers and idempotency | pending | tbd | tbd |
| SC-04 | Two provider adapters, fallback and recorded fixtures | pending | tbd | tbd |
| SC-05 | Billing module, paid package, freemium and gateway webhook | done | cargo:taskr-server:the_sample_consumes_ten_rejects_the_eleventh_without_a_write_then_a_signed_order_buys_more; cargo:taskr-server:parallel_identical_work_makes_one_debit_and_the_dev_subject_maps_to_one_account | book/chapters/14-porting-an-existing-product.md |
| SC-06 | API keys, scopes, rate limits and callback delivery records | pending | tbd | tbd |
| SC-07 | Scheduler and lock ports safe with two replicas | pending | tbd | tbd |
| SC-08 | Deletion saga, retention opt-out and consent | pending | tbd | tbd |
| SC-09 | Paged presentation list with cursors | pending | tbd | tbd |
| SC-10 | Streaming presentation through wasm and UniFFI | pending | tbd | tbd |
| SC-11 | Upload progress and presigned URLs outside the core | pending | tbd | tbd |
| SC-12 | CMS static site/blog, two locales, sitemap and hreflang | pending | tbd | tbd |
| SC-13 | Generated shell tokens, light/dark and E-Ink palette | pending | tbd | tbd |
| SC-14 | Email, analytics and error ports with fakes | pending | tbd | tbd |
| SC-15 | Versioned API with an old client still passing | pending | tbd | tbd |
| SC-16 | Docker, health, graceful shutdown and environment configuration | pending | tbd | tbd |
| SC-17 | Migration shape conversion and count verification | pending | tbd | tbd |

P1 implements token generation and the Transgate runtime, but those do not alone complete the corresponding taskr scenarios. Native runtime evidence and the book must accompany any later promotion. Existing PRD claims for SC-01/02 are retained above rather than silently reclassified as unbuilt.

SC-05 evidence: both named composed-server tests passed in the 454-test database-backed pre-P3 workspace run; a separate actual taskr process smoke exercised the same purchase, refusal and duplicate flow. The book chapter and EPUB/PDF were rebuilt. Details and limits: `docs/milestones/p2-billing.md`. This scenario does not certify unfinished Transgate browser/chat/download clauses or later P3 erasure extensions; the gate reruns its named package rather than trusting this recorded baseline.
