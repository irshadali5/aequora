use aequora::{
    AequoraAggregate, AequoraOperation,
    executor::DomainOperation,
    profile::{
        AggregateDefinition, AggregateProfileId, ConsistencyProfileKind,
        OperationProfileDefinition, OperationSemanticClass,
    },
};
use serde::Deserialize;

#[derive(AequoraOperation, Deserialize)]
#[aequora(
    kind = 0x1002,
    schema = 3,
    entity = "student",
    aggregate = 10,
    semantic = "SetValue"
)]
struct UpdateStudentPhone {
    _phone: String,
}

#[derive(AequoraAggregate)]
#[aequora(aggregate = 10, profile = "OptimisticVersioned")]
struct Student;

#[test]
fn derive_publishes_stable_operation_metadata_through_the_facade() {
    assert_eq!(UpdateStudentPhone::KIND, 0x1002);
    assert_eq!(UpdateStudentPhone::CURRENT_SCHEMA, 3);
    assert_eq!(
        <UpdateStudentPhone as OperationProfileDefinition>::AGGREGATE_ID,
        AggregateProfileId::from_static(10)
    );
    assert_eq!(
        <UpdateStudentPhone as OperationProfileDefinition>::SEMANTIC_CLASS,
        OperationSemanticClass::SetValue
    );
    assert_eq!(
        <Student as AggregateDefinition>::AGGREGATE_ID,
        AggregateProfileId::from_static(10)
    );
    assert_eq!(
        <Student as AggregateDefinition>::PROFILE,
        ConsistencyProfileKind::OptimisticVersioned
    );
}
