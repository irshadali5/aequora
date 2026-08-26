# Aequora Sync — Part 07

# Subscription, Scope, Filter, and Dynamic Dataset Architecture

## 1. Purpose

Aequora is not always intended to synchronize an entire tenant database to every client.

Real applications need subsets.

Examples:

```text
teacher device:
    assigned classes
    current academic year
    attendance module

accountant desktop:
    finance module
    selected campuses

student app:
    own profile
    own timetable
    own fee records

branch office:
    one branch
    one operational region
```

Therefore Aequora needs a formal architecture for:

```text
subscriptions
sync scopes
filters
dynamic dataset membership
permission-driven expansion
permission-driven contraction
revocation
scope versioning
scope-specific cursors
filtered tombstones
multi-scope clients
```

The central rule is:

> **A cursor is only meaningful within the exact scope definition and authority generation for which it was issued.**

A scope change is a synchronization state transition, not merely a different SQL WHERE clause.

---

## 2. Goals

The scope system should provide:

```text
least-data synchronization
tenant isolation
module-based sync
branch/campus sync
user-specific datasets
dynamic membership
permission revocation
safe dataset contraction
safe expansion
scope-specific bootstrap
bounded journal reads
multi-scope clients
clear cursor semantics
```

---

## 3. Non-Goals

The scope system must not become:

```text
an arbitrary client query engine
SQL-over-sync
a mechanism for clients to bypass authorization
a general federated database query planner
```

Clients request declared scope types/parameters.

Server decides actual authorized dataset.

---

## 4. Terminology

Use distinct concepts:

```text
ScopeDefinition
ScopeInstance
Subscription
ScopeVersion
ScopeGeneration
ScopeCursor
Membership
Filter
```

---

## 5. ScopeDefinition

A reusable application-declared type.

Examples:

```text
TeacherWorkspace
FinanceWorkspace
StudentSelf
CampusOperations
```

Conceptually:

```rust
pub struct ScopeDefinitionId(u32);
```

This is stable across releases.

---

## 6. ScopeInstance

A concrete authorized dataset for one client/device/user.

Example:

```text
TeacherWorkspace {
    teacher_id = T1,
    campus = C3,
    academic_year = 2026
}
```

It receives:

```rust
pub struct ScopeId(Uuid);
```

or a deterministic opaque identifier.

---

## 7. Scope Is Server-Issued

Client may request:

```text
"I want TeacherWorkspace for academic year 2026"
```

Server resolves:

```text
what this authenticated principal is actually permitted to receive
```

The client must never define authoritative filter predicates itself.

---

## 8. ScopeResolver

Application implements:

```rust
pub trait ScopeResolver {
    async fn resolve(
        &self,
        auth: &AuthContext,
        request: &ScopeRequest,
    ) -> Result<ResolvedScope, ScopeError>;
}
```

Aequora then handles cursor/bootstrap mechanics.

---

## 9. ResolvedScope

Conceptually:

```rust
pub struct ResolvedScope {
    pub scope_id: ScopeId,
    pub definition: ScopeDefinitionId,
    pub version: ScopeVersion,
    pub generation: ScopeGeneration,
    pub descriptor: ScopeDescriptor,
}
```

---

## 10. ScopeDescriptor

Contains server-owned canonical parameters.

Examples:

```text
tenant
campus set
class set
module set
date range
entity families
```

Do not necessarily expose internal raw predicates to the client.

---

## 11. ScopeVersion

Define:

```rust
pub struct ScopeVersion(u64);
```

Increment when the logical scope membership rules change for the same scope instance.

Examples:

```text
teacher assigned a new class
campus removed
module permission added
```

---

## 12. ScopeGeneration

Define:

```rust
pub struct ScopeGeneration(u64);
```

Increment on incompatible timeline/reset events.

Examples:

```text
PITR
scope history discarded
filter semantics changed incompatibly
server cannot continue old cursor lineage
```

This is stronger than normal `ScopeVersion`.

---

## 13. ScopeCursor

Use:

```rust
pub struct ScopeCursor {
    pub scope_id: ScopeId,
    pub scope_version: ScopeVersion,
    pub generation: ScopeGeneration,
    pub sequence: Sequence,
}
```

A bare global sequence is insufficient.

---

## 14. Cursor Validation

Server validates:

```text
scope ID matches
scope version compatible
generation matches
cursor within retained history
```

Possible outcomes:

```text
Valid
ScopeChanged
ResyncRequired
UpgradeRequired
Forbidden
```

---

## 15. Scope Membership

