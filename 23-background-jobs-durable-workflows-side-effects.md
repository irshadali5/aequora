# Aequora Sync — Part 23

# Background Jobs, Durable Workflows, and Side-Effect Engine Architecture

## 1. Purpose

Aequora already separates:

```text
authoritative domain execution
from
external side effects
```

That separation is essential for:

```text
deterministic replay
safe retries
transaction atomicity
failure recovery
```

However, production systems need many kinds of work that cannot or should not finish inside an HTTP request or database transaction.

Examples:

```text
send email
send SMS
invoke webhook
capture payment
generate document
build snapshot
run export
perform anti-entropy repair
run retention purge
process import batch
rebuild projection
rotate keys
reconcile external provider state
```

These tasks need a durable workflow engine.

The central rule is:

> **Required asynchronous work must be represented durably before execution, claimed with fencing, retried idempotently, and observable until it reaches a terminal state.**

---

# 2. Goals

The subsystem should provide:

```text
durable jobs
durable timers
side-effect outbox
worker leases
fencing
retry policies
idempotency
dependency handling
workflow checkpoints
provider reconciliation
compensation
cancellation
dead-letter/quarantine
operator visibility
tenant fairness
```

---

# 3. Non-Goals

Aequora should not initially become:

```text
a generic BPM suite
a full Temporal replacement
a visual workflow designer
a distributed actor framework
```

The goal is a focused, typed, durable work engine that supports Aequora and application-owned workflows.

---

# 4. Durable Work Principle

In-memory queue:

```text
scheduling optimization
```

Durable job record:

```text
source of truth
```

If the process crashes, required work must still exist.

---

# 5. JobId

Define:

```rust
pub struct JobId(Uuid);
```

UUIDv7 recommended.

---

# 6. JobKind

Stable numeric registry ID:

```rust
pub struct JobKind(u32);
```

Examples:

```text
SendEmail
SendWebhook
PaymentCapture
SnapshotBuild
AuditExport
RetentionPurge
ProjectionRebuild
```

---

# 7. Job Record

Conceptually:

```rust
pub struct JobRecord {
    pub job_id: JobId,
    pub tenant_id: TenantId,
    pub kind: JobKind,
    pub state: JobState,
    pub priority: WorkClass,
    pub payload: JobPayloadRef,
    pub checkpoint: Option<JobCheckpoint>,
    pub attempt_count: u32,
    pub next_run_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
```

---

# 8. JobState

Recommended:

```rust
pub enum JobState {
    Pending,
    Ready,
    Running,
    Waiting,
    RetryScheduled,
    Completed,
    Failed,
    Canceled,
    Quarantined,
}
```

---

# 9. Pending vs Ready

`Pending`:

```text
dependencies/timer not satisfied
```

`Ready`:

```text
eligible to claim now
```

---

# 10. Running

Job is currently claimed by one worker lease.

---

# 11. Waiting

Used when workflow is waiting for:

```text
external callback
time
another job
manual approval
```

---

# 12. RetryScheduled

Retryable failure occurred.

`next_run_at` determines next eligibility.

---

# 13. Terminal States

Terminal:

```text
Completed
Failed
Canceled
Quarantined
```

---

# 14. Job Payload

Prefer typed payload stored as:

```text
Postcard bytes
+
schema version
+
digest
```

---

# 15. JobPayloadSchemaVersion

Define:

```rust
pub struct JobPayloadSchemaVersion(u16);
```

---

# 16. Payload Size

Large files/data should be referenced by:

```text
ObjectRef
BlobRef
ExportRef
```

not embedded in job row.

---

# 17. Job Registry

Application/server registers handlers:

```rust
JobRegistry::new()
    .register::<SendEmailJob>(handler)
    .register::<BuildSnapshotJob>(handler);
```

---

# 18. JobDescriptor

Contains:

```text
JobKind
payload schema
retry policy
timeout
concurrency class
idempotency requirements
```

---

# 19. WorkerId

```rust
pub struct WorkerId(Uuid);
```

Operational identity.

---

# 20. Job Lease

Logical:

```rust
pub struct JobLease {
    pub job_id: JobId,
    pub worker_id: WorkerId,
    pub fencing_token: FencingToken,
    pub expires_at: Timestamp,
}
```

---

# 21. Claim

Worker claims ready job transactionally.

Only one active lease may own a job.

---

# 22. Lease Expiry

If worker dies:

```text
lease expires
job becomes reclaimable
```

---

# 23. Fencing

Every checkpoint/terminal update includes current fencing token.

Old worker cannot commit after newer worker reclaimed job.

---

# 24. Why Lease Alone Is Insufficient

Race:

```text
worker A slow
lease expires
worker B claims
worker A wakes
```

Without fencing, A may overwrite B.

---

# 25. Claim Token

Monotonic per job:

```text
1
2
3
```

Each new claim increments.

---

# 26. Job Attempt

Logical:

```text
aequora_job_attempt
```

Fields:

```text
job_id
attempt_no
worker_id
started_at
finished_at
outcome
error_code
```

Detailed logs may remain separate.

---

# 27. Attempt Record Purpose

Useful for:

```text
support
retry analysis
provider incidents
```

Retention may be shorter than job record.

---

# 28. Worker Polling

