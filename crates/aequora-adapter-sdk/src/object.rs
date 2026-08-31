//! Immutable object storage capability for snapshots and blobs.

use crate::{AdapterError, Digest};
use async_trait::async_trait;

/// Bounded object metadata returned without loading content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectMetadata {
    /// Immutable content size.
    pub bytes: u64,
    /// Expected content digest.
    pub digest: Digest,
}

/// Object storage remains an artifact store and never an authoritative executor.
#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// Publishes a new immutable object and rejects conflicting replacement.
    async fn put_immutable(
        &self,
        key: &str,
        bytes: &[u8],
        digest: Digest,
    ) -> Result<(), AdapterError>;

    /// Loads at most `length` bytes from an object range.
    async fn get_range(
        &self,
        key: &str,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, AdapterError>;

    /// Reads metadata without downloading content.
    async fn head(&self, key: &str) -> Result<Option<ObjectMetadata>, AdapterError>;

    /// Deletes one object according to governance policy.
    async fn delete(&self, key: &str) -> Result<(), AdapterError>;

    /// Lists at most `limit` keys under one controlled prefix.
    async fn list_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<String>, AdapterError>;
}
