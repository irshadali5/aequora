# Aequora Sync — Part 26

# Legacy Application Compatibility and Incremental Adoption Architecture

## 1. Purpose

Many production applications already have:

```text
existing databases
existing REST APIs
existing business services
existing authentication
existing mobile/desktop/web clients
legacy IDs
legacy audit tables
legacy background jobs
legacy CDC streams
```

Aequora must be useful without requiring:

```text
rewrite everything first
```

A big-bang migration is operationally risky and commercially unrealistic.

The central rule is:

> **Aequora should enter a legacy system through explicit compatibility boundaries, progressively take ownership of synchronization semantics, and never create two uncontrolled sources of business truth.**

---

# 2. Goals

The incremental-adoption architecture should support:

```text
legacy read integration
legacy change capture
canonical mapping
incremental aggregate migration
dual-read transition
controlled dual-write bridge where unavoidable
shadow validation
side-by-side verification
cutover fencing
rollback
legacy client coexistence
legacy ID mapping
```

---

# 3. Non-Goals

This architecture does not promise:

```text
automatic conversion of arbitrary legacy business logic
transparent sync from any schema with zero mapping
permanent uncontrolled dual-write
row-level mirroring as the final architecture
```

Legacy integration is a migration path, not the desired end state.

---

# 4. Adoption Stages

Recommended progression:

```text
Stage 0 — Observe
Stage 1 — Canonical Read
Stage 2 — CDC Bridge
Stage 3 — Selected Aequora Operations
Stage 4 — Aequora Authoritative Aggregate
Stage 5 — Legacy Compatibility Facade
Stage 6 — Legacy Retirement
```

---

# 5. Stage 0 — Observe

Aequora does not write.

It only inspects:

```text
legacy schema
ID conventions
change rates
transaction boundaries
business invariants
```

---

# 6. Discovery Output

Produce:

```text
LegacySystemManifest
```

containing:

```text
source system ID
database kind
tables/collections
candidate aggregates
primary keys
timestamps
soft-delete semantics
transaction boundaries
```

---

# 7. LegacySystemId

Define:

```rust
pub struct LegacySystemId(Uuid);
```

Stable identity for migration provenance.

---

# 8. LegacyRecordKey

Opaque source key:

```rust
pub struct LegacyRecordKey(Vec<u8>);
```

or typed application-specific key.

---

# 9. Stage 1 — Canonical Read

Map legacy rows/documents into Aequora canonical domain representation.

No authoritative writes yet.

---

# 10. Legacy Reader Adapter

Concept:

```rust
pub trait LegacyReader {
    async fn fetch_entity(
        &self,
        key: &LegacyRecordKey,
    ) -> Result<CanonicalEntity, LegacyReadError>;
}
```

---

# 11. Canonical Mapper

```rust
pub trait LegacyMapper {
    fn map(
        &self,
        record: LegacyRecord,
    ) -> Result<CanonicalEntity, MappingError>;
}
```

---

# 12. Mapping Is Explicit

Do not infer semantics from:

```text
column names alone
```

Application owns mapping.

---

# 13. Legacy DTO

Use dedicated types:

```rust
LegacyStudentRow
LegacyInvoiceRow
```

Never pass raw SQL row into Aequora core.

---

# 14. Validate Mapping

Legacy data may violate new invariants.

Classify:

```text
Valid
Repairable
Quarantined
Unsupported
```

---

# 15. Read-Only Projection

Aequora may expose:

```text
canonical read model
```

for new clients before taking write ownership.

---

# 16. Stage 2 — CDC Bridge

Capture legacy changes and convert them to canonical change events.

---

# 17. CDC Is a Bridge

CDC should not become permanent substitute for domain operations unless architecture explicitly chooses that mode.

---

# 18. CDC Sources

Examples:

```text
Postgres logical replication
MySQL binlog
SQL Server CDC
application change table
polling updated_at
```

---

# 19. Preferred CDC Quality

Best:

```text
transactional change stream
```

Worst:

```text
polling mutable timestamps
```

---

# 20. CDCBridge

Concept:

```rust
pub trait CdcBridge {
    async fn next_batch(
        &mut self,
    ) -> Result<Vec<LegacyChange>, CdcError>;
}
```

---

# 21. LegacyChange

Contains:

```text
source system
transaction marker
record key
change kind
source position
```

---

# 22. SourcePosition

Opaque:

```rust
pub struct LegacySourcePosition(Vec<u8>);
```

Examples:

```text
LSN
binlog offset
change-table sequence
```

---

# 23. Durable CDC Cursor

Persist:

```text
source position
```

only after mapped change is durably accepted.

---

# 24. CDC Transaction Grouping

