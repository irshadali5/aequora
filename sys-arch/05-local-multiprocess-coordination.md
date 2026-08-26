# Aequora Sync — Part 05

# Local Multi-Process, Multi-Window, and Coordinator Election Architecture

## 1. Purpose

Aequora clients may run as desktop apps, mobile apps, background workers, helper processes, CLIs, or multiple windows. More than one process can therefore open the same local store.

Without coordination, multiple processes may independently run the sync loop and cause:

- duplicate uploads;
- competing retries;
- cursor races;
- duplicate bootstrap/anti-entropy work;
- redundant compaction;
- unnecessary network and CPU use.

Server-side `OperationId` idempotency prevents duplicate authoritative effects, but local coordination is still required.

> **Exactly one active synchronization coordinator should control one local sync store at a time, while other processes remain safe readers/writers and can automatically take over after failure.**

---

## 2. Scope

This subsystem coordinates processes sharing one local Aequora store. It is **not** server consensus, Raft, or global leader election.

The coordination key is conceptually:

```text
DeviceId
+
LocalStoreId
```

where:

- `DeviceId` is the stable installation/device identity;
- `LocalStoreId` is the persistent identity of the local sync store;
- `ProcessInstanceId` identifies one runtime process.

---

## 3. Roles

Each process can be:

```rust
enum CoordinatorRole {
    Leader,
    Follower,
    Candidate,
    Observer,
    Maintenance,
    Stopping,
}
```

### Leader

Only the leader runs:

- network sync;
- retry scheduling;
- journal pull;
- bootstrap;
- anti-entropy;
- deep queue compaction;
- reconciliation;
- repair jobs.

### Follower

Followers may:

- read local data;
- perform local domain transactions;
- atomically append outbox operations;
- observe sync status;
- wake the leader.

They do **not** send synchronization traffic.

### Observer

Read-only tools never participate in election.

### Maintenance

Migration/repair tools may acquire an exclusive maintenance lease that temporarily blocks normal synchronization leadership.

---

## 4. Local Writes Must Not Require Leadership

A follower must still be able to commit:

```text
domain mutation
+
outbox operation
```

in one ACID transaction.

Leadership is needed only for coordinator-owned work.

This preserves offline-first behavior even if the current leader is another process or is temporarily unavailable.

---

## 5. Persistent Store Identity

Define:

```rust
pub struct LocalStoreId(Uuid);
```

Generate it when Aequora metadata is first initialized and persist it permanently.

Do not derive it only from a file path; databases can be moved.

---

## 6. Process Identity

Each runtime generates:

```rust
pub struct ProcessInstanceId(Uuid);
```

It is ephemeral and changes every process start.

Do not confuse it with `DeviceId`.

---

## 7. Lease-Based Election

The recommended portable mechanism is a durable database lease.

Logical record:

```rust
pub struct CoordinatorLease {
    pub store_id: LocalStoreId,
    pub owner_id: ProcessInstanceId,
    pub fencing_token: FencingToken,
    pub expires_at: LeaseTimestamp,
}
```

Only one unexpired lease may exist for one store.

---

## 8. Fencing Tokens

Define:

```rust
pub struct FencingToken(u64);
```

Every successful leadership acquisition increments the token.

Example:

```text
Process A -> token 41
A pauses
lease expires
Process B -> token 42
A resumes
```

Process A may still believe it is leader, but any leader-exclusive local transaction carrying token 41 must be rejected because token 42 is current.

> **The fencing token, not process belief, determines local coordinator authority.**

---

## 9. Core Fencing Invariant

```text
Only the holder of the highest committed fencing token may commit leader-exclusive synchronization state transitions.
```

This prevents stale-leader split brain.

---

## 10. Logical Lease Schema

```text
aequora_coordinator_lease
```

Fields:

```text
store_id
owner_id
fencing_token
expires_at
last_heartbeat
lease_kind
```

A single row per local store is sufficient for the first implementation.

---

## 11. Lease Acquisition

The adapter performs an atomic transaction:

```text
BEGIN

read current lease

if no lease or expired:
    next_token = current_token + 1
    set owner = candidate
    set token = next_token
    set expiry = now + TTL

COMMIT
```

Two simultaneous candidates must not both succeed.

Use conditional update, compare-and-swap, or transaction serialization.

---

## 12. Lease Renewal

The leader periodically renews only if:

```text
owner_id == current process
AND
fencing_token == current token
```

If renewal fails, it immediately transitions out of leader mode.

---

## 13. Lease Duration and Heartbeat

Example starting values:

```text
lease TTL: 15 seconds
heartbeat: 5 seconds
```

The exact production values should be configurable and tested on target platforms.

Heartbeat work must be lightweight and higher priority than background compaction or verification.

---

## 14. Clock Considerations

A persisted lease cannot rely purely on a process-local monotonic clock.

Use a practical combination of:

```text
database/system time
+
short heartbeat
+
fencing token
+
optional OS locking
```

Fencing is what protects correctness if clock behavior is imperfect.

---

## 15. Hybrid Coordination

Recommended production model:

```text
durable DB lease + fencing token
+
best-effort OS/process lock or IPC
```

The DB lease defines semantics.

OS mechanisms optimize detection/wakeup.

---

## 16. File Lock Optimization

For file-backed embedded databases, Aequora may also acquire:

```text
aequora-coordinator.lock
```

using real OS locking.

Do not use plain file existence as leadership proof.

A stale file must never permanently block takeover.

---

## 17. Election State Machine

```text
Follower
   ↓ lease expired
Candidate
   ↓ acquire succeeds
Leader
   ↓ renewal failure / shutdown / handoff
Stopping
   ↓ release
Follower
```

Failed candidates use jittered backoff.

Healthy leaders should not be challenged.

---

## 18. Graceful Release

On clean shutdown:

```text
leader stops new work
finishes/rolls back in-flight local transaction
releases lease
```

A follower can then acquire immediately.

Correctness must not depend on graceful shutdown; crashes are expected.

---

## 19. Crash Takeover

```text
Leader crashes
↓
heartbeat stops
↓
lease expires
↓
Follower becomes Candidate
↓
new fencing token allocated
↓
new Leader resumes synchronization
```

No manual action.

---

## 20. Old Leader Resumption

A suspended process may resume after losing leadership.

Before any leader-exclusive commit it verifies the fencing token.

If stale:

```text
rollback / abandon local coordinator action
transition to Follower
```

---

## 21. Leader-Exclusive Operations

Require current fencing token for:

- marking batches in-flight;
- reconciliation commits;
- cursor updates owned by sync loop;
- bootstrap state changes;
- anti-entropy repair;
- deep compaction;
- coordinator status transitions;
- store-generation changes.

---

## 22. Fenced Reconciliation

Before reconciliation commit:

```text
verify fencing token
apply authoritative events
mark operation results
update conflicts
advance cursor
commit
```

If the token is stale, rollback.

The new leader can retry safely.

---

## 23. In-Flight Network Requests

A leader can lose its lease after sending a request.

That request may still reach the server.

This is safe because:

```text
server: OperationId idempotency
client: local fencing
```

The old leader must not reconcile after leadership loss. The new leader retries/pulls authoritative state.

---

## 24. Outbox Wake-Up

After a follower commits a new outbox item:

```text
commit
↓
best-effort signal leader
```

Possible wakeup mechanisms:

- local IPC;
- Unix socket;
- named pipe;
- database notification marker;
- platform event;
- polling fallback.

Correctness never depends on the signal.

---

## 25. Polling Fallback

The leader periodically checks for pending work.

Therefore:

```text
lost IPC wakeup != lost synchronization
```

---

## 26. Same-Process Multi-Window

If several Dioxus windows share one process, prefer one:

```rust
Arc<AequoraClientHandle>
```

owned by application state.

Do not create one coordinator per window.

Cross-process election is only necessary for independent runtimes.

---

