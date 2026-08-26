//! Database-neutral canonical integrity verification and replica repair planning.
//!
//! Digests cover synchronized semantic state, never database pages, row encodings, local caches,
//! pending overlays, or the normal synchronization cursor. The server remains authoritative.

use aequora_types::{
    Cursor, EntityRef, EntityVersion, HybridTimestamp, IntegritySessionId, OperationId, RepairId,
    SyncScopeId,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};
use thiserror::Error;

const ENTITY_DOMAIN: &[u8] = b"AEQUORA:ENTITY:v1";
const PARTITION_DOMAIN: &[u8] = b"AEQUORA:PARTITION:v1";
const NODE_DOMAIN: &[u8] = b"AEQUORA:MERKLE-NODE:v1";
const BUCKET_DOMAIN: &[u8] = b"AEQUORA:BUCKET:v1";

/// Current canonical hash and partition format.
pub const CURRENT_INTEGRITY_GENERATION: IntegrityGeneration = IntegrityGeneration(1);
/// Current explicit canonical entity writer format.
pub const CURRENT_HASH_SCHEMA: HashSchemaVersion = HashSchemaVersion(1);

/// Anti-entropy capability advertised by a database adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IntegritySupport {
    /// Consistent canonical snapshots and atomic replica repair are supported.
    Full,
    /// Consistent canonical snapshot verification is supported; repair uses bootstrap fallback.
    SnapshotOnly,
    /// The adapter cannot provide canonical anti-entropy evidence.
    None,
}

/// BLAKE3 digest of canonical synchronized state.
#[derive(Clone, Copy, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct IntegrityDigest([u8; 32]);

impl IntegrityDigest {
    /// Builds a digest from its exact wire bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the exact 32-byte digest.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Debug for IntegrityDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "IntegrityDigest({})", hex::encode(self.0))
    }
}

impl fmt::Display for IntegrityDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

/// Version of the complete hash algorithm, canonical writer, and partition rules.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct IntegrityGeneration(pub u64);

/// Version of the explicit canonical entity writer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct HashSchemaVersion(pub u16);

/// Deterministic hash bucket inside one tenant and synchronization scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct IntegrityPartitionId(pub u32);

/// Validated deterministic partition configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PartitionScheme {
    bucket_count: u32,
}

impl PartitionScheme {
    /// Maximum initial partition count, bounding CPU, memory, and protocol amplification.
    pub const MAX_BUCKETS: u32 = 4_096;

    /// Creates a non-empty bounded deterministic partition scheme.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrityError::InvalidPartitionCount`] for zero or excessive counts.
    pub const fn new(bucket_count: u32) -> Result<Self, IntegrityError> {
        if bucket_count == 0 || bucket_count > Self::MAX_BUCKETS {
            Err(IntegrityError::InvalidPartitionCount(bucket_count))
        } else {
            Ok(Self { bucket_count })
        }
    }

    /// Number of deterministic buckets.
    #[must_use]
    pub const fn bucket_count(self) -> u32 {
        self.bucket_count
    }

    /// Assigns an entity independently of database row ordering.
    #[must_use]
    pub fn partition(self, entity: EntityRef) -> IntegrityPartitionId {
        let mut hasher = blake3::Hasher::new();
        hasher.update(BUCKET_DOMAIN);
        hasher.update(&entity.entity_type.get().to_be_bytes());
        hasher.update(entity.entity_id.as_uuid().as_bytes());
        let bytes = hasher.finalize();
        let prefix = u32::from_be_bytes([
            bytes.as_bytes()[0],
            bytes.as_bytes()[1],
            bytes.as_bytes()[2],
            bytes.as_bytes()[3],
        ]);
        IntegrityPartitionId(prefix % self.bucket_count)
    }
}

