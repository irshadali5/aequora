# Aequora Sync — Universal Database Interoperability Architecture

## Database-Agnostic Synchronization Across Same or Different Databases on Client and Server

> This document defines the universal database interoperability layer for **Aequora Sync**.
>
> It extends the existing Aequora architecture, ACID design, enterprise deployment model, and plug-and-play SDK.
>
> The goal is to support:
>
> - the **same database on both sides**;
> - **different databases on client and server**;
> - SQL ↔ SQL;
> - SQL ↔ key-value;
> - SQL ↔ document;
> - embedded DB ↔ server DB;
> - server DB ↔ server DB;
> - local-only DB ↔ cloud DB;
> - custom or proprietary storage engines through adapters.
>
> The central rule is:
>
> **Aequora synchronizes canonical domain operations and authoritative state transitions, never database-specific replication formats as its primary contract.**

---

# 1. Primary Goal

Aequora should support combinations such as:

```text
Stoolap   ↔ PostgreSQL
SQLite    ↔ PostgreSQL
SQLite    ↔ MySQL
Stoolap   ↔ SQLite
PostgreSQL ↔ PostgreSQL
MySQL     ↔ PostgreSQL
Redb      ↔ PostgreSQL
RocksDB-like KV ↔ PostgreSQL
Document DB ↔ SQL DB
Custom embedded DB ↔ custom server DB
```

without rewriting the synchronization engine for each pair.

The implementation must avoid:

```text
N client databases × M server databases
=
N × M custom sync engines
```

Instead:

```text
N client adapters
+
M server adapters
+
one canonical Aequora core
```

---

# 2. The Core Architectural Insight

The system must never define synchronization as:

```text
Database A rows
    ↓
convert
    ↓
Database B rows
```

Instead:

```text
Database A
    ↓
Adapter
    ↓
Canonical Aequora model
    ↓
Protocol
    ↓
Canonical Aequora model
    ↓
Adapter
    ↓
Database B
```

This creates:

```text
O(N + M)
```

adapter complexity instead of:

```text
O(N × M)
```

pairwise integrations.

---

# 3. Universal Architecture

```text
CLIENT STORAGE
      │
      ▼
Client Storage Adapter
      │
      ▼
Canonical Local Mutation Model
      │
      ▼
Aequora Client Engine
      │
      ▼
Postcard Protocol
      │
      ▼
Aequora Server Engine
      │
      ▼
Canonical Authoritative Mutation Model
      │
      ▼
Server Storage Adapter
      │
      ▼
SERVER STORAGE
```

The two databases never need to understand each other.

---

# 4. Same Database on Both Sides

Example:

```text
SQLite client
SQLite server
```

or:

```text
PostgreSQL client
PostgreSQL server
```

Aequora must still use the same abstraction.

Do **not** bypass the canonical model merely because both sides happen to use the same engine.

Reasons:

```text
schema may differ
permissions may differ
server remains authoritative
future DB migration remains possible
business validation remains required
```

---

# 5. Different Database on Both Sides

Example:

```text
Stoolap client
PostgreSQL server
```

Client may store:

```text
student
attendance
invoice
```

in one schema.

Server may normalize them very differently.

Aequora does not care.

It synchronizes:

```text
CreateStudent
UpdateAttendance
PostPayment
```

and authoritative events.

---

# 6. Three Synchronization Modes

Aequora should support three database integration modes.

## Mode A — Domain Operation Sync

Preferred default.

```text
application command
↓
Aequora operation
↓
server validation/execution
```

Best for:

```text
ERP
finance
workflows
business systems
multi-user systems
```

## Mode B — Canonical Record Sync

Useful where domain semantics are simpler.

```text
canonical entity record
↓
field/version metadata
↓
server reconciliation
```

Best for:

```text
notes
preferences
simple catalogs
metadata
```

## Mode C — Change Capture Bridge

Advanced adapter option.

```text
database change
↓
adapter CDC/outbox
↓
canonical Aequora operation/event
```

Useful for:

```text
legacy applications
incremental adoption
existing databases
```

Aequora core should treat all three as producing canonical operations/events.

---

# 7. Preferred Mode

Use:

```text
Domain Operation Sync
```

whenever correctness depends on business meaning.

Example:

Bad:

```text
balance = 5000
```

Better:

```text
PostPayment(2000)
```

---

# 8. Canonical Data Plane

Aequora needs a database-independent data plane.

Core types:

```text
EntityRef
OperationEnvelope
AuthoritativeChange
EntityVersion
Sequence
Cursor
Conflict
SnapshotRecord
Tombstone
```

No database-specific types are allowed here.

---

# 9. Canonical Value Model

For generic record-oriented adapters, define a limited canonical value model.

Conceptually:

```rust
pub enum Value {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    Decimal(DecimalValue),
    String(String),
    Bytes(Bytes),
    Uuid(Uuid),
    Timestamp(CanonicalTimestamp),
    Date(CanonicalDate),
    Time(CanonicalTime),
    Array(Vec<Value>),
    Object(BTreeMap<FieldId, Value>),
}
```

Use only where generic record mapping is required.

Domain operations may continue using strongly typed Rust structs directly.

---

# 10. Why a Canonical Value Model Is Needed

Different DBs disagree on:

```text
integer sizes
decimal representation
timestamps
JSON
binary values
UUID
booleans
arrays
null semantics
```

A canonical layer prevents each adapter from needing to understand every other adapter.

---

# 11. Avoid Universal Lowest-Common-Denominator Design

Do not force all projects into a weak generic record model.

Aequora should support:

```text
strong typed domain payloads
```

as primary mode.

The canonical Value model is mainly for:

```text
generic adapters
record sync
schema tooling
snapshots
migration utilities
```

---

# 12. Database Capability Model

Every adapter declares capabilities.

Example:

```rust
pub struct DbCapabilities {
    pub transactions: TransactionCapabilities,
    pub isolation: IsolationCapabilities,
    pub cdc: CdcCapabilities,
    pub snapshots: SnapshotCapabilities,
    pub types: TypeCapabilities,
    pub constraints: ConstraintCapabilities,
}
```

---

# 13. Transaction Capabilities

