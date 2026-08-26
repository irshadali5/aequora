//! Authorized, versioned, database-neutral synchronization datasets.
//!
//! A scope is a server-issued authorization contract, never an arbitrary client query. This crate
//! defines identity, cursor validation, resolver, membership, projection, subscription, and
//! transition semantics without importing a database or transport implementation.

use std::{collections::BTreeMap, collections::BTreeSet, sync::Arc};

use aequora_types::{
    ActorId, Cursor, DeviceId, EntityRef, OperationId, Sequence, SyncScopeId, TenantId,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Stable application-assigned scope-definition identity. Zero is reserved.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ScopeDefinitionId(u32);

impl ScopeDefinitionId {
    /// Creates a non-zero definition identity.
    ///
    /// # Errors
    ///
    /// Returns [`ScopeError::ZeroIdentity`] for the reserved zero value.
    pub const fn new(value: u32) -> Result<Self, ScopeError> {
        if value == 0 {
            Err(ScopeError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

macro_rules! monotonic_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub const INITIAL: Self = Self(1);

            /// Creates a non-zero monotonic identity.
            ///
            /// # Errors
            ///
            /// Returns [`ScopeError::ZeroIdentity`] for the reserved zero value.
            pub const fn new(value: u64) -> Result<Self, ScopeError> {
                if value == 0 {
                    Err(ScopeError::ZeroIdentity)
                } else {
                    Ok(Self(value))
                }
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }

            /// Advances without wrapping.
            ///
            /// # Errors
            ///
            /// Returns [`ScopeError::VersionExhausted`] at `u64::MAX`.
            pub const fn next(self) -> Result<Self, ScopeError> {
                match self.0.checked_add(1) {
                    Some(value) => Ok(Self(value)),
                    None => Err(ScopeError::VersionExhausted),
                }
            }
        }
    };
}

monotonic_id!(ScopeVersion, "Version of membership rules for one scope.");
monotonic_id!(
    ScopeGeneration,
    "Incompatible authority timeline generation for one scope."
);

macro_rules! uuid_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

uuid_id!(
    SubscriptionId,
    "Durable identity of one client subscription."
);
uuid_id!(
    ScopeTransitionId,
    "Retry-stable identity of one scope transition."
);

/// Versioned projection namespace. Different field visibility must never be merged accidentally.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct ProjectionVersion(pub u32);

/// Stable, bounded partition selected by a server-owned scope definition.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct DatasetPartitionId {
    pub kind: u16,
    pub value: Vec<u8>,
}

impl DatasetPartitionId {
    pub const MAX_VALUE_BYTES: usize = 1_024;

    /// Validates the portable partition representation.
    ///
    /// # Errors
    ///
    /// Returns a typed error for a reserved kind or unbounded value.
    pub fn validate(&self) -> Result<(), ScopeError> {
        if self.kind == 0 || self.value.is_empty() || self.value.len() > Self::MAX_VALUE_BYTES {
            return Err(ScopeError::InvalidPartition);
        }
        Ok(())
    }
}

/// Trusted authenticated identity supplied to a scope resolver by the server boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopePrincipal {
    pub tenant_id: TenantId,
    pub actor_id: ActorId,
    pub device_id: DeviceId,
}

/// Bounded client request for one registered definition. Parameters are opaque to the core.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeRequest {
    pub definition: ScopeDefinitionId,
    pub parameters: Vec<DatasetPartitionId>,
}

impl ScopeRequest {
    pub const MAX_PARAMETERS: usize = 32;

    /// Enforces structural request limits before application resolution.
    ///
    /// # Errors
    ///
    /// Returns a typed error when parameters are excessive or malformed.
    pub fn validate(&self) -> Result<(), ScopeError> {
        if self.parameters.len() > Self::MAX_PARAMETERS {
            return Err(ScopeError::TooManyParameters);
        }
        self.parameters
            .iter()
            .try_for_each(DatasetPartitionId::validate)
    }
}

/// Canonical server-owned dataset descriptor; it contains no raw SQL or executable predicate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeDescriptor {
    pub tenant_id: TenantId,
    pub definition: ScopeDefinitionId,
    pub partitions: BTreeSet<DatasetPartitionId>,
    pub policy_version: u32,
    pub projection: ProjectionVersion,
}

impl ScopeDescriptor {
    /// Validates canonical identity and bounds.
    ///
    /// # Errors
    ///
    /// Returns a typed error for tenant mismatch, zero policy, or malformed partitions.
    pub fn validate_for(&self, principal: ScopePrincipal) -> Result<(), ScopeError> {
        if self.tenant_id != principal.tenant_id {
            return Err(ScopeError::TenantMismatch);
        }
        if self.policy_version == 0 || self.partitions.len() > ScopeRequest::MAX_PARAMETERS {
            return Err(ScopeError::InvalidDescriptor);
        }
        self.partitions
            .iter()
            .try_for_each(DatasetPartitionId::validate)
    }
}

