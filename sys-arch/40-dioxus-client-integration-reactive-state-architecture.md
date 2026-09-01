# Aequora Sync — Part 40

# Dioxus Client Integration, Reactive State, Local-First UX, and UI Boundary Architecture

## 1. Purpose

Part 40 defines the Dioxus integration layer:

```text
aequora-dioxus
```

The central rule is:

> **Dioxus reacts to durable local state and advisory runtime events; it never owns correctness-critical synchronization state.**

A Dioxus signal is not an outbox, cursor, authoritative version, conflict ledger, or durable draft unless that state is separately persisted.

---

## 2. Responsibility Boundary

`aequora-dioxus` should provide:

```text
client context/provider
reactive sync status
query hooks
mutation hooks
conflict hooks
scope/bootstrap hooks
connectivity/lifecycle view state
query invalidation
diagnostic presentation state
```

It should not implement:

```text
outbox persistence
cursor persistence
retry/idempotency
domain conflict resolution
storage transactions
journal semantics
authority decisions
```

Dependency direction:

```text
Dioxus UI
   |
   v
aequora-dioxus
   |
   v
aequora-client
   |
   v
local adapter
```

`aequora-client` must remain usable without Dioxus.

---

## 3. Suggested Crate Layout

```text
aequora-dioxus/
├── provider.rs
├── client_handle.rs
├── hooks/
│   ├── query.rs
│   ├── mutation.rs
│   ├── sync_status.rs
│   ├── events.rs
│   ├── conflicts.rs
│   ├── scopes.rs
│   ├── bootstrap.rs
│   └── lifecycle.rs
├── state/
│   ├── query.rs
│   ├── mutation.rs
│   ├── sync.rs
│   └── diagnostics.rs
├── platform.rs
└── errors.rs
```

---

## 4. One Client Runtime Per Local Store

Create the Aequora client once at the application/store boundary and provide a cheap cloneable handle to Dioxus.

Do not create a new sync runtime every time a component mounts.

```text
App
↓
AequoraProvider
↓
shared AequoraClient
↓
many views/hooks
```

---

## 5. Provider

Conceptually:

```rust
pub fn provide_aequora(client: AequoraClient) {
    // expose through Dioxus context
}
```

The provider contains only a handle. Durable state remains in the local database.

---

## 6. Three UI State Classes

### Durable-derived state

Read from the local store:

```text
students
invoices
conflicts
pending operations
bootstrap state
scope membership
```

### Ephemeral UI state

Lives in Dioxus:

```text
selected tab
dialog visibility
search text
hover/focus
temporary form input
```

### Advisory runtime state

Examples:

```text
Syncing
Offline
Backoff
Connecting
```

These can be recreated after restart.

Golden rule:

> If losing a signal would lose user data or synchronization correctness, that state belongs in durable storage.

---

## 7. Local-First Mutation Flow

```text
User action
↓
typed domain operation
↓
Aequora mutate()
↓
Tx A:
  domain mutation
  + outbox insert
↓
COMMIT
↓
UI rerenders from local DB
↓
SavedLocally
↓
background sync
↓
server accepts/rejects/conflicts
↓
Tx C reconciliation
↓
UI rerenders
```

---

## 8. `SavedLocally` Is Not `ServerConfirmed`

The UI must distinguish:

```text
SavedLocally
Pending
ServerConfirmed
Rejected
Conflict
```

A successful local transaction means:

```text
the user's intent is durably stored on this device
```

It does not mean the authority accepted it.

---

## 9. Mutation Receipt

Concept:

```rust
pub struct MutationReceipt {
    pub operation_id: OperationId,
    pub local_state: LocalMutationState,
}
```

The receipt gives the UI an OperationId for later authoritative status.

---

## 10. Render From Durable Local State

Prefer:

```text
commit local DB
↓
invalidate local query
↓
rerender
```

over maintaining a separate hand-written optimistic object graph in signals.

