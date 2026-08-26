# Aequora Sync — Part 14

# Data Governance, Retention, Legal Hold, Erasure, and Lifecycle Architecture

## 1. Purpose

Aequora is designed for long-lived business systems.

Over time, synchronized data accumulates across:

```text
authoritative databases
client replicas
journal history
operation ledgers
audit trails
snapshots
repair artifacts
imports
exports
blob storage
cold archives
```

A mature system must answer:

```text
How long should each class of data exist?
When may it be deleted?
What if an offline client has not seen a tombstone yet?
What if a legal hold blocks deletion?
How should a tenant be offboarded?
How should privacy erasure work without corrupting financial/audit history?
How do we prove cleanup was safe?
```

This requires an explicit lifecycle architecture.

The central rule is:

> **Data deletion is a coordinated distributed lifecycle transition, not a simple SQL DELETE.**

---

# 2. Goals

The governance subsystem should provide:

```text
retention policies
legal hold
safe tombstone garbage collection
privacy erasure
tenant offboarding
scope-aware deletion
snapshot/archive lifecycle
operation-ledger retention
audit retention
blob lifecycle
client cleanup
proof of safe purge
operator tooling
```

---

# 3. Non-Goals

Aequora core should not hardcode:

```text
country-specific legal retention durations
industry-specific compliance durations
tax retention periods
medical record law
```

Those belong to application/tenant policy.

Aequora provides the mechanics.

---

# 4. Data Classes

At minimum distinguish:

```text
AuthoritativeBusinessData
JournalHistory
OperationLedger
AuditTrail
ConflictRecords
Tombstones
Snapshots
ReplayArtifacts
ImportArtifacts
RepairArtifacts
Blobs
ClientReplicaData
DerivedCaches
```

Each has different lifecycle semantics.

---

# 5. RetentionClass

Define:

```rust
pub struct RetentionClassId(u16);
```

Application registry maps domain objects to retention classes.

---

# 6. RetentionPolicy

Conceptually:

```rust
pub struct RetentionPolicy {
    pub class: RetentionClassId,
    pub minimum_retention: DurationPolicy,
    pub maximum_retention: Option<DurationPolicy>,
    pub deletion_mode: DeletionMode,
    pub legal_hold_eligible: bool,
    pub archive_before_delete: bool,
}
```

---

# 7. DeletionMode

```rust
pub enum DeletionMode {
    HardDeleteWhenSafe,
    TombstoneThenGc,
    Pseudonymize,
    CryptographicErase,
    PermanentByPolicy,
}
```

---

# 8. PermanentByPolicy

Use sparingly.

Examples may include legally required immutable evidence.

Even then, policy may permit:

```text
PII minimization
pseudonymization
```

while preserving business evidence.

---

# 9. Lifecycle States

A governed record may move through:

```text
Active
SoftDeleted
Tombstoned
Archived
Held
ErasurePending
Purged
```

Not every data class uses every state.

---

# 10. Tombstone Purpose

A tombstone is a synchronization artifact.

It tells replicas:

```text
this entity existed
it is now deleted
do not resurrect stale state
```

Tombstones must survive long enough for relevant clients.

---

# 11. Safe Tombstone GC

Never delete tombstone solely because:

```text
30 days passed
```

if offline clients may still legitimately reconnect with old state.

Need a safety condition.

---

# 12. Device Watermarks

Track per device/scope:

```text
last acknowledged cursor
last seen at
status
```

This helps determine which clients have crossed a deletion boundary.

---

# 13. Tombstone GC Condition

Conceptually safe when:

```text
all active retained client watermarks > tombstone sequence
OR
those clients have been retired/revoked
AND
snapshot/bootstrap policy guarantees stale clients cannot resume below retained floor
```

---

# 14. Retired Device

A device can be marked:

```text
Retired
Revoked
Expired
```

Such devices no longer block tombstone GC.

If they reconnect:

```text
must rebootstrap
```

---

# 15. Device Retention Horizon

Define policy:

```text
if device inactive > X
    mark requires rebootstrap
```

Then it no longer pins old journal/tombstones indefinitely.

---

# 16. Rebootstrap Floor

Server maintains per scope/generation:

```text
minimum resumable cursor
```

Below that:

```text
ResyncRequired
```

This is a key lifecycle control.

---

# 17. Journal Retention

Journal need not be infinite.

Retention determined by:

```text
active client cursors
snapshot availability
legal/audit needs
replay needs
storage budget
```

---

# 18. Journal GC Rule

Delete journal events only when:

```text
no valid client needs them
AND
snapshot/bootstrap can recover clients below floor
AND
no governance hold requires them
```

---

# 19. Operation Ledger Retention

Operation ledger supports:

```text
idempotency
retry ambiguity
support
replay
```

Do not delete entries too soon.

---

# 20. Idempotency Horizon

Define a minimum ledger retention at least covering:

```text
maximum legitimate retry window
offline duration policy
client resurrection window
```