If legacy source transaction changes multiple records atomically:

```text
preserve grouping where business semantics require
```

---

# 25. CDC Bridge Output

Prefer:

```text
CanonicalRecordChange
```

or:

```text
migration/import event
```

not fake user domain commands.

---

# 26. Provenance

Every bridged change records:

```text
LegacySystemId
LegacyRecordKey
SourcePosition
```

---

# 27. No Fake Actor

CDC actor:

```text
LegacySystem
```

not a user.

---

# 28. Stage 3 — Selected Aequora Operations

New client/feature starts writing via Aequora for selected operation kinds.

Legacy application may still own other writes.

---

# 29. Partial Operation Ownership

Example:

```text
Aequora owns attendance write
legacy owns timetable write
```

This can be safe if aggregate boundaries do not overlap unsafely.

---

# 30. Ownership Registry

Define:

```rust
pub enum WriteOwner {
    Legacy,
    Aequora,
    Migrating,
}
```

---

# 31. AggregateOwnership

```rust
pub struct AggregateOwnership {
    pub aggregate_type: AggregateType,
    pub owner: WriteOwner,
    pub generation: u64,
}
```

---

# 32. Ownership Must Be Explicit

Never infer from which service received the request.

---

# 33. Migrating State

Temporary controlled phase.

Requires strict bridge/fencing semantics.

---

# 34. Stage 4 — Aequora Authoritative Aggregate

For migrated aggregate:

```text
Aequora domain handler
+
Aequora authoritative DB transaction
```

becomes only authoritative write path.

---

# 35. Legacy Writes Disabled

Legacy application must:

```text
call Aequora
or
be read-only for migrated aggregate
```

---

# 36. Legacy Compatibility Facade

Legacy API can remain available.

Internally:

```text
legacy REST endpoint
↓
compatibility facade
↓
Aequora domain operation
```

---

# 37. Preserve External Contract

This allows old clients to keep using:

```text
old JSON API
```

while authority moves to Aequora.

---

# 38. LegacyApiFacade

Concept:

```rust
pub trait LegacyApiFacade {
    async fn translate(
        &self,
        request: LegacyRequest,
    ) -> Result<CanonicalOperation, CompatibilityError>;
}
```

---

# 39. Translation Boundary

Legacy JSON/XML/etc. stays at edge.

Core receives typed operation.

---

# 40. Stage 5 — Legacy Compatibility Facade

Legacy DB write logic removed.

Old clients still work through facade.

---

# 41. Stage 6 — Legacy Retirement

Eventually remove:

```text
old API
old CDC
legacy tables
ID mapping
compatibility DTOs
```

according to deprecation plan.

---

# 42. Ownership Safety Invariant

At any point, one aggregate must have one effective authoritative write owner.

---

# 43. Dual-Write Danger

Writing simultaneously to:

```text
legacy DB
Aequora DB
```

creates failure ambiguity.

---

# 44. Prefer CDC Over Dual-Write

Recommended migration flow:

```text
legacy remains writer
Aequora follows via CDC
```

until cutover.

Then:

```text
Aequora writer
legacy facade follows Aequora
```

---

# 45. If Dual-Write Unavoidable

Treat as temporary bridge with explicit coordination.

---

# 46. DualWriteBridge

Possible pattern:

```text
legacy transaction
+
outbox record
COMMIT
↓
bridge worker
↓
Aequora import/update
```

This is not synchronous distributed ACID.

---

# 47. No Two-DB Atomicity Claim

If legacy and Aequora DBs are separate:

```text
cannot guarantee one ACID transaction across both
```

unless external distributed transaction technology is intentionally used.

Aequora does not require it.

---

# 48. Bridge Idempotency

Each legacy source change has stable dedup key:

```text
LegacySystemId + SourcePosition + RecordKey
```

---

# 49. BridgeLedger

Logical:

```text
aequora_legacy_bridge_ledger
```

Fields:

```text
legacy_system_id
source_position
record_key
canonical_digest
status
applied_at
```

---

# 50. Duplicate CDC Delivery

Safe.

Lookup bridge ledger.

---

# 51. Source Reordering

If CDC preserves transaction order, keep it.

If source does not:

```text
use version/checkpoint validation
```

---

# 52. Legacy Updated-At Polling

Risky because:

```text
clock granularity
same timestamp
manual edits
missed rows
```

Only fallback.

---

# 53. Polling Cursor

Use:

```text
updated_at + stable primary key
```

not timestamp alone.

---

# 54. Delete Detection

Legacy systems may:

```text
hard delete
soft delete
status flag
```

Mapper defines canonical deletion semantics.

---

# 55. Tombstones

