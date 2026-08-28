use aequora_compat::{
    BlockedBuildRange, CapabilityAvailability, CapabilityRequirementKind, CapabilitySet,
    ClientBuildId, ClientBuildPolicy, ClientCompatibilityRuntime, ClientHello,
    ClientSecurityPolicy, CompatibilityFailureCode, CompatibilityMode, CompatibilityPolicy,
    CompatibilityPolicyGeneration, CompatibilityResult, DurableIntentEvidence, ExtensionSection,
    FeatureState, FleetCapabilities, LocalStoreFormatVersion, NodeCapabilities, OperationAdmission,
    OperationKindId, OperationUpcaster, PlatformId, ProtocolPolicy, RecoveryInstruction,
    SemanticBuildVersion, SemanticPayloadHash, ServerAuthorityContext, ServerFeatureGate,
    SessionCacheKey, SnapshotSchemaVersion, SupportStatus, UpcasterRegistry, canonical_registry,
    capability_availability, classify_operation_attempt, ids, negotiate, validate_extensions,
};
use aequora_types::{
    AuthorityEpoch, AuthorityId, NodeId, OperationId, ProtocolVersion, SchemaVersion,
};
use std::collections::{BTreeMap, BTreeSet};

fn set<T: Ord>(values: impl IntoIterator<Item = T>) -> BTreeSet<T> {
    values.into_iter().collect()
}

fn build(number: u64) -> ClientBuildId {
    ClientBuildId {
        platform: PlatformId(1),
        version: SemanticBuildVersion {
            major: 1,
            minor: 0,
            patch: 0,
        },
        build_number: number,
    }
}

fn policy() -> CompatibilityPolicy {
    let required = [
        ids::POSTCARD_V1,
        ids::SNAPSHOT_V1,
        ids::AUTHORITY_EPOCH_V1,
        ids::NEGOTIATION_V1,
    ];
    CompatibilityPolicy {
        generation: CompatibilityPolicyGeneration(7),
        protocols: ProtocolPolicy {
            preferred: ProtocolVersion::V1,
            supported: vec![ProtocolVersion::V1, ProtocolVersion(2)],
            deprecated: BTreeSet::new(),
            forbidden: BTreeSet::new(),
            minimum_allowed: ProtocolVersion::V1,
        },
        builds: ClientBuildPolicy::default(),
        enabled_capabilities: set(required.into_iter().chain([ids::ZSTD])),
        capability_requirements: required
            .into_iter()
            .map(|id| (id, CapabilityRequirementKind::RequiredForSemantics))
            .collect(),
        supported_snapshots: set([SnapshotSchemaVersion::V1]),
        preferred_snapshot: SnapshotSchemaVersion::V1,
        change_id: 91,
    }
}

fn hello() -> ClientHello {
    ClientHello {
        client_build: build(10),
        supported: CapabilitySet {
            protocol_versions: set([ProtocolVersion::V1, ProtocolVersion(2)]),
            capabilities: set([
                ids::POSTCARD_V1,
                ids::SNAPSHOT_V1,
                ids::AUTHORITY_EPOCH_V1,
                ids::NEGOTIATION_V1,
                ids::ZSTD,
            ]),
            snapshot_versions: set([SnapshotSchemaVersion::V1]),
        },
        authority_hint: None,
        local_store_format: LocalStoreFormatVersion::V1,
        security_policy: ClientSecurityPolicy {
            minimum_protocol: ProtocolVersion::V1,
            required_capabilities: BTreeSet::new(),
        },
    }
}

fn authority() -> ServerAuthorityContext {
    ServerAuthorityContext {
        authority_id: AuthorityId::LOCAL_DEVELOPMENT,
        authority_epoch: AuthorityEpoch::INITIAL,
    }
}

#[test]
fn server_policy_selects_preferred_not_numerically_highest() {
    let result = negotiate(&hello(), &policy(), authority());
    let CompatibilityResult::Compatible(server) = result.unwrap_or_else(|error| panic!("{error}"))
    else {
        panic!("expected compatible session");
    };
    assert_eq!(server.profile.protocol, ProtocolVersion::V1);
    assert_eq!(server.profile.compression, Some(ids::ZSTD));
    assert_eq!(server.profile.mode, CompatibilityMode::Full);
}

