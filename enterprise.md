# Aequora Sync — Enterprise Production Architecture

## Enterprise-Grade Deployment, Operations, Reliability, Security, Upgradeability, and Hassle-Free Usage

> This document continues the existing Aequora Sync architecture and `ACID.md`.
>
> The earlier documents define database-independent synchronization, local-first behavior, Axum validation/execution, Postcard/RON protocol usage, transactional outbox and journal patterns, ACID boundaries, idempotency, conflicts, snapshots, cursors, Stoolap client adapters, and PostgreSQL/Neon server adapters.
>
> This document defines how that system becomes **enterprise-grade, production-ready, operationally safe, easy to integrate, easy to deploy, easy to upgrade, observable, recoverable, scalable, and difficult to misconfigure.**

---

# 1. Primary Objective

Aequora must not merely be technically correct.

It must also be operationally boring.

The ideal production experience is:

```text
configure
↓
start
↓
migrations verify automatically
↓
health checks turn green
↓
clients connect
↓
sync operates
↓
metrics/traces/logs explain problems
↓
upgrades happen without data loss
```

The operator should not need deep knowledge of distributed synchronization internals to run it safely.

---

# 2. Enterprise Design Principles

## 2.1 Safe by Default

Production defaults must favor:

```text
durability
bounded resources
TLS
strict validation
idempotency
migration checks
backups
observability
```

rather than maximum benchmark throughput.

## 2.2 Explicit Unsafe Modes

Any weaker mode must be clearly named.

Example:

```text
durability: Strict
```

If unsafe tuning exists:

```text
durability: UnsafePerformance
```

should be impossible to mistake for a normal production setting.

## 2.3 Stateless Application Nodes

Correctness must survive restart/loss of any Axum process.

Durable truth belongs in:

```text
PostgreSQL
journal
operation ledger
configuration source
secrets store
```

not process memory.

## 2.4 Automate Operational Invariants

The system should automatically verify:

```text
required DB migrations
protocol compatibility
schema compatibility
storage adapter capabilities
TLS configuration
journal health
operation ledger health
```

before reporting readiness.

## 2.5 Separate Control Plane and Data Plane

Data plane:

```text
sync requests
changes
snapshots
blob references
```

Control plane:

```text
configuration
health
metrics
administration
migrations
device revocation
tenant policies
diagnostics
```

---

# 3. Deployment Profiles

Aequora should support multiple deployment profiles without changing core code.

## 3.1 Development

```text
Dioxus client
    ↓
Stoolap

Axum server
    ↓
local PostgreSQL
```

For development, CI, demos, and integration tests.

## 3.2 Small Production

```text
Internet
   ↓
TLS reverse proxy / managed ingress
   ↓
1–2 Axum application nodes
   ↓
managed PostgreSQL / Neon
```

This should be the default first production architecture.

## 3.3 Highly Available Enterprise

```text
                    Clients
                       │
                       ▼
               Global DNS / Edge
                       │
                       ▼
              L7 Load Balancer
                       │
              ┌────────┼────────┐
              ▼        ▼        ▼
          Axum #1   Axum #2   Axum #3
              │        │        │
              └────────┼────────┘
                       ▼
               PostgreSQL Pool
                       │
                       ▼
          Authoritative PostgreSQL
                       │
             ┌─────────┴─────────┐
             ▼                   ▼
          backups            read replicas
```

No sticky session should be required for normal synchronization.

## 3.4 Enterprise Private Network

The same server can run entirely inside:

```text
LAN
VPN
private cloud
on-premises data center
```

## 3.5 Hybrid Offline-First Enterprise

```text
branch clients
      ↓
local-first storage
      ↓
intermittent network
      ↓
central Aequora server
```

Local work does not depend on continuous WAN availability.

---

# 4. Recommended Production Topology

For the current School ERP:

```text
Dioxus Desktop / Android
        │
        │ HTTPS + Postcard
        ▼
Cloudflare / reverse proxy / LB
        │
        ▼
Axum API + Aequora Sync Service
        │
        ▼
SQLx connection pool
        │
        ▼
Neon PostgreSQL
```

Optional:

```text
object storage
    large documents/blobs

email provider
    notifications

telemetry backend
    metrics/traces/logs
```

---

# 5. Server Runtime Structure

Recommended server binary:

```text
apps/
└── sync-server/
    └── src/
        ├── main.rs
        ├── bootstrap.rs
        ├── config.rs
        ├── telemetry.rs
        ├── health.rs
        ├── shutdown.rs
        ├── routes.rs
        ├── auth.rs
        ├── admin.rs
        └── dependencies.rs
```

`main.rs` should only coordinate startup and shutdown.

---

# 6. Deterministic Bootstrap Sequence

