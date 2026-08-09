//! Database-independent optimistic-version conflict detection.

use aequora_protocol::{ChangeKind, ConflictPolicy, OperationEnvelope};
use aequora_types::{EntityVersion, HybridTimestamp};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, sync::Arc};
use thiserror::Error;

/// Result of comparing a client base version with authoritative state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionCheck {
    /// The operation can proceed without a version conflict.
    Current,
    /// The operation targets a missing entity but expected one to exist.
    Missing,
    /// The operation is based on a stale or otherwise different version.
    Diverged {
        client: Option<EntityVersion>,
        server: EntityVersion,
    },
}

/// Performs strict version checking. Creation requires no base and no current entity;
/// updates require exact equality.
#[must_use]
pub fn check_version(
    client_base: Option<EntityVersion>,
    server_version: Option<EntityVersion>,
) -> VersionCheck {
    match (client_base, server_version) {
        (None, None) => VersionCheck::Current,
        (Some(client), Some(server)) if client == server => VersionCheck::Current,
        (Some(_), None) => VersionCheck::Missing,
        (client, Some(server)) => VersionCheck::Diverged { client, server },
    }
}

/// Generic conflict-policy hook. Applications should select policies by operation kind
/// and aggregate semantics rather than use one global last-writer-wins policy.
pub trait ConflictResolver: Send + Sync {
    /// Selects a policy for a detected version mismatch.
    fn policy(&self, operation_kind: u16) -> ConflictPolicy;

    /// Attempts an application-registered deterministic merge after authorization and domain
    /// execution have produced a candidate mutation.
    ///
    /// # Errors
    ///
    /// Returns [`MergeError`] when registered application payloads cannot be merged safely.
    fn merge(&self, _input: MergeInput<'_>) -> Result<MergeDecision, MergeError> {
        Ok(MergeDecision::Unresolved {
            message: "no deterministic merger is registered".to_owned(),
        })
    }
}

/// Current and candidate state supplied to a deterministic conflict merger.
#[derive(Clone, Copy, Debug)]
pub struct MergeInput<'a> {
    /// Operation whose base version diverged.
    pub operation: &'a OperationEnvelope,
    /// Current authoritative payload.
    pub current_payload: &'a [u8],
    /// Whether current authoritative state is a tombstone.
    pub current_tombstone: bool,
    /// Candidate payload produced by the authorized application executor.
    pub candidate_payload: &'a [u8],
    /// Candidate transition kind produced by the application executor.
    pub candidate_kind: ChangeKind,
}

/// Deterministic result of resolving a stale-base mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MergeDecision {
    /// Commit this merged authoritative state against the version that was actually read.
    Merged {
        /// Merged application payload.
        payload: Vec<u8>,
        /// Resulting transition kind.
        change_kind: ChangeKind,
    },
    /// Preserve the normal conflict-inbox behavior.
    Unresolved {
        /// Bounded non-sensitive reason.
        message: String,
    },
}

/// A registered merge could not decode or encode application state.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("conflict merge failed: {message}")]
pub struct MergeError {
    /// Non-sensitive diagnostic text.
    pub message: String,
}

impl MergeError {
    /// Creates a merge failure without exposing domain payloads.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Application-provided deterministic merge implementation.
pub trait MergeStrategy: Send + Sync {
    /// Resolves current and candidate authoritative state.
    ///
    /// # Errors
    ///
    /// Returns [`MergeError`] when application payloads are incompatible or malformed.
    fn merge(&self, input: MergeInput<'_>) -> Result<MergeDecision, MergeError>;
}

/// Safe default that rejects every version conflict.
#[derive(Clone, Copy, Debug, Default)]
pub struct RejectConflicts;

impl ConflictResolver for RejectConflicts {
    fn policy(&self, _operation_kind: u16) -> ConflictPolicy {
        ConflictPolicy::Reject
    }
}

/// Marker implemented by an application's strongly typed operation command.
pub trait TypedOperation {
    /// Compact operation kind registered for the command's wire payload.
    const KIND: u16;
}

/// Marker for accounting, payment, inventory, or other value-bearing operations.
///
/// Applications opt in explicitly so registration can reject replacement-style conflict
/// policies at startup instead of discovering an unsafe policy during synchronization.
pub trait FinancialOperation: TypedOperation {}

/// An unsafe conflict policy was requested for a value-bearing operation.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("conflict policy {policy:?} is unsafe for financial operation kind {operation_kind}")]
pub struct FinancialPolicyError {
    /// Registered wire operation kind.
    pub operation_kind: u16,
    /// Policy rejected by the financial guard.
    pub policy: ConflictPolicy,
}

/// Conflict policies registered by Rust operation type with a safe reject fallback.
#[derive(Clone)]
pub struct ConflictPolicyRegistry {
    policies: HashMap<u16, ConflictPolicy>,
    mergers: HashMap<u16, Arc<dyn MergeStrategy>>,
    fallback: ConflictPolicy,
}

impl fmt::Debug for ConflictPolicyRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConflictPolicyRegistry")
            .field("policies", &self.policies)
            .field("registered_mergers", &self.mergers.len())
            .field("fallback", &self.fallback)
            .finish()
    }
}

