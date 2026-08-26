//! Capability-based persistence contracts.
//!
//! Implementations must preserve the atomicity requirements on each method. No database
//! transaction type appears in this public API.

use aequora_coordination::{
    CoordinationSnapshot, FencingToken, LeaseGrant, LeaseRequest, LocalCoordinationSupport,
    LocalStoreGeneration,
};
use aequora_executor::CurrentEntity;
use aequora_integrity::{
    CanonicalEntity, IntegrityGeneration, IntegritySnapshot, IntegritySupport, PartitionScheme,
    RepairPlan,
};
use aequora_protocol::{
    BootstrapResponse, OperationAck, OperationEnvelope, OperationRejection, Partition,
    RemoteChange, SnapshotEntity, SyncResponse,
};
use aequora_queue::{
    CompactionPlan as QueueCompactionPlan, OptimizationRegistry, RebasePlan, RebaseTarget,
};
use aequora_scope::{LocalScopeState, ScopeTransition, ScopeTransitionOutcome, Subscription};
use aequora_types::{
    ActorId, CorrelationId, Cursor, DeviceId, EntityRef, EntityVersion, EventId, HybridTimestamp,
    LineageContext, LineageRef, OperationId, Sequence, SnapshotId, SyncScopeId, TenantId,
};
use async_trait::async_trait;
use serde::Serialize;
use thiserror::Error;

/// Logical role implemented by a database adapter.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AdapterRole {
    /// Writable local-first replica with durable intent.
    LocalWritable,
    /// Writable server authority with idempotent commits and a journal.
    AuthoritativeWritable,
    /// Read-only source used by import or migration tooling.
    ReadOnlySource,
    /// Destination that can apply canonical replica state.
    ReplicaSink,
    /// Consistent snapshot producer.
    SnapshotSource,
    /// Staged, atomic snapshot consumer.
    SnapshotSink,
    /// Source of database-native change capture for legacy bridges.
    ChangeCaptureSource,
}

/// Public support classification for an adapter.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AdapterTier {
    /// Read-only import/export integration, not a synchronization store.
    ReadOnlyImport,
    /// Unverified or best-effort integration that must not be advertised for production.
    Experimental,
    /// Production integration with explicitly documented limitations.
    ProductionWithLimitations,
    /// Full production adapter passing all required contracts for its advertised roles.
    FullProduction,
}

/// Compact, database-neutral adapter feature declaration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AdapterCapabilities(u32);

impl AdapterCapabilities {
    /// Successful writes survive process/database restart under the configured database policy.
    pub const DURABLE_TRANSACTIONS: Self = Self(1 << 0);
    /// Application/domain state and Aequora metadata can share a native transaction.
    pub const ATOMIC_METADATA_COUPLING: Self = Self(1 << 1);
    /// Durable outbox state and retry schedule are supported.
    pub const OUTBOX: Self = Self(1 << 2);
    /// Authoritative apply, terminal results, applied markers, and cursor reconcile atomically.
    pub const RECONCILIATION: Self = Self(1 << 3);
    /// Durable manual-conflict records are supported.
    pub const CONFLICT_STORE: Self = Self(1 << 4);
    /// Idempotent authoritative operation ledger is supported.
    pub const OPERATION_LEDGER: Self = Self(1 << 5);
    /// Ordered authoritative change journal is supported.
    pub const CHANGE_JOURNAL: Self = Self(1 << 6);
    /// Concurrent version compare-and-set semantics are supported.
    pub const CONCURRENT_VERSIONING: Self = Self(1 << 7);
    /// Consistent snapshots can be read or staged according to the advertised role.
    pub const SNAPSHOTS: Self = Self(1 << 8);
    /// Migrations are ordered, checksummed, and transactionally recorded.
    pub const TRANSACTIONAL_MIGRATIONS: Self = Self(1 << 9);
    /// Immutable, payload-free authoritative audit evidence is supported.
    pub const AUDIT_LOG: Self = Self(1 << 10);
    /// Mutable-unsent queue compaction, supersession, and rebase commit atomically.
    pub const QUEUE_OPTIMIZATION: Self = Self(1 << 11);
    /// Durable local lease election, fencing, and generation transitions are supported.
    pub const LOCAL_COORDINATION: Self = Self(1 << 12);
    /// Scope subscriptions, membership, transitions, and pending-intent quarantine are atomic.
    pub const SCOPE_STATE: Self = Self(1 << 13);

    /// Capabilities required from a full-production writable local adapter.
    pub const FULL_LOCAL: Self = Self::DURABLE_TRANSACTIONS
        .union(Self::ATOMIC_METADATA_COUPLING)
        .union(Self::OUTBOX)
        .union(Self::RECONCILIATION)
        .union(Self::CONFLICT_STORE)
        .union(Self::QUEUE_OPTIMIZATION)
        .union(Self::LOCAL_COORDINATION)
        .union(Self::SCOPE_STATE)
        .union(Self::SNAPSHOTS)
        .union(Self::TRANSACTIONAL_MIGRATIONS);

    /// Capabilities required from a full-production authoritative adapter.
    pub const FULL_AUTHORITATIVE: Self = Self::DURABLE_TRANSACTIONS
        .union(Self::ATOMIC_METADATA_COUPLING)
        .union(Self::OPERATION_LEDGER)
        .union(Self::CHANGE_JOURNAL)
        .union(Self::CONCURRENT_VERSIONING)
        .union(Self::SNAPSHOTS)
        .union(Self::TRANSACTIONAL_MIGRATIONS)
        .union(Self::AUDIT_LOG);

    /// Combines two capability sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every capability in `required` is present.
    #[must_use]
    pub const fn contains(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }

    /// Stable numeric representation for compact diagnostics and generated support matrices.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

/// Versioned, serializable declaration published by one concrete database adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterManifest {
    /// Stable adapter name used in diagnostics and configuration.
    pub name: &'static str,
    /// Adapter crate version.
    pub adapter_version: &'static str,
    /// Aequora core version against which the adapter was tested.
    pub tested_aequora_version: &'static str,
    /// Database engine versions exercised by the adapter maintainers.
    pub tested_database_versions: &'static [&'static str],
    /// Logical roles implemented by this adapter.
    pub roles: &'static [AdapterRole],
    /// Public support classification.
    pub tier: AdapterTier,
    /// Database-neutral capabilities available in those roles.
    pub capabilities: AdapterCapabilities,
    /// Human-readable limitations; empty for an unrestricted Tier A declaration.
    pub limitations: &'static [&'static str],
}

