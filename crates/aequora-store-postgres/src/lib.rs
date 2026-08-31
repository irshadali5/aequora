//! `PostgreSQL` authoritative-store boundary and schema.
//!
//! The backend contract is deliberately transaction-oriented. A `SQLx` implementation can
//! satisfy it without exposing `sqlx::Transaction` to the rest of Aequora.

use aequora_adapter_sdk as adapter_sdk;
use aequora_authority::{
    AuthorityRole, AuthorityRuntimeMode, AuthorityState, AuthorityTransitionManifest,
    PromotionClass, TransitionReason,
};
use aequora_executor::CurrentEntity;
use aequora_integrity::{
    CURRENT_HASH_SCHEMA, CanonicalEntity, IntegrityGeneration, IntegritySnapshot, IntegritySupport,
    PartitionScheme,
};
use aequora_live::{HintBroker, HintSubscription, LiveError, LiveFailureKind, SyncHint};
use aequora_protocol::{
    ChangeKind, OperationAck, OperationRejection, Partition, RemoteChange, SnapshotEntity,
};
use aequora_store::{
    AdapterCapabilities, AdapterManifest, AdapterManifestProvider, AdapterRole, AdapterTier,
    AuditLog, AuditOffset, AuditPage, AuthoritativeIntegritySource, ChangeJournal, ChangePage,
    CommitOperation, CommitOutcome, CorrelationLog, EntityReader, EntitySnapshot,
    IntegrityCapabilityProvider, JournalCompactor, OperationLedger, SnapshotDescriptor,
    SnapshotPage, SnapshotStore, StoreError, StoreErrorReason, TransactionCapabilities,
    TransactionCapabilityProvider,
};
use aequora_types::{
    ActorId, AuthorityEpoch, AuthorityId, AuthorityInstanceId, AuthorityTransitionId,
    CorrelationId, Cursor, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, EventId,
    HybridTimestamp, LineageContext, LineageRef, NodeId, OperationId, Sequence, SnapshotId,
    SyncScopeId, TenantId,
};
use async_trait::async_trait;
use sqlx::{
    PgPool, Postgres, Row, Transaction,
    postgres::{PgConnectOptions, PgListener, PgPoolOptions, PgSslMode},
};
use std::{fmt, str::FromStr, sync::Arc, time::Duration};

/// `PostgreSQL` `LISTEN/NOTIFY` adapter for ephemeral, post-commit live hints.
///
/// The durable transaction must commit before [`HintBroker::publish`] is called. A notification
/// failure is deliberately independent from authoritative commit success.
#[derive(Clone, Debug)]
pub struct PostgresNotifyHintBroker {
    pool: PgPool,
    channel: Arc<str>,
}

impl PostgresNotifyHintBroker {
    /// Creates a broker on one fixed deployment channel.
    ///
    /// # Errors
    ///
    /// Rejects unsafe or empty channel identifiers.
    pub fn new(pool: PgPool, channel: impl Into<Arc<str>>) -> Result<Self, LiveError> {
        let channel = channel.into();
        if channel.is_empty()
            || channel.len() > 63
            || !channel
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(LiveError::new(
                LiveFailureKind::PermanentConfig,
                "invalid PostgreSQL notification channel",
            ));
        }
        Ok(Self { pool, channel })
    }

    fn encode(hint: SyncHint) -> Result<String, LiveError> {
        let bytes = postcard::to_stdvec(&hint)
            .map_err(|error| LiveError::new(LiveFailureKind::PermanentConfig, error.to_string()))?;
        let payload = hex::encode(bytes);
        if payload.len() >= 8_000 {
            return Err(LiveError::new(
                LiveFailureKind::PermanentConfig,
                "PostgreSQL notification payload exceeds limit",
            ));
        }
        Ok(payload)
    }

    fn decode(payload: &str) -> Result<SyncHint, LiveError> {
        let bytes = hex::decode(payload).map_err(|error| {
            LiveError::new(LiveFailureKind::ProtocolMismatch, error.to_string())
        })?;
        postcard::from_bytes(&bytes)
            .map_err(|error| LiveError::new(LiveFailureKind::ProtocolMismatch, error.to_string()))
    }
}

struct PostgresHintSubscription {
    listener: PgListener,
}

#[async_trait]
impl HintSubscription for PostgresHintSubscription {
    async fn next_hint(&mut self) -> Result<Option<SyncHint>, LiveError> {
        let notification = self.listener.recv().await.map_err(|error| {
            LiveError::new(LiveFailureKind::OptionalUnavailable, error.to_string())
        })?;
        PostgresNotifyHintBroker::decode(notification.payload()).map(Some)
    }
}

#[async_trait]
impl HintBroker for PostgresNotifyHintBroker {
    async fn publish(&self, hint: SyncHint) -> Result<(), LiveError> {
        let payload = Self::encode(hint)?;
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(self.channel.as_ref())
            .bind(payload)
            .execute(&self.pool)
            .await
            .map_err(|error| {
                LiveError::new(LiveFailureKind::OptionalUnavailable, error.to_string())
            })?;
        Ok(())
    }

    async fn subscribe(&self) -> Result<Box<dyn HintSubscription>, LiveError> {
        let mut listener = PgListener::connect_with(&self.pool)
            .await
            .map_err(|error| {
                LiveError::new(LiveFailureKind::OptionalUnavailable, error.to_string())
            })?;
        listener.listen(&self.channel).await.map_err(|error| {
            LiveError::new(LiveFailureKind::OptionalUnavailable, error.to_string())
        })?;
        Ok(Box::new(PostgresHintSubscription { listener }))
    }
}

/// `PostgreSQL` schema for opaque authoritative snapshots, scoped journals, and operation ledger.
/// Applications may include this migration in their own migration runner.
pub const MIGRATION_0001: &str = r"
CREATE TABLE IF NOT EXISTS aequora_entities (
    tenant_id UUID NOT NULL,
    entity_type INTEGER NOT NULL CHECK (entity_type BETWEEN 1 AND 65535),
    entity_id UUID NOT NULL,
    version BIGINT NOT NULL CHECK (version > 0),
    payload BYTEA NOT NULL,
    tombstone BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (tenant_id, entity_type, entity_id)
);

CREATE TABLE IF NOT EXISTS aequora_scope_sequences (
    tenant_id UUID NOT NULL,
    scope_id UUID NOT NULL,
    sequence BIGINT NOT NULL CHECK (sequence >= 0),
    PRIMARY KEY (tenant_id, scope_id)
);

CREATE TABLE IF NOT EXISTS aequora_journal_floors (
    tenant_id UUID NOT NULL,
    scope_id UUID NOT NULL,
    minimum_cursor BIGINT NOT NULL CHECK (minimum_cursor >= 0),
    PRIMARY KEY (tenant_id, scope_id)
);

CREATE TABLE IF NOT EXISTS aequora_sync_events (
    tenant_id UUID NOT NULL,
    scope_id UUID NOT NULL,
    sequence BIGINT NOT NULL CHECK (sequence > 0),
    operation_id UUID NOT NULL,
    entity_type INTEGER NOT NULL CHECK (entity_type BETWEEN 1 AND 65535),
    entity_id UUID NOT NULL,
    entity_version BIGINT NOT NULL CHECK (entity_version > 0),
    change_kind SMALLINT NOT NULL,
    payload BYTEA NOT NULL,
    physical_ms BIGINT NOT NULL,
    logical_clock INTEGER NOT NULL CHECK (logical_clock >= 0),
    clock_node UUID NOT NULL,
    PRIMARY KEY (tenant_id, scope_id, sequence),
    UNIQUE (tenant_id, operation_id)
);
CREATE INDEX IF NOT EXISTS aequora_sync_events_entity_idx
    ON aequora_sync_events (tenant_id, entity_type, entity_id);

CREATE TABLE IF NOT EXISTS aequora_entity_scopes (
    tenant_id UUID NOT NULL,
    scope_id UUID NOT NULL,
    entity_type INTEGER NOT NULL CHECK (entity_type BETWEEN 1 AND 65535),
    entity_id UUID NOT NULL,
    PRIMARY KEY (tenant_id, scope_id, entity_type, entity_id),
    FOREIGN KEY (tenant_id, entity_type, entity_id)
        REFERENCES aequora_entities (tenant_id, entity_type, entity_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS aequora_applied_operations (
    tenant_id UUID NOT NULL,
    operation_id UUID NOT NULL,
    entity_version BIGINT NOT NULL CHECK (entity_version > 0),
    scope_id UUID NOT NULL,
    server_sequence BIGINT NOT NULL CHECK (server_sequence > 0),
    PRIMARY KEY (tenant_id, operation_id)
);

CREATE TABLE IF NOT EXISTS aequora_audit_log (
    audit_offset BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL,
    operation_id UUID NOT NULL,
    actor_id UUID NOT NULL,
    device_id UUID NOT NULL,
    operation_kind INTEGER NOT NULL CHECK (operation_kind BETWEEN 0 AND 65535),
    entity_type INTEGER NOT NULL CHECK (entity_type BETWEEN 1 AND 65535),
    entity_id UUID NOT NULL,
    entity_version BIGINT NOT NULL CHECK (entity_version > 0),
    command_digest BYTEA NOT NULL CHECK (octet_length(command_digest) = 32),
    physical_ms BIGINT NOT NULL,
    logical_clock INTEGER NOT NULL CHECK (logical_clock >= 0),
    clock_node UUID NOT NULL,
    UNIQUE (tenant_id, operation_id)
);
CREATE INDEX IF NOT EXISTS aequora_audit_log_tenant_offset_idx
    ON aequora_audit_log (tenant_id, audit_offset);

CREATE TABLE IF NOT EXISTS aequora_snapshots (
    tenant_id UUID NOT NULL,
    snapshot_id UUID NOT NULL,
    scope_id UUID NOT NULL,
    cursor_sequence BIGINT NOT NULL CHECK (cursor_sequence >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, snapshot_id)
);

CREATE TABLE IF NOT EXISTS aequora_snapshot_entities (
    tenant_id UUID NOT NULL,
    snapshot_id UUID NOT NULL,
    entity_order BIGINT NOT NULL CHECK (entity_order >= 0),
    entity_type INTEGER NOT NULL CHECK (entity_type BETWEEN 1 AND 65535),
    entity_id UUID NOT NULL,
    entity_version BIGINT NOT NULL CHECK (entity_version > 0),
    payload BYTEA NOT NULL,
    tombstone BOOLEAN NOT NULL,
    PRIMARY KEY (tenant_id, snapshot_id, entity_order),
    FOREIGN KEY (tenant_id, snapshot_id)
        REFERENCES aequora_snapshots (tenant_id, snapshot_id) ON DELETE CASCADE
);
";

/// Adds durable operation and event lineage to the ledger, journal, and audit log.
pub const MIGRATION_0002: &str = r"
ALTER TABLE aequora_sync_events ADD COLUMN IF NOT EXISTS event_id UUID;
ALTER TABLE aequora_sync_events ADD COLUMN IF NOT EXISTS correlation_id UUID;
ALTER TABLE aequora_sync_events ADD COLUMN IF NOT EXISTS caused_by_kind SMALLINT;
ALTER TABLE aequora_sync_events ADD COLUMN IF NOT EXISTS caused_by_id UUID;
UPDATE aequora_sync_events
SET event_id = operation_id,
    correlation_id = operation_id,
    caused_by_kind = 1,
    caused_by_id = operation_id
WHERE event_id IS NULL OR correlation_id IS NULL;
ALTER TABLE aequora_sync_events ALTER COLUMN event_id SET NOT NULL;
ALTER TABLE aequora_sync_events ALTER COLUMN correlation_id SET NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS aequora_sync_events_event_idx
    ON aequora_sync_events (tenant_id, event_id);

ALTER TABLE aequora_applied_operations ADD COLUMN IF NOT EXISTS event_id UUID;
ALTER TABLE aequora_applied_operations ADD COLUMN IF NOT EXISTS correlation_id UUID;
ALTER TABLE aequora_applied_operations ADD COLUMN IF NOT EXISTS caused_by_kind SMALLINT;
ALTER TABLE aequora_applied_operations ADD COLUMN IF NOT EXISTS caused_by_id UUID;
UPDATE aequora_applied_operations
SET event_id = operation_id,
    correlation_id = operation_id
WHERE event_id IS NULL OR correlation_id IS NULL;
ALTER TABLE aequora_applied_operations ALTER COLUMN event_id SET NOT NULL;
ALTER TABLE aequora_applied_operations ALTER COLUMN correlation_id SET NOT NULL;

ALTER TABLE aequora_audit_log ADD COLUMN IF NOT EXISTS event_id UUID;
ALTER TABLE aequora_audit_log ADD COLUMN IF NOT EXISTS correlation_id UUID;
ALTER TABLE aequora_audit_log ADD COLUMN IF NOT EXISTS caused_by_kind SMALLINT;
ALTER TABLE aequora_audit_log ADD COLUMN IF NOT EXISTS caused_by_id UUID;
UPDATE aequora_audit_log
SET event_id = operation_id,
    correlation_id = operation_id
WHERE event_id IS NULL OR correlation_id IS NULL;
ALTER TABLE aequora_audit_log ALTER COLUMN event_id SET NOT NULL;
ALTER TABLE aequora_audit_log ALTER COLUMN correlation_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS aequora_audit_log_correlation_idx
    ON aequora_audit_log (tenant_id, correlation_id, audit_offset);
";

/// Adds durable authority metadata, commit fencing, and epoch-bound artifacts.
pub const MIGRATION_0003: &str = r"
CREATE TABLE IF NOT EXISTS aequora_authority_state (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    authority_id UUID NOT NULL,
    authority_epoch BIGINT NOT NULL CHECK (authority_epoch > 0),
    authority_instance_id UUID NOT NULL,
    role TEXT NOT NULL,
    fence_token BIGINT NOT NULL CHECK (fence_token > 0),
    runtime_mode TEXT NOT NULL,
    promotion_class TEXT NOT NULL,
    transition_id UUID NOT NULL,
    updated_at_unix_ms BIGINT NOT NULL CHECK (updated_at_unix_ms >= 0)
);

CREATE TABLE IF NOT EXISTS aequora_authority_transitions (
    transition_id UUID PRIMARY KEY,
    authority_id UUID NOT NULL,
    old_epoch BIGINT NOT NULL CHECK (old_epoch > 0),
    new_epoch BIGINT NOT NULL CHECK (new_epoch > 0),
    promotion_class TEXT NOT NULL,
    old_final_sequence BIGINT,
    new_base_sequence BIGINT NOT NULL CHECK (new_base_sequence >= 0),
    reason TEXT NOT NULL,
    created_at_unix_ms BIGINT NOT NULL CHECK (created_at_unix_ms >= 0),
    fence_token BIGINT NOT NULL CHECK (fence_token > 0),
    manifest_digest BYTEA NOT NULL CHECK (octet_length(manifest_digest) = 32)
);

ALTER TABLE aequora_sync_events ADD COLUMN IF NOT EXISTS authority_id UUID;
ALTER TABLE aequora_sync_events ADD COLUMN IF NOT EXISTS authority_epoch BIGINT CHECK (authority_epoch > 0);
ALTER TABLE aequora_applied_operations ADD COLUMN IF NOT EXISTS authority_id UUID;
ALTER TABLE aequora_applied_operations ADD COLUMN IF NOT EXISTS authority_epoch BIGINT CHECK (authority_epoch > 0);
ALTER TABLE aequora_audit_log ADD COLUMN IF NOT EXISTS authority_id UUID;
ALTER TABLE aequora_audit_log ADD COLUMN IF NOT EXISTS authority_epoch BIGINT CHECK (authority_epoch > 0);
ALTER TABLE aequora_snapshots ADD COLUMN IF NOT EXISTS authority_id UUID;
ALTER TABLE aequora_snapshots ADD COLUMN IF NOT EXISTS authority_epoch BIGINT CHECK (authority_epoch > 0);
";

const MIGRATION_LEDGER_SQL: &str = r"
CREATE TABLE IF NOT EXISTS aequora_schema_migrations (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    name TEXT NOT NULL,
    checksum BYTEA NOT NULL CHECK (octet_length(checksum) = 32),
    applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
";

/// Latest `PostgreSQL` schema revision understood by this Aequora release.
pub const POSTGRES_SCHEMA_VERSION: u32 = 3;

/// Versioned role and capability declaration for the built-in `PostgreSQL` authority adapter.
pub const POSTGRES_ADAPTER_MANIFEST: AdapterManifest = AdapterManifest {
    name: "postgresql",
    adapter_version: env!("CARGO_PKG_VERSION"),
    tested_aequora_version: env!("CARGO_PKG_VERSION"),
    tested_database_versions: &["18"],
    roles: &[
        AdapterRole::AuthoritativeWritable,
        AdapterRole::ReadOnlySource,
        AdapterRole::SnapshotSource,
    ],
    tier: AdapterTier::FullProduction,
    capabilities: AdapterCapabilities::FULL_AUTHORITATIVE,
    limitations: &[],
};

const POSTGRES_SDK_ROLES: &[adapter_sdk::AdapterRole] = &[
    adapter_sdk::AdapterRole::AuthoritativeStore,
    adapter_sdk::AdapterRole::JournalStore,
    adapter_sdk::AdapterRole::OperationLedgerStore,
    adapter_sdk::AdapterRole::SnapshotStore,
    adapter_sdk::AdapterRole::IntegrityStore,
];

const POSTGRES_SDK_CAPABILITIES: &[adapter_sdk::AdapterCapability] = &[
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::ATOMIC_AUTHORITATIVE_COMMIT),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::COMPARE_AND_SWAP),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::JOURNAL),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::OPERATION_LEDGER),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::MIGRATIONS),
    adapter_sdk::AdapterCapability {
        id: adapter_sdk::CapabilityId::SNAPSHOT,
        version: adapter_sdk::CapabilityVersion::V1,
        level: adapter_sdk::CapabilityLevel::Snapshot(adapter_sdk::SnapshotLevel::Streaming),
    },
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::INTEGRITY),
    adapter_sdk::AdapterCapability::v1(adapter_sdk::CapabilityId::AUDIT),
];

