# Aequora Sync — Part 36

# Storage Adapter SDK, Official Adapter Architecture, Capability Contracts, and Conformance Integration

## 1. Purpose

Aequora is intended to synchronize applications regardless of which database is used on either side.

That only works if database integration is treated as a first-class extension system rather than a collection of ad-hoc implementations.

The central question is:

> **How does a database become an Aequora storage adapter without leaking its physical model into the sync engine?**

This part defines the Storage Adapter SDK.

It covers:

```text
local adapters
authoritative adapters
journal/ledger adapters
snapshot adapters
blob stores
capability manifests
transaction semantics
migration contracts
error normalization
conformance
official adapter policy
third-party adapter policy
```

The central rule is:

> **A storage adapter is certified by the semantics it preserves, not by the database technology it uses.**

---

# 2. Adapter Roles

A database adapter may implement one or more roles.

```text
LocalReplicaStore
AuthoritativeStore
JournalStore
OperationLedgerStore
SnapshotStore
BlobMetadataStore
IntegrityStore
GovernanceStore
```

Not every adapter must implement every role.

---

# 3. Role Separation

Examples:

```text
PostgreSQL
    AuthoritativeStore
    JournalStore
    OperationLedgerStore
    AuditStore

Stoolap
    LocalReplicaStore
    OutboxStore
    CursorStore
    ConflictStore

SQLite
    LocalReplicaStore
    OutboxStore
    CursorStore

Object Storage
    SnapshotArtifactStore
    BlobObjectStore
```

---

# 4. Why Capability-Oriented Adapters

A giant trait like:

```rust
trait AequoraDatabase {
    // everything
}
```

would force meaningless methods and hide semantic differences.

Use narrow capability traits instead.

---

# 5. Adapter SDK Crate

Create:

```text
aequora-adapter-sdk
```

It should expose stable extension contracts.

---

# 6. Adapter SDK Must Not Depend on Official Adapters

Dependency direction:

```text
aequora-adapter-sdk
        ▲
        │
aequora-postgres
aequora-stoolap
aequora-sqlite
```

---

# 7. Adapter SDK Public Types

It should expose:

```text
transaction capability traits
record models
cursor/journal contracts
migration hooks
capability manifest
error types
conformance hooks
```

---

# 8. Local Transaction Trait

Concept:

```rust
#[async_trait]
pub trait LocalTransactionStore: Send + Sync {
    type Tx<'a>: LocalTransaction + Send
    where
        Self: 'a;

    async fn begin_local_tx(
        &self,
    ) -> Result<Self::Tx<'_>, AdapterError>;
}
```

Exact shape may evolve.

---

# 9. Local Transaction Contract

It must support atomic:

```text
domain mutation
+
outbox insert
```

---

# 10. Local Transaction API

Concept:

```rust
#[async_trait]
pub trait LocalTransaction {
    async fn commit(self) -> Result<(), AdapterError>;
    async fn rollback(self) -> Result<(), AdapterError>;
}
```

Repositories use the same transaction handle.

---

# 11. Authoritative Transaction Trait

Must support atomic:

```text
business mutation
+
entity/aggregate version update
+
journal append
+
operation ledger write
+
required audit write
```

---

# 12. Authoritative Tx Capability

This is stronger than ordinary CRUD.

An adapter that cannot preserve this atomic boundary cannot claim full authoritative capability.

---

# 13. Outbox Store

Methods conceptually:

```text
enqueue
claim batch
mark in-flight
mark accepted
mark rejected
release stale claim
query by OperationId
```

---

# 14. Cursor Store

Must support:

```text
read cursor
compare/update
atomic update with reconcile transaction
```

---

# 15. Journal Store

Must provide:

```text
append event
scan after cursor
monotonic committed sequence
retention floor
```

---

# 16. Journal Ordering Warning

Do not confuse:

```text
sequence allocation order
```

with:

```text
transaction commit order
```

A physical implementation must preserve Aequora's logical cursor semantics.

---

# 17. Operation Ledger Store

Must support:

```text
lookup OperationId
insert outcome
payload digest check
idempotency replay
```

---

# 18. Payload Reuse Rule

Same OperationId with different canonical payload digest:

```text
must fail
```

---

# 19. Snapshot Store

Logical capabilities:

```text
create manifest
stream chunks
publish immutable generation
activate generation
resume install
```

---

# 20. Integrity Store

Optional:

```text
entity digest
partition digest
Merkle root
```

---

# 21. Fencing Store

For local multi-process or authority coordination:

```text
lease acquire
lease renew
fencing token
ownership verification
```

---

# 22. Governance Store

Optional enterprise capability:

```text
legal hold
erasure directives
retention metadata
```

---

# 23. Capability Manifest

Each adapter publishes machine-readable capabilities.

Example:

```rust
pub struct AdapterCapabilities {
    pub atomic_local_outbox: bool,
    pub atomic_authoritative_commit: bool,
    pub compare_and_swap: bool,
    pub snapshot_install: bool,
    pub fencing: bool,
    pub integrity_digest: bool,
}
```

---

# 24. Better Than Booleans

For richer evolution, capabilities should use stable IDs/versions.

```text
CapabilityId
CapabilityVersion
```

---

# 25. Capability Is a Claim

Declaring:

```text
SupportsAtomicAuthoritativeCommit
```

does not make it true.

Conformance must test it.

---

# 26. Capability Levels

Some capabilities can have levels.

Example:

```text
Snapshot:
    None
    Logical
    Streaming
    AtomicGenerationSwap
```

---

# 27. Local Adapter Minimum Profile

Required:

```text
atomic local transaction
outbox
cursor
schema migration
crash reopen
unique OperationId
```

---

# 28. Authoritative Adapter Minimum Profile

Required:

```text
atomic authority transaction
journal
ledger
version control
idempotency
range scan
migration
```

---

# 29. Full Enterprise Profile

Additional:

```text
fencing
governance
audit
snapshot
integrity
backup integration
```

---

# 30. Adapter Identity

Define:

```rust
pub struct AdapterDescriptor {
    pub adapter_id: AdapterId,
    pub name: &'static str,
    pub version: AdapterVersion,
    pub store_kind: StoreKind,
}
```

---

# 31. AdapterId

Stable identifier, not crate name string.

---

# 32. Physical DB Version

Certification should record:

```text
adapter version
database engine version
OS/platform
feature flags
```

---

# 33. Configuration Boundary

Adapter config belongs to adapter crate.

Example:

```rust
PostgresAdapterConfig
StoolapAdapterConfig
```

Do not put every database config field into core config.

---

# 34. Common Config

Only truly generic values belong in core:

```text
timeouts
batch size
storage profile
```

---

# 35. Error Normalization

Adapters return canonical categories.

Example:

```rust
pub enum AdapterErrorKind {
    Unavailable,
    Conflict,
    DiskFull,
    ReadOnly,
    Corruption,
    ConstraintViolation,
    SerializationFailure,
    Timeout,
    Unsupported,
    Internal,
}
```

---

# 36. Preserve Source

Canonical error can retain:

```text
source chain
adapter code
diagnostic context
```

internally.

---

# 37. Do Not Expose SQLSTATE Everywhere

PostgreSQL SQLSTATE may be useful inside adapter diagnostics, but core should reason about canonical error categories.

---

# 38. Retry Classification

Adapter can map physical errors to:

```text
Retryable
NonRetryable
NeedsReopen
NeedsRecovery
```

---

# 39. Transaction Retry

Serialization/deadlock retry belongs in a deliberate layer.

Do not automatically retry arbitrary domain execution unless operation execution is safe/deterministic.

---

# 40. Authoritative Retry

Recommended flow:

```text
begin tx
execute deterministic plan
serialization failure
rollback
retry with bounded policy
```

---

# 41. Adapter Must Not Hide Ambiguous Commit

If commit outcome is uncertain, return:

```text
CommitOutcomeUnknown
```

where applicable.

Idempotency layer handles retry safely.

---

# 42. Migration Contract

Each adapter owns its physical schema migrations.

---

# 43. Migration Descriptor

```rust
pub struct AdapterMigration {
    pub id: MigrationId,
    pub checksum: Digest,
    pub from: AdapterSchemaVersion,
    pub to: AdapterSchemaVersion,
}
```

---

# 44. Migration Rules

```text
IDs never reused
body checksum stable
order deterministic
failure recoverable
```

---

# 45. Local Migration

Must preserve:

```text
pending outbox
cursor
conflicts
store identity
```