Examples:

```text
multi-statement transactions
savepoints
serializable isolation
repeatable-read
read committed
atomic batch
durable commit
```

Aequora validates that required semantics are available.

---

# 14. CDC Capabilities

Adapter may support:

```text
native WAL/binlog CDC
trigger-based CDC
transactional outbox
polling
none
```

Core should not require native CDC.

---

# 15. Snapshot Capabilities

Examples:

```text
consistent snapshot transaction
MVCC snapshot
exported snapshot
full scan only
generation-swap support
```

---

# 16. Type Capabilities

Examples:

```text
UUID native
decimal native
JSON native
binary blobs
arrays
date/time
unsigned integers
```

The adapter maps these into canonical types.

---

# 17. Adapter Roles

A database adapter may implement:

```text
LocalWritableAdapter
AuthoritativeWritableAdapter
ReadOnlySourceAdapter
ReplicaSinkAdapter
SnapshotSource
SnapshotSink
ChangeCaptureSource
```

One adapter may support multiple roles.

---

# 18. Client Adapter Contract

A writable client adapter must support:

```text
local transaction
outbox
cursor
conflict store
incoming authoritative apply
device metadata
bootstrap install
```

---

# 19. Server Adapter Contract

An authoritative server adapter must support:

```text
authoritative transaction
operation ledger
journal
entity versioning
snapshot reads
scope filtering
deduplication
```

---

# 20. Symmetric Database Pair

When both sides use same DB engine:

```text
PostgreSQL ↔ PostgreSQL
```

there should still be two logical adapter roles:

```text
PostgresLocalAdapter
PostgresAuthoritativeAdapter
```

or one implementation providing both traits.

The semantics differ even if the engine is identical.

---

# 21. Role-Based Adapter Design

Prefer:

```rust
impl LocalSyncStore for PostgresAdapter
impl AuthoritativeSyncStore for PostgresAdapter
```

rather than separate codebases if implementation can be shared safely.

---

# 22. Same Schema Is Not Required

Aequora must support:

```text
Client:
students(id, name, phone)

Server:
person
student_profile
contact_point
```

The domain operation remains:

```text
UpdateStudentPhone
```

The server repository decides how to persist it.

---

# 23. Schema Mapping Layer

For generic record sync, define:

```text
SchemaMap
EntityMap
FieldMap
TypeMap
KeyMap
```

---

# 24. Entity Mapping

Example:

```ron
EntityMap(
    canonical: "student",
    local: "students",
    server: "student_profile",
)
```

This is optional.

Domain-operation sync usually does not need it.

---

# 25. Field Mapping

Example:

```ron
FieldMap(
    canonical: "phone",
    local: "phone_number",
    server: "mobile",
)
```

---

# 26. Type Mapping

Example:

```text
client INTEGER
→ canonical I64
→ server BIGINT
```

or:

```text
client TEXT UUID
→ canonical UUID
→ server UUID
```

---

# 27. Explicit Lossy Conversion Rules

If a conversion can lose information, it must be explicit.

Example:

```text
Decimal(38,10)
→ FLOAT64
```

should never happen silently.

Adapter config should reject or require a declared policy.

---

# 28. Numeric Safety

Canonical numeric rules:

```text
no silent narrowing
no silent sign changes
no float conversion for financial decimal
```

Use explicit checked conversions.

---

# 29. Decimal Canonical Form

Represent decimal as:

```rust
pub struct DecimalValue {
    pub coefficient: i128,
    pub scale: u32,
}
```

or another deterministic exact representation.

Avoid generic floating point for money.

---

# 30. Timestamp Canonicalization

Use one canonical representation.

Example:

```text
UTC epoch nanoseconds
+
optional precision metadata
```

Do not assume all databases preserve timezone semantics equally.

---

# 31. Timezone Policy

Domain timestamps should generally be normalized to UTC.

Business-local dates such as:

```text
attendance date
school holiday date
```

should be modeled as dates, not UTC timestamps.

---

# 32. UUID Mapping

Preferred canonical distributed identity:

```text
UUIDv7
```

Adapters may store it as:

```text
native UUID
16-byte binary
text
```

but canonical value remains UUID.

---

# 33. Key Mapping

Never rely on server auto-increment IDs as synchronization identity.

Use stable sync IDs.

Internal DB-specific keys may coexist.

Example:

```text
server row id: BIGSERIAL
sync_id: UUID
```

---

# 34. Composite Keys

Adapters should support canonical composite identity where unavoidable.

Prefer one stable sync UUID for most entities.

Composite natural keys can remain business constraints.

---

# 35. Null Semantics

Databases differ on:

```text
missing
null
empty string
undefined
```

Canonical model must distinguish intentionally where needed.

Do not normalize them silently.

---

# 36. JSON/Document Mapping

Document DBs can map nested objects directly into canonical `Object`.

SQL DBs may map them into:

```text
JSON/JSONB
normalized tables
serialized Postcard
```

depending on application repository.

Aequora does not require one representation.

---

# 37. Key-Value Database Mapping

For KV databases:

```text
key
→ EntityRef

value
→ canonical payload / domain snapshot
```

Version metadata may be:

```text
inside value
separate key
metadata CF/tree
```

adapter-specific.

---

# 38. Graph Database Mapping

Graph stores may expose:

```text
node = entity
edge = relationship
```

Aequora should not try to synchronize arbitrary graph primitives generically in v1.

Use domain operations:

```text
AddMembership
RemoveDependency
ConnectEntities
```

for correctness.

---

# 39. Database Pair Matrix

Aequora core should support the following categories.

```text
SQL ↔ SQL
SQL ↔ KV
SQL ↔ Document
KV ↔ SQL
Document ↔ SQL
KV ↔ KV
Document ↔ Document
```

Support means:

```text
adapters can map to canonical operations/state
```

not that every pair gets specialized code.

---

# 40. SQL ↔ SQL Architecture

Example:

```text
SQLite client
PostgreSQL server
```

Client:

```text
SQLite Tx
+ outbox
```

Server:

```text
PostgreSQL Tx
+ journal
+ operation ledger
```

No row-level SQL translation is necessary for domain operations.

