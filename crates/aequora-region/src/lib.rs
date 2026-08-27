//! Database- and runtime-neutral multi-region read contracts.
//!
//! This crate distributes read latency without distributing write authority. It does not open
//! sockets, inspect `PostgreSQL` WAL, provision replicas, or execute domain commands. Adapters feed
//! it verified apply watermarks and execute the resulting bounded routing decision.

use aequora_types::{AuthorityEpoch, AuthorityId, RegionId, Sequence, TenantId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
    time::Duration,
};
use thiserror::Error;
use uuid::Uuid;

/// Diagnostics-only identity of one regional read replica.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReplicaId(Uuid);

impl ReplicaId {
    /// Creates an approximately time-ordered diagnostics identity.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Wraps an existing UUID.
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    /// Returns the underlying UUID.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for ReplicaId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ReplicaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ReplicaId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

/// Stable identity of one tenant authority shard. Each shard has exactly one writer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AuthorityShardId(pub u32);

/// Stable identity of one regional projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProjectionId(pub u32);

/// Schema generation understood by a regional read API.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProjectionSchemaVersion(pub u32);

/// Serving responsibility of one deployed region.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RegionalRole {
    /// The one active authoritative write executor.
    AuthorityWriter,
    /// A query-only replica backed by verified authoritative apply progress.
    ReadReplica,
    /// Snapshot/blob/cache delivery with no mutable database read capability.
    EdgeOnly,
    /// A fenced promotion candidate governed by Part 16.
    RecoveryStandby,
}

/// Availability mode exposed by regional control-plane observations.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RegionMode {
    Healthy,
    ReadOnlyStale,
    Disconnected,
    PromotionCandidate,
}

/// Coarse health used for deterministic routing order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum RegionHealth {
    Healthy,
    Degraded,
    Unavailable,
}

/// Highest contiguous authoritative journal position durably visible on a replica.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplicaWatermark {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
}

impl ReplicaWatermark {
    /// Whether this watermark proves visibility through the requested sequence and timeline.
    #[must_use]
    pub fn covers(
        self,
        authority_id: AuthorityId,
        authority_epoch: AuthorityEpoch,
        sequence: Sequence,
    ) -> bool {
        self.authority_id == authority_id
            && self.authority_epoch == authority_epoch
            && self.sequence.0 >= sequence.0
    }
}

/// Durable apply position for a journal-derived regional projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectionWatermark {
    pub projection_id: ProjectionId,
    pub schema_version: ProjectionSchemaVersion,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub sequence: Sequence,
}

/// Read guarantee requested by an endpoint or caller.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReadConsistency {
    Eventual,
    AtLeast(Sequence),
    Session,
    Authority,
}

/// Caller-observed lower bound for session-consistent reads.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionWatermark {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub min_sequence: Sequence,
}

impl SessionWatermark {
    /// Creates a session lower bound from trusted server metadata.
    #[must_use]
    pub const fn new(
        authority_id: AuthorityId,
        authority_epoch: AuthorityEpoch,
        min_sequence: Sequence,
    ) -> Self {
        Self {
            authority_id,
            authority_epoch,
            min_sequence,
        }
    }

    /// Advances the session monotonically, resetting sequence comparison on a newer epoch.
    ///
    /// # Errors
    ///
    /// Rejects another authority or an epoch rollback. Part 16 must explicitly resolve an
    /// authority migration before a session changes `AuthorityId`.
    pub fn observe(
        &mut self,
        authority_id: AuthorityId,
        authority_epoch: AuthorityEpoch,
        sequence: Sequence,
    ) -> Result<(), RegionError> {
        if authority_id != self.authority_id {
            return Err(RegionError::AuthorityMismatch);
        }
        if authority_epoch < self.authority_epoch {
            return Err(RegionError::EpochRollback {
                trusted: self.authority_epoch,
                received: authority_epoch,
            });
        }
        if authority_epoch > self.authority_epoch {
            self.authority_epoch = authority_epoch;
            self.min_sequence = sequence;
        } else if sequence > self.min_sequence {
            self.min_sequence = sequence;
        }
        Ok(())
    }
}

/// Additional staleness allowed by an endpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StalenessBudget {
    /// No additional lag constraint. Exact sequence/session requirements still apply.
    None,
    MaxSequenceLag(u64),
    MaxDuration(Duration),
}

/// Explicit policy controlling whether an unsatisfied read may weaken its guarantee.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReadFallbackPolicy {
    NeverDowngrade,
    AllowEventual,
    AllowBoundedStale,
}

/// Endpoint-owned read semantics; callers cannot silently weaken this declaration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointReadPolicy {
    pub consistency: ReadConsistency,
    pub staleness_budget: StalenessBudget,
    pub fallback: ReadFallbackPolicy,
    pub allow_authority_fallback: bool,
    pub authorization_sensitive: bool,
}

impl EndpointReadPolicy {
    /// Safe default for mutable user-facing profile reads.
    #[must_use]
    pub const fn session() -> Self {
        Self {
            consistency: ReadConsistency::Session,
            staleness_budget: StalenessBudget::None,
            fallback: ReadFallbackPolicy::NeverDowngrade,
            allow_authority_fallback: true,
            authorization_sensitive: false,
        }
    }

    /// Strong authority-only policy for security or financial decisions.
    #[must_use]
    pub const fn authority() -> Self {
        Self {
            consistency: ReadConsistency::Authority,
            staleness_budget: StalenessBudget::None,
            fallback: ReadFallbackPolicy::NeverDowngrade,
            allow_authority_fallback: true,
            authorization_sensitive: true,
        }
    }
}

