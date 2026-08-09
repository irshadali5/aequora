# Aequora Synchronization Tutorial

This comprehensive, step-by-step tutorial guides software engineers through building local-first, server-authoritative synchronization systems in Rust using **Aequora**.

---

## Table of Contents

1. [Understanding Local-First Synchronization](#1-understanding-local-first-synchronization)
2. [Cargo Workspace & Feature Flags](#2-cargo-workspace--feature-flags)
3. [Defining Domain Operations](#3-defining-domain-operations)
4. [Building the Client Storage & Outbox](#4-building-the-client-storage--outbox)
5. [Configuring the Client Sync Engine](#5-configuring-the-client-sync-engine)
6. [Building the Authoritative Server](#6-building-the-authoritative-server)
7. [Axum HTTP Sync Endpoint Integration](#7-axum-http-sync-endpoint-integration)
8. [Advanced Topics: CRDTs, Blob Storage, & QUIC Transport](#8-advanced-topics-crdts-blob-storage--quic-transport)
9. [Testing with Aequora Testkit](#9-testing-with-aequora-testkit)

---

## 1. Understanding Local-First Synchronization

Traditional web applications rely on continuous network access: every UI interaction fires an HTTP request to a central database. When the network drops, the application stops functioning.

**Aequora** implements a **local-first, server-authoritative** model:
- **Offline Operations**: User actions execute immediately against a local embedded database (e.g., Stoolap) and are transactionally staged to a local outbox journal.
- **Background Synchronization**: When connected, the client streams Postcard-encoded envelopes to the server over HTTP, Axum, or QUIC.
- **Server Authority**: The server validates domain constraints, enforces tenant authorization, executes operations against authoritative storage (e.g., PostgreSQL / Neon), and returns acknowledged offsets along with downstream updates.
- **Atomic Local Reconciliation**: The client applies downstream changes, resolves conflicts deterministically, and advances local cursors in a single local transaction.

```text
┌───────────────────────────┐                 ┌───────────────────────────┐
│     Client Environment    │                 │     Server Environment    │
│                           │  AEQ1 Postcard  │                           │
│  Stoolap MVCC Local DB    │ ──────────────► │  Axum Gateway & Exchange  │
│  + Transactional Outbox   │ ◄────────────── │  + PostgreSQL Authority   │
└───────────────────────────┘    HTTP / QUIC  └───────────────────────────┘
```

---

## 2. Cargo Workspace & Feature Flags

To start using Aequora in your project, add the `aequora` crate to your `Cargo.toml`. Select features based on your target deployment:

```toml
[dependencies]
# Top-level facade crate with desired integrations
aequora = { version = "0.1", features = ["stoolap", "postgres", "axum", "http-client"] }

# Async runtime & serialization dependencies
tokio = { version = "1.45", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
postcard = { version = "1.1", features = ["use-std"] }
async-trait = "0.1"
```

### Feature Selection Matrix

| Deployment Profile | Recommended Feature Set | Description |
|---|---|---|
| Client App (Embedded DB + HTTP) | `stoolap,http-client` | Uses Stoolap local storage & Reqwest HTTP transport |
| Server API (PostgreSQL + Axum) | `postgres,axum` | Uses PostgreSQL authoritative storage & Axum HTTP endpoint |
| Full Workspace / Testing | `stoolap,postgres,axum,http-client,testkit` | Complete feature matrix for integration testing |

---

## 3. Defining Domain Operations

In Aequora, mutations are represented as strongly typed Rust data structures that implement `DomainOperation`.

### Step 1: Define the Command Struct

```rust
use aequora::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateTaskCommand {
    pub task_id: EntityId,
    pub title: String,
    pub assignee_id: ActorId,
    pub priority: u8,
}

impl DomainOperation for CreateTaskCommand {
    const KIND: u16 = 200;            // Unique operation kind identifier
    const CURRENT_SCHEMA: u16 = 1;     // Domain schema version
}
```

### Step 2: Implement the Operation Handler

An `OperationHandler` contains the server-side business logic and authorization checks:

```rust
use async_trait::async_trait;

pub struct TaskCommandHandler;

#[async_trait]
impl OperationHandler<CreateTaskCommand> for TaskCommandHandler {
    async fn authorize(
        &self,
        auth: &AuthContext,
        command: &CreateTaskCommand,
        _envelope: &OperationEnvelope,
    ) -> Result<(), ExecutionError> {
        // Enforce authorization constraints (e.g., actor must match creator)
        if auth.tenant_id.is_nil() {
            return Err(ExecutionError::unauthorized("Tenant ID required"));
        }
        Ok(())
    }

    async fn execute(
        &self,
        _auth: &AuthContext,
        command: &CreateTaskCommand,
        _envelope: &OperationEnvelope,
        _current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        // Validate domain rules
        if command.title.trim().is_empty() {
            return Err(ExecutionError::business_rule("Task title cannot be empty"));
        }

        // Serialize payload for storage
        let payload = postcard::to_stdvec(command)
            .map_err(|_| ExecutionError::invalid_operation("Failed to encode command payload"))?;

        Ok(AuthoritativeMutation {
            payload,
            change_kind: ChangeKind::Upsert,
        })
    }
}
```

### Step 3: Register Handlers in the Registry

```rust
pub fn create_operation_registry() -> Result<OperationRegistry, RegistrationError> {
    let mut registry = OperationRegistry::new();
    registry.register_handler(TaskCommandHandler)?;
    Ok(registry)
}
```

---

## 4. Building the Client Storage & Outbox

Client storage manages local entities and stages local changes into an outbox.

### Using Stoolap for Local Storage

```rust
use aequora::stoolap::{StoolapDatabase, StoolapStore};
use aequora::prelude::*;
use std::sync::Arc;

pub async fn setup_client_storage(db_path: &str) -> Result<StoolapDatabase, Box<dyn std::error::Error>> {
    // Open local Stoolap database (or memory store with ":memory:")
    let database = StoolapDatabase::open(db_path)?;
    
    // Ensure local schema migrations are installed
    database.run_migrations().await?;
    
    Ok(database)
}
```

### Executing Local Mutations with Atomic Outbox Staging

Use `transact_local_mutation` to guarantee that your local state mutation and outbox append share a single MVCC database transaction:

```rust
pub async fn submit_local_task(
    db: &StoolapDatabase,
    tenant_id: TenantId,
    actor_id: ActorId,
    device_id: DeviceId,
    task: CreateTaskCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let entity_ref = EntityRef {
        entity_type: EntityType::new(1)?,
        entity_id: task.task_id,
    };

    let payload = postcard::to_stdvec(&task)?;

    let envelope = OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id,
        actor_id,
        device_id,
        entity: entity_ref,
        base_version: None,
        created_at: HybridTimestamp::now(NodeId::new()),
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(CreateTaskCommand::KIND),
        payload,
        metadata: OperationMetadata::default(),
    };

    // Atomic local transaction: update local entities table AND insert into outbox
    db.transact_local_mutation(|tx| {
        // 1. Insert or update local entity representation
        tx.execute(
            "INSERT INTO aequora_local_entities (scope_id, entity_type, entity_id, payload, provisional) VALUES ($1, $2, $3, $4, 1)",
            (tenant_id.to_string(), 1i64, task.task_id.to_string(), envelope.payload.clone()),
        )?;

        // 2. Append envelope into outbox
        tx.append_outbox(&envelope)?;
        Ok(())
    })?;

    Ok(())
}
```

---

## 5. Configuring the Client Sync Engine

The `ClientSyncEngine` handles outbox draining, snapshot bootstrap, resynchronization, and exponential retry backoff.

```rust
use aequora::prelude::*;
use std::sync::Arc;

pub async fn run_client_sync_loop<T: SyncTransport>(
    local_store: StoolapStore,
    transport: T,
    session: SessionMetadata,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Construct Engine Builder
    let client = ClientSyncEngineBuilder::new()
        .store(local_store)
        .transport(transport)
        .config(ClientConfig::new(session))
        .build()?;

    // 2. Wrap Engine in Background Coordinator
    let coordinator_config = SyncCoordinatorConfig {
        interval_secs: 15,
        sync_on_start: true,
    };

    let (mut handle, coordinator_task) = SyncCoordinator::spawn(client, coordinator_config);

    // Spawn task loop
    tokio::spawn(coordinator_task);

    // Explicitly trigger a manual sync check
    handle.trigger_sync(SyncTrigger::Manual).await?;

    Ok(())
}
```

---

## 6. Building the Authoritative Server

The authoritative server coordinates client sync exchanges, authorizes sessions, and persists authoritative updates into PostgreSQL or Neon.

### Setting Up PostgreSQL Authoritative Storage

```rust
use aequora::postgres::SqlxPostgresBackend;
use aequora::prelude::*;
use std::sync::Arc;

pub async fn setup_authoritative_server(
    database_url: &str,
    registry: OperationRegistry,
) -> Result<SyncServer<SqlxPostgresBackend>, Box<dyn std::error::Error>> {
    // Connect to PostgreSQL database backend
    let backend = SqlxPostgresBackend::connect(database_url, 10).await?;
    
    // Verify migration status
    backend.run_migrations().await?;
    assert!(backend.schema_status().await?.is_current());

    // Build Sync Server instance
    let server = SyncServerBuilder::new()
        .store(Arc::new(backend))
        .executor(Arc::new(registry))
        .conflict_resolver(Arc::new(RejectConflicts))
        .clock(Arc::new(SystemClock::new(NodeId::new())))
        .build()?;

    Ok(server)
}
```

---

## 7. Axum HTTP Sync Endpoint Integration

Expose your authoritative server over HTTP using Axum and `aequora-axum`:

```rust
use aequora::axum::axum_sync_router;
use aequora::prelude::*;
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;

pub async fn start_http_gateway<S>(
    server: SyncServer<S>,
    bind_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: AuthoritativeStore + 'static,
{
    let exchange_service: Arc<dyn ExchangeService> = Arc::new(server);

    // Construct Axum application router with POST /sync/v1/exchange route
    let app: Router = Router::new().nest("/sync/v1", axum_sync_router(exchange_service));

    println!("Aequora HTTP Gateway listening on {}", bind_addr);
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

---

## 8. Advanced Topics: CRDTs, Blob Storage, & QUIC Transport

### 8.1 State-Based CRDT Counters

Aequora supports CRDT counters like `PnCounter` for concurrent increments/decrements across offline nodes:

```rust
use aequora::crdt::PnCounter;
use aequora::types::NodeId;

let node1 = NodeId::new();
let node2 = NodeId::new();

let mut counter_a = PnCounter::default();
counter_a.increment(node1, 5);

let mut counter_b = PnCounter::default();
counter_b.decrement(node2, 2);

// Merge CRDT state deterministically
counter_a.merge(&counter_b);
assert_eq!(counter_a.value(), 3);
```

### 8.2 BLAKE3 Content-Addressed Blob Storage

For large media or file attachments, use BLAKE3 content-addressed blob references (`BlobStore`):

```rust
use aequora::blob::{BlobStore, InMemoryBlobStore};

let blob_store = InMemoryBlobStore::default();
let data = b"Large attachment binary payload data";

// Store blob and derive immutable hash reference
let blob_ref = blob_store.put_bytes(data).await?;
println!("Stored Blob Hash: {}", blob_ref.digest.to_hex());

// Read back blob
let retrieved = blob_store.get_bytes(&blob_ref.digest).await?;
assert_eq!(retrieved.as_ref(), data);
```

### 8.3 QUIC Transport Setup

For ultra-low latency UDP streaming, use `aequora-quic`:

```rust,no_run
use aequora::quic::QuicClientTransport;

# async fn connect_quic() -> Result<(), Box<dyn std::error::Error>> {
let transport = QuicClientTransport::connect("127.0.0.1:4433", "localhost").await?;
# Ok(())
# }
```

---

## 9. Testing with Aequora Testkit

Aequora provides `aequora-testkit` for fast, deterministic unit and integration testing without needing external database daemons or network sockets:

```rust
#[cfg(test)]
mod tests {
    use aequora::prelude::*;
    use aequora::testkit::{
        AllowAllExecutor, InMemoryAuthoritativeStore, InMemoryLocalStore, InProcessTransport,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn test_end_to_end_sync_flow() -> Result<(), Box<dyn std::error::Error>> {
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

        let auth = AuthContext { actor_id, tenant_id, device_id };

        // Setup in-memory server & local store
        let auth_store = Arc::new(InMemoryAuthoritativeStore::default());
        let server: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
            auth_store,
            Arc::new(AllowAllExecutor),
            Arc::new(RejectConflicts),
            Arc::new(SystemClock::new(NodeId::new())),
        ));

        let local_store = InMemoryLocalStore::default();
        let transport = InProcessTransport::new(server, auth);

        let engine = ClientSyncEngine::new(local_store, transport, ClientConfig::new(session));

        // Execute sync cycle
        let outcome = engine.run_once().await?;
        assert_eq!(outcome.acknowledged, 0);

        Ok(())
    }
}
```

---

## Summary

With Aequora, you get:
- **Complete Database Neutrality**: Choose Stoolap, SQLite, or Redb locally; PostgreSQL or Neon authoritatively.
- **Safety & Soundness**: Atomic transactions, framed binary protocols with BLAKE3 checksums, and strict zero-allocation security limits.
- **High Concurrency**: Hybrid Logical Clocks, optimistic concurrency control, and Rayon CPU offloading.

For complete API documentation, refer to [`README.md`](README.md) and the codebase inline documentation.
