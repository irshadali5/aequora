//! Logical indexes, access patterns, retention ownership, and adapter mapping evidence.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum LogicalRecord {
    MetadataRoot,
    LocalStore,
    Outbox,
    ScopeCursor,
    Subscription,
    ScopeDescriptor,
    EntityScopeRef,
    Conflict,
    BootstrapJob,
    BootstrapChunk,
    RepairJob,
    IntegrityNode,
    SchedulerState,
    CoordinatorLease,
    PurgeDirective,
    ClientKeyMetadata,
    AuthorityState,
    AuthorityTransition,
    OperationLedger,
    Journal,
    ScopeRegistry,
    ScopeMembership,
    Device,
    DeviceScopeWatermark,
    Snapshot,
    SnapshotChunk,
    SnapshotLease,
    AuditEvent,
    FieldProvenance,
    AuditCheckpoint,
    JournalCheckpoint,
    ImportJob,
    ImportRecord,
    ImportIdentityMap,
    ImportQuarantine,
    ExportJob,
    ReplayArtifact,
    RetentionPolicy,
    LegalHold,
    ErasureRequest,
    PurgeJob,
    ErasureLedger,
    StorageSurface,
    KeyRegistry,
    CryptoPolicy,
    CompatibilityPolicy,
    ReplicaWatermark,
    Job,
    JobLease,
    MetadataMigration,
    JournalFloor,
    RetiredEntity,
    OperationRecovery,
    SideEffectIntent,
    SideEffectResult,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum LogicalIndexId {
    OutboxOperationUnique,
    OutboxStateRetry,
    OutboxStatePrioritySequence,
    OutboxEntity,
    OutboxCompactionState,
    ScopeCursorIdentity,
    EntityScopeMembership,
    ConflictOperation,
    BootstrapChunkIdentity,
    BootstrapChunkOrder,
    AuthorityStateSingleton,
    LedgerOperationUnique,
    JournalTimelineOrder,
    JournalEventUnique,
    JournalTenantOrder,
    JournalEntityOrder,
    JournalOperation,
    ScopeRegistryIdentity,
    DeviceTenantStatus,
    DeviceScopeWatermark,
    SnapshotIdentity,
    SnapshotPublishedScope,
    SnapshotChunkIdentity,
    AuditTenantOrder,
    AuditSubject,
    ImportIdentitySource,
    JobRunnable,
    JobLeaseIdentity,
    MigrationIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalIndex {
    pub id: LogicalIndexId,
    pub record: LogicalRecord,
    pub fields: &'static [&'static str],
    pub unique: bool,
    pub required: bool,
}

macro_rules! index {
    ($id:ident, $record:ident, [$($field:literal),+], $unique:literal) => {
        LogicalIndex { id: LogicalIndexId::$id, record: LogicalRecord::$record,
            fields: &[$($field),+], unique: $unique, required: true }
    };
}

