# Aequora Sync — Part 22

# Sync Metadata Schema and Internal Persistence Specification

## 1. Purpose

Across Parts 01–21, Aequora introduced many durable concepts:

```text
client outbox
server operation ledger
authoritative journal
scope manifests
scope cursors
authority epochs
snapshot jobs
bootstrap jobs
audit events
repair metadata
anti-entropy digests
compatibility state
governance jobs
legal holds
key registries
regional watermarks
```

Those concepts are correct individually, but a production implementation needs one coherent persistence specification.

Without that, every adapter may invent its own semantics for:

```text
uniqueness
indexes
retention
transaction boundaries
state transitions
foreign-key behavior
recovery
```

That would undermine database interoperability.

The central rule is:

> **Aequora defines a canonical logical persistence model; database adapters map that model into their native storage without changing its semantics.**

This document specifies that logical model.

---

# 2. Goals

The persistence specification should provide:

```text
DB-agnostic logical records
stable metadata identities
required uniqueness constraints
required indexes
transaction-group rules
retention semantics
migration/versioning rules
adapter capability requirements
client/server separation
reference schemas
recovery rules
```

---

# 3. Non-Goals

This is not:

```text
a universal SQL schema
a requirement that all DBs use tables
a mandate for PostgreSQL-specific types
an ORM model
```

A KV database may map logical records to keyspaces.

A relational database may use tables.

A document database may use collections.

Semantics must remain equivalent.

---

# 4. Persistence Domains

Separate at least:

```text
Client Local Metadata
Server Authoritative Metadata
Shared Logical Metadata
Operational/Derived Metadata
```

---

# 5. Client Local Metadata

Includes:

```text
outbox
scope cursors
local operation state
bootstrap state
repair state
scheduler state
local leader/lease
pending conflicts
local store metadata
```

---

# 6. Server Authoritative Metadata

Includes:

```text
operation ledger
journal
authority state
scope registry
snapshot catalog
audit events
legal holds
compatibility policy
governance state
```

---

# 7. Shared Logical Metadata

Same conceptual types may exist on both sides:

```text
AuthorityId
AuthorityEpoch
ScopeId
ScopeGeneration
OperationId
EntityRef
Cursor
SchemaVersion
```

---

# 8. Storage Version

Every local/server metadata store should have:

```rust
pub struct MetadataSchemaVersion(u32);
```

This tracks Aequora internal persistence schema.

It is separate from:

```text
domain schema
protocol version
local application schema
```

---

# 9. Metadata Root Record

Each store has one root metadata record.

Conceptually:

```rust
pub struct MetadataRoot {
    pub schema_version: MetadataSchemaVersion,
    pub store_id: StoreId,
    pub created_at: Timestamp,
    pub last_migrated_at: Timestamp,
}
```

---

# 10. StoreId

Define stable unique ID:

```rust
pub struct StoreId(Uuid);
```

For client store:

```text
LocalStoreId
```

may be subtype/newtype.

For server metadata store:

```text
AuthorityMetadataStoreId
```

may be separate.

---

# 11. Client Store Metadata

Logical record:

```text
aequora_local_store
```

Fields:

```text
local_store_id
metadata_schema_version
store_generation
device_id
created_at
last_opened_at
```

---

# 12. Store Generation

Part 05/10:

```rust
pub struct LocalStoreGeneration(u64);
```

Increment when local store is replaced/reinitialized.

---

# 13. Device Binding

Client store should know:

```text
DeviceId
```

but one device may host multiple local stores/accounts.

Do not assume one device = one store.

---

# 14. Outbox Record

Logical:

```text
aequora_outbox
```

Canonical fields:

```text
operation_id
tenant_id
actor_id
device_id
entity_type
entity_id
operation_kind
operation_schema_version
base_version
local_seq
state
priority
created_at
next_retry_at
attempt_count
ever_sent
payload_bytes
payload_digest
correlation_id
causation_id
authority_epoch_first_sent
compaction_key
```

---

# 15. Outbox Primary Identity

Primary logical key:

```text
OperationId
```

---

# 16. LocalOperationSeq

Define:

```rust
pub struct LocalOperationSeq(u64);
```

Monotonic per local store.

Used for:

```text
stable queue ordering
compaction segments
diagnostics
```

---

# 17. Outbox State

Recommended:

```rust
pub enum OutboxState {
    Pending,
    InFlight,
    Retryable,
    Blocked,
    Conflict,
    Committed,
    Rejected,
    Superseded,
}
```

---

# 18. Outbox Indexes

Required logical indexes:

```text
state + next_retry_at
state + priority + local_seq
entity_type + entity_id
compaction_key + state
operation_id unique
```

---

# 19. Outbox Payload Storage

Adapters may:

```text
inline payload
or
split metadata/payload
```

Semantics unchanged.

---

# 20. Outbox Atomicity

Part 01 invariant:

```text
local business mutation
+
outbox insert
```

must be one local transaction.

---

# 21. Outbox ACK

Committed/rejected operations may be removed after safe local reconciliation.

Do not remove before client state reflects authoritative outcome.

---

# 22. Outbox Tomb/History

Optional compact operation history may retain:

```text
OperationId
final status
server sequence
```

for diagnostics.

Not required for all clients.

---

# 23. Client Scope Cursor Record

Logical:

```text
aequora_scope_cursor
```

Fields:

```text
scope_id
scope_version
scope_generation
authority_id
authority_epoch
sequence
projection_schema_version
updated_at
```

---

# 24. Scope Cursor Primary Key

Usually:

```text
scope_id
```

within one local store.

If same scope ID can have multiple active generations simultaneously, include generation.

Initial recommendation:

```text
one active generation per ScopeId
```

---

# 25. Cursor Atomicity

When applying authoritative events:

```text
event state
+
cursor update
```

must commit in one local transaction.

---

# 26. Scope Subscription Record

Logical:

```text
aequora_subscription
```

Fields:

```text
subscription_id
scope_id
state
cache_policy
requested_at
activated_at
last_used_at
```

---

# 27. Scope Descriptor Cache

Logical:

```text
aequora_scope_descriptor
```

Fields:

```text
scope_id
scope_version
scope_generation
projection_schema_version
resolved_parameters
policy_version
authority_epoch
```

Server-issued.

---

# 28. Local Scope Membership

For entities that may belong to multiple local scopes:

```text
aequora_entity_scope_ref
```

Fields:

```text
entity_type
entity_id
scope_id
membership_state
```

---

# 29. Membership State

```rust
pub enum MembershipState {
    Present,
    PendingEviction,
}
```

Physical entity deletion occurs only when no remaining scope/reference/pending-op need exists.

---

# 30. Local Conflict Record

Logical:

```text
aequora_conflict
```

Fields:

```text
conflict_id
operation_id
entity_ref
base_version
authoritative_version
conflict_kind
conflicting_fields
created_at
resolution_state
```

---

# 31. Conflict Payload Retention

Large before/after payloads may be stored separately and aged out after resolution.

Part 14 governs.

---

# 32. Client Bootstrap Job

Logical:

```text
aequora_bootstrap_job
```

Fields:

```text
bootstrap_job_id
scope_id
snapshot_id
authority_epoch
state
manifest_digest
started_at
updated_at
staging_generation
boundary_sequence
```

---

# 33. Bootstrap Chunk Record

Logical:

```text
aequora_bootstrap_chunk
```

Fields:

```text
bootstrap_job_id
chunk_id
ordinal
state
downloaded_bytes
expected_bytes
hash
local_storage_ref
attempts
```

---

# 34. Bootstrap Index

Primary:

```text
bootstrap_job_id + chunk_id
```

Ordered:

```text
bootstrap_job_id + ordinal
```

---

# 35. Repair Job

Logical:

```text
aequora_repair_job
```

Fields:

```text
repair_id
scope_id
partition_id
state
detected_at
expected_digest
actual_digest
repair_plan_digest
completed_at
```

---

# 36. Anti-Entropy Digest Cache

Logical:

```text
aequora_integrity_node
```

Fields:

```text
scope_id
generation
partition_path
digest
sequence_boundary
updated_at
```

---

# 37. Scheduler State

Logical:

```text
aequora_scheduler_state
```

Store only durable fields needed across process restarts:

```text
backoff_until
last_success
circuit_state
batch_size_hint
```

Avoid persisting transient in-memory queues.

---

# 38. Client Leader Lease

Part 05 logical:

```text
aequora_local_coordinator_lease
```

Fields:

```text
local_store_id
process_instance_id
fencing_token
expires_at
updated_at
```

---

# 39. Lease Atomicity

Acquire/renew via transactional compare-and-set semantics.

---

# 40. Local Purge Directive State

Logical:

```text
aequora_purge_directive
```

Fields:

```text
directive_id
scope_id
reason
state
received_at
completed_at
```

---

# 41. Client Key Metadata

Do not store private keys in ordinary metadata tables.

May store:

```text
device_signing_key_id
secure_store_reference
key_status
```

---

# 42. Server Authority State

Logical:

```text
aequora_authority_state
```

Fields:

```text
authority_id
authority_epoch
authority_instance_id
role
fence_token
transition_id
updated_at
```

---

# 43. Authority State Cardinality

One active row per logical authority metadata store.

Historical transitions belong in separate transition history.

---

# 44. Authority Transition Record

Logical:

```text
aequora_authority_transition
```

Fields:

```text
transition_id
old_epoch
new_epoch
promotion_class
old_final_sequence
new_base_sequence
reason
created_at
actor
signature_ref
```

---

# 45. Server Operation Ledger

Logical:

```text
aequora_operation_ledger
```

Fields:

```text
operation_id
tenant_id
operation_kind
operation_schema_version
semantic_payload_digest
actor_id
device_id
status
first_seen_at
committed_at
authority_epoch
committed_sequence
entity_ref
base_version
result_code
handler_version
execution_input_digest
execution_plan_digest
```

---

# 46. Operation Ledger Unique Constraint

Required:

```text
operation_id unique
```

---

# 47. Payload Mismatch Rule

If same OperationId arrives with different semantic payload digest:

```text
reject
```

This is a critical idempotency invariant.

---

# 48. Ledger Status

```rust
pub enum LedgerStatus {
    Accepted,
    Rejected,
    Conflict,
    Superseded,
}
```

Potential transient `Executing` record is optional and must be carefully designed if used.

---

# 49. Ledger Transaction Atomicity

For accepted operation:

```text
business mutation
+
entity version
+
journal
+
operation ledger
+
required audit
```

same authoritative transaction.

---

# 50. Authoritative Journal

Logical:

```text
aequora_journal
```

Fields:

```text
authority_epoch
sequence
event_id
tenant_id
entity_type
entity_id
entity_version
event_kind
event_schema_version
routing_metadata
payload_bytes
payload_digest
operation_id
correlation_id
causation_id
occurred_at
```

---

# 51. Journal Primary Order

Logical unique key:

```text
authority_epoch + sequence
```

---

# 52. EventId Uniqueness

Recommended:

```text
event_id unique
```

globally.

---

# 53. Journal Indexes

Depending on scope strategy:

```text
tenant_id + sequence
routing_partition + sequence
entity_ref + sequence
operation_id
```