---

# 21. Old Operation Retry

If ledger entry is gone and an ancient client retries old OperationId:

```text
server must not blindly execute
```

Possible:

```text
OperationTooOld
RebootstrapRequired
UnsupportedRetryWindow
```

---

# 22. Ledger Compaction

Old ledger records may be compacted to:

```text
OperationId
semantic payload hash
final outcome code
committed sequence
```

instead of retaining large response payloads forever.

---

# 23. Audit Retention

Audit retention is separate from journal retention.

Business audit may need to outlive sync journal by years.

---

# 24. Audit vs Erasure Tension

Privacy erasure may request removal of personal data while accounting/audit evidence must remain.

Solution is often:

```text
remove nonessential PII
pseudonymize identity
retain required business evidence
```

rather than deleting all records indiscriminately.

---

# 25. Legal Hold

Define:

```rust
pub struct LegalHoldId(Uuid);
```

Legal hold prevents governed deletion for matching records.

---

# 26. LegalHold

Conceptually:

```rust
pub struct LegalHold {
    pub hold_id: LegalHoldId,
    pub tenant_id: TenantId,
    pub selector: HoldSelector,
    pub reason_code: HoldReasonCode,
    pub created_at: Timestamp,
    pub created_by: PrincipalId,
    pub state: HoldState,
}
```

---

# 27. HoldSelector

May target:

```text
entity
aggregate
user
case
date range
audit category
tenant
custom legal matter
```

---

# 28. Hold Application

Before purge:

```text
retention worker checks active holds
```

Held records transition to:

```text
Held
```

or are simply excluded from deletion.

---

# 29. Hold Immutability

Creating/releasing a legal hold is itself:

```text
security/admin audited
```

---

# 30. Hold Release

Release requires:

```text
authorized action
reason
audit event
```

Then normal retention evaluation resumes.

---

# 31. Erasure Request

Define:

```rust
pub struct ErasureRequestId(Uuid);
```

State machine:

```text
Requested
↓
Validated
↓
Planned
↓
BlockedByHold / Ready
↓
Executing
↓
Verified
↓
Completed
```

---

# 32. Erasure Is Not Immediate Blind Delete

First determine:

```text
what data belongs to subject
what must be retained
what can be anonymized
what blobs exist
what replicas exist
what legal holds apply
```

---

# 33. Erasure Planner

Produces explicit:

```rust
pub struct ErasurePlan {
    pub request_id: ErasureRequestId,
    pub subject: DataSubjectRef,
    pub actions: Vec<ErasureAction>,
    pub blockers: Vec<ErasureBlocker>,
}
```

---

# 34. ErasureAction

```rust
pub enum ErasureAction {
    DeleteEntity,
    PseudonymizeFields,
    RemoveBlob,
    RevokeScope,
    PurgeReplayArtifact,
    CompactAuditIdentity,
    RotateEncryptionKey,
}
```

---

# 35. Pseudonymization

Example:

```text
StudentName = "Deleted User 7F2A"
phone = null
email = null
```

while retaining:

```text
invoice amount
ledger evidence
transaction dates
```

where policy requires.

---

# 36. Stable Pseudonym

Use a deterministic or managed pseudonymous identifier where cross-record linkage must remain.

Avoid reversible mapping unless policy requires.

---

# 37. Cryptographic Erasure

For encrypted data grouped under subject/tenant key:

```text
destroy encryption key
```

can render data unrecoverable.

Useful for:

```text
large blob sets
archives
```

---

# 38. Cryptographic Erasure Caveat

Only valid if:

```text
no plaintext copies
no duplicate unencrypted backup
no shared key with retained data
```

Key architecture must support proper isolation.

---

# 39. Blob Lifecycle

Blob references need separate governance.

When entity deleted:

```text
blob may still be referenced elsewhere
```

Use reference tracking.

---

# 40. Blob GC

Delete blob only when:

```text
reference count / reachability = zero
AND
retention elapsed
AND
no legal hold
```

---

# 41. Content-Addressed Blob Dedup

If same blob is referenced by multiple tenants/users:

```text
physical dedup complicates erasure
```

Need per-owner encryption/key wrapping or logical reference accounting.

---

# 42. Safer Multi-Tenant Blob Policy

Prefer:

```text
tenant-scoped encryption
tenant-scoped object namespace
```

even if physical dedup is reduced.

Governance simplicity often outweighs maximum dedup.

---

# 43. Snapshot Lifecycle

Snapshots can contain deleted/erased data.

Therefore snapshot retention must participate in governance.

---

# 44. Snapshot Purge

If erasure requires removing data from retained snapshots:

Options:

```text
expire old snapshot
build sanitized replacement
cryptographic erase snapshot key
```

---

# 45. Immutable Snapshot Problem

Object-storage snapshots are immutable by design.

Therefore either:

```text
short retention
encrypted per retention domain
or
allow governance-triggered object deletion
```

