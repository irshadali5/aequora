# Aequora Sync — Part 34

# Reference Implementation, Workspace, Crate Boundaries, Dependency Direction, and Build Architecture

## 1. Status of the Architecture Series

The architecture is **not missing a fundamental synchronization concept** after Parts 1–33. The major correctness, protocol, storage, mobile, desktop, governance, operations, security, and ecosystem concerns have been designed.

However, there is still an important gap between:

```text
"we know how Aequora should work"
```

and:

```text
"we can now implement Aequora without gradually destroying those boundaries"
```

Parts 1–33 primarily define system semantics and subsystem architectures.

Part 34 begins the **implementation architecture phase**.

The central question becomes:

> How should the actual Rust workspace be divided so that the architecture remains enforceable by Cargo dependencies, Rust types, feature boundaries, tests, and public APIs?

This part defines that reference implementation.

---

# 2. Central Principle

> **Aequora's Cargo dependency graph should encode the architecture.**

A lower-level crate must not import a higher-level integration merely because doing so is convenient.

Examples:

```text
aequora-core
    must not depend on Axum

aequora-protocol
    must not depend on PostgreSQL

aequora-storage-core
    must not depend on Stoolap

aequora-client
    must not depend on Dioxus

aequora-domain
    must not depend on HTTP

aequora-postgres
    may depend on storage/domain interfaces
```

If dependency direction is correct, many architectural violations become compile-time dependency problems rather than production design problems.

---

# 3. Why Part 34 Is Necessary

Without explicit implementation boundaries, a large Rust workspace commonly drifts into:

```text
shared "utils" crates
circular concepts
SQL types leaking into domain APIs
Axum extractors leaking into business handlers
UI types leaking into client state
database transaction types leaking everywhere
feature-flag explosion
one giant core crate
```

Aequora should prevent this from the beginning.

---

# 4. Implementation Objectives

The reference workspace should provide:

```text
small coherent crates
one-way dependency direction
stable public SDK surfaces
internal implementation crates
feature isolation
adapter isolation
testkit reuse
platform isolation
minimal dependency duplication
fast incremental builds
clear ownership
```

---

# 5. Recommended Repository Strategy

Start with:

```text
one Rust Cargo workspace
one Git repository
```

rather than splitting Aequora into many repositories.

Reasons:

```text
atomic cross-crate changes
simpler protocol evolution
single CI graph
shared test fixtures
consistent dependency policy
easier refactoring before v1
```

Repository splitting can happen later for independently released SDKs if justified.

---

# 6. Top-Level Repository Layout

Recommended:

```text
aequora/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── deny.toml
│
├── crates/
│   ├── foundation/
│   ├── protocol/
│   ├── domain/
│   ├── storage/
│   ├── client/
│   ├── server/
│   ├── platform/
│   ├── integration/
│   ├── operations/
│   ├── tooling/
│   └── testing/
│
├── apps/
│   ├── aequora-server/
│   ├── aequora-cli/
│   └── aequora-agent/
│
├── registry/
├── migrations/
├── fixtures/
├── examples/
├── benches/
├── fuzz/
├── docs/
└── scripts/
```

---

# 7. Crate Families

The workspace should be understood as layers:

```text
Layer 0 — Foundation
Layer 1 — Protocol and Contracts
Layer 2 — Domain Execution
Layer 3 — Storage Interfaces
Layer 4 — Sync Engines
Layer 5 — Physical Adapters
Layer 6 — Platform Integrations
Layer 7 — Applications
Layer 8 — Tooling and Verification
```

---

# 8. Layer 0 — Foundation

Suggested crates:

```text
aequora-types
aequora-error
aequora-time
aequora-identity
aequora-codec
```

These should have the smallest dependency footprint.

---

# 9. `aequora-types`

Contains durable primitive/newtype contracts:

```rust
pub struct OperationId(...);
pub struct EntityId(...);
pub struct TenantId(...);
pub struct ActorId(...);
pub struct DeviceId(...);
pub struct ScopeId(...);
pub struct EntityVersion(...);
pub struct TimelineSequence(...);
```

It should not contain:

```text
HTTP
SQL
Dioxus
Tokio runtime orchestration
platform APIs
```

---

# 10. Strong Newtypes

Never casually use:

```rust
String
u64
Uuid
```

for semantically different identifiers.

Prefer:

```rust
pub struct TenantId(Uuid);
pub struct DeviceId(Uuid);
pub struct OperationId(Uuid);
```

This prevents cross-domain identity mistakes.

---

# 11. `aequora-error`

Defines stable internal error taxonomy.

Do not make it a dumping ground for every adapter error.

Instead distinguish:

```text
canonical error
adapter source error
wire error representation
UI presentation
```

---

# 12. `aequora-time`

Contains:

```text
HLC
server timestamp abstractions
clock traits
deterministic test clock
```

It must not call `SystemTime::now()` throughout the architecture.

---

# 13. `aequora-codec`

Owns canonical encoding utilities:

```text
Postcard framing
RON configuration helpers
BLAKE3 canonical digest helpers
optional zstd framing
```

It does not define business operations.

---

# 14. Layer 1 — Protocol and Durable Contracts

Suggested:

```text
aequora-protocol
aequora-registry-types
aequora-registry-generated
aequora-capabilities
aequora-schema
```

---

# 15. `aequora-protocol`

Owns wire messages:

```text
ClientHello
ServerHello
ExchangeRequest
ExchangeResponse
BootstrapRequest
BootstrapManifest
SyncHint
ProtocolError
```

---

# 16. Protocol Crate Rule

It must not know:

```text
Axum
reqwest
PostgreSQL
Stoolap
Dioxus
```

It is transport-independent.

---

# 17. Wire Types vs Domain Types

Do not make every domain struct directly serializable as the network contract.

Prefer explicit boundary:

```text
WireOperation
    ↓ validate/upcast
CanonicalOperation
```

This gives schema evolution room.

---

# 18. `aequora-capabilities`

Contains capability identifiers and negotiation models.

No runtime implementation.

---

# 19. Registry Crates

Part 29 should materialize as:

```text
aequora-registry-types
aequora-registry-generated
aequora-registry-codegen
```

Generated registry constants should be compile-time accessible.

---

# 20. Layer 2 — Domain Execution

Suggested:

```text
aequora-domain
aequora-operation
aequora-validation
aequora-conflict
aequora-execution
```

---

# 21. `aequora-operation`

Defines canonical operation concepts:

```rust
pub trait Operation {
    type Payload;
    type Outcome;
}
```

plus envelopes, dependencies, semantic metadata, and operation classes.

---

# 22. `aequora-domain`

Defines application-facing handler interfaces.

Example:

```rust
#[async_trait]
pub trait OperationHandler<O: Operation> {
    async fn validate(
        &self,
        ctx: &ValidationContext,
        op: &O,
    ) -> Result<(), DomainError>;

    async fn execute(
        &self,
        ctx: &ExecutionContext,
        op: O,
    ) -> Result<O::Outcome, DomainError>;
}
```

Exact signatures can evolve, but the boundary must remain storage/transport-neutral.

---

# 23. Deterministic Execution Boundary

Part 12 requires that handlers receive explicit inputs.

Do not allow handlers to casually reach:

```text
network
randomness
wall clock
environment
global singleton
```

through hidden dependencies.

---

# 24. `aequora-validation`

Contains reusable validation stages:

```text
schema
dependency
domain preconditions
compatibility
```

Authorization remains a distinct security concern.

---

# 25. `aequora-conflict`

Contains:

```text
conflict models
resolver traits
consistency profile behavior
rebase contracts
```

It should not depend on UI conflict dialogs.

---

# 26. Layer 3 — Storage Contracts

Suggested:

```text
aequora-storage-core
aequora-storage-metadata
aequora-storage-backup
aequora-blob-core
```

---

# 27. `aequora-storage-core`

Defines capability-oriented traits.

Avoid one enormous trait such as:

```rust
trait Database {
    // 70 methods
}
```

Prefer narrow capabilities.

---

# 28. Capability Traits

Conceptually:

```rust
pub trait LocalTransactionStore;
pub trait AuthoritativeTransactionStore;
pub trait OutboxStore;
pub trait JournalStore;
pub trait OperationLedgerStore;
pub trait CursorStore;
pub trait SnapshotStore;
pub trait FencingStore;
pub trait IntegrityStore;
```

---

# 29. Why Narrow Traits Matter

Different stores support different roles.

For example:

```text
PostgreSQL
    authoritative + journal + ledger

Stoolap
    local domain + outbox + cursor

Object storage
    snapshot/blob artifacts
```

They should not implement meaningless methods.

---

# 30. Transaction Scope

Do not expose database-specific transaction types through core APIs.

Bad:

```rust
fn execute(tx: &mut sqlx::Transaction<Postgres>)
```

Better:

```rust
fn execute<T: AuthorityTxn>(tx: &mut T)
```

or an appropriate repository abstraction.

---

# 31. Avoid Over-Abstraction

Aequora should not build a universal SQL ORM.

Storage abstractions should represent:

```text
sync semantics
```

not every possible database operation.

---

# 32. Application Domain Persistence

Application handlers may define repositories such as:

```text
StudentRepository
InvoiceRepository
MessageRepository
```

These are app-domain contracts.

Aequora does not need to understand every table.

---

# 33. Layer 4 — Sync Engines

Suggested:

```text
aequora-client
aequora-server-core
aequora-planner
aequora-reconcile
aequora-bootstrap
aequora-anti-entropy
aequora-scheduler
```

---

# 34. `aequora-client`

Owns the client synchronization state machine.

It should orchestrate:

```text
outbox
transport
pull
reconciliation
cursor
conflicts
bootstrap
```

---

# 35. Client Must Not Know UI

No dependency on:

```text
Dioxus
Swift
Kotlin
Electron
```

Instead expose:

```text
events
status
query/mutation APIs
```

---

# 36. `aequora-server-core`

Owns server-side synchronization orchestration:

```text
authentication context consumption
authorization stage
validation
dependency planning
execution
journal
ledger
response assembly
```

It does not own HTTP routing.

---

# 37. `aequora-planner`

Owns:

```text
dependency DAG
topological ordering
batch planning
aggregate grouping
```

Keep algorithmic code separately testable.

---

# 38. `aequora-reconcile`

Owns atomic client reconciliation semantics.

This is correctness-critical and deserves focused tests.

---

# 39. `aequora-bootstrap`

Owns:

```text
manifest interpretation
chunk state
resume
staging generation
activation
```

---

# 40. `aequora-anti-entropy`

Owns:

```text
digest tree
partition comparison
repair planning
```

---

# 41. `aequora-scheduler`

Owns policy decisions from Parts 6, 18, 20, 31, and 32.

It receives normalized resource context.

It must not import Android/iOS/Windows types.

---

# 42. Layer 5 — Physical Adapters

Suggested:

```text
aequora-postgres
aequora-stoolap
aequora-sqlite
aequora-object-store
aequora-http-client
```

---

# 43. Adapter Rule

Adapters depend inward.

Core never depends outward.

```text
aequora-storage-core
        ▲
        │
aequora-postgres
```

Never:

```text
aequora-storage-core
        │
        ▼
aequora-postgres
```

---

# 44. `aequora-postgres`

Owns PostgreSQL/Neon implementation.

Internally it may use:

```text
SQLx
PostgreSQL migrations
prepared statements
pooling
```

These types remain private to the adapter.

---

# 45. Neon

Neon remains PostgreSQL from Aequora's semantic perspective.

Cloud-specific operational features can be separate integration configuration.

---

# 46. `aequora-stoolap`

Implements certified local-store capabilities.

It should contain all Stoolap-specific:

```text
schema
transaction mapping
query translation
migration
```

---

# 47. `aequora-sqlite`

Provides an alternative local adapter.

The rest of Aequora should not change when selecting it.

---

# 48. Adapter Feature Flags

Avoid:

```text
aequora-core features = ["postgres", "sqlite", "stoolap"]
```

Prefer separate crates.

This avoids giant conditional compilation graphs.

---

# 49. Layer 6 — Transport and Framework Integrations

Suggested:

```text
aequora-axum
aequora-dioxus
aequora-mobile-runtime
aequora-desktop-runtime
aequora-platform-android
aequora-platform-ios
aequora-platform-linux
aequora-platform-windows
aequora-platform-macos
```

---

# 50. `aequora-axum`

Thin integration crate.

Responsibilities:

```text
routes
extractors
request limits
HTTP status mapping
AuthContext construction
```

It delegates to `aequora-server-core`.

---

# 51. Thin Axum Route

Desired shape:

```rust
async fn exchange(
    State(service): State<SyncService>,
    auth: AuthContext,
    body: Bytes,
) -> Result<Response, ApiError> {
    service.exchange(auth, body).await
}
```

Not hundreds of lines of business logic.

---

# 52. `aequora-dioxus`

Optional UI integration.

Contains:

```text
hooks
signals
status adapters
query invalidation helpers
```

No synchronization correctness logic.

---

# 53. Mobile and Desktop Platform Crates

Parts 31–33 define these.

They normalize OS signals into Aequora types.

---

# 54. Layer 7 — Applications

Applications are composition roots.

Examples:

```text
apps/aequora-server
apps/aequora-agent
apps/aequora-cli
```

---

# 55. Composition Root Principle

Only application binaries should know the complete implementation combination.

Example server:

```text
Axum
+
Aequora Server Core
+
Postgres Adapter
+
Domain Handlers
+
Crypto Provider
+
Observability
```

---

# 56. Server Binary

`apps/aequora-server` should mostly contain:

```text
config loading
dependency construction
route installation
startup
shutdown
```

---

