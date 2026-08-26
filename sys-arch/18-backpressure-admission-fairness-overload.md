# Aequora Sync — Part 18

# Backpressure, Admission Control, Fairness, and Overload Architecture

## 1. Purpose

Aequora can be logically correct and still fail operationally if it allows unbounded work.

Real deployments face:

```text
thousands of clients reconnecting after outage
large snapshot/bootstrap downloads
bulk imports
slow mobile clients
high-latency networks
hot tenants
expensive domain operations
large journal catch-up
blob transfers
regional failover reconnect storms
database pool exhaustion
```

Without explicit overload control, the system may enter a feedback loop:

```text
requests increase
↓
DB latency rises
↓
timeouts increase
↓
clients retry
↓
load increases further
↓
system collapses
```

Aequora therefore needs a first-class architecture for:

```text
backpressure
admission control
fairness
rate limiting
concurrency limits
queue bounds
load shedding
retry guidance
tenant isolation
priority handling
```

The central rule is:

> **When overloaded, Aequora should reject or defer work early and predictably rather than accept more work than it can safely complete.**

---

## 2. Goals

The overload subsystem should provide:

```text
bounded memory
bounded queues
bounded DB concurrency
tenant fairness
priority-aware admission
graceful degradation
retry coordination
reconnect-storm resistance
bulk-work isolation
slow-client isolation
operational visibility
```

---

## 3. Non-Goals

This subsystem is not:

```text
a distributed scheduler for all business jobs
a replacement for cloud load balancers
an unlimited queue
a promise that every request is accepted
```

Correct rejection is part of a healthy distributed system.

---

## 4. Overload Domains

Separate resources:

```text
HTTP requests
sync exchanges
DB connections
DB transactions
CPU-heavy validation
Rayon work
snapshot builders
snapshot downloads
blob transfers
live connections
background jobs
tenant work
```

Each may need its own limit.

---

## 5. Resource Budget

Define explicit budgets.

Conceptually:

```rust
pub struct ResourceBudget {
    pub max_in_flight: usize,
    pub max_queue: usize,
    pub max_bytes_in_flight: u64,
}
```

---

## 6. Admission Before Allocation

Reject before:

```text
reading giant body
allocating huge buffer
opening DB transaction
spawning expensive task
```

where possible.

---

## 7. AdmissionController

```rust
pub trait AdmissionController {
    async fn admit(
        &self,
        request: &WorkDescriptor,
    ) -> Result<AdmissionPermit, AdmissionRejection>;
}
```

---

## 8. WorkDescriptor

Contains:

```text
tenant
work class
estimated cost
scope
request size
priority
```

---

## 9. Work Classes

Reuse Part 06:

```text
Critical
Interactive
Normal
Bulk
Background
Maintenance
```

Server admission must not trust arbitrary client-declared priority.

---

## 10. Server-Derived Priority

Priority derives from:

```text
operation registry
endpoint
tenant policy
authenticated role
server-side classification
```

Client hint may only narrow within allowed bounds.

---

## 11. AdmissionPermit

Permit is RAII-style.

When dropped/completed:

```text
resource slot returns
```

---

## 12. Bounded Concurrency

Every expensive subsystem should use:

```text
Semaphore / bounded worker pool
```

not unbounded task spawning.

---

## 13. DB Pool Is Not Enough

A DB pool limits connections, but requests can still pile up waiting.

Need an outer admission layer.

---

## 14. DB Transaction Budget

Example:

```text
max 100 concurrent sync transactions
```

even if HTTP accepts more lightweight requests.

---

## 15. CPU Budget

CPU-heavy work:

```text
canonical hashing
compression
large validation
snapshot encode
```

should use bounded Rayon pool or bounded blocking tasks.

---

## 16. Separate CPU Pool

Do not let large snapshot compression starve interactive validation.

Possible pools:

```text
InteractiveCpu
BulkCpu
MaintenanceCpu
```

or one pool with quotas.

---

## 17. Queue Bounds

Queues must have finite capacity.

If full:

```text
reject
shed
or coalesce
```

Never silently grow without bound.

---

## 18. Overload Rejection

Typed response:

```rust
pub enum AdmissionRejection {
    ServerBusy,
    TenantBusy,
    RateLimited,
    QueueFull,
    ResourceBudgetExceeded,
}
```

---

## 19. HTTP Mapping

Typical:

```text
429 Too Many Requests
503 Service Unavailable
```

with retry guidance.

---

## 20. Retry-After

When possible, send:

```text
Retry-After
```

or protocol equivalent.

---

## 21. Retry Guidance Is Advisory

Client still uses jitter/backoff.

Do not create synchronized retry waves.

---

## 22. Stable Jitter

Part 06 stable jitter by device/tenant helps spread retries.

---

## 23. Reconnect Storm

After outage:

```text
100k clients reconnect
```

Mitigation:

```text
jitter
admission
connection rate limit
snapshot reuse
live-connection backoff
tenant fairness
```

---

## 24. Connection Admission

Limit:

```text
new connections/sec
```

independently from active connections.

---

## 25. Live Connection Budget

Part 08:

```text
max live connections
max connections per tenant
max scopes per connection
```

---

## 26. Slow Live Clients

Never accumulate unbounded hints.

Use:

```text
latest-only hint coalescing
disconnect if needed
```

---

## 27. Sync Exchange Cost

Not all sync requests are equal.

Examples:

```text
empty poll
10 operations
1000 operations
50 MB response
```

Admission should use rough cost estimates.

---

## 28. Cost Units

Define abstract:

```rust
pub struct CostUnits(u32);
```

Estimate from:

```text
request bytes
operation count
scope count
expected DB rows
```

---

## 29. Avoid Perfect Cost Prediction

A simple conservative estimator is better than a complex inaccurate model.

---

## 30. Tenant Fairness

One tenant must not consume all capacity.

Use per-tenant budget.

---

## 31. TenantLimiter

Conceptually:

```text
global permit
+
tenant permit
```

Both required.

---

## 32. Hierarchical Admission

Flow:

```text
global capacity
↓
tenant capacity
↓
work-class capacity
↓
resource-specific permit
```

---

## 33. Weighted Fairness

Large tenants may have higher contractual quota.

Use weights.

Example:

```text
Tenant A weight 1
Tenant B weight 4
```

---

## 34. Fair Queueing

Potential algorithms:

```text
weighted round robin
deficit round robin
weighted fair queue
```

Keep implementation simple.

---

## 35. No FIFO Across Everything

Pure global FIFO allows bulk work to block interactive work.

Use class-aware queues.

---

## 36. Priority Lanes

Example:

```text
Critical
Interactive
Normal
Bulk
Maintenance
```

with bounded capacity per lane.

---

## 37. Starvation Prevention

Background work must still eventually progress.

Use:

```text
aging
minimum reserved capacity
weighted scheduling
```

---

## 38. Reserved Capacity

Example:

```text
20% interactive reserve
10% maintenance reserve
```

actual numbers configurable.

---

## 39. Critical Work

Examples:

```text
security revocation
authority transition
small reconciliation
```

Do not allow bulk snapshot jobs to block them.

---

## 40. Bulk Work Isolation

Bulk import/snapshot creation should use separate permits.

---

## 41. Snapshot Build Limit

Part 10:

```text
one/few builds concurrently
```

per server/tenant.

---

## 42. Snapshot Download Bandwidth

Object storage/CDN should carry most large download traffic.

If server streams directly:

```text
bandwidth budget
```

must be explicit.

---

## 43. Blob Transfer Isolation

Blob traffic should not consume all HTTP worker/DB resources.

Separate endpoint/resource budget.

---

## 44. Request Body Limits

Enforce:

```text
max operations per batch
max encoded bytes
max decompressed bytes
```

before execution.

---

## 45. Compression Limits

Protect against:

```text
compression bombs
```

using expected/decompressed bounds.

---

## 46. Dependency Graph Limits

Operation batch dependency DAG must have bounded:

