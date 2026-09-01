//! Stable adapter-local diagnostic categories.

/// Payload-free local failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoolapFailureCode {
    StorageCritical,
    MigrationRequired,
    RecoveryRequired,
    LeadershipLost,
    CorruptPayload,
    DeviceRebindingRequired,
}
