# Aequora Sync — Part 35

# Public Rust API, SDK Stability, Ergonomics, Extension Points, and Semantic Versioning Architecture

## 1. Purpose

Part 34 defined the reference implementation workspace and crate boundaries.

The next problem is different:

> **What should application developers actually see when they use Aequora as a Rust library?**

A powerful internal architecture can still fail as a reusable library if the public API is:

```text
too low-level
too database-specific
too coupled to internal crates
too unstable
too verbose
too easy to misuse
```

Aequora's public Rust SDK should make the safe path the easy path.

The central rule is:

> **Public APIs should expose synchronization intent and stable domain concepts, not internal machinery.**

---

# 2. Goals

The Rust SDK should provide:

```text
clear entry points
typed configuration
ergonomic builders
stable domain-facing APIs
safe async behavior
typed errors
structured status/events
controlled extension points
stable semver policy
minimal accidental coupling
good documentation
```

---

# 3. Non-Goals

The public SDK should not expose:

```text
raw journal mutation
raw cursor mutation
internal DB transaction structs
Axum extractors
SQLx types
Stoolap internals
Tokio task topology
internal scheduler queues
```

---

# 4. Public SDK Layers

Recommended public surfaces:

```text
aequora
aequora-client
aequora-server
aequora-types
aequora-operation
aequora-adapter-sdk
```

Applications should normally start from:

```text
aequora
```

or the role-specific crate.

---

# 5. Facade Crate

Create a convenience crate:

```text
aequora
```

that re-exports the stable common API.

Example:

```rust
use aequora::prelude::*;
```

But keep the prelude small.

---

# 6. What the Facade Re-Exports

Likely:

```text
AequoraClient
AequoraClientBuilder
Operation
OperationId
EntityId
SyncStatus
SyncEvent
ConflictId
ScopeId
AequoraError
```

---

# 7. What the Facade Should Not Re-Export

Do not re-export:

```text
SQLx
Axum
Stoolap
internal metadata models
internal ledger records
```

---

# 8. Client API Mental Model

A developer should think in five verbs:

```text
open
mutate
query
sync
observe
```

Advanced features can be added later.

---

# 9. Minimal Client API

Concept:

```rust
let client = AequoraClient::builder()
    .store(store)
    .transport(transport)
    .identity(identity)
    .build()
    .await?;
```

Then:

```rust
client.mutate(op).await?;
client.sync_now().await?;
client.status().await?;
```

---

# 10. Builder Pattern

Builders are appropriate for configuration-heavy construction.

They should:

```text
validate required fields
produce actionable errors
avoid giant constructors
```

---

# 11. Required vs Optional Builder Fields

Required:

```text
local store
transport
identity/auth provider
registry/domain
```

Optional:

```text
scheduler policy
logging hooks
resource limits
diagnostics
```

---

# 12. Typestate Builder?

Use only if it materially improves compile-time safety.

Example:

```rust
AequoraClientBuilder<MissingStore, MissingTransport>
```

can become too noisy.

Prefer runtime validation for configuration unless missing state is truly dangerous.

---

# 13. Strong Types

Public SDK should use:

```rust
OperationId
ScopeId
TenantId
DeviceId
```

rather than generic strings.

---

# 14. Public IDs

IDs should be:

```text
Copy/Clone where cheap
Eq
Hash
Display
FromStr
serde-compatible where needed
```

---

# 15. Avoid Leaking UUID Implementation

Even if internal representation is UUIDv7, consumers should depend on:

```text
OperationId
```

not necessarily `uuid::Uuid`.

---

# 16. Domain Operation API

Recommended pattern:

```rust
pub trait Operation {
    type Outcome;

    const KIND: OperationKind;
    const SCHEMA_VERSION: OperationSchemaVersion;
}
```

Application operation:

```rust
#[derive(...)]
pub struct CreateStudent {
    pub student_id: StudentId,
    pub name: StudentName,
}
```

