# Aequora Sync — Part 02

# Causality, Dependency, Provenance, and Event Lineage Architecture

## 1. Purpose

Aequora already defines:

```text
OperationId
EntityVersion
Cursor
Journal Sequence
DeviceId
ActorId
TenantId
```

Those are necessary, but they are not enough to explain **why** an event happened, **what caused it**, **which earlier operations it depends on**, or **how one user action eventually produced many downstream state transitions**.

A production-grade synchronization engine needs a complete model for:

```text
causality
dependency
provenance
correlation
lineage
derived operations
audit chains
replay/debugging
```

The key architectural rule is:

> **Authoritative journal order is not the same thing as causality.**

A journal sequence gives a total replication order.

Causality explains logical relationships.

They must remain separate.

---

# 2. Why This Matters

Consider:

```text
Teacher submits attendance
```

That one user action could produce:

```text
MarkAttendance operation
↓
AttendanceUpdated event
↓
StudentDailySummaryUpdated event
↓
AbsenceAlertRequested event
↓
Notification job
↓
Email sent
```

Without lineage metadata, the system can only show:

```text
five unrelated records
```

With lineage metadata, Aequora can explain:

```text
all five originated from one teacher action
```

---

# 3. Four Different Relationships

Aequora should distinguish:

```text
identity
causation
correlation
dependency
```

These must not be collapsed into one ID.

---

# 4. Identity

Every operation has:

```text
OperationId
```

Every authoritative event has:

```text
EventId
```

These answer:

```text
"What exact object is this?"
```

---

# 5. Causation

`CausationId` answers:

```text
"What directly caused this?"
```

Example:

```text
Operation A
↓
Event B
```

Then:

```text
Event B.causation = Operation A
```

---

# 6. Correlation

`CorrelationId` answers:

```text
"What larger user/system action does this belong to?"
```

One correlation may include:

```text
request
operation
multiple authoritative events
background jobs
notifications
webhooks
```

---

# 7. Dependency

A dependency answers:

```text
"What must exist/complete before this operation can safely execute?"
```

Example:

```text
CreateStudent
↓
CreateInvoice
↓
RecordPayment
```

Dependency is execution ordering.

Causation is semantic lineage.

They often overlap, but not always.

---

# 8. Core Identifier Types

Recommended:

```rust
pub struct CorrelationId(Uuid);
pub struct CausationId(Uuid);
pub struct EventId(Uuid);
pub struct LineageId(Uuid);
```

But avoid adding IDs without clear semantics.

Recommended minimum:

```text
OperationId
EventId
CorrelationId
caused_by: Option<LineageRef>
```

Where:

```rust
pub enum LineageRef {
    Operation(OperationId),
    Event(EventId),
    Job(JobId),
}
```

---

# 9. Root Correlation

A user-initiated action should normally create one:

```text
CorrelationId
```

Example:

```text
Teacher presses "Save Attendance"
```

Client creates:

```text
CorrelationId = C-100
```

Every resulting operation/event derived from that action keeps:

```text
correlation_id = C-100
```

---

# 10. Correlation Across Retries

Retries must preserve:

```text
OperationId
CorrelationId
CausationId
```

A retry is not a new logical operation.

Therefore:

```text
network retry
≠
new correlation
```

---

# 11. Correlation Across Derived Work

Suppose:

```text
PostPayment
```

produces:

```text
PaymentPosted
LedgerEntryCreated
ReceiptRequested
```

All retain:

```text
correlation_id = original payment correlation
```

Each event gets its own `EventId`.

---

# 12. Causation Chain

Example:

```text
Operation O1
   ↓
Event E1
   ↓
Job J1
   ↓
Event E2
```

Metadata:

```text
E1.caused_by = O1
J1.caused_by = E1
E2.caused_by = J1
```

This forms a causal chain.

---

# 13. Lineage DAG

Causality is not always a simple chain.

Example:

```text
Operation O1
   ├── Event E1
   └── Event E2

Event E1 + Event E2
   ↓
Derived Event E3
```

This forms a DAG.

Aequora must support:

```text
one parent
or
multiple causal inputs
```

depending on use case.

---

# 14. Minimal vs Extended Lineage

For the first implementation, use:

```text
one primary caused_by
+
CorrelationId
+
dependency list
```

For advanced workflows, support:

```text
causal_inputs: SmallVec<[LineageRef; 4]>
```

Do not add multi-parent complexity until needed.

---

# 15. Operation Envelope Extension

Recommended:

```rust
pub struct OperationEnvelope {
    pub operation_id: OperationId,
    pub correlation_id: CorrelationId,
    pub caused_by: Option<LineageRef>,

    pub tenant_id: TenantId,
    pub actor_id: ActorId,
    pub device_id: DeviceId,

    pub entity: EntityRef,
    pub base_version: Option<EntityVersion>,

    pub dependencies: SmallVec<[OperationId; 4]>,

    pub client_time: HybridTimestamp,
    pub payload: Bytes,
}
```

---

# 16. Authoritative Event Extension

```rust
pub struct AuthoritativeEvent {
    pub event_id: EventId,
    pub correlation_id: CorrelationId,
    pub caused_by: LineageRef,

    pub sequence: Sequence,
    pub entity: EntityRef,
    pub version: EntityVersion,

    pub schema_version: SchemaVersion,
    pub payload: Bytes,
}
```

---

# 17. Journal Sequence vs Causal Order

Suppose:

```text
E1 causes E3
E2 unrelated
```

The journal may be:

```text
100 = E1
101 = E2
102 = E3
```

The sequence tells replication order:

```text
E1 < E2 < E3
```

But causality says only:

```text
E1 → E3
```

No causal relationship exists between E1 and E2.

Do not infer causality from sequence adjacency.

---

# 18. Causal Partial Order

Causality is naturally:

```text
partial order
```

while journal sequence is:

```text
total order
```

Aequora should preserve both.

---

# 19. Why Total Order Alone Is Misleading

If operators inspect:

```text
sequence 500
sequence 501
```

they may wrongly assume:

```text
500 caused 501
```

This is false.

Use explicit `caused_by`.

---

# 20. HLC Role

Hybrid Logical Clock may provide:

```text
approximate causal-friendly timestamp
```

but HLC does not replace:

```text
caused_by
dependencies
journal sequence
```

Use HLC for:

```text
temporal metadata
causal tie-breaking
debugging
```

not authoritative lineage.

---

# 21. Dependency DAG

Operations may declare:

```rust
dependencies: SmallVec<[OperationId; 4]>
```

Dependency semantics:

```text
operation B cannot execute before operation A reaches required outcome
```

---

# 22. Dependency Types

Not all dependencies mean the same thing.

Potential types:

```text
Exists
Accepted
Committed
Succeeded
Visible
```

Initial implementation should keep one simple semantic:

> Dependency means the referenced operation must have committed successfully.

---

# 23. Future Typed Dependencies

Later:

```rust
pub struct OperationDependency {
    pub operation_id: OperationId,
    pub requirement: DependencyRequirement,
}
```

Where:

```rust
enum DependencyRequirement {
    Committed,
    AcceptedOrAlreadyApplied,
}
```

Avoid overcomplicating v1.

---

# 24. Dependency Validation

Server must verify:

```text
dependency exists
or
dependency is present in current batch
```

Unknown dependency:

```text
DependencyMissing
```

Rejected dependency:

```text
DependencyFailed
```

---

# 25. Dependency Cycles

Example:

```text
A depends on B
B depends on A
```

Reject before execution.

Use DAG cycle detection.

---

# 26. Topological Planning

Server planner:

```text
build dependency graph
↓
detect cycle
↓
topological sort
↓
group independent operations
↓
execute
```

---

# 27. Causation vs Dependency Example

```text
CreateInvoice depends on CreateStudent
```

But:

```text
CreateStudent
```

did not necessarily **cause** the invoice.

Maybe the user created both separately.

Therefore:

```text
dependency ≠ causation
```

---

# 28. Derived Operation

A server handler may intentionally emit another operation.

Example:

```text
ApproveStudentEnrollment
↓
CreateInitialFeeSchedule
```

Then:

```text
new_op.caused_by = approving operation/event
new_op.correlation_id = original correlation
```

---

# 29. Derived Event

Most commonly, handler emits authoritative events.

Example:

```text
PostPayment operation
↓
PaymentPosted event
↓
LedgerAdjusted event
```

Both share one correlation.

---

# 30. User Action Boundary

The application should create a new `CorrelationId` when a new top-level logical action starts.

Examples:

```text
save attendance form
submit invoice
approve leave request
import student batch
```

