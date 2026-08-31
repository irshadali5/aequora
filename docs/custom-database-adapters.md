# Custom database adapters

Aequora synchronizes typed operations and authoritative state transitions. It does not synchronize
SQL, table layouts, database pages, or write-ahead logs. Client and authority persistence are
separate choices and may be replaced independently.

New adapters should use the stable Part 36 contracts and conformance flow documented in
[`storage-adapter-sdk.md`](storage-adapter-sdk.md). The older `aequora-store` traits below remain the
current engine integration surface; Part 36 adds the public role transaction, capability manifest,
environment binding, migration, error normalization, and certification contracts without changing
the database-neutral core direction.

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

Dynamic-dataset deployments additionally implement the optional `ScopeStateStore` and advertise
`AdapterCapabilities::SCOPE_STATE`. Subscription binding, membership additions/removals, scope
cursor/version/generation, transition identity, and pending-operation quarantine must commit
atomically. A normal outbox drain must exclude quarantined operations. This capability remains
separate from `LocalStore` so existing custom adapters retain source compatibility until they opt
into Part 07 scopes.

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
use aequora_testkit::contracts::{
    verify_authoritative_store, verify_local_store, verify_scope_state_store,
};

// Client adapter: supply a unique operation, scope, and server timestamp.
verify_local_store(&local_store, operation, scope, server_time).await?;

// Authority adapter: supply a fresh initial CommitOperation fixture.
verify_authoritative_store(&authority_store, commit).await?;

