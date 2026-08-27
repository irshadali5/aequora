//! Deterministic canonical audit repository, commit failpoints, queries, and explanation fixtures.

use aequora_audit::{
    AuditAccess, AuditCategory, AuditError, AuditEvent, AuditEventId, AuditFieldId, AuditPartition,
    AuditQuery, ChainedAuditRecord, FieldProvenance,
};
use aequora_types::{EntityRef, TenantId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AuditCommitFailPoint {
    #[default]
    None,
    BeforeCommit,
    AfterCommitBeforeResponse,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditCommitOutcome {
    pub inserted: usize,
    pub duplicate: usize,
}

/// Reference append-only repository. Production adapters replace it with one native transaction.
#[derive(Clone, Debug, Default)]
pub struct InMemoryAuditRepository {
    chains: BTreeMap<(TenantId, AuditPartition), Vec<ChainedAuditRecord>>,
    event_digests: BTreeMap<(TenantId, AuditEventId), [u8; 32]>,
    field_provenance: BTreeMap<(TenantId, EntityRef, AuditFieldId), FieldProvenance>,
    fail_point: AuditCommitFailPoint,
}

impl InMemoryAuditRepository {
    pub fn inject(&mut self, fail_point: AuditCommitFailPoint) {
        self.fail_point = fail_point;
    }

    /// Atomically appends canonical events and authoritative field pointers.
    ///
    /// # Errors
    ///
    /// Rejects invalid, conflicting, or unreferenced evidence and injected commit failures.
    pub fn commit(
        &mut self,
        events: &[AuditEvent],
        field_provenance: &[FieldProvenance],
    ) -> Result<AuditCommitOutcome, AuditError> {
        let mut staged = self.clone();
        staged.fail_point = AuditCommitFailPoint::None;
        let mut outcome = AuditCommitOutcome {
            inserted: 0,
            duplicate: 0,
        };
        let mut seen = BTreeSet::new();
        for event in events {
            event.verify()?;
            if !seen.insert((event.tenant_id, event.audit_event_id)) {
                return Err(AuditError::DuplicateEventIdentity);
            }
            let digest = event.digest()?;
            if let Some(existing) = staged
                .event_digests
                .get(&(event.tenant_id, event.audit_event_id))
            {
                if *existing != digest {
                    return Err(AuditError::DuplicateEventIdentity);
                }
                outcome.duplicate += 1;
                continue;
            }
            let partition = partition_for(event.category);
            let chain = staged
                .chains
                .entry((event.tenant_id, partition))
                .or_default();
            let record = ChainedAuditRecord::append(chain.last(), partition, event.clone())?;
            chain.push(record);
            staged
                .event_digests
                .insert((event.tenant_id, event.audit_event_id), digest);
            outcome.inserted += 1;
        }
        for pointer in field_provenance {
            let source = events
                .iter()
                .find(|event| event.audit_event_id == pointer.last_audit_event_id)
                .ok_or(AuditError::DuplicateEventIdentity)?;
            if source.subject != aequora_audit::AuditSubject::Entity(pointer.entity)
                || source.provenance.authoritative_event_id != Some(pointer.last_event_id)
                || !source
                    .changes
                    .iter()
                    .any(|change| change.field == pointer.field)
            {
                return Err(AuditError::DuplicateEventIdentity);
            }
            let tenant = source.tenant_id;
            let Some(digest) = staged
                .event_digests
                .get(&(tenant, pointer.last_audit_event_id))
            else {
                return Err(AuditError::DuplicateEventIdentity);
            };
            if *digest == [0; 32] {
                return Err(AuditError::ZeroDigest);
            }
            staged
                .field_provenance
                .insert((tenant, pointer.entity, pointer.field), pointer.clone());
        }
        if self.fail_point == AuditCommitFailPoint::BeforeCommit {
            return Err(AuditError::InjectedFailure);
        }
        staged.fail_point = self.fail_point;
        *self = staged;
        if self.fail_point == AuditCommitFailPoint::AfterCommitBeforeResponse {
            self.fail_point = AuditCommitFailPoint::None;
            return Err(AuditError::InjectedFailure);
        }
        Ok(outcome)
    }

    /// Returns bounded authorized canonical events.
    ///
    /// # Errors
    ///
    /// Rejects invalid, cross-tenant, or subject-unauthorized queries.
    pub fn query(
        &self,
        access: &AuditAccess,
        query: &AuditQuery,
    ) -> Result<Vec<AuditEvent>, AuditError> {
        access.authorize(query)?;
        let mut events = self
            .chains
            .iter()
            .filter(|((tenant, _), _)| *tenant == query.tenant_id)
            .flat_map(|(_, chain)| chain.iter().map(|record| record.event.clone()))
            .filter(|event| query.matches(event))
            .collect::<Vec<_>>();
        events.sort_by_key(|event| (event.occurred_at, event.audit_event_id));
        events.truncate(query.limit);
        Ok(events)
    }

    #[must_use]
    pub fn field_provenance(
        &self,
        tenant: TenantId,
        entity: EntityRef,
        field: AuditFieldId,
    ) -> Option<&FieldProvenance> {
        self.field_provenance.get(&(tenant, entity, field))
    }

    #[must_use]
    pub fn chain(&self, tenant: TenantId, partition: AuditPartition) -> &[ChainedAuditRecord] {
        self.chains
            .get(&(tenant, partition))
            .map_or(&[], Vec::as_slice)
    }
}

const fn partition_for(category: AuditCategory) -> AuditPartition {
    match category {
        AuditCategory::BusinessChange => AuditPartition::Business,
        AuditCategory::Security | AuditCategory::Authentication => AuditPartition::Security,
        AuditCategory::Administrative | AuditCategory::Configuration => {
            AuditPartition::Administrative
        }
        AuditCategory::DataAccess | AuditCategory::Migration | AuditCategory::Repair => {
            AuditPartition::General
        }
    }
}
