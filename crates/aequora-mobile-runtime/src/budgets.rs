use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// Hard limits for one opportunistic mobile execution window.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MobileSyncBudget {
    pub max_duration: Duration,
    pub max_upload_bytes: u64,
    pub max_download_bytes: u64,
    pub max_operations: usize,
}

impl MobileSyncBudget {
    pub const FOREGROUND: Self = Self {
        max_duration: Duration::from_secs(30),
        max_upload_bytes: 4 * 1_024 * 1_024,
        max_download_bytes: 8 * 1_024 * 1_024,
        max_operations: 500,
    };
    pub const BACKGROUND: Self = Self {
        max_duration: Duration::from_secs(15),
        max_upload_bytes: 1_024 * 1_024,
        max_download_bytes: 1_024 * 1_024,
        max_operations: 100,
    };
    pub const LOW_POWER: Self = Self {
        max_duration: Duration::from_secs(5),
        max_upload_bytes: 256 * 1_024,
        max_download_bytes: 256 * 1_024,
        max_operations: 25,
    };

    /// Rejects a zero or effectively unbounded execution budget.
    ///
    /// # Errors
    ///
    /// Returns [`BudgetError`] when any limit is zero or exceeds the mobile hard ceiling.
    pub fn validate(self) -> Result<(), BudgetError> {
        const MAX_BYTES: u64 = 256 * 1_024 * 1_024;
        if self.max_duration.is_zero()
            || self.max_duration > Duration::from_secs(15 * 60)
            || self.max_upload_bytes == 0
            || self.max_upload_bytes > MAX_BYTES
            || self.max_download_bytes == 0
            || self.max_download_bytes > MAX_BYTES
            || self.max_operations == 0
            || self.max_operations > 100_000
        {
            return Err(BudgetError::InvalidSyncBudget);
        }
        Ok(())
    }

    #[must_use]
    pub fn contains(self, consumption: SyncConsumption) -> bool {
        consumption.elapsed <= self.max_duration
            && consumption.upload_bytes <= self.max_upload_bytes
            && consumption.download_bytes <= self.max_download_bytes
            && consumption.operations <= self.max_operations
    }

    #[must_use]
    pub const fn constrained_by(self, ceiling: Self) -> Self {
        Self {
            max_duration: if self.max_duration.as_nanos() < ceiling.max_duration.as_nanos() {
                self.max_duration
            } else {
                ceiling.max_duration
            },
            max_upload_bytes: if self.max_upload_bytes < ceiling.max_upload_bytes {
                self.max_upload_bytes
            } else {
                ceiling.max_upload_bytes
            },
            max_download_bytes: if self.max_download_bytes < ceiling.max_download_bytes {
                self.max_download_bytes
            } else {
                ceiling.max_download_bytes
            },
            max_operations: if self.max_operations < ceiling.max_operations {
                self.max_operations
            } else {
                ceiling.max_operations
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SyncConsumption {
    pub elapsed: Duration,
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub operations: usize,
}

/// Memory bounds used by streaming sync and coalesced UI delivery.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MobileMemoryBudget {
    pub max_sync_buffer: usize,
    pub max_decode_bytes: usize,
    pub max_pending_view_models: usize,
    pub cpu_workers: u8,
}

impl MobileMemoryBudget {
    pub const STANDARD: Self = Self {
        max_sync_buffer: 4 * 1_024 * 1_024,
        max_decode_bytes: 2 * 1_024 * 1_024,
        max_pending_view_models: 256,
        cpu_workers: 2,
    };

    /// Validates bounded memory and worker counts.
    ///
    /// # Errors
    ///
    /// Returns [`BudgetError`] for zero or excessive limits.
    pub fn validate(self) -> Result<(), BudgetError> {
        if self.max_sync_buffer == 0
            || self.max_sync_buffer > 64 * 1_024 * 1_024
            || self.max_decode_bytes == 0
            || self.max_decode_bytes > self.max_sync_buffer
            || self.max_pending_view_models == 0
            || self.max_pending_view_models > 16_384
            || !(1..=8).contains(&self.cpu_workers)
        {
            return Err(BudgetError::InvalidMemoryBudget);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum BudgetError {
    #[error("invalid mobile sync budget")]
    InvalidSyncBudget,
    #[error("invalid mobile memory budget")]
    InvalidMemoryBudget,
}
