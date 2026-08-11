# Aequora Sync — Next Architecture

## Implementation Architecture, Protocol Semantics, Execution Model, Storage Contracts, Reliability, Security, and Production Plan

> This document continues the high-level **Aequora Sync** architecture.
>
> The previous architecture established the core principle:
>
> **Synchronize typed domain operations and authoritative state transitions, not database rows or database-specific replication logs.**
>
> This document turns that principle into an implementable production system.

---

# 1. Purpose of This Document

This document defines the next layer of Aequora Sync:

- concrete crate boundaries;
- internal data structures;
- wire protocol;
- client synchronization state machine;
- server synchronization state machine;
- operation lifecycle;
- transactional guarantees;
- Axum validation/execution architecture;
- store adapter interfaces;
- Stoolap client implementation model;
- PostgreSQL/Neon server implementation model;
- dependency planning;
- conflict handling;
- snapshots and bootstrap;
- partial synchronization;
- schema evolution;
- security;
- concurrency;
- backpressure;
- Rayon integration;
- observability;
- test architecture;
- failure recovery;
- deployment model;
- implementation phases.

The intended stack for the first production deployment is:

```text
Client UI            Dioxus
Client DB            Stoolap
Sync library         Aequora Sync
Wire format          Postcard
Configuration        RON
HTTP server          Axum
Async runtime        Tokio
CPU parallelism      Rayon
Server repository    SQLx adapter
Authoritative DB     PostgreSQL / Neon
```

However, none of those database choices are embedded into `aequora-core`.

---

# 2. Fundamental System Invariants

Everything in the implementation must protect the following invariants.

## I-1 — Atomic Local Intent

A local mutation that must synchronize is committed atomically with its outbox entry.

```text
local state mutation
+
sync operation append
=
one local transaction
```

---

## I-2 — Atomic Authoritative Publication

An accepted server-side mutation is committed atomically with:

- authoritative state;
- authoritative journal event;
- entity version;
- operation deduplication record.

---

## I-3 — Logical Exactly-Once Effects

Transport is assumed to be at-least-once.

A repeated `OperationId` must never produce the business side effect twice.

---

## I-4 — Durable Cursor Advancement

A client cursor advances only after all events through that cursor are durably applied locally.

---

## I-5 — Monotonic Entity Versions

Every authoritative entity or aggregate version only moves forward.

---

## I-6 — Server Authority

The client may propose state changes.

Only the server determines whether a change is authoritative.

---

## I-7 — Database Independence

No SQL statement, table layout, WAL record, PostgreSQL transaction object, or Stoolap-specific type appears in the protocol crate.

---

## I-8 — Deterministic Retry

Retrying the same request or operation must produce an equivalent logical outcome.

---

## I-9 — Explicit Conflict Semantics

Conflicts are never silently hidden behind a universal "last write wins" rule.

---

## I-10 — Bounded Resource Consumption

All externally supplied batch sizes, payload sizes, strings, dependencies, and snapshot chunks are bounded.

---

# 3. Workspace Structure

Recommended workspace:

```text
aequora/
├── Cargo.toml
├── README.md
├── LICENSE
├── SECURITY.md
├── CHANGELOG.md
├── deny.toml
├── rustfmt.toml
├── clippy.toml
├── benches/
├── examples/
├── fuzz/
├── tests/
└── crates/
    ├── aequora/
    ├── aequora-core/
    ├── aequora-types/
    ├── aequora-protocol/
    ├── aequora-codec/
    ├── aequora-clock/
    ├── aequora-store/
    ├── aequora-journal/
    ├── aequora-client/
    ├── aequora-server/
    ├── aequora-validator/
    ├── aequora-executor/
    ├── aequora-conflict/
    ├── aequora-transport/
    ├── aequora-transport-http/
    ├── aequora-axum/
    ├── aequora-compute/
    ├── aequora-observability/
    ├── aequora-testkit/
    ├── aequora-store-stoolap/
    └── aequora-store-postgres/
```

---

# 4. Dependency Rule

The dependency graph must remain acyclic.

```text
aequora-types
      │
      ▼
aequora-core
      │
 ┌────┼──────────┐
 ▼    ▼          ▼
protocol store   clock
 │      │
 ▼      ▼
codec   journal
 │
 ├──────────────┐
 ▼              ▼
client         server
 │              │
 ▼              ▼
transport      validator
               executor
               conflict

outer integrations:
    axum
    stoolap
    postgres
    tracing
```

The following must never happen:

```text
aequora-core -> axum
aequora-core -> sqlx
aequora-core -> stoolap
aequora-core -> dioxus
```

---

# 5. Public Crate vs Internal Crates

`aequora` should be the ergonomic facade crate.

Applications should normally depend on:

```toml
aequora = { version = "...", features = ["client", "postcard"] }
```

or:

```toml
aequora = { version = "...", features = ["server", "axum", "postgres"] }
```

The facade can re-export stable public types.

Internal implementation details should remain in sub-crates.

---

# 6. Core Type System

Aequora should aggressively use newtypes.

Example:

```rust
pub struct OperationId(Uuid);
pub struct EntityId(Uuid);
pub struct DeviceId(Uuid);
pub struct ActorId(Uuid);
pub struct TenantId(Uuid);
pub struct SessionId(Uuid);
pub struct ScopeId(Uuid);
pub struct SnapshotId(Uuid);

pub struct EntityVersion(u64);
pub struct Sequence(u64);
pub struct SchemaVersion(u32);
pub struct ProtocolVersion(u16);
```

Avoid passing primitive types directly through the core API.

---

# 7. Domain Type Separation

The synchronization library should distinguish four layers of types.

```text
Wire type
    ↓ validation
Protocol type
    ↓ domain decoding
Application command
    ↓ authorization / validation
Executable command
```

For example:

```rust
WireOperation
```

must not be executable.

Only:

```rust
ExecutableOperation
```

is accepted by the executor.

---

# 8. Operation Lifecycle

An operation passes through:

```text
CreatedLocally
      ↓
Queued
      ↓
Batched
      ↓
Sent
      ↓
ReceivedByServer
      ↓
Authenticated
      ↓
Authorized
      ↓
Validated
      ↓
ConflictChecked
      ↓
Planned
      ↓
Executed
      ↓
Committed
      ↓
Acknowledged
      ↓
ReconciledLocally
      ↓
Completed
```

Failure paths:

```text
ValidationRejected
AuthorizationRejected
Conflict
RetryableFailure
PermanentFailure
Superseded
```

---

# 9. Operation Envelope

Recommended core envelope:

```rust
pub struct OperationEnvelope {
    pub operation_id: OperationId,
    pub tenant_id: TenantId,
    pub actor_id: ActorId,
    pub device_id: DeviceId,

    pub entity: EntityRef,
    pub base_version: Option<EntityVersion>,

    pub operation_kind: OperationKind,
    pub schema_version: SchemaVersion,

    pub dependencies: SmallVec<[OperationId; 4]>,

    pub client_time: HybridTimestamp,

    pub payload: Bytes,
}
```

The payload is encoded application data.

The generic core does not know its meaning.

---

# 10. Why Payload Should Be Opaque in Core

Suppose Project A defines:

```rust
enum ErpOperation { ... }
```

Project B defines:

```rust
enum MessengerOperation { ... }
```

Project C defines:

```rust
enum FinanceOperation { ... }
```

Aequora should not need a new release to support each enum.

Therefore the protocol core carries:

```text
operation kind
schema version
opaque payload bytes
```

The application registry is responsible for decoding the payload.

---

