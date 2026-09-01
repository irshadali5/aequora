use crate::{QueryHandle, QueryKey, use_aequora, use_aequora_query};
use aequora_adapter_sdk::ConflictSummary;

/// Queries the client's bounded durable unresolved-conflict page.
#[must_use]
pub fn use_conflicts() -> QueryHandle<Vec<ConflictSummary>> {
    let handle = use_aequora();
    use_aequora_query(QueryKey::new("aequora:conflicts"), move || {
        let client = handle.client();
        async move { client.conflicts().list().await }
    })
}
