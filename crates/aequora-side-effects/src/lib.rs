//! Typed transactional-outbox and external side-effect contracts.
//!
//! Domain decisions commit immutable [`SideEffectIntent`] records in their authoritative
//! transaction. Provider execution is at-least-once and any business-state change is represented
//! as a new authoritative operation rather than a direct worker mutation.

#![allow(clippy::missing_errors_doc)]

use aequora_jobs::WorkflowId;
use aequora_types::{CorrelationId, JobId, OperationId, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const MAX_SIDE_EFFECT_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_PROVIDER_REFERENCE_BYTES: usize = 1_024;
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SideEffectIntentId(Uuid);
impl SideEffectIntentId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
    #[must_use]
    pub fn derive(operation_id: OperationId, semantic_purpose: &[u8]) -> Self {
        let mut bytes = [0_u8; 16];
        let mut hasher = blake3::Hasher::new();
        hasher.update(operation_id.as_uuid().as_bytes());
        hasher.update(&[0]);
        hasher.update(semantic_purpose);
        bytes.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
        Self(Uuid::from_bytes(bytes))
    }
}
impl Default for SideEffectIntentId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SideEffectKind(u32);
impl SideEffectKind {
    pub const fn new(value: u32) -> Result<Self, SideEffectError> {
        if value == 0 {
            Err(SideEffectError::ZeroIdentity)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ExternalIdempotencyKey(String);
impl ExternalIdempotencyKey {
    pub fn new(value: impl Into<String>) -> Result<Self, SideEffectError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_IDEMPOTENCY_KEY_BYTES {
            Err(SideEffectError::InvalidIdempotencyKey)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
    #[must_use]
    pub fn from_intent(intent_id: SideEffectIntentId) -> Self {
        Self(intent_id.as_uuid().to_string())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SideEffectPayload {
    pub schema_version: u16,
    pub bytes: Vec<u8>,
    pub secret_references: Vec<String>,
}
impl SideEffectPayload {
    pub fn verify(&self) -> Result<(), SideEffectError> {
        if self.schema_version == 0 || self.bytes.len() > MAX_SIDE_EFFECT_PAYLOAD_BYTES {
            return Err(SideEffectError::InvalidPayload);
        }
        if self
            .secret_references
            .iter()
            .any(|reference| reference.is_empty() || reference.len() > MAX_PROVIDER_REFERENCE_BYTES)
        {
            return Err(SideEffectError::InvalidSecretReference);
        }
        Ok(())
    }
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.schema_version.to_be_bytes());
        hasher.update(&self.bytes);
        for reference in &self.secret_references {
            hasher.update(&[0]);
            hasher.update(reference.as_bytes());
        }
        *hasher.finalize().as_bytes()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AmbiguousRecoveryPolicy {
    QueryProvider,
    RetryWithIdempotencyKey,
    ManualReview,
    SuppressDuplicateRisk,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SideEffectIntent {
    pub intent_id: SideEffectIntentId,
    pub operation_id: OperationId,
    pub tenant_id: TenantId,
    pub kind: SideEffectKind,
    pub idempotency_key: ExternalIdempotencyKey,
    pub payload: SideEffectPayload,
    pub payload_digest: [u8; 32],
    pub ambiguity_policy: AmbiguousRecoveryPolicy,
    pub correlation_id: CorrelationId,
    pub workflow_id: Option<WorkflowId>,
    pub created_at_unix_ms: u64,
    pub retention_class: u16,
    pub contains_personal_data: bool,
}
impl SideEffectIntent {
    pub fn verify(&self) -> Result<(), SideEffectError> {
        self.payload.verify()?;
        if self.payload.digest() != self.payload_digest {
            return Err(SideEffectError::PayloadDigestMismatch);
        }
        if self.retention_class == 0 {
            return Err(SideEffectError::InvalidRetentionClass);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SideEffectExecutionState {
    Pending,
    Executing,
    Confirmed,
    Retryable,
    Ambiguous,
    Failed,
    ManualReview,
}
impl SideEffectExecutionState {
    #[must_use]
    pub fn can_transition_to(self, next: Self) -> bool {
        use SideEffectExecutionState::{
            Ambiguous, Confirmed, Executing, Failed, ManualReview, Pending, Retryable,
        };
        matches!(
            (self, next),
            (Pending | Retryable, Executing)
                | (Executing, Confirmed | Retryable | Ambiguous | Failed)
                | (Ambiguous, Executing | Confirmed | Failed | ManualReview)
                | (ManualReview, Executing | Confirmed | Failed)
        ) || self == next
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExternalOutcomeKind {
    ConfirmedSuccess,
    ConfirmedFailure,
    RetryableFailure,
    Ambiguous,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExternalOutcome {
    pub kind: ExternalOutcomeKind,
    pub provider_reference: Option<String>,
    pub retry_after_ms: Option<u64>,
    pub result_code: u32,
}
impl ExternalOutcome {
    pub fn verify(&self) -> Result<(), SideEffectError> {
        if self
            .provider_reference
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > MAX_PROVIDER_REFERENCE_BYTES)
        {
            return Err(SideEffectError::InvalidProviderReference);
        }
        if self.kind != ExternalOutcomeKind::RetryableFailure && self.retry_after_ms.is_some() {
            return Err(SideEffectError::UnexpectedRetryAfter);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SideEffectExecution {
    pub intent_id: SideEffectIntentId,
    pub job_id: JobId,
    pub provider: String,
    pub state: SideEffectExecutionState,
    pub provider_reference: Option<String>,
    pub last_outcome: Option<ExternalOutcome>,
    pub updated_at_unix_ms: u64,
}
impl SideEffectExecution {
    pub fn verify(&self) -> Result<(), SideEffectError> {
        if self.provider.is_empty() || self.provider.len() > 256 {
            return Err(SideEffectError::InvalidProvider);
        }
        if let Some(outcome) = &self.last_outcome {
            outcome.verify()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCapabilities {
    pub supports_idempotency: bool,
    pub supports_lookup: bool,
    pub supports_cancel: bool,
}
impl ProviderCapabilities {
    pub fn certify_high_risk(self) -> Result<(), SideEffectError> {
        if self.supports_idempotency || self.supports_lookup {
            Ok(())
        } else {
            Err(SideEffectError::UnsafeHighRiskProvider)
        }
    }

    pub fn supports_policy(self, policy: AmbiguousRecoveryPolicy) -> Result<(), SideEffectError> {
        match policy {
            AmbiguousRecoveryPolicy::QueryProvider if !self.supports_lookup => {
                Err(SideEffectError::UnsupportedRecoveryPolicy)
            }
            AmbiguousRecoveryPolicy::RetryWithIdempotencyKey if !self.supports_idempotency => {
                Err(SideEffectError::UnsupportedRecoveryPolicy)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconciliationResult {
    Confirmed(ExternalOutcome),
    NotFound,
    StillAmbiguous,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProviderError {
    #[error("provider is temporarily unavailable")]
    Unavailable,
    #[error("provider rate limited the request")]
    RateLimited { retry_after_ms: Option<u64> },
    #[error("provider rejected the request permanently with code {code}")]
    Permanent { code: u32 },
    #[error("provider result is ambiguous")]
    Ambiguous,
    #[error("provider integration rejected unsafe configuration")]
    UnsafeConfiguration,
}

#[async_trait]
pub trait SideEffectProvider: Send + Sync {
    fn capabilities(&self) -> ProviderCapabilities;
    async fn execute(
        &self,
        intent: &SideEffectIntent,
        timeout_ms: u64,
    ) -> Result<ExternalOutcome, ProviderError>;
    async fn reconcile(
        &self,
        key: &ExternalIdempotencyKey,
        provider_reference: Option<&str>,
        timeout_ms: u64,
    ) -> Result<ReconciliationResult, ProviderError>;
}

/// Command submitted through the normal authoritative operation pipeline after provider output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeResultOperation {
    pub tenant_id: TenantId,
    pub caused_by_operation_id: OperationId,
    pub intent_id: SideEffectIntentId,
    pub correlation_id: CorrelationId,
    pub operation_kind: u32,
    pub payload: Vec<u8>,
}

#[async_trait]
pub trait AuthoritativeOperationSink: Send + Sync {
    async fn submit_result(
        &self,
        operation: AuthoritativeResultOperation,
    ) -> Result<(), SideEffectStoreError>;
}

/// Write set that must be committed with authoritative state, journal, ledger, and required audit.
#[async_trait]
pub trait SideEffectIntentWrite: Send {
    async fn write_intent(&mut self, intent: SideEffectIntent) -> Result<(), SideEffectStoreError>;
}

#[async_trait]
pub trait SideEffectStore: Send + Sync {
    async fn load_intent(
        &self,
        intent_id: SideEffectIntentId,
        tenant_id: TenantId,
    ) -> Result<SideEffectIntent, SideEffectStoreError>;
    async fn project_job_once(
        &self,
        intent_id: SideEffectIntentId,
        tenant_id: TenantId,
        job_id: JobId,
    ) -> Result<bool, SideEffectStoreError>;
    async fn compare_and_set_execution(
        &self,
        tenant_id: TenantId,
        expected: SideEffectExecutionState,
        execution: SideEffectExecution,
    ) -> Result<(), SideEffectStoreError>;
}

#[must_use]
pub const fn recovery_action(
    policy: AmbiguousRecoveryPolicy,
    capabilities: ProviderCapabilities,
) -> RecoveryAction {
    match policy {
        AmbiguousRecoveryPolicy::QueryProvider if capabilities.supports_lookup => {
            RecoveryAction::Reconcile
        }
        AmbiguousRecoveryPolicy::RetryWithIdempotencyKey if capabilities.supports_idempotency => {
            RecoveryAction::RetrySameKey
        }
        AmbiguousRecoveryPolicy::SuppressDuplicateRisk => RecoveryAction::StopWithoutRetry,
        _ => RecoveryAction::ManualReview,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    Reconcile,
    RetrySameKey,
    ManualReview,
    StopWithoutRetry,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SideEffectStoreError {
    #[error("side-effect intent already exists with different semantics")]
    IntentConflict,
    #[error("side-effect intent was not found in this tenant")]
    NotFound,
    #[error("side-effect execution compare-and-swap failed")]
    StateConflict,
    #[error("side-effect storage is temporarily unavailable")]
    Unavailable,
    #[error("side-effect storage rejected invalid data: {0}")]
    Invalid(SideEffectError),
    #[error("side-effect storage adapter failure: {0}")]
    Adapter(String),
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SideEffectError {
    #[error("persistent numeric identity must be non-zero")]
    ZeroIdentity,
    #[error("external idempotency key is empty or too large")]
    InvalidIdempotencyKey,
    #[error("side-effect payload is invalid or exceeds its hard bound")]
    InvalidPayload,
    #[error("side-effect payload contains an invalid secret reference")]
    InvalidSecretReference,
    #[error("side-effect payload digest does not match payload")]
    PayloadDigestMismatch,
    #[error("provider reference is invalid")]
    InvalidProviderReference,
    #[error("retry-after is only valid for a retryable outcome")]
    UnexpectedRetryAfter,
    #[error("provider identifier is invalid")]
    InvalidProvider,
    #[error("high-risk provider lacks idempotency and lookup")]
    UnsafeHighRiskProvider,
    #[error("provider cannot support the configured ambiguity recovery policy")]
    UnsupportedRecoveryPolicy,
    #[error("side-effect retention class must be non-zero")]
    InvalidRetentionClass,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_intent_identity_is_semantic() {
        let operation = OperationId::new();
        assert_eq!(
            SideEffectIntentId::derive(operation, b"capture-payment"),
            SideEffectIntentId::derive(operation, b"capture-payment")
        );
        assert_ne!(
            SideEffectIntentId::derive(operation, b"capture-payment"),
            SideEffectIntentId::derive(operation, b"send-receipt")
        );
    }

    #[test]
    fn ambiguity_never_becomes_a_blind_retry() {
        let no_capabilities = ProviderCapabilities {
            supports_idempotency: false,
            supports_lookup: false,
            supports_cancel: false,
        };
        assert_eq!(
            recovery_action(
                AmbiguousRecoveryPolicy::RetryWithIdempotencyKey,
                no_capabilities
            ),
            RecoveryAction::ManualReview
        );
    }

    #[test]
    fn high_risk_provider_requires_idempotency_or_lookup() {
        let capabilities = ProviderCapabilities {
            supports_idempotency: false,
            supports_lookup: false,
            supports_cancel: true,
        };
        assert_eq!(
            capabilities.certify_high_risk(),
            Err(SideEffectError::UnsafeHighRiskProvider)
        );
    }
}
