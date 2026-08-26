# Aequora Sync — Part 04

# Offline Operation Compaction, Coalescing, Rebase, and Queue Optimization Architecture

## 1. Purpose

Aequora is local-first.

That means clients may remain offline for:

```text
minutes
hours
days
weeks
months
```

During that time, the client outbox can accumulate:

```text
hundreds
thousands
tens of thousands
hundreds of thousands
```

of pending operations.

A naïve sync engine would send every historical operation exactly as recorded.

That is correct, but potentially wasteful.

Example:

```text
UpdateStudentPhone(A)
UpdateStudentPhone(B)
UpdateStudentPhone(C)
UpdateStudentPhone(D)
```

If none of those operations has reached the server, sending all four may be unnecessary.

The final intended local state is:

```text
phone = D
```

For some operation classes, Aequora can safely compact:

```text
A → B → C → D
```

into:

```text
D
```

But this is only safe when semantic rules permit it.

The central rule is:

> **Compaction is a semantic transformation, not a storage cleanup trick.**

Aequora must never squash operations merely because they target the same entity.

---

# 2. Goals

The offline queue optimization system should:

```text
reduce upload bytes
reduce server validation load
reduce redundant conflicts
reduce local storage growth
preserve business intent
preserve dependency semantics
preserve idempotency guarantees
preserve audit requirements
remain deterministic
be verifiable by property tests
```

---

# 3. Non-Goals

Compaction must not:

```text
rewrite committed server history
hide business actions that require audit
merge financial ledger postings blindly
drop immutable events
guess conflict resolution
```

It applies only to **pending, not-yet-authoritative client operations**.

---

# 4. Operation Lifecycle Boundary

Compaction is allowed only while operation is in:

```text
Pending
WaitingRetry
```

Potentially:

```text
InFlight
```

must be excluded unless request delivery is known not to have happened.

Safest rule:

> **Once an operation may have reached the server, treat its OperationId and semantic existence as immutable.**

---

# 5. Compaction Eligibility

Each operation type should declare a compaction policy.

Conceptual:

```rust
pub enum CompactionPolicy {
    Never,
    ReplaceLatest,
    Merge,
    CancelPairs,
    Custom,
}
```

---

# 6. Default Policy

Unknown operation types default to:

```text
Never
```

This is critical.

Silent over-compaction can destroy business meaning.

---

# 7. Operation Descriptor Extension

Each registered operation may declare:

```rust
OperationDescriptor {
    ...
    compaction_policy: CompactionPolicy,
    rebase_policy: RebasePolicy,
}
```

---

# 8. ReplaceLatest Policy

Suitable for simple overwrite semantics.

Example:

```text
SetUserPreference(theme=dark)
SetUserPreference(theme=light)
```

Only latest value matters.

Compacted form:

```text
SetUserPreference(theme=light)
```

---

# 9. Merge Policy

Suitable when multiple pending updates can combine.

Example:

```text
UpdateProfile(name="A")
UpdateProfile(phone="X")
```

May compact into:

```text
UpdateProfile(name="A", phone="X")
```

if domain semantics permit.

---

# 10. CancelPairs Policy

Example:

```text
AddTag("sports")
RemoveTag("sports")
```

If both are still local-only and no dependent operation observes the intermediate state:

```text
both may cancel
```

---

# 11. Never Policy

Mandatory for operations such as:

```text
PostPayment
IssueRefund
CreateLedgerEntry
SendMessage
RecordAttendanceEvent
ApproveWorkflowStep
CreateAuditRecord
```

Each operation has business meaning even if later state appears to override it.

---

# 12. Finance Rule

Financial operations should default to:

```text
Never
```

Examples:

```text
PostPayment(100)
PostPayment(200)
```

must not become:

```text
PostPayment(300)
```

unless domain explicitly defines a mathematically and legally equivalent aggregation operation.

---

# 13. Audit-Sensitive Rule

If operation must appear in audit history:

```text
Never
```

unless audit is separately preserved before compaction.

Even then, default to preserving operation.

---

# 14. Compaction Key

Operations need a semantic compaction grouping key.

Conceptual:

```rust
pub struct CompactionKey {
    pub entity: EntityRef,
    pub field_group: Option<FieldGroupId>,
    pub operation_class: OperationClassId,
}
```

Do not use only:

```text
entity ID
```

because one entity may have unrelated operations.

---

# 15. Example

Student operations:

```text
UpdatePhone
UpdateAddress
ChangeClass
RecordFee
```

Only certain subsets may compact together.

