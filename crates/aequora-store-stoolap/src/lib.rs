//! Stoolap local-store boundary and schema.
//!
//! Domain repositories should use the same Stoolap transaction for their optimistic write
//! and `aequora_outbox` insert. Reconciliation similarly remains one backend transaction.

use aequora_protocol::{
    BootstrapResponse, Conflict, OperationEnvelope, SnapshotEntity, SyncResponse,
};
use aequora_store::{
    ConflictInbox, ConflictRecord, ConflictResolution, CursorStore, OutboxState, OutboxStateStore,
    OutboxStats, OutboxStore, ReconciliationStore, RetryMetadata, SnapshotProgress, StoreError,
    TransactionCapabilities, TransactionCapabilityProvider,
};
use aequora_types::{
    Cursor, EntityId, EntityRef, EntityType, EntityVersion, OperationId, Sequence, SnapshotId,
    SyncScopeId,
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
pub const STOOLAP_SCHEMA_VERSION: u32 = 2;

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
        drop(transaction);
        Ok(())
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
        database.execute(migration.sql, ()).map_err(stoolap_error)?;
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
    Ok(())
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
    transaction
        .execute(
            "INSERT INTO aequora_outbox (operation_id, enqueued_order, envelope, state) VALUES ($1, $2, $3, 'pending')",
            (&operation_id, next_order, &envelope),
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
    /// Atomically transitions selected replayable operations to `Sending`.
    async fn mark_sending(&self, operations: &[OperationId]) -> Result<(), StoreError>;
    /// Returns in-flight operations to the replayable `Retry` state.
    async fn mark_retry(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
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
    /// Atomically performs all reconciliation effects.
    async fn reconcile(&self, response: &SyncResponse) -> Result<(), StoreError>;
    /// Stages and, for the final page, atomically installs a bootstrap snapshot.
    async fn stage_snapshot(&self, response: &BootstrapResponse) -> Result<(), StoreError>;
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
                "SELECT envelope FROM aequora_outbox WHERE state IN ('pending', 'sending', 'retry') AND operation_id NOT IN (SELECT operation_id FROM aequora_retry_schedule WHERE next_attempt_unix_ms > $1) ORDER BY enqueued_order LIMIT $2",
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

    async fn mark_sending(&self, operations: &[OperationId]) -> Result<(), StoreError> {
        transition_operations(&self.database, operations, OutboxState::Sending)
    }

    async fn mark_retry(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
    ) -> Result<(), StoreError> {
        schedule_retry(&self.database, operations, next_attempt_unix_ms)
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
        let sequence = self
            .database
            .query_opt::<i64, _>(
                "SELECT sequence FROM aequora_cursors WHERE scope_id = $1",
                (scope.to_string(),),
            )
            .map_err(stoolap_error)?;
        sequence
            .map(|sequence| {
                Ok(Cursor {
                    scope,
                    sequence: Sequence(
                        u64::try_from(sequence).map_err(|_| {
                            StoreError::permanent("negative Stoolap cursor sequence")
                        })?,
                    ),
                })
            })
            .transpose()
    }

    async fn reconcile(&self, response: &SyncResponse) -> Result<(), StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        let scope = response.next_cursor.scope;
        if let Some(current) = load_cursor_transaction(&mut transaction, scope)? {
            if response.next_cursor.sequence < current.sequence {
                return Err(StoreError::permanent(
                    "Stoolap reconciliation cursor would regress",
                ));
            }
        }
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
                self.projection_hook
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

    async fn stage_snapshot(&self, response: &BootstrapResponse) -> Result<(), StoreError> {
        let mut transaction = self.database.begin().map_err(stoolap_error)?;
        let scope = response.cursor.scope;
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
            self.projection_hook
                .begin_snapshot(&mut transaction, scope)?;
            let snapshot_entities = load_staged_snapshot_entities(&mut transaction, scope)?;
            for entity in &snapshot_entities {
                self.projection_hook
                    .apply_snapshot_entity(&mut transaction, scope, entity)?;
            }
            self.projection_hook
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

fn transition_operations(
    database: &Database,
    operations: &[OperationId],
    next: OutboxState,
) -> Result<(), StoreError> {
    let mut transaction = database.begin().map_err(stoolap_error)?;
    for operation in operations {
        transaction
            .execute(
                "UPDATE aequora_outbox SET state = $1 WHERE operation_id = $2 AND state IN ('pending', 'sending', 'retry')",
                (state_name(next), operation.to_string()),
            )
            .map_err(stoolap_error)?;
    }
    transaction.commit().map_err(stoolap_error)
}

fn schedule_retry(
    database: &Database,
    operations: &[OperationId],
    next_attempt_unix_ms: u64,
) -> Result<(), StoreError> {
    let next_attempt = to_i64(next_attempt_unix_ms, "retry timestamp")?;
    let mut transaction = database.begin().map_err(stoolap_error)?;
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
        transaction
            .execute(
                "UPDATE aequora_outbox SET state = 'retry' WHERE operation_id = $1",
                (&operation,),
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
    let sequence = transaction
        .query_opt::<i64, _>(
            "SELECT sequence FROM aequora_cursors WHERE scope_id = $1",
            (scope.to_string(),),
        )
        .map_err(stoolap_error)?;
    sequence
        .map(|value| {
            Ok(Cursor {
                scope,
                sequence: Sequence(
                    u64::try_from(value)
                        .map_err(|_| StoreError::permanent("negative Stoolap cursor sequence"))?,
                ),
            })
        })
        .transpose()
}

fn set_cursor(transaction: &mut ApiTransaction, cursor: Cursor) -> Result<(), StoreError> {
    let scope = cursor.scope.to_string();
    let sequence = to_i64(cursor.sequence.0, "cursor sequence")?;
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
                "UPDATE aequora_cursors SET sequence = $1 WHERE scope_id = $2",
                (sequence, &scope),
            )
            .map_err(stoolap_error)?;
    } else {
        transaction
            .execute(
                "INSERT INTO aequora_cursors (scope_id, sequence) VALUES ($1, $2)",
                (&scope, sequence),
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
            "SELECT snapshot_id, cursor_sequence, next_offset FROM aequora_snapshot_progress WHERE scope_id = $1",
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
    Ok(Some(SnapshotProgress {
        snapshot_id: parse_id::<SnapshotId>(&snapshot, "snapshot ID")?,
        cursor: Cursor {
            scope,
            sequence: Sequence(
                u64::try_from(cursor)
                    .map_err(|_| StoreError::permanent("negative snapshot cursor"))?,
            ),
        },
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
    if snapshot_progress_transaction(transaction, progress.cursor.scope)?.is_some() {
        transaction
            .execute(
                "UPDATE aequora_snapshot_progress SET snapshot_id = $1, cursor_sequence = $2, next_offset = $3 WHERE scope_id = $4",
                (&snapshot, cursor, offset, &scope),
            )
            .map_err(stoolap_error)?;
    } else {
        transaction
            .execute(
                "INSERT INTO aequora_snapshot_progress (scope_id, snapshot_id, cursor_sequence, next_offset) VALUES ($1, $2, $3, $4)",
                (&scope, &snapshot, cursor, offset),
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

#[async_trait]
impl<B: StoolapBackend> OutboxStore for StoolapStore<B> {
    async fn pending_operations(&self, limit: usize) -> Result<Vec<OperationEnvelope>, StoreError> {
        self.backend.pending_operations(limit).await
    }
    async fn append_operation(&self, operation: OperationEnvelope) -> Result<(), StoreError> {
        self.backend.append_operation(operation).await
    }
}

#[async_trait]
impl<B: StoolapBackend> OutboxStateStore for StoolapStore<B> {
    async fn mark_sending(&self, operations: &[OperationId]) -> Result<(), StoreError> {
        self.backend.mark_sending(operations).await
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
impl<B: StoolapBackend> ReconciliationStore for StoolapStore<B> {
    async fn reconcile(&self, response: &SyncResponse) -> Result<(), StoreError> {
        self.backend.reconcile(response).await
    }

    async fn stage_snapshot(&self, response: &BootstrapResponse) -> Result<(), StoreError> {
        self.backend.stage_snapshot(response).await
    }

    async fn snapshot_progress(
        &self,
        scope: SyncScopeId,
    ) -> Result<Option<SnapshotProgress>, StoreError> {
        self.backend.snapshot_progress(scope).await
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
    use aequora_testkit::contracts::verify_local_store;
    use aequora_types::{
        ActorId, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, HybridTimestamp, NodeId,
        ProtocolVersion, SchemaVersion, TenantId,
    };
    use tempfile::tempdir;

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

    fn entity() -> EntityRef {
        EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
            entity_id: EntityId::new(),
        }
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
                next_cursor: Cursor {
                    scope,
                    sequence: Sequence(0),
                },
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
        let response = SyncResponse {
            protocol: ProtocolVersion::V1,
            directive: SyncDirective::Continue,
            acknowledged: vec![OperationAck {
                operation_id: operation.operation_id,
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
                entity,
                version: EntityVersion::INITIAL,
                change_kind: ChangeKind::Upsert,
                payload: operation.payload.clone(),
                timestamp: operation.created_at,
            }],
            next_cursor: Cursor {
                scope,
                sequence: Sequence(u64::MAX),
            },
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
        let response = SyncResponse {
            protocol: ProtocolVersion::V1,
            directive: SyncDirective::Continue,
            acknowledged: vec![OperationAck {
                operation_id: operation.operation_id,
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
                entity,
                version: EntityVersion::INITIAL,
                change_kind: ChangeKind::Upsert,
                payload: operation.payload.clone(),
                timestamp: operation.created_at,
            }],
            next_cursor: Cursor {
                scope,
                sequence: Sequence(1),
            },
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
            cursor: Cursor {
                scope,
                sequence: Sequence(1),
            },
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
            cursor: Cursor {
                scope,
                sequence: Sequence(u64::MAX),
            },
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
            cursor: Cursor {
                scope,
                sequence: Sequence(4),
            },
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
        let response = SyncResponse {
            protocol: ProtocolVersion::V1,
            directive: SyncDirective::Continue,
            acknowledged: vec![OperationAck {
                operation_id: operation.operation_id,
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
                entity,
                version: EntityVersion::INITIAL,
                change_kind: ChangeKind::Upsert,
                payload: operation.payload.clone(),
                timestamp,
            }],
            next_cursor: Cursor {
                scope,
                sequence: Sequence(1),
            },
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
            Ok(Some(Cursor {
                scope: retained_scope,
                sequence: Sequence(4),
            }))
        );
    }
}
