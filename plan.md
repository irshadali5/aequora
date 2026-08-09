A strong design for this should treat PostgreSQL, Stoolap, SQLite, Redb, or any other database as persistence implementations, not as the sync protocol itself. PostgreSQL remains authoritative in your current ERP architecture, while the same engine can support a different authority model later.

One important correction to the stack: Tokio handles asynchronous network/database I/O; Rayon should handle CPU-heavy parallel work only—batch validation, hashing, compression preparation, dependency analysis (use guppy), large diffs, etc. Rayon is specifically designed for data-parallel CPU workloads.

# Aequora Sync

## Database-Agnostic Local-First Synchronization Engine for Rust

**Working crate name:** `aequora-sync`

> A reusable, strongly typed, database-independent synchronization engine written in pure Rust for synchronizing local application state with an authoritative server through an Axum validation and execution boundary.

---

# 0. Non-Negotiable Project Direction

Aequora is a database-neutral synchronization engine. Database products are adapters at the outer
edge; they are never protocol identities, mandatory pairings, or assumptions inside the client,
server, transport, validation, execution, or domain layers.

There are three independent composition axes:

```text
local persistence       LocalStore
authoritative storage   AuthoritativeStore
network boundary        SyncTransport / ExchangeService
```

An application must be able to replace any one axis without rewriting either of the other two.
The client engine is generic over `L: LocalStore` and `T: SyncTransport`. The server is generic over
`S: AuthoritativeStore`. These capability contracts—not database names—are the stable public API.

Built-in integrations are optional conveniences:

```text
aequora-store-stoolap    local/client adapter only
aequora-store-postgres   authoritative/server adapter only
aequora-http + Axum      transport boundary only
```

None of these crates may select, import, or require its opposite-side database. A public user may
use a custom database on the client, the server, or both. SQL, table definitions, connection pools,
transactions, and driver error types must remain inside the corresponding adapter crate.

The owner deployment is a permanent acceptance target:

```text
Stoolap client → HTTPS/Axum → Neon pooled PostgreSQL authority
                               └─ direct Neon endpoint for migrations
```

Supporting that deployment must never weaken generality. Conversely, database neutrality must
never remove or silently stop testing the owner deployment.

Every release must therefore prove all of the following:

1. No database feature: custom local and custom authority adapters can compile.
2. Local feature only: Stoolap compiles without SQLx or the PostgreSQL adapter.
3. Authority feature only: PostgreSQL compiles without Stoolap or its adapter.
4. Both features: the owner deployment compiles without introducing protocol coupling.
5. Reusable conformance tests validate third-party `LocalStore` and `AuthoritativeStore` semantics.
6. A live integration test covers persistent Stoolap, HTTP/Axum, and real PostgreSQL; the identical
   path covers Neon whenever pooled and direct test URLs are configured.

This section governs later implementation choices. If a later example or adapter conflicts with
these rules, preserve this direction and change the implementation.

---

# 1. Executive Summary

Aequora Sync is not a PostgreSQL synchronizer, a Stoolap synchronizer, or a SQL replication system.

It is a **domain synchronization protocol and execution framework**.

The fundamental model is:

```text
Client Database
      │
      │ local transaction
      ▼
Local Change Journal / Outbox
      │
      │ typed operations
      ▼
Aequora Client Sync Engine
      │
      │ Postcard over HTTPS
      ▼
Axum Sync Gateway
      │
      ▼
Authentication
      │
Authorization
      │
Protocol Validation
      │
Schema Validation
      │
Business Validation
      │
Conflict Detection
      │
Command Execution
      ▼
Server Storage Adapter
      │
      ▼
Authoritative Database
      │
      ▼
Authoritative Change Journal
      │
      ▼
Sync Response
      │
      ▼
Client Reconciler
      │
      ▼
Client Database
```

The critical principle is:

> **Synchronize operations and authoritative state transitions—not database pages, SQL statements, tables, or database-specific WAL formats.**

Client and authority database choices are orthogonal. A deployment may use a built-in adapter on
either side, both sides, or neither side. Enabling a local adapter must not select an authority
adapter, and enabling an authority adapter must not select a local adapter. The ERP deployment is
Stoolap on the client and Neon/PostgreSQL on the server, but that pairing is an acceptance case
rather than a protocol dependency or framework default.

This allows:

```text
Stoolap        → PostgreSQL
SQLite         → PostgreSQL
Redb           → PostgreSQL
Stoolap        → MySQL
Stoolap        → custom KV store
SQLite         → distributed database
memory store   → PostgreSQL
```

without redesigning the synchronization protocol.

For your school ERP:

```text
Dioxus application
        │
        ▼
Stoolap
        │
        ▼
aequora-sync-client
        │
        ▼
HTTPS
        │
        ▼
Axum
        │
        ▼
aequora-sync-server
        │
        ▼
application/domain services
        │
        ▼
SQLx PostgreSQL adapter
        │
        ▼
Neon PostgreSQL
```

The server remains the **source of truth**.

---

# 2. Design Goals

Aequora should provide:

1. Local-first operation.
2. Offline writes.
3. Bidirectional synchronization.
4. Database independence.
5. Transport independence.
6. Strong Rust type safety.
7. Idempotent retries.
8. Conflict detection.
9. Conflict resolution.
10. Deterministic synchronization.
11. Partial synchronization.
12. Batched synchronization.
13. Multi-device support.
14. Multi-user support.
15. Multi-tenant support.
16. Fine-grained authorization.
17. Schema evolution.
18. Protocol evolution.
19. Dependency ordering.
20. Transaction boundaries.
21. Automatic retry.
22. Backpressure.
23. Crash recovery.
24. Observability.
25. Compression.
26. Efficient binary transport.
27. Human-readable configuration.
28. Testing without real databases.
29. Pluggable database adapters.
30. Pluggable validation policies.
31. Pluggable conflict strategies.
32. Reusable domain-independent infrastructure.

---

# 3. Non-Goals

Aequora should deliberately **not** attempt to become:

* a database;
* an ORM;
* PostgreSQL logical replication;
* database WAL replication;
* distributed SQL;
* a CRDT database;
* an event broker;
* a message queue;
* an Axum replacement;
* a networking framework;
* an application authorization system.

It provides integration points for those responsibilities.

---

# 4. Architectural Philosophy

The architecture should follow:

```text
Persistence
    ≠
Synchronization
    ≠
Business Logic
    ≠
Transport
```

These layers must remain independent.

For example:

```text
Student creation
```

should never become:

```sql
INSERT INTO students ...
```

inside the sync protocol.

Instead:

```rust
CreateStudent {
    student_id,
    admission_number,
    name,
    ...
}
```

travels through the synchronization protocol.

The server then decides what that command means.

This distinction becomes extremely important when business rules change.

---

# 5. Authority Model

For your ERP, use:

# Server-Authoritative Local-First

The client may:

* read locally;
* create records locally;
* update records locally;
* delete locally;
* continue working offline.

But local mutations are initially:

```text
PROVISIONAL
```

until accepted by the authoritative server.

The server can:

```text
accept
reject
modify
merge
supersede
defer
```

an operation.

Therefore:

```text
Client state = working state

Server state = authoritative state
```

Eventually:

```text
Client state → converges → Server state
```

---

# 6. Three Fundamental Objects

Aequora revolves around three concepts.

## 6.1 Entity

A piece of application state.

Examples:

```text
Student
Teacher
Invoice
Payment
Attendance
LedgerEntry
Document
```

---

## 6.2 Operation

An attempted state transition.

Examples:

```text
CreateStudent
UpdateStudentPhone
MarkAttendance
PostPayment
CancelInvoice
```

---

## 6.3 Event

The authoritative result produced by the server.

Example:

```text
Operation:

PostPayment {
    invoice: INV-22,
    amount: ₹5,000
}

Server event:

PaymentPosted {
    payment_id: ...
    invoice_id: ...
    remaining_balance: ...
}
```

This gives us:

```text
Command/Operation
       ↓
Validator
       ↓
Executor
       ↓
Authoritative Event
       ↓
Persistent State
```

---

# 7. Why Operations Are Better Than Raw Row Synchronization

Imagine two users modify:

```text
Invoice.balance
```

Raw synchronization sees:

```text
old balance = 10,000
device A     = 7,000
device B     = 8,000
```

It cannot understand what happened.

Domain synchronization instead sees:

```text
Device A:
PaymentReceived ₹3,000

Device B:
PaymentReceived ₹2,000
```

Now the server can correctly produce:

```text
final balance = ₹5,000
```

Instead of incorrectly choosing one value.

This becomes extremely important for:

* accounting;
* payments;
* inventory;
* attendance;
* payroll;
* student fees;
* examinations;
* workflow systems.

---

# 8. Repository Layout

I would make Aequora a Rust workspace.