Workers query:

```text
Ready / RetryScheduled where next_run_at <= now
```

with bounded batch.

---

# 29. Database Claim Pattern

Relational adapter can use:

```text
SELECT ... FOR UPDATE SKIP LOCKED
```

or equivalent.

Core does not require SQL-specific mechanism.

---

# 30. Poll Interval

Use:

```text
notification/wakeup
+
fallback polling
```

where possible.

---

# 31. Job Wake Hint

Database insert can notify worker.

Notification is best-effort.

Durable job table remains truth.

---

# 32. No Lost Wakeup Problem

Even if notification lost:

```text
polling finds job
```

---

# 33. Job Priority

Reuse Part 06/18 work classes.

Server derives priority.

---

# 34. Tenant Fairness

Workers should not drain one tenant indefinitely.

Use:

```text
tenant-aware claim
weighted queues
per-tenant concurrency
```

---

# 35. Concurrency Class

Examples:

```text
DatabaseLight
CpuHeavy
ExternalHttp
SnapshotBuild
PaymentProvider
EmailProvider
```

---

# 36. Bulkhead

Each class has separate concurrency permits.

---

# 37. Provider Bulkhead

Payment outage should not block:

```text
email
snapshot
repair
```

---

# 38. RetryPolicy

Conceptually:

```rust
pub struct RetryPolicy {
    pub max_attempts: Option<u32>,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter: JitterPolicy,
    pub retryable_errors: ErrorClassSet,
}
```

---

# 39. Infinite Retry

Some critical infrastructure work may retry indefinitely.

But it must:

```text
back off
remain observable
not consume hot loop CPU
```

---

# 40. Retryable vs Terminal

Handler returns typed classification:

```rust
pub enum JobRunOutcome {
    Completed,
    Retryable(JobRetry),
    TerminalFailure(JobFailure),
    Waiting(WaitCondition),
}
```

---

# 41. Error Taxonomy

Separate:

```text
InfrastructureTransient
ProviderTransient
RateLimited
InvalidPayload
AuthorizationRevoked
ProviderPermanent
InvariantViolation
```

---

# 42. Retry-After

External providers may supply retry delay.

Respect within configured max.

---

# 43. Jitter

Mandatory for fleet/provider retry.

---

# 44. Timeout

Every external attempt needs bounded timeout.

---

# 45. Cancellation

Job may be canceled only if:

```text
business semantics allow
```

---

# 46. Cancellation Request

State transition:

```text
Running
→ CancelRequested
```

can be represented separately if needed.

---

# 47. Cooperative Cancellation

Handler periodically checks cancellation.

---

# 48. Irreversible Side Effect

If payment already captured:

```text
cancel cannot undo
```

Need compensation/refund operation.

---

# 49. Compensation

Durable workflow uses explicit compensation job/operation.

Do not pretend rollback can undo external world.

---

# 50. Saga-Like Workflow

For multi-step business workflow:

```text
Reserve
↓
Charge
↓
IssueReceipt
```

each step durable.

Failure can schedule compensating steps.

---

# 51. No Distributed Transaction

Aequora never attempts one ACID transaction across:

```text
Postgres
payment provider
email provider
object storage
```

---

# 52. SideEffectIntent

Part 12 recommended pattern.

Define:

```rust
pub struct SideEffectIntent {
    pub intent_id: SideEffectIntentId,
    pub operation_id: OperationId,
    pub tenant_id: TenantId,
    pub kind: SideEffectKind,
    pub idempotency_key: ExternalIdempotencyKey,
    pub payload: SideEffectPayload,
}
```

---

# 53. IntentId

```rust
pub struct SideEffectIntentId(Uuid);
```

Can be deterministically derived from:

```text
OperationId + semantic purpose
```

where appropriate.

---

# 54. Authoritative Transaction

Critical pattern:

```text
business mutation
+
journal
+
ledger
+
required audit
+
SideEffectIntent
COMMIT
```

---

# 55. Intent-to-Job Projection

After commit:

```text
SideEffectIntent
↓
durable worker/job
```

Could use same row as job or separate semantic intent + execution state.

---

# 56. Recommended Separation

Keep:

```text
SideEffectIntent = domain decision record
Job = execution machinery
```

This preserves auditability.

---

# 57. SideEffectExecution

Logical record:

```text
intent_id
job_id
provider
state
provider_reference
```

---

# 58. External Idempotency Key

Use stable key when provider supports it.

Recommended source:

```text
OperationId
or
SideEffectIntentId
```

---

# 59. Payment Provider

Always use provider idempotency feature if available.

---

# 60. Webhook

Include event/delivery ID so receiver can deduplicate.

---

# 61. Email

Provider may not guarantee idempotency.

Aequora can prevent duplicate local scheduling, but provider/network ambiguity may remain.

---

# 62. Ambiguous External Result

Example:

```text
HTTP timeout after request body sent
```

Provider may have completed action.

State:

```text
Ambiguous
```

should not be treated as ordinary retry blindly.

---

# 63. ExternalOutcome

```rust
pub enum ExternalOutcome {
    ConfirmedSuccess,
    ConfirmedFailure,
    RetryableFailure,
    Ambiguous,
}
```

---

# 64. Ambiguous Policy

