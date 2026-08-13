# Aequora Sync — Part 08

# Live Sync, Push Hints, Presence, and Near-Real-Time Delivery Architecture

## 1. Purpose

Aequora's correctness model is based on:

```text
durable authoritative journal
+
scope-aware cursor
+
idempotent push/pull synchronization
```

That is intentionally robust, but plain polling alone can feel slow in collaborative or interactive applications.

Examples:

```text
teacher marks attendance and another device should see it quickly
finance staff posts a payment and dashboard should refresh
parent receives a new notice
two staff members edit shared data
```

Aequora therefore needs a near-real-time delivery layer.

The central rule is:

> **Push channels may accelerate synchronization, but they must never become the source of truth.**

WebSocket, SSE, mobile push, or local peer wakeups should act as:

```text
"hints that newer authoritative data probably exists"
```

The client still catches up through the normal journal/cursor path.

---

# 2. Goals

The live-sync subsystem should provide:

```text
low-latency wake-up
loss tolerance
reconnect safety
node-independent server fan-out
scope-aware notification
tenant isolation
bounded memory
backpressure
mobile compatibility
presence as optional ephemeral state
```

---

# 3. Non-Goals

The subsystem must not:

```text
carry correctness-critical authoritative state exclusively
replace cursor sync
require sticky sessions
assume persistent connectivity
guarantee delivery
guarantee exact ordering
```

---

# 4. Architecture Principle

Normal correctness:

```text
Client Cursor N
↓
POST /sync/exchange
↓
Server returns changes > N
```

Live acceleration:

```text
Server observes new event > N
↓
Push Hint
↓
Client wakes
↓
Normal sync exchange
```

---

# 5. Push Hint Model

A hint should be intentionally small.

Conceptually:

```rust
pub struct SyncHint {
    pub scope_id: ScopeId,
    pub latest_sequence_hint: Option<Sequence>,
    pub reason: SyncHintReason,
}
```

---

# 6. Hint Reasons

```rust
pub enum SyncHintReason {
    NewAuthoritativeChange,
    ScopeChanged,
    Revocation,
    MaintenanceChanged,
    UpgradeNotice,
}
```

Keep this list small and semantic.

---

# 7. No Payload Replication in Hint

Do not send full domain payloads in generic sync hints.

Why:

```text
larger fan-out cost
ordering ambiguity
duplicate data path
security complexity
harder retry behavior
```

The authoritative payload remains in journal/snapshot protocol.

---

# 8. latest_sequence_hint

This field is only advisory.

Client behavior:

```text
if latest_sequence_hint > local cursor
    sync soon
```

If omitted or stale:

```text
normal periodic pull still works
```

---

# 9. Sequence Hint Is Not ACK

Receiving a hint never advances cursor.

Only durable reconciliation does.

---

# 10. Transport Options

Aequora should support:

```text
WebSocket
Server-Sent Events
mobile push notification
local IPC hint
future QUIC notification channel
```

through one common abstraction.

---

# 11. LiveHintTransport Trait

Conceptually:

```rust
pub trait LiveHintTransport {
    async fn connect(
        &self,
        auth: &AuthContext,
        subscriptions: &[ScopeId],
    ) -> Result<LiveHintStream, LiveHintError>;
}
```

---

# 12. Core Independence

`aequora-client` should not depend directly on:

```text
WebSocket library
SSE library
Android push SDK
```

Use adapter crates.

---

# 13. Recommended Crates

Potential:

```text
aequora-live
aequora-live-websocket
aequora-live-sse
aequora-live-push
```

Core sync remains unchanged.

---

# 14. WebSocket Role

WebSocket is useful when:

```text
desktop/foreground app
long-lived connection
low-latency collaboration
```

---

# 15. SSE Role

SSE is useful when:

```text
server → client only
simple infrastructure
HTTP-friendly streaming
```

Because Aequora hints are mostly server-to-client, SSE can be sufficient.

---

# 16. Mobile Push Role

On Android/iOS background execution, persistent sockets may not be reliable.

