# Aequora Sync — Part 47

# Benchmarking, Performance Regression, Capacity Planning, Workload Modeling, Scalability Testing, and Production Sizing Architecture

## 1. Purpose

Aequora's performance engineering must answer concrete production questions:

```text
How many operations per second can one authority sustain?
How many concurrently syncing clients can the system support?
What happens when one tenant becomes hot?
How large can an outbox become before client UX degrades?
How long does a 20 GB bootstrap take?
How much PostgreSQL capacity is required for 100,000 active clients?
Where is p99 latency spent?
What is the cost of observability, encryption, compression, and integrity checking?
How does SQLite compare with Stoolap for the certified local workload?
At what point should we scale vertically, add API nodes, split workers, or partition tenants?
Did the latest commit make Tx B 15% slower?
```

The central rule is:

> **Performance claims are valid only when tied to a defined workload, environment, dataset, correctness profile, and reproducible measurement.**

Aequora must never gain benchmark numbers by weakening durability, authorization, audit, idempotency, validation, or conflict semantics.

---

# 2. Performance Is a System Property

Do not benchmark only individual functions.

Production latency crosses:

```text
Client DB
↓
Outbox
↓
Encoding
↓
Network
↓
Axum
↓
Authentication
↓
Authorization
↓
Planner
↓
PostgreSQL transaction
↓
Ledger
↓
Domain mutation
↓
Journal
↓
Response
↓
Client reconciliation
```

A fast serializer cannot compensate for a hot authority lock.

A fast API server cannot compensate for database saturation.

---

# 3. Measurement Layers

Aequora needs several benchmark layers:

```text
Microbenchmark
Component Benchmark
Adapter Benchmark
Protocol Benchmark
End-to-End Benchmark
Concurrency Benchmark
Scalability Test
Soak Test
Resource-Constrained Test
Fault-Degraded Performance Test
Production Capacity Model
```

Each answers a different question.

---

# 4. Microbenchmarks

Use microbenchmarks for isolated CPU/memory-sensitive operations:

```text
Postcard encode/decode
BLAKE3 hashing
zstd compression/decompression
canonicalization
HLC operations
dependency DAG planning
conflict-policy evaluation
registry lookup
Merkle digest construction
```

Microbenchmarks do not establish application capacity.

---

# 5. Component Benchmarks

Measure larger units:

```text
outbox claim
operation validation
planner batch
reconciliation
snapshot chunk processing
journal scan
consumer projection
```

---

# 6. Adapter Benchmarks

Every official adapter should have a benchmark suite.

Local:

```text
SQLite
Stoolap
ReferenceStore
```

Authority:

```text
PostgreSQL
Neon operational profile
```

---

# 7. End-to-End Benchmarks

End-to-end benchmark:

```text
local mutation
↓
Tx A
↓
sync exchange
↓
Tx B
↓
journal response
↓
Tx C
↓
confirmed local state
```

This is the most meaningful user-visible synchronization measurement.

---

# 8. Benchmark Workspace

Recommended:

```text
benches/
├── micro/
├── protocol/
├── planner/
├── reconciliation/
├── adapters/
├── server/
├── client/
├── bootstrap/
└── end_to_end/

crates/testing/
├── aequora-benchkit/
├── aequora-workload/
└── aequora-loadgen/
```

---

# 9. Benchmark Tooling

Rust benchmark tooling can include a Criterion-style harness for statistical microbenchmarks plus purpose-built Aequora load generators for distributed tests.

Do not force all scalability testing into a microbenchmark framework.

---

# 10. Benchmark Manifest

Every benchmark run should produce a machine-readable manifest.

Concept:

```rust
pub struct BenchmarkManifest {
    pub benchmark_id: BenchmarkId,
    pub scenario: ScenarioId,
    pub build: BuildId,
    pub git_commit: CommitId,
    pub environment: EnvironmentFingerprint,
    pub dataset: DatasetFingerprint,
    pub config: BenchmarkConfig,
    pub started_at: Timestamp,
}
```

---

# 11. Environment Fingerprint

Record:

```text
OS
kernel
architecture
CPU model
CPU count
memory
storage class
filesystem
Rust compiler
build profile
database version
adapter version
PostgreSQL configuration
network topology
container/VM information
```

Without this, benchmark comparison is often misleading.

---

# 12. Build Fingerprint

Record:

```text
release/debug
LTO
codegen units
target CPU
feature flags
allocator if non-default
panic strategy
```

---

# 13. Dataset Fingerprint

Record:

```text
tenant count
entity count
journal size
ledger size
outbox size
conflict distribution
payload size distribution
dependency graph distribution
snapshot size
```

---

# 14. Benchmark Reproducibility

A benchmark should be reproducible from:

```text
scenario RON
dataset seed
release/build ID
environment description
```

---

# 15. Workload Model

A workload is not:

```text
"1000 requests/sec"
```

A real workload defines:

```text
actors
tenants
operations
payloads
dependencies
conflicts
online/offline behavior
batching
read/write ratio
network conditions
```

---

# 16. Workload Specification

Concept:

```rust
pub struct WorkloadSpec {
    pub tenants: TenantDistribution,
    pub clients: ClientDistribution,
    pub operations: OperationMix,
    pub payloads: PayloadDistribution,
    pub connectivity: ConnectivityModel,
    pub conflicts: ConflictModel,
    pub duration: Duration,
}
```

---

# 17. RON Workload File

Example:

```ron
(
    name: "school_standard",
    tenants: 100,
    clients: 10_000,

    operation_mix: {
        "attendance.record": 0.45,
        "student.update": 0.15,
        "fee.record": 0.10,
        "read.sync": 0.30,
    },

    payload_bytes: (
        p50: 400,
        p95: 2_000,
        p99: 8_000,
    ),

    offline_fraction: 0.20,
)
```

---

# 18. Domain Workloads

Maintain representative workload profiles.

Examples:

```text
School ERP
Finance ERP
Inventory
Field Service
Document Metadata
Messaging Metadata
Generic CRUD
```

---

# 19. School ERP Profile

Possible behavior:

```text
morning attendance burst
fee-payment bursts
moderate student updates
many mostly-idle clients
school-local time clustering
```

---

# 20. Finance Profile

Characteristics:

```text
append-heavy
strong aggregate invariants
more auditing
less compaction
higher importance of deterministic execution
```

---

# 21. Inventory Profile

Characteristics:

```text
high concurrent updates
hot SKUs
warehouse connectivity loss
conflict-heavy counters/reservations
```

---

# 22. Field-Service Profile

Characteristics:

```text
long offline windows
large local outboxes
mobile constraints
bursty reconnect
attachments
```

---

# 23. Synthetic vs Trace-Replay Workloads

Use both:

```text
Synthetic
Recorded/Sanitized Production Shape
```

Synthetic is deterministic and easy to scale.

Production-shape replay catches distributions synthetic models miss.

Never replay sensitive raw production payloads into benchmark systems.

---

# 24. Arrival Models

Support:

```text
Constant
Poisson-like
Burst
Scheduled
ReconnectStorm
Ramp
Step
```

---

# 25. Closed vs Open Load Models

Closed model:

```text
fixed virtual clients
next request after prior completes
```

Open model:

```text
requests arrive independently of system latency
```

Use both.

Closed-only testing can hide overload because slow responses automatically reduce offered load.

---

# 26. Concurrency Model

Model:

```text
active clients
idle connected clients
concurrent HTTP requests
concurrent Tx B
worker concurrency
bootstrap concurrency
```

---

# 27. Client Behavior Model

A virtual client should maintain realistic:

```text
outbox
cursor
scope
batch size
retry/backoff
offline periods
```

rather than sending random stateless HTTP requests.

---

# 28. Load Generator Architecture

```text
aequora-loadgen
├── virtual_client
├── workload
├── dataset
├── network_model
├── assertions
├── metrics
└── report
```

---

# 29. Correctness Under Load

Load tests must assert:

```text
no duplicate authoritative effects
no cursor skips
no lost accepted operation
no cross-tenant leakage
no broken versions
no invalid journal ordering
```

Performance without correctness is a failed benchmark.

---

# 30. Performance Test Modes

```text
Throughput
Latency
Saturation
Scalability
Burst
Recovery
Soak
Resource
Failure
```

---

# 31. Throughput Test

Find sustained throughput while staying inside defined SLO constraints.

Example:

```text
maximum accepted operations/sec
with
p99 < target
error rate < target
```

---

# 32. Latency Test

Measure distributions:

```text
p50
p90
p95
p99
p99.9 where meaningful
max advisory
```

Never report only average latency.

---

# 33. Saturation Test

Increase load until a resource saturates.

Observe:

```text
CPU
DB pool
DB locks
disk IOPS
network
worker queue
memory
```

---

# 34. Knee Point

Identify the point where:

```text
small load increase
→
large latency increase
```

Production should operate below this knee with safety margin.

---

# 35. Burst Test

Simulate:

```text
morning login
attendance opening
mass reconnect
push notification wakeup
```

Measure recovery without unbounded queue growth.

---

# 36. Reconnect Storm

Scenario:

```text
server unavailable 30 min
↓
50,000 clients accumulate work
↓
server returns
↓
clients reconnect with jitter
```

Validate scheduler/backpressure design.

---

# 37. Soak Test

Run realistic load for:

```text
hours
days
```

to detect:

```text
memory leaks
connection leaks
journal growth issues
fragmentation
queue accumulation
performance drift
```

---

# 38. Failure-Degraded Benchmark

Measure performance while:

```text
one API node fails
object store is slow
DB replica unavailable
collector unavailable
worker restarted
```

Correctness and bounded degradation matter more than peak throughput.

---

# 39. Database Benchmark Dimensions

For PostgreSQL measure:

```text
Tx B duration
ledger lookup
journal append
version CAS
timeline lock wait
commit latency
pool wait
deadlock/serialization retry
```

---

# 40. Timeline Contention Benchmark

Critical scenarios:

```text
many tenants, low contention
one hot tenant
few hot tenants
single aggregate hotspot
```

