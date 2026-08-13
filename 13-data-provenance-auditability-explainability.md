# Aequora Sync — Part 13

# Data Provenance, Auditability, and Explainability Architecture

## 1. Purpose

Aequora already defines:

```text
OperationId
EventId
CorrelationId
causation
dependency
authoritative journal
operation ledger
deterministic execution
replay
```

These provide the technical foundation for understanding how synchronized state changed.

However, production enterprise systems need a higher-level architecture for questions such as:

```text
Who changed this value?
Which device initiated it?
Was it a user, service, import, or system process?
What was the previous value?
What rule accepted or rejected the operation?
Which authoritative event produced the current state?
Why does this client show this value?
Was the change imported, repaired, migrated, or manually edited?
Can an auditor reconstruct the change without reading raw logs?
```

That is the role of the provenance, auditability, and explainability subsystem.

The central rule is:

> **Technical lineage identifies the chain of events; audit provenance explains the business-relevant facts of that chain in a durable, queryable, policy-controlled form.**

---

# 2. Goals

The subsystem should provide:

```text
durable audit evidence
business-readable provenance
field-level change history where appropriate
who/what/when/where/why attribution
system/service attribution
tamper detection
incident investigation
compliance support
user-facing explanations
administrative search
retention controls
privacy-aware redaction
```

---

# 3. Non-Goals

The audit subsystem is not:

```text
a replacement for the synchronization journal
a replacement for application logs
a replacement for authorization
a general-purpose SIEM
a database WAL viewer
```

Each subsystem has a different purpose.

---

# 4. Distinguish Four Histories

Aequora should explicitly separate:

```text
1. Sync Journal
2. Operation Ledger
3. Business Audit Trail
4. Operational Logs/Traces
```

---

# 5. Sync Journal

Purpose:

```text
replication
cursor progression
authoritative state propagation
```

Characteristics:

```text
ordered
machine-oriented
retention driven by synchronization needs
```

---

# 6. Operation Ledger

Purpose:

```text
idempotency
execution result lookup
operation provenance
```

Characteristics:

```text
indexed by OperationId
stores acceptance/rejection/outcome
may retain handler/version/input digests
```

---

# 7. Business Audit Trail

Purpose:

```text
answer human/compliance questions
```

Examples:

```text
Irshad changed Student 123 phone from X to Y
System recalculated invoice after fee-plan change
Import job 44 created 4,200 student records
Admin revoked Finance scope from Device 9
```

This should be durable and queryable independently of journal retention.

---

# 8. Operational Logs

Purpose:

```text
debugging runtime behavior
performance
incidents
errors
```

Logs may expire quickly and are not authoritative audit evidence.

---

# 9. AuditEvent

Define a first-class type:

```rust
pub struct AuditEvent {
    pub audit_event_id: AuditEventId,
    pub tenant_id: TenantId,
    pub subject: AuditSubject,
    pub action: AuditAction,
    pub actor: AuditActor,
    pub occurred_at: DomainTimestamp,
    pub correlation_id: CorrelationId,
    pub caused_by: Option<LineageRef>,
    pub outcome: AuditOutcome,
    pub changes: Vec<AuditChange>,
    pub provenance: AuditProvenance,
}
```

---

# 10. AuditEventId

Define:

```rust
pub struct AuditEventId(Uuid);
```

Use globally unique IDs.

AuditEventId is distinct from EventId.

One authoritative event may produce:

```text
zero
one
or several
```

audit records depending on business semantics.

---

# 11. AuditSubject

Represents what the audit entry is about.

```rust
pub enum AuditSubject {
    Entity(EntityRef),
    Aggregate(AggregateRef),
    Scope(ScopeId),
    Device(DeviceId),
    ImportJob(ImportJobId),
    User(PrincipalId),
    SystemResource(SystemResourceId),
}
```

---

# 12. AuditAction

Use stable numeric action identifiers.

Examples:

```text
Student.ProfileUpdated
Invoice.Submitted
Payment.Posted
Scope.Revoked
Import.Completed
Device.Registered
Bootstrap.Repaired
```

Avoid free-form strings as the primary key.

---

# 13. AuditActionId

```rust
pub struct AuditActionId(u32);
```

Maintain application registry similar to operation kind registry.

---

# 14. Audit Actor

Use explicit actor model.

```rust
pub enum AuditActor {
    User {
        principal_id: PrincipalId,
        display_ref: Option<DisplayIdentityRef>,
    },
    Service {
        service_id: ServiceId,
    },
    System {
        component: SystemComponentId,
    },
    Import {
        import_job_id: ImportJobId,
    },
}
```

---

# 15. Never Invent a User Actor

If a scheduled server job performs a mutation:

```text
actor = System/Service
```

not the last logged-in user.

This preserves truthful provenance.

---

# 16. Device Attribution

Where available, audit provenance may include:

```text
DeviceId
client build
client platform
```

but device is not necessarily the actor.

Example:

```text
actor = User A
device = Android device D
```

---

# 17. Request Metadata

Optional audit metadata:

```text
request ID
IP classification/coarse network source
client version
session ID
```

Use sparingly.

Do not turn the audit trail into a surveillance store.

---

# 18. AuditProvenance

Conceptually:

```rust
pub struct AuditProvenance {
    pub operation_id: Option<OperationId>,
    pub authoritative_event_id: Option<EventId>,
    pub handler_id: Option<HandlerId>,
    pub handler_version: Option<HandlerVersion>,
    pub device_id: Option<DeviceId>,
    pub import_job_id: Option<ImportJobId>,
    pub repair_id: Option<RepairId>,
}
```

---

# 19. Audit Outcome

```rust
pub enum AuditOutcome {
    Accepted,
    Rejected,
    Conflict,
    Compensated,
    Revoked,
    Repaired,
}
```

Not every rejected operation needs permanent audit retention.

Policy decides.

---

# 20. Accepted vs Attempted Audit

Separate:

```text
attempt audit
```

from:

```text
state-change audit
```

Examples:

```text
failed login attempt
unauthorized finance update attempt
```

may be security audit events even though no business state changed.

---

# 21. Audit Categories

Recommended categories:

```text
BusinessChange
Security
Administrative
DataAccess
Migration
Repair
Configuration
Authentication
```

---

# 22. BusinessChange

Captures domain-relevant accepted changes.

---

# 23. Security

Examples:

```text
authorization denied
device revoked
scope revoked
credential changed
```

---

# 24. Administrative

Examples:

```text
tenant settings changed
retention policy modified
migration initiated
```

---

# 25. DataAccess

Some regulated systems may require auditing:

```text
sensitive record viewed/exported
```

Do not enable globally by default because read auditing can generate huge volume.

---

# 26. Field-Level Audit

For selected mutable entities, record:

```text
before
after
```

for semantically important fields.

Example:

```text
phone:
    old = ...
    new = ...
```

---

# 27. AuditChange

```rust
pub struct AuditChange {
    pub field: AuditFieldId,
    pub kind: ChangeKind,
    pub before: Option<AuditValue>,
    pub after: Option<AuditValue>,
}
```

---

# 28. Stable Field IDs

Use:

```rust
AuditFieldId(u32)
```

or stable field path IDs.

Do not rely on Rust struct field names forever.

---

# 29. Avoid Auditing Entire Serialized Objects Blindly

Full before/after blobs create:

```text
storage explosion
privacy risk
poor readability
schema migration pain
```

Prefer field-level policy.

---

# 30. Audit Value Policy

Each field can declare:

```rust
pub enum AuditValuePolicy {
    Full,
    Redacted,
    Hashed,
    MetadataOnly,
    Omit,
}
```

---

# 31. Sensitive Fields

Examples:

```text
password hashes
access tokens
private keys
medical notes
sensitive identity documents
```

should usually be:

```text
Omit
or
MetadataOnly
```

---

# 32. Hashed Values

Hashing can prove:

```text
value changed
```

without storing raw value.

Use keyed/appropriate hash policy if dictionary attacks are possible.

---

# 33. Redaction

Examples:

```text
phone: ***1234
email: i***@example.com
```

Useful for operator-facing audit.

---

# 34. Business Audit Policy

Aggregate/profile can declare:

```rust
pub struct AuditPolicy {
    pub category: AuditCategory,
    pub retention: AuditRetentionClass,
    pub include_rejections: bool,
    pub fields: AuditFieldPolicySet,
}
```

---

# 35. Part 11 Integration

Consistency profiles can provide defaults.

Examples:

```text
ImmutableAppendOnly finance:
    audit required

DeviceLocal:
    no server audit

DerivedProjection:
    generally no business audit on rebuild
```

---

# 36. Audit Generation Location

Generate audit records inside the authoritative execution pipeline.

Preferred:

```text
validated operation
↓
domain decision
↓
authoritative transaction
    business mutation
    journal
    operation ledger
    audit event
COMMIT
```

---

# 37. Atomic Audit Rule

For state changes requiring audit:

> **The business mutation and its required audit record must commit atomically.**

Do not commit state and write audit asynchronously later if compliance requires complete evidence.

---

# 38. Optional Audit

Low-value/operational audit can be asynchronous.

But the policy must make the distinction explicit.

---

# 39. RequiredAudit vs BestEffortAudit

