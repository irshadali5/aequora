# Aequora Sync — Part 43

# Configuration, Secrets, Environment Profiles, Runtime Policy, and Feature-Flag Architecture

## 1. Purpose

Aequora now has concrete server, local-storage, UI, transport, and CLI architectures. The next required system boundary is configuration.

Configuration affects nearly every deployment concern:

```text
database connections
network listeners
authentication
sync limits
batch sizing
timeouts
retention
snapshots
scheduler behavior
logging
feature rollout
security policy
mobile/desktop behavior
```

Poor configuration architecture creates:

```text
secret leaks
environment drift
unsafe production defaults
irreproducible incidents
surprising runtime behavior
```

The central rule is:

> **Configuration selects behavior; it must never redefine Aequora's correctness invariants.**

No configuration switch may turn off:

```text
idempotency
authorization
Tx A atomicity
Tx B atomicity
Tx C atomicity
cursor correctness
tenant isolation
authority fencing
required audit semantics
```

---

# 2. Major Configuration Classes

Separate configuration into five classes:

```text
Static Build Configuration
Deployment Configuration
Secret Material
Runtime Operational Policy
Tenant/Application Policy
```

These should not be represented by one unrestricted map.

---

# 3. Static Build Configuration

Examples:

```text
Cargo features
compiled adapters
compiled platform integrations
supported crypto providers
optional diagnostics
FFI/bindings
```

These are fixed at compile time.

---

# 4. Deployment Configuration

Examples:

```text
HTTP bind address
PostgreSQL connection pool
local store path
snapshot object store
trusted proxies
TLS mode
control-plane listener
```

Usually loaded at process startup.

---

# 5. Secret Material

Examples:

```text
database password
JWT signing key
OIDC client secret
API credentials
KMS credentials
TLS private key
admin bootstrap token
```

Secrets must not be ordinary RON configuration values by default.

---

# 6. Runtime Operational Policy

Examples:

```text
batch size
admission limits
timeouts
scheduler policy
snapshot concurrency
consumer concurrency
logging level
maintenance windows
```

Some can change at runtime.

---

# 7. Tenant / Application Policy

Examples:

```text
enabled application modules
scope policy
retention policy
business feature availability
subscription entitlement
```

These belong to product/domain configuration, not the infrastructure config parser.

---

# 8. Recommended Crates

```text
aequora-config
aequora-secrets
aequora-policy
aequora-feature-flags
```

Possible structure:

```text
crates/foundation/
├── aequora-config/
├── aequora-secrets/
└── aequora-policy/
```

---

# 9. Preferred Formats

Aequora preferences:

```text
RON      → human-edited configuration
Postcard → compact internal persisted policy snapshots/artifacts
JSON     → external/API interoperability only where needed
```

RON is the preferred human configuration language.

---

# 10. Why RON

RON works well for Rust-centric configuration because it offers:

```text
typed-like structure
enums
comments
readability
Serde support
```

Example:

```ron
server: (
    bind: "0.0.0.0:8443",
    environment: Production,
)
```

---

# 11. Strongly Typed Config

Do not deserialize into:

```text
HashMap<String, Value>
```

for core settings.

Prefer:

```rust
pub struct ServerConfig {
    pub environment: Environment,
    pub http: HttpConfig,
    pub authority: AuthorityConfig,
    pub sync: SyncConfig,
}
```

---

# 12. Newtypes for Configuration

Use strong types:

```text
BatchSize
ConnectionLimit
Timeout
ByteLimit
Port
RetentionDuration
RetryCount
Percentage
```

to prevent accidental unit/value confusion.

---

# 13. Checked Construction

Deserialize raw forms, then validate into trusted configuration:

```text
RawConfig
↓
validate
↓
ValidatedConfig
```

Do not let unvalidated values propagate into services.

---

# 14. Example Typestate

Concept:

```rust
RawConfig
    -> ParsedConfig
    -> ValidatedConfig
    -> EffectiveConfig
```