/// Trusted writer discovery record. The endpoint is opaque to the core.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityLocation {
    pub region_id: RegionId,
    pub endpoint: String,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub shard_id: Option<AuthorityShardId>,
    pub latest_sequence: Sequence,
}

impl AuthorityLocation {
    /// Validates bounded, nonempty discovery metadata.
    ///
    /// # Errors
    ///
    /// Rejects an empty or excessively large endpoint reference.
    pub fn validate(&self) -> Result<(), RegionError> {
        let length = self.endpoint.len();
        if length == 0 || length > 2_048 {
            return Err(RegionError::InvalidEndpoint);
        }
        Ok(())
    }
}

/// One observed regional replica. Watermarks must come from durable apply progress, never time.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplicaObservation {
    pub replica_id: ReplicaId,
    pub region_id: RegionId,
    pub role: RegionalRole,
    pub watermark: ReplicaWatermark,
    pub projection: Option<ProjectionWatermark>,
    pub authorization_sequence: Option<Sequence>,
    pub observed_authority_sequence: Sequence,
    pub approximate_time_lag: Duration,
    pub health: RegionHealth,
    pub mode: RegionMode,
    pub priority: u16,
    /// Coarse zero-through-10,000 load value; unsuitable as a high-cardinality metric label.
    pub load_basis_points: u16,
    pub maintenance: bool,
}

impl ReplicaObservation {
    /// Current sequence lag, saturating if control-plane metadata is older than the replica.
    #[must_use]
    pub const fn sequence_lag(&self) -> u64 {
        self.observed_authority_sequence
            .0
            .saturating_sub(self.watermark.sequence.0)
    }

    fn validate(&self) -> Result<(), RegionError> {
        if self.load_basis_points > 10_000 {
            return Err(RegionError::InvalidLoad);
        }
        Ok(())
    }
}

/// Read request passed through both router and selected replica guard.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegionalReadRequest {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub policy: EndpointReadPolicy,
    pub session: Option<SessionWatermark>,
    pub region_hint: Option<RegionId>,
    pub required_projection: Option<(ProjectionId, ProjectionSchemaVersion)>,
    pub minimum_authorization_sequence: Option<Sequence>,
}

impl RegionalReadRequest {
    fn required_sequence(self) -> Result<Option<Sequence>, RegionError> {
        match self.policy.consistency {
            ReadConsistency::Eventual | ReadConsistency::Authority => Ok(None),
            ReadConsistency::AtLeast(sequence) => Ok(Some(sequence)),
            ReadConsistency::Session => {
                let session = self.session.ok_or(RegionError::MissingSessionWatermark)?;
                if session.authority_id != self.authority_id {
                    return Err(RegionError::AuthorityMismatch);
                }
                if session.authority_epoch != self.authority_epoch {
                    return Err(RegionError::WrongEpoch {
                        available: session.authority_epoch,
                        required: self.authority_epoch,
                    });
                }
                Ok(Some(session.min_sequence))
            }
        }
    }
}

/// Selected read execution target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReadTarget {
    RegionalReplica(ReplicaId),
    Authority,
}

/// Optional bounded replica catch-up attempt before authority fallback.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplicaWait {
    pub replica_id: ReplicaId,
    pub required_sequence: Sequence,
    pub timeout: Duration,
}

/// Auditable routing result, including any explicitly authorized consistency downgrade.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReadRouteDecision {
    pub target: ReadTarget,
    pub effective_consistency: ReadConsistency,
    pub downgraded: bool,
    pub wait_before_fallback: Option<ReplicaWait>,
}

/// Metadata a regional response or cache entry must return to session-aware clients.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReadResponseMetadata {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub served_sequence: Sequence,
    pub region: RegionId,
    pub projection_version: Option<ProjectionSchemaVersion>,
}

/// Epoch-aware cache generation. TTL alone is insufficient for consistency-sensitive reads.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegionalCacheMetadata {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub served_sequence: Sequence,
    pub projection_version: Option<ProjectionSchemaVersion>,
}

impl RegionalCacheMetadata {
    /// Whether this cache entry can satisfy a guarded regional read.
    ///
    /// # Errors
    ///
    /// Returns session-validation errors for a missing, mismatched, or rolled-back watermark.
    pub fn satisfies(self, request: RegionalReadRequest) -> Result<bool, RegionError> {
        if self.authority_id != request.authority_id
            || self.authority_epoch != request.authority_epoch
        {
            return Ok(false);
        }
        if let Some((_, version)) = request.required_projection {
            if self.projection_version != Some(version) {
                return Ok(false);
            }
        }
        Ok(request
            .required_sequence()?
            .is_none_or(|minimum| self.served_sequence >= minimum))
    }
}

/// Defense-in-depth validation run by a replica even after edge routing.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReplicaReadGuard;

