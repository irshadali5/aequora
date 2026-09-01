use crate::UiError;
use std::sync::Arc;

/// Bounded, redacted diagnostics presentation state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticsState {
    Loading,
    Ready(Arc<str>),
    Error(UiError),
}
