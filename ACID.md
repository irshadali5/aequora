# Aequora Sync — ACID Compliance Architecture

## Transactional Correctness, Atomicity Boundaries, Isolation Semantics, Crash Recovery, and Adapter Compliance

> This document defines the ACID architecture for **Aequora Sync**.
>
> It continues the Aequora system and architecture documents and focuses specifically on transactional correctness across:
>
> - local client storage;
> - local outbox;
> - server authoritative storage;
> - authoritative journal;
> - operation ledger;
> - reconciliation;
> - retries;
> - crashes;
> - multiple concurrent clients;
> - database adapters.
>
> The most important architectural rule is:
>
> **Aequora does not pretend that the client database and server database share one distributed ACID transaction.**
>
> Instead, it guarantees:
>
> 1. **ACID transactions inside each database boundary**;
> 2. **atomic coupling of state and synchronization metadata inside that boundary**;
> 3. **idempotent, durable, retry-safe communication between boundaries**;
> 4. **eventual convergence to authoritative server state**.

---

# 1. Why ACID Needs Its Own Architecture

A synchronization engine fails catastrophically if transactional correctness is treated as a small implementation detail.

Examples:

```text
client updates local entity
but crashes before writing outbox
```

Result:

```text
local state exists
server will never know
```

---

```text
server updates authoritative data
but crashes before writing sync journal
```

Result:

```text
server state changed
other clients never receive the change
```

---

```text
server commits operation
network fails before ACK
client retries
server executes operation again
```

Result:

```text
double payment
double inventory decrement
duplicate student
duplicate ledger entry
```

---

Therefore ACID is not merely a property of PostgreSQL or Stoolap.

Aequora must define **transactional invariants around every synchronization boundary**.

---

# 2. ACID Refresher

ACID means:

```text
A = Atomicity
C = Consistency
I = Isolation
D = Durability
```

---

# 3. Atomicity

A transaction either:

```text
fully commits
```

or:

```text
has no visible effect
```

Aequora depends on this for:

```text
local mutation + outbox
server mutation + journal + operation ledger
incoming reconciliation + cursor advancement
```

---

# 4. Consistency

A committed transaction must preserve declared invariants.

Examples:

```text
entity version cannot decrease
payment cannot be posted twice
tenant ownership cannot change illegally
foreign-key-like domain relationships remain valid
double-entry accounting remains balanced
cursor cannot advance beyond applied data
```

Database constraints help, but application/domain validation is also required.

---

# 5. Isolation

Concurrent transactions must not corrupt each other's logical result.

Examples:

```text
two clients update the same invoice
two payments target the same balance
two users allocate the same unique admission number
```

Aequora must combine:

```text
database isolation
+
optimistic entity versions
+
domain conflict rules
```

---

# 6. Durability

After the database reports commit success:

```text
the transaction must survive process failure
```

Aequora must never send a success acknowledgement before the authoritative transaction is durably committed.

---

# 7. ACID Boundaries in Aequora

There are at least three important transactional boundaries.

```text
CLIENT LOCAL DB
    │
    │ network
    ▼
SERVER AUTHORITATIVE DB
    │
    │ network
    ▼
CLIENT LOCAL DB
```

These are separate transactions.

They are not one global transaction.

---

# 8. No Global Distributed Transaction

Do not implement synchronization as:

```text
BEGIN CLIENT TX
BEGIN SERVER TX
perform both
2-phase commit
COMMIT both
```

This would be:

- fragile on mobile/offline clients;
- expensive;
- difficult across arbitrary databases;
- incompatible with disconnected work;
- poor for availability;
- tightly coupled to database capabilities.

Aequora instead uses:

```text
local ACID
+
durable message intent
+
idempotent transport
+
server ACID
+
durable authoritative event
+
client reconciliation ACID
```

---

# 9. Core Transactional Model

The synchronization lifecycle is:

```text
┌──────────────────────────────┐
│ CLIENT TRANSACTION A         │
│                              │
│ local mutation               │
│ + outbox append              │
└─────────────┬────────────────┘
              │ COMMIT
              ▼
        durable intent
              │
              │ retry-safe transport
              ▼
┌──────────────────────────────┐
│ SERVER TRANSACTION B         │
│                              │
│ authoritative mutation       │
│ + version                    │
│ + journal                    │
│ + operation ledger           │
└─────────────┬────────────────┘
              │ COMMIT
              ▼
       authoritative result
              │
              │ response / retry
              ▼
┌──────────────────────────────┐
│ CLIENT TRANSACTION C         │
│                              │
│ authoritative changes        │
│ + ACK state                  │
│ + conflicts                  │
│ + cursor                     │
└──────────────────────────────┘
```

This is the fundamental ACID architecture.

---

# 10. Transaction A — Local Mutation + Outbox

The client must atomically commit:

```text
domain state mutation
+
synchronization intent
```

Example:

```text
BEGIN

UPDATE student
SET phone = ...

INSERT INTO aequora_outbox (...)

COMMIT
```

---

# 11. Why the Outbox Must Be Atomic

Bad sequence:

```text
UPDATE local state
COMMIT

INSERT outbox
```

Crash between them:

```text
local state changed
outbox missing
```

The mutation becomes permanently unsynchronizable.

---

# 12. Reverse Order Is Also Wrong

Bad:

```text
INSERT outbox
COMMIT

UPDATE local state
```

Crash between them:

```text
operation may reach server
local UI never applied corresponding state
```

Now local state and outgoing intent disagree.

---

# 13. Correct Local Invariant

Define:

> **L1: Every committed synchronizable local mutation has exactly one durable corresponding synchronization intent.**

And:

> **L2: No synchronization intent is committed without its matching local mutation unless it is explicitly a server-only or background command.**

---

# 14. Local Transaction API

Do not expect every developer to manually remember this invariant.

Expose an API that makes incorrect usage difficult.

Conceptual Rust:

```rust
pub trait LocalMutationTx {
    async fn mutate<T>(&mut self, change: T) -> Result<(), StoreError>;

    async fn enqueue(
        &mut self,
        operation: OperationEnvelope,
    ) -> Result<(), StoreError>;

    async fn commit(self) -> Result<(), StoreError>;
}
```

