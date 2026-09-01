# Aequora Sync — Part 38

# Stoolap Embedded Local Replica Adapter & Client Persistence Architecture

## 1. Purpose

Part 37 made the authoritative PostgreSQL/Neon side concrete. This part defines the preferred embedded local implementation: `aequora-stoolap`.

The local database is not a miniature authority. Its job is to preserve durable local intent, provide responsive offline reads/writes, and safely reconcile authoritative results.

> **The local database owns durable provisional state; the server owns authoritative truth.**

Stoolap is the preferred candidate only if its adapter passes Aequora's required semantic and platform conformance tests. Aequora must retain the ability to use SQLite or another certified embedded engine without changing the synchronization protocol or domain model.

## 2. Local Store Responsibilities

The adapter provides application/domain persistence, atomic local mutation + outbox, durable operation queues, authoritative projection metadata, scope cursors, conflicts, bootstrap state, snapshot staging, repair/integrity metadata, scheduler checkpoints, store/device identity, migrations, storage accounting, crash recovery, and multi-process fencing.

It never decides server authorization, authoritative conflict outcomes, global journal order, authority epoch, or server business acceptance.

## 3. Core Local ACID Boundaries

### Tx A — Local Mutation

```text
BEGIN
  apply provisional domain mutation
  enqueue operation in outbox
  update local entity metadata
COMMIT
```

Invariant: a UI must never observe a durable local domain mutation without its corresponding durable synchronization intent.

### Tx C — Reconciliation

```text
BEGIN
  apply authoritative events
  update authoritative/base metadata
  reconcile optimistic overlay
  update operation outcomes
  persist conflicts
  advance scope cursor
COMMIT
```

Invariant: the cursor advances only in the same durable transaction that applies the authoritative state it represents.

## 4. One Embedded Database

Preferred:

```text
One Stoolap database
├── application tables
└── aequora metadata tables
```

This permits Tx A and Tx C to be truly atomic. Avoid a separate application DB plus Aequora DB unless a deliberate transactional bridge preserves the same invariant.

## 5. High-Level Architecture

```text
Dioxus / Application
        |
        v
Domain Repository / Product SDK
        |
        v
AequoraClient
        |
        v
StoolapLocalStore
   |        |        |
   v        v        v
Domain   Outbox   Sync Metadata
   \        |        /
    \       |       /
       Stoolap Tx
           |
           v
      Embedded File
```

## 6. Recommended Crate

```text
aequora-stoolap/
├── lib.rs
├── config.rs
├── connection.rs
├── tx.rs
├── domain_bridge.rs
├── outbox.rs
├── cursor.rs
├── entity_meta.rs
├── conflict.rs
├── bootstrap.rs
├── snapshot.rs
├── integrity.rs
├── repair.rs
├── scheduler.rs
├── fencing.rs
├── migration.rs
├── storage.rs
├── backup.rs
├── health.rs
├── errors.rs
└── capabilities.rs
```

It implements `aequora-adapter-sdk`; Stoolap-specific transaction/query types never leak into core.

## 7. Local Store Identity

Persist:

```text
aequora_local_store
├── local_store_id
├── store_generation
├── device_id
├── device_binding_generation
├── metadata_schema_version
├── created_at
└── last_opened_by_build
```

`LocalStoreId` identifies the physical replica. `DeviceId` identifies the registered logical device. They are distinct.

## 8. Domain Data

Applications retain domain-specific tables such as `student`, `invoice`, `attendance`, or `inventory_item`. Aequora owns synchronization metadata beside them rather than forcing all products into a generic entity table.

## 9. Entity Metadata

Logical sidecar:

```text
aequora_entity_meta
├── entity_type
├── entity_id
├── authoritative_version
├── optimistic_generation
├── tombstone_state
├── last_authoritative_sequence
├── scope_membership
└── integrity_digest
```

Equivalent metadata may be embedded safely in domain tables when appropriate.

