# Aequora Sync — Part 19

# Performance Engineering, Memory Architecture, and Zero-Copy Boundaries

## 1. Purpose

Aequora is intended to be:

```text
production-grade
cross-platform
local-first
database-agnostic
high-throughput
resource-efficient
```

Correctness comes first, but correctness alone is insufficient if the system:

```text
allocates excessively
copies large payloads repeatedly
blocks Tokio workers
spawns unbounded tasks
deserializes entire snapshots into memory
thrashes CPU caches
overuses locks
holds database rows longer than necessary
creates avoidable allocator pressure
```

Aequora therefore needs an explicit performance and memory architecture.

The central rule is:

> **Optimize data movement, allocation, and concurrency around measured hot paths while preserving semantic boundaries and crash-safety invariants.**

---

# 2. Goals

The performance architecture should provide:

```text
bounded memory
predictable allocation
low-copy protocol handling
streaming snapshots
efficient batching
cache-friendly data structures
clear Tokio/Rayon separation
low synchronization overhead
benchmarkable hot paths
performance regression detection
resource-constrained client compatibility
```

---

# 3. Non-Goals

Performance engineering is not:

```text
unsafe Rust everywhere
premature micro-optimization
custom allocator required by default
manual SIMD for ordinary code
zero allocations at any cost
single benchmark number
```

Aequora should remain understandable and maintainable.

---

# 4. Performance Priorities

Optimize in this order:

```text
1. Algorithmic complexity
2. I/O volume
3. Database query shape
4. Allocation volume
5. Data copying
6. Lock contention
7. Cache locality
8. Micro-optimizations
```

---

# 5. Performance Invariants

Performance changes must never violate:

```text
idempotency
transaction atomicity
cursor safety
authorization
scope isolation
audit requirements
epoch correctness
```

---

# 6. Hot Paths

Primary hot paths:

```text
operation enqueue
sync request encode
server decode
validation
dependency planning
authoritative execution
journal encode
response encode
client reconciliation
snapshot stream
anti-entropy hashing
```

Measure each separately.

---

# 7. Memory Budget

Every runtime profile should have explicit memory budgets.

Examples:

```text
maximum request bytes
maximum decoded batch bytes
maximum response bytes
maximum snapshot chunk bytes
maximum ready queue bytes
maximum pending decode records
```

---

# 8. MemoryBudget

Conceptually:

```rust
pub struct MemoryBudget {
    pub request_bytes: usize,
    pub decode_bytes: usize,
    pub response_bytes: usize,
    pub snapshot_pipeline_bytes: usize,
    pub cache_bytes: usize,
}
```

---

# 9. Bounded Pipelines

Prefer:

```text
producer
↓ bounded channel
consumer
```

rather than accumulating complete datasets.

---

# 10. Bytes Ownership

Use:

```rust
bytes::Bytes
```

for immutable shared byte payloads where it avoids copies.

Good candidates:

```text
encoded operation payload
journal payload
snapshot frame
HTTP body chunks
```

---

# 11. BytesMut

Use:

```rust
BytesMut
```

during controlled frame construction.

Freeze to:

```rust
Bytes
```

when immutable.

---

# 12. Avoid Vec Cloning

Do not repeatedly:

```rust
payload.clone()
```

when `Bytes` reference-counted sharing is sufficient.

---

# 13. Typed Boundary

Core domain logic should receive typed values.

Do not carry raw bytes deep into business handlers solely for zero-copy ambitions.

---

# 14. Zero-Copy Boundary Principle

Use zero-copy mainly across:

```text
network framing
codec layers
snapshot streaming
blob transfers
```

Not as an excuse to weaken type safety.

---

# 15. Borrowed Decode

Where Postcard supports it, decode borrowed data from stable input buffer.

Example:

```rust
#[derive(Deserialize)]
struct Borrowed<'a> {
    name: &'a str,
}
```

Useful for:

```text
validate/read-only processing
```

---

# 16. Borrow Lifetime Rule

Borrowed decoded values must never outlive backing buffer.

Avoid complex lifetimes leaking across async boundaries.

---

# 17. Owned Conversion

For durable/domain state:

```text
validate borrowed input
↓
convert required fields to owned domain values
```

This is usually the right tradeoff.

---

# 18. Postcard Framing

Network messages need explicit framing.

Recommended:

```text
length prefix
message kind
protocol version
payload
checksum where needed
```

---

# 19. Frame Limits

Before allocating:

```text
read declared frame length
validate against limit
```

Reject oversized frame early.

---

# 20. Streaming Decode

For large batches/snapshots:

```text
frame
↓
decode one record
↓
process
↓
release
```

rather than decode giant vector.

---

# 21. Request Batch Tradeoff

Small batch:

```text
more overhead
lower memory
```

Large batch:

```text
better throughput
higher latency/memory
```

Part 06 scheduler dynamically balances.

---

# 22. Server Batch Limits

Hard bounds:

```text
operations count
encoded bytes
dependency edges
aggregate count
```

---

# 23. Preallocation

When sizes are known:

```rust
Vec::with_capacity(n)
```

Use in hot paths.

Do not over-preallocate from untrusted client values before bounds validation.

---

# 24. SmallVec

