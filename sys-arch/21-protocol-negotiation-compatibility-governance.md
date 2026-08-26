# Aequora Sync — Part 21

# Protocol Negotiation, Compatibility Governance, and Evolution Architecture

## 1. Purpose

Aequora is intended to run for years across:

```text
mobile clients
desktop clients
servers
multiple database adapters
different release cadences
offline devices
regional deployments
enterprise installations
```

This means different components will inevitably run different software versions at the same time.

Examples:

```text
server v8
Android client v6
desktop client v7
old offline client v4
new snapshot schema v3
operation schema v9
crypto policy v2
```

A synchronization system cannot assume:

```text
all participants upgrade simultaneously
```

Aequora therefore needs a deliberate compatibility architecture.

The central rule is:

> **Protocol evolution must be explicit, capability-driven, downgrade-resistant, and compatible with rolling upgrades and long-offline clients.**

---

# 2. Goals

The compatibility subsystem should provide:

```text
protocol negotiation
schema negotiation
capability advertisement
rolling upgrades
safe deprecation
minimum client versions
server feature gates
downgrade prevention
compatibility validation
offline-client recovery
adapter capability matching
release governance
```

---

# 3. Non-Goals

Compatibility governance is not:

```text
automatic semantic migration of arbitrary business rules
support for every client forever
runtime reflection of unknown operation semantics
silent downgrade to weaker correctness
```

Some old clients must eventually be:

```text
upgrade-required
```

---

# 4. Separate Version Domains

Do not use one global integer like:

```text
version = 12
```

for everything.

Aequora needs independent version domains.

---

# 5. Core Version Types

At minimum:

```text
ProtocolVersion
OperationSchemaVersion
DomainSchemaVersion
SnapshotSchemaVersion
ProjectionSchemaVersion
ProfileVersion
HandlerVersion
CryptoPolicyVersion
AuditSchemaVersion
LocalStoreFormatVersion
GovernancePolicyVersion
```

Each evolves independently.

---

# 6. ProtocolVersion

Defines:

```text
wire envelope
message framing
endpoint semantics
transport-level fields
```

Example:

```rust
pub struct ProtocolVersion(u16);
```

---

# 7. OperationSchemaVersion

Defines the structure of a specific operation payload.

Example:

```text
SetStudentPhone v1
SetStudentPhone v2
```

---

# 8. DomainSchemaVersion

Defines canonical domain-state representation.

Useful for:

```text
snapshots
migrations
adapter certification
replay
```

---

# 9. SnapshotSchemaVersion

Defines:

```text
snapshot manifest/chunk structure
```

separate from domain entity versions.

---

# 10. ProjectionSchemaVersion

Defines:

```text
what clients see
which fields exist
projection encoding
```

Part 07 uses this heavily.

---

# 11. ProfileVersion

Part 11 consistency profile semantics.

Changing profile behavior can be more significant than wire format.

---

# 12. HandlerVersion

Part 12 deterministic domain behavior.

Same payload schema may behave differently under newer handler version.

---

# 13. VersionVector

Do not create a single giant semantic vector on every message.

Instead advertise structured capability sets during session/bootstrap negotiation.

---

# 14. CapabilitySet

Conceptually:

```rust
pub struct CapabilitySet {
    pub protocol_versions: Vec<ProtocolVersion>,
    pub codecs: CapabilityFlags,
    pub compression: CapabilityFlags,
    pub snapshot_versions: Vec<SnapshotSchemaVersion>,
    pub crypto_features: CapabilityFlags,
    pub transport_features: CapabilityFlags,
}
```

---

# 15. Capability IDs

Use stable numeric IDs or bitflags.

Avoid arbitrary strings on hot path.

---

# 16. Capability Categories

Recommended:

```text
Protocol
Codec
Compression
Snapshot
LiveTransport
Crypto
Scope
Replay
Bulk
Regional
```

---

# 17. Negotiation Handshake

Flow:

```text
client hello
↓
server policy + capabilities
↓
intersection
↓
server-selected effective session profile
↓
client accepts
```

---

# 18. ClientHello

Conceptually:

```rust
pub struct ClientHello {
    pub client_build: ClientBuildId,
    pub supported_protocols: Vec<ProtocolVersion>,
    pub capabilities: CapabilitySet,
    pub authority_hint: Option<AuthorityDescriptorHint>,
}
```

---

# 19. ServerHello

```rust
pub struct ServerHello {
    pub authority: AuthorityDescriptor,
    pub selected_protocol: ProtocolVersion,
    pub required_capabilities: CapabilityFlags,
    pub enabled_capabilities: CapabilityFlags,
    pub minimum_client_policy: ClientCompatibilityPolicy,
}
```