Use platform push only to say:

```text
new data may be available
```

Then app/background worker performs normal sync when allowed.

---

# 17. Push Notification Privacy

Mobile push payload should contain minimal metadata.

Prefer:

```text
opaque wake-up token
scope/category hint
```

Avoid sensitive business data.

---

# 18. Persistent Connection Lifecycle

State machine:

```text
Disconnected
↓
Connecting
↓
Authenticating
↓
Subscribed
↓
ReceivingHints
↓
Disconnected
```

Reconnect automatically with backoff.

---

# 19. Reconnect Safety

After reconnect:

```text
do not ask "what hints did I miss?"
```

Instead:

```text
run normal cursor catch-up
```

This avoids needing durable hint history.

---

# 20. Hint Loss

If a hint is lost:

```text
periodic scheduler pull eventually discovers change
```

Therefore correctness survives.

---

# 21. Duplicate Hints

Multiple hints for same sequence/scope are harmless.

Scheduler coalesces them.

---

# 22. Reordered Hints

Hint ordering is irrelevant.

Client only needs:

```text
some reason to sync
```

---

# 23. Coalescing

If 100 server events occur quickly:

```text
send one wake-up hint
```

rather than 100 individual hints.

---

# 24. Hint Debounce

Server can debounce per:

```text
device
scope
connection
```

for a short interval.

This reduces fan-out load.

---

# 25. Client Debounce

Client also coalesces repeated hints into one scheduler wake-up.

---

# 26. Integration With Part 06 Scheduler

Push hint becomes:

```text
SchedulerTrigger::ServerHint
```

Scheduler decides when to perform sync.

Interactive foreground client may sync immediately.

Background mobile client may wait for platform allowance.

---

# 27. Scope-Aware Hints

Part 07 scopes are fundamental.

Hint should identify:

```text
which scope probably changed
```

so client can prioritize that scope.

---

# 28. Multi-Scope Client

Example:

```text
Core
Attendance
Finance
Documents
```

If only Attendance changes:

```text
wake Attendance first
```

Do not unnecessarily pull all modules.

---

# 29. Scope Version Change Hint

Server can notify:

```text
ScopeChanged
```

Client then performs:

```text
scope validation/delta flow
```

not blindly normal pull.

---

# 30. Revocation Hint

If scope access is revoked:

```text
push immediate revocation hint where possible
```

But server still enforces revocation on every reconnect/request.

Push only accelerates awareness.

---

# 31. Security Rule

Live connection authorization must be equivalent to normal sync authorization.

A connection cannot subscribe to arbitrary tenant/scope IDs.

---

# 32. Subscription Validation

On connect:

```text
authenticate
↓
resolve allowed scopes
↓
subscribe only authorized scopes
```

---

# 33. Dynamic Subscription Update

When local subscriptions change:

```text
client sends subscribe/unsubscribe hint-control message
```

or reconnects with new scope set.

---

# 34. Control Plane vs Data Plane

Live channel may carry only control messages such as:

```text
Subscribe
Unsubscribe
Hint
Ping
Pong
ServerNotice
```

Not canonical domain replication.

---

# 35. Connection Identity

Each live connection has:

```rust
pub struct LiveConnectionId(Uuid);
```

Ephemeral only.

Do not use as durable business identity.

---

# 36. Device Identity

Connection is associated with stable:

```text
DeviceId
```

from authenticated session.

---

# 37. One Device Multiple Connections

Possible due to:

```text
multiple processes
browser-like windows
reconnect overlap
```

Server may allow several connections.

Part 05 local coordinator should usually ensure only leader maintains live channel for a shared local store.

---

# 38. Leader-Only Live Connection

Recommended:

```text
only current local sync leader opens WebSocket/SSE
```

Followers do not.

On leadership handoff:

```text
old live connection closes
new leader connects
```

---

# 39. Server Node Independence

Do not require:

```text
client reconnect to same Axum node
```

Any node can accept live connection.

---

# 40. Fan-Out Architecture

New authoritative event occurs on Node A.