Better still, wrap both operations in a domain-facing transaction coordinator.

---

# 15. Compile-Time Transaction Discipline

Prefer APIs where:

```rust
enqueue()
```

requires a transaction context.

Avoid a public API like:

```rust
sync_engine.enqueue(operation).await?;
```

that can run completely independently from the domain transaction.

The local store adapter should help enforce the invariant structurally.

---

# 16. Transaction A Failure Matrix

Case:

```text
domain write fails
```

Result:

```text
transaction rolls back
outbox absent
```

Correct.

---

Case:

```text
outbox write fails
```

Result:

```text
transaction rolls back
domain mutation absent
```

Correct.

---

Case:

```text
process crashes before commit
```

Result:

```text
transaction rolls back/recovery restores pre-transaction state
```

Correct.

---

Case:

```text
commit succeeds
process crashes immediately afterward
```

Result:

```text
domain mutation + outbox survive
```

Correct.

---

# 17. Transaction B — Authoritative Server Commit

When the server accepts an operation, one authoritative transaction should atomically write:

```text
domain state
entity/aggregate version
authoritative journal
operation result ledger
business audit data where required
```

---

# 18. Server Atomicity Invariant

Define:

> **S1: An accepted operation must never become visible in authoritative domain state without a corresponding durable journal event and idempotency record.**

This is one of Aequora's strongest invariants.

---

# 19. Why the Journal Must Commit With Domain State

Bad:

```text
BEGIN
UPDATE authoritative data
COMMIT

INSERT sync journal
```

Crash between them:

```text
authoritative DB changed
clients never receive event
```

This creates permanent divergence.

---

# 20. Why the Operation Ledger Must Commit Together

Bad:

```text
commit business mutation + journal
```

then later:

```text
write applied_operation
```

Crash between them.

Client retries.

Server sees no deduplication record and executes again.

Therefore:

```text
business mutation
+
journal
+
idempotency ledger
```

must commit together.

---

# 21. Authoritative Transaction Template

Canonical model:

```text
BEGIN

1. check OperationId
2. load authoritative aggregate
3. validate preconditions
4. detect conflict
5. execute mutation
6. increment version
7. append authoritative event
8. insert OperationId result
9. append audit record if required

COMMIT
```

Only then may the server acknowledge success.

---

# 22. No Pre-Commit ACK

This is forbidden:

```text
send HTTP 200
then commit DB transaction
```

If commit fails after response:

```text
client believes accepted
server state unchanged
```

Therefore:

```text
commit first
response second
```

---

# 23. The Commit-Then-Network Gap

A distributed system inevitably has this case:

```text
server COMMIT succeeds
↓
network dies before response reaches client
```

This gap cannot be eliminated with ordinary HTTP.

The correct solution is not a distributed transaction.

The solution is:

```text
OperationId idempotency
```

---

# 24. Logical Exactly-Once Effect

Transport guarantees may be:

```text
at-most-once
at-least-once
unknown due to retries
```

Aequora provides:

> **At-least-once delivery with exactly-once logical effect for operations carrying stable OperationIds.**

Client retries same operation.

Server returns stored outcome.

No second business mutation.

---

# 25. Operation Ledger

Logical authoritative record:

```text
operation_id
tenant_id
actor_id
device_id
result_kind
entity_id
entity_version
sequence
response_payload
committed_at
```

`operation_id` must be unique.

---

# 26. Database Constraint for Idempotency

The adapter should enforce a uniqueness constraint equivalent to:

```text
UNIQUE(operation_id)
```

or, if IDs are scoped:

```text
UNIQUE(tenant_id, operation_id)
```

Prefer globally unique OperationIds and still include tenant validation.

---

# 27. Concurrent Duplicate Requests

Two HTTP requests with the same OperationId may arrive simultaneously.

Correct behavior:

```text
request A ─┐
           ├─ database serialization/unique constraint
request B ─┘
```

Exactly one transaction becomes the creator of the result.

The other observes the existing operation ledger result.

---

# 28. Duplicate Race Handling

Possible algorithm:

```text
BEGIN

attempt reserve OperationId
if already exists:
    return committed result

execute business mutation
store result
COMMIT
```

If reservation occurs before execution, state must distinguish:

```text
processing
committed
```

However stale "processing" states complicate crash recovery.

A simpler transactional model often relies on:

```text
single transaction
+
unique operation ledger insertion
+
retry after transaction conflict
```

The PostgreSQL adapter can choose the safest implementation.

---

# 29. Avoid Permanent In-Progress Ledger Rows

Do not write:

```text
operation = processing
COMMIT
```

before business execution unless there is a robust recovery protocol.

Otherwise server crash can leave:

```text
operation permanently "processing"
```

Prefer keeping the operation ledger update within the same transaction as the actual mutation.

---

# 30. Transaction C — Client Reconciliation

After receiving server results, the client must atomically apply:

```text
authoritative changes
outbox acknowledgements
conflict records
inbox/event markers
cursor advancement
```

---

# 31. Cursor Atomicity Invariant

Define:

> **C1: A cursor may advance to sequence N only if every authoritative change through N required by the scope has been durably applied locally.**

---

# 32. Dangerous Reconciliation Order

Bad:

```text
set cursor = 1000
COMMIT

apply changes 900..1000
```

Crash:

```text
cursor says 1000
local data only through 899
```

Those changes may never be requested again.

---

# 33. Correct Reconciliation

```text
BEGIN

apply server changes
mark incoming events processed
update operation acknowledgements
store conflicts
set cursor = N

COMMIT
```

If any step fails:

```text
cursor stays old
```

Retry is safe.

---

# 34. Incoming Event Idempotency

The client should tolerate receiving the same authoritative changes repeatedly.

Ways to enforce this:

```text
sequence + cursor discipline
event_id uniqueness
entity version checks
inbox ledger
```

Do not rely on the network delivering every response only once.

---

# 35. Entity Version Application Rule

For each authoritative change:

```text
incoming_version > local_authoritative_version
    -> apply

incoming_version == local_authoritative_version
    -> duplicate / verify equivalence

incoming_version < local_authoritative_version
    -> stale duplicate / ignore or diagnose
```

Exact behavior depends on optimistic local state representation.

---

# 36. Optimistic State vs Authoritative State

A local-first client may have:

```text
authoritative_version = 10
local optimistic mutation based on 10
```

Therefore local storage may need to distinguish:

```text
server-confirmed state
local optimistic overlay
```

or carry metadata sufficient to rebase pending operations.

---

# 37. Recommended Local Metadata

For synchronized entities, consider:

```text
authoritative_version
local_dirty
pending_operation_count
last_server_sequence
```

Do not necessarily expose this in domain tables if the adapter can keep it separately.

---

# 38. Consistency Has Two Layers

Aequora consistency is:

```text
database consistency
+
domain consistency
```

A database can guarantee a transaction is internally valid while still accepting nonsense business state if application invariants are absent.

---

# 39. Database Consistency Responsibilities

Database constraints can enforce:

```text
primary keys
unique keys
not-null
foreign keys where appropriate
check constraints
transaction atomicity
```

---

# 40. Domain Consistency Responsibilities

Application validators/executors enforce:

```text
invoice cannot be paid twice incorrectly
attendance belongs to valid academic session
student cannot belong to forbidden tenant
journal entry must balance
closed accounting period cannot accept normal mutation
```

---

# 41. Prefer Database Constraints as Backstop

If a domain invariant can also be cheaply and correctly represented as a database constraint, use both.

Example:

```text
domain validator:
admission number unique

database:
UNIQUE(tenant_id, admission_number)
```

Why both?

Because concurrency can invalidate a prior read-based uniqueness check.

The database constraint is the final race-safe enforcement.

---

# 42. Check-Then-Write Race

Bad assumption:

```text
SELECT no existing admission number
```

then:

```text
INSERT
```

Another transaction can insert between those steps.

Therefore final correctness should be guaranteed by:

```text
unique constraint
```

with application-level error translation.

---

# 43. Isolation Architecture

Isolation determines how concurrent transactions observe each other.

Aequora should not assume one universal isolation level for all operations.

---

# 44. Recommended Isolation Strategy

Default server approach:

```text
READ COMMITTED
+
explicit optimistic versions
+
constraints
+
row/aggregate locking where required
```

Use stronger isolation for specific critical operations.

---

# 45. Why Not SERIALIZABLE Everywhere

`SERIALIZABLE` provides strong semantics but may cause:

```text
more retries
higher contention
lower throughput
harder operational behavior
```

It can be valuable for certain finance/inventory workflows, but should not be blindly applied globally.

---

# 46. READ COMMITTED

Useful for common application operations where:

```text
explicit version comparison
+
constraints
```

already protect correctness.

Example:

```text
UPDATE student
WHERE id = ?
AND version = 12
```

If affected rows:

```text
1 -> success
0 -> stale/conflict
```

This is a powerful optimistic concurrency pattern.

---

# 47. Optimistic Compare-And-Swap

Canonical SQL semantics:

```text
UPDATE entity
SET ...,
    version = version + 1
WHERE entity_id = ?
  AND version = expected_version
```

Then:

```text
rows affected = 1
```

means:

```text
version matched
```

Otherwise:

```text
conflict
```

This protects against races between validation and update.

---

# 48. Do Not Validate Version Only in Memory

Bad:

```text
SELECT version = 10
validate
UPDATE entity
```

Without lock or compare-and-swap, another transaction may update between SELECT and UPDATE.

Correct options include:

```text
SELECT ... FOR UPDATE
```

or:

```text
UPDATE ... WHERE version = expected
```

or stronger transaction isolation.

---

# 49. Pessimistic Locking

Use row/aggregate locks when operations require:

```text
read multiple related values
validate invariant
write related values
```

Example:

```text
inventory reservation
account balance workflow
sequence allocation
complex aggregate transition
```

---

# 50. Lock Ordering

If a transaction locks multiple aggregates, define deterministic order.

Example:

```text
sort EntityIds
acquire locks ascending
```

This reduces deadlocks.

---

# 51. Deadlock Handling

Deadlocks can still occur.

Treat database deadlock detection as:

```text
retryable transaction failure
```

provided the operation itself remains idempotent and the entire transaction is retried from the beginning.

---

# 52. Transaction Retry Scope

Never retry only the last SQL statement after a serialization/deadlock error.

Retry:

```text
entire authoritative transaction
```

because prior reads may no longer be valid.

---

# 53. Retry Budget

Transaction retries should be bounded.

Example:

```text
attempt 1
attempt 2
attempt 3
```

with small jitter where useful.

After budget exhaustion:

```text
return retryable server error
```

The client operation remains safely retryable.

---

# 54. Finance/Accounting Isolation

Accounting needs stricter domain architecture.

Do not represent authoritative finance as mutable balances synchronized from clients.

Prefer:

```text
append-only journal entries
immutable posted transactions
explicit reversal entries
derived balances
```

---

# 55. Double-Entry Invariant

A posted journal entry must satisfy:

```text
sum(debits) == sum(credits)
```

This should be validated before commit.

Where possible, include database-level constraints or transaction-local checks as a backstop.

---

# 56. Immutable Posted Entries

After posting:

```text
do not UPDATE ledger row in place
```

Instead:

```text
reverse
+
post corrected transaction
```

This dramatically simplifies synchronization and auditability.

---

# 57. Payment Idempotency

Payment operations need a domain idempotency key in addition to generic sync OperationId when integrating external payment systems.

Example:

```text
Aequora OperationId
+
PSP payment reference
+
business payment identifier
```

This protects against duplicates across system boundaries beyond Aequora.

---

# 58. ACID and External Side Effects

Database transactions cannot roll back:

```text
email already sent
SMS already sent
payment already submitted to external PSP
webhook already delivered
```

Therefore external side effects must not be executed naively inside a database transaction.

---

# 59. Transactional Outbox for External Effects

Use another outbox:

```text
BEGIN

commit business mutation
append sync journal
append notification/payment side-effect intent

COMMIT
```