---

# 41. Per-Tenant Timeline Benefit

Benchmark:

```text
global timeline head
vs
per-tenant timeline head
```

only in controlled architecture experiments.

Production design remains correctness-first.

---

# 42. Hot Aggregate Benchmark

Example:

```text
10,000 clients update same aggregate
```

Useful for measuring unavoidable serialization pressure.

---

# 43. Database Dataset Size

Benchmark at:

```text
small
medium
large
very large
```

because indexes and cache behavior change with scale.

---

# 44. Journal Growth Benchmark

Test with journal sizes such as:

```text
10K
1M
100M events
```

depending target scale.

Measure cursor scan behavior and index effectiveness.

---

# 45. Ledger Growth Benchmark

Duplicate lookup performance must remain predictable as operation ledger grows.

---

# 46. Retention Benchmark

Measure:

```text
retention scans
archive
GC
```

without blocking hot authority transactions.

---

# 47. Migration Benchmark

Large schema/metadata migrations need:

```text
duration
lock impact
disk amplification
rollback/recovery behavior
```

---

# 48. Neon Benchmark Profile

Neon testing should separately characterize operational effects such as:

```text
connection establishment
cold/warm behavior
network latency
pooling
branch-based test environments
```

without changing PostgreSQL semantics.

---

# 49. Local Adapter Benchmarking

SQLite and Stoolap should run the same semantic workload suite.

Measure:

```text
Tx A
Tx C
outbox insert
outbox claim
cursor update
query latency
startup/reopen
migration
large outbox
storage growth
```

---

# 50. SQLite vs Stoolap

Do not choose based on one insert benchmark.

Compare:

```text
correctness/conformance
platform support
crash behavior
transaction semantics
write latency
read latency
concurrency
database size
migration behavior
maintenance
mobile behavior
desktop behavior
```

---

# 51. Local Transaction Benchmark

Tx A:

```text
domain write
+
outbox enqueue
+
entity metadata
```

must be benchmarked as one atomic transaction.

---

# 52. Reconciliation Benchmark

Tx C benchmark includes:

```text
authoritative events
operation outcome
conflict state
cursor update
optimistic rebase
```

---

# 53. Large Outbox Benchmark

Test:

```text
100
1K
10K
100K+
```

pending operations according to plausible product scenarios.

Measure:

```text
startup
claim
dependency planning
compaction
sync
UI query
```

---

# 54. Client Startup Benchmark

Measure:

```text
open DB
validate metadata
migration check
load minimal status
render cached UI
```

Do not load the entire database at startup.

---

# 55. Mobile Benchmarking

Measure on real or representative devices:

```text
CPU
memory
battery
storage
background time
network bytes
thermal behavior
```

---

# 56. Desktop Benchmarking

Measure:

```text
startup
idle memory
multi-window
agent IPC
large local DB
large outbox
sleep/resume
```

---

# 57. Resource-Constrained Profile

Test explicit limits:

```text
2 GB RAM
slow storage
metered network
low battery
low disk
```

as appropriate to supported devices.

---

# 58. Memory Benchmarking

Track:

```text
RSS
heap
peak allocation
allocation rate
buffer sizes
queue memory
```

---

# 59. Memory Per Operation

Estimate:

```text
bytes of transient memory
per in-flight operation
```

to derive concurrency bounds.

---

# 60. Bounded Queue Validation

Load tests should prove queue size remains within configured budgets.

---

# 61. OOM Test

Push system beyond capacity.

Expected:

```text
admission rejects/defer
queues remain bounded
process survives
```

rather than uncontrolled memory growth.

---

# 62. Protocol Benchmarking

Measure:

```text
Postcard encode/decode
framing
compression
decompression
payload validation
```

across realistic payload sizes.

---

# 63. Compression Threshold

Benchmark when compression becomes beneficial.

Small payloads may become slower/larger due to compression overhead.

---

# 64. Network Profiles

Test:

```text
LAN
good broadband
4G/5G
high-latency WAN
lossy mobile
metered slow link
```

---

# 65. Network Emulation

Load harness can inject:

```text
latency
jitter
packet loss
bandwidth limits
disconnects
```

---

# 66. Sync Batch Benchmark

Vary:

```text
operations per push
events per pull
bytes per exchange
```

Measure throughput/latency/memory tradeoffs.

---

# 67. Adaptive Scheduler Benchmark

Validate AIMD/batch adaptation under changing:

```text
latency
429
503
bandwidth
power/network policy
```

---

# 68. Bootstrap Benchmark

Measure:

```text
snapshot generation
chunk compression
upload
download
verification
installation
catch-up
```

separately and end-to-end.

---

# 69. Bootstrap Dataset Classes

Example:

```text
100 MB
1 GB
10 GB
50 GB
```

where relevant.

---

# 70. Bootstrap Resume Benchmark

Interrupt at multiple points.

Measure:

```text
wasted bytes
resume time
verification cost
```

---

# 71. Snapshot Chunk Size

Benchmark chunk sizes against:

```text
memory
parallelism
resume granularity
compression
object-store overhead
```

---

