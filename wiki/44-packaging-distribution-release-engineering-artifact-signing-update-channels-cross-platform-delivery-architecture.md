# Aequora Sync — Part 44

# Packaging, Distribution, Release Engineering, Artifact Signing, Update Channels, and Cross-Platform Delivery Architecture

## 1. Purpose

Aequora now has core synchronization semantics, storage adapters, server/client integrations, CLI tooling, and configuration architecture.

Part 44 defines how those components become trusted release artifacts.

The central rule is:

> **A release is not merely a compiled binary; it is a reproducible, signed, versioned, compatibility-governed artifact set with traceable provenance and a controlled upgrade path.**

This part covers:

```text
release versioning
workspace release coordination
build profiles
reproducibility
cross-platform compilation
artifact layout
package formats
signing
checksums
SBOMs
provenance
update channels
rollout
rollback
migration compatibility
server/client skew
CLI distribution
mobile delivery
desktop delivery
container images
release CI/CD
release gates
incident/revocation handling
```

---

# 2. Release Scope

Aequora may produce multiple kinds of artifacts:

```text
Rust crates
CLI binaries
server binaries
desktop agent binaries
desktop GUI applications
Android libraries/apps
iOS frameworks/apps
container images
migration bundles
registry artifacts
configuration schemas
conformance reports
debug symbols
SBOMs
signatures
checksums
documentation bundles
```

They belong to one coordinated release train where practical.

---

# 3. Release Units

Recommended logical release units:

```text
Aequora Core SDK
Aequora Server
Aequora CLI
Aequora Desktop Agent
Aequora Dioxus Client Integration
Aequora Mobile Runtime
PostgreSQL Adapter
SQLite Adapter
Stoolap Adapter
```

Initially, lockstep workspace versions are simplest.

Example:

```text
aequora = 1.4.0
aequora-client = 1.4.0
aequora-server-core = 1.4.0
aequora-postgres = 1.4.0
aequora-sqlite = 1.4.0
aequora-stoolap = 1.4.0
```

---

# 4. Semantic Versioning

Use SemVer for crate/API releases:

```text
MAJOR.MINOR.PATCH
```

But do not confuse crate version with:

```text
ProtocolVersion
OperationSchemaVersion
StoreFormatVersion
ConfigSchemaVersion
SnapshotSchemaVersion
RegistryGeneration
```

These evolve independently.

---

# 5. Version Dimensions

A release manifest should record:

```text
product version
git commit
build ID
Rust toolchain
target triple
enabled Cargo features
protocol range
registry generation
registry digest
config schema version
local store format versions
adapter versions
migration set digest
```

---

# 6. Release Manifest

Every release should produce a machine-readable manifest.

Concept:

```rust
pub struct ReleaseManifest {
    pub release_version: Version,
    pub git_commit: GitCommit,
    pub build_id: BuildId,
    pub protocol_range: ProtocolRange,
    pub registry_generation: RegistryGeneration,
    pub artifact_set: Vec<ArtifactDescriptor>,
}
```

---

# 7. Artifact Descriptor

Concept:

```rust
pub struct ArtifactDescriptor {
    pub name: ArtifactName,
    pub target: TargetTriple,
    pub sha256: Digest,
    pub blake3: Digest,
    pub size: u64,
    pub signature: SignatureRef,
    pub sbom: Option<ArtifactRef>,
}
```

---

# 8. Build Reproducibility

Aequora should aim for reproducible or near-reproducible builds.

Control:

```text
Rust toolchain version
Cargo.lock
build flags
environment
timestamps
path embedding
linker/toolchain
source revision
```

---

# 9. Toolchain Pinning

Use:

```text
rust-toolchain.toml
```

or equivalent pinning.

CI and local release builds should resolve the same Rust channel/toolchain.

---

# 10. Cargo.lock

For binaries and applications:

```text
Cargo.lock is committed
```

and release builds use locked dependency resolution.

---

# 11. `--locked`

Release CI should build with:

```text
cargo build --locked
```

and equivalent locked workflows.

---

# 12. Build Profiles

Recommended:

```toml
[profile.release]
lto = "thin"
codegen-units = 1
panic = "abort"
strip = false
```

Exact settings should be benchmarked and platform-specific where necessary.

---

# 13. Debug Symbols

Do not simply discard them.

