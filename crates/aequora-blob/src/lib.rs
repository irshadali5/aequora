//! Content-addressed blob metadata kept outside normal synchronization operation batches.

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use thiserror::Error;

/// BLAKE3 content digest.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct BlobDigest(pub [u8; 32]);

impl BlobDigest {
    /// Hashes a complete byte slice.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }

    /// Verifies bytes against this digest.
    #[must_use]
    pub fn verifies(self, bytes: &[u8]) -> bool {
        Self::of(bytes) == self
    }
}

/// Small content-addressed reference safe to embed in a domain operation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BlobRef {
    /// Digest of the complete content.
    pub digest: BlobDigest,
    /// Exact complete byte length.
    pub length: u64,
}

impl BlobRef {
    /// Creates a reference for complete content.
    #[must_use]
    pub fn for_bytes(bytes: &[u8]) -> Self {
        Self {
            digest: BlobDigest::of(bytes),
            length: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        }
    }

    /// Verifies both digest and exact length.
    #[must_use]
    pub fn verifies(self, bytes: &[u8]) -> bool {
        usize::try_from(self.length).is_ok_and(|length| length == bytes.len())
            && self.digest.verifies(bytes)
    }
}

/// Chunked upload manifest for one complete blob.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BlobManifest {
    /// Complete content reference.
    pub blob: BlobRef,
    /// Maximum bytes per chunk.
    pub chunk_size: u32,
    /// Ordered chunk digests.
    pub chunks: Vec<BlobDigest>,
}

impl BlobManifest {
    /// Builds a manifest from complete content.
    ///
    /// # Errors
    ///
    /// Returns [`BlobError::InvalidChunkSize`] when `chunk_size` is zero.
    pub fn for_bytes(bytes: &[u8], chunk_size: u32) -> Result<Self, BlobError> {
        let chunk_size = usize::try_from(chunk_size).map_err(|_| BlobError::InvalidChunkSize)?;
        if chunk_size == 0 {
            return Err(BlobError::InvalidChunkSize);
        }
        Ok(Self {
            blob: BlobRef::for_bytes(bytes),
            chunk_size: u32::try_from(chunk_size).unwrap_or(u32::MAX),
            chunks: bytes.chunks(chunk_size).map(BlobDigest::of).collect(),
        })
    }
}

/// Blob subsystem failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum BlobError {
    /// Chunk size must be non-zero and representable.
    #[error("blob chunk size must be greater than zero")]
    InvalidChunkSize,
    /// Chunk bytes did not match the content-addressed digest.
    #[error("blob chunk digest mismatch")]
    DigestMismatch,
    /// A manifest references a chunk that has not been uploaded.
    #[error("blob chunk is missing")]
    MissingChunk,
    /// The manifest's chunk layout is inconsistent with the complete blob reference.
    #[error("blob manifest layout is invalid")]
    InvalidManifest,
    /// An upload exceeded a configured storage boundary.
    #[error("blob exceeds configured limit")]
    LimitExceeded,
    /// Store-specific typed failure.
    #[error("blob storage failed: {0}")]
    Storage(String),
    /// A streamed source produced more chunks than its explicit manifest bound.
    #[error("blob stream exceeds configured chunk count")]
    ChunkLimit,
    /// A non-final source chunk was short or an empty chunk was supplied.
    #[error("blob source chunk layout is invalid")]
    InvalidChunkLayout,
}

/// Separate content-addressed blob transfer capability.
#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Returns digests not yet present in the store.
    async fn missing(&self, digests: &[BlobDigest]) -> Result<Vec<BlobDigest>, BlobError>;
    /// Stores one verified chunk idempotently.
    async fn put_chunk(&self, digest: BlobDigest, bytes: Vec<u8>) -> Result<(), BlobError>;
    /// Stores immutable shared bytes. Adapters should override this to retain the buffer without a
    /// copy; the compatibility default delegates to the original owned-vector API.
    async fn put_chunk_bytes(&self, digest: BlobDigest, bytes: Bytes) -> Result<(), BlobError> {
        self.put_chunk(digest, bytes.to_vec()).await
    }
    /// Verifies the ordered chunks and publishes the complete blob atomically.
    async fn commit(&self, manifest: &BlobManifest) -> Result<(), BlobError>;
    /// Reads a published complete blob. Uncommitted chunks are never visible here.
    async fn get(&self, blob: BlobRef) -> Result<Option<Vec<u8>>, BlobError>;
}