---

# 20. Server Chooses

Client advertises support.

Server chooses from allowed policy.

This avoids:

```text
client forcing deprecated/weak mode
```

---

# 21. Highest Common Version Is Not Always Correct

Server may intentionally select:

```text
stable v4
```

even if both support experimental v5.

Selection policy matters.

---

# 22. ProtocolPolicy

```rust
pub struct ProtocolPolicy {
    pub preferred: ProtocolVersion,
    pub supported: Vec<ProtocolVersion>,
    pub deprecated: Vec<ProtocolVersion>,
    pub forbidden: Vec<ProtocolVersion>,
}
```

---

# 23. Required Capability

Some deployment policies require:

```text
signed snapshot
scope-v2
specific encryption
```

If client lacks required capability:

```text
incompatible
```

Do not downgrade.

---

# 24. Compatibility Result

```rust
pub enum CompatibilityResult {
    Compatible(SessionProfile),
    UpgradeRecommended(CompatibilityWarning),
    UpgradeRequired(CompatibilityFailure),
}
```

---

# 25. SessionProfile

Resolved negotiated session:

```rust
pub struct SessionProfile {
    pub protocol: ProtocolVersion,
    pub codec: Codec,
    pub compression: CompressionMode,
    pub snapshot_schema: SnapshotSchemaVersion,
    pub capabilities: CapabilityFlags,
}
```

---

# 26. Persist Negotiated Session?

Session profile can be cached.

But client must renegotiate after:

```text
server restart/version change
authority epoch change
client upgrade
policy generation change
```

---

# 27. Stateless Negotiation

Prefer session negotiation that can be repeated cheaply.

Do not require long-lived server-side state.

---

# 28. Protocol Header

Every request should still identify:

```text
protocol version
```

even after negotiation.

This prevents ambiguous decode.

---

# 29. Major vs Minor

Aequora can conceptually distinguish:

```text
compatible additive change
breaking semantic change
```

but explicit numeric versions are clearer than relying on SemVer interpretation alone.

---

# 30. Additive Changes

Examples:

```text
new optional field
new response metadata
new capability bit
```

may remain within same protocol version if decoder safely ignores them.

---

# 31. Postcard Compatibility Challenge

Binary compact codecs do not automatically provide arbitrary forward compatibility.

Therefore protocol structs should be designed intentionally.

---

# 32. Stable Envelope + Versioned Payload

Recommended pattern:

```text
stable outer envelope
+
message kind
+
payload version
+
length-delimited payload
```

Unknown message payload can be skipped/rejected cleanly.

---

# 33. MessageKind

Use:

```rust
pub struct MessageKind(u16);
```

---

# 34. PayloadVersion

Each complex message kind may have:

```rust
PayloadVersion(u16)
```

when evolution demands.

---

# 35. Length Delimiting

This allows older decoders to:

```text
skip unsupported extension section
```

where policy permits.

---

# 36. Extension Sections

For low-frequency optional features:

```text
extension ID
length
payload
```

can avoid exploding core message versions.

Use carefully.

---

# 37. Avoid Generic TLV Everywhere

A fully generic TLV protocol becomes hard to type-check.

Use typed versioned messages for core semantics.

---

# 38. Unknown Required Extension

If extension marked required and unknown:

```text
reject as incompatible
```

---

# 39. Unknown Optional Extension

May be ignored if explicit semantics say safe.

---

# 40. Operation Versioning

Each `OperationKind` has supported schema versions.

Registry entry:

```rust
OperationDescriptor {
    kind,
    min_schema,
    max_schema,
    current_schema,
}
```

---

# 41. Upcasting

Server can support old operation payloads via:

```text
v1 -> v2 -> v3
```

upcasters.

---

# 42. Upcaster Rule

Upcasting must be:

```text
deterministic
pure
side-effect free
```

---

# 43. No Downcast for Authority

Server should not downcast new semantics into old operation payload to execute.

Authority executes canonical/current semantic form.

---

# 44. Response Projection Compatibility

Server may project authoritative state into older client projection schema.

Only while semantics remain safe.

---

# 45. Projection Downgrade

Example:

```text
new server field optional
old client simply does not see it
```

safe.

---

# 46. Unsafe Projection Downgrade

If old client cannot understand:

```text
new permission state
new required workflow state
new conflict semantics
```

it may become unsafe.

Then:

```text
upgrade required
```

---

# 47. Write Compatibility Is Stricter Than Read Compatibility