must be planned.

---

# 46. Backup Tension

Backups may contain erased data.

Application/legal policy decides whether:

```text
backups expire naturally
restore procedure reapplies erasure ledger
```

This is common and practical.

---

# 47. Erasure Ledger

Maintain durable record of completed erasure subjects.

After PITR restore:

```text
replay erasure ledger
```

before restoring service.

---

# 48. Erasure Ledger Content

Store minimal:

```text
ErasureRequestId
subject pseudonymous ref
completion time
policy version
```

Do not preserve erased PII.

---

# 49. Restore Safety

After restore from old backup:

```text
system enters restricted mode
↓
reapply post-backup erasures/revocations
↓
verify
↓
serve traffic
```

---

# 50. Tenant Offboarding

Tenant lifecycle:

```text
Active
Suspended
ReadOnly
ExportPending
DeletionScheduled
Held
Purging
Purged
```

---

# 51. Offboarding Flow

```text
disable new writes
↓
allow/export tenant data
↓
revoke clients/devices
↓
wait grace period
↓
apply legal holds
↓
purge live data
↓
purge snapshots/blobs
↓
purge credentials
↓
retain permitted audit evidence
↓
verify
```

---

# 52. Read-Only Grace Period

Useful before deletion.

Tenant can:

```text
export
review
settle billing
```

without creating new state.

---

# 53. Device Revocation

Before tenant purge:

```text
revoke all devices/scopes
```

Offline devices may still physically contain local copies.

---

# 54. Offline Client Data

Server cannot remotely guarantee immediate deletion on an offline uncontrolled device.

Mitigations:

```text
local encryption
credential expiry
app local purge on reconnect
remote wipe integration
OS-managed storage
```

Document this limitation honestly.

---

# 55. Local Purge Command

On reconnect after revocation:

```text
server returns PurgeRequired
```

client removes affected local scope/store according to policy.

---

# 56. Local Purge Proof

Client may send:

```text
PurgeAcknowledgement
```

after successful deletion.

Useful operationally, but not absolute proof against malicious devices.

---

# 57. Scope Contraction

Part 07 scope eviction is a data lifecycle event.

If data no longer authorized:

```text
evict from local replica
```

even if server entity remains active.

---

# 58. Scope Cache TTL

Inactive authorized scope may be cached.

Revoked scope should follow stronger purge policy.

---

# 59. Derived Cache Lifecycle

Derived caches can usually be deleted aggressively.

They do not block legal retention because source data remains authoritative.

---

# 60. Conflict Record Retention

Conflicts may contain stale sensitive values.

After resolution:

```text
retain minimal metadata
purge full payload after configured period
```

unless audit/legal policy requires longer.

---

# 61. Repair Artifact Retention

Repair bundles can contain canonical data.

Default:

```text
short retention
```

and strong access controls.

---

# 62. Replay Artifact Retention

Part 12 replay bundles may duplicate sensitive state.

Apply:

```text
windowed retention
encryption
legal hold
erasure
```

---

# 63. Import Artifact Retention

Part 09 source files/quarantine rows may contain PII.

Delete once:

```text
migration verified
retention window elapsed
no legal hold
```

---

# 64. Export Lifecycle

Exports are high-risk because they create copies outside primary database.

Track:

```text
who exported
where stored
expiry
deletion status
```

---

# 65. Export Expiry

Generated export URLs/files should have:

```text
short TTL
```

unless explicitly archived.

---

# 66. Data Copy Registry

High-assurance mode can track governed copies:

```text
Snapshot
Export
ReplayBundle
ImportSource
ColdArchive
```

This improves erasure planning.

---

# 67. CopyRef

Conceptually:

```rust
pub struct GovernedCopyRef {
    pub kind: GovernedCopyKind,
    pub location: OpaqueStorageRef,
    pub retention_class: RetentionClassId,
}
```

---

# 68. Avoid Absolute File Paths in Domain Metadata

Use opaque storage references.

---

# 69. Retention Evaluation

A background worker periodically evaluates eligible data.

Pipeline:

```text
select candidate
↓
check minimum retention
↓
check hold
↓
check client watermark / sync safety
↓
check references
↓
plan delete/archive/pseudonymize
↓
execute
↓
verify
```

---

# 70. Retention Worker Is Durable Job

Do not run large purge synchronously in API request.

Part 23 will formalize durable background work.

---

# 71. Retention Checkpoint

Large purge job stores progress.

Restart-safe.

---

# 72. Deletion Idempotency

Deleting same candidate twice should be harmless.

Use:

```text
stable purge action IDs
```

where needed.

---

# 73. PurgePlan

```rust
pub struct PurgePlan {
    pub purge_id: PurgeId,
    pub policy_version: RetentionPolicyVersion,
    pub candidates: Vec<PurgeCandidate>,
}
```

---

# 74. Dry Run

CLI should support:

```text
aequora retention plan --dry-run
```

