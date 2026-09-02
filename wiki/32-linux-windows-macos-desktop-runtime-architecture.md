# Aequora Sync — Part 32

# Linux, Windows, and macOS Desktop Runtime, OS Integration, and Deployment Architecture

## 1. Purpose

Aequora's core synchronization architecture already supports desktop applications conceptually, but production desktop deployments introduce a different set of operating constraints from mobile and server environments.

Desktop software may run as:

```text
single-process GUI
GUI + background sync agent
CLI + GUI sharing one local store
system tray application
multi-user workstation software
enterprise-managed installation
portable application
```

Desktop environments also introduce:

```text
multiple local processes
long-lived sessions
sleep/resume
network transitions
filesystem integrations
OS credential stores
auto-start services
self-update behavior
multi-window UI
```

Therefore:

> **Aequora should use the same synchronization engine on desktop, but provide a dedicated desktop runtime architecture around process coordination, OS integration, IPC, secure storage, lifecycle, and packaging.**

The central rule is:

> **Desktop persistence and process coordination must remain correct even when multiple application surfaces use the same local replica concurrently.**

---

# 2. Goals

Part 32 defines:

```text
Linux runtime
Windows runtime
macOS runtime
in-process desktop mode
sidecar/daemon mode
multi-process coordination
desktop IPC
secure storage
sleep/resume handling
network monitoring
background sync agents
autostart
filesystem integration
multi-window behavior
packaging
self-update strategy
enterprise deployment
diagnostics
desktop conformance
```

---

# 3. Non-Goals

This architecture does not:

```text
change Aequora protocol semantics
require a daemon for every desktop app
depend on one desktop UI framework
require Electron/WebView
require root/admin privileges
```

---

# 4. Desktop Integration Modes

Aequora should support two primary desktop modes.

```text
Mode A — In-Process Client
Mode B — Local Aequora Agent
```

---

# 5. Mode A — In-Process Client

Best for:

```text
pure Rust Dioxus app
simple desktop app
single process
single local store
```

Architecture:

```text
Dioxus Desktop App
      │
      ▼
Aequora Client
      │
      ├── Local DB
      ├── Outbox
      ├── Scheduler
      └── HTTPS Transport
```

---

# 6. Mode B — Local Agent

Best for:

```text
GUI + CLI
multiple desktop processes
Java/Python/.NET/Electron host
background sync while UI is closed
```

Architecture:

```text
       GUI
        │
        ├────────────┐
        │            │
       CLI        Tray App
        │            │
        └──────┬─────┘
               ▼
          Local IPC
               │
               ▼
        aequora-agent
               │
        ┌──────┼──────┐
        ▼      ▼      ▼
      Store  Sync   Crypto
```

---

# 7. Recommended Rust Desktop Mode

For a Dioxus desktop application:

```text
start in-process
```

and only introduce an agent when:

```text
background sync while UI closed
multi-process CLI integration
shared store ownership
```

actually becomes necessary.

---

# 8. Desktop Runtime Crates

Suggested:

```text
aequora-desktop-runtime
aequora-platform-linux
aequora-platform-windows
aequora-platform-macos
aequora-agent
aequora-ipc-protocol
```

---

# 9. Desktop Runtime Responsibilities

`aequora-desktop-runtime` should own:

```text
session lifecycle
sleep/resume
desktop sync policy
local coordinator
agent connectivity
OS integration abstraction
```

---

# 10. Platform Traits

Define:

```rust
pub trait DesktopLifecycleSource;
pub trait DesktopNetworkMonitor;
pub trait DesktopSecureStore;
pub trait DesktopAutostart;
pub trait DesktopNotificationHost;
pub trait DesktopFileIntegration;
```

---

# 11. Shared Rust Core

Desktop still uses:

```text
aequora-client
aequora-sync-core
aequora-storage
aequora-protocol
aequora-crypto
```

Platform crates stay thin.

---

# 12. Multi-Process Coordination

Desktop is where Part 05 becomes especially important.

Possible local actors:

```text
GUI
CLI
background worker
tray process
service
```

Only one sync coordinator should own one store.

---