impl AdapterManifest {
    /// Whether the adapter advertises a particular logical role.
    #[must_use]
    pub fn supports_role(self, required: AdapterRole) -> bool {
        self.roles.contains(&required)
    }

    /// Verifies that a manifest's tier, roles, and capability declaration are internally sound.
    ///
    /// # Errors
    ///
    /// Returns a typed compatibility error for incomplete identity/version data, unsupported Tier A
    /// claims, or a limitations/tier mismatch.
    pub fn validate(self) -> Result<(), AdapterCompatibilityError> {
        if self.name.is_empty()
            || self.adapter_version.is_empty()
            || self.tested_aequora_version.is_empty()
            || self.tested_database_versions.is_empty()
            || self.roles.is_empty()
        {
            return Err(AdapterCompatibilityError::IncompleteManifest { adapter: self.name });
        }
        if self.tier == AdapterTier::FullProduction && !self.limitations.is_empty() {
            return Err(AdapterCompatibilityError::TierContradictsLimitations {
                adapter: self.name,
            });
        }
        if self.tier == AdapterTier::ProductionWithLimitations && self.limitations.is_empty() {
            return Err(AdapterCompatibilityError::MissingLimitations { adapter: self.name });
        }
        if self.tier == AdapterTier::FullProduction {
            if self.supports_role(AdapterRole::LocalWritable)
                && !self.capabilities.contains(AdapterCapabilities::FULL_LOCAL)
            {
                return Err(AdapterCompatibilityError::InvalidTierClaim {
                    adapter: self.name,
                    role: AdapterRole::LocalWritable,
                });
            }
            if self.supports_role(AdapterRole::AuthoritativeWritable)
                && !self
                    .capabilities
                    .contains(AdapterCapabilities::FULL_AUTHORITATIVE)
            {
                return Err(AdapterCompatibilityError::InvalidTierClaim {
                    adapter: self.name,
                    role: AdapterRole::AuthoritativeWritable,
                });
            }
        }
        Ok(())
    }
}

/// Application requirements checked against an adapter before synchronization starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterRequirements {
    /// Logical role the application needs.
    pub role: AdapterRole,
    /// Minimum acceptable public support tier.
    pub minimum_tier: AdapterTier,
    /// Capabilities required by application policy.
    pub capabilities: AdapterCapabilities,
    /// Exact Aequora compatibility version expected by the host.
    pub aequora_version: &'static str,
}

impl AdapterRequirements {
    /// Standard production requirements for a writable local-first database.
    pub const PRODUCTION_LOCAL: Self = Self {
        role: AdapterRole::LocalWritable,
        minimum_tier: AdapterTier::FullProduction,
        capabilities: AdapterCapabilities::FULL_LOCAL,
        aequora_version: env!("CARGO_PKG_VERSION"),
    };

    /// Standard production requirements for a writable authoritative database.
    pub const PRODUCTION_AUTHORITATIVE: Self = Self {
        role: AdapterRole::AuthoritativeWritable,
        minimum_tier: AdapterTier::FullProduction,
        capabilities: AdapterCapabilities::FULL_AUTHORITATIVE,
        aequora_version: env!("CARGO_PKG_VERSION"),
    };

    /// Checks startup requirements against one concrete adapter declaration.
    ///
    /// # Errors
    ///
    /// Returns a typed, payload-free compatibility error and never silently weakens requirements.
    pub fn verify(self, manifest: AdapterManifest) -> Result<(), AdapterCompatibilityError> {
        manifest.validate()?;
        if manifest.tested_aequora_version != self.aequora_version {
            return Err(AdapterCompatibilityError::AequoraVersionMismatch {
                adapter: manifest.name,
                tested: manifest.tested_aequora_version,
                running: self.aequora_version,
            });
        }
        if !manifest.supports_role(self.role) {
            return Err(AdapterCompatibilityError::UnsupportedRole {
                adapter: manifest.name,
                required: self.role,
            });
        }
        if manifest.tier < self.minimum_tier {
            return Err(AdapterCompatibilityError::InsufficientTier {
                adapter: manifest.name,
                required: self.minimum_tier,
                actual: manifest.tier,
            });
        }
        if !manifest.capabilities.contains(self.capabilities) {
            return Err(AdapterCompatibilityError::MissingCapabilities {
                adapter: manifest.name,
                required: self.capabilities,
                actual: manifest.capabilities,
            });
        }
        Ok(())
    }
}

/// Validated, database-neutral production composition selected during application startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProductionAdapterPair {
    /// Writable local replica declaration.
    pub local: AdapterManifest,
    /// Writable authority declaration.
    pub authoritative: AdapterManifest,
}

impl ProductionAdapterPair {
    /// Verifies two independently selected adapters without introducing pair-specific behavior.
    ///
    /// The same manifest may be supplied for both sides when one adapter safely implements both
    /// roles. Different database engines do not require a special protocol or bridge.
    ///
    /// # Errors
    ///
    /// Returns the first fail-closed role, tier, version, or capability incompatibility.
    pub fn verify(
        local: AdapterManifest,
        authoritative: AdapterManifest,
    ) -> Result<Self, AdapterCompatibilityError> {
        AdapterRequirements::PRODUCTION_LOCAL.verify(local)?;
        AdapterRequirements::PRODUCTION_AUTHORITATIVE.verify(authoritative)?;
        Ok(Self {
            local,
            authoritative,
        })
    }
}

