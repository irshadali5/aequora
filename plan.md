# Aequora implementation plan

## Document ownership

The `sys-arch/` documents are the authoritative system-architecture and design specifications.
They contain the detailed protocol, data, security, performance, deployment, and operational
design. This file intentionally does not copy that design.

`plan.md` is limited to implementation sequencing, status, acceptance evidence, and repository
workflow. When architecture changes, update the applicable `sys-arch/*.md` document first; add
only a short status or link here.

Completion evidence lives in the relevant `docs/*-completion.md` reports. Those reports record
what is executable in this repository and which application or deployment responsibilities remain
outside the reusable core.

## Architecture authority index

| Part | Authoritative specification |
|---:|---|
| 01 | [Formal correctness](sys-arch/01-formal-correctness.md) |
| 02 | [Causality, provenance, and lineage](sys-arch/02-causality-provenance-lineage.md) |
| 03 | [Anti-entropy and self-repair](sys-arch/03-anti-entropy-self-repair.md) |
| 04 | [Offline compaction and rebase](sys-arch/04-offline-compaction-rebase.md) |
| 05 | [Local multiprocess coordination](sys-arch/05-local-multiprocess-coordination.md) |
| 06 | [Adaptive sync scheduler and QoS](sys-arch/06-adaptive-sync-scheduler-qos.md) |
| 07 | [Subscription scope and dynamic dataset](sys-arch/07-subscription-scope-dynamic-dataset.md) |
| 08 | [Live sync, push, and presence](sys-arch/08-live-sync-push-presence.md) |
| 09 | [Bulk import, export, seed, and migration](sys-arch/09-bulk-import-export-seed-migration.md) |
| 10 | [Large snapshot streaming and bootstrap](sys-arch/10-large-snapshot-streaming-bootstrap.md) |
| 11 | [Operation semantics and consistency profiles](sys-arch/11-operation-semantics-consistency-profiles.md) |
| 12 | [Deterministic execution and replay](sys-arch/12-deterministic-execution-replay.md) |
| 13 | [Data provenance, auditability, and explainability](sys-arch/13-data-provenance-auditability-explainability.md) |
| 14 | [Data governance, retention, and erasure](sys-arch/14-data-governance-retention-erasure.md) |
| 15 | [Cryptographic integrity, key management, and E2E](sys-arch/15-cryptographic-integrity-key-management-e2e.md) |
| 16 | [Authority failover, epochs, and fork detection](sys-arch/16-authority-failover-timeline-epochs-fork-detection.md) |
| 17 | [Multi-region reads and single-writer global architecture](sys-arch/17-multi-region-read-single-writer-global.md) |
| 18 | [Backpressure, admission, fairness, and overload](sys-arch/18-backpressure-admission-fairness-overload.md) |
| 19 | [Performance engineering and memory architecture](sys-arch/19-performance-engineering-memory-architecture.md) |
| 20 | [Resource-constrained client architecture](sys-arch/20-resource-constrained-client-architecture.md) |
| 21 | [Protocol negotiation and compatibility governance](sys-arch/21-protocol-negotiation-compatibility-governance.md) |
| 22 | [Sync metadata schema and internal persistence](sys-arch/22-sync-metadata-schema-internal-persistence.md) |
| 23 | [Background jobs, durable workflows, and side effects](sys-arch/23-background-jobs-durable-workflows-side-effects.md) |
| 24 | [Operational control plane and admin API](sys-arch/24-operational-control-plane-admin-api.md) |
| 25 | [Diagnostics, forensics, and incident bundles](sys-arch/25-diagnostics-forensics-reproducible-incident-bundles.md) |
| 26 | [Legacy application compatibility](sys-arch/26-legacy-application-compatibility-incremental-adoption.md) |
| 27 | [Security threat model and abuse resistance](sys-arch/27-security-threat-model-abuse-resistance.md) |
| 28 | [Multi-consumer change feed](sys-arch/28-multi-consumer-change-feed-architecture.md) |
| 29 | [Schema, operation registry, and developer governance](sys-arch/29-schema-operation-registry-developer-governance.md) |
| 30 | [Certification and conformance ecosystem](sys-arch/30-certification-conformance-ecosystem-architecture.md) |
| 31 | [Android and iOS mobile runtime platform architecture](sys-arch/31-android-ios-mobile-runtime-platform-architecture.md) |
| 32 | [Linux, Windows, and macOS desktop runtime architecture](sys-arch/32-linux-windows-macos-desktop-runtime-architecture.md) |
| 33 | [Cross-platform local storage architecture](sys-arch/33-cross-platform-local-storage-mobile-desktop-architecture.md) |
| 34 | [Reference implementation and workspace crate boundaries](sys-arch/34-reference-implementation-workspace-crate-boundary-architecture.md) |
| 35 | [Public Rust API and SDK stability](sys-arch/35-public-rust-api-sdk-stability-architecture.md) |
| 36 | [Storage adapter SDK and official adapter architecture](sys-arch/36-storage-adapter-sdk-official-adapter-architecture.md) |
| 37 | [PostgreSQL/Neon authoritative adapter](sys-arch/37-postgresql-neon-authoritative-adapter-detailed-architecture.md) |
| 38 | [Stoolap embedded local persistence](sys-arch/38-stoolap-embedded-local-replica-client-persistence-architecture.md) |
| 39 | [Axum server integration and middleware](sys-arch/39-axum-server-integration-middleware-architecture.md) |
| 40 | [Dioxus client integration](sys-arch/40-dioxus-client-integration-reactive-state-architecture.md) |
| 41 | [CLI and developer toolchain](sys-arch/41-cli-developer-toolchain-architecture.md) |
| 42 | [SQLite embedded local adapter](sys-arch/42-sqlite-embedded-local-adapter-architecture.md) |
| 43 | [Configuration, secrets, runtime policy, and feature flags](sys-arch/43-configuration-secrets-environment-profiles-runtime-policy-feature-flags-architecture.md) |

