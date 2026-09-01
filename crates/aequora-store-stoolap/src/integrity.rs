//! Local integrity checkpoint state.

/// Whether an integrity checkpoint may be trusted for repair decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegrityCheckpointState {
    Pending,
    Verified,
    Diverged,
}