## 27. Foreground and Background Processes

Mobile and desktop systems may have:

```text
foreground app
background sync worker
```

Both open the same store and join the same lease election.

Only one syncs.

---

## 28. Foreground Preference

Foreground process may be preferable because it has:

- active credentials;
- better execution budget;
- interactive urgency.

But avoid involuntary lease preemption initially.

Recommended v1:

```text
healthy current leader stays leader
```

A background leader may voluntarily hand off to foreground.

---

## 29. Cooperative Handoff

```text
foreground requests handoff
↓
background leader stops new work
↓
finishes current transaction
↓
releases lease
↓
foreground acquires next fencing token
```

If handoff fails, the existing leader remains valid.

---

## 30. Credential-Aware Leadership

A process that cannot authenticate should not remain leader indefinitely if another capable process is available.

Optional behavior:

```text
leader detects unusable credentials
↓
releases lease
↓
another candidate acquires
```

---

## 31. Capability-Aware Candidate

A candidate may be eligible only when:

```text
store writable
network use permitted
credentials available
platform allows sync execution
```

Observers and restricted background processes can remain followers.

---

## 32. Maintenance Lease

Define a stronger lease class:

```rust
enum LeaseKind {
    SyncCoordinator,
    Maintenance,
}
```

Maintenance is used for:

- local schema migration;
- store conversion;
- destructive repair;
- generation replacement.

A maintenance lease prevents ordinary sync leadership.

---

## 33. Migration Coordination

When multiple processes start against an old metadata schema:

```text
one process acquires maintenance/migration lease
↓
performs migration
↓
updates metadata version
↓
releases
↓
others reopen/continue
```

No duplicate migration execution.

---

## 34. Mixed Binary Versions

Scenario:

```text
old app process running
new app process starts
new app migrates local Aequora metadata
```

The old process must detect that metadata is outside its supported range and stop unsupported writes/sync.

Never allow an old runtime to mutate a schema it no longer understands.

---

## 35. Local Store Generation

Define:

```rust
pub struct LocalStoreGeneration(u64);
```

Increment it after:

- full bootstrap generation swap;
- local DB engine migration;
- destructive repair;
- store replacement.

A leader caches the generation. If it changes unexpectedly, the process stops leader work and reloads the store.

---

## 36. Why Store Generation Matters

A process may hold stale handles or assumptions after another process atomically replaces the local replica generation.

Generation fencing prevents it from continuing against stale state.

---

## 37. Bootstrap Coordination

Only the leader may perform bootstrap.

Recommended large-bootstrap design:

```text
active generation A
staging generation B
↓
download/install/verify B
↓
atomic generation switch
↓
followers detect LocalStoreGeneration change
```

---

## 38. Follower Refresh

Followers detect changes via:

```text
IPC hint
or
periodic generation check
```

After change:

```text
invalidate caches
re-open repository views if necessary
refresh UI subscriptions
```

---

## 39. IPC Architecture

Optional platform adapters may provide:

```text
Unix domain sockets
Windows named pipes
Android-specific local integration
```

Messages can include:

```text
NewOutboxWork
LeadershipChanged
StoreGenerationChanged
SyncStatusChanged
HandoffRequested
```

All are hints, never durable truth.

---

## 40. Shared Sync Status

Followers need visibility into sync health.

Persist low-frequency durable status:

```text
leader fencing token
last successful sync
last error code
last server contact
store generation
```

High-frequency ephemeral details can stay process-local.

---

## 41. Logical Status Table

```text
aequora_local_sync_status
```

Potential fields:

```text
store_id
leader_owner_id
fencing_token
last_success_at
last_error_code
phase
updated_at
store_generation
```

---

## 42. Status Staleness

A follower must not treat an old status row as current forever.

Expose:

```text
status updated_at
lease expiry
```

so UI can infer stale coordinator status.

---

## 43. Part 04 Compaction Integration

Deep/offline queue compaction is leader-only.

Follower-side enqueue may perform tiny transaction-local eager compaction only when safe.

