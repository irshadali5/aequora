//! Convenient re-exports for the Aequora synchronization workspace.

pub use aequora_blob as blob;
pub use aequora_client as client;
pub use aequora_clock as clock;
pub use aequora_codec as codec;
pub use aequora_compute as compute;
pub use aequora_config as config;
pub use aequora_conflict as conflict;
pub use aequora_crdt as crdt;
pub use aequora_executor as executor;
pub use aequora_journal as journal;
pub use aequora_observability as observability;
pub use aequora_partition as partition;
pub use aequora_protocol as protocol;
pub use aequora_routing as routing;
pub use aequora_server as server;
pub use aequora_store as store;
pub use aequora_transport as transport;
pub use aequora_types as types;
pub use aequora_validator as validator;

#[cfg(feature = "axum")]
pub use aequora_axum as axum;
#[cfg(feature = "http-client")]
pub use aequora_http as http_client;
#[cfg(feature = "quic")]
pub use aequora_quic as quic;
#[cfg(feature = "postgres")]
pub use aequora_store_postgres::{
    self as postgres, POSTGRES_SCHEMA_VERSION, PostgresPoolConfig, PostgresSchemaStatus,
    PostgresStore, SqlxPostgresBackend,
};
#[cfg(feature = "stoolap")]
pub use aequora_store_stoolap::{
    self as stoolap, STOOLAP_SCHEMA_VERSION, StoolapDatabase, StoolapSchemaStatus, StoolapStore,
};
#[cfg(feature = "testkit")]
pub use aequora_testkit as testkit;

/// Common imports needed to build client and server integrations.
pub mod prelude {
    pub use aequora_blob::{BlobDigest, BlobManifest, BlobRef, BlobStore, InMemoryBlobStore};
    pub use aequora_client::{
        AdaptiveBatchConfig, AdaptiveBatcher, BootstrapOutcome, ClientBuildError, ClientConfig,
        ClientSyncEngine, ClientSyncEngineBuilder, CoordinatorClosed, RetryConfig, SyncCoordinator,
        SyncCoordinatorConfig, SyncCoordinatorHandle, SyncHealth, SyncOutcome, SyncStatus,
        SyncSummary, SyncTrigger,
    };
    pub use aequora_clock::{Clock, SystemClock};
    pub use aequora_config::{
        AequoraConfig, CompressionAlgorithm, CompressionConfig, ComputePoolConfig, ConfigError,
        CoordinatorConfig, ProtocolConfig, PullConfig, PushConfig, ResourceLimitsConfig,
        RetryPolicyConfig,
    };
    pub use aequora_conflict::{
        ConflictPolicyRegistry, ConflictResolver, FieldSet, FieldSetMerger, FieldValue,
        FinancialOperation, FinancialPolicyError, MergeDecision, MergeError, MergeInput,
        MergeStrategy, RejectConflicts, TypedOperation,
    };
    pub use aequora_crdt::{Crdt, GCounter, PnCounter, PostcardCrdtMerger};
    pub use aequora_executor::{
        AuthContext, AuthenticatedOperation, AuthoritativeMutation, AuthorizedOperation,
        CurrentEntity, DomainOperation, ExecutableOperation, ExecutionError, IncomingOperation,
        OperationExecutor, OperationHandler, OperationRegistry, PayloadMigrator, RegistrationError,
        ScopeAuthorizer, ValidatedOperation,
    };
    pub use aequora_journal::{CursorWatermarks, TombstoneRetention, tombstone_collectable};
    pub use aequora_observability::{
        AtomicMetrics, MetricEvent, MetricsSnapshot, NoopObserver, Observer, OutcomeKind,
        ServerPhaseKind, TraceContext,
    };
    pub use aequora_partition::{
        PartitionExpression, PartitionHierarchy, PartitionPolicy, PartitionPolicyError,
    };
    pub use aequora_protocol::{
        BootstrapRequest, BootstrapResponse, Capability, ChangeKind, ClientLimits,
        OperationEnvelope, OperationKind, OperationMetadata, Partition, PushHint, PushHintReason,
        ResyncReason, SessionMetadata, SnapshotEntity, SnapshotLimits, SyncDirective, SyncRequest,
        SyncResponse,
    };
    pub use aequora_routing::{
        NoEligibleRegion, RegionHealth, RegionRole, RegionRouter, RegionState, RouteDecision,
        RouteReason, RoutingIntent,
    };
    pub use aequora_server::{
        ExchangeService, ServerCommandOutcome, ServerConfig, SyncServer, SyncServerBuilder,
    };
    pub use aequora_store::{
        AuditLog, AuditOffset, AuditPage, AuditRecord, AuthoritativeStore, ConflictInbox,
        ConflictRecord, ConflictResolution, LocalStore, OutboxState, OutboxStateStore, OutboxStats,
        OutboxStore,
    };
    pub use aequora_transport::{SnapshotPageStream, StreamingSyncTransport, SyncTransport};
    pub use aequora_types::{
        ActorId, Cursor, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, HybridTimestamp,
        NodeId, OperationId, ProtocolVersion, RegionId, RequestId, SchemaVersion, Sequence,
        SessionId, SnapshotId, SyncScopeId, TenantId,
    };
}

#[cfg(feature = "tracing")]
pub use aequora_observability::TracingObserver;

#[cfg(feature = "http-client")]
pub use aequora_http::{
    HttpTransport, HttpTransportConfig, HttpTransportConfigError, NoRequestHeaders, RequestHeaders,
    StaticRequestHeaders,
};

#[cfg(feature = "quic")]
pub use aequora_quic::{QuicConfig, QuicServer, QuicServerError, QuicTransport};