impl ReplicaReadGuard {
    /// Validates exact epoch, sequence, projection, health, and authorization freshness.
    ///
    /// # Errors
    ///
    /// Rejects any replica that cannot prove the endpoint's declared guarantee.
    pub fn validate(
        observation: &ReplicaObservation,
        request: RegionalReadRequest,
    ) -> Result<ReadResponseMetadata, RegionError> {
        validate_replica_base(observation, request)?;
        if matches!(
            observation.mode,
            RegionMode::ReadOnlyStale | RegionMode::Disconnected
        ) && request.policy.consistency != ReadConsistency::Eventual
        {
            return Err(RegionError::ReplicaUnavailable);
        }
        let required = request.required_sequence()?;
        if let Some(required) = required {
            if observation.watermark.sequence < required {
                return Err(RegionError::ReplicaTooStale {
                    available: observation.watermark.sequence,
                    required,
                });
            }
        }
        if request.policy.authorization_sensitive
            || request.minimum_authorization_sequence.is_some()
        {
            let required_security = request
                .minimum_authorization_sequence
                .or(required)
                .unwrap_or(observation.observed_authority_sequence);
            if observation.authorization_sequence < Some(required_security) {
                return Err(RegionError::AuthorizationTooStale {
                    available: observation.authorization_sequence,
                    required: required_security,
                });
            }
        }
        if !within_budget(observation, request.policy.staleness_budget) {
            return Err(RegionError::StalenessBudgetExceeded);
        }
        Ok(ReadResponseMetadata {
            authority_id: observation.watermark.authority_id,
            authority_epoch: observation.watermark.authority_epoch,
            served_sequence: observation.watermark.sequence,
            region: observation.region_id,
            projection_version: observation.projection.map(|value| value.schema_version),
        })
    }
}

fn validate_replica_base(
    observation: &ReplicaObservation,
    request: RegionalReadRequest,
) -> Result<(), RegionError> {
    observation.validate()?;
    if observation.role != RegionalRole::ReadReplica {
        return Err(RegionError::NotReadReplica);
    }
    if observation.maintenance || observation.health == RegionHealth::Unavailable {
        return Err(RegionError::ReplicaUnavailable);
    }
    if matches!(observation.mode, RegionMode::PromotionCandidate) {
        return Err(RegionError::ReplicaUnavailable);
    }
    if observation.watermark.authority_id != request.authority_id {
        return Err(RegionError::AuthorityMismatch);
    }
    if observation.watermark.authority_epoch != request.authority_epoch {
        return Err(RegionError::WrongEpoch {
            available: observation.watermark.authority_epoch,
            required: request.authority_epoch,
        });
    }
    if let Some((projection_id, schema_version)) = request.required_projection {
        let projection = observation
            .projection
            .ok_or(RegionError::ProjectionUnavailable)?;
        if projection.projection_id != projection_id
            || projection.schema_version != schema_version
            || projection.authority_id != request.authority_id
            || projection.authority_epoch != request.authority_epoch
            || projection.sequence > observation.watermark.sequence
        {
            return Err(RegionError::ProjectionIncompatible);
        }
    }
    Ok(())
}

fn within_budget(observation: &ReplicaObservation, budget: StalenessBudget) -> bool {
    match budget {
        StalenessBudget::None => true,
        StalenessBudget::MaxSequenceLag(maximum) => observation.sequence_lag() <= maximum,
        StalenessBudget::MaxDuration(maximum) => observation.approximate_time_lag <= maximum,
    }
}

/// Validated regional router configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegionalRouterConfig {
    pub local_region: RegionId,
    pub replica_wait: Duration,
    pub allow_authority_fallback: bool,
}

impl RegionalRouterConfig {
    /// Validates a bounded wait so a stale replica can never block a read indefinitely.
    ///
    /// # Errors
    ///
    /// Rejects waits above thirty seconds.
    pub fn validate(self) -> Result<(), RegionError> {
        if self.replica_wait > Duration::from_secs(30) {
            return Err(RegionError::UnboundedReplicaWait);
        }
        Ok(())
    }
}

/// Deterministic router fed by deployment health and durable watermark observations.
#[derive(Clone, Debug)]
pub struct RegionalRouter {
    authority: AuthorityLocation,
    authority_available: bool,
    config: RegionalRouterConfig,
    replicas: BTreeMap<ReplicaId, ReplicaObservation>,
}

impl RegionalRouter {
    /// Creates a router for one authority domain.
    ///
    /// # Errors
    ///
    /// Rejects invalid discovery or unbounded wait configuration.
    pub fn new(
        authority: AuthorityLocation,
        authority_available: bool,
        config: RegionalRouterConfig,
    ) -> Result<Self, RegionError> {
        authority.validate()?;
        config.validate()?;
        Ok(Self {
            authority,
            authority_available,
            config,
            replicas: BTreeMap::new(),
        })
    }

    /// Adds or replaces one replica observation after structural validation.
    ///
    /// # Errors
    ///
    /// Rejects invalid load observations.
    pub fn observe(&mut self, observation: ReplicaObservation) -> Result<(), RegionError> {
        observation.validate()?;
        self.replicas.insert(observation.replica_id, observation);
        Ok(())
    }

    /// Removes a retired or wrong-epoch replica from routing.
    pub fn remove(&mut self, replica_id: ReplicaId) -> Option<ReplicaObservation> {
        self.replicas.remove(&replica_id)
    }

    /// Updates writer discovery after a trusted Part 16 transition.
    ///
    /// # Errors
    ///
    /// Rejects an authority change or epoch rollback; cross-authority migration requires a new
    /// router and tenant-directory assignment.
    pub fn update_authority(
        &mut self,
        authority: AuthorityLocation,
        available: bool,
    ) -> Result<(), RegionError> {
        authority.validate()?;
        if authority.authority_id != self.authority.authority_id {
            return Err(RegionError::AuthorityMismatch);
        }
        if authority.authority_epoch < self.authority.authority_epoch {
            return Err(RegionError::EpochRollback {
                trusted: self.authority.authority_epoch,
                received: authority.authority_epoch,
            });
        }
        self.authority = authority;
        self.authority_available = available;
        self.replicas.retain(|_, replica| {
            replica.watermark.authority_id == self.authority.authority_id
                && replica.watermark.authority_epoch == self.authority.authority_epoch
        });
        Ok(())
    }