---

# 16. Compaction Window

Only operations within a safe window should be considered.

Example:

```text
all pending operations after last acknowledged operation
```

Never compact across:

```text
server-confirmed boundary
manual conflict boundary
schema migration boundary
```

without explicit rules.

---

# 17. Dependency Preservation

Suppose:

```text
O1 = CreateStudent
O2 = UpdateStudentPhone
O3 = CreateInvoice(depends O1)
```

Compacting O1 and O2 into a single create-with-phone may be possible only if:

```text
O3 dependency is rewritten safely
```

This requires graph-aware compaction.

---

# 18. Dependency DAG Awareness

Compactor must operate on:

```text
operation DAG
```

not a flat queue.

Inputs:

```text
operations
dependencies
causation
correlation
compaction descriptors
```

---

# 19. Dependency Rewriting

If:

```text
O2 supersedes O1
```

and:

```text
O3 depends on O1
```

the compactor must decide:

```text
O3 dependency remains O1?
O3 now depends on O2?
compaction forbidden?
```

Default:

```text
forbid compaction unless rewrite semantics are explicitly defined
```

---

# 20. Supersession Model

Rather than physically deleting operations immediately, model:

```text
SupersededBy(OperationId)
```

This improves diagnostics.

---

# 21. Stable OperationId Semantics

If a new compacted operation replaces several pending operations, choose one of two models.

## Model A — Preserve Latest OperationId

Use latest operation's ID.

Earlier operations become:

```text
Superseded
```

## Model B — Create New Compacted OperationId

Generate a new operation and preserve lineage to originals.

Recommended default:

```text
Model A
```

for simple ReplaceLatest operations because final logical action already has identity.

Use Model B for custom merged operations.

---

# 22. Supersession Record

Persist:

```rust
pub struct Supersession {
    pub old_operation_id: OperationId,
    pub new_operation_id: Option<OperationId>,
    pub reason: SupersessionReason,
}
```

---

# 23. Why Keep Supersession Metadata

Useful for:

```text
debugging
UI pending count explanation
audit
test replay
dependency diagnostics
```

---

# 24. Physical Cleanup

After compaction is durable and dependencies are rewritten:

```text
superseded payloads
```

may later be garbage collected.

But logical supersession metadata can be retained longer.

---

# 25. Correlation Preservation

Compaction must preserve Part 02 lineage semantics.

If operations share one correlation:

```text
same correlation can remain
```

If operations belong to different user actions:

```text
do not merge blindly
```

---

# 26. Cross-Correlation Compaction

Default:

```text
forbidden
```

because separate user actions may have separate audit/intent.

Applications may explicitly permit it for preference-like data.

---

# 27. Causation Preservation

If compacted operation has multiple causal ancestors, choose:

```text
primary caused_by
+
supersession lineage
```

Do not fabricate a false simple causation chain.

---

# 28. Queue Compaction State Machine

```text
Idle
 ↓
ScanPending
 ↓
BuildCompactionGroups
 ↓
ValidateEligibility
 ↓
PlanRewrite
 ↓
PersistRewriteTransaction
 ↓
VerifyDependencies
 ↓
Done
```

---

# 29. Local ACID Requirement

Compaction metadata changes must be transactional.

Example:

```text
BEGIN

insert new merged operation
mark old operations superseded
rewrite dependencies
update queue indexes

COMMIT
```

Crash before commit:

```text
old queue remains valid
```

---

# 30. Compaction Transaction Boundary

Never:

```text
delete old operation
COMMIT
insert new operation
```

This can lose intent.

---

# 31. Rebase Purpose

Rebase is different from compaction.

Compaction asks:

```text
Can multiple pending operations become fewer equivalent operations?
```

Rebase asks:

```text
Can pending intent be re-expressed against a newer authoritative base?
```

---

# 32. Rebase Scenario

Client offline:

```text
server version = 10
client base version = 10
```

Client queues:

```text
UpdatePhone("A")
```

Meanwhile server reaches:

```text
version = 12
```

Client bootstraps or pulls latest state.

Pending operation now has stale base.

Rebase may transform it to:

```text
UpdatePhone("A") based on version 12
```

if semantics permit.

---

# 33. Rebase Policy

```rust
pub enum RebasePolicy {
    Never,
    ReapplyIntent,
    FieldAware,
    Custom,
}
```

---

# 34. Never Rebase

Use when operation semantics depend on exact prior state.

Examples:

```text
TransferMoney
ApproveVersion
ConditionalWorkflowTransition
```