# 13. Durable Lease

Use:

```text
LocalStoreId
ProcessInstanceId
LeaseExpiry
FencingToken
```

---

# 14. Fencing Requirement

A stale process must not:

```text
advance cursor
checkpoint bootstrap
perform store migration
run maintenance
```

after ownership changes.

---

# 15. Agent Mode Simplifies Ownership

If `aequora-agent` owns the local DB:

```text
GUI never opens DB directly
CLI never opens DB directly
```

All operations go through IPC.

This eliminates many multi-process hazards.

---

# 16. Local IPC

Recommended:

Linux/macOS:

```text
Unix domain sockets
```

Windows:

```text
named pipes
```

Fallback:

```text
loopback TCP with authentication
```

---

# 17. IPC Protocol

Use:

```text
Postcard framed messages
```

with explicit:

```text
IpcProtocolVersion
MessageKind
PayloadLength
```

---

# 18. IPC Security

Require:

```text
local user identity
socket/pipe ACL
session token
```

Never bind agent control API to public network interface by default.

---

# 19. IPC Commands

Examples:

```text
OpenStore
Mutate
Query
SyncNow
GetStatus
Subscribe
ResolveConflict
ExportDiagnostics
ShutdownAgent
```

---

# 20. IPC Events

Examples:

```text
SyncStatusChanged
DataChanged
ConflictCreated
AgentStateChanged
UpdateAvailable
```

---

# 21. Desktop Store Ownership

Preferred:

```text
one Aequora local store
```

for all synchronized domain state and metadata.

---

# 22. Avoid Dual Local Databases

If UI framework stores synchronized business state separately from Aequora metadata:

```text
local atomicity becomes harder
```

Prefer Aequora-owned synchronized state.

---

# 23. Local Store Location

Linux:

```text
$XDG_DATA_HOME/<app>/
```

Windows:

```text
%LOCALAPPDATA%\<app>\
```

macOS:

```text
~/Library/Application Support/<app>/
```

---

# 24. Cache Location

Use OS-specific cache paths for:

```text
rebuildable blob cache
temporary snapshot chunks
derived indexes
```

---

# 25. Config Location

Linux:

```text
$XDG_CONFIG_HOME/<app>/
```

Windows:

```text
%APPDATA% or application-specific config
```

macOS:

```text
~/Library/Preferences or Application Support
```

---

# 26. Secure Storage

Linux:

```text
Secret Service / libsecret-compatible provider
```

Windows:

```text
Windows Credential Manager / DPAPI
```

macOS:

```text
Keychain
```

---

# 27. SecureStore Abstraction

Same logical trait as mobile:

```rust
pub trait SecureStore {
    async fn put(...);
    async fn get(...);
    async fn delete(...);
}
```

---

# 28. Linux Fallback

If Secret Service unavailable:

```text
fail securely
or
use encrypted local secret store with explicit master-key policy
```

Do not silently store plaintext credentials.

---

# 29. Device Identity

Desktop installation has:

```text
DeviceId
DeviceBindingGeneration
device signing key
```

---

# 30. Multi-User Workstations

Device identity should be scoped to:

```text
OS user profile
```

unless product deliberately uses machine-wide identity.

---

# 31. Sleep / Resume

Desktop may suspend for hours.

On resume:

```text
invalidate stale network assumptions
refresh token if needed
check authority epoch
wake scheduler
```

---

# 32. Sleep During Sync

If network disappears during suspend:

```text
request may fail ambiguously
```

OperationId idempotency makes retry safe.

---

# 33. Network Monitoring

Linux:

```text
NetworkManager / system signals where available
```

Windows:

```text
Network List Manager / platform APIs
```

macOS:

```text
NWPathMonitor
```

Normalize to core `NetworkContext`.

---

# 34. VPN Changes

Treat VPN connect/disconnect as network path transition.

Do not assume stable local address.

---

# 35. Proxy Support

Respect enterprise/system proxy configuration where practical.

---

# 36. Captive Portal

Treat as degraded/unavailable until authenticated.

---

# 37. Foreground vs Background Policy

Desktop is less constrained than mobile.

But still distinguish:

```text
Interactive
Background
Bulk
Maintenance
```

---

# 38. Idle Maintenance

Good desktop opportunities:

```text
snapshot cleanup
anti-entropy
index rebuild
outbox compaction
```

---

# 39. Power State

Laptop desktop clients should consider:

```text
battery
charging
low-power mode
```

---

# 40. AC Power Optimization

Large maintenance tasks may prefer:

```text
charging + idle
```

---

# 41. Thermal Pressure

Laptop thermal state can reduce CPU-intensive work.

---

# 42. Desktop Live Sync

Foreground app may maintain:

```text
WebSocket/SSE hint channel
```

Desktop agent may keep it longer than mobile.

Still:

```text
live channel is advisory
```

---

# 43. Background Agent

Agent can continue syncing while GUI is closed.

---

# 44. Autostart

Optional.

Linux:

```text
XDG autostart / user systemd
```

Windows:

```text
Startup task / user service
```

macOS:

```text
LaunchAgent / login item
```

---

# 45. User-Level Service

Prefer user-level service over system-wide privileged service.

---

# 46. Agent Lifecycle

```text
Stopped
Starting
Running
Idle
Syncing
Updating
Stopping
Failed
```

---

# 47. Agent Crash Recovery

GUI should detect disconnected agent and:

```text
restart
or
surface error
```

according to policy.

---

# 48. No Data Loss on Agent Crash

Durable local store remains source of truth.

---

# 49. Agent Version Negotiation

GUI connects with:

```text
IpcProtocolVersion
BuildId
RegistryGeneration
```

---

# 50. Incompatible Agent

Host should display:

```text
AgentUpgradeRequired
```

not communicate with undefined behavior.

---

# 51. Single-Binary Mode

Aequora can also support:

```text
one desktop executable
```

embedding:

```text
UI
client
store
scheduler
```

This is ideal for simple distribution.

---

# 52. Multi-Binary Mode

Possible:

```text
app-ui
aequora-agent
app-cli
```

---

# 53. Portable Mode

Some users may want a portable folder.

This is possible but should explicitly define:

```text
store path
key storage
update policy
```

---

# 54. Portable Mode Security

If secure OS store unavailable/undesired:

```text
require explicit passphrase-encrypted key store
```

Never silently downgrade.

---

# 55. Dioxus Desktop Integration

Dioxus app can use `AequoraClient` directly.

Example architecture:

```text
Dioxus Component
↓
Application Service
↓
Aequora Client
↓
Repository
↓
Local Store
```

---

# 56. UI State

Do not mirror whole DB in Dioxus signals.

Use:

```text
paged queries
view models
change invalidation
```

---

# 57. Multi-Window

Multiple Dioxus windows should share:

```text
same application client handle
```

where process-local.

---

# 58. Multiple Processes

If windows are separate processes:

```text
use agent or lease/fencing
```

---

# 59. CLI Integration

CLI can connect to running agent.

Example:

```text
app-cli sync
app-cli status
app-cli conflicts
```

---

# 60. No Agent Running

CLI may:

```text
start ephemeral agent
or
open store exclusively
```

depending product design.

---

# 61. Preferred CLI Policy

If persistent agent architecture exists:

```text
CLI should use IPC
```

not bypass it.

---

# 62. File Association

Desktop apps may register:

```text
custom file extension
```

for imports/exports.

Imported file should become:

```text
validated import job
```

not direct DB writes.

---

# 63. Drag and Drop

UI receives path/handle.

Rust validates and imports.

---

# 64. File Watcher

If product watches directories:

```text
filesystem events are hints
```

Re-scan safely.

Do not trust one watcher event as guaranteed truth.

---

# 65. External Files

For document sync:

```text
content-addressed blob subsystem
```

should own hash/version state.

---

# 66. Open File While Offline

Local cached blob can open.

Changes create:

```text
new blob version + domain operation
```

---

# 67. OS Notifications

Desktop runtime can publish:

```text
sync conflict
authentication required
large import finished
```

---

# 68. Notification Privacy

Avoid exposing sensitive content on lock screen by default.

---

# 69. Tray/Menu Bar Integration

