//! Dioxus hooks over Aequora's durable, local-first client SDK.
//!
//! Reactive values in this crate are views and bounded wake-up hints. Durable application state,
//! outbox intent, operation status, conflicts, scopes, and bootstrap truth remain owned by the
//! configured [`aequora_client::AequoraClient`] and its local store.

mod client_handle;
mod errors;
pub mod hooks;
mod platform;
mod provider;
pub mod state;

pub use client_handle::{AequoraHandle, Invalidation, InvalidationReceiver, QueryNamespace};
pub use errors::{QueryError, UiError, UiErrorKind};
pub use hooks::{
    MutationHandle, QueryHandle, ScopeMutationHandle, use_aequora, use_aequora_events,
    use_aequora_mutation, use_aequora_query, use_aequora_scope, use_bootstrap_progress,
    use_conflicts, use_connectivity, use_diagnostics, use_operation, use_sync_status,
};
pub use platform::{Connectivity, LifecycleState, PlatformEvent};
pub use provider::{provide_aequora, provide_aequora_with_namespace};
pub use state::{
    BootstrapState, DiagnosticsState, MutationState, QueryKey, QueryState, ScopeState, SyncStatus,
};

/// Focused imports for Dioxus application integration.
pub mod prelude {
    pub use crate::{
        AequoraHandle, BootstrapState, Connectivity, DiagnosticsState, Invalidation,
        LifecycleState, MutationHandle, MutationState, PlatformEvent, QueryError, QueryHandle,
        QueryKey, QueryNamespace, QueryState, ScopeMutationHandle, ScopeState, SyncStatus, UiError,
        UiErrorKind, provide_aequora, provide_aequora_with_namespace, use_aequora,
        use_aequora_events, use_aequora_mutation, use_aequora_query, use_aequora_scope,
        use_bootstrap_progress, use_conflicts, use_connectivity, use_diagnostics, use_operation,
        use_sync_status,
    };
}
