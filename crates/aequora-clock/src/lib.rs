//! Hybrid logical clocks for causal metadata.

use aequora_types::{HybridTimestamp, NodeId};
use std::{
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

/// Source of hybrid timestamps.
pub trait Clock: Send + Sync {
    /// Emits a timestamp greater than any previously emitted local timestamp.
    fn now(&self) -> HybridTimestamp;
    /// Observes a remote timestamp and emits a causally later local timestamp.
    fn observe(&self, remote: HybridTimestamp) -> HybridTimestamp;
}

/// Thread-safe HLC backed by the system wall clock.
pub struct SystemClock {
    node: NodeId,
    state: Mutex<(i64, u32)>,
}

impl SystemClock {
    /// Creates a clock for a stable node identity.
    #[must_use]
    pub fn new(node: NodeId) -> Self {
        Self {
            node,
            state: Mutex::new((0, 0)),
        }
    }

    fn state(&self) -> MutexGuard<'_, (i64, u32)> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wall_ms() -> i64 {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis());
        i64::try_from(millis).unwrap_or(i64::MAX)
    }
}

impl Clock for SystemClock {
    fn now(&self) -> HybridTimestamp {
        let wall = Self::wall_ms();
        let mut state = self.state();
        if wall > state.0 {
            *state = (wall, 0);
        } else {
            state.1 = state.1.saturating_add(1);
        }
        HybridTimestamp {
            physical_ms: state.0,
            logical: state.1,
            node: self.node,
        }
    }

    fn observe(&self, remote: HybridTimestamp) -> HybridTimestamp {
        let wall = Self::wall_ms();
        let mut state = self.state();
        let physical = wall.max(state.0).max(remote.physical_ms);
        let logical = if physical == state.0 && physical == remote.physical_ms {
            state.1.max(remote.logical).saturating_add(1)
        } else if physical == state.0 {
            state.1.saturating_add(1)
        } else if physical == remote.physical_ms {
            remote.logical.saturating_add(1)
        } else {
            0
        };
        *state = (physical, logical);
        HybridTimestamp {
            physical_ms: physical,
            logical,
            node: self.node,
        }
    }
}

/// Deterministic clock for simulations and tests.
pub struct TestClock {
    node: NodeId,
    state: Mutex<(i64, u32)>,
}

impl TestClock {
    /// Creates a clock at a fixed wall-clock value.
    #[must_use]
    pub fn new(node: NodeId, physical_ms: i64) -> Self {
        Self {
            node,
            state: Mutex::new((physical_ms, 0)),
        }
    }

    /// Advances deterministic physical time without sleeping.
    pub fn advance_ms(&self, amount: i64) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.0 = state.0.saturating_add(amount.max(0));
        state.1 = 0;
    }
}

impl Clock for TestClock {
    fn now(&self) -> HybridTimestamp {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.1 = state.1.saturating_add(1);
        HybridTimestamp {
            physical_ms: state.0,
            logical: state.1,
            node: self.node,
        }
    }

    fn observe(&self, remote: HybridTimestamp) -> HybridTimestamp {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if remote.physical_ms > state.0 {
            *state = (remote.physical_ms, remote.logical.saturating_add(1));
        } else {
            state.1 = state.1.max(remote.logical).saturating_add(1);
        }
        HybridTimestamp {
            physical_ms: state.0,
            logical: state.1,
            node: self.node,
        }
    }
}