    /// Selects the nearest safe current target, falling back without implicit downgrade.
    ///
    /// # Errors
    ///
    /// Returns a typed consistency or availability failure when policy has no safe target.
    pub fn route(&self, request: RegionalReadRequest) -> Result<ReadRouteDecision, RegionError> {
        if request.authority_id != self.authority.authority_id {
            return Err(RegionError::AuthorityMismatch);
        }
        if request.authority_epoch != self.authority.authority_epoch {
            return Err(RegionError::WrongEpoch {
                available: self.authority.authority_epoch,
                required: request.authority_epoch,
            });
        }
        let required = request.required_sequence()?;
        if request.policy.consistency == ReadConsistency::Authority {
            return self.authority_decision(ReadConsistency::Authority, false, None);
        }

        let mut exact = self
            .replicas
            .values()
            .filter(|replica| ReplicaReadGuard::validate(replica, request).is_ok())
            .collect::<Vec<_>>();
        sort_replicas(&mut exact, request.region_hint, self.config.local_region);
        if let Some(replica) = exact.first() {
            return Ok(ReadRouteDecision {
                target: ReadTarget::RegionalReplica(replica.replica_id),
                effective_consistency: request.policy.consistency,
                downgraded: false,
                wait_before_fallback: None,
            });
        }

        let wait_before_fallback = required.and_then(|required_sequence| {
            self.stale_wait_candidate(request)
                .map(|replica_id| ReplicaWait {
                    replica_id,
                    required_sequence,
                    timeout: self.config.replica_wait,
                })
        });
        if request.policy.allow_authority_fallback
            && self.config.allow_authority_fallback
            && self.authority_available
        {
            return self.authority_decision(
                request.policy.consistency,
                false,
                wait_before_fallback,
            );
        }

        if request.policy.fallback != ReadFallbackPolicy::NeverDowngrade {
            let mut downgraded = self
                .replicas
                .values()
                .filter(|replica| validate_replica_base(replica, request).is_ok())
                .filter(|replica| {
                    request.policy.fallback == ReadFallbackPolicy::AllowEventual
                        || (request.policy.fallback == ReadFallbackPolicy::AllowBoundedStale
                            && request.policy.staleness_budget != StalenessBudget::None
                            && within_budget(replica, request.policy.staleness_budget))
                })
                .collect::<Vec<_>>();
            sort_replicas(
                &mut downgraded,
                request.region_hint,
                self.config.local_region,
            );
            if let Some(replica) = downgraded.first() {
                return Ok(ReadRouteDecision {
                    target: ReadTarget::RegionalReplica(replica.replica_id),
                    effective_consistency: ReadConsistency::Eventual,
                    downgraded: true,
                    wait_before_fallback: None,
                });
            }
        }

        if let Some(required) = required {
            let available = self
                .replicas
                .values()
                .filter(|replica| {
                    replica.watermark.authority_id == request.authority_id
                        && replica.watermark.authority_epoch == request.authority_epoch
                })
                .map(|replica| replica.watermark.sequence)
                .max()
                .unwrap_or(Sequence(0));
            return Err(RegionError::ReplicaTooStale {
                available,
                required,
            });
        }
        Err(if self.authority_available {
            RegionError::NoEligibleReplica
        } else {
            RegionError::AuthorityUnavailable
        })
    }

    fn authority_decision(
        &self,
        effective_consistency: ReadConsistency,
        downgraded: bool,
        wait_before_fallback: Option<ReplicaWait>,
    ) -> Result<ReadRouteDecision, RegionError> {
        if !self.authority_available {
            return Err(RegionError::AuthorityUnavailable);
        }
        Ok(ReadRouteDecision {
            target: ReadTarget::Authority,
            effective_consistency,
            downgraded,
            wait_before_fallback,
        })
    }

    fn stale_wait_candidate(&self, request: RegionalReadRequest) -> Option<ReplicaId> {
        let mut candidates = self
            .replicas
            .values()
            .filter(|replica| validate_replica_base(replica, request).is_ok())
            .collect::<Vec<_>>();
        sort_replicas(
            &mut candidates,
            request.region_hint,
            self.config.local_region,
        );
        candidates.first().map(|replica| replica.replica_id)
    }
}

fn sort_replicas(
    replicas: &mut Vec<&ReplicaObservation>,
    region_hint: Option<RegionId>,
    local_region: RegionId,
) {
    replicas.sort_by_key(|replica| {
        (
            replica.health,
            u8::from(region_hint.or(Some(local_region)) != Some(replica.region_id)),
            replica.priority,
            replica.load_basis_points,
            replica.sequence_lag(),
            replica.replica_id,
        )
    });
}

/// Region-constrained placement kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum RegionalPlacementKind {
    Authority,
    Replica,
    Projection,
    Snapshot,
    Blob,
    Cache,
    Backup,
    EncryptionKey,
}

/// Tenant residency constraint covering every regional durable or cached copy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResidencyPolicy {
    pub allowed_regions: BTreeSet<RegionId>,
    pub authority_region: RegionId,
    pub replica_regions: BTreeSet<RegionId>,
    pub key_regions: BTreeSet<RegionId>,
}

impl ResidencyPolicy {
    /// Validates that authority, replicas, and key access are subsets of allowed regions.
    ///
    /// # Errors
    ///
    /// Rejects an empty allow-list or any declared placement outside it.
    pub fn validate(&self) -> Result<(), RegionError> {
        if self.allowed_regions.is_empty()
            || !self.allowed_regions.contains(&self.authority_region)
            || !self.replica_regions.is_subset(&self.allowed_regions)
            || !self.key_regions.is_subset(&self.allowed_regions)
        {
            return Err(RegionError::ResidencyViolation);
        }
        Ok(())
    }

