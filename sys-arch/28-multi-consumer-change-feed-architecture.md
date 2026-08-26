# Aequora Sync — Part 28

# Multi-Consumer Change Feed Architecture

## 1. Purpose

Aequora already maintains an authoritative journal for synchronization.

That journal contains valuable authoritative change information which may also be useful to:

```text
search indexers
analytics pipelines
notification services
audit projections
legacy compatibility projections
webhook dispatchers
reporting systems
AI/ML feature pipelines
cache invalidators
regional read-model builders
```

However, client synchronization and downstream event consumption have different requirements.

A synchronization cursor answers:

```text
what changes must this replica apply?
```

A downstream consumer asks:

```text
what authoritative changes must this service process?
```

These concerns should share the same underlying authoritative truth without sharing unsafe state.

The central rule is:

> **Aequora should publish one authoritative change history to many independent consumers, while giving every consumer its own durable position, filtering policy, delivery state, and failure isolation.**

---

# 2. Goals

The multi-consumer feed should provide:

```text
durable consumer cursors
independent consumer progress
at-least-once delivery
idempotent processing
tenant/scope filtering
consumer groups
backpressure
lag visibility
replay
pause/resume
dead-letter handling
schema compatibility
downstream isolation
```

---

# 3. Non-Goals

The subsystem is not:

```text
a replacement for Kafka in every deployment
a distributed log broker
a second authoritative event store
a client sync protocol
a guaranteed exactly-once external pipeline
```

Aequora should expose a clean change-feed abstraction that can run:

```text
directly from PostgreSQL/journal
through internal workers
or through external brokers
```

without changing semantics.

---

# 4. Source of Truth

The authoritative journal remains:

```text
aequora_journal
```

Downstream change feeds derive from it.

No downstream feed may become the source of authoritative state.

---

# 5. One Journal, Many Consumers

Conceptually:

```text
                    Authoritative Journal
                           │
        ┌──────────────────┼──────────────────┐
        ▼                  ▼                  ▼
      Search            Analytics         Notifications
     Consumer           Consumer            Consumer
        │                  │                  │
        ▼                  ▼                  ▼
      Cursor             Cursor             Cursor
```

Each consumer advances independently.

---

# 6. ConsumerId

Define:

```rust
pub struct ConsumerId(Uuid);
```

Stable identity for one logical downstream consumer.

Examples:

```text
search-main
analytics-school-erp
notifications-email
legacy-read-projection
```

Human-readable names may map to stable IDs.

---

# 7. ConsumerKind

Stable numeric registry:

```rust
pub struct ConsumerKind(u32);
```

Examples:

```text
SearchProjection
Analytics
Webhook
Notification
LegacyProjection
RegionalProjection
CustomIntegration
```

---

# 8. Consumer Group

A logical consumer may have multiple worker instances.

Define:

```rust
pub struct ConsumerGroupId(Uuid);
```

Workers in same group cooperate on one feed position.

---

# 9. Consumer Cursor

Canonical:

```rust
pub struct ConsumerCursor {
    pub consumer_id: ConsumerId,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
}
```

---

# 10. Independent Cursor

Client sync cursor and consumer cursor are separate types.

Never reuse:

```text
ScopeCursor
```

for downstream consumer state.

---

# 11. Consumer Epoch Binding

Cursor always binds:

```text
AuthorityId
AuthorityEpoch
```

Part 16 rules apply.

---

# 12. Epoch Transition

On new authority epoch, consumer must follow explicit transition policy:

```text
ContinueMapped
ReplayFromBoundary
Rebuild
ManualRecovery
```

---

# 13. Sequence Meaning

Consumer sequence means:

```text
all feed-visible events <= N have been durably processed
```

under that consumer's configured filter/projection semantics.

---

# 14. At-Least-Once Delivery

Default guarantee:

```text
at least once
```

because:

```text
worker may process event
crash before cursor checkpoint
event redelivered
```

Consumers must be idempotent.

---

# 15. Exactly-Once Logical Effect

Possible inside one DB transaction if:

```text
consumer projection
+
consumer cursor
```

commit atomically in same database.

---

# 16. External Consumer

If effect is in external system:

```text
exactly-once generally unavailable
```

Use:

```text
EventId
idempotency key
dedup ledger
```

---

# 17. EventId

Every journal event should have stable:

```text
EventId
```

Consumers use it as dedup key.

---

# 18. Feed Event Envelope

Conceptually:

```rust
pub struct ChangeFeedEvent {
    pub event_id: EventId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
    pub tenant_id: TenantId,
    pub entity_ref: EntityRef,
    pub event_kind: EventKind,
    pub schema_version: EventSchemaVersion,
    pub payload: Bytes,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<CausationId>,
}
```

---

# 19. Consumer View

A consumer may not need raw full event.

Use projection:

```text
journal event
↓
consumer projector
↓
consumer-specific change
```

---

# 20. ConsumerProjector

```rust
pub trait ConsumerProjector {
    type Output;

    fn project(
        &self,
        event: &CanonicalJournalEvent,
    ) -> Result<Option<Self::Output>, ProjectionError>;
}
```

