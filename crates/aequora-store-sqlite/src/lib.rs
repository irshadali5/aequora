//! Official portable `SQLite` local-replica adapter.
//!
//! The adapter owns synchronization metadata beside application tables in one WAL database.
//! [`SQLiteDatabase::transact_local_mutation`] is Tx A (domain row plus outbox), while
//! [`ReconciliationStore::reconcile`] is Tx C (authoritative apply plus cursor).

pub mod backup;
pub mod capabilities;
pub mod config;

pub use backup::{ForensicCopy, RecoveryState};
pub use capabilities::{
    SQLITE_DESKTOP_LOCAL_FULL_PROFILE, SQLITE_LOCAL_ADAPTER_IDENTITY, SQLITE_LOCAL_CORE_PROFILE,
    SQLITE_MOBILE_LOCAL_FULL_PROFILE,
};
pub use config::{SQLiteConfig, SQLitePlatform};

use aequora_adapter_sdk as adapter_sdk;
use aequora_protocol::{
    BootstrapResponse, ChangeKind, OperationEnvelope, SnapshotEntity, SyncResponse,
};
use aequora_queue::OptimizationRegistry;
use aequora_store::{
    AdapterCapabilities, AdapterManifest, AdapterManifestProvider, AdapterRole, AdapterTier,
    ConflictInbox, ConflictRecord, ConflictResolution, CursorStore, OutboxState, OutboxStateStore,
    OutboxStats, OutboxStore, ReconciliationStore, RetryMetadata, SnapshotProgress, StoreError,
    TransactionCapabilities, TransactionCapabilityProvider,
};
use aequora_types::{
    AuthorityEpoch, AuthorityId, Cursor, EntityType, OperationId, Sequence, SnapshotId, SyncScopeId,
};
use async_trait::async_trait;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Latest `SQLite` metadata schema understood by this release.
pub const SQLITE_SCHEMA_VERSION: u32 = 1;

/// Required `SQLite` schema for durable local synchronization state.
pub const SQLITE_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS operation_outbox (
    local_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL UNIQUE,
    envelope BLOB NOT NULL,
    payload_digest BLOB NOT NULL CHECK(length(payload_digest) = 32),
    state TEXT NOT NULL CHECK(state IN ('pending','sending','acknowledged','rejected','conflict','retry')),
    ever_sent INTEGER NOT NULL DEFAULT 0 CHECK(ever_sent IN (0,1)),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count >= 0),
    next_attempt_unix_ms INTEGER NOT NULL DEFAULT 0 CHECK(next_attempt_unix_ms >= 0),
    in_flight_owner TEXT,
    in_flight_since_unix_ms INTEGER,
    terminal_detail BLOB,
    created_at_physical_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS operation_outbox_ready_idx
    ON operation_outbox(state, next_attempt_unix_ms, local_sequence);

CREATE TABLE IF NOT EXISTS sync_cursor (
    scope_id TEXT PRIMARY KEY,
    authority_id TEXT NOT NULL,
    authority_epoch INTEGER NOT NULL CHECK(authority_epoch > 0),
    sequence INTEGER NOT NULL CHECK(sequence >= 0),
    updated_at_unix_ms INTEGER NOT NULL CHECK(updated_at_unix_ms >= 0)
) STRICT;

CREATE TABLE IF NOT EXISTS applied_authoritative_change (
    scope_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    PRIMARY KEY(scope_id, sequence)
) STRICT;

CREATE TABLE IF NOT EXISTS local_entity (
    scope_id TEXT NOT NULL,
    entity_type INTEGER NOT NULL CHECK(entity_type > 0),
    entity_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK(version > 0),
    payload BLOB NOT NULL,
    tombstone INTEGER NOT NULL CHECK(tombstone IN (0,1)),
    PRIMARY KEY(scope_id, entity_type, entity_id)
) STRICT;

CREATE TABLE IF NOT EXISTS conflict_ledger (
    operation_id TEXT PRIMARY KEY,
    detail BLOB NOT NULL,
    resolution TEXT,
    created_at_unix_ms INTEGER NOT NULL CHECK(created_at_unix_ms >= 0)
) STRICT;
CREATE INDEX IF NOT EXISTS conflict_ledger_unresolved_idx
    ON conflict_ledger(resolution, created_at_unix_ms, operation_id);

CREATE TABLE IF NOT EXISTS scope_cache (
    scope_id TEXT NOT NULL,
    cache_key TEXT NOT NULL,
    value BLOB NOT NULL,
    reconstructable INTEGER NOT NULL CHECK(reconstructable IN (0,1)),
    PRIMARY KEY(scope_id, cache_key)
) STRICT;

CREATE TABLE IF NOT EXISTS blob_manifest (
    blob_id TEXT PRIMARY KEY,
    digest BLOB NOT NULL CHECK(length(digest) = 32),
    size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0),
    available INTEGER NOT NULL CHECK(available IN (0,1)),
    pinned INTEGER NOT NULL CHECK(pinned IN (0,1)),
    last_access_unix_ms INTEGER NOT NULL CHECK(last_access_unix_ms >= 0)
) STRICT;

CREATE TABLE IF NOT EXISTS snapshot_progress (
    scope_id TEXT PRIMARY KEY,
    snapshot_id TEXT NOT NULL,
    authority_id TEXT NOT NULL,
    authority_epoch INTEGER NOT NULL CHECK(authority_epoch > 0),
    cursor_sequence INTEGER NOT NULL CHECK(cursor_sequence >= 0),
    next_offset INTEGER NOT NULL CHECK(next_offset >= 0)
) STRICT;

CREATE TABLE IF NOT EXISTS snapshot_staging (
    scope_id TEXT NOT NULL,
    entity_type INTEGER NOT NULL CHECK(entity_type > 0),
    entity_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK(version > 0),
    payload BLOB NOT NULL,
    tombstone INTEGER NOT NULL CHECK(tombstone IN (0,1)),
    PRIMARY KEY(scope_id, entity_type, entity_id)
) STRICT;
";

/// Versioned role and capability declaration for the built-in `SQLite` adapter.
pub const SQLITE_ADAPTER_MANIFEST: AdapterManifest = AdapterManifest {
    name: "sqlite",
    adapter_version: env!("CARGO_PKG_VERSION"),
    tested_aequora_version: env!("CARGO_PKG_VERSION"),
    tested_database_versions: &["SQLite 3.46 (bundled)"],
    roles: &[
        AdapterRole::LocalWritable,
        AdapterRole::ReplicaSink,
        AdapterRole::SnapshotSink,
    ],
    tier: AdapterTier::FullProduction,
    capabilities: AdapterCapabilities::FULL_LOCAL,
    limitations: &[],
};

