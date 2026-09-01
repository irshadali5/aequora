//! Named local ACID boundaries.

/// Correctness-critical transaction boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalTransactionKind {
    /// Provisional domain mutation plus corresponding durable outbox intent.
    MutationAndOutbox,
    /// Authoritative apply, outcomes/conflicts, overlay reconciliation, and cursor advancement.
    ReconciliationAndCursor,
}
