use aequora_admin::{
    AdminCommand, AdminQuery, AdminTarget, AssuranceLevel, PermissionId, RiskClass,
};
use aequora_feed::{
    ConsumerRegistration, ConsumerRetentionPolicy, FEED_INVARIANTS, RetentionDecision,
    evaluate_retention,
};
use aequora_invariants::InvariantId;
use aequora_journal::{CompactionInputs, plan_compaction};
use aequora_types::{AuthorityEpoch, AuthorityId, Sequence};
use std::collections::BTreeSet;

fn registration() -> ConsumerRegistration {
    ron::from_str(include_str!("../../../config/change-feed.ron"))
        .unwrap_or_else(|error| panic!("change-feed fixture failed: {error}"))
}

#[test]
fn example_consumer_is_safe_and_database_neutral() {
    let registration = registration();
    assert!(registration.validate().is_ok());
    assert_eq!(
        registration.retention,
        ConsumerRetentionPolicy::RebuildIfBehind
    );
    assert_eq!(registration.authorization.tenants.len(), 1);
}

#[test]
fn pinning_consumer_constrains_journal_compaction() {
    let plan = plan_compaction(CompactionInputs {
        snapshot_sequence: Sequence(100),
        minimum_active_cursor: Some(Sequence(90)),
        retention_sequence: Sequence(80),
        audit_sequence: Some(Sequence(70)),
        minimum_pinning_consumer_cursor: Some(Sequence(60)),
    });
    assert_eq!(plan.map(|plan| plan.through), Some(Sequence(60)));
}

#[test]
fn rebuildable_consumer_below_floor_does_not_guess_history() {
    let registration = registration();
    let cursor = aequora_feed::ConsumerCursor {
        consumer_id: registration.consumer_id,
        authority_id: AuthorityId::LOCAL_DEVELOPMENT,
        authority_epoch: AuthorityEpoch::INITIAL,
        sequence: Sequence(5),
    };
    assert_eq!(
        evaluate_retention(&registration, cursor, Sequence(6), Sequence(100)),
        Ok(RetentionDecision::Rebuild)
    );
}

#[test]
fn invariant_registries_agree() {
    let mut stable_ids = BTreeSet::new();
    for (suffix, feed) in (1_u16..=9).zip(FEED_INVARIANTS) {
        let stable = format!("AEQ-INV-FEED{suffix:03}");
        let invariant: InvariantId = stable
            .parse()
            .unwrap_or_else(|error| panic!("invariant lookup failed: {error}"));
        assert_eq!(invariant.as_str(), feed.id);
        assert_eq!(invariant.entry().property_test, feed.test);
        stable_ids.insert(stable);
    }
    assert_eq!(stable_ids.len(), 9);
}

#[test]
fn consumer_reset_is_a_destructive_separately_authorized_admin_action() {
    let target = AdminTarget {
        kind: "consumer".to_owned(),
        id: registration().consumer_id.as_uuid().to_string(),
        tenant_id: None,
    };
    let command = AdminCommand::ResetConsumer { target };
    assert_eq!(command.permission(), PermissionId::ConsumersReset);
    assert_eq!(command.risk(), RiskClass::Destructive);
    assert_eq!(command.required_assurance(), AssuranceLevel::Mfa);
    assert_eq!(
        AdminQuery::Consumers { tenant_id: None }.permission(),
        PermissionId::ConsumersView
    );
}
