use aequora_legacy::{
    AggregateOwnership, BridgeCursor, BridgeLedgerEntry, BridgeLedgerStatus, BridgeStore,
    CatchUpState, CoexistenceSurface, CutoverError, CutoverPlan, CutoverReadiness,
    CutoverVerification, GovernanceCoverage, InMemoryBridgeStore, LegacyCutoverBoundary,
    LegacyMigrationId, LegacyProvenance, LegacyRecordKey, LegacySourcePosition, LegacySystemId,
    MappingClassification, MappingOutcome, OwnershipError, ShadowMatch, ShadowOperationId,
    ShadowPlan, VerificationCheck, WriteOwner, WritePath, WriterClassification,
    WriterInventoryEntry, WriterKind, compare_shadow, complete_cutover, validate_rollback,
};
use aequora_legacy_api::{
    CanonicalOperationHandler, CompatibilityError, LegacyAuthContext, LegacyIdentityMapper,
    LegacyPrincipal, LegacyRequestContext, LegacyRequestTranslator, RetryGuarantee,
    execute_legacy_request,
};
use aequora_types::{ActorId, AuthorityEpoch, EntityType, OperationId, TenantId};
use async_trait::async_trait;

fn entity_type() -> EntityType {
    EntityType::new(7).unwrap_or_else(|error| panic!("entity type fixture failed: {error}"))
}

fn position(value: u8) -> LegacySourcePosition {
    LegacySourcePosition::new(vec![value])
}

fn provenance(system: LegacySystemId, value: u8) -> LegacyProvenance {
    LegacyProvenance {
        system_id: system,
        record_key: LegacyRecordKey::new(vec![9]),
        source_position: position(value),
    }
}

fn ledger(system: LegacySystemId, value: u8) -> BridgeLedgerEntry {
    let provenance = provenance(system, value);
    BridgeLedgerEntry {
        dedup_key: BridgeLedgerEntry::dedup_key(
            system,
            &provenance.source_position,
            &provenance.record_key,
        ),
        provenance,
        canonical_digest: [value; 32],
        status: BridgeLedgerStatus::Applied,
        applied_event_id: None,
        applied_at_unix_ms: u64::from(value),
    }
}

