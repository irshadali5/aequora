# Aequora Sync — Part 17

# Multi-Region Read Architecture and Future Single-Writer Global Deployment

## 1. Purpose

Aequora's recommended authoritative model is intentionally simple:

```text
one authoritative writer timeline
+
many clients
+
optional replicas
```

This provides strong correctness and clean failure semantics.

However, a large SaaS or globally distributed deployment may need users in multiple geographic regions.

Examples:

```text
India
Europe
North America
Middle East
Southeast Asia
```

If every client request travels to one distant region, latency can become unnecessarily high.

Aequora therefore needs a global architecture that allows:

```text
regional reads
regional snapshot delivery
regional blob delivery
low-latency bootstrap
low-latency read APIs
```

while preserving:

```text
one authoritative writer
one AuthorityEpoch
one authoritative journal
one operation ledger
```

The central rule is:

> **Regional replicas may accelerate reads, but only the active authority may decide authoritative state transitions.**

---

## 2. Goals

The multi-region architecture should provide:

```text
low-latency reads
single-writer correctness
explicit replica lag
safe read watermarks
session-aware read routing
regional snapshot delivery
regional blob delivery
writer affinity
cross-region failover compatibility
bounded staleness policies
clear consistency guarantees
```

---

## 3. Non-Goals

This part does not introduce:

```text
active-active writes
multi-primary conflict resolution
global consensus inside Aequora
per-region authoritative journals
```

Those require a different architecture.

---

## 4. Global Topology

Recommended topology:

```text
                Authoritative Region
                       │
                       ▼
                Primary Postgres
                       │
             ┌─────────┼─────────┐
             ▼         ▼         ▼
        Region A    Region B   Region C
        Read Replica Read Replica Read Replica
```

Clients send:

```text
writes → authority
reads → nearest safe region where possible
```

---

## 5. Components

Each region may contain:

```text
Axum read/API nodes
regional cache
read replica
snapshot edge/cache
blob/object-storage edge
live-hint gateway
```

Only authority region contains active authoritative write executor.

---

## 6. Global Authority Identity

Part 16 remains unchanged:

```text
AuthorityId
AuthorityEpoch
```

are global across all regions.

Regional replicas do not have independent authority epochs.

---

## 7. RegionId

Define:

```rust
pub struct RegionId(u16);
```

Stable deployment-defined identifiers.

Examples:

```text
IN-WEST
EU-CENTRAL
US-EAST
```

Avoid using free-form strings in hot protocol paths.

---

## 8. ReplicaRole

```rust
pub enum RegionalRole {
    AuthorityWriter,
    ReadReplica,
    EdgeOnly,
    RecoveryStandby,
}
```

---

## 9. ReadReplica Identity

Define:

```rust
pub struct ReplicaId(Uuid);
```

Useful for diagnostics.

Not a business identity.

---

## 10. Replica Watermark

Every regional replica must expose the highest authoritative journal position it has durably applied.

```rust
pub struct ReplicaWatermark {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
}
```

---

## 11. Watermark Meaning

Replica watermark N means:

```text
all authoritative state through sequence N is visible on this replica
```

This must be tied to actual replica apply position.

---

## 12. Do Not Guess Watermark From Time

Never estimate:

```text
"replica is probably caught up because it is 2 seconds old"
```

Use a real replication/apply watermark.

---

## 13. Read Consistency Levels

Define:

```rust
pub enum ReadConsistency {
    Eventual,
    AtLeast(Sequence),
    Session,
    Authority,
}
```

---

## 14. Eventual

Read from nearest healthy replica.

May be stale.

Suitable for:

```text
public catalog
analytics
noncritical dashboards
```

---

## 15. AtLeast(Sequence)

Client requires replica state at or beyond known authoritative sequence N.

If nearest replica watermark < N:

```text
wait briefly
route to fresher replica
or route to authority
```

---

## 16. Session

Guarantee:

> **A user should not read older state than they have already observed in this session.**

Track session watermark.

---

## 17. SessionWatermark

Client/runtime maintains:

```rust
pub struct SessionWatermark {
    pub authority_epoch: AuthorityEpoch,
    pub min_sequence: Sequence,
}
```

---

## 18. Session Update