```text
process starts
↓
load configuration
↓
validate configuration
↓
load secrets
↓
initialize tracing/logging
↓
initialize DB pool
↓
verify DB connectivity
↓
verify migration state
↓
verify Aequora metadata
↓
initialize operation registry
↓
initialize auth
↓
initialize bounded Rayon pool
↓
build Axum router
↓
bind listener
↓
liveness=true
↓
readiness=true
```

Any critical failure should fail startup or readiness loudly.

---

# 7. Layered Configuration

Priority:

```text
compiled safe defaults
↓
RON configuration
↓
environment variables
↓
secret references
↓
explicit CLI overrides
```

Configuration categories:

```text
server
database
sync
protocol
security
auth
limits
timeouts
retry
compression
compute
observability
health
shutdown
maintenance
```

Example:

```ron
ProductionConfig(
    server: (
        bind: "0.0.0.0:8080",
        graceful_shutdown_seconds: 30,
    ),

    sync: (
        max_push_operations: 256,
        max_pull_events: 1000,
        max_request_bytes: 4194304,
        max_response_bytes: 8388608,
    ),

    database: (
        max_connections: 32,
        min_connections: 4,
        acquire_timeout_ms: 3000,
    ),

    compute: (
        rayon_threads: 4,
        parallel_threshold: 128,
    ),

    security: (
        require_tls_forwarding: true,
        reject_unknown_protocols: true,
    ),

    observability: (
        structured_logs: true,
        metrics: true,
        tracing: true,
    ),
)
```

Configuration must be semantically validated, not only deserialized.

---

# 8. Secret Management

Secrets include:

```text
database credentials
JWT signing material
OAuth/OIDC secrets
SMTP keys
external API credentials
```

Load from:

```text
environment
container secret mount
cloud secret manager
Vault-compatible provider
```

Never store them in RON checked into source control.

Sensitive wrapper types should redact `Debug` output.

---

# 9. Deployment Artifact Strategy

Prefer one statically/simple-linked production Rust server binary where practical.

Benefits:

```text
few moving parts
no language runtime deployment
easy containerization
easy systemd deployment
simple rollback
small operational surface
```

The internal codebase can remain highly modular.

---

# 10. Container Architecture

Recommended image:

```text
multi-stage Rust build
↓
minimal runtime image
↓
single binary
↓
CA certificates
↓
non-root runtime user
```

Production settings:

```text
non-root
read-only root FS where feasible
drop capabilities
no privileged mode
CPU/memory limits
minimal writable mounts
```

---

# 11. Native systemd Deployment

Aequora should work without containers.

Example:

```text
/usr/local/bin/aequora-server
/etc/aequora/config.ron
/etc/aequora/secrets/
```

This supports on-premise schools and private infrastructure.

---

# 12. Kubernetes Deployment

Only when operational scale justifies it.

```text
Deployment: aequora-server
Service: ClusterIP
Ingress/Gateway: HTTPS
Secret: credentials
ConfigMap: non-secret config
PodDisruptionBudget
HorizontalPodAutoscaler
```

Correctness must not depend on Kubernetes.

---

# 13. Liveness and Readiness

Liveness:

```text
process and runtime are alive
```

Readiness:

```text
DB reachable
required migrations applied
operation registry valid
critical auth initialized
server able to process sync
```

Do not restart a healthy process merely because a temporary external dependency is down.

---

# 14. Graceful Shutdown

On SIGTERM:

```text
mark readiness=false
↓
stop accepting new work
↓
finish in-flight requests
↓
stop workers
↓
flush telemetry
↓
close DB pool
↓
exit
```

Transactions that cannot complete before deadline must roll back naturally.

---

# 15. Zero-Downtime Rollouts

Stateless Axum nodes make rolling updates possible.

```text
old nodes
+
new nodes
```

may coexist only if:

```text
wire protocol compatible
database schema compatible
operation semantics compatible
```

---

# 16. Expand-Contract Migrations

Never perform destructive DB changes first.

Use:

```text
EXPAND
↓
deploy compatible server
↓
backfill/migrate
↓
verify
↓
CONTRACT in later release
```

This preserves rollback and mixed-version operation.

---

# 17. Migration Modes

Support:

```text
VerifyOnly
MigrateOnStart
ExternalMigration
```

Recommended:

```text
small production:
    MigrateOnStart for short safe migrations

large enterprise:
    ExternalMigration
```

Large backfills and index creation belong in explicit migration jobs.

---

# 18. Migration Locking

When automatic migration is enabled across multiple server replicas:

```text
only one replica may execute migrations
```

Use PostgreSQL advisory locking or equivalent.

Other nodes wait or remain unready.

---

# 19. Protocol Compatibility

Server should advertise:

```text
min protocol
max protocol
capabilities
minimum client version where policy requires
```

Client/server negotiate compatible capabilities.

