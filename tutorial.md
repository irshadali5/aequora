# Aequora Sync Tutorial

# Building a Correct, Database-Agnostic, Local-First Synchronization Engine in Rust

## 1. What This Tutorial Is For

This tutorial explains how to design and implement a production-grade synchronization architecture inspired by Aequora Sync.

### Repository development order

When developing Aequora itself, use the documents in this order:

```text
enterprise.md
database-interoperability.md
plug-and-play.md
ACID.md
then Parts 01–30 from aequora-roadmap.md
```

`aequora-roadmap.md` is navigation and ordering. The named and numbered architecture documents are
the specifications. Completion records under `docs/` connect those specifications to code and
tests. This tutorial explains how to perform the development; it does not override architecture.

The goal is not to teach one particular application.

Instead, the goal is to teach a reusable architecture that you can apply to:

```text
school ERP
finance/accounting
healthcare applications
inventory systems
field-service tools
messaging metadata
document collaboration
CRM
offline-first business apps
desktop/mobile enterprise software
```

The core idea is simple:

> Synchronize typed domain operations and authoritative state transitions, not database rows.

That one choice changes the entire architecture.

If you synchronize database rows directly, your protocol becomes coupled to:

```text
SQL layout
database engine
column names
migration history
storage quirks
```

If you synchronize domain operations, the database becomes only a persistence adapter.

That gives you:

```text
database independence
better conflict semantics
auditable history
clean offline behavior
easier migrations
stronger correctness
```

---

# 2. The Mental Model

Before writing code, build the correct mental model.

A local-first synchronized system has three different truths:

```text
1. Local provisional state
2. Pending user intent
3. Authoritative server state
```

These are related, but they are not the same thing.

A client may show a user change immediately.

Example:

```text
Student phone changed locally
```

but the server may later:

```text
accept it
reject it
merge it
supersede it
```

So the client should never confuse:

```text
"saved locally"
```

with:

```text
"committed authoritatively"
```

---

# 3. The Fundamental Flow

A correct write flow looks like this:

```text
User Action
    │
    ▼
Typed Local Domain Operation
    │
    ▼
LOCAL TRANSACTION
    ├── update local provisional state
    └── append durable outbox operation
    │
    ▼
Sync Client
    │
    ▼
Postcard over HTTPS
    │
    ▼
Axum Server
    │
    ▼
Authentication
    │
    ▼
Authorization
    │
    ▼
Protocol Validation
    │
    ▼
Domain Validation
    │
    ▼
Conflict / Dependency Planning
    │
    ▼
AUTHORITATIVE TRANSACTION
    ├── business mutation
    ├── entity version update
    ├── journal event
    ├── operation ledger
    └── required audit
    │
    ▼
Response
    │
    ▼
CLIENT RECONCILIATION TRANSACTION
    ├── apply authoritative state
    ├── update cursor
    ├── ACK/reject outbox
    └── record conflict if needed
```

This architecture is the backbone of everything else.

---

# 4. The Three Core Atomicity Boundaries

Do not try to make the entire client/server system one giant ACID transaction.

That is neither realistic nor necessary.

Instead, define three local atomic transactions.

## Transaction A — Client Local Write

```text
local business mutation
+
outbox insert
```

Must commit together.

If the app crashes after changing the local entity but before writing the outbox, the change can never reach the server.

That is a correctness failure.

---

## Transaction B — Server Authoritative Commit

```text
business mutation
+
entity version
+
journal
+
operation ledger
+
required audit
```

Must commit together.

If journal commits but business state does not, synchronization becomes corrupted.

If business state commits but ledger does not, retries may duplicate the effect.

---

## Transaction C — Client Reconciliation

```text
apply authoritative events
+
update local versions
+
ACK/reject outbox
+
advance cursor
```

Must commit together.

The cursor must never advance before local state is durable.

---

# 5. The Most Important Invariants

A synchronization engine should be designed from invariants, not APIs.

Start with these:

```text
I1  Local mutation + outbox are atomic.
I2  Authoritative mutation + journal + ledger are atomic.
I3  OperationId produces one logical authoritative effect.
I4  Cursor advances only after durable client apply.
I5  Entity versions are monotonic.
I6  Client never decides authorization.
I7  SQL never crosses the protocol boundary.
I8  Distributed IDs are independent from DB-generated integer IDs.
I9  Tombstones remain until safe to collect.
I10 Finance/accounting does not use generic LWW.
I11 Retries are safe.
I12 Network loss cannot corrupt state.
I13 Storage adapters must preserve required transaction semantics.
```

Write these down before implementation.

Tests should map directly to them.

---

# 6. Why Row Synchronization Is Usually the Wrong Abstraction

Suppose a student row is:

```text
id
name
phone
class_id
updated_at
```

If the client sends:

```text
UPDATE students SET phone = ...
```

or some row-level equivalent, the protocol now depends on:

```text
table name
column name
schema
storage engine
SQL semantics
```

Instead send:

```rust
SetStudentPhone {
    student_id,
    phone,
}
```

Now the server can choose:

```text
PostgreSQL
MySQL
Stoolap
SQLite
Redb
```

without changing the protocol.

The database only implements:

```text
read current state
validate domain rule
write authoritative state
append journal
```

---

# 7. Use Typed IDs Everywhere

Avoid generic strings and integers.

Define newtypes:

```rust
#[repr(transparent)]
pub struct OperationId(pub uuid::Uuid);

#[repr(transparent)]
pub struct EntityId(pub uuid::Uuid);

#[repr(transparent)]
pub struct TenantId(pub uuid::Uuid);

#[repr(transparent)]
pub struct DeviceId(pub uuid::Uuid);

#[repr(transparent)]
pub struct ActorId(pub uuid::Uuid);
```

Prefer UUIDv7 for distributed IDs.

Why?

Because newtypes prevent mistakes such as:

```rust
fn load_student(id: TenantId)
```

accidentally compiling where `StudentId` was intended.

Strong types move mistakes from runtime to compile time.

---

# 8. Separate Identity From Ordering

Do not confuse:

```text
OperationId
EventId
EntityId
```

with:

```text
Sequence
Version
Epoch
```

Identity answers:

```text
which thing?
```

Ordering answers:

```text
when relative to other things?
```

Use different types:

```rust
pub struct EntityVersion(pub u64);
pub struct JournalSequence(pub u64);
pub struct AuthorityEpoch(pub u64);
```

---

# 9. The Operation Envelope

A useful logical operation envelope might contain:

```rust
pub struct OperationEnvelope<P> {
    pub protocol_version: ProtocolVersion,
    pub schema_version: OperationSchemaVersion,

    pub operation_id: OperationId,
    pub tenant_id: TenantId,
    pub actor_id: ActorId,
    pub device_id: DeviceId,

    pub entity: EntityRef,
    pub base_version: Option<EntityVersion>,

    pub hlc: HybridLogicalClock,
    pub dependencies: Vec<OperationId>,

    pub payload: P,
}
```

But the client-provided:

```text
tenant
actor
device
```

must still be verified by the server.

