//! Coherent, validated consistency profiles for aggregates and operations.
//!
//! Profiles bind versioning, conflict, rebase, compaction, deletion, ordering, audit, snapshot,
//! scope, retry, and adapter requirements into one semantic contract. They are not database
//! isolation levels, scheduler classes, authorization roles, or UI modes.

use aequora_protocol::{ConflictPolicy, OperationKind};
use aequora_queue::{CompactionPolicy, RebasePolicy};
use aequora_store::{DurabilityMode, TransactionCapabilities, TransactionGuarantees};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Version of the profile-manifest encoding and compatibility rules.
pub const PROFILE_MANIFEST_FORMAT_VERSION: u16 = 1;

/// Stable non-zero application aggregate identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AggregateProfileId(u16);

impl AggregateProfileId {
    /// Creates an identifier for compile-time declarations.
    ///
    /// # Panics
    ///
    /// Panics when `value` is zero. Derive macros reject zero before emitting this call.
    #[must_use]
    pub const fn from_static(value: u16) -> Self {
        assert!(value != 0, "aggregate profile identity must be non-zero");
        Self(value)
    }

    /// Creates a stable non-zero identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::ZeroIdentity`] for zero.
    pub const fn new(value: u16) -> Result<Self, ProfileError> {
        if value == 0 {
            Err(ProfileError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Semantic version of one aggregate or operation profile declaration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProfileVersion(u16);

impl ProfileVersion {
    pub const V1: Self = Self(1);

    /// Creates a non-zero profile version.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::ZeroVersion`] for zero.
    pub const fn new(value: u16) -> Result<Self, ProfileError> {
        if value == 0 {
            Err(ProfileError::ZeroVersion)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Recommended stable built-in aggregate profiles.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ConsistencyProfileKind {
    ImmutableAppendOnly,
    OptimisticVersioned,
    Commutative,
    LastWriterWins,
    ManualConflict,
    StrongAggregate,
    ServerOnly,
    DeviceLocal,
    DerivedProjection,
}

/// Version boundary used by the aggregate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum VersioningPolicy {
    None,
    EntityVersion,
    AggregateVersion,
    StreamSequence,
    ServerSequenceOnly,
}

/// Required semantic ordering boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum OrderingPolicy {
    Independent,
    PerEntity,
    PerAggregate,
    DependencyOnly,
    GlobalWithinScope,
}

/// Meaning of removing data from the aggregate model.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeletePolicy {
    Forbidden,
    Tombstone,
    CompensatingEvent,
    ScopeEvictionOnly,
    RebuildableDrop,
}

/// Required audit treatment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AuditPolicy {
    Optional,
    Recommended,
    Required,
    Derived,
}

/// Representation and restoration behavior in snapshots.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SnapshotPolicy {
    Included,
    EventRebuilt,
    DerivedRebuildable,
    Excluded,
}

/// Authority and scope behavior for an aggregate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScopePolicy {
    Replicated,
    ServerOnly,
    DeviceLocal,
    Derived,
}

/// Retry meaning selected by authority/origin semantics, not by scheduler timing.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ProfileRetryPolicy {
    /// Network retries retain the exact operation identity and semantic envelope.
    StableOperationIdentity,
    /// Only the server owns retry and deduplication for this aggregate.
    ServerManaged,
    /// The aggregate never enters remote synchronization.
    NotReplicated,
    /// Projection recovery rebuilds from authoritative source state.
    RebuildFromAuthority,
}

/// Domain risk tightens otherwise valid semantic choices.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DomainRisk {
    General,
    Finance,
    Workflow,
    SecuritySensitive,
}

/// Complete aggregate-level semantic contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConsistencyProfile {
    pub kind: ConsistencyProfileKind,
    pub versioning: VersioningPolicy,
    pub conflict: ConflictPolicy,
    pub delete: DeletePolicy,
    pub ordering: OrderingPolicy,
    pub audit: AuditPolicy,
    pub snapshot: SnapshotPolicy,
    pub scope: ScopePolicy,
    pub retry: ProfileRetryPolicy,
    pub requirements: ProfileRequirements,
    pub custom: bool,
}