    /// Checks one concrete regional placement.
    ///
    /// # Errors
    ///
    /// Rejects a location outside policy or a writer/replica/key role not explicitly declared.
    pub fn verify_placement(
        &self,
        kind: RegionalPlacementKind,
        region: RegionId,
    ) -> Result<(), RegionError> {
        self.validate()?;
        let allowed = self.allowed_regions.contains(&region)
            && match kind {
                RegionalPlacementKind::Authority => region == self.authority_region,
                RegionalPlacementKind::Replica | RegionalPlacementKind::Projection => {
                    self.replica_regions.contains(&region)
                }
                RegionalPlacementKind::EncryptionKey => self.key_regions.contains(&region),
                RegionalPlacementKind::Snapshot
                | RegionalPlacementKind::Blob
                | RegionalPlacementKind::Cache
                | RegionalPlacementKind::Backup => true,
            };
        if allowed {
            Ok(())
        } else {
            Err(RegionError::ResidencyViolation)
        }
    }
}

/// Monotonic control-plane generation protecting cached tenant assignments from rollback.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DirectoryGeneration(pub u64);

/// Trusted control-plane mapping for one tenant's single authoritative writer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TenantAuthorityAssignment {
    pub tenant_id: TenantId,
    pub shard_id: AuthorityShardId,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub authority_region: RegionId,
    pub directory_generation: DirectoryGeneration,
    pub residency: ResidencyPolicy,
}

/// Result of deployment-owned signature/trust verification for a directory record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentTrust {
    Verified,
    Unverified,
}

/// Rollback-protected in-memory view of trusted tenant authority assignments.
#[derive(Clone, Debug, Default)]
pub struct TenantAuthorityDirectory {
    assignments: BTreeMap<TenantId, TenantAuthorityAssignment>,
}

impl TenantAuthorityDirectory {
    /// Installs a signature/trust-verified assignment without generation or epoch rollback.
    ///
    /// # Errors
    ///
    /// Rejects unverified, residency-invalid, or rollback records.
    pub fn install(
        &mut self,
        assignment: TenantAuthorityAssignment,
        trust: AssignmentTrust,
    ) -> Result<(), RegionError> {
        if trust != AssignmentTrust::Verified {
            return Err(RegionError::UntrustedDirectoryAssignment);
        }
        assignment.residency.validate()?;
        assignment.residency.verify_placement(
            RegionalPlacementKind::Authority,
            assignment.authority_region,
        )?;
        if let Some(current) = self.assignments.get(&assignment.tenant_id) {
            if assignment.directory_generation <= current.directory_generation {
                return Err(RegionError::DirectoryRollback);
            }
            if assignment.authority_id == current.authority_id
                && assignment.authority_epoch < current.authority_epoch
            {
                return Err(RegionError::EpochRollback {
                    trusted: current.authority_epoch,
                    received: assignment.authority_epoch,
                });
            }
        }
        self.assignments.insert(assignment.tenant_id, assignment);
        Ok(())
    }

    /// Resolves the trusted assignment for login, bootstrap, read, or write routing.
    #[must_use]
    pub fn resolve(&self, tenant_id: TenantId) -> Option<&TenantAuthorityAssignment> {
        self.assignments.get(&tenant_id)
    }

    /// Routes a tenant write only to its current trusted authority writer.
    ///
    /// # Errors
    ///
    /// Rejects missing/stale discovery, residency mismatch, or a non-authority endpoint.
    pub fn route_write(
        &self,
        tenant_id: TenantId,
        location: &AuthorityLocation,
        role: RegionalRole,
    ) -> Result<RegionId, RegionError> {
        let assignment = self
            .resolve(tenant_id)
            .ok_or(RegionError::TenantAssignmentMissing)?;
        if role != RegionalRole::AuthorityWriter {
            return Err(RegionError::NotAuthorityWriter);
        }
        if location.authority_id != assignment.authority_id
            || location.authority_epoch != assignment.authority_epoch
            || location.region_id != assignment.authority_region
        {
            return Err(RegionError::StaleControlPlane);
        }
        assignment
            .residency
            .verify_placement(RegionalPlacementKind::Authority, location.region_id)?;
        Ok(location.region_id)
    }
}

/// Apply decision for an idempotent regional projection consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionApplyDecision {
    Apply,
    Duplicate,
}

/// Validates idempotent, contiguous projection apply and crash-recovery cursors.
///
/// # Errors
///
/// Rejects an epoch mismatch or a non-contiguous sequence gap.
pub fn validate_projection_apply(
    current: ProjectionWatermark,
    incoming_epoch: AuthorityEpoch,
    incoming_sequence: Sequence,
) -> Result<ProjectionApplyDecision, RegionError> {
    if incoming_epoch != current.authority_epoch {
        return Err(RegionError::WrongEpoch {
            available: current.authority_epoch,
            required: incoming_epoch,
        });
    }
    if incoming_sequence <= current.sequence {
        return Ok(ProjectionApplyDecision::Duplicate);
    }
    if incoming_sequence.0 != current.sequence.0.saturating_add(1) {
        return Err(RegionError::ProjectionGap {
            current: current.sequence,
            incoming: incoming_sequence,
        });
    }
    Ok(ProjectionApplyDecision::Apply)
}

