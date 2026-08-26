# Aequora Sync — Part 20

# Resource-Constrained Client Architecture

## 1. Purpose

Aequora is designed for local-first applications, which means the client runtime is not merely a thin UI.

A client may contain:

```text
embedded database
outbox
journal reconciliation state
scope metadata
snapshot staging
local search/indexes
crypto keys
background scheduler
live connection
```

On powerful desktops this is usually manageable.

But Aequora must also run correctly on:

```text
low-memory Android phones
older laptops
shared school computers
low-storage devices
metered mobile networks
slow Wi-Fi
intermittent power
background-restricted mobile OSes
devices offline for days or weeks
```

A resource-constrained client cannot simply use the same defaults as a server or modern desktop.

The central rule is:

> **Aequora must adapt resource usage without changing synchronization semantics.**

The client may:

```text
sync less often
use smaller batches
bootstrap fewer scopes
delay maintenance
compress selectively
discard rebuildable caches
```

but it may not:

```text
lose durable outbox data
advance cursors early
drop required tombstones
silently weaken authorization
```

---

# 2. Goals

The constrained-client architecture should provide:

```text
bounded RAM
bounded storage
battery-aware scheduling
metered-network awareness
background-safe checkpointing
interruption-safe bootstrap
adaptive batching
local degradation modes
selective scope caching
cache eviction
offline durability
recovery after process death
```

---

# 3. Non-Goals

This architecture is not:

```text
a separate protocol
a lightweight fork of Aequora
a weaker correctness mode
a cloud-only fallback
```

The same core semantics remain.

---

# 4. Resource Dimensions

A client runtime should model at least:

```text
memory
storage
CPU
battery/power
network
background execution budget
thermal state
```

---

# 5. ClientResourceContext

Conceptually:

```rust
pub struct ClientResourceContext {
    pub memory: MemoryClass,
    pub storage: StorageState,
    pub network: NetworkContext,
    pub power: PowerContext,
    pub background: BackgroundBudget,
    pub thermal: ThermalState,
}
```

---

# 6. MemoryClass

```rust
pub enum MemoryClass {
    VeryLow,
    Low,
    Normal,
    High,
}
```

Do not hardcode only by RAM total.

Use conservative platform-derived profile.

---

# 7. StorageState

```rust
pub enum StorageState {
    Healthy,
    Low,
    Critical,
    ReadOnlyRisk,
}
```

---

# 8. BackgroundBudget

```rust
pub enum BackgroundBudget {
    Foreground,
    Short,
    Limited,
    Suspended,
}
```

---

# 9. ThermalState

```rust
pub enum ThermalState {
    Normal,
    Warm,
    Hot,
    Critical,
}
```

Only use if platform exposes reliable signal.

---

# 10. Resource Profiles

Provide built-in profiles:

```text
MobileMinimal
MobileStandard
DesktopConservative
DesktopStandard
```

---

# 11. MobileMinimal

Recommended for:

```text
older Android
< 4 GB RAM
small storage
```

Defaults:

```text
small sync batch
one snapshot chunk at a time
no large background anti-entropy
small caches
low CPU parallelism
```

---

# 12. MobileStandard

For modern phones:

```text
moderate batching
2 download workers
background sync where OS permits
bounded local indexes
```

---

# 13. DesktopConservative

Useful for school/shared PCs.

---

# 14. Dynamic Adaptation

Profile is baseline.

Runtime adjusts when:

```text
battery low
network metered
memory pressure
storage low
thermal high
```

---

# 15. Correctness vs Optimization

Resource policy may influence:

```text
when
how much
which optional work
```

It may not influence:

```text
authoritative validity
idempotency
cursor commit semantics
```

---

# 16. Durable Outbox Priority

The most important client data is:

```text
unsynced user intent
```

Protect it before caches.

---

# 17. Storage Eviction Order

Recommended:

```text
1. transient logs
2. rebuildable UI cache
3. downloaded snapshot chunks already installed
4. derived search/index cache
5. optional old scope cache
6. resolved conflict payload details
```

Never automatically evict:

```text
unsynced outbox
active scope base state required by pending ops
crypto key metadata
```

---

# 18. Low-Storage Mode

When storage is low:

```text
pause optional bootstrap
delete rebuildable caches
compact safe outbox operations
reduce local retained scopes
warn application
```

---

# 19. Critical Storage

If storage approaches transaction failure risk:

```text
enter guarded mode
```

Potential:

```text
block new large local mutations
```

if they cannot be durably committed.

---

# 20. Never Claim Save If Local Commit Failed

Local-first UX rule:

```text
user action is "saved locally"
```

only after local DB transaction commits.

---

# 21. Storage Preflight

Before:

```text
large snapshot
blob download
bulk local import
```

estimate storage requirement.

---

# 22. Streaming Bootstrap

Part 10 low-storage mode:

```text
download
verify
install
delete chunk
repeat
```

---

# 23. Snapshot Cache Policy

```rust
pub enum SnapshotCachePolicy {
    None,
    InstalledOnly,
    KeepRecent,
}
```

---

# 24. Memory Architecture

Use small bounded buffers.

Examples:

```text
network read buffer
decode buffer
reconcile batch
UI query page
```

---

# 25. No Whole-Scope Materialization

Never load entire synchronized scope into RAM.

---

# 26. Local Query Pagination

Repositories should support:

```text
limit
cursor
```

for UI.

---

# 27. Virtualized UI

Large tables/lists use:

```text
virtualized rendering
paged queries
```

---

# 28. Reactive State

Store only:

```text
visible rows
IDs
small view models
sync status
```

in Dioxus signals.

---

# 29. Process Death

Mobile OS may kill app at any time.

Therefore every meaningful background transition must checkpoint.

---

# 30. Crash-Safe Background Loop

Pattern:

```text
claim durable work
↓
do bounded unit
↓
commit checkpoint
↓
yield
```

---

# 31. No Long In-Memory Workflow

Do not require one uninterrupted process lifetime for:

```text
bootstrap
repair
large catch-up
```

---

# 32. Android Background Limits

Mobile OS may restrict:

```text
continuous sockets
long-running jobs
CPU
network
```

Aequora runtime must treat background time as opportunistic.

---

# 33. Foreground Mode

When app active:

```text
live hints
interactive sync
larger budget
```

---

# 34. Background Mode

When app backgrounded:

```text
close live connection if required
persist scheduler
use OS-approved background tasks
perform bounded sync
```

---

# 35. Suspended Mode

If OS grants no work:

```text
do nothing
```

Correctness is preserved by durable state.

---

# 36. Resume

On foreground:

```text
check pending outbox
check scope cursors
perform immediate catch-up
```

---

# 37. Push Notification

Part 08 mobile push is a wake hint.

It does not carry authoritative state.

---

# 38. Background Sync Trigger

Push may cause:

```text
short sync attempt
```

if OS grants time.

---

# 39. No Reliance on Push Delivery

Push may be:

```text
late
dropped
disabled
```

Periodic/foreground catch-up remains.

---

# 40. Battery Awareness

When battery low:

```text
defer maintenance
reduce anti-entropy
avoid large bootstrap unless user requested
```

---

# 41. Charging Mode

When charging + Wi-Fi:

```text
good time for
integrity scans
large snapshot
optional scope prefetch
```

---

# 42. Metered Network

On metered network:

```text
small interactive sync allowed
large bootstrap/blobs deferred or user-approved
```

---

# 43. Roaming

Treat as stricter than metered if platform exposes.

---

# 44. Network Policy

```rust
pub struct ClientNetworkPolicy {
    pub allow_interactive_metered: bool,
    pub allow_bulk_metered: bool,
    pub allow_roaming_bulk: bool,
}
```

---

# 45. Poor Connectivity

Use:

```text
small batches
longer timeouts
resume
compression
```

---

# 46. High Loss

Reduce:

```text
large request bodies
parallel transfers
```

---

# 47. RTT Adaptation

Part 06 can increase batch size on high RTT if bandwidth/memory permit.

But constrained client caps it.

---

# 48. Adaptive Batch Ceiling

```text
scheduler desired batch
↓
resource profile maximum
↓
server maximum
```

effective batch is minimum.

---

# 49. CPU Usage

Avoid using all cores.

On mobile:

```text
1 CPU worker
or
small bounded pool
```

