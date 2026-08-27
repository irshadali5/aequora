//! Durable import planning, identity, checkpoint, quarantine, baseline, and cutover contracts.

use std::collections::{BTreeMap, BTreeSet};

use aequora_schema::{CanonicalEntityId, CanonicalRecord, SchemaRegistry};
use aequora_types::{CorrelationId, EntityId, EntityVersion, Sequence, SyncScopeId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::CanonicalExport;

/// Maximum bytes in an operator-facing source identity or stable source record key.
const MAX_SOURCE_ID_BYTES: usize = 512;

/// Semantically distinct migration paths. One job uses exactly one mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MigrationMode {
    SeedAuthority,
    ImportAsOperations,
    ImportSnapshot,
    LegacyBridge,
    StoreToStoreMigration,
}

/// Stable identity of one durable import job.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ImportJobId(Uuid);

impl ImportJobId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for ImportJobId {
    fn default() -> Self {
        Self::new()
    }
}

/// Stable deployment-assigned source-system identity.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SourceSystemId(String);

impl SourceSystemId {
    /// Creates a bounded non-blank identity.
    ///
    /// # Errors
    ///
    /// Returns [`ImportWorkflowError::InvalidText`] for blank or oversized input.
    pub fn new(value: impl Into<String>) -> Result<Self, ImportWorkflowError> {
        bounded_text(value).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Retry-stable source record reference used for identity, ledger, and quarantine.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SourceRecordKey(String);

impl SourceRecordKey {
    /// Creates a bounded non-blank key.
    ///
    /// # Errors
    ///
    /// Returns [`ImportWorkflowError::InvalidText`] for blank or oversized input.
    pub fn new(value: impl Into<String>) -> Result<Self, ImportWorkflowError> {
        bounded_text(value).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn bounded_text(value: impl Into<String>) -> Result<String, ImportWorkflowError> {
    let value = value.into();
    if value.trim().is_empty() || value.len() > MAX_SOURCE_ID_BYTES {
        Err(ImportWorkflowError::InvalidText)
    } else {
        Ok(value)
    }
}

/// Immutable resume guard binding source bytes, schema, and snapshot/change boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceFingerprint {
    pub digest: [u8; 32],
    pub source_schema_version: u32,
    pub snapshot_token: Option<String>,
}

impl SourceFingerprint {
    /// Hashes an immutable file/snapshot representation with source identity and metadata.
    ///
    /// # Errors
    ///
    /// Rejects zero schema versions and oversized snapshot tokens.
    pub fn calculate(
        source: &SourceSystemId,
        source_schema_version: u32,
        snapshot_token: Option<String>,
        content: &[u8],
    ) -> Result<Self, ImportWorkflowError> {
        if source_schema_version == 0 {
            return Err(ImportWorkflowError::ZeroVersion);
        }
        if snapshot_token
            .as_ref()
            .is_some_and(|token| token.trim().is_empty() || token.len() > MAX_SOURCE_ID_BYTES)
        {
            return Err(ImportWorkflowError::InvalidText);
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(source.as_str().as_bytes());
        hasher.update(&source_schema_version.to_le_bytes());
        if let Some(token) = &snapshot_token {
            hasher.update(token.as_bytes());
        }
        hasher.update(content);
        Ok(Self {
            digest: *hasher.finalize().as_bytes(),
            source_schema_version,
            snapshot_token,
        })
    }
}

/// Durable legal state of one import job.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImportJobState {
    Planned,
    Scanning,
    Transforming,
    Importing,
    Verifying,
    ReadyForCutover,
    Completed,
    Failed,
    Quarantined,
}

impl ImportJobState {
    #[must_use]
    pub const fn may_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Planned | Self::Failed | Self::Quarantined,
                Self::Scanning
            ) | (Self::Scanning, Self::Transforming)
                | (Self::Transforming, Self::Importing)
                | (Self::Importing, Self::Verifying)
                | (Self::Verifying, Self::ReadyForCutover)
                | (Self::ReadyForCutover, Self::Completed)
                | (
                    Self::Planned
                        | Self::Scanning
                        | Self::Transforming
                        | Self::Importing
                        | Self::Verifying
                        | Self::ReadyForCutover,
                    Self::Failed | Self::Quarantined
                )
        )
    }
}

/// Adapter-specific monotonic CDC or polling boundary.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct MigrationWatermark(String);

