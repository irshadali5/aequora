# Aequora Sync — Part 29

# Schema, Operation Registry, and Developer Governance Architecture

## 1. Purpose

Aequora now depends on many stable identifiers and evolving contracts:

```text
OperationKind
EventKind
EntityType
FieldId
CapabilityId
ErrorCode
ProfileId
ConsumerKind
JobKind
AuditActionId
MetadataMigrationId
ProtocolVersion
SchemaVersion
```

These identifiers are not ordinary implementation details.

Once persisted or sent over the wire, they become part of a long-lived interoperability contract.

If developers casually:

```text
reuse an ID
rename semantics without versioning
change an enum discriminant
delete an operation kind
change field meaning
ship incompatible schema silently
```

then old clients, snapshots, journals, replays, imports, audit history, and downstream consumers may become ambiguous or corrupt.

Aequora therefore needs explicit developer governance around every durable semantic identifier.

The central rule is:

> **Any identifier or schema that can survive a process restart, cross a network boundary, enter a journal, or appear in an artifact must be governed as a durable public contract.**

---

# 2. Goals

The registry/governance architecture should provide:

```text
stable numeric IDs
canonical schema registries
ownership metadata
deprecation lifecycle
reserved ID ranges
extension namespaces
code generation
CI collision checks
compatibility review
schema diff tooling
change proposals
release notes
migration hooks
ecosystem governance
```

---

# 3. Non-Goals

This subsystem is not:

```text
a generic package registry
a dynamic plugin marketplace
a runtime reflection framework
a schema-less escape hatch
```

The aim is durable semantic discipline.

---

# 4. Registry Domains

Aequora should govern at least:

```text
EntityTypeRegistry
OperationRegistry
EventRegistry
FieldRegistry
CapabilityRegistry
ConsistencyProfileRegistry
ErrorCodeRegistry
JobKindRegistry
ConsumerKindRegistry
AuditActionRegistry
MigrationRegistry
ProtocolRegistry
```

---

# 5. Canonical Registry Source

Recommended canonical source format:

```text
RON
```

because it is:

```text
human-readable
Rust-friendly
reviewable
version-control friendly
```

---

# 6. Registry Directory

Suggested:

```text
registry/
├── entities.ron
├── operations.ron
├── events.ron
├── fields.ron
├── capabilities.ron
├── profiles.ron
├── errors.ron
├── jobs.ron
├── consumers.ron
├── audit-actions.ron
├── migrations.ron
└── protocol.ron
```

---

# 7. Registry Is Source-Controlled

Registry changes happen through:

```text
code review
CI validation
change proposal
```

not production database edits.

---

# 8. Stable Numeric IDs

Canonical IDs should use explicit integers.

Example:

```rust
pub struct OperationKind(pub u32);
```

---

# 9. Never Persist Rust Enum Discriminants Implicitly

Bad:

```rust
enum OperationKind {
    CreateStudent,
    UpdateStudent,
}
```

then serialize discriminant by declaration order.

Reordering variants would break compatibility.

---

# 10. Explicit Mapping

Good:

```rust
pub const CREATE_STUDENT: OperationKind = OperationKind(1001);
pub const UPDATE_STUDENT: OperationKind = OperationKind(1002);
```

---

# 11. ID Immutability

Once an ID is published:

```text
ID meaning never changes
```

---

# 12. ID Retirement

Removed/deprecated IDs remain reserved forever.

---

# 13. No Reuse

If OperationKind 1005 represented:

```text
ApproveInvoice
```

it can never later mean:

```text
DeleteInvoice
```

---

# 14. Registry Entry

Common shape:

```rust
pub struct RegistryEntry {
    pub id: u32,
    pub name: String,
    pub status: SupportStatus,
    pub introduced_in: VersionRef,
    pub owner: OwnerRef,
}
```

Specific registries add semantic metadata.

---

# 15. OwnerRef

Define:

```text
team
crate
domain module
maintainer group
```

not necessarily individual person.

---

# 16. Ownership Purpose

Every durable contract should have a clear maintainer.

---

# 17. Registry Status

Use lifecycle:

```text
Experimental
Current
Supported
Deprecated
RetryOnly
Reserved
Removed
```

---

# 18. Experimental IDs

Experimental entries should use reserved experimental range.

---

# 19. Experimental Data Warning

Do not allow experimental IDs into production durable history unless feature explicitly accepts migration risk.

---

# 20. Core vs Vendor Ranges

Recommended conceptual allocation:

```text
Core
RegisteredExtension
PrivateExperimental
```

---

# 21. Example ID Ranges

Possible:

```text
0x0000_0000 – 0x3FFF_FFFF  Aequora Core
0x4000_0000 – 0xBFFF_FFFF  Registered Extensions
0xC000_0000 – 0xEFFF_FFFF  Vendor Private
0xF000_0000 – 0xFFFF_FFFF  Experimental
```

