//! Backup, restore, clone, and secure-key validation contracts.

/// Availability of the platform-protected device credential.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecureKeyState {
    Available,
    Missing,
    Mismatched,
}

/// Required action after opening a restored or copied store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreDisposition {
    Resume,
    RebindDevice,
    RecoveryRequired,
}

/// Fails closed when a copied database could silently reuse device credentials.
#[must_use]
pub const fn validate_restore_binding(
    stored_binding_generation: u64,
    expected_binding_generation: u64,
    key_state: SecureKeyState,
) -> RestoreDisposition {
    if stored_binding_generation != expected_binding_generation
        || !matches!(key_state, SecureKeyState::Available)
    {
        RestoreDisposition::RebindDevice
    } else {
        RestoreDisposition::Resume
    }
}