Do not infer all behavior solely from application version.

---

# 20. Rolling Protocol Upgrades

Use a compatibility window.

Example:

```text
server supports protocol 3..5
```

Release sequence:

```text
server learns old + new
↓
new server deploys
↓
new clients begin emitting new format
↓
old clients continue working
↓
old support removed much later
```

---

# 21. Client Upgrade Resilience

Offline clients may return weeks later.

Server must return typed outcomes:

```text
Supported
UpgradeRecommended
UpgradeRequired
ResyncRequired
```

`UpgradeRequired` must not destroy pending local work.

---

# 22. High Availability

Normal sync must work through any app node.

```text
LB
├── Axum A
├── Axum B
└── Axum C
     ↓
PostgreSQL
```

No sticky session.

No correctness state stored only in RAM.

---

# 23. Database Connection Planning

Total possible connections:

```text
replicas × max_pool_connections
```

must fit PostgreSQL capacity.

If the pool is exhausted:

```text
bounded wait
↓
retryable overload
```

Never queue unbounded work.

---

# 24. Read Replicas

Use replicas for:

```text
analytics
heavy reports
admin read-only queries
```

Do not send correctness-sensitive sync reads to lagging replicas unless replica-lag semantics are explicitly handled.

---

# 25. Redis and Other Infrastructure

Redis is not required for core Aequora correctness.

Kafka is not required.

A service mesh is not required.

Kubernetes is not required.

Add infrastructure only when measured requirements justify it.

---

# 26. Dependency Criticality

Critical:

```text
authoritative PostgreSQL
auth verifier when required
```

Noncritical:

```text
email
webhooks
analytics
external telemetry exporter
```

Failure of a noncritical service should not break sync.

---

# 27. Transactional Outbox for External Services

For:

```text
email
webhooks
PSP follow-up
notifications
```

commit intent in the authoritative transaction.

```text
domain mutation
+
sync journal
+
external-side-effect outbox
```

A worker executes later with idempotent retries.

---

# 28. SLO Architecture

Define measurable targets.

Examples:

```text
service availability >= 99.9%
ordinary sync p95 < target based on measured workload
ACKed operation loss = 0
duplicate logical execution for same OperationId = 0
```

Correctness SLOs outrank latency SLOs.

---

# 29. Client Health Indicators

Track:

```text
last successful sync
outbox depth
oldest pending age
conflict count
current cursor
sync status
```

---

# 30. Server Health Indicators

Track:

```text
request success ratio
p50/p95/p99 latency
DB transaction retry rate
journal query latency
dedup hit rate
snapshot success
current protocol versions
```

---

# 31. Observability Stack

Core:

```text
tracing
structured logs
metrics
```

Optional export:

```text
OpenTelemetry
OTLP
Prometheus
```

Trace structure:

```text
HTTP request
  └── sync exchange
      ├── decode
      ├── dedup
      ├── authz
      ├── validate
      ├── plan
      ├── transaction
      ├── journal pull
      └── encode
```

---

# 32. Correlation Model

Every operation should be traceable through:

```text
request_id
sync_session_id
OperationId
tenant_id
device_id
cursor
```

`OperationId` becomes the primary distributed debugging key.

---

# 33. Privacy-Safe Telemetry

Never log by default:

```text
student names
addresses
payment details
document contents
tokens
raw payloads
```

Metrics must avoid high-cardinality IDs.

IDs belong in logs/traces, not metric labels.

---

# 34. Alerting

Alert on:

```text
DB unavailable
journal append failure
transaction retry spike
server error spike
p99 latency spike
snapshot failures
migration mismatch
outbox oldest-age threshold
```

---

# 35. Security Layers

```text
TLS
authentication
authorization
tenant isolation
device identity
request bounds
rate limiting
protocol validation
database constraints
audit logging
secret management
supply-chain controls
```

---

# 36. Authentication Architecture

Aequora consumes:

```rust
AuthContext
```

Application may derive it from:

```text
OIDC
JWT
session tokens
mTLS
enterprise SSO
```

Aequora core should not become an identity provider.

---

# 37. Device Lifecycle

Each installation receives stable:

```text
DeviceId
```

Server records:

```text
tenant
actor
last seen
client version
protocol version
cursor watermark
revocation state
```

---

# 38. Device Revocation

Admin may revoke a device.

Next sync:

```text
authorization rejected
```

Pending operations remain local unless application policy explicitly discards them.

---

# 39. Tenant Isolation

Effective tenant comes from trusted auth context.

Every authoritative query is tenant-scoped.

Client payload cannot grant itself cross-tenant access.

---

# 40. Noisy Neighbor Protection

Use:

```text
per-tenant concurrency bounds
global concurrency bounds
request limits
bounded DB pool
bounded compute pool
fairness policies
```