```rust
pub enum AuditDurability {
    RequiredAtomic,
    DurableAsync,
    BestEffort,
}
```

---

# 40. RequiredAtomic

Use for:

```text
financial mutation
permission change
approval
security configuration
```

---

# 41. DurableAsync

Possible for:

```text
large secondary human-readable explanation projection
```

derived from durable journal.

---

# 42. BestEffort

Suitable only for operational telemetry, not compliance trail.

---

# 43. Audit Projection Pattern

For scalable systems:

```text
transaction stores minimal canonical audit event
↓
background projector builds searchable/readable audit view
```

This keeps authoritative transaction small.

---

# 44. Canonical Audit Record

Minimal immutable form may include:

```text
subject
action
actor
time
lineage IDs
field-change canonical values
```

---

# 45. Search Projection

Can denormalize:

```text
display names
formatted field labels
search text
```

Rebuildable from canonical audit + referenced identity history if available.

---

# 46. Audit Immutability

Business audit events should generally be append-only.

Corrections are represented by:

```text
new correction event
```

not overwriting historical audit record.

---

# 47. Audit Correction

If audit metadata itself was wrong:

```text
AuditCorrection
```

references original AuditEventId.

---

# 48. Tamper Evidence

For high-assurance deployments, add cryptographic chaining.

Conceptually:

```text
hash_i =
H(
    domain_separator
    previous_hash
    canonical_audit_event_i
)
```

---

# 49. Audit Chain

Per tenant or partition:

```text
AuditChainSequence
PreviousHash
CurrentHash
```

---

# 50. Why Not One Global Chain

One global chain causes:

```text
write contention
cross-tenant coupling
scalability issues
```

Prefer:

```text
per tenant
or per tenant + audit partition
```

---

# 51. Chain Partitioning

Potential partitions:

```text
Business
Security
Administrative
```

or simply one chain per tenant.

Start simple.

---

# 52. Chain Sequence

Define:

```rust
pub struct AuditSequence(u64);
```

Distinct from sync journal Sequence.

---

# 53. Tamper-Evident vs Tamper-Proof

Hash chaining is tamper-evident, not magically tamper-proof.

A privileged DB administrator could rewrite:

```text
event + all later hashes
```

unless anchors exist outside the database.

---

# 54. External Anchoring

Optional high-assurance feature:

```text
periodic chain root
↓
signed
↓
stored in external immutable location
```

Examples:

```text
object storage retention lock
external transparency service
separate security system
```

Part 15 will define cryptographic signing more deeply.

---

# 55. Audit Checkpoint

```rust
pub struct AuditCheckpoint {
    pub tenant_id: TenantId,
    pub sequence: AuditSequence,
    pub root_hash: Digest,
    pub created_at: Timestamp,
}
```

---

# 56. Chain Verification

CLI:

```text
aequora audit verify --tenant ...
```

Checks:

```text
sequence continuity
previous hash
canonical event hash
checkpoint agreement
```

---

# 57. Audit Query Architecture

Need queries by:

```text
entity
aggregate
actor
operation
correlation
time range
action
device
import job
repair
```

---

# 58. Core Indexes

Logical indexes:

```text
tenant + occurred_at
tenant + subject
tenant + actor
tenant + correlation_id
tenant + operation_id
tenant + action
```

---

# 59. Avoid Over-Indexing

Every audit index increases write/storage cost.

Choose indexes from actual investigative workflows.

---

# 60. Entity History

API:

```text
audit.history(EntityRef)
```

returns chronological business changes.

---

# 61. Operation Explanation

API:

```text
audit.explain(OperationId)
```

combines:

```text
operation ledger
lineage
audit event
handler/version
conflict decision
```

---

# 62. Current Value Explanation

High-value feature:

```text
Why is Student.phone = X?
```

Aequora should be able to trace:

```text
current field
↓
last authoritative event touching field
↓
operation
↓
actor/device/correlation
↓
prior value
```

---

# 63. Field Provenance Index

For selected fields, maintain:

```text
entity + field
→ latest AuditEventId/EventId
```

This enables fast explainability.

---

# 64. FieldProvenance

Conceptually:

```rust
pub struct FieldProvenance {
    pub entity: EntityRef,
    pub field: AuditFieldId,
    pub last_audit_event_id: AuditEventId,
    pub last_event_id: EventId,
}
```

---

# 65. Update Atomicity

If used as authoritative explanation metadata:

```text
field provenance pointer
```

should update atomically with business mutation/audit.

---

# 66. Rebuildable Alternative

Could derive field provenance asynchronously from audit stream.

Then it is eventually consistent and must expose its boundary.

---

# 67. Explainability Levels

Define:

```rust
pub enum ExplanationLevel {
    Summary,
    ChangeHistory,
    Decision,
    FullLineage,
}
```

---

# 68. Summary

Example:

```text
Updated by user U on 2026-08-13.
```

---

# 69. ChangeHistory

Shows:

```text
before → after
```

with action sequence.

---

# 70. Decision

Shows:

```text
operation type
validation rule
conflict outcome
handler version
```

---

# 71. FullLineage

Shows:

```text
correlation
causation graph
derived jobs
external result
repair/import references
```

for administrators.

---

# 72. Explanation Must Be Policy-Controlled

Ordinary users should not see:

```text
internal service IDs
security events
other users' hidden details
```

Authorization applies to explanation queries too.

---

# 73. Human-Readable Reason Codes

Domain handler should return stable reason/outcome codes.

Example:

```text
PAYMENT_ACCEPTED
STALE_VERSION_REJECTED
APPROVAL_THRESHOLD_EXCEEDED
```

---

# 74. ReasonCode

```rust
pub struct ReasonCode(u32);
```

Map to localized UI text outside core.

---

# 75. Do Not Store Only Human Strings

Strings change with:

```text
language
copy edits
localization
```

Store stable reason code + structured parameters.

---

# 76. Explanation Parameters

Example:

```text
reason = LIMIT_EXCEEDED
params = {
    requested = 1200,
    limit = 1000
}
```

Audit value policy controls sensitivity.

---

# 77. Part 12 Integration

Deterministic execution can persist:

```text
handler version
policy version
execution plan digest
reason code
```

Audit explainability references them.

---

# 78. Explain Rejection

For rejected operation:

```text
authorization rejected
validation rejected
stale version
conflict
```

Some rejection records may be retained in security or support audit.

---

# 79. Conflict Explanation

A conflict record should link:

```text
client OperationId
base version
current authoritative version
conflicting fields
relevant authoritative EventIds
```

---

# 80. Manual Resolution Audit

When user resolves conflict:

```text
new resolution operation
```

Audit records:

```text
who resolved
what value selected
which conflict was resolved
```

---

# 81. Repair Provenance

Part 03 repair is not a business mutation.

Audit category:

```text
Repair
```

Possible record:

```text
Device D repaired local replica for partition P from server authority.
```

Do not pretend user changed business data.

---

# 82. Bootstrap Audit

Routine bootstrap need not generate business audit per record.

Operational/admin audit can record:

```text
device bootstrapped scope S
snapshot ID
boundary
```

if useful.

---

# 83. Import Provenance

Part 09 imports should record:

```text
source system
ImportJobId
mapping version
initiating actor
source record key where needed
```

---

# 84. Imported Entity Explanation

For current imported state:

```text
origin = Legacy ERP
import job = 44
source record = student/991
```

This is valuable during migration support.

---

# 85. Scope Revocation Audit

Part 07 security-sensitive.

Record:

```text
who revoked
which principal/device/scope
when
reason
```

---

# 86. Live Presence

Part 08 presence should generally not enter durable audit.

Presence is ephemeral.

Only explicit security/session events may be audited.

---

# 87. Scheduler

Part 06 scheduling decisions should not create business audit noise.

Operational tracing handles them.

---

# 88. Multi-Process Leader Changes

Part 05 local leader election is operational.

Do not create central business audit unless troubleshooting diagnostics are explicitly collected.

---

# 89. Sync Retry

Retries are not new audit actions.

Same OperationId retains one logical business action.

---

# 90. Audit Deduplication

Audit generation should be tied to authoritative effect.

Duplicate transport retry must not create duplicate business audit records.

---

# 91. AuditId Derivation

For one deterministic audit event per operation/action, derive:

```text
AuditEventId from OperationId + AuditActionId
```

or use operation ledger uniqueness constraints.

---

# 92. Multi-Audit Operation

One operation may produce several audit entries.

Use stable semantic suffix:

```text
OperationId + action + ordinal/key
```

where deterministic.

---

# 93. Required Audit Transaction

Server authoritative invariant extension:

```text
business mutation
+
version
+
journal
+
operation ledger
+
required audit events
```

commit atomically.

---

# 94. Audit Transaction Failure

If required audit insert fails:

```text
business transaction fails
```

This is intentional.

---

# 95. Audit Storage

Could live in:

```text
same authoritative PostgreSQL
```

initially.

This simplifies atomicity.

---

# 96. Separate Audit Database

If later required for retention/security:

```text
canonical audit event written transactionally to authoritative outbox
↓
durable consumer copies to audit store
```

But required-compliance semantics must be carefully specified.

---

# 97. Initial Recommendation

Keep canonical required audit rows in the same authoritative DB transaction.