for hashing/compression.

---

# 50. Thermal Throttling

If thermal state high:

```text
pause compression-heavy background work
reduce hashing
```

---

# 51. Anti-Entropy

Part 03 should support:

```text
incremental partition verification
```

instead of full DB hash.

---

# 52. Integrity Scheduling

On constrained client:

```text
small partitions
charging preference
idle time
```

---

# 53. Repair

Repair of corruption may override battery optimization.

Correctness repair can become high priority.

---

# 54. Repair Size

Large repair uses snapshot streaming.

---

# 55. Local Multi-Process

Part 05 on mobile usually one process/store.

Desktop may have multiple windows.

Resource policy still belongs to store leader.

---

# 56. Live Connection

On mobile foreground:

```text
one connection
```

per active store/account.

Avoid one connection per scope.

---

# 57. Presence

Disable or reduce presence updates on battery-constrained state.

Presence is optional.

---

# 58. Scope Strategy

Part 07 is critical for constrained clients.

Do not replicate entire tenant if user only needs:

```text
their class
current year
assigned campus
```

---

# 59. Minimal Scope

Prefer smallest useful authorized scope.

This reduces:

```text
storage
network
memory
bootstrap
```

---

# 60. Optional Scope

User can download:

```text
offline finance archive
old academic year
```

only on request.

---

# 61. Scope Cache Policy

```rust
pub enum ScopeCachePolicy {
    Required,
    Recent,
    OnDemand,
    NeverPersist,
}
```

---

# 62. Required

Core data required offline.

---

# 63. Recent

Keep recently used scope until storage pressure.

---

# 64. OnDemand

Bootstrap when opened.

---

# 65. NeverPersist

Fetch transiently if architecture allows.

---

# 66. Scope Eviction

Eviction must respect:

```text
pending operations
scope dependencies
authorization
```

---

# 67. Pending Operation Pins Scope

If outbox references entity:

```text
required base/reference state may pin scope
```

until operation resolves.

---

# 68. Local Retention

Part 14 can keep:

```text
current school year
```

locally while server retains more.

---

# 69. Tombstone Retention

Client can drop old tombstones only when server protocol/generation makes it safe.

Do not independently guess.

---

# 70. Cursor Metadata

Cursor state is tiny and must be retained durably.

---

# 71. Outbox Compaction

Part 04 is especially valuable on constrained clients.

Safe examples:

```text
draft preference updates
field set operations
```

---

# 72. Compaction Trigger

Trigger on:

```text
queue growth
storage pressure
pre-sync
background maintenance
```

---

# 73. Finance Compaction

Still:

```text
Never
```

for immutable/high-value operations.

Resource pressure cannot override semantics.

---

# 74. Outbox Payload Compression

Could compress large pending payloads if storage is bottleneck.

But avoid CPU cost for small payloads.

---

# 75. Pending Blob

Large attachment should be stored in blob spool, not operation envelope.

---

# 76. Blob Upload Resume

Use chunked/resumable transfer.

---

# 77. Blob Storage Quota

Local blob spool needs explicit limit.

---

# 78. User-Owned Pending Blob

Do not delete unsent user attachment silently when storage pressure.

Ask UI/application to resolve.

---

# 79. Database Choice

Embedded adapter should provide:

```text
transactional durability
bounded resource use
fast indexed local queries
```

Aequora core remains adapter-independent.

---

# 80. Database Cache Tuning

Expose adapter-specific cache limits through adapter configuration.

Do not hardwire SQL page cache logic in core.

---

# 81. SQLite Example

Possible tuning:

```text
WAL
bounded cache_size
busy_timeout
```

adapter-specific.

---

# 82. Stoolap Example

Use its native transaction/cache configuration through adapter.

---

# 83. Client DB Maintenance

Vacuum/compaction can be expensive.

Run:

```text
charging/idle
```

where possible.

---

# 84. Low Storage DB Maintenance

May become urgent if reclaim can prevent failure.

---

# 85. Derived Search Index

Can be:

```text
smaller
partial
lazy
```

on mobile.

---

# 86. Rebuildable Index

If storage pressure:

```text
drop
```

and rebuild later.

---

# 87. Thumbnail/Media Cache

