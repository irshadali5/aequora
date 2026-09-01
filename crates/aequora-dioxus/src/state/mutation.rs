use crate::UiError;
use aequora_operation::MutationReceipt;

/// Ephemeral state of the latest component submission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MutationState {
    Idle,
    SubmittingLocal,
    /// Local domain state and outbox intent committed; authority has not necessarily accepted it.
    SavedLocally(MutationReceipt),
    Error(UiError),
}