/// Asynchronous bounded source used by blob uploads. Each returned item is one complete chunk.
#[async_trait]
pub trait BlobUploadSource: Send {
    async fn next_chunk(&mut self) -> Result<Option<Bytes>, BlobError>;
}

/// A store that can read one immutable chunk of a committed blob without materializing the blob.
#[async_trait]
pub trait StreamingBlobStore: BlobStore {
    async fn get_committed_chunk(&self, digest: BlobDigest) -> Result<Option<Bytes>, BlobError>;
}

/// Consumer for one verified download chunk at a time.
#[async_trait]
pub trait BlobDownloadSink: Send {
    async fn write_chunk(&mut self, ordinal: u32, bytes: Bytes) -> Result<(), BlobError>;
}

/// Reads, hashes, and uploads one bounded chunk at a time, then atomically publishes the manifest.
/// The complete blob never exists in this function's memory.
///
/// # Errors
///
/// Returns a typed bound, layout, source, storage, or commit failure.
pub async fn upload_stream<S, R>(
    store: &S,
    source: &mut R,
    chunk_size: u32,
    max_blob_bytes: u64,
    max_chunks: usize,
) -> Result<BlobManifest, BlobError>
where
    S: BlobStore + ?Sized,
    R: BlobUploadSource + ?Sized,
{
    let chunk_size = usize::try_from(chunk_size).map_err(|_| BlobError::InvalidChunkSize)?;
    if chunk_size == 0 || max_blob_bytes == 0 || max_chunks == 0 {
        return Err(BlobError::InvalidChunkSize);
    }
    let mut pending: Option<Bytes> = None;
    let mut chunks = Vec::with_capacity(max_chunks.min(1_024));
    let mut complete = blake3::Hasher::new();
    let mut total = 0_u64;
    while let Some(next) = source.next_chunk().await? {
        if next.is_empty() || next.len() > chunk_size {
            return Err(BlobError::InvalidChunkLayout);
        }
        if chunks.len().saturating_add(usize::from(pending.is_some())) == max_chunks {
            return Err(BlobError::ChunkLimit);
        }
        if let Some(previous) = pending.replace(next) {
            if previous.len() != chunk_size {
                return Err(BlobError::InvalidChunkLayout);
            }
            upload_one_chunk(store, previous, &mut chunks, &mut complete, &mut total).await?;
            if total > max_blob_bytes {
                return Err(BlobError::LimitExceeded);
            }
        }
    }
    if let Some(last) = pending {
        upload_one_chunk(store, last, &mut chunks, &mut complete, &mut total).await?;
    }
    if total > max_blob_bytes || chunks.len() > max_chunks {
        return Err(BlobError::LimitExceeded);
    }
    let manifest = BlobManifest {
        blob: BlobRef {
            digest: BlobDigest(*complete.finalize().as_bytes()),
            length: total,
        },
        chunk_size: u32::try_from(chunk_size).map_err(|_| BlobError::InvalidChunkSize)?,
        chunks,
    };
    store.commit(&manifest).await?;
    Ok(manifest)
}

async fn upload_one_chunk<S: BlobStore + ?Sized>(
    store: &S,
    bytes: Bytes,
    chunks: &mut Vec<BlobDigest>,
    complete: &mut blake3::Hasher,
    total: &mut u64,
) -> Result<(), BlobError> {
    let digest = BlobDigest::of(&bytes);
    complete.update(&bytes);
    *total = total
        .checked_add(u64::try_from(bytes.len()).map_err(|_| BlobError::LimitExceeded)?)
        .ok_or(BlobError::LimitExceeded)?;
    store.put_chunk_bytes(digest, bytes).await?;
    chunks.push(digest);
    Ok(())
}