impl ConsistencyProfile {
    /// Returns the stable v1 contract for a built-in kind.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn built_in(kind: ConsistencyProfileKind) -> Self {
        let (versioning, conflict, delete, ordering, audit, snapshot, scope, features) = match kind
        {
            ConsistencyProfileKind::ImmutableAppendOnly => (
                VersioningPolicy::StreamSequence,
                ConflictPolicy::Reject,
                DeletePolicy::CompensatingEvent,
                OrderingPolicy::DependencyOnly,
                AuditPolicy::Required,
                SnapshotPolicy::EventRebuilt,
                ScopePolicy::Replicated,
                [
                    ProfileCapability::DurableAppend,
                    ProfileCapability::IdempotentCommit,
                ]
                .as_slice(),
            ),
            ConsistencyProfileKind::OptimisticVersioned => (
                VersioningPolicy::EntityVersion,
                ConflictPolicy::Reject,
                DeletePolicy::Tombstone,
                OrderingPolicy::PerEntity,
                AuditPolicy::Recommended,
                SnapshotPolicy::Included,
                ScopePolicy::Replicated,
                [
                    ProfileCapability::CompareAndSwap,
                    ProfileCapability::IdempotentCommit,
                ]
                .as_slice(),
            ),
            ConsistencyProfileKind::Commutative => (
                VersioningPolicy::EntityVersion,
                ConflictPolicy::CommutativeOperation,
                DeletePolicy::Tombstone,
                OrderingPolicy::Independent,
                AuditPolicy::Recommended,
                SnapshotPolicy::Included,
                ScopePolicy::Replicated,
                [ProfileCapability::IdempotentCommit].as_slice(),
            ),
            ConsistencyProfileKind::LastWriterWins => (
                VersioningPolicy::EntityVersion,
                ConflictPolicy::LastWriterWins,
                DeletePolicy::Tombstone,
                OrderingPolicy::PerEntity,
                AuditPolicy::Optional,
                SnapshotPolicy::Included,
                ScopePolicy::Replicated,
                [
                    ProfileCapability::AuthoritativeClock,
                    ProfileCapability::IdempotentCommit,
                ]
                .as_slice(),
            ),
            ConsistencyProfileKind::ManualConflict => (
                VersioningPolicy::EntityVersion,
                ConflictPolicy::ManualResolution,
                DeletePolicy::Tombstone,
                OrderingPolicy::PerEntity,
                AuditPolicy::Required,
                SnapshotPolicy::Included,
                ScopePolicy::Replicated,
                [
                    ProfileCapability::CompareAndSwap,
                    ProfileCapability::DurableConflictInbox,
                    ProfileCapability::IdempotentCommit,
                ]
                .as_slice(),
            ),
            ConsistencyProfileKind::StrongAggregate => (
                VersioningPolicy::AggregateVersion,
                ConflictPolicy::Reject,
                DeletePolicy::Forbidden,
                OrderingPolicy::PerAggregate,
                AuditPolicy::Required,
                SnapshotPolicy::Included,
                ScopePolicy::Replicated,
                [
                    ProfileCapability::AtomicAggregate,
                    ProfileCapability::CompareAndSwap,
                    ProfileCapability::IdempotentCommit,
                ]
                .as_slice(),
            ),
            ConsistencyProfileKind::ServerOnly => (
                VersioningPolicy::ServerSequenceOnly,
                ConflictPolicy::Reject,
                DeletePolicy::Forbidden,
                OrderingPolicy::PerEntity,
                AuditPolicy::Recommended,
                SnapshotPolicy::Included,
                ScopePolicy::ServerOnly,
                [ProfileCapability::DurableAppend].as_slice(),
            ),
            ConsistencyProfileKind::DeviceLocal => (
                VersioningPolicy::None,
                ConflictPolicy::Reject,
                DeletePolicy::Forbidden,
                OrderingPolicy::Independent,
                AuditPolicy::Optional,
                SnapshotPolicy::Excluded,
                ScopePolicy::DeviceLocal,
                [].as_slice(),
            ),
            ConsistencyProfileKind::DerivedProjection => (
                VersioningPolicy::ServerSequenceOnly,
                ConflictPolicy::Reject,
                DeletePolicy::RebuildableDrop,
                OrderingPolicy::DependencyOnly,
                AuditPolicy::Derived,
                SnapshotPolicy::DerivedRebuildable,
                ScopePolicy::Derived,
                [ProfileCapability::ConsistentSnapshot].as_slice(),
            ),
        };
        let retry = match kind {
            ConsistencyProfileKind::ServerOnly => ProfileRetryPolicy::ServerManaged,
            ConsistencyProfileKind::DeviceLocal => ProfileRetryPolicy::NotReplicated,
            ConsistencyProfileKind::DerivedProjection => ProfileRetryPolicy::RebuildFromAuthority,
            _ => ProfileRetryPolicy::StableOperationIdentity,
        };
        Self {
            kind,
            versioning,
            conflict,
            delete,
            ordering,
            audit,
            snapshot,
            scope,
            retry,
            requirements: ProfileRequirements {
                capabilities: features.iter().copied().collect(),
            },
            custom: false,
        }
    }

    /// Validates internal coherence independent of an operation or adapter.
    ///
    /// # Errors
    ///
    /// Rejects unsafe built-in overrides and authority/snapshot contradictions.
    pub fn validate(&self) -> Result<(), ProfileError> {
        if !self.custom && self != &Self::built_in(self.kind) {
            return Err(ProfileError::BuiltInOverrideRequiresCustom);
        }
        if self.scope == ScopePolicy::DeviceLocal
            && (self.snapshot != SnapshotPolicy::Excluded
                || self.versioning != VersioningPolicy::None
                || self.retry != ProfileRetryPolicy::NotReplicated)
        {
            return Err(ProfileError::DeviceLocalReplicated);
        }
        if self.scope == ScopePolicy::Derived
            && (self.audit != AuditPolicy::Derived
                || self.delete != DeletePolicy::RebuildableDrop
                || self.retry != ProfileRetryPolicy::RebuildFromAuthority)
        {
            return Err(ProfileError::DerivedClaimsAuthority);
        }
        if self.kind == ConsistencyProfileKind::ImmutableAppendOnly
            && !matches!(
                self.delete,
                DeletePolicy::Forbidden | DeletePolicy::CompensatingEvent
            )
        {
            return Err(ProfileError::ImmutableMutation);
        }
        if self.kind == ConsistencyProfileKind::StrongAggregate
            && self.conflict == ConflictPolicy::LastWriterWins
        {
            return Err(ProfileError::StrongAggregateLastWriterWins);
        }
        if matches!(
            (self.scope, self.retry),
            (ScopePolicy::Replicated, policy)
                if policy != ProfileRetryPolicy::StableOperationIdentity
        ) || matches!(
            (self.scope, self.retry),
            (ScopePolicy::ServerOnly, policy) if policy != ProfileRetryPolicy::ServerManaged
        ) {
            return Err(ProfileError::RetryScopeMismatch);
        }
        let required = [
            (
                self.retry == ProfileRetryPolicy::StableOperationIdentity,
                ProfileCapability::IdempotentCommit,
            ),
            (
                self.conflict == ConflictPolicy::LastWriterWins,
                ProfileCapability::AuthoritativeClock,
            ),
            (
                self.conflict == ConflictPolicy::ManualResolution,
                ProfileCapability::DurableConflictInbox,
            ),
            (
                self.versioning == VersioningPolicy::AggregateVersion,
                ProfileCapability::AtomicAggregate,
            ),
            (
                self.versioning == VersioningPolicy::AggregateVersion,
                ProfileCapability::CompareAndSwap,
            ),
            (
                self.snapshot == SnapshotPolicy::DerivedRebuildable,
                ProfileCapability::ConsistentSnapshot,
            ),
            (
                self.kind == ConsistencyProfileKind::ImmutableAppendOnly,
                ProfileCapability::DurableAppend,
            ),
        ];
        if let Some((_, capability)) = required.iter().find(|(needed, capability)| {
            *needed && !self.requirements.capabilities.contains(capability)
        }) {
            return Err(ProfileError::IncompleteRequirements(*capability));
        }
        Ok(())
    }
}