impl MigrationWatermark {
    /// Creates a bounded non-blank watermark.
    ///
    /// # Errors
    ///
    /// Rejects blank or oversized values.
    pub fn new(value: impl Into<String>) -> Result<Self, ImportWorkflowError> {
        bounded_text(value).map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Checkpoint that may advance only with the corresponding committed target batch.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportCheckpoint {
    pub committed_batches: u64,
    pub committed_records: u64,
    pub source_offset: u64,
    pub last_source_key: Option<SourceRecordKey>,
    pub watermark: Option<MigrationWatermark>,
}

impl ImportCheckpoint {
    /// Checks strict monotonicity and exact one-batch advancement.
    ///
    /// # Errors
    ///
    /// Rejects regression, skipped batch identity, or a record-count decrease.
    pub fn validate_successor(&self, next: &Self) -> Result<(), ImportWorkflowError> {
        if next.committed_batches != self.committed_batches.saturating_add(1)
            || next.committed_records < self.committed_records
            || next.source_offset < self.source_offset
            || next.watermark < self.watermark
        {
            return Err(ImportWorkflowError::CheckpointRegression);
        }
        Ok(())
    }
}

/// Durable job metadata without source credentials or raw quarantined payloads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportJob {
    pub job_id: ImportJobId,
    pub correlation_id: CorrelationId,
    pub mode: MigrationMode,
    pub source: SourceSystemId,
    pub fingerprint: SourceFingerprint,
    pub state: ImportJobState,
    pub checkpoint: ImportCheckpoint,
    pub mapping_version: u32,
    pub policy_version: u32,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl ImportJob {
    /// Creates planned metadata with immutable resume guards.
    ///
    /// # Errors
    ///
    /// Rejects zero mapping/policy versions.
    pub fn new(
        mode: MigrationMode,
        source: SourceSystemId,
        fingerprint: SourceFingerprint,
        mapping_version: u32,
        policy_version: u32,
        now_unix_ms: u64,
    ) -> Result<Self, ImportWorkflowError> {
        if mapping_version == 0 || policy_version == 0 {
            return Err(ImportWorkflowError::ZeroVersion);
        }
        Ok(Self {
            job_id: ImportJobId::new(),
            correlation_id: CorrelationId::new(),
            mode,
            source,
            fingerprint,
            state: ImportJobState::Planned,
            checkpoint: ImportCheckpoint::default(),
            mapping_version,
            policy_version,
            created_at_unix_ms: now_unix_ms,
            updated_at_unix_ms: now_unix_ms,
        })
    }

    /// Applies one legal state transition without changing resume identity.
    ///
    /// # Errors
    ///
    /// Rejects illegal or post-completion transitions and time regression.
    pub fn transition(
        &mut self,
        next: ImportJobState,
        now_unix_ms: u64,
    ) -> Result<(), ImportWorkflowError> {
        if !self.state.may_transition_to(next) {
            return Err(ImportWorkflowError::InvalidStateTransition);
        }
        if now_unix_ms < self.updated_at_unix_ms {
            return Err(ImportWorkflowError::TimeRegression);
        }
        self.state = next;
        self.updated_at_unix_ms = now_unix_ms;
        Ok(())
    }

    /// Verifies an attempted resume uses the exact source, mapping, and policy identity.
    ///
    /// # Errors
    ///
    /// Rejects source mutation or policy drift.
    pub fn validate_resume(
        &self,
        fingerprint: &SourceFingerprint,
        mapping_version: u32,
        policy_version: u32,
    ) -> Result<(), ImportWorkflowError> {
        if &self.fingerprint != fingerprint {
            return Err(ImportWorkflowError::SourceFingerprintChanged);
        }
        if self.mapping_version != mapping_version || self.policy_version != policy_version {
            return Err(ImportWorkflowError::PolicyChanged);
        }
        Ok(())
    }
}

/// Identity assignment policy for source records.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IdMappingStrategy {
    PreserveExistingUuid,
    DeterministicNamespace,
    MappingTable,
    NewUuidWithMap,
}

/// Two-pass stable source-to-target identity plan.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct IdentityPlan {
    mappings: BTreeMap<(CanonicalEntityId, SourceRecordKey), EntityId>,
}

