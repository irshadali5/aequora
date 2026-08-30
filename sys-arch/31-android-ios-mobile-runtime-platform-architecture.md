# Aequora Sync — Part 31

# Android and iOS Mobile Runtime, Platform Integration, and Lifecycle Architecture

## 1. Purpose

Aequora's core synchronization model is already suitable for mobile:

```text
local-first
durable outbox
bounded synchronization
server-authoritative commits
snapshot bootstrap
resource-aware scheduling
process-crash recovery
```

However, Android and iOS introduce platform constraints that are different from desktop and server environments.

Mobile operating systems may:

```text
suspend the process
kill the process
restrict background networking
limit execution time
throttle timers
change network connectivity frequently
enforce app sandboxing
require secure OS-managed key storage
restrict filesystem access
```

Therefore:

> **Aequora does not need a different synchronization model for Android or iOS, but it does need dedicated mobile runtime and platform adapters.**

The central rule is:

> **Mobile platforms may interrupt execution at any time; therefore all synchronization correctness must depend on durable state, never uninterrupted process lifetime.**

---

## 2. Goals

Part 31 defines:

```text
shared Rust mobile core
Android integration
iOS integration
FFI/binding boundaries
background execution
process lifecycle recovery
secure key storage
push-notification wake hints
local database ownership
network monitoring
battery/resource awareness
mobile update/migration behavior
UI event delivery
testing
packaging
release architecture
```

---

## 3. Non-Goals

This architecture does not:

```text
rewrite sync logic in Kotlin
rewrite sync logic in Swift
make Android/iOS authoritative
depend on permanent background processes
depend on push notifications for correctness
```

---

## 4. High-Level Mobile Architecture

```text
                 Android / iOS Application
                          │
                          ▼
                Platform UI / Lifecycle
              Kotlin/Swift or Dioxus shell
                          │
                          ▼
                  Mobile Binding Layer
                          │
                          ▼
                   Aequora Rust Client
            ┌─────────────┼─────────────┐
            ▼             ▼             ▼
      Local Store      Scheduler      Crypto
            │             │             │
            ▼             ▼             ▼
       Outbox/Cursor   Resource       OS Secure
       Domain Data     Policy         Storage
            │
            └─────────────┬─────────────┘
                          ▼
                    HTTPS/Postcard
                          │
                          ▼
                    Aequora Server
```

---

## 5. Shared Rust Core

Android and iOS should reuse the same Rust crates:

```text
aequora-types
aequora-protocol
aequora-client
aequora-sync-core
aequora-metadata
aequora-storage
aequora-crypto
aequora-diagnostics
```

Platform-specific crates should remain thin.

---

## 6. Platform Crates

Recommended:

```text
aequora-platform-android
aequora-platform-ios
aequora-mobile-runtime
aequora-mobile-bindings
```

---

## 7. Mobile Runtime Crate

`aequora-mobile-runtime` should own:

```text
mobile lifecycle state
resource context
foreground/background transitions
sync budgeting
process restart recovery
push-hint handling
```

but remain platform-neutral where possible.

---

## 8. Platform Adapter Boundary

Define traits such as:

```rust
pub trait NetworkMonitor;
pub trait PowerMonitor;
pub trait SecureStore;
pub trait BackgroundExecutionHost;
pub trait AppLifecycleSource;
pub trait PushHintSource;
```

Android and iOS implement these differently.

---

## 9. Rust Core Owns Synchronization Semantics

Rust should own:

```text
outbox
retries
cursor
bootstrap
conflict handling
scheduler policy
idempotency
local storage
```

Platform code should only provide:

```text
signals
permissions
OS scheduling hooks
secure-key handles
```

---

## 10. Android Recommended Architecture

```text
Android UI
    │
    ▼
Kotlin bridge / Dioxus Android shell
    │
    ▼
JNI / generated native binding
    │
    ▼
Rust Aequora Client
    │
    ├── embedded DB
    ├── outbox
    ├── scheduler
    └── HTTPS transport
```

---

## 11. iOS Recommended Architecture

```text
iOS UI
   │
   ▼
Swift bridge / Dioxus iOS shell
   │
   ▼
generated native binding
   │
   ▼
Rust Aequora Client
   │
   ├── embedded DB
   ├── outbox
   ├── scheduler
   └── HTTPS transport
```

---

## 12. Dioxus Mobile

If the application uses Dioxus for both Android and iOS:

```text
Dioxus UI
   │
   ▼
Rust App Layer
   │
   ▼
Aequora Client
```

This is the cleanest integration because most code remains Rust.

Platform bridges are still required for:

```text
secure keystore/keychain
push notifications
background tasks
network state
platform permissions
```

---