A record/event is in scope when application-defined membership rules say so.

Membership may depend on:

```text
tenant
entity fields
relationships
permissions
module
time range
assignment
```

---

## 16. Membership Must Be Deterministic

For a given authoritative state and scope version:

```text
membership(entity, scope)
```

must be deterministic.

Otherwise reconciliation becomes impossible.

---

## 17. Membership vs Authorization

Membership says:

```text
this entity belongs in this dataset
```

Authorization says:

```text
this principal may receive/use it
```

Server scope resolver combines both.

---

## 18. Scope Request Is Not Authorization

Client can ask for:

```text
Campus C5
```

Server may resolve:

```text
Campus C2 only
```

or reject.

Never trust requested IDs.

---

## 19. Subscription

A subscription is the client's durable relationship to one scope.

Conceptually:

```rust
pub struct Subscription {
    pub subscription_id: SubscriptionId,
    pub scope_id: ScopeId,
    pub state: SubscriptionState,
}
```

---

## 20. Subscription States

```rust
pub enum SubscriptionState {
    Bootstrapping,
    Active,
    Expanding,
    Contracting,
    Suspended,
    Revoked,
    ResyncRequired,
}
```

---

## 21. Client Can Have Multiple Subscriptions

Example:

```text
CoreSchool
Finance
Documents
Notifications
```

Each may have its own:

```text
ScopeId
cursor
generation
bootstrap state
```

---

## 22. Do Not Force One Global Cursor

A single client-wide cursor breaks when datasets have different:

```text
permissions
retention
journal history
bootstrap lifecycle
```

Use scope-specific cursors.

---

## 23. Independent Failure

If:

```text
Documents scope requires resync
```

the client should not necessarily wipe:

```text
CoreSchool scope
```

Each subscription is independently recoverable where architecture allows.

---

## 24. Scope Registry

Server application registers:

```text
ScopeDefinition
resolver
membership evaluator
snapshot projector
journal filter
```

---

## 25. ScopeDefinition Trait

Conceptually:

```rust
pub trait ScopeDefinition {
    fn id(&self) -> ScopeDefinitionId;

    async fn resolve(
        &self,
        auth: &AuthContext,
        request: &ScopeRequest,
    ) -> Result<ResolvedScope, ScopeError>;

    fn event_membership(
        &self,
        scope: &ResolvedScope,
        event: &AuthoritativeEvent,
    ) -> MembershipDecision;
}
```

---

## 26. MembershipDecision

```rust
pub enum MembershipDecision {
    Include,
    Exclude,
    RequiresProjection,
}
```

---

## 27. Projection

Sometimes client should not receive the entire authoritative entity.

Example:

```text
server Student contains sensitive internal notes
student app may receive only public/self fields
```

Scope can select a projection.

---

## 28. Scope Projection

Server emits:

```text
ScopeProjectedChange
```

not necessarily raw authoritative event payload.

The journal remains authoritative internally.

The client receives an authorized projection.

---

## 29. Projection Version

Projection schema should be versioned separately if needed.

```rust
pub struct ProjectionVersion(u32);
```

This protects compatibility.

---

## 30. Filtered Journal

There are two broad strategies.

### Strategy A — Filter Global Journal at Read Time

```text
global journal
↓
scope membership filter
↓
client
```

### Strategy B — Maintain Scope-Specific Feed

```text
authoritative event
↓
scope feed index
↓
client
```

---

## 31. Recommended Initial Strategy

Start with:

```text
global authoritative journal
+
indexed scope filtering
```

for moderate scale.

Add precomputed scope feeds only when profiling shows need.

---

## 32. Cursor Semantics With Filtered Events

Suppose global journal:

```text
100 entity A in scope
101 entity X out of scope
102 entity B in scope
```

Client can safely advance scope cursor to:

```text
102
```

even though it only received events 100 and 102, because server confirms that 101 was intentionally filtered.

---

## 33. Scope Cursor as Watermark

`sequence = 102` means:

> All authoritative journal entries through 102 have been evaluated against this scope version, and all required resulting changes have been delivered/applied.

This is stronger than:

```text
last event received = 102
```

---

## 34. Filter Stability

If scope membership rules change, old filtered watermark may no longer be sufficient.

That is why `ScopeVersion` matters.

---

## 35. Scope Expansion

Example:

```text
teacher assigned Class B
```

Old scope:

```text
Class A
```

New scope:

```text
Class A + B
```

The client needs historical state for B that was previously filtered.

Normal incremental pull from current sequence cannot reconstruct it.

---