Not part of sync authority.

Aggressively evictable.

---

# 88. Crypto

Part 15 may add CPU/storage overhead.

Use hardware-backed keys where available.

---

# 89. Encryption Streaming

Large snapshot/blob encryption should stream.

---

# 90. Key Access

Avoid repeated expensive secure-store access per row.

Load/use scoped key handle where platform safely permits.

---

# 91. Secure Store Failure

If device key unavailable:

```text
do not silently fall back to plaintext
```

---

# 92. Local Encryption Key Loss

May require:

```text
local store reset
rebootstrap
```

Pending unsynced work must be considered before reset.

---

# 93. Diagnostics

Constrained clients should keep small ring buffer of diagnostics.

---

# 94. Diagnostic Ring

Example:

```text
last 100 sync transitions
last errors
resource state
```

---

# 95. No Unbounded Logs

Client log files need rotation/size cap.

---

# 96. Incident Bundle

Part 25 can collect:

```text
sanitized small diagnostics
```

without dumping entire DB.

---

# 97. Network Bytes Accounting

Track:

```text
sync bytes
snapshot bytes
blob bytes
```

for scheduler/data budget.

---

# 98. Data Budget

Optional:

```rust
pub struct DataBudget {
    pub daily_metered_bytes: Option<u64>,
    pub monthly_metered_bytes: Option<u64>,
}
```

---

# 99. Budget Exhaustion

Do not block critical security revocation.

Policy can still allow:

```text
critical tiny sync
```

while deferring bulk.

---

# 100. User Override

UI may allow:

```text
Download now using mobile data
```

for explicit user choice.

---

# 101. Scheduler Work Kinds

Resource-aware scheduler prioritizes:

```text
CriticalSync
InteractivePush
InteractivePull
SmallRepair
Bootstrap
BlobTransfer
AntiEntropy
Maintenance
```

---

# 102. Work Estimate

Each work item can estimate:

```text
bytes
CPU class
storage delta
```

roughly.

---

# 103. ResourceAdmission

Client-side:

```rust
pub trait ClientResourceAdmission {
    fn allow(
        &self,
        work: &ClientWork,
        resources: &ClientResourceContext,
    ) -> ResourceDecision;
}
```

---

# 104. ResourceDecision

```rust
pub enum ResourceDecision {
    RunNow,
    RunReduced,
    Defer,
    RequireUserApproval,
    RejectUntilResourceAvailable,
}
```

---

# 105. RunReduced

Example:

```text
sync only 20 operations
```

instead of 500.

---

# 106. User-Visible Status

Useful states:

```text
Up to date
Syncing
Waiting for network
Waiting for Wi-Fi
Storage low
Sync delayed to save battery
Needs rebootstrap
```

---

# 107. Avoid Misleading "Offline"

If server busy or battery-delayed:

```text
not necessarily offline
```

status should be accurate.

---

# 108. Background Progress

Never depend on animated UI for workflow.

State is persisted.

---

# 109. App Upgrade

Upgrade may occur while large bootstrap unfinished.

Persist format version.

On new app version:

```text
resume if compatible
or restart safely
```

---

# 110. Local Schema Migration

Must be:

```text
bounded
restart-aware
```

for large stores.

---

# 111. Migration Preflight

Check:

```text
storage
battery if optional
schema compatibility
```

---

# 112. Mandatory Migration

If new app cannot operate old schema:

```text
perform before sync
```

with recovery backup/rollback where feasible.

---

# 113. App Downgrade

Do not allow older app to open newer incompatible local schema silently.

---

# 114. StoreFormatVersion

Define:

```rust
pub struct LocalStoreFormatVersion(u32);
```

---

# 115. Low-Memory Decode

Cap:

```text
operation batch
snapshot record
audit page
```

---

# 116. Oversized Domain Entity

Application should avoid gigantic single record.

Large content → blob.

---

# 117. Pagination for Audit

Part 13 audit history fetched in small pages.

---

# 118. Pagination for Conflict UI

Do not load every historical conflict.

---

# 119. Local Conflict Storage

Resolved conflict detailed payload can age out per Part 14.

---

# 120. Process Priority

Background maintenance may use lower OS priority if available.

Adapter-specific.