Build external archive/search projection asynchronously.

---

# 98. Audit Archive

Long-term events can be exported to:

```text
immutable object storage
columnar archive
security archive
```

with checksums/checkpoints.

---

# 99. Hot vs Cold Audit

```text
hot:
    recent searchable DB

cold:
    immutable archive
```

Retention can migrate old partitions.

---

# 100. Time Partitioning

PostgreSQL audit tables may be partitioned by:

```text
tenant/time
or time
```

depending on scale.

Do not prematurely partition small deployments.

---

# 101. Retention Classes

Define:

```rust
pub enum AuditRetentionClass {
    Short,
    Standard,
    LongTerm,
    LegalHoldEligible,
    PermanentByPolicy,
}
```

---

# 102. Retention Policy

Application/tenant maps classes to durations.

Do not hardcode legal durations in Aequora core.

---

# 103. Legal Hold

Part 14 will formalize legal hold.

Audit subsystem must support:

```text
do not purge held records
```

---

# 104. Erasure Tension

Privacy erasure may conflict with audit retention.

Use policies such as:

```text
pseudonymize subject identity
retain required transaction evidence
remove nonessential PII
```

Part 14 handles this deeply.

---

# 105. Actor Display Name Changes

Do not rely on current display name to explain historical audit.

Store:

```text
stable PrincipalId
```

Optionally capture historical display label if policy allows.

---

# 106. Historical Labels

If stored, treat as convenience.

PrincipalId remains canonical actor identity.

---

# 107. Deleted User

Audit must still preserve that:

```text
former principal P performed action
```

even if user account is deleted.

Use pseudonymous/stable archival identity.

---

# 108. Audit Search Authorization

A tenant admin may search tenant audit.

Ordinary user may only see:

```text
history for entities they can access
```

Security auditor may have broader role.

---

# 109. Audit Access Itself

High-security deployments may audit:

```text
who exported audit logs
who viewed sensitive audit records
```

This is recursive but manageable as security audit events.

---

# 110. Export

Audit export should include:

```text
manifest
query criteria
exporter principal
time range
record count
hashes
```

---

# 111. AuditExportId

```rust
pub struct AuditExportId(Uuid);
```

Record export in security/admin audit.

---

# 112. Canonical Export Format

Recommended:

```text
manifest.ron
events.postcard.zst
checksums
optional signature
```

---

# 113. Human Export

Optional:

```text
CSV
JSON
PDF report
```

generated from canonical events.

These are presentation formats, not canonical evidence.

---

# 114. Time Semantics

Store:

```text
authoritative occurred_at
```

from server execution context.

Optional:

```text
client observed_at/client HLC
```

for diagnostics.

---

# 115. Do Not Trust Client Time for Audit Ordering

Authoritative audit ordering comes from:

```text
AuditSequence
server time
```

---

# 116. Audit Ordering

Per-tenant audit sequence gives deterministic order.

Events with same wall timestamp remain ordered.

---

# 117. Cross-Tenant Ordering

Not needed.

Avoid global synchronization bottleneck.

---

# 118. Audit Chain Atomicity

If tamper-evident chain enabled:

```text
allocate next AuditSequence
previous hash
new hash
audit row
```

must commit transactionally.

---

# 119. Chain Contention

One per-tenant chain can serialize audit inserts for extremely busy tenants.

If this becomes bottleneck:

```text
partition chain
```

with explicit partition ID.

---

# 120. Chain Partition Example

```text
Finance
Security
General
```

Each independently chained.

---

# 121. Merkle Batch Anchoring

Future optimization:

```text
many audit events
↓
Merkle root per time window
↓
external anchor
```

reduces signature cost.

---

# 122. Audit Integrity Verification

Verify:

```text
event canonical hash
sequence
chain predecessor
checkpoint
archive checksum
```

---

# 123. Audit and Database Backup

Backup/PITR restores audit with business state.

After restore:

```text
audit chain/checkpoint verification
```

should run.

---

# 124. Authority Epoch

Part 16 failover/restore may start new audit chain epoch.

Define:

```rust
pub struct AuditEpoch(u64);
```

if timeline discontinuity is possible.

---

# 125. Epoch Rule

Never silently continue a chain across a timeline fork if continuity cannot be proven.

Start:

```text
new AuditEpoch
```

and record transition.

---

# 126. Audit Schema Evolution

Canonical AuditEvent schema is versioned.

Define:

```rust
pub struct AuditSchemaVersion(u16);
```

---

# 127. Upcasters

Old audit records remain readable via:

```text
audit upcasters
```

Do not rewrite billions of historical rows just for presentation changes.

---

# 128. Stable Canonicalization