When client receives authoritative event through N:

```text
session watermark = max(current, N)
```

---

## 19. Read-Your-Writes

After client operation commits at sequence N:

```text
subsequent reads should request AtLeast(N)
```

This prevents:

```text
write accepted
↓
user refreshes
↓
regional replica still old
↓
change appears to disappear
```

---

## 20. Authority Reads

For strongest consistency:

```text
route read to primary authority
```

Use sparingly.

Examples:

```text
financial finalization
security configuration
workflow transition pre-check
```

---

## 21. Read Policy by Endpoint

Each API/read model can declare default consistency.

Example:

```text
GET /student/profile
    Session

GET /dashboard
    Eventual

GET /payment/status
    AtLeast(last known payment sequence)

GET /critical-balance
    Authority
```

---

## 22. Replica Router

Server edge/router component:

```rust
pub trait ReplicaRouter {
    async fn route(
        &self,
        consistency: ReadConsistency,
        session: Option<SessionWatermark>,
        region_hint: Option<RegionId>,
    ) -> Result<ReadTarget, RoutingError>;
}
```

---

## 23. ReadTarget

```rust
pub enum ReadTarget {
    RegionalReplica(ReplicaId),
    Authority,
}
```

---

## 24. Staleness Budget

Some reads can tolerate bounded staleness.

Define:

```rust
pub enum StalenessBudget {
    None,
    MaxSequenceLag(u64),
    MaxDuration(Duration),
}
```

Sequence lag is more reliable than wall-clock lag.

---

## 25. Duration Lag Caveat

Duration-based staleness is approximate.

Sequence-based guarantees are stronger.

---

## 26. Region Health

Router considers:

```text
replica reachable
watermark freshness
replication lag
load
maintenance state
```

---

## 27. Read Fallback

If nearest replica cannot satisfy:

```text
AtLeast(N)
```

fallback order:

```text
another regional replica
↓
authority
↓
error if authority unavailable
```

depending on endpoint policy.

---

## 28. Do Not Wait Forever

Replica wait should have bounded timeout.

Example:

```text
wait up to 100–500ms
```

then fallback.

Configurable.

---

## 29. Session Token

For stateless web APIs, server can return:

```text
Aequora-Session-Watermark
```

or encode equivalent metadata in application session.

Avoid exposing internal implementation unnecessarily if SDK manages it.

---

## 30. Client SDK Behavior

Aequora client runtime already knows cursor.

For synchronized data reads from local DB:

```text
regional server read path is often unnecessary
```

Local-first clients read local state.

Multi-region reads mainly matter for:

```text
web clients
admin APIs
server-rendered pages
large nonreplicated datasets
```

---

## 31. Local-First Benefit

Aequora's local-first design reduces global read-latency pressure.

Most interactive client reads are:

```text
local DB reads
```

Network is used for convergence.

This is strategically important.

---

## 32. Global Write Path

All authoritative writes route to:

```text
current AuthorityWriter region
```

No region may execute domain operations independently.

---

## 33. Write Router

A stable global endpoint may route:

```text
POST /sync
```

to active writer.

Examples:

```text
Anycast/LB
global DNS
application gateway
```

---

## 34. Writer Location Metadata

Server discovery may expose:

```rust
pub struct AuthorityLocation {
    pub region_id: RegionId,
    pub endpoint: TrustedEndpointRef,
    pub authority_epoch: AuthorityEpoch,
}
```

---

## 35. Avoid Client Hardcoding Region

Use stable service endpoint when possible.

Writer location can change during failover.

---

## 36. Write Latency

Single-writer global architecture means distant writes incur WAN RTT.

Aequora mitigates perceived latency through:

```text
local optimistic writes
outbox
background sync
```

This is exactly where local-first architecture helps.

---

## 37. Interactive Server-Confirmed Actions

Some operations require immediate authority confirmation.

Examples:

```text
payment finalization
seat reservation
critical approval
```

These still pay writer-region latency.

That is a correctness tradeoff.

---

## 38. Do Not Fake Immediate Global Writes

Avoid:

```text
accept write regionally
promise success
replicate later
```

unless operation semantics truly allow asynchronous provisional acceptance.

---

## 39. Provisional Acceptance