/// Canonical authoritative entity input, independent of a database's physical encoding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CanonicalEntity {
    /// Stable entity identity.
    pub entity: EntityRef,
    /// Monotonic authoritative version.
    pub version: EntityVersion,
    /// Canonical entity writer/schema version.
    pub hash_schema: HashSchemaVersion,
    /// Canonical synchronizable payload only.
    pub payload: Vec<u8>,
    /// Authoritative deletion state.
    pub tombstone: bool,
}

/// Canonical digest and metadata for one entity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EntityDigest {
    /// Entity covered by the digest.
    pub entity: EntityRef,
    /// Authoritative version covered by the digest.
    pub version: EntityVersion,
    /// Canonical hash result.
    pub hash: IntegrityDigest,
}

impl CanonicalEntity {
    /// Computes the domain-separated, length-delimited canonical entity digest.
    #[must_use]
    pub fn digest(&self) -> EntityDigest {
        let mut hasher = blake3::Hasher::new();
        hasher.update(ENTITY_DOMAIN);
        hasher.update(&self.hash_schema.0.to_be_bytes());
        hasher.update(&self.entity.entity_type.get().to_be_bytes());
        hasher.update(self.entity.entity_id.as_uuid().as_bytes());
        hasher.update(&self.version.get().to_be_bytes());
        hasher.update(&[u8::from(self.tombstone)]);
        hasher.update(
            &u64::try_from(self.payload.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(&self.payload);
        EntityDigest {
            entity: self.entity,
            version: self.version,
            hash: IntegrityDigest::from_bytes(*hasher.finalize().as_bytes()),
        }
    }
}

/// Digest of one deterministic partition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PartitionDigest {
    /// Bucket identity.
    pub partition: IntegrityPartitionId,
    /// Integrity rules used to compute it.
    pub generation: IntegrityGeneration,
    /// Number of canonical entities in the bucket.
    pub entity_count: u64,
    /// Ordered, domain-separated bucket root.
    pub root_hash: IntegrityDigest,
}

/// Root manifest valid for one explicit authoritative cursor boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IntegrityManifest {
    /// Synchronization scope covered by the digest tree.
    pub scope: SyncScopeId,
    /// Hash/partition format generation.
    pub generation: IntegrityGeneration,
    /// Stable state boundary represented by this manifest.
    pub boundary: Cursor,
    /// Root of all deterministic partition leaves.
    pub root_hash: IntegrityDigest,
    /// Exact partition count.
    pub partition_count: u32,
    /// Total canonical entity count.
    pub entity_count: u64,
}

/// Complete bounded digest snapshot used by adapters, verification, and certification.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IntegritySnapshot {
    /// Root manifest.
    pub manifest: IntegrityManifest,
    /// Every partition leaf, ordered by partition ID and including empty buckets.
    pub partitions: Vec<PartitionDigest>,
}

impl IntegritySnapshot {
    /// Computes a deterministic partition tree from an unordered entity collection.
    ///
    /// # Errors
    ///
    /// Fails on duplicate entities, a scope/cursor mismatch, or an excessive entity count.
    pub fn build(
        scope: SyncScopeId,
        boundary: Cursor,
        generation: IntegrityGeneration,
        scheme: PartitionScheme,
        entities: impl IntoIterator<Item = CanonicalEntity>,
        max_entities: usize,
    ) -> Result<Self, IntegrityError> {
        if boundary.scope != scope {
            return Err(IntegrityError::BoundaryScopeMismatch);
        }
        let mut canonical = BTreeMap::new();
        for entity in entities {
            if canonical.len() == max_entities {
                return Err(IntegrityError::EntityLimitExceeded(max_entities));
            }
            let key = entity.entity;
            if canonical.insert(key, entity).is_some() {
                return Err(IntegrityError::DuplicateEntity(key));
            }
        }
        let mut buckets: BTreeMap<IntegrityPartitionId, Vec<EntityDigest>> = BTreeMap::new();
        for entity in canonical.values() {
            buckets
                .entry(scheme.partition(entity.entity))
                .or_default()
                .push(entity.digest());
        }
        let partitions = (0..scheme.bucket_count())
            .map(|partition| {
                let partition = IntegrityPartitionId(partition);
                partition_digest(
                    partition,
                    generation,
                    buckets.get(&partition).map_or(&[], Vec::as_slice),
                )
            })
            .collect::<Vec<_>>();
        let root_hash = merkle_root(
            &partitions
                .iter()
                .map(|partition| partition.root_hash)
                .collect::<Vec<_>>(),
        );
        Ok(Self {
            manifest: IntegrityManifest {
                scope,
                generation,
                boundary,
                root_hash,
                partition_count: scheme.bucket_count(),
                entity_count: u64::try_from(canonical.len()).unwrap_or(u64::MAX),
            },
            partitions,
        })
    }

