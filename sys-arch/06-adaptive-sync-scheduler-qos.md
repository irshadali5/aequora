# Aequora Sync — Part 06

# Adaptive Sync Scheduler and Quality-of-Service Architecture

## 1. Purpose

Aequora is a local-first synchronization platform.

Its client runtime must operate across very different environments:

```text
desktop on fast broadband
Android on cellular
metered networks
intermittent Wi-Fi
slow rural networks
battery-constrained devices
background mobile execution
high-latency enterprise VPN
LAN-only deployments
large offline reconnect bursts
```

A fixed synchronization strategy is inefficient.

Examples:

```text
sending every mutation immediately
```

may waste radio wakeups and battery.

But:

```text
waiting 30 seconds for every operation
```

makes interactive usage feel broken.

Likewise:

```text
uploading a 50 MB snapshot repair
```

while a user is trying to submit a payment is poor scheduling.

Aequora therefore needs an explicit scheduling and Quality-of-Service subsystem.

The central rule is:

> **Correctness decides what must happen. Scheduling decides when and in what order safe independent work should happen.**

QoS must never change business semantics, idempotency, dependency requirements, or authoritative ordering.

---

## 2. Goals

The scheduler should provide:

```text
interactive responsiveness
battery efficiency
bandwidth efficiency
metered-network awareness
background execution safety
priority-aware batching
retry fairness
server load cooperation
bounded memory
bounded concurrency
anti-thundering-herd behavior
predictable starvation prevention
```

---

## 3. Non-Goals

The scheduler must not:

```text
change operation meaning
violate dependencies
drop durable intent
silently discard low-priority operations
override authorization/conflict policy
make server authority weaker
```

---

## 4. Scheduling Layers

Aequora scheduling has four layers:

```text
L1 — Work classification
L2 — Eligibility
L3 — Prioritization and fairness
L4 — Execution/batching
```

---

## 5. Work Classes

Define:

```rust
pub enum WorkClass {
    Critical,
    Interactive,
    Normal,
    Bulk,
    Background,
    Maintenance,
}
```

### Critical

Examples:

```text
security revocation acknowledgement
payment-related finalization where product requires urgency
critical consistency repair
```

Use sparingly.

### Interactive

User is waiting.

Examples:

```text
submit attendance
save profile
send chat message
```

### Normal

Routine synchronization.

### Bulk

Examples:

```text
import 5,000 students
large offline catch-up
```

### Background

Examples:

```text
anti-entropy
routine metadata refresh
```

### Maintenance

Examples:

```text
journal verification
deep queue optimization
```

---

## 6. Operation Priority

Each operation descriptor may declare a default:

```rust
priority: OperationPriority
```

Application may override within bounded policy.

Do not let an untrusted client mark every operation `Critical`.

Server may normalize or cap client priority hints.

---

## 7. Priority vs Dependency

If:

```text
Interactive B depends on Background A
```

A becomes effectively required.

Dependency takes precedence.

Scheduler may temporarily elevate A enough to unblock B.

---

## 8. Effective Priority

Conceptually:

```text
effective priority
=
max(
    operation default,
    caller hint within policy,
    dependency inheritance,
    age-based promotion
)
```

---

## 9. Age-Based Promotion

Low-priority work must not starve forever.

Example:

```text
Background item older than 24h
```

can receive gradual priority boost.

Use bounded aging.

---

## 10. Starvation Invariant

> **Any eligible durable operation must eventually receive execution opportunity under sustained service availability and fairness assumptions.**

This is a liveness property.

---

## 11. Eligibility

An item is eligible only if:

```text
dependencies satisfied
retry backoff expired
network policy permits
client not paused
coordinator leadership valid
required auth available
storage state healthy
```

---

## 12. Network Context

Define:

```rust
pub struct NetworkContext {
    pub online: bool,
    pub metered: Option<bool>,
    pub roaming: Option<bool>,
    pub estimated_bandwidth: Option<BandwidthEstimate>,
    pub estimated_rtt: Option<Duration>,
}
```

Platform integrations provide hints.

These are advisory, not truth.

---

## 13. Connectivity Hint Principle

OS says online:

```text
may still fail
```

OS says metered:

```text
policy hint
```

Actual request outcome remains authoritative for reachability.

---

## 14. Power Context