## 13. Pure Rust Does Not Mean No Platform Bridge

Some operating-system services are only exposed through native platform APIs.

Therefore "pure Rust" should mean:

```text
business logic
sync engine
domain logic
database
protocol
scheduler policy
```

remain Rust.

Thin Kotlin/Swift/ObjC glue may still be necessary for platform APIs.

---

## 14. Mobile Client Lifetime

The Rust client should conceptually live as:

```text
one logical client instance per local store
```

The process may die and recreate it later.

---

## 15. Process Lifetime Is Not Sync Lifetime

Never rely on:

```text
an always-running Tokio task
```

for required synchronization.

Durable state determines continuation.

---

## 16. Startup Recovery

On application start:

```text
open local store
↓
validate metadata version
↓
recover stale in-flight operations
↓
recover interrupted bootstrap
↓
recover scheduler backoff state
↓
resume local coordinator
↓
render local data
↓
schedule sync
```

---

## 17. Foreground Startup

The application should show local data immediately.

Do not wait for network before rendering.

---

## 18. Local-First UX

Startup flow:

```text
App Launch
↓
Open Embedded DB
↓
Render Cached State
↓
Start Sync Opportunistically
```

---

## 19. Background Execution Is Opportunistic

Mobile OS does not promise immediate background execution.

Therefore correctness cannot depend on:

```text
"job will run in 5 minutes"
```

---

## 20. Android Background Architecture

Use:

```text
WorkManager
foreground service only when genuinely required
push wakeups
app foreground sync
```

---

## 21. Android WorkManager

Kotlin/Android bridge schedules a worker.

Worker:

```text
wake
↓
open/reuse Rust client
↓
run bounded sync_once()
↓
checkpoint
↓
return success/retry
```

---

## 22. Android Worker Must Be Thin

Do not implement:

```text
retry policy
batch planning
outbox logic
```

in Kotlin.

Kotlin only invokes Rust.

---

## 23. Android Foreground Service

Use only for:

```text
long visible upload/download
user-visible urgent sync
```

Do not abuse foreground services for permanent background presence.

---

## 24. iOS Background Architecture

Use:

```text
BGTaskScheduler
background URL session where applicable
push wake hints
foreground sync
```

---

## 25. iOS Execution Window

iOS may provide only a short background window.

Therefore Aequora should:

```text
process bounded batch
checkpoint
exit cleanly
```

---

## 26. Bounded Sync Unit

One background execution should operate on:

```text
max operations
max bytes
max elapsed time
```

---

## 27. Mobile Sync Budget

```rust
pub struct MobileSyncBudget {
    pub max_duration: Duration,
    pub max_upload_bytes: u64,
    pub max_download_bytes: u64,
    pub max_operations: usize,
}
```

---

## 28. Foreground Budget

Larger.

---

## 29. Background Budget

Smaller.

---

## 30. Low-Power Budget

Smaller still.

---

## 31. Process Kill Safety

Assume kill can happen:

```text
before request
during upload
after server commit
before response
during reconcile
during snapshot install
```

Every case must recover safely.

---

## 32. Server Commit + Client Kill

Server may commit operation.

Client dies before reading response.

On restart:

```text
same OperationId retried
```

server ledger returns existing outcome.

---

## 33. Reconcile Kill

If client dies during reconciliation:

```text
cursor not advanced unless transaction committed
```

Events may replay safely.

---

## 34. Bootstrap Kill

Snapshot install uses:

```text
staging generation
chunk checkpoint
verified chunk status
```

Resume after restart.

---

## 35. Local Database Ownership

Recommended:

> **Rust/Aequora should own the synchronized local domain database.**

This preserves:

```text
local domain mutation + outbox atomicity
```

---

## 36. Avoid Kotlin Room + Separate Rust DB

If Kotlin writes domain data to Room while Aequora stores outbox in another DB:

```text
atomicity breaks
```

unless deliberate bridge architecture exists.

---

## 37. Avoid Swift CoreData + Separate Rust DB

Same issue.

---

## 38. Preferred Mobile Store

Use one embedded DB for:

```text
application synchronized entities
Aequora metadata
outbox
cursor
conflicts
```

---

## 39. Stoolap

If Stoolap meets required mobile capabilities and platform support:

```text
atomic transaction
durability
indexes
migration
```

it can serve as local adapter.

Certification suite should verify this.

---

## 40. SQLite Alternative

A mature SQLite adapter may be useful for:

```text
broad platform compatibility
```

while preserving Aequora logical semantics.

---

## 41. Database Adapter Must Be Certified

Mobile support should depend on:

```text
capabilities
```

not database brand.

---

## 42. Local Store Location

Use application-private sandbox directory.

