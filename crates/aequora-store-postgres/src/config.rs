//! Typed `PostgreSQL` adapter configuration. Secrets deliberately stay outside this model.

use std::time::Duration;

/// Bounded transaction behavior applied to every runtime connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresTransactionConfig {
    /// Maximum duration of one SQL statement.
    pub statement_timeout: Duration,
    /// Maximum time spent waiting for a database lock.
    pub lock_timeout: Duration,
    /// Maximum time a transaction may remain idle.
    pub idle_in_transaction_timeout: Duration,
    /// Maximum deterministic whole-transaction attempts, including the first attempt.
    pub retry_limit: u32,
}

impl PostgresTransactionConfig {
    /// Validates correctness-critical timeout and retry bounds.
    ///
    /// # Errors
    ///
    /// Returns a static explanation when any timeout or retry bound is zero.
    pub const fn validate(self) -> Result<(), &'static str> {
        if self.statement_timeout.is_zero() {
            return Err("statement timeout must be non-zero");
        }
        if self.lock_timeout.is_zero() {
            return Err("lock timeout must be non-zero");
        }
        if self.idle_in_transaction_timeout.is_zero() {
            return Err("idle-in-transaction timeout must be non-zero");
        }
        if self.retry_limit == 0 {
            return Err("transaction retry limit must be non-zero");
        }
        Ok(())
    }
}

impl Default for PostgresTransactionConfig {
    fn default() -> Self {
        Self {
            statement_timeout: Duration::from_secs(5),
            lock_timeout: Duration::from_secs(1),
            idle_in_transaction_timeout: Duration::from_secs(10),
            retry_limit: 3,
        }
    }
}

/// Bounded journal reads exposed by the adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresJournalConfig {
    /// Hard event count limit for one pull.
    pub pull_batch_limit: usize,
    /// Hard aggregate payload limit for one pull.
    pub pull_payload_bytes: usize,
}

impl Default for PostgresJournalConfig {
    fn default() -> Self {
        Self {
            pull_batch_limit: 500,
            pull_payload_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Complete non-secret adapter configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PostgresAdapterConfig {
    /// Pool lifecycle and admission bounds.
    pub pool: crate::PostgresPoolConfig,
    /// Transaction timeout and retry policy.
    pub transaction: PostgresTransactionConfig,
    /// Journal scan bounds.
    pub journal: PostgresJournalConfig,
}

impl PostgresAdapterConfig {
    /// Validates all correctness-critical configuration before connecting.
    ///
    /// # Errors
    ///
    /// Returns a static explanation for an unsafe or nonsensical bound.
    pub const fn validate(self) -> Result<(), &'static str> {
        if self.pool.max_connections == 0 {
            return Err("maximum connections must be non-zero");
        }
        if self.pool.min_connections > self.pool.max_connections {
            return Err("minimum connections exceed maximum connections");
        }
        if self.pool.acquire_timeout.is_zero() {
            return Err("pool acquire timeout must be non-zero");
        }
        if self.journal.pull_batch_limit == 0 || self.journal.pull_payload_bytes == 0 {
            return Err("journal pull bounds must be non-zero");
        }
        self.transaction.validate()
    }
}