They are metadata claims, not authorization proof.

---

# 10. Stable Operation Kinds

Never use enum declaration order as wire identity.

Instead:

```rust
pub struct OperationKind(pub u32);

pub const CREATE_STUDENT: OperationKind = OperationKind(1001);
pub const SET_STUDENT_PHONE: OperationKind = OperationKind(1002);
```

These IDs become long-lived ABI-like contracts.

Once published:

```text
never reuse them
```

even after removing an operation.

---

# 11. Why Postcard Works Well

For Rust-to-Rust internal sync:

```text
Postcard
```

is a strong default because it is:

```text
compact
fast
serde-based
no JSON parsing overhead
good for mobile
good for binary envelopes
```

Use RON for:

```text
configuration
registry files
developer-readable fixtures
debug artifacts
```

Use JSON where interoperability genuinely matters:

```text
public admin API
webhooks
external integrations
browser tooling
```

Do not use JSON everywhere by reflex.

---

# 12. Protocol Framing

Binary protocol should still be explicitly framed.

Conceptual format:

```text
magic/version
message kind
payload version
payload length
payload bytes
```

The payload length lets the decoder:

```text
bound memory
skip optional unknown extensions
reject oversized frames early
```

Never deserialize arbitrary-length vectors without limits.

---

# 13. Authentication and Authorization

A good server pipeline is:

```text
Decode
↓
Authenticate
↓
Authorize
↓
Validate
↓
Plan
↓
Execute
```

Do not let Axum routes contain business logic.

Thin Axum:

```rust
async fn exchange(
    State(app): State<AppState>,
    auth: AuthContext,
    body: Bytes,
) -> Result<Response, ApiError> {
    let request = app.protocol.decode(body)?;
    let result = app.sync_service.exchange(auth, request).await?;
    Ok(app.protocol.encode_response(result)?)
}
```

The route should not directly:

```text
run SQL
change entity
choose conflicts
```

---

# 14. Typestate Validation

Rust can make invalid processing states unrepresentable.

For example:

```rust
Incoming<O>
Authenticated<O>
Authorized<O>
Validated<O>
Executable<O>
```

Then only an `Executable<O>` can reach the executor.

This prevents code accidentally skipping:

```text
authentication
authorization
validation
```

---

# 15. Idempotency

Networks duplicate.

Clients retry.

Servers restart.

So your protocol should expect:

```text
at-least-once delivery
```

and design for:

```text
exactly-once logical authoritative effect
```

Use:

```text
OperationId
```

as idempotency identity.

Server ledger:

```text
operation_id
payload_digest
result
committed_sequence
```

If same OperationId arrives again with same payload:

```text
return previous result
```

If same OperationId arrives with different semantic payload:

```text
reject
```

---

# 16. The Operation Ledger

Logical schema:

```text
operation_id
tenant_id
kind
schema_version
payload_digest
status
committed_sequence
result_code
handler_version
```

This is not the same as the journal.

Ledger answers:

```text
what happened to this command?
```

Journal answers:

```text
what authoritative changes occurred?
```

---

# 17. The Journal

The journal is the authoritative synchronization history.

Logical fields:

```text
authority_epoch
sequence
event_id
tenant_id
entity_ref
entity_version
event_kind
event_schema_version
operation_id
payload
```

Clients pull:

```text
events after cursor
```

The cursor should be based on:

```text
server-issued monotonic sequence
```

not client timestamps.

---

# 18. Why Timestamps Are Not Cursors

Wall clocks can:

```text
drift
jump backward
change timezone
have low precision
collide
```

Use timestamps for:

```text
human time
diagnostics
ordering hints
```

Use server sequence for synchronization order.

---

# 19. Hybrid Logical Clock

HLC can be useful for:

```text
causality hints
offline event metadata
relative ordering
```

but it is not a replacement for the authoritative journal sequence.

Use both when needed.

---

# 20. The Client Outbox

Local operation flow:

```text
user mutates state
↓
local transaction
    entity updated
    outbox inserted
↓
UI returns "saved locally"
```

Outbox fields:

```text
OperationId
local sequence
kind
payload
base version
state
attempt count
next retry
ever_sent
```

---

# 21. Outbox States

Useful states:

```text
Pending
InFlight
Retryable
Blocked
Conflict
Committed
Rejected
Superseded
```

Do not overload one boolean like:

```text
synced = true/false
```

State machines are clearer and safer.

---

# 22. The Client Sync State Machine

A reusable state machine:

```text
Dormant
↓
Connecting
↓
Syncing
↓
Reconciling
↓
Idle
```

Failures:

```text
Backoff
```

Other transitions:

```text
NeedsBootstrap
AuthorityChanged
CompatibilityBlocked
```

Persist only what must survive process death.

---

# 23. Push + Pull Exchange

A practical first protocol:

```text
POST /sync/v1/exchange
```

Request:

```text
pending operations
current scope cursor
capabilities
```

Response:

```text
operation ACK/rejections
authoritative events
conflicts
next cursor
server hints
```

This reduces request count and works well over ordinary HTTPS.

---

# 24. Bootstrap Endpoint

Use:

```text
POST /sync/v1/bootstrap
```

for initial state or rebootstrap.

Bootstrap should use:

```text
snapshot at sequence N
+
journal N+1 onward
```

This eliminates race conditions.

---

# 25. Snapshots

A snapshot is:

```text
an immutable representation of authoritative state at a known journal boundary
```

Manifest:

```text
SnapshotId
AuthorityEpoch
ScopeId
BoundarySequence
SchemaVersion
ChunkList
RootDigest
```

Never publish until all chunks are durable and verified.

---

# 26. Streaming Bootstrap

Do not load the entire snapshot into memory.

Use:

```text
download chunk
verify digest
install chunk
checkpoint
continue
```

For mobile clients:

```text
one or few chunks at a time
```

Keep memory independent of snapshot total size.

---

# 27. Scope-Based Synchronization

A user rarely needs the entire tenant.

Examples:

```text
their school campus
their classes
current academic year
assigned patients
their warehouse
```

Define:

```text
ScopeId
ScopeVersion
ScopeGeneration
ProjectionSchemaVersion
```

Scope authorization is resolved on the server.

---

# 28. Scope Expansion

When a scope expands:

```text
seed new data
then continue journal
```

Do not assume old cursor already covers newly added membership.

---

# 29. Scope Contraction

When scope shrinks:

```text
EvictFromScope
```

is not the same as:

```text
DeleteEntity
```

An entity may belong to another active scope.

---

# 30. Conflicts

Do not implement one global conflict strategy.

Different domains need different semantics.

Examples:

```text
profile preference -> LWW may be okay
student phone -> optimistic version
attendance -> domain merge
bank ledger -> append only
invoice approval -> strong aggregate
```

---

# 31. Consistency Profiles

Useful profiles:

```text
ImmutableAppendOnly
OptimisticVersioned
Commutative
LastWriterWins
ManualConflict
StrongAggregate
ServerOnly
DeviceLocal
DerivedProjection
```

Bind them to operation/aggregate metadata.

---

