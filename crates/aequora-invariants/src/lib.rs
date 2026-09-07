//! Stable, normative identifiers for Aequora's cross-layer correctness invariants.
//!
//! The registry is intentionally independent of databases, transports, and runtime frameworks.
//! Model checks, adapter suites, diagnostics, and public documentation should refer to the same
//! identifiers instead of inventing layer-specific names for equivalent guarantees.

use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// A stable identifier from the normative Part 01 invariant registry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum InvariantId {
    /// One operation identifier produces at most one authoritative logical effect.
    IdempotentAuthority,
    /// A committed local synchronizable mutation has durable outbox intent and vice versa.
    LocalIntentAtomicity,
    /// Accepted authority state, journal publication, and ledger result commit atomically.
    AuthoritativePublicationAtomicity,
    /// A cursor advances only after all required preceding events are durably applied.
    CursorSafety,
    /// An authoritative entity version never decreases.
    VersionMonotonicity,
    /// An unauthorized operation never becomes authoritative.
    NoUnauthorizedCommit,
    /// Retrying an operation cannot create an additional logical effect.
    RetryPreservation,
    /// Applying the same authoritative event repeatedly has one logical effect.
    ReconciliationIdempotency,
    /// A stale update cannot silently resurrect a tombstoned entity.
    TombstoneSafety,
    /// A cursor cannot cross incompatible authority epochs.
    TimelineSafety,
    /// Every authoritative event has one stable identity.
    UniqueEventIdentity,
    /// Every authoritative event belongs to exactly one correlation chain.
    EventCorrelation,
    /// A retry cannot alter the original operation lineage.
    RetryLineagePreservation,
    /// A derived event or job retains its parent's correlation chain.
    DerivedLineagePreservation,
    /// Correlation queries and lineage records never cross tenant boundaries.
    TenantLineageSafety,
    /// Direct causal ancestry is acyclic.
    CausalityAcyclic,
    /// Compatible matching roots detect no divergence in the covered scope.
    AntiEntropyRootAgreement,
    /// Replica repair cannot mutate authoritative state from client data.
    RepairAuthoritySafety,
    /// Repair retains exact pending operation intent.
    RepairIntentPreservation,
    /// Same-version canonical mismatch is an integrity anomaly.
    SameVersionDigestSafety,
    /// Integrity verification never advances normal synchronization progress.
    IntegrityCursorIsolation,
    /// Declared compaction preserves observable application semantics.
    QueueSemanticEquivalence,
    /// Possibly delivered operation identities retain an immutable semantic envelope.
    DeliveredPayloadImmutability,
    /// Compaction cannot remove an operation still required by an active dependency.
    QueueDependencySafety,
    /// Storage optimization alone cannot discard pending user intent.
    QueueIntentPreservation,
    /// Rebase retains identity only when semantic intent is unchanged.
    RebaseIdentitySafety,
    /// Sensitive operations remain noncompactable without explicit certification.
    SensitiveOperationSafety,
    /// Only one fencing epoch is current for a persistent local store.
    LocalFenceUniqueness,
    /// Stale leaders cannot commit leader-exclusive local state.
    StaleLeaderCommitSafety,
    /// Domain mutation plus outbox append remains follower-safe.
    FollowerMutationSafety,
    /// An eligible follower can take over after lease expiry.
    LocalLeadershipLiveness,
    /// Leadership changes preserve pending semantic intent.
    LeadershipIntentPreservation,
    /// Store generation changes invalidate earlier coordinator assumptions.
    StoreGenerationSafety,
    /// Scheduling cannot alter operation identity, payload, or dependencies.
    QosSemanticSafety,
    /// Deferred work remains durable and retryable.
    QosIntentPreservation,
    /// Scheduler batches never exceed local hard limits.
    QosBatchBounds,
    /// Weighted fairness and aging prevent permanent eligible-work starvation.
    QosStarvationFreedom,
    /// Server load hints can reduce work but cannot discard it.
    QosServerHintSafety,
    /// Leadership changes preserve durable retry timing and identities.
    QosLeadershipRetrySafety,
    /// A scope cursor is valid only for one exact identity, version, and generation.
    ScopeCursorBinding,
    /// Authorized resolution and projection cannot expose out-of-scope data.
    ScopeAuthorizationSafety,
    /// Contraction deactivates removed membership before the new version is active.
    ScopeContractionSafety,
    /// Scope-local removal never asserts authoritative domain deletion.
    ScopeRemovalDistinction,
    /// Removing one membership cannot erase an entity referenced by another active scope.
    ScopeSharedMembershipSafety,
    /// Revoked pending intent cannot remain eligible for transmission.
    ScopeRevokedIntentSafety,
    /// Expansion data cannot become active before its bootstrap is complete.
    ScopeExpansionAtomicity,
    /// Durable convergence never depends on live hint delivery.
    LiveHintLossSafety,
    /// A live hint cannot directly mutate cursor or replica state.
    LiveNoStateMutation,
    /// Live fan-out never intentionally crosses an unauthorized scope boundary.
    LiveScopeIsolation,
    /// Every slow-consumer hint queue has a hard bound.
    LiveBackpressureBound,
    /// Leadership handoff uses immediate durable catch-up instead of hint replay.
    LiveLeadershipCatchUp,
    /// Retry maps one source record to one stable target identity.
    ImportIdentityDeterminism,
    /// Checkpoints never advance beyond atomically committed target data.
    ImportCheckpointSafety,
    /// Cutover cannot occur before every required verification succeeds.
    ImportCutoverVerification,
    /// Baseline seeding does not fabricate per-row synchronization history.
    ImportBaselineSafety,
    /// Post-activation imported authority changes remain journal-visible.
    ImportJournalVisibility,
    /// Quarantined records cannot count silently as successful imports.
    ImportQuarantineSafety,
    /// A bootstrap cursor activates only with its complete durable generation.
    BootstrapCursorActivationSafety,
    /// Published chunks all belong to one consistent boundary.
    BootstrapBoundaryConsistency,
    /// Partial staging is never exposed as an active scope.
    BootstrapStagingIsolation,
    /// Chunk retry cannot duplicate final entities.
    BootstrapChunkIdempotency,
    /// Rebootstrap preserves pending local intent.
    BootstrapIntentPreservation,
    /// Snapshot plus delta replay converges with continuous replication.
    BootstrapDeltaConvergence,
    /// Operation semantics must remain compatible with their aggregate profile.
    ProfileOperationCompatibility,
    /// An adapter must prove every capability required by a registered profile.
    ProfileCapabilitySafety,
    /// Immutable append-only aggregates cannot lose or rewrite accepted intent.
    ProfileImmutableAppendOnly,
    /// Strong aggregates require atomic aggregate commits and compare-and-swap.
    ProfileStrongAggregateAtomicity,
    /// Derived projections are never accepted as client authority.
    ProfileDerivedAuthoritySafety,
    /// Unknown operations cannot silently fall back to last-writer-wins.
    ProfileNoImplicitLastWriterWins,
    /// Identical captured decisions produce equivalent canonical plans.
    DeterministicPlanEquivalence,
    /// Domain decisions cannot depend on uncaptured time or randomness.
    DeterministicInputCapture,
    /// Replay cannot invoke a real external side effect.
    ReplaySideEffectIsolation,
    /// A committed operation cannot acquire different execution inputs on retry.
    ReplayCommittedInputImmutability,
    /// Handler semantic version changes are explicit and compatibility-tested.
    ReplayHandlerVersionSafety,
    /// Mutable policy/config affecting decisions is explicitly versioned and captured.
    ReplayPolicyVersionSafety,
    /// Required audit evidence commits with the authoritative mutation.
    AuditRequiredAtomicity,
    /// A transport retry cannot duplicate one logical business audit effect.
    AuditRetryDeduplication,
    /// System and service work cannot be attributed to an invented user.
    AuditActorTruthfulness,
    /// Canonical audit history is append-only and corrections reference prior events.
    AuditAppendOnlyCorrection,
    /// Sensitive values follow an explicit field audit policy.
    AuditSensitiveValueSafety,
    /// Explanation queries cannot bypass current authorization.
    AuditExplanationAuthorization,
    /// Chained audit events reference the prior valid partition hash.
    AuditChainContinuity,
    /// Audit integrity failures surface and are never silently repaired.
    AuditTamperDetection,
    /// Authoritative field provenance identifies its latest committed audit/event mutation.
    AuditFieldProvenanceAccuracy,
    /// Tombstones remain while a valid retained client can resume before deletion.
    GovernanceTombstoneGcSafety,
    /// Clients below the journal floor rebootstrap instead of continuing incrementally.
    GovernanceJournalFloorSafety,
    /// Active holds override normal retention purge.
    GovernanceLegalHoldPrecedence,
    /// Erasure retains or minimizes policy-required evidence explicitly.
    GovernanceRequiredEvidenceSafety,
    /// Completion requires successful verification from every required storage surface.
    GovernanceSurfaceCompletion,
    /// Tenant write authority is revoked before destructive offboarding purge.
    GovernanceOffboardingWriteFence,
    /// Restore reapplies erasures and revocations before serving traffic.
    GovernanceRestoreReconciliation,
    /// Retired stale clients cannot resurrect physically purged identities.
    GovernanceResurrectionSafety,
    /// Registered sensitive copies participate in governance plans.
    GovernanceCopyCoverage,
    /// Signed artifacts verify under a trusted key authorized for their purpose.
    CryptoArtifactAuthenticity,
    /// Tenant context is cryptographically bound to signed and encrypted artifacts.
    CryptoTenantBinding,
    /// Rotation preserves historical verification through immutable key identities.
    CryptoRotationHistory,
    /// Revoked keys cannot create new signatures or ciphertext.
    CryptoRevocationSafety,
    /// Secret key material never enters ordinary serializable evidence.
    CryptoSecretHandling,
    /// Required cryptographic verification fails closed.
    CryptoFailClosed,
    /// Opaque payloads cannot hide server-side decision fields.
    CryptoE2eSemanticBoundary,
    /// Erasure completion requires destruction of every usable decrypting key copy.
    CryptoErasureCompletion,
    /// A possibly delivered operation identity retains one signed semantic payload.
    CryptoSignedOperationImmutability,
    /// At most one unfenced authority instance accepts writes for a timeline.
    AuthoritySingleWriter,
    /// A cursor never crosses authority epochs as incremental continuation.
    AuthorityCursorEpochBinding,
    /// Divergent or restored history receives a new epoch unless continuity is proven.
    AuthorityDivergenceEpoch,
    /// A client never silently accepts a lower trusted authority epoch.
    AuthorityRollbackSafety,
    /// Fork detection never automatically merges authoritative histories.
    AuthorityNoAutomaticForkMerge,
    /// Ambiguous old-epoch operations follow explicit recovery policy.
    AuthorityOperationRecoveryPolicy,
    /// A promoted fence prevents the old primary from committing.
    AuthorityOldPrimaryFence,
    /// Proven lossless infrastructure failover retains the epoch.
    AuthorityLosslessEpochContinuity,
    /// Timeline-dependent artifacts bind to their authority epoch.
    AuthorityArtifactEpochBinding,
    /// Regional topology never creates another authoritative writer.
    RegionSingleWriter,
    /// `AtLeast` reads require a current-epoch verified apply watermark.
    RegionAtLeastWatermark,
    /// Session reads never intentionally return below the caller watermark.
    RegionSessionMonotonicity,
    /// Wrong-epoch replicas and caches are never served as current.
    RegionEpochIsolation,
    /// Regional infrastructure failure cannot create a write timeline.
    RegionFailureAuthoritySafety,
    /// Fallback never silently weakens endpoint consistency.
    RegionFallbackSafety,
    /// Edge transport cannot change artifact authority or integrity.
    RegionArtifactIntegrity,
    /// Tenant residency constrains every regional placement.
    RegionResidencyCoverage,
    /// Governance completion includes every required regional copy.
    RegionGovernanceCoverage,
    /// Every in-memory work queue has an explicit configured bound.
    LoadBoundedQueues,
    /// Overload rejection happens before authoritative mutation.
    LoadPreMutationRejection,
    /// Tenant fairness preserves globally reserved opportunity.
    LoadTenantFairness,
    /// Slow live clients cannot consume unbounded memory.
    LoadSlowClientBound,
    /// Bulk and maintenance cannot permanently starve higher-value work.
    LoadPriorityStarvationSafety,
    /// Retry guidance retains client-side jitter and backoff.
    LoadRetryJitterSafety,
    /// Required durable work never exists only in an in-memory queue.
    LoadDurableIntentSafety,
    /// Overload never silently weakens consistency.
    LoadConsistencySafety,
    /// Limits are acquired before entering the protected resource domain.
    LoadAdmissionOrdering,
    /// Every externally driven collection and queue has explicit item and byte bounds.
    PerformanceBoundedMemory,
    /// Large snapshots, blobs, and exports do not require full in-memory materialization.
    PerformanceStreamingLargeObjects,
    /// CPU-heavy work cannot run unbounded on an asynchronous I/O worker.
    PerformanceCpuIsolation,
    /// Performance changes preserve every semantic and durability guarantee.
    PerformanceCorrectnessPreservation,
    /// Hot registries and configuration use immutable versioned snapshots.
    PerformanceImmutableHotState,
    /// Accepted optimization evidence is bound to a reproducible workload and measured phase.
    PerformanceReproducibility,
    /// Reactive UI state remains a bounded query-derived page rather than a database mirror.
    PerformancePagedUiState,
    /// Large binary domain content uses bounded streaming blob references.
    PerformanceBlobReferences,
    /// Admission and frame limits reject work before expensive allocation or execution.
    PerformanceEarlyAdmission,
    /// Resource pressure cannot silently discard unsynchronized durable user intent.
    ClientIntentPreservation,
    /// Client cursors advance only after durable local application.
    ClientCursorDurability,
    /// Required security/governance directives are never silently ignored.
    ClientRequiredDirective,
    /// Large bootstrap and blob work stays memory-bounded on supported client profiles.
    ClientBoundedLargeObject,
    /// Every background unit leaves restart-recoverable durable state.
    ClientCheckpointRecovery,
    /// Eviction cannot remove base state pinned by unresolved pending intent.
    ClientPendingBaseEviction,
    /// Local-first success requires atomic domain mutation and outbox commit.
    ClientLocalCommitAtomicity,
    /// Resource profiles change limits and timing, never consistency semantics.
    ClientSemanticParity,
    /// Server-visible resource capability is coarse and never authorization evidence.
    ClientTelemetryPrivacy,
    /// Messages are decoded only with explicit protocol, kind, and payload-version context.
    CompatExplicitVersionContext,
    /// Required safety or semantic capabilities never silently downgrade.
    CompatRequiredCapabilitySafety,
    /// Possibly-sent operations retain immutable schema and payload semantics.
    CompatPossiblySentImmutability,
    /// Removed protocol and capability IDs are permanently reserved.
    CompatStableIdReservation,
    /// Fleet capabilities are complete before a new required feature activates.
    CompatFleetActivationSafety,
    /// Upgrade-required state preserves durable local user intent.
    CompatUpgradeIntentPreservation,
    /// Operation schema support covers the retry horizon or yields explicit recovery.
    CompatRetryHorizon,
    /// Protocol changes remain independent from authority epoch changes.
    CompatAuthorityEpochIndependence,
    /// Compatibility policy is versioned, auditable, and fail-closed.
    CompatPolicyAuditability,
    /// Every durable store declares one supported internal metadata schema before operation.
    MetadataStoreSchemaDeclaration,
    /// Operation ledger identity is unique and semantic payload drift is rejected.
    MetadataLedgerOperationIdentity,
    /// Authoritative client apply and cursor advancement share one local transaction.
    MetadataClientCursorAtomicity,
    /// Required authority metadata commits atomically with its business mutation.
    MetadataAuthoritativeAtomicity,
    /// Published snapshots contain only durable verified chunks from one boundary.
    MetadataSnapshotPublicationSafety,
    /// Physical adapters preserve logical metadata semantics.
    MetadataAdapterSemanticEquivalence,
    /// Stale fencing tokens cannot update durable metadata state.
    MetadataStaleFenceRejection,
    /// Internal migrations preserve pending intent or leave no partial migration.
    MetadataMigrationIntentPreservation,
    /// Ordinary metadata records never contain private key material.
    MetadataSecretKeyExclusion,
    /// Required asynchronous work is durable before execution can be depended upon.
    JobDurableBeforeExecution,
    /// Only the current fence holder may checkpoint or terminally transition a job.
    JobCurrentFenceOnly,
    /// Domain-side external effects require a committed immutable intent.
    JobCommittedSideEffectIntent,
    /// Workers submit business mutations through authoritative domain handlers.
    JobAuthoritativeResultOperation,
    /// Ambiguous provider outcomes follow an explicit recovery policy.
    JobExplicitAmbiguityRecovery,
    /// Required job and workflow state is never process-memory-only.
    JobDurableState,
    /// Expired stale workers cannot overwrite a newer claim.
    JobStaleWorkerRejection,
    /// Long-running jobs checkpoint bounded progress.
    JobBoundedCheckpoint,
    /// Retry scheduling is backed off and cannot hot-loop.
    JobBoundedRetry,
    /// Admin mutations cannot bypass the responsible correctness service.
    AdminNoDomainBypass,
    /// High-risk admin mutations have authenticated durable attribution.
    AdminDurableAttribution,
    /// An admin idempotency identity cannot change its action payload.
    AdminPayloadImmutability,
    /// Destructive execution binds the exact reviewed non-stale plan.
    AdminReviewedPlanBinding,
    /// Admin authorization is evaluated by the server-side core.
    AdminServerAuthorization,
    /// Admin responses never expose private cryptographic key material.
    AdminPrivateKeyExclusion,
    /// Force operations remain distinct and more strongly guarded.
    AdminOverrideSeparation,
    /// Data-plane correctness does not depend on control-plane availability.
    AdminDataPlaneIndependence,
    /// High-risk completion requires verified postconditions.
    AdminVerifiedCompletion,
    /// Diagnostic evidence never becomes authoritative business state.
    DiagnosticNonAuthoritative,
    /// Ordinary bundles exclude secret material by construction.
    DiagnosticSecretExclusion,
    /// Every bundle declares schema, scope, completeness, and content digest.
    DiagnosticManifestCompleteness,
    /// Reproduction never performs real external side effects.
    DiagnosticReplaySideEffectIsolation,
    /// Collection has explicit size, time, file, and record bounds.
    DiagnosticCollectionBounds,
    /// Explanations preserve evidence-source confidence.
    DiagnosticEvidenceConfidence,
    /// Redaction and governance precede artifact publication.
    DiagnosticPrePublicationSanitization,
    /// Verified bundles passed required schema, hash, and signature checks.
    DiagnosticVerifiedBundle,
    /// Replay cannot mutate production authority or client state.
    DiagnosticReplayProductionIsolation,
    /// Every aggregate has one effective authoritative writer.
    LegacySingleWriteOwner,
    /// CDC cursors advance only with a durable canonical bridge result.
    LegacyCursorAfterDurability,
    /// Duplicate legacy changes produce one canonical effect.
    LegacyBridgeIdempotency,
    /// Direct legacy writes are fenced after Aequora cutover.
    LegacyPostCutoverFence,
    /// Legacy APIs enter authority through typed domain operations.
    LegacyTypedFacade,
    /// Unknown legacy business states never silently become valid canonical states.
    LegacyMappingFailClosed,
    /// Shadow execution cannot commit or perform real external effects.
    LegacyShadowIsolation,
    /// Governance covers legacy copies until formal retirement.
    LegacyGovernanceCoverage,
    /// Cutover completion requires fencing, final-boundary apply, and verification.
    LegacyVerifiedCutover,
    /// Client claims are never accepted as authorization evidence without server validation.
    SecurityServerValidatedClaims,
    /// One operation identity cannot acquire different authoritative semantics.
    SecurityPayloadImmutability,
    /// Required security capabilities cannot be silently downgraded.
    SecurityCapabilityFailClosed,
    /// Every external input has explicit size and complexity bounds.
    SecurityExternalInputBounds,
    /// Known identifiers cannot bypass tenant isolation.
    SecurityTenantIsolation,
    /// Private keys and authentication secrets stay out of ordinary output.
    SecuritySecretExclusion,
    /// A stale authority epoch cannot silently resume trusted synchronization.
    SecurityAuthorityRollback,
    /// Irreversible side effects use idempotency and reconciliation.
    SecuritySideEffectSafety,
    /// Administrative overrides are more strongly authorized and audited.
    SecurityAdminOverride,
    /// Every integration boundary treats its inputs as untrusted.
    SecurityIntegrationDistrust,
    /// Every durable consumer owns an independent epoch-bound cursor.
    FeedIndependentCursor,
    /// Consumer cursors advance only after required effects are durable.
    FeedDurableEffectBeforeCursor,
    /// A stalled consumer cannot block authority, sync, or peers.
    FeedConsumerIsolation,
    /// Duplicate `EventId` delivery cannot duplicate an idempotent consumer effect.
    FeedDuplicateIdempotency,
    /// Consumers below the journal floor follow declared recovery policy.
    FeedRetentionRecovery,
    /// External feeds expose only authorized versioned projections.
    FeedExternalProjectionSafety,
    /// Partitioning preserves the declared ordering policy.
    FeedOrderingSafety,
    /// History-skipping resets require plan, authorization, and audit.
    FeedResetSafety,
    /// Governed consumer stores participate in governance and residency policy.
    FeedGovernanceCoverage,
    /// Published durable IDs never acquire different semantics.
    RegistryIdNonReuse,
    /// Durable IDs resolve canonically or fail closed as unknown.
    RegistryCanonicalResolution,
    /// Breaking semantics require versioned migration, upcast, or incompatibility.
    RegistryBreakingChangePath,
    /// Runtime lookup tables cannot diverge from canonical sources.
    RegistryGeneratedSourceParity,
    /// Retired IDs remain reserved and historically interpretable.
    RegistryHistoricalReservation,
    /// Application and extension allocations cannot collide.
    RegistryNamespaceIsolation,
    /// Security-sensitive registry changes require explicit review.
    RegistrySecurityReview,
    /// Dynamic configuration cannot redefine compiled durable semantics.
    RegistryRuntimeImmutability,
    /// Supported historical artifacts retain durable-ID resolution.
    RegistryHistoricalResolution,
    /// Certification tests only observable semantics, never physical implementation choices.
    CertificationSemanticOnly,
    /// A tier cannot be claimed when a required test fails, is skipped, or is unsupported.
    CertificationTierTruthfulness,
    /// Evidence binds the exact subject, features, suite, and environment.
    CertificationExactBinding,
    /// Claimed capabilities remain unverified until their required tests pass.
    CertificationCapabilityTruthfulness,
    /// Certification never replaces runtime input validation or startup safety checks.
    CertificationRuntimeValidation,
    /// Historical certification artifacts remain immutable and verifiable.
    CertificationArtifactImmutability,
    /// Evidence hashes and signatures are verified before trust is accepted.
    CertificationEvidenceIntegrity,
    /// Performance characterization cannot substitute for correctness evidence.
    CertificationCorrectnessPriority,
    /// Advisories change lifecycle status without reusing certification identity.
    CertificationLifecycleIdentity,
    /// Required synchronization state never exists only in a mobile process.
    MobileDurableProcessIndependence,
    /// Mobile local mutation and outbox insertion remain atomic.
    MobileIntentAtomicity,
    /// Push notifications can request sync but never carry authoritative state.
    MobilePushHintOnly,
    /// Background timeout cannot advance a cursor beyond durable application.
    MobileCursorCheckpointSafety,
    /// Private key material remains in an approved secure provider.
    MobileSecureKeyStorage,
    /// Resource adaptation cannot weaken correctness or governance.
    MobileResourceSemanticSafety,
    /// Upgrade preserves pending intent or fails without partial destruction.
    MobileUpgradeIntentSafety,
    /// Failed durable storage cannot be reported as a saved mutation.
    MobileStorageTruthfulness,
    /// Platform and UI layers cannot bypass Rust synchronization semantics.
    MobilePlatformBoundary,
    /// One active desktop coordinator owns a local store.
    DesktopSingleCoordinator,
    /// A stale desktop fencing token cannot commit coordinator metadata.
    DesktopStaleFenceSafety,
    /// Resume and ambiguous retry cannot duplicate an authoritative effect.
    DesktopResumeIdempotency,
    /// Local IPC cannot bypass domain operations or mutate synchronization metadata.
    DesktopIpcBoundary,
    /// A cloned store cannot silently retain its trusted device binding.
    DesktopCloneBindingSafety,
    /// Desktop upgrade preserves pending user intent.
    DesktopUpgradeIntentSafety,
    /// Agent and in-process modes preserve identical synchronization semantics.
    DesktopModeParity,
    /// Disk and credential-store failure is reported truthfully.
    DesktopPersistenceTruthfulness,
    /// Derived desktop state never becomes authoritative business state.
    DesktopDerivedStateSafety,
    /// Domain mutation and outbox intent commit as one durable transaction.
    StorageLocalIntentAtomicity,
    /// Storage pressure never automatically evicts critical durable intent.
    StorageCriticalIntentRetention,
    /// Derived/cache loss cannot remove pending authoritative intent.
    StorageCacheIsolation,
    /// Restored or cloned stores cannot silently reuse a mismatched binding.
    StorageCloneBindingSafety,
    /// Older binaries reject newer uncertified store formats.
    StorageDowngradeSafety,
    /// Storage semantics are certified on each actual target platform.
    StoragePlatformCertification,
    /// Temporary data is verified before durable publication.
    StoragePublicationSafety,
    /// Low disk cannot produce a false successful commit.
    StorageAdmissionTruthfulness,
    /// Secrets never reside as plaintext ordinary sync metadata.
    StorageSecretIsolation,
    /// Live embedded stores avoid unverified network or cloud filesystems.
    StorageFilesystemSafety,
    /// Foundation and protocol crates never reach physical or framework integrations.
    ImplementationFoundationIsolation,
    /// Physical database types never leak into storage-neutral public contracts.
    ImplementationStorageTypeIsolation,
    /// Client synchronization correctness remains independent of UI frameworks.
    ImplementationClientUiIsolation,
    /// Server synchronization correctness remains independent of HTTP routing frameworks.
    ImplementationServerHttpIsolation,
    /// Physical adapters depend inward on storage contracts, never the reverse.
    ImplementationAdapterDirection,
    /// Platform-specific dependencies remain isolated to their integration crates.
    ImplementationPlatformIsolation,
    /// Cargo features do not replace physical adapter crate boundaries.
    ImplementationFeatureIsolation,
    /// Application binaries remain composition roots rather than semantic owners.
    ImplementationCompositionRoot,
    /// Generated registry artifacts deterministically match canonical sources.
    ImplementationRegistryDeterminism,
    /// Every dependency edge conforms to the declared machine-readable layer graph.
    ImplementationDependencyGraph,
    /// Public client APIs cannot directly advance authoritative synchronization cursors.
    SdkCursorIsolation,
    /// Local mutation success is distinct from authoritative server confirmation.
    SdkLocalCommitDistinction,
    /// Stable public APIs remain storage and platform neutral.
    SdkStorageNeutrality,
    /// Cancelling an SDK future cannot invalidate already-durable intent.
    SdkCancellationSafety,
    /// Advisory event loss cannot make durable state unrecoverable.
    SdkEventLossSafety,
    /// Stable error categories and codes remain interpretable across releases.
    SdkErrorCompatibility,
    /// Extensions cannot redefine core cursor, authority, idempotency, or identity semantics.
    SdkClosedCoreSemantics,
    /// Crate, protocol, store, and operation schema versions remain independent.
    SdkVersionIndependence,
    /// Dangerous operations are absent from ordinary convenience APIs.
    SdkDangerousOperationIsolation,
    /// Public API changes receive an automated semver compatibility check.
    SdkSemverAutomation,
    /// Capability claims require matching environment-bound conformance evidence.
    AdapterCapabilityTruthfulness,
    /// Local domain mutation and outbox insertion remain atomic.
    AdapterLocalAtomicity,
    /// Authoritative business, version, journal, ledger, and audit state commit atomically.
    AdapterAuthorityAtomicity,
    /// Physical transaction and driver error types never leak into neutral APIs.
    AdapterTypeIsolation,
    /// Reusing an operation identity with a different payload digest is rejected.
    AdapterPayloadBinding,
    /// Physical migrations preserve durable synchronization identity and progress.
    AdapterMigrationSafety,
    /// Missing required storage capabilities fail startup.
    AdapterStartupSafety,
    /// Certification binds adapter, engine, platform, and relevant feature configuration.
    AdapterEnvironmentBinding,
    /// Performance tuning cannot weaken critical durable intent.
    AdapterCriticalDurability,
    /// Official adapters publish stable manifests and explicit limitations.
    AdapterManifestTruthfulness,
    /// `PostgreSQL` Tx B atomically commits every required authoritative effect.
    PostgresAuthorityAtomicity,
    /// `PostgreSQL` duplicate delivery returns the prior outcome without another effect.
    PostgresIdempotentReplay,
    /// `PostgreSQL` rejects `OperationId` reuse with a different canonical digest.
    PostgresPayloadBinding,
    /// `PostgreSQL` journal order follows a transactional committed timeline.
    PostgresCommittedTimeline,
    /// PostgreSQL/Neon restore opens a new authority epoch before synchronization.
    PostgresRestoreEpoch,
    /// `SQLx` and `PostgreSQL`-specific types remain inside the physical adapter.
    PostgresTypeIsolation,
    /// Unsafe `PostgreSQL` correctness settings fail authoritative readiness.
    PostgresReadinessSafety,
    /// External side effects exist as durable Tx B intents and execute afterward.
    PostgresSideEffectIntent,
    /// `PostgreSQL` retention never crosses the proven safe journal floor.
    PostgresRetentionSafety,
    /// Neon operational behavior never changes authority semantics.
    PostgresNeonSemanticParity,
    /// Durable provisional domain mutation and outbox intent commit atomically in Stoolap.
    StoolapLocalAtomicity,
    /// Authoritative apply, outcomes, conflicts, overlay, and cursor commit atomically in Stoolap.
    StoolapReconciliationAtomicity,
    /// Possibly transmitted Stoolap operations retain identity and semantic payload.
    StoolapRetryIdentity,
    /// Rebootstrap, repair, migration, and storage pressure preserve pending intent.
    StoolapIntentPreservation,
    /// Stoolap cursors cannot exceed durably installed authoritative state.
    StoolapCursorSafety,
    /// Only the current fenced coordinator performs leader-owned Stoolap transitions.
    StoolapFencedCoordinator,
    /// Restored or cloned Stoolap replicas validate device binding and secure-key state.
    StoolapCloneSafety,
    /// Stoolap storage reclamation never evicts critical pending intent or correctness metadata.
    StoolapStoragePressureSafety,
    /// Stoolap-specific types remain inside the physical local adapter.
    StoolapTypeIsolation,
    /// Stoolap support claims require a passing target-bound conformance profile.
    StoolapPlatformCertification,
    /// Axum owns transport orchestration and never synchronization or domain correctness.
    AxumTransportIsolation,
    /// Raw HTTP authentication credentials never reach domain or storage boundaries.
    AxumCredentialIsolation,
    /// HTTP wire, decompressed, operation, and dependency work is bounded before execution.
    AxumResourceBounds,
    /// Response loss cannot invalidate an already committed authoritative operation.
    AxumCommitDeliveryIndependence,
    /// Overload is rejected before unbounded task, memory, or database-pool growth.
    AxumOverloadBounds,
    /// Protocol tenant and device claims match authenticated server identity before execution.
    AxumIdentityBinding,
    /// HTTP status never replaces stable Aequora operation and error semantics.
    AxumStableSemantics,
    /// Durable convergence never depends on delivery of live HTTP hints.
    AxumLiveHintDurability,
    /// Ordinary HTTP node lifecycle never creates an authority epoch.
    AxumNodeEpochIndependence,
    /// Public HTTP failures never expose internal topology, credentials, or stack details.
    AxumErrorSanitization,
    /// Correctness-critical synchronization state never exists only in Dioxus memory.
    DioxusDurableStateOwnership,
    /// The UI reports local durability only after domain state and outbox intent commit.
    DioxusLocalCommitTruth,
    /// Local persistence never implies authoritative server confirmation.
    DioxusAuthorityDistinction,
    /// Loss of advisory UI events cannot lose synchronization correctness.
    DioxusEventLossSafety,
    /// Component unmount cannot erase previously committed operation intent.
    DioxusUnmountSafety,
    /// Query caches and reactive state are isolated by active store identity.
    DioxusStoreIsolation,
    /// Conflict resolution is durable semantic intent, not direct UI-state mutation.
    DioxusConflictSemantics,
    /// Offline mode preserves domain-permitted local reads and writes.
    DioxusOfflineCapability,
    /// OS background work is owned by platform/client runtimes, not mounted components.
    DioxusBackgroundOwnership,
    /// Large synchronization batches cannot create unbounded UI queues or rerenders.
    DioxusEventBounds,
    /// CLI tooling cannot bypass domain, authority, idempotency, migration, or control semantics.
    CliNoInvariantBypass,
    /// Machine-readable output is versioned independently from human terminal formatting.
    CliMachineOutputVersioning,
    /// Secrets and classified sensitive values are redacted by default.
    CliSensitiveRedaction,
    /// Dangerous production actions use typed authorization and reviewed plan/apply semantics.
    CliPlanApplySafety,
    /// Timeout, cancellation, or response loss never denies a durable submission.
    CliSubmissionAmbiguity,
    /// Local inspection respects ownership, leases, and fencing.
    CliStoreOwnership,
    /// Migration application verifies immutable identity and runtime preconditions.
    CliMigrationSafety,
    /// Operational changes use registered semantic or control operations.
    CliSemanticMutation,
    /// Development-only destructive facilities are excluded or production-guarded.
    CliProductionGuard,
    /// CLI tools reuse canonical SDK and control-plane semantics.
    CliCanonicalSemantics,
    /// Production `SQLite` replicas operate in WAL mode.
    SqliteWalDurability,
    /// Exactly one logical writer owns `SQLite` synchronization mutations.
    SqliteSingleWriter,
    /// `SQLite` cursors change only with durable Tx C reconciliation.
    SqliteCursorAtomicity,
    /// `SQLite` outbox identity and payload remain durable through lifecycle completion.
    SqliteOutboxPreservation,
    /// `SQLite` and Stoolap expose identical neutral adapter behavior.
    SqliteAdapterParity,
    /// Configuration cannot disable correctness, authority, isolation, or idempotency.
    ConfigCorrectnessPreservation,
    /// Secret values remain behind redacting secret-aware abstractions.
    ConfigSecretRedaction,
    /// Only fully validated snapshots become effective configuration.
    ConfigValidatedPublication,
    /// Runtime reload publishes one coherent generation atomically.
    ConfigAtomicReload,
    /// Durable identities remain state rather than editable configuration.
    ConfigDurableIdentityIsolation,
    /// Feature flags cannot silently redefine durable semantics.
    ConfigFeatureSemanticSafety,
    /// Production rejects development-only unsafe facilities.
    ConfigProductionSafety,
    /// Adapter settings require declared certified capabilities.
    ConfigAdapterCapabilitySafety,
    /// Failed reload preserves the previous valid generation.
    ConfigFailedReloadPreservation,
    /// Client flags cannot grant entitlement or weaken server security policy.
    ConfigAuthoritativePolicy,
    /// Every artifact is bound to immutable source and build provenance.
    ReleaseTraceability,
    /// A semantic version cannot silently identify different bytes.
    ReleaseArtifactImmutability,
    /// Production artifacts are hash-checked and purpose-specifically signed.
    ReleaseSignatureIntegrity,
    /// Upgrade requires every independent compatibility dimension to pass.
    ReleaseCompatibilityGate,
    /// Client upgrade preserves all durable local synchronization intent.
    ReleaseClientIntentPreservation,
    /// Rollback requires compatible state or an explicit verified migration.
    ReleaseRollbackSafety,
    /// Channel promotion retains the exact built artifact bytes.
    ReleaseImmutablePromotion,
    /// Ordinary build jobs never possess production signing or publishing credentials.
    ReleaseCredentialIsolation,
    /// Untrusted, expired, incompatible, halted, or revoked updates fail closed.
    ReleaseUpdateFailClosed,
    /// Product `SemVer` never substitutes for protocol, store, config, or registry versions.
    ReleaseVersionDimensionSeparation,
    /// Every active authority scope has exactly one current authoritative writer timeline.
    DeploymentSingleWriter,
    /// Infrastructure topology never changes synchronization durability or ordering semantics.
    DeploymentSemanticPreservation,
    /// Ordinary process replacement never changes the authority epoch.
    DeploymentNodeEpochIndependence,
    /// Clients never receive direct authoritative database access.
    DeploymentDatabaseCredentialIsolation,
    /// Regional reads cannot accept writes and obey explicit consistency semantics.
    DeploymentRegionalReadSafety,
    /// Air-gapped operation preserves trust without public services.
    DeploymentAirGapIndependence,
    /// Infrastructure retries and duplicate workers retain operation idempotency.
    DeploymentRetrySafety,
    /// Uncertain continuity requires a new authority epoch before resuming.
    DeploymentRestoreEpoch,
    /// Correctness-critical state never exists only in process-local storage.
    DeploymentDurableStateOwnership,
    /// Core correctness is independent of optional infrastructure products.
    DeploymentInfrastructureIndependence,
}

