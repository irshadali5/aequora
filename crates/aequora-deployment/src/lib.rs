//! Deployment-topology contracts that preserve Aequora's authority semantics.
//!
//! Infrastructure adapters may translate these validated descriptors into systemd, OCI,
//! Kubernetes, or enterprise-specific resources. This crate deliberately contains no database,
//! transport, cloud, orchestration, or operating-system client.

use aequora_config::Environment;
use aequora_types::{AuthorityEpoch, RegionId, TenantId};
pub use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

/// Version of the checked-in deployment descriptor schema.
pub const DEPLOYMENT_DESCRIPTOR_SCHEMA_VERSION: u16 = 1;
const MAX_DEPLOYMENT_RON_BYTES: usize = 1024 * 1024;
const MAX_DEPLOYMENT_NODES: usize = 4_096;
const MAX_AUTHORITY_BINDINGS: usize = 16_384;

/// Stable identity of one logical deployment across node replacement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DeploymentId(Uuid);

impl DeploymentId {
    /// Creates a new time-ordered deployment identity.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Creates an identity from an existing UUID.
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }
}

impl Default for DeploymentId {
    fn default() -> Self {
        Self::new()
    }
}

/// Supported operational topology without naming a cloud or orchestrator.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum TopologyKind {
    /// One-machine development environment.
    LocalDevelopment,
    /// One production application node.
    SingleNode,
    /// Multiple replaceable nodes sharing one writer authority.
    SingleRegionHa,
    /// Segmented data plane, control plane, and worker pools.
    Enterprise,
    /// One writer region with explicitly consistent regional reads.
    MultiRegionRead,
    /// Independent single-writer authority scopes partitioned by tenant.
    TenantPartitioned,
    /// Environment with no dependency on a public network or public service.
    AirGapped,
}

/// Runtime mode used to preserve critical sync traffic during operations.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum OperationalMode {
    /// All configured work is permitted.
    #[default]
    Normal,
    /// Authoritative writes are rejected while reads and diagnostics continue.
    ReadOnly,
    /// Schema, authority-transition, or repair work owns the deployment.
    Maintenance,
    /// Optional and bulk work is shed while core synchronization continues.
    Degraded,
}

/// Explicit consistency requested from regional read infrastructure.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReadConsistency {
    /// Any valid regional projection may answer.
    Eventual,
    /// A replica must cover the supplied authoritative cursor.
    AtLeast(u64),
    /// A replica must cover the caller's session watermark.
    Session(u64),
    /// The request must be served by the authority region.
    Authority,
}

/// Process roles declared by a deployment node.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum NodeRole {
    /// Public or internal synchronization API.
    Api,
    /// Durable background-job consumer.
    Worker,
    /// Restricted administrative control plane.
    ControlPlane,
    /// Non-authoritative regional read projection.
    ReadReplica,
    /// Immutable snapshot, blob, or release mirror.
    ArtifactMirror,
    /// Opaque store-and-forward edge relay.
    EdgeRelay,
}

/// Correctness semantics that deployment topology is never allowed to configure or redefine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreservedSemantic {
    /// Permanent operation identity and idempotency.
    OperationIdempotency,
    /// Transactionally committed authoritative journal ordering.
    AuthoritativeJournal,
    /// Durable authority timeline fencing.
    AuthorityEpoch,
    /// Cursor-after-durable-apply behavior.
    CursorSemantics,
    /// Atomic local mutation and outbox intent.
    ClientTransactionA,
    /// Atomic authoritative state, ledger, journal, and audit commit.
    AuthorityTransactionB,
    /// Atomic client reconciliation and cursor advancement.
    ClientTransactionC,
    /// Schema and protocol compatibility negotiation.
    VersionNegotiation,
    /// Authenticated tenant isolation and authorization.
    TenantAuthorization,
    /// Environment-bound adapter capability contracts.
    AdapterCapabilities,
}