```text
nodes
edges
depth
```

to prevent CPU abuse.

---

## 47. Validation Budget

Complex domain validation may have:

```text
query count
CPU
time
```

limits.

---

## 48. Per-Operation Deadline

Executor may have a bounded server deadline.

But do not abort in ways that violate transaction ambiguity rules.

---

## 49. Cancellation

If client disconnects before transaction starts:

```text
cancel work
```

If authoritative commit is in progress:

```text
finish/resolve transaction safely
```

---

## 50. DB Timeout

Use:

```text
statement timeout
lock timeout
transaction timeout
```

where appropriate.

---

## 51. Deadlock Retry

Retry only bounded times.

After threshold:

```text
return retryable error
```

---

## 52. Lock Contention

Hot aggregate can serialize many operations.

Use:

```text
per-aggregate contention metrics
```

Do not solve by spawning more concurrency.

---

## 53. Hot-Key Protection

If one entity/aggregate is overloaded:

```text
limit concurrent attempts
```

or naturally rely on aggregate lock/queue.

---

## 54. Per-Aggregate Queue

Optional for extremely hot aggregates.

Not default.

---

## 55. Backpressure to Client

Protocol response can include:

```text
recommended max batch size
retry delay
server load class
```

---

## 56. ServerSchedulingHints

Part 06 can consume:

```text
max_ops
max_bytes
suggested_delay
compression_preference
```

---

## 57. Dynamic Batch Reduction

If server overloaded:

```text
ask client to send smaller batches
```

---

## 58. Client Adaptation

AIMD-like:

```text
success
→ cautiously increase

overload/timeout
→ decrease
```

bounded by configuration.

---

## 59. Overload vs Offline

`ServerBusy` is not:

```text
offline
```

Client should remain connected but back off.

---

## 60. Auth Failure vs Overload

Do not retry authorization errors like overload.

---

## 61. Rate Limiting

Rate limits protect:

```text
abuse
misconfigured clients
runaway retries
```

---

## 62. Rate Limit Dimensions

Possible:

```text
per IP
per device
per principal
per tenant
per endpoint
```

---

## 63. Avoid IP-Only Limits

NAT can place many legitimate users behind one IP.

Use authenticated dimensions after auth.

---

## 64. Token Bucket

Suitable for many rate-limit cases.

---

## 65. Burst Capacity

Allow short bursts while limiting sustained rate.

---

## 66. Hard Quotas

Examples:

```text
max devices per tenant
max concurrent live connections
max bootstrap jobs
```

Different from rate limits.

---

## 67. Quota Exhaustion

Typed response should explain:

```text
quota exceeded
```

not generic 500.

---

## 68. Admission and Billing

Do not couple correctness to billing service availability.

Entitlement/quota may be cached/versioned.

---

## 69. Load Shedding Order

When overloaded, shed lowest-value work first.

Recommended order:

```text
optional maintenance
background integrity scan
bulk snapshot build
bulk import
eventual analytics
normal work
interactive
critical/security
```

---

## 70. Maintenance Deferral

Anti-entropy can be postponed under heavy load.

But not forever.

---

## 71. Integrity Repair Priority

If corruption is confirmed:

```text
repair may become high priority
```

---

## 72. Read Load Shedding

Eventual read APIs may return:

```text
stale cache
```

if policy allows.

Strong reads should fail rather than silently weaken.

---

## 73. Regional Fallback

Part 17 can route reads to alternate replica.

---

## 74. Write Load Shedding

Never return success before authoritative commit.

If overloaded:

```text
reject/defer
```

client keeps operation in outbox.

---

## 75. This Is a Local-First Advantage

Client operation remains durable locally.

Aequora can safely respond:

```text
server busy
```

without losing user intent.

---

## 76. Admission Before Domain Transaction

Ideal:

```text
decode bounded metadata
↓
admit
↓
full validation/execution
```

---

## 77. Cheap Authentication First?

Depending on implementation:

```text
basic auth verification
```

may be needed before per-tenant admission.

Avoid expensive DB auth lookup under attack if token verification can be local.

