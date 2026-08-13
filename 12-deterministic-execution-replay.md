# Aequora Sync — Part 12

# Deterministic Domain Execution, Replay, and Reproducibility Architecture

## 1. Purpose

Aequora already defines:

```text
typed operations
authoritative execution
causality/provenance
formal correctness
idempotency
consistency profiles
bulk migration
snapshot/bootstrap
```

But production debugging becomes difficult if executing the same operation twice can produce different results because handlers directly read:

```text
wall clock
random number generator
UUID generator
environment variables
external API
filesystem state
network state
machine locale
non-deterministic iteration order
```

Aequora therefore needs a deterministic execution architecture.

The goal is not to pretend all real-world systems are deterministic.

The goal is to make nondeterminism **explicit, captured, injectable, and replayable**.

The central rule is:

> **A domain handler should be deterministic with respect to its declared input, authoritative state, and explicitly captured execution inputs.**

---

# 2. Goals

The deterministic execution subsystem should provide:

```text
replayable domain decisions
reproducible incidents
stable testing
deterministic simulations
controlled clocks
controlled randomness
captured external results
portable historical debugging
migration verification
event regeneration checks
```

---

# 3. Non-Goals

It does not require:

```text
deterministic database scheduling
identical CPU instruction order
bit-identical logs
replaying arbitrary external internet state
replaying side effects by actually resending them
```

Instead, it isolates nondeterministic inputs from domain logic.

---

# 4. Execution Model

A server operation should execute against:

```text
ValidatedOperation
+
AuthoritativeState
+
ExecutionContext
```

where `ExecutionContext` contains all nondeterministic values the handler is allowed to use.

---

# 5. ExecutionContext

Conceptually:

```rust
pub struct ExecutionContext {
    pub operation_id: OperationId,
    pub correlation_id: CorrelationId,
    pub principal: OriginPrincipal,
    pub device_id: Option<DeviceId>,

    pub logical_time: DomainTimestamp,
    pub id_source: DeterministicIdSource,
    pub randomness: DeterministicRandomSource,

    pub captured_inputs: CapturedInputs,
}
```

---

# 6. Domain Handler Rule

Domain handlers should not directly call:

```rust
SystemTime::now()
Uuid::new_v4()
rand::thread_rng()
std::env::var(...)
reqwest::get(...)
```

Instead use values/services supplied by the execution context.

---

# 7. Clock Abstraction

Define:

```rust
pub trait DomainClock {
    fn now(&self) -> DomainTimestamp;
}
```

Production implementation:

```text
server-controlled clock
```

Test/replay implementation:

```text
fixed deterministic clock
```

---

# 8. Server Time vs Client Time

Client HLC/time is useful metadata.

But authoritative domain decisions requiring trusted time should use:

```text
server-controlled captured logical time
```

Do not trust arbitrary client wall clock for:

```text
payment deadlines
approval time
subscription expiry
```

---

# 9. Capture Time Once

For one operation execution:

```text
capture authoritative execution time once
```

and expose stable value throughout handler.

Do not let multiple `now()` calls drift during the same operation unless semantics explicitly require phases.

---

# 10. OperationExecutionTime

Define:

```rust
pub struct OperationExecutionTime(Timestamp);
```

Persist it in operation ledger/outcome metadata where relevant.

---

# 11. Randomness Abstraction

Define:

```rust
pub trait DomainRandom {
    fn fill_bytes(&mut self, out: &mut [u8]);
}
```

Production:

```text
secure RNG seed captured/generated at execution boundary
```

Replay:

```text
same captured seed
```

---

# 12. Deterministic PRNG

For replayable non-security randomness:

```text
captured seed
→ deterministic PRNG
```

Examples:

```text
shuffling non-security workloads
randomized simulation
non-secret generated codes where policy allows
```

---

# 13. Cryptographic Randomness Warning

Secrets/tokens/password reset codes may require cryptographically secure randomness.

