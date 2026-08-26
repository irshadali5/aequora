use aequora_audit::{
    AUDIT_FORMAT_VERSION, AuditAccess, AuditActionId, AuditActor, AuditCategory, AuditChange,
    AuditChangeKind, AuditDurability, AuditError, AuditEvent, AuditFieldId, AuditOutcome,
    AuditProvenance, AuditQuery, AuditRetentionClass, AuditSubject, AuditTimestamp, AuditValue,
    FieldProvenance,
};
use aequora_testkit::audit::{AuditCommitFailPoint, InMemoryAuditRepository};
use aequora_types::{
    CorrelationId, EntityId, EntityRef, EntityType, EventId, OperationId, TenantId,
};

fn event(operation: OperationId, tenant: TenantId, entity: EntityRef) -> AuditEvent {
    let action = AuditActionId::new(11).unwrap_or_else(|error| panic!("{error}"));
    AuditEvent {
        format_version: AUDIT_FORMAT_VERSION,
        audit_event_id: AuditEvent::derive_id(operation, action, 0),
        tenant_id: tenant,
        subject: AuditSubject::Entity(entity),
        action,
        actor: AuditActor::System {
            component_id: "reference-worker".to_owned(),
        },
        occurred_at: AuditTimestamp(10),
        correlation_id: CorrelationId::new(),
        outcome: AuditOutcome::Accepted,
        category: AuditCategory::BusinessChange,
        durability: AuditDurability::RequiredAtomic,
        retention: AuditRetentionClass::Standard,
        changes: Vec::new(),
        reason: None,
        provenance: AuditProvenance {
            operation_id: Some(operation),
            ..AuditProvenance::default()
        },
        corrects: None,
    }
}

#[test]
fn field_pointer_commits_only_with_its_exact_audit_and_domain_event() -> Result<(), AuditError> {
    let tenant = TenantId::new();
    let entity = EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };
    let mut event = event(OperationId::new(), tenant, entity);
    let field = AuditFieldId::new(3)?;
    let domain_event = EventId::new();
    event.changes.push(AuditChange {
        field,
        kind: AuditChangeKind::Set,
        before: None,
        after: Some(AuditValue::Digest([3; 32])),
    });
    event.provenance.authoritative_event_id = Some(domain_event);
    let pointer = FieldProvenance {
        entity,
        field,
        last_audit_event_id: event.audit_event_id,
        last_event_id: domain_event,
    };
    let mut repository = InMemoryAuditRepository::default();
    repository.inject(AuditCommitFailPoint::BeforeCommit);
    assert_eq!(
        repository.commit(std::slice::from_ref(&event), std::slice::from_ref(&pointer)),
        Err(AuditError::InjectedFailure)
    );
    assert!(repository.field_provenance(tenant, entity, field).is_none());
    repository.inject(AuditCommitFailPoint::None);
    repository.commit(std::slice::from_ref(&event), std::slice::from_ref(&pointer))?;
    assert_eq!(
        repository.field_provenance(tenant, entity, field),
        Some(&pointer)
    );
    Ok(())
}

#[test]
fn response_loss_retry_is_one_audit_effect_and_queries_remain_tenant_bounded()
-> Result<(), AuditError> {
    let tenant = TenantId::new();
    let entity = EntityRef {
        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
        entity_id: EntityId::new(),
    };
    let event = event(OperationId::new(), tenant, entity);
    let mut repository = InMemoryAuditRepository::default();
    repository.inject(AuditCommitFailPoint::AfterCommitBeforeResponse);
    assert_eq!(
        repository.commit(std::slice::from_ref(&event), &[]),
        Err(AuditError::InjectedFailure)
    );
    let retry = repository.commit(std::slice::from_ref(&event), &[])?;
    assert_eq!(retry.inserted, 0);
    assert_eq!(retry.duplicate, 1);

    let query = AuditQuery {
        tenant_id: tenant,
        subject: Some(AuditSubject::Entity(entity)),
        actor_id: None,
        operation_id: None,
        correlation_id: None,
        action: None,
        category: None,
        occurred_from: Some(AuditTimestamp(0)),
        occurred_through: Some(AuditTimestamp(u64::MAX)),
        after: None,
        limit: 10,
    };
    let access = AuditAccess::TenantAuditor { tenant_id: tenant };
    assert_eq!(repository.query(&access, &query)?.len(), 1);
    let forbidden = AuditAccess::TenantAuditor {
        tenant_id: TenantId::new(),
    };
    assert_eq!(
        repository.query(&forbidden, &query),
        Err(AuditError::Forbidden)
    );
    Ok(())
}