/// Explicit adapter feature required by one or more profiles.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ProfileCapability {
    DurableAppend,
    IdempotentCommit,
    CompareAndSwap,
    AtomicAggregate,
    DurableConflictInbox,
    ConsistentSnapshot,
    AuthoritativeClock,
}

/// Capabilities required by a profile.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileRequirements {
    pub capabilities: BTreeSet<ProfileCapability>,
}

/// Deployment declaration checked before a registry becomes active.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AdapterProfileCapabilities {
    pub capabilities: BTreeSet<ProfileCapability>,
}

impl AdapterProfileCapabilities {
    /// Derives semantic capabilities already proven by the shared transaction declaration.
    #[must_use]
    pub fn from_transactions(transactions: TransactionCapabilities) -> Self {
        let mut capabilities = BTreeSet::new();
        if transactions.durability == DurabilityMode::Durable {
            capabilities.insert(ProfileCapability::DurableAppend);
        }
        if transactions
            .guarantees
            .contains(TransactionGuarantees::CONCURRENT_IDEMPOTENCY)
        {
            capabilities.insert(ProfileCapability::IdempotentCommit);
        }
        if transactions
            .guarantees
            .contains(TransactionGuarantees::AUTHORITATIVE_COMMIT)
        {
            capabilities.insert(ProfileCapability::CompareAndSwap);
            capabilities.insert(ProfileCapability::AtomicAggregate);
        }
        if transactions
            .guarantees
            .contains(TransactionGuarantees::CONSISTENT_SNAPSHOT)
        {
            capabilities.insert(ProfileCapability::ConsistentSnapshot);
        }
        Self { capabilities }
    }

    /// Adds an application/adapter capability not expressible by generic transaction flags.
    #[must_use]
    pub fn with(mut self, capability: ProfileCapability) -> Self {
        self.capabilities.insert(capability);
        self
    }

    fn satisfies(&self, requirements: &ProfileRequirements) -> bool {
        requirements.capabilities.is_subset(&self.capabilities)
    }
}

/// Coarse operation semantic class, separate from the aggregate profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum OperationSemanticClass {
    SetValue,
    PatchFields,
    AppendEvent,
    Increment,
    SetMembership,
    Transition,
    Command,
    DerivedOnly,
}

/// Where an operation is permitted to originate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum OperationOrigin {
    ClientOrServer,
    ServerOnly,
    DeviceLocal,
}

/// Operation-level semantics that inherit aggregate conflict/delete/version/snapshot policy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationSemanticProfile {
    pub operation_kind: OperationKind,
    pub name: String,
    pub aggregate_id: AggregateProfileId,
    pub class: OperationSemanticClass,
    pub compaction: CompactionPolicy,
    pub rebase: RebasePolicy,
    pub ordering: OrderingPolicy,
    pub origin: OperationOrigin,
    pub audit: AuditPolicy,
    pub version: ProfileVersion,
}

impl OperationSemanticProfile {
    /// Conservative built-in defaults for an operation class.
    #[must_use]
    pub fn conservative(
        operation_kind: OperationKind,
        name: impl Into<String>,
        aggregate_id: AggregateProfileId,
        class: OperationSemanticClass,
    ) -> Self {
        let (compaction, rebase, ordering, origin) = match class {
            OperationSemanticClass::SetValue | OperationSemanticClass::PatchFields => (
                CompactionPolicy::Never,
                RebasePolicy::Never,
                OrderingPolicy::PerEntity,
                OperationOrigin::ClientOrServer,
            ),
            OperationSemanticClass::AppendEvent | OperationSemanticClass::SetMembership => (
                CompactionPolicy::Never,
                RebasePolicy::Never,
                OrderingPolicy::DependencyOnly,
                OperationOrigin::ClientOrServer,
            ),
            OperationSemanticClass::Increment => (
                CompactionPolicy::Never,
                RebasePolicy::ReapplyIntent,
                OrderingPolicy::Independent,
                OperationOrigin::ClientOrServer,
            ),
            OperationSemanticClass::Transition | OperationSemanticClass::Command => (
                CompactionPolicy::Never,
                RebasePolicy::Never,
                OrderingPolicy::PerAggregate,
                OperationOrigin::ClientOrServer,
            ),
            OperationSemanticClass::DerivedOnly => (
                CompactionPolicy::Never,
                RebasePolicy::Never,
                OrderingPolicy::DependencyOnly,
                OperationOrigin::ServerOnly,
            ),
        };
        Self {
            operation_kind,
            name: name.into(),
            aggregate_id,
            class,
            compaction,
            rebase,
            ordering,
            origin,
            audit: AuditPolicy::Recommended,
            version: ProfileVersion::V1,
        }
    }
}

/// Registered aggregate contract and its domain-risk classification.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AggregateProfile {
    pub aggregate_id: AggregateProfileId,
    pub name: String,
    pub profile: ConsistencyProfile,
    pub version: ProfileVersion,
    pub risk: DomainRisk,
}