Show:

```text
records eligible
records held
journal floor impact
snapshots affected
blobs affected
```

---

# 75. RetentionPolicyVersion

Define:

```rust
pub struct RetentionPolicyVersion(u32);
```

Every purge records which policy caused it.

---

# 76. Policy Changes

Changing retention policy must not retroactively purge immediately without evaluation.

Use:

```text
plan
preview
execute
```

for destructive changes.

---

# 77. Shortening Retention

Potentially high-risk.

Require:

```text
admin authorization
dry-run
audit
```

---

# 78. Lengthening Retention

Safer, but increases storage and legal exposure.

Also audit policy change.

---

# 79. Legal Hold Precedence

Rule:

```text
active legal hold > normal retention deletion
```

---

# 80. Erasure vs Legal Hold

If erasure request conflicts with legal hold:

```text
block or partially pseudonymize according to policy
```

Never silently violate either requirement.

---

# 81. ErasureBlocker

Examples:

```text
LegalHold
RequiredFinancialRetention
OpenDispute
PendingExport
ActiveMigration
```

---

# 82. Human Review

Some erasure plans may require manual approval.

Architecture should support:

```text
ReadyForApproval
```

state.

---

# 83. Automatic Erasure

Simple low-risk data classes can be automated after validation.

---

# 84. Data Subject Mapping

Need to know which records belong to a person.

Applications should define:

```rust
pub trait DataSubjectResolver {
    async fn resolve(
        &self,
        subject: DataSubjectRef,
    ) -> Result<DataSubjectGraph, GovernanceError>;
}
```

---

# 85. DataSubjectGraph

Contains:

```text
owned entities
referenced entities
shared entities
blobs
audit refs
exports
```

with action policy per relation.

---

# 86. Shared Entity

Example:

```text
invoice references student
```

Cannot delete invoice if retention requires it.

Instead:

```text
pseudonymize student-identifying fields
retain invoice
```

---

# 87. Ownership Semantics

Do not infer ownership from foreign keys generically.

Application defines governance relationship.

---

# 88. GovernanceRelation

```rust
pub enum GovernanceRelation {
    Owned,
    Referenced,
    Shared,
    Derived,
    AuditOnly,
}
```

---

# 89. Cascades

Avoid uncontrolled DB `ON DELETE CASCADE` for governed erasure if it can delete records whose retention differs.

Use explicit purge planning.

---

# 90. Hard Delete

Hard delete is final physical removal.

Use only after:

```text
sync safety
hold check
retention check
reference check
```

---

# 91. Tombstone Then GC

For synced entities:

```text
business delete
↓
tombstone
↓
clients observe delete
↓
watermark floor advances
↓
tombstone eligible for GC
```

---

# 92. Tombstone Metadata

Include:

```text
entity
version
deletion sequence
deletion time
retention class
```

---

# 93. Resurrection Guard After GC

If tombstone removed, stale client must not be allowed to upload old update as resurrection.

Mechanisms:

```text
minimum client generation
entity identity retirement registry
operation age/cursor checks
```

---

# 94. Retired Entity Identity

For high-risk domains, keep compact:

```text
RetiredEntityId
```

record longer than full tombstone.

This prevents accidental reuse/resurrection.

---

# 95. Identity Reuse Rule

Never reuse old EntityId for a new logical entity.

---

# 96. Deletion Epoch

Optional:

```text
EntityDeletionEpoch
```

or generation can help detect stale resurrection attempts.

---

# 97. Journal Floor

Maintain:

```rust
pub struct JournalFloor {
    pub scope_id: ScopeId,
    pub generation: ScopeGeneration,
    pub min_sequence: Sequence,
}
```

---

# 98. Client Below Floor

Response:

```text
CursorExpired
ResyncRequired
```

Never try to continue incrementally.

---

# 99. Snapshot Floor Coordination

Before raising journal floor:

```text
ensure valid bootstrap snapshot exists
```

for active scope generation.

---

# 100. Retention and Snapshot Lease

Part 10 active snapshot lease pins required journal range.

GC must respect it.

---

# 101. Retention and Anti-Entropy

Part 03 anti-entropy may require historical tombstone/identity metadata.

GC policy should preserve enough to distinguish:

```text
missing
deleted
never existed
```

within supported horizon.

---

# 102. Retention and Replay

Part 12 replay retention policy may pin:

```text
operation ledger
handler metadata
pre-state artifacts
```

---

# 103. Retention and Audit

Part 13 audit retention may be much longer than business entity retention.

Audit record can survive after entity deletion with pseudonymized subject.

---

# 104. Audit Subject After Erasure

Use:

```text
stable pseudonymous subject ref
```

instead of raw personal identifier.

---

# 105. Retention and Live Sync

Part 08 live hints have no durable retention requirement.

---

# 106. Retention and Multi-Process

Part 05 local coordinator manages local purge jobs.

Only leader executes governed local cleanup for shared store.

---

