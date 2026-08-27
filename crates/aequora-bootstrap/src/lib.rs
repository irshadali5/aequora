//! Durable, bounded, and transport-neutral large snapshot bootstrap contracts.
//!
//! This crate defines portable manifests, deterministic chunks, restart guards, snapshot leases,
//! staging generations, and atomic activation evidence. Database transactions, object storage,
//! authorization, and application payload interpretation stay in adapters.

use aequora_protocol::SnapshotEntity;
use aequora_scope::{ScopeGeneration, ScopeVersion};
pub use aequora_types::{AuthorityEpoch, AuthorityId};
use aequora_types::{OperationId, Sequence, SnapshotId, SyncScopeId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use uuid::Uuid;

/// Current portable large-bootstrap manifest format.
pub const MANIFEST_FORMAT_VERSION: u32 = 1;

/// Durable identity for one local bootstrap attempt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BootstrapJobId(Uuid);

impl BootstrapJobId {
    /// Creates an approximately time-ordered job identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for BootstrapJobId {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! nonzero_u64 {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a non-zero value.
            ///
            /// # Errors
            ///
            /// Returns [`BootstrapError::ZeroValue`] when `value` is zero.
            pub const fn new(value: u64) -> Result<Self, BootstrapError> {
                if value == 0 {
                    Err(BootstrapError::ZeroValue(stringify!($name)))
                } else {
                    Ok(Self(value))
                }
            }

            /// Returns the wire value.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

nonzero_u64!(
    /// Application projection schema understood by the snapshot payload codec.
    ProjectionSchemaVersion
);
nonzero_u64!(
    /// Monotonic local replica generation; staging and active data never share a generation.
    ReplicaGeneration
);
/// One immutable authoritative state boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotBoundary {
    pub authority_id: AuthorityId,
    pub scope_id: SyncScopeId,
    pub scope_version: ScopeVersion,
    pub scope_generation: ScopeGeneration,
    pub sequence: Sequence,
    pub authority_epoch: AuthorityEpoch,
}

/// Encoding applied to one chunk object.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CompressionKind {
    None,
    Zstd,
}

/// Stable content-bound chunk identity, independent from a temporary URL.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ChunkId(pub [u8; 32]);

/// Inclusive deterministic entity range represented by one chunk.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EntityRange {
    pub first: aequora_types::EntityRef,
    pub last: aequora_types::EntityRef,
}

/// Opaque, refreshable location selected by a transport adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ChunkLocation {
    Service,
    HttpRange { object_ref: String },
    ObjectStore { object_ref: String, via_cdn: bool },
}

/// One ordered, independently verifiable manifest entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChunkDescriptor {
    pub chunk_id: ChunkId,
    pub ordinal: u32,
    pub entity_range: EntityRange,
    pub record_count: u64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub digest: [u8; 32],
    pub compression: CompressionKind,
    pub location: ChunkLocation,
}

/// Immutable, versioned description of a complete snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotManifest {
    pub format_version: u32,
    pub snapshot_id: SnapshotId,
    pub boundary: SnapshotBoundary,
    pub schema_version: ProjectionSchemaVersion,
    pub record_count: u64,
    pub total_uncompressed_bytes: u64,
    pub total_compressed_bytes: u64,
    pub root_digest: [u8; 32],
    pub chunks: Vec<ChunkDescriptor>,
}

/// Bounded encoded chunk produced alongside a manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotChunk {
    pub descriptor: ChunkDescriptor,
    pub bytes: Vec<u8>,
}

/// Verified build result; adapters publish the manifest only after every chunk is durable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltSnapshot {
    pub manifest: SnapshotManifest,
    pub chunks: Vec<SnapshotChunk>,
}

/// Deterministic chunking and hard resource limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChunkingConfig {
    pub target_records: usize,
    pub target_uncompressed_bytes: usize,
    pub max_chunk_uncompressed_bytes: usize,
    pub max_chunks: usize,
    pub max_records: usize,
    pub max_total_uncompressed_bytes: usize,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            target_records: 1_000,
            target_uncompressed_bytes: 8 * 1024 * 1024,
            max_chunk_uncompressed_bytes: 32 * 1024 * 1024,
            max_chunks: 65_536,
            max_records: 10_000_000,
            max_total_uncompressed_bytes: 64 * 1024 * 1024 * 1024,
        }
    }
}