---

# 31. Do Not Create Correlation Per HTTP Request

HTTP retries may generate several requests for one logical action.

Correlation belongs to the domain/user action.

Not the network request.

---

# 32. RequestId Is Separate

Keep:

```text
RequestId
```

for transport observability.

One correlation may span:

```text
RequestId R1
RequestId R2
RequestId R3
```

due to retries.

---

# 33. SessionId Is Separate

`SessionId` answers:

```text
which authentication/sync session?
```

not:

```text
which business action?
```

---

# 34. TraceId Is Separate

OpenTelemetry trace IDs may be useful but should not become durable business lineage IDs.

Tracing systems can sample/drop data.

Correlation metadata must be durable.

---

# 35. Provenance Model

Every authoritative mutation should be attributable to:

```text
tenant
actor
device
operation
correlation
causation
server handler
server version/build
```

where appropriate.

---

# 36. Provenance Record

Conceptual:

```rust
pub struct Provenance {
    pub actor_id: ActorId,
    pub device_id: DeviceId,
    pub operation_id: OperationId,
    pub correlation_id: CorrelationId,
    pub caused_by: Option<LineageRef>,
    pub handler_id: HandlerId,
}
```

---

# 37. Server-Originated Operations

Some operations have no client actor.

Examples:

```text
scheduled billing
automatic cleanup
policy enforcement
background import
```

Use explicit actor type:

```rust
pub enum Principal {
    User(ActorId),
    Service(ServiceId),
    System(SystemPrincipal),
}
```

Do not fake a user ID.

---

# 38. Provenance Principal

Recommended:

```rust
pub enum OriginPrincipal {
    User(ActorId),
    Service(ServiceId),
    System(SystemActor),
}
```

---

# 39. Device Optionality

Server-originated work may not have a `DeviceId`.

Therefore provenance should support:

```text
origin device = optional
```

---

# 40. Trusted vs Untrusted Provenance

Client can claim some metadata.

But authoritative provenance must distinguish:

```text
client asserted
server verified
server generated
```

Never trust arbitrary actor IDs from payload.

---

# 41. Authoritative Provenance Construction

Server should derive:

```text
actor
tenant
session
device
```

from trusted authentication context.

Client contributes:

```text
OperationId
CorrelationId
caused_by
```

subject to validation.

---

# 42. Correlation Spoofing

A malicious client could reuse another correlation ID.

This should not grant permission.

Correlation IDs are:

```text
diagnostic/lineage metadata
```

not authorization credentials.

---

# 43. Causation Validation

Server should ensure claimed `caused_by` is plausible.

Possible checks:

```text
same tenant
referenced object exists
actor/device allowed to reference it where necessary
```

But do not make lineage validation so expensive that every request requires massive graph traversal.

---

# 44. Foreign-Tenant Lineage

Reject:

```text
tenant A operation
caused_by tenant B private event
```

unless explicitly allowed by cross-tenant system workflow.

---

# 45. Batch Correlation

A batch can contain:

```text
many correlations
```

because transport batching is independent of logical action grouping.

Do not force one correlation per SyncRequest.

---

# 46. Form Submission Example

A single form may create:

```text
UpdateStudentName
UpdateStudentAddress
UpdateGuardianPhone
```

All can share:

```text
CorrelationId C1
```

while remaining distinct operations.

---

# 47. Bulk Import Example

A whole import job can have:

```text
CorrelationId C_IMPORT
```

Each imported record has its own OperationId.

Optionally a sub-correlation or batch ID can further organize large imports.

---

# 48. Parent Correlation

Do not create recursive correlation hierarchies initially.

Use:

```text
CorrelationId
+
caused_by chain
```

If hierarchical workflows later require it, add:

```text
parent_correlation_id
```

carefully.

---

# 49. Event Lineage Storage

Authoritative journal should store:

```text
event_id
correlation_id
caused_by_kind
caused_by_id
origin actor/service
origin device
operation_id
```

where useful.

---

# 50. Operation Ledger Lineage

Operation ledger should preserve:

```text
correlation_id
caused_by
origin
```

so retries and audits reproduce original lineage.

---

# 51. Retry Must Not Rewrite Provenance

If operation is retried from a new HTTP request:

```text
RequestId changes
```

but:

```text
OperationId
CorrelationId
caused_by
original device
```