/// The fixed semantic contract shared by every deployment profile.
pub const PRESERVED_SEMANTICS: [PreservedSemantic; 10] = [
    PreservedSemantic::OperationIdempotency,
    PreservedSemantic::AuthoritativeJournal,
    PreservedSemantic::AuthorityEpoch,
    PreservedSemantic::CursorSemantics,
    PreservedSemantic::ClientTransactionA,
    PreservedSemantic::AuthorityTransactionB,
    PreservedSemantic::ClientTransactionC,
    PreservedSemantic::VersionNegotiation,
    PreservedSemantic::TenantAuthorization,
    PreservedSemantic::AdapterCapabilities,
];

/// Certified durable engine permitted to own the authoritative writer timeline.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthorityStoreKind {
    /// Self-managed or managed `PostgreSQL` authority adapter.
    PostgreSql,
    /// Neon using the same `PostgreSQL` authority semantics.
    Neon,
}

/// Security, durability, and operational capabilities proven by deployment preflight.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DeploymentCapability {
    /// Production TLS is configured at a declared trust boundary.
    Tls,
    /// Strong authentication is configured.
    Authentication,
    /// Server-side tenant authorization is configured.
    Authorization,
    /// Database roles are least-privilege and unavailable to clients.
    LeastPrivilegeDatabaseRole,
    /// Secrets are resolved using a secret provider.
    SecretProvider,
    /// `PostgreSQL` backup and restoration are configured.
    DatabaseBackup,
    /// Point-in-time recovery is configured where required.
    PointInTimeRecovery,
    /// Object storage is private, durable, and backed up or versioned.
    ObjectStorage,
    /// Business and security audit records are durable.
    DurableAudit,
    /// Bounded operational telemetry is configured.
    Observability,
    /// Database migrations are compatible and owned by one migrator.
    MigrationReadiness,
    /// Worker leases use durable fencing.
    WorkerFencing,
    /// The control plane has additional network restrictions.
    RestrictedControlPlane,
    /// Release, registry, and configuration bytes/digests are pinned.
    ImmutableRelease,
    /// Signed offline release bundles are verified before import.
    OfflineReleaseVerification,
    /// Internal identity, DNS, trusted time, artifact, and backup services exist.
    InternalInfrastructure,
    /// Restore and authority-promotion drills have passing evidence.
    DisasterRecoveryDrill,
}

/// A dependency whose presence would violate a truly disconnected profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum PublicDependency {
    /// Public telemetry collector.
    CloudTelemetry,
    /// Public package registry used at runtime or install time.
    PackageRegistry,
    /// Public application-update service.
    UpdateService,
    /// External `SaaS` identity provider.
    IdentityProvider,
    /// Any other required public-cloud API.
    CloudApi,
}

/// One replaceable application, worker, control, or regional read node.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentNode {
    /// Deployment-unique stable diagnostic name.
    pub node_id: String,
    /// Region assigned by deployment control metadata.
    pub region: RegionId,
    /// Roles run by this process or node group.
    pub roles: BTreeSet<NodeRole>,
    /// Immutable product release running on this node.
    pub release: Version,
    /// Effective configuration generation.
    pub config_generation: u64,
    /// Hex digest of the effective secret-free configuration.
    pub config_digest: String,
    /// Effective durable registry generation.
    pub registry_generation: u64,
    /// Hex digest of the durable registry snapshot.
    pub registry_digest: String,
    /// Lowest accepted protocol version.
    pub minimum_protocol: u16,
    /// Highest accepted protocol version.
    pub maximum_protocol: u16,
}

/// A single authoritative writer binding for one global or tenant scope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityBinding {
    /// `None` denotes a deployment-wide authority; partitioned topology uses tenant identities.
    pub tenant: Option<TenantId>,
    /// Region containing the only current authoritative writer.
    pub writer_region: RegionId,
    /// Durable authority timeline epoch, which is independent of process lifecycle.
    pub epoch: AuthorityEpoch,
    /// Certified durable authority storage; embedded client adapters cannot be selected here.
    pub store: AuthorityStoreKind,
}

/// Deliberately ordered infrastructure timeouts in milliseconds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimeoutBudget {
    /// `PostgreSQL` statement deadline.
    pub database_ms: u64,
    /// Server operation deadline.
    pub server_ms: u64,
    /// Trusted reverse-proxy deadline.
    pub proxy_ms: u64,
    /// Client wait deadline.
    pub client_ms: u64,
}