---

# 46. Server Migration

Must preserve:

```text
journal continuity
operation ledger
authority epoch
retention floor
```

---

# 47. Online Migration

Optional capability.

If supported:

```text
old/new server versions may coexist
```

---

# 48. Offline Migration

Acceptable for local embedded store if startup migration is bounded.

---

# 49. Adapter Schema vs Domain Schema

Separate.

```text
AdapterSchemaVersion
DomainSchemaVersion
```

must not be conflated.

---

# 50. Reference Record Models

Adapter SDK may define logical metadata records.

Example:

```rust
pub struct OutboxRecord {
    pub operation_id: OperationId,
    pub local_sequence: LocalOperationSequence,
    pub state: OutboxState,
    pub payload: Bytes,
    pub payload_digest: Digest,
}
```

---

# 51. Logical Records Are Not SQL Rows

Adapters map them to any physical representation.

---

# 52. Binary Payload Storage

Prefer:

```text
Postcard bytes
```

for canonical operation payloads.

---

# 53. Adapter May Normalize

Physical DB may store some indexed fields separately.

Canonical payload remains authoritative for operation replay.

---

# 54. Index Requirements

Adapter documentation should state required indexes.

---

# 55. Local Index Examples

```text
outbox(state, local_sequence)
outbox(operation_id)
cursor(scope_id)
conflict(status, entity)
```

---

# 56. Server Index Examples

```text
ledger(operation_id)
journal(sequence)
journal(tenant_id, sequence)
device(device_id)
```

---

# 57. Index Validation

Startup can optionally verify required schema/index generation.

---

# 58. Storage Capability Trait Example

```rust
pub trait SupportsAtomicLocalOutbox {}
pub trait SupportsAtomicAuthorityCommit {}
pub trait SupportsFencing {}
```

Marker traits can help compile-time composition.

---

# 59. Runtime Validation Still Required

Compile-time marker cannot prove:

```text
filesystem behavior
DB setting
version bug
deployment configuration
```

---

# 60. Official Adapter Policy

Official adapters should be maintained in Aequora repository or tightly controlled ecosystem.

Initial likely set:

```text
PostgreSQL/Neon
Stoolap
SQLite
```

---

# 61. Why PostgreSQL First

It is the authoritative reference server implementation.

---

# 62. Why Stoolap

It is a candidate preferred local embedded engine.

---

# 63. Why SQLite

It provides a broadly mature portability baseline.

---

# 64. Redb/Fjall Later

Add only when there is a clear product/performance use case.

---

# 65. Official Adapter Requirements

Must have:

```text
maintainer ownership
conformance coverage
migration tests
fault tests
performance baseline
documentation
support matrix
```

---

# 66. Community Adapter

May live externally.

Must implement adapter SDK.

Can run official conformance suite.

---

# 67. Certification Labels

Possible:

```text
Experimental
CommunityVerified
MaintainerVerified
Official
```

---

# 68. No Brand Trust

An adapter is not trusted just because:

```text
database is famous
crate is popular
```

Semantics still need verification.

---

# 69. Adapter Conformance Harness

Create factory traits.

Example:

```rust
#[async_trait]
pub trait LocalStoreFactory {
    type Store: LocalStore;

    async fn create_clean(
        &self,
    ) -> Result<Self::Store, TestError>;
}
```

---

# 70. Local Conformance Tests

Must include:

```text
Tx A atomicity
duplicate OperationId
crash reopen
cursor atomicity
migration
disk/full simulation where possible
```

---

# 71. Authoritative Conformance Tests

Must include:

```text
Tx B atomicity
journal+ledger
duplicate retry
payload mismatch
deadlock/serialization behavior
version CAS
retention floor
```

---

# 72. Snapshot Conformance

Tests:

```text
publish
partial write
resume
corrupt chunk
atomic activation
```

---

# 73. Fencing Conformance

Tests:

```text
lease takeover
stale writer
token monotonicity
```

---

# 74. Multi-Process Conformance

Important for desktop local adapters.

---

# 75. Mobile Conformance

Important for Android/iOS adapters.

---

# 76. Platform Certification Matrix

Certification is not just crate-level.

Example:

```text
StoolapAdapter 0.x
Linux x86_64
DesktopLocalStoreFull
PASS

StoolapAdapter 0.x
Android arm64
MobileLocalStoreFull
PASS
```

---

# 77. Failure Injection Interface

Adapters may implement test-only failpoints.

Examples:

```text
before commit
after metadata write
before cursor update
after journal append
```

---

# 78. Failpoints Must Be Test-Only

Never expose dangerous fault injection in production build by default.

---

# 79. Differential Testing

Run same logical workload against:

```text
ReferenceStore
PostgreSQL
Stoolap
SQLite
```

Compare observable outcomes.

---

# 80. Reference Store

An in-memory deterministic implementation acts as semantic oracle.

---

# 81. Reference Store Is Not Production

It can trade performance for clarity/determinism.

---

# 82. Adapter Boundary and Domain Repositories

Aequora metadata adapter should not become application ORM.

Application persistence can have separate repository implementations sharing same physical transaction.

---

# 83. Shared Transaction Challenge

Critical:

```text
Aequora metadata
+
application domain write
```

must share transaction.

---

# 84. Solution Pattern

Adapter transaction exposes application-specific repository binding.

Concept:

```rust
pub trait AuthorityTxn {
    fn metadata(&mut self) -> &mut dyn AuthorityMetadataTxn;
}
```

Application may wrap concrete transaction internally without leaking DB type through core.

---

# 85. Alternative Repository Factory

```rust
pub trait DomainRepositoryFactory<Tx> {
    type Repo;
    fn repo(&self, tx: &mut Tx) -> Self::Repo;
}
```

---

# 86. Keep Core Generic Over Transaction Capability

Aequora should not know application's schema.

---

# 87. Postgres App Domain

Application server may use SQLx in its own adapter/repository crate.

But Aequora handler public boundary should remain storage-neutral.

---

# 88. Local Domain Transaction

Same principle.

Application local domain write and outbox must share local DB transaction.

---

# 89. Adapter Hooks

Useful hooks:

```text
before migration
after migration
health check
schema verify
```

---

# 90. Adapter Health

Return structured:

```rust
pub struct AdapterHealth {
    pub reachable: bool,
    pub writable: bool,
    pub schema_ok: bool,
}
```

---

# 91. Startup Validation

Aequora server/client should fail early if required capabilities are missing.

---

# 92. Example

If deployment requires:

```text
fencing
```

but adapter manifest lacks it:

```text
startup fails
```

not silent downgrade.

---

# 93. Capability Negotiation vs Storage Capability

Do not confuse:

```text
client/server protocol capability
```

with:

```text
local physical adapter capability
```

They are related but distinct.

---

# 94. Storage Role Manifest

Application composition can request:

```text
LocalWritable
Authoritative
SnapshotCapable
```

and validate selected adapter.

---

# 95. Adapter Version Compatibility

Official adapter should document:

```text
supported DB engine versions
supported Aequora versions
supported platform targets
```

---

# 96. Database Upgrade

DB engine upgrades require conformance rerun.

---

# 97. Configuration Drift

A database setting may invalidate assumptions.

Examples:

```text
SQLite synchronous mode
Postgres isolation
filesystem durability mode
```

Adapter startup should validate critical settings where possible.

---

# 98. Unsafe Configuration

Fail closed if it breaks correctness.

---

# 99. Performance Profile

Conformance proves semantics.

Benchmark suite characterizes:

```text
latency
throughput
memory
disk write amplification
```

---

# 100. Performance Is Not Certification

A fast adapter that violates atomicity fails.

---

# 101. Adapter Observability

Expose:

```text
transaction duration
queue waits
commit retries
migration status
```

through standard tracing/metrics.

---

# 102. No Sensitive SQL Logging

Redact values where needed.

---

# 103. Connection Pooling

Authoritative adapter owns pooling integration.

Core gets transaction/service capability.

---

# 104. Local Embedded Pool

May simply be one connection or internal handle.

---

# 105. Concurrency Model

Adapter must document:

```text
single writer
multi writer
multi reader
multi process
```

---

# 106. Sync Scheduler Awareness

Core may use capability metadata to choose concurrency.

Example:

```text
single-writer embedded DB
→ serialize local writes
```

---

# 107. Backpressure

Adapter may signal:

```text
PoolExhausted
Busy
LockTimeout
```

canonicalized as resource errors.

