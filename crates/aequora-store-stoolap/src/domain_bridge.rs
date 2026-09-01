//! Application-domain transaction bridge markers.

use aequora_types::OperationId;

/// Proof returned only after a shared domain/outbox transaction commits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalMutationReceipt {
    pub operation_id: OperationId,
}