remain stable.

---

# 52. Forwarded Operation

Suppose server A forwards work to server B.

Do not rewrite lineage into:

```text
server A caused everything
```

Preserve original correlation and provenance while recording the forwarding hop separately in observability.

---

# 53. Conflict Resolution Provenance

Manual conflict resolution creates a new operation.

Example:

```text
original O1 conflicts
user chooses server value
new operation O2 resolves
```

Set:

```text
O2.caused_by = ConflictId / related operation
O2.correlation_id = new resolution action
```

or reuse original correlation depending on UX semantics.

Recommended:

```text
new top-level human action → new CorrelationId
```

while linking original conflict in `caused_by`.

---

# 54. ConflictId

Introduce:

```rust
pub struct ConflictId(Uuid);
```

if conflicts are durable first-class records.

`LineageRef` can then support:

```rust
Conflict(ConflictId)
```

---

# 55. Job Lineage

Background jobs need:

```text
JobId
CorrelationId
caused_by
```

Example:

```text
InvoiceCreated event
↓
GenerateInvoicePdf job
```

---

# 56. External Side-Effect Lineage

Email/webhook/payment intent should also preserve:

```text
correlation
causation
```

This makes support investigations much easier.

---

# 57. Receipt Example

```text
PostPayment Operation O10
 correlation C7

↓ causes

PaymentPosted Event E11
 correlation C7
 caused_by O10

↓ causes

ReceiptGeneration Job J12
 correlation C7
 caused_by E11

↓ causes

ReceiptGenerated Event E13
 correlation C7
 caused_by J12
```

---

# 58. Audit Trail

Audit UI can reconstruct:

```text
Teacher Irshad
at 10:32
from Android device D3
performed MarkAttendance
which produced AttendanceUpdated
which caused AbsenceNotificationRequested
```

without relying on raw logs.

---

# 59. Audit vs Lineage

Lineage explains:

```text
causal relationships
```

Audit records:

```text
who/what/when/result
```

They overlap but are not identical.

Lineage metadata can feed audit records.

---

# 60. Provenance Immutability

Authoritative provenance should be immutable after commit.

If a correction is needed:

```text
append correction metadata/audit event
```

rather than rewrite historical lineage.

---

# 61. Replay Semantics

When replaying historical events for projections:

```text
preserve original event ID
correlation ID
causation
```

Do not generate new lineage unless a new derived effect is intentionally created.

---

# 62. Reprocessing vs New Processing

Projection rebuild:

```text
reprocess existing event
```

should not create a new business event.

A new materialized projection may record:

```text
source_event_id
```

without altering original lineage.

---

# 63. Derived Projection Lineage

A projection row may store:

```text
last_source_sequence
last_source_event_id
```

for diagnostics.

---

# 64. Multi-Consumer Lineage

Analytics/search/notification consumers should preserve:

```text
source EventId
CorrelationId
```

in their own processing metadata.

---

# 65. Distributed Trace Correlation

Tracing span fields:

```text
operation_id
correlation_id
event_id
caused_by
```

This bridges durable lineage with ephemeral telemetry.

---

# 66. Logging

Structured logs should include lineage IDs where safe.

Example:

```text
event="operation_committed"
operation_id=...
correlation_id=...
sequence=...
```

---

# 67. Metric Cardinality Warning

Do not use:

```text
OperationId
CorrelationId
EventId
```

as metric labels.

Use them only in logs/traces.

---

# 68. Causal Debugging

When investigating one event:

```text
find caused_by
↓
walk backward
↓
find root action
```

And optionally:

```text
find descendants
↓
understand downstream effects
```

---

# 69. Lineage Indexes

Authoritative store should index:

```text
event_id
operation_id
correlation_id
caused_by_id
```

depending on expected admin queries.

Avoid over-indexing every lineage field without measurement.

---

# 70. Correlation Query

Admin API:

```text
GET correlation C123
```

can return:

```text
root operation
all related operations
events
jobs
conflicts
```

subject to permissions.

---

# 71. Causal Graph Query

Advanced tooling can reconstruct a graph:

```text
O1
├─ E1
│  └─ J1
│     └─ E3
└─ E2
```

This is valuable for incident forensics.

---

# 72. Lineage Retention

Lineage retention should match:

```text
operation ledger
journal
audit
```

requirements.

If journal is compacted, durable audit/lineage may need separate retention.