Clients may be connected to:

```text
Node B
Node C
```

Therefore live hints require cross-node fan-out.

---

# 41. Initial Small Deployment

If only one server node:

```text
in-memory broadcaster
```

is acceptable as optimization.

Correctness does not depend on it.

---

# 42. Multi-Node Deployment

Need a shared ephemeral fan-out mechanism.

Possible:

```text
PostgreSQL LISTEN/NOTIFY
Redis pub/sub
NATS
dedicated broker
database-backed notification table + poller
```

---

# 43. Recommended First Multi-Node Strategy

For Aequora's simple wake-up hints:

```text
PostgreSQL LISTEN/NOTIFY
```

can be sufficient initially.

Advantages:

```text
already depends on PostgreSQL
no new infrastructure
ephemeral semantics fit hints
```

Do not use it as durable journal replacement.

---

# 44. Why PostgreSQL NOTIFY Fits

Hints are allowed to be:

```text
lost
coalesced
duplicated
```

That matches ephemeral notification semantics.

---

# 45. Broker Abstraction

Define:

```rust
pub trait HintBroker {
    async fn publish(&self, hint: ServerHint) -> Result<(), BrokerError>;
    async fn subscribe(&self) -> Result<HintSubscription, BrokerError>;
}
```

---

# 46. Broker Failure

If broker unavailable:

```text
server sync still works
```

Clients fall back to polling.

This dependency is noncritical.

---

# 47. Event-to-Hint Flow

Authoritative transaction:

```text
domain mutation
+
journal
+
operation ledger
COMMIT
```

After commit:

```text
publish ephemeral hint
```

Never publish before commit.

---

# 48. Post-Commit Hook

Live hint belongs in:

```text
post-commit notification
```

If publishing fails:

```text
do not roll back business transaction
```

Normal pull still catches change.

---

# 49. Hint Generation Source

Best source:

```text
committed authoritative event
```

not HTTP request success.

---

# 50. Sequence Hint

After commit, event has:

```text
Sequence N
```

Hint may publish:

```text
scope S latest >= N
```

---

# 51. Scope Routing

Reuse Part 07 routing keys/membership logic.

Publish hint only to connections subscribed to affected scopes.

---

# 52. Avoid Per-Entity Fan-Out Topics

Do not create millions of broker topics by entity ID.

Prefer:

```text
tenant
scope
partition
```

coarse channels.

---

# 53. Fan-Out Key

Conceptually:

```rust
pub struct HintTopic {
    pub tenant_id: TenantId,
    pub scope_partition: ScopePartition,
}
```

---

# 54. Connection Subscription Map

Each server node keeps ephemeral map:

```text
topic -> connections
```

This can be rebuilt on reconnect.

---

# 55. Memory Bounds

Bound:

```text
connections
subscriptions per connection
queued hints per connection
```

Never let slow clients accumulate unbounded hint queues.

---

# 56. Backpressure

If client cannot consume hints fast enough:

```text
coalesce
drop older redundant hints
or disconnect
```

Because hints are non-durable.

---

# 57. Latest-Only Queue

For each scope:

```text
keep highest latest_sequence_hint
```

instead of queueing every hint.

This is ideal.

---

# 58. Slow Client

If connection backlog exceeds threshold:

```text
replace queue with one "sync required" hint
```

or disconnect.

---

# 59. Disconnect Semantics

Disconnect is not an error requiring user action.

Scheduler continues periodic sync.

---

# 60. Heartbeats

WebSocket/SSE layer may use heartbeat to detect dead connections.

Example:

```text
ping/pong
```

or SSE keepalive comments.

---

# 61. Heartbeat Is Transport-Level

Do not confuse with Part 05 local leader lease heartbeat.

---

# 62. Connection Reconnect Backoff

Use:

```text
exponential backoff + jitter
```

but cap delay.

If live connection cannot recover, polling still operates.

---

# 63. No Reconnect Storm

After server restart, thousands of clients reconnect.

Use:

```text
randomized reconnect
server Retry-After if supported
connection admission control
```