Only `EffectiveConfig` reaches composition roots.

---

# 15. Configuration Sources

Recommended precedence:

```text
compiled defaults
↓
base RON file
↓
environment-specific RON
↓
environment variables
↓
explicit command-line overrides
↓
runtime policy source
```

Secrets use a separate resolver.

---

# 16. Precedence Must Be Deterministic

Every setting should have one clear winner.

The CLI should be able to explain:

```text
setting
effective value
source
```

---

# 17. Effective Configuration

`aequora config effective` may show:

```text
http.bind = 0.0.0.0:8443
source = /etc/aequora/production.ron
```

Secrets show only:

```text
database.password = <redacted>
source = environment:AEQUORA_DB_PASSWORD
```

---

# 18. Environment Profiles

Recommended enum:

```rust
pub enum Environment {
    Development,
    Test,
    Staging,
    Production,
}
```

Do not use arbitrary strings everywhere.

---

# 19. Profiles Are Not Security Boundaries

Setting:

```text
environment = Production
```

does not create security by itself.

Security still comes from:

```text
authentication
authorization
TLS
KMS
network policy
permissions
```

---

# 20. Production Defaults

Production profile should fail closed.

Examples:

```text
debug endpoints off
test auth off
unsafe reset off
verbose payload logging off
fault injection off
dev TLS bypass off
```

---

# 21. Development Defaults

Development may enable:

```text
human logs
local endpoints
test fixtures
developer diagnostics
```

but should still preserve correctness invariants.

---

# 22. Test Profile

Test configuration can enable:

```text
deterministic clock
fake transport
fault injection
temporary stores
reference adapters
```

only in test builds.

---

# 23. Staging

Staging should be production-like:

```text
same security defaults
same migrations
same protocol
same adapter profiles
```

with non-production data.

---

# 24. Secrets Architecture

Use:

```text
SecretRef
```

instead of storing secrets directly in ordinary config.

Example:

```ron
database: (
    password: Env("AEQUORA_DB_PASSWORD"),
)
```

---

# 25. `SecretRef`

Concept:

```rust
pub enum SecretRef {
    Environment(String),
    File(PathBuf),
    OsStore(String),
    Provider(SecretProviderRef),
}
```

Actual API should avoid exposing secret values unnecessarily.

---

# 26. Secret Value Type

Use a redacting wrapper:

```rust
SecretString
SecretBytes
```

whose `Debug` output is:

```text
<redacted>
```

---

# 27. Never Derive Debug Unsafely

Do not accidentally derive:

```rust
Debug
```

for structs containing plaintext secrets unless wrappers guarantee redaction.

---

# 28. Environment Variables

Useful for:

```text
container deployment
CI
simple secret injection
```

Examples:

```text
AEQUORA_DATABASE_URL
AEQUORA_OIDC_CLIENT_SECRET
```

---

# 29. Environment Secret Risks

Remember environment variables can leak through:

```text
process inspection
crash dumps
debug tooling
child processes
```

For high-assurance environments use dedicated secret providers.

---

# 30. Secret Files

Mounted secret files can be appropriate in:

```text
containers
Kubernetes
systemd credentials
```

Read once, enforce permissions, avoid logging paths if sensitive.

---

# 31. OS Secure Stores

Client platforms:

```text
Android Keystore
iOS Keychain
Windows DPAPI/Credential Manager
macOS Keychain
Linux Secret Service
```

The platform integration exposes a secret-provider trait.

---

# 32. Server Secret Providers

Possible:

```text
AWS KMS/Secrets Manager
Azure Key Vault
GCP Secret Manager
HashiCorp Vault
Kubernetes Secrets
systemd credentials
```

Aequora core should not hard-code one provider.

---

# 33. Secret Provider Trait

Concept:

```rust
pub trait SecretProvider {
    async fn resolve(&self, key: &SecretKey)
        -> Result<SecretBytes, SecretError>;
}
```