impl AggregateProfile {
    /// Validates the name, built-in/custom contract, and risk-specific restrictions.
    ///
    /// # Errors
    ///
    /// Rejects blank/oversized names and unsafe finance/workflow/security semantics.
    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.aggregate_id.get() == 0 {
            return Err(ProfileError::ZeroIdentity);
        }
        if self.version.get() == 0 {
            return Err(ProfileError::ZeroVersion);
        }
        validate_name(&self.name)?;
        self.profile.validate()?;
        match self.risk {
            DomainRisk::General => {}
            DomainRisk::Finance => {
                if !matches!(
                    self.profile.kind,
                    ConsistencyProfileKind::ImmutableAppendOnly
                        | ConsistencyProfileKind::StrongAggregate
                ) || self.profile.audit != AuditPolicy::Required
                    || self.profile.conflict == ConflictPolicy::LastWriterWins
                {
                    return Err(ProfileError::UnsafeFinanceProfile);
                }
            }
            DomainRisk::Workflow | DomainRisk::SecuritySensitive => {
                if self.profile.audit != AuditPolicy::Required
                    || self.profile.conflict == ConflictPolicy::LastWriterWins
                {
                    return Err(ProfileError::UnsafeSensitiveProfile);
                }
            }
        }
        Ok(())
    }
}

/// Advanced validated overrides. Setting any field makes the profile custom.
#[derive(Clone, Debug, Default)]
pub struct ProfileOverrides {
    pub versioning: Option<VersioningPolicy>,
    pub conflict: Option<ConflictPolicy>,
    pub delete: Option<DeletePolicy>,
    pub ordering: Option<OrderingPolicy>,
    pub audit: Option<AuditPolicy>,
    pub snapshot: Option<SnapshotPolicy>,
    pub scope: Option<ScopePolicy>,
    pub retry: Option<ProfileRetryPolicy>,
    pub requirements: Option<ProfileRequirements>,
}

/// Builder that requires explicit advanced opt-in before overriding a built-in profile.
#[derive(Clone, Debug)]
pub struct CustomProfileBuilder {
    base: ConsistencyProfileKind,
    overrides: ProfileOverrides,
    advanced_opt_in: bool,
}

impl CustomProfileBuilder {
    #[must_use]
    pub const fn new(base: ConsistencyProfileKind) -> Self {
        Self {
            base,
            overrides: ProfileOverrides {
                versioning: None,
                conflict: None,
                delete: None,
                ordering: None,
                audit: None,
                snapshot: None,
                scope: None,
                retry: None,
                requirements: None,
            },
            advanced_opt_in: false,
        }
    }

    #[must_use]
    pub const fn advanced_opt_in(mut self) -> Self {
        self.advanced_opt_in = true;
        self
    }

    #[must_use]
    pub fn overrides(mut self, overrides: ProfileOverrides) -> Self {
        self.overrides = overrides;
        self
    }

    /// Applies overrides and validates the resulting coherent profile.
    ///
    /// # Errors
    ///
    /// Rejects overrides without opt-in and internally invalid results.
    pub fn build(self) -> Result<ConsistencyProfile, ProfileError> {
        let has_override = self.overrides.versioning.is_some()
            || self.overrides.conflict.is_some()
            || self.overrides.delete.is_some()
            || self.overrides.ordering.is_some()
            || self.overrides.audit.is_some()
            || self.overrides.snapshot.is_some()
            || self.overrides.scope.is_some()
            || self.overrides.retry.is_some()
            || self.overrides.requirements.is_some();
        if has_override && !self.advanced_opt_in {
            return Err(ProfileError::CustomProfileRequiresOptIn);
        }
        let mut profile = ConsistencyProfile::built_in(self.base);
        if let Some(value) = self.overrides.versioning {
            profile.versioning = value;
        }
        if let Some(value) = self.overrides.conflict {
            profile.conflict = value;
        }
        if let Some(value) = self.overrides.delete {
            profile.delete = value;
        }
        if let Some(value) = self.overrides.ordering {
            profile.ordering = value;
        }
        if let Some(value) = self.overrides.audit {
            profile.audit = value;
        }
        if let Some(value) = self.overrides.snapshot {
            profile.snapshot = value;
        }
        if let Some(value) = self.overrides.scope {
            profile.scope = value;
        }
        if let Some(value) = self.overrides.retry {
            profile.retry = value;
        }
        if let Some(value) = self.overrides.requirements {
            profile.requirements = value;
        }
        profile.custom = has_override;
        profile.validate()?;
        Ok(profile)
    }
}

/// Builder ergonomics for one aggregate registration.
#[derive(Clone, Debug)]
pub struct AggregateProfileBuilder {
    aggregate: AggregateProfile,
}

impl AggregateProfileBuilder {
    #[must_use]
    pub fn new(
        aggregate_id: AggregateProfileId,
        name: impl Into<String>,
        kind: ConsistencyProfileKind,
    ) -> Self {
        Self {
            aggregate: AggregateProfile {
                aggregate_id,
                name: name.into(),
                profile: ConsistencyProfile::built_in(kind),
                version: ProfileVersion::V1,
                risk: DomainRisk::General,
            },
        }
    }

    #[must_use]
    pub const fn risk(mut self, risk: DomainRisk) -> Self {
        self.aggregate.risk = risk;
        self
    }

    #[must_use]
    pub const fn version(mut self, version: ProfileVersion) -> Self {
        self.aggregate.version = version;
        self
    }

    #[must_use]
    pub fn custom_profile(mut self, profile: ConsistencyProfile) -> Self {
        self.aggregate.profile = profile;
        self
    }

    /// Validates and returns the declaration.
    ///
    /// # Errors
    ///
    /// Returns the aggregate profile validation error.
    pub fn build(self) -> Result<AggregateProfile, ProfileError> {
        self.aggregate.validate()?;
        Ok(self.aggregate)
    }
}

/// Builder ergonomics for one operation registration.
#[derive(Clone, Debug)]
pub struct OperationProfileBuilder {
    operation: OperationSemanticProfile,
}

