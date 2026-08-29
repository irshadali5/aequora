use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ShadowOperationId(Uuid);

impl ShadowOperationId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for ShadowOperationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ShadowMatch {
    Equivalent,
    ExpectedDifference,
    UnexpectedDifference,
    UnableToCompare,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShadowResult {
    pub operation_id: ShadowOperationId,
    pub legacy_outcome_digest: [u8; 32],
    pub aequora_plan_digest: [u8; 32],
    pub match_state: ShadowMatch,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShadowPlan {
    pub canonical_plan: Vec<u8>,
    pub simulated_side_effect_intents: Vec<Vec<u8>>,
}

/// A shadow executor can produce only an uncommitted plan and captured simulated intents.
pub trait ShadowExecutor<Operation>: Send + Sync {
    type Error;
    fn plan_without_commit_or_effects(
        &self,
        operation: &Operation,
    ) -> Result<ShadowPlan, Self::Error>;
}

#[must_use]
pub fn compare_shadow(
    operation_id: ShadowOperationId,
    legacy_semantics: &[u8],
    plan: &ShadowPlan,
) -> ShadowResult {
    let legacy = *blake3::hash(legacy_semantics).as_bytes();
    let aequora = *blake3::hash(&plan.canonical_plan).as_bytes();
    ShadowResult {
        operation_id,
        legacy_outcome_digest: legacy,
        aequora_plan_digest: aequora,
        match_state: if legacy == aequora {
            ShadowMatch::Equivalent
        } else {
            ShadowMatch::UnexpectedDifference
        },
    }
}