---

# 41. SQL ↔ KV Architecture

Example:

```text
Redb client
PostgreSQL server
```

Client adapter stores:

```text
entity state
outbox
cursor
metadata
```

in KV namespaces.

The server sees exactly the same Aequora operations as with SQLite.

---

# 42. Document ↔ SQL Architecture

Example:

```text
document local DB
PostgreSQL server
```

Client may store one whole entity as a document.

Server may normalize fields.

Aequora operation defines intent.

---

# 43. SQL ↔ Document Server

Example:

```text
SQLite client
Mongo-like server adapter
```

If authoritative DB supports necessary transactions/versioning, adapter can implement server traits.

If it lacks required atomic guarantees, it cannot be certified as a full authoritative adapter.

---

# 44. Capability-Based Certification

Adapters should be classified.

```text
Tier A — Full Production
Tier B — Production With Limitations
Tier C — Experimental
Tier D — Read-Only/Import
```

---

# 45. Tier A Requirements

Must satisfy:

```text
durable transactions
atomic metadata coupling
idempotency storage
consistent cursor/journal semantics
safe concurrent updates
snapshot support or valid bootstrap equivalent
compliance test suite
```

---

# 46. Tier B Example

A DB may lack:

```text
serializable isolation
```

but still work safely using:

```text
optimistic CAS
constraints
application-level locking
```

Document limitations explicitly.

---

# 47. Tier C

Experimental adapters:

```text
best-effort storage
limited transactions
unverified crash semantics
```

must not be advertised as enterprise-ready.

---

# 48. Adapter Manifest

Each adapter publishes:

```ron
AdapterManifest(
    name: "stoolap",
    roles: [LocalWritable],
    tier: FullProduction,
    capabilities: (...),
    tested_aquora_version: "...",
)
```

---

# 49. Startup Capability Negotiation

Aequora runtime checks:

```text
application requirements
vs
adapter capabilities
```

Example:

```text
application requires atomic outbox
adapter lacks transactions
```

Result:

```text
startup failure
```

not silent degradation.

---

# 50. Database-Specific Optimizations

Adapters may optimize internally using:

```text
UPSERT
RETURNING
native JSON
batch insert
prepared statements
WAL
MVCC snapshots
```

Core semantics remain unchanged.

---

# 51. Native CDC Integration

Some server DBs support:

```text
WAL
binlog
change streams
```

Aequora may use them as an optimization or bridge.

But native CDC is not the authoritative protocol.

---

# 52. Why Native CDC Is Not Core

Database CDC reflects:

```text
physical/logical DB mutations
```

but often lacks:

```text
business intent
authorization context
OperationId
conflict semantics
domain transaction meaning
```

Therefore Aequora journal remains canonical.

---

# 53. CDC Bridge Use Case

Legacy system already writes DB directly.

Adapter can:

```text
native CDC
↓
translate known changes
↓
canonical AuthoritativeChange
↓
Aequora journal
```

This enables incremental adoption.

---

# 54. Transactional Outbox Preferred for New Apps

For new applications:

```text
domain mutation
+
Aequora journal
```

in one transaction is more reliable and semantically rich than post-hoc CDC translation.

---

# 55. Legacy Integration Mode

Aequora should support:

```text
LegacyCaptureAdapter
```

with caveats.

Responsibilities:

```text
detect DB changes
assign operation/event IDs
derive entity version
translate fields
produce journal
```

This mode is inherently weaker than domain operation integration.

---

# 56. Polling Adapter

For a database lacking CDC:

```text
updated_at/version scan
```

may be used for import/bridge scenarios.

It should not be the preferred production correctness mechanism.

---

# 57. Full Table Diff

Avoid periodic full-table diff as normal sync.

Use only for:

```text
migration
verification
repair
initial import
```

---

# 58. Snapshot Interoperability

Snapshots use canonical snapshot records.

```rust
pub struct SnapshotRecord {
    pub entity: EntityRef,
    pub version: EntityVersion,
    pub schema: SchemaVersion,
    pub payload: Bytes,
}
```

Each adapter converts its storage format to/from canonical snapshot payload.

---

# 59. Snapshot Mapping Modes

Two modes:

```text
DomainSnapshot
GenericRecordSnapshot
```

DomainSnapshot is preferred where application defines typed entity snapshot.

GenericRecordSnapshot is useful for tooling and generic adapters.

---

# 60. Bootstrap Into Different DB

Example:

```text
PostgreSQL server
↓
canonical snapshot
↓
Stoolap adapter
↓
Stoolap local schema
```

Client schema does not need to match server schema.

---

# 61. Local Projection Architecture

A client may intentionally store only a projection.

Example:

Server:

```text
full Student aggregate
```

Client:

```text
student_id
name
class
photo_ref
```

Aequora scope/snapshot mapping can emit client-oriented authoritative projections.

---

# 62. Projection Contract

Server can register:

```text
ProjectionBuilder
```

for client sync payloads.

This avoids leaking internal DB schema.

---

# 63. Different Client Databases in Same Project

One project may support:

```text
Desktop -> Stoolap
Android -> SQLite
Embedded device -> Redb
```

All talk to the same server protocol.

Only local adapters differ.

---

# 64. Different Server Databases Across Deployments

Community edition:

```text
SQLite/PostgreSQL
```

Enterprise:

```text
PostgreSQL
```

Custom deployment:

```text
another Tier A authoritative adapter
```

Application operations remain unchanged.

---

# 65. Database Migration Without Client Rewrite

Suppose server changes:

```text
PostgreSQL
→ another authoritative DB
```

If both implement the same Aequora store traits:

```text
protocol unchanged
clients unchanged
domain operation IDs unchanged
```

Only infrastructure/repository adapter changes.

---

# 66. Client Database Migration

Client can migrate:

```text
Stoolap
→ SQLite
```

through:

```text
export canonical local snapshot
preserve pending outbox
install into new adapter
preserve DeviceId/cursor
resume
```

---

# 67. Pending Outbox Migration

This is critical.

Migration must carry:

```text
OperationId
payload
base version
dependencies
attempt state
```

unchanged.

Do not regenerate OperationIds.

---

