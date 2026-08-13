# Aequora Sync — Part 09

# Bulk Import, Export, Seed, and Initial Migration Architecture

## 1. Purpose

Aequora will often be introduced into systems that already contain data.

Examples:

```text
existing School ERP database
legacy CSV/Excel exports
old PostgreSQL application
SQLite desktop application
document database
third-party SIS
manual records being digitized
another synchronization engine
```

A production synchronization platform therefore needs a formal architecture for:

```text
bulk import
bulk export
initial seeding
legacy migration
cutover
restartability
validation
quarantine
deterministic identity
journal baselining
snapshot creation
```

The central rule is:

> **Initial migration must create a valid authoritative Aequora state, not imitate millions of historical client sync operations unless history is genuinely required.**

---

# 2. Goals

The migration subsystem should support:

```text
millions of records
restartable jobs
bounded transactions
parallel preparation
deterministic IDs
duplicate detection
schema mapping
validation
quarantine
progress checkpoints
integrity verification
cutover
rollback
auditable provenance
```

---

# 3. Non-Goals

Bulk migration is not:

```text
normal interactive synchronization
a shortcut around domain invariants
raw DB copy without verification
unbounded single transaction
blind replay of unknown legacy history
```

---

# 4. Migration Modes

Aequora should distinguish:

```rust
pub enum MigrationMode {
    SeedAuthority,
    ImportAsOperations,
    ImportSnapshot,
    LegacyBridge,
    StoreToStoreMigration,
}
```

---

# 5. SeedAuthority

Use when introducing Aequora to an existing authoritative dataset.

Example:

```text
existing PostgreSQL ERP
↓
Aequora added
```

The goal is:

```text
treat existing valid records as baseline authority
```

not pretend every old row came from an Aequora operation.

---

# 6. ImportAsOperations

Use when business history must be preserved as domain actions.

Example:

```text
historic payments
historic approvals
immutable accounting events
```

Each imported record may become:

```text
canonical operation/event
```

with deterministic provenance.

This is more expensive but semantically richer.

---

# 7. ImportSnapshot

Use when migrating a complete known-good replica/state into a new Aequora store.

Example:

```text
old local DB engine
→
new local DB engine
```

---

# 8. LegacyBridge

Use when old application continues writing during migration.

Aequora consumes legacy changes through:

```text
CDC
transactional outbox
polling bridge
```

until cutover.

---

# 9. StoreToStoreMigration

Use when moving:

```text
PostgreSQL → PostgreSQL
PostgreSQL → another authoritative adapter
Stoolap → SQLite
```

while preserving Aequora metadata or explicitly creating a new timeline.

---

# 10. Migration Pipeline

Recommended high-level flow:

```text
Discover
↓
Map
↓
Validate
↓
Stage
↓
Transform
↓
Write
↓
Verify
↓
Baseline Journal
↓
Create Snapshot
↓
Cutover
```

---

# 11. ImportJob

Define:

```rust
pub struct ImportJobId(Uuid);
```

Every large import is durable and restartable.

---

# 12. Import Job State

```rust
pub enum ImportJobState {
    Planned,
    Scanning,
    Transforming,
    Importing,
    Verifying,
    ReadyForCutover,
    Completed,
    Failed,
    Quarantined,
}
```

---

# 13. Durable Job Metadata

Logical table:

```text
aequora_import_job
```

Fields:

```text
job_id
mode
source_kind
source_fingerprint
state
started_at
updated_at
checkpoint
policy_version
correlation_id
```

---

# 14. Import Provenance

Use Part 02 lineage.

Import should record:

```text
ImportJobId
CorrelationId
source system
source dataset
source record key
```

Do not pretend imported records originated from a live Aequora device.

---

# 15. Source Identity

Define:

```rust
pub struct SourceSystemId(String);
```

Examples:

```text
legacy-school-erp-v2
csv-2026-08
old-postgres-cluster
```

---

# 16. Source Record Key

Every source record should have stable reference:

```text
table + primary key
document ID
CSV row identity
external system ID
```

Used for:

```text
restartability
deduplication
quarantine
audit
```

---

# 17. Deterministic Distributed IDs

Existing systems may use:

```text
integer primary keys
natural keys
different UUIDs
```

Aequora should generate stable sync IDs deterministically where possible.

---

# 18. ID Mapping Strategies

```rust
pub enum IdMappingStrategy {
    PreserveExistingUuid,
    DeterministicNamespace,
    MappingTable,
    NewUuidWithMap,
}
```

---

# 19. Preserve Existing UUID

If source already has globally stable UUID:

```text
reuse it
```

after validation.

---

# 20. Deterministic Namespace

For stable legacy key:

```text
namespace + source entity type + source primary key
↓
deterministic UUID
```

This makes restart safe.

---

# 21. Mapping Table

For complex migrations, persist:

```text
source key
→
Aequora EntityId
```

Logical:

```text
aequora_import_identity_map
```

---

# 22. Never Regenerate Random IDs on Retry

If import restarts:

```text
same source record
```

must resolve to:

```text
same target EntityId
```

---

# 23. Parent/Child Identity

Import parent IDs before or deterministically derive all IDs first.

Example:

```text
Student
Guardian
Enrollment
Invoice
```

Relationships must use final canonical IDs.

---

# 24. Two-Pass Identity Planning

For complex relational migration:

```text
Pass 1:
    enumerate entities
    assign canonical IDs

Pass 2:
    transform/write relationships
```

This avoids unresolved foreign references.

---

# 25. Mapping Specification

Use explicit RON configuration for legacy sources.

Example:

```ron
EntityImport(
    source: "students",
    target: "Student",
    id: DeterministicNamespace("legacy-student"),
    fields: [
        ("student_name", "name"),
        ("mobile", "phone"),
    ],
)
```

---

# 26. Mapping Is Not Business Validation

Mapping handles:

```text
where data comes from
how it converts
```

Validation handles:

```text
whether resulting domain state is valid
```

Keep separate.

---

# 27. Canonical Transform Layer

Source:

```text
row/document/CSV
```

↓ transform to:

```text
canonical import DTO
```

↓ validate into:

```text
domain type
```

↓ persist.

---

# 28. Import DTO

Do not deserialize legacy records directly into production domain types if source semantics differ.

Use explicit:

```rust
LegacyStudentRecord
```

then convert:

```rust
TryFrom<LegacyStudentRecord> for StudentSeed
```

---

# 29. Validation Layers

```text
structural validation
type conversion
referential validation
domain validation
cross-record invariant validation
```

---

# 30. Structural Validation

Examples:

```text
required field exists
date parses
integer in range
UUID valid
```

---

# 31. Domain Validation

Examples:

```text
admission number format
class assignment valid
invoice totals consistent
```

---

# 32. Cross-Record Validation

Examples:

```text
guardian references existing student
payment references invoice
ledger debits == credits
```

May require later validation pass.

---

# 33. Quarantine

Invalid source rows should not crash entire import unless policy says fail-fast.

Create:

```text
quarantine record
```

with:

```text
source key
reason code
sanitized context
```

---

# 34. Quarantine Table

Logical:

```text
aequora_import_quarantine
```

Fields:

```text
job_id
source_key
entity_kind
error_code
status
resolution
```

---

# 35. No Silent Coercion

Do not silently convert:

```text
invalid date -> current date
missing amount -> 0
bad boolean -> false
```

Use explicit policy or quarantine.

---

# 36. Strict vs Tolerant Policy

```rust
pub enum ImportStrictness {
    Strict,
    Tolerant {
        max_quarantine_percent: u8,
    },
}
```

---

# 37. Fail Threshold

Example:

```text
if > 1% records invalid:
    stop before cutover
```

Actual threshold application-specific.

---

# 38. Dry Run

Always support:

```text
validation-only
```

before writing.

Command:

```text
aequora import plan
aequora import validate
```

---

