# Part 05 Local Multi-Process Coordination Completion

This record maps `05-local-multiprocess-coordination.md` to repository-owned implementation and
verification. It covers the reusable library and built-in Stoolap adapter. It does not claim that
an application's packaging, filesystem permissions, suspend behavior, or production process
supervisor has passed a deployment acceptance drill.

## Implemented contracts

| Requirement | Repository evidence |
|---|---|
| Database-neutral model | `aequora-coordination` defines persistent `LocalStoreId`, ephemeral `ProcessInstanceId`, monotonic `FencingToken`, `LocalStoreGeneration`, lease kinds, process modes, grants, snapshots, and deterministic lease transitions without importing a database engine. |
| Adapter capability | `LocalCoordinationStore`, `AdapterCapabilities::LOCAL_COORDINATION`, and `TransactionGuarantees::LOCAL_COORDINATION_FENCING` expose atomic election/fencing independently of database brand. Conservative adapter defaults reject fenced leader work they cannot prove. |
| Durable Stoolap state | Migration 0004 creates singleton store-generation and coordinator-lease rows. Acquire uses a conditional transaction and advances the token; renew, release, validation, and maintenance generation changes compare owner, token, generation, kind, and expiry. |
| Leader-only commits | Reconciliation, snapshot staging/install, outbox state transitions, retry scheduling, compaction, rebase, and anti-entropy repair accept a fence that is validated inside the same adapter transaction as mutation. |
| Follower safety | Ordinary application domain mutation plus outbox append retains its existing atomic transaction and requires no coordinator lease. Repository reads and outbox appends therefore remain available to followers. |
| Runtime election | `SyncCoordinator::run_multi_process` performs bounded election polling and heartbeat renewal, exposes leader/follower/maintenance/no-leader state, cancels synchronization on renewal loss, releases gracefully, and permits takeover after expiry. Direct sync/bootstrap calls fail closed when multi-process coordination is active without a lease. |
| Process configuration | `LocalProcessMode` is exposed by the client builder. Strict RON configuration carries process mode, TTL, and heartbeat values and rejects zero heartbeat or a TTL not greater than two heartbeat periods. |
| Maintenance and generation | An exclusive maintenance lease advances the store generation atomically. Earlier grants then fail validation. Fenced snapshot installation plus explicit generation switching coordinates full replacement without changing `LocalStoreId`. |
| Compliance and invariants | `verify_local_coordination` is reusable by third-party adapters and covers exclusion, renewal, handoff, stale fencing, maintenance, and generation advancement. The invariant registry includes `AEQ-INV-LC001` through `LC006`. |
| Operations | `CoordinatorStatus`, payload-free leadership metrics/tracing, and `aequora-dev coordination explain/status` expose role, lifecycle counters, token/generation diagnostics, and leader-only boundaries without domain payloads. |

## Verification evidence

- The pure coordination model has unit and property tests for crash takeover, strictly increasing
  fencing tokens, stale-grant rejection, and generation invalidation.
- A Loom compare/exchange model explores simultaneous candidates and proves only one can commit the
  current epoch.
- The real Stoolap suite races two independent process candidates against one database and verifies
  exactly one acquisition. Its crash scenario proves takeover, follower outbox writes, rejection
  of stale outbox/reconciliation commits, maintenance exclusion, stable store identity, and
  generation invalidation.
- The reference adapter and Stoolap both pass the public coordination contract.
- Repository gates cover formatting, strict all-feature Clippy, workspace tests/rustdoc, Guppy
  dependency direction, and the independent database-neutrality policy.

## Deliberate host responsibilities

Hosts must ensure all processes open the same physical local database, use a monotonic-enough Unix
clock source for lease liveness, keep TTL comfortably above observed scheduling/suspend pauses, and
restrict maintenance jobs to the maintenance lease. Packaging must prevent an unsupported adapter
from being configured as multi-process and must surface prolonged no-leader, renewal-failure, and
stale-fence metrics. A deployment acceptance drill should kill the active process during network
sync, bootstrap, compaction, and repair, then verify bounded takeover and unchanged pending intent
on the actual filesystem and operating systems being shipped.
