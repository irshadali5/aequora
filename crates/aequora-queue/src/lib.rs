//! Deterministic, database-neutral planning for offline outbox compaction and rebase.
//!
//! Planning never performs network I/O or mutates authority state. Unknown and audit-sensitive
//! operations fail closed to non-compactable behavior.

use aequora_protocol::{OperationEnvelope, OperationKind};
use aequora_types::{EntityRef, EntityVersion, OperationId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Stable local ordering, distinct from an authoritative journal sequence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LocalOperationSeq(pub u64);

/// Semantic compaction behavior declared for one operation kind.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum CompactionPolicy {
    /// Preserve every operation. This is the fail-closed default.
    #[default]
    Never,
    /// Preserve only the latest equivalent overwrite in a safe group.
    ReplaceLatest,
    /// Use a registered domain merger; the generic planner treats it as a barrier.
    Merge,
    /// Cancel explicitly opposite, local-only pair operations.
    CancelPairs,
    /// Use application-certified custom behavior; the generic planner treats it as a barrier.
    Custom,
}

/// Semantic rebase behavior declared for one operation kind.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum RebasePolicy {
    /// Preserve the original base and let authority conflict handling decide.
    #[default]
    Never,
    /// Re-express an unchanged overwrite intent against the latest base version.
    ReapplyIntent,
    /// Rebase only when declared fields do not overlap authority changes.
    FieldAware,
    /// Use application-certified custom behavior outside the generic planner.
    Custom,
}

/// Audit and semantic risk category used to enforce safe defaults.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum OperationRisk {
    /// Ordinary application intent whose declared policy controls optimization.
    #[default]
    Ordinary,
    /// Append-only intent whose intermediate operations normally remain observable.
    AppendOnly,
    /// Financial, ledger, approval, or otherwise audit-sensitive intent.
    FinancialAuditSensitive,
}

/// Coarse semantic shape used to document and enforce conservative queue defaults.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum OperationShape {
    /// Creates an aggregate; generic compaction does not fold later updates into it.
    Create,
    /// Changes an existing aggregate and may use an explicitly declared policy.
    Update,
    /// Deletes an aggregate; generic compaction does not erase prior intent across it.
    Delete,
    /// Appends an independently observable fact and is preserved by default.
    Append,
    /// Application-specific or unknown semantics.
    #[default]
    Other,
}

/// Result returned by an application-certified semantic compactor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompactionDecision {
    /// Preserve both operations unchanged.
    KeepBoth,
    /// Preserve only the earlier operation.
    KeepPrior,
    /// Preserve only the later operation.
    KeepNext,
    /// Cancel both local-only operations.
    CancelBoth,
    /// Replace both operations with a newly encoded equivalent intent.
    Replace(Box<OperationEnvelope>),
}

/// Application boundary for `Merge` and `Custom` compaction policies.
pub trait OperationCompactor: Send + Sync {
    /// Evaluates two never-sent operations without performing persistence.
    ///
    /// # Errors
    ///
    /// Returns a queue error when the application cannot decode or safely compare the intents.
    fn compact(
        &self,
        prior: &OperationEnvelope,
        next: &OperationEnvelope,
    ) -> Result<CompactionDecision, QueueError>;
}

/// Result returned by an application-certified semantic rebaser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RebaseDecision {
    /// Preserve the operation and let normal conflict handling decide.
    Conflict,
    /// The intent remains valid without changing its envelope.
    Unchanged,
    /// Replace the never-sent envelope while retaining application-defined semantics.
    Replace(Box<OperationEnvelope>),
}

/// Application boundary for custom payload-aware rebase semantics.
pub trait OperationRebaser: Send + Sync {
    /// Re-evaluates one never-sent operation against a newer authority target.
    ///
    /// # Errors
    ///
    /// Returns a queue error when the application cannot decode or safely re-express the intent.
    fn rebase(
        &self,
        operation: &OperationEnvelope,
        target: &RebaseTarget,
    ) -> Result<RebaseDecision, QueueError>;
}