    /// Returns a logarithmic proof for one partition leaf.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrityError::UnknownPartition`] outside the configured tree.
    pub fn proof(&self, partition: IntegrityPartitionId) -> Result<MerkleProof, IntegrityError> {
        let mut index = usize::try_from(partition.0)
            .map_err(|_| IntegrityError::UnknownPartition(partition))?;
        if index >= self.partitions.len() {
            return Err(IntegrityError::UnknownPartition(partition));
        }
        let leaf = self.partitions[index];
        let mut level = self
            .partitions
            .iter()
            .map(|value| value.root_hash)
            .collect::<Vec<_>>();
        let mut siblings = Vec::new();
        while level.len() > 1 {
            let sibling_index = if index % 2 == 0 {
                (index + 1).min(level.len() - 1)
            } else {
                index - 1
            };
            siblings.push(ProofStep {
                sibling: level[sibling_index],
                sibling_is_left: sibling_index < index,
            });
            level = next_level(&level);
            index /= 2;
        }
        Ok(MerkleProof {
            partition: leaf,
            siblings,
        })
    }

    /// Compares compatible roots and localizes mismatching partition leaves.
    ///
    /// # Errors
    ///
    /// Incompatible generation, scope, boundary, or partition layout must trigger bootstrap or a
    /// newly acquired verification boundary rather than comparing unrelated roots.
    pub fn compare(&self, other: &Self) -> Result<IntegrityComparison, IntegrityError> {
        ensure_compatible(&self.manifest, &other.manifest)?;
        if self.manifest.root_hash == other.manifest.root_hash {
            return Ok(IntegrityComparison::Match);
        }
        let mismatching_partitions = self
            .partitions
            .iter()
            .zip(&other.partitions)
            .filter_map(|(left, right)| {
                (left.root_hash != right.root_hash).then_some(left.partition)
            })
            .collect();
        Ok(IntegrityComparison::Mismatch {
            mismatching_partitions,
        })
    }

    /// Verifies manifest counts, leaf ordering, generation, and the Merkle root.
    ///
    /// # Errors
    ///
    /// Returns [`IntegrityError::MalformedSnapshot`] when serialized or transported integrity
    /// evidence is internally inconsistent.
    pub fn verify_structure(&self) -> Result<(), IntegrityError> {
        let expected_count = usize::try_from(self.manifest.partition_count)
            .map_err(|_| IntegrityError::MalformedSnapshot)?;
        if expected_count == 0
            || self.partitions.len() != expected_count
            || self.partitions.iter().enumerate().any(|(index, leaf)| {
                usize::try_from(leaf.partition.0).ok() != Some(index)
                    || leaf.generation != self.manifest.generation
            })
            || self
                .partitions
                .iter()
                .map(|leaf| leaf.entity_count)
                .sum::<u64>()
                != self.manifest.entity_count
            || merkle_root(
                &self
                    .partitions
                    .iter()
                    .map(|leaf| leaf.root_hash)
                    .collect::<Vec<_>>(),
            ) != self.manifest.root_hash
        {
            return Err(IntegrityError::MalformedSnapshot);
        }
        Ok(())
    }
}

