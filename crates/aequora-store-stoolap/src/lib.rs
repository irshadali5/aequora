//! Stoolap local-store boundary and schema.
//!
//! Domain repositories should use the same Stoolap transaction for their optimistic write
//! and `aequora_outbox` insert. Reconciliation similarly remains one backend transaction.

pub mod backup;
pub mod bootstrap;
pub mod capabilities;
pub mod config;
pub mod conflict;
pub mod connection;
pub mod cursor;
pub mod domain_bridge;
pub mod entity_meta;
pub mod errors;
pub mod fencing;
pub mod health;
pub mod integrity;
pub mod migration;
pub mod outbox;
pub mod repair;
pub mod scheduler;
pub mod snapshot;
pub mod storage;

pub use capabilities::{
    STOOLAP_DESKTOP_LOCAL_FULL_PROFILE, STOOLAP_LOCAL_ADAPTER_IDENTITY, STOOLAP_LOCAL_CORE_PROFILE,
    STOOLAP_MOBILE_LOCAL_FULL_PROFILE,
};

use aequora_adapter_sdk as adapter_sdk;
use aequora_coordination::{
    CoordinationSnapshot, FencingToken, LeaseGrant, LeaseKind, LeaseRequest,
    LocalCoordinationSupport, LocalStoreGeneration, LocalStoreId, ProcessInstanceId,
};
use aequora_integrity::{
    CURRENT_HASH_SCHEMA, CanonicalEntity, IntegrityGeneration, IntegritySnapshot, IntegritySupport,
    PartitionScheme, RepairPlan, RepairStrategy,
};
use aequora_protocol::{
    BootstrapResponse, Conflict, OperationEnvelope, SnapshotEntity, SyncResponse,
};
use aequora_queue::{
    CompactionPlan as QueueCompactionPlan, LocalOperationSeq, MutationMutability,
    OptimizationRegistry, QueueEntry, RebasePlan, RebaseTarget, SupersessionReason,
    plan_compaction, plan_rebase, semantic_envelope_hash,
};
use aequora_scope::{LocalScopeState, ScopeTransition, ScopeTransitionOutcome, Subscription};
use aequora_store::{
    AdapterCapabilities, AdapterManifest, AdapterManifestProvider, AdapterRole, AdapterTier,
    ConflictInbox, ConflictRecord, ConflictResolution, CursorStore, IntegrityCapabilityProvider,
    LocalCoordinationStore, LocalIntegrityStore, OutboxState, OutboxStateStore, OutboxStats,
    OutboxStore, ReconciliationStore, ReplicaRepairReport, RetryMetadata, ScopeStateStore,
    SnapshotProgress, StoreError, TransactionCapabilities, TransactionCapabilityProvider,
};
use aequora_types::{
    AuthorityEpoch, AuthorityId, Cursor, DeviceId, EntityId, EntityRef, EntityType, EntityVersion,
    OperationId, Sequence, SnapshotId, SyncScopeId,
};
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    str::FromStr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use stoolap::{ApiTransaction, Database};

/// Portable Stoolap DDL for the local synchronization metadata tables.
pub const MIGRATION_0001: &str = r"
CREATE TABLE IF NOT EXISTS aequora_outbox (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    operation_id TEXT NOT NULL UNIQUE,
    enqueued_order INTEGER NOT NULL,
    envelope TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'sending', 'acknowledged', 'rejected', 'conflict', 'retry')),
    terminal_detail TEXT
);
CREATE INDEX IF NOT EXISTS aequora_outbox_pending_idx
    ON aequora_outbox (state, enqueued_order);

CREATE TABLE IF NOT EXISTS aequora_cursors (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    scope_id TEXT NOT NULL UNIQUE,
    sequence INTEGER NOT NULL CHECK (sequence >= 0)
);

CREATE TABLE IF NOT EXISTS aequora_applied_events (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    scope_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    UNIQUE (scope_id, sequence)
);

CREATE TABLE IF NOT EXISTS aequora_conflicts (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    operation_id TEXT NOT NULL UNIQUE,
    detail TEXT NOT NULL,
    resolved INTEGER NOT NULL DEFAULT 0,
    resolution_detail TEXT
);

CREATE TABLE IF NOT EXISTS aequora_local_entities (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    scope_id TEXT NOT NULL,
    entity_type INTEGER NOT NULL CHECK (entity_type > 0),
    entity_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    payload TEXT NOT NULL,
    tombstone INTEGER NOT NULL,
    provisional INTEGER NOT NULL DEFAULT 0,
    UNIQUE (scope_id, entity_type, entity_id)
);

CREATE TABLE IF NOT EXISTS aequora_snapshot_progress (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    scope_id TEXT NOT NULL UNIQUE,
    snapshot_id TEXT NOT NULL,
    cursor_sequence INTEGER NOT NULL CHECK (cursor_sequence >= 0),
    next_offset INTEGER NOT NULL CHECK (next_offset >= 0)
);

CREATE TABLE IF NOT EXISTS aequora_snapshot_staging (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    scope_id TEXT NOT NULL,
    entity_type INTEGER NOT NULL,
    entity_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    payload TEXT NOT NULL,
    tombstone INTEGER NOT NULL,
    provisional INTEGER NOT NULL DEFAULT 0,
    UNIQUE (scope_id, entity_type, entity_id)
);
";

/// Adds durable retry attempt/deadline state without rewriting published outbox rows.
pub const MIGRATION_0002: &str = r"
CREATE TABLE IF NOT EXISTS aequora_retry_schedule (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    operation_id TEXT NOT NULL UNIQUE,
    attempt_count INTEGER NOT NULL CHECK (attempt_count > 0),
    next_attempt_unix_ms INTEGER NOT NULL CHECK (next_attempt_unix_ms >= 0)
);
CREATE INDEX IF NOT EXISTS aequora_retry_schedule_due_idx
    ON aequora_retry_schedule (next_attempt_unix_ms, operation_id);
";

/// Adds the immutable-delivery boundary and transactional optimization evidence.
pub const MIGRATION_0003: &str = r"
ALTER TABLE aequora_outbox ADD COLUMN ever_sent INTEGER NOT NULL DEFAULT 0;
ALTER TABLE aequora_outbox ADD COLUMN immutable_hash TEXT;
CREATE INDEX IF NOT EXISTS aequora_outbox_mutable_idx
    ON aequora_outbox (state, ever_sent, enqueued_order);

CREATE TABLE IF NOT EXISTS aequora_supersession (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    old_operation_id TEXT NOT NULL UNIQUE,
    new_operation_id TEXT,
    reason TEXT NOT NULL,
    compacted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS aequora_rebase_history (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    operation_id TEXT NOT NULL,
    old_base_version INTEGER,
    new_base_version INTEGER,
    rebased_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
";

/// Adds persistent store identity, monotonic generation, and one durable fenced lease row.
pub const MIGRATION_0004: &str = r"
CREATE TABLE IF NOT EXISTS aequora_local_store (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    store_id TEXT NOT NULL UNIQUE,
    store_generation INTEGER NOT NULL CHECK (store_generation > 0),
    metadata_schema_version INTEGER NOT NULL CHECK (metadata_schema_version > 0)
);

CREATE TABLE IF NOT EXISTS aequora_coordinator_lease (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    owner_id TEXT,
    fencing_token INTEGER NOT NULL CHECK (fencing_token >= 0),
    expires_at_unix_ms INTEGER NOT NULL CHECK (expires_at_unix_ms >= 0),
    last_heartbeat_unix_ms INTEGER NOT NULL CHECK (last_heartbeat_unix_ms >= 0),
    lease_kind TEXT NOT NULL CHECK (lease_kind IN ('sync', 'maintenance'))
);
";

/// Adds one atomically replaced canonical scope-control state.
///
/// The encoded model contains subscription, cursor binding, membership references, applied
/// transition identities, and pending-intent quarantine without duplicating entity payloads.
pub const MIGRATION_0005: &str = r"
CREATE TABLE IF NOT EXISTS aequora_scope_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    encoded_state TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS aequora_scope_quarantine (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    operation_id TEXT NOT NULL UNIQUE,
    disposition TEXT NOT NULL CHECK (
        disposition IN ('authorization_lost', 'scope_revoked', 'scope_removed')
    )
);
";

/// Binds durable local cursors and snapshot staging to authority identity and epoch.
pub const MIGRATION_0006: &str = r"
ALTER TABLE aequora_cursors ADD COLUMN authority_id TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000';
ALTER TABLE aequora_cursors ADD COLUMN authority_epoch INTEGER NOT NULL DEFAULT 1;
ALTER TABLE aequora_snapshot_progress ADD COLUMN authority_id TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000';
ALTER TABLE aequora_snapshot_progress ADD COLUMN authority_epoch INTEGER NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS aequora_authority_trust (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    authority_id TEXT NOT NULL UNIQUE,
    highest_epoch INTEGER NOT NULL CHECK (highest_epoch > 0)
);
";

/// Completes the Part 38 identity, claim, bootstrap, repair, scheduler, and storage metadata.
pub const MIGRATION_0007: &str = r"
ALTER TABLE aequora_local_store ADD COLUMN device_id TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000';
ALTER TABLE aequora_local_store ADD COLUMN device_binding_generation INTEGER NOT NULL DEFAULT 1;
ALTER TABLE aequora_local_store ADD COLUMN created_at_unix_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE aequora_local_store ADD COLUMN last_opened_by_build TEXT NOT NULL DEFAULT '';

ALTER TABLE aequora_outbox ADD COLUMN payload_digest TEXT NOT NULL DEFAULT '';
ALTER TABLE aequora_outbox ADD COLUMN in_flight_owner TEXT;
ALTER TABLE aequora_outbox ADD COLUMN in_flight_since_unix_ms INTEGER;

CREATE TABLE IF NOT EXISTS aequora_outbox_dependency (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    operation_id TEXT NOT NULL,
    depends_on_operation_id TEXT NOT NULL,
    UNIQUE (operation_id, depends_on_operation_id)
);

CREATE TABLE IF NOT EXISTS aequora_entity_meta (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    entity_type INTEGER NOT NULL,
    entity_id TEXT NOT NULL,
    authoritative_version INTEGER,
    optimistic_generation INTEGER NOT NULL DEFAULT 0,
    tombstone_state TEXT NOT NULL DEFAULT 'live',
    last_authoritative_sequence INTEGER,
    scope_membership TEXT NOT NULL DEFAULT '',
    integrity_digest TEXT,
    UNIQUE (entity_type, entity_id)
);

CREATE TABLE IF NOT EXISTS aequora_bootstrap (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    scope_id TEXT NOT NULL UNIQUE,
    phase TEXT NOT NULL,
    bootstrap_generation INTEGER NOT NULL,
    authority_id TEXT,
    authority_epoch INTEGER,
    boundary_sequence INTEGER,
    failure_detail TEXT,
    updated_at_unix_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS aequora_snapshot_stage_meta (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    scope_id TEXT NOT NULL UNIQUE,
    snapshot_id TEXT NOT NULL,
    expected_bytes INTEGER NOT NULL,
    received_bytes INTEGER NOT NULL,
    manifest_digest TEXT NOT NULL,
    verified INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS aequora_integrity_checkpoint (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    scope_id TEXT NOT NULL UNIQUE,
    cursor_sequence INTEGER NOT NULL,
    digest TEXT NOT NULL,
    status TEXT NOT NULL,
    checked_at_unix_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS aequora_repair_state (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    scope_id TEXT NOT NULL UNIQUE,
    repair_id TEXT NOT NULL,
    phase TEXT NOT NULL,
    checkpoint TEXT,
    updated_at_unix_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS aequora_scheduler (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    next_attempt_unix_ms INTEGER NOT NULL DEFAULT 0,
    retry_after_unix_ms INTEGER NOT NULL DEFAULT 0,
    circuit_open_until_unix_ms INTEGER NOT NULL DEFAULT 0,
    data_budget_used_bytes INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS aequora_blob_meta (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    blob_id TEXT NOT NULL UNIQUE,
    digest TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    availability TEXT NOT NULL,
    pin_state TEXT NOT NULL,
    last_access_unix_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS aequora_conflict_status_idx ON aequora_conflicts (resolved, operation_id);
";

const MIGRATION_LEDGER_SQL: &str = r"
CREATE TABLE IF NOT EXISTS aequora_schema_migrations (
    row_id INTEGER PRIMARY KEY AUTO_INCREMENT,
    version INTEGER NOT NULL UNIQUE CHECK (version > 0),
    name TEXT NOT NULL,
    checksum TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
";

/// Latest Stoolap schema revision understood by this Aequora release.
pub const STOOLAP_SCHEMA_VERSION: u32 = 7;

/// Versioned role and capability declaration for the built-in Stoolap local adapter.
pub const STOOLAP_ADAPTER_MANIFEST: AdapterManifest = AdapterManifest {
    name: "stoolap",
    adapter_version: env!("CARGO_PKG_VERSION"),
    tested_aequora_version: env!("CARGO_PKG_VERSION"),
    tested_database_versions: &["0.4.0"],
    roles: &[
        AdapterRole::LocalWritable,
        AdapterRole::ReplicaSink,
        AdapterRole::SnapshotSink,
    ],
    tier: AdapterTier::FullProduction,
    capabilities: AdapterCapabilities::FULL_LOCAL,
    limitations: &[],
};

const STOOLAP_SDK_ROLES: &[adapter_sdk::AdapterRole] = &[
    adapter_sdk::AdapterRole::LocalReplicaStore,
    adapter_sdk::AdapterRole::SnapshotStore,
    adapter_sdk::AdapterRole::IntegrityStore,
    adapter_sdk::AdapterRole::FencingStore,
];

const STOOLAP_SDK_CAPABILITIES: &[adapter_sdk::AdapterCapability] = &[
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::ATOMIC_LOCAL_OUTBOX),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::COMPARE_AND_SWAP),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::OUTBOX),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::CURSOR),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::MIGRATIONS),
    adapter_sdk::AdapterCapability {
        id: adapter_sdk::CapabilityId::SNAPSHOT,
        version: adapter_sdk::CapabilityVersion::V1,
        level: adapter_sdk::CapabilityLevel::Snapshot(
            adapter_sdk::SnapshotLevel::AtomicGenerationSwap,
        ),
    },
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::FENCING),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::INTEGRITY),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::BACKUP),
];

/// Part 36 machine-readable manifest for the official Stoolap local adapter.
///
/// Mobile and desktop certification remains target-bound; this declaration cannot substitute for
/// a passing [`adapter_sdk::CertifiedEnvironment`] on the selected OS and architecture.
pub const STOOLAP_STORAGE_ADAPTER_MANIFEST: adapter_sdk::AdapterManifest =
    adapter_sdk::AdapterManifest {
        descriptor: adapter_sdk::AdapterDescriptor {
            adapter_id: adapter_sdk::AdapterId(0xae02),
            name: "aequora-stoolap-local",
            version: adapter_sdk::AdapterVersion::new(0, 1, 0),
            store_kind: adapter_sdk::StoreKind::Embedded,
        },
        roles: STOOLAP_SDK_ROLES,
        capabilities: STOOLAP_SDK_CAPABILITIES,
        support: adapter_sdk::AdapterSupport::Official,
        supported_engine_versions: &["Stoolap 0.4.0"],
        supported_targets: &["x86_64-unknown-linux-gnu"],
        known_limitations: &[
            "mobile and non-Linux desktop targets require their own certification artifacts",
            "single-writer scheduling is required",
        ],
        concurrency: adapter_sdk::ConcurrencyModel::SingleWriterMultiProcess,
        maintainer_owned: true,
        release_evidence_complete: true,
    };

#[derive(Clone, Copy)]
struct StoolapMigration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

const STOOLAP_MIGRATIONS: &[StoolapMigration] = &[
    StoolapMigration {
        version: 1,
        name: "initial_local_replica_schema",
        sql: MIGRATION_0001,
    },
    StoolapMigration {
        version: 2,
        name: "durable_retry_schedule",
        sql: MIGRATION_0002,
    },
    StoolapMigration {
        version: 3,
        name: "offline_queue_optimization",
        sql: MIGRATION_0003,
    },
    StoolapMigration {
        version: 4,
        name: "local_multiprocess_coordination",
        sql: MIGRATION_0004,
    },
    StoolapMigration {
        version: 5,
        name: "authorized_scope_state",
        sql: MIGRATION_0005,
    },
    StoolapMigration {
        version: 6,
        name: "authority_epoch_cursor_binding",
        sql: MIGRATION_0006,
    },
    StoolapMigration {
        version: 7,
        name: "part_38_local_replica_production_metadata",
        sql: MIGRATION_0007,
    },
];

struct AppliedMigration {
    version: i64,
    name: String,
    checksum: String,
}

/// Applied and expected Stoolap schema revisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoolapSchemaStatus {
    /// Highest migration recorded by the local database.
    pub applied_version: u32,
    /// Highest migration supported by this Aequora build.
    pub expected_version: u32,
}

/// Persistent physical-replica identity and its logical device binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoolapLocalIdentity {
    /// Identity of the physical database replica.
    pub local_store_id: LocalStoreId,
    /// Monotonic physical/logical store generation.
    pub store_generation: LocalStoreGeneration,
    /// Registered logical device currently bound to this store.
    pub device_id: DeviceId,
    /// Monotonic binding generation used for clone detection.
    pub device_binding_generation: u64,
    /// Installed Aequora metadata schema.
    pub metadata_schema_version: u32,
}

impl StoolapSchemaStatus {
    /// Whether the local database is ready for this Aequora build.
    #[must_use]
    pub const fn is_current(self) -> bool {
        self.applied_version == self.expected_version
    }
}

/// Concrete Stoolap backend with transactional outbox, reconciliation, and snapshot staging.
#[derive(Clone)]
pub struct StoolapDatabase {
    database: Database,
    projection_hook: Arc<dyn StoolapProjectionHook>,
}

/// Application-owned projection updates joined to Aequora's reconciliation transaction.
pub trait StoolapProjectionHook: Send + Sync {
    /// Applies one previously unseen authoritative change to application-owned local tables.
    /// Returning an error rolls back the entity, applied-event marker, cursor, and outbox state.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the application projection cannot be updated.
    fn apply_change(
        &self,
        transaction: &mut ApiTransaction,
        scope: SyncScopeId,
        change: &aequora_protocol::RemoteChange,
    ) -> Result<(), StoreError>;

