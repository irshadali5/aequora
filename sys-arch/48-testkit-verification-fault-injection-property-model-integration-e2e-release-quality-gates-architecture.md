# Aequora Sync — Part 48

# Testkit, Verification Infrastructure, Fault Injection, Property Testing, Model Testing, Integration Testing, End-to-End Testing, and Release Quality Gates Architecture

## 1. Purpose

Aequora is a synchronization engine whose hardest failures occur at boundaries:

```text
before commit
after commit but before response
during reconciliation
during retry
during migration
during bootstrap
during authority failover
during process death
during storage pressure
during version skew
```

A conventional unit-test suite is therefore insufficient.

The central rule is:

> **Aequora must test invariants across crashes, retries, concurrency, version changes, adapters, and deployment boundaries—not merely test that functions return expected values on the happy path.**

Verification should make incorrect implementations difficult to merge and difficult to certify.

---

# 2. Verification Pyramid

Aequora should use multiple complementary layers:

```text
Type-System Enforcement
        ↓
Unit Tests
        ↓
Property Tests
        ↓
Deterministic Model Tests
        ↓
Adapter Conformance
        ↓
Integration Tests
        ↓
Fault-Injection Tests
        ↓
End-to-End Tests
        ↓
Load / Soak / Chaos
        ↓
Release Qualification
```

No single layer replaces the others.

---

# 3. Correctness Before Test Count

A project with 50,000 shallow tests can still be unsafe.

Prioritize tests around stable invariants such as:

```text
Tx A atomicity
Tx B atomicity
Tx C atomicity
OperationId idempotency
cursor monotonicity
authority continuity
tenant isolation
outbox durability
migration preservation
```

---

# 4. Test Architecture Goals

The test infrastructure should be:

```text
deterministic where possible
seeded
reproducible
adapter-independent
fault-aware
fast at lower layers
production-realistic at upper layers
CI-friendly
cross-platform
```

---

# 5. Workspace Structure

Recommended:

```text
crates/testing/
├── aequora-testkit/
├── aequora-model/
├── aequora-conformance/
├── aequora-fault/
├── aequora-fixtures/
├── aequora-sim/
└── aequora-loadgen/

tests/
├── integration/
├── end_to_end/
├── compatibility/
├── migration/
├── recovery/
└── security/

fuzz/
├── protocol/
├── postcard/
├── planner/
├── snapshot/
└── registry/
```

---

# 6. `aequora-testkit`

The general reusable test support crate provides:

```text
test identities
test tenants
test operations
deterministic clocks
deterministic IDs
temporary stores
assertion helpers
fake transports
reference authority
test clients
fixture builders
```

---

# 7. Testkit Dependency Direction

Production crates must never depend on `aequora-testkit`.

```text
production
   ↑
testkit
```

not:

```text
production -> testkit
```

---

# 8. Deterministic Test Context

Concept:

```rust
pub struct TestContext {
    pub seed: TestSeed,
    pub clock: DeterministicClock,
    pub ids: DeterministicIdSource,
    pub faults: FaultController,
}
```

Every randomized failure should print the seed required to reproduce it.

---

# 9. Test Seed

Use a stable wrapper:

```rust
pub struct TestSeed(pub u64);
```

A failing run should emit:

```text
seed
scenario
adapter
build
```

---

# 10. Deterministic Clock

Tests should not depend directly on real `SystemTime`.

Use:

```text
advance
freeze
jump
```

under explicit control.

---

# 11. Monotonic-Time Simulation

Retry/backoff/scheduler tests should use a simulated monotonic clock where practical.

This avoids multi-second sleeps in CI.

---

# 12. Deterministic IDs

Tests can generate predictable:

```text
OperationId
EntityId
DeviceId
TenantId
```

without weakening production UUIDv7 semantics.

---

# 13. Fixture Philosophy

Fixtures should express domain intent.

Good:

```rust
fixture.student().with_name("A").build()
```

rather than enormous manually constructed protocol envelopes.

---

# 14. Protocol Fixture Layer

Separate domain fixtures from wire fixtures.

This lets protocol tests intentionally construct malformed messages without contaminating normal application fixtures.

---

# 15. Golden Fixtures

Use committed binary/text fixtures for durable formats:

```text
Postcard protocol messages
RON registry/config
snapshot manifests
incident bundles
migration metadata
```

---

# 16. Golden Fixture Rule

A golden fixture changes only when the format change is deliberate and reviewed.

Never automatically regenerate all fixtures merely to make CI pass.

---

# 17. Type-Level Tests

Use compile-fail tests where Rust's type system is part of correctness.

Examples:

```text
cannot execute Incoming<Operation>
cannot use unauthenticated request as Authorized
cannot pass raw TenantId where TenantContext is required
```

---

# 18. `trybuild`-Style Testing

Compile-time API tests are valuable for:

```text
typestate
sealed traits
public SDK misuse
derive macros
adapter trait contracts
```

---

# 19. Unit Tests

Unit tests target deterministic local behavior:

```text
ID parsing
version comparisons
error mapping
conflict policy
registry lookup
cursor validation
```

---

# 20. Property-Based Testing

Property tests generate many valid and invalid cases.

Useful domains:

```text
operation envelopes
dependency DAGs
cursor sequences
compaction sequences
schema versions
scope transitions
```

---

# 21. `proptest`

A Rust property-testing framework such as `proptest` is a strong fit.

Strategies should live in dedicated testing modules rather than production APIs.

---

# 22. Property: Idempotency

For operation `O`:

```text
apply(O)
apply(O)
apply(O)
```

must produce one logical authoritative effect.

---

# 23. Property: Cursor Monotonicity

For accepted reconciliation sequence:

```text
cursor_next >= cursor_previous
```

and cursor never advances beyond durably applied events.

---

# 24. Property: Retry Equivalence

For retryable transport failures:

```text
single successful attempt
```

and:

```text
N failed/ambiguous attempts + eventual success
```

must converge to equivalent authoritative state.

---

# 25. Property: Compaction Equivalence

For operations where compaction policy allows it:

```text
execute(original unsent sequence)
```

must be semantically equivalent to:

```text
execute(compacted sequence)
```

within the declared domain equivalence.

---

# 26. Property: Serialization Round Trip

For canonical message `M`:

```text
decode(encode(M)) == M
```

for all supported schema versions.

---

# 27. Property: Upcaster Determinism

```text
upcast(old_payload)
```

must always produce the same canonical operation.

---

# 28. Property: Scope Isolation

Generated operations from tenant/scope A must never appear in B without explicit authorized sharing semantics.

---

# 29. Shrinking

Property-test failures should shrink to minimal counterexamples.

This is especially useful for:

```text
dependency graphs
operation sequences
scope transitions
```

---

# 30. Model-Based Testing

Part 01 defined the formal abstract model.

Part 48 turns it into executable verification infrastructure.

---

# 31. Abstract State

A minimal model can represent:

```rust
struct Model {
    server: ModelServer,
    clients: Vec<ModelClient>,
    network: ModelNetwork,
}
```

---

# 32. Model Server

Tracks abstract:

```text
entities
versions
operation ledger
journal
authority epoch
```

---

# 33. Model Client

Tracks:

```text
local state
outbox
cursor
conflicts
connectivity
```

---

# 34. Model Network

Can:

```text
deliver
drop
duplicate
delay
reorder
partition
```

messages.

---

# 35. Model Actions

Examples:

```text
LocalMutate
StartSync
DeliverRequest
CommitServer
DropResponse
CrashClient
RestartClient
DeliverResponse
Reconcile
Compact
Bootstrap
PromoteAuthority
```

---

# 36. Model Invariants

After every transition check:

```text
no duplicate logical effect
cursor safety
version monotonicity
tenant isolation
authority uniqueness
outbox preservation
```

---

# 37. Bounded Model Checking

Explore small state spaces exhaustively.

For example:

```text
2 clients
2 entities
3 operations
drop/duplicate/reorder
```

can reveal bugs that huge random tests miss.

---

# 38. State Explosion

Keep abstract models deliberately small.

Model semantics, not production implementation details.

---

# 39. Model Trace Artifact

On failure emit:

```text
seed
initial state
action sequence
final state
violated invariant
```

as RON.

---

# 40. Replay Command

CLI:

```text
aequora verify replay model-failure.ron
```

should reproduce the transition sequence.

---

# 41. Differential Testing

Run the same semantic scenario against:

```text
ReferenceStore
SQLite
Stoolap
PostgreSQL where applicable
```

and compare canonical outcomes.

---

# 42. Reference Store

Maintain a simple, obviously-correct in-memory reference implementation.

It is not optimized.

Its purpose is:

```text
semantic oracle
```

for adapter and planner tests.

---

# 43. Differential Adapter Test

Example:

```text
generate operation sequence
↓
run ReferenceStore
↓
run SQLite
↓
run Stoolap
↓
compare canonical visible state/outbox/cursor
```

---

# 44. Fault Injection

Aequora needs explicit failpoints at correctness boundaries.

---

# 45. Fault Point Taxonomy

```text
BeforeTransaction
AfterTransactionBegin
AfterDomainMutation
AfterOutboxWrite
BeforeCommit
AfterCommit
BeforeResponse
AfterResponseDecode
BeforeCursorWrite
AfterCursorWrite
DuringMigration
DuringSnapshotInstall
```

---

# 46. Server Tx B Failpoints

Critical:

```text
before ledger lookup
after ledger lookup
after domain mutation
after journal append
after ledger outcome
before commit
after commit before response
```

---

# 47. Client Tx A Failpoints

```text
after provisional mutation
before outbox insert
after outbox insert
before commit
after commit before UI receipt
```

Expected:

```text
either all durable or none durable
```

---

# 48. Client Tx C Failpoints

```text
after authoritative event apply
after operation outcome
before cursor update
after cursor update
before commit
```

Cursor must never become durable ahead of event application.

---

# 49. Failpoint Interface

Concept:

```rust
pub trait FaultInjector {
    fn hit(&self, point: FaultPoint) -> FaultAction;
}
```

---

# 50. Fault Actions

Possible test-only actions:

```text
Continue
ReturnError
Panic
AbortProcess
Delay
DropMessage
CorruptTestArtifact
```

---

# 51. Production Isolation

Failpoint code must be:

```text
test-only
or
explicit non-production feature
```