Each side-effect kind declares:

```rust
pub enum AmbiguousRecoveryPolicy {
    QueryProvider,
    RetryWithIdempotencyKey,
    ManualReview,
    SuppressDuplicateRisk,
}
```

---

# 65. QueryProvider

Preferred for:

```text
payments
orders
reservations
```

if API supports lookup.

---

# 66. RetryWithIdempotencyKey

Safe if provider guarantees idempotent replay.

---

# 67. ManualReview

Use for high-risk ambiguous actions.

---

# 68. SuppressDuplicateRisk

For effects where duplicate is worse than missing, system may not retry automatically.

Document carefully.

---

# 69. Provider Reconciliation Job

Dedicated job can query:

```text
idempotency key
provider reference
```

and record authoritative result.

---

# 70. External Result Becomes Operation

When result affects domain state:

```text
provider result
↓
new authoritative operation
↓
domain handler
```

---

# 71. Never Mutate Domain Directly From Worker

Worker should not bypass domain handlers.

Example:

```text
payment worker
```

must submit:

```text
RecordPaymentProviderResult
```

not directly update invoice table.

---

# 72. Why

Preserves:

```text
validation
audit
journal
lineage
replay
```

---

# 73. System Principal

Worker-generated operation actor:

```text
Service/System
```

with causation from original intent.

---

# 74. Correlation

Maintain same CorrelationId across workflow where useful.

---

# 75. Causation

```text
provider-result operation
caused_by
side-effect intent / original operation
```

---

# 76. Durable Timer

Need delayed work:

```text
reminder tomorrow
retry in 10m
subscription expiry
```

Represent as job with:

```text
next_run_at
```

---

# 77. Do Not Use One Tokio Sleep for Long Delay

Process restart loses it.

Persist timer.

---

# 78. Timer Precision

Background job engine is not hard real-time.

Suitable for:

```text
seconds/minutes/hours
```

not microsecond scheduling.

---

# 79. Clock

Use server authoritative time.

Part 12 deterministic domain semantics remain separate.

---

# 80. Scheduled Domain Action

At timer fire:

```text
worker submits authoritative operation
```

rather than editing state directly.

---

# 81. Example: Fee Reminder

```text
ReminderDue job
↓
SendEmailIntent
```

No business state mutation required unless product records send status.

---

# 82. Example: Subscription Expiry

Timer:

```text
ExpireSubscription job
↓
system operation ExpireSubscription
↓
authoritative handler validates still eligible
```

---

# 83. Revalidate at Execution Time

Delayed job must not assume old state still valid.

Domain operation checks current state.

---

# 84. Job Dependencies

Some jobs require others completed.

Represent:

```text
job_dependency
```

---

# 85. Dependency DAG

Must be bounded and acyclic.

---

# 86. Job Dependency State

Job becomes Ready only when required predecessors satisfy condition.

---

# 87. Dependency Conditions

Examples:

```text
Completed
CompletedOrSkipped
Terminal
```

---

# 88. WorkflowId

For multi-job workflow:

```rust
pub struct WorkflowId(Uuid);
```

---

# 89. Workflow Record

```rust
pub struct WorkflowRecord {
    pub workflow_id: WorkflowId,
    pub kind: WorkflowKind,
    pub state: WorkflowState,
    pub correlation_id: CorrelationId,
}
```

---

# 90. Workflow State

```text
Running
Waiting
Compensating
Completed
Failed
Canceled
ManualIntervention
```

---

# 91. Workflow Checkpoint

Persist:

```text
completed steps
current phase
compensation state
```

---

# 92. Typed Workflow

Prefer compile-time/application-defined steps.

Avoid arbitrary dynamic DSL initially.

---

# 93. Workflow Handler

Conceptual:

```rust
pub trait WorkflowDefinition {
    fn next(
        &self,
        state: &WorkflowStateData,
        event: WorkflowEvent,
    ) -> WorkflowTransition;
}
```

---

# 94. Determinism

Workflow transition logic should be deterministic from:

```text
persisted workflow state
incoming durable event
versioned policy
```

---

# 95. External Nondeterminism

Captured as:

```text
provider result event
```

not hidden inside transition.

---

# 96. Workflow Version

Define:

```rust
pub struct WorkflowVersion(u32);
```

---

# 97. Long-Lived Workflows

Old workflow instance may outlive software release.

Need versioned handler support/migration.

Part 21 compatibility principles apply.

---

# 98. Workflow Migration

Options:

```text
continue old version
explicit state migration
terminate/manual intervention
```

---

# 99. Do Not Auto-Migrate Blindly

Business semantics may change.

---

# 100. Job Schema Upgrade

Possibly-running/retry jobs retain payload semantics.

Use upcasters where safe.

---

# 101. Retry-Only Job Version

Similar to Part 21 operation schemas.

---

# 102. Job Idempotency

A job attempt may execute more than once.

Handler must be:

```text
idempotent
or
protected by external idempotency/reconciliation
```

---

# 103. Exactly-Once Myth

Distributed external side effects cannot generally guarantee exactly-once.

Aequora provides:

```text
durable intent
at-least-once execution
idempotency key
reconciliation
```

---

# 104. Internal Job Effect