# 107. Local Store Retention

Client may locally retain less data than server.

Policy examples:

```text
30 days recent
current academic year
active scopes only
```

---

# 108. Local Retention vs Scope

Local cache policy cannot override security revocation.

---

# 109. Local Purge State Machine

```text
Planned
↓
WaitingForSafePoint
↓
Purging
↓
RebuildingIndexes
↓
Complete
```

---

# 110. Pending Operations

Do not purge local entity needed by pending operation without explicit handling.

---

# 111. Pending Operation Erasure

If subject erasure requires removing pending local intent:

```text
cancel/quarantine operation
```

according to policy.

If operation may already have reached server:

```text
must reconcile authoritative result first
```

---

# 112. Client Offline Erasure

If device remains offline forever, server cannot guarantee purge.

Therefore enterprise policy may:

```text
expire device credentials
require encrypted local store
mark device noncompliant
```

---

# 113. Encryption at Rest

Strong local encryption supports:

```text
account logout purge
device revocation
cryptographic erase
```

especially on mobile/desktop.

---

# 114. Per-Tenant/Per-Account Keys

Prefer isolated encryption domains.

Destroying one tenant/account key should not affect others.

---

# 115. Key Rotation

Governance may trigger:

```text
key rotation
```

after employee departure/security incident.

Part 15 will define cryptographic key architecture.

---

# 116. Archive Lifecycle

Cold archive may retain:

```text
audit
historic finance
closed academic years
```

while removing from hot DB.

---

# 117. Archive Manifest

Store:

```text
archive ID
tenant
retention class
boundary
record counts
hashes
encryption key reference
```

---

# 118. Archive Query

Do not make cold archive transparently writable.

Read through dedicated archive service/tool.

---

# 119. Restore From Archive

If record must become active again:

```text
explicit restore operation
```

with audit/provenance.

---

# 120. Archive Does Not Mean Deleted

Retention semantics distinguish:

```text
not in hot DB
but still retained
```

---

# 121. Tenant Purge Verification

After offboarding, verify absence from:

```text
primary DB
journal above required evidence
snapshots
blobs
exports
replay artifacts
import quarantine
cache
```

except intentionally retained audit/legal evidence.

---

# 122. Purge Verification Report

```rust
pub struct PurgeVerificationReport {
    pub purge_id: PurgeId,
    pub completed_at: Timestamp,
    pub verified_stores: Vec<StoreVerification>,
    pub retained_exceptions: Vec<RetentionException>,
}
```

---

# 123. Proof Limitations

Verification proves configured stores were checked.

It cannot prove destruction of:

```text
unknown manual copies
external screenshots
malicious device copies
```

Do not overstate.

---

# 124. Governance Inventory

Maintain registry of all known storage surfaces.

Examples:

```text
PrimaryPostgres
ObjectStorage
AuditArchive
SnapshotStore
ExportStore
ClientReplica
```

---

# 125. StorageSurfaceId

```rust
pub struct StorageSurfaceId(u16);
```

---

# 126. Governance Adapter

Each storage surface should implement:

```rust
pub trait GovernanceStore {
    async fn plan(
        &self,
        request: &GovernanceRequest,
    ) -> Result<StorePlan, GovernanceError>;

    async fn execute(
        &self,
        plan: &StorePlan,
    ) -> Result<StoreResult, GovernanceError>;

    async fn verify(
        &self,
        plan: &StorePlan,
    ) -> Result<StoreVerification, GovernanceError>;
}
```

---

# 127. Fail Closed

If one required storage surface is unreachable:

```text
erasure not marked Complete
```

unless policy explicitly permits partial completion.

---

# 128. Partial Completion

State:

```text
PartiallyCompleted
```

with outstanding surfaces.

---

# 129. Retry

Governance jobs are retryable and idempotent.

---

# 130. Governance Correlation

All actions in one erasure/offboarding job share:

```text
CorrelationId
```

Part 02 lineage links sub-actions.

---

# 131. Audit

Every destructive governance action generates administrative/security audit.

---

# 132. No Business Audit Explosion

Deleting millions of rows during tenant purge should not necessarily generate one human audit event per row.

Instead:

```text
PurgeJob summary
+
detailed machine manifest
```

unless legal policy requires item-level records.

---

# 133. Retention Manifest

Large purge can produce canonical manifest:

```text
candidate counts
hashes
range IDs
policy
exceptions
```

---

# 134. Tamper Evidence

Governance manifests can be BLAKE3-hashed and optionally signed.

---

# 135. Data Minimization

Best governance strategy is to avoid collecting data not needed.

Aequora should encourage:

```text
minimal synchronized fields
minimal audit values
short-lived diagnostics
```

---

# 136. Scope Projection Minimization

Part 07 field projection can prevent unnecessary PII from reaching clients at all.

This reduces erasure surface.

---

# 137. Replay Minimization

Part 12 replay policies should prefer digests over full sensitive payload when possible.