A background worker later performs the external side effect.

---

# 60. Never Send Email Before Commit

Bad:

```text
send email
then commit invoice
```

If commit fails:

```text
email says invoice created
invoice does not exist
```

Correct:

```text
commit invoice + email intent
then worker sends email
```

---

# 61. External Side Effect Idempotency

The worker must use stable IDs.

Example:

```text
notification_id
webhook_delivery_id
payment_request_id
```

Retries must not create duplicate side effects where provider semantics permit idempotency.

---

# 62. ACID vs Eventual Consistency

Aequora is:

```text
strongly transactional locally
+
eventually consistent across network boundaries
```

These are not contradictory.

---

# 63. Local Strong Consistency

Immediately after local commit:

```text
client database is internally consistent
outbox intent is durable
```

---

# 64. Server Strong Consistency

Immediately after authoritative commit:

```text
server state
journal
operation ledger
versions
```

are transactionally consistent.

---

# 65. Cross-System Eventual Convergence

The client may temporarily differ from server while:

```text
offline
pending
conflicted
retrying
```

Eventually, after successful sync:

```text
local authoritative state converges
```

---

# 66. Consistency Model Must Be Explicit

Document user-visible states:

```text
LocalPending
ServerAccepted
ServerRejected
Conflict
Synchronized
```

Do not present all local data as "server confirmed" when it is merely optimistic.

---

# 67. ACID and Conflict Resolution

A conflict resolution decision must itself be transactionally committed.

Example:

```text
merge operation accepted
```

must atomically produce:

```text
merged authoritative state
new version
journal event
operation result
```

---

# 68. Multi-Entity Operations

Some commands must modify multiple entities atomically.

Example:

```text
transfer student from class A to B
```

If domain semantics require all-or-nothing:

```text
remove from A
add to B
update history
append journal events
```

must occur in one server transaction.

---

# 69. Multi-Aggregate Atomicity

Use only when business semantics truly require it.

Large multi-aggregate transactions increase:

```text
lock scope
contention
deadlock risk
latency
```

Prefer smaller transactional boundaries where invariants allow.

---

# 70. Batch Atomicity Is Not Domain Atomicity

A network batch containing:

```text
100 operations
```

does not mean:

```text
all 100 must commit or all fail
```

Network batching is a transport optimization.

Transactional grouping must be based on domain semantics.

---

# 71. Transaction Group Model

Aequora may assign:

```rust
pub struct TransactionGroupId(Uuid);
```

for explicitly atomic related operations.

But do not infer atomicity merely from request membership.

---

# 72. Dependency Does Not Always Mean Atomicity

Example:

```text
CreateStudent
↓
CreateInvoice
```

B depends on A.

They may:

```text
execute sequentially in separate commits
```

or:

```text
commit together
```

depending on application semantics.

Dependency ordering and atomic transaction grouping are separate concepts.

---

# 73. Transaction Planner

Server execution planning may produce:

```text
ExecutionPlan
├── TxGroup 1
│   ├── op A
│   └── op B
├── TxGroup 2
│   └── op C
└── TxGroup 3
    ├── op D
    └── op E
```

Each group commits independently.

---

# 74. Failed Transaction Group

If one operation in an atomic group fails:

```text
ROLLBACK entire group
```

Responses should identify:

```text
root failure
dependent/transaction-aborted operations
```

---

# 75. ACID and Tombstones

Deletion must be atomic with:

```text
entity deletion/logical tombstone
version increment
journal tombstone event
operation ledger
```

Otherwise clients may resurrect deleted data.

---

# 76. Physical Delete vs Logical Delete

For synchronized entities, prefer:

```text
logical deletion/tombstone first
```

Physical garbage collection happens later after synchronization safety conditions are met.

---

# 77. Tombstone Garbage Collection Transaction

Garbage collection must verify:

```text
retention window passed
required cursor watermarks passed
inactive-device policy satisfied
audit retention permits deletion
```

Then deletion can be committed transactionally.

---

# 78. Snapshot ACID Semantics

A snapshot must represent one coherent authoritative boundary.

The server must not combine arbitrary states from unrelated times without defining semantics.

---

# 79. Snapshot Boundary

Snapshot metadata contains:

```text
boundary cursor = N
```

Meaning:

```text
snapshot represents authoritative state through sequence N
```

Then incremental sync begins at:

```text
N + 1
```

---

# 80. PostgreSQL Snapshot Strategy

Possible adapter implementations include:

```text
repeatable-read transaction
exported snapshot
consistent transaction-local reads
```

The sync core should require only the semantic guarantee:

> **All snapshot chunks correspond to one declared authoritative boundary.**

---

# 81. Snapshot Installation ACID

On the client, a snapshot should install atomically or through staged replacement.

Bad:

```text
delete current local DB
import half snapshot
crash
```

Correct:

```text
download/stage
validate
transactional replace or swap
set cursor
commit
```

---

# 82. Large Snapshot Limitation

Some embedded databases may not efficiently replace millions of rows in one transaction.

Adapters may implement:

```text
staging database/file
shadow tables
generation switch
```

The semantic requirement is atomic visibility, not necessarily one giant physical transaction.

---

# 83. Generation Pointer Pattern

For large local snapshots:

```text
generation A = active
generation B = being built
```

After B is complete:

```text
atomic metadata switch:
active_generation = B
```

Then A can be reclaimed later.

---

# 84. ACID and Schema Migration

Database schema migration itself is part of transactional correctness.

Migrations should be:

```text
versioned
ordered
idempotent where possible
tested against real prior versions
```

---

# 85. Migration Safety

Before changing sync metadata schema, define:

```text
forward migration
rollback policy
client compatibility
server compatibility
journal compatibility
```

---

# 86. Do Not Rewrite Journal Semantics Casually

The authoritative journal is part of replication history.

Changing meaning of old journal payloads without migration/upcasting can break older clients and snapshots.

---

# 87. Local Metadata Migration

Example:

```text
aequora_outbox v1
↓
migration
aequora_outbox v2
```

Pending operations must remain valid across app upgrades.