If two processes race to enqueue compatible operations, correctness remains valid even if they are not immediately compacted; the leader can optimize later.

---

## 44. Part 04 Rebase Integration

The leader applies incoming authoritative state and runs eligible rebase of unsent operations.

Followers merely observe resulting local state.

---

## 45. Part 03 Anti-Entropy Integration

Only leader runs:

```text
integrity verification
repair
Merkle comparison
```

Repair commits are fenced.

---

## 46. Part 02 Lineage Integration

Leadership changes are operational events.

They do **not** change:

```text
OperationId
CorrelationId
Causation
business provenance
```

A retry by a new process remains the same logical operation.

---

## 47. Part 01 Correctness Integration

The abstract model should be extended with:

```text
two local processes
one shared store
lease
fencing token
```

and verify stale leaders cannot commit coordinator-owned state.

---

## 48. Device Identity Rule

All processes sharing the store must reuse the same persisted:

```text
DeviceId
```

Never create a new DeviceId per process.

---

## 49. Identity Separation

```text
DeviceId
    stable installation identity

LocalStoreId
    stable local sync-store identity

ProcessInstanceId
    one runtime process

FencingToken
    one coordinator leadership epoch

LocalStoreGeneration
    one physical/logical local replica generation
```

Each serves a different purpose.

---

## 50. Adapter Requirements

A Tier-A local adapter supporting multi-process mode must provide:

```text
atomic lease acquisition
atomic fencing-token increment
lease renewal
lease release
fence verification inside transaction
store generation read/update
```

---

## 51. Capability Declaration

```rust
enum LocalCoordinationSupport {
    Full,
    SingleProcessOnly,
}
```

If an adapter is `SingleProcessOnly`, Aequora must reject multi-process configuration.

---

## 52. Process Mode

```rust
pub enum LocalProcessMode {
    Auto,
    SingleProcess,
    MultiProcess,
}
```

Recommended default:

```text
Auto
```

when the adapter can report safe capability.

---

## 53. SQL-Like Adapter Implementation

For Stoolap/SQLite-like storage:

```text
single lease row
transaction
conditional UPDATE
increment token
```

is the natural implementation.

---

## 54. KV Adapter Implementation

A KV adapter can use:

```text
transactional key
or
compare-and-swap
```

for:

```text
coordinator/lease
```

If it cannot provide atomic compare/update semantics, it cannot claim full multi-process coordination.

---

## 55. Leader Heartbeat Task

Run heartbeat as a dedicated lightweight Tokio task.

It should not be blocked by:

```text
large snapshot transform
Rayon computation
slow UI work
```

If heartbeat fails, broadcast leadership loss immediately.

---

## 56. Long CPU Work

CPU-heavy work belongs in bounded Rayon tasks.

Heartbeat remains responsive on Tokio.

This prevents false lease expiry caused by local CPU starvation.

---

## 57. Lease Loss During Transaction

If a transaction has already started:

- verify the fence before coordinator-owned commit;
- stale token causes rollback;
- do not forcibly interrupt database commit unsafely.

---

## 58. Lease Loss During Network I/O

Network I/O may complete after leadership loss.

Discard/refrain from local reconciliation when stale.

The new leader later reconciles through normal protocol.

---

## 59. No Per-Operation Claims Initially

Do not implement complex per-outbox-item worker leasing in v1.

One leader per store is simpler and adequate.

Per-operation work stealing can be added only if profiling proves the need.

---

## 60. Per-Scope Leadership

Likewise, begin with:

```text
one leader per store
```

not one leader per scope.

This dramatically simplifies local coordination.

---

## 61. Multi-Profile Apps

Separate local DB files/stores:

```text
independent LocalStoreId
independent election
```

One process may lead several independent stores.

---

## 62. Disk Pressure

Any process may detect disk pressure and persist a hint.

Only the leader adjusts:

```text
deep compaction
snapshot timing
anti-entropy timing
```

Local writes still obey the ACID outbox invariant.

---

## 63. Security Model

