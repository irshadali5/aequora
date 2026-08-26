//! Canonical, chunk-verified record export artifacts for explicit database migrations.
//!
//! Adapters export and import canonical records through their own native transactions. This crate
//! defines the portable artifact and verification boundary; it never opens a database, guesses a
//! schema map, resets an authority timeline, or overwrites a target store.

use aequora_schema::{CanonicalRecord, SchemaError, SchemaRegistry};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod workflow;

pub use workflow::*;

/// Current canonical export artifact format.
pub const EXPORT_FORMAT_VERSION: u32 = 1;

/// Resource bounds applied before an artifact is accepted for verification/import.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportLimits {
    /// Maximum encoded artifact bytes.
    pub max_bytes: usize,
    /// Maximum chunks.
    pub max_chunks: usize,
    /// Maximum total records.
    pub max_records: usize,
    /// Maximum records in one chunk.
    pub max_records_per_chunk: usize,
}

impl Default for ExportLimits {
    fn default() -> Self {
        Self {
            max_bytes: 256 * 1024 * 1024,
            max_chunks: 65_536,
            max_records: 10_000_000,
            max_records_per_chunk: 10_000,
        }
    }
}

/// Payload-free provenance attached to one canonical export.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExportHeader {
    /// Artifact compatibility version.
    pub format_version: u32,
    /// Aequora version producing the artifact.
    pub aequora_version: String,
    /// Source adapter name from its public manifest.
    pub source_adapter: String,
    /// Application-managed source physical schema version.
    pub source_schema_version: u32,
    /// Digest of the validated canonical schema registry.
    pub canonical_schema_digest: [u8; 32],
}

/// Independently verifiable record chunk.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExportChunk {
    /// Contiguous zero-based chunk index.
    pub index: u64,
    /// Canonical records in deterministic source order.
    pub records: Vec<CanonicalRecord>,
    /// BLAKE3 digest of index and record content.
    pub digest: [u8; 32],
}

/// Complete portable export artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CanonicalExport {
    /// Version/provenance and canonical schema identity.
    pub header: ExportHeader,
    /// Independently hashed chunks.
    pub chunks: Vec<ExportChunk>,
    /// Declared total, checked against chunk contents.
    pub record_count: u64,
    /// Digest binding header, record count, and ordered chunk digests.
    pub root_digest: [u8; 32],
}

impl CanonicalExport {
    /// Builds a verified artifact from canonical records in deterministic source order.
    ///
    /// # Errors
    ///
    /// Rejects invalid records, blank provenance, zero schema/chunk sizing, count overflow, or
    /// serialization failure.
    pub fn build(
        registry: &SchemaRegistry,
        source_adapter: impl Into<String>,
        source_schema_version: u32,
        records: &[CanonicalRecord],
        records_per_chunk: usize,
    ) -> Result<Self, ExportError> {
        let source_adapter = source_adapter.into();
        if source_adapter.trim().is_empty() {
            return Err(ExportError::BlankSourceAdapter);
        }
        if source_schema_version == 0 {
            return Err(ExportError::ZeroSourceSchemaVersion);
        }
        if records_per_chunk == 0 {
            return Err(ExportError::ZeroChunkSize);
        }
        for record in records {
            record.validate(registry)?;
        }
        let header = ExportHeader {
            format_version: EXPORT_FORMAT_VERSION,
            aequora_version: env!("CARGO_PKG_VERSION").to_owned(),
            source_adapter,
            source_schema_version,
            canonical_schema_digest: schema_digest(registry)?,
        };
        let mut chunks = Vec::new();
        for (index, records) in records.chunks(records_per_chunk).enumerate() {
            let index = u64::try_from(index).map_err(|_| ExportError::CountOverflow)?;
            let records = records.to_vec();
            let digest = chunk_digest(index, &records)?;
            chunks.push(ExportChunk {
                index,
                records,
                digest,
            });
        }
        let record_count = u64::try_from(records.len()).map_err(|_| ExportError::CountOverflow)?;
        let root_digest = root_digest(&header, record_count, &chunks)?;
        Ok(Self {
            header,
            chunks,
            record_count,
            root_digest,
        })
    }

