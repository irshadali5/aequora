# Aequora Sync — Part 45

# Deployment Topologies, Single-Node, HA, Multi-Region, Edge, Enterprise, Air-Gapped, and Operational Environment Architecture

## 1. Purpose

Aequora must preserve the same synchronization semantics across very different deployment environments, from one developer machine to highly available enterprise and global deployments.

The central rule is:

> **Deployment topology may change where components run, but it must not change who is authoritative, how operations are deduplicated, how cursors advance, or how durability is defined.**

Aequora should support progressive operational complexity:

```text
Level 0 — Local Development
Level 1 — Single-Node Production
Level 2 — Single-Region High Availability
Level 3 — Enterprise Cluster
Level 4 — Multi-Region Read Distribution
Level 5 — Partitioned Authority by Tenant/Domain
Level 6 — Air-Gapped / Disconnected Enterprise
```

Across every level these remain invariant:

```text
OperationId idempotency
authoritative journal
authority epoch
cursor semantics
client Tx A
server Tx B
client Tx C
schema/version negotiation
tenant authorization
adapter capability contracts
```

Topology answers **where components run**. Aequora semantics answer **who may commit authoritative state and what a committed operation means**.

---

## 2. Logical Deployment Components

A complete deployment can contain:

```text
Aequora Server Nodes
Authoritative PostgreSQL / Neon
Load Balancer / Reverse Proxy
Snapshot / Blob Object Store
Background Workers
Control Plane
Observability Stack
Identity Provider
Secrets / KMS
Backup / PITR
Regional Read Infrastructure
Desktop / Mobile Clients
Optional Desktop Agent
```

`aequora-server` should be horizontally scalable because correctness-critical state remains in shared durable infrastructure.

Server memory may cache registry metadata, auth metadata, read projections, and similar data, but loss of a process must not lose authoritative state.

The authoritative PostgreSQL/Neon database owns:

```text
business state
operation ledger
journal
timeline head
authority epoch
entity/aggregate versions
required audit
durable jobs
retention metadata
```

SQLite and Stoolap are client/local embedded adapters, not multi-user server authority substitutes.

Large snapshots and blobs should normally live outside the primary SQL database in object storage, with filesystem-backed storage acceptable for development and simple deployments.

---

# 3. Topology 0 — Local Development

```text
Developer Machine
├── aequora-server
├── PostgreSQL
├── Dioxus / test client
└── SQLite or Stoolap local store
```

Goals:

```text
fast startup
easy reset
fault injection
debugging
reproducibility
```

PostgreSQL may run natively, under Podman/Docker, or through a disposable test harness. A container runtime must not be mandatory for Aequora itself.

Development snapshots may use a local filesystem store.

Development auth may use a test identity provider or fixed developer identity, but only under a build/profile that can never be mistaken for production.

Device testing may expose the server on a trusted LAN while retaining normal authentication and protocol behavior.

---

# 4. Topology 1 — Single-Node Production

```text
Clients
   |
   v
TLS Reverse Proxy
   |
   v
Aequora Server
   |
   v
PostgreSQL / Neon
   |
   +--> Object Storage
   +--> Backup / PITR
```

This is appropriate for:

```text
small schools
small companies
internal applications
early SaaS
single-tenant installations
```

If the server host fails, local-first clients retain durable outboxes and continue permitted local reads/writes. Synchronization resumes later using the same `OperationId`s.

Even in a simple deployment, server-process availability and authoritative data durability should be treated as different responsibilities. Managed PostgreSQL/Neon is often preferable to permanently placing every responsibility on one machine.

Minimum backup strategy:

```text
PostgreSQL backup
PITR where available
object-store backup/versioning
configuration backup
release/registry/migration records
```

Upgrade sequence:

```text
verify backup/readiness
expand schema
deploy compatible binary
health check
complete deferred migration/contract step
```

---

# 5. Topology 2 — Single-Region High Availability

```text
                Load Balancer
             /        |        \
            v         v         v
        Server A   Server B   Server C
             \       |       /
              \      |      /
               v     v     v
              PostgreSQL HA
```

All server nodes share:

```text
same authority database
compatible registry
compatible configuration
overlapping protocol support
```