---

# 108. Busy Handling

SQLite-like busy condition should use bounded policy.

Do not spin indefinitely.

---

# 109. Durability Class Mapping

Part 33 defines critical/standard/reconstructable data.

Adapter can map these to physical durability settings where supported.

---

# 110. Beware Weak Durability Overrides

Do not weaken critical writes for benchmark gains.

---

# 111. Backup Interface

Optional trait:

```rust
#[async_trait]
pub trait LocalBackupProvider {
    async fn create_backup(...);
    async fn restore_backup(...);
}
```

---

# 112. Backup Must Be Consistent

Hot backup requires certified snapshot semantics.

---

# 113. Restore Hook

Restored store must re-run:

```text
identity binding check
epoch check
schema check
```

---

# 114. Encryption Integration

Adapter may support native DB encryption or external encrypted filesystem/key wrapper.

---

# 115. Key Provider

Keep key management separate from storage core.

---

# 116. Secure Erase Capability

Optional capability.

May be implemented via cryptographic key destruction.

---

# 117. Object Storage Adapter

For snapshots/blobs, define separate object traits.

---

# 118. Object Store Capabilities

```text
put immutable object
get range
head metadata
delete
list prefix
```

---

# 119. Immutable Publication

Snapshot artifacts should be immutable after publication.

---

# 120. Object Store Does Not Become Authority

It stores artifacts, not authoritative operation execution.

---

# 121. CDC Adapter

Part 26 legacy bridge may use a separate:

```text
aequora-cdc-adapter-sdk
```

Do not overload ordinary storage adapter with CDC semantics.

---

# 122. Query Adapter

Likewise, external analytics/search stores are consumers, not authority adapters.

---

# 123. Adapter Security Boundary

Treat database errors/data as untrusted enough to validate bounds.

---

# 124. Corrupt Stored Payload

Decode must fail safely.

---

# 125. Length Limits

Adapters must not allocate unbounded memory from stored length fields.

---

# 126. Migration Security

Migration code is privileged.

Review like application code with full data access.

---

# 127. SQL Injection

Official SQL adapters use prepared/bound statements.

---

# 128. Identifier Construction

Dynamic table/index identifiers must be controlled from static schema, not user input.

---

# 129. Tenant Isolation

Authoritative adapter must preserve tenant boundaries in query design.

---

# 130. Optional PostgreSQL RLS

Can add defense in depth.

Aequora authz still remains application-level requirement.

---

# 131. Adapter Documentation Template

Every adapter should document:

```text
roles
capabilities
supported versions
transaction guarantees
migration strategy
backup behavior
multi-process behavior
known limitations
conformance status
```

---

# 132. Known Limitation Policy

Do not hide unsupported semantics.

Example:

```text
"No atomic generation swap"
```

must be explicit.

---

# 133. Capability Downgrade

If adapter lacks feature:

```text
Aequora either selects a safe alternative
or refuses configuration
```

Never silently emulate incorrectly.

---

# 134. Software Emulation

Capability may be implemented above DB primitive if semantically equivalent.

Example:

```text
logical fencing via metadata table + CAS
```

---

# 135. Emulation Must Be Certified

Physical primitive is not required; semantics are.

---

# 136. Third-Party Adapter Crate Naming

Suggested:

```text
aequora-adapter-<db>
```

for community ecosystem.

Official may use:

```text
aequora-postgres
aequora-stoolap
```

---

# 137. Adapter Registration

Normally compile-time via builder.

No dynamic plugin loading initially.

---

# 138. Example Client Composition

```rust
let store = StoolapLocalStore::open(config).await?;

let client = AequoraClient::builder()
    .store(store)
    .transport(http)
    .build()
    .await?;
```

---

# 139. Example Server Composition

```rust
let store = PostgresAuthorityStore::connect(config).await?;

let server = AequoraServer::builder()
    .authority_store(store)
    .domain(domain)
    .build()?;
```

---

# 140. Example Capability Failure

```text
Configured role:
    EnterpriseAuthority

Selected adapter:
    SQLiteLocal

Result:
    UnsupportedRole
```

---

# 141. Adapter Test API

Public test helpers may expose:

```rust
run_local_store_conformance(factory).await;
run_authority_store_conformance(factory).await;
```

---

# 142. Certification Artifact