These should go through normal server conflict handling.

---

# 35. ReapplyIntent

Suitable for:

```text
SetPhone("A")
SetTheme("dark")
```

The user intent is independent of intermediate server version.

---

# 36. FieldAware Rebase

Example:

```text
client changed phone
server changed address
```

The operation may be safely rebased if field-level conflict policy says independent.

---

# 37. Custom Rebase

Domain-specific.

Example:

```text
move task from position 4 to 7
```

requires list-aware semantics.

---

# 38. Rebase Does Not Mean Auto-Accept

Rebased operation still goes through:

```text
authorization
validation
conflict detection
execution
```

on server.

---

# 39. Rebase Identity

Question:

```text
Should rebased operation retain OperationId?
```

If semantic intent is unchanged:

```text
yes
```

Recommended.

Payload/base version may be updated locally before first server delivery.

Once operation may have reached server:

```text
do not mutate payload under same OperationId
```

---

# 40. Operation Immutability Boundary

Before first possible delivery:

```text
payload may be compacted/rebased under policy
```

After possible delivery:

```text
OperationId → payload mapping must be immutable
```

This is a crucial invariant.

---

# 41. Sent-Once Flag

Persist:

```text
ever_sent: bool
```

or equivalent delivery state.

Compaction/rebase requiring payload mutation only allowed when:

```text
ever_sent == false
```

---

# 42. Why

Suppose server has received O1 but client lost response.

If client rewrites O1 payload and retries same ID:

```text
same OperationId
different semantic operation
```

idempotency breaks catastrophically.

---

# 43. Immutable Envelope Hash

Persist:

```text
operation_payload_hash
```

Once sent.

Server operation ledger can optionally preserve hash too.

If same OperationId arrives with different hash:

```text
reject as IdempotencyViolation
```

---

# 44. Idempotency Payload Invariant

Add:

> **For any OperationId that may have reached authority, semantic payload is immutable.**

---

# 45. Queue Coalescing

Compaction can happen:

```text
eagerly
periodically
before sync
under storage pressure
```

---

# 46. Eager Compaction

When a new operation is enqueued:

```text
look at recent compatible pending group
```

and compact immediately if cheap.

Benefits:

```text
smaller queue from start
```

---

# 47. Periodic Compaction

Background task scans older queue.

Useful for:

```text
large offline periods
complex merge groups
```

---

# 48. Pre-Sync Compaction

Before building batch:

```text
run bounded compaction pass
```

Do not delay user sync excessively.

---

# 49. Storage-Pressure Compaction

If local storage grows:

```text
increase compaction aggressiveness
```

within semantic safety.

Never delete non-compactable pending intent.

---

# 50. Priority

Compaction should not block urgent sync.

Priority:

```text
user mutation
sync
reconciliation
small eager compaction
background deep compaction
```

---

# 51. Bounded Compaction Work

Config:

```text
max operations per compaction pass
max CPU time
max transaction size
```

Avoid scanning millions of operations in one UI-critical task.

---

# 52. Rayon Usage

Rayon may help with:

```text
group classification
pure merge preparation
dependency analysis
```

for very large queues.

Do not mutate DB transactions in Rayon threads.

---

# 53. Deterministic Compactor

Given same:

```text
operation sequence
policy version
schema
```

the compaction plan should be deterministic.

This is essential for testing.

---

# 54. Compaction Policy Version

Custom compaction logic should have:

```text
policy version
```

if persisted decisions need explanation across upgrades.

---

# 55. Schema Evolution

Do not compact operations across incompatible schema versions unless upcasting occurs first.

Pipeline:

```text
load pending
↓
upcast to current semantic form
↓
compact
```

or:

```text
leave old operations unchanged
```

---

# 56. Upcast Safety

Never rewrite already-sent operation payload under same OperationId.

Only unsent operations can be upcast in-place.

Sent operations must remain wire-compatible with server.

---

# 57. Create + Update Compaction

Example:

```text
CreateStudent(name=A)
UpdateStudentPhone(X)
```

Can become:

```text
CreateStudent(name=A, phone=X)
```

only if:

```text
both unsent
same correlation or allowed cross-correlation
no dependent semantics require original update
Create handler supports equivalent result
```

---

# 58. Create + Delete Cancellation

Example:

```text
CreateDraft
DeleteDraft
```

If both unsent and no dependent external effect:

```text
both may cancel completely
```

This can eliminate work.

---

# 59. Create + Delete Restrictions

Do not cancel if create operation also represents:

```text
billing
audit
notification
external reservation
```

or if any dependent operation exists.

---

# 60. Update + Delete

Example:

```text
UpdateName
DeleteEntity
```

Often final authoritative outcome only needs:

```text
DeleteEntity
```

but audit semantics may require update preservation.

Policy-specific.

---

# 61. Delete + Recreate

Never collapse casually.

This may represent:

```text
new logical entity
```

even if same business key is reused.

Stable EntityId semantics matter.

---

# 62. Append Operations

Examples:

```text
AddComment
AddMessage
RecordEvent
```

should usually never compact.

Append-only sequences preserve meaning.

---

# 63. Counter Operations

Example:

```text
Increment(1)
Increment(1)
Increment(1)
```

Could compact to:

```text
Increment(3)
```

if operation is mathematically commutative and audit allows.

This requires explicit policy.

---

# 64. Set Operations

Example:

```text
AddTag A
AddTag B
RemoveTag A
```

Could compact to:

```text
AddTag B
```

if no intermediate effect matters.

Use set-semantic custom compactor.

---

# 65. List/Reorder Operations

These are dangerous.

Example:

```text
MoveItem(A, index=2)
MoveItem(B, index=1)
```

Compaction requires list-state semantics.

Default:

```text
Never
```

unless custom algorithm exists.

---

# 66. Text Editing

Rich text/document editing may need:

```text
OT
CRDT
patch compaction
```

Do not treat as ordinary ReplaceLatest unless whole-document overwrite semantics are acceptable.

---

# 67. Dependency-Aware Grouping

Build graph components by:

```text
entity
compaction key
dependency relation
correlation
policy
```

Only merge within safe components.

---

# 68. Barrier Operations

Some operations should stop compaction across them.

Examples:

```text
Submit
Approve
ClosePeriod
Publish
Finalize
```

Descriptor:

```rust
compaction_barrier: bool
```

---

# 69. Example

```text
UpdateDraft A
UpdateDraft B
SubmitDraft
UpdateDraft C
```

Only:

```text
A + B
```

may compact before Submit.

C belongs after barrier and must remain separate.

---

# 70. Conflict Boundary

If an operation already entered:

```text
Conflict
```

do not compact it with later pending operations automatically.

Conflict is a semantic boundary requiring resolution.

---

# 71. Manual Resolution Boundary

Resolution operation starts a new logical phase.

Do not compact across it unless explicitly allowed.

---

# 72. Queue Segments

Outbox can be logically segmented by:

```text
unsent mutable segment
sent immutable segment
conflict segment
acknowledged history
```

Compactor only operates on mutable unsent segment.

---

# 73. Queue State Model

```rust
pub enum MutationMutability {
    MutableUnsent,
    ImmutablePossiblyDelivered,
    Finalized,
}
```

This is safer than deriving mutability from many state combinations.

---

# 74. Upload Batch Optimization

After compaction, batch builder still applies:

```text
max ops
max bytes
dependency order
priority
```

Compaction is upstream of batching.

---

# 75. Priority Operations

Operation descriptor may define:

```text
Urgent
Interactive
Normal
Bulk
Background
```

Compaction must preserve priority or choose the highest relevant priority.

---

# 76. Correlation-Aware Priority

One user action may contain several operations.

Batch them together where practical to reduce partial user-visible completion.

But transport batch does not imply transaction atomicity.

---

# 77. Queue Ordering

Do not assume strict FIFO for independent operations.

Use:

```text
dependency order
priority
fairness
```

while maintaining deterministic planning.

---

# 78. Independent Entity Parallelism

Outbox operations for unrelated entities may be uploaded concurrently in future.

Compaction should increase this opportunity by simplifying dependency graph.

---

# 79. Queue Indexes

Local store should index:

```text
state
ever_sent
compaction key
entity
created sequence
correlation
```

for efficient scans.

---

# 80. Local Operation Sequence

Assign monotonically increasing local queue sequence:

```rust
pub struct LocalOperationSeq(u64);
```

Useful for deterministic ordering and compaction.

---

# 81. Local Seq vs Server Sequence

Do not confuse:

```text
LocalOperationSeq
```

with authoritative:

```text
Sequence
```

They serve different roles.

---

# 82. Rebase After Pull

Client reconciliation may receive new authoritative state while pending operations remain.

Pipeline:

```text
apply authoritative base
↓
identify affected pending ops
↓
rebase eligible ops
↓
recompute optimistic view
```

---

# 83. Rebase Trigger

Run when:

```text
same entity authoritative version changes
bootstrap completes
anti-entropy repair modifies base
scope rebootstrap occurs
```

---

# 84. Rebase Failure

If operation cannot safely rebase:

```text
leave original pending
```

and let server conflict resolution decide.

Or mark:

```text
NeedsConflictEvaluation
```

locally for UI.

---

# 85. Rebase With Field Tracking

For field-aware policies, pending operation should expose:

```text
read set
write set
```

or semantic field group.

Example:

```text
writes: phone
```

Server update writes:

```text
address
```

Safe rebase.

---

# 86. Read/Write Set Descriptor

Optional:

```rust
OperationAccess {
    reads: FieldSet,
    writes: FieldSet,
}
```

Useful for:

```text
rebase
compaction
conflict optimization
```

---

# 87. Avoid False Precision

Not every operation maps neatly to fields.

Complex domain operations should use:

```text
Aggregate-wide
```

access semantics.

---

# 88. Recompute Optimistic State

After compaction/rebase:

```text
authoritative base
+
remaining pending ops
```

must reconstruct same effective local view as before, within declared equivalence.

---

# 89. Semantic Equivalence

Define compaction correctness as:

```text
Apply(original pending sequence, same base)
```

is semantically equivalent to:

```text
Apply(compacted sequence, same base)
```

under compactor preconditions.

---

# 90. Equivalence Is Domain-Specific

For simple set operations:

```text
final state equality
```

may be enough.

For audit-sensitive workflows:

```text
observable event equality
```

may be required.

Thus each policy needs explicit equivalence definition.

---

# 91. Compactor Trait

Conceptual:

```rust
pub trait OperationCompactor<O> {
    fn try_compact(
        &self,
        prior: &O,
        next: &O,
        ctx: &CompactionContext,
    ) -> Result<CompactionDecision<O>, CompactionError>;
}
```

---

# 92. Compaction Decision

```rust
pub enum CompactionDecision<O> {
    KeepBoth,
    ReplaceWith(O),
    KeepNext,
    KeepPrior,
    CancelBoth,
}
```

Custom compactor may need richer graph result.

---

# 93. Rebaser Trait

```rust
pub trait OperationRebaser<O> {
    fn rebase(
        &self,
        op: &O,
        old_base: &CanonicalState,
        new_base: &CanonicalState,
    ) -> Result<RebaseDecision<O>, RebaseError>;
}
```

---

# 94. Rebase Decision

```rust
pub enum RebaseDecision<O> {
    Unchanged,
    Rewritten(O),
    CannotRebase,
    Conflict,
}
```

---

# 95. No Network in Compactor

Compaction/rebase should operate on:

```text
local state
pending operations
known authoritative base
```

Do not make external calls during queue rewrite.

---

# 96. Deterministic Clock

Compaction must not depend on wall-clock randomness unless policy explicitly uses time.

If creating a new merged operation:

```text
preserve meaningful original timestamps
```

or define deterministic timestamp policy.

---

# 97. Timestamp Policy

Potential:

```text
created_at = earliest
modified_at = latest
```

depending on semantics.

Do not silently change business timestamps.

---

# 98. Causal Lineage for Merged Operation

If merging:

```text
O1
O2
O3
```

into:

```text
O4
```

record:

```text
supersedes = [O1, O2, O3]
```

bounded or stored in local supersession table.

Do not put huge arrays into wire envelope by default.

---

# 99. Wire Protocol

Server does not need to know every local superseded operation if they were never sent.

Only final compacted operation is transmitted.

Local diagnostics retain supersession history.

---

# 100. Privacy

Superseded operation payloads can contain sensitive data.

After safe compaction, old payloads may be securely removed according to retention policy.

---

# 101. Queue Storage Model

Logical tables:

```text
aequora_outbox
aequora_supersession
aequora_rebase_history
```

History tables may be optional/configurable.

---

# 102. Minimal Storage

Production low-storage mode may retain only:

```text
superseded operation IDs
new target ID
reason code
```

not full old payload.

---

# 103. Storage Pressure Levels

```text
Normal
Elevated
Critical
```

Actions:

```text
Normal:
    routine compaction

Elevated:
    aggressive safe compaction

Critical:
    suspend large downloads
    warn application
    never drop noncompactable intent
```

---

# 104. Outbox Hard Limit

Aequora should avoid arbitrary fixed operation-count limit that causes data loss.

Instead:

```text
bounded storage awareness
backpressure to application
read-only/degraded mode if disk exhausted
```

---

