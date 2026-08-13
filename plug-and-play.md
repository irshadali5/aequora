# Aequora Sync — Plug-and-Play Developer Integration Architecture

## Reusable SDK, Adapter Model, Project Integration, Code Generation, Configuration, Testing, and Extension System

> This document defines how **Aequora Sync** becomes a genuinely reusable, plug-and-play synchronization framework that developers can integrate into unrelated Rust projects without understanding its internal distributed-systems machinery.
>
> It builds on the existing architecture:
>
> - database-agnostic synchronization;
> - Axum server validation/execution;
> - local-first client behavior;
> - Postcard data plane and RON configuration;
> - ACID local/server boundaries;
> - transactional outbox;
> - authoritative journal;
> - idempotency;
> - conflict handling;
> - snapshots;
> - production deployment;
> - Stoolap and PostgreSQL/Neon adapters.
>
> The goal of this layer is:
>
> **A developer should be able to add Aequora to a project by implementing domain-specific operations and choosing adapters, while Aequora supplies the synchronization mechanics, correctness, migrations, retries, protocol, diagnostics, and lifecycle.**

---

# 1. Plug-and-Play Goal

A developer should not need to implement:

```text
outbox tables
cursor tables
operation ledger
journal schema
retry loops
sync state machine
HTTP protocol
Postcard framing
idempotency
snapshot plumbing
resync logic
device registration
conflict plumbing
migration bookkeeping
```

They should mainly provide:

```text
what an operation means
who may execute it
how it validates
how it mutates domain state
how conflicts are resolved
```

That is the core usability boundary.

---

# 2. Desired Developer Experience

A minimal client integration should look approximately like:

```rust
let aequora = AequoraClient::builder()
    .store(StoolapStore::open("app.db")?)
    .transport(HttpTransport::new(server_url))
    .config(ClientProfile::production())
    .build()
    .await?;

aequora.start().await?;
```

A minimal server integration:

```rust
let registry = OperationRegistry::builder()
    .register(CreateStudentHandler::new(student_service))
    .register(UpdateStudentHandler::new(student_service))
    .build()?;

let aequora = AequoraServer::builder()
    .store(PostgresStore::new(pool))
    .registry(registry)
    .authorizer(authorizer)
    .config(ServerProfile::production())
    .build()
    .await?;

let app = Router::new()
    .merge(aequora_axum::routes(aequora));
```

The integration should remain explicit enough to be understandable, but small enough to avoid boilerplate.

---

# 3. Architecture Philosophy

Plug-and-play does **not** mean hiding all architecture.

It means:

```text
safe defaults
+
small integration surface
+
explicit extension points
+
generated boilerplate
+
strong compile-time contracts
```

Do not use runtime magic where Rust's type system can make integration safer.

---

# 4. Layered SDK Architecture

Aequora should be divided into five developer-facing layers:

```text
Layer 1: Core Protocol
Layer 2: Runtime Engines
Layer 3: Storage/Transport Adapters
Layer 4: Domain Integration SDK
Layer 5: Framework/Tooling Integration
```

---

# 5. Layer 1 — Core Protocol

Developers should rarely touch this directly.

Contains:

```text
OperationId
DeviceId
EntityId
EntityVersion
Cursor
SyncRequest
SyncResponse
AuthoritativeChange
Conflict
ProtocolVersion
SchemaVersion
```

Crate:

```text
aequora-core
aequora-protocol
aequora-types
```

---

# 6. Layer 2 — Runtime Engines

Contains:

```text
AequoraClient
AequoraServer
SyncCoordinator
Reconciler
ExecutionPlanner
ConflictEngine
BootstrapEngine
```

Developers interact mainly through builders and service traits.

---

# 7. Layer 3 — Adapters

Concrete implementations:

```text
aequora-store-stoolap
aequora-store-postgres
aequora-transport-http
aequora-axum
```

Future:

```text
aequora-store-sqlite
aequora-store-redb
aequora-transport-quic
```

Adapters implement stable capability traits.

---

# 8. Layer 4 — Domain Integration SDK

This is the most important plug-and-play layer.

It should provide:

```text
operation declaration
handler registration
authorization hooks
validation hooks
execution hooks
conflict policy hooks
entity/version mapping
transaction helpers
```

Crate:

```text
aequora-domain
```

or exposed through the facade crate.

---

# 9. Layer 5 — Tooling

Developer tooling:

```text
aequora-cli
aequora-admin
aequora-migrate
aequora-inspect
aequora-testkit
```

These reduce manual integration effort.

---

# 10. Facade Crate

Developers should normally depend on:

```toml
aequora = { version = "...", features = ["client", "stoolap", "http"] }
```

or:

```toml
aequora = { version = "...", features = ["server", "postgres", "axum"] }
```

But internally, dependencies stay split across crates.

---

# 11. Preludes

Provide carefully scoped preludes:

```rust
use aequora::client::prelude::*;
```

and:

```rust
use aequora::server::prelude::*;
```

Avoid one giant global prelude that pollutes namespaces.

---

# 12. Project Integration Modes

Support three integration styles.

## Mode A — Explicit

Developer wires every major component.

Best for complex systems.

## Mode B — Convention-Based

Aequora generates/configures standard metadata and routes.

Best default.

## Mode C — Embedded Profile

Single call configures common local-first setup.

Best for small apps, demos, and early adoption.

---

# 13. Convention-Based Defaults

If developer chooses:

```text
Stoolap client
PostgreSQL server
Axum transport
Postcard
```

Aequora can automatically provide:

```text
metadata migrations
standard table names
default indexes
default retry policy
default protocol routes
default health routes
default tracing spans
```

No need to manually define them.

---

# 14. Generated Metadata Schema

Storage adapters should own their metadata migrations.

Example:

```text
aequora_store_stoolap::migrations()
```

and:

```text
aequora_store_postgres::migrations()
```

Developers should not copy SQL from documentation.

---

# 15. Adapter Bootstrap

Ideal:

```rust
let store = StoolapStore::builder()
    .database(db)
    .auto_migrate(true)
    .build()
    .await?;
```

and:

```rust
let store = PostgresStore::builder()
    .pool(pool)
    .migration_mode(MigrationMode::VerifyOnly)
    .build()
    .await?;
```

---

# 16. Migration Ownership

Aequora owns only:

```text
aequora_* metadata
```

Application owns its domain schema.

Never let Aequora rewrite unrelated application tables automatically.

---

# 17. Domain Operation Model

Each application defines its own domain operations.

Example:

```rust
#[derive(Serialize, Deserialize)]
pub enum SchoolOperation {
    Student(StudentOperation),
    Attendance(AttendanceOperation),
    Finance(FinanceOperation),
}
```

Aequora should not require one huge global enum if handlers can be registered independently.

---

# 18. Operation Derive Macro

A derive macro may reduce repetitive metadata.

Example:

```rust
#[derive(
    Serialize,
    Deserialize,
    AequoraOperation
)]
#[aequora(
    kind = 0x0001_0001,
    schema = 1,
    entity = "student"
)]
pub struct CreateStudent {
    pub student_id: StudentId,
    pub name: StudentName,
}
```

Macro can generate:

```text
operation kind
schema version
codec helpers
descriptor
registration glue
```

---

# 19. Macro Philosophy

Macros should generate boilerplate, not hide business logic.

Good:

```text
IDs
descriptors
registration
serialization glue
```

Bad:

```text
implicit SQL
implicit authorization
hidden conflict rules
```

Developers should still write the important semantics explicitly.

---

# 20. Handler Trait

Recommended:

```rust
#[async_trait]
pub trait OperationHandler<O>: Send + Sync
where
    O: AequoraOperation,
{
    type Output;

    async fn authorize(
        &self,
        ctx: &AuthContext,
        op: &O,
    ) -> Result<(), AuthorizationError>;

    async fn validate(
        &self,
        ctx: &ValidationContext<'_>,
        op: &O,
    ) -> Result<(), ValidationError>;

    async fn execute(
        &self,
        ctx: &ExecutionContext<'_>,
        op: O,
        tx: &mut dyn DomainTransaction,
    ) -> Result<Self::Output, ExecutionError>;
}
```

---

# 21. Optional Split Traits

For more complex projects:

```text
Authorizer<O>
Validator<O>
Executor<O>
ConflictResolver<O>
```

can be separate types.

The builder can accept either:

```text
combined handler
```

or:

```text
split handler components
```

---

# 22. Plug-and-Play Default Handler

For simple CRUD-like domain operations:

```rust
impl OperationHandler<UpdateProfile> for UpdateProfileHandler {
    ...
}
```

is sufficient.

Developers should not need to understand the internal registry.

---

# 23. Registration Macro

Optional macro:

```rust
aequora_registry! {
    CreateStudent => CreateStudentHandler,
    UpdateStudent => UpdateStudentHandler,
    PostPayment => PostPaymentHandler,
}
```

Expansion should remain deterministic and compile-time.

---

# 24. Compile-Time Duplicate Operation Detection

Where possible, duplicate operation kind identifiers should be caught:

```text
during compilation
```

or:

```text
at server startup
```

Never only after receiving production traffic.

---

# 25. Entity Model Integration

Developers should define stable sync identity.

Example:

```rust
#[derive(AequoraEntity)]
#[aequora(type_id = 1)]
pub struct Student {
    #[aequora(id)]
    pub id: StudentId,

    #[aequora(version)]
    pub version: EntityVersion,

    ...
}
```

---

# 26. Avoid ORM Coupling

`AequoraEntity` must not imply:

```text
SQL table = entity
```

The derive should provide metadata only.

Database persistence remains application-defined.

---

# 27. Version Adapter

Some projects may keep versions in:

```text
same row
separate metadata table
aggregate root
event stream
```

Provide a trait:

```rust
pub trait VersionedEntity {
    type Id;

    fn id(&self) -> Self::Id;
    fn version(&self) -> EntityVersion;
}
```

---

# 28. Client Mutation API

The most important plug-and-play API is atomic local mutation + outbox.

Recommended:

```rust
aequora
    .transaction()
    .run(|ctx| async move {
        app_repo.update_student(ctx.store(), student).await?;

        ctx.enqueue(UpdateStudent {
            ...
        }).await?;

        Ok(())
    })
    .await?;
```

---

# 29. Domain-Aware Helper

A convenience layer can expose:

```rust
sync_mutation!(
    aequora,
    operation,
    |tx| async {
        repo.update(tx, ...)
    }
)
```

But ordinary functions should remain available for users who avoid macros.

---

# 30. Local Repository Independence

Aequora must not require application repositories to implement one specific ORM.

A project can use:

```text
raw Stoolap
custom repository
SQL builder
domain storage service
```

as long as the transaction adapter can coordinate the outbox.

---

# 31. Transaction Interop Contract

The client adapter should expose a transaction handle usable by application repositories.

Conceptually:

```rust
pub trait LocalAppTransaction {
    type Native;

    fn native(&mut self) -> &mut Self::Native;
}
```

However, exposing native DB types in generic Aequora API risks coupling.

Better adapter-specific extension traits can expose them.

---

# 32. Adapter Extension Traits

Example:

```rust
pub trait StoolapTransactionExt {
    fn stoolap_tx(&mut self) -> &mut stoolap::Transaction;
}
```

This keeps core generic while permitting ergonomic integration.

---

# 33. Server Transaction Interop

Server handlers may need application repositories using SQLx.

Provide adapter-specific extension:

```rust
pub trait PostgresTransactionExt {
    fn postgres_tx(&mut self) -> &mut sqlx::Transaction<'_, Postgres>;
}
```

Core traits remain database-independent.

---

# 34. Repository Pattern

Recommended application design:

```text
domain handler
    ↓
repository trait
    ↓
adapter implementation
```

Then Aequora handlers need not know SQLx/Stoolap directly.

This is the cleanest long-term architecture.

---

# 35. Authorizer Plug-In

Projects should provide:

```rust
impl Authorizer<CreateStudent> for SchoolAuthorizer
```

Aequora supplies:

```text
auth context
tenant context
device context
operation metadata
```

Application supplies permission semantics.

---

# 36. Default Authorization Policy

Aequora should have no permissive "allow everything" production default.

Development can provide:

```text
AllowAllAuthorizer
```

behind:

```text
dev/test feature
```

Production should require explicit authorizer or trusted integration.

---

# 37. Conflict Policies as Plug-Ins

Developers register per-operation or per-entity strategies.

Example:

```rust
registry
    .register_conflict_policy::<UpdateProfile>(FieldMergePolicy::default())
    .register_conflict_policy::<PostPayment>(RejectStale::new());
```

---

# 38. Built-In Conflict Strategies

Provide reusable safe primitives:

```text
RejectStale
ServerWins
ClientWins
LastWriterWins
FieldMerge
Commutative
Manual
```

But do not make a single universal default.

Recommended default for unknown mutable domain data:

```text
RejectStale
```

because silent data loss is worse than explicit conflict.

---

# 39. Schema Upcaster Plug-In

Example:

```rust
registry.register_upcaster::<CreateStudentV1, CreateStudentV2>(...);
```

or:

```rust
impl Upcast<CreateStudentV1> for CreateStudentV2
```

Aequora runs required transformations before handler execution.

---

# 40. Postcard by Default

Operation payload:

```text
Postcard
```

Configuration:

```text
RON
```

Developer should not configure codecs for ordinary use.

Advanced projects may swap codec implementations through trait abstractions.

---

# 41. Protocol Route Installation

`aequora-axum` should install standard routes automatically:

```text
POST /sync/v1/exchange
POST /sync/v1/bootstrap
GET  /sync/v1/health
```

Example:

```rust
Router::new()
    .nest("/api", app_routes)
    .merge(aequora_axum::routes(sync))
```

---

# 42. Route Customization

Support custom base path:

```rust
aequora_axum::RouterConfig::new()
    .base_path("/internal/sync")
```

without rewriting handlers.

---

# 43. Middleware Integration

Aequora Axum routes should accept user middleware.

Examples:

```text
auth
rate limiting
request IDs
CORS if applicable
compression
tracing
```

Do not force a proprietary middleware stack.

---

# 44. Auth Extractor Bridge

Provide a trait:

```rust
pub trait AuthContextExtractor {
    async fn extract(parts: &Parts) -> Result<AuthContext, AuthError>;
}
```

Axum integration uses the application implementation.

---

# 45. Device Identity Plug-and-Play

Client should have a device identity provider.

Default:

```text
generate UUIDv7 on first run
persist in local metadata
reuse forever until reset
```

Trait:

```rust
pub trait DeviceIdentityProvider {
    async fn device_id(&self) -> Result<DeviceId, IdentityError>;
}
```

---

# 46. Credentials Provider

Transport should receive credentials through abstraction:

```rust
pub trait CredentialProvider {
    async fn auth_header(&self) -> Result<SecretString, AuthError>;
}
```

This allows:

```text
JWT
session token
OIDC refresh
API token
```

without coupling core sync to one auth system.

---

# 47. Automatic Token Refresh

HTTP transport may call the credential provider when:

```text
token expired
```

The provider owns refresh semantics.

Aequora should distinguish:

```text
transport unauthorized
```

from:

```text
business authorization rejected
```

---

# 48. Client Lifecycle API

Recommended:

```rust
let handle = aequora.start().await?;
```

Handle exposes:

```text
sync_now()
pause()
resume()
status()
shutdown()
```

---

# 49. No Mandatory Global Runtime

Aequora may require Tokio, but should not create uncontrolled global runtimes.

Applications pass/use existing Tokio runtime.

Desktop and server projects then share lifecycle ownership.

---

# 50. Status Subscription

Provide:

```rust
let mut status = aequora.subscribe_status();
```

Events:

```rust
SyncStatus::Offline
SyncStatus::Idle
SyncStatus::Syncing
SyncStatus::Conflict
SyncStatus::UpgradeRequired
```

Dioxus or any other UI can bridge this into its own reactive system.

---

# 51. Event Hooks

Optional hooks:

```rust
on_sync_started
on_sync_completed
on_conflict
on_rejection
on_resync
```

Use them for application behavior, not correctness.

Hooks must not be required for durable state.

---

# 52. Extension Event Bus

A bounded internal event stream can expose:

```text
diagnostic events
lifecycle events
metrics events
```

