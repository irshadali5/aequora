# Part 12 Deterministic Execution and Replay Completion

Part 12 is implemented at Aequora's database-neutral reusable boundary. The framework makes every
decision-affecting nondeterministic value explicit and replayable; applications and adapters own
authenticated capture, canonical domain data, durable storage, secret governance, and production
acceptance.

## Repository guarantees

| Requirement | Evidence |
|---|---|
| Captured context | `ExecutionContext` binds operation/correlation identity, authenticated origin, one domain timestamp, ID/random seeds, allocated IDs, external results, and policy/config identity. |
| Time, randomness, and IDs | `DomainClock`, `FixedClock`, `DeterministicRandom`, and `DeterministicIdAllocator` prevent ambient input from entering a replayable handler. Labelled stateless derivation avoids iteration-order dependence. |
| External inputs | `CapturedExternalResult` validates bounded canonical bytes and an integrity digest before the value enters a decision. |
| Pure decision | `ReplayHandler::decide` consumes only the canonical operation, canonical pre-state, and execution context and produces a verified `ExecutionPlan`. |
| Explicit plan | Mutations, events, durable `SideEffectIntent` values, and the canonical result are bounded, validated, and covered by `ExecutionPlanDigest`. |
| Commit separation | `PlanCommitter` is a distinct adapter boundary. `InMemoryPlanCommitter` proves duplicate replay, input-drift rejection, pre-commit failure, and committed-but-lost-response behavior. |
| Version safety | `HandlerVersion`, `PolicySnapshot`, and the profile version are captured and integrity-bound; unavailable handler versions fail closed. |
| Replay artifact | `ReplayBundle` binds the canonical operation, captured inputs, embedded/snapshot/journal state reference, versions, expected plan, and bundle digest. |
| Isolated modes | `ReplayMode` covers verification, projection rebuild, simulation, historical debugging, and migration checks. `ReplaySandbox` accepts no database, network, filesystem, or worker handle and enforces byte limits. |
| Differential checks | `ReplaySandbox::compare` and `verify_corpus` compare semantic plan digests across handler versions and captured corpora. |
| Side-effect safety | Replay returns only the count of planned intents. `RecordingSideEffectSink` records intent without invoking an external provider. |
| Boundaries | `aequora-dev check` forbids replay dependencies on runtimes, HTTP/QUIC/server layers, and production database adapters; the database-neutrality gate remains complementary. |
| Operations | `aequora-dev replay explain/verify/inspect` provides bounded payload-free architecture and bundle diagnostics. |
| Telemetry | `ReplayEventKind` records verified bundles/decisions, divergence, version mismatch, and unsupported paths without payload content. |
| Invariants | `AEQ-INV-DET001` through `AEQ-INV-DET006` register plan equivalence, complete input capture, side-effect isolation, committed-input immutability, handler-version safety, and policy-version safety. |
| CI and tests | Replay unit/property/differential tests plus `replay_contracts` run explicitly in CI and within the workspace suite. |

## Authority integration sequence

1. Authenticate the envelope and resolve server-trusted principal and lineage.
2. Read one consistent canonical pre-state and capture every allowed external result.
3. Capture one clock value, retry-stable labelled ID/random sources, and immutable policy/config
   versions; retain the resulting input digest in the operation ledger.
4. Select the exact semantic handler version and call pure `decide`.
5. Verify the plan, then commit mutations, events, side-effect intents, result, and replay metadata
   in one native authority transaction.
6. Dispatch side-effect intents only after commit. A replay sandbox records intents and never
   invokes a provider.

The compatibility `aequora_executor::OperationHandler` accepts ambient asynchronous application
logic and does not claim full replayability by itself. A host earns the Part 12 guarantee only for
paths implemented through `ReplayHandler` and a compliant native plan commit.

## Security and privacy boundary

Replay artifacts can contain operation, state, random seed, and provider-result data. The portable
format supplies integrity and hard size bounds, not confidentiality. A deployment must authorize
bundle creation and access, encrypt artifacts at rest and in transit, redact or tokenize secrets,
apply per-tenant retention/deletion/legal-hold rules, audit access, and keep bundle payloads out of
logs and telemetry. High-security random material may be retained only as an approved encrypted
input or represented by an explicitly unsupported replay mode.

## Remaining deployment acceptance

A concrete authority still needs native transaction and restart evidence for its plan committer,
immutable historical state resolution, retained handler binaries or approved migrations, domain
codec stability, worker idempotency, access control, encryption/key rotation, retention/erasure,
large-corpus resource limits, and incident/migration differential runs. Those environment-specific
claims are release gates; an offline workspace test does not silently mark them passed.
