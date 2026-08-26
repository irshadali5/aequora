# Aequora Sync — Part 10

# Large Snapshot, Streaming Bootstrap, and Resumable Transfer Architecture

## 1. Purpose

Aequora already supports bootstrap snapshots for bringing a new or reset client to a known authoritative state.

That is sufficient for small datasets, but real deployments may need to bootstrap:

```text
hundreds of thousands of entities
millions of entities
gigabytes of structured data
large offline school/branch datasets
document metadata plus blob references
clients reconnecting after long inactivity
clients rebuilding after corruption
clients moving to a new local database engine
```

A simple design such as:

```text
GET one giant snapshot
↓
deserialize entire payload
↓
write everything
```

does not scale.

It causes:

```text
large memory spikes
long transactions
poor resumability
restarts from zero after failure
mobile background execution problems
expensive retries
slow perceived startup
```

Aequora therefore needs a dedicated architecture for:

```text
streaming snapshot production
chunk manifests
resumable transfer
integrity verification
staging installation
atomic activation
delta catch-up
resource throttling
scope-aware partial bootstrap
```

The central rule is:

> **A bootstrap must be resumable, bounded, verifiable, and atomic at the activation boundary even when the physical transfer spans thousands of chunks and many transactions.**

---

# 2. Goals

The large-bootstrap subsystem should provide:

```text
multi-GB dataset support
bounded RAM
bounded local transaction size
resumable download
resumable install
chunk verification
parallel transfer where useful
network adaptation
mobile-friendly execution
scope independence
safe failure recovery
atomic logical cutover
```

---

# 3. Non-Goals

Bootstrap should not:

```text
hold one DB transaction for hours
require all data in memory
advance normal cursor before install is safe
mix old and new generations invisibly
depend on one uninterrupted HTTP connection
```

---

# 4. Bootstrap Model

Bootstrap should be modeled as a multi-phase durable workflow:

```text
Plan
↓
Acquire Manifest
↓
Download Chunks
↓
Verify Chunks
↓
Install Into Staging
↓
Verify Staging
↓
Activate
↓
Catch Up Delta
↓
Complete
```

---

# 5. BootstrapJob

Define:

```rust
pub struct BootstrapJobId(Uuid);
```

Every large bootstrap is represented by durable local state.

---

# 6. Bootstrap State

```rust
pub enum BootstrapState {
    Requested,
    Planning,
    Downloading,
    Installing,
    Verifying,
    ReadyToActivate,
    Activating,
    CatchingUp,
    Complete,
    Failed,
    Quarantined,
}
```

---

# 7. Scope Awareness

Bootstrap must be scoped.

Each job is bound to:

```text
ScopeId
ScopeVersion
ScopeGeneration
ProjectionSchemaVersion
```

This allows one scope to rebuild without resetting unrelated scopes.

---

# 8. Snapshot Boundary

Every manifest identifies a consistent authoritative boundary:

```rust
pub struct SnapshotBoundary {
    pub scope_id: ScopeId,
    pub scope_generation: ScopeGeneration,
    pub sequence: Sequence,
}
```

The snapshot represents:

```text
all authoritative state relevant to that scope through sequence N
```

---

# 9. Delta-After-Snapshot Rule

After snapshot at sequence N:

```text
client installs snapshot
↓
normal journal pull starts at N
↓
apply N+1 ...
```

This is the bridge between bulk state transfer and ordinary synchronization.

---

# 10. Snapshot Manifest

Define:

```rust
pub struct SnapshotManifest {
    pub snapshot_id: SnapshotId,
    pub boundary: SnapshotBoundary,
    pub schema_version: SnapshotSchemaVersion,
    pub chunk_count: u32,
    pub total_uncompressed_bytes: u64,
    pub total_compressed_bytes: Option<u64>,
    pub root_hash: Digest,
    pub chunks: Vec<ChunkDescriptor>,
}
```

For very large manifests, chunk descriptors themselves may be paged.

---

# 11. ChunkDescriptor

```rust
pub struct ChunkDescriptor {
    pub chunk_id: ChunkId,
    pub ordinal: u32,
    pub entity_range: EntityRange,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub hash: Digest,
    pub compression: CompressionKind,
}
```

---

# 12. Chunk Identity