    /// Verifies format, bounds, schema identity, chunk order/digests, records, and root digest.
    ///
    /// # Errors
    ///
    /// Returns a typed failure and never repairs or partially accepts an artifact.
    pub fn verify(
        &self,
        registry: &SchemaRegistry,
        limits: ExportLimits,
    ) -> Result<VerifiedExport<'_>, ExportError> {
        if self.header.format_version != EXPORT_FORMAT_VERSION {
            return Err(ExportError::UnsupportedFormat(self.header.format_version));
        }
        if self.header.canonical_schema_digest != schema_digest(registry)? {
            return Err(ExportError::SchemaDigestMismatch);
        }
        if self.chunks.len() > limits.max_chunks {
            return Err(ExportError::ChunkLimit);
        }
        let mut count = 0_usize;
        for (expected, chunk) in self.chunks.iter().enumerate() {
            let expected = u64::try_from(expected).map_err(|_| ExportError::CountOverflow)?;
            if chunk.index != expected {
                return Err(ExportError::NonContiguousChunk {
                    expected,
                    actual: chunk.index,
                });
            }
            if chunk.records.len() > limits.max_records_per_chunk {
                return Err(ExportError::RecordsPerChunkLimit);
            }
            count = count
                .checked_add(chunk.records.len())
                .ok_or(ExportError::CountOverflow)?;
            if count > limits.max_records {
                return Err(ExportError::RecordLimit);
            }
            if chunk.digest != chunk_digest(chunk.index, &chunk.records)? {
                return Err(ExportError::ChunkDigestMismatch(chunk.index));
            }
            for record in &chunk.records {
                record.validate(registry)?;
            }
        }
        if u64::try_from(count).map_err(|_| ExportError::CountOverflow)? != self.record_count {
            return Err(ExportError::RecordCountMismatch);
        }
        if self.root_digest != root_digest(&self.header, self.record_count, &self.chunks)? {
            return Err(ExportError::RootDigestMismatch);
        }
        Ok(VerifiedExport(self))
    }

    /// Encodes the artifact using the canonical internal Postcard representation.
    ///
    /// # Errors
    ///
    /// Returns serialization failure or a configured byte-limit violation.
    pub fn encode(&self, limits: ExportLimits) -> Result<Vec<u8>, ExportError> {
        let encoded = postcard::to_stdvec(self).map_err(ExportError::Serialize)?;
        if encoded.len() > limits.max_bytes {
            return Err(ExportError::ByteLimit);
        }
        Ok(encoded)
    }

    /// Decodes a bounded artifact. Call [`Self::verify`] before importing any record.
    ///
    /// # Errors
    ///
    /// Rejects byte/chunk/record bounds and malformed Postcard.
    pub fn decode(encoded: &[u8], limits: ExportLimits) -> Result<Self, ExportError> {
        if encoded.len() > limits.max_bytes {
            return Err(ExportError::ByteLimit);
        }
        let artifact: Self = postcard::from_bytes(encoded).map_err(ExportError::Deserialize)?;
        if artifact.chunks.len() > limits.max_chunks {
            return Err(ExportError::ChunkLimit);
        }
        if artifact.record_count
            > u64::try_from(limits.max_records).map_err(|_| ExportError::CountOverflow)?
        {
            return Err(ExportError::RecordLimit);
        }
        if artifact
            .chunks
            .iter()
            .any(|chunk| chunk.records.len() > limits.max_records_per_chunk)
        {
            return Err(ExportError::RecordsPerChunkLimit);
        }
        Ok(artifact)
    }
}

/// Proof that an export passed all structural, schema, record, and digest checks.
#[derive(Clone, Copy, Debug)]
pub struct VerifiedExport<'artifact>(&'artifact CanonicalExport);

