//! Durable scheduler state; runtime task handles remain in memory.

/// Restart-safe scheduler checkpoint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SchedulerCheckpoint {
    pub next_attempt_unix_ms: u64,
    pub retry_after_unix_ms: u64,
    pub circuit_open_until_unix_ms: u64,
    pub data_budget_used_bytes: u64,
}
