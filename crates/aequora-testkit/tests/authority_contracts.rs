use aequora_authority::{
    AuthorityController, AuthorityError, AuthorityPromotionPolicy, AuthorityRole,
    AuthorityRuntimeMode, AuthorityState, CheckpointComparison, ClientAuthorityState,
    CursorDisposition, EpochOperationAction, EpochOperationState, EpochRecoveryPolicy,
    JournalCheckpoint, PromotionClass, PromotionEvidence, PromotionRequest, RecoveryVerification,
    TransitionReason, classify_epoch_operation,
};
use aequora_types::{AuthorityEpoch, AuthorityId, AuthorityInstanceId, Sequence, SyncScopeId};
use uuid::Uuid;

fn authority(value: u128) -> AuthorityId {
    AuthorityId::from_uuid(Uuid::from_u128(value))
}

fn instance(value: u128) -> AuthorityInstanceId {
    AuthorityInstanceId::from_uuid(Uuid::from_u128(value))
}

fn evidence(lag: Option<u64>) -> PromotionEvidence {
    let exact = lag == Some(0);
    PromotionEvidence {
        old_primary_externally_fenced: true,
        replication_lag: lag,
        journal_continuity: exact,
        operation_ledger_continuity: exact,
        audit_continuity: exact,
        authority_metadata_continuity: exact,
        scope_metadata_continuity: exact,
        snapshot_catalog_consistent: exact,
        governance_reconciled: true,
        side_effect_status_known: true,
    }
}

fn verification() -> RecoveryVerification {
    RecoveryVerification {
        authority_metadata_valid: true,
        journal_consistent: true,
        operation_ledger_consistent: true,
        audit_chain_valid: true,
        governance_reconciled: true,
        scope_metadata_valid: true,
        snapshot_strategy_ready: true,
        side_effect_status_known: true,
        external_epoch_valid: true,
    }
}

#[test]
fn lossless_promotion_preserves_timeline_and_rotates_fence() {
    let controller = AuthorityController::new(
        AuthorityState::new(authority(1), instance(1), AuthorityRole::Standby, 10),
        AuthorityPromotionPolicy::default(),
    );
    let outcome = controller
        .promote(
            PromotionRequest {
                class: PromotionClass::LosslessContinuation,
                reason: TransitionReason::StandbyPromotion,
                evidence: evidence(Some(0)),
                allow_data_loss: false,
                manually_approved: true,
                new_instance_id: instance(2),
                created_at_unix_ms: 11,
            },
            Some(Sequence(44)),
            Sequence(44),
        )
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(outcome.state.epoch, AuthorityEpoch::INITIAL);
    assert_eq!(outcome.state.fence_token.get(), 2);
    outcome
        .transition
        .verify()
        .unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn pitr_and_lagging_promotion_require_new_epoch_and_explicit_approval() {
    for class in [
        PromotionClass::PotentialDataLoss,
        PromotionClass::RestoredTimeline,
    ] {
        let controller = AuthorityController::new(
            AuthorityState::new(authority(1), instance(1), AuthorityRole::Recovering, 10),
            AuthorityPromotionPolicy::default(),
        );
        let request = PromotionRequest {
            class,
            reason: TransitionReason::PointInTimeRestore,
            evidence: evidence(Some(3)),
            allow_data_loss: true,
            manually_approved: true,
            new_instance_id: instance(2),
            created_at_unix_ms: 11,
        };
        let outcome = controller
            .promote(request, Some(Sequence(44)), Sequence(41))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(outcome.state.epoch.get(), 2);
        outcome
            .transition
            .verify()
            .unwrap_or_else(|error| panic!("{error}"));
    }
}

#[test]
fn old_primary_context_is_rejected_after_demotion_fence() {
    let mut state = AuthorityState::new(authority(1), instance(1), AuthorityRole::Primary, 10);
    state.runtime_mode = AuthorityRuntimeMode::Serving;
    let controller = AuthorityController::new(state, AuthorityPromotionPolicy::default());
    let old_commit_context = controller
        .authorize_write()
        .unwrap_or_else(|error| panic!("{error}"));
    controller
        .demote(11)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        controller.verify_commit_context(old_commit_context),
        Err(AuthorityError::NotPrimary(AuthorityRole::Demoted))
    );
}

