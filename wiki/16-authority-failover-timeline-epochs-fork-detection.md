# Aequora Sync — Part 16

# Authority Failover, Timeline Epochs, Fork Detection, and Disaster Promotion Architecture

## 1. Purpose

Aequora is designed around a server-authoritative synchronization model.

That gives a clear rule during normal operation:

```text
client state is provisional
server state is authoritative
```

But production systems eventually encounter events such as:

```text
primary database failure
region outage
database restore from backup
PITR recovery
manual disaster promotion
storage corruption
accidental rollback
standby promotion
migration to a new authoritative database
```

These events can create a dangerous condition:

```text
two servers both believe they are authority
```

or:

```text
a restored server has an older history but reuses the same cursor timeline
```

If clients continue syncing without detecting the timeline change, old and new histories can be silently mixed.

Aequora therefore needs an explicit architecture for:

```text
authority identity
authority epochs
timeline generation
promotion
demotion
fork detection
restore handling
cursor invalidation
journal continuity
operation-ledger continuity
client recovery
```

The central rule is:

> **Aequora must never let two different authoritative histories masquerade as one cursor timeline.**

---

## 2. Goals

The authority-failover subsystem should provide:

```text
single-writer authority
safe standby promotion
explicit authority epoch
fork detection
restore/PITR safety
cursor invalidation
client rebootstrap when required
operation dedup continuity where possible
audit continuity
disaster recovery
operator-safe promotion workflows
```

---

## 3. Non-Goals

This subsystem is not necessarily:

```text
multi-primary replication
consensus across arbitrary databases
active-active global writes
automatic cross-region Raft
```

Aequora's recommended architecture remains:

```text
one authoritative writer timeline
```

with failover to a replacement authority when necessary.

---

## 4. Authority Identity

Define a stable identifier for one logical authoritative deployment:

```rust
pub struct AuthorityId(Uuid);
```

This identifies the logical authority domain.

Example:

```text
School ERP production cluster
```

The physical database host may change while `AuthorityId` remains the same.

---

## 5. Authority Epoch

Define:

```rust
pub struct AuthorityEpoch(u64);
```

The epoch changes whenever the authoritative timeline is replaced or cannot prove strict continuation.

Examples:

```text
PITR restore to earlier point
disaster promotion from stale standby
manual history rewrite
migration that cannot preserve exact journal continuity
```

---

## 6. AuthorityGeneration vs ScopeGeneration

Do not confuse:

```text
AuthorityEpoch
ScopeGeneration
LocalStoreGeneration
IntegrityGeneration
```

They serve different purposes.

`AuthorityEpoch` describes:

```text
the global authoritative history timeline
```

---

## 7. Authority Descriptor

Conceptually:

```rust
pub struct AuthorityDescriptor {
    pub authority_id: AuthorityId,
    pub epoch: AuthorityEpoch,
    pub role: AuthorityRole,
    pub instance_id: AuthorityInstanceId,
}
```

---

## 8. AuthorityInstanceId

Define:

```rust
pub struct AuthorityInstanceId(Uuid);
```

Ephemeral or semi-stable identity of one deployed server/database authority instance.

Useful for operations/diagnostics.

---

## 9. Authority Roles

```rust
pub enum AuthorityRole {
    Primary,
    Standby,
    ReadReplica,
    Recovering,
    Demoted,
}
```

---

## 10. Single-Writer Invariant

> **At most one authority instance may accept authoritative writes for one AuthorityId and AuthorityEpoch.**

This is the most important failover invariant.

---

## 11. Write Fencing

Do not rely only on operator convention.

Use explicit write fencing.

Possible mechanisms:

```text
database role/permission
lease row
cloud database primary role
promotion token
external coordinator
storage-level fencing
```

---

## 12. Authority Fence Token

Optional logical type:

```rust
pub struct AuthorityFenceToken(u64);
```

Every primary promotion increments the token.

Authoritative commits verify current token where architecture allows.

---

## 13. Why Fencing Matters

Scenario:

```text
Region A primary loses network
Region B promoted
Region A recovers
```

Without fencing, both may accept writes.

With promotion fencing:

```text
A token 10
B token 11
```

A must be unable to commit as authority after B promotion.

---

## 14. External Fencing

Best fencing often comes from infrastructure:

```text
managed Postgres primary role
storage lease
STONITH
cloud control plane
network isolation
```

Aequora should integrate with these rather than invent a weaker distributed lock.

---

## 15. Database-Level Fencing

If self-hosted:

```text
authority metadata row
+
transactional fence token
```

may protect application-level writes.