---

# 21. None

`None` means:

```text
event irrelevant to this consumer
```

Consumer may still advance cursor after safely deciding irrelevance.

---

# 22. Filter Placement

Filter must be server-controlled/configured.

Do not accept arbitrary untrusted SQL from consumer.

---

# 23. Filter Types

Possible:

```text
tenant
event kind
entity type
scope routing key
operation kind
custom registered predicate
```

---

# 24. ConsumerFilter

```rust
pub struct ConsumerFilter {
    pub tenants: TenantSelector,
    pub entity_types: EntityTypeSet,
    pub event_kinds: EventKindSet,
}
```

---

# 25. Authorization

Internal consumer access is governed by service identity and configured tenant permissions.

External consumer must authenticate/authorize.

---

# 26. Cross-Tenant Safety

Consumer assigned Tenant A cannot request Tenant B by changing filter.

---

# 27. Consumer Registration

Logical registry:

```text
aequora_consumer
```

Fields:

```text
consumer_id
consumer_kind
name
status
filter_version
projection_version
delivery_mode
created_at
```

---

# 28. Consumer Status

```rust
pub enum ConsumerStatus {
    Active,
    Paused,
    Draining,
    Disabled,
    Rebuilding,
    Failed,
}
```

---

# 29. Consumer Cursor Persistence

Logical:

```text
aequora_consumer_cursor
```

Fields:

```text
consumer_id
authority_epoch
sequence
updated_at
```

---

# 30. Cursor Update Rule

Advance only after consumer effect is durably complete.

---

# 31. Projection Consumer Transaction

If projection DB is same Postgres:

```text
apply projection
+
consumer cursor
COMMIT
```

---

# 32. Different Database

If projection store separate:

```text
effect
↓
durable dedup
↓
cursor checkpoint
```

needs careful retry semantics.

---

# 33. Outbox-to-External

For external broker/service, use Part 23 durable job/outbox if direct delivery is required.

---

# 34. Pull Model

Preferred initial architecture:

```text
consumer worker
↓
scan journal after cursor
↓
process bounded batch
↓
checkpoint
```

Simple and reliable.

---

# 35. Push Model

Optional:

```text
journal commit
↓
best-effort wake hint
↓
consumer pulls
```

Same pattern as Part 08.

---

# 36. Broker Model

Future/large deployments:

```text
journal
↓
publisher
↓
Kafka/NATS/Pulsar/etc.
```

Broker becomes transport, not authority.

---

# 37. Broker Cursor

Aequora still tracks publication boundary if broker export must be auditable.

---

# 38. Broker Publication Outbox

If publishing journal to broker:

```text
journal event
+
broker-publication record
```

same authoritative transaction or derived durable job.

---

# 39. Broker At-Least-Once

Duplicate publication possible.

Broker consumers use EventId.

---

# 40. Consumer Lag

Define:

```text
current authoritative sequence
-
consumer sequence
```

within same epoch.

---

# 41. Lag Units

Track:

```text
sequence lag
estimated time lag
oldest unprocessed age
```

---

# 42. Sequence Lag Is Canonical

Time lag is operational estimate.

---

# 43. Lag Metrics

```text
consumer_sequence_lag
consumer_oldest_event_age
consumer_batch_duration
consumer_failures_total
```

---

# 44. Consumer Backpressure

A slow consumer must not block:

```text
authoritative commits
client sync
other consumers
```

---

# 45. Isolation

Consumers pull independently.

No shared global downstream queue that one stalled consumer can block.

---

# 46. Retention Interaction

Journal retention must consider active consumers if they depend on replay.

---

# 47. Consumer Retention Pin

Consumer can register:

```text
retention_required = true
```

or:

```text
rebuildable = true
```

---

# 48. Rebuildable Consumer

Search index can usually rebuild from snapshot/current state.

It should not pin journal forever.

---

# 49. Non-Rebuildable Consumer

A compliance export stream may require guaranteed history.

Such consumers may pin retention or use durable archival feed.

---

# 50. ConsumerRetentionPolicy

```rust
pub enum ConsumerRetentionPolicy {
    PinJournal,
    RebuildIfBehind,
    BestEffort,
}
```

---

# 51. PinJournal

Journal GC floor cannot pass consumer cursor.

Use only for justified consumers.

---

# 52. RebuildIfBehind

If consumer falls below floor:

```text
rebuild projection
```

from snapshot/current authoritative state.

---

# 53. BestEffort

If consumer misses history:

```text
resume from current boundary
```

appropriate only for ephemeral analytics/metrics.

---

# 54. Consumer Rebuild

State:

```text
Paused
↓
Rebuilding
↓
build baseline
↓
set cursor to boundary
↓
Active
```

---

# 55. Search Rebuild

Flow:

```text
take canonical snapshot at N
↓
build index
↓
set cursor N
↓
consume N+1 onward
```

---

# 56. Regional Projection Rebuild

Part 17 same pattern.

---

# 57. Legacy Projection Rebuild