Normal synchronization requires no sticky sessions. A request can land on any healthy node because idempotency, journal, ledger, timeline, and authority state are in shared durable storage.

The load balancer may perform TLS termination, health routing, connection balancing, and transport-level protection, but Aequora correctness cannot depend on affinity.

Forwarded headers must be trusted only from configured proxies.

The HA PostgreSQL layer may be managed PostgreSQL, Neon, PostgreSQL primary/replica infrastructure, or an enterprise cluster. Aequora still expects **one current authoritative writer timeline**.

A process/node restart does not change `AuthorityEpoch`.

Database failover:
- if authority continuity is demonstrably preserved, the epoch may remain;
- if continuity cannot be proven, a new epoch is required.

Background workers may run inside API nodes or as separate processes. Durable job leases, fencing, and idempotency make multiple workers safe.

---

# 6. Topology 3 — Enterprise Cluster

```text
Users / Devices
      |
      v
Enterprise Edge / WAF
      |
      v
Load Balancer
      |
  +---+------------------+
  |                      |
  v                      v
Sync Data Plane      Admin Control Plane
  |                      |
  +----------+-----------+
             |
             v
      PostgreSQL Authority
             |
      +------+-------+
      |              |
      v              v
Object Store     Backup / Archive
```

Enterprise deployments may additionally contain:

```text
enterprise IdP
mTLS PKI
KMS/HSM
central logging/metrics/tracing
SIEM
private control-plane networking
separate worker pools
DR environment
```

The data plane handles normal sync/bootstrap/application operations.

The control plane handles:

```text
admin actions
authority transitions
repairs
consumer cursor resets
governance
runtime policy
```

The control plane should usually have stronger network restrictions such as VPN, private ingress, mTLS, or zero-trust access in addition to application authorization.

Enterprise identity sources such as OIDC, SAML gateways, service identities, and device certificates are normalized into Aequora `AuthContext`.

Network location is not authorization.

Typical segmentation:

```text
public edge
application network
database network
control network
management network
backup network
```

Clients must never receive PostgreSQL credentials.

Server secrets should be supplied through Part 43 secret providers.

Business/security audit must remain durable and separate from ephemeral operational logs.

---

# 7. Topology 4 — Multi-Region Read Distribution

Aequora's preferred early global topology is:

```text
            Authoritative Writer Region
          +-----------------------------+
          | API Writers + PostgreSQL    |
          +-----------------------------+
                        |
              replication / export
                /       |       \
               v        v        v
           Region B  Region C  Region D
             reads      reads      reads
```

The rule remains:

> **Distribute read latency before distributing authority.**

Writes route to the authority region.

Regional infrastructure can serve:

```text
read projections
analytics
historical views
snapshot chunks
blob downloads
static content
```

Initial sync push/pull should usually go directly to the authority.

A regional proxy may terminate TLS, perform safe routing, or cache immutable artifacts, but may never fabricate authoritative operation outcomes.

Read consistency must be explicit:

```text
Eventual
AtLeast(cursor)
Session
Authority
```

Read-your-writes can use a session watermark. If a regional replica has not caught up, the system can wait, route to authority, or return an explicitly weaker read according to product policy.

Snapshot/blob artifacts may be regionally mirrored or CDN-distributed, but clients verify hashes/signatures.

Loss of a read region does not change `AuthorityEpoch`.

Loss of the authority region invokes Part 16 failover semantics.

DNS or a traffic-manager route change does not itself create authority.

---

# 8. Topology 5 — Partitioned Authority by Tenant

Very large installations may assign tenants to different authoritative regions:

```text
Tenant A -> India authority
Tenant B -> EU authority
Tenant C -> US authority
```

Each tenant still has exactly one current authoritative timeline.

A control-plane authority directory can map:

```text
TenantId -> AuthorityEndpoint
```

Clients resolve their tenant's authority after authentication/bootstrap.

Tenant migration between regions is a controlled operation:

```text
freeze/drain
snapshot/replicate
verify
fence old writer
promote destination
create new epoch if required
update authority directory
resume
```

This topology helps with data residency and latency without turning one tenant into an uncontrolled multi-writer system.