Android:

```text
internal app data
```

iOS:

```text
Application Support / private container
```

---

## 43. Do Not Store DB in Public External Storage

Avoid user-accessible/shared filesystem unless explicit export.

---

## 44. File Protection

On iOS, set appropriate file protection class for sensitive data.

---

## 45. Android File Encryption

Use platform/device encryption and optional app-level encryption where product requires.

---

## 46. Secure Key Storage

Android:

```text
Android Keystore
```

iOS:

```text
Keychain / Secure Enclave where appropriate
```

---

## 47. SecureStore Trait

```rust
pub trait SecureStore {
    async fn store_secret(
        &self,
        key: SecretKeyId,
        value: SecretBytes,
    ) -> Result<(), SecureStoreError>;

    async fn load_secret(
        &self,
        key: SecretKeyId,
    ) -> Result<SecretBytes, SecureStoreError>;
}
```

---

## 48. Prefer Key Handles

Where possible, use OS key handles rather than exporting private key bytes into Rust memory.

---

## 49. Device Identity

Device identity may use:

```text
DeviceId
device signing key
registration record
```

---

## 50. Device Registration

Flow:

```text
user authenticates
↓
Rust/platform creates device key
↓
public key registered with server
↓
DeviceId issued/bound
```

---

## 51. Lost Device

Server can revoke:

```text
DeviceId
public key
sessions
```

Remote purge is best-effort only.

---

## 52. Offline Cached Data

If device never reconnects after revocation:

```text
server cannot physically delete data from that device
```

At-rest encryption reduces exposure.

---

## 53. Authentication Tokens

Prefer:

```text
short-lived access token
refresh token in secure storage
```

---

## 54. Token Ownership

Ideally Rust owns token lifecycle if authentication client lives in Rust.

Otherwise platform supplies token via credential callback.

---

## 55. Credential Callback

```rust
pub trait CredentialProvider {
    async fn access_token(&self) -> Result<SecretString, AuthError>;
}
```

---

## 56. Network Monitoring

Android source:

```text
ConnectivityManager
```

iOS source:

```text
NWPathMonitor
```

Normalize into:

```rust
pub struct NetworkContext {
    pub available: bool,
    pub metered: bool,
    pub expensive: bool,
    pub constrained: bool,
    pub roaming: bool,
}
```

---

## 57. Platform Policy Normalization

Core scheduler should not depend on Android or Swift types.

Platform adapters translate into neutral Rust types.

---

## 58. Metered Networks

Policy may:

```text
allow small metadata sync
defer large snapshot
defer large blobs
```

---

## 59. Roaming

Product can choose:

```text
metadata only
manual approval for large transfer
```

---

## 60. Offline Mode

When no network:

```text
mutations continue locally
outbox grows durably
```

subject to storage capacity.

---

## 61. Reconnect

On reconnect:

```text
wake scheduler
coalesce hints
run bounded exchange
```

Avoid stampede.

---

## 62. Push Notifications

Push is only:

```text
wake/sync hint
```

Never the authoritative data channel.

---

## 63. Android Push

Possible:

```text
FCM
```

---

## 64. iOS Push

Possible:

```text
APNs
```

---

## 65. Push Payload

Small:

```text
tenant/account hint
scope hint
reason
```

No business-sensitive state required.

---

## 66. Push Loss

If push never arrives:

```text
foreground sync
periodic background opportunities
manual refresh
```

still converge.

---

## 67. Push Duplication

Coalesce.

---

## 68. Push Security

Do not trust push payload as authorization.

It is only scheduling input.

---

## 69. App Lifecycle States

```rust
pub enum AppLifecycle {
    Foreground,
    Background,
    Suspended,
    Terminating,
}
```

---

## 70. Foreground

Allow:

```text
interactive sync
live hints
faster retries
```

---

## 71. Background

Use reduced budgets.

---

## 72. Suspended

No assumptions of execution.

---

## 73. Terminating

Checkpoint best effort.

Correctness must already be durable.

---

## 74. Power State

```rust
pub struct PowerContext {
    pub charging: bool,
    pub low_power_mode: bool,
    pub battery_level_class: BatteryClass,
}
```

---

## 75. Thermal State

Normalize:

```text
Nominal
Fair
Serious
Critical
```

or coarse equivalent.

---

## 76. Thermal Policy

Defer:

```text
compression-heavy
hash rebuild
large repair
```

under critical thermal pressure.

---

## 77. Correctness Overrides

Security/governance actions may override convenience power policy.

---

## 78. Low Memory

Mobile runtime should avoid:

```text
whole-scope materialization
large decoded batches
large UI mirrors
```

---

## 79. Memory Budget

