use aequora_store::{AdapterRequirements, ProductionAdapterPair};
use aequora_store_postgres::POSTGRES_ADAPTER_MANIFEST;
use aequora_store_stoolap::STOOLAP_ADAPTER_MANIFEST;

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
