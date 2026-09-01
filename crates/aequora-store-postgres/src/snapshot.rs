//! Snapshot publication metadata. Chunk bytes remain in the artifact store.

/// Publication state for immutable bootstrap metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostgresSnapshotPublication {
    /// Metadata and chunks are still being staged.
    Staged,
    /// Manifest/chunks passed integrity verification.
    Verified,
    /// The immutable generation is visible to clients.
    Published,
    /// The generation is no longer offered for bootstrap.
    Retired,
}