/// Regional content type whose transport location may differ from its authority origin.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RegionalArtifactKind {
    SnapshotManifest,
    SnapshotChunk,
    Blob,
}

/// Verification evidence required before activating bytes delivered by an untrusted edge.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegionalArtifactEvidence {
    pub kind: RegionalArtifactKind,
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub origin_region: RegionId,
    pub delivery_region: RegionId,
    pub authority_signature_verified: bool,
    pub content_digest_verified: bool,
}

impl RegionalArtifactEvidence {
    /// Validates authority/epoch integrity and residency before regional activation.
    ///
    /// # Errors
    ///
    /// Rejects stale timelines, missing integrity evidence, or forbidden delivery placement.
    pub fn verify(
        self,
        authority_id: AuthorityId,
        authority_epoch: AuthorityEpoch,
        residency: &ResidencyPolicy,
    ) -> Result<(), RegionError> {
        if self.authority_id != authority_id {
            return Err(RegionError::AuthorityMismatch);
        }
        if self.authority_epoch != authority_epoch {
            return Err(RegionError::WrongEpoch {
                available: self.authority_epoch,
                required: authority_epoch,
            });
        }
        if !self.authority_signature_verified || !self.content_digest_verified {
            return Err(RegionError::ArtifactIntegrityUnverified);
        }
        let placement = match self.kind {
            RegionalArtifactKind::SnapshotManifest | RegionalArtifactKind::SnapshotChunk => {
                RegionalPlacementKind::Snapshot
            }
            RegionalArtifactKind::Blob => RegionalPlacementKind::Blob,
        };
        residency.verify_placement(placement, self.origin_region)?;
        residency.verify_placement(placement, self.delivery_region)
    }
}

/// Diagnostics identity of one governed regional copy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RegionalCopyId(Uuid);

impl RegionalCopyId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for RegionalCopyId {
    fn default() -> Self {
        Self::new()
    }
}

/// Regional copy category participating in governance coverage.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RegionalCopyKind {
    Replica,
    Projection,
    Snapshot,
    Blob,
    Cache,
    SearchIndex,
    Backup,
}

/// Erasure progress for one registered regional copy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RegionalPurgeState {
    Registered,
    Purged,
    Verified,
    PolicyExempt,
}

/// One deployment-registered copy requiring regional erasure accounting.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegionalCopyRecord {
    pub copy_id: RegionalCopyId,
    pub region_id: RegionId,
    pub kind: RegionalCopyKind,
    pub required: bool,
    pub state: RegionalPurgeState,
}

/// Coverage result used by the Part 14 erasure completion gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionalGovernanceReport {
    pub registered: usize,
    pub required: usize,
    pub verified: usize,
    pub complete: bool,
}

/// Registry of all known regional copies for one governed erasure scope.
#[derive(Clone, Debug, Default)]
pub struct RegionalGovernanceRegistry {
    copies: BTreeMap<RegionalCopyId, RegionalCopyRecord>,
}

impl RegionalGovernanceRegistry {
    /// Registers or replaces one deployment-owned regional storage surface.
    pub fn register(&mut self, record: RegionalCopyRecord) {
        self.copies.insert(record.copy_id, record);
    }

    /// Advances one copy from registered to purged to independently verified.
    ///
    /// # Errors
    ///
    /// Rejects missing copies, skipped verification, and state rollback.
    pub fn transition(
        &mut self,
        copy_id: RegionalCopyId,
        next: RegionalPurgeState,
    ) -> Result<(), RegionError> {
        let record = self
            .copies
            .get_mut(&copy_id)
            .ok_or(RegionError::RegionalCopyMissing)?;
        let allowed = matches!(
            (record.state, next),
            (
                RegionalPurgeState::Registered,
                RegionalPurgeState::Purged | RegionalPurgeState::PolicyExempt
            ) | (RegionalPurgeState::Purged, RegionalPurgeState::Verified)
        ) || record.state == next;
        if !allowed {
            return Err(RegionError::InvalidPurgeTransition);
        }
        record.state = next;
        Ok(())
    }

    /// Reports whether every required regional copy is verified or explicitly policy-exempt.
    #[must_use]
    pub fn report(&self) -> RegionalGovernanceReport {
        let required = self
            .copies
            .values()
            .filter(|record| record.required)
            .count();
        let verified = self
            .copies
            .values()
            .filter(|record| {
                record.required
                    && matches!(
                        record.state,
                        RegionalPurgeState::Verified | RegionalPurgeState::PolicyExempt
                    )
            })
            .count();
        RegionalGovernanceReport {
            registered: self.copies.len(),
            required,
            verified,
            complete: required == verified,
        }
    }
}

/// Deployment progression that does not alter protocol or authority semantics.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RegionalDeploymentProfile {
    SingleRegion,
    DualRegion,
    GlobalRead,
    TenantShardedGlobal,
}

/// Payload-free event categories suitable for low-cardinality metrics and structured logs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionalEventKind {
    ReadRoutedReplica,
    ReadFallbackAuthority,
    ReadDowngraded,
    ReplicaTooStale,
    WrongEpochReplica,
    RegionDegraded,
    AuthorityRegionChanged,
    ResidencyViolation,
    ProjectionStalled,
    GovernanceIncomplete,
}

