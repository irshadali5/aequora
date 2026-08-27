use aequora_profile::{
    AdapterProfileCapabilities, AggregateProfileBuilder, AggregateProfileId,
    ConsistencyProfileKind, OperationProfileBuilder, OperationSemanticClass, ProfileCapability,
    ProfileRegistry,
};
use aequora_protocol::OperationKind;
use aequora_store::TransactionCapabilities;
use aequora_testkit::profiles::verify_profile_registry;

#[test]
fn application_registry_and_adapter_capabilities_pass_one_compliance_gate()
-> Result<(), Box<dyn std::error::Error>> {
    let aggregate_id = AggregateProfileId::from_static(7);
    let mut registry = ProfileRegistry::new();
    registry.register_aggregate(
        AggregateProfileBuilder::new(
            aggregate_id,
            "student",
            ConsistencyProfileKind::OptimisticVersioned,
        )
        .build()?,
    )?;
    registry.register_operation(
        OperationProfileBuilder::new(
            OperationKind(0x7001),
            "update-student",
            aggregate_id,
            OperationSemanticClass::SetValue,
        )
        .build(),
    )?;

    let capabilities =
        AdapterProfileCapabilities::from_transactions(TransactionCapabilities::FULL_AUTHORITATIVE)
            .with(ProfileCapability::AuthoritativeClock);
    let report = verify_profile_registry(&registry, &capabilities)?;
    assert_eq!(report.aggregate_count, 1);
    assert_eq!(report.operation_count, 1);
    assert_ne!(report.manifest_digest, [0; 32]);
    Ok(())
}
