//! Transactionally consistent backup manifests and restore decisions.
use aequora_storage_core::{BindingDisposition, DeviceBinding, LocalStoreFormatVersion};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalBackupManifest {
    pub store_id: String,
    pub device_id: String,
    pub device_binding_generation: u64,
    pub authority_epoch: u64,
    pub schema_version: LocalStoreFormatVersion,
    pub created_at_unix_ms: u64,
    pub digest: [u8; 32],
    pub encrypted: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PendingOperationState {
    NeverSent,
    PossiblySent,
    CommittedUnknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RestoreDecision {
    Resume,
    RebindAndRebase,
    Rebootstrap,
    ManualReview,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RestoreCheck {
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RestoreContext {
    pub protocol: RestoreCheck,
    pub operation_schema: RestoreCheck,
    pub journal: RestoreCheck,
    pub authority_epoch: RestoreCheck,
    pub device_trust: RestoreCheck,
}

#[must_use]
pub fn classify_restore(
    binding: &DeviceBinding,
    secure_fingerprint: Option<&str>,
    context: RestoreContext,
    pending: &[PendingOperationState],
) -> RestoreDecision {
    if context.device_trust == RestoreCheck::Rejected
        || context.operation_schema == RestoreCheck::Rejected
        || pending.contains(&PendingOperationState::CommittedUnknown)
    {
        return RestoreDecision::ManualReview;
    }
    if context.protocol == RestoreCheck::Rejected
        || context.journal == RestoreCheck::Rejected
        || context.authority_epoch == RestoreCheck::Rejected
    {
        return RestoreDecision::Rebootstrap;
    }
    if binding.classify(secure_fingerprint) == BindingDisposition::RebindRequired {
        RestoreDecision::RebindAndRebase
    } else {
        RestoreDecision::Resume
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clone_never_silently_resumes() {
        let b = DeviceBinding {
            store_id: "s".into(),
            device_id: "d".into(),
            binding_generation: 1,
            secure_key_fingerprint: "key".into(),
        };
        let c = RestoreContext {
            protocol: RestoreCheck::Accepted,
            operation_schema: RestoreCheck::Accepted,
            journal: RestoreCheck::Accepted,
            authority_epoch: RestoreCheck::Accepted,
            device_trust: RestoreCheck::Accepted,
        };
        assert_eq!(
            classify_restore(&b, Some("new"), c, &[]),
            RestoreDecision::RebindAndRebase
        );
    }
}
