//! Provider-neutral desktop update safety contracts.

use serde::{Deserialize, Serialize};
use thiserror::Error;

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
}