impl IdentityPlan {
    /// Plans an identity once. Repeating the same mapping is idempotent; drift fails closed.
    ///
    /// # Errors
    ///
    /// Rejects malformed UUIDs, missing persisted mappings, or mapping drift.
    pub fn assign(
        &mut self,
        strategy: IdMappingStrategy,
        namespace: Uuid,
        entity: CanonicalEntityId,
        source_key: SourceRecordKey,
        persisted: Option<EntityId>,
    ) -> Result<EntityId, ImportWorkflowError> {
        let target = match strategy {
            IdMappingStrategy::PreserveExistingUuid => {
                let uuid = Uuid::parse_str(source_key.as_str())
                    .map_err(|_| ImportWorkflowError::InvalidUuid)?;
                EntityId::from_uuid(uuid)
            }
            IdMappingStrategy::DeterministicNamespace => {
                deterministic_entity_id(namespace, entity, &source_key)
            }
            IdMappingStrategy::MappingTable | IdMappingStrategy::NewUuidWithMap => {
                persisted.ok_or(ImportWorkflowError::MissingPersistedMapping)?
            }
        };
        let key = (entity, source_key);
        if let Some(existing) = self.mappings.get(&key) {
            if *existing != target {
                return Err(ImportWorkflowError::IdentityMappingChanged);
            }
            return Ok(*existing);
        }
        self.mappings.insert(key, target);
        Ok(target)
    }

    #[must_use]
    pub fn resolve(
        &self,
        entity: CanonicalEntityId,
        source_key: &SourceRecordKey,
    ) -> Option<EntityId> {
        self.mappings.get(&(entity, source_key.clone())).copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.mappings.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }
}

fn deterministic_entity_id(
    namespace: Uuid,
    entity: CanonicalEntityId,
    source_key: &SourceRecordKey,
) -> EntityId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(namespace.as_bytes());
    hasher.update(&entity.get().to_le_bytes());
    hasher.update(source_key.as_str().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    // RFC 9562 UUIDv8 variant: deterministic application-defined payload.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    EntityId::from_uuid(Uuid::from_bytes(bytes))
}

/// Source mapping result before canonical transformation and validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MappedRecord {
    pub source_key: SourceRecordKey,
    pub entity: CanonicalEntityId,
    pub target_id: EntityId,
    pub attributes: BTreeMap<String, String>,
}

/// Mapping and transformation are deliberately distinct, testable phases.
pub trait SourceRecordMapper<S>: Send + Sync {
    /// Maps source identity/fields without canonical transformation.
    ///
    /// # Errors
    ///
    /// Returns a stable record error suitable for durable quarantine.
    fn map_source(
        &self,
        source: &S,
        identities: &IdentityPlan,
    ) -> Result<MappedRecord, ImportRecordError>;
}

/// Converts mapped fields into the canonical schema without database writes.
pub trait CanonicalTransformer: Send + Sync {
    /// Transforms mapped fields into one canonical record.
    ///
    /// # Errors
    ///
    /// Returns a stable transformation error suitable for durable quarantine.
    fn transform(&self, mapped: MappedRecord) -> Result<CanonicalRecord, ImportRecordError>;
}

/// Applies schema plus application/domain validation after transformation.
pub trait ImportRecordValidator: Send + Sync {
    /// Validates canonical schema and application-owned domain rules.
    ///
    /// # Errors
    ///
    /// Returns a stable validation error suitable for durable quarantine.
    fn validate(
        &self,
        record: &CanonicalRecord,
        registry: &SchemaRegistry,
    ) -> Result<(), ImportRecordError>;
}

/// Stable error code retained without requiring raw rejected values.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{code}: {message}")]
pub struct ImportRecordError {
    pub code: String,
    pub message: String,
}

/// Durable quarantine workflow state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum QuarantineStatus {
    Open,
    Corrected,
    Waived,
    Reimported,
}

/// Payload-minimized durable rejection reference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuarantineEntry {
    pub job_id: ImportJobId,
    pub source_key: SourceRecordKey,
    pub entity: CanonicalEntityId,
    pub error_code: String,
    pub source_checksum: [u8; 32],
    pub status: QuarantineStatus,
    pub resolution: Option<String>,
}

/// Strict or explicitly thresholded import acceptance policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImportStrictness {
    Strict,
    Tolerant { max_quarantine_percent: u8 },
}