impl OperationProfileBuilder {
    #[must_use]
    pub fn new(
        operation_kind: OperationKind,
        name: impl Into<String>,
        aggregate_id: AggregateProfileId,
        class: OperationSemanticClass,
    ) -> Self {
        Self {
            operation: OperationSemanticProfile::conservative(
                operation_kind,
                name,
                aggregate_id,
                class,
            ),
        }
    }

    #[must_use]
    pub const fn compaction(mut self, value: CompactionPolicy) -> Self {
        self.operation.compaction = value;
        self
    }

    #[must_use]
    pub const fn rebase(mut self, value: RebasePolicy) -> Self {
        self.operation.rebase = value;
        self
    }

    #[must_use]
    pub const fn ordering(mut self, value: OrderingPolicy) -> Self {
        self.operation.ordering = value;
        self
    }

    #[must_use]
    pub const fn origin(mut self, value: OperationOrigin) -> Self {
        self.operation.origin = value;
        self
    }

    #[must_use]
    pub const fn audit(mut self, value: AuditPolicy) -> Self {
        self.operation.audit = value;
        self
    }

    #[must_use]
    pub const fn version(mut self, value: ProfileVersion) -> Self {
        self.operation.version = value;
        self
    }

    #[must_use]
    pub fn build(self) -> OperationSemanticProfile {
        self.operation
    }
}

/// Fail-closed aggregate/operation semantic registry.
#[derive(Clone, Debug, Default)]
pub struct ProfileRegistry {
    aggregates: BTreeMap<AggregateProfileId, AggregateProfile>,
    operations: BTreeMap<u16, OperationSemanticProfile>,
}

impl ProfileRegistry {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            aggregates: BTreeMap::new(),
            operations: BTreeMap::new(),
        }
    }

    /// Registers one aggregate without replacing existing semantics.
    ///
    /// # Errors
    ///
    /// Rejects invalid or duplicate declarations.
    pub fn register_aggregate(&mut self, aggregate: AggregateProfile) -> Result<(), ProfileError> {
        aggregate.validate()?;
        if self.aggregates.contains_key(&aggregate.aggregate_id) {
            return Err(ProfileError::DuplicateAggregate(aggregate.aggregate_id));
        }
        self.aggregates.insert(aggregate.aggregate_id, aggregate);
        Ok(())
    }

    /// Registers one operation after validating it against its aggregate.
    ///
    /// # Errors
    ///
    /// Rejects unknown aggregates, duplicate kinds, and incompatible semantics.
    pub fn register_operation(
        &mut self,
        operation: OperationSemanticProfile,
    ) -> Result<(), ProfileError> {
        validate_name(&operation.name)?;
        if operation.version.get() == 0 {
            return Err(ProfileError::ZeroVersion);
        }
        let aggregate = self
            .aggregates
            .get(&operation.aggregate_id)
            .ok_or(ProfileError::UnknownAggregate(operation.aggregate_id))?;
        validate_operation(aggregate, &operation)?;
        if self.operations.contains_key(&operation.operation_kind.0) {
            return Err(ProfileError::DuplicateOperation(operation.operation_kind));
        }
        self.operations
            .insert(operation.operation_kind.0, operation);
        Ok(())
    }

    /// Returns an exact registered operation. Unknown kinds never acquire implicit LWW behavior.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::UnknownOperation`] for unregistered kinds.
    pub fn require_operation(
        &self,
        kind: OperationKind,
    ) -> Result<&OperationSemanticProfile, ProfileError> {
        self.operations
            .get(&kind.0)
            .ok_or(ProfileError::UnknownOperation(kind))
    }

    /// Verifies that all aggregate requirements are supported before startup.
    ///
    /// # Errors
    ///
    /// Returns the first aggregate and missing capability set.
    pub fn validate_capabilities(
        &self,
        capabilities: &AdapterProfileCapabilities,
    ) -> Result<(), ProfileError> {
        for aggregate in self.aggregates.values() {
            if !capabilities.satisfies(&aggregate.profile.requirements) {
                let missing = aggregate
                    .profile
                    .requirements
                    .capabilities
                    .difference(&capabilities.capabilities)
                    .copied()
                    .collect();
                return Err(ProfileError::MissingCapabilities {
                    aggregate_id: aggregate.aggregate_id,
                    missing,
                });
            }
        }
        Ok(())
    }

    /// Emits a sorted checksummed compatibility manifest.
    ///
    /// # Errors
    ///
    /// Returns a serialization error only when the portable format cannot be encoded.
    pub fn manifest(&self) -> Result<ProfileManifest, ProfileError> {
        ProfileManifest::build(
            self.aggregates.values().cloned().collect(),
            self.operations.values().cloned().collect(),
        )
    }
}