---

# 34. Secret Lifetime

Keep plaintext secret lifetime as short as practical.

Do not copy secrets across many layers.

---

# 35. Secret Rotation

Services should support rotation for:

```text
DB credentials
API tokens
signing keys
TLS certs
```

where provider/system semantics allow.

---

# 36. Rotation Without Restart

Some secrets can reload dynamically.

Others may require:

```text
new connection pool
listener replacement
service restart
```

Explicitly classify each.

---

# 37. Configuration Mutability Classes

Every setting should be classified:

```text
StartupOnly
Reloadable
RestartRequired
ImmutableAfterInitialization
```

---

# 38. Example

```text
HTTP bind               RestartRequired
log level               Reloadable
DB schema                Not configuration
AuthorityId              Immutable
batch max                Reloadable
crypto algorithm policy  Controlled
```

---

# 39. Runtime Reload

Recommended:

```text
load candidate
↓
validate candidate
↓
compare change classes
↓
reject unsafe changes
↓
publish new generation
```

---

# 40. Config Generation

Track:

```text
ConfigGeneration
```

with each accepted runtime update.

Useful for:

```text
logs
incident bundles
diagnostics
```

---

# 41. Atomic Runtime Config Publication

Use an immutable snapshot:

```text
Arc<EffectiveRuntimeConfig>
```

and atomically replace the pointer.

Readers always see a coherent generation.

---

# 42. No Partially Applied Config

Do not mutate individual shared fields one-by-one.

A request should see:

```text
generation N
or
generation N+1
```

not a mixture.

---

# 43. Runtime Policy vs Correctness

Safe runtime settings include:

```text
batch size
timeouts
scheduler cadence
compression threshold
worker limits
```

Unsafe to expose as ordinary flags:

```text
disable auth
ignore cursor mismatch
skip ledger
disable fencing
accept invalid schema
```

---

# 44. Configuration Invariants Registry

Important configuration settings can carry metadata:

```text
name
type
default
mutability
sensitivity
environment applicability
validation rules
```

This can generate documentation.

---

# 45. Configuration Schema

Maintain a version:

```text
ConfigSchemaVersion
```

Separate from:

```text
ProtocolVersion
DomainSchemaVersion
LocalStoreFormatVersion
```

---

# 46. Config Migration

Old RON config can be:

```text
parsed
upgraded
warned
```

through explicit config-schema migration.

---

# 47. Unknown Fields

Production behavior should generally reject unknown correctness-sensitive fields.

This catches typos.

---

# 48. Deprecation

Deprecated config:

```text
works temporarily
emits warning
has replacement
```

then eventually becomes rejected according to release policy.

---

# 49. Feature Flags

Feature flags are for controlled rollout, not arbitrary semantic mutation.

Classes:

```text
UI feature
optional optimization
new read path
new adapter
new protocol capability
business feature
```

---

# 50. Feature Flag Safety Classes

Recommended:

```rust
pub enum FeatureSafety {
    PresentationOnly,
    Optimization,
    CompatibleBehavior,
    SemanticMigrationRequired,
    SecurityCritical,
}
```

---

# 51. Presentation Flags

Examples:

```text
show new dashboard
enable new conflict UI
```

Low risk.

---

# 52. Optimization Flags

Examples:

```text
enable zstd compression
enable Merkle cache
use new batching heuristic
```

Must preserve observable semantics.

---

# 53. Compatible Behavior Flags

Examples:

```text
new server implementation path
new index-assisted query
```

requires differential tests.

---

# 54. Semantic Changes

A flag must not silently change operation meaning.

Semantic changes require:

```text
new operation/profile/schema version
migration
capability negotiation
```

---

# 55. Security-Critical Flags

Security requirements cannot be casually disabled per tenant.

Examples:

```text
require MFA for admin
minimum TLS
required signature policy
```

These are security policy, not experimentation flags.

---

# 56. Compile-Time Features vs Runtime Flags

