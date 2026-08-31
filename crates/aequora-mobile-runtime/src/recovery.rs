use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreVersions {
    pub readable_min: u32,
    pub writable_current: u32,
    pub on_disk: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StoreOpenDisposition {
    Current,
    Migrate { from: u32, to: u32 },
    RefuseDowngrade,
    UnsupportedLegacy,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoverySnapshot {
    pub stale_in_flight_operations: u64,
    pub pending_outbox_operations: u64,
    pub interrupted_bootstrap_generation: Option<u64>,
    pub durable_cursor: Option<u64>,
    pub applied_through: Option<u64>,
    pub scheduler_backoff_until_ms: Option<u64>,
    pub device_binding_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    OpenCurrent,
    RunMigration { from: u32, to: u32 },
    RecoverInFlight,
    ResumeBootstrap { generation: u64 },
    ResumeCoordinator,
}

/// Computes fail-closed startup order from durable metadata.
///
/// # Errors
///
/// Rejects downgrade, unsupported stores, cursor-ahead corruption, and invalid identities.
pub fn classify_startup(
    versions: StoreVersions,
    snapshot: RecoverySnapshot,
) -> Result<Vec<RecoveryAction>, RecoveryError> {
    if versions.readable_min == 0
        || versions.writable_current < versions.readable_min
        || versions.on_disk == 0
        || snapshot.device_binding_generation == 0
    {
        return Err(RecoveryError::InvalidMetadata);
    }
    if snapshot
        .durable_cursor
        .zip(snapshot.applied_through)
        .is_some_and(|(cursor, applied)| cursor > applied)
    {
        return Err(RecoveryError::CursorAheadOfDurability);
    }
    if versions.on_disk > versions.writable_current {
        return Err(RecoveryError::DowngradeRefused);
    }
    if versions.on_disk < versions.readable_min {
        return Err(RecoveryError::UnsupportedLegacyStore);
    }
    let mut actions = Vec::new();
    if versions.on_disk == versions.writable_current {
        actions.push(RecoveryAction::OpenCurrent);
    } else {
        actions.push(RecoveryAction::RunMigration {
            from: versions.on_disk,
            to: versions.writable_current,
        });
    }
    if snapshot.stale_in_flight_operations > 0 {
        actions.push(RecoveryAction::RecoverInFlight);
    }
    if let Some(generation) = snapshot.interrupted_bootstrap_generation {
        actions.push(RecoveryAction::ResumeBootstrap { generation });
    }
    actions.push(RecoveryAction::ResumeCoordinator);
    Ok(actions)
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RecoveryError {
    #[error("mobile store metadata is invalid")]
    InvalidMetadata,
    #[error("an older app cannot open a newer mobile store")]
    DowngradeRefused,
    #[error("mobile store is older than the supported migration floor")]
    UnsupportedLegacyStore,
    #[error("cursor is ahead of durably applied state")]
    CursorAheadOfDurability,
    #[error("durable recovery store failed")]
    Store,
}

#[async_trait]
pub trait MobileRecoveryStore: Send + Sync {
    async fn inspect(&self) -> Result<(StoreVersions, RecoverySnapshot), RecoveryError>;
    async fn recover_stale_in_flight(&self) -> Result<(), RecoveryError>;
    async fn migrate_preserving_intent(&self, from: u32, to: u32) -> Result<(), RecoveryError>;
    async fn checkpoint_budget_expiration(&self) -> Result<(), RecoveryError>;
}
