# Part 11 Operation Semantics and Consistency Profiles Completion

Part 11 is implemented at Aequora's database-neutral reusable boundary. The framework owns coherent
semantic declarations and validation; applications classify their domain, and adapters prove the
native guarantees required by the selected profiles.

## Repository guarantees

| Requirement | Evidence |
|---|---|
| Built-in profiles | `ConsistencyProfileKind` defines immutable append-only, optimistic versioned, commutative, explicit LWW, manual conflict, strong aggregate, server-only, device-local, and derived projection contracts. |
| Separate levels | `AggregateProfile` owns aggregate-wide conflict/version/delete/snapshot/scope/retry policy; `OperationSemanticProfile` owns operation class, ordering, compaction, rebase, origin, audit, and version. |
| Coherent policy | `ConsistencyProfile::validate` and registry registration reject invalid built-in/custom combinations before mutation. |
| Safe customization | `CustomProfileBuilder` requires explicit advanced opt-in and revalidates every override. |
| Capabilities | `ProfileRequirements`, `AdapterProfileCapabilities`, and `validate_capabilities` bind profile needs to shared transaction guarantees plus explicit adapter evidence. |
| Fail-closed registry | Duplicate IDs, unknown aggregates/operations, unsafe transitions, derived authority, sensitive compaction/rebase, and implicit unknown-operation LWW are rejected. |
| Ergonomics | `AequoraAggregate`, extended `AequoraOperation`, and aggregate/operation builders publish typed declarations through the facade. |
| Compatibility | Sorted `ProfileManifest` values have a portable BLAKE3 digest; successor verification rejects removal or semantic drift without a version increase. |
| Operations | `aequora-dev profile list/explain/verify/compare` provides read-only inspection and CI comparison entry points. |
| Telemetry | `ProfileEventKind` and `MetricEvent::Profile` expose payload-free registration, verification, capability rejection, and compatibility rejection counters/traces. |
| Invariants | `AEQ-INV-PROF001` through `AEQ-INV-PROF006` are registered with model, property, adapter, and diagnostic evidence names. |
| Compliance | Profile unit/property tests and `aequora-testkit::profiles::verify_profile_registry` validate application registries and adapter declarations together. |
| Sensitive domains | Finance accepts only required-audit append-only/strong profiles; workflow and security-sensitive aggregates reject LWW and require audit-safe noncompactable operations. |

## Compatibility workflow

Applications should generate and retain a profile manifest with each released domain schema. CI
compares the previous released manifest with the candidate:

```bash
cargo run -q -p aequora-dev -- profile verify profiles/current.ron
cargo run -q -p aequora-dev -- profile compare profiles/released.ron profiles/current.ron
```

Additive declarations are compatible. Removing a declaration or changing its semantics requires an
explicit migration strategy; a changed retained declaration must increase its profile version.

## Host and adapter responsibilities

A production integration must still classify every aggregate and operation, register all stable
wire kinds, persist the released manifest, and review version changes as protocol/schema changes.
The authority adapter must provide native compare-and-swap and aggregate transaction semantics when
claimed, durable idempotency/append behavior, consistent snapshots, authoritative clock policy, and
a durable manual-conflict inbox where required. Application tests must demonstrate domain-specific
equivalence, especially for finance, workflow, deletion, compensation, and external side effects.

Repository completion does not claim that a host application's domain classification is correct or
that an adapter capability exists merely because it was declared. Those remain release acceptance
evidence for the concrete application/adapter pair.