An old client may safely read data but not safely create new operations.

Support:

```text
ReadOnlyCompatible
```

mode.

---

# 48. CompatibilityMode

```rust
pub enum CompatibilityMode {
    Full,
    ReadOnly,
    BootstrapOnly,
    UpgradeRequired,
}
```

---

# 49. ReadOnly Mode

Useful for old clients after schema evolution.

Client can:

```text
view cached/server data
export
```

but cannot submit unsupported mutations.

---

# 50. BootstrapOnly

Client can:

```text
download migration/upgrade bootstrap metadata
```

but not normal sync.

---

# 51. Minimum Client Policy

Server can enforce:

```rust
pub struct ClientCompatibilityPolicy {
    pub minimum_build: Option<ClientBuildConstraint>,
    pub minimum_protocol: ProtocolVersion,
    pub policy_generation: CompatibilityPolicyGeneration,
}
```

---

# 52. Build IDs

Do not rely only on human app version string.

Use:

```rust
pub struct ClientBuildId {
    pub platform: PlatformId,
    pub version: SemanticBuildVersion,
    pub build_number: u64,
}
```

---

# 53. Platform-Specific Minimum

Android may need minimum build different from desktop.

---

# 54. Emergency Block

Security issue may require:

```text
block client build <= X
```

immediately.

---

# 55. CompatibilityPolicyGeneration

```rust
pub struct CompatibilityPolicyGeneration(u64);
```

Monotonic.

Clients can detect changed requirements.

---

# 56. Policy Signed Metadata

High-assurance mode may sign compatibility policy with Part 15 server key.

---

# 57. Offline Clients

A client offline for months may return with:

```text
old protocol
old cursor
old projection
old local schema
```

Compatibility evaluation order:

```text
1. authority ID/epoch
2. client build support
3. protocol compatibility
4. local store migration
5. scope/projection compatibility
6. cursor retention
7. operation schema compatibility
```

---

# 58. Do Not Mix All Failures Into "Upgrade Required"

Return precise cause:

```text
AuthorityChanged
ProtocolUnsupported
ClientBuildBlocked
ProjectionUpgradeRequired
CursorExpired
StoreMigrationRequired
```

---

# 59. Local Store Migration

Part 20 `LocalStoreFormatVersion`.

Client app upgrades local DB before sync when required.

---

# 60. Store Downgrade Protection

Older app opening newer store should fail safely.

---

# 61. Pending Operation Migration

Critical:

Old local outbox may contain:

```text
operation schema v2
```

new app/server uses v4.

Need:

```text
pending-operation upcaster
```

or preserve old supported wire form.

---

# 62. Unsent Operations

Can be migrated locally if:

```text
never sent
semantic equivalence proven
```

---

# 63. Possibly-Sent Operations

Part 04 immutability applies.

Do not mutate same OperationId payload.

Instead retain original schema bytes and let server support old schema until retry horizon expires.

---

# 64. Compatibility Retention Horizon

Server must support operation schema versions for at least:

```text
maximum supported offline/retry window
```

or explicitly force recovery.

---

# 65. Operation Deprecation

Lifecycle:

```text
Current
↓
Supported
↓
Deprecated
↓
ReadOnly/RetryOnly
↓
Removed
```

---

# 66. RetryOnly

Important state.

Old operation schema may no longer be creatable by clients, but server still accepts same historical OperationId retries.

---

# 67. New Creation Ban

Server can reject:

```text
new op using deprecated schema
```

while accepting:

```text
old retry
```

distinguished by operation ledger/first-seen policy.

---

# 68. Semantic Payload Hash

Operation ledger helps verify:

```text
same OperationId
same payload
```

---

# 69. Schema Removal

Remove old schema only when:

```text
retry horizon expired
supported clients migrated
replay/audit retention requirements understood
```

---

# 70. Handler Version Compatibility

Historical replay may need old handlers after clients stop using old schema.

Do not conflate transport support with replay support.

---

# 71. Snapshot Compatibility

Client advertises supported snapshot schema versions.

Server chooses compatible published snapshot.

---

# 72. Snapshot Profile

Part 10 may have:

```text
Standard v3
LowMemory v2
```

Negotiation chooses one valid combination.

---

# 73. Snapshot Upgrade

If old snapshot schema no longer supported:

```text
client app upgrade required
```

or server builds older compatibility snapshot temporarily.

---

# 74. Projection Schema

Part 07 scope descriptor includes:

```text
ProjectionSchemaVersion
```

---

# 75. Projection Generation

Incompatible projection change may require:

```text
ScopeGeneration bump
rebootstrap
```