Exact ranges can be finalized before ecosystem launch.

---

# 22. Namespace Registry

If third-party ecosystem grows, allocate:

```rust
pub struct VendorNamespaceId(u16);
```

then compose IDs.

---

# 23. Composite Extension ID

Possible structure:

```text
vendor namespace
+
local ID
```

Avoid global collision.

---

# 24. EntityType Registry

Each entity type entry includes:

```text
EntityType ID
canonical name
aggregate root relation
default consistency profile
current schema version
owner
```

---

# 25. EntityTypeId

```rust
pub struct EntityType(pub u32);
```

---

# 26. Entity Schema Version

Each entity may evolve independently.

---

# 27. Entity Registry Example

```ron
(
    id: 100,
    name: "Student",
    schema_version: 3,
    profile: OptimisticVersioned,
    owner: "school-domain",
)
```

---

# 28. Operation Registry

This is one of the most important registries.

Each entry should include:

```text
OperationKind
name
aggregate type
current schema version
supported schema versions
consistency profile
ordering policy
offline policy
compaction policy
rebase policy
authorization policy ref
handler owner
status
```

---

# 29. Operation Descriptor

Conceptually:

```rust
pub struct OperationDescriptor {
    pub kind: OperationKind,
    pub entity_type: EntityType,
    pub current_schema: OperationSchemaVersion,
    pub profile: ConsistencyProfileId,
    pub status: SupportStatus,
}
```

---

# 30. Operation Creation Policy

Registry declares whether new creation is allowed.

Example:

```text
Current -> yes
Deprecated -> maybe
RetryOnly -> no
Removed -> no
```

---

# 31. Operation Retry Compatibility

Part 21 uses registry to determine old schema retry support.

---

# 32. Operation Schema Registry

Each schema version should have:

```text
introduced version
upcaster
compatibility status
payload digest semantics
```

---

# 33. Operation Schema Semantic Hash

Optional CI artifact can hash:

```text
field IDs
field types
requiredness
semantic annotations
```

to detect accidental changes.

---

# 34. Event Registry

Journal events also need stable identifiers.

Each event entry:

```text
EventKind
schema versions
visibility
public integration projection
audit relation
owner
```

---

# 35. Internal vs External Event

Registry marks:

```text
InternalOnly
TenantIntegration
PublicIntegration
```

---

# 36. Event Contract Stability

Internal events may evolve faster.

Public integration events require stronger compatibility guarantees.

---

# 37. Field Registry

For field-aware conflict/audit/provenance, fields need stable IDs.

Define:

```rust
pub struct FieldId(pub u32);
```

---

# 38. Field Identity

Field ID identifies semantic field, not Rust struct position.

---

# 39. Rename

Rust/display field name may change.

FieldId remains same if semantics unchanged.

---

# 40. Semantic Change

If meaning changes materially:

```text
new FieldId
```

or explicit schema migration.

---

# 41. Field Registry Entry

Contains:

```text
FieldId
entity type
canonical name
data class
audit policy
conflict policy
introduced schema
retired schema
```

---

# 42. Sensitive Field Metadata

Part 14/27 governance/security can read:

```text
PII class
retention class
audit redaction
encryption policy
```

from field registry.

---

# 43. Field Reuse Forbidden

Retired field ID never reused.

---

# 44. Capability Registry

Part 21:

```rust
pub struct CapabilityId(pub u32);
```

Entry includes:

```text
name
requirement kind
fallback
introduced protocol
status
```

---

# 45. Capability Requirement

```text
OptionalOptimization
OptionalWithFallback
RequiredForSafety
RequiredForSemantics
```

---

# 46. Capability Governance

Changing a capability from optional to required is a compatibility-policy change.

---

# 47. Consistency Profile Registry

Part 11 built-ins should have stable IDs.

Example:

```text
ImmutableAppendOnly
OptimisticVersioned
Commutative
ManualConflict
StrongAggregate
```

---

# 48. Profile Version

Profile ID identifies profile family.

ProfileVersion identifies semantics revision.

---

# 49. Profile Change

Changing:

```text
LWW
to
RejectStale
```

must be versioned as semantic change.

---

# 50. Error Code Registry

Stable machine-readable errors.

Define:

```rust
pub struct ErrorCode(pub u32);
```

---

# 51. Error Entry

Contains:

```text
code
canonical name
retry class
HTTP mapping
user-safe category
introduced version
```

---

# 52. Error Stability

Human message may change.

ErrorCode meaning must not.

---

# 53. Retry Classification

Registry may say:

```text
Retryable
NonRetryable
Reauthenticate
Rebootstrap
ManualIntervention
```

---