/// Fleet-wide database connection allocation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionBudget {
    /// Number of API node instances.
    pub api_nodes: u32,
    /// Maximum connections in every API-node pool.
    pub connections_per_api_node: u32,
    /// Total connections reserved for workers.
    pub worker_connections: u32,
    /// Capacity retained for migration and emergency administration.
    pub operational_reserve: u32,
    /// Maximum supported connections at the authority database.
    pub database_capacity: u32,
}

impl ConnectionBudget {
    /// Returns the largest planned concurrent connection count.
    #[must_use]
    pub fn planned(self) -> Option<u32> {
        self.api_nodes
            .checked_mul(self.connections_per_api_node)?
            .checked_add(self.worker_connections)?
            .checked_add(self.operational_reserve)
    }

    /// Confirms the fleet remains within database capacity.
    #[must_use]
    pub fn fits(self) -> bool {
        self.planned()
            .is_some_and(|planned| planned <= self.database_capacity)
    }
}

/// Complete, secret-free machine-readable topology description.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentDescriptor {
    /// Descriptor format, independent of product and protocol versions.
    pub schema_version: u16,
    /// Stable logical deployment identity.
    pub deployment_id: DeploymentId,
    /// Configuration environment classification.
    pub environment: Environment,
    /// Selected infrastructure topology.
    pub topology: TopologyKind,
    /// Region to which ordinary writes are routed, when globally defined.
    pub authority_region: Option<RegionId>,
    /// Current brownout or maintenance mode.
    pub mode: OperationalMode,
    /// Replaceable fleet members or node groups.
    pub nodes: Vec<DeploymentNode>,
    /// Exactly one writer binding per active authority scope.
    pub authorities: Vec<AuthorityBinding>,
    /// Preflight-proven deployment capabilities.
    pub capabilities: BTreeSet<DeploymentCapability>,
    /// Public dependencies required by the deployment.
    pub public_dependencies: BTreeSet<PublicDependency>,
    /// Fleet database connection allocation.
    pub connections: ConnectionBudget,
    /// Cross-layer timeout ordering.
    pub timeouts: TimeoutBudget,
}

impl DeploymentDescriptor {
    /// Parses and validates a deployment descriptor from RON.
    ///
    /// # Errors
    ///
    /// Returns an encoding or topology-policy error.
    pub fn from_ron(input: &str) -> Result<Self, DeploymentError> {
        if input.len() > MAX_DEPLOYMENT_RON_BYTES {
            return Err(DeploymentError::Encoding);
        }
        let descriptor = ron::from_str(input).map_err(|_| DeploymentError::Encoding)?;
        Self::validate(&descriptor)?;
        Ok(descriptor)
    }

    /// Validates topology-independent invariants and profile-specific readiness.
    ///
    /// # Errors
    ///
    /// Returns the first fail-closed policy violation.
    pub fn validate(&self) -> Result<(), DeploymentError> {
        if self.schema_version != DEPLOYMENT_DESCRIPTOR_SCHEMA_VERSION {
            return Err(DeploymentError::SchemaVersion);
        }
        if self.nodes.is_empty()
            || self.nodes.len() > MAX_DEPLOYMENT_NODES
            || self.authorities.is_empty()
            || self.authorities.len() > MAX_AUTHORITY_BINDINGS
        {
            return Err(DeploymentError::MissingAuthority);
        }
        if !self.connections.fits() {
            return Err(DeploymentError::ConnectionBudget);
        }
        if !(self.timeouts.database_ms < self.timeouts.server_ms
            && self.timeouts.server_ms < self.timeouts.proxy_ms
            && self.timeouts.proxy_ms < self.timeouts.client_ms)
        {
            return Err(DeploymentError::TimeoutOrder);
        }
        self.validate_nodes()?;
        self.validate_authorities()?;
        self.validate_profile()
    }

