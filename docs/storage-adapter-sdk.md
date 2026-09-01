# Storage adapter SDK

`aequora-adapter-sdk` is the stable database-neutral extension boundary defined by
[Part 36](../sys-arch/36-storage-adapter-sdk-official-adapter-architecture.md). An adapter implements
semantic roles; it does not expose SQL, a driver transaction, or a physical row model to Aequora
core.

## Choose only the roles you implement

- Local databases implement `LocalTransactionStore`, `OutboxStore`, `CursorStore`, and
  `MigrationStore`. The application mutation and matching `OutboxRecord` must use the same concrete
  transaction.
- Authorities implement `AuthorityTransactionStore`, `JournalStore`, `OperationLedgerStore`, and
  `MigrationStore`. Business state, entity version, journal, ledger, and required audit evidence
  commit together.
- Artifact providers implement `SnapshotStore` or `ObjectStore`; they do not become an authority.
- Coordinated writers implement `FencingStore` and reject stale tokens before protected writes.

The associated transaction type remains inside the adapter/application crate. Application
repositories can borrow it through `DomainRepositoryFactory`, so application tables and Aequora
metadata share the physical transaction without teaching core about that database.

## Publish a truthful manifest

Implement `AdapterCapabilities` and return an immutable `AdapterManifest` containing a registry
assigned `AdapterId`, independent adapter version, roles, versioned `CapabilityId` claims, engine
versions, targets, concurrency model, support label, and known limitations. A compile-time marker or
manifest is only a claim.

At startup, construct `AdapterRequirements` from deployment policy and call `verify` with a
`CertifiedEnvironment`. Verification fails closed unless all of these match:

- adapter identity and exact adapter version;
- physical engine/deployment version;
- compilation target and relevant feature/configuration fingerprint;
- minimum conformance suite version;
- every required role and verified capability;
- minimum ecosystem support level.

Never synthesize evidence from the manifest itself. The certification artifact is produced from
tests against the real database, filesystem, platform, and critical settings.

## Normalize failures

Return `AdapterError` with an `AdapterErrorKind` and `RetryDisposition`. Physical codes such as a
PostgreSQL SQLSTATE may be retained in bounded `AdapterDiagnostic` context, but callers never match
on a driver error type. `CommitOutcomeUnknown` uses `ResolveCommitOutcome`; the idempotency layer
queries `OperationLedgerStore` before retrying. Do not automatically rerun arbitrary domain code
after a serialization failure.

## Run conformance

Test code implements the factory and probe traits in `aequora_adapter_sdk::conformance`, then calls:

```rust,ignore
let local = run_local_store_conformance(&local_factory).await?;
let authority = run_authority_store_conformance(&authority_factory).await?;
let snapshots = run_snapshot_conformance(&snapshot_factory).await?;
let fencing = run_fencing_conformance(&fencing_factory).await?;
```

Factories create isolated physical stores and may enable test-only failpoints. Runners cover local
atomicity/reopen/cursor/migration/durability, authority atomicity/idempotency/payload reuse/retry/CAS
/retention, snapshot partial-write/resume/corruption/activation, and fencing takeover/stale-writer
/token monotonicity. Combine these observations with structural, environment-binding, and support
matrix observations under `LocalAdapter`, `AuthoritativeAdapter`, `SnapshotAdapter`, or
`FencingAdapter` and seal them with `aequora-conformance`.

## Official and community policy

Official adapters require named maintainer ownership, migration and fault coverage, a performance
baseline, documentation/support matrix, and required conformance on each relevant change and
release. A database upgrade or correctness-relevant configuration change creates a new certified
environment. Community adapters use the same SDK and suites but must not imply official status.

Current repository manifests are:

| Adapter | Roles | Engine baseline | Concurrency | Important limitation |
|---|---|---|---|---|
| `aequora-store-postgres` (`aequora-postgres` architecture role) | authority, journal, ledger, snapshot, integrity, audit, side-effect intents, device/retention metadata | PostgreSQL 18 | multi-writer | `PostgresAuthorityFull`; Neon adds separate operational certification |
| `aequora-store-stoolap` (`aequora-stoolap` architecture role) | local, snapshot, integrity, repair, backup metadata, fencing | Stoolap 0.4.0 | single writer, multi-process | `StoolapLocalCore`; each `StoolapDesktopLocalFull`/`StoolapMobileLocalFull` target needs its own artifact |
| `aequora-store-sqlite` (`aequora-sqlite` architecture role) | local, snapshot, WAL persistence, online backup, incremental blob metadata | SQLite 3.46 bundled | single writer, WAL readers | `SQLiteLocalCore`; each `SQLiteDesktopLocalFull`/`SQLiteMobileLocalFull` target needs its own artifact |

Standalone object-storage adapters are not implemented by this repository today and must not be
advertised until a concrete crate publishes a manifest and passes the matching suite.

## Adapter documentation checklist

Document roles, capabilities, engine/Aequora/platform versions, transaction guarantees, required
indexes and critical settings, migration and backup strategy, concurrency/multi-process behavior,
observability, known limitations, and exact conformance artifacts. Prepared statements are required
for SQL values; dynamic identifiers come only from controlled schema definitions. Stored lengths
and decoded payloads remain bounded and are treated as untrusted.
