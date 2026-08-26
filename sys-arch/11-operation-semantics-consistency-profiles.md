# Aequora Sync — Part 11

# Operation Semantics, Aggregate Policies, and Consistency Profiles Architecture

## 1. Purpose

Aequora already defines many important mechanisms:

```text
entity versions
conflict policies
outbox compaction
rebase rules
scope projection
server authority
retry semantics
tombstones
bootstrap
```

But if every operation independently configures all of these properties, the system becomes difficult to reason about.

For example, a developer might accidentally configure:

```text
compaction = ReplaceLatest
conflict = ServerWins
rebase = ReapplyIntent
delete = HardDelete
```

for an operation that actually represents an accounting posting.

That would be semantically wrong even if each individual setting is valid.

Aequora therefore needs **consistency profiles**: reusable semantic contracts that bundle together the correct behavior for a class of operations or aggregates.

The central rule is:

> **Consistency behavior should be selected as a coherent semantic profile, not assembled from unrelated switches unless the developer explicitly opts into an advanced custom profile.**

---

# 2. Goals

The profile system should provide:

```text
safe defaults
semantic clarity
compile/startup validation
reusable domain patterns
conflict consistency
compaction consistency
rebase consistency
delete consistency
snapshot consistency
audit compatibility
```

---

# 3. Non-Goals

Consistency profiles are not:

```text
database isolation levels
network QoS classes
authorization roles
UI modes
```

They define the semantics of synchronized operations and aggregates.

---

# 4. Profile Dimensions

Each profile defines a consistent policy across:

```text
identity
versioning
conflict detection
rebase
compaction
delete behavior
retry semantics
ordering requirements
audit expectations
snapshot representation
scope behavior
```

---

# 5. Core Profiles

Recommended initial profiles:

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

---

# 6. ImmutableAppendOnly

Use when records/events are never modified after creation.

Examples:

```text
ledger entry
chat message
audit event
attendance event
payment event
workflow history event
```

Semantics:

```text
create only
no in-place update
no rebase
no compaction by default
delete prohibited or represented by compensating event
idempotency required
```

---

# 7. ImmutableAppendOnly Versioning

Version may be:

```text
fixed 1
```

for immutable entity records.

If aggregate stream semantics exist:

```text
stream sequence
```

can act separately from entity version.

---

# 8. ImmutableAppendOnly Conflict

Typical conflicts:

```text
duplicate semantic event
duplicate external reference
invalid causal dependency
```

Not:

```text
field overwrite conflict
```

---

# 9. ImmutableAppendOnly Delete

Prefer:

```text
reversal
void event
redaction marker
```

rather than mutation of historical record.

---

# 10. Finance Recommendation

Financial postings should normally use:

```text
ImmutableAppendOnly
```

plus domain-level aggregate validation.

---

# 11. OptimisticVersioned

Use for ordinary mutable business entities.

Examples:

```text
student profile
guardian contact
school configuration
inventory item metadata
```

Semantics:

```text
EntityVersion increments
base_version required for mutation
stale update detected
rebase may be allowed
compaction may be allowed for unsent operations
```

---

# 12. OptimisticVersioned Default Conflict

Recommended:

```text
RejectStale
```

or custom field-aware merge.

Do not silently default to LastWriterWins.

---

# 13. OptimisticVersioned Delete

Use tombstone:

```text
version increments
entity becomes deleted
```

Stale updates cannot resurrect silently.

---

# 14. OptimisticVersioned Rebase

Possible:

```text
ReapplyIntent
FieldAware
Custom
```

depending on operation.

---

# 15. OptimisticVersioned Compaction

Possible for unsent operations:

```text
ReplaceLatest
Merge
```

if explicitly declared.

---

# 16. Commutative

Use when operation effects commute.

Examples:

```text
increment counter
add member to set
append independent reaction
record quantity delta
```

Condition:

```text
A then B
```

must produce same semantic result as:

```text
B then A
```

under declared domain semantics.

---

# 17. Commutative Benefits

Can reduce:

```text
conflicts
ordering requirements
rebase complexity
```

---

# 18. Commutative Idempotency

Commutative does **not** mean duplicates are harmless.

OperationId idempotency still required.

Example:

```text
Increment(1)
```

duplicated twice would incorrectly add 2.

---

# 19. Commutative Compaction

May permit:

```text
Increment(1)
Increment(2)
→
Increment(3)
```

only if audit/business semantics permit aggregation.