```rust
pub struct MobileMemoryBudget {
    pub max_sync_buffer: usize,
    pub max_decode_bytes: usize,
    pub max_pending_view_models: usize,
}
```

---

## 80. Snapshot Streaming

Mandatory for large snapshots.

---

## 81. Blob Streaming

Mandatory for large attachments.

---

## 82. Compression

Use zstd only above threshold and within CPU/power budget.

---

## 83. CPU Parallelism

Mobile default:

```text
1 or small bounded CPU worker count
```

Do not blindly use all cores with Rayon.

---

## 84. Tokio Runtime

Use modest worker count.

Potential mobile runtime:

```text
2–4 async worker threads
```

depending device/profile.

---

## 85. Thread Count Config

Make profile-based, not hardcoded globally.

---

## 86. UI Thread

Never block Android main thread or iOS main actor.

---

## 87. FFI Async Boundary

Expose asynchronous commands.

---

## 88. Dioxus Rust UI

If UI is Dioxus in Rust, client API can be directly async.

Platform calls still bridge selectively.

---

## 89. Kotlin Binding

If Android UI is Kotlin:

```text
suspend functions
Flow events
```

should wrap Rust async engine.

---

## 90. Swift Binding

Expose:

```text
async/await
AsyncStream
```

where possible.

---

## 91. Event Model

Rust emits high-level mobile events:

```text
SyncStatusChanged
ConflictCreated
BootstrapProgress
DataChanged
AuthenticationRequired
UpgradeRequired
```

---

## 92. Do Not Emit Raw Journal to UI

UI should query local view.

---

## 93. Data Change Event

Example:

```text
StudentsChanged(scope_id)
```

Then UI refreshes query.

---

## 94. Event Coalescing

During large sync, coalesce UI invalidations per domain/scope.

---

## 95. Dioxus Query Pattern

```text
Aequora local DB
↓
repository query
↓
small view model
↓
signal
↓
virtualized list
```

---

## 96. Mobile Conflict UX

Conflict remains durable.

UI can resolve later.

---

## 97. Conflict Notification

Rust emits:

```text
ConflictCreated(ConflictId)
```

---

## 98. Offline Conflict Resolution

Some resolutions may be queued locally as new operations.

Server remains final authority.

---

## 99. App Upgrade

Mobile apps often update asynchronously across users.

Part 21 compatibility is essential.

---

## 100. App Binary Upgrade

On first launch after update:

```text
check FFI/runtime version
check local metadata schema
run migration
validate pending outbox
resume
```

---

## 101. Pending Operations During Upgrade

Possibly-sent operations:

```text
must keep same OperationId + payload semantics
```

---

## 102. Unsent Operations

May be migrated only if semantic equivalence is proven.

---

## 103. Store Downgrade

Older app opening newer local store:

```text
refuse
```

unless explicitly supported.

---

## 104. Mobile Rollback

Design metadata version checks conservatively.

---

## 105. Server Minimum Build

Server may require:

```text
minimum Android build
minimum iOS build
```

for security/compatibility.

---

## 106. Read-Only Grace

An outdated client may enter:

```text
ReadOnly
```

instead of total lockout if safe.

---

## 107. UpgradeRequired UX

Rust returns structured state.

Platform UI handles store/MDM navigation.

---

## 108. Android Packaging

Build Rust libraries for:

```text
arm64-v8a
x86_64 emulator
```

Optionally other ABIs if required.

---

## 109. Android Artifact

Recommended:

```text
AAR
├── Kotlin wrapper
└── jniLibs/
    ├── arm64-v8a/libaequora.so
    └── x86_64/libaequora.so
```

---

## 110. Android JNI

JNI layer should be minimal.

Prefer generated safe wrapper where practical.

---

## 111. Android Rust Toolchain

Use:

```text
cargo-ndk
Android NDK
```

or equivalent reproducible build pipeline.

---

## 112. iOS Packaging

Recommended:

```text
XCFramework
```

containing supported device/simulator binaries.

---

## 113. iOS Targets

Typical:

```text
aarch64-apple-ios
aarch64-apple-ios-sim
```

and additional simulator target if needed.

---

## 114. Swift Wrapper

Provide ergonomic Swift package/framework wrapper.

---

## 115. Symbol Visibility

Export only binding API.

Hide internal Rust symbols where possible.

---

## 116. ABI Version

Define:

```text
AEQUORA_MOBILE_ABI_VERSION
```

---

## 117. Build Identity

Expose:

```text
core version
commit/build ID
registry generation
protocol support
```

for diagnostics.

---

## 118. Crash Reporting

Rust panic must not unwind through FFI.

---

## 119. Panic Boundary

FFI entry points should catch panic where feasible and translate it into a stable internal error.