Produce separate debug-symbol artifacts for:

```text
Linux
Windows
macOS
Android native code
```

Store them securely for incident analysis.

---

# 14. Stripping

Distributable artifacts may be stripped, while debug symbols are archived separately.

---

# 15. Deterministic Build Metadata

Embed only stable build metadata:

```text
version
commit
build ID
registry digest
```

Avoid volatile timestamps if they prevent reproducibility unless the timestamp is normalized.

---

# 16. Build ID

Use a deterministic or traceable build identifier.

Possible:

```text
hash(source commit + target + features + toolchain + release config)
```

---

# 17. Build Provenance

Record:

```text
source repository
source commit
builder identity
CI workflow
toolchain
dependencies
target
signing identity
```

This supports supply-chain verification.

---

# 18. SLSA-Style Provenance

Aequora should be architected to support verifiable build provenance.

Exact implementation can mature over time, but release artifacts should always be traceable to:

```text
source
builder
parameters
outputs
```

---

# 19. SBOM

Generate an SBOM for release artifacts.

Useful formats may include:

```text
CycloneDX
SPDX
```

The chosen release process should support at least one standard machine-readable format.

---

# 20. SBOM Contents

Include:

```text
crate dependencies
versions
licenses
source references
checksums
```

where available.

---

# 21. License Report

Produce a separate human-readable license report for shipped dependencies.

---

# 22. Supply-Chain Gates

Before release:

```text
cargo audit
cargo deny
license policy
known-vulnerability scan
forbidden-source check
SBOM generation
```

Part 49 will define deeper governance.

---

# 23. Source Policy

Release dependencies should come from controlled sources:

```text
crates.io
approved Git revisions
workspace paths
```

Avoid unpinned Git branches.

---

# 24. Git Dependencies

If required:

```text
pin exact commit
document reason
review license/security
```

---

# 25. Build Environment

Recommended:

```text
clean ephemeral CI runner
no developer-local state
locked dependency cache
explicit environment
```

---

# 26. Hermetic Direction

Aequora should move toward hermetic build environments where practical.

For example:

```text
containerized build environments
Nix
reproducible toolchain images
```

without making one packaging system mandatory.

---

# 27. Release Branching

A simple model:

```text
main
release/x.y
```

or trunk-based release tags.

Prefer low-complexity branching.

---

# 28. Release Tags

Signed Git tags:

```text
v1.4.0
```

should identify immutable source states.

---

# 29. Pre-Releases

Use SemVer pre-release labels:

```text
1.4.0-alpha.1
1.4.0-beta.2
1.4.0-rc.1
```

for unstable channels.

---

# 30. Release Channels

Recommended channels:

```text
Nightly
Alpha
Beta
ReleaseCandidate
Stable
LTS (future)
```

Not every product needs every channel.

---

# 31. Channel Semantics

### Nightly

```text
frequent
low guarantees
developer testing
```

### Alpha

```text
feature incomplete
breaking risk
```

### Beta

```text
feature mostly complete
broader testing
```

### RC

```text
release candidate
only release-blocking fixes
```

### Stable

```text
production supported
```

---

# 32. Server vs Client Channels

Server and client can use different rollout channels, but compatibility policy must define supported skew.

---

# 33. Compatibility Before Release

Before publishing a client/server release:

```text
old client -> new server
new client -> old supported server
rolling mixed server nodes
store migration path
protocol negotiation
```

must be tested.

---

# 34. Server Rolling Upgrade

Aequora server should support:

```text
N and N+1
```

mixed nodes during rolling deployment where protocol/DB compatibility allows it.

---

# 35. Expand-Contract

Database schema changes should follow:

```text
expand
deploy compatible code
migrate data if needed
switch behavior
contract later
```

Avoid one-step destructive schema changes.

---

# 36. Client Upgrade Skew

Mobile/desktop clients may remain old for long periods.

Server must define:

```text
minimum supported client
supported protocol range
deprecated operation schemas
rebootstrap policy
```

---

# 37. Offline Client Upgrade

Upgrade logic must preserve:

```text
pending outbox
cursor
conflicts
local store identity
scope state
```

---

# 38. Local Store Migration During Upgrade

Sequence:

```text
verify current format
backup/checkpoint if needed
apply migration
verify invariants
open client
```