---

# 54. Journal Sequence Allocation

Must be monotonic within AuthorityEpoch.

Gap-free is not strictly required unless implementation chooses it.

Cursor semantics require:

```text
total order
```

not necessarily gapless integers.

---

# 55. Journal Payload

Canonical event payload.

Avoid DB-specific row encoding.

---

# 56. Journal Routing Metadata

Part 07 may include:

```text
before partitions
after partitions
scope routing keys
membership delta
```

Keep bounded and typed.

---

# 57. Server Scope Registry

Logical:

```text
aequora_scope_registry
```

Fields:

```text
scope_id
tenant_id
scope_kind
scope_version
scope_generation
projection_schema_version
resolved_parameters
policy_version
status
created_at
updated_at
```

---

# 58. Scope Registry Unique Rules

ScopeId unique.

If logical scope is recreated:

```text
same ScopeId + increment generation
```

or new ScopeId depending semantics.

---

# 59. Materialized Scope Membership

Optional table:

```text
aequora_scope_membership
```

Fields:

```text
scope_id
entity_type
entity_id
membership_version
```

Only for membership strategies that need it.

---

# 60. Scope Policy Cache

May be derived and rebuildable.

Do not make correctness depend on stale cache without policy version.

---

# 61. Device Registry

Logical:

```text
aequora_device
```

Fields:

```text
device_id
tenant_id
principal_id
status
registered_at
last_seen_at
current_public_key_id
client_build
```

---

# 62. Device Status

```rust
pub enum DeviceStatus {
    Active,
    Retired,
    Revoked,
    Expired,
}
```

---

# 63. Device Scope Watermark

Logical:

```text
aequora_device_scope_watermark
```

Fields:

```text
device_id
scope_id
authority_epoch
scope_generation
last_ack_sequence
last_seen_at
```

Used by:

```text
tombstone GC
journal retention
device retirement
```

---

# 64. Watermark Trust

Update only from authenticated successful client ACK.

Do not trust arbitrary claimed high cursor before server validates session/scope.

---

# 65. Snapshot Catalog

Logical:

```text
aequora_snapshot
```

Fields:

```text
snapshot_id
tenant_id
scope_id
scope_generation
authority_epoch
boundary_sequence
snapshot_schema_version
profile
state
manifest_digest
root_digest
created_at
published_at
expires_at
```

---

# 66. Snapshot State

```rust
pub enum SnapshotState {
    Building,
    Verifying,
    Published,
    Expired,
    Failed,
}
```

---

# 67. Snapshot Chunk Catalog

Logical:

```text
aequora_snapshot_chunk
```

Fields:

```text
snapshot_id
chunk_id
ordinal
object_ref
compressed_bytes
uncompressed_bytes
ciphertext_digest
canonical_digest
compression
encryption_key_id
```

---

# 68. Snapshot Lease

Logical:

```text
aequora_snapshot_lease
```

Fields:

```text
snapshot_id
lease_id
device_or_session_ref
expires_at
```

---

# 69. Snapshot Build Job

May reuse Part 23 generic durable job framework later.

Until then logical metadata:

```text
snapshot_id
builder_lease
checkpoint
```

---

# 70. Audit Event

Logical canonical:

```text
aequora_audit_event
```

Fields:

```text
audit_event_id
tenant_id
audit_epoch
audit_sequence
category
action_id
subject_kind
subject_id
actor_kind
actor_id
occurred_at
operation_id
event_id
correlation_id
reason_code
schema_version
payload_bytes
previous_hash
event_hash
```

---

# 71. Audit Primary Order

```text
tenant_id + audit_epoch + audit_sequence
```

or:

```text
tenant + chain_partition + epoch + sequence
```

if partitioned chains.

---

# 72. Audit Change Payload

Can be embedded canonical binary initially.

Relational decomposition optional.

---

# 73. Field Provenance

Logical:

```text
aequora_field_provenance
```

Fields:

```text
entity_type
entity_id
field_id
audit_event_id
event_id
updated_at
```

---

# 74. Audit Checkpoint

Logical:

```text
aequora_audit_checkpoint
```

Fields:

```text
tenant_id
audit_epoch
sequence
root_hash
signing_key_id
signature
created_at
```

---

# 75. Journal Checkpoint

For Part 16 fork detection:

```text
aequora_journal_checkpoint
```

Fields:

```text
authority_id
authority_epoch
sequence
journal_root
signature_ref
created_at
```

---

# 76. Import Job

Part 09:

```text
aequora_import_job
```

Fields:

```text
job_id
tenant_id
mode
source_kind
source_fingerprint
state
mapping_version
checkpoint
correlation_id
started_at
updated_at
```

---

# 77. Import Record

Logical:

```text
aequora_import_record
```

Fields:

```text
job_id
source_key
target_entity_id
status
canonical_digest
```

---

# 78. Import Identity Map

Logical:

```text
aequora_import_identity_map
```

Fields:

```text
job_id
source_type
source_key
entity_type
entity_id
```

Unique:

```text
job + source_type + source_key
```

---

# 79. Import Quarantine

Logical:

```text
aequora_import_quarantine
```

Fields:

```text
job_id
source_key
error_code
status
sanitized_details
```

---

# 80. Export Job

Logical:

```text
aequora_export_job
```

Fields:

```text
export_id
tenant_id
mode
state
boundary
manifest_digest
storage_ref
expires_at
created_by
created_at
```

---

# 81. Replay Artifact Registry

Logical:

```text
aequora_replay_artifact
```

Fields:

```text
replay_id
operation_id
handler_version
artifact_ref
artifact_digest
retention_class
expires_at
```