/// One sibling in a deterministic Merkle authentication path.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProofStep {
    /// Sibling hash at this level.
    pub sibling: IntegrityDigest,
    /// Whether the sibling precedes the current node.
    pub sibling_is_left: bool,
}

/// Merkle proof for one deterministic partition leaf.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MerkleProof {
    /// Partition leaf being proven.
    pub partition: PartitionDigest,
    /// Bottom-up sibling path.
    pub siblings: Vec<ProofStep>,
}

impl MerkleProof {
    /// Verifies this path against an expected manifest root.
    #[must_use]
    pub fn verifies(&self, expected_root: IntegrityDigest) -> bool {
        let computed = self
            .siblings
            .iter()
            .fold(self.partition.root_hash, |current, step| {
                if step.sibling_is_left {
                    node_hash(step.sibling, current)
                } else {
                    node_hash(current, step.sibling)
                }
            });
        computed == expected_root
    }
}

/// Root comparison outcome at one compatible generation and boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IntegrityComparison {
    /// Canonical roots match.
    Match,
    /// Roots differ; only these deterministic leaves require deeper inspection.
    Mismatch {
        /// Ordered mismatching partitions.
        mismatching_partitions: Vec<IntegrityPartitionId>,
    },
}

/// Entity-level divergence classification after partition localization.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DivergenceKind {
    /// Authority has an entity absent locally.
    MissingLocal,
    /// Local authoritative base has an entity absent at authority.
    ExtraLocal,
    /// Authoritative versions differ.
    VersionMismatch,
    /// Same authoritative version has different canonical payload bytes.
    SameVersionPayloadMismatch,
    /// Active/deleted state differs.
    TombstoneMismatch,
    /// Canonical writer/schema versions differ.
    SchemaMismatch,
}

/// One localized mismatch, without including application payloads.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EntityDivergence {
    /// Affected entity.
    pub entity: EntityRef,
    /// Stable mismatch classification.
    pub kind: DivergenceKind,
}

/// Compares canonical authoritative bases and returns payload-free mismatch classifications.
#[must_use]
pub fn classify_divergence(
    authority: &[CanonicalEntity],
    local: &[CanonicalEntity],
) -> Vec<EntityDivergence> {
    let authority = authority
        .iter()
        .map(|entity| (entity.entity, entity))
        .collect::<BTreeMap<_, _>>();
    let local = local
        .iter()
        .map(|entity| (entity.entity, entity))
        .collect::<BTreeMap<_, _>>();
    let mut keys = authority
        .keys()
        .chain(local.keys())
        .copied()
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter_map(|entity| {
            let kind = match (authority.get(&entity), local.get(&entity)) {
                (Some(_), None) => DivergenceKind::MissingLocal,
                (None, Some(_)) => DivergenceKind::ExtraLocal,
                (Some(authority), Some(local)) if authority.version != local.version => {
                    DivergenceKind::VersionMismatch
                }
                (Some(authority), Some(local)) if authority.hash_schema != local.hash_schema => {
                    DivergenceKind::SchemaMismatch
                }
                (Some(authority), Some(local)) if authority.tombstone != local.tombstone => {
                    DivergenceKind::TombstoneMismatch
                }
                (Some(authority), Some(local)) if authority.payload != local.payload => {
                    DivergenceKind::SameVersionPayloadMismatch
                }
                _ => return None,
            };
            Some(EntityDivergence { entity, kind })
        })
        .collect()
}

/// Replica-only repair strategy selected from localized evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RepairStrategy {
    /// Replace a small set of authoritative base entities.
    ReplaceEntities,
    /// Replace complete deterministic partitions.
    ReplacePartitions(Vec<IntegrityPartitionId>),
    /// Reinstall the complete authorized scope snapshot.
    BootstrapScope,
    /// Stop automatic mutation pending operator review.
    Quarantine,
}

