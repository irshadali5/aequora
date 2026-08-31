# Aequora Sync — Part 37

# PostgreSQL / Neon Authoritative Adapter Detailed Architecture

## 1. Purpose

Part 36 defined the generic Storage Adapter SDK. Part 37 defines the first official authoritative adapter:

```text
aequora-postgres
```

with Neon treated as a PostgreSQL-compatible deployment profile rather than a different synchronization architecture.

> **PostgreSQL is the physical implementation of authority; Aequora's authority, idempotency, journal, and transactional semantics remain the contract.**

## 2. Responsibilities

The adapter provides:

```text
authoritative transactions
operation ledger
journal
entity/aggregate versioning
idempotency
tenant isolation
cursor scans
retention floors
snapshot metadata
device metadata
audit/governance metadata
migrations
health/readiness
PITR/epoch integration
```

It does not own HTTP routing, Axum extractors, client outbox behavior, Dioxus, domain business rules, or protocol framing.

## 3. High-Level Architecture

```text
Aequora Server Core
        |
        v
PostgresAuthorityStore
        |
        +-- Connection Pool
        +-- Tx Manager
        +-- Operation Ledger
        +-- Journal
        +-- Version Repository
        +-- Device / Scope Metadata
        +-- Audit / Governance
        +-- Migration Manager
        |
        v
    PostgreSQL
        |
        v
      Neon
```

## 4. Crate Layout

```text
aequora-postgres/
├── config.rs
├── pool.rs
├── tx.rs
├── migration.rs
├── journal.rs
├── ledger.rs
├── version.rs
├── device.rs
├── scope.rs
├── snapshot.rs
├── audit.rs
├── governance.rs
├── retention.rs
├── health.rs
├── errors.rs
├── capabilities.rs
└── neon.rs
```

Internally it may use SQLx, Tokio, tracing, Postcard, and BLAKE3. SQLx types must remain private.

## 5. Authoritative Transaction Boundary

The core authoritative transaction, Tx B, must atomically include:

```text
ledger/idempotency check
business/domain mutation
entity or aggregate version update
journal append
ledger outcome persistence
required audit record
side-effect intent enqueue
```

Canonical flow:

```text
BEGIN
  verify idempotency
  validate current state/version
  execute deterministic domain mutation
  update version
  append journal
  persist operation outcome in ledger
  append required audit
  enqueue side-effect intents
COMMIT
```

## 6. Operation Ledger

Logical schema:

```text
aequora.operation_ledger
├── operation_id
├── tenant_id
├── payload_digest
├── operation_kind
├── schema_version
├── status
├── outcome_bytes
├── committed_sequence
├── handler_version
└── created_at
```

Rules:

```text
same OperationId + same payload digest
    -> return previous logical outcome

same OperationId + different payload digest
    -> reject
```

The ledger must store enough information to answer duplicate retries without re-executing business logic.

## 7. Duplicate Races

Two identical requests may arrive concurrently. The database must guarantee one logical effect.

Use unique OperationId constraints plus transactional logic. A pre-transaction lookup alone is insufficient because both requests may observe absence.

## 8. Journal

Logical schema:

```text
aequora.journal
├── tenant_id
├── authority_id
├── authority_epoch
├── sequence
├── event_id
├── operation_id
├── entity_type
├── entity_id
├── entity_version
├── event_kind
├── payload
├── schema_version
└── committed_at
```

The journal is append-oriented and must be committed atomically with business state.

## 9. Timeline Ordering

Aequora replication order must not rely on:

```text
client timestamps
HLC alone
PostgreSQL sequence allocation order
```

because ordinary SQL sequence values can be allocated before transaction commit and therefore do not necessarily represent committed publication order.

## 10. Transactional Timeline Allocator

Use an explicit timeline head.

Logical table:

```text
aequora.timeline_head
├── tenant_id
├── authority_id
├── authority_epoch
└── next_sequence
```

Inside Tx B:

```text
lock timeline row
read next_sequence
reserve required range
update timeline head
insert journal rows
commit
```

If the transaction aborts, allocation rolls back.

## 11. Timeline Partitioning

A single global timeline row can become a hotspot.