fn validate_operation(
    aggregate: &AggregateProfile,
    operation: &OperationSemanticProfile,
) -> Result<(), ProfileError> {
    let kind = aggregate.profile.kind;
    if operation.class == OperationSemanticClass::DerivedOnly
        && !matches!(
            kind,
            ConsistencyProfileKind::ServerOnly | ConsistencyProfileKind::DerivedProjection
        )
    {
        return Err(ProfileError::DerivedOperationOnAuthority);
    }
    if matches!(
        kind,
        ConsistencyProfileKind::ServerOnly | ConsistencyProfileKind::DerivedProjection
    ) && operation.origin != OperationOrigin::ServerOnly
    {
        return Err(ProfileError::ServerOnlyClientOperation);
    }
    if kind == ConsistencyProfileKind::DeviceLocal
        && operation.origin != OperationOrigin::DeviceLocal
    {
        return Err(ProfileError::DeviceLocalReplicated);
    }
    if kind == ConsistencyProfileKind::ImmutableAppendOnly
        && (operation.class != OperationSemanticClass::AppendEvent
            || operation.compaction != CompactionPolicy::Never
            || operation.rebase != RebasePolicy::Never)
    {
        return Err(ProfileError::ImmutableMutation);
    }
    if operation.class == OperationSemanticClass::Increment
        && kind != ConsistencyProfileKind::Commutative
    {
        return Err(ProfileError::IncrementNotCommutative);
    }
    if operation.class == OperationSemanticClass::Transition
        && (operation.compaction != CompactionPolicy::Never
            || operation.rebase != RebasePolicy::Never
            || kind == ConsistencyProfileKind::LastWriterWins)
    {
        return Err(ProfileError::UnsafeTransition);
    }
    if operation.compaction != CompactionPolicy::Never
        && matches!(
            kind,
            ConsistencyProfileKind::ImmutableAppendOnly
                | ConsistencyProfileKind::StrongAggregate
                | ConsistencyProfileKind::ServerOnly
                | ConsistencyProfileKind::DerivedProjection
        )
    {
        return Err(ProfileError::CompactionIncompatible);
    }
    if aggregate.risk != DomainRisk::General
        && (operation.audit != AuditPolicy::Required
            || operation.compaction != CompactionPolicy::Never
            || operation.rebase != RebasePolicy::Never)
    {
        return Err(ProfileError::UnsafeSensitiveOperation);
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), ProfileError> {
    if name.trim().is_empty() || name.len() > 128 {
        Err(ProfileError::InvalidName)
    } else {
        Ok(())
    }
}

/// Sorted, checksummed protocol/CI artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileManifest {
    pub format_version: u16,
    pub aggregates: Vec<AggregateProfile>,
    pub operations: Vec<OperationSemanticProfile>,
    pub digest: [u8; 32],
}

impl ProfileManifest {
    fn build(
        aggregates: Vec<AggregateProfile>,
        operations: Vec<OperationSemanticProfile>,
    ) -> Result<Self, ProfileError> {
        let mut manifest = Self {
            format_version: PROFILE_MANIFEST_FORMAT_VERSION,
            aggregates,
            operations,
            digest: [0; 32],
        };
        manifest.digest = manifest.calculate_digest()?;
        Ok(manifest)
    }

    /// Verifies format, sorted unique IDs, cross-references, declarations, and digest.
    ///
    /// # Errors
    ///
    /// Returns a typed structural or semantic failure.
    pub fn verify(&self) -> Result<(), ProfileError> {
        if self.format_version != PROFILE_MANIFEST_FORMAT_VERSION {
            return Err(ProfileError::UnsupportedManifestVersion);
        }
        if self
            .aggregates
            .windows(2)
            .any(|pair| pair[0].aggregate_id >= pair[1].aggregate_id)
            || self
                .operations
                .windows(2)
                .any(|pair| pair[0].operation_kind.0 >= pair[1].operation_kind.0)
        {
            return Err(ProfileError::ManifestOrder);
        }
        let aggregates = self
            .aggregates
            .iter()
            .map(|aggregate| (aggregate.aggregate_id, aggregate))
            .collect::<BTreeMap<_, _>>();
        for aggregate in &self.aggregates {
            aggregate.validate()?;
        }
        for operation in &self.operations {
            validate_name(&operation.name)?;
            if operation.version.get() == 0 {
                return Err(ProfileError::ZeroVersion);
            }
            let aggregate = aggregates
                .get(&operation.aggregate_id)
                .ok_or(ProfileError::UnknownAggregate(operation.aggregate_id))?;
            validate_operation(aggregate, operation)?;
        }
        if self.calculate_digest()? != self.digest {
            return Err(ProfileError::ManifestDigestMismatch);
        }
        Ok(())
    }

    /// Rejects removals and semantic changes without a strictly higher declaration version.
    ///
    /// Additive aggregates/operations are compatible.
    ///
    /// # Errors
    ///
    /// Returns a stable compatibility error for an unversioned breaking change.
    pub fn verify_compatible_successor(&self, next: &Self) -> Result<(), ProfileError> {
        self.verify()?;
        next.verify()?;
        let next_aggregates = next
            .aggregates
            .iter()
            .map(|aggregate| (aggregate.aggregate_id, aggregate))
            .collect::<BTreeMap<_, _>>();
        for previous in &self.aggregates {
            let current = next_aggregates
                .get(&previous.aggregate_id)
                .ok_or(ProfileError::AggregateRemoved(previous.aggregate_id))?;
            if previous != *current && current.version <= previous.version {
                return Err(ProfileError::AggregateChangedWithoutVersion(
                    previous.aggregate_id,
                ));
            }
        }
        let next_operations = next
            .operations
            .iter()
            .map(|operation| (operation.operation_kind.0, operation))
            .collect::<BTreeMap<_, _>>();
        for previous in &self.operations {
            let current = next_operations
                .get(&previous.operation_kind.0)
                .ok_or(ProfileError::OperationRemoved(previous.operation_kind))?;
            if previous != *current && current.version <= previous.version {
                return Err(ProfileError::OperationChangedWithoutVersion(
                    previous.operation_kind,
                ));
            }
        }
        Ok(())
    }

    fn calculate_digest(&self) -> Result<[u8; 32], ProfileError> {
        let encoded =
            postcard::to_stdvec(&(self.format_version, &self.aggregates, &self.operations))?;
        Ok(*blake3::hash(&encoded).as_bytes())
    }
}

/// Trait implemented by `AequoraAggregate` derive and usable by custom builders.
pub trait AggregateDefinition {
    const AGGREGATE_ID: AggregateProfileId;
    const PROFILE: ConsistencyProfileKind;
}

/// Optional operation-profile metadata emitted by `AequoraOperation` derive.
pub trait OperationProfileDefinition {
    const AGGREGATE_ID: AggregateProfileId;
    const SEMANTIC_CLASS: OperationSemanticClass;
}

/// Typed marker profiles for advanced generic constraints.
pub mod markers {
    use super::ConsistencyProfileKind;

    pub trait BuiltInProfileMarker {
        const KIND: ConsistencyProfileKind;
    }