Processes are assumed to share one local OS/application trust boundary.

Lease IDs are not secrets.

Security depends on:

- OS file permissions;
- DB permissions;
- sandboxing;
- storage integrity.

Fencing provides coordination correctness, not authentication.

---

## 64. Stale Lock Handling

Never interpret a stale file or stale owner ID as permanent ownership.

Only:

```text
current lease + current fencing token
```

matters.

---

## 65. Token Overflow

Use `u64`.

On overflow:

```text
fail closed
require store repair/reinitialization
```

Never wrap to zero.

---

## 66. No Busy Waiting

Followers use:

```text
IPC wakeups
timers
jittered polling
```

not continuous DB polling.

---

## 67. Startup Stampede

If many processes start together:

```text
one acquires lease
others back off with jitter
```

A healthy leader suppresses further election attempts.

---

## 68. Public API

Most developers should need only:

```rust
AequoraClient::builder()
    .process_mode(LocalProcessMode::Auto)
```

Election details remain internal.

---

## 69. Status API

```rust
pub enum CoordinatorStatus {
    Leader { token: FencingToken },
    Follower,
    Observer,
    Maintenance,
    NoLeader,
}
```

UI/support tooling may inspect this.

---

## 70. Diagnostics

Safe diagnostic fields:

```text
store ID
process role
fencing token
lease expiry
store generation
last leadership transition
renew failure count
```

---

## 71. Metrics

Useful metrics:

```text
local_leadership_acquired_total
local_leadership_lost_total
local_lease_renew_failure_total
local_stale_leader_fenced_total
local_handoff_total
```

Do not use process IDs as metric labels.

---

## 72. Logs

Structured events:

```text
coordinator_acquired
coordinator_released
coordinator_lost
stale_leader_fenced
maintenance_acquired
store_generation_changed
```

---

## 73. Leadership Thrashing

If leadership changes repeatedly:

```text
detect
back off
report diagnostic
```

Likely causes:

```text
TTL too short
DB contention
runtime starvation
clock problems
OS suspend/resume
```

---

## 74. Desktop Suspend/Resume

After resume, a process must revalidate its lease before acting.

Another process may have acquired a newer token while it slept.

---

## 75. Mobile Process Death

No graceful shutdown is assumed.

The architecture is explicitly designed for:

```text
kill at any moment
```

Lease expiry and server idempotency recover automatically.

---

## 76. Network Filesystems

Aequora coordination cannot make an embedded DB safe on an unsupported network filesystem.

Database engine storage requirements still apply.

---

## 77. Correctness Invariants

Add to the Part 01 invariant registry.

### AEQ-INV-LC001

```text
At most one fencing token is current for one LocalStoreId.
```

### AEQ-INV-LC002

```text
A stale leader cannot commit fenced reconciliation or bootstrap state.
```

### AEQ-INV-LC003

```text
Follower local mutation + outbox append remains valid without leadership.
```

### AEQ-INV-LC004

```text
After leader failure and lease expiry, an eligible follower can eventually acquire leadership.
```

### AEQ-INV-LC005

```text
Leadership change never changes pending OperationId or semantic payload.
```

### AEQ-INV-LC006

```text
Store-generation change invalidates stale coordinator assumptions.
```

---

## 78. Loom Tests

This subsystem is an excellent Loom target.

Explore:

```text
A and B acquire simultaneously
A renews while B attempts takeover
A releases while B acquires
A resumes with stale token
fenced commit races token increment
```

---

## 79. Property-Based Tests

Generate action streams:

```text
start process
stop process
pause process
renew
acquire
mutate as follower
handoff
migrate
resume
```

Check the coordination invariants after every step.

---

## 80. Failpoint Tests

Inject failure:

```text
after acquire before coordinator starts
before renewal commit
after renewal commit
before release
during maintenance handoff
during generation switch
```

---

## 81. Integration Scenario — Lost Response + Leader Crash

```text
Leader A sends O1
server commits O1
response is lost
A crashes
lease expires
B acquires newer token
B retries O1
server returns prior result
B reconciles
```