/// Complete authoritative binding returned by a registered resolver.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResolvedScope {
    pub scope_id: SyncScopeId,
    pub version: ScopeVersion,
    pub generation: ScopeGeneration,
    pub descriptor: ScopeDescriptor,
}

/// A journal watermark meaningful only with its complete scope binding.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeCursor {
    pub scope_id: SyncScopeId,
    pub version: ScopeVersion,
    pub generation: ScopeGeneration,
    pub sequence: Sequence,
}

impl ScopeCursor {
    /// Validates a cursor against the currently authorized binding and retention floor.
    #[must_use]
    pub fn validate(self, current: &ResolvedScope, retained_floor: Sequence) -> CursorValidity {
        if self.scope_id != current.scope_id {
            CursorValidity::ScopeChanged
        } else if self.generation != current.generation {
            CursorValidity::ResyncRequired
        } else if self.version != current.version {
            CursorValidity::ScopeChanged
        } else if self.sequence < retained_floor {
            CursorValidity::ResyncRequired
        } else {
            CursorValidity::Valid
        }
    }

    /// Narrows a validated binding to the legacy cursor consumed by journal, bootstrap, and
    /// anti-entropy adapters. Callers retain this full binding beside the returned value.
    #[must_use]
    pub const fn legacy_cursor(self) -> Cursor {
        Cursor {
            scope: self.scope_id,
            sequence: self.sequence,
        }
    }
}

/// Server result for one bound cursor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CursorValidity {
    Valid,
    ScopeChanged,
    ResyncRequired,
    UpgradeRequired,
    Forbidden,
}

/// One authoritative journal entry evaluated by a server-owned scope filter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvaluatedJournalEntry<E> {
    pub sequence: Sequence,
    pub value: E,
}

/// Filtered delivery plus a watermark through all evaluated entries, including excluded ones.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FilteredScopePage<P> {
    pub projected: Vec<P>,
    pub next_cursor: ScopeCursor,
    pub has_more: bool,
}

/// Applies a deterministic authorized projection while advancing through intentionally excluded
/// journal entries.
///
/// `evaluated_through` is the greatest global sequence the authoritative store proved complete for
/// this page. Therefore the returned scope cursor is a watermark, not merely the last delivered
/// event.
///
/// # Errors
///
/// Returns a typed error for a mismatched prior cursor, decreasing/incomplete watermark, or
/// unordered journal input.
pub fn project_filtered_page<E, P, F>(
    scope: &ResolvedScope,
    prior: Option<ScopeCursor>,
    evaluated_through: Sequence,
    has_more: bool,
    entries: impl IntoIterator<Item = EvaluatedJournalEntry<E>>,
    mut project: F,
) -> Result<FilteredScopePage<P>, ScopeError>
where
    F: FnMut(&ResolvedScope, E) -> Option<P>,
{
    let prior_sequence = if let Some(cursor) = prior {
        if cursor.validate(scope, Sequence(0)) != CursorValidity::Valid {
            return Err(ScopeError::CursorMismatch);
        }
        cursor.sequence
    } else {
        Sequence(0)
    };
    if evaluated_through < prior_sequence {
        return Err(ScopeError::CursorMismatch);
    }
    let mut last = prior_sequence;
    let mut projected = Vec::new();
    for entry in entries {
        if entry.sequence <= last || entry.sequence > evaluated_through {
            return Err(ScopeError::UnorderedJournal);
        }
        last = entry.sequence;
        if let Some(value) = project(scope, entry.value) {
            projected.push(value);
        }
    }
    Ok(FilteredScopePage {
        projected,
        next_cursor: ScopeCursor {
            scope_id: scope.scope_id,
            version: scope.version,
            generation: scope.generation,
            sequence: evaluated_through,
        },
        has_more,
    })
}

/// Current server view of one subscription.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScopeServerState {
    Active,
    Expanded,
    Contracted,
    Suspended,
    Revoked,
    GenerationChanged,
}

/// Payload-free status suitable for a negotiated scope control message.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeStatus {
    pub scope_id: SyncScopeId,
    pub version: ScopeVersion,
    pub generation: ScopeGeneration,
    pub state: ScopeServerState,
}