---

# 64. Live Connection QoS

Server may reject/limit live connections for:

```text
background inactive devices
too many connections
tenant quota
```

Client falls back to polling.

---

# 65. Presence Architecture

Presence is different from sync hints.

Presence asks:

```text
who is currently online/active?
```

It is inherently ephemeral.

---

# 66. Presence Is Not Authoritative Business State

Do not persist presence in authoritative journal as normal entity data.

Use ephemeral presence subsystem.

---

# 67. Presence States

Potential:

```rust
pub enum PresenceState {
    Online,
    Active,
    Idle,
    Away,
}
```

Keep semantics application-specific.

---

# 68. Presence Record

```rust
pub struct Presence {
    pub principal_id: PrincipalId,
    pub device_id: DeviceId,
    pub scope: Option<ScopeId>,
    pub state: PresenceState,
    pub expires_at: InstantLike,
}
```

---

# 69. TTL-Based Presence

Presence expires unless refreshed.

This avoids permanent "online" state after crash.

---

# 70. Presence Privacy

Presence can be sensitive.

Application authorization decides:

```text
who may see whom
```

Do not broadcast tenant-wide presence by default.

---

# 71. Presence Subscription

Client may subscribe to:

```text
team
class
conversation
document
```

presence sets.

Use bounded membership.

---

# 72. Presence Fan-Out

Can use same ephemeral broker infrastructure as sync hints, but separate message type and policy.

---

# 73. Presence Rate Limiting

Do not publish activity state on every keystroke.

Debounce and TTL refresh.

---

# 74. Active Editing Presence

For collaborative UI:

```text
"User X is viewing/editing Student Y"
```

may be useful.

This is advisory only.

It must not become a locking mechanism.

---

# 75. Advisory Locks

If product needs exclusive edit locks, design separately as explicit leases.

Do not infer lock from presence.

---

# 76. Near-Real-Time Conflict Reduction

Presence can reduce concurrent editing socially, but correctness still uses:

```text
versions
conflict policy
server validation
```

---

# 77. Live Server Notices

Live channel can also deliver non-domain notices:

```text
maintenance starting
upgrade recommended
scope changed
session expiring
```

Still typed and small.

---

# 78. Session Expiry Notice

Server may hint:

```text
token/session expiring
```

Client credential provider refreshes.

But auth remains enforced on actual requests.

---

# 79. Connection Authentication Refresh

Long-lived socket credentials can expire.

Options:

```text
re-auth control message
reconnect with new token
```

Simplest initial design:

```text
reconnect
```

---

# 80. Revoked Session

Server should close live connection promptly if session/device revoked.

Even if not, next normal sync request will fail authorization.

---

# 81. Device Revocation Fan-Out

Admin revokes device:

```text
publish control hint
close matching live connection
```

This accelerates enforcement.

---

# 82. Live Protocol Envelope

Conceptually:

```rust
pub enum LiveMessage {
    Hello(LiveHello),
    Subscribe(SubscribeRequest),
    Unsubscribe(UnsubscribeRequest),
    SyncHint(SyncHint),
    Presence(PresenceMessage),
    ServerNotice(ServerNotice),
    Ping,
    Pong,
}
```

---

# 83. Version Negotiation

Live channel should negotiate:

```text
live protocol version
capabilities
```

separately or reuse main protocol capability set.

---

# 84. Keep Live Protocol Additive

Unknown optional message types:

```text
ignore/reconnect according to capability policy
```

Do not silently misinterpret.

---

# 85. Serialization

Use:

```text
Postcard
```

for live binary protocol.

SSE may require transport framing such as base64 or a compact textual wrapper if raw binary is unavailable.

---

# 86. WebSocket Binary Frames

Preferred:

```text
Postcard binary frame
```

---

# 87. SSE Encoding

Possible:

```text
event: sync-hint
data: base64(postcard)
```

or a minimal JSON envelope if operational simplicity outweighs binary purity.

Core message stays transport-neutral.

---

# 88. Mobile Push Encoding

