//! Convenient re-exports for the Aequora synchronization workspace.

extern crate self as aequora;

pub use aequora_macros::{AequoraAggregate, AequoraOperation};

pub use aequora_admission as admission;
pub use aequora_audit as audit;
pub use aequora_authority as authority;
pub use aequora_blob as blob;
pub use aequora_bootstrap as bootstrap;
pub use aequora_client as client;
pub use aequora_client::AequoraClient;
pub use aequora_clock as clock;
pub use aequora_codec as codec;
pub use aequora_compute as compute;
pub use aequora_config as config;
pub use aequora_conflict as conflict;
pub use aequora_coordination as coordination;
pub use aequora_crdt as crdt;
pub use aequora_crypto as crypto;
pub use aequora_executor as executor;
pub use aequora_governance as governance;
pub use aequora_integrity as integrity;
pub use aequora_journal as journal;
pub use aequora_live as live;
#[cfg(feature = "record-sync")]
pub use aequora_mapping as mapping;
#[cfg(feature = "record-sync")]
pub use aequora_migration as migration;
pub use aequora_observability as observability;
pub use aequora_partition as partition;
pub use aequora_performance as performance;
pub use aequora_profile as profile;
pub use aequora_protocol as protocol;
pub use aequora_queue as queue;
pub use aequora_region as region;
pub use aequora_replay as replay;
pub use aequora_routing as routing;
pub use aequora_scheduler as scheduler;
#[cfg(feature = "record-sync")]
pub use aequora_schema as schema;
pub use aequora_scope as scope;
pub use aequora_server as server;
pub use aequora_server::AequoraServer;
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
    self as postgres, POSTGRES_ADAPTER_MANIFEST, POSTGRES_SCHEMA_VERSION, PostgresCommitHook,
    PostgresCommitHookError, PostgresCommitHookOutcome, PostgresNotifyHintBroker,
    PostgresPoolConfig, PostgresSchemaStatus, PostgresStore, SqlxPostgresBackend,
};
#[cfg(feature = "stoolap")]
pub use aequora_store_stoolap::{
    self as stoolap, STOOLAP_ADAPTER_MANIFEST, STOOLAP_SCHEMA_VERSION, StoolapDatabase,
    StoolapProjectionHook, StoolapSchemaStatus, StoolapStore,
};
#[cfg(feature = "testkit")]
pub use aequora_testkit as testkit;