impl ImportStrictness {
    /// Evaluates the cutover threshold using integer arithmetic.
    ///
    /// # Errors
    ///
    /// Rejects invalid percentages and any strict-mode quarantine.
    pub fn permits(self, total: u64, quarantined: u64) -> Result<bool, ImportWorkflowError> {
        if quarantined > total {
            return Err(ImportWorkflowError::InvalidCounts);
        }
        match self {
            Self::Strict => Ok(quarantined == 0),
            Self::Tolerant {
                max_quarantine_percent,
            } if max_quarantine_percent <= 100 => Ok(u128::from(quarantined).saturating_mul(100)
                <= u128::from(total).saturating_mul(u128::from(max_quarantine_percent))),
            Self::Tolerant { .. } => Err(ImportWorkflowError::InvalidPercentage),
        }
    }
}

/// Duplicate handling must be explicit; high-assurance default is rejection.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum DuplicatePolicy {
    #[default]
    Reject,
    KeepFirst,
    KeepLast,
    MergeCustom,
}

/// One transformed, checksummed, dependency-aware target record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedImportRecord {
    pub source_key: SourceRecordKey,
    pub target_id: EntityId,
    pub checksum: [u8; 32],
    pub record: CanonicalRecord,
    pub dependencies: Vec<EntityId>,
    pub scopes: Vec<SyncScopeId>,
}

/// Bounded batch and the checkpoint that must commit with it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportBatch {
    pub records: Vec<PreparedImportRecord>,
    pub next_checkpoint: ImportCheckpoint,
}

/// Hard limits checked before adapter transaction entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportBatchLimits {
    pub max_records: usize,
    pub max_encoded_bytes: usize,
}

impl ImportBatch {
    /// Validates bounded work and exact checkpoint succession.
    ///
    /// # Errors
    ///
    /// Rejects empty/oversized batches or invalid checkpoint progression.
    pub fn validate(
        &self,
        current: &ImportCheckpoint,
        limits: ImportBatchLimits,
    ) -> Result<(), ImportWorkflowError> {
        if self.records.is_empty() || self.records.len() > limits.max_records {
            return Err(ImportWorkflowError::BatchRecordLimit);
        }
        let encoded = postcard::to_stdvec(&(
            self.records
                .iter()
                .map(|record| (&record.source_key, record.target_id, record.checksum))
                .collect::<Vec<_>>(),
            &self.next_checkpoint,
        ))
        .map_err(|_| ImportWorkflowError::Serialization)?;
        if encoded.len() > limits.max_encoded_bytes {
            return Err(ImportWorkflowError::BatchByteLimit);
        }
        current.validate_successor(&self.next_checkpoint)
    }
}

/// Adapter transaction that commits records, import ledger, and checkpoint atomically.
#[async_trait]
pub trait CheckpointedImportSink: Send + Sync {
    async fn commit_batch_and_checkpoint(
        &self,
        job: &ImportJob,
        expected: &ImportCheckpoint,
        batch: &ImportBatch,
    ) -> Result<BatchCommitOutcome, ImportWorkflowStoreError>;

    async fn quarantine(&self, entry: &QuarantineEntry) -> Result<(), ImportWorkflowStoreError>;
}

/// Retry-safe target batch result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchCommitOutcome {
    Applied,
    Duplicate,
}

/// Entity-class dependency node used for topological import ordering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportEntityGroup {
    pub entity: CanonicalEntityId,
    pub depends_on: BTreeSet<CanonicalEntityId>,
}

/// Builds a deterministic parent-before-child order.
///
/// # Errors
///
/// Rejects missing dependency groups and cycles; it never invents placeholder parents.
pub fn plan_entity_order(
    groups: &[ImportEntityGroup],
) -> Result<Vec<CanonicalEntityId>, ImportWorkflowError> {
    let mut remaining: BTreeMap<_, _> = groups
        .iter()
        .map(|group| (group.entity, group.depends_on.clone()))
        .collect();
    if remaining.len() != groups.len()
        || remaining
            .values()
            .flatten()
            .any(|dependency| !remaining.contains_key(dependency))
    {
        return Err(ImportWorkflowError::DependencyMissing);
    }
    let mut ordered = Vec::with_capacity(groups.len());
    while !remaining.is_empty() {
        let ready: Vec<_> = remaining
            .iter()
            .filter(|(_, dependencies)| {
                dependencies
                    .iter()
                    .all(|dependency| ordered.contains(dependency))
            })
            .map(|(entity, _)| *entity)
            .collect();
        if ready.is_empty() {
            return Err(ImportWorkflowError::DependencyCycle);
        }
        for entity in ready {
            remaining.remove(&entity);
            ordered.push(entity);
        }
    }
    Ok(ordered)
}