/// Explicit immutable repair plan. It can never authorize mutation of server state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepairPlan {
    /// Stable repair identity.
    pub repair_id: RepairId,
    /// Authority boundary on which the plan is based.
    pub boundary: Cursor,
    /// Sorted affected entities.
    pub affected_entities: Vec<EntityRef>,
    /// Conservative repair action.
    pub strategy: RepairStrategy,
}

/// Policy bounds for automatic repair escalation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepairPolicy {
    /// Maximum entity replacements allowed automatically.
    pub max_automatic_entities: usize,
    /// Mismatch percentage at which a full bootstrap is cheaper/safer.
    pub bootstrap_threshold_percent: u8,
}

impl Default for RepairPolicy {
    fn default() -> Self {
        Self {
            max_automatic_entities: 100,
            bootstrap_threshold_percent: 10,
        }
    }
}

/// Builds a conservative replica repair plan from payload-free divergence evidence.
#[must_use]
pub fn plan_repair(
    boundary: Cursor,
    divergences: &[EntityDivergence],
    covered_entity_count: usize,
    policy: RepairPolicy,
) -> RepairPlan {
    let suspicious = divergences.iter().any(|value| {
        matches!(
            value.kind,
            DivergenceKind::SameVersionPayloadMismatch | DivergenceKind::SchemaMismatch
        )
    });
    let percent = divergences
        .len()
        .saturating_mul(100)
        .checked_div(covered_entity_count)
        .unwrap_or(100);
    let strategy = if suspicious {
        RepairStrategy::Quarantine
    } else if divergences.len() > policy.max_automatic_entities
        || percent >= usize::from(policy.bootstrap_threshold_percent)
    {
        RepairStrategy::BootstrapScope
    } else {
        RepairStrategy::ReplaceEntities
    };
    let mut affected_entities = divergences
        .iter()
        .map(|value| value.entity)
        .collect::<Vec<_>>();
    affected_entities.sort_unstable();
    affected_entities.dedup();
    RepairPlan {
        repair_id: RepairId::new(),
        boundary,
        affected_entities,
        strategy,
    }
}

/// Exact pending operation intent captured before authoritative-base repair.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PendingIntent {
    /// Durable idempotency identity.
    pub operation_id: OperationId,
    /// Entity touched by the operation.
    pub entity: EntityRef,
    /// Exact opaque serialized operation retained for replay/rebase.
    pub opaque_operation: Vec<u8>,
}

/// Result of atomically replacing local authoritative base while retaining optimistic intent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepairApplication {
    /// Repaired authoritative base.
    pub authoritative_base: Vec<CanonicalEntity>,
    /// Exact pending operations, byte-for-byte unchanged.
    pub pending_intents: Vec<PendingIntent>,
    /// Normal synchronization cursor, deliberately unchanged by repair.
    pub sync_cursor: Cursor,
}

/// Pure reference repair transaction used by adapters and model tests.
#[must_use]
pub fn apply_authoritative_repair(
    current: &[CanonicalEntity],
    replacements: &[CanonicalEntity],
    pending_intents: &[PendingIntent],
    sync_cursor: Cursor,
) -> RepairApplication {
    let mut base = current
        .iter()
        .cloned()
        .map(|entity| (entity.entity, entity))
        .collect::<BTreeMap<_, _>>();
    for replacement in replacements {
        base.insert(replacement.entity, replacement.clone());
    }
    RepairApplication {
        authoritative_base: base.into_values().collect(),
        pending_intents: pending_intents.to_vec(),
        sync_cursor,
    }
}

/// Durable verification lifecycle, independent of normal replication state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum VerificationState {
    /// Waiting for a trigger.
    Idle,
    /// Normal sync is catching up before a stable boundary is acquired.
    CatchUp,
    /// A stable verification boundary is being acquired.
    AcquireBoundary,
    /// Root comparison is in progress.
    CompareRoot,
    /// Partition localization is in progress.
    ComparePartitions,
    /// A repair plan is being constructed.
    RepairPlan,
    /// Replica repair is in progress.
    Repair,
    /// Repaired state is being verified again.
    Reverify,
    /// Compatible roots matched.
    Verified,
    /// Automatic mutation is stopped for operator review.
    Quarantined,
}