Cargo features select:

```text
what code exists
```

Runtime flags select:

```text
which compiled capability is active
```

Keep these distinct.

---

# 57. Cargo Feature Policy

Possible:

```text
postgres
stoolap
sqlite
axum
dioxus
mobile
desktop-agent
json
zstd
```

Avoid a massive combinatorial feature matrix.

---

# 58. Default Features

The core/foundation crates should have minimal defaults.

Facade/application crates may provide ergonomic bundles.

---

# 59. Adapter Selection

Applications may compile multiple adapters:

```text
SQLite
Stoolap
```

but a local store is opened with one explicit adapter.

---

# 60. Adapter Identity

Persist:

```text
AdapterId
AdapterVersion
```

with store metadata where needed.

Do not auto-open the same physical file using a different adapter.

---

# 61. PostgreSQL / Neon Configuration

Logical config:

```ron
authority: (
    adapter: Postgres,
    pool: (
        max_connections: 32,
        min_connections: 2,
    ),
    timeouts: (
        statement_ms: 5000,
        lock_ms: 1000,
    ),
)
```

Connection secrets are resolved separately.

---

# 62. Neon Profile

Neon-specific deployment tuning may extend PostgreSQL config:

```text
connection behavior
pooling
cold-start policy
branch/test environment
```

without changing authority semantics.

---

# 63. SQLite Configuration

Logical settings:

```ron
local_store: (
    adapter: Sqlite,
    durability: Critical,
    busy_timeout_ms: 5000,
)
```

Physical tuning must stay within certified safe ranges.

---

# 64. Stoolap Configuration

Same principle:

```ron
local_store: (
    adapter: Stoolap,
    durability: Critical,
)
```

Only expose capabilities verified by the current adapter.

---

# 65. Adapter-Specific Config Isolation

Core config should not accumulate every DB-specific knob.

Each adapter owns a typed adapter config.

---

# 66. Server HTTP Config

Example:

```ron
http: (
    bind: "0.0.0.0:8443",
    body_limit_bytes: 4194304,
    request_timeout_ms: 15000,
)
```

---

# 67. Trusted Proxy Configuration

Explicit list/range:

```text
trusted_proxies
```

Never:

```text
trust forwarded headers from everyone
```

---

# 68. TLS Configuration

Model:

```text
DisabledDevelopmentOnly
DirectTls
BehindTrustedProxy
```

Production validation rejects unsafe combinations.

---

# 69. Authentication Config

Example:

```text
OIDC issuer
audience
token lifetime policy
device registration policy
```

Secrets remain references.

---

# 70. Scheduler Config

Examples:

```text
foreground debounce
background cadence
batch max
backoff min/max
metered network policy
```

Scheduler policy remains bounded by Part 06/20 safety.

---

# 71. Admission Config

Examples:

```text
global requests
per-tenant requests
bootstrap concurrency
CPU work
DB work
```

Never allow zero/unbounded values accidentally.

---

# 72. Bounded Types

Instead of raw:

```text
usize
```

use validated:

```text
NonZeroUsize
BoundedCount<1, 4096>
```

or equivalent constructors.

---

# 73. Time Units

Never use naked integers for ambiguous durations.

Prefer:

```text
Duration
Seconds
Milliseconds
```

with explicit RON representation.

---

# 74. Byte Sizes

Use typed parser:

```text
ByteSize
```

so configs can express:

```text
4 MiB
512 MiB
```

if supported by chosen format/parser.

---

# 75. Retention Policy

Retention is a policy object, not arbitrary days integer.

Example:

```text
journal floor policy
operation ledger horizon
audit retention
snapshot retention
inactive-device window
```

Part 14 rules apply.

---

# 76. Governance Policy

Governance changes may require:

```text
authorization
audit
legal approval
```

rather than ordinary local file edits.

---

# 77. Authority Configuration

The following are highly protected:

```text
AuthorityId
AuthorityEpoch
writer role
promotion policy
```