/// Part 36 machine-readable manifest for the official `PostgreSQL` authority adapter.
///
/// A deployment must still supply a matching [`adapter_sdk::CertifiedEnvironment`] before these
/// claims satisfy production startup requirements. Neon is certified as a separate deployment
/// environment even though it uses this adapter implementation.
pub const POSTGRES_STORAGE_ADAPTER_MANIFEST: adapter_sdk::AdapterManifest =
    adapter_sdk::AdapterManifest {
        descriptor: adapter_sdk::AdapterDescriptor {
            adapter_id: adapter_sdk::AdapterId(0xae01),
            name: "aequora-postgresql-authority",
            version: adapter_sdk::AdapterVersion::new(0, 1, 0),
            store_kind: adapter_sdk::StoreKind::NetworkDatabase,
        },
        roles: POSTGRES_SDK_ROLES,
        capabilities: POSTGRES_SDK_CAPABILITIES,
        support: adapter_sdk::AdapterSupport::Official,
        supported_engine_versions: &["PostgreSQL 18"],
        supported_targets: &["x86_64-unknown-linux-gnu"],
        known_limitations: &[
            "Neon and other managed PostgreSQL services require deployment-specific certification",
            "fencing is composed through the authority epoch store rather than this manifest",
        ],
        concurrency: adapter_sdk::ConcurrencyModel::MultiWriter,
        maintainer_owned: true,
        release_evidence_complete: true,
    };

#[derive(Clone, Copy)]
struct PostgresMigration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

struct AppliedMigration {
    version: i32,
    name: String,
    checksum: Vec<u8>,
}

const POSTGRES_MIGRATIONS: &[PostgresMigration] = &[
    PostgresMigration {
        version: 1,
        name: "initial_authoritative_schema",
        sql: MIGRATION_0001,
    },
    PostgresMigration {
        version: 2,
        name: "durable_causality_and_lineage",
        sql: MIGRATION_0002,
    },
    PostgresMigration {
        version: 3,
        name: "authority_failover_and_epoch_binding",
        sql: MIGRATION_0003,
    },
];

/// Applied and expected `PostgreSQL` schema revisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresSchemaStatus {
    /// Highest migration recorded by the connected database.
    pub applied_version: u32,
    /// Highest migration supported by this Aequora build.
    pub expected_version: u32,
}

impl PostgresSchemaStatus {
    /// Whether the database is ready for this Aequora build.
    #[must_use]
    pub const fn is_current(self) -> bool {
        self.applied_version == self.expected_version
    }
}

/// Authoritative projection produced by an application-owned `PostgreSQL` commit hook.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PostgresCommitHookOutcome {
    authoritative_payload: Vec<u8>,
}

/// Failure returned by an application-owned commit hook.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PostgresCommitHookError {
    /// Expected authorization/business validation failure that must be returned to the client.
    Rejected(OperationRejection),
    /// Storage failure that follows the ordinary adapter retry/permanence rules.
    Store(StoreError),
}

impl From<StoreError> for PostgresCommitHookError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl PostgresCommitHookOutcome {
    /// Wraps the application projection that Aequora must persist and journal after the hook's
    /// domain mutation succeeds.
    #[must_use]
    pub fn new(authoritative_payload: Vec<u8>) -> Self {
        Self {
            authoritative_payload,
        }
    }

    fn into_authoritative_payload(self) -> Vec<u8> {
        self.authoritative_payload
    }
}

/// Application-owned domain mutation joined to Aequora's authoritative transaction.
///
/// The hook runs only for a new operation, after operation/entity locks and version validation,
/// and before any Aequora entity, journal, ledger, or audit write. Returning an error rolls back
/// the complete transaction. Implementations must therefore be deterministic and safe to invoke
/// again when `PostgreSQL` requests a whole-transaction retry.
///
/// The hook deliberately lives in the `PostgreSQL` edge adapter: Aequora's protocol and store
/// traits remain independent of `SQLx` and of application domain tables.
#[async_trait]
pub trait PostgresCommitHook: Send + Sync {
    /// Applies and validates the application-owned effect in `transaction`.
    async fn apply(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        commit: &CommitOperation,
    ) -> Result<PostgresCommitHookOutcome, PostgresCommitHookError>;
}

/// Application-owned bootstrap materialization joined to Aequora's repeatable-read snapshot.
///
/// The hook may project authoritative application tables into Aequora by calling
/// [`materialize_snapshot_entity`]. It runs after snapshot isolation is established and before
/// the journal cursor and entity view are captured, so the resulting snapshot cannot miss a
/// concurrent change while claiming a later cursor.
#[async_trait]
pub trait PostgresSnapshotHook: Send + Sync {
    /// Materializes every entity visible in the requested tenant, scope, and partitions.
    async fn materialize(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        tenant: TenantId,
        scope: SyncScopeId,
        partitions: &[Partition],
    ) -> Result<(), StoreError>;
}

/// One additional authorized scope delivery for a server-originated projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresFanoutDelivery {
    /// Scope whose journal receives the projection.
    pub scope_id: SyncScopeId,
    /// Unique event identity for this delivery.
    pub operation_id: OperationId,
    /// Stable identity of this derived fan-out journal event.
    pub event_id: EventId,
}

#[derive(Clone, Copy, Debug, Default)]
struct NoopPostgresCommitHook;

#[async_trait]
impl PostgresCommitHook for NoopPostgresCommitHook {
    async fn apply(
        &self,
        _transaction: &mut Transaction<'_, Postgres>,
        commit: &CommitOperation,
    ) -> Result<PostgresCommitHookOutcome, PostgresCommitHookError> {
        Ok(PostgresCommitHookOutcome::new(commit.payload.clone()))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct NoopPostgresSnapshotHook;

#[async_trait]
impl PostgresSnapshotHook for NoopPostgresSnapshotHook {
    async fn materialize(
        &self,
        _transaction: &mut Transaction<'_, Postgres>,
        _tenant: TenantId,
        _scope: SyncScopeId,
        _partitions: &[Partition],
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

/// Concrete `SQLx` `PostgreSQL` backend implementing every authoritative storage capability.
#[derive(Clone)]
pub struct SqlxPostgresBackend {
    pool: PgPool,
    commit_hook: Arc<dyn PostgresCommitHook>,
    snapshot_hook: Arc<dyn PostgresSnapshotHook>,
}

impl fmt::Debug for SqlxPostgresBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SqlxPostgresBackend")
            .field("pool", &self.pool)
            .field("commit_hook", &"application-owned")
            .field("snapshot_hook", &"application-owned")
            .finish()
    }
}

impl TransactionCapabilityProvider for SqlxPostgresBackend {
    fn transaction_capabilities(&self) -> TransactionCapabilities {
        TransactionCapabilities::FULL_AUTHORITATIVE
    }
}

impl IntegrityCapabilityProvider for SqlxPostgresBackend {
    fn integrity_support(&self) -> IntegritySupport {
        IntegritySupport::SnapshotOnly
    }
}

impl AdapterManifestProvider for SqlxPostgresBackend {
    fn adapter_manifest(&self) -> AdapterManifest {
        POSTGRES_ADAPTER_MANIFEST
    }
}

/// Bounded application-side pool behavior for regular and serverless `PostgreSQL` deployments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostgresPoolConfig {
    /// Maximum connections opened by one Aequora process.
    pub max_connections: u32,
    /// Warm connections retained even when the application is idle.
    pub min_connections: u32,
    /// Maximum time to wait for a healthy pooled connection.
    pub acquire_timeout: Duration,
    /// Maximum time an unused connection remains open.
    pub idle_timeout: Option<Duration>,
    /// Maximum lifetime of one underlying database connection.
    pub max_lifetime: Option<Duration>,
}

impl PostgresPoolConfig {
    /// Creates a conservative pool for an ordinary long-running `PostgreSQL` server.
    #[must_use]
    pub const fn new(max_connections: u32) -> Self {
        Self {
            max_connections,
            min_connections: 0,
            acquire_timeout: Duration::from_secs(30),
            idle_timeout: Some(Duration::from_secs(10 * 60)),
            max_lifetime: Some(Duration::from_secs(30 * 60)),
        }
    }

