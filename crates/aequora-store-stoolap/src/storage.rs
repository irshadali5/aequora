//! Bounded local storage accounting and eviction policy.

/// Durability/eviction class for local bytes.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StorageClass {
    /// Local DB, pending intent, identity, cursor, and conflicts.
    Critical,
    /// Snapshots, integrity caches, and indexes reproducible from authority.
    Reconstructable,
    /// Blob cache, thumbnails, and temporary diagnostics/downloads.
    Evictable,
}

/// Result of a bounded storage preflight.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoragePreflight {
    /// Required staging plus safety margin fits.
    Admit,
    /// Evictable data must be reclaimed before retrying.
    ReclaimEvictable { bytes: u64 },
    /// The operation cannot safely make a durable commit.
    StorageCritical { missing_bytes: u64 },
}

/// Evaluates a large bootstrap/import without allowing critical intent eviction.
#[must_use]
pub const fn preflight_storage(
    available_bytes: u64,
    required_staging_bytes: u64,
    safety_margin_bytes: u64,
    reclaimable_evictable_bytes: u64,
) -> StoragePreflight {
    let required = required_staging_bytes.saturating_add(safety_margin_bytes);
    if available_bytes >= required {
        return StoragePreflight::Admit;
    }
    let missing = required - available_bytes;
    if reclaimable_evictable_bytes >= missing {
        StoragePreflight::ReclaimEvictable { bytes: missing }
    } else {
        StoragePreflight::StorageCritical {
            missing_bytes: missing.saturating_sub(reclaimable_evictable_bytes),
        }
    }
}