Epoch must never be manually changed by editing RON.

---

# 78. Authority State vs Config

Keep separate:

```text
desired deployment config
```

and:

```text
durable authority state
```

Authority epoch/timeline live in authoritative metadata.

---

# 79. Device Identity

Likewise:

```text
DeviceId
LocalStoreId
StoreGeneration
```

are durable identity state, not ordinary config options.

---

# 80. Protocol Policy

Server config may set:

```text
minimum supported client
supported protocol range
deprecated operation policy
```

through typed compatibility policy.

---

# 81. Capability Negotiation

Runtime config can enable a compiled optional capability.

But client/server still negotiate it explicitly.

---

# 82. Flag Rollout Strategy

Recommended rollout stages:

```text
Off
Internal
Canary
Percentage
TenantAllowlist
On
```

---

# 83. Percentage Rollout

Assignment must be stable.

Use deterministic hashing of:

```text
tenant ID
feature ID
rollout generation
```

rather than random selection per request.

---

# 84. Do Not Use Percentage Flags for Critical Consistency

Never split one aggregate's semantics randomly between two incompatible implementations.

---

# 85. Tenant Allowlist

Useful for early testing:

```text
specific internal/test tenants
```

Ensure tenant isolation and audit.

---

# 86. Client Flags

Client-side flags may come from:

```text
compiled defaults
local config
server policy
```

Security-critical decisions still server-enforced.

---

# 87. Offline Client Flag Cache

If flags are cached for offline use:

```text
version
expiry
policy generation
```

should be persisted where needed.

---

# 88. Missing Flag State

Use safe default.

For features requiring server support:

```text
disabled
```

unless capability negotiation confirms compatibility.

---

# 89. Flag Evaluation API

Concept:

```rust
pub trait FeatureEvaluator {
    fn evaluate(
        &self,
        feature: FeatureId,
        context: FeatureContext,
    ) -> FeatureDecision;
}
```

---

# 90. Stable Feature IDs

Durable/public feature identifiers should be governed under Part 29 registry principles.

Do not rename/reuse IDs casually.

---

# 91. Runtime Policy Source

Potential sources:

```text
local RON
server control plane
environment
enterprise config service
```

Aequora should normalize them into one validated policy snapshot.

---

# 92. Centralized Dynamic Config

If later added:

```text
control plane
↓
signed/versioned policy
↓
server nodes
```

Nodes validate before activation.

---

# 93. No Mandatory External Config Service

Aequora should run perfectly with:

```text
local RON + secret provider
```

No Consul/etcd dependency is required for v1.

---

# 94. Offline Deployment

Air-gapped deployments should support:

```text
local config
local secret files/OS store
manual signed policy bundles
```

without cloud dependence.

---

# 95. Config Signature

High-assurance environments may sign configuration bundles.

Verify before activation.

---

# 96. Config Digest

Compute:

```text
ConfigDigest
```

over non-secret effective semantic configuration.

Useful for:

```text
diagnostics
fleet comparison
incident replay
```

---

# 97. Secret Exclusion From Digest

Never hash plaintext secrets into exposed diagnostics.

Digest references/config semantics, not sensitive values.

---

# 98. Startup Validation

At startup validate cross-field constraints.

Examples:

```text
TLS required in production
admin listener cannot be public without strong auth
batch limit <= protocol maximum
DB pool > required worker minimum
snapshot concurrency <= admission capacity
```

---

# 99. Cross-Layer Validation

Configuration validity sometimes depends on adapter capabilities.

Example:

```text
snapshot generation swap enabled
```

requires adapter support.

Fail startup rather than silently downgrade.

---

# 100. Warnings vs Errors

Use:

```text
Warning
```

for non-dangerous suboptimal settings.

Use:

```text
Error
```

for invariant/security incompatibility.

---

# 101. `aequora config check`

Should validate:

```text
syntax
schema
types
constraints
adapter capability compatibility
production safety
secret references
```