For DB-only job, transaction + unique JobId may achieve exactly-once logical effect.

---

# 105. JobEffectLedger

Optional for jobs that need explicit dedup.

But many can use job terminal state + transactional write.

---

# 106. Worker Crash Before Effect

Retry safe.

---

# 107. Worker Crash After Effect Before Checkpoint

Ambiguous.

External idempotency/reconciliation required.

---

# 108. Worker Crash After Checkpoint

Job terminal; no retry.

---

# 109. Checkpoint Frequency

Long jobs should checkpoint bounded progress.

Examples:

```text
every 1000 rows
every snapshot chunk
every export partition
```

---

# 110. Chunked Jobs

Pattern:

```text
claim
process bounded chunk
commit checkpoint
release/yield
```

This improves fairness.

---

# 111. Cooperative Yield

Bulk job should not monopolize worker for hours.

---

# 112. Lease Renewal

Long attempt renews lease periodically.

---

# 113. Renewal Failure

If worker cannot renew:

```text
stop before next irreversible unit
```

where possible.

---

# 114. Fencing on External Calls

Cannot fence external provider with DB token.

Use provider idempotency key.

---

# 115. Fencing on DB Checkpoint

Required.

---

# 116. Dead Letter

Repeated terminal-like failures can move to:

```text
Quarantined
```

---

# 117. Quarantine

Requires operator/application action.

Examples:

```text
invalid provider payload
unrecognized destination
schema migration bug
```

---

# 118. Failed vs Quarantined

`Failed`:

```text
known terminal business/technical outcome
```

`Quarantined`:

```text
unsafe/unknown/manual inspection required
```

---

# 119. Manual Retry

Admin may retry quarantined/failed job if semantics permit.

Creates:

```text
new attempt
```

not new JobId by default.

---

# 120. Payload Modification

Do not mutate original job payload after possible execution.

If operator needs changed parameters:

```text
create new job
```

linked to old one.

---

# 121. Job Lineage

Use Part 02:

```text
caused_by
correlation
```

---

# 122. Workflow Lineage

WorkflowId groups related jobs.

---

# 123. Cancellation Audit

Cancel/retry/manual resolve are admin/business audit events.

---

# 124. Security

Job workers operate with:

```text
service principal
least privilege
tenant context
```

---

# 125. No Cross-Tenant Job Execution

TenantId must be explicit in job record/context.

---

# 126. JobHandlerContext

Conceptually:

```rust
pub struct JobHandlerContext {
    pub job_id: JobId,
    pub tenant_id: TenantId,
    pub attempt: u32,
    pub fence: FencingToken,
    pub correlation_id: CorrelationId,
}
```

---

# 127. Secret Access

Provider credentials come from secure secret/KMS layer.

Do not embed secret in job payload.

---

# 128. Credential Rotation

Job resolves current credential at execution unless historical credential identity is semantically required.

---

# 129. Provider Endpoint Config

Version configuration if it affects semantics.

---

# 130. Webhook Secret

Store reference/key ID, not raw secret in payload.

---

# 131. Network Egress Policy

Workers may have allowlist by job kind/provider.

---

# 132. SSRF Protection

Webhook destinations can be dangerous.

Validate:

```text
scheme
host
private IP restrictions
redirect policy
```

Part 27 expands threat model.

---

# 133. Email Abuse

Rate limit tenant email jobs.

---

# 134. Payment Abuse

Payment jobs require strong authorization through originating domain operation.

Worker must not trust arbitrary job insertion.

---

# 135. Job Creation API

Application should create job only through:

```text
registered typed APIs
authoritative side-effect intents
admin workflow APIs
```

not raw arbitrary kind/payload.

---

# 136. Internal DB Permissions

Only Aequora runtime/service roles may mutate job tables.

---

# 137. Scheduler

Part 06 is client scheduler.

Part 23 server job scheduler is distinct but shares principles.

---

# 138. Admission

Part 18 applies:

```text
global
tenant
work class
provider
```

permits.

---

# 139. Performance

Part 19:

```text
bounded payload
streaming large work
no Tokio blocking
```

---

# 140. Tokio Architecture

Use Tokio for:

```text
job polling
provider HTTP
DB I/O
timers
```

---

# 141. Rayon

Use bounded Rayon for:

```text
document render CPU
compression
hashing
bulk transform
```

---

# 142. Job Worker Pool

Structure:

```text
poller
↓
bounded dispatch
↓
class-specific workers
```

---

# 143. No Task Per Job Flood

Claim bounded batch according to available permits.

---

# 144. Poller Backoff

If no jobs:

```text
sleep/backoff
```

or wait on notification.

---

# 145. Poller Failure

DB unavailable:

```text
circuit/backoff
```

---

# 146. Job Notification

Postgres LISTEN/NOTIFY can wake workers.

Not source of truth.

---

# 147. Multi-Node Workers

Multiple nodes can process same shared durable job store.

Claim/fence prevents double ownership.

---

# 148. Region Placement

Part 17 jobs may run:

```text
authority region only
allowed regional worker
provider-local region
```

depending on semantics.

---

# 149. JobExecutionRegionPolicy

```rust
pub enum JobExecutionRegionPolicy {
    AuthorityOnly,
    AnyAllowedRegion,
    SpecificRegion(RegionId),
}
```

