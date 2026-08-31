//! Database-encryption key indirection, resumable rotation, and cryptographic erase contracts.
use aequora_storage_core::LocalStorageError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SecureProvider {
    AndroidKeystore,
    IosKeychain,
    WindowsDpapi,
    MacOsKeychain,
    LinuxSecretService,
    UserPassphrase,
    EnterpriseProvider,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WrappedKeyRef {
    pub key_id: String,
    pub provider: SecureProvider,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RotationPhase {
    Prepared,
    Reencrypting,
    Verified,
    Activated,
    Retired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RotationCheckpoint {
    pub old_key_id: String,
    pub new_key: WrappedKeyRef,
    pub phase: RotationPhase,
    pub last_chunk: u64,
}

impl RotationCheckpoint {
    /// Checkpoints a monotonic, resumable key rotation.
    ///
    /// # Errors
    /// Returns a transaction error when phases regress or skip required verification.
    pub fn transition(
        &mut self,
        next: RotationPhase,
        last_chunk: u64,
    ) -> Result<(), LocalStorageError> {
        let valid = matches!(
            (self.phase, next),
            (RotationPhase::Prepared, RotationPhase::Reencrypting)
                | (
                    RotationPhase::Reencrypting,
                    RotationPhase::Reencrypting | RotationPhase::Verified
                )
                | (RotationPhase::Verified, RotationPhase::Activated)
                | (RotationPhase::Activated, RotationPhase::Retired)
        );
        if !valid || last_chunk < self.last_chunk {
            return Err(LocalStorageError::TransactionFailed);
        }
        self.phase = next;
        self.last_chunk = last_chunk;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EraseDisposition {
    DestroyKey,
    RefuseLegalHold,
}

#[must_use]
pub const fn cryptographic_erase(legal_hold: bool) -> EraseDisposition {
    if legal_hold {
        EraseDisposition::RefuseLegalHold
    } else {
        EraseDisposition::DestroyKey
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_cannot_activate_before_verification() {
        let mut r = RotationCheckpoint {
            old_key_id: "old".into(),
            new_key: WrappedKeyRef {
                key_id: "new".into(),
                provider: SecureProvider::AndroidKeystore,
                generation: 2,
            },
            phase: RotationPhase::Prepared,
            last_chunk: 0,
        };
        assert!(r.transition(RotationPhase::Activated, 0).is_err());
    }
}