This keeps crash behavior and UI behavior aligned.

---

## 11. Query Architecture

Application-specific repositories remain the query layer.

Example:

```rust
let students = use_aequora_query(
    QueryKey::students(class_id),
    move || student_repo.list(class_id),
);
```

Aequora should not become a universal ORM.

---

## 12. Query Key

A stable query key supports targeted invalidation:

```text
Students(class_id)
Student(student_id)
Invoices(account_id)
Dashboard(tenant_id)
```

Tenant/store identity must be part of keying where needed.

---

## 13. Query State

Concept:

```rust
pub enum QueryState<T> {
    Loading,
    Ready(T),
    Error(QueryError),
}
```

If cached data exists, prefer rendering it rather than replacing the screen with a full network spinner.

---

## 14. Query Invalidation

After local or authoritative commit:

```text
commit
↓
emit advisory local-change event
↓
invalidate affected query keys
↓
rerun local query
```

Notification happens after commit, never before.

---

## 15. Event Loss Must Be Safe

If the UI misses an advisory event:

```text
reread durable state
```

must restore correctness.

Therefore UI events are an optimization, not persistence.

---

## 16. Client Event Stream

Possible high-level events:

```rust
pub enum ClientEvent {
    LocalMutationCommitted,
    AuthoritativeApplied,
    OperationStateChanged,
    ConflictCreated,
    ScopeChanged,
    BootstrapChanged,
}
```

Avoid leaking low-level journal/cursor internals.

---

## 17. Status vs Events

Use latest-value/watch semantics for:

```text
SyncStatus
Connectivity
BootstrapProgress
ActiveStore
```

Use bounded event/broadcast semantics for:

```text
query invalidation
conflict creation
operation state changes
```

A slow UI subscriber must never block the sync engine.

---

## 18. Stable Sync Status

Recommended public state:

```rust
pub enum SyncStatus {
    Dormant,
    Connecting,
    Syncing,
    Reconciling,
    Idle,
    Backoff,
    Offline,
    Paused,
    NeedsAttention,
}
```

Keep this smaller than the engine's internal state machine.

---

## 19. `use_sync_status`

Concept:

```rust
let status = use_sync_status();
```

It subscribes to a latest-state channel and rerenders only when coarse status changes.

---

## 20. Mutation Hook

Concept:

```rust
let save = use_aequora_mutation(move |input| {
    client.mutate(UpdateStudent::new(input))
});
```

Ephemeral hook state can be:

```text
Idle
SubmittingLocal
SavedLocally
Error
```

Authoritative status remains durable/queryable by OperationId.

---

## 21. Awaiting Authority

Some operations require immediate authoritative finality:

```text
payment finalization
seat reservation
final payroll close
legal approval
```

Expose explicit:

```rust
client.await_authoritative(operation_id).await
```

Do not make every local-first mutation wait for the network.

---

## 22. Component Cancellation

If a component unmounts after Tx A commits:

```text
the operation remains durable and continues syncing
```

Dropping a UI future must not erase already-committed user intent.

---

## 23. Conflict Integration

Dioxus can query:

```text
open conflicts
conflict summaries
affected entities
resolution options
```

The product decides presentation:

```text
inline warning
review queue
merge dialog
approval screen
```

---

## 24. Conflict Resolution

A conflict resolution must become a durable semantic action.

Do not directly edit an in-memory conflict flag and call it resolved.

Preferred:

```text
user chooses resolution
↓
typed ResolveConflict operation
↓
Tx A
↓
sync
↓
authority accepts
↓
Tx C closes/supersedes conflict
```

---

## 25. Offline UX

Offline is a normal operating state.

When offline:

```text
local reads continue
eligible local writes continue
outbox grows
sync status shows Offline
```

Do not globally disable the application.

---

## 26. Authority-Required Operations

Some actions are not valid offline.

UI should say:

```text
Requires connection
```