# 39. Import Manifest

Generate:

```text
source record counts
mapped entity counts
quarantine counts
estimated target bytes
mapping version
```

---

# 40. Restartability

Large import must checkpoint.

Checkpoint examples:

```text
last source key
source page token
file offset
chunk number
```

---

# 41. Checkpoint Semantics

Persist checkpoint only after target batch commits.

Never:

```text
advance checkpoint
then commit target
```

---

# 42. Import Batch

Use bounded transactions.

Example:

```text
500–5,000 records
```

depending on DB and domain invariants.

Measure.

---

# 43. One Giant Transaction Is Wrong

A transaction containing millions of rows risks:

```text
huge locks
WAL growth
memory pressure
long rollback
timeouts
```

---

# 44. Small Transactions With Global Verification

Import in batches, then run:

```text
cross-batch verification
```

before cutover.

---

# 45. Atomicity Boundary

Some related records may require one transaction.

Example:

```text
invoice
+
invoice lines
```

Keep aggregate atomic, while batching multiple aggregates.

---

# 46. Parallel Preparation

Use Rayon for:

```text
CSV parsing preparation
canonical transformation
hashing
validation that needs no DB
```

---

# 47. Database Writes

Use Tokio/DB concurrency with bounded workers.

Do not parallel-write without respecting:

```text
foreign keys
unique constraints
aggregate ordering
```

---

# 48. Import Planner

Planner builds dependency graph between entity classes.

Example:

```text
Tenant
↓
Campus
↓
Student
↓
Enrollment
↓
Invoice
↓
Payment
```

---

# 49. Topological Import Order

Import entity groups according to dependencies.

Cycles require:

```text
deferred constraints
two-phase relationship fill
or
domain-specific handling
```

---

# 50. Legacy Data With Broken References

Policy choices:

```text
quarantine child
create explicit placeholder
repair source first
```

Never invent hidden parent data automatically.

---

# 51. Seed vs Event History

For ordinary master data, seed current state.

For audit-critical immutable business history, import canonical historical events.

Do not force one mode globally.

---

# 52. Baseline Authority

After seeding existing data, Aequora needs a synchronization starting point.

Create:

```text
BaselineSequence N
```

and initial snapshot.

---

# 53. Do Not Emit One Journal Event Per Historical Row by Default

For 50 million legacy records this would create huge synthetic history.

Instead:

```text
seed authoritative state
↓
create baseline snapshot at sequence N
↓
new live changes start at N+1
```

---

# 54. Baseline Journal Record

Optionally record one special administrative event:

```text
AuthoritySeeded
```

with:

```text
ImportJobId
baseline sequence
dataset hash
```

This is not a substitute for per-record history if legally required.

---

# 55. Existing Audit History

If legacy audit history exists:

```text
import into business audit subsystem
```

separately from Aequora sync journal.

Remember:

```text
sync journal != audit log
```

---

# 56. Initial Entity Versions

Need deterministic version policy.

Options:

```text
all seeded entities version = 1
preserve trusted legacy version
derive from historical events
```

---

# 57. Recommended Seed Version

For current-state seeding:

```text
EntityVersion(1)
```

unless preserving a trusted monotonic legacy version is clearly useful.

---

# 58. Never Use Legacy updated_at as Version

Timestamp is not a safe concurrency version.

---

# 59. Seed Operation Ledger

Seeded records generally have no historical `OperationId`.

Do not fabricate operation ledger entries for every row unless imported as operations.

---

# 60. First Live Mutation

After baseline:

```text
entity version 1
↓
first Aequora update
↓
version 2
```

---

# 61. Initial Snapshot

After seeding and verification, build:

```text
scope-aware snapshots
```

at one consistent boundary.

---

# 62. Snapshot Boundary

Example:

```text
baseline sequence = 100
```

Snapshot represents:

```text
authoritative state through 100
```

New live events begin:

```text
101+
```

---

# 63. Cutover Race

If old system is still accepting writes while snapshot is built:

```text
data can change underneath migration
```

Need cutover strategy.

---

# 64. Cutover Strategies

```rust
pub enum CutoverStrategy {
    MaintenanceWindow,
    DualWriteBridge,
    CDCBridge,
    IncrementalCatchup,
}
```

---

# 65. Maintenance Window

Simplest:

```text
stop legacy writes
↓
final import/catch-up
↓
verify
↓
activate Aequora
```

Best when downtime is acceptable.

---

# 66. CDC Bridge

Legacy continues writing.

Flow:

```text
initial bulk seed
↓
CDC captures changes after seed boundary
↓
apply bridge changes
↓
lag reaches zero
↓
brief write freeze
↓
final catch-up
↓
cutover
```

---

# 67. Dual Write

Application writes both old and new systems.

This is risky because dual-write failure creates divergence.

Prefer:

```text
transactional outbox/CDC
```

over naïve two independent writes.

---

# 68. Incremental Catch-Up

If source has reliable monotonic change token:

```text
bulk scan at token T
↓
replay changes after T
```

until cutover.

---

# 69. Migration Watermark

Define:

```rust
pub struct MigrationWatermark(String);
```

Adapter-specific but persisted in import metadata.

---

# 70. Source Snapshot Consistency

For DB sources, prefer:

```text
repeatable-read/exported snapshot
```

so bulk scan corresponds to a consistent source point.

---

# 71. CSV/File Source

Use immutable file hash:

```text
BLAKE3
```

as source fingerprint.

If file changes between retries:

```text
fail or start new job
```

---

# 72. Source Fingerprint

Persist:

```text
file hash
DB source identity
schema version
snapshot token
```

to detect unexpected source changes.

---

# 73. Duplicate Detection

Potential duplicate classes:

```text
same source key repeated
same natural business key
same canonical EntityId
```

Handle explicitly.

---

# 74. Duplicate Policy

```rust
pub enum DuplicatePolicy {
    Reject,
    KeepFirst,
    KeepLast,
    MergeCustom,
}
```

Default:

```text
Reject
```

for high-assurance migration.

---

# 75. Natural-Key Collisions

Example:

```text
two students same admission number
```

Do not silently pick one.

Quarantine or resolve before cutover.

---

# 76. Idempotent Batch Import

Each imported entity should have stable import identity.

Retrying same batch must not duplicate target rows.

---

# 77. Import Ledger

Logical:

```text
aequora_import_record
```

Fields:

```text
job_id
source_key
target_entity_id
status
checksum
```

This supports restart and audit.

---

# 78. Record Checksum

Hash canonical transformed record.

On retry:

```text
same source key + same checksum
→ already imported
```

If checksum changed:

```text
source mutated
```

requires policy.

---

# 79. Staging Schema

For risky migrations, import into:

```text
staging tables/schema
```

first.

Then validate before promoting.

---

# 80. Direct Authoritative Import

For simple trusted migrations, write directly to final tables in bounded transactions.

Still require verification before service activation.

---

# 81. Staging Generation

Alternative:

```text
authoritative generation A active
generation B staged
```

Then atomic logical cutover to B.

Useful for store migration.

---

# 82. Verification Layers

After import:

```text
record counts
constraint checks
domain invariants
canonical digests
Merkle roots
sample queries
cross-source totals
```

---

# 83. Part 03 Integration

Use anti-entropy/canonical digest architecture for migration verification.

Example:

```text
source canonical root
target canonical root
```

must match where schemas represent same semantics.

---

# 84. Domain Totals

For finance:

```text
sum debits
sum credits
invoice totals
payment totals
```

must reconcile.

Do not rely only on row counts.

---

# 85. Referential Integrity Verification

Check:

```text
orphan foreign references
missing parent entities
invalid scope membership
```

---

# 86. Scope Verification

Part 07 scopes should bootstrap correctly from imported data.

Test representative:

```text
teacher scope
finance scope
parent scope
```

before cutover.

---

# 87. Initial Scope Seeds

