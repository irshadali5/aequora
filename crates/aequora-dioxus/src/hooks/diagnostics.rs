use crate::{QueryHandle, QueryKey, use_aequora, use_aequora_query};
use std::sync::Arc;

/// Queries the client's bounded, redacted durable diagnostics summary.
#[must_use]
pub fn use_diagnostics() -> QueryHandle<Arc<str>> {
    let handle = use_aequora();
    use_aequora_query(QueryKey::new("aequora:diagnostics"), move || {
        let client = handle.client();
        async move { client.diagnostics().summary().await }
    })
}