---

## 120. Native Crash

Rust memory safety reduces risk, but dependencies/FFI can still terminate the host process.

---

## 121. Diagnostics

Mobile diagnostic bundle should include:

```text
app build
Aequora build
local store version
scope cursors
outbox metadata
last sync events
resource state
```

---

## 122. No Secrets

Never include tokens or private keys.

---

## 123. Local Support Bundle

User can export/share a sanitized support bundle.

---

## 124. Android Diagnostic Integration

Host can invoke the platform Share Sheet for an encrypted/sanitized bundle.

---

## 125. iOS Diagnostic Integration

Use standard share/export flow.

---

## 126. Mobile Logging

Rust tracing can bridge to:

```text
Android Logcat
iOS os_log
```

---

## 127. Logging Redaction

Same security rules as server.

---

## 128. App Sandbox Security

Aequora assumes normal OS application isolation but not an uncompromised rooted/jailbroken device.

---

## 129. Root/Jailbreak Threat

Sensitive applications may restrict offline data or require stronger re-authentication.

---

## 130. Biometric Unlock

Optional UI/security layer.

Biometrics unlock a key/session; they are not synchronization authority.

---

## 131. Device Lock

Secure-storage keys may require unlocked device.

---

## 132. Background Key Access

If key unavailable while locked:

```text
background sync defers
```

without correctness loss.

---

## 133. Network TLS

Use verified TLS and trusted roots.

---

## 134. Rust TLS

A Rust TLS stack such as rustls can be used with appropriate platform root integration.

---

## 135. Proxy/VPN

Normal platform networking should remain compatible.

---

## 136. Captive Portal

Treat as network failure/unavailable.

---

## 137. Network Flapping

Debounce network changes.

---

## 138. Retry Storm

Use:

```text
jitter
batch adaptation
server Retry-After
```

---

## 139. Server Hints

Server may suggest smaller batch or retry delay.

---

## 140. Data Usage Tracking

Track:

```text
sync bytes
snapshot bytes
blob bytes
```

if product exposes data budget.

---

## 141. User Sync Controls

Possible:

```text
mobile-data sync
Wi-Fi-only large files
manual sync
```

---

## 142. User Cannot Disable Critical Security Work

Separate optional large-data work from mandatory security/account metadata.

---

## 143. Background Upload

Large blobs may use platform background-transfer facilities.

---

## 144. Blob Transfer Boundary

Aequora owns:

```text
content identity
chunk state
resume metadata
```

Platform may execute transport.

---

## 145. Transfer Completion Callback

Rust verifies digest and updates durable state.

---

## 146. iOS Background URLSession

Useful for large blob transfers where required.

---

## 147. Android WorkManager + HTTP

Suitable for resumable background blob work.

---

## 148. Sync Exchange vs Blob Transfer

Keep structured sync and large content transfer separate.

---

## 149. Mobile Scope Strategy

Scopes are especially important on mobile.

---

## 150. Required Scopes

Examples:

```text
current user
current school
current class
assigned jobs
recent invoices
```

---

## 151. Optional Scopes

Users may pin them for offline access.

---

## 152. Scope Cache Policy

```rust
pub enum ScopeCachePolicy {
    Required,
    Recent,
    OnDemand,
    NeverPersist,
}
```

---

## 153. Scope Eviction

Evict optional scope under pressure only when no pending intent depends on it.

---

## 154. Pending Operation Pins Scope

Pending intent may require base state for rebase/reconciliation.

---

## 155. Storage Pressure States

```text
Healthy
Low
Critical
```

---

## 156. Low Storage

Actions:

```text
evict caches
delete completed snapshot chunks
compact safe outbox
pause large downloads
```

---

## 157. Critical Storage

Block new mutations if durable storage cannot be guaranteed.

---

## 158. UI Must Tell Truth

Return:

```text
StorageUnavailable
```

instead of false success.

---

## 159. Push Registration

FCM/APNs token is registered to authenticated DeviceId.

---

## 160. Push Token Rotation

Update mapping idempotently.

---

## 161. Push Token Is Not Device Identity

Treat separately.

---

## 162. Notification Privacy

Push payload should avoid sensitive business content.

---

## 163. Background Sync Trigger Sources

```text
foreground
network restored
push hint
WorkManager/BGTask
manual sync
scheduled maintenance
```

---

## 164. Trigger Coalescing

Many triggers collapse into one sync request.

---

## 165. Mobile Coordinator

One coordinator per store.

---

## 166. Multiple Rust Components

If UI/background components may both access one store, Part 05 fencing still applies.

---

## 167. Android Separate Process

Avoid unless necessary.

---

## 168. iOS Extensions