Do not start sync against a partially migrated store.

---

# 39. Migration Bundle

Release artifacts should include:

```text
migration IDs
checksums
from/to versions
migration metadata
```

for each adapter.

---

# 40. Migration Digest

Release manifest records:

```text
MigrationSetDigest
```

to detect mismatch between binary and migration bundle.

---

# 41. Binary / Migration Compatibility

Server should refuse startup if:

```text
binary expects migration set A
database has incompatible state B
```

unless a supported migration path exists.

---

# 42. Packaging Targets

Primary native targets may include:

```text
x86_64-unknown-linux-gnu
aarch64-unknown-linux-gnu
x86_64-pc-windows-msvc
aarch64-pc-windows-msvc
x86_64-apple-darwin
aarch64-apple-darwin
Android ABIs
iOS device/simulator architectures
```

Exact support matrix should be explicitly documented.

---

# 43. Linux Distribution

Possible artifacts:

```text
tar.zst
deb
rpm
AppImage or equivalent for GUI
container image
```

Do not make all formats mandatory initially.

---

# 44. Linux Server

Recommended minimum:

```text
compressed binary archive
container image
```

Optionally:

```text
deb/rpm
```

as adoption grows.

---

# 45. Linux Desktop

Potential:

```text
AppImage
Flatpak
deb/rpm
tar.zst
```

Choose according to product distribution needs.

---

# 46. Windows Distribution

Possible:

```text
zip
MSI
MSIX
```

For enterprise deployment, MSI/MSIX can be useful.

---

# 47. Windows Signing

Production desktop binaries/installers should use Authenticode signing.

---

# 48. macOS Distribution

Possible:

```text
.app bundle
.dmg
.pkg
```

---

# 49. macOS Signing

Use:

```text
Developer ID signing
notarization
stapling where applicable
```

for trusted distribution outside the App Store.

---

# 50. Android Distribution

Possible Aequora outputs:

```text
AAR library
APK
Android App Bundle
```

Product apps typically ship through:

```text
Play Store
managed enterprise distribution
direct signed APK
```

depending deployment.

---

# 51. Android Signing

Release builds require controlled signing keys.

Private signing keys should live in secure CI secret infrastructure, not the repository.

---

# 52. iOS Distribution

Possible:

```text
XCFramework for Rust core/bindings
IPA through product app build
App Store / TestFlight
enterprise-managed distribution
```

---

# 53. iOS Signing

Use Apple signing/provisioning infrastructure.

Aequora's Rust core packaging must integrate cleanly into the native build pipeline.

---

# 54. Rust Library Distribution

Public crates can be published to crates.io when appropriate.

Internal/proprietary product crates can remain workspace/private registry artifacts.

---

# 55. Crate Publish Order

If workspace crates depend on each other:

```text
foundation
protocol
adapter SDK
client/server core
adapters
integrations
facade
```

Use automated dependency-aware publishing.

---

# 56. Crate Metadata

Every public crate should include:

```text
license
repository
documentation
description
rust-version
keywords/categories where useful
```

---

# 57. MSRV

Define a Minimum Supported Rust Version.

Example policy:

```text
MSRV changes only in minor releases
```

or whatever governance Aequora adopts.

---

# 58. MSRV Testing

CI should test the declared MSRV for public crates.

---

# 59. Container Images

Server/CLI/control-plane images should be minimal.

Recommended:

```text
multi-stage build
non-root runtime user
read-only filesystem where practical
minimal base
health endpoints
```

---

# 60. Container Image Tags

Examples:

```text
aequora-server:1.4.0
aequora-server:1.4
aequora-server:stable
```

Immutable digest should be the true deployment identity.

---

# 61. Never Rely on `latest`

`latest` may exist for convenience but production deployments should pin:

```text
version
or
image digest
```

---

# 62. Multi-Architecture Images

Publish manifest lists for:

```text
amd64
arm64
```

where supported.

---

# 63. Container Signing

Sign container images or their provenance metadata using a modern signing workflow.

---

# 64. Image SBOM

Attach or publish SBOM per image.

---

# 65. Artifact Signing Strategy

Each release should publish:

```text
checksums
signatures
release manifest
```

---

# 66. Hashes

Use at least one broadly interoperable hash:

```text
SHA-256
```

Aequora may also use:

```text
BLAKE3
```