    /// Creates a scale-to-zero-friendly pool for a Neon pooled runtime endpoint.
    ///
    /// Keeping no minimum connections and reaping idle connections allows the Neon compute to
    /// suspend. Callers should still size `max_connections` across all application replicas.
    #[must_use]
    pub const fn neon(max_connections: u32) -> Self {
        Self {
            max_connections,
            min_connections: 0,
            acquire_timeout: Duration::from_secs(30),
            idle_timeout: Some(Duration::from_secs(5 * 60)),
            max_lifetime: Some(Duration::from_secs(30 * 60)),
        }
    }
}

impl Default for PostgresPoolConfig {
    fn default() -> Self {
        Self::new(10)
    }
}

impl SqlxPostgresBackend {
    /// Connects a bounded `SQLx` pool and installs the Aequora schema.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the pool cannot connect or migration fails.
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self, StoreError> {
        Self::connect_with_config(database_url, PostgresPoolConfig::new(max_connections)).await
    }

    /// Connects using explicit pool lifecycle settings and migrates through the same URL.
    ///
    /// This is appropriate for ordinary `PostgreSQL` servers. For Neon, use
    /// [`Self::connect_neon`] so schema installation uses the direct endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when URL parsing, migration, or runtime connection fails.
    pub async fn connect_with_config(
        database_url: &str,
        config: PostgresPoolConfig,
    ) -> Result<Self, StoreError> {
        Self::connect_with_migration_url(database_url, database_url, config).await
    }

    /// Migrates through a direct/admin URL, closes it, then opens the runtime pool separately.
    ///
    /// This supports deployments where schema changes and application traffic intentionally use
    /// different connection endpoints or roles.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when either URL is invalid, migration fails, or the runtime pool
    /// cannot connect.
    pub async fn connect_with_migration_url(
        runtime_database_url: &str,
        migration_database_url: &str,
        config: PostgresPoolConfig,
    ) -> Result<Self, StoreError> {
        let runtime = parse_connect_options(runtime_database_url, false)?;
        let migration = parse_connect_options(migration_database_url, false)?;
        connect_separate(runtime, migration, config).await
    }

    /// Connects to Neon using a pooled runtime URL and a direct migration URL.
    ///
    /// Both URLs are forced to `verify-full`, regardless of their query parameters, so TLS
    /// certificate and hostname validation cannot silently fall back. The pooled connection is
    /// used only for normal transactions; the direct pool is closed immediately after migration.
    /// Transaction-scoped advisory locks remain compatible with Neon transaction pooling.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when either Neon URL is invalid, migration fails, or the pooled
    /// runtime connection cannot be established.
    pub async fn connect_neon(
        pooled_database_url: &str,
        direct_database_url: &str,
        max_connections: u32,
    ) -> Result<Self, StoreError> {
        Self::connect_neon_with_config(
            pooled_database_url,
            direct_database_url,
            PostgresPoolConfig::neon(max_connections),
        )
        .await
    }

    /// Connects to Neon with caller-controlled scale-to-zero pool limits.
    ///
    /// Start with [`PostgresPoolConfig::neon`] and adjust its public fields only when deployment
    /// concurrency or cold-start measurements justify a different value.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when either Neon URL is invalid, migration fails, or the pooled
    /// runtime connection cannot be established.
    pub async fn connect_neon_with_config(
        pooled_database_url: &str,
        direct_database_url: &str,
        config: PostgresPoolConfig,
    ) -> Result<Self, StoreError> {
        let runtime = parse_connect_options(pooled_database_url, true)?;
        let migration = parse_connect_options(direct_database_url, true)?;
        connect_separate(runtime, migration, config).await
    }

