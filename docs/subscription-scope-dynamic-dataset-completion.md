# Part 07 Subscription, Scope, and Dynamic Dataset Completion

This record maps `07-subscription-scope-dynamic-dataset.md` to repository-owned implementation and
verification. It covers canonical semantics, reusable contracts, the reference adapter, and the
built-in Stoolap local adapter. It does not claim that an application's roles, relationships,
projection policy, revocation feed, or shared-device privacy model has passed deployment acceptance.

## Implemented contracts

| Requirement | Repository evidence |
|---|---|
| Canonical identity | `aequora-scope` defines non-zero `ScopeDefinitionId`, opaque `SyncScopeId`, monotonic `ScopeVersion` and `ScopeGeneration`, retry-stable subscription/transition identities, bounded canonical partitions, and separately versioned projections. |
| Server authority | `ScopePrincipal`, bounded `ScopeRequest`, `ScopeResolver`, and duplicate-safe `ScopeRegistry` ensure the authenticated tenant and registered server policy produce `ResolvedScope`. Client parameters contain no SQL or executable predicate and never confer access. |
| Cursor semantics | `ScopeCursor` binds scope identity, version, generation, and sequence. Validation distinguishes changed scope and incompatible generation/retention. `project_filtered_page` advances an evaluated watermark across intentionally excluded journal entries while rejecting unordered or mixed-boundary input. |
| Membership and projection | Pure `MembershipEvaluator` and `ProjectionRule` boundaries separate deterministic dataset membership from field-level authorized projection. `ScopeResourceAuthorizer` covers scope-aware blobs, integrity artifacts, indexes, and other resources. Projection versions namespace overlapping views. |
| Subscription lifecycle | Requested, resolved, bootstrapping, active, expanding, contracting, suspended, revoked, and resync-required states are portable and independently durable per subscription. Multiple subscriptions retain separate identity, binding, cursor, transition, and recovery state. |
| Bootstrap and transitions | `ScopeBootstrapPlan` binds full/partial bootstrap to the exact target and boundary. `ScopeTransition` validates from/to identity, version, generation, tenant, additions, removals, and staging completion. Incomplete bootstrap/expansion changes lifecycle only; it never activates membership. |
| Contraction and removal | `ScopeRemoval` and its reasons are distinct from protocol/domain tombstones. Atomic contraction removes only the named membership and activates the new binding/cursor in one transition. Retention policy is explicit. |
| Shared entities | `LocalScopeState` tracks `(projection, entity) -> active scope set`; physical removal is reported only after the last active reference disappears. This prevents one scope contraction from deleting another scope's replica requirement. |
| Revocation and intent | Revocation logically deactivates membership immediately, clears the subscription cursor, and records an explicit disposition for affected pending operations. Both reference and Stoolap outbox drains exclude quarantined operations instead of transmitting or silently deleting them. |
| Durable adapter boundary | Optional `ScopeStateStore` preserves existing custom-adapter source compatibility while `AdapterCapabilities::SCOPE_STATE` advertises atomic scope support. Install, transition, fenced transition, and state load are database-neutral contracts. |
| Stoolap transactions | Migration 0005 persists the canonical control model and relational operation quarantine. Subscription install, membership/version/cursor activation, transition identity, and pending-intent disposition commit in one transaction; fenced application validates the current coordinator epoch inside that transaction. |
| Existing authority integration | Authoritative journal and snapshot contracts already take trusted tenant, scope, and server-resolved partitions. The initial safe rollout uses a full per-scope bootstrap and existing `ResyncRequired` directive when a richer transition is unavailable. `Capability::ScopeV1` reserves additive negotiation without changing the v1 HTTP/QUIC exchange body. |
| Part 03/05/06 integration | A validated scope cursor narrows to the legacy cursor used by bootstrap and anti-entropy while the caller retains the complete binding. Client scope transitions require local leadership. Scheduler `WorkKind::ScopeTransition` treats revocation/contraction as critical and expansion/bootstrap as bulk. |
| Operations | Payload-free scope metrics count resolutions, expansions, contractions, revocations, failures, affected membership, and active scopes. `aequora-dev scope explain/status` exposes the safety model and serialized control-state counts without entity or policy payloads. |
| Invariants | `AEQ-INV-SCP001` through `SCP007` register exact cursor binding, authorization, contraction ordering, removal/tombstone distinction, shared-reference safety, revoked-intent quarantine, and expansion activation atomicity. |

## Verification evidence

- Scope unit/property tests cover exact cursor binding, filtered watermark advancement, monotonic
  versions, interrupted expansion, idempotent transition retry, shared-scope contraction,
  revocation with pending intent, and portable state restart round-trips.
- `verify_scope_state_store` is reusable by third-party local adapters. It proves inactive staging,
  atomic activation, transition idempotency, revocation, retained pending-intent disposition, and
  exclusion of revoked operations from normal outbox drains.
- The reference local adapter and real Stoolap adapter pass the same compliance scenario. The
  client contract also proves local transition application does not invoke network transport.
- Stoolap's full suite covers migration drift, interrupted migration, restart durability,
  reconciliation/snapshot rollback, coordination fencing, repair, compaction, and the new scope
  transaction alongside existing guarantees.
- Repository gates cover formatting, strict all-feature Clippy, workspace tests/rustdoc, Guppy
  dependency direction, and the independent database-neutrality policy.

## Deliberate host responsibilities

Applications register real scope definitions and derive canonical membership only from current
authenticated server state. They must index membership predicates, reauthorize every read/write,
version policy/projections, identify every pending operation affected by revocation, and keep raw
local queries behind the active profile/scope gate. Hosts choose full versus certified partial
bootstrap, persist server transition metadata when required, and invoke search/blob/cache cleanup
from physical-removal results. Shared-device deployments should normally isolate tenant/user stores.
Deployment acceptance must exercise permission changes during requests, offline revocation,
relationship fan-out, interrupted expansion/contraction, mass scope changes, stale resolver caches,
and cleanup of actual databases, indexes, blobs, and UI repositories.