/// Typed startup rejection for incompatible or misleading adapter declarations.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum AdapterCompatibilityError {
    /// Required identity, version, database, or role information is absent.
    #[error("adapter {adapter:?} has an incomplete capability manifest")]
    IncompleteManifest { adapter: &'static str },
    /// Tier A was claimed without all semantics required by a writable role.
    #[error("adapter {adapter:?} cannot claim FullProduction for {role:?}")]
    InvalidTierClaim {
        adapter: &'static str,
        role: AdapterRole,
    },
    /// Tier A cannot carry known limitations.
    #[error("adapter {adapter:?} claims FullProduction but declares limitations")]
    TierContradictsLimitations { adapter: &'static str },
    /// Tier B must state its limitations explicitly.
    #[error("adapter {adapter:?} claims ProductionWithLimitations without listing limitations")]
    MissingLimitations { adapter: &'static str },
    /// The adapter has not been tested against this Aequora compatibility version.
    #[error("adapter {adapter:?} targets Aequora {tested}, but the host requires {running}")]
    AequoraVersionMismatch {
        adapter: &'static str,
        tested: &'static str,
        running: &'static str,
    },
    /// The adapter does not implement the logical role selected by the host.
    #[error("adapter {adapter:?} does not support required role {required:?}")]
    UnsupportedRole {
        adapter: &'static str,
        required: AdapterRole,
    },
    /// The adapter's public support tier is below application policy.
    #[error("adapter {adapter:?} tier {actual:?} is below required tier {required:?}")]
    InsufficientTier {
        adapter: &'static str,
        required: AdapterTier,
        actual: AdapterTier,
    },
    /// One or more required semantic capabilities are absent.
    #[error("adapter {adapter:?} capabilities {actual:?} do not contain required {required:?}")]
    MissingCapabilities {
        adapter: &'static str,
        required: AdapterCapabilities,
        actual: AdapterCapabilities,
    },
}

/// Implemented by concrete adapters that publish a stable capability manifest.
pub trait AdapterManifestProvider: Send + Sync {
    /// Returns the adapter's versioned roles, support tier, and semantic capabilities.
    fn adapter_manifest(&self) -> AdapterManifest;
}

/// Persistence durability promised by an adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurabilityMode {
    /// State survives only for the lifetime of the current process.
    Volatile,
    /// A successful commit survives process and database restarts according to the database's
    /// configured durable-commit guarantees.
    Durable,
}

/// Highest Aequora transaction boundary implemented by an adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcidComplianceLevel {
    /// Deterministic reference implementation for tests; never a production durability claim.
    Reference,
    /// Full writable local replica: domain mutation/outbox plus reconciliation/cursor atomicity.
    FullLocal,
    /// Full authoritative persistence: entity/version/journal/ledger/audit atomicity.
    FullAuthoritative,
}

/// Explicit transaction guarantees advertised by a persistence adapter.
///
/// This declaration is intentionally separate from the capability method traits. Implementing a
/// Rust trait proves that methods exist; this value states which cross-record transaction and
/// restart guarantees the implementation claims. Third-party adapters should return a production
/// level only after passing the matching `aequora-testkit` compliance suite against their real
/// database engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionCapabilities {
    /// Highest complete Aequora boundary implemented by this adapter.
    pub compliance: AcidComplianceLevel,
    /// Whether successful commits survive process/database restart.
    pub durability: DurabilityMode,
    /// Individual cross-record guarantees implemented by the adapter.
    pub guarantees: TransactionGuarantees,
}

/// Compact set of cross-record ACID guarantees.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TransactionGuarantees(u16);

impl TransactionGuarantees {
    /// Optimistic local domain mutation and outbox insertion share one transaction.
    pub const LOCAL_MUTATION_OUTBOX: Self = Self(1 << 0);
    /// Authoritative entity/version/journal/operation-ledger/audit writes share one transaction.
    pub const AUTHORITATIVE_COMMIT: Self = Self(1 << 1);
    /// Changes, terminal results, conflicts, applied markers, and cursor share one transaction.
    pub const RECONCILIATION_CURSOR: Self = Self(1 << 2);
    /// Concurrent requests with one operation ID have exactly one logical effect.
    pub const CONCURRENT_IDEMPOTENCY: Self = Self(1 << 3);
    /// Snapshot contents and their journal cursor are captured from one consistent boundary.
    pub const CONSISTENT_SNAPSHOT: Self = Self(1 << 4);
    /// Schema migrations are ordered, checksummed, and transactionally recorded.
    pub const TRANSACTIONAL_MIGRATIONS: Self = Self(1 << 5);
    /// Queue removal, supersession evidence, dependency safety, and rebase rewrite are atomic.
    pub const QUEUE_REWRITE: Self = Self(1 << 6);
    /// Lease transitions and stale-fence rejection occur inside leader-exclusive transactions.
    pub const LOCAL_COORDINATION_FENCING: Self = Self(1 << 7);

    /// Combines two guarantee sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every guarantee in `required` is present.
    #[must_use]
    pub const fn contains(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }
}

impl TransactionCapabilities {
    /// Volatile local reference-store declaration used for deterministic fault and model tests.
    pub const REFERENCE_LOCAL: Self = Self {
        compliance: AcidComplianceLevel::Reference,
        durability: DurabilityMode::Volatile,
        guarantees: TransactionGuarantees::LOCAL_MUTATION_OUTBOX
            .union(TransactionGuarantees::RECONCILIATION_CURSOR)
            .union(TransactionGuarantees::QUEUE_REWRITE)
            .union(TransactionGuarantees::LOCAL_COORDINATION_FENCING),
    };

    /// Volatile authoritative reference declaration used for deterministic concurrency tests.
    pub const REFERENCE_AUTHORITATIVE: Self = Self {
        compliance: AcidComplianceLevel::Reference,
        durability: DurabilityMode::Volatile,
        guarantees: TransactionGuarantees::AUTHORITATIVE_COMMIT
            .union(TransactionGuarantees::CONCURRENT_IDEMPOTENCY)
            .union(TransactionGuarantees::CONSISTENT_SNAPSHOT),
    };

    /// Required declaration for a production writable local adapter.
    pub const FULL_LOCAL: Self = Self {
        compliance: AcidComplianceLevel::FullLocal,
        durability: DurabilityMode::Durable,
        guarantees: TransactionGuarantees::LOCAL_MUTATION_OUTBOX
            .union(TransactionGuarantees::RECONCILIATION_CURSOR)
            .union(TransactionGuarantees::QUEUE_REWRITE)
            .union(TransactionGuarantees::LOCAL_COORDINATION_FENCING)
            .union(TransactionGuarantees::TRANSACTIONAL_MIGRATIONS),
    };