Tamper hashing requires stable canonical encoding independent of application display schema.

---

# 129. Postcard Use

Postcard can store canonical audit envelope if schema/version is explicit.

For hashing, use a dedicated canonical audit encoder.

---

# 130. Explanation Projection Evolution

Human-readable labels can evolve independently.

---

# 131. Decision Provenance

For each accepted operation, optionally record:

```text
validation policy version
conflict policy
consistency profile
handler version
reason code
```

---

# 132. Why Was It Accepted?

Explanation example:

```text
Accepted because:
- actor had Finance.PostPayment permission
- invoice was Open
- expected aggregate version matched 12
- payment amount was within remaining balance
- handler Finance.PostPayment v3 executed
```

Do not expose internal sensitive policy details to unauthorized users.

---

# 133. Why Was It Rejected?

Example:

```text
Rejected:
reason = STALE_VERSION
expected = 12
actual = 14
```

---

# 134. Rule Trace

For high-assurance domains, handler may produce structured:

```rust
DecisionTrace
```

with rule IDs.

---

# 135. DecisionRuleId

```rust
pub struct DecisionRuleId(u32);
```

Examples:

```text
FIN-PAY-OPEN-INVOICE
FIN-PAY-AMOUNT-LIMIT
ATTENDANCE-CLASS-ASSIGNMENT
```

---

# 136. Avoid Capturing Every Branch Automatically

Automatic full execution traces are:

```text
large
fragile
implementation-coupled
```

Prefer explicit high-level decision rules.

---

# 137. Decision Trace Example

```text
rule FIN-001: passed
rule FIN-004: passed
rule FIN-009: failed
```

with structured parameters.

---

# 138. Explainability Stability

Rule IDs remain stable even if internal code refactors.

---

# 139. Rule Registry

Maintain documentation:

```text
RuleId
name
description
module
version
```

---

# 140. Client-Facing Explainability

SDK could expose:

```rust
aequora.explain(entity, field)
```

or app-specific service.

Response must already be authorization-filtered.

---

# 141. Offline Explainability

Client can retain minimal provenance pointers:

```text
last EventId
last AuditEventId
```

for synchronized fields.

Full explanation may require server access.

---

# 142. Local Audit Cache

Optional:

```text
recent audit events for active scope
```

can be synchronized if product needs offline history.

This should be a separate projection/scope, not automatic global replication.

---

# 143. Audit Scope

Part 07 can define:

```text
AuditHistory scope
```

with strict permissions and retention.

---

# 144. Do Not Sync Entire Audit Trail to Every Client

Audit can contain sensitive cross-user information.

---

# 145. Audit Event Projection

If clients receive audit:

```text
server projects/redacts fields
```

according to scope.

---

# 146. Imports

For Part 09 seed-mode imports, avoid generating one huge synthetic user-change audit record per original row unless audit requirements need it.

Instead:

```text
ImportedBaseline
```

plus source provenance.

For legally significant historical data, import historical audit/events explicitly.

---

# 147. Initial Baseline Provenance

Entity can carry:

```text
Origin::Imported {
    job_id,
    source_system,
    source_key
}
```

without pretending it was live-created under Aequora.

---

# 148. Repair Explanation

If user asks:

```text
Why did this local value suddenly revert?
```

Aequora can explain:

```text
local replica divergence detected
server authoritative version 14 restored
pending operation O retained/rebased
repair ID R
```

This is operational explainability, not business change.

---

# 149. Scope Eviction Explanation

If local data disappears due permission/scope contraction:

```text
reason = ScopeEvicted
```

not:

```text
entity deleted
```

Critical for user trust.

---

# 150. Tombstone Explanation

If actual server deletion:

```text
reason = EntityDeleted
```

with audit event if permitted.

---

# 151. Audit Storage Model

Logical tables:

```text
aequora_audit_event
aequora_audit_change
aequora_audit_chain
aequora_field_provenance
aequora_audit_checkpoint
```

---

# 152. Minimal Audit Event Columns

```text
audit_event_id
tenant_id
audit_epoch
audit_sequence
category
action_id
subject_kind
subject_id
actor_kind
actor_id
occurred_at
operation_id
event_id
correlation_id
reason_code
previous_hash
event_hash
schema_version
```

---

# 153. Change Table

```text
audit_event_id
field_id
change_kind
before_value
after_value
value_policy
```

Could be embedded binary payload initially if query needs are low.

---

# 154. Binary vs Relational Change Storage

Relational:

```text
easy field queries
more rows
```

Binary canonical payload:

```text
compact
harder querying
```

Recommended:

```text
canonical binary payload
+
indexed summary columns
```

until field-level query requirements justify normalization.

---

# 155. Performance