Never drop pending outbox rows during ordinary migration.

---

# 88. Durable Serialization Compatibility

Persisted operation payloads may survive multiple application versions.

Therefore:

```text
stored payload schema version
```

must be retained with each operation.

The client should be able to either:

```text
send old payload unchanged
```

or:

```text
upcast it safely
```

depending on protocol policy.

---

# 89. Client Crash Recovery

On startup:

```text
open database
recover DB transaction log
load sync metadata
reset stale transient in-memory states
resume pending outbox
```

No manual repair should normally be required.

---

# 90. Stale InFlight Recovery

Because client process may die after marking an operation in-flight:

```text
InFlight older than startup boundary
```

can safely become:

```text
Pending
```

OperationId guarantees resend safety.

---

# 91. Server Crash Recovery

Server process restart should rely on PostgreSQL durability.

Anything committed is authoritative.

Anything uncommitted is rolled back.

Aequora process memory must not be required to determine commit state.

---

# 92. Stateless Correctness

Correctness must survive loss of:

```text
all Axum process memory
all in-memory caches
all Rayon work queues
all active connections
```

because durable truth resides in the database.

---

# 93. Cache Rules

Caches may contain:

```text
decoded schema
operation registry
authorization metadata
recent read state
```

but caches must not become the sole durable record of:

```text
operation committed
cursor position
journal event
outbox intent
```

---

# 94. WAL/Database Durability Assumptions

Aequora depends on adapters honestly reporting commit semantics.

If an embedded DB or server DB can acknowledge commit before durable persistence, adapter documentation must disclose this.

Production adapters should use durability settings appropriate to the application's correctness requirements.

---

# 95. Configurable Durability Modes

Some databases support weaker/faster durability modes.

Aequora may expose adapter-level modes such as:

```text
Strict
Balanced
UnsafePerformance
```

But core synchronization guarantees should only be advertised under compliant modes.

---

# 96. Strict Production Mode

For ERP/finance workloads, default to:

```text
Strict durability
```

Do not silently choose unsafe fsync/synchronous settings for benchmark speed.

---

# 97. ACID Compliance Trait

Adapters should declare capabilities.

Conceptual:

```rust
pub trait TransactionCapabilities {
    fn atomicity(&self) -> AtomicityLevel;
    fn durability(&self) -> DurabilityLevel;
    fn isolation(&self) -> IsolationCapabilities;
    fn supports_savepoints(&self) -> bool;
}
```

This is useful for diagnostics and test gating.

---

# 98. Adapter Compliance Levels

Possible classifications:

```text
FullAuthoritative
FullLocal
ReadOnlyReplica
BestEffortExperimental
```

Production Aequora should require:

```text
FullLocal
```

for writable client adapters and:

```text
FullAuthoritative
```

for server adapters.

---

# 99. Local Adapter Requirements

A production local adapter MUST provide:

```text
atomic domain+outbox transaction support
durable commit
cursor persistence
conflict persistence
transaction rollback
crash recovery
bounded outbox reads
```

---

# 100. Authoritative Adapter Requirements

A production authoritative adapter MUST provide:

```text
atomic domain+journal+ledger transaction
unique operation IDs
transaction rollback
monotonic journal ordering
consistent snapshot support
safe concurrent writes
durable commit
```

---

# 101. ACID Compliance Test Suite

Every adapter should run shared behavioral tests.

Examples:

```text
atomic_commit_all
rollback_all
crash_before_commit
commit_then_restart
duplicate_operation
concurrent_duplicate_operation
cursor_commit_atomicity
journal_atomicity
snapshot_consistency
conflict_compare_and_swap
```

---

# 102. Local Atomicity Test

Test:

```text
begin
write domain state
force outbox failure
commit
```

Expected:

```text
domain state unchanged
outbox unchanged
```

---

# 103. Server Atomicity Test

Test:

```text
begin
mutate domain
append journal
force ledger failure
commit
```

Expected:

```text
no authoritative mutation
no journal event
no ledger record
```

---

# 104. Reconciliation Atomicity Test

Test:

```text
begin
apply 100 events
force cursor update failure
commit
```

Expected:

```text
none of 100 events visible
cursor unchanged
```

or equivalent generation-based atomic visibility.

---

# 105. Durability Test

Test process:

```text
commit operation
terminate process
restart
verify:
    domain state exists
    journal exists
    operation result exists
```

---

# 106. Isolation Test

Two concurrent transactions use same expected version.

Expected:

```text
one succeeds
one receives conflict/retry
```

Never:

```text
both silently overwrite
```

---

# 107. Uniqueness Race Test

Two concurrent requests create the same domain-unique value.

Expected:

```text
one commits
one gets domain/constraint conflict
```

No duplicate records.

---

# 108. Fault Injection Matrix

Inject failure:

```text
before transaction
after first domain write
before journal append
after journal append
before ledger insert
after ledger insert
before commit
after commit
before response
```

Expected state must be specified for every point.

---

# 109. Commit Point

The architecture must define one authoritative commit point:

```text
database commit success
```

Before it:

```text
operation not authoritative
```

After it:

```text
operation authoritative
```

Network response status does not change this fact.

---

# 110. Acknowledgement Semantics

Client ACK state means:

```text
server has durably committed or explicitly rejected the operation
```

It must not mean merely:

```text
request successfully uploaded
```

---

# 111. Rejection Durability

Some rejections do not need permanent server storage.

But if deterministic retry behavior requires the same result for a stable OperationId, storing rejection results may be useful.

Recommended:

```text
store permanent business rejection
do not store transient transport/storage failure as final result
```

---

# 112. Permanent vs Transient Outcome

Permanent:

```text
accepted
business rejected
authorization rejected if policy chooses
schema-invalid operation
```

Transient:

```text
database unavailable
deadlock retry exhausted
server overload
timeout
```

Only permanent outcomes should normally finalize an OperationId.

---

# 113. Authorization Changes Across Retry

Be careful storing authorization rejection forever.

Example:

```text
user lacked permission at 10:00
permission granted at 10:05
same old OperationId retried
```

Policy options:

```text
authorization rejection final for that operation
or
authorization rejection not permanently memoized
```

Recommended default:

```text
business acceptance/rejection may be final
auth/session failures are generally retryable with fresh authorization unless domain policy says otherwise
```

---

# 114. ACID and Time

Wall-clock timestamps must not determine transactional correctness.

Do not rely on:

```text
updated_at
```

for ordering or conflict detection.

Use:

```text
entity version
server sequence
OperationId
```

Time remains metadata.

---

# 115. HLC Role

Hybrid Logical Clocks can help causal metadata and user-visible ordering.

They do not replace:

```text
database transaction isolation
authoritative sequence
entity version
```

---

# 116. ACID and Rayon

Rayon must never own transactional correctness.

Correct model:

```text
parallel CPU preparation
↓
deterministic transaction plan
↓
database transaction
```

---

# 117. Do Not Share DB Transactions Across Rayon Threads

Database transaction objects are often not safe or appropriate for arbitrary parallel access.

Avoid:

```text
par_iter().for_each(|op| tx.write(op))
```

Use Rayon before the write phase.

---

# 118. Parallel Validation

Safe candidates:

```text
pure structural validation
hashing
schema conversion
independent deterministic calculations
```

Anything requiring authoritative state or shared transactional mutation should use carefully designed async/database coordination.

---

# 119. Deterministic Execution

Parallel preprocessing must not alter business outcome.

Given the same:

```text
authoritative state
operation set
dependency graph
```

execution planning should be deterministic.

This makes testing and conflict debugging much easier.

---

# 120. ACID and HTTP

HTTP success/failure is not the transaction boundary.

Examples:

```text
HTTP timeout
```

does not imply:

```text
server rolled back
```

The server may have committed.

That is why the client must retry the same OperationId.

---

# 121. HTTP 500 After Commit

Avoid generating an error after commit due to nonessential post-commit work.

Example:

```text
commit succeeds
logging formatter panics
response becomes 500
```

Client will retry, which is safe due to idempotency, but operationally noisy.

Keep post-commit response generation simple and robust.

---

# 122. Response Reconstruction

Where useful, operation ledger stores enough result data to reconstruct the same logical ACK after retry.

This is especially important if:

```text
server committed
response lost
```

---

# 123. ACID and Notifications

Server push/WebSocket notifications are hints only.

They must not be correctness-critical.

If notification is lost:

```text
client later pulls journal using cursor
```

and still converges.

---

# 124. Journal Is the Durable Replication Source

The authoritative journal, not transient push notifications, is the durable source for downstream client updates.

---

# 125. Journal Ordering

For a given synchronization scope, journal sequence must define a total order sufficient for incremental consumption.

This can be:

```text
strictly increasing integer
```

per scope or a compatible internal scheme hidden behind the adapter.

---

# 126. Journal Gaps

Sequence gaps are acceptable if semantics define:

```text
sequence is monotonic
not necessarily dense
```

Clients should request:

```text
> cursor
```

rather than assume every integer exists.

---

# 127. Cursor Advancement With Filtered Events

If scope filtering means the client receives only some global journal events, cursor semantics must still be safe.

Better:

```text
scope-specific cursor
```

or:

```text
server confirms processed watermark including filtered events
```

Do not let filtered-out global events create ambiguity.

---

# 128. ACID and Partial Sync

Changing sync scope is a consistency event.

Client must not keep applying an old cursor under a new scope unless the server explicitly supports that transition.

Initial safe policy:

```text
scope changed
↓
new bootstrap
```

---

# 129. Multi-Tenant Transactions

A normal operation should not span tenants.

Tenant is part of the transaction boundary and authorization context.

Cross-tenant administrative operations should be exceptional and explicitly designed.

---

# 130. Tenant Constraint Backstop

Where practical, database keys and unique constraints should include tenant identity.

Example:

```text
UNIQUE(tenant_id, admission_number)
```

This reduces accidental cross-tenant collisions and leakage.

---

# 131. ACID and Audit

For regulated/financial actions:

```text
business mutation
+
audit record
+
sync journal
+
operation ledger
```

should commit atomically where audit completeness is required.

---

# 132. Audit Record Immutability

Audit logs should normally be append-only.

Corrections create new records rather than mutating historical ones.

---

# 133. ACID and Soft Deletes

If the application uses soft deletes:

```text
deleted_at
```

the sync journal must still emit an explicit tombstone/change event.

Do not assume clients will infer deletion from absence.

---

# 134. ACID and Referential Integrity

Client-created UUIDs allow related offline objects to preserve references before server contact.

Server transaction then validates relationships.

Example:

```text
Invoice references StudentId
```

must fail atomically if authoritative rules prohibit that relationship.

---

# 135. Deferred Dependencies

If a dependent operation arrives before its prerequisite becomes committed, possible outcomes:

```text
reject dependency missing
defer and retry later
same-batch reorder
```

Whatever policy is used must not partially commit dependent state.

---

# 136. Savepoints

Savepoints can be useful inside complex server transaction groups.

Example:

```text
BEGIN
SAVEPOINT operation_a
...
ROLLBACK TO operation_a
```

But do not rely on savepoints to fake independent atomicity semantics if the application cannot explain partial success.

---

# 137. Recommended Batch Policy

Default:

```text
independent operations:
    independent transactions

explicit atomic domain group:
    one transaction

dependency chain:
    ordered execution;
    atomicity chosen by domain policy
```

---

# 138. Read Models

Materialized/read-model tables may be updated:

```text
inside authoritative transaction
```

or:

```text
asynchronously from journal
```

depending on consistency needs.

If UI/API correctness requires immediate read-after-write consistency, update the read model in the same transaction.

---

# 139. Asynchronous Projections

For noncritical projections:

```text
authoritative event committed
↓
projection worker
↓
analytics/search/cache update
```

Projection failure must not roll back already committed authoritative data.

The journal enables replay.

---

# 140. ACID and Cache Invalidation

Do not make cache mutation part of the core transaction unless the cache itself supports the necessary semantics.

Pattern:

```text
commit DB
↓
publish invalidation hint
```

Cache miss then reloads authoritative state.

