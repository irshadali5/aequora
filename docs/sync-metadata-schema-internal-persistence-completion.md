# Part 22 completion: sync metadata schema and internal persistence

The governing specification is
[`sys-arch/22-sync-metadata-schema-internal-persistence.md`](../sys-arch/22-sync-metadata-schema-internal-persistence.md).
The canonical implementation is the database- and runtime-neutral `aequora-metadata` crate.

## Executable scope

| Requirement | Evidence |
|---|---|
| Internal version and store root | `MetadataSchemaVersion`, `MetadataMigrationId`, `StoreId`, `MetadataRoot`, UTC `Timestamp`, and `MetadataSchemaRegistry` |
| Client metadata | Typed local-store, outbox/history, scope cursor/subscription/descriptor/membership, conflict, bootstrap/chunk, repair, integrity, scheduler, coordinator lease, purge, and key-reference records |
| Authority metadata | Typed authority/transition, operation ledger, journal, scope/device/watermark, snapshot/chunk/lease, audit/provenance/checkpoint, import/export/replay, governance, public-key/crypto/compatibility, regional, job, retention, recovery, and side-effect records |
| Stable persistent enums | `PersistentState::persistent_id` provides explicit stable numeric IDs instead of relying on Rust discriminants |
| Access and retention | `REQUIRED_INDEXES`, `ACCESS_PATTERNS`, and `RETENTION_MATRIX` define logical requirements independent of physical names |
| Optional schemas | `MetadataFeatures` and `MetadataStoreProfile` encode minimal, standard, full, and enterprise capability sets |
| Adapter contract | `AdapterMappingContract` validates physical mapping evidence and required logical-index coverage; `ADAPTER_CERTIFICATION_REQUIREMENTS` names the required behavioral suites |
| Transaction composition | Capability-specific repositories plus `LocalTransaction`, `AuthoritativeTransaction`, and unit-of-work traits preserve native transaction coupling without exposing SQL |
| Migrations | Checksummed monotonic journal, expand/backfill/switch/contract phases, fail-closed startup compatibility, exclusive-maintenance declaration, and pending-intent evidence |
| Recovery and verification | Typed persistence errors/retry classes, ambiguous commit outcome, startup status, crash injection points, verification findings, snapshot publication checks, and stale-fence rejection |
| Cross-database equivalence | Deterministic `MetadataExport` validates bounded sanitized canonical records and compares semantic state while ignoring adapter layout/name |
| Security boundary | Key records contain public material or secure-store references only; sanitized exports reject secret/private-key field names |
| Normative invariants | `AEQ-INV-META001` through `AEQ-INV-META009` are registered both in `aequora-metadata` and the workspace-wide `aequora-invariants` registry |

## Adapter responsibility boundary

The logical crate does not open databases and does not claim that an arbitrary adapter has indexes
or crash-safe transactions merely because it compiles. Each adapter maps only the profiles it
advertises and must supply the mapping and behavioral evidence named by the certification
requirements. PostgreSQL, Stoolap, SQLite, Redb, KV, schema-per-tenant, and database-per-tenant
layouts may differ physically but cannot weaken operation uniqueness, ordering, tenant scoping,
transaction groups, fencing, migration, or retention semantics.

Application business tables, object storage, private KMS/keystore custody, deployment backup/PITR,
and human database-role policy remain host responsibilities. They participate in certification and
recovery procedures but are intentionally not imported into the provider-neutral metadata crate.

## Verification

Focused acceptance uses:

```text
CARGO_BUILD_JOBS=1 cargo test -p aequora-metadata
CARGO_BUILD_JOBS=1 cargo clippy -p aequora-metadata --all-targets -- -D warnings
CARGO_BUILD_JOBS=1 cargo test -p aequora-invariants
bash scripts/check-metadata-architecture.sh
cargo run -q -p aequora-dev -- check
bash scripts/check-database-neutrality.sh
```

The contract tests cover schema validation, outbox payload identity, duplicate-ledger mismatch,
snapshot publication, stale fencing, migration intent preservation, required indexes,
cross-adapter export equivalence, and stable registration of all nine Part 22 invariants.
