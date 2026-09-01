# Aequora Sync — Part 49

# Licensing, Dependency Policy, Supply-Chain Security, Crate Governance, SBOM, Reproducible Builds, and Third-Party Risk Architecture

## 1. Purpose

Aequora can have excellent synchronization semantics and still become unshippable or unsafe because of:

```text
an incompatible dependency license
an abandoned critical crate
a compromised package release
a malicious build script
an unnecessary native dependency
an unreviewed proc macro
a vulnerable transitive dependency
an unverifiable release artifact
a dependency update that changes durable behavior
```

Therefore dependency and supply-chain governance are architectural concerns.

The central rule is:

> **Every third-party dependency expands Aequora's legal, security, maintenance, reproducibility, and operational trust boundary.**

Dependencies should be selected deliberately rather than accumulated opportunistically.

---

# 2. Goals

Aequora's dependency architecture should optimize for:

```text
legal clarity
security
maintainability
portability
small trusted computing base
reproducible releases
long-term availability
upgradeability
auditable provenance
```

---

# 3. Non-Goals

Aequora should not pursue:

```text
zero dependencies at any cost
rewriting mature cryptography
rewriting TLS
rewriting PostgreSQL drivers
building a custom compression format
```

The goal is not dependency elimination.

The goal is:

> **minimal justified dependency.**

---

# 4. Dependency Classes

Classify dependencies:

```text
Core Semantic
Security Critical
Storage
Networking
Serialization
Platform
UI Integration
Build-Time
Developer Tooling
Testing
Optional Integration
```

Different classes receive different review depth.

---

# 5. Risk Tiers

Recommended:

```text
Tier 0 — trivial/dev-only
Tier 1 — ordinary runtime
Tier 2 — correctness-critical
Tier 3 — security/authority-critical
```

Examples:

```text
test assertion helper       Tier 0
small formatting crate      Tier 1
SQLx/storage adapter        Tier 2
TLS/crypto/auth dependency  Tier 3
```

---

# 6. Dependency Manifest

Maintain machine-readable governance metadata.

Example conceptual RON:

```ron
(
    crate: "example",
    role: Storage,
    risk: CorrectnessCritical,
    license_policy: PermissiveRequired,
    native_code: false,
    build_script: false,
    proc_macro: false,
    owner: "storage",
)
```

This may later be generated partly from Cargo metadata.

---

# 7. License Policy

Aequora should define an explicit allowed-license policy before product release.

Preferred dependency licenses generally include permissive families such as:

```text
MIT
Apache-2.0
BSD-2-Clause
BSD-3-Clause
ISC
Zlib
Unicode-style permissive licenses
```

Exact allow/deny policy should be reviewed legally for the distribution model.

---

# 8. Dual Licensing

A dependency declared:

```text
MIT OR Apache-2.0
```

is generally operationally convenient because the distributor may comply under an allowed option.

The actual SPDX expression must be evaluated rather than guessed from README text.

---

# 9. Copyleft Dependencies

Copyleft is not automatically "bad."

But it can materially affect:

```text
distribution obligations
linking
source availability
commercial licensing
plugin boundaries
SaaS/network use
```

Therefore it requires deliberate legal/product review.

---

# 10. Strong Copyleft

Licenses such as GPL-family licenses may be incompatible with a proprietary or differently licensed distribution strategy depending on how software is combined/distributed.

Do not add such dependencies casually.

---

# 11. Network Copyleft

AGPL-family obligations can be especially relevant to server software offered over a network.

If Aequora or a product using it deliberately chooses AGPL, that is a product licensing decision—not an accidental transitive dependency outcome.

---

# 12. Source-Available Licenses

"Source available" does not necessarily mean:

```text
open source
permissive
commercially unrestricted
```

Review terms explicitly.

---

# 13. Business Source / Restrictive Licenses

Dependencies restricting:

```text
commercial use
competitive products
hosting
redistribution
field of use
```

should be treated as high legal risk unless intentionally accepted.

---

# 14. Unknown License

Default:

```text
unknown license -> fail dependency policy
```

until reviewed.

---

# 15. License Files

Release packaging should retain required:

```text
LICENSE
NOTICE
copyright attribution
third-party notices
```

according to each dependency's obligations.

---

# 16. Third-Party Notices

Generate:

```text
THIRD_PARTY_LICENSES.txt
THIRD_PARTY_NOTICES.md
```

from the resolved release dependency graph, then review.

---

# 17. Aequora's Own License

The Aequora project license should be decided independently from dependency licenses.

Possible strategies include:

```text
Apache-2.0
MIT OR Apache-2.0
AGPL-3.0-or-later
commercial dual licensing
```

Architecture should remain usable under whichever deliberate project model is selected.

---

# 18. Product vs Library Licensing

The reusable Aequora library and products embedding it may have different licensing strategies if legally structured accordingly.

Do not let crate boundaries imply legal conclusions without actual licensing review.

---

# 19. Dependency Admission

Before adding a runtime dependency ask:

```text
What capability does it provide?
Can an existing dependency provide it?
Is it maintained?
What is its license?
Does it use unsafe?
Does it have a build.rs?
Does it compile native code?
Does it pull a large transitive graph?
Does it work on all required targets?
Is it correctness/security critical?
Can it be replaced behind an adapter?
```