instead of pretending they completed locally.

---

## 27. Manual Sync

Expose:

```text
request_sync()
```

for ordinary UI "Sync now" behavior.

A stronger:

```text
sync_now()
```

can perform one bounded foreground attempt where needed.

Both still respect security and backpressure.

---

## 28. Pull-to-Refresh

Recommended:

```text
refresh local query immediately
+
request background sync
```

Cached data should remain visible while network refresh is pending.

---

## 29. Background Sync Ownership

Dioxus components do not own OS background execution.

Architecture:

```text
Android WorkManager
iOS BGTaskScheduler
desktop agent/runtime
        |
        v
Aequora runtime
        |
        v
sync_once / scheduler
```

Dioxus observes resulting state.

---

## 30. Lifecycle Integration

Platform code forwards:

```text
foreground
background
resume
suspend
network changes
power state
```

to the client runtime.

The scheduler decides what work is appropriate.

---

## 31. Android

Dioxus remains the shared UI.

A thin Android bridge handles:

```text
WorkManager
push hints
connectivity
battery
secure storage
file picker integration
```

Rust still owns:

```text
outbox
retry
cursor
scheduler policy
reconciliation
```

---

## 32. iOS

A thin Swift/platform bridge supplies:

```text
BGTask
APNs wake hints
network/power
Keychain hooks
file integration
```

without reimplementing synchronization semantics.

---

## 33. Desktop In-Process Mode

```text
Dioxus
↓
AequoraClient
↓
Stoolap
```

Simple for a single process.

---

## 34. Desktop Agent Mode

```text
Dioxus
↓
IPC facade
↓
aequora-agent
↓
AequoraClient
↓
Stoolap
```

Useful for:

```text
GUI + CLI
multiple processes
background sync
foreign-language shells
```

The product-facing UI API should remain as similar as practical in both modes.

---

## 35. Do Not Mirror the Entire DB Into Signals

Avoid:

```text
load all entities at startup
store all in global Dioxus state
```

This creates stale duplicate state and memory pressure.

Prefer:

```text
targeted local queries
pagination
virtualization
read models
```

---

## 36. Large Lists

For ERP-scale tables:

```text
paged DB query
↓
virtualized UI list/table
```

Do not rerender thousands of rows on every sync batch.

---

## 37. Event Coalescing

If a server batch modifies 1,000 rows in one list:

```text
emit one StudentsList invalidation
```

rather than 1,000 immediate rerenders.

---

## 38. Frame Budget

Sync throughput and UI refresh cadence should be decoupled.

The engine can reconcile a large batch while the UI applies bounded/coalesced invalidations.

---

## 39. Search

Search text is ephemeral UI state.

Search indexes are reconstructable local derived state.

Do not synchronize keystrokes unless domain semantics explicitly require collaborative search input.

---

## 40. Drafts

Two categories:

### Ephemeral draft

Dioxus signal is sufficient when losing unfinished input is acceptable.

### Durable draft

Persist locally when draft must survive:

```text
crash
reboot
offline period
```

A durable draft need not necessarily be synced until submitted.

---

## 41. Autosave

Autosave should use domain semantic compaction rules.

A profile text edit may use:

```text
ReplaceLatest
```

for unsent operations.

Accounting entries should normally use append-only semantics.

---

## 42. Error Model

The UI should distinguish:

```text
local storage failure
validation failure
authentication required
permission denied
temporary network failure
server overload
business rejection
conflict
upgrade required
rebootstrap required
```

Map stable Aequora error codes to localized user messages.

Avoid displaying raw DB/network errors.

---

## 43. Routine Retry

Transient sync failures usually retry automatically.

Do not interrupt the user for:

```text
brief network outage
temporary 503
ordinary backoff
```

---

## 44. Reauthentication

When auth expires:

```text
sync pauses
outbox remains intact
UI prompts sign-in
new credentials installed
same OperationIds retry
```

