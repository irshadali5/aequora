//! Deterministic replay repositories, captured services, compliance, and commit failpoints.

use aequora_replay::{
    CapturedExternalResult, ExecutionPlan, ExecutionPlanDigest, HandlerVersion, PlanCommitter,
    ReplayBundle, ReplayError, ReplayHandler, ReplayReport, ReplaySandbox, verify_corpus,
};
use aequora_types::OperationId;
use std::collections::BTreeMap;

/// Stable ordered canonical pre-state repository for handler and projection tests.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InMemoryReplayRepository {
    values: BTreeMap<String, Vec<u8>>,
}

impl InMemoryReplayRepository {
    pub fn insert(&mut self, key: impl Into<String>, value: Vec<u8>) -> Option<Vec<u8>> {
        self.values.insert(key.into(), value)
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.values.get(key).map(Vec::as_slice)
    }

    pub fn ordered(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.values
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_slice()))
    }
}

/// Stub external-service results that are already captured canonical input.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapturedExternalService {
    results: BTreeMap<(String, String), CapturedExternalResult>,
}

impl CapturedExternalService {
    pub fn insert(&mut self, result: CapturedExternalResult) -> Option<CapturedExternalResult> {
        self.results.insert(
            (result.provider.clone(), result.result_kind.clone()),
            result,
        )
    }

    /// Returns an exact captured result.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayError::ExternalStateUnavailable`] when the fixture was not captured.
    pub fn require(
        &self,
        provider: &str,
        result_kind: &str,
    ) -> Result<&CapturedExternalResult, ReplayError> {
        self.results
            .get(&(provider.to_owned(), result_kind.to_owned()))
            .ok_or(ReplayError::ExternalStateUnavailable)
    }
}

/// Transaction failpoint used to prove retry input and plan immutability.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlanCommitFailPoint {
    #[default]
    None,
    BeforeCommit,
    AfterCommitBeforeResponse,
}

/// Durable logical outcome retained by the in-memory plan ledger.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommittedPlan {
    pub operation_id: OperationId,
    pub handler_version: HandlerVersion,
    pub inputs_digest: [u8; 32],
    pub plan_digest: ExecutionPlanDigest,
    pub duplicate: bool,
}

/// Failpoint plan committer with durable idempotency and committed-input drift rejection.
#[derive(Clone, Debug, Default)]
pub struct InMemoryPlanCommitter {
    committed: BTreeMap<OperationId, CommittedPlan>,
    fail_point: PlanCommitFailPoint,
}

impl InMemoryPlanCommitter {
    pub fn inject(&mut self, fail_point: PlanCommitFailPoint) {
        self.fail_point = fail_point;
    }

    #[must_use]
    pub fn committed(&self, operation_id: OperationId) -> Option<CommittedPlan> {
        self.committed.get(&operation_id).copied()
    }
}

impl PlanCommitter for InMemoryPlanCommitter {
    type Outcome = CommittedPlan;

    fn commit(
        &mut self,
        operation_id: OperationId,
        handler_version: HandlerVersion,
        inputs_digest: [u8; 32],
        plan: &ExecutionPlan,
    ) -> Result<Self::Outcome, ReplayError> {
        let plan_digest = plan.digest()?;
        if let Some(previous) = self.committed.get(&operation_id).copied() {
            if previous.handler_version != handler_version
                || previous.inputs_digest != inputs_digest
                || previous.plan_digest != plan_digest
            {
                return Err(ReplayError::CommittedDecisionDrift);
            }
            return Ok(CommittedPlan {
                duplicate: true,
                ..previous
            });
        }
        if self.fail_point == PlanCommitFailPoint::BeforeCommit {
            self.fail_point = PlanCommitFailPoint::None;
            return Err(ReplayError::InjectedFailure);
        }
        let committed = CommittedPlan {
            operation_id,
            handler_version,
            inputs_digest,
            plan_digest,
            duplicate: false,
        };
        self.committed.insert(operation_id, committed);
        if self.fail_point == PlanCommitFailPoint::AfterCommitBeforeResponse {
            self.fail_point = PlanCommitFailPoint::None;
            return Err(ReplayError::InjectedFailure);
        }
        Ok(committed)
    }
}

/// Runs the reusable golden-corpus gate for an application handler.
///
/// # Errors
///
/// Returns the first bundle, handler, limit, or semantic-divergence error.
pub fn verify_replay_handler<H: ReplayHandler>(
    bundles: &[ReplayBundle],
    handler: &H,
) -> Result<Vec<ReplayReport>, ReplayError> {
    verify_corpus(&ReplaySandbox::default(), bundles, handler)
}