---

# 73. Compacting Lineage

When old events are compacted, retain enough metadata for:

```text
audit
legal requirements
important causal investigation
```

This may mean summarizing lineage rather than storing full payload.

---

# 74. Privacy

Lineage IDs themselves are not usually sensitive, but linked provenance may identify users/devices.

Apply:

```text
authorization
retention
redaction
```

to admin access.

---

# 75. Device Privacy

Do not expose raw internal DeviceId in ordinary user UI unless useful.

Admin/support tools may use it.

---

# 76. Correlation in Client Local Store

Outbox records should persist:

```text
correlation_id
caused_by
```

so app restarts do not lose lineage.

---

# 77. Correlation in Pending Offline Work

A user may create several linked offline operations.

Dependencies and correlation survive until server sync.

---

# 78. Offline Derived Operations

Client-side domain logic may derive an operation before server contact.

Example:

```text
CreateOrder
↓
CreateOrderLine
```

Use:

```text
same CorrelationId
```

and explicit dependency.

---

# 79. Client-Side Causation

If one local operation directly causes another:

```text
O2.caused_by = O1
```

Server validates reference.

---

# 80. Server Rewriting Lineage

Server may normalize invalid/missing lineage.

Example:

```text
missing correlation
```

Possible policy:

```text
server generates correlation
```

But recommended client SDK should always generate it.

---

# 81. Backward Compatibility

Older clients may not send correlation metadata.

Protocol compatibility can define:

```text
server generates CorrelationId = OperationId-derived/random
```

for legacy operations.

---

# 82. Deterministic Correlation Fallback

Do not derive correlation from operation payload.

If missing, server may generate and persist one at first acceptance.

Retries then return stored correlation from operation ledger.

---

# 83. Protocol Versioning

Adding lineage fields should be additive where Postcard/schema compatibility permits.

Otherwise introduce new protocol/schema version.

---

# 84. Operation Manifest

Operation descriptor may declare:

```text
may_emit_events
may_emit_jobs
allows_dependencies
```

Useful for static analysis and docs.

---

# 85. Causal Depth

Protect against pathological causal chains.

Define optional maximum diagnostic traversal depth.

Do not reject valid business operations solely because a long historical lineage exists unless resource abuse is possible.

---

# 86. Dependency Bomb Protection

Dependencies are network input.

Bound:

```text
max dependencies per operation
max DAG nodes per batch
max edges
```

Cycle detection must be linear-time.

---

# 87. Causation Reference Abuse

`caused_by` should reference one stable ID, not an arbitrary unbounded list in the basic protocol.

Multi-parent causal inputs, if enabled, need strict bounds.

---

# 88. Causal Integrity Validation

Do not require synchronous recursive traversal of entire lineage for every operation.

Validate only:

```text
direct reference existence
tenant compatibility
basic type validity
```

Deeper graph integrity can be checked asynchronously.

---

# 89. Lineage Integrity Job

Optional verifier checks:

```text
dangling caused_by
cross-tenant lineage
causal cycles where forbidden
missing root correlation
```

---

# 90. Causality Cycles

Causality should normally be acyclic.

If:

```text
E1 caused_by E2
E2 caused_by E1
```

that is invalid.

Prevent creation where feasible.

---

# 91. Distinguish Workflow Loops

Business workflows may loop:

```text
review → revise → review
```

This does not require causal graph cycles.

Each iteration creates new events, preserving a forward DAG.

---

# 92. Sequence and Causality Invariant

For server-generated lineage where both records are in same authoritative timeline:

```text
cause.sequence < effect.sequence
```

should normally hold.

This is a useful invariant.

---

# 93. Exceptions

If cause is:

```text
external system event
client-side operation
conflict record
```

it may not have authoritative journal sequence.

So sequence ordering cannot universally define causality.

---

# 94. Provenance Handler ID

Each server operation handler may have stable:

```text
HandlerId
```

or operation kind/schema may be sufficient.

Do not persist unstable Rust type names.

---

# 95. Build Provenance

For high-assurance environments, authoritative audit may store:

```text
server release version
protocol version
handler schema version
```

This helps explain historical behavior.

---

# 96. Avoid Full Binary Build Hash Per Event by Default

That can create unnecessary storage.

Store:

```text
release/build version
```

in operation ledger or request/session metadata and reference it where needed.

