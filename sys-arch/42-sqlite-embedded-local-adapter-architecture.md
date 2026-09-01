# Aequora Sync — Part 42

# SQLite Embedded Local Replica Adapter Architecture

## Purpose

SQLite becomes Aequora's official portable embedded adapter for desktop, Android, and iOS. It is not the authority database; it is a durable local replica that implements the same adapter contracts as Stoolap.

## Design goals

- ACID local transactions
- WAL-based concurrent reads
- Crash-safe outbox and cursor storage
- Mobile and desktop portability
- Identical sync semantics across platforms

## Position in the ecosystem

Server:
- PostgreSQL / Neon → authoritative

Client:
- SQLite → official portable adapter
- Stoolap → high-performance adapter implementing the same contracts

## Layering

UI
↓
Aequora Client
↓
SQLite Adapter
↓
SQLite WAL Database

## Required tables

- domain tables
- operation_outbox
- sync_cursor
- conflict_ledger
- scope_cache
- blob_manifest
- metadata

## Transaction model

Tx A:
1. validate
2. update domain rows
3. append outbox
4. commit

Tx C:
1. apply authoritative changes
2. advance cursor
3. resolve conflicts
4. commit

No cursor advances without durable reconciliation.

## WAL configuration

Recommended:

```sql
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;
PRAGMA busy_timeout=5000;
```

WAL allows many readers while one writer performs sync.

## Concurrency

Single writer.
Many readers.
Background sync never blocks UI reads.

## Android storage

Store inside the application's private data directory.
Do not rely on temporary external storage for durable state.

## iOS storage

Keep the SQLite database inside the Application Support directory and exclude caches from durable replica state.

## Desktop storage

```
Windows  %LOCALAPPDATA%
Linux    ~/.local/share
macOS    ~/Library/Application Support
```

## Backup

SQLite backups must preserve:
- domain data
- outbox
- cursor
- conflicts
- metadata

Never export only business tables.

## Corruption recovery

1. Detect corruption.
2. Preserve forensic copy.
3. Restore latest valid snapshot.
4. Replay journal from authority.

## Encryption boundary

SQLite encryption is optional and belongs below the adapter.
Aequora never depends on plaintext storage.

## Adapter capabilities

Required:
- transactions
- savepoints
- WAL
- foreign keys
- prepared statements
- incremental blob support

## Conformance

SQLite must pass every Aequora adapter test:
- Tx A durability
- Tx C reconciliation
- crash recovery
- idempotent replay
- conflict persistence
- snapshot restore

## Invariants

- **AEQ-INV-SQLITE001** — WAL is mandatory for production.
- **AEQ-INV-SQLITE002** — One logical writer owns the replica.
- **AEQ-INV-SQLITE003** — Cursor changes occur only inside Tx C.
- **AEQ-INV-SQLITE004** — Outbox entries are append-only until lifecycle completion.
- **AEQ-INV-SQLITE005** — SQLite and Stoolap expose identical adapter behavior to higher layers.

SQLite is therefore the stable reference implementation for every future embedded database adapter in Aequora.