Use careful shared-store coordination.

---

## 169. Widget/Share Extension

Prefer read-only projection rather than full sync runtime unless genuinely required.

---

## 170. Notification Extension

Should not mutate Aequora authoritative state directly.

---

## 171. Testing Architecture

Use:

```text
Rust core tests
Android instrumentation tests
iOS integration tests
process-kill tests
network-fault tests
background tests
upgrade tests
low-storage tests
```

---

## 172. Android Process Kill Test

Enqueue → sync → kill → restart.

Expected:

```text
operation preserved and safely retried
```

---

## 173. iOS Termination Test

Terminate during reconcile.

Expected:

```text
cursor never advances prematurely
```

---

## 174. Background Budget Test

Force execution expiration.

Expected:

```text
checkpoint + later resume
```

---

## 175. Disk Full Test

Mutation must fail atomically.

---

## 176. Metered Network Test

Large snapshot deferred according to policy.

---

## 177. Push Loss Test

Convergence still occurs later.

---

## 178. Duplicate Push Test

No duplicate authoritative effect.

---

## 179. Key Locked Test

Authenticated background sync defers.

---

## 180. App Upgrade Test

Old store with pending outbox migrates without losing intent.

---

## 181. App Downgrade Test

Newer store opened by older app safely refuses.

---

## 182. Snapshot Resume Test

Kill after chunk N; resume from verified checkpoint.

---

## 183. Scope Eviction Test

Optional unused scope can be removed safely.

---

## 184. Scope Pin Test

Pending operation prevents unsafe eviction.

---

## 185. Conflict Persistence Test

Conflict survives process death.

---

## 186. Android Binding Test

Kotlin APIs map correctly to Rust outcomes.

---

## 187. iOS Binding Test

Swift task cancellation does not undo already-durable intent.

---

## 188. FFI Memory Test

Repeated binding calls leak no handles/buffers.

---

## 189. Conformance Profile

Create:

```text
MobileClientFull
```

---

## 190. Mobile Certification Requirements

Must pass:

```text
atomic local outbox
process-death recovery
cursor atomicity
bootstrap resume
low-storage behavior
binding compatibility
secure-store integration
```

---

## 191. Android Release Architecture

```text
Rust cross-compile
↓
native libraries
↓
AAR packaging
↓
instrumentation tests
↓
Play/enterprise artifact
```

---

## 192. iOS Release Architecture

```text
Rust cross-compile
↓
XCFramework
↓
Swift wrapper
↓
integration tests
↓
App Store/enterprise artifact
```

---

## 193. Dioxus Release

With Dioxus mobile:

```text
Dioxus app
+
Aequora Rust crates
+
thin platform bridge
```

---

## 194. Dependency Rules

Mobile core must not depend on JNI, Swift, Android Context, or UIKit types.

---

## 195. Suggested Workspace

```text
crates/
├── aequora-client/
├── aequora-mobile-runtime/
├── aequora-platform-android/
├── aequora-platform-ios/
├── aequora-mobile-bindings/
├── aequora-stoolap/
├── aequora-sqlite/
└── aequora-diagnostics/
```

---

## 196. Android Adapter Layout

```text
aequora-platform-android/
├── lifecycle.rs
├── network.rs
├── power.rs
├── secure_store.rs
├── push.rs
├── background.rs
└── bindings.rs
```

---

## 197. iOS Adapter Layout

```text
aequora-platform-ios/
├── lifecycle.rs
├── network.rs
├── power.rs
├── keychain.rs
├── push.rs
├── background.rs
└── bindings.rs
```

---

## 198. Mobile Runtime Layout

```text
aequora-mobile-runtime/
├── context.rs
├── budgets.rs
├── coordinator.rs
├── wake.rs
├── recovery.rs
├── policies.rs
└── events.rs
```

---

## 199. Mobile Config

```ron
mobile: (
    foreground: (
        max_batch_ops: 500,
        max_batch_bytes: 4194304,
    ),

    background: (
        max_batch_ops: 100,
        max_batch_bytes: 1048576,
    ),

    network: (
        allow_metered_metadata: true,
        allow_metered_blobs: false,
    ),
)
```

---

## 200. Config Is Policy, Not Correctness

Changing batch size changes throughput, not semantics.

---

## 201. Android Permissions

Keep minimal.

Typically:

```text
INTERNET
network-state access if needed
notifications if used
```

---

## 202. iOS Entitlements

Enable only required:

```text
background modes
push
keychain groups
```

---

## 203. Privacy Declarations

Platform privacy declarations should match actual telemetry/data behavior.

---

## 204. Mobile Security Invariants

### AEQ-INV-MOBILE001

```text
No required sync state depends solely on Android/iOS process lifetime.
```

