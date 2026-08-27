//! Database-neutral authority identity, failover fencing, promotion, fork detection, and recovery.
//!
//! Infrastructure remains responsible for physically fencing independent database copies. This
//! crate makes that evidence explicit and enforces the application-level state machine at every
//! authoritative write boundary.

use aequora_types::{
    AuthorityEpoch, AuthorityId, AuthorityInstanceId, AuthorityTransitionId, Cursor, OperationId,
    Sequence,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
use thiserror::Error;

/// Monotonic application-level fencing value. Every promotion allocates a new token.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AuthorityFenceToken(u64);

impl AuthorityFenceToken {
    /// First valid fence token.
    pub const INITIAL: Self = Self(1);

    /// Creates a non-zero token.
    ///
    /// # Errors
    ///
    /// Returns [`AuthorityError::ZeroFenceToken`] for zero.
    pub const fn new(value: u64) -> Result<Self, AuthorityError> {
        if value == 0 {
            Err(AuthorityError::ZeroFenceToken)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the persistent integer representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    fn next(self) -> Result<Self, AuthorityError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(AuthorityError::FenceTokenExhausted)
    }
}

impl Default for AuthorityFenceToken {
    fn default() -> Self {
        Self::INITIAL
    }
}

/// Operational role of one authority instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthorityRole {
    /// The only role permitted to accept authoritative commits.
    Primary,
    /// Replicated promotion candidate.
    Standby,
    /// Query-only replica that is not a promotion candidate by implication.
    ReadReplica,
    /// Instance undergoing restore or integrity reconciliation.
    Recovering,
    /// Former primary permanently fenced from its old timeline.
    Demoted,
}

/// Runtime traffic mode, independent of the persistent authority role.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthorityRuntimeMode {
    /// Normal reads and authoritative writes are enabled.
    Serving,
    /// Recovery and reconciliation are still in progress.
    Recovering,
    /// Reads and administrative verification are allowed; writes are blocked.
    ReadOnlyVerification,
    /// Promotion evidence or approval is incomplete.
    PromotionPending,
    /// A fork or rollback has quarantined this instance.
    Quarantined,
}

/// Continuity claim made by a promotion or migration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PromotionClass {
    /// Journal, ledger, audit, metadata, and business state continue exactly.
    LosslessContinuation,
    /// The replacement may be missing acknowledged commits.
    PotentialDataLoss,
    /// The database was restored to an earlier point or a different history.
    RestoredTimeline,
    /// An explicitly fenced store-to-store authority migration.
    NewAuthorityMigration,
}

/// Stable reason recorded for an authority transition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TransitionReason {
    PlannedMigration,
    RegionFailover,
    PointInTimeRestore,
    CorruptionRecovery,
    StandbyPromotion,
    ForkResolution,
    OperatorDemotion,
}

/// Current persistent identity and write-fence state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityState {
    pub authority_id: AuthorityId,
    pub epoch: AuthorityEpoch,
    pub role: AuthorityRole,
    pub instance_id: AuthorityInstanceId,
    pub fence_token: AuthorityFenceToken,
    pub runtime_mode: AuthorityRuntimeMode,
    pub last_promotion_class: PromotionClass,
    pub transition_id: AuthorityTransitionId,
    pub updated_at_unix_ms: u64,
}

impl AuthorityState {
    /// Creates a first-epoch authority state. A primary starts read-only until verification.
    #[must_use]
    pub fn new(
        authority_id: AuthorityId,
        instance_id: AuthorityInstanceId,
        role: AuthorityRole,
        updated_at_unix_ms: u64,
    ) -> Self {
        Self {
            authority_id,
            epoch: AuthorityEpoch::INITIAL,
            role,
            instance_id,
            fence_token: AuthorityFenceToken::INITIAL,
            runtime_mode: if role == AuthorityRole::Primary {
                AuthorityRuntimeMode::ReadOnlyVerification
            } else {
                AuthorityRuntimeMode::Recovering
            },
            last_promotion_class: PromotionClass::LosslessContinuation,
            transition_id: AuthorityTransitionId::new(),
            updated_at_unix_ms,
        }
    }

    /// Descriptor safe to expose in protocol and diagnostics.
    #[must_use]
    pub const fn descriptor(self) -> AuthorityDescriptor {
        AuthorityDescriptor {
            authority_id: self.authority_id,
            epoch: self.epoch,
            role: self.role,
            instance_id: self.instance_id,
        }
    }

    /// Binds a cursor to this exact timeline.
    #[must_use]
    pub const fn cursor(self, scope: aequora_types::SyncScopeId, sequence: Sequence) -> Cursor {
        Cursor::new(self.authority_id, self.epoch, scope, sequence)
    }
}

/// Public identity of one authority instance and timeline.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityDescriptor {
    pub authority_id: AuthorityId,
    pub epoch: AuthorityEpoch,
    pub role: AuthorityRole,
    pub instance_id: AuthorityInstanceId,
}

/// Token embedded in an authoritative store commit and verified in its transaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityCommitContext {
    pub authority_id: AuthorityId,
    pub epoch: AuthorityEpoch,
    pub instance_id: AuthorityInstanceId,
    pub fence_token: AuthorityFenceToken,
}

