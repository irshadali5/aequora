//! Idempotent operation ledger capability.

use crate::{AdapterError, Digest, LedgerRecord};
use aequora_types::OperationId;
use async_trait::async_trait;

/// Operation ledger semantics, including canonical payload-reuse rejection.
#[async_trait]
pub trait OperationLedgerStore: Send + Sync {
    /// Looks up the terminal outcome for an operation.
    async fn lookup(
        &self,
        operation_id: OperationId,
    ) -> Result<Option<LedgerRecord>, AdapterError>;

    /// Inserts one terminal outcome, rejecting identifier reuse with a different digest.
    async fn insert_outcome(&self, record: LedgerRecord) -> Result<(), AdapterError>;

    /// Verifies that an existing operation identity is bound to `payload_digest`.
    async fn verify_payload_digest(
        &self,
        operation_id: OperationId,
        payload_digest: Digest,
    ) -> Result<(), AdapterError>;
}
