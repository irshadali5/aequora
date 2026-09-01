mod bootstrap;
mod conflicts;
mod diagnostics;
mod events;
mod lifecycle;
mod mutation;
mod operation;
mod query;
mod scope;
mod sync_status;

pub use bootstrap::use_bootstrap_progress;
pub use conflicts::use_conflicts;
pub use diagnostics::use_diagnostics;
pub use events::use_aequora_events;
pub use lifecycle::use_connectivity;
pub use mutation::{MutationHandle, use_aequora_mutation};
pub use operation::use_operation;
pub use query::{QueryHandle, use_aequora_query};
pub use scope::{ScopeMutationHandle, use_aequora_scope};
pub use sync_status::use_sync_status;

use crate::AequoraHandle;
use dioxus::prelude::use_context;

/// Returns the provider's cheap cloneable client handle.
#[must_use]
pub fn use_aequora() -> AequoraHandle {
    use_context::<AequoraHandle>()
}