Do not expose replay artifacts containing secret random outputs carelessly.

For high-security values, replay may use:

```text
captured redacted result
```

instead of reproducing secret generation.

---

# 14. ID Generation

Client-created entities already use client-generated distributed IDs where appropriate.

Server-derived entities need deterministic/replayable identity semantics.

Options:

```text
derive from OperationId + role
capture generated ID in execution input
allocate stable server ID before handler
```

---

# 15. Deterministic Derived ID

For internal derived records:

```text
hash(namespace, OperationId, semantic child key)
```

can yield stable ID.

Example:

```text
ReceiptId = derive(OperationId, "receipt")
```

This is excellent for retry and replay.

---

# 16. ID Derivation Caution

Do not derive IDs where unpredictability/security is required.

Use captured secure generation instead.

---

# 17. Stable Child Keys

If one operation creates multiple children:

```text
line-0
line-1
tax
receipt
```

derive using stable semantic keys, not unordered map iteration position.

---

# 18. Non-Deterministic Collection Iteration

Rust `HashMap` iteration order is intentionally not stable.

If handler outcome depends on order:

```text
sort by stable key
```

or use:

```text
BTreeMap
BTreeSet
```

where semantic ordering matters.

---

# 19. Stable Serialization

Replay comparison requires canonical serialization.

Use:

```text
stable field identifiers
explicit enum IDs
canonical collection ordering
```

for hash/equivalence checks.

---

# 20. Execution Input Envelope

Define:

```rust
pub struct ExecutionInputs {
    pub execution_time: OperationExecutionTime,
    pub random_seed: Option<ExecutionSeed>,
    pub allocated_ids: SmallVec<[AllocatedId; 4]>,
    pub external_results: Vec<CapturedExternalResult>,
}
```

---

# 21. ExecutionSeed

```rust
pub struct ExecutionSeed([u8; 32]);
```

Store only if safe and necessary.

---

# 22. Captured External Result

External systems are nondeterministic.

Examples:

```text
payment gateway
email provider
identity provider
tax service
remote API
```

A domain handler should not make irreversible external calls inside authoritative DB transaction.

Use durable side-effect architecture.

---

# 23. Transactional Side Effects

Pattern:

```text
domain operation
↓
authoritative DB transaction
↓
write SideEffectIntent
↓
COMMIT
↓
worker performs external call
↓
capture result
↓
new authoritative operation/event if needed
```

---

# 24. Why This Helps Determinism

The original domain transaction only decides:

```text
what side effect should be attempted
```

not:

```text
what external service happened to return during transaction
```

---

# 25. External Result as New Input

When gateway responds:

```text
GatewayResult
```

convert it into:

```text
new operation/event
```

with captured payload.

Then replay uses that captured result.

---

# 26. Example — Payment

```text
CreatePaymentIntent
↓
authoritative event PaymentIntentCreated
↓
side-effect worker contacts PSP
↓
PSP returns success/ref XYZ
↓
RecordPaymentGatewayResult operation
↓
PaymentConfirmed event
```

Replay does not call PSP again.

---

# 27. ExternalResponseId

Persist provider response/reference IDs.

Use Part 02 lineage:

```text
gateway result operation caused_by payment intent
same or related correlation
```

---

# 28. Replay Modes

Define:

```rust
pub enum ReplayMode {
    VerifyDecision,
    RebuildProjection,
    Simulate,
    HistoricalDebug,
    MigrationCheck,
}
```

---

# 29. VerifyDecision

Re-run domain handler against captured inputs and historical state.

Compare:

```text
expected authoritative outcome
vs
replayed outcome
```

---

# 30. RebuildProjection

Replay authoritative events into:

```text
derived read model
search index
analytics projection
```

No business side effects.

---

# 31. Simulate

Use deterministic TestKit environment to explore:

```text
alternative failure/order scenarios
```

---

# 32. HistoricalDebug

Reconstruct:

```text
why operation produced this result
```

using historical inputs and provenance.