    /// Required declaration for a production authoritative adapter.
    pub const FULL_AUTHORITATIVE: Self = Self {
        compliance: AcidComplianceLevel::FullAuthoritative,
        durability: DurabilityMode::Durable,
        guarantees: TransactionGuarantees::AUTHORITATIVE_COMMIT
            .union(TransactionGuarantees::CONCURRENT_IDEMPOTENCY)
            .union(TransactionGuarantees::CONSISTENT_SNAPSHOT)
            .union(TransactionGuarantees::TRANSACTIONAL_MIGRATIONS),
    };

    /// Checks that the detailed flags are internally consistent with the advertised level.
    #[must_use]
    pub const fn is_consistent(self) -> bool {
        match self.compliance {
            AcidComplianceLevel::Reference => {
                matches!(self.durability, DurabilityMode::Volatile)
            }
            AcidComplianceLevel::FullLocal => {
                matches!(self.durability, DurabilityMode::Durable)
                    && self.guarantees.contains(
                        TransactionGuarantees::LOCAL_MUTATION_OUTBOX
                            .union(TransactionGuarantees::RECONCILIATION_CURSOR)
                            .union(TransactionGuarantees::QUEUE_REWRITE)
                            .union(TransactionGuarantees::LOCAL_COORDINATION_FENCING)
                            .union(TransactionGuarantees::TRANSACTIONAL_MIGRATIONS),
                    )
            }
            AcidComplianceLevel::FullAuthoritative => {
                matches!(self.durability, DurabilityMode::Durable)
                    && self.guarantees.contains(
                        TransactionGuarantees::AUTHORITATIVE_COMMIT
                            .union(TransactionGuarantees::CONCURRENT_IDEMPOTENCY)
                            .union(TransactionGuarantees::CONSISTENT_SNAPSHOT)
                            .union(TransactionGuarantees::TRANSACTIONAL_MIGRATIONS),
                    )
            }
        }
    }
}

/// Adapter capability declaration used by startup diagnostics and compliance tests.
pub trait TransactionCapabilityProvider: Send + Sync {
    /// Returns transaction and durability guarantees for this concrete adapter.
    fn transaction_capabilities(&self) -> TransactionCapabilities;
}

/// Optional anti-entropy capability declaration, independent of database brand.
pub trait IntegrityCapabilityProvider: Send + Sync {
    /// Returns the strongest canonical verification/repair level implemented by this adapter.
    fn integrity_support(&self) -> IntegritySupport;
}

/// Stored authoritative entity state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntitySnapshot {
    /// Entity identity.
    pub entity: EntityRef,
    /// Tenant that owns the entity.
    pub tenant_id: TenantId,
    /// Current application snapshot.
    pub current: CurrentEntity,
}

/// Atomic authoritative commit requested after validation and execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitOperation {
    /// Original permanent operation ID.
    pub operation_id: OperationId,
    /// Stable authoritative event identity selected before the atomic commit.
    pub event_id: EventId,
    /// Retry-stable root correlation and direct cause from the authenticated operation.
    pub operation_lineage: LineageContext,
    /// Authenticated actor responsible for the command.
    pub actor_id: ActorId,
    /// Authenticated device from which the command originated.
    pub device_id: DeviceId,
    /// Registered application operation kind.
    pub operation_kind: u16,
    /// Tenant owning the effect.
    pub tenant_id: TenantId,
    /// Journal scope.
    pub scope_id: SyncScopeId,
    /// Target entity.
    pub entity: EntityRef,
    /// Version observed before application. The store must compare it atomically.
    pub expected_version: Option<EntityVersion>,
    /// Resulting version selected by the server.
    pub next_version: EntityVersion,
    /// Authoritative state transition payload.
    pub payload: Vec<u8>,
    /// Upsert or tombstone.
    pub change_kind: aequora_protocol::ChangeKind,
    /// Event timestamp.
    pub timestamp: HybridTimestamp,
    /// BLAKE3 digest of the command payload. The audit log never stores the command payload.
    pub command_digest: [u8; 32],
}

impl CommitOperation {
    /// Whether the requested authoritative version is exactly the required next value.
    #[must_use]
    pub const fn has_valid_version_transition(&self) -> bool {
        match self.expected_version {
            None => self.next_version.get() == EntityVersion::INITIAL.get(),
            Some(expected) => match expected.checked_next() {
                Some(next) => next.get() == self.next_version.get(),
                None => false,
            },
        }
    }

    /// Lineage of the authoritative event directly caused by this operation.
    #[must_use]
    pub const fn event_lineage(&self) -> LineageContext {
        self.operation_lineage
            .derived(LineageRef::Operation(self.operation_id))
    }
}

/// Monotonic position in the immutable accountability log.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct AuditOffset(pub u64);

/// Payload-free immutable evidence of one committed authoritative command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditRecord {
    /// Monotonic audit position, independent of synchronization cursors.
    pub offset: AuditOffset,
    /// Authenticated tenant boundary.
    pub tenant_id: TenantId,
    /// Permanent idempotency key.
    pub operation_id: OperationId,
    /// Authoritative event published by the command.
    pub event_id: EventId,
    /// Original operation correlation and direct cause retained without customer payloads.
    pub operation_lineage: LineageContext,
    /// Authenticated actor responsible for the command.
    pub actor_id: ActorId,
    /// Authenticated originating device.
    pub device_id: DeviceId,
    /// Registered application command kind.
    pub operation_kind: u16,
    /// Entity affected by the authoritative transition.
    pub entity: EntityRef,
    /// Resulting authoritative version.
    pub entity_version: EntityVersion,
    /// BLAKE3 command digest for accountability without retaining sensitive payloads.
    pub command_digest: [u8; 32],
    /// Authoritative commit timestamp.
    pub timestamp: HybridTimestamp,
}

/// One bounded immutable audit page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditPage {
    /// Records ordered by ascending audit offset.
    pub records: Vec<AuditRecord>,
    /// Last returned offset, or the input offset for an empty page.
    pub next_offset: AuditOffset,
    /// Whether another retained record follows this page.
    pub has_more: bool,
}

/// Result of an authoritative atomic commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitOutcome {
    /// This operation was newly applied.
    Applied(OperationAck),
    /// The operation already existed; the stored deterministic result is returned.
    Duplicate(OperationAck),
    /// State changed after validation. Nothing was committed.
    VersionChanged { current: Option<EntityVersion> },
    /// Transaction-time application validation rejected the operation. Nothing was committed.
    Rejected(OperationRejection),
}

