//! Durable repair-plan lifecycle.

/// Persisted repair execution phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepairPhase {
    Planned,
    Applying,
    Verifying,
    Complete,
    Failed,
}