and impossible to activate accidentally in normal production builds.

---

# 52. Process-Kill Testing

Some crash behavior cannot be validated with an ordinary returned error.

Spawn child process:

```text
perform operation
hit failpoint
hard exit/kill
restart
inspect durable state
```

---

# 53. Real Crash Testing

This is particularly important for:

```text
SQLite
Stoolap
migrations
snapshot activation
```

---

# 54. Power-Loss Approximation

True hardware power-loss testing is specialized.

Aequora can approximate through:

```text
hard process kill
filesystem fault harness
VM crash
```

and use database durability guarantees plus conformance evidence.

---

# 55. Transport Fault Injection

Fake transport supports:

```text
drop request
drop response
duplicate response
delay
reorder
truncate
disconnect
timeout
```

---

# 56. Ambiguous Commit Test

Critical scenario:

```text
server commits Tx B
↓
response lost
↓
client retries same OperationId
```

Expected:

```text
ledger returns prior authoritative outcome
no duplicate effect
```

---

# 57. Duplicate Request Storm

Send same operation concurrently from many requests.

Expected:

```text
one logical effect
consistent outcome
```

---

# 58. Database Faults

Inject:

```text
connection loss
timeout
deadlock
serialization failure
read-only
disk full
corruption where safely simulated
```

---

# 59. Disk-Full Tests

Client:

```text
Tx A fails atomically
outbox not partially written
existing unsynced intent preserved
```

Server:

```text
no partial authoritative commit
```

---

# 60. Storage Corruption Tests

Corruption should produce:

```text
RecoveryRequired
diagnostic evidence
no silent destructive reset
```

---

# 61. Migration Crash Matrix

For each migration test:

```text
before migration
during each critical step
after migration metadata write
after schema change
before completion marker
```

Restart must result in safe resumable/recoverable state.

---

# 62. Snapshot Fault Matrix

Inject failure during:

```text
download
hash verify
decompression
staging write
activation
journal catch-up
```

---

# 63. Bootstrap with Pending Intent

Must test:

```text
pending local operations
+
rebootstrap
+
authoritative snapshot
+
rebase
```

without losing user intent.

---

# 64. Concurrency Testing

Test:

```text
concurrent clients
concurrent operations
same entity
same aggregate
same OperationId
different tenants
```

---

# 65. Loom-Style Testing

Use a concurrency model checker such as Loom only for in-process synchronization primitives where it fits.

Do not use it as a substitute for database/distributed model testing.

---

# 66. Local Multi-Process Testing

Desktop test:

```text
Process A coordinator
Process B starts
A dies
B takes lease
stale A returns
```

Stale fencing token must be rejected.

---

# 67. SQLite Contention Test

Multiple readers + competing writers.

Verify bounded `BUSY/LOCKED` handling and one logical Aequora writer coordinator.

---

# 68. Stoolap Concurrency Test

Run equivalent semantic scenarios against Stoolap according to its certified capability profile.

---

# 69. PostgreSQL Concurrency Test

Use real PostgreSQL for:

```text
unique OperationId races
version CAS
timeline head locking
deadlocks
serialization retry
tenant isolation
```

---

# 70. No Database Mock for Critical Tx B

Mocks are useful for unit tests.

They cannot prove PostgreSQL transaction semantics.

Critical authority tests require a real PostgreSQL instance.

---

# 71. Neon Integration Tests

Where practical, maintain an operational test profile for Neon-specific deployment behavior.

Core PostgreSQL semantics remain tested locally against PostgreSQL.

---

# 72. Integration Test Definition

An integration test combines multiple real Aequora crates/components but may still replace external infrastructure.

Example:

```text
client core
+
SQLite
+
fake transport
+
server core
+
ReferenceAuthority
```

---

# 73. End-to-End Test Definition

E2E uses the actual production path:

```text
Dioxus/product SDK or test client
↓
Aequora client
↓
SQLite/Stoolap
↓
HTTP/Postcard
↓
Axum
↓
server core
↓
PostgreSQL
```

---

# 74. E2E Harness

Recommended:

```text
start PostgreSQL
run migrations
start aequora-server
create tenant/user/device
start client
execute scenario
assert local + server state
```

---

# 75. E2E Isolation

Each test gets:

```text
unique tenant
or
isolated database/schema
```

to support parallel CI.

---

# 76. E2E Scenario: Offline Create

```text
client offline
create entity
verify Tx A
bring online
sync
verify server
verify Tx C
```

---

# 77. E2E Scenario: Lost Response

```text
create
server commits
proxy drops response
retry
verify one effect
```

---

# 78. E2E Scenario: Conflict

```text
two clients start same base
both edit
first commits
second syncs
verify declared conflict policy
```

---

# 79. E2E Scenario: Delete/Tombstone

Verify:

```text
delete
offline stale client
reconnect
no resurrection
```

---

# 80. E2E Scenario: Scope Expansion

```text
client gains scope
seed newly visible state
catch up
cursor valid
```

---

# 81. E2E Scenario: Scope Contraction

Verify removed-from-scope data follows eviction/governance policy and is not mistaken for authoritative deletion.

---

# 82. E2E Scenario: Bootstrap

```text
large dataset
snapshot boundary N
install
journal N+1
converge
```

