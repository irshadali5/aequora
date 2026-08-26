# Part 09 Bulk Import, Export, Seed, and Migration Completion

Part 09 is implemented at Aequora's database-neutral reusable boundary. The framework defines and
tests the safety contracts; applications supply semantic mappings and adapters execute the actual
source/target transactions.

## Repository guarantees

| Requirement | Evidence |
|---|---|
| Migration modes | `MigrationMode` distinguishes authority seed, operation history, replica snapshot, legacy bridge, and store migration |
| Durable jobs | `ImportJobId`, `ImportJob`, and the fail-closed `ImportJobState` transition graph |
| Resume identity | Immutable `SourceSystemId`, `SourceFingerprint`, mapping/policy versions, and lineage correlation |
| Deterministic IDs | `IdentityPlan` supports UUID preservation, deterministic namespace UUIDv8, and persisted mapping strategies |
| Two-pass mapping | Stable identity planning is separate from `SourceRecordMapper`, `CanonicalTransformer`, and `ImportRecordValidator` |
| Quarantine | Payload-minimized `QuarantineEntry`, stable error codes, lifecycle status, duplicate policy, and strict/tolerant threshold evaluation |
| Bounded transactions | `ImportBatch`, `ImportBatchLimits`, exact `ImportCheckpoint` succession, and `CheckpointedImportSink` atomic batch/ledger/checkpoint contract |
| Dependency ordering | Deterministic topological `plan_entity_order`; absent parents and cycles fail closed |
| Baseline authority | `BaselinePlan`, `SeedVersionPolicy`, and `AuthorityImportKind::SeedBaseline` avoid synthetic per-row history |
| Journal visibility | Imported operations and post-activation bridge changes explicitly require normal journal semantics |
| Snapshot baseline | Baseline sequence, canonical root, and required scope snapshots bind one consistent activation boundary |
| CDC and catch-up | `LegacyChangeSource`, bounded `LegacyChangePage`, and persisted `MigrationWatermark` |
| Cutover | Four `CutoverStrategy` values, complete `CutoverEvidence`, `verify_cutover`, and atomic `AuthorityImportSink::activate_cutover` |
| Split-brain prevention | Cutover fails until source lag is zero and the legacy writer is fenced |
| Export | Existing verified `CanonicalExport` plus `ExportBundleManifest` modes, cursor/scope boundary, exact root/count binding, and encryption policy |
| CLI | `aequora import plan`, `validate`, `status`, `cutover`, and `explain`; planning/validation execute zero writes |
| Scheduling and telemetry | `WorkKind::BulkMigration`, `MigrationCutover`, `MetricEvent::Import`, aggregate counters, and payload-free tracing |
| Invariants | `AEQ-INV-IMP001` through `AEQ-INV-IMP006` in the shared invariant registry |

## Executable evidence

`aequora-migration` tests cover canonical chunk/root verification, bounded source paging, resumable
artifact staging, legal job transitions, source mutation and policy drift, generated deterministic
identity stability, exact checkpoint progression, quarantine thresholds, dependency cycles,
baseline version policy, export-manifest binding, and the complete cutover gate.

The public `aequora-testkit::migration::FaultInjectingImportSink` and `migration_contracts` test
exercise rollback after staged data but before checkpoint, commit followed by response loss,
idempotent retry, exact source checksum/identity reuse, and durable duplicate-safe quarantine.

## Host and deployment responsibilities

A real migration still requires the host to:

- implement source adapters, semantic mappings, transformations, domain validation, natural-key
  collision policy, and scope routing for its data;
- implement `CheckpointedImportSink` or `AuthorityImportSink` so target rows, import ledger, and
  checkpoint share one native database transaction;
- keep source credentials outside job metadata and protect export/quarantine PII;
- choose seed versus imported-operation semantics per entity class and reconcile finance/domain
  totals in addition to canonical roots;
- produce scope snapshots at the baseline boundary and prove fresh-client bootstrap;
- implement and test actual CDC/outbox/polling catch-up when the legacy writer remains live;
- verify backups and rehearse rollback, then atomically fence the legacy writer before activating
  the new authority generation;
- run production-scale throughput, WAL/storage, lock, memory, and cutover acceptance.

The generic CLI intentionally does not mutate an authority without an application adapter. Its
cutover command verifies evidence and reports readiness; the host invokes the adapter's atomic
activation after its privileged operational approval.