# 11. Operation Registry

Server:

```rust
pub trait OperationHandler: Send + Sync {
    fn kind(&self) -> OperationKind;

    fn decode(
        &self,
        payload: &[u8],
        schema: SchemaVersion,
    ) -> Result<Box<dyn DomainOperation>, DecodeError>;

    async fn validate(...);

    async fn execute(...);
}
```

A registry maps:

```text
OperationKind -> OperationHandler
```

The registry should be immutable after server startup.

---

# 12. Static Registration Preferred

Prefer application setup like:

```rust
let registry = OperationRegistry::builder()
    .register(CreateStudentHandler::new(...))
    .register(UpdateStudentHandler::new(...))
    .register(PostPaymentHandler::new(...))
    .build()?;
```

Do not perform dynamic plugin discovery on every request.

---

# 13. Operation Kind Namespace

Use numeric identifiers.

Example:

```text
0x0001_0001 -> CreateStudent
0x0001_0002 -> UpdateStudent
0x0002_0001 -> MarkAttendance
0x0003_0001 -> CreateInvoice
0x0003_0002 -> PostPayment
```

A useful convention is:

```text
high 16 bits = domain/module
low 16 bits  = operation
```

This permits stable allocation without large strings on the wire.

---

# 14. Wire Message Envelope

Every request should contain a protocol envelope.

```rust
pub struct WireEnvelope<T> {
    pub magic: ProtocolMagic,
    pub protocol: ProtocolVersion,
    pub message_id: MessageId,
    pub flags: ProtocolFlags,
    pub body: T,
}
```

For HTTP, some framing metadata is redundant, but an internal envelope still provides protocol validation and portability to future transports.

---

# 15. Primary Messages

Initial protocol should require only a small set.

```rust
enum ClientMessage {
    Exchange(SyncRequest),
    Bootstrap(BootstrapRequest),
}

enum ServerMessage {
    Exchange(SyncResponse),
    Bootstrap(BootstrapResponse),
    Error(ProtocolErrorResponse),
}
```

Keep the protocol small.

---

# 16. SyncRequest

```rust
pub struct SyncRequest {
    pub session: ClientSession,
    pub scope: SyncScope,
    pub cursor: Option<Cursor>,
    pub operations: BoundedVec<OperationEnvelope>,
    pub capabilities: ClientCapabilities,
}
```

The request performs push and pull together.

---

# 17. SyncResponse

```rust
pub struct SyncResponse {
    pub session: ServerSession,
    pub results: Vec<OperationResult>,
    pub changes: Vec<AuthoritativeChange>,
    pub next_cursor: Cursor,
    pub has_more: bool,
    pub instructions: ServerInstructions,
}
```

---

# 18. OperationResult

```rust
pub enum OperationResult {
    Accepted(OperationAccepted),
    Rejected(OperationRejected),
    Conflict(OperationConflict),
    RetryLater(OperationRetry),
    AlreadyApplied(OperationAccepted),
}
```

---

# 19. Server Instructions

The server can guide the client.

```rust
pub struct ServerInstructions {
    pub resync_required: bool,
    pub min_next_poll_ms: Option<u64>,
    pub max_batch_hint: Option<u32>,
    pub upgrade_required: Option<UpgradeInstruction>,
}
```

Avoid excessive intelligence initially.

Only add hints that produce clear operational benefit.

---

# 20. Cursor Model

A cursor should represent an authoritative replication position.

```rust
pub struct Cursor {
    pub scope: ScopeId,
    pub sequence: Sequence,
    pub generation: CursorGeneration,
}
```

The `generation` permits journal resets or major compaction epochs.

---

# 21. Cursor Validity

The server must validate:

```text
cursor scope matches requested scope
cursor generation is current
cursor sequence exists or is within retained range
```

Possible response:

```text
Valid
Expired
WrongScope
UnknownGeneration
AheadOfServer
```

`AheadOfServer` should be treated as corruption or invalid input.

---

# 22. Client State Model

Persist synchronization metadata separately from domain tables.

Recommended logical tables:

```text
aequora_client_state
aequora_outbox
aequora_inbox
aequora_conflicts
aequora_snapshots
aequora_scope_state
```

---

# 23. Client Sync State

Conceptual record:

```rust
pub struct ClientSyncState {
    pub device_id: DeviceId,
    pub scope: ScopeId,
    pub cursor: Option<Cursor>,
    pub last_successful_sync: Option<SystemTime>,
    pub bootstrap_state: BootstrapState,
}
```

---

# 24. Outbox Record

```rust
pub struct OutboxEntry {
    pub operation: OperationEnvelope,
    pub state: OutboxState,
    pub attempt_count: u32,
    pub next_attempt_at: Option<SystemTime>,
    pub last_error: Option<StoredSyncError>,
}
```

---

# 25. Outbox State

```rust
pub enum OutboxState {
    Pending,
    InFlight,
    WaitingRetry,
    Conflict,
    Rejected,
    Acknowledged,
}
```

`InFlight` must be recoverable after process death.

On startup:

```text
stale InFlight -> Pending
```

because operation IDs make resend safe.

---

# 26. Inbox Ledger

Incoming server events can be recorded in an inbox ledger.

```rust
pub struct InboxEntry {
    pub sequence: Sequence,
    pub event_id: EventId,
    pub applied: bool,
}
```

This is useful for duplicate suppression and diagnostics.

In simple implementations, sequence + cursor may be sufficient.

---

# 27. Local Transaction Pattern

Every synchronizable local command should use a transaction helper.

Conceptual API:

```rust
local_sync.transaction(|tx| async move {
    repositories.students.update(tx, ...).await?;

    tx.enqueue(
        OperationEnvelope::new(...)
    ).await?;

    Ok(())
}).await?;
```

This prevents callers from forgetting the outbox entry.

---

# 28. Prefer a Domain Transaction API

Instead of letting every feature manually manipulate the outbox, expose a higher-level helper:

```rust
pub trait SyncMutationContext {
    async fn persist_domain_change(...);
    async fn enqueue_operation(...);
}
```

Then the application's repository integration ensures the invariant automatically.

---

# 29. Client Coordinator

The client needs one synchronization coordinator per logical store/device.

Responsibilities:

```text
watch outbox
watch connectivity
handle manual sync
periodic retry
construct batches
perform exchange
reconcile responses
emit status
handle shutdown
```

The coordinator should be a Tokio task.

---

# 30. Coordinator Inputs

Use a bounded Tokio channel.

```rust
enum SyncSignal {
    LocalMutation,
    ConnectivityRestored,
    ManualSync,
    Timer,
    Resume,
    Shutdown,
}
```

Multiple signals may be coalesced.

---

# 31. Signal Coalescing

If 100 mutations occur within a short window:

```text
100 LocalMutation signals
```

should not produce 100 requests.

The coordinator can debounce:

```text
first mutation
↓
short batching window
↓
collect queued mutations
↓
one exchange
```

---

# 32. Sync Loop

Conceptual loop:

```text
wait for trigger
    ↓
check network eligibility
    ↓
load due pending operations
    ↓
load cursor
    ↓
build request
    ↓
exchange
    ↓
reconcile
    ↓
if has_more:
    continue immediately
else:
    idle
```

---

# 33. Never Rely Exclusively on Connectivity APIs

Operating-system connectivity APIs can be wrong.

A connection may appear "online" while the server is unreachable.

Therefore:

```text
network signal = hint
actual sync request = truth
```

---

# 34. Retry Scheduler

Retryable failures should use:

```text
exponential backoff
+
jitter
+
server retry-after hints
```