/// Promotion policy selected by a deployment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityPromotionPolicy {
    pub allow_automatic_lossless: bool,
    pub require_manual_for_data_loss: bool,
    pub require_external_fence: bool,
}

impl Default for AuthorityPromotionPolicy {
    fn default() -> Self {
        Self {
            allow_automatic_lossless: false,
            require_manual_for_data_loss: true,
            require_external_fence: true,
        }
    }
}

/// Checkpoints and operator evidence used to classify a proposed promotion.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PromotionEvidence {
    pub old_primary_externally_fenced: bool,
    pub replication_lag: Option<u64>,
    pub journal_continuity: bool,
    pub operation_ledger_continuity: bool,
    pub audit_continuity: bool,
    pub authority_metadata_continuity: bool,
    pub scope_metadata_continuity: bool,
    pub snapshot_catalog_consistent: bool,
    pub governance_reconciled: bool,
    pub side_effect_status_known: bool,
}

impl PromotionEvidence {
    /// Exact continuity needed to retain the current epoch.
    #[must_use]
    pub const fn proves_lossless(self) -> bool {
        matches!(self.replication_lag, Some(0))
            && self.journal_continuity
            && self.operation_ledger_continuity
            && self.audit_continuity
            && self.authority_metadata_continuity
            && self.scope_metadata_continuity
            && self.snapshot_catalog_consistent
    }

    /// Derives the conservative readiness shown to operators.
    #[must_use]
    pub const fn readiness(self) -> PromotionReadiness {
        if self.proves_lossless() {
            PromotionReadiness::LosslessReady
        } else if let Some(gap) = self.replication_lag {
            PromotionReadiness::Lagging { estimated_gap: gap }
        } else {
            PromotionReadiness::Unknown
        }
    }
}

/// Database-neutral standby readiness result.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PromotionReadiness {
    LosslessReady,
    Lagging { estimated_gap: u64 },
    Unknown,
    Unsafe,
}

/// Explicit promotion request. Risk acceptance is never inferred from the class.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PromotionRequest {
    pub class: PromotionClass,
    pub reason: TransitionReason,
    pub evidence: PromotionEvidence,
    pub allow_data_loss: bool,
    pub manually_approved: bool,
    pub new_instance_id: AuthorityInstanceId,
    pub created_at_unix_ms: u64,
}

/// Immutable evidence linking two authority timelines.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityTransitionManifest {
    pub transition_id: AuthorityTransitionId,
    pub authority_id: AuthorityId,
    pub old_epoch: AuthorityEpoch,
    pub new_epoch: AuthorityEpoch,
    pub promotion_class: PromotionClass,
    pub old_final_sequence: Option<Sequence>,
    pub new_base_sequence: Sequence,
    pub reason: TransitionReason,
    pub created_at_unix_ms: u64,
    pub fence_token: AuthorityFenceToken,
}

impl AuthorityTransitionManifest {
    /// Canonical digest that Part 15 signing can authenticate without serializing secrets.
    #[must_use]
    pub fn canonical_digest(self) -> [u8; 32] {
        let mut hash = blake3::Hasher::new();
        hash.update(self.transition_id.as_uuid().as_bytes());
        hash.update(self.authority_id.as_uuid().as_bytes());
        hash.update(&self.old_epoch.get().to_le_bytes());
        hash.update(&self.new_epoch.get().to_le_bytes());
        hash.update(&[self.promotion_class as u8, self.reason as u8]);
        hash.update(
            &self
                .old_final_sequence
                .map_or(u64::MAX, |value| value.0)
                .to_le_bytes(),
        );
        hash.update(&self.new_base_sequence.0.to_le_bytes());
        hash.update(&self.created_at_unix_ms.to_le_bytes());
        hash.update(&self.fence_token.get().to_le_bytes());
        *hash.finalize().as_bytes()
    }

    /// Verifies the epoch rule implied by the recorded promotion class.
    ///
    /// # Errors
    ///
    /// Returns an epoch-continuity error when the class and epoch transition disagree.
    pub fn verify(self) -> Result<(), AuthorityError> {
        let same_epoch = self.old_epoch == self.new_epoch;
        match self.promotion_class {
            PromotionClass::LosslessContinuation if !same_epoch => {
                Err(AuthorityError::UnexpectedEpochChange)
            }
            PromotionClass::PotentialDataLoss | PromotionClass::RestoredTimeline if same_epoch => {
                Err(AuthorityError::UnsafeEpochReuse)
            }
            _ if self.new_epoch < self.old_epoch => Err(AuthorityError::UnsafeEpochReuse),
            _ => Ok(()),
        }
    }
}

/// Result of a successful promotion state transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PromotionOutcome {
    pub state: AuthorityState,
    pub transition: AuthorityTransitionManifest,
}

/// Serializable input for promotion dry-runs and host control-plane orchestration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityPromotionPlan {
    pub current: AuthorityState,
    pub policy: AuthorityPromotionPolicy,
    pub request: PromotionRequest,
    pub old_final_sequence: Option<Sequence>,
    pub new_base_sequence: Sequence,
}