Use:

```rust
smallvec::SmallVec
```

for structures usually containing a few elements.

Good candidates:

```text
dependencies
routing partitions
generated child IDs
```

Only where measurement supports it.

---

# 25. ArrayVec

For hard small upper bounds, `ArrayVec` may eliminate heap allocation.

Use cautiously; large inline arrays increase object size.

---

# 26. Box Large Enums

Rust enum size equals largest variant.

If one rare variant is huge:

```rust
Box<LargeVariant>
```

to keep common enum size small.

---

# 27. Enum Layout Audit

Use tooling/tests to inspect:

```text
size_of::<OperationEnvelope>()
size_of::<Event>()
```

Large hot structs deserve review.

---

# 28. Newtypes Cost

Most primitive newtypes are zero runtime cost.

Keep strong typing.

Do not remove newtypes for imagined performance.

---

# 29. String Strategy

Prefer stable IDs/enums over repeated strings.

Use strings for:

```text
user content
diagnostics
configuration
```

not hot operation-kind lookup.

---

# 30. Interning

Avoid global string interning unless profiling shows benefit.

It adds lifecycle/lock complexity.

---

# 31. Numeric Registry IDs

Use:

```text
OperationKind(u32)
EntityType(u32)
AuditActionId(u32)
```

for hot registry dispatch.

---

# 32. Registry Lookup

Prefer:

```text
dense vector
perfect-ish map
hash map built once
```

depending on ID density.

---

# 33. Static Registry

Operation registry should be constructed once at startup and treated immutable.

Allows lock-free shared reads via:

```rust
Arc<Registry>
```

---

# 34. Lock Strategy

Prefer:

```text
immutable shared data
message passing
transactional DB coordination
```

over broad mutexes.

---

# 35. Mutex Scope

If mutex required:

```text
hold for smallest possible critical section
```

Never hold ordinary mutex across `.await`.

---

# 36. Async Mutex

Use async-aware mutex only when protected state genuinely spans await.

Prefer redesigning ownership first.

---

# 37. RwLock

Do not assume `RwLock` is faster.

Under frequent writes it can be worse than mutex.

Measure.

---

# 38. Atomics

Use atomics for simple:

```text
flags
counters
generation numbers
```

where memory ordering is understood.

Do not encode complex state machines in atomics without need.

---

# 39. Tokio Boundary

Tokio handles:

```text
network I/O
database I/O
timers
orchestration
```

---

# 40. Never Block Tokio Worker

Do not run heavy:

```text
compression
hashing of multi-GB data
CPU transforms
blocking filesystem APIs
```

directly on Tokio executor.

---

# 41. Rayon Boundary

Rayon handles CPU-heavy parallel work:

```text
canonical hashing
compression
bulk transform
snapshot encoding
```

---

# 42. Dedicated Pool

Use a dedicated Rayon pool with bounded thread count.

Avoid consuming every CPU core if UI/database work also needs CPU.

---

# 43. Rayon Threshold

Parallelization has overhead.

Only use Rayon above measured threshold.

Small payloads remain serial.

---

# 44. spawn_blocking

Use `tokio::task::spawn_blocking` for blocking operations that do not fit Rayon well.

Bound externally.

---

# 45. CPU Admission

Part 18 provides permits before entering CPU pool.

---

# 46. NUMA

Do not optimize for NUMA initially.

If deploying very large servers, benchmark before adding affinity logic.

---

# 47. Thread Count

Default:

```text
Tokio worker threads ≈ runtime defaults
Rayon workers bounded separately
```

Expose config for large deployments.

---

# 48. Database Performance

Most server performance problems will often be database-shaped.

Focus on:

```text
correct indexes
bounded transactions
query batching
prepared statements
avoiding N+1 queries
short lock duration
```

---

# 49. One Transaction Per Operation?

Not necessarily.

A batch may execute multiple independent operations, but correctness/locking may favor:

```text
per aggregate
or bounded group
```

Measure.

---

# 50. Transaction Size

Too large:

```text
locks
WAL
rollback cost
```

Too small:

```text
round trips
commit overhead
```

Use bounded adaptive batch policies.

---

# 51. Prepared Statements

PostgreSQL adapter should use SQLx/prepared query support.

Do not build SQL strings repeatedly in hot path.

---

# 52. Query Shape

Prefer explicit indexed predicates:

```text
tenant_id
entity_id
version
sequence
```

---

# 53. Cursor Query

Critical index:

```text
journal(tenant/scope routing, sequence)
```

depending on schema.

---

# 54. Outbox Query

Client outbox index should efficiently fetch:

```text
state
next_retry
priority
local_seq
```

---

# 55. Avoid SELECT *

Fetch only required fields for hot paths.

---

# 56. Projection Reads

Read models can denormalize to reduce expensive joins.

Keep source authority separate.

---

# 57. Cache Policy

Cache only:

```text
expensive
frequently reused
safe-to-stale
```

data.

Do not cache everything.

---

# 58. Cache Types

Potential:

```text
operation registry
scope resolution
authorization metadata
snapshot manifest
projection metadata
```

---

# 59. Cache Correctness

Every cache needs:

```text
key
version/generation
invalidator
staleness policy
```