impl SnapshotManifest {
    /// Deterministically sorts, chunks, encodes, and hashes one consistent record set.
    ///
    /// # Errors
    ///
    /// Rejects invalid bounds, duplicate entities, serialization failures, and size overflow.
    pub fn build(
        snapshot_id: SnapshotId,
        boundary: SnapshotBoundary,
        schema_version: ProjectionSchemaVersion,
        records: &[SnapshotEntity],
        config: ChunkingConfig,
    ) -> Result<BuiltSnapshot, BootstrapError> {
        config.validate()?;
        if records.len() > config.max_records {
            return Err(BootstrapError::RecordLimit);
        }
        let mut ordered = records.to_vec();
        ordered.sort_by_key(|record| record.entity);
        for pair in ordered.windows(2) {
            if pair[0].entity == pair[1].entity {
                return Err(BootstrapError::DuplicateEntity);
            }
        }

        let mut chunks = Vec::new();
        let mut current = Vec::new();
        for record in ordered {
            current.push(record);
            let encoded = postcard::to_stdvec(&current)?;
            let reached_target = current.len() >= config.target_records
                || encoded.len() >= config.target_uncompressed_bytes;
            if encoded.len() > config.max_chunk_uncompressed_bytes {
                if current.len() == 1 {
                    return Err(BootstrapError::ChunkByteLimit);
                }
                let last = current.pop().ok_or(BootstrapError::InvalidState)?;
                chunks.push(build_chunk(snapshot_id, chunks.len(), &current)?);
                current = vec![last];
                if postcard::to_stdvec(&current)?.len() > config.max_chunk_uncompressed_bytes {
                    return Err(BootstrapError::ChunkByteLimit);
                }
            } else if reached_target {
                chunks.push(build_chunk(snapshot_id, chunks.len(), &current)?);
                current.clear();
            }
            if chunks.len() > config.max_chunks {
                return Err(BootstrapError::ChunkLimit);
            }
        }
        if !current.is_empty() {
            chunks.push(build_chunk(snapshot_id, chunks.len(), &current)?);
        }
        if chunks.len() > config.max_chunks {
            return Err(BootstrapError::ChunkLimit);
        }

        let record_count = u64::try_from(records.len()).map_err(|_| BootstrapError::Overflow)?;
        let total_uncompressed_bytes = chunks.iter().try_fold(0_u64, |total, chunk| {
            total
                .checked_add(chunk.descriptor.uncompressed_bytes)
                .ok_or(BootstrapError::Overflow)
        })?;
        let max_total = u64::try_from(config.max_total_uncompressed_bytes)
            .map_err(|_| BootstrapError::Overflow)?;
        if total_uncompressed_bytes > max_total {
            return Err(BootstrapError::TotalByteLimit);
        }
        let descriptors = chunks
            .iter()
            .map(|chunk| chunk.descriptor.clone())
            .collect::<Vec<_>>();
        let mut manifest = Self {
            format_version: MANIFEST_FORMAT_VERSION,
            snapshot_id,
            boundary,
            schema_version,
            record_count,
            total_uncompressed_bytes,
            total_compressed_bytes: total_uncompressed_bytes,
            root_digest: [0; 32],
            chunks: descriptors,
        };
        manifest.root_digest = manifest.calculate_root()?;
        Ok(BuiltSnapshot { manifest, chunks })
    }

    /// Verifies format, boundary identity, deterministic ordinals, totals, ranges, and root.
    ///
    /// # Errors
    ///
    /// Returns a typed failure for any manifest inconsistency.
    pub fn verify(&self) -> Result<(), BootstrapError> {
        if self.format_version != MANIFEST_FORMAT_VERSION {
            return Err(BootstrapError::UnsupportedManifestVersion);
        }
        let mut records = 0_u64;
        let mut compressed = 0_u64;
        let mut uncompressed = 0_u64;
        let mut previous_last = None;
        for (index, chunk) in self.chunks.iter().enumerate() {
            let ordinal = u32::try_from(index).map_err(|_| BootstrapError::Overflow)?;
            if chunk.ordinal != ordinal || chunk.record_count == 0 {
                return Err(BootstrapError::ChunkOrder);
            }
            if chunk.entity_range.first > chunk.entity_range.last
                || previous_last.is_some_and(|last| last >= chunk.entity_range.first)
            {
                return Err(BootstrapError::EntityOrder);
            }
            validate_location(&chunk.location)?;
            records = records
                .checked_add(chunk.record_count)
                .ok_or(BootstrapError::Overflow)?;
            compressed = compressed
                .checked_add(chunk.compressed_bytes)
                .ok_or(BootstrapError::Overflow)?;
            uncompressed = uncompressed
                .checked_add(chunk.uncompressed_bytes)
                .ok_or(BootstrapError::Overflow)?;
            previous_last = Some(chunk.entity_range.last);
        }
        if records != self.record_count
            || compressed != self.total_compressed_bytes
            || uncompressed != self.total_uncompressed_bytes
        {
            return Err(BootstrapError::ManifestTotals);
        }
        if self.calculate_root()? != self.root_digest {
            return Err(BootstrapError::RootMismatch);
        }
        Ok(())
    }

    fn calculate_root(&self) -> Result<[u8; 32], BootstrapError> {
        let encoded = postcard::to_stdvec(&(
            self.format_version,
            self.snapshot_id,
            self.boundary,
            self.schema_version,
            self.record_count,
            self.total_uncompressed_bytes,
            self.total_compressed_bytes,
            &self.chunks,
        ))?;
        Ok(*blake3::hash(&encoded).as_bytes())
    }
}

impl SnapshotChunk {
    /// Verifies exact identity, size, hash, ordering, and record count before installation.
    ///
    /// # Errors
    ///
    /// Returns a typed error without exposing record payloads.
    pub fn decode_verified(
        &self,
        expected: &ChunkDescriptor,
        max_uncompressed_bytes: usize,
    ) -> Result<Vec<SnapshotEntity>, BootstrapError> {
        if &self.descriptor != expected {
            return Err(BootstrapError::ChunkIdentityMismatch);
        }
        let expected_bytes =
            usize::try_from(expected.uncompressed_bytes).map_err(|_| BootstrapError::Overflow)?;
        if self.bytes.len() != expected_bytes || self.bytes.len() > max_uncompressed_bytes {
            return Err(BootstrapError::ChunkByteLimit);
        }
        if *blake3::hash(&self.bytes).as_bytes() != expected.digest {
            return Err(BootstrapError::ChunkDigestMismatch);
        }
        let records: Vec<SnapshotEntity> = postcard::from_bytes(&self.bytes)?;
        let count = u64::try_from(records.len()).map_err(|_| BootstrapError::Overflow)?;
        if count != expected.record_count {
            return Err(BootstrapError::ChunkRecordCount);
        }
        let first = records.first().ok_or(BootstrapError::ChunkRecordCount)?;
        let last = records.last().ok_or(BootstrapError::ChunkRecordCount)?;
        if first.entity != expected.entity_range.first || last.entity != expected.entity_range.last
        {
            return Err(BootstrapError::EntityOrder);
        }
        if records
            .windows(2)
            .any(|pair| pair[0].entity >= pair[1].entity)
        {
            return Err(BootstrapError::EntityOrder);
        }
        Ok(records)
    }
}

