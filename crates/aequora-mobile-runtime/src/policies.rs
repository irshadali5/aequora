use crate::{AppLifecycle, MobileResourceContext, MobileSyncBudget, StoragePressure, ThermalState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScopeCachePolicy {
    Required,
    Recent,
    OnDemand,
    NeverPersist,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TransferKind {
    SecurityMetadata,
    StructuredSync,
    Snapshot,
    Blob,
    Maintenance,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SyncClass {
    Foreground,
    Background,
    LowPower,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyDecision {
    Run(MobileSyncBudget),
    WaitForNetwork,
    WaitForBudget,
    WaitForCredentials,
    DeferForResources,
    StorageUnavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MobilePolicy {
    pub foreground: MobileSyncBudget,
    pub background: MobileSyncBudget,
    pub low_power: MobileSyncBudget,
    pub allow_metered_metadata: bool,
    pub allow_metered_blobs: bool,
    pub allow_roaming_large_transfers: bool,
}

impl Default for MobilePolicy {
    fn default() -> Self {
        Self {
            foreground: MobileSyncBudget::FOREGROUND,
            background: MobileSyncBudget::BACKGROUND,
            low_power: MobileSyncBudget::LOW_POWER,
            allow_metered_metadata: true,
            allow_metered_blobs: false,
            allow_roaming_large_transfers: false,
        }
    }
}

impl MobilePolicy {
    /// Selects throughput only. Authorization, durability, conflict, and audit semantics are not
    /// represented here and therefore cannot be relaxed by resource input.
    #[must_use]
    pub fn decide(
        self,
        context: MobileResourceContext,
        transfer: TransferKind,
        granted: Option<MobileSyncBudget>,
    ) -> PolicyDecision {
        if matches!(
            context.storage,
            StoragePressure::Critical | StoragePressure::Unavailable
        ) {
            return PolicyDecision::StorageUnavailable;
        }
        if !context.network.available() {
            return PolicyDecision::WaitForNetwork;
        }
        if !context.secure_keys_available {
            return PolicyDecision::WaitForCredentials;
        }
        if matches!(
            context.lifecycle,
            AppLifecycle::Suspended | AppLifecycle::Terminating
        ) {
            return PolicyDecision::WaitForBudget;
        }
        let large = matches!(transfer, TransferKind::Snapshot | TransferKind::Blob);
        if large
            && ((context.network.metered() && !self.allow_metered_blobs)
                || (context.network.roaming() && !self.allow_roaming_large_transfers)
                || context.network.expensive()
                || context.network.constrained()
                || matches!(
                    context.power.thermal,
                    ThermalState::Serious | ThermalState::Critical
                ))
        {
            return PolicyDecision::DeferForResources;
        }
        if transfer == TransferKind::SecurityMetadata
            && context.network.metered()
            && !self.allow_metered_metadata
        {
            return PolicyDecision::DeferForResources;
        }
        let configured = if context.power.low_power_mode {
            self.low_power
        } else if context.lifecycle == AppLifecycle::Foreground {
            self.foreground
        } else {
            self.background
        };
        PolicyDecision::Run(granted.map_or(configured, |value| configured.constrained_by(value)))
    }

    #[must_use]
    pub const fn may_evict_scope(
        cache_policy: ScopeCachePolicy,
        pending_intent: bool,
        pinned_offline: bool,
    ) -> bool {
        !pending_intent && !pinned_offline && !matches!(cache_policy, ScopeCachePolicy::Required)
    }
}