Chunk ID must be stable for the lifetime of one snapshot.

Do not derive solely from temporary object-storage URL.

---

# 13. Deterministic Chunk Ordering

Chunks should have deterministic ordinal/order.

The client must not depend on transfer completion order.

---

# 14. Chunking Strategy

Possible strategies:

```text
fixed entity count
fixed uncompressed byte target
entity-range partitions
scope partition
aggregate-grouped chunks
```

---

# 15. Recommended Default

Chunk by:

```text
scope partition
+
target uncompressed size
```

while never splitting one atomic aggregate representation across incompatible boundaries.

---

# 16. Target Chunk Size

Start with configurable target, for example:

```text
4–32 MiB uncompressed
```

depending on platform and dataset.

Do not hardcode one universal value.

---

# 17. Why Not Tiny Chunks

Too-small chunks create:

```text
manifest explosion
request overhead
too many transactions
too many hashes
```

---

# 18. Why Not Huge Chunks

Too-large chunks create:

```text
memory pressure
slow retry
long install transaction
bad mobile behavior
```

---

# 19. Aggregate Boundary

If an aggregate must be installed atomically:

```text
Invoice + lines
```

do not split it across chunks unless install layer can preserve aggregate atomicity.

---

# 20. Snapshot Generation

Server creates snapshot from a consistent read view.

Possible implementations:

```text
MVCC transaction
exported DB snapshot
materialized snapshot table
background snapshot builder consuming journal
```

---

# 21. Snapshot Consistency

All chunks in one snapshot must represent the same boundary.

Never build:

```text
chunk 1 at seq 100
chunk 2 at seq 120
```

under one manifest pretending to be one snapshot.

---

# 22. Snapshot Builder

Server component:

```text
SnapshotCoordinator
↓
SnapshotReader
↓
ChunkEncoder
↓
ChunkStore
↓
ManifestPublisher
```

---

# 23. Snapshot Storage

For small snapshots:

```text
server local disk / DB blob
```

For large production snapshots:

```text
object storage
```

is often preferable.

---

# 24. Object Storage Role

Object storage stores:

```text
immutable chunk objects
manifest objects
```

Aequora server still controls:

```text
authorization
snapshot issuance
scope validation
signed/temporary access
```

---

# 25. Database Is Still Authority

Object storage is a delivery mechanism.

It is not the authoritative business database.

---

# 26. Signed Download URLs

Server may return temporary signed URLs for chunks.

Benefits:

```text
offload bandwidth
CDN support
large-file efficiency
```

But authorization must occur before URL issuance.

---

# 27. URL Lifetime

Chunk URLs should have limited lifetime.

If expired:

```text
client asks server for refreshed access
```

without restarting bootstrap.

---

# 28. Snapshot Immutability

Once manifest published:

```text
chunk content must never change
```

If snapshot must be rebuilt:

```text
new SnapshotId
```

---

# 29. Content Addressing

Chunk storage can use:

```text
BLAKE3 digest
```

as object key or part of key.

This helps deduplication and integrity.

---

# 30. Root Hash

Manifest root hash should cover:

```text
ordered chunk descriptors
snapshot metadata
boundary
schema generation
```

not just concatenate chunk hashes ambiguously.

---

# 31. Merkle Manifest

For very large snapshot sets, manifest can itself be Merkle-structured.

This aligns with Part 03 integrity architecture.

---

# 32. Client Download State

Persist per chunk:

```text
NotStarted
Downloading
Downloaded
Verified
Installed
```

---

# 33. Durable Chunk Progress

Logical table:

```text
aequora_bootstrap_chunk
```

Fields:

```text
bootstrap_job_id
chunk_id
ordinal
state
downloaded_bytes
hash
local_path_or_storage_ref
attempts
```

---

# 34. Resume After Restart

Client restarts:

```text
load BootstrapJob
↓
verify manifest still valid
↓
continue from incomplete chunks
```

No restart from zero.

---

# 35. Partial Download Resume

If transport/object storage supports range requests:

```text
resume chunk from byte offset
```

Otherwise restart only that chunk.

---

# 36. Range Request Safety

Resume only when remote object identity is stable.

Use:

```text
ETag/content hash/snapshot ID
```

to ensure partial file belongs to same object.