/// Typed regional correctness and availability failures.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RegionError {
    #[error("authority identity does not match the trusted routing domain")]
    AuthorityMismatch,
    #[error("received authority epoch {received:?} is below trusted epoch {trusted:?}")]
    EpochRollback {
        trusted: AuthorityEpoch,
        received: AuthorityEpoch,
    },
    #[error("replica epoch {available:?} does not match required epoch {required:?}")]
    WrongEpoch {
        available: AuthorityEpoch,
        required: AuthorityEpoch,
    },
    #[error("session consistency requires a session watermark")]
    MissingSessionWatermark,
    #[error("replica is visible through {available:?}, below required {required:?}")]
    ReplicaTooStale {
        available: Sequence,
        required: Sequence,
    },
    #[error("authorization watermark {available:?} is below required {required:?}")]
    AuthorizationTooStale {
        available: Option<Sequence>,
        required: Sequence,
    },
    #[error("replica staleness exceeds the endpoint budget")]
    StalenessBudgetExceeded,
    #[error("regional node is not a read replica")]
    NotReadReplica,
    #[error("regional node is not the active authority writer")]
    NotAuthorityWriter,
    #[error("replica is unavailable, in maintenance, or a promotion candidate")]
    ReplicaUnavailable,
    #[error("no regional replica satisfies the request")]
    NoEligibleReplica,
    #[error("authority writer is unavailable")]
    AuthorityUnavailable,
    #[error("projection is unavailable")]
    ProjectionUnavailable,
    #[error("projection timeline or schema is incompatible")]
    ProjectionIncompatible,
    #[error("projection apply has a gap from {current:?} to {incoming:?}")]
    ProjectionGap {
        current: Sequence,
        incoming: Sequence,
    },
    #[error("replica load must be between zero and 10000 basis points")]
    InvalidLoad,
    #[error("replica wait exceeds the bounded maximum")]
    UnboundedReplicaWait,
    #[error("writer endpoint metadata is empty or too large")]
    InvalidEndpoint,
    #[error("regional placement violates tenant residency policy")]
    ResidencyViolation,
    #[error("tenant authority assignment has not been trust verified")]
    UntrustedDirectoryAssignment,
    #[error("tenant directory generation did not advance")]
    DirectoryRollback,
    #[error("tenant has no trusted authority assignment")]
    TenantAssignmentMissing,
    #[error("routing control plane is stale relative to the trusted tenant assignment")]
    StaleControlPlane,
    #[error("regional artifact authority or integrity evidence is incomplete")]
    ArtifactIntegrityUnverified,
    #[error("regional governed copy is not registered")]
    RegionalCopyMissing,
    #[error("regional purge transition skipped verification or rolled back")]
    InvalidPurgeTransition,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority(epoch: AuthorityEpoch, latest: u64) -> AuthorityLocation {
        AuthorityLocation {
            region_id: RegionId::new(1),
            endpoint: "https://writer.internal".to_owned(),
            authority_id: AuthorityId::LOCAL_DEVELOPMENT,
            authority_epoch: epoch,
            shard_id: Some(AuthorityShardId(7)),
            latest_sequence: Sequence(latest),
        }
    }

    fn replica(region: u16, epoch: AuthorityEpoch, sequence: u64) -> ReplicaObservation {
        ReplicaObservation {
            replica_id: ReplicaId::new(),
            region_id: RegionId::new(region),
            role: RegionalRole::ReadReplica,
            watermark: ReplicaWatermark {
                authority_id: AuthorityId::LOCAL_DEVELOPMENT,
                authority_epoch: epoch,
                sequence: Sequence(sequence),
            },
            projection: None,
            authorization_sequence: Some(Sequence(sequence)),
            observed_authority_sequence: Sequence(10),
            approximate_time_lag: Duration::from_millis(50),
            health: RegionHealth::Healthy,
            mode: RegionMode::Healthy,
            priority: 1,
            load_basis_points: 100,
            maintenance: false,
        }
    }

    fn request(
        consistency: ReadConsistency,
        session: Option<SessionWatermark>,
    ) -> RegionalReadRequest {
        RegionalReadRequest {
            authority_id: AuthorityId::LOCAL_DEVELOPMENT,
            authority_epoch: AuthorityEpoch::INITIAL,
            policy: EndpointReadPolicy {
                consistency,
                staleness_budget: StalenessBudget::None,
                fallback: ReadFallbackPolicy::NeverDowngrade,
                allow_authority_fallback: true,
                authorization_sensitive: false,
            },
            session,
            region_hint: Some(RegionId::new(2)),
            required_projection: None,
            minimum_authorization_sequence: None,
        }
    }

    fn router(authority_available: bool) -> RegionalRouter {
        RegionalRouter::new(
            authority(AuthorityEpoch::INITIAL, 10),
            authority_available,
            RegionalRouterConfig {
                local_region: RegionId::new(2),
                replica_wait: Duration::from_millis(200),
                allow_authority_fallback: true,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn read_your_writes_falls_back_instead_of_returning_old_state() {
        let mut router = router(true);
        let stale = replica(2, AuthorityEpoch::INITIAL, 8);
        let stale_id = stale.replica_id;
        router
            .observe(stale)
            .unwrap_or_else(|error| panic!("{error}"));
        let session = SessionWatermark::new(
            AuthorityId::LOCAL_DEVELOPMENT,
            AuthorityEpoch::INITIAL,
            Sequence(10),
        );

        let decision = router
            .route(request(ReadConsistency::Session, Some(session)))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(decision.target, ReadTarget::Authority);
        assert_eq!(
            decision.wait_before_fallback.map(|wait| wait.replica_id),
            Some(stale_id)
        );
        assert!(!decision.downgraded);
    }

    #[test]
    fn caught_up_replica_serves_at_least_read() {
        let mut router = router(true);
        let caught_up = replica(2, AuthorityEpoch::INITIAL, 10);
        let replica_id = caught_up.replica_id;
        router
            .observe(caught_up)
            .unwrap_or_else(|error| panic!("{error}"));
        let decision = router
            .route(request(ReadConsistency::AtLeast(Sequence(10)), None))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(decision.target, ReadTarget::RegionalReplica(replica_id));
    }

    #[test]
    fn wrong_epoch_replica_is_never_routed_as_current() {
        let next = AuthorityEpoch::INITIAL
            .checked_next()
            .unwrap_or(AuthorityEpoch::INITIAL);
        let mut router = router(false);
        router
            .observe(replica(2, next, 10))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            router.route(request(ReadConsistency::AtLeast(Sequence(10)), None)),
            Err(RegionError::ReplicaTooStale { .. })
        ));
    }

    #[test]
    fn explicit_downgrade_is_visible_and_policy_bound() {
        let mut router = router(false);
        let stale = replica(2, AuthorityEpoch::INITIAL, 8);
        let id = stale.replica_id;
        router
            .observe(stale)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut read = request(ReadConsistency::AtLeast(Sequence(10)), None);
        read.policy.fallback = ReadFallbackPolicy::AllowEventual;
        let decision = router.route(read).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(decision.target, ReadTarget::RegionalReplica(id));
        assert!(decision.downgraded);
        assert_eq!(decision.effective_consistency, ReadConsistency::Eventual);
    }

    #[test]
    fn epoch_transition_evicts_old_replica_and_cache() {
        let next = AuthorityEpoch::INITIAL
            .checked_next()
            .unwrap_or(AuthorityEpoch::INITIAL);
        let mut router = router(true);
        let old = replica(2, AuthorityEpoch::INITIAL, 10);
        router
            .observe(old)
            .unwrap_or_else(|error| panic!("{error}"));
        router
            .update_authority(authority(next, 1), true)
            .unwrap_or_else(|error| panic!("{error}"));

        let mut read = request(ReadConsistency::Eventual, None);
        read.authority_epoch = next;
        assert_eq!(
            router
                .route(read)
                .unwrap_or_else(|error| panic!("{error}"))
                .target,
            ReadTarget::Authority
        );
        let cache = RegionalCacheMetadata {
            authority_id: AuthorityId::LOCAL_DEVELOPMENT,
            authority_epoch: AuthorityEpoch::INITIAL,
            served_sequence: Sequence(10),
            projection_version: None,
        };
        assert!(!cache.satisfies(read).unwrap_or(false));
    }

    #[test]
    fn writes_route_only_to_trusted_resident_authority() {
        let tenant = TenantId::new();
        let allowed = BTreeSet::from([RegionId::new(1), RegionId::new(2)]);
        let residency = ResidencyPolicy {
            allowed_regions: allowed,
            authority_region: RegionId::new(1),
            replica_regions: BTreeSet::from([RegionId::new(2)]),
            key_regions: BTreeSet::from([RegionId::new(1)]),
        };
        let mut directory = TenantAuthorityDirectory::default();
        directory
            .install(
                TenantAuthorityAssignment {
                    tenant_id: tenant,
                    shard_id: AuthorityShardId(7),
                    authority_id: AuthorityId::LOCAL_DEVELOPMENT,
                    authority_epoch: AuthorityEpoch::INITIAL,
                    authority_region: RegionId::new(1),
                    directory_generation: DirectoryGeneration(1),
                    residency,
                },
                AssignmentTrust::Verified,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            directory
                .route_write(
                    tenant,
                    &authority(AuthorityEpoch::INITIAL, 10),
                    RegionalRole::AuthorityWriter
                )
                .unwrap_or_else(|error| panic!("{error}")),
            RegionId::new(1)
        );
        assert_eq!(
            directory.route_write(
                tenant,
                &authority(AuthorityEpoch::INITIAL, 10),
                RegionalRole::ReadReplica,
            ),
            Err(RegionError::NotAuthorityWriter)
        );
    }

    #[test]
    fn governance_requires_every_registered_required_copy() {
        let copy = RegionalCopyId::new();
        let mut registry = RegionalGovernanceRegistry::default();
        registry.register(RegionalCopyRecord {
            copy_id: copy,
            region_id: RegionId::new(2),
            kind: RegionalCopyKind::Projection,
            required: true,
            state: RegionalPurgeState::Registered,
        });
        assert!(!registry.report().complete);
        registry
            .transition(copy, RegionalPurgeState::Purged)
            .and_then(|()| registry.transition(copy, RegionalPurgeState::Verified))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(registry.report().complete);
    }

    #[test]
    fn edge_artifact_requires_integrity_epoch_and_residency() {
        let residency = ResidencyPolicy {
            allowed_regions: BTreeSet::from([RegionId::new(1), RegionId::new(2)]),
            authority_region: RegionId::new(1),
            replica_regions: BTreeSet::from([RegionId::new(2)]),
            key_regions: BTreeSet::from([RegionId::new(1)]),
        };
        let evidence = RegionalArtifactEvidence {
            kind: RegionalArtifactKind::SnapshotChunk,
            authority_id: AuthorityId::LOCAL_DEVELOPMENT,
            authority_epoch: AuthorityEpoch::INITIAL,
            origin_region: RegionId::new(1),
            delivery_region: RegionId::new(2),
            authority_signature_verified: true,
            content_digest_verified: true,
        };
        evidence
            .verify(
                AuthorityId::LOCAL_DEVELOPMENT,
                AuthorityEpoch::INITIAL,
                &residency,
            )
            .unwrap_or_else(|error| panic!("{error}"));
    }
}