```text
aequora/
│
├── Cargo.toml
│
├── README.md
├── LICENSE
│
├── deny.toml
│
├── rustfmt.toml
│
├── clippy.toml
│
├── examples/
│
├── benches/
│
├── fuzz/
│
└── crates/
    │
    ├── aequora-core/
    ├── aequora-types/
    ├── aequora-protocol/
    ├── aequora-codec/
    ├── aequora-clock/
    ├── aequora-journal/
    ├── aequora-store/
    ├── aequora-client/
    ├── aequora-server/
    ├── aequora-validator/
    ├── aequora-executor/
    ├── aequora-conflict/
    ├── aequora-transport/
    ├── aequora-axum/
    ├── aequora-observability/
    ├── aequora-testkit/
    │
    ├── aequora-store-stoolap/
    ├── aequora-store-postgres/
    │
    └── aequora/
```

The final:

```text
aequora
```

crate can provide convenient re-exports.

---

# 9. Dependency Direction

Dependencies should always point inward.

```text
                       application
                           │
                 ┌─────────┴─────────┐
                 ▼                   ▼
        aequora-client       aequora-server
                 │                   │
                 └─────────┬─────────┘
                           ▼
                     aequora-core
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
           protocol      types        traits
```

Database crates depend upon abstractions:

```text
aequora-store
       ↑
       │
 ┌─────┴─────────┐
 │               │
Stoolap       PostgreSQL
adapter        adapter
```

Never:

```text
aequora-core → PostgreSQL
```

or:

```text
aequora-core → Stoolap
```

The facade feature graph must preserve all four supported profiles:

```text
custom LocalStore    + custom AuthoritativeStore
Stoolap LocalStore   + custom AuthoritativeStore
custom LocalStore    + PostgreSQL AuthoritativeStore
Stoolap LocalStore   + PostgreSQL/Neon AuthoritativeStore
```

Transport selection is independent again: in-process, HTTP/Axum, QUIC, or an application-defined
transport must not determine either database.

---

# 10. Core Identifiers

Never rely upon database auto-increment IDs for synchronization.

Use globally unique identifiers generated client-side.

Recommended:

```rust
pub struct EntityId(Uuid);
pub struct OperationId(Uuid);
pub struct DeviceId(Uuid);
pub struct ActorId(Uuid);
pub struct TenantId(Uuid);
pub struct BatchId(Uuid);
```

UUIDv7 is particularly useful because it provides approximately time-ordered identifiers.

Other options:

```text
ULID
UUIDv7
custom 128-bit ID
```

But expose your own newtypes.

Never expose raw UUID everywhere.

---

# 11. Strongly Typed IDs

Instead of:

```rust
fn get_student(id: Uuid)
```

prefer:

```rust
fn get_student(id: StudentId)
```

For example:

```rust
pub struct StudentId(EntityId);

pub struct InvoiceId(EntityId);

pub struct PaymentId(EntityId);
```

This allows the compiler to prevent:

```text
TeacherId accidentally passed as StudentId
```

---

# 12. Operation Envelope

Every mutation should travel inside an envelope.

Conceptually:

```rust
pub struct OperationEnvelope<O> {
    pub protocol_version: ProtocolVersion,

    pub operation_id: OperationId,

    pub tenant_id: TenantId,

    pub actor_id: ActorId,

    pub device_id: DeviceId,

    pub entity: EntityRef,

    pub base_version: Option<EntityVersion>,

    pub created_at: HybridTimestamp,

    pub schema_version: SchemaVersion,

    pub operation: O,

    pub metadata: OperationMetadata,
}
```

The envelope provides synchronization metadata independently from business payloads.

---

# 13. Entity References

```rust
pub struct EntityRef {
    pub entity_type: EntityType,
    pub entity_id: EntityId,
}
```

Do not make `entity_type` arbitrary user strings on the hot path.

Prefer:

```rust
#[repr(u16)]
pub enum EntityType {
    Student = 1,
    Teacher = 2,
    Invoice = 3,
    Payment = 4,
}
```

or application-defined numeric IDs.

This makes Postcard representation much smaller.

---

# 14. Operation IDs and Idempotency

Every mutation gets a permanent:

```text
OperationId
```

Suppose the client sends:

```text
operation ABC
```

The connection dies after the server commits it.

The client does not receive the response.

Client retries:

```text
operation ABC
```

The server must recognize:

```text
ABC already executed
```

and return the previous result.

It MUST NOT execute it again.

This provides:

# Exactly-once logical effects over at-least-once delivery

The network may deliver operations repeatedly.

The server produces the business effect once.

---

# 15. Change Journal

Both sides should maintain a durable synchronization journal.

Client:

```text
local_change_log
```

Server:

```text
authoritative_change_log
```

Conceptually:

```rust
pub struct JournalEntry {
    sequence: Sequence,
    operation_id: OperationId,
    entity: EntityRef,
    version: EntityVersion,
    change_kind: ChangeKind,
    timestamp: HybridTimestamp,
}
```

---

# 16. Client Outbox

Every local mutation must use an atomic transaction:

```text
BEGIN

modify domain table

+

append outbox operation

COMMIT
```

Never:

```text
write database
COMMIT

then

write sync queue
```

Otherwise a crash between those operations creates unsynchronizable state.

The correct invariant is:

> Local domain state and its synchronization intent commit atomically.

---

# 17. Example Stoolap Transaction

Conceptually:

```text
transaction
│
├── UPDATE students
│
└── INSERT sync_outbox
│
└── COMMIT
```

Stoolap provides ACID transactions and MVCC, which makes this pattern appropriate for its adapter.

But Aequora itself knows nothing about Stoolap.

---

# 18. Store Abstraction

Define capabilities rather than SQL APIs.

Something approximately like:

```rust
pub trait SyncStore {
    type Error;

    async fn begin(&self) -> Result<Self::Transaction, Self::Error>;

    async fn read_entity(
        &self,
        entity: EntityRef,
    ) -> Result<Option<EntitySnapshot>, Self::Error>;

    async fn read_changes_after(
        &self,
        cursor: Cursor,
        limit: usize,
    ) -> Result<ChangePage, Self::Error>;
}
```

Then a transactional abstraction:

```rust
pub trait SyncTransaction {
    async fn apply(
        &mut self,
        mutation: AuthoritativeMutation,
    ) -> Result<(), StoreError>;

    async fn append_journal(
        &mut self,
        event: AuthoritativeEvent,
    ) -> Result<Sequence, StoreError>;

    async fn mark_operation_applied(
        &mut self,
        id: OperationId,
    ) -> Result<(), StoreError>;

    async fn commit(self) -> Result<(), StoreError>;
}
```

---

# 19. Capability-Based Adapter Design

Do not make every database implement unnecessary functionality.

Split traits:

```text
EntityReader
EntityWriter
TransactionStore
ChangeJournal
OperationLedger
SnapshotStore
CursorStore
MetadataStore
```

Then define:

```rust
pub trait AuthoritativeStore:
    EntityReader
    + EntityWriter
    + TransactionStore
    + ChangeJournal
    + OperationLedger
{}
```

And:

```rust
pub trait LocalStore:
    EntityReader
    + EntityWriter
    + TransactionStore
    + OutboxStore
    + CursorStore
{}
```

This is significantly cleaner than one enormous `Database` trait.

---

# 20. Client Architecture

The client contains:

```text
Application
    │
    ▼
Local Repository
    │
    ▼
Local Transaction
    ├─────────────► domain tables
    │
    └─────────────► synchronization outbox
                         │
                         ▼
                  Change Collector
                         │
                         ▼
                    Batch Builder
                         │
                         ▼
                    Sync Session
                         │
                         ▼
                     Transport
```

Incoming data follows:

```text
Transport
    │
    ▼
Response Decoder
    │
    ▼
Integrity Validator
    │
    ▼
Reconciliation Planner
    │
    ▼
Local Transaction
    ├── authoritative updates
    ├── conflicts
    ├── acknowledgements
    └── cursor update
```

---

# 21. Server Architecture

```text
Internet
   │
   ▼
Axum
   │
   ▼
Transport Middleware
   │
   ├── TLS
   ├── request limits
   ├── authentication
   ├── rate limiting
   └── tracing
   │
   ▼
Protocol Decoder
   │
   ▼
Sync Session Handler
   │
   ▼
Protocol Validator
   │
   ▼
Authorization Validator
   │
   ▼
Domain Validator
   │
   ▼
Dependency Planner
   │
   ▼
Conflict Detector
   │
   ▼
Command Executor
   │
   ▼
Store Transaction
   │
   ├── mutate authoritative data
   ├── append events
   ├── mark OperationIds
   └── commit
   │
   ▼
Pull Change Collector
   │
   ▼
Response Encoder
```

---

# 22. Axum's Responsibility

Axum should only be the **network boundary**.

It should perform:

```text
HTTP extraction
authentication extraction
body limits
request decompression
Postcard decoding
calling synchronization service
encoding response
HTTP errors
```

It should NOT contain:

```text
business rules
SQL
conflict handling
entity mutation logic
```

Thus:

```rust
async fn sync(
    State(sync): State<Arc<SyncService>>,
    auth: AuthContext,
    body: Bytes,
) -> Result<Response, SyncHttpError>
```

should mostly delegate to:

```rust
sync.process(auth, request).await
```

---

# 23. Validator / Executor Architecture

Your proposed "Axum validator/executor" concept is good, but it should actually become multiple stages.

Use:

```text
Decode
  ↓
ProtocolValidator
  ↓
IdentityValidator
  ↓
AuthorizationValidator
  ↓
SchemaValidator
  ↓
DomainValidator
  ↓
ConflictValidator
  ↓
DependencyPlanner
  ↓
Executor
```

Do not create one giant:

```rust
Validator
```

with every responsibility.

---

# 24. Validation Pipeline

## Stage 1 — Transport Validation

Check:

```text
HTTP body size
compression format
content type
request deadline
authentication
```

---

## Stage 2 — Protocol Validation

Check:

```text
magic bytes
protocol version
message kind
frame length
batch length
schema compatibility
```

---

## Stage 3 — Identity Validation

Verify:

```text
tenant
actor
device
session
```

---

## Stage 4 — Authorization Validation

Example:

```text
Teacher A may update attendance for Section A.

Teacher A may NOT update:
Teacher B's salary.
```

Authorization belongs on the server.

Never trust authorization decisions made by clients.

---

# 25. Structural Validation

For example:

```text
student name ≤ 200 characters
currency amount valid
date within valid representation
required identifier present
```

Prefer validation when constructing domain types.

Instead of:

```rust
String
```

use:

```rust
StudentName
```

whose constructor guarantees validity.

---

# 26. Business Validation

Example:

```text
student exists
invoice open
academic year active
payment amount positive
teacher assigned to class
attendance date valid
```

These validations may require authoritative database reads.

---

# 27. Conflict Validation

Check:

```text
base_version
```

against:

```text
authoritative_version
```

If:

```text
base_version == server_version
```

normal execution.

If:

```text
base_version < server_version
```

concurrent modification exists.

Then invoke the conflict policy.

---

# 28. Executor

Validation answers:

```text
"May this happen?"
```

Executor answers:

```text
"Apply it."
```

Example trait:

```rust
pub trait OperationExecutor<O> {
    type Output;

    async fn execute(
        &self,
        ctx: &ExecutionContext,
        operation: Validated<O>,
        tx: &mut dyn Transaction,
    ) -> Result<Self::Output, ExecuteError>;
}
```

Notice:

```text
Validated<O>
```

rather than:

```text
O
```

The type system can prevent accidentally executing unvalidated input.

---

# 29. Type-State Validation

Rust can encode the workflow:

```rust
Incoming<O>
```

becomes:

```rust
Authenticated<O>
```

then:

```rust
Authorized<O>
```

then:

```rust
Validated<O>
```

then:

```rust
Executable<O>
```

Conceptually:

```text
Incoming
   │ authenticate
   ▼
Authenticated
   │ authorize
   ▼
Authorized
   │ validate
   ▼
Validated
   │ plan
   ▼
Executable
```

The executor accepts only:

```rust
Executable<O>
```

This moves entire classes of programming mistakes to compile time.

---

# 30. Synchronization Protocol

Use a request-response sync exchange.

Client sends:

```text
SyncRequest
```

containing:

```text
session metadata
client cursor
pending operations
capabilities
```

Server returns:

```text
SyncResponse
```

containing:

```text
operation results
server changes
next cursor
conflicts
resync instructions
```

---

# 31. Sync Request

Conceptually:

```rust
pub struct SyncRequest {
    pub protocol: ProtocolVersion,

    pub session: SessionMetadata,

    pub cursor: Option<Cursor>,

    pub operations: Vec<EncodedOperation>,

    pub limits: ClientLimits,

    pub capabilities: Capabilities,
}
```

---

# 32. Sync Response

```rust
pub struct SyncResponse {
    pub protocol: ProtocolVersion,

    pub acknowledged: Vec<OperationAck>,

    pub rejected: Vec<OperationRejection>,

    pub conflicts: Vec<Conflict>,

    pub changes: Vec<RemoteChange>,

    pub next_cursor: Cursor,

    pub has_more: bool,

    pub server_time: HybridTimestamp,
}
```

---

# 33. Push and Pull in One Exchange

Normally use:

```text
Client
  |
  | pending operations
  | cursor = 5000
  ▼
Server
  |
  | operation acknowledgements
  | changes 5001...5100
  | cursor = 5100
  ▼
Client
```

This reduces HTTP round trips.

---

# 34. Cursor-Based Pulling

Do not use timestamps as synchronization cursors.

Bad:

```text
give me records modified since 13:45:02
```

Clock skew and equal timestamps create problems.

Use server-issued monotonic cursors:

```text
Cursor(918182)
```

The meaning is:

```text
client has processed all authoritative events <= 918182
```

Then:

```text
GET changes > 918182
```

This is deterministic.

---

# 35. Cursor Scope

A cursor should have a defined scope.

For example:

```rust
pub struct Cursor {
    scope: SyncScopeId,
    sequence: u64,
}
```

Never assume a global sequence must be exposed across all tenants.

You can maintain:

```text
tenant-specific sequences
workspace sequences
server global internal sequences
```

---

# 36. Server Journal

For PostgreSQL, conceptually:

```text
sync_events

sequence
tenant_id
entity_type
entity_id
entity_version
operation_id
event_type
payload
created_at
```

Indexes:

```text
(tenant_id, sequence)
(operation_id)
(entity_type, entity_id)
```

The exact database schema belongs to the PostgreSQL adapter.

---

# 37. Operation Ledger

Maintain:

```text
applied_operations
```

containing:

```text
operation_id
result
server_sequence
processed_at
```

This supports idempotency.

When receiving an operation:

```text
if OperationId exists:
    return stored result

otherwise:
    execute
```

---

# 38. Client Outbox States

Operations can move through:

```text
Pending
    ↓
Sending
    ↓
Acknowledged

Pending
    ↓
Sending
    ↓
Rejected

Pending
    ↓
Sending
    ↓
Conflict

Pending
    ↓
Sending
    ↓
Retry
```

Persist these states.

Do not keep synchronization status only in memory.

---

# 39. Crash Recovery

Imagine:

```text
1. server response received
2. client applies changes
3. client crashes
4. cursor not persisted
```

After restart it receives the same changes.

Therefore applying incoming changes must also be idempotent.

Ideally:

```text
BEGIN

apply authoritative events

mark event IDs/sequences processed

update sync cursor

update outbox acknowledgements

COMMIT
```

One atomic local transaction.

---

# 40. Versions

Every synchronizable entity should have an authoritative version.

For example:

```rust
pub struct EntityVersion(u64);
```

Initial creation:

```text
version 1
```

Update:

```text
version 2
```

Another update:

```text
version 3
```

Client operation carries:

```text
base_version = 2
```

If server is:

```text
version = 2
```

no conflict.

If:

```text
version = 5
```

the operation was based on stale state.

---

# 41. Never Use Timestamp as Entity Version

Do not make:

```text
updated_at
```

your conflict/version mechanism.

Use explicit:

```text
version: u64
```

and separately store:

```text
updated_at
```

for user-facing information.

---

# 42. Hybrid Logical Clock

For ordering client-originated activity, use a Hybrid Logical Clock.

Conceptually:

```rust
pub struct HybridTimestamp {
    physical_ms: i64,
    logical: u32,
    node: NodeId,
}
```

But distinguish:

```text
HLC ordering
```

from:

```text
server authoritative sequence
```

Use server sequence for replication cursors.

Use HLC for causal/event metadata.

---

# 43. Conflict Strategies

Different data requires different conflict semantics.

Aequora should support strategies including:

```text
Reject
ServerWins
ClientWins
LastWriterWins
FieldMerge
CustomMerge
CommutativeOperation
CRDT
ManualResolution
```

But don't make LWW the universal default.

---

# 44. Recommended Defaults

Use:

```text
financial data       → domain operations / reject illegal conflict
ledger entries       → immutable
payments             → append-only operation
attendance           → custom business resolution
profiles             → field-level merge
preferences          → LWW acceptable
documents            → version or dedicated document merge
counters             → commutative operation / CRDT
```

---

# 45. Accounting Must Not Use LWW

For example, never sync:

```text
balance = 5000
```

as a mutable value between devices.

Instead send:

```text
PaymentPosted(+2000)
RefundIssued(-500)
ChargeCreated(+1000)
```

and calculate balance from authoritative ledger state.

This prevents catastrophic financial conflicts.

---

# 46. Deletes

Never immediately physically remove synchronized entities.

Represent deletion as:

```text
tombstone
```

Example:

```rust
pub struct Tombstone {
    entity: EntityRef,
    deleted_at: HybridTimestamp,
    deleted_version: EntityVersion,
}
```

Clients must receive the tombstone.

Otherwise an offline client may later resurrect the deleted row.

---

# 47. Tombstone Garbage Collection

Deletion flow:

```text
Active
   ↓
Deleted/Tombstone
   ↓
retention period
   ↓
safe garbage collection
```

Garbage collection may depend on:

```text
device cursor watermarks
retention policy
inactive-device expiration
```

Example:

```text
devices inactive > 90 days require full resync
```

Then old tombstones can eventually be removed.

---

# 48. Initial Synchronization

A brand-new device has no cursor.

Do not replay years of operation history.

Use:

# Snapshot + Cursor

Server produces:

```text
snapshot at sequence 5,000,000
```

Client downloads snapshot.

Then starts incremental sync:

```text
cursor = 5,000,000
```