/// Explicit mutability boundary for one durable outbox row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MutationMutability {
    /// Pending and durably known never to have entered a transport attempt.
    MutableUnsent,
    /// Selected for delivery at least once and therefore possibly authoritative.
    ImmutablePossiblyDelivered,
    /// Terminal local or authoritative outcome.
    Finalized,
}

/// Optional semantic field group used for safe grouping and field-aware rebase.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FieldGroupId(pub u16);

/// Stable application-defined operation class used in a compaction key.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct OperationClassId(pub u16);

/// Explicit access description for field-aware rebase.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum OperationAccess {
    /// Complex aggregate-wide semantics; overlap must be assumed.
    #[default]
    AggregateWide,
    /// Stable semantic field groups read or written by the operation.
    Fields {
        /// Fields whose prior values affect the operation.
        reads: BTreeSet<FieldGroupId>,
        /// Fields changed by the operation.
        writes: BTreeSet<FieldGroupId>,
    },
}

/// Opposite-pair classification for explicitly certified cancellation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PairAction {
    /// Adds or enables the identified semantic value.
    Add,
    /// Removes or disables the identified semantic value.
    Remove,
}

/// Pair token and direction supplied by an application decoder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CancellationDescriptor {
    /// Stable payload-derived token; it must not contain sensitive data in diagnostics.
    pub token: Vec<u8>,
    /// Direction of the pair operation.
    pub action: PairAction,
}

/// Complete optimization descriptor for one operation kind.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationOptimization {
    /// Generic compaction behavior.
    pub compaction: CompactionPolicy,
    /// Generic rebase behavior.
    pub rebase: RebasePolicy,
    /// Stable grouping class.
    pub operation_class: OperationClassId,
    /// Optional finer grouping key.
    pub field_group: Option<FieldGroupId>,
    /// Stops compaction across this operation.
    pub barrier: bool,
    /// Audit/semantic risk category.
    pub risk: OperationRisk,
    /// Coarse create/update/delete/append classification.
    pub shape: OperationShape,
    /// Explicit certification required to optimize sensitive operations.
    pub certified_sensitive_compaction: bool,
    /// Whether separate root correlations may be combined.
    pub allow_cross_correlation: bool,
    /// Version of application compaction semantics.
    pub policy_version: u32,
    /// Field access semantics for rebase.
    pub access: OperationAccess,
}

impl Default for OperationOptimization {
    fn default() -> Self {
        Self {
            compaction: CompactionPolicy::Never,
            rebase: RebasePolicy::Never,
            operation_class: OperationClassId(0),
            field_group: None,
            barrier: false,
            risk: OperationRisk::Ordinary,
            shape: OperationShape::Other,
            certified_sensitive_compaction: false,
            allow_cross_correlation: false,
            policy_version: 1,
            access: OperationAccess::AggregateWide,
        }
    }
}

/// Fail-closed operation optimization registry.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct OptimizationRegistry {
    descriptors: BTreeMap<u16, OperationOptimization>,
}

impl OptimizationRegistry {
    /// Registers or replaces the local optimization descriptor for an application operation kind.
    pub fn register(&mut self, kind: OperationKind, descriptor: OperationOptimization) {
        self.descriptors.insert(kind.0, descriptor);
    }

    /// Returns the descriptor or a non-compactable default for an unknown kind.
    #[must_use]
    pub fn descriptor(&self, kind: OperationKind) -> OperationOptimization {
        self.descriptors.get(&kind.0).cloned().unwrap_or_default()
    }
}

/// Durable queue input supplied by a local database adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QueueEntry {
    /// Deterministic local enqueue sequence.
    pub local_sequence: LocalOperationSeq,
    /// Complete operation envelope.
    pub operation: OperationEnvelope,
    /// Explicit row mutability derived from durable state and `ever_sent`.
    pub mutability: MutationMutability,
    /// Persisted semantic-envelope hash once delivery becomes possible.
    pub immutable_hash: Option<[u8; 32]>,
    /// Optional domain-decoded opposite-pair evidence.
    pub cancellation: Option<CancellationDescriptor>,
}

/// Why a local-only operation was superseded.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SupersessionReason {
    /// A later equivalent overwrite retained its existing identity.
    ReplacedByLatest,
    /// An explicitly opposite local-only pair canceled.
    CanceledPair,
}