#[tokio::test]
async fn cursor_and_ledger_commit_atomically_and_duplicate_delivery_is_idempotent() {
    let system = LegacySystemId::new();
    let store = InMemoryBridgeStore::default();
    assert_eq!(store.cursor(system, "main"), Ok(None));
    let first = ledger(system, 1);
    let cursor = BridgeCursor {
        legacy_system_id: system,
        stream_id: "main".to_owned(),
        source_position: position(1),
        updated_at_unix_ms: 1,
    };
    let recorded = store
        .record_result_and_advance(None, first.clone(), cursor.clone())
        .await
        .unwrap_or_else(|error| panic!("first bridge commit failed: {error}"));
    assert_eq!(recorded, first);
    assert_eq!(store.cursor(system, "main"), Ok(Some(cursor)));

    let duplicate = store
        .record_result_and_advance(
            None,
            first.clone(),
            BridgeCursor {
                legacy_system_id: system,
                stream_id: "main".to_owned(),
                source_position: position(2),
                updated_at_unix_ms: 2,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("duplicate lookup failed: {error}"));
    assert_eq!(duplicate, first);
    assert_eq!(
        store
            .cursor(system, "main")
            .map(|cursor| cursor.map(|value| value.source_position)),
        Ok(Some(position(1)))
    );
}

#[tokio::test]
async fn cdc_gap_or_crash_before_commit_never_advances_cursor() {
    let system = LegacySystemId::new();
    let store = InMemoryBridgeStore::default();
    let result = store
        .record_result_and_advance(
            Some(&position(9)),
            ledger(system, 1),
            BridgeCursor {
                legacy_system_id: system,
                stream_id: "main".to_owned(),
                source_position: position(1),
                updated_at_unix_ms: 1,
            },
        )
        .await;
    assert!(result.is_err());
    assert_eq!(store.cursor(system, "main"), Ok(None));
}

#[test]
fn write_ownership_fences_stale_generations_and_legacy_nodes() {
    let ownership = AggregateOwnership {
        tenant_id: TenantId::new(),
        aggregate_type: entity_type(),
        owner: WriteOwner::Aequora,
        generation: 4,
        updated_at_unix_ms: 10,
    };
    assert_eq!(
        ownership.authorize(WritePath::AequoraNative, 3),
        Err(OwnershipError::StaleGeneration)
    );
    assert_eq!(
        ownership.authorize(WritePath::LegacyDirect, 4),
        Err(OwnershipError::LegacyWriteAfterCutover)
    );
    assert_eq!(ownership.authorize(WritePath::LegacyFacade, 4), Ok(()));
}

fn ready() -> CutoverReadiness {
    CutoverReadiness {
        catch_up: CatchUpState::CaughtUp,
        cdc_lag: 0,
        shadow_mismatch_parts_per_million: 0,
        max_shadow_mismatch_parts_per_million: 10,
        source_schema_drift: false,
        critical_quarantine: 0,
        writers: vec![WriterInventoryEntry {
            writer_id: "api".to_owned(),
            kind: WriterKind::Api,
            classification: WriterClassification::Fenced,
        }],
        rollback_ready: true,
        backup_ready: true,
    }
}

fn plan(generation: u64) -> CutoverPlan {
    CutoverPlan {
        migration_id: LegacyMigrationId::new(),
        reviewed_ownership_generation: generation,
        boundary: LegacyCutoverBoundary {
            legacy_position: position(8),
            authority_epoch: AuthorityEpoch::INITIAL,
        },
        irreversible_after_first_aequora_write: true,
        reverse_bridge_tested: false,
        requires_second_approval: false,
    }
}

#[test]
fn cutover_requires_fence_final_boundary_and_all_canonical_checks() {
    let ownership = AggregateOwnership {
        tenant_id: TenantId::new(),
        aggregate_type: entity_type(),
        owner: WriteOwner::Migrating,
        generation: 2,
        updated_at_unix_ms: 0,
    };
    let mut verification = CutoverVerification {
        writers_fenced: VerificationCheck::Passed,
        final_boundary_applied: VerificationCheck::Failed,
        count_checks: VerificationCheck::Passed,
        referential_checks: VerificationCheck::Passed,
        domain_invariants: VerificationCheck::Passed,
        canonical_digest: VerificationCheck::Passed,
    };
    assert_eq!(
        complete_cutover(&plan(2), &ready(), &verification, &ownership, 5),
        Err(CutoverError::VerificationIncomplete)
    );
    verification.final_boundary_applied = VerificationCheck::Passed;
    let completed = complete_cutover(&plan(2), &ready(), &verification, &ownership, 5)
        .unwrap_or_else(|error| panic!("verified cutover failed: {error}"));
    assert_eq!(completed.owner, WriteOwner::Aequora);
    assert_eq!(completed.generation, 3);
    assert_eq!(
        validate_rollback(&plan(2), true),
        Err(CutoverError::UnsupportedRollback)
    );
}

#[test]
fn unknown_mapping_and_incomplete_governance_fail_closed() {
    let outcome = MappingOutcome {
        classification: MappingClassification::Unsupported,
        entity: None,
        reason_code: Some("unknown_status".to_owned()),
    };
    assert!(outcome.validated().is_err());
    let coverage = GovernanceCoverage {
        required: vec![
            CoexistenceSurface::LegacySource,
            CoexistenceSurface::AequoraAuthority,
        ],
        planned: vec![CoexistenceSurface::AequoraAuthority],
        legacy_retired: false,
    };
    assert!(!coverage.complete());
}

#[test]
fn shadow_comparison_is_semantic_and_keeps_effects_as_data() {
    let plan = ShadowPlan {
        canonical_plan: b"same".to_vec(),
        simulated_side_effect_intents: vec![b"email".to_vec()],
    };
    let result = compare_shadow(ShadowOperationId::new(), b"same", &plan);
    assert_eq!(result.match_state, ShadowMatch::Equivalent);
    assert_eq!(plan.simulated_side_effect_intents.len(), 1);
}

#[derive(Clone)]
struct Request {
    value: u8,
}
#[derive(Clone)]
struct Operation {
    id: OperationId,
    value: u8,
}
struct Identities;
impl LegacyIdentityMapper for Identities {
    fn map_identity(
        &self,
        _principal: &LegacyPrincipal,
    ) -> Result<LegacyAuthContext, CompatibilityError> {
        Ok(LegacyAuthContext {
            actor_id: ActorId::new(),
            tenant_id: TenantId::new(),
            granted_permissions: vec!["write".to_owned()],
        })
    }
}
struct Translator;
impl LegacyRequestTranslator<Request, Operation> for Translator {
    fn translate(
        &self,
        request: Request,
        _auth: LegacyAuthContext,
        operation_id: OperationId,
    ) -> Result<Operation, CompatibilityError> {
        Ok(Operation {
            id: operation_id,
            value: request.value,
        })
    }
}
struct Handler;
#[async_trait]
impl CanonicalOperationHandler<Operation> for Handler {
    type Output = (OperationId, u8);
    type Error = &'static str;
    async fn execute(&self, operation: Operation) -> Result<Self::Output, Self::Error> {
        Ok((operation.id, operation.value))
    }
}

#[tokio::test]
async fn legacy_facade_retries_route_to_typed_handler_with_stable_operation_id() {
    let context = LegacyRequestContext {
        principal: LegacyPrincipal {
            issuer: "old-auth".to_owned(),
            subject: "42".to_owned(),
            tenant_hint: "school".to_owned(),
            role_claims: vec!["teacher".to_owned()],
        },
        request_id: Some("request-7".to_owned()),
        idempotency_key: None,
    };
    let first = execute_legacy_request(
        Request { value: 9 },
        &context,
        b"erp",
        &Identities,
        &Translator,
        &Handler,
    )
    .await
    .unwrap_or_else(|error| panic!("facade execution failed: {error}"));
    let retry = execute_legacy_request(
        Request { value: 9 },
        &context,
        b"erp",
        &Identities,
        &Translator,
        &Handler,
    )
    .await
    .unwrap_or_else(|error| panic!("facade retry failed: {error}"));
    assert_eq!(first.0, retry.0);
    assert_eq!(first.1.retry_guarantee, RetryGuarantee::StableOperationId);
}
