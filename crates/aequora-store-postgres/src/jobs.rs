//! Durable side-effect intent records committed inside the authority transaction.

use aequora_types::JobId;

/// One external side effect to execute only after the authoritative commit succeeds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PostgresSideEffectIntent {
    /// Stable job identity selected before transaction retry.
    pub job_id: JobId,
    /// Registered side-effect kind.
    pub kind: u16,
    /// Opaque bounded worker payload.
    pub payload: Vec<u8>,
}

/// Durable worker lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostgresSideEffectStatus {
    /// Committed and eligible for claim.
    Pending,
    /// Claimed by a worker lease.
    InFlight,
    /// Provider effect completed.
    Completed,
    /// Retry policy was exhausted or permanently rejected.
    Failed,
}