Avoid distributed transactions across tenants. Prefer asynchronous integration operations.

Future domain-specific authorities may exist only where domain transaction boundaries are truly independent. They should not be introduced in v1 merely for architectural elegance.

---

# 9. Edge and Remote-Site Deployment

Edge environments include:

```text
schools with unreliable WAN
warehouses
clinics
field offices
remote teams
```

The primary edge strategy is already Aequora's local-first client:

```text
UI
↓
Aequora client
↓
SQLite / Stoolap
```

WAN loss should not stop locally permitted work.

A site-local machine may be introduced as:

```text
cache
artifact mirror
proxy
relay
```

but should not silently become another authority.

A store-and-forward relay can temporarily carry encrypted/opaque operations, but it must never alter:

```text
OperationId
semantic payload
device identity
authority outcome
```

A site may mirror snapshots, blobs, and release packages to save bandwidth.

If a deployment truly requires a disconnected site to accept authoritative writes, that becomes an explicit authority-delegation or independent-authority design and should be treated as future scope—not as a proxy configuration switch.

---

# 10. Air-Gapped Deployment

```text
Internal Clients
      |
      v
Internal Load Balancer
      |
      v
Aequora Server Cluster
      |
      v
Internal PostgreSQL
      |
      +--> Internal Object Store
      +--> Internal Backups
      +--> Internal Observability
```

Core Aequora must not require:

```text
cloud telemetry
public package registries
public update service
external SaaS identity provider
```

Possible internal identity:

```text
internal OIDC
LDAP/SAML gateway
mTLS PKI
```

Releases enter the environment as signed release bundles:

```text
transfer
↓
verify signature
↓
verify hashes
↓
import to internal repository
↓
deploy
```

Offline revocation metadata can similarly be delivered as signed bundles.

Air-gapped deployments require internal trusted time, DNS/service discovery, artifact mirrors, and backup infrastructure where appropriate.

Aequora's journal ordering still does not depend on wall-clock accuracy.

---

# 11. Containers and Kubernetes

A typical server can ship as an OCI image with:

```text
non-root runtime
minimal base
read-only filesystem where practical
mounted/provided secrets
external PostgreSQL
external object storage
```

Kubernetes is supported but never required.

Typical Kubernetes resources:

```text
Deployment        API
Deployment        workers
Service
Ingress/Gateway
ConfigMap
secret-provider integration
PodDisruptionBudget
HPA
NetworkPolicy
```

PostgreSQL should not be casually placed inside the same ephemeral application pod.

Readiness requires:

```text
valid config
valid registry
compatible migrations
DB connectivity
authority metadata
required adapter capabilities
```

Liveness should test the process itself rather than fail whenever a remote dependency is temporarily unavailable.

Autoscaling may use CPU, concurrency, queue pressure, and latency. More API pods cannot compensate for database saturation or a timeline hotspot.

API nodes should not require persistent volumes for authoritative state.

---

# 12. Traditional Linux / systemd Deployment

Aequora should also support a simple native Linux deployment:

```text
/usr/bin/aequora-server
/etc/aequora/server.ron
systemd unit
external PostgreSQL
```

Possible systemd hardening:

```text
NoNewPrivileges
ProtectSystem
PrivateTmp
ProtectHome
CapabilityBoundingSet
systemd credentials
```

Exact permissions depend on snapshot/log/socket requirements.

This profile is valuable for small deployments and air-gapped environments where Kubernetes would be unnecessary complexity.

---

# 13. Desktop and Mobile Runtime Placement

Desktop:

```text
Dioxus GUI
   |
   +--> in-process Aequora runtime
   or
   +--> authenticated IPC -> aequora-agent
```

Mobile:

```text
Android/iOS UI
   |
   v
Rust Aequora client runtime
   |
   v
SQLite/Stoolap local adapter
```

The server's infrastructure topology remains largely invisible to the client beyond:

```text
authority endpoint
authentication
capabilities
snapshot locations
```

---

# 14. Networking Architecture

Primary transport:

```text
HTTPS + Postcard
```

Future transports such as QUIC or gRPC must reuse the same server core.

Production requires TLS.

TLS may terminate at:

```text
Aequora server
trusted proxy
trusted service mesh
```

but the trust boundary must be explicit.

mTLS is especially useful for service-to-service, admin/control plane, or high-assurance device identity.

A service mesh may improve operations but cannot replace core Aequora authentication, authorization, idempotency, or authority semantics.

Minimal paths:

```text
clients -> API
API -> PostgreSQL
API/workers -> object storage
API -> IdP/KMS where configured
telemetry exporters -> collectors
```

Use deny-by-default networking where practical.

Restrict server egress, especially around webhook and SSRF-capable integrations.

---

# 15. Proxy and Timeout Architecture

Infrastructure timeout layers should be deliberately ordered.

Conceptually:

```text
DB statement timeout
<
server operation timeout
<
reverse proxy timeout
<
client wait timeout
```

with operation-specific exceptions.

A client timeout means the client stopped waiting—not that the authoritative operation failed.

A load balancer should not blindly retry arbitrary non-idempotent HTTP requests. Aequora's own retry layer uses `OperationId` and durable idempotency.

Long-running tasks such as snapshots, exports, imports, and repairs should become durable jobs instead of very long request handlers.

---

# 16. PostgreSQL Connection Budget

Each API node has a bounded pool.

Fleet planning:

```text
API nodes × connections per node
+ worker connections
+ migration/admin reserve
<= PostgreSQL connection capacity
```

More server replicas without connection budgeting can make reliability worse.

Pooling proxies such as PgBouncer may be used only after transaction/session behavior is validated against Aequora's PostgreSQL adapter.

Authority Tx B always uses the writer, never a read replica.

Reserve operational DB capacity for migrations and emergency control-plane work.

---

# 17. Timeline Scalability

Part 37's per-tenant timeline head is preferred over one global timeline row.

Monitor:

```text
timeline lock wait
journal append latency
transaction retries
```

before introducing more complex partitioning.

Physical table/index partitioning by tenant is an optimization, not a protocol change.

---

# 18. Object Storage Topologies

A snapshot/blob provider can be:

```text
filesystem
single-region object store
multi-region replicated object store
enterprise S3-compatible service
```

Aequora trusts an artifact only after validating its expected digest/signature.

Storage-provider integrity alone is insufficient.

Large blobs should use dedicated upload/download mechanisms, not unbounded normal sync messages.

CDNs can accelerate authorized immutable content, snapshot chunks, and release packages.

---

# 19. Backup and Disaster Recovery

Every production topology requires a defined backup strategy for:

```text
PostgreSQL
object storage
configuration
registry artifacts
migration artifacts
release metadata
trust/signing metadata where appropriate
```

PostgreSQL PITR is recommended for important deployments.

A database restore is not merely a database problem. Aequora must determine whether the restored authority preserves timeline continuity.

If a restore loses previously acknowledged authoritative commits:

```text
new AuthorityEpoch
```

is normally required.

Periodically restore backups into an isolated environment and run:

```text
integrity verification
journal/snapshot verification
bootstrap test
```

Define explicit:

```text
RPO — acceptable data-loss window
RTO — acceptable restoration time
```

Local client outboxes help users survive outages, but they are not a substitute for authoritative backups.

---

# 20. Disaster Promotion

A recovery-region promotion sequence:

```text
detect failure
↓
fence old writer
↓
determine continuity
↓
restore/promote candidate
↓
increment epoch if continuity uncertain
↓
publish authority endpoint
↓
verify journal/ledger
↓
resume synchronization
```

The largest HA danger is split-brain: two active writers.

Use infrastructure fencing plus Aequora authority fencing.

High-assurance deployments may maintain an external epoch/checkpoint anchor.

---

# 21. Observability by Deployment

All production deployments should expose:

```text
request rate
sync latency
DB pool wait
transaction latency
timeline lock wait
journal append rate
duplicate-operation rate
job backlog
snapshot throughput
compatibility errors
```

Small systems may use host-local monitoring.

HA and enterprise systems should aggregate telemetry centrally.

Operational logs must not replace durable audit.

A telemetry backend outage should not take down synchronization; exporters need bounded queues/drop policies.