# 105. Low Disk Behavior

If outbox cannot be durably persisted:

```text
reject local synchronized mutation before domain commit
```

because ACID invariant requires domain mutation + outbox.

Do not commit domain state without sync intent.

---

# 106. Compaction Before Reject

When storage is low:

```text
attempt bounded safe compaction
```

before refusing new synchronized mutation.

---

# 107. User Experience

Expose status:

```text
Sync queue large
Optimizing offline changes
Storage critically low
Some changes require sync before more edits
```

Application decides wording.

---

# 108. Background Queue Maintenance

Coordinator may schedule:

```text
small compaction after mutation bursts
deep compaction during idle
```

---

# 109. Compaction Debounce

Do not compact after every keystroke if application generates rapid operations.

Better:

```text
short debounce
```

or application emits semantically meaningful operations less frequently.

---

# 110. UI Mutation Granularity

Aequora cannot compensate for badly modeled operations indefinitely.

Example:

```text
one operation per text keystroke
```

may create huge queues.

Application should prefer meaningful batching:

```text
UpdateDraftContent
```

periodically.

---

# 111. Operation Modeling Guidance

Good sync operation:

```text
meaningful domain intent
```

Bad sync operation:

```text
every internal UI event
```

Compaction should be optimization, not a substitute for good domain modeling.

---

# 112. Batch Edit Operation

For forms:

```text
UpdateStudentProfile {
    changed_fields
}
```

may be better than many tiny field operations if conflict semantics remain clear.

---

# 113. Server-Side Supersession

Server can also understand explicit supersession in future.

But local unsent compaction is simpler and safer.

Do not introduce server-side history rewriting.

---

# 114. Retry Queue

Operations waiting after transient error remain immutable if ever sent.

Compaction cannot rewrite them.

It may still reorder independent retries based on priority.

---

# 115. Poison Operation

A permanently malformed operation should become:

```text
Rejected
Quarantined
```

not block unrelated queue forever.

Dependency descendants are handled separately.

---

# 116. Dependency Failure Cleanup

If root operation permanently rejected:

```text
dependent pending operations
```

may become:

```text
DependencyFailed
```

and potentially removable after user/admin resolution.

This is not compaction.

---

# 117. Canceled User Action

User may intentionally undo pending local change before sync.

Represent as:

```text
new inverse operation
```

or local compaction cancellation if both operations are still unsent and policy allows.

---

# 118. Undo Semantics

If original operation may have reached server:

```text
undo must be a new operation
```

Never mutate/delete original.

---

# 119. Example — Preference

Queue:

```text
O1 SetTheme(Light)
O2 SetTheme(Dark)
O3 SetTheme(System)
```

All unsent.

Policy:

```text
ReplaceLatest
```

Result:

```text
O1 superseded
O2 superseded
O3 retained
```

---

# 120. Example — Payment

Queue:

```text
O1 PostPayment(100)
O2 PostPayment(200)
```

Policy:

```text
Never
```

Result:

```text
both retained
```

---

# 121. Example — Tag Set

Queue:

```text
O1 AddTag(A)
O2 AddTag(B)
O3 RemoveTag(A)
```

Custom set compactor:

```text
O1 + O3 cancel
O2 retained
```

if all unsent and no dependent operations.

---

# 122. Example — Create Then Update

```text
O1 CreateStudent(name=A)
O2 SetPhone(X)
```

If same action and safe:

```text
CreateStudent(name=A, phone=X)
```

Else:

```text
retain both
```

---

# 123. Example — Barrier

```text
O1 EditInvoiceDraft
O2 EditInvoiceDraft
O3 SubmitInvoice
O4 EditInvoiceDraft
```

Can compact:

```text
O1 + O2
```

Cannot cross:

```text
SubmitInvoice
```

---

# 124. Rebase After Server Change

Local:

```text
base version 4
SetPhone(A)
```

Server updates address to version 5.

Rebase policy:

```text
ReapplyIntent
```

Result:

```text
SetPhone(A), base version 5
```

if operation never sent.

---

# 125. Rebase Conflict

Local:

```text
SetPhone(A)
```

Server:

```text
SetPhone(B)
```

Field-aware policy:

```text
Conflict
```

Do not silently rebase.

---

# 126. Rebase After Bootstrap

Client preserves pending outbox.

Bootstrap installs latest authority.

Then:

```text
for each unsent pending operation:
    attempt rebase
```

Sent/possibly-delivered operations remain immutable and are retried as originally encoded.

---

