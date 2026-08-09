# Aequora Sync

[![Rust Version](https://img.shields.io/badge/MSRV-1.87.0-blue.svg)](https://www.rust-lang.org)
[![Edition](https://img.shields.io/badge/edition-2024-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE-MIT)
[![Crates](https://img.shields.io/badge/workspace-26%20crates-purple.svg)](crates/)

**Aequora** is a high-performance, database-agnostic, server-authoritative local-first synchronization engine written in pure Rust.

Instead of replicating raw SQL statements, database pages, or vendor-specific write-ahead logs (WAL), Aequora synchronizes **strongly typed domain operations** and **authoritative state transitions**. This guarantees data integrity, multi-tenant isolation, rich business logic enforcement, and seamless offline-first application capabilities.

---

## Key Principles & Architectural Design

Aequora separates concerns into three completely independent, decoupled composition axes:

```text
┌────────────────────────────────┐     ┌────────────────────────────────┐     ┌────────────────────────────────┐
│       Local Persistence        │     │     Network Transport Layer    │     │     Authoritative Storage      │
│      (trait LocalStore)        │ ──► │      (trait SyncTransport)     │ ──► │   (trait AuthoritativeStore)   │
│ e.g., Stoolap, Redb, In-Memory │     │ e.g., Axum/HTTP, QUIC, In-Proc │     │ e.g., PostgreSQL, Neon, Custom │
└────────────────────────────────┘     └────────────────────────────────┘     └────────────────────────────────┘
```

1. **Database Neutrality**: The core synchronization protocol, framed codecs, conflict resolution engine, and operation execution pipeline operate exclusively against capability traits (`LocalStore` and `AuthoritativeStore`). Client databases and server databases are completely independent adapters at the outer edge.
2. **Server-Authoritative Local-First Sync**: Local client mutations are executed immediately in local storage and staged into a durable outbox transactionally. When connectivity is restored, the `ClientSyncEngine` streams Postcard-encoded envelopes to the server, where registered application handlers validate authorization, check tenant boundaries, enforce optimistic concurrency, and commit state transitions atomically.
3. **Optimistic Concurrency & HLC Metadata**: Every domain mutation carries a strongly typed `EntityRef`, `EntityVersion`, `OperationId`, `TenantId`, `DeviceId`, and a Hybrid Logical Clock (`HybridTimestamp`) timestamp ensuring strict deterministic ordering across distributed nodes without requiring central lock coordination.
4. **Framed Postcard Binary Protocol**: Wire DTOs use compact Postcard serialization framed by a 16-byte header (`AEQ1` magic identifier, protocol version, payload length, and BLAKE3 checksum). Bounded visitor allocations protect servers against malicious memory exhaustion attacks.
5. **Rayon Compute Offloading**: CPU-intensive operations—such as batch dependency analysis, BLAKE3 content hashing, zstd payload decompression, and topological sorting—are automatically offloaded to a dedicated Rayon thread pool, ensuring Tokio async I/O worker threads remain unblocked.

---

## Architectural Dataflow

The following sequence diagram illustrates the lifecycle of an offline client mutation synchronized to an authoritative server:

```mermaid
sequenceDiagram
    autonumber
    actor User as User / App Code
    participant LocalDB as Client Local Storage<br/>(e.g., Stoolap)
    participant ClientEngine as Client Sync Engine<br/>(ClientSyncEngine)
    participant Gateway as Sync Transport / HTTP<br/>(Axum Gateway)
    participant Server as Sync Server<br/>(SyncServer)
    participant AuthorityDB as Authoritative Storage<br/>(e.g., PostgreSQL / Neon)

    User->>LocalDB: Transact Domain Mutation & Outbox Append
    Note over LocalDB: Local state updated<br/>Outbox entry staged offline
    
    loop Background Sync Loop
        ClientEngine->>LocalDB: Fetch Unacknowledged Outbox Operations
        ClientEngine->>Gateway: POST /sync/v1/exchange (AEQ1 Postcard Envelopes)
        Gateway->>Server: Forward Framed Protocol Request
        
        Note over Server: 1. Validate AEQ1 Header & BLAKE3 Checksum<br/>2. Authenticate Session & Tenant ID<br/>3. Authorize Scope & Operation Permissions<br/>4. Construct Dependency DAG (Rayon)<br/>5. Apply Business Logic & OCC Checks
        
        Server->>AuthorityDB: Commit Mutated State + Scope Sequence + Journal + Idempotency Ledger (Atomic Tx)
        AuthorityDB-->>Server: Tx Success + New Scope Cursor Watermark
        
        Server-->>Gateway: Construct SyncResponse (Acks + Downstream Changes)
        Gateway-->>ClientEngine: Stream Response
        
        ClientEngine->>LocalDB: Apply Downstream Changes + Advance Cursor + Prune Outbox (Atomic Tx)
    end
```

---

## Workspace Crates Architecture

The workspace consists of 26 focused crates, structured into foundational types, execution layers, storage backends, and networking facades:

| Crate | Responsibility / Description | Key Types & Exports |
|---|---|---|
| [`aequora`](crates/aequora) | Top-level workspace facade re-exporting prelude & feature-gated subsystems | `prelude::*`, `stoolap`, `postgres`, `axum`, `quic` |
| [`aequora-types`](crates/aequora-types) | Strongly typed UUIDv7 identifiers, versions, sequences, and timestamps | `EntityId`, `TenantId`, `DeviceId`, `ActorId`, `OperationId`, `HybridTimestamp` |
| [`aequora-clock`](crates/aequora-clock) | Hybrid Logical Clock (HLC) for physical/logical timestamping & drift enforcement | `HlcClock`, `SystemClock`, `Clock` |
| [`aequora-codec`](crates/aequora-codec) | `AEQ1` framed binary codec, BLAKE3 checksum verification, zstd compression | `FramedCodec`, `FrameHeader`, `BoundedVisitor` |
| [`aequora-crdt`](crates/aequora-crdt) | Conflict-Free Replicated Data Types & state-based CRDT merging | `GCounter`, `PnCounter`, `PostcardCrdtMerger` |
| [`aequora-conflict`](crates/aequora-conflict) | Operational & state-based conflict detection, optimistic concurrency policies | `ConflictPolicyRegistry`, `FieldSetMerger`, `FinancialOperation` |
| [`aequora-journal`](crates/aequora-journal) | Log-structured sync journal, compaction planner, idempotency ledger, audit log | `CompactionPlanner`, `CursorWatermarks`, `TombstoneRetention` |
| [`aequora-blob`](crates/aequora-blob) | BLAKE3 content-addressed blob storage, chunk manifests, atomic reference store | `BlobStore`, `BlobDigest`, `BlobManifest`, `InMemoryBlobStore` |
| [`aequora-partition`](crates/aequora-partition) | Multi-tenant partition rules, boolean/hierarchical partial-sync authorization | `PartitionPolicy`, `PartitionExpression`, `PartitionHierarchy` |
| [`aequora-routing`](crates/aequora-routing) | Multi-region routing, read-replica dispatch, write-safety enforcement | `RegionRouter`, `RegionHealth`, `RouteDecision` |
| [`aequora-store`](crates/aequora-store) | Core storage abstraction traits for local and authoritative databases | `LocalStore`, `AuthoritativeStore`, `OutboxStore`, `AuditLog` |
| [`aequora-store-stoolap`](crates/aequora-store-stoolap) | Local MVCC database adapter using Stoolap (checksummed DDL migrations) | `StoolapDatabase`, `StoolapStore`, `STOOLAP_SCHEMA_VERSION` |
| [`aequora-store-postgres`](crates/aequora-store-postgres) | Authoritative server adapter for PostgreSQL & Neon (pooled & direct modes) | `SqlxPostgresBackend`, `PostgresStore`, `POSTGRES_SCHEMA_VERSION` |
| [`aequora-protocol`](crates/aequora-protocol) | Protocol DTOs for sync requests, responses, push hints, and streaming snapshots | `SyncRequest`, `SyncResponse`, `OperationEnvelope`, `SnapshotPage` |
| [`aequora-transport`](crates/aequora-transport) | Network transport traits and deterministic in-process channel transport | `SyncTransport`, `StreamingSyncTransport`, `InProcessTransport` |
| [`aequora-quic`](crates/aequora-quic) | High-performance QUIC transport implementation using Quinn & Rustls | `QuicClientTransport`, `QuicServerListener` |
| [`aequora-axum`](crates/aequora-axum) | Axum HTTP server integration handling `POST /sync/v1/exchange` | `SyncExchangeHandler`, `axum_sync_router` |
| [`aequora-http`](crates/aequora-http) | Reqwest Postcard-over-HTTP client transport | `HttpSyncTransport`, `HttpTransportConfig` |
| [`aequora-executor`](crates/aequora-executor) | Operation execution pipeline, dependency DAG planning, Rayon execution | `OperationExecutor`, `OperationRegistry`, `OperationHandler` |
| [`aequora-validator`](crates/aequora-validator) | Schema migration validator, payload inspector, scope authorization rules | `PayloadMigrator`, `ScopeAuthorizer`, `ValidatedOperation` |
| [`aequora-compute`](crates/aequora-compute) | Dedicated Rayon compute pool offloader for heavy hashing/compression | `ComputePool`, `OffloadTask` |
| [`aequora-config`](crates/aequora-config) | Strictly typed RON configuration parser with cross-limit validation | `AequoraConfig`, `ClientConfig`, `ServerConfig` |
| [`aequora-observability`](crates/aequora-observability) | Zero-allocation payload-free observers, atomic metrics counters, trace contexts | `Observer`, `AtomicMetrics`, `TraceContext` |
| [`aequora-client`](crates/aequora-client) | `ClientSyncEngine`, background sync coordinator, exponential backoff engine | `ClientSyncEngine`, `ClientSyncEngineBuilder`, `SyncCoordinator` |
| [`aequora-server`](crates/aequora-server) | `SyncServer`, server transaction coordinator, command execution gateway | `SyncServer`, `SyncServerBuilder`, `ExchangeService` |
| [`aequora-testkit`](crates/aequora-testkit) | Deterministic in-memory stores, in-process network, behavioral conformance suite | `InMemoryLocalStore`, `InMemoryAuthoritativeStore`, `ConformanceSuite` |

---

## Quickstart & Usage

### 1. Minimal In-Process Synchronization

The example below demonstrates setting up an in-memory client and server using `aequora-testkit`:

```rust,no_run
use aequora::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tenant_id = TenantId::new();
    let actor_id = ActorId::new();
    let device_id = DeviceId::new();
    let scope_id = SyncScopeId::new();

    let session = SessionMetadata {
        session_id: SessionId::new(),
        device_id,
        actor_id,
        tenant_id,
        scope_id,
        partitions: vec![],
    };

    let auth_context = AuthContext {
        actor_id,
        tenant_id,
        device_id,
    };

    // 1. Authoritative Server Setup
    let authoritative_store = Arc::new(aequora::testkit::InMemoryAuthoritativeStore::default());
    let server: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
        authoritative_store,
        Arc::new(aequora::testkit::AllowAllExecutor),
        Arc::new(RejectConflicts),
        Arc::new(SystemClock::new(NodeId::new())),
    ));

    // 2. Client Local Store & Transport Setup
    let local_store = aequora::testkit::InMemoryLocalStore::default();
    let transport = aequora::testkit::InProcessTransport::new(server, auth_context);

    // 3. Client Sync Engine
    let engine = ClientSyncEngine::new(
        local_store,
        transport,
        ClientConfig::new(session),
    );

    // 4. Run Sync Cycle
    let outcome = engine.run_once().await?;
    println!("Sync outcome: acknowledged={}, changes={}", outcome.acknowledged, outcome.changes);

    Ok(())
}
```

---

## Database Adapters & Feature Flags

Selection of client storage, authority storage, and transport backends is configured independently via Cargo features:

| Integration Target | Feature Flag | Struct / Backend | Role |
|---|---|---|---|
| Stoolap Local Storage | `stoolap` | `StoolapDatabase`, `StoolapStore` | Embedded MVCC client store with transactional outbox |
| PostgreSQL / Neon Authority | `postgres` | `SqlxPostgresBackend`, `PostgresStore` | Server authoritative backend (supports pooled & direct migration URLs) |
| Axum HTTP Gateway | `axum` | `ExchangeService`, Axum Router | Server-side HTTP synchronization endpoint |
| Reqwest HTTP Client | `http-client` | `HttpSyncTransport` | Client-side Postcard-over-HTTP transport |
| Quinn QUIC Transport | `quic` | `QuicClientTransport`, `QuicServerListener` | Ultra-low latency UDP/QUIC transport |
| In-Memory Test Kit | `testkit` | `InMemoryLocalStore`, `InProcessTransport` | Deterministic simulation & behavioral test suite |

### Neon PostgreSQL Configuration Example

For Neon cloud deployments, `SqlxPostgresBackend::connect_neon` configures transaction-pooled endpoints for runtime traffic alongside direct migration endpoints for schema installation:

```rust,no_run
use aequora::postgres::SqlxPostgresBackend;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let backend = SqlxPostgresBackend::connect_neon(
    "postgres://user:pass@ep-pooled.neon.tech/main",  // Transaction pooled URL
    "postgres://user:pass@ep-direct.neon.tech/main",  // Direct migration URL
    10,                                                // Max pool connections
).await?;

backend.health_check().await?;
# Ok(())
# }
```

---

## Configuration

Aequora uses RON (Rusty Object Notation) for type-safe, strict runtime configuration parsing:

```rust,no_run
use aequora::prelude::*;

# fn configure() -> Result<(), Box<dyn std::error::Error>> {
let ron_config = r#"
(
    protocol: (max_frame_bytes: 4194304),
    push: (max_operations: 256),
    coordinator: (sync_on_start: true, interval_secs: 30)
)
"#;

let config = AequoraConfig::from_ron(ron_config)?;
# Ok(())
# }
```

---

## Verification & Testing Suite

Execute the full workspace quality gate commands:

```bash
# Code formatting check
cargo fmt --all -- --check

# Offline compilation check for exact MSRV 1.87.0
cargo +1.87.0 check --workspace --all-targets --all-features --offline

# Workspace Clippy lint check
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Execute all workspace tests
cargo test --workspace --all-features

# Documentation build with warnings treated as errors
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps

# Check fuzz binaries & benchmarks
cargo check --manifest-path fuzz/Cargo.toml --bins
cargo bench -p aequora-testkit --bench core_pipeline --no-run

# Run the full School ERP example
cargo run -p aequora --example school_erp --features stoolap,testkit
```

---

## License

This project is licensed under the [MIT License](LICENSE-MIT).