---

# 138. Logging Minimization

Operational logs should never become hidden long-term PII archives.

---

# 139. Metrics

Never place personal IDs or raw data in metric labels.

---

# 140. Retention Scheduler

Part 06 QoS:

```text
routine cleanup = Maintenance
security erasure = Critical/Admin
```

---

# 141. Backpressure

Large purge jobs should be throttled to avoid production DB overload.

---

# 142. Chunked Purge

Delete in bounded batches.

Example:

```text
1000 records/transaction
```

depending on DB.

---

# 143. Avoid Long Locks

Do not purge millions of rows in one transaction.

---

# 144. Index Maintenance

Large deletes can create bloat.

Database-specific adapter/runbook may schedule:

```text
VACUUM
reindex
partition drop
```

where applicable.

Core governance API stays DB-agnostic.

---

# 145. Partition Drop Optimization

If entire historical partition is eligible and unheld:

```text
drop partition
```

can be efficient.

Only if retention semantics align exactly.

---

# 146. Tenant-Partitioned Storage

For SaaS with large tenants, tenant partitioning can simplify offboarding.

But do not redesign schema solely for deletion unless scale justifies it.

---

# 147. Cold Storage Encryption

Each archive segment should have:

```text
key reference
```

so cryptographic erasure is possible where required.

---

# 148. Backup Strategy

Backups should have:

```text
finite retention
encryption
restore erasure replay procedure
```

---

# 149. PITR

Point-in-time restore can reintroduce deleted data.

Run governance reconciliation before reopening traffic.

---

# 150. Governance Reconciliation

After restore:

```text
load erasure ledger
load revocation ledger
load legal holds
reapply post-restore governance changes
verify
```

---

# 151. Revocation Ledger

Similar to erasure ledger for:

```text
devices
scopes
credentials
```

if restore could resurrect revoked access.

---

# 152. Legal Hold Restore

Legal hold state must be restored before retention worker resumes.

---

# 153. Disaster Recovery Order

Recommended:

```text
restore DB
↓
restore governance metadata
↓
reapply erasures/revocations
↓
verify audit/hold state
↓
resume service
```

---

# 154. Policy Registry

Aequora should expose:

```text
retention class
legal hold eligibility
erasure behavior
archive behavior
```

per aggregate/field.

---

# 155. Field-Level Governance

Some fields can be erased independently.

Example:

```text
phone/email
```

while entity remains.

Define:

```rust
FieldGovernancePolicy
```

---

# 156. FieldGovernancePolicy

```rust
pub struct FieldGovernancePolicy {
    pub retention_class: RetentionClassId,
    pub erasure: FieldErasureMode,
}
```

---

# 157. FieldErasureMode

```rust
pub enum FieldErasureMode {
    Null,
    ReplaceWithPseudonym,
    Hash,
    KeepRequired,
}
```

---

# 158. Schema Migration

If field governance policy changes, migration may be required.

Example:

```text
previously stored raw phone in audit
now hash-only
```

Need cleanup of historical audit values where legally allowed.

---

# 159. Governance CI

Generate report of:

```text
aggregate
field
retention class
audit policy
erasure mode
legal-hold eligibility
```

---

# 160. Missing Policy

Production startup/CI should reject sensitive domain fields with no governance classification if application opts into strict mode.

---

# 161. Strict Governance Mode

```rust
GovernanceMode::Strict
```

requires explicit classification for:

```text
synchronized business fields
audit values
blobs
```

---

# 162. Default Governance Mode

For general library usability:

```text
Standard
```

with conservative defaults.

---

# 163. Admin CLI

Suggested:

```text
aequora retention plan
aequora retention execute
aequora legal-hold create
aequora legal-hold release
aequora erasure plan
aequora erasure execute
aequora erasure verify
aequora tenant offboard
aequora governance inventory
```

---

# 164. Dry Run Everywhere

Destructive commands should support:

```text
--dry-run
```

and show affected counts/surfaces.

---

# 165. Confirmation

High-risk destructive admin operations should require explicit operator confirmation at CLI/API level.

---

# 166. Admin API

Large destructive tasks create durable job and return:

```text
job ID
```

rather than blocking request.

---

# 167. Authorization

Separate permissions:

```text
Retention.Plan
Retention.Execute
LegalHold.Manage
Erasure.Execute
Tenant.Offboard
Governance.Verify
```

---

# 168. Separation of Duties

Enterprise deployments may require:

```text
one person requests
another approves
```

for legal hold release or mass purge.

Architecture should support optional approval workflow.

---

# 169. Approval State

```text
PendingApproval
Approved
Rejected
```

before execution.

---

# 170. Governance Job State Machine

```text
Requested
↓
Planned
↓
AwaitingApproval
↓
Executing
↓
Verifying
↓
Completed
```

Alternative branches:

```text
Blocked
Failed
PartiallyCompleted
Canceled
```

---

# 171. Cancellation