State should be durable enough that app restart does not cause an immediate retry storm.

---

# 35. Retry Classification

Retry:

```text
timeout
temporary DNS failure
connection reset
HTTP 429
HTTP 502
HTTP 503
HTTP 504
temporary storage unavailability
```

Do not automatically retry forever:

```text
authentication failure
authorization failure
invalid schema
malformed operation
business rejection
unsupported protocol
```

---

# 36. Client Reconciliation Transaction

A response should normally be applied as one local transaction.

```text
BEGIN

for each operation result:
    update outbox state

for each server change:
    apply authoritative state

store conflicts

advance cursor

COMMIT
```

If this transaction fails:

```text
cursor must remain unchanged
```

The same server response can safely be reprocessed.

---

# 37. Authoritative Change Representation

```rust
pub struct AuthoritativeChange {
    pub sequence: Sequence,
    pub event_id: EventId,
    pub entity: EntityRef,
    pub version: EntityVersion,
    pub change_kind: ChangeKind,
    pub schema_version: SchemaVersion,
    pub payload: Bytes,
}
```

---

# 38. Change Kinds

```rust
pub enum ChangeKind {
    Created,
    Updated,
    Deleted,
    DomainEvent,
    SnapshotReplacement,
}
```

Avoid excessive genericity.

Applications should still interpret payload semantics.

---

# 39. Server Pipeline Overview

The Axum-facing server path is:

```text
HTTP request
   ↓
body limit
   ↓
transport authentication
   ↓
Postcard decode
   ↓
protocol checks
   ↓
normalize auth context
   ↓
server sync service
   ↓
deduplication
   ↓
dependency analysis
   ↓
authorization
   ↓
domain validation
   ↓
conflict detection
   ↓
execution plan
   ↓
transaction
   ↓
collect journal changes
   ↓
Postcard response
```

---

# 40. Axum Integration Boundary

`aequora-axum` should provide:

```rust
pub fn router<S>(service: Arc<S>) -> Router
where
    S: SyncService + 'static;
```

The crate handles only:

- request extraction;
- body limits;
- content type;
- authentication context extraction hook;
- tracing span creation;
- mapping errors to HTTP responses.

---

# 41. Axum Must Not Know the Domain

The router should not contain:

```rust
if operation == "create_student" { ... }
```

or:

```rust
sqlx::query!("UPDATE students ...")
```

That belongs below the sync service boundary.

---

# 42. Server Sync Service

Conceptual service:

```rust
pub trait SyncService {
    async fn exchange(
        &self,
        auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, SyncServiceError>;

    async fn bootstrap(
        &self,
        auth: AuthContext,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, SyncServiceError>;
}
```

---

# 43. Authentication Context

Authentication should be normalized before core processing.

```rust
pub struct AuthContext {
    pub actor_id: ActorId,
    pub tenant_id: TenantId,
    pub device_id: DeviceId,
    pub roles: SmallVec<[RoleId; 4]>,
    pub session_id: SessionId,
}
```

Never trust IDs duplicated inside client payloads over authenticated context.

---

# 44. Protocol Validation

Before expensive domain logic:

```text
validate magic
validate protocol version
validate batch count
validate total bytes
validate operation kinds
validate dependency limits
validate schema range
validate cursor structure
```

Reject malformed requests early.

---

# 45. Deduplication Stage

Before normal execution:

```text
for each operation:
    lookup OperationId
```

If already committed:

```text
return original stored result
```

without executing again.

This lookup should be indexed.

---

# 46. Deduplication Result Storage

Store enough information to reproduce a stable acknowledgement.

Example logical schema:

```text
operation_id
tenant_id
result_code
entity_id
entity_version
server_sequence
result_payload
created_at
```

The exact storage form is adapter-specific.

---

# 47. Dependency Planner

Operations may reference earlier operations in the same batch.

The planner performs:

```text
validate dependency references
build directed graph
detect cycles
topological sort
group independent operations
```

Complexity should be approximately:

```text
O(V + E)
```

---

# 48. Missing Dependencies

If operation B depends on A but A is:

- already committed: dependency satisfied;
- earlier in current batch: plan A before B;
- absent and unknown: reject/defer B;
- permanently rejected: B should normally be rejected as dependent failure.

---

# 49. Dependency Failure Propagation

Example:

```text
A = CreateStudent
B = CreateInvoice(student=A)
C = RecordPayment(invoice=B)
```

If A fails permanently:

```text
A -> Rejected
B -> DependencyFailed
C -> DependencyFailed
```

Do not attempt B or C.

---

# 50. Validation Architecture

Validation is intentionally split.

```text
wire validation
auth validation
authorization
structural/domain validation
conflict validation
execution preconditions
```

Each layer should return typed errors.

---

# 51. Type-State Pipeline

Recommended internal wrappers:

```rust
pub struct Incoming<T>(T);
pub struct Authenticated<T>(T);
pub struct Authorized<T>(T);
pub struct Validated<T>(T);
pub struct ConflictChecked<T>(T);
pub struct Executable<T>(T);
```

Functions consume one state and return another.

This prevents accidental execution of unvalidated input.

---

# 52. Authorization Contract

```rust
#[async_trait]
pub trait Authorizer {
    async fn authorize(
        &self,
        ctx: &AuthContext,
        op: &DecodedOperation,
    ) -> Result<AuthorizedOperation, AuthorizationError>;
}
```

Authorization should be evaluated against authoritative server context.

---

# 53. Domain Validator Contract

```rust
#[async_trait]
pub trait DomainValidator {
    async fn validate(
        &self,
        ctx: &ValidationContext,
        op: AuthorizedOperation,
    ) -> Result<ValidatedOperation, ValidationError>;
}
```

The context can expose read-only repositories.

---

# 54. Conflict Detector Contract

```rust
#[async_trait]
pub trait ConflictDetector {
    async fn check(
        &self,
        op: ValidatedOperation,
        current: &AuthoritativeState,
    ) -> Result<ConflictCheckedOperation, Conflict>;
}
```

---

# 55. Executor Contract

```rust
#[async_trait]
pub trait Executor {
    async fn execute(
        &self,
        ctx: &ExecutionContext,
        op: ExecutableOperation,
        tx: &mut dyn AuthoritativeTransaction,
    ) -> Result<ExecutionOutcome, ExecutionError>;
}
```

---

# 56. Execution Outcome

```rust
pub struct ExecutionOutcome {
    pub entity: EntityRef,
    pub new_version: EntityVersion,
    pub events: SmallVec<[PendingAuthoritativeEvent; 2]>,
    pub response_payload: Option<Bytes>,
}
```

A single command may emit multiple authoritative events.

---

# 57. Aggregate-Oriented Execution

Prefer operations targeting an aggregate root.

Example:

```text
Invoice
├── invoice header
├── line items
├── adjustments
└── payment relationship
```

The aggregate version protects its invariants.

This is safer than independently synchronizing every table row.

---

# 58. Server Transaction Boundary

For one independent aggregate operation:

```text
BEGIN
  verify idempotency
  load authoritative aggregate
  validate conflict/version
  execute domain command
  persist aggregate
  increment version
  append journal event(s)
  store operation result
COMMIT
```

The acknowledgement is returned only after commit.

---

# 59. Batch Transaction Strategy

Do not automatically put the entire client batch into one database transaction.

A batch may contain independent operations.

Use one of three policies:

```text
AtomicBatch
IndependentOperations
DependencyGroups
```

Default recommendation:

```text
DependencyGroups
```

A dependent chain commits together where business semantics require it.