---

# 33. MigrationCheck

Run old and new handler/schema versions on same captured fixtures to detect semantic drift.

---

# 34. Replay Bundle

A replay artifact should contain:

```text
operation envelope
validated domain payload
execution inputs
required pre-state snapshot/hash
handler/profile version
expected outcome
causality metadata
```

---

# 35. ReplayBundle

Conceptually:

```rust
pub struct ReplayBundle {
    pub format_version: u16,
    pub operation: CanonicalOperation,
    pub execution_inputs: ExecutionInputs,
    pub pre_state: ReplayStateRef,
    pub handler_version: HandlerVersion,
    pub expected: ExecutionOutcomeDigest,
}
```

---

# 36. Pre-State Strategies

Options:

```text
embedded minimal aggregate snapshot
snapshot reference
journal replay range
canonical state fixture
```

---

# 37. Minimal Replay State

For one aggregate operation, prefer:

```text
only required aggregate state
```

not entire tenant DB.

---

# 38. Read Set

Handlers can declare/read through repository interfaces.

For advanced replay, capture:

```text
which aggregate/entities were read
```

This helps create minimal reproducible state.

---

# 39. ReadSet

Conceptually:

```rust
pub struct ReadSet {
    pub entities: Vec<EntityVersionRef>,
}
```

---

# 40. Read Set Is Diagnostic

Do not initially make full dynamic read-set tracking mandatory for every handler.

Can be enabled for high-assurance modules.

---

# 41. Write Set

Execution result should make authoritative writes explicit.

```rust
pub struct WriteSet {
    pub mutations: Vec<CanonicalMutation>,
}
```

This improves replay comparison.

---

# 42. Handler Should Return Decision, Not Commit Directly

Ideal architecture:

```text
handler reads through domain repository
↓
returns ExecutionPlan / DomainDecision
↓
executor persists plan atomically
```

This separates:

```text
decision
from
persistence
```

and improves replay.

---

# 43. ExecutionPlan

Conceptually:

```rust
pub struct ExecutionPlan {
    pub mutations: Vec<DomainMutation>,
    pub events: Vec<DomainEvent>,
    pub side_effects: Vec<SideEffectIntent>,
}
```

---

# 44. Validation Before Persistence

Executor verifies plan invariants before commit.

---

# 45. Plan Determinism

Given same:

```text
validated operation
pre-state
execution inputs
handler version
```

the plan should be semantically identical.

---

# 46. Plan Digest

Compute canonical:

```text
ExecutionPlanDigest
```

using BLAKE3.

Persist optionally for high-assurance operations.

---

# 47. Outcome Digest

Operation ledger may store:

```text
outcome digest
```

for replay verification.

---

# 48. Handler Version

Define stable:

```rust
pub struct HandlerVersion(u32);
```

Do not use Git hash alone as semantic version.

---

# 49. Operation Schema vs Handler Version

Separate:

```text
operation schema version
handler semantic version
```

Same payload schema can have changed business implementation.

---

# 50. Handler Upgrade

If behavior changes intentionally:

```text
HandlerVersion increments
```

Historical replay selects original version when available.

---

# 51. Historical Handler Retention

Options:

```text
keep old handler code
keep WASM/plugin artifact
keep deterministic decision fixtures
accept limited replay horizon
```

Initial recommendation:

```text
retain old semantic handlers/upcasters for supported audit window
```

where practical.

---

# 52. Do Not Promise Infinite Replay

Long-term exact replay may become impossible after:

```text
code removal
schema retirement
external policy changes
```

Document retention horizon.

---

# 53. Replay Compatibility Window

Define:

```text
supported replay versions
```

similar to protocol compatibility.

---

# 54. Upcasting Historical Operations

Before replay:

```text
old operation payload
↓
historical handler path
```

or:

```text
upcast into replay-compatible canonical form
```

Do not silently use latest semantics.

---

# 55. Policy Dependencies