impl<'artifact> VerifiedExport<'artifact> {
    /// Iterates verified records in source/chunk order.
    pub fn records(self) -> impl Iterator<Item = &'artifact CanonicalRecord> {
        self.0.chunks.iter().flat_map(|chunk| chunk.records.iter())
    }

    /// Number of verified records available for import.
    #[must_use]
    pub const fn len(self) -> u64 {
        self.0.record_count
    }

    /// Whether the verified artifact contains no records.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0.record_count == 0
    }

    /// Verified artifact root for source/target comparison and operator evidence.
    #[must_use]
    pub const fn root_digest(self) -> [u8; 32] {
        self.0.root_digest
    }

    /// Payload-free descriptor used to begin or resume a target import.
    #[must_use]
    pub fn descriptor(self) -> ImportDescriptor {
        ImportDescriptor {
            root_digest: self.0.root_digest,
            canonical_schema_digest: self.0.header.canonical_schema_digest,
            record_count: self.0.record_count,
            source_adapter: self.0.header.source_adapter.clone(),
            source_schema_version: self.0.header.source_schema_version,
        }
    }

    /// Independently verified chunks in contiguous source order.
    pub fn chunks(self) -> impl ExactSizeIterator<Item = VerifiedChunk<'artifact>> {
        self.0.chunks.iter().map(VerifiedChunk)
    }
}

/// Borrowed chunk that can only be obtained from a fully [`VerifiedExport`].
#[derive(Clone, Copy, Debug)]
pub struct VerifiedChunk<'artifact>(&'artifact ExportChunk);

impl<'artifact> VerifiedChunk<'artifact> {
    /// Contiguous chunk index.
    #[must_use]
    pub const fn index(self) -> u64 {
        self.0.index
    }

    /// Verified record slice.
    #[must_use]
    pub fn records(self) -> &'artifact [CanonicalRecord] {
        &self.0.records
    }

    /// Verified content digest used for idempotent staging.
    #[must_use]
    pub const fn digest(self) -> [u8; 32] {
        self.0.digest
    }
}

/// Stable identity and expected totals for a resumable canonical import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDescriptor {
    /// Artifact identity and final target verification value.
    pub root_digest: [u8; 32],
    /// Canonical schema required by the artifact.
    pub canonical_schema_digest: [u8; 32],
    /// Exact record count expected before publication.
    pub record_count: u64,
    /// Payload-free source adapter identity.
    pub source_adapter: String,
    /// Application-owned source schema version.
    pub source_schema_version: u32,
}

/// Result of atomically publishing a completely staged canonical import.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportOutcome {
    /// This call published the target state.
    Applied,
    /// The same root was already published and no additional logical effect occurred.
    Duplicate,
}

/// Bounded, deterministic page produced by an adapter-owned canonical record source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalRecordPage {
    /// Records strictly after the requested source offset.
    pub records: Vec<CanonicalRecord>,
    /// Opaque monotonic adapter offset for the next page.
    pub next_offset: u64,
    /// Whether another page is required for a complete consistent export.
    pub has_more: bool,
}

/// Database-specific consistent source boundary for canonical record export.
///
/// Implementations must hold or identify one stable source snapshot for the complete paging run.
/// Returning `has_more` without records and monotonic offset progress is a contract violation.
#[async_trait]
pub trait CanonicalRecordSource: Send + Sync {
    /// Reads a bounded page from one consistent canonical source view.
    async fn read_canonical_page(
        &self,
        after_offset: u64,
        max_records: usize,
    ) -> Result<CanonicalRecordPage, MigrationStoreError>;
}