Independent operations may commit separately.

---

# 60. Why Not One Huge Transaction

A 500-operation batch in one transaction can:

- hold locks too long;
- increase deadlock probability;
- increase memory usage;
- amplify one failure;
- reduce concurrency;
- increase latency.

Use domain-aware groups.

---

# 61. Parallelism Model

There are three distinct concurrency domains:

```text
Tokio async concurrency
Database transaction concurrency
Rayon CPU parallelism
```

Do not mix them blindly.

---

# 62. Tokio Responsibilities

Use Tokio for:

```text
HTTP I/O
database I/O
timers
channels
network retries
async storage calls
coordination
```

---

# 63. Rayon Responsibilities

Use Rayon for sufficiently large CPU-bound work:

```text
checksum verification
batch decoding post-processing
large validation transforms
dependency graph analysis
snapshot transformation
compression preparation
large conflict comparisons
```

---

# 64. Avoid Async Blocking

Never perform a long Rayon computation directly inside latency-sensitive async flow without controlling the boundary.

Recommended abstraction:

```rust
pub trait ComputeExecutor {
    async fn execute_cpu<F, R>(&self, job: F) -> R
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static;
}
```

Implementation delegates to a dedicated Rayon pool.

---

# 65. Dedicated Compute Pool

```rust
pub struct RayonComputePool {
    pool: Arc<rayon::ThreadPool>,
}
```

Configure separately from Tokio.

Example RON:

```ron
compute: (
    rayon_threads: 4,
    parallel_validation_threshold: 128,
    parallel_snapshot_threshold: 4096,
)
```

---

# 66. No Parallelism for Tiny Work

Parallel processing can be slower for small batches.

Use thresholds.

```text
batch size < threshold
    -> sequential

batch size >= threshold
    -> parallel CPU stage
```

---

# 67. Storage Abstraction

Do not define one giant database trait.

Use capability traits.

```text
EntityReader
EntityWriter
TransactionFactory
OutboxStore
CursorStore
ConflictStore
JournalReader
JournalWriter
OperationLedger
SnapshotReader
SnapshotWriter
```

---

# 68. Local Store Contract

```rust
pub trait LocalSyncStore:
    LocalTransactionFactory
    + OutboxStore
    + CursorStore
    + ConflictStore
    + IncomingChangeStore
    + Send
    + Sync
{}
```

---

# 69. Authoritative Store Contract

```rust
pub trait AuthoritativeSyncStore:
    AuthoritativeTransactionFactory
    + JournalReader
    + SnapshotReader
    + OperationLedger
    + Send
    + Sync
{}
```

---

# 70. Transaction Capability

Transactions should expose only synchronization capabilities required within the transaction.

Example:

```rust
pub trait LocalSyncTransaction {
    async fn enqueue(...);
    async fn mark_result(...);
    async fn apply_change(...);
    async fn set_cursor(...);
    async fn commit(self);
}
```

Do not leak database-native transaction types.

---

# 71. Stoolap Adapter

`aequora-store-stoolap` is the first local adapter.

Responsibilities:

```text
create/manage sync metadata tables
atomic local mutation + outbox helper
outbox scanning
cursor persistence
conflict persistence
incoming event application support
transaction implementation
```

---

# 72. Stoolap Suggested Metadata Schema

Logical schema:

```text
aequora_device
aequora_scope
aequora_outbox
aequora_conflict
aequora_inbox
aequora_metadata
```

Do not require application domain tables to follow a fixed schema.

---

# 73. Outbox Suggested Columns

```text
operation_id
scope_id
entity_type
entity_id
base_version
operation_kind
schema_version
dependencies
payload
state
attempt_count
next_attempt_at
created_at
updated_at
```

Binary fields can hold Postcard payloads.

---

# 74. PostgreSQL Adapter

`aequora-store-postgres` is the first authoritative adapter.

Responsibilities:

```text
operation deduplication
authoritative journal
cursor paging
transaction wrappers
snapshot queries
scope filtering
journal compaction support
```

---

# 75. PostgreSQL Logical Tables

Recommended conceptual metadata:

```text
aequora_operation_ledger
aequora_journal
aequora_scope_generation
aequora_snapshot_manifest
aequora_device_watermark
```

The application continues to own its domain tables.

---

# 76. PostgreSQL Journal

Conceptual schema:

```text
sequence BIGINT
tenant_id
scope_id
event_id
operation_id
entity_type
entity_id
entity_version
change_kind
schema_version
payload
created_at
```

Required indexing should support:

```text
(scope_id, sequence)
(operation_id)
(entity_type, entity_id)
```

---

# 77. Server Sequence Generation

PostgreSQL can allocate server ordering through a database sequence or equivalent transactional mechanism.

The public protocol only knows:

```text
Sequence(u64)
```

not how it was generated.

---

# 78. Neon Consideration

Neon/PostgreSQL should be treated exactly as PostgreSQL from Aequora's point of view.

Do not build cloud-provider semantics into the sync library.

Provider-specific concerns belong to deployment/infrastructure.

---

# 79. Conflict Detection Model

Conflict detection should begin with optimistic version checking.

Client sends:

```text
base_version = 17
```

Server currently has:

```text
version = 17
```

No stale-state conflict.

If server has:

```text
version = 19
```

the operation is stale and must enter the conflict strategy.

---

# 80. Conflict Strategy Trait

```rust
pub trait ConflictResolver {
    async fn resolve(
        &self,
        ctx: &ConflictContext,
        incoming: &DecodedOperation,
        current: &AuthoritativeState,
    ) -> Result<ConflictDecision, ConflictError>;
}
```

---

# 81. Conflict Decisions

```rust
pub enum ConflictDecision {
    Apply,
    Reject,
    Merge(MergedOperation),
    Supersede,
    Manual(ConflictRecord),
}
```

---

# 82. Default Conflict Policies

Recommended:

```text
immutable ledger          -> reject mutation
append-only transaction   -> apply if idempotent
profile fields            -> field merge/custom
user preferences          -> LWW acceptable
financial values          -> domain operation
inventory                 -> domain operation
attendance                -> domain-specific
document metadata         -> version check/custom
```

---

# 83. Manual Conflict Storage

The client should retain unresolved conflicts.

```rust
pub struct StoredConflict {
    pub conflict_id: ConflictId,
    pub operation_id: OperationId,
    pub entity: EntityRef,
    pub local_payload: Bytes,
    pub authoritative_payload: Option<Bytes>,
    pub reason: ConflictReason,
    pub created_at: SystemTime,
}
```

---

# 84. Conflict Resolution Is a New Operation

A manual conflict should not mutate local/server state outside the synchronization system.

Resolution should create a new operation.

Example:

```text
Conflict:
phone local=X
server=Y

User chooses X
    ↓
new operation:
ResolveStudentPhoneConflict(...)
```

This preserves auditability.

---

# 85. Deletes and Tombstones

Deletion must produce an authoritative tombstone.

```rust
pub struct TombstoneChange {
    pub entity: EntityRef,
    pub version: EntityVersion,
    pub deleted_at: HybridTimestamp,
}
```

Clients apply deletion and remember enough metadata to prevent resurrection.

---

# 86. Resurrection Rules

If an old offline device submits an update for a tombstoned entity:

```text
base version < tombstone version
```

default behavior should be:

```text
reject as DeletedEntity
```

Any resurrection must be explicit domain behavior.

---

# 87. Snapshot Bootstrap

A new device should not replay the entire journal.

Bootstrap model:

```text
request scope
   ↓
server establishes snapshot boundary
   ↓
snapshot manifest
   ↓
chunk download
   ↓
client validates chunks
   ↓
atomic/local staged install
   ↓
set cursor to snapshot boundary
   ↓
continue incremental sync
```

---

# 88. Bootstrap Request

```rust
pub struct BootstrapRequest {
    pub scope: SyncScope,
    pub capabilities: ClientCapabilities,
    pub preferred_chunk_bytes: u32,
}
```

---

# 89. Bootstrap Manifest

```rust
pub struct SnapshotManifest {
    pub snapshot_id: SnapshotId,
    pub scope: ScopeId,
    pub boundary: Cursor,
    pub chunks: Vec<SnapshotChunkDescriptor>,
    pub schema_version: SchemaVersion,
}
```

---

# 90. Snapshot Chunk Descriptor

```rust
pub struct SnapshotChunkDescriptor {
    pub index: u32,
    pub byte_len: u64,
    pub hash: Hash,
}
```

---

# 91. Snapshot Installation

Do not expose partially installed snapshots to the application.

Recommended process:

```text
download chunks
    ↓
validate hashes
    ↓
load into staging tables/store
    ↓
validate completeness
    ↓
atomic swap/replace
    ↓
commit cursor
```

The exact mechanism is adapter-specific.

---

# 92. Snapshot Interruption

If bootstrap stops halfway:

```text
existing local state remains valid
staging snapshot remains incomplete
cursor does not advance
```

The client may resume or discard staging data later.

---

# 93. Partial Synchronization

Every client should operate within one or more explicit sync scopes.

Examples:

```text
school
branch
academic year
class
workspace
project
user account
```

---

# 94. Scope Descriptor

Core should not hard-code school-specific fields.

Use opaque normalized partitions.

```rust
pub struct SyncScope {
    pub scope_id: ScopeId,
    pub partitions: SmallVec<[PartitionId; 8]>,
}
```

The application maps domain filters to scope IDs.

---

# 95. Scope Change

If the user's permissions or assigned data scope changes:

```text
old scope A
new scope B
```

do not blindly continue the old cursor.

Options:

```text
new scope bootstrap
scope transition protocol
server-generated delta
```

Initial implementation should prefer:

```text
bootstrap new scope
```

for correctness.

---

# 96. Schema Evolution

Maintain independent versions for:

```text
transport protocol
domain operation schema
snapshot schema
local DB schema
server DB schema
```

Do not collapse them into one number.

---

# 97. Operation Upcasting

Suppose the server receives an older operation schema.

```text
CreateStudentV1
    ↓
upcaster
CreateStudentV2
    ↓
current handler
```

This allows a controlled compatibility window.

---

# 98. Compatibility Window

Example policy:

```text
current protocol: 4
supported: 3..=4

current ERP operation schema: 11
supported: 9..=11
```

Clients outside the window receive a typed upgrade response.

---

# 99. Codec Architecture

`aequora-codec` should define:

```rust
pub trait Codec {
    fn encode<T: Serialize>(&self, value: &T) -> Result<Bytes, CodecError>;
    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, CodecError>;
}
```

Production default:

```text
PostcardCodec
```

---

# 100. RON Role

RON should remain for:

- configuration;
- test fixtures;
- human-readable protocol dumps;
- debug snapshots;
- developer tools;
- migration definitions.

Do not use RON as the default network payload.

---

# 101. Diagnostic Conversion

A useful CLI can decode Postcard packets into RON.

Example:

```text
aequora inspect request.bin --format ron
```

This gives binary efficiency in production and human visibility during development.

---

# 102. Compression

Compression belongs after serialization.

```text
domain message
    ↓
Postcard
    ↓
optional zstd
    ↓
HTTP
```

Response advertises compression in protocol flags / content encoding.

---

# 103. Compression Threshold

Configuration example:

```ron
compression: (
    enabled: true,
    min_payload_bytes: 4096,
    level: 3,
)
```

Do not compress tiny messages.

---

# 104. Integrity Hashing

Use payload hashes where corruption detection matters:

```text
snapshot chunks
blob chunks
persisted cached packets
```

Transport TLS is still mandatory.

Hashing is not a substitute for authentication.

---

# 105. Large Blob Separation

Large files should never ride through the normal operation channel.

Use:

```rust
pub struct BlobRef {
    pub hash: BlobHash,
    pub length: u64,
    pub media_type: Option<MediaTypeId>,
}
```

Domain operation carries only the reference.

---

# 106. Blob Sync Protocol

Potential future module:

```text
check blob existence
    ↓
upload missing chunks
    ↓
verify digest
    ↓
commit blob manifest
    ↓
domain operation may reference blob
```

This subsystem should remain optional.

---

# 107. Security Boundary

All client data is untrusted.

Threats:

```text
malformed Postcard
oversized batches
dependency bombs
duplicate operations
tenant spoofing
invalid cursor
unauthorized entity access
compression bombs
replay
resource exhaustion
logic abuse
```

---

# 108. Request Limits

Recommended configurable limits:

```text
max compressed request bytes
max decompressed request bytes
max operations
max operation payload
max dependencies
max strings
max snapshot chunk
max response bytes
max processing deadline
```

---

# 109. Validation Ordering for Security

Perform cheap checks before expensive checks.

```text
body length
↓
decompression bound
↓
Postcard decode
↓
batch count
↓
protocol version
↓
authentication
↓
authorization
↓
database reads
↓
domain execution
```

---

# 110. No Client SQL

The protocol must never support:

```rust
RawSql(String)
```

or:

```rust
ExecuteQuery(Bytes)
```

Doing so destroys:

- portability;
- authorization clarity;
- validation;
- domain semantics;
- security.

---

# 111. Tenant Isolation

A request's effective tenant must come from authenticated context.

Client-supplied tenant metadata is checked for consistency only.

Server repository queries must always include authoritative tenant scope.

---

# 112. Replay Protection

Operation IDs already neutralize repeated business effects.

For stronger session security, authentication layers may additionally use:

```text
token expiry
session IDs
nonce
request timestamp windows
mTLS
```

These belong outside the core business operation semantics.

---

# 113. Rate Limiting

Rate limiting should occur before expensive sync processing.

Dimensions may include:

```text
IP
tenant
actor
device
authentication principal
```

Use Tower/Axum middleware at the edge.

---

# 114. Backpressure

The server must remain stable if clients synchronize simultaneously.

Use:

```text
bounded request sizes
bounded Axum concurrency
bounded DB connection pool
bounded compute pool
bounded per-tenant work
server batch hints
HTTP 429/503 when overloaded
```

---

# 115. Client Backpressure

If the outbox grows rapidly:

```text
10
100
10,000
100,000 operations
```

the client should not load everything into RAM.

Read bounded pages from storage.

---

# 116. Streaming Outbox Reader

Conceptual API:

```rust
async fn next_outbox_batch(
    max_ops: usize,
    max_bytes: usize,
) -> Result<OutboxBatch, StoreError>;
```

Stop when either limit is reached.

---

# 117. Server Pull Paging

Journal queries should be bounded.

```text
cursor = 1000
limit = 1000

return 1001..2000
has_more = true
```

Client immediately continues until caught up or policy says yield.

---

# 118. Fairness

A client with millions of missing events should not monopolize server resources.

Use:

```text
bounded page size
bounded per-request processing time
optional continuation delay
```

---

# 119. Deadlines

Every server request should carry an internal deadline.

Stages must respect it.

Do not start a large snapshot computation when only milliseconds remain.

---

# 120. Observability Model

Every exchange should create a tracing span.