```rust
pub struct PowerContext {
    pub charging: Option<bool>,
    pub battery_level: Option<u8>,
    pub low_power_mode: Option<bool>,
}
```

Only platform helper crates should gather these signals.

Core scheduler consumes normalized context.

---

## 15. Application Activity Context

```rust
pub enum AppActivity {
    ForegroundInteractive,
    ForegroundIdle,
    Background,
    SuspendedImminent,
}
```

---

## 16. QoS Policy Input

Scheduler decision consumes:

```text
WorkClass
operation age
network context
power context
application activity
server hints
queue depth
storage pressure
recent sync latency
```

---

## 17. Policy Output

```rust
pub struct SchedulingDecision {
    pub eligible: bool,
    pub priority: EffectivePriority,
    pub max_batch_ops: usize,
    pub max_batch_bytes: usize,
    pub concurrency: usize,
    pub compression: CompressionDecision,
}
```

---

## 18. Safe Defaults

If platform signals are unavailable:

```text
assume normal network
use conservative batching
avoid aggressive background work
```

Do not fail synchronization because battery/network metadata is missing.

---

## 19. Interactive Fast Path

For small interactive operations:

```text
commit local mutation
↓
wake coordinator
↓
short debounce
↓
small immediate batch
```

Target low user-perceived latency.

---

## 20. Debounce

A short debounce can collect bursts.

Example:

```text
50–200 ms
```

for interactive UI bursts.

Do not hardcode one value globally.

---

## 21. Burst Coalescing

Example:

```text
user edits 4 fields quickly
```

Coordinator waits a short interactive window and sends one batch.

Part 04 may compact unsent replaceable operations first.

---

## 22. Background Batch Strategy

Background work favors:

```text
larger batches
fewer wakeups
higher compression
```

to improve energy/network efficiency.

---

## 23. Metered Network Policy

On metered network:

Prefer:

```text
interactive operations
small normal sync
```

Defer where policy permits:

```text
anti-entropy
large snapshots
bulk imports
large blob transfer
```

---

## 24. Never Defer Safety-Critical Data Blindly

If product policy says a critical operation must sync immediately:

```text
metered network does not block it
```

QoS policy must be domain configurable.

---

## 25. Roaming Policy

Roaming may be treated more conservatively than merely metered.

Example:

```text
interactive allowed
bulk paused
background paused
```

unless user opts in.

---

## 26. Charging Policy

When charging:

```text
run deferred anti-entropy
larger maintenance batches
snapshot verification
```

especially on mobile.

---

## 27. Low-Power Mode

Reduce:

```text
background concurrency
verification frequency
compression CPU level
polling frequency
```

but retain durable sync correctness.

---

## 28. CPU vs Network Tradeoff

Compression saves bandwidth but costs CPU/battery.

Decision can use:

```text
payload size
network type
power state
CPU budget
```

---

## 29. Compression Policy

Example:

```text
Wi-Fi + charging:
    normal zstd

metered + battery:
    compress larger payloads, moderate level

tiny payload:
    no compression
```

---

## 30. Adaptive Batch Size

Batch size can react to recent outcomes.

Increase when:

```text
low latency
stable server
fast network
low error rate
```

Decrease when:

```text
timeouts
429/503
high RTT
server hints
memory pressure
```

---

## 31. AIMD Strategy

A simple robust strategy is similar to additive-increase/multiplicative-decrease.

Example:

```text
success streak:
    + small increment

overload/timeout:
    batch *= 0.5
```

Bound by configured min/max.

---

## 32. Why Simple Control Is Better

Avoid ML/adaptive complexity initially.

A deterministic bounded controller is:

```text
easier to test
easier to explain
safer
```

---

## 33. Batch Dimensions

A batch is bounded by:

```text
operation count
encoded bytes
dependency group
priority class
time window
```

Stop when any cap is reached.

---

## 34. Priority Mixing

Do not let a large bulk batch delay interactive work.

Options:

```text
separate batches by priority
or
cap low-priority occupancy
```

Recommended:

```text
interactive batches separate
```

---

## 35. Queue Selection

Conceptual weighted scheduler:

```text
Critical      weight 16
Interactive   weight 8
Normal        weight 4
Bulk          weight 2
Background    weight 1
```