Never discard pending work because a token expired.

---

## 45. Tenant / Workspace Switching

Switching tenant means switching:

```text
active store
client context
query namespace
scope set
```

Do not let cached state from the previous tenant appear in the new context.

---

## 46. Multiple Accounts

Prefer separate local stores per account/workspace where practical.

This simplifies:

```text
isolation
purge
backup
scope
security
```

---

## 47. Scope Integration

Possible hook:

```rust
let scope = use_aequora_scope(scope_request);
```

State:

```text
Active
Expanding
Contracting
NeedsBootstrap
Error
```

---

## 48. Scope Expansion

For a new offline scope:

```text
request scope
↓
bootstrap/stage if needed
↓
activate
↓
query invalidation
```

The UI can display progress while preserving currently cached data.

---

## 49. Scope Contraction

`EvictFromScope` is not business deletion.

The UI removes data from the relevant view/cache without showing a deletion event unless the domain entity was actually deleted.

---

## 50. Bootstrap UI

Stable progress stages:

```text
Preparing
Downloading
Verifying
Installing
CatchingUp
Complete
Failed
```

After process restart, progress is reconstructed from durable bootstrap metadata.

---

## 51. No Main-Thread Heavy Work

Never run on the UI thread:

```text
large DB scans
Postcard decoding of large batches
compression
Merkle hashing
snapshot verification
```

Use appropriate bounded async/blocking/CPU workers.

---

## 52. Blocking Stoolap Calls

If the local engine API is blocking:

```text
bounded blocking executor
↓
return result to Dioxus
```

Do not block Tokio/Dioxus event loops.

---

## 53. Blob UX

Local blob states:

```text
NotDownloaded
Queued
Downloading
Available
Failed
```

Blob cache state is local and reconstructable unless pinned.

---

## 54. Durable Attachment Intent

Before enqueueing an offline attachment operation, ensure the file is durably under app control.

Do not rely on a temporary picker path/URI that may disappear.

---

## 55. Android File Inputs

Copy or persist permissions/content as required by platform semantics before recording durable upload intent.

---

## 56. iOS File Inputs

Normalize security-scoped/external references into a durable application-managed representation when offline upload must survive termination.

---

## 57. Desktop External Paths

External files can move or disappear.

For durable intent, either:

```text
copy into managed staging
or
capture content-addressed snapshot
```

before enqueue.

---

## 58. Sync Indicator

Useful high-level states:

```text
Up to date
3 pending
Syncing
Offline
Needs attention
```

Do not expose internal journal offsets to ordinary users.

---

## 59. Pending Count

Derive from durable outbox state.

Do not maintain only an in-memory counter.

---

## 60. Last Sync Time

Useful only as advisory information.

Freshness is better described by:

```text
pending operations
caught-up cursor state
has_more
offline state
```

---

## 61. Rejected Operations

Products may provide a:

```text
Needs attention
```

queue.

A rejected local row can be:

```text
restored
converted to editable draft
marked rejected
```

according to domain semantics.

---

## 62. Strong Aggregate UX

Operations needing server finality can display:

```text
Pending confirmation
```

until authority responds.

---

## 63. Financial UX

For accounting/financial domains:

```text
no generic silent undo
no generic LWW
append corrections/reversals
```

Dioxus should surface domain-specific reversal workflows.

---

## 64. Local Permission Hints

UI may hide/disable actions using cached permission hints.

Server still authorizes every operation.

If permission changes while offline, queued work can be rejected later and must be handled explicitly.

---

## 65. Security of UI State

Avoid retaining unnecessary sensitive data in global signals.

Be careful with:

```text
clipboard
debug overlays
screenshots
logs
```

Product-level privacy protections may be added for sensitive screens.

---

## 66. Push Notifications

Push carries:

```text
wake/change hint
```

not authoritative business state.

On wake/open:

```text
normal cursor exchange
```

fetches truth.

---

