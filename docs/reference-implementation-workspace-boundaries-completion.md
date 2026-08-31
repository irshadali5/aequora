# Part 34 reference implementation and workspace boundary completion evidence

The authoritative design is [Part 34](../sys-arch/34-reference-implementation-workspace-crate-boundary-architecture.md). This report records executable repository evidence and does not redefine that architecture.

## Implemented boundary

- `architecture/workspace-boundaries.ron` assigns every workspace crate to one of the nine ordered implementation layers. It declares the narrowly justified tooling edges, dependency budgets for critical crates, and exclusive ownership of database, transport-framework, and platform dependencies.
- `aequora-dev architecture check` parses that policy and uses Guppy metadata to fail on unclassified crates, outward production edges, dependency-budget growth, duplicate assignments, missing policy crates, and isolated dependencies imported by the wrong owner.
- `aequora-dev architecture map` emits a deterministic, reviewable map of the declared layers and their direct production edges. The existing `aequora-dev check` command now runs both the established transitive boundary rules and the Part 34 policy.
- `aequora-invariants` registers `AEQ-INV-IMPL001` through `AEQ-INV-IMPL010` with stable identifiers and evidence hooks.
- The focused structural gate checks the neutral public-contract manifests and source boundary, client/UI and server/HTTP separation, the Guppy policy, invariant registry, and canonical/generated registry parity. CI executes this gate before workspace linting and tests.

## Deliberate boundaries

The workspace remains a modular monolith. Physical PostgreSQL and Stoolap implementations stay in adapter crates; Axum, HTTP, QUIC, and OS-facing dependencies remain in integration crates; application and tooling crates are the only outward composition layers. Test-only dependencies do not alter production layer direction. The umbrella `aequora` crate may expose opt-in composition features, but lower-level correctness and contract crates do not use features to select physical adapters.

The two declared outward exceptions are explicit and reviewable: the umbrella facade optionally exposes the testkit, and the CLI invokes the independent deterministic model for verification. Neither edge participates in reusable synchronization semantics.

## Verification

The focused gate is `bash scripts/check-reference-implementation-architecture.sh`. Repository-wide formatting, Clippy, tests, Rustdoc, Guppy direction, database neutrality, registry generation, and diff checks are recorded in the final task handoff after execution.