Optional:

```text
sync status
pause/resume
open app
diagnostics
```

---

# 70. Pause Sync

User pause should pause:

```text
normal/background sync
```

but product may still require security-critical metadata operations.

---

# 71. Offline Mode

Explicit offline mode can prevent network use.

Local mutations continue.

---

# 72. Metered Desktop Network

Windows/macOS/Linux laptops may use metered tethering.

Honor `NetworkContext`.

---

# 73. Update Architecture

Desktop self-update is more flexible than mobile app-store updates.

But update must preserve:

```text
local store compatibility
pending operations
agent/UI compatibility
```

---

# 74. Update Sequence

Safe sequence:

```text
download update
↓
verify signature/hash
↓
quiesce agent/client
↓
checkpoint
↓
install
↓
restart
↓
run metadata migration
↓
resume
```

---

# 75. Signed Updates

Update artifact should be:

```text
cryptographically signed
```

for production distributions.

---

# 76. Update Rollback

Only safe if:

```text
older binary supports current local store
```

Otherwise rollback must also restore compatible store snapshot, which is dangerous.

Prefer forward-fix over unsafe downgrade.

---

# 77. Agent/UI Version Skew

During staged update:

```text
new UI + old agent
old UI + new agent
```

may briefly coexist.

IPC compatibility rules must handle this.

---

# 78. Update Compatibility Window

Support at least:

```text
N and N-1
```

for IPC where practical.

---

# 79. Linux Packaging

Possible:

```text
AppImage
Flatpak
native distro package
tarball
```

---

# 80. Linux Sandboxing

Flatpak may affect:

```text
filesystem access
secret service
socket paths
```

Provide portal-aware integration where needed.

---

# 81. Windows Packaging

Possible:

```text
MSIX
MSI
portable ZIP
```

---

# 82. macOS Packaging

Possible:

```text
.app bundle
DMG
PKG
```

---

# 83. macOS Signing

Production distribution may require:

```text
code signing
notarization
```

---

# 84. Windows Signing

Sign executables/installers.

---

# 85. Linux Signing

Provide:

```text
release checksums/signatures
```

and package-repository trust where applicable.

---

# 86. Auto-Update Provider

Treat update service as external provider.

Do not let update transport bypass verification.

---

# 87. Enterprise Deployment

Support:

```text
silent install
managed config
proxy
certificate policy
disabled self-update
central policy
```

---

# 88. Managed Config

Desktop app can load organization policy from:

```text
signed local config
MDM-like mechanism
environment/file deployment
```

---

# 89. Policy Precedence

Recommended:

```text
security/enterprise policy
> application config
> user preference
```

---

# 90. Certificate Pinning

Use only when operationally justified.

Enterprise proxies may complicate pinning.

---

# 91. Private CA

Enterprise deployments may require custom trust roots.

Make this explicit and auditable.

---

# 92. Local Firewall

Application should not require inbound firewall exceptions in normal client mode.

---

# 93. Agent IPC Firewall

Use local sockets/pipes, avoiding TCP where possible.

---

# 94. Diagnostics

Desktop diagnostic bundle should include:

```text
app build
agent build
OS
store metadata version
outbox summary
scope cursors
recent sync state
network state
```

---

# 95. No Secrets

Never include:

```text
credentials
private keys
full sensitive payloads
```

---

# 96. Local Log Rotation

Logs must be bounded.

---

# 97. Crash Reports

Native crash reports may include:

```text
build ID
stack trace
module list
```

but redact secrets/business data.

---

# 98. Windows Event Log

Optional enterprise integration.

---

# 99. Linux Journald

Optional when agent runs as user service.

---

# 100. macOS Unified Logging

Optional `os_log` bridge.

---

# 101. Support Export

UI can create encrypted/sanitized `.aeqincident` bundle.

---

# 102. File Permissions

Local store should be restricted to current user.

---

# 103. Shared Machine

Do not put synchronized DB in world-readable location.

---

# 104. Screen Lock

Product may require session re-auth after long lock.

---

# 105. Lock Event

Desktop lifecycle can emit:

```text
SessionLocked
SessionUnlocked
```

---

# 106. Sensitive UI

May hide sensitive content on lock.

This is product policy, not sync semantics.

---

# 107. Logout

Same as mobile:

```text
preserve local store
purge local store
block if unsynced
```

must be explicit.

---

# 108. Multiple Accounts

Prefer:

```text
one store per account/profile
```

---

# 109. Account Switch

Quiesce current coordinator before opening another store.

---

# 110. Store Registry

Desktop runtime may maintain:

```text
known profile stores
```

outside each store.

---

# 111. Profile Metadata

Keep minimal:

```text
display label
store path
last opened
```

No secrets.

---

# 112. Backup

Desktop users may copy local app data manually.

Need restore detection.

---

# 113. Restored Local Store

Check:

```text
DeviceId
DeviceBindingGeneration
AuthorityEpoch
store generation
```

---

# 114. Cloned Store

If same store is copied to another machine:

```text
detect device binding mismatch
```

and re-register as a new device/replica.

---

# 115. Never Clone Credentials Blindly

Same principle as mobile.

---

# 116. Local Export

If user needs portable backup/export:

```text
create signed/encrypted export package
```

through explicit API.

---

# 117. Import

Import becomes a durable migration/import job.

---

# 118. Desktop Blob Cache

May be larger than mobile.

Still:

```text
bounded
evictable
policy-driven
```

---

# 119. Storage Pressure

Detect free-space thresholds.

---

# 120. Low Disk

Pause:

```text
large bootstrap
blob download
maintenance
```

---

# 121. Critical Disk

Reject new mutation if transaction cannot be safely durable.

---

# 122. Desktop Snapshot

Can use larger chunk size than mobile.

---

# 123. CPU Budget

Desktop may allow more Rayon workers.

Still bounded.

---

# 124. Hardware Profile

Could classify:

```text
Low
Standard
High
```

---

# 125. Adaptive Batch

Desktop foreground/AC power can use larger batches.

---

# 126. Anti-Entropy

Desktop is a good place for periodic background integrity scans.

---

# 127. Maintenance Window

Prefer idle/charging.

---

# 128. Agent Scheduled Maintenance

Agent can persist:

```text
next maintenance due
```

---

# 129. Change Feed Local Consumers

Desktop application may have local projections:

```text
search
thumbnail index
notifications
```

They should use local durable projection cursors where needed.

---

# 130. Search Index

Derived.

Can rebuild.

Do not make search index authoritative.

---

# 131. Local Plugin Integration

Avoid arbitrary in-process plugins initially.

Prefer:

```text
IPC extension
external tool
registered integration
```

---

# 132. Security Risk of Plugins

Native plugins can compromise:

```text
credentials
DB
process
```

So plugin runtime should be separate design if ever needed.

---

# 133. Desktop Sandboxing

Where applicable:

```text
Flatpak
macOS sandbox
Windows app container
```

platform adapter must expose required capabilities explicitly.

---

# 134. Sandboxed File Access

Use user-approved file handles/portals.

---

# 135. Notification Permissions

macOS/Windows may require permission depending channel.

Gracefully degrade.

---

# 136. Browser-Based Desktop Shells

If product uses Electron/Tauri-like shell, Aequora should still live in:

```text
Rust native process/agent
```

not JavaScript.

---

# 137. Tauri

If Rust backend is available, Aequora can be integrated natively.

---

# 138. Electron

Prefer sidecar agent.

---

# 139. JavaFX

Prefer agent or JNI.

---

# 140. .NET

Prefer agent or C ABI.

---

# 141. Python GUI

Prefer agent for isolation.

---

# 142. Offline Installer

Enterprise/offline installation may package:

```text
app
agent
runtime
```

without internet dependency.

---

# 143. Initial Bootstrap Without Internet

Possible if product supports:

```text
seed package/import
```

then later synchronize when network available.

---

# 144. Air-Gapped Mode

Aequora can still act as:

```text
local operation engine
```

without remote authority only if product explicitly defines a local authority mode.

Do not accidentally treat unsynced client as authoritative.

---

# 145. Local Authority Profile