/// One bounded page of authoritative journal entries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangePage {
    /// Ordered events after the requested sequence.
    pub changes: Vec<RemoteChange>,
    /// Greatest complete sequence represented by this page.
    pub next_sequence: Sequence,
    /// Current highest retained sequence in this tenant and scope when the page was read.
    pub journal_head: Sequence,
    /// Whether more events remain.
    pub has_more: bool,
}

/// Identity and journal boundary of a newly captured consistent snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotDescriptor {
    /// Stable identity used to resume pages.
    pub snapshot_id: SnapshotId,
    /// Incremental synchronization starts strictly after this cursor.
    pub cursor: Cursor,
}

/// Bounded page read from an immutable consistent snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotPage {
    /// Identity and cursor shared by every page.
    pub descriptor: SnapshotDescriptor,
    /// Snapshot entities beginning at the requested offset.
    pub entities: Vec<SnapshotEntity>,
    /// Offset for the next page.
    pub next_offset: u64,
    /// True when another page remains.
    pub has_more: bool,
}

/// Persistence failure at a library boundary.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("storage {kind:?}: {message}")]
pub struct StoreError {
    /// Stable failure category used to decide retry behavior.
    pub kind: StoreErrorKind,
    /// More specific transient cause used for safe whole-transaction retry decisions.
    pub reason: StoreErrorReason,
    /// Non-sensitive implementation explanation.
    pub message: String,
}

impl StoreError {
    /// Creates a transient storage failure.
    #[must_use]
    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            kind: StoreErrorKind::Transient,
            reason: StoreErrorReason::Unspecified,
            message: message.into(),
        }
    }

    /// Creates a transient storage failure with a stable retry reason.
    #[must_use]
    pub fn transient_with_reason(reason: StoreErrorReason, message: impl Into<String>) -> Self {
        Self {
            kind: StoreErrorKind::Transient,
            reason,
            message: message.into(),
        }
    }

    /// Creates a permanent/corruption storage failure.
    #[must_use]
    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            kind: StoreErrorKind::Permanent,
            reason: StoreErrorReason::Unspecified,
            message: message.into(),
        }
    }

    /// Creates a transient stale-leader failure that must not retry under the same lease.
    #[must_use]
    pub fn leadership_lost(message: impl Into<String>) -> Self {
        Self {
            kind: StoreErrorKind::Transient,
            reason: StoreErrorReason::LeadershipLost,
            message: message.into(),
        }
    }

    /// Whether retrying the complete database transaction is safe and specifically required.
    #[must_use]
    pub const fn requires_transaction_retry(&self) -> bool {
        matches!(
            self.reason,
            StoreErrorReason::SerializationFailure | StoreErrorReason::Deadlock
        )
    }
}

/// Retry significance of a persistence error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreErrorKind {
    /// Operation may succeed unchanged later.
    Transient,
    /// Operation requires intervention or repair.
    Permanent,
}

/// Stable cause for a transient persistence failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreErrorReason {
    /// No narrower cause was supplied by the adapter.
    Unspecified,
    /// The database aborted a serializable transaction and requires a complete retry.
    SerializationFailure,
    /// The database selected this transaction as a deadlock victim.
    Deadlock,
    /// A leader-exclusive transaction presented a stale local fencing token.
    LeadershipLost,
}

/// Durable lifecycle of one local outbox operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxState {
    /// Appended atomically with the optimistic local mutation.
    Pending,
    /// Selected for an exchange. A restart must treat this state as replayable.
    Sending,
    /// The server acknowledged the authoritative logical effect.
    Acknowledged,
    /// The server permanently rejected the operation.
    Rejected,
    /// The operation requires application conflict handling.
    Conflict,
    /// Delivery or reconciliation failed before a terminal result was durable.
    Retry,
}

impl OutboxState {
    /// Returns whether a restart or later synchronization may safely resubmit the operation.
    #[must_use]
    pub const fn is_replayable(self) -> bool {
        matches!(self, Self::Pending | Self::Sending | Self::Retry)
    }

    /// Returns whether the server supplied a durable terminal result.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Acknowledged | Self::Rejected | Self::Conflict)
    }
}

/// Application decision recorded for a manual conflict.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictResolution {
    /// Keep the current authoritative state and abandon the optimistic operation.
    AcceptServer,
    /// A new domain operation supersedes the conflicted operation.
    SupersededBy(OperationId),
}

/// Conflict inbox entry with optional application resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictRecord {
    /// Server-provided conflict details.
    pub conflict: aequora_protocol::Conflict,
    /// Durable application decision, or `None` while unresolved.
    pub resolution: Option<ConflictResolution>,
}

/// Durable client queue statistics for UI status, metrics, and backpressure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OutboxStats {
    /// Operations not yet selected for delivery.
    pub pending: usize,
    /// Operations selected before a possible process restart.
    pub sending: usize,
    /// Operations awaiting replay after a failed attempt.
    pub retry: usize,
    /// Operations permanently rejected by authority and awaiting product inspection.
    pub rejected: usize,
    /// Oldest replayable operation timestamp.
    pub oldest_pending_at: Option<HybridTimestamp>,
}

/// Durable retry scheduling metadata for one replayable outbox operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryMetadata {
    /// Number of failed delivery/reconciliation attempts recorded for this operation.
    pub attempt_count: u32,
    /// Earliest Unix timestamp in milliseconds at which the operation may be selected again.
    pub next_attempt_unix_ms: u64,
}

impl OutboxStats {
    /// Total replayable queue depth.
    #[must_use]
    pub const fn replayable(self) -> usize {
        self.pending
            .saturating_add(self.sending)
            .saturating_add(self.retry)
    }
}

/// Read capability for authoritative entities.
#[async_trait]
pub trait EntityReader: Send + Sync {
    /// Reads one entity inside its tenant boundary.
    async fn read_entity(
        &self,
        tenant: TenantId,
        entity: EntityRef,
    ) -> Result<Option<EntitySnapshot>, StoreError>;
}

/// Authoritative journal pull capability.
#[async_trait]
pub trait ChangeJournal: Send + Sync {
    /// Oldest cursor from which retained incremental history remains complete.
    async fn minimum_retained_cursor(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
    ) -> Result<Sequence, StoreError>;