Conformance produces:

```text
adapter identity
build hash
DB version
platform
test suite version
results
```

---

# 143. CI Integration

Official adapters must run conformance on every relevant change.

---

# 144. Release Gate

Official adapter release cannot ship if required conformance profile regresses.

---

# 145. Adapter SemVer

Adapter crate follows semver separately from protocol version.

---

# 146. Breaking Adapter API

May require major version even if Aequora protocol unchanged.

---

# 147. Adapter Capability Evolution

New capabilities can be added without breaking old deployments if optional.

---

# 148. Removing Capability

Breaking operational change.

Must be documented and may require major version.

---

# 149. Physical Schema Evolution

Can occur without public Rust API break.

Still requires migration/versioning.

---

# 150. Adapter Invariants

## AEQ-INV-ADAPTER001

```text
An adapter may claim a capability only if the corresponding conformance suite passes for the certified environment.
```

## AEQ-INV-ADAPTER002

```text
Local adapters preserve atomic domain mutation plus outbox insertion.
```

## AEQ-INV-ADAPTER003

```text
Authoritative adapters preserve atomic business mutation plus journal plus ledger plus required audit metadata.
```

## AEQ-INV-ADAPTER004

```text
Database-specific transaction and error types do not leak into storage-neutral core APIs.
```

## AEQ-INV-ADAPTER005

```text
Same OperationId with a different canonical payload digest is rejected.
```

---

# 151. Additional Adapter Invariants

## AEQ-INV-ADAPTER006

```text
Physical schema migration preserves all durable Aequora identity, cursor, outbox, and authority semantics.
```

## AEQ-INV-ADAPTER007

```text
Unsupported required capabilities fail startup rather than silently degrading correctness.
```

## AEQ-INV-ADAPTER008

```text
Conformance results are bound to adapter version, physical database version, platform, and relevant feature configuration.
```

## AEQ-INV-ADAPTER009

```text
Performance optimizations cannot weaken the durability class of critical intent.
```

## AEQ-INV-ADAPTER010

```text
Official adapters expose stable documented capability manifests and known limitations.
```

---

# 152. Recommended Crate Layout

```text
crates/
├── aequora-adapter-sdk/
│   ├── capabilities.rs
│   ├── local.rs
│   ├── authority.rs
│   ├── journal.rs
│   ├── ledger.rs
│   ├── snapshot.rs
│   ├── fencing.rs
│   ├── migration.rs
│   └── errors.rs
│
├── aequora-postgres/
├── aequora-stoolap/
├── aequora-sqlite/
├── aequora-object-store/
└── aequora-conformance/
```

---

# 153. `aequora-postgres` Layout

```text
src/
├── pool.rs
├── tx.rs
├── journal.rs
├── ledger.rs
├── versions.rs
├── snapshots.rs
├── migrations.rs
├── errors.rs
└── capabilities.rs
```

---

# 154. `aequora-stoolap` Layout

```text
src/
├── connection.rs
├── tx.rs
├── outbox.rs
├── cursor.rs
├── conflicts.rs
├── bootstrap.rs
├── migrations.rs
├── errors.rs
└── capabilities.rs
```

---

# 155. `aequora-sqlite` Layout

Similar role-oriented structure.

---

# 156. Avoid One Giant Adapter File

Keep semantic responsibilities separated.

---

# 157. Internal SQL/Query Modules

Physical query helpers remain private.

---

# 158. Adapter Startup

Recommended:

```text
open/connect
↓
read adapter metadata
↓
verify physical schema
↓
run/plan migrations
↓
validate critical configuration
↓
publish capabilities
```

---

# 159. Adapter Shutdown

Gracefully close pools/handles.

Correctness still survives forced process termination.

---

# 160. Health Check

A health check should not perform destructive write unless necessary.

Separate:

```text
liveness
readiness
deep diagnostics
```

---

# 161. Readiness

Authoritative adapter ready only if:

```text
schema valid
writable
required capabilities active
```

---

# 162. Local Open Result

May return:

```text
Ready
MigrationRequired
RecoveryRequired
BindingMismatch
```

---

# 163. Recovery Integration

Adapter should produce structured repair diagnostics, not auto-delete corrupted store.

---

# 164. Adapter SDK Ergonomics

Third-party author should be able to:

```text
implement traits
run conformance
generate report
```

without reading internal crates.

---

# 165. Adapter Author Quickstart

Documentation should include:

```text
minimal local adapter
minimal authority adapter
conformance runner
capability manifest
```

---

# 166. Certification Example

```text
Adapter: aequora-stoolap 0.1.0
Engine: Stoolap X.Y
Platform: Android arm64
Profile: MobileLocalStoreFull
Suite: 1.0
Result: PASS
```

---

# 167. Adapter Ecosystem Rule

Aequora should support many databases through:

```text
O(number of databases)
```

adapters, not pairwise sync implementations.

---

# 168. Why

Without canonical semantics:

```text
SQLite ↔ Postgres
SQLite ↔ MySQL
Stoolap ↔ Postgres
Stoolap ↔ MySQL
```

becomes pairwise complexity.

With Aequora:

```text
each DB ↔ Aequora contract
```

---

# 169. Official Adapter Priority

Recommended order:

```text
1. PostgreSQL/Neon authoritative
2. Stoolap local
3. SQLite local
4. object storage snapshot/blob
```

Then evaluate demand.

---

# 170. Future Adapters

Possible:

```text
Redb
Fjall
RocksDB-like Rust wrappers
SurrealDB bridge
MySQL authority
CockroachDB authority
```

only after semantic fit analysis.

---

# 171. MySQL/Cockroach Warning

Do not assume PostgreSQL adapter code can be mechanically ported.

Transaction/isolation/sequence semantics must be re-evaluated.

---

# 172. Distributed SQL

Authority epoch and commit ordering require careful certification.

---

# 173. Cloud Database Branding

Cloud providers may expose compatible engines but operational behavior may differ.

Certification should identify actual deployment class.

---

# 174. Neon

Treat as PostgreSQL authority adapter plus Neon deployment profile.

---

# 175. Managed SQLite/Turso-Like Remote Systems

A local adapter and remote service are different roles.

Do not conflate.

---

# 176. Adapter ABI

Rust adapter SDK uses Rust API, not stable C ABI.

Third-party Rust adapters compile against semver-stable traits.

---

# 177. Non-Rust Database Integration

If a database has no Rust driver:

```text
bridge service
C ABI wrapper
```

can exist, but must still satisfy adapter semantics.

---

# 178. No Driver Leakage

Core should not care whether adapter internally uses Rust-native or FFI DB driver.

---

# 179. Adapter Review Checklist

Before accepting official adapter:

```text
[ ] transaction semantics documented
[ ] capability manifest complete
[ ] migrations defined
[ ] crash behavior tested
[ ] duplicate OperationId tested
[ ] cursor ordering tested
[ ] platform matrix defined
[ ] errors normalized
[ ] observability included
[ ] conformance passing
```

---

# 180. Completion Criteria

Part 36 is complete when:

```text
[ ] adapter SDK crate defined
[ ] local role defined
[ ] authority role defined
[ ] journal/ledger roles defined
[ ] snapshot/fencing roles defined
[ ] capability manifest defined
[ ] error normalization defined
[ ] migration contract defined
[ ] official/community adapter policy defined
[ ] conformance integration defined
[ ] platform certification defined
[ ] adapter invariants added
```

---

# 181. Final Architecture

```text
                    AEQUORA CORE
                         │
                         ▼
                 Adapter Contracts
       ┌─────────────────┼─────────────────┐
       ▼                 ▼                 ▼
  Local Store       Authority Store      Artifact Store
       │                 │                 │
       ▼                 ▼                 ▼
   Stoolap            PostgreSQL        Object Storage
   SQLite             Future DBs        Blob Backend
       │                 │
       └──────────┬──────┘
                  ▼
           Conformance Suite
                  │
                  ▼
        Certified Capabilities
```

---

# 182. Final Recommendation

Aequora should not define database support as:

```text
"we have code that seems to work with this DB"
```

It should define support as:

```text
"this adapter implements declared semantic capabilities and passes the corresponding conformance profile on this environment"
```

The key principle is:

> **Databases are replaceable physical mechanisms; Aequora's transactional, idempotency, cursor, and authority semantics are the contract.**

This lets Aequora remain genuinely database-agnostic while still being strict enough for financial, enterprise, mobile, and offline-first software.