# 57. Client App Composition

An application using Aequora chooses:

```text
Aequora Client
+
Stoolap
+
HTTP transport
+
Dioxus
+
platform adapter
```

---

# 58. Dependency Injection

Use explicit constructors/builders.

Avoid global service locators.

---

# 59. Builder Example

Concept:

```rust
let client = AequoraClientBuilder::new()
    .store(store)
    .transport(transport)
    .registry(registry)
    .scheduler(scheduler)
    .build()?;
```

---

# 60. Compile-Time vs Runtime Polymorphism

Use generics where:

```text
hot path
small number of concrete implementations
compile-time capability useful
```

Use trait objects where:

```text
composition flexibility
plugin-like provider
binary size/code generation matters
```

Do not make every abstraction dynamic.

---

# 61. Typestate

Use typestate where transitions matter.

Examples:

```text
Incoming
Authenticated
Authorized
Validated
Executable
```

---

# 62. Typestate Example

```rust
struct OperationEnvelope<S> {
    inner: CanonicalOperation,
    state: PhantomData<S>,
}
```

Only appropriate methods produce next state.

---

# 63. Do Not Overuse Typestate

Use it for correctness-critical lifecycle.

Do not turn every ordinary builder into ten generic states.

---

# 64. Feature Flag Policy

Features should represent optional capabilities such as:

```text
compression-zstd
tracing
test-failpoints
```

Not architectural layers.

---

# 65. Additive Features

Cargo features are additive.

Do not design mutually exclusive architecture such as:

```text
feature postgres OR sqlite
```

inside one core crate.

Separate adapter crates instead.

---

# 66. Default Features

Keep core default features minimal.

---

# 67. `std` Strategy

Aequora targets normal OS applications/server environments.

Do not impose `no_std` on the entire architecture unless a real requirement appears.

---

# 68. Async Runtime

Tokio should be isolated mainly to orchestration/integration crates.

Pure algorithms should remain synchronous where possible.

---

# 69. Rayon

Use only in measured CPU-bound modules:

```text
hashing
large validation
compression
Merkle work
```

through bounded execution.

---

# 70. Serialization Boundary

Postcard is the default binary protocol.

RON is primarily:

```text
configuration
registry source
developer-readable fixtures
```

JSON remains optional at external interoperability boundaries.

---

# 71. Public API Surface

Not every crate needs to be public/stable.

Classify crates:

```text
Public SDK
Public Adapter SDK
Internal Stable
Internal Unstable
Application
```

---

# 72. Public SDK Crates

Likely:

```text
aequora-types
aequora-client
aequora-operation
aequora-storage-core
aequora-protocol
```

after API maturity.

---

# 73. Internal Crates

Examples:

```text
aequora-planner
aequora-reconcile
aequora-metadata-internal
```

can initially be workspace-private.

---

# 74. `pub` Discipline

Rust `pub` should not mean:

```text
"another crate currently needs it"
```

Use:

```rust
pub(crate)
pub(super)
```

aggressively.

---

# 75. Prelude

Avoid a giant prelude that re-exports the whole system.

Small SDK-specific preludes may be acceptable.

---

# 76. Utility Crate Warning

Do not create:

```text
aequora-utils
```

as a dumping ground.

Place functionality in the crate that owns the concept.

---

# 77. Circular Dependency Prevention

Cargo prevents direct cycles, but conceptual cycles can still emerge via shared crates.

Do not solve cycles by moving everything into `common`.

Instead reconsider ownership.

---

# 78. Dependency Direction

Reference:

```text
platform/integration
        ↓
sync engines
        ↓
domain + storage contracts
        ↓
protocol/contracts
        ↓
foundation
```

Adapters attach to contract layers from the side.

---

# 79. Forbidden Dependency Examples

```text
aequora-types -> aequora-client
aequora-protocol -> aequora-axum
aequora-domain -> aequora-postgres
aequora-client -> aequora-dioxus
aequora-storage-core -> aequora-stoolap
```

---

# 80. Architecture CI

Automate dependency rules.

Possible checks:

```text
cargo metadata graph inspection
cargo-deny
custom architecture lint script
```

---

# 81. Workspace Dependencies

Use `[workspace.dependencies]` for shared versions.

Example categories:

```text
serde
postcard
uuid
blake3
tokio
tracing
thiserror
```

---

# 82. Avoid Version Drift

All workspace crates should normally use one compatible version of foundational dependencies.

---

# 83. MSRV

Define explicit Minimum Supported Rust Version when approaching stable releases.

Before v1, tracking recent stable Rust is reasonable.

---

# 84. Rust Edition

Use one edition workspace-wide unless migration requires otherwise.

