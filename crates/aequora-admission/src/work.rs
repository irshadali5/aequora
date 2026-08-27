use aequora_scheduler::{WorkClass, WorkKind};
use aequora_types::{SyncScopeId, TenantId};
use serde::{Deserialize, Serialize};

use crate::{AdmissionRejection, PolicyError};

/// Abstract, conservative work estimate used for admission rather than billing.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CostUnits(u32);

impl CostUnits {
    /// Creates a non-zero cost estimate. Zero is conservatively normalized to one unit.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(if value == 0 { 1 } else { value })
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Default for CostUnits {
    fn default() -> Self {
        Self(1)
    }
}

/// Independently protected server resource domains.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ResourceDomain {
    HttpRequest,
    SyncExchange,
    DatabaseTransaction,
    InteractiveCpu,
    BulkCpu,
    MaintenanceCpu,
    SnapshotBuild,
    SnapshotDownload,
    BlobTransfer,
    LiveConnection,
    BackgroundJob,
}

/// Explicit bound for one hierarchical or resource-specific admission scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourceBudget {
    pub max_in_flight: usize,
    pub max_queue: usize,
    pub max_bytes_in_flight: u64,
    pub max_cost_units_in_flight: u64,
}

impl ResourceBudget {
    /// Validates all execution bounds. A zero queue explicitly means immediate rejection.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidResourceBudget`] when an execution bound is zero.
    pub const fn validate(self) -> Result<(), PolicyError> {
        if self.max_in_flight == 0
            || self.max_bytes_in_flight == 0
            || self.max_cost_units_in_flight == 0
        {
            return Err(PolicyError::InvalidResourceBudget);
        }
        Ok(())
    }
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self {
            max_in_flight: 256,
            max_queue: 512,
            max_bytes_in_flight: 256 * 1_024 * 1_024,
            max_cost_units_in_flight: 100_000,
        }
    }
}

/// Cheap shape information checked before domain validation or authoritative mutation.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RequestShape {
    pub operations: usize,
    pub encoded_bytes: u64,
    pub decompressed_bytes: u64,
    pub scopes: usize,
    pub dependency_edges: usize,
    pub dependency_depth: usize,
}

/// Hard limits for attacker-controlled request and dependency shapes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RequestLimits {
    pub max_operations: usize,
    pub max_encoded_bytes: u64,
    pub max_decompressed_bytes: u64,
    pub max_scopes: usize,
    pub max_dependency_edges: usize,
    pub max_dependency_depth: usize,
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self {
            max_operations: 1_000,
            max_encoded_bytes: 4 * 1_024 * 1_024,
            max_decompressed_bytes: 16 * 1_024 * 1_024,
            max_scopes: 32,
            max_dependency_edges: 5_000,
            max_dependency_depth: 128,
        }
    }
}

impl RequestLimits {
    /// Validates non-zero and monotonic encoded/decompressed request bounds.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidRequestLimits`] for unsafe limits.
    pub const fn validate(self) -> Result<(), PolicyError> {
        if self.max_operations == 0
            || self.max_encoded_bytes == 0
            || self.max_decompressed_bytes < self.max_encoded_bytes
            || self.max_scopes == 0
            || self.max_dependency_edges == 0
            || self.max_dependency_depth == 0
        {
            return Err(PolicyError::InvalidRequestLimits);
        }
        Ok(())
    }

    /// Rejects oversized work before it enters a protected execution domain.
    ///
    /// # Errors
    ///
    /// Returns [`AdmissionRejection::RequestTooLarge`] or
    /// [`AdmissionRejection::TooManyDependencies`] when the shape exceeds a hard bound.
    pub const fn check(self, shape: RequestShape) -> Result<(), AdmissionRejection> {
        if shape.operations > self.max_operations
            || shape.encoded_bytes > self.max_encoded_bytes
            || shape.decompressed_bytes > self.max_decompressed_bytes
            || shape.scopes > self.max_scopes
        {
            return Err(AdmissionRejection::RequestTooLarge);
        }
        if shape.dependency_edges > self.max_dependency_edges
            || shape.dependency_depth > self.max_dependency_depth
        {
            return Err(AdmissionRejection::TooManyDependencies);
        }
        Ok(())
    }
}

/// Payload-free metadata used to make a complete hierarchical admission decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkDescriptor {
    pub tenant_id: TenantId,
    /// Server-derived contractual weight, clamped by [`crate::TenantAdmissionPolicy`].
    pub tenant_weight: u16,
    pub kind: WorkKind,
    pub class: WorkClass,
    pub cost: CostUnits,
    pub shape: RequestShape,
    pub scope: Option<SyncScopeId>,
    pub estimated_response_bytes: u64,
    pub resources: Vec<ResourceDomain>,
}

impl WorkDescriptor {
    /// Total conservative bytes charged while the permit is alive.
    #[must_use]
    pub const fn estimated_bytes(&self) -> u64 {
        let bytes = self
            .shape
            .decompressed_bytes
            .saturating_add(self.estimated_response_bytes);
        if bytes == 0 { 1 } else { bytes }
    }
}

/// Server-owned classification rule for one logical work kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PriorityRule {
    pub kind: WorkKind,
    pub default: WorkClass,
    pub maximum_client_hint: WorkClass,
}

/// Bounded registry ensuring a caller hint cannot self-promote beyond server policy.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerPriorityPolicy {
    pub rules: Vec<PriorityRule>,
}

impl ServerPriorityPolicy {
    /// Classifies work from a server-owned kind and an optional untrusted hint.
    #[must_use]
    pub fn classify(&self, kind: WorkKind, client_hint: Option<WorkClass>) -> WorkClass {
        let rule = self.rules.iter().find(|rule| rule.kind == kind);
        let default = rule.map_or_else(|| kind.default_class(), |rule| rule.default);
        let maximum = rule.map_or(default, |rule| rule.maximum_client_hint);
        client_hint.map_or(default, |hint| {
            class_from_rank(hint.rank().min(maximum.rank()).min(default.rank()))
        })
    }
}

const fn class_from_rank(rank: u8) -> WorkClass {
    match rank {
        5.. => WorkClass::Critical,
        4 => WorkClass::Interactive,
        3 => WorkClass::Normal,
        2 => WorkClass::Bulk,
        1 => WorkClass::Background,
        _ => WorkClass::Maintenance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_priority_is_capped_by_server_policy() {
        let policy = ServerPriorityPolicy {
            rules: vec![PriorityRule {
                kind: WorkKind::BulkMigration,
                default: WorkClass::Bulk,
                maximum_client_hint: WorkClass::Normal,
            }],
        };
        assert_eq!(
            policy.classify(WorkKind::BulkMigration, Some(WorkClass::Critical)),
            WorkClass::Bulk
        );
        assert_eq!(
            policy.classify(WorkKind::BulkMigration, Some(WorkClass::Maintenance)),
            WorkClass::Maintenance
        );
    }

    #[test]
    fn request_shape_rejects_expensive_graph_before_execution() {
        let limits = RequestLimits::default();
        let shape = RequestShape {
            dependency_depth: limits.max_dependency_depth + 1,
            ..RequestShape::default()
        };
        assert_eq!(
            limits.check(shape),
            Err(AdmissionRejection::TooManyDependencies)
        );
    }
}