# 54. Job Kind Registry

Part 23 durable jobs need stable kinds.

Entry:

```text
JobKind
payload schema
retry policy class
epoch recovery policy
concurrency class
owner
```

---

# 55. Consumer Kind Registry

Part 28.

Entry:

```text
ConsumerKind
default ordering
default retention
visibility
owner
```

---

# 56. Audit Action Registry

Part 13.

Stable:

```text
AuditActionId
ReasonCode
DecisionRuleId
```

---

# 57. Audit Action Stability

Audit history may be retained for years.

Never reinterpret old action IDs.

---

# 58. Reason Code Registry

Useful for:

```text
admin actions
domain decisions
governance
authority transitions
```

---

# 59. Decision Rule Registry

Part 13/12 explainability.

Each business decision rule can have stable ID.

---

# 60. Migration Registry

Part 22 metadata migrations and domain migrations need stable IDs.

---

# 61. MetadataMigrationId

Monotonic.

Never edit a migration after release.

---

# 62. Migration Checksum

Registry stores checksum.

CI/runtime rejects:

```text
same migration ID, different body
```

---

# 63. Domain Migration Registry

Separate from metadata migration.

---

# 64. Protocol Registry

Part 21.

Contains:

```text
protocol versions
support status
message kinds
introduced/deprecated
```

---

# 65. Message Kind Registry

Define:

```rust
pub struct MessageKind(pub u16);
```

Stable ID.

---

# 66. Snapshot Format Registry

Could track:

```text
SnapshotSchemaVersion
ChunkFormatVersion
```

---

# 67. Artifact Format Registry

For:

```text
incident bundles
exports
replay bundles
migration bundles
```

---

# 68. Registry Generation

A canonical registry set can have:

```rust
pub struct RegistryGeneration(u64);
```

---

# 69. Registry Digest

Compute canonical digest over all registry files.

---

# 70. Build Embed

Embed:

```text
RegistryGeneration
RegistryDigest
```

into binary.

---

# 71. Runtime Diagnostics

Part 25 can report:

```text
registry generation
digest
```

---

# 72. Compatibility Check

Client/server registry mismatch is not automatically incompatible.

Negotiation uses relevant versions/capabilities.

Registry digest mainly helps diagnostics/build provenance.

---

# 73. Code Generation

Recommended:

```text
RON registry
↓
build-time generator
↓
typed Rust constants
↓
lookup tables
↓
docs
```

---

# 74. Generated Rust

Example:

```rust
pub const STUDENT: EntityType = EntityType(100);
pub const UPDATE_STUDENT: OperationKind = OperationKind(1002);
```

---

# 75. Generated Lookup

```rust
pub fn operation_descriptor(
    kind: OperationKind,
) -> Option<&'static OperationDescriptor>;
```

---

# 76. Generated Docs

Produce:

```text
Markdown
JSON schema for external tooling if needed
```

---

# 77. Generated Compatibility Matrix

From registry metadata.

---

# 78. Build Failure

If registry invalid:

```text
build fails
```

---

# 79. CI Validation

Checks:

```text
duplicate IDs
ID reuse
missing owner
invalid range
status regression
schema version regression
missing migration
```

---

# 80. Historical Registry Snapshot

Keep previous registry versions in Git history.

No need to copy all into source tree unless tooling requires.

---

# 81. Registry Lockfile

Possible generated:

```text
registry.lock
```

containing immutable published IDs/digests.

---

# 82. Purpose of Registry Lockfile

CI catches accidental deletion/reuse even if Git history not inspected.

---

# 83. Registry Lock Entry

Example:

```text
kind
ID
first semantic digest
status
```

---

# 84. Published Semantic Digest

Once published, semantic digest acts as guard.

---

# 85. Semantic Digest Caveat

Text description changes should not trigger breaking alarm.

Digest only stable semantic fields.

---

# 86. Schema DSL

Do not invent huge custom schema language initially.

Use:

```text
typed Rust definitions
+
RON metadata
+
tests
```

---

# 87. Derive Macros

Potential:

```rust
#[derive(AequoraOperation)]
#[aequora(kind = 1002, schema = 3)]
struct UpdateStudent { ... }
```

---

# 88. Macro Validation

Macro-generated metadata must match registry.

---

# 89. Registry Is Canonical

Code annotations cannot silently create new durable IDs outside registry.

---

# 90. Compile-Time Check

Build script/macros compare:

```text
Rust type
registry entry
```

---

# 91. Schema Field IDs

Could annotate:

```rust
#[aequora(field_id = 17)]
phone: PhoneNumber
```

---

# 92. Field Type Change

CI detects:

```text
FieldId 17
String -> Money
```

as breaking unless migration/version updated.

---

# 93. Requiredness Change

Optional -> required is breaking for old data/messages.