After authority import:

```text
generate/verify scope snapshots
```

so first clients do not trigger expensive ad hoc full scans.

---

# 88. Import and Projection Schema

If clients use projection schemas, migration should validate:

```text
source domain state
→
scope projections
```

not merely internal server rows.

---

# 89. Quarantine Resolution Workflow

Operator can:

```text
inspect
correct mapping
provide override
re-run affected records
```

---

# 90. Quarantine Must Be Durable

Do not keep invalid records only in logs.

Operators need stable resolution workflow.

---

# 91. Import Error Codes

Examples:

```text
AEQ-IMP-TYPE-001
AEQ-IMP-DUP-001
AEQ-IMP-REF-001
AEQ-IMP-DOMAIN-001
AEQ-IMP-SOURCE-001
```

---

# 92. Import CLI

Suggested:

```text
aequora import plan
aequora import validate
aequora import run
aequora import status
aequora import resume
aequora import quarantine
aequora import verify
aequora import cutover
```

---

# 93. Export Architecture

Aequora also needs canonical export.

Uses:

```text
tenant data portability
backup adjunct
migration
support
offline archive
test fixtures
```

---

# 94. Export Modes

```rust
pub enum ExportMode {
    CanonicalSnapshot,
    DomainExport,
    PendingClientOperations,
    MigrationBundle,
}
```

---

# 95. Canonical Export Bundle

Recommended:

```text
manifest.ron
entities/*.postcard
checksums
optional pending operations
scope metadata
```

---

# 96. Export Manifest

Contains:

```text
format version
Aequora version
schema versions
scope IDs
cursor boundary
entity counts
hashes
source adapter
created_at
```

---

# 97. Export Security

Export may contain sensitive data.

Require:

```text
authorization
encryption where needed
secure filesystem permissions
audit
```

---

# 98. Export Consistency

Use consistent snapshot boundary.

Do not export:

```text
half old / half new
```

across live changing authority without snapshot semantics.

---

# 99. Client Pending Export

For corrupted client recovery, export:

```text
pending outbox
OperationId
dependencies
lineage
payload hashes
```

Part 04 uses this.

---

# 100. Importing Pending Client Operations

When restoring a client store:

```text
preserve original OperationIds
```

Do not regenerate.

---

# 101. Legacy Sync System Migration

If old system already has:

```text
change tokens
client pending queues
replication metadata
```

do not blindly map them to Aequora cursors.

Use:

```text
new baseline authority
new ScopeGeneration
```

unless exact semantics can be proven equivalent.

---

# 102. Safer Legacy Cutover

Recommended:

```text
import authoritative state
create new Aequora baseline
clients bootstrap fresh
```

rather than converting opaque historical replication metadata.

---

# 103. Client Migration

Existing local app users may have unsynced legacy changes.

Need product-specific bridge:

```text
extract legacy pending intent
↓
convert to Aequora operations
↓
preserve user work
```

---

# 104. Shadow Mode

Before cutover, run Aequora in:

```text
shadow validation
```

It reads/imports changes but does not serve authoritative clients yet.

Compare output against legacy behavior.

---

# 105. Comparison Mode

Run same source changes through:

```text
legacy system
Aequora transformation
```

Compare canonical states.

Useful for confidence.

---

# 106. Cutover Gate

Do not cut over until:

```text
source lag = 0
quarantine within accepted threshold
verification passes
backups exist
rollback plan tested
client bootstrap tested
```

---

# 107. Cutover Checklist

```text
freeze legacy writes if required
capture final watermark
apply final delta
verify canonical roots/totals
set new authority epoch/generation if needed
enable Aequora writes
disable legacy writer
monitor
```

---

# 108. Rollback Strategy

Before cutover, define:

```text
how to return to old system
```

If Aequora accepts new writes after cutover, rollback becomes harder.

Potential:

```text
reverse bridge
short rollback window
```

or no rollback after committed point.

---

# 109. Authority Epoch

Part 16 will formalize authority failover.