/// Initial concurrency-version treatment for seeded state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeedVersionPolicy {
    Initial,
    PreserveTrusted,
    DeriveFromImportedHistory,
}

impl SeedVersionPolicy {
    /// Resolves a version without treating timestamps as concurrency counters.
    ///
    /// # Errors
    ///
    /// Requires a trusted nonzero version for preservation/history modes.
    pub fn resolve(
        self,
        trusted_version: Option<EntityVersion>,
    ) -> Result<EntityVersion, ImportWorkflowError> {
        match self {
            Self::Initial => Ok(EntityVersion::INITIAL),
            Self::PreserveTrusted | Self::DeriveFromImportedHistory => {
                trusted_version.ok_or(ImportWorkflowError::MissingTrustedVersion)
            }
        }
    }
}

/// Baseline snapshot contract. Historical rows do not require synthetic journal events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaselinePlan {
    pub import_job_id: ImportJobId,
    pub baseline_sequence: Sequence,
    pub version_policy: SeedVersionPolicy,
    pub scope_snapshots: BTreeSet<SyncScopeId>,
    pub canonical_root: [u8; 32],
}

/// Authority write semantics for each migration mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorityImportKind {
    /// Pre-activation baseline state; no fabricated per-row journal or operation ledger.
    SeedBaseline,
    /// Historical domain actions with deterministic operation/event provenance.
    ImportedOperations,
    /// Post-activation bridge changes that must be journal-visible normally.
    LiveBridgeChange,
}

impl AuthorityImportKind {
    /// Whether every accepted record must publish through normal journal/ledger semantics.
    #[must_use]
    pub const fn requires_journal(self) -> bool {
        matches!(self, Self::ImportedOperations | Self::LiveBridgeChange)
    }
}

/// Database-neutral authority migration boundary.
#[async_trait]
pub trait AuthorityImportSink: Send + Sync {
    async fn apply_authority_batch(
        &self,
        job: &ImportJob,
        kind: AuthorityImportKind,
        expected: &ImportCheckpoint,
        batch: &ImportBatch,
    ) -> Result<BatchCommitOutcome, ImportWorkflowStoreError>;

    /// Atomically records baseline sequence/root and publishes all required scope snapshots.
    async fn publish_baseline(&self, plan: &BaselinePlan) -> Result<(), ImportWorkflowStoreError>;

    /// Atomically fences the legacy writer and activates the new authority generation.
    async fn activate_cutover(
        &self,
        job_id: ImportJobId,
        final_watermark: Option<&MigrationWatermark>,
    ) -> Result<(), ImportWorkflowStoreError>;
}

/// Legacy writer catch-up boundary for CDC, outbox, or monotonic polling adapters.
#[async_trait]
pub trait LegacyChangeSource: Send + Sync {
    async fn changes_after(
        &self,
        watermark: Option<&MigrationWatermark>,
        max_records: usize,
    ) -> Result<LegacyChangePage, ImportWorkflowStoreError>;
}

/// Bounded source delta page; checkpointing its watermark occurs with target commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyChangePage {
    pub records: Vec<PreparedImportRecord>,
    pub next_watermark: MigrationWatermark,
    pub has_more: bool,
}

/// Live cutover strategy selected during planning.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CutoverStrategy {
    MaintenanceWindow,
    DualWriteBridge,
    CdcBridge,
    IncrementalCatchup,
}

/// One explicit verification result used by the cutover gate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EvidenceCheck {
    Failed,
    Passed,
}

impl EvidenceCheck {
    const fn passed(self) -> bool {
        matches!(self, Self::Passed)
    }
}

/// Complete evidence required before the single-writer cutover transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CutoverEvidence {
    pub fingerprint_matches: EvidenceCheck,
    pub canonical_root_matches: EvidenceCheck,
    pub domain_totals_match: EvidenceCheck,
    pub referential_integrity_valid: EvidenceCheck,
    pub scope_snapshots_verified: EvidenceCheck,
    pub source_lag: u64,
    pub quarantine_permitted: EvidenceCheck,
    pub backup_verified: EvidenceCheck,
    pub rollback_rehearsed: EvidenceCheck,
    pub client_bootstrap_verified: EvidenceCheck,
    pub legacy_writer_fenced: EvidenceCheck,
}