Fields:

```text
request_id
session_id
device_id
tenant_id
scope_id
operation_count
cursor
```

Avoid logging sensitive payloads.

---

# 121. Metrics

Client:

```text
aequora_outbox_pending
aequora_outbox_oldest_seconds
aequora_last_success_timestamp
aequora_sync_latency_seconds
aequora_sync_failures_total
aequora_conflicts_pending
aequora_cursor_lag
```

Server:

```text
aequora_requests_total
aequora_operations_total
aequora_rejections_total
aequora_conflicts_total
aequora_deduplicated_total
aequora_request_bytes
aequora_response_bytes
aequora_validation_seconds
aequora_execution_seconds
aequora_journal_query_seconds
```

---

# 122. Structured Error Model

Library errors should remain typed.

```rust
pub enum SyncError {
    Transport(TransportError),
    Codec(CodecError),
    Protocol(ProtocolError),
    Authentication(AuthenticationError),
    Authorization(AuthorizationError),
    Validation(ValidationError),
    Conflict(ConflictError),
    Execution(ExecutionError),
    Store(StoreError),
    Compatibility(CompatibilityError),
    ResourceLimit(ResourceLimitError),
}
```

---

# 123. User-Facing Error Mapping

The core error taxonomy should not dictate UI strings.

The application maps:

```text
AuthorizationError::Forbidden
```

to:

```text
"You no longer have permission to modify this record."
```

or a localized equivalent.

---

# 124. Stable Error Codes

Protocol responses should contain stable machine-readable codes.

Example:

```text
AEQ-AUTH-001
AEQ-PROTO-003
AEQ-CONFLICT-002
AEQ-SCHEMA-004
```

Human-readable diagnostic text may change.

Machine codes should remain stable.

---

# 125. Testing Layers

Aequora should use several testing layers.

```text
unit tests
property tests
simulation tests
adapter tests
integration tests
fuzzing
fault injection
benchmarks
```

---

# 126. Unit Tests

Test:

```text
ID validation
cursor behavior
batch limits
dependency graph
type-state transitions
retry classification
codec roundtrips
schema migration
```

---

# 127. Property Tests

Useful invariants:

```text
same OperationId never applies twice
cursor never moves backward
reordering independent operations preserves result
retries converge to same state
duplicate incoming events do not duplicate effects
```

---

# 128. Sync Simulator

`aequora-testkit` should provide:

```rust
Simulation {
    server,
    clients,
    network,
    clock,
}
```

The fake network can:

```text
drop packets
duplicate packets
delay packets
reorder delivery
disconnect clients
corrupt payloads
```

---

# 129. Deterministic Clock

Use an injectable clock.

```rust
pub trait Clock {
    fn now(&self) -> HybridTimestamp;
}
```

Testing implementation controls time precisely.

Never bake `SystemTime::now()` throughout business logic.

---

# 130. Fault Injection

Storage adapters used in tests should support failure points.

Example:

```text
fail before journal append
fail after domain write
fail before commit
fail after commit
fail during response encoding
```

This validates transactional recovery.

---

# 131. Critical Failure Scenario

Test continuously:

```text
server accepts operation
server commits transaction
network fails before response
client retries same OperationId
server returns existing result
business effect remains exactly once
```

This is one of the most important distributed-system tests.

---

# 132. Client Crash Scenario

Test:

```text
response received
incoming changes partially processed in memory
client crashes before transaction commit
restart
same response/events arrive again
client applies once
cursor advances correctly
```

---

# 133. Server Crash Scenario

Test:

```text
server starts transaction
writes domain state
process terminates before commit
restart
operation retried
no partial state exists
```

Database transaction semantics should guarantee rollback.

---

# 134. Migration Testing

Maintain fixtures for:

```text
N-2 client
N-1 client
current client
```

against:

```text
current server
```

and vice versa where supported.

---

# 135. Adapter Compliance Test Suite

Every store adapter should pass a common test suite.

```rust
pub trait StoreComplianceHarness {
    fn make_store() -> Self::Store;
}
```

Tests verify:

```text
atomic outbox
cursor durability
deduplication
transaction rollback
journal paging
tombstones
snapshot reads
```

---

# 136. Stoolap Compliance

`aequora-store-stoolap` must pass the exact same local-store behavior tests as future:

```text
SQLite
Redb
custom embedded DB
```

This proves database independence.

---

# 137. PostgreSQL Compliance

`aequora-store-postgres` should pass authoritative-store tests against a real PostgreSQL integration environment.

Do not substitute an in-memory mock for transactional integration tests.

---

# 138. Benchmarks

Benchmark:

```text
Postcard encode/decode
batch building
dependency sort
validation
hashing
compression
outbox scanning
journal paging
snapshot construction
conflict resolution
```

---

# 139. Performance Optimization Rule

Do not optimize based on intuition.

Process:

```text
measure
profile
identify bottleneck
change
measure again
```

This is especially important before adding complex zero-copy designs.

---

# 140. Memory Ownership Strategy

Prefer simple ownership in public APIs.

Use zero-copy selectively inside implementation.

Potential tools:

```text
Bytes
Arc<[u8]>
Cow<'a, [u8]>
SmallVec
Box<[T]>
```

Avoid exposing lifetime-heavy APIs unless profiling proves a need.

---

# 141. Server Deployment Architecture

Initial deployment:

```text
             Internet
                │
                ▼
         TLS / reverse proxy
                │
                ▼
           Axum server
                │
     ┌──────────┼──────────┐
     ▼          ▼          ▼
 auth      sync service  metrics
                │
                ▼
          DB connection pool
                │
                ▼
        PostgreSQL / Neon
```

One server instance is enough initially.

---

# 142. Horizontal Scaling

The architecture should support multiple Axum instances later.

```text
load balancer
   │
   ├── server A
   ├── server B
   └── server C
          │
          ▼
      PostgreSQL
```

Because operation idempotency and journal ordering are stored centrally, sessions need not be pinned to one server.

---

# 143. Stateless Axum Nodes

Keep synchronization correctness in:

```text
database
protocol
operation IDs
cursor
```

not in process memory.

In-memory caches are optional accelerators only.

---

# 144. Multi-Region Later

Do not attempt multi-primary global synchronization in v1.

Start with:

```text
one authoritative write region
```

If multi-region becomes necessary later, the protocol can remain stable while the authoritative store architecture changes.

---

# 145. Dioxus Integration

Dioxus should consume a small facade.

Example:

```rust
pub struct SyncUiState {
    pub status: SyncStatus,
    pub pending: usize,
    pub conflicts: usize,
    pub last_success: Option<Instant>,
}
```

Expose through:

```text
watch channel
signal adapter
application state
```

Aequora itself should not depend on Dioxus.

---

# 146. Application Command Integration

Recommended pattern:

```text
UI
 ↓
Domain command service
 ↓
local repository transaction
 ├─ apply optimistic state
 └─ enqueue Aequora operation
```

Do not let UI components construct raw wire messages.

---

# 147. Server Command Reuse

Normal server APIs and sync operations should reuse the same application command handlers.

```text
REST endpoint ───┐
                 ▼
            Domain Handler
                 ▲
                 │
Aequora sync ────┘
```

This prevents divergent business rules.

---

# 148. Finance-Safe Pattern

For accounting/finance:

Never synchronize mutable balances directly.

Synchronize commands/events such as:

```text
PostJournalEntry
ReceivePayment
IssueRefund
ReverseTransaction
ApplyAdjustment
```

The authoritative server validates double-entry/accounting invariants.

---

# 149. Audit Integration