---

# 83. E2E Scenario: Authority Epoch Change

Client with old epoch must:

```text
detect mismatch
stop unsafe continuation
follow recovery/rebootstrap policy
```

---

# 84. E2E Scenario: Auth Expiry

```text
Tx A succeeds locally
credential expires
sync pauses
reauthenticate
retry same OperationId
```

---

# 85. E2E Scenario: Server Upgrade

Run:

```text
old client ↔ new server
new client ↔ old-compatible server
mixed server fleet
```

according to compatibility matrix.

---

# 86. E2E Scenario: Client Upgrade

Upgrade local store with pending outbox.

Pending semantic intent must survive.

---

# 87. E2E Scenario: Backup Restore

Restore server from backup/PITR and verify authority continuity/epoch behavior.

---

# 88. E2E Scenario: Air-Gapped

Run without public network dependency.

Verify:

```text
auth
sync
snapshot
telemetry local
release verification
```

---

# 89. Security Testing

Security tests should cover:

```text
cross-tenant access
auth bypass
mass assignment
malformed payload
oversized payload
compression bomb
dependency graph abuse
SSRF defenses
webhook replay
admin authorization
```

---

# 90. Fuzzing

Fuzz parsers and bounded complex inputs.

Targets:

```text
Postcard protocol decoder
envelope parser
snapshot manifest
registry parser
dependency planner
canonical value decoder
incident bundle parser
```

---

# 91. Fuzz Safety Properties

Fuzzer must verify:

```text
no panic
no OOM from bounded input
no infinite loop
no unsafe memory behavior
structured error
```

---

# 92. Decoder Bounds

Fuzz with configured maximum:

```text
message size
collection count
string length
dependency count
nesting
```

---

# 93. Differential Codec Testing

Where schemas support multiple external codecs:

```text
Postcard canonical
RON dev
JSON interoperability
```

must map to equivalent canonical values.

---

# 94. Snapshot Fuzzing

Malformed chunks/manifests must never activate corrupted state.

---

# 95. Registry Fuzzing

Malformed or adversarial registry input must fail closed.

---

# 96. API Compatibility Tests

Compile representative downstream code against public SDK.

This detects accidental API breakage.

---

# 97. SemVer Checks

Use API surface tooling plus explicit compatibility tests.

Rust SemVer compatibility alone does not prove protocol/store compatibility.

---

# 98. Protocol Compatibility Matrix

Test supported combinations:

```text
client protocol
server protocol
operation schema
capabilities
```

---

# 99. Compatibility Fixture Corpus

Keep fixtures from prior supported releases.

New release must decode/upcast them according to policy.

---

# 100. Retry-Only Operations

Deprecated RetryOnly operations must still accept legitimate retries while rejecting new creation when policy requires.

---

# 101. Store Migration Matrix

For each supported local upgrade path:

```text
old store format
+
pending outbox
+
conflicts
+
cursor
↓
new release
```

verify preservation.

---

# 102. PostgreSQL Migration Matrix

Test:

```text
previous supported DB schema
↓
expand migration
↓
mixed server versions
↓
new version
↓
contract when safe
```

---

# 103. Downgrade Tests

If downgrade is unsupported, verify it fails explicitly without corrupting data.

---

# 104. Release Upgrade Tests

Test at least:

```text
N-1 -> N
```

and any longer compatibility window promised by policy.

---

# 105. Cross-Platform Matrix

Client tests:

```text
Linux
Windows
macOS
Android
iOS
```

according to supported release targets.

---

# 106. SQLite Platform Matrix

At minimum distinguish:

```text
desktop
Android
iOS
```

because filesystem/process lifecycle differs.

---

# 107. Stoolap Platform Matrix

Only claim a platform certified after the required suite passes there.

---

# 108. Architecture Tests

Automate dependency-boundary checks:

```text
protocol does not depend on Axum
core does not depend on SQLx
domain does not depend on Dioxus
public facade does not leak adapter internals
```

---

# 109. Cargo Metadata Graph Test

CI can inspect Cargo metadata and reject forbidden dependency edges.

---

# 110. Feature Matrix Testing

Test meaningful Cargo feature combinations.

Avoid combinatorial explosion by defining supported profiles.

---

# 111. Minimal Feature Build

Verify core crates build with minimal features.

---

# 112. No-Default-Features Tests

Useful where crate contract promises them.

---

# 113. Unsafe Policy Test

CI can scan/deny unexpected `unsafe`.

Approved unsafe blocks require documented justification.

---

# 114. Panic Policy Test

Production hot paths should not use accidental:

```text
unwrap
expect
panic
```

for recoverable external input/errors.

---

# 115. Clippy

Use strict Clippy policy with reviewed exceptions.

---

# 116. Formatting

`cargo fmt --check` is a basic gate.

---

# 117. Documentation Tests

Public examples should compile.

---

# 118. License Tests

Part 49 will govern dependencies, but release CI should run license policy checks.

---

# 119. Vulnerability Tests

Run dependency/security advisory scanning as part of CI/release gates.

---

# 120. SBOM Verification

Release qualification verifies SBOM generation and correspondence to shipped artifacts.

---

# 121. Reproducible Build Check

Where supported, rebuild artifacts and compare according to reproducibility policy.