without allowing plugins to mutate core synchronization state directly.

---

# 53. No Arbitrary Runtime Plugins in v1

"Plug-and-play" should mean Rust crates and trait implementations, not dynamic shared-library loading.

Dynamic plugins create:

```text
ABI issues
security issues
versioning complexity
deployment complexity
```

Use compile-time composition.

---

# 54. Cargo Feature Profiles

Example:

```toml
[dependencies]
aequora = {
    version = "1",
    features = [
        "client",
        "stoolap",
        "http",
        "postcard"
    ]
}
```

Server:

```toml
aequora = {
    version = "1",
    features = [
        "server",
        "postgres",
        "axum",
        "postcard"
    ]
}
```

---

# 55. Keep Features Orthogonal

Avoid feature combinations like:

```text
stoolap-postgres-axum-special-mode
```

Features should represent independent capabilities.

---

# 56. Starter Templates

Provide project templates:

```text
aequora-client-stoolap
aequora-server-postgres
aequora-fullstack-dioxus-axum
```

Generated through:

```text
cargo generate
```

or an `aequora init` command.

---

# 57. `aequora init`

CLI should offer:

```text
aequora init
```

Interactive choices:

```text
Client DB:
    Stoolap
    Custom

Server DB:
    PostgreSQL
    Custom

Transport:
    Axum HTTP

Config:
    RON

Auth:
    Custom bridge
```

Then generate boilerplate.

---

# 58. Generated Project Files

Example:

```text
sync/
├── mod.rs
├── client.rs
├── server.rs
├── operations/
├── conflicts.rs
├── auth.rs
└── config.ron
```

It should also add required dependencies and starter migrations.

---

# 59. `aequora doctor`

Provide:

```text
aequora doctor
```

Checks:

```text
Aequora versions
Rust version
adapter compatibility
DB connectivity
metadata migration state
protocol compatibility
config validity
operation registry duplicates
```

This significantly improves developer experience.

---

# 60. `aequora migrate`

Commands:

```text
aequora migrate status
aequora migrate apply
aequora migrate verify
```

Aequora metadata migrations only.

Application migrations remain separate.

---

# 61. `aequora inspect`

Inspect Postcard protocol:

```text
aequora inspect packet.bin --to ron
```

Inspect stored operation:

```text
aequora inspect operation <id>
```

Developer should never need to decode binary payloads manually.

---

# 62. TestKit Plug-and-Play

The test kit should allow:

```rust
let sim = AequoraTestHarness::new()
    .client()
    .server()
    .build();

sim.disconnect();
sim.client().mutate(...);
sim.reconnect();
sim.sync().await?;
sim.assert_converged();
```

---

# 63. In-Memory Development Store

Provide:

```text
InMemoryLocalStore
InMemoryAuthoritativeStore
```

for tests and examples.

Never recommend them for production.

---

# 64. Fake Transport

`InProcessTransport` should bypass HTTP and call server engine directly.

Useful for:

```text
unit/integration tests
benchmarks
CI
```

The same protocol structures remain used.

---

# 65. Fault-Injecting Transport

Test:

```text
drop next response
duplicate request
delay request
disconnect
corrupt bytes
```

through:

```rust
FaultyTransport
```

---

# 66. Compliance Test Macros

Adapter developers should be able to run:

```rust
aequora_local_store_compliance!(MyStoreFactory);
```

and:

```rust
aequora_authoritative_store_compliance!(MyServerStoreFactory);
```

These generate the standard behavioral test suite.

---

# 67. Adapter Developer SDK

Crate:

```text
aequora-adapter-sdk
```

Contains:

```text
required traits
compliance tests
migration helpers
transaction capability contracts
error conversion helpers
```

---

# 68. Storage Adapter Contract

A local adapter implements:

```text
LocalTransactionFactory
OutboxStore
CursorStore
ConflictStore
InboxStore
MetadataStore
```

Server adapter:

```text
AuthoritativeTransactionFactory
JournalStore
OperationLedger
SnapshotStore
ScopeStore
```

---

# 69. Adapter Capability Declaration

Each adapter reports:

```rust
AdapterCapabilities {
    durable_transactions,
    savepoints,
    snapshot_consistency,
    max_parameter_count,
    native_upsert,
    ...
}
```

The runtime can validate required capabilities at startup.

---

# 70. Capability Gating

If an application requires:

```text
atomic snapshot install
```

but adapter cannot provide it:

```text
build/startup should fail
```

rather than degrade silently.

---

# 71. Adapter Error Normalization

Adapters map native errors into:

```text
Transient
Conflict
ConstraintViolation
Corruption
Unavailable
FatalConfiguration
```

The core runtime should not parse database error strings.

---

# 72. Custom Database Support

A project with an uncommon DB should not fork Aequora.

They implement:

```text
LocalSyncStore
```

or:

```text
AuthoritativeSyncStore
```

using the adapter SDK.

---

# 73. Transport Adapter SDK

Transport contract:

```rust
pub trait SyncTransport {
    async fn exchange(
        &self,
        request: SyncRequest,
    ) -> Result<SyncResponse, TransportError>;

    async fn bootstrap(
        &self,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, TransportError>;
}
```

Custom transport implementations can use:

```text
HTTP
QUIC
IPC
embedded in-process
```

---

# 74. Transport Correctness Requirements

Aequora assumes transport may:

```text
fail
timeout
duplicate
lose response
```

Therefore adapters need not promise exactly-once delivery.

Idempotency belongs in core.

---

# 75. Server Framework Independence