even if protocol unchanged.

---

# 76. Domain Schema Migration

Server DB schema may change internally without protocol change if canonical semantics remain.

Aequora compatibility concerns canonical contracts, not SQL layout.

---

# 77. Adapter Compatibility

Each adapter declares:

```text
Aequora adapter API version
capability set
store format versions
```

---

# 78. AdapterApiVersion

```rust
pub struct AdapterApiVersion(u16);
```

---

# 79. Adapter Capability Manifest

Example:

```text
transactions
CAS
snapshot install
governance purge
integrity scan
```

---

# 80. Startup Certification

Core validates required capabilities for registered profiles.

---

# 81. Plugin/Crate SemVer

Public Rust crates should follow SemVer.

But runtime protocol compatibility remains separately governed.

---

# 82. Workspace Release

A monorepo may release many crates together.

Do not assume crate version equals protocol version.

---

# 83. Protocol Registry

Maintain canonical registry file.

Example:

```ron
(
    protocols: [
        (
            version: 4,
            status: Current,
            introduced: "0.8.0",
        ),
        (
            version: 3,
            status: Deprecated,
        ),
    ],
)
```

---

# 84. Operation Registry

Machine-readable registry:

```text
OperationKind
schema versions
status
profile
minimum protocol
```

---

# 85. Capability Registry

Stable IDs must never be reused with new semantics.

---

# 86. Reserved IDs

Removed IDs remain reserved forever.

---

# 87. Compatibility Matrix

Generate:

```text
server version
client version
protocol
read
write
bootstrap
notes
```

---

# 88. CI Matrix

Test at least:

```text
current server + current client
current server + previous client
previous server + current client during rolling deploy
current server + oldest supported client
```

---

# 89. Rolling Server Upgrade

Typical sequence:

```text
old server fleet
↓
deploy mixed old/new fleet
↓
new DB migrations compatible with old code
↓
enable new code
↓
retire old nodes
↓
activate new protocol feature
```

---

# 90. Expand/Contract Database Migration

Use:

```text
expand
migrate/backfill
switch
contract
```

Avoid schema change that immediately breaks old server nodes.

---

# 91. Feature Activation After Rollout

New capability should not be used immediately when first new node appears.

Need:

```text
fleet supports feature
↓
feature gate enabled
```

---

# 92. ServerFeatureGate

```rust
pub struct ServerFeatureGate {
    pub capability: CapabilityId,
    pub state: FeatureState,
}
```

---

# 93. FeatureState

```rust
pub enum FeatureState {
    Disabled,
    Shadow,
    EnabledForCanary,
    Enabled,
    Required,
}
```

---

# 94. Shadow

Server parses/observes capability without changing behavior.

Useful for rollout.

---

# 95. Canary

Enable for selected tenants/devices.

---

# 96. Required

Once required, incompatible clients are rejected/upgraded.

---

# 97. Client Feature Gate

Client can also ship code disabled until server supports feature.

---

# 98. Capability Rollout

Sequence:

```text
ship client support
ship server support
observe compatibility
enable optional feature
later make required
```

This avoids synchronized releases.

---

# 99. Forward Server Compatibility

New client connecting to older server during rolling deploy must not assume new feature exists.

Capability negotiation handles it.

---

# 100. Backward Client Compatibility

New server accepts older supported client.

---

# 101. Downgrade Attack

An attacker may try to force:

```text
weaker protocol
weaker crypto
```

---

# 102. Downgrade Prevention

Server policy defines:

```text
minimum allowed
required capability
```

and authenticated session chooses accordingly.

---

# 103. Client Minimum Security Policy

Client can also refuse:

```text
protocol < N
crypto feature missing
```

for high-assurance deployment.

---

# 104. Negotiation Transcript

High-assurance mode may hash/sign:

```text
client offered
server selected
policy generation
```

for diagnostics/anti-downgrade evidence.

Usually TLS + server policy is sufficient.

---

# 105. Crypto Compatibility

Part 15 algorithm migration needs:

```text
supported algorithms
allowed policy
```

---

# 106. No Opportunistic Weak Crypto

If strong crypto required and no common algorithm:

```text
fail
```

---

# 107. Compression Compatibility

Safe to negotiate more flexibly.

Example:

```text
zstd
none
```

---

# 108. Codec Compatibility

Postcard primary.

JSON only explicit interoperability endpoint.

Do not silently switch core sync to JSON because client lacks Postcard.

---

# 109. Transport Compatibility

HTTPS baseline.

Future:

```text
QUIC
```

can be optional capability.