# 127. Rebase After Anti-Entropy Repair

Same mechanism.

Part 03 repairs authoritative base.

Part 04 replays/rebases pending local intent.

---

# 128. Integration With Part 02 Lineage

Compaction:

```text
preserves root correlation
records supersession
```

Rebase:

```text
preserves OperationId
preserves correlation
preserves causation
```

if semantic intent unchanged.

---

# 129. Integration With Part 01 Correctness

Add model-checking scenarios:

```text
compaction before send
lost response then attempted compaction
rebase after bootstrap
dependency rewrite
create-delete cancellation
```

---

# 130. Formal Invariants

Add:

## AEQ-INV-OQ001

```text
Compaction never changes observable semantics under declared policy preconditions.
```

## AEQ-INV-OQ002

```text
An OperationId that may have reached server never changes semantic payload.
```

## AEQ-INV-OQ003

```text
Compaction never removes a dependency-required operation unless dependencies are safely rewritten.
```

## AEQ-INV-OQ004

```text
Pending user intent is never discarded solely to reduce queue size.
```

## AEQ-INV-OQ005

```text
Rebase preserves OperationId only when semantic intent is unchanged.
```

## AEQ-INV-OQ006

```text
Financial/audit-sensitive operations are noncompactable unless explicitly certified otherwise.
```

---

# 131. Property Tests

For each compactor:

```text
original sequence
vs
compacted sequence
```

Apply to same model state.

Assert semantic equivalence.

---

# 132. Shrinking

Proptest can discover minimal unsafe sequence.

Example:

```text
Update A
Barrier
Update B
```

if compactor accidentally crosses barrier.

---

# 133. Differential Tests

Run compaction result through:

```text
reference in-memory engine
real client adapter
server model
```

and compare outcome.

---

# 134. Crash Tests

Inject crash:

```text
after new operation inserted
before old operations superseded
before dependency rewrite
before commit
after commit
```

Local transaction guarantees old or new queue is valid.

Never half-rewritten.

---

# 135. Queue Verification

After compaction transaction:

```text
all dependency refs resolve
no cycles introduced
no duplicated active OperationId
no mutable operation has ever_sent=true
```

---

# 136. Debug CLI

Commands:

```text
aequora queue status
aequora queue explain <operation-id>
aequora queue compact --dry-run
aequora queue verify
```

---

# 137. Dry Run

Useful output:

```text
154 pending operations
82 eligible for compaction
proposed active operations: 91
saved bytes: estimated 41%
barriers: 6
noncompactable finance ops: 23
```

---

# 138. Production Auto-Compaction

Auto-compaction should not require operator involvement for safe built-in policies.

Custom compaction policies should pass compliance/property tests before production enablement.

---

# 139. Compaction Metrics

```text
outbox_operations_before
outbox_operations_after
compaction_operations_removed_total
compaction_bytes_saved_total
compaction_duration
rebase_success_total
rebase_conflict_total
```

Avoid OperationId metric labels.

---

# 140. Queue Age Metrics

Track:

```text
oldest pending operation age
oldest unsent mutable operation age
largest dependency component
```

---

# 141. Alerting

Alert if:

```text
outbox size grows continuously
compaction repeatedly fails
dependency graph corrupt
disk pressure critical
rebase conflict rate spikes
```

---

# 142. Adaptive Compaction Threshold

Config example:

```ron
outbox_optimization: (
    enabled: true,
    eager_threshold_ops: 32,
    background_threshold_ops: 512,
    deep_compaction_threshold_ops: 5000,
    max_pass_ops: 10000,
)
```

---

# 143. Per-Operation Policy Registration

Example:

```rust
registry
    .operation::<SetTheme>()
    .compaction(ReplaceLatest)
    .rebase(ReapplyIntent);

registry
    .operation::<PostPayment>()
    .compaction(Never)
    .rebase(Never);
```

---

# 144. Custom Compaction Certification

A custom policy should define:

```text
preconditions
semantic equivalence
dependency behavior
lineage behavior
barrier behavior
audit behavior
```

and provide tests.

---

# 145. Compaction Profile

Reusable profile examples:

```text
OverwriteField
SetMembership
CommutativeCounter
AppendOnly
StrongWorkflow
FinancialImmutable
```

Part 11 can formalize these broader consistency profiles.

---

# 146. Storage Adapter Requirements

Local adapter needs:

```text
transactional queue rewrite
query by compaction key
query dependencies
ever_sent persistence
supersession metadata
```

---

# 147. Server Adapter Impact