fn build_chunk(
    snapshot_id: SnapshotId,
    ordinal: usize,
    records: &[SnapshotEntity],
) -> Result<SnapshotChunk, BootstrapError> {
    let bytes = postcard::to_stdvec(records)?;
    let digest = *blake3::hash(&bytes).as_bytes();
    let ordinal = u32::try_from(ordinal).map_err(|_| BootstrapError::Overflow)?;
    let first = records.first().ok_or(BootstrapError::ChunkRecordCount)?;
    let last = records.last().ok_or(BootstrapError::ChunkRecordCount)?;
    let mut identity = blake3::Hasher::new();
    identity.update(snapshot_id.as_uuid().as_bytes());
    identity.update(&ordinal.to_le_bytes());
    identity.update(&digest);
    let byte_count = u64::try_from(bytes.len()).map_err(|_| BootstrapError::Overflow)?;
    let record_count = u64::try_from(records.len()).map_err(|_| BootstrapError::Overflow)?;
    let descriptor = ChunkDescriptor {
        chunk_id: ChunkId(*identity.finalize().as_bytes()),
        ordinal,
        entity_range: EntityRange {
            first: first.entity,
            last: last.entity,
        },
        record_count,
        compressed_bytes: byte_count,
        uncompressed_bytes: byte_count,
        digest,
        compression: CompressionKind::None,
        location: ChunkLocation::Service,
    };
    Ok(SnapshotChunk { descriptor, bytes })
}

impl ChunkingConfig {
    fn validate(self) -> Result<(), BootstrapError> {
        if self.target_records == 0
            || self.target_uncompressed_bytes == 0
            || self.max_chunk_uncompressed_bytes == 0
            || self.max_chunks == 0
            || self.max_records == 0
            || self.max_total_uncompressed_bytes == 0
            || self.target_uncompressed_bytes > self.max_chunk_uncompressed_bytes
        {
            return Err(BootstrapError::InvalidLimits);
        }
        Ok(())
    }
}

fn validate_location(location: &ChunkLocation) -> Result<(), BootstrapError> {
    match location {
        ChunkLocation::Service => Ok(()),
        ChunkLocation::HttpRange { object_ref } | ChunkLocation::ObjectStore { object_ref, .. }
            if !object_ref.trim().is_empty() =>
        {
            Ok(())
        }
        ChunkLocation::HttpRange { .. } | ChunkLocation::ObjectStore { .. } => {
            Err(BootstrapError::BlankObjectReference)
        }
    }
}

/// Durable workflow states. Only the declared transition graph is accepted.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BootstrapState {
    Requested,
    Planning,
    Downloading,
    Installing,
    Verifying,
    ReadyToActivate,
    Activating,
    CatchingUp,
    Complete,
    Failed,
    Quarantined,
    Cancelled,
}

impl BootstrapState {
    fn may_transition(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Requested | Self::Failed | Self::Cancelled,
                Self::Planning
            ) | (Self::Planning, Self::Downloading)
                | (Self::Downloading, Self::Installing | Self::Cancelled)
                | (
                    Self::Installing,
                    Self::Downloading | Self::Verifying | Self::Cancelled
                )
                | (Self::Verifying, Self::ReadyToActivate | Self::Quarantined)
                | (Self::ReadyToActivate, Self::Activating | Self::Cancelled)
                | (Self::Activating, Self::CatchingUp)
                | (Self::CatchingUp, Self::Complete)
                | (
                    Self::Requested
                        | Self::Planning
                        | Self::Downloading
                        | Self::Installing
                        | Self::Verifying
                        | Self::ReadyToActivate
                        | Self::Activating
                        | Self::CatchingUp,
                    Self::Failed
                )
        )
    }
}

/// Durable state for one independently retryable chunk.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ChunkState {
    NotStarted,
    Downloading,
    Downloaded,
    Verified,
    Installed,
}

/// Persisted resume metadata; object identity prevents cross-object range append.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChunkProgress {
    pub chunk_id: ChunkId,
    pub ordinal: u32,
    pub state: ChunkState,
    pub downloaded_bytes: u64,
    pub object_identity: Option<String>,
    pub attempts: u32,
}

/// Policy for local mutations while a replacement generation is staged.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BootstrapMutationPolicy {
    AllowQueue,
    ReadOnly,
    Custom,
}

/// Payload-free commitment to pending intent preserved across activation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PendingIntentPlan {
    pub operation_ids: Vec<OperationId>,
    pub immutable_sent: BTreeSet<OperationId>,
    pub digest: [u8; 32],
}

impl PendingIntentPlan {
    /// Sorts identities, rejects duplicate declarations, and calculates a restart-stable digest.
    ///
    /// # Errors
    ///
    /// Returns an error when a sent identity is absent or an identity is duplicated.
    pub fn build(
        mut operation_ids: Vec<OperationId>,
        immutable_sent: BTreeSet<OperationId>,
    ) -> Result<Self, BootstrapError> {
        operation_ids.sort_unstable();
        if operation_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(BootstrapError::DuplicatePendingIntent);
        }
        if !immutable_sent
            .iter()
            .all(|operation| operation_ids.binary_search(operation).is_ok())
        {
            return Err(BootstrapError::UnknownSentIntent);
        }
        let encoded = postcard::to_stdvec(&(&operation_ids, &immutable_sent))?;
        Ok(Self {
            operation_ids,
            immutable_sent,
            digest: *blake3::hash(&encoded).as_bytes(),
        })
    }
}

/// Complete durable job record; manifest identity and scope cannot drift on resume.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BootstrapJob {
    pub job_id: BootstrapJobId,
    pub snapshot_id: SnapshotId,
    pub manifest_root: [u8; 32],
    pub boundary: SnapshotBoundary,
    pub schema_version: ProjectionSchemaVersion,
    pub staging_generation: ReplicaGeneration,
    pub mutation_policy: BootstrapMutationPolicy,
    pub pending_intent_digest: [u8; 32],
    pub state: BootstrapState,
    pub chunks: BTreeMap<u32, ChunkProgress>,
}