This avoids replaying millions of historical events.

---

# 49. Snapshot Protocol

```text
Client
   │
   ├── BootstrapRequest
   ▼
Server
   │
   ├── SnapshotMetadata
   ├── chunks
   └── cursor
   ▼
Client
```

Snapshot can be chunked:

```text
students
teachers
classes
attendance
invoices
...
```

---

# 50. Snapshot Consistency

All snapshot chunks must correspond to one logical snapshot boundary.

Do not generate:

```text
students at cursor 100
teachers at cursor 150
```

unless the protocol explicitly accounts for it.

The cleaner model is:

```text
snapshot sequence = 150
```

followed by:

```text
incremental changes > 150
```

---

# 51. Partial Synchronization

Large ERP clients should not necessarily download an entire organization's database.

Define:

```rust
pub struct SyncScope {
    tenant: TenantId,
    partitions: Vec<Partition>,
}
```

Example teacher device:

```text
school = ABC
academic_year = 2026
classes = [8A, 8B]
```

Administration desktop:

```text
school = ABC
all modules
```

---

# 52. Partitioning

Potential partition keys:

```text
tenant
school
branch
academic year
class
department
user
project
workspace
```

Partitioning should be domain-configured.

The core engine should treat them as opaque scope identifiers.

---

# 53. Dependency Graph

Operations frequently depend upon one another.

Example:

```text
CreateStudent A

then:

CreateInvoice(student=A)

then:

RecordPayment(invoice=B)
```

The server cannot safely execute them randomly.

Represent dependencies:

```rust
dependencies: SmallVec<[OperationId; 4]>
```

Then build a DAG.

```text
CreateStudent
      │
      ▼
CreateInvoice
      │
      ▼
RecordPayment
```

---

# 54. Dependency Planner

The server performs:

```text
operation collection
      ↓
dependency graph
      ↓
cycle detection
      ↓
topological sort
      ↓
execution groups
```

Cycles are rejected as invalid protocol/application batches unless explicitly supported.

---

# 55. Rayon

Rayon should be used carefully.

Rayon is appropriate for CPU-bound parallelism such as:

```text
decode-independent validation
hash verification
checksum calculation
large snapshot transformations
conflict candidate comparison
compression preprocessing
dependency graph calculations
large batch normalization
serialization preparation
```

Rayon is designed for data-parallel CPU execution and work-stealing parallelism.

Do NOT use Rayon as a replacement for Tokio.

---

# 56. Tokio vs Rayon

Use:

```text
Tokio
```

for:

```text
HTTP
Axum
database waits
timers
sockets
TLS
async storage
```

Use:

```text
Rayon
```

for:

```text
CPU-heavy algorithms
parallel validation
hashing
large transformations
```

Architecture:

```text
                    Sync Request
                         │
                         ▼
                     Tokio Task
                         │
         ┌───────────────┴────────────────┐
         │                                │
         ▼                                ▼
 Async DB/network                  CPU-heavy batch
       Tokio                            Rayon
```

Never perform a huge Rayon computation directly on Axum's async execution thread without crossing a proper boundary.

---

# 57. Dedicated Rayon Pool

Do not blindly use the process-global pool for server-critical workloads.

Aequora can expose:

```rust
pub struct ComputePool {
    pool: rayon::ThreadPool,
}
```

Configuration:

```ron
compute: (
    worker_threads: 8,
    parallel_validation_threshold: 128,
)
```

Below the threshold:

```text
validate sequentially
```

Above it:

```text
use Rayon
```

Parallelism has overhead, so tiny batches should remain sequential.

---

# 58. What Can Be Validated in Parallel?

Operations targeting unrelated aggregate roots may be validated concurrently.

Example:

```text
Student A update
Student B update
Teacher C update
```

Potentially parallel.

But:

```text
Invoice A update
Payment for Invoice A
Cancel Invoice A
```

must be ordered.

Therefore:

```text
Dependency Planner
      ↓
Independent Execution Groups
      ↓
parallel CPU validation
```

rather than indiscriminately:

```rust
operations.par_iter()
```

---

# 59. Database Writes and Parallelism

Avoid uncontrolled parallel database writes.

A single batch should often produce:

```text
parallel pre-validation

        ↓

deterministic execution planning

        ↓

transactional execution
```

rather than:

```text
Rayon writing 500 records concurrently
```

The database itself manages internal concurrency.

---

# 60. Serialization Strategy

You asked for:

```text
RON
Postcard
```

Use both—but for very different purposes.

---

# 61. Postcard

Use Postcard for:

```text
network protocol
operation payload
event payload
cached serialized operations
persistent outbox binary payload
snapshot binary chunks
```

For example:

```text
Content-Type:
application/vnd.aequora.postcard
```

Postcard keeps protocol frames compact and strongly integrates with Serde.

---

# 62. RON

Use RON for:

```text
configuration
development fixtures
debug dumps
schema descriptions
test scenarios
manual conflict policies
developer tooling
```

Example:

```ron
SyncConfig(
    push_batch_size: 256,
    pull_batch_size: 1024,

    retry: (
        initial_ms: 500,
        maximum_ms: 30000,
    ),

    compression: Zstd,

    compute: (
        rayon_threads: 6,
    ),
)
```

---

# 63. Never Use RON for Main Production Transport

RON is human-friendly.

Postcard is machine-friendly.

Therefore:

```text
RON      → control plane/configuration
Postcard → data plane
```

This distinction should remain permanent.

---

# 64. Optional JSON

A reusable library should optionally provide JSON support for:

```text
debugging
web interoperability
third-party APIs
admin tooling
protocol inspection
```

But JSON does not need to be the primary sync representation.

Feature:

```toml
aequora = {
    features = [
        "postcard",
        "ron",
    ]
}
```

Optional:

```text
json
```

---

# 65. Framing

Postcard itself serializes structures.

You still need application-level protocol framing.

Conceptually:

```text
+--------------------+
| Magic              |
+--------------------+
| Protocol version   |
+--------------------+
| Flags              |
+--------------------+
| Message type       |
+--------------------+
| Payload length     |
+--------------------+
| Payload checksum   |
+--------------------+
| Payload            |
+--------------------+
```

For HTTP, some framing information may overlap with HTTP metadata, but an internal envelope remains valuable.

---

# 66. Protocol Magic

For example:

```text
AEQ1
```

This quickly prevents accidentally interpreting unrelated data as Aequora messages.

---

# 67. Protocol Versioning

Separate:

```text
protocol version
```

from:

```text
domain schema version
```

Example:

```text
ProtocolVersion(2)

StudentSchemaVersion(7)
```

Changing Student fields should not automatically require an entirely new transport protocol.

---

# 68. Capability Negotiation

Client sends:

```text
supported capabilities
```

Example:

```text
postcard-v1
zstd
snapshot-v2
field-merge
streaming-pull
```

Server chooses mutually compatible features.

This allows rolling upgrades.

---

# 69. Forward Compatibility

Prefer additive protocol evolution.

For example:

```text
v1:
A B C

v2:
A B C D
```

instead of constantly changing semantic meaning of existing fields.

Use explicit enum/version migration where necessary.

---

# 70. Compression

Compression can be valuable for:

```text
snapshots
large pull responses
documents metadata
thousands of operations
```

Consider:

```text
zstd
```

as an optional feature.

Avoid compressing every 100-byte request.

Use a configurable size threshold.

---

# 71. Payload Integrity

HTTPS protects transport.

Aequora can additionally use payload hashes for:

```text
snapshot chunks
blob manifests
large batch verification
```

Use modern cryptographic hashing such as:

```text
BLAKE3
```

where suitable.

Do not invent cryptography.

---

# 72. Authentication

Authentication should be outside generic synchronization semantics.

Provide:

```rust
pub struct AuthContext {
    actor: ActorId,
    tenant: TenantId,
    device: DeviceId,
    session: SessionId,
}
```

The application converts:

```text
JWT/session/mTLS/etc.
```

into:

```text
AuthContext
```

Aequora consumes the normalized context.

---

# 73. Authorization

Define:

```rust
pub trait Authorizer<O> {
    async fn authorize(
        &self,
        ctx: &AuthContext,
        operation: &O,
    ) -> Result<Authorization, AuthorizationError>;
}
```

This allows each application to implement its own permissions.

---

# 74. Tenant Isolation

Never trust:

```text
tenant_id
```

sent inside the operation.

Compare it against authenticated tenant context.

Ideally derive tenant context server-side.

Request claiming:

```text
tenant = school B
```

while authenticated for:

```text
school A
```

must be rejected before domain execution.

---

# 75. Server Transaction Model

For an operation:

```text
BEGIN

check operation ID

read authoritative state

validate

detect conflicts

apply domain mutation

increment entity version

append authoritative event

record operation result

COMMIT
```

This should be one database transaction whenever possible.

---

# 76. Critical Atomicity Invariant

The following must never happen:

```text
domain update committed

but

sync event missing
```

Therefore:

```text
authoritative data mutation
+
authoritative journal append
+
operation ledger update
```

must commit atomically.

---

# 77. PostgreSQL Adapter

For your server:

```text
aequora-store-postgres
```

can use:

```text
sqlx
```