Domain decisions may depend on policy/configuration.

Example:

```text
late fee percentage
approval threshold
grading policy
```

These are nondeterministic over time unless versioned.

---

# 56. Versioned Policy

Handler receives:

```text
PolicySnapshot / PolicyVersion
```

captured at operation execution.

---

# 57. Config Is Input

Do not read mutable global config ad hoc during handler.

Resolve config into explicit:

```text
DomainPolicyContext
```

---

# 58. Feature Flags

Feature flags can change behavior.

Capture:

```text
semantic feature set/version
```

for operations where flag affects domain outcome.

---

# 59. Environment Variables

Infrastructure env vars should not alter business logic directly.

Business policy should come through versioned configuration service/context.

---

# 60. Locale

Formatting locale should not affect authoritative numeric/date logic.

Store canonical values.

Formatting belongs at UI/export boundary.

---

# 61. Floating Point

Avoid floating-point nondeterminism for finance/business invariants.

Use:

```text
exact Decimal
integer minor units
fixed-point
```

---

# 62. Decimal Canonicalization

Part 03 digest and replay should use exact:

```text
coefficient + scale
```

representation.

---

# 63. Time Zones

Business local dates may depend on tenant timezone.

Capture:

```text
TenantTimezoneVersion / timezone ID
```

if decision uses local calendar boundaries.

---

# 64. Timezone Database Changes

Historical timezone rules can change in tzdata.

For high-assurance replay, capture:

```text
resolved offset/local boundary
```

or tzdb version.

---

# 65. Date-Based Rules

Example:

```text
fee due after local midnight
```

Handler should receive resolved authoritative time context.

---

# 66. External Exchange Rates

Exchange rate is external nondeterministic data.

Capture:

```text
rate
source
timestamp
reference ID
```

as explicit input.

Do not fetch during replay.

---

# 67. Tax Tables

Version tax/business rule tables.

Historical execution references exact policy version.

---

# 68. Machine Learning / AI

If future domain workflow uses AI:

```text
model output is nondeterministic/external input
```

Never expect exact regeneration.

Capture:

```text
model ID/version
prompt/input digest
returned structured decision
```

and treat accepted AI result as explicit input subject to validation.

---

# 69. AI Must Not Be Hidden in Core Deterministic Handler

Use:

```text
side-effect/decision request
↓
capture output
↓
validated operation
```

---

# 70. Replay Safety

Replay must never:

```text
send email
charge card
call webhook
modify production DB
```

unless explicitly running a controlled migration mode.

---

# 71. Replay Side-Effect Sink

Use:

```rust
pub trait SideEffectSink {
    fn record(&mut self, intent: SideEffectIntent);
}
```

Replay sink only records/compares.

Production sink persists durable intents.

---

# 72. Dry Execution

Executor can support:

```text
DryRun
```

that executes handler/plan construction without committing.

Useful for:

```text
migration validation
admin preview
tests
```

---

# 73. Dry Run Security

Authorization still applies unless running privileged test/admin environment.

---

# 74. Replay Database

Replay should run against:

```text
in-memory canonical store
temporary DB
snapshot sandbox
```

not production writable tables.

---

# 75. Deterministic Repository

TestKit provides:

```text
BTreeMap-based canonical repository
```

with deterministic query ordering.

---

# 76. Query Ordering Rule

If handler needs deterministic "first" result:

```text
query must specify stable ordering
```

Never rely on database natural order.

---

# 77. SQL Ordering

Database adapter repositories must add explicit `ORDER BY` where semantic order matters.

---

# 78. Concurrency

Replay of one operation uses captured pre-state.

It does not attempt to reproduce actual thread scheduling.

Concurrency effects are represented by:

```text
which authoritative version/state existed at execution
```

---

# 79. Conflict Replay

To replay a conflict decision, capture:

```text
operation base version
current authoritative version
relevant state
policy version
```

---

# 80. StrongAggregate Replay