But it does not protect against two independently writable database copies that cannot see each other.

For disaster failover, infrastructure fencing is essential.

---

## 16. Promotion Preconditions

Before promoting standby:

```text
confirm old primary fenced/unreachable for writes
determine replication position
determine journal continuity
determine operation-ledger continuity
determine audit continuity
determine snapshot/catalog state
```

---

## 17. Promotion Categories

Distinguish:

```rust
pub enum PromotionClass {
    LosslessContinuation,
    PotentialDataLoss,
    RestoredTimeline,
    NewAuthorityMigration,
}
```

---

## 18. LosslessContinuation

Use when standby is fully caught up and strict history continuity is proven.

Example:

```text
synchronous replicated PostgreSQL standby
```

Then:

```text
AuthorityEpoch may remain unchanged
```

if all authoritative metadata is preserved.

---

## 19. PotentialDataLoss

Example:

```text
async replica lagged 3 seconds
primary destroyed
standby promoted
```

Some committed operations may be missing.

This is a history fork risk.

Recommended:

```text
increment AuthorityEpoch
```

unless losslessness can be proven.

---

## 20. RestoredTimeline

PITR restore to earlier sequence:

```text
always new AuthorityEpoch
```

because old clients may possess state/events beyond restore point.

---

## 21. NewAuthorityMigration

If moving to a new authoritative store and exact metadata continuity is preserved:

```text
epoch may remain
```

If not:

```text
new epoch
```

Correctness over convenience.

---

## 22. Cursor Extension

All server cursors should include authority epoch.

Conceptually:

```rust
pub struct Cursor {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub scope_id: ScopeId,
    pub scope_generation: ScopeGeneration,
    pub sequence: Sequence,
}
```

---

## 23. Cursor Validation

Server rejects cursor if:

```text
authority ID mismatches
authority epoch mismatches
scope generation mismatches
sequence below retention floor
```

---

## 24. Epoch Mismatch Response

Typed response:

```text
AuthorityChanged
RebootstrapRequired
```

Do not try to continue normal incremental replay.

---

## 25. Why Epoch Must Be Explicit

Suppose old authority:

```text
epoch 4
sequence 1000
```

is restored to:

```text
sequence 800
```

and starts issuing:

```text
801, 802...
```

If epoch remains 4, clients holding cursor 1000 cannot distinguish the new timeline.

With:

```text
epoch 5
```

the mismatch is unambiguous.

---

## 26. Timeline Identity

Logical timeline key:

```text
AuthorityId + AuthorityEpoch
```

All authoritative sequences exist inside this namespace.

---

## 27. Sequence Reset

When epoch changes, sequence may:

```text
restart
or continue numerically
```

Either is acceptable because epoch disambiguates.

Recommended:

```text
allow sequence restart from 0/1
```

if implementation simpler.

---

## 28. Do Not Infer Continuity From Sequence Number

Sequence `1000` in epoch 4 and sequence `1000` in epoch 5 are unrelated positions.

---

## 29. Journal Metadata

Every authoritative event implicitly or explicitly belongs to:

```text
AuthorityEpoch
```

---

## 30. Operation Ledger Metadata

Operation ledger also belongs to authority timeline.

But OperationId identity spans retries across failover.

---

## 31. Lossless Failover Dedup

If operation ledger replicated synchronously/losslessly:

```text
new primary can deduplicate old OperationIds
```

seamlessly.

---

## 32. Ledger Loss Scenario

If promoted standby lost recent ledger entries:

```text
operation may be retried
```

and server may not know it previously committed on lost primary.

This is why potential-data-loss promotion must be treated carefully.

---

## 33. Operation Ambiguity After Data-Loss Failover

Possible states:

```text
client thinks operation may have committed
new authority does not contain it
```

Aequora needs a defined reconciliation rule.

---

## 34. Epoch-Bound Retry Policy

After epoch change, client should not blindly retry all possibly-sent operations immediately.

Classify:

```text
Unsent
ConfirmedCommitted
PossiblyCommittedOldEpoch
Rejected
```

---

## 35. Unsent Operation

Safe to submit to new epoch if current authorization/domain semantics still permit.

---

## 36. ConfirmedCommitted Operation

If its authoritative result is absent in new epoch due restore/data loss:

```text
history has been rolled back
```

This is not ordinary retry.

System may need:

```text
recovery/reconciliation policy
```

---

## 37. PossiblyCommittedOldEpoch

Needs explicit handling.

Possible strategy:

```text
re-submit same OperationId with epoch-transition context
```

only if application profile says operation is safely replayable.

---

## 38. Finance Warning

