# Reusable bounded contexts

The workspace will contain `modules/billing` (P2) and `modules/cms` (P6). Neither exists as an implemented module yet. P1 does not create empty crates that could make a module gate appear green.

Each context has a pure `core`, a `postgres` adapter with its own schema prefix and migrations, an `http` contract/router, and Svelte `web` views where needed. Add actual crates to the root Cargo workspace as they are implemented; do not introduce nested workspaces.

Modules must not depend, directly or transitively, on `crates/taskr-*` or `transgate/*`. Both applications depend on the module and implement its host-facing ports. Modules exchange domain events and identifiers, not access to another context's tables.

`./scripts/gate-modules.sh` checks the resolved Cargo graph, requires billing and CMS to exist and have both consumers, runs each module package's tests and checks workspace consumers. An absent module fails explicitly. This is expected during P1, not a passing implementation claim.