## 10. Base and Optimistic State

Conceptually:

```text
authoritative/base state
+
pending local intent
=
visible optimistic state
```

Simple products may materialize the visible state plus sufficient outbox metadata. Conflict-heavy domains may retain explicit base snapshots or richer metadata.

## 11. Outbox

Logical shape:

```text
aequora_outbox
├── operation_id
├── local_sequence
├── tenant_id
├── actor_id
├── device_id
├── entity_type
├── entity_id
├── operation_kind
├── schema_version
├── base_version
├── hlc
├── payload
├── payload_digest
├── state
├── ever_sent
├── attempt_count
├── next_attempt_at
├── in_flight_owner
├── in_flight_since
├── correlation_id
├── causation_id
└── created_at
```

`LocalOperationSequence`, `EntityVersion`, `TimelineSequence`, and HLC remain distinct concepts.

## 12. Outbox States

```text
Pending
InFlight
Accepted
Rejected
Conflict
Superseded
Cancelled
```

Terminal history may be compacted according to retention policy.

## 13. `ever_sent`

Once an operation may have reached authority, its `OperationId` and semantic payload are immutable. Compaction/rebase may freely transform only operations known never to have been transmitted.

## 14. Payload Digest

Store canonical BLAKE3 digest with the Postcard payload for corruption detection, diagnostics, and OperationId misuse detection.

## 15. Dependencies and Supersession

```text
aequora_outbox_dependency
├── operation_id
└── depends_on_operation_id
```

Track semantic supersession explicitly rather than silently deleting history.

## 16. Atomic Mutation API

Normal application code calls a high-level operation such as `client.mutate(operation)`. Internally one Stoolap transaction applies the domain mutation, inserts the outbox operation, updates metadata, and commits.

Never commit the domain row and enqueue later; never enqueue and independently commit the domain mutation later.

## 17. Domain Repository Bridge

The adapter needs an internal transaction bridge so application repositories and Aequora metadata use the same physical transaction. The public high-level SDK should hide most of this complexity.

## 18. Cursor Storage

```text
aequora_scope_cursor
├── scope_id
├── scope_version
├── scope_generation
├── authority_id
├── authority_epoch
├── sequence
├── bootstrap_generation
└── updated_at
```

Cursor advancement occurs only inside Tx C.

## 19. Crash During Reconciliation

If the process dies before Tx C commits, none of its state is accepted. The same authoritative batch can safely be fetched and applied again.

## 20. ACK Reconciliation

Server outcomes transition local operations to accepted, rejected, conflict, or another semantic state inside Tx C.

When authoritative state changes an entity with pending local intent, update the authoritative base and then rebase/reapply pending intent according to operation semantics. Never blindly overwrite pending work.

## 21. Conflict Storage

```text
aequora_conflict
├── conflict_id
├── entity_type
├── entity_id
├── operation_id
├── conflict_kind
├── authoritative_version
├── local_summary
├── authoritative_summary
├── status
└── created_at
```

Conflicts survive app restart, network loss, and OS termination.

## 22. Tombstones

Deletion normally retains tombstone semantics long enough to prevent stale resurrection. Physical GC follows server/governance safety rules; a client cannot independently declare a server tombstone globally obsolete.

## 23. Bootstrap State

```text
NotStarted
ManifestReceived
Downloading
Verifying
Installing
Activating
CatchUp
Complete
Failed
```

Progress is durable.

## 24. Snapshot Staging

Prefer active and staging generations rather than installing a huge bootstrap destructively into the active replica.

If Stoolap lacks atomic generation swapping, implement a certified logical generation or exclusive file replacement strategy.

Snapshot metadata includes `AuthorityEpoch` and boundary sequence `N`; after activation, pull journal from `N + 1`.

## 25. Pending Intent During Rebootstrap

Before replacing authoritative cache:

```text
preserve pending operations
classify compatibility
install authoritative snapshot
rebase valid pending intent
quarantine invalid intent
```