impl BootstrapJob {
    /// Creates a job whose per-chunk progress exactly matches a verified manifest.
    ///
    /// # Errors
    ///
    /// Rejects an invalid manifest.
    pub fn new(
        manifest: &SnapshotManifest,
        staging_generation: ReplicaGeneration,
        mutation_policy: BootstrapMutationPolicy,
        pending: &PendingIntentPlan,
    ) -> Result<Self, BootstrapError> {
        manifest.verify()?;
        let chunks = manifest
            .chunks
            .iter()
            .map(|chunk| {
                (
                    chunk.ordinal,
                    ChunkProgress {
                        chunk_id: chunk.chunk_id,
                        ordinal: chunk.ordinal,
                        state: ChunkState::NotStarted,
                        downloaded_bytes: 0,
                        object_identity: None,
                        attempts: 0,
                    },
                )
            })
            .collect();
        Ok(Self {
            job_id: BootstrapJobId::new(),
            snapshot_id: manifest.snapshot_id,
            manifest_root: manifest.root_digest,
            boundary: manifest.boundary,
            schema_version: manifest.schema_version,
            staging_generation,
            mutation_policy,
            pending_intent_digest: pending.digest,
            state: BootstrapState::Requested,
            chunks,
        })
    }

    /// Fails closed when the durable state machine is asked to skip a phase.
    ///
    /// # Errors
    ///
    /// Returns [`BootstrapError::IllegalTransition`] for an undeclared edge.
    pub fn transition(&mut self, next: BootstrapState) -> Result<(), BootstrapError> {
        if !self.state.may_transition(next) {
            return Err(BootstrapError::IllegalTransition {
                from: self.state,
                to: next,
            });
        }
        self.state = next;
        Ok(())
    }

    /// Verifies immutable resume identity and exact manifest chunk membership.
    ///
    /// # Errors
    ///
    /// Rejects a changed snapshot, boundary, schema, root, or chunk map.
    pub fn validate_resume(&self, manifest: &SnapshotManifest) -> Result<(), BootstrapError> {
        manifest.verify()?;
        if self.snapshot_id != manifest.snapshot_id
            || self.manifest_root != manifest.root_digest
            || self.boundary != manifest.boundary
            || self.schema_version != manifest.schema_version
            || self.chunks.len() != manifest.chunks.len()
            || manifest.chunks.iter().any(|chunk| {
                self.chunks
                    .get(&chunk.ordinal)
                    .is_none_or(|progress| progress.chunk_id != chunk.chunk_id)
            })
        {
            return Err(BootstrapError::ResumeIdentityMismatch);
        }
        Ok(())
    }

    /// Records one bounded range response while preserving immutable object identity.
    ///
    /// The returned progress must be persisted before requesting the next range.
    ///
    /// # Errors
    ///
    /// Rejects an unknown chunk, identity drift, non-contiguous offset, overflow, or premature
    /// completion.
    pub fn record_chunk_read(
        &mut self,
        descriptor: &ChunkDescriptor,
        read: &ChunkRead,
    ) -> Result<ChunkProgress, BootstrapError> {
        if read.object_identity.trim().is_empty() {
            return Err(BootstrapError::BlankObjectIdentity);
        }
        let progress = self
            .chunks
            .get_mut(&descriptor.ordinal)
            .ok_or(BootstrapError::UnknownChunk)?;
        if progress.chunk_id != descriptor.chunk_id
            || progress
                .object_identity
                .as_ref()
                .is_some_and(|identity| identity != &read.object_identity)
        {
            return Err(BootstrapError::RangeResumeMismatch);
        }
        if !matches!(
            progress.state,
            ChunkState::NotStarted | ChunkState::Downloading
        ) {
            return Err(BootstrapError::InvalidChunkState);
        }
        let bytes = u64::try_from(read.bytes.len()).map_err(|_| BootstrapError::Overflow)?;
        let expected_next = progress
            .downloaded_bytes
            .checked_add(bytes)
            .ok_or(BootstrapError::Overflow)?;
        if read.next_offset != expected_next || read.next_offset > descriptor.compressed_bytes {
            return Err(BootstrapError::RangeResumeMismatch);
        }
        if read.complete != (read.next_offset == descriptor.compressed_bytes) {
            return Err(BootstrapError::RangeCompletionMismatch);
        }
        progress.downloaded_bytes = read.next_offset;
        progress.object_identity = Some(read.object_identity.clone());
        progress.attempts = progress.attempts.saturating_add(1);
        progress.state = if read.complete {
            ChunkState::Downloaded
        } else {
            ChunkState::Downloading
        };
        Ok(progress.clone())
    }

    /// Advances an exactly downloaded chunk after independent content verification.
    ///
    /// # Errors
    ///
    /// Rejects unknown chunks and phase skipping.
    pub fn mark_chunk_verified(
        &mut self,
        descriptor: &ChunkDescriptor,
    ) -> Result<(), BootstrapError> {
        self.advance_chunk(descriptor, ChunkState::Downloaded, ChunkState::Verified)
    }

    /// Advances a verified chunk only after the adapter atomically installs records and progress.
    ///
    /// # Errors
    ///
    /// Rejects unknown chunks and phase skipping.
    pub fn mark_chunk_installed(
        &mut self,
        descriptor: &ChunkDescriptor,
    ) -> Result<(), BootstrapError> {
        self.advance_chunk(descriptor, ChunkState::Verified, ChunkState::Installed)
    }

    fn advance_chunk(
        &mut self,
        descriptor: &ChunkDescriptor,
        expected: ChunkState,
        next: ChunkState,
    ) -> Result<(), BootstrapError> {
        let progress = self
            .chunks
            .get_mut(&descriptor.ordinal)
            .ok_or(BootstrapError::UnknownChunk)?;
        if progress.chunk_id != descriptor.chunk_id || progress.state != expected {
            return Err(BootstrapError::InvalidChunkState);
        }
        progress.state = next;
        Ok(())
    }