---

# 60. Cache Epoch

Include Part 16 AuthorityEpoch where timeline matters.

---

# 61. Tenant Cache Isolation

Prevent cross-tenant key collision.

---

# 62. Cache Size

Bound cache memory.

Use LRU/TinyLFU-like policy only if needed.

---

# 63. No Hidden Redis Requirement

In-process caches first.

Add Redis only if shared cache is operationally justified.

---

# 64. Client Memory Architecture

Mobile/desktop clients need lower budgets.

Prefer:

```text
streaming reconciliation
small batches
lazy UI loading
bounded status history
```

---

# 65. UI State

Do not mirror entire local DB into Dioxus reactive state.

Keep:

```text
query-derived view model
pagination
incremental subscriptions
```

---

# 66. Dioxus Data Flow

Recommended:

```text
local DB
↓ repository/query
small view model
↓ signal
UI
```

---

# 67. Avoid Huge Signals

Large `Vec<Entity>` in reactive state causes copies/re-renders.

Use:

```text
IDs
pagination
virtualized lists
```

---

# 68. Change Notification

Sync engine can emit:

```text
entity/aggregate changed
scope updated
```

UI re-queries affected view.

---

# 69. Local DB Page Cache

Let embedded DB manage its own cache.

Do not duplicate massive object cache above it without evidence.

---

# 70. Memory-Mapped I/O

Only use if storage adapter supports safely and platform semantics are understood.

Not a core requirement.

---

# 71. Blob Memory

Large blobs must never be loaded fully into memory by sync core.

Stream:

```text
file
↓ hash/encrypt
↓ transport
```

---

# 72. Hash Streaming

BLAKE3 supports streaming.

Use for:

```text
large snapshots
blobs
exports
```

---

# 73. Compression Streaming

Zstd encoder/decoder should stream for large payloads.

---

# 74. Compression Threshold

Do not compress tiny messages.

Compression overhead can dominate.

---

# 75. Compression Level

Choose low/moderate level for interactive traffic.

Higher level only for reusable snapshots/export where CPU tradeoff is worthwhile.

---

# 76. Response Encoding

Avoid:

```text
typed response
→ Vec
→ clone
→ HTTP body
```

where possible.

Encode directly into reusable buffer/stream.

---

# 77. Buffer Pooling

Buffer pools can reduce allocator churn.

But pools add:

```text
contention
memory retention
complexity
```

Use only after profiling.

---

# 78. Per-Task Buffers

Often simpler:

```text
allocate with expected capacity
reuse within request
drop
```

Modern allocators handle this well.

---

# 79. Allocator Choice

Default system allocator is acceptable.

Benchmark alternatives such as:

```text
mimalloc
jemalloc
```

only if production workload shows allocator contention/fragmentation.

---

# 80. Cross-Platform Allocator

Do not force one allocator globally if it harms Android/Windows/macOS portability.

Use feature/deployment-specific choice.

---

# 81. Fragmentation

Long-lived servers should monitor:

```text
RSS
heap allocated
heap resident
```

Large divergence can indicate fragmentation/caching.

---

# 82. Arena Allocation

Arena/bump allocation may help temporary decode/planning phases.

Use only where lifetime is clearly batch-scoped.

---

# 83. Bump Arena Risk

Objects must not escape arena lifetime.

Keep behind internal API.

---

# 84. Dependency Planner Memory

For DAG of N operations:

```text
O(N + E)
```

memory and time target.

Use compact indices:

```text
usize/u32 node indices
```

instead of repeated UUID map lookups inside inner loop.

---

# 85. Operation Index Mapping

Build once:

```text
OperationId -> node index
```

then operate on dense arrays.

---

# 86. Topological Sort

Use:

```text
Kahn or DFS
```

with bounded graph.

---

# 87. Conflict Detection

Avoid scanning full entity history.

Use:

```text
EntityVersion
field write-set metadata
```

where applicable.

---

# 88. Field-Aware Merge

Represent write sets compactly.

Potential:

```text
bitset
small sorted IDs
```

for known field registries.

---

# 89. Scope Routing

Part 07 event routing should use indexed routing keys.

Avoid per-client predicate evaluation.

---

# 90. Scope Membership Storage

For high cardinality, use compact key IDs rather than strings.

---

# 91. Anti-Entropy Merkle

Part 03 tree should allow incremental updates.

Avoid recomputing entire tenant root after every change.

---

# 92. Incremental Digest

Update affected:

```text
entity leaf
partition node
root path
```

---

# 93. Snapshot Reuse

Part 10 reusable immutable chunks greatly reduce CPU/network.

---

# 94. Content-Addressed Reuse

If chunk digest unchanged:

```text
do not rebuild/upload
```

future optimization.

---

# 95. Journal Encoding

Journal payload should contain canonical event bytes once.

Avoid repeatedly reserializing for every client.

---

# 96. Event Payload Cache

Potential short-lived encoded-event cache.

Only if fan-out is high.

---

# 97. Per-Client Projection

If scope projection differs, may still require projection encode per client/scope class.

Cache by:

```text
event + projection schema + scope class
```

not individual client where possible.

---

# 98. Live Hint Efficiency

Hints are tiny.

