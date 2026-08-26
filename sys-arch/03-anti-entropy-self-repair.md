# Aequora Sync — Part 03

# Anti-Entropy, Integrity Verification, Divergence Detection, and Self-Repair Architecture

## 1. Purpose

Aequora already provides:

```text
operation idempotency
authoritative journal
cursor-based replication
ACID transaction boundaries
conflict handling
bootstrap snapshots
formal correctness architecture
causality and provenance
```

Those mechanisms greatly reduce divergence, but they do not eliminate every possible source of corruption.

Real systems can still experience:

```text
application bugs
adapter bugs
disk corruption
incorrect migrations
manual database edits
failed recovery
bad historical versions
implementation defects
unexpected partial legacy imports
```

Therefore Aequora needs an **anti-entropy and integrity subsystem** capable of discovering and repairing silent divergence even when ordinary incremental synchronization appears healthy.

The central rule is:

> **Cursor equality does not prove state equality.**

A client can hold:

```text
cursor = 100000
```

while still having corrupted or missing state.

Anti-entropy verifies the state itself.

---

# 2. Goals

The anti-entropy architecture should provide:

```text
silent divergence detection
integrity verification
partition-level comparison
entity-level localization
safe repair
corruption quarantine
low network overhead
low CPU overhead
background scheduling
manual verification
operator diagnostics
```

---

# 3. Non-Goals

Anti-entropy is not:

```text
normal replication
a substitute for ACID
a replacement for cursor sync
a universal distributed consensus system
```

Normal sync remains:

```text
operation push
+
journal pull
```

Anti-entropy is a verification and repair layer.

---

# 4. Why Incremental Sync Alone Is Insufficient

Suppose:

```text
server event 500 updates Student A
client receives event 500
client transaction commits incorrectly due to adapter bug
cursor still advances to 500
```

Now:

```text
server Student A = version 8
client Student A = version 7
client cursor = 500
```

Normal pull sees:

```text
no events > 500
```

The divergence persists forever.

Anti-entropy discovers it.

---

# 5. Integrity Layers

Use layered verification:

```text
L1 — metadata invariants
L2 — entity digest
L3 — partition digest
L4 — Merkle tree
L5 — canonical snapshot comparison
```

The runtime should select the cheapest sufficient layer.

---

# 6. Canonical Digest

Integrity hashes must be computed from a **canonical representation**, not raw database pages or storage-specific row bytes.

Otherwise:

```text
PostgreSQL encoding
≠
Stoolap encoding
```

would generate meaningless mismatches.

---

# 7. Canonical Entity Digest

Conceptually:

```rust
pub struct EntityDigest {
    pub entity: EntityRef,
    pub version: EntityVersion,
    pub hash: Digest,
}
```

Hash input should include:

```text
entity type
entity ID
authoritative version
canonical synchronizable payload
tombstone state
schema version
```

---

# 8. Exclude Local-Only State

Do not include:

```text
UI caches
local search index
temporary flags
pending optimistic metadata
local timestamps
```

unless they are part of authoritative synchronized semantics.

Anti-entropy compares authoritative synchronized state.

---

# 9. Canonical Serialization

Preferred integrity pipeline:

```text
typed authoritative state
↓
canonical serialization
↓
BLAKE3 digest
```

Postcard may be used only if the encoding is stable for the exact canonical schema/version.

Safer approach:

```text
explicit canonical digest writer
```

for critical entities.

---

# 10. Digest Domain Separation

Never hash arbitrary concatenated bytes without domain separation.

Conceptually:

```text
"AEQUORA:ENTITY:v1"
+
entity_type
+
entity_id
+
version
+
payload
```

This prevents accidental hash-domain ambiguity.

---

# 11. Partition Digest

Checking every entity individually is expensive.

Group entities into deterministic partitions.

Example:

```text
tenant
scope
entity type
hash bucket
```

Partition digest:

```rust
pub struct PartitionDigest {
    pub partition: PartitionId,
    pub generation: IntegrityGeneration,
    pub entity_count: u64,
    pub root_hash: Digest,
}
```

---

# 12. Stable Partitioning