But dependency and age promotion can alter effective selection.

---

## 36. Weighted Fair Queuing

Aequora can use a simplified weighted fair scheduler.

Purpose:

```text
serve high priority quickly
without permanent starvation
```

---

## 37. Per-Entity Fairness

Avoid one noisy entity generating endless operations and starving others.

Part 04 compaction helps.

Scheduler may also cap consecutive operations per entity/aggregate.

---

## 38. Per-Scope Fairness

If one client syncs multiple scopes:

```text
one huge scope
```

should not permanently starve smaller scope updates.

Round-robin or weighted scheduling can help.

---

## 39. Dependency Component Scheduling

Operations in one dependency DAG component should be scheduled coherently.

Do not split required prerequisite after dependent due to priority.

---

## 40. Local Operation Sequence

Part 04 `LocalOperationSeq` provides deterministic tie-breaking.

Within equal priority/eligibility:

```text
oldest local seq first
```

is a good default.

---

## 41. Retry Scheduling

Transient failures use:

```text
exponential backoff
+
jitter
```

Priority affects retry urgency but must not create retry storms.

---

## 42. Retry Budget

Per-operation or per-batch backoff should be bounded by:

```text
initial
max
server Retry-After
```

---

## 43. Server Retry-After

Server hints should normally dominate client retry timing.

Example:

```text
Retry-After: 15s
```

Client should not hammer again after 500 ms.

---

## 44. Stable Jitter

Use randomized or stable jitter to prevent synchronized fleets.

For recurring background jobs, stable jitter based on `DeviceId` is useful.

---

## 45. Reconnect Storm

After outage:

```text
100,000 clients reconnect
```

Client behavior:

```text
randomized initial delay
bounded exponential recovery
honor server hints
```

---

## 46. Immediate User Action During Backoff

If normal background sync is backing off and user performs an interactive action:

```text
may trigger earlier retry
```

within safety limits.

Do not reset backoff endlessly due to UI noise.

---

## 47. Circuit Breaker

Repeated server unavailability can enter:

```text
Open
```

state.

Scheduler pauses frequent attempts.

After cooldown:

```text
HalfOpen
```

tries a probe.

---

## 48. Circuit Breaker Scope

Could be:

```text
per server endpoint
```

not per individual operation.

---

## 49. Authentication Failures

Do not treat:

```text
401 due expired token
```

as ordinary network retry.

Invoke credential refresh path.

If refresh impossible:

```text
AuthRequired
```

and pause server traffic until resolved.

---

## 50. Authorization Rejection

Permanent domain authorization rejection should finalize that operation.

Scheduler must not retry forever.

---

## 51. Server Load Hints

Sync response may include:

```rust
pub struct ServerSchedulingHints {
    pub retry_after_ms: Option<u64>,
    pub preferred_max_batch_ops: Option<u32>,
    pub preferred_max_batch_bytes: Option<u32>,
    pub background_allowed: Option<bool>,
}
```

---

## 52. Hints Are Bounded

Client applies server hints within local hard safety bounds.

Server cannot tell client to allocate unbounded batch memory.

---

## 53. QoS Protocol Versioning

Scheduling hints should be optional capabilities.

Older clients ignore them safely.

---

## 54. Request Deadline

Scheduler assigns an internal deadline per work item/batch.

Do not start work that cannot reasonably finish before mobile background execution expires.

---

## 55. Mobile Background Budget

Platform helper may report:

```text
remaining execution budget
```

or simply background mode.

Scheduler selects:

```text
small batches
no expensive anti-entropy
no huge snapshot
```

---

## 56. Suspension Imminent

If app is about to suspend:

```text
finish current safe transaction
avoid starting large work
persist scheduler state
```

No correctness dependence on completing network request.

---

## 57. Scheduler State Persistence

Persist only what is necessary.

Examples:

```text
next retry time
server backoff hint
adaptive batch target
circuit breaker state
```

Avoid persisting noisy transient counters unless useful.

---

## 58. Restart Behavior

On restart:

```text
load durable retry timing
normalize stale transient state
resume
```

Do not hammer server immediately if a long Retry-After is still valid.

---

## 59. Monotonic vs Wall Clock

Persisted retry time requires wall-clock-like representation.

Process-local scheduling uses monotonic timers.

On clock anomalies:

```text
bound delay
avoid negative/huge unexpected sleep
```

---

## 60. Maximum Retry Deferral

Even with bad clock/server hint, cap retry deferral according to policy unless explicitly maintenance-paused.

---

## 61. Maintenance Mode

If server reports:

```text
maintenance
```

scheduler:

```text
keeps local writes
retains outbox
backs off
```

No permanent rejection.

---

## 62. Bootstrap Scheduling

Bootstrap is heavy.

Priority:

```text
required bootstrap to make app usable
```

may outrank routine background sync.

But large bootstrap should still:

```text
chunk
resume
yield between chunks
```

---

## 63. Partial Bootstrap Usability

If architecture permits module/scope-specific bootstrap:

```text
critical scope first
less important scopes later
```

Part 07 will define dynamic scopes.

---

## 64. Anti-Entropy Scheduling

Part 03 anti-entropy defaults to:

```text
Background
```

unless:

```text
corruption suspected
```

then elevate to:

```text
Critical/Repair
```

---

## 65. Repair Scheduling

Small authoritative repair can outrank routine upload if local correctness is compromised.

Pending user operations touching repaired entities may wait for repair/rebase.

---

## 66. Part 04 Compaction Scheduling

Eager small compaction:

```text
Interactive-adjacent
```

Deep compaction:

```text
Maintenance
```

Do not let deep compaction delay urgent sync.

---

## 67. Part 05 Leader Coordination

Only current local leader runs scheduler execution.

Followers may enqueue work and wake leader.

Leadership change should preserve durable scheduling state.

---

## 68. Work Item Model

Conceptual:

```rust
pub struct ScheduledWork {
    pub work_id: WorkId,
    pub class: WorkClass,
    pub earliest_at: InstantOrPersistedTime,
    pub deadline: Option<Deadline>,
    pub dependencies: SmallVec<[WorkId; 4]>,
}
```

Do not necessarily persist all work as separate rows; operations already exist in outbox.

---

## 69. Work Kinds

```rust
pub enum WorkKind {
    PushOperations,
    PullChanges,
    Bootstrap,
    IntegrityCheck,
    Repair,
    QueueCompaction,
    BlobTransfer,
    Maintenance,
}
```

---

## 70. Scheduler Queues

Logical queues:

```text
interactive
normal
bulk
background
maintenance
```

Could be implemented as indexed DB queries rather than in-memory priority queues.

---

## 71. Durable Source of Work

Durable work remains in:

```text
outbox
bootstrap state
repair state
maintenance metadata
```

The in-memory scheduler is only a planner.

---

## 72. Crash Safety

If scheduler crashes:

```text
durable work remains
```

No special recovery log required for ordinary planning.

---

## 73. Concurrency Model

Client should use small bounded concurrency.

Example:

```text
1 sync exchange
1 blob transfer
1 background verification
```

depending on device profile.

Do not launch dozens of concurrent sync exchanges.

---

## 74. Default Single Exchange

For normal operation:

```text
one exchange at a time per store
```

is simplest and avoids cursor/reconciliation complexity.

---

## 75. Parallel Blob Transfer

Blob subsystem may run independently because large files should not block metadata sync.

Use separate concurrency limits.

---

## 76. Server-Side QoS

Client QoS alone is insufficient.

Server needs admission/fairness, covered deeply in Part 18.

Client should cooperate but never assume server will accept work.

---

## 77. Server Overload Response

On:

```text
429
503
```

scheduler reduces:

```text
concurrency
batch size
retry frequency
```

---

## 78. Adaptive Batch Controller

Maintain:

```rust
pub struct BatchController {
    pub target_ops: usize,
    pub target_bytes: usize,
    pub success_streak: u32,
}
```

---

## 79. Controller Inputs

Update after:

```text
successful request latency
timeout
server overload
response too large
memory pressure
```

---

## 80. Deterministic Bounds

Even adaptive controller must obey:

```text
min batch
max batch
max bytes
max ops
```

from validated config.

---

## 81. RTT Measurement

HTTP transport may report request duration.

Use EWMA:

```text
smoothed_rtt
```

for scheduling hints.

No need for precise network measurement framework.

---

## 82. Bandwidth Estimate

Optional estimate from:

```text
recent payload bytes / duration
platform network API
```