Required audit adds write cost.

Keep canonical event compact.

Avoid:

```text
large JSON blobs
unbounded stack traces
full entity snapshots
```

---

# 156. Asynchronous Search Index

Build richer audit search asynchronously from canonical rows.

---

# 157. Audit Backpressure

If required audit storage is unavailable:

```text
fail required audited mutations
```

Do not silently continue.

If only search projection is down:

```text
business mutation continues
```

because canonical audit remains durable.

---

# 158. Metrics

```text
audit_events_total
audit_required_write_failure_total
audit_projection_lag
audit_verify_failure_total
audit_export_total
```

---

# 159. Logs

Structured events:

```text
audit_event_committed
audit_chain_verify_failed
audit_projection_lagging
audit_export_created
```

Do not duplicate sensitive change values into logs.

---

# 160. Alerting

Alert on:

```text
audit chain failure
required audit transaction failures
archive failure
projection lag beyond SLO
unexpected audit volume spike
```

---

# 161. CLI

Suggested:

```text
aequora audit history <entity>
aequora audit explain <operation-id>
aequora audit verify
aequora audit export
aequora audit checkpoint
```

---

# 162. Admin API

Queries should support:

```text
time range
actor
subject
action
correlation
outcome
```

with pagination.

---

# 163. Cursor-Based Audit Pagination

Use:

```text
AuditSequence
```

not offset pagination for large audit logs.

---

# 164. Audit Read Model

Return structured data.

Formatting/localization belongs in UI.

---

# 165. Security

Audit endpoints are sensitive.

Require:

```text
explicit audit permissions
tenant scope
rate limits
export controls
```

---

# 166. Query Abuse

Do not allow arbitrary unbounded audit scans.

Require:

```text
bounded time range
pagination
result limits
```

---

# 167. Audit Export Rate Limit

Large exports should become durable admin jobs rather than blocking HTTP.

Part 23 background-work architecture will support this.

---

# 168. Correctness Invariants

Add:

## AEQ-INV-AUD001

```text
Every mutation configured as RequiredAtomic audit commits its required audit event in the same authoritative transaction.
```

## AEQ-INV-AUD002

```text
Transport retries for one OperationId do not produce duplicate logical business audit effects.
```

## AEQ-INV-AUD003

```text
Audit actor attribution never invents a user principal for system/service work.
```

## AEQ-INV-AUD004

```text
Audit records are append-only; corrections reference prior records rather than mutating history.
```

## AEQ-INV-AUD005

```text
Sensitive values follow their configured audit-value policy and are never implicitly stored in full.
```

## AEQ-INV-AUD006

```text
Audit explanation never bypasses current authorization policy.
```

---

# 169. Tamper Invariants

## AEQ-INV-AUD007

```text
When hash chaining is enabled, each audit event references the prior valid chain hash for its audit partition/epoch.
```

## AEQ-INV-AUD008

```text
A chain discontinuity or hash mismatch is surfaced as an integrity failure and never silently repaired.
```

---

# 170. Explainability Invariant

## AEQ-INV-AUD009

```text
A field provenance pointer, when declared authoritative, references the audit/event record corresponding to the latest committed mutation of that field.
```

---

# 171. Property Tests

Generate:

```text
mutations
retries
system jobs
imports
repairs
conflicts
```

Assert audit count, attribution, and linkage.

---

# 172. Tamper Test

Modify historical audit payload.

Expected:

```text
chain verification fails
```

---

# 173. Retry Test

Send same OperationId repeatedly.

Expected:

```text
one authoritative business audit effect
```

---

# 174. System Actor Test

Background job changes state.

Expected:

```text
actor = System/Service
```

never user.

---

# 175. Sensitive Field Test

Change secret field.

Expected:

```text
audit record contains no prohibited raw value
```

---

# 176. Explainability Test

Mutate same field several times.

Expected:

```text
current field provenance points to latest committed change
history remains complete
```

---

# 177. Scope Security Test

User without audit permission asks explanation for inaccessible entity.

Expected:

```text
denied
```

---

# 178. Restore Test

Restore DB from backup/PITR.

Run:

```text
audit verify
```

and check chain/epoch policy.

---

# 179. Migration Test

Import old data.

Expected:

```text
origin provenance points to ImportJob/source
```

without false live-user attribution.

---

# 180. Recommended Modules

```text
aequora-audit/
├── event.rs
├── actor.rs
├── subject.rs
├── action.rs
├── change.rs
├── policy.rs
├── reason.rs
├── chain.rs
├── checkpoint.rs
├── query.rs
├── explain.rs
└── export.rs
```

Server integration:

```text
aequora-server/
└── audit/
    ├── writer.rs
    ├── projector.rs
    ├── verifier.rs
    └── authorization.rs
```