/// Common imports needed to build client and server integrations.
pub mod prelude {
    pub use aequora_admission::{
        AdmissionController, AdmissionMetricsSnapshot, AdmissionPermit, AdmissionPolicy,
        AdmissionRejection, BrownoutPolicy, ClassBudget, CostUnits, FairQueue, FairQueueConfig,
        HierarchicalAdmission, LoadPolicy, LoadSignals, LoadState, LoadStateTracker,
        LoadThresholds, PolicyError as AdmissionPolicyError, QueueRejection, QueuedWork, RateLimit,
        RequestLimits, RequestShape, ResourceBudget, ResourceDomain, ServerPriorityPolicy,
        TenantAdmissionPolicy, TokenBucket, WorkDescriptor as AdmissionWorkDescriptor,
    };
    pub use aequora_audit::{
        AUDIT_FORMAT_VERSION, AuditAccess, AuditActionId, AuditActor, AuditAnchorSink,
        AuditArchiveSink, AuditCategory, AuditChange, AuditChangeKind, AuditCheckpoint,
        AuditDurability, AuditError, AuditEvent, AuditEventId, AuditExportId, AuditExportManifest,
        AuditFieldId, AuditFieldPolicy, AuditOrigin, AuditOutcome, AuditPageCursor, AuditPartition,
        AuditPolicy as BusinessAuditPolicy, AuditProvenance, AuditQuery, AuditReason,
        AuditRetentionClass, AuditRetentionDecision, AuditSequence, AuditSubject, AuditTimestamp,
        AuditValue, AuditValuePolicy, ChainedAuditRecord, ExplanationLevel, FieldProvenance,
        ReasonCode, verify_chain,
    };
    pub use aequora_authority::{
        AuthorityArtifactBinding, AuthorityArtifactKind, AuthorityCommitContext,
        AuthorityController, AuthorityDescriptor, AuthorityError, AuthorityFenceToken,
        AuthorityPermission, AuthorityPromotionPlan, AuthorityPromotionPolicy, AuthorityRole,
        AuthorityRuntimeMode, AuthorityState, AuthorityTransitionManifest, BackupAuthorityMetadata,
        CheckpointComparison, ClientAuthorityState, CursorDisposition, EpochOperationAction,
        EpochOperationResolution, EpochOperationState, EpochRecoveryPolicy, EpochTransitionPhase,
        ExternalEpochRegistry, InMemoryEpochRegistry, JournalCheckpoint, OperationRecoveryRecord,
        PromotionClass, PromotionEvidence, PromotionOutcome, PromotionReadiness, PromotionRequest,
        RecoveryVerification, TransitionReason, classify_epoch_operation, compare_checkpoints,
        validate_cursor,
    };
    #[cfg(feature = "axum")]
    pub use aequora_axum::{
        AxumConfig, DrainOutcome, ReadinessFn, ReadinessProbe, ServerLifecycle,
        router_with_lifecycle, router_with_readiness,
    };
    pub use aequora_blob::{
        BlobDigest, BlobDownloadSink, BlobManifest, BlobRef, BlobStore, BlobUploadSource,
        InMemoryBlobStore, StreamingBlobStore, download_stream as download_blob_stream,
        upload_stream as upload_blob_stream,
    };
    pub use aequora_bootstrap::{
        ActivationEvidence, ActivationOutcome, AuthorityEpoch, BootstrapError, BootstrapJob,
        BootstrapJobId, BootstrapMutationPolicy, BootstrapPreflight, BootstrapState,
        BootstrapStatus, BootstrapStoreError, BootstrapStoreErrorKind, BuiltSnapshot,
        ChunkDescriptor, ChunkId, ChunkLocation, ChunkProgress, ChunkRead, ChunkState,
        ChunkingConfig, CompressionKind as SnapshotCompressionKind, EntityRange, PendingIntentPlan,
        ProjectionSchemaVersion, RangeResumeGuard, ReplicaGeneration, SnapshotBoundary,
        SnapshotChunk, SnapshotChunkSource, SnapshotInstallCapability, SnapshotInstallFeature,
        SnapshotLease, SnapshotLeaseStore, SnapshotManifest, SnapshotReadView, SnapshotRequest,
        SnapshotSink, SnapshotSource, VerifiedActivation, verify_activation,
    };
    pub use aequora_client::{
        AdaptiveBatchConfig, AdaptiveBatcher, AequoraClient, BootstrapOutcome, ClientBuildError,
        ClientConfig, ClientSyncEngine, ClientSyncEngineBuilder, CoordinatorClosed,
        CoordinatorStatus, MultiProcessCoordinatorConfig, RetryConfig, SyncCoordinator,
        SyncCoordinatorConfig, SyncCoordinatorHandle, SyncHealth, SyncOutcome, SyncStatus,
        SyncSummary, SyncTrigger,
    };
    pub use aequora_clock::{Clock, SystemClock};
    pub use aequora_config::{
        AdaptivePushConfig, AequoraConfig, CompressionAlgorithm, CompressionConfig,
        ComputePoolConfig, ConfigError, CoordinatorConfig, IntegrityConfig, OperationalConfig,
        OutboxOptimizationConfig, ProtocolConfig, PullConfig, PushConfig, ResourceLimitsConfig,
        RetryPolicyConfig,
    };
    pub use aequora_conflict::{
        ConflictPolicyRegistry, ConflictResolver, FieldSet, FieldSetMerger, FieldValue,
        FinancialOperation, FinancialPolicyError, LastWriterWinsMerger, MergeDecision, MergeError,
        MergeInput, MergeStrategy, RejectConflicts, TimestampedValue, TypedOperation,
    };
    pub use aequora_coordination::{
        CoordinationSnapshot, FencingToken, LeaseGrant, LeaseKind, LeaseRequest,
        LocalCoordinationSupport, LocalProcessMode, LocalStoreGeneration, LocalStoreId,
        ProcessInstanceId,
    };
    pub use aequora_crdt::{Crdt, GCounter, PnCounter, PostcardCrdtMerger};
    pub use aequora_crypto::{
        AAD_VERSION_V1, AequoraCrypto, AequoraCryptoBuilder, ArtifactContent, ArtifactDigest,
        ArtifactSigningContext, ArtifactType, AssociatedData, AuditDigest,
        BlobDigest as CryptoBlobDigest, CanonicalDigest, CheckpointScope, CiphertextDigest,
        CryptoBuildError, CryptoKeyStore, CryptoPolicy, CryptoPolicyVersion, CryptoProfile,
        CryptoRequirement, CryptoTimestamp, DataEncryptionKey, DeviceKeyRecord, DeviceKeyRegistry,
        DigestAlgorithm, E2eKeyEpoch, E2eScheme, EncryptedPayload, EncryptionAlgorithm,
        EncryptionError, EncryptionKeyProvider, InMemoryEncryptionKeyProvider,
        InMemorySigningKeyProvider, KeyDestructionDecision, KeyDestructionEvidence,
        KeyDestructionReceipt, KeyDestructionRequest, KeyIdentity, KeyLifecycleError,
        KeyLifecycleEvent, KeyLifecycleEventKind, KeyProvider, KeyPurpose, KeyRecord, KeyReference,
        KeyReferenceIndex, KeyRegistryGeneration, KeyRegistryManifest, KeyStatus, OperationDigest,
        PayloadProtectionMode, PlaintextDigest, ProtectedDomainPolicy, ProtectedPayload,
        ProtectedPayloadError, PublicKeyBytes, RegistryAcceptance, RegistryDigest, RegistryError,
        RootKeyId, ServerPlaintextResponsibility, ServerPlaintextUse, SignatureAlgorithm,
        SignatureBytes, SignatureEnvelope, SignedArtifactManifest, SignedCheckpoint,
        SignedOperation, SigningError, SigningKeyId, SigningSecret, SnapshotDigest, TenantKeyId,
        TrustContext, TrustedKeyRegistry, UnsignedArtifactManifest, VerificationError,
        canonical_bytes as canonical_crypto_bytes, decrypt_payload, derive_export_key,
        domain_digest, encrypt_payload, sign_artifact, sign_checkpoint, sign_digest,
        sign_key_registry, sign_operation, verify_artifact, verify_checkpoint, verify_digest,
        verify_operation_signature,
    };
    pub use aequora_executor::{
        AuthContext, AuthenticatedOperation, AuthoritativeMutation, AuthorizedOperation,
        CurrentEntity, DerivedEventProvenance, DomainOperation, ExecutableOperation,
        ExecutionError, IncomingOperation, JobProvenance, OperationExecutor, OperationHandler,
        OperationRegistry, PayloadMigrator, RegistrationError, ScopeAuthorizer, TrustedProvenance,
        ValidatedOperation,
    };
    pub use aequora_governance::{
        ApprovalState, DataSubjectGraph, DataSubjectRef, DataSubjectResolver, DeletionMode,
        DeviceLifecycle, DeviceWatermark, DurationPolicy, ErasureAction, ErasureActionKind,
        ErasureBlocker, ErasureLedgerEntry, ErasurePlan, ErasureRequestId, FieldErasureMode,
        FieldGovernancePolicy, GovernanceCapabilities, GovernanceCapability, GovernanceError,
        GovernanceJobState, GovernancePolicyGeneration, GovernanceRegistry, GovernanceRelation,
        GovernanceStore, GovernedCopyKind, GovernedCopyRef, GovernedDataClass, GovernedObjectRef,
        HoldSelector, HoldState, JournalFloor, LegalHold, LegalHoldId, LifecycleState,
        LocalPurgeState, OperationLedgerPolicy, PurgeAck, PurgeDirective, PurgeDirectiveId,
        PurgeId, PurgePlan, PurgeReason, PurgeVerificationReport, RestoreGovernanceGate,
        RetentionClassId, RetentionPolicy, RetentionPolicyVersion, StorageSurface,
        StorageSurfaceId, StoreVerification, SubjectGraphNode, SurfaceOutcome, TenantLifecycle,
        TombstoneGcCandidate, TombstoneGcDecision, evaluate_tombstone_gc,
    };
    pub use aequora_integrity::{
        CURRENT_HASH_SCHEMA, CURRENT_INTEGRITY_GENERATION, CanonicalEntity, DivergenceKind,
        EntityDigest, EntityDivergence, HashSchemaVersion, IntegrityComparison, IntegrityDigest,
        IntegrityError, IntegrityGeneration, IntegrityManifest, IntegrityMetadata,
        IntegrityPartitionId, IntegrityRootRequest, IntegrityRootResponse, IntegritySnapshot,
        IntegritySupport, MerkleProof, PartitionDigest, PartitionScheme, PendingIntent,
        RepairApplication, RepairPlan, RepairPolicy, RepairStrategy, VerificationEvent,
        VerificationState, apply_authoritative_repair, classify_divergence, plan_repair,
    };
    pub use aequora_journal::{CursorWatermarks, TombstoneRetention, tombstone_collectable};
    pub use aequora_live::{
        FanoutOutcome, HintBroker, HintSubscription, HintTopic, HintWakeOutcome, HintWakeTracker,
        InMemoryHintBroker, LIVE_PROTOCOL_V1, LatestHintQueue, LiveConnection, LiveConnectionId,
        LiveError, LiveFailureKind, LiveHintStream, LiveHintTransport, LiveLeadership, LiveLimits,
        LiveMessage, LiveRouter, LiveScopeAuthorizer, LiveSession, PostCommitHintPublisher,
        PresenceDirectory, PresenceRecord, PresenceState, PublicationOutcome, QueueOutcome,
        ReconnectPolicy, SyncHint, SyncHintReason, publish_best_effort,
    };
    pub use aequora_macros::{AequoraAggregate, AequoraOperation};
    #[cfg(feature = "record-sync")]
    pub use aequora_migration::{
        AuthorityImportKind, AuthorityImportSink, BaselinePlan, BatchCommitOutcome,
        CanonicalExport, CanonicalRecordPage, CanonicalRecordSink, CanonicalRecordSource,
        CanonicalTransformer, CheckpointedImportSink, CutoverBlocker, CutoverEvidence,
        CutoverStrategy, DuplicatePolicy, EvidenceCheck, ExportBundleManifest, ExportLimits,
        ExportMode, IdMappingStrategy, IdentityPlan, ImportBatch, ImportBatchLimits,
        ImportCheckpoint, ImportJob, ImportJobId, ImportJobState, ImportPlan, ImportRecordError,
        ImportRecordValidator, ImportStrictness, ImportWorkflowError, ImportWorkflowStoreError,
        LegacyChangePage, LegacyChangeSource, MappedRecord, MigrationMode, MigrationWatermark,
        PreparedImportRecord, QuarantineEntry, QuarantineStatus, SeedVersionPolicy,
        SourceFingerprint, SourceRecordKey, SourceRecordMapper, SourceSystemId, VerifiedExport,
        export_source, import_verified, plan_entity_order, verify_cutover,
    };
    pub use aequora_observability::{
        AtomicMetrics, AuditEventKind, CryptoEventKind, GovernanceEventKind, ImportEventKind,
        IntegrityOutcomeKind, LargeBootstrapEventKind, LiveEventKind, LocalCoordinationEventKind,
        MetricEvent, MetricsSnapshot, NoopObserver, Observer, OutcomeKind, ProfileEventKind,
        ReplayEventKind, SchedulerEventKind, ScopeEventKind, ServerPhaseKind, TraceContext,
    };
    pub use aequora_partition::{
        PartitionExpression, PartitionHierarchy, PartitionPolicy, PartitionPolicyError,
    };
    pub use aequora_performance::{
        BenchmarkEnvironment, BenchmarkKind, BoundedQueue, CpuBudget, HOT_QUERY_CONTRACTS,
        HotQuery, HotQueryContract, ImmutableRegistry, InvalidationCoalescer, MeasuredPhase,
        MemoryBudget, MemoryPressure, PERFORMANCE_INVARIANTS, PagedView, PerformanceError,
        PerformancePolicy, PerformanceProfile, PerformanceReport, PhaseMeasurement,
        PressureDecision, ReactiveViewBudget, Regression, RegressionMetric, RegressionPolicy,
        StreamBudget, WorkloadManifest, WorkloadProfile, compare_reports,
    };
    pub use aequora_profile::{
        AdapterProfileCapabilities, AggregateDefinition, AggregateProfile, AggregateProfileBuilder,
        AggregateProfileId, AuditPolicy, ConsistencyProfile, ConsistencyProfileKind,
        CustomProfileBuilder, DeletePolicy, DomainRisk, OperationOrigin, OperationProfileBuilder,
        OperationProfileDefinition, OperationSemanticClass, OperationSemanticProfile,
        OrderingPolicy, ProfileCapability, ProfileError, ProfileManifest, ProfileOverrides,
        ProfileRegistry, ProfileRequirements, ProfileRetryPolicy, ProfileVersion, ScopePolicy,
        SnapshotPolicy, VersioningPolicy,
    };
    pub use aequora_protocol::{
        BootstrapRequest, BootstrapResponse, Capability, ChangeKind, ClientLimits,
        OperationEnvelope, OperationKind, OperationMetadata, Partition, PushHint, PushHintReason,
        ResyncReason, SessionMetadata, SnapshotEntity, SnapshotLimits, SyncDirective, SyncRequest,
        SyncResponse,
    };
    pub use aequora_queue::{
        CancellationDescriptor, CompactionPlan as QueueCompactionPlan, CompactionPolicy,
        FieldGroupId, LocalOperationSeq, MutationMutability, OperationAccess, OperationClassId,
        OperationOptimization, OperationRisk, OptimizationRegistry, PairAction, QueueEntry,
        QueueError, RebasePlan, RebasePolicy, RebaseRewrite, RebaseTarget, Supersession,
        SupersessionReason, plan_compaction, plan_rebase, semantic_envelope_hash,
    };
    pub use aequora_region::{
        AssignmentTrust, AuthorityLocation, AuthorityShardId, DirectoryGeneration,
        EndpointReadPolicy, ProjectionApplyDecision, ProjectionId,
        ProjectionSchemaVersion as RegionalProjectionSchemaVersion, ProjectionWatermark,
        ReadConsistency, ReadFallbackPolicy, ReadResponseMetadata, ReadRouteDecision, ReadTarget,
        RegionError, RegionMode, RegionalArtifactEvidence, RegionalArtifactKind,
        RegionalCacheMetadata, RegionalCopyId, RegionalCopyKind, RegionalCopyRecord,
        RegionalDeploymentProfile, RegionalEventKind, RegionalGovernanceRegistry,
        RegionalGovernanceReport, RegionalPlacementKind, RegionalPurgeState, RegionalReadRequest,
        RegionalRole, RegionalRouter, RegionalRouterConfig, ReplicaId, ReplicaObservation,
        ReplicaReadGuard, ReplicaWait, ReplicaWatermark, ResidencyPolicy, SessionWatermark,
        StalenessBudget, TenantAuthorityAssignment, TenantAuthorityDirectory,
        validate_projection_apply,
    };
    pub use aequora_replay::{
        AllocatedId, AllocatedIdKind, CanonicalOperation, CapturedExternalResult,
        DeterministicIdAllocator, DeterministicRandom, DifferentialReport, DomainClock,
        DomainTimestamp, ExecutionContext, ExecutionInputs, ExecutionPlan, ExecutionPlanDigest,
        ExecutionSeed, FixedClock, HandlerVersion, OriginPrincipal, PlanCommitter, PlannedEvent,
        PlannedMutation, PolicySnapshot, RecordingSideEffectSink, ReplayBundle, ReplayError,
        ReplayHandler, ReplayMode, ReplayOutcome, ReplayReport, ReplaySandbox, ReplaySandboxLimits,
        ReplayStateRef, SideEffectIntent, SideEffectSink, SystemDomainClock, verify_corpus,
    };
    pub use aequora_routing::{
        NoEligibleRegion, RegionHealth, RegionRole, RegionRouter, RegionState, RouteDecision,
        RouteReason, RoutingIntent,
    };
    pub use aequora_scheduler::{
        AdaptiveScheduler, AppActivity, BandwidthEstimate, BatchBounds, BatchController,
        CircuitBreaker, CircuitState, CompressionDecision, DeferralReason, EffectivePriority,
        NetworkContext, OperationPriorityMetadata, OperationPriorityRegistry, PowerContext,
        ResourcePressure, RetryPolicy, SchedulerError, SchedulerPolicy, SchedulerState,
        SchedulingContext, SchedulingDecision, SchedulingPolicy, SelectedWork,
        ServerSchedulingHints, SyncProfile, WorkClass, WorkDescriptor, WorkId, WorkKind,
    };
    pub use aequora_scope::{
        CursorValidity, DatasetPartitionId, EvaluatedJournalEntry, FilteredScopePage,
        LocalRetentionPolicy, LocalScopeState, MembershipDecision, MembershipEvaluator,
        MembershipRecord, PendingIntentDisposition, ProjectionRule, ProjectionVersion,
        ResolvedScope, ScopeBootstrapMode, ScopeBootstrapPlan, ScopeCursor, ScopeDefinitionId,
        ScopeDescriptor, ScopeError, ScopeGeneration, ScopePrincipal, ScopeRegistry, ScopeRemoval,
        ScopeRemovalReason, ScopeRequest, ScopeResolver, ScopeResourceAuthorizer, ScopeServerState,
        ScopeStatus, ScopeTransition, ScopeTransitionId, ScopeTransitionInstruction,
        ScopeTransitionKind, ScopeTransitionOutcome, ScopeVersion, Subscription, SubscriptionId,
        SubscriptionState, project_filtered_page,
    };
    pub use aequora_server::{
        AdmittedExchangeService, AequoraServer, ExchangeService, ServerBuildError,
        ServerCommandOutcome, ServerConfig, ServerError, SyncServer, SyncServerBuilder,
    };
    pub use aequora_store::{
        AdapterCapabilities, AdapterCompatibilityError, AdapterManifest, AdapterManifestProvider,
        AdapterRequirements, AdapterRole, AdapterTier, AuditLog, AuditOffset, AuditPage,
        AuditRecord, AuthoritativeIntegritySource, AuthoritativeStore, ConflictInbox,
        ConflictRecord, ConflictResolution, CorrelationLog, IntegrityCapabilityProvider,
        LocalIntegrityStore, LocalStore, OutboxState, OutboxStateStore, OutboxStats, OutboxStore,
        ProductionAdapterPair, ReplicaRepairReport, ScopeStateStore,
    };
    pub use aequora_transport::{SnapshotPageStream, StreamingSyncTransport, SyncTransport};
    pub use aequora_types::{
        ActorId, CorrelationId, Cursor, DeviceId, EntityId, EntityRef, EntityType, EntityVersion,
        EventId, HybridTimestamp, IntegritySessionId, JobId, LineageContext, LineageRef, NodeId,
        OperationId, ProtocolVersion, RegionId, RepairId, RequestId, SchemaVersion, Sequence,
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