---

# 97. Cross-Service Causality

If Aequora later integrates multiple services, propagate:

```text
CorrelationId
caused_by
```

across service boundaries.

Do not rely only on trace headers.

---

# 98. Cross-Database Causality

Secondary consumers preserve source lineage even if they use another database.

Example:

```text
PostgreSQL event E1
↓
search index document
source_event_id = E1
```

---

# 99. External Webhook Causality

Webhook delivery metadata:

```text
delivery_id
source_event_id
correlation_id
```

If partner calls back later, optionally map external reference into a new correlation/causation link.

---

# 100. Import Provenance

Bulk import should identify:

```text
ImportJobId
CorrelationId
source system
source record key
```

This provides traceability from imported record to origin.

---

# 101. Migration Provenance

During DB migration:

```text
do not rewrite business correlation
```

Storage migration is not a new business action.

Preserve lineage.

---

# 102. Rebootstrap Provenance

Client bootstrap does not create new authoritative events.

It simply installs existing authoritative state.

No new business lineage should be generated.

---

# 103. Snapshot Provenance

Snapshot records may contain:

```text
latest_event_id
latest_sequence
```

for diagnostics.

Full causal graph need not be embedded in every snapshot record.

---

# 104. API for Root Correlation

Client SDK:

```rust
let correlation = aequora.begin_action();
```

Then:

```rust
ctx.enqueue_with_correlation(op, correlation)
```

But avoid requiring manual ID management everywhere.

---

# 105. Automatic Correlation Scope

Ergonomic option:

```rust
aequora
    .action(|action| async move {
        action.enqueue(op1).await?;
        action.enqueue(op2).await?;
    })
    .await?;
```

All operations share one generated CorrelationId.

---

# 106. Nested Action Policy

Nested action scopes should either:

```text
inherit parent correlation
```

or explicitly start a new correlation.

Default:

```text
inherit
```

to avoid accidental fragmentation.

---

# 107. Server Action Scope

Derived server operations/events inherit current correlation automatically unless handler explicitly starts a new top-level system action.

---

# 108. Lineage Context

Internal type:

```rust
pub struct LineageContext {
    pub correlation_id: CorrelationId,
    pub caused_by: Option<LineageRef>,
}
```

Pass through executor/job/event APIs.

---

# 109. Handler Event API

Instead of:

```rust
emit(event)
```

use:

```rust
ctx.emit(event)
```

where context automatically applies:

```text
correlation
causation
origin
```

This prevents developers forgetting lineage metadata.

---

# 110. Job Enqueue API

Likewise:

```rust
ctx.enqueue_job(job)
```

automatically links the new job to current authoritative event/operation.

---

# 111. Strong Default

Aequora should make:

```text
lineage-preserving API
```

the easy path.

Manual raw metadata construction should be advanced/internal.

---

# 112. Admin Visualization

Future admin UI can display:

```text
Correlation C1

Root:
Teacher submitted attendance

Operations:
O1 MarkAttendance

Events:
E1 AttendanceUpdated
E2 AbsenceDetected

Jobs:
J1 SendNotification

Outcome:
Completed
```

---

# 113. Incident Example

Problem:

```text
Parent received wrong absence email.
```

Operator searches email delivery.

Finds:

```text
Job J90
caused_by Event E80
correlation C20
```

Walks backward:

```text
E80 caused_by O70
O70 originated on device D4
actor A3
```

Now the entire path is explainable.

---

# 114. Correctness Invariants

Add to Part 01 invariant registry.

## AEQ-INV-C001

```text
Every authoritative event has exactly one EventId.
```

## AEQ-INV-C002

```text
Every authoritative event has one CorrelationId.
```

## AEQ-INV-C003

```text
Retry of same OperationId preserves original correlation/provenance.
```

## AEQ-INV-C004

```text
Server-derived event caused by committed operation references that operation/event lineage.
```

## AEQ-INV-C005

```text
Cross-tenant lineage references are rejected unless explicitly authorized.
```

## AEQ-INV-C006

```text
Causality graph is acyclic for authoritative internal events.
```

---

# 115. Model-Checking Causality

Extend abstract model with:

```text
correlation
caused_by
dependency graph
```

Verify:

```text
retry does not fork lineage
derived event retains correlation
dependency cycle rejected
causal cycle impossible
```

---

# 116. Property Tests

