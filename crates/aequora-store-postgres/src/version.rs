//! Explicit compare-and-swap outcome vocabulary.

use aequora_types::EntityVersion;

/// Result of validating an expected aggregate version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompareAndSwap {
    /// The expected version remains current.
    Current,
    /// The authoritative version changed before the transaction acquired its lock.
    Stale(Option<EntityVersion>),
}