If legacy source has no tombstones:

```text
bridge may need delete journal
```

or reconciliation scan.

---

# 56. Periodic Reconciliation

CDC bridge should periodically compare:

```text
source counts/digests
target canonical digests
```

using Part 03.

---

# 57. Shadow Mode

Before write cutover:

```text
run Aequora handler in shadow
```

against copied/current state.

Compare expected result to legacy outcome.

---

# 58. Shadow Execution

Does not commit authoritative mutation.

---

# 59. ShadowResult

```rust
pub struct ShadowResult {
    pub operation_id: ShadowOperationId,
    pub legacy_outcome_digest: Digest,
    pub aequora_plan_digest: Digest,
    pub match_state: ShadowMatch,
}
```

---

# 60. ShadowMatch

```text
Equivalent
ExpectedDifference
UnexpectedDifference
UnableToCompare
```

---

# 61. Shadow Side Effects

Never execute real side effects.

---

# 62. Differential Validation

Useful for:

```text
pricing
fees
workflow transitions
permissions
```

---

# 63. Canonical Equivalence

Do not compare raw row bytes.

Compare:

```text
domain semantic result
```

---

# 64. Cutover Criteria

Before migrating aggregate ownership:

```text
CDC lag near zero
shadow mismatch below threshold
mapping validated
legacy write path identified
rollback plan ready
```

---

# 65. Cutover Boundary

Define:

```rust
pub struct LegacyCutoverBoundary {
    pub legacy_position: LegacySourcePosition,
    pub authority_epoch: AuthorityEpoch,
}
```

---

# 66. Planned Cutover

Flow:

```text
announce maintenance / fence legacy writes
↓
drain in-flight legacy txns
↓
capture final CDC position
↓
bridge through final position
↓
verify canonical state
↓
switch ownership to Aequora
↓
enable legacy facade
```

---

# 67. Legacy Write Fence

Must be real.

Examples:

```text
DB role revoked
feature flag at legacy service
trigger rejects writes
maintenance mode
```

---

# 68. Do Not Rely on Documentation

"Please don't write here" is not fencing.

---

# 69. OwnershipGeneration

Increment on cutover.

Clients/services can reject stale ownership assumptions.

---

# 70. Stale Legacy Process

Old legacy node may continue running.

Fence must prevent authoritative write.

---

# 71. Rollback

If cutover fails before Aequora accepts new writes:

```text
restore legacy ownership
```

easy.

---

# 72. Rollback After Aequora Writes

Harder.

Need:

```text
reverse bridge
or
forward-only fix
```

---

# 73. Preferred Post-Cutover Policy

Once Aequora has accepted authoritative writes:

```text
do not casually revert authority
```

Treat rollback as authority migration.

---

# 74. Reverse Bridge

If business requires temporary rollback:

```text
Aequora journal
↓
legacy compatibility writer
```

must be designed/tested in advance.

---

# 75. Reverse Bridge Risk

Legacy schema may not represent new semantics.

Therefore rollback may be impossible after certain feature activation.

---

# 76. Point of No Return

Cutover plan should identify:

```text
irreversible boundary
```

---

# 77. Legacy IDs

Existing systems may use:

```text
INT
BIGINT
GUID
string keys
compound keys
```

Aequora uses canonical distributed IDs.

---

# 78. ID Mapping Strategies

```text
Preserve UUID
Deterministic Namespace UUID
Mapping Table
Opaque LegacyRef
```

---

# 79. LegacyIdMap

Logical:

```text
aequora_legacy_id_map
```

Fields:

```text
legacy_system_id
legacy_entity_type
legacy_key
aequora_entity_id
```

Unique both directions where mapping is one-to-one.

---

# 80. Deterministic Mapping

Useful for retryable migration.

Example:

```text
UUIDv5(namespace, legacy key)
```

if chosen and stable.

---

# 81. Do Not Expose Mapping Internally Everywhere

Convert at compatibility boundary.

Core uses:

```text
EntityId
```

---

# 82. Legacy Foreign Keys

Mapper resolves through ID map.

---

# 83. Two-Pass Migration

For strongly connected references:

```text
Pass 1 allocate IDs
Pass 2 map relationships
```

---

# 84. Legacy Natural Keys

Do not use mutable natural key as Aequora identity.

---

# 85. Business Number

Invoice number/student roll number remains domain field, not EntityId.

---

# 86. Legacy Versions

Many systems use:

```text
updated_at
rowversion
etag
revision integer
```

---

# 87. Mapping to EntityVersion

If reliable monotonic revision exists, may seed from it.

Otherwise:

```text
seed EntityVersion(1)
```

at migration baseline.

---