Use latest-only coalescing per scope.

---

# 99. Audit Performance

Part 13 required audit should keep hot transaction record compact.

Search projection asynchronous.

---

# 100. Crypto Performance

Part 15:

```text
hash streaming
signature on manifest/checkpoint rather than every row
```

where threat model allows.

---

# 101. Key Service Latency

KMS signing/decryption can dominate.

Cache public keys and wrapped metadata safely.

Do not cache raw private keys unless policy permits.

---

# 102. Governance Performance

Part 14 purge in bounded batches.

Avoid long-running exclusive locks.

---

# 103. Authority Recovery

Part 16 fleet rebootstrap can create massive load.

Part 18 admission + Part 10 CDN reuse protect system.

---

# 104. Multi-Region

Part 17 shifts read load away from authority.

Do not duplicate authoritative write logic.

---

# 105. Object Layout

Hot structs should group frequently accessed fields.

Example operation scheduling hot fields:

```text
operation_id
state
priority
retry_at
local_seq
```

Payload can be separate/indirect.

---

# 106. Hot/Cold Split

Separate:

```text
hot metadata
cold payload
```

for outbox/journal tables where beneficial.

---

# 107. Database Hot/Cold Split

Example:

```text
outbox_meta
outbox_payload
```

only if profiling shows large payloads hurt common scans.

Do not complicate schema prematurely.

---

# 108. Inline Payload Threshold

Small payload inline.

Large payload may be external/blob ref.

---

# 109. Large Domain Payload

Normal operations should stay bounded.

Large binary data belongs in blob subsystem.

---

# 110. Cache Locality

Dense arrays beat pointer-heavy trees for hot batch algorithms.

Use indices in planner/scheduler.

---

# 111. HashMap Choice

Rust std HashMap uses security-oriented hashing.

For trusted internal numeric IDs, faster hashers may help.

Do not use weak hasher on attacker-controlled keys unless DoS risk addressed.

---

# 112. Hash DoS

External arbitrary strings/IDs should use collision-resistant hashing or bounded inputs.

---

# 113. BTreeMap

Useful when:

```text
stable ordering needed
range queries
deterministic tests
```

Not universally faster/slower.

---

# 114. Vec vs LinkedList

Prefer `Vec`/`VecDeque`.

`LinkedList` is rarely cache-friendly.

---

# 115. VecDeque

Useful for queues.

---

# 116. BinaryHeap

Useful for priority scheduling.

For multi-class fairness, separate queues may be simpler.

---

# 117. Bitsets

Use bitsets for:

```text
capabilities
field write sets
small registry flags
```

when IDs are bounded/dense.

---

# 118. Copy vs Clone

Small primitive structs can derive `Copy`.

Large structs should not.

---

# 119. Arc

Use `Arc` for genuinely shared immutable state.

Avoid `Arc` around every domain object.

Atomic refcount has cost.

---

# 120. ArcSwap

Potential for hot-read config/registry snapshots with rare updates.

Only if needed.

---

# 121. Configuration Snapshot

Load RON once.

Convert to typed runtime config.

Do not parse RON on hot path.

---

# 122. Logging Cost

Structured tracing can be expensive at high volume.

Avoid:

```text
debug log per entity
```

in production hot path.

---

# 123. Sampling

Sample high-volume success traces.

Always retain:

```text
errors
security events
rare state transitions
```

---

# 124. Metrics Cardinality

High-cardinality labels increase memory dramatically.

Never label metrics with:

```text
OperationId
EntityId
DeviceId
ScopeId
```

unless bounded and intentional.

---

# 125. Benchmark Taxonomy

Need:

```text
microbenchmarks
component benchmarks
database benchmarks
end-to-end benchmarks
soak tests
```

---

# 126. Microbenchmarks

Use Criterion or equivalent for:

```text
Postcard encode/decode
canonical hashing
dependency planning
compaction
scope routing
```

---

# 127. Component Benchmarks

Examples:

```text
10k operation batch planning
100k event reconciliation
1GB snapshot stream
```

---

# 128. Database Benchmarks

Use real PostgreSQL/Stoolap/SQLite adapters.

Measure:

```text
transaction throughput
journal pull
outbox scan
snapshot write
```

---

# 129. End-to-End Benchmark

Scenario:

```text
client local mutate
↓
sync
↓
server commit
↓
response
↓
client reconcile
```

Measure latency/allocations/bytes.

---

# 130. Soak Test

Run hours/days.

Look for:

```text
memory growth
fragmentation
queue leaks
connection leaks
performance drift
```

---

# 131. Benchmark Dataset

Use deterministic generated dataset.

Keep corpus versioned.

---

# 132. Performance Baseline

Store baseline metrics per release.

Example:

```text
ops/sec
p95
alloc bytes/op
snapshot MB/s
peak RSS
```

---

# 133. Regression Gate

CI can fail if critical benchmark regresses beyond threshold.

Use noise-tolerant thresholds.

---

# 134. Avoid Overly Strict CI Microbenchmarks

Shared CI runners are noisy.

Use dedicated performance runner/nightly for serious gates.

---

# 135. Flamegraphs

Use flamegraphs/perf on Linux for CPU hotspots.

Workflow:

```text
reproduce workload
capture profile
identify top stacks
optimize
re-measure
```

---

# 136. Heap Profiling

Use allocator/profiling tools to inspect:

```text
allocation count
retained memory
hot allocation sites
```

---

# 137. Tokio Console

Useful for:

```text
task stalls
long polls
resource contention
```

in development/staging.

---

# 138. Database Explain

Use:

```text
EXPLAIN (ANALYZE, BUFFERS)
```

for hot PostgreSQL queries.

---

# 139. Index Regression Tests

Schema migration CI can check important query plans on representative DB.

---

# 140. Synthetic Load

Use deterministic load generator:

```text
tenant count
devices/tenant
operation mix
payload distribution
offline duration
```

---

# 141. Workload Profiles

Examples:

```text
SchoolDayMorning
FeePaymentPeak
MassReconnect
LargeBootstrap
BulkImport
LowBandwidthMobile
```

---

# 142. SchoolDayMorning

Many attendance writes.

Tests:

```text
hot class/tenant
interactive latency
```

---

# 143. FeePaymentPeak

High-value strong aggregate operations.

Measure:

```text
DB locks
audit
idempotency
```

---

# 144. MassReconnect

Part 18 overload.

---

# 145. LargeBootstrap

Part 10 throughput/memory.

---

# 146. LowBandwidthMobile

Part 20 resource client profile.

---

# 147. Performance Trace Correlation

Use low-cardinality phase spans:

```text
decode
auth
validate
plan
db
encode
reconcile
```

---

# 148. Per-Operation Timing

Can record in tracing, not metrics labels.

---

# 149. Time Budget

Optional endpoint budget:

```text
decode 5%
validation 15%
DB 60%
encode 20%
```

Not hard-coded; useful diagnostic decomposition.

---

# 150. Latency vs Throughput

Do not optimize throughput by creating huge latency.

Interactive workloads prioritize:

```text
p95/p99 latency
```

Bulk workloads prioritize:

```text
throughput
```

---

# 151. Tail Latency

Tail latency matters more than mean.

Monitor:

```text
p95
p99
p99.9
```

where scale justifies.

---

# 152. Queueing Delay

Tail latency often comes from queueing.

Part 18 admission should reject before queues explode.

---

# 153. Batching Delay

Part 06 debounce should stay small for interactive work.

---

# 154. Nagle-Like Overbatching

Do not wait too long merely to fill batches.

---

# 155. Adaptive Batch

Use measured RTT/throughput.

Bound by:

```text
memory
latency
server hints
```

---

# 156. Serialization Cost

Postcard usually gives compact binary data.

Still benchmark domain payload.

---

# 157. RON

RON is for:

```text
configuration
debug
manifests where human-readable desired
```

not hot sync wire.

---

# 158. JSON

Use only interoperability surfaces.

Do not add JSON encode/decode inside core sync path.

---

# 159. Compression Cost Model

Rough decision inputs:

```text
payload size
network bandwidth
CPU availability
reusability
```

---

# 160. Mobile Compression

May prefer lower zstd level or no compression for small messages.

---

# 161. Server Snapshot Compression

Higher compression can be worthwhile because one snapshot serves many clients.

---

# 162. Dictionary Compression

Possible future optimization for repetitive schemas.

Not v1.

---

# 163. Delta Encoding

Possible for journal payloads.

Do not add unless profiling shows wire size bottleneck.

Semantic simplicity first.

---

# 164. Database Round Trips

Combine related reads in transaction where safe.

Avoid N+1 validation.

---

# 165. Batch Lookup

Fetch versions for multiple referenced entities in one query.

---

# 166. Prepared Registry Queries

Domain handlers can provide optimized repository methods.

Do not expose raw generic query builder in hot path.

---

# 167. Server Cache of Immutable Metadata

Examples:

```text
operation descriptors
schema registry
policy versions
```

good candidates.

---

# 168. Authorization Cache

Can help, but revocation correctness matters.

Use:

```text
short TTL/version
revocation generation
```

---

# 169. Client Reconciliation

Apply response in one bounded local transaction where feasible.

If huge:

```text
chunk + staging generation
```

---

# 170. UI Notification Coalescing

After applying 1000 events:

```text
emit one scope/view invalidation
```

rather than 1000 UI signals.

---

# 171. Outbox Compaction Performance

Part 04 compactor should be:

```text
O(N + E)
```

or near-linear.

---

# 172. Index Compaction Keys

Local DB can index:

```text
compaction_key
state
local_seq
```

---

# 173. Avoid Full Queue Rescan

Incremental compaction index where useful.

---

# 174. Anti-Entropy Scheduling

Do not hash entire database while interactive workload peaks.

Part 06/18 schedules maintenance.

---

# 175. Incremental Merkle Cache

Keep compact hashes.

Do not keep duplicate entity payloads in integrity cache.

---

# 176. Snapshot Staging

Install chunks directly into DB.

Avoid second in-memory representation.

---

# 177. File I/O

Use async file I/O only where platform/runtime implementation helps.

For large sequential file reads, blocking worker can be efficient.

Benchmark.

---

# 178. fsync Policy

Durability paths must obey ACID requirements.