Part 26 can rebuild compatibility read store.

---

# 58. Analytics

Analytics may prefer immutable event stream rather than current-state snapshot.

If history required, use:

```text
PinJournal
or external archive
```

---

# 59. Archival Feed

Aequora can optionally persist long-term immutable event archive separate from hot journal.

---

# 60. Archive Purpose

Protect hot DB from infinite journal retention.

---

# 61. Archive Format

Possible:

```text
partitioned Postcard + zstd
object storage
manifest + hashes
```

---

# 62. Archive Boundary

Each archive segment records:

```text
AuthorityEpoch
start sequence
end sequence
root digest
```

---

# 63. Archive Is Derived Durable History

May become consumer replay source if integrity verified.

---

# 64. Archive Publication

Do not delete hot journal until archive required by policy is durable and verified.

---

# 65. Consumer Replay Source

Could be:

```text
HotJournal
Archive
SnapshotPlusJournal
```

---

# 66. ConsumerCheckpoint

Optional richer record:

```rust
pub struct ConsumerCheckpoint {
    pub cursor: ConsumerCursor,
    pub projection_digest: Option<Digest>,
    pub updated_at: Timestamp,
}
```

---

# 67. Projection Digest

Useful for:

```text
search projection verification
regional read model consistency
```

---

# 68. Consumer Lease

Multiple workers in same group require claim coordination.

---

# 69. Consumer Worker Lease

Logical:

```text
aequora_consumer_lease
```

Fields:

```text
consumer_id
worker_id
fencing_token
expires_at
```

---

# 70. Single Cursor Worker

Simplest:

```text
one active worker per consumer
```

---

# 71. Parallelism

High-throughput consumers may partition feed.

---

# 72. Partitioning Strategy

Possible:

```text
tenant partition
entity hash partition
routing partition
```

---

# 73. Partitioned Consumer Cursor

```rust
pub struct ConsumerPartitionCursor {
    pub consumer_id: ConsumerId,
    pub partition_id: ConsumerPartitionId,
    pub epoch: AuthorityEpoch,
    pub sequence: Sequence,
}
```

---

# 74. Parallelism Caveat

Global journal ordering may be lost across partitions.

Only use where consumer semantics tolerate it.

---

# 75. Per-Entity Ordering

Partition function must ensure same entity goes to same partition.

---

# 76. Per-Tenant Ordering

Partition by TenantId if needed.

---

# 77. Global Ordering Consumer

Must remain single ordered cursor or use merge barrier.

---

# 78. ConsumerOrderingPolicy

```rust
pub enum ConsumerOrderingPolicy {
    Global,
    PerTenant,
    PerEntity,
    Unordered,
}
```

---

# 79. Global

Events processed in journal order.

---

# 80. PerTenant

Ordering guaranteed within tenant.

---

# 81. PerEntity

Ordering guaranteed for one entity/aggregate.

---

# 82. Unordered

Analytics/independent processors may tolerate.

---

# 83. Event Batching

Workers read bounded batches.

---

# 84. Batch Size

Part 18/19 rules:

```text
max events
max bytes
max processing time
```

---

# 85. Poison Event

One event repeatedly fails.

Do not block consumer forever without policy.

---

# 86. Failure Policy

```rust
pub enum ConsumerFailurePolicy {
    Stop,
    Retry,
    Quarantine,
    SkipWithAudit,
}
```

---

# 87. Stop

Safety-critical projection.

---

# 88. Retry

Transient failure.

---

# 89. Quarantine

Record bad event and pause/continue depending policy.

---

# 90. SkipWithAudit

Only for consumers where missing event is acceptable.

Never for authoritative projections.

---

# 91. Consumer Quarantine

Logical:

```text
aequora_consumer_quarantine
```

Fields:

```text
consumer_id
event_id
sequence
error_code
payload_digest
created_at
resolution
```

---

# 92. Poison Event Resolution

Admin may:

```text
retry after fix
mark ignored if policy permits
rebuild consumer
```

---

# 93. No Mutation of Journal Event

Do not edit bad historical event to satisfy consumer.

Fix consumer/upcaster.

---

# 94. Schema Evolution

Consumer projection version must be explicit.

---

# 95. ConsumerProjectionVersion

```rust
pub struct ConsumerProjectionVersion(u32);
```

---

# 96. Event Upcasting

Part 21 compatibility principles apply.

Consumer can upcast old journal event schema.

---

# 97. New Event Kind

Consumer may:

```text
ignore if declared optional/unrelated
fail if required semantic unknown
```

---

# 98. Consumer Capability Registry

Each consumer declares supported:

```text
event kinds
schema versions
projection version
```

---

# 99. Activation Check

Do not enable feed for consumer missing required schemas.

---

# 100. Rolling Consumer Upgrade

Old and new worker versions may coexist carefully.

Simplest:

```text
drain old
upgrade
resume
```

for one cursor.

---

# 101. Blue/Green Consumer

Create:

```text
consumer-v2
```

with independent cursor.

Build/verify.

Then switch downstream traffic.

---