Part 11 `StrongAggregate` replay should use one canonical aggregate snapshot.

---

# 81. ImmutableAppendOnly Replay

Verify:

```text
same input
→ same append plan / semantic identifiers
```

---

# 82. Commutative Replay

Can verify permutation invariants.

---

# 83. LWW Replay

Winner should be based on captured server ordering/time policy, not current wall clock.

---

# 84. DerivedProjection Replay

Ideal use case:

```text
drop projection
↓
replay source events
↓
rebuild
```

---

# 85. Replay and Journal

Authoritative journal is a major replay source.

But journal may contain projections rather than full original command context.

Therefore keep operation ledger/provenance for command replay where required.

---

# 86. Operation Ledger Extension

Recommended optional fields:

```text
handler_version
execution_time
execution_input_digest
execution_plan_digest
policy_version
```

---

# 87. Full Inputs vs Digests

Do not store every full replay input forever automatically.

Profiles can choose:

```text
DigestOnly
ReplayWindow
FullAuditReplay
```

---

# 88. ReplayRetentionPolicy

```rust
pub enum ReplayRetentionPolicy {
    None,
    DigestOnly,
    Window(Duration),
    Full,
}
```

---

# 89. Finance Replay Policy

High-assurance finance may retain enough input/state metadata for long audit horizon.

---

# 90. Privacy Tradeoff

Replay data may duplicate sensitive state.

Minimize:

```text
store IDs/digests where sufficient
encrypt sensitive replay artifacts
apply retention
```

---

# 91. Replay Bundle Export

Admin CLI:

```text
aequora replay export <operation-id>
```

Produces sanitized RON/Postcard bundle.

---

# 92. Replay Bundle Format

Use:

```text
RON manifest
Postcard binary payloads
BLAKE3 checksums
```

---

# 93. Bundle Integrity

Manifest includes hashes for all payload sections.

---

# 94. Bundle Encryption

Optional:

```text
encrypt at rest/export
```

for sensitive incidents.

---

# 95. Replay CLI

Suggested:

```text
aequora replay verify <operation-id>
aequora replay run <bundle>
aequora replay explain <operation-id>
aequora replay compare --old-handler X --new-handler Y
```

---

# 96. Explain Mode

Output:

```text
operation
pre-state version
execution time
policy version
handler version
decision
events
side-effect intents
outcome digest
```

---

# 97. Differential Handler Testing

Run:

```text
handler v1
handler v2
```

against same replay corpus.

Classify differences:

```text
expected
breaking
unexpected
```

---

# 98. Golden Replay Corpus

Maintain representative production-sanitized or synthetic fixtures.

Examples:

```text
payment
refund
attendance
student update
scope transfer
workflow approval
```

---

# 99. CI Replay Gate

Before releasing domain handler changes:

```text
run replay corpus
```

Unexpected semantic differences fail CI.

---

# 100. Mutation Testing

Part 01 mutation testing can target replay-sensitive code.

If semantic mutation does not change replay digest/tests:

```text
coverage may be insufficient
```

---

# 101. Determinism Linter

Potential custom lint/guideline:

Flag direct uses in domain crates of:

```text
SystemTime::now
rand::thread_rng
Uuid::new_v4
reqwest
std::env
filesystem
```

unless explicitly allowed.

---

# 102. Crate Boundary Enforcement

Domain crates should not depend directly on:

```text
reqwest
tokio networking
OS filesystem APIs
```

Business execution stays pure-ish.

---

# 103. Dependency Policy

Use workspace architecture/lint tooling to enforce allowed dependency graph.

---

# 104. Handler Signature

Conceptual:

```rust
pub trait OperationHandler<O> {
    async fn decide(
        &self,
        op: &Validated<O>,
        state: &dyn DomainReadRepository,
        ctx: &ExecutionContext,
    ) -> Result<ExecutionPlan, DomainError>;
}
```

---

# 105. Executor Signature

