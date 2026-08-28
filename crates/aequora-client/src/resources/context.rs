use serde::{Deserialize, Serialize};

/// Conservative platform-derived memory classification.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum MemoryClass {
    VeryLow,
    Low,
    #[default]
    Normal,
    High,
}

/// Risk to durable local writes and bounded staging.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum StorageState {
    #[default]
    Healthy,
    Low,
    Critical,
    ReadOnlyRisk,
}

/// Execution time currently granted by the application platform.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum BackgroundBudget {
    #[default]
    Foreground,
    Short,
    Limited,
    Suspended,
}

/// Coarse thermal signal. Unknown platforms should report [`ThermalState::Normal`].
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ThermalState {
    #[default]
    Normal,
    Warm,
    Hot,
    Critical,
}

/// Network facts used locally for resource admission.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NetworkContext {
    pub online: bool,
    pub metered: Option<bool>,
    pub roaming: Option<bool>,
    pub high_loss: bool,
    pub estimated_rtt_ms: Option<u64>,
}

impl Default for NetworkContext {
    fn default() -> Self {
        Self {
            online: true,
            metered: None,
            roaming: None,
            high_loss: false,
            estimated_rtt_ms: None,
        }
    }
}

/// Power facts used only to schedule optional work.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PowerContext {
    pub charging: Option<bool>,
    pub battery_percent: Option<u8>,
    pub low_power_mode: Option<bool>,
}

impl PowerContext {
    /// Clamps untrusted platform percentages to their valid coarse range.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.battery_percent = self.battery_percent.map(|value| value.min(100));
        self
    }

    /// Whether the device explicitly reports a constrained power state.
    #[must_use]
    pub fn constrained(self) -> bool {
        self.low_power_mode == Some(true) || self.battery_percent.is_some_and(|value| value <= 15)
    }
}

/// Complete local resource snapshot supplied through a platform adapter.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientResourceContext {
    pub memory: MemoryClass,
    pub storage: StorageState,
    pub network: NetworkContext,
    pub power: PowerContext,
    pub background: BackgroundBudget,
    pub thermal: ThermalState,
}

impl ClientResourceContext {
    /// Normalizes bounded platform values without inventing missing signals.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.power = self.power.normalized();
        self
    }
}

/// Platform abstraction for Android, iOS, desktop, or future browser resource signals.
pub trait PlatformResourceMonitor: Send + Sync {
    fn current(&self) -> ClientResourceContext;
}