---

# 82. Governance Policy Registry

Logical:

```text
aequora_retention_policy
```

Fields:

```text
retention_class_id
policy_version
minimum_retention
maximum_retention
deletion_mode
legal_hold_eligible
archive_before_delete
```

---

# 83. Legal Hold

Logical:

```text
aequora_legal_hold
```

Fields:

```text
hold_id
tenant_id
selector_kind
selector_payload
reason_code
state
created_by
created_at
released_by
released_at
```

---

# 84. Erasure Request

Logical:

```text
aequora_erasure_request
```

Fields:

```text
request_id
tenant_id
subject_ref
state
policy_version
requested_at
completed_at
verification_digest
```

---

# 85. Purge Job

Logical:

```text
aequora_purge_job
```

Fields:

```text
purge_id
tenant_id
state
policy_version
plan_digest
started_at
completed_at
```

---

# 86. Erasure Ledger

Logical:

```text
aequora_erasure_ledger
```

Minimal fields:

```text
request_id
subject_pseudonymous_ref
completed_at
policy_version
```

---

# 87. Governance Storage Surface

Logical:

```text
aequora_storage_surface
```

Fields:

```text
storage_surface_id
kind
status
capabilities
```

May be configuration rather than DB table.

---

# 88. Key Registry

Part 15:

```text
aequora_key_registry
```

Fields:

```text
key_id
purpose
algorithm
public_key
status
not_before
not_after
revoked_at
registry_generation
```

Private key absent.

---

# 89. Crypto Policy

Logical:

```text
aequora_crypto_policy
```

Fields:

```text
policy_version
allowed_digests
allowed_signatures
allowed_encryption
required_features
created_at
```

---

# 90. Compatibility Policy

Logical:

```text
aequora_compatibility_policy
```

Fields:

```text
policy_generation
preferred_protocol
supported_protocols
deprecated_protocols
minimum_build_constraints
required_capabilities
updated_at
```

---

# 91. Capability Registry

Usually static compiled registry, not mutable DB table.

May expose read-only generated metadata.

---

# 92. Protocol Registry

Likewise:

```text
source-controlled RON
```

compiled into binary.

Server policy references IDs.

---

# 93. Regional Replica Watermark

Part 17:

```text
aequora_replica_watermark
```

Fields:

```text
replica_id
region_id
authority_epoch
sequence
projection_id
updated_at
```

May be control-plane state rather than authoritative DB.

---

# 94. Background Job Metadata

Part 23 will define generic durable jobs.

Part 22 should reserve common schema:

```text
aequora_job
aequora_job_attempt
aequora_job_lease
```

---

# 95. Generic Job Record

Conceptually:

```text
job_id
tenant_id
job_kind
state
priority
payload_ref
checkpoint
attempt_count
next_run_at
created_at
updated_at
```

---

# 96. Job Lease

Fields:

```text
job_id
worker_id
fencing_token
expires_at
```

---

# 97. Internal ID Strategy

All globally relevant metadata IDs should use:

```text
UUIDv7
```

unless natural monotonic sequence is semantically required.

---

# 98. Sequence Types

Use integer monotonic counters for:

```text
journal sequence
audit sequence
local operation sequence
fencing token
generation
epoch
```

---

# 99. Never Confuse IDs and Sequences

```text
OperationId
```

is identity.

```text
Sequence
```

is order.

Different types.

---

# 100. Timestamp Policy

Store authoritative times in canonical UTC timestamp type.

Client local timestamps may also exist for diagnostics.

---

# 101. Timestamp Is Not Ordering Authority

Use sequence/version for ordering.

---

# 102. Boolean Flags

Avoid many loosely related booleans.

Prefer enums:

```text
state
status
mode
```

---

# 103. Nullability

Use null/optional only for genuinely absent concepts.

Do not use null to represent hidden state machine transitions.

---

# 104. Canonical Enum IDs

Persistent enum representation should use stable numeric IDs.

Rust enum discriminants should not be relied upon implicitly.

---

# 105. Schema Registry

Maintain mapping:

```text
persistent field
semantic type
introduced version
nullable/default
```

for internal migrations.

---

# 106. DB-Agnostic Value Types

Core logical persistence types:

```text
u64
i64
bool
bytes
UTF-8 string
UUID
timestamp
small enums
```

Avoid vendor-specific types in core spec.

---

# 107. Decimal

Business domain Decimal belongs in domain payload, not generic metadata unless specifically needed.

---

# 108. JSON

Do not require JSON storage for internal metadata.

Postcard/RON blobs are acceptable where queryability is not required.

---

# 109. Queryable vs Opaque Fields

Split metadata into:

```text
indexed/queryable columns
opaque canonical payload
```

This is often ideal.

---

# 110. Example Journal Storage

Relational:

```text
sequence
tenant
entity IDs
routing keys
payload BYTEA
```

Not:

```text
all domain fields flattened
```

---

# 111. Adapter Mapping Contract

Each adapter must document:

```text
logical record
physical representation
unique constraints
transaction support
index support
retention behavior
```

---

# 112. Adapter Certification

Conformance tests validate behavior, not physical schema names.

---

# 113. PostgreSQL Mapping

Recommended:

```text
tables
B-tree indexes
BYTEA payloads
UUID
BIGINT sequences
TIMESTAMPTZ
```

Partition only when scale justifies.

---

# 114. SQLite/Stoolap Mapping

Use:

```text
tables
indexes
BLOB payload
INTEGER sequences
```

with local transaction semantics.

---

# 115. KV Mapping

Key layout example:

```text
outbox/<state>/<priority>/<local_seq>/<op_id>
journal/<epoch>/<sequence>
scope_cursor/<scope_id>
```