internally and for fast verification.

---

# 67. Why Both

```text
SHA-256 -> ecosystem compatibility
BLAKE3   -> Aequora-native speed/integrity workflows
```

---

# 68. Signing Key Separation

Separate key purposes:

```text
Git tag signing
release artifact signing
container signing
application platform signing
update metadata signing
```

Avoid one universal private key.

---

# 69. Offline Root Key

For mature release security, keep a high-trust root key offline.

Use subordinate/online signing keys for routine releases where practical.

---

# 70. Key Rotation

Release metadata should support:

```text
multiple trusted keys
activation
deprecation
revocation
```

---

# 71. Key IDs

Signatures identify:

```text
SigningKeyId
algorithm
signature
```

not just raw signature bytes.

---

# 72. Update Metadata

Desktop auto-update should use signed metadata describing:

```text
version
channel
target platform
artifact hash
artifact size
minimum compatible version
criticality
```

---

# 73. Update Metadata Must Be Signed

Transport TLS alone is not sufficient.

The client verifies signed update metadata/artifacts.

---

# 74. Update Channel Config

Client may be configured for:

```text
Stable
Beta
Nightly
EnterprisePinned
```

---

# 75. Enterprise-Pinned

Enterprise environments may disable self-update and let:

```text
MDM
package manager
IT deployment
```

control versions.

---

# 76. Automatic Update Policy

Possible policies:

```text
NotifyOnly
DownloadAutomatically
InstallOnRestart
ManagedExternally
```

---

# 77. Server Updates

Server updates should usually be handled by deployment orchestration, not self-updating binaries.

---

# 78. Desktop Agent Update

If a desktop agent and GUI are separately packaged, define compatibility:

```text
GUI N compatible with agent N/N-1
```

or use lockstep versioning.

---

# 79. Atomic Desktop Update

Preferred pattern:

```text
download new artifact
verify signature/hash
stage
close old process
atomic replace
restart
health check
rollback if failed
```

---

# 80. Windows Locked Files

Update architecture must account for files that cannot be replaced while processes are running.

Use a small updater/bootstrap process or installer service where necessary.

---

# 81. macOS Update Constraints

Respect code-signature integrity and bundle replacement semantics.

---

# 82. Linux Update Modes

Depending packaging:

```text
package-manager update
Flatpak/AppImage update
self-update for standalone binary
```

Aequora core need not force one.

---

# 83. Rollback

Rollback is not merely reinstalling an old binary.

Must assess:

```text
DB schema
local store format
protocol compatibility
operation schema
config schema
```

---

# 84. Rollback Classes

Classify releases:

```text
BinaryRollbackSafe
RollbackRequiresMigration
ForwardOnly
```

---

# 85. Forward-Only Changes

Examples may include:

```text
irreversible schema transformation
new cryptographic state
authority epoch transition
```

These need explicit release notes and recovery strategy.

---

# 86. Client Rollback

If local store format was upgraded irreversibly, old client must refuse to open rather than corrupt state.

---

# 87. Server Rollback

Old server code must not start against an incompatible newer schema.

---

# 88. Release Notes

Every release should document:

```text
features
bug fixes
security fixes
protocol changes
migration requirements
deprecations
breaking changes
rollback class
minimum supported client/server
```

---

# 89. Generated Release Notes

Part 29 registry diffs can generate a significant portion automatically.

---

# 90. Security Advisories

Security releases should have:

```text
advisory ID
affected versions
fixed versions
severity
mitigation
upgrade instructions
```

---

# 91. Emergency Release

Emergency path must still preserve:

```text
source traceability
tests
signing
manifest
```

Do not bypass provenance because a release is urgent.

---

# 92. Revoked Release

A release can be marked:

```text
revoked
```

if discovered unsafe.

Clients/control-plane can warn or block according to policy.

---

# 93. Minimum Version Policy

Server may enforce:

```text
minimum client build
```

only when necessary for:

```text
security
semantic correctness
unsupported protocol
```

---

# 94. Forced Upgrade

Forced upgrade should be rare.

If required, server returns a precise compatibility error:

```text
UpdateRequired
```

with minimum version and reason class.

---

# 95. Offline Grace

If product semantics permit, old clients may continue local-only work during temporary update-required periods, but must not sync incompatible operations.

---