---

# 20. Commutative Delete

Usually domain-specific.

Do not inherit generic delete behavior blindly.

---

# 21. LastWriterWins

Use sparingly.

Suitable for low-value preference-like data.

Examples:

```text
UI theme
draft preference
noncritical personalization
```

Semantics:

```text
newest accepted write wins
```

---

# 22. LastWriterWins Clock

Do not base LWW on untrusted client wall clock.

Safer options:

```text
server receipt/order
server-issued sequence
HLC with server normalization
```

---

# 23. LastWriterWins Warning

Avoid for:

```text
finance
permissions
inventory
workflow state
legal records
```

because it can silently erase legitimate concurrent work.

---

# 24. ManualConflict

Use when conflicting versions require human choice.

Examples:

```text
important document metadata
complex schedule edit
sensitive master data
```

Semantics:

```text
stale divergent update
→
durable Conflict record
→
user/admin resolution operation
```

---

# 25. ManualConflict Compaction

Before send, safe local compaction may still happen.

After conflict is materialized:

```text
do not auto-merge
```

---

# 26. ManualConflict Rebase

Usually:

```text
CannotRebase
```

when overlapping semantic fields changed.

---

# 27. StrongAggregate

Use when multiple entities/records form one invariant boundary.

Examples:

```text
invoice + lines
purchase order + lines
accounting journal entry
inventory transfer
payroll batch
```

Semantics:

```text
aggregate root version
atomic authoritative transaction
aggregate-wide validation
conflict at aggregate boundary
```

---

# 28. StrongAggregate Versioning

Use:

```text
AggregateVersion
```

rather than independent child-row versions for concurrency decisions.

---

# 29. StrongAggregate Transaction

Server execution:

```text
lock/read aggregate
validate expected version
apply all invariant-related writes
increment aggregate version
append journal
operation ledger
commit
```

---

# 30. StrongAggregate Conflict

A conflicting child modification typically conflicts at root aggregate level unless domain provides a safe sub-merge.

---

# 31. StrongAggregate Snapshot

Snapshot should preserve:

```text
root
children
relationships
aggregate version
```

as one logical consistency unit.

---

# 32. ServerOnly

Use when clients may read replicated state but cannot authoritatively mutate it.

Examples:

```text
system-calculated fee schedule
server-generated analytics
admin-only configuration
```

Semantics:

```text
client write operations absent/rejected
server creates authoritative events
client only applies projection
```

---

# 33. ServerOnly Benefits

Prevents accidental offline mutation path for data whose semantics require central computation.

---

# 34. DeviceLocal

Use for data that must never synchronize.

Examples:

```text
window position
local cache
device preferences
temporary drafts explicitly local-only
```

Semantics:

```text
no outbox
no journal
no scope
no authoritative server version
```

---

# 35. DeviceLocal Must Be Explicit

Do not accidentally place synchronizable business data into DeviceLocal because it is convenient.

---

# 36. DerivedProjection

Use for data computed from authoritative source state.

Examples:

```text
dashboard totals
search index
materialized report
local denormalized view
```

Semantics:

```text
not independently mutated
rebuildable
source event/cursor tracked
```

---

# 37. DerivedProjection Recovery

If corrupt:

```text
rebuild from authoritative base/journal
```

not conflict with server.

---

# 38. Profile Descriptor

Define:

```rust
pub struct ConsistencyProfile {
    pub kind: ConsistencyProfileKind,
    pub versioning: VersioningPolicy,
    pub conflict: ConflictPolicy,
    pub rebase: RebasePolicy,
    pub compaction: CompactionPolicy,
    pub delete: DeletePolicy,
    pub ordering: OrderingPolicy,
    pub audit: AuditPolicy,
}
```

---

# 39. Prefer Enum + Validated Overrides

Built-ins:

```rust
ConsistencyProfileKind
```

Advanced users may override individual dimensions only through:

```text
CustomProfile
```

with validation.

---

# 40. CustomProfile

Conceptually:

```rust
pub struct CustomProfile {
    pub base: ConsistencyProfileKind,
    pub overrides: ProfileOverrides,
}
```

Builder must validate incompatible combinations.

---

# 41. Invalid Combination Example

Reject:

```text
ImmutableAppendOnly
+
ReplaceLatest compaction
```

unless operation explicitly represents merge-safe immutable aggregation.

---

# 42. Invalid Finance Example

Reject or strongly warn:

```text
StrongAggregate finance
+
LastWriterWins
```

---

# 43. Operation vs Aggregate Profile

Some policy belongs to operation.

Some belongs to aggregate.

Example:

```text
Student aggregate = OptimisticVersioned
UpdatePhone operation = ReplaceLatest unsent compaction
```

Therefore use:

```text
AggregateProfile
+
OperationSemanticProfile
```

---

# 44. AggregateProfile

Defines:

```text
version boundary
conflict boundary
transaction boundary
delete semantics
snapshot semantics
```

---

# 45. OperationSemanticProfile

Defines:

```text
idempotency
compaction
rebase
ordering
causal/dependency behavior
```

---

# 46. Profile Inheritance

Operation inherits aggregate defaults unless overridden safely.

---

# 47. Aggregate Registration

Example:

```rust
registry
    .aggregate::<Student>()
    .profile(OptimisticVersioned::default());
```

---

# 48. Operation Registration

Example:

```rust
registry
    .operation::<SetStudentPhone>()
    .aggregate::<Student>()
    .compaction(ReplaceLatest)
    .rebase(FieldAware);
```

---

# 49. Compile-Time Helpers

Provide typed marker profiles.

Example:

```rust
struct ImmutableAppendOnly;
struct OptimisticVersioned;
struct StrongAggregate;
```

Could enable generic constraints.

---

# 50. Typestate Opportunity

Certain APIs can require profile capabilities.

Example:

```rust
fn register_compactor<P: SupportsCompaction>(...)
```

This moves invalid combinations toward compile time.

---

# 51. Capability Traits

Potential internal traits:

```text
SupportsUpdate
SupportsDelete
SupportsRebase
SupportsCompaction
RequiresAggregateTransaction
```

---

# 52. Avoid Over-Generic Public API

Do not expose dozens of generic parameters to normal developers.

Use builders/macros to hide complexity.

---

# 53. Derive Macro

Potential:

```rust
#[derive(AequoraAggregate)]
#[aequora(profile = "OptimisticVersioned")]
struct Student { ... }
```

---

# 54. Operation Macro

```rust
#[derive(AequoraOperation)]
#[aequora(
    aggregate = "Student",
    semantics = "SetValue",
    compaction = "ReplaceLatest"
)]
struct SetStudentPhone { ... }
```

---

# 55. Built-In Operation Semantic Classes

Useful classes:

```text
SetValue
PatchFields
AppendEvent
Increment
SetMembership
Transition
Command
DerivedOnly
```

---

# 56. SetValue

Typical:

```text
latest unsent value wins
field-aware rebase possible
```

---

# 57. PatchFields

Tracks write set.

Supports:

```text
field-aware conflict
merge of disjoint updates
```

---

# 58. AppendEvent

Maps naturally to:

```text
ImmutableAppendOnly
```

---

# 59. Increment

Maps naturally to:

```text
Commutative
```

when domain guarantees.

---

# 60. SetMembership

Examples:

```text
add/remove tag
team member set
```

May use custom set compaction/conflict.

---

# 61. Transition

Examples:

```text
Draft -> Submitted
Submitted -> Approved
```

Requires strict state-machine validation.

Default:

```text
no rebase
no LWW
no compaction across transition
```

---

# 62. Command

Complex operation with arbitrary domain semantics.

Default conservative profile:

```text
Never compact
Never rebase
Reject stale where applicable
```

---

# 63. DerivedOnly

No client authoring.

Maps to:

```text
ServerOnly / DerivedProjection
```

---

# 64. Ordering Policies

Define:

```rust
pub enum OrderingPolicy {
    Independent,
    PerEntity,
    PerAggregate,
    DependencyOnly,
    GlobalWithinScope,
}
```

---

# 65. Independent

Operations can execute independently.

---

# 66. PerEntity

Preserve semantic order for one entity.

---

# 67. PerAggregate

Strong aggregate operations serialize logically by aggregate version.

---

# 68. DependencyOnly

Only explicit dependency DAG determines ordering.

---

# 69. GlobalWithinScope

Rare.

Use only when business semantics truly require one ordered stream.

Avoid as default due to scalability cost.

---

# 70. Delete Policies

```rust
pub enum DeletePolicy {
    Forbidden,
    Tombstone,
    CompensatingEvent,
    ScopeEvictionOnly,
    RebuildableDrop,
}
```

---

# 71. Forbidden

For immutable/legal records.

---

# 72. Tombstone