// Dynamic-scope adapter: use one isolated subscription/activation/operation fixture.
verify_scope_state_store(&local_store, subscription, activation, operation).await?;
```

The local contract verifies the durable replay state machine, exactly-once outbox visibility,
idempotent reconciliation, terminal cleanup, and cursor durability. The authority contract verifies
atomic initial commit, duplicate replay, durable idempotency result, entity state, exactly one
journal event, exactly one audit record, and consistent snapshot visibility.
The scope contract verifies inactive staging, atomic activation, idempotent retry, logical
revocation, retained intent disposition, and exclusion of revoked operations from transmission.

The generic `LocalStore` boundary cannot create an application-specific optimistic entity mutation.
Each local adapter must additionally test that its native transaction commits the domain mutation
and outbox append together. The built-in Stoolap adapter includes that separate transaction test.

## Consistency-profile capability evidence

Convert the adapter's shared `TransactionCapabilities` with
`AdapterProfileCapabilities::from_transactions`, then add only independently proven capabilities
such as an authoritative clock or durable conflict inbox. Before accepting application traffic,
call `ProfileRegistry::validate_capabilities` or the combined
`aequora_testkit::profiles::verify_profile_registry` gate. A capability declaration is evidence
metadata, not an implementation: compare-and-swap, atomic aggregate commit, durable idempotency,
consistent snapshot, and append durability still require native transaction/restart tests.

Retain the application's checksummed `ProfileManifest` beside schema/protocol release metadata.
Adapter CI should verify the current manifest and compare it with the last supported release so a
semantic change cannot silently exceed the adapter guarantees.

## Deterministic plan commit evidence

Full replay support uses `aequora_replay::ReplayHandler`; the older ambient `OperationHandler`
boundary is explicitly outside that guarantee. Capture authenticated principal, time, ID/random
seeds, external results, and policy/config versions before calling `decide`. Then implement
`PlanCommitter` as one native authority transaction that validates the input and plan digests and
atomically persists mutations, events, durable side-effect intents, the operation result, handler
version, and replay metadata. A duplicate operation may return the recorded outcome only when its
captured-input and plan identities match; drift must fail closed.

External SDK calls never run inside that transaction or a replay sandbox. A worker claims the
committed `SideEffectIntent` after commit and uses its durable idempotency key. Adapter tests should
inject failure before commit and response loss after commit, reopen the store, and prove that retry
returns the same decision without duplicating an event or side effect. Historical snapshot or
journal references also need immutable digest verification and deployment-specific retention.

## Canonical business-audit evidence

The existing `AuditRecord` contract is minimal payload-free command accountability. Part 13's
`aequora_audit::AuditEvent` is richer canonical business evidence and remains distinct from the sync
journal, operation ledger, and logs. When policy is `RequiredAtomic`, persist every
`ExecutionPlan.audit_events` item, its tenant-partition sequence and hashes, and any authoritative
`FieldProvenance` pointers in the same native transaction as the mutation, journal, ledger result,
and durable side-effect intents. A retry-stable event identity must return identical retained
content or fail on drift.

Prove failure before commit leaves no state or audit row, response loss after commit returns one
logical audit effect, required-audit insertion failure aborts the business mutation, correction
appends instead of rewriting, field pointers cannot lead or lag their mutation, chain tampering is
visible, and every query is tenant-bounded and authorized. Search/archive projections may be
asynchronous only when canonical evidence remains durable and rebuildable. External anchors,
encryption, redaction/keyed digest policy, indexes, retention, legal holds, and erasure remain
host-owned.

## Governance storage surfaces

Register every authoritative, local, derived, exported, archived, and backup location that may hold
governed data in `GovernanceRegistry`; an unregistered copy is outside Aequora's completion claim.
Implement `GovernanceStore` with opaque object references and bounded, idempotent checkpoints.
Planning must be read-only. Execution accepts only a structurally verified, non-dry-run
`PurgePlan` with independent approval and a non-nil canonical administrative audit identity.

Apply active legal holds before destructive work, report each required surface as verified,
failed, or unreachable, and never turn partial evidence into completion. Tombstone collection must
also satisfy the journal-floor, active-device watermark, bootstrap, and retained-identity checks.
Restored tenants remain unable to serve until required purge directives and surface reconciliation
are complete. Adapter acceptance should inject crashes between bounded batches, reopen storage,
resume from the checkpoint, and prove that retries neither resurrect data nor broaden tenant scope.

## Optional live broker adapters

Live fan-out is independent from both `LocalStore` and `AuthoritativeStore`. Implement
`aequora_live::HintBroker` only when a deployment needs cross-node wakeups. Delivery may be lossy,
duplicated, or reordered, but must be tenant-isolated and bounded. Call it only after the durable
authority transaction commits and treat publication errors as degraded latency. The built-in
`PostgresNotifyHintBroker` demonstrates `LISTEN/NOTIFY`; a Redis or NATS adapter can replace it
without changing sync protocol or storage contracts.

## Bulk migration adapters

Record migration is independent from steady-state `LocalStore` and `AuthoritativeStore`. A source
implements `CanonicalRecordSource` or `LegacyChangeSource`. A target implements
`CanonicalRecordSink`, `CheckpointedImportSink`, or `AuthorityImportSink` according to the workflow.
For every target batch, imported rows, stable source-key/checksum ledger entries, and the next
checkpoint must share one native transaction. Repeating the exact job/root/batch is idempotent;
source fingerprint, mapping, checksum, or checkpoint drift fails permanently.

Authority adapters must distinguish pre-live baseline seeding from journal-visible imported
operations and bridge changes. Baseline publication atomically binds its sequence, canonical root,
and scope snapshots. Cutover activation atomically fences the old writer and activates the new
authority generation only after `verify_cutover` succeeds.

## Large snapshot adapters

Large bootstrap is independent from steady-state snapshot paging. An authority adapter implements
`SnapshotSource`/`SnapshotReadView` for one consistent scoped boundary. A service, HTTP range,
object-store, or CDN adapter implements `SnapshotChunkSource`; opaque object references may be
refreshed, but stable chunk identity and digest must not depend on a temporary signed URL.

A local adapter implements `SnapshotSink`. Each verified chunk and its installed-progress marker
share one bounded transaction in an inactive `ReplicaGeneration`. `activate` is a separate atomic
transaction that rechecks the verified token, switches the active generation, sets the exact
snapshot cursor, and preserves the pending-intent commitment. Crashes before activation leave the
old generation active; retries after a committed-but-lost response return the same outcome.

Implement `SnapshotLeaseStore` when snapshot transfer can outlive ordinary journal retention.
Tier-A adapters declare generation swap, resumable chunk install, consistent reads, and pending
intent preservation through `SnapshotInstallCapability`, then pass the testkit large-bootstrap
failpoint contract plus engine-specific restart and capacity acceptance.