/// Versioned server instruction. Legacy peers safely map non-continue instructions to full resync.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScopeTransitionInstruction {
    Continue,
    ExpandWithBootstrap {
        transition_id: ScopeTransitionId,
        boundary: ScopeCursor,
    },
    ContractWithRemovals {
        transition_id: ScopeTransitionId,
        boundary: ScopeCursor,
        removals: Vec<ScopeRemoval>,
    },
    FullResync,
    Revoke,
    Suspend,
}

/// Safe bootstrap strategy for a version transition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScopeBootstrapMode {
    FullScope,
    AddedPartitions,
}

/// Scope-bound bootstrap plan; activation still requires an atomic [`ScopeTransition`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeBootstrapPlan {
    pub transition_id: ScopeTransitionId,
    pub target: ResolvedScope,
    pub boundary: ScopeCursor,
    pub mode: ScopeBootstrapMode,
    pub partitions: BTreeSet<DatasetPartitionId>,
}

impl ScopeBootstrapPlan {
    /// Validates snapshot identity, version, generation, and partial partitions together.
    ///
    /// # Errors
    ///
    /// Returns a typed error instead of allowing a mixed-scope bootstrap.
    pub fn validate(&self) -> Result<(), ScopeError> {
        if self.boundary.scope_id != self.target.scope_id
            || self.boundary.version != self.target.version
            || self.boundary.generation != self.target.generation
            || (self.mode == ScopeBootstrapMode::FullScope && !self.partitions.is_empty())
            || (self.mode == ScopeBootstrapMode::AddedPartitions && self.partitions.is_empty())
            || !self
                .partitions
                .is_subset(&self.target.descriptor.partitions)
        {
            return Err(ScopeError::InvalidTransition);
        }
        Ok(())
    }
}

/// Authorization hook for blobs, integrity manifests, search indexes, or other scope-bound
/// resources. Possessing a stale resource identifier never grants access by itself.
pub trait ScopeResourceAuthorizer<R>: Send + Sync {
    fn authorized(&self, principal: ScopePrincipal, scope: &ResolvedScope, resource: &R) -> bool;
}

/// Server-owned asynchronous scope resolution contract.
#[async_trait]
pub trait ScopeResolver: Send + Sync {
    fn definition_id(&self) -> ScopeDefinitionId;

    async fn resolve(
        &self,
        principal: ScopePrincipal,
        request: &ScopeRequest,
    ) -> Result<ResolvedScope, ScopeError>;
}

/// Definition registry that rejects duplicate identifiers and unknown client requests.
#[derive(Default)]
pub struct ScopeRegistry {
    definitions: BTreeMap<ScopeDefinitionId, Arc<dyn ScopeResolver>>,
}

impl ScopeRegistry {
    /// Registers one definition.
    ///
    /// # Errors
    ///
    /// Returns [`ScopeError::DuplicateDefinition`] instead of silently replacing policy.
    pub fn register(&mut self, resolver: Arc<dyn ScopeResolver>) -> Result<(), ScopeError> {
        let id = resolver.definition_id();
        if self.definitions.contains_key(&id) {
            return Err(ScopeError::DuplicateDefinition(id));
        }
        self.definitions.insert(id, resolver);
        Ok(())
    }

    /// Resolves and validates a requested scope after authentication.
    ///
    /// # Errors
    ///
    /// Returns a typed request, lookup, authorization, or descriptor error.
    pub async fn resolve(
        &self,
        principal: ScopePrincipal,
        request: &ScopeRequest,
    ) -> Result<ResolvedScope, ScopeError> {
        request.validate()?;
        let definition = self
            .definitions
            .get(&request.definition)
            .ok_or(ScopeError::UnknownDefinition(request.definition))?;
        let resolved = definition.resolve(principal, request).await?;
        if resolved.descriptor.definition != request.definition {
            return Err(ScopeError::InvalidDescriptor);
        }
        resolved.descriptor.validate_for(principal)?;
        Ok(resolved)
    }
}

/// Deterministic membership result for an authoritative entity/event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MembershipDecision {
    Include,
    Exclude,
    RequiresProjection,
}

/// Pure application membership boundary.
pub trait MembershipEvaluator<E>: Send + Sync {
    fn membership(&self, scope: &ResolvedScope, entity: &E) -> MembershipDecision;
}

/// Authorized projection boundary. `None` means the entity must not be delivered.
pub trait ProjectionRule<E>: Send + Sync {
    type Projected;

    fn project(&self, scope: &ResolvedScope, entity: &E) -> Option<Self::Projected>;
}

/// Durable client subscription lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SubscriptionState {
    Requested,
    Resolved,
    Bootstrapping,
    Active,
    Expanding,
    Contracting,
    Suspended,
    Revoked,
    ResyncRequired,
}