Partition assignment must be deterministic.

Example:

```text
bucket = hash(EntityId) mod 1024
```

Both client and server then compute matching buckets independently.

---

# 13. Avoid Table-Order Dependency

Digest result must not depend on database row ordering.

Sort canonical entries by:

```text
EntityRef
```

or use order-independent tree construction with explicit deterministic rules.

---

# 14. Merkle Tree Architecture

For large datasets, use Merkle trees.

```text
                   Root
              /             \
            A                 B
         /     \           /     \
       A1      A2        B1      B2
```

If root matches:

```text
entire covered dataset matches
```

If root differs:

```text
compare children
```

until mismatching leaf ranges are located.

---

# 15. Why Merkle Trees

Without Merkle:

```text
compare 10 million entities
```

With Merkle:

```text
compare one root
↓
only descend into mismatched branches
```

This drastically reduces repair traffic.

---

# 16. Merkle Scope

A Merkle tree must be scoped.

Recommended dimensions:

```text
tenant
sync scope
entity family
integrity generation
```

Do not build one enormous global tree for unrelated tenants.

---

# 17. Integrity Generation

Define:

```rust
pub struct IntegrityGeneration(u64);
```

Generation changes when:

```text
canonical hash algorithm changes
canonical schema changes incompatibly
partitioning changes
integrity format changes
```

Never compare digests from incompatible generations.

---

# 18. Integrity Manifest

Server can expose:

```rust
pub struct IntegrityManifest {
    pub scope: ScopeId,
    pub integrity_generation: IntegrityGeneration,
    pub boundary: Cursor,
    pub root_hash: Digest,
    pub partition_count: u32,
}
```

---

# 19. Boundary Cursor

Integrity comparison must correspond to a known authoritative boundary.

Example:

```text
root hash valid through cursor 90000
```

Client may have:

```text
cursor = 90500
```

Then comparison needs careful handling.

---

# 20. Simplest Correct Anti-Entropy Model

Use a synchronization barrier:

```text
client first catches up
↓
server returns current anti-entropy boundary N
↓
client verifies state representing N
```

If new events arrive during verification:

```text
verification remains against boundary N
```

then normal sync continues afterward.

---

# 21. Stable Verification Snapshot

Server adapter should provide a consistent state view corresponding to:

```text
cursor N
```

This may use:

```text
MVCC snapshot
snapshot manifest
precomputed digest tree
```

---

# 22. Precomputed vs On-Demand Digests

Two modes:

## On-Demand

Compute when requested.

Good for:

```text
small deployments
low verification frequency
```

## Incremental/Precomputed

Maintain digest structures as state changes.

Good for:

```text
large datasets
large fleets
frequent verification
```

---

# 23. Start Simple

Initial implementation should use:

```text
partition digests
+
on-demand canonical hashing
```

before introducing sophisticated incremental Merkle maintenance.

---

# 24. Verification Trigger

Anti-entropy can run:

```text
periodically
after bootstrap
after database migration
after corruption suspicion
after long offline period
after restore/PITR
manually
```

---

# 25. Do Not Run Too Frequently

Anti-entropy can be expensive.

Normal synchronization already provides strong consistency.

A reasonable default might be:

```text
daily
weekly
after major events
```

depending on dataset size and risk.

The exact cadence belongs to configuration.

---

# 26. Adaptive Frequency

Increase verification frequency if:

```text
recent repair occurred
adapter is experimental
device suffered unclean shutdown
migration recently completed
```

Decrease frequency on healthy stable devices.

---

# 27. Verification State Machine

```text
Idle
 ↓
CatchUp
 ↓
AcquireBoundary
 ↓
CompareRoot
 ├─ Match → Verified
 └─ Mismatch
       ↓
   ComparePartitions
       ↓
   LocalizeMismatch
       ↓
   RepairPlan
       ↓
   Repair
       ↓
   Reverify
```

---

# 28. Anti-Entropy Request

Conceptually:

```rust
pub struct IntegrityRequest {
    pub scope: ScopeId,
    pub cursor: Cursor,
    pub integrity_generation: Option<IntegrityGeneration>,
    pub client_root: Option<Digest>,
}
```