---

# 17. Operation Submission

Developer-facing:

```rust
let receipt = client.mutate(CreateStudent { ... }).await?;
```

---

# 18. Mutation Receipt

Return more than just `OperationId`.

Example:

```rust
pub struct MutationReceipt {
    pub operation_id: OperationId,
    pub local_status: LocalCommitStatus,
}
```

---

# 19. Local Status

Possible:

```rust
pub enum LocalCommitStatus {
    SavedLocally,
    Superseded,
}
```

Do not claim server confirmation at local commit time.

---

# 20. Server Confirmation

Separate status/event:

```text
AuthoritativeAccepted
AuthoritativeRejected
Conflict
```

---

# 21. Awaiting Confirmation

Optional API:

```rust
client
    .operation(receipt.operation_id)
    .await_authoritative()
    .await?;
```

---

# 22. Why This Matters

Some applications need:

```text
save immediately
```

others need:

```text
wait until server confirms
```

Aequora should support both without conflating them.

---

# 23. Query API

Aequora should not become a universal ORM.

Prefer either:

```text
application repositories
```

or a small query facade over supported local adapters.

---

# 24. Recommended Public Query Boundary

Application-specific repository:

```rust
let students = app.students().by_class(class_id).await?;
```

This keeps domain query semantics outside generic sync core.

---

# 25. Generic Entity Access

A small low-level read API may still exist for tooling.

But normal app SDK should be domain-friendly.

---

# 26. Sync API

Provide:

```rust
client.sync_now().await?
```

and optionally:

```rust
client.request_sync(SyncReason::UserInitiated);
```

---

# 27. Distinguish Request vs Completion

`request_sync()`:

```text
scheduler hint
```

`sync_now().await`:

```text
run a bounded sync attempt and await result
```

---

# 28. Sync Result

Example:

```rust
pub struct SyncResult {
    pub pushed: u64,
    pub pulled: u64,
    pub conflicts: u64,
    pub next_action: SyncNextAction,
}
```

---

# 29. SyncNextAction

Possible:

```rust
pub enum SyncNextAction {
    Idle,
    RetryAfter(Duration),
    NeedsBootstrap,
    UpgradeRequired,
    AuthenticationRequired,
}
```

---

# 30. Status API

Expose stable high-level state:

```rust
pub enum SyncStatus {
    Offline,
    Idle,
    Pending { operations: u64 },
    Syncing,
    Conflict,
    NeedsBootstrap,
    AuthenticationRequired,
    UpgradeRequired,
    StorageBlocked,
}
```

---

# 31. Avoid Internal State Leakage

Do not expose:

```text
PlannerPhase3
PullBatchApplying
JournalTxnOpen
```

as public status.

---

# 32. Event API

Provide structured events.

```rust
pub enum SyncEvent {
    StatusChanged(SyncStatus),
    DataChanged(DataChange),
    ConflictCreated(ConflictId),
    OperationUpdated(OperationUpdate),
    BootstrapProgress(BootstrapProgress),
}
```

---

# 33. Event Stream

Rust-native API:

```rust
let mut events = client.events();
```

using:

```text
Stream
broadcast channel wrapper
```

implementation remains internal.

---

# 34. Event Delivery Semantics

Document whether events are:

```text
best-effort notification
durable replayable event
```

UI events are normally best-effort hints.

Durable state remains queryable.

---

# 35. Do Not Make UI Depend on Event Delivery

If event missed:

```text
query current state
```

must still work.

---

# 36. Conflict API

Expose:

```rust
client.conflicts().list().await?;
client.conflicts().get(id).await?;
client.conflicts().resolve(id, resolution).await?;
```

---

# 37. Conflict Model

Public model should include:

```text
conflict ID
entity reference
reason
local intent summary
authoritative state summary
available resolution options
```

without leaking storage internals.

---

# 38. Scope API

Advanced:

```rust
client.scopes().subscribe(scope).await?;
client.scopes().unsubscribe(scope_id).await?;
```

---

# 39. Scope Subscription Result

Should distinguish:

```text
requested
bootstrapping
active
```

---

# 40. Blob API

High-level:

```rust
client.blobs().import(path).await?;
client.blobs().ensure_local(blob_ref).await?;
```

---

# 41. Diagnostics API

Provide safe bounded diagnostics:

```rust
client.diagnostics().summary().await?;
```

and explicit support-bundle generation.

---

# 42. Client Handle Semantics

`AequoraClient` should likely be cheaply clonable via internal `Arc`.

---

# 43. Clone Meaning

Cloning the handle should not create another sync engine.

It creates another reference to the same runtime.

---

# 44. Send + Sync

Where platform supports it, client handle should be:

```text
Send + Sync
```

unless platform-specific constraints require otherwise.

---

# 45. Drop Semantics

Dropping one handle does not necessarily shut down client if others exist.

---

# 46. Explicit Shutdown

Provide:

```rust
client.shutdown().await?;
```

for graceful application termination.

---

# 47. Forced Termination

Correctness must not depend on graceful shutdown.

---

# 48. Client Lifecycle

Possible:

```text
open
running
shutdown requested
closed
```

---

# 49. Operation Inspection

Provide:

```rust
client.operations().get(operation_id).await?;
```

---

# 50. Operation State

Public:

```rust
pub enum OperationState {
    Pending,
    InFlight,
    AuthoritativeAccepted,
    Rejected,
    Conflict,
    Superseded,
}
```

---

# 51. Retry API

Ordinary users should not manually retry raw operations.

Scheduler handles retries.

Possible explicit:

```rust
client.request_sync(...)
```

is enough.

---

# 52. Cancel API

Cancellation of user intent must be semantic.

Do not expose:

```text
delete outbox row
```

Instead:

```rust
client.operations().cancel(operation_id).await?
```

only if operation profile supports it.

---

# 53. Error Architecture

Use one top-level:

```rust
AequoraError
```

with stable categories.

---

# 54. Error Categories

Example:

```rust
pub enum AequoraError {
    Storage(StorageError),
    Transport(TransportError),
    Authentication(AuthError),
    Authorization(AuthorizationError),
    Conflict(ConflictError),
    Compatibility(CompatibilityError),
    Validation(ValidationError),
    Resource(ResourceError),
    Internal(InternalError),
}
```

---

# 55. Error Stability

Public error categories should be semver-stable.

Internal source details can evolve.

---

# 56. Non-Exhaustive Enums

Use:

```rust
#[non_exhaustive]
```

on public enums expected to gain variants.

---

# 57. Pattern Matching

Consumers should use catch-all arm when enum is non-exhaustive.

---

# 58. Stable Error Codes

Expose:

```text
ErrorCode
```

from Part 29.

This is useful across:

```text
logs
FFI
support
API
```

---

# 59. Source Errors

Implement:

```rust
std::error::Error::source()
```

internally where useful.

Do not require callers to downcast adapter-specific errors for normal logic.

---

# 60. Retryability

Expose:

```rust
error.retry_class()
```

or structured metadata.

---

# 61. Public Error Message

`Display` should be useful for developers.

User-facing text belongs to application UI.

---

# 62. Configuration API

Configuration should be split:

```text
programmatic builder
RON config
environment/secrets integration
```

---

# 63. Programmatic Config

Strongly typed.

---

# 64. Config File

RON can deserialize into stable config model.

---

# 65. Secrets

Never require storing tokens directly in RON.

Use provider/secret reference.

---

# 66. API Defaults

Safe defaults.

Example:

```text
bounded batch
HTTPS transport
no aggressive background work
```

---

# 67. Explicit Dangerous Options

Dangerous behavior should require explicit opt-in and naming.

Bad:

```rust
.allow(true)
```

Better:

```rust
.allow_insecure_dev_transport(true)
```

for development-only cases.

---

# 68. Extension Points

Public SDK should allow extension in controlled categories:

```text
storage adapters
transport adapters
credential providers
domain handlers
platform providers
observability sinks
```

---

# 69. Closed Core Semantics

Do not allow extension to redefine:

```text
OperationId semantics
cursor ordering
ledger idempotency
authority epoch
```

These are core invariants.

---

# 70. Adapter SDK

Third-party storage adapters should implement dedicated traits from:

```text
aequora-adapter-sdk
```

not internal storage modules.

---

# 71. Transport Trait

Example:

```rust
#[async_trait]
pub trait SyncTransport: Send + Sync {
    async fn exchange(
        &self,
        request: ExchangeRequest,
    ) -> Result<ExchangeResponse, TransportError>;
}
```

---

# 72. Credential Provider

```rust
#[async_trait]
pub trait CredentialProvider: Send + Sync {
    async fn credential(
        &self,
    ) -> Result<Credential, AuthError>;
}
```

---

# 73. Clock Provider

For deterministic tests:

```rust
pub trait Clock: Send + Sync {
    fn now(&self) -> InstantLike;
}
```

Production users should rarely need to provide one.

---

# 74. Randomness Provider

If deterministic execution needs controlled randomness, keep it inside execution context rather than global public SDK surface.

---

# 75. Hooks vs Plugins

Prefer typed hooks:

```text
on_status
on_diagnostic
on_metric
```

over arbitrary plugin execution.

---

# 76. Hook Failure Semantics

Observability hook failure must not break sync correctness.

---

# 77. Public Server API

Server builders should be similarly ergonomic.

Example:

```rust
let server = AequoraServer::builder()
    .authority_store(store)
    .registry(registry)
    .authenticator(auth)
    .build()?;
```

---

# 78. Axum Integration

Separate:

```rust
let router = aequora_axum::router(server.clone());
```

---

# 79. Server Core Can Be Used Without Axum

Useful for:

```text
tests
IPC
future QUIC
embedded server
```

---

# 80. Domain Handler Registration

Public API should support explicit registration.

Example:

```rust
let registry = DomainRegistry::builder()
    .register::<CreateStudent>(handler)
    .register::<UpdateStudent>(handler2)
    .build()?;
```

---

# 81. Duplicate Operation Registration

Builder should fail:

```text
duplicate kind
duplicate schema owner
missing profile
```

before runtime traffic.

---

# 82. Handler Ergonomics

Avoid forcing handlers to understand:

```text
wire framing
ledger internals
HTTP headers
```

They should receive validated context.

---

# 83. Execution Context

Public domain handler context may expose:

```text
actor
tenant
device
server time
operation ID
correlation
```

---

# 84. Context Stability

Fields likely to evolve should be accessed through methods where useful rather than exposing huge public struct fields.

---

# 85. Request Extensions

If custom metadata is needed, define typed extension mechanism carefully.

Avoid arbitrary `HashMap<String, Value>` everywhere.

---

# 86. Public Metadata

Use explicit stable fields for semantic metadata.

Dynamic metadata only for non-critical extension data.

---

# 87. API Namespace Design

Prefer grouped handles:

```rust
client.conflicts()
client.scopes()
client.blobs()
client.operations()
client.diagnostics()
```

instead of one type with 100 methods.

---

# 88. Capability-Based API

Some APIs may be available only when configured capability exists.

Return:

```text
UnsupportedCapability
```

rather than panic.

---

# 89. Compile-Time Capability APIs

For strongly typed advanced embedding, additional generic APIs can exist.

But common SDK should remain ergonomic.

---

# 90. Async Cancellation

Dropping an awaited future should not corrupt state.

---

# 91. Mutation Cancellation

If local mutation transaction already committed:

```text
dropping future
```

does not erase durable intent.

---

# 92. Sync Cancellation