    /// Starts replacement of application-owned projections from a complete authoritative
    /// snapshot. Implementations should remove only rows owned by `scope`. The call is part of
    /// the same transaction that installs Aequora's staged entities and cursor.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the existing scoped projection cannot be prepared.
    fn begin_snapshot(
        &self,
        _transaction: &mut ApiTransaction,
        _scope: SyncScopeId,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Applies one entity from a complete authoritative snapshot.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the snapshot entity cannot be projected.
    fn apply_snapshot_entity(
        &self,
        _transaction: &mut ApiTransaction,
        _scope: SyncScopeId,
        _entity: &SnapshotEntity,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Finishes application projection replacement before the snapshot transaction commits.
    ///
    /// # Errors
    ///
    /// Returns a storage error when application snapshot finalization fails.
    fn finish_snapshot(
        &self,
        _transaction: &mut ApiTransaction,
        _scope: SyncScopeId,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Starts an atomic authoritative-base repair for application-owned projections.
    ///
    /// # Errors
    ///
    /// Returns a storage error when application projection repair cannot start safely.
    fn begin_repair(
        &self,
        _transaction: &mut ApiTransaction,
        _plan: &RepairPlan,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Removes one stale application projection during repair.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the projection cannot be removed atomically.
    fn remove_repair_entity(
        &self,
        _transaction: &mut ApiTransaction,
        _scope: SyncScopeId,
        _entity: EntityRef,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    /// Installs one authoritative application projection during repair.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the projection cannot be installed atomically.
    fn apply_repair_entity(
        &self,
        transaction: &mut ApiTransaction,
        scope: SyncScopeId,
        entity: &SnapshotEntity,
    ) -> Result<(), StoreError> {
        self.apply_snapshot_entity(transaction, scope, entity)
    }

    /// Finishes application projection repair before the shared transaction commits.
    ///
    /// # Errors
    ///
    /// Returns a storage error when application projection repair cannot finish safely.
    fn finish_repair(
        &self,
        _transaction: &mut ApiTransaction,
        _plan: &RepairPlan,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct NoopStoolapProjectionHook;

impl StoolapProjectionHook for NoopStoolapProjectionHook {
    fn apply_change(
        &self,
        _transaction: &mut ApiTransaction,
        _scope: SyncScopeId,
        _change: &aequora_protocol::RemoteChange,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

impl TransactionCapabilityProvider for StoolapDatabase {
    fn transaction_capabilities(&self) -> TransactionCapabilities {
        TransactionCapabilities::FULL_LOCAL
    }
}

impl IntegrityCapabilityProvider for StoolapDatabase {
    fn integrity_support(&self) -> IntegritySupport {
        IntegritySupport::Full
    }
}

impl AdapterManifestProvider for StoolapDatabase {
    fn adapter_manifest(&self) -> AdapterManifest {
        STOOLAP_ADAPTER_MANIFEST
    }
}

fn lease_kind_name(kind: LeaseKind) -> &'static str {
    match kind {
        LeaseKind::SyncCoordinator => "sync",
        LeaseKind::Maintenance => "maintenance",
    }
}

#[async_trait]
impl LocalCoordinationStore for StoolapDatabase {
    fn coordination_support(&self) -> LocalCoordinationSupport {
        LocalCoordinationSupport::Full
    }

    async fn coordination_snapshot(&self) -> Result<CoordinationSnapshot, StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        let snapshot = coordination_snapshot_transaction(&mut transaction)?;
        transaction.commit().map_err(stoolap_error)?;
        Ok(snapshot)
    }

    async fn acquire_lease(&self, request: LeaseRequest) -> Result<LeaseGrant, StoreError> {
        let expires_at = request
            .expires_at()
            .map_err(|error| StoreError::permanent(error.to_string()))?;
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        let snapshot = coordination_snapshot_transaction(&mut transaction)?;
        if snapshot.is_active(request.now_unix_ms) {
            return Err(StoreError::leadership_lost(
                "Stoolap coordinator lease is held",
            ));
        }
        let token = snapshot
            .fencing_token
            .checked_next()
            .map_err(|error| StoreError::permanent(error.to_string()))?;
        let changed = transaction
            .execute(
                "UPDATE aequora_coordinator_lease SET owner_id=$1, fencing_token=$2, expires_at_unix_ms=$3, last_heartbeat_unix_ms=$4, lease_kind=$5 WHERE singleton=1 AND fencing_token=$6 AND (owner_id IS NULL OR expires_at_unix_ms <= $7)",
                (
                    request.owner_id.to_string(),
                    to_i64(token.0, "fencing token")?,
                    to_i64(expires_at, "lease expiry")?,
                    to_i64(request.now_unix_ms, "lease heartbeat")?,
                    lease_kind_name(request.kind),
                    to_i64(snapshot.fencing_token.0, "fencing token")?,
                    to_i64(request.now_unix_ms, "lease acquisition timestamp")?,
                ),
            )
            .map_err(stoolap_error)?;
        if changed != 1 {
            return Err(StoreError::leadership_lost(
                "Stoolap coordinator lease acquisition lost its compare-and-swap",
            ));
        }
        let grant = LeaseGrant {
            store_id: snapshot.store_id,
            owner_id: request.owner_id,
            fencing_token: token,
            store_generation: snapshot.store_generation,
            kind: request.kind,
            expires_at_unix_ms: expires_at,
        };
        transaction.commit().map_err(stoolap_error)?;
        Ok(grant)
    }

    async fn renew_lease(
        &self,
        grant: LeaseGrant,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<LeaseGrant, StoreError> {
        let expires_at = LeaseRequest {
            owner_id: grant.owner_id,
            kind: grant.kind,
            now_unix_ms,
            ttl_ms,
        }
        .expires_at()
        .map_err(|error| StoreError::permanent(error.to_string()))?;
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        validate_coordination_fence(&mut transaction, grant, now_unix_ms)?;
        let changed = transaction
            .execute(
                "UPDATE aequora_coordinator_lease SET expires_at_unix_ms=$1, last_heartbeat_unix_ms=$2 WHERE singleton=1 AND owner_id=$3 AND fencing_token=$4 AND expires_at_unix_ms > $2",
                (
                    to_i64(expires_at, "lease expiry")?,
                    to_i64(now_unix_ms, "lease heartbeat")?,
                    grant.owner_id.to_string(),
                    to_i64(grant.fencing_token.0, "fencing token")?,
                ),
            )
            .map_err(stoolap_error)?;
        if changed != 1 {
            return Err(StoreError::leadership_lost(
                "Stoolap lease renewal was fenced",
            ));
        }
        transaction.commit().map_err(stoolap_error)?;
        Ok(LeaseGrant {
            expires_at_unix_ms: expires_at,
            ..grant
        })
    }

    async fn release_lease(&self, grant: LeaseGrant, now_unix_ms: u64) -> Result<(), StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        validate_coordination_fence(&mut transaction, grant, now_unix_ms)?;
        let changed = transaction
            .execute(
                "UPDATE aequora_coordinator_lease SET owner_id=NULL, expires_at_unix_ms=0, last_heartbeat_unix_ms=$1 WHERE singleton=1 AND owner_id=$2 AND fencing_token=$3",
                (
                    to_i64(now_unix_ms, "lease release timestamp")?,
                    grant.owner_id.to_string(),
                    to_i64(grant.fencing_token.0, "fencing token")?,
                ),
            )
            .map_err(stoolap_error)?;
        if changed != 1 {
            return Err(StoreError::leadership_lost(
                "Stoolap lease release was fenced",
            ));
        }
        transaction.commit().map_err(stoolap_error)
    }

    async fn validate_fence(&self, grant: LeaseGrant, now_unix_ms: u64) -> Result<(), StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        validate_coordination_fence(&mut transaction, grant, now_unix_ms)?;
        transaction.commit().map_err(stoolap_error)
    }

    async fn advance_store_generation(
        &self,
        grant: LeaseGrant,
        now_unix_ms: u64,
    ) -> Result<LocalStoreGeneration, StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        validate_coordination_fence(&mut transaction, grant, now_unix_ms)?;
        if grant.kind != LeaseKind::Maintenance {
            return Err(StoreError::permanent(
                "Stoolap generation change requires a maintenance lease",
            ));
        }
        let generation = grant
            .store_generation
            .checked_next()
            .map_err(|error| StoreError::permanent(error.to_string()))?;
        let changed = transaction
            .execute(
                "UPDATE aequora_local_store SET store_generation=$1 WHERE singleton=1 AND store_generation=$2",
                (
                    to_i64(generation.0, "store generation")?,
                    to_i64(grant.store_generation.0, "store generation")?,
                ),
            )
            .map_err(stoolap_error)?;
        if changed != 1 {
            return Err(StoreError::leadership_lost(
                "Stoolap store generation advance was fenced",
            ));
        }
        transaction.commit().map_err(stoolap_error)?;
        Ok(generation)
    }
}

fn apply_stoolap_scope_transition(
    database: &StoolapDatabase,
    transition: &ScopeTransition,
    fence: Option<LeaseGrant>,
) -> Result<ScopeTransitionOutcome, StoreError> {
    let mut transaction = database.database.begin().map_err(stoolap_error)?;
    if let Some(grant) = fence {
        validate_coordination_fence(&mut transaction, grant, unix_time_millis())?;
    }
    let mut state = load_scope_state_transaction(&mut transaction)?;
    let outcome = state
        .apply(transition)
        .map_err(|error| StoreError::permanent(error.to_string()))?;
    if outcome.applied {
        store_scope_state_transaction(&mut transaction, &state)?;
        let disposition = quarantine_disposition(&transition.kind);
        for operation in &outcome.quarantined_operations {
            let changed = transaction
                .execute(
                    "UPDATE aequora_scope_quarantine SET disposition=$1 WHERE operation_id=$2",
                    (disposition, operation.to_string()),
                )
                .map_err(stoolap_error)?;
            if changed == 0 {
                transaction
                    .execute(
                        "INSERT INTO aequora_scope_quarantine (operation_id, disposition) VALUES ($1, $2)",
                        (operation.to_string(), disposition),
                    )
                    .map_err(stoolap_error)?;
            }
        }
    } else {
        // Incomplete staging changes lifecycle state even though membership is not activated.
        store_scope_state_transaction(&mut transaction, &state)?;
    }
    transaction.commit().map_err(stoolap_error)?;
    Ok(outcome)
}

#[async_trait]
impl ScopeStateStore for StoolapDatabase {
    async fn load_scope_state(&self) -> Result<LocalScopeState, StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        let state = load_scope_state_transaction(&mut transaction)?;
        transaction.commit().map_err(stoolap_error)?;
        Ok(state)
    }

    async fn install_subscription(&self, subscription: &Subscription) -> Result<(), StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        let mut state = load_scope_state_transaction(&mut transaction)?;
        state
            .install(subscription.clone())
            .map_err(|error| StoreError::permanent(error.to_string()))?;
        store_scope_state_transaction(&mut transaction, &state)?;
        transaction.commit().map_err(stoolap_error)
    }

    async fn apply_scope_transition(
        &self,
        transition: &ScopeTransition,
    ) -> Result<ScopeTransitionOutcome, StoreError> {
        apply_stoolap_scope_transition(self, transition, None)
    }

    async fn apply_scope_transition_fenced(
        &self,
        transition: &ScopeTransition,
        fence: Option<LeaseGrant>,
    ) -> Result<ScopeTransitionOutcome, StoreError> {
        apply_stoolap_scope_transition(self, transition, fence)
    }
}

impl StoolapDatabase {
    /// Opens a Stoolap DSN and installs Aequora metadata tables.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the database cannot open or migration SQL fails.
    pub fn open(dsn: &str) -> Result<Self, StoreError> {
        let database = Database::open(dsn).map_err(stoolap_error)?;
        let backend = Self {
            database,
            projection_hook: Arc::new(NoopStoolapProjectionHook),
        };
        backend.migrate()?;
        Ok(backend)
    }

    /// Creates a uniquely isolated in-memory database and installs the schema.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when Stoolap initialization or migration fails.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let database = Database::open_in_memory().map_err(stoolap_error)?;
        let backend = Self {
            database,
            projection_hook: Arc::new(NoopStoolapProjectionHook),
        };
        backend.migrate()?;
        Ok(backend)
    }

    /// Borrows the underlying database for application repository reads.
    #[must_use]
    pub const fn database(&self) -> &Database {
        &self.database
    }

    /// Reads the installed authoritative version for one scoped entity.
    ///
    /// Applications use this when constructing a new optimistic operation after bootstrap or
    /// reconciliation. Tombstones remain versioned entities and are therefore returned.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the stored version is invalid or cannot be read.
    pub fn entity_version(
        &self,
        scope: SyncScopeId,
        entity: EntityRef,
    ) -> Result<Option<EntityVersion>, StoreError> {
        self.database
            .query_opt::<i64, _>(
                "SELECT version FROM aequora_local_entities
                  WHERE scope_id=$1 AND entity_type=$2 AND entity_id=$3",
                (
                    scope.to_string(),
                    i64::from(entity.entity_type.get()),
                    entity.entity_id.to_string(),
                ),
            )
            .map_err(stoolap_error)?
            .map(|version| {
                EntityVersion::new(
                    u64::try_from(version)
                        .map_err(|_| StoreError::permanent("invalid local entity version"))?,
                )
                .map_err(|_| StoreError::permanent("invalid local entity version"))
            })
            .transpose()
    }

    /// Erases cached synchronization state for one revoked/replaced scope.
    ///
    /// # Errors
    ///
    /// Returns a storage error and rolls back every removal if any scoped table cannot be updated.
    pub fn erase_scope_cache(&self, scope: SyncScopeId) -> Result<(), StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        let scope = scope.to_string();
        for statement in [
            "DELETE FROM aequora_applied_events WHERE scope_id=$1",
            "DELETE FROM aequora_local_entities WHERE scope_id=$1",
            "DELETE FROM aequora_snapshot_staging WHERE scope_id=$1",
            "DELETE FROM aequora_snapshot_progress WHERE scope_id=$1",
            "DELETE FROM aequora_cursors WHERE scope_id=$1",
        ] {
            transaction
                .execute(statement, (&scope,))
                .map_err(stoolap_error)?;
        }
        transaction.commit().map_err(stoolap_error)
    }

    /// Discards commands and conflicts created by one revoked device.
    ///
    /// Device matching is performed against the decoded typed envelope. Other devices' rows are
    /// never selected, so a shared local store cannot erase unrelated pending work.
    ///
    /// # Errors
    ///
    /// Returns a storage error and preserves all rows when an envelope is corrupt or deletion
    /// cannot commit.
    pub fn discard_device_operations(
        &self,
        device: aequora_types::DeviceId,
    ) -> Result<u64, StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        let rows = transaction
            .query("SELECT envelope FROM aequora_outbox", ())
            .map_err(stoolap_error)?;
        let mut operation_ids = Vec::new();
        for row in rows {
            let row = row.map_err(stoolap_error)?;
            let envelope = row.get::<String>(0).map_err(stoolap_error)?;
            let operation: OperationEnvelope = decode(&envelope)?;
            if operation.device_id == device {
                operation_ids.push(operation.operation_id.to_string());
            }
        }
        for operation_id in &operation_ids {
            transaction
                .execute(
                    "DELETE FROM aequora_conflicts WHERE operation_id=$1",
                    (operation_id,),
                )
                .map_err(stoolap_error)?;
            transaction
                .execute(
                    "DELETE FROM aequora_outbox WHERE operation_id=$1",
                    (operation_id,),
                )
                .map_err(stoolap_error)?;
        }
        transaction.commit().map_err(stoolap_error)?;
        Ok(u64::try_from(operation_ids.len()).unwrap_or(u64::MAX))
    }

    /// Installs an application-owned projection hook before the backend begins synchronizing.
    #[must_use]
    pub fn with_projection_hook(mut self, hook: impl StoolapProjectionHook + 'static) -> Self {
        self.projection_hook = Arc::new(hook);
        self
    }

    /// Installs all missing local schema migrations and verifies recorded checksums.
    ///
    /// Stoolap's public transaction API permits DML but not DDL. Each published migration is
    /// therefore required to be idempotent: DDL is applied first, then its ledger row is committed
    /// in an ACID transaction. A crash between those steps safely replays the DDL on next open.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when migration SQL fails or the durable history is missing,
    /// rewritten, non-contiguous, or newer than this build.
    pub fn migrate(&self) -> Result<(), StoreError> {
        migrate_database(&self.database)
    }

    /// Reads and verifies the durable local migration ledger.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the ledger cannot be read or contains schema drift.
    pub fn schema_status(&self) -> Result<StoolapSchemaStatus, StoreError> {
        let history = load_migration_history(&self.database)?;
        let applied_version = verify_migration_history(&history)?;
        Ok(StoolapSchemaStatus {
            applied_version,
            expected_version: STOOLAP_SCHEMA_VERSION,
        })
    }

    /// Verifies local availability, schema compatibility, transaction start/rollback, and access
    /// to the outbox, applied-event, and cursor metadata used by synchronization.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the database cannot execute a query or its schema is stale.
    pub fn health_check(&self) -> Result<(), StoreError> {
        self.database
            .query_one::<i64, _>("SELECT 1", ())
            .map_err(stoolap_error)?;
        let status = self.schema_status()?;
        if !status.is_current() {
            return Err(StoreError::permanent(format!(
                "Stoolap schema is at version {}, but Aequora requires version {}",
                status.applied_version, status.expected_version
            )));
        }
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        transaction
            .execute("UPDATE aequora_outbox SET state = state WHERE 1 = 0", ())
            .map_err(stoolap_error)?;
        transaction
            .execute(
                "UPDATE aequora_applied_events SET sequence = sequence WHERE 1 = 0",
                (),
            )
            .map_err(stoolap_error)?;
        transaction
            .query("SELECT sequence FROM aequora_cursors WHERE 1 = 0", ())
            .map_err(stoolap_error)?;
        transaction
            .query(
                "SELECT attempt_count FROM aequora_retry_schedule WHERE 1 = 0",
                (),
            )
            .map_err(stoolap_error)?;
        transaction
            .query(
                "SELECT encoded_state FROM aequora_scope_state WHERE 1 = 0",
                (),
            )
            .map_err(stoolap_error)?;
        transaction
            .query(
                "SELECT operation_id FROM aequora_scope_quarantine WHERE 1 = 0",
                (),
            )
            .map_err(stoolap_error)?;
        drop(transaction);
        Ok(())
    }

    /// Reads the physical store identity and current logical device binding.
    ///
    /// # Errors
    ///
    /// Returns a storage error when identity metadata is missing, malformed, or out of range.
    pub fn local_identity(&self) -> Result<StoolapLocalIdentity, StoreError> {
        let row = self
            .database
            .query(
                "SELECT store_id, store_generation, device_id, device_binding_generation, metadata_schema_version FROM aequora_local_store WHERE singleton = 1",
                (),
            )
            .map_err(stoolap_error)?
            .next()
            .ok_or_else(|| StoreError::permanent("Stoolap local-store identity is missing"))?
            .map_err(stoolap_error)?;
        let store_id: String = row.get(0).map_err(stoolap_error)?;
        let store_generation: i64 = row.get(1).map_err(stoolap_error)?;
        let device_id: String = row.get(2).map_err(stoolap_error)?;
        let device_binding_generation: i64 = row.get(3).map_err(stoolap_error)?;
        let schema_version: i64 = row.get(4).map_err(stoolap_error)?;
        Ok(StoolapLocalIdentity {
            local_store_id: parse_id(&store_id, "local store ID")?,
            store_generation: LocalStoreGeneration(from_i64(store_generation, "store generation")?),
            device_id: parse_id(&device_id, "device ID")?,
            device_binding_generation: from_i64(
                device_binding_generation,
                "device binding generation",
            )?,
            metadata_schema_version: u32::try_from(schema_version)
                .map_err(|_| StoreError::permanent("invalid Stoolap metadata schema version"))?,
        })
    }

    /// Atomically changes the logical device binding and advances its generation.
    ///
    /// This is an explicit recovery/registration action; opening a copied store never performs it
    /// implicitly.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the expected generation is stale or the update cannot commit.
    pub fn rebind_device(
        &self,
        expected_generation: u64,
        device_id: DeviceId,
    ) -> Result<StoolapLocalIdentity, StoreError> {
        let next_generation = expected_generation
            .checked_add(1)
            .ok_or_else(|| StoreError::permanent("device binding generation exhausted"))?;
        let affected = self
            .database
            .execute(
                "UPDATE aequora_local_store SET device_id=$1, device_binding_generation=$2 WHERE singleton=1 AND device_binding_generation=$3",
                (
                    device_id.to_string(),
                    to_i64(next_generation, "device binding generation")?,
                    to_i64(expected_generation, "expected device binding generation")?,
                ),
            )
            .map_err(stoolap_error)?;
        if affected != 1 {
            return Err(StoreError::permanent(
                "stale Stoolap device binding generation",
            ));
        }
        self.local_identity()
    }

    /// Atomically claims a bounded eligible upload batch under the current coordinator fence.
    ///
    /// Claiming sets `ever_sent` before the operation leaves the process. Retries therefore retain
    /// the same operation identity and canonical semantic digest after crashes.
    ///
    /// # Errors
    ///
    /// Returns a storage error for an empty bound, stale fence, corrupt envelope/digest, or failed
    /// transaction.
    pub fn claim_operations(
        &self,
        owner: ProcessInstanceId,
        limit: usize,
        fence: LeaseGrant,
    ) -> Result<Vec<outbox::ClaimedOperation>, StoreError> {
        self.claim_operations_inner(owner, limit, Some(fence))
    }