## 36. Expansion Strategies

Use one of:

```text
delta bootstrap for added membership
partial snapshot
full scope bootstrap
```

Recommended initial safe strategy:

```text
partial bootstrap for added partition if supported
otherwise full scope bootstrap
```

---

## 37. Scope Expansion State Machine

```text
Active v5
↓ server detects membership expansion
Expanding v6
↓ bootstrap newly added dataset
Install
↓ reconcile journal changes after bootstrap boundary
Active v6
```

---

## 38. Expansion Boundary

Server issues:

```text
new scope version
bootstrap boundary N
```

The added dataset snapshot represents state through N.

Then incremental events:

```text
> N
```

are applied.

---

## 39. Existing Dataset During Expansion

Existing scope data can remain usable.

If architecture supports partitioned bootstrap:

```text
old membership remains active
new partition staged
```

This improves UX.

---

## 40. Scope Contraction

Example:

```text
teacher loses Class A assignment
```

Client must stop receiving and generally remove data no longer authorized.

This is a security-sensitive transition.

---

## 41. Contraction Is Not Ordinary Delete

An entity can leave scope without being deleted globally.

Example:

```text
Student still exists
teacher no longer assigned
```

The client needs:

```text
ScopeRemoval
```

not an authoritative domain tombstone.

---

## 42. ScopeRemoval

Define a distinct change:

```rust
pub struct ScopeRemoval {
    pub scope_id: ScopeId,
    pub entity: EntityRef,
    pub reason: ScopeRemovalReason,
}
```

---

## 43. ScopeRemoval vs Tombstone

```text
Tombstone:
    entity deleted from authoritative domain

ScopeRemoval:
    entity still exists but no longer belongs in this client's dataset
```

Never confuse them.

---

## 44. Client Apply

On `ScopeRemoval`:

```text
remove authoritative replica data from that scope
```

unless another active scope still references the same shared local entity.

---

## 45. Multi-Scope Reference Problem

Example:

```text
Student S1 present in TeacherScope
and FinanceScope
```

TeacherScope contracts.

Do not physically delete S1 if FinanceScope still needs it.

---

## 46. Scope Reference Tracking

Client needs local membership metadata:

```text
EntityRef
↔
ScopeId set
```

Physical replica state can be removed only when:

```text
no active scope references entity
```

and no local-only requirement needs it.

---

## 47. Membership Table

Logical:

```text
aequora_scope_membership
```

Fields:

```text
scope_id
entity_type
entity_id
membership_version
state
```

---

## 48. Shared Entity Storage

Recommended:

```text
store entity once
+
track membership in multiple scopes
```

rather than duplicate full row per scope, unless adapter/project intentionally isolates stores.

---

## 49. Scope-Specific Projection Conflict

Two scopes may expose different projections of same server entity.

Example:

```text
student app projection
admin projection
```

If one local store contains both, use:

```text
projection namespaces
```

or select a canonical superset permitted to local principal.

Do not merge unauthorized fields accidentally.

---

## 50. Projection Security Rule

A more privileged scope must never leak fields into a less privileged account/session sharing the same local store unless the application intentionally permits it.

Multi-user shared-device designs need stronger profile isolation.

---

## 51. Revocation

Revocation is stronger than ordinary contraction.

Examples:

```text
device revoked
user removed from tenant
subscription permission revoked
```

Client may receive:

```text
ScopeRevoked
```

---

## 52. Revocation Behavior

On scope revocation:

```text
stop push for that scope
stop pull
remove unauthorized replicated data
quarantine/handle pending operations
invalidate cursor
```

---

## 53. Pending Operations During Revocation

A client may have pending edits for now-revoked data.

Do not silently upload them.

Mark:

```text
AuthorizationLost
```

or:

```text
ScopeRevokedPending
```

and stop transmission.

---

## 54. Preserve Pending Payload?

Policy depends on security and UX.

Options:

```text
quarantine encrypted/local
discard after explicit policy
export for admin support
```

Do not retain indefinitely if policy requires data erasure.

---

## 55. Permission Downgrade

If user still has some access:

```text
scope contracts
```

rather than full revocation.

---

## 56. Permission Upgrade

If user gains access:

```text
scope expands
```

with partial/full bootstrap.

---

## 57. Dynamic Relationship Membership

Membership can depend on relationships.

Example:

```text
teacher sees students in assigned classes
```

When assignment changes, many entities may enter/leave scope.

Need efficient membership diff.

---

## 58. Membership Diff

Server computes:

```text
old authorized membership
vs
new authorized membership
```