    /// Returns payload-free progress for status UIs and telemetry.
    #[must_use]
    pub fn status(&self) -> BootstrapStatus {
        let chunks_complete = self
            .chunks
            .values()
            .filter(|progress| progress.state == ChunkState::Installed)
            .count();
        BootstrapStatus {
            state: self.state,
            scope_id: self.boundary.scope_id,
            bytes_downloaded: self
                .chunks
                .values()
                .map(|progress| progress.downloaded_bytes)
                .sum(),
            chunks_complete,
            chunks_total: self.chunks.len(),
        }
    }
}

/// Payload-free job status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootstrapStatus {
    pub state: BootstrapState,
    pub scope_id: SyncScopeId,
    pub bytes_downloaded: u64,
    pub chunks_complete: usize,
    pub chunks_total: usize,
}

/// Stable identity required before appending bytes to a partial chunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RangeResumeGuard {
    pub snapshot_id: SnapshotId,
    pub chunk_id: ChunkId,
    pub object_identity: String,
    pub downloaded_bytes: u64,
}

impl RangeResumeGuard {
    /// Verifies a range response still belongs to the same immutable object and stays in bounds.
    ///
    /// # Errors
    ///
    /// Rejects object drift, wrong chunks, blank identities, or offsets past the object.
    pub fn verify(
        &self,
        snapshot_id: SnapshotId,
        descriptor: &ChunkDescriptor,
        object_identity: &str,
    ) -> Result<(), BootstrapError> {
        if object_identity.trim().is_empty() {
            return Err(BootstrapError::BlankObjectIdentity);
        }
        if self.snapshot_id != snapshot_id
            || self.chunk_id != descriptor.chunk_id
            || self.object_identity != object_identity
            || self.downloaded_bytes > descriptor.compressed_bytes
        {
            return Err(BootstrapError::RangeResumeMismatch);
        }
        Ok(())
    }
}

/// Server guarantee that the journal delta after a snapshot boundary remains readable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotLease {
    pub snapshot_id: SnapshotId,
    pub boundary_sequence: Sequence,
    pub retained_from: Sequence,
    pub expires_at_unix_ms: u64,
}

impl SnapshotLease {
    /// Verifies identity, time, and retained journal coverage.
    ///
    /// # Errors
    ///
    /// Rejects expired leases or a journal floor beyond the first required delta.
    pub fn verify(
        self,
        snapshot_id: SnapshotId,
        boundary: SnapshotBoundary,
        now_unix_ms: u64,
    ) -> Result<(), BootstrapError> {
        let first_delta = boundary
            .sequence
            .0
            .checked_add(1)
            .ok_or(BootstrapError::Overflow)?;
        if self.snapshot_id != snapshot_id || self.boundary_sequence != boundary.sequence {
            return Err(BootstrapError::LeaseIdentityMismatch);
        }
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err(BootstrapError::LeaseExpired);
        }
        if self.retained_from.0 > first_delta {
            return Err(BootstrapError::JournalGap);
        }
        Ok(())
    }
}

/// Disk and memory budgets checked before transfer begins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootstrapPreflight {
    pub available_disk_bytes: u64,
    pub reserved_disk_bytes: u64,
    pub max_memory_bytes: u64,
    pub ready_queue_bytes: u64,
    pub max_chunk_uncompressed_bytes: u64,
}

impl BootstrapPreflight {
    /// Proves the manifest and bounded pipeline fit declared resources without overflow.
    ///
    /// # Errors
    ///
    /// Returns a disk or memory budget failure.
    pub fn verify(self, manifest: &SnapshotManifest) -> Result<(), BootstrapError> {
        let required_disk = manifest
            .total_compressed_bytes
            .checked_add(manifest.total_uncompressed_bytes)
            .and_then(|value| value.checked_add(self.reserved_disk_bytes))
            .ok_or(BootstrapError::Overflow)?;
        if required_disk > self.available_disk_bytes {
            return Err(BootstrapError::InsufficientDisk);
        }
        let required_memory = self
            .ready_queue_bytes
            .checked_add(self.max_chunk_uncompressed_bytes)
            .ok_or(BootstrapError::Overflow)?;
        if required_memory > self.max_memory_bytes {
            return Err(BootstrapError::InsufficientMemory);
        }
        if manifest
            .chunks
            .iter()
            .any(|chunk| chunk.uncompressed_bytes > self.max_chunk_uncompressed_bytes)
        {
            return Err(BootstrapError::ChunkByteLimit);
        }
        Ok(())
    }
}

/// Complete fail-closed evidence required by the atomic activation transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivationEvidence {
    pub manifest_root: [u8; 32],
    pub installed_chunks: BTreeSet<ChunkId>,
    pub current_scope_version: ScopeVersion,
    pub current_scope_generation: ScopeGeneration,
    pub current_authority_epoch: AuthorityEpoch,
    pub authorization_current: bool,
    pub staging_verified: bool,
    pub pending_intent_digest: [u8; 32],
    pub lease: SnapshotLease,
}

/// Validated activation token passed to an adapter transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifiedActivation {
    pub job_id: BootstrapJobId,
    pub snapshot_id: SnapshotId,
    pub staging_generation: ReplicaGeneration,
    pub cursor_sequence: Sequence,
    pub pending_intent_digest: [u8; 32],
}