    /// Claims a batch when the host explicitly guarantees exclusive single-process ownership.
    ///
    /// Multi-process desktop/agent deployments must use [`Self::claim_operations`] instead.
    ///
    /// # Errors
    ///
    /// Returns a storage error for an empty bound, corrupt envelope/digest, or failed transaction.
    pub fn claim_operations_single_process(
        &self,
        owner: ProcessInstanceId,
        limit: usize,
    ) -> Result<Vec<outbox::ClaimedOperation>, StoreError> {
        self.claim_operations_inner(owner, limit, None)
    }

    fn claim_operations_inner(
        &self,
        owner: ProcessInstanceId,
        limit: usize,
        fence: Option<LeaseGrant>,
    ) -> Result<Vec<outbox::ClaimedOperation>, StoreError> {
        if limit == 0 {
            return Err(StoreError::permanent(
                "Stoolap outbox claim limit must be greater than zero",
            ));
        }
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        if let Some(grant) = fence {
            validate_coordination_fence(&mut transaction, grant, unix_time_millis())?;
            if grant.owner_id != owner {
                return Err(StoreError::leadership_lost(
                    "claim owner does not match the current Stoolap lease",
                ));
            }
        }
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let now = to_i64(unix_time_millis(), "claim timestamp")?;
        let rows = transaction
            .query(
                "SELECT operation_id, envelope, payload_digest FROM aequora_outbox WHERE state IN ('pending', 'retry') AND operation_id NOT IN (SELECT operation_id FROM aequora_retry_schedule WHERE next_attempt_unix_ms > $1) AND operation_id NOT IN (SELECT operation_id FROM aequora_scope_quarantine) ORDER BY enqueued_order LIMIT $2",
                (now, limit),
            )
            .map_err(stoolap_error)?;
        let mut selected = Vec::new();
        for row in rows {
            let row = row.map_err(stoolap_error)?;
            selected.push((
                row.get::<String>(0).map_err(stoolap_error)?,
                row.get::<String>(1).map_err(stoolap_error)?,
                row.get::<String>(2).map_err(stoolap_error)?,
            ));
        }
        let mut claimed = Vec::with_capacity(selected.len());
        for (operation_id, encoded, stored_digest) in selected {
            let operation: OperationEnvelope = decode(&encoded)?;
            let digest = semantic_envelope_hash(&operation)
                .map_err(|error| StoreError::permanent(error.to_string()))?;
            if !stored_digest.is_empty() && decode_hash(&stored_digest)? != digest {
                return Err(StoreError::permanent(
                    "Stoolap outbox payload digest mismatch",
                ));
            }
            let digest_hex = hex::encode(digest);
            let affected = transaction
                .execute(
                    "UPDATE aequora_outbox SET state='sending', ever_sent=1, immutable_hash=$1, payload_digest=$1, in_flight_owner=$2, in_flight_since_unix_ms=$3 WHERE operation_id=$4 AND state IN ('pending', 'retry')",
                    (&digest_hex, owner.to_string(), now, &operation_id),
                )
                .map_err(stoolap_error)?;
            if affected != 1 {
                return Err(StoreError::transient(
                    "Stoolap outbox claim lost a concurrent candidate",
                ));
            }
            claimed.push(outbox::ClaimedOperation {
                operation,
                payload_digest: digest,
            });
        }
        transaction.commit().map_err(stoolap_error)?;
        Ok(claimed)
    }

    /// Returns stale in-flight claims to retry under the current coordinator fence.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the fence is stale or recovery cannot commit.
    pub fn recover_stale_claims(
        &self,
        stale_before_unix_ms: u64,
        fence: LeaseGrant,
    ) -> Result<u64, StoreError> {
        self.recover_stale_claims_inner(stale_before_unix_ms, Some(fence))
    }

    /// Recovers stale claims under an explicit exclusive single-process host guarantee.
    ///
    /// # Errors
    ///
    /// Returns a storage error when recovery cannot commit.
    pub fn recover_stale_claims_single_process(
        &self,
        stale_before_unix_ms: u64,
    ) -> Result<u64, StoreError> {
        self.recover_stale_claims_inner(stale_before_unix_ms, None)
    }

    fn recover_stale_claims_inner(
        &self,
        stale_before_unix_ms: u64,
        fence: Option<LeaseGrant>,
    ) -> Result<u64, StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        if let Some(grant) = fence {
            validate_coordination_fence(&mut transaction, grant, unix_time_millis())?;
        }
        let affected = transaction
            .execute(
                "UPDATE aequora_outbox SET state='retry', in_flight_owner=NULL, in_flight_since_unix_ms=NULL WHERE state='sending' AND in_flight_since_unix_ms <= $1",
                (to_i64(stale_before_unix_ms, "stale claim timestamp")?,),
            )
            .map_err(stoolap_error)?;
        transaction.commit().map_err(stoolap_error)?;
        u64::try_from(affected)
            .map_err(|_| StoreError::permanent("negative stale Stoolap claim count"))
    }

    /// Persists only scheduler data required across process restarts.
    ///
    /// # Errors
    ///
    /// Returns a storage error when a timestamp exceeds the engine range or the checkpoint fails.
    pub fn store_scheduler_checkpoint(
        &self,
        checkpoint: scheduler::SchedulerCheckpoint,
    ) -> Result<(), StoreError> {
        self.database
            .execute(
                "UPDATE aequora_scheduler SET next_attempt_unix_ms=$1, retry_after_unix_ms=$2, circuit_open_until_unix_ms=$3, data_budget_used_bytes=$4 WHERE singleton=1",
                (
                    to_i64(checkpoint.next_attempt_unix_ms, "scheduler next attempt")?,
                    to_i64(checkpoint.retry_after_unix_ms, "scheduler Retry-After")?,
                    to_i64(checkpoint.circuit_open_until_unix_ms, "scheduler circuit deadline")?,
                    to_i64(checkpoint.data_budget_used_bytes, "scheduler data budget")?,
                ),
            )
            .map_err(stoolap_error)?;
        Ok(())
    }

    /// Loads restart-safe scheduler state.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the singleton checkpoint is missing or malformed.
    pub fn scheduler_checkpoint(&self) -> Result<scheduler::SchedulerCheckpoint, StoreError> {
        let row = self
            .database
            .query(
                "SELECT next_attempt_unix_ms, retry_after_unix_ms, circuit_open_until_unix_ms, data_budget_used_bytes FROM aequora_scheduler WHERE singleton=1",
                (),
            )
            .map_err(stoolap_error)?
            .next()
            .ok_or_else(|| StoreError::permanent("Stoolap scheduler checkpoint is missing"))?
            .map_err(stoolap_error)?;
        Ok(scheduler::SchedulerCheckpoint {
            next_attempt_unix_ms: from_i64(
                row.get(0).map_err(stoolap_error)?,
                "scheduler next attempt",
            )?,
            retry_after_unix_ms: from_i64(
                row.get(1).map_err(stoolap_error)?,
                "scheduler Retry-After",
            )?,
            circuit_open_until_unix_ms: from_i64(
                row.get(2).map_err(stoolap_error)?,
                "scheduler circuit deadline",
            )?,
            data_budget_used_bytes: from_i64(
                row.get(3).map_err(stoolap_error)?,
                "scheduler data budget",
            )?,
        })
    }

    /// Produces a structured payload-free health snapshot without deleting damaged state.
    ///
    /// # Errors
    ///
    /// Returns a storage error when basic schema or write probing fails.
    pub fn structured_health(&self) -> Result<health::StoolapHealth, StoreError> {
        self.health_check()?;
        Ok(health::StoolapHealth {
            store: health::HealthCheckState::Ready,
            schema: health::HealthCheckState::Ready,
            writes: health::HealthCheckState::Ready,
            storage: health::HealthCheckState::Ready,
            migration: health::MigrationHealth::Current,
            integrity: health::IntegrityHealth::VerificationPending,
            leader: health::LeaderHealth::CurrentOrFollowerSafe,
        })
    }

    /// Runs an optimistic domain mutation and outbox append in the same Stoolap transaction.
    ///
    /// # Errors
    ///
    /// Rolls back and returns [`StoreError`] if the application mutation, outbox encoding,
    /// insert, or commit fails.
    pub fn transact_local_mutation<F>(
        &self,
        operation: &OperationEnvelope,
        mutate: F,
    ) -> Result<(), StoreError>
    where
        F: FnOnce(&mut ApiTransaction) -> Result<(), StoreError>,
    {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        mutate(&mut transaction)?;
        insert_outbox(&mut transaction, operation)?;
        transaction.commit().map_err(stoolap_error)
    }
}

fn migrate_database(database: &Database) -> Result<(), StoreError> {
    database
        .execute(MIGRATION_LEDGER_SQL, ())
        .map_err(stoolap_error)?;
    let history = load_migration_history(database)?;
    verify_migration_history(&history)?;

    for migration in STOOLAP_MIGRATIONS {
        if history
            .iter()
            .any(|applied| applied.version == i64::from(migration.version))
        {
            continue;
        }
        apply_idempotent_migration_ddl(database, migration.sql)?;
        let mut transaction = database.begin().map_err(stoolap_error)?;
        if transaction
            .query_opt::<i64, _>(
                "SELECT version FROM aequora_schema_migrations WHERE version = $1",
                (i64::from(migration.version),),
            )
            .map_err(stoolap_error)?
            .is_some()
        {
            transaction.commit().map_err(stoolap_error)?;
            continue;
        }
        transaction
            .execute(
                "INSERT INTO aequora_schema_migrations (version, name, checksum) VALUES ($1, $2, $3)",
                (
                    i64::from(migration.version),
                    migration.name,
                    migration_checksum(migration.sql),
                ),
            )
            .map_err(stoolap_error)?;
        transaction.commit().map_err(stoolap_error)?;
    }
    let final_history = load_migration_history(database)?;
    let applied = verify_migration_history(&final_history)?;
    if applied != STOOLAP_SCHEMA_VERSION {
        return Err(StoreError::permanent(format!(
            "Stoolap schema migration stopped at version {applied}, expected {STOOLAP_SCHEMA_VERSION}"
        )));
    }
    ensure_coordination_state(database)?;
    Ok(())
}

fn apply_idempotent_migration_ddl(database: &Database, sql: &str) -> Result<(), StoreError> {
    for statement in sql
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let tokens = statement.split_whitespace().collect::<Vec<_>>();
        if tokens.len() >= 6
            && tokens[0].eq_ignore_ascii_case("ALTER")
            && tokens[1].eq_ignore_ascii_case("TABLE")
            && tokens[3].eq_ignore_ascii_case("ADD")
            && tokens[4].eq_ignore_ascii_case("COLUMN")
            && stoolap_column_exists(database, tokens[2], tokens[5])?
        {
            continue;
        }
        database.execute(statement, ()).map_err(stoolap_error)?;
    }
    Ok(())
}

fn stoolap_column_exists(
    database: &Database,
    table: &str,
    column: &str,
) -> Result<bool, StoreError> {
    let rows = database
        .query(&format!("DESCRIBE {table}"), ())
        .map_err(stoolap_error)?;
    for row in rows {
        let row = row.map_err(stoolap_error)?;
        let field: String = row.get(0).map_err(stoolap_error)?;
        if field.eq_ignore_ascii_case(column) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ensure_coordination_state(database: &Database) -> Result<(), StoreError> {
    let mut transaction = database.begin().map_err(stoolap_error)?;
    if transaction
        .query_opt::<String, _>(
            "SELECT store_id FROM aequora_local_store WHERE singleton = 1",
            (),
        )
        .map_err(stoolap_error)?
        .is_none()
    {
        transaction
            .execute(
                "INSERT INTO aequora_local_store (singleton, store_id, store_generation, metadata_schema_version) VALUES (1, $1, 1, $2)",
                (LocalStoreId::new().to_string(), i64::from(STOOLAP_SCHEMA_VERSION)),
            )
            .map_err(stoolap_error)?;
    }
    let device_id = transaction
        .query_one::<String, _>(
            "SELECT device_id FROM aequora_local_store WHERE singleton = 1",
            (),
        )
        .map_err(stoolap_error)?;
    if device_id == "00000000-0000-0000-0000-000000000000" {
        transaction
            .execute(
                "UPDATE aequora_local_store SET device_id=$1, created_at_unix_ms=$2 WHERE singleton=1",
                (
                    DeviceId::new().to_string(),
                    to_i64(unix_time_millis(), "store creation timestamp")?,
                ),
            )
            .map_err(stoolap_error)?;
    }
    transaction
        .execute(
            "UPDATE aequora_local_store SET metadata_schema_version=$1, last_opened_by_build=$2 WHERE singleton=1",
            (i64::from(STOOLAP_SCHEMA_VERSION), env!("CARGO_PKG_VERSION")),
        )
        .map_err(stoolap_error)?;
    if transaction
        .query_opt::<i64, _>(
            "SELECT fencing_token FROM aequora_coordinator_lease WHERE singleton = 1",
            (),
        )
        .map_err(stoolap_error)?
        .is_none()
    {
        transaction
            .execute(
                "INSERT INTO aequora_coordinator_lease (singleton, owner_id, fencing_token, expires_at_unix_ms, last_heartbeat_unix_ms, lease_kind) VALUES (1, NULL, 0, 0, 0, 'sync')",
                (),
            )
            .map_err(stoolap_error)?;
    }
    if transaction
        .query_opt::<i64, _>(
            "SELECT singleton FROM aequora_scheduler WHERE singleton = 1",
            (),
        )
        .map_err(stoolap_error)?
        .is_none()
    {
        transaction
            .execute("INSERT INTO aequora_scheduler (singleton) VALUES (1)", ())
            .map_err(stoolap_error)?;
    }
    let rows = transaction
        .query(
            "SELECT operation_id, envelope FROM aequora_outbox WHERE payload_digest = ''",
            (),
        )
        .map_err(stoolap_error)?;
    let mut missing_digests = Vec::new();
    for row in rows {
        let row = row.map_err(stoolap_error)?;
        missing_digests.push((
            row.get::<String>(0).map_err(stoolap_error)?,
            row.get::<String>(1).map_err(stoolap_error)?,
        ));
    }
    for (operation_id, encoded) in missing_digests {
        let operation: OperationEnvelope = decode(&encoded)?;
        let digest = semantic_envelope_hash(&operation)
            .map_err(|error| StoreError::permanent(error.to_string()))?;
        transaction
            .execute(
                "UPDATE aequora_outbox SET payload_digest=$1 WHERE operation_id=$2",
                (hex::encode(digest), operation_id),
            )
            .map_err(stoolap_error)?;
    }
    transaction.commit().map_err(stoolap_error)
}

fn coordination_snapshot_transaction(
    transaction: &mut ApiTransaction,
) -> Result<CoordinationSnapshot, StoreError> {
    let store_row = transaction
        .query(
            "SELECT store_id, store_generation FROM aequora_local_store WHERE singleton = 1",
            (),
        )
        .map_err(stoolap_error)?
        .next()
        .ok_or_else(|| StoreError::permanent("Stoolap local-store identity is missing"))?
        .map_err(stoolap_error)?;
    let lease_row = transaction
        .query(
            "SELECT owner_id, fencing_token, expires_at_unix_ms, lease_kind FROM aequora_coordinator_lease WHERE singleton = 1",
            (),
        )
        .map_err(stoolap_error)?
        .next()
        .ok_or_else(|| StoreError::permanent("Stoolap coordinator lease row is missing"))?
        .map_err(stoolap_error)?;
    let store_id: String = store_row.get(0).map_err(stoolap_error)?;
    let generation: i64 = store_row.get(1).map_err(stoolap_error)?;
    let owner_id: Option<String> = lease_row.get(0).map_err(stoolap_error)?;
    let fencing_token: i64 = lease_row.get(1).map_err(stoolap_error)?;
    let expires_at_unix_ms: i64 = lease_row.get(2).map_err(stoolap_error)?;
    let lease_kind: String = lease_row.get(3).map_err(stoolap_error)?;
    Ok(CoordinationSnapshot {
        store_id: parse_id(&store_id, "local store ID")?,
        store_generation: LocalStoreGeneration(from_i64(generation, "store generation")?),
        fencing_token: FencingToken(from_i64(fencing_token, "fencing token")?),
        owner_id: owner_id
            .map(|value| parse_id(&value, "process instance ID"))
            .transpose()?,
        kind: match lease_kind.as_str() {
            "sync" => LeaseKind::SyncCoordinator,
            "maintenance" => LeaseKind::Maintenance,
            _ => {
                return Err(StoreError::permanent(
                    "invalid Stoolap coordinator lease kind",
                ));
            }
        },
        expires_at_unix_ms: from_i64(expires_at_unix_ms, "lease expiry")?,
    })
}

fn validate_coordination_fence(
    transaction: &mut ApiTransaction,
    grant: LeaseGrant,
    now_unix_ms: u64,
) -> Result<(), StoreError> {
    coordination_snapshot_transaction(transaction)?
        .validate(grant, now_unix_ms)
        .map_err(|error| StoreError::leadership_lost(error.to_string()))
}

fn load_migration_history(database: &Database) -> Result<Vec<AppliedMigration>, StoreError> {
    let rows = database
        .query(
            "SELECT version, name, checksum FROM aequora_schema_migrations ORDER BY version",
            (),
        )
        .map_err(stoolap_error)?;
    decode_migration_rows(rows)
}

fn decode_migration_rows(rows: stoolap::api::Rows) -> Result<Vec<AppliedMigration>, StoreError> {
    rows.map(|row| {
        let row = row.map_err(stoolap_error)?;
        Ok(AppliedMigration {
            version: row.get(0).map_err(stoolap_error)?,
            name: row.get(1).map_err(stoolap_error)?,
            checksum: row.get(2).map_err(stoolap_error)?,
        })
    })
    .collect()
}

fn migration_checksum(sql: &str) -> String {
    blake3::hash(sql.as_bytes()).to_hex().to_string()
}

fn verify_migration_history(history: &[AppliedMigration]) -> Result<u32, StoreError> {
    let mut previous = 0_u32;
    for applied in history {
        let version = u32::try_from(applied.version)
            .map_err(|_| StoreError::permanent("negative Stoolap schema migration version"))?;
        if version != previous.saturating_add(1) {
            return Err(StoreError::permanent(format!(
                "Stoolap migration history is not contiguous at version {version}"
            )));
        }
        let Some(migration) = STOOLAP_MIGRATIONS
            .iter()
            .find(|migration| migration.version == version)
        else {
            return Err(StoreError::permanent(format!(
                "Stoolap schema version {version} is newer than supported version {STOOLAP_SCHEMA_VERSION}"
            )));
        };
        verify_migration_record(*migration, &applied.name, &applied.checksum)?;
        previous = version;
    }
    Ok(previous)
}

fn verify_migration_record(
    migration: StoolapMigration,
    applied_name: &str,
    applied_checksum: &str,
) -> Result<(), StoreError> {
    if applied_name != migration.name {
        return Err(StoreError::permanent(format!(
            "Stoolap migration {} name drift: expected {}, found {applied_name}",
            migration.version, migration.name
        )));
    }
    if applied_checksum != migration_checksum(migration.sql) {
        return Err(StoreError::permanent(format!(
            "Stoolap migration {} checksum drift detected",
            migration.version
        )));
    }
    Ok(())
}

fn insert_outbox(
    transaction: &mut ApiTransaction,
    operation: &OperationEnvelope,
) -> Result<(), StoreError> {
    let operation_id = operation.operation_id.to_string();
    if transaction
        .query_opt::<i64, _>(
            "SELECT enqueued_order FROM aequora_outbox WHERE operation_id = $1",
            (&operation_id,),
        )
        .map_err(stoolap_error)?
        .is_some()
    {
        return Ok(());
    }
    let next_order = transaction
        .query_one::<i64, _>(
            "SELECT COALESCE(MAX(enqueued_order), 0) + 1 FROM aequora_outbox",
            (),
        )
        .map_err(stoolap_error)?;
    let envelope = encode(operation)?;
    let payload_digest = hex::encode(
        semantic_envelope_hash(operation)
            .map_err(|error| StoreError::permanent(error.to_string()))?,
    );
    transaction
        .execute(
            "INSERT INTO aequora_outbox (operation_id, enqueued_order, envelope, state, payload_digest) VALUES ($1, $2, $3, 'pending', $4)",
            (&operation_id, next_order, &envelope, &payload_digest),
        )
        .map_err(stoolap_error)?;
    Ok(())
}

fn encode<T: Serialize>(value: &T) -> Result<String, StoreError> {
    postcard::to_stdvec(value)
        .map(hex::encode)
        .map_err(|error| StoreError::permanent(format!("Stoolap value encoding failed: {error}")))
}

fn decode<T: DeserializeOwned>(value: &str) -> Result<T, StoreError> {
    let bytes = hex::decode(value)
        .map_err(|error| StoreError::permanent(format!("Stoolap hex value is corrupt: {error}")))?;
    postcard::from_bytes(&bytes).map_err(|error| {
        StoreError::permanent(format!("Stoolap Postcard value is corrupt: {error}"))
    })
}

fn load_scope_state_transaction(
    transaction: &mut ApiTransaction,
) -> Result<LocalScopeState, StoreError> {
    transaction
        .query_opt::<String, _>(
            "SELECT encoded_state FROM aequora_scope_state WHERE singleton = 1",
            (),
        )
        .map_err(stoolap_error)?
        .map_or_else(
            || Ok(LocalScopeState::default()),
            |encoded| decode(&encoded),
        )
}

fn store_scope_state_transaction(
    transaction: &mut ApiTransaction,
    state: &LocalScopeState,
) -> Result<(), StoreError> {
    let encoded = encode(state)?;
    let updated = transaction
        .execute(
            "UPDATE aequora_scope_state SET encoded_state = $1 WHERE singleton = 1",
            (&encoded,),
        )
        .map_err(stoolap_error)?;
    if updated == 0 {
        transaction
            .execute(
                "INSERT INTO aequora_scope_state (singleton, encoded_state) VALUES (1, $1)",
                (&encoded,),
            )
            .map_err(stoolap_error)?;
    }
    Ok(())
}

fn quarantine_disposition(kind: &aequora_scope::ScopeTransitionKind) -> &'static str {
    match kind {
        aequora_scope::ScopeTransitionKind::Revocation => "scope_revoked",
        aequora_scope::ScopeTransitionKind::Contraction { .. } => "scope_removed",
        _ => "authorization_lost",
    }
}

fn stoolap_error(error: impl std::fmt::Display) -> StoreError {
    StoreError::transient(format!("Stoolap operation failed: {error}"))
}

fn parse_id<T>(value: &str, field: &str) -> Result<T, StoreError>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    value.parse().map_err(|error| {
        StoreError::permanent(format!("invalid {field} in Stoolap storage: {error}"))
    })
}