---

# 37. Temporary File Strategy

Download into:

```text
chunk.tmp
```

After full verification:

```text
atomic rename → chunk.ready
```

Do not treat partially downloaded bytes as verified.

---

# 38. Chunk Verification

For each chunk:

```text
size check
decompression bound check
BLAKE3 hash check
format validation
schema validation
```

---

# 39. Compression Bomb Protection

Manifest provides expected:

```text
compressed size
uncompressed size
```

Client enforces configured maximum expansion.

Do not trust compressed stream headers alone.

---

# 40. Decode Streaming

Prefer:

```text
stream decompress
↓
stream Postcard records
↓
bounded install buffer
```

rather than:

```text
decompress whole chunk into RAM
```

---

# 41. Postcard Framing

A chunk containing many records needs explicit record framing.

Example:

```text
chunk header
record_count
[length][record]
[length][record]
...
chunk checksum
```

Do not rely on ambiguous concatenated Postcard objects.

---

# 42. Chunk Header

Potential:

```rust
pub struct SnapshotChunkHeader {
    pub magic: [u8; 8],
    pub format_version: u16,
    pub snapshot_id: SnapshotId,
    pub chunk_id: ChunkId,
    pub ordinal: u32,
    pub record_count: u32,
}
```

---

# 43. Record Envelope

```rust
pub struct SnapshotRecordEnvelope {
    pub entity: EntityRef,
    pub version: EntityVersion,
    pub projection_schema: ProjectionSchemaVersion,
    pub tombstone: bool,
    pub payload: Bytes,
}
```

---

# 44. Streaming Install

Client should install records incrementally into staging.

Pipeline:

```text
verified chunk
↓
stream decode
↓
bounded batch
↓
staging transaction
↓
checkpoint installed records/chunk
```

---

# 45. No One-Giant Install Transaction

Use bounded DB transactions.

Logical atomicity comes from:

```text
staging generation
+
final activation switch
```

not from a single hour-long transaction.

---

# 46. Staging Generation

Define:

```rust
pub struct ReplicaGeneration(u64);
```

Client currently serves:

```text
generation A
```

Bootstrap writes:

```text
generation B
```

in parallel.

---

# 47. Staging Isolation

Application reads should continue from active generation A until B is complete.

---

# 48. Activation

After all chunks installed and verified:

```text
BEGIN

verify job state
verify scope/manifest
switch active generation A → B
set scope cursor = snapshot boundary
mark bootstrap activation

COMMIT
```

This is the key logical atomic point.

---

# 49. Old Generation Cleanup

Do not immediately delete generation A before B proves usable.

Possible policy:

```text
retain A briefly
```

for rollback/debug.

Then garbage collect.

---

# 50. Activation Crash

If crash before activation commit:

```text
A remains active
B remains staging
```

If after commit:

```text
B is active
```

No half-activated state.

---

# 51. Catch-Up Phase

Immediately after activation:

```text
cursor = snapshot boundary N
↓
pull journal N+1...
```

Until current.

---

# 52. Snapshot Staleness

Snapshot may have been generated hours earlier.

This is fine if:

```text
journal history after N still retained
```

---

# 53. Retention Contract

Server must not issue a snapshot whose boundary will become unusable before reasonable client completion.

Possible strategy:

```text
pin journal retention for active snapshot
```

---

# 54. Snapshot Lease

Define optional:

```rust
pub struct SnapshotLease {
    pub snapshot_id: SnapshotId,
    pub expires_at: Timestamp,
}
```

While valid, server guarantees:

```text
required post-snapshot journal remains available
```

---

# 55. Long Bootstrap

If job takes longer than lease:

```text
client renews snapshot lease
```

where policy permits.

---

# 56. Snapshot Expiry

If snapshot expires and delta history is gone:

```text
BootstrapRestartRequired
```

Client can reuse already-downloaded chunks only if new snapshot declares identical content chunks and semantics.

Initial simpler behavior:

```text
restart with new manifest
```

---

# 57. Content Dedup Reuse

Future optimization:

If new snapshot manifest references same content-addressed chunks:

```text
reuse verified local chunks
```

This can save bandwidth.

---

# 58. Parallel Download

Client may download multiple chunks concurrently.

Concurrency bounded by:

```text
device profile
network
battery
memory
server hints
```

Part 06 scheduler controls this.

---

# 59. Parallel Install

Installing chunks concurrently is more complex.

Initial recommendation:

```text
download parallel
install serial or low bounded concurrency
```

depending on local DB capabilities.

---

# 60. Download vs Install Pipeline

Efficient model:

```text
download workers
↓ bounded ready queue
install worker
```

This overlaps network and disk without unbounded buffering.

---

# 61. Backpressure

If installer is slower than downloader:

```text
ready queue fills
↓
download workers pause
```

---

# 62. Disk Budget

Before bootstrap starts, estimate required temporary storage:

```text
compressed chunks
+
staging DB growth
+
safety margin
```

---

# 63. Disk Preflight

If insufficient storage:

```text
do not start large bootstrap
```

Return:

```text
InsufficientLocalStorage
```

---

# 64. Low-Storage Streaming Mode

If adapter supports streaming direct install without keeping all chunk files:

```text
download one/few chunks
verify
install
delete chunk
continue
```

This minimizes temporary storage.

---

# 65. Resume Tradeoff

Deleting installed chunk means re-download may be needed if staging later becomes invalid.

Policy can choose:

```text
LowStorage
Balanced
FastRecovery
```

---

# 66. Bootstrap Storage Profiles

```rust
pub enum BootstrapStorageProfile {
    LowStorage,
    Balanced,
    CacheChunks,
}
```

---

# 67. Mobile Background Constraints

On mobile:

```text
download/install in chunks
persist after every chunk
stop safely when execution budget ends
resume later
```

---

# 68. Foreground Partial Usability

Some applications may want:

```text
core data usable before optional modules finish
```

Achieve through Part 07 independent scopes.

Do not partially activate one internally inconsistent scope unless projection semantics explicitly support it.

---

# 69. Scope-Level Parallel Bootstrap

Example:

```text
Core scope
Attendance scope
Finance scope
```

Scheduler may bootstrap:

```text
Core first
Attendance second
Finance later
```

---

# 70. Scope Dependency

Respect Part 07 scope dependency graph.

Do not activate dependent scope before required prerequisite scope.

---

# 71. Priority

Bootstrap work classes:

```text
RequiredInteractiveBootstrap
NormalBootstrap
BackgroundOptionalBootstrap
```

Part 06 schedules accordingly.

---

# 72. Large Bootstrap Fairness

Do not let one huge bootstrap monopolize all network.

Yield to:

```text
interactive outgoing operations
small journal pulls
critical repairs
```

---

# 73. Live Hint During Bootstrap

Part 08 hints may arrive while bootstrap is running.

Do not process them as direct state changes.

Record:

```text
sync needed after boundary
```

Catch up after activation.

---

# 74. Pending Local Operations During Bootstrap

Critical case.

Client may have pending unsynced operations when rebootstrap begins.

Never discard them.

---

# 75. Pending Preservation Flow

```text
freeze/export pending intent metadata
↓
bootstrap staging authoritative base
↓
activate new generation
↓
Part 04 rebase unsent operations
↓
retry sent immutable operations
```

---

# 76. Sent Pending Operations

If an operation may already have reached server:

```text
payload immutable
```

After bootstrap, retry same OperationId or inspect ledger result.

---

# 77. Optimistic Local View During Bootstrap

Application may continue showing old generation + pending overlays until cutover.

Avoid writing new authoritative incoming state into old generation once bootstrap replacement is committed to.

---

# 78. New Local Mutations During Bootstrap

Two policies:

```text
AllowAndQueue
TemporarilyReadOnly
```

Recommended local-first default:

```text
AllowAndQueue
```

if domain can safely rebase later.

For high-risk modules:

```text
temporarily read-only
```

may be safer.

---

# 79. Bootstrap Mutation Barrier

Per scope/domain profile can declare:

```rust
pub enum BootstrapMutationPolicy {
    AllowQueue,
    ReadOnly,
    Custom,
}
```

---

# 80. Anti-Entropy Integration

Part 03 may trigger partial/full bootstrap when divergence is widespread.

The same large-bootstrap machinery should be reused.

---

# 81. Repair Bootstrap

A repair bootstrap is not a new business event.