Aequora client already distinguishes:

```text
local provisional
server authoritative
```

That model should remain explicit.

---

## 40. Regional Ingress

Clients may connect to nearest regional edge.

Edge can:

```text
authenticate transport
rate-limit
forward write to authority
```

but should not execute domain command itself.

---

## 41. Edge Forwarding

Potential flow:

```text
Client
↓ nearest edge
↓ authenticated internal channel
Authority Writer
↓ result
Edge
↓ Client
```

---

## 42. Direct-to-Authority Alternative

Simpler initially:

```text
client uses global endpoint
LB routes directly to authority
```

Avoid extra hop unless edge benefits justify it.

---

## 43. Regional Read Replica Sources

Possible:

```text
Postgres physical replica
logical replica
managed read replica
application projection replica
```

---

## 44. Physical Postgres Replica

Strongest compatibility with authoritative relational state.

Watermark can derive from WAL/LSN mapped to Aequora journal boundary.

---

## 45. Application Projection Replica

Could contain:

```text
read models only
```

built from authoritative journal.

Then watermark is simply consumer sequence.

This can be very clean for read APIs.

---

## 46. Read Model Consumer

Each region:

```text
authoritative journal
↓
regional projection consumer
↓
regional read database
```

---

## 47. Projection Watermark

```rust
pub struct ProjectionWatermark {
    pub projection_id: ProjectionId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
}
```

---

## 48. Projection Advantages

Read model can be optimized for:

```text
search
dashboard
API queries
```

without replicating entire primary schema.

---

## 49. Projection Tradeoff

Eventual lag.

But explicit watermark makes consistency visible.

---

## 50. Recommended Strategy

For early enterprise scale:

```text
managed Postgres read replica
```

is simplest.

Later, high-volume read endpoints can use regional projections.

---

## 51. Sync Pull From Replica

Can clients pull journal changes from regional replica?

Potentially yes, if replica guarantees:

```text
journal data and business projections consistent through watermark N
```

---

## 52. Safer Initial Rule

Use authority for:

```text
sync exchange
```

including pull.

Use regional replicas for ordinary read APIs.

This keeps sync correctness simpler.

---

## 53. Future Regional Sync Pull

If needed:

```text
client sends cursor C
replica watermark R
```

Replica may serve changes only through:

```text
min(R, retained journal)
```

and response explicitly states:

```text
served_through = R
```

---

## 54. No Cursor Beyond Replica Watermark

Replica cannot advance client beyond data it actually has.

---

## 55. Push and Pull Split

Client outgoing operations still go to authority.

Incoming pull could eventually come from regional replica.

This creates more complexity in exchange API.

---

## 56. Recommendation

Do not split push/pull paths in v1.

Keep:

```text
exchange at authority
```

until latency/load proves it necessary.

---

## 57. Snapshot Regionalization

Part 10 snapshots are ideal for regional distribution.

Build once at authority.

Distribute immutable chunks to:

```text
regional object storage
CDN
edge cache
```

---

## 58. Snapshot Manifest Authority

Manifest remains authority-signed.

Regional edge cannot modify it.

---

## 59. Snapshot Chunk CDN

Because chunks are:

```text
immutable
content-addressed
hashed
```

they are excellent CDN objects.

---

## 60. Regional Snapshot Benefit

New client in distant region downloads gigabytes locally instead of from writer region.

---

## 61. Blob Regionalization

Large blob content can similarly use:

```text
regional edge/CDN
```

while metadata authority remains central.

---

## 62. Blob Consistency

Blob reference becomes visible only after authoritative metadata commit.

Content-addressed hash verifies bytes.

---

## 63. Write-Then-Read Blob

If user uploads blob and immediately downloads from another region:

```text
regional object replication may lag
```

Need fallback to origin or signed origin URL.

---

## 64. Blob Availability State

Metadata can distinguish:

```text
Committed
Replicating
RegionallyAvailable
```

if product needs precise UX.

---

## 65. Live Hints Multi-Region

Part 08 live connections may terminate in nearest region.

Hint broker distributes:

```text
new authoritative sequence
```

across regions.

---

## 66. Live Hint Does Not Require Replica Catch-Up

Important race:

```text
hint seq 1000 arrives region B
regional read replica only at 998
```

Client normal sync still goes authority or waits.

Do not assume hint implies regional read availability.

---

## 67. Hint Watermark

Regional live node can optionally attach:

```text
authority latest = 1000
regional readable through = 998
```

for advanced routing.

Not required initially.

---

## 68. Presence Multi-Region

Presence can be region-local plus brokered globally if product needs it.

Presence remains ephemeral.

---

## 69. Region Affinity

Client may prefer:

```text
nearest region
```

for reads/live/snapshots.

But authority may be elsewhere.

---

## 70. Region Selection

Use:

```text
latency
health
policy
data residency
```

not just geographic distance.

---

## 71. Data Residency

Some tenants may require data stay in specific region.

Then architecture may need:

```text
tenant-specific authority region
```

rather than one global writer for all tenants.

---

## 72. Tenant-Sharded Authorities

Aequora can support:

```text
Tenant A authority = India
Tenant B authority = EU
Tenant C authority = US
```

Each tenant belongs to exactly one AuthorityId/authority domain.

---

## 73. This Is Not Multi-Primary

Different tenants can have different writers.

Within one tenant/authority domain:

```text
one writer
```

---

## 74. Authority Directory

Global control plane can map:

```text
TenantId
→
AuthorityId
→
RegionId
```

---

## 75. Tenant Move

Moving tenant between regions uses Part 16 authority migration.

---

## 76. Data Residency Policy

```rust
pub struct ResidencyPolicy {
    pub allowed_regions: Vec<RegionId>,
    pub authority_region: RegionId,
    pub replica_regions: Vec<RegionId>,
}
```

---

## 77. Residency Enforcement

Object storage, snapshots, backups, read replicas must obey allowed regions.

---

## 78. Cross-Region Encryption

Part 15 tenant keys may remain managed in tenant's allowed region/KMS.

Regional read services need decrypt capability only if policy permits.

---

## 79. Server-Readable Encrypted Fields

If tenant encryption key unavailable in read region:

```text
regional replica cannot serve plaintext reads
```

Need:

```text
authority read
or
regional KMS access
```

---

## 80. ClientManagedE2E

Server region does not matter for plaintext because server cannot decrypt.

Only metadata residency remains.

---

## 81. Failover Interaction

Part 16 may promote writer to another region.

Then:

```text
AuthorityEpoch may remain
```

if continuity is proven.

Regional routers update writer location.

---

## 82. Failover With Same Epoch

Read replicas must follow new primary timeline.

Any replica on old divergent timeline must be rebuilt.

---

## 83. Failover With New Epoch

All regional replicas/projections need:

```text
new epoch recognition
```

Potential:

```text
rebuild
or
controlled transition
```

---

## 84. Replica Epoch Guard

A read replica must never serve data from:

```text
wrong AuthorityEpoch
```

as if current.

---

## 85. Router Epoch Check

Router only selects replicas reporting:

```text
current AuthorityEpoch
```

---

## 86. Stale Region After Failover

If region still serving old epoch:

```text
remove from routing
```

immediately.

---

## 87. Epoch-Aware Cache

Caches must include:

```text
AuthorityEpoch
```

in cache key/generation.

Otherwise stale cached responses can survive timeline transition.

---

## 88. Cache Generation

Use:

```text
epoch + projection version
```

for invalidation.

---

## 89. CDN Snapshot Epoch

Snapshot URLs/manifests bind:

```text
AuthorityEpoch
```

Old epoch snapshots cannot activate under new epoch.

---

## 90. DNS and Cache TTL

Writer failover routing must account for DNS TTL.

Application-level writer discovery or global LB may recover faster.

---

## 91. Read Replica Lag Metrics

Per region:

```text
replica_sequence_lag
replica_time_lag_estimate
projection_sequence_lag
```

Sequence lag is primary.

---

## 92. Read SLOs

Examples:

```text
95% eventual reads < 100ms
99% session reads satisfy watermark or fall back < 300ms
```

Application defines actual targets.

---

## 93. Global Write SLO

Single writer may have:

```text
higher RTT for distant users
```

but local-first UI can hide much of it.

---

## 94. Read Routing State

Router should use:

```text
health
watermark
load
region policy
```

from control plane.

---

## 95. Control Plane Staleness

If routing metadata stale:

```text
replica itself must reject unsupported consistency request
```

Defense in depth.

---

## 96. Replica Read Guard

Each read request includes required:

```text
AuthorityEpoch
minimum sequence
```

where consistency requires it.

Replica validates before serving.

---

## 97. ReadResponseMetadata

Optional:

```rust
pub struct ReadResponseMetadata {
    pub authority_epoch: AuthorityEpoch,
    pub served_sequence: Sequence,
    pub region: RegionId,
}
```

Useful for SDK/session tracking.

---

## 98. Client Session Watermark Update

After server read served through N:

```text
session watermark = max(session watermark, N)
```

---

## 99. Cache and Session Guarantees

A cache can serve session read only if cached entry metadata satisfies:

```text
served_sequence >= required_sequence
```

---

## 100. Edge Cache Metadata

Cache entry:

```text
epoch
sequence
projection version
```

not just TTL.

---

## 101. Eventual Cache

For low-value eventual reads, TTL cache is acceptable.

---

## 102. Authority Cache

Do not cache strongly consistent mutable results without explicit validation strategy.

---

## 103. Regional Read API Types

Potential split:

```text
StrongReadService
EventualReadService
```

instead of every repository call choosing arbitrary consistency.

---

## 104. Domain Default

Aggregate/profile can declare read preference.

But read consistency should remain endpoint/use-case specific.

---

## 105. Finance Read Example

Dashboard:

```text
Eventual
```

Payment confirmation:

```text
AtLeast(payment commit sequence)
```

Account balance used for new transaction decision:

```text
Authority
```

---

## 106. Student Profile Example

Normal display:

```text
Session
```

---

## 107. Analytics Example

```text
Eventual
```

regional projection ideal.

---

## 108. Search Example

Search index is derived projection.

Return:

```text
projection watermark
```

if caller needs freshness awareness.

---

## 109. Search Read-Your-Writes

If user adds student then immediately searches:

```text
regional search projection may lag
```

Options:

```text
show local result
temporarily query authority
wait for projection watermark
```

---

## 110. Local-First Search

On native clients:

```text
local index
```

can provide immediate read-your-writes.

Again, local-first reduces global consistency pressure.

---

## 111. Regional Projection Build

Projection worker consumes:

```text
authoritative journal
```

through durable consumer cursor.

---

## 112. Projection Idempotency

Apply EventId/Sequence idempotently.

---

## 113. Projection Crash Recovery

Resume from durable watermark.

---

## 114. Projection Rebuild

If corrupt:

```text
rebuild from snapshot + journal
```

using Part 10/03.

---

## 115. Projection Schema Version

Each regional projection declares:

```text
ProjectionSchemaVersion
```

Router only serves compatible API.

---

## 116. Rolling Deployment

Different regions may temporarily run different read projection versions.

API compatibility layer handles this.

---

## 117. Global Deployment Phases

Recommended progression:

```text
Phase 1:
    single region writer/read

Phase 2:
    object-storage CDN for snapshots/blobs

Phase 3:
    read replicas in secondary regions

Phase 4:
    regional derived projections

Phase 5:
    tenant-sharded authority regions
```

Do not jump directly to complex global topology.

---

## 118. Cost Discipline

Each region adds:

```text
database replica cost
network egress
operational complexity
observability
failover testing
```

Only add regions justified by users/SLO/data residency.

---

## 119. Cross-Region Egress

Snapshots and journal replication can create substantial egress.

Use:

```text
compressed journal
content-addressed snapshots
regional cache
```

---

## 120. Snapshot Deduplication

Part 10 content-addressed chunks reduce repeated transfer.

---

## 121. Regional Blob Replication Policy

Possible:

```text
OnDemand
PopularOnly
Full
ResidencyRestricted
```

---

## 122. OnDemand

Fetch from origin first time, cache regionally.

---

## 123. PopularOnly

Replicate hot objects.

---

## 124. Full

All blobs replicated.

Expensive.

---

## 125. ResidencyRestricted

Only approved regions.

---

## 126. Multi-Region Live Broker

Part 08 broker abstraction may need:

```text
cross-region pub/sub
```

or hierarchical broker.

---

## 127. Hint Fan-Out Optimization

Authority publishes:

```text
tenant/scope latest sequence
```

Regional nodes fan out locally.

---

## 128. Broker Outage

Regions fall back to polling.

No correctness loss.

---

## 129. Region Partition

Region B loses connectivity to authority.

What should happen?

Reads:

```text
may continue from stale replica if policy allows
```

Writes:

```text
remain queued locally on clients
```

Do not promote region automatically unless Part 16 promotion conditions met.

---

## 130. Degraded Regional Mode

```rust
pub enum RegionMode {
    Healthy,
    ReadOnlyStale,
    Disconnected,
    PromotionCandidate,
}
```

---

## 131. ReadOnlyStale

UI/API should know data may be stale.

---

## 132. Offline Client + Region Outage

Native Aequora clients keep working locally.

Outbox grows.

When authority reachable again:

```text
normal sync resumes
```

---

## 133. Regional Server-Side Users

Web-only users do not have local DB.

During authority outage:

```text
read-only degraded mode
```

may be possible from regional replica.

Writes should not be falsely accepted as committed.

---

## 134. Queued Server-Side Writes?

Avoid accepting server-side web writes into regional durable queue unless product explicitly wants asynchronous command acceptance.

That creates another local-first client role.

If needed, model regional gateway as Aequora client with durable outbox—not as authority.

---

## 135. Regional Command Gateway

Future pattern:

```text
web request
↓
regional gateway durable command queue
↓
authority later
```

Response must say:

```text
accepted for processing
```

not:

```text
committed
```

---

## 136. Not Recommended Initially

Adds:

```text
user-facing async semantics
regional queue recovery
identity/provenance complexity
```

Native local-first clients already solve this better.

---

## 137. Replica Freshness Header

Optional HTTP header or protocol metadata:

```text
served_epoch
served_sequence
```

SDK interprets.

---

## 138. Consistency Error

Typed response:

```text
ReplicaTooStale {
    available_sequence,
    required_sequence
}
```

Router can fallback.

---

## 139. AuthorityUnavailable

If caller requires `Authority` and writer unreachable:

```text
return unavailable
```

Do not silently downgrade to stale read unless endpoint policy explicitly allows.

---

## 140. Downgrade Policy

```rust
pub enum ReadFallbackPolicy {
    NeverDowngrade,
    AllowEventual,
    AllowBoundedStale,
}
```

---

## 141. Security Reads

Authorization state must be fresh enough.

Do not authorize sensitive access from stale replica unless security model explicitly permits.

---

## 142. AuthN/AuthZ Region Design

Prefer authentication token verification locally if cryptographic.

But authorization based on mutable roles/scopes may require:

```text
fresh authority
short-lived cached authorization
revocation channel
```

---

## 143. Scope Revocation

Part 07 revocation must propagate quickly.

Regional stale replica must not continue granting access indefinitely.

---

## 144. Security Watermark

Authorization-sensitive read may require:

```text
minimum security policy sequence
```

separate or same global sequence.

---

## 145. Recommended Security Rule

For sensitive reads:

```text
route through authority or replica with very fresh/verified authorization watermark
```

---

## 146. Token Claims

Short-lived token may carry authorized tenant/principal identity.

Still revalidate high-risk operations at authority.

---

## 147. Data Residency and Audit

Part 13 audit storage may require region-specific retention.

Authority can write canonical audit centrally per tenant's residency region.

---

## 148. Regional Audit Search

Read replicas/projections can serve audit search if permitted and sufficiently fresh.

---

## 149. Governance

Part 14 erasure must reach all regional replicas/caches.

Governance job tracks each region/storage surface.

---

## 150. Regional Purge

Erasure/purge complete only after:

```text
authority
regional replicas/projections
snapshot/CDN/object storage
```

required surfaces are verified.

---

## 151. Cache Invalidation After Erasure

Governance must invalidate:

```text
regional caches
CDN
search indexes
```

---

## 152. Cryptographic Erasure

Tenant key destruction can simplify purge across encrypted regional copies.

---

## 153. Key Residency

KMS policies may restrict keys to certain regions.

Design read regions accordingly.

---

