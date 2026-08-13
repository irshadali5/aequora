# Aequora Sync — Part 01

# Formal Correctness, Invariants, Model Checking, and Deterministic Simulation Architecture

## 1. Purpose

Aequora is a distributed state machine operating across:

```text
client storage
network
server runtime
authoritative storage
network
client reconciliation
```

Traditional unit and integration tests can demonstrate expected examples, but they cannot efficiently explore the enormous number of possible retries, crashes, duplicate deliveries, timeouts, transaction races, process restarts, and conflicting writes.

Aequora therefore needs a dedicated **correctness verification architecture**.

The objective is not to mathematically prove every line of implementation. It is to make the most important synchronization invariants:

```text
explicit
machine-checkable
continuously tested
reproducible
adapter-independent
```

---

## 2. Verification Layers

Use five complementary layers:

```text
L1 — Rust type-system invariants
L2 — deterministic unit/property tests
L3 — concurrent implementation tests
L4 — abstract distributed model checking
L5 — end-to-end fault simulation
```

No one layer replaces another.

---

## 3. Layer 1 — Type-System Invariants

Use type states to stop invalid transitions:

```rust
Incoming<O>
Authenticated<O>
Authorized<O>
Validated<O>
ConflictChecked<O>
Executable<O>
Committed<O>
```

Only `Executable<O>` may reach the executor.

Use newtypes for:

```text
OperationId
EntityVersion
Cursor
Sequence
ScopeId
DeviceId
AuthorityEpoch
```

Avoid using raw primitive values where semantic confusion would be dangerous.

---

## 4. Durable State Types

Transient and durable states should be represented explicitly.

Example:

```text
PendingOperation
InFlightOperation
CommittedOperation
RejectedOperation
ConflictOperation
```

Do not model durable state transitions as arbitrary strings.

---

## 5. Normative Invariant Registry

Maintain a stable registry of invariants.

Each invariant has:

```text
stable ID
human description
model assertion
property-test reference
adapter-test reference
production metric/diagnostic where applicable
```

Example:

```text
AEQ-INV-001
One OperationId produces at most one authoritative logical effect.
```

---

## 6. Minimum Core Invariants

### AEQ-INV-001 — Idempotent Authority

```text
same OperationId
→ at most one authoritative logical effect
```

### AEQ-INV-002 — Local Intent Atomicity

```text
committed local synchronizable mutation
↔ durable outbox intent
```

### AEQ-INV-003 — Authoritative Publication Atomicity

```text
accepted authoritative mutation
↔ journal event
↔ operation ledger result
```

### AEQ-INV-004 — Cursor Safety

```text
cursor N
⇒ every required event through N is durably applied locally
```

### AEQ-INV-005 — Version Monotonicity

```text
authoritative entity version never decreases
```

### AEQ-INV-006 — No Unauthorized Commit

```text
an unauthorized operation cannot become authoritative
```

### AEQ-INV-007 — Retry Preservation

```text
retry cannot create an additional logical effect
```

### AEQ-INV-008 — Reconciliation Idempotency

```text
reapplying the same authoritative event does not duplicate its logical effect
```

### AEQ-INV-009 — Tombstone Safety

```text
a stale update cannot silently resurrect a deleted entity
```

### AEQ-INV-010 — Timeline Safety

```text
a cursor from authority epoch A cannot be interpreted as valid in incompatible epoch B
```

---

## 7. Abstract State-Machine Model

Define a model independent of Axum, SQLx, Stoolap, PostgreSQL, and HTTP.

```rust
struct Model {
    server: ServerModel,
    clients: BTreeMap<ClientId, ClientModel>,
    network: NetworkModel,
}
```

The model represents semantic state rather than production implementation details.

---

## 8. Client Model

```rust
struct ClientModel {
    authoritative_state: CanonicalState,
    optimistic_state: CanonicalState,
    outbox: VecDeque<ModelOperation>,
    cursor: Cursor,
    applied_events: BTreeSet<EventId>,
    online: bool,
}
```

Additional modeled state can include:

```text
conflicts
bootstrap generation
pending response
local crash state
```

---

## 9. Server Model

```rust
struct ServerModel {
    state: CanonicalState,
    applied_operations: BTreeMap<OperationId, OperationOutcome>,
    journal: Vec<ModelEvent>,
    next_sequence: u64,
    authority_epoch: u64,
}
```