# 68. Database Adapter Interface Layers

Split into:

```text
storage primitive layer
sync metadata layer
domain repository bridge
```

This prevents mixing Aequora internals with app repositories.

---

# 69. Storage Primitive Layer

Provides:

```text
transaction
get/put/query
commit/rollback
```

adapter-specific.

---

# 70. Sync Metadata Layer

Provides:

```text
outbox
cursor
journal
operation ledger
device metadata
snapshot metadata
```

standard Aequora semantics.

---

# 71. Domain Repository Bridge

Application uses its preferred database access.

Aequora only coordinates transaction boundaries.

---

# 72. Adapter SDK Traits

Recommended:

```rust
pub trait LocalAdapter {
    type Tx;

    async fn begin(&self) -> Result<Self::Tx, AdapterError>;
    async fn load_outbox(&self, ...) -> ...;
    async fn load_cursor(&self, ...) -> ...;
}

pub trait AuthoritativeAdapter {
    type Tx;

    async fn begin(&self) -> Result<Self::Tx, AdapterError>;
    async fn read_journal(&self, ...) -> ...;
    async fn lookup_operation(&self, ...) -> ...;
}
```

Public API should be refined into smaller capability traits internally.

---

# 73. Native Transaction Escape Hatch

Applications sometimes need native DB handles.

Provide adapter-specific extension traits rather than leaking them into core.

Example:

```rust
trait SqlxPostgresTxExt { ... }
trait StoolapTxExt { ... }
```

---

# 74. Data Type Registry

Generic mapping system should use a type registry.

```text
CanonicalTypeId
↔
adapter type mapping
```

Custom types can register codecs.

---

# 75. Custom Scalar Types

Examples:

```text
Money
EmailAddress
PhoneNumber
StudentCode
```

Domain operations should serialize them directly with Postcard.

Generic record sync may map them via:

```text
custom scalar codec
```

---

# 76. Enum Mapping

Canonical enums should use stable numeric discriminants.

Do not map enum variants solely by display string.

---

# 77. Binary Data

Small binary values:

```text
Bytes
```

Large binary values:

```text
BlobRef
```

Do not embed large files in normal DB sync payloads.

---

# 78. Query Independence

Aequora protocol must never send:

```text
SQL
Mongo query
KV command
```

across the sync boundary.

That would couple client/server DB semantics.

---

# 79. Repository Independence

Server validator/executor calls domain repositories.

The repository may use:

```text
SQLx
Diesel
SeaORM
raw protocol
custom DB client
```

Aequora does not care.

---

# 80. Generic CRUD Adapter

For simple applications, offer optional generic CRUD handler.

Example config:

```text
entity
fields
version field
allowed operations
```

This reduces boilerplate.

But it should be opt-in.

---

# 81. Generic CRUD Limitations

Do not use generic CRUD for:

```text
finance
inventory
multi-record invariants
security-sensitive workflows
```

Use domain operations.

---

# 82. Schema Registry

Aequora can maintain a logical schema registry for generic sync.

Contains:

```text
entity type
field IDs
field types
version
mapping rules
```

---

# 83. Stable Field IDs

Prefer stable numeric field IDs for wire-level generic records.

Renaming a field then does not break protocol identity.

---

# 84. Field Rename

Example:

```text
field id 7
name "phone"
```

later:

```text
name "mobile_phone"
```

The numeric field ID remains 7.

---

# 85. Field Removal

Removed fields should be tombstoned in schema history.

Do not immediately reuse their IDs.

---

# 86. Schema Evolution Across Different DBs

Each adapter may migrate physical schema differently.

Canonical schema version remains independent.

Example:

```text
canonical Student v4
```

Client Stoolap schema:

```text
local migration 9
```

Server PostgreSQL schema:

```text
server migration 17
```

These numbers need not match.

---

# 87. Database-Specific Migration Layer

Adapter may expose:

```text
prepare_schema
verify_schema
migrate_metadata
```

Application migration remains separate.

---

# 88. Mapping Validation at Startup

Generic mapping config should validate:

```text
all required fields mapped
types compatible
primary sync identity present
version field available
no duplicate mapping
no lossy conversions without policy
```

Fail startup on invalid mapping.

---

# 89. Example SQL↔SQL Mapping

```ron
EntityMap(
    canonical: "student",
    client: (
        table: "students",
        id: "sync_id",
        version: "version",
    ),
    server: (
        table: "student_profile",
        id: "sync_id",
        version: "sync_version",
    ),
)
```

Again, domain-operation mode usually avoids direct mapping.

---

# 90. Example KV↔SQL Mapping

Client:

```text
key:
student/<uuid>

value:
Postcard(StudentLocal)
```

Server:

```text
normalized PostgreSQL tables
```

Canonical operation:

```text
UpdateStudentPhone
```

No generic field mapping needed.

---

# 91. Example Document↔Document

Client document:

```json
{
  "id": "...",
  "name": "...",
  "address": {...}
}
```

Server document may differ structurally.

Canonical domain operation still provides compatibility.

---

# 92. Read-Only Source Adapter

Useful for migration/import.

```text
legacy database
↓
ReadOnlySourceAdapter
↓
canonical snapshot/events
↓
new authoritative store
```

---

# 93. Replica Sink Adapter

Useful for:

```text
analytics
search index
secondary reporting DB
```

It consumes authoritative events but does not become authoritative.

---

# 94. Multi-Sink Architecture

One authoritative server can feed:

```text
client sync
analytics DB
search engine
warehouse
```

through separate consumers.

Aequora client sync correctness should not depend on those sinks.

---

# 95. Change Feed Consumer

Expose:

```rust
pub trait ChangeConsumer {
    async fn apply(&self, change: &AuthoritativeChange) -> Result<(), ...>;
}
```

for secondary integrations.

Use durable consumer cursor separately.

---

# 96. Do Not Reuse Client Cursor for Analytics

Each consumer needs its own:

```text
consumer cursor
```

Client device cursors and backend projection cursors are distinct.

---

# 97. Cross-Database Consistency

Aequora does not guarantee one ACID transaction across:

```text
PostgreSQL authoritative DB
+
analytics DB
+
search index
```

