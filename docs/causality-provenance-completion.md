# Part 02 Causality and Provenance Completion Map

This record maps `02-causality-provenance-lineage.md` to executable repository evidence. It does
not replace the normative architecture document.

## Stable contracts

- `aequora-types` owns `CorrelationId`, `EventId`, `JobId`, `LineageRef`, and `LineageContext`.
- `OperationMetadata` carries retry-stable originating lineage. `OperationAck` and `RemoteChange`
  carry the authoritative event identity and event lineage.
- Dependencies remain an independent DAG planned by `aequora-executor::plan_dependencies`; they
  never imply semantic causation.
- `AuthenticatedOperation::provenance` is the only typed upgrade to `TrustedProvenance`.
  Primary/derived event and job metadata preserve the root correlation while changing only the
  direct cause.
- `JobProvenance` must be stored atomically with any future durable job payload. Part 23 owns the
  job scheduler/outbox runtime; it must consume this contract rather than invent lineage fields.

## Persistence and diagnostics

- PostgreSQL schema version 2 stores event identity, correlation, and direct cause in the journal,
  operation ledger, and immutable audit log. Legacy version-1 rows are backfilled deterministically.
- The reference authority retains the same information in its atomic state transition.
- A repeated `OperationId` with different originating lineage is a permanent error; an unchanged
  retry returns the original event identity and lineage.
- `CorrelationLog::read_correlation` is tenant-bounded, offset-paginated administrative evidence.
  It exposes payload-free audit records and cannot query across tenants.
- The Stoolap outbox round-trips `OperationMetadata` losslessly. Historical envelopes without
  lineage use a sentinel during decoding and deterministically derive correlation from their stable
  `OperationId` at the trusted provenance boundary. Authoritative peers require `LineageV1` for the
  extended acknowledgement and journal format.

## Executable evidence

- The reusable local adapter contract compares the entire replayed operation, including lineage.
- The authoritative adapter contract checks event/ledger/journal/audit lineage equality, rejects
  lineage-changing retries, and proves tenant isolation for correlation lookup.
- `AEQ-INV-C001` through `AEQ-INV-C006` extend the Part 01 invariant registry.
- Model schema version 3 tracks correlation and event identity, checks durable correlation, and
  rejects a retry that attempts to replace the recorded correlation.
- Executor tests prove primary events, derived events, and job metadata inherit correlation from
  authenticated provenance.

## Environment gate

Offline unit, model, adapter, Clippy, documentation, dependency-boundary, and database-neutrality
checks can prove the reusable repository boundary. A live PostgreSQL/Neon contract still requires
`AEQUORA_TEST_POSTGRES_URL`; absence of credentials must be reported rather than treated as live
validation.