```rust
pub trait PlanExecutor {
    async fn commit(
        &self,
        op: &ExecutableOperation,
        plan: ExecutionPlan,
        tx: &mut dyn AuthoritativeTransaction,
    ) -> Result<CommittedOutcome, ExecutionError>;
}
```

---

# 106. Separate Decision From Commit

This is one of the strongest architectural improvements for Aequora.

Benefits:

```text
deterministic testing
replay
dry run
plan verification
easier audit
clean transaction semantics
```

---

# 107. Practical Exception

Some handlers require transactional reads/locks during decision.

In that case:

```text
decision executes inside transaction
```

but still uses explicit `ExecutionContext`.

Replay supplies equivalent state snapshot.

---

# 108. Locking Is Not Replay Input

Locks are concurrency mechanism.

Replay cares about resulting pre-state/version.

---

# 109. Server-Generated Sequence

Authoritative journal `Sequence` is allocated during commit.

It is not part of domain decision unless business semantics explicitly expose it.

---

# 110. Sequence Replay

Replay can compare:

```text
event semantic contents
```

while ignoring different test sequence number.

Use semantic equality vs physical equality.

---

# 111. Equality Levels

Define:

```rust
pub enum ReplayComparison {
    ExactBytes,
    Semantic,
    OutcomeClass,
}
```

---

# 112. ExactBytes

Useful for:

```text
canonical serializer regression
```

---

# 113. Semantic

Ignores irrelevant generated metadata.

Preferred for most handler replay.

---

# 114. OutcomeClass

Useful when exact details intentionally changed but acceptance/rejection class should remain.

---

# 115. Canonical Normalization

Before semantic comparison, normalize:

```text
unordered collections
generated sequence
transport IDs
ephemeral trace IDs
```

---

# 116. Replay Divergence

If replay differs:

```text
handler drift
missing captured input
nondeterministic logic
bad historical state reconstruction
```

Classify explicitly.

---

# 117. Divergence Report

Include:

```text
expected plan digest
actual plan digest
first differing mutation/event
handler versions
input digests
```

---

# 118. Non-Replayable Handler

Allow explicit marker:

```text
ReplaySupport::Unsupported
```

only for exceptional legacy integrations.

Production diagnostics should warn.

---

# 119. Profile Requirement

Part 11 profiles can specify replay expectation.

Examples:

```text
ImmutableAppendOnly finance:
    Full deterministic decision expected

DerivedProjection:
    Full replay expected

DeviceLocal:
    no server replay
```

---

# 120. Adapter Independence

Replay state is canonical.

It should work regardless of original DB:

```text
PostgreSQL
MySQL
Stoolap
SQLite
```

---

# 121. Database Migration Verification

Part 09 can replay selected operations against migrated target to verify equivalent behavior.

---

# 122. Snapshot Verification

Part 10 can use deterministic projection replay after snapshot activation.

---

# 123. Anti-Entropy Forensics

Part 03 mismatch investigation can find:

```text
last source EventId
```

then replay responsible operation when metadata available.

---

# 124. Multi-Process Independence

Part 05 process leadership does not affect deterministic domain result.

ProcessInstanceId is operational and should not enter business decision unless explicitly required.

---

# 125. Scheduler Independence

Part 06 timing/scheduling must not change semantics.

Two different retry schedules for same operation should produce same result given same authoritative pre-state and captured execution inputs.

---

# 126. Live Hint Independence

Part 08 hints never affect domain execution result.

---

# 127. Scope Projection Determinism

Part 07 scope projector should also be deterministic from:

```text
authoritative event
resolved scope version
projection schema
```

---

# 128. Scope Projection Replay

Useful for verifying:

```text
same business event
→ same client projection
```

across releases.

---

# 129. Side Effect Determinism

Side-effect **intent** should be deterministic.

Actual external outcome is captured later.

---

# 130. Email Example

Handler emits:

```text
SendEmailIntent {
    template_id,
    recipient_ref,
    template_data
}
```

Replay compares intent.