Do not alert merely because individual local-first clients are offline.

Alert on meaningful fleet/system conditions such as:

```text
authority unavailable
journal failures
DB saturation
disk pressure
backup failure
epoch mismatch
migration mismatch
unexpected rejection/conflict spikes
```

---

# 22. Configuration and Fleet Drift

Track on every server:

```text
ReleaseVersion
BuildId
ConfigGeneration
ConfigDigest
RegistryGeneration
RegistryDigest
ProtocolRange
```

The control plane/doctor tooling can identify mixed fleet state.

Mixed versions during a rolling upgrade are allowed only under the compatibility rules defined in Parts 21 and 44.

Production startup should usually **check** migrations and refuse an incompatible schema, rather than every API node trying to migrate the database concurrently.

Use one migration owner/lock.

Small single-node deployments may optionally auto-migrate through a guarded profile.

---

# 23. Maintenance and Degraded Modes

Operational modes:

```text
Normal
ReadOnly
Maintenance
Degraded
```

ReadOnly:
- reject authoritative writes;
- permit reads/diagnostics;
- clients retain outbox entries.

Maintenance:
- intended for schema migration, authority transition, or repair.

Degraded:
- shed optional work such as analytics, bulk exports, or expensive background tasks while retaining core sync.

Brownout policy should protect correctness-critical traffic first.

---

# 24. Multi-Tenant Deployment Options

Shared SaaS:

```text
shared server fleet
shared PostgreSQL cluster
shared object storage
strict TenantId isolation
```

Dedicated enterprise:

```text
dedicated server fleet
dedicated database
dedicated object storage
```

Hybrid:

```text
shared control plane
dedicated tenant data plane
```

Every request still binds `TenantId` to `AuthContext`.

PostgreSQL RLS may provide defense in depth but does not replace application authorization.

---

# 25. Deployment Descriptor

Concept:

```rust
pub struct DeploymentDescriptor {
    pub deployment_id: DeploymentId,
    pub environment: Environment,
    pub topology: TopologyKind,
    pub release: ReleaseVersion,
    pub authority_region: Option<RegionId>,
}
```

Possible topology enum:

```rust
pub enum TopologyKind {
    LocalDevelopment,
    SingleNode,
    SingleRegionHa,
    Enterprise,
    MultiRegionRead,
    TenantPartitioned,
    AirGapped,
}
```

This metadata aids diagnostics and runbooks. It must never become a shortcut around correctness rules.

---

# 26. Infrastructure as Code

Aequora can provide reference deployments for:

```text
systemd
OCI container
Kubernetes
air-gapped package install
```

Organizations may manage them with:

```text
Terraform
Pulumi
Ansible
Nix
Kubernetes manifests
```

Aequora should not mandate one infrastructure tool.

Repository structure can include:

```text
deploy/
├── local/
├── systemd/
├── container/
├── kubernetes/
└── air-gapped/
```

Reference deployments are examples, not hidden runtime dependencies.

Production secrets must never be committed inside IaC.

---

# 27. Environment Promotion

Promote immutable artifacts:

```text
development
↓
staging
↓
production
```

without rebuilding whenever practical.

Configuration differs by environment; signed release bytes do not.

Canaries may be:

```text
subset of nodes
subset of tenants
shadow environment
```

Shadow traffic should avoid duplicating real side effects.

Blue/green versions must both be compatible with the expanded DB schema during transition.

---

# 28. Graceful Shutdown and Crash Behavior

Graceful API shutdown:

```text
mark node unready
stop accepting new traffic
drain bounded requests
release worker leases
flush bounded telemetry
exit
```

Abrupt node loss remains safe because authoritative transactions, operation idempotency, and durable leases exist outside the process.

Server↔DB partition:
- fail safely;
- never claim success unless commit outcome is known;
- ambiguous commit is resolved by retrying the same `OperationId`.

Client↔server partition:
- normal local-first condition;
- durable outbox retries later.

---

# 29. Security Baseline

Minimum production baseline:

```text
TLS
strong authentication
server-side authorization
least-privilege DB roles
secret provider
signed/pinned releases
backup/PITR
audit
monitoring
```

Possible host hardening:

```text
minimal packages
restricted administration
firewall
SELinux/AppArmor
disk encryption
```

Database hardening:

```text
TLS
private network
least privilege
encrypted backups
```

Object storage:

```text
private access
least privilege
encryption
versioning
retention lock where required
```

Managed enterprise devices remain untrusted from Aequora's authority perspective.

---

# 30. Deployment Doctor

`aequora doctor` should understand deployment profiles.

HA checks:

```text
fleet version compatibility
registry digest agreement
migration state
authority metadata
worker fencing/leases
DB connectivity/capability
```

Multi-region checks:

```text
authority region
replica lag
snapshot replication
routing directory
epoch consistency
```

Air-gapped checks:

```text
no accidental public dependency
local trust roots
internal identity
internal object storage
release signature verification
```

---

# 31. Failure Testing

Deployment verification should intentionally test:

```text
kill API node
kill worker
drop DB connection
object-store outage
read-region loss
secret rotation/outage
disk full
PITR restore
authority promotion
```

Chaos testing is useful only when tied to explicit failure hypotheses.

Production/enterprise environments should periodically rehearse:

```text
backup restore
authority failover
rolling upgrade
client reconnect
```

---

# 32. Scaling Strategy

Capacity depends on:

```text
operations/sec
active clients
journal growth
batch size
PostgreSQL CPU/IO
pool wait
snapshot bandwidth
worker lag
```

Recommended progression:

```text
measure
↓
optimize query/index/transaction shape
↓
tune DB and pools
↓
scale API horizontally
↓
separate/scale workers
↓
add regional reads
↓
partition tenant authorities
```

Do not start with multi-region complexity.

---

# 33. Optional Infrastructure

Core Aequora correctness must not require:

```text
Redis
Kafka
NATS
Kubernetes
Consul
service mesh
```

Optional uses:

Redis/NATS:
```text
ephemeral hints
fan-out
cache
```

Kafka:
```text
large downstream change-feed integration
```

Neither becomes the authoritative journal or idempotency ledger.

---

# 34. Topology Evolution

The same Aequora product should be able to evolve:

```text
Single Node
    ↓
Single-Region HA
    ↓
Dedicated Worker Pools
    ↓
Regional Reads
    ↓
Tenant-Partitioned Authority
```

without redesigning the client protocol.

Clients interact with an `AuthorityEndpoint` or endpoint discovery abstraction rather than assuming one physical server forever.

Changing an endpoint does not change:

```text
TenantId
DeviceId
OperationId
```

Authority epoch changes only when required by continuity semantics.

---

# 35. Deployment Invariants

### AEQ-INV-DEPLOY001

```text
Exactly one authoritative writer timeline exists for each active authority scope/tenant at a time.
```

### AEQ-INV-DEPLOY002

```text
Adding nodes, load balancers, regions, proxies, or workers cannot change OperationId idempotency, journal ordering, cursor semantics, or Tx A/B/C durability.
```

### AEQ-INV-DEPLOY003

```text
Server process restart or horizontal node replacement does not create a new AuthorityEpoch.
```

### AEQ-INV-DEPLOY004

```text
Clients never connect directly to the authoritative database and never receive credentials capable of bypassing server domain/authz execution.
```

### AEQ-INV-DEPLOY005

```text
Regional read infrastructure serves data only under explicit consistency semantics and cannot accept authoritative writes unless formally promoted.
```

### AEQ-INV-DEPLOY006

```text
Air-gapped deployments preserve cryptographic verification, compatibility, authority, audit, and upgrade semantics without public-cloud dependencies.
```

### AEQ-INV-DEPLOY007

```text
Infrastructure retries, failover, load balancing, and duplicate workers cannot produce duplicate logical authoritative effects beyond OperationId semantics.
```

### AEQ-INV-DEPLOY008

```text
A restore/failover that cannot prove timeline continuity establishes a new AuthorityEpoch before synchronization resumes.
```

### AEQ-INV-DEPLOY009

```text
Correctness-critical state exists only in certified durable storage, never solely in pod/node/process-local caches.
```

### AEQ-INV-DEPLOY010

```text
Aequora's core correctness does not require Kubernetes, Redis, Kafka, NATS, a service mesh, or a public cloud.
```