    fn validate_nodes(&self) -> Result<(), DeploymentError> {
        let mut names = BTreeSet::new();
        for node in &self.nodes {
            if node.node_id.trim().is_empty()
                || node.roles.is_empty()
                || !names.insert(node.node_id.as_str())
                || node.minimum_protocol == 0
                || node.minimum_protocol > node.maximum_protocol
                || !valid_digest(&node.config_digest)
                || !valid_digest(&node.registry_digest)
            {
                return Err(DeploymentError::InvalidNode);
            }
        }
        Ok(())
    }

    fn validate_authorities(&self) -> Result<(), DeploymentError> {
        let mut scopes = BTreeSet::new();
        for binding in &self.authorities {
            if !scopes.insert(binding.tenant) {
                return Err(DeploymentError::DuplicateAuthorityScope);
            }
            if !self.nodes.iter().any(|node| {
                node.region == binding.writer_region && node.roles.contains(&NodeRole::Api)
            }) {
                return Err(DeploymentError::AuthorityWithoutApi);
            }
        }
        if self.topology == TopologyKind::TenantPartitioned {
            if self
                .authorities
                .iter()
                .any(|binding| binding.tenant.is_none())
            {
                return Err(DeploymentError::TenantAuthorityRequired);
            }
        } else if self.authorities.len() != 1
            || self
                .authorities
                .first()
                .is_none_or(|binding| binding.tenant.is_some())
        {
            return Err(DeploymentError::GlobalAuthorityRequired);
        }
        Ok(())
    }

    fn validate_profile(&self) -> Result<(), DeploymentError> {
        if self.environment != Environment::Development {
            require_all(
                &self.capabilities,
                &[
                    DeploymentCapability::Tls,
                    DeploymentCapability::Authentication,
                    DeploymentCapability::Authorization,
                    DeploymentCapability::LeastPrivilegeDatabaseRole,
                    DeploymentCapability::SecretProvider,
                    DeploymentCapability::DatabaseBackup,
                    DeploymentCapability::DurableAudit,
                    DeploymentCapability::Observability,
                    DeploymentCapability::MigrationReadiness,
                    DeploymentCapability::ImmutableRelease,
                ],
            )?;
        }
        let api_count = self
            .nodes
            .iter()
            .filter(|node| node.roles.contains(&NodeRole::Api))
            .count();
        match self.topology {
            TopologyKind::LocalDevelopment => {
                if self.environment != Environment::Development {
                    return Err(DeploymentError::DevelopmentTopologyInProduction);
                }
            }
            TopologyKind::SingleNode => {
                if api_count != 1 {
                    return Err(DeploymentError::ApiNodeCount);
                }
            }
            TopologyKind::SingleRegionHa => {
                if api_count < 2 || region_count(&self.nodes) != 1 {
                    return Err(DeploymentError::HaTopology);
                }
                require_all(&self.capabilities, &[DeploymentCapability::WorkerFencing])?;
            }
            TopologyKind::Enterprise => {
                if api_count < 2
                    || !has_role(&self.nodes, NodeRole::ControlPlane)
                    || !has_role(&self.nodes, NodeRole::Worker)
                {
                    return Err(DeploymentError::EnterpriseTopology);
                }
                require_all(
                    &self.capabilities,
                    &[
                        DeploymentCapability::WorkerFencing,
                        DeploymentCapability::RestrictedControlPlane,
                    ],
                )?;
            }
            TopologyKind::MultiRegionRead => {
                if self.authority_region.is_none()
                    || region_count(&self.nodes) < 2
                    || !has_role(&self.nodes, NodeRole::ReadReplica)
                {
                    return Err(DeploymentError::MultiRegionTopology);
                }
            }
            TopologyKind::TenantPartitioned => {
                if region_count(&self.nodes) < 2 {
                    return Err(DeploymentError::TenantPartitionTopology);
                }
            }
            TopologyKind::AirGapped => {
                if !self.public_dependencies.is_empty() {
                    return Err(DeploymentError::PublicDependency);
                }
                require_all(
                    &self.capabilities,
                    &[
                        DeploymentCapability::OfflineReleaseVerification,
                        DeploymentCapability::InternalInfrastructure,
                    ],
                )?;
            }
        }
        Ok(())
    }