Requires migration/new schema version.

---

# 94. Numeric Unit Change

Requires semantic version bump.

---

# 95. Enum Variant Registry

Durable domain enums may need stable variant IDs.

---

# 96. DurableEnum

Example:

```text
StudentStatus:
    Active = 1
    Suspended = 2
    Archived = 3
```

---

# 97. Unknown Variant Policy

Registry marks whether unknown is:

```text
safe-display
must-reject
```

---

# 98. Security Enum

Authorization/workflow states usually:

```text
must-reject
```

---

# 99. Compatibility Governance

Every registry change classified:

```text
NonBreaking
Additive
Deprecated
BreakingWithMigration
SecurityRequired
```

---

# 100. ChangeClassification

```rust
pub enum ChangeClassification {
    NonBreaking,
    Additive,
    Deprecated,
    BreakingWithMigration,
    SecurityRequired,
}
```

---

# 101. Change Proposal Requirement

Breaking/security-sensitive registry changes require design/change proposal.

---

# 102. Proposal Content

Include:

```text
motivation
old semantics
new semantics
migration
compatibility impact
offline-client impact
rollback
security impact
test plan
```

---

# 103. RegistryChangeId

```rust
pub struct RegistryChangeId(Uuid);
```

---

# 104. Link Registry Entry to Proposal

Optional metadata:

```text
change_proposal_ref
```

---

# 105. Reviewers

Different changes require different reviewers.

---

# 106. Operation Change Review

Needs:

```text
domain owner
sync compatibility reviewer
```

---

# 107. Security Capability Review

Needs:

```text
security reviewer
```

---

# 108. Crypto Registry Review

Requires security/crypto owner.

---

# 109. Audit/Governance Review

Requires governance owner.

---

# 110. Migration Review

Requires storage/schema owner.

---

# 111. Release Governance

Registry changes included in release manifest.

---

# 112. Release Manifest

Contains:

```text
new IDs
deprecated IDs
schema bumps
required capability changes
migration IDs
```

---

# 113. Changelog Generation

Can generate developer-facing changelog section.

---

# 114. Client Upgrade Notes

From registry change classification.

---

# 115. Server Upgrade Notes

Same.

---

# 116. Deprecation Lifecycle

Recommended:

```text
Current
↓
Supported
↓
Deprecated
↓
RetryOnly
↓
Reserved/Removed
```

---

# 117. Deprecation Metadata

Include:

```text
deprecated_since
replacement ID
sunset policy
```

---

# 118. Replacement Link

Example:

```text
OldOperationKind -> NewOperationKind
```

---

# 119. Do Not Auto-Translate Semantics Without Upcaster

Replacement link is documentation, not execution logic.

---

# 120. Registry Drift

Multiple crates should not maintain duplicate hand-written ID lists.

---

# 121. Single Generated Crate

Recommended:

```text
aequora-registry-generated
```

---

# 122. Generated Crate Contents

```text
IDs
descriptors
lookup tables
schema metadata
```

---

# 123. Core Crates Depend On Generated Registry

Avoid circular dependency by careful layering.

---

# 124. Suggested Layering

```text
aequora-registry-types
        │
        ▼
registry files + codegen
        │
        ▼
aequora-registry-generated
        │
        ▼
protocol/domain/jobs/etc.
```

---

# 125. Registry Types Crate

Contains only:

```text
newtypes
status enums
descriptor structs
```

---

# 126. Domain-App Extension

Applications need their own operation/entity IDs.

---

# 127. Application Namespace

Each application can declare:

```text
namespace ID
```

or use reserved app range.

---

# 128. Library Core IDs

Aequora core should not allocate app business IDs.

---

# 129. App Registry Directory

Example:

```text
app-registry/
├── entities.ron
├── operations.ron
├── fields.ron
└── audit-actions.ron
```

---

# 130. Merge Registry

Build combines:

```text
Aequora core registry
+
application registry
+
approved extensions
```

---

# 131. Collision Check

CI fails on collision.

---

# 132. Third-Party Extension Registry

Future ecosystem may publish signed registry package.

---

# 133. Extension Manifest

Contains:

```text
namespace
version
IDs
required Aequora protocol
capabilities
```

---

# 134. Dynamic Loading

Not required.

Compile-time integration preferred initially.

---

# 135. Registry Package Signature

Future high-assurance ecosystem can sign extension manifests.

---

# 136. Schema Compatibility Tool

CLI:

```text
aequora registry diff old.ron new.ron
```

---

# 137. Diff Output

Example:

```text
ADDED OperationKind 1042 CreateGuardian
DEPRECATED OperationKind 1008 SetPhoneLegacy
BREAKING FieldId 17 type changed
ERROR ID 1005 reused
```

---

# 138. Registry Check CLI