fn to_i64(value: u64, field: &str) -> Result<i64, StoreError> {
    i64::try_from(value)
        .map_err(|_| StoreError::permanent(format!("{field} exceeds Stoolap INTEGER range")))
}

fn from_i64(value: i64, field: &str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::permanent(format!("negative Stoolap {field}")))
}

fn state_name(state: OutboxState) -> &'static str {
    match state {
        OutboxState::Pending => "pending",
        OutboxState::Sending => "sending",
        OutboxState::Acknowledged => "acknowledged",
        OutboxState::Rejected => "rejected",
        OutboxState::Conflict => "conflict",
        OutboxState::Retry => "retry",
    }
}

fn parse_state(state: &str) -> Result<OutboxState, StoreError> {
    match state {
        "pending" => Ok(OutboxState::Pending),
        "sending" => Ok(OutboxState::Sending),
        "acknowledged" => Ok(OutboxState::Acknowledged),
        "rejected" => Ok(OutboxState::Rejected),
        "conflict" => Ok(OutboxState::Conflict),
        "retry" => Ok(OutboxState::Retry),
        _ => Err(StoreError::permanent("invalid Stoolap outbox state")),
    }
}

/// Stoolap-specific implementation point. The backend owns statement details and must make
/// `reconcile` one transaction that advances the cursor only after applying every change.
#[async_trait]
pub trait StoolapBackend: Send + Sync {
    /// Loads ordered pending operations.
    async fn pending_operations(&self, limit: usize) -> Result<Vec<OperationEnvelope>, StoreError>;
    /// Appends an operation within the caller's optimistic domain transaction.
    async fn append_operation(&self, operation: OperationEnvelope) -> Result<(), StoreError>;
    /// Atomically applies one deterministic local-only compaction plan.
    async fn compact_outbox(
        &self,
        registry: &OptimizationRegistry,
        max_operations: usize,
    ) -> Result<QueueCompactionPlan, StoreError>;
    /// Compacts only while the supplied coordination fence remains current.
    async fn compact_outbox_fenced(
        &self,
        registry: &OptimizationRegistry,
        max_operations: usize,
        fence: Option<LeaseGrant>,
    ) -> Result<QueueCompactionPlan, StoreError>;
    /// Atomically rebases never-sent operations against newly installed authority versions.
    async fn rebase_outbox(
        &self,
        registry: &OptimizationRegistry,
        targets: &[RebaseTarget],
        max_operations: usize,
    ) -> Result<RebasePlan, StoreError>;
    /// Rebases only while the supplied coordination fence remains current.
    async fn rebase_outbox_fenced(
        &self,
        registry: &OptimizationRegistry,
        targets: &[RebaseTarget],
        max_operations: usize,
        fence: Option<LeaseGrant>,
    ) -> Result<RebasePlan, StoreError>;
    /// Atomically transitions selected replayable operations to `Sending`.
    async fn mark_sending(&self, operations: &[OperationId]) -> Result<(), StoreError>;
    /// Marks sending under the current coordination fence.
    async fn mark_sending_fenced(
        &self,
        operations: &[OperationId],
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError>;
    /// Returns in-flight operations to the replayable `Retry` state.
    async fn mark_retry(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
    ) -> Result<(), StoreError>;
    /// Marks retry under the current coordination fence.
    async fn mark_retry_fenced(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError>;
    /// Loads durable retry scheduling metadata.
    async fn retry_metadata(
        &self,
        operation: OperationId,
    ) -> Result<Option<RetryMetadata>, StoreError>;
    /// Loads one durable outbox state.
    async fn operation_state(
        &self,
        operation: OperationId,
    ) -> Result<Option<OutboxState>, StoreError>;
    /// Loads durable replayable queue statistics.
    async fn outbox_stats(&self) -> Result<OutboxStats, StoreError>;
    /// Loads unresolved conflicts in stable insertion order.
    async fn unresolved_conflicts(&self, limit: usize) -> Result<Vec<ConflictRecord>, StoreError>;
    /// Counts unresolved conflicts without loading their details.
    async fn unresolved_conflict_count(&self) -> Result<usize, StoreError>;
    /// Records one manual conflict decision.
    async fn resolve_conflict(
        &self,
        operation: OperationId,
        resolution: ConflictResolution,
    ) -> Result<(), StoreError>;
    /// Loads a durable scope cursor.
    async fn load_cursor(&self, scope: SyncScopeId) -> Result<Option<Cursor>, StoreError>;
    /// Computes canonical authoritative-base state at the installed cursor boundary.
    async fn capture_local_integrity(
        &self,
        scope: SyncScopeId,
        boundary: Cursor,
        generation: IntegrityGeneration,
        scheme: PartitionScheme,
        max_entities: usize,
    ) -> Result<IntegritySnapshot, StoreError>;
    /// Atomically repairs local authoritative base without touching replayable outbox intent.
    async fn repair_local_replica(
        &self,
        plan: &RepairPlan,
        replacements: &[CanonicalEntity],
        removals: &[EntityRef],
    ) -> Result<ReplicaRepairReport, StoreError>;
    /// Repairs only while the supplied coordination fence remains current.
    async fn repair_local_replica_fenced(
        &self,
        plan: &RepairPlan,
        replacements: &[CanonicalEntity],
        removals: &[EntityRef],
        fence: Option<LeaseGrant>,
    ) -> Result<ReplicaRepairReport, StoreError>;
    /// Atomically performs all reconciliation effects.
    async fn reconcile(&self, response: &SyncResponse) -> Result<(), StoreError>;
    /// Atomically verifies a lease fence and performs every reconciliation effect.
    async fn reconcile_fenced(
        &self,
        response: &SyncResponse,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError>;
    /// Stages and, for the final page, atomically installs a bootstrap snapshot.
    async fn stage_snapshot(&self, response: &BootstrapResponse) -> Result<(), StoreError>;
    /// Atomically verifies a lease fence and stages/installs a bootstrap page.
    async fn stage_snapshot_fenced(
        &self,
        response: &BootstrapResponse,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError>;
    /// Loads crash-recoverable bootstrap staging progress.
    async fn snapshot_progress(
        &self,
        scope: SyncScopeId,
    ) -> Result<Option<SnapshotProgress>, StoreError>;
}

#[async_trait]
impl StoolapBackend for StoolapDatabase {
    async fn pending_operations(&self, limit: usize) -> Result<Vec<OperationEnvelope>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let now = to_i64(unix_time_millis(), "current Unix timestamp")?;
        let rows = self
            .database
            .query(
                "SELECT envelope FROM aequora_outbox WHERE state IN ('pending', 'sending', 'retry') AND operation_id NOT IN (SELECT operation_id FROM aequora_retry_schedule WHERE next_attempt_unix_ms > $1) AND operation_id NOT IN (SELECT operation_id FROM aequora_scope_quarantine) ORDER BY enqueued_order LIMIT $2",
                (now, limit),
            )
            .map_err(stoolap_error)?;
        let mut operations = Vec::new();
        for row in rows {
            let row = row.map_err(stoolap_error)?;
            let encoded: String = row.get(0).map_err(stoolap_error)?;
            operations.push(decode(&encoded)?);
        }
        Ok(operations)
    }

    async fn append_operation(&self, operation: OperationEnvelope) -> Result<(), StoreError> {
        self.transact_local_mutation(&operation, |_| Ok(()))
    }

    async fn compact_outbox(
        &self,
        registry: &OptimizationRegistry,
        max_operations: usize,
    ) -> Result<QueueCompactionPlan, StoreError> {
        compact_stoolap_outbox(self, registry, max_operations, None)
    }

    async fn compact_outbox_fenced(
        &self,
        registry: &OptimizationRegistry,
        max_operations: usize,
        fence: Option<LeaseGrant>,
    ) -> Result<QueueCompactionPlan, StoreError> {
        compact_stoolap_outbox(self, registry, max_operations, fence)
    }

    async fn rebase_outbox(
        &self,
        registry: &OptimizationRegistry,
        targets: &[RebaseTarget],
        max_operations: usize,
    ) -> Result<RebasePlan, StoreError> {
        rebase_stoolap_outbox(self, registry, targets, max_operations, None)
    }

    async fn rebase_outbox_fenced(
        &self,
        registry: &OptimizationRegistry,
        targets: &[RebaseTarget],
        max_operations: usize,
        fence: Option<LeaseGrant>,
    ) -> Result<RebasePlan, StoreError> {
        rebase_stoolap_outbox(self, registry, targets, max_operations, fence)
    }

    async fn mark_sending(&self, operations: &[OperationId]) -> Result<(), StoreError> {
        transition_operations(&self.database, operations, OutboxState::Sending, None)
    }

    async fn mark_sending_fenced(
        &self,
        operations: &[OperationId],
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        transition_operations(&self.database, operations, OutboxState::Sending, fence)
    }

    async fn mark_retry(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
    ) -> Result<(), StoreError> {
        schedule_retry(&self.database, operations, next_attempt_unix_ms, None)
    }

    async fn mark_retry_fenced(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        schedule_retry(&self.database, operations, next_attempt_unix_ms, fence)
    }

    async fn retry_metadata(
        &self,
        operation: OperationId,
    ) -> Result<Option<RetryMetadata>, StoreError> {
        let mut rows = self
            .database
            .query(
                "SELECT attempt_count, next_attempt_unix_ms FROM aequora_retry_schedule WHERE operation_id = $1",
                (operation.to_string(),),
            )
            .map_err(stoolap_error)?;
        let Some(row) = rows.next() else {
            return Ok(None);
        };
        let row = row.map_err(stoolap_error)?;
        let attempt_count: i64 = row.get(0).map_err(stoolap_error)?;
        let next_attempt_unix_ms: i64 = row.get(1).map_err(stoolap_error)?;
        Ok(Some(RetryMetadata {
            attempt_count: u32::try_from(attempt_count)
                .map_err(|_| StoreError::permanent("invalid Stoolap retry attempt count"))?,
            next_attempt_unix_ms: u64::try_from(next_attempt_unix_ms)
                .map_err(|_| StoreError::permanent("negative Stoolap retry timestamp"))?,
        }))
    }

    async fn operation_state(
        &self,
        operation: OperationId,
    ) -> Result<Option<OutboxState>, StoreError> {
        self.database
            .query_opt::<String, _>(
                "SELECT state FROM aequora_outbox WHERE operation_id = $1",
                (operation.to_string(),),
            )
            .map_err(stoolap_error)?
            .map(|state| parse_state(&state))
            .transpose()
    }

    async fn outbox_stats(&self) -> Result<OutboxStats, StoreError> {
        let rows = self
            .database
            .query(
                "SELECT state, envelope FROM aequora_outbox
                  WHERE state IN ('pending', 'sending', 'retry', 'rejected')
                  ORDER BY enqueued_order",
                (),
            )
            .map_err(stoolap_error)?;
        let mut queue = OutboxStats::default();
        for row in rows {
            let row = row.map_err(stoolap_error)?;
            let state: String = row.get(0).map_err(stoolap_error)?;
            let envelope: String = row.get(1).map_err(stoolap_error)?;
            match parse_state(&state)? {
                OutboxState::Pending => queue.pending = queue.pending.saturating_add(1),
                OutboxState::Sending => queue.sending = queue.sending.saturating_add(1),
                OutboxState::Retry => queue.retry = queue.retry.saturating_add(1),
                OutboxState::Rejected => {
                    queue.rejected = queue.rejected.saturating_add(1);
                    continue;
                }
                OutboxState::Acknowledged | OutboxState::Conflict => continue,
            }
            let operation: OperationEnvelope = decode(&envelope)?;
            queue.oldest_pending_at = Some(
                queue
                    .oldest_pending_at
                    .map_or(operation.created_at, |current| {
                        current.min(operation.created_at)
                    }),
            );
        }
        Ok(queue)
    }

    async fn unresolved_conflicts(&self, limit: usize) -> Result<Vec<ConflictRecord>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = self
            .database
            .query(
                "SELECT detail FROM aequora_conflicts WHERE resolved = 0 ORDER BY operation_id LIMIT $1",
                (limit,),
            )
            .map_err(stoolap_error)?;
        let mut conflicts = Vec::new();
        for row in rows {
            let row = row.map_err(stoolap_error)?;
            let detail: String = row.get(0).map_err(stoolap_error)?;
            conflicts.push(ConflictRecord {
                conflict: decode(&detail)?,
                resolution: None,
            });
        }
        Ok(conflicts)
    }

    async fn unresolved_conflict_count(&self) -> Result<usize, StoreError> {
        let count = self
            .database
            .query_one::<i64, _>(
                "SELECT COUNT(*) FROM aequora_conflicts WHERE resolved = 0",
                (),
            )
            .map_err(stoolap_error)?;
        usize::try_from(count).map_err(|_| StoreError::permanent("negative Stoolap conflict count"))
    }

    async fn resolve_conflict(
        &self,
        operation: OperationId,
        resolution: ConflictResolution,
    ) -> Result<(), StoreError> {
        let detail = match resolution {
            ConflictResolution::AcceptServer => "accept_server".to_owned(),
            ConflictResolution::SupersededBy(replacement) => {
                format!("superseded_by:{replacement}")
            }
        };
        let affected = self
            .database
            .execute(
                "UPDATE aequora_conflicts SET resolved = 1, resolution_detail = $1 WHERE operation_id = $2 AND resolved = 0",
                (&detail, operation.to_string()),
            )
            .map_err(stoolap_error)?;
        if affected == 0 {
            return Err(StoreError::permanent(
                "conflict does not exist or was already resolved",
            ));
        }
        Ok(())
    }

    async fn load_cursor(&self, scope: SyncScopeId) -> Result<Option<Cursor>, StoreError> {
        let mut rows = self
            .database
            .query(
                "SELECT sequence, authority_id, authority_epoch FROM aequora_cursors WHERE scope_id = $1",
                (scope.to_string(),),
            )
            .map_err(stoolap_error)?;
        let Some(row) = rows.next() else {
            return Ok(None);
        };
        cursor_from_stoolap_row(scope, &row.map_err(stoolap_error)?).map(Some)
    }

    async fn capture_local_integrity(
        &self,
        scope: SyncScopeId,
        boundary: Cursor,
        generation: IntegrityGeneration,
        scheme: PartitionScheme,
        max_entities: usize,
    ) -> Result<IntegritySnapshot, StoreError> {
        capture_stoolap_integrity(self, scope, boundary, generation, scheme, max_entities)
    }

    async fn repair_local_replica(
        &self,
        plan: &RepairPlan,
        replacements: &[CanonicalEntity],
        removals: &[EntityRef],
    ) -> Result<ReplicaRepairReport, StoreError> {
        repair_stoolap_replica(self, plan, replacements, removals, None)
    }

    async fn repair_local_replica_fenced(
        &self,
        plan: &RepairPlan,
        replacements: &[CanonicalEntity],
        removals: &[EntityRef],
        fence: Option<LeaseGrant>,
    ) -> Result<ReplicaRepairReport, StoreError> {
        repair_stoolap_replica(self, plan, replacements, removals, fence)
    }

    async fn reconcile(&self, response: &SyncResponse) -> Result<(), StoreError> {
        reconcile_stoolap(self, response, None)
    }

    async fn reconcile_fenced(
        &self,
        response: &SyncResponse,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        reconcile_stoolap(self, response, fence)
    }

    async fn stage_snapshot(&self, response: &BootstrapResponse) -> Result<(), StoreError> {
        stage_stoolap_snapshot(self, response, None)
    }

    async fn stage_snapshot_fenced(
        &self,
        response: &BootstrapResponse,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        stage_stoolap_snapshot(self, response, fence)
    }

    async fn snapshot_progress(
        &self,
        scope: SyncScopeId,
    ) -> Result<Option<SnapshotProgress>, StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        let progress = snapshot_progress_transaction(&mut transaction, scope)?;
        transaction.commit().map_err(stoolap_error)?;
        Ok(progress)
    }
}

fn reconcile_stoolap(
    backend: &StoolapDatabase,
    response: &SyncResponse,
    fence: Option<LeaseGrant>,
) -> Result<(), StoreError> {
    let mut transaction = backend.database.begin().map_err(stoolap_error)?;
    if let Some(grant) = fence {
        validate_coordination_fence(&mut transaction, grant, unix_time_millis())?;
    }
    let scope = response.next_cursor.scope;
    if let Some(current) = load_cursor_transaction(&mut transaction, scope)? {
        if current.authority_id != AuthorityId::LEGACY_UNBOUND
            && (response.next_cursor.authority_id != current.authority_id
                || response.next_cursor.authority_epoch != current.authority_epoch)
        {
            return Err(StoreError::permanent(
                "Stoolap incremental reconciliation cannot cross authority timelines",
            ));
        }
        if response.next_cursor.sequence < current.sequence {
            return Err(StoreError::permanent(
                "Stoolap reconciliation cursor would regress",
            ));
        }
    }
    validate_authority_trust(&mut transaction, response.next_cursor)?;
    for change in &response.changes {
        let sequence = to_i64(change.sequence.0, "change sequence")?;
        let already_applied = transaction
            .query_opt::<i64, _>(
                "SELECT sequence FROM aequora_applied_events WHERE scope_id = $1 AND sequence = $2",
                (scope.to_string(), sequence),
            )
            .map_err(stoolap_error)?
            .is_some();
        if !already_applied {
            put_entity(
                &mut transaction,
                scope,
                &SnapshotEntity {
                    entity: change.entity,
                    version: change.version,
                    payload: change.payload.clone(),
                    tombstone: matches!(
                        change.change_kind,
                        aequora_protocol::ChangeKind::Tombstone
                    ),
                },
                "aequora_local_entities",
            )?;
            backend
                .projection_hook
                .apply_change(&mut transaction, scope, change)?;
            transaction
                .execute(
                    "INSERT INTO aequora_applied_events (scope_id, sequence) VALUES ($1, $2)",
                    (scope.to_string(), sequence),
                )
                .map_err(stoolap_error)?;
        }
    }
    for acknowledgement in &response.acknowledged {
        set_terminal(
            &mut transaction,
            acknowledgement.operation_id,
            OutboxState::Acknowledged,
            encode(acknowledgement)?,
        )?;
    }
    for rejection in &response.rejected {
        set_terminal(
            &mut transaction,
            rejection.operation_id,
            OutboxState::Rejected,
            encode(rejection)?,
        )?;
    }
    for conflict in &response.conflicts {
        set_terminal(
            &mut transaction,
            conflict.operation_id,
            OutboxState::Conflict,
            encode(conflict)?,
        )?;
        put_conflict(&mut transaction, conflict)?;
    }
    set_cursor(&mut transaction, response.next_cursor)?;
    transaction.commit().map_err(stoolap_error)
}

fn stage_stoolap_snapshot(
    backend: &StoolapDatabase,
    response: &BootstrapResponse,
    fence: Option<LeaseGrant>,
) -> Result<(), StoreError> {
    let mut transaction = backend.database.begin().map_err(stoolap_error)?;
    if let Some(grant) = fence {
        validate_coordination_fence(&mut transaction, grant, unix_time_millis())?;
    }
    let scope = response.cursor.scope;
    validate_authority_trust(&mut transaction, response.cursor)?;
    let existing = snapshot_progress_transaction(&mut transaction, scope)?;
    match existing {
        Some(progress)
            if progress.snapshot_id != response.snapshot_id
                || progress.next_offset != response.offset =>
        {
            return Err(StoreError::permanent(
                "Stoolap snapshot page does not match durable staging progress",
            ));
        }
        None if response.offset != 0 => {
            return Err(StoreError::permanent(
                "Stoolap snapshot staging must begin at offset zero",
            ));
        }
        None => {
            transaction
                .execute(
                    "DELETE FROM aequora_snapshot_staging WHERE scope_id = $1",
                    (scope.to_string(),),
                )
                .map_err(stoolap_error)?;
        }
        Some(_) => {}
    }
    for entity in &response.entities {
        put_entity(&mut transaction, scope, entity, "aequora_snapshot_staging")?;
    }
    if response.has_more {
        put_snapshot_progress(
            &mut transaction,
            SnapshotProgress {
                snapshot_id: response.snapshot_id,
                cursor: response.cursor,
                next_offset: response.next_offset,
            },
        )?;
    } else {
        transaction
            .execute(
                "DELETE FROM aequora_local_entities WHERE scope_id = $1",
                (scope.to_string(),),
            )
            .map_err(stoolap_error)?;
        backend
            .projection_hook
            .begin_snapshot(&mut transaction, scope)?;
        let snapshot_entities = load_staged_snapshot_entities(&mut transaction, scope)?;
        for entity in &snapshot_entities {
            backend
                .projection_hook
                .apply_snapshot_entity(&mut transaction, scope, entity)?;
        }
        backend
            .projection_hook
            .finish_snapshot(&mut transaction, scope)?;
        transaction
            .execute(
                "INSERT INTO aequora_local_entities (scope_id, entity_type, entity_id, version, payload, tombstone, provisional) SELECT scope_id, entity_type, entity_id, version, payload, tombstone, 0 FROM aequora_snapshot_staging WHERE scope_id = $1",
                (scope.to_string(),),
            )
            .map_err(stoolap_error)?;
        transaction
            .execute(
                "DELETE FROM aequora_snapshot_staging WHERE scope_id = $1",
                (scope.to_string(),),
            )
            .map_err(stoolap_error)?;
        transaction
            .execute(
                "DELETE FROM aequora_snapshot_progress WHERE scope_id = $1",
                (scope.to_string(),),
            )
            .map_err(stoolap_error)?;
        set_cursor(&mut transaction, response.cursor)?;
    }
    transaction.commit().map_err(stoolap_error)
}

fn load_staged_snapshot_entities(
    transaction: &mut ApiTransaction,
    scope: SyncScopeId,
) -> Result<Vec<SnapshotEntity>, StoreError> {
    let rows = transaction
        .query(
            "SELECT entity_type, entity_id, version, payload, tombstone
               FROM aequora_snapshot_staging
              WHERE scope_id = $1
              ORDER BY entity_type, entity_id",
            (scope.to_string(),),
        )
        .map_err(stoolap_error)?;
    let mut entities = Vec::new();
    for row in rows {
        let row = row.map_err(stoolap_error)?;
        let entity_type = row.get::<i64>(0).map_err(stoolap_error)?;
        let entity_id = row.get::<String>(1).map_err(stoolap_error)?;
        let version = row.get::<i64>(2).map_err(stoolap_error)?;
        let payload = row.get::<String>(3).map_err(stoolap_error)?;
        entities.push(SnapshotEntity {
            entity: EntityRef {
                entity_type: EntityType::new(
                    u16::try_from(entity_type)
                        .map_err(|_| StoreError::permanent("invalid staged entity type"))?,
                )
                .map_err(|_| StoreError::permanent("invalid staged entity type"))?,
                entity_id: EntityId::from_str(&entity_id)
                    .map_err(|_| StoreError::permanent("invalid staged entity ID"))?,
            },
            version: EntityVersion::new(
                u64::try_from(version)
                    .map_err(|_| StoreError::permanent("invalid staged entity version"))?,
            )
            .map_err(|_| StoreError::permanent("invalid staged entity version"))?,
            payload: hex::decode(payload)
                .map_err(|_| StoreError::permanent("invalid staged entity payload"))?,
            tombstone: row.get::<bool>(4).map_err(stoolap_error)?,
        });
    }
    Ok(entities)
}

fn capture_stoolap_integrity(
    backend: &StoolapDatabase,
    scope: SyncScopeId,
    boundary: Cursor,
    generation: IntegrityGeneration,
    scheme: PartitionScheme,
    max_entities: usize,
) -> Result<IntegritySnapshot, StoreError> {
    if boundary.scope != scope {
        return Err(StoreError::permanent(
            "integrity boundary belongs to another scope",
        ));
    }
    let mut transaction = backend.database.begin().map_err(stoolap_error)?;
    if load_cursor_transaction(&mut transaction, scope)? != Some(boundary) {
        return Err(StoreError::permanent(
            "local integrity capture requires the installed scope cursor boundary",
        ));
    }
    let limit = i64::try_from(max_entities.saturating_add(1)).unwrap_or(i64::MAX);
    let rows = transaction
        .query(
            "SELECT entity_type, entity_id, version, payload, tombstone
               FROM aequora_local_entities
              WHERE scope_id = $1 AND provisional = 0
              ORDER BY entity_type, entity_id
              LIMIT $2",
            (scope.to_string(), limit),
        )
        .map_err(stoolap_error)?;
    let mut entities = Vec::new();
    for row in rows {
        if entities.len() == max_entities {
            return Err(StoreError::permanent(
                "integrity input exceeds the configured entity bound",
            ));
        }
        let row = row.map_err(stoolap_error)?;
        let entity_type = row.get::<i64>(0).map_err(stoolap_error)?;
        let entity_id = row.get::<String>(1).map_err(stoolap_error)?;
        let version = row.get::<i64>(2).map_err(stoolap_error)?;
        let payload = row.get::<String>(3).map_err(stoolap_error)?;
        entities.push(CanonicalEntity {
            entity: EntityRef {
                entity_type: EntityType::new(
                    u16::try_from(entity_type)
                        .map_err(|_| StoreError::permanent("invalid local entity type"))?,
                )
                .map_err(|_| StoreError::permanent("invalid local entity type"))?,
                entity_id: EntityId::from_str(&entity_id)
                    .map_err(|_| StoreError::permanent("invalid local entity ID"))?,
            },
            version: EntityVersion::new(
                u64::try_from(version)
                    .map_err(|_| StoreError::permanent("invalid local entity version"))?,
            )
            .map_err(|_| StoreError::permanent("invalid local entity version"))?,
            hash_schema: CURRENT_HASH_SCHEMA,
            payload: hex::decode(payload)
                .map_err(|_| StoreError::permanent("invalid local entity payload"))?,
            tombstone: row.get::<bool>(4).map_err(stoolap_error)?,
        });
    }
    let snapshot =
        IntegritySnapshot::build(scope, boundary, generation, scheme, entities, max_entities)
            .map_err(|error| StoreError::permanent(error.to_string()))?;
    transaction.commit().map_err(stoolap_error)?;
    Ok(snapshot)
}

fn repair_stoolap_replica(
    backend: &StoolapDatabase,
    plan: &RepairPlan,
    replacements: &[CanonicalEntity],
    removals: &[EntityRef],
    fence: Option<LeaseGrant>,
) -> Result<ReplicaRepairReport, StoreError> {
    if !matches!(plan.strategy, RepairStrategy::ReplaceEntities) {
        return Err(StoreError::permanent(
            "this repair plan requires snapshot bootstrap or operator quarantine",
        ));
    }
    if replacements
        .iter()
        .map(|value| value.entity)
        .chain(removals.iter().copied())
        .any(|entity| !plan.affected_entities.contains(&entity))
    {
        return Err(StoreError::permanent(
            "repair payload contains an entity outside the approved plan",
        ));
    }
    let mut transaction = backend.database.begin().map_err(stoolap_error)?;
    if let Some(grant) = fence {
        validate_coordination_fence(&mut transaction, grant, unix_time_millis())?;
    }
    if load_cursor_transaction(&mut transaction, plan.boundary.scope)? != Some(plan.boundary) {
        return Err(StoreError::permanent(
            "repair boundary no longer matches the local synchronization cursor",
        ));
    }
    let rows = transaction
        .query(
            "SELECT operation_id FROM aequora_outbox
              WHERE state IN ('pending', 'sending', 'retry')
              ORDER BY enqueued_order",
            (),
        )
        .map_err(stoolap_error)?;
    let mut preserved_pending_operations = Vec::new();
    for row in rows {
        let operation = row
            .map_err(stoolap_error)?
            .get::<String>(0)
            .map_err(stoolap_error)?;
        preserved_pending_operations.push(
            OperationId::from_str(&operation)
                .map_err(|_| StoreError::permanent("invalid local operation ID"))?,
        );
    }
    backend
        .projection_hook
        .begin_repair(&mut transaction, plan)?;
    for entity in removals {
        transaction
            .execute(
                "DELETE FROM aequora_local_entities
                  WHERE scope_id = $1 AND entity_type = $2 AND entity_id = $3",
                (
                    plan.boundary.scope.to_string(),
                    i64::from(entity.entity_type.get()),
                    entity.entity_id.to_string(),
                ),
            )
            .map_err(stoolap_error)?;
        backend.projection_hook.remove_repair_entity(
            &mut transaction,
            plan.boundary.scope,
            *entity,
        )?;
    }
    for replacement in replacements {
        let entity = SnapshotEntity {
            entity: replacement.entity,
            version: replacement.version,
            payload: replacement.payload.clone(),
            tombstone: replacement.tombstone,
        };
        put_entity(
            &mut transaction,
            plan.boundary.scope,
            &entity,
            "aequora_local_entities",
        )?;
        backend.projection_hook.apply_repair_entity(
            &mut transaction,
            plan.boundary.scope,
            &entity,
        )?;
    }
    backend
        .projection_hook
        .finish_repair(&mut transaction, plan)?;
    transaction.commit().map_err(stoolap_error)?;
    Ok(ReplicaRepairReport {
        repair_id: plan.repair_id,
        sync_cursor: plan.boundary,
        preserved_pending_operations,
    })
}

fn transition_operations(
    database: &Database,
    operations: &[OperationId],
    next: OutboxState,
    fence: Option<LeaseGrant>,
) -> Result<(), StoreError> {
    let mut transaction = database.begin().map_err(stoolap_error)?;
    if let Some(grant) = fence {
        validate_coordination_fence(&mut transaction, grant, unix_time_millis())?;
    }
    for operation in operations {
        let operation_id = operation.to_string();
        let encoded = transaction
            .query_opt::<String, _>(
                "SELECT envelope FROM aequora_outbox WHERE operation_id = $1 AND state IN ('pending', 'sending', 'retry')",
                (&operation_id,),
            )
            .map_err(stoolap_error)?
            .ok_or_else(|| StoreError::permanent("operation is missing or terminal"))?;
        let envelope: OperationEnvelope = decode(&encoded)?;
        let immutable_hash = hex::encode(
            semantic_envelope_hash(&envelope)
                .map_err(|error| StoreError::permanent(error.to_string()))?,
        );
        transaction
            .execute(
                "UPDATE aequora_outbox SET state = $1, ever_sent = 1, immutable_hash = $2 WHERE operation_id = $3 AND state IN ('pending', 'sending', 'retry')",
                (state_name(next), &immutable_hash, &operation_id),
            )
            .map_err(stoolap_error)?;
    }
    transaction.commit().map_err(stoolap_error)
}

fn load_queue_entries(
    database: &Database,
    max_operations: usize,
) -> Result<Vec<QueueEntry>, StoreError> {
    let limit = i64::try_from(max_operations).unwrap_or(i64::MAX);
    let rows = database
        .query(
            "SELECT enqueued_order, envelope, state, ever_sent, immutable_hash FROM aequora_outbox WHERE state IN ('pending', 'sending', 'retry') ORDER BY enqueued_order LIMIT $1",
            (limit,),
        )
        .map_err(stoolap_error)?;
    let mut entries = Vec::new();
    for row in rows {
        let row = row.map_err(stoolap_error)?;
        let sequence: i64 = row.get(0).map_err(stoolap_error)?;
        let encoded: String = row.get(1).map_err(stoolap_error)?;
        let state: String = row.get(2).map_err(stoolap_error)?;
        let ever_sent: bool = row.get(3).map_err(stoolap_error)?;
        let stored_hash: Option<String> = row.get(4).map_err(stoolap_error)?;
        let operation: OperationEnvelope = decode(&encoded)?;
        let mutability = if state == "pending" && !ever_sent {
            MutationMutability::MutableUnsent
        } else {
            MutationMutability::ImmutablePossiblyDelivered
        };
        let immutable_hash = if mutability == MutationMutability::ImmutablePossiblyDelivered {
            match stored_hash {
                Some(hash) => Some(decode_hash(&hash)?),
                None => Some(
                    semantic_envelope_hash(&operation)
                        .map_err(|error| StoreError::permanent(error.to_string()))?,
                ),
            }
        } else {
            None
        };
        entries.push(QueueEntry {
            local_sequence: LocalOperationSeq(
                u64::try_from(sequence)
                    .map_err(|_| StoreError::permanent("negative local operation sequence"))?,
            ),
            operation,
            mutability,
            immutable_hash,
            cancellation: None,
        });
    }
    Ok(entries)
}

fn decode_hash(value: &str) -> Result<[u8; 32], StoreError> {
    let bytes = hex::decode(value)
        .map_err(|error| StoreError::permanent(format!("invalid immutable hash: {error}")))?;
    bytes
        .try_into()
        .map_err(|_| StoreError::permanent("invalid immutable hash length"))
}

fn compact_stoolap_outbox(
    backend: &StoolapDatabase,
    registry: &OptimizationRegistry,
    max_operations: usize,
    fence: Option<LeaseGrant>,
) -> Result<QueueCompactionPlan, StoreError> {
    let entries = load_queue_entries(&backend.database, max_operations)?;
    let plan = plan_compaction(&entries, registry, max_operations)
        .map_err(|error| StoreError::permanent(error.to_string()))?;
    let mut transaction = backend.database.begin().map_err(stoolap_error)?;
    if let Some(grant) = fence {
        validate_coordination_fence(&mut transaction, grant, unix_time_millis())?;
    }
    for supersession in &plan.supersessions {
        let old = supersession.old_operation_id.to_string();
        let eligible = transaction
            .query_opt::<i64, _>(
                "SELECT enqueued_order FROM aequora_outbox WHERE operation_id = $1 AND state = 'pending' AND ever_sent = 0",
                (&old,),
            )
            .map_err(stoolap_error)?
            .is_some();
        if !eligible {
            return Err(StoreError::permanent(
                "queue changed before compaction transaction committed",
            ));
        }
        let reason = match supersession.reason {
            SupersessionReason::ReplacedByLatest => "replaced_by_latest",
            SupersessionReason::CanceledPair => "canceled_pair",
        };
        match supersession.new_operation_id {
            Some(replacement) => transaction
                .execute(
                    "INSERT INTO aequora_supersession (old_operation_id, new_operation_id, reason) VALUES ($1, $2, $3)",
                    (&old, replacement.to_string(), reason),
                )
                .map_err(stoolap_error)?,
            None => transaction
                .execute(
                    "INSERT INTO aequora_supersession (old_operation_id, new_operation_id, reason) VALUES ($1, NULL, $2)",
                    (&old, reason),
                )
                .map_err(stoolap_error)?,
        };
        transaction
            .execute(
                "DELETE FROM aequora_retry_schedule WHERE operation_id = $1",
                (&old,),
            )
            .map_err(stoolap_error)?;
        transaction
            .execute(
                "DELETE FROM aequora_outbox WHERE operation_id = $1",
                (&old,),
            )
            .map_err(stoolap_error)?;
    }
    transaction.commit().map_err(stoolap_error)?;
    Ok(plan)
}

fn rebase_stoolap_outbox(
    backend: &StoolapDatabase,
    registry: &OptimizationRegistry,
    targets: &[RebaseTarget],
    max_operations: usize,
    fence: Option<LeaseGrant>,
) -> Result<RebasePlan, StoreError> {
    let entries = load_queue_entries(&backend.database, max_operations)?;
    let plan = plan_rebase(&entries, targets, registry)
        .map_err(|error| StoreError::permanent(error.to_string()))?;
    let by_id = entries
        .into_iter()
        .map(|entry| (entry.operation.operation_id, entry.operation))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut transaction = backend.database.begin().map_err(stoolap_error)?;
    if let Some(grant) = fence {
        validate_coordination_fence(&mut transaction, grant, unix_time_millis())?;
    }
    for rewrite in &plan.rewrites {
        let mut operation = by_id
            .get(&rewrite.operation_id)
            .cloned()
            .ok_or_else(|| StoreError::permanent("rebase operation disappeared"))?;
        operation.base_version = rewrite.new_base;
        let encoded = encode(&operation)?;
        let affected = transaction
            .execute(
                "UPDATE aequora_outbox SET envelope = $1 WHERE operation_id = $2 AND state = 'pending' AND ever_sent = 0",
                (&encoded, rewrite.operation_id.to_string()),
            )
            .map_err(stoolap_error)?;
        if affected != 1 {
            return Err(StoreError::permanent(
                "queue changed before rebase transaction committed",
            ));
        }
        let old = rewrite.old_base.map(EntityVersion::get);
        let new = rewrite.new_base.map(EntityVersion::get);
        transaction
            .execute(
                "INSERT INTO aequora_rebase_history (operation_id, old_base_version, new_base_version) VALUES ($1, $2, $3)",
                (
                    rewrite.operation_id.to_string(),
                    old.and_then(|value| i64::try_from(value).ok()),
                    new.and_then(|value| i64::try_from(value).ok()),
                ),
            )
            .map_err(stoolap_error)?;
    }
    transaction.commit().map_err(stoolap_error)?;
    Ok(plan)
}

fn schedule_retry(
    database: &Database,
    operations: &[OperationId],
    next_attempt_unix_ms: u64,
    fence: Option<LeaseGrant>,
) -> Result<(), StoreError> {
    let next_attempt = to_i64(next_attempt_unix_ms, "retry timestamp")?;
    let mut transaction = database.begin().map_err(stoolap_error)?;
    if let Some(grant) = fence {
        validate_coordination_fence(&mut transaction, grant, unix_time_millis())?;
    }
    for operation in operations {
        let operation = operation.to_string();
        let state = transaction
            .query_opt::<String, _>(
                "SELECT state FROM aequora_outbox WHERE operation_id = $1",
                (&operation,),
            )
            .map_err(stoolap_error)?
            .ok_or_else(|| StoreError::permanent("operation is missing from the outbox"))?;
        if !parse_state(&state)?.is_replayable() {
            return Err(StoreError::permanent(
                "terminal outbox operation cannot transition back to retry",
            ));
        }
        let encoded = transaction
            .query_one::<String, _>(
                "SELECT envelope FROM aequora_outbox WHERE operation_id = $1",
                (&operation,),
            )
            .map_err(stoolap_error)?;
        let envelope: OperationEnvelope = decode(&encoded)?;
        let immutable_hash = hex::encode(
            semantic_envelope_hash(&envelope)
                .map_err(|error| StoreError::permanent(error.to_string()))?,
        );
        transaction
            .execute(
                "UPDATE aequora_outbox SET state = 'retry', ever_sent = 1, immutable_hash = $1 WHERE operation_id = $2",
                (&immutable_hash, &operation),
            )
            .map_err(stoolap_error)?;
        if transaction
            .query_opt::<i64, _>(
                "SELECT attempt_count FROM aequora_retry_schedule WHERE operation_id = $1",
                (&operation,),
            )
            .map_err(stoolap_error)?
            .is_some()
        {
            transaction
                .execute(
                    "UPDATE aequora_retry_schedule SET attempt_count = attempt_count + 1, next_attempt_unix_ms = $1 WHERE operation_id = $2",
                    (next_attempt, &operation),
                )
                .map_err(stoolap_error)?;
        } else {
            transaction
                .execute(
                    "INSERT INTO aequora_retry_schedule (operation_id, attempt_count, next_attempt_unix_ms) VALUES ($1, 1, $2)",
                    (&operation, next_attempt),
                )
                .map_err(stoolap_error)?;
        }
    }
    transaction.commit().map_err(stoolap_error)
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn load_cursor_transaction(
    transaction: &mut ApiTransaction,
    scope: SyncScopeId,
) -> Result<Option<Cursor>, StoreError> {
    let mut rows = transaction
        .query(
            "SELECT sequence, authority_id, authority_epoch FROM aequora_cursors WHERE scope_id = $1",
            (scope.to_string(),),
        )
        .map_err(stoolap_error)?;
    let Some(row) = rows.next() else {
        return Ok(None);
    };
    cursor_from_stoolap_row(scope, &row.map_err(stoolap_error)?).map(Some)
}

fn validate_authority_trust(
    transaction: &mut ApiTransaction,
    cursor: Cursor,
) -> Result<(), StoreError> {
    if cursor.authority_id == AuthorityId::LEGACY_UNBOUND {
        return Ok(());
    }
    let mut rows = transaction
        .query(
            "SELECT authority_id, highest_epoch FROM aequora_authority_trust",
            (),
        )
        .map_err(stoolap_error)?;
    let Some(row) = rows.next() else {
        transaction
            .execute(
                "INSERT INTO aequora_authority_trust (singleton, authority_id, highest_epoch) VALUES (1, $1, $2)",
                (
                    cursor.authority_id.to_string(),
                    to_i64(cursor.authority_epoch.get(), "trusted authority epoch")?,
                ),
            )
            .map_err(stoolap_error)?;
        return Ok(());
    };
    let row = row.map_err(stoolap_error)?;
    let trusted_id: String = row.get(0).map_err(stoolap_error)?;
    let trusted_epoch: i64 = row.get(1).map_err(stoolap_error)?;
    let trusted_id = parse_id::<AuthorityId>(&trusted_id, "trusted authority ID")?;
    let trusted_epoch = u64::try_from(trusted_epoch)
        .map_err(|_| StoreError::permanent("invalid trusted authority epoch"))?;
    if trusted_id != cursor.authority_id {
        return Err(StoreError::permanent(
            "Stoolap local store rejected a different authority identity",
        ));
    }
    if cursor.authority_epoch.get() < trusted_epoch {
        return Err(StoreError::permanent(
            "Stoolap local store detected authority epoch rollback",
        ));
    }
    if cursor.authority_epoch.get() > trusted_epoch {
        transaction
            .execute(
                "UPDATE aequora_authority_trust SET highest_epoch = $1 WHERE singleton = 1 AND authority_id = $2",
                (
                    to_i64(cursor.authority_epoch.get(), "trusted authority epoch")?,
                    cursor.authority_id.to_string(),
                ),
            )
            .map_err(stoolap_error)?;
    }
    if rows.next().is_some() {
        return Err(StoreError::permanent(
            "Stoolap local store contains multiple trusted authorities",
        ));
    }
    Ok(())
}

fn cursor_from_stoolap_row(
    scope: SyncScopeId,
    row: &stoolap::api::ResultRow,
) -> Result<Cursor, StoreError> {
    let sequence: i64 = row.get(0).map_err(stoolap_error)?;
    let authority_id: String = row.get(1).map_err(stoolap_error)?;
    let authority_epoch: i64 = row.get(2).map_err(stoolap_error)?;
    Ok(Cursor::new(
        parse_id::<AuthorityId>(&authority_id, "authority ID")?,
        AuthorityEpoch::new(
            u64::try_from(authority_epoch)
                .map_err(|_| StoreError::permanent("invalid Stoolap authority epoch"))?,
        )
        .map_err(|error| StoreError::permanent(error.to_string()))?,
        scope,
        Sequence(
            u64::try_from(sequence)
                .map_err(|_| StoreError::permanent("negative Stoolap cursor sequence"))?,
        ),
    ))
}

fn set_cursor(transaction: &mut ApiTransaction, cursor: Cursor) -> Result<(), StoreError> {
    let scope = cursor.scope.to_string();
    let sequence = to_i64(cursor.sequence.0, "cursor sequence")?;
    let authority_id = cursor.authority_id.to_string();
    let authority_epoch = to_i64(cursor.authority_epoch.get(), "cursor authority epoch")?;
    if transaction
        .query_opt::<i64, _>(
            "SELECT sequence FROM aequora_cursors WHERE scope_id = $1",
            (&scope,),
        )
        .map_err(stoolap_error)?
        .is_some()
    {
        transaction
            .execute(
                "UPDATE aequora_cursors SET sequence = $1, authority_id = $2, authority_epoch = $3 WHERE scope_id = $4",
                (sequence, &authority_id, authority_epoch, &scope),
            )
            .map_err(stoolap_error)?;
    } else {
        transaction
            .execute(
                "INSERT INTO aequora_cursors (scope_id, sequence, authority_id, authority_epoch) VALUES ($1, $2, $3, $4)",
                (&scope, sequence, &authority_id, authority_epoch),
            )
            .map_err(stoolap_error)?;
    }
    Ok(())
}

fn put_entity(
    transaction: &mut ApiTransaction,
    scope: SyncScopeId,
    entity: &SnapshotEntity,
    table: &'static str,
) -> Result<(), StoreError> {
    let scope = scope.to_string();
    let entity_type = i64::from(entity.entity.entity_type.get());
    let entity_id = entity.entity.entity_id.to_string();
    let version = to_i64(entity.version.get(), "entity version")?;
    let payload = hex::encode(&entity.payload);
    let exists = transaction
        .query_opt::<i64, _>(
            &format!("SELECT version FROM {table} WHERE scope_id = $1 AND entity_type = $2 AND entity_id = $3"),
            (&scope, entity_type, &entity_id),
        )
        .map_err(stoolap_error)?
        .is_some();
    if exists {
        transaction
            .execute(
                &format!("UPDATE {table} SET version = $1, payload = $2, tombstone = $3, provisional = 0 WHERE scope_id = $4 AND entity_type = $5 AND entity_id = $6"),
                (version, &payload, entity.tombstone, &scope, entity_type, &entity_id),
            )
            .map_err(stoolap_error)?;
    } else {
        transaction
            .execute(
                &format!("INSERT INTO {table} (scope_id, entity_type, entity_id, version, payload, tombstone, provisional) VALUES ($1, $2, $3, $4, $5, $6, 0)"),
                (&scope, entity_type, &entity_id, version, &payload, entity.tombstone),
            )
            .map_err(stoolap_error)?;
    }
    Ok(())
}

fn set_terminal(
    transaction: &mut ApiTransaction,
    operation: OperationId,
    state: OutboxState,
    detail: String,
) -> Result<(), StoreError> {
    transaction
        .execute(
            "UPDATE aequora_outbox SET state = $1, terminal_detail = $2 WHERE operation_id = $3",
            (state_name(state), detail, operation.to_string()),
        )
        .map_err(stoolap_error)?;
    Ok(())
}

fn put_conflict(transaction: &mut ApiTransaction, conflict: &Conflict) -> Result<(), StoreError> {
    let operation = conflict.operation_id.to_string();
    let detail = encode(conflict)?;
    let exists = transaction
        .query_opt::<String, _>(
            "SELECT operation_id FROM aequora_conflicts WHERE operation_id = $1",
            (&operation,),
        )
        .map_err(stoolap_error)?
        .is_some();
    if exists {
        transaction
            .execute(
                "UPDATE aequora_conflicts SET detail = $1, resolved = 0, resolution_detail = NULL WHERE operation_id = $2",
                (&detail, &operation),
            )
            .map_err(stoolap_error)?;
    } else {
        transaction
            .execute(
                "INSERT INTO aequora_conflicts (operation_id, detail, resolved) VALUES ($1, $2, 0)",
                (&operation, &detail),
            )
            .map_err(stoolap_error)?;
    }
    Ok(())
}

fn snapshot_progress_transaction(
    transaction: &mut ApiTransaction,
    scope: SyncScopeId,
) -> Result<Option<SnapshotProgress>, StoreError> {
    let mut rows = transaction
        .query(
            "SELECT snapshot_id, cursor_sequence, next_offset, authority_id, authority_epoch FROM aequora_snapshot_progress WHERE scope_id = $1",
            (scope.to_string(),),
        )
        .map_err(stoolap_error)?;
    let Some(row) = rows.next() else {
        return Ok(None);
    };
    let row = row.map_err(stoolap_error)?;
    let snapshot: String = row.get(0).map_err(stoolap_error)?;
    let cursor: i64 = row.get(1).map_err(stoolap_error)?;
    let offset: i64 = row.get(2).map_err(stoolap_error)?;
    let authority_id: String = row.get(3).map_err(stoolap_error)?;
    let authority_epoch: i64 = row.get(4).map_err(stoolap_error)?;
    Ok(Some(SnapshotProgress {
        snapshot_id: parse_id::<SnapshotId>(&snapshot, "snapshot ID")?,
        cursor: Cursor::new(
            parse_id::<AuthorityId>(&authority_id, "snapshot authority ID")?,
            AuthorityEpoch::new(
                u64::try_from(authority_epoch)
                    .map_err(|_| StoreError::permanent("invalid snapshot authority epoch"))?,
            )
            .map_err(|error| StoreError::permanent(error.to_string()))?,
            scope,
            Sequence(
                u64::try_from(cursor)
                    .map_err(|_| StoreError::permanent("negative snapshot cursor"))?,
            ),
        ),
        next_offset: u64::try_from(offset)
            .map_err(|_| StoreError::permanent("negative snapshot offset"))?,
    }))
}

fn put_snapshot_progress(
    transaction: &mut ApiTransaction,
    progress: SnapshotProgress,
) -> Result<(), StoreError> {
    let scope = progress.cursor.scope.to_string();
    let snapshot = progress.snapshot_id.to_string();
    let cursor = to_i64(progress.cursor.sequence.0, "snapshot cursor")?;
    let offset = to_i64(progress.next_offset, "snapshot offset")?;
    let authority_id = progress.cursor.authority_id.to_string();
    let authority_epoch = to_i64(
        progress.cursor.authority_epoch.get(),
        "snapshot authority epoch",
    )?;
    if snapshot_progress_transaction(transaction, progress.cursor.scope)?.is_some() {
        transaction
            .execute(
                "UPDATE aequora_snapshot_progress SET snapshot_id = $1, cursor_sequence = $2, next_offset = $3, authority_id = $4, authority_epoch = $5 WHERE scope_id = $6",
                (&snapshot, cursor, offset, &authority_id, authority_epoch, &scope),
            )
            .map_err(stoolap_error)?;
    } else {
        transaction
            .execute(
                "INSERT INTO aequora_snapshot_progress (scope_id, snapshot_id, cursor_sequence, next_offset, authority_id, authority_epoch) VALUES ($1, $2, $3, $4, $5, $6)",
                (&scope, &snapshot, cursor, offset, &authority_id, authority_epoch),
            )
            .map_err(stoolap_error)?;
    }
    Ok(())
}

/// Aequora local-store adapter over an application-owned Stoolap backend.
pub struct StoolapStore<B> {
    backend: B,
}

impl<B> StoolapStore<B> {
    /// Wraps a Stoolap backend without exposing connection types to core crates.
    #[must_use]
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }
    /// Borrows the backend for migration or domain-transaction integration.
    #[must_use]
    pub const fn backend(&self) -> &B {
        &self.backend
    }
}

impl<B: TransactionCapabilityProvider> TransactionCapabilityProvider for StoolapStore<B> {
    fn transaction_capabilities(&self) -> TransactionCapabilities {
        self.backend.transaction_capabilities()
    }
}

impl<B: IntegrityCapabilityProvider> IntegrityCapabilityProvider for StoolapStore<B> {
    fn integrity_support(&self) -> IntegritySupport {
        self.backend.integrity_support()
    }
}

impl<B: AdapterManifestProvider> AdapterManifestProvider for StoolapStore<B> {
    fn adapter_manifest(&self) -> AdapterManifest {
        self.backend.adapter_manifest()
    }
}

#[async_trait]
impl<B: StoolapBackend> OutboxStore for StoolapStore<B> {
    async fn pending_operations(&self, limit: usize) -> Result<Vec<OperationEnvelope>, StoreError> {
        self.backend.pending_operations(limit).await
    }
    async fn append_operation(&self, operation: OperationEnvelope) -> Result<(), StoreError> {
        self.backend.append_operation(operation).await
    }

