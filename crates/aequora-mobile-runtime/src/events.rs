use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MobileStatus {
    SavedLocally,
    WaitingForNetwork,
    Syncing,
    ServerConfirmed,
    Conflict,
    NeedsRebootstrap,
    UpgradeRequired,
    StorageCritical,
    AuthenticationRequired,
}

/// Coarse events invalidate local views without exposing raw journal records.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MobileEvent {
    SyncStatusChanged(MobileStatus),
    ConflictCreated { conflict_id: String },
    BootstrapProgress { completed: u64, total: Option<u64> },
    DataChanged { scope: String },
    AuthenticationRequired,
    UpgradeRequired { read_only_allowed: bool },
}