Do not disable fsync for benchmark scores in production mode.

---

# 179. Benchmark Durability Modes

Clearly label:

```text
durable
relaxed
```

results.

Never compare unfairly.

---

# 180. Client DB Durability

Outbox/local mutation atomicity is mandatory.

Performance tuning cannot decouple them.

---

# 181. Write-Ahead Batching

Embedded DB may batch multiple local operations in one transaction if UI semantics permit.

---

# 182. Startup Performance

Avoid scanning full DB at startup.

Load:

```text
metadata
pending outbox indexes
scope manifests
```

through indexed queries.

---

# 183. Lazy Initialization

Initialize optional:

```text
anti-entropy
large snapshot cache
diagnostics
```

on demand/background.

---

# 184. Server Startup

Do not preload all tenant data.

Load registries/config globally and tenant metadata lazily/cache bounded.

---

# 185. Schema Migrations

Large migration should not block app startup indefinitely.

Enterprise deployment handles migration phase separately.

---

# 186. Binary Size

For mobile/desktop, feature-gate optional adapters/providers.

Do not link:

```text
all DBs
all KMS providers
all transports
```

into every client.

---

# 187. Feature Matrix

Example:

```text
client-stoolap
client-sqlite
server-postgres
live-websocket
crypto-standard
```

---

# 188. Monomorphization

Rust generics can increase binary size.

Use static dispatch in hot core where useful, but trait objects at plugin/tooling boundaries if size becomes issue.

---

# 189. LTO

Release profiles may use:

```text
thin LTO
```

or full LTO based on build-time/binary-size tradeoff.

Benchmark.

---

# 190. Codegen Units

Tune release build profile only after measuring.

---

# 191. Panic Strategy

`panic = "abort"` may reduce binary size, but changes crash behavior.

Choose by deployment, not blindly.

---

# 192. Debug Symbols

Production artifacts may keep split debug symbols for crash analysis.

---

# 193. Android ABI

Build only required ABIs per distribution where appropriate.

---

# 194. Platform-Specific Performance

Do not assume Linux server optimization applies to Android client.

Maintain separate benchmark profiles.

---

# 195. Resource Profiles

Define:

```rust
pub enum PerformanceProfile {
    MobileLowMemory,
    Desktop,
    ServerStandard,
    ServerHighThroughput,
}
```

---

# 196. MobileLowMemory

Defaults:

```text
small batches
1-2 downloads
small decode buffers
limited Rayon threads
aggressive release
```

---

# 197. Desktop

Larger:

```text
batches
parallel bootstrap
cache
```

---

# 198. ServerStandard

Balanced concurrency.

---

# 199. ServerHighThroughput

Higher limits only after load testing.

---

# 200. Auto Detection

Client may derive safe baseline from:

```text
available memory
CPU count
platform
```

but allow override.

---

# 201. No Memory Guessing From Total RAM Alone

OS pressure and other apps matter.

Use conservative caps.

---

# 202. OOM Policy

If memory pressure approaches hard limit:

```text
shed optional work
pause snapshot
reduce queues
```

rather than risk process kill.

---

# 203. Memory Pressure Signal

Platform-specific adapters may expose memory pressure.

Core treats it as scheduling hint.

---

# 204. Alloc Failure

Rust allocation may abort depending platform.

Prevention via bounds is better than recovery.

---

# 205. Performance Security

Performance limits are part of security.

Unbounded decode is DoS risk.

---

# 206. Constant Work Bounds

For attacker-controlled structures:

```text
operation count
dependency edges
scope filter complexity
```

must be bounded.

---

# 207. Hashing Large Inputs

Limit input before hashing to avoid CPU DoS.

---

# 208. Signature Verification

Batch/limit signatures.

Device signature verification should occur before expensive DB work.

---

# 209. CPU Amplification

Reject malformed inputs cheaply before cryptographic/domain expensive work where safely possible.

---

# 210. Data Copy Budget

For major pipelines document expected copies.

Example sync decode:

```text
socket → HTTP buffer
HTTP buffer → Postcard borrowed decode
owned domain fields only where needed
```

---

# 211. Copy Audit

Performance review can count:

```text
payload copies
allocation count
```

for hot request.

---

# 212. Memory Ownership Diagram

Maintain architecture docs identifying owner at each stage.

---

# 213. Example Sync Request Ownership

```text
HTTP body Bytes
    │
    ├── borrowed protocol view
    │
    └── validated owned operation payloads only as required
            │
            ▼
      Execution Plan
            │
            ▼
      DB/journal encoded bytes
```

---

# 214. Example Snapshot Ownership

```text
object stream
↓
small compressed buffer
↓
stream decoder
↓
record buffer
↓
DB staging transaction
↓
release
```

---

# 215. Performance Review Checklist

For each new subsystem:

```text
What is max input size?
What is max in-memory size?
What allocations happen per item?
Can it stream?
Can it block Tokio?
What is complexity?
What is its DB query count?
What is its queue bound?
```

---

# 216. Performance Budget CI

Critical crates can expose benchmarks.

Example targets:

```text
protocol decode allocations/op
planner ns/op
hash MB/s
snapshot peak memory
```

---

# 217. Avoid Fixed Universal Numbers