Semantics remain same.

---

# 110. Live Transport

WebSocket/SSE/mobile push are optional.

No live capability:

```text
polling still correct
```

---

# 111. Graceful Capability Absence

Optional capabilities should have defined fallback.

Examples:

```text
no live → polling
no zstd → uncompressed
no range resume → chunk restart
```

---

# 112. No Fallback for Required Semantics

Examples:

```text
required device signature
required signed snapshot
required scope isolation
```

cannot downgrade.

---

# 113. Compatibility Categories

Classify each capability:

```rust
pub enum CapabilityRequirementKind {
    OptionalWithFallback,
    OptionalOptimization,
    RequiredForSafety,
    RequiredForSemantics,
}
```

---

# 114. OptionalOptimization

Example:

```text
zstd
regional snapshot CDN
```

---

# 115. RequiredForSafety

Example:

```text
signed artifact verification under tenant policy
```

---

# 116. RequiredForSemantics

Example:

```text
new operation profile behavior
```

---

# 117. Deprecation Policy

Every protocol/schema capability should have lifecycle.

```rust
pub enum SupportStatus {
    Experimental,
    Current,
    Supported,
    Deprecated,
    RetryOnly,
    Removed,
}
```

---

# 118. Experimental

No compatibility promise.

Do not enable for production data unless explicitly accepted.

---

# 119. Current

Preferred.

---

# 120. Supported

Fully supported older version.

---

# 121. Deprecated

Works but warns.

---

# 122. RetryOnly

Historical retry/replay compatibility only.

---

# 123. Removed

Rejected.

ID remains reserved.

---

# 124. Deprecation Window

Define in releases/time based on product support policy.

Do not hardcode global duration in core.

---

# 125. Upgrade Notification

Server can return:

```text
UpgradeRecommended
```

before enforcing.

---

# 126. Upgrade Required

Response includes stable reason code, not marketing string.

---

# 127. Client UX

Possible:

```text
A newer version is required to continue syncing.
Your local data is preserved.
```

---

# 128. Preserve Local Data

Upgrade-required state must not wipe outbox.

---

# 129. Read-Only Grace

Product may allow:

```text
local access
export
```

while sync blocked.

---

# 130. Security Emergency

If old client has severe vulnerability:

```text
block immediately
```

even if usual deprecation window not elapsed.

---

# 131. Emergency Policy Distribution

Compatibility policy should be reloadable without server binary release.

Signed/controlled config.

---

# 132. Config Reload

Server can atomically swap compatibility policy snapshot.

---

# 133. Policy Validation

Reject invalid config like:

```text
preferred protocol not supported
required capability unavailable on fleet
```

---

# 134. Fleet Capability Awareness

Before making feature `Required`, control plane should know all serving nodes support it.

---

# 135. NodeCapabilities

Each server node reports:

```text
binary build
protocols
capabilities
```

---

# 136. Mixed Fleet

Router/admission should not send sessions requiring capability to node that lacks it.

Simplest:

```text
do not require new capability until old nodes drained
```

---

# 137. Canary Tenants

Feature gate by:

```text
tenant
client cohort
region
```

with deterministic assignment.

---

# 138. Server-Side Gate Only

Never trust client to self-select safety-sensitive canary behavior.

---

# 139. Compatibility Telemetry

Track:

```text
client build distribution
protocol distribution
deprecated usage
upgrade-required attempts
```

---

# 140. Privacy

Use coarse build/platform aggregates.

Avoid device IDs in metrics.

---

# 141. Compatibility Dashboard

Operators should see:

```text
% current
% supported old
% deprecated
% blocked
```

---

# 142. Removal Readiness

Before removing old version:

```text
usage near zero
retry horizon passed
offline-device policy satisfied
```

---

# 143. Offline Device Policy

Part 14 may retire very old inactive devices.

This enables old protocol removal.

---

# 144. Device Rebootstrap

An ancient client may require:

```text
app upgrade
store migration
fresh bootstrap
```

rather than incremental compatibility.

---

# 145. Compatibility Recovery Plan

Server can return:

```text
UpgradeApp
MigrateLocalStore
RebootstrapScope
```

as structured steps.

---

# 146. RecoveryInstruction

```rust
pub enum RecoveryInstruction {
    UpgradeClient,
    MigrateStore,
    Rebootstrap,
    Reauthenticate,
    ContactAdministrator,
}
```

---

# 147. Protocol Error Taxonomy

```text
UnsupportedProtocol
UnsupportedCapability
RequiredCapabilityMissing
DeprecatedVersion
ClientBuildBlocked
ProjectionIncompatible
SnapshotSchemaUnsupported
OperationSchemaUnsupported
```