Migration cutover should record:

```text
new authority epoch
```

when changing authoritative system/timeline.

---

# 110. Split-Brain Prevention

After cutover:

```text
old system must stop accepting authoritative writes
```

unless a controlled bridge exists.

Never allow two independent authorities.

---

# 111. Write Fence

Use operational flag/credential revocation to prevent old writers after cutover.

---

# 112. Import Performance

Optimize:

```text
prepared statements
COPY/bulk insert where adapter supports
bounded concurrency
streaming source reads
```

while preserving semantic checks.

---

# 113. PostgreSQL Fast Path

For trusted staging data:

```text
COPY into staging
↓
validate
↓
transform/insert authoritative tables
```

may be efficient.

Do not put SQL-specific behavior in core migration API.

---

# 114. Memory Bounds

Never load whole migration dataset into RAM.

Use:

```text
streaming readers
bounded channels
chunked transforms
```

---

# 115. Backpressure

Pipeline:

```text
source reader
↓ bounded channel
transform workers
↓ bounded channel
DB writer
```

DB throughput controls upstream pressure.

---

# 116. Rayon

Use Rayon for pure CPU-heavy transformation/hash work.

Do not share DB transaction objects across Rayon workers.

---

# 117. Transaction Writer

Use a small number of Tokio DB writer tasks.

Maintain deterministic dependency ordering.

---

# 118. Parallel Entity Classes

Independent entity groups can import concurrently.

Example:

```text
configuration
catalog
```

But dependent groups wait.

---

# 119. Progress

Expose:

```text
records scanned
records imported
bytes read
quarantined
current phase
estimated remaining records
```

Avoid promising exact time estimates.

---

# 120. Resume

On restart:

```text
load job
validate source fingerprint
resume from committed checkpoint
```

---

# 121. Changed Source on Resume

If source fingerprint differs:

```text
fail
```

unless source type supports a known consistent delta protocol.

Do not resume against mutated CSV accidentally.

---

# 122. Import Cancellation

Operator may cancel.

Behavior:

```text
stop after current safe batch
retain staged/imported state
mark job canceled
```

Cleanup is separate.

---

# 123. Cleanup

If canceled staging import:

```text
drop staging generation
```

after operator confirmation.

If direct authoritative import partially occurred before cutover:

```text
must have explicit rollback/cleanup plan
```

---

# 124. Direct Import Safety

Prefer staging when import cannot be naturally isolated from existing live data.

---

# 125. Seed Authority on Empty Target

Simplest case:

```text
empty target DB
```

Import directly in bounded transactions, then verify before enabling service.

---

# 126. Import Into Existing Tenant

More complex.

Need:

```text
duplicate detection
merge policy
scope implications
version policy
```

Default should be conservative.

---

# 127. Merge Policy

```rust
pub enum ExistingEntityPolicy {
    Reject,
    SkipIfIdentical,
    ReplaceIfUnmodified,
    CustomMerge,
}
```

---

# 128. SkipIfIdentical

Use canonical digest.

If same ID and same canonical state:

```text
safe skip
```

---

# 129. ReplaceIfUnmodified

Requires explicit version/baseline check.

Do not overwrite live edited entity blindly.

---

# 130. Live Import

If importing into active production tenant:

```text
treat import as actual domain operations
```

or use tightly controlled server-side import transaction that emits normal journal events.

Do not bypass journal.

---

# 131. Seed Import vs Live Import

Critical distinction:

```text
before Aequora authority goes live:
    baseline seeding can avoid per-row journal

after service is live:
    imported changes must become journal-visible
```

---

# 132. Live Bulk Operation

For production bulk import:

```text
BulkImport command
```

may internally insert many aggregates and append authoritative events.

Batch carefully.

---

# 133. Journal Fan-Out

Large live import can create huge journal backlog.

Part 06 scheduler and server admission should classify as:

```text
Bulk
```

---

# 134. Client Impact

Server may throttle large import publication.

Clients eventually catch up through normal cursors.

