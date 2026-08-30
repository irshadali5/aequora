# Aequora Sync

[![CI](https://github.com/irshadali5/aequora/actions/workflows/ci.yml/badge.svg)](https://github.com/irshadali5/aequora/actions/workflows/ci.yml)
[![Rust 1.87+](https://img.shields.io/badge/MSRV-1.87.0-blue.svg)](https://www.rust-lang.org)
[![Edition 2024](https://img.shields.io/badge/edition-2024-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE-MIT)
[![Workspace](https://img.shields.io/badge/workspace-66%20crates-purple.svg)](crates/)
[![Architecture Wiki](https://img.shields.io/badge/architecture-30%20part%20wiki-brightgreen.svg)](wiki/Home.md)

Aequora is a database-neutral, server-authoritative, local-first synchronization engine written in
Rust. It synchronizes typed domain operations and authoritative state transitions—not SQL,
database pages, or vendor-specific write-ahead logs.

Use it to build software that accepts writes offline, reconciles safely after reconnecting, and
keeps application authorization and business rules at the authoritative server.

> Current release line: `0.1.0` · MSRV: Rust `1.87` · Edition: `2024`

---

## About

**Aequora** is built from first principles to solve the fundamental tensions of distributed,
offline-capable systems: how to provide instant local mutations without sacrificing centralized
authorization, formal data consistency, or database portability.

### Key Architectural Tenets

- **Local-First with Authoritative Server**: Clients execute local ACID transactions immediately with
  zero network latency. When connectivity resumes, the server evaluates domain authorization,
  executes business validation, resolves conflicts via deterministic policies, and produces an
  authoritative linear event stream.
- **Database Neutrality**: Zero coupling to vendor-specific database engines or WAL formats. Local
  embedded persistence (Stoolap, SQLite, or custom) and authoritative cloud backends (PostgreSQL,
  Neon, CockroachDB, or custom) interact strictly through strongly-typed capability contracts.
- **Formal Invariants & Correctness**: Every state transition is bounded by an executable invariant
  registry, verified via model checking, and validated under high-fault deterministic simulation.
- **Industrial-Grade Resilience**: Built-in anti-entropy range Merkle tree exchange, local offline
  operation log compaction, resumable snapshot bootstrap, and tamper-evident cryptographic provenance.

### When to Use Aequora

- **Offline-First Applications**: Mobile, desktop, and embedded applications where users must create
  and modify data without an active network connection.
- **Multi-Tenant SaaS & Enterprise ERP**: Collaborative systems where business rules, compliance,
  and permissions must be enforced by a trusted central authority rather than client peer-to-peer code.
- **Edge & Distributed Deployments**: Edge gateways and branch offices that operate autonomously
  during WAN outages and reconcile deterministically upon link restoration.
- **Audit-Critical & Regulated Workflows**: Financial, medical, and legal platforms requiring
  tamper-evident audit chains, zero-knowledge payload encryption, and verifiable data governance.

---

## Topics & Key Concepts

### Repository Topics & Tags

`rust` · `distributed-systems` · `local-first` · `offline-first` · `sync-engine` ·
`server-authoritative` · `crdt` · `event-sourcing` · `anti-entropy` · `merkle-tree` ·
`database-agnostic` · `postgresql` · `neon` · `stoolap` · `quic` · `axum` · `formal-verification` ·
`audit-logging` · `zero-knowledge-encryption` · `resumable-streaming` · `multi-region`

### Core Subject Areas

- **Formal Correctness & Causality**: Executable invariant registries, state machine model checking,
  Lamport clocks, hybrid logical timestamps (HLC), dependency DAGs, and causal cut consistency.
- **Anti-Entropy & Self-Repair**: Range-based Merkle tree exchange, state fingerprinting, divergence
  detection, and automatic self-repair protocols.
- **Local Coordination & Storage**: Multi-process lock election across browser tabs/processes,
  offline operation queue compaction, timeline rebasing, and Stoolap embedded storage.
- **Transport & Networking**: AEQ1 binary framed Postcard codec over HTTPS (Axum) and multiplexed
  QUIC streams (Quinn), with adaptive rate limiting and pre-body admission control.
- **Snapshot & Bulk Interoperability**: Resumable streaming snapshots, cold-replica bootstrap,
  out-of-band artifact transfer, and canonical schema migration mappings.
- **Audit, Governance & Security**: Tamper-evident cryptographic audit logs, envelope encryption,
  asymmetric key rotation, GDPR/CCPA erasure cascades, and Byzantine abuse resistance.
- **Scale, Performance & Embedded**: SIMD acceleration, memory-mapped ring buffers, zero-copy
  protocol framing, multi-region single-writer topologies, and resource-constrained client profiles.
- **Protocol Governance & Workflows**: Runtime protocol negotiation, schema registries, transactional
  outbox patterns, and durable distributed sagas.

---

## Why Aequora?

Offline mutation is easy. Correct recovery is not.

A synchronization system must survive the client crashing after a local write, the server
committing before its response is lost, two devices updating the same entity version, interrupted
snapshot installation, schema upgrades, journal compaction, and transient network failure.

Aequora makes those failure boundaries explicit:

```text
1. client transaction
   optimistic local state + durable outbox operation

2. at-least-once delivery
   stable OperationId across retries

3. authoritative transaction
   entity + version + journal + operation result + audit

4. client reconciliation transaction
   changes + applied markers + terminal outbox state + conflicts + cursor
```

Every root action also receives a retry-stable `CorrelationId`. The authority allocates a distinct
`EventId`, records the direct `LineageRef`, and returns the same identity and lineage on retry.
Derived events and durable jobs inherit correlation only after client identity claims have been
matched to the authenticated server context.

The result is local ACID plus durable eventual convergence. Aequora does not pretend an offline
client and a remote authority share a distributed transaction.

## Architecture

Local persistence, authoritative persistence, and transport are independent composition axes:

```text
┌──────────────────────────┐
│ Application domain       │
│ typed commands + policy  │
└────────────┬─────────────┘
             │
     ┌───────▼────────┐      ┌──────────────────┐      ┌────────────────────┐
     │ LocalStore     │      │ SyncTransport    │      │ AuthoritativeStore │
     │                │─────▶│                  │─────▶│                    │
     │ Stoolap/custom │◀─────│ HTTP/QUIC/custom │◀─────│ PostgreSQL/custom  │
     └────────────────┘      └──────────────────┘      └────────────────────┘
```

The built-in production acceptance topology is:

```text
Stoolap client
    │
    │ AEQ1 framed Postcard over HTTPS
    ▼
Axum gateway
    │
    ▼
Neon pooled PostgreSQL authority
    └── direct Neon endpoint for migrations
```

That topology is an integration profile, not a protocol dependency. A custom adapter can replace
either database without rewriting the client/server engine or wire protocol.

## Core guarantees

- Atomic local application mutation and outbox insertion in compliant local adapters.
- Stable `OperationId` idempotency across response loss and retry.
- Atomic authoritative entity, version, sequence, journal, result-ledger, and audit commit.
- Exact optimistic version transitions and deterministic conflict policy selection.
- Atomic local reconciliation with cursor advancement last.
- Durable retry attempt/deadline state and replayable `Sending` recovery.
- Consistent, resumable snapshot bootstrap with atomic final installation.
- Monotonic scoped cursors, retained-floor resynchronization, and safe compaction watermarks.
- Bounded operation count, frame bytes, decompressed bytes, snapshots, dependencies, and scopes.
- Authenticated pre-body global/per-tenant admission, rate limiting, and execution deadlines.
- Database capability declarations and reusable local/authority compliance contracts.
- Payload-free metrics and tracing across client, transport, server, and transaction boundaries.

## Deliberate boundaries

- A request batch is not one atomic business transaction; each operation is independently atomic.
- Dependency ordering does not imply group rollback.
- Cross-client/server two-phase commit is neither implemented nor claimed.
- HTTP success is not the source of truth; the durable operation ledger is.
- Authentication credentials, TLS, database secrets, backup policy, and restoration remain owned by
  the host application.
- Finance balancing and external-effect outboxes must be enforced in the application's own domain
  transaction and tests.
- SQLite, Redb, document stores, and other databases require a real custom adapter and compliance
  proof; they are not built-in merely because the protocol is database-neutral.

## Quick start

### Requirements

```bash
rustup toolchain install 1.87.0 --profile minimal
rustup override set 1.87.0
```

Use the GitHub source directly:

```toml
[dependencies]
aequora = { git = "https://github.com/irshadali5/aequora", features = ["stoolap", "http-client"] }
```

When consuming a published `0.1.x` release, replace `git` with `version = "0.1"`.

### Run the verified examples

The minimum deterministic flow uses an in-memory client, authority, and transport:

```bash
cargo run -p aequora --example in_process --features testkit --locked
```

The ERP example performs an optimistic offline write and outbox append in one real Stoolap
transaction, then executes and reconciles it through a typed server handler:

```bash
cargo run -p aequora --example school_erp --features stoolap,testkit --locked
```

Expected output:

```text
offline attendance accepted and reconciled at sequence 1
```

Read the [complete tutorial](TUTORIAL.md) to build the same vertical slice step by step.

### Minimal in-process assembly

```rust,no_run
use aequora::{
    client::{ClientConfig, ClientSyncEngine},
    clock::TestClock,
    conflict::RejectConflicts,
    executor::AuthContext,
    protocol::SessionMetadata,
    server::{ExchangeService, SyncServer},
    testkit::{
        AllowAllExecutor, InMemoryAuthoritativeStore, InMemoryLocalStore,
        InProcessTransport,
    },
    types::{ActorId, DeviceId, NodeId, SessionId, SyncScopeId, TenantId},
};
use std::sync::Arc;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let tenant = TenantId::new();
let actor = ActorId::new();
let device = DeviceId::new();
let scope = SyncScopeId::new();
let session = SessionMetadata {
    session_id: SessionId::new(),
    device_id: device,
    actor_id: actor,
    tenant_id: tenant,
    scope_id: scope,
    partitions: Vec::new(),
};
let auth = AuthContext {
    actor_id: actor,
    tenant_id: tenant,
    device_id: device,
};

let authority = InMemoryAuthoritativeStore::default();
let service: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
    Arc::new(authority),
    Arc::new(AllowAllExecutor),
    Arc::new(RejectConflicts),
    Arc::new(TestClock::new(NodeId::new(), 1_000)),
));
let engine = ClientSyncEngine::new(
    InMemoryLocalStore::default(),
    InProcessTransport::new(service, auth),
    ClientConfig::new(session),
);

let outcome = engine.run_once().await?;
assert_eq!(outcome.acknowledged, 0);
# Ok(())
# }
```

This demonstrates assembly. The runnable examples also create and synchronize a real operation.

## Feature profiles

Select each deployment axis independently:

| Feature | Integration | Primary types |
|---|---|---|
| default `postcard` | AEQ1 framed binary protocol | codec and wire DTOs |
| `stoolap` | Embedded local/client persistence | `StoolapDatabase`, `StoolapStore` |
| `postgres` | PostgreSQL or Neon authority | `SqlxPostgresBackend`, `PostgresStore` |
| `axum` | HTTP server gateway | `router_with_lifecycle`, `ServerLifecycle` |
| `http-client` | Bounded Reqwest transport | `HttpTransport`, `RequestHeaders` |
| `quic` | Quinn request/snapshot/hint transport | `QuicTransport`, `QuicServer` |
| `testkit` | Deterministic reference components | in-memory stores and adapter contracts |
| `ron` / `json` | Diagnostic codecs | optional human-readable encodings |
| `tracing` | Structured payload-free tracing | `TracingObserver` |
| `record-sync` | Optional canonical record mapping/migration | `SchemaRegistry`, `SchemaMap`, `CanonicalExport` |

Recommended dependencies:

```toml
# Native client
aequora = { version = "0.1", features = ["stoolap", "http-client", "tracing"] }

# Authority server
aequora = { version = "0.1", features = ["postgres", "axum", "tracing"] }

# End-to-end integration tests
aequora = { version = "0.1", features = ["stoolap", "postgres", "axum", "http-client", "testkit"] }
```

The database-neutrality gate compiles custom/custom, Stoolap/custom, custom/PostgreSQL, and
Stoolap/PostgreSQL profiles to prevent cross-adapter leakage.

## Production components

### Stoolap client

`StoolapDatabase` owns checksummed local migrations, the durable outbox/retry schedule,
reconciliation, conflict inbox, cursors, and staged snapshot installation. Application repositories
use its native transaction to commit optimistic state and the outbox operation together.

```rust,no_run
use aequora::stoolap::{StoolapDatabase, StoolapStore};

# fn open() -> Result<(), Box<dyn std::error::Error>> {
let backend = StoolapDatabase::open("file:///var/lib/my-app/client")?;
backend.health_check()?;
let local_store = StoolapStore::new(backend);
# Ok(())
# }
```

### PostgreSQL and Neon authority

```rust,no_run
use aequora::postgres::{PostgresPoolConfig, PostgresStore, SqlxPostgresBackend};

# async fn connect() -> Result<(), Box<dyn std::error::Error>> {
let url = std::env::var("DATABASE_URL")?;
let backend = SqlxPostgresBackend::connect_with_config(
    &url,
    PostgresPoolConfig::new(10),
)
.await?;
backend.health_check().await?;
let authority = PostgresStore::new(backend);
# Ok(())
# }
```

For Neon, use a pooled runtime endpoint and a direct migration endpoint:

```rust,no_run
# use aequora::postgres::SqlxPostgresBackend;
# async fn connect() -> Result<(), Box<dyn std::error::Error>> {
let backend = SqlxPostgresBackend::connect_neon(
    &std::env::var("NEON_POOLED_DATABASE_URL")?,
    &std::env::var("NEON_DIRECT_DATABASE_URL")?,
    10,
)
.await?;
# Ok(())
# }
```

The Neon constructor enforces certificate/hostname verification and scale-to-zero-friendly pooling.

### Axum/HTTP boundary

The Axum integration exposes:

```text
POST /sync/v1/exchange       bounded incremental push/pull
POST /sync/v1/bootstrap      resumable snapshot bootstrap
GET  /sync/v1/health         compatibility liveness alias
GET  /sync/v1/health/live    process liveness
GET  /sync/v1/health/ready   bounded dependency readiness
```

The host application must authenticate JWT/session/mTLS credentials and insert a verified
`AuthContext` before these routes. Aequora then enforces tenant admission, rate limits, body and
decompression bounds, request deadlines, readiness, and graceful draining.

## Strict runtime configuration

`AequoraConfig` parses secret-free RON with `deny_unknown_fields`, non-zero bounds, and cross-field
validation. The same configuration maps into client, server, HTTP, QUIC, compute, and coordinator
settings.

```ron
(
    protocol: (minimum_version: 1, version: 1),
    push: (
        max_operations: 128,
        max_bytes: 1048576,
        max_wait_ms: 150,
    ),
    pull: (max_events: 1024, max_bytes: 4194304),
    retry: (
        max_attempts: 5,
        initial_ms: 500,
        max_ms: 30000,
        multiplier: 2,
        jitter_percent: 20,
        max_exchanges_per_sync: 1024,
    ),
    coordinator: (
        channel_capacity: 32,
        periodic_interval_ms: Some(30000),
        sync_on_start: true,
    ),
    operational: (
        max_in_flight_requests: 256,
        max_in_flight_per_tenant: 64,
        tenant_requests_per_second: 64,
        tenant_request_burst: 128,
        max_rate_limit_tenants: 4096,
        rate_limit_idle_timeout_ms: 300000,
        body_read_timeout_ms: 15000,
        request_timeout_ms: 30000,
        readiness_timeout_ms: 2000,
        drain_timeout_ms: 30000,
        retry_after_seconds: 1,
    ),
)
```

Database URLs, access tokens, and TLS keys do not belong in this object.

## Workspace map

The workspace contains 66 crates managed via `aequora-dev`. The functional areas are:

| Functional Area | Crates |
|---|---|
| **Facade** | `aequora` |
| **Core Values, Clocks & Protocol** | `aequora-types`, `aequora-clock`, `aequora-protocol`, `aequora-codec`, `aequora-compat`, `aequora-scope`, `aequora-live`, `aequora-bootstrap`, `aequora-metadata` |
| **Client & Server Kernel** | `aequora-client`, `aequora-server`, `aequora-authority`, `aequora-executor`, `aequora-validator`, `aequora-admission` |
| **Storage Contracts & Adapters** | `aequora-store`, `aequora-store-stoolap`, `aequora-store-postgres` |
| **Network Boundaries & Transport** | `aequora-transport`, `aequora-http`, `aequora-axum`, `aequora-quic`, `aequora-feed` |
| **Domain Policies & Consensus** | `aequora-crypto`, `aequora-security`, `aequora-profile`, `aequora-replay`, `aequora-audit`, `aequora-governance`, `aequora-conflict`, `aequora-crdt`, `aequora-partition`, `aequora-journal`, `aequora-queue` |
| **Workflows, Jobs & Side Effects** | `aequora-jobs`, `aequora-workflow`, `aequora-side-effects` |
| **Record Interoperability & Schema** | `aequora-schema`, `aequora-mapping`, `aequora-migration`, `aequora-registry-types`, `aequora-registry-codegen`, `aequora-registry-generated`, `aequora-registry-cli` |
| **Topology, Regions & Routing** | `aequora-region`, `aequora-routing`, `aequora-blob`, `aequora-admin` |
| **Performance, Diagnostics & Control** | `aequora-compute`, `aequora-performance`, `aequora-config`, `aequora-observability`, `aequora-coordination`, `aequora-integrity`, `aequora-scheduler`, `aequora-diagnostics` |
| **Legacy Interoperability** | `aequora-legacy`, `aequora-legacy-api` |
| **Verification, Conformance & Tooling** | `aequora-invariants`, `aequora-model`, `aequora-testkit`, `aequora-conformance`, `aequora-macros`, `aequora-cli`, `aequora-dev` |

Run `cargo run -q -p aequora-dev -- summary` for the live workspace graph or
`cargo run -q -p aequora-dev -- graph aequora-client` for one crate's dependency direction.
Use `cargo run -q -p aequora-dev -- profile list`, `profile explain <kind>`, `profile verify
<manifest.ron>`, or `profile compare <old.ron> <new.ron>` for profile governance.
Use `cargo run -q -p aequora-dev -- replay explain`, `replay verify <bundle.ron>`, or `replay
inspect <bundle.ron>` for payload-free deterministic replay diagnostics.
Use `cargo run -q -p aequora-dev -- audit explain`, `audit verify <chain.ron>`, or `audit
inspect <chain.ron>` for payload-free audit architecture and chain-integrity diagnostics.
Use `cargo run -q -p aequora-dev -- governance explain` or `governance verify
<erasure-plan.ron>` for read-only lifecycle diagnostics.
Use `cargo run -q -p aequora-dev -- crypto policy` or `crypto registry-verify
<root-and-registry.ron>` for secret-free cryptographic policy and trust diagnostics.
Use `cargo run -q -p aequora-dev -- performance explain`, `performance profile <name>`,
`performance workload-verify <workload.ron>`, or `performance compare <baseline.ron>
<candidate.ron>` for Part 19 memory budgets and reproducible regression diagnostics.
Part 20 resource policy is available under `aequora::client_resources`; applications select a
`ClientResourceProfile`, feed coarse platform signals through `PlatformResourceMonitor`, and retain
durable work while admission reduces or defers bounded units.
Part 21 compatibility policy is available under `aequora::compatibility`; the runtime-neutral
negotiator selects a server-governed session profile, rejects security/semantic downgrades, and
keeps protocol, authority epoch, operation schema, snapshot schema, and local-store versions
independent. Use `aequora compat show`, `compat matrix`, `compat deprecated`, `compat check-client`,
or `compat registry` for read-only release diagnostics.

Payload-free built-in adapter diagnostics are available without database credentials:

```bash
cargo run -q -p aequora-cli -- doctor adapters
cargo run -q -p aequora-cli -- inspect adapters
cargo run -q -p aequora-cli -- inspect adapter stoolap
cargo run -q -p aequora-cli -- verify pair stoolap postgresql
cargo run -q -p aequora-cli -- verify export ./export.postcard ./schema.ron
cargo run -q -p aequora-cli -- verify model
cargo run -q -p aequora-cli -- verify trace ./failure.ron
cargo run -q -p aequora-cli -- compat registry
cargo run -q -p aequora-cli -- init ./my-aequora-client client
```

## Verification

The normal release gates are:

```bash
cargo fmt --all -- --check
cargo +1.87.0 check --workspace --all-targets --all-features --locked
cargo run -q -p aequora-dev --locked -- check
bash scripts/check-database-neutrality.sh
bash scripts/check-performance-architecture.sh
bash scripts/check-client-resource-architecture.sh
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
cargo check --manifest-path fuzz/Cargo.toml --bins --locked
cargo bench -p aequora-testkit --bench core_pipeline --no-run --locked
cargo package --workspace --no-verify --locked
git diff --check
```

The live PostgreSQL/Neon suites run only when their URLs are configured:

```bash
AEQUORA_TEST_POSTGRES_URL='postgres://...' \
    cargo test -p aequora-store-postgres --test postgres_live --locked

AEQUORA_TEST_POSTGRES_URL='postgres://...' \
    cargo test -p aequora --test database_neutrality_live --all-features --locked
```

Neon requires both `AEQUORA_TEST_NEON_POOLED_URL` and `AEQUORA_TEST_NEON_DIRECT_URL`. A test that
skips because these variables are absent is not a current live-database proof.

CI also builds all fuzz targets, the Criterion harness, both runnable examples, and every
publishable package.

## Local retrieval-first developer workflow

The repository includes RTK, semantic RAG, Octocode configuration, and Guppy dependency checks to
keep automated development context bounded:

```bash
# Index after material refactors
scripts/rag index

# Retrieve architectural or semantic context
scripts/rag query "authoritative transaction idempotency"

# Find exact identifiers or policy strings
rtk rg "TransactionCapabilityProvider" crates

# Inspect dependency direction
cargo run -q -p aequora-dev -- graph aequora-store-postgres

# Enforce architecture
cargo run -q -p aequora-dev -- check
```

See [Local AI context](docs/local-ai-context.md), [AGENTS.md](AGENTS.md), and [RTK.md](RTK.md).
The local RAG index and Octocode cache are ignored; only their portable scripts/configuration are
versioned.

## Documentation & Architecture Specifications

- [Architecture Wiki (30-Part System Specification)](wiki/Home.md)
- [Complete developer tutorial](TUTORIAL.md)
- [Governing implementation plan](plan.md)
- [Architecture specification index](next.md) ([authoritative `sys-arch/` specifications](sys-arch/))
- [ACID architecture](ACID.md)
- [ACID compliance evidence](docs/acid-compliance.md)
- [Enterprise implementation evidence](docs/enterprise-completion.md)
- [Database interoperability implementation evidence](docs/database-interoperability-completion.md)
- [Plug-and-play implementation evidence](docs/plug-and-play-completion.md)
- [System Architecture Specifications (Parts 01–36)](sys-arch/):
  - [Part 01: Formal Correctness, Invariants & Simulation](wiki/01-formal-correctness.md)
  - [Part 02: Causality, Dependency & Event Lineage](wiki/02-causality-provenance-lineage.md)
  - [Part 03: Anti-Entropy, Divergence Detection & Self-Repair](wiki/03-anti-entropy-self-repair.md)
  - [Part 04: Offline Operation Compaction & Rebase](wiki/04-offline-compaction-rebase.md)
  - [Part 05: Local Multi-Process & Coordinator Election](wiki/05-local-multiprocess-coordination.md)
  - [Part 06: Adaptive Sync Scheduler & QoS](wiki/06-adaptive-sync-scheduler-qos.md)
  - [Part 07: Subscription, Scope & Dynamic Datasets](wiki/07-subscription-scope-dynamic-dataset.md)
  - [Part 08: Live Sync, Push Hints & Presence](wiki/08-live-sync-push-presence.md)
  - [Part 09: Bulk Import, Export & Seed Migration](wiki/09-bulk-import-export-seed-migration.md)
  - [Part 10: Large Snapshot & Streaming Bootstrap](wiki/10-large-snapshot-streaming-bootstrap.md)
  - [Part 11: Operation Semantics & Consistency Profiles](wiki/11-operation-semantics-consistency-profiles.md)
  - [Part 12: Deterministic Domain Execution & Replay](wiki/12-deterministic-execution-replay.md)
  - [Part 13: Data Provenance, Auditability & Explainability](wiki/13-data-provenance-auditability-explainability.md)
  - [Part 14: Data Governance, Retention, Legal Hold & Erasure](wiki/14-data-governance-retention-erasure.md)
  - [Part 15: Cryptographic Integrity, Key Management & E2E](wiki/15-cryptographic-integrity-key-management-e2e.md)
  - [Part 16: Authority Failover, Timeline Epochs & Fork Detection](wiki/16-authority-failover-timeline-epochs-fork-detection.md)
  - [Part 17: Multi-Region Read & Single-Writer Global Deployment](wiki/17-multi-region-read-single-writer-global.md)
  - [Part 18: Backpressure, Admission Control & Fairness](wiki/18-backpressure-admission-fairness-overload.md)
  - [Part 19: Performance Engineering & Memory Architecture](wiki/19-performance-engineering-memory-architecture.md)
  - [Part 20: Resource-Constrained Client Architecture](wiki/20-resource-constrained-client-architecture.md)
  - [Part 21: Protocol Negotiation & Compatibility Governance](wiki/21-protocol-negotiation-compatibility-governance.md)
  - [Part 22: Sync Metadata Schema & Internal Persistence](wiki/22-sync-metadata-schema-internal-persistence.md)
  - [Part 23: Background Jobs, Durable Workflows & Side Effects](wiki/23-background-jobs-durable-workflows-side-effects.md)
  - [Part 24: Operational Control Plane & Admin API](wiki/24-operational-control-plane-admin-api.md)
  - [Part 25: Diagnostics, Forensics & Incident Bundles](wiki/25-diagnostics-forensics-reproducible-incident-bundles.md)
  - [Part 26: Legacy Application Compatibility & Migration](wiki/26-legacy-application-compatibility-incremental-adoption.md)
  - [Part 27: Security Threat Model & Abuse Resistance](wiki/27-security-threat-model-abuse-resistance.md)
  - [Part 28: Multi-Consumer Change Feed Architecture](wiki/28-multi-consumer-change-feed-architecture.md)
  - [Part 29: Schema & Operation Registry Governance](wiki/29-schema-operation-registry-developer-governance.md)
  - [Part 30: Certification, Conformance & Ecosystem Architecture](wiki/30-certification-conformance-ecosystem-architecture.md)
  - [Part 31: Android & iOS Mobile Runtime Platform Architecture](sys-arch/31-android-ios-mobile-runtime-platform-architecture.md)
  - [Part 32: Linux, Windows & macOS Desktop Runtime Architecture](sys-arch/32-linux-windows-macos-desktop-runtime-architecture.md)
  - [Part 33: Cross-Platform Local Storage for Mobile & Desktop](sys-arch/33-cross-platform-local-storage-mobile-desktop-architecture.md)
  - [Part 34: Reference Implementation & Workspace Crate Boundary Architecture](sys-arch/34-reference-implementation-workspace-crate-boundary-architecture.md)
  - [Part 35: Public Rust API & SDK Stability Architecture](sys-arch/35-public-rust-api-sdk-stability-architecture.md)
  - [Part 36: Storage Adapter SDK & Official Adapter Architecture](sys-arch/36-storage-adapter-sdk-official-adapter-architecture.md)
- [Custom database adapter guide](docs/custom-database-adapters.md)
- [Local retrieval and tooling guide](docs/local-ai-context.md)

## Project status

The repository-owned implementation described by the `sys-arch/` specifications, `plan.md`,
`ACID.md`, and `enterprise.md` is present in
code, migrations, public contracts, real Stoolap tests, deterministic simulations, model/property
tests, HTTP/QUIC integration tests, and environment-gated PostgreSQL/Neon suites.

Production acceptance remains deployment-specific. Before calling a deployment complete, run the
live database suites with real credentials and prove TLS, backup restoration, capacity, monitoring,
and graceful rollout/drain against the actual infrastructure.

## License

Licensed under the [MIT License](LICENSE-MIT).