# 32. Finance Rule

For finance/accounting:

```text
never use generic LWW for monetary history
```

Prefer:

```text
append-only journal
balanced entries
reversal/correction entries
strong aggregate validation
```

Resource pressure never changes this.

---

# 33. Aggregate Roots

Treat aggregate root as:

```text
conflict boundary
invariant boundary
authorization boundary
```

If updating multiple rows is one business decision, model them under one aggregate transaction.

Do not copy SQL table boundaries blindly into domain boundaries.

---

# 34. Dependencies Between Operations

Offline operations may depend on earlier operations.

Example:

```text
CreateStudent
↓
AssignStudentToClass
```

Represent dependency DAG.

Then:

```text
topological sort
```

before execution.

Complexity:

```text
O(N + E)
```

Keep dependency graph bounded.

---

# 35. Operation Compaction

Offline outbox may grow.

Some operations can be compacted.

Example:

```text
SetTheme("light")
SetTheme("dark")
SetTheme("blue")
```

can become:

```text
SetTheme("blue")
```

only if none were possibly sent and semantics allow.

---

# 36. Never Mutate Possibly-Sent Operations

Once an operation may have reached the server:

```text
OperationId + semantic payload
```

must become immutable.

Otherwise retry idempotency breaks.

---

# 37. Compaction Policies

Define per operation:

```text
Never
ReplaceLatest
Merge
CancelPairs
Custom
```

Finance:

```text
Never
```

---

# 38. Rebase

After server rejects stale base version:

```text
rebase pending intent
```

using operation-specific policy.

Do not implement universal mechanical rebase.

---

# 39. Tombstones

Deletes need tombstones because stale offline clients may return.

Keep:

```text
entity deleted
at sequence N
```

until all relevant clients are past safe retention boundary or forced to rebootstrap.

---

# 40. Journal Floor

Track minimum retained sequence.

If client cursor is older than floor:

```text
rebootstrap required
```

Do not pretend incremental history still exists.

---

# 41. Device Watermarks

Server tracks client progress:

```text
device
scope
epoch
last acknowledged sequence
```

This helps safe GC.

But never trust arbitrary client-reported high sequence without validated session/state.

---

# 42. Anti-Entropy

Cursor equality does not prove state equality.

A client may have:

```text
cursor N
```

but corrupted data.

Use canonical digests and Merkle-style partitioning.

---

# 43. Canonical Digest

Digest semantic representation, not raw DB pages.

Use:

```text
BLAKE3
```

over stable canonical encoding.

---

# 44. Merkle Partitioning

Hierarchy:

```text
scope
  partition
    subpartition
      entity digest
```

This lets repair localize divergence without transferring the whole dataset.

---

# 45. Repair Direction

In a server-authoritative system:

```text
server repairs client
```

The client never silently overwrites authority because its hash differs.

---

# 46. Local Multi-Process Coordination

Desktop apps may have:

```text
multiple windows
background process
CLI
worker
```

sharing one local store.

Only one active sync coordinator should own the store.

Use:

```text
durable lease
+
fencing token
```

---

# 47. Why Fencing Matters

Worker A gets token 4.

Lease expires.

Worker B gets token 5.

Worker A wakes up late.

Without fencing, A may still write.

With fencing:

```text
token 4 rejected
```

---

# 48. Adaptive Scheduler

Correctness says:

```text
what must happen
```

Scheduler says:

```text
when and how much
```

Work classes:

```text
Critical
Interactive
Normal
Bulk
Background
Maintenance
```

---

# 49. Scheduler Inputs

Consider:

```text
network
battery
charging
thermal
storage
foreground/background
server backpressure
```

But these only affect scheduling, never authoritative semantics.

---

# 50. Resource-Constrained Clients

On mobile:

```text
small batches
bounded memory
streamed snapshots
small caches
few CPU workers
durable checkpoints
```

A process may be killed at any time.

Therefore:

```text
no required state only in RAM
```

---

# 51. Dioxus Client Architecture

Do not mirror the entire database into reactive UI state.

Instead:

```text
local DB
↓
repository
↓
paginated query
↓
small view model
↓
Dioxus signal
```

Use:

```text
virtualized lists
pagination
IDs in state
```

---

# 52. Tokio vs Rayon

Use Tokio for:

```text
network
database I/O
timers
orchestration
```

Use Rayon or bounded blocking workers for:

```text
hashing
compression
CPU-heavy transforms
```

Never launch unbounded CPU work on Tokio executor.

---

# 53. Memory Rules

Prefer:

```text
Bytes
BytesMut
Vec
VecDeque
small bounded maps
```

Avoid unnecessary cloning.

Do not chase zero-copy through the whole domain model.

Use zero-copy primarily at:

```text
wire
snapshot
blob
```

boundaries.

---

# 54. Borrowed Decode

Postcard can decode borrowed data from stable buffer.

Pattern:

```text
read bytes
↓
borrowed decode
↓
validate
↓
convert to owned domain object
```

Do not keep complex borrows alive across async boundaries.

---

# 55. Database Abstraction

Avoid one giant database trait.

Use capabilities:

```text
OutboxStore
JournalStore
OperationLedgerStore
SnapshotStore
ScopeStore
AuditStore
GovernanceStore
```

---

# 56. Authoritative Transaction Trait

Conceptually:

```rust
pub trait AuthoritativeTransaction:
    BusinessWrite
    + JournalWrite
    + LedgerWrite
    + AuditWrite
{}
```

The PostgreSQL adapter may implement it around an SQLx transaction.

Core should never leak SQLx types into domain/protocol crates.

---

# 57. Client Transaction Trait

Conceptually:

```rust
pub trait LocalTransaction:
    LocalBusinessWrite
    + OutboxWrite
    + CursorWrite
    + ConflictWrite
{}
```

---

# 58. PostgreSQL Adapter

Good authoritative adapter features:

```text
transactions
unique OperationId
B-tree journal scans
strong constraints
PITR
replication
```

Use SQLx internally.

Do not let SQL statements appear in sync core.

---

# 59. Stoolap/SQLite/Redb Client Adapter

What matters is not brand.

The client DB must support the required capability set:

```text
atomic local mutation+outbox
durable cursor
bounded range scans
schema migration
```

If a DB cannot preserve those semantics, it should not be used in that role.

---

# 60. Metadata Schema

Aequora-like systems need canonical logical metadata.

Client:

```text
outbox
scope cursor
conflict
bootstrap
repair
scheduler
coordinator lease
```

Server:

```text
authority state
operation ledger
journal
scope registry
device watermarks
snapshot catalog
audit
jobs
governance
```

---

# 61. Logical Schema vs Physical Schema

The specification should define:

```text
identity
uniqueness
index requirements
transaction groups
retention
```

not exact SQL table syntax.

Then each adapter maps logical records to:

```text
tables
collections
keyspaces
```

---

# 62. Protocol Compatibility

Never assume every client upgrades together.

Separate:

```text
ProtocolVersion
OperationSchemaVersion
SnapshotSchemaVersion
ProfileVersion
HandlerVersion
CryptoPolicyVersion
```

