# Part 42 SQLite local adapter completion

The governing authority is
[`sys-arch/42-sqlite-embedded-local-adapter-architecture.md`](../sys-arch/42-sqlite-embedded-local-adapter-architecture.md).
This report records executable repository evidence and does not replace that design.

## Implemented scope

| Requirement | Executable evidence |
|---|---|
| Official portable adapter | `aequora-store-sqlite` publishes neutral store and adapter-SDK manifests while containing every `rusqlite` type inside the physical-adapter layer. The facade exposes an independent `sqlite` feature. |
| Tx A | `SQLiteDatabase::transact_local_mutation` passes the same native transaction to application domain work and the immutable outbox insertion. A rollback/reopen test proves both effects commit or neither does. The mutable transaction supports native savepoints. |
| Tx C | `ReconciliationStore::reconcile` deduplicates and installs authoritative entities, invokes application projection work, persists ACK/rejection/conflict outcomes, and writes the cursor last in one immediate transaction. An injected projection failure proves the entity, outcome, and cursor roll back together. |
| Required durable schema | Strict tables implement `operation_outbox`, `sync_cursor`, `conflict_ledger`, `scope_cache`, `blob_manifest`, and `metadata`, plus authoritative-change deduplication, canonical entities, and resumable snapshot staging. Application domain tables live in the same database and join Tx A/Tx C through `SQLiteProjectionHook`. |
| WAL and concurrency | Production open enforces WAL, `synchronous=NORMAL`, foreign keys, and a five-second default busy timeout. One mutex-owned immediate writer serializes sync mutations; independent read-only connections use WAL snapshots and query-only mode. |
| Outbox and crash recovery | Operations are stored with Postcard bytes and a BLAKE3 digest, remain append-only through terminal lifecycle state, and retain identity across sending, retry, process restart, and duplicate reconciliation. |
| Snapshots and blobs | Snapshot pages stage durably and activate the complete generation with its cursor in one transaction. `SQLiteBlobManifest` stores bounded digest/size/availability/pin metadata while large bytes remain incrementally managed outside the database. |
| Backup and corruption | SQLite's online backup API captures the complete database, including application data and all sync metadata. Restore verifies integrity first. Health and corruption detection preserve damaged files for forensics instead of deleting them. |
| Platform placement and encryption boundary | `SQLitePlatform` distinguishes desktop, Android private-data, and iOS Application Support policies. The adapter accepts a durable path and does not own plaintext/encryption policy; encryption may remain below the same contract. |
| Conformance and invariants | `SQLiteLocalCore`, `SQLiteDesktopLocalFull`, and `SQLiteMobileLocalFull`, conformance tests 109–113, and `AEQ-INV-SQLITE001`–`005` are registered in canonical RON and generated Rust/Markdown snapshots. Target-specific full profiles remain claims only when their environment-bound evidence exists. |
| Structural gate | `scripts/check-sqlite-local-adapter-architecture.sh` verifies schema, pragmas, transaction/backup/recovery surfaces, neutral dependency isolation, invariants, registry parity, focused tests, and built-in manifest composition. CI executes the gate. |

## Verification contract

The focused gate is:

```bash
bash scripts/check-sqlite-local-adapter-architecture.sh
```

Repository completion also requires formatting, strict Clippy, Rust 1.87 all-target/all-feature
checking, warning-denied Rustdoc, locked/offline workspace tests, Guppy boundaries,
database-neutrality profiles, registry verification, and diff hygiene.

## Platform-bound evidence

The repository verifies real bundled SQLite behavior on the host: WAL configuration, strict schema,
Tx A/Tx C rollback and persistence, crash reopen, idempotent reconciliation, online backup, and the
public local-store contract. Windows, macOS, Android, and iOS full support remains separately bound
to a concrete binary, filesystem, OS version, and certification artifact; this repository-only run
does not fabricate those results.

## Completed verification

Verified on 2026-09-02 with `CARGO_BUILD_JOBS=1`:

- the focused Part 42 gate and seven real SQLite WAL, savepoint, Tx A, Tx C, crash-reopen,
  online-backup, forensic-copy, and public local-store contract tests;
- locked/offline all-feature workspace tests;
- strict workspace Clippy and all-target/all-feature checking;
- Rust 1.87 all-target/all-feature checking and warning-denied workspace Rustdoc;
- Guppy/workspace boundaries (22 rules, 91 crates, 9 layers, 12 isolated dependencies);
- six database-neutral composition profiles covering custom, Stoolap, SQLite, and PostgreSQL; and
- registry generation/verification, formatting, and diff hygiene.

The first sandboxed workspace test attempt could not bind the existing `aequora-http` loopback
listener. The permitted rerun passed; this was an execution-environment restriction rather than a
source failure.