    /// Reads events after `sequence`, ordered ascending and restricted to tenant/scope.
    async fn read_changes_after(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        sequence: Sequence,
        limit: usize,
        max_payload_bytes: usize,
    ) -> Result<ChangePage, StoreError>;
}

/// Explicit synchronization-journal compaction capability.
#[async_trait]
pub trait JournalCompactor: Send + Sync {
    /// Removes sync events at or below a separately planned safe boundary.
    /// Operation-ledger and audit records must remain unaffected.
    async fn compact_journal(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        through: Sequence,
    ) -> Result<u64, StoreError>;
}

/// Atomic authoritative mutation, journal, and idempotency-ledger capability.
#[async_trait]
pub trait OperationLedger: Send + Sync {
    /// Returns a prior logical result, if any.
    async fn operation_result(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
    ) -> Result<Option<OperationAck>, StoreError>;

    /// Returns the original client/server operation lineage retained for retry validation.
    async fn operation_lineage(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
    ) -> Result<Option<LineageContext>, StoreError>;

    /// Atomically compares the expected version, requires `next_version` to advance exactly one,
    /// mutates entity state, appends exactly one journal/audit entry, and records the operation
    /// result. A repeated ID returns `Duplicate`.
    async fn commit_operation(&self, commit: CommitOperation) -> Result<CommitOutcome, StoreError>;
}

/// Immutable accountability capability, deliberately separate from the compactable sync journal.
#[async_trait]
pub trait AuditLog: Send + Sync {
    /// Reads tenant-bounded records strictly after `offset` in commit order.
    async fn read_audit_after(
        &self,
        tenant: TenantId,
        offset: AuditOffset,
        limit: usize,
    ) -> Result<AuditPage, StoreError>;
}

/// Tenant-bounded diagnostic lookup for one distributed causal chain.
#[async_trait]
pub trait CorrelationLog: Send + Sync {
    /// Reads audit records with exactly `correlation_id`, strictly after `offset`.
    async fn read_correlation(
        &self,
        tenant: TenantId,
        correlation_id: CorrelationId,
        offset: AuditOffset,
        limit: usize,
    ) -> Result<AuditPage, StoreError>;
}

/// Consistent, resumable authoritative snapshot capability.
#[async_trait]
pub trait SnapshotStore: Send + Sync {
    /// Atomically captures a snapshot boundary and immutable entity view for a partial scope.
    async fn create_snapshot(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        partitions: &[Partition],
    ) -> Result<SnapshotDescriptor, StoreError>;

    /// Reads a bounded page from a previously captured snapshot.
    async fn read_snapshot(
        &self,
        tenant: TenantId,
        snapshot_id: SnapshotId,
        offset: u64,
        max_entities: usize,
        max_payload_bytes: usize,
    ) -> Result<SnapshotPage, StoreError>;
}

/// Storage needed by an authoritative server.
pub trait AuthoritativeStore:
    EntityReader + ChangeJournal + OperationLedger + SnapshotStore + AuditLog + CorrelationLog
{
}
impl<T> AuthoritativeStore for T where
    T: EntityReader + ChangeJournal + OperationLedger + SnapshotStore + AuditLog + CorrelationLog
{
}

/// Durable client outbox capability.
#[async_trait]
pub trait OutboxStore: Send + Sync {
    /// Returns pending operations in stable enqueue order, bounded by `limit`.
    async fn pending_operations(&self, limit: usize) -> Result<Vec<OperationEnvelope>, StoreError>;

    /// Appends an operation. Real local adapters should call this within the same database
    /// transaction as the optimistic domain mutation.
    async fn append_operation(&self, operation: OperationEnvelope) -> Result<(), StoreError>;

    /// Runs one bounded, transactional compaction pass when supported by the adapter.
    /// Unsupported adapters safely return an unchanged empty report.
    async fn compact_outbox(
        &self,
        _registry: &OptimizationRegistry,
        _max_operations: usize,
    ) -> Result<QueueCompactionPlan, StoreError> {
        Ok(QueueCompactionPlan::default())
    }

    /// Runs compaction under a current leadership fence. Adapters without atomic fencing reject
    /// multi-process calls while preserving the existing single-process method.
    async fn compact_outbox_fenced(
        &self,
        registry: &OptimizationRegistry,
        max_operations: usize,
        fence: Option<LeaseGrant>,
    ) -> Result<QueueCompactionPlan, StoreError> {
        if fence.is_some() {
            return Err(StoreError::leadership_lost(
                "adapter does not implement fenced outbox compaction",
            ));
        }
        self.compact_outbox(registry, max_operations).await
    }

    /// Rebases never-sent operations after bootstrap, pull, or anti-entropy repair when supported.
    /// Unsupported adapters safely leave every operation unchanged.
    async fn rebase_outbox(
        &self,
        _registry: &OptimizationRegistry,
        _targets: &[RebaseTarget],
        _max_operations: usize,
    ) -> Result<RebasePlan, StoreError> {
        Ok(RebasePlan::default())
    }

    /// Rebases only while the supplied leadership epoch remains current.
    async fn rebase_outbox_fenced(
        &self,
        registry: &OptimizationRegistry,
        targets: &[RebaseTarget],
        max_operations: usize,
        fence: Option<LeaseGrant>,
    ) -> Result<RebasePlan, StoreError> {
        if fence.is_some() {
            return Err(StoreError::leadership_lost(
                "adapter does not implement fenced outbox rebase",
            ));
        }
        self.rebase_outbox(registry, targets, max_operations).await
    }
}

/// Durable outbox state-machine capability.
#[async_trait]
pub trait OutboxStateStore: Send + Sync {
    /// Marks selected operations as in flight before network I/O begins.
    async fn mark_sending(&self, operations: &[OperationId]) -> Result<(), StoreError>;

    /// Marks a batch in flight only while the supplied leadership epoch remains current.
    async fn mark_sending_fenced(
        &self,
        operations: &[OperationId],
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        if fence.is_some() {
            return Err(StoreError::leadership_lost(
                "adapter does not implement fenced outbox transitions",
            ));
        }
        self.mark_sending(operations).await
    }

