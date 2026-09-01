//! Consistent backup, corruption preservation, and restore contracts.

use std::path::PathBuf;

/// Result of preserving evidence from a damaged replica.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForensicCopy {
    /// Path containing the byte-for-byte damaged database copy.
    pub path: PathBuf,
    /// Number of copied bytes.
    pub bytes: u64,
}

/// Recovery decision produced by an integrity check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryState {
    /// `SQLite` reported a healthy database.
    Healthy,
    /// The damaged file was preserved and requires operator-directed restore/rebootstrap.
    RecoveryRequired(ForensicCopy),
}