# 96. Canary Releases

Server release rollout:

```text
internal
small tenant cohort
larger cohort
full
```

---

# 97. Canary Metrics

Observe:

```text
error rate
sync latency
operation rejection changes
DB lock contention
CPU/memory
journal throughput
crash rate
compatibility failures
```

---

# 98. Automatic Rollback

Can be used for stateless server binaries where database compatibility remains safe.

Never blindly auto-rollback across irreversible migrations.

---

# 99. Blue-Green Deployment

Useful for server upgrades:

```text
Blue = current
Green = new
```

Both must be schema/protocol compatible during overlap.

---

# 100. Mobile Rollout

App stores can use staged rollout.

Server compatibility must account for users remaining on old versions.

---

# 101. Desktop Staged Rollout

Update service may assign stable rollout cohorts by:

```text
installation ID
tenant ID
channel
```

without exposing sensitive identifiers.

---

# 102. Release Candidate Conformance

Before stable:

```text
all official adapters
all platform profiles
all protocol fixtures
all migration tests
```

must pass.

---

# 103. Release Gate Categories

Recommended gates:

```text
Correctness
Compatibility
Security
Migration
Conformance
Performance
Documentation
Packaging
Provenance
```

---

# 104. Correctness Gate

Requires:

```text
unit tests
property tests
model tests
fault tests
```

appropriate to changed components.

---

# 105. Compatibility Gate

Requires:

```text
protocol matrix
client/server skew
store upgrade
config upgrade
registry compatibility
```

---

# 106. Security Gate

Requires:

```text
dependency audit
license checks
secret scanning
security tests
threat-review signoff for relevant changes
```

---

# 107. Migration Gate

Requires:

```text
fresh install
upgrade from supported prior versions
failure recovery
rollback classification
```

---

# 108. Conformance Gate

Official adapters must pass required certification profiles.

---

# 109. Performance Gate

Critical hot paths should not regress beyond defined budget.

Part 46 will define detailed benchmarking.

---

# 110. Documentation Gate

Release cannot be called stable if required:

```text
migration guide
config changes
breaking changes
release notes
```

are missing.

---

# 111. Packaging Gate

Install/uninstall/smoke tests on supported target packages.

---

# 112. Provenance Gate

Verify:

```text
manifest
hashes
signatures
SBOM
source commit
```

all match.

---

# 113. Release CI Pipeline

Concept:

```text
tag/release commit
↓
source verification
↓
locked build
↓
tests
↓
cross-platform matrix
↓
conformance
↓
package
↓
SBOM/provenance
↓
sign
↓
publish staging
↓
smoke verify
↓
promote channel
```

---

# 114. Never Sign Before Final Bytes

Artifact signing must occur after final packaging.

Changing the artifact after signing invalidates trust.

---

# 115. Immutable Promotion

Prefer:

```text
build once
promote same bytes
```

from RC to stable when possible.

Do not rebuild stable from source separately unless reproducibility is guaranteed and verified.

---

# 116. Artifact Repository

Store releases in immutable object storage/package registries.

Prevent overwrite of:

```text
v1.4.0
```

once published.

---

# 117. Retention

Keep:

```text
stable releases
security releases
debug symbols
manifests
signatures
SBOM
provenance
```

according to release retention policy.

---

# 118. Nightly Retention

Nightlies may have shorter retention.

---

# 119. Reproducibility Verification

A second independent build can compare:

```text
artifact digest
or
normalized artifact contents
```

for high-assurance releases.

---

# 120. Build Cache

Caches may accelerate CI but must not weaken reproducibility.

Cache keys include:

```text
toolchain
Cargo.lock
target
features
source digest
```

---

# 121. Cross Compilation

Cross compilation is useful, but platform-native signing/package steps may require native runners.

---

# 122. Linux Builds

Can often be built in Linux CI for multiple architectures.

---

# 123. Windows Builds

Use MSVC-native Windows runners for production Windows artifacts where required.

---

# 124. macOS/iOS Builds

Require macOS infrastructure for Apple signing and packaging.

---

# 125. Android Builds

Can build Rust native libraries with Android NDK toolchains on supported hosts.

Final app signing follows Android release tooling.

---

# 126. Platform Test Before Publish

Do not trust compile success alone.

Run smoke tests on real/virtual target environments.

---