impl InvariantId {
    /// Every required core invariant in stable identifier order.
    pub const ALL: [Self; 381] = [
        Self::IdempotentAuthority,
        Self::LocalIntentAtomicity,
        Self::AuthoritativePublicationAtomicity,
        Self::CursorSafety,
        Self::VersionMonotonicity,
        Self::NoUnauthorizedCommit,
        Self::RetryPreservation,
        Self::ReconciliationIdempotency,
        Self::TombstoneSafety,
        Self::TimelineSafety,
        Self::UniqueEventIdentity,
        Self::EventCorrelation,
        Self::RetryLineagePreservation,
        Self::DerivedLineagePreservation,
        Self::TenantLineageSafety,
        Self::CausalityAcyclic,
        Self::AntiEntropyRootAgreement,
        Self::RepairAuthoritySafety,
        Self::RepairIntentPreservation,
        Self::SameVersionDigestSafety,
        Self::IntegrityCursorIsolation,
        Self::QueueSemanticEquivalence,
        Self::DeliveredPayloadImmutability,
        Self::QueueDependencySafety,
        Self::QueueIntentPreservation,
        Self::RebaseIdentitySafety,
        Self::SensitiveOperationSafety,
        Self::LocalFenceUniqueness,
        Self::StaleLeaderCommitSafety,
        Self::FollowerMutationSafety,
        Self::LocalLeadershipLiveness,
        Self::LeadershipIntentPreservation,
        Self::StoreGenerationSafety,
        Self::QosSemanticSafety,
        Self::QosIntentPreservation,
        Self::QosBatchBounds,
        Self::QosStarvationFreedom,
        Self::QosServerHintSafety,
        Self::QosLeadershipRetrySafety,
        Self::ScopeCursorBinding,
        Self::ScopeAuthorizationSafety,
        Self::ScopeContractionSafety,
        Self::ScopeRemovalDistinction,
        Self::ScopeSharedMembershipSafety,
        Self::ScopeRevokedIntentSafety,
        Self::ScopeExpansionAtomicity,
        Self::LiveHintLossSafety,
        Self::LiveNoStateMutation,
        Self::LiveScopeIsolation,
        Self::LiveBackpressureBound,
        Self::LiveLeadershipCatchUp,
        Self::ImportIdentityDeterminism,
        Self::ImportCheckpointSafety,
        Self::ImportCutoverVerification,
        Self::ImportBaselineSafety,
        Self::ImportJournalVisibility,
        Self::ImportQuarantineSafety,
        Self::BootstrapCursorActivationSafety,
        Self::BootstrapBoundaryConsistency,
        Self::BootstrapStagingIsolation,
        Self::BootstrapChunkIdempotency,
        Self::BootstrapIntentPreservation,
        Self::BootstrapDeltaConvergence,
        Self::ProfileOperationCompatibility,
        Self::ProfileCapabilitySafety,
        Self::ProfileImmutableAppendOnly,
        Self::ProfileStrongAggregateAtomicity,
        Self::ProfileDerivedAuthoritySafety,
        Self::ProfileNoImplicitLastWriterWins,
        Self::DeterministicPlanEquivalence,
        Self::DeterministicInputCapture,
        Self::ReplaySideEffectIsolation,
        Self::ReplayCommittedInputImmutability,
        Self::ReplayHandlerVersionSafety,
        Self::ReplayPolicyVersionSafety,
        Self::AuditRequiredAtomicity,
        Self::AuditRetryDeduplication,
        Self::AuditActorTruthfulness,
        Self::AuditAppendOnlyCorrection,
        Self::AuditSensitiveValueSafety,
        Self::AuditExplanationAuthorization,
        Self::AuditChainContinuity,
        Self::AuditTamperDetection,
        Self::AuditFieldProvenanceAccuracy,
        Self::GovernanceTombstoneGcSafety,
        Self::GovernanceJournalFloorSafety,
        Self::GovernanceLegalHoldPrecedence,
        Self::GovernanceRequiredEvidenceSafety,
        Self::GovernanceSurfaceCompletion,
        Self::GovernanceOffboardingWriteFence,
        Self::GovernanceRestoreReconciliation,
        Self::GovernanceResurrectionSafety,
        Self::GovernanceCopyCoverage,
        Self::CryptoArtifactAuthenticity,
        Self::CryptoTenantBinding,
        Self::CryptoRotationHistory,
        Self::CryptoRevocationSafety,
        Self::CryptoSecretHandling,
        Self::CryptoFailClosed,
        Self::CryptoE2eSemanticBoundary,
        Self::CryptoErasureCompletion,
        Self::CryptoSignedOperationImmutability,
        Self::AuthoritySingleWriter,
        Self::AuthorityCursorEpochBinding,
        Self::AuthorityDivergenceEpoch,
        Self::AuthorityRollbackSafety,
        Self::AuthorityNoAutomaticForkMerge,
        Self::AuthorityOperationRecoveryPolicy,
        Self::AuthorityOldPrimaryFence,
        Self::AuthorityLosslessEpochContinuity,
        Self::AuthorityArtifactEpochBinding,
        Self::RegionSingleWriter,
        Self::RegionAtLeastWatermark,
        Self::RegionSessionMonotonicity,
        Self::RegionEpochIsolation,
        Self::RegionFailureAuthoritySafety,
        Self::RegionFallbackSafety,
        Self::RegionArtifactIntegrity,
        Self::RegionResidencyCoverage,
        Self::RegionGovernanceCoverage,
        Self::LoadBoundedQueues,
        Self::LoadPreMutationRejection,
        Self::LoadTenantFairness,
        Self::LoadSlowClientBound,
        Self::LoadPriorityStarvationSafety,
        Self::LoadRetryJitterSafety,
        Self::LoadDurableIntentSafety,
        Self::LoadConsistencySafety,
        Self::LoadAdmissionOrdering,
        Self::PerformanceBoundedMemory,
        Self::PerformanceStreamingLargeObjects,
        Self::PerformanceCpuIsolation,
        Self::PerformanceCorrectnessPreservation,
        Self::PerformanceImmutableHotState,
        Self::PerformanceReproducibility,
        Self::PerformancePagedUiState,
        Self::PerformanceBlobReferences,
        Self::PerformanceEarlyAdmission,
        Self::ClientIntentPreservation,
        Self::ClientCursorDurability,
        Self::ClientRequiredDirective,
        Self::ClientBoundedLargeObject,
        Self::ClientCheckpointRecovery,
        Self::ClientPendingBaseEviction,
        Self::ClientLocalCommitAtomicity,
        Self::ClientSemanticParity,
        Self::ClientTelemetryPrivacy,
        Self::CompatExplicitVersionContext,
        Self::CompatRequiredCapabilitySafety,
        Self::CompatPossiblySentImmutability,
        Self::CompatStableIdReservation,
        Self::CompatFleetActivationSafety,
        Self::CompatUpgradeIntentPreservation,
        Self::CompatRetryHorizon,
        Self::CompatAuthorityEpochIndependence,
        Self::CompatPolicyAuditability,
        Self::MetadataStoreSchemaDeclaration,
        Self::MetadataLedgerOperationIdentity,
        Self::MetadataClientCursorAtomicity,
        Self::MetadataAuthoritativeAtomicity,
        Self::MetadataSnapshotPublicationSafety,
        Self::MetadataAdapterSemanticEquivalence,
        Self::MetadataStaleFenceRejection,
        Self::MetadataMigrationIntentPreservation,
        Self::MetadataSecretKeyExclusion,
        Self::JobDurableBeforeExecution,
        Self::JobCurrentFenceOnly,
        Self::JobCommittedSideEffectIntent,
        Self::JobAuthoritativeResultOperation,
        Self::JobExplicitAmbiguityRecovery,
        Self::JobDurableState,
        Self::JobStaleWorkerRejection,
        Self::JobBoundedCheckpoint,
        Self::JobBoundedRetry,
        Self::AdminNoDomainBypass,
        Self::AdminDurableAttribution,
        Self::AdminPayloadImmutability,
        Self::AdminReviewedPlanBinding,
        Self::AdminServerAuthorization,
        Self::AdminPrivateKeyExclusion,
        Self::AdminOverrideSeparation,
        Self::AdminDataPlaneIndependence,
        Self::AdminVerifiedCompletion,
        Self::DiagnosticNonAuthoritative,
        Self::DiagnosticSecretExclusion,
        Self::DiagnosticManifestCompleteness,
        Self::DiagnosticReplaySideEffectIsolation,
        Self::DiagnosticCollectionBounds,
        Self::DiagnosticEvidenceConfidence,
        Self::DiagnosticPrePublicationSanitization,
        Self::DiagnosticVerifiedBundle,
        Self::DiagnosticReplayProductionIsolation,
        Self::LegacySingleWriteOwner,
        Self::LegacyCursorAfterDurability,
        Self::LegacyBridgeIdempotency,
        Self::LegacyPostCutoverFence,
        Self::LegacyTypedFacade,
        Self::LegacyMappingFailClosed,
        Self::LegacyShadowIsolation,
        Self::LegacyGovernanceCoverage,
        Self::LegacyVerifiedCutover,
        Self::SecurityServerValidatedClaims,
        Self::SecurityPayloadImmutability,
        Self::SecurityCapabilityFailClosed,
        Self::SecurityExternalInputBounds,
        Self::SecurityTenantIsolation,
        Self::SecuritySecretExclusion,
        Self::SecurityAuthorityRollback,
        Self::SecuritySideEffectSafety,
        Self::SecurityAdminOverride,
        Self::SecurityIntegrationDistrust,
        Self::FeedIndependentCursor,
        Self::FeedDurableEffectBeforeCursor,
        Self::FeedConsumerIsolation,
        Self::FeedDuplicateIdempotency,
        Self::FeedRetentionRecovery,
        Self::FeedExternalProjectionSafety,
        Self::FeedOrderingSafety,
        Self::FeedResetSafety,
        Self::FeedGovernanceCoverage,
        Self::RegistryIdNonReuse,
        Self::RegistryCanonicalResolution,
        Self::RegistryBreakingChangePath,
        Self::RegistryGeneratedSourceParity,
        Self::RegistryHistoricalReservation,
        Self::RegistryNamespaceIsolation,
        Self::RegistrySecurityReview,
        Self::RegistryRuntimeImmutability,
        Self::RegistryHistoricalResolution,
        Self::CertificationSemanticOnly,
        Self::CertificationTierTruthfulness,
        Self::CertificationExactBinding,
        Self::CertificationCapabilityTruthfulness,
        Self::CertificationRuntimeValidation,
        Self::CertificationArtifactImmutability,
        Self::CertificationEvidenceIntegrity,
        Self::CertificationCorrectnessPriority,
        Self::CertificationLifecycleIdentity,
        Self::MobileDurableProcessIndependence,
        Self::MobileIntentAtomicity,
        Self::MobilePushHintOnly,
        Self::MobileCursorCheckpointSafety,
        Self::MobileSecureKeyStorage,
        Self::MobileResourceSemanticSafety,
        Self::MobileUpgradeIntentSafety,
        Self::MobileStorageTruthfulness,
        Self::MobilePlatformBoundary,
        Self::DesktopSingleCoordinator,
        Self::DesktopStaleFenceSafety,
        Self::DesktopResumeIdempotency,
        Self::DesktopIpcBoundary,
        Self::DesktopCloneBindingSafety,
        Self::DesktopUpgradeIntentSafety,
        Self::DesktopModeParity,
        Self::DesktopPersistenceTruthfulness,
        Self::DesktopDerivedStateSafety,
        Self::StorageLocalIntentAtomicity,
        Self::StorageCriticalIntentRetention,
        Self::StorageCacheIsolation,
        Self::StorageCloneBindingSafety,
        Self::StorageDowngradeSafety,
        Self::StoragePlatformCertification,
        Self::StoragePublicationSafety,
        Self::StorageAdmissionTruthfulness,
        Self::StorageSecretIsolation,
        Self::StorageFilesystemSafety,
        Self::ImplementationFoundationIsolation,
        Self::ImplementationStorageTypeIsolation,
        Self::ImplementationClientUiIsolation,
        Self::ImplementationServerHttpIsolation,
        Self::ImplementationAdapterDirection,
        Self::ImplementationPlatformIsolation,
        Self::ImplementationFeatureIsolation,
        Self::ImplementationCompositionRoot,
        Self::ImplementationRegistryDeterminism,
        Self::ImplementationDependencyGraph,
        Self::SdkCursorIsolation,
        Self::SdkLocalCommitDistinction,
        Self::SdkStorageNeutrality,
        Self::SdkCancellationSafety,
        Self::SdkEventLossSafety,
        Self::SdkErrorCompatibility,
        Self::SdkClosedCoreSemantics,
        Self::SdkVersionIndependence,
        Self::SdkDangerousOperationIsolation,
        Self::SdkSemverAutomation,
        Self::AdapterCapabilityTruthfulness,
        Self::AdapterLocalAtomicity,
        Self::AdapterAuthorityAtomicity,
        Self::AdapterTypeIsolation,
        Self::AdapterPayloadBinding,
        Self::AdapterMigrationSafety,
        Self::AdapterStartupSafety,
        Self::AdapterEnvironmentBinding,
        Self::AdapterCriticalDurability,
        Self::AdapterManifestTruthfulness,
        Self::PostgresAuthorityAtomicity,
        Self::PostgresIdempotentReplay,
        Self::PostgresPayloadBinding,
        Self::PostgresCommittedTimeline,
        Self::PostgresRestoreEpoch,
        Self::PostgresTypeIsolation,
        Self::PostgresReadinessSafety,
        Self::PostgresSideEffectIntent,
        Self::PostgresRetentionSafety,
        Self::PostgresNeonSemanticParity,
        Self::StoolapLocalAtomicity,
        Self::StoolapReconciliationAtomicity,
        Self::StoolapRetryIdentity,
        Self::StoolapIntentPreservation,
        Self::StoolapCursorSafety,
        Self::StoolapFencedCoordinator,
        Self::StoolapCloneSafety,
        Self::StoolapStoragePressureSafety,
        Self::StoolapTypeIsolation,
        Self::StoolapPlatformCertification,
        Self::AxumTransportIsolation,
        Self::AxumCredentialIsolation,
        Self::AxumResourceBounds,
        Self::AxumCommitDeliveryIndependence,
        Self::AxumOverloadBounds,
        Self::AxumIdentityBinding,
        Self::AxumStableSemantics,
        Self::AxumLiveHintDurability,
        Self::AxumNodeEpochIndependence,
        Self::AxumErrorSanitization,
        Self::DioxusDurableStateOwnership,
        Self::DioxusLocalCommitTruth,
        Self::DioxusAuthorityDistinction,
        Self::DioxusEventLossSafety,
        Self::DioxusUnmountSafety,
        Self::DioxusStoreIsolation,
        Self::DioxusConflictSemantics,
        Self::DioxusOfflineCapability,
        Self::DioxusBackgroundOwnership,
        Self::DioxusEventBounds,
        Self::CliNoInvariantBypass,
        Self::CliMachineOutputVersioning,
        Self::CliSensitiveRedaction,
        Self::CliPlanApplySafety,
        Self::CliSubmissionAmbiguity,
        Self::CliStoreOwnership,
        Self::CliMigrationSafety,
        Self::CliSemanticMutation,
        Self::CliProductionGuard,
        Self::CliCanonicalSemantics,
        Self::SqliteWalDurability,
        Self::SqliteSingleWriter,
        Self::SqliteCursorAtomicity,
        Self::SqliteOutboxPreservation,
        Self::SqliteAdapterParity,
        Self::ConfigCorrectnessPreservation,
        Self::ConfigSecretRedaction,
        Self::ConfigValidatedPublication,
        Self::ConfigAtomicReload,
        Self::ConfigDurableIdentityIsolation,
        Self::ConfigFeatureSemanticSafety,
        Self::ConfigProductionSafety,
        Self::ConfigAdapterCapabilitySafety,
        Self::ConfigFailedReloadPreservation,
        Self::ConfigAuthoritativePolicy,
        Self::ReleaseTraceability,
        Self::ReleaseArtifactImmutability,
        Self::ReleaseSignatureIntegrity,
        Self::ReleaseCompatibilityGate,
        Self::ReleaseClientIntentPreservation,
        Self::ReleaseRollbackSafety,
        Self::ReleaseImmutablePromotion,
        Self::ReleaseCredentialIsolation,
        Self::ReleaseUpdateFailClosed,
        Self::ReleaseVersionDimensionSeparation,
        Self::DeploymentSingleWriter,
        Self::DeploymentSemanticPreservation,
        Self::DeploymentNodeEpochIndependence,
        Self::DeploymentDatabaseCredentialIsolation,
        Self::DeploymentRegionalReadSafety,
        Self::DeploymentAirGapIndependence,
        Self::DeploymentRetrySafety,
        Self::DeploymentRestoreEpoch,
        Self::DeploymentDurableStateOwnership,
        Self::DeploymentInfrastructureIndependence,
    ];