/// Checks every activation guard without mutating local state.
///
/// # Errors
///
/// Returns a typed blocker when authorization, scope, epoch, lease, manifest, chunks, staging, or
/// pending-intent identity changed.
pub fn verify_activation(
    job: &BootstrapJob,
    manifest: &SnapshotManifest,
    evidence: &ActivationEvidence,
    now_unix_ms: u64,
) -> Result<VerifiedActivation, BootstrapError> {
    job.validate_resume(manifest)?;
    if job.state != BootstrapState::ReadyToActivate {
        return Err(BootstrapError::NotReadyToActivate);
    }
    evidence
        .lease
        .verify(job.snapshot_id, job.boundary, now_unix_ms)?;
    let expected_chunks = manifest
        .chunks
        .iter()
        .map(|chunk| chunk.chunk_id)
        .collect::<BTreeSet<_>>();
    if evidence.manifest_root != job.manifest_root {
        return Err(BootstrapError::RootMismatch);
    }
    if evidence.installed_chunks != expected_chunks
        || job
            .chunks
            .values()
            .any(|progress| progress.state != ChunkState::Installed)
    {
        return Err(BootstrapError::IncompleteStaging);
    }
    if !evidence.authorization_current {
        return Err(BootstrapError::AuthorizationRevoked);
    }
    if evidence.current_scope_version != job.boundary.scope_version
        || evidence.current_scope_generation != job.boundary.scope_generation
    {
        return Err(BootstrapError::ScopeChanged);
    }
    if evidence.current_authority_epoch != job.boundary.authority_epoch {
        return Err(BootstrapError::AuthorityEpochChanged);
    }
    if !evidence.staging_verified {
        return Err(BootstrapError::StagingNotVerified);
    }
    if evidence.pending_intent_digest != job.pending_intent_digest {
        return Err(BootstrapError::PendingIntentChanged);
    }
    Ok(VerifiedActivation {
        job_id: job.job_id,
        snapshot_id: job.snapshot_id,
        staging_generation: job.staging_generation,
        cursor_sequence: job.boundary.sequence,
        pending_intent_digest: job.pending_intent_digest,
    })
}

/// One bounded read from a service, HTTP range endpoint, object store, or CDN.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkRead {
    pub bytes: Vec<u8>,
    pub next_offset: u64,
    pub complete: bool,
    pub object_identity: String,
}

/// Request used to capture one database-neutral consistent read view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotRequest {
    pub scope_id: SyncScopeId,
    pub scope_version: ScopeVersion,
    pub scope_generation: ScopeGeneration,
    pub schema_version: ProjectionSchemaVersion,
}

/// Open consistent database view from which bounded canonical snapshot entities are streamed.
#[async_trait]
pub trait SnapshotReadView: Send {
    fn snapshot_id(&self) -> SnapshotId;
    fn boundary(&self) -> SnapshotBoundary;
    async fn next_records(
        &mut self,
        max_records: usize,
        max_uncompressed_bytes: usize,
    ) -> Result<Vec<SnapshotEntity>, BootstrapStoreError>;
}

/// Authority adapter capable of opening a consistent, scoped snapshot read view.
#[async_trait]
pub trait SnapshotSource: Send + Sync {
    async fn open_snapshot(
        &self,
        request: SnapshotRequest,
    ) -> Result<Box<dyn SnapshotReadView>, BootstrapStoreError>;
}

/// Transfer adapter for manifests and bounded/ranged chunk reads.
#[async_trait]
pub trait SnapshotChunkSource: Send + Sync {
    async fn manifest(
        &self,
        snapshot_id: SnapshotId,
    ) -> Result<SnapshotManifest, BootstrapStoreError>;

    async fn read_chunk(
        &self,
        snapshot_id: SnapshotId,
        chunk: &ChunkDescriptor,
        offset: u64,
        max_bytes: usize,
    ) -> Result<ChunkRead, BootstrapStoreError>;
}

/// Local adapter contract for bounded staging and one atomic logical generation swap.
#[async_trait]
pub trait SnapshotSink: Send + Sync {
    async fn begin_staging(
        &self,
        job: &BootstrapJob,
        manifest: &SnapshotManifest,
        pending: &PendingIntentPlan,
    ) -> Result<(), BootstrapStoreError>;

    /// Installs one verified chunk and its durable progress in the same bounded transaction.
    async fn install_chunk(
        &self,
        job: &BootstrapJob,
        descriptor: &ChunkDescriptor,
        records: &[SnapshotEntity],
    ) -> Result<(), BootstrapStoreError>;

    async fn verify_staging(
        &self,
        job: &BootstrapJob,
        manifest: &SnapshotManifest,
    ) -> Result<(), BootstrapStoreError>;

    /// Atomically activates the complete staging generation, boundary cursor, and pending intent.
    async fn activate(
        &self,
        activation: VerifiedActivation,
    ) -> Result<ActivationOutcome, BootstrapStoreError>;

    /// Removes or seals staged unauthorized data after revocation. Active data is unaffected.
    async fn quarantine_revoked(&self, job: &BootstrapJob) -> Result<(), BootstrapStoreError>;
}

/// Authority adapter contract for journal-retention lease acquisition and renewal.
#[async_trait]
pub trait SnapshotLeaseStore: Send + Sync {
    async fn acquire(
        &self,
        snapshot_id: SnapshotId,
        boundary: SnapshotBoundary,
    ) -> Result<SnapshotLease, BootstrapStoreError>;

    async fn renew(&self, lease: SnapshotLease) -> Result<SnapshotLease, BootstrapStoreError>;
}

/// Result of the one atomic activation transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivationOutcome {
    pub active_generation: ReplicaGeneration,
    pub cursor_sequence: Sequence,
    pub preserved_pending_operations: usize,
}

/// Adapter capability statement used by deployment certification.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SnapshotInstallFeature {
    GenerationSwap,
    ResumableChunkInstall,
    ConsistentRead,
    PreservesPendingIntent,
}

/// Explicit local adapter snapshot capabilities.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SnapshotInstallCapability {
    pub features: BTreeSet<SnapshotInstallFeature>,
}

impl SnapshotInstallCapability {
    /// Tier-A requires every crash-safety capability.
    #[must_use]
    pub fn is_tier_a(&self) -> bool {
        [
            SnapshotInstallFeature::GenerationSwap,
            SnapshotInstallFeature::ResumableChunkInstall,
            SnapshotInstallFeature::ConsistentRead,
            SnapshotInstallFeature::PreservesPendingIntent,
        ]
        .iter()
        .all(|feature| self.features.contains(feature))
    }
}

