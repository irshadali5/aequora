# Custom database adapters

Aequora synchronizes typed operations and authoritative state transitions. It does not synchronize
SQL, table layouts, database pages, or write-ahead logs. Client and authority persistence are
separate choices and may be replaced independently.

## Client-side contract

A local adapter implements the capabilities combined by `aequora_store::LocalStore`:

- `OutboxStore` and `OutboxStateStore` for durable, ordered, replayable operations;
- `CursorStore` for the last fully reconciled scope position;
- `ReconciliationStore` for atomic authoritative changes, terminal outbox transitions, and cursor
  advancement;
- `ConflictInbox` for durable application-visible manual conflicts.

The application-specific optimistic entity mutation and outbox append must commit in one local
database transaction. Reconciliation must also be one transaction, with cursor advancement last.
The built-in Stoolap adapter demonstrates these rules but is not required by the client engine.

## Authority-side contract

An authority adapter implements the capabilities combined by `aequora_store::AuthoritativeStore`:

- `EntityReader` for tenant-bounded current state;
- `OperationLedger` for atomic version comparison, state mutation, journal append, audit append,
  and idempotency result;
- `ChangeJournal` for ordered, scoped incremental pulls;
- `SnapshotStore` for consistent, resumable bootstrap pages;
- `AuditLog` for immutable accountability evidence.

The server's atomic commit is the essential portability boundary. PostgreSQL and Neon are built-in
implementations through SQLx, but another SQL, document, key-value, or distributed database can
implement the same behavior without changing client code or the wire protocol.

## Independent composition

Use the narrowest dependencies for each binary:

```toml
# Client using the built-in local adapter.
aequora = { version = "0.1", features = ["stoolap", "http-client"] }

# Server using Neon or another PostgreSQL service.
aequora = { version = "0.1", features = ["postgres", "axum"] }
```

For custom databases, depend on `aequora-store` and the client or server crate directly, or use the
facade without database features. Do not implement a database-to-database bridge. Implement the
appropriate capability set on each side and compose it with any `SyncTransport` implementation.

The repository policy gate checks neutral, client-only, authority-only, and combined feature trees.
The live integration gate separately exercises the built-in Stoolap client through HTTP/Axum to a
real PostgreSQL server and, when credentials are configured, Neon pooled runtime and direct
migration endpoints.

## Reusable conformance tests

Adapter crates should add `aequora-testkit` as a development dependency and run the public
behavioral contracts against an isolated database:

```rust,ignore
use aequora_testkit::contracts::{verify_authoritative_store, verify_local_store};

// Client adapter: supply a unique operation, scope, and server timestamp.
verify_local_store(&local_store, operation, scope, server_time).await?;

// Authority adapter: supply a fresh initial CommitOperation fixture.
verify_authoritative_store(&authority_store, commit).await?;
```

The local contract verifies the durable replay state machine, exactly-once outbox visibility,
idempotent reconciliation, terminal cleanup, and cursor durability. The authority contract verifies
atomic initial commit, duplicate replay, durable idempotency result, entity state, exactly one
journal event, exactly one audit record, and consistent snapshot visibility.

The generic `LocalStore` boundary cannot create an application-specific optimistic entity mutation.
Each local adapter must additionally test that its native transaction commits the domain mutation
and outbox append together. The built-in Stoolap adapter includes that separate transaction test.