---

# 63. Capability Negotiation

Client sends:

```text
supported protocol versions
compression
snapshot versions
crypto capabilities
```

Server chooses permitted common set.

Never automatically choose a weaker security capability if server requires stronger one.

---

# 64. Rolling Upgrade

Good rollout:

```text
ship support first
↓
run mixed fleet
↓
observe
↓
enable optional feature
↓
make feature required later
```

This avoids synchronized client/server upgrades.

---

# 65. Operation Upcasting

Old operation payload:

```text
v1
```

can be converted:

```text
v1 -> v2 -> v3
```

if transformation is deterministic and semantics are preserved.

Do not mutate possibly-sent operation payloads locally.

Server should preserve old retry support for legitimate retry horizon.

---

# 66. Compatibility Modes

Useful results:

```text
Full
ReadOnly
BootstrapOnly
UpgradeRequired
```

An old client may still safely read while being too old to write.

---

# 67. Registry Governance

Every durable ID should live in a canonical registry.

Use RON:

```text
registry/
  entities.ron
  operations.ron
  events.ron
  fields.ron
  capabilities.ron
  errors.ron
```

Generate typed Rust constants from it.

---

# 68. Registry Rules

Once published:

```text
never reuse ID
never silently change semantics
never rely on enum declaration order
```

Breaking change requires:

```text
new version
migration
upcaster
or explicit incompatibility
```

---

# 69. Background Jobs

Not every task belongs inside request/response flow.

Use durable jobs for:

```text
email
payments
webhooks
exports
snapshot building
retention
repair
document generation
```

---

# 70. Job Model

Logical:

```text
JobId
JobKind
state
payload
attempt count
next run time
lease
fencing token
```

---

# 71. Durable Job Rule

Required work must be persisted before worker execution.

An in-memory Tokio channel is an optimization, not the source of truth.

---

# 72. Side-Effect Intent

External effects should be represented inside the authoritative transaction:

```text
business state
+
journal
+
ledger
+
SideEffectIntent
COMMIT
```

Then a worker executes it.

---

# 73. Why

Never hold a DB transaction open while calling:

```text
payment provider
email provider
webhook
```

That creates fragile distributed transaction behavior.

---

# 74. External Idempotency

If provider supports an idempotency key:

```text
use it
```

Good source:

```text
OperationId
SideEffectIntentId
```

---

# 75. Ambiguous Provider Result

If request may have succeeded but response was lost:

```text
do not blindly retry
```

Use:

```text
provider lookup
idempotency retry
manual review
```

according to provider capability.

---

# 76. Provider Result Back Into Domain

Worker should not directly mutate business tables.

Instead:

```text
provider result
↓
new authoritative system operation
↓
domain handler
```

This preserves:

```text
journal
audit
validation
lineage
```

---

# 77. Deterministic Domain Execution

Domain handlers should avoid hidden nondeterminism.

Do not directly call inside handler:

```text
SystemTime::now()
random()
environment variables
HTTP APIs
```

Instead supply explicit execution inputs.

---

# 78. ExecutionContext

Example:

```rust
pub struct ExecutionContext {
    pub authoritative_time: Timestamp,
    pub deterministic_ids: IdSource,
    pub policy_version: PolicyVersion,
    pub correlation_id: CorrelationId,
}
```

---

# 79. Replay

If all meaningful inputs are captured, a domain operation can be replayed.

This is enormously useful for:

```text
debugging
audit
incident reproduction
regression testing
```

---

# 80. Provenance

Track:

```text
OperationId
EventId
CausationId
CorrelationId
```

Causation answers:

```text
what caused this?
```

Correlation answers:

```text
which larger workflow does this belong to?
```

---

# 81. Audit

Do not confuse:

```text
sync journal
operation ledger
business audit
logs
```

They serve different purposes.

Journal:

```text
replication
```

Ledger:

```text
idempotency/result
```

Audit:

```text
who/what/why
```

Logs:

```text
operations/debugging
```

---

# 82. Audit Atomicity

If an audit event is required for the business mutation:

```text
commit it in the same authoritative transaction
```

---

# 83. Governance

Define:

```text
retention classes
legal holds
erasure
pseudonymization
backup behavior
```

from the start for enterprise-grade systems.

---

# 84. Tombstone GC and Retention

Never delete sync metadata because:

```text
it is older than 30 days
```

alone.

You need safety boundaries:

```text
device watermarks
journal floor
inactive device policy
scope generation
```

---

# 85. Crypto

Use crypto for distinct purposes:

```text
transport TLS
artifact digest
artifact signature
encryption at rest
optional E2E
```

Do not conflate them.

---

# 86. BLAKE3

Good choice for:

```text
canonical digests
chunk hashes
integrity trees
payload digest
```

---

# 87. Ed25519

Good for:

```text
signed manifests
audit checkpoints
high-assurance device signatures
```

---

# 88. Key Management

Store:

```text
KeyId
purpose
algorithm
status
```

in metadata.

Private keys live in:

```text
KMS
HSM
Android Keystore
iOS Keychain
OS secure store
```

not ordinary DB tables.

---

# 89. Authority Epochs

A server-authoritative system needs to know when authority continuity changes.

Define:

```text
AuthorityId
AuthorityEpoch
AuthorityInstanceId
```

If continuity cannot be proven:

```text
increment epoch
```

---

# 90. Why Epochs Matter

After disaster restore, sequence 5000 in old timeline is not necessarily same as sequence 5000 in restored timeline.

Epoch binds cursor to timeline.

---

# 91. Fork Detection

If same:

```text
AuthorityEpoch
Sequence
```

has different checkpoint root:

```text
fork detected
```

Fail closed.

Do not auto-merge authoritative forks.

---

# 92. Multi-Region

A simple, safe global architecture:

```text
one authoritative writer region
+
regional read replicas
```

This reduces read latency without introducing multi-primary write conflicts.

---

# 93. Read Consistency

Expose:

```text
Eventual
AtLeast(sequence)
Session
Authority
```

Session watermark helps read-your-writes.

---

# 94. Backpressure

Overload should fail early.

Bound:

```text
HTTP requests
DB transactions
CPU tasks
snapshot workers
jobs
tenant queues
live connections
```

No unbounded queue.

---

# 95. Admission Control

Use hierarchical budgets:

```text
global
↓
tenant
↓
work class
↓
resource
```

Reject/defer before expensive work.

---

# 96. 429 vs 503

Use typed overload responses.

Example:

```text
429 tenant/client rate limited
503 server capacity/dependency unavailable
```

Include Retry-After where appropriate.

---

# 97. Performance Engineering

Optimize in this order:

```text
algorithms
I/O
DB query shape
allocations
copies
locks
cache
micro-optimizations
```

Do not start with unsafe code or custom allocators.

---

# 98. Bounded Pipelines

Every producer/consumer pipeline should have explicit capacity.

Example:

```text
network decode queue: 32
CPU hash workers: 2
snapshot downloads: 1
```

---

# 99. Observability

Track:

```text
sync latency
outbox size
journal lag
conflicts
job backlog
snapshot duration
repair count
consumer lag
```

Use low-cardinality metric labels.

Put IDs in logs/traces instead.

---

# 100. Diagnostics

Production systems need explainability.

Given:

```text
OperationId
```

you should be able to answer:

```text
Was it sent?
Was it accepted?
Which journal sequence?
Which audit event?
Did client reconcile?
Which job did it cause?
```

---

# 101. Incident Bundle

A good incident bundle contains:

```text
manifest
build/version inventory
relevant operation metadata
journal/ledger evidence
scope/cursor state
trace/log snippets
integrity digests
replay inputs
```

It should not contain:

```text
private keys
access tokens
entire DB dumps
```

---

# 102. Reproducible Incidents

Turn a production failure into:

```text
deterministic replay input
```

Then into:

```text
minimal regression test
```

This is one of the strongest engineering feedback loops you can build.

---

# 103. Legacy Migration

Do not require a big-bang rewrite.

Adopt incrementally:

```text
Observe
↓
Canonical Read
↓
CDC Follow
↓
Shadow
↓
Cutover Selected Aggregate
↓
Legacy API Facade
↓
Retire Legacy
```

---

# 104. One Write Owner

During migration, each aggregate should have exactly one authoritative write owner:

```text
Legacy
or
Aequora
```

Temporary migration state must be explicitly fenced.

---

# 105. Prefer CDC Over Permanent Dual-Write

Before cutover:

```text
legacy writes
Aequora follows
```

After cutover:

```text
Aequora writes
legacy reads/facade follows
```

Avoid permanent bidirectional row mirroring.

---

# 106. Multi-Consumer Change Feed

The authoritative journal can power:

```text
search
analytics
notifications
legacy projection
regional projection
external integrations
```

Each consumer gets its own:

```text
ConsumerId
cursor
failure policy
ordering policy
retention policy
```

---

# 107. Consumer Rule

A slow search index must not block:

```text
authoritative commits
client sync
analytics
notifications
```

Consumers are independent.

---

# 108. Consumer Delivery

Default:

```text
at least once
```

Use EventId for deduplication.

Projection cursor advances only after durable effect.

---

# 109. Search Example

```text
journal event
↓
SearchConsumer
↓
upsert document by EntityId/EntityVersion
↓
advance consumer cursor
```

---

# 110. Notification Example

Do not send email directly from journal consumer.

Instead:

```text
journal
↓
NotificationDecision
↓
SideEffectIntent
↓
Job
↓
Provider
```

---

# 111. Security Threat Model

Assume:

```text
client lies
payload is malicious
device compromised
network hostile
provider ambiguous
operator may make mistakes
```

Build server correctness independent from client honesty.

---

# 112. Security Boundaries

Protect:

```text
tenant isolation
authorization
request bounds
protocol downgrade
snapshot integrity
admin plane
secrets
providers
imports
webhooks
```

---

# 113. SSRF

Webhook URLs must be validated against:

```text
localhost
private RFC1918
cloud metadata
redirect-to-private
DNS rebinding
```

---

# 114. Cross-Tenant Tests

Every repository/service should have tests for:

```text
Tenant A cannot read/write Tenant B
```

even when A knows B's:

```text
EntityId
ScopeId
BlobRef
OperationId
```

---

# 115. Supply-Chain Security

Rust memory safety helps, but dependencies still matter.

Use:

```text
Cargo.lock
cargo-audit
cargo-deny
minimal dependencies
review build.rs/proc macros
SBOM where needed
```

---

# 116. Control Plane

Separate admin operations from normal sync.

Control plane handles:

```text
authority
jobs
snapshots
governance
compatibility
crypto
maintenance
diagnostics
```

---

# 117. Admin Commands

Use typed admin operations:

```text
PromoteAuthority
RetryJob
CreateLegalHold
RotateKey
BuildSnapshot
```

not raw SQL mutations.

---

# 118. High-Risk Admin Flow

Use:

```text
Plan
↓
Review
↓
Approve
↓
Execute
↓
Verify
↓
Audit
```

for destructive actions.

---

# 119. Break-Glass

Emergency access should be:

```text
short-lived
strongly authenticated
heavily audited
alerted
```

---

# 120. Certification

If you want database/provider independence, you need conformance tests.

Do not say:

```text
"supports any DB"
```

just because an adapter trait exists.

Require the adapter to prove:

```text
atomicity
cursor durability
idempotency
fencing
snapshot behavior
```

---

# 121. Conformance Tiers

Example:

```text
Experimental
Core Transactional
Full Sync
Enterprise
```

An adapter can truthfully say:

```text
Client Full
```

but:

```text
not Authoritative Server
```

---

# 122. Differential Testing

Run same workload against:

```text
reference model
Postgres adapter
Stoolap adapter
SQLite adapter
```

Then compare canonical final state.

This is one of the best ways to validate DB independence.

---

# 123. Recommended Workspace

A reusable Rust workspace might look like:

```text
aequora/
├── crates/
│   ├── aequora-types/
│   ├── aequora-protocol/
│   ├── aequora-domain/
│   ├── aequora-sync-core/
│   ├── aequora-client/
│   ├── aequora-server/
│   ├── aequora-metadata/
│   ├── aequora-storage/
│   ├── aequora-adapter-sdk/
│   ├── aequora-postgres/
│   ├── aequora-stoolap/
│   ├── aequora-jobs/
│   ├── aequora-audit/
│   ├── aequora-governance/
│   ├── aequora-crypto/
│   ├── aequora-diagnostics/
│   ├── aequora-registry-types/
│   ├── aequora-registry-generated/
│   └── aequora-conformance/
│
├── registry/
├── schemas/
├── fixtures/
├── docs/
└── examples/
```

Keep it modular-monolithic initially.

---

# 124. Dependency Direction

Good dependency flow:

```text
types
  ↓
protocol/domain contracts
  ↓
sync core
  ↓
client/server application services
  ↓
adapters/transports/platform integrations
```

Avoid:

```text
core -> Axum
core -> SQLx
core -> Dioxus
```

Instead:

```text
Axum -> core
SQLx adapter -> storage traits
Dioxus app -> client facade
```

---

# 125. What Belongs in Core

Core should know:

```text
OperationId
EntityVersion
ConflictDecision
ScopeCursor
Planner
State machines
Validation pipeline
```

Core should not know:

```text
SQL
HTTP headers
Android API
Dioxus components
Postgres connection pool
```

---

# 126. What Belongs in Application Domain

Application owns:

```text
Student
Invoice
Attendance
Payment
Operation payloads
Domain handlers
Authorization rules
Conflict policies
```

Aequora is infrastructure, not business logic.

---

# 127. What Belongs in Adapter

Adapter owns:

```text
physical schema
queries
transaction implementation
indexes
DB-specific tuning
```

but must preserve logical semantics.

---

# 128. What Belongs in UI

UI owns:

```text
interaction
display
offline status
conflict resolution screens
sync diagnostics
```

UI must not decide:

```text
authorization
authoritative conflict outcome
```