/// Fail-closed cutover gate.
///
/// # Errors
///
/// Returns the first unmet safety requirement; activation remains adapter-atomic.
pub fn verify_cutover(evidence: &CutoverEvidence) -> Result<(), CutoverBlocker> {
    if !evidence.fingerprint_matches.passed() {
        return Err(CutoverBlocker::SourceChanged);
    }
    if !evidence.canonical_root_matches.passed()
        || !evidence.domain_totals_match.passed()
        || !evidence.referential_integrity_valid.passed()
        || !evidence.scope_snapshots_verified.passed()
    {
        return Err(CutoverBlocker::VerificationFailed);
    }
    if evidence.source_lag != 0 {
        return Err(CutoverBlocker::SourceLag);
    }
    if !evidence.quarantine_permitted.passed() {
        return Err(CutoverBlocker::QuarantineThreshold);
    }
    if !evidence.backup_verified.passed() || !evidence.rollback_rehearsed.passed() {
        return Err(CutoverBlocker::RecoveryEvidence);
    }
    if !evidence.client_bootstrap_verified.passed() {
        return Err(CutoverBlocker::ClientBootstrap);
    }
    if !evidence.legacy_writer_fenced.passed() {
        return Err(CutoverBlocker::SplitBrainRisk);
    }
    Ok(())
}

/// Canonical export use case; all modes retain the same verified artifact core.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExportMode {
    CanonicalSnapshot,
    DomainExport,
    PendingClientOperations,
    MigrationBundle,
}

/// Operational manifest surrounding one canonical export artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExportBundleManifest {
    pub mode: ExportMode,
    pub created_at_unix_ms: u64,
    pub scopes: BTreeSet<SyncScopeId>,
    pub cursor_boundary: Option<Sequence>,
    pub artifact_root: [u8; 32],
    pub record_count: u64,
    pub encrypted: bool,
}

impl ExportBundleManifest {
    /// Binds a manifest to one exact canonical artifact and security policy.
    ///
    /// # Errors
    ///
    /// Rejects root/count mismatch or an unencrypted sensitive export policy.
    pub fn verify(
        &self,
        artifact: &CanonicalExport,
        require_encryption: bool,
    ) -> Result<(), ImportWorkflowError> {
        if self.artifact_root != artifact.root_digest || self.record_count != artifact.record_count
        {
            return Err(ImportWorkflowError::ExportManifestMismatch);
        }
        if require_encryption && !self.encrypted {
            return Err(ImportWorkflowError::ExportEncryptionRequired);
        }
        Ok(())
    }
}

/// Dry-run counts and resource estimate. Planning does not authorize writes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportPlan {
    pub source_records: u64,
    pub mapped_records: u64,
    pub quarantined_records: u64,
    pub estimated_target_bytes: u64,
    pub entity_order: Vec<CanonicalEntityId>,
    pub dry_run: bool,
}

/// Adapter persistence failure classified for exact-root retry.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("import workflow store ({transient}): {message}")]
pub struct ImportWorkflowStoreError {
    pub transient: bool,
    pub message: String,
}

/// Pure workflow validation failure.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ImportWorkflowError {
    #[error("bounded text is blank or too long")]
    InvalidText,
    #[error("version zero is unassigned")]
    ZeroVersion,
    #[error("illegal import job state transition")]
    InvalidStateTransition,
    #[error("job timestamp regressed")]
    TimeRegression,
    #[error("source fingerprint changed")]
    SourceFingerprintChanged,
    #[error("mapping or policy version changed")]
    PolicyChanged,
    #[error("checkpoint does not exactly follow committed target data")]
    CheckpointRegression,
    #[error("source key is not a valid UUID")]
    InvalidUuid,
    #[error("mapping-table strategy requires a durable mapping")]
    MissingPersistedMapping,
    #[error("source identity mapped to a different target")]
    IdentityMappingChanged,
    #[error("quarantine count is greater than total count")]
    InvalidCounts,
    #[error("quarantine percentage must be between zero and one hundred")]
    InvalidPercentage,
    #[error("import batch record bound violated")]
    BatchRecordLimit,
    #[error("import batch byte bound violated")]
    BatchByteLimit,
    #[error("import batch metadata serialization failed")]
    Serialization,
    #[error("dependency references an absent entity group")]
    DependencyMissing,
    #[error("entity dependency graph contains a cycle")]
    DependencyCycle,
    #[error("trusted entity version is required by policy")]
    MissingTrustedVersion,
    #[error("export manifest does not bind the supplied artifact")]
    ExportManifestMismatch,
    #[error("export security policy requires encryption")]
    ExportEncryptionRequired,
}

