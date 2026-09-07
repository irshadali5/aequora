//! Provider-neutral desktop update safety contracts.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use aequora_release::{AutomaticUpdatePolicy, ReleaseChannel, RollbackClass};

const MAX_ID_BYTES: usize = 512;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpdateManifest {
    pub build_id: String,
    pub artifact_digest: [u8; 32],
    pub signature: Vec<u8>,
    pub minimum_store_version: u32,
    pub resulting_store_version: u32,
    pub minimum_ipc_version: u16,
    pub maximum_ipc_version: u16,
}

impl UpdateManifest {
    /// Validates a signed update manifest before provider-specific verification.
    ///
    /// # Errors
    ///
    /// Rejects malformed identities, signatures, or compatibility windows.
    pub fn validate(&self) -> Result<(), UpdateError> {
        if self.build_id.is_empty()
            || self.build_id.len() > MAX_ID_BYTES
            || self.build_id.chars().any(char::is_control)
            || self.artifact_digest == [0; 32]
            || self.signature.is_empty()
            || self.signature.len() > 16 * 1024
            || self.minimum_store_version > self.resulting_store_version
            || self.minimum_ipc_version > self.maximum_ipc_version
        {
            return Err(UpdateError::InvalidManifest);
        }
        Ok(())
    }

    /// Verifies both the artifact digest and the deployment provider signature.
    ///
    /// # Errors
    ///
    /// Rejects malformed manifests, digest mismatches, and invalid signatures.
    pub fn verify(
        &self,
        artifact: &[u8],
        verifier: &dyn UpdateVerifier,
    ) -> Result<(), UpdateError> {
        self.validate()?;
        if blake3::hash(artifact).as_bytes() != &self.artifact_digest {
            return Err(UpdateError::DigestMismatch);
        }
        if !verifier.verify_signature(self, artifact) {
            return Err(UpdateError::InvalidSignature);
        }
        Ok(())
    }
}

pub trait UpdateVerifier: Send + Sync {
    fn verify_signature(&self, manifest: &UpdateManifest, artifact: &[u8]) -> bool;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum UpdateStage {
    Downloaded,
    Verified,
    Quiesced,
    Checkpointed,
    Installed,
    Migrated,
    Resumed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpdateState {
    pub stage: UpdateStage,
    pub pending_operations: u64,
    pub store_version: u32,
}

impl UpdateState {
    /// Advances the update only through the safe sequence and never across pending-intent loss.
    ///
    /// # Errors
    ///
    /// Rejects skipped stages and migration states that would discard pending operations.
    pub const fn advance(self, next: UpdateStage, pending_after: u64) -> Result<Self, UpdateError> {
        let valid = matches!(
            (self.stage, next),
            (UpdateStage::Downloaded, UpdateStage::Verified)
                | (UpdateStage::Verified, UpdateStage::Quiesced)
                | (UpdateStage::Quiesced, UpdateStage::Checkpointed)
                | (UpdateStage::Checkpointed, UpdateStage::Installed)
                | (UpdateStage::Installed, UpdateStage::Migrated)
                | (UpdateStage::Migrated, UpdateStage::Resumed)
        );
        if !valid {
            return Err(UpdateError::UnsafeTransition);
        }
        if pending_after < self.pending_operations {
            return Err(UpdateError::PendingIntentLost);
        }
        Ok(Self {
            stage: next,
            pending_operations: pending_after,
            store_version: self.store_version,
        })
    }

    #[must_use]
    pub const fn rollback_safe(self, older_max_store_version: u32) -> bool {
        self.store_version <= older_max_store_version
    }
}

/// Durable client state captured immediately before an atomic desktop replacement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpgradeCheckpoint {
    pub pending_operations: u64,
    pub cursor: u64,
    pub conflict_count: u64,
    pub store_identity: [u8; 16],
    pub durable_intent_digest: [u8; 32],
    pub store_version: u32,
}

impl UpgradeCheckpoint {
    fn validate(self) -> Result<(), UpdateError> {
        if self.store_identity == [0; 16]
            || self.durable_intent_digest == [0; 32]
            || self.store_version == 0
        {
            return Err(UpdateError::InvalidCheckpoint);
        }
        Ok(())
    }

    fn preserves_intent(self, after: Self) -> bool {
        self.pending_operations == after.pending_operations
            && self.cursor == after.cursor
            && self.conflict_count == after.conflict_count
            && self.store_identity == after.store_identity
            && self.durable_intent_digest == after.durable_intent_digest
    }
}

/// Full atomic-update state machine used by Part 44 delivery adapters.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AtomicUpdateState {
    pub stage: UpdateStage,
    pub checkpoint: UpgradeCheckpoint,
}