---

# 129. First Implementation Order

Do not implement all advanced features immediately.

Start in this order.

## Phase 1

```text
typed IDs
OperationEnvelope
EntityVersion
basic protocol
local outbox
server ledger
server journal
push/pull exchange
```

---

## Phase 2

```text
client reconciliation
cursor
retries
idempotency
optimistic conflicts
```

---

## Phase 3

```text
scope
snapshot/bootstrap
tombstones
rebootstrap
```

---

## Phase 4

```text
audit
jobs/side effects
scheduler
resource policies
```

---

## Phase 5

```text
anti-entropy
repair
multi-process fencing
compatibility governance
```

---

## Phase 6

```text
authority epochs
multi-region
governance
crypto
control plane
```

---

## Phase 7

```text
legacy migration
change feed
registry governance
certification
```

---

# 130. Minimum Viable Correct Sync

A good first production milestone should support:

```text
single authority
one client DB adapter
Postgres server adapter
typed operations
OperationId idempotency
outbox
journal
cursor
optimistic version conflicts
snapshot bootstrap
HTTPS/Postcard
```

Do not add:

```text
QUIC
multi-primary
CRDT framework
distributed consensus
plugin runtime
```

before this is solid.

---

# 131. Example Domain

Suppose we build a task app.

Entity:

```rust
pub struct Task {
    pub id: TaskId,
    pub version: EntityVersion,
    pub title: TaskTitle,
    pub completed: bool,
}
```

Operations:

```rust
CreateTask
RenameTask
CompleteTask
ReopenTask
```

---

# 132. Local Write Example

Pseudo-Rust:

```rust
async fn rename_task_local(
    tx: &mut impl LocalTransaction,
    task_id: TaskId,
    title: TaskTitle,
) -> Result<OperationId, Error> {
    let task = tx.load_task(task_id).await?;

    let op_id = OperationId::new_v7();

    tx.update_task_title(task_id, &title).await?;

    tx.enqueue_operation(NewOperation {
        operation_id: op_id,
        entity: task_id.into(),
        base_version: Some(task.version),
        kind: RENAME_TASK,
        payload: postcard::to_allocvec(&RenameTask { title })?,
    })
    .await?;

    tx.commit().await?;

    Ok(op_id)
}
```

Important:

```text
state change and outbox insert happen before commit
```

---

# 133. Server Handler Example

```rust
pub async fn rename_task(
    ctx: &ExecutionContext,
    tx: &mut impl AuthoritativeTransaction,
    op: Validated<RenameTask>,
) -> Result<ExecutionResult, DomainError> {
    let current = tx.load_task(op.entity_id()).await?;

    ensure_version(current.version, op.base_version())?;

    let next = current.rename(op.payload().title.clone())?;

    tx.save_task(&next).await?;

    let event = TaskRenamed {
        task_id: next.id,
        title: next.title.clone(),
        version: next.version,
    };

    tx.append_journal(event.into()).await?;
    tx.record_operation_result(op.operation_id(), Accepted).await?;

    Ok(ExecutionResult::accepted(next.version))
}
```

In real implementation:

```text
journal
ledger
audit
business mutation
```

must share one transaction.

---

# 134. Retry Example

Client sends OperationId `O1`.

Server commits.

Response is lost.

Client retries `O1`.

Server ledger sees:

```text
O1 already accepted
```

and returns previous result.

No duplicate mutation.

That is the design target.

---

# 135. Conflict Example

Client based on version 3.

Server currently version 5.

Operation profile says:

```text
OptimisticVersioned
```

Server returns:

```text
Conflict {
    authoritative_version: 5,
    conflict_kind: StaleBase,
}
```

Client may:

```text
show resolution UI
rebase operation
discard local intent
```

according to domain policy.

---

# 136. Snapshot Example

Server publishes snapshot:

```text
Scope: Tasks
Boundary: Sequence 10000
```

Client installs it atomically.

Then fetches:

```text
journal > 10000
```

No missing gap.

---

# 137. Search Consumer Example

```text
Journal seq 10005 TaskRenamed
↓
Search consumer receives EventId E5
↓
Upserts search doc version 12
↓
Commits projection
↓
Consumer cursor = 10005
```

If worker crashes after projection write:

```text
E5 redelivered
```

version/idempotency makes it safe.

---

# 138. Payment Example

Domain operation:

```text
PayInvoice
```

Server transaction:

```text
mark payment pending
journal
ledger
audit
create CapturePaymentIntent
COMMIT
```

Worker:

```text
call provider with idempotency key
```

Provider result:

```text
Captured
```

Worker submits:

```text
RecordPaymentCaptured
```

through domain handler.

Never directly patch invoice table from provider worker.

---

# 139. Common Mistakes

Avoid these.

## Mistake 1

```text
Use updated_at as sync cursor
```

Wrong.

---

## Mistake 2

```text
Mark local row synced=true
```

without operation identity/version.

Too weak.

---

## Mistake 3

```text
Retry HTTP request and hope duplicate is harmless
```

Use OperationId.

---

## Mistake 4

```text
Store business mutation then enqueue later
```

Breaks local durability.

---

## Mistake 5

```text
Call payment provider inside DB transaction
```

Creates distributed transaction failure.

---

## Mistake 6

```text
Let client decide permissions
```

Never.

---

## Mistake 7

```text
Use one conflict strategy everywhere
```

Domain semantics differ.

---

## Mistake 8

```text
Use one huge storage trait
```

Use capability traits.

---

## Mistake 9

```text
Make UI hold complete DB state
```

Use paginated queries.

---

## Mistake 10

```text
Add Redis/Kafka/QUIC immediately
```

Start with minimal reliable architecture.

---

# 140. Testing Strategy

A strong sync engine should have several layers.

```text
unit
property
state machine
model
crash
fault injection
cross-adapter
end-to-end
load
soak
```

---

# 141. Unit Tests

Test:

```text
version arithmetic
operation validation
conflict rules
cursor comparisons
registry parsing
```

---

# 142. Property Tests

Use `proptest` for invariants such as:

```text
duplicate operation never duplicates effect
cursor never decreases
compaction preserves semantics
```

---

# 143. Model Checking

Use abstract model:

```text
client state
server state
network messages
```

Actions:

```text
LocalMutate
Send
Drop
Duplicate
Commit
Crash
Restart
```

Use Stateright-style bounded exploration if practical.

---

# 144. Loom

Use Loom only for in-process concurrency primitives.

Do not use it as distributed system simulator.

---

# 145. Crash Tests

Inject crashes:

```text
after business write
after journal insert
after ledger insert
after commit
before response
```

Authoritative state must remain correct.

---

# 146. Client Crash Tests

Crash:

```text
after local mutation
before outbox
```

should roll back both.

Crash:

```text
after event apply
before cursor
```

should replay safely.

---

# 147. Cross-Adapter Tests

Same logical test suite should run against every adapter.

This makes "database agnostic" a tested statement instead of marketing.

---

# 148. Security Tests

Mandatory:

```text
cross-tenant
replay
payload substitution
downgrade
oversized input
SSRF
archive traversal
admin privilege
```

---

# 149. Load Tests

Test realistic profiles.

Examples:

```text
SchoolDayMorning
FeePaymentPeak
MassReconnect
LargeBootstrap
BulkImport
LowBandwidthMobile
```

---

# 150. Operational Readiness

Before production, verify:

```text
backup
restore
migration
drain
health
metrics
alerts
incident bundle
admin recovery
```

A system is not production-ready only because unit tests pass.

---

# 151. Deployment Profiles

Start simple.

## Standalone

```text
one process
embedded DB
local UI
```

## Single-Region Server

```text
Axum
Postgres/Neon
workers
object storage
```

## Enterprise

```text
multiple stateless server nodes
single writer authority
read replicas
private control plane
KMS
```

---

# 152. Avoid Mandatory Infrastructure

Do not require:

```text
Redis
Kafka
NATS
Kubernetes
```

for basic correctness.

These should be optional scaling integrations.

---

# 153. How to Reuse This Architecture in Another Project

When applying this to a new system, answer these questions first.

## Domain

```text
What are the aggregate roots?
What operations represent user intent?
Which operations may happen offline?
Which operations are append-only?
```

---

## Identity

```text
What IDs are globally stable?
Which sequences represent ordering?
```

---

## Authority

```text
Who is authoritative?
Can clients ever be authoritative?
What happens after disaster restore?
```

---

## Conflict

```text
Which operations reject stale state?
Which merge?
Which commute?
Which require manual resolution?
```

---

## Storage

```text
What must be atomic locally?
What must be atomic server-side?
Can selected DB preserve those guarantees?
```

---

## Sync Scope

```text
What subset does a device need?
How are scopes authorized?
```

---

## Retention

```text
How long can clients stay offline?
When can tombstones/journal entries be removed?
```

---

## Side Effects

```text
Which operations cause email/payment/webhook?
How are they made idempotent?
```

---

## Security

```text
What happens if client is malicious?
What is tenant boundary?
What operations need stronger admin approval?
```

---

# 154. Reusable Project Checklist

Use this when starting a new project.

```text
[ ] Define aggregate roots
[ ] Define typed IDs
[ ] Define OperationId/EventId
[ ] Define EntityVersion
[ ] Define domain operations
[ ] Assign consistency profiles
[ ] Define operation registry
[ ] Implement local atomic outbox
[ ] Implement authoritative operation ledger
[ ] Implement journal
[ ] Implement cursor
[ ] Implement client reconciliation
[ ] Implement retry/idempotency
[ ] Implement conflicts
[ ] Add scope model
[ ] Add snapshot bootstrap
[ ] Add tombstones and journal floor
[ ] Add audit
[ ] Add jobs/side effects
[ ] Add scheduler/backpressure
[ ] Add resource-constrained behavior
[ ] Add protocol compatibility
[ ] Add governance/security
[ ] Add diagnostics
[ ] Add conformance suite
```

---

# 155. Suggested Learning Path

If you are learning while building, study in this order:

```text
1. Rust ownership/types
2. async Rust + Tokio
3. transactions and isolation
4. domain modeling
5. idempotency
6. distributed system failure modes
7. event/journal architecture
8. synchronization
9. security
10. operations/testing
```

Do not start with CRDT research before understanding:

```text
transactions
idempotency
versions
authoritative sequencing
```

Most business sync systems do not require full CRDTs.

---

# 156. Rust Skills That Matter Most

Focus on:

```text
newtypes
enums/state machines
traits
associated types
error enums
typestate
serde
async traits
transaction lifetimes
Arc ownership
bounded channels
```

Rust is especially useful here because the type system can encode:

```text
validated vs unvalidated
authenticated vs unauthenticated
current vs stale state
different ID classes
```

---

# 157. Recommended Crate Families

Typical dependencies:

```text
tokio
axum
serde
postcard
ron
uuid
thiserror
bytes
blake3
tracing
sqlx
proptest
```

Optional:

```text
zstd
rayon
stateright
loom
ed25519-dalek
zeroize
```

Choose permissive and well-maintained crates where practical.

---

# 158. Error Design

Do not return strings everywhere.

Use typed errors:

```rust
pub enum SyncError {
    Unauthorized,
    Conflict(ConflictInfo),
    UnsupportedProtocol,
    CursorExpired,
    AuthorityChanged,
    Storage(StorageError),
}
```

Then edge adapters map to:

```text
HTTP
CLI text
UI message
```

---

# 159. Public API Design

Expose high-level facades.

Client:

```rust
client.mutate(...)
client.sync_now()
client.status()
```

Server:

```rust
server.register_domain(...)
server.mount_axum(...)
```

Adapter implementers use lower-level traits.

Application developers should not need to manipulate journal rows directly.

---

# 160. Keep Sharp Tools Internal

Operations such as:

```text
set cursor
insert ledger row
change epoch
```

should not be ordinary public APIs.

Expose safe higher-level operations.

---

# 161. Documentation Strategy

For every important subsystem, document:

```text
purpose
invariants
state machine
transaction boundaries
failure cases
test matrix
```

Architecture documentation should explain:

```text
why
```

not only code structure.

---

# 162. ADRs

Write architecture decision records for choices such as:

```text
server-authoritative model
Postcard protocol
OperationId idempotency
single-writer authority
no multi-primary
no universal LWW
```

This prevents future developers from accidentally undoing important design assumptions.

---

# 163. Version Every Durable Contract

Anything that can survive a process restart should be treated as versioned.

Examples:

```text
operation payload
snapshot manifest
metadata schema
replay bundle
job payload
consumer projection
```

---

# 164. Do Not Hide Compatibility in Serde Tricks

Be explicit.

If semantics change:

```text
bump version
```

Do not rely on:

```text
serde(default)
```

to make a breaking semantic change appear compatible.

---

# 165. Start With One Authority

Do not introduce multi-writer authority until there is a proven business requirement.

Single authority simplifies:

```text
ordering
conflicts
audit
recovery
finance
```

You can still scale reads globally.

---

# 166. Start With HTTPS

HTTPS request/response is:

```text
simple
observable
deployable
firewall-friendly
```

Future QUIC can implement the same transport trait.

Do not tie semantics to transport.

---

# 167. Transport Trait

Conceptually:

```rust
pub trait SyncTransport {
    async fn exchange(
        &self,
        request: ExchangeRequest,
    ) -> Result<ExchangeResponse, TransportError>;
}
```

Then implementations can be:

```text
HTTP
QUIC
local IPC
test transport
```

---

# 168. Blob Separation

Do not put large binaries inside operation payload.

Use:

```text
content-addressed blob store
```

Operation carries:

```text
BlobRef
```

Blob subsystem handles:

```text
chunking
resume
hash
storage
```

---

# 169. Blob Digest

Use:

```text
BLAKE3
```

as content identity if appropriate.

---

# 170. Offline Blob Flow

Client:

```text
store attachment locally
create BlobRef intent
enqueue operation
upload blob resumably
```

Domain commit rules determine whether operation requires blob already present.