for dynamic transition.

For large datasets, avoid storing massive explicit sets if a deterministic query can generate them efficiently.

---

## 59. Scope Version Transition Record

Server may persist:

```text
scope_id
old_version
new_version
change_kind
boundary
```

for diagnostics and resync behavior.

---

## 60. Scope Identity Stability

If the same logical subscription changes incrementally:

```text
ScopeId stays same
ScopeVersion increments
```

If semantics are fundamentally replaced:

```text
new ScopeId
```

may be cleaner.

---

## 61. Filter Identity

Do not hash raw SQL.

Canonical filter identity should derive from:

```text
scope definition
normalized parameters
server policy version
projection version
```

---

## 62. Opaque Scope Token

Client can carry:

```text
opaque scope token
```

issued by server.

It should not need to understand internal filter details.

---

## 63. Signed Scope Token

Optional future optimization:

```text
signed serialized scope descriptor
```

can reduce DB lookup.

But authorization freshness still matters.

Do not let a long-lived token override current revocation state.

---

## 64. Scope Refresh

Client periodically or on sync obtains:

```text
scope status
```

Server may report:

```text
unchanged
expanded
contracted
revoked
generation changed
```

---

## 65. Efficient Change Detection

Server can return:

```text
ScopeVersion
```

in every sync response.

If client version differs:

```text
perform transition protocol
```

---

## 66. Scope Change During Sync

Suppose permission changes while request is running.

Server transaction/response must use a coherent authorization decision.

Safe approach:

```text
resolve scope version at request start
validate again before sensitive commit where needed
return newer scope version hint if changed
```

---

## 67. Push Operations and Scope

Outgoing operation must be authorized independently.

Being subscribed to an entity does not automatically grant write permission.

---

## 68. Write Scope

Read scope and write authority are distinct.

Example:

```text
student can read fee record
cannot edit it
```

Operation handler authorization remains authoritative.

---

## 69. Scope Removal of Locally Dirty Entity

If an entity leaves scope while unsent local operations exist:

```text
do not simply delete entity and pending intent
```

Transition to explicit state:

```text
ScopeRemovedWithPendingIntent
```

Policy decides whether operation can still be submitted.

---

## 70. Default Security Rule

If write authorization is no longer valid:

```text
do not transmit
```

Pending operation becomes authorization conflict/quarantine.

---

## 71. Scope Expansion and Pending Local Creation

A locally created entity may not yet be server-authoritative.

Membership logic for pending local entities is application-specific.

Outbox remains source of unsynced intent.

Scope bootstrap must not accidentally overwrite local pending creation.

---

## 72. Bootstrap + Pending Operations

Reuse Part 04:

```text
preserve outbox
install authoritative scope state
rebase eligible pending ops
```

---

## 73. Scope Transition + Anti-Entropy

Part 03 anti-entropy must compare within:

```text
exact ScopeId
ScopeVersion
ScopeGeneration
```

Never compare root hashes across different scope versions.

---

## 74. Scope-Specific Integrity Root

Integrity manifest:

```text
scope_id
scope_version
generation
boundary
root
```

---

## 75. Scope Transition + Part 05

Only local coordinator leader performs:

```text
bootstrap
scope contraction
membership rewrite
```

Follower processes observe local generation/status changes.

---

## 76. Scope Transition + Part 06

QoS scheduler prioritizes:

```text
security-sensitive contraction/revocation
```

above routine background work.

Large expansion bootstrap may be:

```text
interactive if required for UI
bulk otherwise
```

---

## 77. Scope Change Notifications

Future push hint can notify:

```text
scope version changed
```

but client still verifies with server.

---

## 78. Time-Bounded Scopes

Example:

```text
current academic year only
```

When year changes:

```text
scope parameters/version change
```

Old data retention is application policy.

---

## 79. Historical Offline Access

A client may intentionally retain historical data after it leaves active sync scope.

This must be explicit.

Distinguish:

```text
NotActivelySynced
```

from:

```text
NoLongerAuthorized
```

---

## 80. Retention Policy

Scope contraction can specify:

```rust
pub enum LocalRetentionPolicy {
    RemoveImmediately,
    RetainReadOnly,
    RetainUntil(Date),
    ApplicationManaged,
}
```

Security-sensitive revocation should normally force removal.

---

## 81. Retained Read-Only Historical Data

If allowed:

```text
remove from active membership
keep local archival copy
mark non-authoritative/stale
```

Do not continue presenting it as current synced state.

---

## 82. Scope Tombstone

Do not overload domain tombstone.

Use:

```text
MembershipRemoved
```

or `ScopeRemoval`.

---

## 83. Entity Re-Enters Scope

If an entity previously left and later returns:

```text
server sends current authoritative representation
```

Do not rely on stale retained copy.

---

## 84. Re-Entry Version

If client retained version 5 and server is version 11:

```text
replace/update to 11
```

normal authoritative rules apply.

---

## 85. Scope-Local Sequence

The simplest implementation can still use global server sequence as watermark within scope cursor.

Alternative future:

```text
scope-specific sequence
```

for large-scale optimized feeds.

---

## 86. Global Sequence Pros

```text
simple authoritative ordering
one journal
easy debugging
```

---

## 87. Global Sequence Cons

```text
many filtered entries
large sparse scans
high-cost membership evaluation
```

---

## 88. Scope Feed Pros

```text
efficient tenant/module-specific reads
dense feed
```

---

## 89. Scope Feed Cons

```text
more storage
more transaction/index complexity
```

---

## 90. Evolution Strategy

Start:

```text
global sequence + filter
```

Later:

```text
precomputed scope feed/index
```

without changing client semantics.

---

## 91. Scope Feed Index

Derived table may store:

```text
scope_partition
global_sequence
entity
change type
```

It must remain recoverable from authoritative journal.

---

## 92. Dynamic Scope Explosion

Do not materialize a separate feed for every user if millions of unique dynamic scopes exist.

Use shared partitions where possible.

Examples:

```text
campus
class
tenant
module
```

compose into user scope.

---

## 93. Scope Partitioning

Scope can reference deterministic partitions.

Example:

```text
Tenant T
Campus C
Class K
Module Attendance
```

This supports reuse.

---

## 94. PartitionId

Define:

```rust
pub struct DatasetPartitionId(UuidOrStableKey);
```

A scope may be:

```text
set of partitions + projection policy
```

---

## 95. Partition Advantages

Makes:

```text
expansion
contraction
bootstrap
Merkle integrity
retention
```

more efficient.

---

## 96. Avoid Arbitrary Per-Row Filter Logic

For high-scale deployments, favor scope predicates that map to indexed partition keys.

Application should design sync boundaries intentionally.

---

## 97. Scope Design Guidance

Good scope dimensions:

```text
tenant
campus
branch
class
module
user-owned
time bucket
```

Bad:

```text
arbitrary client-provided expression
```

---

## 98. Multi-Tenant Safety

Every scope belongs to exactly one trusted tenant context unless explicitly cross-tenant administrative.

Cross-tenant scope requires dedicated privileged design.

---

## 99. Scope IDs Are Not Secrets

Knowing `ScopeId` does not grant access.

Every sync request authenticates and reauthorizes scope use.

---

## 100. Device-Specific Scope

Server may bind subscription to:

```text
DeviceId
```

for policies like:

```text
maximum devices
offline access
device trust
```

---

## 101. Actor-Specific Scope

Some scope membership is actor-specific.

If actor changes on same device:

```text
old subscriptions may need revoke/remove
new subscriptions bootstrap
```

---

## 102. Shared Device

For multiple users on one physical device, safest design:

```text
separate local profile store per user/tenant
```

This avoids cross-user data leakage.

---

## 103. Subscription Lifecycle

```text
Requested
↓
Resolved
↓
Bootstrapping
↓
Active
├─ Expanding
├─ Contracting
├─ Suspended
├─ ResyncRequired
└─ Revoked
```

---

## 104. Suspended

Used when:

```text
temporary policy
billing state
maintenance
```

and data may remain locally.

Do not confuse with revocation.

---

## 105. Revoked

Means:

```text
access no longer valid
```

typically requires local data removal.

---

## 106. Server Response

Every exchange can include:

```rust
pub struct ScopeStatus {
    pub scope_id: ScopeId,
    pub version: ScopeVersion,
    pub generation: ScopeGeneration,
    pub state: ScopeServerState,
}
```

---

## 107. Transition Instructions

Server may return:

```text
Continue
ExpandWithBootstrap
ContractWithRemovals
FullResync
Revoke
Suspend
```

---

## 108. Atomic Contraction Apply

Client should transactionally:

```text
apply scope removals
update membership table
preserve/mark affected pending ops
update scope version
commit
```

---

## 109. Cursor Advance During Contraction

New scope version and membership changes must be installed coherently.

Do not set:

```text
scope_version = new
```

before removals are durably applied.

---

## 110. Expansion Install Atomicity

For large expansion:

```text
stage added partition
verify
atomically activate membership
update scope version/cursor
```