/// Portable workflow validation failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum BootstrapError {
    #[error("{0} must be non-zero")]
    ZeroValue(&'static str),
    #[error("bootstrap limits are invalid")]
    InvalidLimits,
    #[error("snapshot record limit exceeded")]
    RecordLimit,
    #[error("snapshot chunk limit exceeded")]
    ChunkLimit,
    #[error("snapshot total-byte limit exceeded")]
    TotalByteLimit,
    #[error("snapshot chunk byte limit exceeded")]
    ChunkByteLimit,
    #[error("snapshot contains a duplicate entity")]
    DuplicateEntity,
    #[error("snapshot manifest version is unsupported")]
    UnsupportedManifestVersion,
    #[error("snapshot chunk ordering is invalid")]
    ChunkOrder,
    #[error("snapshot entity ordering is invalid")]
    EntityOrder,
    #[error("snapshot manifest totals do not match descriptors")]
    ManifestTotals,
    #[error("snapshot root digest does not match")]
    RootMismatch,
    #[error("snapshot chunk identity changed")]
    ChunkIdentityMismatch,
    #[error("snapshot chunk digest does not match")]
    ChunkDigestMismatch,
    #[error("snapshot chunk record count does not match")]
    ChunkRecordCount,
    #[error("bootstrap state is invalid")]
    InvalidState,
    #[error("bootstrap chunk is unknown")]
    UnknownChunk,
    #[error("bootstrap chunk phase is invalid")]
    InvalidChunkState,
    #[error("illegal bootstrap transition from {from:?} to {to:?}")]
    IllegalTransition {
        from: BootstrapState,
        to: BootstrapState,
    },
    #[error("bootstrap resume identity changed")]
    ResumeIdentityMismatch,
    #[error("range resume object identity changed")]
    RangeResumeMismatch,
    #[error("range response completion marker does not match its offset")]
    RangeCompletionMismatch,
    #[error("range resume object identity is blank")]
    BlankObjectIdentity,
    #[error("chunk object reference is blank")]
    BlankObjectReference,
    #[error("pending operation identity is duplicated")]
    DuplicatePendingIntent,
    #[error("sent operation is absent from the pending plan")]
    UnknownSentIntent,
    #[error("snapshot lease identity changed")]
    LeaseIdentityMismatch,
    #[error("snapshot lease expired")]
    LeaseExpired,
    #[error("required post-snapshot journal history is unavailable")]
    JournalGap,
    #[error("insufficient disk for bounded bootstrap")]
    InsufficientDisk,
    #[error("insufficient memory for bounded bootstrap")]
    InsufficientMemory,
    #[error("bootstrap is not ready to activate")]
    NotReadyToActivate,
    #[error("staging does not contain every required chunk")]
    IncompleteStaging,
    #[error("scope authorization was revoked")]
    AuthorizationRevoked,
    #[error("scope version or generation changed")]
    ScopeChanged,
    #[error("authority epoch changed")]
    AuthorityEpochChanged,
    #[error("staging verification is incomplete")]
    StagingNotVerified,
    #[error("pending intent changed during bootstrap")]
    PendingIntentChanged,
    #[error("integer or size overflow")]
    Overflow,
    #[error("snapshot serialization failed")]
    Codec,
}

impl From<postcard::Error> for BootstrapError {
    fn from(_: postcard::Error) -> Self {
        Self::Codec
    }
}

/// Payload-free adapter failure classification.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("bootstrap store {kind:?}: {message}")]
pub struct BootstrapStoreError {
    pub kind: BootstrapStoreErrorKind,
    pub message: String,
}