One large tenant should not starve others.

---

# 41. Rate Limiting

Layers:

```text
edge/IP
authenticated actor/device
tenant
expensive endpoint
```

Overload should yield:

```text
429/503
+
Retry-After/server hint
```

Client keeps outbox entries pending.

---

# 42. Resource Limits

Strictly bound:

```text
compressed request bytes
decompressed request bytes
operation count
operation payload
dependency count
response bytes
snapshot chunk bytes
in-flight requests
Rayon queue
```

This protects availability and security.

---

# 43. Decompression Bomb Protection

Enforce a maximum decompressed size independently from compressed request size.

Never allocate based only on untrusted encoded length.

---

# 44. Operation Descriptor

Each registered operation should declare:

```text
kind
supported schema range
maximum payload
transaction policy
conflict policy
permission class
```

This allows fast early validation.

---

# 45. Bounded Compute

Tokio handles:

```text
network I/O
DB I/O
timers
coordination
```

Dedicated Rayon pool handles only sufficiently large CPU work:

```text
hashing
snapshot transforms
large pure validation
compression preparation
dependency graph calculations
```

Do not let Rayon consume all host CPUs accidentally.

---

# 46. Client SDK Ergonomics

Ideal setup:

```rust
let sync = AequoraClient::builder()
    .store(stoolap_store)
    .transport(http_transport)
    .identity(device_identity)
    .config(config)
    .build()?;

sync.spawn();
```

Correct usage should be simpler than unsafe usage.

---

# 47. Mutation Ergonomics

Application:

```rust
sync_tx
    .run(|tx| async move {
        student_repo.update(tx, student).await?;
        tx.enqueue(operation).await?;
        Ok(())
    })
    .await?;
```

The library should structurally encourage atomic domain+outbox behavior.

---

# 48. Server Ergonomics

```rust
let service = AequoraServer::builder()
    .store(postgres_store)
    .registry(operation_registry)
    .authorizer(authorizer)
    .config(config)
    .build()?;

let app = Router::new()
    .merge(aequora_axum::routes(service));
```

---

# 49. Production Profiles

Provide safe profiles:

```text
Development
SmallProduction
Enterprise
HighLatencyNetwork
```

Profiles select conservative defaults and can be overridden.

---

# 50. Startup Self-Diagnostics

Example:

```text
Aequora server vX.Y.Z
Protocol: 3..5
DB migration: 12 OK
Store adapter: FullAuthoritative
TLS forwarding: required
Telemetry: enabled
Status: Ready
```

Never print secrets.

---

# 51. Hassle-Free Client Bootstrap

```text
authenticate
↓
create/recover DeviceId
↓
negotiate protocol
↓
request scope bootstrap
↓
download chunks
↓
verify hashes
↓
install atomically
↓
set cursor
↓
incremental sync
```

No manual DB copy/import.

---

# 52. Automatic Resynchronization

When:

```text
cursor expired
timeline changed
scope changed
journal compacted
```

server returns:

```text
ResyncRequired
```

Client:

```text
preserve pending local operations
↓
bootstrap
↓
reconcile/rebase pending intent
↓
resume
```

---

# 53. Never Lose Pending Work

Do not delete pending operations during:

```text
bootstrap
migration
cache clear
upgrade
repair
```

without an explicit user/admin decision.

---

# 54. Quarantine

If local corruption or incompatible pending payloads are detected:

```text
quarantine
```

instead of silently dropping or blindly submitting them.

---

# 55. Conflict UX Contract

Aequora exposes:

```text
conflict ID
entity
operation
server version
reason code
possible resolution classes
```

The application decides presentation/localization.

---

# 56. Offline User States

Expose:

```text
Offline
Pending
Syncing
Synced
Conflict
ActionRequired
Maintenance
UpgradeRequired
```

Avoid a single ambiguous "Sync failed".

---

# 57. Mobile Efficiency

Mobile client should:

```text
batch writes
avoid busy polling
use jittered retries
compress sufficiently large payloads
stop active work when network absent
integrate with platform background scheduling
```

Core Aequora remains platform-neutral.

---

# 58. Server Push Hints

Optional:

```text
WebSocket
SSE
platform push notification
```

may signal that new data exists.

Correctness remains:

```text
cursor-based journal pull
```

Push is only a wake-up hint.

---

# 59. Snapshot Service

For large tenants:

```text
request snapshot
↓
reuse valid cached snapshot
or
build asynchronous snapshot
↓
store manifest/chunks
↓
client downloads
```

Object storage can hold large immutable snapshot files.

---

# 60. Snapshot Security

Snapshot authorization is tenant/scope-based.

Manifest includes:

```text
scope
boundary cursor
chunk hashes
schema
expiration
```

---

# 61. Blob Architecture