impl Default for ConflictPolicyRegistry {
    fn default() -> Self {
        Self {
            policies: HashMap::new(),
            mergers: HashMap::new(),
            fallback: ConflictPolicy::Reject,
        }
    }
}

impl ConflictPolicyRegistry {
    /// Creates a registry whose unregistered operations use `fallback`.
    #[must_use]
    pub fn new(fallback: ConflictPolicy) -> Self {
        Self {
            policies: HashMap::new(),
            mergers: HashMap::new(),
            fallback,
        }
    }

    /// Registers a policy using the command's Rust type instead of a call-site integer.
    pub fn register<O: TypedOperation>(&mut self, policy: ConflictPolicy) -> &mut Self {
        self.policies.insert(O::KIND, policy);
        self
    }

    /// Registers both a conflict policy and its deterministic typed merger.
    pub fn register_merger<O, M>(&mut self, policy: ConflictPolicy, merger: M) -> &mut Self
    where
        O: TypedOperation,
        M: MergeStrategy + 'static,
    {
        self.policies.insert(O::KIND, policy);
        self.mergers.insert(O::KIND, Arc::new(merger));
        self
    }

    /// Registers a value-bearing operation using conflict semantics that cannot silently
    /// replace authoritative financial state.
    ///
    /// Append-only commands normally use [`ConflictPolicy::Reject`]. Algebraic deltas may use
    /// [`ConflictPolicy::CommutativeOperation`], and exceptional workflows may be sent to
    /// [`ConflictPolicy::ManualResolution`]. A custom financial merge must instead use
    /// [`Self::register_financial_merger`] so its implementation is present atomically.
    ///
    /// # Errors
    ///
    /// Returns [`FinancialPolicyError`] for replacement, field, CRDT, or unimplemented custom
    /// policies.
    pub fn register_financial<O>(
        &mut self,
        policy: ConflictPolicy,
    ) -> Result<&mut Self, FinancialPolicyError>
    where
        O: FinancialOperation,
    {
        if !matches!(
            policy,
            ConflictPolicy::Reject
                | ConflictPolicy::CommutativeOperation
                | ConflictPolicy::ManualResolution
        ) {
            return Err(FinancialPolicyError {
                operation_kind: O::KIND,
                policy,
            });
        }
        self.policies.insert(O::KIND, policy);
        Ok(self)
    }

    /// Registers an explicit deterministic domain merger for a value-bearing operation.
    pub fn register_financial_merger<O, M>(&mut self, merger: M) -> &mut Self
    where
        O: FinancialOperation,
        M: MergeStrategy + 'static,
    {
        self.policies.insert(O::KIND, ConflictPolicy::CustomMerge);
        self.mergers.insert(O::KIND, Arc::new(merger));
        self
    }
}

impl ConflictResolver for ConflictPolicyRegistry {
    fn policy(&self, operation_kind: u16) -> ConflictPolicy {
        self.policies
            .get(&operation_kind)
            .copied()
            .unwrap_or(self.fallback)
    }

    fn merge(&self, input: MergeInput<'_>) -> Result<MergeDecision, MergeError> {
        self.mergers
            .get(&input.operation.operation_kind.0)
            .map_or_else(
                || {
                    Ok(MergeDecision::Unresolved {
                        message: "no deterministic merger is registered".to_owned(),
                    })
                },
                |merger| merger.merge(input),
            )
    }
}

/// One independently timestamped application field used by [`FieldSetMerger`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FieldValue {
    /// Stable application-defined field number.
    pub field: u16,
    /// Causal timestamp for this field only.
    pub timestamp: HybridTimestamp,
    /// Application-owned encoded field value.
    pub value: Vec<u8>,
}

/// Canonical field-level state representation for applications that opt into the provided merger.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct FieldSet {
    /// Fields sorted by field number with at most one entry per number.
    pub fields: Vec<FieldValue>,
}

