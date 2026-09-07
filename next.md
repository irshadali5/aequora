# Aequora architecture specification index

## Document ownership

The numbered documents in `sys-arch/` are the authoritative Aequora system-architecture
specifications. They own the detailed protocol, storage, security, reliability, performance,
deployment, and conformance design.

This file is an index and compatibility pointer only. The former 184-section “Next Architecture”
document duplicated that design and has been removed. `next.md` must not become a second design
source.

Implementation sequencing belongs in [`plan.md`](plan.md). Executable implementation evidence and
explicit v1 decisions belong in [`docs/next-completion.md`](docs/next-completion.md) and the
part-specific completion reports.

## Authoritative architecture

| Part | Specification |
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
| 31 | [Android and iOS mobile runtime](sys-arch/31-android-ios-mobile-runtime-platform-architecture.md) |
| 32 | [Desktop runtime platform](sys-arch/32-linux-windows-macos-desktop-runtime-architecture.md) |
| 33 | [Cross-platform local storage](sys-arch/33-cross-platform-local-storage-mobile-desktop-architecture.md) |
| 34 | [Reference implementation workspace boundaries](sys-arch/34-reference-implementation-workspace-crate-boundary-architecture.md) |
| 35 | [Public Rust SDK stability](sys-arch/35-public-rust-api-sdk-stability-architecture.md) |
| 36 | [Storage adapter SDK](sys-arch/36-storage-adapter-sdk-official-adapter-architecture.md) |
| 37 | [PostgreSQL and Neon authority adapter](sys-arch/37-postgresql-neon-authoritative-adapter-detailed-architecture.md) |
| 38 | [Stoolap local persistence](sys-arch/38-stoolap-embedded-local-replica-client-persistence-architecture.md) |
| 39 | [Axum server integration](sys-arch/39-axum-server-integration-middleware-architecture.md) |
| 40 | [Dioxus client integration](sys-arch/40-dioxus-client-integration-reactive-state-architecture.md) |
| 41 | [CLI and developer toolchain](sys-arch/41-cli-developer-toolchain-architecture.md) |
| 42 | [SQLite embedded local adapter](sys-arch/42-sqlite-embedded-local-adapter-architecture.md) |
| 43 | [Configuration, secrets, runtime policy, and feature flags](sys-arch/43-configuration-secrets-environment-profiles-runtime-policy-feature-flags-architecture.md) |
| 44 | [Packaging, distribution, and release engineering](sys-arch/44-packaging-distribution-release-engineering-artifact-signing-update-channels-cross-platform-delivery-architecture.md) |
| 45 | [Deployment topologies and operational environments](sys-arch/45-deployment-topologies-single-node-ha-multi-region-edge-enterprise-air-gapped-operational-environment-architecture.md) |
| 46 | [Observability, metrics, distributed tracing, and SLOs](sys-arch/46-observability-metrics-tracing-logging-slos-alerting-production-telemetry-architecture.md) |
| 47 | [Benchmarking, performance regression, and capacity planning](sys-arch/47-benchmarking-performance-regression-capacity-planning-workload-modeling-scalability-testing-production-sizing-architecture.md) |
| 48 | [Testkit, verification, fault injection, and quality gates](sys-arch/48-testkit-verification-fault-injection-property-model-integration-e2e-release-quality-gates-architecture.md) |
| 49 | [Licensing, dependency policy, and supply-chain security](sys-arch/49-licensing-dependency-policy-supply-chain-security-crate-governance-sbom-reproducible-builds-third-party-risk-architecture.md) |
| 50 | [Aequora v1 scope, productization milestones, and GA](sys-arch/50-aequora-v1-scope-productization-milestones-production-readiness-release-candidate-ga-long-term-evolution-architecture.md) |

## Repository status

The reusable implementation is complete through Part 47. See the [implementation plan](plan.md)
for sequencing and the [completion audit](docs/next-completion.md) for the historical v1 kernel
evidence and compatibility decisions. Deployment-specific certification evidence remains exact to
each built binary and environment and is not inferred from repository ownership.

## Change policy

Architecture prose belongs in exactly one numbered `sys-arch` document. Update that specification
first, then record only implementation status, acceptance evidence, or a link in `plan.md` and
this index. Completion reports may explain implementation decisions, but they must not introduce a
competing architecture.
