use aequora_adapter_sdk::{
    AdapterError,
    conformance::{
        AuthorityConformanceCase, AuthorityConformanceStore, AuthorityStoreFactory,
        FencingConformanceCase, FencingConformanceStore, FencingStoreFactory, LocalConformanceCase,
        LocalConformanceStore, LocalStoreFactory, ProbeResult, SnapshotConformanceCase,
        SnapshotConformanceStore, SnapshotStoreFactory, run_authority_store_conformance,
        run_fencing_conformance, run_local_store_conformance, run_snapshot_conformance,
    },
};
use aequora_conformance::TestStatus;
use async_trait::async_trait;

struct ReferenceFactory;
struct ReferenceProbe;

#[async_trait]
impl LocalStoreFactory for ReferenceFactory {
    type Store = ReferenceProbe;

    async fn create_clean(&self) -> Result<Self::Store, AdapterError> {
        Ok(ReferenceProbe)
    }
}

#[async_trait]
impl LocalConformanceStore for ReferenceProbe {
    async fn observe(&self, case: LocalConformanceCase) -> Result<ProbeResult, AdapterError> {
        Ok(ProbeResult::passed(format!("local:{case:?}")))
    }
}

#[async_trait]
impl AuthorityStoreFactory for ReferenceFactory {
    type Store = ReferenceProbe;

    async fn create_clean(&self) -> Result<Self::Store, AdapterError> {
        Ok(ReferenceProbe)
    }
}

#[async_trait]
impl AuthorityConformanceStore for ReferenceProbe {
    async fn observe(&self, case: AuthorityConformanceCase) -> Result<ProbeResult, AdapterError> {
        Ok(ProbeResult::passed(format!("authority:{case:?}")))
    }
}

#[async_trait]
impl SnapshotStoreFactory for ReferenceFactory {
    type Store = ReferenceProbe;

    async fn create_clean(&self) -> Result<Self::Store, AdapterError> {
        Ok(ReferenceProbe)
    }
}

#[async_trait]
impl SnapshotConformanceStore for ReferenceProbe {
    async fn observe(&self, case: SnapshotConformanceCase) -> Result<ProbeResult, AdapterError> {
        Ok(ProbeResult::passed(format!("snapshot:{case:?}")))
    }
}

#[async_trait]
impl FencingStoreFactory for ReferenceFactory {
    type Store = ReferenceProbe;

    async fn create_clean(&self) -> Result<Self::Store, AdapterError> {
        Ok(ReferenceProbe)
    }
}

#[async_trait]
impl FencingConformanceStore for ReferenceProbe {
    async fn observe(&self, case: FencingConformanceCase) -> Result<ProbeResult, AdapterError> {
        Ok(ProbeResult::passed(format!("fencing:{case:?}")))
    }
}

#[tokio::test]
async fn public_role_runners_emit_unique_passing_observations() {
    let local = run_local_store_conformance(&ReferenceFactory)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let authority = run_authority_store_conformance(&ReferenceFactory)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let snapshot = run_snapshot_conformance(&ReferenceFactory)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let fencing = run_fencing_conformance(&ReferenceFactory)
        .await
        .unwrap_or_else(|error| panic!("{error}"));

    for observations in [&local, &authority, &snapshot, &fencing] {
        assert!(
            observations
                .iter()
                .all(|value| value.status == TestStatus::Passed)
        );
        let unique = observations
            .iter()
            .map(|value| value.test_id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), observations.len());
        assert!(observations.iter().all(|value| !value.evidence.is_empty()));
    }
}