A cancelled sync attempt may leave:

```text
operation in-flight
```

which startup/retry logic safely recovers.

---

# 93. Timeouts

Expose timeout configuration as policy.

Do not make caller wrap every operation in ad-hoc timeout logic.

---

# 94. Backpressure Surface

If local queues/resources are full:

```text
ResourceError::Backpressure
```

or similar.

---

# 95. User-Initiated Priority

Allow bounded public hints:

```rust
SyncReason::UserInitiated
```

but do not let app bypass scheduler safety limits.

---

# 96. Observability API

Allow application to install tracing/metrics bridge.

Do not invent proprietary logging system.

---

# 97. Tracing

Use `tracing` internally.

Public SDK should not require callers to configure it, but integrate naturally when they do.

---

# 98. Metrics

Expose stable metrics names only after careful review.

---

# 99. Debug Representation

Public IDs and state types may derive `Debug`.

Sensitive types must redact.

---

# 100. Serialization of Public Types

Do not automatically promise serialized stability just because a type derives Serde.

Document which serialized forms are contractual.

---

# 101. Rust Struct Layout

Never treat Rust memory layout as stable ABI.

---

# 102. SemVer Policy

Aequora crate versions should follow Semantic Versioning.

Before `1.0`:

```text
minor releases may break APIs
```

but still document migrations.

---

# 103. Post-1.0

Breaking public API:

```text
major release
```

unless hidden behind unstable namespace/feature.

---

# 104. Runtime Protocol Version Is Separate

Important:

```text
crate semver != protocol version
```

---

# 105. Local Store Format Version Is Separate

Also:

```text
crate semver != local DB format
```

---

# 106. Operation Schema Version Is Separate

Application domain schema evolves independently.

---

# 107. One Release Can Support Multiple Protocol Versions

Yes.

Do not tightly couple.

---

# 108. Deprecation

Use Rust `#[deprecated]` for public APIs.

Document:

```text
replacement
removal target
migration example
```

---

# 109. Deprecation Window

After 1.0, keep common deprecated APIs for at least one reasonable release window unless security demands faster removal.

---

# 110. Renaming

Prefer:

```text
add new method
deprecate old method
```

over abrupt rename.

---

# 111. Breaking Type Changes

If a public struct is likely to gain fields, consider:

```text
private fields + constructor
builder
#[non_exhaustive]
```

---

# 112. Public Struct Fields

Use public fields only for simple value objects intended for direct construction.

---

# 113. Builders for Evolving Config

Recommended.

---

# 114. Sealed Traits

Use sealed traits where external implementation would violate invariants.

---

# 115. Open Traits

Use public implementable traits only where extension is intentionally supported.

---

# 116. Example Sealed Trait

Internal protocol state marker types.

---

# 117. Example Open Trait

```text
CredentialProvider
SyncTransport
StorageAdapter
```

---

# 118. Blanket Impl Caution

Public blanket implementations can become semver constraints.

Use deliberately.

---

# 119. Generic Parameter Caution

Too many public generics can make future evolution hard.

Prefer encapsulated client type where possible.

---

# 120. Trait Object Boundary

Internally:

```rust
Arc<dyn SyncTransport>
```

may simplify stable API.

---

# 121. Zero-Cost vs Stable Ergonomics

Do not optimize every public abstraction for theoretical zero-cost at the expense of usability.

Sync/network/DB operations dominate most overhead anyway.

---

# 122. Compile Time

A facade SDK should avoid pulling all optional adapters.

---

# 123. Feature Isolation

Example:

```text
aequora
```

depends on core SDK only.

User separately adds:

```text
aequora-stoolap
aequora-postgres
```

---

# 124. Example Client Dependencies

```toml
[dependencies]
aequora = "..."
aequora-stoolap = "..."
aequora-http = "..."
```

---

# 125. Example Server Dependencies