impl AuthorityPromotionPlan {
    /// Evaluates the complete promotion state machine without mutating external infrastructure.
    ///
    /// # Errors
    ///
    /// Returns a promotion-policy or continuity error when the plan is unsafe.
    pub fn evaluate(self) -> Result<PromotionOutcome, AuthorityError> {
        AuthorityController::new(self.current, self.policy).promote(
            self.request,
            self.old_final_sequence,
            self.new_base_sequence,
        )
    }
}

/// All checks required before recovery mode can enable writes.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoveryVerification {
    pub authority_metadata_valid: bool,
    pub journal_consistent: bool,
    pub operation_ledger_consistent: bool,
    pub audit_chain_valid: bool,
    pub governance_reconciled: bool,
    pub scope_metadata_valid: bool,
    pub snapshot_strategy_ready: bool,
    pub side_effect_status_known: bool,
    pub external_epoch_valid: bool,
}

impl RecoveryVerification {
    /// True only when every fail-closed recovery check passed.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        self.authority_metadata_valid
            && self.journal_consistent
            && self.operation_ledger_consistent
            && self.audit_chain_valid
            && self.governance_reconciled
            && self.scope_metadata_valid
            && self.snapshot_strategy_ready
            && self.side_effect_status_known
            && self.external_epoch_valid
    }
}

/// Signed/checkpointable journal state used to detect same-epoch divergence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JournalCheckpoint {
    pub authority_id: AuthorityId,
    pub epoch: AuthorityEpoch,
    pub sequence: Sequence,
    pub journal_root: [u8; 32],
}

/// Result of comparing two authority checkpoints.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointComparison {
    Match,
    DifferentPosition,
    DifferentTimeline,
    ForkDetected,
}

/// Compares checkpoints without ever attempting to merge histories.
#[must_use]
pub fn compare_checkpoints(
    local: JournalCheckpoint,
    peer: JournalCheckpoint,
) -> CheckpointComparison {
    if local.authority_id != peer.authority_id || local.epoch != peer.epoch {
        CheckpointComparison::DifferentTimeline
    } else if local.sequence.0 != peer.sequence.0 {
        CheckpointComparison::DifferentPosition
    } else if local.journal_root != peer.journal_root {
        CheckpointComparison::ForkDetected
    } else {
        CheckpointComparison::Match
    }
}

/// Validates and mutates one authority's runtime failover state.
#[derive(Clone)]
pub struct AuthorityController {
    state: Arc<RwLock<AuthorityState>>,
    policy: AuthorityPromotionPolicy,
    external_highest_epoch: Arc<RwLock<Option<AuthorityEpoch>>>,
}

impl AuthorityController {
    /// Creates a controller around state loaded from the authoritative metadata store.
    #[must_use]
    pub fn new(state: AuthorityState, policy: AuthorityPromotionPolicy) -> Self {
        Self {
            state: Arc::new(RwLock::new(state)),
            policy,
            external_highest_epoch: Arc::new(RwLock::new(None)),
        }
    }

    /// Returns a consistent state snapshot.
    ///
    /// # Errors
    ///
    /// Fails closed if another thread poisoned the controller state.
    pub fn state(&self) -> Result<AuthorityState, AuthorityError> {
        self.state
            .read()
            .map(|state| *state)
            .map_err(|_| AuthorityError::ControllerPoisoned)
    }

    /// Installs the control-plane rollback anchor checked before any write.
    ///
    /// # Errors
    ///
    /// Returns an epoch regression error or a poisoned-controller error.
    pub fn set_external_highest_epoch(&self, epoch: AuthorityEpoch) -> Result<(), AuthorityError> {
        let state = self.state()?;
        if epoch < state.epoch {
            return Err(AuthorityError::ExternalEpochRegression {
                external: epoch,
                local: state.epoch,
            });
        }
        let mut anchor = self
            .external_highest_epoch
            .write()
            .map_err(|_| AuthorityError::ControllerPoisoned)?;
        if anchor.is_some_and(|current| epoch < current) {
            return Err(AuthorityError::ExternalEpochRegression {
                external: epoch,
                local: anchor.unwrap_or(state.epoch),
            });
        }
        *anchor = Some(epoch);
        Ok(())
    }

    /// Validates startup metadata and rollback protection before traffic is enabled.
    ///
    /// # Errors
    ///
    /// Returns an authority metadata, rollback, or poisoned-controller error.
    pub fn validate_startup(&self) -> Result<(), AuthorityError> {
        let state = self.state()?;
        self.ensure_external_epoch(state.epoch)?;
        if state.role == AuthorityRole::Primary && state.fence_token.get() == 0 {
            return Err(AuthorityError::ZeroFenceToken);
        }
        Ok(())
    }