---

## 10. Network Model

The model network must intentionally behave badly.

It may:

```text
deliver
delay
duplicate
drop
reorder
disconnect
```

Do not assume FIFO ordering unless a protocol layer explicitly guarantees it.

---

## 11. Model Actions

State transitions include:

```text
LocalMutate(client)
BuildBatch(client)
SendRequest(client)
DeliverRequest
ServerExecute
ServerCommit
DropResponse
DeliverResponse
ClientReconcile
ClientCrash
ClientRestart
ServerCrash
ServerRestart
NetworkDisconnect
NetworkReconnect
```

Each action must define precise preconditions and resulting state.

---

## 12. Exhaustive Interleaving Exploration

The model checker should explore sequences such as:

```text
local mutation
send
server commit
response lost
client crash
client restart
retry
```

and:

```text
client sends operation
client retries
both requests arrive
server processes concurrently
```

Every reachable state is checked against safety invariants.

---

## 13. Rust-Native Model Checking

Prefer a Rust-native abstract state model and model-checking implementation where practical.

The architecture should support a Stateright-style exhaustive search over small state spaces.

The model is intentionally small:

```text
2 clients
1–2 entities
2–3 operations
bounded queue sizes
bounded sequence numbers
```

Small models are sufficient to uncover many protocol bugs.

---

## 14. Do Not Model the Entire Production Stack

Do not attempt to model-check:

```text
TCP
SQL parsing
allocator internals
TLS
Axum routing
```

Model the semantic protocol:

```text
operation accepted?
transaction committed?
response delivered?
cursor advanced?
```

---

## 15. State Canonicalization

Equivalent states should normalize to a stable representation before hashing.

Use this for:

```text
visited-state detection
symmetry reduction
failure deduplication
```

---

## 16. Symmetry Reduction

Two identical clients can often be treated symmetrically.

Normalize states so equivalent permutations do not multiply the search space unnecessarily.

---

## 17. Deterministic State Hash

Each model state must generate a deterministic hash.

This supports:

```text
visited set
failure identity
regression fixtures
```

---

## 18. Failure Trace Format

When an invariant fails, record:

```text
model version
Aequora version
invariant ID
initial state
action sequence
state after each action
final violation
```

Example:

```text
1. A.local_mutate(op1)
2. A.send(op1)
3. server.commit(op1)
4. network.drop(response)
5. A.retry(op1)
6. server.commit(op1)  <-- violation
```

---

## 19. Replayable Failure Fixtures

Store minimized traces in RON:

```ron
FailureTrace(
    model_version: 1,
    invariant: "AEQ-INV-001",
    actions: [
        ...
    ],
)
```

Any discovered failure becomes a permanent regression test.

---

## 20. Property-Based State-Machine Testing

Model checking explores bounded exhaustive spaces.

Property testing explores larger randomized spaces.

Generate actions such as:

```text
mutate
connect
disconnect
sync
crash
restart
duplicate
conflict
resync
```

Check invariants after every step.

---

## 21. Proptest Action Model

```rust
enum TestAction {
    Mutate(ClientId, ModelMutation),
    Connect(ClientId),
    Disconnect(ClientId),
    Sync(ClientId),
    CrashClient(ClientId),
    RestartClient(ClientId),
    CrashServer,
    RestartServer,
}
```

Proptest shrinking is valuable because it reduces a long failing scenario to the smallest reproducer.

---

## 22. Deterministic Random Seeds

Every randomized simulation must accept and report a seed.

A CI failure must be reproducible locally from that seed.

---

## 23. Loom Concurrency Tests

Use Loom for in-process concurrency primitives such as:

```text
local coordinator lease
shared sync status
in-memory operation reservation
bounded queues
shutdown races
worker fencing
```

Do not use Loom as a replacement for distributed protocol model checking.

---

## 24. Real Database Concurrency Tests

Every authoritative adapter must run concurrent transaction scenarios:

```text
same expected version
same OperationId
same unique business key
same worker lease
same snapshot generation
```

Expected semantic outcome:

```text
one succeeds
others conflict/retry/observe duplicate
```

Never silent lost update.

---

## 25. Fault Injection

Adapters need test-only failpoints.

Examples:

```text
before transaction begin
after local domain write
before outbox append
after outbox append
before journal append
after journal append
before operation ledger write
after operation ledger write
before commit
after commit
before cursor update
after authoritative apply
```