---

# 171. Presence and Live Hints

Presence is ephemeral.

Do not persist it as authoritative business state.

Live hint:

```text
"something changed"
```

Client still pulls journal.

---

# 172. Push Notification

FCM/APNs message should carry:

```text
wake/sync hint
```

not authoritative entity payload.

Push can be delayed or dropped.

---

# 173. Android Integration

Keep core Rust.

Use thin platform bridge for:

```text
WorkManager
ConnectivityManager
Battery
Keystore
notifications
```

Do not duplicate sync engine in Kotlin.

---

# 174. iOS Integration

Same idea:

```text
Rust core
+
thin Swift/ObjC bridge
```

for:

```text
BGTaskScheduler
Keychain
push
lifecycle
```

---

# 175. Desktop Integration

Desktop can run:

```text
Dioxus UI
sync coordinator
embedded DB
```

with optional background process.

Use local lease/fencing if multiple processes share store.

---

# 176. Background Work on Mobile

Assume OS can kill you at any `await`.

Therefore every meaningful workflow is:

```text
claim durable work
↓
perform bounded unit
↓
checkpoint
↓
yield
```

---

# 177. Storage Pressure

Eviction priority:

```text
temporary files
UI cache
derived indexes
completed snapshot chunks
optional scope cache
```

Never silently delete:

```text
unsynced outbox
required local business data
```

---

# 178. Critical Storage

If the device cannot guarantee a durable local transaction:

```text
block new mutation
```

and tell the UI.

Do not claim a save succeeded.

---

# 179. Data Budget

On mobile, distinguish:

```text
small interactive metadata
large snapshot/blob
```

Allow product policy to defer large data on metered/roaming networks.

---

# 180. Client Status UX

Good statuses:

```text
Saved locally
Sync pending
Syncing
Waiting for network
Waiting for Wi-Fi
Storage low
Conflict needs attention
Server confirmation pending
Up to date
```

Avoid showing:

```text
Synced
```

before authoritative confirmation.

---

# 181. Legacy API Facade

A migration-friendly pattern:

```text
old JSON REST API
↓
translation
↓
typed Aequora operation
↓
same domain handler
```

This lets old clients and new sync clients coexist.

---

# 182. Cutover Discipline

Before moving authority from legacy to Aequora:

```text
fence old writers
capture final source position
apply remaining CDC
verify canonical state
switch ownership
```

"Developers promise not to write there" is not fencing.

---

# 183. Change Feed Consumers

Each consumer must declare:

```text
ordering
rebuildability
retention
failure policy
```

Search index:

```text
RebuildIfBehind
```

Compliance archive:

```text
PinJournal
```

Notification:

```text
materialize side-effect jobs
```

---

# 184. Registry Change Workflow

New operation:

```text
reserve OperationKind
↓
add RON registry entry
↓
define payload
↓
define schema
↓
assign profile
↓
assign permission
↓
add handler
↓
add golden fixture
↓
CI compatibility check
```

---

# 185. Certification Workflow

New DB adapter:

```text
implement capability traits
↓
declare capability manifest
↓
run conformance suite
↓
fault injection
↓
differential tests
↓
publish certification report
```

---

# 186. How to Judge a Storage Engine

Ask:

```text
Can it atomically update local state + outbox?
Can it support ordered journal scan?
Can it enforce unique OperationId?
Can it implement fencing?
Can it survive process crash correctly?
```

If not:

```text
do not use it for that role
```

The brand/name is secondary.

---

# 187. How to Judge a Sync Architecture

Ask:

```text
What happens if response is lost after commit?
What happens if client is offline for 90 days?
What happens if server restores old backup?
What happens if device storage fills?
What happens if two local processes sync same DB?
What happens if client submits same OperationId with changed payload?
What happens if search consumer is down for a week?
```

If architecture has no precise answer, it is not finished.

---

# 188. Strongest Reusable Principle

The most reusable principle from this architecture is:

> Make failure states explicit and durable.

Examples:

```text
OutboxState
JobState
BootstrapState
CompatibilityMode
AuthorityEpoch
ConsumerCursor
```

Avoid implicit behavior hidden in:

```text
logs
timers
in-memory flags
```

---

# 189. Second Strongest Principle

> Treat every boundary as a contract.

Boundaries include:

```text
protocol
database adapter
snapshot artifact
job payload
registry ID
admin action
legacy bridge
consumer feed
```

Contracts should be:

```text
typed
versioned
bounded
tested
```

---

# 190. Third Strongest Principle

> Preserve correctness first; adapt performance second.

On a slow device:

```text
smaller batch
```

not:

```text
skip validation
```

Under server overload:

```text
reject early
```

not:

```text
accept unbounded queue
```

During migration:

```text
one write owner
```

not:

```text
hope dual-write stays consistent
```

---

# 191. A Practical Build Sequence for a New Project

If you start tomorrow, implement this exact order:

```text
1. Domain types/newtypes
2. Operation registry
3. Local transaction + outbox
4. Server operation ledger
5. Journal
6. Exchange protocol
7. Reconciliation
8. Retry/idempotency tests
9. Optimistic conflicts
10. Scope
11. Snapshot bootstrap
12. Tombstone/journal floor
13. Audit
14. Jobs/side effects
15. Scheduler/resource limits
16. Compatibility
17. Anti-entropy
18. Diagnostics
19. Security hardening
20. Governance/admin/conformance
```

---

# 192. Final Architecture

```text
                             DOMAIN
                     typed operations/aggregates
                               │
                               ▼
                        CLIENT LOCAL STORE
                   provisional state + outbox
                               │
                               ▼
                          SYNC CLIENT
                               │
                        Postcard / HTTPS
                               │
                               ▼
                            AXUM EDGE
                               │
                               ▼
                  AUTH / AUTHZ / VALIDATION
                               │
                               ▼
                       DOMAIN EXECUTION
                               │
                               ▼
                    AUTHORITATIVE TRANSACTION
        ┌──────────────────────┼──────────────────────┐
        ▼                      ▼                      ▼
   Business State          Journal               Ledger/Audit
        │                      │                      │
        └──────────────────────┼──────────────────────┘
                               ▼
                         CLIENT RECONCILE
                               │
                               ▼
                          Local Cursor

Derived systems:

Journal
 ├── Search Consumer
 ├── Analytics Consumer
 ├── Notification Jobs
 ├── Legacy Projection
 └── Regional Projection

Operational systems:

Control Plane
 ├── Authority
 ├── Jobs
 ├── Snapshots
 ├── Governance
 ├── Compatibility
 ├── Crypto
 └── Diagnostics
```

---

# 193. Closing Principle

A robust local-first sync engine is not mainly about moving bytes between two databases.

It is about preserving meaning under:

```text
offline work
retries
crashes
concurrency
schema evolution
device loss
server failover
migration
overload
external-provider ambiguity
```

If you remember only one sentence, remember this:

> **Synchronize durable typed intent against one clearly defined authority, and make every failure/retry/version boundary explicit.**

That principle is reusable across almost any serious offline-first business application.