---

# 122. Artifact Signature Verification

Test release signature verification before publishing.

---

# 123. Test Categories

Tag tests:

```text
unit
property
model
integration
e2e
fault
migration
security
performance
soak
platform
```

---

# 124. Test Runtime Classes

```text
Fast      < seconds
Medium    < minutes
Long      scheduled
Soak      hours+
```

Avoid hard dependence on exact durations; categories are operational.

---

# 125. CI Tier 0 — Local Developer

Before commit:

```text
fmt
targeted unit tests
targeted compile
```

---

# 126. CI Tier 1 — Pull Request

Required:

```text
fmt
clippy
unit
property smoke
compile-fail
architecture dependency checks
protocol fixtures
migration smoke
security static checks
```

---

# 127. CI Tier 2 — Merge/Main

Add:

```text
full property suite
model exploration
SQLite
Stoolap where supported
real PostgreSQL integration
HTTP E2E
fault smoke
```

---

# 128. CI Tier 3 — Nightly

Add:

```text
large model seeds
fuzz time budget
cross-version compatibility
full migration matrix
process-kill tests
cross-platform tests
moderate load
```

---

# 129. CI Tier 4 — Weekly

Add:

```text
soak
large bootstrap
reconnect storm
storage pressure
fault campaigns
multi-process
HA/failover
```

---

# 130. CI Tier 5 — Release Candidate

Required production qualification:

```text
all supported platforms
all official adapters
compatibility matrix
migration matrix
security suite
fault suite
capacity suite
backup/restore
authority failover
artifact verification
conformance
```

---

# 131. Release Gate Philosophy

A release is not approved because:

```text
cargo test passed
```

It is approved because all required quality gates for its declared support profile passed.

---

# 132. Quality Gate Manifest

Concept:

```rust
pub struct QualityGateManifest {
    pub release: ReleaseVersion,
    pub profile: ReleaseProfile,
    pub required_suites: Vec<SuiteId>,
    pub evidence: Vec<EvidenceRef>,
}
```

---

# 133. Gate Results

```text
Pass
Fail
Waived
NotApplicable
```

---

# 134. Waiver Governance

A failed required test cannot be silently ignored.

A waiver requires:

```text
reason
owner
risk
expiry
affected profile
```

---

# 135. No Permanent Waivers

Waivers expire.

Persistent exceptions should become explicit product/support policy.

---

# 136. Release Profiles

Example:

```text
CoreLibrary
Desktop
Mobile
ServerStandard
Enterprise
AirGapped
```

Each has different required suites.

---

# 137. Adapter Release Gate

An adapter can ship as:

```text
Experimental
CommunityVerified
MaintainerVerified
Official
```

according to Part 30 evidence.

---

# 138. SQLite Official Gate

Requires:

```text
Tx A
Tx C
crash reopen
migration
large outbox
contention
disk full
backup/restore
desktop/mobile profile tests
```

---

# 139. Stoolap Official Gate

Must pass the same semantic requirements appropriate to its claimed profile.

No preferential exemption because it is pure Rust.

---

# 140. PostgreSQL Official Gate

Requires:

```text
Tx B
ledger race
journal order
version CAS
timeline contention
deadlock retry
migration
backup/PITR
failover/epoch
```

---

# 141. Axum Integration Gate

Test:

```text
body limits
auth
timeouts
disconnect
overload
graceful shutdown
compatibility
```

---

# 142. Dioxus Integration Gate

Test:

```text
offline mutation
reactive invalidation
process restart
conflict
bootstrap
auth expiry
lost advisory event
```

---

# 143. Mobile Gate

Test:

```text
background kill
network transition
low storage
upgrade
secure store
backup/restore behavior
```

---

# 144. Desktop Gate

Test:

```text
multi-window
agent IPC
lease takeover
sleep/resume
upgrade
shared store
```

---

# 145. Authority Gate

Any code touching:

```text
OperationId ledger
timeline
epoch
Tx B
journal
version CAS
```

requires enhanced review and targeted verification.

---

# 146. Finance Gate

Financial consistency profiles require additional:

```text
append-only
balanced accounting
reversal
idempotency
audit
deterministic replay
```

tests.

---

# 147. Security Gate

Any change to:

```text
auth
authz
crypto
tenant isolation
admin
serialization bounds
```

requires security tests/review.

---

# 148. Migration Gate

Every migration includes:

```text
forward test
crash test
data-preservation test
compatibility test
```

---

# 149. Protocol Gate

Protocol changes require:

```text
golden fixture
compatibility classification
upcaster test
fuzz corpus
negotiation test
```

---

# 150. Registry Gate

Registry changes require Part 29 validation:

```text
no ID reuse
owner
status transition
schema compatibility
migration where required
```

---

# 151. Performance Gate

Part 47 benchmark gates run on designated critical paths.

Correctness gates always take precedence.

---

# 152. Observability Gate

Part 46 tests:

```text
secret redaction
bounded cardinality
exporter failure
trace propagation
```

---

# 153. Governance Gate

Retention/erasure/legal-hold behavior must have integration tests for products claiming governance support.

---

# 154. Backup Gate

A backup is not considered verified merely because it was created.

Test restore.

---