Financial/payment operations require special reconciliation.

Do not auto-replay a possibly committed external payment after authority loss.

Need:

```text
external provider reconciliation
idempotency key lookup
manual review
```

---

## 39. Operation Recovery Policy

Part 11 consistency profile can declare:

```rust
pub enum EpochRecoveryPolicy {
    SafeReplay,
    VerifyExternalState,
    ManualReview,
    DropIfDerived,
}
```

---

## 40. SafeReplay

Examples:

```text
SetTheme
SetPhone
```

may be safely resubmitted under normal validation.

---

## 41. VerifyExternalState

Examples:

```text
payment
email with legal effect
external reservation
```

Need side-effect reconciliation first.

---

## 42. ManualReview

Use for sensitive ambiguous operations.

---

## 43. DropIfDerived

Derived projections simply rebuild.

---

## 44. Client Epoch Transition State Machine

```text
Normal
↓ receives AuthorityChanged
FreezeAuthoritativeReconciliation
↓
ClassifyPendingOperations
↓
AcquireNewScopeManifest
↓
BootstrapNewEpoch
↓
RebaseSafeUnsentIntent
↓
ResolveAmbiguousOldEpochOps
↓
Resume
```

---

## 45. Preserve Pending Intent

Epoch transition must not delete offline work.

Keep:

```text
OperationId
payload
lineage
ever_sent
old epoch metadata
```

---

## 46. Pending Operation Epoch Metadata

Add:

```text
created_under_epoch
first_sent_under_epoch
```

where useful.

---

## 47. Old-Epoch Signature

Part 15 device signature remains valid for semantic operation bytes.

But new server may require:

```text
new epoch binding
```

if epoch is part of signed semantics.

Recommendation:

```text
do not include authority epoch in immutable operation semantic digest unless operation semantics require it
```

so safe retry across lossless failover remains possible.

---

## 48. Epoch Transition Envelope

If needed, transport can add:

```text
current authority epoch
```

outside immutable operation payload.

---

## 49. Snapshot After Epoch Change

New authority should publish:

```text
fresh snapshots
```

for new epoch.

Old snapshots must not be accepted as current unless explicitly compatible.

---

## 50. Snapshot Manifest

Part 10 snapshot manifest includes:

```text
AuthorityId
AuthorityEpoch
```

---

## 51. Anti-Entropy

Part 03 integrity manifests also include:

```text
AuthorityEpoch
```

Never compare roots across epochs as if same timeline.

---

## 52. Audit Chain

Part 13 may use:

```text
AuditEpoch
```

aligned with AuthorityEpoch or derived transition.

If continuity cannot be proven:

```text
start new audit epoch
```

and record linkage to previous checkpoint.

---

## 53. Signed Authority Transition

High-assurance mode can create:

```text
AuthorityTransitionManifest
```

signed by server/root key.

---

## 54. AuthorityTransitionManifest

Conceptually:

```rust
pub struct AuthorityTransitionManifest {
    pub authority_id: AuthorityId,
    pub old_epoch: AuthorityEpoch,
    pub new_epoch: AuthorityEpoch,
    pub promotion_class: PromotionClass,
    pub old_final_sequence: Option<Sequence>,
    pub new_base_sequence: Sequence,
    pub reason: TransitionReason,
    pub created_at: Timestamp,
    pub signature: SignatureEnvelope,
}
```

---

## 55. Transition Reason

Examples:

```text
PlannedMigration
RegionFailover
PITRRestore
CorruptionRecovery
StandbyPromotion
```

---

## 56. Fork Detection

A fork exists when two authoritative histories share:

```text
AuthorityId
AuthorityEpoch
```

but diverge after some point.

This must be treated as critical integrity failure.

---

## 57. Fork Fingerprint

Periodically compute signed checkpoint:

```text
epoch
sequence N
journal root hash
```

Two authorities claiming same epoch/N with different roots prove fork.

---

## 58. Journal Checkpoint

```rust
pub struct JournalCheckpoint {
    pub authority_id: AuthorityId,
    pub epoch: AuthorityEpoch,
    pub sequence: Sequence,
    pub journal_root: Digest,
}
```

---

## 59. Checkpoint Exchange

Standbys/monitoring can compare checkpoints.

Mismatch:

```text
critical
```

Do not auto-merge.

---

## 60. Fork Response

If fork detected:

```text
stop writes
quarantine affected authority
require operator recovery
```

---

## 61. No Automatic History Merge

Aequora should not attempt to merge two authoritative server histories automatically.

That is a business/disaster-recovery decision.

---

## 62. Fork Recovery Options