---

## 111. Scope Transition ID

Define:

```rust
pub struct ScopeTransitionId(Uuid);
```

Useful for resumable expansion/contraction and diagnostics.

---

## 112. Idempotent Scope Transition

Retrying same transition must not duplicate/delete incorrectly.

Persist:

```text
transition_id
from version
to version
state
```

---

## 113. Interrupted Expansion

On restart:

```text
resume staged transition
```

or discard staging and restart safely.

Old active scope remains coherent until activation.

---

## 114. Interrupted Contraction

Contraction is security-sensitive.

Prefer transactional or partitioned idempotent removal with transition progress.

Do not leave client claiming new version while old unauthorized data remains.

---

## 115. Urgent Revocation

For severe revocation:

```text
stop presenting affected data immediately
```

even if physical cleanup continues in background.

Use logical access gate first.

---

## 116. Logical Access Gate

Client UI/repositories consult active membership.

When revoked:

```text
membership inactive immediately
```

Physical deletion can follow.

This prevents data display during cleanup.

---

## 117. Local Query Safety

Application repositories should query through:

```text
active scope/profile context
```

rather than raw local tables if data from multiple scopes/users coexist.

---

## 118. Scope Membership API

Client SDK may expose:

```rust
aequora.is_entity_active_in_scope(entity, scope)
```

for advanced use.

Ordinary app code should rely on repositories.

---

## 119. Search Index Cleanup

When data leaves scope:

```text
local FTS/search index
cache
derived projections
```

must also remove or deactivate it.

Scope removal should emit local maintenance hooks.

---

## 120. Blob Cleanup

If entity leaves all scopes:

```text
BlobRef membership count may drop
```

Unreferenced local blob can be cleaned according to policy.

---

## 121. Scope-Aware Blob Authorization

Blob fetch must validate active scope/entity authorization.

Possessing old BlobRef must not grant access after revocation.

---

## 122. Scope-Aware Outbox

Every outgoing operation should record:

```text
origin scope/subscription if applicable
```

for diagnostics.

But write authorization is independently rechecked server-side.

---

## 123. Cross-Scope Operation

Some operations touch entities across scopes.

Example:

```text
move student between classes
```

This is domain operation semantics, not two client scopes controlling authority.

Server handles transaction and resulting membership transitions.

---

## 124. Membership-Changing Operation

An accepted operation may itself change scope membership.

Example:

```text
AssignTeacherToClass
```

Server emits:

```text
domain event
+
scope version changes for affected teachers
```

---

## 125. Scope Recalculation

Do not synchronously recompute millions of user scopes inside one business transaction.

Use:

```text
durable scope-change job/feed
```

where scale requires.

---

## 126. Strong Immediate Revocation

For security-critical revocation, authorization is checked at request time immediately even if scope transition feed lags.

Thus stale local data cannot be used to perform unauthorized server writes.

---

## 127. Read Revocation Lag

Local offline client may still possess previously authorized data while disconnected.

No sync architecture can remotely erase a physically offline device instantly.

Document this threat model.

---

## 128. Offline Revocation Reality

Guarantee:

```text
once device reconnects and receives revocation,
Aequora stops exposing/syncing revoked scope and removes according to policy
```

For stronger protection, use:

```text
encrypted local data with revocable keys
```

covered later in cryptographic/security parts.

---

## 129. Scope Privacy

Do not expose server-internal membership rationale unnecessarily.

Client needs:

```text
scope ID
version
state
transition instructions
```

not confidential policy details.

---

## 130. Scope Diagnostics

Admin should inspect:

```text
scope owner
definition
version
generation
active partitions
last cursor
transition state
```

subject to RBAC.

---

## 131. Client Diagnostics

Expose:

```text
subscription state
scope version
generation
cursor
last bootstrap
pending transition
```

---

## 132. Metrics

Server:

```text
scope_resolution_total
scope_expansion_total
scope_contraction_total
scope_revocation_total
scope_bootstrap_bytes
scope_transition_duration
```

Client:

```text
active_scope_count
scope_transition_pending
scope_removal_entities
scope_expansion_entities
```

---

## 133. Alerting

Alert on:

```text
repeated scope transition failure
revocation cleanup failure
scope resolver errors
mass unexpected expansion
membership diff explosion
```

---

## 134. Scope Change Storm

Example:

```text
new academic year
```

could update thousands of scopes.

Use:

```text
partitioned background recomputation
rate limiting
staggered client bootstrap
```

---

## 135. Bulk Scope Transition