Preferred v1 strategy:

```text
one authoritative timeline per tenant
```

or another bounded partition aligned with sync semantics.

Future scale may use multiple ordered lanes only if cursor semantics are explicitly extended.

## 12. Cursor Model

A cursor should bind:

```text
tenant/scope
authority_id
authority_epoch
sequence
```

The server returns events strictly after the client's accepted sequence for that authority epoch.

## 13. Versioning

Use either:

```text
version column in domain table
```

or a generic sidecar:

```text
aequora.entity_version
├── tenant_id
├── entity_type
├── entity_id
└── version
```

For aggregate consistency, use aggregate-root versioning rather than blindly versioning rows independently.

## 14. Compare-and-Swap

Optimistic concurrency can use:

```text
WHERE current_version = expected_version
```

A zero-row update maps to stale-base/conflict semantics, not infrastructure failure.

## 15. Isolation

Default candidate:

```text
READ COMMITTED
```

with explicit locks and CAS.

Use `SERIALIZABLE` only where domain semantics justify it; forcing it globally increases retries and reduces throughput.

## 16. Deadlocks

Deadlocks are normal possibilities.

Mitigation:

```text
canonical lock ordering
short transactions
bounded retries
good diagnostics
```

## 17. Retryable Database Errors

Adapter maps physical errors such as:

```text
serialization failure
deadlock
pool timeout
connection interruption
```

into canonical Aequora categories.

Automatic transaction replay is allowed only when execution is deterministic/replay-safe.

## 18. Ambiguous Commit

If the connection fails after commit submission but before confirmation, the result may be unknown.

Do not assume rollback.

Retry by the same OperationId; the ledger resolves ambiguity safely.

## 19. External Side Effects

Never call payment/email/webhook providers inside Tx B.

Instead insert durable jobs:

```text
aequora.side_effect_job
├── job_id
├── operation_id
├── kind
├── payload
├── status
└── retry_state
```

The job worker runs after commit.

## 20. Audit

If audit is mandatory:

```text
business mutation + required audit
```

must share Tx B.

Logical audit data includes action ID, actor, provenance, reason code, and permitted before/after data.

## 21. Device Metadata

Logical schema:

```text
aequora.device
├── device_id
├── tenant_id
├── actor_id
├── public_key
├── binding_generation
├── status
├── last_seen
└── last_ack_sequence
```

Statuses include Active, Revoked, and Retired.

## 22. Retention Leases

Track active consumers/devices:

```text
device_id
scope_id
ack_sequence
last_active
lease_expiry
```

The journal floor advances only when policy proves older history is no longer required.

Inactive devices eventually require rebootstrap rather than pinning the journal forever.

## 23. Tenant Isolation

Every multi-tenant authoritative query must bind tenant identity explicitly.

Optional PostgreSQL RLS can provide defense in depth, but Aequora application authorization remains required.

## 24. Dedicated PostgreSQL Schema

Recommended:

```text
aequora.operation_ledger
aequora.journal
aequora.timeline_head
aequora.device
aequora.retention_lease
aequora.migration
aequora.snapshot
aequora.audit
aequora.job
```

Application business tables may live in another schema while sharing the same database transaction.

## 25. Same Database Requirement

For the cleanest authority invariant, place authoritative application tables and Aequora metadata in the same PostgreSQL database.

Using separate databases breaks the single ACID transaction unless a deliberate distributed bridge is designed.

## 26. Migrations

Recommended:

```text
migrations/postgres/
├── 0001_core.sql
├── 0002_ledger.sql
├── 0003_journal.sql
├── 0004_device.sql
└── ...
```

Migration ledger stores:

```text
migration_id
checksum
applied_at
build_id
```

Same migration ID with changed body must fail.

## 27. Migration Lock

Only one schema migrator runs at once.

Use a dedicated row/advisory lock for migration coordination.

## 28. Rolling Deployment

Prefer expand-and-contract:

```text
add schema
deploy compatible code
backfill
switch usage
remove old schema later
```

Avoid destructive rename/drop in the same deployment where old and new servers may coexist.

## 29. Physical Types

Typical choices:

```text
UUID
BIGINT
BYTEA
TIMESTAMPTZ
INTEGER/SMALLINT
BOOLEAN
```

Use JSONB only for genuinely dynamic metadata. Canonical durable operation/event payloads should normally remain Postcard bytes in `BYTEA`.

## 30. Large Content

Never place large attachments directly in journal rows.

Journal records should reference a `BlobRef`; large content belongs in the blob/object subsystem.

## 31. Required Indexes

Examples:

```text
operation_ledger(operation_id)
journal(tenant_id, authority_epoch, sequence)
journal(operation_id)
device(tenant_id, device_id)
retention_lease(tenant_id, scope_id, ack_sequence)
```

Measure before adding broad secondary indexing.

## 32. Journal Partitioning

Partitioning may eventually be useful by:

```text
tenant hash
time
epoch
```

but must preserve efficient ordered scans after cursor and safe retention.

Physical time partitioning must never redefine sequence semantics.

## 33. Ledger Retention

Ledger retention and journal retention are separate.

The idempotency horizon may need to exceed the hot journal horizon.

Never delete ledger entries merely because the corresponding journal row aged out.

## 34. Pooling

Use a bounded pool with:

```text
min/max connections
acquire timeout
idle timeout
max lifetime
```

Admission control should prevent pool exhaustion from turning into unbounded waiting.

## 35. Transaction Timeouts

Recommended controls:

```text
statement timeout
lock timeout
idle-in-transaction timeout
```

The adapter should surface typed timeout/resource errors.

## 36. Neon as a Deployment Profile

Neon changes operational characteristics such as:

```text
compute autosuspend
cold starts
connection behavior
branching
managed storage
```

but does not change Aequora authority semantics.

## 37. Neon Cold Starts

Cold compute can increase first-query latency.

Timeout and retry policy should account for this operationally without relaxing correctness.

## 38. Neon Branches

Branches are useful for:

```text
development
migration rehearsal
testing
```

but a branch is not automatically the same authoritative timeline.

Promoting/restoring a branch must interact with AuthorityEpoch rules.

## 39. PITR and Restore

A PostgreSQL/Neon point-in-time restore can move authoritative state backward.

Therefore:

> **A restore that may invalidate previously observed history must create a new AuthorityEpoch before sync resumes.**

Otherwise clients could see an impossible timeline rollback.

## 40. Authority Metadata

Persist:

```text
AuthorityId
AuthorityEpoch
TimelineHead
CheckpointRoot
```

where required.

For high-assurance environments, protect epoch rollback with an external operational/cryptographic anchor.

## 41. Backup

Database backups are authority disaster-recovery mechanisms.

They are distinct from client bootstrap snapshots.

Regular restore drills should verify:

```text
restore
migration
epoch transition
journal/ledger consistency
```

## 42. Read Replicas

Read replicas may serve eventual application reads or analytics.

Authoritative Tx B always goes to the writer.

For v1, sync pull should also come from authority unless replica watermark semantics are explicitly implemented.

## 43. Multi-Region

Use one authoritative writer region.

Regional replicas may serve reads according to Part 17.

## 44. LISTEN/NOTIFY

PostgreSQL `LISTEN/NOTIFY` can accelerate live sync/change-feed wakeups.

It remains a hint channel.

Missed notifications are harmless because consumers resume from durable cursor state.

## 45. Snapshot Metadata

PostgreSQL may store:

```text
snapshot_id
scope/tenant
boundary sequence
manifest digest
object keys
publication status
```

while actual snapshot chunks live in object storage.

## 46. Snapshot Consistency

The snapshot state and boundary sequence must correspond to one consistent authority view.

Use PostgreSQL MVCC/snapshot facilities as appropriate without holding unnecessarily long write-blocking transactions.

## 47. Governance

Legal hold, erasure directives, retention policies, and governance workflow metadata may live in the authoritative PostgreSQL store.

PITR restores must account for already-completed erasures before restored authority is exposed again.

## 48. Error Mapping

Examples:

```text
SQLSTATE 40001
-> serialization retry

SQLSTATE 40P01
-> deadlock retry

unique violation on operation_id
-> duplicate/idempotency handling
```