---

## 78. Layered Edge Protection

Possible:

```text
load balancer rate limit
Axum connection limit
protocol body limit
tenant admission
DB permit
```

---

## 79. Circuit Breakers

For downstream dependencies:

```text
DB
KMS
object storage
email/payment providers
```

use circuit breakers where suitable.

---

## 80. DB Circuit Breaker

If DB unavailable:

```text
stop accepting expensive write work quickly
```

rather than timing out thousands of requests.

---

## 81. Half-Open Recovery

After failure, probe with small traffic.

Avoid instantly releasing full fleet.

---

## 82. Dependency Bulkhead

Separate concurrency for:

```text
payment provider
email provider
webhook destination
```

Part 23 side-effect workers should use per-provider bulkheads.

---

## 83. Queue Durability

In-memory queues are only scheduling aids.

Durable work remains in:

```text
client outbox
server durable job table
side-effect outbox
```

---

## 84. Never Rely on In-Memory Queue for Required Work

If process crashes:

```text
required work must still be discoverable
```

---

## 85. Worker Pull Model

Background workers should claim bounded jobs from durable store.

---

## 86. Lease/Claim

Use:

```text
job claim token
lease expiry
```

to prevent duplicate uncontrolled execution.

---

## 87. Work Stealing

Multiple workers may claim different jobs.

Do not use global lock.

---

## 88. Per-Tenant Background Fairness

Job scheduler should avoid one tenant monopolizing all workers.

---

## 89. Retry Budget

Each job/request has bounded retry policy.

---

## 90. Retry Storm Protection

If failing dependency causes thousands of retries:

```text
circuit breaker
backoff
jitter
shared cooldown
```

---

## 91. Global Cooldown

Server may publish:

```text
retry after 30s
```

to fleet.

Clients still jitter around it.

---

## 92. Retry Token

Optional future mechanism:

```text
server-issued retry slot/token
```

Not needed initially.

---

## 93. Snapshot Herd

After epoch change, thousands rebootstrap.

Mitigation:

```text
shared immutable snapshot
CDN
jitter
per-tenant bootstrap concurrency
```

---

## 94. Bootstrap Admission

Server authorizes manifest cheaply.

Large chunk delivery offloaded where possible.

---

## 95. Manifest Request Limit

Even cheap endpoints need limits during storms.

---

## 96. Import Isolation

Part 09 live bulk import may generate huge journal.

Run with:

```text
Bulk class
tenant quota
journal backlog monitoring
```

---

## 97. Publication Throttle

Bulk import can pause between batches if:

```text
replication lag
journal consumer lag
DB latency
```

cross thresholds.

---

## 98. Feedback Controller

Simple controller:

```text
if p95 DB latency high:
    reduce bulk concurrency

if healthy:
    slowly increase
```

---

## 99. Avoid ML Scheduler

Deterministic simple control is easier to reason about.

---

## 100. Load Signals

Useful signals:

```text
DB pool utilization
DB transaction latency
CPU utilization
memory pressure
queue depth
replica lag
disk I/O
network bandwidth
```

---

## 101. LoadState

```rust
pub enum LoadState {
    Healthy,
    Elevated,
    Overloaded,
    Critical,
}
```

---

## 102. Load Policy

Each state changes:

```text
admission rate
bulk concurrency
maintenance execution
batch hints
```

---

## 103. Hysteresis

Avoid flapping between states.

Use:

```text
enter threshold
lower exit threshold
minimum dwell time
```

---

## 104. Memory Pressure

If memory high:

```text
reduce concurrent decode
disable large in-memory compression batches
shed bulk work
```

---

## 105. Disk Pressure

If server disk low:

```text
pause snapshot builds/import staging
```

but preserve core write availability if safe.

---

## 106. Client Disk Pressure

Part 06/10:

```text
compact queue
pause optional bootstrap
surface storage required
```

---

## 107. Overload Status Endpoint

Admin health can expose:

```text
load state
queue depths
limits
rejection rates
```

not necessarily public.

---

## 108. Readiness

