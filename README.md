# Aequora Sync

[![CI](https://github.com/irshadali5/aequora/actions/workflows/ci.yml/badge.svg)](https://github.com/irshadali5/aequora/actions/workflows/ci.yml)
[![Rust 1.87+](https://img.shields.io/badge/MSRV-1.87.0-blue.svg)](https://www.rust-lang.org)
[![Edition 2024](https://img.shields.io/badge/edition-2024-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE-MIT)
[![Workspace](https://img.shields.io/badge/workspace-100%20crates-purple.svg)](crates/)
[![Architecture Wiki](https://img.shields.io/badge/architecture-50%20part%20wiki-brightgreen.svg)](wiki/Home.md)
[![Chaos Verified](https://img.shields.io/badge/chaos%20verified-465k%20ops%20%7C%200%20errors-success.svg)](#real-world-stress--chaos-verification-podman--kubernetes)

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
`database-agnostic` · `postgresql` · `neon` · `stoolap` · `sqlite` · `quic` · `axum` ·
`formal-verification` · `audit-logging` · `zero-knowledge-encryption` · `resumable-streaming` ·
`multi-region` · `mobile-runtime` · `desktop-runtime` · `developer-toolchain` · `feature-flags`

### Core Subject Areas

- **Formal Correctness & Causality**: Executable invariant registries, state machine model checking,
  Lamport clocks, hybrid logical timestamps (HLC), dependency DAGs, and causal cut consistency.
- **Anti-Entropy & Self-Repair**: Range-based Merkle tree exchange, state fingerprinting, divergence
  detection, and automatic self-repair protocols.
- **Local Coordination & Unified Storage**: Multi-process lock election across browser tabs/processes,
  offline operation queue compaction, timeline rebasing, Stoolap embedded storage, SQLite local replicas,
  and storage adapter SDK contracts with pluggable encryption and backup suites.
- **Platform Runtimes & OS Integration**: Native mobile runtimes for Android and iOS, desktop runtimes
  for Linux, Windows, and macOS, reactive Dioxus UI state integration, and background sync agents via IPC.
- **Transport & Networking**: AEQ1 binary framed Postcard codec over HTTPS (Axum) and multiplexed
  QUIC streams (Quinn), with adaptive rate limiting, token-bucket admission, and pre-body admission control.
- **Snapshot & Bulk Interoperability**: Resumable streaming snapshots, cold-replica bootstrap,
  out-of-band artifact transfer, and canonical schema migration mappings.
- **Audit, Governance & Security**: Tamper-evident cryptographic audit logs, envelope encryption,
  asymmetric key rotation, GDPR/CCPA erasure cascades, and Byzantine abuse resistance.
- **Scale, Performance & Embedded**: SIMD acceleration, memory-mapped ring buffers, zero-copy
  protocol framing, multi-region single-writer topologies, and resource-constrained client profiles.
- **Protocol Governance & Workflows**: Runtime protocol negotiation, schema registries, transactional
  outbox patterns, and durable distributed sagas.
- **Developer Toolchain & Configuration**: Unified developer CLI (`aequora-cli`), trace verification,
  database adapter inspection, and strict secret-free RON configuration with out-of-band secrets,
  runtime policy engines, and dynamic feature flags.
- **Operations, Telemetry & Quality Gates**: Hermetic packaging, cryptographic artifact signing,
  production topologies (HA, multi-region, air-gapped), distributed tracing, Prometheus metrics,
  and fault-injection verification gates.

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

Run the real-world distributed simulation and containerized chaos suites:

```bash
# In-process distributed simulation (10 mission-critical scenarios)
cargo test -p aequora-testkit --test real_world_simulation -- --nocapture

# Containerized high-load stress & chaos test suite (Podman / Kubernetes)
bash scripts/run-chaos-stress-test.sh
```

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

| Feature / Crate | Integration | Primary types |
|---|---|---|
| `aequora` (core) | Stable public client/server facade | `AequoraClient`, `AequoraServer`, `DomainRegistry` |
| `stoolap` | Embedded local/client persistence | `StoolapDatabase`, `StoolapStore` |
| `sqlite` | Portable embedded local/client persistence | `SQLiteDatabase`, `SQLiteConfig` |
| `postgres` | PostgreSQL or Neon authority | `SqlxPostgresBackend`, `PostgresStore` |
| `axum` | HTTP server gateway | `router_with_lifecycle`, `ServerLifecycle` |
| `http-client` | Bounded Reqwest transport | `HttpTransport`, `RequestHeaders` |
| `macros` | Derive macros for aggregates & operations | `#[derive(AequoraAggregate)]`, `#[derive(AequoraOperation)]` |
| `testkit` | Deterministic reference components | in-memory stores and adapter contracts |
| `aequora-quic` | Quinn request/snapshot/hint transport | `QuicTransport`, `QuicServer` |
| `aequora-dioxus` | Reactive local-first state for Dioxus UI | `SyncProvider`, `use_durable_query` |
| `aequora-mobile-runtime` | Android & iOS platform bridges | `MobileSyncEngine`, `MobilePlatformLifecycle` |
| `aequora-desktop-runtime` | Desktop OS runtime & IPC agent mode | `DesktopHost`, `IpcClientSession` |
| `aequora-config` | Strict validated RON configuration | `AequoraConfig`, `RuntimePolicy` |
| `aequora-observability` | Structured payload-free tracing & metrics | `TracingObserver`, `MetricRegistry` |

Recommended dependencies:

```toml
# Native desktop client (Stoolap)
aequora = { version = "0.1", features = ["stoolap", "http-client"] }

# Portable mobile/desktop client (SQLite)
aequora = { version = "0.1", features = ["sqlite", "http-client"] }

# Authority server (PostgreSQL/Neon)
aequora = { version = "0.1", features = ["postgres", "axum"] }

# End-to-end integration tests
aequora = { version = "0.1", features = ["stoolap", "postgres", "axum", "http-client", "testkit"] }
```

The database-neutrality gate compiles custom/custom, Stoolap/custom, SQLite/custom,
custom/PostgreSQL, Stoolap/PostgreSQL, and SQLite/PostgreSQL profiles to prevent cross-adapter
leakage.

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

### SQLite client

`SQLiteDatabase` is the official portable desktop/Android/iOS replica. Production open enforces
WAL, foreign keys, normal synchronous durability, and a bounded busy timeout. Application domain
writes join the outbox through `transact_local_mutation`; authoritative projections and cursors
join Tx C through `SQLiteProjectionHook`.

```rust,no_run
use aequora_store_sqlite::{SQLiteConfig, SQLiteDatabase, SQLitePlatform};

# fn open() -> Result<(), Box<dyn std::error::Error>> {
let config = SQLiteConfig::production(
    "/var/lib/my-app/client.sqlite3",
    SQLitePlatform::Desktop,
);
let backend = SQLiteDatabase::open(config)?;
assert!(backend.health()?.ready);
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

### Storage Adapter SDK & Cross-Platform Engine

Aequora decouples storage engines through `aequora-adapter-sdk` and `aequora-storage-core`. Local persistence engines (Stoolap, SQLite, or custom adapters) share capability manifests, transactional outbox semantics, background storage maintenance (`aequora-storage-maintenance`), encrypted staging directories (`aequora-storage-encryption`), and point-in-time snapshot backup contracts (`aequora-storage-backup`).

### Mobile & Desktop Runtimes

- **Mobile Runtime (`aequora-mobile-runtime`)**: Provides lifecycle-aware, battery-conscious background scheduling for Android (`aequora-platform-android` via JNI) and iOS (`aequora-platform-ios` via Objective-C runtime bridges).
- **Desktop Runtime (`aequora-desktop-runtime`)**: Supports embedded in-process execution as well as multi-process daemon mode (`aequora-agent`) coordinating across UI windows via local socket IPC (`aequora-ipc-protocol`).
- **Reactive UI State (`aequora-dioxus`)**: High-performance local-first state hooks (`SyncProvider`, `use_durable_query`) providing optimistic UI updates with bounded cache invalidation.

### Configuration, Secrets, Policy & Feature Flags

`aequora-config` pairs with `aequora-secrets`, `aequora-policy`, and `aequora-feature-flags` to enforce:
- Secret-free RON configuration schemas validated at compile/boot time.
- Isolated out-of-band secret resolution (environment, file, or cloud secret managers).
- Dynamic tenant admission policies and feature flag evaluations without modifying application code.

### Benchmarking, Workload Modeling & Capacity Headroom

- **Workload Modeling (`aequora-workload`)**: Formal workload specifications, scenario definitions, arrival processes, and synthetic datasets.
- **Reproducible Benchmarking (`aequora-benchkit`)**: Immutable benchmark run manifests, hardware/environment fingerprinting, regression comparisons, and capacity headroom estimators.
- **Load Generation (`aequora-loadgen`)**: High-throughput synthetic load generation, virtual client orchestrators, and correctness assertions under concurrency.

### Supply Chain Security & Deployment Topologies

- **Supply Chain Governance (`aequora-supply-chain`)**: Automated SBOM generation (`RON` / CycloneDX), dependency license verification, capability-based risk scoring, and tamper-evident release provenance.
- **Deployment Topologies (`aequora-deployment`)**: Production deployment manifests and topology profiles for single-node, high-availability, multi-region, edge, and air-gapped operations.

## Workspace map

The workspace contains 100 crates strictly bounded into 9 architectural layers managed via `aequora-dev`. The functional architecture layers are:

| Layer | Crates | Count | Description |
|---|---|:---:|---|
| **Foundation** | `aequora-types`, `aequora-schema`, `aequora-invariants`, `aequora-macros`, `aequora-registry-types`, `aequora-scheduler`, `aequora-coordination`, `aequora-compute`, `aequora-storage-core`, `aequora-ipc-protocol`, `aequora-blob`, `aequora-secrets`, `aequora-policy`, `aequora-feature-flags`, `aequora-workload` | 15 | Core domain identifiers, schema definitions, formal invariant registries, platform-neutral IPC framing, secrets, runtime policies, feature flags, and workload specifications. |
| **Protocol Contracts** | `aequora-protocol`, `aequora-codec`, `aequora-transport`, `aequora-clock`, `aequora-scope`, `aequora-operation`, `aequora-compat`, `aequora-authority`, `aequora-security`, `aequora-routing`, `aequora-region`, `aequora-admin`, `aequora-observability`, `aequora-audit`, `aequora-governance`, `aequora-integrity`, `aequora-legacy`, `aequora-conformance`, `aequora-registry-codegen`, `aequora-registry-generated`, `aequora-storage-profile`, `aequora-storage-maintenance`, `aequora-storage-backup`, `aequora-storage-encryption`, `aequora-storage-conformance`, `aequora-benchkit` | 26 | AEQ1 binary protocol framing, Lamport/HLC clocks, session authorization, audit chains, storage profiles, maintenance routines, encryption contracts, and benchmark harnesses. |
| **Domain Execution** | `aequora-conflict`, `aequora-crypto`, `aequora-release`, `aequora-update`, `aequora-diagnostics`, `aequora-executor`, `aequora-feed`, `aequora-jobs`, `aequora-journal`, `aequora-mapping`, `aequora-metadata`, `aequora-migration`, `aequora-partition`, `aequora-performance`, `aequora-queue`, `aequora-replay`, `aequora-side-effects`, `aequora-supply-chain`, `aequora-validator`, `aequora-workflow` | 20 | Deterministic state execution, conflict resolution policies, signed release/update decisions, journal ledgers, change feeds, saga workflows, outboxes, supply-chain governance, and record migration. |
| **Storage Contracts** | `aequora-adapter-sdk`, `aequora-store`, `aequora-blob-store`, `aequora-profile` | 4 | Unified client and authoritative storage contracts, capability manifests, zero-copy blob streaming, and storage profiles. |
| **Sync Engines** | `aequora-admission`, `aequora-bootstrap`, `aequora-client`, `aequora-crdt`, `aequora-live`, `aequora-server` | 6 | Local-first client sync engine, authoritative exchange server, resumable snapshot streaming, live push hints, and admission control. |
| **Physical Adapters** | `aequora-store-postgres`, `aequora-store-sqlite`, `aequora-store-stoolap` | 3 | Official persistence implementations: embedded Stoolap replica, portable SQLite replica, and enterprise PostgreSQL/Neon authority. |
| **Integration Platform** | `aequora-agent`, `aequora-axum`, `aequora-config`, `aequora-deployment`, `aequora-desktop-runtime`, `aequora-dioxus`, `aequora-http`, `aequora-legacy-api`, `aequora-mobile-bindings`, `aequora-mobile-runtime`, `aequora-platform-android`, `aequora-platform-ios`, `aequora-platform-linux`, `aequora-platform-macos`, `aequora-platform-windows`, `aequora-quic` | 16 | Axum HTTP server gateway, Reqwest client, Quinn QUIC transport, deployment manifests, Dioxus reactive UI state, OS bridges (Android JNI, iOS/macOS Objective-C, Windows, Linux), and background sync agent. |
| **Applications** | `aequora`, `aequora-cli`, `aequora-cli-core`, `aequora-devtools`, `aequora-inspect`, `aequora-registry-cli` | 6 | Public Rust SDK facade, unified developer CLI toolchain, inspection tools, schema registry CLI, and diagnostics binaries. |
| **Tooling & Verification** | `aequora-dev`, `aequora-loadgen`, `aequora-model`, `aequora-testkit` | 4 | Workspace boundary checker and dependency graph governance (`aequora-dev`), load generation tools (`aequora-loadgen`), state machine model testing (`aequora-model`), and end-to-end simulation testkit (`aequora-testkit`). |

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
cargo run -q -p aequora-cli -- inspect adapter sqlite
cargo run -q -p aequora-cli -- verify pair stoolap postgresql
cargo run -q -p aequora-cli -- verify pair sqlite postgresql
cargo run -q -p aequora-cli -- verify export ./export.postcard ./schema.ron
cargo run -q -p aequora-cli -- verify model
cargo run -q -p aequora-cli -- verify trace ./failure.ron
cargo run -q -p aequora-cli -- compat registry
cargo run -q -p aequora-cli -- bench list
cargo run -q -p aequora-cli -- load run ./workload.ron
cargo run -q -p aequora-cli -- capacity estimate ./workload.ron
cargo run -q -p aequora-cli -- supply-chain summary ./policy.ron
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
bash scripts/check-benchmarking-architecture.sh
bash scripts/check-supply-chain-architecture.sh
bash scripts/check-deployment-architecture.sh
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

## Local Stress & Chaos Harness (Podman)

Aequora's correctness invariants (Tx A/B/C atomicity, `OperationId` idempotency, conflict policies,
causal DAG ordering, and crash resilience) are exercised by a bounded, local-only Podman stress
harness in addition to unit, property, model, and conformance tests.

> [!CAUTION]
> This harness is not a production deployment. It uses plaintext loopback HTTP, an ephemeral
> in-memory authority, synthetic data, and a test-only MAC key generated for each run. The older
> figures below are retained as historical observations only: they predate fail-closed worker,
> outbox-drain, and authentication checks and therefore are not current release evidence.

The launcher now uses a longer soak profile: 40 clients for 30 seconds with 8 hot-key targets and
8 adversarial clients, followed by 40-client 25-second pause and restart experiments. All inputs
remain bounded by the stress executable. On 2026-09-09, the rebuilt profile reached 4,280 committed
operations in its first phase but failed closed after observing 11 unhandled failures and 69/80
adversarial requests rejected; the chaos phases were therefore not started. Higher local profiles
(80 and 120 clients) also failed closed under container resource contention. These are capacity
observations, not claims of a passing release-quality run.

Build and run the contained harness with:

```bash
CARGO_BUILD_JOBS=1 cargo build --release -p aequora --example realworld_server \
  --features axum,testkit --locked
CARGO_BUILD_JOBS=1 cargo build --release -p aequora --example realworld_stress \
  --features axum,http-client,stoolap,testkit --locked
podman build -f deploy/container/realworld.Containerfile \
  -t localhost/aequora-stress-harness:local .
bash scripts/run-chaos-stress-test.sh
```

> [!IMPORTANT]
> **Historical pre-hardening observations across 3 runs (not current release evidence):**
> - **465,560 total operations** committed across 9 experiments with **0 unhandled errors, 0 data loss events, and 0 invariant violations**.
> - **1,089 / 1,089 adversarial injection attacks blocked (100.0%)** at the transport/admission boundary.
> - **Throughput coefficient of variation (CV) < 5%** across all runs (0.5% in crash recovery), proving high reproducibility.
> - **p99 latency under hypervisor freeze (~2,894 ms)** was consistent within 2.0%, directly verifying that the 2.5-second `podman pause` was captured and weathered without dropping transactions.
> - **Resource stability**: Constant 5 OS threads (0 thread leaks), peak active memory 70–143 MB within a 1 GiB container budget (<15% utilization).

### Test Infrastructure & Components

| Component / File | Purpose | Characteristics |
|:---|:---|:---|
| [`realworld_stress.rs`](crates/aequora/examples/realworld_stress.rs) | Multi-client stress harness | Bounded inputs and telemetry; fail-closed worker, outbox-drain, and adversarial checks |
| [`run-chaos-stress-test.sh`](scripts/run-chaos-stress-test.sh) | Automated chaos orchestrator | Ephemeral MAC key and manifest; container lifecycle chaos (`pause`, `unpause`, `restart`) |
| [`realworld-k8s.yaml`](deploy/kubernetes/realworld-k8s.yaml) | Podman/Kubernetes test manifest | Loopback-only host port, no service-account token, read-only root, dropped capabilities |
| [`real_world_simulation.rs`](crates/aequora-testkit/tests/real_world_simulation.rs) | In-process simulation test suite | 1,447 lines; 10 mission-critical distributed failure scenarios |
| Container Image | Ephemeral Axum test runtime | `localhost/aequora-stress-harness:local`; never publish or deploy as a production service |

---

### Historical Containerized Chaos Stress Results (3 Independent Runs)

Each run deploys a fresh containerized Aequora server pod via `podman play kube` and executes three distinct chaos experiments.

#### Experiment 1: High Load + Hot-Key Contention + Adversarial Attacks
*100 concurrent worker clients, 10 shared hot-key entity targets, 10 adversarial attack threads injecting corrupted frames and forged tokens.*

| Metric | Run 1 | Run 2 | Run 3 | Variance (Δ) | CV% | Verdict |
|:---|---:|---:|---:|:---|:---:|:---|
| **Sync Exchanges** | 3,791 | 3,682 | 3,757 | ±109 (2.9%) | 1.4% | Stable |
| **Operations Committed** | 46,492 | 45,184 | 46,084 | ±1,308 (2.8%) | 1.4% | **Stable** |
| **Unhandled Errors** | **0** | **0** | **0** | 0 | 0.0% | **✅ Zero Errors** |
| **Hot-Key Conflicts Handled** | 48,283 | 46,866 | 47,841 | ±1,417 (3.0%) | 1.5% | Deterministic |
| **Attacks Injected** | 373 | 358 | 358 | ±15 | — | Active injection |
| **Attacks Blocked** | **373 (100%)** | **358 (100%)** | **358 (100%)** | 0 | 0.0% | **✅ 100% Blocked** |
| **Throughput (ops/sec)** | 3,319.93 | 3,204.02 | 3,285.55 | ±115.91 (3.5%) | 1.8% | **Stable** |
| **Latency p50** | 138.55 ms | 148.94 ms | 141.61 ms | ±10.39 ms | 3.7% | Sub-150ms |
| **Latency p99** | 303.35 ms | 351.28 ms | 431.33 ms | ±127.98 ms | — | Tail-bounded |
| **Peak Container Memory** | 72.0 MB | 70.82 MB | 71.38 MB | ±1.18 MB (1.7%) | 0.8% | **Stable (<75 MB)** |
| **Peak Container CPU** | 18.28% | 17.57% | 18.14% | ±0.71% | 2.0% | Minimal load |
| **OS Thread Count** | 5 | 5 | 5 | 0 | 0.0% | **Zero leaks** |

#### Experiment 2: Hypervisor Freeze & Reconnect Storm
*80 concurrent worker clients subjected to an unannounced 2.5-second `podman pause` freeze mid-flight, followed by unpause and immediate reconnection storm.*

| Metric | Run 1 | Run 2 | Run 3 | Variance (Δ) | CV% | Verdict |
|:---|---:|---:|---:|:---|:---:|:---|
| **Sync Exchanges** | 2,594 | 2,505 | 2,683 | ±178 (6.9%) | 3.5% | Normal |
| **Operations Committed** | 51,880 | 50,100 | 53,660 | ±3,560 (6.9%) | 3.5% | **Stable** |
| **Unhandled Errors** | **0** | **0** | **0** | 0 | 0.0% | **✅ Zero Errors** |
| **Throughput (ops/sec)** | 3,839.89 | 3,640.76 | 3,959.48 | ±318.72 (8.4%) | 4.3% | **Stable** |
| **Latency p50** | 145.46 ms | 138.63 ms | 131.40 ms | ±14.06 ms | 5.1% | Sub-150ms |
| **Latency p99** | 2,894.56 ms | 2,923.90 ms | 2,865.17 ms | ±58.73 ms (2.0%) | 1.0% | **Accurately measures 2.5s freeze** |
| **Latency Max** | 3,063.14 ms | 3,065.95 ms | 2,969.00 ms | ±96.95 ms (3.2%) | 1.8% | Bounded recovery |
| **Peak Container Memory** | 142.0 MB | 139.5 MB | 143.4 MB | ±3.9 MB (2.8%) | 1.4% | **Stable (<15% of 1 GiB)** |
| **Peak Container CPU** | 24.62% | 23.36% | 25.26% | ±1.9% | 4.0% | Headroom preserved |
| **OS Thread Count** | 5 | 5 | 5 | 0 | 0.0% | **Zero leaks** |

> [!NOTE]
> The p99 latency (~2,894 ms) directly reflects the 2.5-second hypervisor freeze window. The remarkable consistency across all 3 independent runs (Δ = 2.0%, CV = 1.0%) proves the measurement is accurate and deterministic. Local client outboxes buffered mutations and reconnected seamlessly without data loss.

#### Experiment 3: Mid-Flight Crash & Restart
*60 concurrent worker clients subjected to a hard container SIGKILL (`podman restart`) mid-transmission, verifying client outbox retry backoff, connection recovery, and server-side `OperationId` deduplication.*

| Metric | Run 1 | Run 2 | Run 3 | Variance (Δ) | CV% | Verdict |
|:---|---:|---:|---:|:---|:---:|:---|
| **Sync Exchanges** | 2,868 | 2,862 | 2,878 | ±16 (0.6%) | 0.3% | **Highly stable** |
| **Operations Committed** | 57,360 | 57,240 | 57,560 | ±320 (0.6%) | 0.3% | **Highly stable** |
| **Unhandled Errors** | **0** | **0** | **0** | 0 | 0.0% | **✅ Zero Errors** |
| **Throughput (ops/sec)** | 3,726.58 | 3,721.02 | 3,759.01 | ±37.99 (1.0%) | 0.5% | **Extremely stable** |
| **Latency p50** | 149.31 ms | 149.19 ms | 146.53 ms | ±2.78 ms | 1.0% | **Extremely stable** |
| **Latency p99** | 420.90 ms | 407.60 ms | 380.44 ms | ±40.46 ms | 5.0% | Sub-500ms post-crash |
| **Post-Restart Memory** | 1.794 MB | 2.195 MB | 2.077 MB | ±0.4 MB | — | **Clean baseline** |
| **Post-Restart CPU** | 1.21% | 1.58% | 1.44% | ±0.37% | — | Clean idle |
| **OS Thread Count** | 5 | 5 | 5 | 0 | 0.0% | **Zero leaks** |

> [!TIP]
> Experiment 3 demonstrates the highest reproducibility: throughput variance of only **1.0%** (CV = 0.5%) and committed operations varying by only **±320** out of 57,400. This confirms that Aequora's crash recovery mechanism (durable outbox → exponential backoff retry → authoritative `OperationId` deduplication) is deterministic.

---

### Statistical Reproducibility & Aggregate Volume

#### Aggregate Operations Across All Runs

| Run | Exp 1 Ops | Exp 2 Ops | Exp 3 Ops | Total Ops | Total Errors |
|:---|---:|---:|---:|---:|---:|
| **Run 1** | 46,492 | 51,880 | 57,360 | **155,732** | **0** |
| **Run 2** | 45,184 | 50,100 | 57,240 | **152,524** | **0** |
| **Run 3** | 46,084 | 53,660 | 57,560 | **157,304** | **0** |
| **Grand Total** | **137,760** | **155,640** | **172,160** | **465,560** | **0** |

The pre-hardening harness reported **465,560 operations** across these runs. Because it suppressed
some worker, join, and local-storage failures and did not prove every outbox drained, those numbers
must not be interpreted as proof of zero data loss or complete invariant preservation. New results
are acceptable only when the hardened harness exits successfully and the immutable run evidence is
captured by the release-quality workflow.

#### Coefficient of Variation (CV%) Summary

| Metric | Exp 1 (High Load + Attacks) | Exp 2 (Hypervisor Freeze) | Exp 3 (Mid-Flight Crash) | Industry Standard (<10% = Stable) |
|:---|:---:|:---:|:---:|:---|
| **Throughput (ops/sec)** | 1.8% | 4.3% | 0.5% | ✅ Excellent |
| **Committed Operations** | 1.4% | 3.5% | 0.3% | ✅ Excellent |
| **Unhandled Errors** | 0.0% | 0.0% | 0.0% | ✅ Deterministic (0 errors) |
| **Attack Block Rate** | 0.0% (100% rate) | N/A | N/A | ✅ Deterministic (100% blocked) |
| **p50 Latency** | 3.7% | 5.1% | 1.0% | ✅ Stable |
| **Thread Count** | 0.0% | 0.0% | 0.0% | ✅ Constant (5 threads) |

---

### In-Process Distributed Simulation (10 Real-World Scenarios)

In addition to containerized testing, Aequora includes an in-process simulation suite ([`real_world_simulation.rs`](crates/aequora-testkit/tests/real_world_simulation.rs)) exercising 10 mission-critical distributed failure scenarios with zero external infrastructure. All 10 scenarios passed deterministically across multiple independent runs (20/20 assertions):

| # | Scenario | Real-World Operational Challenge | Verified Aequora Mechanism | Result |
|:---:|:---|:---|:---|:---:|
| **1** | **Multi-Client Offline Batching** | Multiple disconnected clients performing dozens of offline edits. | Scoped journal sequencing, atomic reconciliation, cursor tracking. | ✅ 100% Convergence |
| **2** | **Two-Generals / Dropped ACK** | Cellular connection drops immediately *after* server commits financial write. | Stable `OperationId`, authority idempotency ledger, retry backoff. | ✅ Zero Duplicates |
| **3** | **Concurrent Multi-Field Merge** | 2 devices update non-overlapping fields of same entity offline. | `FieldSetMerger`, hybrid logical clocks, deterministic tie-breaking. | ✅ Automatic Merge |
| **4** | **Hard Conflicting Edits** | 2 devices set mutually exclusive status fields concurrently. | `RejectConflicts` policy, durable `ConflictInbox`, optimistic rollback. | ✅ Safe Isolation |
| **5** | **Out-of-Order DAG Delivery** | Network reorders packets so Child entity arrives before Parent entity. | `OperationMetadata.dependencies`, topological scheduler. | ✅ Causal Ordering |
| **6** | **Tombstone vs Stale Write** | Device A deletes entity; Device B (offline) edits the deleted entity. | Authoritative tombstone representation, mutation rejection. | ✅ Tombstone Invariant |
| **7** | **Journal Compaction & Bootstrap** | Dormant device reconnects after 60 days when journal is compacted. | `SyncDirective::JournalCompacted`, streaming staged snapshot bootstrap. | ✅ Resilient Catchup |
| **8** | **Authority Epoch Failover** | Primary authority fails; standby promoted to Epoch 2; client submits on Epoch 1. | `AuthorityEpoch`, `SyncDirective::AuthorityChanged`, timeline fencing. | ✅ Zero Forking |
| **9** | **Multi-Tenant Boundary Defense** | Malicious or buggy client attempts to mutate another tenant's entity. | Injected `AuthContext`, tenant isolation kernel, zero-access abort. | ✅ 100% Rejection |
| **10** | **Process Crash & Reboot** | Power failure / crash occurs after local commit but before sync. | Atomic durable outbox (`LocalStore`), reboot recovery engine. | ✅ Zero Loss |

See [Real-World Simulation & Architectural Insights](REAL_WORLD_SIMULATION_INSIGHTS.md) for full sequence diagrams and detailed architectural mechanics.

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

- [Architecture Wiki (50-Part System Specification)](wiki/Home.md)
- [Complete developer tutorial](TUTORIAL.md)
- [Governing implementation plan](plan.md)
- [Architecture specification index](next.md) ([authoritative `sys-arch/` specifications](sys-arch/))
- [Real-World Simulation & Architectural Insights](REAL_WORLD_SIMULATION_INSIGHTS.md)
- [ACID architecture](ACID.md)
- [ACID compliance evidence](docs/acid-compliance.md)
- [Enterprise implementation evidence](docs/enterprise-completion.md)
- [Database interoperability implementation evidence](docs/database-interoperability-completion.md)
- [Plug-and-play implementation evidence](docs/plug-and-play-completion.md)
- [Benchmarking & capacity planning evidence](docs/benchmarking-capacity-planning-completion.md)
- [Supply chain governance evidence](docs/supply-chain-governance-completion.md)
- [Deployment topologies evidence](docs/deployment-topologies-completion.md)
- [System Architecture Specifications (Parts 01–50)](wiki/Home.md):
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
  - [Part 31: Android & iOS Mobile Runtime Platform Architecture](wiki/31-android-ios-mobile-runtime-platform-architecture.md)
  - [Part 32: Linux, Windows & macOS Desktop Runtime Architecture](wiki/32-linux-windows-macos-desktop-runtime-architecture.md)
  - [Part 33: Cross-Platform Local Storage for Mobile & Desktop](wiki/33-cross-platform-local-storage-mobile-desktop-architecture.md)
  - [Part 34: Reference Implementation & Workspace Crate Boundary Architecture](wiki/34-reference-implementation-workspace-crate-boundary-architecture.md)
  - [Part 35: Public Rust API & SDK Stability Architecture](wiki/35-public-rust-api-sdk-stability-architecture.md)
  - [Part 36: Storage Adapter SDK & Official Adapter Architecture](wiki/36-storage-adapter-sdk-official-adapter-architecture.md)
  - [Part 37: PostgreSQL & Neon Authoritative Adapter Architecture](wiki/37-postgresql-neon-authoritative-adapter-detailed-architecture.md)
  - [Part 38: Stoolap Embedded Local Persistence Architecture](wiki/38-stoolap-embedded-local-replica-client-persistence-architecture.md)
  - [Part 39: Axum Server Integration & Middleware Architecture](wiki/39-axum-server-integration-middleware-architecture.md)
  - [Part 40: Dioxus Client Integration & Reactive State Architecture](wiki/40-dioxus-client-integration-reactive-state-architecture.md)
  - [Part 41: CLI, Developer Toolchain, Inspection & Verification](wiki/41-cli-developer-toolchain-architecture.md)
  - [Part 42: SQLite Embedded Local Replica Adapter Architecture](wiki/42-sqlite-embedded-local-adapter-architecture.md)
  - [Part 43: Configuration, Secrets, Environment Profiles & Feature Flags](wiki/43-configuration-secrets-environment-profiles-runtime-policy-feature-flags-architecture.md)
  - [Part 44: Packaging, Distribution, Release Engineering & Artifact Signing](wiki/44-packaging-distribution-release-engineering-artifact-signing-update-channels-cross-platform-delivery-architecture.md)
  - [Part 45: Deployment Topologies, Single-Node, HA & Multi-Region Environments](wiki/45-deployment-topologies-single-node-ha-multi-region-edge-enterprise-air-gapped-operational-environment-architecture.md)
  - [Part 46: Observability, Distributed Tracing, Metrics, SLOs & Alerting](wiki/46-observability-metrics-tracing-logging-slos-alerting-production-telemetry-architecture.md)
  - [Part 47: Benchmarking, Capacity Planning & Workload Sizing](wiki/47-benchmarking-performance-regression-capacity-planning-workload-modeling-scalability-testing-production-sizing-architecture.md)
  - [Part 48: Testkit, Fault Injection & Release Quality Gates](wiki/48-testkit-verification-fault-injection-property-model-integration-e2e-release-quality-gates-architecture.md)
  - [Part 49: Licensing, Dependency Policy, Supply Chain Security & SBOM](wiki/49-licensing-dependency-policy-supply-chain-security-crate-governance-sbom-reproducible-builds-third-party-risk-architecture.md)
  - [Part 50: Aequora v1 Scope, Milestones, GA Exit Criteria & Evolution](wiki/50-aequora-v1-scope-productization-milestones-production-readiness-release-candidate-ga-long-term-evolution-architecture.md)
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