Never silently discard unsynchronized user work.

## 26. Streaming Snapshot Installation

Large snapshot chunks use staging files/streams, not whole-snapshot memory buffers. Verify manifest, lengths, BLAKE3 hashes, schema version, scope, and authority epoch before activation.

## 27. Anti-Entropy and Repair

Optional local metadata includes entity/partition digests, Merkle caches, integrity checkpoints, and durable repair plans. These support verification; they do not give the client authority to invent state.

## 28. Scheduler Persistence

Persist only scheduler state that must survive restart, such as backoff, Retry-After, circuit-breaker state, and data-budget counters. Task handles remain in memory.

## 29. Crash Recovery on Open

```text
verify store identity
verify schema
recover migrations
inspect bootstrap staging
recover stale in-flight operations
inspect repair state
verify fencing/leader ownership
check storage health
resume scheduler
```

## 30. Stale In-Flight Operations

An operation marked `InFlight` before process death may already have reached the server. Requeue/retry it using the same OperationId and payload; never manufacture a replacement identity.

## 31. Multi-Process Coordination

Desktop may have GUI, CLI, agent, and multiple processes. Exactly one active coordinator owns leader-only sync actions for a local store.

Use a durable lease plus monotonic fencing token:

```text
aequora_local_lease
├── local_store_id
├── owner_process_id
├── fencing_token
├── lease_expires_at
└── heartbeat_at
```

A stale process that resumes after losing leadership must fail fenced writes.

# Desktop and Mobile Storage Differences

## 32. Shared Semantics

Desktop and mobile use identical Tx A, Tx C, outbox, cursor, conflict, bootstrap, OperationId, and AuthorityEpoch semantics. Only physical environment and resource policy differ.

## 33. Desktop Characteristics

```text
larger storage budget
longer process lifetime
multiple windows/processes
background agent possible
larger caches
larger snapshot staging
easier diagnostics/export
```

## 34. Mobile Characteristics

```text
tighter storage budget
frequent OS process termination
background time limits
battery/thermal constraints
storage pressure
secure sandbox
smaller memory budget
```

## 35. Storage Classes

### Critical

```text
local DB
pending outbox
identity metadata
conflicts
cursor
```

### Reconstructable

```text
downloaded snapshots
integrity cache
derived indexes
```

### Evictable

```text
blob cache
thumbnails
temporary diagnostics
temporary download chunks
```

## 36. Platform Placement

Desktop uses platform application-data directories. Android uses app-private database/files areas, never cache for critical state. iOS uses durable app-private Application Support/data locations for critical state and cache locations only for reconstructable data.

## 37. Low-Storage Policy

Evict in roughly this order:

```text
temporary files
derived caches
old diagnostics
re-downloadable blobs
obsolete snapshots
```

Never evict pending outbox to free space.

If Tx A cannot be durably committed, reject the mutation and surface `StorageCritical`; the UI must not report `SavedLocally`.

## 38. Storage Preflight

Before large bootstrap/import operations, estimate staging requirements, inspect available space where possible, and reserve a safety margin.

## 39. Maintenance

Schedule DB checkpoint/compaction/integrity/index maintenance under resource-aware policy. Do not run heavy maintenance in latency-critical foreground paths, especially on mobile.

## 40. Durability Configuration

If Stoolap exposes multiple durability modes, map them to Aequora durability classes. Critical Tx A/Tx C must never use a correctness-breaking weak mode merely for benchmark speed.

## 41. Platform Certification

Embedded durability depends partly on filesystem and OS behavior, so certification is platform-specific.

Desktop targets should include Linux, Windows, and macOS. Mobile targets should include supported Android architectures/API levels and iOS device/simulator environments where Stoolap supports them.

## 42. Stoolap Capability Gate

Before making Stoolap the default, verify:

```text
ACID transactions
atomic domain + outbox
atomic reconcile + cursor
crash recovery
unique constraints/indexing
safe concurrency
migration behavior
platform support
durability
backup/export behavior
```