# 102. Blue/Green Benefit

No risky in-place projection migration.

---

# 103. Search Example

```text
journal
↓
SearchConsumer
↓
OpenSearch/Tantivy/custom index
```

---

# 104. Search Idempotency

Index document by:

```text
EntityId
EntityVersion
```

Ignore older version.

---

# 105. Search Delete

Tombstone event removes document.

---

# 106. Analytics Example

```text
journal
↓
AnalyticsConsumer
↓
warehouse/lake
```

Use EventId as dedup key.

---

# 107. Notification Example

A notification consumer should not send email directly in the projection transaction.

Instead:

```text
event
↓
NotificationDecision
↓
SideEffectIntent / Job
```

Part 23 handles delivery.

---

# 108. Why

Preserves:

```text
retry
provider ambiguity
audit
```

---

# 109. Webhook Consumer

Similar:

```text
journal event
↓
delivery selection
↓
durable webhook jobs
```

---

# 110. Legacy Projection

Part 26:

```text
journal
↓
LegacyProjectionConsumer
↓
legacy read tables
```

---

# 111. Regional Projection

Part 17:

```text
journal/archive
↓
regional projection
```

---

# 112. Cache Invalidation

Consumer can emit:

```text
cache key invalidation
```

but cache correctness must tolerate duplicate invalidation.

---

# 113. AI/ML Features

Downstream feature extraction can consume feed.

It must not silently become domain authority.

---

# 114. Audit Projection

Searchable audit index can consume Part 13 audit feed or journal depending purpose.

---

# 115. Journal vs Audit Feed

Do not assume journal is full business audit.

Audit consumers needing actor/reason should consume audit trail.

---

# 116. Multiple Feed Sources

Aequora may expose:

```text
AuthoritativeChangeFeed
AuditFeed
JobFeed
SecurityEventFeed
```

Part 28 focuses on authoritative change feed.

---

# 117. FeedSourceId

Future abstraction:

```rust
pub enum FeedSource {
    Journal,
    Audit,
    Jobs,
    Security,
}
```

---

# 118. External Consumer API

Potential:

```text
POST /api/feed/v1/pull
```

but not required initially.

---

# 119. Internal Preferred

For same deployment:

```text
direct storage adapter
```

or worker service.

---

# 120. External Pull API

If exposed, request:

```text
consumer token
cursor
batch limits
```

---

# 121. Consumer Authentication

Use service credentials.

---

# 122. Cursor Validation

Server owns registered consumer position where stronger control required.

Do not let external client arbitrarily claim cursor forward.

---

# 123. Server-Managed Cursor

Preferred for guaranteed delivery.

Consumer ACK:

```text
processed through N
```

server validates expected sequence and advances.

---

# 124. Client-Managed Cursor

Simpler stateless API, but easier to misuse.

Could be allowed for best-effort feed.

---

# 125. ACK

Explicit:

```text
AckThrough(sequence)
```

after durable consumer processing.

---

# 126. Partial Batch ACK

Potential but adds complexity.

Initial:

```text
whole batch contiguous ACK
```

recommended.

---

# 127. Out-of-Order ACK

Reject for global ordered consumer.

---

# 128. Long Processing

Consumer should not hold feed lease indefinitely.

Use bounded batches.

---

# 129. Visibility Timeout

For queue-style external delivery, claim batch can have lease.

---

# 130. Pull Cursor vs Queue Claim

Two models:

```text
ordered cursor consumer
queue delivery consumer
```

Keep separate.

---

# 131. Ordered Cursor Consumer

Best for:

```text
projections
analytics
```

---

# 132. Queue Delivery Consumer

Best for:

```text
independent notifications/jobs
```

But can be implemented by Part 23 jobs after journal projection.

---

# 133. Recommendation

Use:

```text
journal cursor consumer
↓
materialize durable jobs
```

for side effects.

---

# 134. Consumer Idempotency Store

External consumers may maintain:

```text
processed_event(EventId)
```

---

# 135. Dedup Retention

Must cover event retry/replay horizon.

---

# 136. Event Version Guard

Projection update:

```text
if incoming EntityVersion <= stored version
    no-op
```

where semantics fit.

---

# 137. Non-Entity Events

Use EventId dedup.

---

# 138. Consumer State Export

Admin can inspect:

```text
cursor
lag
status
last error
lease
projection version
```

---

# 139. Admin APIs

Part 24:

```text
GET /consumers
GET /consumers/{id}
POST /consumers/{id}/pause
POST /consumers/{id}/resume
POST /consumers/{id}/rebuild
POST /consumers/{id}/reset-plan
```

---

# 140. Reset Is Dangerous

Do not allow arbitrary cursor edit.

Use plan:

```text
replay from X
rebuild at snapshot N
skip to current
```

---

# 141. ConsumerResetPlan

```rust
pub enum ConsumerResetMode {
    ReplayFrom(Sequence),
    RebuildFromSnapshot(SnapshotId),
    SkipToCurrent,
}
```

---

# 142. Authorization

`SkipToCurrent` may lose events.