impl AtomicUpdateState {
    /// Advances exactly one update stage while preserving durable intent and state identity.
    ///
    /// Only the migration step may change the store format, and it must move forward.
    ///
    /// # Errors
    ///
    /// Rejects skipped stages, invalid checkpoints, lost intent, identity drift, cursor/conflict
    /// changes, or store-format changes outside the migration step.
    pub fn advance(self, next: UpdateStage, after: UpgradeCheckpoint) -> Result<Self, UpdateError> {
        self.checkpoint.validate()?;
        after.validate()?;
        let valid = matches!(
            (self.stage, next),
            (UpdateStage::Downloaded, UpdateStage::Verified)
                | (UpdateStage::Verified, UpdateStage::Quiesced)
                | (UpdateStage::Quiesced, UpdateStage::Checkpointed)
                | (UpdateStage::Checkpointed, UpdateStage::Installed)
                | (UpdateStage::Installed, UpdateStage::Migrated)
                | (UpdateStage::Migrated, UpdateStage::Resumed)
        );
        if !valid {
            return Err(UpdateError::UnsafeTransition);
        }
        if !self.checkpoint.preserves_intent(after) {
            return Err(UpdateError::DurableStateChanged);
        }
        if next == UpdateStage::Migrated {
            if after.store_version < self.checkpoint.store_version {
                return Err(UpdateError::UnsafeStoreMigration);
            }
        } else if after.store_version != self.checkpoint.store_version {
            return Err(UpdateError::UnsafeStoreMigration);
        }
        Ok(Self {
            stage: next,
            checkpoint: after,
        })
    }

    /// Checks whether the selected rollback policy and old binary format permit a rollback.
    ///
    /// # Errors
    ///
    /// Rejects forward-only state and rollback-migration requirements without evidence.
    pub const fn authorize_rollback(
        self,
        class: RollbackClass,
        older_max_store_version: u32,
        rollback_migration_verified: bool,
    ) -> Result<(), UpdateError> {
        match class {
            RollbackClass::ForwardOnly => Err(UpdateError::RollbackRejected),
            RollbackClass::RollbackRequiresMigration if !rollback_migration_verified => {
                Err(UpdateError::RollbackMigrationRequired)
            }
            RollbackClass::BinaryRollbackSafe | RollbackClass::RollbackRequiresMigration => {
                if self.checkpoint.store_version <= older_max_store_version {
                    Ok(())
                } else {
                    Err(UpdateError::RollbackRejected)
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum UpdateError {
    #[error("desktop update manifest is invalid")]
    InvalidManifest,
    #[error("desktop update artifact digest does not match the manifest")]
    DigestMismatch,
    #[error("desktop update signature is invalid")]
    InvalidSignature,
    #[error("desktop update transition is unsafe")]
    UnsafeTransition,
    #[error("desktop update would lose pending user intent")]
    PendingIntentLost,
    #[error("desktop update checkpoint is invalid")]
    InvalidCheckpoint,
    #[error("desktop update changed cursor, conflicts, store identity, or durable intent")]
    DurableStateChanged,
    #[error("desktop update changed the store format outside a forward migration")]
    UnsafeStoreMigration,
    #[error("desktop rollback is incompatible with the resulting state")]
    RollbackRejected,
    #[error("desktop rollback requires a verified rollback migration")]
    RollbackMigrationRequired,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_cannot_drop_pending_intent() {
        let state = UpdateState {
            stage: UpdateStage::Installed,
            pending_operations: 3,
            store_version: 2,
        };
        assert_eq!(
            state.advance(UpdateStage::Migrated, 2),
            Err(UpdateError::PendingIntentLost)
        );
    }

    #[test]
    fn atomic_upgrade_preserves_all_durable_state() {
        let checkpoint = UpgradeCheckpoint {
            pending_operations: 3,
            cursor: 41,
            conflict_count: 2,
            store_identity: [1; 16],
            durable_intent_digest: [2; 32],
            store_version: 4,
        };
        let installed = AtomicUpdateState {
            stage: UpdateStage::Installed,
            checkpoint,
        };
        let migrated = installed
            .advance(
                UpdateStage::Migrated,
                UpgradeCheckpoint {
                    store_version: 5,
                    ..checkpoint
                },
            )
            .unwrap_or_else(|error| panic!("safe migration failed: {error}"));
        assert_eq!(migrated.checkpoint.pending_operations, 3);
        assert_eq!(migrated.checkpoint.cursor, 41);

        let changed_cursor = UpgradeCheckpoint {
            cursor: 42,
            ..checkpoint
        };
        assert_eq!(
            installed.advance(UpdateStage::Migrated, changed_cursor),
            Err(UpdateError::DurableStateChanged)
        );
    }

    #[test]
    fn forward_only_release_cannot_roll_back() {
        let state = AtomicUpdateState {
            stage: UpdateStage::Resumed,
            checkpoint: UpgradeCheckpoint {
                pending_operations: 0,
                cursor: 41,
                conflict_count: 0,
                store_identity: [1; 16],
                durable_intent_digest: [2; 32],
                store_version: 5,
            },
        };
        assert_eq!(
            state.authorize_rollback(RollbackClass::ForwardOnly, 5, true),
            Err(UpdateError::RollbackRejected)
        );
    }
}