Hardware varies.

Use relative regression thresholds plus absolute safety limits.

---

# 218. Performance Documentation

Every benchmark result records:

```text
CPU
RAM
OS
DB version
Aequora commit
dataset
durability settings
```

---

# 219. Reproducibility

Benchmark harness uses fixed seed and workload manifest.

---

# 220. Workload Manifest

RON example:

```ron
(
    tenants: 100,
    clients_per_tenant: 50,
    operations_per_second: 10000,
    payload_bytes_p50: 256,
    payload_bytes_p99: 4096,
)
```

---

# 221. Profiler-Friendly Build

Provide dev profile with optimizations + symbols.

---

# 222. Benchmark CLI

Potential:

```text
aequora bench protocol
aequora bench planner
aequora bench sync
aequora bench snapshot
aequora bench adapter postgres
aequora bench adapter stoolap
```

---

# 223. Performance Report

Generate:

```text
throughput
latency
allocations
memory peak
CPU
I/O
```

---

# 224. Adapter Certification

Part 30 conformance can include performance baseline, but correctness certification must not require a particular speed.

Separate:

```text
semantic certification
performance characterization
```

---

# 225. Performance Classes

Optional:

```text
P1 Mobile
P2 Desktop
P3 Server
```

for documented tested capacity.

---

# 226. No Marketing Benchmarks in Core Spec

Keep measurements reproducible and technical.

---

# 227. Regression Investigation

If performance drops:

```text
confirm benchmark
profile
identify phase
compare allocations/query plan
fix
re-measure
```

---

# 228. Correctness First Under Optimization

Examples of forbidden "optimizations":

```text
advance cursor before durable apply
skip audit insert
send success before DB commit
drop tombstones early
reuse OperationId with modified payload
disable authorization cache invalidation
```

---

# 229. Unsafe Rust Policy

Default:

```text
forbid unsafe in core crates
```

unless a small audited module demonstrates substantial benefit not available safely.

---

# 230. Unsafe Exception

Requirements:

```text
document invariant
isolated module
tests/fuzzing
benchmark proving value
review
```

---

# 231. SIMD

BLAKE3/zstd already use optimized implementations.

Do not hand-write SIMD unless unavoidable.

---

# 232. Zero-Copy Serialization Limit

Postcard is compact, but database adapters may need conversion.

Accept necessary copy at semantic boundaries.

---

# 233. Shared Nothing Preference

Server request handlers should share mostly immutable global state.

Tenant/domain mutations coordinated through DB.

This scales better than large in-process shared mutable maps.

---

# 234. Horizontal Scaling

Stateless Axum nodes can scale out.

Performance architecture should avoid:

```text
node-local authoritative session state
```

---

# 235. Sticky Sessions

Not required for sync correctness.

Live sockets naturally stay on one node during connection, but reconnect can move.

---

# 236. Connection Pool Sizing

More DB connections is not always faster.

Benchmark DB capacity.

Use Part 18 outer admission.

---

# 237. Pool Timeout

Short bounded wait.

If pool saturated:

```text
admission/503
```

rather than thousands waiting.

---

# 238. Pipelining

HTTP/2 or HTTP/3 may reduce connection overhead.

Transport optimization does not change protocol semantics.

---

# 239. QUIC

Future optional transport.

Do not add before clear latency/connection need.

---

# 240. HTTP Compression

Avoid double-compressing already-zstd Postcard payload.

---

# 241. TLS Cost

TLS session reuse/HTTP keepalive reduces handshake cost.

Infrastructure handles much of it.

---

# 242. Connection Reuse

Client should reuse HTTP client/connection pool.

Do not create new HTTP client per sync.

---

# 243. DNS

Cache according to HTTP client/resolver policy but respect failover TTL.

---

# 244. Server Encoding Cache

For identical immutable snapshot manifests:

```text
cache encoded bytes
```

safe and useful.

---

# 245. Immutable Config Snapshot

Similarly cache encoded public capability responses.

---

# 246. Batch Ack Encoding

Use compact numeric status codes.

Avoid verbose strings on wire.

---

# 247. Error Details

Human-readable diagnostics optional and bounded.

---

# 248. Trace IDs

Keep operational IDs small/fixed-size.

---

# 249. Time Representation

Use fixed integer timestamp form on wire.

Avoid expensive text parsing in hot path.

---

# 250. Decimal Representation

Use canonical exact binary representation.

Avoid string decimal parsing repeatedly.

---

# 251. UUID Representation

Transmit 16 bytes, not text.

---

# 252. Collections

Bound all vectors/maps decoded from untrusted wire.

Serde alone may allocate declared collection.

Consider custom guarded decoder/framing where necessary.

---

# 253. Decode Guard

Protocol preamble should carry bounded counts and byte lengths.

---

# 254. Schema Compatibility

Older fields can be skipped/handled without materializing unknown huge payloads blindly.

---

# 255. Fuzz Performance

Fuzz not only crashes but pathological CPU/memory behavior for bounded inputs.

---

# 256. Complexity Assertions

Document expected complexity:

```text
dependency planning: O(N+E)
compaction: O(N+E)
scope match: O(keys)
reconciliation: O(events)
```

---

# 257. Performance Invariants