    /// Marks operations replayable after delivery or reconciliation fails, increments their
    /// durable attempt count, and prevents selection before `next_attempt_unix_ms`.
    async fn mark_retry(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
    ) -> Result<(), StoreError>;

    /// Returns a delivered batch to retry only while the leadership epoch remains current.
    async fn mark_retry_fenced(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        if fence.is_some() {
            return Err(StoreError::leadership_lost(
                "adapter does not implement fenced retry transitions",
            ));
        }
        self.mark_retry(operations, next_attempt_unix_ms).await
    }

    /// Loads the durable retry schedule for diagnostics and restart-safe coordination.
    async fn retry_metadata(
        &self,
        operation: OperationId,
    ) -> Result<Option<RetryMetadata>, StoreError>;

    /// Loads the last durable state for application status and crash recovery.
    async fn operation_state(
        &self,
        operation: OperationId,
    ) -> Result<Option<OutboxState>, StoreError>;

    /// Returns durable queue depth and oldest replayable timestamp.
    async fn outbox_stats(&self) -> Result<OutboxStats, StoreError>;
}

/// Durable manual-conflict inbox independent of any UI framework.
#[async_trait]
pub trait ConflictInbox: Send + Sync {
    /// Returns unresolved conflicts in stable persistence order.
    async fn unresolved_conflicts(&self, limit: usize) -> Result<Vec<ConflictRecord>, StoreError>;

    /// Returns the durable unresolved-conflict count without loading details.
    async fn unresolved_conflict_count(&self) -> Result<usize, StoreError>;

    /// Records an application decision without inventing an authoritative mutation.
    async fn resolve_conflict(
        &self,
        operation: OperationId,
        resolution: ConflictResolution,
    ) -> Result<(), StoreError>;
}

/// Durable client cursor capability.
#[async_trait]
pub trait CursorStore: Send + Sync {
    /// Loads the last durably reconciled cursor for a scope.
    async fn load_cursor(&self, scope: SyncScopeId) -> Result<Option<Cursor>, StoreError>;
}

/// Atomic client reconciliation capability.
#[async_trait]
pub trait ReconciliationStore: Send + Sync {
    /// Atomically applies authoritative changes/tombstones, persists conflicts/rejections,
    /// acknowledges outbox operations, and advances the cursor last.
    async fn reconcile(&self, response: &SyncResponse) -> Result<(), StoreError>;

    /// Reconciles only if the supplied leadership epoch is current in the same transaction.
    async fn reconcile_fenced(
        &self,
        response: &SyncResponse,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        if fence.is_some() {
            return Err(StoreError::leadership_lost(
                "adapter does not implement fenced reconciliation",
            ));
        }
        self.reconcile(response).await
    }

    /// Durably stages a snapshot page. When `has_more` is false, the implementation must
    /// atomically replace the requested scope, set the snapshot cursor, and clear staging.
    async fn stage_snapshot(&self, response: &BootstrapResponse) -> Result<(), StoreError>;

    /// Stages/installs a snapshot only if the leadership epoch is current in the transaction.
    async fn stage_snapshot_fenced(
        &self,
        response: &BootstrapResponse,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        if fence.is_some() {
            return Err(StoreError::leadership_lost(
                "adapter does not implement fenced snapshot installation",
            ));
        }
        self.stage_snapshot(response).await
    }

    /// Returns a staged snapshot so bootstrap can resume after a process crash.
    async fn snapshot_progress(
        &self,
        scope: SyncScopeId,
    ) -> Result<Option<SnapshotProgress>, StoreError>;
}

/// Durable client progress for an incomplete bootstrap snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotProgress {
    /// Snapshot being staged.
    pub snapshot_id: SnapshotId,
    /// Snapshot boundary that will become the scope cursor at final commit.
    pub cursor: Cursor,
    /// Offset to request next.
    pub next_offset: u64,
}

/// Payload-free evidence returned after an atomic local replica repair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplicaRepairReport {
    /// Applied repair identity.
    pub repair_id: aequora_types::RepairId,
    /// Normal sync cursor before and after repair; the values must be identical.
    pub sync_cursor: Cursor,
    /// Replayable operation identities retained byte-for-byte by the adapter transaction.
    pub preserved_pending_operations: Vec<OperationId>,
}

/// Consistent canonical-state read implemented by authoritative adapters.
#[async_trait]
pub trait AuthoritativeIntegritySource: Send + Sync {
    /// Computes canonical state at exactly `boundary` or fails without returning a mixed view.
    async fn capture_authoritative_integrity(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        boundary: Cursor,
        generation: IntegrityGeneration,
        scheme: PartitionScheme,
        max_entities: usize,
    ) -> Result<IntegritySnapshot, StoreError>;
}

/// Canonical authoritative-base verification and atomic repair implemented by local adapters.
#[async_trait]
pub trait LocalIntegrityStore: Send + Sync {
    /// Computes canonical authoritative-base state, excluding provisional optimistic overlays.
    async fn capture_local_integrity(
        &self,
        scope: SyncScopeId,
        boundary: Cursor,
        generation: IntegrityGeneration,
        scheme: PartitionScheme,
        max_entities: usize,
    ) -> Result<IntegritySnapshot, StoreError>;

    /// Atomically installs authoritative replacements/removals while retaining pending intent and
    /// leaving the normal synchronization cursor unchanged.
    async fn repair_local_replica(
        &self,
        plan: &RepairPlan,
        replacements: &[CanonicalEntity],
        removals: &[EntityRef],
    ) -> Result<ReplicaRepairReport, StoreError>;

    /// Atomically validates leadership and applies one local anti-entropy repair.
    async fn repair_local_replica_fenced(
        &self,
        plan: &RepairPlan,
        replacements: &[CanonicalEntity],
        removals: &[EntityRef],
        fence: Option<LeaseGrant>,
    ) -> Result<ReplicaRepairReport, StoreError> {
        if fence.is_some() {
            return Err(StoreError::leadership_lost(
                "adapter does not implement fenced replica repair",
            ));
        }
        self.repair_local_replica(plan, replacements, removals)
            .await
    }
}

/// Durable database-neutral lease, fencing, and store-generation capability.
#[async_trait]
pub trait LocalCoordinationStore: Send + Sync {
    /// Declares whether independent processes can safely share this adapter.
    fn coordination_support(&self) -> LocalCoordinationSupport;