---

# 121. Wake Locks

Android background jobs should avoid long wake locks.

Use bounded tasks.

---

# 122. Foreground Service

Only use when product genuinely requires long visible work such as:

```text
user-requested large offline download
```

and platform policy permits.

---

# 123. App Lifecycle Hooks

Client runtime accepts:

```text
Foreground
Background
Suspending
Resumed
```

events from platform shell.

---

# 124. Suspend Hook

Before suspension:

```text
flush in-memory scheduler metadata
commit checkpoints
close transient sockets
```

Do not delay OS indefinitely.

---

# 125. Network Change Hook

On:

```text
offline → online
Wi-Fi → metered
```

recompute schedule.

---

# 126. Battery Change Hook

Recompute optional work eligibility.

---

# 127. Storage Pressure Hook

Immediate:

```text
evict caches
pause bulk
```

---

# 128. Resource Event Coalescing

Platform can fire many events.

Coalesce into one scheduler reevaluation.

---

# 129. No Polling Every Second

Prefer platform events + bounded timers.

---

# 130. Timers

Use coarse timer for maintenance.

Save wakeups/battery.

---

# 131. Live Poll Safety Interval

If live hints active, still periodic catch-up but not too frequent.

---

# 132. Very Low Power Mode

Potential:

```text
sync only user-triggered/critical
```

until power improves.

---

# 133. Security Revocation Exception

If a revocation/purge directive arrives:

```text
execute promptly
```

even under low battery if possible.

---

# 134. Local Purge and Low Power

Security beats convenience.

---

# 135. Offline-First UX

User may continue modifying authorized cached data offline.

Outbox durable.

---

# 136. Read-Only Scope

Some domains may disable offline mutation.

Part 11 profile can say:

```text
ServerOnly
```

---

# 137. Device Clock

Resource scheduling may use wall clock.

Domain authoritative semantics do not trust it.

---

# 138. HLC

HLC remains metadata/causality aid.

Low-power suspension does not harm correctness.

---

# 139. Clock Jump

Scheduler should tolerate:

```text
time changed
timezone changed
```

Use monotonic time for retry timers within process where possible.

Persist wall-time deadlines with robust re-evaluation.

---

# 140. Data Compression

Network constrained profile may favor compression.

CPU/thermal constrained profile may disable for small payload.

---

# 141. Compression Decision

Based on:

```text
payload bytes
network type
CPU/thermal
```

---

# 142. Dictionary Cache

Not v1.

---

# 143. TLS Connection Reuse

Reuse client.

Avoid repeated handshakes.

---

# 144. Connection Idle Policy

Mobile OS may kill idle socket.

Handle reconnect normally.

---

# 145. QUIC

Future optional transport may improve poor-network behavior but is not required for resource correctness.

---

# 146. HTTP/2

Good initial multiplexed transport where available.

---

# 147. Retry Policy

Constrained clients should use:

```text
exponential backoff
jitter
network-aware pause
```

---

# 148. Retry Persistence

Persist enough retry state so process restart does not reset storm behavior.

---

# 149. Retry Counter

Can cap/rescale after long offline.

---

# 150. Immediate User Action

User pressing:

```text
Sync now
```

may temporarily override background delay but not hard resource/security limits.

---

# 151. Storage Emergency

If outbox cannot accept new mutation:

```text
UI must show failure
```

No fake local-first guarantee.

---

# 152. Corruption

If local DB corruption detected:

```text
preserve recoverable pending operations
rebootstrap
```

---

# 153. Pending Operation Export

Part 10/12 can export pending intent before destructive repair.

---

# 154. Recovery Bundle Size

Keep minimal.

---

# 155. Device Retirement

Part 14 inactive old device can be forced to rebootstrap.

This helps server retention.

---

# 156. Old App Version

Very old client may be too inefficient/incompatible.

Part 21 protocol policy can require upgrade.

---

# 157. Capability Negotiation

Client advertises:

```text
max batch bytes
snapshot chunk max
compression support
resource profile
```

Server does not trust resource profile for authorization, but can tailor transport.

---

# 158. Privacy

Do not send exact RAM/storage telemetry unless needed.

Prefer coarse capability classes.

---

# 159. ClientCapabilityProfile