It replaces local replica state.

---

# 82. Import Integration

Part 09 server baseline snapshots can be produced in this chunked format.

This gives newly migrated clients scalable first bootstrap.

---

# 83. Universal DB Adapter Integration

Server snapshot producer converts DB-specific state into canonical snapshot records.

Client adapter installs into its own schema.

Thus:

```text
PostgreSQL server
→ canonical chunks
→ Stoolap/SQLite/Redb client
```

remains database-independent.

---

# 84. Snapshot Encoder Trait

Conceptually:

```rust
pub trait SnapshotSource {
    async fn open_snapshot(
        &self,
        scope: &ResolvedScope,
    ) -> Result<Box<dyn SnapshotReadView>, SnapshotError>;
}
```

---

# 85. SnapshotReadView

Provides:

```text
consistent boundary
stream canonical records
```

No DB-specific types escape.

---

# 86. Snapshot Sink Trait

Client adapter:

```rust
pub trait SnapshotSink {
    async fn begin_staging(
        &self,
        manifest: &SnapshotManifest,
    ) -> Result<StagingHandle, SnapshotError>;

    async fn install_chunk(
        &self,
        staging: &mut StagingHandle,
        chunk: SnapshotRecordStream,
    ) -> Result<(), SnapshotError>;

    async fn activate(
        &self,
        staging: StagingHandle,
        boundary: SnapshotBoundary,
    ) -> Result<(), SnapshotError>;
}
```

---

# 87. Adapter Capability

Declare:

```rust
SnapshotInstallCapability {
    generation_swap,
    resumable_chunk_install,
    consistent_read,
}
```

---

# 88. Tier-A Requirement

A production local adapter should support either:

```text
generation swap
```

or an equivalent crash-safe bootstrap install strategy.

---

# 89. Same-Table Staging

SQL adapters can stage via:

```text
generation column
staging tables
temporary schema
```

depending on implementation.

---

# 90. KV Staging

KV adapter can use:

```text
generation-prefixed keyspace
```

Example:

```text
gen/42/student/...
gen/43/student/...
```

Activation changes one metadata pointer.

---

# 91. Snapshot Schema Version

Define:

```rust
pub struct SnapshotSchemaVersion(u32);
```

Separate from:

```text
protocol version
domain schema
projection schema
```

---

# 92. Compatibility

Client must verify it can decode/install snapshot schema before downloading gigabytes.

---

# 93. Early Compatibility Check

Manifest negotiation returns:

```text
supported codec
snapshot schema
compression
projection schema
```

before transfer.

---

# 94. Capability Negotiation

Client advertises:

```text
max chunk bytes
supported compression
range resume support
parallelism hint
snapshot schema versions
```

Server selects compatible snapshot.

---

# 95. Server Should Not Tailor Snapshot Per Tiny Client Difference Excessively

Too many variants destroy cacheability.

Prefer a small set of snapshot profiles.

---

# 96. Snapshot Profiles

Example:

```text
Standard
LowMemory
LegacyCompatibility
```

---

# 97. Compression

Recommended:

```text
zstd
```

for larger chunks.

But compression level should favor decompression speed for mobile unless bandwidth constraints justify more CPU.

---

# 98. Chunk Encryption

Transport TLS protects in transit.

If chunks stored in third-party object storage and policy requires stronger confidentiality:

```text
encrypt chunk objects
```

with keys controlled by server/application.

Part 15 will formalize cryptographic protection.

---

# 99. Object Storage Naming

Example:

```text
snapshots/<tenant>/<snapshot_id>/<chunk_hash>
```

Avoid embedding sensitive human-readable entity names.

---

# 100. CDN

For geographically distributed large downloads, CDN may accelerate immutable chunks.

Authorization must remain scoped.

---

# 101. Signed URL Leakage

Signed URLs are bearer capabilities during lifetime.

Keep expiration short and avoid logging them.

---

# 102. Server Bandwidth Fallback

If no object storage configured:

```text
Axum streams chunk directly
```

using same logical chunk protocol.

---

# 103. HTTP Range

Direct Axum/object storage adapter can support:

```text
Range
```

for resumable chunk transfer.

---

# 104. Stream Timeouts

Use:

```text
idle timeout
overall reasonable chunk timeout
```

