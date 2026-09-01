//! Durable conflict lifecycle.

/// Persisted local conflict status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictStatus {
    Unresolved,
    AcceptedServer,
    Superseded,
}
