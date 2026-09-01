use aequora_client::ClientSdkSyncStatus;

/// Coarse, advisory status intended for UI presentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SyncStatus {
    Dormant,
    Connecting,
    Syncing,
    Reconciling,
    Idle,
    Pending { operations: u64 },
    Backoff,
    Offline,
    Paused,
    NeedsAttention,
    AuthenticationRequired,
    UpgradeRequired,
    NeedsBootstrap,
    StorageBlocked,
    Closed,
}

impl From<ClientSdkSyncStatus> for SyncStatus {
    fn from(status: ClientSdkSyncStatus) -> Self {
        match status {
            ClientSdkSyncStatus::Offline => Self::Offline,
            ClientSdkSyncStatus::Idle => Self::Idle,
            ClientSdkSyncStatus::Pending { operations } => Self::Pending { operations },
            ClientSdkSyncStatus::Syncing => Self::Syncing,
            ClientSdkSyncStatus::NeedsBootstrap => Self::NeedsBootstrap,
            ClientSdkSyncStatus::AuthenticationRequired => Self::AuthenticationRequired,
            ClientSdkSyncStatus::UpgradeRequired => Self::UpgradeRequired,
            ClientSdkSyncStatus::StorageBlocked => Self::StorageBlocked,
            ClientSdkSyncStatus::Closed => Self::Closed,
            _ => Self::NeedsAttention,
        }
    }
}