# 72. Integrity Benchmark

Measure:

```text
BLAKE3 canonical digest
Merkle construction
partition comparison
repair verification
```

---

# 73. Anti-Entropy Benchmark

Test:

```text
zero divergence
small divergence
large divergence
```

The normal no-divergence path should be inexpensive.

---

# 74. Conflict Benchmark

Measure conflict detection/resolution for domain profiles.

Do not optimize by replacing domain semantics with LWW.

---

# 75. Dependency Planner Benchmark

Graph profiles:

```text
wide
deep
sparse
dense-but-valid
near configured limits
```

---

# 76. Complexity Guard

Confirm planner remains approximately:

```text
O(N + E)
```

and rejects abusive graph sizes before resource exhaustion.

---

# 77. Compaction Benchmark

Measure:

```text
queue scan
semantic merge
storage saved
CPU cost
```

for operations where compaction is valid.

Finance `Never` policy remains unchanged.

---

# 78. Crypto Benchmark

Measure:

```text
hash
signature verification
encryption/decryption
key lookup
```

under real payload distributions.

---

# 79. Security Cost Is Not Optional

Do not disable required:

```text
auth
signature
encryption
validation
audit
```

to publish better numbers.

---

# 80. Observability Overhead Benchmark

Run:

```text
telemetry disabled
normal production telemetry
high debug sampling
```

Compare:

```text
throughput
latency
CPU
memory
network
```

---

# 81. Audit Overhead

Required audit is part of normal production benchmark.

Do not benchmark authority transactions without it if production requires it.

---

# 82. Job Engine Benchmark

Measure:

```text
claim
lease
execute
retry
dead-letter
```

and backlog recovery.

---

# 83. Change Feed Benchmark

Measure:

```text
journal read
projection
consumer lag
partition scaling
snapshot+tail rebuild
```

---

# 84. Control Plane Benchmark

Admin/control APIs are not throughput-critical, but expensive commands must be bounded.

Benchmark:

```text
large diagnostics
repair planning
registry queries
consumer resets
```

---

# 85. Benchmark Isolation

Do not run performance-sensitive CI benchmarks on heavily noisy shared runners and treat small changes as regressions.

Use:

```text
dedicated runner
or
large regression thresholds
```

---

# 86. Statistical Noise

Record repeated samples.

Compare distributions rather than one run.

---

# 87. Warm-Up

Many tests require warm-up:

```text
JIT not relevant to Rust itself
but
OS page cache
DB cache
connection pools
TLS
filesystem
```

Document whether test is cold or warm.

---

# 88. Cold vs Warm Benchmarks

Both matter.

Examples:

```text
cold client startup
warm client query
cold Neon connection
warm PostgreSQL pool
```

---

# 89. Cache State

Benchmark reports must state:

```text
cold
warm
mixed
```

---

# 90. CPU Frequency and Thermal Noise

On laptops/mobile:

```text
thermal throttling
power mode
background apps
```

can distort results.

Record or control them where possible.

---

# 91. Benchmark Baseline

Each important benchmark has:

```text
baseline result
candidate result
allowed regression threshold
```

---

# 92. Regression Policy

Not every 1% change should fail CI.

Example classes:

```text
Critical hot path     5%
Important             10%
Informational         report only
```

Actual thresholds are established empirically.

---

# 93. Regression Direction

Some metrics are lower-is-better:

```text
latency
memory
CPU
```

Others higher-is-better:

```text
throughput
```

The benchmark metadata should encode this.

---

# 94. Benchmark ID

Stable IDs:

```text
AEQ-BENCH-PG-TXB-001
AEQ-BENCH-CLIENT-TXA-001
AEQ-BENCH-PROTOCOL-DECODE-001
```

---

# 95. Baseline Storage

Store benchmark results as artifacts:

```text
RON summary
JSON interoperability summary
CSV where useful
raw samples
environment manifest
```

Postcard may be used for compact internal binary artifacts.

---

# 96. CI Tiers

Tier 1 — PR:

```text
fast microbench smoke
correctness load smoke
```

Tier 2 — main/nightly:

```text
full microbench
adapter benchmarks
moderate load
```

Tier 3 — scheduled:

```text
large scalability
soak
mobile
multi-node
failure
```

Tier 4 — release:

```text
production profile
capacity certification
regression report
```

---

# 97. PR Performance Gate

Fail only on meaningful statistically supported regressions in designated critical benchmarks.

---

# 98. Nightly Trend

Track performance over time to catch slow cumulative regressions.

---

# 99. Release Performance Report

Release artifact can include:

```text
benchmark suite version
environment
baseline
candidate
significant regressions
capacity profile
```

---

# 100. Performance Change Review

A significant regression may be accepted if it buys:

```text
correctness
security
durability
important functionality
```

but must be explicit.

---

# 101. Performance Budget

Define budgets for major paths.

Example conceptual:

```text
decode budget
auth budget
planner budget
DB budget
encode budget
```

Do not prematurely hard-code arbitrary numbers.

---

# 102. Latency Decomposition

For sync:

```text
T_total =
T_network
+ T_http
+ T_decode
+ T_auth
+ T_plan
+ T_db
+ T_encode
```

Use traces/bench instrumentation to estimate each.

---

# 103. Little's Law for Capacity Reasoning

A useful relationship:

```text
Concurrency ≈ Throughput × Average Time in System
```

Use it as a sanity check, not as a substitute for load testing.

---

# 104. Capacity Unit

Define a normalized Aequora Capacity Unit only if real workload measurements justify it.

Avoid inventing a misleading universal "request unit" too early.

---

# 105. Operation Cost Classes

More useful initially:

```text
Tiny
Small
Medium
Large
Bulk
```

based on measured:

```text
DB work
payload
CPU
dependencies
```

---

# 106. Capacity Model Inputs

Server sizing model uses:

```text
active tenants
active clients
operations/client/hour
peak multiplier
batching
payload size
DB cost/op
journal bytes/op
ledger bytes/op
snapshot demand
worker demand
```

---

# 107. Peak vs Average

Capacity must size for realistic peak.

Example:

```text
average = 500 ops/s
morning peak multiplier = 6
required offered peak = 3,000 ops/s
```

---

# 108. Safety Headroom

Do not plan production at measured maximum throughput.

Concept:

```text
Provisioned Capacity
=
Measured Sustainable Capacity
× Safety Factor
```

with a factor below 1.

---

# 109. Why Headroom Matters

Needed for:

```text
bursts
failover
background work
schema changes
cache misses
noisy neighbors
unexpected growth
```

---

# 110. N+1 Capacity

HA environments should consider surviving loss of one node.

If three API nodes are required to barely handle peak, node failure immediately overloads the system.

---

# 111. Database Headroom

Database is usually the most important authority bottleneck.

Track:

```text
CPU
IO
connections
locks
WAL
storage
cache hit
transaction latency
```

---

# 112. API Node Sizing

Estimate from:

```text
request CPU
memory/in-flight request
network throughput
TLS cost
serialization
```

but validate end-to-end.

---

# 113. Worker Sizing

Separate worker classes:

```text
interactive
normal
bulk
maintenance
```

where isolation is beneficial.

---

# 114. Snapshot Capacity

Plan:

```text
snapshot generation CPU
DB read bandwidth
object-store write
client download bandwidth
```

---

# 115. Storage Growth Model

Estimate:

```text
business data
+
journal
+
ledger
+
audit
+
jobs
+
indexes
```

separately.

---

# 116. Journal Growth Formula

Concept:

```text
journal bytes/day
≈
operations/day
× events/operation
× average encoded event bytes
× storage/index amplification
```

Measure amplification instead of guessing long-term.

---

# 117. Ledger Growth Formula

Concept:

```text
ledger bytes/day
≈
unique operations/day
× average ledger record footprint
```

Retention policy changes long-term size.

---

# 118. Client Storage Growth

Estimate:

```text
replicated domain state
outbox
conflicts
metadata
snapshot staging
blob cache
```

---

# 119. Bandwidth Model

Per client:

```text
push bytes
+
pull bytes
+
protocol overhead
+
TLS overhead
+
bootstrap/blob traffic
```

---

# 120. Compression Model

Use measured compressed ratios by payload class rather than one global estimate.

---

# 121. Cost Model

Optional production sizing can estimate:

```text
DB compute
DB storage
object storage
egress
API compute
observability
backup
```

Cost is a capacity dimension.

---

# 122. Tenant Cost Attribution

For SaaS, collect bounded accounting counters separately from high-cardinality metrics.

Possible:

```text
operations
storage
blob bytes
snapshot traffic
```

Billing must not depend solely on lossy telemetry.

---

# 123. Capacity Planning Output

Generate a report:

```text
workload assumptions
peak load
tested environment
measured sustainable capacity
headroom
bottleneck
recommended topology
recommended DB size
recommended API nodes
storage growth
network estimate
risk notes
```

---

# 124. Production Sizing Profiles

Do not claim universal exact sizes.

Provide empirically validated profiles such as:

```text
Small
Medium
Large
Enterprise
```

with explicit workload assumptions.

---

# 125. Small Profile

Could eventually describe:

```text
tenant range
active client range
peak ops/sec
DB class
API node count
```

only after measurements exist.

---

# 126. Never Fabricate Capacity Numbers

Until benchmark evidence exists:

```text
Unknown
Not Yet Certified
```

is better than invented throughput.

---

# 127. Capacity Certification

Part 30 certification can attach tested capacity evidence to:

```text
adapter version
server release
hardware
DB version
workload
```

---

# 128. Scaling Trigger

Examples:

```text
DB pool p95 wait exceeds threshold
CPU sustained high
timeline lock wait increases
error-budget burn
worker lag
storage forecast
```

---

# 129. Vertical Scaling

Often first step for PostgreSQL:

```text
more CPU
more memory
faster storage
```

before adding architectural complexity.

---

# 130. Horizontal API Scaling

Useful when API CPU/network is bottleneck and DB has headroom.

---

# 131. Worker Separation

Move heavy jobs from API nodes when they interfere with interactive sync.