    /// Wraps an application-owned pool. Call [`Self::migrate`] before serving traffic.
    #[must_use]
    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            pool,
            commit_hook: Arc::new(NoopPostgresCommitHook),
            snapshot_hook: Arc::new(NoopPostgresSnapshotHook),
        }
    }

    /// Installs an application-owned domain hook for authoritative commits.
    ///
    /// This consumes the backend so the hook cannot be changed while it is serving traffic.
    /// Duplicate operation deliveries return their original acknowledgement without invoking the
    /// hook again.
    #[must_use]
    pub fn with_commit_hook(mut self, hook: impl PostgresCommitHook + 'static) -> Self {
        self.commit_hook = Arc::new(hook);
        self
    }

    /// Installs an application-owned materializer for consistent bootstrap snapshots.
    ///
    /// This consumes the backend so the hook cannot be changed while it is serving traffic.
    #[must_use]
    pub fn with_snapshot_hook(mut self, hook: impl PostgresSnapshotHook + 'static) -> Self {
        self.snapshot_hook = Arc::new(hook);
        self
    }

    /// Borrows the application-owned `SQLx` pool.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Initializes the singleton authority metadata row or verifies the existing identity.
    ///
    /// # Errors
    ///
    /// Rejects a database already initialized for a different authority or timeline.
    pub async fn initialize_authority_state(
        &self,
        state: AuthorityState,
    ) -> Result<AuthorityState, StoreError> {
        sqlx::query(
            "INSERT INTO aequora_authority_state
                (singleton,authority_id,authority_epoch,authority_instance_id,role,fence_token,
                 runtime_mode,promotion_class,transition_id,updated_at_unix_ms)
             VALUES (TRUE,$1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (singleton) DO NOTHING",
        )
        .bind(state.authority_id.as_uuid())
        .bind(to_i64(state.epoch.get(), "authority epoch")?)
        .bind(state.instance_id.as_uuid())
        .bind(authority_role_name(state.role))
        .bind(to_i64(state.fence_token.get(), "authority fence token")?)
        .bind(runtime_mode_name(state.runtime_mode))
        .bind(promotion_class_name(state.last_promotion_class))
        .bind(state.transition_id.as_uuid())
        .bind(to_i64(
            state.updated_at_unix_ms,
            "authority update timestamp",
        )?)
        .execute(&self.pool)
        .await
        .map_err(postgres_error)?;
        let stored = self
            .load_authority_state()
            .await?
            .ok_or_else(|| corrupt("PostgreSQL authority state was not initialized"))?;
        if stored != state {
            return Err(corrupt(
                "PostgreSQL authority state already belongs to different metadata",
            ));
        }
        Ok(stored)
    }

    /// Loads the durable singleton authority state used by startup validation.
    ///
    /// # Errors
    ///
    /// Returns a typed store error when the query fails or persisted metadata is invalid.
    pub async fn load_authority_state(&self) -> Result<Option<AuthorityState>, StoreError> {
        let row = sqlx::query(
            "SELECT authority_id,authority_epoch,authority_instance_id,role,fence_token,
                    runtime_mode,promotion_class,transition_id,updated_at_unix_ms
               FROM aequora_authority_state WHERE singleton=TRUE",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(postgres_error)?;
        row.map(|row| authority_state_from_row(&row)).transpose()
    }

    /// Atomically replaces authority state under the old instance/epoch/fence and records the
    /// immutable transition manifest in the same transaction.
    ///
    /// # Errors
    ///
    /// Returns a typed store error for an invalid manifest, stale expected fence, or transaction
    /// failure.
    pub async fn persist_authority_transition(
        &self,
        expected: AuthorityState,
        replacement: AuthorityState,
        manifest: AuthorityTransitionManifest,
    ) -> Result<(), StoreError> {
        manifest
            .verify()
            .map_err(|error| corrupt(error.to_string()))?;
        if replacement.authority_id != expected.authority_id
            || manifest.authority_id != expected.authority_id
            || replacement.transition_id != manifest.transition_id
            || replacement.epoch != manifest.new_epoch
            || replacement.fence_token != manifest.fence_token
        {
            return Err(corrupt(
                "authority replacement state does not match its transition manifest",
            ));
        }
        let mut transaction = self.pool.begin().await.map_err(postgres_error)?;
        let changed = sqlx::query(
            "UPDATE aequora_authority_state
                SET authority_epoch=$1,authority_instance_id=$2,role=$3,fence_token=$4,
                    runtime_mode=$5,promotion_class=$6,transition_id=$7,updated_at_unix_ms=$8
              WHERE singleton=TRUE AND authority_id=$9 AND authority_epoch=$10
                AND authority_instance_id=$11 AND fence_token=$12 AND transition_id=$13",
        )
        .bind(to_i64(replacement.epoch.get(), "authority epoch")?)
        .bind(replacement.instance_id.as_uuid())
        .bind(authority_role_name(replacement.role))
        .bind(to_i64(
            replacement.fence_token.get(),
            "authority fence token",
        )?)
        .bind(runtime_mode_name(replacement.runtime_mode))
        .bind(promotion_class_name(replacement.last_promotion_class))
        .bind(replacement.transition_id.as_uuid())
        .bind(to_i64(
            replacement.updated_at_unix_ms,
            "authority update timestamp",
        )?)
        .bind(expected.authority_id.as_uuid())
        .bind(to_i64(expected.epoch.get(), "expected authority epoch")?)
        .bind(expected.instance_id.as_uuid())
        .bind(to_i64(
            expected.fence_token.get(),
            "expected authority fence token",
        )?)
        .bind(expected.transition_id.as_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;
        if changed.rows_affected() != 1 {
            return Err(StoreError::leadership_lost(
                "authority transition lost its epoch or fencing compare-and-swap",
            ));
        }
        sqlx::query(
            "INSERT INTO aequora_authority_transitions
                (transition_id,authority_id,old_epoch,new_epoch,promotion_class,
                 old_final_sequence,new_base_sequence,reason,created_at_unix_ms,fence_token,
                 manifest_digest)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(manifest.transition_id.as_uuid())
        .bind(manifest.authority_id.as_uuid())
        .bind(to_i64(manifest.old_epoch.get(), "old authority epoch")?)
        .bind(to_i64(manifest.new_epoch.get(), "new authority epoch")?)
        .bind(promotion_class_name(manifest.promotion_class))
        .bind(
            manifest
                .old_final_sequence
                .map(|sequence| to_i64(sequence.0, "old final sequence"))
                .transpose()?,
        )
        .bind(to_i64(manifest.new_base_sequence.0, "new base sequence")?)
        .bind(transition_reason_name(manifest.reason))
        .bind(to_i64(manifest.created_at_unix_ms, "transition timestamp")?)
        .bind(to_i64(
            manifest.fence_token.get(),
            "transition fence token",
        )?)
        .bind(manifest.canonical_digest().as_slice())
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;
        transaction.commit().await.map_err(postgres_error)
    }

    /// Publishes an application/server-originated authoritative change inside an existing
    /// application transaction.
    ///
    /// This is the write-side counterpart to [`PostgresCommitHook`]: use it when the application
    /// already owns the domain transaction instead of receiving a client operation through
    /// Aequora. Entity comparison, journal append, idempotency evidence, and audit evidence are
    /// still atomic with the caller's domain writes. The caller remains responsible for committing
    /// or rolling back `transaction`; the configured client-operation hook is not invoked.
    ///
    /// # Errors
    ///
    /// Returns a storage error for invalid version transitions or rejected `PostgreSQL` writes.
    pub async fn publish_in_transaction(
        transaction: &mut Transaction<'_, Postgres>,
        commit: &CommitOperation,
        authoritative_payload: &[u8],
    ) -> Result<CommitOutcome, StoreError> {
        if !commit.has_valid_version_transition() {
            return Err(corrupt(
                "authoritative entity version must advance by exactly one",
            ));
        }
        if let Some(outcome) = prepare_commit_in_transaction(transaction, commit).await? {
            return Ok(outcome);
        }
        persist_commit_in_transaction(transaction, commit, authoritative_payload)
            .await
            .map(CommitOutcome::Applied)
    }

    /// Publishes one server-originated entity change to every authorized scope atomically.
    ///
    /// `commit.scope_id` is the primary delivery. Additional deliveries allocate independent
    /// monotonic sequences and event IDs without mutating the entity or command ledger again.
    /// A duplicate primary operation proves the earlier transaction, including all deliveries,
    /// committed and therefore returns without appending regenerated fan-out IDs.
    ///
    /// # Errors
    ///
    /// Returns a storage error for repeated scopes/event IDs, invalid versions, or rejected
    /// `PostgreSQL` writes. The caller owns the surrounding commit/rollback.
    pub async fn publish_to_scopes_in_transaction(
        transaction: &mut Transaction<'_, Postgres>,
        commit: &CommitOperation,
        authoritative_payload: &[u8],
        additional_deliveries: &[PostgresFanoutDelivery],
    ) -> Result<CommitOutcome, StoreError> {
        let mut scopes = std::collections::BTreeSet::new();
        scopes.insert(commit.scope_id);
        let mut operation_ids = std::collections::BTreeSet::new();
        operation_ids.insert(commit.operation_id);
        for delivery in additional_deliveries {
            if !scopes.insert(delivery.scope_id) || !operation_ids.insert(delivery.operation_id) {
                return Err(corrupt(
                    "fan-out deliveries must have unique scopes and event IDs",
                ));
            }
        }
        let outcome =
            Self::publish_in_transaction(transaction, commit, authoritative_payload).await?;
        if matches!(outcome, CommitOutcome::Applied(_)) {
            for delivery in additional_deliveries {
                append_fanout_event_in_transaction(
                    transaction,
                    commit,
                    authoritative_payload,
                    *delivery,
                )
                .await?;
            }
        }
        Ok(outcome)
    }

    /// Verifies that the runtime pool can acquire a healthy connection, the schema is current,
    /// and a transaction can access and no-op-write critical journal/ledger metadata before an
    /// explicit rollback. The probe never inserts or changes domain state.
    ///
    /// This is suitable for application readiness checks, including after a Neon compute wakes
    /// from suspension.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when no healthy connection can be acquired or queried.
    pub async fn health_check(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(postgres_error)?;
        let status = self.schema_status().await?;
        if !status.is_current() {
            return Err(corrupt(format!(
                "PostgreSQL schema is at version {}, but Aequora requires version {}",
                status.applied_version, status.expected_version
            )));
        }
        let mut transaction = self.pool.begin().await.map_err(postgres_error)?;
        sqlx::query("UPDATE aequora_sync_events SET sequence = sequence WHERE FALSE")
            .execute(&mut *transaction)
            .await
            .map_err(postgres_error)?;
        sqlx::query(
            "UPDATE aequora_applied_operations SET server_sequence = server_sequence WHERE FALSE",
        )
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;
        sqlx::query("SELECT minimum_cursor FROM aequora_journal_floors LIMIT 0")
            .execute(&mut *transaction)
            .await
            .map_err(postgres_error)?;
        transaction.rollback().await.map_err(postgres_error)?;
        Ok(())
    }

    /// Reads the durable migration ledger for readiness and deployment diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the ledger is unavailable, corrupt, or newer than this build.
    pub async fn schema_status(&self) -> Result<PostgresSchemaStatus, StoreError> {
        let rows = sqlx::query(
            "SELECT version, name, checksum FROM aequora_schema_migrations ORDER BY version",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(postgres_error)?;
        let history = decode_migration_rows(rows)?;
        let applied_version = verify_migration_history(&history)?;
        Ok(PostgresSchemaStatus {
            applied_version,
            expected_version: POSTGRES_SCHEMA_VERSION,
        })
    }

    /// Installs or upgrades the idempotent Aequora schema.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `PostgreSQL` rejects the migration.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        migrate_pool(&self.pool).await
    }
}

fn parse_connect_options(
    database_url: &str,
    verify_full: bool,
) -> Result<PgConnectOptions, StoreError> {
    let options = PgConnectOptions::from_str(database_url).map_err(postgres_error)?;
    Ok(if verify_full {
        options.ssl_mode(PgSslMode::VerifyFull)
    } else {
        options
    })
}

fn pool_options(config: PostgresPoolConfig) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(config.max_connections.max(1))
        .min_connections(config.min_connections.min(config.max_connections.max(1)))
        .acquire_timeout(config.acquire_timeout)
        .idle_timeout(config.idle_timeout)
        .max_lifetime(config.max_lifetime)
        .test_before_acquire(true)
}

async fn connect_separate(
    runtime: PgConnectOptions,
    migration: PgConnectOptions,
    config: PostgresPoolConfig,
) -> Result<SqlxPostgresBackend, StoreError> {
    let migration_pool = PgPoolOptions::new()
        .max_connections(1)
        .min_connections(0)
        .acquire_timeout(config.acquire_timeout)
        .connect_with(migration)
        .await
        .map_err(postgres_error)?;
    let migration_result = migrate_pool(&migration_pool).await;
    migration_pool.close().await;
    migration_result?;

    let pool = pool_options(config)
        .connect_with(runtime)
        .await
        .map_err(postgres_error)?;
    Ok(SqlxPostgresBackend::from_pool(pool))
}

async fn migrate_pool(pool: &PgPool) -> Result<(), StoreError> {
    let mut transaction = pool.begin().await.map_err(postgres_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('aequora-schema-migration', 0))")
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;
    sqlx::raw_sql(MIGRATION_LEDGER_SQL)
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;

    let rows = sqlx::query(
        "SELECT version, name, checksum FROM aequora_schema_migrations ORDER BY version",
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(postgres_error)?;
    let history = decode_migration_rows(rows)?;
    verify_migration_history(&history)?;

    for migration in POSTGRES_MIGRATIONS {
        let existing =
            sqlx::query("SELECT name, checksum FROM aequora_schema_migrations WHERE version = $1")
                .bind(
                    i32::try_from(migration.version)
                        .map_err(|_| corrupt("migration version overflow"))?,
                )
                .fetch_optional(&mut *transaction)
                .await
                .map_err(postgres_error)?;
        if let Some(row) = existing {
            let name = row.try_get::<String, _>("name").map_err(postgres_error)?;
            let checksum = row
                .try_get::<Vec<u8>, _>("checksum")
                .map_err(postgres_error)?;
            verify_migration_record(*migration, &name, &checksum)?;
            continue;
        }

        sqlx::raw_sql(migration.sql)
            .execute(&mut *transaction)
            .await
            .map_err(postgres_error)?;
        sqlx::query(
            "INSERT INTO aequora_schema_migrations (version, name, checksum) VALUES ($1, $2, $3)",
        )
        .bind(i32::try_from(migration.version).map_err(|_| corrupt("migration version overflow"))?)
        .bind(migration.name)
        .bind(migration_checksum(migration.sql).as_slice())
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;
    }
    transaction.commit().await.map_err(postgres_error)
}

fn migration_checksum(sql: &str) -> [u8; 32] {
    *blake3::hash(sql.as_bytes()).as_bytes()
}

fn decode_migration_rows(
    rows: Vec<sqlx::postgres::PgRow>,
) -> Result<Vec<AppliedMigration>, StoreError> {
    rows.into_iter()
        .map(|row| {
            Ok(AppliedMigration {
                version: row.try_get::<i32, _>("version").map_err(postgres_error)?,
                name: row.try_get::<String, _>("name").map_err(postgres_error)?,
                checksum: row
                    .try_get::<Vec<u8>, _>("checksum")
                    .map_err(postgres_error)?,
            })
        })
        .collect()
}

fn verify_migration_history(history: &[AppliedMigration]) -> Result<u32, StoreError> {
    let mut previous = 0_u32;
    for applied in history {
        let version = u32::try_from(applied.version)
            .map_err(|_| corrupt("negative PostgreSQL schema migration version"))?;
        if version != previous.saturating_add(1) {
            return Err(corrupt(format!(
                "PostgreSQL migration history is not contiguous at version {version}"
            )));
        }
        let Some(migration) = POSTGRES_MIGRATIONS
            .iter()
            .find(|migration| migration.version == version)
        else {
            return Err(corrupt(format!(
                "PostgreSQL schema version {version} is newer than supported version {POSTGRES_SCHEMA_VERSION}"
            )));
        };
        verify_migration_record(*migration, &applied.name, &applied.checksum)?;
        previous = version;
    }
    Ok(previous)
}

fn verify_migration_record(
    migration: PostgresMigration,
    applied_name: &str,
    applied_checksum: &[u8],
) -> Result<(), StoreError> {
    if applied_name != migration.name {
        return Err(corrupt(format!(
            "PostgreSQL migration {} name drift: expected {}, found {applied_name}",
            migration.version, migration.name
        )));
    }
    if applied_checksum != migration_checksum(migration.sql) {
        return Err(corrupt(format!(
            "PostgreSQL migration {} checksum drift detected",
            migration.version
        )));
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn authority_role_name(role: AuthorityRole) -> &'static str {
    match role {
        AuthorityRole::Primary => "primary",
        AuthorityRole::Standby => "standby",
        AuthorityRole::ReadReplica => "read_replica",
        AuthorityRole::Recovering => "recovering",
        AuthorityRole::Demoted => "demoted",
    }
}

fn runtime_mode_name(mode: AuthorityRuntimeMode) -> &'static str {
    match mode {
        AuthorityRuntimeMode::Serving => "serving",
        AuthorityRuntimeMode::Recovering => "recovering",
        AuthorityRuntimeMode::ReadOnlyVerification => "read_only_verification",
        AuthorityRuntimeMode::PromotionPending => "promotion_pending",
        AuthorityRuntimeMode::Quarantined => "quarantined",
    }
}

fn promotion_class_name(class: PromotionClass) -> &'static str {
    match class {
        PromotionClass::LosslessContinuation => "lossless_continuation",
        PromotionClass::PotentialDataLoss => "potential_data_loss",
        PromotionClass::RestoredTimeline => "restored_timeline",
        PromotionClass::NewAuthorityMigration => "new_authority_migration",
    }
}

fn transition_reason_name(reason: TransitionReason) -> &'static str {
    match reason {
        TransitionReason::PlannedMigration => "planned_migration",
        TransitionReason::RegionFailover => "region_failover",
        TransitionReason::PointInTimeRestore => "point_in_time_restore",
        TransitionReason::CorruptionRecovery => "corruption_recovery",
        TransitionReason::StandbyPromotion => "standby_promotion",
        TransitionReason::ForkResolution => "fork_resolution",
        TransitionReason::OperatorDemotion => "operator_demotion",
    }
}

fn authority_state_from_row(row: &sqlx::postgres::PgRow) -> Result<AuthorityState, StoreError> {
    let epoch = AuthorityEpoch::new(from_i64(
        row.try_get("authority_epoch").map_err(postgres_error)?,
        "authority epoch",
    )?)
    .map_err(|error| corrupt(error.to_string()))?;
    let fence_token = aequora_authority::AuthorityFenceToken::new(from_i64(
        row.try_get("fence_token").map_err(postgres_error)?,
        "authority fence token",
    )?)
    .map_err(|error| corrupt(error.to_string()))?;
    let role = match row
        .try_get::<String, _>("role")
        .map_err(postgres_error)?
        .as_str()
    {
        "primary" => AuthorityRole::Primary,
        "standby" => AuthorityRole::Standby,
        "read_replica" => AuthorityRole::ReadReplica,
        "recovering" => AuthorityRole::Recovering,
        "demoted" => AuthorityRole::Demoted,
        _ => return Err(corrupt("unknown PostgreSQL authority role")),
    };
    let runtime_mode = match row
        .try_get::<String, _>("runtime_mode")
        .map_err(postgres_error)?
        .as_str()
    {
        "serving" => AuthorityRuntimeMode::Serving,
        "recovering" => AuthorityRuntimeMode::Recovering,
        "read_only_verification" => AuthorityRuntimeMode::ReadOnlyVerification,
        "promotion_pending" => AuthorityRuntimeMode::PromotionPending,
        "quarantined" => AuthorityRuntimeMode::Quarantined,
        _ => return Err(corrupt("unknown PostgreSQL authority runtime mode")),
    };
    let last_promotion_class = match row
        .try_get::<String, _>("promotion_class")
        .map_err(postgres_error)?
        .as_str()
    {
        "lossless_continuation" => PromotionClass::LosslessContinuation,
        "potential_data_loss" => PromotionClass::PotentialDataLoss,
        "restored_timeline" => PromotionClass::RestoredTimeline,
        "new_authority_migration" => PromotionClass::NewAuthorityMigration,
        _ => return Err(corrupt("unknown PostgreSQL authority promotion class")),
    };
    Ok(AuthorityState {
        authority_id: AuthorityId::from_uuid(row.try_get("authority_id").map_err(postgres_error)?),
        epoch,
        role,
        instance_id: AuthorityInstanceId::from_uuid(
            row.try_get("authority_instance_id")
                .map_err(postgres_error)?,
        ),
        fence_token,
        runtime_mode,
        last_promotion_class,
        transition_id: AuthorityTransitionId::from_uuid(
            row.try_get("transition_id").map_err(postgres_error)?,
        ),
        updated_at_unix_ms: from_i64(
            row.try_get("updated_at_unix_ms").map_err(postgres_error)?,
            "authority update timestamp",
        )?,
    })
}

#[allow(clippy::needless_pass_by_value)]
fn postgres_error(error: sqlx::Error) -> StoreError {
    let reason = error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .and_then(|code| postgres_transaction_retry_reason(code.as_ref()));
    reason.map_or_else(
        || StoreError::transient(format!("PostgreSQL operation failed: {error}")),
        |reason| {
            StoreError::transient_with_reason(
                reason,
                format!("PostgreSQL transaction aborted: {error}"),
            )
        },
    )
}

fn postgres_transaction_retry_reason(code: &str) -> Option<StoreErrorReason> {
    match code {
        "40001" => Some(StoreErrorReason::SerializationFailure),
        "40P01" => Some(StoreErrorReason::Deadlock),
        _ => None,
    }
}

fn corrupt(message: impl Into<String>) -> StoreError {
    StoreError::permanent(message)
}

fn to_i64(value: u64, field: &str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| corrupt(format!("{field} exceeds PostgreSQL BIGINT range")))
}