It does not send email.

---

# 131. Template Version

If rendered content matters legally:

```text
capture template version
```

not just template name.

---

# 132. Document Generation

PDF/document output may depend on rendering engine version.

Capture:

```text
template version
renderer version
input digest
```

if exact regeneration matters.

---

# 133. Cryptographic Signatures

Signature generation may be nondeterministic depending on algorithm.

Treat signature result as captured output/side effect where appropriate.

---

# 134. Deterministic Signature Algorithms

If using deterministic algorithms, still separate secret key handling from replay artifact.

Never export private keys.

---

# 135. Replay Security Boundary

Only privileged users/tools may access historical replay data.

Replay should never bypass tenant isolation.

---

# 136. Cross-Tenant Replay

Disallow by default.

Replay bundle should be tenant-scoped and sanitized.

---

# 137. Production Replay Environment

Run:

```text
isolated sandbox
read-only source
no external side effects
```

---

# 138. Resource Limits

Untrusted replay bundle must still enforce:

```text
size limits
record count limits
decode limits
dependency limits
```

---

# 139. Fuzz Replay Decoder

Replay bundle parser is an input surface.

Fuzz it.

---

# 140. Correctness Invariants

Add:

## AEQ-INV-DET001

```text
Given identical validated operation, canonical pre-state, handler version, and execution inputs, deterministic handlers produce semantically equivalent execution plans.
```

## AEQ-INV-DET002

```text
Domain handlers do not depend on untracked wall-clock time or randomness.
```

## AEQ-INV-DET003

```text
Replay never performs real external side effects.
```

## AEQ-INV-DET004

```text
A retried OperationId cannot receive a different captured semantic execution input after authoritative commit.
```

## AEQ-INV-DET005

```text
Handler semantic version changes are explicit and compatibility-tested.
```

## AEQ-INV-DET006

```text
Mutable policy/config affecting domain decisions is referenced by an explicit version/snapshot.
```

---

# 141. TestKit

Provide:

```text
FixedClock
DeterministicRandom
DeterministicIdAllocator
CapturedExternalService
InMemoryReplayRepository
NoopSideEffectSink
```

---

# 142. Deterministic Simulation

Part 01 simulator should route all nondeterminism through deterministic seeds.

One seed can reproduce:

```text
network failures
operation generation
clock progression
handler randomness
```

while preserving subsystem separation.

---

# 143. Property Tests

Run same operation many times with same seed/state.

Assert same plan.

Then vary only declared input and verify controlled differences.

---

# 144. Collection Order Test

Insert same map/set data in different physical order.

Replay result must remain semantically identical.

---

# 145. Clock Test

Advance real system clock during handler test.

Fixed execution context should keep result unchanged.

---

# 146. Random Test

Same captured seed:

```text
same semantic random result
```

Different seed:

```text
different result only where allowed
```

---

# 147. External Service Test

Replay uses captured gateway response.

Assert no network call is made.

---

# 148. Handler Upgrade Test

Replay historical corpus through new handler.

Generate difference report.

---

# 149. Crash Test

If server crashes after plan generation but before commit:

```text
no authoritative result
```

Retry may recalculate with a new execution input because no commit occurred, depending on policy.

---

# 150. Commit-Ambiguity Rule

If commit may have succeeded:

```text
OperationId ledger decides
```

Do not recompute with new time/randomness until dedup lookup determines operation truly absent.

---

# 151. Input Reservation

For operations where generated inputs must remain stable across pre-commit retry attempts, reserve/persist them before or within authoritative transaction.

Example:

```text
allocated business number
```

---

# 152. Business Sequence Numbers

Invoice number allocation may be authoritative state.

Do not derive from replay random source.

Treat as:

```text
transactionally allocated domain value
```

and capture in committed outcome.

---

# 153. Sequence Gap Policy

If allocation transaction rolls back, business numbering policy determines whether gap allowed.

This is domain-specific.

---