/// Exports one bounded consistent source into a verified canonical artifact.
///
/// # Errors
///
/// Returns typed source, paging-contract, resource-bound, record/schema, or artifact errors.
pub async fn export_source<S>(
    source: &S,
    registry: &SchemaRegistry,
    source_adapter: impl Into<String>,
    source_schema_version: u32,
    limits: ExportLimits,
) -> Result<CanonicalExport, MigrationError>
where
    S: CanonicalRecordSource,
{
    if limits.max_records_per_chunk == 0 {
        return Err(ExportError::ZeroChunkSize.into());
    }
    let mut records = Vec::new();
    let mut offset = 0_u64;
    loop {
        let page = source
            .read_canonical_page(offset, limits.max_records_per_chunk)
            .await?;
        if page.records.len() > limits.max_records_per_chunk {
            return Err(MigrationStoreError::permanent(
                "canonical source exceeded requested page size",
            )
            .into());
        }
        if page.has_more && (page.records.is_empty() || page.next_offset <= offset) {
            return Err(MigrationStoreError::permanent(
                "canonical source did not make monotonic paging progress",
            )
            .into());
        }
        records.extend(page.records);
        if records.len() > limits.max_records {
            return Err(ExportError::RecordLimit.into());
        }
        if !page.has_more {
            break;
        }
        offset = page.next_offset;
    }
    CanonicalExport::build(
        registry,
        source_adapter,
        source_schema_version,
        &records,
        limits.max_records_per_chunk,
    )
    .map_err(MigrationError::from)
}

/// Database-specific, resumable target transaction boundary for canonical migration artifacts.
///
/// Implementations must keep staged chunks invisible to normal application reads. Repeating
/// `begin_import` or `stage_chunk` with the same root/index/digest must be idempotent. A mismatched
/// descriptor or digest must fail closed. `commit_import` must verify completeness and publish all
/// target changes atomically according to the adapter's documented migration boundary.
#[async_trait]
pub trait CanonicalRecordSink: Send + Sync {
    /// Creates or resumes staging for one verified artifact.
    async fn begin_import(&self, descriptor: &ImportDescriptor) -> Result<(), MigrationStoreError>;

    /// Stages one independently verified chunk.
    async fn stage_chunk(
        &self,
        root_digest: [u8; 32],
        chunk: VerifiedChunk<'_>,
    ) -> Result<(), MigrationStoreError>;

    /// Atomically publishes a complete staged import or returns its prior identical outcome.
    async fn commit_import(
        &self,
        root_digest: [u8; 32],
    ) -> Result<ImportOutcome, MigrationStoreError>;
}

/// Runs a verified artifact through an adapter-owned resumable staging boundary.
///
/// A failure intentionally retains adapter staging so the same root/chunk identities can resume.
/// It never asks the sink to publish until every verified chunk has been accepted.
///
/// # Errors
///
/// Returns a typed adapter error from begin, staging, or final atomic publication.
pub async fn import_verified<S>(
    sink: &S,
    export: VerifiedExport<'_>,
) -> Result<ImportOutcome, MigrationStoreError>
where
    S: CanonicalRecordSink,
{
    let descriptor = export.descriptor();
    sink.begin_import(&descriptor).await?;
    for chunk in export.chunks() {
        sink.stage_chunk(descriptor.root_digest, chunk).await?;
    }
    sink.commit_import(descriptor.root_digest).await
}

/// Retry classification for adapter-owned migration staging/publication failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationStoreErrorKind {
    /// The same root/chunk may be retried without changing its identity.
    Transient,
    /// Schema, digest, capability, or semantic mismatch requires operator correction.
    Permanent,
}

/// Payload-free migration adapter failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("migration store error ({kind:?}): {message}")]
pub struct MigrationStoreError {
    /// Safe retry classification.
    pub kind: MigrationStoreErrorKind,
    /// Structural diagnostic that must not contain record payloads or credentials.
    pub message: String,
}

/// Failure from a complete source export or target import workflow.
#[derive(Debug, Error)]
pub enum MigrationError {
    /// Adapter-owned source/sink boundary failed.
    #[error(transparent)]
    Store(#[from] MigrationStoreError),
    /// Canonical artifact construction or verification failed.
    #[error(transparent)]
    Export(#[from] ExportError),
}

impl MigrationStoreError {
    /// Creates a retryable staging/publication failure.
    #[must_use]
    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            kind: MigrationStoreErrorKind::Transient,
            message: message.into(),
        }
    }

