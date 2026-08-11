# Aequora Sync

[![CI](https://github.com/irshadali5/aequora/actions/workflows/ci.yml/badge.svg)](https://github.com/irshadali5/aequora/actions/workflows/ci.yml)
[![Rust 1.87+](https://img.shields.io/badge/MSRV-1.87.0-blue.svg)](https://www.rust-lang.org)
[![Edition 2024](https://img.shields.io/badge/edition-2024-orange.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
[![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE-MIT)
[![Workspace](https://img.shields.io/badge/workspace-26%20libraries%20%2B%201%20dev%20tool-purple.svg)](crates/)

Aequora is a database-neutral, server-authoritative, local-first synchronization engine written in
Rust. It synchronizes typed domain operations and authoritative state transitions—not SQL,
database pages, or vendor-specific write-ahead logs.

Use it to build software that accepts writes offline, reconciles safely after reconnecting, and
keeps application authorization and business rules at the authoritative server.

> Current release line: `0.1.0` · MSRV: Rust `1.87` · Edition: `2024`

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

The workspace contains 26 publishable libraries plus one non-publishable developer utility:

| Area | Crates |
|---|---|
| Facade | `aequora` |
| Core values and protocol | `aequora-types`, `aequora-clock`, `aequora-protocol`, `aequora-codec` |
| Client/server kernel | `aequora-client`, `aequora-server`, `aequora-executor`, `aequora-validator` |
| Storage contracts/adapters | `aequora-store`, `aequora-store-stoolap`, `aequora-store-postgres` |
| Network boundaries | `aequora-transport`, `aequora-http`, `aequora-axum`, `aequora-quic` |
| Domain policies | `aequora-conflict`, `aequora-crdt`, `aequora-partition`, `aequora-journal` |
| Supporting capabilities | `aequora-blob`, `aequora-routing`, `aequora-compute`, `aequora-config`, `aequora-observability` |
| Verification/tooling | `aequora-testkit`, `aequora-dev` |

Run `cargo run -q -p aequora-dev -- summary` for the live workspace graph or
`cargo run -q -p aequora-dev -- graph aequora-client` for one crate's dependency direction.

## Verification

The normal release gates are:

```bash
cargo fmt --all -- --check
cargo +1.87.0 check --workspace --all-targets --all-features --locked
cargo run -q -p aequora-dev --locked -- check
bash scripts/check-database-neutrality.sh
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

## Documentation

- [Complete developer tutorial](TUTORIAL.md)
- [Governing implementation plan](plan.md)
- [Detailed synchronization architecture](next.md)
- [ACID architecture](ACID.md)
- [ACID compliance evidence](docs/acid-compliance.md)
- [Architecture implementation matrix](docs/next-completion.md)
- [Plan completion evidence](docs/plan-completion.md)
- [Custom database adapter guide](docs/custom-database-adapters.md)
- [Local retrieval and tooling guide](docs/local-ai-context.md)

## Project status

The repository-owned implementation described by `plan.md`, `next.md`, and `ACID.md` is present in
code, migrations, public contracts, real Stoolap tests, deterministic simulations, model/property
tests, HTTP/QUIC integration tests, and environment-gated PostgreSQL/Neon suites.

Production acceptance remains deployment-specific. Before calling a deployment complete, run the
live database suites with real credentials and prove TLS, backup restoration, capacity, monitoring,
and graceful rollout/drain against the actual infrastructure.

## License

Licensed under the [MIT License](LICENSE-MIT).