fn from_i64(value: i64, field: &str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| corrupt(format!("negative {field} in PostgreSQL")))
}

fn entity_type(value: i32) -> Result<EntityType, StoreError> {
    let value = u16::try_from(value).map_err(|_| corrupt("negative PostgreSQL entity type"))?;
    EntityType::new(value).map_err(|_| corrupt("zero PostgreSQL entity type"))
}

fn entity_version(value: i64) -> Result<EntityVersion, StoreError> {
    EntityVersion::new(from_i64(value, "entity version")?)
        .map_err(|_| corrupt("zero PostgreSQL entity version"))
}

fn node_id(row: &sqlx::postgres::PgRow, column: &str) -> Result<NodeId, StoreError> {
    row.try_get::<uuid::Uuid, _>(column)
        .map(NodeId::from_uuid)
        .map_err(postgres_error)
}

fn change_kind(value: i16) -> Result<ChangeKind, StoreError> {
    match value {
        1 => Ok(ChangeKind::Upsert),
        2 => Ok(ChangeKind::Tombstone),
        _ => Err(corrupt("invalid PostgreSQL change kind")),
    }
}

const fn change_kind_code(value: ChangeKind) -> i16 {
    match value {
        ChangeKind::Upsert => 1,
        ChangeKind::Tombstone => 2,
    }
}

