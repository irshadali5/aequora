//! Edge-only translation of legacy API contracts into typed Aequora operations.
//!
//! Legacy JSON, XML, authentication claims, roles, and error strings do not enter domain core.

#![allow(clippy::missing_errors_doc)]

use aequora_types::{ActorId, OperationId, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyPrincipal {
    pub issuer: String,
    pub subject: String,
    pub tenant_hint: String,
    pub role_claims: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyAuthContext {
    pub actor_id: ActorId,
    pub tenant_id: TenantId,
    pub granted_permissions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CompatibilityError {
    #[error("legacy identity is not explicitly mapped")]
    IdentityNotMapped,
    #[error("legacy role or permission is not explicitly mapped")]
    PermissionNotMapped,
    #[error("legacy request is invalid: {0}")]
    InvalidRequest(String),
    #[error("legacy business state is unsupported: {0}")]
    UnsupportedState(String),
    #[error("canonical operation failed: {0}")]
    Operation(String),
}

pub trait LegacyIdentityMapper: Send + Sync {
    fn map_identity(
        &self,
        principal: &LegacyPrincipal,
    ) -> Result<LegacyAuthContext, CompatibilityError>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyRequestContext {
    pub principal: LegacyPrincipal,
    pub request_id: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryGuarantee {
    StableOperationId,
    BestEffortOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DerivedOperationId {
    pub operation_id: OperationId,
    pub retry_guarantee: RetryGuarantee,
}

#[must_use]
pub fn derive_operation_id(
    system_namespace: &[u8],
    context: &LegacyRequestContext,
) -> DerivedOperationId {
    let stable_key = context
        .request_id
        .as_ref()
        .or(context.idempotency_key.as_ref());
    match stable_key {
        Some(key) => {
            let mut hasher = blake3::Hasher::new();
            hasher.update(system_namespace);
            hasher.update(context.principal.issuer.as_bytes());
            hasher.update(context.principal.subject.as_bytes());
            hasher.update(key.as_bytes());
            let digest = hasher.finalize();
            let mut bytes = [0_u8; 16];
            bytes.copy_from_slice(&digest.as_bytes()[..16]);
            DerivedOperationId {
                operation_id: OperationId::from_uuid(uuid::Uuid::from_bytes(bytes)),
                retry_guarantee: RetryGuarantee::StableOperationId,
            }
        }
        None => DerivedOperationId {
            operation_id: OperationId::new(),
            retry_guarantee: RetryGuarantee::BestEffortOnly,
        },
    }
}

/// Translation is typed: the request and operation are application-owned DTOs, not raw payloads.
pub trait LegacyRequestTranslator<Request, Operation>: Send + Sync {
    fn translate(
        &self,
        request: Request,
        auth: LegacyAuthContext,
        operation_id: OperationId,
    ) -> Result<Operation, CompatibilityError>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyErrorResponse {
    pub status: u16,
    pub code: String,
    pub message: String,
}

pub trait LegacyErrorMapper<DomainError>: Send + Sync {
    fn to_legacy_error(&self, error: &DomainError) -> LegacyErrorResponse;
}

#[async_trait]
pub trait CanonicalOperationHandler<Operation>: Send + Sync {
    type Output: Send;
    type Error: Send;
    async fn execute(&self, operation: Operation) -> Result<Self::Output, Self::Error>;
}

/// Facade pipeline which always routes translated requests through the canonical handler.
pub async fn execute_legacy_request<Request, Operation, Output, DomainError>(
    request: Request,
    context: &LegacyRequestContext,
    namespace: &[u8],
    identities: &impl LegacyIdentityMapper,
    translator: &impl LegacyRequestTranslator<Request, Operation>,
    handler: &impl CanonicalOperationHandler<Operation, Output = Output, Error = DomainError>,
) -> Result<(Output, DerivedOperationId), CompatibilityError>
where
    Request: Send,
    Operation: Send,
    Output: Send,
    DomainError: core::fmt::Display + Send,
{
    let auth = identities.map_identity(&context.principal)?;
    let derived = derive_operation_id(namespace, context);
    let operation = translator.translate(request, auth, derived.operation_id)?;
    let output = handler
        .execute(operation)
        .await
        .map_err(|error| CompatibilityError::Operation(error.to_string()))?;
    Ok((output, derived))
}