    /// Stable external identifier used by traces, tests, diagnostics, and documentation.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdempotentAuthority => "AEQ-INV-001",
            Self::LocalIntentAtomicity => "AEQ-INV-002",
            Self::AuthoritativePublicationAtomicity => "AEQ-INV-003",
            Self::CursorSafety => "AEQ-INV-004",
            Self::VersionMonotonicity => "AEQ-INV-005",
            Self::NoUnauthorizedCommit => "AEQ-INV-006",
            Self::RetryPreservation => "AEQ-INV-007",
            Self::ReconciliationIdempotency => "AEQ-INV-008",
            Self::TombstoneSafety => "AEQ-INV-009",
            Self::TimelineSafety => "AEQ-INV-010",
            Self::UniqueEventIdentity => "AEQ-INV-C001",
            Self::EventCorrelation => "AEQ-INV-C002",
            Self::RetryLineagePreservation => "AEQ-INV-C003",
            Self::DerivedLineagePreservation => "AEQ-INV-C004",
            Self::TenantLineageSafety => "AEQ-INV-C005",
            Self::CausalityAcyclic => "AEQ-INV-C006",
            Self::AntiEntropyRootAgreement => "AEQ-INV-AE001",
            Self::RepairAuthoritySafety => "AEQ-INV-AE002",
            Self::RepairIntentPreservation => "AEQ-INV-AE003",
            Self::SameVersionDigestSafety => "AEQ-INV-AE004",
            Self::IntegrityCursorIsolation => "AEQ-INV-AE005",
            Self::QueueSemanticEquivalence => "AEQ-INV-OQ001",
            Self::DeliveredPayloadImmutability => "AEQ-INV-OQ002",
            Self::QueueDependencySafety => "AEQ-INV-OQ003",
            Self::QueueIntentPreservation => "AEQ-INV-OQ004",
            Self::RebaseIdentitySafety => "AEQ-INV-OQ005",
            Self::SensitiveOperationSafety => "AEQ-INV-OQ006",
            Self::LocalFenceUniqueness => "AEQ-INV-LC001",
            Self::StaleLeaderCommitSafety => "AEQ-INV-LC002",
            Self::FollowerMutationSafety => "AEQ-INV-LC003",
            Self::LocalLeadershipLiveness => "AEQ-INV-LC004",
            Self::LeadershipIntentPreservation => "AEQ-INV-LC005",
            Self::StoreGenerationSafety => "AEQ-INV-LC006",
            Self::QosSemanticSafety => "AEQ-INV-QOS001",
            Self::QosIntentPreservation => "AEQ-INV-QOS002",
            Self::QosBatchBounds => "AEQ-INV-QOS003",
            Self::QosStarvationFreedom => "AEQ-INV-QOS004",
            Self::QosServerHintSafety => "AEQ-INV-QOS005",
            Self::QosLeadershipRetrySafety => "AEQ-INV-QOS006",
            Self::ScopeCursorBinding => "AEQ-INV-SCP001",
            Self::ScopeAuthorizationSafety => "AEQ-INV-SCP002",
            Self::ScopeContractionSafety => "AEQ-INV-SCP003",
            Self::ScopeRemovalDistinction => "AEQ-INV-SCP004",
            Self::ScopeSharedMembershipSafety => "AEQ-INV-SCP005",
            Self::ScopeRevokedIntentSafety => "AEQ-INV-SCP006",
            Self::ScopeExpansionAtomicity => "AEQ-INV-SCP007",
            Self::LiveHintLossSafety => "AEQ-INV-LIVE001",
            Self::LiveNoStateMutation => "AEQ-INV-LIVE002",
            Self::LiveScopeIsolation => "AEQ-INV-LIVE003",
            Self::LiveBackpressureBound => "AEQ-INV-LIVE004",
            Self::LiveLeadershipCatchUp => "AEQ-INV-LIVE005",
            Self::ImportIdentityDeterminism => "AEQ-INV-IMP001",
            Self::ImportCheckpointSafety => "AEQ-INV-IMP002",
            Self::ImportCutoverVerification => "AEQ-INV-IMP003",
            Self::ImportBaselineSafety => "AEQ-INV-IMP004",
            Self::ImportJournalVisibility => "AEQ-INV-IMP005",
            Self::ImportQuarantineSafety => "AEQ-INV-IMP006",
            Self::BootstrapCursorActivationSafety => "AEQ-INV-BS001",
            Self::BootstrapBoundaryConsistency => "AEQ-INV-BS002",
            Self::BootstrapStagingIsolation => "AEQ-INV-BS003",
            Self::BootstrapChunkIdempotency => "AEQ-INV-BS004",
            Self::BootstrapIntentPreservation => "AEQ-INV-BS005",
            Self::BootstrapDeltaConvergence => "AEQ-INV-BS006",
            Self::ProfileOperationCompatibility => "AEQ-INV-PROF001",
            Self::ProfileCapabilitySafety => "AEQ-INV-PROF002",
            Self::ProfileImmutableAppendOnly => "AEQ-INV-PROF003",
            Self::ProfileStrongAggregateAtomicity => "AEQ-INV-PROF004",
            Self::ProfileDerivedAuthoritySafety => "AEQ-INV-PROF005",
            Self::ProfileNoImplicitLastWriterWins => "AEQ-INV-PROF006",
            Self::DeterministicPlanEquivalence => "AEQ-INV-DET001",
            Self::DeterministicInputCapture => "AEQ-INV-DET002",
            Self::ReplaySideEffectIsolation => "AEQ-INV-DET003",
            Self::ReplayCommittedInputImmutability => "AEQ-INV-DET004",
            Self::ReplayHandlerVersionSafety => "AEQ-INV-DET005",
            Self::ReplayPolicyVersionSafety => "AEQ-INV-DET006",
            Self::AuditRequiredAtomicity => "AEQ-INV-AUD001",
            Self::AuditRetryDeduplication => "AEQ-INV-AUD002",
            Self::AuditActorTruthfulness => "AEQ-INV-AUD003",
            Self::AuditAppendOnlyCorrection => "AEQ-INV-AUD004",
            Self::AuditSensitiveValueSafety => "AEQ-INV-AUD005",
            Self::AuditExplanationAuthorization => "AEQ-INV-AUD006",
            Self::AuditChainContinuity => "AEQ-INV-AUD007",
            Self::AuditTamperDetection => "AEQ-INV-AUD008",
            Self::AuditFieldProvenanceAccuracy => "AEQ-INV-AUD009",
            Self::GovernanceTombstoneGcSafety => "AEQ-INV-GOV001",
            Self::GovernanceJournalFloorSafety => "AEQ-INV-GOV002",
            Self::GovernanceLegalHoldPrecedence => "AEQ-INV-GOV003",
            Self::GovernanceRequiredEvidenceSafety => "AEQ-INV-GOV004",
            Self::GovernanceSurfaceCompletion => "AEQ-INV-GOV005",
            Self::GovernanceOffboardingWriteFence => "AEQ-INV-GOV006",
            Self::GovernanceRestoreReconciliation => "AEQ-INV-GOV007",
            Self::GovernanceResurrectionSafety => "AEQ-INV-GOV008",
            Self::GovernanceCopyCoverage => "AEQ-INV-GOV009",
            Self::CryptoArtifactAuthenticity => "AEQ-INV-CRYPTO001",
            Self::CryptoTenantBinding => "AEQ-INV-CRYPTO002",
            Self::CryptoRotationHistory => "AEQ-INV-CRYPTO003",
            Self::CryptoRevocationSafety => "AEQ-INV-CRYPTO004",
            Self::CryptoSecretHandling => "AEQ-INV-CRYPTO005",
            Self::CryptoFailClosed => "AEQ-INV-CRYPTO006",
            Self::CryptoE2eSemanticBoundary => "AEQ-INV-CRYPTO007",
            Self::CryptoErasureCompletion => "AEQ-INV-CRYPTO008",
            Self::CryptoSignedOperationImmutability => "AEQ-INV-CRYPTO009",
            Self::AuthoritySingleWriter => "AEQ-INV-AUTH001",
            Self::AuthorityCursorEpochBinding => "AEQ-INV-AUTH002",
            Self::AuthorityDivergenceEpoch => "AEQ-INV-AUTH003",
            Self::AuthorityRollbackSafety => "AEQ-INV-AUTH004",
            Self::AuthorityNoAutomaticForkMerge => "AEQ-INV-AUTH005",
            Self::AuthorityOperationRecoveryPolicy => "AEQ-INV-AUTH006",
            Self::AuthorityOldPrimaryFence => "AEQ-INV-AUTH007",
            Self::AuthorityLosslessEpochContinuity => "AEQ-INV-AUTH008",
            Self::AuthorityArtifactEpochBinding => "AEQ-INV-AUTH009",
            Self::RegionSingleWriter => "AEQ-INV-REG001",
            Self::RegionAtLeastWatermark => "AEQ-INV-REG002",
            Self::RegionSessionMonotonicity => "AEQ-INV-REG003",
            Self::RegionEpochIsolation => "AEQ-INV-REG004",
            Self::RegionFailureAuthoritySafety => "AEQ-INV-REG005",
            Self::RegionFallbackSafety => "AEQ-INV-REG006",
            Self::RegionArtifactIntegrity => "AEQ-INV-REG007",
            Self::RegionResidencyCoverage => "AEQ-INV-REG008",
            Self::RegionGovernanceCoverage => "AEQ-INV-REG009",
            Self::LoadBoundedQueues => "AEQ-INV-LOAD001",
            Self::LoadPreMutationRejection => "AEQ-INV-LOAD002",
            Self::LoadTenantFairness => "AEQ-INV-LOAD003",
            Self::LoadSlowClientBound => "AEQ-INV-LOAD004",
            Self::LoadPriorityStarvationSafety => "AEQ-INV-LOAD005",
            Self::LoadRetryJitterSafety => "AEQ-INV-LOAD006",
            Self::LoadDurableIntentSafety => "AEQ-INV-LOAD007",
            Self::LoadConsistencySafety => "AEQ-INV-LOAD008",
            Self::LoadAdmissionOrdering => "AEQ-INV-LOAD009",
            Self::PerformanceBoundedMemory => "AEQ-INV-PERF001",
            Self::PerformanceStreamingLargeObjects => "AEQ-INV-PERF002",
            Self::PerformanceCpuIsolation => "AEQ-INV-PERF003",
            Self::PerformanceCorrectnessPreservation => "AEQ-INV-PERF004",
            Self::PerformanceImmutableHotState => "AEQ-INV-PERF005",
            Self::PerformanceReproducibility => "AEQ-INV-PERF006",
            Self::PerformancePagedUiState => "AEQ-INV-PERF007",
            Self::PerformanceBlobReferences => "AEQ-INV-PERF008",
            Self::PerformanceEarlyAdmission => "AEQ-INV-PERF009",
            Self::ClientIntentPreservation => "AEQ-INV-CLIENT001",
            Self::ClientCursorDurability => "AEQ-INV-CLIENT002",
            Self::ClientRequiredDirective => "AEQ-INV-CLIENT003",
            Self::ClientBoundedLargeObject => "AEQ-INV-CLIENT004",
            Self::ClientCheckpointRecovery => "AEQ-INV-CLIENT005",
            Self::ClientPendingBaseEviction => "AEQ-INV-CLIENT006",
            Self::ClientLocalCommitAtomicity => "AEQ-INV-CLIENT007",
            Self::ClientSemanticParity => "AEQ-INV-CLIENT008",
            Self::ClientTelemetryPrivacy => "AEQ-INV-CLIENT009",
            Self::CompatExplicitVersionContext => "AEQ-INV-COMP001",
            Self::CompatRequiredCapabilitySafety => "AEQ-INV-COMP002",
            Self::CompatPossiblySentImmutability => "AEQ-INV-COMP003",
            Self::CompatStableIdReservation => "AEQ-INV-COMP004",
            Self::CompatFleetActivationSafety => "AEQ-INV-COMP005",
            Self::CompatUpgradeIntentPreservation => "AEQ-INV-COMP006",
            Self::CompatRetryHorizon => "AEQ-INV-COMP007",
            Self::CompatAuthorityEpochIndependence => "AEQ-INV-COMP008",
            Self::CompatPolicyAuditability => "AEQ-INV-COMP009",
            Self::MetadataStoreSchemaDeclaration => "AEQ-INV-META001",
            Self::MetadataLedgerOperationIdentity => "AEQ-INV-META002",
            Self::MetadataClientCursorAtomicity => "AEQ-INV-META003",
            Self::MetadataAuthoritativeAtomicity => "AEQ-INV-META004",
            Self::MetadataSnapshotPublicationSafety => "AEQ-INV-META005",
            Self::MetadataAdapterSemanticEquivalence => "AEQ-INV-META006",
            Self::MetadataStaleFenceRejection => "AEQ-INV-META007",
            Self::MetadataMigrationIntentPreservation => "AEQ-INV-META008",
            Self::MetadataSecretKeyExclusion => "AEQ-INV-META009",
            Self::JobDurableBeforeExecution => "AEQ-INV-JOB001",
            Self::JobCurrentFenceOnly => "AEQ-INV-JOB002",
            Self::JobCommittedSideEffectIntent => "AEQ-INV-JOB003",
            Self::JobAuthoritativeResultOperation => "AEQ-INV-JOB004",
            Self::JobExplicitAmbiguityRecovery => "AEQ-INV-JOB005",
            Self::JobDurableState => "AEQ-INV-JOB006",
            Self::JobStaleWorkerRejection => "AEQ-INV-JOB007",
            Self::JobBoundedCheckpoint => "AEQ-INV-JOB008",
            Self::JobBoundedRetry => "AEQ-INV-JOB009",
            Self::AdminNoDomainBypass => "AEQ-INV-ADMIN001",
            Self::AdminDurableAttribution => "AEQ-INV-ADMIN002",
            Self::AdminPayloadImmutability => "AEQ-INV-ADMIN003",
            Self::AdminReviewedPlanBinding => "AEQ-INV-ADMIN004",
            Self::AdminServerAuthorization => "AEQ-INV-ADMIN005",
            Self::AdminPrivateKeyExclusion => "AEQ-INV-ADMIN006",
            Self::AdminOverrideSeparation => "AEQ-INV-ADMIN007",
            Self::AdminDataPlaneIndependence => "AEQ-INV-ADMIN008",
            Self::AdminVerifiedCompletion => "AEQ-INV-ADMIN009",
            Self::DiagnosticNonAuthoritative => "AEQ-INV-DIAG001",
            Self::DiagnosticSecretExclusion => "AEQ-INV-DIAG002",
            Self::DiagnosticManifestCompleteness => "AEQ-INV-DIAG003",
            Self::DiagnosticReplaySideEffectIsolation => "AEQ-INV-DIAG004",
            Self::DiagnosticCollectionBounds => "AEQ-INV-DIAG005",
            Self::DiagnosticEvidenceConfidence => "AEQ-INV-DIAG006",
            Self::DiagnosticPrePublicationSanitization => "AEQ-INV-DIAG007",
            Self::DiagnosticVerifiedBundle => "AEQ-INV-DIAG008",
            Self::DiagnosticReplayProductionIsolation => "AEQ-INV-DIAG009",
            Self::LegacySingleWriteOwner => "AEQ-INV-LEG001",
            Self::LegacyCursorAfterDurability => "AEQ-INV-LEG002",
            Self::LegacyBridgeIdempotency => "AEQ-INV-LEG003",
            Self::LegacyPostCutoverFence => "AEQ-INV-LEG004",
            Self::LegacyTypedFacade => "AEQ-INV-LEG005",
            Self::LegacyMappingFailClosed => "AEQ-INV-LEG006",
            Self::LegacyShadowIsolation => "AEQ-INV-LEG007",
            Self::LegacyGovernanceCoverage => "AEQ-INV-LEG008",
            Self::LegacyVerifiedCutover => "AEQ-INV-LEG009",
            Self::SecurityServerValidatedClaims => "AEQ-INV-SEC001",
            Self::SecurityPayloadImmutability => "AEQ-INV-SEC002",
            Self::SecurityCapabilityFailClosed => "AEQ-INV-SEC003",
            Self::SecurityExternalInputBounds => "AEQ-INV-SEC004",
            Self::SecurityTenantIsolation => "AEQ-INV-SEC005",
            Self::SecuritySecretExclusion => "AEQ-INV-SEC006",
            Self::SecurityAuthorityRollback => "AEQ-INV-SEC007",
            Self::SecuritySideEffectSafety => "AEQ-INV-SEC008",
            Self::SecurityAdminOverride => "AEQ-INV-SEC009",
            Self::SecurityIntegrationDistrust => "AEQ-INV-SEC010",
            Self::FeedIndependentCursor => "AEQ-INV-FEED001",
            Self::FeedDurableEffectBeforeCursor => "AEQ-INV-FEED002",
            Self::FeedConsumerIsolation => "AEQ-INV-FEED003",
            Self::FeedDuplicateIdempotency => "AEQ-INV-FEED004",
            Self::FeedRetentionRecovery => "AEQ-INV-FEED005",
            Self::FeedExternalProjectionSafety => "AEQ-INV-FEED006",
            Self::FeedOrderingSafety => "AEQ-INV-FEED007",
            Self::FeedResetSafety => "AEQ-INV-FEED008",
            Self::FeedGovernanceCoverage => "AEQ-INV-FEED009",
            Self::RegistryIdNonReuse => "AEQ-INV-REGISTRY001",
            Self::RegistryCanonicalResolution => "AEQ-INV-REGISTRY002",
            Self::RegistryBreakingChangePath => "AEQ-INV-REGISTRY003",
            Self::RegistryGeneratedSourceParity => "AEQ-INV-REGISTRY004",
            Self::RegistryHistoricalReservation => "AEQ-INV-REGISTRY005",
            Self::RegistryNamespaceIsolation => "AEQ-INV-REGISTRY006",
            Self::RegistrySecurityReview => "AEQ-INV-REGISTRY007",
            Self::RegistryRuntimeImmutability => "AEQ-INV-REGISTRY008",
            Self::RegistryHistoricalResolution => "AEQ-INV-REGISTRY009",
            Self::CertificationSemanticOnly => "AEQ-INV-CERT001",
            Self::CertificationTierTruthfulness => "AEQ-INV-CERT002",
            Self::CertificationExactBinding => "AEQ-INV-CERT003",
            Self::CertificationCapabilityTruthfulness => "AEQ-INV-CERT004",
            Self::CertificationRuntimeValidation => "AEQ-INV-CERT005",
            Self::CertificationArtifactImmutability => "AEQ-INV-CERT006",
            Self::CertificationEvidenceIntegrity => "AEQ-INV-CERT007",
            Self::CertificationCorrectnessPriority => "AEQ-INV-CERT008",
            Self::CertificationLifecycleIdentity => "AEQ-INV-CERT009",
            Self::MobileDurableProcessIndependence => "AEQ-INV-MOBILE001",
            Self::MobileIntentAtomicity => "AEQ-INV-MOBILE002",
            Self::MobilePushHintOnly => "AEQ-INV-MOBILE003",
            Self::MobileCursorCheckpointSafety => "AEQ-INV-MOBILE004",
            Self::MobileSecureKeyStorage => "AEQ-INV-MOBILE005",
            Self::MobileResourceSemanticSafety => "AEQ-INV-MOBILE006",
            Self::MobileUpgradeIntentSafety => "AEQ-INV-MOBILE007",
            Self::MobileStorageTruthfulness => "AEQ-INV-MOBILE008",
            Self::MobilePlatformBoundary => "AEQ-INV-MOBILE009",
            Self::DesktopSingleCoordinator => "AEQ-INV-DESKTOP001",
            Self::DesktopStaleFenceSafety => "AEQ-INV-DESKTOP002",
            Self::DesktopResumeIdempotency => "AEQ-INV-DESKTOP003",
            Self::DesktopIpcBoundary => "AEQ-INV-DESKTOP004",
            Self::DesktopCloneBindingSafety => "AEQ-INV-DESKTOP005",
            Self::DesktopUpgradeIntentSafety => "AEQ-INV-DESKTOP006",
            Self::DesktopModeParity => "AEQ-INV-DESKTOP007",
            Self::DesktopPersistenceTruthfulness => "AEQ-INV-DESKTOP008",
            Self::DesktopDerivedStateSafety => "AEQ-INV-DESKTOP009",
            Self::StorageLocalIntentAtomicity => "AEQ-INV-STORAGE001",
            Self::StorageCriticalIntentRetention => "AEQ-INV-STORAGE002",
            Self::StorageCacheIsolation => "AEQ-INV-STORAGE003",
            Self::StorageCloneBindingSafety => "AEQ-INV-STORAGE004",
            Self::StorageDowngradeSafety => "AEQ-INV-STORAGE005",
            Self::StoragePlatformCertification => "AEQ-INV-STORAGE006",
            Self::StoragePublicationSafety => "AEQ-INV-STORAGE007",
            Self::StorageAdmissionTruthfulness => "AEQ-INV-STORAGE008",
            Self::StorageSecretIsolation => "AEQ-INV-STORAGE009",
            Self::StorageFilesystemSafety => "AEQ-INV-STORAGE010",
            Self::ImplementationFoundationIsolation => "AEQ-INV-IMPL001",
            Self::ImplementationStorageTypeIsolation => "AEQ-INV-IMPL002",
            Self::ImplementationClientUiIsolation => "AEQ-INV-IMPL003",
            Self::ImplementationServerHttpIsolation => "AEQ-INV-IMPL004",
            Self::ImplementationAdapterDirection => "AEQ-INV-IMPL005",
            Self::ImplementationPlatformIsolation => "AEQ-INV-IMPL006",
            Self::ImplementationFeatureIsolation => "AEQ-INV-IMPL007",
            Self::ImplementationCompositionRoot => "AEQ-INV-IMPL008",
            Self::ImplementationRegistryDeterminism => "AEQ-INV-IMPL009",
            Self::ImplementationDependencyGraph => "AEQ-INV-IMPL010",
            Self::SdkCursorIsolation => "AEQ-INV-SDK001",
            Self::SdkLocalCommitDistinction => "AEQ-INV-SDK002",
            Self::SdkStorageNeutrality => "AEQ-INV-SDK003",
            Self::SdkCancellationSafety => "AEQ-INV-SDK004",
            Self::SdkEventLossSafety => "AEQ-INV-SDK005",
            Self::SdkErrorCompatibility => "AEQ-INV-SDK006",
            Self::SdkClosedCoreSemantics => "AEQ-INV-SDK007",
            Self::SdkVersionIndependence => "AEQ-INV-SDK008",
            Self::SdkDangerousOperationIsolation => "AEQ-INV-SDK009",
            Self::SdkSemverAutomation => "AEQ-INV-SDK010",
            Self::AdapterCapabilityTruthfulness => "AEQ-INV-ADAPTER001",
            Self::AdapterLocalAtomicity => "AEQ-INV-ADAPTER002",
            Self::AdapterAuthorityAtomicity => "AEQ-INV-ADAPTER003",
            Self::AdapterTypeIsolation => "AEQ-INV-ADAPTER004",
            Self::AdapterPayloadBinding => "AEQ-INV-ADAPTER005",
            Self::AdapterMigrationSafety => "AEQ-INV-ADAPTER006",
            Self::AdapterStartupSafety => "AEQ-INV-ADAPTER007",
            Self::AdapterEnvironmentBinding => "AEQ-INV-ADAPTER008",
            Self::AdapterCriticalDurability => "AEQ-INV-ADAPTER009",
            Self::AdapterManifestTruthfulness => "AEQ-INV-ADAPTER010",
            Self::PostgresAuthorityAtomicity => "AEQ-INV-PG001",
            Self::PostgresIdempotentReplay => "AEQ-INV-PG002",
            Self::PostgresPayloadBinding => "AEQ-INV-PG003",
            Self::PostgresCommittedTimeline => "AEQ-INV-PG004",
            Self::PostgresRestoreEpoch => "AEQ-INV-PG005",
            Self::PostgresTypeIsolation => "AEQ-INV-PG006",
            Self::PostgresReadinessSafety => "AEQ-INV-PG007",
            Self::PostgresSideEffectIntent => "AEQ-INV-PG008",
            Self::PostgresRetentionSafety => "AEQ-INV-PG009",
            Self::PostgresNeonSemanticParity => "AEQ-INV-PG010",
            Self::StoolapLocalAtomicity => "AEQ-INV-STOOLAP001",
            Self::StoolapReconciliationAtomicity => "AEQ-INV-STOOLAP002",
            Self::StoolapRetryIdentity => "AEQ-INV-STOOLAP003",
            Self::StoolapIntentPreservation => "AEQ-INV-STOOLAP004",
            Self::StoolapCursorSafety => "AEQ-INV-STOOLAP005",
            Self::StoolapFencedCoordinator => "AEQ-INV-STOOLAP006",
            Self::StoolapCloneSafety => "AEQ-INV-STOOLAP007",
            Self::StoolapStoragePressureSafety => "AEQ-INV-STOOLAP008",
            Self::StoolapTypeIsolation => "AEQ-INV-STOOLAP009",
            Self::StoolapPlatformCertification => "AEQ-INV-STOOLAP010",
            Self::AxumTransportIsolation => "AEQ-INV-AXUM001",
            Self::AxumCredentialIsolation => "AEQ-INV-AXUM002",
            Self::AxumResourceBounds => "AEQ-INV-AXUM003",
            Self::AxumCommitDeliveryIndependence => "AEQ-INV-AXUM004",
            Self::AxumOverloadBounds => "AEQ-INV-AXUM005",
            Self::AxumIdentityBinding => "AEQ-INV-AXUM006",
            Self::AxumStableSemantics => "AEQ-INV-AXUM007",
            Self::AxumLiveHintDurability => "AEQ-INV-AXUM008",
            Self::AxumNodeEpochIndependence => "AEQ-INV-AXUM009",
            Self::AxumErrorSanitization => "AEQ-INV-AXUM010",
            Self::DioxusDurableStateOwnership => "AEQ-INV-DIOXUS001",
            Self::DioxusLocalCommitTruth => "AEQ-INV-DIOXUS002",
            Self::DioxusAuthorityDistinction => "AEQ-INV-DIOXUS003",
            Self::DioxusEventLossSafety => "AEQ-INV-DIOXUS004",
            Self::DioxusUnmountSafety => "AEQ-INV-DIOXUS005",
            Self::DioxusStoreIsolation => "AEQ-INV-DIOXUS006",
            Self::DioxusConflictSemantics => "AEQ-INV-DIOXUS007",
            Self::DioxusOfflineCapability => "AEQ-INV-DIOXUS008",
            Self::DioxusBackgroundOwnership => "AEQ-INV-DIOXUS009",
            Self::DioxusEventBounds => "AEQ-INV-DIOXUS010",
            Self::CliNoInvariantBypass => "AEQ-INV-CLI001",
            Self::CliMachineOutputVersioning => "AEQ-INV-CLI002",
            Self::CliSensitiveRedaction => "AEQ-INV-CLI003",
            Self::CliPlanApplySafety => "AEQ-INV-CLI004",
            Self::CliSubmissionAmbiguity => "AEQ-INV-CLI005",
            Self::CliStoreOwnership => "AEQ-INV-CLI006",
            Self::CliMigrationSafety => "AEQ-INV-CLI007",
            Self::CliSemanticMutation => "AEQ-INV-CLI008",
            Self::CliProductionGuard => "AEQ-INV-CLI009",
            Self::CliCanonicalSemantics => "AEQ-INV-CLI010",
            Self::SqliteWalDurability => "AEQ-INV-SQLITE001",
            Self::SqliteSingleWriter => "AEQ-INV-SQLITE002",
            Self::SqliteCursorAtomicity => "AEQ-INV-SQLITE003",
            Self::SqliteOutboxPreservation => "AEQ-INV-SQLITE004",
            Self::SqliteAdapterParity => "AEQ-INV-SQLITE005",
            Self::ConfigCorrectnessPreservation => "AEQ-INV-CONFIG001",
            Self::ConfigSecretRedaction => "AEQ-INV-CONFIG002",
            Self::ConfigValidatedPublication => "AEQ-INV-CONFIG003",
            Self::ConfigAtomicReload => "AEQ-INV-CONFIG004",
            Self::ConfigDurableIdentityIsolation => "AEQ-INV-CONFIG005",
            Self::ConfigFeatureSemanticSafety => "AEQ-INV-CONFIG006",
            Self::ConfigProductionSafety => "AEQ-INV-CONFIG007",
            Self::ConfigAdapterCapabilitySafety => "AEQ-INV-CONFIG008",
            Self::ConfigFailedReloadPreservation => "AEQ-INV-CONFIG009",
            Self::ConfigAuthoritativePolicy => "AEQ-INV-CONFIG010",
            Self::ReleaseTraceability => "AEQ-INV-RELEASE001",
            Self::ReleaseArtifactImmutability => "AEQ-INV-RELEASE002",
            Self::ReleaseSignatureIntegrity => "AEQ-INV-RELEASE003",
            Self::ReleaseCompatibilityGate => "AEQ-INV-RELEASE004",
            Self::ReleaseClientIntentPreservation => "AEQ-INV-RELEASE005",
            Self::ReleaseRollbackSafety => "AEQ-INV-RELEASE006",
            Self::ReleaseImmutablePromotion => "AEQ-INV-RELEASE007",
            Self::ReleaseCredentialIsolation => "AEQ-INV-RELEASE008",
            Self::ReleaseUpdateFailClosed => "AEQ-INV-RELEASE009",
            Self::ReleaseVersionDimensionSeparation => "AEQ-INV-RELEASE010",
            Self::DeploymentSingleWriter => "AEQ-INV-DEPLOY001",
            Self::DeploymentSemanticPreservation => "AEQ-INV-DEPLOY002",
            Self::DeploymentNodeEpochIndependence => "AEQ-INV-DEPLOY003",
            Self::DeploymentDatabaseCredentialIsolation => "AEQ-INV-DEPLOY004",
            Self::DeploymentRegionalReadSafety => "AEQ-INV-DEPLOY005",
            Self::DeploymentAirGapIndependence => "AEQ-INV-DEPLOY006",
            Self::DeploymentRetrySafety => "AEQ-INV-DEPLOY007",
            Self::DeploymentRestoreEpoch => "AEQ-INV-DEPLOY008",
            Self::DeploymentDurableStateOwnership => "AEQ-INV-DEPLOY009",
            Self::DeploymentInfrastructureIndependence => "AEQ-INV-DEPLOY010",
        }
    }

    /// Normative, human-readable safety statement.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub const fn description(self) -> &'static str {
        match self {
            Self::IdempotentAuthority => {
                "one OperationId produces at most one authoritative logical effect"
            }
            Self::LocalIntentAtomicity => {
                "a committed local synchronizable mutation exists exactly with durable outbox intent"
            }
            Self::AuthoritativePublicationAtomicity => {
                "accepted authority state, journal event, and operation result commit atomically"
            }
            Self::CursorSafety => {
                "cursor N means every required event through N is durably applied locally"
            }
            Self::VersionMonotonicity => "authoritative entity versions never decrease",
            Self::NoUnauthorizedCommit => "an unauthorized operation cannot become authoritative",
            Self::RetryPreservation => "retry cannot create an additional logical effect",
            Self::ReconciliationIdempotency => {
                "reapplying an authoritative event cannot duplicate its logical effect"
            }
            Self::TombstoneSafety => "a stale update cannot silently resurrect a deleted entity",
            Self::TimelineSafety => {
                "a cursor from one authority epoch is invalid in an incompatible epoch"
            }
            Self::UniqueEventIdentity => "every authoritative event has one stable EventId",
            Self::EventCorrelation => {
                "every authoritative event retains exactly one originating correlation chain"
            }
            Self::RetryLineagePreservation => {
                "retrying an OperationId cannot change its original lineage"
            }
            Self::DerivedLineagePreservation => {
                "derived events and jobs inherit their parent's correlation chain"
            }
            Self::TenantLineageSafety => {
                "lineage persistence and correlation lookup cannot cross tenant boundaries"
            }
            Self::CausalityAcyclic => "direct causal ancestry cannot contain a cycle",
            Self::AntiEntropyRootAgreement => {
                "matching compatible roots detect no synchronized-state divergence"
            }
            Self::RepairAuthoritySafety => {
                "replica repair never mutates authoritative state from client replica data"
            }
            Self::RepairIntentPreservation => {
                "replica repair preserves exact pending operation intent"
            }
            Self::SameVersionDigestSafety => {
                "same-version canonical digest mismatch is an integrity anomaly"
            }
            Self::IntegrityCursorIsolation => {
                "integrity verification never advances the normal synchronization cursor"
            }
            Self::QueueSemanticEquivalence => {
                "declared compaction preserves observable application semantics"
            }
            Self::DeliveredPayloadImmutability => {
                "a possibly delivered OperationId retains one immutable semantic envelope"
            }
            Self::QueueDependencySafety => {
                "compaction cannot remove an operation required by an active dependency"
            }
            Self::QueueIntentPreservation => {
                "storage optimization alone cannot discard pending user intent"
            }
            Self::RebaseIdentitySafety => {
                "rebase retains OperationId only when semantic intent is unchanged"
            }
            Self::SensitiveOperationSafety => {
                "financial and audit-sensitive operations require explicit compaction certification"
            }
            Self::LocalFenceUniqueness => {
                "at most one fencing token is current for one persistent local store"
            }
            Self::StaleLeaderCommitSafety => {
                "a stale leader cannot commit fenced reconciliation or bootstrap state"
            }
            Self::FollowerMutationSafety => {
                "follower domain mutation and outbox append remain valid without leadership"
            }
            Self::LocalLeadershipLiveness => {
                "an eligible follower can acquire leadership after failure and lease expiry"
            }
            Self::LeadershipIntentPreservation => {
                "leadership changes preserve pending operation identity and semantic payload"
            }
            Self::StoreGenerationSafety => {
                "a store-generation change invalidates stale coordinator assumptions"
            }
            Self::QosSemanticSafety => {
                "scheduling never changes operation identity, payload, dependencies, or meaning"
            }
            Self::QosIntentPreservation => {
                "scheduler deferral never removes durable retryable intent"
            }
            Self::QosBatchBounds => {
                "no scheduler batch exceeds configured hard operation or byte limits"
            }
            Self::QosStarvationFreedom => {
                "eligible durable work receives execution opportunity under fairness assumptions"
            }
            Self::QosServerHintSafety => {
                "server overload hints reduce work without causing durable intent loss"
            }
            Self::QosLeadershipRetrySafety => {
                "leadership changes preserve durable retry timing and operation identity"
            }
            Self::ScopeCursorBinding => {
                "a scope cursor is interpreted only with matching identity, version, and generation"
            }
            Self::ScopeAuthorizationSafety => {
                "scope resolution and projection cannot expose unauthorized data"
            }
            Self::ScopeContractionSafety => {
                "removed membership is inactive before a contracted scope version is active"
            }
            Self::ScopeRemovalDistinction => {
                "scope-local removal never implies authoritative domain deletion"
            }
            Self::ScopeSharedMembershipSafety => {
                "one scope contraction cannot erase data referenced by another active scope"
            }
            Self::ScopeRevokedIntentSafety => {
                "pending intent cannot be transmitted after its scope authorization is revoked"
            }
            Self::ScopeExpansionAtomicity => {
                "incomplete expansion bootstrap data cannot become active"
            }
            Self::LiveHintLossSafety => {
                "authoritative convergence remains available when every live hint is lost"
            }
            Self::LiveNoStateMutation => {
                "receiving a live hint cannot advance a cursor or mutate replica state directly"
            }
            Self::LiveScopeIsolation => {
                "live fan-out intentionally targets only authenticated authorized scopes"
            }
            Self::LiveBackpressureBound => {
                "slow live consumers cannot cause an unbounded hint queue"
            }
            Self::LiveLeadershipCatchUp => {
                "a new local leader restores freshness by durable catch-up without hint replay"
            }
            Self::ImportIdentityDeterminism => {
                "the same source record and mapping resolve to the same target identity on retry"
            }
            Self::ImportCheckpointSafety => {
                "an import checkpoint never advances beyond atomically committed target data"
            }
            Self::ImportCutoverVerification => {
                "authority cutover cannot occur while any required verification has failed"
            }
            Self::ImportBaselineSafety => {
                "baseline seeding does not require fabricated per-row synchronization history"
            }
            Self::ImportJournalVisibility => {
                "post-activation imported authority changes are visible through the normal journal"
            }
            Self::ImportQuarantineSafety => {
                "quarantined invalid records never count silently as successfully imported"
            }
            Self::BootstrapCursorActivationSafety => {
                "a bootstrap cursor activates only with its corresponding durable generation"
            }
            Self::BootstrapBoundaryConsistency => {
                "every published snapshot chunk represents one consistent authority boundary"
            }
            Self::BootstrapStagingIsolation => {
                "partial snapshot staging is never exposed as an active complete scope"
            }
            Self::BootstrapChunkIdempotency => {
                "retrying a snapshot chunk cannot duplicate entities after activation"
            }
            Self::BootstrapIntentPreservation => {
                "rebootstrap preserves pending local operation identity and intent"
            }
            Self::BootstrapDeltaConvergence => {
                "snapshot activation followed by delta replay converges with continuous replication"
            }
            Self::ProfileOperationCompatibility => {
                "every operation semantic contract is compatible with its aggregate profile"
            }
            Self::ProfileCapabilitySafety => {
                "registration fails unless the adapter proves every required profile capability"
            }
            Self::ProfileImmutableAppendOnly => {
                "immutable append-only intent is never compacted, rebased, deleted, or overwritten"
            }
            Self::ProfileStrongAggregateAtomicity => {
                "strong aggregate operations require atomic aggregate commit and compare-and-swap"
            }
            Self::ProfileDerivedAuthoritySafety => {
                "derived projections cannot originate authoritative client mutations"
            }
            Self::ProfileNoImplicitLastWriterWins => {
                "unknown operations fail closed instead of inheriting last-writer-wins semantics"
            }
            Self::DeterministicPlanEquivalence => {
                "identical validated operation, state, inputs, and handler version produce equivalent plans"
            }
            Self::DeterministicInputCapture => {
                "domain decisions cannot depend on untracked wall time or randomness"
            }
            Self::ReplaySideEffectIsolation => {
                "replay records side-effect intent without invoking an external system"
            }
            Self::ReplayCommittedInputImmutability => {
                "retry cannot change captured semantic inputs after authoritative commit"
            }
            Self::ReplayHandlerVersionSafety => {
                "handler semantic version changes are explicit and replay compatibility-tested"
            }
            Self::ReplayPolicyVersionSafety => {
                "mutable decision policy and configuration are captured with explicit versions"
            }
            Self::AuditRequiredAtomicity => {
                "required business audit evidence commits atomically with its authoritative mutation"
            }
            Self::AuditRetryDeduplication => {
                "one logical operation cannot create duplicate business audit effects on retry"
            }
            Self::AuditActorTruthfulness => {
                "system and service work is never attributed to an invented user principal"
            }
            Self::AuditAppendOnlyCorrection => {
                "canonical audit history is append-only and corrections reference prior events"
            }
            Self::AuditSensitiveValueSafety => {
                "audit values never exceed their explicit field sensitivity policy"
            }
            Self::AuditExplanationAuthorization => {
                "audit and explanation queries remain tenant-bounded and currently authorized"
            }
            Self::AuditChainContinuity => {
                "each chained audit record references the prior valid hash in its tenant partition"
            }
            Self::AuditTamperDetection => {
                "audit chain discontinuity or digest mismatch fails visibly and is never repaired silently"
            }
            Self::AuditFieldProvenanceAccuracy => {
                "authoritative field provenance identifies the latest committed audit and event mutation"
            }
            Self::GovernanceTombstoneGcSafety => {
                "tombstones remain while any valid retained client can resume before deletion"
            }
            Self::GovernanceJournalFloorSafety => {
                "clients below the retained journal floor must rebootstrap"
            }
            Self::GovernanceLegalHoldPrecedence => {
                "active legal holds prevent covered normal-retention purge"
            }
            Self::GovernanceRequiredEvidenceSafety => {
                "erasure explicitly retains or minimizes policy-required evidence"
            }
            Self::GovernanceSurfaceCompletion => {
                "governance completes only after every required storage surface verifies"
            }
            Self::GovernanceOffboardingWriteFence => {
                "tenant writes are revoked before destructive offboarding purge"
            }
            Self::GovernanceRestoreReconciliation => {
                "restore reapplies erasures and revocations before serving traffic"
            }
            Self::GovernanceResurrectionSafety => {
                "retired stale clients cannot resurrect physically purged identities"
            }
            Self::GovernanceCopyCoverage => {
                "registered snapshots blobs exports replay imports archives and backups participate in governance"
            }
            Self::CryptoArtifactAuthenticity => {
                "signed artifacts verify only under a trusted key authorized for their purpose"
            }
            Self::CryptoTenantBinding => {
                "tenant-scoped signatures and ciphertext cryptographically bind tenant context"
            }
            Self::CryptoRotationHistory => {
                "key rotation preserves historical verification without changing old signatures"
            }
            Self::CryptoRevocationSafety => {
                "revoked keys cannot create new signatures or ciphertext"
            }
            Self::CryptoSecretHandling => {
                "private keys and data-encryption keys never enter ordinary serializable evidence"
            }
            Self::CryptoFailClosed => {
                "required cryptographic verification and decryption failures reject the artifact"
            }
            Self::CryptoE2eSemanticBoundary => {
                "opaque payloads never supply server-side decisions that require plaintext semantics"
            }
            Self::CryptoErasureCompletion => {
                "cryptographic erasure completes only after every usable decrypting key copy is destroyed"
            }
            Self::CryptoSignedOperationImmutability => {
                "a possibly delivered OperationId retains one immutable signed semantic payload"
            }
            Self::AuthoritySingleWriter => {
                "at most one unfenced authority instance accepts writes for one authority timeline"
            }
            Self::AuthorityCursorEpochBinding => {
                "a cursor from one authority epoch is never accepted as continuation in another"
            }
            Self::AuthorityDivergenceEpoch => {
                "restored or potentially divergent history receives a new epoch unless continuity is proven"
            }
            Self::AuthorityRollbackSafety => {
                "a client never silently accepts an epoch below its highest trusted epoch"
            }
            Self::AuthorityNoAutomaticForkMerge => {
                "fork detection quarantines writes and never automatically merges histories"
            }
            Self::AuthorityOperationRecoveryPolicy => {
                "possibly committed old-epoch operations follow explicit recovery policy"
            }
            Self::AuthorityOldPrimaryFence => {
                "an old primary cannot commit after a replacement promotion fence"
            }
            Self::AuthorityLosslessEpochContinuity => {
                "proven lossless infrastructure failover does not open a new authority epoch"
            }
            Self::AuthorityArtifactEpochBinding => {
                "snapshot integrity audit and governance artifacts bind to their authority epoch"
            }
            Self::RegionSingleWriter => {
                "only the active authority writer commits authoritative regional domain transitions"
            }
            Self::RegionAtLeastWatermark => {
                "an AtLeast read is served regionally only through a verified current-epoch watermark"
            }
            Self::RegionSessionMonotonicity => {
                "a session read never intentionally returns state below its caller watermark"
            }
            Self::RegionEpochIsolation => {
                "wrong-epoch regional replicas and caches are never served as current"
            }
            Self::RegionFailureAuthoritySafety => {
                "regional read failure cannot create an alternative authoritative write timeline"
            }
            Self::RegionFallbackSafety => {
                "regional fallback never silently weakens endpoint consistency policy"
            }
            Self::RegionArtifactIntegrity => {
                "regional artifact delivery changes location but not authority or integrity semantics"
            }
            Self::RegionResidencyCoverage => {
                "tenant residency constrains authority replica snapshot blob cache backup and key placement"
            }
            Self::RegionGovernanceCoverage => {
                "governance completion accounts for every required registered regional copy"
            }
            Self::LoadBoundedQueues => {
                "no Aequora in-memory work queue grows without an explicit configured bound"
            }
            Self::LoadPreMutationRejection => {
                "overload rejection occurs before authoritative mutation and cannot report a false commit"
            }
            Self::LoadTenantFairness => {
                "one tenant cannot consume all globally reserved capacity when fairness is enabled"
            }
            Self::LoadSlowClientBound => "slow live clients cannot consume unbounded server memory",
            Self::LoadPriorityStarvationSafety => {
                "bulk and maintenance work cannot permanently starve critical or interactive work"
            }
            Self::LoadRetryJitterSafety => {
                "server retry guidance never removes client-side jitter and backoff"
            }
            Self::LoadDurableIntentSafety => {
                "required durable work is never represented only by an in-memory scheduling queue"
            }
            Self::LoadConsistencySafety => {
                "consistency is never silently weakened solely because the system is overloaded"
            }
            Self::LoadAdmissionOrdering => {
                "admission limits are enforced before entering the resource domains they protect"
            }
            Self::PerformanceBoundedMemory => {
                "externally driven in-memory collections and queues have explicit item and byte bounds"
            }
            Self::PerformanceStreamingLargeObjects => {
                "large snapshots blobs and exports stream without full payload materialization"
            }
            Self::PerformanceCpuIsolation => {
                "CPU-heavy work cannot execute unbounded on asynchronous I/O worker threads"
            }
            Self::PerformanceCorrectnessPreservation => {
                "performance optimization preserves cursor transaction idempotency authorization and audit guarantees"
            }
            Self::PerformanceImmutableHotState => {
                "hot registries and configuration are immutable or replaced as versioned snapshots"
            }
            Self::PerformanceReproducibility => {
                "performance changes are measured by reproducible workloads and attributed phases"
            }
            Self::PerformancePagedUiState => {
                "reactive UI state is a bounded query-derived page rather than a synchronized database mirror"
            }
            Self::PerformanceBlobReferences => {
                "large binary content is referenced by normal domain sync and streamed by the blob subsystem"
            }
            Self::PerformanceEarlyAdmission => {
                "framing and admission reject excessive work before expensive allocation decode or execution"
            }
            Self::ClientIntentPreservation => {
                "resource pressure never discards unsynchronized durable user intent without an explicit decision"
            }
            Self::ClientCursorDurability => {
                "client cursors advance only after required local application commits durably"
            }
            Self::ClientRequiredDirective => {
                "resource deferral never silently ignores required security or governance directives"
            }
            Self::ClientBoundedLargeObject => {
                "bootstrap and blob operations remain memory-bounded on every supported client profile"
            }
            Self::ClientCheckpointRecovery => {
                "process death at a bounded work boundary leaves restart-recoverable durable state"
            }
            Self::ClientPendingBaseEviction => {
                "storage eviction cannot remove base state pinned by an unresolved pending operation"
            }
            Self::ClientLocalCommitAtomicity => {
                "local-first success requires an atomic durable domain mutation and outbox append"
            }
            Self::ClientSemanticParity => {
                "resource profiles change scheduling and bounds but never domain consistency semantics"
            }
            Self::ClientTelemetryPrivacy => {
                "server-visible resource capability is coarse and never accepted as authorization evidence"
            }
            Self::CompatExplicitVersionContext => {
                "a message is decoded only with explicit protocol, kind, and payload-version context"
            }
            Self::CompatRequiredCapabilitySafety => {
                "a required safety or semantic capability is never silently downgraded"
            }
            Self::CompatPossiblySentImmutability => {
                "possibly-sent operations retain immutable schema and payload semantics across upgrades"
            }
            Self::CompatStableIdReservation => {
                "removed protocol and capability IDs are never reused with different semantics"
            }
            Self::CompatFleetActivationSafety => {
                "a required capability activates only after every required serving path supports it"
            }
            Self::CompatUpgradeIntentPreservation => {
                "upgrade-required compatibility state preserves durable local user intent"
            }
            Self::CompatRetryHorizon => {
                "operation schema support covers the legitimate retry horizon or provides explicit recovery"
            }
            Self::CompatAuthorityEpochIndependence => {
                "protocol version changes remain independent from authority epoch changes"
            }
            Self::CompatPolicyAuditability => {
                "compatibility policy changes are versioned, auditable, and fail closed"
            }
            Self::MetadataStoreSchemaDeclaration => {
                "every durable store declares one supported MetadataSchemaVersion before operation"
            }
            Self::MetadataLedgerOperationIdentity => {
                "OperationId is unique in the ledger and semantic payload drift is rejected"
            }
            Self::MetadataClientCursorAtomicity => {
                "authoritative client apply and cursor advancement commit in one local transaction"
            }
            Self::MetadataAuthoritativeAtomicity => {
                "required authority metadata commits atomically with the business mutation"
            }
            Self::MetadataSnapshotPublicationSafety => {
                "published snapshots contain only durable verified chunks from one boundary"
            }
            Self::MetadataAdapterSemanticEquivalence => {
                "physical adapters preserve logical uniqueness ordering transaction and retention semantics"
            }
            Self::MetadataStaleFenceRejection => {
                "a stale fencing token cannot update durable metadata state"
            }
            Self::MetadataMigrationIntentPreservation => {
                "metadata migrations preserve pending intent or commit no partial migration"
            }
            Self::MetadataSecretKeyExclusion => {
                "ordinary metadata records contain no private secret key material"
            }
            Self::JobDurableBeforeExecution => {
                "required asynchronous work exists durably before execution can be depended upon"
            }
            Self::JobCurrentFenceOnly => {
                "only the current fencing token holder may checkpoint or terminally transition a job"
            }
            Self::JobCommittedSideEffectIntent => {
                "domain-side external execution requires a committed immutable SideEffectIntent"
            }
            Self::JobAuthoritativeResultOperation => {
                "workers submit business-state changes through authoritative domain handlers"
            }
            Self::JobExplicitAmbiguityRecovery => {
                "ambiguous external outcomes follow explicit reconciliation policy instead of blind retry"
            }
            Self::JobDurableState => {
                "required job and workflow correctness state is never stored only in process memory"
            }
            Self::JobStaleWorkerRejection => {
                "an expired stale worker cannot overwrite progress committed by a newer claim"
            }
            Self::JobBoundedCheckpoint => {
                "long-running jobs checkpoint bounded progress for restart-safe recovery"
            }
            Self::JobBoundedRetry => {
                "retry scheduling is backed off and cannot create an uncontrolled tight loop"
            }
            Self::AdminNoDomainBypass => {
                "admin mutations execute through the correctness service responsible for the affected invariant"
            }
            Self::AdminDurableAttribution => {
                "high-risk admin mutations bind an authenticated principal and durable AdminOperationId"
            }
            Self::AdminPayloadImmutability => {
                "one AdminOperationId cannot be retried with a different semantic payload"
            }
            Self::AdminReviewedPlanBinding => {
                "destructive execution binds the exact reviewed plan and rejects stale safety state"
            }
            Self::AdminServerAuthorization => {
                "admin authorization is evaluated server-side independent of UI visibility"
            }
            Self::AdminPrivateKeyExclusion => {
                "admin API responses never contain private cryptographic key material"
            }
            Self::AdminOverrideSeparation => {
                "force and override actions are distinct and more strongly authorized than normal actions"
            }
            Self::AdminDataPlaneIndependence => {
                "control-plane unavailability does not invalidate normal data-plane correctness"
            }
            Self::AdminVerifiedCompletion => {
                "high-risk admin actions complete only after their defined postconditions verify"
            }
            Self::DiagnosticNonAuthoritative => {
                "incident diagnostics never become a source of authoritative business state"
            }
            Self::DiagnosticSecretExclusion => {
                "ordinary incident bundles never contain private keys authentication secrets or raw secret values"
            }
            Self::DiagnosticManifestCompleteness => {
                "every bundle declares schema selectors completeness and cryptographic content digest"
            }
            Self::DiagnosticReplaySideEffectIsolation => {
                "diagnostic reproduction records simulated intents without real external side effects"
            }
            Self::DiagnosticCollectionBounds => {
                "diagnostic collection obeys explicit size time file and record bounds"
            }
            Self::DiagnosticEvidenceConfidence => {
                "forensic explanations distinguish authoritative durable derived and best-effort evidence"
            }
            Self::DiagnosticPrePublicationSanitization => {
                "governance and redaction policy is applied before diagnostic artifact publication"
            }
            Self::DiagnosticVerifiedBundle => {
                "a verified bundle passed required schema hash and signature checks for its mode"
            }
            Self::DiagnosticReplayProductionIsolation => {
                "diagnostic replay cannot mutate production authority or production client state"
            }
            Self::LegacySingleWriteOwner => {
                "each migrated aggregate has exactly one effective authoritative write owner"
            }
            Self::LegacyCursorAfterDurability => {
                "legacy CDC position advances only with a durably recorded canonical result"
            }
            Self::LegacyBridgeIdempotency => {
                "duplicate legacy delivery cannot duplicate canonical authoritative effects"
            }
            Self::LegacyPostCutoverFence => {
                "direct legacy writes after Aequora cutover are fenced or critical violations"
            }
            Self::LegacyTypedFacade => {
                "legacy compatibility APIs translate into typed Aequora operations"
            }
            Self::LegacyMappingFailClosed => {
                "unknown legacy business states are never silently coerced into canonical state"
            }
            Self::LegacyShadowIsolation => {
                "shadow execution produces neither authoritative mutation nor real side effects"
            }
            Self::LegacyGovernanceCoverage => {
                "governance accounts for legacy copies until formal retirement"
            }
            Self::LegacyVerifiedCutover => {
                "cutover completes only after writer fencing final-boundary apply and canonical verification"
            }
            Self::SecurityServerValidatedClaims => {
                "client tenant role scope priority and authority claims require server validation"
            }
            Self::SecurityPayloadImmutability => {
                "one OperationId cannot produce different semantics through payload substitution"
            }
            Self::SecurityCapabilityFailClosed => {
                "required security capabilities are never silently downgraded"
            }
            Self::SecurityExternalInputBounds => {
                "every external collection payload archive graph and upload has explicit bounds"
            }
            Self::SecurityTenantIsolation => {
                "known entity scope blob operation and snapshot identifiers do not bypass tenant isolation"
            }
            Self::SecuritySecretExclusion => {
                "private keys and authentication secrets never enter logs diagnostics audit payloads or responses"
            }
            Self::SecurityAuthorityRollback => {
                "a stale authority epoch cannot silently resume trusted synchronization"
            }
            Self::SecuritySideEffectSafety => {
                "financial and irreversible side effects use explicit idempotency and reconciliation"
            }
            Self::SecurityAdminOverride => {
                "admin override paths are more strongly authorized and audited than ordinary paths"
            }
            Self::SecurityIntegrationDistrust => {
                "legacy import webhook diagnostic and provider inputs remain untrusted"
            }
            Self::FeedIndependentCursor => {
                "every durable consumer has an independent cursor bound to authority identity and epoch"
            }
            Self::FeedDurableEffectBeforeCursor => {
                "a consumer cursor advances only after its required effect is durably complete"
            }
            Self::FeedConsumerIsolation => {
                "a stalled consumer cannot block authoritative commits synchronization or unrelated consumers"
            }
            Self::FeedDuplicateIdempotency => {
                "duplicate EventId delivery cannot duplicate a declared idempotent consumer effect"
            }
            Self::FeedRetentionRecovery => {
                "a consumer below the journal floor follows declared rebuild or recovery policy"
            }
            Self::FeedExternalProjectionSafety => {
                "external feeds expose only explicitly versioned and authorized integration projections"
            }
            Self::FeedOrderingSafety => {
                "parallel partitioning never silently weakens declared consumer ordering"
            }
            Self::FeedResetSafety => {
                "history-skipping consumer reset requires an explicit authorized audited plan"
            }
            Self::FeedGovernanceCoverage => {
                "governed consumer stores participate in governance and residency policy"
            }
            Self::RegistryIdNonReuse => {
                "a published durable registry ID is never reused for different semantics"
            }
            Self::RegistryCanonicalResolution => {
                "every durable identifier resolves canonically or is rejected as unknown"
            }
            Self::RegistryBreakingChangePath => {
                "breaking durable semantics require a new version migration upcaster or declared incompatibility"
            }
            Self::RegistryGeneratedSourceParity => {
                "generated constants and lookup tables derive from canonical registry sources"
            }
            Self::RegistryHistoricalReservation => {
                "deprecated and removed IDs remain reserved and historically interpretable"
            }
            Self::RegistryNamespaceIsolation => {
                "application and extension namespaces cannot collide with core or each other"
            }
            Self::RegistrySecurityReview => {
                "security-sensitive registry changes require explicit compatibility and security review"
            }
            Self::RegistryRuntimeImmutability => {
                "runtime configuration cannot redefine compiled durable registry semantics"
            }
            Self::RegistryHistoricalResolution => {
                "supported replay audit and incident tooling can resolve retained historical IDs"
            }
            Self::CertificationSemanticOnly => {
                "certification evaluates observable semantics rather than physical implementation choices"
            }
            Self::CertificationTierTruthfulness => {
                "a certification tier is never claimed when a required test fails is skipped or is unsupported"
            }
            Self::CertificationExactBinding => {
                "certification evidence binds the exact subject features suite and execution environment"
            }
            Self::CertificationCapabilityTruthfulness => {
                "a claimed capability remains unverified until every required capability test passes"
            }
            Self::CertificationRuntimeValidation => {
                "certification never replaces runtime validation compatibility negotiation or startup safety checks"
            }
            Self::CertificationArtifactImmutability => {
                "historical certification artifacts remain immutable content-addressed and verifiable"
            }
            Self::CertificationEvidenceIntegrity => {
                "evidence hashes and signatures are verified before certification trust is accepted"
            }
            Self::CertificationCorrectnessPriority => {
                "performance characterization cannot substitute for a failed correctness requirement"
            }
            Self::CertificationLifecycleIdentity => {
                "suspension revocation and supersession retain the original certification identity"
            }
            Self::MobileDurableProcessIndependence => {
                "required synchronization state never depends solely on Android or iOS process lifetime"
            }
            Self::MobileIntentAtomicity => {
                "mobile local mutation and durable outbox insertion remain one atomic outcome"
            }
            Self::MobilePushHintOnly => {
                "push notifications are untrusted scheduling hints and never authoritative state"
            }
            Self::MobileCursorCheckpointSafety => {
                "background expiration cannot advance a cursor beyond durably applied state"
            }
            Self::MobileSecureKeyStorage => {
                "mobile private keys remain in an approved platform secure provider or equivalent"
            }
            Self::MobileResourceSemanticSafety => {
                "resource adaptation may reduce throughput but cannot weaken correctness authorization audit or conflict semantics"
            }
            Self::MobileUpgradeIntentSafety => {
                "mobile upgrade preserves pending user intent or fails without partial destruction"
            }
            Self::MobileStorageTruthfulness => {
                "unavailable durable storage cannot produce a successful saved-mutation result"
            }
            Self::MobilePlatformBoundary => {
                "platform and UI code cannot advance cursors mutate authority ledgers or bypass domain operations"
            }
            Self::DesktopSingleCoordinator => {
                "at most one active synchronization coordinator owns a desktop local store"
            }
            Self::DesktopStaleFenceSafety => {
                "a stale desktop process cannot commit coordinator metadata after ownership changes"
            }
            Self::DesktopResumeIdempotency => {
                "desktop sleep resume and ambiguous retries cannot duplicate an authoritative effect"
            }
            Self::DesktopIpcBoundary => {
                "desktop IPC routes writes through domain operations and cannot mutate synchronization metadata directly"
            }
            Self::DesktopCloneBindingSafety => {
                "a copied desktop store cannot silently continue with the original trusted device binding"
            }
            Self::DesktopUpgradeIntentSafety => {
                "desktop upgrades preserve pending user intent or fail before destructive migration"
            }
            Self::DesktopModeParity => {
                "agent and in-process desktop modes preserve identical synchronization semantics"
            }
            Self::DesktopPersistenceTruthfulness => {
                "desktop disk and credential-store failures produce explicit failure rather than false persistence success"
            }
            Self::DesktopDerivedStateSafety => {
                "desktop indexes caches tray and UI state never become authoritative business state"
            }
            Self::StorageLocalIntentAtomicity => {
                "local domain state and outbox intent commit as one durable transaction"
            }
            Self::StorageCriticalIntentRetention => {
                "storage pressure never automatically evicts critical durable intent"
            }
            Self::StorageCacheIsolation => {
                "cache or derived-state loss cannot remove pending authoritative intent"
            }
            Self::StorageCloneBindingSafety => {
                "a restored or cloned store cannot silently reuse a mismatched secure device binding"
            }
            Self::StorageDowngradeSafety => {
                "an older binary rejects a newer store format unless downgrade compatibility is certified"
            }
            Self::StoragePlatformCertification => {
                "required transaction semantics are certified on each actual target platform"
            }
            Self::StoragePublicationSafety => {
                "temporary snapshot and blob data is verified before durable publication"
            }
            Self::StorageAdmissionTruthfulness => {
                "low disk may reduce function but cannot produce a false successful commit"
            }
            Self::StorageSecretIsolation => {
                "secrets and encryption keys are excluded from plaintext ordinary sync metadata"
            }
            Self::StorageFilesystemSafety => {
                "live embedded stores are excluded from unverified network and cloud-synchronized filesystems"
            }
            Self::ImplementationFoundationIsolation => {
                "foundation and protocol crates never depend on frameworks, databases, UI, or platform integrations"
            }
            Self::ImplementationStorageTypeIsolation => {
                "physical database types never appear in storage-neutral public contracts"
            }
            Self::ImplementationClientUiIsolation => {
                "client synchronization correctness does not depend on a UI framework"
            }
            Self::ImplementationServerHttpIsolation => {
                "server synchronization correctness does not depend on HTTP routing behavior"
            }
            Self::ImplementationAdapterDirection => {
                "physical adapters implement inward storage contracts and are never imported by storage core"
            }
            Self::ImplementationPlatformIsolation => {
                "platform-specific dependencies are isolated to platform and integration crates"
            }
            Self::ImplementationFeatureIsolation => {
                "Cargo features do not substitute for architectural adapter boundaries"
            }
            Self::ImplementationCompositionRoot => {
                "application binaries compose libraries without owning reusable synchronization semantics"
            }
            Self::ImplementationRegistryDeterminism => {
                "generated registry artifacts deterministically correspond to canonical registry sources"
            }
            Self::ImplementationDependencyGraph => {
                "every production dependency edge conforms to the declared workspace layer graph"
            }
            Self::SdkCursorIsolation => {
                "public client APIs cannot directly advance authoritative synchronization cursors"
            }
            Self::SdkLocalCommitDistinction => {
                "public mutation success distinguishes durable local commit from authority confirmation"
            }
            Self::SdkStorageNeutrality => {
                "stable public APIs expose no physical database or platform transaction types"
            }
            Self::SdkCancellationSafety => {
                "cancelling an SDK future cannot invalidate already-durable local intent"
            }
            Self::SdkEventLossSafety => {
                "advisory event loss cannot make durable synchronization state unrecoverable"
            }
            Self::SdkErrorCompatibility => {
                "stable public error categories and codes remain interpretable across releases"
            }
            Self::SdkClosedCoreSemantics => {
                "extensions cannot redefine cursor authority idempotency or identity semantics"
            }
            Self::SdkVersionIndependence => {
                "crate protocol store and operation schema versions evolve independently"
            }
            Self::SdkDangerousOperationIsolation => {
                "dangerous operations are explicit and absent from ordinary convenience APIs"
            }
            Self::SdkSemverAutomation => {
                "public API changes are automatically checked for accidental semver breakage"
            }
            Self::AdapterCapabilityTruthfulness => {
                "an adapter capability claim is valid only with matching conformance evidence"
            }
            Self::AdapterLocalAtomicity => {
                "local application mutation and durable outbox intent commit atomically"
            }
            Self::AdapterAuthorityAtomicity => {
                "authoritative business version journal ledger and required audit state commit atomically"
            }
            Self::AdapterTypeIsolation => {
                "database transaction and driver error types never leak into storage-neutral APIs"
            }
            Self::AdapterPayloadBinding => {
                "an operation identifier cannot be reused with a different canonical payload digest"
            }
            Self::AdapterMigrationSafety => {
                "physical migrations preserve durable identity cursor outbox ledger and authority semantics"
            }
            Self::AdapterStartupSafety => {
                "missing required adapter capabilities reject startup instead of weakening correctness"
            }
            Self::AdapterEnvironmentBinding => {
                "adapter certification binds exact adapter engine platform and relevant configuration"
            }
            Self::AdapterCriticalDurability => {
                "performance tuning cannot weaken the durability class of critical intent"
            }
            Self::AdapterManifestTruthfulness => {
                "official adapters publish stable capability manifests and explicit known limitations"
            }
            Self::PostgresAuthorityAtomicity => {
                "PostgreSQL business mutation version journal ledger audit and intents commit atomically"
            }
            Self::PostgresIdempotentReplay => {
                "an identical PostgreSQL OperationId retry returns its prior outcome without another effect"
            }
            Self::PostgresPayloadBinding => {
                "PostgreSQL rejects an OperationId retry with a different canonical payload digest"
            }
            Self::PostgresCommittedTimeline => {
                "PostgreSQL journal cursors follow transactional committed timeline allocation"
            }
            Self::PostgresRestoreEpoch => {
                "a PostgreSQL or Neon restore opens a newer authority epoch before synchronization resumes"
            }
            Self::PostgresTypeIsolation => {
                "SQLx and PostgreSQL-specific types remain inside the physical adapter boundary"
            }
            Self::PostgresReadinessSafety => {
                "unsafe correctness-critical PostgreSQL settings prevent authoritative readiness"
            }
            Self::PostgresSideEffectIntent => {
                "external side effects are durable transaction intents executed only after commit"
            }
            Self::PostgresRetentionSafety => {
                "PostgreSQL compaction never crosses active consumer policy or bootstrap safety floors"
            }
            Self::PostgresNeonSemanticParity => {
                "Neon autosuspend scaling branching and pooling never redefine authority semantics"
            }
            Self::StoolapLocalAtomicity => {
                "every durable provisional domain mutation commits atomically with its Stoolap outbox intent"
            }
            Self::StoolapReconciliationAtomicity => {
                "Stoolap authoritative apply outcomes conflicts overlay reconciliation and cursor commit atomically"
            }
            Self::StoolapRetryIdentity => {
                "a possibly transmitted Stoolap operation retains its OperationId and semantic payload across retries"
            }
            Self::StoolapIntentPreservation => {
                "Stoolap rebootstrap migration repair and storage reclamation preserve pending user intent"
            }
            Self::StoolapCursorSafety => {
                "a Stoolap cursor never advances beyond authoritative state durably installed locally"
            }
            Self::StoolapFencedCoordinator => {
                "only the current fenced coordinator performs leader-owned Stoolap metadata transitions"
            }
            Self::StoolapCloneSafety => {
                "a restored or cloned Stoolap store validates device binding generation and secure-key state"
            }
            Self::StoolapStoragePressureSafety => {
                "Stoolap storage pressure never evicts critical pending intent or correctness metadata"
            }
            Self::StoolapTypeIsolation => {
                "Stoolap-specific types remain inside the physical local adapter boundary"
            }
            Self::StoolapPlatformCertification => {
                "Stoolap support on a platform requires the matching target-bound conformance profile"
            }
            Self::AxumTransportIsolation => {
                "Axum routes contain transport orchestration while correctness remains in transport-neutral server core"
            }
            Self::AxumCredentialIsolation => {
                "raw authentication credentials never reach domain handlers or storage adapters"
            }
            Self::AxumResourceBounds => {
                "HTTP wire decompressed operation and dependency work is bounded before expensive execution"
            }
            Self::AxumCommitDeliveryIndependence => {
                "client disconnect or response failure cannot invalidate an already committed authoritative operation"
            }
            Self::AxumOverloadBounds => {
                "temporary overload is rejected before unbounded memory task or database-pool growth"
            }
            Self::AxumIdentityBinding => {
                "protocol tenant and device identity matches authenticated server identity before execution"
            }
            Self::AxumStableSemantics => {
                "HTTP status remains transport metadata and never replaces stable operation or error semantics"
            }
            Self::AxumLiveHintDurability => {
                "missed HTTP live hints cannot lose durable state because cursor exchange remains authoritative"
            }
            Self::AxumNodeEpochIndependence => {
                "ordinary Axum node restart scaling or replacement never creates a new AuthorityEpoch"
            }
            Self::AxumErrorSanitization => {
                "public HTTP errors never expose internal database topology credential or stack details"
            }
            Self::DioxusDurableStateOwnership => {
                "correctness-critical synchronization state never exists only in Dioxus component memory"
            }
            Self::DioxusLocalCommitTruth => {
                "the UI reports local durability only after domain state and outbox intent commit atomically"
            }
            Self::DioxusAuthorityDistinction => {
                "successful local persistence never implies authoritative server confirmation"
            }
            Self::DioxusEventLossSafety => {
                "loss of advisory UI events cannot lose correctness because durable state can be reread"
            }
            Self::DioxusUnmountSafety => {
                "component unmount cannot cancel or erase previously committed operation intent"
            }
            Self::DioxusStoreIsolation => {
                "query caches and reactive state are isolated by active local store identity"
            }
            Self::DioxusConflictSemantics => {
                "conflict resolution uses durable domain intent rather than direct UI-state mutation"
            }
            Self::DioxusOfflineCapability => {
                "offline mode preserves every domain-permitted local read and write"
            }
            Self::DioxusBackgroundOwnership => {
                "OS background scheduling belongs to platform and client runtimes rather than mounted components"
            }
            Self::DioxusEventBounds => {
                "large synchronization batches cannot create unbounded UI queues rerenders or memory growth"
            }
            Self::CliNoInvariantBypass => {
                "CLI tooling cannot bypass domain authority idempotency migration or control-plane invariants"
            }
            Self::CliMachineOutputVersioning => {
                "machine-readable output is versioned and never requires scraping human terminal formatting"
            }
            Self::CliSensitiveRedaction => {
                "secrets and registry-classified sensitive values are redacted by default"
            }
            Self::CliPlanApplySafety => {
                "dangerous production mutations require typed authorization and reviewed plan/apply semantics"
            }
            Self::CliSubmissionAmbiguity => {
                "timeout cancellation or response loss never implies a durably submitted operation did not execute"
            }
            Self::CliStoreOwnership => {
                "local store inspection respects process ownership leases and fencing"
            }
            Self::CliMigrationSafety => {
                "migration application verifies identity checksum source state capabilities and fencing"
            }
            Self::CliSemanticMutation => {
                "conflict repair authority and consumer changes use registered semantic or control operations"
            }
            Self::CliProductionGuard => {
                "development-only destructive facilities cannot be accidentally enabled against production"
            }
            Self::CliCanonicalSemantics => {
                "CLI and developer tooling reuse canonical SDK and control-plane semantics"
            }
            Self::SqliteWalDurability => {
                "every production SQLite local replica opens and remains in WAL journal mode"
            }
            Self::SqliteSingleWriter => {
                "one logical writer serializes SQLite local mutation and reconciliation transactions"
            }
            Self::SqliteCursorAtomicity => {
                "a SQLite cursor changes only in the transaction that durably installs its authoritative state"
            }
            Self::SqliteOutboxPreservation => {
                "SQLite outbox identity and canonical payload remain durable until lifecycle completion"
            }
            Self::SqliteAdapterParity => {
                "SQLite and Stoolap expose identical synchronization behavior through neutral adapter contracts"
            }
            Self::ConfigCorrectnessPreservation => {
                "configuration cannot disable correctness authority tenant isolation or idempotency invariants"
            }
            Self::ConfigSecretRedaction => {
                "secrets use secret-aware resolution and remain redacted from ordinary diagnostics and CLI output"
            }
            Self::ConfigValidatedPublication => {
                "only fully parsed and validated configuration snapshots become effective"
            }
            Self::ConfigAtomicReload => {
                "runtime configuration publication exposes one complete generation to every consumer"
            }
            Self::ConfigDurableIdentityIsolation => {
                "authority device store and generation identities remain durable state rather than editable deployment configuration"
            }
            Self::ConfigFeatureSemanticSafety => {
                "feature flags cannot silently redefine durable operation semantics"
            }
            Self::ConfigProductionSafety => {
                "production rejects authentication bypass destructive reset fault injection and unsafe TLS modes"
            }
            Self::ConfigAdapterCapabilitySafety => {
                "adapter-specific configuration activates only against declared certified capabilities"
            }
            Self::ConfigFailedReloadPreservation => {
                "a failed runtime reload leaves the previous valid configuration generation active"
            }
            Self::ConfigAuthoritativePolicy => {
                "client feature flags cannot grant business entitlement or weaken authoritative server security policy"
            }
            Self::ReleaseTraceability => {
                "every official artifact is bound to immutable source build configuration target and release manifest"
            }
            Self::ReleaseArtifactImmutability => {
                "a published semantic version cannot silently identify different artifact bytes"
            }
            Self::ReleaseSignatureIntegrity => {
                "production artifacts are integrity checked and signed by a purpose-specific identity"
            }
            Self::ReleaseCompatibilityGate => {
                "upgrade cannot bypass database store protocol configuration or registry compatibility checks"
            }
            Self::ReleaseClientIntentPreservation => {
                "supported client upgrades preserve pending operations cursors conflicts store identity and durable intent"
            }
            Self::ReleaseRollbackSafety => {
                "rollback requires compatible resulting state or an explicit verified rollback migration"
            }
            Self::ReleaseImmutablePromotion => {
                "release channels promote the same exact artifact bytes whenever practical"
            }
            Self::ReleaseCredentialIsolation => {
                "production signing and publishing credentials remain isolated from ordinary build jobs"
            }
            Self::ReleaseUpdateFailClosed => {
                "unsigned mismatched expired incompatible halted and revoked updates never become trusted"
            }
            Self::ReleaseVersionDimensionSeparation => {
                "product SemVer remains independent from protocol store operation config snapshot and registry versions"
            }
            Self::DeploymentSingleWriter => {
                "every active authority scope has exactly one current authoritative writer timeline"
            }
            Self::DeploymentSemanticPreservation => {
                "nodes load balancers regions proxies and workers cannot change idempotency ordering cursor or transaction durability"
            }
            Self::DeploymentNodeEpochIndependence => {
                "ordinary server restart or horizontal replacement does not create a new authority epoch"
            }
            Self::DeploymentDatabaseCredentialIsolation => {
                "clients never connect directly to or receive credentials for the authoritative database"
            }
            Self::DeploymentRegionalReadSafety => {
                "regional reads use explicit consistency and cannot accept authoritative writes without promotion"
            }
            Self::DeploymentAirGapIndependence => {
                "air-gapped deployments preserve verification compatibility authority audit and upgrade semantics without public services"
            }
            Self::DeploymentRetrySafety => {
                "infrastructure retries failover balancing and duplicate workers cannot create duplicate logical effects"
            }
            Self::DeploymentRestoreEpoch => {
                "restore or failover with uncertain timeline continuity establishes a new epoch before synchronization resumes"
            }
            Self::DeploymentDurableStateOwnership => {
                "correctness-critical state exists in certified durable storage and never solely in process-local caches"
            }
            Self::DeploymentInfrastructureIndependence => {
                "core correctness requires no orchestrator broker cache service mesh or public cloud"
            }
        }
    }

    /// Registry entry with required cross-layer evidence hooks.
    #[must_use]
    pub const fn entry(self) -> InvariantEntry {
        REGISTRY[self.index()]
    }

    #[allow(clippy::too_many_lines)]
    const fn index(self) -> usize {
        match self {
            Self::IdempotentAuthority => 0,
            Self::LocalIntentAtomicity => 1,
            Self::AuthoritativePublicationAtomicity => 2,
            Self::CursorSafety => 3,
            Self::VersionMonotonicity => 4,
            Self::NoUnauthorizedCommit => 5,
            Self::RetryPreservation => 6,
            Self::ReconciliationIdempotency => 7,
            Self::TombstoneSafety => 8,
            Self::TimelineSafety => 9,
            Self::UniqueEventIdentity => 10,
            Self::EventCorrelation => 11,
            Self::RetryLineagePreservation => 12,
            Self::DerivedLineagePreservation => 13,
            Self::TenantLineageSafety => 14,
            Self::CausalityAcyclic => 15,
            Self::AntiEntropyRootAgreement => 16,
            Self::RepairAuthoritySafety => 17,
            Self::RepairIntentPreservation => 18,
            Self::SameVersionDigestSafety => 19,
            Self::IntegrityCursorIsolation => 20,
            Self::QueueSemanticEquivalence => 21,
            Self::DeliveredPayloadImmutability => 22,
            Self::QueueDependencySafety => 23,
            Self::QueueIntentPreservation => 24,
            Self::RebaseIdentitySafety => 25,
            Self::SensitiveOperationSafety => 26,
            Self::LocalFenceUniqueness => 27,
            Self::StaleLeaderCommitSafety => 28,
            Self::FollowerMutationSafety => 29,
            Self::LocalLeadershipLiveness => 30,
            Self::LeadershipIntentPreservation => 31,
            Self::StoreGenerationSafety => 32,
            Self::QosSemanticSafety => 33,
            Self::QosIntentPreservation => 34,
            Self::QosBatchBounds => 35,
            Self::QosStarvationFreedom => 36,
            Self::QosServerHintSafety => 37,
            Self::QosLeadershipRetrySafety => 38,
            Self::ScopeCursorBinding => 39,
            Self::ScopeAuthorizationSafety => 40,
            Self::ScopeContractionSafety => 41,
            Self::ScopeRemovalDistinction => 42,
            Self::ScopeSharedMembershipSafety => 43,
            Self::ScopeRevokedIntentSafety => 44,
            Self::ScopeExpansionAtomicity => 45,
            Self::LiveHintLossSafety => 46,
            Self::LiveNoStateMutation => 47,
            Self::LiveScopeIsolation => 48,
            Self::LiveBackpressureBound => 49,
            Self::LiveLeadershipCatchUp => 50,
            Self::ImportIdentityDeterminism => 51,
            Self::ImportCheckpointSafety => 52,
            Self::ImportCutoverVerification => 53,
            Self::ImportBaselineSafety => 54,
            Self::ImportJournalVisibility => 55,
            Self::ImportQuarantineSafety => 56,
            Self::BootstrapCursorActivationSafety => 57,
            Self::BootstrapBoundaryConsistency => 58,
            Self::BootstrapStagingIsolation => 59,
            Self::BootstrapChunkIdempotency => 60,
            Self::BootstrapIntentPreservation => 61,
            Self::BootstrapDeltaConvergence => 62,
            Self::ProfileOperationCompatibility => 63,
            Self::ProfileCapabilitySafety => 64,
            Self::ProfileImmutableAppendOnly => 65,
            Self::ProfileStrongAggregateAtomicity => 66,
            Self::ProfileDerivedAuthoritySafety => 67,
            Self::ProfileNoImplicitLastWriterWins => 68,
            Self::DeterministicPlanEquivalence => 69,
            Self::DeterministicInputCapture => 70,
            Self::ReplaySideEffectIsolation => 71,
            Self::ReplayCommittedInputImmutability => 72,
            Self::ReplayHandlerVersionSafety => 73,
            Self::ReplayPolicyVersionSafety => 74,
            Self::AuditRequiredAtomicity => 75,
            Self::AuditRetryDeduplication => 76,
            Self::AuditActorTruthfulness => 77,
            Self::AuditAppendOnlyCorrection => 78,
            Self::AuditSensitiveValueSafety => 79,
            Self::AuditExplanationAuthorization => 80,
            Self::AuditChainContinuity => 81,
            Self::AuditTamperDetection => 82,
            Self::AuditFieldProvenanceAccuracy => 83,
            Self::GovernanceTombstoneGcSafety => 84,
            Self::GovernanceJournalFloorSafety => 85,
            Self::GovernanceLegalHoldPrecedence => 86,
            Self::GovernanceRequiredEvidenceSafety => 87,
            Self::GovernanceSurfaceCompletion => 88,
            Self::GovernanceOffboardingWriteFence => 89,
            Self::GovernanceRestoreReconciliation => 90,
            Self::GovernanceResurrectionSafety => 91,
            Self::GovernanceCopyCoverage => 92,
            Self::CryptoArtifactAuthenticity => 93,
            Self::CryptoTenantBinding => 94,
            Self::CryptoRotationHistory => 95,
            Self::CryptoRevocationSafety => 96,
            Self::CryptoSecretHandling => 97,
            Self::CryptoFailClosed => 98,
            Self::CryptoE2eSemanticBoundary => 99,
            Self::CryptoErasureCompletion => 100,
            Self::CryptoSignedOperationImmutability => 101,
            Self::AuthoritySingleWriter => 102,
            Self::AuthorityCursorEpochBinding => 103,
            Self::AuthorityDivergenceEpoch => 104,
            Self::AuthorityRollbackSafety => 105,
            Self::AuthorityNoAutomaticForkMerge => 106,
            Self::AuthorityOperationRecoveryPolicy => 107,
            Self::AuthorityOldPrimaryFence => 108,
            Self::AuthorityLosslessEpochContinuity => 109,
            Self::AuthorityArtifactEpochBinding => 110,
            Self::RegionSingleWriter => 111,
            Self::RegionAtLeastWatermark => 112,
            Self::RegionSessionMonotonicity => 113,
            Self::RegionEpochIsolation => 114,
            Self::RegionFailureAuthoritySafety => 115,
            Self::RegionFallbackSafety => 116,
            Self::RegionArtifactIntegrity => 117,
            Self::RegionResidencyCoverage => 118,
            Self::RegionGovernanceCoverage => 119,
            Self::LoadBoundedQueues => 120,
            Self::LoadPreMutationRejection => 121,
            Self::LoadTenantFairness => 122,
            Self::LoadSlowClientBound => 123,
            Self::LoadPriorityStarvationSafety => 124,
            Self::LoadRetryJitterSafety => 125,
            Self::LoadDurableIntentSafety => 126,
            Self::LoadConsistencySafety => 127,
            Self::LoadAdmissionOrdering => 128,
            Self::PerformanceBoundedMemory => 129,
            Self::PerformanceStreamingLargeObjects => 130,
            Self::PerformanceCpuIsolation => 131,
            Self::PerformanceCorrectnessPreservation => 132,
            Self::PerformanceImmutableHotState => 133,
            Self::PerformanceReproducibility => 134,
            Self::PerformancePagedUiState => 135,
            Self::PerformanceBlobReferences => 136,
            Self::PerformanceEarlyAdmission => 137,
            Self::ClientIntentPreservation => 138,
            Self::ClientCursorDurability => 139,
            Self::ClientRequiredDirective => 140,
            Self::ClientBoundedLargeObject => 141,
            Self::ClientCheckpointRecovery => 142,
            Self::ClientPendingBaseEviction => 143,
            Self::ClientLocalCommitAtomicity => 144,
            Self::ClientSemanticParity => 145,
            Self::ClientTelemetryPrivacy => 146,
            Self::CompatExplicitVersionContext => 147,
            Self::CompatRequiredCapabilitySafety => 148,
            Self::CompatPossiblySentImmutability => 149,
            Self::CompatStableIdReservation => 150,
            Self::CompatFleetActivationSafety => 151,
            Self::CompatUpgradeIntentPreservation => 152,
            Self::CompatRetryHorizon => 153,
            Self::CompatAuthorityEpochIndependence => 154,
            Self::CompatPolicyAuditability => 155,
            Self::MetadataStoreSchemaDeclaration => 156,
            Self::MetadataLedgerOperationIdentity => 157,
            Self::MetadataClientCursorAtomicity => 158,
            Self::MetadataAuthoritativeAtomicity => 159,
            Self::MetadataSnapshotPublicationSafety => 160,
            Self::MetadataAdapterSemanticEquivalence => 161,
            Self::MetadataStaleFenceRejection => 162,
            Self::MetadataMigrationIntentPreservation => 163,
            Self::MetadataSecretKeyExclusion => 164,
            Self::JobDurableBeforeExecution => 165,
            Self::JobCurrentFenceOnly => 166,
            Self::JobCommittedSideEffectIntent => 167,
            Self::JobAuthoritativeResultOperation => 168,
            Self::JobExplicitAmbiguityRecovery => 169,
            Self::JobDurableState => 170,
            Self::JobStaleWorkerRejection => 171,
            Self::JobBoundedCheckpoint => 172,
            Self::JobBoundedRetry => 173,
            Self::AdminNoDomainBypass => 174,
            Self::AdminDurableAttribution => 175,
            Self::AdminPayloadImmutability => 176,
            Self::AdminReviewedPlanBinding => 177,
            Self::AdminServerAuthorization => 178,
            Self::AdminPrivateKeyExclusion => 179,
            Self::AdminOverrideSeparation => 180,
            Self::AdminDataPlaneIndependence => 181,
            Self::AdminVerifiedCompletion => 182,
            Self::DiagnosticNonAuthoritative => 183,
            Self::DiagnosticSecretExclusion => 184,
            Self::DiagnosticManifestCompleteness => 185,
            Self::DiagnosticReplaySideEffectIsolation => 186,
            Self::DiagnosticCollectionBounds => 187,
            Self::DiagnosticEvidenceConfidence => 188,
            Self::DiagnosticPrePublicationSanitization => 189,
            Self::DiagnosticVerifiedBundle => 190,
            Self::DiagnosticReplayProductionIsolation => 191,
            Self::LegacySingleWriteOwner => 192,
            Self::LegacyCursorAfterDurability => 193,
            Self::LegacyBridgeIdempotency => 194,
            Self::LegacyPostCutoverFence => 195,
            Self::LegacyTypedFacade => 196,
            Self::LegacyMappingFailClosed => 197,
            Self::LegacyShadowIsolation => 198,
            Self::LegacyGovernanceCoverage => 199,
            Self::LegacyVerifiedCutover => 200,
            Self::SecurityServerValidatedClaims => 201,
            Self::SecurityPayloadImmutability => 202,
            Self::SecurityCapabilityFailClosed => 203,
            Self::SecurityExternalInputBounds => 204,
            Self::SecurityTenantIsolation => 205,
            Self::SecuritySecretExclusion => 206,
            Self::SecurityAuthorityRollback => 207,
            Self::SecuritySideEffectSafety => 208,
            Self::SecurityAdminOverride => 209,
            Self::SecurityIntegrationDistrust => 210,
            Self::FeedIndependentCursor => 211,
            Self::FeedDurableEffectBeforeCursor => 212,
            Self::FeedConsumerIsolation => 213,
            Self::FeedDuplicateIdempotency => 214,
            Self::FeedRetentionRecovery => 215,
            Self::FeedExternalProjectionSafety => 216,
            Self::FeedOrderingSafety => 217,
            Self::FeedResetSafety => 218,
            Self::FeedGovernanceCoverage => 219,
            Self::RegistryIdNonReuse => 220,
            Self::RegistryCanonicalResolution => 221,
            Self::RegistryBreakingChangePath => 222,
            Self::RegistryGeneratedSourceParity => 223,
            Self::RegistryHistoricalReservation => 224,
            Self::RegistryNamespaceIsolation => 225,
            Self::RegistrySecurityReview => 226,
            Self::RegistryRuntimeImmutability => 227,
            Self::RegistryHistoricalResolution => 228,
            Self::CertificationSemanticOnly => 229,
            Self::CertificationTierTruthfulness => 230,
            Self::CertificationExactBinding => 231,
            Self::CertificationCapabilityTruthfulness => 232,
            Self::CertificationRuntimeValidation => 233,
            Self::CertificationArtifactImmutability => 234,
            Self::CertificationEvidenceIntegrity => 235,
            Self::CertificationCorrectnessPriority => 236,
            Self::CertificationLifecycleIdentity => 237,
            Self::MobileDurableProcessIndependence => 238,
            Self::MobileIntentAtomicity => 239,
            Self::MobilePushHintOnly => 240,
            Self::MobileCursorCheckpointSafety => 241,
            Self::MobileSecureKeyStorage => 242,
            Self::MobileResourceSemanticSafety => 243,
            Self::MobileUpgradeIntentSafety => 244,
            Self::MobileStorageTruthfulness => 245,
            Self::MobilePlatformBoundary => 246,
            Self::DesktopSingleCoordinator => 247,
            Self::DesktopStaleFenceSafety => 248,
            Self::DesktopResumeIdempotency => 249,
            Self::DesktopIpcBoundary => 250,
            Self::DesktopCloneBindingSafety => 251,
            Self::DesktopUpgradeIntentSafety => 252,
            Self::DesktopModeParity => 253,
            Self::DesktopPersistenceTruthfulness => 254,
            Self::DesktopDerivedStateSafety => 255,
            Self::StorageLocalIntentAtomicity => 256,
            Self::StorageCriticalIntentRetention => 257,
            Self::StorageCacheIsolation => 258,
            Self::StorageCloneBindingSafety => 259,
            Self::StorageDowngradeSafety => 260,
            Self::StoragePlatformCertification => 261,
            Self::StoragePublicationSafety => 262,
            Self::StorageAdmissionTruthfulness => 263,
            Self::StorageSecretIsolation => 264,
            Self::StorageFilesystemSafety => 265,
            Self::ImplementationFoundationIsolation => 266,
            Self::ImplementationStorageTypeIsolation => 267,
            Self::ImplementationClientUiIsolation => 268,
            Self::ImplementationServerHttpIsolation => 269,
            Self::ImplementationAdapterDirection => 270,
            Self::ImplementationPlatformIsolation => 271,
            Self::ImplementationFeatureIsolation => 272,
            Self::ImplementationCompositionRoot => 273,
            Self::ImplementationRegistryDeterminism => 274,
            Self::ImplementationDependencyGraph => 275,
            Self::SdkCursorIsolation => 276,
            Self::SdkLocalCommitDistinction => 277,
            Self::SdkStorageNeutrality => 278,
            Self::SdkCancellationSafety => 279,
            Self::SdkEventLossSafety => 280,
            Self::SdkErrorCompatibility => 281,
            Self::SdkClosedCoreSemantics => 282,
            Self::SdkVersionIndependence => 283,
            Self::SdkDangerousOperationIsolation => 284,
            Self::SdkSemverAutomation => 285,
            Self::AdapterCapabilityTruthfulness => 286,
            Self::AdapterLocalAtomicity => 287,
            Self::AdapterAuthorityAtomicity => 288,
            Self::AdapterTypeIsolation => 289,
            Self::AdapterPayloadBinding => 290,
            Self::AdapterMigrationSafety => 291,
            Self::AdapterStartupSafety => 292,
            Self::AdapterEnvironmentBinding => 293,
            Self::AdapterCriticalDurability => 294,
            Self::AdapterManifestTruthfulness => 295,
            Self::PostgresAuthorityAtomicity => 296,
            Self::PostgresIdempotentReplay => 297,
            Self::PostgresPayloadBinding => 298,
            Self::PostgresCommittedTimeline => 299,
            Self::PostgresRestoreEpoch => 300,
            Self::PostgresTypeIsolation => 301,
            Self::PostgresReadinessSafety => 302,
            Self::PostgresSideEffectIntent => 303,
            Self::PostgresRetentionSafety => 304,
            Self::PostgresNeonSemanticParity => 305,
            Self::StoolapLocalAtomicity => 306,
            Self::StoolapReconciliationAtomicity => 307,
            Self::StoolapRetryIdentity => 308,
            Self::StoolapIntentPreservation => 309,
            Self::StoolapCursorSafety => 310,
            Self::StoolapFencedCoordinator => 311,
            Self::StoolapCloneSafety => 312,
            Self::StoolapStoragePressureSafety => 313,
            Self::StoolapTypeIsolation => 314,
            Self::StoolapPlatformCertification => 315,
            Self::AxumTransportIsolation => 316,
            Self::AxumCredentialIsolation => 317,
            Self::AxumResourceBounds => 318,
            Self::AxumCommitDeliveryIndependence => 319,
            Self::AxumOverloadBounds => 320,
            Self::AxumIdentityBinding => 321,
            Self::AxumStableSemantics => 322,
            Self::AxumLiveHintDurability => 323,
            Self::AxumNodeEpochIndependence => 324,
            Self::AxumErrorSanitization => 325,
            Self::DioxusDurableStateOwnership => 326,
            Self::DioxusLocalCommitTruth => 327,
            Self::DioxusAuthorityDistinction => 328,
            Self::DioxusEventLossSafety => 329,
            Self::DioxusUnmountSafety => 330,
            Self::DioxusStoreIsolation => 331,
            Self::DioxusConflictSemantics => 332,
            Self::DioxusOfflineCapability => 333,
            Self::DioxusBackgroundOwnership => 334,
            Self::DioxusEventBounds => 335,
            Self::CliNoInvariantBypass => 336,
            Self::CliMachineOutputVersioning => 337,
            Self::CliSensitiveRedaction => 338,
            Self::CliPlanApplySafety => 339,
            Self::CliSubmissionAmbiguity => 340,
            Self::CliStoreOwnership => 341,
            Self::CliMigrationSafety => 342,
            Self::CliSemanticMutation => 343,
            Self::CliProductionGuard => 344,
            Self::CliCanonicalSemantics => 345,
            Self::SqliteWalDurability => 346,
            Self::SqliteSingleWriter => 347,
            Self::SqliteCursorAtomicity => 348,
            Self::SqliteOutboxPreservation => 349,
            Self::SqliteAdapterParity => 350,
            Self::ConfigCorrectnessPreservation => 351,
            Self::ConfigSecretRedaction => 352,
            Self::ConfigValidatedPublication => 353,
            Self::ConfigAtomicReload => 354,
            Self::ConfigDurableIdentityIsolation => 355,
            Self::ConfigFeatureSemanticSafety => 356,
            Self::ConfigProductionSafety => 357,
            Self::ConfigAdapterCapabilitySafety => 358,
            Self::ConfigFailedReloadPreservation => 359,
            Self::ConfigAuthoritativePolicy => 360,
            Self::ReleaseTraceability => 361,
            Self::ReleaseArtifactImmutability => 362,
            Self::ReleaseSignatureIntegrity => 363,
            Self::ReleaseCompatibilityGate => 364,
            Self::ReleaseClientIntentPreservation => 365,
            Self::ReleaseRollbackSafety => 366,
            Self::ReleaseImmutablePromotion => 367,
            Self::ReleaseCredentialIsolation => 368,
            Self::ReleaseUpdateFailClosed => 369,
            Self::ReleaseVersionDimensionSeparation => 370,
            Self::DeploymentSingleWriter => 371,
            Self::DeploymentSemanticPreservation => 372,
            Self::DeploymentNodeEpochIndependence => 373,
            Self::DeploymentDatabaseCredentialIsolation => 374,
            Self::DeploymentRegionalReadSafety => 375,
            Self::DeploymentAirGapIndependence => 376,
            Self::DeploymentRetrySafety => 377,
            Self::DeploymentRestoreEpoch => 378,
            Self::DeploymentDurableStateOwnership => 379,
            Self::DeploymentInfrastructureIndependence => 380,
        }
    }
}

