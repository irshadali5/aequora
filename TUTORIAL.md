# Aequora: A Complete Local-First Synchronization Tutorial

This tutorial explains how to build a reusable, server-authoritative, local-first application with
Aequora. It covers the architecture, a complete domain-operation flow, client and server assembly,
Stoolap and PostgreSQL/Neon integration, HTTP/Axum transport, conflict handling, snapshots,
observability, testing, custom adapters, and production operations.

The examples target Aequora `0.1.0`, Rust `1.87` or newer, and Edition 2024. The repository's
runnable examples and tests are the source of truth if the public API changes after this document
is published.

> Aequora synchronizes typed domain operations and authoritative state transitions. It does not
> replicate SQL statements, database pages, or write-ahead logs.

## Table of contents

1. [What Aequora solves](#1-what-aequora-solves)
2. [The system model and guarantees](#2-the-system-model-and-guarantees)
3. [Install Aequora and select features](#3-install-aequora-and-select-features)
4. [Run the repository examples](#4-run-the-repository-examples)
5. [Model a domain operation](#5-model-a-domain-operation)
6. [Create an atomic local mutation](#6-create-an-atomic-local-mutation)
7. [Build an in-process end-to-end system](#7-build-an-in-process-end-to-end-system)
8. [Configure Aequora from RON](#8-configure-aequora-from-ron)
9. [Run a PostgreSQL or Neon authority](#9-run-a-postgresql-or-neon-authority)
10. [Expose the service through Axum](#10-expose-the-service-through-axum)
11. [Connect a production HTTP client](#11-connect-a-production-http-client)
12. [Run background synchronization](#12-run-background-synchronization)
13. [Understand retries, idempotency, and ACID](#13-understand-retries-idempotency-and-acid)
14. [Choose conflict semantics](#14-choose-conflict-semantics)
15. [Bootstrap, cursors, scopes, and tombstones](#15-bootstrap-cursors-scopes-and-tombstones)
16. [Synchronize large blobs separately](#16-synchronize-large-blobs-separately)
17. [Add metrics and tracing](#17-add-metrics-and-tracing)
18. [Implement a custom database adapter](#18-implement-a-custom-database-adapter)
19. [Use QUIC or a custom transport](#19-use-quic-or-a-custom-transport)
20. [Test correctness and failure recovery](#20-test-correctness-and-failure-recovery)
21. [Deploy and operate safely](#21-deploy-and-operate-safely)
22. [Troubleshoot common failures](#22-troubleshoot-common-failures)
23. [Reusable project checklist](#23-reusable-project-checklist)
24. [Where to go next](#24-where-to-go-next)

---

## 1. What Aequora solves

A local-first application accepts user work without waiting for the network. A user can mark
attendance, edit a task, record an inspection, or draft an order while offline. The local database
updates immediately, and a durable outbox remembers the intent until it reaches the authority.

That convenience creates difficult correctness questions:

- What happens if the client crashes after changing local state but before writing the outbox?
- What happens if the server commits but the response is lost?
- What happens when two devices update the same entity from the same old version?
- When is it safe to advance the client's cursor?
- How does a new device obtain current state without replaying an unbounded journal?
- Can the application replace either database without rewriting the protocol?

Aequora supplies the synchronization kernel for those questions. Your application still owns its
domain model, authorization policy, authentication system, user interface, and business-specific
database tables.

The three composition axes are independent:

```text
local persistence       LocalStore
authoritative storage   AuthoritativeStore
network boundary        SyncTransport / ExchangeService
```

The built-in production path is:

```text
Stoolap client
    │
    │ AEQ1 framed Postcard over HTTPS
    ▼
Axum gateway
    │
    ▼
PostgreSQL or Neon authority
```

This is an integration profile, not a protocol identity. A custom local adapter, authority adapter,
or transport can replace its corresponding axis independently.

---

## 2. The system model and guarantees

### 2.1 The operation lifecycle

One offline mutation moves through four durable boundaries:

```text
1. Client local transaction
   optimistic entity update + outbox insertion

2. At-least-once delivery
   stable OperationId across every retry

3. Server authoritative transaction
   entity + version + journal + operation result + audit

4. Client reconciliation transaction
   changes + applied markers + terminal outbox state + conflicts + cursor
```

The client and server databases do not share one distributed transaction. Aequora instead combines
local ACID, durable intent, idempotent authoritative execution, and local reconciliation ACID.

### 2.2 What Aequora guarantees

- A stable `OperationId` identifies one logical command across retries.
- A committed authoritative operation has one durable result in the operation ledger.
- Authoritative state, entity version, journal event, operation result, and audit record commit
  atomically in compliant authority adapters.
- Client reconciliation applies authoritative changes and advances the cursor atomically.
- Cursors never move backward and never skip an incomplete authoritative sequence.
- Entity versions advance monotonically by exactly one.
- Batches are bounded by operation count and encoded frame bytes.
- Retry state, attempt count, and next-attempt deadline can survive process restart.
- Protocol, operation schema, local schema, authority schema, and snapshot capabilities evolve
  independently.

### 2.3 What Aequora deliberately does not guarantee

- There is no cross-client/server two-phase commit.
- A request batch is not one atomic business transaction. Each operation is independently atomic.
- Dependency ordering does not imply group rollback.
- HTTP success is not authoritative truth; the durable operation ledger is.
- Wall-clock order is not an authoritative conflict decision by itself.
- Financial balancing, payment-provider idempotency, and application side-effect outboxes remain
  application responsibilities.
- Authentication credentials, TLS certificates, database URLs, backups, and restoration policy
  remain host-application responsibilities.

These boundaries make failure behavior explainable instead of pretending an offline distributed
system has stronger semantics than it can provide.

---

## 3. Install Aequora and select features

### 3.1 Minimum toolchain

```bash
rustup toolchain install 1.87.0 --profile minimal
rustup override set 1.87.0
```

For a released crate:

```toml
[dependencies]
aequora = { version = "0.1.0", features = ["stoolap", "http-client"] }
async-trait = "0.1"
postcard = { version = "1.1", features = ["use-std"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1.45", features = ["macros", "net", "rt-multi-thread", "sync", "time"] }
```

The worked snippets also use `hex`, `http`, `reqwest`, and `axum` at their application boundaries:

```toml
hex = "0.4"
http = "1"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
axum = "0.8"
```

While developing from a checkout, use a path dependency:

```toml
[dependencies]
aequora = { path = "../aequora/crates/aequora", features = ["stoolap", "http-client"] }
```

### 3.2 Feature matrix

| Feature | Adds | Typical owner |
|---|---|---|
| `postcard` | Default framed binary protocol support | Every client and server |
| `stoolap` | `StoolapDatabase` and `StoolapStore` | Native/local client |
| `postgres` | `SqlxPostgresBackend` and `PostgresStore` | Authority server |
| `axum` | HTTP exchange/bootstrap routes and lifecycle | Authority server |
| `http-client` | Bounded Reqwest transport | Client |
| `quic` | Quinn transport and server adapter | Optional low-latency deployment |
| `testkit` | Deterministic stores, transport, clocks, contracts | Tests and examples |
| `ron` | Diagnostic RON codec | Development/diagnostics |
| `json` | Diagnostic JSON codec | Development/diagnostics |
| `tracing` | Payload-free tracing observer | Production observability |

Use the narrowest feature set for each binary:

```toml
# Client binary
aequora = { version = "0.1.0", features = ["stoolap", "http-client", "tracing"] }

# Server binary
aequora = { version = "0.1.0", features = ["postgres", "axum", "tracing"] }

# Integration-test crate
aequora = { version = "0.1.0", features = ["stoolap", "postgres", "axum", "http-client", "testkit"] }
```

Do not enable both database features merely because both exist. A mobile client does not need SQLx,
and an authority server does not need Stoolap.

---

## 4. Run the repository examples

Clone the repository and run the deterministic in-process example:

```bash
cargo run -p aequora --example in_process --features testkit --locked
```

Run the more complete offline-first attendance example with a real Stoolap transaction:

```bash
cargo run -p aequora --example school_erp --features stoolap,testkit --locked
```

The source files are intentionally small enough to reuse:

- [`crates/aequora/examples/in_process.rs`](crates/aequora/examples/in_process.rs) demonstrates the
  minimum operation/outbox/exchange/reconciliation path.
- [`crates/aequora/examples/school_erp.rs`](crates/aequora/examples/school_erp.rs) demonstrates a
  typed command, scope authorization, local optimistic state, a Stoolap outbox transaction, server
  execution, and authoritative reconciliation.

Start from one of these examples before introducing HTTP or PostgreSQL. If the in-process version
does not work, a network or hosted database will only make the problem harder to diagnose.

---

## 5. Model a domain operation

This section builds a `MarkAttendance` command. The same pattern works for tasks, inspections,
inventory adjustments, forms, field-service reports, and other domain mutations.

### 5.1 Define stable command data

```rust
use aequora::prelude::{ActorId, DomainOperation, EntityId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MarkAttendance {
    student_id: EntityId,
    teacher_id: ActorId,
    school_day: i64,
    present: bool,
}

impl DomainOperation for MarkAttendance {
    const KIND: u16 = 100;
    const CURRENT_SCHEMA: u16 = 1;
}
```

Treat `KIND` as a permanent wire identifier. Never reuse an old number for a different command.
Treat `CURRENT_SCHEMA` as the version of the command payload, not the database schema or wire
protocol version.

Good command design has these properties:

- It expresses intent, such as `MarkAttendance`, instead of shipping raw SQL or a table patch.
- It includes the stable IDs needed for validation.
- It avoids secrets and unbounded data.
- It is deterministic to decode and validate.
- It can be rejected safely without partially mutating authoritative state.

### 5.2 Authorize synchronization scope

Scope authorization runs before authoritative entity or journal data is read:

```rust
use aequora::prelude::{AuthContext, ExecutionError, ScopeAuthorizer, SessionMetadata};
use async_trait::async_trait;

struct SchoolScopeAuthorizer;

#[async_trait]
impl ScopeAuthorizer for SchoolScopeAuthorizer {
    async fn authorize_scope(
        &self,
        auth: &AuthContext,
        session: &SessionMetadata,
    ) -> Result<(), ExecutionError> {
        if auth.tenant_id == session.tenant_id {
            Ok(())
        } else {
            Err(ExecutionError::unauthorized(
                "requested synchronization scope is not assigned",
            ))
        }
    }
}
```

A production authorizer normally checks tenant membership, device registration, role, and each
opaque partition selector. Do not trust `SessionMetadata` merely because it decoded successfully.

### 5.3 Implement the typed handler

```rust
use aequora::prelude::{
    AuthContext, AuthoritativeMutation, ChangeKind, CurrentEntity, ExecutionError,
    OperationEnvelope, OperationHandler,
};
use async_trait::async_trait;

struct AttendanceHandler;

#[async_trait]
impl OperationHandler<MarkAttendance> for AttendanceHandler {
    async fn authorize(
        &self,
        auth: &AuthContext,
        command: &MarkAttendance,
        _envelope: &OperationEnvelope,
    ) -> Result<(), ExecutionError> {
        if command.teacher_id == auth.actor_id {
            Ok(())
        } else {
            Err(ExecutionError::unauthorized(
                "teacher cannot submit attendance for another actor",
            ))
        }
    }

    async fn execute(
        &self,
        _auth: &AuthContext,
        command: &MarkAttendance,
        _envelope: &OperationEnvelope,
        _current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        if command.school_day <= 0 {
            return Err(ExecutionError::business_rule(
                "school day must be positive",
            ));
        }

        let payload = postcard::to_stdvec(command)
            .map_err(|_| ExecutionError::invalid_operation("attendance encoding failed"))?;

        Ok(AuthoritativeMutation {
            payload,
            change_kind: ChangeKind::Upsert,
        })
    }
}
```

The handler returns an opaque authoritative representation. The store adapter does not interpret
this payload. The handler should perform CPU-only decoding, authorization, and business validation;
network calls and external side effects do not belong inside the authoritative transaction.

### 5.4 Register the handler

```rust
use aequora::prelude::OperationRegistry;

let mut registry = OperationRegistry::new(SchoolScopeAuthorizer);
registry.register::<MarkAttendance, _>(AttendanceHandler)?;
```

The registry rejects duplicate operation kinds. For an older command schema, implement
`PayloadMigrator` and call `register_with_migration`. Keep its compatibility window bounded and
test every supported historical payload.

---

## 6. Create an atomic local mutation

### 6.1 Build the operation envelope

An envelope carries identity, optimistic-version, routing, schema, and dependency metadata around
the typed command bytes:

```rust
use aequora::prelude::{
    ActorId, DeviceId, EntityId, EntityRef, EntityType, HybridTimestamp, NodeId,
    DomainOperation, OperationEnvelope, OperationId, OperationKind, OperationMetadata,
    ProtocolVersion, SchemaVersion, SyncScopeId, TenantId,
};

let tenant = TenantId::new();
let actor = ActorId::new();
let device = DeviceId::new();
let scope = SyncScopeId::new();
let student = EntityId::new();
let entity = EntityRef {
    entity_type: EntityType::new(20)?,
    entity_id: student,
};

let command = MarkAttendance {
    student_id: student,
    teacher_id: actor,
    school_day: 20_260_811,
    present: true,
};
let payload = postcard::to_stdvec(&command)?;

let operation = OperationEnvelope {
    protocol_version: ProtocolVersion::V1,
    operation_id: OperationId::new(),
    tenant_id: tenant,
    actor_id: actor,
    device_id: device,
    entity,
    base_version: None,
    created_at: HybridTimestamp {
        physical_ms: 1_000,
        logical: 0,
        node: NodeId::new(),
    },
    schema_version: SchemaVersion(MarkAttendance::CURRENT_SCHEMA),
    operation_kind: OperationKind(MarkAttendance::KIND),
    payload: payload.clone(),
    metadata: OperationMetadata::default(),
};
```

Use `base_version: None` only for creation. For an update, copy the last reconciled
`EntityVersion` into `base_version`. Never generate a new `OperationId` when retrying the same
logical action.

### 6.2 Open persistent Stoolap storage

`StoolapDatabase::open` installs and verifies Aequora's local metadata migrations:

```rust
use aequora::stoolap::StoolapDatabase;

let local_backend = StoolapDatabase::open("file:///var/lib/my-app/client")?;
local_backend.health_check()?;
assert!(local_backend.schema_status()?.is_current());
```

Use `StoolapDatabase::open_in_memory()` for tests. A file-backed DSN is required to prove restart
durability.

### 6.3 Commit the optimistic entity and outbox together

The optimistic UI state and outbox insertion must share one native database transaction:

```rust
use aequora::store::StoreError;

local_backend.transact_local_mutation(&operation, |transaction| {
    transaction
        .execute(
            "INSERT INTO aequora_local_entities \
             (scope_id, entity_type, entity_id, version, payload, tombstone, provisional) \
             VALUES ($1, $2, $3, 1, $4, 0, 1)",
            (
                scope.to_string(),
                i64::from(entity.entity_type.get()),
                entity.entity_id.to_string(),
                hex::encode(&payload),
            ),
        )
        .map_err(|error| StoreError::transient(error.to_string()))?;
    Ok(())
})?;
```

`transact_local_mutation` appends the supplied operation within the same transaction as the
application callback. If either part fails, neither becomes visible.

Do not follow this unsafe sequence:

```text
update local entity
commit
append outbox operation
```

A crash between those commits creates local state that can never synchronize.

### 6.4 Keep provisional state visible to the application

The example schema uses a `provisional` flag. Your repository can render provisional entities
immediately and clear the flag when authoritative reconciliation arrives. Keep synchronization
metadata out of UI logic where possible; expose a small application status model instead.

---

## 7. Build an in-process end-to-end system

Before adding sockets or PostgreSQL, prove the operation with the deterministic testkit.

```rust
use aequora::{
    client::{ClientConfig, ClientSyncEngine},
    clock::TestClock,
    conflict::RejectConflicts,
    executor::AuthContext,
    protocol::SessionMetadata,
    server::{ExchangeService, SyncServer},
    stoolap::StoolapStore,
    testkit::{InMemoryAuthoritativeStore, InProcessTransport},
    types::{NodeId, SessionId, SyncScopeId},
};
use std::sync::Arc;

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

let authoritative = InMemoryAuthoritativeStore::default();
let service: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
    Arc::new(authoritative.clone()),
    Arc::new(registry),
    Arc::new(RejectConflicts),
    Arc::new(TestClock::new(NodeId::new(), 2_000)),
));

let engine = ClientSyncEngine::new(
    StoolapStore::new(local_backend),
    InProcessTransport::new(service, auth),
    ClientConfig::new(session),
);

let outcome = engine.run_once().await?;
assert_eq!(outcome.acknowledged, 1);
assert_eq!(authoritative.applied_operation_count(), 1);
```

`run_once` performs one bounded push/pull exchange. `sync` drains multiple bounded batches/pages
until complete or until `max_exchanges_per_sync` is reached.

The in-memory authority is a deterministic semantic reference, not production durability evidence.
Production adapters must separately prove real transaction, rollback, concurrency, migration, and
restart behavior.

---

## 8. Configure Aequora from RON

`AequoraConfig` contains secret-free runtime tuning. Database URLs, access tokens, and TLS material
must come from the host's secret manager or environment, never this file.

Create `config/aequora.ron`:

```ron
(
    protocol: (
        minimum_version: 1,
        version: 1,
    ),
    push: (
        max_operations: 128,
        max_bytes: 1048576,
        max_wait_ms: 150,
        adaptive: Some((
            minimum_operations: 16,
            maximum_operations: 256,
            increase_step: 16,
            target_latency_ms: 150,
        )),
    ),
    pull: (
        max_events: 1024,
        max_bytes: 4194304,
    ),
    retry: (
        max_attempts: 5,
        initial_ms: 500,
        max_ms: 30000,
        multiplier: 2,
        jitter_percent: 20,
        max_exchanges_per_sync: 1024,
    ),
    compute: (
        worker_threads: 4,
        parallel_threshold: 128,
    ),
    compression: (
        algorithm: Zstd,
        min_bytes: 4096,
        zstd_level: 3,
    ),
    limits: (
        max_operation_bytes: 262144,
        max_dependencies: 32,
        max_trace_id_bytes: 128,
        max_partitions: 32,
        max_partition_bytes: 1024,
        max_decompressed_bytes: 4194304,
        max_snapshot_entities: 512,
        max_snapshot_bytes: 4194304,
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

Load and map it into each runtime component:

```rust
use aequora::config::AequoraConfig;

let text = std::fs::read_to_string("config/aequora.ron")?;
let config = AequoraConfig::from_ron(&text)?;

let client_config = config.client_config(session.clone())?;
let server_config = config.server_config()?;
let coordinator_config = config.coordinator_config()?;
let compute_config = config.compute_config()?;
```

With the relevant features enabled, the same validated object also produces `axum_config`,
`http_transport_config`, and `quic_config`. Unknown fields and unsafe zero/cross-field bounds are
rejected. This prevents client and server limits from silently drifting apart.

---

## 9. Run a PostgreSQL or Neon authority

### 9.1 Ordinary PostgreSQL

Enable `postgres`, obtain the URL from the host environment, and create a bounded pool:

```rust
use aequora::postgres::{PostgresPoolConfig, PostgresStore, SqlxPostgresBackend};

let database_url = std::env::var("DATABASE_URL")?;
let backend = SqlxPostgresBackend::connect_with_config(
    &database_url,
    PostgresPoolConfig::new(10),
)
.await?;

backend.health_check().await?;
assert!(backend.schema_status().await?.is_current());

let authority = Arc::new(PostgresStore::new(backend.clone()));
```

The connect methods install and verify Aequora's checksummed migrations. If your deployment uses
different runtime and migration roles, use `connect_with_migration_url`.

### 9.2 Neon

Neon should use its pooled URL for normal transactions and direct URL for migrations:

```rust
let pooled_url = std::env::var("NEON_POOLED_DATABASE_URL")?;
let direct_url = std::env::var("NEON_DIRECT_DATABASE_URL")?;

let backend = SqlxPostgresBackend::connect_neon(
    &pooled_url,
    &direct_url,
    10,
)
.await?;
```

The Neon constructor forces certificate and hostname verification and uses a scale-to-zero-friendly
pool. Size `max_connections` across every replica, not independently per process without regard for
the provider's connection budget.

### 9.3 Build the server

```rust
use aequora::{
    clock::SystemClock,
    compute::ComputePool,
    conflict::RejectConflicts,
    server::SyncServerBuilder,
    types::NodeId,
};

let compute = Arc::new(ComputePool::new(config.compute_config()?)?);
let server = SyncServerBuilder::new()
    .store(authority.clone())
    .executor(Arc::new(registry))
    .conflicts(Arc::new(RejectConflicts))
    .clock(Arc::new(SystemClock::new(NodeId::new())))
    .config(config.server_config()?)
    .compute_pool(compute)
    .build();
```

Tokio owns network and database I/O. The dedicated Rayon pool is for bounded CPU-heavy planning,
hashing, validation, compression preparation, and large diffs. Never hold a database transaction
open while waiting for Rayon or external I/O.

---

## 10. Expose the service through Axum

### 10.1 Create readiness and lifecycle-aware routes

```rust
use aequora::{
    axum::{ReadinessFn, router_with_lifecycle},
    observability::AtomicMetrics,
    server::ExchangeService,
};
use std::sync::Arc;

let readiness_backend = backend.clone();
let readiness = Arc::new(ReadinessFn::new(move || {
    let backend = readiness_backend.clone();
    async move { backend.health_check().await.is_ok() }
}));

let service: Arc<dyn ExchangeService> = Arc::new(server);
let metrics = Arc::new(AtomicMetrics::default());
let (router, lifecycle) = router_with_lifecycle(
    service,
    config.axum_config()?,
    metrics.clone(),
    readiness,
);
```

The router exposes:

| Route | Purpose |
|---|---|
| `POST /sync/v1/exchange` | Bounded bidirectional incremental synchronization |
| `POST /sync/v1/bootstrap` | Bounded/resumable snapshot bootstrap |
| `GET /sync/v1/health` | Compatibility liveness route |
| `GET /sync/v1/health/live` | Process liveness, independent of dependencies |
| `GET /sync/v1/health/ready` | Dependency readiness; unavailable while draining |

### 10.2 Install authentication before the Aequora routes

The host must validate a JWT, session cookie, API token, or mTLS identity and insert an
`AuthContext` into request extensions. The Axum adapter deliberately does not choose an identity
provider.

```rust
use aequora::executor::AuthContext;
use axum::Extension;

// Tests may install a fixed context:
let test_router = router.layer(Extension(AuthContext {
    actor_id,
    tenant_id,
    device_id,
}));
```

In production, never accept actor, tenant, or device identity directly from unverified headers or
the request body. Authentication middleware must derive them from verified credentials. Aequora
then compares every envelope/session claim with that connection-derived context.

### 10.3 Understand HTTP admission behavior

The Axum boundary applies controls before expensive decoding or execution:

- global and per-tenant in-flight limits;
- per-tenant sustained/burst rate limits;
- bounded tenant-bucket retention;
- compressed body byte limits;
- body-read and authoritative execution deadlines;
- decompression limits;
- `Retry-After` on transient overload/deadline responses.

Clients classify `408`, `429`, `503`, and `504` as transient. `413 Payload Too Large` is permanent
until the operation or configured limit changes.

### 10.4 Drain gracefully

```rust
use aequora::axum::DrainOutcome;

match lifecycle.drain(config.axum_config()?.drain_timeout).await {
    DrainOutcome::Drained => {}
    DrainOutcome::TimedOut { remaining } => {
        eprintln!("drain deadline reached with {remaining} admitted requests");
    }
}
```

Draining is irreversible. Readiness fails immediately, new synchronization work receives a
transient response, admitted work may finish, and liveness stays available to the supervisor.

Terminate TLS at a trusted proxy or configure TLS in the application. Never expose the binary
protocol over plaintext on an untrusted network.

---

## 11. Connect a production HTTP client

### 11.1 Supply bounded transport configuration

```rust
use aequora::http_client::{
    HttpTransport, StaticRequestHeaders,
};
use http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
use reqwest::Url;

let mut headers = HeaderMap::new();
headers.insert(
    AUTHORIZATION,
    HeaderValue::from_str(&format!("Bearer {access_token}"))?,
);

let base_url = Url::parse("https://api.example.com/")?;
let transport = HttpTransport::new(
    reqwest::Client::new(),
    &base_url,
    StaticRequestHeaders::new(headers),
    config.http_transport_config()?,
)?;
```

`StaticRequestHeaders` is convenient for short-lived tokens. For refreshable credentials,
implement `RequestHeaders`; its `headers` method runs before each request and can load the newest
credential from the application's secure session store.

### 11.2 Build the client engine

```rust
use aequora::{
    client::ClientSyncEngineBuilder,
    stoolap::StoolapStore,
};

let client = ClientSyncEngineBuilder::new()
    .store(StoolapStore::new(local_backend))
    .transport(transport)
    .config(config.client_config(session)?)
    .observer(metrics.clone())
    .build()?;
```

The type-state builder cannot build until it has both a `LocalStore` and a `SyncTransport`.

### 11.3 Choose `run_once` or `sync`

- `run_once()` performs one bounded exchange.
- `sync()` drains multiple batches and pull pages up to a safety ceiling.
- `bootstrap()` is selected by the engine when the server requires a snapshot/resynchronization.

For a manual “Sync now” button, call `sync`. For background work, use the coordinator described
next.

---

## 12. Run background synchronization

```rust
use aequora::client::{SyncCoordinator, SyncTrigger};
use std::sync::Arc;

let (coordinator, handle) = SyncCoordinator::new(
    Arc::new(client),
    config.coordinator_config()?,
);
let coordinator_task = tokio::spawn(coordinator.run());

// After a successful local mutation:
handle.trigger(SyncTrigger::LocalMutation).await?;

// Connectivity is a hint, never the source of truth:
handle.trigger(SyncTrigger::NetworkUnavailable).await?;
handle.trigger(SyncTrigger::NetworkAvailable).await?;

// During application shutdown:
handle.trigger(SyncTrigger::Shutdown).await?;
coordinator_task.await?;
```

Repeated local-mutation wakes are debounced and coalesced through a bounded channel. The periodic
timer is optional. A network-available event triggers a sync, but a failed request remains the
actual signal that transport is unavailable.

The handle exposes UI-neutral watches:

```rust
let mut status = handle.subscribe();
let mut health = handle.subscribe_health();

tokio::spawn(async move {
    while status.changed().await.is_ok() {
        println!("sync status: {:?}", *status.borrow());
    }
});

println!("pending operations: {}", health.borrow().pending_operations);
```

Do not bind the synchronization kernel directly to a particular GUI framework. Translate
`SyncStatus` and `SyncHealth` into your application's state-management system.

---

## 13. Understand retries, idempotency, and ACID

### 13.1 Lost response after commit

The most important retry scenario is:

```text
client sends operation 7
server commits operation 7
response is lost
client retries operation 7
server returns the stored result without applying it again
```

This works only if the same `OperationId` is retained. Creating a new ID during retry turns one
logical action into two authoritative actions.

### 13.2 Durable retry scheduling

On a transient failure, the client stores:

- outbox state `Retry`;
- a saturating attempt count;
- an absolute next-attempt Unix-millisecond deadline.

Stoolap's due-only pending query excludes future retry rows. Restarting the process does not reset
the backoff schedule or hide a row left in `Sending`; both `Sending` and `Retry` are replayable.

### 13.3 Server atomicity

A compliant authority commits these values together:

```text
entity payload and tombstone state
entity version
scope sequence
journal event
OperationId result
audit record
```

PostgreSQL also serializes operation and entity races with a fixed lock order. Only serialization
and deadlock SQLSTATEs (`40001`, `40P01`) trigger bounded retry of the complete transaction.

### 13.4 Client reconciliation atomicity

The client transaction applies:

```text
authoritative entity/tombstone changes
applied-event markers
acknowledged/rejected/conflict outbox states
conflict inbox records
cursor advancement last
```

If any step fails, the cursor does not advance and the response can be reconciled safely again.

See [`ACID.md`](ACID.md) for the architecture and
[`docs/acid-compliance.md`](docs/acid-compliance.md) for executable evidence.

---

## 14. Choose conflict semantics

Version comparison is strict:

- Creation requires `base_version: None` and no existing authoritative entity.
- Update/delete requires an exact current `EntityVersion`.
- An accepted transition advances exactly one version.

The safe default is `RejectConflicts`.

### 14.1 Register policies by operation type

```rust
use aequora::{
    conflict::{ConflictPolicyRegistry, TypedOperation},
    protocol::ConflictPolicy,
};

impl TypedOperation for MarkAttendance {
    const KIND: u16 = <Self as DomainOperation>::KIND;
}

let mut conflicts = ConflictPolicyRegistry::default();
conflicts.register::<MarkAttendance>(ConflictPolicy::ManualResolution);
```

Available semantics include reject, manual resolution, explicit deterministic merge strategies,
commutative operations, and opt-in last-writer-wins. Choose by domain meaning, not convenience.

### 14.2 Financial operations

Accounting, payments, inventory, and other value-bearing commands must not silently replace state.
Implement `FinancialOperation` and use `register_financial`; unsafe replacement policies are then
rejected during startup.

Prefer append-only domain commands such as `RecordPayment` or `PostAdjustment` over overwriting a
balance. Aequora does not prove that debits equal credits; your database transaction and domain
tests must prove that invariant.

### 14.3 Manual resolution

Manual conflicts are durable and UI-independent. The application may:

- accept the current server state with `ConflictResolution::AcceptServer`; or
- create a new explicit operation and resolve the old conflict with
  `ConflictResolution::SupersededBy(new_operation_id)`.

Never mutate a rejected/conflicted operation in place and resubmit it under the same ID. A new
intent needs a new `OperationId`.

### 14.4 CRDTs

CRDT merge is opt-in and appropriate only for algebraic state with tested merge laws:

```rust
use aequora::{crdt::{Crdt, PnCounter}, types::NodeId};

let left_node = NodeId::new();
let right_node = NodeId::new();
let mut left = PnCounter::default();
let mut right = PnCounter::default();

left.increment(left_node, 7);
right.decrement(right_node, 2);
left.merge(&right);
assert_eq!(left.value(), 5);
```

Deletion conflicts are not automatically resolved by the provided CRDT merger.

---

## 15. Bootstrap, cursors, scopes, and tombstones

### 15.1 Snapshot-first onboarding

A new client should install a consistent snapshot instead of replaying an unbounded history. The
authority captures a repeatable-read snapshot, pages it within entity/byte limits, and returns the
cursor from which incremental synchronization continues.

The local store stages pages durably and performs one atomic final install. An interrupted download
can resume; an incomplete snapshot never replaces the active local scope.

### 15.2 Cursor rules

A cursor contains a `SyncScopeId` and monotonically increasing `Sequence`.

- Never fabricate or advance a cursor outside reconciliation.
- A cursor before the retained journal floor receives a typed resync directive.
- A cursor ahead of the server or missing part of a sequence is rejected.
- An incompatible scope change receives a new scope identity and bootstrap.

Protocol v1 does not reset a sequence inside an existing scope. Compaction advances the retained
floor instead.

### 15.3 Partial scopes

Partition selectors are opaque application values. Authorize the complete requested scope before
reading any entity or journal data. Changing access may require a new scope and snapshot rather
than attempting to surgically retain data whose authorization has changed.

### 15.4 Tombstones

Deletes are authoritative tombstone transitions, not silent row disappearance. Retain tombstones
until every relevant active-device and snapshot watermark makes collection safe. Journal
compaction must not delete operation-ledger or audit evidence.

---

## 16. Synchronize large blobs separately

Do not place large files in normal operation batches. Embed a small `BlobRef` in domain state and
transfer content through a separate bounded blob capability.

```rust
use aequora::blob::{BlobDigest, BlobManifest, BlobStore, InMemoryBlobStore};

let bytes = b"content-addressed attachment";
let manifest = BlobManifest::for_bytes(bytes, 8)?;
let store = InMemoryBlobStore::new(8, 1024)?;

for chunk in bytes.chunks(8) {
    store
        .put_chunk(BlobDigest::of(chunk), chunk.to_vec())
        .await?;
}

assert_eq!(store.get(manifest.blob).await?, None);
store.commit(&manifest).await?;
assert_eq!(store.get(manifest.blob).await?, Some(bytes.to_vec()));
```

Chunks are verified with BLAKE3 and remain unpublished until the complete manifest commits. A
production blob adapter should preserve the same “incomplete uploads are invisible” rule.

Recommended application flow:

1. Build a manifest.
2. Ask which chunks are missing.
3. Upload missing chunks idempotently.
4. Commit the complete manifest.
5. Submit a small domain operation referencing the published `BlobRef`.

---

## 17. Add metrics and tracing

`AtomicMetrics` is a payload-free observer suitable for health/status export:

```rust
use aequora::observability::AtomicMetrics;

let metrics = Arc::new(AtomicMetrics::default());
let client = client.with_observer(metrics.clone());
let snapshot = metrics.snapshot();

println!("outbox pending: {}", snapshot.outbox_pending);
println!("server commits: {}", snapshot.transaction_commits);
```

The observer model covers:

- client exchange outcomes and retry delays;
- outbox depth, oldest pending age, conflict count, and last success;
- exact transport bytes;
- validation, execution, and database timings;
- server transaction commit/rollback/failure/dedup outcomes;
- readiness, overload, rate-limit, ingestion-timeout, and drain lifecycle events;
- journal lag.

Enable the `tracing` feature to use `TracingObserver`. Keep tenant IDs, operation payloads,
credentials, and domain data out of metric labels and logs. Correlate with bounded request/trace
identifiers rather than sensitive business values.

Operational alerts should include:

- continuously growing outbox depth;
- oldest pending operation beyond the product's offline SLA;
- no successful sync while the device is active;
- unresolved conflict growth;
- repeated readiness failures;
- sustained server overload or tenant rate limiting;
- journal lag approaching retention limits;
- transaction rollback/failure spikes.

---

## 18. Implement a custom database adapter

Aequora's protocol is database-neutral. SQLite, Redb, a document store, or another database is
usable only after you implement and verify the required behavioral capabilities. Mentioning a
database in configuration is not enough.

### 18.1 Local adapter capabilities

Implement the capabilities combined by `LocalStore`:

- `OutboxStore` and `OutboxStateStore`;
- `CursorStore`;
- `ReconciliationStore`;
- `ConflictInbox`;
- snapshot staging/install capabilities required by bootstrap.

Also implement `TransactionCapabilityProvider` honestly. A volatile test double must not claim
full durability.

Your native adapter must prove two database-specific transactions that a generic trait cannot
create for you:

1. application optimistic mutation plus outbox insertion;
2. authoritative reconciliation plus cursor advancement.

### 18.2 Authority adapter capabilities

Implement the capabilities combined by `AuthoritativeStore`:

- `EntityReader`;
- `OperationLedger`;
- `ChangeJournal`;
- `SnapshotStore`;
- `AuditLog`.

`OperationLedger::commit_operation` is the critical atomic boundary. The adapter must commit the
entity transition, exact next version, journal sequence/event, stable operation result, and audit
record together.

### 18.3 Run the public compliance contracts

Add `aequora-testkit` as a development dependency:

```rust,ignore
use aequora_testkit::contracts::{
    verify_authoritative_store,
    verify_local_store,
};

verify_local_store(&local_store, operation, scope, server_time).await?;
verify_authoritative_store(&authority_store, initial_commit).await?;
```

The contracts exercise replayable outbox states, durable retry scheduling, idempotent
reconciliation, cursor durability, invalid version transitions, duplicate delivery, concurrent
creation races, one journal/audit effect, operation-ledger replay, and snapshot consistency.

Then add real-engine tests for rollback, restart, migration drift, and concurrent connections. An
in-memory contract pass proves semantics, not physical durability.

See [`docs/custom-database-adapters.md`](docs/custom-database-adapters.md) for the concise contract
reference.

---

## 19. Use QUIC or a custom transport

### 19.1 QUIC

The optional QUIC integration wraps an application-owned, already authenticated Quinn connection:

```rust
use aequora::quic::{QuicConfig, QuicServer, QuicTransport};

let client_transport = QuicTransport::new(client_connection, QuicConfig::default());
let server = QuicServer::new(service, QuicConfig::default());
server.serve_connection(server_connection, auth_context).await?;
```

The host owns certificates, trust roots, endpoint construction, connection authentication, and
connection lifecycle. Do not pass an `AuthContext` until the connection identity has been verified.

QUIC provides bidirectional request streams, streaming bootstrap pages, and advisory push hints.
Push hints never mutate state and never replace polling/retry correctness.

### 19.2 Custom transport

Implement `SyncTransport` when you need another network boundary:

```rust,ignore
#[async_trait]
impl SyncTransport for MyTransport {
    async fn exchange(&self, request: SyncRequest) -> Result<SyncResponse, TransportError> {
        // Authenticate, encode, bound, send, receive, bound, decode, and classify errors.
    }

    async fn bootstrap(
        &self,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, TransportError> {
        // Preserve the same protocol and resource-limit rules.
    }
}
```

Classify only retry-safe failures as transient. Bound both compressed wire bytes and decompressed
payload bytes. The transport must not choose a database or weaken authenticated tenant/scope
checks.

---

## 20. Test correctness and failure recovery

### 20.1 Fast local gates

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --all-features --no-deps --locked
git diff --check
```

### 20.2 Architecture and database-neutrality gates

```bash
cargo run -q -p aequora-dev --locked -- check
bash scripts/check-database-neutrality.sh
```

The first command uses Guppy to enforce dependency direction across the workspace. The second
compiles four profiles: custom/custom, Stoolap/custom, custom/PostgreSQL, and
Stoolap/PostgreSQL.

### 20.3 Adapter contracts

```bash
cargo test -p aequora-testkit --test adapter_contracts --locked
cargo test -p aequora-store-stoolap --all-features --locked
```

### 20.4 Live PostgreSQL and Neon

```bash
export AEQUORA_TEST_POSTGRES_URL='postgres://user:password@localhost:5432/aequora'
cargo test -p aequora-store-postgres --test postgres_live --locked
cargo test -p aequora --test database_neutrality_live --all-features --locked
```

For Neon, set both variables:

```bash
export AEQUORA_TEST_NEON_POOLED_URL='postgresql://...-pooler...'
export AEQUORA_TEST_NEON_DIRECT_URL='postgresql://...direct...'
```

These tests safely skip when their environment variables are absent. A green skipped test is not
evidence of a current live-database run; CI/release reporting must state whether URLs were set.

### 20.5 Fuzz, model, property, and benchmark gates

```bash
cargo check --manifest-path fuzz/Cargo.toml --bins --locked
cargo test -p aequora-testkit --test model_based --locked
cargo test -p aequora-testkit --test property_invariants --locked
cargo bench -p aequora-testkit --bench core_pipeline --no-run --locked
```

Important application-specific tests include:

- crash between every local and server transaction phase;
- response loss after server commit;
- two concurrent clients updating one entity version;
- duplicate delivery of one `OperationId`;
- client restart from `Sending` and future-dated `Retry`;
- cursor regression and incomplete sequence rejection;
- snapshot interruption and final-install rollback;
- migration checksum/name drift;
- tombstone replay and compaction safety;
- scope authorization before any data disclosure;
- unsafe financial conflict policy rejection;
- body/decompression limits and slow-client deadlines.

---

## 21. Deploy and operate safely

### 21.1 Migration sequence

For PostgreSQL/Neon:

1. Back up the target database and verify the backup is usable.
2. Use the direct/admin endpoint for migrations.
3. Start the new server revision against the migrated schema.
4. Wait for `/sync/v1/health/ready`.
5. Shift traffic.
6. Drain the old revision.
7. Monitor transaction errors, queue depth, conflict rate, and journal lag.

For clients, open Stoolap early during startup. Opening applies idempotent local migrations and
verifies the checksummed ledger. Never delete pending outbox rows during an ordinary migration.

### 21.2 Rolling compatibility

- Keep the server's minimum/current protocol window compatible with deployed clients.
- Add enum variants and capabilities append-only.
- Never reuse operation kinds.
- Ship payload migrators before commands using the new schema reach the server.
- Reject unsupported schemas before starting an authoritative transaction.
- Force bootstrap when a retained journal floor makes incremental replay impossible.

### 21.3 Capacity planning

Measure, do not guess:

- maximum encoded operation and response sizes;
- operations per exchange and exchanges per sync;
- snapshot page count/bytes;
- per-tenant and global concurrency;
- PostgreSQL connection usage across replicas;
- transaction latency and retry rate;
- outbox growth during expected offline periods;
- journal/tombstone retention relative to device activity;
- CPU pool saturation and memory at configured limits.

### 21.4 Backup and recovery

Repository tests prove transactional behavior, but production acceptance must separately prove:

- scheduled backup creation;
- encrypted storage and access control;
- restoration into an isolated environment;
- migration-ledger and schema verification after restore;
- application readiness and a real synchronization exchange after restore;
- documented recovery point and recovery time objectives.

Do not report backup compliance merely because PostgreSQL transactions pass.

### 21.5 Security checklist

- Require TLS and verify hostnames/certificates.
- Derive `AuthContext` from verified credentials.
- Authorize tenant, device, actor, scope, and partitions.
- Keep secrets outside `AequoraConfig` and logs.
- Bound request, response, decompressed, operation, dependency, partition, and snapshot sizes.
- Apply pre-body global/per-tenant admission and rate limits.
- Use parameterized database statements only.
- Keep network/external I/O outside database transactions.
- Avoid operation payloads in metrics, traces, and error messages.
- Preserve immutable audit evidence independently from the compactable sync journal.

---

## 22. Troubleshoot common failures

### “The operation appears locally but never reaches the server”

Check `SyncHealth.pending_operations`, oldest pending age, the coordinator's connectivity state, and
retry metadata. Confirm the local mutation used `transact_local_mutation`; otherwise the entity may
exist without an outbox row.

### “The server applies the same action twice”

Confirm every retry preserves the original `OperationId`. Verify the authority adapter's unique
tenant/operation ledger constraint and run `verify_authoritative_store` concurrently.

### “The client repeatedly bootstraps”

Check whether the stored cursor is below the journal retained floor, belongs to another scope, or
was advanced outside atomic reconciliation. Verify snapshot final install commits the cursor with
the installed scope.

### “Updates always conflict”

Ensure update envelopes use the last reconciled `EntityVersion`, not `None`, a local provisional
counter, or a wall-clock timestamp. Confirm successful authoritative changes replace provisional
state locally.

### “HTTP returns 401/500 because AuthContext is missing”

Install authentication middleware before the Aequora router and insert a verified `AuthContext`
into request extensions. Do not fix this by trusting client-supplied identity.

### “HTTP returns 408, 429, 503, or 504”

These are transient. Inspect body-read timeouts, per-tenant rate limits, global/per-tenant in-flight
limits, readiness, request deadlines, and `Retry-After`. The client will retain the operation and
persist a retry deadline.

### “HTTP returns 413”

The framed request exceeded the configured compressed body limit. Reduce operation/batch size or
raise the limit only after reviewing memory and decompression limits. Large files belong in the
blob path.

### “A migration is rejected after a code change”

Published migration version/name/checksum history is immutable. Add a new migration; never rewrite
an applied migration. Investigate unexpected database history before serving traffic.

### “Tests pass without contacting PostgreSQL or Neon”

The live suites skip when their URL variables are absent. Set the variables and inspect the test
environment before claiming live validation.

### “A custom adapter passes unit tests but loses data after restart”

In-memory contracts do not prove physical durability. Add file/service-backed restart, rollback,
concurrency, interrupted migration, and snapshot-install tests against the real engine.

---

## 23. Reusable project checklist

### Domain design

- [ ] Every mutation is a typed intent, not raw SQL.
- [ ] Every operation kind is stable and unique.
- [ ] Payload schema versions and migrators are explicit.
- [ ] Authorization is separate from decoding and business validation.
- [ ] Financial/value-bearing operations use safe conflict semantics.
- [ ] External side effects use an application-owned transactional outbox.

### Client

- [ ] Optimistic state and outbox append share one native transaction.
- [ ] Stable operation IDs survive every retry.
- [ ] Retry deadlines and attempts survive restart.
- [ ] Reconciliation and cursor advancement share one transaction.
- [ ] Snapshot staging is resumable and final installation is atomic.
- [ ] UI consumes `SyncStatus`/`SyncHealth` instead of database internals.

### Server

- [ ] Authentication derives `AuthContext` from verified credentials.
- [ ] Scope authorization happens before data access.
- [ ] Validation happens before authoritative mutation.
- [ ] Entity/version/journal/ledger/audit commit atomically.
- [ ] Duplicate operations replay the stored result.
- [ ] Transactions contain no network or external I/O.
- [ ] Serialization/deadlock retry is bounded and retries the whole transaction.

### Transport and security

- [ ] TLS is required outside trusted loopback/test environments.
- [ ] Compressed and decompressed sizes are independently bounded.
- [ ] Batch count and encoded bytes are bounded.
- [ ] Admission, rate limiting, body-read, execution, and readiness deadlines are configured.
- [ ] Transient/permanent errors preserve retry safety.
- [ ] Logs and metrics contain no payloads or credentials.

### Verification and operations

- [ ] Shared local and authority adapter contracts pass.
- [ ] Real-engine rollback, restart, migration, and concurrency tests pass.
- [ ] Guppy and database-neutrality policy gates pass.
- [ ] Live PostgreSQL/Neon runs are distinguished from skipped tests.
- [ ] Backup restoration is exercised, not merely configured.
- [ ] Rolling protocol/schema compatibility is tested.
- [ ] Readiness, graceful drain, capacity, and retention are monitored.

---

## 24. Where to go next

Use these repository resources as deeper references:

- [`plan.md`](plan.md): governing database-neutral architecture and implementation direction.
- [`next.md`](next.md): detailed synchronization architecture and protocol semantics.
- [`ACID.md`](ACID.md): transaction, isolation, idempotency, and recovery model.
- [`docs/next-completion.md`](docs/next-completion.md): architecture-to-code implementation map.
- [`docs/acid-compliance.md`](docs/acid-compliance.md): ACID requirements mapped to contracts/tests.
- [`docs/custom-database-adapters.md`](docs/custom-database-adapters.md): adapter capability guide.
- [`crates/aequora/examples/in_process.rs`](crates/aequora/examples/in_process.rs): smallest runnable flow.
- [`crates/aequora/examples/school_erp.rs`](crates/aequora/examples/school_erp.rs): typed offline ERP flow.

The safest implementation order for a new project is:

1. Model one typed operation and its authorization/business rules.
2. Prove it with the in-process testkit.
3. Add one real local transaction and durable outbox.
4. Add the real authority transaction and compliance tests.
5. Add HTTP/Axum authentication and bounded transport.
6. Add restart, response-loss, concurrency, conflict, and bootstrap tests.
7. Add metrics, readiness, graceful drain, backup restoration, and capacity acceptance.
8. Expand to more operations only after the first vertical slice is correct.

That sequence keeps the hardest property visible: after retries, crashes, concurrent edits,
offline periods, and upgrades, the system still converges to a correct and explainable state.
