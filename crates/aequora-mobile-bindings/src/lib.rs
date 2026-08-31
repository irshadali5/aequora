//! Stable high-level binding surface for generated Kotlin and Swift wrappers.
//!
//! Native generators may map [`MobileCommand`] to Kotlin suspending functions/Flows or Swift
//! async functions/AsyncStreams. Raw journal events, cursor mutation, and operation-ledger access
//! are intentionally absent. A concrete C/JNI generator lives in the application release pipeline.

use aequora_mobile_runtime::{AEQUORA_MOBILE_ABI_VERSION, MobileEvent, MobileStatus};
use async_trait::async_trait;
use futures_util::FutureExt as _;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, panic::AssertUnwindSafe};
use thiserror::Error;

pub const MAX_BINDING_TEXT_BYTES: usize = 4_096;
pub const MAX_PENDING_EVENTS: usize = 256;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MobileBuildIdentity {
    pub abi_version: u32,
    pub core_version: String,
    pub build_id: String,
    pub registry_generation: u64,
    pub protocol_versions: Vec<u16>,
}

impl MobileBuildIdentity {
    /// Validates diagnostic identity exposed across the ABI.
    ///
    /// # Errors
    ///
    /// Rejects mismatched ABI, empty identity, excessive text, or invalid protocols.
    pub fn validate(&self) -> Result<(), BindingError> {
        let valid = |value: &str| !value.is_empty() && value.len() <= MAX_BINDING_TEXT_BYTES;
        if self.abi_version != AEQUORA_MOBILE_ABI_VERSION
            || !valid(&self.core_version)
            || !valid(&self.build_id)
            || self.registry_generation == 0
            || self.protocol_versions.is_empty()
            || self.protocol_versions.len() > 64
            || self.protocol_versions.contains(&0)
        {
            return Err(BindingError::new(BindingErrorCode::IncompatibleAbi));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MobileCommand {
    Open,
    Mutate {
        operation: Vec<u8>,
    },
    SyncNow,
    SyncOnce,
    QueryStatus,
    ResolveConflict {
        conflict_id: String,
        resolution: Vec<u8>,
    },
    ExportSanitizedDiagnostics,
}

impl MobileCommand {
    /// Bounds all host-controlled binding values before they reach core APIs.
    ///
    /// # Errors
    ///
    /// Rejects empty or oversized payloads and identifiers.
    pub fn validate(&self) -> Result<(), BindingError> {
        const MAX_OPERATION_BYTES: usize = 8 * 1_024 * 1_024;
        match self {
            Self::Mutate { operation }
                if operation.is_empty() || operation.len() > MAX_OPERATION_BYTES =>
            {
                Err(BindingError::new(BindingErrorCode::InvalidInput))
            }
            Self::ResolveConflict {
                conflict_id,
                resolution,
            } if conflict_id.is_empty()
                || conflict_id.len() > MAX_BINDING_TEXT_BYTES
                || resolution.is_empty()
                || resolution.len() > MAX_OPERATION_BYTES =>
            {
                Err(BindingError::new(BindingErrorCode::InvalidInput))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MobileOutcome {
    Opened,
    SavedLocally { operation_id: String },
    SyncCheckpointed { more_work: bool },
    Status(MobileStatus),
    ConflictResolutionQueued { operation_id: String },
    SanitizedDiagnostics { bytes: Vec<u8> },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BindingErrorCode {
    InvalidInput,
    IncompatibleAbi,
    StorageUnavailable,
    AuthenticationRequired,
    UpgradeRequired,
    Busy,
    Internal,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("mobile binding error {code:?}")]
pub struct BindingError {
    pub code: BindingErrorCode,
}

impl BindingError {
    #[must_use]
    pub const fn new(code: BindingErrorCode) -> Self {
        Self { code }
    }
}

#[async_trait]
pub trait MobileBindingHost: Send + Sync {
    async fn execute(&self, command: MobileCommand) -> Result<MobileOutcome, BindingError>;
}

/// Panic-contained owner of one logical Rust client/local store.
pub struct MobileBinding<H> {
    host: H,
    identity: MobileBuildIdentity,
}

impl<H> MobileBinding<H>
where
    H: MobileBindingHost,
{
    /// Creates a binding only for the current ABI.
    ///
    /// # Errors
    ///
    /// Returns an incompatible-ABI error for invalid build identity.
    pub fn new(host: H, identity: MobileBuildIdentity) -> Result<Self, BindingError> {
        identity.validate()?;
        Ok(Self { host, identity })
    }

    #[must_use]
    pub const fn identity(&self) -> &MobileBuildIdentity {
        &self.identity
    }

    /// Executes an asynchronous command without allowing a Rust panic to unwind through FFI.
    /// Cancellation of the returned host future never rolls back already-durable intent.
    ///
    /// # Errors
    ///
    /// Returns validated host errors, or stable `Internal` if the Rust host panics.
    pub async fn call(&self, command: MobileCommand) -> Result<MobileOutcome, BindingError> {
        command.validate()?;
        AssertUnwindSafe(self.host.execute(command))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(BindingError::new(BindingErrorCode::Internal)))
    }
}

/// Bounded event buffer that coalesces data invalidations by scope.
#[derive(Clone, Debug, Default)]
pub struct MobileEventBuffer {
    priority: Vec<MobileEvent>,
    changed_scopes: BTreeMap<String, MobileEvent>,
}

impl MobileEventBuffer {
    /// Adds an event while bounding pending UI state. Data-change events coalesce by scope.
    ///
    /// # Errors
    ///
    /// Returns `Busy` when distinct priority events exceed the hard bound.
    pub fn push(&mut self, event: MobileEvent) -> Result<(), BindingError> {
        if let MobileEvent::DataChanged { scope } = &event {
            if self.changed_scopes.len() >= MAX_PENDING_EVENTS
                && !self.changed_scopes.contains_key(scope)
            {
                return Err(BindingError::new(BindingErrorCode::Busy));
            }
            self.changed_scopes.insert(scope.clone(), event);
        } else {
            if self.priority.len() >= MAX_PENDING_EVENTS {
                return Err(BindingError::new(BindingErrorCode::Busy));
            }
            self.priority.push(event);
        }
        Ok(())
    }

    #[must_use]
    pub fn drain(&mut self) -> Vec<MobileEvent> {
        let mut events = std::mem::take(&mut self.priority);
        events.extend(std::mem::take(&mut self.changed_scopes).into_values());
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_invalidations_coalesce_without_raw_journal_delivery() {
        let mut events = MobileEventBuffer::default();
        assert!(
            events
                .push(MobileEvent::DataChanged {
                    scope: "school".to_owned()
                })
                .is_ok()
        );
        assert!(
            events
                .push(MobileEvent::DataChanged {
                    scope: "school".to_owned()
                })
                .is_ok()
        );
        assert_eq!(events.drain().len(), 1);
    }
}
