//! Canonical stable-ID registry and release compatibility reports.

use crate::{
    CapabilityCategory, CapabilityId, CapabilityRequirementKind, CompatibilityError,
    OperationKindId, RecoveryInstruction,
};
use aequora_types::{ProtocolVersion, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Lifecycle shared by protocols, capabilities, and operation schemas.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SupportStatus {
    Experimental,
    Current,
    Supported,
    Deprecated,
    RetryOnly,
    Removed,
}

/// One allocated wire protocol version.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolDescriptor {
    pub version: ProtocolVersion,
    pub status: SupportStatus,
    pub introduced_release: String,
}

/// One stable capability allocation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityDescriptor {
    pub id: CapabilityId,
    pub name: String,
    pub category: CapabilityCategory,
    pub requirement: CapabilityRequirementKind,
    pub status: SupportStatus,
}

/// Compatibility metadata for one operation kind.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationDescriptor {
    pub kind: OperationKindId,
    pub name: String,
    pub minimum_schema: SchemaVersion,
    pub current_schema: SchemaVersion,
    pub minimum_protocol: ProtocolVersion,
    pub status: SupportStatus,
    pub minimum_server_capability: Option<CapabilityId>,
    /// Product-governed releases for which historical retries remain accepted.
    pub retry_horizon_releases: u16,
    /// Explicit recovery once this schema is no longer accepted.
    pub removal_recovery: Vec<RecoveryInstruction>,
}

/// Reviewed source of truth. Removed IDs remain listed in a reserved set forever.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CompatibilityRegistry {
    pub registry_generation: u64,
    pub protocols: Vec<ProtocolDescriptor>,
    pub capabilities: Vec<CapabilityDescriptor>,
    pub operations: Vec<OperationDescriptor>,
    pub reserved_protocol_versions: BTreeSet<ProtocolVersion>,
    pub reserved_capability_ids: BTreeSet<CapabilityId>,
    pub reserved_operation_ids: BTreeSet<OperationKindId>,
}

impl CompatibilityRegistry {
    /// Enforces stable IDs, non-decreasing ranges, and complete removal reservations.
    ///
    /// # Errors
    ///
    /// Returns a registry error for duplicate, reused, zero, or incomplete entries.
    pub fn validate(&self) -> Result<(), CompatibilityError> {
        if self.registry_generation == 0 || self.protocols.is_empty() {
            return Err(CompatibilityError::InvalidRegistryEntry);
        }
        let protocols = self
            .protocols
            .iter()
            .map(|descriptor| descriptor.version)
            .collect::<BTreeSet<_>>();
        if protocols.len() != self.protocols.len()
            || self
                .protocols
                .iter()
                .any(|entry| entry.version.0 == 0 || entry.introduced_release.is_empty())
        {
            return Err(CompatibilityError::DuplicateRegistryId);
        }
        if self.protocols.iter().any(|entry| {
            entry.status == SupportStatus::Removed
                && !self.reserved_protocol_versions.contains(&entry.version)
        }) || self.reserved_protocol_versions.iter().any(|version| {
            protocols.contains(version)
                && !self.protocols.iter().any(|entry| {
                    entry.version == *version && entry.status == SupportStatus::Removed
                })
        }) {
            return Err(CompatibilityError::ReservedRegistryIdReused);
        }

        let capabilities = self
            .capabilities
            .iter()
            .map(|descriptor| descriptor.id)
            .collect::<BTreeSet<_>>();
        if capabilities.len() != self.capabilities.len() {
            return Err(CompatibilityError::DuplicateRegistryId);
        }
        if self.capabilities.iter().any(|entry| {
            entry.id.0 == 0
                || entry.name.is_empty()
                || (entry.status == SupportStatus::Removed
                    && !self.reserved_capability_ids.contains(&entry.id))
        }) {
            return Err(CompatibilityError::InvalidRegistryEntry);
        }
        if self
            .reserved_capability_ids
            .iter()
            .any(|id| capabilities.contains(id) && !self.is_removed_capability(*id))
        {
            return Err(CompatibilityError::ReservedRegistryIdReused);
        }

        let operations = self
            .operations
            .iter()
            .map(|descriptor| descriptor.kind)
            .collect::<BTreeSet<_>>();
        if operations.len() != self.operations.len() {
            return Err(CompatibilityError::DuplicateRegistryId);
        }
        if self.operations.iter().any(|entry| {
            entry.kind.0 == 0
                || entry.name.is_empty()
                || entry.minimum_schema.0 == 0
                || entry.current_schema < entry.minimum_schema
                || !protocols.contains(&entry.minimum_protocol)
                || entry
                    .minimum_server_capability
                    .is_some_and(|id| !capabilities.contains(&id))
                || (matches!(
                    entry.status,
                    SupportStatus::Deprecated | SupportStatus::RetryOnly
                ) && entry.retry_horizon_releases == 0)
                || (entry.status == SupportStatus::Removed && entry.removal_recovery.is_empty())
                || (entry.status == SupportStatus::Removed
                    && !self.reserved_operation_ids.contains(&entry.kind))
        }) {
            return Err(CompatibilityError::InvalidRegistryEntry);
        }
        if self
            .reserved_operation_ids
            .iter()
            .any(|id| operations.contains(id) && !self.is_removed_operation(*id))
        {
            return Err(CompatibilityError::ReservedRegistryIdReused);
        }
        Ok(())
    }

    fn is_removed_capability(&self, id: CapabilityId) -> bool {
        self.capabilities
            .iter()
            .any(|entry| entry.id == id && entry.status == SupportStatus::Removed)
    }

    fn is_removed_operation(&self, id: OperationKindId) -> bool {
        self.operations
            .iter()
            .any(|entry| entry.kind == id && entry.status == SupportStatus::Removed)
    }

    /// Produces a payload-free release report.
    #[must_use]
    pub fn report(&self) -> CompatibilityReport {
        CompatibilityReport {
            registry_generation: self.registry_generation,
            protocol_count: self.protocols.len(),
            capability_count: self.capabilities.len(),
            operation_count: self.operations.len(),
            deprecated_protocols: self
                .protocols
                .iter()
                .filter(|entry| entry.status == SupportStatus::Deprecated)
                .map(|entry| entry.version)
                .collect(),
            retry_only_operations: self
                .operations
                .iter()
                .filter(|entry| entry.status == SupportStatus::RetryOnly)
                .map(|entry| entry.kind)
                .collect(),
        }
    }
}

/// Bounded, payload-free registry report for CLI and release evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompatibilityReport {
    pub registry_generation: u64,
    pub protocol_count: usize,
    pub capability_count: usize,
    pub operation_count: usize,
    pub deprecated_protocols: Vec<ProtocolVersion>,
    pub retry_only_operations: Vec<OperationKindId>,
}

/// Returns the reviewed repository registry compiled into this crate.
///
/// # Errors
///
/// Returns a decode or validation error if the checked-in registry is malformed.
pub fn canonical_registry() -> Result<CompatibilityRegistry, CompatibilityError> {
    let registry: CompatibilityRegistry = ron::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/registry/compatibility.ron"
    )))
    .map_err(|error| CompatibilityError::RegistryDecode(error.to_string()))?;
    registry.validate()?;
    Ok(registry)
}