---

# 141. Post-Commit Hooks

Post-commit hooks may perform:

```text
metrics
notifications
cache invalidation
wake workers
```

But they must not define whether the business operation committed.

---

# 142. Transaction Context Type

A useful internal API:

```rust
pub struct TransactionContext<'a> {
    pub auth: &'a AuthContext,
    pub operation_id: OperationId,
    pub now: HybridTimestamp,
}
```

The authoritative transaction executor receives this context and a transaction capability.

---

# 143. No Hidden Nested Transactions

Avoid repository methods that silently start and commit their own transaction when already called inside a parent authoritative transaction.

Prefer passing:

```text
&mut transaction
```

through repository operations.

---

# 144. Transaction Ownership

One layer should own commit/rollback responsibility.

Recommended:

```text
Sync transaction orchestrator
```

owns:

```text
begin
commit
retry
rollback-by-drop
```

Domain handlers perform writes but do not commit independently.

---

# 145. Panic Safety

A panic during transaction execution must not accidentally commit partial state.

Rust unwinding/drop should leave the database transaction uncommitted, and adapter tests should verify rollback behavior.

Production services should also isolate panics at request/task boundaries.

---

# 146. Cancellation Safety

Async request cancellation can happen because:

```text
client disconnects
timeout fires
task cancelled
```

If cancellation occurs before commit:

```text
transaction must roll back
```

If commit completed:

```text
operation is authoritative even if response is abandoned
```

---

# 147. Do Not Couple Commit to Socket Lifetime

The server should not assume:

```text
client disconnected -> transaction did not commit
```

Once transaction processing begins, commit semantics are database-controlled.

OperationId handles ambiguity.

---

# 148. Timeout Semantics

A server timeout after uncertain commit should not cause the server to invent a failure result.

If commit status is uncertain at application level, retry/deduplication on the next request resolves it against durable state.

---

# 149. Connection Pool Exhaustion

DB pool exhaustion is:

```text
transient infrastructure failure
```

No operation should be marked permanently rejected.

Return retryable error.

---

# 150. Serialization Failures After Execution

All validation/decoding should occur before mutation.

However response serialization can still fail after commit.

This is safe if:

```text
operation ledger contains result
```

Client retry retrieves committed outcome.

---

# 151. Precompute Response Metadata

Where practical, compute deterministic response payload before final commit or store enough result inside the transaction so post-commit reconstruction is guaranteed.

---

# 152. ACID and Protocol Upgrades

A server must never partially execute an operation whose schema it cannot safely interpret.

Compatibility validation occurs before transaction execution.

---

# 153. Unsupported Operation Kind

Result:

```text
permanent protocol/schema rejection
```

No domain mutation.

No authoritative journal mutation.

---

# 154. Malformed Payload

Malformed Postcard/domain payload must be rejected before opening expensive write transactions where possible.

---

# 155. Transaction Duration

Keep transactions short.

Do not perform inside an open DB transaction:

```text
network requests
email sending
large file uploads
slow compression
external API calls
user interaction
long Rayon jobs
```

---

# 156. Transaction Phase Separation

Recommended server phases:

```text
decode
↓
cheap validate
↓
authorize
↓
CPU preparation
↓
BEGIN TX
↓
authoritative reads
↓
final conflict/precondition checks
↓
writes
↓
journal + ledger
↓
COMMIT
↓
encode response
```

---

# 157. Why Final Validation Must Be In-Transaction

Some validation can happen before opening a transaction.

But concurrency-sensitive checks must happen inside the transaction or be enforced by atomic database writes/constraints.

Examples:

```text
current version
current available inventory
unique key still free
period still open
```

---

# 158. Two-Stage Validation

Use:

```text
Stage A:
pure/cheap prevalidation

Stage B:
transactional authoritative validation
```

This minimizes transaction duration without sacrificing correctness.

---

# 159. PostgreSQL Recommended Patterns

Adapter should favor:

```text
parameterized statements
unique constraints
foreign keys where useful
CHECK constraints
UPDATE ... WHERE version = expected
SELECT ... FOR UPDATE for complex invariants
transaction retry on serialization/deadlock
```

---

# 160. Client Embedded DB Patterns

Client adapter must verify its DB supports:

```text
multi-statement transactions
rollback
durable commit
read-your-writes
consistent recovery
```

If not, it cannot qualify as a full writable Aequora local adapter.

---

# 161. Stoolap Role

For Stoolap-based clients, Aequora should encapsulate all Stoolap-specific transaction details inside:

```text
aequora-store-stoolap
```

The rest of the sync engine consumes only local store traits.

---

# 162. PostgreSQL/Neon Role

For server deployments, Aequora should treat Neon as:

```text
PostgreSQL-compatible authoritative persistence
```

The Aequora transactional contract attaches to PostgreSQL semantics, not provider branding.

---

# 163. ACID Health Checks

Add diagnostics that verify basic adapter health:

```text
can begin transaction
can rollback
can commit
journal writable
operation ledger writable
cursor query operational
```

Do not mutate real domain state in ordinary liveness probes.

---

# 164. Readiness vs Liveness

Liveness:

```text
process is alive
```

Readiness:

```text
process can currently serve sync traffic
DB connectivity available
critical migrations applied
```

Keep them separate.

---

# 165. Migration Gate

Axum server must not accept synchronization writes before required database migrations are applied.

Fail readiness instead of serving with incompatible metadata schema.

---

# 166. ACID Metrics

Important metrics:

```text
transaction_commit_total
transaction_rollback_total
transaction_retry_total
serialization_failure_total
deadlock_total
dedup_hit_total
journal_append_failure_total
reconciliation_rollback_total
cursor_commit_failure_total
```

---

# 167. Transaction Tracing

A server operation span may include:

```text
operation_id
tenant_id
entity_type
expected_version
transaction_attempt
commit_outcome
```

Never log sensitive payload by default.

---

# 168. Debugging Transaction Failures

Every durable operation should be traceable via:

```text
OperationId
```

Across:

```text
client outbox
HTTP request trace
server transaction
operation ledger
journal
client ACK
```