# 154. Execution Input Persistence Strategy

Three levels:

```text
Minimal:
    time + versions/digests

Standard:
    time + generated IDs + policy version + plan digest

FullReplay:
    all replay-required inputs + minimal pre-state
```

---

# 155. Configuration

Example:

```ron
replay: (
    enabled: true,
    retention: Window(days: 90),
    persist_plan_digest: true,
    persist_execution_time: true,
    export_sensitive_payloads: false,
)
```

---

# 156. Observability

Structured tracing fields:

```text
operation_id
handler_version
execution_input_digest
execution_plan_digest
replay_supported
```

---

# 157. Metrics

```text
replay_verify_total
replay_divergence_total
handler_determinism_failure_total
replay_bundle_export_total
```

Avoid operation IDs as metric labels.

---

# 158. Logs

Events:

```text
replay_verified
replay_diverged
handler_version_changed
replay_unsupported
```

---

# 159. Recommended Modules

```text
aequora-server/
└── execution/
    ├── context.rs
    ├── clock.rs
    ├── random.rs
    ├── ids.rs
    ├── plan.rs
    ├── policy.rs
    └── deterministic.rs

aequora-replay/
├── bundle.rs
├── runner.rs
├── compare.rs
├── sandbox.rs
├── report.rs
└── fixtures.rs
```

---

# 160. Public Developer API

Application handler:

```rust
async fn decide(
    &self,
    op: &Validated<PostPayment>,
    repo: &dyn FinanceReadRepository,
    ctx: &ExecutionContext,
) -> Result<ExecutionPlan, DomainError>
```

No direct network, random, or system-clock calls.

---

# 161. Crate Dependency Rule

Domain crates should depend on:

```text
aequora-core
domain types
repository traits
```

not:

```text
Axum
reqwest
OS services
payment SDKs
```

---

# 162. Side-Effect Worker Separation

External SDKs belong in:

```text
infrastructure/worker crates
```

not core domain handlers.

---

# 163. Completion Criteria

Part 12 is complete when:

```text
[ ] ExecutionContext defined
[ ] domain clock abstraction defined
[ ] deterministic/random input policy defined
[ ] server-derived ID policy defined
[ ] external result capture architecture defined
[ ] side effects removed from authoritative handler transaction
[ ] ExecutionPlan defined
[ ] decision-vs-commit separation defined
[ ] HandlerVersion defined
[ ] policy/config version capture defined
[ ] ReplayBundle defined
[ ] replay modes defined
[ ] replay sandbox defined
[ ] differential handler CI defined
[ ] determinism lints/dependency boundaries defined
[ ] property/replay/fault tests defined
[ ] correctness invariants added
```

---

# 164. Final Architecture

```text
                  VALIDATED OPERATION
                           │
                           ▼
                 Authoritative Pre-State
                           │
                           ▼
                   ExecutionContext
             ┌─────────────┼─────────────┐
             ▼             ▼             ▼
           Time        Random/IDs      Policy
             │             │             │
             └─────────────┼─────────────┘
                           ▼
                    Domain Handler
                           │
                           ▼
                    ExecutionPlan
              ┌────────────┼────────────┐
              ▼            ▼            ▼
          Mutations      Events     SideEffectIntents
              │            │            │
              └────────────┼────────────┘
                           ▼
                 Authoritative Commit
                           │
                           ▼
                  Operation Ledger
                 + execution metadata

Replay:

        Operation + Pre-State + Captured Inputs
                           │
                           ▼
                    Same Handler Version
                           │
                           ▼
                    Replayed Plan
                           │
                           ▼
                 Semantic Comparison
```

The architectural principle is:

> **Aequora should not try to eliminate nondeterminism from the real world; it should prevent nondeterminism from being hidden inside domain execution.**

By capturing time, randomness, IDs, policies, and external results explicitly, Aequora can make authoritative decisions reproducible, testable, explainable, and safely replayable across incidents, migrations, and software upgrades.