Need secondary index emulation where required.

---

# 116. KV Atomicity

Adapter must provide transaction/batch atomicity for required invariants.

---

# 117. Redb Mapping

Could use:

```text
multiple tables/keyspaces
transactional updates
```

but must prove required range scans/indexes.

---

# 118. Schema Namespacing

Avoid collisions with application tables.

Use:

```text
aequora_*
```

or dedicated schema:

```text
aequora.*
```

on PostgreSQL.

---

# 119. Tenant Partition Key

Every server-side tenant-owned metadata row should include:

```text
tenant_id
```

unless inherently global.

---

# 120. Cross-Tenant Query Guard

Repositories should require tenant context.

Avoid unscoped generic queries.

---

# 121. Row-Level Security

Optional PostgreSQL defense-in-depth.

Do not make core correctness depend on RLS alone.

---

# 122. Foreign Keys

Use where they improve integrity.

But avoid cascading deletes that violate independent retention semantics.

---

# 123. Soft References

Some historical audit/import records may reference entities later deleted.

Use logical soft reference, not mandatory FK, where retention differs.

---

# 124. Referential Retention

A strict FK can prevent legal deletion/pseudonymization unexpectedly.

Choose intentionally.

---

# 125. Migration Strategy

Internal metadata migrations use:

```text
expand
backfill
switch
contract
```

where rolling compatibility needed.

---

# 126. Metadata Migration Lock

Before destructive local migration:

```text
acquire maintenance/local coordinator lease
```

---

# 127. Server Migration

Enterprise server migration should run before enabling binary requiring new schema.

---

# 128. Migration Id

Define:

```rust
pub struct MetadataMigrationId(u32);
```

Monotonic stable IDs.

---

# 129. Migration Journal

Logical:

```text
aequora_metadata_migration
```

Fields:

```text
migration_id
applied_at
binary_version
checksum
```

---

# 130. Checksum

Migration source/spec may have checksum.

If same migration ID with different checksum:

```text
fail
```

---

# 131. No Auto-Repair Unknown Schema

If DB metadata version is newer than binary supports:

```text
refuse startup
```

---

# 132. Backward-Compatible Read

Older binary should not write to newer schema unless explicitly certified.

---

# 133. Client Store Upgrade

Part 20:

```text
pending outbox preserved
```

through migration.

---

# 134. Migration Backup

For risky client metadata migration:

```text
backup metadata/outbox
```

where feasible.

---

# 135. Journal Retention Metadata

Logical:

```text
aequora_journal_floor
```

Fields:

```text
scope_id or tenant partition
authority_epoch
minimum_sequence
updated_at
```

---

# 136. Tombstone Metadata

If tombstones stored in business tables, metadata may still track:

```text
deletion_sequence
retention_class
```

---

# 137. Retired Entity Registry

Optional:

```text
aequora_retired_entity
```

Fields:

```text
entity_type
entity_id
deletion_epoch
deletion_sequence
expires_at
```

prevents stale resurrection after tombstone GC.

---

# 138. Operation Recovery Record

Part 16:

```text
aequora_operation_recovery
```

Fields:

```text
operation_id
old_epoch
new_epoch
resolution
resolved_at
actor
```

---

# 139. Side-Effect Intent Placeholder

Part 23 will define:

```text
aequora_side_effect_intent
```

Fields likely:

```text
intent_id
operation_id
kind
state
idempotency_key
attempt_count
next_attempt_at
payload
```

---

# 140. Side-Effect Result

Separate:

```text
provider reference
outcome
captured response digest
```

---

# 141. Data Access Pattern Table

Each logical record should document primary access pattern.

Example:

```text
Outbox:
    dequeue pending
    lookup OperationId
    compact by key

Journal:
    scan after sequence
    lookup event
    lookup operation

Audit:
    scan by tenant/time
    lookup subject
```

Indexes derive from these.

---

# 142. Index Governance

Every required index should exist in adapter certification.

Optional indexes are workload-specific.

---

# 143. Index Naming

Physical names are adapter-specific.

Logical index IDs can be stable in schema docs.

---

# 144. Hot Index Budget

Too many indexes increase write cost.

Only index known query paths.

---

# 145. Covering Indexes

PostgreSQL adapter may optimize with INCLUDE columns.

Not core requirement.

---

# 146. Partial Indexes

Useful for:

```text
outbox pending only
active device only
published snapshots only
```

adapter-specific optimization.

---

# 147. Local Outbox Partial Index

Example SQL adapter:

```text
WHERE state IN (Pending, Retryable)
```

can reduce hot index size.

---

# 148. Data Retention

Each metadata record declares retention owner.

Examples:

```text
journal -> governance/sync retention
ledger -> retry/audit policy
bootstrap chunks -> bootstrap completion
conflict details -> conflict retention
```

---

# 149. Retention Metadata Matrix

Maintain table in code/docs:

```text
record type
minimum retention reason
GC precondition
legal hold eligible?
```

---

# 150. GC Jobs

GC should run through durable jobs.

Do not delete synchronously in ordinary sync path except trivial cleanup.

---

# 151. Crash Safety

Every state machine persistence update must be crash-safe.

Rule:

```text
persist new durable state
then perform next external step
```

or use idempotent compensating recovery.

---

# 152. State Transition Compare-And-Set

For jobs/outbox:

```text
expected state
→ new state
```

atomically.

Prevents duplicate worker races.

---

# 153. Optimistic State Version

Optional:

```text
row_version
```

for admin/job records.

---

# 154. Fencing Token

Worker/leader updates include current fencing token where required.

---

# 155. Duplicate Worker