# 88. Do Not Treat Timestamp as Strong Version

Especially wall-clock updated_at.

---

# 89. Legacy Audit

Existing audit history may be retained separately.

Aequora does not need to synthesize entire past as live journal.

---

# 90. Audit Import

Options:

```text
archive legacy audit as external source
import selected historical audit
start Aequora audit at cutover baseline
```

---

# 91. Provenance Baseline

Create audit event:

```text
AggregateMigratedFromLegacy
```

with:

```text
LegacySystemId
cutover boundary
mapping digest
```

---

# 92. Legacy Operation History

Do not invent fake OperationIds for historical row mutations unless reconstructable and needed.

---

# 93. Operation Ledger Baseline

Normally:

```text
empty for pre-Aequora history
```

with migration baseline marker.

---

# 94. Client Coexistence

During migration:

```text
new Aequora client
legacy mobile/web client
```

may coexist.

---

# 95. Compatibility Gateway

Old client API calls translated to Aequora operations for migrated aggregates.

---

# 96. Legacy Read Path

Could still read legacy projection temporarily.

But after authority cutover, ensure it is updated from Aequora.

---

# 97. Projection Bridge

```text
Aequora journal
↓
legacy read-model updater
↓
legacy read DB
```

---

# 98. Eventual Legacy Read

Old UI may become slightly eventual.

Document if changed.

---

# 99. Strong Legacy Read

If needed, legacy facade reads directly from Aequora authority/projection.

---

# 100. Legacy Session/Auth

Existing authentication may remain.

Compatibility facade converts legacy identity to:

```text
AuthContext
ActorId
TenantId
```

---

# 101. Identity Mapping

Logical:

```text
LegacyPrincipalId
→
Aequora PrincipalId
```

---

# 102. Never Trust Legacy Role String Blindly

Authorization policy must explicitly map roles/claims.

---

# 103. Authentication Migration

Can happen independently from sync migration.

---

# 104. Legacy Permissions

Shadow-compare Aequora authorization decisions before cutover.

---

# 105. Legacy Error Semantics

Old API may expect:

```text
HTTP 409
custom error code
```

Facade maps typed Aequora error to legacy contract.

---

# 106. No Error Semantics Leakage

Core does not encode legacy error strings.

---

# 107. Legacy Transactions

Legacy endpoint may update multiple aggregates.

Need classify:

```text
true business transaction
or accidental SQL grouping
```

---

# 108. Aggregate Boundary Review

If business requires atomic multi-entity transition:

```text
model as StrongAggregate
or
workflow
```

not blindly copy SQL transaction shape.

---

# 109. Stored Procedures

Legacy business logic may live in DB procedures/triggers.

Must inventory them.

---

# 110. Hidden Logic Risk

Cutover before understanding triggers can create semantic divergence.

---

# 111. Trigger Discovery

Migration tooling should inspect:

```text
triggers
procedures
constraints
generated columns
```

where DB supports discovery.

---

# 112. Application Hooks

Also inspect:

```text
ORM callbacks
event listeners
scheduled jobs
```

---

# 113. Legacy Side Effects

Old write may send:

```text
email
payment
webhook
```

during transaction path.

Aequora migration must disable duplicate side effects.

---

# 114. Side-Effect Ownership

Define:

```text
Legacy
Aequora
DisabledDuringShadow
```

per effect.

---

# 115. Cutover Side Effects

Switch ownership atomically with write-path fence as operationally possible.

---

# 116. Payment Migration

Especially dangerous.

Recommended:

```text
migrate payment command path separately
shadow first
provider idempotency stable
```

---

# 117. Existing Idempotency Keys

Preserve provider idempotency where possible.

---

# 118. External Reference Mapping

Store:

```text
legacy payment ID
provider reference
Aequora OperationId
```

---

# 119. Background Jobs

Legacy cron/jobs may mutate migrated aggregates.

They must be:

```text
disabled
or
rewired through Aequora
```

---

# 120. Job Inventory

Before cutover list every writer:

```text
API
cron
admin script
DB trigger
batch import
support tool
```

---

# 121. Writer Inventory Completeness

Cutover blocked until all known writers classified.

---

# 122. Unknown Writer Detection

Use:

```text
DB audit/logging
CDC origin metadata
```

during shadow period.

---

# 123. Post-Cutover Write Alarm

If legacy DB records forbidden direct write:

```text
critical alert
```

---

# 124. Database Trigger Fence

A trigger can reject updates to migrated rows/tables except dedicated bridge role.

Useful defense-in-depth.

---

# 125. Compatibility Views

Legacy SQL/reporting may expect old schema.

Can create:

```text
views
materialized projections
```

