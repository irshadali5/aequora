# Part 17 Multi-Region Read and Single-Writer Global Completion

Part 17 is implemented at Aequora's database-neutral reusable boundary. The framework owns
epoch-safe regional contracts and deterministic policy; deployments own databases, replication,
load balancers, object storage, regional networks, and production SLO acceptance.

## Repository guarantees

| Requirement | Evidence |
|---|---|
| Region identity and roles | `RegionId` is a compact deployment-assigned `u16`; `RegionalRole`, `ReplicaId`, `RegionMode`, and `RegionHealth` keep writer, replica, edge, and standby responsibilities explicit. |
| Durable watermarks | `ReplicaWatermark` and `ProjectionWatermark` bind authority identity, epoch, and contiguous applied sequence. Routing never guesses correctness from time lag. |
| Read consistency | `ReadConsistency`, `SessionWatermark`, `EndpointReadPolicy`, `StalenessBudget`, and `ReadFallbackPolicy` express eventual, sequence-bounded, session, and authority reads without implicit weakening. |
| Read-your-writes | `ClientReadSession` advances after trusted commits and reads. A stale regional replica fails its server-side guard; routing may wait only for the configured bound and then use the authority. |
| Routing | `RegionalRouter` selects current-epoch replicas deterministically by health, affinity, priority, load, lag, and identity. `ReadRouteDecision` exposes target, effective consistency, downgrade, and bounded wait metadata. |
| Defense in depth | `ReplicaReadGuard` and `ServerRegionalReadGuard` revalidate role, health, epoch, minimum sequence, projection schema, staleness, and authorization watermark at the serving node. |
| Authority-only writes | `TenantAuthorityDirectory::route_write` accepts only a trusted current tenant assignment and `AuthorityWriter` endpoint. Part 16 fencing remains the authoritative commit boundary. |
| Failover and caches | Writer discovery rejects epoch rollback, removes old-epoch replicas after transition, and `RegionalCacheMetadata` binds authority, epoch, sequence, and projection generation. |
| Regional projections | Projection watermarks expose durable consumer progress; `validate_projection_apply` permits idempotent duplicates and rejects epoch mismatch or sequence gaps. Adapters own the projection database and atomic cursor commit. |
| Snapshots and blobs | `RegionalArtifactEvidence` requires authority signature and content digest evidence, current epoch, and residency-valid origin/delivery regions before activation. Edge location cannot change artifact authority. |
| Residency and keys | `ResidencyPolicy` verifies authority, replica/projection, snapshot, blob, cache, backup, and encryption-key placement against the tenant allow-list. |
| Tenant sharding | Rollback-protected trusted `TenantAuthorityAssignment` records map tenant to shard, authority, epoch, authority region, and residency. Each shard still has one writer. |
| Governance | `RegionalGovernanceRegistry` tracks every registered replica, projection, snapshot, blob, cache, search index, and backup copy; completion requires every required copy to be verified or explicitly policy-exempt. |
| Security | Authorization-sensitive requests require a verified authorization sequence and never use an absent/stale security watermark. High-risk operations can declare authority-only reads. |
| Operations | `aequora region status`, `route`, and `explain` are bounded read-only diagnostics over RON topology evidence. `RegionalEventKind` provides payload-free, low-cardinality operational categories. |
| Boundaries | `aequora-dev check` forbids `aequora-region` and `aequora-authority` from reaching runtimes, transports, servers, clients, or production database adapters. Database-neutrality remains a complementary gate. |
| Invariants/tests | `AEQ-INV-REG001` through `REG009`, unit contracts, client/server testkit scenarios, partition/failover/governance tests, and deterministic large-replica routing run in CI. |

## Deliberate initial boundaries

Sync push and pull continue to use the authority. Regional replicas accelerate ordinary read APIs;
they do not advance sync cursors or execute domain operations. A future split pull path must prove
journal and projection consistency through its served watermark before it can be enabled.

Local-first native clients still read synchronized state from their local database. Regional API
routing primarily serves web clients, administrative APIs, server-rendered pages, search,
analytics, and large datasets that are not locally replicated.

## Deployment responsibilities

Deployments map PostgreSQL WAL/LSN or a projection consumer cursor to the durable Aequora sequence,
publish authenticated health/control-plane observations, provision regional databases and
object/CDN storage, enforce physical/KMS residency, authenticate internal forwarding, propagate
revocation, and fence the old writer during Part 16 failover. They also own regional purge
execution, CDN invalidation, backup verification, broker/poll fallback, and DNS/global-load-balancer
behavior.

Production acceptance must measure read latency, write RTT, fallback rate, sequence lag, projection
stall recovery, snapshot edge hit ratio, and cross-region egress. It must inject replica lag,
region partition/loss, stale routing metadata, broker outage, same-epoch failover, new-epoch
transition, and governance cleanup against real infrastructure. A reusable library cannot claim
those deployment-specific results without the target regions and credentials.