None for purely unsent local compaction.

Server only receives final active operations.

However server should enforce:

```text
same OperationId + different payload hash = rejection
```

as protection against client bugs.

---

# 148. Idempotency Hash

Server operation ledger may store:

```text
request semantic hash
```

On duplicate:

```text
same ID + same hash → normal dedup
same ID + different hash → protocol integrity violation
```

---

# 149. Hash Scope

Hash should include semantic fields:

```text
operation kind
schema
entity
base version
dependencies
payload
```

but not volatile transport metadata.

---

# 150. Security

A malicious client may intentionally reuse OperationId with different payload.

Server must reject.

Never trust client compaction implementation.

---

# 151. Queue Corruption

If compactor detects impossible state:

```text
dependency points to missing active/superseded operation
duplicate active ID
sent operation mutated
```

enter:

```text
QueueQuarantine
```

rather than continuing blindly.

---

# 152. Quarantine Recovery

Options:

```text
diagnostic repair
bootstrap
manual pending export
```

Do not delete queue automatically.

---

# 153. Pending Export

For severe client repair, allow export of pending operations:

```text
Postcard package
+
RON manifest
```

for forensic/recovery use.

---

# 154. Queue Import

Import must preserve:

```text
OperationId
payload hash
correlation
causation
dependencies
ever_sent
```

---

# 155. Cross-Database Client Migration

Part 04 queue format supports moving from:

```text
Stoolap
→ SQLite
```

without losing compaction/rebase metadata.

---

# 156. Queue Schema

Logical fields:

```text
operation_id
local_seq
state
ever_sent
operation_kind
schema_version
entity
base_version
dependencies
correlation_id
caused_by
payload
payload_hash
compaction_key
priority
created_at
```

---

# 157. Supersession Schema

```text
superseded_operation_id
replacement_operation_id
reason
created_at
```

---

# 158. Rebase History Schema

Optional:

```text
operation_id
old_base_version
new_base_version
policy
timestamp
```

May be omitted in low-storage profile.

---

# 159. Memory Architecture

Never load whole huge outbox.

Use paged scans and group windows.

---

# 160. Streaming Compaction

Process:

```text
read ordered chunk
carry open compaction groups
emit rewrite plan
advance
```

for very large queues.

---

# 161. Group Memory Bound

Bound number of open groups.

If exceeded:

```text
flush conservative groups
```

rather than unbounded RAM use.

---

# 162. Compaction Complexity

Target:

```text
O(N + E)
```

for queue plus dependency edges where possible.

Avoid pairwise O(N²) scanning.

---

# 163. Index-Assisted Grouping

Use:

```text
compaction_key
local_seq
state
```

indexes.

---

# 164. Completion Criteria

Part 04 is complete when:

```text
[ ] CompactionPolicy defined
[ ] RebasePolicy defined
[ ] mutable-unsent boundary defined
[ ] ever_sent semantics defined
[ ] payload immutability invariant defined
[ ] dependency-aware compaction planner defined
[ ] supersession model defined
[ ] barrier semantics defined
[ ] create/update/delete cases defined
[ ] finance-safe defaults defined
[ ] rebase-after-bootstrap defined
[ ] anti-entropy repair integration defined
[ ] property tests specified
[ ] crash/failpoint tests specified
[ ] queue metrics/diagnostics defined
[ ] adapter requirements defined
```

---

# 165. Final Architecture

```text
                 LOCAL OUTBOX

        Pending Unsent Operations
                  │
                  ▼
         Compaction Classifier
                  │
        ┌─────────┼──────────┐
        ▼         ▼          ▼
 ReplaceLatest   Merge      Never
        │         │          │
        └────┬────┴──────────┘
             ▼
      Dependency-Aware Planner
             │
             ▼
       Transactional Rewrite
             │
             ▼
        Optimized Outbox
             │
             ▼
          Batch Builder
             │
             ▼
            Sync

After authoritative state changes:

       New Server Base
             │
             ▼
       Rebase Analyzer
             │
      ┌──────┼────────┐
      ▼      ▼        ▼
   Reapply  Rewrite  Conflict
      │      │
      └──┬───┘
         ▼
   Pending Intent Preserved
```

The architectural principle is:

> **Aequora may optimize unsent history, but it must never optimize away meaning.**

Compaction is allowed only when semantic equivalence is explicit, testable, dependency-safe, lineage-aware, and compatible with idempotency.

That gives Aequora the efficiency of a smart offline queue without sacrificing the correctness of an immutable distributed operation protocol.