not one tiny fixed request deadline for a huge chunk.

---

# 105. Chunk Retry

Retry one chunk with:

```text
same ChunkId
```

not whole snapshot.

---

# 106. Retry Backoff

Use Part 06 scheduler.

Interactive required bootstrap may retry sooner than optional background scope.

---

# 107. Corrupt Chunk

If hash mismatch:

```text
delete local partial
retry from trusted source
```

Repeated mismatch:

```text
quarantine / alert
```

---

# 108. Root Verification

After all chunks installed:

```text
compute staging canonical root
compare manifest root
```

where practical.

This integrates Part 03.

---

# 109. Per-Chunk vs Full Verification

Minimum:

```text
every chunk hash
```

Preferred:

```text
chunk hashes
+
full staging canonical root
```

for enterprise mode.

---

# 110. Snapshot Record Count

Manifest may include:

```text
record count per chunk
total records
```

Useful for diagnostics, not security alone.

---

# 111. Ordering

Client should not rely on record order for semantics except where snapshot format explicitly defines dependency ordering.

---

# 112. Referential Installation

If local DB enforces foreign keys and chunks arrive out of parent-child order:

Options:

```text
topologically ordered chunks
deferred constraints
staging tables without final constraints
two-phase install
```

---

# 113. Recommended SQL Strategy

Where practical:

```text
generate chunks in dependency-compatible entity groups
```

This simplifies install.

---

# 114. Two-Phase Snapshot Install

For complex schemas:

```text
Phase 1:
    entities

Phase 2:
    relationships/indexes/derived metadata
```

---

# 115. Derived Local Indexes

Do not necessarily include:

```text
search index
UI caches
derived aggregates
```

in snapshot.

Rebuild locally after activation if cheaper.

---

# 116. Rebuild Phase

Optional:

```text
activate authoritative replica
↓
background rebuild local derived indexes
```

Application must tolerate temporarily rebuilding non-authoritative caches.

---

# 117. Snapshot Manifest Pagination

If millions of chunks unlikely but possible, manifest entries may be paged:

```text
manifest root
↓
manifest pages
↓
chunk descriptors
```

Initial implementation can use bounded chunk count avoiding this complexity.

---

# 118. Snapshot Catalog

Server may maintain:

```text
latest snapshot per scope profile
older retained snapshots
```

---

# 119. Snapshot Reuse Across Clients

Many clients in same scope/profile can download same immutable snapshot.

This is important for cost efficiency.

---

# 120. Personalized Scope Problem

If every user has unique scope, snapshot reuse decreases.

Part 07 partitioned scopes can help compose reusable base snapshots plus small deltas.

---

# 121. Base + Delta Snapshot

Future optimization:

```text
shared base snapshot
+
user-specific seed delta
```

Do not build initially unless needed.

---

# 122. Snapshot Creation Frequency

Balance:

```text
snapshot freshness
build cost
journal retention
storage cost
```

---

# 123. Snapshot Scheduler

Server background worker can create snapshots:

```text
periodically
after large import
after schema migration
on demand for large scope
```

---

# 124. Snapshot Build Deduplication

If many clients request same missing snapshot:

```text
one build job
many waiters
```

avoid duplicate expensive builds.

---

# 125. Build Lease

Server can use durable job lease to ensure one snapshot builder per scope/profile.

Part 23 durable-job architecture will formalize this.

---

# 126. Snapshot Failure

Failed snapshot build never becomes visible.

Publish manifest only after:

```text
all chunks written
all hashes verified
root computed
```

---

# 127. Two-Phase Publish

```text
build unpublished
↓
verify
↓
atomically mark published
```

Clients only receive published snapshots.

---

# 128. Garbage Collection

Snapshot GC considers:

```text
retention period
active SnapshotLease
client download references
journal retention
```

---

# 129. Do Not Delete Active Snapshot

If clients hold valid lease:

```text
retain chunks
```

until lease expires.

---

# 130. Journal Pinning Cost

Long snapshot leases delay journal GC.

Limit:

```text
max lease duration
renewal count
```

---

# 131. Very Slow Client

If client cannot finish before retention window despite renewals:

```text
restart with fresher snapshot
```

may be better.

---

# 132. Multi-Tenant Fairness