    /// Reads persistent store identity, generation, and current lease state.
    async fn coordination_snapshot(&self) -> Result<CoordinationSnapshot, StoreError>;

    /// Atomically acquires an absent/expired lease and increments its fencing token.
    async fn acquire_lease(&self, request: LeaseRequest) -> Result<LeaseGrant, StoreError>;

    /// Atomically renews only the current owner/token/generation.
    async fn renew_lease(
        &self,
        grant: LeaseGrant,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<LeaseGrant, StoreError>;

    /// Gracefully releases only the current owner/token/generation.
    async fn release_lease(&self, grant: LeaseGrant, now_unix_ms: u64) -> Result<(), StoreError>;

    /// Rejects a stale owner, token, lease kind, expiry, or store generation.
    async fn validate_fence(&self, grant: LeaseGrant, now_unix_ms: u64) -> Result<(), StoreError>;

    /// Atomically advances the store generation under a maintenance fence.
    async fn advance_store_generation(
        &self,
        grant: LeaseGrant,
        now_unix_ms: u64,
    ) -> Result<LocalStoreGeneration, StoreError>;

    /// Current fencing token for low-cost diagnostics.
    async fn current_fencing_token(&self) -> Result<FencingToken, StoreError> {
        self.coordination_snapshot()
            .await
            .map(|snapshot| snapshot.fencing_token)
    }
}

/// Optional durable local scope/subscription capability.
///
/// This remains separate from [`LocalStore`] so existing custom adapters retain source
/// compatibility. Deployments enabling dynamic scopes must require
/// [`AdapterCapabilities::SCOPE_STATE`] and this trait at construction.
#[async_trait]
pub trait ScopeStateStore: Send + Sync {
    /// Loads the complete payload-free subscription/membership control state.
    async fn load_scope_state(&self) -> Result<LocalScopeState, StoreError>;

    /// Installs a newly resolved subscription without activating unbootstrapped data.
    async fn install_subscription(&self, subscription: &Subscription) -> Result<(), StoreError>;

    /// Atomically applies membership, cursor/version, transition, and pending-intent disposition.
    async fn apply_scope_transition(
        &self,
        transition: &ScopeTransition,
    ) -> Result<ScopeTransitionOutcome, StoreError>;

    /// Applies a transition only while the supplied leadership epoch remains current.
    async fn apply_scope_transition_fenced(
        &self,
        transition: &ScopeTransition,
        fence: Option<LeaseGrant>,
    ) -> Result<ScopeTransitionOutcome, StoreError> {
        if fence.is_some() {
            return Err(StoreError::leadership_lost(
                "adapter does not implement fenced scope transitions",
            ));
        }
        self.apply_scope_transition(transition).await
    }
}

/// Storage needed by a local client engine.
pub trait LocalStore:
    OutboxStore + OutboxStateStore + ConflictInbox + CursorStore + ReconciliationStore
{
}
impl<T> LocalStore for T where
    T: OutboxStore + OutboxStateStore + ConflictInbox + CursorStore + ReconciliationStore
{
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPLETE_LOCAL: AdapterManifest = AdapterManifest {
        name: "test-local",
        adapter_version: env!("CARGO_PKG_VERSION"),
        tested_aequora_version: env!("CARGO_PKG_VERSION"),
        tested_database_versions: &["test-1"],
        roles: &[AdapterRole::LocalWritable, AdapterRole::SnapshotSink],
        tier: AdapterTier::FullProduction,
        capabilities: AdapterCapabilities::FULL_LOCAL,
        limitations: &[],
    };

    #[test]
    fn production_local_manifest_passes_startup_requirements() {
        assert_eq!(COMPLETE_LOCAL.validate(), Ok(()));
        assert_eq!(
            AdapterRequirements::PRODUCTION_LOCAL.verify(COMPLETE_LOCAL),
            Ok(())
        );
    }

    #[test]
    fn production_claim_fails_closed_when_atomic_coupling_is_missing() {
        let incomplete = AdapterManifest {
            capabilities: AdapterCapabilities::FULL_LOCAL,
            ..COMPLETE_LOCAL
        };
        let incomplete = AdapterManifest {
            capabilities: AdapterCapabilities(
                incomplete.capabilities.bits()
                    & !AdapterCapabilities::ATOMIC_METADATA_COUPLING.bits(),
            ),
            ..incomplete
        };
        assert_eq!(
            incomplete.validate(),
            Err(AdapterCompatibilityError::InvalidTierClaim {
                adapter: "test-local",
                role: AdapterRole::LocalWritable,
            })
        );
    }

    #[test]
    fn role_mismatch_is_a_typed_startup_error() {
        assert_eq!(
            AdapterRequirements::PRODUCTION_AUTHORITATIVE.verify(COMPLETE_LOCAL),
            Err(AdapterCompatibilityError::UnsupportedRole {
                adapter: "test-local",
                required: AdapterRole::AuthoritativeWritable,
            })
        );
    }

    #[test]
    fn tier_with_limitations_must_publish_them() {
        let incomplete = AdapterManifest {
            tier: AdapterTier::ProductionWithLimitations,
            capabilities: AdapterCapabilities::DURABLE_TRANSACTIONS,
            limitations: &[],
            ..COMPLETE_LOCAL
        };
        assert_eq!(
            incomplete.validate(),
            Err(AdapterCompatibilityError::MissingLimitations {
                adapter: "test-local"
            })
        );
    }

    #[test]
    fn production_pair_verifies_roles_independently() {
        let authority = AdapterManifest {
            name: "test-authority",
            roles: &[
                AdapterRole::AuthoritativeWritable,
                AdapterRole::SnapshotSource,
            ],
            capabilities: AdapterCapabilities::FULL_AUTHORITATIVE,
            ..COMPLETE_LOCAL
        };
        assert_eq!(
            ProductionAdapterPair::verify(COMPLETE_LOCAL, authority),
            Ok(ProductionAdapterPair {
                local: COMPLETE_LOCAL,
                authoritative: authority,
            })
        );
        assert_eq!(
            ProductionAdapterPair::verify(authority, COMPLETE_LOCAL),
            Err(AdapterCompatibilityError::UnsupportedRole {
                adapter: "test-authority",
                required: AdapterRole::LocalWritable,
            })
        );
    }
}