Use:

```text
authoritative commit
+
durable journal
+
idempotent consumers
```

---

# 98. Database Pair Testing

Do not test only each adapter independently.

Also test representative pairings:

```text
Stoolap ↔ PostgreSQL
SQLite ↔ PostgreSQL
PostgreSQL ↔ PostgreSQL
KV ↔ PostgreSQL
```

This verifies canonical interoperability.

---

# 99. Pairwise Matrix Strategy

Avoid testing every possible N×M pair exhaustively.

Instead:

```text
each adapter passes canonical compliance suite
+
selected cross-family integration tests
```

This keeps testing scalable.

---

# 100. Canonical Compliance Suite

Every adapter must prove:

```text
ID roundtrip
version semantics
transaction semantics
cursor durability
snapshot behavior
type mapping
null behavior
timestamp mapping
decimal mapping
binary mapping
crash recovery
```

---

# 101. Golden Fixtures

Maintain canonical fixtures.

Example:

```text
integers min/max
UUID
decimal
timestamp
nested object
binary
null
enum
tombstone
```

Run through every generic adapter.

---

# 102. Property Testing

Generate canonical records and assert:

```text
canonical
→ adapter encode
→ adapter decode
→ canonical
```

preserves meaning.

---

# 103. Lossless Roundtrip Requirement

Tier A generic adapters should guarantee lossless roundtrip for declared supported canonical types.

Unsupported types must fail explicitly.

---

# 104. Database-Specific Unsupported Features

Example:

Adapter lacks arrays.

Options:

```text
encode as document
custom scalar
reject mapping
```

Never silently flatten or stringify without policy.

---

# 105. Ordering Differences

Do not rely on database default row order.

Canonical journal sequence controls synchronization order.

---

# 106. Collation Differences

String uniqueness can differ by:

```text
case sensitivity
locale
Unicode normalization
```

Business-critical uniqueness should be defined explicitly at domain/database layer.

---

# 107. Unicode Normalization

If domain requires canonical text equality, normalize intentionally.

Do not assume all DBs compare Unicode identically.

---

# 108. Boolean Mapping

Some DBs use:

```text
BOOLEAN
INTEGER 0/1
text
```

Adapter converts explicitly to canonical Bool.

---

# 109. Date/Time Precision

Adapters must declare precision.

Example:

```text
nanoseconds
microseconds
milliseconds
seconds
```

If destination precision is lower, policy must define acceptable truncation.

---

# 110. Floating-Point Semantics

Do not use float equality for concurrency/version semantics.

Floats are payload data only.

---

# 111. Database Constraints and Canonical Validation

Canonical validator handles generic structural rules.

Destination database constraints remain authoritative backstop.

---

# 112. Conflict Handling Across Different Schemas

Conflict logic must compare:

```text
canonical domain/version semantics
```

not raw row bytes.

A server with normalized tables still exposes one aggregate version.

---

# 113. Aggregate Version Mapping

Adapter/repository decides how to persist:

```text
AggregateVersion
```

Possible:

```text
column on root row
metadata table
event-stream version
```

---

# 114. Same DB, Different Version Storage

Even if both sides use PostgreSQL, client may store:

```text
version in metadata table
```

server:

```text
version column
```

Aequora remains unaffected.

---

# 115. Conflict-Free Commutative Operations

Certain operations can ignore row structure entirely.

Examples:

```text
increment counter
append comment
add tag
record immutable event
```

These are naturally portable across DB types.

---

# 116. Delete Interoperability

Canonical delete:

```text
Tombstone
```

Adapter translates to:

```text
soft delete
hard delete + metadata tombstone
document marker
KV tombstone key
```

depending on DB.

---

# 117. Tombstone Persistence

Even if physical record is removed, adapter must retain enough deletion metadata until safe GC.

---

# 118. Full Resync

Any adapter must support one of:

```text
canonical snapshot install
adapter-native database rebuild
generation switch
```

to recover from invalid cursor/history.

---

# 119. Adapter Migration Between Engines

Provide a generic migration path:

```text
SourceAdapter
↓
canonical export
↓
TargetAdapter
```

Useful for:

```text
Stoolap -> SQLite
SQLite -> PostgreSQL
PostgreSQL -> another DB
```

---

# 120. Migration Export Format

Use:

```text
Postcard data file
+
RON manifest
+
checksums
```

Manifest includes:

```text
schema version
entities
cursor
device metadata
pending operations
hashes
```

---

# 121. Client Store Migration

Must preserve:

```text
DeviceId
scope
cursor
outbox
conflicts
authoritative entity versions
```

Otherwise sync semantics reset.

---

# 122. Server Store Migration

Must preserve:

```text
domain state
operation ledger
journal sequence semantics
scope generation
snapshot metadata
```

or intentionally create a new timeline generation.

---

# 123. Timeline Reset During Server Migration

If exact journal continuity cannot be preserved:

```text
increment CursorGeneration
```

and require clients to bootstrap.

This is safer than pretending history is continuous.

---

# 124. Database Adapter Discovery

Rust integration should remain compile-time.

Developers choose crates:

```toml
aequora-store-stoolap
aequora-store-postgres
aequora-store-sqlite
```

No runtime driver loading required.

---

# 125. Adapter Registry

Facade can expose known adapters through feature flags.

Third-party adapters implement public SDK traits.

---

# 126. Adapter Naming

Recommended ecosystem:

```text
aequora-store-postgres
aequora-store-sqlite
aequora-store-stoolap
aequora-store-redb
aequora-store-mysql
aequora-store-mongodb
```

Only advertise production support after compliance certification.

---

# 127. Initial Database Support Priority

Recommended order:

```text
1. Stoolap — local
2. PostgreSQL — authoritative/local
3. SQLite — local
4. Redb — local KV
5. MySQL/MariaDB — authoritative
6. selected document DB
```

Do not attempt every database simultaneously.

---

# 128. Why PostgreSQL First on Server

It provides a strong baseline for:

```text
transactions
constraints
MVCC
locking
sequences
JSONB
mature tooling
```

Other adapters can then prove equivalent Aequora semantics.

---

# 129. Why Embedded DBs First on Client