Old worker with stale token cannot commit new checkpoint.

---

# 156. Metadata Transactions

Classify transaction groups.

---

# 157. Client Transaction Group A

```text
local business mutation
+
outbox insert
```

---

# 158. Client Transaction Group B

```text
authoritative event apply
+
entity version
+
scope membership
+
cursor
+
outbox ACK/conflict
```

---

# 159. Server Transaction Group C

```text
business mutation
+
entity/aggregate version
+
journal
+
operation ledger
+
required audit
+
side-effect intent
```

---

# 160. Server Transaction Group D

```text
authority epoch transition metadata
+
write role/fence state
```

under controlled promotion procedure.

---

# 161. Scope Transaction Group

Scope generation change should atomically publish:

```text
new descriptor
generation
policy version
```

---

# 162. Snapshot Publish Group

Only mark snapshot `Published` after:

```text
all chunks durable
manifest durable
root verified
```

---

# 163. Legal Hold Group

Hold activation + audit should commit atomically if policy requires.

---

# 164. Key Rotation Group

Key registry transition + audit should be consistent.

Private KMS state may require two-phase operational workflow.

---

# 165. Metadata Eventual Projections

Search dashboards/metrics can lag.

Core correctness metadata cannot.

---

# 166. Read-Only Derived Metadata

Examples:

```text
top noisy tenants
compat dashboard
audit search index
```

rebuildable.

---

# 167. Internal Persistence API

Core should not expose raw SQL.

Use repositories:

```text
OutboxStore
JournalStore
LedgerStore
ScopeStore
AuditStore
SnapshotStore
GovernanceStore
```

---

# 168. Capability-Specific Traits

Avoid one giant `StorageAdapter` trait with 100 methods.

Use smaller capability traits.

---

# 169. OutboxStore

Conceptual:

```rust
pub trait OutboxStore {
    async fn enqueue(...);
    async fn claim_batch(...);
    async fn mark_retry(...);
    async fn reconcile(...);
}
```

---

# 170. JournalStore

```rust
pub trait JournalStore {
    async fn append(...);
    async fn scan_after(...);
    async fn floor(...);
}
```

---

# 171. OperationLedgerStore

```rust
pub trait OperationLedgerStore {
    async fn lookup(OperationId);
    async fn insert_result(...);
}
```

---

# 172. ScopeStore

```text
descriptor
membership
watermarks
```

---

# 173. SnapshotStore

```text
catalog
lease
chunk metadata
```

---

# 174. GovernanceStore

Part 14 broader destructive lifecycle interface.

---

# 175. Transaction Handle

Traits that must participate in same transaction should accept shared logical transaction handle.

---

# 176. AuthoritativeTransaction

Conceptual:

```rust
pub trait AuthoritativeTransaction:
    BusinessWrite
    + JournalWrite
    + LedgerWrite
    + AuditWrite
    + SideEffectWrite
{}
```

Implementation may wrap Postgres transaction.

---

# 177. LocalTransaction

```rust
pub trait LocalTransaction:
    LocalBusinessWrite
    + OutboxWrite
    + CursorWrite
    + ConflictWrite
{}
```

---

# 178. Avoid Trait Object Explosion

Public API can expose higher-level unit-of-work abstraction.

Internally compose capability traits.

---

# 179. Persistence Error Taxonomy

Examples:

```text
ConstraintViolation
VersionConflict
DuplicateOperation
PayloadDigestMismatch
StorageUnavailable
TransactionAborted
MigrationRequired
SchemaTooNew
CorruptionDetected
```

---

# 180. Retry Classification

Persistence errors should indicate:

```text
retryable
nonretryable
ambiguous
```

---

# 181. Ambiguous Commit

Network/client losing response after DB commit is handled by operation ledger.

Adapter API must not make caller invent state.

---

# 182. Commit Result

Conceptually:

```rust
pub enum CommitResult<T> {
    Committed(T),
    NotCommitted(StorageError),
    Ambiguous(StorageError),
}
```

For local embedded transaction ambiguity usually process crash recovery handles it.

---

# 183. Database Adapter Certification Tests

Every adapter should pass:

```text
atomic local mutation+outbox
atomic authoritative mutation+journal+ledger
unique OperationId
cursor atomic apply
lease fencing
snapshot staging
retention scans
```

as applicable.

---

# 184. Crash Injection

Test failures:

```text
after business write before journal
after journal before ledger
after ledger before commit
after commit before response
```

Expected:

```text
all-or-none authoritative transaction
```

---

# 185. Local Crash Injection

```text
after local entity write before outbox
```

Expected:

```text
rollback both
```

---

# 186. Cursor Crash Test

```text
apply events
crash before cursor commit
```

Expected:

```text
events replay idempotently
cursor not advanced
```

---

# 187. Metadata Corruption Detection

Critical metadata should have internal consistency checks.

Examples:

```text
cursor references unknown epoch
ledger committed sequence missing journal event
snapshot published without manifest
```

---

# 188. Startup Consistency Scan

Small bounded checks at startup:

```text
schema version
authority metadata
active local generation
in-flight stale states
```

Do not full-scan huge DB.

---

# 189. Deep Verify CLI

```text
aequora metadata verify
```

runs expensive checks.

---

# 190. Verify Checks

Examples:

```text
journal sequence uniqueness
ledger/journal linkage
cursor bounds
scope generation validity
snapshot catalog consistency
audit chain
```

---

# 191. Repair Policy

Do not auto-repair authoritative metadata inconsistencies silently.

Fail closed/quarantine.

---

# 192. Client Metadata Repair

Some local metadata can rebuild:

```text
derived caches
integrity digest cache
```