# 155. Disaster Recovery Gate

Enterprise profile should periodically verify:

```text
restore
promotion
new epoch if required
client recovery
```

---

# 156. Production Smoke Tests

After deployment:

```text
health
readiness
auth
safe read
test tenant sync
```

without destructive behavior.

---

# 157. Canary Verification

Compare:

```text
errors
latency
DB retries
conflicts
resource usage
```

against baseline before wider rollout.

---

# 158. Rollback Test

Release process should periodically verify rollback strategy.

Remember DB contract migrations may make binary rollback unsafe unless expand-contract rules are followed.

---

# 159. Test Data Management

Use:

```text
synthetic data
deterministic seeds
sanitized production-shaped data
```

Never depend on live customer data.

---

# 160. Privacy in Test Artifacts

CI artifacts can leak data.

Apply the same safe diagnostics/redaction principles as production.

---

# 161. Secret Injection in Tests

Use ephemeral test credentials.

Never commit real provider/cloud secrets.

---

# 162. External Provider Tests

For email/payment/webhook providers:

```text
fake provider
sandbox provider
contract tests
```

Production provider calls should not be required for ordinary CI.

---

# 163. Contract Tests

Verify integration against provider expectations without making core correctness depend on provider availability.

---

# 164. Side-Effect Ambiguity Tests

Simulate:

```text
provider accepted request
response lost
```

and verify reconciliation/idempotency strategy.

---

# 165. HTTP Proxy Harness

A test proxy can inject:

```text
latency
disconnect
response loss
corruption
status codes
```

between client and Axum.

---

# 166. Network Partition Test

Keep client offline for long simulated period, generate operations, reconnect, and verify convergence.

---

# 167. Reordering Test

Although HTTP exchange may naturally constrain ordering, future transports and retries can reorder logical delivery.

Server must remain safe.

---

# 168. Duplicate Journal Delivery

Client reconciliation should tolerate duplicate authoritative events according to event identity/version rules.

---

# 169. Missing Journal Event Simulation

Anti-entropy must eventually detect semantic divergence if cursor/state become inconsistent through injected corruption.

---

# 170. Repair Test

Inject divergence:

```text
detect
plan
repair
verify
```

without losing pending intent.

---

# 171. Tombstone GC Test

Ensure stale clients cannot resurrect safely-GCed deleted data.

Clients beyond retention floor must rebootstrap.

---

# 172. Retention Lease Test

Active device/scope retention leases correctly constrain GC.

---

# 173. Legal Hold Test

Held data must not be removed by ordinary retention GC.

---

# 174. Erasure Test

Verify all governed copies follow erasure plan:

```text
authority
snapshots
derived stores
clients where applicable
```

according to policy.

---

# 175. Cryptographic Tests

Test:

```text
signature verification
wrong key
rotated key
expired key
tampered artifact
algorithm downgrade
```

---

# 176. Snapshot Signature Test

Tampered signed snapshot must fail before activation.

---

# 177. Authority Fork Test

Construct conflicting evidence:

```text
same epoch
same sequence
different checkpoint
```

Expected:

```text
fork detected
automatic synchronization stops
```

---

# 178. Epoch Rollback Test

Client with newer epoch must reject old authority timeline.

---

# 179. Read Replica Test

Verify replica reads never accept authoritative writes and consistency modes behave as declared.

---

# 180. Tenant Migration Test

For future partitioned authority:

```text
freeze
drain
copy
fence
promote
directory update
resume
```

must preserve authority uniqueness.

---

# 181. Air-Gapped Test

Block internet access entirely.

System must not fail because telemetry/update/package services are unreachable.

---

# 182. Dependency Failure Test

Optional:

```text
Redis
NATS
telemetry backend
```

failure cannot break core correctness when configured only as optional infrastructure.

---

# 183. Test Observability

Tests themselves should emit:

```text
scenario
seed
phase
component
```

to aid failure diagnosis.

---

# 184. Failure Artifact

On test failure automatically preserve:

```text
seed
safe logs
trace
DB metadata
operation IDs
model trace
config digest
registry digest
```

---

# 185. Minimal Failure Artifact

Property/model failures should be shrunk/minimized before long-term storage when possible.

---

# 186. Replayability

A bug report becomes much more valuable when represented as:

```text
deterministic test
model trace
fixture
```

---

# 187. Production Incident → Regression Test

Part 25 incident workflow should end with:

```text
incident
↓
reproduction
↓
minimal fixture
↓
regression test
↓
release gate if systemic
```

---

# 188. Test Naming

Use behavior-oriented names.

Good:

```text
retry_after_committed_response_loss_is_idempotent
```

not:

```text
test_sync_42
```

---

# 189. Stable Test IDs

Critical conformance/release tests can have IDs:

```text
AEQ-TEST-IDEMP-001
AEQ-TEST-CURSOR-002
AEQ-TEST-EPOCH-003
```

---

# 190. Invariant Coverage Map

Maintain:

```text
InvariantId -> TestIds
```

Example:

```text
AEQ-INV-CORE003
    -> AEQ-TEST-IDEMP-001
    -> AEQ-TEST-IDEMP-004
    -> AEQ-MODEL-IDEMP-002
```

---

# 191. Coverage Is Not Only Line Coverage