from Aequora-owned data.

---

# 126. Read-Only SQL Compatibility

Safer than writable compatibility views.

---

# 127. Reporting Tools

Legacy BI can continue through read replica/projection.

---

# 128. Bulk Import Compatibility

Existing CSV/import workflows can translate into Aequora Part 09 import jobs.

---

# 129. Admin Script Compatibility

Replace raw update scripts with:

```text
Aequora admin/domain command
```

---

# 130. Legacy Direct DB Clients

These are high risk.

After cutover:

```text
revoke write credentials
```

---

# 131. Migration Manifest

Create durable:

```text
LegacyMigrationManifest
```

---

# 132. Manifest Fields

```text
migration_id
legacy_system_id
aggregate types
source schema digest
mapping version
cutover position
ownership generation
created_at
```

---

# 133. MigrationId

```rust
pub struct LegacyMigrationId(Uuid);
```

---

# 134. MappingVersion

```rust
pub struct LegacyMappingVersion(u32);
```

---

# 135. Source Schema Digest

Capture structural schema/config digest.

Useful for detecting legacy schema drift during migration.

---

# 136. Schema Drift

If legacy schema changes unexpectedly:

```text
pause bridge
```

until mapper verified.

---

# 137. Drift Policy

Compatible additive column may be ignored.

Breaking type/semantics change:

```text
fail closed
```

---

# 138. CDC Decoder Version

Version separately.

---

# 139. LegacyBridgeState

```rust
pub enum LegacyBridgeState {
    Disabled,
    Shadow,
    Following,
    CaughtUp,
    CutoverPending,
    Fenced,
    Retired,
    Failed,
}
```

---

# 140. Bridge Health

Expose:

```text
source position
applied position
lag
last error
quarantine count
```

---

# 141. Quarantine

Bad legacy record does not silently disappear.

Store:

```text
record key
source position
mapping error
```

---

# 142. Quarantine Threshold

Cutover blocked if unresolved critical records exceed threshold.

---

# 143. Tolerant Import vs Live Bridge

Initial migration may tolerate quarantined noncritical rows.

Live CDC for authoritative compatibility should be stricter.

---

# 144. Legacy Change Reconciliation

Periodic:

```text
source canonical digest
target canonical digest
```

for migrated/followed dataset.

---

# 145. Cutover Verification

Run:

```text
count checks
referential checks
domain invariants
canonical digest
CDC caught-up
writer fence test
```

---

# 146. Readiness Report

CLI output:

```text
CDC lag: 0
shadow mismatch: 0.02%
unmapped writers: 0
quarantine critical: 0
source schema drift: none
```

---

# 147. Migration CLI

Suggested:

```text
aequora legacy discover
aequora legacy map verify
aequora legacy bridge start
aequora legacy shadow report
aequora legacy cutover plan
aequora legacy cutover
aequora legacy verify
aequora legacy retire
```

---

# 148. Cutover Plan

Part 24 plan/approval applies.

---

# 149. High-Risk Cutover

Could require:

```text
two-person approval
maintenance window
```

for finance/critical systems.

---

# 150. Migration Audit

Every stage transition audited.

---

# 151. Legacy Bridge Storage

Logical records:

```text
aequora_legacy_system
aequora_legacy_id_map
aequora_legacy_bridge_cursor
aequora_legacy_bridge_ledger
aequora_legacy_quarantine
aequora_aggregate_ownership
aequora_legacy_migration
```

---

# 152. `aequora_legacy_system`

Fields:

```text
legacy_system_id
kind
schema_digest
status
registered_at
```

---

# 153. `aequora_legacy_bridge_cursor`

Fields:

```text
legacy_system_id
stream_id
source_position
updated_at
```

---

# 154. `aequora_legacy_bridge_ledger`

Fields:

```text
legacy_system_id
source_position
record_key
canonical_digest
status
applied_event_id
```

---

# 155. `aequora_aggregate_ownership`

Fields:

```text
tenant_id
aggregate_type
owner
ownership_generation
updated_at
```

---

# 156. Ownership Cache

Runtime may cache ownership map.

Versioned by generation.

---

# 157. Write Guard

Every migrated write path checks:

```text
Aequora owns aggregate?
```

Legacy facade knows current owner.

---

# 158. Legacy Direct Write Detection

If CDC observes change after fence:

```text
LegacyWriteAfterCutover
```

critical incident.

---

# 159. Reverse Sync

Do not maintain bidirectional row mirroring indefinitely.

That recreates multi-master.

---

# 160. One-Way Compatibility

After cutover:

```text
Aequora
→
legacy read projection
```

only.

---

# 161. Legacy Data Decommission