---

# 148. Avoid Generic "VersionMismatch"

Precise errors improve safe recovery.

---

# 149. Compatibility and Part 16

Authority epoch changes are not protocol upgrades.

Keep separate.

---

# 150. Compatibility and Part 11

Profile semantic changes can be breaking even when wire unchanged.

Require:

```text
profile version
```

governance.

---

# 151. Compatibility and Part 12

Handler semantic changes are release-governed and replay-tested.

---

# 152. Compatibility and Part 15

Crypto policy can require capability independently of protocol.

---

# 153. Compatibility and Part 17

Regional nodes must advertise same required protocol policy before global activation.

---

# 154. Compatibility and Part 18

Overload responses must remain understood by supported older clients.

---

# 155. Compatibility and Part 20

Low-resource clients may support fewer optional capabilities.

Do not classify low-memory profile as incompatible unless required feature missing.

---

# 156. Serialization Golden Tests

Maintain golden Postcard byte fixtures for critical protocol messages.

---

# 157. Decode Old Fixtures

Current server/client code must decode every still-supported old fixture.

---

# 158. Encode Stability

If same protocol version promises byte-level stability:

```text
golden encode must not change
```

Otherwise increment payload/protocol version.

---

# 159. Fuzz Cross-Version

Fuzz current decoder with:

```text
old version
unknown extension
truncated message
invalid version
```

---

# 160. Upcaster Tests

For each old operation fixture:

```text
v1 -> canonical current
```

expected deterministic result.

---

# 161. Downgrade Tests

Client offers:

```text
v4, v5
```

attacker strips v5 conceptually.

Server policy requiring v5:

```text
must reject v4
```

---

# 162. Rolling Upgrade Test

Run:

```text
old server node
new server node
old client
new client
```

through load balancer.

Ensure negotiated features remain safe.

---

# 163. RetryOnly Test

Old operation payload with known old OperationId:

```text
accepted/deduplicated
```

New operation using deprecated schema:

```text
rejected
```

---

# 164. ReadOnly Compatibility Test

Old client can read but mutation attempt returns:

```text
UpgradeRequiredForWrite
```

without corrupting local outbox.

---

# 165. Offline Return Test

Client offline beyond protocol support.

Expected:

```text
structured upgrade/rebootstrap path
```

---

# 166. Compatibility Invariants

Add:

## AEQ-INV-COMP001

```text
A server never decodes a message without an explicit protocol/message version context.
```

## AEQ-INV-COMP002

```text
A required safety or semantic capability is never silently downgraded to an optional weaker mode.
```

## AEQ-INV-COMP003

```text
Possibly-sent operations retain immutable schema/payload semantics across client upgrades.
```

## AEQ-INV-COMP004

```text
Removed protocol and capability IDs are never reused with different semantics.
```

## AEQ-INV-COMP005

```text
Rolling upgrades do not activate a capability until all serving paths required for that capability support it.
```

## AEQ-INV-COMP006

```text
Upgrade-required states preserve durable local user intent.
```

---

# 167. Additional Invariants

## AEQ-INV-COMP007

```text
Operation schema support outlives the legitimate retry horizon or provides an explicit recovery path.
```

## AEQ-INV-COMP008

```text
Protocol version changes are independent from authority epoch changes.
```

## AEQ-INV-COMP009

```text
Compatibility policy changes are versioned, auditable, and fail closed when required capabilities are missing.
```

---

# 168. Registry Layout

Suggested crate:

```text
aequora-compat/
├── protocol.rs
├── capability.rs
├── negotiation.rs
├── policy.rs
├── registry.rs
├── operation.rs
├── snapshot.rs
├── projection.rs
├── deprecation.rs
└── errors.rs
```

---

# 169. Build-Time Codegen

Optional:

```text
registry RON
↓
build.rs/codegen
↓
typed constants
```

This prevents manual ID drift.

---

# 170. Registry Source of Truth

Keep one reviewed registry file in repository.

---

# 171. CI Registry Checks

Fail if:

```text
ID duplicated
removed ID reused
version decreases
required metadata missing
```

---

# 172. Compatibility Report CLI

```text
aequora compat show
aequora compat check-client
aequora compat matrix
aequora compat deprecated
aequora compat registry verify
```

---

# 173. Release Checklist

Before release:

```text
protocol changes reviewed
golden fixtures updated
old client tests pass
upcasters tested
feature gates default safe
minimum client policy unchanged or documented
```

---

# 174. Breaking Change Checklist

