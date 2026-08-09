//! Stoolap local-store boundary and schema.
//!
//! Domain repositories should use the same Stoolap transaction for their optimistic write
//! and `aequora_outbox` insert. Reconciliation similarly remains one backend transaction.

use aequora_protocol::{
    BootstrapResponse, Conflict, OperationEnvelope, SnapshotEntity, SyncResponse,
};
use aequora_store::{
    ConflictInbox, ConflictRecord, ConflictResolution, CursorStore, OutboxState, OutboxStateStore,
    OutboxStats, OutboxStore, ReconciliationStore, SnapshotProgress, StoreError,
};
use aequora_types::{Cursor, OperationId, Sequence, SnapshotId, SyncScopeId};
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::str::FromStr;
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
pub const STOOLAP_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy)]
struct StoolapMigration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

const STOOLAP_MIGRATIONS: &[StoolapMigration] = &[StoolapMigration {
    version: 1,
    name: "initial_local_replica_schema",
    sql: MIGRATION_0001,
}];

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
}

impl StoolapDatabase {
    /// Opens a Stoolap DSN and installs Aequora metadata tables.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the database cannot open or migration SQL fails.
    pub fn open(dsn: &str) -> Result<Self, StoreError> {
        let database = Database::open(dsn).map_err(stoolap_error)?;
        let backend = Self { database };
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
        let backend = Self { database };
        backend.migrate()?;
        Ok(backend)
    }

    /// Borrows the underlying database for application repository reads.
    #[must_use]
    pub const fn database(&self) -> &Database {
        &self.database
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

    /// Verifies both local database availability and schema compatibility.
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
    async fn mark_retry(&self, operations: &[OperationId]) -> Result<(), StoreError>;
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
        let rows = self
            .database
            .query(
                "SELECT envelope FROM aequora_outbox WHERE state IN ('pending', 'sending', 'retry') ORDER BY enqueued_order LIMIT $1",
                (limit,),
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

    async fn mark_retry(&self, operations: &[OperationId]) -> Result<(), StoreError> {
        transition_operations(&self.database, operations, OutboxState::Retry)
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
                "SELECT state, envelope FROM aequora_outbox WHERE state IN ('pending', 'sending', 'retry') ORDER BY enqueued_order",
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
                _ => continue,
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

    async fn mark_retry(&self, operations: &[OperationId]) -> Result<(), StoreError> {
        self.backend.mark_retry(operations).await
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
        ChangeKind, OperationAck, OperationKind, OperationMetadata, RemoteChange, SyncDirective,
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
                "SELECT version, name, checksum, applied_at FROM aequora_schema_migrations",
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
            STOOLAP_MIGRATIONS[0].name
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
            1
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
            1
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
}