/// Downloads and verifies a committed blob one chunk at a time before handing each chunk to the
/// sink. The full content is never retained by the orchestration layer.
///
/// # Errors
///
/// Returns a typed manifest, storage, digest, or sink failure.
pub async fn download_stream<S, W>(
    store: &S,
    manifest: &BlobManifest,
    max_chunk_bytes: usize,
    max_chunks: usize,
    sink: &mut W,
) -> Result<(), BlobError>
where
    S: StreamingBlobStore + ?Sized,
    W: BlobDownloadSink + ?Sized,
{
    let chunk_size =
        usize::try_from(manifest.chunk_size).map_err(|_| BlobError::InvalidManifest)?;
    if chunk_size == 0 || chunk_size > max_chunk_bytes || manifest.chunks.len() > max_chunks {
        return Err(BlobError::LimitExceeded);
    }
    let expected_chunks = if manifest.blob.length == 0 {
        0
    } else {
        usize::try_from(manifest.blob.length)
            .map_err(|_| BlobError::LimitExceeded)?
            .div_ceil(chunk_size)
    };
    if manifest.chunks.len() != expected_chunks {
        return Err(BlobError::InvalidManifest);
    }
    let mut complete = blake3::Hasher::new();
    let mut total = 0_u64;
    for (index, digest) in manifest.chunks.iter().copied().enumerate() {
        let bytes = store
            .get_committed_chunk(digest)
            .await?
            .ok_or(BlobError::MissingChunk)?;
        if bytes.is_empty() || bytes.len() > chunk_size || !digest.verifies(&bytes) {
            return Err(BlobError::DigestMismatch);
        }
        if index + 1 != expected_chunks && bytes.len() != chunk_size {
            return Err(BlobError::InvalidChunkLayout);
        }
        complete.update(&bytes);
        total = total
            .checked_add(u64::try_from(bytes.len()).map_err(|_| BlobError::LimitExceeded)?)
            .ok_or(BlobError::LimitExceeded)?;
        sink.write_chunk(
            u32::try_from(index).map_err(|_| BlobError::ChunkLimit)?,
            bytes,
        )
        .await?;
    }
    if total != manifest.blob.length
        || BlobDigest(*complete.finalize().as_bytes()) != manifest.blob.digest
    {
        return Err(BlobError::DigestMismatch);
    }
    Ok(())
}

/// Bounded in-memory reference implementation of the separate blob capability.
///
/// Chunks remain private staging data until [`BlobStore::commit`] validates the complete
/// manifest under one lock. This is useful for tests, embedded deployments, and as executable
/// semantics for durable adapters.
#[derive(Clone, Debug)]
pub struct InMemoryBlobStore {
    inner: Arc<Mutex<BlobState>>,
    max_chunk_bytes: usize,
    max_blob_bytes: usize,
}

#[derive(Debug, Default)]
struct BlobState {
    chunks: HashMap<BlobDigest, Bytes>,
    published_chunks: HashSet<BlobDigest>,
    blobs: HashMap<BlobDigest, Vec<u8>>,
}

impl InMemoryBlobStore {
    /// Creates an empty store with hard upload and complete-content limits.
    ///
    /// # Errors
    ///
    /// Returns [`BlobError::InvalidChunkSize`] when `max_chunk_bytes` is zero or
    /// [`BlobError::LimitExceeded`] when the complete limit is smaller than one chunk.
    pub fn new(max_chunk_bytes: usize, max_blob_bytes: usize) -> Result<Self, BlobError> {
        if max_chunk_bytes == 0 {
            return Err(BlobError::InvalidChunkSize);
        }
        if max_blob_bytes < max_chunk_bytes {
            return Err(BlobError::LimitExceeded);
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(BlobState::default())),
            max_chunk_bytes,
            max_blob_bytes,
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, BlobState>, BlobError> {
        self.inner
            .lock()
            .map_err(|_| BlobError::Storage("blob store lock was poisoned".to_owned()))
    }
}

#[async_trait]
impl BlobStore for InMemoryBlobStore {
    async fn missing(&self, digests: &[BlobDigest]) -> Result<Vec<BlobDigest>, BlobError> {
        let state = self.lock()?;
        Ok(digests
            .iter()
            .copied()
            .filter(|digest| !state.chunks.contains_key(digest))
            .collect())
    }