Requires stronger permission/reason.

---

# 143. Audit

Pause/resume/reset/rebuild actions audited.

---

# 144. Consumer Checkpoint History

Optional:

```text
last N checkpoints
```

useful for diagnostics.

---

# 145. Incident Integration

Part 25 bundle can include:

```text
consumer cursor
lag
quarantined events
projection version
last checkpoint
```

---

# 146. Feed Health

Healthy consumer:

```text
cursor advances
lag bounded
no poison event
```

---

# 147. Stalled Consumer

Detect:

```text
cursor unchanged
while authority sequence advances
```

---

# 148. Lag Alert

Alert by policy.

---

# 149. Consumer SLO

Examples:

```text
search < 5s behind
notifications < 10s
analytics < 5m
```

Deployment-specific.

---

# 150. Fairness

Many consumers may read same journal.

Part 18 controls:

```text
DB read budget
consumer class
tenant impact
```

---

# 151. Consumer WorkClass

Examples:

```text
Search = Normal
Notifications = Interactive
Analytics = Bulk
Archive = Background
```

---

# 152. DB Load

Consumer scans must not starve authoritative writes.

Use:

```text
separate pool
read replica
rate limit
```

where appropriate.

---

# 153. Read Replica Consumption

Part 17 consumers may read from replica if:

```text
watermark/epoch safe
```

---

# 154. Authority-Only Consumer

Some critical feed may require primary.

---

# 155. ConsumerReadPolicy

```rust
pub enum ConsumerReadPolicy {
    Authority,
    ReplicaAllowed,
    ArchivePreferred,
}
```

---

# 156. Archive Consumer

Historical analytics can read archive, reducing primary load.

---

# 157. Snapshot + Tail

Fast rebuild pattern:

```text
snapshot at N
+
journal N+1 onward
```

---

# 158. Consumer Snapshot Format

Can reuse canonical snapshot or build consumer-specific projection snapshot.

---

# 159. Canonical Snapshot Preferred

Keeps source neutral.

Consumer builds its own index.

---

# 160. Projection Snapshot

Useful for huge search indexes if rebuild cost excessive.

It is derived and versioned.

---

# 161. Consumer Security

External feed may expose sensitive data.

Use least-data projection.

---

# 162. Field Projection

Consumer should receive only required fields.

---

# 163. Search Consumer

May not need:

```text
bank details
password metadata
```

---

# 164. Analytics Consumer

Could receive pseudonymized IDs.

---

# 165. Notification Consumer

Needs only:

```text
recipient ref
template vars
```

not full entity.

---

# 166. Data Minimization

Part 14 governance applies to every consumer copy.

---

# 167. Consumer Storage Surface

Register downstream durable copy with governance if controlled by Aequora ecosystem.

---

# 168. External Third-Party

Governance may only track export/delivery, not control deletion beyond contractual/provider APIs.

---

# 169. Erasure Propagation

For managed projections:

```text
journal tombstone/erasure event
↓
consumer deletes/pseudonymizes
```

---

# 170. Legal Hold

A derived search index usually need not retain held historical data if canonical source does.

But audit/archive consumer may.

---

# 171. Security Events

Consumer failures that risk data loss can generate security/operational alerts.

---

# 172. Tenant Isolation

Multi-tenant consumer store should include tenant key/index.

---

# 173. Dedicated Consumer Per Tenant

Possible but operationally expensive.

---

# 174. Shared Consumer

Usually better with tenant partitioning.

---

# 175. Consumer Quotas

External consumer may have:

```text
max batch
max requests/sec
max retained lag
```

---

# 176. Abuse Resistance

Feed API must enforce:

```text
bounded batch
auth
rate limit
filter constraints
```

---

# 177. Cursor Scraping Attack

Unauthorized consumer cannot request arbitrary historical tenant feed.

---

# 178. Feed Export Authorization

Historical replay may expose deleted/old data.

Requires stronger permission than current-state read.

---

# 179. Data Residency

Consumer placement must respect tenant policy.

---

# 180. Regional Consumer

Run in allowed region.

---

# 181. Broker Residency

External broker may violate policy if cross-region.

Register/validate.

---

# 182. Encryption

Feed transport TLS.

Archive/broker encryption according to Part 15.

---

# 183. Signed Feed Events

Not needed for every internal consumer.

For external offline/high-assurance feed, signed checkpoints or batch manifests may be used.

---

# 184. Feed Batch Manifest

Potential:

```rust
pub struct FeedBatchManifest {
    pub consumer_id: ConsumerId,
    pub epoch: AuthorityEpoch,
    pub start_sequence: Sequence,
    pub end_sequence: Sequence,
    pub root_digest: Digest,
}
```

---

# 185. Signed Batch

Useful for:

```text
external audit ingest
offline transfer
```

not ordinary internal use.

---

# 186. Schema Registry

Part 29 will govern stable:

```text
EventKind
EventSchemaVersion
ConsumerKind
```

---

# 187. Event Contract

Journal event semantics must be documented enough for downstream consumers.

---

# 188. Internal Event vs Public Event

