//! Stable public facade for the Aequora Rust SDK.
//!
//! Most applications should depend on this crate plus explicitly selected adapter crates. The
//! facade exposes semantic client/server concepts, not journal rows, cursors, database drivers,
//! HTTP frameworks, scheduler queues, or runtime topology.

pub use aequora_client::{
    AequoraClient, AequoraClientBuilder, AequoraError, AequoraErrorCode, BlobHandle,
    BootstrapProgress, ClientSdkConfig, ConflictHandle, DataChange, DiagnosticsHandle, EventStream,
    OperationHandle, OperationRef, RetryClass, ScopeHandle, ScopeSubscriptionState, SyncEvent,
    SyncNextAction, SyncReason, SyncResult, SyncStatus,
};
pub use aequora_operation::{
    EntityRef, LocalCommitStatus, MutationReceipt, Operation, OperationEncodingError,
    OperationKind, OperationSchemaVersion, OperationState, OperationUpdate,
};
pub use aequora_registry_generated as registry;
pub use aequora_server::{
    AequoraServer, AequoraServerBuilder, DomainRegistrationError, DomainRegistry,
    DomainRegistryBuilder, ServerOperationOutcome, StableServerError,
};
pub use aequora_types::{DeviceId, EntityId, OperationId, SyncScopeId, TenantId};

/// Public name used by application-facing SDK documentation.
pub type ScopeId = SyncScopeId;

/// Intentionally small set of imports for ordinary application use.
pub mod prelude {
    pub use crate::{
        AequoraClient, AequoraClientBuilder, AequoraError, AequoraErrorCode, AequoraServer,
        AequoraServerBuilder, DomainRegistry, EntityId, LocalCommitStatus, MutationReceipt,
        Operation, OperationId, OperationKind, OperationSchemaVersion, OperationState, RetryClass,
        ScopeId, SyncEvent, SyncNextAction, SyncReason, SyncResult, SyncStatus,
    };
    pub use aequora_adapter_sdk::{
        Authenticator, AuthorityService, ClientIdentity, ClientStore, ConflictId,
        ConflictResolution, ConflictSummary, Credential, CredentialProvider, DomainContext,
        DomainHandler, DomainId, DomainOutcome, SyncTransport,
    };
}

/// Unstable APIs may be added here without the facade's stable semver guarantee.
pub mod experimental {}

#[cfg(feature = "macros")]
pub use aequora_executor as executor;
#[cfg(feature = "macros")]
pub use aequora_macros::{AequoraAggregate, AequoraOperation};
#[cfg(feature = "macros")]
pub use aequora_profile as profile;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_client_handle_is_send_and_sync() {
        assert_send_sync::<AequoraClient>();
    }
}