Snapshot downloads/builds can be expensive.

Apply:

```text
per-tenant concurrency
global build concurrency
download bandwidth budgets
```

Part 18 will formalize server admission.

---

# 133. Thundering Herd

After app reinstall/update:

```text
many clients request bootstrap
```

Mitigations:

```text
snapshot reuse
CDN/object storage
jitter
server admission
cached manifests
```

---

# 134. Client Preflight

Before starting:

```text
validate auth
resolve scope
check compatibility
check disk
check network policy
check battery/background constraints
```

---

# 135. Bootstrap Cancellation

User/app may pause.

Persist state.

Do not discard verified chunks unless storage policy requires it.

---

# 136. Bootstrap Resume

On resume:

```text
revalidate snapshot lease
refresh signed URLs
continue
```

---

# 137. Scope Revocation During Bootstrap

If access is revoked mid-bootstrap:

```text
stop transfer
invalidate job
delete staged unauthorized data according to policy
```

Never activate.

---

# 138. Scope Version Change During Bootstrap

If compatible:

```text
finish snapshot + apply scope delta
```

Potentially.

Initial safer rule:

```text
if scope version/generation changed materially:
    restart/upgrade bootstrap plan
```

---

# 139. Authority Epoch Change During Bootstrap

If server authority epoch changes:

```text
manifest may be invalid
```

Client must revalidate before activation.

Part 16 will formalize authority epochs.

---

# 140. Activation Validation

Before activation, client verifies:

```text
auth still valid
scope still valid
snapshot generation still accepted
authority epoch still compatible
manifest root valid
all required chunks installed
```

---

# 141. Bootstrap Status API

Expose:

```text
state
bytes downloaded
chunks completed
chunks total
records installed
current scope
```

Avoid promising time remaining.

---

# 142. UI

Potential user-facing states:

```text
Preparing offline data
Downloading data
Installing data
Finalizing
Catching up
Ready
```

---

# 143. Metrics — Client

```text
bootstrap_started_total
bootstrap_completed_total
bootstrap_resumed_total
bootstrap_bytes_downloaded
bootstrap_chunk_retry_total
bootstrap_verify_failure_total
bootstrap_activation_duration
```

---

# 144. Metrics — Server

```text
snapshot_build_total
snapshot_build_duration
snapshot_bytes
snapshot_reuse_total
snapshot_manifest_requests
snapshot_chunk_requests
snapshot_lease_active
```

---

# 145. Logs

Structured:

```text
snapshot_build_started
snapshot_published
bootstrap_started
chunk_verified
bootstrap_activated
bootstrap_restarted
```

Do not log signed URLs or sensitive payload.

---

# 146. Alerts

Potential:

```text
snapshot build repeatedly failing
hash mismatches across clients
journal retention pinned too long
bootstrap completion rate drops
object storage unavailable
```

---

# 147. Correctness Invariants

Add:

## AEQ-INV-BS001

```text
A bootstrap cursor is activated only after the corresponding snapshot state is durably installed.
```

## AEQ-INV-BS002

```text
All chunks in one published snapshot represent one consistent authoritative boundary.
```

## AEQ-INV-BS003

```text
Partial staging data is never exposed as an active complete scope.
```

## AEQ-INV-BS004

```text
Chunk retry cannot duplicate authoritative entities after final activation.
```

## AEQ-INV-BS005

```text
Pending local intent is preserved across rebootstrap.
```

## AEQ-INV-BS006

```text
Snapshot activation followed by journal replay from boundary N yields the same authoritative replica state as continuous journal replication from a valid earlier state.
```

---

# 148. Property Tests

Generate:

```text
random chunk order
chunk duplication
download restart
install restart
activation crash
new journal events during bootstrap
```

Assert final convergence.

---

# 149. Model Checking

Part 01 model can abstract:

```text
old generation
staging generation
activation flag
pending outbox
```

Explore crash at each transition.

---

# 150. Fault Injection

Inject:

```text
network failure halfway chunk
hash mismatch
disk full during install
DB crash before activation
crash after activation before status update
journal event after snapshot boundary
```

---

# 151. Differential Test

Compare:

```text
A. normal incremental replay from empty baseline
B. snapshot bootstrap at N + journal replay N+1...
```