/// Durable relationship between a client and one resolved scope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Subscription {
    pub subscription_id: SubscriptionId,
    pub scope: ResolvedScope,
    pub state: SubscriptionState,
    pub cursor: Option<ScopeCursor>,
    pub pending_transition: Option<ScopeTransitionId>,
}

/// Why membership ended without deleting the authoritative domain entity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScopeRemovalReason {
    MembershipChanged,
    PermissionDowngrade,
    SubscriptionEnded,
    Revoked,
}

/// Scope-local removal, deliberately distinct from a domain tombstone.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeRemoval {
    pub scope_id: SyncScopeId,
    pub projection: ProjectionVersion,
    pub entity: EntityRef,
    pub reason: ScopeRemovalReason,
}

/// One active entity-to-scope reference.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MembershipRecord {
    pub scope_id: SyncScopeId,
    pub projection: ProjectionVersion,
    pub entity: EntityRef,
    pub membership_version: ScopeVersion,
}

/// Explicit disposition of pending intent affected by authorization loss.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PendingIntentDisposition {
    AuthorizationLost,
    ScopeRevoked,
    ScopeRemoved,
}

/// Local retention behavior for data leaving an active scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LocalRetentionPolicy {
    RemoveImmediately,
    RetainReadOnly,
    RetainUntilUnixMs(u64),
    ApplicationManaged,
}

/// Coherent transition category.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScopeTransitionKind {
    FullBootstrap,
    Expansion {
        added_partitions: BTreeSet<DatasetPartitionId>,
    },
    Contraction {
        removed_partitions: BTreeSet<DatasetPartitionId>,
        retention: LocalRetentionPolicy,
    },
    Revocation,
    Suspension,
    GenerationReset,
}

/// Retry-stable atomic transition input for a local adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeTransition {
    pub transition_id: ScopeTransitionId,
    pub subscription_id: SubscriptionId,
    pub from_version: ScopeVersion,
    pub from_generation: ScopeGeneration,
    pub target: Option<ResolvedScope>,
    pub boundary: Option<ScopeCursor>,
    pub kind: ScopeTransitionKind,
    pub additions: Vec<MembershipRecord>,
    pub removals: Vec<ScopeRemoval>,
    pub affected_pending_operations: Vec<OperationId>,
    /// False means expansion/bootstrap staging may persist but cannot become active.
    pub staging_complete: bool,
}

impl ScopeTransition {
    /// Validates identity/version/generation coherence before any adapter mutation.
    ///
    /// # Errors
    ///
    /// Returns a typed error for mixed scopes, stale versions, or unsafe activation.
    pub fn validate(&self, current: &Subscription) -> Result<(), ScopeError> {
        if current.subscription_id != self.subscription_id
            || current.scope.version != self.from_version
            || current.scope.generation != self.from_generation
        {
            return Err(ScopeError::StaleTransition);
        }
        if matches!(
            self.kind,
            ScopeTransitionKind::Revocation | ScopeTransitionKind::Suspension
        ) {
            if self.target.is_some() || self.boundary.is_some() {
                return Err(ScopeError::InvalidTransition);
            }
        } else {
            let target = self.target.as_ref().ok_or(ScopeError::InvalidTransition)?;
            let boundary = self.boundary.ok_or(ScopeError::InvalidTransition)?;
            if target.scope_id != current.scope.scope_id
                || boundary.scope_id != target.scope_id
                || boundary.version != target.version
                || boundary.generation != target.generation
                || target.descriptor.tenant_id != current.scope.descriptor.tenant_id
            {
                return Err(ScopeError::InvalidTransition);
            }
            if target.generation == self.from_generation {
                if target.version <= self.from_version {
                    return Err(ScopeError::StaleTransition);
                }
            } else if target.generation <= self.from_generation
                || !matches!(
                    self.kind,
                    ScopeTransitionKind::GenerationReset | ScopeTransitionKind::FullBootstrap
                )
            {
                return Err(ScopeError::InvalidTransition);
            }
        }
        let scope = current.scope.scope_id;
        if self.additions.iter().any(|item| item.scope_id != scope)
            || self.removals.iter().any(|item| item.scope_id != scope)
        {
            return Err(ScopeError::InvalidTransition);
        }
        Ok(())
    }
}

/// Result of one idempotent transition application.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeTransitionOutcome {
    pub transition_id: ScopeTransitionId,
    pub applied: bool,
    pub physical_removals: BTreeSet<EntityRef>,
    pub quarantined_operations: BTreeSet<OperationId>,
}

