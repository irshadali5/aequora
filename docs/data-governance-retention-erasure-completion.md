# Part 14 Data Governance, Retention, Legal Hold, and Erasure Completion

Part 14 is implemented at Aequora's database-neutral reusable boundary. Core plans and verifies
lifecycle transitions; applications choose legal policy and adapters execute destructive work.

## Repository guarantees

| Requirement | Evidence |
|---|---|
| Policy | Non-zero retention classes/versions, minimum/maximum windows, explicit deletion modes, legal-hold eligibility, and archive-before-delete rules validate fail closed. |
| Holds | `LegalHold` has tenant, selector, reason, actor, time, and immutable active/released metadata; active holds block normal purge. |
| Subject discovery | `DataSubjectResolver` and bounded `DataSubjectGraph` classify owned, referenced, shared, derived, and audit-only records without inferring ownership from foreign keys. |
| Erasure | Integrity-bound `ErasurePlan` records subject, policy, correlation, deterministic actions, irreversible boundaries, and explicit blockers. Required/shared evidence is minimized or retained rather than silently deleted. |
| Sync safety | Tombstone evaluation requires active clients beyond deletion, bootstrap-safe `JournalFloor`, no hold, and a retained identity guard. Retired clients below the floor must rebootstrap. |
| Ledger safety | `OperationLedgerPolicy` rejects retry horizons shorter than legitimate offline retry and exposes an ancient-operation cutoff. |
| Governed copies | Snapshots, blobs, exports, replay/import/repair artifacts, archives, and backups use opaque copy references bound to registered storage surfaces. |
| Offboarding/restore | Tenant lifecycle removes writes before purge; restore cannot serve until holds and all required erasures/revocations are reconciled. |
| Client purge | Stable `PurgeDirective`/`PurgeAck` model idempotent cleanup; failed sensitive purge has a restricted state. |
| Stores | Registry, capability, adapter, and verification contracts require every configured required surface to verify before completion. |
| Operations | Purge plans distinguish dry-run, approval, separation of duties, required surfaces, irreversible action digest, and a non-nil canonical administrative audit identity. CLI verification is read-only. |
| Observability | Payload-free governance counters and tracing cover candidates, purges, holds, blocked erasure/restore, partial completion, and verification without subject or storage identifiers. |
| Invariants/tests | `AEQ-INV-GOV001`–`GOV009`, properties, legal-hold/restore tests, partial-surface failure, offboarding fencing, and watermark tests run in CI. |

## Host and adapter responsibilities

Applications own legal durations, classification/subject relations, hold authorization,
pseudonym rules, approvals, and administrative audit declarations. Adapters own bounded idempotent
deletion, checkpoints, reachability checks, archives/objects, key destruction, local restricted
mode, backup reconciliation, and verification. Unknown copies and malicious offline devices cannot
be proven erased; completion covers registered surfaces and honest clients.

Part 15 supplies cryptographic mechanics. Part 23 supplies production durable governance workers.