---

# 85. Unsafe Rust Policy

Default:

```text
forbid unsafe
```

in Aequora-owned core crates where practical.

---

# 86. Adapter Exceptions

FFI/platform crates may require unsafe.

Require:

```text
small unsafe boundary
SAFETY comments
focused tests
review
```

---

# 87. Panic Policy

Library APIs should return typed errors for expected failures.

Do not use panic for:

```text
network failure
DB conflict
invalid remote input
disk full
```

---

# 88. Assertions

Assertions are appropriate for impossible internal states after validation, but external data must not trigger uncontrolled panics.

---

# 89. Error Layers

Recommended:

```text
AdapterError
↓
CanonicalInternalError
↓
ProtocolError / API Error
↓
UI Presentation
```

---

# 90. Secret Types

Use dedicated wrappers:

```text
SecretString
SecretBytes
```

with redacted `Debug`.

---

# 91. Sensitive Data

Avoid deriving `Debug` blindly on structures containing:

```text
tokens
keys
PII
```

---

# 92. Test Architecture

Each crate should have:

```text
unit tests
contract tests
integration tests
```

appropriate to responsibility.

---

# 93. Shared Testkit

Suggested:

```text
aequora-testkit
aequora-model
aequora-conformance
```

---

# 94. `aequora-model`

Contains the deterministic reference model from Part 01.

It should not share implementation details with production engine in ways that invalidate differential testing.

---

# 95. `aequora-testkit`

Provides:

```text
fake clock
fake transport
fault injection
fixture builders
reference stores
```

---

# 96. Conformance Crate

Part 30 becomes executable through:

```text
aequora-conformance
```

---

# 97. Test Fixtures

Store durable protocol fixtures under:

```text
fixtures/protocol/
fixtures/snapshots/
fixtures/operations/
```

---

# 98. Golden Fixture Rule

Published fixture bytes must never be casually regenerated.

A diff must explain compatibility impact.

---

# 99. Fuzzing

Separate fuzz targets:

```text
protocol decode
snapshot manifest
registry parser
import parser
IPC framing
```

---

# 100. Benchmarks

Bench:

```text
operation encode/decode
batch planning
reconciliation
Merkle hashing
storage adapter operations
snapshot streaming
```

---

# 101. Build Profiles

Use:

```text
dev
test
release
bench
```

with deliberate release settings.

---

# 102. Release Optimization

Potential:

```toml
[profile.release]
lto = "thin"
codegen-units = 1
```

but benchmark build-time/runtime tradeoffs before locking policy.

---

# 103. Mobile Release Profiles

May optimize for binary size separately.

---

# 104. Server Release Profiles

May prioritize throughput.

---

# 105. Feature Matrix Testing

CI should test meaningful combinations.

Avoid Cartesian explosion.

---

# 106. Core CI Matrix

Example:

```text
Linux stable Rust
Linux MSRV
Windows stable
macOS stable
Android compile
iOS compile
```

---

# 107. Adapter CI

Each official adapter runs its conformance profile.

---

# 108. PostgreSQL Integration CI

Use real PostgreSQL.

Do not certify authoritative transaction semantics using only mocks.

---

# 109. Mobile Storage CI

Run target-platform tests where possible.

---

# 110. Workspace Lints

At workspace level:

```text
rustfmt
clippy
unused_must_use
unsafe policy
missing docs for public SDK
```

---

# 111. Dependency Policy

Use:

```text
cargo-deny
cargo-audit
license allowlist
duplicate dependency review
```

---

# 112. Permissive Dependency Preference

For Aequora's reusable ecosystem, prefer permissively licensed dependencies when suitable.

Record exceptions deliberately.

---

# 113. Supply Chain

Lock dependency versions for application releases.

Generate:

```text
SBOM
dependency inventory
```

for production artifacts.

---

# 114. Proc Macros

Keep proc-macro crates separate.

Suggested:

```text
aequora-macros
```

---

# 115. Macro Responsibility

Macros may generate:

```text
operation boilerplate
registry linkage
stable ID checks
```

They must not hide critical runtime semantics.

---

# 116. Code Generation

Registry codegen should be deterministic.

Input:

```text
registry/*.ron
```

Output:

```text
generated Rust
documentation
compatibility manifest
```

---

# 117. Generated Files

Decide one policy:

```text
checked into Git
```

or:

```text
generated during build
```

For durable registry review, checked/generated verification can be useful.

---

# 118. Build Script Policy

Avoid complex networked `build.rs`.

Builds should be hermetic.

---

# 119. No Network During Build

Dependencies and generated artifacts should not require arbitrary online fetches.

---

# 120. Migrations

Keep migrations explicit.

