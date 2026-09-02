use aequora_config::{
    AdapterCapabilities, ConfigLoader, ConfigPatch, DevelopmentFacilities, DevelopmentFacility,
    Environment, RawDeploymentConfig,
};
use aequora_feature_flags::{
    EntitlementId, FeatureContext, FeatureDefinition, FeatureEvaluator, FeatureId, FeaturePolicy,
    FeatureSafety, RolloutStage,
};
use aequora_policy::{
    AtomicRuntimeConfigStore, BatchSize, ConfigGeneration, PolicyError, PolicySnapshot,
    RuntimeConfigStore, RuntimePolicy, WorkerLimit,
};

fn capabilities() -> AdapterCapabilities {
    AdapterCapabilities {
        authoritative_transactions: true,
        local_atomicity: true,
        snapshot_generation_swap: true,
    }
}

#[test]
fn only_validated_config_reaches_effective_typestate() {
    let effective = ConfigLoader::new(RawDeploymentConfig::default())
        .cli(ConfigPatch {
            environment: Some(Environment::Test),
            ..ConfigPatch::default()
        })
        .load(ConfigGeneration(1), capabilities())
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(effective.environment, Environment::Test);

    let unsafe_production = RawDeploymentConfig {
        development: DevelopmentFacilities([DevelopmentFacility::AuthenticationBypass].into()),
        ..RawDeploymentConfig::default()
    };
    assert!(unsafe_production.validate(capabilities()).is_err());
}

#[test]
fn invalid_reload_preserves_the_complete_previous_generation() {
    let initial = PolicySnapshot::new(ConfigGeneration(9), RuntimePolicy::default())
        .unwrap_or_else(|error| panic!("{error}"));
    let store = AtomicRuntimeConfigStore::new(initial);
    let candidate = RuntimePolicy {
        worker_limit: WorkerLimit::new(12).unwrap_or_else(|error| panic!("{error}")),
        ..RuntimePolicy::default()
    };
    assert_eq!(store.reload(candidate), Err(PolicyError::RestartRequired));
    assert_eq!(store.current().generation, ConfigGeneration(9));

    let reloadable = RuntimePolicy {
        batch_size: BatchSize::new(64).unwrap_or_else(|error| panic!("{error}")),
        ..RuntimePolicy::default()
    };
    let published = store
        .reload(reloadable)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(published.generation, ConfigGeneration(10));
    assert_eq!(published.policy.batch_size.get(), 64);
}

#[test]
fn client_flag_cannot_grant_authoritative_entitlement() {
    let feature = FeatureId::new("advanced-module-ui").unwrap_or_else(|error| panic!("{error}"));
    let entitlement =
        EntitlementId::new("advanced-module").unwrap_or_else(|error| panic!("{error}"));
    let policy = FeaturePolicy::new([FeatureDefinition {
        id: feature.clone(),
        safety: FeatureSafety::PresentationOnly,
        rollout_generation: 1,
        stage: RolloutStage::On,
        required_capability: None,
        required_entitlement: Some(entitlement.clone()),
        semantic_migration: None,
        differential_evidence: false,
    }])
    .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        !policy
            .evaluate(&feature, &FeatureContext::default())
            .enabled
    );
    let context = FeatureContext {
        entitlements: [entitlement].into(),
        ..FeatureContext::default()
    };
    assert!(policy.evaluate(&feature, &context).enabled);
}