This ID becomes the primary distributed debugging key.

---

# 169. Recovery Tooling

`aequora-cli` can provide read-only diagnostics:

```text
aequora operation inspect <id>
aequora journal inspect --after N
aequora outbox inspect
aequora cursor inspect
aequora conflict inspect
```

Avoid casual manual mutation commands in production tooling.

---

# 170. Manual Repair Philosophy

If corruption is detected:

```text
do not silently "fix" by advancing cursor or deleting operation rows
```

Prefer:

```text
quarantine
diagnose
re-bootstrap scope
or execute explicit repair command
```

Correctness beats hiding inconsistency.

---

# 171. Invariant Verification Jobs

Optional background verification can check:

```text
journal events reference valid operations
operation ledger sequence exists
entity version monotonicity
cursor generation validity
no impossible outbox state transitions
```

These are diagnostic, not a replacement for transaction guarantees.

---

# 172. Production ACID Checklist

Before shipping an adapter/system:

```text
[ ] local mutation+outbox atomic
[ ] authoritative mutation+journal+ledger atomic
[ ] reconciliation+cursor atomic
[ ] duplicate operations safe
[ ] concurrent duplicates safe
[ ] version races safe
[ ] unique-key races safe
[ ] crash-before-commit safe
[ ] crash-after-commit safe
[ ] request cancellation safe
[ ] response loss safe
[ ] transaction retries safe
[ ] external side effects use outbox
[ ] snapshot boundary consistent
[ ] migration preserves pending operations
[ ] adapter compliance suite passes
[ ] durability mode documented
[ ] finance invariants tested where relevant
```

---

# 173. Recommended Aequora ACID Layers

The implementation can be organized as:

```text
aequora-acid
├── invariant.rs
├── transaction.rs
├── retry.rs
├── idempotency.rs
├── outcome.rs
├── compliance.rs
└── error.rs
```

However, avoid creating a separate crate unless multiple existing crates genuinely need shared ACID abstractions.

It may be cleaner to keep:

```text
core transaction contracts in aequora-store
idempotency in aequora-server
compliance suite in aequora-testkit
```

and treat this document as the architectural specification.

---

# 174. ACID State Transition

The full system is:

```text
LOCAL DOMAIN COMMAND
       │
       ▼
┌──────────────────────────┐
│ CLIENT ACID TX           │
│ domain mutation          │
│ + outbox                 │
└────────────┬─────────────┘
             │
             ▼
      durable pending op
             │
             │ retry until known outcome
             ▼
┌──────────────────────────┐
│ SERVER ACID TX           │
│ dedup check              │
│ validation               │
│ domain mutation          │
│ version                  │
│ journal                  │
│ operation ledger         │
└────────────┬─────────────┘
             │
             ▼
     durable authority
             │
             │ response may be lost
             ▼
┌──────────────────────────┐
│ CLIENT ACID TX           │
│ authoritative changes    │
│ operation result         │
│ conflict state           │
│ cursor                   │
└────────────┬─────────────┘
             │
             ▼
      converged client
```

---

# 175. What ACID Means for Aequora

Aequora should make this guarantee:

> **If a local database adapter and authoritative database adapter satisfy Aequora's transactional contracts, then crashes, retries, duplicated requests, connection loss, and process restarts must not create partially synchronized authoritative mutations.**

This does **not** mean:

```text
client and server commit simultaneously
```

It means:

```text
each boundary is atomic
and ambiguity between boundaries is resolved safely through durable intent, idempotency, and replay.
```

---

# 176. Final Architectural Rules

The ACID architecture can be reduced to twelve rules.

## Rule 1

Never mutate synchronizable local state without atomically writing sync intent.

## Rule 2

Never mutate authoritative state without atomically writing its replication journal event.

## Rule 3

Never commit an accepted authoritative operation without atomically writing its OperationId result.

## Rule 4

Never advance a client cursor before corresponding authoritative changes are durably applied.

## Rule 5

Never assume an HTTP timeout means rollback.

## Rule 6

Always retry using the same OperationId.

## Rule 7

Use database constraints and compare-and-swap as concurrency backstops.

## Rule 8

Keep external side effects outside DB transactions and represent them through transactional outboxes.

## Rule 9

Keep transactions short and free from network/external I/O.

## Rule 10

Retry complete transactions after serialization/deadlock failures.

## Rule 11

Require every database adapter to pass the same ACID compliance suite.

## Rule 12

Do not fake distributed ACID across offline client and server databases; use local ACID plus durable eventual convergence.

---

# 177. Final Recommendation

For the first production Aequora deployment:

```text
Client:
Stoolap
    ↓
ACID local domain + outbox transaction

Transport:
Postcard over HTTPS
    ↓
at-least-once retry

Server:
Axum
    ↓
validation/execution
    ↓
PostgreSQL/Neon
    ↓
ACID authoritative state + version + journal + operation ledger

Return:
Postcard
    ↓
client ACID reconciliation + cursor transaction
```

This architecture gives the correct distributed guarantee without coupling Aequora to one database engine or requiring distributed two-phase commit.

The most important implementation effort should go into:

```text
transaction boundaries
idempotency
version compare-and-swap
journal atomicity
cursor atomicity
failure injection
adapter compliance testing
```

Those are the mechanisms that turn Aequora from "data syncing code" into a reliable synchronization engine.

---

# 178. Implementation Status

The synchronization-core ACID requirements in sections 1–177 are implemented and mapped to
concrete transaction contracts, adapter behavior, failure/concurrency tests, diagnostics, and
release gates in `docs/acid-compliance.md`.

The implementation now includes explicit adapter durability/compliance declarations, invalid
version-transition rejection, concurrent operation-ID and entity-creation race tests, fixed-order
PostgreSQL operation/entity locking, bounded whole-transaction retry for serialization/deadlock,
real Stoolap rollback/restart/snapshot-install tests, durable client retry deadlines, and
payload-free transaction outcome metrics.

Finance balancing and application external-effect outboxes remain application-owned conditional
requirements. Hosted backup/restore and load acceptance remain deployment gates and are not
claimed without the target environment.