### AEQ-INV-MOBILE002

```text
Local domain mutation and outbox insertion remain atomic on mobile.
```

### AEQ-INV-MOBILE003

```text
Push notifications are scheduling hints and never authoritative state.
```

### AEQ-INV-MOBILE004

```text
Background execution timeout cannot advance a cursor beyond durably applied state.
```

### AEQ-INV-MOBILE005

```text
Private key material uses approved platform secure storage or equivalent provider.
```

### AEQ-INV-MOBILE006

```text
Resource adaptation may reduce throughput but never weaken domain, authorization, audit, or conflict semantics.
```

---

## 205. Additional Mobile Invariants

### AEQ-INV-MOBILE007

```text
An app upgrade preserves pending user intent or fails without partially destroying it.
```

### AEQ-INV-MOBILE008

```text
When durable storage is unavailable, Aequora does not report a new mutation as saved.
```

### AEQ-INV-MOBILE009

```text
Platform/UI code cannot directly advance cursors, mutate the server operation ledger, or bypass domain operations.
```

---

## 206. Recommended API

```rust
pub struct MobileAequoraClient {
    inner: AequoraClient,
}

impl MobileAequoraClient {
    pub async fn open(...);
    pub async fn mutate<O: Operation>(...);
    pub async fn sync_now(...);
    pub async fn sync_once_with_budget(...);
    pub fn events(&self) -> EventStream;
    pub async fn status(...);
}
```

---

## 207. Platform Event Input

```rust
pub enum MobilePlatformEvent {
    Foreground,
    Background,
    NetworkChanged(NetworkContext),
    PowerChanged(PowerContext),
    PushHint(SyncHint),
    BackgroundBudgetGranted(MobileSyncBudget),
}
```

---

## 208. Coordinator Input

All platform events flow into one Rust coordinator.

---

## 209. Coordinator State

```text
Idle
SyncRequested
Running
Backoff
WaitingForNetwork
WaitingForBudget
NeedsBootstrap
CompatibilityBlocked
```

---

## 210. No Competing Schedulers

OS layer grants execution opportunities.

Rust scheduler decides the sync work.

---

## 211. Mobile Live Sync

Foreground may keep WebSocket/SSE hints.

Background may disconnect.

---

## 212. Presence

Presence remains TTL-based and advisory.

---

## 213. App Suspension

Closing live connection is normal.

---

## 214. Mobile Authority Failover

Client persists highest trusted AuthorityEpoch.

---

## 215. Old Server/Rollback Detection

Lower epoch causes sync to fail closed.

---

## 216. Rebootstrap on Epoch Change

Policy determines safe continuation/rebootstrap/manual recovery.

---

## 217. Multi-Account Push Routing

Push carries opaque store/account hint.

---

## 218. Store Locking

Only one active coordinator owns one store.

---

## 219. Android Auto Backup

Automatic backup must not blindly clone device identity or sensitive local state.

---

## 220. iCloud Backup

Same concern.

---

## 221. Restored Mobile Backup

Detect:

```text
device identity mismatch
stale token
old pending operations
```

---

## 222. Restore Policy

```text
re-register device
classify pending operations
rebootstrap if required
```

---

## 223. Never Clone Device Credentials Blindly

Two devices must not silently share one identity.

---

## 224. DeviceBindingGeneration

Optional generation helps distinguish restored/rotated binding.

---

## 225. Reinstallation

Treat as a new local replica if local state/keys are lost.

---

## 226. Offline Pending Data on Uninstall

Cannot be recovered unless an explicit backup/export product feature exists.

---

## 227. Sign-Out With Pending Operations

Product must explicitly choose preserve/block/discard-with-warning behavior.

---

## 228. Backup of Pending Intent

If supported, backup outbox and base state consistently.

---

## 229. Push Registration Security

Push token is bound to authenticated DeviceId.

---

## 230. Token Rotation

FCM/APNs rotation is handled idempotently.

---

## 231. Battery Optimization Exceptions

Do not require disabling OS battery optimization for correctness.

---

## 232. Offline Horizon

Product defines maximum supported offline horizon relative to journal retention and compatibility.

---

## 233. Long-Offline Client

Reconnect flow:

```text
compatibility check
epoch check
journal floor check
↓
incremental sync / rebootstrap / upgrade required
```

---

## 234. Mobile UX States

Expose:

```text
SavedLocally
WaitingForNetwork
Syncing
ServerConfirmed
Conflict
NeedsRebootstrap
UpgradeRequired
StorageCritical
AuthenticationRequired
```

---

## 235. Accessibility

Do not encode status using color alone.

---

## 236. Responsiveness

Never block UI for sync/bootstrap/hash work.