/// Serializable reference model used by adapters and compliance tests.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalScopeState {
    subscriptions: BTreeMap<SubscriptionId, Subscription>,
    memberships: BTreeMap<(ProjectionVersion, EntityRef), BTreeSet<SyncScopeId>>,
    applied_transitions: BTreeSet<ScopeTransitionId>,
    quarantined_operations: BTreeMap<OperationId, PendingIntentDisposition>,
}

impl LocalScopeState {
    /// Installs a newly server-resolved subscription without activating data.
    ///
    /// # Errors
    ///
    /// Returns an error when either identity is already present.
    pub fn install(&mut self, subscription: Subscription) -> Result<(), ScopeError> {
        if self
            .subscriptions
            .contains_key(&subscription.subscription_id)
            || self
                .subscriptions
                .values()
                .any(|known| known.scope.scope_id == subscription.scope.scope_id)
        {
            return Err(ScopeError::DuplicateSubscription);
        }
        self.subscriptions
            .insert(subscription.subscription_id, subscription);
        Ok(())
    }

    #[must_use]
    pub fn subscription(&self, id: SubscriptionId) -> Option<&Subscription> {
        self.subscriptions.get(&id)
    }

    #[must_use]
    pub fn active_scopes(
        &self,
        projection: ProjectionVersion,
        entity: EntityRef,
    ) -> BTreeSet<SyncScopeId> {
        self.memberships
            .get(&(projection, entity))
            .cloned()
            .unwrap_or_default()
    }

    #[must_use]
    pub fn pending_disposition(&self, operation: OperationId) -> Option<PendingIntentDisposition> {
        self.quarantined_operations.get(&operation).copied()
    }

    #[must_use]
    pub fn active_subscription_count(&self) -> usize {
        self.subscriptions
            .values()
            .filter(|item| item.state == SubscriptionState::Active)
            .count()
    }

    #[must_use]
    pub fn pending_transition_count(&self) -> usize {
        self.subscriptions
            .values()
            .filter(|item| item.pending_transition.is_some())
            .count()
    }

    #[must_use]
    pub fn membership_reference_count(&self) -> usize {
        self.memberships.values().map(BTreeSet::len).sum()
    }

    #[must_use]
    pub fn quarantined_operation_count(&self) -> usize {
        self.quarantined_operations.len()
    }

    /// Applies a transition atomically to a cloned model, swapping only after all checks pass.
    ///
    /// # Errors
    ///
    /// Returns a typed error without changing state when the transition is invalid or stale.
    pub fn apply(
        &mut self,
        transition: &ScopeTransition,
    ) -> Result<ScopeTransitionOutcome, ScopeError> {
        if self.applied_transitions.contains(&transition.transition_id) {
            return Ok(ScopeTransitionOutcome {
                transition_id: transition.transition_id,
                applied: false,
                physical_removals: BTreeSet::new(),
                quarantined_operations: BTreeSet::new(),
            });
        }
        let mut next = self.clone();
        let outcome = next.apply_checked(transition)?;
        *self = next;
        Ok(outcome)
    }