const SQLITE_SDK_ROLES: &[adapter_sdk::AdapterRole] = &[
    adapter_sdk::AdapterRole::LocalReplicaStore,
    adapter_sdk::AdapterRole::SnapshotStore,
];

const SQLITE_SDK_CAPABILITIES: &[adapter_sdk::AdapterCapability] = &[
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
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::BACKUP),
];

/// Machine-readable Part 36 manifest for the official `SQLite` adapter.
pub const SQLITE_STORAGE_ADAPTER_MANIFEST: adapter_sdk::AdapterManifest =
    adapter_sdk::AdapterManifest {
        descriptor: adapter_sdk::AdapterDescriptor {
            adapter_id: adapter_sdk::AdapterId(0xae03),
            name: "aequora-sqlite-local",
            version: adapter_sdk::AdapterVersion::new(0, 1, 0),
            store_kind: adapter_sdk::StoreKind::Embedded,
        },
        roles: SQLITE_SDK_ROLES,
        capabilities: SQLITE_SDK_CAPABILITIES,
        support: adapter_sdk::AdapterSupport::Official,
        supported_engine_versions: &["SQLite 3.46 (bundled)"],
        supported_targets: &["x86_64-unknown-linux-gnu"],
        known_limitations: &[
            "non-Linux desktop and mobile targets require target-bound certification",
            "one logical writer must own synchronization mutations",
        ],
        concurrency: adapter_sdk::ConcurrencyModel::SingleWriterMultiProcess,
        maintainer_owned: true,
        release_evidence_complete: true,
    };

/// Application projection callbacks executed inside `SQLite` Tx A and Tx C.
pub trait SQLiteProjectionHook: Send + Sync {
    /// Applies an authoritative change to application-owned domain tables.
    ///
    /// # Errors
    ///
    /// Returning an error rolls back the complete reconciliation transaction.
    fn apply_change(
        &self,
        transaction: &Transaction<'_>,
        scope: SyncScopeId,
        change: &aequora_protocol::RemoteChange,
    ) -> Result<(), StoreError>;

