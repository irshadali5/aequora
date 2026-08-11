# ACID compliance evidence

This document maps the requirements in `ACID.md` to executable Aequora contracts and built-in
adapter behavior. It is an implementation record, not a replacement for the architecture.

## Compliance scope

Aequora guarantees local ACID plus durable intent, at-least-once delivery with a stable
`OperationId`, authoritative ACID, and local reconciliation ACID. It does not claim a distributed
transaction spanning an offline client and server.

The library owns three boundaries:

1. local domain mutation plus outbox insertion;
2. authoritative entity/version plus journal, operation result, and audit evidence;
3. local authoritative changes plus applied-event markers, terminal outbox state, conflicts, and
   cursor advancement.

Application finance tables and application-triggered external effects remain application-owned.
They comply only when the application places balanced ledger writes or a side-effect outbox in its
own domain transaction. Aequora does not claim finance compliance merely because sync metadata is
atomic.

## Explicit adapter declaration

`aequora-store` exposes `TransactionCapabilityProvider`, `TransactionCapabilities`,
`AcidComplianceLevel`, and `DurabilityMode`. A method-trait implementation alone is no longer
treated as evidence of production durability.

| Adapter | Declared level | Durable boundaries |
|---|---|---|
| `StoolapDatabase` / `StoolapStore` | `FullLocal` | local mutation + outbox; reconciliation + cursor; checksummed migrations |
| `SqlxPostgresBackend` / `PostgresStore` | `FullAuthoritative` | authoritative commit; concurrent idempotency; consistent snapshots; checksummed migrations |
| testkit in-memory stores | `Reference` / `Volatile` | deterministic semantic and fault testing only |

The shared adapter contracts reject internally inconsistent declarations and reject a local or
authoritative adapter that omits a required boundary flag.

## Architecture requirement map

| `ACID.md` area | Implementation evidence | Status |
|---|---|---|
| 1–9: ACID model and no distributed 2PC | `LocalStore`, `AuthoritativeStore`, `SyncTransport`, durable outbox replay, operation ledger, and reconciliation are independent boundaries | Implemented |
| 10–17: local mutation + outbox | `StoolapDatabase::transact_local_mutation` runs the application callback and encoded outbox insert in one native transaction; rollback and commit paths are real-engine tested | Implemented |
| 18–29: authoritative state + journal + ledger | `OperationLedger::commit_operation`; PostgreSQL and the reference store commit entity, version, scope sequence, journal event, operation result, and audit record together | Implemented |
| 30–39: client reconciliation + cursor | `ReconciliationStore::reconcile`; Stoolap writes entities/tombstones, applied markers, terminal results, conflicts, and cursor in one transaction, with cursor last | Implemented |
| 40–47: transaction ownership and failures | Adapter methods own transaction lifetime; error, cancellation, and dropped transaction paths cannot expose partial state; network/external work is outside these methods | Implemented |
| 48–63: consistency and isolation | tenant-aware keys, non-zero/version checks, scoped unique constraints, entity locks, exact version compare, and single-transaction final checks | Implemented |
| 64–80: idempotency and concurrency | tenant + operation unique ledger; operation advisory lock before entity lock; sequential and concurrent duplicate tests; exact-one journal/audit assertions | Implemented |
| 81–95: retries, batching, and dependencies | same `OperationId` is retained across transport retry; PostgreSQL retries the complete transaction up to three times only for SQLSTATE `40001` or `40P01`; dependency DAGs are validated before effects | Implemented |
| 96–113: conflicts, tombstones, snapshots, compaction | atomic manual conflict CAS, durable tombstones, repeatable-read PostgreSQL snapshot capture, staged atomic Stoolap install, watermark-bounded journal compaction | Implemented |
| 114–129: migrations, payload versions, and recovery | ordered checksummed migration ledgers; schema readiness gates; operation payload schema version; replayable `Sending`; durable retry deadline/attempt; persistent snapshot progress | Implemented |
| 130–145: durability and adapter conformance | explicit durability mode and compliance level; real persistent Stoolap restart proof; shared local/authority suites; live PostgreSQL/Neon gate | Implemented; live hosted runs remain environment-gated |
| 146–158: ACK, validation, and transaction duration | ACK means durable authoritative result; response loss reuses ledger result; structural/authorization/CPU work precedes commit; CAS-sensitive checks remain inside commit | Implemented |
| 159–165: database patterns and health | parameterized SQL, constraints, `FOR UPDATE`, advisory locking, repeatable-read snapshots, no-op write/read transaction diagnostics, explicit rollback, and migration-aware readiness | Implemented |
| 166–171: observability and repair | transaction commit/rollback/failure/dedup counters; payload-free tracing categories; inspection remains read-only; compaction cannot delete ledger/audit evidence | Implemented |
| 172–177: production checklist and final rules | capability declarations, shared contracts, real-engine rollback/restart tests, policy gates, and CI live-database branches | Implemented within the sync-core scope described above |