    /// Returns a token that the authoritative database transaction must compare to stored state.
    ///
    /// # Errors
    ///
    /// Returns an error unless this instance is the serving primary on a valid external epoch.
    pub fn authorize_write(&self) -> Result<AuthorityCommitContext, AuthorityError> {
        let state = self.state()?;
        self.ensure_external_epoch(state.epoch)?;
        if state.role != AuthorityRole::Primary {
            return Err(AuthorityError::NotPrimary(state.role));
        }
        if state.runtime_mode != AuthorityRuntimeMode::Serving {
            return Err(AuthorityError::WritesBlocked(state.runtime_mode));
        }
        Ok(AuthorityCommitContext {
            authority_id: state.authority_id,
            epoch: state.epoch,
            instance_id: state.instance_id,
            fence_token: state.fence_token,
        })
    }

    /// Verifies a commit token against the current state immediately before persistence.
    ///
    /// # Errors
    ///
    /// Returns an authority-state error or [`AuthorityError::StaleFence`].
    pub fn verify_commit_context(
        &self,
        context: AuthorityCommitContext,
    ) -> Result<(), AuthorityError> {
        let current = self.authorize_write()?;
        if current == context {
            Ok(())
        } else {
            Err(AuthorityError::StaleFence)
        }
    }

    /// Performs a fail-closed promotion after evaluating evidence and explicit approvals.
    ///
    /// # Errors
    ///
    /// Returns a fencing, continuity, approval, exhaustion, or controller-state error.
    pub fn promote(
        &self,
        request: PromotionRequest,
        old_final_sequence: Option<Sequence>,
        new_base_sequence: Sequence,
    ) -> Result<PromotionOutcome, AuthorityError> {
        if self.policy.require_external_fence && !request.evidence.old_primary_externally_fenced {
            return Err(AuthorityError::ExternalFenceRequired);
        }
        let continuity_proven = request.evidence.proves_lossless();
        if request.class == PromotionClass::LosslessContinuation && !continuity_proven {
            return Err(AuthorityError::LosslessContinuityUnproven);
        }
        let risky = matches!(
            request.class,
            PromotionClass::PotentialDataLoss | PromotionClass::RestoredTimeline
        ) || (request.class == PromotionClass::NewAuthorityMigration
            && !continuity_proven);
        if risky && !request.allow_data_loss {
            return Err(AuthorityError::DataLossApprovalRequired);
        }
        if risky && self.policy.require_manual_for_data_loss && !request.manually_approved {
            return Err(AuthorityError::ManualApprovalRequired);
        }
        let mut state = self
            .state
            .write()
            .map_err(|_| AuthorityError::ControllerPoisoned)?;
        let old_epoch = state.epoch;
        let retain_epoch = request.class == PromotionClass::LosslessContinuation
            || (request.class == PromotionClass::NewAuthorityMigration && continuity_proven);
        let new_epoch = if retain_epoch {
            old_epoch
        } else {
            old_epoch
                .checked_next()
                .ok_or(AuthorityError::EpochExhausted)?
        };
        let new_fence = state.fence_token.next()?;
        let transition_id = AuthorityTransitionId::new();
        *state = AuthorityState {
            authority_id: state.authority_id,
            epoch: new_epoch,
            role: AuthorityRole::Primary,
            instance_id: request.new_instance_id,
            fence_token: new_fence,
            runtime_mode: AuthorityRuntimeMode::ReadOnlyVerification,
            last_promotion_class: request.class,
            transition_id,
            updated_at_unix_ms: request.created_at_unix_ms,
        };
        let transition = AuthorityTransitionManifest {
            transition_id,
            authority_id: state.authority_id,
            old_epoch,
            new_epoch,
            promotion_class: request.class,
            old_final_sequence,
            new_base_sequence,
            reason: request.reason,
            created_at_unix_ms: request.created_at_unix_ms,
            fence_token: new_fence,
        };
        Ok(PromotionOutcome {
            state: *state,
            transition,
        })
    }

    /// Demotes and fences the current instance. Rejoin requires reinitialization and promotion.
    ///
    /// # Errors
    ///
    /// Returns a fence-exhaustion or poisoned-controller error.
    pub fn demote(&self, updated_at_unix_ms: u64) -> Result<AuthorityState, AuthorityError> {
        let mut state = self
            .state
            .write()
            .map_err(|_| AuthorityError::ControllerPoisoned)?;
        state.role = AuthorityRole::Demoted;
        state.runtime_mode = AuthorityRuntimeMode::Recovering;
        state.fence_token = state.fence_token.next()?;
        state.transition_id = AuthorityTransitionId::new();
        state.updated_at_unix_ms = updated_at_unix_ms;
        Ok(*state)
    }

    /// Moves the current instance into a write-blocked recovery mode.
    ///
    /// # Errors
    ///
    /// Returns [`AuthorityError::ControllerPoisoned`] if state cannot be updated.
    pub fn begin_recovery(&self, updated_at_unix_ms: u64) -> Result<(), AuthorityError> {
        let mut state = self
            .state
            .write()
            .map_err(|_| AuthorityError::ControllerPoisoned)?;
        state.runtime_mode = AuthorityRuntimeMode::Recovering;
        state.updated_at_unix_ms = updated_at_unix_ms;
        Ok(())
    }