Use coarse categories:

```text
Slow
Normal
Fast
```

rather than pretending high precision.

---

## 83. Scheduler Profiles

Provide:

```rust
SyncProfile::Desktop()
SyncProfile::Mobile()
SyncProfile::HighLatency()
SyncProfile::LowBandwidth()
SyncProfile::EnterpriseLan()
```

Profiles set defaults only.

---

## 84. Desktop Profile

Typical:

```text
fast interactive flush
moderate batch size
routine anti-entropy
background work allowed
```

---

## 85. Mobile Profile

Typical:

```text
foreground fast path
metered-awareness
battery-aware background work
larger debounce for background
strict concurrency
```

---

## 86. High-Latency Profile

Increase:

```text
batch size
request timeout
```

while reducing unnecessary round trips.

---

## 87. LAN Profile

Can favor:

```text
smaller low-latency batches
less compression
```

if bandwidth cheap and RTT low.

---

## 88. Custom Policy Trait

Advanced applications can implement:

```rust
pub trait SchedulingPolicy {
    fn decide(
        &self,
        ctx: &SchedulingContext,
        work: &WorkDescriptor,
    ) -> SchedulingDecision;
}
```

---

## 89. Pure Policy Requirement

Prefer policy function to be:

```text
deterministic
side-effect free
```

This makes testing easier.

---

## 90. Policy Version

Custom policy may expose:

```text
policy version
```

for diagnostics.

---

## 91. Untrusted Client Priority

Server must not trust QoS priority for authorization or financial importance.

Priority is scheduling metadata only.

---

## 92. Security Abuse

A malicious client may mark everything urgent.

Server applies its own admission/fairness classes.

---

## 93. Queue Storage Indexes

Useful indexes:

```text
state
priority
next_attempt_at
local_seq
entity
scope
```

---

## 94. Selection Query

Logical:

```text
eligible pending
ORDER BY effective priority DESC,
         age promotion DESC,
         local_seq ASC
LIMIT ...
```

Actual effective priority may be computed partially in memory.

---

## 95. Aging Without Rewriting Rows

Avoid updating every pending row periodically.

Compute age-based boost at selection time.

---

## 96. Batch Dependency Validation

Before sending selected operations:

```text
ensure all intra-batch dependency order valid
ensure unresolved prerequisite included or already committed
```

---

## 97. Priority Inversion

If low-priority prerequisite blocks high-priority operation:

```text
temporarily inherit higher priority
```

This prevents inversion.

---

## 98. Fairness Across Tenants on Shared Client

If one app stores multiple tenants/accounts:

```text
weighted round-robin
```

can avoid starvation.

---

## 99. User-Triggered Sync Now

`sync_now()` should:

```text
wake scheduler
prioritize eligible interactive/normal work
```

It should not bypass:

```text
backoff from hard server maintenance
dependency rules
auth failure
```

---

## 100. Pause/Resume

Public API:

```text
pause()
resume()
```

Pause stops network activity but preserves local writes/outbox.

---

## 101. Policy Pause

Examples:

```text
Wi-Fi only
battery saver
user disabled background sync
```

Represent as scheduler constraints, not rejected operations.

---

## 102. Emergency Override

Applications may explicitly allow:

```text
Critical work despite Wi-Fi-only policy
```

if business requirements demand it.

This must be deliberate.

---

## 103. Data Usage Budget

Optional:

```text
max background bytes/day
```

for mobile/enterprise policies.

Interactive work may have separate allowance.

---

## 104. Byte Accounting

Track approximate:

```text
payload sent
payload received
blob bytes
snapshot bytes
```

Do not promise carrier-billing precision.

---

## 105. Blob QoS

Large blob upload/download uses separate class.

Example:

```text
metadata sync first
blob background later
```

unless blob required to commit operation.

---

## 106. Required Blob Dependency

If operation references a blob that server must have first:

```text
blob upload becomes prerequisite
```

Scheduler honors dependency.

---

## 107. Snapshot QoS

Large snapshot:

```text
chunked
resumable
yielding
```

Part 10 will define streaming bootstrap deeply.

---

## 108. Work Preemption

Do not attempt unsafe preemption inside database transaction.

Scheduler may stop between:

```text
batches
chunks
operations
```

not in the middle of a commit.