/// Materializes one application-owned entity for a consistent snapshot.
///
/// Call this only from [`PostgresSnapshotHook::materialize`] using the transaction supplied to
/// that hook. Existing payloads are replaced because the application tables are authoritative,
/// while an existing Aequora version is never regressed by a lower application row version. No
/// journal entry is appended: the entity is delivered by the snapshot whose boundary is captured
/// after materialization.
///
/// # Errors
///
/// Returns a storage error if `PostgreSQL` rejects either the entity or scope write.
pub async fn materialize_snapshot_entity(
    transaction: &mut Transaction<'_, Postgres>,
    tenant: TenantId,
    scope: SyncScopeId,
    entity: &SnapshotEntity,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO aequora_entities
            (tenant_id, entity_type, entity_id, version, payload, tombstone)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (tenant_id, entity_type, entity_id) DO UPDATE SET
            version = GREATEST(aequora_entities.version, EXCLUDED.version),
            payload = EXCLUDED.payload,
            tombstone = EXCLUDED.tombstone",
    )
    .bind(tenant.as_uuid())
    .bind(i32::from(entity.entity.entity_type.get()))
    .bind(entity.entity.entity_id.as_uuid())
    .bind(to_i64(entity.version.get(), "snapshot entity version")?)
    .bind(&entity.payload)
    .bind(entity.tombstone)
    .execute(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    sqlx::query(
        "INSERT INTO aequora_entity_scopes
            (tenant_id, scope_id, entity_type, entity_id)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT DO NOTHING",
    )
    .bind(tenant.as_uuid())
    .bind(scope.as_uuid())
    .bind(i32::from(entity.entity.entity_type.get()))
    .bind(entity.entity.entity_id.as_uuid())
    .execute(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    Ok(())
}

async fn prepare_commit_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    commit: &CommitOperation,
) -> Result<Option<CommitOutcome>, StoreError> {
    if let Some(authority) = commit.authority {
        let current: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM aequora_authority_state
                 WHERE singleton=TRUE AND authority_id=$1 AND authority_epoch=$2
                   AND authority_instance_id=$3 AND fence_token=$4
                   AND role='primary' AND runtime_mode='serving'
             )",
        )
        .bind(authority.authority_id.as_uuid())
        .bind(to_i64(authority.epoch.get(), "commit authority epoch")?)
        .bind(authority.instance_id.as_uuid())
        .bind(to_i64(
            authority.fence_token.get(),
            "commit authority fence token",
        )?)
        .fetch_one(&mut **transaction)
        .await
        .map_err(postgres_error)?;
        if !current {
            return Err(StoreError::leadership_lost(
                "authoritative commit presented a stale database authority fence",
            ));
        }
    }
    let operation_lock_key = format!("operation:{}:{}", commit.tenant_id, commit.operation_id);
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(operation_lock_key)
        .execute(&mut **transaction)
        .await
        .map_err(postgres_error)?;
    let entity_lock_key = format!(
        "entity:{}:{}:{}",
        commit.tenant_id,
        commit.entity.entity_type.get(),
        commit.entity.entity_id
    );
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(entity_lock_key)
        .execute(&mut **transaction)
        .await
        .map_err(postgres_error)?;
    if let Some(row) = sqlx::query(
        "SELECT entity_version, server_sequence, event_id, correlation_id,
                caused_by_kind, caused_by_id
           FROM aequora_applied_operations
          WHERE tenant_id = $1 AND operation_id = $2",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(commit.operation_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(postgres_error)?
    {
        let stored_lineage = LineageContext {
            correlation_id: CorrelationId::from_uuid(
                row.try_get("correlation_id").map_err(postgres_error)?,
            ),
            caused_by: lineage_ref_from_row(&row)?,
        };
        if stored_lineage != commit.operation_lineage {
            return Err(StoreError::permanent(
                "retry changed operation lineage for an existing OperationId",
            ));
        }
        return Ok(Some(CommitOutcome::Duplicate(ack_from_row(
            commit.operation_id,
            &row,
            true,
        )?)));
    }
    let current = sqlx::query(
        "SELECT version FROM aequora_entities
          WHERE tenant_id = $1 AND entity_type = $2 AND entity_id = $3
          FOR UPDATE",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(i32::from(commit.entity.entity_type.get()))
    .bind(commit.entity.entity_id.as_uuid())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(postgres_error)?
    .map(|row| {
        row.try_get::<i64, _>("version")
            .map_err(postgres_error)
            .and_then(entity_version)
    })
    .transpose()?;
    if current != commit.expected_version {
        return Ok(Some(CommitOutcome::VersionChanged { current }));
    }
    Ok(None)
}

#[allow(clippy::too_many_lines)]
async fn persist_commit_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    commit: &CommitOperation,
    authoritative_payload: &[u8],
) -> Result<OperationAck, StoreError> {
    let next_version = to_i64(commit.next_version.get(), "next entity version")?;
    let (operation_cause_kind, operation_cause_id) = lineage_ref_columns(commit.operation_lineage);
    let event_lineage = commit.event_lineage();
    let (event_cause_kind, event_cause_id) = lineage_ref_columns(event_lineage);
    let authority_id = commit
        .authority
        .map(|authority| authority.authority_id.as_uuid());
    let authority_epoch = commit
        .authority
        .map(|authority| to_i64(authority.epoch.get(), "commit authority epoch"))
        .transpose()?;
    sqlx::query(
        "INSERT INTO aequora_entities (tenant_id, entity_type, entity_id, version, payload, tombstone) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (tenant_id, entity_type, entity_id) DO UPDATE SET version = EXCLUDED.version, payload = EXCLUDED.payload, tombstone = EXCLUDED.tombstone",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(i32::from(commit.entity.entity_type.get()))
    .bind(commit.entity.entity_id.as_uuid())
    .bind(next_version)
    .bind(authoritative_payload)
    .bind(matches!(commit.change_kind, ChangeKind::Tombstone))
    .execute(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    sqlx::query(
        "INSERT INTO aequora_entity_scopes (tenant_id, scope_id, entity_type, entity_id) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(commit.scope_id.as_uuid())
    .bind(i32::from(commit.entity.entity_type.get()))
    .bind(commit.entity.entity_id.as_uuid())
    .execute(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    let sequence: i64 = sqlx::query_scalar(
        "INSERT INTO aequora_scope_sequences (tenant_id, scope_id, sequence) VALUES ($1, $2, 1) ON CONFLICT (tenant_id, scope_id) DO UPDATE SET sequence = aequora_scope_sequences.sequence + 1 RETURNING sequence",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(commit.scope_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    sqlx::query(
        "INSERT INTO aequora_sync_events (tenant_id, scope_id, sequence, operation_id, event_id, correlation_id, caused_by_kind, caused_by_id, entity_type, entity_id, entity_version, change_kind, payload, physical_ms, logical_clock, clock_node, authority_id, authority_epoch) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(commit.scope_id.as_uuid())
    .bind(sequence)
    .bind(commit.operation_id.as_uuid())
    .bind(commit.event_id.as_uuid())
    .bind(event_lineage.correlation_id.as_uuid())
    .bind(event_cause_kind)
    .bind(event_cause_id)
    .bind(i32::from(commit.entity.entity_type.get()))
    .bind(commit.entity.entity_id.as_uuid())
    .bind(next_version)
    .bind(change_kind_code(commit.change_kind))
    .bind(authoritative_payload)
    .bind(commit.timestamp.physical_ms)
    .bind(
        i32::try_from(commit.timestamp.logical)
            .map_err(|_| corrupt("logical clock exceeds PostgreSQL INTEGER"))?,
    )
    .bind(commit.timestamp.node.as_uuid())
    .bind(authority_id)
    .bind(authority_epoch)
    .execute(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    sqlx::query(
        "INSERT INTO aequora_applied_operations (tenant_id, operation_id, event_id, correlation_id, caused_by_kind, caused_by_id, entity_version, scope_id, server_sequence, authority_id, authority_epoch) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(commit.operation_id.as_uuid())
    .bind(commit.event_id.as_uuid())
    .bind(commit.operation_lineage.correlation_id.as_uuid())
    .bind(operation_cause_kind)
    .bind(operation_cause_id)
    .bind(next_version)
    .bind(commit.scope_id.as_uuid())
    .bind(sequence)
    .bind(authority_id)
    .bind(authority_epoch)
    .execute(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    sqlx::query(
        "INSERT INTO aequora_audit_log (tenant_id, operation_id, event_id, correlation_id, caused_by_kind, caused_by_id, actor_id, device_id, operation_kind, entity_type, entity_id, entity_version, command_digest, physical_ms, logical_clock, clock_node, authority_id, authority_epoch) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(commit.operation_id.as_uuid())
    .bind(commit.event_id.as_uuid())
    .bind(commit.operation_lineage.correlation_id.as_uuid())
    .bind(operation_cause_kind)
    .bind(operation_cause_id)
    .bind(commit.actor_id.as_uuid())
    .bind(commit.device_id.as_uuid())
    .bind(i32::from(commit.operation_kind))
    .bind(i32::from(commit.entity.entity_type.get()))
    .bind(commit.entity.entity_id.as_uuid())
    .bind(next_version)
    .bind(commit.command_digest.as_slice())
    .bind(commit.timestamp.physical_ms)
    .bind(
        i32::try_from(commit.timestamp.logical)
            .map_err(|_| corrupt("logical clock exceeds PostgreSQL INTEGER"))?,
    )
    .bind(commit.timestamp.node.as_uuid())
    .bind(authority_id)
    .bind(authority_epoch)
    .execute(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    Ok(OperationAck {
        operation_id: commit.operation_id,
        event_id: commit.event_id,
        lineage: event_lineage,
        entity_version: commit.next_version,
        sequence: Sequence(from_i64(sequence, "server sequence")?),
        duplicate: false,
    })
}

async fn append_fanout_event_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    commit: &CommitOperation,
    authoritative_payload: &[u8],
    delivery: PostgresFanoutDelivery,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO aequora_entity_scopes (tenant_id,scope_id,entity_type,entity_id)
         VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(delivery.scope_id.as_uuid())
    .bind(i32::from(commit.entity.entity_type.get()))
    .bind(commit.entity.entity_id.as_uuid())
    .execute(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    let sequence: i64 = sqlx::query_scalar(
        "INSERT INTO aequora_scope_sequences (tenant_id,scope_id,sequence)
         VALUES ($1,$2,1)
         ON CONFLICT (tenant_id,scope_id) DO UPDATE
             SET sequence=aequora_scope_sequences.sequence+1
         RETURNING sequence",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(delivery.scope_id.as_uuid())
    .fetch_one(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    let lineage = commit
        .event_lineage()
        .derived(LineageRef::Event(commit.event_id));
    let (cause_kind, cause_id) = lineage_ref_columns(lineage);
    let authority_id = commit
        .authority
        .map(|authority| authority.authority_id.as_uuid());
    let authority_epoch = commit
        .authority
        .map(|authority| to_i64(authority.epoch.get(), "fan-out authority epoch"))
        .transpose()?;
    sqlx::query(
        "INSERT INTO aequora_sync_events
            (tenant_id,scope_id,sequence,operation_id,event_id,correlation_id,caused_by_kind,
             caused_by_id,entity_type,entity_id,entity_version,change_kind,payload,physical_ms,
             logical_clock,clock_node,authority_id,authority_epoch)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
    )
    .bind(commit.tenant_id.as_uuid())
    .bind(delivery.scope_id.as_uuid())
    .bind(sequence)
    .bind(delivery.operation_id.as_uuid())
    .bind(delivery.event_id.as_uuid())
    .bind(lineage.correlation_id.as_uuid())
    .bind(cause_kind)
    .bind(cause_id)
    .bind(i32::from(commit.entity.entity_type.get()))
    .bind(commit.entity.entity_id.as_uuid())
    .bind(to_i64(commit.next_version.get(), "fan-out entity version")?)
    .bind(change_kind_code(commit.change_kind))
    .bind(authoritative_payload)
    .bind(commit.timestamp.physical_ms)
    .bind(
        i32::try_from(commit.timestamp.logical)
            .map_err(|_| corrupt("logical clock exceeds PostgreSQL INTEGER"))?,
    )
    .bind(commit.timestamp.node.as_uuid())
    .bind(authority_id)
    .bind(authority_epoch)
    .execute(&mut **transaction)
    .await
    .map_err(postgres_error)?;
    Ok(())
}

/// Database-specific implementation point. `commit_operation` must run version comparison,
/// entity mutation, scoped sequence allocation, journal append, and ledger insert in one
/// `PostgreSQL` transaction. It should serialize concurrent retries by operation ID.
#[async_trait]
pub trait PostgresBackend: Send + Sync {
    /// Reads one authoritative entity.
    async fn read_entity(
        &self,
        tenant: TenantId,
        entity: EntityRef,
    ) -> Result<Option<EntitySnapshot>, StoreError>;
    /// Reads an idempotency-ledger result.
    async fn operation_result(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
    ) -> Result<Option<aequora_protocol::OperationAck>, StoreError>;
    /// Reads the original operation lineage used to reject altered retries.
    async fn operation_lineage(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
    ) -> Result<Option<LineageContext>, StoreError>;
    /// Performs the critical authoritative atomic transaction.
    async fn commit_operation(&self, commit: CommitOperation) -> Result<CommitOutcome, StoreError>;
    /// Loads the oldest cursor from which retained journal history is complete.
    async fn minimum_retained_cursor(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
    ) -> Result<Sequence, StoreError>;
    /// Reads an ordered, tenant- and scope-bounded journal page.
    async fn read_changes_after(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        sequence: Sequence,
        limit: usize,
        max_payload_bytes: usize,
    ) -> Result<ChangePage, StoreError>;
    /// Captures a consistent partial-scope snapshot.
    async fn create_snapshot(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        partitions: &[Partition],
    ) -> Result<SnapshotDescriptor, StoreError>;
    /// Captures a snapshot bound to an explicit authority timeline.
    async fn create_snapshot_in_timeline(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        partitions: &[Partition],
        authority_id: AuthorityId,
        authority_epoch: AuthorityEpoch,
    ) -> Result<SnapshotDescriptor, StoreError> {
        let mut descriptor = self.create_snapshot(tenant, scope, partitions).await?;
        descriptor.cursor.authority_id = authority_id;
        descriptor.cursor.authority_epoch = authority_epoch;
        Ok(descriptor)
    }
    /// Reads a bounded page from a captured snapshot.
    async fn read_snapshot(
        &self,
        tenant: TenantId,
        snapshot_id: SnapshotId,
        offset: u64,
        max_entities: usize,
        max_payload_bytes: usize,
    ) -> Result<SnapshotPage, StoreError>;
    /// Deletes compactable synchronization events without touching the operation ledger/audit.
    async fn compact_journal(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        through: Sequence,
    ) -> Result<u64, StoreError>;
    /// Reads immutable accountability records independently of journal retention.
    async fn read_audit_after(
        &self,
        tenant: TenantId,
        offset: AuditOffset,
        limit: usize,
    ) -> Result<AuditPage, StoreError>;
    /// Reads a tenant-bounded causal chain from durable audit evidence.
    async fn read_correlation(
        &self,
        tenant: TenantId,
        correlation_id: CorrelationId,
        offset: AuditOffset,
        limit: usize,
    ) -> Result<AuditPage, StoreError>;
    /// Computes a canonical digest tree from one repeatable-read authoritative boundary.
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

#[async_trait]
impl PostgresBackend for SqlxPostgresBackend {
    async fn read_entity(
        &self,
        tenant: TenantId,
        entity: EntityRef,
    ) -> Result<Option<EntitySnapshot>, StoreError> {
        let row = sqlx::query(
            "SELECT version, payload, tombstone FROM aequora_entities WHERE tenant_id = $1 AND entity_type = $2 AND entity_id = $3",
        )
        .bind(tenant.as_uuid())
        .bind(i32::from(entity.entity_type.get()))
        .bind(entity.entity_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(postgres_error)?;
        row.map(|row| {
            Ok(EntitySnapshot {
                entity,
                tenant_id: tenant,
                current: CurrentEntity {
                    version: entity_version(
                        row.try_get::<i64, _>("version").map_err(postgres_error)?,
                    )?,
                    payload: row
                        .try_get::<Vec<u8>, _>("payload")
                        .map_err(postgres_error)?,
                    tombstone: row
                        .try_get::<bool, _>("tombstone")
                        .map_err(postgres_error)?,
                },
            })
        })
        .transpose()
    }

    async fn operation_result(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
    ) -> Result<Option<OperationAck>, StoreError> {
        let row = sqlx::query(
            "SELECT entity_version, server_sequence, event_id, correlation_id FROM aequora_applied_operations WHERE tenant_id = $1 AND operation_id = $2",
        )
        .bind(tenant.as_uuid())
        .bind(operation_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(postgres_error)?;
        row.map(|row| ack_from_row(operation_id, &row, false))
            .transpose()
    }

    async fn operation_lineage(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
    ) -> Result<Option<LineageContext>, StoreError> {
        let row = sqlx::query(
            "SELECT correlation_id, caused_by_kind, caused_by_id FROM aequora_applied_operations WHERE tenant_id = $1 AND operation_id = $2",
        )
        .bind(tenant.as_uuid())
        .bind(operation_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(postgres_error)?;
        row.map(|row| {
            Ok(LineageContext {
                correlation_id: CorrelationId::from_uuid(
                    row.try_get("correlation_id").map_err(postgres_error)?,
                ),
                caused_by: lineage_ref_from_row(&row)?,
            })
        })
        .transpose()
    }

    #[allow(clippy::too_many_lines)]
    async fn commit_operation(&self, commit: CommitOperation) -> Result<CommitOutcome, StoreError> {
        const MAX_TRANSACTION_ATTEMPTS: u32 = 3;
        if !commit.has_valid_version_transition() {
            return Err(corrupt(
                "authoritative entity version must advance by exactly one",
            ));
        }
        let mut attempt = 1_u32;
        loop {
            let result: Result<CommitOutcome, StoreError> = async {
                let mut transaction = self.pool.begin().await.map_err(postgres_error)?;
                if let Some(outcome) =
                    prepare_commit_in_transaction(&mut transaction, &commit).await?
                {
                    transaction.commit().await.map_err(postgres_error)?;
                    return Ok(outcome);
                }
                let authoritative_payload =
                    match self.commit_hook.apply(&mut transaction, &commit).await {
                        Ok(outcome) => outcome.into_authoritative_payload(),
                        Err(PostgresCommitHookError::Rejected(rejection)) => {
                            transaction.rollback().await.map_err(postgres_error)?;
                            return Ok(CommitOutcome::Rejected(rejection));
                        }
                        Err(PostgresCommitHookError::Store(error)) => return Err(error),
                    };
                let acknowledgement = persist_commit_in_transaction(
                    &mut transaction,
                    &commit,
                    &authoritative_payload,
                )
                .await?;
                transaction.commit().await.map_err(postgres_error)?;
                Ok(CommitOutcome::Applied(acknowledgement))
            }
            .await;
            match result {
                Err(error)
                    if attempt < MAX_TRANSACTION_ATTEMPTS && error.requires_transaction_retry() =>
                {
                    attempt = attempt.saturating_add(1);
                }
                result => return result,
            }
        }
    }

    async fn minimum_retained_cursor(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
    ) -> Result<Sequence, StoreError> {
        let value = sqlx::query_scalar::<_, i64>(
            "SELECT minimum_cursor FROM aequora_journal_floors WHERE tenant_id = $1 AND scope_id = $2",
        )
        .bind(tenant.as_uuid())
        .bind(scope.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(postgres_error)?
        .unwrap_or(0);
        Ok(Sequence(from_i64(value, "journal floor")?))
    }

    async fn read_changes_after(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        sequence: Sequence,
        limit: usize,
        max_payload_bytes: usize,
    ) -> Result<ChangePage, StoreError> {
        let query_limit = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
        let rows = sqlx::query(
            "SELECT sequence, operation_id, event_id, correlation_id, caused_by_kind, caused_by_id, entity_type, entity_id, entity_version, change_kind, payload, physical_ms, logical_clock, clock_node, MAX(sequence) OVER () AS journal_head FROM aequora_sync_events WHERE tenant_id = $1 AND scope_id = $2 AND sequence > $3 ORDER BY sequence LIMIT $4",
        )
        .bind(tenant.as_uuid())
        .bind(scope.as_uuid())
        .bind(to_i64(sequence.0, "journal cursor")?)
        .bind(query_limit)
        .fetch_all(&self.pool)
        .await
        .map_err(postgres_error)?;
        let journal_head = rows
            .first()
            .map(|row| {
                row.try_get::<i64, _>("journal_head")
                    .map_err(postgres_error)
                    .and_then(|value| from_i64(value, "journal head"))
                    .map(Sequence)
            })
            .transpose()?
            .unwrap_or(sequence)
            .max(sequence);
        let mut changes = Vec::with_capacity(rows.len().min(limit));
        let mut payload_bytes = 0_usize;
        let mut has_more = rows.len() > limit;
        for row in rows.into_iter().take(limit) {
            let payload = row
                .try_get::<Vec<u8>, _>("payload")
                .map_err(postgres_error)?;
            if payload_bytes.saturating_add(payload.len()) > max_payload_bytes {
                has_more = true;
                break;
            }
            payload_bytes = payload_bytes.saturating_add(payload.len());
            changes.push(remote_change_from_row(tenant, scope, &row, payload)?);
        }
        let next_sequence = changes.last().map_or(sequence, |change| change.sequence);
        Ok(ChangePage {
            changes,
            next_sequence,
            journal_head,
            has_more,
        })
    }

    async fn create_snapshot(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        partitions: &[Partition],
    ) -> Result<SnapshotDescriptor, StoreError> {
        self.create_snapshot_in_timeline(
            tenant,
            scope,
            partitions,
            AuthorityId::LEGACY_UNBOUND,
            AuthorityEpoch::INITIAL,
        )
        .await
    }

    async fn create_snapshot_in_timeline(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        partitions: &[Partition],
        authority_id: AuthorityId,
        authority_epoch: AuthorityEpoch,
    ) -> Result<SnapshotDescriptor, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(postgres_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *transaction)
            .await
            .map_err(postgres_error)?;
        self.snapshot_hook
            .materialize(&mut transaction, tenant, scope, partitions)
            .await?;
        let sequence = sqlx::query_scalar::<_, i64>(
            "SELECT sequence FROM aequora_scope_sequences WHERE tenant_id = $1 AND scope_id = $2",
        )
        .bind(tenant.as_uuid())
        .bind(scope.as_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(postgres_error)?
        .unwrap_or(0);
        let snapshot_id = SnapshotId::new();
        sqlx::query(
            "INSERT INTO aequora_snapshots (tenant_id, snapshot_id, scope_id, cursor_sequence, authority_id, authority_epoch) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(tenant.as_uuid())
        .bind(snapshot_id.as_uuid())
        .bind(scope.as_uuid())
        .bind(sequence)
        .bind(authority_id.as_uuid())
        .bind(to_i64(authority_epoch.get(), "snapshot authority epoch")?)
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;
        sqlx::query(
            "INSERT INTO aequora_snapshot_entities (tenant_id, snapshot_id, entity_order, entity_type, entity_id, entity_version, payload, tombstone) SELECT e.tenant_id, $2, ROW_NUMBER() OVER (ORDER BY e.entity_type, e.entity_id) - 1, e.entity_type, e.entity_id, e.version, e.payload, e.tombstone FROM aequora_entities e JOIN aequora_entity_scopes s ON s.tenant_id = e.tenant_id AND s.entity_type = e.entity_type AND s.entity_id = e.entity_id WHERE e.tenant_id = $1 AND s.scope_id = $3",
        )
        .bind(tenant.as_uuid())
        .bind(snapshot_id.as_uuid())
        .bind(scope.as_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;
        transaction.commit().await.map_err(postgres_error)?;
        Ok(SnapshotDescriptor {
            snapshot_id,
            cursor: Cursor::new(
                authority_id,
                authority_epoch,
                scope,
                Sequence(from_i64(sequence, "snapshot cursor")?),
            ),
        })
    }

    async fn read_snapshot(
        &self,
        tenant: TenantId,
        snapshot_id: SnapshotId,
        offset: u64,
        max_entities: usize,
        max_payload_bytes: usize,
    ) -> Result<SnapshotPage, StoreError> {
        let descriptor_row = sqlx::query(
            "SELECT scope_id, cursor_sequence, authority_id, authority_epoch FROM aequora_snapshots WHERE tenant_id = $1 AND snapshot_id = $2",
        )
        .bind(tenant.as_uuid())
        .bind(snapshot_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(postgres_error)?
        .ok_or_else(|| corrupt("PostgreSQL snapshot does not exist"))?;
        let scope = SyncScopeId::from_uuid(
            descriptor_row
                .try_get::<uuid::Uuid, _>("scope_id")
                .map_err(postgres_error)?,
        );
        let cursor_sequence = descriptor_row
            .try_get::<i64, _>("cursor_sequence")
            .map_err(postgres_error)?;
        let authority_id = descriptor_row
            .try_get::<Option<uuid::Uuid>, _>("authority_id")
            .map_err(postgres_error)?
            .map_or(AuthorityId::LEGACY_UNBOUND, AuthorityId::from_uuid);
        let authority_epoch = descriptor_row
            .try_get::<Option<i64>, _>("authority_epoch")
            .map_err(postgres_error)?
            .map(|value| {
                AuthorityEpoch::new(from_i64(value, "snapshot authority epoch")?)
                    .map_err(|error| corrupt(error.to_string()))
            })
            .transpose()?
            .unwrap_or(AuthorityEpoch::INITIAL);
        let rows = sqlx::query(
            "SELECT entity_type, entity_id, entity_version, payload, tombstone FROM aequora_snapshot_entities WHERE tenant_id = $1 AND snapshot_id = $2 AND entity_order >= $3 ORDER BY entity_order LIMIT $4",
        )
        .bind(tenant.as_uuid())
        .bind(snapshot_id.as_uuid())
        .bind(to_i64(offset, "snapshot offset")?)
        .bind(i64::try_from(max_entities.saturating_add(1)).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(postgres_error)?;
        let mut entities = Vec::with_capacity(rows.len().min(max_entities));
        let mut payload_bytes = 0_usize;
        let mut has_more = rows.len() > max_entities;
        for row in rows.into_iter().take(max_entities) {
            let payload = row
                .try_get::<Vec<u8>, _>("payload")
                .map_err(postgres_error)?;
            if payload_bytes.saturating_add(payload.len()) > max_payload_bytes {
                has_more = true;
                break;
            }
            payload_bytes = payload_bytes.saturating_add(payload.len());
            entities.push(snapshot_entity_from_row(&row, payload)?);
        }
        let next_offset = offset.saturating_add(u64::try_from(entities.len()).unwrap_or(u64::MAX));
        Ok(SnapshotPage {
            descriptor: SnapshotDescriptor {
                snapshot_id,
                cursor: Cursor::new(
                    authority_id,
                    authority_epoch,
                    scope,
                    Sequence(from_i64(cursor_sequence, "snapshot cursor")?),
                ),
            },
            entities,
            next_offset,
            has_more,
        })
    }

    async fn compact_journal(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        through: Sequence,
    ) -> Result<u64, StoreError> {
        let through = to_i64(through.0, "compaction cursor")?;
        let mut transaction = self.pool.begin().await.map_err(postgres_error)?;
        sqlx::query(
            "INSERT INTO aequora_journal_floors (tenant_id, scope_id, minimum_cursor) VALUES ($1, $2, $3) ON CONFLICT (tenant_id, scope_id) DO UPDATE SET minimum_cursor = GREATEST(aequora_journal_floors.minimum_cursor, EXCLUDED.minimum_cursor)",
        )
        .bind(tenant.as_uuid())
        .bind(scope.as_uuid())
        .bind(through)
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;
        let removed = sqlx::query(
            "DELETE FROM aequora_sync_events WHERE tenant_id = $1 AND scope_id = $2 AND sequence <= $3",
        )
        .bind(tenant.as_uuid())
        .bind(scope.as_uuid())
        .bind(through)
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?
        .rows_affected();
        transaction.commit().await.map_err(postgres_error)?;
        Ok(removed)
    }

    async fn read_audit_after(
        &self,
        tenant: TenantId,
        offset: AuditOffset,
        limit: usize,
    ) -> Result<AuditPage, StoreError> {
        let rows = sqlx::query(
            "SELECT audit_offset, operation_id, event_id, correlation_id, caused_by_kind, caused_by_id, actor_id, device_id, operation_kind, entity_type, entity_id, entity_version, command_digest, physical_ms, logical_clock, clock_node, authority_id, authority_epoch FROM aequora_audit_log WHERE tenant_id = $1 AND audit_offset > $2 ORDER BY audit_offset LIMIT $3",
        )
        .bind(tenant.as_uuid())
        .bind(to_i64(offset.0, "audit offset")?)
        .bind(i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(postgres_error)?;
        let has_more = rows.len() > limit;
        let mut records = Vec::with_capacity(rows.len().min(limit));
        for row in rows.into_iter().take(limit) {
            records.push(audit_from_row(tenant, &row)?);
        }
        let next_offset = records.last().map_or(offset, |record| record.offset);
        Ok(AuditPage {
            records,
            next_offset,
            has_more,
        })
    }

    async fn read_correlation(
        &self,
        tenant: TenantId,
        correlation_id: CorrelationId,
        offset: AuditOffset,
        limit: usize,
    ) -> Result<AuditPage, StoreError> {
        let rows = sqlx::query(
            "SELECT audit_offset, operation_id, event_id, correlation_id, caused_by_kind, caused_by_id, actor_id, device_id, operation_kind, entity_type, entity_id, entity_version, command_digest, physical_ms, logical_clock, clock_node, authority_id, authority_epoch FROM aequora_audit_log WHERE tenant_id = $1 AND correlation_id = $2 AND audit_offset > $3 ORDER BY audit_offset LIMIT $4",
        )
        .bind(tenant.as_uuid())
        .bind(correlation_id.as_uuid())
        .bind(to_i64(offset.0, "audit offset")?)
        .bind(i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(postgres_error)?;
        let has_more = rows.len() > limit;
        let mut records = Vec::with_capacity(rows.len().min(limit));
        for row in rows.into_iter().take(limit) {
            records.push(audit_from_row(tenant, &row)?);
        }
        let next_offset = records.last().map_or(offset, |record| record.offset);
        Ok(AuditPage {
            records,
            next_offset,
            has_more,
        })
    }

    async fn capture_authoritative_integrity(
        &self,
        tenant: TenantId,
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
        let mut transaction = self.pool.begin().await.map_err(postgres_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(postgres_error)?;
        let head = sqlx::query_scalar::<_, i64>(
            "SELECT sequence FROM aequora_scope_sequences WHERE tenant_id = $1 AND scope_id = $2",
        )
        .bind(tenant.as_uuid())
        .bind(scope.as_uuid())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(postgres_error)?
        .unwrap_or(0);
        if Sequence(from_i64(head, "integrity boundary")?) != boundary.sequence {
            return Err(StoreError::permanent(
                "integrity capture requires the current repeatable-read scope boundary",
            ));
        }
        let rows = sqlx::query(
            "SELECT e.entity_type, e.entity_id, e.version, e.payload, e.tombstone
               FROM aequora_entities e
               JOIN aequora_entity_scopes s
                 ON s.tenant_id = e.tenant_id
                AND s.entity_type = e.entity_type
                AND s.entity_id = e.entity_id
              WHERE e.tenant_id = $1 AND s.scope_id = $2
              ORDER BY e.entity_type, e.entity_id
              LIMIT $3",
        )
        .bind(tenant.as_uuid())
        .bind(scope.as_uuid())
        .bind(i64::try_from(max_entities.saturating_add(1)).unwrap_or(i64::MAX))
        .fetch_all(&mut *transaction)
        .await
        .map_err(postgres_error)?;
        if rows.len() > max_entities {
            return Err(StoreError::permanent(
                "integrity input exceeds the configured entity bound",
            ));
        }
        let mut entities = Vec::with_capacity(rows.len());
        for row in rows {
            entities.push(CanonicalEntity {
                entity: EntityRef {
                    entity_type: entity_type(
                        row.try_get::<i32, _>("entity_type")
                            .map_err(postgres_error)?,
                    )?,
                    entity_id: EntityId::from_uuid(
                        row.try_get::<uuid::Uuid, _>("entity_id")
                            .map_err(postgres_error)?,
                    ),
                },
                version: entity_version(row.try_get::<i64, _>("version").map_err(postgres_error)?)?,
                hash_schema: CURRENT_HASH_SCHEMA,
                payload: row
                    .try_get::<Vec<u8>, _>("payload")
                    .map_err(postgres_error)?,
                tombstone: row
                    .try_get::<bool, _>("tombstone")
                    .map_err(postgres_error)?,
            });
        }
        let snapshot =
            IntegritySnapshot::build(scope, boundary, generation, scheme, entities, max_entities)
                .map_err(|error| StoreError::permanent(error.to_string()))?;
        transaction.commit().await.map_err(postgres_error)?;
        Ok(snapshot)
    }
}

fn lineage_ref_columns(lineage: LineageContext) -> (Option<i16>, Option<uuid::Uuid>) {
    match lineage.caused_by {
        None => (None, None),
        Some(LineageRef::Operation(id)) => (Some(1), Some(id.as_uuid())),
        Some(LineageRef::Event(id)) => (Some(2), Some(id.as_uuid())),
        Some(LineageRef::Job(id)) => (Some(3), Some(id.as_uuid())),
    }
}

fn lineage_ref_from_row(row: &sqlx::postgres::PgRow) -> Result<Option<LineageRef>, StoreError> {
    let kind = row
        .try_get::<Option<i16>, _>("caused_by_kind")
        .map_err(postgres_error)?;
    let id = row
        .try_get::<Option<uuid::Uuid>, _>("caused_by_id")
        .map_err(postgres_error)?;
    match (kind, id) {
        (None, None) => Ok(None),
        (Some(1), Some(id)) => Ok(Some(LineageRef::Operation(OperationId::from_uuid(id)))),
        (Some(2), Some(id)) => Ok(Some(LineageRef::Event(EventId::from_uuid(id)))),
        (Some(3), Some(id)) => Ok(Some(LineageRef::Job(aequora_types::JobId::from_uuid(id)))),
        _ => Err(corrupt("invalid PostgreSQL lineage reference")),
    }
}

fn ack_from_row(
    operation_id: OperationId,
    row: &sqlx::postgres::PgRow,
    duplicate: bool,
) -> Result<OperationAck, StoreError> {
    Ok(OperationAck {
        operation_id,
        event_id: EventId::from_uuid(
            row.try_get::<uuid::Uuid, _>("event_id")
                .map_err(postgres_error)?,
        ),
        lineage: LineageContext {
            correlation_id: CorrelationId::from_uuid(
                row.try_get::<uuid::Uuid, _>("correlation_id")
                    .map_err(postgres_error)?,
            ),
            caused_by: Some(LineageRef::Operation(operation_id)),
        },
        entity_version: entity_version(
            row.try_get::<i64, _>("entity_version")
                .map_err(postgres_error)?,
        )?,
        sequence: Sequence(from_i64(
            row.try_get::<i64, _>("server_sequence")
                .map_err(postgres_error)?,
            "server sequence",
        )?),
        duplicate,
    })
}

fn remote_change_from_row(
    tenant: TenantId,
    scope: SyncScopeId,
    row: &sqlx::postgres::PgRow,
    payload: Vec<u8>,
) -> Result<RemoteChange, StoreError> {
    let logical = row
        .try_get::<i32, _>("logical_clock")
        .map_err(postgres_error)?;
    Ok(RemoteChange {
        tenant_id: tenant,
        scope_id: scope,
        sequence: Sequence(from_i64(
            row.try_get::<i64, _>("sequence").map_err(postgres_error)?,
            "journal sequence",
        )?),
        operation_id: OperationId::from_uuid(
            row.try_get::<uuid::Uuid, _>("operation_id")
                .map_err(postgres_error)?,
        ),
        event_id: EventId::from_uuid(
            row.try_get::<uuid::Uuid, _>("event_id")
                .map_err(postgres_error)?,
        ),
        lineage: LineageContext {
            correlation_id: CorrelationId::from_uuid(
                row.try_get::<uuid::Uuid, _>("correlation_id")
                    .map_err(postgres_error)?,
            ),
            caused_by: lineage_ref_from_row(row)?,
        },
        entity: EntityRef {
            entity_type: entity_type(
                row.try_get::<i32, _>("entity_type")
                    .map_err(postgres_error)?,
            )?,
            entity_id: EntityId::from_uuid(
                row.try_get::<uuid::Uuid, _>("entity_id")
                    .map_err(postgres_error)?,
            ),
        },
        version: entity_version(
            row.try_get::<i64, _>("entity_version")
                .map_err(postgres_error)?,
        )?,
        change_kind: change_kind(
            row.try_get::<i16, _>("change_kind")
                .map_err(postgres_error)?,
        )?,
        payload,
        timestamp: HybridTimestamp {
            physical_ms: row
                .try_get::<i64, _>("physical_ms")
                .map_err(postgres_error)?,
            logical: u32::try_from(logical)
                .map_err(|_| corrupt("negative PostgreSQL logical clock"))?,
            node: node_id(row, "clock_node")?,
        },
    })
}

fn snapshot_entity_from_row(
    row: &sqlx::postgres::PgRow,
    payload: Vec<u8>,
) -> Result<SnapshotEntity, StoreError> {
    Ok(SnapshotEntity {
        entity: EntityRef {
            entity_type: entity_type(
                row.try_get::<i32, _>("entity_type")
                    .map_err(postgres_error)?,
            )?,
            entity_id: EntityId::from_uuid(
                row.try_get::<uuid::Uuid, _>("entity_id")
                    .map_err(postgres_error)?,
            ),
        },
        version: entity_version(
            row.try_get::<i64, _>("entity_version")
                .map_err(postgres_error)?,
        )?,
        payload,
        tombstone: row
            .try_get::<bool, _>("tombstone")
            .map_err(postgres_error)?,
    })
}

fn audit_from_row(
    tenant: TenantId,
    row: &sqlx::postgres::PgRow,
) -> Result<aequora_store::AuditRecord, StoreError> {
    let digest = row
        .try_get::<Vec<u8>, _>("command_digest")
        .map_err(postgres_error)?;
    let command_digest: [u8; 32] = digest
        .try_into()
        .map_err(|_| corrupt("invalid PostgreSQL audit digest length"))?;
    let operation_kind = row
        .try_get::<i32, _>("operation_kind")
        .map_err(postgres_error)?;
    let logical = row
        .try_get::<i32, _>("logical_clock")
        .map_err(postgres_error)?;
    let authority_id = row
        .try_get::<Option<uuid::Uuid>, _>("authority_id")
        .map_err(postgres_error)?;
    let authority_epoch = row
        .try_get::<Option<i64>, _>("authority_epoch")
        .map_err(postgres_error)?;
    let authority = match (authority_id, authority_epoch) {
        (Some(authority_id), Some(authority_epoch)) => Some(aequora_types::AuthorityTimeline {
            authority_id: AuthorityId::from_uuid(authority_id),
            epoch: AuthorityEpoch::new(from_i64(authority_epoch, "audit authority epoch")?)
                .map_err(|error| corrupt(error.to_string()))?,
        }),
        (None, None) => None,
        _ => return Err(corrupt("partial PostgreSQL audit authority binding")),
    };
    Ok(aequora_store::AuditRecord {
        authority,
        offset: AuditOffset(from_i64(
            row.try_get::<i64, _>("audit_offset")
                .map_err(postgres_error)?,
            "audit offset",
        )?),
        tenant_id: tenant,
        operation_id: OperationId::from_uuid(
            row.try_get::<uuid::Uuid, _>("operation_id")
                .map_err(postgres_error)?,
        ),
        event_id: EventId::from_uuid(
            row.try_get::<uuid::Uuid, _>("event_id")
                .map_err(postgres_error)?,
        ),
        operation_lineage: LineageContext {
            correlation_id: CorrelationId::from_uuid(
                row.try_get::<uuid::Uuid, _>("correlation_id")
                    .map_err(postgres_error)?,
            ),
            caused_by: lineage_ref_from_row(row)?,
        },
        actor_id: ActorId::from_uuid(
            row.try_get::<uuid::Uuid, _>("actor_id")
                .map_err(postgres_error)?,
        ),
        device_id: DeviceId::from_uuid(
            row.try_get::<uuid::Uuid, _>("device_id")
                .map_err(postgres_error)?,
        ),
        operation_kind: u16::try_from(operation_kind)
            .map_err(|_| corrupt("invalid PostgreSQL operation kind"))?,
        entity: EntityRef {
            entity_type: entity_type(
                row.try_get::<i32, _>("entity_type")
                    .map_err(postgres_error)?,
            )?,
            entity_id: EntityId::from_uuid(
                row.try_get::<uuid::Uuid, _>("entity_id")
                    .map_err(postgres_error)?,
            ),
        },
        entity_version: entity_version(
            row.try_get::<i64, _>("entity_version")
                .map_err(postgres_error)?,
        )?,
        command_digest,
        timestamp: HybridTimestamp {
            physical_ms: row
                .try_get::<i64, _>("physical_ms")
                .map_err(postgres_error)?,
            logical: u32::try_from(logical)
                .map_err(|_| corrupt("negative PostgreSQL logical clock"))?,
            node: node_id(row, "clock_node")?,
        },
    })
}

/// Aequora authoritative-store adapter over an application-owned `PostgreSQL` backend.
pub struct PostgresStore<B> {
    backend: B,
}

impl<B> PostgresStore<B> {
    /// Wraps a `PostgreSQL` backend without leaking its connection types.
    #[must_use]
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }
    /// Borrows the backend for migration or pool lifecycle operations.
    #[must_use]
    pub const fn backend(&self) -> &B {
        &self.backend
    }
}

impl<B: TransactionCapabilityProvider> TransactionCapabilityProvider for PostgresStore<B> {
    fn transaction_capabilities(&self) -> TransactionCapabilities {
        self.backend.transaction_capabilities()
    }
}

impl<B: IntegrityCapabilityProvider> IntegrityCapabilityProvider for PostgresStore<B> {
    fn integrity_support(&self) -> IntegritySupport {
        self.backend.integrity_support()
    }
}

impl<B: AdapterManifestProvider> AdapterManifestProvider for PostgresStore<B> {
    fn adapter_manifest(&self) -> AdapterManifest {
        self.backend.adapter_manifest()
    }
}

#[async_trait]
impl<B: PostgresBackend> EntityReader for PostgresStore<B> {
    async fn read_entity(
        &self,
        tenant: TenantId,
        entity: EntityRef,
    ) -> Result<Option<EntitySnapshot>, StoreError> {
        self.backend.read_entity(tenant, entity).await
    }
}

#[async_trait]
impl<B: PostgresBackend> OperationLedger for PostgresStore<B> {
    async fn operation_result(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
    ) -> Result<Option<aequora_protocol::OperationAck>, StoreError> {
        self.backend.operation_result(tenant, operation_id).await
    }
    async fn operation_lineage(
        &self,
        tenant: TenantId,
        operation_id: OperationId,
    ) -> Result<Option<LineageContext>, StoreError> {
        self.backend.operation_lineage(tenant, operation_id).await
    }
    async fn commit_operation(&self, commit: CommitOperation) -> Result<CommitOutcome, StoreError> {
        self.backend.commit_operation(commit).await
    }
}

#[async_trait]
impl<B: PostgresBackend> ChangeJournal for PostgresStore<B> {
    async fn minimum_retained_cursor(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
    ) -> Result<Sequence, StoreError> {
        self.backend.minimum_retained_cursor(tenant, scope).await
    }

    async fn read_changes_after(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        sequence: Sequence,
        limit: usize,
        max_payload_bytes: usize,
    ) -> Result<ChangePage, StoreError> {
        self.backend
            .read_changes_after(tenant, scope, sequence, limit, max_payload_bytes)
            .await
    }
}

#[async_trait]
impl<B: PostgresBackend> SnapshotStore for PostgresStore<B> {
    async fn create_snapshot(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        partitions: &[Partition],
    ) -> Result<SnapshotDescriptor, StoreError> {
        self.backend
            .create_snapshot(tenant, scope, partitions)
            .await
    }

    async fn create_snapshot_in_timeline(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        partitions: &[Partition],
        authority_id: AuthorityId,
        authority_epoch: AuthorityEpoch,
    ) -> Result<SnapshotDescriptor, StoreError> {
        self.backend
            .create_snapshot_in_timeline(tenant, scope, partitions, authority_id, authority_epoch)
            .await
    }

    async fn read_snapshot(
        &self,
        tenant: TenantId,
        snapshot_id: SnapshotId,
        offset: u64,
        max_entities: usize,
        max_payload_bytes: usize,
    ) -> Result<SnapshotPage, StoreError> {
        self.backend
            .read_snapshot(tenant, snapshot_id, offset, max_entities, max_payload_bytes)
            .await
    }
}

#[async_trait]
impl<B: PostgresBackend> JournalCompactor for PostgresStore<B> {
    async fn compact_journal(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        through: Sequence,
    ) -> Result<u64, StoreError> {
        self.backend.compact_journal(tenant, scope, through).await
    }
}

#[async_trait]
impl<B: PostgresBackend> AuditLog for PostgresStore<B> {
    async fn read_audit_after(
        &self,
        tenant: TenantId,
        offset: AuditOffset,
        limit: usize,
    ) -> Result<AuditPage, StoreError> {
        self.backend.read_audit_after(tenant, offset, limit).await
    }
}

#[async_trait]
impl<B: PostgresBackend> CorrelationLog for PostgresStore<B> {
    async fn read_correlation(
        &self,
        tenant: TenantId,
        correlation_id: CorrelationId,
        offset: AuditOffset,
        limit: usize,
    ) -> Result<AuditPage, StoreError> {
        self.backend
            .read_correlation(tenant, correlation_id, offset, limit)
            .await
    }
}

#[async_trait]
impl<B: PostgresBackend> AuthoritativeIntegritySource for PostgresStore<B> {
    async fn capture_authoritative_integrity(
        &self,
        tenant: TenantId,
        scope: SyncScopeId,
        boundary: Cursor,
        generation: IntegrityGeneration,
        scheme: PartitionScheme,
        max_entities: usize,
    ) -> Result<IntegritySnapshot, StoreError> {
        self.backend
            .capture_authoritative_integrity(
                tenant,
                scope,
                boundary,
                generation,
                scheme,
                max_entities,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        POSTGRES_MIGRATIONS, POSTGRES_STORAGE_ADAPTER_MANIFEST, PgSslMode,
        PostgresNotifyHintBroker, PostgresPoolConfig, adapter_sdk, migration_checksum,
        parse_connect_options, postgres_transaction_retry_reason, verify_migration_record,
    };
    use aequora_live::{SyncHint, SyncHintReason};
    use aequora_store::{StoreErrorKind, StoreErrorReason};
    use aequora_types::{Sequence, SyncScopeId, TenantId};

    #[test]
    fn part_36_manifest_is_structurally_valid_and_explicit() {
        POSTGRES_STORAGE_ADAPTER_MANIFEST
            .validate()
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            POSTGRES_STORAGE_ADAPTER_MANIFEST
                .supports_role(adapter_sdk::AdapterRole::AuthoritativeStore)
        );
        assert!(
            !POSTGRES_STORAGE_ADAPTER_MANIFEST
                .known_limitations
                .is_empty()
        );
    }

    #[test]
    fn notify_hint_codec_is_small_and_exact() {
        let hint = SyncHint::v1(
            TenantId::new(),
            SyncScopeId::new(),
            Some(Sequence(42)),
            SyncHintReason::NewAuthoritativeChange,
        );
        let encoded = PostgresNotifyHintBroker::encode(hint);
        assert!(encoded.is_ok());
        let encoded = encoded.unwrap_or_default();
        assert!(encoded.len() < 8_000);
        assert_eq!(PostgresNotifyHintBroker::decode(&encoded), Ok(hint));
    }

    #[test]
    fn neon_urls_force_hostname_verified_tls_and_scale_to_zero_pooling() {
        let options = parse_connect_options(
            "postgresql://user:secret@ep-example-pooler.us-east-2.aws.neon.tech/neondb?sslmode=require&channel_binding=require",
            true,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            options.get_host(),
            "ep-example-pooler.us-east-2.aws.neon.tech"
        );
        assert!(matches!(options.get_ssl_mode(), PgSslMode::VerifyFull));

        let config = PostgresPoolConfig::neon(5);
        assert_eq!(config.max_connections, 5);
        assert_eq!(config.min_connections, 0);
    }

    #[test]
    fn migration_records_reject_name_and_checksum_drift_permanently() {
        let migration = POSTGRES_MIGRATIONS[0];
        assert!(
            verify_migration_record(
                migration,
                migration.name,
                &migration_checksum(migration.sql),
            )
            .is_ok()
        );

        let Err(name_error) =
            verify_migration_record(migration, "renamed", &migration_checksum(migration.sql))
        else {
            panic!("renamed migration was accepted");
        };
        assert_eq!(name_error.kind, StoreErrorKind::Permanent);

        let Err(checksum_error) = verify_migration_record(migration, migration.name, &[0; 32])
        else {
            panic!("modified migration was accepted");
        };
        assert_eq!(checksum_error.kind, StoreErrorKind::Permanent);
    }

    #[test]
    fn only_serialization_and_deadlock_sqlstates_request_whole_transaction_retry() {
        assert_eq!(
            postgres_transaction_retry_reason("40001"),
            Some(StoreErrorReason::SerializationFailure)
        );
        assert_eq!(
            postgres_transaction_retry_reason("40P01"),
            Some(StoreErrorReason::Deadlock)
        );
        assert_eq!(postgres_transaction_retry_reason("23505"), None);
    }
}