```text
aequora registry verify
```

---

# 139. Registry Explain CLI

```text
aequora registry explain operation 1002
```

---

# 140. Registry Reserve CLI

Could allocate next valid ID:

```text
aequora registry reserve operation
```

---

# 141. Reservation Workflow

Avoid two developers selecting same ID concurrently.

---

# 142. Reserved But Unpublished

Registry status:

```text
Reserved
```

---

# 143. Reservation Expiry

Optional.

If never published, ID could potentially return to pool before release.

Once published, never reused.

---

# 144. Published Definition

A registry entry is published when:

```text
included in released artifact
or durable production data
```

---

# 145. CI Published Set

Release pipeline writes registry lockfile/tag.

---

# 146. Branch Coordination

In monorepo, merge conflicts in registry file are desirable.

They force ID collision resolution.

---

# 147. Generated Files

Do not manually edit.

---

# 148. CI Ensures Clean Generation

Run codegen and fail if generated output differs.

---

# 149. Schema Documentation

Each entry should include concise semantics.

---

# 150. Semantics Description

Example:

```text
"Sets the student's primary phone number after authorization and phone validation."
```

---

# 151. Avoid Vague Names

Bad:

```text
UpdateData
Event1
```

Good:

```text
SetStudentPrimaryPhone
StudentPrimaryPhoneChanged
```

---

# 152. Command/Event Distinction

OperationKind names use intent.

EventKind names use fact/past tense.

---

# 153. Examples

```text
Operation:
    ApproveLeaveRequest

Event:
    LeaveRequestApproved
```

---

# 154. Registry Naming Convention

Enforce consistent casing and uniqueness.

---

# 155. Domain Prefix

Not always needed if entity association explicit.

---

# 156. Error Naming

Use stable uppercase or CamelCase canonical.

---

# 157. Registry Search Tool

CLI can search by:

```text
name
ID
owner
status
```

---

# 158. Documentation Site

Future generated registry docs can serve:

```text
developer portal
```

---

# 159. Schema Explorer

Optional UI showing:

```text
entities
operations
events
fields
relationships
```

---

# 160. Developer Experience

Registry should make safe changes easier than unsafe changes.

---

# 161. New Operation Workflow

Recommended:

```text
1. Reserve OperationKind.
2. Add operation registry entry.
3. Define payload type.
4. Define field/schema IDs.
5. Register handler/profile.
6. Add auth policy.
7. Add tests.
8. Generate docs.
9. CI verifies compatibility.
```

---

# 162. New Event Workflow

```text
1. Reserve EventKind.
2. Define schema.
3. Define visibility.
4. Add projector/upcaster if needed.
5. Add golden fixture.
```

---

# 163. New Field Workflow

```text
1. Reserve FieldId.
2. Classify data sensitivity.
3. Define conflict/audit policy.
4. Add migration if existing entity.
```

---

# 164. Schema Change Workflow

For additive optional field:

```text
schema bump if wire format requires
compatibility review
golden fixture update
```

---

# 165. Breaking Schema Change

Requires:

```text
new schema version
upcaster/migration
compatibility window
```

---

# 166. Operation Removal

Never simply delete registry entry.

Mark:

```text
Deprecated
then RetryOnly
then Removed/Reserved
```

---

# 167. Handler Removal

Cannot remove while old retries/replay need it.

---

# 168. Replay Governance

Part 12 historical handler support policy references registry.

---

# 169. Audit Registry Governance

Audit action IDs likely retained indefinitely.

Treat removal as:

```text
retired, not erased
```

---

# 170. Error Code Governance

Error code can become deprecated, but old code remains meaningful for logs/bundles.

---

# 171. Capability Removal

Old CapabilityId remains reserved.

---

# 172. Protocol Message Removal

Same.

---

# 173. Migration IDs

Never reused even if rollback removes migration effect.

---

# 174. Registry Compatibility Report

Generated per release:

```text
New
Deprecated
Removed
Breaking
Security Required
```

---

# 175. CI Modes

Recommended:

```text
registry lint
registry diff
registry compatibility
registry codegen
registry golden
```

---

# 176. Registry Lint

Checks:

```text
name format
owner present
status valid
description present
range valid
```

---

# 177. Registry Diff

Compare against main/release baseline.

---

# 178. Compatibility Gate

Breaking change requires approved metadata:

```text
change proposal ID
migration/upcaster reference
```

---

# 179. Golden Tests

For protocol/operation/event schema.

---

# 180. Snapshot Schema Registry

Part 10 snapshot versions listed.

---

# 181. Incident Bundle Schema Registry

Part 25 bundle version listed.

---

# 182. Archive Schema Registry

Part 28 feed archive versions listed.

---

# 183. Security Registry

Security event kinds/error codes may have stable registry too.