Although Axum is the first integration, core server API should be framework-neutral.

Other frameworks can call:

```rust
server.exchange(auth, request).await
```

directly.

---

# 76. Dioxus Independence

No Dioxus dependency in core or client runtime.

Provide optional helper crate:

```text
aequora-dioxus
```

with:

```text
hooks/signals/status bridge
```

if useful.

---

# 77. Dioxus Helper

Potential API:

```rust
let sync_state = use_aequora_status(aequora.clone());
```

This belongs outside `aequora-client`.

---

# 78. Project Module Boundaries

Recommended application integration:

```text
my-app/
├── domain/
├── persistence/
├── sync/
│   ├── operations.rs
│   ├── handlers.rs
│   ├── conflicts.rs
│   ├── auth.rs
│   └── mod.rs
├── client/
└── server/
```

Aequora code should not spread across every feature module unnecessarily.

---

# 79. Domain Co-Location Alternative

Large systems may colocate:

```text
student/
├── domain.rs
├── repository.rs
├── sync.rs
```

Both layouts should be supported.

---

# 80. Operation Registration Inventory

At build/startup, Aequora should be able to print:

```text
Registered operations:
0x00010001 CreateStudent schema 1..2
0x00010002 UpdateStudent schema 1
0x00030001 PostPayment schema 3
```

Useful for deployment diagnostics.

---

# 81. Static Protocol Manifest

Generate a protocol manifest:

```text
aequora-protocol-manifest.ron
```

containing:

```text
operation kinds
schemas
capabilities
conflict policies
```

This can be compared in CI.

---

# 82. Compatibility CI

Command:

```text
aequora compat check old-manifest.ron new-manifest.ron
```

Detect:

```text
removed operation
schema narrowing
kind reuse
incompatible protocol change
```

before release.

---

# 83. Prevent Operation Kind Reuse

Once:

```text
0x0001_0001
```

has represented `CreateStudent`, it must never later represent another operation.

Keep a tombstoned registry of retired IDs.

---

# 84. Developer Error Messages

Compile/startup diagnostics should explain fixes.

Bad:

```text
Invalid registry
```

Good:

```text
OperationKind 0x00010001 is registered by both CreateStudent and ImportStudent.
Assign a unique stable operation kind.
```

---

# 85. Safe Development Mode

Development profile may provide:

```text
verbose RON dumps
allow-all auth
auto migrations
small in-memory adapters
```

But these must be clearly unavailable or loudly warned in production profile.

---

# 86. Production Profile Guardrails

Production build/startup should reject:

```text
AllowAllAuthorizer
unsafe durability
unbounded request limits
missing auth integration
debug payload logging
```

unless explicit override is provided.

---

# 87. Typed Configuration

Prefer enums/newtypes:

```rust
enum MigrationMode {
    VerifyOnly,
    MigrateOnStart,
    External,
}
```

instead of strings.

RON remains human-readable but deserializes into typed validated config.

---

# 88. Config Schema Documentation

Generate documentation from config types.

Command:

```text
aequora config schema
```

or:

```text
aequora config example
```

Avoid stale hand-written configuration docs.

---

# 89. Default Profiles as Code

Profiles should be Rust values:

```rust
ClientProfile::development()
ClientProfile::production()
ServerProfile::small_production()
ServerProfile::enterprise()
```

RON can override them.

---

# 90. Plug-and-Play Bootstrap Migrations

Client startup:

```text
open local DB
↓
read Aequora metadata version
↓
apply safe local metadata migrations
↓
recover DeviceId
↓
recover cursor/outbox
↓
start coordinator
```

This should require no manual SQL.

---

# 91. Plug-and-Play Server Startup

Server startup:

```text
connect DB
↓
verify/apply Aequora metadata migrations
↓
validate registry
↓
validate auth bridge
↓
validate adapter capabilities
↓
construct routes
↓
ready
```

---

# 92. First-Sync Experience

New client:

```text
no DeviceId
no cursor
no local snapshot
```

Aequora automatically:

```text
creates DeviceId
negotiates protocol
bootstraps scope
sets cursor
enters incremental mode
```

---

# 93. Developer-Controlled Scope Resolver

Application implements:

```rust
pub trait ScopeResolver {
    async fn resolve(
        &self,
        auth: &AuthContext,
        request: &ScopeRequest,
    ) -> Result<SyncScope, ScopeError>;
}
```

Aequora handles cursor/bootstrap mechanics after scope is resolved.

---

# 94. Multi-Tenant Convenience Layer

Optional `TenantContext`:

```rust
pub struct TenantContext {
    pub tenant_id: TenantId,
    pub actor_id: ActorId,
    pub device_id: DeviceId,
}
```

Handlers receive it automatically.

---

# 95. Multi-Module Registry

Large applications can compose registries:

```rust
let registry = OperationRegistry::merge([
    student::sync_registry(),
    attendance::sync_registry(),
    finance::sync_registry(),
]);
```

This keeps modules independent.

---

# 96. Library Reuse Across Projects

The reusable Aequora repository should contain no school-specific modules.

Project-specific crates depend on Aequora:

```text
school-erp-sync
finance-app-sync
messaging-sync
field-service-sync
```

All reuse the same core.

---

# 97. Domain-Specific Extension Crates

A company can build shared internal crates:

```text
company-aequora-auth
company-aequora-observability
company-aequora-policies
```

without modifying Aequora itself.

---

# 98. Semver Policy

Stable:

```text
public traits
wire protocol contracts
persistent metadata invariants
operation ID rules
```