Not every internal journal event should be public.

---

# 189. Feed Exposure Policy

```rust
pub enum FeedVisibility {
    InternalOnly,
    TenantIntegration,
    PublicIntegration,
}
```

---

# 190. Public Integration Event

Should be stable, minimized, versioned.

---

# 191. Internal Event

May evolve faster under compatibility policy.

---

# 192. Integration Projection

Recommended:

```text
internal journal
↓
stable integration event projector
↓
external feed
```

---

# 193. Why

Prevents external consumers coupling to internal implementation.

---

# 194. IntegrationEventKind

Separate registry from internal EventKind if needed.

---

# 195. Public Webhook Event

Example:

```text
student.updated.v2
```

edge representation may be JSON for interoperability.

Core remains typed.

---

# 196. JSON Justification

External integration is a valid JSON boundary.

---

# 197. Internal Postcard

Use Postcard between Rust services where suitable.

---

# 198. Consumer Replay

A consumer can request/rebuild historical sequence within retention.

---

# 199. Replay Rate Limit

Historical replay can be expensive.

Use Part 18 Bulk class.

---

# 200. Consumer Shadow Upgrade

Run v2 consumer in parallel.

Compare:

```text
projection digest
counts
sample queries
```

before cutover.

---

# 201. Differential Consumer Test

Same journal:

```text
old projector
new projector
```

compare expected semantic differences.

---

# 202. Search Migration

Build new index version from snapshot/tail.

Swap alias when verified.

---

# 203. Analytics Schema Migration

Write new table/topic version in parallel.

---

# 204. Notification Rule Upgrade

Rule version affects whether event creates side-effect intent.

Capture version for audit/replay.

---

# 205. ConsumerRuleVersion

```rust
pub struct ConsumerRuleVersion(u32);
```

---

# 206. Deterministic Projection

Projection should ideally be deterministic from:

```text
event
current consumer projection state
versioned rule
```

---

# 207. External Lookup

Avoid hidden network lookups inside projection if result affects durable output.

Use Part 23 side-effect/workflow pattern.

---

# 208. Search Enrichment

If external enrichment needed:

```text
projection writes base doc
↓
enrichment job
```

---

# 209. Analytics Enrichment

Capture version/source.

---

# 210. Consumer State Machine

```text
Registered
↓
Bootstrapping
↓
Active
├── Paused
├── Lagging
├── Failed
└── Rebuilding
↓
Disabled
```

---

# 211. Consumer Bootstrap

Choose:

```text
from beginning
snapshot + tail
current only
```

according to retention policy.

---

# 212. From Beginning

Only if full history available and required.

---

# 213. Current Only

Best-effort consumers.

---

# 214. Rebuildability Declaration

Every consumer must declare:

```text
CanRebuild
CannotRebuild
```

---

# 215. CannotRebuild Review

Should be rare and operationally important.

---

# 216. Retention Pin Budget

Too many pinning consumers can prevent journal GC forever.

Admin must monitor.

---

# 217. Max Pin Age

Policy can force:

```text
consumer rebuild/archive
```

instead of unlimited pin.

---

# 218. Consumer Retirement

Lifecycle:

```text
pause
drain
final checkpoint
disable
retention release
delete projection if policy
```

---

# 219. Cursor Cleanup

Do not delete immediately if audit/history needs.

---

# 220. Consumer Identity Reuse

Never reuse retired ConsumerId for a different semantic consumer.

---

# 221. Consumer Clone

New semantic version gets new ConsumerId or projection version with explicit migration.

---

# 222. Metadata Schema

Logical records:

```text
aequora_consumer
aequora_consumer_cursor
aequora_consumer_partition_cursor
aequora_consumer_lease
aequora_consumer_quarantine
aequora_consumer_checkpoint
```

---

# 223. `aequora_consumer`

Fields:

```text
consumer_id
kind
name
status
filter_version
projection_version
ordering_policy
retention_policy
read_policy
created_at
```

---

# 224. `aequora_consumer_cursor`

Fields:

```text
consumer_id
authority_epoch
sequence
updated_at
```

---

# 225. `aequora_consumer_partition_cursor`

Fields:

```text
consumer_id
partition_id
authority_epoch
sequence
updated_at
```

---

# 226. `aequora_consumer_lease`

Fields:

```text
consumer_id
worker_id
fencing_token
expires_at
```

---

# 227. `aequora_consumer_quarantine`

Fields:

```text
consumer_id
event_id
sequence
error_code
payload_digest
state
created_at
```

---

# 228. Required Indexes

```text
consumer_id unique
status
cursor by consumer
quarantine by consumer + state
lease expiry
```

---

# 229. Feed Archive Metadata

Optional:

```text
aequora_feed_archive_segment
```

Fields:

```text
authority_epoch
start_sequence
end_sequence
object_ref
root_digest
created_at
```

---

# 230. Archive Verification

Before marking available:

```text
segment complete
hash verified
manifest durable
```

---

# 231. Backup

Consumer metadata is operationally useful but may be rebuildable.