Separate deployment profile if required.

---

# 146. Desktop Peer-to-Peer

Not part of baseline Part 32.

Future transport may use P2P, but authority semantics remain explicit.

---

# 147. Testing

Desktop test layers:

```text
core tests
platform integration tests
agent IPC tests
multi-process tests
sleep/resume tests
upgrade tests
packaging tests
```

---

# 148. Multi-Process Test

Run:

```text
GUI
CLI
agent
```

and verify one coordinator.

---

# 149. Stale Lease Test

Kill agent, start replacement, revive old process.

Old fencing token must fail.

---

# 150. Sleep Test

Suspend during sync.

Resume and retry safely.

---

# 151. Network Switch Test

Wi-Fi → Ethernet → VPN.

No duplicate effect.

---

# 152. Agent Crash Test

Kill during reconcile.

Restart preserves cursor correctness.

---

# 153. UI Crash Test

Agent continues syncing if configured.

---

# 154. Update Test

Old UI/new agent and new UI/old agent within supported skew.

---

# 155. Store Migration Test

Pending outbox survives update.

---

# 156. Low-Disk Test

Mutation fails safely when durability impossible.

---

# 157. Clone Store Test

Copy local store to another machine.

Device-binding mismatch detected.

---

# 158. Secure Store Test

Credentials inaccessible to unauthorized user/process.

---

# 159. IPC ACL Test

Different OS user cannot connect to agent.

---

# 160. Portable Mode Test

Encrypted secret store required.

---

# 161. Enterprise Proxy Test

Sync works through configured proxy.

---

# 162. Desktop Conformance Profile

Create:

```text
DesktopClientFull
```

---

# 163. Agent Conformance Profile

Create:

```text
DesktopAgentFull
```

---

# 164. Desktop Certification Requirements

Must verify:

```text
atomic local outbox
multi-process fencing
sleep/resume recovery
IPC versioning
secure store
upgrade preservation
low-disk behavior
```

---

# 165. Desktop Invariants

## AEQ-INV-DESKTOP001

```text
At most one active sync coordinator owns a given local store at a time.
```

## AEQ-INV-DESKTOP002

```text
A stale desktop process cannot commit coordinator-owned metadata after losing its fencing token.
```

## AEQ-INV-DESKTOP003

```text
Desktop sleep/resume cannot cause a duplicated authoritative effect.
```

## AEQ-INV-DESKTOP004

```text
Local IPC cannot bypass Aequora domain operations or directly mutate synchronization metadata.
```

## AEQ-INV-DESKTOP005

```text
A local store copied to another machine cannot silently continue using the same trusted device binding.
```

## AEQ-INV-DESKTOP006

```text
Application upgrades preserve pending user intent or fail before destructive migration.
```

---

# 166. Additional Invariants

## AEQ-INV-DESKTOP007

```text
The background agent is optional for correctness; if absent, in-process mode preserves the same synchronization semantics.
```

## AEQ-INV-DESKTOP008

```text
Low disk or OS credential-store failure results in explicit degraded/failure state, not false successful persistence.
```

## AEQ-INV-DESKTOP009

```text
Derived local indexes, search caches, and tray/UI state never become authoritative business state.
```

---

# 167. Recommended Workspace

```text
crates/
├── aequora-client/
├── aequora-desktop-runtime/
├── aequora-agent/
├── aequora-ipc-protocol/
├── aequora-platform-linux/
├── aequora-platform-windows/
├── aequora-platform-macos/
├── aequora-diagnostics/
└── aequora-update/
```

---

# 168. Linux Adapter Layout

```text
aequora-platform-linux/
├── paths.rs
├── network.rs
├── power.rs
├── secure_store.rs
├── autostart.rs
├── notifications.rs
└── lifecycle.rs
```

---

# 169. Windows Adapter Layout

```text
aequora-platform-windows/
├── paths.rs
├── network.rs
├── power.rs
├── credential_manager.rs
├── autostart.rs
├── notifications.rs
└── lifecycle.rs
```

---

# 170. macOS Adapter Layout