```toml
[dependencies]
aequora-server = "..."
aequora-postgres = "..."
aequora-axum = "..."
```

---

# 126. Public Macro API

Optional derive:

```rust
#[derive(AequoraOperation)]
#[aequora(kind = 1002, schema = 1)]
struct SetStudentPhone { ... }
```

---

# 127. Macro Should Validate Registry Linkage

Compile-time or CI:

```text
ID exists
schema matches
duplicate impossible
```

---

# 128. Macro Stability

Proc-macro syntax itself is public API.

Keep it small and explicit.

---

# 129. Manual API Must Exist

Do not make macros the only way to use Aequora.

---

# 130. Generated SDK Support

Application registry can generate:

```text
operation constants
typed wrappers
docs
```

while still using public Rust traits.

---

# 131. Documentation Architecture

Public SDK should include:

```text
crate-level overview
quickstart
client guide
server guide
adapter guide
error guide
upgrade guide
examples
```

---

# 132. Quickstart

Should achieve first successful sync quickly.

---

# 133. Architecture Docs

Keep deeper docs separate from quickstart.

Do not force every adopter to read 30 architecture documents before using SDK.

---

# 134. Example Quality

Examples should be:

```text
compilable
small
production-oriented
```

not toy code that bypasses safety.

---

# 135. Doctests

Use for small APIs.

---

# 136. Integration Examples

Use real directories for:

```text
Dioxus
Axum
mobile
desktop
custom adapter
```

---

# 137. Stability Tiers

Mark APIs:

```text
Stable
Experimental
Internal
```

---

# 138. Experimental Namespace

Possible:

```rust
aequora::experimental
```

Do not promise semver stability there.

---

# 139. Feature-Gated Experimental APIs

Acceptable before stabilization.

---

# 140. Unstable Marker

Document clearly.

---

# 141. MSRV Policy

Public release notes must state MSRV changes.

Post-1.0, MSRV increases should follow declared policy.

---

# 142. Rust Edition Changes

Do not require unnecessary immediate edition migration from consumers.

---

# 143. Cross-Crate Versioning

Initially release Aequora crates in lockstep version.

This simplifies ecosystem compatibility.

---

# 144. Independent Versioning Later

Can split once crate boundaries stabilize.

---

# 145. Compatibility Matrix

Publish:

```text
Aequora crate release
supported protocol versions
local store format
adapter versions
```

---

# 146. Error Compatibility Matrix

Stable error codes remain interpretable across releases.

---

# 147. Public API Tests

Use:

```text
trybuild
compile-fail tests
API snapshot tooling
```

where useful.

---

# 148. SemVer Check

CI can use semver checking tooling to detect accidental breaking API changes.

---

# 149. Public API Snapshot

Track exported items.

Review diffs.

---

# 150. Compile-Fail Tests

Useful for proving:

```text
cannot call execute before validate
cannot construct invalid typestate
```

---

# 151. Misuse Resistance

A good SDK should make invalid operations difficult.

Examples:

```text
cannot manually advance cursor
cannot directly insert server ledger row
cannot mutate outbox bypassing domain mutation
```

---

# 152. Escape Hatches

If an advanced escape hatch is necessary:

```text
explicitly named
unsafe/dangerous module
document invariant responsibility
```

---

# 153. Raw Store Access

If exposed for read-only app queries, distinguish from sync metadata mutation.

---

# 154. Mutable Raw Store Access

Avoid in public SDK unless carefully scoped inside transaction API that preserves Aequora invariants.

---

# 155. Application Transaction API

Possible:

```rust
client.local_transaction(|tx| async move {
    tx.domain().insert(...)?;
    tx.outbox().enqueue(...)?;
})
```

But this may expose too much complexity.

Prefer operation-based mutation API for ordinary users.

---

# 156. Advanced Domain SDK

For sophisticated applications, provide a lower-level domain integration crate.

---

# 157. Two-Level API Strategy