---

# 20. Dependency Budget

Every crate should have a dependency budget mindset.

Avoid:

```text
adding a large framework
```

for one tiny utility.

---

# 21. Duplicate Capability

Prefer one established implementation for a capability unless multiple implementations are architecturally intentional.

Bad accidental duplication:

```text
three HTTP clients
four UUID libraries
multiple TLS stacks
```

---

# 22. Dependency Graph Layers

The Cargo graph should preserve Part 34's architecture:

```text
Foundation
↓
Protocol
↓
Domain
↓
Storage Contracts
↓
Sync Core
↓
Adapters
↓
Integrations
↓
Applications
```

Dependency governance should reject architectural back-edges.

---

# 23. Core Dependency Standard

Foundation/protocol/domain crates should have especially small dependency graphs.

They are reused everywhere and form the long-term semantic base.

---

# 24. Adapter Isolation

Database-specific dependencies belong only in adapter crates.

Example:

```text
sqlx -> aequora-postgres
SQLite driver -> aequora-sqlite
Stoolap -> aequora-stoolap
```

Never leak them into `aequora-core`.

---

# 25. UI Isolation

Dioxus belongs in:

```text
aequora-dioxus
```

not protocol/server/storage core.

---

# 26. Axum Isolation

Axum/Tower belong in:

```text
aequora-axum
```

not domain execution.

---

# 27. Native Dependency Policy

Aequora prefers Rust-native dependencies where practical.

Reasons:

```text
cross-compilation
mobile support
supply-chain simplicity
memory safety
build reproducibility
deployment
```

But "pure Rust" is not an absolute excuse to choose an immature implementation over a well-understood mature boundary.

---

# 28. SQLite Exception

SQLite itself is implemented in C.

Therefore:

```text
aequora-sqlite
```

is an explicit optional native-code boundary.

Aequora core remains Rust.

SQLite is justified by:

```text
maturity
portability
ecosystem
embedded reliability
reference-adapter value
```

---

# 29. Native Boundary Inventory

Maintain a list of crates introducing:

```text
C
C++
assembly
system libraries
```

into the build.

---

# 30. Build Script Risk

`build.rs` executes code during build.

Treat it as part of the supply-chain attack surface.

Review:

```text
filesystem access
network access
code generation
native compilation
environment dependence
```

---

# 31. Proc Macro Risk

Proc macros execute compiler-time Rust code.

Review important proc-macro dependencies, especially those used broadly.

---

# 32. Unsafe Code Policy

Classify crates by unsafe usage:

```text
No Unsafe
Contained Unsafe
Significant Unsafe
Unknown
```

---

# 33. Unsafe Is Not Automatically Rejected

Many mature low-level libraries use necessary unsafe code.

But Tier 2/3 dependencies with significant unsafe deserve deeper review.

---

# 34. `unsafe` in Aequora

Default policy:

```text
safe Rust
```

Any Aequora-owned unsafe block requires:

```text
documented invariant
review
tests
fuzzing where relevant
measured need
```

---

# 35. Cryptography Policy

Do not implement novel cryptographic primitives.

Prefer widely reviewed Rust cryptography libraries with clear maintenance and licensing.

---

# 36. Crypto Agility

Part 15 requires algorithm agility.

Dependency selection should allow replacing crypto providers without rewriting synchronization semantics.

---

# 37. TLS

TLS implementation belongs behind transport/integration boundaries.

Do not expose TLS library types through core APIs.

---

# 38. Authentication Dependencies

JWT/OIDC/password libraries are security-critical.

Require Tier 3 review.

---

# 39. Serialization Dependencies

Postcard is part of Aequora's preferred binary encoding strategy.

Serde-related dependencies are therefore broad foundational dependencies and should be upgraded carefully.

---

# 40. Durable Format Sensitivity

Updating a serializer library must not silently change a durable/wire format contract.

Golden fixtures from Part 48 protect this.

---

# 41. Compression Dependencies

Compression is optional optimization.

A compression dependency must not become necessary to interpret canonical uncompressed semantics unless explicitly protocol-governed.

---

# 42. Hash Dependencies

BLAKE3 usage should remain explicit and domain-separated according to Part 15.

---

# 43. Randomness

Cryptographic randomness and ordinary deterministic test randomness are separate concerns.

Never use test PRNG policy for security-sensitive key material.

---

# 44. Database Dependencies

Database drivers are Tier 2.

Review:

```text
transaction semantics
runtime integration
TLS
pooling
migration behavior
error mapping
platform support
```

---

# 45. SQLx Boundary

If SQLx is used for PostgreSQL:

```text
SQLx remains internal to aequora-postgres
```

No SQLx transaction or row type appears in public Aequora APIs.

---

# 46. SQLite Driver Choice

`aequora-sqlite` may use a suitable Rust SQLite binding internally.

The public adapter contract must not depend on that binding.

This permits future replacement.

---

# 47. Stoolap Dependency

Stoolap remains isolated behind `aequora-stoolap`.

Its maturity is evaluated by conformance evidence rather than preference alone.

