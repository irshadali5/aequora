//! Bounded atomic outbox-claim contracts.

use aequora_protocol::OperationEnvelope;

/// One operation atomically transitioned to in-flight ownership.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimedOperation {
    pub operation: OperationEnvelope,
    pub payload_digest: [u8; 32],
}