---

# 132. Regional Reads

Add when measured latency/residency requirements justify it.

---

# 133. Tenant Partitioning

Add when:

```text
single authority cluster capacity
residency
failure-domain
```

requirements justify it.

---

# 134. Do Not Scale by Guessing

Every topology escalation should have:

```text
measured bottleneck
expected improvement
benchmark validation
rollback plan
```

---

# 135. Load Shedding Benchmark

Test admission controller at overload.

Expected:

```text
bounded latency
controlled rejection
no memory explosion
core work protected
```

---

# 136. Fairness Benchmark

Scenario:

```text
one huge tenant
+
many small tenants
```

Validate small tenants continue making progress.

---

# 137. Priority Benchmark

Mix:

```text
Critical
Interactive
Normal
Bulk
Background
```

Verify priority without permanent starvation.

---

# 138. Starvation Test

Long-running load must prove aged lower-priority work eventually progresses when resources permit.

---

# 139. Rate-Limit Benchmark

Validate limiter itself does not become a global lock bottleneck.

---

# 140. Auth Benchmark

Measure:

```text
token validation
key lookup/cache
authorization
```

under production security configuration.

---

# 141. Registry Benchmark

Registry lookups should be effectively negligible and not query DB on every operation.

---

# 142. Handler Benchmark

Domain handlers can provide domain-specific benchmark fixtures.

---

# 143. Deterministic Replay Benchmark

Measure replay throughput separately from live authoritative execution.

Replay must not emit side effects.

---

# 144. Import Benchmark

Bulk import tests:

```text
rows/records
validation
quarantine
dependency ordering
journal generation
```

---

# 145. Export Benchmark

Measure:

```text
stream throughput
memory
DB impact
compression
```

---

# 146. Repair Benchmark

Measure anti-entropy repair without starving normal sync.

---

# 147. Multi-Region Benchmark

Measure:

```text
authority RTT
regional read latency
replica lag
snapshot mirror performance
failover recovery
```

---

# 148. Failover Capacity

After one node/region failure, remaining capacity must be known.

---

# 149. Air-Gapped Benchmark

Test representative enterprise hardware rather than assuming cloud-like storage/network behavior.

---

# 150. Virtualization Benchmark

Record whether running on:

```text
bare metal
VM
container
```

because noisy-neighbor effects matter.

---

# 151. Cloud Benchmark Variability

Cloud instances may vary.

Repeat runs and record instance class/environment.

---

# 152. Dedicated Performance Environment

For release certification, use controlled infrastructure.

---

# 153. Benchmark Database Reset

Each scenario should define:

```text
fresh DB
preloaded DB
retained journal
```

and deterministic reset/setup.

---

# 154. Benchmark Data Generator

Use seeded deterministic generation.

Concept:

```rust
DatasetSeed(u64)
```

---

# 155. Data Realism

Generate realistic:

```text
string lengths
payload distributions
relationships
hot keys
tenant skew
```

not only uniform random data.

---

# 156. Zipf-Like Hotness

Many real systems have:

```text
few hot entities
many cold entities
```

Include skewed distributions.

---

# 157. Tenant Skew

Model:

```text
many small tenants
few large tenants
```

---

# 158. Offline Skew

Some clients may sync continuously; others reconnect after days.

---

# 159. Conflict Distribution

Model realistic conflicts instead of making every update conflict-free.

---

# 160. Payload Distribution

Use percentile distributions, not one fixed payload size.

---

# 161. Dependency Distribution

Most operations may have no dependencies, while some workflows have chains.

---

# 162. Benchmark Assertions

Every scenario can declare:

```rust
pub struct PerformanceAssertions {
    pub max_error_rate: Option<f64>,
    pub max_p99_latency: Option<Duration>,
    pub max_memory: Option<Bytes>,
    pub min_throughput: Option<f64>,
}
```

Correctness assertions remain mandatory and separate.

---

# 163. Result Schema

Concept:

```rust
pub struct BenchmarkResult {
    pub manifest: BenchmarkManifest,
    pub throughput: ThroughputStats,
    pub latency: LatencyStats,
    pub resources: ResourceStats,
    pub correctness: CorrectnessResult,
}
```

---

# 164. Comparison Schema

```rust
pub struct RegressionComparison {
    pub benchmark_id: BenchmarkId,
    pub baseline: Measurement,
    pub candidate: Measurement,
    pub delta: f64,
    pub significance: Significance,
    pub decision: RegressionDecision,
}
```

---

# 165. Regression Decision

```text
Pass
Warn
Fail
AcceptedWithJustification
Inconclusive
```

---

# 166. Inconclusive Results

Noise or environment drift should produce:

```text
Inconclusive
```

not false precision.

---

# 167. Benchmark Artifacts

Store:

```text
manifest.ron
summary.ron
summary.json
raw samples
system metrics
trace sample
flamegraph where requested
report.md
```

---

# 168. Flamegraphs

Use flamegraphs for CPU diagnosis after a benchmark shows a problem.

Do not optimize from flamegraphs without representative workloads.

---

# 169. Heap Profiling