    async fn compact_outbox(
        &self,
        registry: &OptimizationRegistry,
        max_operations: usize,
    ) -> Result<QueueCompactionPlan, StoreError> {
        self.backend.compact_outbox(registry, max_operations).await
    }

    async fn compact_outbox_fenced(
        &self,
        registry: &OptimizationRegistry,
        max_operations: usize,
        fence: Option<LeaseGrant>,
    ) -> Result<QueueCompactionPlan, StoreError> {
        self.backend
            .compact_outbox_fenced(registry, max_operations, fence)
            .await
    }

    async fn rebase_outbox(
        &self,
        registry: &OptimizationRegistry,
        targets: &[RebaseTarget],
        max_operations: usize,
    ) -> Result<RebasePlan, StoreError> {
        self.backend
            .rebase_outbox(registry, targets, max_operations)
            .await
    }

    async fn rebase_outbox_fenced(
        &self,
        registry: &OptimizationRegistry,
        targets: &[RebaseTarget],
        max_operations: usize,
        fence: Option<LeaseGrant>,
    ) -> Result<RebasePlan, StoreError> {
        self.backend
            .rebase_outbox_fenced(registry, targets, max_operations, fence)
            .await
    }
}

#[async_trait]
impl<B: StoolapBackend> OutboxStateStore for StoolapStore<B> {
    async fn mark_sending(&self, operations: &[OperationId]) -> Result<(), StoreError> {
        self.backend.mark_sending(operations).await
    }