---

# 36. Recommended Deployment Profiles

## Profile A — Developer

```text
1 local server
local PostgreSQL
filesystem snapshots
test auth
SQLite/Stoolap client
```

## Profile B — Small Production

```text
1 Aequora server
managed PostgreSQL/Neon
object storage
TLS
backup/PITR
```

## Profile C — Standard SaaS HA

```text
3 server nodes
load balancer
managed PostgreSQL HA
object storage
central observability
```

## Profile D — Enterprise

```text
HA API
separate workers
private control plane
enterprise IdP
KMS/HSM
central audit/telemetry
DR
```

## Profile E — Global Read

```text
single writer region
regional reads
regional snapshot mirrors
global routing
```

## Profile F — Tenant-Partitioned Global

```text
tenant-specific authority region
authority directory
regional HA per partition
```

## Profile G — Air-Gapped

```text
internal server HA
internal PostgreSQL
internal IdP
internal object store
offline signed release bundles
```

---

# 37. Recommended v1 Production Architecture

For most early Aequora products:

```text
Clients
   |
   v
HTTPS Reverse Proxy / Load Balancer
   |
   v
1–3 Aequora Server Nodes
   |
   v
PostgreSQL / Neon
   |
   +--> Object Storage
   +--> Backup / PITR
```

Client:

```text
Dioxus / Native UI
        |
        v
Aequora Client Runtime
        |
        v
SQLite or Stoolap
```

This provides a strong production base without unnecessary distributed-system dependencies.

---

# 38. What Not to Add Early

Avoid defaulting to:

```text
Kafka
Redis
NATS
Kubernetes
multi-master PostgreSQL
cross-region consensus
service mesh
custom service discovery
```

unless measured requirements justify them.

---

# 39. Deployment Completion Checklist

```text
[ ] deployment profile selected
[ ] authoritative writer location defined
[ ] TLS configured
[ ] authentication/authz configured
[ ] DB role least privilege
[ ] secret provider configured
[ ] PostgreSQL backups/PITR configured
[ ] object store configured
[ ] exact release pinned
[ ] registry/config digest recorded
[ ] migrations verified
[ ] readiness/liveness configured
[ ] metrics/logs/traces configured
[ ] admin plane restricted
[ ] worker fencing verified
[ ] restore drill completed
[ ] authority failover policy documented
[ ] epoch-transition runbook documented
[ ] local-first outage behavior tested
[ ] topology-aware doctor passes
```

---

# 40. Final Architecture

```text
                         Clients
                   mobile / desktop
                          |
                          v
                   SQLite / Stoolap
                          |
                          v
                        HTTPS
                          |
                 +--------+--------+
                 | Load Balancer   |
                 +--------+--------+
                          |
              +-----------+-----------+
              |           |           |
              v           v           v
           API A       API B       API C
              \           |           /
               \          |          /
                +----------+---------+
                           |
                           v
                Authoritative PostgreSQL
                           |
                 +---------+---------+
                 |                   |
                 v                   v
          Snapshot / Blob Store   Backup / PITR
                 |
                 v
          Regional Mirrors / CDN
```

Enterprise additions:

```text
IdP
KMS/HSM
Control Plane
Central Observability
DR Site
Regional Reads
Tenant Authority Directory
```

These extend deployment capability without becoming alternate synchronization semantics.

---

# 41. Final Recommendation

Aequora should begin operationally simple and increase infrastructure complexity only when measurement and business requirements justify it.

Preferred progression:

```text
single node
↓
single-region HA
↓
separate worker pools
↓
regional reads
↓
tenant-level authority partitioning
```

while preserving one clear authority for every timeline.

> **Scale infrastructure around Aequora's authority model; never mutate the authority model merely to make the infrastructure look distributed.**

This lets the same client, protocol, operation model, adapter contracts, and correctness invariants work from a developer laptop through SaaS HA, global reads, dedicated enterprise deployments, and completely air-gapped environments.

---

## Next

**Part 46 — Observability Implementation, Metrics, Distributed Tracing, Structured Logging, SLOs, Alerting, and Production Telemetry Architecture**