Before dropping old table:

```text
backup/export
retention check
legal hold
report dependency check
```

---

# 162. Governance

Part 14 applies to both:

```text
legacy copies
Aequora copies
bridge staging
```

during migration.

---

# 163. Erasure During Migration

Erasure must reach both systems until legacy copy retired.

---

# 164. Legal Hold

Hold selectors include legacy records.

---

# 165. Snapshot

After cutover create fresh Aequora snapshot.

---

# 166. Authority Epoch

Normal legacy aggregate cutover inside same authority may not require AuthorityEpoch change.

---

# 167. Authority Migration

If cutover also moves entire authoritative DB:

```text
Part 16 rules
```

apply.

---

# 168. Scope Generation

If projection semantics change due migration:

```text
ScopeGeneration
```

may need bump/rebootstrap.

---

# 169. Operation Compatibility

Legacy facade may generate current Aequora operation schema.

Old legacy request schema stays edge-only.

---

# 170. Idempotency for Legacy API

If legacy API already has request ID:

```text
map to stable OperationId
```

where semantics allow.

---

# 171. No Legacy Request ID

Facade generates OperationId.

Retry detection may use:

```text
client-provided idempotency key
```

if available.

---

# 172. Duplicate Old Requests

Without idempotency key, some legacy APIs inherently cannot guarantee duplicate suppression.

Document limitation.

---

# 173. Legacy GET/PUT Semantics

PUT may naturally map to:

```text
SetState
```

with base version.

---

# 174. Legacy PATCH

Map fields explicitly.

---

# 175. Legacy SQL Null Semantics

Be careful distinguishing:

```text
missing
null
empty
default
```

when mapping.

---

# 176. Legacy Boolean Flags

Often encode multi-state business concepts.

Normalize before Aequora ownership.

---

# 177. Legacy Status Codes

Map to explicit enum.

Unknown code:

```text
quarantine
```

not arbitrary default.

---

# 178. Legacy Decimal

Validate units/scale.

---

# 179. Legacy Time

Normalize:

```text
timezone
precision
DST assumptions
```

explicitly.

---

# 180. Text Encoding

Validate UTF-8 or define conversion.

---

# 181. Binary Data

Move large binaries to blob subsystem.

---

# 182. Legacy Attachments

Migration records:

```text
source file ID
blob digest
new BlobRef
```

---

# 183. Missing Blob

Quarantine or placeholder policy.

---

# 184. Data Quality Report

Before cutover:

```text
invalid enum count
missing refs
duplicate natural keys
bad dates
oversized payloads
```

---

# 185. Repair Policy

Repairs to legacy data should be:

```text
explicit
audited
```

not silently coerced in mapper.

---

# 186. Mapping Rule

Example:

```ron
(
    source: "student.status",
    target: StudentStatus,
    mapping: {
        1: Active,
        2: Suspended,
        9: Archived,
    },
)
```

---

# 187. Mapping Code vs Config

Simple static mapping may be config.

Complex business semantics should be typed Rust.

---

# 188. No User-Supplied Arbitrary Mapping Code

Keep trusted build-time/application code.

---

# 189. Performance

CDC bridge should be:

```text
streaming
bounded
checkpointed
```

---

# 190. Backpressure

Part 18 applies.

If target overloaded:

```text
bridge pauses
source cursor not advanced
```

---

# 191. Bulk Catch-Up

Initial backlog can use larger batches.

Live tail uses smaller low-latency batches.

---

# 192. Catch-Up State

```text
Historical
NearRealtime
CaughtUp
```

---

# 193. Lag Threshold

Cutover only when:

```text
CaughtUp
```

and final fence applied.

---

# 194. Multi-Tenant Legacy DB

Bridge partitions by tenant where possible.

---

# 195. Tenant Isolation

Legacy source row must map to exactly one TenantId.

Ambiguous tenant:

```text
quarantine
```

---

# 196. Security

Legacy DB credentials restricted:

```text
read-only for follower
```

before cutover.

---

# 197. Cutover Credentials

After Aequora ownership:

```text
legacy writer creds revoked
```

---

# 198. Secret Handling

Bridge config stores secret references only.

---

# 199. Threat: Compromised Legacy App

After cutover, it must not be able to mutate authority directly.

---

# 200. Observability

Metrics:

```text
legacy_cdc_lag
legacy_bridge_events_total
legacy_mapping_failure_total
legacy_shadow_mismatch_total
legacy_write_after_cutover_total
```

---

# 201. Logs

Structured:

```text
legacy_bridge_started
legacy_change_quarantined
legacy_cutover_fenced
legacy_write_after_cutover
legacy_retired
```

---

# 202. Alerting