---

# 48. Dependency Ownership

Every Tier 2/3 dependency should have an internal owner or owning area:

```text
storage
security
protocol
runtime
```

---

# 49. Update Responsibility

Owner responsibilities:

```text
watch advisories
review major updates
validate migration
maintain replacement plan
```

---

# 50. Version Pinning

Cargo.lock should be committed for shipped applications/workspace reproducibility.

Libraries still declare semantically appropriate version ranges in manifests.

---

# 51. Lockfile Authority

Release builds use the reviewed lockfile.

Do not allow uncontrolled dependency resolution during release.

---

# 52. `--locked`

CI/release builds should use locked dependency resolution where appropriate.

---

# 53. Offline Builds

For high-assurance/air-gapped release workflows, support building from a pre-fetched/vendor-reviewed dependency set.

---

# 54. Vendoring

Optional vendor workflow:

```text
cargo vendor
↓
review/archive dependency sources
↓
offline build
```

especially useful for enterprise/air-gapped releases.

---

# 55. Source Provenance

For every resolved dependency, preserve:

```text
package name
version
source registry/repository
checksum
license
```

---

# 56. Git Dependencies

Avoid unpinned branch-based Git dependencies in production releases.

If a Git dependency is necessary, pin an immutable commit and record provenance.

---

# 57. Path Dependencies

Workspace path dependencies are expected.

Release tooling must distinguish internal crates from external third-party code.

---

# 58. Forked Dependencies

If Aequora forks a dependency:

```text
document reason
upstream source
fork commit
local changes
license
security responsibility
upstream sync strategy
```

---

# 59. Fork Governance

A fork makes Aequora partially responsible for maintenance.

Do not fork merely to avoid waiting for upstream unless long-term ownership is acceptable.

---

# 60. Abandoned Dependencies

Signals:

```text
unanswered critical issues
unmerged security fixes
stale releases
broken current toolchain
unsupported platforms
```

do not automatically prove abandonment, but increase risk.

---

# 61. Replacement Plan

Tier 2/3 dependencies should ideally sit behind boundaries that make replacement possible.

Examples:

```text
StorageAdapter
SyncTransport
CryptoProvider
SecretProvider
TelemetryExporter
```

---

# 62. Dependency Health Score

A governance tool may record:

```text
maintenance activity
release cadence
open security issues
bus factor
documentation
platform support
```

as advisory evidence.

Do not reduce trust to one simplistic score.

---

# 63. Transitive Dependencies

Review not only direct dependencies.

A tiny direct crate can pull dozens of transitive packages.

---

# 64. Feature Pruning

Disable unnecessary default features.

Example principle:

```toml
default-features = false
```

when this is actually supported and tested.

---

# 65. Feature Unification Awareness

Cargo feature unification can enable features transitively.

CI should inspect the resolved feature graph for important crates.

---

# 66. Dependency Graph Report

Generate:

```text
direct dependencies
transitive count
duplicate versions
native dependencies
build scripts
proc macros
licenses
```

---

# 67. Duplicate Versions

Duplicate crate versions are not automatically wrong.

But excessive duplication increases:

```text
binary size
compile time
audit surface
```

---

# 68. Minimal Versions

Minimum-supported-version testing may be useful for public libraries, but only if Aequora commits to such a policy.

Do not claim it without CI evidence.

---

# 69. MSRV

Define a Minimum Supported Rust Version deliberately.

Record it in:

```text
workspace metadata
documentation
CI
```

---

# 70. Rust Edition

Keep edition policy workspace-wide unless a specific crate needs otherwise.

---

# 71. Toolchain Pinning

Release CI should pin a Rust toolchain.

Example:

```text
rust-toolchain.toml
```

---

# 72. Toolchain Provenance

Record:

```text
rustc version
cargo version
target
components
```

in release evidence.

---

# 73. Supply-Chain Threat Model

Threats include:

```text
malicious package release
compromised maintainer account
dependency takeover
typosquatting
malicious build script
compromised CI action
stolen release key
tampered artifact
registry compromise
dependency confusion
```

---

# 74. New Dependency Review

A new dependency should trigger automated and human checks.

---

# 75. Automated Checks

Possible tooling:

```text
cargo-deny
cargo-audit or equivalent advisory tooling
cargo metadata
license scanners
SBOM generators
```

Tool choices remain replaceable.

---

# 76. `cargo-deny`

Useful policy areas include:

```text
licenses
advisories
bans
sources
```

Keep configuration in version control.

---

# 77. Advisory Databases

Security scanning should use maintained vulnerability/advisory sources.

An advisory hit requires triage, not blind suppression.

---

# 78. Advisory Triage

Classify:

```text
Affected
NotReachable
NotAffectedByConfiguration
Mitigated
FalsePositive
PendingUpgrade
```

with evidence.

---

# 79. Advisory Exceptions

Every ignored advisory requires:

```text
advisory ID
reason
owner
expiry
```

---

# 80. Dependency Ban

Ban known problematic:

```text
licenses
sources
versions
crate duplicates where necessary
```

through policy tooling.

---

# 81. Registry Source Policy

Default external Rust packages should come from approved registries/sources.

Unknown alternative registries require review.