Should server remain ready under overload?

Usually:

```text
yes, if it can still serve bounded traffic
```

Do not remove all nodes from load balancer just because they are busy.

---

## 109. Readiness Failure

Fail readiness when:

```text
cannot safely serve core work
DB unavailable
authority fence lost
critical dependency unavailable
```

---

## 110. Load Balancer Interaction

If every node becomes unready under ordinary high load:

```text
traffic may thrash
```

Prefer explicit 429/503.

---

## 111. Adaptive Concurrency Limit

Potential algorithm based on latency gradient.

Initial recommendation:

```text
fixed safe limits
+
operator tuning
```

before adaptive logic.

---

## 112. Per-Tenant Concurrency

Example:

```text
global 200 sync exchanges
tenant max 40
```

prevents one tenant taking all slots.

---

## 113. Small Tenant Protection

Weighted fairness should ensure small tenants still get service.

---

## 114. Noisy Neighbor Detection

Metrics:

```text
tenant request share
CPU share
DB time
rejection rate
```

Use tenant ID in internal logs, not high-cardinality metrics unless controlled.

---

## 115. Abuse Resistance

Part 27 threat model will expand.

Admission protects against:

```text
oversized batches
complex DAGs
connection floods
expensive filter requests
```

---

## 116. Scope Abuse

Part 07 limits:

```text
max subscribed scopes
filter complexity
```

---

## 117. Anti-Entropy Abuse

Integrity endpoints can be expensive.

Require:

```text
auth
rate limit
maintenance budget
```

---

## 118. Audit Export Abuse

Part 13 large audit export becomes durable background job.

---

## 119. Governance Purge

Part 14 mass purge runs maintenance/admin lane.

Can be high priority for legal/security request but still bounded.

---

## 120. Crypto Work

Part 15 signing/encryption may be expensive.

KMS requests need:

```text
rate limits
batching where safe
circuit breaker
```

---

## 121. Authority Failover

Part 16 recovery may cause fleet storm.

Admission policies switch to:

```text
RecoveryMode
```

---

## 122. RecoveryMode

Could prioritize:

```text
small metadata validation
existing active users
critical operations
```

and throttle massive bootstrap starts.

---

## 123. Multi-Region

Part 17 can absorb read load regionally.

Writer overload still requires authority admission.

---

## 124. Backpressure Invariants

### AEQ-INV-LOAD001

```text
No Aequora in-memory work queue grows without an explicit configured bound.
```

### AEQ-INV-LOAD002

```text
Overload rejection occurs before authoritative mutation and therefore never creates a false committed result.
```

### AEQ-INV-LOAD003

```text
One tenant cannot consume all globally reserved capacity when tenant fairness is enabled.
```

### AEQ-INV-LOAD004

```text
Slow live clients cannot consume unbounded server memory.
```

### AEQ-INV-LOAD005

```text
Bulk and maintenance work cannot permanently starve critical/interactive work.
```

### AEQ-INV-LOAD006

```text
Retry guidance never removes client-side jitter/backoff requirements.
```

---

## 125. Additional Invariants

### AEQ-INV-LOAD007

```text
Required durable work is never represented only by an in-memory scheduling queue.
```

### AEQ-INV-LOAD008

```text
Consistency is never silently weakened solely because the system is overloaded.
```

### AEQ-INV-LOAD009

```text
Admission limits are enforced before entering resource domains they are intended to protect.
```

---

## 126. Test — Reconnect Storm

Simulate:

```text
100k clients reconnecting
```

Assert:

```text
bounded memory
bounded DB concurrency
controlled 429/503
eventual recovery
```

---

## 127. Test — Hot Tenant

Tenant A sends 90% traffic.

Tenant B sends 10%.

Expected:

```text
B still receives service
```

under configured fairness.

---

## 128. Test — Slow DB

Inject DB latency.

Expected:

```text
admission tightens
bulk reduced
queue remains bounded
retry responses increase
```

---

## 129. Test — Slow Client

Client reads response extremely slowly.

Expected:

```text
response/body budget enforced
connection eventually canceled
no unbounded server buffer
```

---

## 130. Test — Oversized Batch

Client sends:

```text
too many ops
too many bytes
too deep DAG
```

Expected:

```text
rejected before expensive execution
```

---

## 131. Test — Snapshot Flood

Many clients request bootstrap.

Expected:

```text
same snapshot reused
manifest admission bounded
chunk delivery offloaded
```

---

## 132. Test — Bulk Import + Interactive Sync

Run large import while normal users sync.

Expected:

```text
interactive latency remains within target
bulk throughput reduces as needed
```

---

## 133. Test — Retry Storm

Dependency fails.

Thousands of jobs retry.

Expected:

```text
circuit breaker
jitter
bounded retries
```

---

## 134. Chaos Test

Inject simultaneously:

```text
region failover
DB latency
live reconnect storm
snapshot demand
```

Validate graceful degradation.

---

## 135. Load Testing

Measure saturation curves.

Important:

```text
throughput
p50/p95/p99 latency
rejection rate
DB utilization
CPU
memory
queue depth
```

---

## 136. Capacity Planning

Find:

```text
safe operating point
```

not maximum possible throughput.

---

## 137. Headroom

Operate below saturation.

Example:

```text
target steady-state < 70% of measured bottleneck
```

actual policy deployment-specific.

---

## 138. SLO-Based Admission

If p99 latency violates SLO:

```text
reduce low-priority admission
```

before total collapse.

---

## 139. Brownout Mode

Under severe overload, disable optional features.

Examples:

```text
presence
background integrity
analytics refresh
optional snapshot build
rich diagnostics
```

Core sync remains.

---

## 140. BrownoutPolicy

```rust
pub struct BrownoutPolicy {
    pub disable_presence: bool,
    pub pause_maintenance: bool,
    pub pause_bulk_builds: bool,
}
```

---

## 141. Do Not Disable Security

Never brown out:

```text
authorization
revocation checks
required audit
integrity needed for commit
```

---

## 142. Load Shedding Response Semantics

Tell client:

```text
retryable
non-retryable
suggested delay
```

---

## 143. Error Taxonomy

```rust
pub enum OverloadError {
    RetryableBusy { retry_after: Option<Duration> },
    TenantQuotaExceeded,
    RequestTooLarge,
    TooManyDependencies,
    ConcurrencyLimit,
}
```

---

## 144. Client UI

User should usually see:

```text
Sync delayed
Changes are saved locally
```

not raw 503.

---

## 145. Critical Operation UI

If operation requires immediate server confirmation:

```text
Server busy; not yet confirmed
```

Avoid implying success.

---

## 146. Admin UI

Show:

```text
load state
rejection rate
top work classes
queue depths
DB pool
```

---

## 147. Metrics

Global:

```text
admission_allowed_total
admission_rejected_total
inflight_requests
queue_depth
load_state
db_permits_in_use
cpu_permits_in_use
```

---

## 148. Per-Class Metrics

```text
admission_rejected_total{class="bulk"}
```

Low cardinality.

---

## 149. Tenant Metrics

Prefer top-N diagnostic views rather than unbounded per-tenant Prometheus labels.

---

## 150. Tracing

Trace:

```text
admission wait
DB wait
execution time
response encode
```

This identifies bottleneck.

---

## 151. Queue Wait Histogram

Important:

```text
queue_wait_duration
```

High wait means saturation even before rejection spikes.

---

## 152. Alerting

Alert on:

```text
sustained overload
queue near capacity
DB permits saturated
rejection spike
retry storm
bulk starving
```

---

## 153. Runbooks

Operators need:

```text
how to lower bulk concurrency
how to disable optional work
how to raise safe limits
how to identify noisy tenant
how to recover after storm
```

---

## 154. Configuration

Example:

```ron
admission: (
    global: (
        sync_inflight: 200,
        sync_queue: 500,
    ),

    tenant: (
        max_sync_inflight: 40,
    ),

    classes: (
        interactive_reserved: 40,
        bulk_max: 20,
        maintenance_max: 5,
    ),

    requests: (
        max_ops: 1000,
        max_encoded_bytes: 4194304,
        max_dependency_edges: 5000,
    ),

    overload: (
        retry_after_ms: 1000,
        brownout_enabled: true,
    ),
)
```

---

## 155. Static Defaults

Ship conservative defaults.

Production operators tune from load tests.

---

## 156. Auto-Tuning

Future optional feature.

Do not require for correctness.

---

## 157. Module Layout

```text
aequora-admission/
├── controller.rs
├── permit.rs
├── work.rs
├── tenant.rs
├── fairness.rs
├── limiter.rs
├── load_state.rs
├── brownout.rs
└── errors.rs
```

Server:

```text
aequora-server/
└── admission/
    ├── middleware.rs
    ├── sync.rs
    ├── database.rs
    ├── cpu.rs
    └── metrics.rs
```

---

## 158. Axum Integration

Request flow:

```text
connection limit
↓
body/header limit
↓
authentication
↓
tenant/work classification
↓
admission permit
↓
decode/validate
↓
DB/CPU sub-permits
↓
execute
```

---

## 159. Background Worker Integration

Worker:

```text
claim durable job
↓
acquire class/tenant permit
↓
execute bounded batch
↓
checkpoint
↓
release
```

---

## 160. Client Scheduler Integration

Part 06 client reacts to overload:

```text
reduce batch
increase backoff
coalesce triggers
respect Retry-After
```

---

## 161. Live Integration

Part 08:

```text
disconnect/coalesce slow connections
```

---

## 162. Snapshot Integration

Part 10:

```text
build and transfer budgets
```

---

## 163. Import Integration

Part 09:

```text
bulk lane
```

---

## 164. Governance Integration

Part 14:

```text
chunked purge
maintenance/admin lane
```

---

## 165. Crypto Integration

Part 15:

```text
bounded KMS/signing/encryption concurrency
```

---

## 166. Authority Recovery Integration

Part 16:

```text
RecoveryMode priorities
```

---

## 167. Regional Integration

Part 17:

```text
read load distributed
authority write budget preserved
```

---

## 168. Completion Criteria

Part 18 is complete when:

```text
[ ] all major resource domains have explicit bounds
[ ] AdmissionController defined
[ ] global + tenant + class limits defined
[ ] bounded queues defined
[ ] server-derived priority defined
[ ] fairness/starvation prevention defined
[ ] retry-after/backoff integration defined
[ ] reconnect-storm behavior defined
[ ] bulk/snapshot/blob isolation defined
[ ] DB/CPU permits defined
[ ] load state/brownout defined
[ ] request/dependency size limits defined
[ ] durable-vs-in-memory work distinction defined
[ ] overload errors defined
[ ] load/chaos tests defined
[ ] overload correctness invariants added
```

---

## 169. Final Architecture

```text
                         INCOMING WORK
                              │
                              ▼
                       Cheap Validation
                              │
                              ▼
                    Admission Controller
                              │
               ┌──────────────┼──────────────┐
               ▼              ▼              ▼
           Global Budget   Tenant Budget   Work Class
               │              │              │
               └──────────────┼──────────────┘
                              ▼
                        Admission Permit
                              │
               ┌──────────────┼──────────────┐
               ▼              ▼              ▼
             DB Permit      CPU Permit     I/O Permit
               │              │              │
               └──────────────┼──────────────┘
                              ▼
                           Execute
                              │
                              ▼
                           Commit

If no safe capacity:

                         REJECT / DEFER
                              │
                              ▼
                      Retry-After + Jitter
                              │
                              ▼
                     Client Durable Outbox
```

The architectural principle is:

> **Aequora should preserve correctness by refusing excess work before overload turns into cascading failure.**

With bounded queues, hierarchical admission, per-tenant fairness, work-class isolation, retry coordination, brownout, and explicit load-state handling, Aequora can remain responsive and predictable under reconnect storms, bulk workloads, hot tenants, and partial infrastructure failure.