If a capability is missing, implement a semantically equivalent layer, limit the supported profile, or use another adapter. Never weaken Aequora invariants to fit the engine.

## 43. SQLite Reference Alternative

Maintain `aequora-sqlite` as a mature portability/reference adapter. It provides fallback, differential testing, mobile comparison, and an independent validation of the adapter contract.

Changing Stoolap to SQLite must not require changing operation models, wire protocol, server semantics, domain IDs, or journal semantics.

## 44. Backup and Restore

A local backup must represent a consistent store; do not blindly copy an open database file unless the engine guarantees that method.

After restore verify StoreId, DeviceId/binding, schema, AuthorityEpoch, and pending outbox. A copied store on another device must not silently reuse security credentials.

## 45. Device Clone Detection

Use DeviceBindingGeneration, secure-key availability, and store identity. Mobile OS backup policy should deliberately decide which DB data participates; device credentials generally should not be clonable by ordinary backup.

## 46. Encryption and Secure Keys

If local DB encryption is required, keep encryption keys in platform secure storage:

```text
Android -> Keystore
iOS -> Keychain/Secure Enclave
Linux -> Secret Service
Windows -> DPAPI/Credential Manager
macOS -> Keychain
```

Do not silently fall back to plaintext key storage when policy requires protection.

## 47. Local Migrations

```text
aequora_migration
├── migration_id
├── checksum
├── from_version
├── to_version
└── applied_at
```

Migrations coordinate Aequora metadata, application domain schema, and operation compatibility.

Pending unsent operations may be upcast only when semantic equivalence is proven. Possibly-sent operations retain their semantic payload identity.

An older application opening a newer unsupported store format must refuse rather than perform destructive downgrade.

## 48. Migration Crash Safety

Migrations must be transactional or checkpointed/recoverable depending on Stoolap capability. Risky migrations may create a bounded pre-migration backup when resources allow.

## 49. Local Health

Expose structured state such as:

```text
store open
schema valid
writable
storage available
migration status
integrity status
leader/fence state
```

Never auto-delete a corrupted store. Enter `RecoveryRequired` and preserve pending user intent/evidence where possible.

## 50. Recovery Priority

Recovery order should protect unsynchronized operations first, then reconstruct authoritative cache through repair, backup restore, or rebootstrap.

## 51. Indexing

Typical metadata indexes:

```text
outbox(state, local_sequence)
outbox(operation_id)
cursor(scope_id)
conflict(status)
entity_meta(entity_type, entity_id)
```

Tune from measured workloads.

## 52. UI Queries

Do not load the whole embedded database into Dioxus signals. Use paged queries, view models, and incremental invalidation.

Emit UI change notifications only after transaction commit; these notifications are advisory, while DB state remains durable truth.

## 53. Tokio and Blocking I/O

If Stoolap's API is blocking, execute it through a bounded blocking pool rather than blocking Tokio reactor threads. Rayon remains for bounded CPU-heavy tasks such as hashing/compression, not ordinary DB I/O.

## 54. Memory Bounds

Never materialize an unbounded outbox, snapshot, scope, or conflict collection. Use bounded batches, pagination, and streaming.

## 55. Operation Batch Claim

Claim a bounded eligible batch atomically:

```text
Pending -> InFlight(owner, timestamp)
```

Include coordinator/fencing identity where multi-process coordination is enabled. Stale claims become retryable after ownership validation.

## 56. Server Response Application

A response may contain operation outcomes, authoritative events, conflicts, scope changes, cursor information, and policy hints. Only durable semantic state enters Tx C.

## 57. Scope Expansion and Contraction

Large expansion should bootstrap/stage then tail journal. Contraction is `EvictFromScope`, not domain deletion. Entities with pending local intent may remain pinned even when otherwise evictable.

## 58. Blob Metadata

Store BlobId, digest, size, availability, pin state, and last-access metadata locally. Large bytes live outside the main DB.