/// Durable payload-free supersession evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Supersession {
    /// Local-only operation removed from the active queue.
    pub old_operation_id: OperationId,
    /// Retained replacement, or `None` when a pair canceled completely.
    pub new_operation_id: Option<OperationId>,
    /// Stable semantic reason.
    pub reason: SupersessionReason,
}

/// Deterministic bounded queue rewrite plan.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CompactionPlan {
    /// Input entries inspected.
    pub operations_before: usize,
    /// Active entries remaining after the proposed rewrite.
    pub operations_after: usize,
    /// Exact local-only operations to remove atomically.
    pub remove_operations: Vec<OperationId>,
    /// Supersession metadata persisted in the same transaction.
    pub supersessions: Vec<Supersession>,
    /// Approximate serialized bytes no longer retained in the active queue.
    pub bytes_saved: u64,
    /// Explicit barriers encountered.
    pub barriers: usize,
    /// Sensitive operations preserved by fail-closed policy.
    pub sensitive_preserved: usize,
}

/// Plans one O(N + E) dependency-aware compaction pass.
///
/// # Errors
///
/// Rejects duplicate IDs/sequences, invalid immutable hashes, excessive input, and unresolved
/// dependencies in the proposed active queue.
pub fn plan_compaction(
    entries: &[QueueEntry],
    registry: &OptimizationRegistry,
    max_operations: usize,
) -> Result<CompactionPlan, QueueError> {
    if entries.len() > max_operations {
        return Err(QueueError::OperationLimitExceeded(max_operations));
    }
    validate_entries(entries)?;
    let dependent_ids = entries
        .iter()
        .flat_map(|entry| entry.operation.metadata.dependencies.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut active = entries
        .iter()
        .map(|entry| (entry.operation.operation_id, true))
        .collect::<BTreeMap<_, _>>();
    let mut latest: BTreeMap<CompactionKey, usize> = BTreeMap::new();
    let mut pair_latest: BTreeMap<(CompactionKey, Vec<u8>), usize> = BTreeMap::new();
    let mut plan = CompactionPlan {
        operations_before: entries.len(),
        ..CompactionPlan::default()
    };
    for (index, entry) in entries.iter().enumerate() {
        let descriptor = registry.descriptor(entry.operation.operation_kind);
        if descriptor.barrier || entry.mutability != MutationMutability::MutableUnsent {
            plan.barriers = plan.barriers.saturating_add(1);
            latest.clear();
            pair_latest.clear();
            continue;
        }
        if (descriptor.risk != OperationRisk::Ordinary
            || matches!(
                descriptor.shape,
                OperationShape::Create | OperationShape::Delete | OperationShape::Append
            ))
            && !descriptor.certified_sensitive_compaction
        {
            plan.sensitive_preserved = plan.sensitive_preserved.saturating_add(1);
            continue;
        }
        let key = CompactionKey::new(entry.operation.entity, &descriptor);
        match descriptor.compaction {
            CompactionPolicy::ReplaceLatest => {
                if let Some(prior_index) = latest.get(&key).copied() {
                    let prior = &entries[prior_index];
                    if compatible_replace(prior, entry, &descriptor, &dependent_ids) {
                        supersede(
                            prior,
                            Some(entry.operation.operation_id),
                            &mut active,
                            &mut plan,
                        )?;
                    }
                }
                latest.insert(key, index);
            }
            CompactionPolicy::CancelPairs => {
                if let Some(cancellation) = &entry.cancellation {
                    let pair_key = (key, cancellation.token.clone());
                    if let Some(prior_index) = pair_latest.get(&pair_key).copied() {
                        let prior = &entries[prior_index];
                        if opposite_pair(prior, entry, &descriptor, &dependent_ids) {
                            supersede(prior, None, &mut active, &mut plan)?;
                            supersede(entry, None, &mut active, &mut plan)?;
                            pair_latest.remove(&pair_key);
                            continue;
                        }
                    }
                    pair_latest.insert(pair_key, index);
                }
            }
            CompactionPolicy::Never | CompactionPolicy::Merge | CompactionPolicy::Custom => {}
        }
    }
    verify_active_dependencies(entries, &active)?;
    plan.operations_after = active.values().filter(|active| **active).count();
    plan.remove_operations.sort_unstable();
    plan.supersessions
        .sort_by_key(|supersession| supersession.old_operation_id);
    Ok(plan)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CompactionKey {
    entity: EntityRef,
    class: OperationClassId,
    field_group: Option<FieldGroupId>,
    policy_version: u32,
}

impl CompactionKey {
    const fn new(entity: EntityRef, descriptor: &OperationOptimization) -> Self {
        Self {
            entity,
            class: descriptor.operation_class,
            field_group: descriptor.field_group,
            policy_version: descriptor.policy_version,
        }
    }
}

fn compatible_replace(
    prior: &QueueEntry,
    next: &QueueEntry,
    descriptor: &OperationOptimization,
    dependent_ids: &BTreeSet<OperationId>,
) -> bool {
    prior.mutability == MutationMutability::MutableUnsent
        && prior.operation.schema_version == next.operation.schema_version
        && (descriptor.allow_cross_correlation
            || prior.operation.metadata.lineage.correlation_id
                == next.operation.metadata.lineage.correlation_id)
        && !dependent_ids.contains(&prior.operation.operation_id)
}

fn opposite_pair(
    prior: &QueueEntry,
    next: &QueueEntry,
    descriptor: &OperationOptimization,
    dependent_ids: &BTreeSet<OperationId>,
) -> bool {
    let actions_are_opposite = matches!(
        (
            prior.cancellation.as_ref().map(|value| value.action),
            next.cancellation.as_ref().map(|value| value.action),
        ),
        (Some(PairAction::Add), Some(PairAction::Remove))
            | (Some(PairAction::Remove), Some(PairAction::Add))
    );
    actions_are_opposite
        && compatible_replace(prior, next, descriptor, dependent_ids)
        && !dependent_ids.contains(&next.operation.operation_id)
}

fn supersede(
    entry: &QueueEntry,
    replacement: Option<OperationId>,
    active: &mut BTreeMap<OperationId, bool>,
    plan: &mut CompactionPlan,
) -> Result<(), QueueError> {
    active.insert(entry.operation.operation_id, false);
    plan.remove_operations.push(entry.operation.operation_id);
    plan.bytes_saved = plan
        .bytes_saved
        .saturating_add(serialized_size(&entry.operation)?);
    plan.supersessions.push(Supersession {
        old_operation_id: entry.operation.operation_id,
        new_operation_id: replacement,
        reason: replacement.map_or(SupersessionReason::CanceledPair, |_| {
            SupersessionReason::ReplacedByLatest
        }),
    });
    Ok(())
}

fn serialized_size(operation: &OperationEnvelope) -> Result<u64, QueueError> {
    postcard::to_stdvec(operation)
        .map(|bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX))
        .map_err(|_| QueueError::Encoding)
}

/// Computes the immutable semantic envelope hash persisted before possible delivery.
///
/// # Errors
///
/// Returns [`QueueError::Encoding`] if the stable envelope cannot be encoded.
pub fn semantic_envelope_hash(operation: &OperationEnvelope) -> Result<[u8; 32], QueueError> {
    postcard::to_stdvec(operation)
        .map(|bytes| *blake3::hash(&bytes).as_bytes())
        .map_err(|_| QueueError::Encoding)
}

fn validate_entries(entries: &[QueueEntry]) -> Result<(), QueueError> {
    let mut ids = BTreeSet::new();
    let mut sequences = BTreeSet::new();
    for entry in entries {
        if !ids.insert(entry.operation.operation_id) {
            return Err(QueueError::DuplicateOperation(entry.operation.operation_id));
        }
        if !sequences.insert(entry.local_sequence) {
            return Err(QueueError::DuplicateLocalSequence(entry.local_sequence));
        }
        if entry.mutability == MutationMutability::ImmutablePossiblyDelivered
            && entry.immutable_hash != Some(semantic_envelope_hash(&entry.operation)?)
        {
            return Err(QueueError::ImmutablePayloadChanged(
                entry.operation.operation_id,
            ));
        }
    }
    Ok(())
}

fn verify_active_dependencies(
    entries: &[QueueEntry],
    active: &BTreeMap<OperationId, bool>,
) -> Result<(), QueueError> {
    let known = entries
        .iter()
        .map(|entry| entry.operation.operation_id)
        .collect::<BTreeSet<_>>();
    let mut indegree = active
        .iter()
        .filter_map(|(operation_id, is_active)| is_active.then_some((*operation_id, 0_usize)))
        .collect::<BTreeMap<_, _>>();
    let mut dependents = BTreeMap::<OperationId, Vec<OperationId>>::new();
    for entry in entries {
        if !active[&entry.operation.operation_id] {
            continue;
        }
        for dependency in &entry.operation.metadata.dependencies {
            if known.contains(dependency) && !active.get(dependency).copied().unwrap_or(false) {
                return Err(QueueError::RequiredOperationRemoved(*dependency));
            }
            if indegree.contains_key(dependency) {
                if let Some(degree) = indegree.get_mut(&entry.operation.operation_id) {
                    *degree = degree.saturating_add(1);
                }
                dependents
                    .entry(*dependency)
                    .or_default()
                    .push(entry.operation.operation_id);
            }
        }
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(operation_id, degree)| (*degree == 0).then_some(*operation_id))
        .collect::<BTreeSet<_>>();
    let mut visited = 0_usize;
    while let Some(operation_id) = ready.pop_first() {
        visited = visited.saturating_add(1);
        for dependent in dependents.get(&operation_id).into_iter().flatten() {
            if let Some(degree) = indegree.get_mut(dependent) {
                *degree = degree.saturating_sub(1);
                if *degree == 0 {
                    ready.insert(*dependent);
                }
            }
        }
    }
    if visited != indegree.len() {
        return Err(QueueError::DependencyCycle);
    }
    Ok(())
}

/// One current authority version used to analyze pending rebase.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RebaseTarget {
    /// Entity whose authoritative base changed.
    pub entity: EntityRef,
    /// Newly installed authoritative version, or `None` when absent/deleted.
    pub version: Option<EntityVersion>,
    /// Stable field groups changed since the operation's original base.
    pub changed_fields: BTreeSet<FieldGroupId>,
}