#[test]
fn required_capability_and_protocol_downgrades_fail_closed() {
    let mut missing = hello();
    missing
        .supported
        .capabilities
        .remove(&ids::AUTHORITY_EPOCH_V1);
    let result = negotiate(&missing, &policy(), authority());
    assert!(matches!(
        result,
        Ok(CompatibilityResult::UpgradeRequired(failure))
            if failure.code == CompatibilityFailureCode::RequiredCapabilityMissing
    ));

    let mut strict = policy();
    strict.protocols.preferred = ProtocolVersion(2);
    strict.protocols.supported = vec![ProtocolVersion(2)];
    strict.protocols.minimum_allowed = ProtocolVersion(2);
    let mut stripped = hello();
    stripped.supported.protocol_versions = set([ProtocolVersion::V1]);
    let result = negotiate(&stripped, &strict, authority());
    assert!(matches!(
        result,
        Ok(CompatibilityResult::UpgradeRequired(failure))
            if failure.code == CompatibilityFailureCode::UnsupportedProtocol
    ));
}

#[test]
fn read_only_and_upgrade_states_preserve_durable_intent() {
    let mut read_only = policy();
    read_only.builds.minimum_for_write = BTreeMap::from([(PlatformId(1), 11)]);
    let result =
        negotiate(&hello(), &read_only, authority()).unwrap_or_else(|error| panic!("{error}"));
    let evidence = DurableIntentEvidence {
        pending_operations: 9,
        semantic_fingerprint: [42; 32],
    };
    let mut runtime = ClientCompatibilityRuntime::new(evidence);
    runtime.begin_negotiation();
    runtime.apply(&result);
    assert_eq!(runtime.durable_intent(), evidence);
    let CompatibilityResult::Compatible(server) = result else {
        panic!("expected read-only compatibility");
    };
    assert_eq!(server.profile.mode, CompatibilityMode::ReadOnly);
    assert_eq!(
        capability_availability(&server.profile, ids::POSTCARD_V1),
        CapabilityAvailability::ReadOnly
    );

    let mut blocked = policy();
    blocked.builds.blocked = vec![BlockedBuildRange {
        platform: PlatformId(1),
        first: 10,
        last: 10,
    }];
    let result =
        negotiate(&hello(), &blocked, authority()).unwrap_or_else(|error| panic!("{error}"));
    runtime.apply(&result);
    assert_eq!(runtime.durable_intent(), evidence);
    assert!(matches!(
        result,
        CompatibilityResult::UpgradeRequired(ref failure)
            if failure.recovery == vec![RecoveryInstruction::UpgradeClient]
    ));
}

#[test]
fn retry_only_accepts_only_the_ledger_identical_historical_operation() {
    let hash = SemanticPayloadHash::of(OperationKindId(8), SchemaVersion(1), b"original");
    assert_eq!(
        classify_operation_attempt(SupportStatus::RetryOnly, Some(hash), hash),
        OperationAdmission::AcceptHistoricalRetry
    );
    assert_eq!(
        classify_operation_attempt(
            SupportStatus::RetryOnly,
            Some(hash),
            SemanticPayloadHash::of(OperationKindId(8), SchemaVersion(1), b"changed")
        ),
        OperationAdmission::RejectPayloadMismatch
    );
    assert_eq!(
        classify_operation_attempt(SupportStatus::RetryOnly, None, hash),
        OperationAdmission::RejectNewCreation
    );

    let operation = aequora_compat::PossiblySentOperation::new(
        OperationId::new(),
        OperationKindId(8),
        SchemaVersion(1),
        b"original".to_vec(),
    );
    assert!(
        operation
            .verify_retry(SchemaVersion(1), b"original")
            .is_ok()
    );
    assert!(
        operation
            .verify_retry(SchemaVersion(2), b"original")
            .is_err()
    );
}

struct AppendVersion {
    from: SchemaVersion,
}

impl OperationUpcaster for AppendVersion {
    fn kind(&self) -> OperationKindId {
        OperationKindId(9)
    }

    fn from(&self) -> SchemaVersion {
        self.from
    }