/// Events accepted by the verification state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationEvent {
    Start,
    CaughtUp,
    BoundaryAcquired,
    RootMatched,
    RootMismatched,
    Localized,
    PlanReady,
    RepairCommitted,
    Reverified,
    Quarantine,
    Reset,
}

impl VerificationState {
    /// Performs one legal deterministic lifecycle transition.
    ///
    /// # Errors
    ///
    /// Invalid transitions fail closed without changing persistent state.
    pub const fn transition(self, event: VerificationEvent) -> Result<Self, IntegrityError> {
        match (self, event) {
            (Self::Idle, VerificationEvent::Start) => Ok(Self::CatchUp),
            (Self::CatchUp, VerificationEvent::CaughtUp) => Ok(Self::AcquireBoundary),
            (Self::AcquireBoundary, VerificationEvent::BoundaryAcquired) => Ok(Self::CompareRoot),
            (Self::CompareRoot, VerificationEvent::RootMatched)
            | (Self::Reverify, VerificationEvent::Reverified) => Ok(Self::Verified),
            (Self::CompareRoot, VerificationEvent::RootMismatched) => Ok(Self::ComparePartitions),
            (Self::ComparePartitions, VerificationEvent::Localized) => Ok(Self::RepairPlan),
            (Self::RepairPlan, VerificationEvent::PlanReady) => Ok(Self::Repair),
            (Self::Repair, VerificationEvent::RepairCommitted) => Ok(Self::Reverify),
            (_, VerificationEvent::Quarantine) => Ok(Self::Quarantined),
            (Self::Verified | Self::Quarantined, VerificationEvent::Reset) => Ok(Self::Idle),
            _ => Err(IntegrityError::InvalidStateTransition),
        }
    }
}

/// Client integrity metadata deliberately separate from its synchronization cursor.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct IntegrityMetadata {
    pub last_verified_boundary: Option<Cursor>,
    pub last_verified_generation: Option<IntegrityGeneration>,
    pub last_verified_root: Option<IntegrityDigest>,
    pub last_verified_at: Option<HybridTimestamp>,
    pub last_repair_id: Option<RepairId>,
}

/// Bounded root verification request, transport-independent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IntegrityRootRequest {
    pub session_id: IntegritySessionId,
    pub scope: SyncScopeId,
    pub cursor: Cursor,
    pub generation: Option<IntegrityGeneration>,
    pub client_root: Option<IntegrityDigest>,
}

/// Bounded root response that either matches or directs partition comparison.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IntegrityRootResponse {
    pub session_id: IntegritySessionId,
    pub manifest: IntegrityManifest,
    pub status: IntegrityComparison,
}

/// Stable integrity failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum IntegrityError {
    #[error("integrity partition count {0} is outside the supported range")]
    InvalidPartitionCount(u32),
    #[error("integrity boundary belongs to another scope")]
    BoundaryScopeMismatch,
    #[error("integrity input exceeds the configured {0}-entity bound")]
    EntityLimitExceeded(usize),
    #[error("canonical integrity input contains duplicate entity {0:?}")]
    DuplicateEntity(EntityRef),
    #[error("unknown integrity partition {0:?}")]
    UnknownPartition(IntegrityPartitionId),
    #[error("integrity generations are incompatible")]
    GenerationMismatch,
    #[error("integrity manifests cover different scopes")]
    ScopeMismatch,
    #[error("integrity manifests cover different cursor boundaries")]
    BoundaryMismatch,
    #[error("integrity manifests use different partition layouts")]
    PartitionLayoutMismatch,
    #[error("integrity snapshot is internally inconsistent")]
    MalformedSnapshot,
    #[error("invalid integrity verification state transition")]
    InvalidStateTransition,
}

