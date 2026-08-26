# Part 13 Data Provenance, Auditability, and Explainability Completion

Part 13 is implemented at Aequora's database-neutral reusable boundary. The framework defines
canonical business evidence and verification rules; hosts own domain registries, authorization,
native persistence, privacy policy, external anchors, presentation, and regulatory acceptance.

## Repository guarantees

| Requirement | Evidence |
|---|---|
| Four histories | Crate and architecture documentation keep sync journal, operation ledger, canonical business audit, and operational logs separate by purpose and retention. |
| Canonical event | `AuditEvent` binds global identity, tenant, stable subject/action, truthful actor, authoritative time, correlation, outcome, category, durability, retention, changes, structured reason, provenance, and optional correction. |
| Stable semantics | `AuditActionId`, `AuditFieldId`, and `ReasonCode` are non-zero numeric registry keys; localized or mutable human strings are not canonical identities. |
| Truthful actor | `AuditActor` distinguishes user, service, system, and import work. Device is provenance rather than automatically the actor. |
| Value privacy | `AuditValuePolicy` and `AuditValue` distinguish full, redacted, digest, metadata-only, and omitted evidence. Unknown fields are denied by `AuditPolicy`; high-value categories reject best-effort durability. |
| Field history | `AuditChange` is bounded and field-stable; `FieldProvenance` points to the latest authoritative audit and domain event for selected fields. |
| Origin explanations | `AuditOrigin` structurally distinguishes imports, repairs, scope-only removal/revocation, and bootstrap rather than pretending each is a user business mutation. |
| Atomic declaration | `ExecutionPlan.audit_events` binds canonical audit declarations into the deterministic decision digest. `PlanCommitter` owns one native commit for mutations, events, required audit, side-effect intents, result, and ledger metadata. |
| Retry safety | `AuditEvent::derive_id` is stable for operation/action/ordinal. `InMemoryAuditRepository` returns a duplicate only for identical canonical content and rejects identity drift. |
| Append-only correction | Events are immutable; `corrects` can reference a prior event and self-correction is rejected. |
| Tamper evidence | `ChainedAuditRecord` creates a contiguous per-tenant/per-partition sequence over previous hash plus canonical event digest. `verify_chain` rejects boundary, sequence, previous-hash, content, and digest tampering. |
| External anchoring | `AuditCheckpoint` verifies a chain root; `AuditAnchorSink` leaves signing and immutable external storage credentials at the application edge. |
| Queries and authorization | `AuditQuery` requires an explicit time window, hard result limit, and optional stable page cursor alongside tenant, subject, actor, operation, correlation, action, and category filters. `AuditAccess` requires tenant equality and fail-closed subject restriction. |
| Export and lifecycle | `AuditExportManifest`, `AuditArchiveSink`, retention classes, and `AuditRetentionDecision` provide canonical export/archive hooks; a legal hold prevents purge eligibility. |
| Operations | `aequora-dev audit explain/verify/inspect` explains boundaries and validates a RON chain without printing subjects, actors, fields, reasons, or values. |
| Telemetry | `AuditEventKind` counts committed events, authorized/forbidden queries, integrity failures, and anchors without sensitive labels or payloads. |
| Invariants | `AEQ-INV-AUD001` through `AEQ-INV-AUD009` register atomicity, deduplication, attribution, immutability, privacy, authorization, chain, tamper, and field-provenance guarantees. |
| Tests and CI | Unit/property tests cover deterministic identity, sensitivity, authorization, chain tampering, checkpoint rules, and retention. `audit_contracts` covers response-loss retry and tenant isolation; both suites run explicitly in CI. |

## Native authority transaction

For each policy requiring `RequiredAtomic`, a concrete authority must commit the business mutation,
entity version, sync journal event, operation-ledger result, canonical audit events, audit chain
sequence/hash, and authoritative field-provenance pointers in one native transaction. Failure to
insert required audit evidence fails the business transaction. Duplicate `OperationId` processing
returns the existing logical audit effect and cannot append another event.

`DurableAsync` is reserved for projections reconstructible from retained canonical evidence.
`BestEffort` is operational telemetry and is rejected for business, security, and administrative
canonical events. External search/archive projection happens after the required canonical commit.

## Security and privacy boundary

Canonical integrity does not grant read authorization or confidentiality. A deployment must define
the action/field/reason registry, authorize every history/explanation/export request against current
entity access, select redaction or keyed digest policies that resist guessing, prevent secrets from
entering full values, encrypt hot/cold evidence, audit privileged audit access, and apply tenant
retention, pseudonymization, legal-hold, and erasure decisions. CLI and telemetry intentionally
expose only counts and chain metadata.

## Remaining deployment acceptance

A production adapter still needs native atomic/restart tests, unique constraints, chain locking or
partition fencing, indexes selected from real investigation workflows, archive/search rebuilds,
anchor signing and immutable-storage verification, key rotation, backup/restore chain checks,
large-tenant query/export controls, historical identity display policy, localization, and domain
review of field coverage. Part 14 owns the deeper lifecycle conflicts among retention, legal hold,
erasure, journals, tombstones, replay bundles, snapshots, imports, archives, and backups.