    async fn put_chunk(&self, digest: BlobDigest, bytes: Vec<u8>) -> Result<(), BlobError> {
        self.put_chunk_bytes(digest, Bytes::from(bytes)).await
    }

    async fn put_chunk_bytes(&self, digest: BlobDigest, bytes: Bytes) -> Result<(), BlobError> {
        if bytes.len() > self.max_chunk_bytes {
            return Err(BlobError::LimitExceeded);
        }
        verify_chunk(digest, &bytes)?;
        let mut state = self.lock()?;
        state.chunks.entry(digest).or_insert(bytes);
        Ok(())
    }

    async fn commit(&self, manifest: &BlobManifest) -> Result<(), BlobError> {
        let blob_length =
            usize::try_from(manifest.blob.length).map_err(|_| BlobError::LimitExceeded)?;
        if blob_length > self.max_blob_bytes
            || manifest.chunk_size == 0
            || usize::try_from(manifest.chunk_size).is_err()
        {
            return Err(BlobError::LimitExceeded);
        }
        let chunk_size =
            usize::try_from(manifest.chunk_size).map_err(|_| BlobError::InvalidChunkSize)?;
        if chunk_size > self.max_chunk_bytes {
            return Err(BlobError::LimitExceeded);
        }
        let expected_chunks = if blob_length == 0 {
            0
        } else {
            blob_length.div_ceil(chunk_size)
        };
        if manifest.chunks.len() != expected_chunks {
            return Err(BlobError::InvalidManifest);
        }

        let mut state = self.lock()?;
        if state.blobs.contains_key(&manifest.blob.digest) {
            return Ok(());
        }
        let mut complete = Vec::with_capacity(blob_length);
        for (index, digest) in manifest.chunks.iter().enumerate() {
            let chunk = state.chunks.get(digest).ok_or(BlobError::MissingChunk)?;
            verify_chunk(*digest, chunk)?;
            let expected_length = if index + 1 == expected_chunks {
                blob_length.saturating_sub(index.saturating_mul(chunk_size))
            } else {
                chunk_size
            };
            if chunk.len() != expected_length {
                return Err(BlobError::InvalidManifest);
            }
            complete.extend_from_slice(chunk);
        }
        if !manifest.blob.verifies(&complete) {
            return Err(BlobError::DigestMismatch);
        }
        state
            .published_chunks
            .extend(manifest.chunks.iter().copied());
        state.blobs.insert(manifest.blob.digest, complete);
        Ok(())
    }