fn partition_digest(
    partition: IntegrityPartitionId,
    generation: IntegrityGeneration,
    entities: &[EntityDigest],
) -> PartitionDigest {
    let mut hasher = blake3::Hasher::new();
    hasher.update(PARTITION_DOMAIN);
    hasher.update(&generation.0.to_be_bytes());
    hasher.update(&partition.0.to_be_bytes());
    hasher.update(
        &u64::try_from(entities.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for entity in entities {
        hasher.update(&entity.entity.entity_type.get().to_be_bytes());
        hasher.update(entity.entity.entity_id.as_uuid().as_bytes());
        hasher.update(&entity.version.get().to_be_bytes());
        hasher.update(&entity.hash.as_bytes());
    }
    PartitionDigest {
        partition,
        generation,
        entity_count: u64::try_from(entities.len()).unwrap_or(u64::MAX),
        root_hash: IntegrityDigest::from_bytes(*hasher.finalize().as_bytes()),
    }
}

fn merkle_root(leaves: &[IntegrityDigest]) -> IntegrityDigest {
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        level = next_level(&level);
    }
    level[0]
}

fn next_level(level: &[IntegrityDigest]) -> Vec<IntegrityDigest> {
    level
        .chunks(2)
        .map(|pair| node_hash(pair[0], pair.get(1).copied().unwrap_or(pair[0])))
        .collect()
}

fn node_hash(left: IntegrityDigest, right: IntegrityDigest) -> IntegrityDigest {
    let mut hasher = blake3::Hasher::new();
    hasher.update(NODE_DOMAIN);
    hasher.update(&left.as_bytes());
    hasher.update(&right.as_bytes());
    IntegrityDigest::from_bytes(*hasher.finalize().as_bytes())
}

fn ensure_compatible(
    left: &IntegrityManifest,
    right: &IntegrityManifest,
) -> Result<(), IntegrityError> {
    if left.generation != right.generation {
        return Err(IntegrityError::GenerationMismatch);
    }
    if left.scope != right.scope {
        return Err(IntegrityError::ScopeMismatch);
    }
    if left.boundary != right.boundary {
        return Err(IntegrityError::BoundaryMismatch);
    }
    if left.partition_count != right.partition_count {
        return Err(IntegrityError::PartitionLayoutMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_types::{EntityId, EntityType, Sequence};

    fn entity(value: u8) -> CanonicalEntity {
        CanonicalEntity {
            entity: EntityRef {
                entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
                entity_id: EntityId::from_uuid(uuid_from_byte(value)),
            },
            version: EntityVersion::INITIAL,
            hash_schema: CURRENT_HASH_SCHEMA,
            payload: vec![value],
            tombstone: false,
        }
    }

    fn uuid_from_byte(value: u8) -> uuid::Uuid {
        let mut bytes = [0_u8; 16];
        bytes[15] = value;
        uuid::Uuid::from_bytes(bytes)
    }

    fn snapshot(entities: Vec<CanonicalEntity>) -> IntegritySnapshot {
        let scope = SyncScopeId::from_uuid(uuid_from_byte(42));
        IntegritySnapshot::build(
            scope,
            Cursor {
                scope,
                sequence: Sequence(9),
            },
            CURRENT_INTEGRITY_GENERATION,
            PartitionScheme::new(8).unwrap_or_else(|error| panic!("{error}")),
            entities,
            100,
        )
        .unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn roots_are_database_order_independent_and_proofs_verify() {
        let left = snapshot(vec![entity(1), entity(2), entity(3)]);
        let right = snapshot(vec![entity(3), entity(1), entity(2)]);
        assert_eq!(left, right);
        for partition in 0..left.manifest.partition_count {
            let proof = left
                .proof(IntegrityPartitionId(partition))
                .unwrap_or_else(|error| panic!("{error}"));
            assert!(proof.verifies(left.manifest.root_hash));
        }
    }

    #[test]
    fn one_payload_change_localizes_and_same_version_mismatch_quarantines() {
        let authority_entities = vec![entity(1), entity(2)];
        let mut local_entities = authority_entities.clone();
        local_entities[1].payload = vec![99];
        let authority = snapshot(authority_entities.clone());
        let local = snapshot(local_entities.clone());
        assert!(matches!(
            authority.compare(&local),
            Ok(IntegrityComparison::Mismatch { ref mismatching_partitions })
                if mismatching_partitions.len() == 1
        ));
        let divergence = classify_divergence(&authority_entities, &local_entities);
        assert_eq!(
            divergence[0].kind,
            DivergenceKind::SameVersionPayloadMismatch
        );
        assert_eq!(
            plan_repair(
                authority.manifest.boundary,
                &divergence,
                authority_entities.len(),
                RepairPolicy::default(),
            )
            .strategy,
            RepairStrategy::Quarantine
        );
    }

    #[test]
    fn repair_preserves_pending_bytes_and_never_advances_cursor() {
        let current = vec![entity(1)];
        let replacement = entity(2);
        let pending = vec![PendingIntent {
            operation_id: OperationId::new(),
            entity: replacement.entity,
            opaque_operation: vec![7, 8, 9],
        }];
        let scope = SyncScopeId::new();
        let cursor = Cursor {
            scope,
            sequence: Sequence(44),
        };
        let repaired = apply_authoritative_repair(&current, &[replacement], &pending, cursor);
        assert_eq!(repaired.pending_intents, pending);
        assert_eq!(repaired.sync_cursor, cursor);
        assert_eq!(repaired.authoritative_base.len(), 2);
    }

    #[test]
    fn verification_state_machine_requires_reverification_after_repair() {
        let state = VerificationState::Idle
            .transition(VerificationEvent::Start)
            .and_then(|state| state.transition(VerificationEvent::CaughtUp))
            .and_then(|state| state.transition(VerificationEvent::BoundaryAcquired))
            .and_then(|state| state.transition(VerificationEvent::RootMismatched))
            .and_then(|state| state.transition(VerificationEvent::Localized))
            .and_then(|state| state.transition(VerificationEvent::PlanReady))
            .and_then(|state| state.transition(VerificationEvent::RepairCommitted));
        assert_eq!(state, Ok(VerificationState::Reverify));
        assert_eq!(
            VerificationState::Repair.transition(VerificationEvent::RootMatched),
            Err(IntegrityError::InvalidStateTransition)
        );
    }

    #[test]
    fn executable_fault_model_detects_delete_modify_and_tombstone_corruption() {
        let authority = vec![entity(1), entity(2), entity(3)];
        for fault in 0..3 {
            let mut local = authority.clone();
            match fault {
                0 => {
                    local.remove(1);
                }
                1 => local[1].payload.push(99),
                2 => local[1].tombstone = true,
                _ => unreachable!(),
            }
            let comparison = snapshot(authority.clone()).compare(&snapshot(local.clone()));
            assert!(matches!(
                comparison,
                Ok(IntegrityComparison::Mismatch { ref mismatching_partitions })
                    if !mismatching_partitions.is_empty()
            ));
            let divergences = classify_divergence(&authority, &local);
            assert_eq!(divergences.len(), 1);
            let expected = match fault {
                0 => DivergenceKind::MissingLocal,
                1 => DivergenceKind::SameVersionPayloadMismatch,
                2 => DivergenceKind::TombstoneMismatch,
                _ => unreachable!(),
            };
            assert_eq!(divergences[0].kind, expected);
        }
    }

    #[test]
    fn serialized_snapshot_structure_fails_closed_after_root_tampering() {
        let mut evidence = snapshot(vec![entity(1), entity(2)]);
        assert_eq!(evidence.verify_structure(), Ok(()));
        evidence.partitions[0].root_hash = IntegrityDigest::from_bytes([0xA5; 32]);
        assert_eq!(
            evidence.verify_structure(),
            Err(IntegrityError::MalformedSnapshot)
        );
    }
}