Possible:

```text
choose one branch
manually reconcile lost operations
create new epoch
import selected compensating actions
```

---

## 63. New Epoch After Fork Resolution

Always create:

```text
new AuthorityEpoch
```

for reconciled authority.

---

## 64. Split-Brain Prevention

Operational architecture should prioritize prevention:

```text
fencing
single-writer DB role
promotion workflow
network isolation
```

over post-fork repair.

---

## 65. Standby Replication Requirements

A production standby should replicate at least:

```text
business state
journal
operation ledger
scope metadata
audit records
authority metadata
```

---

## 66. Snapshot Catalog Replication

Snapshot objects may live in shared object storage.

Catalog metadata should remain consistent.

After promotion, stale snapshot publication must be disabled.

---

## 67. Blob Storage

Blob metadata and content references need failover continuity.

If object storage remains shared:

```text
new authority reuses
```

If not:

```text
blob failover architecture needed separately
```

---

## 68. Promotion Readiness

Standby exposes:

```text
replication lag
journal checkpoint
ledger checkpoint
audit checkpoint
```

Operators can assess risk.

---

## 69. Promotion Readiness State

```rust
pub enum PromotionReadiness {
    LosslessReady,
    Lagging { estimated_gap: u64 },
    Unknown,
    Unsafe,
}
```

---

## 70. Automatic Promotion

Only consider automatic promotion when:

```text
fencing is reliable
lossless/acceptable replication semantics known
runbook tested
```

Otherwise prefer explicit operator promotion.

---

## 71. Promotion Command

CLI concept:

```text
aequora authority promote
```

should require:

```text
old primary fencing confirmation
readiness report
promotion class
explicit approval
```

---

## 72. Force Promotion

If primary destroyed and data loss accepted:

```text
--allow-data-loss
```

or equivalent explicit destructive acknowledgement.

Never hide this.

---

## 73. Promotion Transaction

On promoted DB:

```text
acquire primary role/fence
set AuthorityRole::Primary
possibly increment AuthorityEpoch
record transition
enable writes
```

---

## 74. Demotion

Old primary:

```text
disable writes
mark Demoted
```

before rejoining.

---

## 75. Rejoin

Old primary cannot simply resume replication as peer if its timeline diverged.

Usually:

```text
reinitialize from current authority
```

---

## 76. Standby Rebuild

```text
take new base backup/snapshot
↓
restore standby
↓
replicate current epoch
```

---

## 77. PITR Restore

Flow:

```text
stop service
↓
restore DB to point T
↓
increment AuthorityEpoch
↓
reconcile governance/audit state
↓
build fresh snapshots
↓
start restricted verification
↓
resume clients
```

---

## 78. Governance Reconciliation

Part 14 after PITR:

```text
reapply erasures
revocations
legal holds
```

before traffic.

---

## 79. External Side Effects

Part 23/12 side-effect jobs may have executed after restore point.

Need reconciliation.

Example:

```text
email sent
payment captured
webhook delivered
```

but DB restored before recording result.

---

## 80. Side-Effect Reconciliation

Before replaying ambiguous side-effect intents:

```text
query provider by idempotency key/reference
```

where possible.

---

## 81. Payment Recovery

Use provider idempotency key tied to:

```text
OperationId / SideEffectIntentId
```

This prevents double charge during epoch recovery.

---

## 82. Email Recovery

Email send may not be queryable reliably.

Policy may accept duplicate risk, suppress retry, or require manual review.

---

## 83. Disaster Recovery Classes

Classify domain side effects:

```text
RecoverableByIdempotency
QueryableExternalState
NonQueryableSideEffect
ManualReconciliation
```

---

## 84. Restore Barrier

After timeline-changing restore:

```text
do not immediately execute old pending side-effect jobs
```

until reconciliation phase completes.

---

## 85. Authority Recovery Mode

Server state:

```rust
pub enum AuthorityRuntimeMode {
    Serving,
    Recovering,
    ReadOnlyVerification,
    PromotionPending,
}
```

---

## 86. ReadOnlyVerification

After restore/promotion:

```text
reads/admin verification allowed
writes blocked
```

until integrity checks pass.

---

## 87. Verification Checklist

Before serving writes:

```text
authority metadata valid
journal consistent
operation ledger consistent
audit chain valid
governance reconciliation complete
scope metadata valid
snapshot strategy ready
side-effect reconciliation status known
```

---

## 88. Integrity Verification

Use Part 03:

```text
canonical roots
journal/entity consistency
```

---

## 89. Audit Verification

Use Part 13 signed checkpoints/hash chain.

