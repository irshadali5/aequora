# Aequora durable registry

Generation: `2`

Digest: `59a6078d50ac62889e49b96904d7ebec33614baa162eba0368e13d6e30b0966e`

| Domain | ID | Name | Status | Owner | Schema | Description |
|---|---:|---|---|---|---:|---|
| capability | 1 | PostcardV1 | Current | protocol | - | Postcard version one payload codec. |
| capability | 2 | Zstd | Current | transport | - | Zstandard payload compression. |
| capability | 3 | SnapshotV1 | Current | bootstrap | - | Snapshot format version one. |
| capability | 4 | Tombstones | Current | protocol | - | Durable tombstone semantics. |
| capability | 5 | StreamingSnapshots | Current | bootstrap | - | Bounded streaming snapshot transfer. |
| capability | 6 | PushHints | Current | live-sync | - | Advisory push notifications. |
| capability | 7 | Quic | Current | transport | - | QUIC transport support. |
| capability | 8 | MultiRegion | Current | regional | - | Epoch-aware regional reads. |
| capability | 9 | LineageV1 | Current | replay | - | Operation lineage format version one. |
| capability | 10 | IntegrityV1 | Current | integrity | - | Integrity proof format version one. |
| capability | 11 | ScopeV1 | Current | scope | - | Scope generation semantics version one. |
| capability | 12 | LiveV1 | Current | live-sync | - | Live synchronization hints version one. |
| capability | 13 | SignedSnapshotV1 | Current | crypto | - | Signed snapshot envelope version one. |
| capability | 14 | EncryptedSnapshotV1 | Current | crypto | - | Encrypted snapshot envelope version one. |
| capability | 15 | DeviceSignatureV1 | Current | security | - | Device operation signature version one. |
| capability | 16 | AuthorityEpochV1 | Current | authority | - | Authority epoch fencing semantics. |
| capability | 17 | ResourceConstrainedV1 | Current | client | - | Coarse constrained-client behavior profile. |
| capability | 18 | NegotiationV1 | Current | protocol | - | Explicit compatibility negotiation version one. |
| profile | 1 | ImmutableAppendOnly | Current | semantics | 1 | Append-only immutable aggregate semantics. |
| profile | 2 | OptimisticVersioned | Current | semantics | 1 | Reject-stale optimistic aggregate semantics. |
| profile | 3 | Commutative | Current | semantics | 1 | Order-independent commutative operation semantics. |
| profile | 4 | LastWriterWins | Current | semantics | 1 | Explicit last-writer-wins field semantics. |
| profile | 5 | ManualConflict | Current | semantics | 1 | Conflict materialization requiring manual resolution. |
| profile | 6 | StrongAggregate | Current | semantics | 1 | Authority-serialized strong aggregate semantics. |
| profile | 7 | ServerOnly | Current | semantics | 1 | Server-originated operation semantics. |
| profile | 8 | DeviceLocal | Current | semantics | 1 | Device-local non-replicated semantics. |
| profile | 9 | DerivedProjection | Current | semantics | 1 | Rebuildable derived projection semantics. |
| error | 1 | Overloaded | Current | runtime | - | Admission capacity was exhausted. |
| error | 2 | Maintenance | Current | operations | - | Maintenance policy rejected work. |
| error | 3 | UpgradeRequired | Current | compatibility | - | Client is outside the support window. |
| error | 4 | Authentication | Current | security | - | Authenticated identity validation failed. |
| error | 5 | Protocol | Current | protocol | - | Protocol framing or compatibility validation failed. |
| error | 6 | Storage | Current | storage | - | Authoritative persistence failed. |
| error | 7 | Conflict | Current | semantics | - | Domain conflict requires resolution. |
| error | 8 | Validation | Current | protocol | - | Request data failed validation. |
| error | 9 | Deadline | Current | runtime | - | A bounded deadline elapsed. |
| error | 10 | Draining | Current | operations | - | Server is draining new work. |
| error | 11 | PayloadLimit | Current | security | - | Wire or decompressed payload exceeded policy. |
| error | 12 | Authority | Current | authority | - | Authority role, epoch, or fencing rejected work. |
| error | 100 | UnsupportedOperationKind | Current | registry | - | Operation kind has no canonical registry entry. |
| protocol | 1 | ProtocolV1 | Current | protocol | - | Initial checksummed Aequora synchronization protocol. |
| message | 1 | SyncRequest | Current | protocol | - | Bounded client synchronization request. |
| message | 2 | SyncResponse | Current | protocol | - | Bounded authoritative synchronization response. |
| message | 3 | BootstrapRequest | Current | protocol | - | Bootstrap negotiation request. |
| message | 4 | BootstrapResponse | Current | protocol | - | Bootstrap negotiation response. |
| message | 5 | SnapshotStreamRequest | Current | protocol | - | Request for a bounded snapshot page. |
| message | 6 | SnapshotStreamResponse | Current | protocol | - | One bounded snapshot page response. |
| message | 7 | PushHint | Current | live-sync | - | Advisory journal-advance notification. |
| message | 8 | TransportError | Current | protocol | - | Payload-free transport failure response. |
| permission | 1 | AuthorityView | Current | control-plane | - | View authority state. |
| permission | 2 | AuthorityPromote | Current | control-plane | - | Promote an authority candidate. |
| permission | 3 | AuthorityForcePromote | Current | control-plane | - | Force authority promotion under reviewed policy. |
| permission | 10 | JobsView | Current | control-plane | - | View durable jobs. |
| permission | 11 | JobsRetry | Current | control-plane | - | Retry a durable job. |
| permission | 12 | JobsReconcile | Current | control-plane | - | Reconcile ambiguous durable work. |
| permission | 13 | JobsCancel | Current | control-plane | - | Cancel a durable job. |
| permission | 14 | JobsQuarantine | Current | control-plane | - | Quarantine a durable job. |
| permission | 20 | SnapshotView | Current | control-plane | - | View snapshot state. |
| permission | 21 | SnapshotBuild | Current | control-plane | - | Build a snapshot. |
| permission | 22 | SnapshotVerify | Current | control-plane | - | Verify a snapshot. |
| permission | 23 | SnapshotExpire | Current | control-plane | - | Expire a snapshot. |
| permission | 30 | IntegrityScan | Current | control-plane | - | Run an integrity scan. |
| permission | 31 | IntegrityRepair | Current | control-plane | - | Run a reviewed integrity repair. |
| permission | 40 | GovernancePlan | Current | governance | - | Create a governance plan. |
| permission | 41 | GovernanceExecute | Current | governance | - | Execute a reviewed governance plan. |
| permission | 42 | LegalHoldManage | Current | governance | - | Manage legal holds. |
| permission | 50 | CryptoView | Current | crypto | - | View key lifecycle state. |
| permission | 51 | CryptoRotate | Current | crypto | - | Rotate a managed key. |
| permission | 52 | CryptoRevoke | Current | crypto | - | Revoke a managed key. |
| permission | 53 | CryptoDestroy | Current | crypto | - | Destroy key material under policy. |
| permission | 60 | CompatibilityView | Current | compatibility | - | View compatibility policy. |
| permission | 61 | CompatibilityUpdate | Current | compatibility | - | Update compatibility policy. |
| permission | 70 | RegionView | Current | regional | - | View regional state. |
| permission | 71 | RegionDrain | Current | regional | - | Drain a region. |
| permission | 80 | MaintenanceManage | Current | control-plane | - | Manage maintenance and emergency stops. |
| permission | 90 | TenantManage | Current | control-plane | - | Manage tenant operational state. |
| permission | 91 | DeviceManage | Current | security | - | Manage device trust state. |
| permission | 92 | ScopeManage | Current | scope | - | Manage scope generations. |
| permission | 100 | DiagnosticsView | Current | diagnostics | - | View authenticated diagnostics. |
| permission | 101 | DiagnosticsExport | Current | diagnostics | - | Export redacted diagnostic artifacts. |
| permission | 110 | ConfigView | Current | control-plane | - | View dynamic configuration. |
| permission | 111 | ConfigUpdate | Current | control-plane | - | Update dynamic configuration. |
| permission | 120 | OperationsView | Current | control-plane | - | View operation status. |
| permission | 130 | ConsumersView | Current | integrations | - | View feed consumers. |
| permission | 131 | ConsumersManage | Current | integrations | - | Manage feed consumer activation. |
| permission | 132 | ConsumersReset | Current | integrations | - | Reset feed consumer state. |
| admin-action | 1 | PromoteAuthority | Current | control-plane | - | Promote an authority candidate. |
| admin-action | 2 | ForcePromoteAuthority | Current | control-plane | - | Force authority promotion. |
| admin-action | 10 | RetryJob | Current | control-plane | - | Retry a durable job. |
| admin-action | 11 | ReconcileJob | Current | control-plane | - | Reconcile ambiguous durable work. |
| admin-action | 12 | CancelJob | Current | control-plane | - | Cancel a durable job. |
| admin-action | 13 | QuarantineJob | Current | control-plane | - | Quarantine a durable job. |
| admin-action | 20 | BuildSnapshot | Current | bootstrap | - | Build a snapshot. |
| admin-action | 21 | VerifySnapshot | Current | bootstrap | - | Verify a snapshot. |
| admin-action | 22 | ExpireSnapshot | Current | bootstrap | - | Expire a snapshot. |
| admin-action | 30 | ScanIntegrity | Current | integrity | - | Scan integrity state. |
| admin-action | 31 | RepairIntegrity | Current | integrity | - | Repair integrity state. |
| admin-action | 40 | ExecuteGovernancePlan | Current | governance | - | Execute a reviewed governance plan. |
| admin-action | 41 | CreateLegalHold | Current | governance | - | Create a legal hold. |
| admin-action | 42 | ReleaseLegalHold | Current | governance | - | Release a legal hold. |
| admin-action | 50 | RotateKey | Current | crypto | - | Rotate a managed key. |
| admin-action | 51 | RevokeKey | Current | crypto | - | Revoke a managed key. |
| admin-action | 52 | DestroyKey | Current | crypto | - | Destroy managed key material. |
| admin-action | 60 | UpdateCompatibility | Current | compatibility | - | Update compatibility policy. |
| admin-action | 70 | DrainRegion | Current | regional | - | Drain a region. |
| admin-action | 80 | SetMaintenanceMode | Current | control-plane | - | Set maintenance mode. |
| admin-action | 81 | EmergencyStopWrites | Current | control-plane | - | Emergency stop authoritative writes. |
| admin-action | 90 | SuspendTenant | Current | control-plane | - | Suspend a tenant. |
| admin-action | 91 | SetTenantReadOnly | Current | control-plane | - | Set tenant read-only mode. |
| admin-action | 92 | RevokeDevice | Current | security | - | Revoke a device. |
| admin-action | 93 | ForceRebootstrap | Current | security | - | Force client rebootstrap. |
| admin-action | 94 | BumpScopeGeneration | Current | scope | - | Bump a scope generation. |
| admin-action | 100 | CreateExport | Current | governance | - | Create a governed export. |
| admin-action | 101 | CreateIncidentBundle | Current | diagnostics | - | Create a redacted incident bundle. |
| admin-action | 110 | UpdateDynamicConfig | Current | control-plane | - | Update dynamic configuration. |
| admin-action | 111 | RollbackDynamicConfig | Current | control-plane | - | Rollback dynamic configuration. |
| admin-action | 130 | PauseConsumer | Current | integrations | - | Pause a feed consumer. |
| admin-action | 131 | ResumeConsumer | Current | integrations | - | Resume a feed consumer. |
| admin-action | 132 | RebuildConsumer | Current | integrations | - | Rebuild a feed consumer projection. |
| admin-action | 133 | ResetConsumer | Current | integrations | - | Reset feed consumer state. |
| reason | 1 | PlannedMaintenance | Current | control-plane | - | Planned maintenance reason. |
| reason | 2 | IncidentResponse | Current | control-plane | - | Incident response reason. |
| reason | 3 | CustomerRequest | Current | control-plane | - | Customer request reason. |
| reason | 4 | SecurityCompromise | Current | security | - | Security compromise reason. |
| reason | 5 | Migration | Current | storage | - | Migration reason. |
| reason | 6 | GovernanceRequirement | Current | governance | - | Governance requirement reason. |
| reason | 7 | CapacityManagement | Current | operations | - | Capacity management reason. |
| artifact-format | 1 | SnapshotV1 | Current | bootstrap | 1 | Streaming snapshot artifact format version one. |
| artifact-format | 2 | IncidentBundleV1 | Current | diagnostics | 1 | Redacted reproducible incident bundle format version one. |
| artifact-format | 3 | FeedArchiveV1 | Current | integrations | 1 | Multi-consumer change-feed archive format version one. |
| artifact-format | 4 | ReplayBundleV1 | Current | replay | 1 | Deterministic replay bundle format version one. |
| conformance-profile | 1 | StorageCore | Current | conformance | - | Core local and authoritative storage contracts. |
| conformance-profile | 2 | StorageFullSync | Current | conformance | - | Storage plus snapshot, cursor, fencing, and anti-entropy contracts. |
| conformance-profile | 10 | ClientCore | Current | conformance | - | Core offline client behavior. |
| conformance-profile | 11 | ClientFullSync | Current | conformance | - | Full client synchronization and bounded-resource behavior. |
| conformance-profile | 20 | ServerCore | Current | conformance | - | Core authoritative server behavior. |
| conformance-profile | 21 | ServerEnterprise | Current | conformance | - | Enterprise server, provider, security, and feed behavior. |
| conformance-profile | 30 | ProtocolCore | Current | conformance | - | Protocol negotiation and canonical trace behavior. |
| conformance-profile | 40 | Provider | Current | conformance | - | Snapshot, crypto, job, and feed provider behavior. |
| conformance-profile | 50 | Integration | Current | conformance | - | Legacy bridge, extension, and application integration behavior. |
| conformance-profile | 60 | MobileClientFull | Current | mobile | - | Full mobile runtime, client, and protocol behavior. |
| conformance-profile | 61 | DesktopClientFull | Current | desktop | - | Full in-process desktop runtime, client, and protocol behavior. |
| conformance-profile | 62 | DesktopAgentFull | Current | desktop | - | Full desktop agent and local IPC behavior. |
| conformance-profile | 63 | MobileLocalStoreFull | Current | storage | - | Full target-bound Android or iOS local storage behavior. |
| conformance-profile | 64 | DesktopLocalStoreFull | Current | storage | - | Full target-bound Linux, Windows, or macOS local storage behavior. |
| conformance-profile | 65 | LocalAdapter | Current | storage | - | Local transaction, outbox, cursor, reopen, migration, and durability semantics. |
| conformance-profile | 66 | AuthoritativeAdapter | Current | storage | - | Authoritative transaction, journal, ledger, idempotency, and fencing semantics. |
| conformance-profile | 67 | SnapshotAdapter | Current | storage | - | Immutable snapshot publication, resume, verification, and atomic activation semantics. |
| conformance-profile | 68 | FencingAdapter | Current | storage | - | Lease takeover, stale-writer rejection, and monotonic fencing semantics. |
| conformance-profile | 69 | PostgresAuthorityFull | Current | storage | - | Full PostgreSQL authoritative transaction, ledger, journal, retention, restore, and readiness semantics. |
| conformance-profile | 70 | NeonOperationalProfile | Current | storage | - | Neon operational evidence layered over unchanged PostgreSQL authority semantics. |
| conformance-profile | 71 | StoolapLocalCore | Current | storage | - | Core Stoolap Tx A, Tx C, identity, migration, retry, and crash-recovery semantics. |
| conformance-profile | 72 | StoolapDesktopLocalFull | Current | storage | - | Full target-bound Stoolap local persistence on Linux, Windows, or macOS. |
| conformance-profile | 73 | StoolapMobileLocalFull | Current | storage | - | Full target-bound Stoolap local persistence on supported Android or iOS targets. |
| conformance-profile | 74 | AxumServerFull | Current | transport | - | Bounded authenticated Axum transport integration around transport-neutral server core. |
| conformance-profile | 75 | DioxusClientFull | Current | client | - | Bounded reactive Dioxus view over durable local-first client state. |
| conformance-profile | 76 | CliToolchainFull | Current | developer-experience | - | Typed, versioned, redacted, ownership-aware CLI and developer toolchain behavior. |
| conformance-profile | 77 | SQLiteLocalCore | Current | storage | - | Portable SQLite WAL, Tx A, Tx C, outbox, cursor, backup, and crash-reopen semantics. |
| conformance-profile | 78 | SQLiteDesktopLocalFull | Current | storage | - | Full target-bound SQLite local persistence on Linux, Windows, or macOS. |
| conformance-profile | 79 | SQLiteMobileLocalFull | Current | storage | - | Full target-bound SQLite local persistence on Android or iOS. |
| conformance-profile | 80 | ConfigurationRuntimeFull | Current | runtime | - | Typed, validated, redacted, atomic, capability-aware configuration and feature policy. |
| conformance-profile | 81 | ReleaseEngineeringFull | Current | release | - | Signed immutable artifacts, compatibility-gated updates, rollback safety, and traceable release provenance. |
| conformance-test | 1 | LocalIntentAtomicity | Current | conformance | - | Local mutation and durable intent commit atomically. |
| conformance-test | 2 | AuthoritativePublicationAtomicity | Current | conformance | - | Authority state, journal, ledger, and audit publish atomically. |
| conformance-test | 3 | IdempotentAuthority | Current | conformance | - | Retries produce one authoritative logical effect. |
| conformance-test | 4 | CursorAfterDurableApply | Current | conformance | - | Cursors advance only after durable apply. |
| conformance-test | 5 | AuthorityFencing | Current | conformance | - | Stale authority epochs cannot commit. |
| conformance-test | 6 | SnapshotRoundTrip | Current | conformance | - | Snapshots verify and restore exact logical state. |
| conformance-test | 7 | TombstoneAndAntiEntropy | Current | conformance | - | Repair preserves tombstones and canonical agreement. |
| conformance-test | 8 | GovernanceRestore | Current | conformance | - | Restore preserves erasure and legal-hold truth. |
| conformance-test | 9 | ProtocolUnknownIdFailClosed | Current | conformance | - | Unknown required protocol IDs fail closed. |
| conformance-test | 10 | ProtocolDifferentialTrace | Current | conformance | - | Canonical reference and subject traces agree. |
| conformance-test | 11 | ClientOfflineReplay | Current | conformance | - | Offline replay preserves operation identity and intent. |
| conformance-test | 12 | ClientResourceBounds | Current | conformance | - | Client buffering and retries obey hard bounds. |
| conformance-test | 13 | ServerFailoverFencing | Current | conformance | - | Server failover rejects stale writers. |
| conformance-test | 14 | ServerSecurityFailClosed | Current | conformance | - | Server identity and authorization failures fail closed. |
| conformance-test | 15 | CryptoKnownAnswerAndMisuse | Current | conformance | - | Crypto known-answer, rotation, and misuse tests pass. |
| conformance-test | 16 | JobExactlyOnceEffect | Current | conformance | - | Durable job retry does not duplicate logical effects. |
| conformance-test | 17 | FeedCursorAndDuplicateSafety | Current | conformance | - | Feed consumers durably effect before cursor and tolerate duplicates. |
| conformance-test | 18 | LegacyShadowAndCutover | Current | conformance | - | Legacy shadow execution is isolated and cutover is verified. |
| conformance-test | 19 | ExtensionNamespaceIsolation | Current | conformance | - | Extension IDs cannot collide with core or other namespaces. |
| conformance-test | 20 | ApplicationCapabilityTruthfulness | Current | conformance | - | Applications cannot advertise unverified capabilities. |
| conformance-test | 21 | MobileProcessDeathRecovery | Current | mobile | - | Process death resumes from durable synchronization state. |
| conformance-test | 22 | MobileAtomicLocalOutbox | Current | mobile | - | Local mutation and outbox insertion remain atomic on mobile. |
| conformance-test | 23 | MobilePushLossAndDuplication | Current | mobile | - | Lost and duplicate push hints do not affect convergence semantics. |
| conformance-test | 24 | MobileBackgroundCursorCheckpoint | Current | mobile | - | Background expiration never advances a cursor past durable apply. |
| conformance-test | 25 | MobileSecureStoreIntegration | Current | mobile | - | Mobile private keys use the approved secure provider. |
| conformance-test | 26 | MobileResourceSemanticParity | Current | mobile | - | Resource adaptation preserves all synchronization semantics. |
| conformance-test | 27 | MobileUpgradeIntentPreservation | Current | mobile | - | Mobile upgrades preserve pending intent or fail atomically. |
| conformance-test | 28 | MobileStorageFailureTruthfulness | Current | mobile | - | Unavailable durable storage never reports mutation success. |
| conformance-test | 29 | MobileBindingBoundary | Current | mobile | - | Platform bindings cannot bypass Rust synchronization semantics. |
| conformance-test | 30 | DesktopSingleCoordinator | Current | desktop | - | One fenced coordinator owns a desktop local store. |
| conformance-test | 31 | DesktopStaleFenceRejection | Current | desktop | - | Revived stale desktop processes cannot commit coordinator metadata. |
| conformance-test | 32 | DesktopSleepResumeIdempotency | Current | desktop | - | Sleep and resume recovery preserves operation idempotency. |
| conformance-test | 33 | DesktopIpcDomainBoundary | Current | desktop | - | Local IPC routes writes through domain operations only. |
| conformance-test | 34 | DesktopCloneRebinding | Current | desktop | - | Cloned desktop stores require a new device binding. |
| conformance-test | 35 | DesktopUpgradeIntentPreservation | Current | desktop | - | Desktop upgrades preserve pending operations or fail safely. |
| conformance-test | 36 | DesktopModeSemanticParity | Current | desktop | - | In-process and agent modes preserve synchronization semantics. |
| conformance-test | 37 | DesktopPersistenceFailureTruthfulness | Current | desktop | - | Low disk and credential-store failures are explicit. |
| conformance-test | 38 | DesktopDerivedStateNonAuthority | Current | desktop | - | Desktop caches and UI state remain derived and rebuildable. |
| conformance-test | 39 | StorageAtomicLocalIntent | Current | storage | - | Domain mutation and outbox commit atomically. |
| conformance-test | 40 | StorageCriticalIntentRetention | Current | storage | - | Pressure never evicts critical intent. |
| conformance-test | 41 | StorageCachePurgeIsolation | Current | storage | - | Cache purge preserves pending intent. |
| conformance-test | 42 | StorageCloneBinding | Current | storage | - | Clone restore requires secure rebinding. |
| conformance-test | 43 | StorageFormatDowngrade | Current | storage | - | Newer store formats fail closed on older binaries. |
| conformance-test | 44 | StoragePlatformCertification | Current | storage | - | Claims bind to actual target platform observations. |
| conformance-test | 45 | StorageVerifiedPublication | Current | storage | - | Staged data verifies before publication. |
| conformance-test | 46 | StorageLowDiskTruthfulness | Current | storage | - | Low disk cannot produce false commit success. |
| conformance-test | 47 | StorageSecretIsolation | Current | storage | - | Secrets remain outside ordinary sync metadata. |
| conformance-test | 48 | StorageFilesystemPlacement | Current | storage | - | Live stores avoid unverified network and cloud paths. |
| conformance-test | 49 | AdapterCapabilityConformance | Current | storage | - | Every advertised capability is backed by passing environment-bound evidence. |
| conformance-test | 50 | AdapterAtomicLocalOutbox | Current | storage | - | Local domain mutation and outbox insertion commit atomically. |
| conformance-test | 51 | AdapterAtomicAuthorityCommit | Current | storage | - | Authority business, version, journal, ledger, and audit state commit atomically. |
| conformance-test | 52 | AdapterNeutralErrorBoundary | Current | storage | - | Physical transaction and driver error types stay behind the adapter boundary. |
| conformance-test | 53 | AdapterPayloadReuseRejection | Current | storage | - | Operation identifier reuse with a different canonical digest is rejected. |
| conformance-test | 54 | AdapterMigrationPreservation | Current | storage | - | Physical migrations preserve durable synchronization identity and progress. |
| conformance-test | 55 | AdapterStartupFailClosed | Current | storage | - | Missing required storage capabilities reject startup. |
| conformance-test | 56 | AdapterEnvironmentBinding | Current | storage | - | Certification binds the adapter, engine, platform, and feature configuration. |
| conformance-test | 57 | AdapterCriticalDurability | Current | storage | - | Performance settings never weaken critical durable intent. |
| conformance-test | 58 | AdapterManifestAndLimitations | Current | storage | - | Official adapters publish stable manifests and explicit limitations. |
| conformance-test | 59 | PostgresTxBAtomicity | Current | storage | - | Business, version, journal, ledger, audit, and side-effect intents commit atomically. |
| conformance-test | 60 | PostgresIdempotentReplay | Current | storage | - | Identical operation retries return the durable logical outcome without another effect. |
| conformance-test | 61 | PostgresPayloadBinding | Current | storage | - | Changed canonical payloads cannot reuse an OperationId. |
| conformance-test | 62 | PostgresCommittedTimeline | Current | storage | - | Journal cursor order is allocated transactionally and rolls back on abort. |
| conformance-test | 63 | PostgresRestoreEpoch | Current | storage | - | PITR and restored history create a newer authority epoch before serving sync. |
| conformance-test | 64 | PostgresTypeIsolation | Current | storage | - | SQLx and PostgreSQL types remain inside the physical adapter. |
| conformance-test | 65 | PostgresReadinessSettings | Current | storage | - | Unsafe transaction settings, schema, writer, or authority state fail readiness. |
| conformance-test | 66 | PostgresDurableSideEffectIntent | Current | storage | - | Side-effect intent commits in Tx B and provider execution remains outside it. |
| conformance-test | 67 | PostgresRetentionFloor | Current | storage | - | Compaction respects active lease and bootstrap floors. |
| conformance-test | 68 | NeonSemanticParity | Current | storage | - | Neon autosuspend, pooling, branches, and scale do not change authority semantics. |
| conformance-test | 69 | StoolapTxAAtomicity | Current | storage | - | Provisional domain mutation and corresponding outbox intent commit atomically. |
| conformance-test | 70 | StoolapTxCAtomicity | Current | storage | - | Authoritative apply, outcomes, conflicts, overlay, and cursor commit atomically. |
| conformance-test | 71 | StoolapRetryIdentity | Current | storage | - | Possibly transmitted operations retain OperationId and canonical digest across crash recovery. |
| conformance-test | 72 | StoolapIntentPreservation | Current | storage | - | Rebootstrap, migration, repair, and storage reclamation preserve pending intent. |
| conformance-test | 73 | StoolapCursorSafety | Current | storage | - | Cursor advancement cannot exceed authoritative state durably installed locally. |
| conformance-test | 74 | StoolapFencedCoordinator | Current | storage | - | Only the current fenced coordinator performs leader-owned metadata transitions. |
| conformance-test | 75 | StoolapCloneRebinding | Current | storage | - | Cloned or restored stores validate device binding generation and secure-key state. |
| conformance-test | 76 | StoolapStoragePressure | Current | storage | - | Storage pressure never evicts critical pending intent or correctness metadata. |
| conformance-test | 77 | StoolapTypeIsolation | Current | storage | - | Stoolap physical types remain inside the adapter boundary. |
| conformance-test | 78 | StoolapPlatformCertification | Current | storage | - | Platform support requires a matching target-bound conformance profile. |
| conformance-test | 79 | AxumTransportIsolation | Current | transport | - | Routes orchestrate transport concerns while synchronization correctness remains in server core. |
| conformance-test | 80 | AxumCredentialIsolation | Current | transport | - | Opaque bearer credentials normalize into AuthContext and never reach domain or storage code. |
| conformance-test | 81 | AxumResourceBounds | Current | transport | - | Wire, decompressed, operation, dependency, and response work has hard bounds. |
| conformance-test | 82 | AxumCommitDeliveryIndependence | Current | transport | - | Disconnect and response failure recover through durable operation identity after commit. |
| conformance-test | 83 | AxumOverloadBounds | Current | transport | - | Global, tenant, and rate saturation reject before unbounded resource growth. |
| conformance-test | 84 | AxumIdentityBinding | Current | transport | - | Authenticated actor, tenant, and device identity is validated before execution. |
| conformance-test | 85 | AxumStableSemantics | Current | transport | - | HTTP status is accompanied by stable Aequora error semantics and request correlation. |
| conformance-test | 86 | AxumLiveHintDurability | Current | transport | - | Live HTTP hints remain advisory and cursor exchange remains authoritative. |
| conformance-test | 87 | AxumNodeEpochIndependence | Current | transport | - | HTTP node lifecycle does not alter the authority epoch. |
| conformance-test | 88 | AxumErrorSanitization | Current | transport | - | Public errors expose no database, topology, credential, or stack details. |
| conformance-test | 89 | DioxusDurableStateOwnership | Current | client | - | Correctness-critical state remains durable below Dioxus component memory. |
| conformance-test | 90 | DioxusLocalCommitTruth | Current | client | - | Saved locally is reported only after durable domain and outbox commit. |
| conformance-test | 91 | DioxusAuthorityDistinction | Current | client | - | Local durability never claims authoritative acceptance. |
| conformance-test | 92 | DioxusEventLossSafety | Current | client | - | Lost advisory events recover by rereading durable local state. |
| conformance-test | 93 | DioxusUnmountSafety | Current | client | - | Component unmount cannot erase committed operation intent. |
| conformance-test | 94 | DioxusStoreIsolation | Current | client | - | Reactive state and query caches are isolated by active store namespace. |
| conformance-test | 95 | DioxusConflictSemantics | Current | client | - | Conflict resolution creates durable semantic intent. |
| conformance-test | 96 | DioxusOfflineCapability | Current | client | - | Offline mode retains domain-permitted local reads and writes. |
| conformance-test | 97 | DioxusBackgroundOwnership | Current | client | - | Platform runtimes own OS background scheduling outside component lifetimes. |
| conformance-test | 98 | DioxusEventBounds | Current | client | - | Latest-value coalescing bounds UI event storms and slow subscribers. |
| conformance-test | 99 | CliNoInvariantBypass | Current | developer-experience | - | CLI actions remain clients of canonical SDK and control-plane boundaries. |
| conformance-test | 100 | CliMachineOutputVersioning | Current | developer-experience | - | Machine output carries a stable schema version and field names. |
| conformance-test | 101 | CliSensitiveRedaction | Current | developer-experience | - | Secrets and classified sensitive fields are redacted by default. |
| conformance-test | 102 | CliPlanApplySafety | Current | developer-experience | - | Dangerous actions require reviewed plans, reason, authorization, and step-up where applicable. |
| conformance-test | 103 | CliSubmissionAmbiguity | Current | developer-experience | - | Timeout and cancellation after durable submission stop waiting without denying execution. |
| conformance-test | 104 | CliStoreOwnership | Current | developer-experience | - | Inspection routes through agent IPC or explicit read-only/fenced direct access. |
| conformance-test | 105 | CliMigrationSafety | Current | developer-experience | - | Migration identity, checksum, source state, capability, and fencing are verified before application. |
| conformance-test | 106 | CliSemanticMutation | Current | developer-experience | - | Operational mutation uses registered semantic and control operations. |
| conformance-test | 107 | CliProductionGuard | Current | developer-experience | - | Development reset, seed, benchmark, and failpoint actions reject production targets. |
| conformance-test | 108 | CliCanonicalSemantics | Current | developer-experience | - | The CLI composes reusable canonical clients instead of reimplementing synchronization. |
| conformance-test | 109 | SQLiteWalDurability | Current | storage | - | Production replicas enforce WAL, normal synchronous durability, foreign keys, and bounded busy waits. |
| conformance-test | 110 | SQLiteSingleWriter | Current | storage | - | One logical writer serializes immediate transactions while independent WAL readers remain available. |
| conformance-test | 111 | SQLiteCursorAtomicity | Current | storage | - | Authoritative apply, operation outcomes, conflicts, and cursor advancement commit atomically. |
| conformance-test | 112 | SQLiteOutboxPreservation | Current | storage | - | Outbox identity and canonical digest survive retry, crash, backup, and lifecycle completion. |
| conformance-test | 113 | SQLiteAdapterParity | Current | storage | - | Reference, SQLite, and Stoolap local stores expose the same neutral synchronization semantics. |
| conformance-test | 114 | ConfigCorrectnessPreservation | Current | runtime | - | Configuration exposes no switch that disables correctness, authority, isolation, or idempotency. |
| conformance-test | 115 | ConfigSecretRedaction | Current | security | - | Secret values remain behind redacting wrappers and provider-aware resolution. |
| conformance-test | 116 | ConfigValidatedPublication | Current | runtime | - | Only parsed, schema-checked, cross-field validated configuration becomes effective. |
| conformance-test | 117 | ConfigAtomicReload | Current | runtime | - | Readers observe one complete immutable configuration generation. |
| conformance-test | 118 | ConfigDurableIdentityIsolation | Current | runtime | - | Authority, device, store, and generation identities are absent from editable deployment configuration. |
| conformance-test | 119 | ConfigFeatureSemanticSafety | Current | runtime | - | Runtime flags cannot silently redefine durable operation semantics. |
| conformance-test | 120 | ConfigProductionSafety | Current | security | - | Production and staging reject development authentication, reset, fault, logging, and TLS bypasses. |
| conformance-test | 121 | ConfigAdapterCapabilitySafety | Current | storage | - | Adapter settings activate only when declared certified capabilities satisfy them. |
| conformance-test | 122 | ConfigFailedReloadPreservation | Current | runtime | - | Rejected reloads leave the previous valid generation fully active. |
| conformance-test | 123 | ConfigAuthoritativePolicy | Current | security | - | Client feature decisions cannot grant business entitlement or weaken server policy. |
| conformance-test | 124 | ReleaseTraceability | Current | release | - | Every artifact binds immutable source, build configuration, target, and release identity. |
| conformance-test | 125 | ReleaseArtifactImmutability | Current | release | - | A semantic version cannot be rebound to different bytes. |
| conformance-test | 126 | ReleaseSignatureIntegrity | Current | security | - | Final bytes, hashes, signatures, key purpose, and lifecycle are verified together. |
| conformance-test | 127 | ReleaseCompatibilityGate | Current | release | - | Upgrade checks protocol, store, operation, config, snapshot, registry, and migration dimensions. |
| conformance-test | 128 | ReleaseClientIntentPreservation | Current | desktop | - | Atomic updates preserve pending operations, cursor, conflicts, store identity, and durable intent. |
| conformance-test | 129 | ReleaseRollbackSafety | Current | release | - | Rollback requires compatible state or a verified reversible migration. |
| conformance-test | 130 | ReleaseImmutablePromotion | Current | release | - | Channel promotion retains the exact candidate artifact bytes. |
| conformance-test | 131 | ReleaseCredentialIsolation | Current | security | - | Production signing and publishing credentials are absent from ordinary build jobs. |
| conformance-test | 132 | ReleaseUpdateFailClosed | Current | security | - | Unsigned, changed, expired, incompatible, halted, and revoked updates fail closed. |
| conformance-test | 133 | ReleaseVersionDimensionSeparation | Current | release | - | Product SemVer remains separate from every semantic compatibility dimension. |
| certification-tier | 1 | Experimental | Current | conformance | - | Experimental evidence without a production claim. |
| certification-tier | 10 | CoreTransactional | Current | conformance | - | Core atomic transaction and replay semantics. |
| certification-tier | 20 | FullSync | Current | conformance | - | Full synchronization, fencing, cursor, and snapshot semantics. |
| certification-tier | 30 | Enterprise | Current | conformance | - | Governance, security, provider, and operational semantics. |