Add:

## AEQ-INV-PERF001

```text
All externally driven in-memory collections and queues have explicit bounds.
```

## AEQ-INV-PERF002

```text
Large snapshot/blob/export processing is streaming and does not require full payload materialization in RAM.
```

## AEQ-INV-PERF003

```text
CPU-heavy work does not execute unbounded on Tokio I/O worker threads.
```

## AEQ-INV-PERF004

```text
Performance optimizations never weaken cursor, transaction, idempotency, authorization, or audit invariants.
```

## AEQ-INV-PERF005

```text
Hot-path registries/configuration are immutable or version-swapped, not guarded by broad mutable locks.
```

## AEQ-INV-PERF006

```text
A performance regression can be attributed to a reproducible workload and measured phase before optimization is accepted.
```

---

# 258. Additional Invariants

## AEQ-INV-PERF007

```text
Local UI state does not require mirroring the entire synchronized database in reactive memory.
```

## AEQ-INV-PERF008

```text
Large binary content is represented by blob references in normal domain sync and is streamed through the blob subsystem.
```

## AEQ-INV-PERF009

```text
Request admission occurs before resource-expensive decode/execution wherever protocol framing allows.
```

---

# 259. Performance Test Matrix

Test at:

```text
1 client
100 clients
10k clients
large tenant
many small tenants
high RTT
low bandwidth
DB saturation
CPU saturation
```

---

# 260. Client Matrix

Platforms:

```text
low-memory Android
modern Android
desktop Linux
Windows
macOS
```

---

# 261. Server Matrix

At least:

```text
2-core small instance
8-core normal instance
larger production instance
```

---

# 262. Cross-DB Matrix

Benchmark:

```text
Stoolap client
SQLite client
Postgres server
future adapters
```

---

# 263. Baseline Performance Targets

Do not hardcode universal throughput promises.

Instead define project-specific SLOs.

Examples:

```text
interactive sync p95
memory peak
snapshot MB/s
max RSS during 1M event catch-up
```

---

# 264. Optimization Approval

Performance PR should include:

```text
before
after
workload
profile
correctness tests
tradeoffs
```

---

# 265. Avoid Benchmark-Only Code

Do not distort architecture for synthetic microbenchmark that does not represent production.

---

# 266. Performance Debt Registry

Track known bottlenecks with evidence.

Avoid vague:

```text
"optimize later"
```

---

# 267. Capacity Model

Estimate:

```text
operations/sec
bytes/sec
DB transactions/sec
snapshot egress
active devices
```

from real tenant workload.

---

# 268. Little's Law

Queue/concurrency planning can use:

```text
L = λW
```

to reason about in-flight requests.

Use as operational model, not hardcoded logic.

---

# 269. Backpressure Coupling

Part 18 admission limits are informed by measured performance capacity from Part 19.

---

# 270. Scheduler Coupling

Part 06 client batch controller uses observed:

```text
RTT
success
throughput
server hints
```

---

# 271. Resource-Constrained Coupling

Part 20 will specialize these rules for:

```text
battery
memory
storage
mobile background limits
```

---

# 272. Final Recommended Architecture

```text
                     NETWORK INPUT
                          │
                          ▼
                 Bounded Frame Reader
                          │
                          ▼
                  Borrowed Decode
                          │
                          ▼
                 Typed Validation
                          │
                   owned only as needed
                          │
                          ▼
                 Dense Batch Planner
                          │
                          ▼
            ┌─────────────┴─────────────┐
            ▼                           ▼
       Tokio I/O                    Rayon CPU
 DB/network/orchestration      hash/compress/transform
            │                           │
            └─────────────┬─────────────┘
                          ▼
                 Bounded Execution
                          │
                          ▼
                  Durable Commit
                          │
                          ▼
                Streaming Encode
                          │
                          ▼
                       Client
                          │
                          ▼
              Bounded Reconciliation
                          │
                          ▼
                    Local Database
                          │
                          ▼
               Small Reactive View
```

---

# 273. Completion Criteria

Part 19 is complete when:

```text
[ ] memory budgets defined
[ ] Bytes/borrowed decode boundaries defined
[ ] large-path streaming rules defined
[ ] Tokio/Rayon boundaries defined
[ ] bounded CPU concurrency defined
[ ] DB query/index performance rules defined
[ ] client UI memory rules defined
[ ] blob/snapshot zero-copy boundaries defined
[ ] allocator policy defined
[ ] hot/cold data split guidance defined
[ ] benchmark taxonomy defined
[ ] workload manifests defined
[ ] profiling workflow defined
[ ] regression gates defined
[ ] unsafe/SIMD policy defined
[ ] performance invariants added
```

---

# 274. Final Principle

> **Aequora should be fast because it moves less data, allocates deliberately, streams large workloads, uses databases efficiently, and keeps concurrency bounded—not because it bypasses correctness or replaces safe Rust with fragile tricks.**

The intended performance model is therefore:

```text
typed
streaming
bounded
measured
cache-conscious
database-aware
platform-aware
correctness-preserving
```

This gives Aequora a sustainable path from small local-first applications to high-throughput enterprise deployments without requiring a later architectural rewrite solely to control memory or latency.