Client requirements differ:

```text
small footprint
offline
single-process
fast local reads
durability
simple deployment
```

Hence:

```text
Stoolap
SQLite
Redb
```

are more natural first targets.

---

# 130. Server-on-Embedded DB

Aequora should permit it if adapter passes authoritative compliance.

Useful for:

```text
small self-hosted deployment
single-user systems
edge servers
```

But capability tier must be explicit.

---

# 131. Client-on-Server DB

Possible:

```text
desktop app connects to local PostgreSQL
```

if a local adapter exists.

Aequora architecture should not forbid unusual deployment patterns.

---

# 132. One Database Shared by Client and Server

Sometimes a "client" and "server" process may share one physical DB.

Even then:

```text
logical local/authoritative roles
```

must remain separate if the application uses Aequora sync semantics.

Do not bypass validation merely because storage is shared.

---

# 133. Direct Shared-DB Mode

Optional optimization:

```text
InProcessTransport
+
same DB
```

for tests/single-node mode.

But preserve:

```text
OperationId
validation
journal
versions
```

so behavior matches distributed deployment.

---

# 134. Cross-Database Query Translation Is Out of Scope

Aequora should not become:

```text
SQL translator
ORM translator
federated query engine
```

It synchronizes domain changes.

This keeps scope manageable.

---

# 135. Database-Specific Search/Analytics

If one side has features such as:

```text
full-text search
vector index
graph traversal
```

those remain local/server-specific capabilities.

Only authoritative domain data syncs.

---

# 136. Schema Introspection

Generic adapter tooling may inspect:

```text
tables
columns
types
indexes
```

to help generate mappings.

It should not auto-assume business semantics.

---

# 137. Mapping Generator

CLI:

```text
aequora map inspect --database ...
```

can generate draft RON mapping.

Developer reviews and commits it.

---

# 138. Avoid Automatic Semantic Guessing

Do not assume:

```text
updated_at = sync cursor
id = distributed ID
deleted = tombstone
```

without explicit configuration.

---

# 139. Generic Sync Mapping Config

Example:

```ron
DatabaseMapping(
    entities: [
        (
            canonical: "student",
            source: (
                collection: "students",
                id_field: "sync_id",
                version_field: "sync_version",
            ),
        ),
    ],
)
```

---

# 140. Adapter Health

Each adapter exposes health diagnostics:

```text
connectivity
transaction test
schema version
journal availability
outbox availability
capability report
```

---

# 141. `aequora doctor` Database Output

Example:

```text
Client adapter:
    Stoolap
    role: LocalWritable
    tier: A
    ACID: PASS
    metadata migration: current

Server adapter:
    PostgreSQL
    role: AuthoritativeWritable
    tier: A
    ACID: PASS
    journal: healthy
```

---

# 142. Database Error Normalization

Core categories:

```text
Unavailable
Timeout
Deadlock
SerializationConflict
ConstraintViolation
Corruption
PermissionDenied
Unsupported
Fatal
```

Adapters convert native errors.

---

# 143. Retry Rules by Error Class

Retry:

```text
Unavailable
Timeout
Deadlock
SerializationConflict
```

Usually do not retry automatically:

```text
ConstraintViolation
PermissionDenied
Unsupported
Corruption
```

---

# 144. Database Corruption

If adapter detects corruption:

```text
stop normal sync
quarantine
surface fatal diagnostic
```

Do not keep retrying indefinitely.

---

# 145. Client Corruption Recovery

```text
preserve recoverable outbox
bootstrap into clean store
reconcile pending operations
```

---

# 146. Server Corruption Recovery

Restore authoritative DB from backup/PITR.

If timeline changes:

```text
new cursor generation
```

---

# 147. Multi-Database Server Internals

An application may itself use multiple authoritative databases.

Example:

```text
PostgreSQL domain DB
object storage
analytics DB
```

Aequora authoritative transaction should have exactly one designated authoritative transaction boundary for each operation.

---

# 148. Avoid Cross-DB Distributed Transactions

If one operation needs two independent DBs:

```text
DB A
DB B
```

do not assume atomic commit unless an explicit distributed transaction architecture exists.

Prefer:

```text
commit authoritative DB
+
durable side-effect/outbox
+
idempotent secondary consumer
```

---

# 149. Authoritative Store Selection

Each operation descriptor can optionally declare:

```text
authoritative store group
```

for applications with multiple independent bounded contexts.

Example:

```text
school-core
finance
documents
```

---

# 150. Multi-Store Server Registry

Possible:

```rust
StoreRegistry {
    "core" -> PostgresAdapter,
    "documents" -> DocumentAdapter,
}
```

Operation handler chooses one authoritative transaction domain.

Do not allow arbitrary two-store atomic writes by default.

---

# 151. Multi-DB Bounded Contexts

Good architecture:

```text
Student operations -> Core DB
Finance operations -> Finance DB
Document metadata -> Document DB
```

Each has its own journal or unified Aequora journal bridge.

---

# 152. Unified Journal Options

Two approaches:

```text
A. one central authoritative journal
B. per-store journals merged into canonical sequence
```

Initial recommendation:

```text
one authoritative primary DB/journal per sync service
```

Simpler and safer.

---

# 153. Per-Store Sync Service

For truly independent DB domains:

```text
/sync/core
/sync/finance
```

or logical scopes.

Each can have its own cursor generation.

---

# 154. Cross-Domain Client Coordinator

Client can coordinate multiple Aequora sync channels.

Example:

```text
core sync
finance sync
documents sync
```

They remain independently recoverable.

---

# 155. Don't Force One Global Transaction Across Modules

Independent bounded contexts should remain independent.

This improves scalability and fault isolation.

---

# 156. Adapter Performance Contract

Adapters should report/benchmark:

```text
max safe batch
preferred batch
snapshot throughput
journal paging throughput
transaction latency
```

Runtime may use hints.

---

# 157. Adaptive Batch Hints

Aequora may ask adapter:

```rust
fn preferred_batch_limits(&self) -> BatchLimits;
```

This is advisory.

Global safety caps still apply.

---

# 158. Prepared Statements