Internal implementation can evolve behind these boundaries.

---

# 99. Avoid Trait Explosion in Public API

Too many tiny public traits make integration difficult.

Expose a small set of top-level traits.

Use internal/private traits for implementation detail.

---

# 100. Builder Validation

`.build()` should validate:

```text
store configured
transport configured
auth configured where required
duplicate operation kinds absent
config valid
required adapter capabilities available
```

Fail early.

---

# 101. Compile-Time vs Runtime Validation

Use compile-time for:

```text
type correctness
trait requirements
derive metadata shape
```

Use startup validation for:

```text
duplicate numeric IDs
DB capability
migration state
config semantics
```

Use request-time validation only for dynamic data.

---

# 102. Error Taxonomy for Integrators

Separate:

```text
BuildError
ConfigurationError
MigrationError
AdapterError
ProtocolError
OperationError
RuntimeSyncError
```

Developer should know whether failure is:

```text
integration mistake
configuration mistake
runtime transient failure
domain rejection
```

---

# 103. `thiserror` Public Errors

Library error enums should expose structured variants and sources.

Applications may wrap them with `anyhow` at executable boundaries.

---

# 104. Documentation Generation

Every public integration trait should have:

```text
minimal example
production note
safety/correctness invariant
```

Rustdoc should be sufficient to integrate basic usage.

---

# 105. Examples Repository

Provide examples:

```text
examples/basic-client
examples/basic-server
examples/stoolap-postgres
examples/dioxus-client
examples/custom-store
examples/custom-conflict
examples/multi-tenant
```

Examples should compile in CI.

---

# 106. End-to-End Starter Example

A single example should demonstrate:

```text
client local write
offline mode
server startup
sync
multi-device update
conflict
resync
```

Developers learn fastest from one complete working reference.

---

# 107. Testing a Project Integration

Application test:

```rust
#[tokio::test]
async fn student_update_syncs() {
    let env = SchoolSyncHarness::new().await;

    env.client_a().go_offline();
    env.client_a().update_student(...).await;

    env.client_a().go_online();
    env.sync_all().await;

    env.assert_server_student(...);
    env.assert_clients_converged();
}
```

---

# 108. Reusable Scenario DSL

TestKit can provide:

```text
offline(client)
mutate(client, op)
sync(client)
duplicate_next_request()
drop_next_response()
restart_server()
assert_converged()
```

This makes distributed failure tests readable.

---

# 109. Project-Specific Test Harness Extension

Projects can wrap AequoraTestHarness:

```rust
struct SchoolSyncHarness {
    inner: AequoraTestHarness,
    ...
}
```

and add domain helpers.

---

# 110. Adapter Conformance Before Publishing

Third-party adapters should publish their compliance result:

```text
Aequora Local Adapter Compliance v1: PASS
```

or:

```text
Experimental
```

This improves ecosystem trust.

---

# 111. Plugin Registry Without Dynamic Plugins

Aequora can maintain a documented ecosystem registry of crates:

```text
store adapters
transport adapters
UI integrations
auth bridges
```

But installation remains ordinary Cargo dependencies.

---

# 112. Naming Convention for Ecosystem Crates

Recommend:

```text
aequora-store-*
aequora-transport-*
aequora-auth-*
aequora-ui-*
```

Third parties can follow:

```text
vendor-aequora-store-xyz
```

to avoid namespace confusion.

---

# 113. Feature Stability Tiers

Mark APIs:

```text
Stable
Experimental
Internal
```

Experimental features:

```text
QUIC
CRDT framework
multi-region
dynamic snapshot backends
```

should not destabilize core plug-and-play use.

---

# 114. Upgrade Experience for Developers

Typical upgrade:

```text
cargo update
↓
aequora compat check
↓
aequora migrate verify
↓
cargo test
↓
deploy
```

No manual protocol reverse-engineering.

---

# 115. Deprecation Policy

Deprecate:

```text
old API
```

before removal.

Provide migration guide:

```text
v1 -> v2
```

with mechanical code changes where possible.

---

# 116. Metadata Schema Compatibility

Metadata migrations should be forward-only by default.

Application rollback relies on expand-contract and declared compatibility windows.

---

# 117. Project Bootstrap Command

Ideal:

```text
aequora add
```

inside an existing Cargo workspace.

It detects:

```text
Axum?
Dioxus?
Stoolap?
SQLx/Postgres?
```

and generates integration modules.

Detection should only suggest; never silently rewrite large portions of code.

---

# 118. Non-Destructive Code Generation

Generated files should:

```text
live in dedicated sync module
```

and avoid overwriting user code.

Use marker-free regeneration or create-once templates.

---

# 119. Generated Code Ownership

Generated starter code becomes application code.

Aequora runtime should not depend on regenerating it every upgrade.

---

# 120. Plug-and-Play Security Defaults

Client:

```text
TLS required in production
bounded payload
token provider required
```

Server:

```text
auth bridge required
request limits enabled
rate limit hooks available
unknown operation rejected
unknown schema rejected
```

---

# 121. Development Security Escape Hatch

Development may use:

```text
http://localhost
AllowAllAuthorizer
```

only when:

```text
profile == Development
```

Production profile should reject these by default.

---

# 122. Minimal Developer Responsibilities

For a normal project, developer should need to decide only:

```text
1. Which local store?
2. Which authoritative store?
3. What are my domain operations?
4. How do I authorize them?
5. How do I validate/execute them?
6. What conflict policy does each need?
7. What sync scope should each user/device receive?
```

