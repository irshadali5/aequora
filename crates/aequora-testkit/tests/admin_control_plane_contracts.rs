use aequora_admin::{
    AdminAction, AdminCommand, AdminCommandExecutor, AdminError, AdminOperationId,
    AdminOperationStatus, AdminPrincipal, AdminReasonCode, AdminResultCode, AdminService,
    AdminTarget, AssuranceLevel, ConfigGeneration, ExecutionResult, InMemoryAdminStore,
    PermissionId, PlanReference, SafetySnapshot,
};
use aequora_types::AuthorityEpoch;
use std::{collections::BTreeSet, sync::Arc};

struct DurableAudit;

impl aequora_admin::AdminAuditSink for DurableAudit {
    fn record(&self, _event: aequora_admin::AdminAuditEvent) -> Result<(), AdminError> {
        Ok(())
    }
}

struct GuardedExecutor;

impl AdminCommandExecutor for GuardedExecutor {
    fn safety_snapshot(&self, _command: &AdminCommand) -> Result<SafetySnapshot, AdminError> {
        Ok(SafetySnapshot {
            authority_epoch: AuthorityEpoch::INITIAL,
            policy_generation: ConfigGeneration::INITIAL,
            governance_generation: 7,
            target_version: 11,
        })
    }

    fn execute(&self, action: &AdminAction) -> Result<ExecutionResult, AdminError> {
        if !matches!(action.command, AdminCommand::PromoteAuthority { .. }) {
            return Err(AdminError::UnsafeOperation);
        }
        Ok(ExecutionResult {
            result_code: AdminResultCode::Applied,
            job_id: None,
            postcondition_verified: true,
        })
    }
}

fn sre() -> AdminPrincipal {
    AdminPrincipal {
        principal_id: "sre-1".to_owned(),
        tenant_scope: None,
        permissions: BTreeSet::from([PermissionId::AuthorityPromote]),
        assurance: AssuranceLevel::Mfa,
        authenticated_at_unix_ms: 1,
        expires_at_unix_ms: 10_000,
        break_glass_session_id: None,
    }
}

#[test]
fn server_facing_contract_requires_plan_and_verified_subsystem_execution() {
    let service = AdminService::new(
        Arc::new(InMemoryAdminStore::default()),
        Arc::new(GuardedExecutor),
        Arc::new(DurableAudit),
    );
    let command = AdminCommand::PromoteAuthority {
        target: AdminTarget {
            kind: "authority".to_owned(),
            id: "candidate-a".to_owned(),
            tenant_id: None,
        },
    };
    let principal = sre();
    let plan = service
        .plan(&principal, &command, 100)
        .unwrap_or_else(|error| panic!("plan failed: {error}"));
    let record = service
        .submit(
            AdminAction {
                admin_operation_id: AdminOperationId::new(),
                actor: principal,
                reason: AdminReasonCode::IncidentResponse,
                reason_note: Some("old primary externally fenced".to_owned()),
                command,
                plan: Some(PlanReference {
                    plan_id: plan.plan_id,
                    action_digest: plan.action_digest,
                }),
            },
            101,
        )
        .unwrap_or_else(|error| panic!("submit failed: {error}"));
    assert_eq!(record.status, AdminOperationStatus::Completed);
}