## 154. Authority Failover + Residency

Promotion candidate must be allowed by tenant residency policy.

---

## 155. Tenant-Sharded Global Authority

Large SaaS can map tenants to different authority regions.

Benefits:

```text
lower write latency
data residency
failure isolation
```

---

## 156. ShardId

Optional:

```rust
pub struct AuthorityShardId(u32);
```

One shard has one writer.

---

## 157. Tenant Directory

Control-plane record:

```text
TenantId
AuthorityShardId
AuthorityId
AuthorityEpoch
AuthorityRegion
```

---

## 158. Client Discovery

On login/bootstrap:

```text
resolve tenant authority
```

Client caches signed/trusted assignment.

---

## 159. Tenant Move

Moving tenant between shards:

```text
quiesce writes
copy authority state
verify
fence old shard
activate new shard
new authority transition
rebootstrap/redirect as needed
```

---

## 160. Avoid Cross-Shard Transactions

If two tenants/shards need interaction:

```text
durable asynchronous workflow
```

not distributed transaction.

---

## 161. Cross-Region Server APIs

Internal service-to-service calls should carry:

```text
AuthorityEpoch
required sequence
tenant
```

where freshness matters.

---

## 162. Observability

Metrics per region:

```text
regional_read_requests_total
regional_read_fallback_total
replica_sequence_lag
projection_sequence_lag
authority_write_rtt
region_unavailable
snapshot_edge_hit_ratio
```

---

## 163. Avoid High Cardinality

RegionId is safe as label.

ReplicaId usually not necessary.

---

## 164. Logs

Structured:

```text
read_routed_replica
read_fallback_authority
replica_too_stale
region_degraded
authority_region_changed
```

---

## 165. Alerting

Alert on:

```text
replica lag above SLO
wrong epoch replica
regional projection stalled
authority write route failure
unexpected cross-region residency violation
```

---

## 166. Correctness Invariants

Add:

### AEQ-INV-REG001

```text
Only the active AuthorityWriter may commit authoritative domain transitions.
```

### AEQ-INV-REG002

```text
A regional replica serves an AtLeast(N) read only when its verified watermark is >= N in the current AuthorityEpoch.
```

### AEQ-INV-REG003

```text
A session read never intentionally returns state older than the caller's session watermark.
```

### AEQ-INV-REG004

```text
Replica routing never serves data from a stale AuthorityEpoch as current.
```

### AEQ-INV-REG005

```text
Failure of regional read infrastructure cannot create an alternative authoritative write timeline.
```

### AEQ-INV-REG006

```text
Read fallback never silently weakens consistency below the endpoint's declared fallback policy.
```

---

## 167. Additional Invariants

### AEQ-INV-REG007

```text
Regional snapshot/blob delivery may change transport location but not artifact authority or integrity semantics.
```

### AEQ-INV-REG008

```text
Tenant residency policy constrains authority, replica, snapshot, and blob placement.
```

### AEQ-INV-REG009

```text
Governance completion accounts for all registered regional copies that are required by policy.
```

---

## 168. Tests — Read-Your-Writes

```text
write commits at N
nearest replica at N-2
session read requires N
```

Expected:

```text
wait/fallback
never return N-2 result as session-consistent
```

---

## 169. Replica Catch-Up Test

Replica advances to N.

Expected:

```text
subsequent AtLeast(N) served regionally
```

---

## 170. Wrong Epoch Test

Replica reports epoch 4 while authority current epoch 5.

Expected:

```text
router removes/rejects replica
```

---

## 171. Region Partition Test

Regional replica disconnected from authority.

Expected:

```text
eventual reads optionally continue
strong/session reads fallback/fail
writes remain authority-only
```

---

## 172. Failover Test

Writer moves Region A → B with same epoch.

Expected:

```text
write routing updates
replicas follow new primary
no client epoch transition
```

---

## 173. Epoch Change Test

Failover creates epoch E+1.

Expected:

```text
old regional caches/replicas invalidated
clients transition per Part 16
```

---

## 174. Governance Test

Erase subject.

Expected:

```text
authority purge
regional projection purge
cache invalidation
object-storage purge
verification before complete
```

---

## 175. Snapshot CDN Test