Suggested:

```text
migrations/
├── postgres/
├── stoolap/
├── sqlite/
└── metadata/
```

---

# 121. Migration Ownership

A migration belongs to the adapter/schema that owns it.

---

# 122. Migration Checksums

Part 29 rule:

```text
same migration ID
+
different body
=
error
```

---

# 123. Application Domain Registration

An application should build its own registry/handler set.

Concept:

```rust
let registry = DomainRegistry::builder()
    .register(CreateStudentHandler::new(...))
    .register(UpdateInvoiceHandler::new(...))
    .build()?;
```

---

# 124. Compile-Time Registration

Prefer explicit registration over hidden global constructors.

---

# 125. Startup Validation

At startup verify:

```text
every registered operation has handler
every handler references valid profile
required storage capabilities available
protocol capabilities coherent
```

Fail before accepting traffic.

---

# 126. Server Composition Example

```text
PostgresPool
↓
PostgresAuthorityStore
↓
DomainRegistry
↓
SyncService
↓
Axum Router
```

---

# 127. Client Composition Example

```text
StoolapLocalStore
↓
AequoraClient
├── HttpTransport
├── Scheduler
├── ConflictEngine
└── BootstrapEngine
↓
Dioxus Integration
```

---

# 128. Mobile Composition

```text
Android/iOS platform adapter
↓
MobileRuntime
↓
AequoraClient
↓
Certified LocalStore
```

---

# 129. Desktop Composition

```text
Desktop platform adapter
↓
DesktopRuntime
↓
AequoraClient
```

or:

```text
GUI
↓ IPC
aequora-agent
↓
AequoraClient
```

---

# 130. Other-Language Composition

For Python/Java/.NET/etc.:

```text
Host Application
↓
language binding / local agent
↓
Aequora Rust runtime
```

Core architecture remains unchanged.

---

# 131. C ABI Crate

If needed:

```text
aequora-ffi
```

should be separate.

---

# 132. FFI Rule

Never expose Rust layout directly as ABI.

Use:

```text
opaque handles
explicit buffers
stable integer error codes
```

---

# 133. Language SDKs

Future:

```text
aequora-java
aequora-swift
aequora-kotlin
aequora-python
aequora-dotnet
```

may wrap the agent/C ABI.

Do not rewrite synchronization logic in those languages.

---

# 134. Public SDK Stability

Before v1:

```text
iterate quickly
```

After v1:

```text
semantic versioning
deprecation windows
migration notes
```

---

# 135. Internal API Freedom

Keep unstable implementation details private so refactoring remains possible.

---

# 136. Workspace Ownership

Recommended ownership domains:

```text
Core/Protocol
Client
Server
Storage
Platform
Security
Tooling
```

Even for a solo developer, this conceptual ownership helps review AI-generated changes.

---

# 137. AI-Assisted Development Rule

AI-generated code should be constrained by crate ownership.

Prompting rule:

```text
Do not add dependencies from lower layers to higher layers.
Do not move code to common/utils to bypass dependency direction.
Do not expose adapter-specific types through public core APIs.
```

---

# 138. Architectural Review Checklist

For every new dependency ask:

```text
Who owns this concept?
Is dependency direction inward?
Could a trait/interface remove physical coupling?
Does this make core aware of a platform/framework?
```

---

# 139. Change Review

Any change affecting:

```text
durable ID
protocol
operation semantics
storage invariant
authority
security
```

requires stronger review than ordinary implementation changes.

---

# 140. Crate Count Warning

Modularity does not mean hundreds of tiny crates.

Split only when a boundary provides:

```text
dependency isolation
public API boundary
independent testing
platform isolation
compile-time benefit
```

---

# 141. Initial Practical Crate Set

Do not implement every conceptual crate on day one.

A practical initial workspace:

```text
aequora-types
aequora-protocol
aequora-domain
aequora-storage
aequora-client
aequora-server-core
aequora-postgres
aequora-stoolap
aequora-axum
aequora-testkit
aequora-cli
```

---

# 142. Split Later

As complexity grows, extract:

```text
scheduler
bootstrap
anti-entropy
registry
platform
conformance
```

when boundaries become valuable.

---

# 143. Modular Monolith Principle

Aequora should be:

```text
modular monolith repository
```

not:

```text
distributed microservice codebase
```

The network architecture can scale independently without splitting source ownership prematurely.

---

# 144. Server Deployment Does Not Dictate Crate Boundaries

One server binary can contain many internal modules while preserving clean interfaces.

---

# 145. Reference Implementation Sequence

Recommended implementation order:

```text
1. types + IDs
2. operation envelope
3. storage traits
4. in-memory reference store
5. local outbox
6. client state machine
7. protocol exchange
8. server ledger/journal
9. Postgres adapter
10. Stoolap adapter
11. reconciliation
12. bootstrap
13. conflict profiles
14. scheduler
15. platform integrations
16. anti-entropy
17. enterprise/control-plane features
```

---

# 146. Why Reference Store First

It gives:

```text
fast tests
deterministic behavior
model comparison
adapter contract oracle
```

before database complexity.

---

# 147. Minimal Vertical Slice

First end-to-end milestone:

```text
local mutation
↓
outbox
↓
Postcard exchange
↓
Axum
↓
Postgres authority
↓
journal
↓
response
↓
local reconcile
```

for one simple entity.

---

# 148. Do Not Build Everything Before Vertical Slice

Aequora's architecture is broad.

Implementation should validate the core loop early.

---

# 149. First Example Domain

Use a deliberately simple domain:

```text
Note
Todo
Contact
```

not accounting first.

---

# 150. Why

It tests synchronization mechanics without complex business invariants.

---

# 151. Second Example Domain

Use a stronger aggregate:

```text
inventory reservation
invoice
school attendance
```

to test conflicts and authority semantics.

---

# 152. Third Example

Append-only financial ledger can validate strict consistency profile.

---

# 153. Reference Server

Provide:

```text
examples/reference-server
```

showing Axum + Postgres.

---

# 154. Reference Client

Provide:

```text
examples/reference-client
```

showing local store + sync.

---

# 155. Dioxus Example

Later:

```text
examples/dioxus-local-first
```

---

# 156. Android/iOS Example

Use same domain/protocol with mobile runtime.

---

# 157. Documentation Tests

Public SDK examples should compile in CI.

---

# 158. Architecture Decision Records

Add:

```text
docs/adr/
```

for decisions such as:

```text
Postcard wire format
server authority
cursor semantics
adapter separation
```

---

# 159. Why ADRs

Future contributors/AI should know:

```text
why
```

not only:

```text
what
```

---

# 160. Generated Architecture Map

CI can generate dependency graph for review.

---

# 161. Forbidden Edge Check

Maintain machine-readable architecture rules such as:

```ron
forbidden_dependencies: [
    ("aequora-protocol", "aequora-axum"),
    ("aequora-client", "aequora-dioxus"),
    ("aequora-storage-core", "aequora-postgres"),
]
```

---

# 162. Dependency Budget

Core crates should have strict dependency budgets.

This reduces:

```text
compile time
attack surface
binary size
API leakage
```

---

# 163. Platform Dependency Isolation

Android JNI dependencies only in Android integration crate.

Apple bindings only in Apple integration crate.

Windows APIs only in Windows crate.

---

# 164. Build Portability

Core crates should compile on all supported targets without platform SDK dependencies.

---

# 165. Server-Only Dependencies

Keep:

```text
sqlx-postgres
axum
tower
```

out of client builds.

---

# 166. Client-Only Dependencies

Keep:

```text
mobile bindings
Dioxus helpers
embedded DB
```

out of server builds.

---

# 167. Binary Size

This separation matters especially on mobile.

---

# 168. Compile-Time Capability Marker

Where useful:

```rust
pub trait SupportsAtomicOutbox {}
pub trait SupportsFencing {}
```

can encode certified capabilities.

Do not overcomplicate dynamic deployment checks.

---

# 169. Runtime Capability Manifest

Still required because:

```text
database configuration
server policy
provider version
```

can affect real capability.

---

# 170. Compile-Time + Runtime

Use both:

```text
types prevent impossible composition
runtime validates environment
```

---

# 171. Storage Adapter SDK

Part 34 establishes the boundary, but a future standalone part should define how third parties implement and certify adapters.

---

# 172. Public API Architecture

Likewise, a future part should deeply specify:

```text
Rust SDK ergonomics
semver
builders
extension traits
error stability
```

---

# 173. Implementation-Phase Roadmap

After Part 34, useful remaining parts are:

```text
35 — Public Rust API and SDK Stability Architecture
36 — Storage Adapter SDK and Official Adapter Architecture
37 — PostgreSQL/Neon Authoritative Adapter Detailed Architecture
38 — Stoolap Local Adapter Detailed Architecture
39 — Axum Server Integration and Middleware Architecture
40 — Dioxus Client Integration and Reactive State Architecture
41 — CLI and Developer Toolchain Architecture
42 — Configuration, Secrets, and Feature-Flag Architecture
43 — Packaging, Distribution, and Release Engineering
44 — Deployment Topologies Architecture
45 — Observability Implementation Architecture
46 — Benchmark and Capacity Planning Architecture
47 — Testkit and Verification Infrastructure Architecture
48 — Documentation, Examples, and Adoption Architecture
49 — Licensing, Dependency, and Supply-Chain Governance
50 — v1 Scope, Milestones, Production Readiness, and GA Exit Criteria
```