---

# 150. Domain-Mutating Job

If job ultimately submits authoritative operation:

```text
can originate elsewhere
```

but authoritative mutation still happens at writer.

---

# 151. Data Residency

Job payload/object data must respect allowed regions.

---

# 152. Authority Failover

Part 16:

```text
old worker fleet may still run
```

Need epoch/authority guard for jobs that create authoritative effects.

---

# 153. AuthorityEpoch in Job

Jobs tied to authoritative timeline may store:

```text
created_under_epoch
```

---

# 154. Epoch Transition

Classify outstanding jobs:

```text
SafeContinue
Revalidate
ReconcileExternal
CancelDerived
ManualReview
```

---

# 155. Snapshot Build After Epoch Change

Old-epoch snapshot build:

```text
cancel
```

new epoch rebuild.

---

# 156. Payment Job After Epoch Change

Reconcile external state before retry.

---

# 157. Audit Export

May continue if source boundary remains valid; otherwise replan.

---

# 158. Governance Purge

Must revalidate current legal holds/policy.

---

# 159. Job Epoch Recovery Policy

```rust
pub enum JobEpochPolicy {
    Continue,
    Revalidate,
    Reconcile,
    Cancel,
    Manual,
}
```

---

# 160. Recovery Mode

Part 16 server can pause selected job kinds after disaster restore.

---

# 161. Side-Effect Restore Barrier

Do not run ambiguous external jobs until recovery reconciliation completes.

---

# 162. Background Job Persistence Schema

Logical:

```text
aequora_job
aequora_job_attempt
aequora_job_lease
aequora_job_dependency
aequora_workflow
aequora_side_effect_intent
aequora_side_effect_execution
```

---

# 163. `aequora_job`

Fields:

```text
job_id
tenant_id
job_kind
payload_schema_version
payload_bytes/ref
payload_digest
state
priority
workflow_id
attempt_count
next_run_at
created_at
updated_at
epoch_policy
```

---

# 164. `aequora_job_lease`

Fields:

```text
job_id
worker_id
fencing_token
expires_at
```

---

# 165. `aequora_job_dependency`

Fields:

```text
job_id
depends_on_job_id
condition
```

Unique edge.

---

# 166. `aequora_workflow`

Fields:

```text
workflow_id
tenant_id
workflow_kind
workflow_version
state
checkpoint
correlation_id
created_at
updated_at
```

---

# 167. `aequora_side_effect_intent`

Fields:

```text
intent_id
operation_id
tenant_id
kind
idempotency_key
payload
payload_digest
created_at
```

Immutable after commit.

---

# 168. `aequora_side_effect_execution`

Fields:

```text
intent_id
job_id
provider
state
provider_reference
last_outcome
updated_at
```

---

# 169. Required Indexes

Job:

```text
state + next_run_at + priority
tenant_id + state
workflow_id
```

Lease:

```text
expires_at
```

Intent:

```text
operation_id
idempotency_key unique where applicable
```

---

# 170. Job Claim Index

Hot path should avoid scanning completed jobs.

Partial/filtered index where adapter supports.

---

# 171. Retention

Completed jobs need not live forever.

Retention class by kind.

---

# 172. Side-Effect Intent Retention

May need to outlive job record for:

```text
audit
provider reconciliation
idempotency
```

---

# 173. Attempt Retention

Could be shorter.

---

# 174. Workflow Retention

High-value workflow summary may retain longer.

---

# 175. Legal Hold

Part 14 can hold relevant jobs/intents.

---

# 176. Erasure

Job payload may contain PII.

Governance must include job store.

---

# 177. Payload Minimization

Prefer entity IDs and references over duplicated full user data.

---

# 178. Snapshot Jobs

Payload references:

```text
scope
boundary
profile
```

not entity data.

---

# 179. Export Jobs

Payload references query/export definition.

Output stored separately.

---

# 180. Document Generation

Input can be immutable document snapshot/reference.

If exact document needed later, capture template/version.

---

# 181. Job Progress

Optional:

```rust
pub struct JobProgress {
    pub completed_units: u64,
    pub total_units: Option<u64>,
}
```

---

# 182. Progress Is Advisory

Do not use percentage as correctness state.

---

# 183. Progress Update Frequency

Throttle to avoid DB write amplification.

---

# 184. Heartbeat

Lease renewal serves as worker liveness.

Separate heartbeat may be unnecessary.

---

# 185. Worker Registry

Optional operational table:

```text
worker_id
node_id
started_at
last_seen_at
capabilities
```

Not required for correctness.

---

# 186. Capability-Aware Workers

Some workers support:

```text
PDF render
KMS
specific provider
```

Job claim filters by capability.

---

# 187. WorkerCapability

Stable IDs.

---

# 188. No Hidden Singletons

A job kind can have many worker instances.

---

# 189. Idempotent Completion

If terminal update retried with same fencing token/outcome:

```text
safe
```

---

# 190. Terminal Transition Guard

Once completed:

```text
cannot return to Running
```

without explicit admin retry transition.

---

# 191. Retry Transition

`RetryScheduled -> Running` through new lease.

---

# 192. Job State CAS

All transitions specify expected prior state.

---