Aequora journal and business audit log should remain logically distinct.

However, execution can emit both atomically.

```text
domain mutation
+
sync journal
+
business audit record
+
operation ledger
```

within one transaction when required.

---

# 150. Privacy and Sensitive Data

Avoid storing full payloads in:

```text
logs
metrics
error reports
tracing spans
```

Protocol payloads may contain personal or financial information.

Use IDs and safe metadata in observability.

---

# 151. Configuration Model

Example `aequora.ron`:

```ron
AequoraConfig(
    protocol: (
        min_version: 1,
        max_version: 1,
    ),

    client: (
        max_push_operations: 256,
        max_push_bytes: 1048576,
        max_pull_events: 1024,
        debounce_ms: 150,
    ),

    server: (
        max_request_bytes: 4194304,
        max_decompressed_bytes: 16777216,
        max_operations: 512,
        request_timeout_ms: 15000,
    ),

    retry: (
        initial_ms: 500,
        max_ms: 30000,
        multiplier: 2.0,
        jitter: true,
    ),

    compute: (
        rayon_threads: 4,
        parallel_threshold: 128,
    ),

    compression: (
        enabled: true,
        algorithm: Zstd,
        min_bytes: 4096,
    ),
)
```

---

# 152. Feature Design

Prefer separate integration crates over giant feature matrices.

Good:

```text
aequora-core
aequora-axum
aequora-store-postgres
aequora-store-stoolap
```

rather than making `aequora-core` conditionally compile every ecosystem dependency.

---

# 153. API Stability

The stable public API should focus on:

```text
IDs
protocol messages
core traits
client engine
server engine
configuration
error types
adapter traits
```

Internal planners and execution details can remain private.

---

# 154. Versioning Policy

Use semantic versioning.

Breaking protocol compatibility does not necessarily require breaking Rust API compatibility, and vice versa.

Document both:

```text
crate/API compatibility
wire protocol compatibility
```

---

# 155. CLI Tool

A companion binary is extremely useful.

Suggested name:

```text
aequora-cli
```

Commands:

```text
aequora inspect
aequora decode
aequora encode
aequora validate
aequora journal
aequora conflicts
aequora benchmark
aequora snapshot
```

---

# 156. Packet Inspector

Example:

```text
aequora decode request.bin --to ron
```

Output:

```ron
SyncRequest(
    ...
)
```

This makes Postcard practical during development.

---

# 157. Development Test Server

Provide an in-process or local development server.

```text
cargo run -p aequora-example-server
```

with:

```text
in-memory store
test auth
verbose RON diagnostics
```

This accelerates application integration.

---

# 158. First Production Milestone

The first usable release should support:

```text
one client DB adapter: Stoolap
one server DB adapter: PostgreSQL
Axum HTTPS transport
Postcard
client outbox
server journal
idempotency ledger
entity versions
cursor pull
push+pull exchange
basic conflicts
retry/backoff
tombstones
bootstrap snapshot
test simulator
tracing
```

That is already a substantial and production-useful sync engine.

---

# 159. What NOT to Build in Version 1

Do not start with:

```text
QUIC
CRDT framework
multi-primary
peer-to-peer synchronization
adaptive ML batching
arbitrary plugin runtime
distributed consensus
custom storage engine
complex blob deduplication
global multi-region writes
```

These can be added after the core invariants are proven.

---

# 160. Phase 0 — Specification

Before implementation:

1. freeze terminology;
2. define invariants;
3. define wire messages;
4. define error taxonomy;
5. define adapter contracts;
6. define conflict semantics;
7. define cursor rules;
8. define operation lifecycle.

Deliverables:

```text
SPEC.md
PROTOCOL.md
INVARIANTS.md
ERRORS.md
```

---

# 161. Phase 1 — Core Types

Implement:

```text
aequora-types
aequora-core
aequora-protocol
aequora-codec
aequora-clock
```

Tests:

```text
serialization roundtrip
bounded types
IDs
cursor rules
protocol compatibility
```

---

# 162. Phase 2 — Storage Traits

Implement:

```text
aequora-store
aequora-journal
aequora-testkit in-memory stores
```

Before writing the real DB adapters, prove the abstractions with in-memory tests.

---

# 163. Phase 3 — Client Engine

Implement:

```text
outbox
coordinator
batch builder
retry scheduler
reconciler
cursor management
status reporting
```

Use fake transport initially.

---

# 164. Phase 4 — Server Engine

Implement:

```text
registry
validator pipeline
deduplication
dependency planner
conflict engine
executor
journal pull
```

Use in-memory authoritative store initially.

---

# 165. Phase 5 — End-to-End Simulation

Connect:

```text
in-memory client
fake network
in-memory server
```

Simulate:

```text
offline
reconnect
duplicate delivery
crashes
conflicts
multi-device
```

Do not move to database adapters until these invariants pass.

---

# 166. Phase 6 — Stoolap Adapter

Implement:

```text
metadata migrations
outbox
cursor
conflict storage
local transactional helper
incoming reconciliation
```

Run local-store compliance suite.

---

# 167. Phase 7 — PostgreSQL Adapter

Implement:

```text
journal
operation ledger
transactions
cursor queries
snapshot support
```

Run authoritative-store compliance suite.

---

# 168. Phase 8 — Axum Transport

Implement:

```text
POST /sync/v1/exchange
POST /sync/v1/bootstrap
GET /sync/v1/health
```

Add:

```text
TLS termination
body limits
timeouts
authentication integration
rate limiting
tracing
```

---

# 169. Phase 9 — Real ERP Integration

Integrate one bounded ERP vertical first.

Recommended:

```text
Student profile
```

Then add:

```text
Attendance
Fees
Accounting
Documents
```

Do not begin with every ERP module at once.

---

# 170. Phase 10 — Performance

Only after correctness:

```text
profiling
batch tuning
Rayon thresholds
journal indexes
compression thresholds
allocation reduction
snapshot chunk tuning
```

---

# 171. Phase 11 — Production Hardening

Add:

```text
fuzzing
fault injection
load tests
migration tests
security review
rate limiting
monitoring
backup/restore validation
chaos scenarios
```

---

# 172. Recommended Module Tree

Example `aequora-client`:

```text
src/
├── lib.rs
├── engine.rs
├── coordinator.rs
├── batch.rs
├── outbox.rs
├── reconciliation.rs
├── retry.rs
├── status.rs
├── trigger.rs
└── error.rs
```

---

# 173. Recommended Server Tree

```text
src/
├── lib.rs
├── service.rs
├── session.rs
├── dedup.rs
├── planner.rs
├── authorization.rs
├── validation.rs
├── conflict.rs
├── execution.rs
├── pull.rs
├── bootstrap.rs
└── error.rs
```

---

# 174. Protocol Tree

```text
src/
├── lib.rs
├── envelope.rs
├── request.rs
├── response.rs
├── operation.rs
├── change.rs
├── cursor.rs
├── capability.rs
├── bootstrap.rs
├── error.rs
└── version.rs
```

---

# 175. Adapter Tree — Stoolap

```text
src/
├── lib.rs
├── connection.rs
├── migration.rs
├── transaction.rs
├── outbox.rs
├── cursor.rs
├── inbox.rs
├── conflict.rs
└── error.rs
```

---

# 176. Adapter Tree — PostgreSQL

```text
src/
├── lib.rs
├── pool.rs
├── migration.rs
├── transaction.rs
├── ledger.rs
├── journal.rs
├── cursor.rs
├── snapshot.rs
└── error.rs
```

---