```text
aequora-platform-macos/
├── paths.rs
├── network.rs
├── power.rs
├── keychain.rs
├── login_item.rs
├── notifications.rs
└── lifecycle.rs
```

---

# 171. Agent Layout

```text
aequora-agent/
├── main.rs
├── ipc.rs
├── coordinator.rs
├── lifecycle.rs
├── store_registry.rs
├── update.rs
└── diagnostics.rs
```

---

# 172. IPC Protocol Layout

```text
aequora-ipc-protocol/
├── version.rs
├── command.rs
├── event.rs
├── framing.rs
└── errors.rs
```

---

# 173. Desktop Runtime API

Concept:

```rust
pub struct DesktopAequoraClient {
    client: AequoraClient,
    runtime: DesktopRuntime,
}
```

---

# 174. In-Process Open

```rust
DesktopAequoraClient::open(config).await
```

---

# 175. Agent Client

```rust
AequoraAgentClient::connect().await
```

---

# 176. Mode Selection

Application config:

```ron
desktop: (
    runtime_mode: Auto,
)
```

where `Auto` may mean:

```text
use running agent if present
else in-process
```

Only use this if store locking semantics are robust.

---

# 177. Simpler Recommendation

For v1:

```text
application chooses one mode explicitly
```

Avoid runtime ambiguity.

---

# 178. Background Policy

Example:

```ron
desktop: (
    background_sync: true,
    sync_on_resume: true,
    maintenance_on_ac_power: true,
)
```

---

# 179. Update Policy

Example:

```ron
updates: (
    channel: Stable,
    auto_download: true,
    auto_install: false,
)
```

---

# 180. Enterprise Policy

May override:

```text
update channel
proxy
autostart
telemetry
certificate roots
```

---

# 181. Completion Criteria

Part 32 is complete when:

```text
[ ] Linux runtime defined
[ ] Windows runtime defined
[ ] macOS runtime defined
[ ] in-process mode defined
[ ] agent mode defined
[ ] IPC architecture defined
[ ] multi-process fencing defined
[ ] sleep/resume behavior defined
[ ] secure storage defined
[ ] OS path rules defined
[ ] autostart/background sync defined
[ ] multi-window/CLI integration defined
[ ] file integration defined
[ ] update architecture defined
[ ] packaging defined
[ ] enterprise deployment defined
[ ] clone/restore handling defined
[ ] diagnostics defined
[ ] desktop conformance defined
[ ] desktop invariants added
```

---

# 182. Final Architecture

```text
                  DESKTOP APPLICATION SURFACES
             GUI / CLI / Tray / Background Task
                         │
                         ▼
             ┌───────────────────────┐
             │ Integration Strategy  │
             └───────────┬───────────┘
                         │
            ┌────────────┴────────────┐
            ▼                         ▼
      In-Process Mode            Agent Mode
            │                         │
            │                    Local IPC
            │                         │
            └────────────┬────────────┘
                         ▼
                 Aequora Rust Client
                         │
             ┌───────────┼───────────┐
             ▼           ▼           ▼
         Local DB    Scheduler    Secure Store
             │
             ├── Domain State
             ├── Outbox
             ├── Cursor
             └── Conflicts
                         │
                         ▼
                  HTTPS + Postcard
                         │
                         ▼
                  Aequora Server

OS integration:

Linux:
    XDG paths / Secret Service / user systemd

Windows:
    LocalAppData / Credential Manager / named pipes

macOS:
    Application Support / Keychain / LaunchAgent
```

---

# 183. Final Recommendation

Aequora does not need a second desktop synchronization engine.

It needs a dedicated desktop integration layer that handles:

```text
multi-process coordination
local IPC
desktop lifecycle
secure credential storage
sleep/resume
OS-specific paths
background agents
updates
packaging
```

For a pure Rust Dioxus desktop application, start with:

```text
in-process Aequora client
```

For applications that need:

```text
GUI + CLI
sync while UI is closed
multiple host languages
```

use:

```text
aequora-agent + local IPC
```

The key architectural principle is:

> **Keep synchronization semantics identical across Linux, Windows, and macOS; isolate desktop-specific behavior behind platform adapters and process-coordination boundaries.**