Exact SQLSTATE mapping stays inside `aequora-postgres`.

## 49. Stable Constraint Names

Name critical constraints explicitly so adapter error classification does not depend on brittle generated names.

## 50. Secrets and TLS

Database credentials come from secret providers/environment.

Never log full DSNs.

Production DB connections should use verified TLS according to deployment requirements.

## 51. Least Privilege

Prefer separate roles where useful:

```text
runtime application role
migration role
read-only operational role
```

The runtime role should have only required permissions.

## 52. Observability

Useful metrics:

```text
pool wait
transaction latency
deadlock retries
serialization retries
journal append latency
duplicate OperationId rate
cursor scan latency
timeline lock wait
```

Use tracing fields with safe IDs, not sensitive payloads.

## 53. Health

Liveness:

```text
process alive
```

Readiness:

```text
DB reachable
writer available
schema correct
migrations compatible
required capabilities active
```

Deep diagnostics may additionally inspect timeline metadata and migration state.

## 54. Reference DDL — Operation Ledger

Illustrative:

```sql
CREATE TABLE aequora.operation_ledger (
    operation_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    payload_digest BYTEA NOT NULL,
    operation_kind INTEGER NOT NULL,
    schema_version INTEGER NOT NULL,
    outcome BYTEA,
    committed_sequence BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

## 55. Reference DDL — Journal

Illustrative:

```sql
CREATE TABLE aequora.journal (
    tenant_id UUID NOT NULL,
    authority_epoch BIGINT NOT NULL,
    sequence BIGINT NOT NULL,
    event_id UUID NOT NULL,
    operation_id UUID,
    entity_type INTEGER NOT NULL,
    entity_id UUID NOT NULL,
    entity_version BIGINT NOT NULL,
    event_kind INTEGER NOT NULL,
    payload BYTEA NOT NULL,
    schema_version INTEGER NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, authority_epoch, sequence)
);
```

## 56. Reference DDL — Timeline Head

```sql
CREATE TABLE aequora.timeline_head (
    tenant_id UUID PRIMARY KEY,
    authority_id UUID NOT NULL,
    authority_epoch BIGINT NOT NULL,
    next_sequence BIGINT NOT NULL
);
```

## 57. Multi-Event Operation

If one operation emits N journal events:

```text
lock timeline head
reserve contiguous N-sequence range
emit deterministic event order
commit
```

## 58. Network Batch vs DB Transaction

A network request can contain many operations.

Do not automatically place all of them in one DB transaction.

Default atomicity should be:

```text
per operation
or
per explicitly grouped aggregate
```

as determined by the dependency/domain planner.

## 59. TOCTOU

Validation that depends on current authoritative state may be performed optimistically before Tx, but concurrency-sensitive assumptions must be revalidated inside Tx.

## 60. Auth Context

The adapter receives already-established `AuthContext`/domain execution context.

It must not derive authorization solely from PostgreSQL session state.

## 61. HLC vs Sequence

Persist client/server HLC where useful for causality.

Replication cursor order remains authoritative sequence.

## 62. Archive

Older journal ranges may eventually move to immutable archive storage.

Postgres keeps archive metadata:

```text
start/end sequence
object key
digest
```

Clients below supported history floor rebootstrap.

## 63. Testing

Required integration tests use real PostgreSQL, not mocks alone.

Test:

```text
Tx B crash atomicity
duplicate OperationId race
different-payload OperationId misuse
version conflicts
deadlocks
serialization retries
timeline ordering
pool exhaustion
migration
PITR/epoch transition
snapshot boundary
retention floor
```

## 64. Fault Injection

Inject failures after:

```text
business mutation
version update
journal append
ledger insert
audit insert
before commit
```

Expected result:

```text
no partial authoritative effect
```

## 65. Ambiguous Commit Test

Drop connection around commit and retry same OperationId.

Expected:

```text
one logical effect
stable recovered outcome
```

## 66. Neon Operational Tests

Include:

```text
cold-start latency
connection churn
branch migration rehearsal
restore/epoch procedure
```

## 67. Conformance Profile

Official profile:

```text
PostgresAuthorityFull
```

Neon adds an operational profile:

```text
NeonOperationalProfile
```

without redefining core authority capability.

## 68. PostgreSQL Invariants

### AEQ-INV-PG001

```text
Business mutation, version update, journal append, operation ledger outcome, and required audit commit in one PostgreSQL transaction.
```

### AEQ-INV-PG002

```text
A duplicate OperationId with identical canonical payload returns the prior logical outcome without reapplying business effects.
```

### AEQ-INV-PG003

```text
The same OperationId with a different canonical payload digest is rejected.
```

### AEQ-INV-PG004

```text
Journal cursor order comes from Aequora's committed timeline semantics, not client timestamps or naive SQL sequence allocation.
```

### AEQ-INV-PG005

```text
A PITR or authority restore capable of moving state backward creates a new AuthorityEpoch before synchronization resumes.
```

### AEQ-INV-PG006

```text
SQLx/PostgreSQL-specific types remain inside the adapter boundary.
```

### AEQ-INV-PG007

```text
Correctness-critical database configuration is validated at startup and unsafe settings fail readiness.
```

### AEQ-INV-PG008

```text
External side effects are represented as durable intents committed in Tx B and executed outside the DB transaction.
```

### AEQ-INV-PG009

```text
Journal retention never moves past the minimum safe floor required by active consumers, governance policy, and available bootstrap paths.
```

### AEQ-INV-PG010

```text
Neon autosuspend, scaling, branching, and pooling behavior never change Aequora authority semantics.
```

## 69. Example RON Configuration

```ron
postgres: (
    pool: (
        min_connections: 2,
        max_connections: 20,
        acquire_timeout_ms: 3000,
    ),

    transaction: (
        statement_timeout_ms: 5000,
        lock_timeout_ms: 1000,
        retry_limit: 3,
    ),

    journal: (
        pull_batch_limit: 500,
    ),
)
```

Secrets such as connection passwords remain external.

## 70. Startup

```text
load typed config
↓
resolve secret DSN
↓
connect pool
↓
check PostgreSQL compatibility
↓
verify/apply migrations under migration lock
↓
validate authority/timeline metadata
↓
validate correctness-critical settings
↓
publish capability manifest
↓
become ready
```

## 71. Shutdown

```text
stop admission
↓
drain bounded in-flight work
↓
finish/rollback transactions
↓
stop workers
↓
close pool
```

Forced termination remains safe because durability is transactional.

## 72. Reference Tx Flow

```text
Operation
   |
   v