Core outbox/cursor corruption requires stronger recovery.

---

# 193. Export Metadata

Admin/support can export sanitized metadata bundle.

Exclude:

```text
secret keys
full sensitive payloads by default
```

---

# 194. Metadata Observability

Metrics from stores:

```text
outbox_rows
journal_rows
ledger_rows
snapshot_count
conflict_count
```

Avoid expensive full counts on every scrape.

Use approximate/cached counters.

---

# 195. Metadata Size Accounting

Track bytes where possible:

```text
outbox bytes
journal bytes
audit bytes
snapshot catalog bytes
```

---

# 196. Growth Alerts

Alert on:

```text
outbox growing
journal floor stuck
ledger growth above policy
conflict backlog
snapshot leak
```

---

# 197. Schema Documentation Generation

Generate logical schema docs from typed definitions/registry where possible.

---

# 198. Stable Field IDs

Internal persistence migration tooling may use field names physically, but canonical serialized metadata should use stable field semantics.

---

# 199. Store Feature Flags

Adapter may omit unused subsystems.

Example client without audit cache:

```text
no aequora_local_audit table
```

Core capability manifest records absence.

---

# 200. Minimal Client Schema

At minimum:

```text
local_store
outbox
scope_cursor
subscription
coordinator lease
```

plus application entities.

---

# 201. Standard Client Schema

Adds:

```text
conflict
bootstrap
repair
scheduler
scope membership
```

---

# 202. Full Client Schema

Adds:

```text
local audit projection
integrity tree
diagnostic metadata
```

---

# 203. Minimal Server Schema

At minimum:

```text
authority_state
operation_ledger
journal
scope_registry
device
device_scope_watermark
```

plus application data.

---

# 204. Enterprise Server Schema

Adds:

```text
audit
snapshots
governance
crypto registry
jobs
regional metadata
```

---

# 205. Namespace Isolation

If multiple Aequora instances use same DB:

```text
separate schema/database
```

Do not rely on table prefixes alone if operational isolation important.

---

# 206. Multi-Tenant Physical Models

Aequora supports:

```text
shared tables + tenant_id
schema-per-tenant
database-per-tenant
```

through adapter.

Logical semantics unchanged.

---

# 207. Shared Table

Most efficient for SaaS.

Requires strong tenant predicates/indexes.

---

# 208. Schema Per Tenant

Simplifies some isolation but makes migrations harder.

---

# 209. Database Per Tenant

Strong isolation/high cost.

Adapter/control plane maps tenant.

---

# 210. OperationId Scope

Still globally unique regardless physical tenant layout.

---

# 211. Sequence Scope

Journal sequence may be:

```text
global authority sequence
```

recommended initially.

Could be partition-local in future only with cursor redesign.

---

# 212. Why Global Sequence

Simplifies:

```text
ordering
cursor
fork checkpoints
replica watermark
```

---

# 213. Scalability Limit

If global sequence becomes bottleneck at extreme scale, future sharded authority design may introduce per-shard timelines.

Do not optimize prematurely.

---

# 214. Audit Sequence

May be per tenant to reduce contention.

Distinct from journal sequence.

---

# 215. Fencing Sequence

Per lease/authority scope.

---

# 216. Metadata Encryption

Sensitive opaque payloads may use Part 15 encryption.

Queryable identity/index fields generally remain visible unless field-level design says otherwise.

---

# 217. Key References

Store:

```text
key_id
```

not raw key.

---

# 218. Payload Digest

For any opaque durable payload where mutation detection matters, store:

```text
canonical digest
```

---

# 219. Compression

DB payload blobs may be compressed only if:

```text
query does not need fields
CPU tradeoff justified
```

---

# 220. Journal Compression

Avoid per-row heavy compression if CPU expensive.

Could compress archival partitions later.

---

# 221. Snapshot Payload

Already optimized separately.

---

# 222. Metadata Backup

Authoritative metadata must be backed up with application business data.

Do not backup only domain tables.

---

# 223. Restore Ordering

Restore:

```text
business data
metadata
authority state
governance
crypto public registry
```

as one coherent recovery set.

---

# 224. Partial Restore Forbidden

Restoring journal without matching business state can corrupt semantics.

---

# 225. PITR

Use same DB transaction log where possible so metadata and business state restore consistently.

---

# 226. Cross-Store Metadata

If snapshot/blob catalogs live in separate store, backup/restore procedure must reconcile.

---

# 227. Object Ref

Never store signed temporary URLs durably.

Store stable opaque object key/ref.

---

# 228. Cleanup Orphans

Periodic job finds:

```text
object chunks with no catalog
catalog refs with missing object
```

---

# 229. Object Verification

Use digest.

---

# 230. Testing — Schema Migration

Test upgrade:

```text
v1 metadata
→ v2
→ v3
```

with pending outbox/conflicts/bootstrap.

---

# 231. Downgrade Test

Open v3 store with v2 binary.

Expected:

```text
refuse
```

unless explicit downgrade migration exists.

---

# 232. Cross-DB Logical Equivalence Test

Run same metadata state through:

```text
Postgres adapter
SQLite adapter
Stoolap adapter
Redb adapter
```

Export canonical metadata.

Compare semantic equality.

---

# 233. Index Requirement Test

Adapter conformance can benchmark/validate critical scan plan.

Exact physical index differs.

---

# 234. Retention Test

Advance device watermarks.

Verify:

```text
journal floor
tombstone eligibility
```

metadata transitions correctly.

---

# 235. Epoch Test

Increment authority epoch.

Verify old cursors/snapshots rejected.

---

# 236. Snapshot Test

Published snapshot cannot exist without all chunk metadata.

---