For mutable replicated entities.

---

# 73. CompensatingEvent

For append-only business history.

---

# 74. ScopeEvictionOnly

For data leaving a Part 07 scope but not deleted globally.

---

# 75. RebuildableDrop

For derived projections/caches.

---

# 76. Audit Policies

```rust
pub enum AuditPolicy {
    Required,
    Recommended,
    Optional,
    Derived,
}
```

---

# 77. Audit Required

Use for:

```text
finance
workflow approval
security-sensitive mutations
```

Compaction/rewrite restrictions tighten.

---

# 78. Versioning Policies

```rust
pub enum VersioningPolicy {
    None,
    EntityVersion,
    AggregateVersion,
    StreamSequence,
    ServerSequenceOnly,
}
```

---

# 79. Conflict Policies

Profiles should constrain allowed conflict policies.

Example:

```text
ImmutableAppendOnly:
    DuplicateSemantic
    UniqueConstraint
    DependencyFailure

OptimisticVersioned:
    RejectStale
    FieldMerge
    Manual

Commutative:
    CommutativeMerge

LastWriterWins:
    LWW

StrongAggregate:
    RejectStale
    CustomAggregateMerge
```

---

# 80. Profile Validation Matrix

Maintain a machine-readable matrix.

Example:

```text
Profile              Rebase       Compaction      LWW
-------------------------------------------------------
ImmutableAppendOnly  No           No              No
OptimisticVersioned  Maybe        Maybe           Optional
Commutative          Usually Yes  Maybe           No
ManualConflict       Limited      Limited         No
StrongAggregate      Usually No   Rare            No
ServerOnly           N/A          N/A             N/A
DeviceLocal          N/A          Local only      N/A
DerivedProjection    Rebuild      N/A             N/A
```

---

# 81. Profile Versioning

Built-in profile semantics must be versioned.

Example:

```rust
pub struct ProfileVersion(u16);
```

If behavior changes incompatibly, old operation manifests remain interpretable.

---

# 82. Protocol Manifest

Part 21 compatibility governance should include:

```text
operation kind
aggregate profile
operation semantic profile
profile version
```

---

# 83. Registry Diagnostics

At startup print:

```text
PostPayment
  aggregate: Ledger
  profile: ImmutableAppendOnly
  compaction: Never
  rebase: Never

SetStudentPhone
  aggregate: Student
  profile: OptimisticVersioned
  compaction: ReplaceLatest
  rebase: FieldAware
```

---

# 84. Safe Default for Unknown Operation

If developer registers operation without semantic profile:

```text
Command
+
conservative defaults
```

Better to reject startup for production if aggregate semantics are required.

---

# 85. Development Convenience

Development profile may infer basic defaults from operation class.

Production should make semantics explicit.

---

# 86. Finance Aggregate Example

```text
Ledger = StrongAggregate + ImmutableAppendOnly children
```

Posting operation:

```text
PostJournalEntry
```

Semantics:

```text
aggregate transaction
no rebase
no compaction
idempotent external reference
audit required
compensating reversal only
```

---

# 87. Student Profile Example

```text
Student = OptimisticVersioned
```

Operations:

```text
SetPhone
SetAddress
SetName
```

May support:

```text
field-aware rebase
ReplaceLatest unsent compaction
tombstone delete
```

---

# 88. Workflow Example

```text
LeaveRequest = StrongAggregate
```

Operations:

```text
Submit
Approve
Reject
Cancel
```

These are state transitions.

Semantics:

```text
no LWW
no cross-barrier compaction
strict expected state
audit required
```

---

# 89. Preference Example

```text
ThemePreference = LastWriterWins
```

Simple:

```text
SetTheme
```

Can compact aggressively before first send.

---

# 90. Tag Example

```text
TagSet = Commutative/SetMembership
```

Operations:

```text
AddTag
RemoveTag
```

May compact/cancel unsent pairs.

---

# 91. Derived Dashboard Example

```text
DashboardTotals = DerivedProjection
```

No client writes.

If stale/corrupt:

```text
rebuild
```

---

# 92. Server-Only Fee Schedule

```text
FeeSchedule = ServerOnly
```

Client may display offline projection but cannot enqueue mutation.

---

# 93. Device-Local Draft Example

```text
UnsavedEditorState = DeviceLocal
```

No Aequora sync metadata.

---

# 94. Profile and Scope Interaction

Part 07 scope eviction does not alter aggregate profile.

Example:

```text
Student = OptimisticVersioned
```

leaving teacher scope causes:

```text
EvictFromScope
```

not `Student` delete.

---

# 95. Profile and Anti-Entropy

Part 03 repair behavior differs:

```text
DerivedProjection:
    rebuild

OptimisticVersioned:
    replace authoritative base + rebase pending

ImmutableAppendOnly:
    restore missing immutable event/entity
```

---

# 96. Profile and Bootstrap

Part 10 snapshot installer may use aggregate profile to preserve atomic group boundaries.

---

# 97. Profile and Scheduler

Part 06 may assign default QoS based on operation semantic class.

Example:

```text
interactive command
bulk import event
background projection rebuild
```

But QoS remains separate from consistency semantics.

---

# 98. Profile and Live Sync

Part 08 hint behavior unaffected.

Live channel never weakens consistency profile.

---

# 99. Profile and Compaction

Part 04 should consume profile capability.

If profile says:

```text
NoCompaction
```

custom operation cannot accidentally enable compaction without validated override.

---

# 100. Profile and Rebase

Same for Part 04 rebase.

---

# 101. Profile and Causality

Part 02 lineage is universal.

All profiles keep:

```text
OperationId
CorrelationId
caused_by
```

where applicable.

---

# 102. Profile and Database Adapter

Profiles are database-independent.

Storage adapter only needs to provide required transactional capability.

---

# 103. Adapter Capability Validation

Example:

```text
StrongAggregate requires multi-record atomic transaction
```

If adapter lacks:

```text
startup fails
```

---

# 104. Capability Requirements

Each profile declares required adapter capabilities.

Example:

```rust
pub struct ProfileRequirements {
    pub multi_record_tx: bool,
    pub compare_and_swap: bool,
    pub durable_append: bool,
}
```

---

# 105. StrongAggregate Requirement

Requires:

```text
atomic transaction across aggregate writes
```

---

# 106. OptimisticVersioned Requirement

Requires:

```text
atomic version compare/update
```

or equivalent serializable mechanism.

---

# 107. ImmutableAppendOnly Requirement

Requires:

```text
unique OperationId
durable insert
unique domain references where needed
```

---

# 108. DerivedProjection Requirement

May tolerate weaker storage if rebuildable, but source authority remains strong.

---

# 109. Profile Compliance Tests

Every built-in profile should have reusable test suite.

Examples:

```text
OptimisticVersioned:
    stale update rejected
    version monotonic
    tombstone blocks resurrection

ImmutableAppendOnly:
    update prohibited
    duplicate op idempotent

Commutative:
    order permutations converge
```

---

# 110. Property Testing Commutativity

Generate operations A/B.

Verify:

```text
apply A then B
==
apply B then A
```

under declared preconditions.

---

# 111. Property Testing Compaction

For profile permitting compaction:

```text
original pending sequence
==
compacted sequence
```

semantically.

---

# 112. State-Machine Testing Workflow

For `Transition` class:

Generate invalid transitions.

Example:

```text
Approved -> Draft
```

must reject.

---

# 113. ManualConflict Tests

Concurrent conflicting edits:

```text
must produce conflict record
```

not silently resolve.

---

# 114. StrongAggregate Tests

Concurrent child mutations must preserve root invariant.

---

# 115. LWW Tests

Ensure server ordering policy, not client wall clock manipulation, determines winner.

---

# 116. Invariants

Add:

## AEQ-INV-PROF001

```text
Operation semantics cannot violate the declared aggregate consistency profile.
```

## AEQ-INV-PROF002

```text
Profiles requiring stronger adapter capabilities cannot start on an adapter lacking those guarantees.
```

## AEQ-INV-PROF003

```text
ImmutableAppendOnly records are never updated in place by normal synchronization.
```

## AEQ-INV-PROF004

```text
StrongAggregate mutations commit atomically at aggregate boundary.
```

## AEQ-INV-PROF005

```text
DerivedProjection state is never treated as independent authority.
```

## AEQ-INV-PROF006

```text
LastWriterWins is never implicitly selected for an unknown operation type.
```

---

# 117. Profile Manifest

Generate RON:

```ron
(
    aggregates: [
        (
            kind: "Student",
            profile: OptimisticVersioned,
            version: 1,
        ),
        (
            kind: "Ledger",
            profile: StrongAggregate,
            version: 1,
        ),
    ],
)
```

---

# 118. Compatibility CI

Part 21 should detect profile changes.