---

# 135. Scope-Aware Import

Journal events from live import must include correct Part 07 routing keys.

---

# 136. Causality

All records imported in one job can share:

```text
CorrelationId
```

while each authoritative operation/event has own ID.

---

# 137. Bulk Correlation Size

Do not attach list of millions of source IDs to one envelope.

Store source mapping separately.

---

# 138. Audit

Record:

```text
who initiated import
source
mapping version
job ID
counts
cutover result
```

---

# 139. Security

Migration tools need high privilege.

Use separate:

```text
migration/admin role
```

not ordinary client auth.

---

# 140. Secrets

Source DB credentials belong in secret manager/env.

Never persist plaintext in job metadata.

---

# 141. PII in Quarantine

Quarantine records can contain sensitive values.

Prefer:

```text
error metadata + source reference
```

and store raw rejected row only if necessary with strict access.

---

# 142. Malware/File Safety

If importing files/documents:

```text
validate file metadata
size limits
content scanning policy
blob subsystem
```

Do not pass arbitrary large files through normal record importer.

---

# 143. Canonical Hash Verification

Part 03 digest system should verify final imported state.

For same-semantics migration:

```text
source root == target root
```

at defined boundary.

---

# 144. Import Invariants

Add to Part 01:

## AEQ-INV-IMP001

```text
Same source record under same import mapping resolves to the same target identity across retries.
```

## AEQ-INV-IMP002

```text
Import checkpoint never advances beyond committed target data.
```

## AEQ-INV-IMP003

```text
Cutover cannot occur while required verification has failed.
```

## AEQ-INV-IMP004

```text
Baseline seeding before live authority does not require synthetic per-row sync history.
```

## AEQ-INV-IMP005

```text
After Aequora becomes live authority, imported authoritative changes are journal-visible.
```

## AEQ-INV-IMP006

```text
Quarantined invalid records are never silently treated as successfully imported.
```

---

# 145. Property Tests

Generate:

```text
duplicate source keys
restart at arbitrary checkpoints
batch failures
mapping errors
```

Verify idempotent final state.

---

# 146. Failpoint Tests

Crash:

```text
before batch commit
after batch commit before checkpoint
after checkpoint write
during verification
during cutover flag change
```

---

# 147. Restart Test

Expected:

```text
already committed rows skipped/idempotently reused
uncommitted rows retried
no duplicates
```

---

# 148. Source Mutation Test

CSV changes after partial import.

Expected:

```text
fingerprint mismatch
job refuses unsafe resume
```

---

# 149. Cutover Failure Test

Failure after write freeze but before final activation.

Runbook must specify:

```text
resume migration
or
restore legacy writes
```

without two authorities.

---

# 150. Migration TestKit

Potential API:

```text
source()
map()
inject_duplicate()
inject_bad_reference()
crash_after_batch()
resume()
verify()
```

---

# 151. Metrics

```text
import_jobs_total
import_records_scanned_total
import_records_committed_total
import_quarantine_total
import_batch_duration
import_verify_failure_total
import_bytes_total
```

---

# 152. Logs

Structured events:

```text
import_started
import_checkpoint
import_record_quarantined
import_verification_passed
import_ready_for_cutover
import_cutover_completed
```

---

# 153. Alerting

For long-running production migration:

```text
job stalled
quarantine threshold exceeded
source lag growing
verification mismatch
cutover aborted
```

---

# 154. Recommended Modules

```text
aequora-migrate/
├── job.rs
├── source.rs
├── mapping.rs
├── identity.rs
├── transform.rs
├── validate.rs
├── quarantine.rs
├── checkpoint.rs
├── verify.rs
├── cutover.rs
└── export.rs
```

Adapter SDK:

```text
aequora-adapter-sdk/
└── migration.rs
```

---

# 155. Source Adapter Trait

Conceptually:

```rust
pub trait ImportSource {
    type Record;

    async fn fingerprint(&self) -> Result<SourceFingerprint, ImportError>;

    async fn stream(
        &self,
        checkpoint: Option<ImportCheckpoint>,
    ) -> Result<ImportStream<Self::Record>, ImportError>;
}
```

---

# 156. Import Transformer

```rust
pub trait ImportTransformer<S, T> {
    fn transform(
        &self,
        source: S,
        ctx: &ImportContext,
    ) -> Result<T, ImportTransformError>;
}
```

---

# 157. Target Writer

A target adapter may expose optimized bulk writer while preserving Aequora migration semantics.

---

# 158. Verification Trait

```rust
pub trait ImportVerifier {
    async fn verify(
        &self,
        job: ImportJobId,
    ) -> Result<VerificationReport, ImportError>;
}
```

---

# 159. Migration Profiles

Provide:

```text
SmallFileImport
LargeDatabaseSeed
LiveLegacyCutover
ClientStoreMigration
```

Profiles choose safe defaults.

---

# 160. School ERP Migration Example

Legacy school database:

```text
schools
students
guardians
classes
attendance
fees
payments
```

Plan:

```text
1. assign canonical tenant/campus IDs
2. assign deterministic student/guardian IDs
3. import school configuration
4. import students/guardians
5. import enrollment/class relationships
6. import attendance history
7. import finance history with strict validation
8. verify totals/references
9. create baseline snapshots
10. cut clients to Aequora
```

Finance history may use event-preserving mode while simple profile data uses seed mode.

---

# 161. Initial Client Deployment

After server cutover, existing users should generally:

```text
authenticate
↓
bootstrap new Aequora local store
```

rather than manually copy old DB files, unless a local migration bridge preserves pending changes.

---

# 162. Local Legacy Pending Changes

If old desktop app has unsynced work:

```text
extract pending intent
↓
convert to Aequora operations
↓
bootstrap current authority
↓
rebase/submit pending operations
```

---

# 163. Avoid Silent Legacy Loss

Do not delete old client DB until:

```text
new bootstrap completed
legacy pending migration verified
```

---

# 164. Archive

After successful migration, retain source/archive according to:

```text
business
legal
security
retention
```

policy.

---

# 165. Completion Criteria

Part 09 is complete when:

```text
[ ] migration modes defined
[ ] ImportJob state machine defined
[ ] deterministic identity mapping defined
[ ] source fingerprinting defined
[ ] mapping/transform/validation separation defined
[ ] quarantine architecture defined
[ ] bounded transaction/checkpoint semantics defined
[ ] dependency-aware import planning defined
[ ] baseline journal strategy defined
[ ] entity version seeding policy defined
[ ] snapshot baseline defined
[ ] live cutover strategies defined
[ ] source CDC/catch-up path defined
[ ] export bundle defined
[ ] canonical verification defined
[ ] rollback/split-brain prevention defined
[ ] CLI/API/testing/invariants defined
```

---

# 166. Final Architecture

```text
                      LEGACY SOURCE
                           │
                           ▼
                     Source Adapter
                           │
                           ▼
                     Fingerprint
                           │
                           ▼
                      Stream Records
                           │
                           ▼
                     ID Mapping
                           │
                           ▼
                  Transform + Validate
                     │           │
                     │           └────► Quarantine
                     ▼
                 Bounded Batches
                     │
                     ▼
                  Target Stage
                     │
                     ▼
                 Verification
            ┌────────┼────────┐
            ▼        ▼        ▼
         Counts   Digests   Domain
                  /Merkle   Invariants
            └────────┼────────┘
                     ▼
               Baseline Sequence
                     │
                     ▼
               Scope Snapshots
                     │
                     ▼
                   CUTOVER
                     │
                     ▼
              Aequora Authority
                     │
                     ▼
             New journal events
```

The architectural principle is:

> **Migration should establish a clean, verified synchronization baseline first; only genuinely meaningful historical operations should be reconstructed as operations.**

This gives Aequora a practical path into existing production systems without forcing a destructive big-bang rewrite or polluting the live journal with synthetic history.