---

# 82. Dependency Confusion

Internal crate names should not accidentally resolve to public packages.

Use explicit workspace/path/source configuration.

---

# 83. Typosquatting

Review exact package identity before adding.

Do not rely solely on similar names.

---

# 84. Maintainer Verification

For high-risk dependencies, inspect:

```text
official repository
package ownership
release provenance where available
```

---

# 85. Update Automation

Automated dependency update PRs are useful.

They must not auto-merge Tier 2/3 changes without required verification.

---

# 86. Update Classes

```text
Patch
Minor
Major
SecurityEmergency
```

---

# 87. Patch Update

Still run relevant tests.

SemVer reduces risk but does not eliminate it.

---

# 88. Major Update

Requires:

```text
changelog review
API review
behavior review
migration impact
benchmark
full relevant tests
```

---

# 89. Security Emergency

May justify accelerated release but never bypass core correctness tests.

---

# 90. Dependency Freeze

Before a major release candidate, consider a short dependency freeze except:

```text
critical security
release blockers
```

to stabilize evidence.

---

# 91. SBOM

Every production release should produce a Software Bill of Materials.

---

# 92. SBOM Contents

At minimum:

```text
component
version
supplier/source where available
license
dependency relationship
artifact identity
```

---

# 93. SBOM Formats

Support established machine-readable SBOM formats such as:

```text
CycloneDX
SPDX
```

according to ecosystem/customer requirements.

---

# 94. SBOM Scope

Generate per shipped artifact/profile:

```text
server
CLI
desktop
Android
iOS
agent
```

because feature/dependency graphs differ.

---

# 95. SBOM Build Binding

SBOM must correspond to the actual resolved dependency graph used to build the artifact.

---

# 96. SBOM Artifact Digest

Associate:

```text
artifact digest
SBOM digest
build ID
```

---

# 97. SBOM Signing

High-assurance releases can sign SBOM/provenance metadata alongside binaries.

---

# 98. SBOM Retention

Keep SBOMs for every supported release to answer future vulnerability questions.

---

# 99. Vulnerability Query

Operational tooling should be able to answer:

```text
Which supported Aequora artifacts contain crate X version Y?
```

---

# 100. Provenance

Release provenance should describe:

```text
source commit
builder identity
toolchain
dependency lockfile
build parameters
artifact digest
```

---

# 101. SLSA-Style Direction

Aequora can progressively adopt stronger software supply-chain provenance concepts without making one external framework part of runtime architecture.

---

# 102. Hermetic Build Goal

Release builds should minimize dependence on uncontrolled machine state.

---

# 103. Build Inputs

Explicitly capture:

```text
source
lockfile
toolchain
target
features
environment configuration
native toolchain where required
```

---

# 104. Network During Build

High-assurance builds should avoid arbitrary network access after dependencies/toolchains are staged.

---

# 105. Environment Variables

Build output should not accidentally depend on undeclared environment variables.

---

# 106. Timestamps

Embedding current timestamps harms byte-for-byte reproducibility.

Prefer deterministic build metadata where possible.

---

# 107. Build Paths

Absolute filesystem paths can also harm reproducibility.

Use path remapping where appropriate.

---

# 108. Reproducible Builds

Goal:

```text
same declared inputs
↓
same artifact
```

where platform/toolchain permits.

---

# 109. Reproducibility Levels

Define:

```text
Level 0 — no claim
Level 1 — functionally reproducible
Level 2 — deterministic package contents
Level 3 — byte-identical artifact
```

Do not claim Level 3 without evidence.

---

# 110. Native-Code Limitation

Native dependencies/platform packaging may make byte-identical reproducibility harder.

Report actual achieved level per artifact.

---

# 111. Rebuild Verification

Release pipeline can rebuild an artifact in an independent environment and compare:

```text
digest
package contents
SBOM
```

according to declared reproducibility level.

---

# 112. Reproducibility Report

Record:

```text
artifact
level
builder A digest
builder B digest
differences
```

---

# 113. Release Artifact Manifest

Concept:

```rust
pub struct ReleaseArtifactManifest {
    pub product: ProductId,
    pub version: ReleaseVersion,
    pub target: TargetTriple,
    pub build_id: BuildId,
    pub source_commit: CommitId,
    pub artifact_digest: Digest,
    pub sbom_digest: Digest,
    pub provenance_digest: Digest,
}
```

---

# 114. Artifact Signing

Sign release artifacts using controlled release keys.

---

# 115. Signing Key Isolation

Release signing keys should not be ordinary CI environment variables available to every build job.

Use:

```text
dedicated signing service
hardware-backed key
protected CI environment
```

depending deployment maturity.

---

# 116. Key Rotation

Signing keys need:

```text
identity
validity
rotation
revocation
```

policy.

---

# 117. Signature Verification

Installers/updaters should verify signatures before accepting updates.

---

# 118. Update Metadata

Signed update metadata should bind:

```text
version
artifact digest
target
release channel
```

---

# 119. Rollback Protection

For security-sensitive products, updater policy may reject unauthorized downgrade.

Compatibility and emergency rollback policy must remain explicit.

---

# 120. Release Channels

Example:

```text
nightly
beta
stable
LTS
```

Each has separate trust/support expectations.

---

# 121. Crate Publication

Not every workspace crate needs publication to crates.io.

Classify:

```text
Public SDK
Public Adapter SDK
Official Adapter
Internal
Application-Only
```

---

# 122. Public Crate Naming

Reserve consistent names:

```text
aequora
aequora-client
aequora-server
aequora-adapter-sdk
aequora-postgres
aequora-sqlite
aequora-stoolap
aequora-axum
aequora-dioxus
```

before broad ecosystem growth where possible.

---

# 123. Crate Ownership

Each public crate has:

```text
maintainer group
stability tier
release policy
MSRV
license
security contact
```

---

# 124. Internal Crates

Mark non-public crates clearly.

Do not accidentally create ecosystem dependency on unstable internals.

---

# 125. Publish Allowlist

Release tooling should publish only explicitly allowed crates.

---

# 126. Crate Metadata

Public crates should include:

```text
license
repository
documentation
description
rust-version
keywords/categories where useful
```

---

# 127. README Accuracy

README claims such as:

```text
production-ready
mobile-supported
official adapter
```

must match certification/release evidence.

---

# 128. Crate SemVer

Part 35 governs API stability.

Dependency governance ensures coordinated version release where crates are lockstep.

---

# 129. Lockstep Initial Versioning

Initially, keeping Aequora crates on one workspace release version simplifies:

```text
compatibility
support
SBOM
release
```

---

# 130. Independent Versioning Later

Only split versions when ecosystem maturity justifies the operational complexity.

---

# 131. Crate Yank Policy

Yank a release when necessary for severe defects, but remember existing lockfiles may still use yanked versions.

Security response must address actual users.

---

# 132. Security Policy

Repository should include:

```text
SECURITY.md
```

with private vulnerability reporting instructions.

---

# 133. Vulnerability Disclosure

Define:

```text
intake
acknowledgement
triage
fix
release
advisory
credit
```

---

# 134. CVE/GHSA Handling

For applicable vulnerabilities, coordinate ecosystem advisories through appropriate channels.

---

# 135. Security Patch Support

Define which release lines receive security fixes.

Example policy could be:

```text
latest stable
plus designated LTS
```

only if the project can actually maintain it.

---

# 136. End-of-Life

Every supported line should eventually have:

```text
EOL date/policy
```

or explicit rolling support rules.

---

# 137. Dependency EOL

If a critical dependency becomes unsupported:

```text
upgrade
replace
fork temporarily
or
reduce supported profile
```

with explicit decision.

---

# 138. Third-Party Service Risk

Dependencies are not only crates.

Aequora deployments may rely on:

```text
Neon
object storage
OIDC provider
email provider
payment provider
telemetry service
```

These are third-party operational dependencies.

---

# 139. Service Inventory

Maintain:

```text
provider
purpose
data handled
failure impact
replacement strategy
residency
contract/SLA
```

---

# 140. Neon Risk Boundary

Neon is an operational PostgreSQL provider.

Aequora's authoritative semantics remain PostgreSQL-based so a deployment can migrate to another compatible PostgreSQL environment if necessary.

---

# 141. Provider Portability

Avoid embedding provider-specific semantics in domain/core code.

---

# 142. Object Storage Portability

Use an Aequora blob/snapshot storage abstraction rather than hardwiring one cloud provider.

---

# 143. Identity Provider Portability

Normalize external auth into `AuthContext`.

Core authorization does not depend on a specific OIDC vendor.

---

# 144. Telemetry Provider Portability

Part 46 uses vendor-neutral internal telemetry boundaries.

---

# 145. Payment Provider Boundary

Payment semantics must be modeled as domain/provider adapters, not direct vendor objects throughout ERP code.

---

# 146. Third-Party Outage

Each service inventory entry should state:

```text
what happens if unavailable?
```

Example:

```text
telemetry unavailable -> core continues
email unavailable -> durable job retries
OIDC unavailable -> existing session policy applies
object storage unavailable -> snapshot/blob work degrades
```

---

# 147. Third-Party Data Exposure

Record whether a provider receives:

```text
PII
financial data
credentials
encrypted blobs
telemetry
```

---

# 148. Data Processing Review

Enterprise deployments may require legal/privacy review of external service providers.

Architecture should make providers replaceable or disableable.

---

# 149. Air-Gapped Profile

Air-gapped Aequora must be able to operate without:

```text
crates.io
GitHub
cloud telemetry
external identity
external object storage
automatic update service
```

after required artifacts are staged.

---

# 150. Air-Gapped Dependency Bundle

Prepare:

```text
source/vendor bundle
toolchain
SBOM
licenses
signatures
checksums
```

for controlled environments.

---

# 151. Offline Verification

Air-gapped installer can verify:

```text
artifact signature
SBOM digest
release manifest
```

without internet.

---

# 152. CI Supply Chain

CI itself is part of the trusted build system.

Review:

```text
third-party CI actions
runner images
secrets
permissions
cache
artifacts
```

---

# 153. CI Least Privilege

PR jobs should not receive release signing credentials.

---

# 154. Untrusted Pull Requests