impl fmt::Display for InvariantId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for InvariantId {
    type Err = UnknownInvariant;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|invariant| invariant.as_str() == value)
            .ok_or_else(|| UnknownInvariant(value.to_owned()))
    }
}

/// Error returned when a trace or diagnostic names an unknown invariant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnknownInvariant(String);

impl fmt::Display for UnknownInvariant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown Aequora invariant {:?}", self.0)
    }
}

impl std::error::Error for UnknownInvariant {}

/// Cross-layer evidence expected for one normative invariant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvariantEntry {
    /// Stable invariant identifier.
    pub id: InvariantId,
    /// Human-readable normative safety statement.
    pub description: &'static str,
    /// Assertion name used by the abstract model.
    pub model_assertion: &'static str,
    /// Property or scenario suite that exercises the invariant.
    pub property_test: &'static str,
    /// Adapter conformance or transaction test that exercises the invariant.
    pub adapter_test: &'static str,
    /// Payload-free production signal suitable for operational diagnostics.
    pub diagnostic: &'static str,
}

/// Complete minimum registry required by `01-formal-correctness.md`.
pub static REGISTRY: [InvariantEntry; 381] = [
    entry(
        InvariantId::IdempotentAuthority,
        "authority_idempotency",
        "duplicate_and_lost_response",
        "verify_authoritative_store",
        "duplicate_operation_effect_total",
    ),
    entry(
        InvariantId::LocalIntentAtomicity,
        "local_intent_atomicity",
        "local_mutation_preserves_intent",
        "local_transaction_boundary",
        "local_intent_gap_total",
    ),
    entry(
        InvariantId::AuthoritativePublicationAtomicity,
        "authority_publication_atomicity",
        "commit_failpoint_matrix",
        "authoritative_transaction_boundary",
        "publication_gap_total",
    ),
    entry(
        InvariantId::CursorSafety,
        "cursor_safety",
        "cursor_never_skips_event",
        "verify_local_store",
        "cursor_gap_total",
    ),
    entry(
        InvariantId::VersionMonotonicity,
        "version_monotonicity",
        "two_client_conflict",
        "authoritative_version_race",
        "version_regression_total",
    ),
    entry(
        InvariantId::NoUnauthorizedCommit,
        "authorization_safety",
        "unauthorized_operation_rejected",
        "executor_authorization_suite",
        "unauthorized_commit_total",
    ),
    entry(
        InvariantId::RetryPreservation,
        "retry_preservation",
        "duplicate_and_lost_response",
        "verify_authoritative_store",
        "retry_duplicate_effect_total",
    ),
    entry(
        InvariantId::ReconciliationIdempotency,
        "reconciliation_idempotency",
        "duplicate_response_reconcile",
        "verify_local_store",
        "duplicate_reconcile_total",
    ),
    entry(
        InvariantId::TombstoneSafety,
        "tombstone_safety",
        "stale_write_after_tombstone",
        "adapter_tombstone_suite",
        "stale_resurrection_total",
    ),
    entry(
        InvariantId::TimelineSafety,
        "timeline_safety",
        "incompatible_epoch_rejected",
        "adapter_epoch_suite",
        "epoch_mismatch_total",
    ),
    entry(
        InvariantId::UniqueEventIdentity,
        "unique_event_identity",
        "lineage_event_identity",
        "verify_authoritative_store",
        "duplicate_event_id_total",
    ),
    entry(
        InvariantId::EventCorrelation,
        "event_correlation",
        "lineage_event_correlation",
        "verify_authoritative_store",
        "missing_event_correlation_total",
    ),
    entry(
        InvariantId::RetryLineagePreservation,
        "retry_lineage_preservation",
        "lineage_retry_preservation",
        "verify_authoritative_store",
        "retry_lineage_mismatch_total",
    ),
    entry(
        InvariantId::DerivedLineagePreservation,
        "derived_lineage_preservation",
        "lineage_derivation",
        "verify_authoritative_store",
        "derived_lineage_mismatch_total",
    ),
    entry(
        InvariantId::TenantLineageSafety,
        "tenant_lineage_safety",
        "lineage_tenant_isolation",
        "correlation_query_tenant_boundary",
        "cross_tenant_lineage_total",
    ),
    entry(
        InvariantId::CausalityAcyclic,
        "causality_acyclic",
        "lineage_causality_acyclic",
        "dependency_and_lineage_validation",
        "causality_cycle_total",
    ),
    entry(
        InvariantId::AntiEntropyRootAgreement,
        "anti_entropy_root_agreement",
        "canonical_root_determinism",
        "verify_integrity_adapter",
        "integrity_root_mismatch_total",
    ),
    entry(
        InvariantId::RepairAuthoritySafety,
        "repair_authority_safety",
        "repair_is_replica_only",
        "verify_replica_repair",
        "repair_authority_mutation_total",
    ),
    entry(
        InvariantId::RepairIntentPreservation,
        "repair_intent_preservation",
        "repair_preserves_pending_intent",
        "verify_replica_repair",
        "repair_intent_loss_total",
    ),
    entry(
        InvariantId::SameVersionDigestSafety,
        "same_version_digest_safety",
        "same_version_mismatch_quarantines",
        "verify_integrity_adapter",
        "same_version_digest_mismatch_total",
    ),
    entry(
        InvariantId::IntegrityCursorIsolation,
        "integrity_cursor_isolation",
        "repair_does_not_advance_cursor",
        "verify_replica_repair",
        "integrity_cursor_advance_total",
    ),
    entry(
        InvariantId::QueueSemanticEquivalence,
        "queue_semantic_equivalence",
        "compacted_sequence_matches_original",
        "verify_queue_compaction",
        "queue_semantic_mismatch_total",
    ),
    entry(
        InvariantId::DeliveredPayloadImmutability,
        "delivered_payload_immutability",
        "lost_response_then_compaction",
        "verify_queue_immutability",
        "queue_immutable_payload_mismatch_total",
    ),
    entry(
        InvariantId::QueueDependencySafety,
        "queue_dependency_safety",
        "dependency_rewrite_model",
        "verify_queue_dependencies",
        "queue_dependency_corruption_total",
    ),
    entry(
        InvariantId::QueueIntentPreservation,
        "queue_intent_preservation",
        "storage_pressure_preserves_intent",
        "verify_queue_compaction",
        "queue_intent_loss_total",
    ),
    entry(
        InvariantId::RebaseIdentitySafety,
        "rebase_identity_safety",
        "rebase_after_bootstrap",
        "verify_queue_rebase",
        "rebase_identity_violation_total",
    ),
    entry(
        InvariantId::SensitiveOperationSafety,
        "sensitive_operation_safety",
        "finance_operations_never_compact_by_default",
        "verify_queue_compaction",
        "sensitive_compaction_total",
    ),
    entry(
        InvariantId::LocalFenceUniqueness,
        "single_current_fencing_epoch",
        "simultaneous_candidates",
        "verify_local_coordination",
        "local_leadership_acquired_total",
    ),
    entry(
        InvariantId::StaleLeaderCommitSafety,
        "stale_leader_cannot_commit",
        "crash_takeover_fences_commit",
        "verify_local_coordination",
        "local_stale_fences_rejected_total",
    ),
    entry(
        InvariantId::FollowerMutationSafety,
        "follower_writes_preserve_outbox",
        "follower_local_mutation",
        "verify_local_store",
        "local_intent_gap_total",
    ),
    entry(
        InvariantId::LocalLeadershipLiveness,
        "expired_lease_allows_takeover",
        "lease_expiry_takeover",
        "verify_local_coordination",
        "local_leadership_lost_total",
    ),
    entry(
        InvariantId::LeadershipIntentPreservation,
        "leadership_preserves_pending_intent",
        "takeover_preserves_outbox",
        "verify_local_coordination",
        "leadership_intent_mismatch_total",
    ),
    entry(
        InvariantId::StoreGenerationSafety,
        "generation_invalidates_old_grant",
        "maintenance_generation_switch",
        "verify_local_coordination",
        "local_generation_mismatch_total",
    ),
    entry(
        InvariantId::QosSemanticSafety,
        "scheduler_semantic_immutability",
        "scheduler_preserves_descriptor",
        "scheduler_deterministic_contract",
        "scheduler_semantic_mutation_total",
    ),
    entry(
        InvariantId::QosIntentPreservation,
        "scheduler_deferral_preserves_intent",
        "constrained_context_defers_only",
        "verify_local_store",
        "scheduler_intent_loss_total",
    ),
    entry(
        InvariantId::QosBatchBounds,
        "scheduler_hard_batch_bounds",
        "adaptive_batch_never_crosses_hard_bounds",
        "scheduler_deterministic_contract",
        "scheduler_batch_bound_violation_total",
    ),
    entry(
        InvariantId::QosStarvationFreedom,
        "scheduler_weighted_fairness",
        "weighted_cycle_serves_background",
        "scheduler_deterministic_contract",
        "scheduler_starvation_alert_total",
    ),
    entry(
        InvariantId::QosServerHintSafety,
        "server_hints_only_reduce_bounds",
        "server_hint_cannot_expand_or_drop",
        "scheduler_deterministic_contract",
        "scheduler_server_hint_violation_total",
    ),
    entry(
        InvariantId::QosLeadershipRetrySafety,
        "leadership_preserves_retry_state",
        "scheduler_state_restart_takeover",
        "verify_local_coordination",
        "scheduler_retry_state_loss_total",
    ),
    entry(
        InvariantId::ScopeCursorBinding,
        "scope_cursor_exact_binding",
        "cursor_identity_version_generation",
        "verify_scope_state_store",
        "scope_cursor_mismatch_total",
    ),
    entry(
        InvariantId::ScopeAuthorizationSafety,
        "scope_authorized_projection_only",
        "resolver_fails_closed",
        "scope_resolver_contract",
        "scope_authorization_failure_total",
    ),
    entry(
        InvariantId::ScopeContractionSafety,
        "scope_contraction_atomic_activation",
        "contraction_deactivates_before_version",
        "verify_scope_state_store",
        "scope_contraction_failure_total",
    ),
    entry(
        InvariantId::ScopeRemovalDistinction,
        "scope_removal_not_tombstone",
        "scope_removal_preserves_authority",
        "verify_scope_state_store",
        "scope_removal_domain_delete_total",
    ),
    entry(
        InvariantId::ScopeSharedMembershipSafety,
        "shared_membership_reference_safety",
        "multi_scope_reference_count",
        "verify_scope_state_store",
        "scope_shared_entity_loss_total",
    ),
    entry(
        InvariantId::ScopeRevokedIntentSafety,
        "revoked_intent_quarantine",
        "revocation_pending_operation",
        "verify_scope_state_store",
        "scope_revoked_transmission_total",
    ),
    entry(
        InvariantId::ScopeExpansionAtomicity,
        "scope_expansion_atomic_activation",
        "interrupted_expansion_inactive",
        "verify_scope_state_store",
        "scope_partial_activation_total",
    ),
    entry(
        InvariantId::LiveHintLossSafety,
        "live_hint_loss_preserves_convergence",
        "lossy_hints_match_polling_model",
        "verify_live_hint_service",
        "live_fallback_poll_total",
    ),
    entry(
        InvariantId::LiveNoStateMutation,
        "live_hint_has_no_replica_mutator",
        "hint_only_changes_wake_generation",
        "verify_live_hint_service",
        "live_direct_cursor_mutation_total",
    ),
    entry(
        InvariantId::LiveScopeIsolation,
        "live_scope_authorization",
        "tenant_scope_fanout_isolation",
        "verify_live_hint_service",
        "live_scope_isolation_failure_total",
    ),
    entry(
        InvariantId::LiveBackpressureBound,
        "live_queue_hard_bound",
        "slow_consumer_latest_only_queue",
        "verify_live_hint_service",
        "live_hint_drop_total",
    ),
    entry(
        InvariantId::LiveLeadershipCatchUp,
        "live_leader_fencing_and_catchup",
        "leader_handoff_requires_exchange",
        "verify_live_hint_service",
        "live_stale_leader_total",
    ),
    entry(
        InvariantId::ImportIdentityDeterminism,
        "import_identity_determinism",
        "restart_maps_same_source_identity",
        "verify_checkpointed_import_sink",
        "import_identity_drift_total",
    ),
    entry(
        InvariantId::ImportCheckpointSafety,
        "import_checkpoint_after_commit",
        "batch_failpoint_checkpoint_model",
        "verify_checkpointed_import_sink",
        "import_checkpoint_violation_total",
    ),
    entry(
        InvariantId::ImportCutoverVerification,
        "import_cutover_verification",
        "cutover_gate_matrix",
        "verify_authority_import_sink",
        "import_cutover_blocked_total",
    ),
    entry(
        InvariantId::ImportBaselineSafety,
        "import_baseline_without_synthetic_history",
        "baseline_seed_model",
        "verify_authority_import_sink",
        "import_synthetic_history_total",
    ),
    entry(
        InvariantId::ImportJournalVisibility,
        "import_live_change_journal_visibility",
        "post_cutover_bridge_change_model",
        "verify_authority_import_sink",
        "import_journal_gap_total",
    ),
    entry(
        InvariantId::ImportQuarantineSafety,
        "import_quarantine_not_success",
        "quarantine_threshold_model",
        "verify_checkpointed_import_sink",
        "import_quarantine_miscount_total",
    ),
    entry(
        InvariantId::BootstrapCursorActivationSafety,
        "bootstrap_cursor_after_activation",
        "activation_crash_model",
        "verify_snapshot_sink",
        "bootstrap_cursor_violation_total",
    ),
    entry(
        InvariantId::BootstrapBoundaryConsistency,
        "bootstrap_single_boundary",
        "manifest_chunk_boundary_property",
        "verify_snapshot_source",
        "bootstrap_boundary_mismatch_total",
    ),
    entry(
        InvariantId::BootstrapStagingIsolation,
        "bootstrap_staging_invisible",
        "partial_generation_model",
        "verify_snapshot_sink",
        "bootstrap_partial_visibility_total",
    ),
    entry(
        InvariantId::BootstrapChunkIdempotency,
        "bootstrap_chunk_idempotency",
        "duplicate_chunk_property",
        "verify_snapshot_sink",
        "bootstrap_duplicate_entity_total",
    ),
    entry(
        InvariantId::BootstrapIntentPreservation,
        "bootstrap_pending_intent_preserved",
        "pending_digest_activation_model",
        "verify_snapshot_sink",
        "bootstrap_pending_intent_loss_total",
    ),
    entry(
        InvariantId::BootstrapDeltaConvergence,
        "bootstrap_delta_convergence",
        "snapshot_plus_delta_differential",
        "verify_snapshot_source_and_sink",
        "bootstrap_delta_gap_total",
    ),
    entry(
        InvariantId::ProfileOperationCompatibility,
        "profile_operation_compatibility",
        "profile_operation_matrix",
        "verify_profile_registry",
        "profile_operation_incompatible_total",
    ),
    entry(
        InvariantId::ProfileCapabilitySafety,
        "profile_capability_safety",
        "profile_capability_matrix",
        "verify_profile_capabilities",
        "profile_capability_missing_total",
    ),
    entry(
        InvariantId::ProfileImmutableAppendOnly,
        "profile_immutable_append_only",
        "append_only_intent_property",
        "verify_profile_registry",
        "profile_append_only_violation_total",
    ),
    entry(
        InvariantId::ProfileStrongAggregateAtomicity,
        "profile_strong_aggregate_atomicity",
        "strong_aggregate_capability_property",
        "verify_profile_capabilities",
        "profile_atomicity_violation_total",
    ),
    entry(
        InvariantId::ProfileDerivedAuthoritySafety,
        "profile_derived_authority_safety",
        "derived_operation_origin_property",
        "verify_profile_registry",
        "profile_derived_authority_total",
    ),
    entry(
        InvariantId::ProfileNoImplicitLastWriterWins,
        "profile_unknown_operation_fail_closed",
        "unknown_operation_property",
        "verify_profile_registry",
        "profile_unknown_operation_total",
    ),
    entry(
        InvariantId::DeterministicPlanEquivalence,
        "deterministic_plan_equivalence",
        "replay_same_bundle_property",
        "verify_replay_handler",
        "handler_determinism_failure_total",
    ),
    entry(
        InvariantId::DeterministicInputCapture,
        "deterministic_input_capture",
        "clock_random_id_capture_property",
        "verify_replay_handler",
        "replay_uncaptured_input_total",
    ),
    entry(
        InvariantId::ReplaySideEffectIsolation,
        "replay_side_effect_isolation",
        "recording_side_effect_sink_property",
        "verify_replay_handler",
        "replay_side_effect_attempt_total",
    ),
    entry(
        InvariantId::ReplayCommittedInputImmutability,
        "replay_committed_input_immutability",
        "commit_ambiguity_failpoint",
        "verify_plan_committer",
        "replay_committed_input_drift_total",
    ),
    entry(
        InvariantId::ReplayHandlerVersionSafety,
        "replay_handler_version_safety",
        "differential_handler_corpus",
        "verify_replay_handler",
        "replay_handler_version_mismatch_total",
    ),
    entry(
        InvariantId::ReplayPolicyVersionSafety,
        "replay_policy_version_safety",
        "policy_config_capture_property",
        "verify_replay_handler",
        "replay_policy_version_missing_total",
    ),
    entry(
        InvariantId::AuditRequiredAtomicity,
        "audit_required_atomicity",
        "required_audit_commit_property",
        "verify_audit_committer",
        "audit_required_missing_total",
    ),
    entry(
        InvariantId::AuditRetryDeduplication,
        "audit_retry_deduplication",
        "audit_retry_identity_property",
        "verify_audit_committer",
        "audit_duplicate_effect_total",
    ),
    entry(
        InvariantId::AuditActorTruthfulness,
        "audit_actor_truthfulness",
        "system_actor_property",
        "verify_audit_handler",
        "audit_actor_misattribution_total",
    ),
    entry(
        InvariantId::AuditAppendOnlyCorrection,
        "audit_append_only_correction",
        "audit_correction_property",
        "verify_audit_repository",
        "audit_history_rewrite_total",
    ),
    entry(
        InvariantId::AuditSensitiveValueSafety,
        "audit_sensitive_value_safety",
        "audit_value_policy_property",
        "verify_audit_repository",
        "audit_sensitive_value_violation_total",
    ),
    entry(
        InvariantId::AuditExplanationAuthorization,
        "audit_explanation_authorization",
        "audit_query_authorization_property",
        "verify_audit_repository",
        "audit_query_forbidden_total",
    ),
    entry(
        InvariantId::AuditChainContinuity,
        "audit_chain_continuity",
        "audit_chain_append_property",
        "verify_audit_chain",
        "audit_chain_gap_total",
    ),
    entry(
        InvariantId::AuditTamperDetection,
        "audit_tamper_detection",
        "audit_chain_tamper_property",
        "verify_audit_chain",
        "audit_integrity_failure_total",
    ),
    entry(
        InvariantId::AuditFieldProvenanceAccuracy,
        "audit_field_provenance_accuracy",
        "field_provenance_latest_property",
        "verify_audit_repository",
        "audit_field_provenance_stale_total",
    ),
    entry(
        InvariantId::GovernanceTombstoneGcSafety,
        "governance_tombstone_gc_safety",
        "tombstone_watermark_property",
        "verify_governance_store",
        "governance_unsafe_tombstone_gc_total",
    ),
    entry(
        InvariantId::GovernanceJournalFloorSafety,
        "governance_journal_floor_safety",
        "journal_floor_property",
        "verify_governance_store",
        "governance_cursor_below_floor_total",
    ),
    entry(
        InvariantId::GovernanceLegalHoldPrecedence,
        "governance_legal_hold_precedence",
        "legal_hold_property",
        "verify_governance_store",
        "governance_held_purge_total",
    ),
    entry(
        InvariantId::GovernanceRequiredEvidenceSafety,
        "governance_required_evidence_safety",
        "erasure_minimization_property",
        "verify_governance_store",
        "governance_required_evidence_loss_total",
    ),
    entry(
        InvariantId::GovernanceSurfaceCompletion,
        "governance_surface_completion",
        "partial_surface_failpoint",
        "verify_governance_registry",
        "governance_partial_total",
    ),
    entry(
        InvariantId::GovernanceOffboardingWriteFence,
        "governance_offboarding_write_fence",
        "tenant_lifecycle_property",
        "verify_governance_store",
        "governance_write_after_offboarding_total",
    ),
    entry(
        InvariantId::GovernanceRestoreReconciliation,
        "governance_restore_reconciliation",
        "restore_gate_property",
        "verify_restore_governance",
        "governance_restore_gate_failure_total",
    ),
    entry(
        InvariantId::GovernanceResurrectionSafety,
        "governance_resurrection_safety",
        "retired_client_property",
        "verify_governance_store",
        "governance_resurrection_attempt_total",
    ),
    entry(
        InvariantId::GovernanceCopyCoverage,
        "governance_copy_coverage",
        "copy_registry_property",
        "verify_governance_registry",
        "governance_unregistered_copy_total",
    ),
    entry(
        InvariantId::CryptoArtifactAuthenticity,
        "crypto_artifact_authenticity",
        "signed_artifact_tamper_property",
        "verify_crypto_provider",
        "crypto_signature_verify_failure_total",
    ),
    entry(
        InvariantId::CryptoTenantBinding,
        "crypto_tenant_binding",
        "cross_tenant_swap_property",
        "verify_crypto_provider",
        "crypto_tenant_mismatch_total",
    ),
    entry(
        InvariantId::CryptoRotationHistory,
        "crypto_rotation_history",
        "historical_key_rotation_property",
        "verify_key_registry",
        "crypto_rotation_history_failure_total",
    ),
    entry(
        InvariantId::CryptoRevocationSafety,
        "crypto_revocation_safety",
        "revoked_key_use_property",
        "verify_key_provider",
        "crypto_revoked_key_use_total",
    ),
    entry(
        InvariantId::CryptoSecretHandling,
        "crypto_secret_handling",
        "secret_redaction_property",
        "verify_key_provider",
        "crypto_secret_boundary_failure_total",
    ),
    entry(
        InvariantId::CryptoFailClosed,
        "crypto_fail_closed",
        "crypto_negative_vectors",
        "verify_crypto_provider",
        "crypto_verification_failure_total",
    ),
    entry(
        InvariantId::CryptoE2eSemanticBoundary,
        "crypto_e2e_semantic_boundary",
        "opaque_domain_policy_property",
        "verify_protected_domain",
        "crypto_e2e_policy_rejection_total",
    ),
    entry(
        InvariantId::CryptoErasureCompletion,
        "crypto_erasure_completion",
        "key_copy_erasure_property",
        "verify_governance_crypto_store",
        "crypto_erasure_incomplete_total",
    ),
    entry(
        InvariantId::CryptoSignedOperationImmutability,
        "crypto_signed_operation_immutability",
        "signed_operation_drift_property",
        "verify_operation_ledger",
        "crypto_signed_operation_drift_total",
    ),
    entry(
        InvariantId::AuthoritySingleWriter,
        "authority_single_writer",
        "authority_fence_property",
        "verify_authority_guard",
        "authority_dual_writer_detected_total",
    ),
    entry(
        InvariantId::AuthorityCursorEpochBinding,
        "authority_cursor_epoch_binding",
        "cross_epoch_cursor_property",
        "verify_cursor_binding",
        "authority_cursor_epoch_mismatch_total",
    ),
    entry(
        InvariantId::AuthorityDivergenceEpoch,
        "authority_divergence_epoch",
        "promotion_epoch_decision_property",
        "verify_promotion_plan",
        "authority_unsafe_epoch_reuse_total",
    ),
    entry(
        InvariantId::AuthorityRollbackSafety,
        "authority_rollback_safety",
        "client_epoch_rollback_property",
        "verify_client_authority_state",
        "authority_rollback_detected_total",
    ),
    entry(
        InvariantId::AuthorityNoAutomaticForkMerge,
        "authority_no_automatic_fork_merge",
        "fork_checkpoint_property",
        "verify_fork_quarantine",
        "authority_fork_detected_total",
    ),
    entry(
        InvariantId::AuthorityOperationRecoveryPolicy,
        "authority_operation_recovery_policy",
        "ambiguous_operation_policy_property",
        "verify_epoch_recovery_registry",
        "authority_ambiguous_operations_total",
    ),
    entry(
        InvariantId::AuthorityOldPrimaryFence,
        "authority_old_primary_fence",
        "old_primary_reappearance_property",
        "verify_authority_commit_fence",
        "authority_stale_fence_rejection_total",
    ),
    entry(
        InvariantId::AuthorityLosslessEpochContinuity,
        "authority_lossless_epoch_continuity",
        "lossless_promotion_property",
        "verify_replication_checkpoint",
        "authority_unnecessary_epoch_change_total",
    ),
    entry(
        InvariantId::AuthorityArtifactEpochBinding,
        "authority_artifact_epoch_binding",
        "artifact_timeline_property",
        "verify_authority_artifacts",
        "authority_artifact_epoch_mismatch_total",
    ),
    entry(
        InvariantId::RegionSingleWriter,
        "regional_single_writer",
        "regional_write_route_property",
        "verify_regional_write_guard",
        "regional_non_authority_write_total",
    ),
    entry(
        InvariantId::RegionAtLeastWatermark,
        "regional_at_least_watermark",
        "replica_watermark_property",
        "verify_replica_read_guard",
        "replica_too_stale_total",
    ),
    entry(
        InvariantId::RegionSessionMonotonicity,
        "regional_session_monotonicity",
        "read_your_writes_property",
        "verify_session_read_routing",
        "regional_session_regression_total",
    ),
    entry(
        InvariantId::RegionEpochIsolation,
        "regional_epoch_isolation",
        "wrong_epoch_replica_property",
        "verify_epoch_cache_generation",
        "regional_wrong_epoch_total",
    ),
    entry(
        InvariantId::RegionFailureAuthoritySafety,
        "regional_failure_authority_safety",
        "region_partition_property",
        "verify_authority_only_writes",
        "regional_alternative_writer_total",
    ),
    entry(
        InvariantId::RegionFallbackSafety,
        "regional_fallback_safety",
        "fallback_policy_property",
        "verify_regional_router",
        "regional_silent_downgrade_total",
    ),
    entry(
        InvariantId::RegionArtifactIntegrity,
        "regional_artifact_integrity",
        "edge_artifact_property",
        "verify_regional_artifact",
        "regional_artifact_verify_failure_total",
    ),
    entry(
        InvariantId::RegionResidencyCoverage,
        "regional_residency_coverage",
        "residency_placement_property",
        "verify_residency_inventory",
        "regional_residency_violation_total",
    ),
    entry(
        InvariantId::RegionGovernanceCoverage,
        "regional_governance_coverage",
        "regional_erasure_property",
        "verify_regional_governance_registry",
        "regional_governance_incomplete_total",
    ),
    entry(
        InvariantId::LoadBoundedQueues,
        "bounded_overload_queues",
        "queue_capacity_property",
        "verify_queue_configuration",
        "admission_queue_overflow_total",
    ),
    entry(
        InvariantId::LoadPreMutationRejection,
        "pre_mutation_overload_rejection",
        "saturation_rejection_property",
        "verify_admission_decorator",
        "admission_post_mutation_rejection_total",
    ),
    entry(
        InvariantId::LoadTenantFairness,
        "tenant_fair_admission",
        "hot_tenant_property",
        "verify_hierarchical_admission",
        "admission_fairness_violation_total",
    ),
    entry(
        InvariantId::LoadSlowClientBound,
        "slow_client_memory_bound",
        "slow_consumer_property",
        "verify_live_queue_bounds",
        "live_slow_client_overflow_total",
    ),
    entry(
        InvariantId::LoadPriorityStarvationSafety,
        "priority_starvation_safety",
        "weighted_aging_property",
        "verify_fair_queue",
        "admission_starvation_total",
    ),
    entry(
        InvariantId::LoadRetryJitterSafety,
        "retry_jitter_safety",
        "retry_storm_property",
        "verify_scheduler_retry_policy",
        "retry_unjittered_guidance_total",
    ),
    entry(
        InvariantId::LoadDurableIntentSafety,
        "durable_intent_outlives_queue",
        "queue_restart_property",
        "verify_durable_job_source",
        "ephemeral_only_required_work_total",
    ),
    entry(
        InvariantId::LoadConsistencySafety,
        "overload_consistency_safety",
        "brownout_consistency_property",
        "verify_brownout_policy",
        "overload_consistency_downgrade_total",
    ),
    entry(
        InvariantId::LoadAdmissionOrdering,
        "resource_admission_ordering",
        "resource_saturation_property",
        "verify_resource_permits",
        "late_admission_total",
    ),
    entry(
        InvariantId::PerformanceBoundedMemory,
        "bounded_external_memory",
        "item_and_byte_capacity_property",
        "verify_performance_policy",
        "performance_bound_rejection_total",
    ),
    entry(
        InvariantId::PerformanceStreamingLargeObjects,
        "streaming_large_objects",
        "chunk_window_property",
        "verify_snapshot_blob_streaming",
        "large_object_materialization_total",
    ),
    entry(
        InvariantId::PerformanceCpuIsolation,
        "bounded_cpu_isolation",
        "compute_submission_property",
        "verify_compute_pool_bounds",
        "compute_saturation_total",
    ),
    entry(
        InvariantId::PerformanceCorrectnessPreservation,
        "optimization_preserves_semantics",
        "optimized_path_equivalence_property",
        "verify_correctness_gates",
        "optimization_invariant_failure_total",
    ),
    entry(
        InvariantId::PerformanceImmutableHotState,
        "immutable_hot_state",
        "registry_generation_property",
        "verify_immutable_registry",
        "hot_state_lock_contention_total",
    ),
    entry(
        InvariantId::PerformanceReproducibility,
        "reproducible_performance_evidence",
        "fixed_workload_phase_property",
        "verify_performance_report",
        "performance_regression_total",
    ),
    entry(
        InvariantId::PerformancePagedUiState,
        "paged_reactive_state",
        "view_window_property",
        "verify_reactive_view_budget",
        "reactive_view_overflow_total",
    ),
    entry(
        InvariantId::PerformanceBlobReferences,
        "streamed_blob_references",
        "blob_chunk_bound_property",
        "verify_blob_streaming",
        "blob_inline_limit_total",
    ),
    entry(
        InvariantId::PerformanceEarlyAdmission,
        "early_resource_rejection",
        "header_limit_property",
        "verify_frame_admission_order",
        "late_frame_rejection_total",
    ),
    entry(
        InvariantId::ClientIntentPreservation,
        "client_intent_preserved",
        "resource_pressure_intent_property",
        "verify_client_eviction_policy",
        "client_intent_discard_total",
    ),
    entry(
        InvariantId::ClientCursorDurability,
        "client_cursor_after_durable_apply",
        "checkpoint_cursor_property",
        "verify_client_checkpoint_order",
        "client_early_cursor_total",
    ),
    entry(
        InvariantId::ClientRequiredDirective,
        "required_directive_retained",
        "security_work_admission_property",
        "verify_client_required_work",
        "client_required_directive_deferred_total",
    ),
    entry(
        InvariantId::ClientBoundedLargeObject,
        "client_large_object_bounded",
        "client_chunk_bound_property",
        "verify_client_streaming_limits",
        "client_large_object_overflow_total",
    ),
    entry(
        InvariantId::ClientCheckpointRecovery,
        "client_checkpoint_recoverable",
        "process_kill_checkpoint_property",
        "verify_client_restart_recovery",
        "client_checkpoint_recovery_failure_total",
    ),
    entry(
        InvariantId::ClientPendingBaseEviction,
        "pending_base_pinned",
        "scope_eviction_pin_property",
        "verify_client_scope_eviction",
        "client_pending_base_eviction_total",
    ),
    entry(
        InvariantId::ClientLocalCommitAtomicity,
        "local_save_atomic",
        "local_commit_receipt_property",
        "verify_client_local_commit",
        "client_false_local_save_total",
    ),
    entry(
        InvariantId::ClientSemanticParity,
        "client_profile_semantic_parity",
        "resource_profile_equivalence_property",
        "verify_client_profile_limits",
        "client_resource_semantic_change_total",
    ),
    entry(
        InvariantId::ClientTelemetryPrivacy,
        "client_capability_coarse",
        "capability_privacy_property",
        "verify_client_capability_profile",
        "client_precise_resource_telemetry_total",
    ),
    entry(
        InvariantId::CompatExplicitVersionContext,
        "explicit_version_context",
        "golden_versioned_hello",
        "compatibility_contracts",
        "compat_decode_without_context_total",
    ),
    entry(
        InvariantId::CompatRequiredCapabilitySafety,
        "required_capability_fail_closed",
        "downgrade_required_capability",
        "compatibility_contracts",
        "compat_required_capability_missing_total",
    ),
    entry(
        InvariantId::CompatPossiblySentImmutability,
        "possibly_sent_immutable",
        "retry_payload_immutability",
        "compatibility_contracts",
        "compat_retry_payload_mismatch_total",
    ),
    entry(
        InvariantId::CompatStableIdReservation,
        "removed_ids_reserved",
        "registry_reservation_validation",
        "compatibility_registry",
        "compat_registry_reuse_total",
    ),
    entry(
        InvariantId::CompatFleetActivationSafety,
        "fleet_complete_before_required",
        "mixed_fleet_feature_gate",
        "compatibility_contracts",
        "compat_early_activation_total",
    ),
    entry(
        InvariantId::CompatUpgradeIntentPreservation,
        "upgrade_preserves_intent",
        "upgrade_state_intent_evidence",
        "compatibility_contracts",
        "compat_intent_loss_total",
    ),
    entry(
        InvariantId::CompatRetryHorizon,
        "retry_horizon_or_recovery",
        "retry_only_operation_admission",
        "compatibility_registry",
        "compat_schema_retry_rejected_total",
    ),
    entry(
        InvariantId::CompatAuthorityEpochIndependence,
        "protocol_epoch_independent",
        "session_cache_epoch_invalidation",
        "compatibility_contracts",
        "compat_epoch_conflation_total",
    ),
    entry(
        InvariantId::CompatPolicyAuditability,
        "versioned_auditable_policy",
        "invalid_policy_fail_closed",
        "compatibility_registry",
        "compat_invalid_policy_total",
    ),
    entry(
        InvariantId::MetadataStoreSchemaDeclaration,
        "metadata_schema_declared",
        "metadata_root_schema_gate",
        "metadata_store_startup",
        "metadata_schema_rejection_total",
    ),
    entry(
        InvariantId::MetadataLedgerOperationIdentity,
        "metadata_operation_identity",
        "ledger_payload_mismatch",
        "metadata_ledger_contract",
        "metadata_payload_mismatch_total",
    ),
    entry(
        InvariantId::MetadataClientCursorAtomicity,
        "metadata_cursor_atomicity",
        "cursor_crash_matrix",
        "metadata_local_transaction",
        "metadata_cursor_gap_total",
    ),
    entry(
        InvariantId::MetadataAuthoritativeAtomicity,
        "metadata_authority_atomicity",
        "authoritative_crash_matrix",
        "metadata_authority_transaction",
        "metadata_authority_gap_total",
    ),
    entry(
        InvariantId::MetadataSnapshotPublicationSafety,
        "metadata_snapshot_publication",
        "snapshot_publish_matrix",
        "metadata_snapshot_contract",
        "metadata_snapshot_invalid_total",
    ),
    entry(
        InvariantId::MetadataAdapterSemanticEquivalence,
        "metadata_adapter_equivalence",
        "cross_adapter_export_equivalence",
        "metadata_mapping_contract",
        "metadata_adapter_drift_total",
    ),
    entry(
        InvariantId::MetadataStaleFenceRejection,
        "metadata_stale_fence_rejected",
        "metadata_fencing_race",
        "metadata_fence_contract",
        "metadata_stale_fence_total",
    ),
    entry(
        InvariantId::MetadataMigrationIntentPreservation,
        "metadata_migration_intent",
        "metadata_migration_failure_matrix",
        "metadata_migration_contract",
        "metadata_migration_intent_loss_total",
    ),
    entry(
        InvariantId::MetadataSecretKeyExclusion,
        "metadata_secret_exclusion",
        "metadata_secret_field_scan",
        "metadata_export_contract",
        "metadata_secret_violation_total",
    ),
    entry(
        InvariantId::JobDurableBeforeExecution,
        "job_durable_before_execution",
        "job_insert_crash_matrix",
        "job_store_insert_contract",
        "job_durability_gap_total",
    ),
    entry(
        InvariantId::JobCurrentFenceOnly,
        "job_current_fence_only",
        "double_worker_fencing",
        "job_store_fence_contract",
        "job_stale_fence_total",
    ),
    entry(
        InvariantId::JobCommittedSideEffectIntent,
        "job_committed_side_effect_intent",
        "authoritative_outbox_crash_matrix",
        "side_effect_transaction_contract",
        "side_effect_intent_gap_total",
    ),
    entry(
        InvariantId::JobAuthoritativeResultOperation,
        "job_authoritative_result_operation",
        "provider_result_operation_path",
        "authoritative_result_sink_contract",
        "job_domain_bypass_total",
    ),
    entry(
        InvariantId::JobExplicitAmbiguityRecovery,
        "job_explicit_ambiguity_recovery",
        "provider_timeout_ambiguity",
        "provider_reconciliation_contract",
        "side_effect_ambiguous_total",
    ),
    entry(
        InvariantId::JobDurableState,
        "job_durable_state",
        "worker_restart_recovery",
        "job_workflow_store_contract",
        "job_memory_only_state_total",
    ),
    entry(
        InvariantId::JobStaleWorkerRejection,
        "job_stale_worker_rejection",
        "lease_expiry_reclaim_race",
        "job_store_fence_contract",
        "job_stale_worker_update_total",
    ),
    entry(
        InvariantId::JobBoundedCheckpoint,
        "job_bounded_checkpoint",
        "chunk_crash_resume",
        "job_checkpoint_contract",
        "job_checkpoint_bound_violation_total",
    ),
    entry(
        InvariantId::JobBoundedRetry,
        "job_bounded_retry",
        "provider_outage_backoff",
        "job_retry_contract",
        "job_retry_storm_total",
    ),
    entry(
        InvariantId::AdminNoDomainBypass,
        "admin_no_domain_bypass",
        "admin_subsystem_guard_matrix",
        "admin_executor_contract",
        "admin_domain_bypass_total",
    ),
    entry(
        InvariantId::AdminDurableAttribution,
        "admin_durable_attribution",
        "admin_auth_attribution",
        "admin_store_contract",
        "admin_unattributed_mutation_total",
    ),
    entry(
        InvariantId::AdminPayloadImmutability,
        "admin_payload_immutability",
        "admin_idempotency_payload_drift",
        "admin_store_idempotency_contract",
        "admin_payload_mismatch_total",
    ),
    entry(
        InvariantId::AdminReviewedPlanBinding,
        "admin_reviewed_plan_binding",
        "admin_plan_staleness_matrix",
        "admin_plan_store_contract",
        "admin_stale_plan_total",
    ),
    entry(
        InvariantId::AdminServerAuthorization,
        "admin_server_authorization",
        "admin_role_scope_matrix",
        "admin_authorization_contract",
        "admin_denied_total",
    ),
    entry(
        InvariantId::AdminPrivateKeyExclusion,
        "admin_private_key_exclusion",
        "admin_response_secret_scan",
        "admin_dto_contract",
        "admin_secret_violation_total",
    ),
    entry(
        InvariantId::AdminOverrideSeparation,
        "admin_override_separation",
        "admin_force_action_policy",
        "admin_permission_registry_contract",
        "admin_force_action_total",
    ),
    entry(
        InvariantId::AdminDataPlaneIndependence,
        "admin_data_plane_independence",
        "admin_outage_data_plane",
        "server_plane_separation_contract",
        "admin_dependency_data_plane_total",
    ),
    entry(
        InvariantId::AdminVerifiedCompletion,
        "admin_verified_completion",
        "admin_postcondition_failure",
        "admin_executor_verification_contract",
        "admin_unverified_completion_total",
    ),
    entry(
        InvariantId::DiagnosticNonAuthoritative,
        "diagnostic_non_authoritative",
        "diagnostic_read_only_state",
        "diagnostic_provider_contract",
        "diagnostic_authority_mutation_total",
    ),
    entry(
        InvariantId::DiagnosticSecretExclusion,
        "diagnostic_secret_exclusion",
        "diagnostic_secret_redaction",
        "diagnostic_sanitizer_contract",
        "diagnostic_secret_violation_total",
    ),
    entry(
        InvariantId::DiagnosticManifestCompleteness,
        "diagnostic_manifest_complete",
        "diagnostic_manifest_validation",
        "diagnostic_bundle_contract",
        "diagnostic_manifest_invalid_total",
    ),
    entry(
        InvariantId::DiagnosticReplaySideEffectIsolation,
        "diagnostic_replay_side_effect_isolation",
        "diagnostic_replay_capture_only",
        "diagnostic_replay_contract",
        "diagnostic_real_side_effect_total",
    ),
    entry(
        InvariantId::DiagnosticCollectionBounds,
        "diagnostic_collection_bounds",
        "diagnostic_archive_limit_matrix",
        "diagnostic_provider_bounds_contract",
        "diagnostic_limit_rejection_total",
    ),
    entry(
        InvariantId::DiagnosticEvidenceConfidence,
        "diagnostic_evidence_confidence",
        "diagnostic_explanation_sources",
        "diagnostic_view_contract",
        "diagnostic_unlabelled_inference_total",
    ),
    entry(
        InvariantId::DiagnosticPrePublicationSanitization,
        "diagnostic_prepublication_sanitization",
        "diagnostic_publication_redaction",
        "diagnostic_sanitizer_contract",
        "diagnostic_unsanitized_publish_total",
    ),
    entry(
        InvariantId::DiagnosticVerifiedBundle,
        "diagnostic_verified_bundle",
        "diagnostic_hash_signature_matrix",
        "diagnostic_verifier_contract",
        "diagnostic_verification_failed_total",
    ),
    entry(
        InvariantId::DiagnosticReplayProductionIsolation,
        "diagnostic_replay_production_isolation",
        "diagnostic_production_attachment_rejected",
        "diagnostic_replay_contract",
        "diagnostic_production_replay_total",
    ),
    entry(
        InvariantId::LegacySingleWriteOwner,
        "legacy_single_write_owner",
        "cutover_ownership_state_machine",
        "legacy_ownership_store_contract",
        "legacy_owner_violation_total",
    ),
    entry(
        InvariantId::LegacyCursorAfterDurability,
        "legacy_cursor_after_durability",
        "cdc_crash_before_checkpoint",
        "legacy_bridge_store_contract",
        "legacy_cursor_ahead_total",
    ),
    entry(
        InvariantId::LegacyBridgeIdempotency,
        "legacy_bridge_idempotency",
        "cdc_duplicate_delivery",
        "legacy_bridge_store_contract",
        "legacy_duplicate_effect_total",
    ),
    entry(
        InvariantId::LegacyPostCutoverFence,
        "legacy_post_cutover_fence",
        "stale_legacy_node_write",
        "legacy_write_guard_contract",
        "legacy_write_after_cutover_total",
    ),
    entry(
        InvariantId::LegacyTypedFacade,
        "legacy_typed_facade",
        "legacy_facade_retry",
        "legacy_facade_contract",
        "legacy_domain_bypass_total",
    ),
    entry(
        InvariantId::LegacyMappingFailClosed,
        "legacy_mapping_fail_closed",
        "unknown_legacy_status",
        "legacy_mapper_contract",
        "legacy_mapping_failure_total",
    ),
    entry(
        InvariantId::LegacyShadowIsolation,
        "legacy_shadow_isolation",
        "shadow_side_effect_capture",
        "legacy_shadow_executor_contract",
        "legacy_shadow_real_effect_total",
    ),
    entry(
        InvariantId::LegacyGovernanceCoverage,
        "legacy_governance_coverage",
        "migration_erasure_surfaces",
        "legacy_governance_contract",
        "legacy_governance_gap_total",
    ),
    entry(
        InvariantId::LegacyVerifiedCutover,
        "legacy_verified_cutover",
        "cutover_race_matrix",
        "legacy_cutover_contract",
        "legacy_unverified_cutover_total",
    ),
    entry(
        InvariantId::SecurityServerValidatedClaims,
        "security_server_validated_claims",
        "authentication_and_tenant_binding",
        "security_authentication_contract",
        "authz_denied_total",
    ),
    entry(
        InvariantId::SecurityPayloadImmutability,
        "security_payload_immutability",
        "operation_payload_substitution",
        "security_replay_contract",
        "payload_mismatch_total",
    ),
    entry(
        InvariantId::SecurityCapabilityFailClosed,
        "security_capability_fail_closed",
        "required_security_capability_downgrade",
        "compatibility_security_contract",
        "protocol_downgrade_rejected_total",
    ),
    entry(
        InvariantId::SecurityExternalInputBounds,
        "security_external_input_bounds",
        "boundary_plus_one_and_archive_bomb",
        "security_input_contract",
        "input_limit_rejected_total",
    ),
    entry(
        InvariantId::SecurityTenantIsolation,
        "security_tenant_isolation",
        "cross_tenant_resource_matrix",
        "security_tenant_contract",
        "cross_tenant_denied_total",
    ),
    entry(
        InvariantId::SecuritySecretExclusion,
        "security_secret_exclusion",
        "secret_redaction_and_serialization_exclusion",
        "security_secret_contract",
        "secret_exposure_total",
    ),
    entry(
        InvariantId::SecurityAuthorityRollback,
        "security_authority_rollback",
        "authority_epoch_rollback",
        "security_authority_contract",
        "authority_rollback_total",
    ),
    entry(
        InvariantId::SecuritySideEffectSafety,
        "security_side_effect_safety",
        "side_effect_safety",
        "security_side_effect_contract",
        "unsafe_side_effect_total",
    ),
    entry(
        InvariantId::SecurityAdminOverride,
        "security_admin_override",
        "admin_override_policy",
        "security_admin_contract",
        "admin_override_total",
    ),
    entry(
        InvariantId::SecurityIntegrationDistrust,
        "security_integration_distrust",
        "ssrf_archive_and_provider_input",
        "security_integration_contract",
        "untrusted_input_rejected_total",
    ),
    entry(
        InvariantId::FeedIndependentCursor,
        "feed_independent_cursor",
        "independent_epoch_bound_cursor",
        "feed_cursor_store_contract",
        "consumer_cursor_invalid_total",
    ),
    entry(
        InvariantId::FeedDurableEffectBeforeCursor,
        "feed_durable_effect_before_cursor",
        "durable_effect_before_ack",
        "feed_checkpoint_contract",
        "consumer_unsafe_ack_total",
    ),
    entry(
        InvariantId::FeedConsumerIsolation,
        "feed_consumer_isolation",
        "lagging_consumer_isolation",
        "feed_source_isolation_contract",
        "consumer_isolation_violation_total",
    ),
    entry(
        InvariantId::FeedDuplicateIdempotency,
        "feed_duplicate_idempotency",
        "duplicate_delivery_idempotency",
        "feed_idempotency_contract",
        "consumer_duplicate_effect_total",
    ),
    entry(
        InvariantId::FeedRetentionRecovery,
        "feed_retention_recovery",
        "journal_floor_recovery_policy",
        "feed_retention_contract",
        "consumer_floor_miss_total",
    ),
    entry(
        InvariantId::FeedExternalProjectionSafety,
        "feed_external_projection_safety",
        "external_projection_visibility",
        "feed_integration_contract",
        "feed_visibility_denied_total",
    ),
    entry(
        InvariantId::FeedOrderingSafety,
        "feed_ordering_safety",
        "partition_ordering_stability",
        "feed_partition_contract",
        "consumer_ordering_violation_total",
    ),
    entry(
        InvariantId::FeedResetSafety,
        "feed_reset_safety",
        "reset_plan_authorization_audit",
        "feed_admin_contract",
        "consumer_reset_denied_total",
    ),
    entry(
        InvariantId::FeedGovernanceCoverage,
        "feed_governance_coverage",
        "governance_residency_coverage",
        "feed_governance_contract",
        "consumer_governance_gap_total",
    ),
    entry(
        InvariantId::RegistryIdNonReuse,
        "registry_id_non_reuse",
        "removed_id_reuse",
        "registry_lock_contract",
        "registry_id_reuse_total",
    ),
    entry(
        InvariantId::RegistryCanonicalResolution,
        "registry_canonical_resolution",
        "unknown_id_rejection",
        "generated_registry_contract",
        "registry_unknown_id_total",
    ),
    entry(
        InvariantId::RegistryBreakingChangePath,
        "registry_breaking_change_path",
        "breaking_field_change",
        "registry_compatibility_contract",
        "registry_unmigrated_break_total",
    ),
    entry(
        InvariantId::RegistryGeneratedSourceParity,
        "registry_generated_source_parity",
        "generated_artifact_drift",
        "registry_codegen_contract",
        "registry_codegen_drift_total",
    ),
    entry(
        InvariantId::RegistryHistoricalReservation,
        "registry_historical_reservation",
        "retired_id_resolution",
        "registry_lock_contract",
        "registry_history_gap_total",
    ),
    entry(
        InvariantId::RegistryNamespaceIsolation,
        "registry_namespace_isolation",
        "extension_collision",
        "registry_namespace_contract",
        "registry_collision_total",
    ),
    entry(
        InvariantId::RegistrySecurityReview,
        "registry_security_review",
        "security_change_without_proposal",
        "registry_review_contract",
        "registry_security_review_gap_total",
    ),
    entry(
        InvariantId::RegistryRuntimeImmutability,
        "registry_runtime_immutability",
        "runtime_redefinition_rejected",
        "generated_registry_contract",
        "registry_runtime_mutation_total",
    ),
    entry(
        InvariantId::RegistryHistoricalResolution,
        "registry_historical_resolution",
        "historical_id_resolution",
        "registry_history_contract",
        "registry_historical_unknown_total",
    ),
    entry(
        InvariantId::CertificationSemanticOnly,
        "certification_semantic_observation_only",
        "implementation_independence",
        "conformance_observation_contract",
        "certification_physical_assumption_total",
    ),
    entry(
        InvariantId::CertificationTierTruthfulness,
        "certification_required_tests_pass",
        "required_skip_failure_matrix",
        "conformance_tier_contract",
        "certification_false_tier_total",
    ),
    entry(
        InvariantId::CertificationExactBinding,
        "certification_exact_subject_binding",
        "binary_feature_environment_substitution",
        "conformance_identity_contract",
        "certification_binding_mismatch_total",
    ),
    entry(
        InvariantId::CertificationCapabilityTruthfulness,
        "certification_capability_evidence",
        "false_capability_advertisement",
        "conformance_capability_contract",
        "certification_unverified_capability_total",
    ),
    entry(
        InvariantId::CertificationRuntimeValidation,
        "certification_runtime_checks_preserved",
        "certified_invalid_input",
        "conformance_runtime_contract",
        "certification_validation_bypass_total",
    ),
    entry(
        InvariantId::CertificationArtifactImmutability,
        "certification_content_identity",
        "historical_artifact_tamper",
        "conformance_artifact_contract",
        "certification_artifact_tamper_total",
    ),
    entry(
        InvariantId::CertificationEvidenceIntegrity,
        "certification_hash_signature_verification",
        "report_and_signature_tamper",
        "conformance_evidence_contract",
        "certification_evidence_invalid_total",
    ),
    entry(
        InvariantId::CertificationCorrectnessPriority,
        "certification_correctness_precedes_performance",
        "fast_but_incorrect_subject",
        "conformance_correctness_contract",
        "certification_performance_override_total",
    ),
    entry(
        InvariantId::CertificationLifecycleIdentity,
        "certification_lifecycle_identity_stable",
        "suspend_revoke_supersede_matrix",
        "conformance_catalog_contract",
        "certification_identity_reuse_total",
    ),
    entry(
        InvariantId::MobileDurableProcessIndependence,
        "mobile_process_independent_state",
        "process_kill_restart_matrix",
        "mobile_recovery_store_contract",
        "mobile_volatile_state_loss_total",
    ),
    entry(
        InvariantId::MobileIntentAtomicity,
        "mobile_local_intent_atomicity",
        "disk_full_mutation_matrix",
        "mobile_local_store_contract",
        "mobile_intent_atomicity_failure_total",
    ),
    entry(
        InvariantId::MobilePushHintOnly,
        "mobile_push_hint_only",
        "push_loss_duplicate_untrusted_matrix",
        "mobile_push_contract",
        "mobile_push_authority_attempt_total",
    ),
    entry(
        InvariantId::MobileCursorCheckpointSafety,
        "mobile_cursor_after_durable_apply",
        "background_expiration_reconcile_matrix",
        "mobile_checkpoint_contract",
        "mobile_cursor_ahead_total",
    ),
    entry(
        InvariantId::MobileSecureKeyStorage,
        "mobile_secure_key_provider",
        "locked_key_and_export_matrix",
        "mobile_secure_store_contract",
        "mobile_key_exposure_total",
    ),
    entry(
        InvariantId::MobileResourceSemanticSafety,
        "mobile_resource_semantic_safety",
        "network_power_thermal_policy_matrix",
        "mobile_policy_contract",
        "mobile_semantic_downgrade_total",
    ),
    entry(
        InvariantId::MobileUpgradeIntentSafety,
        "mobile_upgrade_intent_preservation",
        "pending_outbox_upgrade_downgrade_matrix",
        "mobile_migration_contract",
        "mobile_upgrade_intent_loss_total",
    ),
    entry(
        InvariantId::MobileStorageTruthfulness,
        "mobile_storage_truthful_result",
        "storage_unavailable_mutation_matrix",
        "mobile_store_failure_contract",
        "mobile_false_saved_total",
    ),
    entry(
        InvariantId::MobilePlatformBoundary,
        "mobile_platform_no_semantic_bypass",
        "binding_surface_authority_matrix",
        "mobile_binding_contract",
        "mobile_platform_bypass_total",
    ),
    entry(
        InvariantId::DesktopSingleCoordinator,
        "desktop_single_coordinator",
        "desktop_multiprocess_election_matrix",
        "desktop_coordination_contract",
        "desktop_multiple_coordinator_total",
    ),
    entry(
        InvariantId::DesktopStaleFenceSafety,
        "desktop_stale_fence_rejected",
        "desktop_stale_process_revive_matrix",
        "desktop_fencing_contract",
        "desktop_stale_commit_total",
    ),
    entry(
        InvariantId::DesktopResumeIdempotency,
        "desktop_resume_idempotent",
        "desktop_suspend_ambiguous_retry_matrix",
        "desktop_resume_contract",
        "desktop_duplicate_effect_total",
    ),
    entry(
        InvariantId::DesktopIpcBoundary,
        "desktop_ipc_domain_only",
        "desktop_raw_metadata_command_matrix",
        "desktop_ipc_contract",
        "desktop_ipc_bypass_total",
    ),
    entry(
        InvariantId::DesktopCloneBindingSafety,
        "desktop_clone_rebind",
        "desktop_store_clone_matrix",
        "desktop_binding_contract",
        "desktop_binding_reuse_total",
    ),
    entry(
        InvariantId::DesktopUpgradeIntentSafety,
        "desktop_upgrade_intent_preserved",
        "desktop_pending_outbox_upgrade_matrix",
        "desktop_update_contract",
        "desktop_upgrade_intent_loss_total",
    ),
    entry(
        InvariantId::DesktopModeParity,
        "desktop_mode_semantic_parity",
        "desktop_agent_in_process_trace_matrix",
        "desktop_mode_contract",
        "desktop_mode_divergence_total",
    ),
    entry(
        InvariantId::DesktopPersistenceTruthfulness,
        "desktop_persistence_truthful",
        "desktop_low_disk_secure_store_matrix",
        "desktop_persistence_contract",
        "desktop_false_persistence_total",
    ),
    entry(
        InvariantId::DesktopDerivedStateSafety,
        "desktop_derived_state_non_authoritative",
        "desktop_cache_rebuild_matrix",
        "desktop_derived_state_contract",
        "desktop_derived_authority_total",
    ),
    entry(
        InvariantId::StorageLocalIntentAtomicity,
        "storage_local_intent_atomicity",
        "storage_atomic_commit_matrix",
        "storage_atomicity_contract",
        "storage_intent_gap_total",
    ),
    entry(
        InvariantId::StorageCriticalIntentRetention,
        "storage_critical_intent_retained",
        "storage_pressure_eviction_matrix",
        "storage_eviction_contract",
        "storage_intent_eviction_total",
    ),
    entry(
        InvariantId::StorageCacheIsolation,
        "storage_cache_non_authoritative",
        "storage_cache_purge_matrix",
        "storage_cache_purge_contract",
        "storage_cache_intent_loss_total",
    ),
    entry(
        InvariantId::StorageCloneBindingSafety,
        "storage_clone_rebind",
        "storage_restore_binding_matrix",
        "storage_restore_contract",
        "storage_binding_reuse_total",
    ),
    entry(
        InvariantId::StorageDowngradeSafety,
        "storage_downgrade_rejected",
        "storage_format_version_matrix",
        "storage_migration_contract",
        "storage_unsafe_downgrade_total",
    ),
    entry(
        InvariantId::StoragePlatformCertification,
        "storage_target_certified",
        "storage_platform_matrix",
        "storage_platform_conformance",
        "storage_uncertified_target_total",
    ),
    entry(
        InvariantId::StoragePublicationSafety,
        "storage_publish_after_verify",
        "storage_staging_crash_matrix",
        "storage_publication_contract",
        "storage_unverified_publish_total",
    ),
    entry(
        InvariantId::StorageAdmissionTruthfulness,
        "storage_admission_truthful",
        "storage_low_disk_matrix",
        "storage_admission_contract",
        "storage_false_commit_total",
    ),
    entry(
        InvariantId::StorageSecretIsolation,
        "storage_secret_isolation",
        "storage_metadata_secret_scan",
        "storage_secure_provider_contract",
        "storage_plaintext_secret_total",
    ),
    entry(
        InvariantId::StorageFilesystemSafety,
        "storage_filesystem_safe",
        "storage_path_policy_matrix",
        "storage_layout_contract",
        "storage_unsafe_path_total",
    ),
    entry(
        InvariantId::ImplementationFoundationIsolation,
        "foundation_dependency_isolation",
        "workspace_layer_graph_matrix",
        "guppy_foundation_boundary",
        "architecture_foundation_edge_total",
    ),
    entry(
        InvariantId::ImplementationStorageTypeIsolation,
        "storage_contract_type_isolation",
        "public_contract_source_scan",
        "database_neutrality_gate",
        "architecture_storage_type_leak_total",
    ),
    entry(
        InvariantId::ImplementationClientUiIsolation,
        "client_ui_independence",
        "client_dependency_policy_matrix",
        "client_boundary_gate",
        "architecture_client_ui_edge_total",
    ),
    entry(
        InvariantId::ImplementationServerHttpIsolation,
        "server_http_independence",
        "server_dependency_policy_matrix",
        "server_boundary_gate",
        "architecture_server_http_edge_total",
    ),
    entry(
        InvariantId::ImplementationAdapterDirection,
        "adapter_dependency_direction",
        "adapter_reverse_edge_matrix",
        "storage_adapter_boundary_gate",
        "architecture_adapter_reverse_edge_total",
    ),
    entry(
        InvariantId::ImplementationPlatformIsolation,
        "platform_dependency_isolation",
        "exclusive_dependency_owner_matrix",
        "platform_boundary_gate",
        "architecture_platform_leak_total",
    ),
    entry(
        InvariantId::ImplementationFeatureIsolation,
        "feature_adapter_isolation",
        "core_feature_policy_matrix",
        "feature_boundary_gate",
        "architecture_adapter_feature_total",
    ),
    entry(
        InvariantId::ImplementationCompositionRoot,
        "application_composition_root",
        "application_surface_matrix",
        "composition_root_gate",
        "architecture_application_semantic_owner_total",
    ),
    entry(
        InvariantId::ImplementationRegistryDeterminism,
        "registry_source_generated_parity",
        "registry_regeneration_matrix",
        "registry_verify_gate",
        "architecture_registry_drift_total",
    ),
    entry(
        InvariantId::ImplementationDependencyGraph,
        "declared_dependency_graph",
        "new_dependency_edge_matrix",
        "workspace_architecture_gate",
        "architecture_forbidden_edge_total",
    ),
    entry(
        InvariantId::SdkCursorIsolation,
        "sdk_cursor_isolation",
        "public_surface_cursor_scan",
        "sdk_architecture_gate",
        "sdk_cursor_bypass_total",
    ),
    entry(
        InvariantId::SdkLocalCommitDistinction,
        "sdk_local_commit_distinction",
        "mutation_receipt_contract",
        "client_store_commit_contract",
        "sdk_false_confirmation_total",
    ),
    entry(
        InvariantId::SdkStorageNeutrality,
        "sdk_storage_neutrality",
        "public_surface_dependency_scan",
        "database_neutrality_gate",
        "sdk_storage_type_leak_total",
    ),
    entry(
        InvariantId::SdkCancellationSafety,
        "sdk_cancellation_safety",
        "cancel_after_durable_commit",
        "client_store_cancellation_contract",
        "sdk_cancelled_intent_loss_total",
    ),
    entry(
        InvariantId::SdkEventLossSafety,
        "sdk_event_loss_safety",
        "bounded_event_lag_status_query",
        "client_store_status_contract",
        "sdk_event_state_loss_total",
    ),
    entry(
        InvariantId::SdkErrorCompatibility,
        "sdk_error_compatibility",
        "error_code_compatibility_matrix",
        "sdk_public_api_gate",
        "sdk_unknown_error_code_total",
    ),
    entry(
        InvariantId::SdkClosedCoreSemantics,
        "sdk_closed_core_semantics",
        "extension_surface_source_scan",
        "sdk_architecture_gate",
        "sdk_core_semantic_override_total",
    ),
    entry(
        InvariantId::SdkVersionIndependence,
        "sdk_version_independence",
        "compatibility_matrix_contract",
        "sdk_release_gate",
        "sdk_version_conflation_total",
    ),
    entry(
        InvariantId::SdkDangerousOperationIsolation,
        "sdk_dangerous_operation_isolation",
        "convenience_surface_misuse_scan",
        "sdk_architecture_gate",
        "sdk_dangerous_path_total",
    ),
    entry(
        InvariantId::SdkSemverAutomation,
        "sdk_semver_automation",
        "public_api_snapshot_review",
        "cargo_semver_checks",
        "sdk_unreviewed_break_total",
    ),
    entry(
        InvariantId::AdapterCapabilityTruthfulness,
        "adapter_capability_truthful",
        "adapter_claim_evidence_matrix",
        "adapter_capability_conformance",
        "adapter_unverified_claim_total",
    ),
    entry(
        InvariantId::AdapterLocalAtomicity,
        "adapter_local_atomicity",
        "adapter_local_failpoint_matrix",
        "adapter_atomic_local_outbox",
        "adapter_local_atomicity_failure_total",
    ),
    entry(
        InvariantId::AdapterAuthorityAtomicity,
        "adapter_authority_atomicity",
        "adapter_authority_failpoint_matrix",
        "adapter_atomic_authority_commit",
        "adapter_authority_atomicity_failure_total",
    ),
    entry(
        InvariantId::AdapterTypeIsolation,
        "adapter_type_isolation",
        "adapter_public_type_scan",
        "adapter_neutral_error_boundary",
        "adapter_physical_type_leak_total",
    ),
    entry(
        InvariantId::AdapterPayloadBinding,
        "adapter_payload_binding",
        "adapter_operation_reuse_matrix",
        "adapter_payload_reuse_rejection",
        "adapter_payload_mismatch_total",
    ),
    entry(
        InvariantId::AdapterMigrationSafety,
        "adapter_migration_safety",
        "adapter_migration_crash_matrix",
        "adapter_migration_preservation",
        "adapter_migration_loss_total",
    ),
    entry(
        InvariantId::AdapterStartupSafety,
        "adapter_startup_fail_closed",
        "adapter_requirement_matrix",
        "adapter_startup_fail_closed",
        "adapter_unsafe_downgrade_total",
    ),
    entry(
        InvariantId::AdapterEnvironmentBinding,
        "adapter_environment_binding",
        "adapter_environment_drift_matrix",
        "adapter_environment_binding",
        "adapter_uncertified_environment_total",
    ),
    entry(
        InvariantId::AdapterCriticalDurability,
        "adapter_critical_durability",
        "adapter_durability_configuration_matrix",
        "adapter_critical_durability",
        "adapter_weak_critical_write_total",
    ),
    entry(
        InvariantId::AdapterManifestTruthfulness,
        "adapter_manifest_truthfulness",
        "adapter_support_matrix_review",
        "adapter_manifest_and_limitations",
        "adapter_manifest_drift_total",
    ),
    entry(
        InvariantId::PostgresAuthorityAtomicity,
        "postgres_tx_b_atomicity",
        "postgres_failpoint_atomicity_matrix",
        "postgres_authority_full_atomicity",
        "postgres_partial_commit_total",
    ),
    entry(
        InvariantId::PostgresIdempotentReplay,
        "postgres_duplicate_single_effect",
        "postgres_duplicate_race_matrix",
        "postgres_operation_ledger_replay",
        "postgres_duplicate_effect_total",
    ),
    entry(
        InvariantId::PostgresPayloadBinding,
        "postgres_operation_payload_binding",
        "postgres_payload_misuse_matrix",
        "postgres_payload_digest_rejection",
        "postgres_payload_mismatch_total",
    ),
    entry(
        InvariantId::PostgresCommittedTimeline,
        "postgres_committed_timeline",
        "postgres_concurrent_commit_order_matrix",
        "postgres_transactional_timeline_allocator",
        "postgres_timeline_order_failure_total",
    ),
    entry(
        InvariantId::PostgresRestoreEpoch,
        "postgres_restore_epoch_transition",
        "postgres_restore_transition_matrix",
        "postgres_pitr_epoch_procedure",
        "postgres_restore_epoch_violation_total",
    ),
    entry(
        InvariantId::PostgresTypeIsolation,
        "postgres_physical_type_isolation",
        "postgres_public_surface_scan",
        "postgres_adapter_boundary_gate",
        "postgres_type_leak_total",
    ),
    entry(
        InvariantId::PostgresReadinessSafety,
        "postgres_readiness_fail_closed",
        "postgres_setting_drift_matrix",
        "postgres_correctness_setting_validation",
        "postgres_unsafe_readiness_total",
    ),
    entry(
        InvariantId::PostgresSideEffectIntent,
        "postgres_durable_side_effect_intent",
        "postgres_side_effect_failpoint_matrix",
        "postgres_tx_b_side_effect_outbox",
        "postgres_missing_intent_total",
    ),
    entry(
        InvariantId::PostgresRetentionSafety,
        "postgres_retention_safe_floor",
        "postgres_lease_snapshot_floor_matrix",
        "postgres_safe_journal_compaction",
        "postgres_unsafe_compaction_total",
    ),
    entry(
        InvariantId::PostgresNeonSemanticParity,
        "postgres_neon_semantic_parity",
        "neon_operational_profile_matrix",
        "neon_authority_semantic_conformance",
        "neon_semantic_drift_total",
    ),
    entry(
        InvariantId::StoolapLocalAtomicity,
        "stoolap_tx_a_atomicity",
        "stoolap_tx_a_failpoint_matrix",
        "stoolap_domain_outbox_atomicity",
        "stoolap_partial_local_commit_total",
    ),
    entry(
        InvariantId::StoolapReconciliationAtomicity,
        "stoolap_tx_c_atomicity",
        "stoolap_tx_c_failpoint_matrix",
        "stoolap_reconcile_cursor_atomicity",
        "stoolap_partial_reconcile_total",
    ),
    entry(
        InvariantId::StoolapRetryIdentity,
        "stoolap_retry_identity",
        "stoolap_crash_retry_matrix",
        "stoolap_digest_bound_claim_retry",
        "stoolap_retry_identity_mismatch_total",
    ),
    entry(
        InvariantId::StoolapIntentPreservation,
        "stoolap_pending_intent_preservation",
        "stoolap_rebootstrap_repair_matrix",
        "stoolap_pending_intent_recovery",
        "stoolap_pending_intent_loss_total",
    ),
    entry(
        InvariantId::StoolapCursorSafety,
        "stoolap_cursor_after_apply",
        "stoolap_cursor_failpoint_matrix",
        "stoolap_reconcile_cursor_boundary",
        "stoolap_cursor_ahead_total",
    ),
    entry(
        InvariantId::StoolapFencedCoordinator,
        "stoolap_current_fence_only",
        "stoolap_coordinator_race_matrix",
        "stoolap_fenced_metadata_transitions",
        "stoolap_stale_fence_total",
    ),
    entry(
        InvariantId::StoolapCloneSafety,
        "stoolap_clone_rebinding",
        "stoolap_restore_binding_matrix",
        "stoolap_device_binding_validation",
        "stoolap_clone_identity_reuse_total",
    ),
    entry(
        InvariantId::StoolapStoragePressureSafety,
        "stoolap_critical_storage_preservation",
        "stoolap_storage_pressure_matrix",
        "stoolap_storage_preflight",
        "stoolap_critical_eviction_total",
    ),
    entry(
        InvariantId::StoolapTypeIsolation,
        "stoolap_physical_type_isolation",
        "stoolap_public_surface_scan",
        "stoolap_adapter_boundary_gate",
        "stoolap_type_leak_total",
    ),
    entry(
        InvariantId::StoolapPlatformCertification,
        "stoolap_target_bound_support",
        "stoolap_platform_profile_matrix",
        "stoolap_environment_conformance",
        "stoolap_uncertified_target_total",
    ),
    entry(
        InvariantId::AxumTransportIsolation,
        "axum_transport_only",
        "axum_route_boundary_scan",
        "axum_thin_route_contract",
        "axum_domain_leak_total",
    ),
    entry(
        InvariantId::AxumCredentialIsolation,
        "axum_credential_isolation",
        "axum_auth_redaction_matrix",
        "axum_auth_context_extraction",
        "axum_credential_leak_total",
    ),
    entry(
        InvariantId::AxumResourceBounds,
        "axum_resource_bounds",
        "axum_malformed_and_bomb_matrix",
        "axum_bounded_decode_pipeline",
        "axum_limit_rejection_total",
    ),
    entry(
        InvariantId::AxumCommitDeliveryIndependence,
        "axum_commit_before_delivery",
        "axum_disconnect_retry_matrix",
        "axum_idempotent_delivery_contract",
        "axum_delivery_commit_violation_total",
    ),
    entry(
        InvariantId::AxumOverloadBounds,
        "axum_bounded_overload",
        "axum_saturation_matrix",
        "axum_hierarchical_admission",
        "axum_unbounded_admission_total",
    ),
    entry(
        InvariantId::AxumIdentityBinding,
        "axum_identity_binding",
        "axum_cross_tenant_device_matrix",
        "axum_authenticated_identity_contract",
        "axum_identity_mismatch_total",
    ),
    entry(
        InvariantId::AxumStableSemantics,
        "axum_stable_error_semantics",
        "axum_status_error_matrix",
        "axum_error_envelope_contract",
        "axum_unstable_error_total",
    ),
    entry(
        InvariantId::AxumLiveHintDurability,
        "axum_live_hint_advisory_only",
        "axum_hint_loss_matrix",
        "axum_cursor_recovery_contract",
        "axum_hint_dependency_total",
    ),
    entry(
        InvariantId::AxumNodeEpochIndependence,
        "axum_node_epoch_independence",
        "axum_node_replacement_matrix",
        "axum_stateless_node_contract",
        "axum_node_epoch_change_total",
    ),
    entry(
        InvariantId::AxumErrorSanitization,
        "axum_public_error_sanitization",
        "axum_error_leak_matrix",
        "axum_sanitized_envelope_contract",
        "axum_error_leak_total",
    ),
    entry(
        InvariantId::DioxusDurableStateOwnership,
        "dioxus_durable_state_ownership",
        "ui_restart_rehydrates_durable_state",
        "dioxus_state_boundary_gate",
        "dioxus_volatile_intent_total",
    ),
    entry(
        InvariantId::DioxusLocalCommitTruth,
        "dioxus_local_commit_truth",
        "mutation_receipt_after_commit",
        "dioxus_mutation_contract",
        "dioxus_early_saved_total",
    ),
    entry(
        InvariantId::DioxusAuthorityDistinction,
        "dioxus_authority_distinction",
        "local_receipt_not_authority",
        "dioxus_status_contract",
        "dioxus_false_confirmation_total",
    ),
    entry(
        InvariantId::DioxusEventLossSafety,
        "dioxus_event_loss_safety",
        "lost_hint_forces_durable_reread",
        "dioxus_invalidation_contract",
        "dioxus_event_dependency_total",
    ),
    entry(
        InvariantId::DioxusUnmountSafety,
        "dioxus_unmount_safety",
        "unmount_after_local_commit",
        "dioxus_cancellation_contract",
        "dioxus_unmount_intent_loss_total",
    ),
    entry(
        InvariantId::DioxusStoreIsolation,
        "dioxus_store_isolation",
        "rapid_tenant_switch",
        "dioxus_namespace_contract",
        "dioxus_cross_store_leak_total",
    ),
    entry(
        InvariantId::DioxusConflictSemantics,
        "dioxus_conflict_semantics",
        "resolution_creates_durable_intent",
        "dioxus_conflict_contract",
        "dioxus_direct_conflict_mutation_total",
    ),
    entry(
        InvariantId::DioxusOfflineCapability,
        "dioxus_offline_capability",
        "offline_read_write_matrix",
        "dioxus_offline_contract",
        "dioxus_offline_capability_loss_total",
    ),
    entry(
        InvariantId::DioxusBackgroundOwnership,
        "dioxus_background_ownership",
        "component_unmount_scheduler_survives",
        "dioxus_platform_boundary_gate",
        "dioxus_component_scheduler_total",
    ),
    entry(
        InvariantId::DioxusEventBounds,
        "dioxus_event_bounds",
        "ui_event_storm_coalesces",
        "dioxus_bounded_event_contract",
        "dioxus_unbounded_event_total",
    ),
    entry(
        InvariantId::CliNoInvariantBypass,
        "cli_no_invariant_bypass",
        "cli_semantic_boundary_matrix",
        "cli_sdk_control_api_gate",
        "cli_direct_mutation_attempt_total",
    ),
    entry(
        InvariantId::CliMachineOutputVersioning,
        "cli_machine_output_versioned",
        "cli_machine_output_golden",
        "cli_output_schema_contract",
        "cli_schema_mismatch_total",
    ),
    entry(
        InvariantId::CliSensitiveRedaction,
        "cli_sensitive_redaction",
        "cli_secret_leak_matrix",
        "cli_redaction_contract",
        "cli_sensitive_leak_total",
    ),
    entry(
        InvariantId::CliPlanApplySafety,
        "cli_plan_apply_safety",
        "cli_dangerous_action_matrix",
        "cli_control_plan_contract",
        "cli_unplanned_action_total",
    ),
    entry(
        InvariantId::CliSubmissionAmbiguity,
        "cli_submission_ambiguity",
        "cli_timeout_cancel_matrix",
        "cli_idempotent_submission_contract",
        "cli_unknown_submission_total",
    ),
    entry(
        InvariantId::CliStoreOwnership,
        "cli_store_ownership",
        "cli_concurrent_owner_matrix",
        "cli_fenced_inspection_contract",
        "cli_writer_conflict_total",
    ),
    entry(
        InvariantId::CliMigrationSafety,
        "cli_migration_safety",
        "cli_migration_precondition_matrix",
        "cli_migration_identity_contract",
        "cli_migration_rejection_total",
    ),
    entry(
        InvariantId::CliSemanticMutation,
        "cli_semantic_mutation_only",
        "cli_operational_action_matrix",
        "cli_registered_operation_contract",
        "cli_metadata_mutation_attempt_total",
    ),
    entry(
        InvariantId::CliProductionGuard,
        "cli_production_guard",
        "cli_dev_action_profile_matrix",
        "cli_failpoint_build_contract",
        "cli_production_dev_action_total",
    ),
    entry(
        InvariantId::CliCanonicalSemantics,
        "cli_canonical_semantics",
        "cli_sdk_equivalence_matrix",
        "cli_public_api_dependency_gate",
        "cli_semantic_drift_total",
    ),
    entry(
        InvariantId::SqliteWalDurability,
        "sqlite_production_wal",
        "sqlite_wal_open_matrix",
        "sqlite_wal_configuration_contract",
        "sqlite_non_wal_open_total",
    ),
    entry(
        InvariantId::SqliteSingleWriter,
        "sqlite_single_logical_writer",
        "sqlite_writer_contention_matrix",
        "sqlite_immediate_transaction_contract",
        "sqlite_writer_violation_total",
    ),
    entry(
        InvariantId::SqliteCursorAtomicity,
        "sqlite_cursor_after_reconciliation",
        "sqlite_tx_c_failure_matrix",
        "sqlite_reconcile_cursor_atomicity",
        "sqlite_cursor_ahead_total",
    ),
    entry(
        InvariantId::SqliteOutboxPreservation,
        "sqlite_outbox_lifecycle_preservation",
        "sqlite_crash_reopen_matrix",
        "sqlite_digest_bound_outbox_contract",
        "sqlite_outbox_loss_total",
    ),
    entry(
        InvariantId::SqliteAdapterParity,
        "sqlite_stoolap_semantic_parity",
        "sqlite_reference_differential_matrix",
        "sqlite_local_adapter_conformance",
        "sqlite_semantic_drift_total",
    ),
    entry(
        InvariantId::ConfigCorrectnessPreservation,
        "config_correctness_preservation",
        "unsafe_switch_absence_matrix",
        "configuration_correctness_gate",
        "config_correctness_rejection_total",
    ),
    entry(
        InvariantId::ConfigSecretRedaction,
        "config_secret_redaction",
        "secret_marker_leak_matrix",
        "secret_provider_redaction_contract",
        "config_secret_leak_total",
    ),
    entry(
        InvariantId::ConfigValidatedPublication,
        "config_validated_publication",
        "invalid_bounds_never_effective",
        "configuration_typestate_contract",
        "config_invalid_publication_total",
    ),
    entry(
        InvariantId::ConfigAtomicReload,
        "config_atomic_reload",
        "concurrent_generation_observation",
        "runtime_snapshot_publication_contract",
        "config_mixed_generation_total",
    ),
    entry(
        InvariantId::ConfigDurableIdentityIsolation,
        "config_durable_identity_isolation",
        "editable_identity_absence_matrix",
        "configuration_schema_identity_gate",
        "config_identity_override_total",
    ),
    entry(
        InvariantId::ConfigFeatureSemanticSafety,
        "config_feature_semantic_safety",
        "semantic_rollout_rejection_matrix",
        "feature_definition_safety_contract",
        "feature_semantic_drift_total",
    ),
    entry(
        InvariantId::ConfigProductionSafety,
        "config_production_safety",
        "production_unsafe_facility_matrix",
        "production_profile_validation_contract",
        "config_production_bypass_total",
    ),
    entry(
        InvariantId::ConfigAdapterCapabilitySafety,
        "config_adapter_capability_safety",
        "unsupported_capability_matrix",
        "adapter_configuration_validation_contract",
        "config_adapter_downgrade_total",
    ),
    entry(
        InvariantId::ConfigFailedReloadPreservation,
        "config_failed_reload_preservation",
        "invalid_reload_generation_matrix",
        "runtime_reload_rollback_contract",
        "config_failed_reload_activation_total",
    ),
    entry(
        InvariantId::ConfigAuthoritativePolicy,
        "config_authoritative_policy",
        "flag_entitlement_intersection_matrix",
        "feature_entitlement_separation_contract",
        "config_client_policy_grant_total",
    ),
    entry(
        InvariantId::ReleaseTraceability,
        "release_traceability",
        "release_manifest_provenance_matrix",
        "release_manifest_verification_contract",
        "release_untraceable_artifact_total",
    ),
    entry(
        InvariantId::ReleaseArtifactImmutability,
        "release_artifact_immutability",
        "semantic_version_digest_conflict_matrix",
        "immutable_release_index_contract",
        "release_version_repoint_total",
    ),
    entry(
        InvariantId::ReleaseSignatureIntegrity,
        "release_signature_integrity",
        "artifact_tamper_and_key_purpose_matrix",
        "final_artifact_signature_contract",
        "release_signature_failure_total",
    ),
    entry(
        InvariantId::ReleaseCompatibilityGate,
        "release_compatibility_gate",
        "upgrade_dimension_matrix",
        "release_compatibility_contract",
        "release_incompatible_upgrade_total",
    ),
    entry(
        InvariantId::ReleaseClientIntentPreservation,
        "release_client_intent_preservation",
        "atomic_upgrade_failure_matrix",
        "desktop_upgrade_checkpoint_contract",
        "release_upgrade_intent_loss_total",
    ),
    entry(
        InvariantId::ReleaseRollbackSafety,
        "release_rollback_safety",
        "rollback_class_and_format_matrix",
        "release_rollback_contract",
        "release_unsafe_rollback_total",
    ),
    entry(
        InvariantId::ReleaseImmutablePromotion,
        "release_immutable_promotion",
        "channel_promotion_digest_matrix",
        "release_promotion_contract",
        "release_rebuild_on_promotion_total",
    ),
    entry(
        InvariantId::ReleaseCredentialIsolation,
        "release_credential_isolation",
        "release_job_permission_matrix",
        "release_workflow_permission_contract",
        "release_credential_exposure_total",
    ),
    entry(
        InvariantId::ReleaseUpdateFailClosed,
        "release_update_fail_closed",
        "update_metadata_adversarial_matrix",
        "signed_update_metadata_contract",
        "release_untrusted_update_total",
    ),
    entry(
        InvariantId::ReleaseVersionDimensionSeparation,
        "release_version_dimension_separation",
        "independent_version_dimension_matrix",
        "release_compatibility_dimension_contract",
        "release_version_conflation_total",
    ),
    entry(
        InvariantId::DeploymentSingleWriter,
        "deployment_single_writer",
        "authority_scope_writer_uniqueness_matrix",
        "deployment_descriptor_authority_contract",
        "deployment_multiple_writer_total",
    ),
    entry(
        InvariantId::DeploymentSemanticPreservation,
        "deployment_semantic_preservation",
        "topology_semantic_equivalence_matrix",
        "deployment_topology_contract",
        "deployment_semantic_drift_total",
    ),
    entry(
        InvariantId::DeploymentNodeEpochIndependence,
        "deployment_node_epoch_independence",
        "node_restart_epoch_matrix",
        "deployment_epoch_lifecycle_contract",
        "deployment_node_epoch_change_total",
    ),
    entry(
        InvariantId::DeploymentDatabaseCredentialIsolation,
        "deployment_database_credential_isolation",
        "client_database_access_matrix",
        "deployment_secret_boundary_contract",
        "deployment_client_database_access_total",
    ),
    entry(
        InvariantId::DeploymentRegionalReadSafety,
        "deployment_regional_read_safety",
        "regional_cursor_consistency_matrix",
        "deployment_regional_routing_contract",
        "deployment_regional_write_attempt_total",
    ),
    entry(
        InvariantId::DeploymentAirGapIndependence,
        "deployment_air_gap_independence",
        "air_gap_public_dependency_matrix",
        "deployment_air_gap_contract",
        "deployment_public_dependency_total",
    ),
    entry(
        InvariantId::DeploymentRetrySafety,
        "deployment_retry_safety",
        "infrastructure_retry_idempotency_matrix",
        "deployment_retry_contract",
        "deployment_duplicate_effect_total",
    ),
    entry(
        InvariantId::DeploymentRestoreEpoch,
        "deployment_restore_epoch",
        "restore_continuity_matrix",
        "deployment_promotion_contract",
        "deployment_uncertain_epoch_resume_total",
    ),
    entry(
        InvariantId::DeploymentDurableStateOwnership,
        "deployment_durable_state_ownership",
        "process_loss_state_matrix",
        "deployment_durable_ownership_contract",
        "deployment_process_only_state_total",
    ),
    entry(
        InvariantId::DeploymentInfrastructureIndependence,
        "deployment_infrastructure_independence",
        "optional_infrastructure_absence_matrix",
        "deployment_minimal_profile_contract",
        "deployment_required_optional_service_total",
    ),
];

const fn entry(
    id: InvariantId,
    model_assertion: &'static str,
    property_test: &'static str,
    adapter_test: &'static str,
    diagnostic: &'static str,
) -> InvariantEntry {
    InvariantEntry {
        id,
        description: id.description(),
        model_assertion,
        property_test,
        adapter_test,
        diagnostic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_complete_unique_and_round_trips() {
        assert_eq!(REGISTRY.len(), InvariantId::ALL.len());
        for (index, invariant) in InvariantId::ALL.into_iter().enumerate() {
            assert_eq!(invariant.entry(), REGISTRY[index]);
            assert_eq!(invariant.as_str().parse(), Ok(invariant));
            assert!(!invariant.description().is_empty());
            assert!(!invariant.entry().model_assertion.is_empty());
            assert!(!invariant.entry().property_test.is_empty());
            assert!(!invariant.entry().adapter_test.is_empty());
            assert!(!invariant.entry().diagnostic.is_empty());
        }
    }
}
