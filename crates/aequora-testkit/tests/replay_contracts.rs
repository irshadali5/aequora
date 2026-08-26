use aequora_replay::{ExecutionPlan, HandlerVersion, PlanCommitter, ReplayError, SideEffectIntent};
use aequora_testkit::replay::{InMemoryPlanCommitter, PlanCommitFailPoint};
use aequora_types::{JobId, OperationId};

fn plan(value: u8) -> ExecutionPlan {
    ExecutionPlan {
        side_effects: vec![SideEffectIntent {
            job_id: JobId::new(),
            effect_kind: "notify".to_owned(),
            idempotency_key: "notify:fixture".to_owned(),
            canonical_payload: vec![value],
        }],
        canonical_result: vec![value],
        ..ExecutionPlan::default()
    }
}

#[test]
fn committed_but_lost_response_replays_same_plan_and_rejects_input_drift() -> Result<(), ReplayError>
{
    let operation_id = OperationId::new();
    let inputs_digest = *blake3::hash(b"inputs").as_bytes();
    let plan = plan(7);
    let mut committer = InMemoryPlanCommitter::default();
    committer.inject(PlanCommitFailPoint::AfterCommitBeforeResponse);
    assert_eq!(
        committer.commit(operation_id, HandlerVersion::V1, inputs_digest, &plan),
        Err(ReplayError::InjectedFailure)
    );

    let duplicate = committer.commit(operation_id, HandlerVersion::V1, inputs_digest, &plan)?;
    assert!(duplicate.duplicate);
    assert_eq!(
        committer.commit(
            operation_id,
            HandlerVersion::V1,
            *blake3::hash(b"changed").as_bytes(),
            &plan,
        ),
        Err(ReplayError::CommittedDecisionDrift)
    );
    Ok(())
}