Canonical final state must match.

---

# 152. Cross-Database Test

Build snapshot from PostgreSQL.

Install into:

```text
Stoolap
SQLite
Redb
```

Canonical root must match authoritative source.

---

# 153. Large-Scale Soak Test

Simulate:

```text
10M records
thousands of chunks
repeated pause/resume
network throttling
process restarts
```

Measure:

```text
peak memory
disk overhead
throughput
retry amplification
```

---

# 154. Resource Invariants

Add:

```text
max decoded record size
max chunk uncompressed size
max concurrent downloads
max ready queue bytes
max staging batch size
```

---

# 155. Memory Architecture

Use bounded pipeline:

```text
Network Reader
    ↓
Decompressor
    ↓
Record Framer
    ↓
Small Decode Buffer
    ↓
Install Batch
```

No full-snapshot materialization.

---

# 156. Zero-Copy

Use:

```text
Bytes
borrowed slices
streamed decode
```

where practical.

Do not complicate snapshot semantics solely to eliminate every copy.

---

# 157. Recommended Modules

```text
aequora-client/
└── bootstrap/
    ├── mod.rs
    ├── job.rs
    ├── manifest.rs
    ├── downloader.rs
    ├── verifier.rs
    ├── installer.rs
    ├── activation.rs
    ├── resume.rs
    └── metrics.rs

aequora-server/
└── snapshot/
    ├── coordinator.rs
    ├── builder.rs
    ├── reader.rs
    ├── chunker.rs
    ├── publisher.rs
    ├── lease.rs
    └── catalog.rs
```

---

# 158. Configuration

Example:

```ron
bootstrap: (
    target_chunk_uncompressed_bytes: 8388608,
    max_chunk_uncompressed_bytes: 33554432,

    download: (
        max_concurrency: 4,
        resume_ranges: true,
    ),

    install: (
        records_per_transaction: 1000,
        ready_queue_bytes: 33554432,
    ),

    verification: (
        verify_chunk_hashes: true,
        verify_final_root: true,
    ),

    storage_profile: Balanced,
)
```

---

# 159. Plug-and-Play Experience

Most developers should not manually manage chunks.

Client API:

```rust
aequora.bootstrap(scope).await?;
```

or automatic:

```text
ResyncRequired
↓
client runtime schedules bootstrap
↓
status stream reports progress
```

Server adapter handles snapshot production.

---

# 160. Completion Criteria

Part 10 is complete when:

```text
[ ] BootstrapJob state machine defined
[ ] snapshot boundary defined
[ ] chunk manifest defined
[ ] deterministic chunking defined
[ ] resumable transfer defined
[ ] range retry safety defined
[ ] streaming decode/install defined
[ ] staging generation defined
[ ] atomic activation defined
[ ] delta-after-snapshot catch-up defined
[ ] journal retention/snapshot lease defined
[ ] pending-operation preservation defined
[ ] scope/version/revocation handling defined
[ ] object-storage/CDN path defined
[ ] adapter source/sink traits defined
[ ] disk/memory preflight defined
[ ] correctness/property/fault tests defined
```

---

# 161. Final Architecture

```text
                   AUTHORITATIVE DATABASE
                            │
                            ▼
                    Consistent Read View
                            │
                            ▼
                      Snapshot Builder
                            │
                ┌───────────┴───────────┐
                ▼                       ▼
             Manifest              Chunk Objects
                │                       │
                └───────────┬───────────┘
                            ▼
                         CLIENT
                            │
                       BootstrapJob
                            │
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
          Download       Verify        Resume State
              │
              ▼
        Streaming Decode
              │
              ▼
        Staging Generation B
              │
              ▼
         Full Verification
              │
              ▼
       Atomic Activation A → B
              │
              ▼
      Cursor = Snapshot Boundary N
              │
              ▼
        Journal Catch-Up N+1...
              │
              ▼
             Ready
```

The architectural principle is:

> **A large bootstrap is not one giant transfer. It is a durable, resumable state machine that progressively builds a verified staging replica and changes logical reality only at one small atomic activation point.**

That architecture allows Aequora to bootstrap gigabyte-scale offline datasets on desktop and mobile devices without sacrificing crash safety, memory bounds, database independence, or synchronization correctness.