    /// Enables serving only after every recovery verification succeeds.
    ///
    /// # Errors
    ///
    /// Returns an incomplete-verification, role, rollback, or controller-state error.
    pub fn complete_recovery(
        &self,
        verification: RecoveryVerification,
        updated_at_unix_ms: u64,
    ) -> Result<(), AuthorityError> {
        if !verification.is_complete() {
            return Err(AuthorityError::RecoveryVerificationIncomplete);
        }
        let mut state = self
            .state
            .write()
            .map_err(|_| AuthorityError::ControllerPoisoned)?;
        self.ensure_external_epoch(state.epoch)?;
        if state.role != AuthorityRole::Primary {
            return Err(AuthorityError::NotPrimary(state.role));
        }
        state.runtime_mode = AuthorityRuntimeMode::Serving;
        state.updated_at_unix_ms = updated_at_unix_ms;
        Ok(())
    }

    /// Compares a peer checkpoint and quarantines this authority when a fork is proven.
    ///
    /// # Errors
    ///
    /// Returns [`AuthorityError::ControllerPoisoned`] if quarantine state cannot be persisted.
    pub fn observe_checkpoint(
        &self,
        local: JournalCheckpoint,
        peer: JournalCheckpoint,
        updated_at_unix_ms: u64,
    ) -> Result<CheckpointComparison, AuthorityError> {
        let comparison = compare_checkpoints(local, peer);
        if comparison == CheckpointComparison::ForkDetected {
            let mut state = self
                .state
                .write()
                .map_err(|_| AuthorityError::ControllerPoisoned)?;
            state.runtime_mode = AuthorityRuntimeMode::Quarantined;
            state.updated_at_unix_ms = updated_at_unix_ms;
        }
        Ok(comparison)
    }

    fn ensure_external_epoch(&self, local: AuthorityEpoch) -> Result<(), AuthorityError> {
        let external = *self
            .external_highest_epoch
            .read()
            .map_err(|_| AuthorityError::ControllerPoisoned)?;
        if external.is_some_and(|highest| local < highest) {
            Err(AuthorityError::AuthorityRollbackDetected {
                presented: local,
                highest: external.unwrap_or(local),
            })
        } else {
            Ok(())
        }
    }
}

/// Server-side cursor decision for one request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorDisposition {
    Continue,
    AuthorityChanged {
        previous: AuthorityEpoch,
        current: AuthorityEpoch,
    },
}

/// Validates authority ID, rollback, and epoch continuity before reading a cursor.
///
/// # Errors
///
/// Returns an identity-change or epoch-rollback error.
pub fn validate_cursor(
    state: AuthorityState,
    cursor: Cursor,
) -> Result<CursorDisposition, AuthorityError> {
    if cursor.authority_id == AuthorityId::LEGACY_UNBOUND {
        return Ok(CursorDisposition::AuthorityChanged {
            previous: cursor.authority_epoch,
            current: state.epoch,
        });
    }
    if cursor.authority_id != state.authority_id {
        return Err(AuthorityError::AuthorityIdChanged {
            trusted: cursor.authority_id,
            presented: state.authority_id,
        });
    }
    if cursor.authority_epoch > state.epoch {
        return Err(AuthorityError::AuthorityRollbackDetected {
            presented: state.epoch,
            highest: cursor.authority_epoch,
        });
    }
    if cursor.authority_epoch < state.epoch {
        Ok(CursorDisposition::AuthorityChanged {
            previous: cursor.authority_epoch,
            current: state.epoch,
        })
    } else {
        Ok(CursorDisposition::Continue)
    }
}

/// Per-operation policy used when an old timeline may have committed an operation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EpochRecoveryPolicy {
    SafeReplay,
    VerifyExternalState,
    ManualReview,
    DropIfDerived,
}

/// Durable knowledge the client has about an operation crossing an epoch boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EpochOperationState {
    Unsent,
    ConfirmedCommitted,
    PossiblyCommittedOldEpoch,
    Rejected,
}

/// Required action produced by explicit recovery policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EpochOperationAction {
    Submit,
    RecordRollback,
    ReplaySameId,
    VerifyExternalState,
    ManualReview,
    DropDerived,
    RetainTerminal,
}

/// Classifies pending intent without blindly replaying ambiguous old-epoch side effects.
#[must_use]
pub const fn classify_epoch_operation(
    state: EpochOperationState,
    policy: EpochRecoveryPolicy,
) -> EpochOperationAction {
    match state {
        EpochOperationState::Unsent => EpochOperationAction::Submit,
        EpochOperationState::ConfirmedCommitted => EpochOperationAction::RecordRollback,
        EpochOperationState::Rejected => EpochOperationAction::RetainTerminal,
        EpochOperationState::PossiblyCommittedOldEpoch => match policy {
            EpochRecoveryPolicy::SafeReplay => EpochOperationAction::ReplaySameId,
            EpochRecoveryPolicy::VerifyExternalState => EpochOperationAction::VerifyExternalState,
            EpochRecoveryPolicy::ManualReview => EpochOperationAction::ManualReview,
            EpochRecoveryPolicy::DropIfDerived => EpochOperationAction::DropDerived,
        },
    }
}

/// Durable phase of one client authority transition.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum EpochTransitionPhase {
    #[default]
    Normal,
    FreezeAuthoritativeReconciliation,
    ClassifyPendingOperations,
    AcquireNewScopeManifest,
    BootstrapNewEpoch,
    RebaseSafeUnsentIntent,
    ResolveAmbiguousOldEpochOperations,
}

