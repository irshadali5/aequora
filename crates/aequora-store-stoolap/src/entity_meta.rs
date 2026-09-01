//! Local entity sidecar metadata contracts.

/// Durable deletion semantics for one local projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TombstoneState {
    Live,
    Tombstoned,
    PendingGarbageCollection,
}