or another pure-Rust-compatible PostgreSQL client layer.

It implements only Aequora storage traits.

Your sync engine should not expose:

```text
sqlx::Transaction
```

through its public API.

Otherwise PostgreSQL leaks into the abstraction.

---

# 78. Stoolap Adapter

Client:

```text
aequora-store-stoolap
```

provides:

```text
local transaction
outbox storage
cursor storage
tombstones
local entity mutations
conflict storage
```

Stoolap is currently an embedded pure-Rust SQL database with transactions and MVCC, making those capabilities relevant to this adapter design.

But another project can replace it with:

```text
aequora-store-sqlite
```

without touching the protocol.

---

# 79. Application Domain Integration

Do not ask Aequora to understand every entity.

Instead applications register operation handlers.

Example:

```text
ERP crate

register:
    CreateStudentHandler
    UpdateStudentHandler
    RecordAttendanceHandler
    CreateInvoiceHandler
    RecordPaymentHandler
```

Aequora provides:

```text
transport
journaling
ordering
retries
cursoring
batching
idempotency
conflict infrastructure
```

Application provides:

```text
meaning
validation
authorization
execution
merge semantics
```

---

# 80. Operation Registry

A server can maintain:

```text
OperationKind
    ↓
Decoder
    ↓
Validator
    ↓
Executor
```

Conceptually:

```text
1 → CreateStudent
2 → UpdateStudent
3 → DeleteStudent
10 → CreateInvoice
11 → PostPayment
```

Prefer compact numeric protocol identifiers over large repeated strings.

---

# 81. Generic vs Dynamic Operations

Avoid forcing every application operation into one enormous generic enum inside `aequora-core`.

Better architecture:

```text
core envelope
+
application operation payload
```

The application defines:

```rust
enum ErpOperation {
    Student(StudentOperation),
    Attendance(AttendanceOperation),
    Finance(FinanceOperation),
}
```

while Aequora remains application-independent.

---

# 82. Reconciliation

After receiving an authoritative response, the client must reconcile optimistic local state.

Possible response:

```text
Accepted
AcceptedWithChanges
Rejected
Conflict
Superseded
```

---

# 83. Accepted

```text
local state
    ↓
server accepts unchanged
    ↓
mark synchronized
```

---

# 84. Accepted With Modification

Example:

Client creates:

```text
Invoice {
    local number: TEMP-1
}
```

Server assigns:

```text
invoice_number = INV-2026-00098
```

Response contains authoritative changes.

Client updates local entity.

---

# 85. Rejected

Example:

Client offline:

```text
Mark attendance
```

But server later determines:

```text
academic year already closed
```

Then:

```text
operation → rejected
```

The client should preserve:

```text
rejection reason
original operation
local context
```

rather than silently deleting it.

---

# 86. Conflict Object

```rust
pub struct Conflict {
    pub operation_id: OperationId,

    pub entity: EntityRef,

    pub client_base: EntityVersion,

    pub server_version: EntityVersion,

    pub policy: ConflictPolicy,

    pub resolution: ConflictResolution,
}
```

Optional application-specific details can be attached separately.

---

# 87. Manual Conflict UI

Some conflicts cannot safely be automated.

For example:

```text
administrative correction
financial reversal
student transfer
```

Expose them to the application:

```text
ConflictInbox
```

The UI can show:

```text
Your change
Server value
Difference
Recommended resolution
```

---

# 88. Retry Strategy

Transient failures:

```text
connection reset
timeout
503
database unavailable
```

should retry.

Permanent failures:

```text
unauthorized
invalid operation
schema incompatible
business rule rejected
```

should not.

Use typed errors.

---

# 89. Error Taxonomy

```rust
enum SyncError {
    Transport(TransportError),
    Protocol(ProtocolError),
    Authentication(AuthenticationError),
    Authorization(AuthorizationError),
    Validation(ValidationError),
    Conflict(ConflictError),
    Storage(StorageError),
    Execution(ExecutionError),
    Compatibility(CompatibilityError),
}
```

Never reduce everything to:

```rust
anyhow!("sync failed")
```

inside library boundaries.

Applications may use `anyhow` at their outer boundary.

The library should expose meaningful errors.

---

# 90. Backoff

Use exponential backoff with jitter.

Conceptually:

```text
500 ms
1 sec
2 sec
4 sec
8 sec
16 sec
30 sec max
```

with randomness.

This prevents thousands of clients reconnecting simultaneously after an outage.

---

# 91. Sync State Machine

Client engine:

```text
            ┌──────────────┐
            │   Dormant    │
            └──────┬───────┘
                   │ work/network
                   ▼
            ┌──────────────┐
            │ Connecting   │
            └──────┬───────┘
                   ▼
            ┌──────────────┐
            │   Syncing    │
            └──────┬───────┘
                   ▼
          ┌──────────────────┐
          │    Reconciling   │
          └────────┬─────────┘
                   ▼
            ┌──────────────┐
            │    Idle      │
            └──────────────┘

errors:

Syncing
   ↓
Backoff
   ↓
Connecting
```

Persist sufficient state for crash recovery.

---

# 92. Triggering Synchronization

Client can sync when:

```text
application starts
network becomes available
local mutation occurs
periodic timer fires
user manually refreshes
application resumes
server notification arrives
```

Debounce frequent mutations.

For example:

```text
20 changes within 200 ms
```

can become one batch.

---

# 93. Batch Strategy

Configuration:

```ron
batch: (
    max_operations: 256,
    max_bytes: 1048576,
    max_wait_ms: 100,
)
```

Batch by whichever limit is reached first.

---

# 94. Adaptive Batching

Eventually Aequora can dynamically tune:

```text
batch size
compression
concurrency
```

based on:

```text
latency
error rate
server feedback
network quality
device resources
```

But start with deterministic static configuration.

---

# 95. Slow Networks

The design must tolerate:

```text
mobile connections
packet loss
high latency
brief connectivity
```

Therefore:

```text
small resumable batches
cursor-based incremental pulls
idempotent operation submission
compressed snapshots
```

are more important than maintaining permanent WebSocket connections.

---

# 96. HTTP First

For the first production version, use:

```text
HTTPS request/response
```

through Axum.

Do not start with custom TCP or QUIC.

HTTP gives:

```text
proxies
TLS tooling
load balancers
observability
CDNs where relevant
mature infrastructure
simple debugging
```

The transport abstraction allows QUIC later.

---

# 97. Transport Trait

```rust
pub trait SyncTransport {
    async fn exchange(
        &self,
        request: SyncRequest,
    ) -> Result<SyncResponse, TransportError>;
}
```

Implementations:

```text
HttpTransport
InProcessTransport
TestTransport
future QuicTransport
```

The client engine does not care.

---

# 98. Axum Endpoints

Minimal API:

```text
POST /sync/v1/exchange

POST /sync/v1/bootstrap

GET  /sync/v1/health
```

Optionally:

```text
POST /sync/v1/blob/*
```

for a separate blob subsystem.

Avoid building dozens of CRUD synchronization endpoints.

---

# 99. Why One Exchange Endpoint?

Instead of:

```text
POST /students
PUT /student/1
POST /attendance
...
```

sync traffic becomes:

```text
POST /sync/v1/exchange
```

because synchronization is a protocol independent of public CRUD APIs.

Normal APIs may coexist separately.

---

# 100. Attachments and Large Files

Do NOT place:

```text
100 MB PDF
video
image
large binary
```

inside normal operation batches.

Use content-addressed blob synchronization.

Operation contains:

```text
BlobRef {
    digest,
    length,
}
```

Blob service handles the bytes separately.

---

# 101. Blob Architecture

```text
domain operation
      │
      ▼
 BlobManifest
      │
      ▼
missing blob query
      │
      ▼
chunk upload
      │
      ▼
hash verification
```

The core sync engine synchronizes metadata/reference state.

---

# 102. Security

Threat model includes:

```text
malicious client
stolen token
replayed request
oversized request
invalid Postcard payload
invalid enum discriminant
tenant spoofing
operation replay
confused deputy
resource exhaustion
dependency bombs
compression bombs
```

Every client must be treated as untrusted.

Even your own official Dioxus application.

---

# 103. Resource Limits

Define strict limits:

```text
maximum HTTP body
maximum operations per batch
maximum operation payload
maximum dependencies per operation
maximum snapshot chunk
maximum decompressed size
maximum validation complexity
```

Never accept unlimited `Vec`s from an untrusted network.

---

# 104. Bounded Protocol Types

Instead of conceptually allowing:

```rust
Vec<Operation>
```

validate into:

```text
BoundedBatch<Operation, MAX_OPERATIONS>
```

Likewise:

```text
BoundedString
BoundedBytes
BoundedDependencies
```

This is an excellent place to exploit Rust's type system.

---

# 105. Serialization Security

Deserialize into transport DTOs first.

Do not deserialize untrusted bytes straight into fully trusted domain objects.

Pipeline:

```text
bytes
 ↓
WireRequest
 ↓
structural validation
 ↓
ValidatedRequest
 ↓
domain conversion
```

---

# 106. Database Security

Never accept SQL from the client.

Absolutely avoid:

```text
SyncOperation::RawSql(...)
```