Re-downloadable blob cache is evictable unless pinned by pending intent.

A newly created offline blob must be durably stored before an operation referencing it can be considered safely queued.

## 59. Diagnostics

Useful diagnostics:

```text
pending operation count
oldest pending age
last successful sync
cursor
authority epoch
store generation
DB size
blob cache size
conflict count
migration version
leader state
```

Sensitive payloads remain redacted by default.

## 60. Testing Strategy

Use deterministic adapter tests, real Stoolap integration tests, and platform conformance tests.

Tx A fault injection points:

```text
after domain write
before outbox insert
after outbox insert
before commit
```

Expected: both durable or neither.

Tx C fault points:

```text
after event apply
after operation ACK
after conflict insert
before cursor update
after cursor update before commit
```

Expected: full reconcile + cursor or none.

## 61. Required Failure Tests

Test arbitrary process kill during mutation, upload, reconciliation, bootstrap, migration, and compaction; duplicate authoritative responses; stale responses; disk full; cloned store; competing coordinators; migration; rebootstrap; storage pressure; mobile background expiry; and desktop sleep/resume.

## 62. Differential Testing

Run identical workloads against:

```text
ReferenceLocalStore
StoolapLocalStore
SQLiteLocalStore
```

Observable synchronization semantics should match.

## 63. Conformance Profiles

Recommended:

```text
StoolapLocalCore
StoolapDesktopLocalFull
StoolapMobileLocalFull
```

Support claims are bound to adapter version, Stoolap version, target platform, filesystem/runtime assumptions, and relevant feature configuration.

## 64. Suggested Metadata Namespace

```text
aequora_local_store
aequora_outbox
aequora_outbox_dependency
aequora_entity_meta
aequora_scope_cursor
aequora_conflict
aequora_bootstrap
aequora_snapshot_stage
aequora_integrity
aequora_repair
aequora_scheduler
aequora_local_lease
aequora_migration
```

Exact Stoolap DDL belongs to implementation after validating the engine's current syntax and capabilities.

## 65. Core Invariants

### AEQ-INV-STOOLAP001
Every durable provisional domain mutation commits atomically with its corresponding outbox operation.

### AEQ-INV-STOOLAP002
Authoritative events, operation reconciliation, conflicts, and cursor advancement commit atomically.

### AEQ-INV-STOOLAP003
An operation that may have been transmitted retains the same OperationId and semantic payload across retries and restarts.

### AEQ-INV-STOOLAP004
Pending unsynchronized user intent is never discarded merely to reclaim storage, rebootstrap, migrate, or repair reconstructable state.

### AEQ-INV-STOOLAP005
A cursor never advances beyond authoritative state durably installed locally.

### AEQ-INV-STOOLAP006
Only the current fenced coordinator may perform leader-owned synchronization metadata transitions.

### AEQ-INV-STOOLAP007
A cloned/restored store cannot silently reuse device security identity without validating binding generation and secure-key state.

### AEQ-INV-STOOLAP008
Storage pressure may evict reconstructable/cache data but never critical pending intent or correctness metadata.

### AEQ-INV-STOOLAP009
Stoolap-specific types remain inside the adapter boundary and cannot alter Aequora protocol/domain semantics.

### AEQ-INV-STOOLAP010
Stoolap is considered supported on a platform only after the corresponding local-store conformance profile passes there.

## 66. Recommended Implementation Order

### Phase 1 — Minimal Correct Store

```text
open/store identity
Tx A
outbox
cursor
Tx C
migrations
```

### Phase 2 — Sync Completeness

```text
conflicts
dependencies
compaction metadata
scope metadata
bootstrap
```

### Phase 3 — Production Reliability

```text
fencing
storage accounting
backup
repair
integrity
diagnostics
```

### Phase 4 — Platform Certification

```text
Linux
Windows
macOS
Android
iOS where supported
```

### Phase 5 — Optimization