#[test]
fn same_epoch_checkpoint_divergence_quarantines_authority() {
    let mut state = AuthorityState::new(authority(1), instance(1), AuthorityRole::Primary, 10);
    state.runtime_mode = AuthorityRuntimeMode::Serving;
    let controller = AuthorityController::new(state, AuthorityPromotionPolicy::default());
    let local = JournalCheckpoint {
        authority_id: authority(1),
        epoch: AuthorityEpoch::INITIAL,
        sequence: Sequence(44),
        journal_root: [1; 32],
    };
    assert_eq!(
        controller.observe_checkpoint(
            local,
            JournalCheckpoint {
                journal_root: [2; 32],
                ..local
            },
            11,
        ),
        Ok(CheckpointComparison::ForkDetected)
    );
    assert!(matches!(
        controller.authorize_write(),
        Err(AuthorityError::WritesBlocked(
            AuthorityRuntimeMode::Quarantined
        ))
    ));
}

#[test]
fn client_detects_rollback_and_requires_transition_for_higher_epoch() {
    let mut trust = ClientAuthorityState::new(Some(authority(1)));
    let first = aequora_authority::AuthorityDescriptor {
        authority_id: authority(1),
        epoch: AuthorityEpoch::INITIAL,
        role: AuthorityRole::Primary,
        instance_id: instance(1),
    };
    assert_eq!(trust.observe(first), Ok(CursorDisposition::Continue));
    let higher = AuthorityEpoch::new(2).unwrap_or_else(|error| panic!("{error}"));
    assert!(matches!(
        trust.observe(aequora_authority::AuthorityDescriptor {
            epoch: higher,
            instance_id: instance(2),
            ..first
        }),
        Ok(CursorDisposition::AuthorityChanged { .. })
    ));
    assert!(matches!(
        trust.observe(first),
        Err(AuthorityError::AuthorityRollbackDetected { .. })
    ));
}

#[test]
fn ambiguous_side_effects_are_never_classified_as_blind_safe_replay() {
    assert_eq!(
        classify_epoch_operation(
            EpochOperationState::PossiblyCommittedOldEpoch,
            EpochRecoveryPolicy::VerifyExternalState,
        ),
        EpochOperationAction::VerifyExternalState
    );
    assert_eq!(
        classify_epoch_operation(
            EpochOperationState::PossiblyCommittedOldEpoch,
            EpochRecoveryPolicy::ManualReview,
        ),
        EpochOperationAction::ManualReview
    );
}

#[test]
fn recovery_mode_blocks_writes_until_every_check_passes() {
    let mut state = AuthorityState::new(authority(1), instance(1), AuthorityRole::Primary, 10);
    state.runtime_mode = AuthorityRuntimeMode::Recovering;
    let controller = AuthorityController::new(state, AuthorityPromotionPolicy::default());
    let mut incomplete = verification();
    incomplete.governance_reconciled = false;
    assert_eq!(
        controller.complete_recovery(incomplete, 11),
        Err(AuthorityError::RecoveryVerificationIncomplete)
    );
    controller
        .complete_recovery(verification(), 12)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(controller.authorize_write().is_ok());
}

#[test]
fn cursor_is_bound_to_authority_and_epoch() {
    let state = AuthorityState::new(authority(1), instance(1), AuthorityRole::Standby, 10);
    let scope = SyncScopeId::from_uuid(Uuid::from_u128(9));
    let cursor = state.cursor(scope, Sequence(7));
    assert_eq!(cursor.authority_id, state.authority_id);
    assert_eq!(cursor.authority_epoch, state.epoch);
    assert_eq!(cursor.scope, scope);
}