Mobile push payload should not reuse full live protocol.

Use provider-specific compact wake-up metadata.

Then client performs normal protocol sync.

---

# 89. Delivery Semantics

Document explicitly:

```text
at-most-best-effort hint delivery
```

No guarantee of:

```text
exactly once
ordered
durable
```

---

# 90. Live Hint Deduplication

Client can track:

```text
last hinted sequence per scope
```

ephemerally.

If new hint <= known hinted sequence:

```text
ignore
```

But this optimization is optional.

---

# 91. Persist Hinted Sequence?

Usually unnecessary.

On restart:

```text
normal cursor sync
```

is sufficient.

---

# 92. Polling Fallback

Even with live hints enabled, retain safety polling.

Example profiles:

```text
foreground with live:
    poll every few minutes as safety

background:
    platform-dependent

no live:
    adaptive polling
```

Exact values configurable.

---

# 93. Push Hint + Scheduler Race

If sync already running and hint arrives:

```text
mark another sync-needed flag
```

After current exchange:

```text
if cursor still below hinted/latest state
    run again
```

---

# 94. Coalesced SyncNeeded Flag

Use:

```text
Atomic/Watch channel boolean or generation counter
```

not unbounded trigger queue.

---

# 95. Hint Generation Counter

Internal client:

```rust
SyncWakeGeneration(u64)
```

can increment on wakeups.

Coordinator records generation before sync and checks after sync to know if new hints arrived during run.

---

# 96. Multi-Process Integration

Only Part 05 leader owns live socket.

Followers notify leader through local IPC when their subscription set changes.

---

# 97. Leadership Loss

On fencing loss:

```text
close live connection
```

or let it expire, but old process must not trigger coordinator work.

---

# 98. Leadership Acquisition

New leader:

```text
load active scopes
connect live transport
subscribe
run immediate cursor catch-up
```

Immediate catch-up handles missed hints during handoff.

---

# 99. Server Restart

All sockets drop.

Clients reconnect with jitter.

After reconnect:

```text
run cursor catch-up
```

No durable hint replay needed.

---

# 100. Broker Restart

Same behavior.

Live acceleration temporarily degrades to polling.

---

# 101. Multi-Node Broker Failure

If some nodes lose broker subscription:

```text
their connected clients may miss hints
```

Still safe.

Observe broker health as degraded feature, not core outage.

---

# 102. Liveness Health

Server readiness should not normally fail because live hint broker is unavailable.

Unless product explicitly requires live feature.

---

# 103. Metrics

Server:

```text
live_connections
live_connect_total
live_disconnect_total
live_hints_published_total
live_hints_coalesced_total
live_hint_drop_total
live_broker_error_total
```

Client:

```text
live_connected
live_reconnect_total
live_hint_received_total
live_hint_coalesced_total
live_fallback_poll_total
```

---

# 104. Presence Metrics

```text
presence_sessions
presence_updates_total
presence_expired_total
```

Avoid user IDs as labels.

---

# 105. Logs

Structured events:

```text
live_connected
live_disconnected
live_reconnect_backoff
hint_published
hint_queue_overflow
presence_expired
```

Do not log sensitive payloads.

---

# 106. Alerting

Alert only if live feature matters operationally.

Examples:

```text
broker unavailable for long period
mass reconnect loop
connection rejection spike
hint drop spike
```

Core sync may remain healthy.

---

# 107. Backpressure Invariant

> **Slow live consumers must never consume unbounded server memory.**

---

# 108. Hint Loss Invariant

> **Loss of all live hints must not prevent eventual convergence while periodic/triggered cursor synchronization remains available.**

---

# 109. Scope Isolation Invariant

> **A live connection must never receive a scope hint for a scope it is not currently authorized to subscribe to.**

---

# 110. No Cursor Mutation Invariant

> **Receiving a live hint never advances a cursor or marks authoritative state applied.**

---

# 111. Presence Expiry Invariant

> **Presence state expires automatically after loss of refresh/connection.**

---

# 112. Correctness Invariants