```text
High-level Application SDK
Low-level Integration SDK
```

---

# 158. High-Level SDK

For product developers:

```text
mutate
sync
status
conflicts
```

---

# 159. Integration SDK

For framework/database authors:

```text
transactions
storage traits
handler registry
transport traits
```

---

# 160. Keep Them Separate

This prevents every user from seeing implementation complexity.

---

# 161. Server Handler Errors

Domain handlers return:

```text
DomainError
```

which server maps to:

```text
stable operation outcome/error
```

---

# 162. Domain Errors

Application may define typed errors.

Aequora needs conversion into canonical public result.

---

# 163. Business Error vs System Error

Keep distinct.

Example:

```text
InvoiceAlreadyPaid
```

is not:

```text
DatabaseUnavailable
```

---

# 164. User-Visible Operation Result

Server may reject business operation without treating sync itself as broken.

---

# 165. SDK Operation Outcome

Expose:

```text
accepted
business rejected
conflict
```

distinct from transport/storage failure.

---

# 166. Retry Semantics

Business rejection is usually non-retryable without new intent.

Transport failure may be retryable.

---

# 167. Auth API

Client SDK should support:

```text
credential provider
auth state events
reauthentication trigger
```

without exposing HTTP authorization headers directly.

---

# 168. Multi-Tenant API

Tenant context should be explicit where application can switch tenant.

---

# 169. Current Tenant

Avoid hidden global tenant when one client may sync multiple scopes/tenants.

---

# 170. Device API

Expose device diagnostics/status, not private key access.

---

# 171. Local Store API

Public operations:

```text
storage usage
backup
clear cache
verify
```

can be grouped under:

```rust
client.local_store()
```

---

# 172. Dangerous Store Operations

Delete/reset/rebootstrap should require explicit methods and confirmation at UI layer.

---

# 173. Threading Model Documentation

Document:

```text
methods async
client cloneable
callbacks/events may execute off UI thread
```

---

# 174. Reentrancy

Callbacks should not deadlock if they call safe read APIs.

Avoid holding internal mutex across external callback invocation.

---

# 175. No Lock Across `.await`

Public implementation should preserve async safety.

---

# 176. Bounded Internal Queues

If event subscriber is slow:

```text
drop/coalesce advisory events
```

rather than unbounded memory growth.

---

# 177. Durable Event State

Important events remain queryable through durable state.

---

# 178. SDK Invariants

## AEQ-INV-SDK001

```text
Public client APIs cannot directly advance authoritative sync cursors.
```

## AEQ-INV-SDK002

```text
Public mutation success distinguishes local durable commit from authoritative server confirmation.
```

## AEQ-INV-SDK003

```text
Public storage-neutral APIs do not expose SQLx, Stoolap, SQLite, or platform-specific transaction types.
```

## AEQ-INV-SDK004

```text
Dropping or cancelling an async SDK future cannot invalidate already-durable local intent.
```

## AEQ-INV-SDK005

```text
Advisory event-stream loss cannot make durable synchronization state unrecoverable.
```

---

# 179. Additional SDK Invariants

## AEQ-INV-SDK006

```text
Stable public error categories and error codes remain interpretable across supported releases.
```

## AEQ-INV-SDK007

```text
External extension points cannot redefine core cursor, authority, idempotency, or identity semantics.
```

## AEQ-INV-SDK008

```text
Public API semver is versioned independently from network protocol and local-store format versions.
```

## AEQ-INV-SDK009

```text
Dangerous operations are explicit and cannot be reached through ordinary convenience APIs.
```

## AEQ-INV-SDK010

```text
Public API changes are automatically checked for accidental semver breakage before release.
```

---

# 180. Recommended Public Crate Layout

```text
crates/
├── aequora/
│   └── facade
├── aequora-types/
├── aequora-client/
├── aequora-server/
├── aequora-operation/
├── aequora-adapter-sdk/
├── aequora-macros/
└── aequora-experimental/
```

