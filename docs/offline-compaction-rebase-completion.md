# Part 04 Offline Compaction and Rebase Completion

This record maps `04-offline-compaction-rebase.md` to repository-owned implementation and
verification. It claims the reusable library and built-in local-adapter boundary, not an
application's domain-specific merge policy or a production deployment acceptance drill.

## Implemented contracts

| Requirement | Repository evidence |
|---|---|
| Policies and safe defaults | `aequora-queue` defines `CompactionPolicy`, `RebasePolicy`, `OperationShape`, risk classification, barriers, field access, and a fail-closed registry. Unknown, create/delete, merge/custom, append-only, financial, and audit-sensitive intent is preserved unless explicitly certified. |
| Payload-aware extensions | `OperationCompactor` and `OperationRebaser` expose application-certified `Merge`/`Custom` decisions without placing domain payload knowledge in the database adapter. |
| Mutable-unsent boundary | `MutationMutability` distinguishes never-sent, possibly-delivered, and finalized rows. Stoolap migration 0003 durably records `ever_sent` and `immutable_hash`; selecting an operation for send or retry atomically seals its semantic envelope. |
| Deterministic planner | `plan_compaction` uses stable local sequence, lineage, schema, entity, class, field group, policy version, barriers, cancellation evidence, and dependency edges. It rejects duplicate identities, duplicate sequences, removed required dependencies, dependency cycles, changed immutable payloads, and oversized passes. |
| Durable supersession | `CompactionPlan` emits payload-free `Supersession` records. Stoolap writes those records and removes only revalidated mutable-unsent rows in one transaction. |
| Rebase | `plan_rebase` supports explicit reapply-intent and non-overlapping field-aware policies while preserving operation identity. Possibly-delivered operations are skipped and unsafe cases remain normal conflicts. |
| Adapter contract | `OutboxStore` exposes bounded compaction and rebase hooks; `AdapterCapabilities::QUEUE_OPTIMIZATION` and `TransactionGuarantees::QUEUE_REWRITE` advertise support. Default methods are conservative no-ops for source-compatible custom adapters. |
| Client/configuration | The client builder can opt into a registry and bounded pass before batch construction. `OutboxOptimizationConfig` supplies validated pressure thresholds and maximum pass size without coupling policy to a database engine. |
| Diagnostics | `aequora queue status`, `verify`, `compact`, and `explain` operate on bounded RON artifacts; compact is a dry run. Observability reports duration, rows removed, bytes saved, rebase successes/conflicts, and failures without payloads. |
| Invariants | The registry includes `OQ-001` through `OQ-006`: semantic equivalence, delivered-payload immutability, dependency safety, intent preservation, rebase identity, and sensitive-operation safety. |

## Verification evidence

- Queue unit/property tests prove latest-overwrite equivalence, explicit barriers, dependency
  preservation and cycle rejection, immutable-payload detection, append/finance fail-closed
  behavior, and preservation of the final declared overwrite across generated histories.
- Stoolap tests exercise real transactions for compaction and rebase. A forced supersession-key
  failure occurs after rewrite work starts and proves rollback restores every original queue row.
- The reference adapter implements the same planner contract. Anti-entropy contract coverage
  repairs authoritative state and then rebases pending local intent, proving the integration point.
- Repository gates include formatting, strict all-feature Clippy, all-target workspace tests,
  rustdoc, Guppy dependency direction, and database-neutrality policy.

## Deliberate host responsibilities

Applications must register semantic policy for their own operation kinds and provide payload-aware
`Merge` or `Custom` implementations where needed. They must certify sensitive compaction only with
domain tests demonstrating business and audit equivalence. Production operators choose queue
thresholds, alert on compaction/rebase failures, and validate storage pressure and crash recovery
on the deployed local database/filesystem combination.

The generic library never guesses how to merge create/update/delete payloads. That boundary keeps
Aequora database-neutral and prevents a storage optimization from silently changing business
meaning.
