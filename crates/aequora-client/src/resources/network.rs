use serde::{Deserialize, Serialize};

use super::context::NetworkContext;

/// Application policy for metered and roaming connectivity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientNetworkPolicy {
    pub allow_interactive_metered: bool,
    pub allow_bulk_metered: bool,
    pub allow_roaming_bulk: bool,
}

/// Optional metered transfer allowance. `None` means application policy has no byte budget.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DataBudget {
    pub daily_metered_bytes: Option<u64>,
    pub monthly_metered_bytes: Option<u64>,
}

/// Current privacy-local accounting for a configured data budget.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DataBudgetUsage {
    pub daily_metered_bytes: u64,
    pub monthly_metered_bytes: u64,
}

impl DataBudget {
    /// Returns whether an estimated metered transfer remains within both configured caps.
    #[must_use]
    pub fn permits(self, usage: DataBudgetUsage, additional_bytes: u64) -> bool {
        self.daily_metered_bytes
            .is_none_or(|limit| usage.daily_metered_bytes.saturating_add(additional_bytes) <= limit)
            && self.monthly_metered_bytes.is_none_or(|limit| {
                usage.monthly_metered_bytes.saturating_add(additional_bytes) <= limit
            })
    }
}

/// Coarse network class used by client admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkWorkClass {
    Critical,
    Interactive,
    Bulk,
    Optional,
}

/// Network-specific resource decision before power/storage policy is evaluated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkAdmission {
    Allow,
    DeferOffline,
    DeferForUnmetered,
    RequireUserApproval,
}

impl ClientNetworkPolicy {
    /// Applies network class and explicit user intent without treating connectivity as authority.
    #[must_use]
    pub fn admit(
        self,
        context: NetworkContext,
        class: NetworkWorkClass,
        user_initiated: bool,
    ) -> NetworkAdmission {
        if !context.online {
            return NetworkAdmission::DeferOffline;
        }
        if class == NetworkWorkClass::Critical {
            return NetworkAdmission::Allow;
        }
        if context.roaming == Some(true) && class == NetworkWorkClass::Bulk {
            return if self.allow_roaming_bulk {
                NetworkAdmission::Allow
            } else if user_initiated {
                NetworkAdmission::RequireUserApproval
            } else {
                NetworkAdmission::DeferForUnmetered
            };
        }
        if context.metered == Some(true) {
            if class == NetworkWorkClass::Interactive && self.allow_interactive_metered {
                return NetworkAdmission::Allow;
            }
            if class == NetworkWorkClass::Bulk && self.allow_bulk_metered {
                return NetworkAdmission::Allow;
            }
            return if user_initiated {
                NetworkAdmission::RequireUserApproval
            } else {
                NetworkAdmission::DeferForUnmetered
            };
        }
        NetworkAdmission::Allow
    }
}
