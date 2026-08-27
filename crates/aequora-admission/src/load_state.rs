use aequora_scheduler::WorkClass;
use serde::{Deserialize, Serialize};

use crate::PolicyError;

/// Coarse server saturation state used for deterministic shedding policy.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u8)]
pub enum LoadState {
    #[default]
    Healthy = 0,
    Elevated = 1,
    Overloaded = 2,
    Critical = 3,
}

/// Normalized resource pressure, where every value is in `0..=1000` permille.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoadSignals {
    pub database: u16,
    pub cpu: u16,
    pub memory: u16,
    pub disk: u16,
    pub queue: u16,
    pub replica_lag: u16,
    pub network: u16,
}

impl LoadSignals {
    #[must_use]
    pub fn maximum(self) -> u16 {
        [
            self.database,
            self.cpu,
            self.memory,
            self.disk,
            self.queue,
            self.replica_lag,
            self.network,
        ]
        .into_iter()
        .max()
        .unwrap_or(0)
        .min(1_000)
    }
}

/// Enter/exit thresholds and dwell time that prevent load-state flapping.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoadThresholds {
    pub elevated_enter: u16,
    pub elevated_exit: u16,
    pub overloaded_enter: u16,
    pub overloaded_exit: u16,
    pub critical_enter: u16,
    pub critical_exit: u16,
    pub minimum_dwell_ms: u64,
}

impl Default for LoadThresholds {
    fn default() -> Self {
        Self {
            elevated_enter: 700,
            elevated_exit: 600,
            overloaded_enter: 850,
            overloaded_exit: 750,
            critical_enter: 950,
            critical_exit: 850,
            minimum_dwell_ms: 5_000,
        }
    }
}

impl LoadThresholds {
    /// Validates ordered enter/exit thresholds and a non-zero dwell time.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidLoadPolicy`] for inconsistent hysteresis.
    pub const fn validate(self) -> Result<(), PolicyError> {
        if self.minimum_dwell_ms == 0
            || self.elevated_exit >= self.elevated_enter
            || self.overloaded_exit >= self.overloaded_enter
            || self.critical_exit >= self.critical_enter
            || self.elevated_enter >= self.overloaded_enter
            || self.overloaded_enter >= self.critical_enter
            || self.critical_enter > 1_000
        {
            return Err(PolicyError::InvalidLoadPolicy);
        }
        Ok(())
    }
}

/// Stateful hysteresis tracker. Time is supplied by the host to keep the core runtime-neutral.
#[derive(Clone, Copy, Debug)]
pub struct LoadStateTracker {
    thresholds: LoadThresholds,
    state: LoadState,
    last_transition_ms: Option<u64>,
}

impl LoadStateTracker {
    /// Creates a healthy tracker using validated hysteresis thresholds.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidLoadPolicy`] for inconsistent thresholds.
    pub fn new(thresholds: LoadThresholds) -> Result<Self, PolicyError> {
        thresholds.validate()?;
        Ok(Self {
            thresholds,
            state: LoadState::Healthy,
            last_transition_ms: None,
        })
    }

    #[must_use]
    pub const fn state(self) -> LoadState {
        self.state
    }

    /// Observes pressure and returns the hysteretic state after at most one transition.
    pub fn observe(&mut self, now_ms: u64, signals: LoadSignals) -> LoadState {
        let pressure = signals.maximum();
        let dwell_elapsed = self
            .last_transition_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= self.thresholds.minimum_dwell_ms);
        if !dwell_elapsed {
            return self.state;
        }
        let next = match self.state {
            LoadState::Healthy if pressure >= self.thresholds.elevated_enter => LoadState::Elevated,
            LoadState::Elevated if pressure >= self.thresholds.overloaded_enter => {
                LoadState::Overloaded
            }
            LoadState::Elevated if pressure <= self.thresholds.elevated_exit => LoadState::Healthy,
            LoadState::Overloaded if pressure >= self.thresholds.critical_enter => {
                LoadState::Critical
            }
            LoadState::Overloaded if pressure <= self.thresholds.overloaded_exit => {
                LoadState::Elevated
            }
            LoadState::Critical if pressure <= self.thresholds.critical_exit => {
                LoadState::Overloaded
            }
            current => current,
        };
        if next != self.state {
            self.state = next;
            self.last_transition_ms = Some(now_ms);
        }
        self.state
    }
}

