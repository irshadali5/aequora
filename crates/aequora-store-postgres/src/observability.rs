//! Payload-free operational counters for the official `PostgreSQL` adapter.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

#[derive(Debug, Default)]
struct MetricsInner {
    pool_wait_nanoseconds: AtomicU64,
    transaction_nanoseconds: AtomicU64,
    cursor_scan_nanoseconds: AtomicU64,
    committed_transactions: AtomicU64,
    journal_appends: AtomicU64,
    duplicate_operations: AtomicU64,
    deadlock_retries: AtomicU64,
    serialization_retries: AtomicU64,
}

/// Cloneable, lock-free metric recorder shared by backend clones.
#[derive(Clone, Debug, Default)]
pub struct PostgresMetrics(Arc<MetricsInner>);

impl PostgresMetrics {
    pub(crate) fn record_pool_wait(&self, duration: Duration) {
        add_duration(&self.0.pool_wait_nanoseconds, duration);
    }

    pub(crate) fn record_transaction(&self, duration: Duration) {
        add_duration(&self.0.transaction_nanoseconds, duration);
    }

    pub(crate) fn record_cursor_scan(&self, duration: Duration) {
        add_duration(&self.0.cursor_scan_nanoseconds, duration);
    }

    pub(crate) fn committed(&self) {
        self.0
            .committed_transactions
            .fetch_add(1, Ordering::Relaxed);
        self.0.journal_appends.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn duplicate(&self) {
        self.0.duplicate_operations.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn retry(&self, reason: aequora_store::StoreErrorReason) {
        match reason {
            aequora_store::StoreErrorReason::Deadlock => {
                self.0.deadlock_retries.fetch_add(1, Ordering::Relaxed);
            }
            aequora_store::StoreErrorReason::SerializationFailure => {
                self.0.serialization_retries.fetch_add(1, Ordering::Relaxed);
            }
            aequora_store::StoreErrorReason::Unspecified
            | aequora_store::StoreErrorReason::LeadershipLost => {}
        }
    }

    /// Returns a point-in-time payload-free metric snapshot.
    #[must_use]
    pub fn snapshot(&self) -> PostgresMetricsSnapshot {
        PostgresMetricsSnapshot {
            pool_wait_nanoseconds: self.0.pool_wait_nanoseconds.load(Ordering::Relaxed),
            transaction_nanoseconds: self.0.transaction_nanoseconds.load(Ordering::Relaxed),
            cursor_scan_nanoseconds: self.0.cursor_scan_nanoseconds.load(Ordering::Relaxed),
            committed_transactions: self.0.committed_transactions.load(Ordering::Relaxed),
            journal_appends: self.0.journal_appends.load(Ordering::Relaxed),
            duplicate_operations: self.0.duplicate_operations.load(Ordering::Relaxed),
            deadlock_retries: self.0.deadlock_retries.load(Ordering::Relaxed),
            serialization_retries: self.0.serialization_retries.load(Ordering::Relaxed),
        }
    }
}

fn add_duration(counter: &AtomicU64, duration: Duration) {
    let nanoseconds = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(nanoseconds))
    });
}

/// Monotonic adapter metric values safe to export through an observability backend.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PostgresMetricsSnapshot {
    /// Aggregate pool acquisition wait in nanoseconds.
    pub pool_wait_nanoseconds: u64,
    /// Aggregate authoritative transaction latency in nanoseconds.
    pub transaction_nanoseconds: u64,
    /// Aggregate cursor scan latency in nanoseconds.
    pub cursor_scan_nanoseconds: u64,
    /// Newly committed authoritative operations.
    pub committed_transactions: u64,
    /// Journal rows appended for primary operations.
    pub journal_appends: u64,
    /// Identical retries resolved from the operation ledger.
    pub duplicate_operations: u64,
    /// Whole-transaction deadlock retries.
    pub deadlock_retries: u64,
    /// Whole-transaction serialization retries.
    pub serialization_retries: u64,
}