/// Safe in-place rewrite of an operation that has never been sent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RebaseRewrite {
    /// Operation identity retained because semantic intent is unchanged.
    pub operation_id: OperationId,
    /// Expected base before rebase.
    pub old_base: Option<EntityVersion>,
    /// New authoritative base.
    pub new_base: Option<EntityVersion>,
}

/// Payload-free rebase analysis.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RebasePlan {
    /// Safe base-version rewrites.
    pub rewrites: Vec<RebaseRewrite>,
    /// Operations left unchanged for normal conflict evaluation.
    pub conflicts: Vec<OperationId>,
    /// Immutable possibly-delivered operations deliberately skipped.
    pub immutable_skipped: Vec<OperationId>,
}

/// Plans rebase after bootstrap, pull, or anti-entropy repair.
///
/// # Errors
///
/// Rejects duplicate queue identities or invalid immutable envelope hashes.
pub fn plan_rebase(
    entries: &[QueueEntry],
    targets: &[RebaseTarget],
    registry: &OptimizationRegistry,
) -> Result<RebasePlan, QueueError> {
    validate_entries(entries)?;
    let targets = targets
        .iter()
        .map(|target| (target.entity, target))
        .collect::<BTreeMap<_, _>>();
    let mut plan = RebasePlan::default();
    for entry in entries {
        let Some(target) = targets.get(&entry.operation.entity) else {
            continue;
        };
        if entry.mutability != MutationMutability::MutableUnsent {
            plan.immutable_skipped.push(entry.operation.operation_id);
            continue;
        }
        let descriptor = registry.descriptor(entry.operation.operation_kind);
        let safe = match descriptor.rebase {
            RebasePolicy::ReapplyIntent => true,
            RebasePolicy::FieldAware => match &descriptor.access {
                OperationAccess::Fields { reads, writes } => reads
                    .union(writes)
                    .all(|field| !target.changed_fields.contains(field)),
                OperationAccess::AggregateWide => false,
            },
            RebasePolicy::Never | RebasePolicy::Custom => false,
        };
        if safe {
            plan.rewrites.push(RebaseRewrite {
                operation_id: entry.operation.operation_id,
                old_base: entry.operation.base_version,
                new_base: target.version,
            });
        } else {
            plan.conflicts.push(entry.operation.operation_id);
        }
    }
    Ok(plan)
}