Pinning consumer cursor should be backed up.

---

# 232. Restore

After authority PITR/new epoch:

```text
consumer epoch recovery
```

Part 16 rules.

---

# 233. Consumer Recovery Policy

```rust
pub enum ConsumerEpochRecoveryPolicy {
    ContinueIfMapped,
    Rebuild,
    ReplayFromRecoveryBoundary,
    Manual,
}
```

---

# 234. Lost Consumer Cursor

If projection can verify its own applied sequence, recover.

Otherwise rebuild.

Do not guess.

---

# 235. Projection Metadata

Store applied sequence in projection DB too when useful.

---

# 236. Two-Sided Checkpoint

For external projection DB:

```text
Aequora cursor
projection cursor
```

must be reconciled after crash.

---

# 237. Projection Ahead of Aequora Cursor

Possible:

```text
effect committed
cursor checkpoint lost
```

Redelivery + idempotency fixes.

---

# 238. Aequora Cursor Ahead of Projection

Must never happen if ACK rule correct.

If detected:

```text
critical consumer corruption
```

---

# 239. Consumer Invariants

Add:

## AEQ-INV-FEED001

```text
Every durable consumer has an independent cursor bound to AuthorityId and AuthorityEpoch.
```

## AEQ-INV-FEED002

```text
A consumer cursor advances only after the consumer's required effect is durably complete.
```

## AEQ-INV-FEED003

```text
A stalled or failed consumer cannot block authoritative commits, client synchronization, or unrelated consumers.
```

## AEQ-INV-FEED004

```text
Duplicate delivery of the same EventId cannot produce duplicate logical consumer effects when the consumer declares idempotent delivery support.
```

## AEQ-INV-FEED005

```text
A consumer below the retained journal floor follows its declared rebuild/recovery policy rather than inventing missing history.
```

## AEQ-INV-FEED006

```text
External integration feeds expose only explicitly versioned and authorized projections, not arbitrary internal journal payloads.
```

---

# 240. Additional Invariants

## AEQ-INV-FEED007

```text
Consumer ordering guarantees are explicit and are not weakened by parallel partitioning without changing the declared policy.
```

## AEQ-INV-FEED008

```text
A consumer reset that may skip historical events requires an explicit plan, authorization, and audit record.
```

## AEQ-INV-FEED009

```text
Derived consumer stores participate in governance and residency policy when they contain governed tenant data.
```

---

# 241. Test — Duplicate Delivery

Process event.

Crash before cursor update.

Redeliver.

Expected:

```text
one logical projection result
```

---

# 242. Test — Poison Event

Projector fails deterministically.

Expected:

```text
retry/quarantine/stop according to policy
```

---

# 243. Test — Lagging Consumer

Authority advances 1M events.

Consumer paused.

Expected:

```text
other consumers unaffected
```

---

# 244. Test — Journal Floor Passed

Rebuildable consumer cursor below floor.

Expected:

```text
rebuild
```

---

# 245. Test — Pinning Consumer

Retention floor cannot advance past cursor.

---

# 246. Test — Wrong Epoch

Consumer cursor E, authority E+1.

Expected:

```text
epoch recovery policy
```

---

# 247. Test — Partition Ordering

Same EntityId events assigned same partition.

---

# 248. Test — Cross-Tenant Filter

Consumer authorized A requests B.

Expected:

```text
denied
```

---

# 249. Test — Search Blue/Green

Build v2 index from snapshot + tail.

Compare digest/query suite.

Swap only after verification.

---

# 250. Test — External Webhook

Duplicate journal event creates one durable webhook delivery intent per subscribed endpoint/event.

---

# 251. Test — Consumer Crash

Crash:

```text
after projection write
before cursor checkpoint
```

Expected:

```text
idempotent redelivery
```

---

# 252. Test — Cursor Corruption

Cursor ahead of projection applied position.

Expected:

```text
verification failure
rebuild/manual repair
```

---

# 253. Test — Governance Erasure

Erasure event removes subject from search/analytics projection according to policy.

---

# 254. Test — Archive Segment

Tamper object.

Digest verification fails.

---

# 255. Load Test

Run:

```text
10–100 consumers
```

with mixed rates.

Measure:

```text
journal scan load
lag
DB pool usage
consumer isolation
```

---

# 256. Performance Strategy

Prefer sharing journal page/cache reads where database naturally does so.

Do not build complex shared fan-out until measured need.

---

# 257. Dedicated Read Pool

Consumer DB scans can use separate pool.

---

# 258. Replica Offload

Part 17 read replica can serve consumer scans if consistent.

---

# 259. Archive Offload

Historical rebuild from archive/object storage.

---

# 260. Tokio Architecture

Use Tokio for:

```text
journal reads
projection DB writes
broker/network delivery
```

---

# 261. Rayon

Use bounded Rayon for:

```text
CPU-heavy transformation
index preprocessing
compression
```

---

# 262. Admission

Part 18 consumer classes receive quotas.

---

# 263. Resource Limits

Each consumer config:

```text
max batch events
max batch bytes
max concurrency
max lag pin
```