That destroys database independence and security.

Operations are domain commands.

---

# 107. Observability

Instrument:

```text
sync sessions
request latency
operations pushed
events pulled
bytes uploaded
bytes downloaded
validation duration
database duration
conflicts
rejections
retry count
snapshot duration
queue depth
cursor lag
```

Use:

```text
tracing
```

for structured Rust telemetry.

Optional:

```text
OpenTelemetry
Prometheus
```

through feature flags/integration crates.

---

# 108. Trace Context

Every sync session should have:

```text
sync_session_id
request_id
device_id
tenant_id
```

in tracing spans.

Do not log sensitive business payloads by default.

---

# 109. Metrics Worth Tracking

Client:

```text
outbox_pending
oldest_pending_age
sync_last_success
sync_latency
sync_failures
conflicts_pending
```

Server:

```text
sync_requests
operations_processed
operation_rejections
operation_conflicts
journal_lag
bootstrap_count
batch_size
validation_seconds
execution_seconds
```

---

# 110. Configuration

Use RON.

Example:

```ron
AequoraConfig(
    protocol: (
        version: 1,
    ),

    push: (
        max_operations: 256,
        max_bytes: 1048576,
    ),

    pull: (
        max_events: 1024,
        max_bytes: 4194304,
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
        algorithm: Zstd,
        min_bytes: 4096,
    ),

    limits: (
        max_operation_bytes: 262144,
        max_dependencies: 32,
    ),
)
```

---

# 111. Feature Flags

Possible crate configuration:

```toml
[features]
default = ["postcard"]

postcard = []
ron = []
json = []

rayon = []

axum = []

stoolap = []
postgres = []

zstd = []
blake3 = []

tracing = []
otel = []

testkit = []
```

But avoid creating hundreds of mutually interacting flags.

Database implementations should preferably remain separate crates.

---

# 112. Public API Design

Keep public API small.

Something conceptually like:

```rust
let engine = ClientSyncEngine::builder()
    .store(store)
    .transport(transport)
    .codec(PostcardCodec)
    .config(config)
    .build()?;
```

Then:

```rust
engine.sync().await?;
```

Applications should not need to know 50 internal components.

---

# 113. Server API

Conceptually:

```rust
let sync = SyncServer::builder()
    .store(postgres)
    .authorizer(authorizer)
    .operations(erp_registry)
    .conflicts(conflicts)
    .compute_pool(compute_pool)
    .build()?;
```

Then:

```rust
Router::new()
    .merge(aequora_axum::routes(sync))
```

---

# 114. Background Worker

Client application can run:

```text
SyncCoordinator
```

with Tokio.

Channels:

```text
LocalMutation
NetworkChanged
ManualSync
PeriodicSync
Shutdown
```

Example architecture:

```text
Dioxus
  │
  ├── mutation
  ▼
Stoolap
  │
  ▼
Outbox
  │
  │ wake
  ▼
SyncCoordinator
  │
  ▼
HttpTransport
```

The UI never directly handles synchronization protocol details.

---

# 115. Notifications to UI

Expose application events:

```rust
enum SyncStatus {
    Offline,
    Idle,
    Synchronizing,
    Conflict,
    Error,
}
```

and perhaps:

```text
pending_operations
last_successful_sync
```

through an observable/watch channel.

Do not bind the sync library directly to Dioxus.

Dioxus consumes generic sync status.

---

# 116. Local Reads

A local-first application should usually read:

```text
UI → local database
```

not:

```text
UI → server API
```

Therefore:

```text
Dioxus
  ↓
Repository
  ↓
Stoolap
```

works even offline.

Synchronization updates Stoolap in the background.

---

# 117. Local Writes

```text
Dioxus
   ↓
Domain Service
   ↓
Local transaction
   ├── mutate Stoolap entity
   └── append Aequora operation
```

The UI immediately sees the result.

No network waiting.

---

# 118. Complete Example

Teacher offline marks:

```text
Student Ali = Present
```

Client generates:

```text
OperationId = 019...
DeviceId = ...
BaseVersion = 12

MarkAttendance {
    student_id,
    date,
    status: Present
}
```

Local transaction:

```text
BEGIN

update attendance locally

insert operation into outbox

COMMIT
```

UI immediately shows:

```text
Present
```

---

When network returns:

```text
Aequora Sync Coordinator
        ↓
read pending outbox
        ↓
create SyncRequest
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
authenticate teacher
 ↓
authorize class
 ↓
validate date
 ↓
compare entity version
 ↓
execute MarkAttendance
 ↓
update PostgreSQL
 ↓
version 12 → 13
 ↓
append sync event #88291
 ↓
record OperationId
 ↓
commit
```

Response:

```text
Operation accepted
version = 13
sequence = 88291
```

Client:

```text
BEGIN

mark outbox acknowledged

apply authoritative version 13

advance cursor

COMMIT
```

Synchronization complete.

---

# 119. Concurrent Modification Example

Device A and B both have:

```text
Student.version = 5
```

A changes:

```text
phone
```

B changes:

```text
address
```

A syncs first:

```text
server version → 6
```

B submits:

```text
base_version = 5
server_version = 6
```

Conflict detector invokes:

```text
StudentConflictPolicy
```

Because fields differ:

```text
phone
address
```

policy can safely merge.

Result:

```text
version → 7
```

Both modifications survive.

---

# 120. Same-Field Conflict

A:

```text
phone = X
```

B:

```text
phone = Y
```

Now field merge cannot automatically prove correctness.

Possible result:

```text
manual conflict
```

or domain policy:

```text
most recent authorized administrative update wins
```

The synchronization infrastructure should not decide this universally.

---

# 121. Referential Integrity

Offline clients may create related objects before the server knows either ID.

Client-generated UUIDs solve this.

Example:

```text
StudentId = S
InvoiceId = I
```

Then:

```text
CreateStudent(S)

CreateInvoice(
    invoice_id = I,
    student_id = S
)
```

The dependency graph guarantees order.

No server-generated primary key remapping is necessary.

---

# 122. Database Primary Keys

You may still use PostgreSQL internal numeric keys for optimization if desired.

But synchronized identity should remain stable:

```text
sync_id UUID
```

Do not make synchronization dependent on:

```text
SERIAL/BIGSERIAL
```

---

# 123. Database Schema Independence

A client may store:

```text
Student
```

in one normalized Stoolap table.

Server may store:

```text
students
student_contacts
student_guardians
```

in three PostgreSQL tables.

Aequora does not care.

It synchronizes:

```text
Student domain operation
```

rather than mirroring SQL structures.

This is one of the largest benefits of this architecture.

---

# 124. Schema Migrations

There are three different migrations:

```text
local database migration
server database migration
sync domain schema migration
```

Keep them separate.

A new server table should not necessarily change the synchronization protocol.

---

# 125. Domain Schema Migration

Suppose:

```text
StudentV1 {
    name
}
```

becomes:

```text
StudentV2 {
    given_name
    family_name
}
```

Support controlled migration:

```text
V1 operation
    ↓
migration adapter
    ↓
current domain model
```

Do not keep every old business implementation forever if avoiding it is possible.

Set supported compatibility windows.

---

# 126. Old Clients

Server may say:

```text
MinimumProtocolVersion = 3
CurrentProtocolVersion = 5
```

Client version 4:

```text
allowed
```

Client version 1:

```text
UpgradeRequired
```

Return a typed compatibility response rather than mysterious decoding failures.

---

# 127. Resynchronization

Sometimes incremental history cannot be continued.

Reasons:

```text
cursor expired
scope changed
journal compacted
schema incompatible
device inactive too long
corruption detected
```

Server returns:

```text
ResyncRequired
```

Client then performs:

```text
bootstrap snapshot
```

Do not attempt increasingly complicated recovery forever.

---

# 128. Compaction

The authoritative journal cannot grow indefinitely without management.

Possible strategy:

```text
current state snapshots
+
recent incremental event window
```

Old events become eligible for compaction after:

```text
retention requirements
active cursor watermarks
audit requirements
```

Audit/event sourcing requirements may require longer retention than synchronization itself.

Keep those concerns separate.

---

# 129. Synchronization Journal vs Audit Log

These are related but should not necessarily be identical.

Sync journal:

```text
optimized for replication
may eventually compact
```

Audit log:

```text
optimized for accountability
may require permanent retention
```

Financial/ERP systems often need an audit trail long after synchronization events are no longer needed.

---

# 130. Testing Architecture

This library needs unusually strong testing because distributed synchronization failures are subtle.

Provide:

```text
aequora-testkit
```

with an in-memory adapter.

---

# 131. Deterministic Simulation

Test:

```text
client A
client B
server
```

without actual networking.

Use:

```text
InMemoryTransport
InMemoryStore
DeterministicClock
```

Then simulate:

```text
offline
duplicate requests
reordered responses
timeouts
server restart
client restart
concurrent mutation
```

---

# 132. Property Testing

Use property testing for invariants.

Examples:

```text
retrying same OperationId never changes state twice
```

```text
cursor never moves backward
```

```text
acknowledged operation is eventually removed from pending outbox
```

```text
applied event cannot be applied twice
```

```text
two converged clients equal authoritative state
```

---