---

# 181. Domain Integration API

Example:

```rust
ctx.audit()
    .action(AuditAction::StudentProfileUpdated)
    .subject(student_id)
    .change(StudentField::Phone, old_phone, new_phone)
    .reason(ReasonCode::USER_UPDATE);
```

Executor adds:

```text
actor
device
operation
correlation
handler version
time
```

automatically.

---

# 182. Avoid Manual Metadata Duplication

Domain code should declare:

```text
business action
subject
semantic changes
reason
```

Infrastructure should attach:

```text
tenant
actor
device
OperationId
EventId
CorrelationId
time
```

---

# 183. Audit Builder

The builder should enforce required fields at compile time or startup where practical.

---

# 184. Profile-Driven Audit

Example:

```rust
registry
    .aggregate::<JournalEntry>()
    .audit(AuditPolicy::required_atomic_long_term());
```

Then any mutation lacking required audit declaration fails validation/startup.

---

# 185. Audit Coverage Report

CLI/CI can generate:

```text
operation
aggregate
audit durability
action ID
field policy
retention class
```

This is useful for security/compliance review.

---

# 186. Audit Coverage Gate

Production CI can fail if:

```text
finance mutation has no required audit
security configuration operation has no audit action
```

---

# 187. Explanation Architecture

High-level query:

```text
Why is value X?
```

Flow:

```text
field/entity
↓
FieldProvenance
↓
AuditEvent
↓
Operation/Event
↓
Actor + ReasonCode
↓
optional Causation Graph
↓
authorized explanation
```

---

# 188. Full Causal Explanation

For complex workflow:

```text
Payment confirmed
↓ caused by
GatewayResult
↓ caused by
PaymentIntent
↓ caused by
User Checkout
```

Part 02 supplies graph structure.

Part 13 supplies human-readable audit semantics.

---

# 189. Explain Imported Data

```text
Current value came from import job 44,
source system Legacy SIS,
source record student/991,
mapping version 3.
```

---

# 190. Explain Repaired Data

```text
Current local replica was restored from authoritative server state
after integrity mismatch,
RepairId R,
server version 14.
```

---

# 191. Explain Scope Disappearance

```text
Record is no longer stored locally because scope S was revoked/contracted.
The server record was not deleted.
```

This distinction should be represented structurally.

---

# 192. Explain Conflict

```text
Your change was not applied automatically because
the authoritative version changed from 12 to 14
and the configured profile requires manual resolution.
```

---

# 193. Explain Server-Generated Value

Example:

```text
Invoice total = 1,240
```

Trace:

```text
Invoice recalculation event
↓
fee policy version 7
↓
handler Invoice.Calculate v4
```

---

# 194. Completion Criteria

Part 13 is complete when:

```text
[ ] sync journal/ledger/audit/log separation documented
[ ] AuditEvent model defined
[ ] actor/subject/action/reason types defined
[ ] required-vs-optional audit durability defined
[ ] field-level audit policy defined
[ ] sensitive-value policy defined
[ ] atomic audit integration defined
[ ] tamper-evident chaining defined
[ ] external checkpoint/anchor path defined
[ ] field provenance/explainability defined
[ ] import/repair/scope provenance integrated
[ ] audit query/index model defined
[ ] retention/archive hooks defined
[ ] authorization/export model defined
[ ] property/tamper/security tests defined
[ ] audit coverage CI defined
```

---

# 195. Final Architecture

```text
                    VALIDATED OPERATION
                             │
                             ▼
                      Domain Decision
                             │
             ┌───────────────┼────────────────┐
             ▼               ▼                ▼
      Business Mutation   Journal Event   Audit Declaration
             │               │                │
             └───────────────┼────────────────┘
                             ▼
                 AUTHORITATIVE TRANSACTION
                             │
          mutation + journal + ledger + required audit
                             │
                             ▼
                         COMMIT
                             │
              ┌──────────────┼───────────────┐
              ▼              ▼               ▼
          Sync Clients   Audit Search    Chain/Archive
                             │
                             ▼
                       Explainability
                             │
                    ┌────────┼─────────┐
                    ▼        ▼         ▼
                   Who      What      Why
                    │        │         │
                    └────────┼─────────┘
                             ▼
                    Causality / Provenance
```

The architectural principle is:

> **Aequora should be able to prove not only that synchronized state changed, but also who or what caused the change, what business action occurred, which values were affected, why the decision was made, and how the current value can be traced back to authoritative evidence.**

That turns Aequora from a synchronization mechanism into an auditable enterprise data platform whose behavior can be investigated, explained, and verified without relying on transient logs or developer intuition.