Only after measurement: index, batch, compaction, and memory tuning.

## 67. Relationship to Previous Parts

```text
Part 04 -> compaction/rebase
Part 05 -> multi-process fencing
Part 06 -> scheduler
Part 07 -> scopes
Part 10 -> bootstrap/snapshots
Part 20 -> constrained clients
Part 22 -> metadata schema
Part 31 -> Android/iOS runtime
Part 32 -> desktop runtime
Part 33 -> platform storage architecture
Part 35 -> public SDK
Part 36 -> adapter SDK
Part 37 -> PostgreSQL authority
Part 38 -> Stoolap local persistence
```

## 68. End-to-End Local Flow

```text
USER EDIT
   |
   v
Tx A
+-----------------------------+
| domain mutation             |
| outbox operation            |
| local entity metadata       |
+-----------------------------+
   |
 COMMIT
   |
   v
SavedLocally
   |
   v
Scheduler claims outbox
   |
   v
Postcard exchange
   |
   v
PostgreSQL Authority
   |
   v
authoritative response
   |
   v
Tx C
+-----------------------------+
| apply authoritative events  |
| reconcile pending operation |
| persist conflicts           |
| update base/version         |
| advance cursor              |
+-----------------------------+
   |
 COMMIT
   |
   v
ServerConfirmed / Conflict
```

## 69. Desktop vs Mobile Summary

```text
                   Desktop                 Mobile
--------------------------------------------------------------
DB semantics       identical               identical
Tx A / Tx C        identical               identical
Outbox             durable                 durable
Cursor             durable                 durable
Process lifetime   usually longer          frequently killed
Multi-process      common                  less common
Storage budget     larger                  tighter
Background work    flexible                OS constrained
Blob cache         larger                  aggressively bounded
Snapshot staging   larger                  streamed/bounded
Fencing            important               needed when concurrent
Secure keys        OS credential store     hardware-backed OS store
Maintenance        background/agent        budget-aware
```

The platform changes resource policy, not synchronization correctness.

## 70. Completion Criteria

```text
[ ] Stoolap adapter role defined
[ ] Tx A defined
[ ] Tx C defined
[ ] app + metadata transaction model defined
[ ] outbox schema/state defined
[ ] cursor persistence defined
[ ] conflicts defined
[ ] bootstrap/snapshot staging defined
[ ] crash recovery defined
[ ] fencing defined
[ ] desktop/mobile differences defined
[ ] storage pressure defined
[ ] backup/restore defined
[ ] migrations defined
[ ] secure-key integration defined
[ ] conformance strategy defined
[ ] SQLite reference role defined
[ ] invariants defined
```

# 71. Final Architecture

```text
                       Application
                           |
                           v
                    Aequora Client SDK
                           |
                           v
                    StoolapLocalStore
                           |
          +----------------+----------------+
          |                |                |
          v                v                v
     Domain Data        Outbox         Sync Metadata
          |                |                |
          +----------------+----------------+
                           |
                    SAME ACID TX
                           |
                           v
                       Stoolap
                           |
             +-------------+-------------+
             |                           |
          Desktop                       Mobile
     larger resource budget      constrained lifecycle
     multi-process fencing       process-kill resilience
             |                           |
             +-------------+-------------+
                           |
                    same invariants
```

# 72. Final Recommendation

Use Stoolap as Aequora's preferred embedded local database **only after it proves the required semantics through conformance on every supported platform**.

The key local rule is not which embedded database wins a benchmark:

> **The user's local mutation and the durable intent to synchronize it must be one transaction; authoritative reconciliation and cursor advancement must also be one transaction.**

This produces Aequora's clean three-boundary model:

```text
Tx A — local mutation + outbox
Tx B — server authority + journal + ledger
Tx C — local authoritative reconciliation + cursor
```

With these boundaries preserved, Stoolap can later be replaced by SQLite or another certified embedded engine without redesigning Aequora itself.