---

# 184. Admin Action Registry

Part 24 admin action kinds/permissions.

---

# 185. Permission Registry

Stable PermissionId.

---

# 186. PermissionId

```rust
pub struct PermissionId(pub u32);
```

---

# 187. Permission Semantics

Never reuse permission ID for broader/different access.

---

# 188. Permission Split

If one permission becomes too broad:

```text
create new permissions
```

do not silently redefine old one.

---

# 189. Role Registry

Roles are often application policy, not protocol.

Could still be versioned but need not be global core registry.

---

# 190. ReasonCode Registry

Part 13/24.

---

# 191. Governance RetentionClass Registry

Part 14.

Stable retention class IDs.

---

# 192. Crypto Purpose Registry

Part 15 stable key purposes.

---

# 193. Algorithm Registry

Use explicit algorithm IDs.

Do not rely on library names.

---

# 194. Algorithm Retirement

Mark forbidden/deprecated via crypto policy.

ID still reserved.

---

# 195. Registry Security

Registry files themselves are source code/config.

Protect repository branch.

---

# 196. Signed Release Registry

Optional:

```text
registry digest signed in release manifest
```

---

# 197. Runtime Registry Mutation

Not allowed for core semantic IDs.

Dynamic business configuration is separate.

---

# 198. User-Defined Fields

Some applications may need custom fields.

Do not inject them into global core FieldId space dynamically.

---

# 199. Custom Field Namespace

Use:

```text
tenant/app custom schema registry
```

with separate semantics.

---

# 200. Dynamic Custom Field

Can have:

```rust
CustomFieldId(Uuid);
```

instead of global u32.

---

# 201. Why

Dynamic tenant-defined schema has different lifecycle from compiled protocol schema.

---

# 202. Custom Field Sync

Represent through generic typed custom-field operation profile.

---

# 203. Registry Boundary

Compiled registry:

```text
engine/application semantic contracts
```

Dynamic registry:

```text
tenant-configured business data
```

Do not mix.

---

# 204. Plugin Registry

Future plugins may contribute namespaced IDs.

---

# 205. Compatibility Contract

Plugin manifest declares:

```text
required core registry generation
protocol versions
namespace
```

---

# 206. Registry Conflict

Build/install rejects duplicate namespace/ID.

---

# 207. Multi-Repo Ecosystem

If extensions live in different repos, central namespace allocation service/file needed.

---

# 208. Initial Recommendation

Keep ecosystem monorepo/application-local until external extension demand exists.

---

# 209. Developer Governance Levels

Classify changes:

```text
Level 0 — Internal implementation
Level 1 — Non-durable public API
Level 2 — Durable schema/registry
Level 3 — Security/authority/financial semantic contract
```

---

# 210. Level 0

Normal code review.

---

# 211. Level 1

API compatibility review.

---

# 212. Level 2

Registry + compatibility review.

---

# 213. Level 3

Requires:

```text
domain owner
security/correctness reviewer
migration/replay plan
```

---

# 214. Finance Operation

Usually Level 3.

---

# 215. Authority Epoch Format

Level 3.

---

# 216. UI-only View Rename

Level 0/1.

---

# 217. Change Proposal Template

Suggested:

```text
Title
Registry entries affected
Current semantics
Proposed semantics
Compatibility
Migration
Replay impact
Security impact
Governance impact
Rollback
Tests
```

---

# 218. ADR Link

Important accepted proposal can generate ADR.

---

# 219. Review Automation

CI can require CODEOWNERS for:

```text
registry/operations.ron
registry/crypto.ron
registry/permissions.ron
```

---

# 220. Git Governance

Protected branch.

No force-push on release registry history.

---

# 221. Release Tags

Registry state tied to release tag.

---

# 222. Registry Diff Across Release

CLI:

```text
aequora registry diff v1.4.0 v1.5.0
```

---

# 223. Runtime Self-Description

Server can expose authenticated diagnostics:

```text
registry generation
supported operation kinds
capabilities
```

---

# 224. Client Registry

Client binary knows subset/current registry.

Negotiation uses capabilities/versions, not giant registry exchange.

---

# 225. Unknown OperationKind

Server:

```text
reject UnsupportedOperationKind
```

---

# 226. Unknown EventKind

Client policy:

```text
ignore only if explicitly optional/skippable
otherwise upgrade required
```

---

# 227. Unknown FieldId

Audit/provenance tools can display:

```text
UnknownField(1234)
```

without pretending semantics.

---

# 228. Registry Documentation Retention

Even removed entries remain documented historically.

---

# 229. Incident Forensics

Part 25 can resolve old IDs using historical registry metadata.

---

# 230. Historical Registry Bundle

Support tooling may ship registry database covering all supported historical IDs.

---

# 231. Registry Archive

