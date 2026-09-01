//! Local coordinator fencing markers.

/// Leader-only metadata transition class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FencedTransition {
    ClaimOutbox,
    Reconcile,
    InstallSnapshot,
    Repair,
    Compact,
}