    async fn mark_sending_fenced(
        &self,
        operations: &[OperationId],
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        self.backend.mark_sending_fenced(operations, fence).await
    }

    async fn mark_retry(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
    ) -> Result<(), StoreError> {
        self.backend
            .mark_retry(operations, next_attempt_unix_ms)
            .await
    }

    async fn mark_retry_fenced(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        self.backend
            .mark_retry_fenced(operations, next_attempt_unix_ms, fence)
            .await
    }

    async fn retry_metadata(
        &self,
        operation: OperationId,
    ) -> Result<Option<RetryMetadata>, StoreError> {
        self.backend.retry_metadata(operation).await
    }

    async fn operation_state(
        &self,
        operation: OperationId,
    ) -> Result<Option<OutboxState>, StoreError> {
        self.backend.operation_state(operation).await
    }

    async fn outbox_stats(&self) -> Result<OutboxStats, StoreError> {
        self.backend.outbox_stats().await
    }
}

#[async_trait]
impl<B: StoolapBackend> ConflictInbox for StoolapStore<B> {
    async fn unresolved_conflicts(&self, limit: usize) -> Result<Vec<ConflictRecord>, StoreError> {
        self.backend.unresolved_conflicts(limit).await
    }

    async fn unresolved_conflict_count(&self) -> Result<usize, StoreError> {
        self.backend.unresolved_conflict_count().await
    }

    async fn resolve_conflict(
        &self,
        operation: OperationId,
        resolution: ConflictResolution,
    ) -> Result<(), StoreError> {
        self.backend.resolve_conflict(operation, resolution).await
    }
}

#[async_trait]
impl<B: StoolapBackend> CursorStore for StoolapStore<B> {
    async fn load_cursor(&self, scope: SyncScopeId) -> Result<Option<Cursor>, StoreError> {
        self.backend.load_cursor(scope).await
    }
}

#[async_trait]
impl<B: StoolapBackend> LocalIntegrityStore for StoolapStore<B> {
    async fn capture_local_integrity(
        &self,
        scope: SyncScopeId,
        boundary: Cursor,
        generation: IntegrityGeneration,
        scheme: PartitionScheme,
        max_entities: usize,
    ) -> Result<IntegritySnapshot, StoreError> {
        self.backend
            .capture_local_integrity(scope, boundary, generation, scheme, max_entities)
            .await
    }

    async fn repair_local_replica(
        &self,
        plan: &RepairPlan,
        replacements: &[CanonicalEntity],
        removals: &[EntityRef],
    ) -> Result<ReplicaRepairReport, StoreError> {
        self.backend
            .repair_local_replica(plan, replacements, removals)
            .await
    }

    async fn repair_local_replica_fenced(
        &self,
        plan: &RepairPlan,
        replacements: &[CanonicalEntity],
        removals: &[EntityRef],
        fence: Option<LeaseGrant>,
    ) -> Result<ReplicaRepairReport, StoreError> {
        self.backend
            .repair_local_replica_fenced(plan, replacements, removals, fence)
            .await
    }
}

#[async_trait]
impl<B: StoolapBackend> ReconciliationStore for StoolapStore<B> {
    async fn reconcile(&self, response: &SyncResponse) -> Result<(), StoreError> {
        self.backend.reconcile(response).await
    }

    async fn reconcile_fenced(
        &self,
        response: &SyncResponse,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        self.backend.reconcile_fenced(response, fence).await
    }

    async fn stage_snapshot(&self, response: &BootstrapResponse) -> Result<(), StoreError> {
        self.backend.stage_snapshot(response).await
    }

    async fn stage_snapshot_fenced(
        &self,
        response: &BootstrapResponse,
        fence: Option<LeaseGrant>,
    ) -> Result<(), StoreError> {
        self.backend.stage_snapshot_fenced(response, fence).await
    }