    #[allow(clippy::too_many_lines)]
    fn apply_checked(
        &mut self,
        transition: &ScopeTransition,
    ) -> Result<ScopeTransitionOutcome, ScopeError> {
        let current = self
            .subscriptions
            .get(&transition.subscription_id)
            .cloned()
            .ok_or(ScopeError::UnknownSubscription)?;
        transition.validate(&current)?;

        if !transition.staging_complete
            && matches!(
                transition.kind,
                ScopeTransitionKind::Expansion { .. }
                    | ScopeTransitionKind::FullBootstrap
                    | ScopeTransitionKind::GenerationReset
            )
        {
            let subscription = self
                .subscriptions
                .get_mut(&transition.subscription_id)
                .ok_or(ScopeError::UnknownSubscription)?;
            subscription.state = if matches!(transition.kind, ScopeTransitionKind::Expansion { .. })
            {
                SubscriptionState::Expanding
            } else {
                SubscriptionState::Bootstrapping
            };
            subscription.pending_transition = Some(transition.transition_id);
            return Ok(ScopeTransitionOutcome {
                transition_id: transition.transition_id,
                applied: false,
                physical_removals: BTreeSet::new(),
                quarantined_operations: BTreeSet::new(),
            });
        }

        let mut physical_removals = BTreeSet::new();
        let disposition = match transition.kind {
            ScopeTransitionKind::Revocation => PendingIntentDisposition::ScopeRevoked,
            ScopeTransitionKind::Contraction { .. } => PendingIntentDisposition::ScopeRemoved,
            _ => PendingIntentDisposition::AuthorizationLost,
        };
        let scope_id = current.scope.scope_id;

        if matches!(
            transition.kind,
            ScopeTransitionKind::FullBootstrap
                | ScopeTransitionKind::GenerationReset
                | ScopeTransitionKind::Revocation
        ) {
            let keys = self
                .memberships
                .iter()
                .filter(|(_, scopes)| scopes.contains(&scope_id))
                .map(|(key, _)| *key)
                .collect::<Vec<_>>();
            for key in keys {
                self.remove_reference(key.0, key.1, scope_id, &mut physical_removals);
            }
        }
        for removal in &transition.removals {
            self.remove_reference(
                removal.projection,
                removal.entity,
                scope_id,
                &mut physical_removals,
            );
        }
        for addition in &transition.additions {
            self.memberships
                .entry((addition.projection, addition.entity))
                .or_default()
                .insert(scope_id);
        }
        let mut quarantined_operations = BTreeSet::new();
        for operation in &transition.affected_pending_operations {
            self.quarantined_operations.insert(*operation, disposition);
            quarantined_operations.insert(*operation);
        }

        let subscription = self
            .subscriptions
            .get_mut(&transition.subscription_id)
            .ok_or(ScopeError::UnknownSubscription)?;
        match transition.kind {
            ScopeTransitionKind::Revocation => {
                subscription.state = SubscriptionState::Revoked;
                subscription.cursor = None;
            }
            ScopeTransitionKind::Suspension => subscription.state = SubscriptionState::Suspended,
            _ => {
                subscription.scope = transition
                    .target
                    .clone()
                    .ok_or(ScopeError::InvalidTransition)?;
                subscription.cursor = transition.boundary;
                subscription.state = SubscriptionState::Active;
            }
        }
        subscription.pending_transition = None;
        self.applied_transitions.insert(transition.transition_id);
        Ok(ScopeTransitionOutcome {
            transition_id: transition.transition_id,
            applied: true,
            physical_removals,
            quarantined_operations,
        })
    }

    fn remove_reference(
        &mut self,
        projection: ProjectionVersion,
        entity: EntityRef,
        scope: SyncScopeId,
        physical_removals: &mut BTreeSet<EntityRef>,
    ) {
        let key = (projection, entity);
        if let Some(scopes) = self.memberships.get_mut(&key) {
            scopes.remove(&scope);
            if scopes.is_empty() {
                self.memberships.remove(&key);
                physical_removals.insert(entity);
            }
        }
    }
}