Expected:

```text
one authoritative effect
one correct local final state
```

---

## 82. Integration Scenario — Stale Leader Resume

```text
A has token 10
A suspends
B acquires token 11
A resumes
A tries reconciliation
```

Expected:

```text
token 10 rejected
A becomes follower
```

---

## 83. Integration Scenario — Follower Mutation

```text
A leader
B follower
B commits local mutation + outbox
A receives wakeup or polls
A uploads operation
```

Expected:

```text
normal convergence
```

---

## 84. Integration Scenario — Maintenance Migration

```text
leader releases
maintenance lease acquired
migration runs
LocalStoreGeneration increments
maintenance releases
new coordinator elected
followers refresh store generation
```

---

## 85. Adapter Compliance Tests

A multi-process-capable adapter must pass:

```text
simultaneous_acquire_only_one_wins
fencing_token_monotonic
stale_renew_rejected
stale_commit_rejected
crash_takeover
maintenance_excludes_sync
generation_change_detected
```

---

## 86. Recommended Modules

```text
aequora-client/
├── coordinator/
│   ├── mod.rs
│   ├── election.rs
│   ├── lease.rs
│   ├── fencing.rs
│   ├── heartbeat.rs
│   ├── follower.rs
│   └── handoff.rs
```

Adapter SDK:

```text
aequora-adapter-sdk/
└── local_coordination.rs
```

TestKit:

```text
aequora-testkit/
└── multiprocess.rs
```

---

## 87. Logical Local Metadata

```text
aequora_local_store
aequora_coordinator_lease
aequora_local_sync_status
aequora_maintenance_state
```

`aequora_local_store` contains:

```text
store_id
device_id
store_generation
metadata_schema_version
```

---

## 88. Plug-and-Play Behavior

A developer should be able to open the same Aequora-backed local store from two processes without special application logic.

Expected behavior:

```text
Process A starts
→ Leader

Process B starts
→ Follower

Both can edit local data.

Only A performs sync.

A crashes.

B detects expiry
→ Candidate
→ Leader with newer fencing token

A resumes later
→ detects stale token
→ Follower
```

---

## 89. Completion Criteria

Part 05 is complete when:

```text
[ ] LocalStoreId exists
[ ] ProcessInstanceId exists
[ ] FencingToken exists
[ ] LocalStoreGeneration exists
[ ] durable lease schema exists
[ ] acquisition/renew/release are atomic
[ ] stale-leader fencing exists
[ ] followers can write domain+outbox
[ ] leader-only work is defined
[ ] crash takeover works
[ ] maintenance lease works
[ ] bootstrap generation switch is coordinated
[ ] process-mode capability is exposed
[ ] Loom tests are implemented
[ ] adapter compliance includes local coordination
[ ] diagnostics/metrics are available
```

---

## 90. Final Architecture

```text
                     SHARED LOCAL STORE
                            │
                 Coordinator Lease Row
                            │
               current fencing token = 43
                            │
           ┌────────────────┼────────────────┐
           ▼                ▼                ▼
      Process A         Process B        Process C
      Follower           Leader          Observer
                            │
             ┌──────────────┼──────────────┐
             ▼              ▼              ▼
          Sync Loop      Reconcile      Anti-Entropy
             │
             ▼
           Server

Followers:
    read local state
    commit domain + outbox
    signal leader

Leader dies:
    lease expires
       ↓
Follower acquires token 44
       ↓
new leader resumes

Old leader resumes:
    token 43 is stale
       ↓
fenced from coordinator-owned commits
```

The architectural principle is:

> **Aequora uses local lease election to choose who should sync, fencing tokens to decide who is allowed to commit coordinator-owned state, and server-side OperationId idempotency to make ambiguous network delivery safe across leadership changes.**

This gives Aequora robust multi-process, multi-window, background-worker, suspend/resume, and crash-takeover behavior without introducing heavyweight distributed consensus into a single-device problem.