Before destructive execution:

```text
cancel safely
```

After partial execution:

```text
cannot necessarily restore deleted data
```

State this clearly.

---

# 172. Irreversible Boundary

Governance plan should mark:

```text
point of no return
```

for destructive actions.

---

# 173. Archive-Before-Delete

Optional policy:

```text
export immutable archive
↓
verify archive
↓
delete hot data
```

---

# 174. Archive Is Still Retention

If privacy erasure demands deletion:

```text
archive cannot be used as loophole
```

Erasure planner includes archives.

---

# 175. Legal Hold Copy

If legal hold requires preservation before source deletion:

```text
create protected held copy
```

only if policy permits.

---

# 176. Held Copy Security

Encrypt and restrict heavily.

---

# 177. Client-Side Governance Metadata

Client stores:

```text
scope revocation state
local purge state
retention policy generation
```

where necessary.

---

# 178. Policy Generation

Define:

```rust
pub struct GovernancePolicyGeneration(u32);
```

Clients need not understand all server retention law, but may need new purge behavior after generation change.

---

# 179. Server-Controlled Purge Directive

Protocol message:

```rust
pub struct PurgeDirective {
    pub directive_id: PurgeDirectiveId,
    pub scope_id: ScopeId,
    pub reason: PurgeReason,
    pub minimum_store_generation: Option<LocalStoreGeneration>,
}
```

---

# 180. Purge Directive Idempotency

Client records processed directive IDs.

Repeated directive is safe.

---

# 181. Purge Ack

```rust
pub struct PurgeAck {
    pub directive_id: PurgeDirectiveId,
    pub completed_at: Timestamp,
}
```

---

# 182. Local Purge Failure

If local DB cannot purge due corruption/disk error:

```text
client enters restricted state
```

for revoked sensitive scope.

Do not continue exposing stale unauthorized data if policy can prevent it.

---

# 183. App-Level Lock

If purge cannot complete:

```text
lock affected account/store
```

until repair/reinitialization.

---

# 184. Local Encryption Key Delete

For account logout/offboarding:

```text
destroy local account encryption key
```

may be strongest fast purge mechanism, followed by physical cleanup.

---

# 185. Storage Adapter Requirements

Local adapter should support:

```text
scope purge
generation replacement
tombstone GC
enumeration for verification
```

Server adapter should support:

```text
bounded deletion
hold-aware selection
journal floor management
```

---

# 186. Governance Capability

```rust
pub struct GovernanceCapabilities {
    pub hard_delete: bool,
    pub transactional_batch_delete: bool,
    pub generation_drop: bool,
    pub cryptographic_erase: bool,
}
```

---

# 187. Certification

Tier-A adapters should pass lifecycle tests.

---

# 188. Property Tests

Generate:

```text
devices at different cursors
tombstones
revocations
holds
retention changes
```

Assert no unsafe tombstone deletion.

---

# 189. Tombstone GC Test

Client A cursor > delete.
Client B cursor < delete.

Expected:

```text
tombstone retained
```

After B retired:

```text
eligible
```

---

# 190. Resurrection Test

GC tombstone then stale retired client reconnects.

Expected:

```text
rebootstrap required
old update cannot resurrect entity
```

---

# 191. Legal Hold Test

Record retention expired but hold active.

Expected:

```text
not purged
```

---

# 192. Erasure Test

Subject has:

```text
profile
invoice
blob
audit
```

Expected:

```text
profile deleted/pseudonymized
invoice retained with minimized identity
blob removed if unreferenced
audit pseudonymized according to policy
```

---

# 193. Restore Test

Restore backup predating erasure.

Expected:

```text
erasure ledger reapplied before traffic
```

---

# 194. Scope Revocation Test

Offline client reconnects after scope revoked.

Expected:

```text
no new data
purge directive
local scope removed
```

---

# 195. Partial Failure Test

Primary DB purge succeeds.
Object storage unavailable.

Expected:

```text
job PartiallyCompleted
not Completed
retry outstanding surface
```

---

# 196. Governance Invariants

Add:

## AEQ-INV-GOV001

```text
A tombstone is not garbage-collected while any valid retained client can still legally resume from before its deletion boundary.
```

## AEQ-INV-GOV002

```text
A client below the retained journal floor must rebootstrap rather than continue incrementally.
```

## AEQ-INV-GOV003

```text
Active legal hold prevents normal retention purge of covered governed data.
```

## AEQ-INV-GOV004

```text
Erasure never silently deletes records that policy requires to retain; it uses explicit pseudonymization/minimization where necessary.
```

## AEQ-INV-GOV005

```text
Governance job is not marked complete until all required storage surfaces report successful execution and verification.
```

## AEQ-INV-GOV006

```text
Tenant offboarding revokes authoritative write access before destructive purge begins.
```

---

# 197. Additional Invariants

## AEQ-INV-GOV007

```text
Restore procedures reapply post-backup erasure and revocation state before normal service resumes.
```