Generated static artifact:

```text
registry-history.postcard
```

optional.

---

# 232. Migration Provenance

Part 09/26 import manifests include:

```text
mapping version
registry generation
```

---

# 233. Snapshot Manifest

Part 10 includes relevant schema versions.

Registry helps tooling explain.

---

# 234. Feed Schema

Part 28 consumers use EventKind registry.

---

# 235. Job Schema

Part 23 workers use JobKind registry.

---

# 236. Audit Explainability

Part 13 uses AuditAction/ReasonCode/FieldId registry.

---

# 237. Security

Part 27 uses PermissionId/SecurityEventKind/ErrorCode registry.

---

# 238. Control Plane

Part 24 uses AdminActionKind/PermissionId/ReasonCode.

---

# 239. Registry Persistence

Do not need production DB copy for canonical definitions.

Can persist:

```text
active policy references
generation
```

only.

---

# 240. Registry Lookup Performance

Generated static arrays/maps.

No DB query in hot path.

---

# 241. Dense Range Optimization

For dense small IDs, array indexed by ID offset.

---

# 242. Sparse Range

Use perfect/static map.

---

# 243. Startup Validation

Application checks:

```text
all registered handlers have registry entry
all required entries have handler
all profile references resolve
all field IDs unique
```

---

# 244. Missing Handler

If current operation kind lacks handler:

```text
startup fails
```

---

# 245. Missing Old Handler

If RetryOnly operation expected but handler/upcaster missing:

```text
startup compatibility failure
```

---

# 246. Missing Consumer Projector

Only affects configured consumer.

Consumer fails to activate.

---

# 247. Registry Dependency Graph

Entries may reference:

```text
entity
profile
capability
permission
handler
```

Validate graph.

---

# 248. No Cyclic Semantic Dependency

Where cycles are invalid, CI rejects.

---

# 249. Documentation Drift

Generated docs prevent hand-maintained mismatch.

---

# 250. Registry Test Fixture

Every current operation/event should have at least one canonical fixture.

---

# 251. Fixture Metadata

```text
schema version
payload bytes
expected digest
```

---

# 252. Compatibility Fixture

Old versions retained while supported.

---

# 253. Fuzzing

Fuzz generated decoders/upcasters against registry bounds.

---

# 254. Registry Invariants

Add:

## AEQ-INV-REGISTRY001

```text
A published durable registry ID is never reused for different semantics.
```

## AEQ-INV-REGISTRY002

```text
Every persisted or wire-visible operation, event, capability, error, permission, and migration kind resolves to a canonical registry entry or is rejected as unknown.
```

## AEQ-INV-REGISTRY003

```text
A breaking schema or semantic change requires an explicit new version, migration/upcaster path, or declared incompatibility.
```

## AEQ-INV-REGISTRY004

```text
Generated runtime constants and lookup tables are derived from the canonical registry source and cannot silently diverge from it.
```

## AEQ-INV-REGISTRY005

```text
Removed or deprecated IDs remain reserved and historically interpretable.
```

## AEQ-INV-REGISTRY006

```text
Third-party/application extensions cannot collide with Aequora core or another registered extension namespace.
```

---

# 255. Additional Invariants

## AEQ-INV-REGISTRY007

```text
Security-sensitive capability, permission, crypto-purpose, and admin-action changes require explicit compatibility/security review.
```

## AEQ-INV-REGISTRY008

```text
Runtime dynamic configuration cannot redefine compiled durable registry semantics.
```

## AEQ-INV-REGISTRY009

```text
Historical replay, audit, and incident tooling can resolve every retained historical durable ID used by supported data.
```

---

# 256. Tests — Duplicate ID

Add two OperationKind entries with same ID.

Expected:

```text
CI/build failure
```

---

# 257. Test — Reuse Removed ID

Old ID reserved.

New semantic entry attempts reuse.

Expected:

```text
failure
```

---

# 258. Test — Missing Owner

Registry lint fails.

---

# 259. Test — Schema Version Regression

Current 4 → proposed 3.

Expected:

```text
failure
```

---

# 260. Test — Breaking Field Change

FieldId same, type incompatible, no migration/version bump.

Expected:

```text
compatibility failure
```

---

# 261. Test — RetryOnly Operation

Registry says RetryOnly.

New creation:

```text
rejected
```

historical retry:

```text
accepted if supported
```

---

# 262. Test — Capability Required

Registry marks RequiredForSafety.

Compatibility policy tries fallback.

Expected:

```text
validation failure
```

---

# 263. Test — Extension Collision

App extension ID collides with core.

Expected:

```text
build failure
```

---

# 264. Test — Codegen Drift

Generated file manually edited.

CI regeneration detects diff.

---

# 265. Test — Historical Resolution

