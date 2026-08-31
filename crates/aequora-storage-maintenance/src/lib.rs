//! Bounded, checkpointed maintenance that preserves critical local intent.
use aequora_storage_core::{LocalStorageError, StorageClass, StoragePressure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MaintenanceKind {
    IntegrityCheck,
    DerivedRebuild,
    BlobGc,
    Compaction,
    Migration,
    KeyRotation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MaintenanceOpportunity {
    MobileChargingWindow,
    MobileUnavailable,
    DesktopIdle,
    DesktopActive,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MaintenanceContext {
    pub opportunity: MaintenanceOpportunity,
    pub pressure: StoragePressure,
}

impl MaintenanceContext {
    #[must_use]
    pub const fn permits(self, kind: MaintenanceKind) -> bool {
        if matches!(
            self.pressure,
            StoragePressure::Critical | StoragePressure::ReadMostly
        ) {
            return matches!(kind, MaintenanceKind::IntegrityCheck);
        }
        matches!(
            self.opportunity,
            MaintenanceOpportunity::MobileChargingWindow | MaintenanceOpportunity::DesktopIdle
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationCheckpoint {
    pub migration_id: String,
    pub phase: u32,
    pub last_key: Option<Vec<u8>>,
    pub generation: u64,
}

impl MigrationCheckpoint {
    /// Advances one durable migration phase.
    ///
    /// # Errors
    /// Returns an error when a stale generation attempts to continue.
    pub fn advance(
        &mut self,
        generation: u64,
        phase: u32,
        last_key: Option<Vec<u8>>,
    ) -> Result<(), LocalStorageError> {
        if generation != self.generation || phase < self.phase {
            return Err(LocalStorageError::MigrationRequired);
        }
        self.phase = phase;
        self.last_key = last_key;
        Ok(())
    }
}

#[must_use]
pub const fn cleanup_allowed(class: StorageClass, published: bool, grace_elapsed: bool) -> bool {
    class.automatically_evictable() && (!published || grace_elapsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_preserves_intent() {
        assert!(!cleanup_allowed(StorageClass::CriticalIntent, false, true));
    }
    #[test]
    fn stale_migration_cannot_advance() {
        let mut c = MigrationCheckpoint {
            migration_id: "m".into(),
            phase: 1,
            last_key: None,
            generation: 2,
        };
        assert!(c.advance(1, 2, None).is_err());
    }
}
