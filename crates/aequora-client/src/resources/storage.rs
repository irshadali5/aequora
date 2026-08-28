use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::context::StorageState;

/// Local snapshot artifact retention after a verified chunk is installed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SnapshotCachePolicy {
    None,
    InstalledOnly,
    KeepRecent,
}

/// Persistence policy for one authorized synchronization scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScopeCachePolicy {
    Required,
    Recent,
    OnDemand,
    NeverPersist,
}

/// Storage settings independent from the selected embedded database adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoragePolicy {
    pub low_watermark_bytes: u64,
    pub critical_watermark_bytes: u64,
    pub transaction_reserve_bytes: u64,
    pub blob_spool_quota_bytes: u64,
    pub snapshot_cache: SnapshotCachePolicy,
}

impl StoragePolicy {
    pub(crate) const fn valid(self) -> bool {
        self.critical_watermark_bytes > 0
            && self.low_watermark_bytes > self.critical_watermark_bytes
            && self.transaction_reserve_bytes > 0
            && self.blob_spool_quota_bytes > 0
    }
}

/// Coarse local asset class used to produce deterministic eviction order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StorageAssetKind {
    TransientLog,
    RebuildableUiCache,
    InstalledSnapshotChunk,
    DerivedSearchIndex,
    OptionalScope,
    ResolvedConflictDetails,
    ThumbnailOrMediaCache,
    RequiredScopeBase,
    UnsyncedOutbox,
    PendingBlob,
    CryptoKeyMetadata,
    CursorMetadata,
    RequiredTombstone,
}

impl StorageAssetKind {
    const fn eviction_rank(self) -> Option<u8> {
        match self {
            Self::TransientLog => Some(0),
            Self::RebuildableUiCache | Self::ThumbnailOrMediaCache => Some(1),
            Self::InstalledSnapshotChunk => Some(2),
            Self::DerivedSearchIndex => Some(3),
            Self::OptionalScope => Some(4),
            Self::ResolvedConflictDetails => Some(5),
            Self::RequiredScopeBase
            | Self::UnsyncedOutbox
            | Self::PendingBlob
            | Self::CryptoKeyMetadata
            | Self::CursorMetadata
            | Self::RequiredTombstone => None,
        }
    }
}

/// One adapter/application-owned candidate. The planner returns IDs and never deletes directly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvictionCandidate {
    pub asset_id: u64,
    pub kind: StorageAssetKind,
    pub bytes: u64,
    /// Pending operations or required cross-scope references pin this asset.
    pub pending_intent_pinned: bool,
    /// Required for optional scope/conflict retention decisions owned by the application.
    pub application_allows_eviction: bool,
}

/// Why a candidate remains retained during storage-pressure planning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvictionReason {
    AuthoritativeOrDurable,
    PendingIntentPinned,
    ApplicationPolicy,
    TargetAlreadyMet,
}

/// Deterministic, non-mutating eviction proposal.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EvictionPlan {
    pub evict_asset_ids: Vec<u64>,
    pub retained: Vec<(u64, EvictionReason)>,
    pub reclaimable_bytes: u64,
    pub target_bytes: u64,
}

impl EvictionPlan {
    /// Plans lowest-value rebuildable data first and never selects protected or pinned data.
    #[must_use]
    pub fn build(candidates: &[EvictionCandidate], target_bytes: u64) -> Self {
        let mut ordered = candidates.to_vec();
        ordered.sort_by_key(|candidate| {
            (
                candidate.kind.eviction_rank().unwrap_or(u8::MAX),
                candidate.asset_id,
            )
        });
        let mut plan = Self {
            target_bytes,
            ..Self::default()
        };
        for candidate in ordered {
            let reason = if candidate.kind.eviction_rank().is_none() {
                Some(EvictionReason::AuthoritativeOrDurable)
            } else if candidate.pending_intent_pinned {
                Some(EvictionReason::PendingIntentPinned)
            } else if !candidate.application_allows_eviction {
                Some(EvictionReason::ApplicationPolicy)
            } else if plan.reclaimable_bytes >= target_bytes {
                Some(EvictionReason::TargetAlreadyMet)
            } else {
                None
            };
            if let Some(reason) = reason {
                plan.retained.push((candidate.asset_id, reason));
            } else {
                plan.evict_asset_ids.push(candidate.asset_id);
                plan.reclaimable_bytes = plan.reclaimable_bytes.saturating_add(candidate.bytes);
            }
        }
        plan
    }
}