Old audit event references retired AuditActionId.

Tool still resolves description.

---

# 266. Test — Unknown ID

Unknown current OperationKind.

Server rejects with stable unsupported error.

---

# 267. Registry CLI

Suggested:

```text
aequora registry verify
aequora registry lint
aequora registry diff
aequora registry explain
aequora registry reserve
aequora registry docs
aequora registry compatibility
```

---

# 268. Developer Workflow Example

Adding new finance operation:

```text
reserve OperationKind
↓
create registry entry
↓
define payload/schema
↓
assign consistency profile
↓
assign permissions
↓
add handler
↓
add audit action
↓
add fixtures
↓
compatibility check
↓
security/domain review
↓
release
```

---

# 269. CI Pipeline

Recommended:

```text
registry lint
↓
registry lock verification
↓
codegen
↓
compile
↓
golden fixtures
↓
compatibility diff
↓
cross-version tests
↓
docs generation
```

---

# 270. Repository Layout

```text
registry/
├── core/
│   ├── capabilities.ron
│   ├── errors.ron
│   ├── protocol.ron
│   └── admin-actions.ron
├── app/
│   ├── entities.ron
│   ├── fields.ron
│   ├── operations.ron
│   ├── events.ron
│   └── audit-actions.ron
├── extensions/
└── registry.lock
```

---

# 271. Crates

Suggested:

```text
aequora-registry-types/
aequora-registry-codegen/
aequora-registry-generated/
aequora-registry-cli/
```

---

# 272. `aequora-registry-types`

Contains:

```text
ID newtypes
descriptor structs
status enums
change classification
```

---

# 273. `aequora-registry-codegen`

Reads RON and generates:

```text
Rust
docs
lockfile checks
```

---

# 274. `aequora-registry-generated`

Compiled immutable runtime registry.

---

# 275. `aequora-registry-cli`

Developer/operator tooling.

---

# 276. No Runtime Parser Requirement

Production binary need not parse RON registry at startup.

Compile generated registry.

---

# 277. Build-Time Validation

Ensures zero runtime startup cost for parsing.

---

# 278. Registry Documentation Bundle

Publish alongside crate docs.

---

# 279. Ecosystem Governance

If Aequora becomes public ecosystem:

```text
namespace request
extension review
compatibility policy
reserved ranges
```

can be formalized later.

---

# 280. Initial Open-Source Governance

Simple:

```text
PR
CODEOWNERS
CI
maintainer approval
```

is enough.

---

# 281. Avoid Bureaucracy Too Early

Governance should prevent dangerous semantic drift without making every internal refactor difficult.

---

# 282. Rule of Thumb

Ask:

```text
Can this value survive a release boundary?
```

If yes:

```text
govern it
```

If no:

```text
normal code refactor rules
```

---

# 283. Completion Criteria

Part 29 is complete when:

```text
[ ] durable registry domains enumerated
[ ] stable numeric ID policy defined
[ ] non-reuse/reservation rules defined
[ ] canonical RON registry source defined
[ ] ownership/status metadata defined
[ ] operation/event/entity/field registries defined
[ ] capability/error/job/consumer/audit registries defined
[ ] migration/protocol registry defined
[ ] core/app/extension namespaces defined
[ ] code generation architecture defined
[ ] registry lock/history defined
[ ] compatibility diff/gates defined
[ ] change proposal/review levels defined
[ ] developer workflow defined
[ ] CI/tooling defined
[ ] historical resolution defined
[ ] registry correctness invariants added
```

---

# 284. Final Architecture

```text
                    CANONICAL REGISTRY SOURCES
                              RON
                               │
              ┌────────────────┼────────────────┐
              ▼                ▼                ▼
         Operations         Events          Fields
              │                │                │
              ▼                ▼                ▼
        Capabilities       Errors          Profiles
              │                │                │
              └────────────────┼────────────────┘
                               ▼
                       Registry Validator
                               │
                    collision / range / diff
                               │
                               ▼
                         Code Generator
                               │
              ┌────────────────┼────────────────┐
              ▼                ▼                ▼
         Rust Constants    Lookup Tables     Documentation
              │                │                │
              └────────────────┼────────────────┘
                               ▼
                       Runtime Components

Developer change:

reserve ID
   ↓
registry entry
   ↓
schema/handler
   ↓
compatibility diff
   ↓
review/change proposal
   ↓
CI/golden tests
   ↓
release
   ↓
ID becomes permanently governed
```

The architectural principle is:

> **Aequora should treat durable identifiers and schemas like a protocol ABI: explicit, versioned, reviewable, non-reusable, and historically interpretable.**

This prevents a large class of long-term synchronization failures where the software still compiles but old journal entries, snapshots, operations, audit records, jobs, or integrations no longer mean what the new code thinks they mean.