## Implementation status

The table below records implementation independently by part; an implemented later part does not
implicitly claim completion of an earlier part.

| Scope | Status | Evidence |
|---|---|---|
| Parts 01–04: core synchronization foundations | Implemented | [Completion audit](docs/plan-completion.md) |
| Parts 05–10: scheduling, scopes, live delivery, import/export, and bootstrap | Implemented | [Completion audit](docs/plan-completion.md) |
| Parts 11–14: semantics, replay, audit, and governance | Implemented | [Completion audit](docs/plan-completion.md) |
| Part 15: cryptographic integrity and key lifecycle | Implemented | [Cryptography report](docs/cryptographic-integrity-key-management-e2e-completion.md) |
| Parts 16–17: authority epochs and regional reads | Implemented | [Multi-region report](docs/multi-region-read-single-writer-global-completion.md) |
| Part 18: backpressure and fair admission | Implemented | [Completion audit](docs/plan-completion.md) |
| Part 19: performance and memory architecture | Implemented | [Part 19 report](docs/performance-engineering-memory-architecture-completion.md) |
| Part 20: resource-constrained client architecture | Implemented | [Part 20 report](docs/resource-constrained-client-architecture-completion.md) |
| Part 21: protocol negotiation and compatibility governance | Implemented | [Part 21 report](docs/protocol-negotiation-compatibility-governance-completion.md) |
| Part 22: sync metadata schema and internal persistence | Implemented | [Part 22 report](docs/sync-metadata-schema-internal-persistence-completion.md) |
| Part 23: background jobs, durable workflows, and side effects | Implemented | [Part 23 report](docs/background-jobs-durable-workflows-side-effects-completion.md) |
| Part 24: operational control plane and admin API | Implemented | [Part 24 report](docs/operational-control-plane-admin-api-completion.md) |
| Part 25: diagnostics, forensics, and reproducible incident bundles | Implemented | [Part 25 report](docs/diagnostics-forensics-reproducible-incident-bundles-completion.md) |
| Part 26: legacy application compatibility and incremental adoption | Implemented | [Part 26 report](docs/legacy-application-compatibility-incremental-adoption-completion.md) |
| Part 29: schema, operation registry, and developer governance | Implemented | [Part 29 report](docs/schema-operation-registry-developer-governance-completion.md) |
| Part 30: certification, conformance, and ecosystem architecture | Implemented | [Part 30 report](docs/certification-conformance-ecosystem-completion.md) |
| Part 31: Android and iOS mobile runtime platform architecture | Implemented | [Part 31 report](docs/android-ios-mobile-runtime-platform-completion.md) |
| Part 32: Linux, Windows, and macOS desktop runtime architecture | Implemented | [Part 32 report](docs/linux-windows-macos-desktop-runtime-completion.md) |
| Part 33: Cross-platform local storage architecture | Implemented | [Part 33 report](docs/cross-platform-local-storage-completion.md) |
| Part 34: Reference implementation and workspace crate boundaries | Implemented | [Part 34 report](docs/reference-implementation-workspace-boundaries-completion.md) |
| Part 35: Public Rust API and SDK stability | Implemented | [Part 35 report](docs/public-rust-sdk-completion.md) |
| Part 36: Storage adapter SDK and official adapter architecture | Implemented | [Part 36 report](docs/storage-adapter-sdk-completion.md) |
| Part 37: PostgreSQL/Neon authoritative adapter | Implemented | [Part 37 report](docs/postgresql-neon-authority-completion.md) |
| Part 38: Stoolap embedded local replica and client persistence | Implemented | [Part 38 report](docs/stoolap-local-persistence-completion.md) |
| Part 39: Axum server integration, authentication, and middleware | Implemented | [Part 39 report](docs/axum-server-integration-completion.md) |
| Part 40: Dioxus client integration and reactive state | Implemented | [Part 40 report](docs/dioxus-client-integration-completion.md) |
| Part 41: CLI and developer toolchain | Implemented | [Part 41 report](docs/cli-developer-toolchain-completion.md) |
| Part 42: SQLite embedded local adapter | Implemented | [Part 42 report](docs/sqlite-local-adapter-completion.md) |
| Part 43: Configuration, secrets, runtime policy, and feature flags | Implemented | [Part 43 report](docs/configuration-secrets-runtime-policy-feature-flags-completion.md) |