Large documents/media:

```text
object storage
```

Normal sync carries:

```text
BlobRef
```

not the entire file.

Blob flow:

```text
stage upload
↓
verify hash
↓
commit domain BlobRef
↓
mark referenced
```

Garbage collect orphaned blobs after retention.

---

# 62. Backup Architecture

Back up authoritative data as one logical timeline:

```text
domain tables
Aequora journal
operation ledger
scope generation
snapshot metadata
```

Restoring domain tables without sync metadata is unsafe.

---

# 63. PITR

Use PostgreSQL/provider capabilities for:

```text
point-in-time recovery
scheduled snapshots
offsite redundancy
```

Define application RPO/RTO explicitly.

---

# 64. Disaster Recovery

Example policy:

```text
RPO <= chosen business tolerance
RTO <= chosen business tolerance
```

Restore runbook:

```text
freeze writes
↓
restore authoritative DB
↓
verify migrations
↓
verify journal/ledger consistency
↓
increment cursor generation if timeline changed
↓
start restricted
↓
run integrity verification
↓
enable service
```

---

# 65. Cursor Generation After Restore

After rollback/PITR, old clients may hold cursors from a timeline that no longer exists.

Increment:

```text
CursorGeneration
```

Then old clients receive:

```text
ResyncRequired
```

Never silently reuse an invalid timeline.

---

# 66. Backup Restore Testing

A backup is not proven until restored.

Perform recurring restore drills in staging or isolated infrastructure.

Verify:

```text
domain data
journal
operation ledger
migrations
bootstrap
new client sync
```

---

# 67. Client Recovery

If local DB is corrupted:

```text
preserve recoverable pending outbox
↓
recreate local store
↓
bootstrap
↓
reconcile/replay pending operations
```

---

# 68. Admin Plane

Restricted administration capabilities:

```text
tenant sync health
device list
device revoke
operation lookup
cursor diagnostics
journal lag
snapshot jobs
migration status
maintenance mode
protocol compatibility
```

Prefer private/admin authentication boundary.

---

# 69. Admin CLI

Suggested commands:

```text
aequora-admin status
aequora-admin migrations
aequora-admin devices
aequora-admin operation <id>
aequora-admin journal
aequora-admin snapshot
aequora-admin verify
```

Read-only diagnostics should be the default.

---

# 70. Maintenance Mode

Support:

```text
Normal
ReadOnly
SyncPaused
```

Client receives typed response and keeps changes safely pending.

---

# 71. Stable Operational Error Codes

Examples:

```text
AEQ-OVERLOAD-001
AEQ-MAINT-001
AEQ-UPGRADE-001
AEQ-AUTH-001
AEQ-PROTO-001
AEQ-STORAGE-001
AEQ-CONFLICT-001
```

Human messages can change; machine codes remain stable.

---

# 72. Scalability Model

Scale by bottleneck:

```text
more HTTP concurrency
    -> Axum replicas

DB latency
    -> query/index/pool/DB scaling

large journal
    -> partitioning/retention/snapshots

large snapshots
    -> chunking/object storage

CPU-heavy preparation
    -> bounded Rayon pool
```

---

# 73. Database Optimization Before Distributed Complexity

Before adding Kafka/Redis/microservices:

```text
measure queries
add correct indexes
bound page sizes
profile transactions
tune connection pool
partition journal where justified
```

---

# 74. Journal Partitioning

At scale, adapter may partition journal by:

```text
tenant
scope
time
```

without changing the protocol.

---

# 75. Journal Retention

Define separate policies for:

```text
incremental sync journal
business audit
tombstones
snapshots
logs
```

They serve different purposes.

---

# 76. Device Watermarks

Track active device cursor watermarks where necessary for:

```text
safe journal compaction
safe tombstone GC
inactive device bootstrap decisions
```

---

# 77. Inactive Device Policy

Example:

```text
inactive > 90 days
↓
incremental history may expire
↓
bootstrap required when device returns
```

This bounds retained journal history.

---

# 78. Modular Monolith First

The server should initially remain:

```text
one Axum application
many internal Rust crates
one authoritative PostgreSQL
```

This preserves:

```text
simple deployment
simple ACID transactions
easy tracing
low latency
few failure boundaries
```

---

# 79. Avoid Premature Microservices

Do not split core sync into services merely for architecture fashion.

Possible later extractions:

```text
blob service
snapshot worker
analytics
notification worker
```

Only extract when independent scaling or operational ownership clearly requires it.

---

# 80. Background Workers

Durable workers may process:

```text
notification outbox
webhooks
snapshot generation
blob cleanup
journal compaction
integrity verification
```

Initially these can run in the same binary.

Later they can become separate worker processes without changing business semantics.

---

# 81. Job Idempotency

Every durable background job needs:

```text
JobId
state
attempt count
retry policy
final outcome
```

Retries must be safe.

---

# 82. Supply-Chain Security

CI should include:

```text
cargo fmt
clippy
cargo test
cargo audit
cargo deny
license policy
dependency lock
SBOM generation
artifact checksums
release signatures/provenance
container scan
```

---

# 83. Release Artifacts

Publish:

```text
server binary
container image
checksums
signature/provenance
SBOM
migration bundle
release notes
protocol compatibility matrix
```

---

# 84. Staging

Staging must use:

```text
real PostgreSQL
real migrations
TLS
realistic auth
multiple client versions
snapshot/bootstrap
restore drills
rolling upgrades
```

Mocks are not enough for release validation.

---

# 85. Canary Deployment

Roll high-risk changes gradually:

```text
internal tenant
↓
pilot tenant
↓
small cohort
↓
general rollout
```

Watch:

```text
errors
latency
transaction retries
conflict rates
journal health
```

---

# 86. Blue/Green Deployment

Use when mixed old/new server versions are unsafe.

```text
blue active
green deployed
green verified
traffic switched
blue retained for rollback window
```

---

# 87. Rollback Safety

An old binary may not understand a destructively migrated DB.

That is why expand-contract migration is a release requirement.

---

# 88. CI/CD Release Gates

A release must pass:

```text
format/lint
unit tests
property tests
integration tests
adapter compliance tests
migration compatibility
fuzz smoke
security audit
license check
benchmark regression threshold
backup/restore verification
rolling-upgrade test
```

---

# 89. Production Runbooks

Maintain runbooks for:

```text
DB outage
migration failure
high conflict rate
journal growth
cursor invalidation
snapshot failure
auth outage
overload
deployment rollback
PITR restore
high latency
```

---

# 90. DB Outage Runbook

```text
DB unavailable
↓
server readiness=false
↓
clients retain outboxes
↓
DB restored
↓
server ready
↓
clients retry
↓
OperationId dedup resolves ambiguous prior requests
```

---

# 91. High Conflict Runbook

Investigate:

```text
poor aggregate boundaries
long offline intervals
stale base version handling
UI generating unnecessary writes
wrong merge strategy
```

Do not "solve" by globally enabling last-write-wins.

---

# 92. Operation Investigation

Given `OperationId`:

```text
client outbox
↓
HTTP trace
↓
server transaction
↓
operation ledger
↓
journal sequence
↓
client reconciliation
```

Operators should be able to reconstruct the lifecycle.

---

# 93. Chaos Testing

Inject:

```text
Axum kill
DB restart
network drop
response loss after commit
duplicate request
slow DB
client crash
snapshot interruption
mixed server versions
```

Correctness invariants must still hold.

---

# 94. Soak Testing

Run realistic workloads over long periods and inspect:

```text
memory growth
connection leaks
retry storms
outbox accumulation
journal growth
latency drift
```

---

# 95. Load Workloads

Test:

```text
many small writes
large batches
offline reconnection storms
new-device bootstrap storms
same-entity contention
large tenant catch-up
```

---

# 96. Thundering Herd Protection

After an outage:

```text
client exponential backoff + jitter
server Retry-After
global concurrency limits
tenant fairness
bounded DB pool
```

---

# 97. Multi-Environment Promotion

Build once:

```text
CI artifact
↓
staging
↓
production
```

Only configuration/secrets differ.

Never rebuild production manually from a different source tree.

---

# 98. Reproducible Releases

Pin:

```text
Cargo.lock
Rust toolchain
container base digest where practical
migration version
```

Generate checksums and provenance.

---

# 99. Licensing Hygiene

Automate license policy.

Block dependencies whose licenses conflict with the intended commercial/distribution model.

Keep integration-heavy dependencies isolated in adapter crates.

---

# 100. Data Retention

Tenant/application policy should separately define:

```text
domain records
sync journal
audit logs
snapshots
tombstones
blobs
telemetry logs
```

---

# 101. Tenant Offboarding

Workflow:

```text
disable new writes
↓
final sync/export
↓
retention window
↓
revoke devices/tokens
↓
delete according to policy
↓
preserve required audit evidence
```

---

# 102. Capacity Planning

Measure:

```text
active devices
operations/device/day
average operation size
events/day
snapshot size
peak concurrent syncs
journal retention window
```

Approximate journal bytes/day:

```text
events/day × average encoded event bytes + indexes/overhead
```

---

# 103. Scaling Triggers

Scale Axum when:

```text
CPU
concurrency
request latency
```

are limiting.

Scale/tune DB when:

```text
transaction latency
I/O
connection contention
journal reads
lock contention
```

are limiting.

---

# 104. Database Security

Prefer separate roles:

```text
migration role:
    DDL

runtime role:
    required DML only
```