---

## 109. Cooperative Yield

Long background jobs should periodically yield so interactive work can run.

Example:

```text
after every N snapshot chunks
```

---

## 110. Rayon QoS

Rayon compute tasks should have separate queue/concurrency budget.

Do not let background hashing occupy all CPU while user waits for interactive validation.

---

## 111. CPU Priority Classes

Potential internal pools:

```text
interactive compute
background compute
```

Initial simpler design:

```text
one bounded pool
interactive tasks submitted first
background chunk sizes limited
```

---

## 112. Memory Pressure

If client memory pressure detected:

```text
reduce batch bytes
reduce concurrent decode
avoid large verification
```

---

## 113. Disk Pressure

If disk critically low:

```text
prioritize syncing/compacting pending work
defer large downloads
```

Part 04 rule still applies:

```text
never drop pending intent
```

---

## 114. Scheduler State Machine

```text
Dormant
 ↓ trigger
Evaluate
 ↓
SelectWork
 ↓
BuildBatch
 ↓
Execute
 ↓
ObserveResult
 ├─ Success → UpdateController
 ├─ Retryable → Backoff
 ├─ Overload → ReduceLoad
 ├─ AuthRequired → PauseAuth
 └─ Maintenance → PauseUntilHint
```

---

## 115. Triggers

```text
local mutation
network restored
manual sync
retry timer
foreground resume
background task start
server push hint
bootstrap requirement
repair requirement
```

---

## 116. Trigger Coalescing

Many triggers collapse into one wake-up.

Do not enqueue 1,000 identical `LocalMutation` signals.

---

## 117. Event-Driven + Timer Hybrid

Use:

```text
event wakeups
+
periodic safety timer
```

This avoids busy polling and lost-signal problems.

---

## 118. Durable Retry Time

Each retryable outbox entry may have:

```text
next_attempt_at
```

Batch scheduler selects only due entries.

---

## 119. Shared Batch Retry

If transport batch fails ambiguously:

```text
individual operations remain retryable with same IDs
```

Do not generate new IDs.

---

## 120. Partial Response

If server returns per-operation results:

```text
accepted ops finalize
retry-later ops reschedule
rejected ops finalize
```

Scheduler operates at operation granularity after reconciliation.

---

## 121. Server Pull Scheduling

Even with no outgoing operations, client periodically pulls.

Frequency can adapt:

```text
foreground active -> faster
background idle -> slower
push hints available -> much slower baseline
```

---

## 122. Push Hint Integration

Future Part 08 push hint:

```text
wake client
```

but normal cursor pull remains correctness path.

---

## 123. Polling Backoff

When no changes:

```text
increase idle poll interval
```

When recent activity:

```text
decrease
```

within bounds.

---

## 124. Active Session Heuristic

If user is actively editing collaborative data:

```text
keep shorter pull interval
```

If app idle:

```text
lengthen
```

---

## 125. Privacy

Scheduler should not infer sensitive user behavior for remote telemetry unnecessarily.

Activity state can remain local.

---

## 126. Server Hint Privacy

Server only needs operational hints, not battery percentage or detailed device state.

Do not upload private platform context unless needed.

---

## 127. Observability

Client metrics:

```text
scheduler_work_selected_total
scheduler_batch_ops
scheduler_batch_bytes
scheduler_retry_delay
scheduler_backoff_state
scheduler_deferred_background_total
scheduler_interactive_latency
```

---

## 128. Logs

Structured events:

```text
work_selected
batch_adapted
server_overload_backoff
network_policy_defer
circuit_open
circuit_half_open
```

---

## 129. Avoid Noisy Logs

Do not log every scheduler tick.

Log state changes and significant decisions.

---

## 130. Diagnostics

Expose:

```text
current profile
effective batch target
network class
circuit state
next retry
deferred work count
```

---

## 131. Testing Strategy

Use deterministic fake:

```text
clock
network context
power context
server hints
transport outcomes
```

---

## 132. Property Tests

Assert:

```text
critical eligible work outranks background
dependencies cannot be bypassed
low-priority work eventually ages upward
batch never exceeds hard bounds
server Retry-After respected
```

---

## 133. Simulation

Generate:

```text
network flaps
battery changes
foreground/background
server 429
server timeout
user actions
```

