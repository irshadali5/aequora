# Aequora Sync — Part 33

# Cross-Platform Local Storage Architecture:
# Mobile vs Desktop Storage Engines, Filesystem Layout, Durability, Encryption, Backup, and Recovery

## 1. Purpose

Aequora uses the same synchronization semantics across:

```text
Android
iOS
Linux
Windows
macOS
```

but the local storage environment is not the same.

Mobile storage is constrained by:

```text
app sandboxing
OS lifecycle termination
storage quotas
background limits
mobile backup systems
secure keystore/keychain integration
flash wear
limited free space
```

Desktop storage usually provides:

```text
larger disks
longer-running processes
more filesystem access
multiple local processes
user-controlled backups
portable modes
enterprise-managed paths
```

Therefore:

> **Aequora should use one logical local-storage contract across all platforms, while allowing platform-specific physical storage layouts and capabilities.**

The central rule is:

> **The synchronized local store must preserve the same transactional and recovery invariants on every platform, even though its physical implementation may differ.**

---

# 2. Why Part 33 Is Necessary

Parts 31 and 32 defined mobile and desktop runtime behavior.

They did not fully specify:

```text
which data belongs in which storage tier
how local DB requirements differ by platform
how cache and durable data are separated
how encrypted keys and DB files are stored
how backup/restore interacts with DeviceId
how mobile and desktop storage pressure policies differ
how an adapter proves durability on each platform
```

Part 33 closes that gap.

---

# 3. Core Storage Principle

Aequora local storage is not "just a cache".

It may contain:

```text
user-visible offline state
pending operations
conflicts
scope cursors
bootstrap state
device metadata
local indexes
blob references
```

Some of this data is reconstructable.

Some is not.

The most important distinction is:

```text
reconstructable data
vs
irreplaceable local intent
```

---

# 4. Storage Classes

Define five storage classes.

```text
Class A — Critical Durable Intent
Class B — Durable Replicated State
Class C — Recoverable Derived State
Class D — Ephemeral Cache
Class E — Secret Material
```

---

# 5. Class A — Critical Durable Intent

Examples:

```text
outbox operations
unsent local domain intent
conflict resolutions not yet confirmed
local-only drafts when product requires preservation
```

This is the highest-priority data.

Loss may mean:

```text
user work disappears
```

Therefore:

> **Aequora must never evict Class A automatically.**

---

# 6. Class B — Durable Replicated State

Examples:

```text
authoritative entity projection
scope cursor
entity versions
tombstones
bootstrap generation metadata
```

This data can theoretically be reconstructed from server authority, but rebuilding may be expensive or impossible while offline.

Treat as durable.

---

# 7. Class C — Recoverable Derived State

Examples:

```text
local search index
thumbnail metadata
precomputed UI projections
integrity cache
query acceleration indexes
```

Can be rebuilt.

---

# 8. Class D — Ephemeral Cache

Examples:

```text
temporary snapshot chunks
download cache
completed transfer fragments
decoded transient artifacts
temporary exports
```

Can be removed aggressively.

---

# 9. Class E — Secret Material

Examples:

```text
refresh token
device private key
local encryption key
```

Should not be stored as ordinary database rows unless encrypted under OS-backed keys.

---

# 10. Logical Store Layout

Recommended logical layout:

```text
LocalStore
├── DomainState
├── AequoraMetadata
│   ├── Outbox
│   ├── Cursors
│   ├── Conflicts
│   ├── BootstrapState
│   ├── RepairState
│   └── CoordinatorLease
├── DerivedState
├── BlobMetadata
└── Diagnostics
```

Secret material belongs outside ordinary store:

```text
OS Secure Store
```

---

# 11. One Transactional Database for Synchronized State

Recommended:

> **Domain state and Aequora metadata should share one transactional embedded database whenever possible.**

This enables:

```text
local mutation
+
outbox insert
```

to commit atomically.

---

# 12. Why Separate DBs Are Dangerous

Suppose:

```text
app DB
+
sync metadata DB
```

are separate.

Then:

```text
app mutation commits
outbox insert fails
```

or:

```text
outbox commits
app mutation fails
```

becomes possible.

This violates the most important client invariant.

---

# 13. Acceptable Separate-DB Architecture

Only use separate stores if there is a deliberate bridge such as:

```text
transactional host outbox
+
Aequora bridge consumer
```

This is more complex and should be treated as a separate integration profile.

---

# 14. Platform-Neutral Storage Traits

Aequora core should depend on capabilities, not a specific engine.

Concept:

```rust
pub trait LocalStore:
    LocalTransactionStore
    + OutboxStore
    + CursorStore
    + ConflictStore
    + MetadataStore
{}
```

---

# 15. Required Local Store Capabilities

Minimum production client requirements:

```text
atomic multi-record transaction
durable commit
unique OperationId
ordered local sequence
atomic cursor update
schema migration
crash recovery
bounded indexed lookup
```

---

# 16. Strongly Recommended Capabilities

```text
WAL/journaled durability
snapshot/backup support
transaction savepoints
integrity checking
concurrent readers
```

---

# 17. Optional Capabilities

```text
full-text search
native encryption
incremental backup
MVCC
change notifications
```

---

# 18. Mobile vs Desktop Storage Goals

Mobile prioritizes:

```text
small footprint
bounded I/O
low memory
resumability
storage pressure survival
minimal wake time
```

Desktop prioritizes:

```text
throughput
multi-process coordination
larger datasets
larger caches
faster local search
long-running maintenance
```

---

# 19. Android Storage Architecture

Recommended filesystem layout:

```text
app-private/
├── data/
│   └── aequora.db
├── files/
│   └── durable_blobs/
├── cache/
│   ├── snapshot_chunks/
│   ├── temp_blobs/
│   └── derived/
└── diagnostics/
```

---

# 20. Android Private Storage

Use app-internal storage for:

```text
database
durable metadata
critical blobs
```

Avoid public/external shared storage for the main DB.

---

# 21. Android Cache Directory

Use cache directory for:

```text
reconstructable
temporary
evictable
```

data.

The OS may remove cache files.

Therefore:

```text
never place outbox or cursor there
```

---

# 22. Android Scoped Storage

External user-picked files should enter through:

```text
Storage Access Framework
```

The application may get:

```text
content:// URI
```

Platform bridge can open a file descriptor for Rust.

---

# 23. Android Storage Permission Philosophy

Aequora should not need broad filesystem permissions just to operate.

Main store stays private.

---

# 24. Android Backup

Automatic backup can be dangerous for sync identity.

Potential risks:

```text
restored stale DeviceId
restored refresh token
duplicated local store on new phone
old pending operations replayed
```

---

# 25. Android Backup Policy

Classify files:

```text
DB
keys
tokens
cache
exports
```

and explicitly decide:

```text
include
exclude
restore-aware
```

---

# 26. Android Device Identity Restore

Never assume restored DB means same physical device identity.

On restore:

```text
detect missing/mismatched secure key
create new device binding
classify pending operations
rebootstrap if required
```

---

# 27. iOS Storage Architecture

Recommended:

```text
Application Support/
    aequora.db
    durable_blobs/

Caches/
    snapshot_chunks/
    derived/
    temp_blobs/

Keychain/
    tokens
    device keys
```

---

# 28. iOS Application Support

Use for:

```text
database
durable offline state
required files
```

---

# 29. iOS Caches

Only for reconstructable data.

The OS may purge it.

---

# 30. iOS File Protection

Sensitive durable files should use appropriate file-protection class.

Tradeoff:

```text
stronger protection
vs
background accessibility
```

---

# 31. iOS Background Access

If database/key is unavailable while device locked:

```text
background sync may defer
```

This is preferable to weakening key protection.

---

# 32. iCloud Backup

As with Android:

```text
device identity
pending outbox
cached server state
```

need explicit backup policy.

---

# 33. iOS Restore

Restored store on a new phone should be treated as:

```text
restored replica
```

not automatically same trusted device.

---

# 34. Linux Storage Architecture

Recommended paths:

```text
$XDG_DATA_HOME/app/
    aequora.db
    durable_blobs/

$XDG_CACHE_HOME/app/
    snapshot_chunks/
    temp/
    derived/

$XDG_STATE_HOME/app/
    logs/
    diagnostics/
```

---

# 35. Linux Portable Alternative

Optional portable mode:

```text
./data/
./cache/
```

but must explicitly handle:

```text
permissions
encryption
identity
backups
```

---

# 36. Linux Multi-Process

Embedded DB must be evaluated for:

```text
multi-process readers
writer locking
crash recovery
```

If weak:

```text
use aequora-agent as sole DB owner
```

---

# 37. Windows Storage Architecture

Recommended:

```text
%LOCALAPPDATA%\App\
    data\
        aequora.db
    blobs\
    cache\
    diagnostics\
```

---

# 38. Windows Roaming Profile Warning

Do not put large synchronized DB into roaming profile by default.

Roaming copies can:

```text
duplicate stores
create stale replicas
cause large profile sync
```

---

# 39. Windows Credential Storage

Secrets:

```text
Credential Manager
DPAPI-protected storage
```

depending implementation.

---

# 40. Windows File Locking

Adapter certification should test:

```text
restart
exclusive writer
multi-process access
AV interference
```

where practical.

---

# 41. macOS Storage Architecture

Recommended:

```text
~/Library/Application Support/App/
    aequora.db
    durable_blobs/

~/Library/Caches/App/
    snapshot_chunks/
    derived/
    temp/
```

Secrets:

```text
Keychain
```

---

# 42. macOS Sandbox

If sandboxed, use application container paths.

Do not hardcode home directory assumptions.

---

# 43. macOS Backup

Time Machine may copy application data.

Restored store identity must still be verified.

---

# 44. Embedded Database Selection

Aequora should not hardcode one client database forever.

Instead define certified adapters.

Possible categories:

```text
SQL embedded
KV embedded
document-like embedded
custom transactional store
```

---

# 45. Stoolap Role

If Stoolap provides required:

```text
transactions
durability
indexes
platform support
```

it may be a strong candidate.

But mobile/desktop certification should prove real behavior.

---

# 46. SQLite Role

SQLite is attractive where broad platform maturity is valuable.

Aequora can treat it as one adapter, not a special architecture.

---

# 47. Redb/Fjall-like Engines

KV engines can work if adapter supplies:

```text
transactional metadata model
indexes
ordered scans
migration
```

But application query ergonomics may differ.

---

# 48. Client DB Role Is More Than Outbox

The local DB may serve:

```text
offline read model
domain state
sync metadata
query engine
```

So selection should consider product query patterns too.

---

# 49. Server and Client DB Need Not Match

Example:

```text
Mobile:
    Stoolap

Desktop:
    Stoolap or SQLite

Server:
    PostgreSQL/Neon
```

This is fine because Aequora synchronizes domain semantics, not DB rows.

---

# 50. Mobile and Desktop Can Use Different Local DBs

Also valid:

```text
Android:
    SQLite adapter

iOS:
    SQLite adapter

Desktop:
    Stoolap adapter
```

if all pass conformance.

---

# 51. Database Interoperability Rule

Aequora protocol must not include:

```text
SQLite rowid
Postgres ctid
Stoolap internal page ID
```

Only canonical Aequora/domain identities.

---

# 52. Durability Levels

Define:

```rust
pub enum DurabilityClass {
    Critical,
    Standard,
    Reconstructable,
}
```

---

# 53. Critical Durability

Used for:

```text
outbox
cursor
device metadata
conflict state
```

---

# 54. Standard Durability

Used for:

```text
replicated domain state
```

---

# 55. Reconstructable

Used for:

```text
derived indexes
temporary snapshot state
```

---

# 56. Commit Policy

Do not globally force expensive full-fsync behavior for every cache write.

Use storage class semantics.

But:

> **A committed local user mutation must meet the durability guarantee the UI claims.**

---

# 57. "Saved Locally" Meaning

When UI says:

```text
Saved locally
```

it should mean:

```text
local transaction durably committed according to product durability contract
```

not merely:

```text
data is still in an in-memory buffer
```

---

# 58. WAL and Journaling

If the DB uses WAL/journal mode:

```text
configure for crash safety
```

and test actual platform behavior.

---

# 59. Flash Storage Considerations

Mobile flash benefits from:

```text
batched metadata writes
bounded checkpoint frequency
not rewriting large files unnecessarily
```

without weakening correctness.

---

# 60. Write Amplification

Watch:

```text
large indexes
frequent vacuum
whole-file rewrite
```

especially on mobile.

---

# 61. Desktop SSD Considerations

Still important, but desktop generally has more headroom.

Maintenance can be more aggressive when idle.

---

# 62. Local Sequence

Client outbox ordering should use explicit:

```text
LocalOperationSequence
```

Do not rely on file timestamp.

---

# 63. Metadata Indexes

Minimum:

```text
outbox by state/local sequence
outbox by OperationId
scope cursor by ScopeId
conflict by entity/status
bootstrap by scope/generation
```

---

# 64. Domain Indexes

Application-specific.

Examples:

```text
student by class
invoice by status/date
message by conversation/time
```

---

# 65. Mobile Index Budget

Avoid excessive secondary indexes.

Every index costs:

```text
disk
write amplification
migration time
```

---

# 66. Desktop Index Budget

Can support richer local indexes/search.

Still benchmark.

---

# 67. Full-Text Search

Treat as derived state when possible.

---

# 68. Rebuildable Search

If search index corrupts:

```text
drop
rebuild
```

without affecting authoritative/local intent.

---

# 69. Blob Storage

Keep large binary blobs outside main relational/KV record space when practical.

---

# 70. Blob Directory

Durable blobs:

```text
content-addressed
```

by digest.

---

# 71. Blob Metadata

DB stores:

```text
BlobRef
digest
size
availability
pin state
sync state
```

---

# 72. Mobile Blob Policy

Default:

```text
on-demand
bounded
evictable if reconstructable
```

---

# 73. Desktop Blob Policy

May cache more aggressively.

---