Critical:

```text
write after cutover
CDC stalled
schema drift
bridge digest mismatch
```

---

# 203. Shadow Mismatch Metrics

Use bounded labels:

```text
aggregate type
operation kind
```

not record ID.

---

# 204. Migration Dashboard

Show:

```text
records mapped
quarantine
CDC lag
shadow equivalence
ownership state
```

---

# 205. Incident Integration

Part 25 incident bundle can include:

```text
legacy source position
mapping version
bridge ledger
ownership generation
```

---

# 206. Replay

Legacy change can be replayed against mapper in sandbox.

---

# 207. Golden Legacy Fixtures

Store sanitized fixture rows for migration tests.

---

# 208. Mapping Regression

Current mapper must preserve expected canonical result.

---

# 209. Schema Drift Test

New legacy column/type appears.

Expected policy-specific response.

---

# 210. CDC Duplicate Test

Deliver same source event twice.

Expected:

```text
one logical apply
```

---

# 211. CDC Crash Test

Crash after target apply before source cursor checkpoint.

Expected:

```text
redelivery deduplicated
```

---

# 212. CDC Gap Test

Skip source position.

Expected:

```text
detect gap
pause
```

where source position semantics allow.

---

# 213. Reordering Test

Out-of-order source records.

Expected:

```text
version/order guard
```

---

# 214. Cutover Race Test

Legacy write occurs while fence activating.

Expected:

```text
either included before final boundary
or rejected
```

never silently lost.

---

# 215. Stale Legacy Node Test

Old app writes after cutover.

Expected:

```text
DB/bridge fence rejects
critical alert
```

---

# 216. Legacy Facade Retry Test

Same idempotency key.

Expected:

```text
same OperationId/effect
```

---

# 217. Reverse Projection Test

Aequora write updates legacy read projection.

Old client sees consistent result.

---

# 218. Governance Test

Erase subject during migration.

Expected:

```text
legacy + Aequora surfaces both included
```

---

# 219. Rollback Test

Before point-of-no-return, rollback plan restores legacy ownership safely.

---

# 220. Post-Cutover Rollback Test

If reverse bridge unsupported:

```text
tool refuses unsupported rollback
```

---

# 221. Correctness Invariants

Add:

## AEQ-INV-LEG001

```text
Each migrated aggregate has exactly one effective authoritative write owner at any logical point in time.
```

## AEQ-INV-LEG002

```text
Legacy CDC source position advances only after the corresponding canonical bridge result is durably recorded.
```

## AEQ-INV-LEG003

```text
Duplicate delivery of the same legacy change cannot produce duplicate canonical authoritative effects.
```

## AEQ-INV-LEG004

```text
After Aequora ownership cutover, direct legacy writes to the migrated aggregate are fenced or treated as critical integrity violations.
```

## AEQ-INV-LEG005

```text
Legacy compatibility APIs translate into typed Aequora operations rather than bypassing domain handlers.
```

## AEQ-INV-LEG006

```text
Legacy-to-Aequora mapping never silently coerces an unknown business state into a valid canonical state.
```

---

# 222. Additional Invariants

## AEQ-INV-LEG007

```text
Shadow execution never produces real external side effects or authoritative mutations.
```

## AEQ-INV-LEG008

```text
Governance and legal-hold policy accounts for legacy copies until those copies are formally retired.
```

## AEQ-INV-LEG009

```text
A cutover is not declared complete until source writers are fenced, the final source boundary is applied, and canonical verification succeeds.
```

---

# 223. Recommended Crates

```text
aequora-legacy/
├── system.rs
├── reader.rs
├── mapping.rs
├── id_map.rs
├── ownership.rs
├── bridge.rs
├── cursor.rs
├── ledger.rs
├── shadow.rs
├── cutover.rs
├── quarantine.rs
└── errors.rs
```

---

# 224. CDC Provider Crates

Examples:

```text
aequora-cdc-postgres
aequora-cdc-mysql
aequora-cdc-sqlserver
aequora-cdc-polling
```

Core depends only on trait.

---

# 225. Legacy Facade Crate

```text
aequora-legacy-api/
├── dto.rs
├── translate.rs
├── errors.rs
└── response.rs
```

---

# 226. Migration Tooling

```text
aequora-cli/
└── legacy/
    ├── discover.rs
    ├── map.rs
    ├── bridge.rs
    ├── shadow.rs
    ├── cutover.rs
    ├── verify.rs
    └── retire.rs
```

---

# 227. Admin Integration

Part 24 endpoints:

```text
GET /legacy/systems
GET /legacy/bridge/status
POST /legacy/cutover/plan
POST /legacy/cutover/execute
```