Use TLS to PostgreSQL where required and rotate credentials.

---

# 105. Timeouts

Bound:

```text
HTTP request
DB acquisition
DB statement
transaction
snapshot generation
external API calls
```

No production request should wait indefinitely.

---

# 106. Load Shedding

When saturated:

```text
reject early
```

rather than allow queues and memory usage to grow indefinitely.

Use:

```text
Tower concurrency limits
bounded channels
bounded Rayon pool
bounded DB pool
```

---

# 107. Operational Database Namespace

Keep metadata explicit:

```text
aequora_operation_ledger
aequora_journal
aequora_scope_generation
aequora_device
aequora_snapshot
aequora_job
```

Application domain tables remain application-owned.

---

# 108. Supportability

Every issue should be diagnosable using:

```text
server version
client version
protocol version
request ID
OperationId
tenant
device
cursor
error code
trace
```

without inspecting sensitive business payloads.

---

# 109. Sanitized Support Bundle

Optional tool can generate:

```text
version metadata
safe config summary
health state
migration state
metrics snapshot
recent error codes
```

Never include:

```text
tokens
passwords
raw customer payloads
```

---

# 110. Documentation Set

Recommended repository docs:

```text
ARCHITECTURE.md
ACID.md
ENTERPRISE.md
PROTOCOL.md
DEPLOYMENT.md
OPERATIONS.md
SECURITY.md
MIGRATIONS.md
BACKUP-RESTORE.md
RUNBOOKS.md
COMPATIBILITY.md
```

This document is the umbrella `ENTERPRISE.md` design.

---

# 111. Production Checklist

```text
[ ] TLS
[ ] secrets externalized
[ ] migrations verified
[ ] backups enabled
[ ] restore tested
[ ] liveness configured
[ ] readiness configured
[ ] request limits configured
[ ] decompression limits configured
[ ] DB pool bounded
[ ] Rayon pool bounded
[ ] timeouts configured
[ ] retry/backoff configured
[ ] rate limiting configured
[ ] tracing enabled
[ ] metrics enabled
[ ] structured logs enabled
[ ] tenant isolation tested
[ ] device revocation tested
[ ] protocol compatibility tested
[ ] rolling upgrade tested
[ ] migration rollback window tested
[ ] bootstrap tested
[ ] resync tested
[ ] idempotency tested
[ ] outage recovery tested
[ ] load testing passed
[ ] chaos scenarios passed
```

---

# 112. Enterprise Release Gate

Do not call a build production-ready until:

```text
correctness tests pass
security checks pass
adapter compliance passes
backup restore passes
upgrade test passes
rollback strategy exists
observability works
runbooks exist
```

---

# 113. Golden Deployment Path

```text
1. Build signed Rust server artifact.

2. Provision PostgreSQL/Neon.

3. Apply application + Aequora migrations.

4. Configure TLS ingress.

5. Deploy two stateless Axum instances.

6. Configure bounded SQLx pools.

7. Enable liveness/readiness probes.

8. Enable tracing, metrics, structured logs.

9. Enable backups/PITR.

10. Register domain handlers.

11. Roll out to pilot tenant.

12. Observe correctness, latency, journal growth, conflicts.

13. Expand gradually.
```

---

# 114. Golden Client Integration Path

```text
1. Open Stoolap.

2. Initialize Aequora metadata.

3. Create/recover DeviceId.

4. Start SyncCoordinator.

5. Route every synchronized mutation through atomic domain+outbox transaction.

6. Read application UI state from local DB.

7. Reconcile server changes transactionally.

8. Surface pending/conflict/offline status.

9. Bootstrap automatically when required.

10. Keep database-specific sync logic out of UI/domain code.
```

---

# 115. Golden Upgrade Path

```text
expand DB schema
↓
deploy server compatible with old+new
↓
canary
↓
rolling deployment
↓
upgrade clients gradually
↓
observe
↓
contract obsolete schema later
```

---

# 116. Golden Failure Path

Never:

```text
guess commit status
advance cursor early
delete pending work
blindly replay operations with new IDs
hide conflicts
manually edit sync metadata casually
```

Instead:

```text
retry same OperationId
inspect durable state
use typed outcome
bootstrap on invalid timeline
quarantine suspicious data
```

---

# 117. Enterprise Architecture Diagram