# 74. Blob Pinning

User/product can pin required offline content.

Pinned blob is not evictable.

---

# 75. Blob Integrity

Verify digest after:

```text
download
restore
import
```

---

# 76. Snapshot Storage

Snapshot staging files are:

```text
temporary until verified
```

---

# 77. Snapshot Activation

Never overwrite active store incrementally without rollback plan.

Use:

```text
staging generation
verify
activate
```

---

# 78. Mobile Snapshot Activation

May prefer:

```text
chunked DB import
```

into staging tables/generation to avoid huge duplicate files.

---

# 79. Desktop Snapshot Activation

Can often afford more temporary disk.

Still preflight free space.

---

# 80. Storage Preflight

Before large operation calculate:

```text
expected download
temporary overhead
DB growth
safety margin
```

---

# 81. Preflight Result

```rust
pub enum StorageAdmission {
    Allowed,
    AllowedReduced,
    Deferred,
    InsufficientSpace,
}
```

---

# 82. Mobile Free-Space Safety Margin

Use conservative percentage + absolute minimum.

---

# 83. Desktop Free-Space Safety Margin

Can differ by profile.

---

# 84. Low-Storage State Machine

```text
Healthy
↓
Pressure
↓
Critical
↓
ReadMostly
```

---

# 85. Healthy

Normal operation.

---

# 86. Pressure

Evict:

```text
cache
temporary chunks
derived state
```

---

# 87. Critical

Pause:

```text
large downloads
nonessential maintenance
```

---

# 88. ReadMostly

If durable writes cannot be guaranteed:

```text
reject new local mutations
```

but allow safe reads.

---

# 89. Never Delete Outbox Automatically

Even under storage pressure.

---

# 90. Safe Outbox Compaction

Part 04 rules apply.

Only semantically safe unsent operations may compact.

---

# 91. Cache Eviction Priority

Recommended:

```text
1. temporary transfer files
2. reconstructable derived caches
3. unpinned remote blobs
4. optional scopes with no pending dependencies
```

Never evict Class A.

---

# 92. Scope Eviction

Must preserve:

```text
pending operation dependencies
conflicts
required metadata
```

---

# 93. Database Compaction/Vacuum

Treat as maintenance job.

---

# 94. Mobile Maintenance

Run under:

```text
charging
foreground/allowed background window
sufficient storage
```

where possible.

---

# 95. Desktop Maintenance

Can run during idle/low activity.

---

# 96. Encryption Architecture

Separate:

```text
OS device encryption
DB/application encryption
secret encryption
```

---

# 97. OS Device Encryption

Useful baseline.

Not sufficient for every threat model.

---

# 98. Database Encryption

Optional application-level storage encryption may be required for sensitive products.

---

# 99. Encryption Key

Store DB encryption key using:

```text
Android Keystore
iOS Keychain/Secure Enclave strategy
Windows DPAPI/Credential Manager
macOS Keychain
Linux Secret Service
```

---

# 100. Linux Without Secret Service

Require explicit fallback policy.

Possible:

```text
user passphrase
TPM-backed integration
enterprise key provider
```

---

# 101. Key Rotation

Support:

```text
new key generation
re-encryption/migration
old key retirement
```

as durable maintenance workflow.

---

# 102. Mobile Re-Encryption

Must be resumable because app may terminate.

---

# 103. Desktop Re-Encryption

Can use longer maintenance windows but still checkpoint.

---

# 104. Backup Types

Distinguish:

```text
OS backup
Aequora local backup
user export
server-authoritative backup
```

---

# 105. OS Backup

Platform-managed.

Potentially opaque/unreliable from app perspective.

Do not depend on it as primary sync durability.

---

# 106. Aequora Local Backup

Explicit application backup of:

```text
domain state
metadata
outbox
conflicts
```

with manifest.

---

# 107. User Export

Usually a business/data portability format.

Not necessarily suitable for exact local-store recovery.

---

# 108. Server Backup

Authoritative disaster recovery.

Different from client backup.

---

# 109. Local Backup Manifest

Concept:

```rust
pub struct LocalBackupManifest {
    pub store_id: LocalStoreId,
    pub device_id: DeviceId,
    pub device_binding_generation: u64,
    pub authority_epoch: AuthorityEpoch,
    pub schema_version: LocalStoreFormatVersion,
    pub created_at: Timestamp,
    pub digest: Digest,
}
```

---

# 110. Backup Encryption

Sensitive local backup should be encrypted.

---

# 111. Backup Restore Classification

Restore may be:

```text
same device
same OS user
new device
new machine
```

These are not equivalent.

---

# 112. Same Device Restore

May preserve device binding if secure key still matches.

---

# 113. New Device Restore

Must create/rebind device identity.

---

# 114. Pending Outbox Restore