    macro_rules! marker {
        ($name:ident, $kind:ident) => {
            #[derive(Clone, Copy, Debug, Default)]
            pub struct $name;
            impl BuiltInProfileMarker for $name {
                const KIND: ConsistencyProfileKind = ConsistencyProfileKind::$kind;
            }
        };
    }

    marker!(ImmutableAppendOnly, ImmutableAppendOnly);
    marker!(OptimisticVersioned, OptimisticVersioned);
    marker!(Commutative, Commutative);
    marker!(LastWriterWins, LastWriterWins);
    marker!(ManualConflict, ManualConflict);
    marker!(StrongAggregate, StrongAggregate);
    marker!(ServerOnly, ServerOnly);
    marker!(DeviceLocal, DeviceLocal);
    marker!(DerivedProjection, DerivedProjection);
}

/// Fail-closed profile and compatibility diagnostics.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProfileError {
    #[error("aggregate profile identity must be non-zero")]
    ZeroIdentity,
    #[error("profile version must be non-zero")]
    ZeroVersion,
    #[error("profile name must contain 1 through 128 bytes")]
    InvalidName,
    #[error("built-in profile dimensions require an explicit custom profile")]
    BuiltInOverrideRequiresCustom,
    #[error("custom profile overrides require advanced opt-in")]
    CustomProfileRequiresOptIn,
    #[error("device-local state cannot claim replicated semantics")]
    DeviceLocalReplicated,
    #[error("derived projection cannot claim independent authority")]
    DerivedClaimsAuthority,
    #[error("immutable append-only semantics prohibit update, rebase, or compaction")]
    ImmutableMutation,
    #[error("strong aggregate cannot use last-writer-wins")]
    StrongAggregateLastWriterWins,
    #[error("profile retry policy is incompatible with its authority/scope policy")]
    RetryScopeMismatch,
    #[error("profile requirements omit semantic capability {0:?}")]
    IncompleteRequirements(ProfileCapability),
    #[error("finance aggregate requires audited strong or immutable semantics")]
    UnsafeFinanceProfile,
    #[error("workflow/security aggregate requires audited non-LWW semantics")]
    UnsafeSensitiveProfile,
    #[error("aggregate {0:?} is already registered")]
    DuplicateAggregate(AggregateProfileId),
    #[error("aggregate {0:?} is not registered")]
    UnknownAggregate(AggregateProfileId),
    #[error("operation {0:?} is already registered")]
    DuplicateOperation(OperationKind),
    #[error("operation {0:?} is not registered; implicit fallback is prohibited")]
    UnknownOperation(OperationKind),
    #[error("derived-only operation is attached to independent authority")]
    DerivedOperationOnAuthority,
    #[error("server-only or derived aggregate cannot accept client-authored operations")]
    ServerOnlyClientOperation,
    #[error("increment requires a commutative aggregate profile")]
    IncrementNotCommutative,
    #[error("workflow transition cannot rebase, compact, or use LWW")]
    UnsafeTransition,
    #[error("compaction is incompatible with the aggregate profile")]
    CompactionIncompatible,
    #[error("sensitive operation must be audited, noncompactable, and nonrebasable")]
    UnsafeSensitiveOperation,
    #[error("aggregate {aggregate_id:?} lacks adapter capabilities {missing:?}")]
    MissingCapabilities {
        aggregate_id: AggregateProfileId,
        missing: BTreeSet<ProfileCapability>,
    },
    #[error("profile manifest version is unsupported")]
    UnsupportedManifestVersion,
    #[error("profile manifest entries are not strictly ordered")]
    ManifestOrder,
    #[error("profile manifest digest does not match")]
    ManifestDigestMismatch,
    #[error("aggregate {0:?} was removed from a successor manifest")]
    AggregateRemoved(AggregateProfileId),
    #[error("operation {0:?} was removed from a successor manifest")]
    OperationRemoved(OperationKind),
    #[error("aggregate {0:?} semantics changed without a higher profile version")]
    AggregateChangedWithoutVersion(AggregateProfileId),
    #[error("operation {0:?} semantics changed without a higher profile version")]
    OperationChangedWithoutVersion(OperationKind),
    #[error("profile manifest serialization failed")]
    Codec,
}