# 127. Server Smoke Test

Example:

```text
start server
connect PostgreSQL
run migrations
health/ready
execute sample sync
shutdown cleanly
```

---

# 128. SQLite Client Smoke Test

```text
create store
Tx A
restart
sync/reconcile
Tx C
verify cursor
```

---

# 129. Stoolap Client Smoke Test

Same semantic smoke workflow as SQLite.

---

# 130. Desktop Smoke Test

```text
install
launch
open store
mutate
restart
upgrade
uninstall
```

---

# 131. Mobile Smoke Test

```text
install
first launch
bootstrap
offline mutation
background/resume
upgrade
process kill
```

---

# 132. Release Artifact Naming

Use deterministic names:

```text
aequora-cli-1.4.0-x86_64-unknown-linux-gnu.tar.zst
aequora-server-1.4.0-aarch64-unknown-linux-gnu.tar.zst
```

---

# 133. Checksums File

Publish:

```text
SHA256SUMS
BLAKE3SUMS
```

plus signatures.

---

# 134. Signature Verification CLI

Possible:

```text
aequora verify-release <manifest>
```

This can verify:

```text
manifest signature
artifact hash
registry digest
SBOM reference
```

---

# 135. Self-Update CLI

A CLI self-update command is optional.

If implemented:

```text
aequora update
```

must use signed update metadata and artifact verification.

---

# 136. Offline Installation

Support manually downloaded signed artifacts.

Air-gapped environments can verify using bundled/public trust roots.

---

# 137. Enterprise Mirror

Enterprise users may mirror:

```text
crates
container images
desktop packages
update metadata
```

inside private infrastructure.

---

# 138. Mirror Trust

Mirror may distribute bytes, but artifact signatures remain verifiable independently.

---

# 139. Update Service

A future update service can answer:

```text
latest allowed version for channel/platform
```

but must not be the sole trust root.

---

# 140. Update Decision

Client considers:

```text
current version
channel
platform
policy
minimum version
rollout cohort
```

---

# 141. Update Metadata Expiry

Signed metadata should have expiry/validity to reduce replay of obsolete release information.

---

# 142. Clock Issues

Expiry validation must handle reasonable clock skew without disabling signature verification.

---

# 143. Rollout Kill Switch

A bad release can be halted by update metadata/control plane.

Already-installed binaries still require compatibility/security policy handling.

---

# 144. Crash Reporting Version Identity

Every crash/incident report should include:

```text
release version
build ID
commit
target
feature set
registry digest
```

---

# 145. Reproducible Incident Mapping

Debug symbols and source commit enable:

```text
stack trace
↓
exact binary
↓
exact source
```

---

# 146. Platform Signing Key Security

Use secure signing infrastructure:

```text
HSM
managed signing service
protected CI key store
```

where available.

---

# 147. CI Permissions

Build jobs should not automatically possess production signing/publishing permissions.

Use staged jobs:

```text
build
verify
sign
publish
```

with least privilege.

---

# 148. Two-Person Release Approval

High-assurance/enterprise releases may require two-person approval before stable promotion.

---

# 149. Separation of Duties

Possible:

```text
developer merges
CI builds
release approver promotes
signing service signs
```

---

# 150. Release Audit Log

Record:

```text
release ID
source commit
builder
approver
signing identity
publication time
channel
artifact digests
```

---

# 151. Release ID

Concept:

```text
ReleaseId
```

can be globally unique independent of semantic version.

Useful for rebuilds/internal candidates.

---

# 152. Build vs Release

One source commit may produce multiple build candidates.

Only promoted, signed artifact sets become official releases.

---

# 153. Release Bundle

Logical bundle:

```text
release-manifest.ron
release-manifest.postcard
SHA256SUMS
BLAKE3SUMS
signatures/
sbom/
provenance/
migrations/
artifacts/
release-notes.md
```

---

# 154. Registry Snapshot

Include the exact registry artifact/digest associated with the release.

---

# 155. Config Schema Artifact

Include machine-readable configuration schema/reference version.

---

# 156. Protocol Fixtures

Stable releases may publish golden protocol fixtures for integrators.

---

# 157. Conformance Report

Official adapter certification/conformance summary can be attached.

---

# 158. Compatibility Matrix

Publish:

```text
client versions
server versions
protocol range
store formats
migration paths
```