    /// Applies one final snapshot entity to application-owned domain tables.
    ///
    /// # Errors
    ///
    /// Returning an error rolls back snapshot activation and cursor installation.
    fn apply_snapshot_entity(
        &self,
        _transaction: &Transaction<'_>,
        _scope: SyncScopeId,
        _entity: &SnapshotEntity,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct NoopProjectionHook;

impl SQLiteProjectionHook for NoopProjectionHook {
    fn apply_change(
        &self,
        _transaction: &Transaction<'_>,
        _scope: SyncScopeId,
        _change: &aequora_protocol::RemoteChange,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

/// Structured `SQLite` health state. Corrupt files are preserved rather than auto-deleted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SQLiteHealth {
    /// Active journal mode reported by `SQLite`.
    pub journal_mode: String,
    /// Result of `PRAGMA integrity_check`.
    pub integrity: String,
    /// Installed adapter schema version.
    pub schema_version: u32,
    /// Whether the adapter satisfies production open requirements.
    pub ready: bool,
}

/// Bounded metadata for a blob whose bytes are stored incrementally outside the main database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SQLiteBlobManifest {
    /// Stable application blob identity.
    pub blob_id: String,
    /// Canonical BLAKE3 digest of the complete blob.
    pub digest: [u8; 32],
    /// Expected complete size.
    pub size_bytes: u64,
    /// Whether all verified bytes are locally available.
    pub available: bool,
    /// Whether pending intent prevents eviction.
    pub pinned: bool,
}

/// Official SQLite-backed local replica with one serialized writer and WAL readers.
#[derive(Clone)]
pub struct SQLiteDatabase {
    path: Arc<PathBuf>,
    writer: Arc<Mutex<Connection>>,
    projection_hook: Arc<dyn SQLiteProjectionHook>,
    busy_timeout: Duration,
    production: bool,
}

impl SQLiteDatabase {
    /// Opens, configures, migrates, and verifies a durable `SQLite` replica.
    ///
    /// # Errors
    ///
    /// Returns a permanent storage error when configuration, WAL activation, schema migration, or
    /// integrity verification fails.
    pub fn open(config: SQLiteConfig) -> Result<Self, StoreError> {
        config.validate().map_err(StoreError::permanent)?;
        let mut connection = open_connection(&config.database_path, config.busy_timeout, false)?;
        configure_writer(&mut connection, config.production)?;
        migrate(&mut connection)?;
        let integrity = integrity_check(&connection)?;
        if integrity != "ok" {
            return Err(StoreError::permanent(format!(
                "SQLite integrity check failed: {integrity}"
            )));
        }
        Ok(Self {
            path: Arc::new(config.database_path),
            writer: Arc::new(Mutex::new(connection)),
            projection_hook: Arc::new(NoopProjectionHook),
            busy_timeout: config.busy_timeout,
            production: config.production,
        })
    }

    /// Replaces the no-op domain projection with an application-owned transactional hook.
    #[must_use]
    pub fn with_projection_hook(mut self, hook: Arc<dyn SQLiteProjectionHook>) -> Self {
        self.projection_hook = hook;
        self
    }

    /// Runs Tx A: application domain updates and the immutable outbox intent commit together.
    ///
    /// # Errors
    ///
    /// Returns an error and rolls back both effects when the domain hook, outbox insert, digest,
    /// or `SQLite` commit fails.
    pub fn transact_local_mutation<F>(
        &self,
        operation: &OperationEnvelope,
        mutate_domain: F,
    ) -> Result<(), StoreError>
    where
        F: FnOnce(&mut Transaction<'_>) -> Result<(), StoreError>,
    {
        let mut connection = self.writer()?;
        let mut transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        mutate_domain(&mut transaction)?;
        insert_outbox(&transaction, operation)?;
        transaction.commit().map_err(sqlite_error)
    }

    /// Upserts bounded incremental-blob metadata without placing large bytes in `SQLite`.
    ///
    /// # Errors
    ///
    /// Returns an error when the size exceeds `SQLite`'s signed range or the write fails.
    pub fn put_blob_manifest(&self, manifest: &SQLiteBlobManifest) -> Result<(), StoreError> {
        self.writer()?
            .execute(
                "INSERT INTO blob_manifest(blob_id,digest,size_bytes,available,pinned,last_access_unix_ms)
                 VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(blob_id) DO UPDATE SET digest=excluded.digest,
                 size_bytes=excluded.size_bytes,available=excluded.available,pinned=excluded.pinned,
                 last_access_unix_ms=excluded.last_access_unix_ms",
                params![
                    &manifest.blob_id,
                    manifest.digest.as_slice(),
                    to_i64(manifest.size_bytes, "blob size")?,
                    i64::from(manifest.available),
                    i64::from(manifest.pinned),
                    to_i64(unix_time_millis(), "blob access time")?,
                ],
            )
            .map_err(sqlite_error)?;
        Ok(())
    }

    /// Stores a bounded scope-cache entry with explicit reconstructability.
    ///
    /// # Errors
    ///
    /// Returns a storage error if `SQLite` rejects the write.
    pub fn put_scope_cache(
        &self,
        scope: SyncScopeId,
        cache_key: &str,
        value: &[u8],
        reconstructable: bool,
    ) -> Result<(), StoreError> {
        self.writer()?
            .execute(
                "INSERT INTO scope_cache(scope_id,cache_key,value,reconstructable)
                 VALUES(?1,?2,?3,?4)
                 ON CONFLICT(scope_id,cache_key) DO UPDATE SET value=excluded.value,
                 reconstructable=excluded.reconstructable",
                params![
                    scope.to_string(),
                    cache_key,
                    value,
                    i64::from(reconstructable)
                ],
            )
            .map_err(sqlite_error)?;
        Ok(())
    }

    /// Creates a consistent online backup containing domain and all synchronization tables.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the destination cannot be created or `SQLite` backup fails.
    pub fn backup_to(&self, destination: impl AsRef<Path>) -> Result<(), StoreError> {
        let source = self.writer()?;
        let mut destination = Connection::open(destination).map_err(sqlite_error)?;
        let backup =
            rusqlite::backup::Backup::new(&source, &mut destination).map_err(sqlite_error)?;
        backup
            .run_to_completion(128, Duration::from_millis(5), None)
            .map_err(sqlite_error)
    }

    /// Replaces this replica from a valid `SQLite` backup using the online backup API.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the backup is invalid or restore fails. Callers must validate
    /// device credentials separately before resuming synchronization.
    pub fn restore_from(&self, source: impl AsRef<Path>) -> Result<(), StoreError> {
        let source = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(sqlite_error)?;
        if integrity_check(&source)? != "ok" {
            return Err(StoreError::permanent(
                "SQLite backup failed integrity check",
            ));
        }
        let mut destination = self.writer()?;
        let backup =
            rusqlite::backup::Backup::new(&source, &mut destination).map_err(sqlite_error)?;
        backup
            .run_to_completion(128, Duration::from_millis(5), None)
            .map_err(sqlite_error)
    }

    /// Runs integrity detection and preserves a forensic copy on failure.
    ///
    /// # Errors
    ///
    /// Returns an error only if integrity detection or forensic preservation itself fails.
    pub fn detect_and_preserve_corruption(
        &self,
        forensic_path: impl AsRef<Path>,
    ) -> Result<RecoveryState, StoreError> {
        let connection = self.writer()?;
        if integrity_check(&connection).is_ok_and(|result| result == "ok") {
            return Ok(RecoveryState::Healthy);
        }
        Self::preserve_forensic_copy(self.path.as_ref(), forensic_path)
            .map(RecoveryState::RecoveryRequired)
    }

    /// Preserves an unopened or unopenable database and any WAL sidecars for forensic recovery.
    ///
    /// # Errors
    ///
    /// Returns an error when the main database copy cannot be created. Missing sidecars are normal;
    /// an existing sidecar that cannot be copied is reported rather than silently omitted.
    pub fn preserve_forensic_copy(
        database_path: impl AsRef<Path>,
        forensic_path: impl AsRef<Path>,
    ) -> Result<ForensicCopy, StoreError> {
        let database_path = database_path.as_ref();
        let forensic_path = forensic_path.as_ref();
        let mut bytes = fs::copy(database_path, forensic_path)
            .map_err(|error| StoreError::permanent(error.to_string()))?;
        for suffix in ["-wal", "-shm"] {
            let source = sidecar_path(database_path, suffix);
            if source.exists() {
                bytes = bytes.saturating_add(
                    fs::copy(source, sidecar_path(forensic_path, suffix))
                        .map_err(|error| StoreError::permanent(error.to_string()))?,
                );
            }
        }
        Ok(ForensicCopy {
            path: forensic_path.to_path_buf(),
            bytes,
        })
    }

    /// Reports WAL, integrity, and schema readiness without deleting damaged state.
    ///
    /// # Errors
    ///
    /// Returns a storage error when `SQLite` cannot answer a health query.
    pub fn health(&self) -> Result<SQLiteHealth, StoreError> {
        let connection = self.writer()?;
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(sqlite_error)?;
        let integrity = integrity_check(&connection)?;
        let schema_version = schema_version(&connection)?;
        let ready = integrity == "ok"
            && schema_version == SQLITE_SCHEMA_VERSION
            && (!self.production || journal_mode.eq_ignore_ascii_case("wal"));
        Ok(SQLiteHealth {
            journal_mode,
            integrity,
            schema_version,
            ready,
        })
    }

    /// Performs a passive WAL checkpoint without blocking active readers.
    ///
    /// # Errors
    ///
    /// Returns a storage error if `SQLite` cannot run the checkpoint.
    pub fn checkpoint(&self) -> Result<(), StoreError> {
        self.writer()?
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE);")
            .map_err(sqlite_error)
    }

    fn writer(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.writer
            .lock()
            .map_err(|_| StoreError::permanent("SQLite writer lock was poisoned"))
    }

    fn reader(&self) -> Result<Connection, StoreError> {
        open_connection(self.path.as_ref(), self.busy_timeout, true)
    }
}

impl TransactionCapabilityProvider for SQLiteDatabase {
    fn transaction_capabilities(&self) -> TransactionCapabilities {
        TransactionCapabilities::FULL_LOCAL
    }
}

impl AdapterManifestProvider for SQLiteDatabase {
    fn adapter_manifest(&self) -> AdapterManifest {
        SQLITE_ADAPTER_MANIFEST
    }
}

#[async_trait]
impl OutboxStore for SQLiteDatabase {
    async fn pending_operations(&self, limit: usize) -> Result<Vec<OperationEnvelope>, StoreError> {
        let connection = self.reader()?;
        let mut statement = connection
            .prepare(
                "SELECT envelope FROM operation_outbox
                 WHERE state IN ('pending','sending','retry') AND next_attempt_unix_ms <= ?1
                 ORDER BY local_sequence LIMIT ?2",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                params![
                    to_i64(unix_time_millis(), "current time")?,
                    usize_to_i64(limit)
                ],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .map_err(sqlite_error)?;
        let mut operations = Vec::new();
        for encoded in rows {
            operations.push(decode(&encoded.map_err(sqlite_error)?)?);
        }
        Ok(operations)
    }

    async fn append_operation(&self, operation: OperationEnvelope) -> Result<(), StoreError> {
        self.transact_local_mutation(&operation, |_| Ok(()))
    }

    async fn compact_outbox(
        &self,
        _registry: &OptimizationRegistry,
        _max_operations: usize,
    ) -> Result<aequora_queue::CompactionPlan, StoreError> {
        Ok(aequora_queue::CompactionPlan::default())
    }
}

#[async_trait]
impl OutboxStateStore for SQLiteDatabase {
    async fn mark_sending(&self, operations: &[OperationId]) -> Result<(), StoreError> {
        transition_operations(self, operations, "sending", None)
    }

    async fn mark_retry(
        &self,
        operations: &[OperationId],
        next_attempt_unix_ms: u64,
    ) -> Result<(), StoreError> {
        transition_operations(self, operations, "retry", Some(next_attempt_unix_ms))
    }

    async fn retry_metadata(
        &self,
        operation: OperationId,
    ) -> Result<Option<RetryMetadata>, StoreError> {
        self.reader()?
            .query_row(
                "SELECT attempt_count, next_attempt_unix_ms FROM operation_outbox
                 WHERE operation_id=?1 AND attempt_count > 0",
                [operation.to_string()],
                |row| {
                    let attempt: i64 = row.get(0)?;
                    let next: i64 = row.get(1)?;
                    Ok((attempt, next))
                },
            )
            .optional()
            .map_err(sqlite_error)?
            .map(|(attempt, next)| {
                Ok(RetryMetadata {
                    attempt_count: u32::try_from(attempt)
                        .map_err(|_| StoreError::permanent("invalid SQLite retry count"))?,
                    next_attempt_unix_ms: u64::try_from(next)
                        .map_err(|_| StoreError::permanent("invalid SQLite retry timestamp"))?,
                })
            })
            .transpose()
    }

    async fn operation_state(
        &self,
        operation: OperationId,
    ) -> Result<Option<OutboxState>, StoreError> {
        self.reader()?
            .query_row(
                "SELECT state FROM operation_outbox WHERE operation_id=?1",
                [operation.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_error)?
            .map(|state| parse_state(&state))
            .transpose()
    }

    async fn outbox_stats(&self) -> Result<OutboxStats, StoreError> {
        let connection = self.reader()?;
        let mut statement = connection
            .prepare(
                "SELECT state, envelope FROM operation_outbox
                 WHERE state IN ('pending','sending','retry','rejected')",
            )
            .map_err(sqlite_error)?;
        let mut rows = statement.query([]).map_err(sqlite_error)?;
        let mut stats = OutboxStats::default();
        while let Some(row) = rows.next().map_err(sqlite_error)? {
            let lifecycle = parse_state(&row.get::<_, String>(0).map_err(sqlite_error)?)?;
            match lifecycle {
                OutboxState::Pending => stats.pending = stats.pending.saturating_add(1),
                OutboxState::Sending => stats.sending = stats.sending.saturating_add(1),
                OutboxState::Retry => stats.retry = stats.retry.saturating_add(1),
                OutboxState::Rejected => {
                    stats.rejected = stats.rejected.saturating_add(1);
                    continue;
                }
                OutboxState::Acknowledged | OutboxState::Conflict => continue,
            }
            let envelope: OperationEnvelope =
                decode(&row.get::<_, Vec<u8>>(1).map_err(sqlite_error)?)?;
            stats.oldest_pending_at = Some(
                stats
                    .oldest_pending_at
                    .map_or(envelope.created_at, |current| {
                        current.min(envelope.created_at)
                    }),
            );
        }
        Ok(stats)
    }
}

#[async_trait]
impl ConflictInbox for SQLiteDatabase {
    async fn unresolved_conflicts(&self, limit: usize) -> Result<Vec<ConflictRecord>, StoreError> {
        let connection = self.reader()?;
        let mut statement = connection
            .prepare(
                "SELECT detail FROM conflict_ledger WHERE resolution IS NULL
                 ORDER BY created_at_unix_ms, operation_id LIMIT ?1",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([usize_to_i64(limit)], |row| row.get::<_, Vec<u8>>(0))
            .map_err(sqlite_error)?;
        let mut conflicts = Vec::new();
        for detail in rows {
            conflicts.push(ConflictRecord {
                conflict: decode(&detail.map_err(sqlite_error)?)?,
                resolution: None,
            });
        }
        Ok(conflicts)
    }

    async fn unresolved_conflict_count(&self) -> Result<usize, StoreError> {
        let count: i64 = self
            .reader()?
            .query_row(
                "SELECT COUNT(*) FROM conflict_ledger WHERE resolution IS NULL",
                [],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        usize::try_from(count).map_err(|_| StoreError::permanent("negative SQLite conflict count"))
    }

    async fn resolve_conflict(
        &self,
        operation: OperationId,
        resolution: ConflictResolution,
    ) -> Result<(), StoreError> {
        let detail = match resolution {
            ConflictResolution::AcceptServer => "accept_server".to_owned(),
            ConflictResolution::SupersededBy(replacement) => format!("superseded_by:{replacement}"),
        };
        let changed = self
            .writer()?
            .execute(
                "UPDATE conflict_ledger SET resolution=?1 WHERE operation_id=?2 AND resolution IS NULL",
                params![detail, operation.to_string()],
            )
            .map_err(sqlite_error)?;
        if changed != 1 {
            return Err(StoreError::permanent(
                "SQLite conflict does not exist or was already resolved",
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl CursorStore for SQLiteDatabase {
    async fn load_cursor(&self, scope: SyncScopeId) -> Result<Option<Cursor>, StoreError> {
        load_cursor(&self.reader()?, scope)
    }
}

#[async_trait]
impl ReconciliationStore for SQLiteDatabase {
    async fn reconcile(&self, response: &SyncResponse) -> Result<(), StoreError> {
        let mut connection = self.writer()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        validate_cursor(&transaction, response.next_cursor)?;

        for change in &response.changes {
            let inserted = transaction
                .execute(
                    "INSERT OR IGNORE INTO applied_authoritative_change(scope_id,sequence) VALUES(?1,?2)",
                    params![change.scope_id.to_string(), to_i64(change.sequence.0, "change sequence")?],
                )
                .map_err(sqlite_error)?;
            if inserted == 1 {
                transaction
                    .execute(
                        "INSERT INTO local_entity(scope_id,entity_type,entity_id,version,payload,tombstone)
                         VALUES(?1,?2,?3,?4,?5,?6)
                         ON CONFLICT(scope_id,entity_type,entity_id) DO UPDATE SET
                         version=excluded.version,payload=excluded.payload,tombstone=excluded.tombstone",
                        params![
                            change.scope_id.to_string(),
                            i64::from(change.entity.entity_type.get()),
                            change.entity.entity_id.to_string(),
                            to_i64(change.version.get(), "entity version")?,
                            &change.payload,
                            i64::from(matches!(change.change_kind, ChangeKind::Tombstone)),
                        ],
                    )
                    .map_err(sqlite_error)?;
                self.projection_hook
                    .apply_change(&transaction, change.scope_id, change)?;
            }
        }
        for acknowledgement in &response.acknowledged {
            terminal_outbox(
                &transaction,
                acknowledgement.operation_id,
                "acknowledged",
                &encode(acknowledgement)?,
            )?;
        }
        for rejection in &response.rejected {
            terminal_outbox(
                &transaction,
                rejection.operation_id,
                "rejected",
                &encode(rejection)?,
            )?;
        }
        for conflict in &response.conflicts {
            let detail = encode(conflict)?;
            transaction
                .execute(
                    "INSERT INTO conflict_ledger(operation_id,detail,resolution,created_at_unix_ms)
                     VALUES(?1,?2,NULL,?3)
                     ON CONFLICT(operation_id) DO UPDATE SET detail=excluded.detail",
                    params![
                        conflict.operation_id.to_string(),
                        detail,
                        to_i64(unix_time_millis(), "conflict time")?
                    ],
                )
                .map_err(sqlite_error)?;
            terminal_outbox(&transaction, conflict.operation_id, "conflict", &[])?;
        }
        write_cursor(&transaction, response.next_cursor)?;
        transaction.commit().map_err(sqlite_error)
    }

    async fn stage_snapshot(&self, response: &BootstrapResponse) -> Result<(), StoreError> {
        let mut connection = self.writer()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let scope = response.cursor.scope;
        let progress = load_snapshot_progress(&transaction, scope)?;
        if let Some(current) = progress {
            if current.snapshot_id != response.snapshot_id || current.next_offset != response.offset
            {
                return Err(StoreError::permanent(
                    "SQLite snapshot resume identity or offset mismatch",
                ));
            }
        } else if response.offset != 0 {
            return Err(StoreError::permanent(
                "SQLite snapshot must start at offset zero",
            ));
        }
        for entity in &response.entities {
            transaction
                .execute(
                    "INSERT INTO snapshot_staging(scope_id,entity_type,entity_id,version,payload,tombstone)
                     VALUES(?1,?2,?3,?4,?5,?6)
                     ON CONFLICT(scope_id,entity_type,entity_id) DO UPDATE SET
                     version=excluded.version,payload=excluded.payload,tombstone=excluded.tombstone",
                    params![
                        scope.to_string(),
                        i64::from(entity.entity.entity_type.get()),
                        entity.entity.entity_id.to_string(),
                        to_i64(entity.version.get(), "snapshot version")?,
                        &entity.payload,
                        i64::from(entity.tombstone),
                    ],
                )
                .map_err(sqlite_error)?;
        }
        if response.has_more {
            write_snapshot_progress(&transaction, response)?;
        } else {
            transaction
                .execute(
                    "DELETE FROM local_entity WHERE scope_id=?1",
                    [scope.to_string()],
                )
                .map_err(sqlite_error)?;
            transaction
                .execute(
                    "INSERT INTO local_entity(scope_id,entity_type,entity_id,version,payload,tombstone)
                     SELECT scope_id,entity_type,entity_id,version,payload,tombstone
                     FROM snapshot_staging WHERE scope_id=?1",
                    [scope.to_string()],
                )
                .map_err(sqlite_error)?;
            let mut statement = transaction
                .prepare(
                    "SELECT entity_type,entity_id,version,payload,tombstone
                     FROM snapshot_staging WHERE scope_id=?1 ORDER BY entity_type,entity_id",
                )
                .map_err(sqlite_error)?;
            let rows = statement
                .query_map([scope.to_string()], snapshot_from_row)
                .map_err(sqlite_error)?;
            for entity in rows {
                self.projection_hook.apply_snapshot_entity(
                    &transaction,
                    scope,
                    &entity.map_err(sqlite_error)?,
                )?;
            }
            drop(statement);
            transaction
                .execute(
                    "DELETE FROM snapshot_staging WHERE scope_id=?1",
                    [scope.to_string()],
                )
                .map_err(sqlite_error)?;
            transaction
                .execute(
                    "DELETE FROM snapshot_progress WHERE scope_id=?1",
                    [scope.to_string()],
                )
                .map_err(sqlite_error)?;
            write_cursor(&transaction, response.cursor)?;
        }
        transaction.commit().map_err(sqlite_error)
    }

    async fn snapshot_progress(
        &self,
        scope: SyncScopeId,
    ) -> Result<Option<SnapshotProgress>, StoreError> {
        load_snapshot_progress(&self.reader()?, scope)
    }
}

fn open_connection(
    path: &Path,
    timeout: Duration,
    read_only: bool,
) -> Result<Connection, StoreError> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    let connection = Connection::open_with_flags(path, flags).map_err(sqlite_error)?;
    connection.busy_timeout(timeout).map_err(sqlite_error)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(sqlite_error)?;
    if read_only {
        connection
            .pragma_update(None, "query_only", true)
            .map_err(sqlite_error)?;
    }
    Ok(connection)
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn configure_writer(connection: &mut Connection, production: bool) -> Result<(), StoreError> {
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(sqlite_error)?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(sqlite_error)?;
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(sqlite_error)?;
    if production && !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(StoreError::permanent(format!(
            "production SQLite requires WAL, got {journal_mode}"
        )));
    }
    Ok(())
}

fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Exclusive)
        .map_err(sqlite_error)?;
    transaction
        .execute_batch(SQLITE_SCHEMA)
        .map_err(sqlite_error)?;
    transaction
        .execute(
            "INSERT INTO metadata(key,value) VALUES('schema_version',?1)
             ON CONFLICT(key) DO NOTHING",
            [SQLITE_SCHEMA_VERSION.to_string()],
        )
        .map_err(sqlite_error)?;
    transaction
        .execute(
            "INSERT INTO metadata(key,value) VALUES('adapter','aequora-sqlite')
             ON CONFLICT(key) DO NOTHING",
            [],
        )
        .map_err(sqlite_error)?;
    let version = schema_version(&transaction)?;
    if version != SQLITE_SCHEMA_VERSION {
        return Err(StoreError::permanent(format!(
            "unsupported SQLite schema version {version}"
        )));
    }
    transaction.commit().map_err(sqlite_error)
}

fn schema_version(connection: &Connection) -> Result<u32, StoreError> {
    let value: String = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    value
        .parse()
        .map_err(|_| StoreError::permanent("invalid SQLite schema version"))
}

fn integrity_check(connection: &Connection) -> Result<String, StoreError> {
    connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(sqlite_error)
}

fn insert_outbox(
    transaction: &Transaction<'_>,
    operation: &OperationEnvelope,
) -> Result<(), StoreError> {
    let encoded = encode(operation)?;
    let digest = blake3::hash(&encoded);
    transaction
        .execute(
            "INSERT INTO operation_outbox(operation_id,envelope,payload_digest,state,created_at_physical_ms)
             VALUES(?1,?2,?3,'pending',?4)",
            params![
                operation.operation_id.to_string(),
                encoded,
                digest.as_bytes().as_slice(),
                operation.created_at.physical_ms,
            ],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn transition_operations(
    database: &SQLiteDatabase,
    operations: &[OperationId],
    state: &str,
    retry_at: Option<u64>,
) -> Result<(), StoreError> {
    let mut connection = database.writer()?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    for operation in operations {
        let changed = if let Some(retry_at) = retry_at {
            transaction.execute(
                "UPDATE operation_outbox SET state=?1,ever_sent=1,attempt_count=attempt_count+1,
                 next_attempt_unix_ms=?2 WHERE operation_id=?3 AND state IN ('pending','sending','retry')",
                params![state, to_i64(retry_at, "retry timestamp")?, operation.to_string()],
            )
        } else {
            transaction.execute(
                "UPDATE operation_outbox SET state=?1,ever_sent=1 WHERE operation_id=?2
                 AND state IN ('pending','sending','retry')",
                params![state, operation.to_string()],
            )
        }
        .map_err(sqlite_error)?;
        if changed != 1 {
            return Err(StoreError::permanent(
                "SQLite outbox transition requires one replayable operation",
            ));
        }
    }
    transaction.commit().map_err(sqlite_error)
}

fn terminal_outbox(
    transaction: &Transaction<'_>,
    operation: OperationId,
    state: &str,
    detail: &[u8],
) -> Result<(), StoreError> {
    transaction
        .execute(
            "UPDATE operation_outbox SET state=?1,terminal_detail=?2 WHERE operation_id=?3",
            params![state, detail, operation.to_string()],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn validate_cursor(connection: &Connection, next: Cursor) -> Result<(), StoreError> {
    if let Some(current) = load_cursor(connection, next.scope)? {
        if current.sequence > next.sequence {
            return Err(StoreError::permanent(
                "SQLite reconciliation cursor would regress",
            ));
        }
        if current.authority_id != AuthorityId::LEGACY_UNBOUND
            && (current.authority_id != next.authority_id
                || current.authority_epoch != next.authority_epoch)
        {
            return Err(StoreError::permanent(
                "SQLite incremental reconciliation cannot cross authority timelines",
            ));
        }
    }
    Ok(())
}

fn load_cursor(connection: &Connection, scope: SyncScopeId) -> Result<Option<Cursor>, StoreError> {
    connection
        .query_row(
            "SELECT authority_id,authority_epoch,sequence FROM sync_cursor WHERE scope_id=?1",
            [scope.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite_error)?
        .map(|(authority, epoch, sequence)| {
            Ok(Cursor::new(
                AuthorityId::from_str(&authority)
                    .map_err(|error| StoreError::permanent(error.to_string()))?,
                AuthorityEpoch::new(
                    u64::try_from(epoch)
                        .map_err(|_| StoreError::permanent("invalid SQLite authority epoch"))?,
                )
                .map_err(|error| StoreError::permanent(error.to_string()))?,
                scope,
                Sequence(
                    u64::try_from(sequence)
                        .map_err(|_| StoreError::permanent("invalid SQLite cursor sequence"))?,
                ),
            ))
        })
        .transpose()
}

fn write_cursor(transaction: &Transaction<'_>, cursor: Cursor) -> Result<(), StoreError> {
    transaction
        .execute(
            "INSERT INTO sync_cursor(scope_id,authority_id,authority_epoch,sequence,updated_at_unix_ms)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(scope_id) DO UPDATE SET authority_id=excluded.authority_id,
             authority_epoch=excluded.authority_epoch,sequence=excluded.sequence,
             updated_at_unix_ms=excluded.updated_at_unix_ms",
            params![
                cursor.scope.to_string(),
                cursor.authority_id.to_string(),
                to_i64(cursor.authority_epoch.get(), "authority epoch")?,
                to_i64(cursor.sequence.0, "cursor sequence")?,
                to_i64(unix_time_millis(), "cursor update time")?,
            ],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn load_snapshot_progress(
    connection: &Connection,
    scope: SyncScopeId,
) -> Result<Option<SnapshotProgress>, StoreError> {
    connection
        .query_row(
            "SELECT snapshot_id,authority_id,authority_epoch,cursor_sequence,next_offset
             FROM snapshot_progress WHERE scope_id=?1",
            [scope.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite_error)?
        .map(|(snapshot, authority, epoch, sequence, offset)| {
            Ok(SnapshotProgress {
                snapshot_id: SnapshotId::from_str(&snapshot)
                    .map_err(|error| StoreError::permanent(error.to_string()))?,
                cursor: Cursor::new(
                    AuthorityId::from_str(&authority)
                        .map_err(|error| StoreError::permanent(error.to_string()))?,
                    AuthorityEpoch::new(u64::try_from(epoch).map_err(|_| {
                        StoreError::permanent("invalid SQLite snapshot authority epoch")
                    })?)
                    .map_err(|error| StoreError::permanent(error.to_string()))?,
                    scope,
                    Sequence(
                        u64::try_from(sequence).map_err(|_| {
                            StoreError::permanent("invalid SQLite snapshot sequence")
                        })?,
                    ),
                ),
                next_offset: u64::try_from(offset)
                    .map_err(|_| StoreError::permanent("invalid SQLite snapshot offset"))?,
            })
        })
        .transpose()
}

fn write_snapshot_progress(
    transaction: &Transaction<'_>,
    response: &BootstrapResponse,
) -> Result<(), StoreError> {
    transaction
        .execute(
            "INSERT INTO snapshot_progress(scope_id,snapshot_id,authority_id,authority_epoch,cursor_sequence,next_offset)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(scope_id) DO UPDATE SET snapshot_id=excluded.snapshot_id,
             authority_id=excluded.authority_id,authority_epoch=excluded.authority_epoch,
             cursor_sequence=excluded.cursor_sequence,next_offset=excluded.next_offset",
            params![
                response.cursor.scope.to_string(),
                response.snapshot_id.to_string(),
                response.cursor.authority_id.to_string(),
                to_i64(response.cursor.authority_epoch.get(), "snapshot authority epoch")?,
                to_i64(response.cursor.sequence.0, "snapshot cursor")?,
                to_i64(response.next_offset, "snapshot offset")?,
            ],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotEntity> {
    let entity_type: i64 = row.get(0)?;
    let entity_id: String = row.get(1)?;
    let version: i64 = row.get(2)?;
    let payload: Vec<u8> = row.get(3)?;
    let tombstone: bool = row.get(4)?;
    let entity_type = u16::try_from(entity_type)
        .ok()
        .and_then(|value| EntityType::new(value).ok())
        .ok_or_else(|| rusqlite::Error::IntegralValueOutOfRange(0, entity_type))?;
    let entity_id = aequora_types::EntityId::from_str(&entity_id).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let version = aequora_types::EntityVersion::new(
        u64::try_from(version).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, version))?,
    )
    .map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    Ok(SnapshotEntity {
        entity: aequora_types::EntityRef {
            entity_type,
            entity_id,
        },
        version,
        payload,
        tombstone,
    })
}

fn parse_state(value: &str) -> Result<OutboxState, StoreError> {
    match value {
        "pending" => Ok(OutboxState::Pending),
        "sending" => Ok(OutboxState::Sending),
        "acknowledged" => Ok(OutboxState::Acknowledged),
        "rejected" => Ok(OutboxState::Rejected),
        "conflict" => Ok(OutboxState::Conflict),
        "retry" => Ok(OutboxState::Retry),
        _ => Err(StoreError::permanent("invalid SQLite outbox state")),
    }
}

fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, StoreError> {
    postcard::to_allocvec(value).map_err(|error| StoreError::permanent(error.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(value: &[u8]) -> Result<T, StoreError> {
    postcard::from_bytes(value).map_err(|error| StoreError::permanent(error.to_string()))
}

#[allow(clippy::needless_pass_by_value)]
fn sqlite_error(error: rusqlite::Error) -> StoreError {
    match &error {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            StoreError::transient(error.to_string())
        }
        _ => StoreError::permanent(error.to_string()),
    }
}

fn to_i64(value: u64, name: &str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::permanent(format!("{name} exceeds SQLite range")))
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_protocol::{
        OperationAck, OperationKind, OperationMetadata, RemoteChange, SyncDirective,
    };
    use aequora_testkit::contracts::verify_local_store;
    use aequora_types::{
        ActorId, DeviceId, EntityId, EntityRef, EntityVersion, EventId, HybridTimestamp,
        LineageRef, NodeId, ProtocolVersion, SchemaVersion, TenantId,
    };
    use tempfile::TempDir;

    fn open_store() -> (TempDir, SQLiteDatabase) {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let config = SQLiteConfig::production(
            directory.path().join("replica.sqlite3"),
            SQLitePlatform::Desktop,
        );
        let database = SQLiteDatabase::open(config).unwrap_or_else(|error| panic!("{error}"));
        (directory, database)
    }

    fn entity() -> EntityRef {
        EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
            entity_id: EntityId::new(),
        }
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
            created_at: timestamp(),
            schema_version: SchemaVersion(1),
            operation_kind: OperationKind(1),
            payload: b"portable sqlite".to_vec(),
            metadata: OperationMetadata::default(),
        }
    }

    fn timestamp() -> HybridTimestamp {
        HybridTimestamp {
            physical_ms: 1,
            logical: 0,
            node: NodeId::new(),
        }
    }

    fn response(operation: &OperationEnvelope, scope: SyncScopeId) -> SyncResponse {
        let event_id = EventId::new();
        let lineage = operation
            .metadata
            .lineage
            .derived(LineageRef::Operation(operation.operation_id));
        SyncResponse {
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
                entity: operation.entity,
                version: EntityVersion::INITIAL,
                change_kind: ChangeKind::Upsert,
                payload: b"authority".to_vec(),
                timestamp: timestamp(),
            }],
            next_cursor: Cursor::legacy(scope, Sequence(1)),
            has_more: false,
            server_time: timestamp(),
        }
    }

    #[test]
    fn production_open_enforces_wal_foreign_keys_and_required_schema() {
        let (_directory, database) = open_store();
        let health = database.health().unwrap_or_else(|error| panic!("{error}"));
        assert!(health.ready);
        assert_eq!(health.journal_mode.to_ascii_lowercase(), "wal");
        let reader = database.reader().unwrap_or_else(|error| panic!("{error}"));
        let foreign_keys: i64 = reader
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(foreign_keys, 1);
        for table in [
            "operation_outbox",
            "sync_cursor",
            "conflict_ledger",
            "scope_cache",
            "blob_manifest",
            "metadata",
        ] {
            let found: i64 = reader
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(found, 1, "missing required table {table}");
        }
    }

    #[tokio::test]
    async fn tx_a_commits_domain_and_outbox_together_and_survives_reopen() {
        let (directory, database) = open_store();
        let first = operation(entity());
        database
            .transact_local_mutation(&first, |transaction| {
                transaction
                    .execute_batch(
                        "CREATE TABLE domain_note(id INTEGER PRIMARY KEY, body TEXT NOT NULL);",
                    )
                    .map_err(sqlite_error)?;
                transaction
                    .execute("INSERT INTO domain_note(id,body) VALUES(1,'first')", [])
                    .map_err(sqlite_error)?;
                Ok(())
            })
            .unwrap_or_else(|error| panic!("{error}"));

        let failed = operation(entity());
        let result = database.transact_local_mutation(&failed, |transaction| {
            transaction
                .execute("INSERT INTO domain_note(id,body) VALUES(1,'duplicate')", [])
                .map_err(sqlite_error)?;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(
            database.operation_state(failed.operation_id).await,
            Ok(None)
        );

        drop(database);
        let reopened = SQLiteDatabase::open(SQLiteConfig::production(
            directory.path().join("replica.sqlite3"),
            SQLitePlatform::Desktop,
        ))
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            reopened.operation_state(first.operation_id).await,
            Ok(Some(OutboxState::Pending))
        );
        let count: i64 = reopened
            .reader()
            .unwrap_or_else(|error| panic!("{error}"))
            .query_row("SELECT COUNT(*) FROM domain_note", [], |row| row.get(0))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn savepoints_and_wal_readers_preserve_single_writer_semantics() {
        let (_directory, database) = open_store();
        let first = operation(entity());
        database
            .transact_local_mutation(&first, |transaction| {
                transaction
                    .execute_batch("CREATE TABLE domain_savepoint(id INTEGER PRIMARY KEY);")
                    .map_err(sqlite_error)?;
                {
                    let mut savepoint = transaction.savepoint().map_err(sqlite_error)?;
                    savepoint
                        .execute("INSERT INTO domain_savepoint VALUES(1)", [])
                        .map_err(sqlite_error)?;
                    savepoint.rollback().map_err(sqlite_error)?;
                }
                transaction
                    .execute("INSERT INTO domain_savepoint VALUES(2)", [])
                    .map_err(sqlite_error)?;
                Ok(())
            })
            .unwrap_or_else(|error| panic!("{error}"));

        let entered = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let writer = database.clone();
        let second = operation(entity());
        let second_id = second.operation_id;
        let entered_writer = Arc::clone(&entered);
        let release_writer = Arc::clone(&release);
        let handle = std::thread::spawn(move || {
            writer.transact_local_mutation(&second, |_| {
                entered_writer.wait();
                release_writer.wait();
                Ok(())
            })
        });
        entered.wait();
        let visible = database
            .pending_operations(16)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            visible
                .iter()
                .any(|item| item.operation_id == first.operation_id)
        );
        assert!(!visible.iter().any(|item| item.operation_id == second_id));
        release.wait();
        handle
            .join()
            .unwrap_or_else(|_| panic!("writer thread panicked"))
            .unwrap_or_else(|error| panic!("{error}"));

        let ids: Vec<i64> = database
            .reader()
            .unwrap_or_else(|error| panic!("{error}"))
            .prepare("SELECT id FROM domain_savepoint ORDER BY id")
            .unwrap_or_else(|error| panic!("{error}"))
            .query_map([], |row| row.get(0))
            .unwrap_or_else(|error| panic!("{error}"))
            .collect::<Result<_, _>>()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(ids, vec![2]);
    }

    #[derive(Clone, Copy)]
    struct FailingProjection;

    impl SQLiteProjectionHook for FailingProjection {
        fn apply_change(
            &self,
            _transaction: &Transaction<'_>,
            _scope: SyncScopeId,
            _change: &RemoteChange,
        ) -> Result<(), StoreError> {
            Err(StoreError::permanent("injected projection failure"))
        }
    }

    #[tokio::test]
    async fn tx_c_rolls_back_authoritative_state_outcome_and_cursor_together() {
        let (_directory, database) = open_store();
        let pending = operation(entity());
        database
            .append_operation(pending.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let scope = SyncScopeId::new();
        let failing = database
            .clone()
            .with_projection_hook(Arc::new(FailingProjection));
        assert!(failing.reconcile(&response(&pending, scope)).await.is_err());
        assert_eq!(database.load_cursor(scope).await, Ok(None));
        assert_eq!(
            database.operation_state(pending.operation_id).await,
            Ok(Some(OutboxState::Pending))
        );

        database
            .reconcile(&response(&pending, scope))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            database.load_cursor(scope).await,
            Ok(Some(Cursor::legacy(scope, Sequence(1))))
        );
    }

    #[tokio::test]
    async fn sqlite_passes_the_public_local_adapter_contract() {
        let (_directory, database) = open_store();
        let pending = operation(entity());
        verify_local_store(&database, pending, SyncScopeId::new(), timestamp())
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    }

    #[tokio::test]
    async fn online_backup_preserves_domain_outbox_cursor_and_metadata() {
        let (directory, database) = open_store();
        let pending = operation(entity());
        database
            .transact_local_mutation(&pending, |transaction| {
                transaction
                    .execute_batch("CREATE TABLE domain_value(id INTEGER PRIMARY KEY, value TEXT);")
                    .map_err(sqlite_error)?;
                transaction
                    .execute("INSERT INTO domain_value VALUES(1,'kept')", [])
                    .map_err(sqlite_error)?;
                Ok(())
            })
            .unwrap_or_else(|error| panic!("{error}"));
        let scope = SyncScopeId::new();
        database
            .reconcile(&response(&pending, scope))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let backup_path = directory.path().join("backup.sqlite3");
        database
            .backup_to(&backup_path)
            .unwrap_or_else(|error| panic!("{error}"));

        let restored_path = directory.path().join("restored.sqlite3");
        fs::copy(&backup_path, &restored_path).unwrap_or_else(|error| panic!("{error}"));
        let restored = SQLiteDatabase::open(SQLiteConfig::production(
            restored_path,
            SQLitePlatform::Desktop,
        ))
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            restored.load_cursor(scope).await,
            database.load_cursor(scope).await
        );
        assert_eq!(
            restored.operation_state(pending.operation_id).await,
            Ok(Some(OutboxState::Acknowledged))
        );
        let value: String = restored
            .reader()
            .unwrap_or_else(|error| panic!("{error}"))
            .query_row("SELECT value FROM domain_value WHERE id=1", [], |row| {
                row.get(0)
            })
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(value, "kept");
    }

    #[test]
    fn forensic_copy_preserves_main_database_and_existing_wal_sidecars() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let source = directory.path().join("damaged.sqlite3");
        let destination = directory.path().join("forensic.sqlite3");
        fs::write(&source, b"damaged-main").unwrap_or_else(|error| panic!("{error}"));
        fs::write(sidecar_path(&source, "-wal"), b"pending-wal")
            .unwrap_or_else(|error| panic!("{error}"));

        let copy = SQLiteDatabase::preserve_forensic_copy(&source, &destination)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(copy.path, destination);
        assert_eq!(copy.bytes, 23);
        assert_eq!(
            fs::read(&copy.path).unwrap_or_else(|error| panic!("{error}")),
            b"damaged-main"
        );
        assert_eq!(
            fs::read(sidecar_path(&copy.path, "-wal")).unwrap_or_else(|error| panic!("{error}")),
            b"pending-wal"
        );
    }
}