```text
                             USERS / DEVICES
                                    │
              ┌─────────────────────┼─────────────────────┐
              ▼                     ▼                     ▼
        Dioxus Desktop         Android Client        Other Client
              │                     │                     │
              └──────────── HTTPS + Postcard ─────────────┘
                                    │
                                    ▼
                         Edge / TLS / Rate Limits
                                    │
                                    ▼
                           Load Balancer / Ingress
                                    │
                  ┌─────────────────┼─────────────────┐
                  ▼                 ▼                 ▼
             Axum Node A       Axum Node B       Axum Node C
                  │                 │                 │
                  └─────────────────┼─────────────────┘
                                    ▼
                         Aequora Sync Service
                                    │
                     ┌──────────────┼──────────────┐
                     ▼              ▼              ▼
                 Auth/AuthZ     Validator      Executor
                                    │
                                    ▼
                            SQLx Connection Pool
                                    │
                                    ▼
                          PostgreSQL / Neon
                                    │
               ┌────────────────────┼────────────────────┐
               ▼                    ▼                    ▼
        Domain State           Sync Journal       Operation Ledger
               │
               └────────────────────┬────────────────────┘
                                    ▼
                            Backup / PITR Layer

Optional:
    object storage
    telemetry collector
    notification worker
    snapshot worker
```

---

# 118. Recommended Enterprise Architecture

The strongest first enterprise deployment is deliberately simple:

```text
CLIENT
Dioxus
+
Stoolap
+
Aequora Client

          ↓ HTTPS/Postcard

EDGE
TLS
+
rate limiting
+
request limits

          ↓

SERVER
Stateless Axum
+
Aequora Server
+
typed validators/executors
+
Tokio
+
bounded Rayon

          ↓

PERSISTENCE
SQLx
+
PostgreSQL / Neon

          ↓

OPERATIONS
PITR/backups
+
tracing
+
metrics
+
structured logs
+
migration gates
+
runbooks
```

---

# 119. Enterprise Guarantees

A properly implemented deployment should provide:

```text
local-first operation
offline writes
durable pending intent
server-authoritative validation
ACID execution
logical exactly-once effects
idempotent retry
multi-device convergence
explicit conflicts
tenant isolation
bounded resource use
horizontal Axum scaling
no sticky sessions
safe rolling upgrades
migration discipline
automatic resynchronization
backup/PITR recovery
cursor timeline reset
device revocation
structured observability
graceful degradation
graceful shutdown
controlled overload
```

---

# 120. Final Architecture Principle

The production goal is not maximum architectural complexity.

It is:

> **A developer should be able to integrate Aequora correctly without becoming a distributed-systems expert, and an operator should be able to deploy, upgrade, recover, and diagnose it without manually manipulating synchronization internals.**

That means the system should make the safe path automatic:

```text
safe defaults
+
small public API
+
strict invariants
+
typed configuration
+
migration gates
+
automatic bootstrap/resync
+
durable retries
+
clear diagnostics
+
tested recovery
```

Only after real production evidence proves a need should Aequora add:

```text
Redis
Kafka
microservices
service mesh
multi-region active-active
custom consensus
```

The enterprise version should first be **boring, correct, observable, recoverable, and easy to operate**.

---

# 121. Implementation Status

Sections 1–120 are implemented at the reusable framework boundary or resolved by an explicit
host/deployment responsibility in [`docs/enterprise-completion.md`](docs/enterprise-completion.md).
The completion record is normative when an illustrative shape in this document would otherwise be
mistaken for an application binary, a mandatory infrastructure dependency, or evidence that a real
production environment passed acceptance.

Repository-owned enterprise guarantees include:

```text
strict, semantically validated RON configuration
safe named deployment profiles
redacted secret wrappers with no serialization surface
bounded global and per-tenant admission
bounded per-tenant request-rate state
bounded body receive and execution deadlines
independent liveness and dependency readiness
irreversible graceful drain with an observable deadline outcome
runtime Normal / ReadOnly / SyncPaused maintenance policy
stable AEQ-* operational error codes over HTTP and QUIC
protocol compatibility and typed upgrade/resync directives
payload-free metrics, tracing hooks, and request correlation
database-neutral local, authoritative, and transport composition
checksummed and locked Stoolap/PostgreSQL migration ledgers
durable retry, idempotency, journal, snapshot, and recovery primitives
reusable adapter contracts and deterministic failure simulations
Guppy dependency boundaries and database-neutrality policy checks
```

The following remain mandatory acceptance work for each real deployment and must never be reported
as passed merely because the framework tests pass:

```text
external TLS and identity-provider policy
secret-manager integration and credential rotation
production PostgreSQL/Neon connectivity and capacity
backup/PITR configuration and a timed restore drill
deployment-specific tenant isolation and authorization tests
metrics/log/trace backend delivery and alert routing
signed artifacts, provenance, SBOM, and image scanning
staging, canary, rolling-upgrade, rollback, chaos, and soak evidence
SLO/error-budget targets approved by the operating organization
retention, legal hold, tenant offboarding, and audit policies
```

No container platform, Redis, Kafka, service mesh, database vendor, authentication product, or
telemetry backend is required by Aequora correctness. A host application owns those edge choices
and composes them around the library contracts.
