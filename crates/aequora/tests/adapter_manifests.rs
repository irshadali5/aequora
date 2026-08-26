use aequora::{
    POSTGRES_ADAPTER_MANIFEST, STOOLAP_ADAPTER_MANIFEST,
    store::{AdapterRequirements, ProductionAdapterPair},
};

#[test]
fn built_in_adapters_form_a_verified_database_neutral_production_pair() {
    assert_eq!(
        AdapterRequirements::PRODUCTION_LOCAL.verify(STOOLAP_ADAPTER_MANIFEST),
        Ok(())
    );
    assert_eq!(
        AdapterRequirements::PRODUCTION_AUTHORITATIVE.verify(POSTGRES_ADAPTER_MANIFEST),
        Ok(())
    );
    assert_eq!(
        ProductionAdapterPair::verify(STOOLAP_ADAPTER_MANIFEST, POSTGRES_ADAPTER_MANIFEST),
        Ok(ProductionAdapterPair {
            local: STOOLAP_ADAPTER_MANIFEST,
            authoritative: POSTGRES_ADAPTER_MANIFEST,
        })
    );
}