without starting the full service.

---

# 102. `aequora config explain`

Example:

```text
aequora config explain sync.batch.max_operations
```

shows:

```text
meaning
default
allowed range
mutability
source precedence
safety class
```

---

# 103. `aequora config diff`

Compare:

```text
current
candidate
```

and classify changes:

```text
Reloadable
RestartRequired
Forbidden
SecuritySensitive
```

---

# 104. Runtime Reload Trigger

Potential:

```text
SIGHUP
admin API
file watcher
```

File watcher should be optional; atomic reload still required.

---

# 105. File Watching

Avoid partially written config.

Recommended deployment pattern:

```text
write new file
fsync
atomic rename
```

then reload.

---

# 106. Invalid Reload

If candidate config fails:

```text
keep current generation
log typed error
surface diagnostics
```

Never partially activate it.

---

# 107. Feature Flag Persistence

Server flag state may live in:

```text
control-plane DB
versioned policy file
```

depending deployment.

Do not mix it into journal ordering.

---

# 108. Business Entitlements vs Feature Flags

Subscription/payment entitlement is not merely an engineering flag.

Separate:

```text
Entitlement
FeatureFlag
```

Entitlements are authoritative business state.

---

# 109. Example

```text
FeatureFlag:
new invoice screen

Entitlement:
advanced accounting module purchased
```

The client may use both to decide UI exposure, but server enforces entitlement.

---

# 110. Experiments

If experimentation is ever supported:

```text
ExperimentId
VariantId
AssignmentGeneration
```

Keep experiments away from financial/security semantics.

---

# 111. Observability

Every process should expose safe configuration metadata:

```text
ConfigGeneration
ConfigDigest
Environment
enabled capability IDs
feature-policy generation
```

---

# 112. Log Changes

On accepted reload:

```text
old generation
new generation
changed keys
change classes
actor/source
```

Do not log secret values.

---

# 113. Admin Audit

Control-plane runtime config changes are audited.

Local startup file changes are observable through generation/digest but may not have an authenticated actor unless deployment tooling supplies one.

---

# 114. Incident Bundles

Include:

```text
sanitized effective config
config generation
config digest
feature decisions relevant to incident
```

This improves reproducibility.

---

# 115. Deterministic Replay

Replay bundle should capture effective policy inputs that affected deterministic domain execution.

Do not assume current production config equals incident-time config.

---

# 116. Configuration Testing

Required tests:

```text
parse
validation
precedence
redaction
reload atomicity
cross-field constraints
profile safety
feature rollout determinism
old config migration
unknown fields
secret-provider failure
```

---

# 117. Property Tests

Property test:

```text
invalid bounds never produce ValidatedConfig
```

and:

```text
serialization roundtrip preserves semantic config
```

where applicable.

---

# 118. Secret Leak Tests

Inject known marker secrets.

Verify absence from:

```text
Debug
logs
CLI output
incident bundle
panic report
metrics
```

---

# 119. Reload Race Test

Under concurrent requests:

```text
reload N -> N+1
```

Every request must see one complete generation.

---

# 120. Feature Assignment Test

Same:

```text
FeatureId + TenantId + RolloutGeneration
```

must produce same percentage assignment across processes.

---

# 121. Production Safety Test

Attempt startup with:

```text
Production
+
development auth bypass
```

Expected:

```text
hard failure
```

---

# 122. Adapter Capability Test

Attempt config requiring unsupported adapter capability.

Expected:

```text
validation failure
```

not silent behavior change.

---

# 123. Config Migration Test

All supported old config versions should upgrade to the current canonical model or produce precise incompatibility.

---

# 124. Configuration Invariants

## AEQ-INV-CONFIG001

```text
No configuration value can disable Aequora's correctness, authority, tenant-isolation, or idempotency invariants.
```

## AEQ-INV-CONFIG002

