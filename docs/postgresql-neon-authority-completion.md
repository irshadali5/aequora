# Part 37 PostgreSQL/Neon authority completion

The governing authority is
[`sys-arch/37-postgresql-neon-authoritative-adapter-detailed-architecture.md`](../sys-arch/37-postgresql-neon-authoritative-adapter-detailed-architecture.md).
This report records executable repository evidence and does not replace that design.

## Implemented scope

| Requirement | Executable evidence |
|---|---|
| Authority transaction | `PostgresCommitHook` joins application domain work to entity/version, transactional timeline allocation, journal, digest-bound ledger outcome, required audit, and durable side-effect intent insertion in one SQL transaction. Deterministic serialization/deadlock retries are bounded. |
| Ledger and duplicate races | Transaction-scoped operation locking and the `(tenant_id, operation_id)` primary key serialize races. The ledger stores canonical digest, operation kind/schema, handler version, outcome bytes, sequence, status, and creation time. Changed digest/kind fails permanently. |
| Journal and versioning | Tenant/scope timeline rows allocate cursor sequence transactionally, so aborts roll allocation back. Entity locks plus exact version transitions implement compare-and-swap. Reads are tenant/scope bounded. |
| Side effects | `PostgresCommitHookOutcome::with_side_effect_intents` inserts retry-stable jobs only inside Tx B; external provider calls are absent from the database transaction. |
| Migration and pool safety | Migration 0004 is checksummed and runs under the migration advisory lock. Typed pool, transaction, and journal configuration validates nonzero bounds; runtime connections apply statement, lock, and idle-in-transaction timeouts. Neon uses verified TLS plus separate pooled runtime/direct migration endpoints. |
| Device, retention, snapshot, and archive metadata | Monotonic device bindings and retention leases are persisted with explicit tenant/scope keys. Compaction is capped by active acknowledgements and requires a published bootstrap path. Snapshot publication and immutable archive-range metadata are represented without placing large attachments in journal rows. |
| Restore and readiness | `record_restore_epoch_transition` accepts only a strictly increasing `RestoredTimeline` transition. Structured readiness checks reachability, writer state, exact schema, critical timeouts, and initialized authority metadata. |
| Observability | Lock-free payload-free metrics cover pool wait, transaction/cursor latency, committed journal work, duplicate rates, and serialization/deadlock retries; safe snapshots can be exported by the deployment observability backend. |
| Profiles and invariants | `PostgresAuthorityFull` and `NeonOperationalProfile`, conformance tests 59–68, and `AEQ-INV-PG001`–`010` are registered in canonical RON and generated Rust/Markdown snapshots. |
| Structural gate | `scripts/check-postgres-authority-architecture.sh` verifies module ownership, DDL/contracts, neutral dependency boundaries, invariants/registry parity, focused tests, and registry integrity. CI runs the gate and the live PostgreSQL/optional Neon job. |

## Verification contract

The focused architecture gate is:

```bash
bash scripts/check-postgres-authority-architecture.sh
```

Repository completion also requires formatting, strict Clippy, Rust 1.87 all-target/all-feature
checking, warning-denied Rustdoc, locked/offline workspace tests, Guppy workspace architecture,
database-neutrality, registry verification, and diff hygiene.

## Completed verification

Verified on 2026-09-01:

- the focused Part 37 architecture gate, including its contract and registry checks;
- the locked/offline all-target, all-feature workspace test suite with loopback access enabled;
- strict workspace Clippy with warnings denied;
- Rust 1.87 workspace checking and warning-denied workspace Rustdoc;
- Guppy boundaries and workspace architecture (22 rules, 86 crates, 9 layers);
- all four database-neutral composition profiles; and
- formatting and generated registry integrity.

The first sandboxed workspace test attempt could not bind loopback ports. The permitted rerun passed;
this was an execution-environment restriction rather than a product failure.

## Environment-bound evidence

The checked-in live suite runs automatically against PostgreSQL 18 when
`AEQUORA_TEST_POSTGRES_URL` is present. It exercises migrations, Tx B behavior, duplicate and
payload-mismatch handling, snapshot boundaries, compaction, and rollback after an injected
mid-transaction SQL failure. Neon cold start, connection churn, branch rehearsal, and restore drills
run only when deployment URLs and operational infrastructure are supplied. No repository-only run
claims a Neon environment certification.