---

# 264. Consumer Config

Example RON:

```ron
consumer: (
    kind: SearchProjection,
    ordering: PerEntity,
    retention: RebuildIfBehind,
    read_policy: ReplicaAllowed,

    batch: (
        max_events: 1000,
        max_bytes: 4194304,
    ),

    retry: (
        max_delay_seconds: 60,
    ),
)
```

---

# 265. Consumer SDK

Application/library can register:

```rust
AequoraConsumer::builder()
    .kind(SearchProjection)
    .ordering(PerEntity)
    .retention(RebuildIfBehind)
    .handler(search_handler)
    .build();
```

---

# 266. Handler Context

```rust
pub struct ConsumerContext {
    pub consumer_id: ConsumerId,
    pub event_id: EventId,
    pub sequence: Sequence,
    pub tenant_id: TenantId,
}
```

---

# 267. Handler Result

```rust
pub enum ConsumerResult {
    Applied,
    Ignored,
    Retryable,
    Quarantine,
}
```

---

# 268. Application Boundary

Consumer handler owns external projection semantics.

Aequora owns:

```text
cursor
lease
retry
filter
ordering
```

---

# 269. Broker Adapter

Future:

```text
aequora-feed-kafka
aequora-feed-nats
aequora-feed-pulsar
```

---

# 270. Broker Adapter Contract

Must preserve:

```text
EventId
AuthorityEpoch
Sequence
TenantId
schema
```

---

# 271. No Broker Lock-In

Core consumer API independent.

---

# 272. Internal Direct Consumer

Initial recommended implementation:

```text
Postgres journal
+
Aequora worker
```

No external broker required.

---

# 273. When Broker Becomes Useful

Examples:

```text
many independent teams
very high fan-out
external ecosystem
cross-language consumers
long retention
```

---

# 274. Broker Tradeoff

Adds:

```text
infrastructure
operational cost
schema governance
security
```

Only introduce when justified.

---

# 275. Public Integration API

If exposing customer integrations:

```text
stable integration events
consumer registration
per-tenant auth
bounded replay
```

---

# 276. Customer Webhook Subscription

This is a specialized consumer whose effect becomes Part 23 webhook jobs.

---

# 277. Customer Feed Token

Scoped to:

```text
tenant
consumer
event types
```

---

# 278. Revocation

Disable consumer and credentials.

---

# 279. Consumer Delete

Prefer:

```text
disable/retire
```

then retention cleanup.

---

# 280. Event Contract Documentation

Generate docs:

```text
event kind
schema
fields
visibility
retention
```

---

# 281. Registry Governance

Part 29 will own stable IDs/compatibility.

---

# 282. Observability Dashboard

Show:

```text
consumer
status
cursor
lag
throughput
errors
quarantine
```

---

# 283. Incident Explain

Part 25:

```text
why did search not update?
```

Trace:

```text
EventId
consumer cursor
projection attempt
error
```

---

# 284. Security Monitoring

Unusual historical replay/export attempts logged.

---

# 285. Audit Consumer Admin Actions

Pause/reset/rebuild create admin audit.

---

# 286. Completion Criteria

Part 28 is complete when:

```text
[ ] ConsumerId/ConsumerKind defined
[ ] consumer registry defined
[ ] independent cursor defined
[ ] epoch binding defined
[ ] at-least-once semantics defined
[ ] idempotent processing defined
[ ] consumer filtering/projection defined
[ ] ordering policies defined
[ ] worker leases/fencing defined
[ ] lag/backpressure defined
[ ] retention pin/rebuild policies defined
[ ] poison event/quarantine defined
[ ] snapshot+tail rebuild defined
[ ] external integration projection defined
[ ] broker abstraction defined
[ ] governance/security integration defined
[ ] admin/diagnostic APIs defined
[ ] feed correctness invariants added
```

---

# 287. Final Architecture

```text
                     AUTHORITATIVE JOURNAL
                              │
                              ▼
                      Change Feed Layer
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
     Search                 Analytics           Notifications
   Consumer A              Consumer B           Consumer C
       │                       │                    │
       ▼                       ▼                    ▼
  Cursor A / Lease       Cursor B / Lease      Cursor C / Lease
       │                       │                    │
       ▼                       ▼                    ▼
 Search Index             Warehouse          SideEffect Jobs

Additional consumers:

   Legacy Projection
   Regional Projection
   Cache Invalidation
   External Integration Feed

Each consumer:
    own cursor
    own failure policy
    own ordering
    own projection version
    own retention/rebuild policy

No consumer:
    blocks authoritative commit
    becomes authoritative truth
    shares client sync cursor
```

The architectural principle is:

> **Aequora should let many systems consume authoritative change without letting any consumer become coupled to client synchronization or become a second authority.**

With independent durable cursors, explicit ordering and retention policies, idempotent processing, snapshot-based rebuilds, consumer isolation, schema governance, and optional broker adapters, Aequora can serve search, analytics, notifications, legacy projections, regional replicas, and external integrations from one coherent authoritative history.