Treat code from forks as untrusted.

Do not expose secrets to their build steps.

---

# 155. CI Action Pinning

Pin third-party CI actions/tool versions according to platform capability and review update changes.

---

# 156. Build Cache Risk

Cache must not allow untrusted jobs to poison trusted release builds.

Separate trust domains where necessary.

---

# 157. Release Runner

Production releases should use a hardened, controlled runner/environment.

---

# 158. Release Approval

Stable release may require:

```text
quality gates passed
security checks passed
SBOM generated
provenance generated
artifact signed
human approval
```

---

# 159. Two-Person Release Approval

High-assurance enterprise releases may require two-person approval for signing/publishing.

---

# 160. Artifact Repository

Published binaries should reside in controlled repositories with:

```text
immutability
checksums
signatures
retention
```

---

# 161. Package Managers

Distribution may include:

```text
crates.io
GitHub-style releases
Linux packages
Windows installer
macOS package
Android package
iOS framework
containers
```

Each has separate signing/provenance requirements.

---

# 162. Container Images

If shipping containers:

```text
minimal base
non-root
pinned base digest
SBOM
signature
vulnerability scan
```

---

# 163. Base Image Policy

Prefer small maintained images.

Do not use floating:

```text
latest
```

for reproducible releases.

---

# 164. Distroless/Scratch

Use only where operational/debugging requirements are satisfied.

Minimalism is useful but not a goal above operability.

---

# 165. System Packages

Native/system package dependencies must appear in deployment documentation and provenance.

---

# 166. Mobile Dependencies

Android/iOS wrappers introduce:

```text
Gradle packages
Android SDK/NDK
Apple SDK/toolchain
binding generators
```

These also require version governance.

---

# 167. Java/Kotlin Dependencies

Keep mobile platform glue small to reduce a second ecosystem's dependency surface.

---

# 168. Swift Dependencies

Likewise keep Swift-side dependencies minimal.

---

# 169. Node/Python Tooling

If used only for docs/build tooling, keep them outside core release path where possible.

---

# 170. Generated Code

Generated code must be reproducible from:

```text
source registry/schema
generator version
```

---

# 171. Generator Pinning

Registry/protocol generators are build inputs and should be version-controlled/pinned.

---

# 172. Generated-Code Verification

CI can regenerate and fail if committed generated output differs.

---

# 173. Dependency Update Verification Matrix

For Tier 2/3 update:

```text
unit
property
model
adapter conformance
integration
migration
security
performance critical subset
```

as relevant.

---

# 174. Storage Dependency Update

Additionally test:

```text
existing DB reopen
old store migration
crash recovery
backup/restore
```

---

# 175. Protocol Dependency Update

Additionally test:

```text
golden fixtures
compatibility
fuzz
```

---

# 176. Crypto Dependency Update

Additionally test:

```text
known vectors
key rotation
artifact signatures
old ciphertext/signatures
```

---

# 177. HTTP Dependency Update

Additionally test:

```text
limits
timeouts
TLS
proxy
disconnect
graceful shutdown
```

---

# 178. Dependency Removal

Regularly remove dependencies no longer needed.

Dead dependencies still create supply-chain and compile-time cost.

---

# 179. Dependency Dashboard

Useful internal view:

```text
direct dependency count
high-risk dependencies
outdated critical dependencies
active advisories
license exceptions
native boundaries
proc macros
build scripts
```

---

# 180. Supply-Chain Alerts

Alert/ticket on:

```text
critical advisory
revoked signing key
unexpected lockfile change
new unapproved source
SBOM mismatch
artifact signature failure
```

---

# 181. Lockfile Review

Pull requests changing `Cargo.lock` should make the dependency changes visible.

---

# 182. Unexpected Transitive Change

A one-line manifest update may change many transitive packages.

Review the resolved diff.

---

# 183. Dependency Diff Artifact

CI can generate:

```text
added
removed
updated
license changed
source changed
risk tier
```

---

# 184. Source Change Alert

Same crate/version from unexpected source should fail.

---

# 185. Checksum Mismatch

Registry/package checksum mismatch is a hard failure.

---

# 186. Build Network Audit

High-assurance CI may detect unexpected outbound network access during build.

---

# 187. Runtime Network Inventory

Document which components may make outbound connections:

```text
server -> PostgreSQL
server -> OIDC/JWKS
worker -> provider
telemetry exporter -> collector
```

Core storage/domain code should not unexpectedly access network.

---

# 188. Plugin Risk

Aequora v1 should avoid arbitrary runtime plugins.

Compile-time integration gives:

```text
known dependency graph
SBOM visibility
strong typing
reviewability
```

---

# 189. Future Plugins

If introduced later, require:

```text
signed package
capability manifest
sandboxing
registry namespace
compatibility policy
```

---

# 190. Dynamic Library Loading

Avoid in core v1.

It complicates:

```text
ABI
security
reproducibility
SBOM
support
```

---

# 191. C ABI Boundary

For foreign-language integrations, Aequora controls its own stable C ABI wrapper.

Do not expose arbitrary third-party native ABI.

---

# 192. Binary Provenance

`aequora version --verbose` should show safe provenance:

```text
Aequora version
Git commit
build ID
target
Rust version
feature profile
registry generation
```

---

# 193. Dependency Provenance CLI

Possible:

```text
aequora supply-chain summary
aequora supply-chain licenses
aequora supply-chain sbom
aequora supply-chain verify
```

---

# 194. Release Verification CLI

Example:

```text
aequora release verify \
    --artifact aequora-server \
    --manifest release-manifest.ron
```

verifies:

```text
digest
signature
SBOM binding
provenance
```

---

# 195. Dependency Policy CLI

Development tooling:

```text
aequora deps verify
aequora deps diff
aequora deps explain <crate>
```

---

# 196. Exception Record

Concept:

```rust
pub struct DependencyException {
    pub dependency: PackageId,
    pub policy: PolicyId,
    pub reason: String,
    pub owner: OwnerId,
    pub expires_at: Date,
}
```

---

# 197. Exceptions Expire

No indefinite:

```text
temporary ignore
```

without review.

---

# 198. Risk Acceptance

Some dependencies may be worth accepting despite risk.

Document:

```text
benefit
risk
mitigation
replacement
review date
```

---

# 199. Dependency ADR

Tier 3 dependencies should have an ADR explaining selection and alternatives.

---

# 200. Example PostgreSQL ADR

Compare:

```text
SQLx
alternative drivers
```

on:

```text
async integration
type system
migrations
TLS
maintenance
license
```

without exposing choice beyond adapter boundary.

---

# 201. Example SQLite ADR

Document:

```text
why SQLite is accepted despite native C engine
which Rust binding
bundled vs system SQLite policy
platform support
security/update responsibility
```

---

# 202. Bundled SQLite vs System SQLite

This is a release-profile decision.

Bundled:

```text
predictable version
reproducible behavior
larger responsibility for updates
```

System:

```text
OS-managed security updates
version variability
feature variability
```

Conformance profile must record which model is tested.

---

# 203. Stoolap ADR

Document maturity, capability/conformance status, migration strategy, and fallback to SQLite.

---

# 204. Dependency Substitution

Architecture should permit:

```text
Stoolap -> SQLite
PostgreSQL provider A -> provider B
telemetry vendor A -> vendor B
```

without rewriting domain/protocol semantics.

---

# 205. Third-Party Risk Register

Maintain entries for critical crates/services:

```text
identity
purpose
risk
failure mode
security history
license
replacement difficulty
owner
review date
```

---

# 206. Review Frequency

Tier 3 dependencies:

```text
continuous advisory monitoring
periodic deeper review
```

Tier 0 tools need less scrutiny.

---

# 207. Bus Factor

Low-maintainer critical dependencies increase maintenance risk.

This is a selection signal, not an automatic rejection.

---

# 208. Maintainer Transition

If a package changes ownership unexpectedly, trigger review for critical dependencies.

---

# 209. Package Namespace Takeover

A dormant package acquired by an unknown maintainer may materially change risk.

---

# 210. Dependency Source Archive

For long-lived enterprise support, retain exact source packages required to reproduce supported releases where legally permitted.

---

# 211. Long-Term Support

LTS requires retaining:

```text
toolchain
dependencies
source
SBOM
signing verification material
build instructions
```

---

# 212. Cryptographic Timestamping

Optional high-assurance release provenance may use external timestamp/transparency mechanisms.

Do not make them required for air-gapped operation.

---

# 213. Transparency Log Direction

Future public releases may publish signed provenance to a transparency system.

The architecture should keep this optional.

---

# 214. License Change Detection

A dependency update that changes license metadata must trigger review.

---

# 215. Repository License Files

CI should ensure all Aequora public crates/packages contain correct licensing metadata.

---

# 216. Documentation

Maintain:

```text
DEPENDENCIES.md
SECURITY.md
RELEASING.md
LICENSE
THIRD_PARTY_NOTICES
```

where appropriate.

---

# 217. Developer Workflow

Adding a dependency:

```text
cargo add / Cargo.toml edit
↓
dependency policy scan
↓
license/source/risk review
↓
tests
↓
lockfile diff
↓
approval
```

---

# 218. AI-Assisted Dependency Selection

AI-generated code must not add crates casually.

Repository instructions should require:

```text
reuse existing dependency if appropriate
justify new crate
check license
check maintenance
check feature graph
check architecture layer
```

---

# 219. AI Hallucinated Crates

Never trust an AI-generated crate name/version without verifying that the package actually exists and is the intended project.

---

# 220. AI and License Claims

Do not rely on model memory for legal licensing facts.

Automation should inspect package metadata/license files and humans should review material decisions.

---

# 221. AI and Security Updates

An AI may propose upgrading a vulnerable dependency, but CI/conformance evidence determines whether the update is safe to ship.

---

# 222. Repository Agent Rule

Suggested:

```text
DO NOT add, remove, or materially upgrade a Tier 2/3 dependency without explaining:
1. why it is needed,
2. its license,
3. architecture boundary,
4. transitive/native/build-script impact,
5. required verification.
```

---

# 223. Supply-Chain Invariants

## AEQ-INV-SUPPLY001

```text
No production dependency with an unknown, prohibited, or unreviewed license may enter a release artifact.
```