    /// Produces deterministic, secret-free fleet drift and topology diagnostics.
    #[must_use]
    pub fn doctor(&self) -> DoctorReport {
        let mut findings = Vec::new();
        if let Err(error) = self.validate() {
            findings.push(DoctorFinding::Error(error));
        }
        let releases: BTreeSet<_> = self.nodes.iter().map(|node| &node.release).collect();
        if releases.len() > 1 && !protocol_overlap(&self.nodes) {
            findings.push(DoctorFinding::MixedFleetWithoutProtocolOverlap);
        }
        if distinct(&self.nodes, |node| &node.registry_digest) > 1 {
            findings.push(DoctorFinding::RegistryDrift);
        }
        if distinct(&self.nodes, |node| &node.config_digest) > 1 {
            findings.push(DoctorFinding::ConfigurationDrift);
        }
        DoctorReport { findings }
    }
}

/// One deterministic deployment-doctor observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DoctorFinding {
    /// Descriptor or readiness validation failed.
    Error(DeploymentError),
    /// Mixed releases do not share any supported protocol.
    MixedFleetWithoutProtocolOverlap,
    /// Nodes report different registry digests.
    RegistryDrift,
    /// Nodes report different effective configuration digests.
    ConfigurationDrift,
}

/// Topology-aware doctor result. An empty finding list is ready.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DoctorReport {
    /// Fail-closed readiness and drift findings.
    pub findings: Vec<DoctorFinding>,
}

impl DoctorReport {
    /// Whether no blocking topology or fleet-drift issue was found.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Regional read target selected without granting write authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadTarget {
    /// The regional replica satisfies the declared consistency contract.
    RegionalReplica,
    /// Route to the single authority region.
    Authority,
    /// Wait for a regional replica to reach the required watermark.
    WaitForReplica,
}

/// Selects a read target from explicit consistency and replica progress.
#[must_use]
pub const fn route_read(consistency: ReadConsistency, replica_cursor: u64) -> ReadTarget {
    match consistency {
        ReadConsistency::Eventual => ReadTarget::RegionalReplica,
        ReadConsistency::Authority => ReadTarget::Authority,
        ReadConsistency::AtLeast(required) | ReadConsistency::Session(required) => {
            if replica_cursor >= required {
                ReadTarget::RegionalReplica
            } else {
                ReadTarget::WaitForReplica
            }
        }
    }
}

/// Evidence about authority continuity during restore or failover.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinuityEvidence {
    /// The candidate contains every acknowledged commit on the fenced timeline.
    Proven,
    /// The deployment cannot prove that no acknowledged commit was lost.
    Uncertain,
}

/// Required epoch action before a promoted writer may serve synchronization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EpochAction {
    /// Continue the existing timeline after infrastructure fencing succeeds.
    Preserve,
    /// Establish a new epoch before synchronization resumes.
    IncrementBeforeResume,
}

/// Applies Part 16 continuity semantics to topology-level promotion.
#[must_use]
pub const fn promotion_epoch_action(evidence: ContinuityEvidence) -> EpochAction {
    match evidence {
        ContinuityEvidence::Proven => EpochAction::Preserve,
        ContinuityEvidence::Uncertain => EpochAction::IncrementBeforeResume,
    }
}