Example:

```rust
pub struct ClientCapabilityProfile {
    pub max_request_bytes: u32,
    pub max_snapshot_chunk_bytes: u32,
    pub supports_zstd: bool,
    pub low_memory: bool,
}
```

---

# 160. Server Adaptation

Server may choose:

```text
smaller snapshot profile
smaller response page
```

---

# 161. No Per-Device Snapshot Explosion

Part 10 warns against too many variants.

Use small set:

```text
Standard
LowMemory
```

---

# 162. Testing Profiles

Must test at least:

```text
VeryLowMemory
LowStorage
MeteredNetwork
HighRTT
ProcessKill
BackgroundSuspend
```

---

# 163. Process-Kill Test

Kill app:

```text
mid sync
mid bootstrap
mid repair
mid local mutation
```

Expected:

```text
no corruption
resume safely
```

---

# 164. Low-Memory Test

Artificially cap process memory.

Expected:

```text
bounded batch
no full snapshot allocation
```

---

# 165. Disk-Full Test

Inject disk-full:

```text
before local transaction
during snapshot staging
during outbox append
```

Expected:

```text
safe failure
no cursor advance
```

---

# 166. Metered Test

Large optional bootstrap should defer.

Interactive small operation should still sync if policy allows.

---

# 167. Background Timeout Test

OS terminates background work after short interval.

Expected:

```text
checkpoint
resume later
```

---

# 168. Thermal Test

High thermal signal.

Expected:

```text
CPU-heavy maintenance reduced
```

---

# 169. Offline Week Test

Client offline 7 days.

Generate many local operations.

Expected:

```text
durable outbox
safe compaction where allowed
normal rebase on reconnect
```

---

# 170. Old Cursor Test

Client below journal floor.

Expected:

```text
rebootstrap
pending intent preserved
```

---

# 171. Scope Eviction Test

Storage pressure evicts optional scope.

Expected:

```text
required/pending-pinned data retained
```

---

# 172. Battery Test

Low battery:

```text
maintenance deferred
```

critical small sync remains possible by policy.

---

# 173. Correctness Invariants

Add:

## AEQ-INV-CLIENT001

```text
Resource pressure never causes the client to discard unsynchronized durable user intent without an explicit domain/user decision.
```

## AEQ-INV-CLIENT002

```text
A client cursor advances only after durable local application regardless of background timeout, battery state, or memory pressure.
```

## AEQ-INV-CLIENT003

```text
Optional work may be deferred under resource pressure, but required security/governance directives are not silently ignored.
```

## AEQ-INV-CLIENT004

```text
Large bootstrap/blob operations remain bounded in memory under every supported client resource profile.
```

## AEQ-INV-CLIENT005

```text
A process kill at any scheduler/bootstrap/reconciliation checkpoint leaves a recoverable durable state.
```

## AEQ-INV-CLIENT006

```text
Storage-pressure eviction never removes authoritative base state required to safely resolve a pending local operation without first handling that operation.
```

---

# 174. Additional Invariants

## AEQ-INV-CLIENT007

```text
Local-first success is reported only after the local mutation plus outbox entry commits atomically.
```

## AEQ-INV-CLIENT008

```text
A resource profile changes scheduling and resource limits, not domain consistency semantics.
```

## AEQ-INV-CLIENT009

```text
Client resource telemetry exposed to the server is coarse and never used as authorization evidence.
```

---

# 175. Metrics

Client-local metrics:

```text
client_memory_profile
client_storage_state
sync_deferred_resource_total
bootstrap_paused_resource_total
outbox_bytes
local_db_bytes
cache_evicted_bytes
```

Telemetry upload should be privacy-controlled.

---

# 176. Logs

Structured:

```text
resource_profile_changed
storage_low
sync_deferred_metered
background_budget_expired
snapshot_paused
cache_evicted
```

---

# 177. User Diagnostics

Expose:

```text
local data size
pending changes
last successful sync
network restriction
storage warning
```

---

# 178. Admin Diagnostics

If organization manages devices:

```text
client version
last sync
requires rebootstrap
storage-pressure status
```

with privacy limits.

---

# 179. Configuration

Example RON:

```ron
client_resources: (
    profile: MobileStandard,

    memory: (
        sync_decode_bytes: 4194304,
        snapshot_pipeline_bytes: 8388608,
    ),

    storage: (
        low_watermark_bytes: 536870912,
        critical_watermark_bytes: 134217728,
    ),

    network: (
        allow_interactive_metered: true,
        allow_bulk_metered: false,
        allow_bulk_roaming: false,
    ),

    background: (
        maintenance_requires_charging: true,
        anti_entropy_requires_unmetered: true,
    ),
)
```

---

# 180. Safe Defaults

On mobile:

```text
small memory caps
bulk on unmetered
maintenance on charging
```

but allow application-specific override.

---

# 181. Platform Adapter

Resource signals belong behind platform abstraction.

```rust
pub trait PlatformResourceMonitor {
    fn current(&self) -> ClientResourceContext;
}
```

---

# 182. Android Adapter

May integrate:

```text
connectivity manager
battery state
work scheduler
storage stats
lifecycle hooks
```

through Kotlin/NDK bridge if necessary.

Core policy stays Rust.

---

# 183. iOS Adapter

Equivalent native bridge where needed.

---

# 184. Desktop Adapter

Uses:

```text
OS memory/storage/network signals
application lifecycle
```

---

# 185. Web

If future web client uses browser storage:

```text
different quota/background model
```

but same high-level resource policy concept can apply.

---

# 186. Module Layout

```text
aequora-client/
└── resources/
    ├── context.rs
    ├── profile.rs
    ├── memory.rs
    ├── storage.rs
    ├── network.rs
    ├── power.rs
    ├── thermal.rs
    ├── admission.rs
    └── events.rs
```

Platform crates:

```text
aequora-platform-android
aequora-platform-ios
aequora-platform-desktop
```

---

# 187. Scheduler Integration

Part 06 remains policy owner for:

```text
when to run work
```

Part 20 supplies:

```text
resource constraints
```

---

# 188. Admission Integration

Part 18 server admission protects server.

Part 20 client admission protects device.

---

# 189. Performance Integration

Part 19 defines optimized pipelines.

Part 20 chooses conservative caps per platform.

---

# 190. Governance Integration

Part 14 local purge can override optional resource deferral.

---

# 191. Crypto Integration

Part 15 secure storage/key management must fit device platform.

---

# 192. Authority Integration

Part 16 epoch transition may require large rebootstrap.

Resource-aware scheduling can stagger it but cannot skip it.

---

# 193. Multi-Region Integration

Part 17 regional snapshot delivery reduces bandwidth/latency.

---

# 194. Completion Criteria

Part 20 is complete when:

```text
[ ] client resource context defined
[ ] constrained profiles defined
[ ] low-memory pipeline limits defined
[ ] low-storage eviction policy defined
[ ] outbox protection defined
[ ] mobile background checkpointing defined
[ ] network/battery/thermal policies defined
[ ] scope cache policy defined
[ ] local DB/cache maintenance rules defined
[ ] resource-aware bootstrap/blob transfer defined
[ ] platform monitor abstraction defined
[ ] coarse capability negotiation defined
[ ] process-kill/disk-full/low-memory tests defined
[ ] client resource correctness invariants added
```

---

# 195. Final Architecture

```text
                   PLATFORM RESOURCE SIGNALS
                memory / storage / power / network
                             │
                             ▼
                   ClientResourceContext
                             │
                             ▼
                   Resource Admission Policy
                             │
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
          Run Now        Run Reduced       Defer
              │              │              │
              └──────────────┼──────────────┘
                             ▼
                     Part 06 Scheduler
                             │
                             ▼
                 Durable Work Source
              outbox / bootstrap / repair
                             │
                             ▼
                   Bounded Execution
                             │
                checkpoint after each unit
                             │
                             ▼
                       Local Database
                             │
                             ▼
                    Small UI View Model
```

The architectural principle is:

> **Aequora should degrade resource consumption, not correctness.**

On a low-memory phone or poor network, the engine should do less work at once, synchronize later, cache less, stream more aggressively, and rely more heavily on durable checkpoints—but preserve the exact same authoritative, idempotent, crash-safe synchronization model used on powerful desktop and server deployments.
