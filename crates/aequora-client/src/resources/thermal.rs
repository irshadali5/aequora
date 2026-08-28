use serde::{Deserialize, Serialize};

use super::context::ThermalState;

/// Policy for CPU-heavy work under reliable platform thermal signals.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientThermalPolicy {
    pub defer_maintenance_when_hot: bool,
    pub force_single_worker_when_warm: bool,
}

impl ClientThermalPolicy {
    /// Whether optional CPU-heavy maintenance should pause.
    #[must_use]
    pub fn defer_maintenance(self, state: ThermalState) -> bool {
        self.defer_maintenance_when_hot
            && matches!(state, ThermalState::Hot | ThermalState::Critical)
    }

    /// Caps the configured worker count without ever increasing it.
    #[must_use]
    pub fn workers(self, configured: u8, state: ThermalState) -> u8 {
        if state == ThermalState::Critical
            || (self.force_single_worker_when_warm
                && matches!(state, ThermalState::Warm | ThermalState::Hot))
        {
            1
        } else {
            configured.max(1)
        }
    }
}