---

# 159. Release Tooling

Potential CLI:

```text
aequora release verify
aequora release manifest
aequora release compatibility
```

Actual release creation may be a dedicated internal tool.

---

# 160. Avoid Release Logic in Shell Alone

Shell scripts can orchestrate, but core release manifest/signature/compatibility logic should live in typed Rust tooling where feasible.

---

# 161. Release Workspace Crate

Possible:

```text
aequora-release-tool
```

with types:

```text
ReleaseManifest
ArtifactDescriptor
SigningMetadata
CompatibilityMatrix
```

---

# 162. Rebuild Detection

If the same semantic version is rebuilt with different bytes:

```text
new BuildId
```

and it should not silently replace existing immutable release artifacts.

---

# 163. No Mutable Stable Tags Internally

Do not repoint:

```text
1.4.0
```

to different bytes.

---

# 164. Package Repository Metadata

APT/RPM/other repository metadata must be signed.

---

# 165. Windows Installer Upgrade Codes

Installer metadata should support clean upgrade paths and avoid accidental parallel incompatible installs.

---

# 166. macOS Bundle Versioning

Keep application semantic version and build number properly separated.

---

# 167. Android Version Codes

Product app must maintain monotonically increasing platform version codes independent of SemVer string.

---

# 168. iOS Build Numbers

Likewise maintain required monotonic build identifiers.

---

# 169. Cross-Platform Version Mapping

Release tooling can map:

```text
Aequora 1.4.0
↓
Windows package version
macOS CFBundleShortVersionString
Android versionName/versionCode
iOS marketing/build version
```

---

# 170. Desktop Auto-Update and DB Migration

Update order should be:

```text
download
verify
stage
close app
replace binary
start new version
migrate local store
verify
resume
```

Rollback decision must account for migration class.

---

# 171. Partial Update Failure

If GUI updated but agent did not:

```text
version handshake
```

must detect incompatibility and present recovery instructions.

---

# 172. Agent Protocol Version

Desktop agent IPC should negotiate its own stable compatibility version/capabilities.

---

# 173. Mobile Rust Core Upgrade

Native Rust library changes are shipped as part of full app releases.

There is no independent code self-update on mobile.

---

# 174. Server Runtime Compatibility

Horizontal server nodes should expose:

```text
BuildId
ReleaseVersion
ProtocolRange
RegistryDigest
```

in safe diagnostics.

---

# 175. Mixed Fleet Detection

Control plane can report:

```text
node version skew
```

during rollout.

---

# 176. Rollout Completion

A rollout is complete only when:

```text
all target nodes healthy
migration finalized
old version compatibility window handled
metrics stable
```

---

# 177. Release Freeze

Before GA/major release, a temporary freeze can restrict changes to:

```text
release blockers
security fixes
documentation
```

---

# 178. GA Criteria

Part 50 will define full GA, but packaging-related GA requires:

```text
signed artifacts
supported install paths
upgrade tests
rollback classification
SBOM
provenance
release notes
compatibility matrix
```

---

# 179. Release Invariants

## AEQ-INV-RELEASE001

```text
Every official release artifact is traceable to an immutable source revision, build configuration, target, and ReleaseManifest.
```

## AEQ-INV-RELEASE002

```text
Published release artifacts are immutable; an existing semantic version may never silently point to different bytes.
```

## AEQ-INV-RELEASE003

```text
Production release artifacts are integrity-checked and cryptographically signed through a purpose-specific signing identity.
```

## AEQ-INV-RELEASE004

```text
A binary upgrade cannot bypass required database, local-store, protocol, configuration, or registry compatibility checks.
```

## AEQ-INV-RELEASE005

```text
Client updates preserve pending operations, cursors, conflicts, store identity, and durable local intent across supported upgrade paths.
```

## AEQ-INV-RELEASE006

```text
Rollback is permitted only when the release's schema/store/protocol state is compatible with the target older version or an explicit rollback migration exists.
```

## AEQ-INV-RELEASE007

```text
The same exact artifact bytes are promoted across release channels whenever practical instead of rebuilding separately for stable promotion.
```

## AEQ-INV-RELEASE008

```text
Production signing and publishing credentials are isolated from ordinary build jobs and protected by least-privilege release workflows.
```

## AEQ-INV-RELEASE009