Example:

```text
OptimisticVersioned
→
LastWriterWins
```

is a semantic breaking change.

Require explicit migration/version change.

---

# 119. Profile Evolution

Changing profile of existing data may require:

```text
migration
new aggregate schema version
new operation kinds
new conflict rules
```

Do not silently switch behavior.

---

# 120. Profile Migration Example

Changing:

```text
mutable balance row
```

to:

```text
immutable ledger
```

is not a config change.

It is a domain migration.

---

# 121. Developer API

Example:

```rust
registry
    .aggregate::<Student>()
    .profile(ConsistencyProfiles::optimistic_versioned());

registry
    .operation::<SetStudentPhone>()
    .semantic(OperationSemantics::set_value())
    .compaction(ReplaceLatest)
    .rebase(FieldAware);
```

---

# 122. Simplified Macro

```rust
#[aequora::aggregate(
    profile = "OptimisticVersioned"
)]
struct Student { ... }
```

---

# 123. Finance Macro

Potential:

```rust
#[aequora::aggregate(
    profile = "StrongAggregate",
    audit = "Required"
)]
struct JournalEntry { ... }
```

---

# 124. Startup Validation

Validate:

```text
aggregate profile exists
operation semantic class compatible
compaction compatible
rebase compatible
delete compatible
adapter capabilities sufficient
```

---

# 125. Error Example

Good diagnostic:

```text
AEQ-PROFILE-004:
Operation PostPayment declares ReplaceLatest compaction,
but aggregate Ledger uses ImmutableAppendOnly.
Use CompactionPolicy::Never or define a certified custom profile.
```

---

# 126. Runtime Descriptor

Hot-path runtime should use compact IDs.

Example:

```text
AggregateProfileId(u16)
OperationSemanticId(u16)
```

Avoid string comparisons.

---

# 127. Documentation Generation

CLI:

```text
aequora profile list
aequora profile explain OptimisticVersioned
aequora registry explain PostPayment
```

---

# 128. Project Documentation

Generate a semantic matrix from registry so reviewers can inspect all operations.

---

# 129. Enterprise Review

Before production release, review especially:

```text
finance
permissions
workflow
inventory
billing
```

for correct profiles.

---

# 130. Profile Ownership

Domain team/module owns its aggregate profile choice.

Infrastructure team should not override business semantics merely for performance.

---

# 131. Performance Tradeoffs

Profiles may constrain optimization.

Example:

```text
StrongAggregate
```

can reduce parallelism.

That is acceptable because correctness comes first.

---

# 132. Avoid Profile Proliferation

Do not create 50 built-in profiles.

Start with a small orthogonal set.

Custom profiles handle exceptional cases.

---

# 133. Profile Composition

Potential composition:

```text
StrongAggregate
+
ImmutableChildren
```

Use validated composition only if it remains understandable.

---

# 134. Recommended Initial Built-Ins

Final recommended v1 set:

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

---

# 135. Completion Criteria

Part 11 is complete when:

```text
[ ] built-in consistency profiles defined
[ ] aggregate profile vs operation semantics separated
[ ] versioning policies defined
[ ] ordering policies defined
[ ] delete policies defined
[ ] audit policies defined
[ ] profile capability requirements defined
[ ] invalid combinations rejected
[ ] registry API defined
[ ] derive/builder ergonomics defined
[ ] profile manifest generated
[ ] compatibility CI hooks defined
[ ] profile-specific property/compliance tests defined
[ ] finance/workflow examples covered
```

---

# 136. Final Architecture

```text
                   DOMAIN AGGREGATE
                         │
                         ▼
                 Aggregate Profile
                         │
          ┌──────────────┼──────────────┐
          ▼              ▼              ▼
      Versioning      Conflict       Delete
          │              │              │
          └──────────────┼──────────────┘
                         ▼
                Operation Semantics
                         │
          ┌──────────────┼──────────────┐
          ▼              ▼              ▼
       Rebase        Compaction       Ordering
          │              │              │
          └──────────────┼──────────────┘
                         ▼
                Runtime Validation
                         │
                         ▼
                  Adapter Capability
                         │
                         ▼
                   Safe Execution
```

The architectural principle is:

> **Aequora should make developers declare what kind of truth an aggregate represents, then derive safe synchronization behavior from that declaration.**

This turns conflict handling, versioning, compaction, rebase, deletion, and transaction requirements from scattered configuration into one coherent semantic contract.
