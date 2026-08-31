use aequora_blob_store::BlobMetadata;
use aequora_invariants::InvariantId;
use aequora_storage_backup::{RestoreCheck, RestoreContext, RestoreDecision, classify_restore};
use aequora_storage_conformance::{CertificationError, CertificationProfile, CertificationRecord};
use aequora_storage_core::{
    DeviceBinding, LocalStorageCapabilities, Platform, PublicationState, STORAGE_INVARIANT_IDS,
    StorageClass,
};
use aequora_storage_profile::{StorageProfile, may_evict};
use std::collections::BTreeMap;

#[test]
fn storage_invariants_are_registered_exactly_once() {
    for id in STORAGE_INVARIANT_IDS {
        assert_eq!(id.parse::<InvariantId>().map(InvariantId::as_str), Ok(id));
    }
    assert_eq!(
        InvariantId::ALL
            .iter()
            .filter(|id| id.as_str().starts_with("AEQ-INV-STORAGE"))
            .count(),
        10
    );
}

#[test]
fn cache_purge_cannot_remove_pending_intent() {
    assert!(!may_evict(StorageClass::CriticalIntent, false, false));
    assert!(!may_evict(StorageClass::DerivedState, false, true));
}

#[test]
fn new_device_restore_requires_rebinding() {
    let binding = DeviceBinding {
        store_id: "store".into(),
        device_id: "old-device".into(),
        binding_generation: 7,
        secure_key_fingerprint: "old-key".into(),
    };
    let context = RestoreContext {
        protocol: RestoreCheck::Accepted,
        operation_schema: RestoreCheck::Accepted,
        journal: RestoreCheck::Accepted,
        authority_epoch: RestoreCheck::Accepted,
        device_trust: RestoreCheck::Accepted,
    };
    assert_eq!(
        classify_restore(&binding, None, context, &[]),
        RestoreDecision::RebindAndRebase
    );
}

#[test]
fn low_disk_is_rejected_before_mutation() {
    assert_ne!(
        StorageProfile::MOBILE_STANDARD.admit(1, 1_000_000, 10_000, 10_000),
        aequora_storage_core::StorageAdmission::Allowed
    );
}

#[test]
fn corrupt_staging_blob_never_publishes() {
    let mut blob = BlobMetadata {
        digest: [0; 32],
        size: 3,
        pinned: false,
        reconstructable: true,
        publication: PublicationState::Staging,
    };
    assert!(blob.verify(b"bad").is_err());
    assert!(blob.publish().is_err());
}

#[test]
fn platform_claim_without_actual_observations_fails() {
    let record = CertificationRecord {
        adapter: "adapter".into(),
        adapter_version: "1".into(),
        platform: Platform::Linux,
        target: "x86_64-unknown-linux-gnu".into(),
        profile: CertificationProfile::DesktopLocalStoreFull,
        capabilities: LocalStorageCapabilities::REQUIRED,
        observations: BTreeMap::new(),
    };
    assert!(matches!(
        record.validate(),
        Err(CertificationError::RequiredTestFailed(_))
    ));
}
