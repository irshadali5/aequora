use serde::{Deserialize, Serialize};

use super::context::PowerContext;

/// Policy for optional battery-expensive client work.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientPowerPolicy {
    pub maintenance_requires_charging: bool,
    pub defer_bulk_when_constrained: bool,
}

impl ClientPowerPolicy {
    /// Whether maintenance is currently eligible from a power perspective.
    #[must_use]
    pub fn maintenance_allowed(self, context: PowerContext) -> bool {
        !context.constrained()
            && (!self.maintenance_requires_charging || context.charging == Some(true))
    }

    /// Whether bulk work should yield until power improves.
    #[must_use]
    pub fn defer_bulk(self, context: PowerContext) -> bool {
        self.defer_bulk_when_constrained && context.constrained()
    }
}