---

## 26. Failpoint Actions

A failpoint may simulate:

```text
returned error
panic/unwind
task cancellation
connection loss
process termination
```

Production builds should not expose dangerous arbitrary failpoint controls.

---

## 27. Crash Semantics Matrix

Distinguish:

```text
ordinary error
panic
async cancellation
client process death
server process death
DB connection death
server death after DB commit
```

Each has different recovery semantics and deserves explicit tests.

---

## 28. Cancellation Safety

A Tokio task can be cancelled at an `.await` boundary.

Verify:

```text
uncommitted transaction rolls back
committed operation remains authoritative
client ambiguity resolves by retrying same OperationId
```

---

## 29. Deterministic Reference Store

TestKit should contain a slow but precise reference store.

It needs:

```text
clear transaction semantics
complete introspection
deterministic ordering
failpoints
```

It becomes the semantic oracle for adapter comparison.

---

## 30. Differential Adapter Testing

Run the same scenario against:

```text
ReferenceStore
Stoolap
SQLite
PostgreSQL
other adapters
```

Compare canonical outcomes.

This proves database adapters preserve Aequora semantics.

---

## 31. Linearization Point

Aequora does not claim global instantaneous consistency across offline clients.

But the authoritative linearization point for an accepted operation is:

```text
authoritative DB commit
```

Tests should ensure results are compatible with that commit point.

---

## 32. Exactly-Once Terminology

Never claim exactly-once network delivery.

Correct terminology:

```text
at-least-once delivery/retry
+
exactly-once logical authoritative effect by OperationId
```

Tests, docs, and telemetry must use the same semantics.

---

## 33. Convergence Property

Assuming:

```text
no new writes
the network eventually works
the server remains available
retry eventually executes
no unresolved manual conflict remains
```

then all active clients should eventually converge to authoritative server state.

This is a liveness property.

---

## 34. Safety vs Liveness

Safety:

```text
nothing bad happens
```

Examples:

```text
no duplicate payment
no unauthorized commit
no premature cursor advancement
```

Liveness:

```text
something good eventually happens
```

Examples:

```text
pending operation eventually resolves
client eventually catches up
```

Keep these categories explicit.

---

## 35. Fairness Assumptions

Liveness requires assumptions.

Examples:

```text
network eventually becomes available
server eventually responds
retry scheduler eventually runs
```

Document them instead of claiming impossible guarantees for permanently disconnected systems.

---

## 36. Conflict Verification

Create reusable scenario suites for:

```text
same-field updates
different-field updates
delete vs update
append vs append
commutative operations
manual conflicts
```

Every built-in consistency/conflict profile should pass its own property suite.

---

## 37. Compaction Verification Hook

Future offline compaction must prove:

```text
original operation sequence
```

and:

```text
compacted operation sequence
```

are semantically equivalent under the compactor's documented preconditions.

This should be property-tested.

---

## 38. Snapshot Verification

Property:

```text
install snapshot at cursor N
+
apply events after N
```

must equal the canonical state produced by replaying authoritative history to the same point.

---

## 39. Resync Verification

Scenario:

```text
client has pending operations
cursor becomes invalid
client bootstraps
pending operations are preserved/rebased/retried
```

Invariant:

```text
pending user intent is not silently lost
```

---

## 40. Authority Epoch Verification

Future authority failover needs:

```text
epoch 5 primary
epoch 6 promoted primary
```

Invariant:

```text
epoch 5 cannot make new authoritative commits after epoch 6 fencing becomes valid
```

This protects against split brain.

---

## 41. Security-State Invariants

Model or property-test:

```text
tenant A cannot mutate tenant B
revoked device cannot create new accepted commits
unknown operation kind cannot execute
invalid schema cannot execute
```

---

## 42. Resource Invariants

Correctness also includes bounded resource behavior.

Examples:

```text
batch operations <= configured maximum
dependency nodes <= maximum
decoded bytes <= maximum
retry queue bounded by persisted outbox
```

---

## 43. Protocol Fuzzing

Fuzz:

```text
Postcard envelope
operation batches
capability negotiation
schema upcasters
dependency graph
snapshot manifests
```

Expected behavior:

```text
no panic
no UB
no unbounded allocation
typed rejection
```

---

## 44. Fuzz Corpus