    async fn get(&self, blob: BlobRef) -> Result<Option<Vec<u8>>, BlobError> {
        let state = self.lock()?;
        Ok(state
            .blobs
            .get(&blob.digest)
            .filter(|bytes| blob.verifies(bytes))
            .cloned())
    }
}

#[async_trait]
impl StreamingBlobStore for InMemoryBlobStore {
    async fn get_committed_chunk(&self, digest: BlobDigest) -> Result<Option<Bytes>, BlobError> {
        let state = self.lock()?;
        if !state.published_chunks.contains(&digest) {
            return Ok(None);
        }
        Ok(state.chunks.get(&digest).cloned())
    }
}

/// Verifies a chunk before it reaches storage.
///
/// # Errors
///
/// Returns [`BlobError::DigestMismatch`] if its BLAKE3 digest differs.
pub fn verify_chunk(digest: BlobDigest, bytes: &[u8]) -> Result<(), BlobError> {
    if digest.verifies(bytes) {
        Ok(())
    } else {
        Err(BlobError::DigestMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct ChunkSource {
        chunks: VecDeque<Bytes>,
    }

    #[async_trait]
    impl BlobUploadSource for ChunkSource {
        async fn next_chunk(&mut self) -> Result<Option<Bytes>, BlobError> {
            Ok(self.chunks.pop_front())
        }
    }

    #[derive(Default)]
    struct CollectSink {
        chunks: Vec<Bytes>,
    }

    #[async_trait]
    impl BlobDownloadSink for CollectSink {
        async fn write_chunk(&mut self, ordinal: u32, bytes: Bytes) -> Result<(), BlobError> {
            if usize::try_from(ordinal).map_err(|_| BlobError::ChunkLimit)? != self.chunks.len() {
                return Err(BlobError::InvalidChunkLayout);
            }
            self.chunks.push(bytes);
            Ok(())
        }
    }

    #[test]
    fn manifests_reference_chunks_and_complete_content() {
        let bytes = b"abcdefgh";
        let manifest = BlobManifest::for_bytes(bytes, 3).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(manifest.chunks.len(), 3);
        assert!(manifest.blob.verifies(bytes));
        assert!(verify_chunk(manifest.chunks[0], b"abc").is_ok());
        assert!(verify_chunk(manifest.chunks[0], b"abd").is_err());
    }

    #[tokio::test]
    async fn chunks_are_invisible_until_atomic_manifest_commit() {
        let bytes = b"content addressed attachment";
        let manifest = BlobManifest::for_bytes(bytes, 8).unwrap_or_else(|error| panic!("{error}"));
        let store = InMemoryBlobStore::new(8, 1024).unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(
            store
                .missing(&manifest.chunks)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            manifest.chunks
        );
        for chunk in bytes.chunks(8) {
            store
                .put_chunk(BlobDigest::of(chunk), chunk.to_vec())
                .await
                .unwrap_or_else(|error| panic!("{error}"));
        }
        assert_eq!(
            store
                .get(manifest.blob)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            None
        );
        store
            .commit(&manifest)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        store
            .commit(&manifest)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            store
                .get(manifest.blob)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            Some(bytes.to_vec())
        );
        assert!(
            store
                .missing(&manifest.chunks)
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .is_empty()
        );
    }

    #[tokio::test]
    async fn incomplete_or_invalid_uploads_never_publish() {
        let bytes = b"two chunks";
        let manifest = BlobManifest::for_bytes(bytes, 5).unwrap_or_else(|error| panic!("{error}"));
        let store = InMemoryBlobStore::new(5, 1024).unwrap_or_else(|error| panic!("{error}"));
        store
            .put_chunk(manifest.chunks[0], bytes[..5].to_vec())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(store.commit(&manifest).await, Err(BlobError::MissingChunk));
        assert_eq!(
            store
                .get(manifest.blob)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            None
        );
        assert_eq!(
            store
                .put_chunk(manifest.chunks[1], b"incorrect".to_vec())
                .await,
            Err(BlobError::LimitExceeded)
        );
        assert_eq!(
            store
                .get(manifest.blob)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            None
        );
    }

    #[tokio::test]
    async fn upload_and_download_streams_retain_only_bounded_chunks() {
        let store = InMemoryBlobStore::new(4, 1_024).unwrap_or_else(|error| panic!("{error}"));
        let mut source = ChunkSource {
            chunks: VecDeque::from([
                Bytes::from_static(b"abcd"),
                Bytes::from_static(b"efgh"),
                Bytes::from_static(b"ij"),
            ]),
        };
        let manifest = upload_stream(&store, &mut source, 4, 1_024, 8)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(manifest.blob.length, 10);
        let mut sink = CollectSink::default();
        download_stream(&store, &manifest, 4, 8, &mut sink)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(sink.chunks.len(), 3);
        assert_eq!(sink.chunks.concat(), b"abcdefghij");
    }

    #[tokio::test]
    async fn short_non_final_stream_chunk_is_rejected() {
        let store = InMemoryBlobStore::new(4, 1_024).unwrap_or_else(|error| panic!("{error}"));
        let mut source = ChunkSource {
            chunks: VecDeque::from([Bytes::from_static(b"ab"), Bytes::from_static(b"cdef")]),
        };
        assert_eq!(
            upload_stream(&store, &mut source, 4, 1_024, 8).await,
            Err(BlobError::InvalidChunkLayout)
        );
    }
}