---

## 90. Crypto Verification

Use Part 15:

```text
key registry
signed transition manifest
artifact signatures
```

---

## 91. Authority Metadata Table

Logical:

```text
aequora_authority_state
```

Fields:

```text
authority_id
authority_epoch
authority_instance_id
role
fence_token
promotion_class
transition_id
updated_at
```

---

## 92. AuthorityTransitionId

```rust
pub struct AuthorityTransitionId(Uuid);
```

---

## 93. Transition Audit

Every promotion/demotion/restore is:

```text
Administrative/Security audit event
```

with actor/system and reason.

---

## 94. Authority Epoch Persistence

Must be stored in authoritative DB and included in backups.

But after PITR restore:

```text
operator/control-plane increments it
```

before serving.

---

## 95. External Epoch Anchor

High-assurance deployments may also store latest epoch outside DB.

Why:

```text
PITR could restore old authority metadata too
```

External control plane prevents accidental epoch rollback.

---

## 96. Epoch Registry

Possible external record:

```text
AuthorityId -> highest issued AuthorityEpoch
```

stored in:

```text
KMS metadata
control-plane DB
WORM object
```

---

## 97. Epoch Rollback Protection

On startup:

```text
local DB epoch < external highest epoch
```

means:

```text
do not serve
```

unless explicit recovery procedure.

---

## 98. Client Highest Epoch

Client may persist highest observed epoch for AuthorityId.

If server presents lower epoch:

```text
reject as rollback
```

unless trust-reset/admin recovery flow.

---

## 99. AuthorityRollbackDetected

Typed error:

```text
AuthorityRollbackDetected
```

This protects clients from stale restored servers.

---

## 100. Higher Epoch

If server presents higher epoch:

```text
normal transition flow
```

---

## 101. AuthorityId Change

If endpoint suddenly presents different AuthorityId:

```text
treat as different authority
```

Do not silently adopt.

Potential:

```text
server misconfiguration
tenant migration
wrong environment
```

---

## 102. Trust Reset

Explicit user/admin action may allow changing AuthorityId for migration/testing.

Never automatic in production.

---

## 103. Multi-Environment Safety

Dev/staging/prod must use distinct:

```text
AuthorityId
```

This prevents accidental client sync to wrong environment.

---

## 104. Tenant Move Between Authorities

If tenant migrates to another authority cluster:

```text
TenantAuthorityMigration
```

requires signed migration manifest/explicit client trust transition.

---

## 105. TenantAuthorityMigration

Could preserve logical tenant but change:

```text
AuthorityId
```

Client must rebootstrap.

---

## 106. Migration Token

Server may issue:

```text
signed migration token
```

authorizing transition from Authority A to B.

Part 15 signing supports trust.

---

## 107. Multi-Region Read Replicas

Read replicas may serve:

```text
read-only queries
snapshot chunks
```

but not authoritative sync writes unless promoted.

---

## 108. Sync Pull From Replica

Danger:

```text
replica lag
```

Could serve stale pull.

Recommended initial rule:

```text
sync exchange served by authority primary
```

or by replica with explicit safe watermark/lag semantics.

Part 17 will cover multi-region reads.

---

## 109. Write Routing

All authoritative operations route to current primary.

Load balancer/control plane must know active authority.

---

## 110. DNS Failover

DNS alone is not enough for write fencing.

Old endpoint may still accept traffic.

Use server/database role enforcement.

---

## 111. Client Endpoint Discovery

Client may use:

```text
stable service URL
```

behind failover routing.

Authority epoch/ID still validates logical continuity.

---

## 112. Redirect

Server may return:

```text
AuthorityMoved
```

with trusted endpoint metadata if architecture supports explicit endpoint migration.

---

## 113. Endpoint Trust

Do not trust arbitrary redirect URL from unverified source.

Use TLS/trusted signed migration metadata.

---

## 114. Promotion and Live Hints

Part 08 sockets drop or reconnect.

New leader:

```text
clients reconnect
run immediate cursor catch-up
```

No live-hint continuity required.

---

## 115. Promotion and Scheduler

Part 06 sees:

```text
transport failures
authority changed
```

and enters recovery path, not aggressive retry storm.

---

## 116. Backoff After Failover

Clients should use jitter.

Avoid all clients rebootstrap simultaneously.

---

## 117. Bootstrap Herd

If epoch change forces fleet rebootstrap:

```text
snapshot reuse
CDN/object storage
admission control
jitter
```

are critical.

---

## 118. Partial Fleet Transition

Some clients offline during epoch change.

When they return weeks later:

```text
old epoch cursor rejected
rebootstrap
```

---

## 119. Scope Version During Epoch Change

New authority may preserve ScopeVersion but cursor still invalid due epoch change.

AuthorityEpoch supersedes incremental continuity.

---

## 120. IntegrityGeneration

May remain same if canonical hashing unchanged.

Still do not compare old/new authority roots without explicit transition semantics.

---

## 121. OperationId Across Epochs

OperationId remains globally unique.

This helps identify ambiguous historical operations across transitions.

---

## 122. Reconciliation Registry

For potentially lost operations, server/admin may maintain:

```text
OperationRecoveryRecord
```

linking old epoch status to new resolution.

---

## 123. OperationRecoveryRecord

Conceptually:

```rust
pub struct OperationRecoveryRecord {
    pub operation_id: OperationId,
    pub old_epoch: AuthorityEpoch,
    pub resolution: EpochOperationResolution,
}
```

---

## 124. EpochOperationResolution

```rust
pub enum EpochOperationResolution {
    Replayed,
    ConfirmedExternally,
    Compensated,
    DroppedDerived,
    ManualResolved,
}
```

---

## 125. Recovery Audit

Every ambiguous high-value operation resolution should be audited.

---

## 126. Finance Example

Old primary committed:

```text
PaymentCaptured
```

but standby did not receive it.

After promotion:

```text
provider says charge exists
```

New authority records:

```text
RecoveryConfirmedExternalPayment
```

rather than charging again.

---

## 127. Inventory Example

If lost update cannot be proven externally:

```text
manual stock reconciliation
```

may be required.

---

## 128. Preference Example

Lost theme preference:

```text
safe replay
```

no special review.

---

## 129. Derived Projection Example

Rebuild from current authority.

---

## 130. Backup Metadata

Backups should record:

```text
AuthorityId
AuthorityEpoch
highest journal sequence
audit checkpoint
governance checkpoint
```

---

## 131. Restore Candidate Report

Before restore, tooling can show:

```text
backup epoch
sequence
age
expected data-loss window
```

---

## 132. DR Runbook Automation

CLI:

```text
aequora authority status
aequora authority readiness
aequora authority promote
aequora authority demote
aequora authority restore-plan
aequora authority verify
aequora authority fork-check
```

---

## 133. Promotion Dry Run

`promote --dry-run` should report:

```text
old primary fence status
replication lag
epoch decision
journal checkpoint
ledger checkpoint
audit checkpoint
side-effect risk
```

---

## 134. Promotion Approval

High-risk `PotentialDataLoss` promotion requires explicit approval.

---

## 135. Promotion Policy

Config:

```rust
pub struct AuthorityPromotionPolicy {
    pub allow_automatic_lossless: bool,
    pub require_manual_for_data_loss: bool,
    pub require_external_fence: bool,
}
```

---

## 136. Synchronous Replication

If using synchronous PostgreSQL HA:

```text
lossless continuation
```

is easier.

But operational configuration must truly guarantee commits are durable on promoted standby.

Aequora should not assume.

---

## 137. Asynchronous Replication

Treat as:

```text
potential data loss
```

unless promotion tooling proves replica caught up to known primary commit boundary.

---

## 138. Commit Boundary

A primary can periodically persist externally visible checkpoint:

```text
latest durable journal sequence
```

Standby readiness compares.

---

## 139. Unknown Primary State

If primary is destroyed and last commit boundary unknown:

```text
assume ambiguity
```

Use new epoch.

---

## 140. Quorum Database

If underlying database already uses consensus/HA that guarantees single history:

```text
Aequora epoch may not change for ordinary leader election
```

because logical database timeline continues.

---

## 141. Managed Neon/Postgres

If managed provider failover preserves one PostgreSQL timeline and durable state:

```text
ordinary provider failover
```

need not change epoch.

Aequora should respond to actual continuity guarantees, not hostname change.

---

## 142. Database Restore

Even on managed platform:

```text
restore/PITR to earlier state
```

does require new epoch.

---

## 143. Authority Migration

Part 09 store-to-store migration can preserve epoch only if:

```text
journal
operation ledger
versions
scope state
audit
authority metadata
```

are all copied exactly and cutover is fenced.

---

## 144. Migration With Baseline Reset

If new system creates fresh baseline:

```text
new epoch
```

clients rebootstrap.

---

## 145. Fork Testing

Test two isolated authority copies.

Apply:

```text
O1 on A
O2 on B
```

with same epoch.

Checkpoint comparison must detect mismatch.

---

## 146. Promotion Test

Lossless replicated standby promotion.