Download chunk from untrusted edge.

Expected:

```text
manifest signature + chunk digest verify
```

---

## 176. Load Test

Measure:

```text
regional read throughput
fallback rate
replica lag under burst
write RTT by geography
snapshot edge hit ratio
```

---

## 177. Chaos Test

Inject:

```text
replica lag
region loss
routing stale metadata
broker outage
writer failover
```

Validate invariants.

---

## 178. Recommended Modules

```text
aequora-region/
├── region.rs
├── replica.rs
├── watermark.rs
├── read_consistency.rs
├── router.rs
├── session.rs
├── residency.rs
└── health.rs
```

Server:

```text
aequora-server/
└── region/
    ├── read_guard.rs
    ├── routing.rs
    ├── projection.rs
    └── admin.rs
```

---

## 179. Configuration

Example:

```ron
regions: (
    local_region: IN_WEST,

    read: (
        default_consistency: Session,
        replica_wait_ms: 200,
        allow_authority_fallback: true,
    ),

    snapshots: (
        regional_cache: true,
    ),

    residency: (
        enforce: true,
    ),
)
```

---

## 180. Deployment Profile — Single Region

```text
authority writer
read from primary
snapshot local/object storage
```

No regional complexity.

---

## 181. Deployment Profile — Dual Region

```text
Region A:
    authority writer

Region B:
    read replica
    snapshot/blob edge
```

---

## 182. Deployment Profile — Global Read

```text
one authority region
multiple read regions
regional projections
CDN/object storage
```

---

## 183. Deployment Profile — Tenant-Sharded Global

```text
tenant groups assigned to authority regions
one writer per tenant authority
regional reads around each authority
```

This is the preferred long-term way to reduce global write latency without multi-primary semantics.

---

## 184. Why Tenant Sharding Is Better Than Multi-Primary

It preserves:

```text
one authority timeline per tenant
simple conflict model
clean failover
```

while distributing load/geography.

---

## 185. Migration Path

Aequora should be designed so adding regions does not change:

```text
operation model
journal semantics
client outbox
cursor semantics
authority rules
```

Only deployment/routing expands.

---

## 186. Avoid Premature Global Complexity

For early product stages:

```text
one region
managed Postgres
object storage/CDN
```

may be more than enough.

Add replicas only after observed latency/SLO/residency need.

---

## 187. Completion Criteria

Part 17 is complete when:

```text
[ ] RegionId and replica roles defined
[ ] replica watermark defined
[ ] read consistency levels defined
[ ] session/read-your-writes semantics defined
[ ] replica routing/fallback defined
[ ] authority-only writes preserved
[ ] regional snapshot/blob delivery defined
[ ] regional projection architecture defined
[ ] failover/epoch integration defined
[ ] cache epoch invalidation defined
[ ] residency policy defined
[ ] tenant-sharded authority model defined
[ ] governance/security interactions defined
[ ] fault/load tests defined
[ ] regional correctness invariants added
```

---

## 188. Final Architecture

```text
                         GLOBAL CLIENTS
                    ┌─────────┼─────────┐
                    ▼         ▼         ▼
                 Region A  Region B  Region C
                    │         │         │
          ┌─────────┘         │         └─────────┐
          ▼                   ▼                   ▼
      Read Replica        Read Replica        Read Replica
          │                   │                   │
          └───────────────┬───┴───────────────────┘
                          │
                          ▼
                   AUTHORITY WRITER
                   AuthorityId / Epoch
                          │
                          ▼
                  Authoritative Journal
                          │
              ┌───────────┼───────────┐
              ▼           ▼           ▼
          Replica A   Replica B   Replica C
          watermark   watermark   watermark

Writes:
    always → Authority Writer

Reads:
    nearest replica
    if watermark satisfies requested consistency
    otherwise → fresher replica / authority

Snapshots/Blobs:
    authority-created, signed/hashed
    regionally cached/delivered
```

The architectural principle is:

> **Aequora should distribute read latency, not distribute authority.**

By combining local-first clients, explicit replica watermarks, session/read-your-writes guarantees, regional snapshot delivery, and one globally coherent writer timeline, Aequora can scale geographically without introducing the much harder correctness problems of active-active multi-primary synchronization.