/// Scope semantic failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ScopeError {
    #[error("zero is reserved for scope identities and versions")]
    ZeroIdentity,
    #[error("scope version space is exhausted")]
    VersionExhausted,
    #[error("scope partition is empty, unbounded, or uses a reserved kind")]
    InvalidPartition,
    #[error("scope request has too many parameters")]
    TooManyParameters,
    #[error("resolved scope tenant differs from authenticated tenant")]
    TenantMismatch,
    #[error("resolved scope descriptor is inconsistent")]
    InvalidDescriptor,
    #[error("scope definition {0:?} is not registered")]
    UnknownDefinition(ScopeDefinitionId),
    #[error("scope definition {0:?} is already registered")]
    DuplicateDefinition(ScopeDefinitionId),
    #[error("subscription identity or scope is already installed")]
    DuplicateSubscription,
    #[error("subscription is not installed")]
    UnknownSubscription,
    #[error("scope transition does not match current version or generation")]
    StaleTransition,
    #[error("scope transition mixes identities or violates activation rules")]
    InvalidTransition,
    #[error("scope is not authorized for this principal")]
    Forbidden,
    #[error("scope cursor does not match the current authorized binding")]
    CursorMismatch,
    #[error("scope-filtered journal input is unordered or exceeds its evaluated watermark")]
    UnorderedJournal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_types::{EntityId, EntityType};
    use proptest::prelude::*;

    struct TenantResolver(ScopeDefinitionId);

    #[async_trait]
    impl ScopeResolver for TenantResolver {
        fn definition_id(&self) -> ScopeDefinitionId {
            self.0
        }

        async fn resolve(
            &self,
            principal: ScopePrincipal,
            _request: &ScopeRequest,
        ) -> Result<ResolvedScope, ScopeError> {
            Ok(ResolvedScope {
                scope_id: SyncScopeId::new(),
                version: ScopeVersion::INITIAL,
                generation: ScopeGeneration::INITIAL,
                descriptor: ScopeDescriptor {
                    tenant_id: principal.tenant_id,
                    definition: self.0,
                    partitions: BTreeSet::new(),
                    policy_version: 1,
                    projection: ProjectionVersion(1),
                },
            })
        }
    }

    fn definition() -> ScopeDefinitionId {
        ScopeDefinitionId::new(1).unwrap_or_else(|error| panic!("{error}"))
    }

    fn entity() -> EntityRef {
        EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
            entity_id: EntityId::new(),
        }
    }

    fn resolved(scope_id: SyncScopeId, version: ScopeVersion) -> ResolvedScope {
        ResolvedScope {
            scope_id,
            version,
            generation: ScopeGeneration::INITIAL,
            descriptor: ScopeDescriptor {
                tenant_id: TenantId::new(),
                definition: definition(),
                partitions: BTreeSet::new(),
                policy_version: 1,
                projection: ProjectionVersion(1),
            },
        }
    }

    fn subscription(scope: ResolvedScope) -> Subscription {
        Subscription {
            subscription_id: SubscriptionId::new(),
            scope,
            state: SubscriptionState::Active,
            cursor: None,
            pending_transition: None,
        }
    }

    #[test]
    fn cursor_requires_exact_identity_version_generation_and_retention() {
        let scope = resolved(SyncScopeId::new(), ScopeVersion::INITIAL);
        let cursor = ScopeCursor {
            scope_id: scope.scope_id,
            version: scope.version,
            generation: scope.generation,
            sequence: Sequence(9),
        };
        assert_eq!(cursor.validate(&scope, Sequence(8)), CursorValidity::Valid);
        assert_eq!(
            cursor.validate(&scope, Sequence(10)),
            CursorValidity::ResyncRequired
        );
        let newer = resolved(
            scope.scope_id,
            scope
                .version
                .next()
                .unwrap_or_else(|error| panic!("{error}")),
        );
        assert_eq!(
            cursor.validate(&newer, Sequence(0)),
            CursorValidity::ScopeChanged
        );
    }

    #[tokio::test]
    async fn registry_uses_authenticated_tenant_and_rejects_duplicate_policy() {
        let mut registry = ScopeRegistry::default();
        registry
            .register(Arc::new(TenantResolver(definition())))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            registry.register(Arc::new(TenantResolver(definition()))),
            Err(ScopeError::DuplicateDefinition(definition()))
        );
        let principal = ScopePrincipal {
            tenant_id: TenantId::new(),
            actor_id: ActorId::new(),
            device_id: DeviceId::new(),
        };
        let resolved = registry
            .resolve(
                principal,
                &ScopeRequest {
                    definition: definition(),
                    parameters: Vec::new(),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(resolved.descriptor.tenant_id, principal.tenant_id);
    }

    #[test]
    fn filtered_cursor_advances_across_excluded_entries() {
        let scope = resolved(SyncScopeId::new(), ScopeVersion::INITIAL);
        let page = project_filtered_page(
            &scope,
            None,
            Sequence(3),
            false,
            [
                EvaluatedJournalEntry {
                    sequence: Sequence(1),
                    value: 1_u8,
                },
                EvaluatedJournalEntry {
                    sequence: Sequence(2),
                    value: 2_u8,
                },
                EvaluatedJournalEntry {
                    sequence: Sequence(3),
                    value: 3_u8,
                },
            ],
            |_scope, value| (value != 2).then_some(value),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(page.projected, vec![1, 3]);
        assert_eq!(page.next_cursor.sequence, Sequence(3));
    }

    #[test]
    fn contraction_preserves_an_entity_referenced_by_another_scope() {
        let shared = entity();
        let first = resolved(SyncScopeId::new(), ScopeVersion::INITIAL);
        let second = resolved(SyncScopeId::new(), ScopeVersion::INITIAL);
        let first_subscription = subscription(first.clone());
        let second_subscription = subscription(second.clone());
        let mut state = LocalScopeState::default();
        state
            .install(first_subscription.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        state
            .install(second_subscription)
            .unwrap_or_else(|error| panic!("{error}"));
        state.memberships.insert(
            (ProjectionVersion(1), shared),
            BTreeSet::from([first.scope_id, second.scope_id]),
        );
        let next_version = first
            .version
            .next()
            .unwrap_or_else(|error| panic!("{error}"));
        let mut target = first.clone();
        target.version = next_version;
        let transition = ScopeTransition {
            transition_id: ScopeTransitionId::new(),
            subscription_id: first_subscription.subscription_id,
            from_version: first.version,
            from_generation: first.generation,
            target: Some(target),
            boundary: Some(ScopeCursor {
                scope_id: first.scope_id,
                version: next_version,
                generation: first.generation,
                sequence: Sequence(10),
            }),
            kind: ScopeTransitionKind::Contraction {
                removed_partitions: BTreeSet::new(),
                retention: LocalRetentionPolicy::RemoveImmediately,
            },
            additions: Vec::new(),
            removals: vec![ScopeRemoval {
                scope_id: first.scope_id,
                projection: ProjectionVersion(1),
                entity: shared,
                reason: ScopeRemovalReason::MembershipChanged,
            }],
            affected_pending_operations: Vec::new(),
            staging_complete: true,
        };
        let outcome = state
            .apply(&transition)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(outcome.physical_removals.is_empty());
        assert_eq!(
            state.active_scopes(ProjectionVersion(1), shared),
            BTreeSet::from([second.scope_id])
        );
    }

    #[test]
    fn incomplete_expansion_never_activates_membership_and_retry_is_idempotent() {
        let original = resolved(SyncScopeId::new(), ScopeVersion::INITIAL);
        let subscription = subscription(original.clone());
        let mut state = LocalScopeState::default();
        state
            .install(subscription.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        let added = entity();
        let target_version = original
            .version
            .next()
            .unwrap_or_else(|error| panic!("{error}"));
        let mut target = original.clone();
        target.version = target_version;
        let mut transition = ScopeTransition {
            transition_id: ScopeTransitionId::new(),
            subscription_id: subscription.subscription_id,
            from_version: original.version,
            from_generation: original.generation,
            target: Some(target),
            boundary: Some(ScopeCursor {
                scope_id: original.scope_id,
                version: target_version,
                generation: original.generation,
                sequence: Sequence(20),
            }),
            kind: ScopeTransitionKind::Expansion {
                added_partitions: BTreeSet::new(),
            },
            additions: vec![MembershipRecord {
                scope_id: original.scope_id,
                projection: ProjectionVersion(1),
                entity: added,
                membership_version: target_version,
            }],
            removals: Vec::new(),
            affected_pending_operations: Vec::new(),
            staging_complete: false,
        };
        assert!(
            !state
                .apply(&transition)
                .unwrap_or_else(|error| panic!("{error}"))
                .applied
        );
        assert!(state.active_scopes(ProjectionVersion(1), added).is_empty());
        transition.staging_complete = true;
        assert!(
            state
                .apply(&transition)
                .unwrap_or_else(|error| panic!("{error}"))
                .applied
        );
        assert!(
            !state
                .apply(&transition)
                .unwrap_or_else(|error| panic!("{error}"))
                .applied
        );
    }

    #[test]
    fn revocation_deactivates_data_and_quarantines_pending_intent() {
        let scope = resolved(SyncScopeId::new(), ScopeVersion::INITIAL);
        let subscription = subscription(scope.clone());
        let operation = OperationId::new();
        let member = entity();
        let mut state = LocalScopeState::default();
        state
            .install(subscription.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        state.memberships.insert(
            (ProjectionVersion(1), member),
            BTreeSet::from([scope.scope_id]),
        );
        let transition = ScopeTransition {
            transition_id: ScopeTransitionId::new(),
            subscription_id: subscription.subscription_id,
            from_version: scope.version,
            from_generation: scope.generation,
            target: None,
            boundary: None,
            kind: ScopeTransitionKind::Revocation,
            additions: Vec::new(),
            removals: Vec::new(),
            affected_pending_operations: vec![operation],
            staging_complete: true,
        };
        let outcome = state
            .apply(&transition)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(outcome.physical_removals, BTreeSet::from([member]));
        assert_eq!(
            state.pending_disposition(operation),
            Some(PendingIntentDisposition::ScopeRevoked)
        );
        assert_eq!(
            state
                .subscription(subscription.subscription_id)
                .map(|item| item.state),
            Some(SubscriptionState::Revoked)
        );
    }

    proptest! {
        #[test]
        fn scope_versions_never_wrap(start in 1_u64..u64::MAX) {
            let version = ScopeVersion::new(start).unwrap_or_else(|error| panic!("{error}"));
            let next = version.next();
            if start == u64::MAX {
                prop_assert_eq!(next, Err(ScopeError::VersionExhausted));
            } else {
                prop_assert_eq!(next.map(ScopeVersion::get), Ok(start + 1));
            }
        }
    }

    #[test]
    fn state_round_trips_without_losing_transition_guards() {
        let scope = resolved(SyncScopeId::new(), ScopeVersion::INITIAL);
        let mut state = LocalScopeState::default();
        state
            .install(subscription(scope))
            .unwrap_or_else(|error| panic!("{error}"));
        let encoded = postcard::to_stdvec(&state).unwrap_or_else(|error| panic!("{error}"));
        let decoded: LocalScopeState =
            postcard::from_bytes(&encoded).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(decoded, state);
    }
}