Expected:

```text
same epoch
same ledger
same journal continuity
clients continue
```

---

## 147. PITR Test

Restore to earlier sequence.

Expected:

```text
epoch increments
old client cursor rejected
new snapshot required
```

---

## 148. Old Server Reappearance Test

Demoted old primary reconnects to load balancer.

Expected:

```text
write requests rejected due role/fence
```

---

## 149. Client Rollback Test

Client has seen epoch 7.

Connects to server claiming epoch 6.

Expected:

```text
AuthorityRollbackDetected
```

---

## 150. Ambiguous Operation Test

Operation sent before failover, response lost, standby lacks ledger.

Expected:

```text
classified PossiblyCommittedOldEpoch
not blindly re-executed if profile says ManualReview
```

---

## 151. Safe Replay Test

Preference operation ambiguous across epoch.

Expected:

```text
replayed safely
```

---

## 152. Finance Recovery Test

Payment ambiguous.

Expected:

```text
external provider reconciliation before any retry
```

---

## 153. Side-Effect Restore Test

Webhook/email intent exists before restore boundary mismatch.

Expected:

```text
recovery mode blocks unsafe blind resend
```

---

## 154. Correctness Invariants

Add:

### AEQ-INV-AUTH001

```text
At most one unfenced authority instance accepts writes for one AuthorityId and AuthorityEpoch.
```

### AEQ-INV-AUTH002

```text
A cursor from one AuthorityEpoch is never accepted as an incremental continuation cursor in a different AuthorityEpoch.
```

### AEQ-INV-AUTH003

```text
A restored or potentially divergent authoritative timeline receives a new AuthorityEpoch unless strict continuity is proven.
```

### AEQ-INV-AUTH004

```text
A client never silently accepts a lower AuthorityEpoch than the highest trusted epoch it has observed.
```

### AEQ-INV-AUTH005

```text
Fork detection never attempts automatic merge of two authoritative histories.
```

### AEQ-INV-AUTH006

```text
Potentially committed old-epoch operations are handled according to explicit epoch-recovery policy.
```

---

## 155. Additional Invariants

### AEQ-INV-AUTH007

```text
Old primary instances cannot resume authoritative commits after successful promotion fencing.
```

### AEQ-INV-AUTH008

```text
A new epoch is not opened for normal infrastructure failover when exact authoritative timeline continuity is proven.
```

### AEQ-INV-AUTH009

```text
Snapshot, integrity, audit, and governance artifacts bind to the authority epoch required for their interpretation.
```

---

## 156. Model Checking

Extend Part 01 model with:

```text
primary A
standby B
partition
promotion
old-primary recovery
authority epoch
fence token
```

Check:

```text
no dual accepted authoritative commit under valid fencing model
epoch mismatch forces recovery
```

---

## 157. Failure Simulation

Inject:

```text
network partition
replica lag
primary crash
PITR restore
promotion
old primary resume
```

---

## 158. Differential Timeline Test

Run:

```text
continuous authority
```

vs:

```text
lossless failover
```

Final canonical state/journal semantics should match.

---

## 159. Data-Loss Promotion Test

Explicitly verify system reports:

```text
new epoch
ambiguous operation set
rebootstrap requirement
```

rather than hiding loss.

---

## 160. Fork Check CLI

Potential output:

```text
Authority: prod-school
Epoch: 12
Checkpoint sequence: 940233
Local root: ...
Peer root: ...

STATUS: MATCH
```

or:

```text
STATUS: FORK DETECTED
WRITES MUST STOP
```

---

## 161. Metrics

```text
authority_epoch
authority_promotion_total
authority_demote_total
authority_fork_detected_total
authority_recovery_mode
authority_replication_lag
authority_ambiguous_operations_total
```

Avoid high-cardinality instance IDs in metrics.

---

## 162. Logs

Structured events:

```text
authority_promoted
authority_demoted
authority_epoch_incremented
authority_rollback_detected
authority_fork_detected
authority_recovery_started
authority_recovery_completed
```

---

## 163. Alerting

Critical alerts:

```text
fork detected
two primaries visible
epoch rollback attempt
promotion with unexpected lag
authority metadata mismatch
recovery verification failed
```

---

## 164. Admin Authorization

Separate permissions:

```text
Authority.View
Authority.Promote
Authority.ForcePromote
Authority.Demote
Authority.Restore
Authority.Verify
```

---

## 165. Separation of Duties

High-assurance environments may require:

```text
one operator proposes promotion
another approves
```

especially for data-loss promotion.

---

## 166. Transition Audit Record

Audit fields:

```text
old epoch
new epoch
promotion class
replication gap
fence confirmation
operator
reason
```

---

## 167. Signed Transition Manifest

Part 15 can sign this record.

Clients/admin tools can verify transition authenticity.

---

## 168. Control Plane

Long-term enterprise deployment may expose a small Aequora authority control plane.

Responsibilities:

```text
epoch registry
promotion orchestration
health/readiness
fencing integration
transition records
```

Do not mix with ordinary domain API routes.

---

## 169. Minimal Deployment

For small single-server deployment:

```text
AuthorityId fixed
AuthorityEpoch stored in DB/config
manual restore procedure increments epoch
```

No separate control plane required.

---

## 170. HA Deployment

For HA:

```text
external fencing
standby readiness
authority transition tooling
```

becomes mandatory.

---

## 171. Multi-Region Future

Part 17 will allow:

```text
multi-region reads
single writer
```

AuthorityEpoch remains global for writer timeline.

---

## 172. Multi-Primary Future

If Aequora ever supports multi-primary, it would require a fundamentally different authority model.

Do not stretch AuthorityEpoch semantics to fake multi-primary correctness.

---

## 173. Recommended Modules

```text
aequora-authority/
├── id.rs
├── epoch.rs
├── fence.rs
├── role.rs
├── transition.rs
├── checkpoint.rs
├── fork.rs
├── promotion.rs
├── recovery.rs
└── policy.rs
```

Server:

```text
aequora-server/
└── authority/
    ├── guard.rs
    ├── readiness.rs
    ├── restore.rs
    └── admin.rs
```

Client:

```text
aequora-client/
└── authority/
    ├── epoch.rs
    ├── rollback_guard.rs
    └── transition.rs
```

---

## 174. AuthorityGuard

Every authoritative write path passes:

```text
AuthorityGuard
```

which verifies runtime role/fence.

Axum route cannot bypass it.

---

## 175. Repository Guard

Prefer guard enforcement inside application/executor boundary, not only HTTP middleware.

This protects:

```text
background jobs
admin commands
internal service calls
```

too.

---

## 176. Startup Validation

Server startup verifies:

```text
AuthorityId configured
epoch valid
role known
fence acquired if Primary
external epoch not newer
DB metadata consistent
```

---

## 177. Refuse Unsafe Startup

If restored DB has stale epoch relative to external registry:

```text
do not serve writes
```

---

## 178. Recovery Command

Operator explicitly:

```text
aequora authority recover --new-epoch
```

after verification.

---

## 179. Plug-and-Play Behavior

Most applications never manipulate authority metadata directly.

During normal managed-DB failover with preserved timeline:

```text
nothing changes
```

During timeline-changing restore:

```text
Aequora detects/increments epoch
clients rebootstrap safely
```

---

## 180. Completion Criteria

Part 16 is complete when:

```text
[ ] AuthorityId defined
[ ] AuthorityEpoch defined
[ ] authority roles defined
[ ] single-writer fencing defined
[ ] promotion classes defined
[ ] cursor epoch binding defined
[ ] restore/PITR epoch rules defined
[ ] client epoch-transition state machine defined
[ ] ambiguous operation recovery policy defined
[ ] fork checkpoint/detection defined
[ ] old-primary rejoin policy defined
[ ] snapshot/audit/integrity epoch integration defined
[ ] external epoch rollback protection defined
[ ] promotion/readiness CLI defined
[ ] DR verification workflow defined
[ ] formal/fault tests defined
```

---

## 181. Final Architecture

```text
                     AUTHORITY CONTROL
                           │
                           ▼
                AuthorityId + Epoch E
                           │
                           ▼
                     PRIMARY WRITER
                           │
                  fence token / role
                           │
                           ▼
              Authoritative Journal
                           │
                           ▼
                 Client Cursors(E)

Normal lossless failover:

 Primary A ──replicated──► Standby B
    │                         │
    X                         ▼
                         Promote B
                         same epoch E
                         if continuity proven

Timeline-changing recovery:

 Old epoch E
    │
    ▼
PITR / stale promotion / fork resolution
    │
    ▼
New epoch E+1
    │
    ▼
old client cursors rejected
    │
    ▼
fresh snapshot/bootstrap
    │
    ▼
pending intent classified/recovered
```

The architectural principle is:

> **Aequora should preserve one authoritative history whenever continuity can be proven, and explicitly create a new authority epoch whenever it cannot.**

That makes failover, PITR, disaster promotion, database migration, and fork recovery visible to clients and operators instead of allowing stale cursors or duplicated authorities to silently corrupt synchronized state.