Everything else should have safe framework defaults.

---

# 123. Responsibilities Aequora Owns

Aequora owns:

```text
operation envelope
OperationId
device ID persistence
outbox lifecycle
retry/backoff
HTTP exchange
Postcard
protocol negotiation
cursor
journal pull
deduplication
reconciliation
bootstrap
snapshot chunking
resync
sync status
metadata migrations
adapter compliance
diagnostics
```

---

# 124. Responsibilities Application Owns

Application owns:

```text
domain model
domain repositories
authorization
business validation
domain transactions
operation definitions
conflict semantics
scope semantics
user-facing conflict UI
```

---

# 125. Responsibilities Deployment Owns

Deployment owns:

```text
TLS
database provisioning
secrets
backups
monitoring backend
capacity
release rollout
```

Aequora provides hooks and health checks.

---

# 126. Anti-Patterns

Do not require developers to:

```text
manually poll sync tables
manually advance cursors
manually write operation ledger
manually implement retry loop
write raw SQL into protocol
copy migrations by hand
couple UI directly to transport
```

These defeat plug-and-play goals.

---

# 127. Advanced Escape Hatches

Expert users may override:

```text
batch strategy
retry policy
conflict engine
snapshot backend
transport
store adapter
compute executor
clock
telemetry
```

But default integration should not require them.

---

# 128. Small Public Surface

An ideal user-facing API can revolve around:

```text
AequoraClient
AequoraServer
OperationHandler
OperationRegistry
SyncStore adapters
AuthContext
ConflictPolicy
SyncScope
Config/Profile
TestHarness
```

Everything else can remain internal.

---

# 129. Compile-Time Safety Goal

Rust should reject:

```text
handler registered for wrong operation type
missing trait implementation
wrong ID type
executing unvalidated operation internally
invalid adapter type combinations
```

where practical.

---

# 130. Startup Safety Goal

Startup should reject:

```text
duplicate OperationKind
migration mismatch
unsupported adapter capability
unsafe production profile
missing auth
invalid config
incompatible protocol manifest
```

---

# 131. Runtime Safety Goal

Runtime should handle:

```text
offline
timeouts
duplicate delivery
response loss
server restart
client restart
stale state
conflict
cursor expiry
```

without developer-written recovery logic.

---

# 132. Example Client Integration

```rust
use aequora::client::prelude::*;
use aequora_store_stoolap::StoolapStore;
use aequora_transport_http::HttpTransport;

pub async fn start_sync(
    db: stoolap::Database,
    server: Url,
    credentials: AppCredentials,
) -> anyhow::Result<AequoraClientHandle> {
    let store = StoolapStore::builder()
        .database(db)
        .migration_mode(MigrationMode::Auto)
        .build()
        .await?;

    let transport = HttpTransport::builder()
        .server(server)
        .credentials(credentials)
        .build()?;

    let client = AequoraClient::builder()
        .store(store)
        .transport(transport)
        .profile(ClientProfile::production())
        .build()
        .await?;

    Ok(client.start().await?)
}
```

The exact API may evolve, but integration complexity should remain close to this level.

---

# 133. Example Operation

```rust
#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    AequoraOperation,
)]
#[aequora(
    kind = 0x0001_0002,
    schema = 1,
    entity = "student"
)]
pub struct UpdateStudentPhone {
    pub student_id: StudentId,
    pub phone: PhoneNumber,
}
```

---

# 134. Example Handler

```rust
pub struct UpdateStudentPhoneHandler<R> {
    repo: R,
}

#[async_trait]
impl<R> OperationHandler<UpdateStudentPhone>
    for UpdateStudentPhoneHandler<R>
where
    R: StudentRepository + Send + Sync,
{
    type Output = ();

    async fn authorize(
        &self,
        ctx: &AuthContext,
        op: &UpdateStudentPhone,
    ) -> Result<(), AuthorizationError> {
        // project-specific authorization
        Ok(())
    }

    async fn validate(
        &self,
        ctx: &ValidationContext<'_>,
        op: &UpdateStudentPhone,
    ) -> Result<(), ValidationError> {
        // project-specific domain validation
        Ok(())
    }

    async fn execute(
        &self,
        ctx: &ExecutionContext<'_>,
        op: UpdateStudentPhone,
        tx: &mut dyn DomainTransaction,
    ) -> Result<(), ExecutionError> {
        self.repo
            .update_phone(tx, op.student_id, op.phone)
            .await?;

        Ok(())
    }
}
```

---

# 135. Example Server Registration

```rust
let registry = OperationRegistry::builder()
    .register(
        UpdateStudentPhoneHandler::new(student_repo.clone())
    )
    .register(
        PostPaymentHandler::new(finance_repo.clone())
    )
    .build()?;
```

No manual switch statement.

---

# 136. Example Local Mutation

```rust
client
    .transaction()
    .run(|tx| async move {
        student_repo
            .update_phone(
                tx.store(),
                student_id,
                phone.clone(),
            )
            .await?;

        tx.enqueue(UpdateStudentPhone {
            student_id,
            phone,
        })
        .await?;

        Ok(())
    })
    .await?;
```

This automatically satisfies the local outbox ACID rule.

---

# 137. Example Dioxus Bridge

Optional helper:

```rust
let status = use_aequora_status(sync.clone());
```

UI can render:

```text
Synced
3 pending
Offline
Conflict
```

without understanding journal/cursor internals.

---

# 138. Example Integration Test