---

# 29. Fast Match

Client sends root.

Server:

```text
root equal
```

returns:

```text
IntegrityStatus::Match
```

No further data transfer needed.

---

# 30. Mismatch Response

Server may return:

```text
mismatching partition IDs
```

or request child hashes.

Do not immediately send the entire snapshot.

---

# 31. Partition Walk

Protocol:

```text
client partition hashes
↓
server compares
↓
returns mismatched partitions
↓
client sends child hashes / leaf summaries
↓
server localizes entities
```

---

# 32. Leaf Granularity

A leaf may represent:

```text
one entity
small deterministic range
small bucket
```

Choose based on dataset scale.

---

# 33. Repair Strategy

Once mismatch is localized, server remains authoritative.

Possible repair actions:

```text
replace entity from server
delete stale local entity
install tombstone
refresh partition
partial snapshot
full bootstrap
```

---

# 34. Never Let Client Repair Server Automatically

Anti-entropy is not bidirectional authority arbitration.

If client has unexpected data:

```text
client may have pending operations
or corruption
```

Do not upload raw state to overwrite server.

Server authority remains unchanged.

---

# 35. Pending Operation Preservation

Before repairing local state, identify pending operations touching affected entities.

Example:

```text
local Student version 8 optimistic
server authoritative version 7
pending UpdateStudentPhone
```

Replacing local entity blindly could destroy optimistic user work.

---

# 36. Authoritative Base + Optimistic Overlay

Best local model:

```text
authoritative base state
+
pending operations
=
effective local state
```

Then anti-entropy repairs:

```text
authoritative base
```

and replays/rebases pending operations.

---

# 37. If Local DB Does Not Separate Overlay

The reconciler must:

```text
capture pending operations
↓
replace authoritative state
↓
reapply optimistic intent
```

carefully.

This should be tested extensively.

---

# 38. Repair Plan

Create explicit:

```rust
pub struct RepairPlan {
    pub repair_id: RepairId,
    pub boundary: Cursor,
    pub affected_entities: Vec<EntityRef>,
    pub strategy: RepairStrategy,
}
```

---

# 39. Repair Strategies

```rust
pub enum RepairStrategy {
    ReplaceEntities,
    ReplacePartition,
    BootstrapScope,
    Quarantine,
}
```

---

# 40. Replace Entity

Use when:

```text
few mismatches
```

Server sends canonical authoritative entity snapshot.

Client transaction:

```text
replace authoritative base
preserve/rebase pending ops
mark repair
commit
```

---

# 41. Replace Partition

Use when:

```text
many entities in one partition mismatch
```

More efficient than individual replacement.

---

# 42. Full Bootstrap

Use if:

```text
widespread corruption
integrity generation incompatible
mapping version incompatible
repair cost too high
```

---

# 43. Quarantine

Use when:

```text
local corruption cannot be safely interpreted
pending operations malformed
adapter reports storage corruption
```

Quarantine prevents further automatic mutation until safe recovery.

---

# 44. Repair Transaction

Repair must be atomic locally.

```text
BEGIN

install authoritative repair data
update integrity metadata
preserve/rebase pending operations
update repair state

COMMIT
```

Cursor should not move incorrectly.

---

# 45. Repair Is Not a New Business Event

Repairing local replica state does not create authoritative journal events.

It is replica maintenance.

---

# 46. Repair Provenance

Still record locally:

```text
RepairId
reason
boundary cursor
affected entities
integrity mismatch
time
```

for diagnostics.

---

# 47. Server Repair Audit

Server may log:

```text
device requested repair
partitions mismatched
repair delivered
```

No business audit event required unless organization policy demands it.

---

# 48. Divergence Classification

Classify mismatches:

```text
MissingLocal
ExtraLocal
PayloadMismatch
VersionMismatch
TombstoneMismatch
SchemaMismatch
UnknownEntity
CorruptPayload
```

---

# 49. Missing Local

Server has authoritative entity.

Client lacks it.

Repair:

```text
install entity
```

---

# 50. Extra Local

Client has an authoritative-marked entity not present on server.