## AEQ-INV-GOV008

```text
A stale retired client cannot resurrect a physically purged entity using obsolete state.
```

## AEQ-INV-GOV009

```text
Sensitive governed copies such as exports, replay bundles, and snapshots participate in erasure/retention planning when registered as governed storage surfaces.
```

---

# 198. Metrics

```text
retention_candidates_total
retention_purged_total
retention_held_total
erasure_jobs_total
erasure_blocked_total
erasure_partial_total
legal_holds_active
journal_floor_sequence
tombstone_gc_total
```

Avoid subject IDs as metric labels.

---

# 199. Logs

Structured:

```text
retention_plan_created
legal_hold_applied
erasure_started
erasure_surface_completed
tenant_purge_completed
tombstone_gc_advanced
journal_floor_advanced
```

---

# 200. Alerting

Alert on:

```text
governance job stuck
erasure verification failed
unexpected held-data growth
journal floor unable to advance
snapshot store purge failure
restore governance reconciliation failure
```

---

# 201. Recommended Modules

```text
aequora-governance/
├── policy.rs
├── retention.rs
├── legal_hold.rs
├── erasure.rs
├── subject_graph.rs
├── purge_plan.rs
├── journal_floor.rs
├── tombstone_gc.rs
├── offboarding.rs
├── inventory.rs
└── verify.rs
```

Client:

```text
aequora-client/
└── governance/
    ├── purge_directive.rs
    ├── local_purge.rs
    └── ack.rs
```

---

# 202. Governance Store Registry

Server:

```rust
GovernanceRegistry::new()
    .register(primary_db)
    .register(snapshot_store)
    .register(blob_store)
    .register(export_store)
    .register(replay_store);
```

---

# 203. Plug-and-Play Default

Developers classify domain data once.

Aequora handles:

```text
journal floors
tombstone safety
retention scheduling
scope purge
governance job state
```

without application code manipulating sync metadata directly.

---

# 204. School ERP Examples

Possible classifications:

```text
Student contact details
    erasure/pseudonymization eligible

Attendance history
    long retention by school policy

Fee/payment ledger
    long-term immutable evidence

Temporary generated report
    short retention

Uploaded document
    retention by document category

Audit/security history
    independent retention class
```

Aequora should not decide the durations; the ERP policy layer does.

---

# 205. Finance Example

A payment cannot simply be deleted because user requests account erasure.

Instead:

```text
retain payment/ledger evidence
remove unnecessary personal fields
replace subject reference with pseudonymous archival ID
```

subject to application/legal policy.

---

# 206. Local-First Reality

A local-first system increases governance complexity because data exists on devices.

Therefore deployment policy should combine:

```text
scope minimization
local encryption
credential expiry
purge directives
device retirement
rebootstrap floors
```

---

# 207. Completion Criteria

Part 14 is complete when:

```text
[ ] retention classes defined
[ ] deletion modes defined
[ ] legal hold model defined
[ ] erasure planner defined
[ ] data-subject graph defined
[ ] tombstone GC safety defined
[ ] journal floor defined
[ ] stale-client resurrection prevention defined
[ ] operation-ledger retention defined
[ ] snapshot/blob/export/replay lifecycle integrated
[ ] tenant offboarding workflow defined
[ ] backup/PITR erasure replay defined
[ ] client purge directive defined
[ ] governance storage registry defined
[ ] dry-run/approval/audit model defined
[ ] lifecycle correctness tests defined
```

---

# 208. Final Architecture

```text
                     GOVERNANCE POLICY
                            │
             ┌──────────────┼───────────────┐
             ▼              ▼               ▼
         Retention       Legal Hold       Erasure
             │              │               │
             └──────────────┼───────────────┘
                            ▼
                      Governance Planner
                            │
                 discover affected copies
                            │
         ┌──────────────────┼──────────────────┐
         ▼                  ▼                  ▼
     Primary DB        Snapshot/Blob       Client Scope
         │                  │                  │
         ▼                  ▼                  ▼
   purge/pseudonymize   delete/archive     purge directive
         │                  │                  │
         └──────────────────┼──────────────────┘
                            ▼
                       Verification
                            │
                            ▼
                         Complete

Tombstone lifecycle:

Business Delete
      │
      ▼
   Tombstone
      │
      ▼
Client Watermarks + Journal Floor
      │
      ▼
All valid old clients crossed boundary
or are retired/rebootstrap-only
      │
      ▼
Safe Tombstone GC
```

The architectural principle is:

> **Aequora should retain data only as long as synchronization correctness, business policy, audit obligations, legal holds, or recovery requirements actually need it—and should make every destructive transition explicit, verifiable, restartable, and safe for offline replicas.**

This gives Aequora a complete lifecycle model for enterprise retention, privacy erasure, legal hold, offboarding, tombstone cleanup, snapshots, blobs, backups, and local client copies without weakening synchronization correctness.
