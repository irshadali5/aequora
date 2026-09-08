# Part 50 v1 productization and GA-readiness completion

The governing authority is
[`sys-arch/50-aequora-v1-scope-productization-milestones-production-readiness-release-candidate-ga-long-term-evolution-architecture.md`](../sys-arch/50-aequora-v1-scope-productization-milestones-production-readiness-release-candidate-ga-long-term-evolution-architecture.md).
This report records executable repository contracts; it does not declare the current development
snapshot generally available.

## Executable productization contracts

- `aequora-release::productization` owns the frozen v1 product scope, categorical support
  maturity, target-bound claims, S0-S4 defects, owned risk records, supported-upgrade state,
  M0-M11 milestone evidence, the standard subsystem readiness review, and fail-closed GA decisions.
- `release/v1-scope.ron` fixes the critical production path to a Rust SDK, Postcard over HTTPS,
  Axum, a single logical PostgreSQL authority, and SQLite as the required official local adapter path.
  Stoolap remains conditional on platform conformance. Additional authority databases, transports,
  consensus, P2P authority, generic CRDTs, and runtime plugins remain explicit non-goals.
- `GaReadinessManifest::evaluate` requires all fourteen GA evidence categories, no open tracked
  risk, no open S0/S1 or correctness/security-critical defect, all M0-M11 evidence-backed
  milestones, at least one supported upgrade and subsystem review, and identical RC/promoted
  digests. Validation and eligibility are separate so a truthful incomplete snapshot remains
  machine-readable without becoming releasable.
- Completed milestones require code, tests, documentation, migration state, and verification
  reports. `Ready` and `ReadyWithLimitations` claims require evidence from the claimed target or
  profile; no aggregate numeric score can conceal a missing category.
- The standalone release tool and main CLI validate the scope and readiness manifests. The main
  CLI also exposes `aequora version --verbose` and `aequora compatibility check` for separate
  version-axis and compatibility diagnostics.

## Registered evidence and enforcement

`AEQ-INV-GA001` through `AEQ-INV-GA010` are registered in `aequora-invariants` and the durable
registry. `GeneralAvailabilityFull` binds conformance test IDs 184 through 193 to those invariants.
The focused testkit suite verifies the frozen storage/transport path, conditional Stoolap status,
the deliberately blocked development readiness snapshot, and registry parity.

`scripts/check-productization-architecture.sh` enforces required artifacts, invariant parity,
neutral release dependencies, CLI diagnostics, registry freshness, and focused Rust contracts. CI
runs that gate after Parts 48 and 49.

## Honest current readiness state

`release/v1-readiness.ron` is a development snapshot, not an RC or GA attestation. It records
repository evidence and explicit limitations, keeps unresolved risk areas owned, leaves M0-M11
release evidence unclaimed, supplies no fictional upgrade/production-review artifacts, and uses
different placeholder candidate/promotion digests. Its expected decision is `eligible=false`.

An actual RC/GA decision still requires exact-artifact evidence from protected release systems,
real PostgreSQL and declared Tier 1 target campaigns, production workload capacity and soak
results, upgrade/restore/incident exercises, signing custody, vulnerability response, support
ownership, pilot evidence, and approval of the release-specific known-issues set. These are
release/deployment inputs and are never inferred from repository architecture.

## Verification contract

The focused gate is:

```bash
bash scripts/check-productization-architecture.sh
```

Repository-wide completion additionally requires formatting, strict Clippy, Rust 1.87 MSRV,
warning-denied Rustdoc, locked/offline workspace tests, Guppy workspace policy,
database-neutrality profiles, registry generation/verification, and diff hygiene.

## Completed verification

Validated on 2026-09-09 with `CARGO_BUILD_JOBS=1`: the focused Part 50 architecture gate; all-
feature workspace tests and doctests; strict all-target/all-feature Clippy; Rust 1.87 all-target/
all-feature checking; warning-denied workspace Rustdoc; Guppy/workspace boundaries;
database-neutral custom, Stoolap, SQLite, PostgreSQL, and combined profiles; regenerated Rust and
Markdown registry snapshots; formatting; and diff hygiene all passed. The semantic index was
refreshed with 18 new or changed documents and a focused query returned the Part 50 specification,
implementation contracts, CLI requirement, and GA evidence rules.