## 67. Presence / Typing

These are ephemeral and best effort.

They may use dedicated live channels and reactive state without journal persistence.

---

## 68. Startup Flow

Recommended:

```text
render app shell
↓
open local store
↓
migration/recovery
↓
create AequoraClient
↓
provide client context
↓
render cached local data
↓
resume scheduler
↓
background sync
```

Do not wait for network before showing existing local data.

---

## 69. Startup Recovery States

UI should have deliberate flows for:

```text
migration failed
store corruption
binding mismatch
secure key unavailable
store locked
rebootstrap required
```

Never silently reset local data if pending work may exist.

---

## 70. App Exit

Users should be able to close the app while operations are pending.

Durable outbox survives.

"Sync before exit" may be offered as convenience, never as correctness requirement.

---

## 71. Logout

Logout policy may:

```text
remove credentials only
lock local encrypted data
purge local tenant store
```

according to security requirements.

Pending outbox must not be silently destroyed.

---

## 72. Memory Pressure

UI query caches and view models are reconstructable.

On mobile memory pressure:

```text
drop UI cache
retain durable DB/outbox
```

---

## 73. Diagnostics UI

Advanced screen can expose:

```text
sync status
pending count
conflict count
last successful sync
active scopes
authority epoch
store generation
local storage usage
client build
protocol version
```

without showing sensitive payloads by default.

---

## 74. Testing

Required Dioxus integration scenarios:

```text
offline create
reconnect and confirm
server rejection
manual conflict
auth expiry
bootstrap
process kill after local save
storage full
tenant switch
large batch invalidation
desktop agent disconnect
```

---

## 75. Process-Kill Test

Scenario:

```text
Tx A commits
↓
kill app
↓
restart
```

Expected:

```text
local row remains
outbox remains
UI rehydrates
sync continues
```

---

## 76. Lost Event Test

Drop all advisory client events.

A query refresh/reopen must reconstruct the correct UI entirely from durable state.

---

## 77. Event Storm Test

Apply thousands of authoritative events.

Expected:

```text
bounded event channel
coalesced invalidation
responsive UI
```

---

## 78. Slow Subscriber Test

A blocked component must not block reconciliation or scheduler progress.

---

## 79. Tenant Isolation Test

Switch stores/tenant rapidly.

No previous tenant row or query cache may leak into the active context.

---

## 80. Public API Sketch

Concept:

```rust
pub fn provide_aequora(client: AequoraClient);

pub fn use_sync_status() -> SyncStatusSignal;

pub fn use_aequora_query<K, T, F>(
    key: K,
    query: F,
) -> QueryState<T>;

pub fn use_aequora_mutation<I, O, F>(
    mutation: F,
) -> MutationHandle<I, O>;

pub fn use_conflicts(
    filter: ConflictFilter,
) -> QueryState<Vec<ConflictSummary>>;
```

Exact signatures should match the Dioxus version used during implementation.

---

## 81. Product SDK Layer

A School ERP should preferably expose:

```rust
SchoolClient
```

with domain APIs such as:

```text
students()
attendance()
fees()
documents()
```

Dioxus components should not construct raw operation envelopes.

---

## 82. Typed Operations

Bad:

```rust
OperationEnvelope { kind: 42, payload: ... }
```

inside a component.

Good:

```rust
UpdateStudentAddress::new(...)
```

Strong domain types should be created before submission.

---

## 83. Validation Layers

```text
UI input validation
↓
local domain validation
↓
server authoritative validation
```

UI validation improves UX but never replaces server validation/security.

---

## 84. Dioxus Integration Invariants

### AEQ-INV-DIOXUS001

```text
No correctness-critical synchronization state exists only in Dioxus component memory.
```

### AEQ-INV-DIOXUS002

```text
The UI reports a local mutation as durable only after Tx A commits domain state and outbox intent.
```

### AEQ-INV-DIOXUS003