Use heap profiling when:

```text
allocation rate
peak RSS
memory growth
```

is problematic.

---

# 170. PostgreSQL EXPLAIN

For slow DB paths, capture:

```text
EXPLAIN
EXPLAIN ANALYZE in safe test environments
```

with representative data.

---

# 171. Lock Diagnostics

Capture lock wait/contended transaction evidence for authority hotspots.

---

# 172. Tokio Diagnostics

Measure:

```text
task latency
blocking
runtime saturation
```

where tooling supports it.

---

# 173. Blocking Detection

No blocking embedded DB call should accidentally execute on latency-sensitive async runtime threads if adapter API is synchronous.

---

# 174. Rayon Diagnostics

CPU-heavy work should have bounded Rayon concurrency.

Benchmark interference with Tokio/API work.

---

# 175. CPU Affinity

Only use affinity/pinning after measurement demonstrates benefit.

Do not make it baseline complexity.

---

# 176. Allocator Experiments

Alternative allocators are experimental optimizations until benchmarked across supported platforms.

---

# 177. Unsafe Optimization

Any unsafe optimization requires:

```text
measured benefit
soundness review
tests/fuzzing
```

and must not weaken the pure-safe-Rust preference without compelling evidence.

---

# 178. Benchmark Governance

Changes to canonical benchmark scenarios require review because changing workload can hide regressions.

---

# 179. Scenario Version

Each workload has:

```text
ScenarioVersion
```

---

# 180. Baseline Compatibility

Compare results only when:

```text
scenario compatible
environment sufficiently comparable
build settings comparable
```

---

# 181. Benchmark Registry

Part 29 registry may govern durable benchmark IDs and scenario IDs once externally referenced.

---

# 182. Performance ADR

Major architecture performance decisions should record:

```text
problem
measurement
alternatives
result
tradeoff
```

---

# 183. Optimization Workflow

```text
observe
↓
reproduce
↓
profile
↓
hypothesize
↓
change one thing
↓
benchmark
↓
verify correctness
↓
retain or revert
```

---

# 184. Anti-Pattern: Optimize Before Measuring

Avoid:

```text
custom allocator
unsafe zero-copy
complex sharding
manual SIMD
cache layer
```

without evidence.

---

# 185. Anti-Pattern: Benchmark Toy Payloads

A 16-byte synthetic operation does not represent an ERP workload.

---

# 186. Anti-Pattern: Average Only

Average hides tail latency.

---

# 187. Anti-Pattern: Unlimited Load Generator

If load generator itself saturates, results are invalid.

Observe generator CPU/network too.

---

# 188. Anti-Pattern: Benchmark Without Audit/Auth

If production uses them, benchmark with them enabled.

---

# 189. Anti-Pattern: Compare Different Hardware

Do not claim a code regression when environment changed.

---

# 190. Anti-Pattern: Throughput at Any Latency

A system producing 100K ops/sec with 30-second p99 may be unusable.

---

# 191. Anti-Pattern: Hide Rejections

Throughput should distinguish:

```text
offered
admitted
accepted
rejected
completed
```

---

# 192. Offered Load

Measure load generator's attempted work.

---

# 193. Admitted Load

Measure requests accepted by admission controller.

---

# 194. Authoritative Throughput

Measure committed authoritative operations.

This is often the most meaningful server throughput.

---

# 195. Goodput

Define useful successful work:

```text
valid authoritative operations completed
```

rather than raw HTTP requests.

---

# 196. Benchmark Security

Load tools must not accidentally target production unless explicitly authorized.

Use environment guards.

---

# 197. Production Load Testing

If ever used:

```text
explicit allowlist
rate cap
tenant isolation
operator confirmation
```

---

# 198. Sensitive Data

Benchmark datasets must be synthetic or sanitized.

---

# 199. Performance Telemetry Privacy

Capacity metrics should not expose business payloads.

---

# 200. Production Capacity Feedback

Compare modeled capacity with real production:

```text
forecast
vs
observed
```

and refine models.

---

# 201. Capacity Forecasting

Use trends for:

```text
active clients
operations
journal storage
DB utilization
object storage
```

---

# 202. Forecast Horizon

Operational planning can project:

```text
30
90
180 days
```

depending business needs.

---

# 203. Capacity Alert

Ticket when forecast crosses safe capacity before expected procurement/scale lead time.

---

# 204. Performance Budget in Code Review

For hot-path changes, PR description should state expected performance impact when material.

---

# 205. Benchmark CLI

Suggested:

```text
aequora bench list
aequora bench run <scenario>
aequora bench compare <baseline> <candidate>
aequora bench report
aequora load run <workload.ron>
aequora capacity estimate <workload.ron>
```

---

# 206. Safety of `capacity estimate`

It must clearly distinguish:

```text
Measured
Interpolated
Extrapolated
Unknown
```

---

# 207. Never Present Extrapolation as Certification

Extrapolated capacity is planning guidance, not tested capacity.

---

# 208. Capacity Report Example

