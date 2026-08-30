use std::collections::{BTreeSet, VecDeque};

use aequora_types::Cursor;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{ClientResourceProfile, MemoryClass, StorageState};

/// Lifecycle signals forwarded by Android, iOS, desktop, or another platform shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppLifecycleEvent {
    Foreground,
    Background,
    Suspending,
    Resumed,
}

/// Actions requested from existing durable scheduler/live components.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct LifecycleActions {
    pub flush_scheduler_metadata: bool,
    pub commit_checkpoints: bool,
    pub close_transient_connections: bool,
    pub immediate_catch_up: bool,
}

impl AppLifecycleEvent {
    /// Maps lifecycle changes to short, non-blocking integration actions.
    #[must_use]
    pub const fn actions(self) -> LifecycleActions {
        match self {
            Self::Foreground | Self::Resumed => LifecycleActions {
                immediate_catch_up: true,
                ..LifecycleActions {
                    flush_scheduler_metadata: false,
                    commit_checkpoints: false,
                    close_transient_connections: false,
                    immediate_catch_up: false,
                }
            },
            Self::Background | Self::Suspending => LifecycleActions {
                flush_scheduler_metadata: true,
                commit_checkpoints: true,
                close_transient_connections: true,
                immediate_catch_up: false,
            },
        }
    }
}

/// Platform events are coalesced before one scheduler reevaluation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ResourceEvent {
    NetworkChanged,
    PowerChanged,
    StoragePressure,
    MemoryPressure,
    ThermalChanged,
    BackgroundBudgetChanged,
}

/// Bounded duplicate-eliminating resource event collector.
#[derive(Clone, Debug, Default)]
pub struct ResourceEventCoalescer {
    pending: BTreeSet<ResourceEvent>,
}

impl ResourceEventCoalescer {
    /// Records a coarse event; duplicates do not grow memory.
    pub fn push(&mut self, event: ResourceEvent) {
        self.pending.insert(event);
    }

    /// Drains one reevaluation batch in stable order.
    pub fn take(&mut self) -> Vec<ResourceEvent> {
        std::mem::take(&mut self.pending).into_iter().collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// Persistable bounded-unit checkpoint. Payloads remain in their durable work source.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DurableWorkCheckpoint {
    pub format_version: u32,
    pub completed_units: u64,
    pub committed_cursor: Option<Cursor>,
}

impl DurableWorkCheckpoint {
    /// Starts a non-zero format checkpoint.
    #[must_use]
    pub const fn new(format_version: u32) -> Option<Self> {
        if format_version == 0 {
            None
        } else {
            Some(Self {
                format_version,
                completed_units: 0,
                committed_cursor: None,
            })
        }
    }

    /// Advances only after the bounded unit and checkpoint transaction are both durable.
    ///
    /// # Errors
    ///
    /// Rejects uncommitted work, non-contiguous units, and cursor regression.
    pub fn commit_unit(
        &mut self,
        unit: u64,
        cursor: Option<Cursor>,
        local_apply_durable: bool,
        checkpoint_durable: bool,
    ) -> Result<(), CheckpointError> {
        if !local_apply_durable || !checkpoint_durable {
            return Err(CheckpointError::DurabilityNotProven);
        }
        if unit != self.completed_units.saturating_add(1) {
            return Err(CheckpointError::NonContiguousUnit);
        }
        if let (Some(previous), Some(next)) = (self.committed_cursor, cursor) {
            if previous.authority_id != next.authority_id
                || previous.authority_epoch != next.authority_epoch
                || next.sequence < previous.sequence
            {
                return Err(CheckpointError::CursorRegression);
            }
        }
        self.completed_units = unit;
        if cursor.is_some() {
            self.committed_cursor = cursor;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CheckpointError {
    #[error("bounded work and checkpoint durability must both be proven")]
    DurabilityNotProven,
    #[error("checkpoint units must advance exactly one")]
    NonContiguousUnit,
    #[error("checkpoint cursor cannot change authority or regress")]
    CursorRegression,
}

/// Restart-stable retry state so process death cannot reset a request storm.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RetryCheckpoint {
    pub attempts: u32,
    pub not_before_unix_ms: u64,
}

impl RetryCheckpoint {
    /// Records capped exponential delay without relying on a process lifetime.
    #[must_use]
    pub fn after_failure(
        self,
        now_unix_ms: u64,
        initial_delay_ms: u64,
        maximum_delay_ms: u64,
    ) -> Self {
        let exponent = self.attempts.min(31);
        let delay = initial_delay_ms
            .saturating_mul(1_u64 << exponent)
            .min(maximum_delay_ms.max(1));
        Self {
            attempts: self.attempts.saturating_add(1),
            not_before_unix_ms: now_unix_ms.saturating_add(delay),
        }
    }

    /// A long offline interval may rescale attempts but never makes the retry due before `now`.
    #[must_use]
    pub fn rescale_after_offline(self, now_unix_ms: u64) -> Self {
        Self {
            attempts: self.attempts.min(8),
            not_before_unix_ms: self.not_before_unix_ms.max(now_unix_ms),
        }
    }
}

/// Accurate application-facing state; resource deferral is not mislabeled as offline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientStatus {
    UpToDate,
    Syncing,
    WaitingForNetwork,
    WaitingForUnmetered,
    StorageLow,
    SyncDelayedForPower,
    NeedsRebootstrap,
}

/// Privacy-local resource metrics. Upload requires an application telemetry decision.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClientResourceMetrics {
    pub profile: ClientResourceProfile,
    pub memory_class: MemoryClass,
    pub storage_state: StorageState,
    pub sync_deferred_resource_total: u64,
    pub bootstrap_paused_resource_total: u64,
    pub outbox_bytes: u64,
    pub local_db_bytes: u64,
    pub cache_evicted_bytes: u64,
    pub sync_bytes: u64,
    pub snapshot_bytes: u64,
    pub blob_bytes: u64,
}

/// Bounded, payload-free structured client resource event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientResourceLogEvent {
    ProfileChanged { profile: ClientResourceProfile },
    StorageLow,
    SyncDeferredMetered,
    BackgroundBudgetExpired,
    SnapshotPaused,
    CacheEvicted { bytes: u64 },
}

/// Sanitized user-facing diagnostics; no domain payload or exact hardware telemetry appears.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClientUserDiagnostics {
    pub local_data_bytes: u64,
    pub pending_changes: u64,
    pub last_successful_sync_unix_ms: Option<u64>,
    pub network_restricted: bool,
    pub storage_state: StorageState,
}

/// Small in-memory diagnostic ring. Applications decide whether sanitized upload is permitted.
#[derive(Clone, Debug)]
pub struct DiagnosticRing<T> {
    capacity: usize,
    entries: VecDeque<T>,
}

impl<T> DiagnosticRing<T> {
    /// Creates a strictly bounded ring.
    #[must_use]
    pub fn new(capacity: usize) -> Option<Self> {
        (capacity > 0).then(|| Self {
            capacity,
            entries: VecDeque::with_capacity(capacity),
        })
    }

    pub fn push(&mut self, entry: T) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.entries.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