# 237. Audit Test

Required audit linkage survives retry and restore.

---

# 238. Job Lease Test

Two workers contend.

Only valid fencing token can checkpoint.

---

# 239. Metadata Invariants

Add:

## AEQ-INV-META001

```text
Every durable Aequora store declares exactly one supported MetadataSchemaVersion before normal operation.
```

## AEQ-INV-META002

```text
OperationId is unique in the authoritative operation ledger, and a duplicate OperationId with a different semantic payload digest is rejected.
```

## AEQ-INV-META003

```text
Client authoritative-state application and corresponding scope cursor advancement occur in one local transaction.
```

## AEQ-INV-META004

```text
Required server metadata participating in an authoritative effect commits atomically with the business mutation.
```

## AEQ-INV-META005

```text
A published snapshot references only durable verified chunks belonging to the same authority epoch and snapshot boundary.
```

## AEQ-INV-META006

```text
Adapters may change physical storage layout but may not weaken logical uniqueness, ordering, transaction, or retention semantics.
```

---

# 240. Additional Invariants

## AEQ-INV-META007

```text
A stale fencing token cannot update durable coordinator/job/authority state after a newer token has been issued.
```

## AEQ-INV-META008

```text
Metadata schema migrations preserve pending unsynchronized user intent or fail without committing a partial migration.
```

## AEQ-INV-META009

```text
No durable secret private key material is stored in ordinary Aequora metadata records.
```

---

# 241. Recommended Crates

```text
aequora-metadata/
├── version.rs
├── client.rs
├── server.rs
├── records.rs
├── indexes.rs
├── migrations.rs
├── invariants.rs
└── export.rs
```

Adapter SDK:

```text
aequora-adapter-sdk/
├── outbox.rs
├── journal.rs
├── ledger.rs
├── scope.rs
├── snapshot.rs
├── audit.rs
└── transaction.rs
```

---

# 242. Generated Schema Specs

Potential:

```text
schemas/
├── metadata-v1.ron
├── metadata-v2.ron
└── compatibility.ron
```

Used for:

```text
documentation
migration checks
adapter conformance
```

---

# 243. PostgreSQL Reference Schema

A reference SQL schema may exist in:

```text
aequora-postgres/migrations/
```

but is not the canonical specification.

The canonical specification is the typed logical schema in `aequora-metadata`.

---

# 244. Client Reference Schema

Similarly:

```text
aequora-stoolap/migrations/
```

maps logical client metadata into Stoolap.

---

# 245. Application Tables

Aequora does not require all business tables to follow one schema.

Only adapter/domain handlers must map:

```text
EntityRef
Version
Aggregate
```

correctly.

---

# 246. Metadata API Encapsulation

Application code should not update:

```text
journal
ledger
cursor
outbox state
authority epoch
```

directly.

Only Aequora runtime APIs.

---

# 247. Admin Read Access

Admins may query through CLI/API.

Direct SQL remains emergency/diagnostic only.

---

# 248. Metadata Mutation Access

Restrict DB roles where possible.

Application runtime role needs normal writes.

Human read-only role separate.

---

# 249. Audit DB Admin Actions

Manual correction of metadata should produce incident/admin record.

---

# 250. No Silent Manual Fixes

If operator edits metadata directly, future verification may fail.

Prefer repair tooling.

---

# 251. Metadata Verify Command

Suggested:

```text
aequora metadata status
aequora metadata verify
aequora metadata migrate
aequora metadata export
aequora metadata explain <record>
```

---

# 252. Explain Record

Useful output:

```text
OperationId O
ledger status committed
journal sequence 4412
audit event A
entity version 8
```

---

# 253. Completion Criteria

Part 22 is complete when:

```text
[ ] MetadataSchemaVersion defined
[ ] client metadata records specified
[ ] outbox schema/indexes specified
[ ] cursor/scope membership specified
[ ] bootstrap/repair metadata specified
[ ] authority state specified
[ ] operation ledger specified
[ ] journal specified
[ ] scope/device/watermark specified
[ ] snapshot catalog specified
[ ] audit/governance/crypto/compat metadata specified
[ ] transaction groups specified
[ ] adapter mapping contract specified
[ ] migration strategy specified
[ ] index/access-pattern matrix specified
[ ] conformance/crash tests specified
[ ] metadata invariants added
```

---

# 254. Final Architecture

```text
                     AEQUORA LOGICAL METADATA
                                │
        ┌───────────────────────┼───────────────────────┐
        ▼                       ▼                       ▼
    CLIENT STORE            AUTHORITY STORE        AUXILIARY STORES
        │                       │                       │
        │                       │                       │
   Outbox/Cursor         Ledger/Journal           Snapshot Objects
   Scope/Conflict        Scope/Device              Audit Archive
   Bootstrap/Repair      Authority/Audit           Export/Replay
        │                       │                       │
        └───────────────────────┼───────────────────────┘
                                ▼
                    Canonical Typed Records
                                │
                    Adapter Mapping Contract
                                │
          ┌─────────────────────┼─────────────────────┐
          ▼                     ▼                     ▼
      PostgreSQL             Stoolap              SQLite/Redb
       tables              tables/store            keyspaces
          │                     │                     │
          └─────────────────────┼─────────────────────┘
                                ▼
                   SAME LOGICAL INVARIANTS
```

The architectural principle is:

> **Aequora's metadata schema is an interoperability contract, not a particular database layout.**

By defining durable identities, state machines, transaction groups, indexes, retention semantics, and migration rules at the logical level, Aequora can support PostgreSQL, Stoolap, SQLite, Redb, and future storage engines without allowing adapter-specific persistence choices to alter synchronization correctness.