    /// Creates a fail-closed semantic/capability failure.
    #[must_use]
    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            kind: MigrationStoreErrorKind::Permanent,
            message: message.into(),
        }
    }
}

fn schema_digest(registry: &SchemaRegistry) -> Result<[u8; 32], ExportError> {
    let encoded = postcard::to_stdvec(registry).map_err(ExportError::Serialize)?;
    Ok(*blake3::hash(&encoded).as_bytes())
}

fn chunk_digest(index: u64, records: &[CanonicalRecord]) -> Result<[u8; 32], ExportError> {
    let encoded = postcard::to_stdvec(&(index, records)).map_err(ExportError::Serialize)?;
    Ok(*blake3::hash(&encoded).as_bytes())
}

fn root_digest(
    header: &ExportHeader,
    record_count: u64,
    chunks: &[ExportChunk],
) -> Result<[u8; 32], ExportError> {
    let chunk_digests: Vec<_> = chunks.iter().map(|chunk| chunk.digest).collect();
    let encoded = postcard::to_stdvec(&(header, record_count, chunk_digests))
        .map_err(ExportError::Serialize)?;
    Ok(*blake3::hash(&encoded).as_bytes())
}

/// Fail-closed canonical export construction or verification failure.
#[derive(Debug, Error)]
pub enum ExportError {
    /// Source adapter identity is absent.
    #[error("source adapter name must not be blank")]
    BlankSourceAdapter,
    /// Physical schema revision zero is unassigned.
    #[error("source physical schema version must be non-zero")]
    ZeroSourceSchemaVersion,
    /// Chunk sizing must be explicit and non-zero.
    #[error("records per chunk must be non-zero")]
    ZeroChunkSize,
    /// Artifact format is unsupported.
    #[error("unsupported canonical export format {0}")]
    UnsupportedFormat(u32),
    /// Canonical registry differs from the exporter registry.
    #[error("canonical schema digest mismatch")]
    SchemaDigestMismatch,
    /// Encoded bytes exceed configured bounds.
    #[error("canonical export exceeds byte limit")]
    ByteLimit,
    /// Chunk count exceeds configured bounds.
    #[error("canonical export exceeds chunk limit")]
    ChunkLimit,
    /// Record count exceeds configured bounds.
    #[error("canonical export exceeds record limit")]
    RecordLimit,
    /// One chunk exceeds configured bounds.
    #[error("canonical export chunk exceeds per-chunk record limit")]
    RecordsPerChunkLimit,
    /// Chunk indices are missing or reordered.
    #[error("expected export chunk {expected}, got {actual}")]
    NonContiguousChunk {
        /// Required index.
        expected: u64,
        /// Actual index.
        actual: u64,
    },
    /// Chunk content does not match its digest.
    #[error("canonical export chunk {0} digest mismatch")]
    ChunkDigestMismatch(u64),
    /// Declared and actual record totals differ.
    #[error("canonical export record count mismatch")]
    RecordCountMismatch,
    /// Root does not bind the observed header/count/chunks.
    #[error("canonical export root digest mismatch")]
    RootDigestMismatch,
    /// Platform count conversion overflowed.
    #[error("canonical export count overflow")]
    CountOverflow,
    /// Canonical schema/record is invalid.
    #[error(transparent)]
    Schema(#[from] SchemaError),
    /// Artifact construction encoding failed.
    #[error("canonical export serialization failed: {0}")]
    Serialize(postcard::Error),
    /// Artifact input is malformed.
    #[error("canonical export decoding failed: {0}")]
    Deserialize(postcard::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_schema::{
        CanonicalEntityId, CanonicalFieldId, CanonicalType, CanonicalValue, EntitySchema,
        FieldSchema,
    };
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::{Mutex, MutexGuard},
    };

    #[derive(Default)]
    struct ImportState {
        descriptor: Option<ImportDescriptor>,
        chunks: BTreeMap<u64, ([u8; 32], Vec<CanonicalRecord>)>,
        committed: Option<[u8; 32]>,
    }

    #[derive(Default)]
    struct InMemorySink(Mutex<ImportState>);

    struct InMemorySource {
        records: Vec<CanonicalRecord>,
    }

    #[async_trait]
    impl CanonicalRecordSource for InMemorySource {
        async fn read_canonical_page(
            &self,
            after_offset: u64,
            max_records: usize,
        ) -> Result<CanonicalRecordPage, MigrationStoreError> {
            let offset = usize::try_from(after_offset).map_err(|_| {
                MigrationStoreError::permanent("source offset exceeds platform size")
            })?;
            let end = offset.saturating_add(max_records).min(self.records.len());
            let next_offset = u64::try_from(end)
                .map_err(|_| MigrationStoreError::permanent("source offset conversion failed"))?;
            Ok(CanonicalRecordPage {
                records: self.records[offset..end].to_vec(),
                next_offset,
                has_more: end < self.records.len(),
            })
        }
    }

    impl InMemorySink {
        fn state(&self) -> MutexGuard<'_, ImportState> {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

    #[async_trait]
    impl CanonicalRecordSink for InMemorySink {
        async fn begin_import(
            &self,
            descriptor: &ImportDescriptor,
        ) -> Result<(), MigrationStoreError> {
            let mut state = self.state();
            if let Some(existing) = &state.descriptor {
                if existing != descriptor {
                    return Err(MigrationStoreError::permanent(
                        "import descriptor changed for existing staging",
                    ));
                }
                return Ok(());
            }
            state.descriptor = Some(descriptor.clone());
            Ok(())
        }

        async fn stage_chunk(
            &self,
            root_digest: [u8; 32],
            chunk: VerifiedChunk<'_>,
        ) -> Result<(), MigrationStoreError> {
            let mut state = self.state();
            if state
                .descriptor
                .as_ref()
                .is_none_or(|descriptor| descriptor.root_digest != root_digest)
            {
                return Err(MigrationStoreError::permanent(
                    "chunk root does not match staged import",
                ));
            }
            if let Some((digest, _)) = state.chunks.get(&chunk.index()) {
                if *digest != chunk.digest() {
                    return Err(MigrationStoreError::permanent(
                        "chunk index was reused with a different digest",
                    ));
                }
                return Ok(());
            }
            state
                .chunks
                .insert(chunk.index(), (chunk.digest(), chunk.records().to_vec()));
            Ok(())
        }

        async fn commit_import(
            &self,
            root_digest: [u8; 32],
        ) -> Result<ImportOutcome, MigrationStoreError> {
            let mut state = self.state();
            if state.committed == Some(root_digest) {
                return Ok(ImportOutcome::Duplicate);
            }
            let descriptor = state
                .descriptor
                .as_ref()
                .ok_or_else(|| MigrationStoreError::permanent("import was not initialized"))?;
            if descriptor.root_digest != root_digest {
                return Err(MigrationStoreError::permanent(
                    "commit root does not match staged import",
                ));
            }
            let count = state
                .chunks
                .values()
                .try_fold(0_u64, |count, (_, records)| {
                    count.checked_add(u64::try_from(records.len()).ok()?)
                });
            if count != Some(descriptor.record_count) {
                return Err(MigrationStoreError::permanent(
                    "staged record count is incomplete",
                ));
            }
            state.committed = Some(root_digest);
            Ok(ImportOutcome::Applied)
        }
    }

    fn fixture() -> (SchemaRegistry, Vec<CanonicalRecord>) {
        let entity = CanonicalEntityId::new(1)
            .unwrap_or_else(|error| panic!("entity fixture failed: {error}"));
        let field = CanonicalFieldId::new(1)
            .unwrap_or_else(|error| panic!("field fixture failed: {error}"));
        let mut registry = SchemaRegistry::new();
        registry
            .register(EntitySchema {
                id: entity,
                name: "student".to_owned(),
                fields: vec![FieldSchema {
                    id: field,
                    name: "name".to_owned(),
                    field_type: CanonicalType::Text,
                    nullable: false,
                }],
            })
            .unwrap_or_else(|error| panic!("registry fixture failed: {error}"));
        let records = (0..5)
            .map(|index| CanonicalRecord {
                entity,
                key: CanonicalValue::Unsigned(index),
                fields: BTreeMap::from([(field, CanonicalValue::Text(format!("student-{index}")))]),
            })
            .collect();
        (registry, records)
    }

    #[test]
    fn artifact_round_trips_and_verifies_chunks() {
        let (registry, records) = fixture();
        let limits = ExportLimits::default();
        let artifact = CanonicalExport::build(&registry, "legacy-db", 7, &records, 2)
            .unwrap_or_else(|error| panic!("export failed: {error}"));
        let encoded = artifact
            .encode(limits)
            .unwrap_or_else(|error| panic!("encoding failed: {error}"));
        let decoded = CanonicalExport::decode(&encoded, limits)
            .unwrap_or_else(|error| panic!("decoding failed: {error}"));
        let verified = decoded
            .verify(&registry, limits)
            .unwrap_or_else(|error| panic!("verification failed: {error}"));
        assert_eq!(verified.records().count(), 5);
        assert_eq!(verified.len(), 5);
        assert_eq!(verified.root_digest(), artifact.root_digest);
        let manifest = ExportBundleManifest {
            mode: ExportMode::MigrationBundle,
            created_at_unix_ms: 1,
            scopes: BTreeSet::new(),
            cursor_boundary: None,
            artifact_root: artifact.root_digest,
            record_count: artifact.record_count,
            encrypted: true,
        };
        assert_eq!(manifest.verify(&artifact, true), Ok(()));
    }

    #[test]
    fn tampered_chunk_fails_closed() {
        let (registry, records) = fixture();
        let mut artifact = CanonicalExport::build(&registry, "legacy-db", 7, &records, 2)
            .unwrap_or_else(|error| panic!("export failed: {error}"));
        artifact.chunks[0].records[0].fields.clear();
        assert!(matches!(
            artifact.verify(&registry, ExportLimits::default()),
            Err(ExportError::ChunkDigestMismatch(0))
        ));
    }

    #[tokio::test]
    async fn verified_import_is_resumable_and_idempotent() {
        let (registry, records) = fixture();
        let artifact = CanonicalExport::build(&registry, "legacy-db", 7, &records, 2)
            .unwrap_or_else(|error| panic!("export failed: {error}"));
        let verified = artifact
            .verify(&registry, ExportLimits::default())
            .unwrap_or_else(|error| panic!("verification failed: {error}"));
        let sink = InMemorySink::default();
        assert_eq!(
            import_verified(&sink, verified).await,
            Ok(ImportOutcome::Applied)
        );
        assert_eq!(
            import_verified(&sink, verified).await,
            Ok(ImportOutcome::Duplicate)
        );
        assert_eq!(sink.state().chunks.len(), 3);
    }

    #[tokio::test]
    async fn bounded_source_export_round_trips_through_sink() {
        let (registry, records) = fixture();
        let source = InMemorySource { records };
        let limits = ExportLimits {
            max_records_per_chunk: 2,
            ..ExportLimits::default()
        };
        let artifact = export_source(&source, &registry, "reference-source", 1, limits)
            .await
            .unwrap_or_else(|error| panic!("source export failed: {error}"));
        let verified = artifact
            .verify(&registry, limits)
            .unwrap_or_else(|error| panic!("source artifact failed verification: {error}"));
        assert_eq!(verified.len(), 5);
        assert_eq!(
            import_verified(&InMemorySink::default(), verified).await,
            Ok(ImportOutcome::Applied)
        );
    }
}