## AEQ-INV-SUPPLY002

```text
Every shipped artifact is bound to its source commit, reviewed dependency lockfile, build configuration, SBOM, provenance metadata, and cryptographic artifact digest.
```

## AEQ-INV-SUPPLY003

```text
Tier 2 and Tier 3 dependencies remain behind architectural boundaries that prevent their implementation-specific types and semantics from leaking into Aequora core contracts.
```

## AEQ-INV-SUPPLY004

```text
Dependency advisories, policy exceptions, and risk acceptances cannot be silently ignored; each exception is explicit, owned, justified, and time-bounded.
```

## AEQ-INV-SUPPLY005

```text
Release signing credentials are isolated from untrusted pull-request/build execution and ordinary developer jobs.
```

## AEQ-INV-SUPPLY006

```text
A dependency update affecting durable storage, protocol, cryptography, authority, or security semantics must pass the corresponding compatibility, conformance, migration, and security verification before release.
```

## AEQ-INV-SUPPLY007

```text
Aequora does not require uncontrolled network access during high-assurance release builds once declared build inputs have been staged.
```

## AEQ-INV-SUPPLY008

```text
Runtime third-party services remain explicit dependencies with documented failure behavior and cannot silently become hidden sources of authority.
```

## AEQ-INV-SUPPLY009

```text
Claims of reproducible builds, official adapters, supported platforms, or secure dependency status are made only at the level demonstrated by current evidence.
```

## AEQ-INV-SUPPLY010

```text
New dependencies are justified by capability and lifecycle value; convenience alone does not override legal, security, architectural, and maintenance risk.
```

---

# 224. Recommended v1 Dependency Policy

For Aequora v1:

```text
prefer permissive dependencies
deny unknown licenses
explicitly review copyleft/restrictive licenses
commit Cargo.lock
use locked CI/release builds
run advisory/license/source checks
minimize native dependencies
isolate SQLite native boundary
minimize proc macros/build scripts
review all Tier 2/3 dependencies
generate SBOM per artifact
sign release artifacts
retain provenance
```

---

# 225. Recommended Dependency Tiers

Tier 3:

```text
crypto
TLS
authentication
authorization-critical helpers
release signing
```

Tier 2:

```text
PostgreSQL driver
SQLite binding
Stoolap
Postcard/Serde durable format dependencies
Axum/Tower transport path
```

Tier 1:

```text
general runtime utilities
CLI libraries
observability exporters
```

Tier 0:

```text
test/dev helpers
```

Actual tier assignment remains reviewed in repository policy.

---

# 226. Recommended Supply-Chain CI

Every PR:

```text
cargo fmt
cargo clippy
dependency policy
license check
source check
lockfile diff
advisory scan
```

Dependency-changing PR:

```text
dependency diff report
risk classification
targeted tests
```

Release:

```text
full verification
SBOM
provenance
reproducibility check
artifact signing
signature verification
third-party notices
```

---

# 227. Recommended Repository Files

```text
/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── deny.toml
├── LICENSE
├── SECURITY.md
├── DEPENDENCIES.md
├── RELEASING.md
├── THIRD_PARTY_NOTICES.md
└── supply-chain/
    ├── policy.ron
    ├── exceptions.ron
    ├── risk-register.ron
    └── approved-sources.ron
```

---

# 228. Final Architecture

```text
                    Source Repository
                           |
                           v
                  Dependency Policy
             /             |              \
            v              v               v
        License         Security        Architecture
         Check          Advisory         Boundary
            \              |               /
             +-------------+--------------+
                           |
                           v
                    Reviewed Lockfile
                           |
                           v
                    Controlled Build
                 /          |          \
                v           v           v
             Tests        SBOM      Provenance
                \           |           /
                 +----------+----------+
                            |
                            v
                    Signed Artifacts
                            |
                            v
                   Release Repository
                            |
                            v
                    Runtime Verification
```

Third-party services remain separate:

```text
Aequora Core
   |
   +--> PostgreSQL provider adapter
   +--> object-storage adapter
   +--> identity-provider adapter
   +--> notification/payment adapters
   +--> telemetry exporter
```

No provider becomes the semantic core merely because it is convenient.

---

# 229. Final Recommendation

Aequora should deliberately maintain a **small, replaceable, well-governed dependency surface**.

The selection hierarchy should be:

```text
correct semantics
↓
license compatibility
↓
security and maintenance quality
↓
platform support
↓
replaceability
↓
performance
↓
developer convenience
```

For critical dependencies, the question is not merely:

```text
"Does this crate work?"
```

It is:

```text
Can we legally ship it?
Can we trust its update path?
Can we reproduce the build?
Can we identify it in every shipped artifact?
Can we patch or replace it years later?
Can we prove an upgrade did not change Aequora's durable semantics?
```

> **A dependency is borrowed code plus borrowed risk. Aequora should know exactly what it borrows, why it borrows it, how it verifies it, and how it can replace it.**

---

## Next

**Part 50 — Aequora v1 Scope, Productization Plan, Milestones, Production Readiness, Release Candidate Process, General Availability Exit Criteria, and Long-Term Evolution Architecture**
