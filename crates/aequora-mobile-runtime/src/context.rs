use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum AppLifecycle {
    #[default]
    Foreground,
    Background,
    Suspended,
    Terminating,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum BatteryClass {
    Critical,
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ThermalState {
    #[default]
    Nominal,
    Fair,
    Serious,
    Critical,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum StoragePressure {
    #[default]
    Healthy,
    Low,
    Critical,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum NetworkAvailability {
    Unavailable,
    #[default]
    Available,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum NetworkCost {
    #[default]
    Unmetered,
    Metered,
    Expensive,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum NetworkConstraint {
    #[default]
    Unconstrained,
    Constrained,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum RoamingState {
    #[default]
    Home,
    Roaming,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct NetworkContext {
    pub availability: NetworkAvailability,
    pub cost: NetworkCost,
    pub constraint: NetworkConstraint,
    pub roaming: RoamingState,
}

impl NetworkContext {
    #[must_use]
    pub const fn available(self) -> bool {
        matches!(self.availability, NetworkAvailability::Available)
    }

    #[must_use]
    pub const fn metered(self) -> bool {
        matches!(self.cost, NetworkCost::Metered | NetworkCost::Expensive)
    }

    #[must_use]
    pub const fn expensive(self) -> bool {
        matches!(self.cost, NetworkCost::Expensive)
    }

    #[must_use]
    pub const fn constrained(self) -> bool {
        matches!(self.constraint, NetworkConstraint::Constrained)
    }

    #[must_use]
    pub const fn roaming(self) -> bool {
        matches!(self.roaming, RoamingState::Roaming)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PowerContext {
    pub charging: bool,
    pub low_power_mode: bool,
    pub battery: BatteryClass,
    pub thermal: ThermalState,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MobileResourceContext {
    pub lifecycle: AppLifecycle,
    pub network: NetworkContext,
    pub power: PowerContext,
    pub storage: StoragePressure,
    pub secure_keys_available: bool,
}
