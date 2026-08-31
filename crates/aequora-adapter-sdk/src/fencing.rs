//! Lease ownership and monotonic fencing semantics.

use crate::AdapterError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Monotonic adapter-level fencing token.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct FencingToken(pub u64);

/// Active lease returned after acquisition or renewal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FencingLease {
    /// Opaque owner identifier.
    pub owner_id: String,
    /// Monotonic token that every protected write must verify.
    pub token: FencingToken,
    /// Lease lifetime granted by the store.
    pub lease_for: Duration,
}

/// Store-backed lease and fencing capability.
#[async_trait]
pub trait FencingStore: Send + Sync {
    /// Acquires ownership or takes over an expired lease with a greater token.
    async fn acquire(
        &self,
        owner_id: &str,
        lease_for: Duration,
    ) -> Result<FencingLease, AdapterError>;

    /// Renews only when owner and fencing token remain current.
    async fn renew(&self, lease: &FencingLease) -> Result<FencingLease, AdapterError>;

    /// Rejects stale writers before they mutate protected state.
    async fn verify_owner(&self, lease: &FencingLease) -> Result<(), AdapterError>;
}

/// Compile-time composition marker for fencing semantics.
pub trait SupportsFencing: FencingStore {}