/// Client trust state that prevents epoch rollback and silent authority replacement.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientAuthorityState {
    trusted_authority: Option<AuthorityId>,
    highest_epochs: BTreeMap<AuthorityId, AuthorityEpoch>,
    phase: EpochTransitionPhase,
}

impl ClientAuthorityState {
    /// Creates empty trust-on-first-use state. Production hosts may preload a trusted ID.
    #[must_use]
    pub fn new(trusted_authority: Option<AuthorityId>) -> Self {
        Self {
            trusted_authority,
            highest_epochs: BTreeMap::new(),
            phase: EpochTransitionPhase::Normal,
        }
    }

    /// Returns the current transition phase.
    #[must_use]
    pub const fn phase(&self) -> EpochTransitionPhase {
        self.phase
    }

    /// Highest epoch durably trusted for an authority.
    #[must_use]
    pub fn highest_epoch(&self, authority: AuthorityId) -> Option<AuthorityEpoch> {
        self.highest_epochs.get(&authority).copied()
    }

    /// Observes a server descriptor, rejecting rollback or authority-ID changes.
    ///
    /// # Errors
    ///
    /// Returns an identity-change or epoch-rollback error.
    pub fn observe(
        &mut self,
        descriptor: AuthorityDescriptor,
    ) -> Result<CursorDisposition, AuthorityError> {
        if let Some(trusted) = self.trusted_authority {
            if trusted != descriptor.authority_id {
                return Err(AuthorityError::AuthorityIdChanged {
                    trusted,
                    presented: descriptor.authority_id,
                });
            }
        } else {
            self.trusted_authority = Some(descriptor.authority_id);
        }
        let previous = self.highest_epochs.get(&descriptor.authority_id).copied();
        if previous.is_some_and(|highest| descriptor.epoch < highest) {
            return Err(AuthorityError::AuthorityRollbackDetected {
                presented: descriptor.epoch,
                highest: previous.unwrap_or(descriptor.epoch),
            });
        }
        self.highest_epochs
            .insert(descriptor.authority_id, descriptor.epoch);
        if let Some(old) = previous.filter(|old| *old < descriptor.epoch) {
            self.phase = EpochTransitionPhase::FreezeAuthoritativeReconciliation;
            Ok(CursorDisposition::AuthorityChanged {
                previous: old,
                current: descriptor.epoch,
            })
        } else {
            Ok(CursorDisposition::Continue)
        }
    }

    /// Advances exactly one durable transition phase.
    ///
    /// # Errors
    ///
    /// Returns [`AuthorityError::NoEpochTransition`] when the client is in normal operation.
    pub fn advance_transition(&mut self) -> Result<EpochTransitionPhase, AuthorityError> {
        self.phase = match self.phase {
            EpochTransitionPhase::Normal => return Err(AuthorityError::NoEpochTransition),
            EpochTransitionPhase::FreezeAuthoritativeReconciliation => {
                EpochTransitionPhase::ClassifyPendingOperations
            }
            EpochTransitionPhase::ClassifyPendingOperations => {
                EpochTransitionPhase::AcquireNewScopeManifest
            }
            EpochTransitionPhase::AcquireNewScopeManifest => {
                EpochTransitionPhase::BootstrapNewEpoch
            }
            EpochTransitionPhase::BootstrapNewEpoch => EpochTransitionPhase::RebaseSafeUnsentIntent,
            EpochTransitionPhase::RebaseSafeUnsentIntent => {
                EpochTransitionPhase::ResolveAmbiguousOldEpochOperations
            }
            EpochTransitionPhase::ResolveAmbiguousOldEpochOperations => {
                EpochTransitionPhase::Normal
            }
        };
        Ok(self.phase)
    }

    /// Explicit trust reset used only by migration/testing administration.
    pub fn trust_reset(&mut self, authority: AuthorityId, epoch: AuthorityEpoch) {
        self.trusted_authority = Some(authority);
        self.highest_epochs.clear();
        self.highest_epochs.insert(authority, epoch);
        self.phase = EpochTransitionPhase::FreezeAuthoritativeReconciliation;
    }
}

/// Final resolution of one ambiguous operation in the replacement timeline.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EpochOperationResolution {
    Replayed,
    ConfirmedExternally,
    Compensated,
    DroppedDerived,
    ManualResolved,
}

/// Auditable cross-epoch resolution record.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationRecoveryRecord {
    pub operation_id: OperationId,
    pub old_epoch: AuthorityEpoch,
    pub new_epoch: AuthorityEpoch,
    pub resolution: EpochOperationResolution,
}

/// Authority-bound artifact classes that must never be interpreted across timelines implicitly.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthorityArtifactKind {
    Snapshot,
    IntegrityManifest,
    AuditCheckpoint,
    GovernanceCheckpoint,
    Backup,
}

/// Payload-free timeline binding shared by snapshot, integrity, audit, governance, and backup
/// manifests without coupling those crates to authority control-plane implementation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityArtifactBinding {
    pub kind: AuthorityArtifactKind,
    pub authority_id: AuthorityId,
    pub epoch: AuthorityEpoch,
    pub sequence: Option<Sequence>,
    pub digest: [u8; 32],
}