Operations need classification:

```text
never sent
possibly sent
committed unknown
```

---

# 115. Restore After Long Offline Period

Check:

```text
protocol compatibility
operation schema support
journal floor
authority epoch
device revocation
```

---

# 116. Restore Decision

Possible:

```text
resume
rebase
rebootstrap
manual review
```

---

# 117. Client Store Snapshot

A local DB backup should be transactionally consistent.

---

# 118. Hot Backup

If adapter supports safe online snapshot:

```text
use it
```

---

# 119. Cold Backup

Otherwise:

```text
quiesce coordinator
checkpoint
copy
resume
```

---

# 120. Desktop Backup Coordination

Agent mode simplifies:

```text
one process owns store
```

and can produce consistent backup.

---

# 121. Mobile Backup Coordination

OS backup may occur unpredictably.

Sensitive/inconsistent files may need exclusion.

---

# 122. Corruption Detection

At startup or periodically:

```text
metadata sanity
schema version
cursor invariants
optional DB integrity check
```

---

# 123. Corruption Response

Classify:

```text
derived-state corruption
replicated-state corruption
critical-intent corruption
```

---

# 124. Derived-State Corruption

Drop and rebuild.

---

# 125. Replicated-State Corruption

Repair/rebootstrap from authority.

---

# 126. Critical-Intent Corruption

Do not silently discard.

Generate diagnostics and recovery path.

---

# 127. Operation Payload Digest

Store BLAKE3 digest.

This helps detect:

```text
accidental mutation
corruption
OperationId payload mismatch
```

---

# 128. Local Metadata Integrity

Critical records can include checksums where useful.

---

# 129. Merkle/Anti-Entropy

Part 03 applies to local replicated state.

---

# 130. Database Migration

Migration categories:

```text
metadata schema
domain schema
derived index
blob layout
encryption format
```

---

# 131. Migration Rule

Never perform destructive migration without:

```text
preflight
version check
transaction/checkpoint
recovery plan
```

---

# 132. Mobile Migration

Keep bounded.

Avoid multi-minute uninterruptible migration.

---

# 133. Migration Checkpoint

Large migration:

```text
phase
last key
generation
```

persisted.

---

# 134. Desktop Migration

May perform larger batch size.

Same correctness rules.

---

# 135. Downgrade

Default:

```text
newer store format + older binary = refuse
```

---

# 136. Schema Compatibility

Store format version is separate from protocol version.

---

# 137. Store Format Version

Define:

```rust
pub struct LocalStoreFormatVersion(pub u32);
```

---

# 138. Adapter Format Version

Physical adapter may also need:

```text
AdapterSchemaVersion
```

---

# 139. Platform Storage Profiles

Define profiles:

```text
MobileMinimal
MobileStandard
DesktopConservative
DesktopStandard
DesktopLarge
```

---

# 140. MobileMinimal

Use for low-end devices.

```text
small DB cache
few indexes
small blob cache
aggressive derived eviction
```

---

# 141. MobileStandard

Moderate offline dataset.

---

# 142. DesktopConservative

Laptop with limited storage.

---

# 143. DesktopStandard

Default desktop.

---

# 144. DesktopLarge

Enterprise workstation/large local scope.

---

# 145. StorageProfile

Concept:

```rust
pub struct StorageProfile {
    pub max_blob_cache_bytes: u64,
    pub max_optional_scope_bytes: u64,
    pub min_free_bytes: u64,
    pub maintenance_policy: MaintenancePolicy,
}
```

---

# 146. Dynamic Adaptation

Profile is baseline.

Runtime may reduce limits under pressure.

---

# 147. User-Controlled Storage

Product may expose:

```text
offline storage limit
clear cache
pin scope
pin document
```

---

# 148. Clear Cache

Must never delete:

```text
outbox
required metadata
unconfirmed conflict resolution
```

---

# 149. Clear Offline Data

This is a stronger action.

Require confirmation if pending intent exists.

---

# 150. Logout and Store Deletion

Before delete:

```text
check unsynced operations
```

---

# 151. Forced Delete

If user explicitly chooses to discard unsynced work:

```text
audit locally where feasible
```

and make UI clear.

---

# 152. Multiple Local Stores

One per:

```text
account
workspace
tenant profile
```

is often simpler than one giant multi-account DB.

---

# 153. Store Registry

Keep lightweight metadata about available stores.

---

# 154. Store Registry Is Not Sync Metadata

It only locates local stores.

---

# 155. Desktop Shared Store

If multiple applications need same store:

```text
agent owns it
```

recommended.

---

# 156. Mobile Shared Store

Generally avoid cross-app sharing.

OS sandbox makes this complicated and risky.

---

# 157. App Extensions

iOS share/widget extensions should access:

```text
read-only projection
```

or carefully coordinated shared container.

---

# 158. Android Multiple Processes

Avoid direct concurrent writers.

---

# 159. Filesystem Atomicity

When writing non-DB metadata files:

```text
write temp
fsync if required
rename atomically
```

where filesystem guarantees permit.

---

# 160. Manifest Publication

For snapshots/backups:

```text
data first
manifest last
```

so incomplete data is never considered published.

---

# 161. Temporary File Naming

Include:

```text
StoreId
JobId
Generation
```

to support cleanup/recovery.

---

# 162. Crash Cleanup

On startup:

```text
scan known temp job metadata
```

not entire disk recursively.

---

# 163. Orphan Files

Durable cleanup job can remove unreachable cache/temp files.

---

# 164. Blob Garbage Collection

Reference-counting alone may be unsafe with crashes.

Prefer:

```text
reachability scan + grace period
```

or transactional reference metadata.

---

# 165. Mobile Blob GC

Small bounded batches.

---

# 166. Desktop Blob GC

Can process larger batches.

---

# 167. File Descriptor Limits

Keep open DB/blob handles bounded.

---

# 168. Windows Antivirus Interaction

Large temporary/executable-like files may be scanned.

Avoid fragile timing assumptions.

---

# 169. Filesystem Watchers

Do not use filesystem watcher as durability mechanism.

---

# 170. Network Filesystems

Do not place embedded local store on NFS/SMB/cloud-synced folder unless adapter explicitly certifies that environment.

---

# 171. Why

File locking/fsync semantics may differ.

---

# 172. Cloud-Synced Folders

Avoid:

```text
OneDrive
Dropbox
iCloud Drive
Google Drive
```

for live embedded DB files.

They can create conflicts/copies.

---

# 173. User Export to Cloud Folder

Fine if export package is immutable/closed.

---

# 174. Database Locking

Adapter should expose:

```text
single writer
multi reader
multi process
```

capability manifest.

---

# 175. Mobile Locking

Normally one app process.

Simpler.

---

# 176. Desktop Locking

More important because CLI/agent/UI may coexist.

---

# 177. Store Lock File

Can supplement DB locking, but not replace fencing.

---

# 178. LocalStoreId

Persist stable store identity.

---

# 179. StoreGeneration

Increment when:

```text
destructive replacement
restore
rebootstrap activation
```

where appropriate.

---

# 180. Clone Detection

Store includes:

```text
LocalStoreId
DeviceId
DeviceBindingGeneration
```

Secure store includes matching binding secret/key.

Mismatch indicates:

```text
restored/cloned store
```

---

# 181. Clone Recovery

Do not continue blindly.

Flow:

```text
freeze sync
↓
rebind device
↓
validate epoch/journal floor
↓
rebootstrap or reconcile
```

---

# 182. Secure Erase

Deleting file does not guarantee physical flash overwrite.

For strong erase:

```text
cryptographic erase
```

is more reliable.

---

# 183. Cryptographic Erase

Encrypt local store with DEK.

Destroy DEK to make remaining ciphertext inaccessible.

---

# 184. Mobile Secure Erase

Useful because flash remapping prevents reliable physical overwrite.

---

# 185. Desktop Secure Erase

Also preferable on SSDs.

---

# 186. Tenant Offboarding

Local store purge should remove:

```text
DB
durable blobs
cache
keys
store registry entry
```

according to governance policy.

---

# 187. Legal Hold

If product applies legal hold locally:

```text
do not purge held required data
```

unless architecture specifies authority-only hold behavior.

---

# 188. Diagnostics Retention

Bound and classify as:

```text
derived/operational
```

not critical domain state.

---

# 189. Logs

Store outside DB if convenient, but never depend on logs for recovery.

---

# 190. Local Audit

If application maintains local audit cache:

```text
replicated
```

not source of authority unless offline-authority profile explicitly exists.

---

# 191. Offline-Authority Variant

If future Aequora supports a local-authority deployment:

```text
storage requirements become stronger
```

because client DB is then authoritative.

This should be a separate certification profile.

---

# 192. Client Storage Certification

Create profiles:

```text
MobileLocalStoreFull
DesktopLocalStoreFull
```

---

# 193. Shared Mandatory Tests

```text
Tx A atomicity
cursor atomicity
crash reopen
unique OperationId
migration
durability
corruption handling
```

---

# 194. Mobile-Specific Certification

Add:

```text
process kill
low disk
cache purge
backup restore
key unavailable
small-memory operation
```

---

# 195. Desktop-Specific Certification

Add:

```text
multi-process access
stale writer fencing
clone restore
large dataset
agent ownership
sleep/resume
```

---

# 196. Crash Atomicity Test

Inject kill:

```text
after domain write
before outbox insert
```