/// Reason cutover cannot safely activate a new authority writer.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum CutoverBlocker {
    #[error("source fingerprint changed")]
    SourceChanged,
    #[error("canonical, domain, reference, or scope verification failed")]
    VerificationFailed,
    #[error("legacy source catch-up lag is nonzero")]
    SourceLag,
    #[error("quarantine threshold is not accepted")]
    QuarantineThreshold,
    #[error("backup or rollback evidence is missing")]
    RecoveryEvidence,
    #[error("client bootstrap acceptance is missing")]
    ClientBootstrap,
    #[error("legacy writer is not fenced")]
    SplitBrainRisk,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn entity(value: u32) -> CanonicalEntityId {
        CanonicalEntityId::new(value).unwrap_or_else(|error| panic!("fixture failed: {error}"))
    }

    fn key(value: &str) -> SourceRecordKey {
        SourceRecordKey::new(value).unwrap_or_else(|error| panic!("fixture failed: {error}"))
    }

    fn fingerprint(content: &[u8]) -> SourceFingerprint {
        SourceFingerprint::calculate(
            &SourceSystemId::new("legacy-erp").unwrap_or_else(|error| panic!("{error}")),
            1,
            Some("snapshot-7".to_owned()),
            content,
        )
        .unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn job_state_machine_and_resume_guards_fail_closed() {
        let mut job = ImportJob::new(
            MigrationMode::SeedAuthority,
            SourceSystemId::new("legacy-erp").unwrap_or_else(|error| panic!("{error}")),
            fingerprint(b"stable"),
            2,
            3,
            10,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            job.transition(ImportJobState::Importing, 11),
            Err(ImportWorkflowError::InvalidStateTransition)
        );
        assert!(job.transition(ImportJobState::Scanning, 11).is_ok());
        assert_eq!(
            job.validate_resume(&fingerprint(b"changed"), 2, 3),
            Err(ImportWorkflowError::SourceFingerprintChanged)
        );
        assert_eq!(
            job.validate_resume(&fingerprint(b"stable"), 9, 3),
            Err(ImportWorkflowError::PolicyChanged)
        );
    }

    #[test]
    fn deterministic_identity_survives_restart_and_entity_namespace() {
        let namespace = Uuid::from_u128(7);
        let mut first = IdentityPlan::default();
        let first_id = first
            .assign(
                IdMappingStrategy::DeterministicNamespace,
                namespace,
                entity(1),
                key("student/42"),
                None,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let mut restarted = IdentityPlan::default();
        let restarted_id = restarted
            .assign(
                IdMappingStrategy::DeterministicNamespace,
                namespace,
                entity(1),
                key("student/42"),
                None,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let other_type = restarted
            .assign(
                IdMappingStrategy::DeterministicNamespace,
                namespace,
                entity(2),
                key("student/42"),
                None,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(first_id, restarted_id);
        assert_ne!(first_id, other_type);
    }

    #[test]
    fn checkpoint_advances_exactly_after_one_batch() {
        let current = ImportCheckpoint {
            committed_batches: 3,
            committed_records: 500,
            source_offset: 600,
            last_source_key: Some(key("600")),
            watermark: Some(
                MigrationWatermark::new("lsn/10").unwrap_or_else(|error| panic!("{error}")),
            ),
        };
        let mut next = current.clone();
        next.committed_batches = 4;
        next.committed_records = 700;
        next.source_offset = 800;
        next.last_source_key = Some(key("800"));
        next.watermark =
            Some(MigrationWatermark::new("lsn/11").unwrap_or_else(|error| panic!("{error}")));
        assert!(current.validate_successor(&next).is_ok());
        next.committed_batches = 6;
        assert_eq!(
            current.validate_successor(&next),
            Err(ImportWorkflowError::CheckpointRegression)
        );
    }

    #[test]
    fn quarantine_threshold_is_explicit_and_integer_safe() {
        assert_eq!(ImportStrictness::Strict.permits(100, 1), Ok(false));
        assert_eq!(
            ImportStrictness::Tolerant {
                max_quarantine_percent: 1
            }
            .permits(100, 1),
            Ok(true)
        );
        assert_eq!(
            ImportStrictness::Tolerant {
                max_quarantine_percent: 1
            }
            .permits(99, 1),
            Ok(false)
        );
    }

    #[test]
    fn dependency_plan_is_deterministic_and_rejects_cycles() {
        let groups = vec![
            ImportEntityGroup {
                entity: entity(3),
                depends_on: BTreeSet::from([entity(2)]),
            },
            ImportEntityGroup {
                entity: entity(1),
                depends_on: BTreeSet::new(),
            },
            ImportEntityGroup {
                entity: entity(2),
                depends_on: BTreeSet::from([entity(1)]),
            },
        ];
        assert_eq!(
            plan_entity_order(&groups),
            Ok(vec![entity(1), entity(2), entity(3)])
        );
        let cyclic = vec![
            ImportEntityGroup {
                entity: entity(1),
                depends_on: BTreeSet::from([entity(2)]),
            },
            ImportEntityGroup {
                entity: entity(2),
                depends_on: BTreeSet::from([entity(1)]),
            },
        ];
        assert_eq!(
            plan_entity_order(&cyclic),
            Err(ImportWorkflowError::DependencyCycle)
        );
    }

    #[test]
    fn baseline_defaults_to_initial_version_without_synthetic_history() {
        assert_eq!(
            SeedVersionPolicy::Initial.resolve(None),
            Ok(EntityVersion::INITIAL)
        );
        assert_eq!(
            SeedVersionPolicy::PreserveTrusted.resolve(None),
            Err(ImportWorkflowError::MissingTrustedVersion)
        );
        assert!(!AuthorityImportKind::SeedBaseline.requires_journal());
        assert!(AuthorityImportKind::ImportedOperations.requires_journal());
        assert!(AuthorityImportKind::LiveBridgeChange.requires_journal());
    }

    #[test]
    fn cutover_requires_zero_lag_verification_recovery_and_writer_fence() {
        let mut evidence = CutoverEvidence {
            fingerprint_matches: EvidenceCheck::Passed,
            canonical_root_matches: EvidenceCheck::Passed,
            domain_totals_match: EvidenceCheck::Passed,
            referential_integrity_valid: EvidenceCheck::Passed,
            scope_snapshots_verified: EvidenceCheck::Passed,
            source_lag: 1,
            quarantine_permitted: EvidenceCheck::Passed,
            backup_verified: EvidenceCheck::Passed,
            rollback_rehearsed: EvidenceCheck::Passed,
            client_bootstrap_verified: EvidenceCheck::Passed,
            legacy_writer_fenced: EvidenceCheck::Passed,
        };
        assert_eq!(verify_cutover(&evidence), Err(CutoverBlocker::SourceLag));
        evidence.source_lag = 0;
        evidence.legacy_writer_fenced = EvidenceCheck::Failed;
        assert_eq!(
            verify_cutover(&evidence),
            Err(CutoverBlocker::SplitBrainRisk)
        );
        evidence.legacy_writer_fenced = EvidenceCheck::Passed;
        assert_eq!(verify_cutover(&evidence), Ok(()));
    }

    #[test]
    fn job_round_trip_retains_exact_resume_guards() {
        let job = ImportJob::new(
            MigrationMode::LegacyBridge,
            SourceSystemId::new("source").unwrap_or_else(|error| panic!("{error}")),
            fingerprint(b"stable"),
            1,
            1,
            10,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let encoded = postcard::to_stdvec(&job).unwrap_or_else(|error| panic!("{error}"));
        let decoded: ImportJob =
            postcard::from_bytes(&encoded).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(decoded, job);
    }

    proptest! {
        #[test]
        fn deterministic_mapping_is_stable_for_generated_source_keys(
            source_key in "[a-zA-Z0-9/_-]{1,128}"
        ) {
            let namespace = Uuid::from_u128(99);
            let source_key = SourceRecordKey::new(source_key)
                .unwrap_or_else(|error| panic!("{error}"));
            let left = deterministic_entity_id(namespace, entity(1), &source_key);
            let right = deterministic_entity_id(namespace, entity(1), &source_key);
            prop_assert_eq!(left, right);
        }
    }
}
