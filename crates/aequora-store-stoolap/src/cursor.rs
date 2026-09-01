//! Cursor persistence markers.

/// Cursor writes are legal only as part of the reconciliation transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconciliationCursorWrite;