/// Deterministic queue planning failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum QueueError {
    /// Input exceeded the configured bounded pass size.
    #[error("queue compaction exceeds the configured {0}-operation bound")]
    OperationLimitExceeded(usize),
    /// One durable operation identity appeared twice.
    #[error("duplicate queue operation {0}")]
    DuplicateOperation(OperationId),
    /// Two rows shared a local sequence.
    #[error("duplicate local operation sequence {0:?}")]
    DuplicateLocalSequence(LocalOperationSeq),
    /// A possibly delivered identity no longer maps to its original semantic envelope.
    #[error("possibly delivered operation {0} changed semantic payload")]
    ImmutablePayloadChanged(OperationId),
    /// A rewrite would remove an operation still required by an active dependency.
    #[error("compaction would remove dependency-required operation {0}")]
    RequiredOperationRemoved(OperationId),
    /// Active local dependencies contain a cycle and cannot be safely ordered.
    #[error("active outbox dependencies contain a cycle")]
    DependencyCycle,
    /// Stable envelope encoding failed.
    #[error("operation envelope encoding failed")]
    Encoding,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_protocol::{OperationKind, OperationMetadata};
    use aequora_types::{
        ActorId, DeviceId, EntityId, EntityType, HybridTimestamp, NodeId, ProtocolVersion,
        SchemaVersion, TenantId,
    };
    use proptest::prelude::*;

    fn operation(kind: u16, entity: EntityRef, payload: u8) -> OperationEnvelope {
        OperationEnvelope {
            protocol_version: ProtocolVersion::V1,
            operation_id: OperationId::new(),
            tenant_id: TenantId::new(),
            actor_id: ActorId::new(),
            device_id: DeviceId::new(),
            entity,
            base_version: Some(EntityVersion::INITIAL),
            created_at: HybridTimestamp {
                physical_ms: i64::from(payload),
                logical: 0,
                node: NodeId::new(),
            },
            schema_version: SchemaVersion(1),
            operation_kind: OperationKind(kind),
            payload: vec![payload],
            metadata: OperationMetadata::default(),
        }
    }

    fn entity() -> EntityRef {
        EntityRef {
            entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
            entity_id: EntityId::new(),
        }
    }

    fn entry(sequence: u64, operation: OperationEnvelope) -> QueueEntry {
        QueueEntry {
            local_sequence: LocalOperationSeq(sequence),
            operation,
            mutability: MutationMutability::MutableUnsent,
            immutable_hash: None,
            cancellation: None,
        }
    }

    fn replace_registry(kind: u16) -> OptimizationRegistry {
        let mut registry = OptimizationRegistry::default();
        registry.register(
            OperationKind(kind),
            OperationOptimization {
                compaction: CompactionPolicy::ReplaceLatest,
                rebase: RebasePolicy::ReapplyIntent,
                operation_class: OperationClassId(kind),
                ..OperationOptimization::default()
            },
        );
        registry
    }

    #[test]
    fn latest_unsent_overwrite_is_kept_with_durable_supersession() {
        let entity = entity();
        let first = entry(1, operation(7, entity, 1));
        let mut second_operation = operation(7, entity, 2);
        second_operation.metadata.lineage = first.operation.metadata.lineage;
        let second = entry(2, second_operation);
        let plan = plan_compaction(&[first.clone(), second.clone()], &replace_registry(7), 10)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(plan.operations_after, 1);
        assert_eq!(plan.remove_operations, vec![first.operation.operation_id]);
        assert_eq!(
            plan.supersessions[0].new_operation_id,
            Some(second.operation.operation_id)
        );
    }

    #[test]
    fn barriers_dependencies_and_finance_defaults_preserve_intent() {
        let entity = entity();
        let first = entry(1, operation(7, entity, 1));
        let mut second = entry(2, operation(7, entity, 2));
        second
            .operation
            .metadata
            .dependencies
            .push(first.operation.operation_id);
        assert!(matches!(
            plan_compaction(&[first, second], &replace_registry(7), 10),
            Ok(CompactionPlan {
                operations_after: 2,
                ..
            })
        ));

        let mut registry = replace_registry(7);
        let mut sensitive = registry.descriptor(OperationKind(7));
        sensitive.risk = OperationRisk::FinancialAuditSensitive;
        registry.register(OperationKind(7), sensitive);
        let entries = [
            entry(1, operation(7, entity, 1)),
            entry(2, operation(7, entity, 2)),
        ];
        let plan =
            plan_compaction(&entries, &registry, 10).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(plan.operations_after, 2);
        assert_eq!(plan.sensitive_preserved, 2);
    }

    #[test]
    fn dependency_cycles_and_append_intent_fail_closed() {
        let entity = entity();
        let mut first = entry(1, operation(7, entity, 1));
        let mut second = entry(2, operation(7, entity, 2));
        first
            .operation
            .metadata
            .dependencies
            .push(second.operation.operation_id);
        second
            .operation
            .metadata
            .dependencies
            .push(first.operation.operation_id);
        assert_eq!(
            plan_compaction(&[first, second], &replace_registry(7), 10),
            Err(QueueError::DependencyCycle)
        );

        for shape in [
            OperationShape::Create,
            OperationShape::Delete,
            OperationShape::Append,
        ] {
            let mut registry = replace_registry(7);
            let mut protected = registry.descriptor(OperationKind(7));
            protected.shape = shape;
            registry.register(OperationKind(7), protected);
            let entries = [
                entry(1, operation(7, entity, 1)),
                entry(2, operation(7, entity, 2)),
            ];
            let plan =
                plan_compaction(&entries, &registry, 10).unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(plan.operations_after, 2);
            assert_eq!(plan.sensitive_preserved, 2);
        }
    }

    #[test]
    fn possibly_delivered_payload_is_immutable_and_never_rebased() {
        let entity = entity();
        let mut delivered = entry(1, operation(7, entity, 1));
        delivered.mutability = MutationMutability::ImmutablePossiblyDelivered;
        delivered.immutable_hash = Some(
            semantic_envelope_hash(&delivered.operation).unwrap_or_else(|error| panic!("{error}")),
        );
        let target = RebaseTarget {
            entity,
            version: EntityVersion::INITIAL.checked_next(),
            changed_fields: BTreeSet::new(),
        };
        let plan = plan_rebase(&[delivered.clone()], &[target], &replace_registry(7))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            plan.immutable_skipped,
            vec![delivered.operation.operation_id]
        );
        delivered.operation.payload.push(9);
        assert!(matches!(
            plan_compaction(&[delivered], &replace_registry(7), 10),
            Err(QueueError::ImmutablePayloadChanged(_))
        ));
    }

    proptest! {
        #[test]
        fn replace_latest_preserves_the_final_declared_overwrite(values in prop::collection::vec(any::<u8>(), 1..32)) {
            let entity = entity();
            let mut entries = Vec::new();
            let mut lineage = None;
            for (index, value) in values.iter().copied().enumerate() {
                let mut operation = operation(7, entity, value);
                if let Some(existing) = lineage {
                    operation.metadata.lineage = existing;
                } else {
                    lineage = Some(operation.metadata.lineage);
                }
                entries.push(entry(u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1), operation));
            }
            let plan = plan_compaction(&entries, &replace_registry(7), entries.len())?;
            prop_assert_eq!(plan.operations_after, 1);
            prop_assert_eq!(plan.remove_operations.len(), entries.len().saturating_sub(1));
            prop_assert_eq!(entries.last().map(|entry| entry.operation.payload.clone()), Some(vec![*values.last().unwrap_or(&0)]));
        }
    }
}