/// Fail-closed descriptor validation errors.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DeploymentError {
    /// RON could not be decoded.
    #[error("deployment descriptor encoding is invalid")]
    Encoding,
    /// Descriptor schema is unsupported.
    #[error("deployment descriptor schema is unsupported")]
    SchemaVersion,
    /// No node or authority binding was declared.
    #[error("deployment has no node or authority")]
    MissingAuthority,
    /// Fleet connections exceed database capacity or overflow.
    #[error("database connection budget is unsafe")]
    ConnectionBudget,
    /// Database, server, proxy, and client timeouts are not strictly ordered.
    #[error("infrastructure timeouts are not strictly ordered")]
    TimeoutOrder,
    /// A node identity, digest, role set, or protocol range is invalid.
    #[error("deployment node metadata is invalid")]
    InvalidNode,
    /// More than one writer binding was declared for one scope.
    #[error("authority scope has multiple writer bindings")]
    DuplicateAuthorityScope,
    /// A writer region has no API node.
    #[error("authority writer region has no API node")]
    AuthorityWithoutApi,
    /// Partitioned authority requires tenant-scoped bindings.
    #[error("tenant-partitioned topology requires tenant authority scopes")]
    TenantAuthorityRequired,
    /// Non-partitioned topology requires exactly one global binding.
    #[error("topology requires exactly one global authority scope")]
    GlobalAuthorityRequired,
    /// A required production or topology capability is missing.
    #[error("required deployment capability is absent")]
    MissingCapability,
    /// Local-development topology was selected outside development.
    #[error("local-development topology is forbidden outside development")]
    DevelopmentTopologyInProduction,
    /// Single-node topology did not declare exactly one API node.
    #[error("single-node topology requires exactly one API node")]
    ApiNodeCount,
    /// HA topology lacks multiple same-region API nodes or fencing.
    #[error("single-region HA topology is incomplete")]
    HaTopology,
    /// Enterprise topology lacks its segregated roles.
    #[error("enterprise topology is incomplete")]
    EnterpriseTopology,
    /// Global read topology lacks multiple regions or a read replica.
    #[error("multi-region read topology is incomplete")]
    MultiRegionTopology,
    /// Tenant partitioning does not span authority regions.
    #[error("tenant-partitioned topology is incomplete")]
    TenantPartitionTopology,
    /// An air-gapped descriptor declares a required public dependency.
    #[error("air-gapped deployment requires a public dependency")]
    PublicDependency,
}

