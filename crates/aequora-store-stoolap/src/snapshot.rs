//! Bounded snapshot staging policy.

/// Limits applied while streaming snapshot chunks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotStagingPolicy {
    pub max_chunk_bytes: u64,
    pub max_entities_per_transaction: usize,
}