Maintain fixtures for:

```text
current valid messages
old supported messages
truncated messages
unknown discriminants
maximum batch sizes
deep dependency graphs
snapshot manifests
```

---

## 45. Mutation Testing

For critical correctness modules, mutation testing can reveal weak test coverage.

Example:

If changing an equality check in OperationId deduplication does not break tests, the verification suite is insufficient.

Use this selectively on high-risk modules.

---

## 46. Invariant Coverage Matrix

Maintain a matrix:

```text
Invariant | Model | Property | Loom | Adapter | E2E
```

Critical invariants should be covered by multiple independent techniques.

---

## 47. CI Verification Tiers

### Pull Request CI

```text
unit tests
small proptest suite
small model search
adapter smoke tests
```

### Nightly

```text
larger model bounds
large randomized simulations
fuzzing
fault injection
real DB concurrency tests
```

### Release

```text
full adapter compliance
migration matrix
long-running deterministic simulation
chaos suite
performance/correctness regression gates
```

---

## 48. Model Versioning

The abstract model is a specification artifact.

Version it.

If protocol semantics intentionally change, the model version should change too.

---

## 49. Prevent Specification Drift

Every meaningful behavior change should answer:

```text
Does the abstract model change?
Does an invariant change?
Does protocol documentation change?
Does adapter compliance change?
```

This keeps architecture, implementation, and tests aligned.

---

## 50. Verification CLI

Future tooling:

```text
aequora verify model
aequora verify adapter
aequora verify protocol
aequora verify trace <file>
```

This helps core maintainers and third-party adapter authors.

---

## 51. Failure Artifact Format

A failure bundle should contain:

```text
Aequora version
model version
adapter versions
DB versions
seed
action trace
OperationIds
cursor state
authority epoch
invariant ID
```

No customer-sensitive payload is necessary.

---

## 52. Production Incident Replay

When a real incident happens, operators should be able to export a sanitized structural trace and replay an equivalent scenario in TestKit.

This bridges production forensics and correctness engineering.

---

## 53. Public Correctness Documentation

A universal synchronization library should publicly document:

```text
invariants
adapter requirements
failure assumptions
verification methodology
known limitations
```

This makes the term "production-ready" meaningful.

---

## 54. Recommended Crate Layout

```text
crates/
├── aequora-invariants/
├── aequora-model/
├── aequora-testkit/
└── aequora-adapter-sdk/
```

Possible responsibilities:

```text
aequora-invariants
    invariant IDs and normative descriptions

aequora-model
    abstract distributed model and model checker integration

aequora-testkit
    deterministic simulator, reference store, failpoints

aequora-adapter-sdk
    shared compliance suites
```

If crate count becomes excessive, `aequora-invariants` can remain a module within `aequora-testkit`, but the specification itself should remain explicit.

---

## 55. Completion Criteria

Part 01 is implemented when:

```text
[ ] invariant registry exists
[ ] abstract client/server/network model exists
[ ] idempotency model check exists
[ ] cursor-safety model check exists
[ ] lost-response model check exists
[ ] duplicate-request model check exists
[ ] two-client conflict model exists
[ ] property-based state machine exists
[ ] Loom covers local concurrency primitives
[ ] adapter failpoints exist in test builds
[ ] differential adapter tests exist
[ ] failures emit replayable RON traces
[ ] CI tiers execute verification automatically
```

---

## 56. Final Architecture

```text
                    AEQUORA CORRECTNESS SYSTEM

               ┌─────────────────────────┐
               │ Normative Invariants    │
               └────────────┬────────────┘
                            │
          ┌─────────────────┼──────────────────┐
          ▼                 ▼                  ▼
   Abstract Model      Property Tests      Type System
          │                 │                  │
          ▼                 ▼                  ▼
   Model Checker      Random Simulation     Compile-Time
          │                 │                  │
          └────────────┬────┴────────────┬────┘
                       ▼                 ▼
                  Reference Store     Loom Tests
                       │                 │
                       └────────┬────────┘
                                ▼
                         Adapter Suites
                                │
                                ▼
                         Real DB/Network
                                │
                                ▼
                      Production Confidence
```

The correctness system should be treated as part of Aequora itself, not merely as a test folder.

A synchronization platform is only as trustworthy as its ability to demonstrate that difficult failure interleavings have been considered, explored, reproduced, and continuously checked.