If breaking:

```text
new version allocated
migration/upcaster designed
deprecation plan
offline-client path
rollback plan
```

---

# 175. Rollback

New server release should be rollback-compatible where possible.

That means database/schema changes follow expand/contract.

---

# 176. Feature Gate Rollback

If new feature causes issue:

```text
disable feature gate
```

without reverting binary where possible.

---

# 177. Protocol Rollback

Do not emit new protocol version broadly until rollback strategy understood.

---

# 178. Client Rollout

Mobile stores can take days to propagate updates.

Therefore server must tolerate mixed client versions.

---

# 179. Desktop Self-Update

Could be faster, but do not assume instant.

---

# 180. Enterprise Managed Client

Some organizations upgrade slowly.

Support window is product policy.

---

# 181. Compatibility Contract

Public documentation should say:

```text
supported client versions
supported protocol versions
deprecation policy
```

---

# 182. Library Users

Aequora as library should expose stable APIs and migration notes.

Runtime compatibility docs separate from Rust API compatibility.

---

# 183. Protocol Documentation

Generate from registry:

```text
message kinds
versions
capabilities
status
introduced/deprecated
```

---

# 184. Operation Documentation

Generate:

```text
operation kind
schema versions
consistency profile
handler version
support status
```

---

# 185. Snapshot Documentation

Generate:

```text
snapshot schema
chunk format
compression
crypto requirements
```

---

# 186. Compatibility Governance Roles

In larger organization:

```text
protocol owner
domain owner
security reviewer
release owner
```

approve relevant changes.

---

# 187. Change Proposal

Any breaking compatibility change should require a design/change proposal.

---

# 188. Stable-ID Governance

IDs are effectively protocol ABI.

Review them like database migration IDs.

---

# 189. Experimental Namespace

Reserve ID ranges for:

```text
experimental
vendor-private
```

if ecosystem needs it.

Do not allow experimental IDs into durable production data without migration plan.

---

# 190. Third-Party Extensions

Future ecosystem may register custom capabilities/operation kinds.

Part 29 will formalize registry governance.

---

# 191. Vendor Range

Potential:

```text
0x0000_0000–0x7FFF_FFFF core
0x8000_0000–0xEFFF_FFFF registered vendor
0xF000_0000–0xFFFF_FFFF experimental/private
```

Exact ranges can be decided later.

---

# 192. Capability Explosion

Avoid hundreds of tiny capabilities.

A capability should represent a meaningful interoperability behavior.

---

# 193. Version Explosion

Similarly, do not bump protocol for every internal code change.

Only externally observable contract changes matter.

---

# 194. Semantic Compatibility

A message may decode successfully but still be semantically incompatible.

Example:

```text
old client treats state X as impossible
```

Compatibility tests must include domain semantics.

---

# 195. Semantic Compatibility Review

For each new enum variant/state:

```text
what does old client do?
```

If unsafe:

```text
new projection/protocol version
or upgrade requirement
```

---

# 196. Unknown Enum Variant

Wire design should not map unknown enum to arbitrary default.

Use:

```text
Unknown(code)
```

where safe, otherwise reject.

---

# 197. Safety-Critical Enum

If unknown state affects authorization/workflow:

```text
reject/upgrade
```

---

# 198. Optional Display Enum

Could render:

```text
Unknown
```

safely.

---

# 199. Field Removal

Do not remove field while old clients rely on it.

Deprecate first.

---

# 200. Field Rename

Wire field ID stays same; human/Rust name may change.

---

# 201. Numeric Semantics

Changing units:

```text
cents -> rupees
seconds -> milliseconds
```

is breaking even if integer type unchanged.

Version explicitly.

---

# 202. Time Semantics

Changing timezone interpretation is semantic breaking change.

---

# 203. Conflict Semantics

Changing from:

```text
RejectStale
```

to:

```text
LWW
```

requires profile version/migration, not silent server upgrade.

---

# 204. Audit Semantics

Changing required audit policy may be governance change.

---

# 205. Crypto Semantics

Changing signature coverage requires artifact format/version change.

---

# 206. Compatibility State Machine

Client runtime:

```text
Unknown
↓
Negotiating
↓
Compatible
   ├── Full
   ├── ReadOnly
   └── UpgradeRecommended
↓
UpgradeRequired / Incompatible
```

---

# 207. Negotiation Failure

Should not corrupt existing local store.

---

# 208. Offline Use While Incompatible

Product may allow local read-only access.

Do not submit unsupported operations.

---

# 209. Pending New Operations

If app knows server incompatible with a new operation type:

```text
do not enqueue as normal sendable op
```

or mark:

```text
BlockedUntilServerUpgrade
```

---

# 210. Server Upgrade Dependency

Useful in enterprise self-hosted where clients may update first.

---

# 211. OperationCapability

Each operation descriptor can specify:

```text
minimum server capability
```

---

# 212. Client UI

Feature can be hidden/disabled until capability available.

---

# 213. Capability Cache

Refresh periodically or after server upgrade signal.

---

# 214. Server Upgrade Notice

Part 08 live notice can say:

```text
compatibility policy changed
```

Client renegotiates.

---

# 215. No Reliance on Live Notice

Next normal sync also detects.

---

# 216. Server Version Disclosure

Do not need to expose full internal build details publicly.

Expose interoperability metadata only.

---

# 217. Debug Metadata

Authenticated diagnostics can include build IDs.

---

# 218. Metrics

```text
compat_negotiation_total
compat_upgrade_required_total
compat_deprecated_protocol_total
compat_readonly_sessions
compat_operation_upcast_total
```

---

# 219. Logs

Structured:

```text
protocol_selected
client_deprecated
client_blocked
capability_required_missing
operation_upcasted
compat_policy_changed
```

---

# 220. Alerting

Alert on:

```text
large spike in incompatible clients
unexpected deprecated protocol usage
new feature activated before fleet ready
upcaster failures
```

---

# 221. Dashboard

Useful:

```text
client build distribution
protocol distribution
capability adoption
deprecated usage trend
```

---

# 222. Security

Negotiation input is untrusted.

Bound:

```text
number of versions
number of capabilities
metadata sizes
```

---

# 223. Capability Bomb

Client cannot send millions of capability IDs.

Hard limits.

---

# 224. Parser Fuzzing

Fuzz:

```text
ClientHello
ServerHello
extension sections
old protocol frames
```

---

# 225. Golden Compatibility Corpus

Store:

```text
old client hello
old op payload
old snapshot manifest
old conflict response
```

---

# 226. Long-Term Support

Enterprise deployments may designate:

```text
LTS protocol/client lines
```

This is product policy, not core necessity.

---

# 227. LTS Benefit

Reduces upgrade pressure for schools/enterprises.

---

# 228. LTS Cost

More:

```text
testing
upcasters
security maintenance
```

Only offer if business justifies.

---

# 229. Compatibility Budget

Track how many old versions are supported.

Avoid indefinite accumulation.

---

# 230. Sunset Process

Before removal:

```text
announce
telemetry
warn
block new creation
retry-only period
remove
```

---

# 231. Completion Criteria

Part 21 is complete when:

```text
[ ] version domains separated
[ ] capability registry defined
[ ] client/server negotiation defined
[ ] server-selected session profile defined
[ ] required-vs-optional capability semantics defined
[ ] protocol framing/version rules defined
[ ] operation upcasting defined
[ ] retry-only schema support defined
[ ] read-only compatibility mode defined
[ ] minimum client policy defined
[ ] rolling upgrade feature gates defined
[ ] downgrade prevention defined
[ ] local store/pending-op migration rules defined
[ ] compatibility CI matrix defined
[ ] golden fixtures/upcaster tests defined
[ ] deprecation lifecycle defined
[ ] compatibility invariants added
```

---

# 232. Final Architecture

```text
                    CLIENT START / SYNC
                           │
                           ▼
                       ClientHello
             protocols + capabilities + build
                           │
                           ▼
                    SERVER POLICY
          supported / required / deprecated / blocked
                           │
                           ▼
                      Negotiation
                           │
             ┌─────────────┼─────────────┐
             ▼             ▼             ▼
           Full         ReadOnly      Incompatible
             │             │             │
             ▼             ▼             ▼
       SessionProfile   Safe access   Upgrade/Recovery
             │
             ▼
      Versioned Wire Envelope
             │
             ▼
     Operation Schema Upcasters
             │
             ▼
      Canonical Current Semantics
             │
             ▼
        Authoritative Execution

Rolling rollout:

ship support
    ↓
mixed fleet
    ↓
shadow/canary
    ↓
enable optional
    ↓
make required
    ↓
deprecate old
    ↓
retry-only
    ↓
remove
```

The architectural principle is:

> **Aequora should evolve by negotiating explicit capabilities and preserving semantic compatibility—not by assuming every client, server, adapter, and stored operation changes at the same moment.**

This enables long-lived offline clients, rolling server upgrades, database migrations, crypto upgrades, new snapshot formats, and future transports to coexist safely without turning version evolution into a source of silent synchronization corruption.