```text
A failed, cancelled, or revoked release cannot cause update clients to trust unsigned, hash-mismatched, expired, or incompatible artifacts.
```

## AEQ-INV-RELEASE010

```text
Release versioning never substitutes for ProtocolVersion, StoreFormatVersion, RegistryGeneration, ConfigSchemaVersion, or other semantic compatibility dimensions.
```

---

# 180. Recommended v1 Artifact Set

For an initial production Aequora release:

```text
aequora CLI:
  Linux x86_64
  Linux arm64
  Windows x86_64
  macOS arm64/x86_64 where supported

aequora-server:
  Linux x86_64
  Linux arm64
  OCI multi-arch container

aequora-agent:
  Linux
  Windows
  macOS

Rust crates:
  core/public crates
  PostgreSQL
  SQLite
  Stoolap
  Axum
  Dioxus

mobile:
  Android AAR/Rust native artifacts
  iOS XCFramework

release metadata:
  manifest
  checksums
  signatures
  SBOM
  provenance
  migration bundle
  release notes
```

Product-specific GUI/app packages sit on top of this.

---

# 181. Recommended Release Flow

```text
1. Merge approved changes
2. Freeze release commit
3. Verify registry/migrations
4. Run full test/conformance matrix
5. Build locked artifacts
6. Run target smoke tests
7. Generate SBOM/provenance
8. Generate ReleaseManifest
9. Sign final bytes and metadata
10. Publish RC
11. Canary/staged verification
12. Promote same artifacts to Stable
13. Monitor
14. Close release audit record
```

---

# 182. Completion Criteria

```text
[ ] release/version dimensions separated
[ ] ReleaseManifest defined
[ ] artifact descriptor defined
[ ] toolchain/Cargo.lock policy defined
[ ] reproducible-build direction defined
[ ] SBOM/provenance defined
[ ] signing/hash model defined
[ ] release channels defined
[ ] server/client compatibility defined
[ ] migration bundle compatibility defined
[ ] Linux packaging defined
[ ] Windows packaging/signing defined
[ ] macOS packaging/signing defined
[ ] Android/iOS packaging defined
[ ] container image policy defined
[ ] update metadata/signing defined
[ ] desktop update flow defined
[ ] rollback classes defined
[ ] canary/blue-green rollout defined
[ ] release CI gates defined
[ ] artifact immutability defined
[ ] incident/debug-symbol retention defined
[ ] enterprise/offline distribution defined
[ ] release invariants defined
```

---

# 183. Final Architecture

```text
                        Source + Tag
                            |
                            v
                    Locked Release Build
                            |
             +--------------+---------------+
             |              |               |
             v              v               v
          Tests        Conformance       Security
             |              |               |
             +--------------+---------------+
                            |
                            v
                         Package
                            |
             +--------------+--------------+
             |              |              |
             v              v              v
          SBOM         Provenance       Manifest
             |              |              |
             +--------------+--------------+
                            |
                            v
                      Sign Final Bytes
                            |
                            v
                   Immutable Artifact Store
                            |
             +--------------+--------------+
             |                             |
             v                             v
        RC / Canary                    Stable
             |                             |
             v                             v
      controlled rollout           production rollout
```

Client update trust path:

```text
Signed Update Metadata
        |
        v
Compatibility Check
        |
        v
Download Artifact
        |
        v
Verify Signature + Hash
        |
        v
Stage
        |
        v
Install / Migrate
        |
        v
Health Verify
```

---

# 184. Final Recommendation

Aequora should treat release engineering as part of the correctness architecture.

The user should be able to answer, for any running binary:

```text
What source produced this?
What dependencies were used?
Which protocol/store/config versions does it support?
Who signed it?
Has it been altered?
Which migration set belongs to it?
Can it safely upgrade or roll back?
```

If those questions cannot be answered, the artifact should not be considered production-grade.

> **Build once, verify deeply, sign final bytes, promote immutably, and never let packaging or update convenience bypass compatibility semantics.**

That gives Aequora a trustworthy path from source code to server, CLI, desktop, Android, iOS, containers, enterprise mirrors, and future distribution channels.

---

## Next

**Part 45 — Deployment Topologies, Single-Node, HA, Multi-Region, Edge, Enterprise, Air-Gapped, and Operational Environment Architecture**