```text
Scenario: school_standard/v3
Authority: PostgreSQL 18 / tested profile
Clients: 25,000
Peak offered load: ...
Measured sustainable goodput: ...
p99: ...
Primary bottleneck: ...
Recommended headroom: ...
Evidence bundle: ...
```

Actual numeric values appear only after tests.

---

# 209. Benchmark Invariants

## AEQ-INV-BENCH001

```text
No benchmark may disable or weaken correctness-critical durability, idempotency, authorization, validation, audit, or conflict semantics merely to improve performance results.
```

## AEQ-INV-BENCH002

```text
Every published performance result is bound to a workload version, dataset, build, configuration, and environment fingerprint.
```

## AEQ-INV-BENCH003

```text
Performance tests verify core synchronization correctness in addition to throughput and latency.
```

## AEQ-INV-BENCH004

```text
Unbounded offered load results in bounded admission, queues, and memory rather than uncontrolled resource growth.
```

## AEQ-INV-BENCH005

```text
Capacity recommendations contain explicit headroom and never use measured saturation throughput as normal production capacity.
```

## AEQ-INV-BENCH006

```text
Regression gates compare statistically meaningful measurements from compatible scenarios and environments and may report Inconclusive rather than false precision.
```

## AEQ-INV-BENCH007

```text
Local adapter comparisons such as SQLite versus Stoolap use the same semantic Tx A/Tx C workloads and conformance requirements.
```

## AEQ-INV-BENCH008

```text
Client and server benchmarks include representative offline, reconnect, conflict, dataset-size, and tail-latency behavior rather than only ideal steady-state traffic.
```

## AEQ-INV-BENCH009

```text
Capacity estimates clearly distinguish measured, interpolated, extrapolated, and unknown values.
```

## AEQ-INV-BENCH010

```text
Optimization is accepted only after representative measurement and correctness verification; architectural complexity is not introduced solely from intuition.
```

---

# 210. Recommended v1 Benchmark Suite

Start with:

```text
Postcard encode/decode
Tx A SQLite
Tx A Stoolap
Tx C SQLite
Tx C Stoolap
PostgreSQL Tx B
ledger duplicate path
journal scan
timeline contention
planner
reconciliation
sync exchange E2E
large outbox
bootstrap
reconnect storm
bounded overload
```

---

# 211. Recommended v1 Workloads

```text
generic_small
generic_medium
school_standard
finance_append_only
mobile_offline
reconnect_storm
hot_tenant
bootstrap_large
```

---

# 212. Recommended CI

PR:

```text
microbench smoke
critical regression subset
correctness load smoke
```

Nightly:

```text
full microbench
SQLite/Stoolap/PostgreSQL adapter suite
moderate E2E load
```

Weekly:

```text
soak
reconnect storm
large bootstrap
failure degradation
```

Release:

```text
controlled production-profile capacity run
regression report
capacity evidence
```

---

# 213. Production Sizing Decision Tree

```text
Is latency high?
├── API CPU high -> optimize/scale API
├── DB pool wait high -> inspect DB/pool/query shape
├── timeline lock high -> hot tenant/authority contention
├── worker backlog high -> isolate/scale workers
└── network RTT high -> consider regional read/artifact strategy
```

Then measure again.

---

# 214. Scaling Philosophy

Aequora should prefer the simplest scaling step that removes the measured bottleneck:

```text
better query
before
more cache

bigger PostgreSQL
before
authority sharding

more API nodes
only when
API is bottleneck

regional reads
before
multi-writer authority
```

---

# 215. Final Architecture

```text
             Workload Definitions
                     |
                     v
              Aequora Loadgen
                     |
          +----------+----------+
          |                     |
          v                     v
      Client Fleet          Server Fleet
          |                     |
          v                     v
   SQLite / Stoolap         PostgreSQL
          |                     |
          +----------+----------+
                     |
                     v
             Measurement Layer
          /          |          \
         v           v           v
      Latency     Resources    Correctness
         \           |           /
          +----------+----------+
                     |
                     v
              Benchmark Artifact
                     |
          +----------+----------+
          |                     |
          v                     v
 Regression Analysis      Capacity Model
          |                     |
          v                     v
       CI Gate           Production Sizing
```

---

# 216. Final Recommendation

Aequora should not chase a universal headline such as:

```text
"1 million operations per second"
```

because that number is meaningless without semantics and workload.

Instead, Aequora should be able to say:

```text
For this workload,
with this database,
on this hardware,
with these correctness/security features enabled,
this release sustained this goodput,
at these latency percentiles,
with this resource usage,
while passing these correctness assertions.
```

That is a trustworthy engineering performance claim.

The optimization hierarchy remains:

```text
correctness
↓
measurement
↓
bottleneck identification
↓
simple optimization
↓
repeatable benchmark
↓
capacity evidence
↓
production sizing
```

> **Benchmark the real semantic system, size from measured sustainable goodput rather than theoretical peak, and scale only in response to demonstrated bottlenecks.**

---

## Next

**Part 48 — Testkit, Verification Infrastructure, Fault Injection, Property Testing, Model Testing, Integration Testing, End-to-End Testing, and Release Quality Gates Architecture**