impl FieldSet {
    /// Sorts fields and keeps the deterministic newest value for duplicate field numbers.
    #[must_use]
    pub fn canonical(mut fields: Vec<FieldValue>) -> Self {
        fields.sort_by(|left, right| {
            left.field
                .cmp(&right.field)
                .then_with(|| left.timestamp.cmp(&right.timestamp))
                .then_with(|| left.value.cmp(&right.value))
        });
        let mut canonical: Vec<FieldValue> = Vec::with_capacity(fields.len());
        for field in fields {
            if canonical
                .last()
                .is_some_and(|last| last.field == field.field)
            {
                let _replaced = canonical.pop();
            }
            canonical.push(field);
        }
        Self { fields: canonical }
    }

    /// Merges independently timestamped fields. Equal timestamps use encoded-value ordering as
    /// a stable tie-breaker, so merge order cannot change the result.
    #[must_use]
    pub fn merge(&self, other: &Self) -> Self {
        let mut fields = Vec::with_capacity(self.fields.len().saturating_add(other.fields.len()));
        fields.extend(self.fields.iter().cloned());
        fields.extend(other.fields.iter().cloned());
        Self::canonical(fields)
    }
}

/// Postcard field-set merger for profile/preferences-style entities.
#[derive(Clone, Copy, Debug, Default)]
pub struct FieldSetMerger;

impl MergeStrategy for FieldSetMerger {
    fn merge(&self, input: MergeInput<'_>) -> Result<MergeDecision, MergeError> {
        if input.current_tombstone || input.candidate_kind == ChangeKind::Tombstone {
            return Ok(MergeDecision::Unresolved {
                message: "field merge does not implicitly resolve deletion conflicts".to_owned(),
            });
        }
        let current: FieldSet = postcard::from_bytes(input.current_payload)
            .map_err(|_| MergeError::new("current field set is malformed"))?;
        let candidate: FieldSet = postcard::from_bytes(input.candidate_payload)
            .map_err(|_| MergeError::new("candidate field set is malformed"))?;
        let payload = postcard::to_stdvec(&current.merge(&candidate))
            .map_err(|_| MergeError::new("merged field set could not be encoded"))?;
        Ok(MergeDecision::Merged {
            payload,
            change_kind: ChangeKind::Upsert,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_types::NodeId;

    struct ProfileUpdate;
    impl TypedOperation for ProfileUpdate {
        const KIND: u16 = 7;
    }

    struct PostPayment;
    impl TypedOperation for PostPayment {
        const KIND: u16 = 8;
    }
    impl FinancialOperation for PostPayment {}

    #[test]
    fn policies_are_registered_by_operation_type() {
        let mut registry = ConflictPolicyRegistry::default();
        registry.register::<ProfileUpdate>(ConflictPolicy::CustomMerge);
        assert_eq!(registry.policy(7), ConflictPolicy::CustomMerge);
        assert_eq!(registry.policy(8), ConflictPolicy::Reject);
    }

    #[test]
    fn financial_registration_rejects_silent_state_replacement() {
        let mut registry = ConflictPolicyRegistry::default();
        for policy in [
            ConflictPolicy::ServerWins,
            ConflictPolicy::ClientWins,
            ConflictPolicy::FieldMerge,
            ConflictPolicy::Crdt,
            ConflictPolicy::CustomMerge,
        ] {
            assert_eq!(
                registry.register_financial::<PostPayment>(policy).err(),
                Some(FinancialPolicyError {
                    operation_kind: PostPayment::KIND,
                    policy,
                })
            );
        }
        registry
            .register_financial::<PostPayment>(ConflictPolicy::Reject)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(registry.policy(PostPayment::KIND), ConflictPolicy::Reject);
    }

    #[test]
    fn field_merge_is_commutative_and_keeps_independent_updates() {
        let node = NodeId::new();
        let older = HybridTimestamp {
            physical_ms: 1,
            logical: 0,
            node,
        };
        let newer = HybridTimestamp {
            physical_ms: 2,
            logical: 0,
            node,
        };
        let server = FieldSet::canonical(vec![
            FieldValue {
                field: 1,
                timestamp: newer,
                value: b"server-name".to_vec(),
            },
            FieldValue {
                field: 2,
                timestamp: older,
                value: b"old-email".to_vec(),
            },
        ]);
        let client = FieldSet::canonical(vec![FieldValue {
            field: 2,
            timestamp: newer,
            value: b"new-email".to_vec(),
        }]);

        assert_eq!(server.merge(&client), client.merge(&server));
        let merged = server.merge(&client);
        assert_eq!(merged.fields.len(), 2);
        assert_eq!(merged.fields[0].value, b"server-name");
        assert_eq!(merged.fields[1].value, b"new-email");
    }
}