impl From<postcard::Error> for ProfileError {
    fn from(_: postcard::Error) -> Self {
        Self::Codec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn id(value: u16) -> AggregateProfileId {
        AggregateProfileId::new(value).unwrap_or_else(|error| panic!("{error}"))
    }

    fn full_capabilities() -> AdapterProfileCapabilities {
        AdapterProfileCapabilities::from_transactions(TransactionCapabilities::FULL_AUTHORITATIVE)
            .with(ProfileCapability::DurableConflictInbox)
            .with(ProfileCapability::AuthoritativeClock)
    }

    #[test]
    fn every_built_in_is_coherent_and_unknown_operations_fail_closed() {
        for kind in [
            ConsistencyProfileKind::ImmutableAppendOnly,
            ConsistencyProfileKind::OptimisticVersioned,
            ConsistencyProfileKind::Commutative,
            ConsistencyProfileKind::LastWriterWins,
            ConsistencyProfileKind::ManualConflict,
            ConsistencyProfileKind::StrongAggregate,
            ConsistencyProfileKind::ServerOnly,
            ConsistencyProfileKind::DeviceLocal,
            ConsistencyProfileKind::DerivedProjection,
        ] {
            ConsistencyProfile::built_in(kind)
                .validate()
                .unwrap_or_else(|error| panic!("{error}"));
        }
        assert_eq!(
            ProfileRegistry::new().require_operation(OperationKind(999)),
            Err(ProfileError::UnknownOperation(OperationKind(999)))
        );
    }

    #[test]
    fn invalid_combinations_and_missing_capabilities_are_rejected() {
        assert_eq!(
            CustomProfileBuilder::new(ConsistencyProfileKind::StrongAggregate)
                .overrides(ProfileOverrides {
                    conflict: Some(ConflictPolicy::LastWriterWins),
                    ..ProfileOverrides::default()
                })
                .advanced_opt_in()
                .build(),
            Err(ProfileError::StrongAggregateLastWriterWins)
        );
        assert_eq!(
            CustomProfileBuilder::new(ConsistencyProfileKind::OptimisticVersioned)
                .overrides(ProfileOverrides {
                    retry: Some(ProfileRetryPolicy::ServerManaged),
                    ..ProfileOverrides::default()
                })
                .advanced_opt_in()
                .build(),
            Err(ProfileError::RetryScopeMismatch)
        );
        assert_eq!(
            CustomProfileBuilder::new(ConsistencyProfileKind::LastWriterWins)
                .overrides(ProfileOverrides {
                    requirements: Some(ProfileRequirements::default()),
                    ..ProfileOverrides::default()
                })
                .advanced_opt_in()
                .build(),
            Err(ProfileError::IncompleteRequirements(
                ProfileCapability::IdempotentCommit
            ))
        );
        let invalid_version = AggregateProfileBuilder::new(
            id(9),
            "invalid-version",
            ConsistencyProfileKind::OptimisticVersioned,
        )
        .version(ProfileVersion(0))
        .build();
        assert_eq!(invalid_version, Err(ProfileError::ZeroVersion));
        let aggregate =
            AggregateProfileBuilder::new(id(1), "ledger", ConsistencyProfileKind::StrongAggregate)
                .risk(DomainRisk::Finance)
                .build()
                .unwrap_or_else(|error| panic!("{error}"));
        let mut registry = ProfileRegistry::new();
        registry
            .register_aggregate(aggregate)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            registry
                .validate_capabilities(&AdapterProfileCapabilities::default())
                .is_err()
        );
        registry
            .validate_capabilities(&full_capabilities())
            .unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn finance_and_workflow_defaults_preserve_every_intent() {
        let ledger =
            AggregateProfileBuilder::new(id(1), "ledger", ConsistencyProfileKind::StrongAggregate)
                .risk(DomainRisk::Finance)
                .build()
                .unwrap_or_else(|error| panic!("{error}"));
        let payment = OperationProfileBuilder::new(
            OperationKind(1),
            "post_payment",
            id(1),
            OperationSemanticClass::Command,
        )
        .audit(AuditPolicy::Required)
        .build();
        let workflow = AggregateProfileBuilder::new(
            id(2),
            "approval",
            ConsistencyProfileKind::StrongAggregate,
        )
        .risk(DomainRisk::Workflow)
        .build()
        .unwrap_or_else(|error| panic!("{error}"));
        let approve = OperationProfileBuilder::new(
            OperationKind(2),
            "approve",
            id(2),
            OperationSemanticClass::Transition,
        )
        .audit(AuditPolicy::Required)
        .build();
        let mut registry = ProfileRegistry::new();
        registry
            .register_aggregate(ledger)
            .unwrap_or_else(|error| panic!("{error}"));
        registry
            .register_aggregate(workflow)
            .unwrap_or_else(|error| panic!("{error}"));
        registry
            .register_operation(payment)
            .unwrap_or_else(|error| panic!("{error}"));
        registry
            .register_operation(approve)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            registry
                .require_operation(OperationKind(1))
                .unwrap_or_else(|error| panic!("{error}"))
                .compaction,
            CompactionPolicy::Never
        );
    }

    #[test]
    fn manifest_detects_unversioned_semantic_drift() {
        let mut registry = ProfileRegistry::new();
        registry
            .register_aggregate(
                AggregateProfileBuilder::new(
                    id(1),
                    "student",
                    ConsistencyProfileKind::OptimisticVersioned,
                )
                .build()
                .unwrap_or_else(|error| panic!("{error}")),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        registry
            .register_operation(
                OperationProfileBuilder::new(
                    OperationKind(1),
                    "set_phone",
                    id(1),
                    OperationSemanticClass::SetValue,
                )
                .build(),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let first = registry
            .manifest()
            .unwrap_or_else(|error| panic!("{error}"));
        first.verify().unwrap_or_else(|error| panic!("{error}"));
        let mut changed = first.clone();
        changed.operations[0].rebase = RebasePolicy::FieldAware;
        changed.digest = changed
            .calculate_digest()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            first.verify_compatible_successor(&changed),
            Err(ProfileError::OperationChangedWithoutVersion(OperationKind(
                1
            )))
        );
        changed.operations[0].version =
            ProfileVersion::new(2).unwrap_or_else(|error| panic!("{error}"));
        changed.digest = changed
            .calculate_digest()
            .unwrap_or_else(|error| panic!("{error}"));
        first
            .verify_compatible_successor(&changed)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    proptest! {
        #[test]
        fn registry_manifest_is_insertion_order_independent(mut ids in prop::collection::btree_set(1_u16..500, 1..30).prop_map(|values| values.into_iter().collect::<Vec<_>>())) {
            let mut forward = ProfileRegistry::new();
            for value in &ids {
                forward.register_aggregate(
                    AggregateProfileBuilder::new(id(*value), format!("aggregate_{value}"), ConsistencyProfileKind::OptimisticVersioned)
                        .build().unwrap_or_else(|error| panic!("{error}"))
                ).unwrap_or_else(|error| panic!("{error}"));
            }
            ids.reverse();
            let mut reverse = ProfileRegistry::new();
            for value in &ids {
                reverse.register_aggregate(
                    AggregateProfileBuilder::new(id(*value), format!("aggregate_{value}"), ConsistencyProfileKind::OptimisticVersioned)
                        .build().unwrap_or_else(|error| panic!("{error}"))
                ).unwrap_or_else(|error| panic!("{error}"));
            }
            prop_assert_eq!(
                forward.manifest().unwrap_or_else(|error| panic!("{error}")),
                reverse.manifest().unwrap_or_else(|error| panic!("{error}")),
            );
        }
    }
}