impl AuthorityArtifactBinding {
    /// Rejects interpreting this artifact under a different authority timeline.
    ///
    /// # Errors
    ///
    /// Returns an authority identity or epoch mismatch error.
    pub fn verify_timeline(
        self,
        authority_id: AuthorityId,
        epoch: AuthorityEpoch,
    ) -> Result<(), AuthorityError> {
        if self.authority_id != authority_id {
            return Err(AuthorityError::AuthorityIdChanged {
                trusted: self.authority_id,
                presented: authority_id,
            });
        }
        if self.epoch != epoch {
            return Err(AuthorityError::ArtifactEpochMismatch {
                artifact: self.epoch,
                current: epoch,
            });
        }
        Ok(())
    }
}

/// Backup metadata needed to assess a restore candidate without opening it for traffic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BackupAuthorityMetadata {
    pub authority_id: AuthorityId,
    pub epoch: AuthorityEpoch,
    pub highest_journal_sequence: Sequence,
    pub audit_checkpoint: [u8; 32],
    pub governance_checkpoint: [u8; 32],
    pub created_at_unix_ms: u64,
}

/// External monotonic epoch anchor protecting against restored database metadata rollback.
pub trait ExternalEpochRegistry: Send + Sync {
    fn highest_epoch(&self, authority_id: AuthorityId) -> Option<AuthorityEpoch>;
    /// Records a monotonic epoch for one authority.
    ///
    /// # Errors
    ///
    /// Returns an epoch-rollback or registry-state error.
    fn record_epoch(
        &self,
        authority_id: AuthorityId,
        epoch: AuthorityEpoch,
    ) -> Result<(), AuthorityError>;
}

/// Process-local reference registry for tests and embedded control planes.
#[derive(Clone, Default)]
pub struct InMemoryEpochRegistry {
    epochs: Arc<RwLock<BTreeMap<AuthorityId, AuthorityEpoch>>>,
}

impl ExternalEpochRegistry for InMemoryEpochRegistry {
    fn highest_epoch(&self, authority_id: AuthorityId) -> Option<AuthorityEpoch> {
        self.epochs
            .read()
            .ok()
            .and_then(|epochs| epochs.get(&authority_id).copied())
    }

    fn record_epoch(
        &self,
        authority_id: AuthorityId,
        epoch: AuthorityEpoch,
    ) -> Result<(), AuthorityError> {
        let mut epochs = self
            .epochs
            .write()
            .map_err(|_| AuthorityError::ControllerPoisoned)?;
        if let Some(highest) = epochs.get(&authority_id).copied() {
            if epoch < highest {
                return Err(AuthorityError::AuthorityRollbackDetected {
                    presented: epoch,
                    highest,
                });
            }
        }
        epochs.insert(authority_id, epoch);
        Ok(())
    }
}

/// Administrative permissions intentionally separated by risk.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum AuthorityPermission {
    View,
    Promote,
    ForcePromote,
    Demote,
    Restore,
    Verify,
}