Idempotency / Ledger
   |
 duplicate? ------> return stored outcome
   |
   v
BEGIN
   |
   v
lock + validate aggregate
   |
   v
execute domain mutation
   |
   v
update version
   |
   v
allocate authoritative sequence
   |
   v
append journal
   |
   v
store ledger outcome
   |
   v
audit + side-effect intents
   |
   v
COMMIT
   |
   v
authoritative response
```

The actual implementation must protect the duplicate race with DB uniqueness/locking inside the transactional path.

## 73. Completion Criteria

```text
[ ] authority transaction defined
[ ] ledger defined
[ ] journal defined
[ ] committed timeline allocator defined
[ ] version/CAS defined
[ ] isolation/retry policy defined
[ ] side-effect outbox defined
[ ] migration strategy defined
[ ] pool/backpressure defined
[ ] tenant isolation defined
[ ] snapshot/retention metadata defined
[ ] PITR/AuthorityEpoch behavior defined
[ ] Neon operational profile defined
[ ] observability/health defined
[ ] conformance and fault testing defined
[ ] PG invariants defined
```

## 74. Final Recommendation

Use PostgreSQL/Neon as Aequora's first authoritative reference adapter because it gives a strong transactional foundation for the most important server invariant:

> **business mutation + version + journal + ledger + required audit must commit together.**

The five concepts that must remain explicit are:

```text
OperationId          -> idempotency
EntityVersion        -> concurrency
AuthorityEpoch       -> timeline continuity
TimelineSequence     -> replication order
PostgreSQL Tx        -> atomic authority
```

If these stay explicit, Neon/PostgreSQL can provide a strong authoritative backend without making the rest of Aequora PostgreSQL-specific.