/// Minimum logical indexes every adapter must map or emulate for advertised records.
pub const REQUIRED_INDEXES: &[LogicalIndex] = &[
    index!(OutboxOperationUnique, Outbox, ["operation_id"], true),
    index!(OutboxStateRetry, Outbox, ["state", "next_retry_at"], false),
    index!(
        OutboxStatePrioritySequence,
        Outbox,
        ["state", "priority", "local_seq"],
        false
    ),
    index!(OutboxEntity, Outbox, ["entity_type", "entity_id"], false),
    index!(
        OutboxCompactionState,
        Outbox,
        ["compaction_key", "state"],
        false
    ),
    index!(ScopeCursorIdentity, ScopeCursor, ["scope_id"], true),
    index!(
        EntityScopeMembership,
        EntityScopeRef,
        ["entity_type", "entity_id", "scope_id"],
        true
    ),
    index!(ConflictOperation, Conflict, ["operation_id"], false),
    index!(
        BootstrapChunkIdentity,
        BootstrapChunk,
        ["bootstrap_job_id", "chunk_id"],
        true
    ),
    index!(
        BootstrapChunkOrder,
        BootstrapChunk,
        ["bootstrap_job_id", "ordinal"],
        true
    ),
    index!(AuthorityStateSingleton, AuthorityState, ["store_id"], true),
    index!(
        LedgerOperationUnique,
        OperationLedger,
        ["operation_id"],
        true
    ),
    index!(
        JournalTimelineOrder,
        Journal,
        ["authority_epoch", "sequence"],
        true
    ),
    index!(JournalEventUnique, Journal, ["event_id"], true),
    index!(
        JournalTenantOrder,
        Journal,
        ["tenant_id", "sequence"],
        false
    ),
    index!(
        JournalEntityOrder,
        Journal,
        ["entity_type", "entity_id", "sequence"],
        false
    ),
    index!(JournalOperation, Journal, ["operation_id"], false),
    index!(ScopeRegistryIdentity, ScopeRegistry, ["scope_id"], true),
    index!(DeviceTenantStatus, Device, ["tenant_id", "status"], false),
    index!(
        DeviceScopeWatermark,
        DeviceScopeWatermark,
        ["device_id", "scope_id"],
        true
    ),
    index!(SnapshotIdentity, Snapshot, ["snapshot_id"], true),
    index!(
        SnapshotPublishedScope,
        Snapshot,
        ["tenant_id", "scope_id", "state"],
        false
    ),
    index!(
        SnapshotChunkIdentity,
        SnapshotChunk,
        ["snapshot_id", "chunk_id"],
        true
    ),
    index!(
        AuditTenantOrder,
        AuditEvent,
        ["tenant_id", "audit_epoch", "audit_sequence"],
        true
    ),
    index!(
        AuditSubject,
        AuditEvent,
        ["tenant_id", "subject_kind", "subject_id"],
        false
    ),
    index!(
        ImportIdentitySource,
        ImportIdentityMap,
        ["job_id", "source_type", "source_key"],
        true
    ),
    index!(
        JobRunnable,
        Job,
        ["state", "next_run_at", "priority"],
        false
    ),
    index!(JobLeaseIdentity, JobLease, ["job_id"], true),
    index!(MigrationIdentity, MetadataMigration, ["migration_id"], true),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccessPattern {
    pub record: LogicalRecord,
    pub name: &'static str,
    pub index: LogicalIndexId,
    pub tenant_bounded: bool,
}

pub const ACCESS_PATTERNS: &[AccessPattern] = &[
    AccessPattern {
        record: LogicalRecord::Outbox,
        name: "dequeue pending",
        index: LogicalIndexId::OutboxStatePrioritySequence,
        tenant_bounded: false,
    },
    AccessPattern {
        record: LogicalRecord::Outbox,
        name: "retry due",
        index: LogicalIndexId::OutboxStateRetry,
        tenant_bounded: false,
    },
    AccessPattern {
        record: LogicalRecord::Outbox,
        name: "lookup operation",
        index: LogicalIndexId::OutboxOperationUnique,
        tenant_bounded: false,
    },
    AccessPattern {
        record: LogicalRecord::Journal,
        name: "scan timeline after sequence",
        index: LogicalIndexId::JournalTimelineOrder,
        tenant_bounded: true,
    },
    AccessPattern {
        record: LogicalRecord::Journal,
        name: "lookup event",
        index: LogicalIndexId::JournalEventUnique,
        tenant_bounded: true,
    },
    AccessPattern {
        record: LogicalRecord::Journal,
        name: "lookup operation",
        index: LogicalIndexId::JournalOperation,
        tenant_bounded: true,
    },
    AccessPattern {
        record: LogicalRecord::AuditEvent,
        name: "scan tenant audit chain",
        index: LogicalIndexId::AuditTenantOrder,
        tenant_bounded: true,
    },
    AccessPattern {
        record: LogicalRecord::AuditEvent,
        name: "lookup subject history",
        index: LogicalIndexId::AuditSubject,
        tenant_bounded: true,
    },
    AccessPattern {
        record: LogicalRecord::Job,
        name: "claim runnable jobs",
        index: LogicalIndexId::JobRunnable,
        tenant_bounded: false,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionRule {
    pub record: LogicalRecord,
    pub owner: &'static str,
    pub minimum_reason: &'static str,
    pub gc_precondition: &'static str,
    pub legal_hold_eligible: bool,
}

pub const RETENTION_MATRIX: &[RetentionRule] = &[
    RetentionRule {
        record: LogicalRecord::Outbox,
        owner: "client reconciliation",
        minimum_reason: "pending user intent",
        gc_precondition: "authoritative result is reflected locally",
        legal_hold_eligible: false,
    },
    RetentionRule {
        record: LogicalRecord::Journal,
        owner: "sync governance",
        minimum_reason: "active client resume and tombstone safety",
        gc_precondition: "watermarks, floor, snapshot, and policy allow deletion",
        legal_hold_eligible: true,
    },
    RetentionRule {
        record: LogicalRecord::OperationLedger,
        owner: "idempotency governance",
        minimum_reason: "legitimate retry horizon",
        gc_precondition: "retry horizon and audit policy elapsed",
        legal_hold_eligible: true,
    },
    RetentionRule {
        record: LogicalRecord::BootstrapChunk,
        owner: "bootstrap workflow",
        minimum_reason: "resumable installation",
        gc_precondition: "job terminal and staging no longer required",
        legal_hold_eligible: false,
    },
    RetentionRule {
        record: LogicalRecord::Conflict,
        owner: "conflict governance",
        minimum_reason: "unresolved user decision",
        gc_precondition: "resolved and retention elapsed",
        legal_hold_eligible: true,
    },
    RetentionRule {
        record: LogicalRecord::Snapshot,
        owner: "snapshot governance",
        minimum_reason: "bootstrap and journal-floor recovery",
        gc_precondition: "no lease and newer recovery point exists",
        legal_hold_eligible: true,
    },
    RetentionRule {
        record: LogicalRecord::AuditEvent,
        owner: "audit governance",
        minimum_reason: "accountability evidence",
        gc_precondition: "retention and holds allow lifecycle action",
        legal_hold_eligible: true,
    },
];

/// Evidence supplied by an adapter for one logical record mapping.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdapterRecordMapping {
    pub logical_record: LogicalRecord,
    pub physical_representation: String,
    pub logical_indexes: Vec<LogicalIndexId>,
    pub transaction_support: Vec<String>,
    pub retention_behavior: String,
}

/// Provider-neutral mapping declaration used by certification and generated docs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdapterMappingContract {
    pub adapter_name: String,
    pub records: Vec<AdapterRecordMapping>,
}

/// Logical metadata subsystems an adapter may advertise independently.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MetadataFeatures(u64);

impl MetadataFeatures {
    pub const OUTBOX: Self = Self(1 << 0);
    pub const CURSORS: Self = Self(1 << 1);
    pub const SUBSCRIPTIONS: Self = Self(1 << 2);
    pub const CONFLICTS: Self = Self(1 << 3);
    pub const BOOTSTRAP: Self = Self(1 << 4);
    pub const REPAIR: Self = Self(1 << 5);
    pub const SCOPE_MEMBERSHIP: Self = Self(1 << 6);
    pub const LEDGER: Self = Self(1 << 7);
    pub const JOURNAL: Self = Self(1 << 8);
    pub const DEVICES: Self = Self(1 << 9);
    pub const SNAPSHOTS: Self = Self(1 << 10);
    pub const AUDIT: Self = Self(1 << 11);
    pub const GOVERNANCE: Self = Self(1 << 12);
    pub const CRYPTO_REGISTRY: Self = Self(1 << 13);
    pub const COMPATIBILITY: Self = Self(1 << 14);
    pub const JOBS: Self = Self(1 << 15);
    pub const REGIONAL: Self = Self(1 << 16);
    pub const DIAGNOSTICS: Self = Self(1 << 17);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    #[must_use]
    pub const fn contains(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataStoreProfile {
    MinimalClient,
    StandardClient,
    FullClient,
    MinimalServer,
    EnterpriseServer,
}

impl MetadataStoreProfile {
    #[must_use]
    pub const fn required_features(self) -> MetadataFeatures {
        match self {
            Self::MinimalClient => MetadataFeatures::OUTBOX
                .union(MetadataFeatures::CURSORS)
                .union(MetadataFeatures::SUBSCRIPTIONS),
            Self::StandardClient => Self::MinimalClient
                .required_features()
                .union(MetadataFeatures::CONFLICTS)
                .union(MetadataFeatures::BOOTSTRAP)
                .union(MetadataFeatures::REPAIR)
                .union(MetadataFeatures::SCOPE_MEMBERSHIP),
            Self::FullClient => Self::StandardClient
                .required_features()
                .union(MetadataFeatures::AUDIT)
                .union(MetadataFeatures::DIAGNOSTICS),
            Self::MinimalServer => MetadataFeatures::LEDGER
                .union(MetadataFeatures::JOURNAL)
                .union(MetadataFeatures::SUBSCRIPTIONS)
                .union(MetadataFeatures::DEVICES),
            Self::EnterpriseServer => Self::MinimalServer
                .required_features()
                .union(MetadataFeatures::SNAPSHOTS)
                .union(MetadataFeatures::AUDIT)
                .union(MetadataFeatures::GOVERNANCE)
                .union(MetadataFeatures::CRYPTO_REGISTRY)
                .union(MetadataFeatures::COMPATIBILITY)
                .union(MetadataFeatures::JOBS)
                .union(MetadataFeatures::REGIONAL),
        }
    }
}

/// Behavioral evidence required from each advertised adapter capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterCertificationRequirement {
    pub name: &'static str,
    pub transaction_group: Option<crate::MetadataTransactionGroup>,
    pub applicable_feature: MetadataFeatures,
}

pub const ADAPTER_CERTIFICATION_REQUIREMENTS: &[AdapterCertificationRequirement] = &[
    AdapterCertificationRequirement {
        name: "atomic local mutation plus outbox",
        transaction_group: Some(crate::MetadataTransactionGroup::ClientMutationAndOutbox),
        applicable_feature: MetadataFeatures::OUTBOX,
    },
    AdapterCertificationRequirement {
        name: "atomic authoritative mutation journal ledger and audit",
        transaction_group: Some(crate::MetadataTransactionGroup::AuthoritativeCommit),
        applicable_feature: MetadataFeatures::LEDGER,
    },
    AdapterCertificationRequirement {
        name: "unique OperationId and payload mismatch rejection",
        transaction_group: None,
        applicable_feature: MetadataFeatures::LEDGER,
    },
    AdapterCertificationRequirement {
        name: "cursor atomic apply",
        transaction_group: Some(crate::MetadataTransactionGroup::ClientReconciliation),
        applicable_feature: MetadataFeatures::CURSORS,
    },
    AdapterCertificationRequirement {
        name: "lease fencing",
        transaction_group: None,
        applicable_feature: MetadataFeatures::JOBS,
    },
    AdapterCertificationRequirement {
        name: "snapshot staging and verified publication",
        transaction_group: Some(crate::MetadataTransactionGroup::SnapshotPublication),
        applicable_feature: MetadataFeatures::SNAPSHOTS,
    },
    AdapterCertificationRequirement {
        name: "retention scans",
        transaction_group: None,
        applicable_feature: MetadataFeatures::GOVERNANCE,
    },
    AdapterCertificationRequirement {
        name: "schema migration and downgrade refusal",
        transaction_group: None,
        applicable_feature: MetadataFeatures::DIAGNOSTICS,
    },
];

impl AdapterMappingContract {
    /// Verifies required index coverage for every logical record the adapter declares.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.adapter_name.trim().is_empty() {
            return Err("adapter name is blank");
        }
        for mapping in &self.records {
            if mapping.physical_representation.trim().is_empty()
                || mapping.retention_behavior.trim().is_empty()
            {
                return Err("adapter mapping is incomplete");
            }
            if REQUIRED_INDEXES
                .iter()
                .filter(|index| index.record == mapping.logical_record)
                .any(|required| !mapping.logical_indexes.contains(&required.id))
            {
                return Err("adapter omits a required logical index");
            }
        }
        Ok(())
    }
}