These are not missing core sync semantics.

They are implementation/productization architecture.

---

# 174. Definition of Architectural Completion

Aequora can be considered **architecturally complete for v1 design** when:

```text
core semantics are specified
implementation boundaries are specified
official adapters are specified
public SDK is specified
deployment/release path is specified
test/conformance path is specified
GA scope is explicitly frozen
```

Parts 34–50 can accomplish this.

---

# 175. Core Implementation Invariants

## AEQ-INV-IMPL001

```text
Foundation and protocol crates never depend on framework, database, UI, or platform integration crates.
```

## AEQ-INV-IMPL002

```text
Physical database types never appear in Aequora's storage-neutral public contracts.
```

## AEQ-INV-IMPL003

```text
Client synchronization correctness does not depend on a UI framework.
```

## AEQ-INV-IMPL004

```text
Server synchronization correctness does not depend on Axum routing behavior.
```

## AEQ-INV-IMPL005

```text
Official adapters implement storage contracts from outer crates; core storage contracts never import official adapters.
```

---

# 176. Additional Invariants

## AEQ-INV-IMPL006

```text
Platform-specific dependencies are isolated to platform/integration crates.
```

## AEQ-INV-IMPL007

```text
Cargo features do not become a substitute for architectural adapter boundaries.
```

## AEQ-INV-IMPL008

```text
Every application binary is a composition root and does not contain reusable domain/sync semantics that belong in library crates.
```

## AEQ-INV-IMPL009

```text
Generated registry artifacts are deterministic and verifiably correspond to canonical registry sources.
```

## AEQ-INV-IMPL010

```text
A new dependency edge that violates the declared layer graph fails architecture CI.
```

---

# 177. Reference Dependency Graph

```text
                    APPLICATIONS
              ┌────────┼────────┐
              ▼        ▼        ▼
            CLI      Server    Agent
              │        │        │
              ▼        ▼        ▼
       INTEGRATION / PLATFORM LAYER
       Dioxus  Axum  Mobile  Desktop
              │        │
              └────┬───┘
                   ▼
               SYNC ENGINES
        Client / Server / Bootstrap
          Planner / Reconcile
                   │
          ┌────────┴─────────┐
          ▼                  ▼
       DOMAIN            STORAGE CONTRACTS
          │                  ▲
          │                  │
          │            PHYSICAL ADAPTERS
          │        Postgres / Stoolap / SQLite
          │
          ▼
      PROTOCOL / REGISTRY
              │
              ▼
          FOUNDATION
```

Important nuance:

Physical adapters depend on the storage contracts they implement.

They do not become lower-level dependencies of storage core.

---

# 178. Recommended First Workspace

Start implementation with:

```text
aequora/
├── crates/
│   ├── aequora-types/
│   ├── aequora-protocol/
│   ├── aequora-domain/
│   ├── aequora-storage/
│   ├── aequora-client/
│   ├── aequora-server-core/
│   ├── aequora-postgres/
│   ├── aequora-stoolap/
│   ├── aequora-axum/
│   └── aequora-testkit/
│
├── apps/
│   ├── reference-server/
│   └── aequora-cli/
│
├── registry/
├── migrations/
├── fixtures/
├── examples/
└── docs/
```

Do not create all 40+ possible crates immediately.

---

# 179. Extraction Rule

Extract a new crate only when at least one is true:

```text
it isolates a heavy dependency
it establishes a public contract
it isolates a platform
it has an independent test/conformance boundary
it prevents an architectural dependency edge
```

---

# 180. Final Recommendation

Parts 1–33 complete the major **system-design layer**, but the overall Aequora architecture should continue because implementation/productization boundaries still deserve explicit design.

Part 34 therefore starts a new phase:

```text
Parts 1–33
    = synchronization/system architecture

Parts 34–50
    = reference implementation, SDK, adapters,
      integration, deployment, release, and GA architecture
```

The most important decision introduced by Part 34 is:

> **Do not rely only on documentation to preserve Aequora's architecture. Encode the architecture into Cargo crate boundaries, Rust types, dependency direction, capability traits, conformance tests, and CI rules.**

If that rule is followed, Aequora can remain modular while growing from:

```text
one Rust client + one Postgres server
```

into:

```text
mobile
desktop
multiple databases
multiple languages
enterprise deployments
third-party adapters
```

without turning its core into a framework-specific or database-specific monolith.