SQL adapters should cache/prepare repeated metadata operations where driver supports it.

Core does not care.

---

# 159. Batched Writes

Adapters may batch:

```text
journal inserts
ACK updates
snapshot loads
```

while preserving transaction semantics.

---

# 160. Zero-Copy and Database Interop

Do not over-design zero-copy across arbitrary DB drivers.

Use:

```text
Bytes
borrowed slices where practical
```

but prioritize adapter simplicity and correctness.

---

# 161. Postcard Persistence

Adapters may store operation/event payloads exactly as Postcard bytes.

This avoids repeated schema translation for opaque domain payloads.

---

# 162. RON for Mapping/Diagnostics

RON is well suited for:

```text
adapter config
schema maps
capability manifests
migration/export manifests
debugging
```

---

# 163. JSON Only at External Boundaries

Use JSON when:

```text
third-party API
admin web API
interop ecosystem requires it
```

Aequora internal wire remains Postcard.

---

# 164. Database Support Matrix Documentation

Maintain table like:

```text
Database | Client | Server | Tier | Transactions | Snapshot | CDC | Notes
```

This should be generated from adapter manifests where possible.

---

# 165. Compatibility Matrix

Also track:

```text
Adapter version
Aequora core version
DB engine versions tested
```

---

# 166. Database Version Drift

Adapters should declare supported DB versions.

Startup may warn or fail outside tested range depending on policy.

---

# 167. Enterprise Certification

A Tier A adapter should pass:

```text
ACID suite
crash suite
concurrency suite
type roundtrip suite
snapshot suite
migration suite
load test
```

---

# 168. Third-Party Adapter Certification

Third-party crates can run the same public compliance harness.

This enables ecosystem growth without central implementation of every DB.

---

# 169. Example Project Matrix

Project A:

```text
client = Stoolap
server = PostgreSQL
```

Project B:

```text
client = SQLite
server = PostgreSQL
```

Project C:

```text
client = Redb
server = MySQL
```

All reuse:

```text
same Aequora protocol
same client state machine
same server validator/executor
same OperationId semantics
same conflict model
```

---

# 170. Example Same-DB Matrix

Project D:

```text
client PostgreSQL
server PostgreSQL
```

Still reuse the same system.

No special "same DB sync" architecture is needed.

---

# 171. Example Different Schema

Client:

```text
contact card document
```

Server:

```text
people
phones
emails
addresses
```

Operation:

```text
UpdateContact
```

Server handler maps the domain operation to normalized tables.

---

# 172. Full Data Flow

```text
APP MUTATION
    ↓
Local DB native transaction
    ↓
Local adapter
    ├─ app state
    └─ Aequora outbox
    ↓
Canonical OperationEnvelope
    ↓
Postcard
    ↓
HTTP/Axum
    ↓
Aequora Server
    ↓
Domain Handler
    ↓
Server adapter native transaction
    ├─ authoritative state
    ├─ version
    ├─ journal
    └─ operation ledger
    ↓
Canonical AuthoritativeChange
    ↓
Postcard
    ↓
Client reconciler
    ↓
Local adapter
    ↓
Client DB native transaction
```

---

# 173. Universal Database Boundary

The architecture can be summarized as:

```text
                 CANONICAL AEQUORA CORE

Local DB A  ── Adapter A ──┐
Local DB B  ── Adapter B ──┤
Local DB C  ── Adapter C ──┼── Aequora Protocol
                           │
Server DB X ─ Adapter X ───┤
Server DB Y ─ Adapter Y ───┤
Server DB Z ─ Adapter Z ───┘
```

No adapter talks directly to another adapter.

---

# 174. What Is Universal

These remain identical for every DB pair:

```text
OperationId
Postcard protocol
cursor semantics
entity versions
conflict framework
retry behavior
bootstrap protocol
device identity
server authority
Axum API
security model
observability
```

---

# 175. What Is Adapter-Specific

Only:

```text
native transaction implementation
metadata persistence
journal query implementation
snapshot implementation
type mapping
error mapping
DB-specific optimizations
```

---

# 176. What Is Application-Specific

Only:

```text
domain operations
authorization
business validation
repositories
execution
conflict policy
scope policy
```

---

# 177. Recommended Crate Layout

```text
crates/
├── aequora-core/
├── aequora-protocol/
├── aequora-client/
├── aequora-server/
├── aequora-adapter-sdk/
├── aequora-schema/
├── aequora-mapping/
├── aequora-testkit/
│
├── aequora-store-stoolap/
├── aequora-store-postgres/
├── aequora-store-sqlite/
├── aequora-store-redb/
├── aequora-store-mysql/
│
└── aequora/
```

---

# 178. `aequora-schema`

Responsible for:

```text
canonical types
generic record schema
field IDs
mapping validation
schema evolution
```

Optional for projects using only domain-operation sync.

---

# 179. `aequora-mapping`

Responsible for:

```text
generic entity mapping
field mapping
type conversion
schema introspection helpers
migration/export maps
```

Keep it out of core hot path unless needed.

---

# 180. Feature Minimization

A domain-operation-only project should not compile generic mapping machinery unless enabled.

Example:

```toml
features = ["domain-sync"]
```

Generic record support:

```toml
features = ["record-sync"]
```

---

# 181. API Example — Same Database

```rust
let client_store = PostgresStore::local(client_pool);
let server_store = PostgresStore::authoritative(server_pool);
```

Same adapter crate, different roles.

---

# 182. API Example — Different Databases

```rust
let client_store = StoolapStore::local(db);
let server_store = PostgresStore::authoritative(pool);
```

No other Aequora code changes.

---

# 183. API Example — Custom DB

```rust
struct MyDbAdapter { ... }

impl LocalSyncStore for MyDbAdapter { ... }
```

Run:

```rust
aequora_local_store_compliance!(MyDbFactory);
```

Then use with normal client builder.

---

# 184. Plug-and-Play Selection

Ideal developer config:

```ron
Storage(
    client: Stoolap(...),
    server: Postgres(...),
)
```

At Rust compile time, concrete adapters are still explicitly linked.

---

# 185. Runtime DB Selection

If an application needs runtime-selectable adapters:

```text
enum ConfiguredStore {
    Stoolap(...),
    SQLite(...),
}
```

Facade may wrap behind trait objects.

Avoid this overhead unless genuinely required.

---

# 186. Static Dispatch Preferred

For most products:

```text
compile one known client adapter
compile one known server adapter
```

This gives:

```text
simpler binaries
better type checking
less complexity
```

---

# 187. Dynamic Dispatch for Tooling

Admin/migration CLI may use trait objects to support many adapters in one binary.

This is a good use case for runtime polymorphism.

---

# 188. Database Migration Tooling

CLI:

```text
aequora db export
aequora db import
aequora db migrate
aequora db verify
```

Use canonical export/import.

---

# 189. Verification Mode

Cross-check two stores:

```text
source canonical hash
target canonical hash
```

Useful during DB migration.

---

# 190. Merkle/Chunk Verification

For large datasets, optional chunk hashes can verify migration/snapshot equality efficiently.

---

# 191. Repair Mode

If target diverges:

```text
identify mismatched canonical entities
```

Do not automatically overwrite authoritative data without explicit operator action.

---

# 192. Security of Database Credentials

Each adapter owns connection configuration but consumes secret wrappers.

Never expose credentials through protocol manifests or diagnostics.

---

# 193. Least Privilege by Adapter Role

Local adapter:

```text
local DB access only
```

Server runtime adapter:

```text
DML required for app/Aequora
```

Migration adapter:

```text
DDL
```

Separate roles where supported.

---

# 194. Fail-Closed Behavior

If type mapping, schema mapping, or capability verification is uncertain:

```text
fail
```

Do not silently coerce.

---

# 195. Universal Correctness Principle

Database independence must never weaken correctness.

Aequora should prefer:

```text
"this adapter cannot safely provide required semantics"
```

over:

```text
"it mostly works"
```

---

# 196. Recommended Support Strategy

Do not attempt to officially maintain 20 DB adapters immediately.

Create:

```text
strong adapter SDK
public compliance suite
clear capability manifest
```

Then officially maintain a focused set.

---

# 197. First Official Adapter Set

Recommended:

```text
Local:
    Stoolap
    SQLite
    Redb

Authoritative:
    PostgreSQL

Later:
    MySQL/MariaDB
    selected document DB
```

---

# 198. Why This Scales Better

Pairwise approach:

```text
Stoolap↔Postgres
Stoolap↔MySQL
SQLite↔Postgres
SQLite↔MySQL
Redb↔Postgres
...
```

becomes unmaintainable.

Canonical approach:

```text
Stoolap adapter
SQLite adapter
Redb adapter
Postgres adapter
MySQL adapter
```

All interoperate through the same core.

---

# 199. Final Universal Database Architecture

```text
                       APPLICATION DOMAIN
                               │
                               ▼
                       Aequora Operations
                               │
                               ▼
                     Canonical Sync Engine
                               │
             ┌─────────────────┴─────────────────┐
             │                                   │
             ▼                                   ▼
       Client Adapter                       Server Adapter
             │                                   │
   ┌─────────┼─────────┐               ┌─────────┼─────────┐
   ▼         ▼         ▼               ▼         ▼         ▼
Stoolap   SQLite     Redb          PostgreSQL   MySQL    Other
```

No database pair receives special protocol logic.

---

# 200. Final Recommendation

To make Aequora support "most databases on any side", do not build a database-to-database replication engine.

Build:

```text
1. one canonical operation/event protocol;
2. strong local and authoritative adapter traits;
3. a capability manifest;
4. canonical generic data types for record-mode sync;
5. explicit schema/type mapping;
6. adapter-specific transaction implementations;
7. a public compliance/certification suite;
8. snapshot/export/import tooling;
9. CDC bridges only as optional legacy integrations.
```

The strongest default remains:

```text
domain operations
+
adapter-owned persistence
+
server-authoritative execution
```

because it works whether the two databases are:

```text
the same
different
relational
embedded
document
key-value
```

without sacrificing business correctness.

The architectural objective is:

> **A database choice should become a deployment/infrastructure decision, not a reason to redesign the synchronization protocol.**

That is the foundation required for Aequora to become a truly universal Rust synchronization library.

---

# 201. Implementation Status

The database-neutral domain-operation path is implemented: local, authoritative, and transport
selection are independent; built-in Stoolap and PostgreSQL adapters own native transaction and
migration boundaries; shared conformance contracts verify both roles; and no adapter communicates
directly with another adapter.

The adapter SDK now publishes a serializable `AdapterManifest` with stable roles, certification
tier, tested Aequora/database versions, semantic capabilities, and limitations. Startup can use
`AdapterRequirements` or `ProductionAdapterPair::verify` to fail closed before composing a local
and authoritative adapter. Built-in `STOOLAP_ADAPTER_MANIFEST` and
`POSTGRES_ADAPTER_MANIFEST` pass those production requirements independently.

Generic record interoperability is feature-gated behind `record-sync`. `aequora-schema` supplies
stable entity/field IDs, exact decimals, UTC timestamps, a non-floating canonical value model,
validated schemas, and fail-closed record validation. `aequora-mapping` supplies explicit physical
entity/field/type maps, required-field coverage, and mandatory acknowledgement for lossy
conversions. It validates declarations without introducing SQL or database-pair logic into core.

`aequora-migration` defines a bounded Postcard export artifact with canonical-schema identity,
contiguous independently hashed chunks, a root digest, record validation, tamper detection, and a
`VerifiedExport` type that must exist before import. It does not open or overwrite databases;
adapter-specific export/import transactions and authority timeline changes remain explicit.

The installable `aequora` binary supplied by `aequora-cli` exposes payload-free `doctor adapters`,
`inspect adapter`, and `verify pair` commands. Static inspection never opens a database or prints a
credential. Live database health remains an explicit host/environment check.

The exact section mapping and remaining migration-tooling, record-conversion, and legacy CDC
work are tracked in
[`docs/database-interoperability-completion.md`](docs/database-interoperability-completion.md).
Those gaps remain within the prerequisite phase; their absence must not be hidden by claiming that
support for arbitrary database schemas or engines already exists.