# 133. Fuzzing

Fuzz:

```text
Postcard decoder
protocol framing
dependency graph
migration logic
conflict resolver
batch parser
```

Untrusted network bytes should be a major fuzzing target.

---

# 134. Failure Injection

Test failure at every transactional step:

```text
before write
after write
before journal append
after journal append
before commit
after commit
before response
```

Particularly important scenario:

```text
server commits
network fails before client receives response
```

This verifies idempotency.

---

# 135. Model-Based Testing

Create a simple reference synchronization model.

Generate random:

```text
creates
updates
deletes
offline periods
retries
conflicts
```

Compare Aequora's resulting state against the reference model.

This is extremely valuable for a synchronization engine.

---

# 136. Performance Testing

Benchmark independently:

```text
Postcard encoding
Postcard decoding
validation
dependency sorting
conflict detection
journal queries
snapshot construction
Rayon threshold
reconciliation
```

Do not benchmark only HTTP throughput.

---

# 137. Performance Targets

Reasonable architectural goals—not guarantees:

```text
zero network roundtrip for local writes

O(batch) common validation

O(V + E) dependency graph ordering

bounded memory per request

incremental cursor queries using indexes

batch-oriented transport

minimal allocations where practical
```

---

# 138. Memory Strategy

Avoid unnecessary cloning of operation payloads.

Use:

```text
Bytes
Arc
Cow
borrowed decoding where safely practical
smallvec for small dependency lists
```

But do not pursue "zero-copy everywhere" at the cost of an unusable lifetime-heavy public API.

Correctness first.

Profiling determines where zero-copy is useful.

---

# 139. Async Architecture

Never hold:

```text
MutexGuard
database transaction
large object lock
```

across unrelated `.await`s unnecessarily.

Prefer clear ownership boundaries.

A synchronization batch should have explicit phases:

```text
receive
decode
validate
read state
plan
transaction
encode
respond
```

---

# 140. Locking

Avoid one global synchronization mutex.

Partition locking by:

```text
tenant
aggregate root
entity
```

where appropriate.

PostgreSQL transactional semantics should handle much of authoritative concurrency.

Application-level locks should be exceptional rather than default.

---

# 141. Aggregate Roots

Domain operations should normally lock/conflict around aggregate roots.

Example:

```text
Invoice
├── line items
├── payments
└── adjustments
```

Rather than independently versioning every tiny row without understanding their invariant relationship.

Domain-driven aggregate boundaries can make synchronization much safer.

---

# 142. Recommended Internal Processing Pipeline

Final server pipeline:

```text
HTTP Request
    ↓
Body Limit
    ↓
Postcard Decoder
    ↓
Wire Validation
    ↓
Auth Context
    ↓
Protocol Compatibility
    ↓
Deduplicate Operation IDs
    ↓
Build Dependency DAG
    ↓
Structural Validation
    ↓
Authorization
    ↓
Load Authoritative State
    ↓
Business Validation
    ↓
Conflict Detection
    ↓
Conflict Resolution
    ↓
Execution Plan
    ↓
Atomic Store Transaction
    ├── mutations
    ├── versions
    ├── journal
    └── idempotency ledger
    ↓
Collect Pull Events
    ↓
Build Response
    ↓
Postcard Encode
    ↓
HTTP Response
```

---

# 143. Recommended Client Pipeline

```text
Domain Mutation
    ↓
Local DB Transaction
    ├── optimistic entity update
    └── outbox append
    ↓
Wake Sync Coordinator
    ↓
Read Pending Operations
    ↓
Dependency/Batches
    ↓
Postcard Encode
    ↓
HTTP Exchange
    ↓
Postcard Decode
    ↓
Response Validation
    ↓
Reconciliation Plan
    ↓
Local DB Transaction
    ├── authoritative changes
    ├── tombstones
    ├── operation ACKs
    ├── conflicts
    └── cursor
    ↓
Notify Application
```

---

# 144. Proposed Crate Responsibilities

## `aequora-types`

Contains:

```text
IDs
versions
cursor
timestamps
bounded primitives
common enums
```

No database.

No Axum.

---

## `aequora-protocol`

Contains:

```text
SyncRequest
SyncResponse
OperationEnvelope
RemoteChange
ACK
Conflict
Capability negotiation
ProtocolVersion
```

---

## `aequora-codec`

Contains:

```text
PostcardCodec
RON diagnostic codec
optional JSON codec
compression abstraction
```

---

## `aequora-clock`

Contains:

```text
HLC
Clock trait
SystemClock
TestClock
```

---

## `aequora-store`

Contains persistence traits only.

```text
EntityReader
TransactionStore
JournalStore
OutboxStore
CursorStore
```

---

## `aequora-journal`

Contains:

```text
journal abstractions
cursor semantics
event sequencing
compaction helpers
```

---

## `aequora-validator`

Contains generic validation pipeline machinery.

Not application business rules.

---

## `aequora-conflict`

Contains:

```text
conflict detection
base-version checking
conflict strategy traits
merge helpers
```

---

## `aequora-executor`

Contains:

```text
typed execution pipeline
operation handler registry
execution plans
dependency execution
```

---

## `aequora-client`

Contains:

```text
outbox worker
sync coordinator
batch builder
pull processor
reconciler
retry/backoff
```

---

## `aequora-server`

Contains:

```text
server sync session
validation orchestration
idempotency
execution orchestration
pull collector
bootstrap
```

---

## `aequora-transport`

Contains:

```text
SyncTransport trait
HTTP client transport
in-memory transport
```

---

## `aequora-axum`

Contains only Axum integration:

```text
routes
extractors
middleware integration
HTTP error mapping
```

---

## `aequora-store-stoolap`

Client database adapter.

---

## `aequora-store-postgres`

Server PostgreSQL adapter.

---

## `aequora-observability`

Optional:

```text
tracing
metrics
OpenTelemetry integration
```

---

## `aequora-testkit`

Contains:

```text
InMemoryStore
FakeTransport
FaultInjectingTransport
TestClock
SyncSimulator
assertion helpers
```

---

# 145. ERP Integration Structure

Your ERP workspace could look like:

```text
school-erp/
│
├── crates/
│   │
│   ├── domain/
│   ├── students/
│   ├── attendance/
│   ├── finance/
│   │
│   ├── sync-domain/
│   ├── sync-client/
│   ├── sync-server/
│   │
│   ├── db-stoolap/
│   └── db-postgres/
│
├── apps/
│   ├── desktop/
│   ├── android/
│   └── server/
│
└── vendor/workspace dependency
    └── aequora
```

---

# 146. ERP Sync Domain

```text
sync-domain/
├── src/
│   ├── lib.rs
│   ├── operation.rs
│   ├── event.rs
│   ├── student.rs
│   ├── teacher.rs
│   ├── attendance.rs
│   ├── finance.rs
│   ├── examination.rs
│   └── conflict.rs
```

Example:

```rust
pub enum ErpOperation {
    Student(StudentOperation),
    Teacher(TeacherOperation),
    Attendance(AttendanceOperation),
    Finance(FinanceOperation),
}
```

---

# 147. A Very Important Boundary

Aequora should know:

```text
operation ID
entity ID
version
dependencies
cursor
tenant
device
retry
encoding
```

Aequora should NOT know what:

```text
Student
Invoice
Teacher
Attendance
```

means.

Your ERP knows those things.

This is what makes Aequora reusable.

---

# 148. Data Flow for Your Actual Stack

## Write

```text
Dioxus
   ↓
ERP Domain Service
   ↓
Stoolap transaction
   ├── local ERP tables
   └── aequora_outbox
   ↓
Sync Coordinator
   ↓
Postcard
   ↓
HTTPS
   ↓
Axum
   ↓
Aequora Server
   ↓
ERP Validator
   ↓
ERP Executor
   ↓
SQLx
   ↓
Neon/PostgreSQL
```

---

# 149. Return Flow

```text
Neon/PostgreSQL
   ↓
Aequora authoritative journal
   ↓
PostgreSQL Adapter
   ↓
Aequora Server
   ↓
Postcard
   ↓
Axum response
   ↓
Client HTTP transport
   ↓
Aequora Reconciler
   ↓
Stoolap transaction
   ↓
Dioxus observes local data changes
```

---

# 150. Changes Made by Another Device

Suppose:

```text
Desktop A changes student address.
```

Server journal:

```text
sequence 1001
```

Android B currently has:

```text
cursor 997
```

Android sends normal sync request:

```text
cursor = 997
```

Server returns:

```text
998
999
1000
1001
```

Android transaction:

```text
apply events
cursor = 1001
```

No database-specific synchronization occurs.

---

# 151. Server-Originated Changes

The same system also handles changes created by:

```text
admin portal
background job
scheduled task
import process
server API
another client
```

All authoritative mutations that should synchronize must append to:

```text
authoritative change journal
```

This is essential.

Do not let server-side code bypass the journal.

---

# 152. Command Bus Integration

A particularly strong server architecture is:

```text
Axum
    ↓
Command Bus
    ↓
Domain Handler
    ↓
Transaction
    ├── domain mutation
    └── Aequora event journal
```

Then synchronization and normal server APIs use the same domain command handlers.

