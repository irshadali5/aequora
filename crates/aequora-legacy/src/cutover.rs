use crate::{
    AggregateOwnership, CatchUpState, LegacyMigrationId, LegacySourcePosition, WriteOwner,
    WriterClassification, WriterInventoryEntry,
};
use aequora_types::AuthorityEpoch;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyCutoverBoundary {
    pub legacy_position: LegacySourcePosition,
    pub authority_epoch: AuthorityEpoch,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CutoverReadiness {
    pub catch_up: CatchUpState,
    pub cdc_lag: u64,
    pub shadow_mismatch_parts_per_million: u32,
    pub max_shadow_mismatch_parts_per_million: u32,
    pub source_schema_drift: bool,
    pub critical_quarantine: u64,
    pub writers: Vec<WriterInventoryEntry>,
    pub rollback_ready: bool,
    pub backup_ready: bool,
}

impl CutoverReadiness {
    #[must_use]
    pub fn blockers(&self) -> Vec<CutoverBlocker> {
        let mut result = Vec::new();
        if self.catch_up != CatchUpState::CaughtUp || self.cdc_lag != 0 {
            result.push(CutoverBlocker::CdcNotCaughtUp);
        }
        if self.shadow_mismatch_parts_per_million > self.max_shadow_mismatch_parts_per_million {
            result.push(CutoverBlocker::ShadowMismatch);
        }
        if self.source_schema_drift {
            result.push(CutoverBlocker::SchemaDrift);
        }
        if self.critical_quarantine != 0 {
            result.push(CutoverBlocker::CriticalQuarantine);
        }
        if self
            .writers
            .iter()
            .any(|writer| writer.classification == WriterClassification::Unknown)
        {
            result.push(CutoverBlocker::UnknownWriter);
        }
        if !self.rollback_ready {
            result.push(CutoverBlocker::RollbackMissing);
        }
        if !self.backup_ready {
            result.push(CutoverBlocker::BackupMissing);
        }
        result
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CutoverBlocker {
    CdcNotCaughtUp,
    ShadowMismatch,
    SchemaDrift,
    CriticalQuarantine,
    UnknownWriter,
    RollbackMissing,
    BackupMissing,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CutoverVerification {
    pub writers_fenced: VerificationCheck,
    pub final_boundary_applied: VerificationCheck,
    pub count_checks: VerificationCheck,
    pub referential_checks: VerificationCheck,
    pub domain_invariants: VerificationCheck,
    pub canonical_digest: VerificationCheck,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum VerificationCheck {
    Passed,
    Failed,
}

impl CutoverVerification {
    #[must_use]
    pub const fn complete(&self) -> bool {
        matches!(self.writers_fenced, VerificationCheck::Passed)
            && matches!(self.final_boundary_applied, VerificationCheck::Passed)
            && matches!(self.count_checks, VerificationCheck::Passed)
            && matches!(self.referential_checks, VerificationCheck::Passed)
            && matches!(self.domain_invariants, VerificationCheck::Passed)
            && matches!(self.canonical_digest, VerificationCheck::Passed)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CutoverPlan {
    pub migration_id: LegacyMigrationId,
    pub reviewed_ownership_generation: u64,
    pub boundary: LegacyCutoverBoundary,
    pub irreversible_after_first_aequora_write: bool,
    pub reverse_bridge_tested: bool,
    pub requires_second_approval: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CutoverError {
    #[error("cutover readiness checks failed: {0:?}")]
    NotReady(Vec<CutoverBlocker>),
    #[error("cutover verification is incomplete")]
    VerificationIncomplete,
    #[error("reviewed ownership generation is stale")]
    StalePlan,
    #[error("rollback after authoritative writes requires a tested reverse bridge")]
    UnsupportedRollback,
    #[error("ownership generation overflow")]
    GenerationOverflow,
}

pub fn complete_cutover(
    plan: &CutoverPlan,
    readiness: &CutoverReadiness,
    verification: &CutoverVerification,
    ownership: &AggregateOwnership,
    now_unix_ms: u64,
) -> Result<AggregateOwnership, CutoverError> {
    let blockers = readiness.blockers();
    if !blockers.is_empty() {
        return Err(CutoverError::NotReady(blockers));
    }
    if !verification.complete() {
        return Err(CutoverError::VerificationIncomplete);
    }
    if ownership.generation != plan.reviewed_ownership_generation {
        return Err(CutoverError::StalePlan);
    }
    let generation = ownership
        .generation
        .checked_add(1)
        .ok_or(CutoverError::GenerationOverflow)?;
    Ok(AggregateOwnership {
        owner: WriteOwner::Aequora,
        generation,
        updated_at_unix_ms: now_unix_ms,
        ..ownership.clone()
    })
}

pub fn validate_rollback(
    plan: &CutoverPlan,
    aequora_writes_accepted: bool,
) -> Result<(), CutoverError> {
    if aequora_writes_accepted && !plan.reverse_bridge_tested {
        Err(CutoverError::UnsupportedRollback)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CoexistenceSurface {
    LegacySource,
    AequoraAuthority,
    BridgeStaging,
    LegacyReadProjection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LegacyControlPermission {
    View,
    Bridge,
    Cutover,
    Retire,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LegacyAdminRoute {
    Systems,
    BridgeStatus,
    CutoverPlan,
    CutoverExecute,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GovernanceCoverage {
    pub required: Vec<CoexistenceSurface>,
    pub planned: Vec<CoexistenceSurface>,
    pub legacy_retired: bool,
}

impl GovernanceCoverage {
    #[must_use]
    pub fn complete(&self) -> bool {
        self.required
            .iter()
            .all(|surface| self.planned.contains(surface))
            && (self.legacy_retired || self.required.contains(&CoexistenceSurface::LegacySource))
    }
}