Track:

```text
code coverage
invariant coverage
protocol version coverage
migration coverage
adapter capability coverage
platform coverage
```

---

# 192. Line Coverage

Useful as a diagnostic signal, not a correctness score.

Do not chase 100% line coverage at the expense of meaningful tests.

---

# 193. Mutation Testing

Mutation testing can reveal tests that execute code without actually verifying behavior.

Use selectively on correctness-critical pure logic.

---

# 194. Test Flakiness

A flaky correctness test is a defect in the test infrastructure.

Track and fix it.

---

# 195. No Silent Retry of Failing Tests

CI should not simply rerun until green.

If retry is used for infrastructure diagnosis, original failure remains visible.

---

# 196. Quarantine

Temporarily quarantined test requires:

```text
owner
reason
issue
expiry
```

Critical invariant tests should rarely be quarantined.

---

# 197. Test Runtime Budget

Keep PR feedback fast by placing expensive suites in appropriate tiers.

Do not remove important tests merely because they are slow.

---

# 198. Parallelism

Tests may run in parallel when isolated.

Serial execution is acceptable for:

```text
shared authority failover
exclusive migration
specific process/fault tests
```

---

# 199. Resource Cleanup

Test harness owns lifecycle:

```text
database
server process
client process
temporary files
ports
```

Cleanup should be robust even after failure.

---

# 200. Random Port Allocation

Avoid fixed ports in parallel CI.

---

# 201. Database Isolation

Possible strategies:

```text
database per test
schema per test
tenant per test
```

Select based on required isolation/performance.

---

# 202. Testcontainers/Containers

Containers may simplify PostgreSQL integration but should not be the only possible harness.

Local native/CI service PostgreSQL should remain usable.

---

# 203. Nix/Dev Environment

A reproducible dev environment may pin:

```text
PostgreSQL
toolchain
test utilities
```

without making Nix a runtime dependency.

---

# 204. Test Configuration

Use dedicated RON profiles:

```text
config/test/
├── unit.ron
├── integration.ron
├── e2e.ron
└── fault.ron
```

---

# 205. Test Secrets

Use `SecretRef` to ephemeral test secret providers where integration requires credentials.

---

# 206. Test Database Migrations

E2E starts from:

```text
empty DB
↓
all migrations
```

and separate upgrade tests start from prior release schema.

---

# 207. Fixture Versioning

Fixtures should declare:

```text
schema version
protocol version
registry generation
```

---

# 208. Snapshot Fixture Versioning

Old snapshot fixtures ensure newer clients can handle supported historical formats.

---

# 209. Compatibility Corpus Retention

Do not delete fixtures for still-supported versions.

---

# 210. Fuzz Corpus Retention

Crashes found by fuzzing become permanent regression corpus entries.

---

# 211. Security Regression Corpus

Known malformed/abusive payload classes should remain tested.

---

# 212. Model Corpus

Important historical model-checker failure traces become fixed regression tests.

---

# 213. Performance Interaction

Correctness load tests and Part 47 performance tests share workload infrastructure but have different pass criteria.

---

# 214. Benchmark Does Not Replace Test

A fast benchmark that returns wrong state is a failed test.

---

# 215. Verification CLI

Suggested:

```text
aequora verify unit
aequora verify property
aequora verify model
aequora verify adapter sqlite
aequora verify adapter stoolap
aequora verify authority postgres
aequora verify compatibility
aequora verify migration
aequora verify release
aequora verify replay <artifact>
```

---

# 216. `aequora conform`

Part 30 conformance remains distinct:

```text
verify
```

tests this repository/build.

```text
conform
```

produces evidence that an implementation satisfies a declared certification profile.

---

# 217. Verification Report

Machine-readable report:

```rust
pub struct VerificationReport {
    pub build: BuildId,
    pub suite: SuiteId,
    pub environment: EnvironmentFingerprint,
    pub results: Vec<TestResult>,
    pub evidence: Vec<EvidenceRef>,
}
```

---

# 218. Release Evidence Bundle

A release candidate should retain:

```text
quality gate manifest
verification reports
conformance reports
benchmark report
SBOM
signatures
migration checks
compatibility report
```

---

# 219. Quality Gate Example

```text
ServerStandard:
    unit                  PASS
    property              PASS
    model                 PASS
    postgres              PASS
    axum_e2e              PASS
    compatibility         PASS
    migration             PASS
    security              PASS
    performance           PASS
```

---

# 220. Release Blocking

Block release on failure of required:

```text
correctness
security
migration
compatibility
official-adapter
artifact-integrity
```

gates.

---

# 221. Advisory Gates

Some early performance or optional-platform checks may initially be advisory.

Their status must be explicit.

---

# 222. GA Gate

General Availability should require stable evidence, not only feature completeness.

GA criteria belong in Part 50.

---

# 223. AI-Assisted Development Verification

Because AI may generate substantial code, verification must assume plausible-looking code can contain subtle semantic mistakes.

Required workflow:

```text
AI/code author proposes implementation
↓
compiler/type system
↓
unit/property tests
↓
invariant tests
↓
adapter/integration tests
↓
review
```

---

# 224. AI Must Not Rewrite Tests to Hide Failure

When implementation fails a stable invariant test, default action is:

```text
fix implementation
```

not:

```text
weaken/delete test
```

unless architecture/invariant was deliberately changed through review.

---

# 225. Prompt/Agent Repository Rules

Repository instructions for coding agents should state:

```text
never bypass Tx A/B/C
never weaken OperationId idempotency
never advance cursor outside Tx C
never silently delete pending outbox
never modify stable protocol/registry IDs without governance
never disable failing correctness tests
```

---

# 226. Test-First for Critical Bugs

For correctness incidents:

```text
reproduce failing case
↓
commit regression test
↓
fix
↓
prove test passes
```

---

# 227. Verification Invariants

## AEQ-INV-VERIFY001

```text
Every correctness-critical Aequora invariant has executable verification evidence at one or more appropriate layers.
```

## AEQ-INV-VERIFY002

```text
Retries, crashes, process death, and ambiguous commits are tested explicitly; successful request/response tests alone are insufficient.
```

## AEQ-INV-VERIFY003

```text
Official storage adapters pass the same semantic conformance properties for every capability they claim, regardless of implementation technology.
```

## AEQ-INV-VERIFY004

```text
Cursor durability is tested atomically with authoritative local reconciliation, and no injected failure may leave a durable cursor ahead of applied state.
```

## AEQ-INV-VERIFY005

```text
A committed authoritative operation followed by response loss and retry produces one logical effect and the same authoritative outcome.
```

## AEQ-INV-VERIFY006

```text
Compatibility and migration tests preserve legitimate pending operations, cursor state, conflicts, authority metadata, and durable identifiers across supported upgrades.
```

## AEQ-INV-VERIFY007

```text
Fault-injection and randomized test failures are reproducible from preserved seeds, traces, fixtures, or failure artifacts.
```

## AEQ-INV-VERIFY008

```text
Release qualification cannot silently waive a failed required gate; every waiver is explicit, owned, risk-documented, scoped, and expiring.
```

## AEQ-INV-VERIFY009

```text
Production incidents that reveal systemic correctness defects become durable regression tests or model traces before the fix is considered complete.
```

## AEQ-INV-VERIFY010

```text
Test infrastructure never weakens production correctness semantics merely to make verification faster or easier.
```

---

# 228. Recommended Initial Implementation Order

Build verification infrastructure in this order:

```text
1. aequora-testkit
2. deterministic clock + ID sources
3. ReferenceStore
4. core invariant assertions
5. property strategies
6. fake transport
7. model state machine
8. SQLite conformance
9. PostgreSQL integration
10. Stoolap conformance
11. Tx A/B/C failpoints
12. process-kill harness
13. HTTP E2E
14. migration/compatibility matrix
15. fuzzing
16. release quality manifest
```

---

# 229. Minimum v1 Release Suite

At minimum:

```text
unit
property
model
SQLite local adapter
Stoolap claimed-profile tests
PostgreSQL authority
Axum HTTP
protocol compatibility
migration
process-kill Tx A/Tx C
ambiguous Tx B
bootstrap
conflict
scope
security
release artifact integrity
```

---

# 230. Highest-Priority Scenarios

If resources are initially limited, prioritize:

```text
1. Tx A crash atomicity
2. Tx B idempotency under response loss
3. Tx C cursor atomicity
4. duplicate OperationId race
5. pending outbox across restart/upgrade
6. stale-client tombstone behavior
7. authority epoch transition
8. tenant isolation
9. bootstrap + journal boundary
10. disk-full behavior
```

These protect the architecture's most valuable guarantees.

---

# 231. Final Architecture

```text
                    Source Change
                         |
                         v
                 Compiler / Type System
                         |
                         v
                Unit + Property Tests
                         |
                         v
                 Deterministic Model
                         |
                         v
              Adapter Conformance Suite
                  /       |       \
                 v        v        v
              SQLite   Stoolap  PostgreSQL
                  \       |       /
                   +------+------+
                          |
                          v
                  Integration Tests
                          |
                          v
                  Fault Injection
                          |
                          v
                    HTTP / E2E
                          |
                          v
               Load / Soak / Security
                          |
                          v
                Release Quality Gates
                          |
                          v
                 Evidence + Artifacts
```

Incident feedback loop:

```text
Production Incident
        |
        v
Reproducible Failure Artifact
        |
        v
Minimal Regression Test
        |
        v
Fix + Verification
        |
        v
Permanent Release Gate
```

---

# 232. Final Recommendation

Aequora should treat testing infrastructure as part of the product architecture, not as cleanup after implementation.

The strongest workflow is:

```text
state invariant
↓
encode it in types where possible
↓
encode it as executable tests
↓
inject the failures most likely to violate it
↓
run the same semantics across adapters
↓
preserve every discovered counterexample
↓
require evidence before release
```

The core question for every critical feature should be:

> **“How will we prove this remains correct if the process crashes at the worst possible instruction, the response disappears, the request is retried, another client races it, the database is upgraded, and the user reconnects days later?”**

If the architecture cannot answer that question with executable verification, the feature is not yet production-ready.

---

## Next

**Part 49 — Licensing, Dependency Policy, Supply-Chain Security, Crate Governance, SBOM, Reproducible Builds, and Third-Party Risk Architecture**