---

# 181. Recommended First Stable API

For v1, keep public surface intentionally small:

```text
AequoraClient
AequoraClientBuilder
Operation
OperationId
MutationReceipt
SyncStatus
SyncResult
SyncEvent
ConflictHandle
ScopeHandle
AequoraError
```

---

# 182. Avoid v1 Overexposure

Do not stabilize every internal type just because examples need access.

Create intentional facade methods.

---

# 183. Client Example

```rust
use aequora::prelude::*;

let client = AequoraClient::builder()
    .store(store)
    .transport(transport)
    .credentials(credentials)
    .build()
    .await?;

let receipt = client
    .mutate(CreateStudent {
        student_id,
        name,
    })
    .await?;

println!("saved locally as {}", receipt.operation_id);

client.sync_now().await?;
```

---

# 184. Event Example

```rust
let mut events = client.events();

while let Some(event) = events.next().await {
    match event {
        SyncEvent::StatusChanged(status) => {
            println!("{status:?}");
        }
        SyncEvent::ConflictCreated(id) => {
            show_conflict(id);
        }
        _ => {}
    }
}
```

---

# 185. Server Example

```rust
let domain = DomainRegistry::builder()
    .register(CreateStudentHandler::new(repo.clone()))
    .build()?;

let server = AequoraServer::builder()
    .authority_store(postgres)
    .domain(domain)
    .authenticator(auth)
    .build()?;
```

---

# 186. Application Wrapper

Products should wrap generic Aequora SDK.

Example:

```rust
pub struct SchoolSync {
    inner: AequoraClient,
}
```

Then expose:

```text
students()
attendance()
fees()
```

---

# 187. Why Product Wrapper Helps

It prevents product code from becoming tightly coupled to generic operation construction everywhere.

---

# 188. Public API Review Checklist

Before stabilizing any item ask:

```text
Does app code really need this?
Is it a semantic concept or implementation detail?
Can we evolve it later?
Does it expose an external crate unnecessarily?
Does it preserve invariant safety?
```

---

# 189. Release Checklist

Before release:

```text
cargo test
doctests
semver API check
public docs build
examples compile
compatibility matrix updated
deprecation report reviewed
```

---

# 190. Completion Criteria

Part 35 is complete when:

```text
[ ] facade crate defined
[ ] client public API defined
[ ] server public API defined
[ ] operation API defined
[ ] status/events defined
[ ] conflict/scope/blob handles defined
[ ] error architecture defined
[ ] extension points defined
[ ] safe async cancellation semantics defined
[ ] semver policy defined
[ ] protocol/store-version separation defined
[ ] deprecation policy defined
[ ] public API testing defined
[ ] SDK invariants added
```

---

# 191. Final Architecture

```text
                   PRODUCT APPLICATION
                          │
                          ▼
                    Product SDK
                          │
                          ▼
                Aequora Public Facade
        ┌─────────────────┼─────────────────┐
        ▼                 ▼                 ▼
   Client SDK         Server SDK       Adapter SDK
        │                 │                 │
        ▼                 ▼                 ▼
   Sync Runtime       Domain Exec      Implementations
        │                 │
        └──────────┬──────┘
                   ▼
             Internal Core

Public:
    semantic, stable, ergonomic

Internal:
    free to evolve

Adapters:
    explicit extension contracts
```

---

# 192. Final Recommendation

Aequora's public Rust API should be deliberately smaller than its internal architecture.

The library may internally contain:

```text
journal
ledger
planner
scheduler
bootstrap
anti-entropy
governance
conformance
```

but a normal application developer should primarily see:

```text
open
mutate
query
sync
observe
resolve
```

The key principle is:

> **A reusable sync engine becomes trustworthy when its public API makes the correct workflow easier than bypassing the engine.**

This keeps Aequora usable for small Rust applications while preserving enough extension points for large enterprise systems and third-party adapters.