Possible causes:

```text
stale delete
corruption
pending unsynced local creation
```

Need pending-operation analysis before removal.

---

# 51. Version Mismatch

Same entity ID but versions differ.

If client has newer **authoritative** version than server:

```text
serious anomaly
```

Potential causes:

```text
timeline mismatch
restore
corruption
wrong authority generation
```

Do not blindly overwrite.

---

# 52. Payload Mismatch With Same Version

This is especially serious.

Invariant:

```text
same entity
same authoritative version
different canonical payload
```

indicates:

```text
corruption
nondeterministic serialization
adapter bug
manual edit
```

Default action:

```text
quarantine or authoritative replacement + diagnostic
```

---

# 53. Tombstone Mismatch

Client believes active.

Server has tombstone.

Repair should remove/mark deleted locally while preserving any stale pending operations as explicit conflicts.

---

# 54. Integrity Metadata

Client stores:

```text
last_verified_cursor
last_verified_generation
last_verified_root
last_verified_at
last_repair_id
```

---

# 55. Verification Does Not Replace Cursor

Keep:

```text
sync cursor
```

and:

```text
integrity verification metadata
```

separate.

---

# 56. Server Digest Cache

Server may cache:

```text
partition digest at boundary
```

if many clients request the same scope.

Cache correctness must be keyed by:

```text
scope
boundary
integrity generation
```

---

# 57. Incremental Merkle Maintenance

Future optimization:

When authoritative entity changes:

```text
recompute leaf
↓
recompute ancestor hashes
```

This makes root update:

```text
O(log N)
```

instead of rescanning all entities.

---

# 58. Transactional Merkle Update

If Merkle metadata becomes part of correctness, update it atomically with authoritative mutation/journal.

However this increases transaction complexity.

Alternative:

```text
asynchronous derived Merkle tree
```

with explicit boundary lag.

Initial recommendation:

```text
derived verification index
```

not critical transaction state.

---

# 59. Verification Index Lag

If anti-entropy index is asynchronous:

```text
index boundary = 1000
server current = 1050
```

Client verifies against:

```text
1000
```

then continues journal sync.

This is safe if boundary is explicit.

---

# 60. Integrity Worker

Background server worker may build:

```text
partition hashes
Merkle roots
snapshot hashes
```

from authoritative journal.

It has its own consumer cursor.

---

# 61. Consumer Idempotency

Integrity worker must process events idempotently.

Derived digest state can be rebuilt from authoritative data if needed.

---

# 62. Hash Algorithm

Recommended:

```text
BLAKE3
```

for performance and strong integrity properties.

Algorithm must be encoded in:

```text
IntegrityGeneration / manifest
```

so future changes are possible.

---

# 63. Cryptographic Scope

This detects accidental or malicious data changes where hash source is trusted.

It does not automatically provide:

```text
authenticated remote proof
```

unless manifests are signed.

Optional signing belongs to the later cryptographic-integrity architecture.

---

# 64. Canonical Hash Version

Define:

```text
HashSchemaVersion
```

separately from:

```text
application schema
```

because canonicalization rules may evolve.

---

# 65. Field Ordering

Canonical hash must use stable field ordering.

For generic objects:

```text
sort by stable FieldId
```

not display names.

---

# 66. Optional Field Semantics

Canonicalization must distinguish:

```text
missing field
null field
defaulted field
```

according to schema semantics.

Otherwise two logically distinct states may hash equal or vice versa.

---

# 67. Decimal Hashing

Hash exact canonical decimal representation.

Never hash formatted locale string.

---

# 68. Timestamp Hashing

Hash canonical timestamp precision and normalized value.

Avoid database-specific textual formatting.

---

# 69. Blob Hashing

For large blobs:

```text
hash BlobRef/content digest
```

not the full blob bytes during every entity anti-entropy pass.

Blob subsystem can verify content separately.

---

# 70. Relationship Hashing

For aggregate snapshots, hash canonical relationships that are part of synchronized aggregate state.

Do not accidentally omit child entities that affect business meaning.

---

# 71. Aggregate Digest

Some domains should hash aggregate root plus children as one integrity unit.