# 193. Workflow Concurrency

One workflow transition at a time.

Use:

```text
workflow row version
or lease
```

---

# 194. Parallel Steps

Workflow can schedule independent child jobs.

Workflow waits on dependency outcomes.

---

# 195. Fan-Out

Bound number of child jobs.

Avoid million-row fan-out if a chunked bulk job is better.

---

# 196. Fan-In

Aggregate results through durable counters/checkpoints.

---

# 197. Large Bulk Work

Prefer:

```text
one job with chunk checkpoint
```

over:

```text
one job per row
```

unless independent scheduling truly needed.

---

# 198. Job Explosion Protection

Part 18 quotas:

```text
max pending jobs per tenant
max fan-out
max workflow children
```

---

# 199. User-Initiated Jobs

Examples:

```text
export
document generation
bulk action
```

API returns:

```text
JobId
```

---

# 200. Status API

```text
GET /jobs/{id}
```

authorized by tenant/user policy.

---

# 201. Job Result

Small result can be metadata.

Large result:

```text
artifact ref
```

---

# 202. Polling vs Push

Client may:

```text
poll status
```

or receive Part 08 hint:

```text
JobCompleted
```

Hint remains advisory.

---

# 203. Admin API

Support:

```text
list
inspect
retry
cancel
quarantine
release
```

with strong permissions.

---

# 204. Job Explainability

Part 13 can answer:

```text
why this job exists
which operation caused it
what attempts happened
what provider result occurred
```

---

# 205. Replay

Part 12 replay suppresses side effects.

It may compare generated `SideEffectIntent`s.

---

# 206. Deterministic Intent

Same deterministic domain input should generate same semantic side-effect intent.

---

# 207. Intent Digest

Persist:

```text
payload_digest
```

for replay/audit.

---

# 208. Audit

Required audit events can be generated:

```text
job scheduled
manual retry
manual cancel
external payment confirmed
```

Not every low-level attempt needs business audit.

---

# 209. Operational Logs

Attempt details belong primarily in logs/attempt records.

---

# 210. Metrics

```text
jobs_ready
jobs_running
jobs_retry_scheduled
jobs_quarantined
job_attempt_total
job_latency
job_queue_age
side_effect_ambiguous_total
```

---

# 211. Queue Age

Important SLO:

```text
now - created_at
```

for oldest Ready job.

---

# 212. Per-Kind Metrics

JobKind is bounded/registered, suitable label.

---

# 213. Tenant Metrics

Avoid unbounded tenant label in global Prometheus.

Use top-N diagnostics.

---

# 214. Alerting

Alert on:

```text
queue age high
quarantine spike
provider ambiguity spike
lease churn
retry storm
stuck workflow
```

---

# 215. SLO

Examples:

```text
95% interactive jobs start < 5s
99% critical jobs start < 1s
bulk jobs progress continuously
```

Product/deployment specific.

---

# 216. Job Deadline

Some jobs may have business deadline:

```text
must attempt before X
```

Store separately from retry schedule.

---

# 217. Expired Job

If deadline passes:

```text
terminal failure
or
domain revalidation
```

kind-specific.

---

# 218. Scheduled Time vs Deadline

Different concepts.

---

# 219. Timezone

Store canonical UTC instant.

Business-local scheduling resolved before job creation or by versioned scheduler policy.

---

# 220. Recurring Jobs

Examples:

```text
nightly cleanup
monthly statement
```

Represent recurrence as:

```text
schedule definition
↓
materialized Job per occurrence
```

---

# 221. Recurrence Definition

```rust
pub struct RecurringSchedule {
    pub schedule_id: ScheduleId,
    pub kind: JobKind,
    pub rule: ScheduleRule,
}
```

---

# 222. Avoid Infinite In-Memory Cron

Persist schedule.

---

# 223. Schedule Materializer

Creates due jobs idempotently.

---

# 224. OccurrenceId

Derive from:

```text
schedule_id + scheduled instant
```

so duplicate scheduler runs do not create duplicate occurrence.

---

# 225. Recurring Domain Operation

At occurrence:

```text
job submits domain operation
```

which revalidates current business state.

---

# 226. Clock Skew

One authority time source.

Multiple schedulers coordinate through unique occurrence key.

---

# 227. Daylight Saving

Recurring local-time schedules need:

```text
timezone ID
schedule policy
```

Part 12 time semantics.

---

# 228. Missed Occurrence Policy

```rust
pub enum MissedOccurrencePolicy {
    RunImmediately,
    Skip,
    Coalesce,
    RunAll,
}
```

---

# 229. Coalesce

Useful for:

```text
"refresh dashboard"
```

---

# 230. RunAll

Potentially dangerous after long outage.

Use limits.

---

# 231. Maintenance Jobs

Examples:

```text
journal GC
tombstone GC
snapshot GC
integrity scan
metadata verify
```

---

# 232. Maintenance Priority

Normally:

```text
Maintenance
```

unless correctness/security escalates.

---

# 233. Leadership

Only one scheduler should materialize a given recurring global job occurrence.

Use DB uniqueness, not only leader assumption.

---

# 234. Multi-Node Safety

Multiple nodes can run scheduler concurrently.

Unique occurrence key prevents duplicate.

---

# 235. Node Shutdown

