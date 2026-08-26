# Part 03 Anti-Entropy and Self-Repair Completion Map

This record maps `03-anti-entropy-self-repair.md` to executable repository evidence. It does not
replace the normative architecture document.

## Canonical verification contracts

- `aequora-integrity` owns the explicit, domain-separated canonical entity writer, BLAKE3 digest,
  hash-schema version, integrity generation, deterministic partition assignment, partition roots,
  Merkle proofs, and cursor-bound manifests. Database pages, row order, pending overlays, caches,
  and the normal synchronization cursor are not hash inputs.
- Snapshot construction is bounded, includes empty partitions, rejects duplicate entities and
  scope/boundary mismatches, and sorts canonical identity before hashing. Serialized evidence can
  be checked with `IntegritySnapshot::verify_structure` before it is trusted.
- `Capability::IntegrityV1` advertises additive protocol support. `IntegrityRootRequest` and
  `IntegrityRootResponse` are transport-independent root-comparison DTOs; HTTP or QUIC hosts may
  expose them without changing digest semantics.

## Divergence and repair safety

- Root comparison first localizes mismatching deterministic partitions. Entity comparison then
  classifies missing, extra, version, tombstone, schema, and same-version payload divergence.
- Same-version payload or schema mismatches always produce `RepairStrategy::Quarantine`. Large
  divergence produces `BootstrapScope`; only a bounded, explicitly listed replica repair may use
  `ReplaceEntities`.
- The verification state machine requires catch-up, stable-boundary acquisition, localization,
  repair planning, atomic repair, and re-verification. Integrity metadata is stored independently
  from normal replication progress.
- `LocalIntegrityStore::repair_local_replica` is replica-only. Stoolap performs entity and
  application-projection replacement in one transaction while leaving the cursor and durable
  outbox untouched. The full-bootstrap strategy routes through the existing authorized snapshot
  installation path.

## Adapter and executable evidence

- `AuthoritativeIntegritySource`, `LocalIntegrityStore`, and `IntegrityCapabilityProvider` extend
  adapter certification without forcing unsupported databases to claim repair capability.
- PostgreSQL captures canonical evidence at the exact current scope head in a bounded,
  repeatable-read, read-only transaction. Stoolap captures only installed authoritative base rows
  at the exact local cursor and supports atomic bounded repair.
- `aequora-testkit::contracts::verify_integrity_pair` proves compatible adapters produce the same
  root. `verify_replica_repair` proves cursor and pending-envelope preservation.
- Stoolap tests compare database-derived roots with roots built directly from canonical entities,
  then corrupt and repair the replica. Integrity fault tests inject deletion, same-version payload
  mutation, and tombstone mutation; tampered serialized roots fail closed.
- `AEQ-INV-AE001` through `AEQ-INV-AE005` cover root agreement, authority safety, pending-intent
  preservation, same-version mismatch safety, and cursor isolation.

## Operations

- `IntegrityConfig` provides conservative routine-verification, partition, entity, automatic
  repair, and bootstrap thresholds with fail-closed validation.
- `MetricEvent::IntegrityVerification` records payload-free duration, partition counts, mismatch
  counts, repairs, quarantines, failures, and alert-worthy outcomes. Tracing warns on quarantine or
  failure without logging domain payloads.
- `aequora integrity status`, `aequora integrity verify <snapshot.ron>`, and
  `aequora integrity explain <repair-plan.ron>` provide bounded, payload-free operator diagnostics.
  Applying a repair remains the responsibility of an authenticated host administration boundary.

## Environment gate

Offline unit, model, adapter, Clippy, documentation, dependency-boundary, and database-neutrality
checks prove the reusable repository boundary. A live PostgreSQL/Neon differential capture still
requires `AEQUORA_TEST_POSTGRES_URL`; absence of credentials must be reported rather than treated
as live database validation.