You avoid duplicated business logic.

---

# 153. The Golden Rule

Never implement:

```text
REST update logic
```

and separately:

```text
sync update logic
```

for the same business operation.

Instead:

```text
REST API ──────┐
               ▼
           Domain Command
               ▲
               │
Sync Engine ───┘
```

Both execute the same business code.

---

# 154. SQL Is an Implementation Detail

The deepest architecture should look like:

```text
Domain
   ↑
Application Services
   ↑
Aequora
   ↑
Transport/UI
```

with infrastructure plugged beneath:

```text
Domain/Application
       │
       ▼
Repository traits
       │
   ┌───┴────┐
   ▼        ▼
Stoolap  PostgreSQL
```

Not:

```text
Aequora → SQL statements → database
```

---

# 155. Suggested Dependencies

Core candidates:

```text
serde
postcard
ron
thiserror

tokio
futures

axum
tower
tower-http

rayon

uuid
smallvec
bytes

tracing

blake3

zstd
```

Database adapter:

```text
stoolap
```

Server adapter:

```text
sqlx
```

Testing:

```text
proptest
```

Fuzzing:

```text
cargo-fuzz/libFuzzer
```

Keep many of these behind adapter crates/features rather than putting everything into `aequora-core`.

---

# 156. Avoid Dependency Pollution

`aequora-core` ideally should not depend upon:

```text
Axum
SQLx
Stoolap
Dioxus
reqwest
OpenTelemetry
```

Those belong at the outer edges.

This keeps compilation and reuse manageable.

---

# 157. Minimum Viable Implementation

Do not implement everything above immediately.

## Phase 1

Build:

```text
IDs
OperationEnvelope
Postcard protocol
SyncRequest
SyncResponse
storage traits
outbox
server journal
cursor
HTTP/Axum transport
idempotent operation handling
basic version conflicts
Stoolap adapter
PostgreSQL adapter
```

That already produces a usable engine.

---

# 158. Phase 2

Add:

```text
batching
retry/backoff
dependency DAG
tombstones
bootstrap snapshots
partial synchronization
typed conflict policies
test simulator
```

---

# 159. Phase 3

Add:

```text
Rayon CPU parallelism
compression
adaptive batching
journal compaction
blob references
metrics
advanced observability
```

Measure before optimizing.

---

# 160. Phase 4

Add where genuinely required:

```text
field-level merge
CRDT support
streaming snapshots
QUIC transport
server push hints
advanced partitioning
multi-region awareness
```

Do not begin here.

---

# 161. Recommended First API

Keep the first release conceptually tiny.

Client:

```rust
let sync = ClientSyncEngine::new(
    local_store,
    transport,
    config,
);

sync.run_once().await?;
```

Server:

```rust
let server = SyncServer::new(
    authoritative_store,
    registry,
    authorizer,
);

let app = Router::new()
    .merge(aequora_axum::router(server));
```

Domain registration:

```rust
registry.register::<CreateStudent>(
    CreateStudentHandler::new(...)
);
```

---

# 162. Important Invariants

Document these prominently and test them permanently.

### I1

Every local optimistic mutation that requires synchronization has an atomic outbox entry.

### I2

Every authoritative mutation that clients must observe has an atomic journal event.

### I3

An `OperationId` can produce its logical effect at most once.

### I4

A client cursor advances only after corresponding changes are durably applied.

### I5

Entity versions monotonically increase.

### I6

The client never decides authoritative authorization.

### I7

Raw SQL never crosses the synchronization protocol.

### I8

Database-generated integer IDs are never required for distributed identity.

### I9

Deleted synchronized entities remain represented by tombstones until safely reclaimable.

### I10

Financial mutations use domain semantics rather than LWW state replacement.

### I11

Retried requests must be safe.

### I12

Network loss must never corrupt logical state.

### I13

Database adapters must preserve Aequora's transactional guarantees.

These invariants matter more than almost any individual crate choice.

---

# 163. Architecture Diagram

```text
┌──────────────────────────────── CLIENT ────────────────────────────────┐
│                                                                       │
│  ┌──────────┐      ┌─────────────────┐                                │
│  │  Dioxus  │─────►│ Domain Services │                                │
│  └──────────┘      └────────┬────────┘                                │
│                             │                                         │
│                             ▼                                         │
│                     ┌───────────────┐                                 │
│                     │ Local Store   │                                 │
│                     │   Adapter     │                                 │
│                     └───────┬───────┘                                 │
│                             │                                         │
│                  ┌──────────┴───────────┐                              │
│                  ▼                      ▼                              │
│              ERP Data               Outbox                            │
│                  │                      │                              │
│                  │                      ▼                              │
│                  │              ┌──────────────┐                       │
│                  │              │ Sync Client  │                       │
│                  │              └──────┬───────┘                       │
│                  │                     │                               │
│                  │               Postcard                             │
│                  │                     │                               │
└──────────────────┼─────────────────────┼───────────────────────────────┘
                   │                     │
                   │                   HTTPS
                   │                     │
┌──────────────────┼─────────────────────┼───────────────────────────────┐
│                  │                     ▼                SERVER         │
│                  │                  ┌──────┐                           │
│                  │                  │ Axum │                           │
│                  │                  └───┬──┘                           │
│                  │                      ▼                              │
│                  │            ┌───────────────────┐                    │
│                  │            │ Protocol Validator│                    │
│                  │            └─────────┬─────────┘                    │
│                  │                      ▼                              │
│                  │            ┌───────────────────┐                    │
│                  │            │   Authentication  │                    │
│                  │            └─────────┬─────────┘                    │
│                  │                      ▼                              │
│                  │            ┌───────────────────┐                    │
│                  │            │   Authorization   │                    │
│                  │            └─────────┬─────────┘                    │
│                  │                      ▼                              │
│                  │            ┌───────────────────┐                    │
│                  │            │ Domain Validation │                    │
│                  │            └─────────┬─────────┘                    │
│                  │                      ▼                              │
│                  │            ┌───────────────────┐                    │
│                  │            │Conflict Detection │                    │
│                  │            └─────────┬─────────┘                    │
│                  │                      ▼                              │
│                  │            ┌───────────────────┐                    │
│                  │            │ Dependency Planner│                    │
│                  │            └─────────┬─────────┘                    │
│                  │                      ▼                              │
│                  │            ┌───────────────────┐                    │
│                  │            │     Executor      │                    │
│                  │            └─────────┬─────────┘                    │
│                  │                      ▼                              │
│                  │              Store Adapter                         │
│                  │                      │                              │
│                  │               ┌──────┴───────┐                      │
│                  │               ▼              ▼                      │
│                  │          Domain State      Journal                  │
│                  │               │              │                      │
│                  │               └──────┬───────┘                      │
│                  │                      ▼                              │
│                  │                PostgreSQL                           │
│                  │                 / Neon                              │
│                  │                                                     │
└────────────────────────────────────────────────────────────────────────┘
```

---

# 164. Final Architecture Principle

The complete conceptual architecture can be reduced to:

```text
               LOCAL-FIRST CLIENT

Application
    ↓
Domain operation
    ↓
Local transaction
    ├── optimistic state
    └── durable outbox
    ↓
Aequora
    ↓
Postcard
    ↓
HTTPS

               TRUST BOUNDARY

Axum
    ↓
Authenticate
    ↓
Authorize
    ↓
Validate
    ↓
Detect conflicts
    ↓
Resolve dependencies
    ↓
Execute
    ↓
Authoritative transaction
    ├── domain state
    ├── entity version
    ├── operation ledger
    └── change journal
    ↓
Postcard response

               CLIENT RECONCILIATION

Verify
    ↓
Reconcile
    ↓
Atomic local apply
    ├── authoritative state
    ├── acknowledgements
    ├── conflicts
    └── cursor
    ↓
UI observes updated local database
```

The result is:

```text
database-independent
transport-independent
local-first
offline-capable
server-authoritative
idempotent
conflict-aware
transactional
typed
testable
reusable
```

---

# 165. Recommended Design for Your Projects

For your current projects I would standardize on:

```text
Client UI:
Dioxus

Client persistence:
Stoolap

Sync engine:
Aequora Sync

Wire format:
Postcard

Configuration:
RON

HTTP server:
Axum

Async runtime:
Tokio

CPU parallelism:
Rayon

Authoritative application layer:
Rust domain services

Server database interface:
repository/store traits

Current production adapter:
SQLx + PostgreSQL

Hosted PostgreSQL:
Neon

Observability:
tracing

Hashing:
BLAKE3

Optional compression:
zstd

Distributed IDs:
UUIDv7

Concurrency metadata:
entity version + HLC

Replication position:
server monotonic cursor
```

Most importantly:

```text
Stoolap                   PostgreSQL
   │                           │
   │ implementations          │ implementations
   ▼                           ▼
LocalStore trait        AuthoritativeStore trait
          \                /
           \              /
            ▼            ▼
             Aequora Sync
```

If you replace Stoolap five years from now, Aequora does not change.

If you replace PostgreSQL, Aequora does not change.

If you replace Axum, the core Aequora protocol does not change.

If you replace Dioxus, Aequora does not change.

That is the architectural boundary worth protecting.
