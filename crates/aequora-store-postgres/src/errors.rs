//! PostgreSQL-private SQLSTATE classification.

/// Canonical action selected from a `PostgreSQL` SQLSTATE without leaking driver types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostgresErrorClass {
    /// Replay a deterministic whole transaction within its bound.
    RetryTransaction,
    /// Resolve an operation retry through its idempotency ledger.
    ResolveAmbiguousCommit,
    /// Reject the logical input or violated invariant.
    Permanent,
    /// Retry later without assuming the transaction is replay-safe.
    Transient,
}

/// Classifies SQLSTATE values used by the adapter.
#[must_use]
pub fn classify_sqlstate(code: &str) -> PostgresErrorClass {
    match code {
        "40001" | "40P01" => PostgresErrorClass::RetryTransaction,
        "23505" | "23514" | "23503" => PostgresErrorClass::Permanent,
        "08003" | "08006" | "57P01" => PostgresErrorClass::ResolveAmbiguousCommit,
        _ => PostgresErrorClass::Transient,
    }
}