/// Retry semantics for adapter failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapStoreErrorKind {
    Transient,
    Permanent,
    Conflict,
    Unauthorized,
    Capacity,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_types::{EntityId, EntityRef, EntityType, EntityVersion};
    use proptest::prelude::*;

    fn boundary() -> SnapshotBoundary {
        SnapshotBoundary {
            authority_id: AuthorityId::from_uuid(Uuid::from_u128(1)),
            scope_id: SyncScopeId::from_uuid(Uuid::from_u128(11)),
            scope_version: ScopeVersion::INITIAL,
            scope_generation: ScopeGeneration::INITIAL,
            sequence: Sequence(41),
            authority_epoch: AuthorityEpoch::new(1).unwrap_or_else(|error| panic!("{error}")),
        }
    }

    fn record(id: u128, payload_size: usize) -> SnapshotEntity {
        SnapshotEntity {
            entity: EntityRef {
                entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
                entity_id: EntityId::from_uuid(Uuid::from_u128(id)),
            },
            version: EntityVersion::INITIAL,
            payload: vec![7; payload_size],
            tombstone: false,
        }
    }

    fn built(records: &[SnapshotEntity]) -> BuiltSnapshot {
        SnapshotManifest::build(
            SnapshotId::from_uuid(Uuid::from_u128(77)),
            boundary(),
            ProjectionSchemaVersion::new(1).unwrap_or_else(|error| panic!("{error}")),
            records,
            ChunkingConfig {
                target_records: 2,
                target_uncompressed_bytes: 1_024,
                max_chunk_uncompressed_bytes: 4_096,
                max_chunks: 32,
                max_records: 100,
                max_total_uncompressed_bytes: 65_536,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn deterministic_manifest_is_order_independent_and_chunks_verify() {
        let one = built(&[record(3, 8), record(1, 8), record(2, 8)]);
        let two = built(&[record(2, 8), record(3, 8), record(1, 8)]);
        assert_eq!(one.manifest, two.manifest);
        assert_eq!(one.chunks, two.chunks);
        one.manifest
            .verify()
            .unwrap_or_else(|error| panic!("{error}"));
        for chunk in &one.chunks {
            let decoded = chunk
                .decode_verified(&chunk.descriptor, 4_096)
                .unwrap_or_else(|error| panic!("{error}"));
            assert!(!decoded.is_empty());
        }
    }

    #[test]
    fn manifest_and_chunk_tampering_fail_closed() {
        let mut snapshot = built(&[record(1, 8), record(2, 8)]);
        snapshot.manifest.record_count += 1;
        assert_eq!(
            snapshot.manifest.verify(),
            Err(BootstrapError::ManifestTotals)
        );
        let mut chunk = snapshot.chunks.remove(0);
        chunk.bytes[0] ^= 1;
        assert_eq!(
            chunk.decode_verified(&chunk.descriptor, 4_096),
            Err(BootstrapError::ChunkDigestMismatch)
        );
    }

    #[test]
    fn state_machine_and_resume_identity_are_fail_closed() {
        let snapshot = built(&[record(1, 8)]);
        let pending = PendingIntentPlan::build(Vec::new(), BTreeSet::new())
            .unwrap_or_else(|error| panic!("{error}"));
        let mut job = BootstrapJob::new(
            &snapshot.manifest,
            ReplicaGeneration::new(2).unwrap_or_else(|error| panic!("{error}")),
            BootstrapMutationPolicy::AllowQueue,
            &pending,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert!(job.transition(BootstrapState::Activating).is_err());
        job.transition(BootstrapState::Planning)
            .unwrap_or_else(|error| panic!("{error}"));
        job.validate_resume(&snapshot.manifest)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut changed = snapshot.manifest.clone();
        changed.root_digest[0] ^= 1;
        assert!(job.validate_resume(&changed).is_err());
    }

    #[test]
    fn range_resume_requires_the_same_immutable_object() {
        let snapshot = built(&[record(1, 8)]);
        let descriptor = &snapshot.manifest.chunks[0];
        let guard = RangeResumeGuard {
            snapshot_id: snapshot.manifest.snapshot_id,
            chunk_id: descriptor.chunk_id,
            object_identity: "etag-v1".to_owned(),
            downloaded_bytes: 3,
        };
        guard
            .verify(snapshot.manifest.snapshot_id, descriptor, "etag-v1")
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            guard.verify(snapshot.manifest.snapshot_id, descriptor, "etag-v2"),
            Err(BootstrapError::RangeResumeMismatch)
        );

        let pending = PendingIntentPlan::build(Vec::new(), BTreeSet::new())
            .unwrap_or_else(|error| panic!("{error}"));
        let mut job = BootstrapJob::new(
            &snapshot.manifest,
            ReplicaGeneration::new(2).unwrap_or_else(|error| panic!("{error}")),
            BootstrapMutationPolicy::AllowQueue,
            &pending,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let first = ChunkRead {
            bytes: snapshot.chunks[0].bytes[..3].to_vec(),
            next_offset: 3,
            complete: false,
            object_identity: "etag-v1".to_owned(),
        };
        assert_eq!(
            job.record_chunk_read(descriptor, &first)
                .unwrap_or_else(|error| panic!("{error}"))
                .state,
            ChunkState::Downloading
        );
        let rest = ChunkRead {
            bytes: snapshot.chunks[0].bytes[3..].to_vec(),
            next_offset: descriptor.compressed_bytes,
            complete: true,
            object_identity: "etag-v1".to_owned(),
        };
        assert_eq!(
            job.record_chunk_read(descriptor, &rest)
                .unwrap_or_else(|error| panic!("{error}"))
                .state,
            ChunkState::Downloaded
        );
        job.mark_chunk_verified(descriptor)
            .unwrap_or_else(|error| panic!("{error}"));
        job.mark_chunk_installed(descriptor)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn lease_preflight_and_activation_require_complete_current_evidence() {
        let snapshot = built(&[record(1, 8), record(2, 8)]);
        BootstrapPreflight {
            available_disk_bytes: 1_000_000,
            reserved_disk_bytes: 1_000,
            max_memory_bytes: 16_384,
            ready_queue_bytes: 4_096,
            max_chunk_uncompressed_bytes: 4_096,
        }
        .verify(&snapshot.manifest)
        .unwrap_or_else(|error| panic!("{error}"));
        let pending = PendingIntentPlan::build(Vec::new(), BTreeSet::new())
            .unwrap_or_else(|error| panic!("{error}"));
        let mut job = BootstrapJob::new(
            &snapshot.manifest,
            ReplicaGeneration::new(2).unwrap_or_else(|error| panic!("{error}")),
            BootstrapMutationPolicy::AllowQueue,
            &pending,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        for progress in job.chunks.values_mut() {
            progress.state = ChunkState::Installed;
        }
        job.state = BootstrapState::ReadyToActivate;
        let installed_chunks = snapshot
            .manifest
            .chunks
            .iter()
            .map(|chunk| chunk.chunk_id)
            .collect();
        let evidence = ActivationEvidence {
            manifest_root: snapshot.manifest.root_digest,
            installed_chunks,
            current_scope_version: job.boundary.scope_version,
            current_scope_generation: job.boundary.scope_generation,
            current_authority_epoch: job.boundary.authority_epoch,
            authorization_current: true,
            staging_verified: true,
            pending_intent_digest: pending.digest,
            lease: SnapshotLease {
                snapshot_id: job.snapshot_id,
                boundary_sequence: job.boundary.sequence,
                retained_from: Sequence(0),
                expires_at_unix_ms: 10_000,
            },
        };
        let activation = verify_activation(&job, &snapshot.manifest, &evidence, 5_000)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(activation.cursor_sequence, Sequence(41));
        let mut revoked = evidence;
        revoked.authorization_current = false;
        assert_eq!(
            verify_activation(&job, &snapshot.manifest, &revoked, 5_000),
            Err(BootstrapError::AuthorizationRevoked)
        );
    }

    proptest! {
        #[test]
        fn generated_input_order_never_changes_manifest(mut ids in prop::collection::btree_set(1_u128..10_000, 1..40).prop_map(|values| values.into_iter().collect::<Vec<_>>())) {
            let records = ids.iter().map(|id| record(*id, 4)).collect::<Vec<_>>();
            let expected = built(&records);
            ids.reverse();
            let reversed = ids.iter().map(|id| record(*id, 4)).collect::<Vec<_>>();
            prop_assert_eq!(built(&reversed), expected);
        }
    }
}