Add to Part 01:

## AEQ-INV-LIVE001

```text
Live hint delivery is not required for authoritative convergence.
```

## AEQ-INV-LIVE002

```text
Live hints cannot mutate authoritative or local replica state directly.
```

## AEQ-INV-LIVE003

```text
Unauthorized scope hints are never delivered intentionally.
```

## AEQ-INV-LIVE004

```text
Hint queue memory is bounded.
```

## AEQ-INV-LIVE005

```text
Leader change does not require durable replay of hints; immediate cursor catch-up restores correctness.
```

---

# 113. TestKit

Add fake live broker:

```rust
FakeHintBroker
```

Capabilities:

```text
drop hint
duplicate hint
reorder hint
delay hint
disconnect stream
```

---

# 114. Property Tests

Generate random:

```text
journal events
hint loss
hint duplication
connection failures
```

Assert final client state equals normal polling-only model.

---

# 115. Differential Test

Run same workload:

```text
A. polling only
B. perfect live hints
C. lossy live hints
```

Final authoritative replica state must be identical.

Only latency differs.

---

# 116. Reconnect Test

```text
disconnect before event
event commits
hint missed
reconnect
cursor catch-up
```

Expected:

```text
event applied
```

---

# 117. Slow Consumer Test

Generate 100,000 events while client stops reading hints.

Expected:

```text
bounded queue
coalesced latest hint or disconnect
no memory explosion
```

---

# 118. Revocation Test

```text
client subscribed
admin revokes scope/device
server closes/hints
client attempts sync
auth rejects
```

Correctness does not depend solely on connection closure.

---

# 119. Multi-Node Test

```text
event commits on node A
client connected node B
broker fan-out
hint received
```

Then repeat with broker drop:

```text
client catches through poll
```

---

# 120. Presence Test

```text
client publishes Online
connection dies
TTL expires
presence becomes offline/absent
```

---

# 121. Load Test

Measure:

```text
connections per node
hints/sec
broker throughput
memory per connection
scope subscriptions per connection
reconnect burst behavior
```

---

# 122. Connection Quotas

Configurable limits:

```text
max connections per tenant
max connections per device
max scopes per connection
max queued hints
```

---

# 123. Connection Admission

If server cannot accept live connection:

```text
reject live feature
```

but normal HTTPS sync remains usable.

---

# 124. Live Endpoint

Possible:

```text
GET /sync/v1/live
```

upgraded to WebSocket.

SSE:

```text
GET /sync/v1/events
```

Transport adapters can choose.

---

# 125. Unified Live Endpoint

If supporting multiple transports, core should expose one logical service.

Axum integration maps to specific route.

---

# 126. Presence Endpoint

Presence can travel on same WebSocket control channel.

Avoid separate infrastructure initially.

---

# 127. Broker Topic Design

Avoid one topic per device if broker cardinality becomes excessive.

Potential hierarchy:

```text
tenant
tenant + coarse partition
```

Node filters to subscribed connections locally.

---

# 128. Tenant-Wide Topic

For moderate deployments:

```text
one broker topic per tenant
```

may be enough.

Node receives tenant hint, matches local connection scopes.

---

# 129. Partitioned Topics

At larger scale:

```text
tenant + partition
```

reduces unnecessary fan-out.

Part 07 routing keys can drive this.

---

# 130. Avoid Global Broadcast

Do not publish every event to every server node/client.

At minimum isolate by tenant.

---

# 131. Broker Abstraction Levels

Start:

```text
InMemoryHintBroker
PostgresNotifyHintBroker
```

Later:

```text
NatsHintBroker
RedisHintBroker
```

No core protocol changes.

---

# 132. Failure Classification

Live failures:

```text
OptionalUnavailable
AuthFailed
ProtocolMismatch
RateLimited
PermanentConfigError
```

Most live failures should not transition overall sync to fatal error.

---

# 133. UI State

Do not expose:

```text
"WebSocket disconnected"
```

as primary sync failure if normal sync works.

Potential diagnostics:

```text
Live updates unavailable; periodic sync active
```

---

# 134. Presence UI

Presence should be labeled approximate.

Examples:

```text
Online
Active recently
```

Do not imply perfect real-time certainty.

---

# 135. Typing Indicators

Typing indicators are even more ephemeral than presence.

They can use same live channel but should not enter Aequora journal.

---

# 136. Collaboration Cursor

For shared document cursors/selections:

```text
ephemeral live channel
```

not authoritative sync.

---

# 137. Boundary Between Ephemeral and Durable

Use:

```text
Aequora journal
```

for data that must survive disconnect/restart.

Use:

```text
live ephemeral channel
```

for data whose loss is acceptable.

This boundary must be explicit.

---

# 138. Chat Messages

A chat message is durable domain data:

```text
journal/operation path
```

Typing indicator is ephemeral:

```text
live channel
```

Do not confuse them.

---

# 139. Notifications

"In-app notification record" may be durable.

"Wake device because notification exists" is ephemeral.

---

# 140. Server Push of Full Changes

A future optimization may send actual journal changes over WebSocket.

If ever added, they must still be:

```text
identified by authoritative sequence
reconcilable idempotently
cursor-validated
recoverable via pull
```

Initial recommendation:

```text
do not implement
```

Hints are enough.

---

# 141. Why Delay Full Push Replication

It duplicates:

```text
batching
reconciliation
retry
flow control
scope filtering
```

and adds substantial complexity.

---

# 142. Scheduler Interaction Example

```text
Event commits at seq 900
↓
hint(scope Attendance, latest=900)
↓
client scheduler sees local cursor 897
↓
Interactive wake
↓
POST exchange cursor=897
↓
events 898..900
↓
atomic reconcile
↓
cursor=900
```

---

# 143. Missed Hint Example

```text
Event commits at 900
hint lost
↓
safety poll after interval
↓
exchange cursor=897
↓
same correct result
```

---

# 144. Duplicate Hint Example

```text
hint 900
hint 900
hint 900
```

Client coalesces to one sync-needed signal.

---

# 145. Hint During Sync Example

```text
sync starts at cursor 900
new event 901 commits
hint arrives during request
response only through 900
```

Client wake generation changed:

```text
run another exchange
```

and fetch 901.

---

# 146. Completion Criteria

Part 08 is complete when:

```text
[ ] SyncHint defined
[ ] live transport abstraction defined
[ ] WebSocket/SSE roles defined
[ ] mobile push is hint-only
[ ] reconnect/catch-up behavior defined
[ ] hint coalescing defined
[ ] scope-aware subscriptions defined
[ ] server fan-out architecture defined
[ ] broker abstraction defined
[ ] PostgreSQL NOTIFY initial strategy defined
[ ] slow-client backpressure defined
[ ] leader-only live connection defined
[ ] presence is explicitly ephemeral
[ ] revocation interaction defined
[ ] scheduler integration defined
[ ] test/fault/load scenarios defined
```

---

# 147. Final Architecture

```text
                     AUTHORITATIVE COMMIT
                              │
                              ▼
                     Journal Sequence N
                              │
                              ├──────────────► Durable correctness path
                              │
                              ▼
                      Post-Commit Hint
                              │
                              ▼
                         Hint Broker
                 ┌────────────┼────────────┐
                 ▼            ▼            ▼
              Node A       Node B       Node C
                              │
                              ▼
                       Live Connection
                              │
                              ▼
                          SyncHint
                              │
                              ▼
                     Client Scheduler
                              │
                              ▼
                     Normal Sync Exchange
                              │
                              ▼
                     Cursor Reconciliation
```

Presence runs alongside this path:

```text
Client
  ↓ ephemeral TTL refresh
Live Presence Channel
  ↓
Authorized observers
```

The architectural principle is:

> **Live channels optimize latency; the journal preserves truth.**

Aequora should be just as correct if every WebSocket, SSE event, broker message, and mobile push notification is lost. Their value is making a correct system feel immediate rather than making an immediate system responsible for correctness.