    fn to(&self) -> SchemaVersion {
        SchemaVersion(self.from.0 + 1)
    }

    fn upcast(&self, payload: &[u8]) -> Result<Vec<u8>, aequora_compat::CompatibilityError> {
        let mut next = payload.to_vec();
        next.push(u8::try_from(self.to().0).unwrap_or(u8::MAX));
        Ok(next)
    }
}

#[test]
fn operation_upcasting_is_explicit_ordered_and_deterministic() {
    let mut registry = UpcasterRegistry::default();
    registry
        .register(AppendVersion {
            from: SchemaVersion(1),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    registry
        .register(AppendVersion {
            from: SchemaVersion(2),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    let first = registry.upcast(
        OperationKindId(9),
        SchemaVersion(1),
        SchemaVersion(3),
        b"v1",
    );
    let second = registry.upcast(
        OperationKindId(9),
        SchemaVersion(1),
        SchemaVersion(3),
        b"v1",
    );
    assert_eq!(first, second);
    assert_eq!(first.unwrap_or_default(), b"v1\x02\x03");
}

#[test]
fn mixed_fleet_cannot_activate_required_capability() {
    let mut gate = ServerFeatureGate {
        capability: ids::NEGOTIATION_V1,
        state: FeatureState::Shadow,
        canary_tenants: BTreeSet::new(),
    };
    let mut fleet = FleetCapabilities {
        nodes: vec![
            NodeCapabilities {
                node_id: NodeId::new(),
                build_number: 1,
                serving: true,
                capabilities: set([ids::NEGOTIATION_V1]),
            },
            NodeCapabilities {
                node_id: NodeId::new(),
                build_number: 2,
                serving: true,
                capabilities: BTreeSet::new(),
            },
        ],
    };
    assert!(gate.transition(FeatureState::Required, &fleet).is_err());
    fleet.nodes[1].capabilities.insert(ids::NEGOTIATION_V1);
    assert!(gate.transition(FeatureState::Required, &fleet).is_ok());
}

#[test]
fn registry_and_extensions_enforce_stable_bounded_evolution() {
    let registry = canonical_registry().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(registry.capabilities.len(), 18);
    assert!(registry.report().deprecated_protocols.is_empty());

    let extensions = vec![ExtensionSection {
        id: 77,
        required: true,
        payload: vec![1, 2, 3],
    }];
    assert!(validate_extensions(&extensions, &BTreeSet::new()).is_err());
    assert!(validate_extensions(&extensions, &set([77])).is_ok());
}

#[test]
fn stable_ids_policy_generation_and_authority_epoch_are_independent_guards() {
    let mut registry = canonical_registry().unwrap_or_else(|error| panic!("{error}"));
    registry.reserved_capability_ids.insert(ids::POSTCARD_V1);
    assert!(registry.validate().is_err());

    let mut invalid_policy = policy();
    invalid_policy.generation = CompatibilityPolicyGeneration(0);
    assert!(invalid_policy.validate(None).is_err());

    let base = SessionCacheKey {
        authority_id: AuthorityId::LOCAL_DEVELOPMENT,
        authority_epoch: AuthorityEpoch::INITIAL,
        client_build: build(10),
        policy_generation: CompatibilityPolicyGeneration(7),
        server_instance_generation: 1,
    };
    let next_epoch = SessionCacheKey {
        authority_epoch: AuthorityEpoch::new(2).unwrap_or(AuthorityEpoch::INITIAL),
        ..base
    };
    let next_policy = SessionCacheKey {
        policy_generation: CompatibilityPolicyGeneration(8),
        ..base
    };
    assert_ne!(base, next_epoch);
    assert_ne!(base, next_policy);
}

#[test]
fn client_hello_postcard_v1_golden_bytes_are_stable_and_decodable() {
    let value = hello();
    let encoded = postcard::to_stdvec(&value).unwrap_or_else(|error| panic!("{error}"));
    let expected = vec![
        1, 1, 0, 0, 10, 2, 1, 2, 5, 1, 2, 3, 16, 18, 1, 1, 0, 1, 1, 0,
    ];
    assert_eq!(encoded, expected);
    let decoded: ClientHello =
        postcard::from_bytes(&encoded).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(decoded, value);
}