## Implementation workflow

For each future part:

1. Read the complete governing specification in `sys-arch/`.
2. Add or update the implementation and its focused tests.
3. Add a concise evidence row or completion report; do not copy the specification into this plan.
4. Run the repository architecture, dependency-direction, database-neutrality, formatting, and
   relevant workspace gates.
5. Refresh the retrieval index after material refactors.

## Required repository gates

Run the bounded, offline-friendly checks with one Cargo build job when memory is constrained:

```text
cargo fmt --all -- --check
cargo run -q -p aequora-dev -- check
bash scripts/check-database-neutrality.sh
bash scripts/check-performance-architecture.sh
bash scripts/check-client-resource-architecture.sh
bash scripts/check-compatibility-architecture.sh
bash scripts/check-metadata-architecture.sh
bash scripts/check-jobs-architecture.sh
bash scripts/check-admin-architecture.sh
bash scripts/check-diagnostics-architecture.sh
bash scripts/check-legacy-architecture.sh
bash scripts/check-mobile-architecture.sh
bash scripts/check-desktop-architecture.sh
bash scripts/check-local-storage-architecture.sh
bash scripts/check-reference-implementation-architecture.sh
bash scripts/check-postgres-authority-architecture.sh
bash scripts/check-stoolap-local-persistence-architecture.sh
bash scripts/check-axum-server-architecture.sh
bash scripts/check-dioxus-client-architecture.sh
bash scripts/check-cli-toolchain-architecture.sh
bash scripts/check-sqlite-local-adapter-architecture.sh
bash scripts/check-configuration-architecture.sh
cargo check --workspace --all-targets --all-features --locked --offline
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo test --workspace --all-features --locked --offline
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --all-features --no-deps --locked --offline
```

Deployment-specific PostgreSQL, Neon, mobile, soak, capacity, and production benchmark gates are
reported as environment-dependent evidence rather than represented as universal architecture
claims in this plan.

## Change policy

Architecture prose belongs in exactly one numbered `sys-arch` specification. This plan may link to
that prose, summarize implementation status, or record a decision needed to schedule work, but it
must not reproduce the design. Completion reports may cite both the specification and executable
evidence without becoming a second design source.