Graceful shutdown:

```text
stop claiming
finish/checkpoint bounded active jobs
release/allow lease expiry
```

---

# 236. Forced Shutdown

Lease expiry recovers.

---

# 237. Rolling Deploy

Old/new workers may coexist.

Part 21 job payload/workflow version compatibility required.

---

# 238. New Job Kind

Do not schedule until sufficient worker fleet supports it.

Feature gate.

---

# 239. Required Worker Capability

Control plane/registry verifies before activation.

---

# 240. Old Worker

If claims unknown kind:

```text
must not
```

claim query filters supported kinds.

---

# 241. Job Version Negotiation

Not network negotiation; persisted payload version.

Handler registry knows supported versions.

---

# 242. Payload Upcast

Old queued jobs can be upcast if semantics safe.

---

# 243. Possibly-Executed Job

Do not mutate payload in place after an attempt may have produced external effect.

---

# 244. New Corrective Job

Create new job linked to original.

---

# 245. Provider SDK Isolation

External provider crates live outside domain/core.

Example:

```text
aequora-provider-email
aequora-provider-webhook
aequora-provider-payment-*
```

---

# 246. Provider Trait

Example:

```rust
pub trait SideEffectProvider {
    async fn execute(
        &self,
        intent: &SideEffectIntent,
    ) -> Result<ExternalOutcome, ProviderError>;

    async fn reconcile(
        &self,
        key: &ExternalIdempotencyKey,
    ) -> Result<ReconciliationResult, ProviderError>;
}
```

---

# 247. Not Every Provider Supports Reconcile

Capability manifest:

```text
supports_idempotency
supports_lookup
supports_cancel
```

---

# 248. Provider Capability

Worker policy adapts.

---

# 249. Payment Provider Requirement

High-risk payment integration should require:

```text
idempotency or reconciliation support
```

for production certification.

---

# 250. Webhook Signing

Part 15 can sign outgoing webhook payload.

---

# 251. Webhook Delivery Record

Store:

```text
delivery_id
destination
attempt
response code
```

with retention.

---

# 252. Destination Change

Existing queued webhook should use captured destination/version if semantics require.

Do not silently send historical event to newly configured destination unless policy says so.

---

# 253. Email Template Version

Capture template version in intent.

---

# 254. Document Template Version

Same.

---

# 255. External Policy Snapshot

Tax/payment/provider config affecting operation should be versioned.

---

# 256. Correctness Invariants

Add:

## AEQ-INV-JOB001

```text
Required asynchronous work exists durably before any worker is allowed to depend on its execution.
```

## AEQ-INV-JOB002

```text
Only the holder of the current fencing token may checkpoint or terminally transition a claimed job.
```

## AEQ-INV-JOB003

```text
A domain-side external effect is represented by a committed SideEffectIntent before external execution begins.
```

## AEQ-INV-JOB004

```text
Workers never bypass authoritative domain handlers when external results require business-state mutation.
```

## AEQ-INV-JOB005

```text
Ambiguous external outcomes are handled by an explicit reconciliation policy rather than blind retry.
```

## AEQ-INV-JOB006

```text
Required job/workflow state is never stored only in process memory.
```

---

# 257. Additional Invariants

## AEQ-INV-JOB007

```text
A stale worker whose lease has expired cannot overwrite progress committed by a newer worker.
```

## AEQ-INV-JOB008

```text
Long-running jobs checkpoint bounded progress so process death does not require restarting unbounded work from zero.
```

## AEQ-INV-JOB009

```text
Retry scheduling is bounded/backed off and cannot create an uncontrolled tight retry loop.
```

---

# 258. Property Tests

Generate:

```text
claims
lease expiry
worker crash
retry
reclaim
```

Assert single valid checkpoint lineage.

---

# 259. Double Worker Test

Worker A lease token 1.

Worker B reclaims token 2.

A attempts checkpoint.

Expected:

```text
rejected
```

---

# 260. Crash Before Provider Call

Expected:

```text
job retry
no effect
```

---

# 261. Crash After Provider Call

Expected:

```text
ambiguous
reconcile/idempotent retry
```

---

# 262. Crash After Terminal Commit

Expected:

```text
no re-execution
```

---

# 263. Provider Timeout Test

Provider receives request but response lost.

Policy-specific handling.

---

# 264. Payment Idempotency Test

Retry same key.

Expected:

```text
one provider charge
```

assuming provider guarantee.

---

# 265. Workflow Compensation Test

Step 2 fails after step 1 committed.

Expected:

```text
compensation job scheduled
```

not DB rollback fiction.

---

# 266. Recurring Duplicate Scheduler Test

Two schedulers materialize same occurrence.

Expected:

```text
one JobId/occurrence
```

---

# 267. Epoch Recovery Test

Restore/failover with ambiguous side-effect job.

Expected:

```text
recovery policy applied
```

---

# 268. Governance Test

Erase subject with pending email/export job containing PII.

Expected:

```text
governance plan includes job payload/artifact
```

---

# 269. Load Test

Simulate:

```text
1M pending jobs
```

with indexed claim.

Measure:

```text
claim latency
queue age
DB load
```

---

# 270. Provider Outage Test

Provider down for hours.

Expected:

```text
circuit breaker
backoff
no DB/job storm
other job classes continue
```

---

# 271. Recommended Modules

```text
aequora-jobs/
├── id.rs
├── kind.rs
├── state.rs
├── record.rs
├── registry.rs
├── lease.rs
├── retry.rs
├── scheduler.rs
├── worker.rs
├── dependency.rs
├── recurring.rs
└── errors.rs
```

---

# 272. Workflow Crate

```text
aequora-workflow/
├── workflow.rs
├── state.rs
├── transition.rs
├── checkpoint.rs
├── compensation.rs
└── version.rs
```

---

# 273. Side Effect Crate

```text
aequora-side-effects/
├── intent.rs
├── execution.rs
├── provider.rs
├── idempotency.rs
├── ambiguity.rs
├── reconciliation.rs
└── result.rs
```

---

# 274. Server Integration

```text
aequora-server/
└── jobs/
    ├── poller.rs
    ├── claim.rs
    ├── dispatch.rs
    ├── admin.rs
    └── metrics.rs
```

---

# 275. Storage Traits

```rust
pub trait JobStore {
    async fn insert(&self, job: NewJob) -> Result<(), JobStoreError>;
    async fn claim_ready(
        &self,
        worker: WorkerId,
        kinds: &[JobKind],
        limit: usize,
    ) -> Result<Vec<ClaimedJob>, JobStoreError>;
    async fn checkpoint(...);
    async fn complete(...);
    async fn schedule_retry(...);
}
```

---

# 276. WorkflowStore

Separate smaller trait.

---

# 277. SideEffectStore

Must participate in authoritative transaction for intent creation.

---

# 278. Atomic Intent Creation

Important:

```text
if business state commits
intent must exist
```

for required side effect.

---

# 279. Intent Worker Notification

After commit, notification can wake workers.

---

# 280. Transactional Outbox Pattern

This is essentially:

```text
transactional outbox
```

but typed and integrated into Aequora lineage/replay.

---

# 281. Side Effect State Machine

Recommended:

```text
Pending
↓
Executing
↓
Confirmed
```

branches:

```text
Retryable
Ambiguous
Failed
ManualReview
```

---

# 282. Payment State Example

```text
PaymentIntentCreated
↓
CapturePayment side effect
↓
ProviderConfirmed
↓
RecordPaymentResult operation
↓
PaymentCaptured domain state
```

---

# 283. Email State Example

```text
NotificationRequested
↓
SendEmail
↓
Sent/Failed
```

Depending on business, email sent status may be operational only.

---

# 284. Webhook State Example

```text
EventCommitted
↓
WebhookDelivery jobs
↓
receiver 2xx
```

---

# 285. Snapshot Build Example

```text
SnapshotBuild job
↓
chunk loop + checkpoints
↓
verify root
↓
publish manifest
```

---

# 286. Export Example

```text
ExportRequested operation/admin action
↓
Export job
↓
stream data
↓
encrypt/sign
↓
artifact ready
```

---

# 287. Governance Example

```text
ErasureRequest
↓
Plan job
↓
surface purge jobs
↓
verification job
↓
Completed
```

---

# 288. Anti-Entropy Example

```text
IntegrityMismatch
↓
Repair job
↓
verify
↓
client/server repair plan
```

---

# 289. Completion Criteria

Part 23 is complete when:

```text
[ ] JobId/JobKind/JobState defined
[ ] durable job store defined
[ ] leases + fencing defined
[ ] retry policies defined
[ ] side-effect intent boundary defined
[ ] external idempotency/reconciliation defined
[ ] ambiguous outcome handling defined
[ ] workflow/checkpoint/compensation defined
[ ] recurring durable schedules defined
[ ] provider bulkheads/fairness defined
[ ] authority epoch recovery policy defined
[ ] governance/retention integration defined
[ ] admin/status APIs defined
[ ] crash/duplicate/provider failure tests defined
[ ] job correctness invariants added
```

---

# 290. Final Architecture

```text
                 AUTHORITATIVE OPERATION
                          │
                          ▼
                    Domain Decision
                          │
                          ▼
                Authoritative Transaction
          mutation + journal + ledger + audit
                      + SideEffectIntent
                          │
                          ▼
                        COMMIT
                          │
                          ▼
                   Durable Job Store
                          │
                     claim + lease
                          │
                          ▼
                       Worker
                ┌─────────┼─────────┐
                ▼         ▼         ▼
             Email     Payment    Webhook
                │         │         │
                ▼         ▼         ▼
          Provider Result / Ambiguity
                          │
                          ▼
                 Reconcile / Capture
                          │
                          ▼
              New Authoritative Operation
                          │
                          ▼
                    Domain State

Long-running work:

Job
 │
 ▼
bounded chunk
 │
 ▼
checkpoint
 │
 ▼
yield / retry / resume
```

The architectural principle is:

> **Aequora should make asynchronous work durable before it becomes necessary, idempotent before it becomes retryable, fenced before it becomes concurrent, and observable before it becomes operationally important.**

This gives Aequora a reliable foundation for payments, email, webhooks, exports, snapshots, governance, repairs, scheduled workflows, and all other work that must survive process crashes and external-system ambiguity without weakening authoritative domain correctness.