---

# 228. Control Plane Permissions

Examples:

```text
Legacy.View
Legacy.Bridge
Legacy.Cutover
Legacy.Retire
```

---

# 229. Cutover Approval

High-value aggregate may require:

```text
Legacy.Cutover
+
second approval
```

---

# 230. Data Model

Logical records:

```text
aequora_legacy_system
aequora_legacy_id_map
aequora_legacy_bridge_cursor
aequora_legacy_bridge_ledger
aequora_legacy_quarantine
aequora_aggregate_ownership
aequora_legacy_migration
```

---

# 231. Retention

Bridge ledger may be compacted after:

```text
cutover
retry horizon
audit requirement
```

---

# 232. Legacy ID Map Retention

Keep as long as:

```text
legacy API/reporting/external references
```

need it.

---

# 233. Retirement Manifest

When legacy retired, create:

```text
LegacyRetirementManifest
```

with:

```text
final source position
archive reference
ownership state
retention decision
```

---

# 234. Backup

Before cutover:

```text
backup legacy source
Aequora target
mapping manifest
```

---

# 235. Restore

Restoring legacy environment after Aequora cutover must not reopen legacy writes automatically.

---

# 236. Environment Clone Safety

Staging clone gets:

```text
different AuthorityId
disabled external side effects
disabled CDC back to production
```

---

# 237. Legacy Prod Clone

Critical to prevent accidental write-back.

---

# 238. Development Fixtures

Use sanitized copies, not production CDC.

---

# 239. Migration Phasing by Aggregate

Recommended order:

```text
low-risk independent entities
↓
moderate CRUD aggregates
↓
workflow aggregates
↓
finance/payment
```

---

# 240. Why

Build confidence and migration tooling before highest-risk domains.

---

# 241. School ERP Example

Possible migration order:

```text
school metadata
subjects/classes
students
attendance
documents
fees/accounting
payments
```

---

# 242. Finance Example

Finance should migrate with:

```text
append-only profile
ledger verification
payment provider reconciliation
strict audit
```

---

# 243. Legacy ERP Example

Keep old desktop client alive through facade while new Dioxus local-first client uses native Aequora sync.

---

# 244. Progressive Client Migration

```text
legacy client
↓
legacy API facade
↓
Aequora authority
```

and:

```text
new client
↓
Aequora sync
```

both coexist safely.

---

# 245. Final Desired State

Eventually:

```text
Aequora domain authority
Aequora sync
legacy API facade optional
legacy DB read projection optional
```

then remove compatibility layers.

---

# 246. Avoid Permanent Migration Architecture

Every bridge should have:

```text
owner
retirement criteria
sunset plan
```

---

# 247. Migration Debt

Track:

```text
remaining legacy writers
remaining read dependencies
old ID mappings
unsupported rollback paths
```

---

# 248. Completion Criteria

Part 26 is complete when:

```text
[ ] staged adoption model defined
[ ] legacy reader/mapping interfaces defined
[ ] CDC bridge/cursor/ledger defined
[ ] provenance defined
[ ] aggregate write ownership defined
[ ] shadow mode defined
[ ] cutover fencing defined
[ ] rollback/point-of-no-return defined
[ ] legacy ID mapping defined
[ ] legacy auth/API facade defined
[ ] side-effect/job migration defined
[ ] governance during coexistence defined
[ ] migration persistence schema defined
[ ] readiness/verification tooling defined
[ ] failure/race tests defined
[ ] legacy compatibility invariants added
```

---

# 249. Final Architecture

```text
                   EXISTING LEGACY SYSTEM
                           │
                  current authoritative writer
                           │
                           ▼
                     CDC / Reader
                           │
                           ▼
                   Canonical Mapper
                           │
                           ▼
                    Aequora Shadow
                           │
                  compare / verify
                           │
                           ▼
                     CUTOVER PLAN
                           │
                    fence legacy writes
                           │
                           ▼
             apply final source position
                           │
                           ▼
                  canonical verification
                           │
                           ▼
                 Aequora Authority
                   /              \
                  /                \
        Native Aequora Client   Legacy API Facade
                  \                /
                   \              /
                    same domain handlers

After migration:

Aequora Journal
      │
      ▼
optional legacy read projection
      │
      ▼
old reports / clients during sunset
```

The architectural principle is:

> **Aequora should replace legacy authority one aggregate at a time, with explicit ownership and verifiable cutovers, rather than creating an uncontrolled permanent dual-write system.**

This enables existing applications to adopt local-first synchronization, typed domain operations, idempotency, auditability, and modern client architecture incrementally—while preserving the ability to keep old clients and read models working until they can be safely retired.