Measure/verify scheduler state transitions.

---

## 134. Model Checking

Part 01 model can abstract scheduling to verify:

```text
no eligible operation is permanently starved under fairness assumptions
```

within bounded model.

---

## 135. Fault Injection

Inject:

```text
timer cancellation
clock jump
status signal loss
leadership change mid-backoff
server hint corruption
```

Validate safe fallback.

---

## 136. Scheduler Invariants

Add:

### AEQ-INV-QOS001

```text
Scheduling never changes OperationId or semantic payload.
```

### AEQ-INV-QOS002

```text
An ineligible operation is never transmitted before dependencies and retry constraints permit it.
```

### AEQ-INV-QOS003

```text
No batch exceeds configured hard operation/byte limits.
```

### AEQ-INV-QOS004

```text
Low-priority eligible durable work cannot be permanently starved under scheduler fairness assumptions.
```

### AEQ-INV-QOS005

```text
Server overload hints can reduce work but cannot cause durable intent loss.
```

### AEQ-INV-QOS006

```text
Leadership changes preserve durable retry state.
```

---

## 137. Recommended Modules

```text
aequora-client/
└── scheduler/
    ├── mod.rs
    ├── policy.rs
    ├── priority.rs
    ├── eligibility.rs
    ├── batch_controller.rs
    ├── retry.rs
    ├── circuit_breaker.rs
    ├── context.rs
    └── metrics.rs
```

Platform helpers:

```text
aequora-platform-desktop
aequora-platform-android
```

Core scheduler remains platform-neutral.

---

## 138. Configuration

Example:

```ron
scheduler: (
    profile: Mobile,
    interactive_debounce_ms: 100,

    batch: (
        min_ops: 1,
        target_ops: 64,
        max_ops: 256,
        max_bytes: 1048576,
    ),

    retry: (
        initial_ms: 500,
        max_ms: 30000,
        multiplier: 2.0,
        jitter: true,
    ),

    idle_pull: (
        active_ms: 5000,
        idle_ms: 60000,
        max_ms: 300000,
    ),

    metered: (
        defer_bulk: true,
        defer_background: true,
    ),
)
```

---

## 139. Plug-and-Play Defaults

Most developers should only choose:

```rust
SyncProfile::Desktop
```

or:

```rust
SyncProfile::Mobile
```

Advanced policy configuration should remain optional.

---

## 140. Completion Criteria

Part 06 is complete when:

```text
[ ] WorkClass defined
[ ] operation priority metadata defined
[ ] eligibility model defined
[ ] network/power/activity context normalized
[ ] adaptive batch controller defined
[ ] retry/circuit-breaker model defined
[ ] metered/roaming rules defined
[ ] fairness/aging defined
[ ] dependency priority inheritance defined
[ ] background/interactive separation defined
[ ] bootstrap/anti-entropy/blob QoS integration defined
[ ] scheduler state persistence defined
[ ] server scheduling hints defined
[ ] deterministic tests specified
[ ] QoS invariants added
[ ] profiles and configuration defined
```

---

## 141. Final Architecture

```text
                     DURABLE WORK
                         │
          ┌──────────────┼──────────────┐
          ▼              ▼              ▼
       Outbox         Bootstrap       Repair
          │              │              │
          └──────────────┼──────────────┘
                         ▼
                Eligibility Filter
                         │
                         ▼
                   QoS Classifier
                         │
          ┌──────────────┼──────────────┐
          ▼              ▼              ▼
     Interactive       Normal       Background
          │              │              │
          └──────────────┼──────────────┘
                         ▼
              Fair Priority Scheduler
                         │
                         ▼
               Adaptive Batch Builder
                         │
                         ▼
                 Transport Execution
                         │
             ┌───────────┼────────────┐
             ▼           ▼            ▼
          Success     Overload      Failure
             │           │            │
             ▼           ▼            ▼
        Increase      Reduce       Backoff
        cautiously     batch        + jitter
```

The architectural principle is:

> **Aequora should synchronize the most important eligible work as quickly as practical, defer expensive nonurgent work when the environment is constrained, and continuously adapt without ever weakening correctness or losing durable intent.**

This makes the same synchronization core suitable for desktops, mobile devices, unreliable networks, enterprise LANs, and large offline reconnect scenarios.