Example:

```text
Invoice
+ line items
+ adjustments
```

This aligns with conflict/version semantics.

---

# 72. Record Digest vs Aggregate Digest

Support:

```text
EntityDigest
AggregateDigest
```

through application policy.

Do not force one model globally.

---

# 73. Anti-Entropy Scope Resolver

Server uses same authorization/scope rules as normal sync.

Client cannot request digest information for unauthorized data.

---

# 74. Digest Information Leakage

Even hashes/counts can reveal metadata.

Therefore integrity endpoints must require normal authenticated tenant/scope authorization.

---

# 75. Count Leakage

`entity_count` may be sensitive in some deployments.

Allow policy to omit it from client-visible manifests if unnecessary.

---

# 76. Rate Limiting

Anti-entropy endpoints can be expensive.

Apply:

```text
per-device
per-tenant
global
```

limits.

---

# 77. Admission Control

Large verification should run only if:

```text
server load permits
```

Otherwise:

```text
RetryLater
```

with scheduling hint.

---

# 78. Client Scheduling

Anti-entropy is normally background priority.

Do not compete with urgent user mutation sync.

Priority:

```text
user operation sync
journal catch-up
conflict resolution
anti-entropy
bulk verification
```

---

# 79. Battery-Aware Verification

Mobile clients can defer full verification until:

```text
charging
Wi-Fi
foreground idle
```

when platform integration permits.

---

# 80. Low-Storage Device

Anti-entropy must avoid requiring a full second database copy just to compare state.

Merkle/partition hashes are ideal.

---

# 81. Verification After Bootstrap

After installing a snapshot:

```text
verify snapshot manifest hashes
```

This provides immediate baseline confidence.

---

# 82. Verification After Migration

Client DB engine migration:

```text
old store
→ new store
```

should run canonical digest comparison before cutover.

---

# 83. Server Migration Verification

When migrating authoritative DB:

```text
source canonical root
target canonical root
```

should match at declared boundary before promotion.

---

# 84. PITR Verification

After restore:

```text
new authority epoch
```

run integrity verification before serving clients where feasible.

---

# 85. Adapter Compliance

Add anti-entropy requirements to adapter certification.

Local adapter must support:

```text
canonical entity enumeration
canonical snapshot read
repair transaction
```

Server adapter must support:

```text
consistent canonical state read at declared boundary
```

or equivalent snapshot mechanism.

---

# 86. Optional Adapter Capability

Some lightweight adapters may not support efficient anti-entropy.

Capability:

```text
IntegritySupport::Full
IntegritySupport::SnapshotOnly
IntegritySupport::None
```

Tier A enterprise adapters should support at least robust snapshot verification.

---

# 87. Generic Adapter Digest

Generic record adapters can compute canonical digest automatically.

Domain-operation projects may register:

```text
IntegrityProjector
```

for each aggregate/entity.

---

# 88. IntegrityProjector Trait

Conceptually:

```rust
pub trait IntegrityProjector {
    async fn canonical_bytes(
        &self,
        entity: EntityRef,
        view: &dyn AuthoritativeReadView,
    ) -> Result<Bytes, IntegrityError>;
}
```

But avoid allocating huge buffers where streaming hash is possible.

---

# 89. Streaming Digest Writer

Better interface:

```rust
pub trait CanonicalHash {
    fn write_canonical(
        &self,
        writer: &mut dyn DigestWriter,
    ) -> Result<(), IntegrityError>;
}
```

This avoids unnecessary serialized intermediate allocation.

---

# 90. Stable Domain Hashing

Application-specific canonical hash implementation must be versioned and tested.

If it changes:

```text
IntegrityGeneration changes
```

---

# 91. Integrity Test Fixtures

Maintain known canonical hash fixtures.

Example:

```text
Student v3 fixture
expected BLAKE3 digest
```

Prevents accidental hash changes.

---

# 92. Property Tests

Properties:

```text
same canonical state -> same digest
DB-specific storage layout changes -> same digest
field ordering changes -> same digest
different authoritative state -> overwhelmingly different digest
```

---

# 93. Differential Hash Testing

For one canonical entity:

```text
Stoolap adapter hash
SQLite adapter hash
PostgreSQL adapter hash
```

must match.

This is essential for universal DB interoperability.

---

# 94. Merkle Tree Determinism Test

Same entity set inserted in different order must produce same root.

---

# 95. Crash During Repair

If client crashes:

```text
before repair commit
```

old state remains.

If:

```text
after repair commit
```

repaired state survives.

Repair transaction must be ACID.

---

# 96. Response Loss During Repair

Repair data can be resent safely.

Repair application should be idempotent by:

```text
entity version
repair ID
boundary
```

---

# 97. Repair ID

Define:

```rust
pub struct RepairId(Uuid);
```

Useful for:

```text
diagnostics
idempotent repair application
support
```

---

# 98. Repair Generation

If same repair is retried:

```text
RepairId unchanged
```

Do not create repeated local audit entries.

---

# 99. Conflict With Pending Operation

If repair updates an entity with pending local mutations:

```text
rebase
```

or:

```text
mark pending operation conflict
```

according to domain policy.

---

# 100. Repair Must Not Auto-Resolve Business Conflict

Anti-entropy fixes replica inconsistency.

It should not choose:

```text
client business edit vs server business edit
```

That remains conflict engine responsibility.

---

# 101. Divergence Due to Pending Optimistic State

Do not classify expected optimistic differences as corruption.

Digest comparison should use:

```text
authoritative base state
```

not effective UI state including pending operations.

---

# 102. If Base State Is Not Separately Stored

The adapter/reconciler needs a way to reconstruct:

```text
authoritative representation
```

for integrity checks.

This architectural requirement should influence local persistence design.

---

# 103. Recommended Local State Model

For robust long-term sync:

```text
authoritative replica state
+
outbox operations
+
derived optimistic view
```

is cleaner than overwriting authoritative fields irreversibly.

---

# 104. Incremental Optimistic Overlay

Application may materialize optimistic state for performance.

But it should retain enough metadata to reconstruct/rebase.

---

# 105. Integrity Epoch After Schema Change

When synchronized domain schema changes:

```text
new canonical hash generation
```

Avoid comparing old/new roots.

---

# 106. Rolling Upgrade

Mixed client versions may use different integrity generations.

Server can support:

```text
generation G
generation G+1
```

for a compatibility window.

---

# 107. Old Client

If server no longer supports client's integrity generation:

```text
anti-entropy unavailable
```

but normal sync may continue if protocol compatible.

Or:

```text
UpgradeRequired
```

depending on policy.

---

# 108. Repair Authorization

Repair endpoint should only return data already authorized by client's sync scope.

No extra privilege.

---

# 109. Repair Payload

Repair should reuse canonical snapshot/entity payloads.

Do not invent a second data serialization system.

---

# 110. Repair Compression

Large partition repair may use:

```text
Postcard
+
zstd
```

with existing resource bounds.

---

# 111. Root Verification API

Potential endpoint:

```text
POST /sync/v1/integrity/root
```

or fold into sync protocol messages.

Long-term recommendation:

```text
protocol message type
```

rather than many REST-specific semantics.

---

# 112. Partition Verification API

Potential messages:

```text
IntegrityRootRequest
IntegrityRootResponse
IntegrityPartitionRequest
IntegrityPartitionResponse
RepairRequest
RepairResponse
```

---

# 113. Avoid Chatty Protocol

Merkle traversal can become many round trips.

Allow batching:

```text
request hashes for multiple child nodes
```

per exchange.

---

# 114. Server-Directed Walk

Server can return:

```text
next node IDs to compare
```

to simplify client logic.

But preserve deterministic protocol.

---

# 115. Anti-Entropy Session

Define:

```rust
pub struct IntegritySessionId(Uuid);
```

Optional for multi-step verification.

Session state should not be correctness-critical in memory.

---

# 116. Stateless Session Tokens

If multi-step state is needed, prefer:

```text
signed/encoded continuation token
```

or durable server state.

Do not require sticky server nodes.

---

# 117. Verification Persistence

Client persists progress for long verification jobs.

Example:

```text
current partition
session boundary
integrity generation
```

so restart can resume.

---

# 118. Resume Safety

If authoritative integrity boundary is still retained:

```text
resume
```

otherwise:

```text
restart verification
```

---

# 119. Verification Window Expiry

Server may expire old verification boundaries.

Client handles:

```text
IntegrityBoundaryExpired
```

by restarting later.

---

# 120. Operator-Triggered Verification

Admin:

```text
verify tenant T
verify device D
verify scope S
```

This should enqueue background work, not block admin HTTP request for large datasets.

---

# 121. Fleet Verification

For enterprise fleets, stagger anti-entropy.

Do not schedule all devices at midnight.

Use:

```text
stable jitter based on DeviceId
```

---

# 122. Stable Jitter

Example:

```text
verification slot = hash(DeviceId) mod period
```

This distributes load consistently.

---

# 123. Verification Priority Classes

```text
Critical
RepairFollowup
PostMigration
Routine
LowPriority
```

Admission controller can prioritize appropriately.

---

# 124. Metrics

Client:

```text
integrity_last_verified_age
integrity_mismatch_total
integrity_repair_total
integrity_verify_duration
integrity_bytes
```

Server:

```text
integrity_sessions_total
integrity_root_matches_total
integrity_partition_mismatches_total
integrity_repair_bytes
integrity_generation
```

---

# 125. Logs

Structured events:

```text
integrity_match
integrity_mismatch
repair_started
repair_completed
repair_quarantined
```

Include:

```text
tenant
device
scope
boundary
repair_id
```

not sensitive payload.

---

# 126. Alerting

Alert on:

```text
unexpected mismatch rate
same-version payload mismatch
repeated repair of same partition
corruption across many devices
integrity worker lag
```

---

# 127. Fleet-Wide Mismatch

If many clients show same mismatch:

```text
likely server projection/hash bug
```

Do not automatically repair thousands of clients until cause is understood.

Circuit-breaker:

```text
suspend auto-repair if mismatch rate exceeds threshold
```

---

# 128. Repair Circuit Breaker

Configuration:

```text
max automatic repairs per tenant/hour
max affected entity percentage
```

Above threshold:

```text
require admin review or full controlled bootstrap
```

---

# 129. Same-Version Mismatch Escalation

This indicates strong anomaly.

Default:

```text
record critical diagnostic
repair authoritative local replica
trigger verification of adjacent partition
```

Potentially:

```text
quarantine adapter/device
```

if repeated.

---

# 130. Integrity Incident Correlation

Use Part 02 provenance concepts.

Repair diagnostic can reference:

```text
last known source event
operation ID
correlation ID
```

where available.

This helps find which historical mutation introduced divergence.

---

# 131. Forensic Bundle

Mismatch bundle may include:

```text
EntityRef
local authoritative version
server authoritative version
local digest
server digest
last source EventId
cursor
adapter version
schema version
```

No full payload unless privileged diagnostic mode.

---

# 132. Repair Verification

After repair:

```text
recompute affected digest
```

Do not assume repair succeeded merely because write transaction committed.

---

# 133. Escalation Path

```text
entity repair fails
↓
partition repair
↓
scope bootstrap
↓
quarantine
```

This creates a deterministic recovery ladder.

---

# 134. Automatic Full Bootstrap Threshold

If:

```text
mismatch > X% of scope
```

full bootstrap may be cheaper and safer.

Threshold should be configurable and based on measured cost.

---

# 135. Bootstrap Reuse

Anti-entropy repair should reuse existing:

```text
snapshot transport
chunking
hashing
staging install
```

rather than duplicate implementation.

---

# 136. Integration With Universal DB Adapters

Each adapter converts physical state to the same:

```text
canonical integrity representation
```

Therefore anti-entropy works across:

```text
Stoolap ↔ PostgreSQL
SQLite ↔ MySQL
Redb ↔ PostgreSQL
same DB ↔ same DB
```

without pair-specific integrity code.

---

# 137. Adapter-Specific Fast Paths

Adapters may optimize enumeration/hash computation using:

```text
native checksum
indexes
materialized digest table
```

