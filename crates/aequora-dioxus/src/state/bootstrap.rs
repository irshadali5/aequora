use crate::UiError;

/// Stable bootstrap presentation state. Progress values are advisory; restart recovery is read
/// from the client's durable bootstrap metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BootstrapState {
    Dormant,
    Preparing,
    Downloading { completed: u64, total: Option<u64> },
    Verifying,
    Installing,
    CatchingUp,
    Complete,
    Failed(UiError),
}