fn require_all(
    actual: &BTreeSet<DeploymentCapability>,
    required: &[DeploymentCapability],
) -> Result<(), DeploymentError> {
    if required
        .iter()
        .all(|capability| actual.contains(capability))
    {
        Ok(())
    } else {
        Err(DeploymentError::MissingCapability)
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn has_role(nodes: &[DeploymentNode], role: NodeRole) -> bool {
    nodes.iter().any(|node| node.roles.contains(&role))
}

fn region_count(nodes: &[DeploymentNode]) -> usize {
    nodes
        .iter()
        .map(|node| node.region)
        .collect::<BTreeSet<_>>()
        .len()
}

fn protocol_overlap(nodes: &[DeploymentNode]) -> bool {
    let minimum = nodes.iter().map(|node| node.minimum_protocol).max();
    let maximum = nodes.iter().map(|node| node.maximum_protocol).min();
    minimum.zip(maximum).is_some_and(|(low, high)| low <= high)
}

fn distinct<'a, F>(nodes: &'a [DeploymentNode], field: F) -> usize
where
    F: Fn(&'a DeploymentNode) -> &'a String,
{
    nodes.iter().map(field).collect::<BTreeSet<_>>().len()
}

/// Groups authority bindings by writer region for topology diagnostics.
#[must_use]
pub fn authority_distribution(
    descriptor: &DeploymentDescriptor,
) -> BTreeMap<RegionId, BTreeSet<Option<TenantId>>> {
    let mut result = BTreeMap::new();
    for authority in &descriptor.authorities {
        result
            .entry(authority.writer_region)
            .or_insert_with(BTreeSet::new)
            .insert(authority.tenant);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_descriptor_is_rejected_before_decoding() {
        assert_eq!(
            DeploymentDescriptor::from_ron(&" ".repeat(MAX_DEPLOYMENT_RON_BYTES + 1)),
            Err(DeploymentError::Encoding)
        );
    }

    fn digest(value: char) -> String {
        std::iter::repeat_n(value, 64).collect()
    }

    fn node(name: &str, region: u16, roles: &[NodeRole]) -> DeploymentNode {
        DeploymentNode {
            node_id: name.to_owned(),
            region: RegionId::new(region),
            roles: roles.iter().copied().collect(),
            release: Version::new(1, 0, 0),
            config_generation: 1,
            config_digest: digest('a'),
            registry_generation: 1,
            registry_digest: digest('b'),
            minimum_protocol: 1,
            maximum_protocol: 1,
        }
    }

    fn capabilities() -> BTreeSet<DeploymentCapability> {
        [
            DeploymentCapability::Tls,
            DeploymentCapability::Authentication,
            DeploymentCapability::Authorization,
            DeploymentCapability::LeastPrivilegeDatabaseRole,
            DeploymentCapability::SecretProvider,
            DeploymentCapability::DatabaseBackup,
            DeploymentCapability::DurableAudit,
            DeploymentCapability::Observability,
            DeploymentCapability::MigrationReadiness,
            DeploymentCapability::ImmutableRelease,
            DeploymentCapability::WorkerFencing,
        ]
        .into_iter()
        .collect()
    }

    fn descriptor(topology: TopologyKind, nodes: Vec<DeploymentNode>) -> DeploymentDescriptor {
        DeploymentDescriptor {
            schema_version: 1,
            deployment_id: DeploymentId::from_uuid(Uuid::from_u128(1)),
            environment: Environment::Production,
            topology,
            authority_region: Some(RegionId::new(1)),
            mode: OperationalMode::Normal,
            nodes,
            authorities: vec![AuthorityBinding {
                tenant: None,
                writer_region: RegionId::new(1),
                epoch: AuthorityEpoch::INITIAL,
                store: AuthorityStoreKind::PostgreSql,
            }],
            capabilities: capabilities(),
            public_dependencies: BTreeSet::new(),
            connections: ConnectionBudget {
                api_nodes: 2,
                connections_per_api_node: 10,
                worker_connections: 4,
                operational_reserve: 6,
                database_capacity: 40,
            },
            timeouts: TimeoutBudget {
                database_ms: 1_000,
                server_ms: 2_000,
                proxy_ms: 3_000,
                client_ms: 4_000,
            },
        }
    }

    #[test]
    fn ha_nodes_share_one_durable_authority() {
        let value = descriptor(
            TopologyKind::SingleRegionHa,
            vec![
                node("api-a", 1, &[NodeRole::Api]),
                node("api-b", 1, &[NodeRole::Api]),
            ],
        );
        assert!(value.validate().is_ok());
        assert!(value.doctor().is_ready());
    }

    #[test]
    fn read_replica_never_fabricates_cursor_consistency() {
        assert_eq!(
            route_read(ReadConsistency::AtLeast(20), 19),
            ReadTarget::WaitForReplica
        );
        assert_eq!(
            route_read(ReadConsistency::Session(20), 20),
            ReadTarget::RegionalReplica
        );
        assert_eq!(
            route_read(ReadConsistency::Authority, u64::MAX),
            ReadTarget::Authority
        );
    }

    #[test]
    fn uncertain_restore_requires_new_epoch() {
        assert_eq!(
            promotion_epoch_action(ContinuityEvidence::Uncertain),
            EpochAction::IncrementBeforeResume
        );
    }

    #[test]
    fn capacity_and_timeout_misconfiguration_fail_closed() {
        let mut value = descriptor(
            TopologyKind::SingleRegionHa,
            vec![
                node("api-a", 1, &[NodeRole::Api]),
                node("api-b", 1, &[NodeRole::Api]),
            ],
        );
        value.connections.database_capacity = 10;
        assert_eq!(value.validate(), Err(DeploymentError::ConnectionBudget));
        value.connections.database_capacity = 40;
        value.timeouts.proxy_ms = value.timeouts.server_ms;
        assert_eq!(value.validate(), Err(DeploymentError::TimeoutOrder));
    }

    #[test]
    fn air_gap_rejects_public_dependencies() {
        let mut value = descriptor(
            TopologyKind::AirGapped,
            vec![node("internal-api", 1, &[NodeRole::Api])],
        );
        value.connections.api_nodes = 1;
        value
            .capabilities
            .insert(DeploymentCapability::OfflineReleaseVerification);
        value
            .capabilities
            .insert(DeploymentCapability::InternalInfrastructure);
        value
            .public_dependencies
            .insert(PublicDependency::CloudTelemetry);
        assert_eq!(value.validate(), Err(DeploymentError::PublicDependency));
    }

    #[test]
    fn checked_in_small_production_profile_is_valid() {
        let input = include_str!("../../../deploy/profiles/small-production.ron");
        let parsed: DeploymentDescriptor =
            ron::from_str(input).unwrap_or_else(|error| panic!("profile RON failed: {error}"));
        assert!(parsed.validate().is_ok());
    }
}