```text
Successful local persistence never implies authoritative server confirmation.
```

### AEQ-INV-DIOXUS004

```text
Loss of advisory UI events cannot lose synchronization correctness because durable state can always be reread.
```

### AEQ-INV-DIOXUS005

```text
Unmounting a component cannot cancel or erase a previously committed operation.
```

### AEQ-INV-DIOXUS006

```text
Query caches and reactive state are scoped to the active store/tenant identity.
```

### AEQ-INV-DIOXUS007

```text
Conflict resolution uses durable domain semantics rather than direct UI-state mutation.
```

### AEQ-INV-DIOXUS008

```text
Offline mode preserves all domain-permitted local reads and writes.
```

### AEQ-INV-DIOXUS009

```text
OS background scheduling belongs to the platform/client runtime, not mounted Dioxus components.
```

### AEQ-INV-DIOXUS010

```text
Large synchronization batches cannot cause unbounded UI event queues, rerenders, or memory growth.
```

---

## 85. Relationship to Earlier Parts

```text
Part 31 -> Android/iOS runtime
Part 32 -> desktop runtime/agent
Part 33 -> platform local storage
Part 35 -> public Rust SDK
Part 38 -> Stoolap local Tx A/Tx C
Part 39 -> Axum server boundary
Part 40 -> Dioxus UI/client boundary
```

Part 40 deliberately does not duplicate storage or sync-engine responsibilities.

---

## 86. End-to-End UI Flow

```text
Dioxus Component
      |
      v
Product Hook / View Model
      |
      v
Typed Domain Operation
      |
      v
AequoraClient
      |
      v
Tx A: domain + outbox
      |
      v
Local Query Invalidation
      |
      v
Dioxus rerender: Saved locally
      |
      v
Scheduler / Axum Exchange
      |
      v
Authority
      |
      v
Tx C: reconcile + cursor
      |
      v
Coalesced Invalidation
      |
      v
Dioxus rerender: Confirmed / Conflict / Rejected
```

---

## 87. Completion Criteria

```text
[ ] Dioxus provider/context defined
[ ] durable vs ephemeral state boundary defined
[ ] query hook architecture defined
[ ] mutation hook architecture defined
[ ] SavedLocally vs ServerConfirmed defined
[ ] status/event model defined
[ ] event-loss safety defined
[ ] conflict integration defined
[ ] scope/bootstrap integration defined
[ ] offline UX defined
[ ] background runtime ownership defined
[ ] Android/iOS/desktop integration defined
[ ] query invalidation/coalescing defined
[ ] tenant switching/isolation defined
[ ] performance and memory boundaries defined
[ ] error/recovery UX defined
[ ] tests defined
[ ] DIOXUS invariants defined
```

---

# 88. Final Architecture

```text
                         Dioxus UI
                 +-----------+-----------+
                 |           |           |
                 v           v           v
              Queries     Mutations    Status
                 |           |           |
                 +-----------+-----------+
                             |
                             v
                      aequora-dioxus
                             |
                             v
                       AequoraClient
                  +----------+----------+
                  |                     |
                  v                     v
             Local Store            Scheduler
                  |                     |
                  v                     v
             Stoolap Tx A/C        Axum Exchange
                  |                     |
                  +----------+----------+
                             |
                             v
                     Authoritative State
```

---

# 89. Final Recommendation

Use Dioxus as a reactive view over Aequora's durable local-first model, not as a second state database.

The intended mental model is:

```text
Dioxus asks:
"What is true locally right now?"

Aequora answers from durable state.

Dioxus asks:
"Perform this typed operation."

Aequora commits it locally,
queues it durably,
synchronizes it,
reconciles authority,
and then emits bounded hints that the UI should reread.
```

> **UI state may disappear at any moment; user intent and synchronization correctness must not.**

That boundary keeps the client robust across desktop, Android, iOS, offline periods, app restarts, process kills, background execution, and future UI-framework changes.