/// Result of estimating disk requirements before a large local operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoragePreflightDecision {
    Proceed,
    ProceedGuarded,
    RejectUntilSpaceAvailable,
}

/// Snapshot/blob/import preflight with an explicit durable transaction reserve.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoragePreflight {
    pub available_bytes: u64,
    pub estimated_payload_bytes: u64,
    pub staging_overhead_bytes: u64,
}

impl StoragePreflight {
    /// Ensures large work cannot consume the transaction reserve needed by outbox/cursor commits.
    #[must_use]
    pub fn decide(self, policy: StoragePolicy, state: StorageState) -> StoragePreflightDecision {
        if matches!(state, StorageState::Critical | StorageState::ReadOnlyRisk) {
            return StoragePreflightDecision::RejectUntilSpaceAvailable;
        }
        let required = self
            .estimated_payload_bytes
            .saturating_add(self.staging_overhead_bytes)
            .saturating_add(policy.transaction_reserve_bytes);
        if self.available_bytes < required {
            StoragePreflightDecision::RejectUntilSpaceAvailable
        } else if state == StorageState::Low
            || self.available_bytes.saturating_sub(required) < policy.low_watermark_bytes
        {
            StoragePreflightDecision::ProceedGuarded
        } else {
            StoragePreflightDecision::Proceed
        }
    }
}

/// Proof returned only for an atomic local mutation plus outbox commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalCommitReceipt {
    transaction_sequence: u64,
}

impl LocalCommitReceipt {
    /// Creates a save receipt only when domain state and durable outbox intent both committed.
    ///
    /// # Errors
    ///
    /// Returns a typed error instead of permitting a false "saved locally" status.
    pub const fn from_atomic_commit(
        transaction_sequence: u64,
        domain_committed: bool,
        outbox_committed: bool,
    ) -> Result<Self, LocalCommitReceiptError> {
        if transaction_sequence == 0 || !domain_committed || !outbox_committed {
            return Err(LocalCommitReceiptError::AtomicCommitNotProven);
        }
        Ok(Self {
            transaction_sequence,
        })
    }

    /// Adapter-owned durable transaction sequence used only for diagnostics.
    #[must_use]
    pub const fn transaction_sequence(self) -> u64 {
        self.transaction_sequence
    }
}

/// Failure to prove local-first atomic durability.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum LocalCommitReceiptError {
    #[error("domain mutation and outbox append did not commit atomically")]
    AtomicCommitNotProven,
}

/// Version of the durable local store format, independent from application schema versions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LocalStoreFormatVersion(u32);

impl LocalStoreFormatVersion {
    /// Creates a non-zero format version.
    #[must_use]
    pub const fn new(version: u32) -> Option<Self> {
        if version == 0 {
            None
        } else {
            Some(Self(version))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Fails closed for an older executable opening a newer incompatible format.
    #[must_use]
    pub const fn open_decision(self, supported: Self) -> StoreOpenDecision {
        if self.0 > supported.0 {
            StoreOpenDecision::RejectNewerFormat
        } else if self.0 < supported.0 {
            StoreOpenDecision::MigrationRequired
        } else {
            StoreOpenDecision::Open
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreOpenDecision {
    Open,
    MigrationRequired,
    RejectNewerFormat,
}

/// Cache retention selected after storage-state and scope policy are combined.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheRetention {
    Retain,
    EvictWhenUnused,
    TransientOnly,
}

impl ScopeCachePolicy {
    /// Required scopes remain durable; recent/on-demand scopes can shrink only when unpinned.
    #[must_use]
    pub fn retention(self, storage: StorageState, pending_pinned: bool) -> CacheRetention {
        if pending_pinned || matches!(self, Self::Required) {
            return CacheRetention::Retain;
        }
        match self {
            Self::Recent if storage == StorageState::Healthy => CacheRetention::Retain,
            Self::Recent | Self::OnDemand => CacheRetention::EvictWhenUnused,
            Self::NeverPersist => CacheRetention::TransientOnly,
            Self::Required => CacheRetention::Retain,
        }
    }
}