Server can prepare:

```text
new partition snapshots
```

once and reuse across many eligible clients where projections match.

---

## 136. Shared Snapshot Artifact

Example:

```text
Campus C / Class A / Attendance module
```

snapshot can be reused by many teacher devices, with authorization checked before delivery.

---

## 137. Snapshot Cache Key

```text
dataset partition
projection version
boundary
schema version
```

not user identity unless data truly user-specific.

---

## 138. Scope Resolver Caching

Cache resolved membership carefully.

Invalidation triggers:

```text
role change
assignment change
tenant policy change
module permission change
```

Stale cache must not override authoritative authorization.

---

## 139. Fail-Closed Rule

If scope authorization cannot be determined safely:

```text
do not expand access
```

Return retryable/error.

For existing scope, application policy decides whether reads remain local while server unavailable.

---

## 140. Schema and Scope Evolution

If a schema upgrade changes which fields/entities belong to scope:

```text
increment ScopeVersion
```

or `ScopeGeneration` if incremental transition is unsafe.

---

## 141. Protocol Evolution

Scope transition messages are versioned capabilities.

Older clients may receive:

```text
ResyncRequired
```

instead of complex partial transitions.

That is a safe compatibility fallback.

---

## 142. Minimal v1 Scope System

The first production implementation should support:

```text
one or more named server-defined scopes
scope-specific cursor
full bootstrap per scope
server-detected version change
full resync on scope change
revocation
```

This is simpler and correct.

---

## 143. Phase 2 Scope Optimization

Then add:

```text
partial expansion
incremental contraction
scope removals
shared partition snapshots
```

---

## 144. Phase 3 Scale Optimization

Later:

```text
precomputed scope feeds
partition indexes
large-scale dynamic membership
```

---

## 145. Why Start Conservatively

A wrong partial-scope transition can leak or orphan data.

A full scope rebootstrap is more expensive but much easier to reason about.

Correctness first.

---

## 146. Scope Adapter Requirements

Authoritative adapter/server layer must support:

```text
scope-filtered journal read
consistent scope snapshot
membership transition metadata
scope cursor validation
```

---

## 147. Local Adapter Requirements

Client adapter must support:

```text
scope subscription metadata
entity-to-scope membership
scope cursor
scope transition state
atomic activation/removal
```

---

## 148. Universal DB Interoperability

Scope semantics remain canonical.

Physical implementations may differ:

```text
SQL:
    membership table

KV:
    scope/entity keys

document:
    metadata collection
```

No database-pair-specific protocol.

---

## 149. Scope Compliance Suite

Test:

```text
full bootstrap
filtered pull
expansion
contraction
revocation
multi-scope shared entity
cursor mismatch
generation mismatch
interrupted transition
```

---

## 150. Correctness Invariants

Add to Part 01.

### AEQ-INV-SCP001

```text
A scope cursor is interpreted only with matching ScopeId, ScopeVersion, and ScopeGeneration.
```

### AEQ-INV-SCP002

```text
Client never receives data outside server-authorized scope.
```

### AEQ-INV-SCP003

```text
Scope contraction removes/deactivates data no longer authorized before activating the new scope version.
```

### AEQ-INV-SCP004

```text
ScopeRemoval never implies authoritative domain deletion.
```

### AEQ-INV-SCP005

```text
An entity referenced by another active local scope is not physically removed solely because one scope contracts.
```

### AEQ-INV-SCP006

```text
Pending operations are never silently transmitted after their originating authorization/scope is revoked.
```

### AEQ-INV-SCP007

```text
Expansion does not activate incomplete bootstrap data.
```

---

## 151. Property Tests

Generate:

```text
scope expansion
scope contraction
entity moves between partitions
multiple overlapping scopes
revocation with pending operation
```

Assert:

```text
no unauthorized active data
correct membership refcounts
cursor/version coherence
```

---

## 152. Model Checking

Extend abstract model with:

```text
scope membership
permission changes
transition states
```

Explore:

```text
scope changes while response lost
revocation during pending operation
expansion interrupted
multi-scope removal
```

---

## 153. Fault Injection

Inject crash:

```text
during expansion staging
before activation
during contraction
before membership-version commit
after logical revoke before physical cleanup
```

---

## 154. Example — Teacher Scope Expansion

Initial:

```text
Teacher T
Classes: A
ScopeVersion 4
Cursor 900
```

Assignment adds Class B.

Server:

```text
ScopeVersion 5
added partition B
bootstrap boundary 950
```

Client:

```text
stage Class B through 950
apply journal >950
activate B
set scope version 5
```