```text
Secrets are resolved through secret-aware abstractions and are redacted from ordinary Debug, logs, diagnostics, and CLI output.
```

## AEQ-INV-CONFIG003

```text
Only fully parsed and validated configuration snapshots may become effective runtime configuration.
```

## AEQ-INV-CONFIG004

```text
Runtime configuration reload is atomic: a consumer observes one complete ConfigGeneration.
```

## AEQ-INV-CONFIG005

```text
AuthorityEpoch, DeviceId, LocalStoreId, StoreGeneration, and similar durable identities are state, not editable deployment configuration.
```

## AEQ-INV-CONFIG006

```text
Feature flags cannot silently redefine durable operation semantics; semantic changes require versioned protocol/domain mechanisms.
```

## AEQ-INV-CONFIG007

```text
Production profiles reject development-only authentication bypasses, destructive reset paths, and fault-injection facilities.
```

## AEQ-INV-CONFIG008

```text
Adapter-specific configuration is validated against the adapter's declared and certified capability manifest.
```

## AEQ-INV-CONFIG009

```text
A failed runtime reload leaves the previous valid configuration generation fully active.
```

## AEQ-INV-CONFIG010

```text
Business entitlements and security policy remain authoritative server-side and cannot be granted merely by client feature flags.
```

---

# 125. Suggested Workspace Interfaces

```rust
pub struct EffectiveConfig {
    pub generation: ConfigGeneration,
    pub environment: Environment,
    pub runtime: RuntimePolicy,
    pub adapters: AdapterConfigSet,
}
```

Secret values should be resolved through dedicated handles rather than embedded everywhere.

---

# 126. Config Loader

Concept:

```rust
let raw = ConfigLoader::new()
    .defaults(defaults)
    .ron_file(base)
    .ron_file(environment_file)
    .environment("AEQUORA_")
    .cli(overrides)
    .load()?;

let validated = raw.validate(capabilities)?;
```

---

# 127. Secret Resolution

Composition root:

```rust
let secrets = SecretResolver::builder()
    .environment()
    .os_store(...)
    .provider(...)
    .build();
```

Services request typed secret references.

---

# 128. Runtime Config Store

Concept:

```rust
pub trait RuntimeConfigStore {
    fn current(&self) -> Arc<EffectiveRuntimeConfig>;
}
```

Update path validates before publication.

---

# 129. Product Configuration

Application config should wrap Aequora config.

Example:

```rust
struct SchoolServerConfig {
    aequora: AequoraServerConfig,
    school_product: SchoolProductConfig,
}
```

Do not add school-specific fields to generic Aequora crates.

---

# 130. Mobile Configuration

Mobile clients should compile safe defaults and receive bounded server policy.

Do not ship database credentials or privileged server secrets inside the app.

---

# 131. Desktop Configuration

Desktop may use a local RON file for:

```text
agent socket
local store path
diagnostics
sync preferences
```

Device credentials still live in secure storage.

---

# 132. User Preferences vs System Policy

Separate:

```text
UserPreference
```

from:

```text
RuntimePolicy
```

Example:

```text
user preference: sync attachments on metered network = false
server policy: attachment size maximum = 100 MiB
```

Effective behavior is the safe intersection.

---

# 133. Policy Precedence

Concept:

```text
hard correctness/security policy
↓
server operational policy
↓
tenant/application policy
↓
device/user preference
```

Lower levels cannot weaken higher-level requirements.

---

# 134. Policy Merge

Do not use generic last-write-wins for policy.

Each field defines its merge rule:

```text
minimum
maximum
intersection
override
deny-wins
allowlist intersection
```

---

# 135. Example

Client says:

```text
max batch = 1000
```

Server permits:

```text
max batch = 256
```

Effective:

```text
256
```

---

# 136. Security Policy Merge

For requirements:

```text
client wants optional MFA
server requires MFA
```

Effective:

```text
required
```

---

# 137. Configuration Documentation Generation

Generate from typed metadata:

```text
configuration reference
default values
ranges
environment variables
mutability
security notes
```

to avoid documentation drift.

---

# 138. Example Configuration

```ron
(
    schema_version: 1,

    environment: Production,

    http: (
        bind: "0.0.0.0:8443",
        trusted_proxies: [],
    ),

    authority: (
        adapter: Postgres,
        pool: (
            max_connections: 32,
        ),
    ),

    sync: (
        max_operations_per_exchange: 256,
        max_response_events: 1024,
    ),

    admission: (
        max_inflight_requests: 1024,
        per_tenant_inflight: 64,
    ),

    observability: (
        log_level: "info",
    ),
)
```

Secrets are intentionally absent.

---

# 139. Example Local Client Config

```ron
(
    schema_version: 1,

    local_store: (
        adapter: SQLite,
        path: Auto,
    ),

    sync: (
        foreground: Automatic,
        metered_network: MetadataOnly,
    ),

    diagnostics: (
        ring_buffer_entries: 2048,
    ),
)
```

---

# 140. SQLite vs Stoolap Selection

Aequora should support:

```text
SQLite
Stoolap
```

through the same local adapter traits.

Application code above adapter composition should not change.

---

# 141. No Mandatory Additional Database Part

For v1, the database foundation is sufficiently covered by:

```text
PostgreSQL / Neon authoritative adapter
SQLite embedded local adapter
Stoolap embedded local adapter
```

Future adapters such as:

```text
Redb
Fjall
other embedded engines
```

should be added only when an actual workload justifies their additional implementation and certification cost.

---

# 142. Completion Criteria

```text
[ ] configuration classes separated
[ ] RON configuration model defined
[ ] typed validation defined
[ ] source precedence defined
[ ] environment profiles defined
[ ] secrets architecture defined
[ ] secret rotation/lifetime defined
[ ] mutability classes defined
[ ] atomic reload defined
[ ] ConfigGeneration/Digest defined
[ ] feature flag safety model defined
[ ] Cargo features separated from runtime flags
[ ] adapter configuration boundaries defined
[ ] policy merge rules defined
[ ] entitlement distinction defined
[ ] observability/incident integration defined
[ ] CLI workflows defined
[ ] tests defined
[ ] CONFIG invariants defined
```

---

# 143. Final Architecture

```text
                        Sources
          +---------------+----------------+
          |               |                |
          v               v                v
      RON Files      Environment        CLI
          |               |                |
          +---------------+----------------+
                          |
                          v
                     RawConfig
                          |
                          v
                       Validate
                          |
             +------------+-------------+
             |                          |
             v                          v
      Adapter Capabilities        Safety Policy
             |                          |
             +------------+-------------+
                          |
                          v
                  EffectiveConfig
                 Generation + Digest
                          |
        +-----------------+------------------+
        |                                    |
        v                                    v
  Startup Services                    Runtime Config Store
                                             |
                                             v
                                     Atomic Reload
```

Secrets follow a separate path:

```text
SecretRef
   |
   v
SecretResolver
   |
   +--> Env
   +--> OS Secure Store
   +--> File/Credential
   +--> External Provider
   |
   v
Short-lived Secret Value
```

---

# 144. Final Recommendation

Aequora should treat configuration as typed, versioned, validated policy—not as arbitrary text that services read whenever they want.

The correct flow is:

```text
load
↓
validate
↓
resolve capabilities
↓
apply safety policy
↓
publish one immutable generation
↓
observe and audit changes
```

And secrets should travel through an entirely separate path.

> **Configuration may tune Aequora, but it must never be able to configure correctness away.**

For the v1 database ecosystem, PostgreSQL/Neon + SQLite + Stoolap are enough. Additional embedded databases should become new adapter projects only when their concrete workload advantages justify their maintenance and conformance burden.

---

## Next

**Part 44 — Packaging, Distribution, Release Engineering, Artifact Signing, Update Channels, and Cross-Platform Delivery Architecture**