Expected:

```text
both rollback
```

---

# 197. Durability Test

Commit mutation.

Force process termination.

Reopen.

Expected:

```text
mutation + outbox present
```

---

# 198. Cursor Test

Apply authoritative events.

Kill before cursor commit.

Expected:

```text
replay safely
```

---

# 199. Low-Disk Test

Force insufficient storage.

Expected:

```text
mutation rejected
no partial state
```

---

# 200. Cache Purge Test

Delete cache directory.

Expected:

```text
no loss of pending intent
```

---

# 201. Backup Restore Test

Restore on new device.

Expected:

```text
device rebind detected
```

---

# 202. Store Clone Test

Copy desktop store.

Expected:

```text
binding mismatch
```

---

# 203. Encryption-Key-Loss Test

If key lost:

```text
store becomes unrecoverable
```

unless recovery mechanism explicitly exists.

This must be documented.

---

# 204. Migration Crash Test

Kill during migration.

Expected:

```text
resume or rollback
```

never half-open state.

---

# 205. Storage Metrics

Useful:

```text
db_bytes
blob_cache_bytes
outbox_bytes
free_disk_bytes
scope_cache_bytes
```

---

# 206. Avoid High-Cardinality Labels

Do not label metrics by EntityId.

---

# 207. User Storage UX

Expose understandable categories:

```text
Offline data
Attachments
Cache
Pending changes
```

---

# 208. Pending Changes Cannot Be Cleared Casually

Make destructive action explicit.

---

# 209. Storage Policy Example

RON:

```ron
storage: (
    profile: MobileStandard,

    cache: (
        max_bytes: 536870912,
    ),

    blobs: (
        max_unpinned_bytes: 2147483648,
    ),

    pressure: (
        minimum_free_bytes: 268435456,
    ),
)
```

---

# 210. Desktop Policy Example

```ron
storage: (
    profile: DesktopStandard,

    cache: (
        max_bytes: 4294967296,
    ),

    blobs: (
        max_unpinned_bytes: 21474836480,
    ),
)
```

---

# 211. Policy Is Not Correctness

Larger or smaller cache limits do not change sync semantics.

---

# 212. Storage Invariants

## AEQ-INV-STORAGE001

```text
A local user mutation is not considered saved unless its domain state and outbox record commit atomically.
```

## AEQ-INV-STORAGE002

```text
Critical durable intent is never automatically evicted because of storage pressure.
```

## AEQ-INV-STORAGE003

```text
Cache or derived-state loss cannot make Aequora lose pending authoritative intent.
```

## AEQ-INV-STORAGE004

```text
A restored or cloned local store cannot silently reuse a device binding when secure identity no longer matches.
```

## AEQ-INV-STORAGE005

```text
An older binary cannot open a newer local store format unless downgrade compatibility is explicitly certified.
```

## AEQ-INV-STORAGE006

```text
A local storage adapter must preserve required transaction semantics on the actual target platform, not merely in abstract trait tests.
```

---

# 213. Additional Storage Invariants

## AEQ-INV-STORAGE007

```text
Temporary snapshot/blob files are never treated as committed durable state before integrity verification and publication.
```

## AEQ-INV-STORAGE008

```text
A low-disk condition may reduce functionality but cannot produce a false successful commit.
```

## AEQ-INV-STORAGE009

```text
Secrets and encryption keys are not stored as plaintext ordinary sync metadata.
```

## AEQ-INV-STORAGE010

```text
Live embedded database files are not placed on unverified network/cloud-synchronized filesystems.
```

---

# 214. Recommended Workspace Additions

```text
crates/
├── aequora-storage-core/
├── aequora-storage-profile/
├── aequora-storage-maintenance/
├── aequora-storage-backup/
├── aequora-storage-encryption/
├── aequora-storage-conformance/
├── aequora-stoolap/
├── aequora-sqlite/
└── aequora-blob-store/
```

---

# 215. Storage Core

Contains:

```text
logical traits
storage classes
admission types
store metadata
```

---

# 216. Storage Profile

Contains:

```text
mobile/desktop policy presets
```

---

# 217. Maintenance

Contains:

```text
vacuum
compaction
GC
integrity jobs
```

---

# 218. Backup

Contains:

```text
manifest
snapshot/export
restore classifier
```

---

# 219. Encryption

Contains:

```text
DEK/KEK integration
key rotation
cryptographic erase
```

---

# 220. Conformance

Runs platform-specific storage certification.

---

# 221. Adapter Selection Strategy

Do not choose DB only by benchmarks.

Evaluate:

```text
atomicity
platform stability
migration
recovery
query needs
multi-process behavior
maintenance status
license
```

---

# 222. Mobile DB Recommendation Pattern

For mobile:

```text
prefer mature transactional embedded engine
with small runtime footprint and strong crash recovery
```

---

# 223. Desktop DB Recommendation Pattern

For desktop:

```text
prefer strong transactional engine
with richer local query support and safe multi-process strategy
```

---

# 224. Same Engine Everywhere

Using one embedded DB across mobile + desktop simplifies:

```text
adapter code
migration logic
testing
learning
```

This is valuable if it meets every platform requirement.

---

# 225. Different Engines Where Needed

It is also acceptable to use:

```text
SQLite mobile
Stoolap desktop
```

if this improves platform reliability.

Aequora's logical contract allows it.

---

# 226. Do Not Force Uniformity

The architecture should optimize for:

```text
semantic consistency
```

not necessarily:

```text
identical physical DB
```

---

# 227. Recommended Decision

Start with one preferred adapter.

Build conformance tests.

Only add second adapter when:

```text
platform support
performance
licensing
maintenance
```

creates a real reason.

---

# 228. Storage Failure Classification

Use typed errors:

```rust
pub enum LocalStorageError {
    DiskFull,
    ReadOnlyFilesystem,
    Corruption,
    KeyUnavailable,
    MigrationRequired,
    UnsupportedStoreVersion,
    BindingMismatch,
    TransactionFailed,
}
```

---

# 229. UI Mapping

Examples:

```text
DiskFull
→ "Free storage before saving new changes."

KeyUnavailable
→ "Unlock device to continue sync."

BindingMismatch
→ "This local data was restored from another device and must be reconnected."
```

---

# 230. No Raw DB Errors in UI

Map adapter-specific errors to canonical storage errors.

---

# 231. Operational Runbook

For local storage incident:

```text
1. identify storage class affected
2. stop destructive maintenance
3. preserve outbox
4. export diagnostics
5. repair/rebootstrap reconstructable state
6. recover pending intent if possible
```

---

# 232. Corrupt Local DB Strategy

Possible:

```text
copy corrupt store
extract Class A intent if safe
create new store
bootstrap
reapply/reconcile recovered intent
```

This should be tooling-assisted.

---

# 233. Recovery Tool

Future CLI:

```text
aequora local-store inspect
aequora local-store verify
aequora local-store recover
aequora local-store backup
aequora local-store restore
```

---

# 234. Mobile Recovery UX

Keep simple and safe:

```text
Repair local data
Re-download data
Contact support
```

Do not expose raw database repair operations to ordinary users.

---

# 235. Desktop Recovery UX

Can offer more advanced diagnostics.

---

# 236. Developer Testing Matrix

At minimum:

```text
Android + selected DB
iOS + selected DB
Linux + selected DB
Windows + selected DB
macOS + selected DB
```

---

# 237. Do Not Assume One Adapter Behaves Identically Everywhere

Filesystem and locking semantics differ.

Certification is per supported platform range.

---

# 238. Platform Adapter Certification Record

Example:

```text
StoolapAdapter 1.x
Android arm64
MobileLocalStoreFull
Passed

StoolapAdapter 1.x
Windows x86_64
DesktopLocalStoreFull
Passed
```

---

# 239. Final Storage Architecture

```text
                         Aequora Client
                              │
                              ▼
                     Local Storage Contract
                              │
          ┌───────────────────┼───────────────────┐
          ▼                   ▼                   ▼
   Critical Intent     Replicated State      Derived/Cache
   Outbox              Entities              Search
   Conflicts           Versions              Thumbnails
   Cursors             Tombstones            Temp files
          │                   │                   │
          └───────────────────┼───────────────────┘
                              ▼
                     Embedded DB + Blob Store
                              │
       ┌──────────────────────┼──────────────────────┐
       ▼                      ▼                      ▼
     Mobile                Desktop              Secure Store
 Android / iOS      Linux / Windows / macOS     OS key store
       │                      │
       ▼                      ▼
 bounded storage       larger cache/indexes
 process kills         multi-process concerns
 OS backup             clone/portable concerns

Same logical invariants everywhere.
Different physical policy per platform.
```

---

# 240. Final Recommendation

Yes, storage deserves a dedicated architecture because mobile and desktop platforms differ significantly in:

```text
filesystem layout
backup behavior
process model
cache eviction
disk capacity
multi-process access
secure key storage
maintenance windows
```

The recommended approach is:

> **Use one logical Aequora local-storage contract, one set of correctness invariants, and platform-specific storage profiles/adapters.**

The preferred local database may be the same across all platforms if it passes certification, but Aequora should not require that.

What must remain identical is:

```text
atomic mutation + outbox
durable cursor semantics
safe crash recovery
stable IDs
restore/clone detection
no automatic loss of pending intent
```

That is what makes Aequora genuinely cross-platform rather than merely cross-compilable.