    async fn snapshot_progress(
        &self,
        scope: SyncScopeId,
    ) -> Result<Option<SnapshotProgress>, StoreError> {
        self.backend.snapshot_progress(scope).await
    }
}

#[async_trait]
impl<B> LocalCoordinationStore for StoolapStore<B>
where
    B: StoolapBackend + LocalCoordinationStore,
{
    fn coordination_support(&self) -> LocalCoordinationSupport {
        self.backend.coordination_support()
    }

    async fn coordination_snapshot(&self) -> Result<CoordinationSnapshot, StoreError> {
        self.backend.coordination_snapshot().await
    }

    async fn acquire_lease(&self, request: LeaseRequest) -> Result<LeaseGrant, StoreError> {
        self.backend.acquire_lease(request).await
    }

    async fn renew_lease(
        &self,
        grant: LeaseGrant,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<LeaseGrant, StoreError> {
        self.backend.renew_lease(grant, now_unix_ms, ttl_ms).await
    }

    async fn release_lease(&self, grant: LeaseGrant, now_unix_ms: u64) -> Result<(), StoreError> {
        self.backend.release_lease(grant, now_unix_ms).await
    }

    async fn validate_fence(&self, grant: LeaseGrant, now_unix_ms: u64) -> Result<(), StoreError> {
        self.backend.validate_fence(grant, now_unix_ms).await
    }

    async fn advance_store_generation(
        &self,
        grant: LeaseGrant,
        now_unix_ms: u64,
    ) -> Result<LocalStoreGeneration, StoreError> {
        self.backend
            .advance_store_generation(grant, now_unix_ms)
            .await
    }
}

#[async_trait]
impl<B> ScopeStateStore for StoolapStore<B>
where
    B: StoolapBackend + ScopeStateStore,
{
    async fn load_scope_state(&self) -> Result<LocalScopeState, StoreError> {
        self.backend.load_scope_state().await
    }

    async fn install_subscription(&self, subscription: &Subscription) -> Result<(), StoreError> {
        self.backend.install_subscription(subscription).await
    }

    async fn apply_scope_transition(
        &self,
        transition: &ScopeTransition,
    ) -> Result<ScopeTransitionOutcome, StoreError> {
        self.backend.apply_scope_transition(transition).await
    }

    async fn apply_scope_transition_fenced(
        &self,
        transition: &ScopeTransition,
        fence: Option<LeaseGrant>,
    ) -> Result<ScopeTransitionOutcome, StoreError> {
        self.backend
            .apply_scope_transition_fenced(transition, fence)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_protocol::{
        ChangeKind, Conflict, ConflictPolicy, OperationAck, OperationKind, OperationMetadata,
        OperationRejection, RejectionCode, RemoteChange, SyncDirective,
    };
    use aequora_store::StoreErrorKind;
    use aequora_testkit::{
        InMemoryLocalStore,
        contracts::{scope_state_contract_fixture, verify_local_store, verify_scope_state_store},
    };
    use aequora_types::{
        ActorId, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, EventId,
        HybridTimestamp, LineageRef, NodeId, ProtocolVersion, SchemaVersion, TenantId,
    };
    use tempfile::tempdir;

    #[test]
    fn part_36_manifest_is_structurally_valid_and_explicit() {
        STOOLAP_STORAGE_ADAPTER_MANIFEST
            .validate()
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            STOOLAP_STORAGE_ADAPTER_MANIFEST
                .supports_role(adapter_sdk::AdapterRole::LocalReplicaStore)
        );
        assert!(
            !STOOLAP_STORAGE_ADAPTER_MANIFEST
                .known_limitations
                .is_empty()
        );
    }

    fn persistent_dsn(name: &str) -> (tempfile::TempDir, String) {
        let directory = tempdir().unwrap_or_else(|error| panic!("{error}"));
        let path = directory.path().join(name);
        let dsn = format!("file://{}", path.display());
        (directory, dsn)
    }

    fn operation(entity: EntityRef) -> OperationEnvelope {
        OperationEnvelope {
            protocol_version: ProtocolVersion::V1,
            operation_id: OperationId::new(),
            tenant_id: TenantId::new(),
            actor_id: ActorId::new(),
            device_id: DeviceId::new(),
            entity,
            base_version: None,
            created_at: HybridTimestamp {
                physical_ms: 1,
                logical: 0,
                node: NodeId::new(),
            },
            schema_version: SchemaVersion(1),
            operation_kind: OperationKind(1),
            payload: b"present".to_vec(),
            metadata: OperationMetadata::default(),
        }
    }

    #[tokio::test]
    async fn stoolap_passes_scope_state_contract() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let store = StoolapStore::new(backend);
        let (subscription, transition, operation) =
            scope_state_contract_fixture().unwrap_or_else(|error| panic!("{error}"));
        verify_scope_state_store(&store, subscription, transition, operation)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }

    fn entity() -> EntityRef {
        EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
            entity_id: EntityId::new(),
        }
    }

    fn expected_integrity_snapshot(
        scope: SyncScopeId,
        boundary: Cursor,
        entity: EntityRef,
        payload: &[u8],
    ) -> aequora_integrity::IntegritySnapshot {
        aequora_integrity::IntegritySnapshot::build(
            scope,
            boundary,
            aequora_integrity::CURRENT_INTEGRITY_GENERATION,
            PartitionScheme::new(8).unwrap_or_else(|error| panic!("{error}")),
            [CanonicalEntity {
                entity,
                version: EntityVersion::INITIAL,
                hash_schema: CURRENT_HASH_SCHEMA,
                payload: payload.to_vec(),
                tombstone: false,
            }],
            100,
        )
        .unwrap_or_else(|error| panic!("{error}"))
    }

    fn replace_latest_registry(kind: u16) -> OptimizationRegistry {
        let mut registry = OptimizationRegistry::default();
        registry.register(
            aequora_protocol::OperationKind(kind),
            aequora_queue::OperationOptimization {
                compaction: aequora_queue::CompactionPolicy::ReplaceLatest,
                rebase: aequora_queue::RebasePolicy::ReapplyIntent,
                operation_class: aequora_queue::OperationClassId(kind),
                ..aequora_queue::OperationOptimization::default()
            },
        );
        registry
    }

    fn same_lineage_operations(entity: EntityRef, count: usize) -> Vec<OperationEnvelope> {
        let mut operations = (0..count).map(|_| operation(entity)).collect::<Vec<_>>();
        if let Some(lineage) = operations
            .first()
            .map(|operation| operation.metadata.lineage)
        {
            for operation in &mut operations {
                operation.metadata.lineage = lineage;
            }
        }
        operations
    }

    fn lease_request(
        owner_id: aequora_coordination::ProcessInstanceId,
        kind: LeaseKind,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> LeaseRequest {
        LeaseRequest {
            owner_id,
            kind,
            now_unix_ms,
            ttl_ms,
        }
    }

    fn empty_sync_response(scope: SyncScopeId) -> SyncResponse {
        SyncResponse {
            protocol: ProtocolVersion::V1,
            directive: SyncDirective::Continue,
            acknowledged: Vec::new(),
            rejected: Vec::new(),
            conflicts: Vec::new(),
            changes: Vec::new(),
            next_cursor: Cursor::legacy(scope, Sequence(0)),
            has_more: false,
            server_time: HybridTimestamp {
                physical_ms: 1,
                logical: 0,
                node: NodeId::new(),
            },
        }
    }

    #[tokio::test]
    async fn durable_authority_trust_rejects_epoch_rollback_and_identity_change() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let scope = SyncScopeId::new();
        let trusted = AuthorityId::new();
        let other = AuthorityId::new();
        let epoch_two = AuthorityEpoch::new(2).unwrap_or_else(|error| panic!("{error}"));
        let mut response = empty_sync_response(scope);
        response.next_cursor = Cursor::new(trusted, epoch_two, scope, Sequence(0));
        backend
            .reconcile(&response)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        response.next_cursor = Cursor::new(trusted, AuthorityEpoch::INITIAL, scope, Sequence(0));
        assert!(backend.reconcile(&response).await.is_err());

        response.next_cursor = Cursor::new(other, epoch_two, scope, Sequence(0));
        assert!(backend.reconcile(&response).await.is_err());
        assert_eq!(
            backend.load_cursor(scope).await,
            Ok(Some(Cursor::new(trusted, epoch_two, scope, Sequence(0))))
        );
    }

