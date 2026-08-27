use crate::{E2eScheme, PayloadProtectionMode, ProtectedPayloadError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct E2eKeyEpoch(pub u64);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ServerPlaintextUse {
    Validation,
    ConflictMerge,
    Search,
    ScopeFiltering,
    Analytics,
    ContentAuthorization,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServerPlaintextResponsibility {
    pub uses: BTreeSet<ServerPlaintextUse>,
}

impl ServerPlaintextResponsibility {
    #[must_use]
    pub fn requires_plaintext(&self) -> bool {
        !self.uses.is_empty()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtectedDomainPolicy {
    pub mode: PayloadProtectionMode,
    pub server_responsibility: ServerPlaintextResponsibility,
    pub append_only_or_whole_value: bool,
}

impl ProtectedDomainPolicy {
    /// Validates that server responsibilities remain possible in the selected protection mode.
    ///
    /// # Errors
    ///
    /// Returns [`ProtectedPayloadError`] if client-managed E2E hides required semantics.
    pub fn validate(&self) -> Result<(), ProtectedPayloadError> {
        if self.mode == PayloadProtectionMode::ClientManagedE2E
            && (self.server_responsibility.requires_plaintext() || !self.append_only_or_whole_value)
        {
            return Err(ProtectedPayloadError::ServerRequiresPlaintext);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtectedPayload {
    pub scheme: E2eScheme,
    pub key_epoch: E2eKeyEpoch,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl ProtectedPayload {
    /// Validates mandatory envelope fields without interpreting opaque ciphertext.
    ///
    /// # Errors
    ///
    /// Returns [`ProtectedPayloadError`] for an empty epoch, nonce, or ciphertext.
    pub fn validate_structure(&self) -> Result<(), ProtectedPayloadError> {
        if self.key_epoch.0 == 0 || self.nonce.is_empty() || self.ciphertext.is_empty() {
            return Err(ProtectedPayloadError::InvalidEnvelope);
        }
        Ok(())
    }

    /// Requires key epoch advancement after group membership changes.
    ///
    /// # Errors
    ///
    /// Returns [`ProtectedPayloadError`] when `next` does not exceed `previous`.
    pub fn validate_membership_rotation(
        previous: E2eKeyEpoch,
        next: E2eKeyEpoch,
    ) -> Result<(), ProtectedPayloadError> {
        if next <= previous {
            return Err(ProtectedPayloadError::EpochDidNotAdvance);
        }
        Ok(())
    }
}