---

## 237. Migration UX

Long migration should expose progress.

---

## 238. Migration Failure

Fail closed and preserve recoverable diagnostics.

---

## 239. Startup Time

Use bounded startup checks, not full DB scans.

---

## 240. Maintenance Work

Run compaction/integrity maintenance under suitable resource conditions.

---

## 241. Mobile Anti-Entropy

Incremental, not frequent whole-DB scans.

---

## 242. Repair Priority

Real divergence may elevate repair while still staying bounded.

---

## 243. Blob Cache

Large cached blobs may be evictable.

---

## 244. Offline Required Blob

User/product may pin it.

---

## 245. Blob Quota

Product-defined.

---

## 246. Android Cache Placement

Use internal cache/files according to durability needs.

---

## 247. iOS Cache Placement

Rebuildable data goes to cache; required offline data to protected persistent storage.

---

## 248. Mobile Observability

Measure privacy-safe operational metrics such as sync success, outbox age, bootstrap failures, and storage pressure.

---

## 249. Minimal Telemetry

Do not collect unnecessary fine-grained user behavior.

---

## 250. Diagnostics IDs

Prefer OperationId, ScopeId, ErrorCode, BuildId over business payloads.

---

## 251. Recommended Build Milestones

### Milestone 1

```text
Android foreground-only
iOS foreground-only
local DB
manual sync
```

### Milestone 2

```text
background wake
network monitoring
secure storage
push hints
```

### Milestone 3

```text
snapshot resume
blob background transfer
resource-aware scheduler
```

### Milestone 4

```text
upgrade/rebootstrap
diagnostics
certification
```

---

## 252. Why Foreground-First

It validates mobile correctness before adding OS background complexity.

---

## 253. Android-First Release

Implement Android platform adapter first while preserving iOS-neutral core traits.

---

## 254. Later iOS Support

Implement the same traits with iOS adapters.

No sync-core rewrite should be required.

---

## 255. Cross-Platform Contract

Android and iOS normalize into the same Rust concepts:

```text
NetworkContext
PowerContext
AppLifecycle
SecureStore
BackgroundExecutionHost
```

---

## 256. Completion Criteria

```text
[ ] shared Rust mobile core defined
[ ] Android integration defined
[ ] iOS integration defined
[ ] Dioxus integration clarified
[ ] local DB ownership defined
[ ] process-kill recovery defined
[ ] WorkManager/BGTask integration defined
[ ] push-hint semantics defined
[ ] network/power/thermal normalization defined
[ ] secure key storage defined
[ ] resource budgets defined
[ ] snapshot/blob streaming defined
[ ] FFI/binding architecture defined
[ ] mobile migrations/upgrades defined
[ ] backup/restore/reinstall behavior defined
[ ] diagnostics defined
[ ] packaging/release architecture defined
[ ] mobile conformance tests defined
[ ] mobile invariants added
```

---

## 257. Final Architecture

```text
                     MOBILE APPLICATION
                  Android / iOS / Dioxus
                            │
                            ▼
                 Platform Integration Layer
          lifecycle / background / network / keys
                            │
                            ▼
                    Aequora Mobile Runtime
                            │
               ┌────────────┼────────────┐
               ▼            ▼            ▼
           Scheduler     Local Store   Diagnostics
               │            │
               │            ├── Domain State
               │            ├── Outbox
               │            ├── Cursor
               │            └── Conflicts
               │
               ▼
                      Sync Coordinator
                            │
                    bounded sync budget
                            │
                            ▼
                    HTTPS + Postcard
                            │
                            ▼
                     Aequora Server

Push:
APNs / FCM
    │
    ▼
Wake Hint
    │
    ▼
Rust Scheduler
    │
    ▼
Normal Journal Catch-Up

Background:
WorkManager / BGTask
    │
    ▼
Grant Execution Opportunity
    │
    ▼
Rust sync_once_with_budget()
    │
    ▼
checkpoint + exit
```

---

## 258. Final Recommendation

Aequora **already has the correct synchronization model for Android and iOS**, so it does not need another mobile-specific sync protocol or a second sync engine.

It does need the mobile runtime architecture defined here:

```text
Android/iOS lifecycle adapters
background-task adapters
secure-store adapters
network/power adapters
process-death recovery
binding/packaging layers
mobile-specific resource policies
```

The key architectural decision is:

> **Keep one synchronization implementation in Rust and treat Android/iOS as constrained execution environments around that core.**

This allows the same Aequora engine to support:

```text
Dioxus Android
Dioxus iOS
Kotlin Android
Swift iOS
```

without duplicating outbox, cursor, conflict, retry, snapshot, or idempotency logic in each platform language.
