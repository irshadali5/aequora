use crate::UiError;

/// Ephemeral presentation of a durable scope transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScopeState {
    Idle,
    Expanding,
    Active,
    Contracting,
    NeedsBootstrap,
    Error(UiError),
}
