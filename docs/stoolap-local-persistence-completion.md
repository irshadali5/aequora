# Part 38 Stoolap local persistence completion

The governing authority is
[`sys-arch/38-stoolap-embedded-local-replica-client-persistence-architecture.md`](../sys-arch/38-stoolap-embedded-local-replica-client-persistence-architecture.md).
This report records executable repository evidence and does not replace that design.

## Implemented scope

| Requirement | Executable evidence |
|---|---|
| Tx A and domain bridge | `StoolapDatabase::transact_local_mutation` joins application-owned domain work, outbox insertion, canonical payload digest, and commit in one physical transaction. Fault tests prove rollback before commit. |
| Tx C and cursor safety | Reconciliation applies authoritative entities and application projections, deduplicates events, records ACK/rejection/conflict outcomes, validates authority identity/epoch, and advances the cursor in one transaction. Existing fault injection proves all effects roll back together. |
| Identity and clone safety | Migration 0007 persists distinct local-store/device identities, store and binding generations, schema/build metadata, and explicit compare-and-swap device rebinding. Restore policy fails closed when binding generation or secure-key state differs. |
| Durable outbox claims | `claim_operations` selects a bounded eligible batch atomically under a required coordinator fence; a separately named API requires an explicit single-process host guarantee. Claims record owner/time, mark `ever_sent`, and bind retries to the canonical BLAKE3 semantic digest. Stale claims return to retry without changing `OperationId` or payload. |
| Bootstrap, repair, scheduler, and blobs | Durable metadata tables cover bootstrap phases, snapshot staging verification, integrity checkpoints, repair progress, restart-safe scheduler policy, outbox dependencies, entity sidecars, and external blob accounting. Large collections remain bounded by caller limits and streaming metadata. |
| Fencing | Existing durable lease/store-generation support protects claim, reconciliation, snapshot, repair, compaction, rebase, and scope transitions. Stale coordinator tests exercise takeover and rejection. |
| Storage pressure and health | Typed storage classes and preflight decisions allow only reconstructable/evictable reclamation and surface `StorageCritical` when critical Tx A cannot fit. Structured health preserves damaged state and reports readiness rather than deleting it. |
| Migrations and recovery | Checksummed, contiguous migrations reject drift and newer unsupported formats. Open initializes identity and scheduler state, backfills old outbox digests, recovers staged work through existing durable APIs, and never performs destructive downgrade. |
| Adapter boundary and alternatives | Stoolap types remain in `aequora-store-stoolap`; neutral store, protocol, domain, and adapter SDK crates do not depend on the driver. Existing reference-store differential tests prove replaceable local semantics; SQLite remains an unclaimed reference alternative until implemented and certified. |
| Profiles and invariants | `StoolapLocalCore`, `StoolapDesktopLocalFull`, and `StoolapMobileLocalFull`, conformance tests 69–78, and `AEQ-INV-STOOLAP001`–`010` are registered in canonical RON and generated Rust/Markdown snapshots. |
| Structural gate | `scripts/check-stoolap-local-persistence-architecture.sh` verifies module ownership, DDL/contracts, neutral dependency boundaries, invariants/registry parity, focused tests, public adapter contracts, and registry integrity. CI runs the gate. |

## Verification contract

The focused architecture gate is:

```bash
bash scripts/check-stoolap-local-persistence-architecture.sh
```

Repository completion also requires formatting, strict Clippy, Rust 1.87 all-target/all-feature
checking, warning-denied Rustdoc, locked/offline workspace tests, Guppy workspace architecture,
database-neutrality, registry verification, and diff hygiene.

## Platform-bound evidence

The repository exercises real Stoolap behavior on the host, including persistence/reopen,
migrations, transaction rollback, reconciliation, snapshot activation, fencing, repair, bounded
claims, and reference-store differential outcomes. Linux, Windows, macOS, Android, and iOS support
claims remain separate environment-bound certification artifacts; this repository-only run does
not claim unexecuted target certification or platform secure-key integration.

## Completed verification

Verified on 2026-09-01 with `CARGO_BUILD_JOBS=1`:

- the focused Part 38 architecture gate and real Stoolap contract/fault/reopen tests;
- the locked/offline all-feature workspace test suite;
- strict workspace Clippy with warnings denied;
- Rust 1.87 all-target/all-feature checking and warning-denied workspace Rustdoc;
- Guppy boundaries and workspace architecture (22 rules, 86 crates, 9 layers);
- all four database-neutral composition profiles; and
- formatting, generated registry integrity, and diff hygiene.

The first sandboxed workspace test attempt could not bind the HTTP loopback listener. The permitted
rerun passed; this was an execution-environment restriction rather than a source failure.