```rust
#[tokio::test]
async fn phone_update_converges_after_offline_period() {
    let env = SchoolHarness::new().await;

    env.client_a().disconnect();

    env.client_a()
        .update_student_phone(student_id, phone)
        .await;

    env.client_a().connect();

    env.sync_all().await;

    env.assert_server_phone(student_id, phone).await;
    env.assert_converged().await;
}
```

---

# 139. Plug-and-Play Architecture Diagram

```text
                     APPLICATION DEVELOPER

        ┌────────────────────────────────────┐
        │ Defines                           │
        │                                    │
        │ Domain Operations                  │
        │ Handlers                           │
        │ Authorization                      │
        │ Conflict Policies                  │
        │ Scope Resolver                     │
        └──────────────────┬─────────────────┘
                           │
                           ▼
                 Aequora Domain SDK
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
          Client API    Server API    TestKit
              │            │
              ▼            ▼
       Local Store     Authoritative Store
          Adapter          Adapter
              │            │
          Stoolap       PostgreSQL
              │            │
              └─────┬──────┘
                    ▼
               Aequora Core
                    │
             Postcard Protocol
                    │
          HTTP/Axum Transport
```

---

# 140. Plugin/Adapter Architecture Diagram

```text
                       Aequora Core
                           │
        ┌──────────────────┼──────────────────┐
        ▼                  ▼                  ▼
   Store Traits       Transport Traits    Domain Traits
        │                  │                  │
   ┌────┴─────┐       ┌────┴────┐       ┌────┴─────┐
   ▼          ▼       ▼         ▼       ▼          ▼
Stoolap   PostgreSQL HTTP      QUIC   ERP Ops   Other Ops
```

No project-specific code enters the core.

---

# 141. "Pit of Success" Design

The safest integration should require the least code.

Easy:

```text
atomic local mutation
safe retry
server transaction
conflict rejection
automatic migrations
automatic resync
```

Hard/explicit:

```text
unsafe durability
manual cursor manipulation
custom transaction semantics
allow-all auth
unbounded resources
```

This is the correct usability philosophy.

---

# 142. Recommended Initial Plug-and-Play Deliverables

Implement in this order:

```text
1. facade crate
2. client/server builders
3. OperationHandler trait
4. operation derive macro
5. registry
6. Stoolap automatic metadata migrations
7. PostgreSQL automatic metadata migrations
8. Axum route installer
9. credential/auth bridge
10. status subscription
11. TestKit harness
12. adapter compliance macros
13. aequora doctor
14. aequora inspect
15. protocol manifest compatibility checker
16. project templates
```

---

# 143. What to Delay

Do not initially build:

```text
dynamic plugins
WASM handler plugins
runtime-loaded shared libraries
code marketplace
automatic ORM generation
magic conflict inference
automatic business authorization
```

They add complexity without improving the core integration story.

---

# 144. Final Developer Contract

Aequora should make this promise:

> **Choose adapters, register domain operations, implement business semantics, and run. Aequora owns the distributed synchronization machinery.**

The developer should not need to understand or manually implement:

```text
network retry ambiguity
deduplication races
journal cursors
bootstrap generations
outbox state recovery
lost ACKs
snapshot recovery
```

to use the library correctly.

---

# 145. Final Recommendation

For Aequora to become genuinely reusable across your projects and by other Rust developers, prioritize the following above adding more synchronization algorithms:

```text
excellent builders
small public API
automatic metadata migrations
operation derive macros
explicit handler traits
adapter SDK
compliance tests
test simulator
protocol manifest
excellent diagnostics
strong startup validation
safe production profiles
working examples
```

The final architecture should feel like:

```text
Cargo dependency
+
choose adapter
+
define operations
+
register handlers
+
start client/server
```

while internally retaining all of the strict architecture already designed:

```text
ACID
idempotency
outbox
journal
versioning
cursoring
conflict detection
bootstrap
retry
security
observability
```

That separation—**simple outside, rigorous inside**—is what makes Aequora a true plug-and-play synchronization library rather than merely a reusable internal framework.

---

# 146. Implementation Status

The reusable SDK path now includes the facade crate, focused `client::prelude` and
`server::prelude` modules, `AequoraClient::builder()` and `AequoraServer::builder()` entry points,
typed handlers and registry, the `AequoraOperation` derive, automatic built-in metadata migrations,
Axum route installers, status subscriptions, deterministic TestKit stores/transports/faults,
public adapter conformance contracts, versioned adapter manifests, and fail-closed production
adapter-pair verification.

`aequora-cli` provides payload-free adapter doctor, manifest inspection, and pair verification.
Database migration commands are intentionally absent until canonical export/import contracts are
implemented; the CLI does not present a placeholder as a safe migration tool.

The CLI also provides `aequora init <new-directory> <client|server>`. Generation is
non-destructive: the parent must already exist, the target must not exist, files are assembled in a
temporary sibling, and the complete starter is renamed into place. Client starters contain no
credentials; server starters contain no allow-all authentication.

Production assembly can call `build_production()` on client and server type-state builders. These
paths validate the selected adapter manifest before returning a runtime. Explicit `build()` remains
available for volatile TestKit/reference stores and advanced non-production construction.

The derive accepts stable `u16` `kind` and non-zero `schema` metadata. Optional `entity` metadata is
documentation only and never couples a domain operation to a table name.

The exact section map and tooling still required before this prerequisite is fully closed are in
[`docs/plug-and-play-completion.md`](docs/plug-and-play-completion.md). In particular, project
templates and the migration command surface are not claimed merely because internal developer
checks or static adapter inspection exist.