---

## 155. Example — Teacher Scope Contraction

Initial:

```text
Classes A+B
```

Teacher loses B.

Server sends:

```text
ScopeTransition v5→v6
Remove partition B
```

Client:

```text
deactivate B membership
remove entities only referenced by B
preserve shared entities needed by A
update scope version
```

---

## 156. Example — Student Self Scope

Scope:

```text
own StudentId
own fee records
own timetable
own results
```

Server determines identity from `AuthContext`.

Client never supplies another StudentId as authoritative scope identity.

---

## 157. Example — Finance Module Scope

Accountant receives:

```text
finance entities
selected campus
current + previous fiscal period
```

A change in campus assignment triggers scope transition.

---

## 158. Recommended Modules

```text
aequora-core/
└── scope/
    ├── id.rs
    ├── version.rs
    ├── cursor.rs
    ├── subscription.rs
    └── transition.rs

aequora-server/
└── scope/
    ├── resolver.rs
    ├── membership.rs
    ├── filter.rs
    ├── projection.rs
    └── transition.rs

aequora-client/
└── scope/
    ├── subscription.rs
    ├── membership.rs
    ├── bootstrap.rs
    ├── contraction.rs
    └── state.rs
```

---

## 159. Logical Client Tables

```text
aequora_subscription
aequora_scope_cursor
aequora_scope_membership
aequora_scope_transition
```

---

## 160. Logical Server Tables

Optional, depending on application:

```text
aequora_scope_instance
aequora_scope_transition
aequora_scope_device_state
```

Many scope definitions can be computed rather than fully materialized.

---

## 161. Public Client API

Conceptual:

```rust
let sub = aequora
    .subscribe(TeacherWorkspaceRequest { ... })
    .await?;
```

Most applications should establish subscriptions automatically after authentication.

---

## 162. Server Registration

```rust
ScopeRegistry::builder()
    .register(TeacherWorkspaceScope::new(...))
    .register(StudentSelfScope::new(...))
    .register(FinanceWorkspaceScope::new(...))
    .build()?;
```

---

## 163. Plug-and-Play Defaults

For simple projects:

```text
DefaultTenantScope
```

can mean:

```text
all authorized syncable data in current tenant
```

Developers need not design fine-grained scopes initially.

---

## 164. Progressive Adoption

Start project with:

```text
one tenant-wide scope
```

Later split into:

```text
core
finance
documents
campus
user-specific
```

without rewriting the synchronization core.

---

## 165. Completion Criteria

Part 07 is complete when:

```text
[ ] ScopeDefinitionId defined
[ ] ScopeId defined
[ ] ScopeVersion defined
[ ] ScopeGeneration defined
[ ] ScopeCursor defined
[ ] ScopeResolver defined
[ ] subscription lifecycle defined
[ ] filtered cursor semantics defined
[ ] full bootstrap per scope defined
[ ] expansion architecture defined
[ ] contraction architecture defined
[ ] ScopeRemoval distinct from Tombstone
[ ] multi-scope membership reference tracking defined
[ ] revocation and pending-op handling defined
[ ] projection rules defined
[ ] scope transition atomicity defined
[ ] anti-entropy/bootstrap/QoS integrations defined
[ ] adapter compliance tests defined
[ ] correctness invariants added
```

---

## 166. Final Architecture

```text
                  AUTHENTICATED CLIENT
                         │
                         ▼
                    Scope Request
                         │
                         ▼
                   Scope Resolver
                         │
             server authorization/policy
                         │
                         ▼
                  Resolved Scope v7
                         │
            ┌────────────┼────────────┐
            ▼            ▼            ▼
        Snapshot      Journal      Projection
        Builder       Filter        Policy
            │            │            │
            └────────────┼────────────┘
                         ▼
                    Scope Cursor
                         │
                         ▼
                      Client
                         │
        ┌────────────────┼────────────────┐
        ▼                ▼                ▼
   Subscription      Membership       Local Data
      State             Map
        │
        ├── expansion -> partial/full bootstrap
        ├── contraction -> ScopeRemoval
        ├── revocation -> deactivate/remove
        └── generation change -> ResyncRequired
```

The architectural principle is:

> **Aequora synchronizes an authorized, versioned dataset—not an arbitrary query result. Scope identity, scope version, authority generation, membership, and cursor must move together as one coherent synchronization contract.**

This gives Aequora a safe foundation for tenant-aware, module-aware, branch-aware, user-specific, dynamically changing datasets without leaking data or corrupting cursor semantics.
