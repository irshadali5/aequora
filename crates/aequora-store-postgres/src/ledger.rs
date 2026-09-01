//! Operation-ledger helpers.

/// Computes the canonical payload binding stored with an operation outcome.
#[must_use]
pub fn payload_digest(payload: &[u8]) -> [u8; 32] {
    *blake3::hash(payload).as_bytes()
}

/// Constant-time-independent exact digest comparison used before duplicate replay.
#[must_use]
pub fn same_payload(left: &[u8], right: &[u8; 32]) -> bool {
    left == right
}