# 177. Example End-to-End Operation

Client creates a student.

```text
Dioxus
 ↓
CreateStudent local command
 ↓
Stoolap transaction
 ├─ INSERT local student
 └─ INSERT Aequora outbox operation
 ↓
commit
```

Aequora coordinator wakes.

```text
load operation
 ↓
build SyncRequest
 ↓
Postcard encode
 ↓
HTTPS
 ↓
Axum
```

Server:

```text
decode
 ↓
authenticate
 ↓
deduplicate
 ↓
decode CreateStudent payload
 ↓
authorize
 ↓
validate
 ↓
execute
 ↓
PostgreSQL transaction
 ├─ insert student
 ├─ version = 1
 ├─ journal event
 └─ operation ledger
 ↓
commit
```

Return:

```text
Accepted
entity version = 1
sequence = 90001
```

Client:

```text
BEGIN
mark operation acknowledged
apply authoritative representation
set cursor = 90001
COMMIT
```

---

# 178. Multi-Device Example

Desktop cursor:

```text
100
```

Android cursor:

```text
80
```

Desktop makes update:

```text
server sequence 101
```

Android next sync:

```text
cursor 80
```

server returns:

```text
81..101
```

Android catches up without knowing anything about Desktop.

---

# 179. Offline Conflict Example

Both devices have:

```text
Student version = 12
```

A changes phone.

B changes address.

A syncs:

```text
version 13
```

B syncs with base 12.

Conflict resolver inspects field semantics.

If fields are independent:

```text
merge
version 14
```

If both changed phone:

```text
manual/reject/domain policy
```

---

# 180. Production Readiness Checklist

Before release, verify:

```text
[ ] outbox atomicity
[ ] journal atomicity
[ ] idempotency
[ ] crash recovery
[ ] retry safety
[ ] cursor durability
[ ] tombstone behavior
[ ] snapshot interruption
[ ] schema compatibility
[ ] bounded decoding
[ ] auth isolation
[ ] tenant isolation
[ ] rate limits
[ ] timeouts
[ ] metrics
[ ] tracing
[ ] fuzz tests
[ ] property tests
[ ] integration tests
[ ] adapter compliance tests
[ ] migration tests
[ ] load tests
[ ] backup restore
```

---

# 181. Final Architecture

The production system should ultimately behave like this:

```text
                          CLIENT

┌──────────────────────────────────────────────────────┐
│ Dioxus                                               │
│   ↓                                                  │
│ Domain Services                                      │
│   ↓                                                  │
│ Stoolap Transaction                                  │
│   ├─ Optimistic Domain State                         │
│   └─ Durable Aequora Outbox                          │
│          ↓                                           │
│ Aequora Client Coordinator                           │
│          ↓                                           │
│ Batch + Cursor                                       │
│          ↓                                           │
│ Postcard                                             │
└──────────┬───────────────────────────────────────────┘
           │
           │ HTTPS
           ▼

                    SERVER TRUST BOUNDARY

┌──────────────────────────────────────────────────────┐
│ Axum                                                 │
│   ↓                                                  │
│ Transport Validation                                 │
│   ↓                                                  │
│ Authentication                                       │
│   ↓                                                  │
│ Aequora Sync Service                                 │
│   ↓                                                  │
│ Operation Deduplication                              │
│   ↓                                                  │
│ Dependency Planner                                   │
│   ↓                                                  │
│ Authorization                                        │
│   ↓                                                  │
│ Domain Validation                                    │
│   ↓                                                  │
│ Conflict Detection                                   │
│   ↓                                                  │
│ Execution Plan                                       │
│   ↓                                                  │
│ PostgreSQL Transaction                               │
│   ├─ Domain Mutation                                 │
│   ├─ Entity Version                                  │
│   ├─ Operation Ledger                                │
│   └─ Authoritative Journal                           │
│          ↓                                           │
│ Journal Pull                                         │
│          ↓                                           │
│ Postcard Response                                    │
└──────────┬───────────────────────────────────────────┘
           │
           ▼

                         CLIENT

┌──────────────────────────────────────────────────────┐
│ Response Validation                                  │
│   ↓                                                  │
│ Reconciliation                                       │
│   ↓                                                  │
│ Stoolap Transaction                                  │
│   ├─ Authoritative Changes                           │
│   ├─ ACKs                                            │
│   ├─ Conflicts                                       │
│   └─ Cursor                                          │
│   ↓                                                  │
│ Dioxus observes local database                       │
└──────────────────────────────────────────────────────┘
```

---

# 182. Architectural Principle to Preserve

The most important separation is:

```text
Application semantics
        │
        ▼
Aequora synchronization semantics
        │
        ▼
Storage abstraction
        │
   ┌────┴────┐
   ▼         ▼
Stoolap   PostgreSQL
```

and independently:

```text
Aequora synchronization semantics
        │
        ▼
Transport abstraction
        │
   ┌────┴───────────┐
   ▼                ▼
HTTP/Axum       future QUIC
```

Therefore the reusable engine remains valid even when:

- the client database changes;
- the server database changes;
- the UI framework changes;
- the HTTP framework changes;
- the deployment provider changes;
- the application domain changes.

The protocol's long-term value comes from maintaining those boundaries.

---

# 183. Recommended Next Documents

After this file, the architecture should be split into implementation specifications:

```text
protocol.md
    Exact binary protocol, message definitions, versioning, error codes.

client.md
    Local outbox, coordinator, retry, reconciliation, Stoolap integration.

server.md
    Axum pipeline, validation, execution, transaction semantics.

storage.md
    Adapter traits and compliance requirements.

conflicts.md
    Conflict algorithms and domain policy model.

bootstrap.md
    Snapshots, scope changes, journal compaction.

security.md
    Threat model, resource limits, tenant isolation, authentication boundary.

testing.md
    Simulator, property tests, fuzzing, fault injection.

implementation.md
    Exact crate-by-crate coding sequence and milestone gates.
```

These documents should describe the same architecture rather than introducing competing implementations.

---

# 184. Final Recommendation

Build Aequora first as a **small, rigorously correct synchronization kernel**.

The foundation should be:

```text
typed IDs
typed errors
durable outbox
authoritative journal
OperationId idempotency
explicit entity versions
server monotonic cursor
Postcard protocol
Axum transport adapter
database capability traits
Stoolap local adapter
PostgreSQL authoritative adapter
deterministic tests
```

Only after those guarantees are proven should you add:

```text
Rayon optimization
compression
advanced merge
snapshots at scale
blob synchronization
QUIC
CRDTs
multi-region
```

The difficult part of synchronization is not moving bytes quickly.

The difficult part is guaranteeing that after retries, crashes, concurrent edits, partial failures, offline periods, schema upgrades, and multiple devices, the system still reaches a correct and explainable state.

Aequora should optimize for that property first.

---

# 185. Implementation Status

Sections 1–184 are implemented or resolved by an explicit v1 policy in
`docs/next-completion.md`. The completion record maps every section to current code/tests and
records the deliberate non-resetting cursor, independent-operation batch, opaque authoritative
payload, compact operation-kind, and single-event decisions so recommendations are not mistaken
for unsupported guarantees.

The final missing repository-owned reliability item was durable retry scheduling. Retry attempt
counts and next-attempt deadlines are now persisted by the local-store contract, enforced by
due-only Stoolap scans, honored by the client retry loop, migration-versioned, restart-tested, and
required by the common local adapter suite.

Production backup/restore, external TLS, and deployment capacity tests remain acceptance gates for
the actual hosting environment and must not be reported as passed without that environment.