Generate random derived workflows.

Assert:

```text
all descendants retain root correlation
cause exists
same OperationId never changes provenance
```

---

# 117. Storage Adapter Requirements

Adapters must preserve lineage fields losslessly.

Add to compliance suite:

```text
CorrelationId roundtrip
LineageRef roundtrip
EventId uniqueness
operation retry provenance equality
```

---

# 118. Protocol Manifest

Manifest should include whether lineage fields are:

```text
required
optional
legacy-generated
```

for each supported protocol generation.

---

# 119. Recommended Internal Modules

```text
aequora-core/
└── lineage/
    ├── mod.rs
    ├── correlation.rs
    ├── causation.rs
    ├── provenance.rs
    ├── dependency.rs
    └── context.rs
```

Server:

```text
aequora-server/
├── lineage.rs
├── dependency_planner.rs
└── provenance.rs
```

TestKit:

```text
aequora-testkit/
└── lineage_assertions.rs
```

---

# 120. Database Schema Additions

Logical operation ledger:

```text
operation_id
correlation_id
caused_by_kind
caused_by_id
origin_principal_kind
origin_principal_id
device_id
...
```

Journal:

```text
event_id
operation_id
correlation_id
caused_by_kind
caused_by_id
sequence
...
```

---

# 121. Index Recommendations

Initially:

```text
UNIQUE(event_id)
UNIQUE(operation_id)
INDEX(correlation_id)
INDEX(caused_by_id)
```

Measure before adding more.

---

# 122. Client Outbox Additions

Persist:

```text
correlation_id
caused_by
```

with each pending operation.

---

# 123. Conflict Store Additions

Persist:

```text
source_operation_id
source_correlation_id
resolution_operation_id
```

when available.

---

# 124. Durable Job Store Additions

Persist:

```text
job_id
correlation_id
caused_by
source_event_id
```

---

# 125. Backward Compatibility Strategy

For old operation rows lacking correlation:

```text
assign stable generated correlation during migration or first read
```

Prefer deterministic migration preserving one value forever.

---

# 126. Migration Rule

Never generate a new correlation every time a legacy row is read.

Migration must persist the generated value.

---

# 127. Performance

Lineage metadata adds some bytes per operation/event.

This is acceptable because:

```text
traceability
auditability
debuggability
```

are high-value capabilities.

Use compact binary IDs.

---

# 128. Payload Separation

Lineage metadata belongs in the envelope, not embedded in application payload.

This keeps business structs clean.

---

# 129. Security

Lineage metadata must not bypass auth.

Example:

```text
caused_by = privileged operation
```

does not grant privilege.

Authorization always evaluates current operation independently.

---

# 130. Data Minimization

Do not put:

```text
full username
email
IP
device name
```

inside every lineage envelope.

Use stable IDs and look up descriptive metadata when authorized.

---

# 131. Completion Criteria

Part 02 is complete when:

```text
[ ] CorrelationId defined
[ ] EventId defined
[ ] LineageRef defined
[ ] OperationEnvelope extended
[ ] authoritative event extended
[ ] correlation survives retry
[ ] dependency planner remains separate from causality
[ ] server derived event API inherits lineage
[ ] job outbox inherits lineage
[ ] provenance built from trusted auth context
[ ] lineage persisted in operation ledger/journal
[ ] admin correlation query specified
[ ] adapter compliance includes lineage roundtrip
[ ] Part 01 invariants extended
[ ] model tests cover lineage preservation
```

---

# 132. Final Architecture

```text
USER / SYSTEM ACTION
       │
       ▼
CorrelationId C1
       │
       ▼
Operation O1
       │
       ├──────── dependency ───────► Operation O2
       │
       ▼
Authoritative Event E1
       │
       ├────────► Event E2
       │
       └────────► Job J1
                        │
                        ▼
                     Event E3

Every node:
    unique identity

Every descendant:
    correlation C1

Every direct relationship:
    caused_by

Execution ordering:
    dependencies

Replication ordering:
    journal Sequence
```

The final principle is:

> **Sequence tells Aequora where a change sits in the authoritative replication stream. Causality tells Aequora why the change exists. Dependency tells Aequora what must happen before it. Provenance tells Aequora who or what originated it.**

Those concepts must remain distinct if Aequora is to be explainable, auditable, debuggable, and safe at enterprise scale.