/// Fail-closed authority state-machine error.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum AuthorityError {
    #[error("authority fence token cannot be zero")]
    ZeroFenceToken,
    #[error("authority fence token exhausted")]
    FenceTokenExhausted,
    #[error("authority epoch exhausted")]
    EpochExhausted,
    #[error("authority controller state is unavailable")]
    ControllerPoisoned,
    #[error("authority role {0:?} cannot accept writes")]
    NotPrimary(AuthorityRole),
    #[error("authority runtime mode {0:?} blocks writes")]
    WritesBlocked(AuthorityRuntimeMode),
    #[error("the authoritative commit presented a stale fence")]
    StaleFence,
    #[error("promotion requires independently confirmed old-primary fencing")]
    ExternalFenceRequired,
    #[error("lossless promotion continuity was not proven")]
    LosslessContinuityUnproven,
    #[error("potential data loss requires explicit acknowledgement")]
    DataLossApprovalRequired,
    #[error("promotion policy requires manual approval")]
    ManualApprovalRequired,
    #[error("recovery verification is incomplete")]
    RecoveryVerificationIncomplete,
    #[error("authority rollback detected: presented {presented:?}, highest {highest:?}")]
    AuthorityRollbackDetected {
        presented: AuthorityEpoch,
        highest: AuthorityEpoch,
    },
    #[error("authority identity changed from {trusted} to {presented}")]
    AuthorityIdChanged {
        trusted: AuthorityId,
        presented: AuthorityId,
    },
    #[error("external epoch {external:?} regressed behind local epoch {local:?}")]
    ExternalEpochRegression {
        external: AuthorityEpoch,
        local: AuthorityEpoch,
    },
    #[error("there is no active authority epoch transition")]
    NoEpochTransition,
    #[error("lossless transition unexpectedly changed authority epoch")]
    UnexpectedEpochChange,
    #[error("timeline-changing transition unsafely reused or lowered authority epoch")]
    UnsafeEpochReuse,
    #[error("artifact epoch {artifact:?} does not match current authority epoch {current:?}")]
    ArtifactEpochMismatch {
        artifact: AuthorityEpoch,
        current: AuthorityEpoch,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn id(value: u128) -> AuthorityId {
        AuthorityId::from_uuid(Uuid::from_u128(value))
    }

    fn instance(value: u128) -> AuthorityInstanceId {
        AuthorityInstanceId::from_uuid(Uuid::from_u128(value))
    }

    fn verified() -> RecoveryVerification {
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

    fn evidence(lag: Option<u64>) -> PromotionEvidence {
        PromotionEvidence {
            old_primary_externally_fenced: true,
            replication_lag: lag,
            journal_continuity: lag == Some(0),
            operation_ledger_continuity: lag == Some(0),
            audit_continuity: lag == Some(0),
            authority_metadata_continuity: lag == Some(0),
            scope_metadata_continuity: lag == Some(0),
            snapshot_catalog_consistent: lag == Some(0),
            governance_reconciled: true,
            side_effect_status_known: true,
        }
    }

    #[test]
    fn lossless_promotion_retains_epoch_but_rotates_fence() {
        let controller = AuthorityController::new(
            AuthorityState::new(id(1), instance(1), AuthorityRole::Standby, 1),
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
                    created_at_unix_ms: 2,
                },
                Some(Sequence(12)),
                Sequence(12),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(outcome.state.epoch, AuthorityEpoch::INITIAL);
        assert_eq!(outcome.state.fence_token.get(), 2);
        assert_eq!(
            controller.authorize_write(),
            Err(AuthorityError::WritesBlocked(
                AuthorityRuntimeMode::ReadOnlyVerification
            ))
        );
        controller
            .complete_recovery(verified(), 3)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(controller.authorize_write().is_ok());
    }

    #[test]
    fn data_loss_promotion_requires_approval_and_opens_new_epoch() {
        let controller = AuthorityController::new(
            AuthorityState::new(id(1), instance(1), AuthorityRole::Standby, 1),
            AuthorityPromotionPolicy::default(),
        );
        let mut request = PromotionRequest {
            class: PromotionClass::PotentialDataLoss,
            reason: TransitionReason::RegionFailover,
            evidence: evidence(Some(4)),
            allow_data_loss: false,
            manually_approved: false,
            new_instance_id: instance(2),
            created_at_unix_ms: 2,
        };
        assert_eq!(
            controller.promote(request, Some(Sequence(8)), Sequence(4)),
            Err(AuthorityError::DataLossApprovalRequired)
        );
        request.allow_data_loss = true;
        request.manually_approved = true;
        let outcome = controller
            .promote(request, Some(Sequence(8)), Sequence(4))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(outcome.state.epoch.get(), 2);
    }

    #[test]
    fn same_position_root_mismatch_quarantines_writes() {
        let mut state = AuthorityState::new(id(1), instance(1), AuthorityRole::Primary, 1);
        state.runtime_mode = AuthorityRuntimeMode::Serving;
        let controller = AuthorityController::new(state, AuthorityPromotionPolicy::default());
        let local = JournalCheckpoint {
            authority_id: id(1),
            epoch: AuthorityEpoch::INITIAL,
            sequence: Sequence(3),
            journal_root: [1; 32],
        };
        let peer = JournalCheckpoint {
            journal_root: [2; 32],
            ..local
        };
        assert_eq!(
            controller.observe_checkpoint(local, peer, 2),
            Ok(CheckpointComparison::ForkDetected)
        );
        assert_eq!(
            controller.authorize_write(),
            Err(AuthorityError::WritesBlocked(
                AuthorityRuntimeMode::Quarantined
            ))
        );
    }

    #[test]
    fn client_rejects_epoch_rollback_and_different_authority() {
        let mut client = ClientAuthorityState::new(Some(id(1)));
        let descriptor = AuthorityDescriptor {
            authority_id: id(1),
            epoch: AuthorityEpoch::new(2).unwrap_or_else(|error| panic!("{error}")),
            role: AuthorityRole::Primary,
            instance_id: instance(1),
        };
        assert_eq!(client.observe(descriptor), Ok(CursorDisposition::Continue));
        assert!(matches!(
            client.observe(AuthorityDescriptor {
                epoch: AuthorityEpoch::INITIAL,
                ..descriptor
            }),
            Err(AuthorityError::AuthorityRollbackDetected { .. })
        ));
        assert!(matches!(
            client.observe(AuthorityDescriptor {
                authority_id: id(2),
                ..descriptor
            }),
            Err(AuthorityError::AuthorityIdChanged { .. })
        ));
    }

    #[test]
    fn ambiguous_financial_work_never_blindly_replays() {
        assert_eq!(
            classify_epoch_operation(
                EpochOperationState::PossiblyCommittedOldEpoch,
                EpochRecoveryPolicy::VerifyExternalState
            ),
            EpochOperationAction::VerifyExternalState
        );
        assert_eq!(
            classify_epoch_operation(
                EpochOperationState::PossiblyCommittedOldEpoch,
                EpochRecoveryPolicy::ManualReview
            ),
            EpochOperationAction::ManualReview
        );
    }
}
