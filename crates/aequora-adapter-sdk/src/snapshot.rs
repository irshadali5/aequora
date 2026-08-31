//! Immutable snapshot publication and resumable installation.

use crate::{AdapterError, SnapshotChunk, SnapshotGeneration, SnapshotManifest};
use aequora_types::SnapshotId;
use async_trait::async_trait;

/// Snapshot creation, publication, resume, and atomic activation semantics.
#[async_trait]
pub trait SnapshotStore: Send + Sync {
    /// Creates a manifest bound to one consistent state/journal boundary.
    async fn create_manifest(&self) -> Result<SnapshotManifest, AdapterError>;

    /// Loads one bounded immutable chunk.
    async fn chunk(
        &self,
        snapshot_id: SnapshotId,
        index: u32,
    ) -> Result<Option<SnapshotChunk>, AdapterError>;

    /// Publishes a fully verified immutable generation.
    async fn publish(&self, manifest: &SnapshotManifest) -> Result<(), AdapterError>;

    /// Stages a verified chunk idempotently for resumable install.
    async fn stage_chunk(&self, chunk: SnapshotChunk) -> Result<(), AdapterError>;

    /// Atomically activates a complete verified generation.
    async fn activate(&self, generation: SnapshotGeneration) -> Result<(), AdapterError>;
}

/// Compile-time marker for atomic verified-generation activation.
pub trait SupportsAtomicSnapshotActivation: SnapshotStore {}