/// Capacity, shedding, and client-guidance behavior for each load state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoadPolicy {
    pub thresholds: LoadThresholds,
    pub healthy_capacity_percent: u8,
    pub elevated_capacity_percent: u8,
    pub overloaded_capacity_percent: u8,
    pub critical_capacity_percent: u8,
    pub preferred_max_batch_ops_overloaded: usize,
    pub preferred_max_batch_bytes_overloaded: usize,
}

impl Default for LoadPolicy {
    fn default() -> Self {
        Self {
            thresholds: LoadThresholds::default(),
            healthy_capacity_percent: 100,
            elevated_capacity_percent: 85,
            overloaded_capacity_percent: 60,
            critical_capacity_percent: 25,
            preferred_max_batch_ops_overloaded: 128,
            preferred_max_batch_bytes_overloaded: 512 * 1_024,
        }
    }
}

impl LoadPolicy {
    /// Validates monotonic capacity reduction and non-zero batch guidance.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidLoadPolicy`] for unsafe settings.
    pub const fn validate(self) -> Result<(), PolicyError> {
        if self.thresholds.validate().is_err()
            || self.healthy_capacity_percent == 0
            || self.healthy_capacity_percent > 100
            || self.elevated_capacity_percent == 0
            || self.elevated_capacity_percent > self.healthy_capacity_percent
            || self.overloaded_capacity_percent == 0
            || self.overloaded_capacity_percent > self.elevated_capacity_percent
            || self.critical_capacity_percent == 0
            || self.critical_capacity_percent > self.overloaded_capacity_percent
            || self.preferred_max_batch_ops_overloaded == 0
            || self.preferred_max_batch_bytes_overloaded == 0
        {
            return Err(PolicyError::InvalidLoadPolicy);
        }
        Ok(())
    }

    #[must_use]
    pub const fn capacity_percent(self, state: LoadState) -> u8 {
        match state {
            LoadState::Healthy => self.healthy_capacity_percent,
            LoadState::Elevated => self.elevated_capacity_percent,
            LoadState::Overloaded => self.overloaded_capacity_percent,
            LoadState::Critical => self.critical_capacity_percent,
        }
    }

    /// Low-value work is shed first; security/critical work is retained longest.
    #[must_use]
    pub const fn allows(self, state: LoadState, class: WorkClass) -> bool {
        match state {
            LoadState::Healthy => true,
            LoadState::Elevated => !matches!(class, WorkClass::Maintenance),
            LoadState::Overloaded => !matches!(
                class,
                WorkClass::Bulk | WorkClass::Background | WorkClass::Maintenance
            ),
            LoadState::Critical => {
                matches!(class, WorkClass::Critical | WorkClass::Interactive)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hysteresis_requires_dwell_and_distinct_exit_threshold() {
        let mut tracker = LoadStateTracker::new(LoadThresholds::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let high = LoadSignals {
            database: 900,
            ..LoadSignals::default()
        };
        assert_eq!(tracker.observe(0, high), LoadState::Elevated);
        assert_eq!(tracker.observe(1, high), LoadState::Elevated);
        assert_eq!(tracker.observe(5_000, high), LoadState::Overloaded);
        let middle = LoadSignals {
            database: 800,
            ..LoadSignals::default()
        };
        assert_eq!(tracker.observe(10_000, middle), LoadState::Overloaded);
        let low = LoadSignals {
            database: 700,
            ..LoadSignals::default()
        };
        assert_eq!(tracker.observe(15_000, low), LoadState::Elevated);
    }
}