## Concurrency and invariant backstops

- `CommitOperation::has_valid_version_transition` requires creation at version 1 and every later
  mutation to advance exactly one. Both built-in authoritative stores reject invalid transitions
  before mutation, and the public contract verifies that rejection leaves no entity, ledger,
  journal, or audit residue.
- PostgreSQL obtains an advisory lock for `(tenant, operation_id)` before the entity advisory lock.
  The fixed operation-then-entity order serializes malformed replays and absent-row creation races
  without a lock-order cycle.
- The PostgreSQL primary/unique keys independently enforce tenant-scoped entity identity,
  operation identity, journal sequence, audit operation identity, and snapshot page identity.
- The shared authoritative contract runs two calls with one fresh operation concurrently and
  requires exactly one `Applied` plus one identical `Duplicate`. It separately races two fresh
  operations creating one entity and requires one `Applied`, one `VersionChanged`, and no loser
  ledger/journal/audit state.
- Stoolap conflict resolution uses a compare-and-set update (`resolved = 0`) and rejects a missing
  or already resolved record.

## Failure and recovery evidence

The reusable/reference suite and built-in adapter tests cover:

- every authoritative failpoint before/after entity, journal, ledger, audit, and final commit;
- commit succeeded but response was lost, followed by retry of the same operation ID;
- reconciliation replay of the same authoritative event;
- a Stoolap failure at final cursor conversion after entity/ACK/applied-marker work was staged,
  proving all staged reconciliation work rolls back;
- persistent Stoolap reopen while an outbox item is `Sending`, proving it remains replayable;
- persistent retry attempt/deadline metadata, due-only selection, and reopen before/after release;
- persistent reopen after reconciliation, proving ACK state, applied marker, entity, and cursor
  survive together;
- PostgreSQL mid-transaction failure after entity/sequence work is staged, with live-gate checks
  that entity, operation result, journal, and audit remain absent;
- consistent snapshot capture and staged, resumable local snapshot installation;
- version races, concurrent duplicate delivery, cursor regression, invalid cursor completeness,
  dependency cycles, tombstone replay, and safe compaction.

## Operational evidence

`SqlxPostgresBackend::health_check` verifies connectivity, exact migration history, transaction
start, no-op journal/ledger write permission, cursor metadata access, and explicit rollback without
changing domain rows. `StoolapDatabase::health_check` verifies schema history and transaction
access to outbox, applied-event, and cursor tables, then drops the uncommitted probe transaction.

`MetricEvent::ServerTransaction` and `MetricsSnapshot` expose payload-free totals for committed
operations, version-race rollbacks, persistence failures, and dedup hits. Existing client retry,
journal lag, readiness, deadline, overload, and lifecycle metrics cover the surrounding delivery
and recovery path.

## Deliberately unsupported semantics

- A request batch is not advertised as one transaction. Each operation is independently atomic.
  Dependency ordering does not imply group rollback.
- Cross-client/server two-phase commit is not implemented or claimed.
- Savepoints and caller-owned nested database transactions are not exposed through core traits.
- HTTP success is not the source of truth; the operation ledger is.
- Cache, notification, and wall-clock/HLC ordering are never authoritative commit evidence.
- Finance balancing, payment-provider idempotency, and email/webhook/job outboxes must be enforced
  by the application schema and application compliance tests when those features exist.

## Required verification

The ACID implementation is part of the normal release gates:

```text
cargo test --workspace --all-features --offline
cargo run -q -p aequora-dev --locked -- check
bash scripts/check-database-neutrality.sh
```

The PostgreSQL suite runs when `AEQUORA_TEST_POSTGRES_URL` is set. The same authoritative contract
runs against Neon when both pooled and direct Neon URLs are set. A skipped environment-gated run
must never be reported as a current live-database pass.