    #[test]
    fn simultaneous_stoolap_candidates_commit_exactly_one_lease_owner() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let now = unix_time_millis();
        let results = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..2 {
                let candidate = backend.clone();
                let barrier = Arc::clone(&barrier);
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap_or_else(|error| panic!("{error}"))
                        .block_on(candidate.acquire_lease(lease_request(
                            aequora_coordination::ProcessInstanceId::new(),
                            LeaseKind::SyncCoordinator,
                            now,
                            10_000,
                        )))
                }));
            }
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| panic!("candidate panicked"))
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    }

    #[tokio::test]
    async fn stoolap_passes_public_local_coordination_contract() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let report = aequora_testkit::contracts::verify_local_coordination(&backend)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(report.fencing_token.0 >= 3);
        assert!(report.store_generation.0 > 1);
    }

    #[tokio::test]
    async fn stoolap_crash_takeover_fences_old_commits_and_coordinates_generation() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let first_snapshot = backend
            .coordination_snapshot()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let now = unix_time_millis();
        let first = backend
            .acquire_lease(lease_request(
                aequora_coordination::ProcessInstanceId::new(),
                LeaseKind::SyncCoordinator,
                now,
                1_000,
            ))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let pending = operation(entity());
        backend
            .append_operation(pending.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let takeover_at = now.saturating_add(1_000);
        let second = backend
            .acquire_lease(lease_request(
                aequora_coordination::ProcessInstanceId::new(),
                LeaseKind::SyncCoordinator,
                takeover_at,
                1_000,
            ))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(second.fencing_token > first.fencing_token);
        assert!(
            backend
                .mark_sending_fenced(&[pending.operation_id], Some(first))
                .await
                .is_err()
        );
        assert_eq!(
            backend.operation_state(pending.operation_id).await,
            Ok(Some(OutboxState::Pending))
        );
        let scope = SyncScopeId::new();
        assert!(
            backend
                .reconcile_fenced(&empty_sync_response(scope), Some(first))
                .await
                .is_err()
        );
        assert_eq!(backend.load_cursor(scope).await, Ok(None));

        let maintenance_at = takeover_at.saturating_add(1_000);
        let maintenance = backend
            .acquire_lease(lease_request(
                aequora_coordination::ProcessInstanceId::new(),
                LeaseKind::Maintenance,
                maintenance_at,
                1_000,
            ))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let generation = backend
            .advance_store_generation(maintenance, maintenance_at)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(generation > first_snapshot.store_generation);
        assert!(
            backend
                .validate_fence(second, maintenance_at)
                .await
                .is_err()
        );
        let updated_maintenance = LeaseGrant {
            store_generation: generation,
            ..maintenance
        };
        backend
            .release_lease(updated_maintenance, maintenance_at)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let final_snapshot = backend
            .coordination_snapshot()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(final_snapshot.store_id, first_snapshot.store_id);
        assert_eq!(final_snapshot.store_generation, generation);
    }

    #[derive(Clone, Copy)]
    struct SnapshotProjectionHook;

    impl StoolapProjectionHook for SnapshotProjectionHook {
        fn apply_change(
            &self,
            _transaction: &mut ApiTransaction,
            _scope: SyncScopeId,
            _change: &RemoteChange,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        fn begin_snapshot(
            &self,
            transaction: &mut ApiTransaction,
            scope: SyncScopeId,
        ) -> Result<(), StoreError> {
            transaction
                .execute(
                    "DELETE FROM application_projection WHERE scope_id = $1",
                    (scope.to_string(),),
                )
                .map_err(stoolap_error)?;
            Ok(())
        }

        fn apply_snapshot_entity(
            &self,
            transaction: &mut ApiTransaction,
            scope: SyncScopeId,
            entity: &SnapshotEntity,
        ) -> Result<(), StoreError> {
            transaction
                .execute(
                    "INSERT INTO application_projection (scope_id, entity_id, payload)
                     VALUES ($1, $2, $3)",
                    (
                        scope.to_string(),
                        entity.entity.entity_id.to_string(),
                        String::from_utf8_lossy(&entity.payload).into_owned(),
                    ),
                )
                .map_err(stoolap_error)?;
            Ok(())
        }
    }

    async fn reopen_and_release_retry(
        dsn: &str,
        operation: &OperationEnvelope,
        retry_deadline: u64,
    ) -> StoolapDatabase {
        let backend = StoolapDatabase::open(dsn).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            backend.operation_state(operation.operation_id).await,
            Ok(Some(OutboxState::Retry))
        );
        assert_eq!(
            backend.retry_metadata(operation.operation_id).await,
            Ok(Some(RetryMetadata {
                attempt_count: 1,
                next_attempt_unix_ms: retry_deadline,
            }))
        );
        assert!(
            backend
                .pending_operations(10)
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .is_empty()
        );
        backend
            .mark_retry(&[operation.operation_id], 0)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            backend.retry_metadata(operation.operation_id).await,
            Ok(Some(RetryMetadata {
                attempt_count: 2,
                next_attempt_unix_ms: 0,
            }))
        );
        assert_eq!(
            backend
                .pending_operations(10)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            vec![operation.clone()]
        );
        backend
    }

    #[test]
    fn migration_record_verification_rejects_name_and_checksum_drift() {
        let migration = STOOLAP_MIGRATIONS[0];
        let checksum = migration_checksum(migration.sql);
        assert!(verify_migration_record(migration, migration.name, &checksum).is_ok());

        let renamed = verify_migration_record(migration, "rewritten_migration", &checksum)
            .err()
            .unwrap_or_else(|| panic!("renaming a published migration must fail"));
        assert_eq!(renamed.kind, StoreErrorKind::Permanent);

        let rewritten = verify_migration_record(migration, migration.name, &"0".repeat(64))
            .err()
            .unwrap_or_else(|| panic!("rewriting a published migration must fail"));
        assert_eq!(rewritten.kind, StoreErrorKind::Permanent);
    }

    #[test]
    fn persistent_legacy_schema_is_adopted_and_reopens_current() {
        let (_directory, dsn) = persistent_dsn("legacy-adoption");
        let database = Database::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        database
            .execute(MIGRATION_0001, ())
            .unwrap_or_else(|error| panic!("{error}"));
        drop(database);

        let backend = StoolapDatabase::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        backend
            .health_check()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            backend.schema_status(),
            Ok(StoolapSchemaStatus {
                applied_version: STOOLAP_SCHEMA_VERSION,
                expected_version: STOOLAP_SCHEMA_VERSION,
            })
        );
        let row = backend
            .database()
            .query(
                "SELECT version, name, checksum, applied_at FROM aequora_schema_migrations ORDER BY version DESC LIMIT 1",
                (),
            )
            .unwrap_or_else(|error| panic!("{error}"))
            .next()
            .unwrap_or_else(|| panic!("migration ledger row"))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            row.get::<i64>(0).unwrap_or_else(|error| panic!("{error}")),
            i64::from(STOOLAP_SCHEMA_VERSION)
        );
        assert_eq!(
            row.get::<String>(1)
                .unwrap_or_else(|error| panic!("{error}")),
            STOOLAP_MIGRATIONS
                .last()
                .map_or("", |migration| migration.name)
        );
        assert_eq!(
            row.get::<String>(2)
                .unwrap_or_else(|error| panic!("{error}"))
                .len(),
            64
        );
        assert!(
            !row.get::<String>(3)
                .unwrap_or_else(|error| panic!("{error}"))
                .is_empty()
        );

        backend.migrate().unwrap_or_else(|error| panic!("{error}"));
        drop(backend);
        let reopened = StoolapDatabase::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            reopened
                .database()
                .query_one::<i64, _>("SELECT COUNT(*) FROM aequora_schema_migrations", ())
                .unwrap_or_else(|error| panic!("{error}")),
            i64::from(STOOLAP_SCHEMA_VERSION)
        );
        reopened
            .health_check()
            .unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn interrupted_ddl_is_replayed_before_recording_migration() {
        let (_directory, dsn) = persistent_dsn("crash-replay");
        let database = Database::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        database
            .execute(MIGRATION_LEDGER_SQL, ())
            .unwrap_or_else(|error| panic!("{error}"));
        database
            .execute(MIGRATION_0001, ())
            .unwrap_or_else(|error| panic!("{error}"));
        drop(database);

        let recovered = StoolapDatabase::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        recovered
            .health_check()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            recovered
                .database()
                .query_one::<i64, _>("SELECT COUNT(*) FROM aequora_schema_migrations", ())
                .unwrap_or_else(|error| panic!("{error}")),
            i64::from(STOOLAP_SCHEMA_VERSION)
        );
    }

    #[test]
    fn interrupted_part_38_alter_migration_is_idempotently_recovered() {
        let (_directory, dsn) = persistent_dsn("part-38-alter-replay");
        let backend = StoolapDatabase::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        backend
            .database()
            .execute(
                "DELETE FROM aequora_schema_migrations WHERE version=$1",
                (i64::from(STOOLAP_SCHEMA_VERSION),),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        drop(backend);

        let recovered = StoolapDatabase::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            recovered.schema_status(),
            Ok(StoolapSchemaStatus {
                applied_version: STOOLAP_SCHEMA_VERSION,
                expected_version: STOOLAP_SCHEMA_VERSION,
            })
        );
        recovered
            .health_check()
            .unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn persistent_migration_checksum_drift_is_rejected() {
        let (_directory, dsn) = persistent_dsn("checksum-drift");
        let backend = StoolapDatabase::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        backend
            .database()
            .execute(
                "UPDATE aequora_schema_migrations SET checksum = $1 WHERE version = $2",
                ("corrupt", i64::from(STOOLAP_SCHEMA_VERSION)),
            )
            .unwrap_or_else(|error| panic!("{error}"));

        let error = backend
            .migrate()
            .err()
            .unwrap_or_else(|| panic!("a tampered migration ledger must fail"));
        assert_eq!(error.kind, StoreErrorKind::Permanent);
        assert!(error.message.contains("checksum drift"));
    }

    #[tokio::test]
    async fn built_in_stoolap_passes_the_public_local_adapter_contract() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let store = StoolapStore::new(backend);
        let operation = operation(entity());
        let scope = SyncScopeId::new();
        let report = verify_local_store(&store, operation.clone(), scope, operation.created_at)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(report.operation_id, operation.operation_id);
        assert_eq!(report.cursor.scope, scope);
    }

    #[tokio::test]
    async fn stoolap_and_reference_store_have_identical_local_contract_outcomes() {
        let operation = operation(entity());
        let server_time = operation.created_at;
        let scope = SyncScopeId::new();
        let reference = verify_local_store(
            &InMemoryLocalStore::default(),
            operation.clone(),
            scope,
            operation.created_at,
        )
        .await
        .unwrap_or_else(|error| panic!("reference contract failed: {error}"));
        let stoolap = verify_local_store(
            &StoolapStore::new(
                StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}")),
            ),
            operation,
            scope,
            server_time,
        )
        .await;
        let stoolap = stoolap.unwrap_or_else(|error| panic!("Stoolap contract failed: {error}"));
        assert_eq!(stoolap, reference);
    }

    #[tokio::test]
    async fn durable_stats_include_terminal_work_needing_attention() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let store = StoolapStore::new(backend);
        let rejected = operation(entity());
        let conflicted = operation(entity());
        store
            .append_operation(rejected.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        store
            .append_operation(conflicted.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let scope = SyncScopeId::new();
        store
            .reconcile(&SyncResponse {
                protocol: ProtocolVersion::V1,
                directive: SyncDirective::Continue,
                acknowledged: Vec::new(),
                rejected: vec![OperationRejection {
                    operation_id: rejected.operation_id,
                    code: RejectionCode::BusinessRule,
                    message: "attendance session is already submitted".into(),
                }],
                conflicts: vec![Conflict {
                    operation_id: conflicted.operation_id,
                    entity: conflicted.entity,
                    client_base: conflicted.base_version,
                    server_version: Some(EntityVersion::INITIAL),
                    policy: ConflictPolicy::ManualResolution,
                    message: "attendance changed on another device".into(),
                }],
                changes: Vec::new(),
                next_cursor: Cursor::legacy(scope, Sequence(0)),
                has_more: false,
                server_time: rejected.created_at,
            })
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let stats = store
            .outbox_stats()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(stats.replayable(), 0);
        assert_eq!(stats.rejected, 1);
        assert_eq!(
            store
                .unresolved_conflict_count()
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            1
        );
    }

    #[tokio::test]
    async fn domain_mutation_and_outbox_append_share_one_real_transaction() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        backend
            .database()
            .execute(
                "CREATE TABLE attendance (id INTEGER PRIMARY KEY AUTO_INCREMENT, external_id TEXT UNIQUE, status TEXT)",
                (),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let operation = operation(entity());
        let failed = backend.transact_local_mutation(&operation, |transaction| {
            transaction
                .execute(
                    "INSERT INTO attendance (external_id, status) VALUES ($1, $2)",
                    (operation.entity.entity_id.to_string(), "present"),
                )
                .map_err(stoolap_error)?;
            Err(StoreError::permanent("application validation failed"))
        });
        assert!(failed.is_err());
        let count = backend
            .database()
            .query_one::<i64, _>("SELECT COUNT(*) FROM attendance", ())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(count, 0);
        assert!(
            backend
                .pending_operations(10)
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .is_empty()
        );

        backend
            .transact_local_mutation(&operation, |transaction| {
                transaction
                    .execute(
                        "INSERT INTO attendance (external_id, status) VALUES ($1, $2)",
                        (operation.entity.entity_id.to_string(), "present"),
                    )
                    .map_err(stoolap_error)?;
                Ok(())
            })
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            backend
                .pending_operations(10)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            vec![operation]
        );
    }

    #[tokio::test]
    async fn reconciliation_failure_rolls_back_entity_ack_marker_and_cursor() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let entity = entity();
        let operation = operation(entity);
        backend
            .append_operation(operation.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let scope = SyncScopeId::new();
        let event_id = EventId::new();
        let lineage = operation
            .metadata
            .lineage
            .derived(LineageRef::Operation(operation.operation_id));
        let response = SyncResponse {
            protocol: ProtocolVersion::V1,
            directive: SyncDirective::Continue,
            acknowledged: vec![OperationAck {
                operation_id: operation.operation_id,
                event_id,
                lineage,
                entity_version: EntityVersion::INITIAL,
                sequence: Sequence(1),
                duplicate: false,
            }],
            rejected: Vec::new(),
            conflicts: Vec::new(),
            changes: vec![RemoteChange {
                tenant_id: operation.tenant_id,
                scope_id: scope,
                sequence: Sequence(1),
                operation_id: operation.operation_id,
                event_id,
                lineage,
                entity,
                version: EntityVersion::INITIAL,
                change_kind: ChangeKind::Upsert,
                payload: operation.payload.clone(),
                timestamp: operation.created_at,
            }],
            next_cursor: Cursor::legacy(scope, Sequence(u64::MAX)),
            has_more: false,
            server_time: operation.created_at,
        };

        assert!(backend.reconcile(&response).await.is_err());
        assert_eq!(
            backend.operation_state(operation.operation_id).await,
            Ok(Some(OutboxState::Pending))
        );
        assert_eq!(backend.load_cursor(scope).await, Ok(None));
        assert_eq!(
            backend
                .database()
                .query_one::<i64, _>("SELECT COUNT(*) FROM aequora_local_entities", ())
                .unwrap_or_else(|error| panic!("{error}")),
            0
        );
        assert_eq!(
            backend
                .database()
                .query_one::<i64, _>("SELECT COUNT(*) FROM aequora_applied_events", ())
                .unwrap_or_else(|error| panic!("{error}")),
            0
        );
    }

    #[tokio::test]
    async fn outbox_and_reconciliation_commit_survive_restart() {
        let (_directory, dsn) = persistent_dsn("durable-sync-boundaries");
        let entity = entity();
        let operation = operation(entity);
        let scope = SyncScopeId::new();
        let backend = StoolapDatabase::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        let retry_deadline = unix_time_millis().saturating_add(60_000);
        backend
            .append_operation(operation.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        backend
            .mark_retry(&[operation.operation_id], retry_deadline)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        drop(backend);

        let backend = reopen_and_release_retry(&dsn, &operation, retry_deadline).await;
        let event_id = EventId::new();
        let lineage = operation
            .metadata
            .lineage
            .derived(LineageRef::Operation(operation.operation_id));
        let response = SyncResponse {
            protocol: ProtocolVersion::V1,
            directive: SyncDirective::Continue,
            acknowledged: vec![OperationAck {
                operation_id: operation.operation_id,
                event_id,
                lineage,
                entity_version: EntityVersion::INITIAL,
                sequence: Sequence(1),
                duplicate: false,
            }],
            rejected: Vec::new(),
            conflicts: Vec::new(),
            changes: vec![RemoteChange {
                tenant_id: operation.tenant_id,
                scope_id: scope,
                sequence: Sequence(1),
                operation_id: operation.operation_id,
                event_id,
                lineage,
                entity,
                version: EntityVersion::INITIAL,
                change_kind: ChangeKind::Upsert,
                payload: operation.payload.clone(),
                timestamp: operation.created_at,
            }],
            next_cursor: Cursor::legacy(scope, Sequence(1)),
            has_more: false,
            server_time: operation.created_at,
        };
        backend
            .reconcile(&response)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        drop(backend);

        let reopened = StoolapDatabase::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            reopened.operation_state(operation.operation_id).await,
            Ok(Some(OutboxState::Acknowledged))
        );
        assert_eq!(
            reopened.load_cursor(scope).await,
            Ok(Some(response.next_cursor))
        );
        assert_eq!(
            reopened
                .database()
                .query_one::<i64, _>("SELECT COUNT(*) FROM aequora_applied_events", ())
                .unwrap_or_else(|error| panic!("{error}")),
            1
        );
        let stored_payload = reopened
            .database()
            .query_one::<String, _>("SELECT payload FROM aequora_local_entities", ())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(hex::decode(stored_payload), Ok(operation.payload));
    }

    #[tokio::test]
    async fn failed_final_snapshot_install_preserves_previous_scope_and_cursor() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let scope = SyncScopeId::new();
        let entity = entity();
        let timestamp = HybridTimestamp {
            physical_ms: 3,
            logical: 0,
            node: NodeId::new(),
        };
        let installed = BootstrapResponse {
            protocol: ProtocolVersion::V1,
            snapshot_id: SnapshotId::new(),
            cursor: Cursor::legacy(scope, Sequence(1)),
            offset: 0,
            entities: vec![SnapshotEntity {
                entity,
                version: EntityVersion::INITIAL,
                payload: b"installed".to_vec(),
                tombstone: false,
            }],
            next_offset: 1,
            has_more: false,
            server_time: timestamp,
        };
        backend
            .stage_snapshot(&installed)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let replacement_version = EntityVersion::INITIAL
            .checked_next()
            .unwrap_or_else(|| panic!("initial entity version must advance"));
        let replacement = BootstrapResponse {
            protocol: ProtocolVersion::V1,
            snapshot_id: SnapshotId::new(),
            cursor: Cursor::legacy(scope, Sequence(u64::MAX)),
            offset: 0,
            entities: vec![SnapshotEntity {
                entity,
                version: replacement_version,
                payload: b"must-not-install".to_vec(),
                tombstone: false,
            }],
            next_offset: 1,
            has_more: false,
            server_time: timestamp,
        };
        assert!(backend.stage_snapshot(&replacement).await.is_err());
        assert_eq!(backend.load_cursor(scope).await, Ok(Some(installed.cursor)));
        let stored_payload = backend
            .database()
            .query_one::<String, _>(
                "SELECT payload FROM aequora_local_entities WHERE scope_id = $1",
                (scope.to_string(),),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(hex::decode(stored_payload), Ok(b"installed".to_vec()));
        assert_eq!(
            backend
                .database()
                .query_one::<i64, _>("SELECT COUNT(*) FROM aequora_snapshot_staging", ())
                .unwrap_or_else(|error| panic!("{error}")),
            0
        );
    }

    #[tokio::test]
    async fn final_snapshot_replaces_application_projection_in_same_transaction() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        backend
            .database()
            .execute(
                "CREATE TABLE application_projection (
                    id INTEGER PRIMARY KEY AUTO_INCREMENT,
                    scope_id TEXT NOT NULL,
                    entity_id TEXT NOT NULL,
                    payload TEXT NOT NULL
                )",
                (),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let backend = backend.with_projection_hook(SnapshotProjectionHook);
        let scope = SyncScopeId::new();
        backend
            .database()
            .execute(
                "INSERT INTO application_projection (scope_id, entity_id, payload)
                 VALUES ($1, $2, $3)",
                (scope.to_string(), EntityId::new().to_string(), "stale"),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let replacement = BootstrapResponse {
            protocol: ProtocolVersion::V1,
            snapshot_id: SnapshotId::new(),
            cursor: Cursor::legacy(scope, Sequence(4)),
            offset: 0,
            entities: vec![SnapshotEntity {
                entity: entity(),
                version: EntityVersion::INITIAL,
                payload: b"authoritative".to_vec(),
                tombstone: false,
            }],
            next_offset: 1,
            has_more: false,
            server_time: HybridTimestamp {
                physical_ms: 4,
                logical: 0,
                node: NodeId::new(),
            },
        };

        backend
            .stage_snapshot(&replacement)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let payload = backend
            .database()
            .query_one::<String, _>(
                "SELECT payload FROM application_projection WHERE scope_id = $1",
                (scope.to_string(),),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(payload, "authoritative");
        assert_eq!(
            backend.load_cursor(scope).await,
            Ok(Some(replacement.cursor))
        );
    }

    #[tokio::test]
    async fn reconciliation_is_atomic_and_applied_events_are_idempotent() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let entity = entity();
        let operation = operation(entity);
        backend
            .append_operation(operation.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let scope = SyncScopeId::new();
        let timestamp = HybridTimestamp {
            physical_ms: 2,
            logical: 0,
            node: NodeId::new(),
        };
        let event_id = EventId::new();
        let lineage = operation
            .metadata
            .lineage
            .derived(LineageRef::Operation(operation.operation_id));
        let response = SyncResponse {
            protocol: ProtocolVersion::V1,
            directive: SyncDirective::Continue,
            acknowledged: vec![OperationAck {
                operation_id: operation.operation_id,
                event_id,
                lineage,
                entity_version: EntityVersion::INITIAL,
                sequence: Sequence(1),
                duplicate: false,
            }],
            rejected: Vec::new(),
            conflicts: Vec::new(),
            changes: vec![RemoteChange {
                tenant_id: operation.tenant_id,
                scope_id: scope,
                sequence: Sequence(1),
                operation_id: operation.operation_id,
                event_id,
                lineage,
                entity,
                version: EntityVersion::INITIAL,
                change_kind: ChangeKind::Upsert,
                payload: operation.payload.clone(),
                timestamp,
            }],
            next_cursor: Cursor::legacy(scope, Sequence(1)),
            has_more: false,
            server_time: timestamp,
        };
        backend
            .reconcile(&response)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        backend
            .reconcile(&response)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(
            backend.operation_state(operation.operation_id).await,
            Ok(Some(OutboxState::Acknowledged))
        );
        assert_eq!(
            backend.load_cursor(scope).await,
            Ok(Some(response.next_cursor))
        );
        let applied = backend
            .database()
            .query_one::<i64, _>("SELECT COUNT(*) FROM aequora_applied_events", ())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(applied, 1);
        let stored_payload = backend
            .database()
            .query_one::<String, _>("SELECT payload FROM aequora_local_entities", ())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(hex::decode(stored_payload), Ok(operation.payload));
    }

    #[tokio::test]
    async fn replacement_erasure_is_scope_and_device_bounded() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let revoked_scope = SyncScopeId::new();
        let retained_scope = SyncScopeId::new();
        let revoked = operation(entity());
        let retained = operation(entity());
        backend
            .append_operation(revoked.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        backend
            .append_operation(retained.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        backend
            .database()
            .execute(
                "INSERT INTO aequora_cursors (scope_id,sequence) VALUES ($1,$2)",
                (revoked_scope.to_string(), 3_i64),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        backend
            .database()
            .execute(
                "INSERT INTO aequora_cursors (scope_id,sequence) VALUES ($1,$2)",
                (retained_scope.to_string(), 4_i64),
            )
            .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(backend.discard_device_operations(revoked.device_id), Ok(1));
        backend
            .erase_scope_cache(revoked_scope)
            .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(
            backend
                .pending_operations(10)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            vec![retained]
        );
        assert_eq!(backend.load_cursor(revoked_scope).await, Ok(None));
        assert_eq!(
            backend.load_cursor(retained_scope).await,
            Ok(Some(Cursor::legacy(retained_scope, Sequence(4))))
        );
    }

    #[tokio::test]
    async fn integrity_repair_is_atomic_and_preserves_pending_operations() {
        let store = StoolapStore::new(
            StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}")),
        );
        let scope = SyncScopeId::new();
        let entity = entity();
        let boundary = Cursor::legacy(scope, Sequence(7));
        store
            .stage_snapshot(&BootstrapResponse {
                protocol: ProtocolVersion::V1,
                snapshot_id: SnapshotId::new(),
                cursor: boundary,
                offset: 0,
                entities: vec![SnapshotEntity {
                    entity,
                    version: EntityVersion::INITIAL,
                    payload: b"before-repair".to_vec(),
                    tombstone: false,
                }],
                next_offset: 1,
                has_more: false,
                server_time: HybridTimestamp {
                    physical_ms: 7,
                    logical: 0,
                    node: NodeId::new(),
                },
            })
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let pending = operation(entity);
        store
            .append_operation(pending.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let before = store
            .capture_local_integrity(
                scope,
                boundary,
                aequora_integrity::CURRENT_INTEGRITY_GENERATION,
                PartitionScheme::new(8).unwrap_or_else(|error| panic!("{error}")),
                100,
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let expected_before =
            expected_integrity_snapshot(scope, boundary, entity, b"before-repair");
        assert_eq!(before, expected_before);
        let plan = RepairPlan {
            repair_id: aequora_types::RepairId::new(),
            boundary,
            affected_entities: vec![entity],
            strategy: RepairStrategy::ReplaceEntities,
        };
        let report = store
            .repair_local_replica(
                &plan,
                &[CanonicalEntity {
                    entity,
                    version: EntityVersion::INITIAL,
                    hash_schema: CURRENT_HASH_SCHEMA,
                    payload: b"after-repair".to_vec(),
                    tombstone: false,
                }],
                &[],
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let after = store
            .capture_local_integrity(
                scope,
                boundary,
                aequora_integrity::CURRENT_INTEGRITY_GENERATION,
                PartitionScheme::new(8).unwrap_or_else(|error| panic!("{error}")),
                100,
            )
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let expected_after = expected_integrity_snapshot(scope, boundary, entity, b"after-repair");

        assert_ne!(before.manifest.root_hash, after.manifest.root_hash);
        assert_eq!(after, expected_after);
        assert_eq!(report.sync_cursor, boundary);
        assert_eq!(
            report.preserved_pending_operations,
            vec![pending.operation_id]
        );
        assert_eq!(
            store
                .pending_operations(10)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            vec![pending]
        );
    }

    #[tokio::test]
    async fn queue_compaction_and_rebase_are_atomic_and_never_rewrite_sent_intent() {
        let store = StoolapStore::new(
            StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}")),
        );
        let entity = entity();
        let operations = same_lineage_operations(entity, 3);
        for operation in &operations {
            store
                .append_operation(operation.clone())
                .await
                .unwrap_or_else(|error| panic!("{error}"));
        }
        let registry = replace_latest_registry(operations[0].operation_kind.0);
        let plan = store
            .compact_outbox(&registry, 10)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(plan.operations_after, 1);
        assert_eq!(plan.supersessions.len(), 2);
        assert_eq!(
            store
                .pending_operations(10)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            vec![operations[2].clone()]
        );

        store
            .mark_sending(&[operations[2].operation_id])
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let target = RebaseTarget {
            entity,
            version: EntityVersion::INITIAL.checked_next(),
            changed_fields: std::collections::BTreeSet::new(),
        };
        let rebase = store
            .rebase_outbox(&registry, &[target], 10)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(rebase.rewrites.is_empty());
        assert_eq!(rebase.immutable_skipped, vec![operations[2].operation_id]);
    }

    #[tokio::test]
    async fn failed_compaction_transaction_restores_the_complete_original_queue() {
        let store = StoolapStore::new(
            StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}")),
        );
        let operations = same_lineage_operations(entity(), 3);
        for operation in &operations {
            store
                .append_operation(operation.clone())
                .await
                .unwrap_or_else(|error| panic!("{error}"));
        }
        store
            .backend()
            .database
            .execute(
                "INSERT INTO aequora_supersession (old_operation_id, new_operation_id, reason) VALUES ($1, NULL, 'fixture')",
                (operations[1].operation_id.to_string(),),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let registry = replace_latest_registry(operations[0].operation_kind.0);
        assert!(store.compact_outbox(&registry, 10).await.is_err());
        assert_eq!(
            store
                .pending_operations(10)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            operations
        );
    }

    #[tokio::test]
    async fn part_38_claim_is_bounded_digest_bound_and_crash_retryable() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let first = operation(entity());
        let second = operation(entity());
        backend
            .append_operation(first.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        backend
            .append_operation(second)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let claimed = backend
            .claim_operations_single_process(ProcessInstanceId::new(), 1)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].operation, first);
        assert_eq!(
            claimed[0].payload_digest,
            semantic_envelope_hash(&first).unwrap_or_else(|error| panic!("{error}"))
        );
        assert_eq!(
            backend
                .operation_state(first.operation_id)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            Some(OutboxState::Sending)
        );
        assert_eq!(
            backend
                .recover_stale_claims_single_process(i64::MAX as u64)
                .unwrap_or_else(|error| panic!("{error}")),
            1
        );
        let retried = backend
            .claim_operations_single_process(ProcessInstanceId::new(), 1)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(retried[0].operation.operation_id, first.operation_id);
        assert_eq!(retried[0].payload_digest, claimed[0].payload_digest);
    }

    #[tokio::test]
    async fn part_38_corrupt_claim_digest_rolls_back_the_complete_claim() {
        let backend = StoolapDatabase::open_in_memory().unwrap_or_else(|error| panic!("{error}"));
        let operation = operation(entity());
        backend
            .append_operation(operation.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        backend
            .database
            .execute(
                "UPDATE aequora_outbox SET payload_digest=$1 WHERE operation_id=$2",
                ("00".repeat(32), operation.operation_id.to_string()),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            backend
                .claim_operations_single_process(ProcessInstanceId::new(), 1)
                .is_err()
        );
        assert_eq!(
            backend
                .operation_state(operation.operation_id)
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            Some(OutboxState::Pending)
        );
    }

    #[test]
    fn part_38_identity_scheduler_health_and_storage_policy_are_durable() {
        let (_directory, dsn) = persistent_dsn("part-38-identity.db");
        let backend = StoolapDatabase::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        let identity = backend
            .local_identity()
            .unwrap_or_else(|error| panic!("{error}"));
        let rebound = backend
            .rebind_device(identity.device_binding_generation, DeviceId::new())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            rebound.device_binding_generation,
            identity.device_binding_generation + 1
        );
        let checkpoint = scheduler::SchedulerCheckpoint {
            next_attempt_unix_ms: 11,
            retry_after_unix_ms: 12,
            circuit_open_until_unix_ms: 13,
            data_budget_used_bytes: 14,
        };
        backend
            .store_scheduler_checkpoint(checkpoint)
            .unwrap_or_else(|error| panic!("{error}"));
        drop(backend);

        let reopened = StoolapDatabase::open(&dsn).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            reopened
                .local_identity()
                .unwrap_or_else(|error| panic!("{error}")),
            rebound
        );
        assert_eq!(
            reopened
                .scheduler_checkpoint()
                .unwrap_or_else(|error| panic!("{error}")),
            checkpoint
        );
        assert!(
            reopened
                .structured_health()
                .unwrap_or_else(|error| panic!("{error}"))
                .ready()
        );
        assert_eq!(
            storage::preflight_storage(100, 90, 20, 15),
            storage::StoragePreflight::ReclaimEvictable { bytes: 10 }
        );
        assert_eq!(
            backup::validate_restore_binding(2, 3, backup::SecureKeyState::Available),
            backup::RestoreDisposition::RebindDevice
        );
        assert_eq!(STOOLAP_LOCAL_CORE_PROFILE, "StoolapLocalCore");
        assert_eq!(
            STOOLAP_DESKTOP_LOCAL_FULL_PROFILE,
            "StoolapDesktopLocalFull"
        );
        assert_eq!(STOOLAP_MOBILE_LOCAL_FULL_PROFILE, "StoolapMobileLocalFull");
    }
}