only if result exactly matches canonical digest semantics.

---

# 138. Never Trust Native DB Checksum as Canonical Digest

Database checksums usually cover:

```text
storage pages
physical encoding
```

not canonical sync semantics.

Use only as local corruption hint, not cross-database comparison.

---

# 139. Full Store Verification

Admin/testing mode can compute:

```text
canonical root of entire sync scope
```

for both source and destination DB.

Useful for migrations and certification.

---

# 140. Compliance Invariants

Add:

## AEQ-INV-AE001

```text
Matching canonical root at same generation/boundary implies no detected synchronized-state divergence within covered scope.
```

## AEQ-INV-AE002

```text
Repair never mutates authoritative server state from client replica data.
```

## AEQ-INV-AE003

```text
Repair preserves pending operation intent.
```

## AEQ-INV-AE004

```text
Same entity version with different canonical digest is treated as integrity anomaly.
```

## AEQ-INV-AE005

```text
Integrity verification never advances normal sync cursor.
```

---

# 141. Model Checking

Extend Part 01 model with:

```text
inject local corruption
run anti-entropy
repair
reapply pending operation
```

Verify convergence without intent loss.

---

# 142. Fault Simulation

Scenarios:

```text
delete one local entity silently
modify payload without version
corrupt tombstone
corrupt cursor-adjacent entity
interrupt repair
interrupt bootstrap
```

---

# 143. Differential DB Tests

For identical canonical state loaded into two DB engines:

```text
root hashes must match
```

This becomes a major adapter certification test.

---

# 144. Migration Certification

Before promoting new adapter/database:

```text
export source canonical root
import target
compute target canonical root
compare
```

---

# 145. CLI

Suggested:

```text
aequora integrity status
aequora integrity verify
aequora integrity verify --scope ...
aequora integrity repair --plan ...
aequora integrity explain <repair-id>
```

Production mutation commands should require strong admin authorization.

---

# 146. Developer API

Client:

```rust
aequora.verify_integrity().await?;
```

Normally the scheduler handles this automatically.

Server/admin:

```rust
integrity_service.verify_scope(scope).await?;
```

---

# 147. Configuration

Example:

```ron
integrity: (
    enabled: true,
    routine_interval_hours: 168,
    partition_count: 1024,
    automatic_repair: true,
    max_auto_repair_entities: 100,
    full_bootstrap_threshold_percent: 10,
)
```

---

# 148. Safe Default

Routine anti-entropy should be enabled for production-capable adapters, but conservative.

Automatic repair:

```text
small, clearly authoritative mismatches
```

may be enabled.

Large or suspicious mismatches should escalate.

---

# 149. Completion Criteria

Part 03 is complete when:

```text
[ ] canonical entity digest defined
[ ] integrity generation defined
[ ] deterministic partitioning defined
[ ] root/partition verification protocol defined
[ ] Merkle path designed
[ ] repair state machine defined
[ ] pending operation preservation defined
[ ] same-version mismatch escalation defined
[ ] full bootstrap fallback defined
[ ] adapter compliance extended
[ ] DB differential digest tests defined
[ ] model/fault tests specified
[ ] metrics/alerts defined
[ ] operator CLI/API specified
```

---

# 150. Final Architecture

```text
                 NORMAL SYNC

Client Cursor N
      │
      ▼
Journal > N
      │
      ▼
Client catches up

                 ANTI-ENTROPY

Client Canonical Root
      │
      ▼
Server Canonical Root
      │
      ├── equal ─────────────► Verified
      │
      └── mismatch
             │
             ▼
       Compare Partitions
             │
             ▼
       Localize Entities
             │
             ▼
         Repair Plan
             │
       ┌─────┼─────────┐
       ▼     ▼         ▼
    Entity Partition Bootstrap
    Repair   Repair    Scope
       │
       ▼
 Preserve/Rebase Pending Intent
       │
       ▼
      Rehash
       │
       ▼
     Verified
```

The architectural principle is:

> **Journal synchronization tells Aequora what changes should have arrived. Anti-entropy proves whether the resulting replica state actually matches the authority.**

A mature synchronization platform needs both.
