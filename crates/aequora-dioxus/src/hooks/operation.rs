use crate::{QueryHandle, QueryKey, use_aequora, use_aequora_query};
use aequora_adapter_sdk::OperationSnapshot;
use aequora_types::OperationId;

/// Rereads the durable state of one submitted operation after local or authoritative changes.
#[must_use]
pub fn use_operation(operation_id: OperationId) -> QueryHandle<Option<OperationSnapshot>> {
    let handle = use_aequora();
    use_aequora_query(
        QueryKey::new(format!("aequora:operation:{operation_id}")),
        move || {
            let client = handle.client();
            async move {
                client
                    .operations()
                    .get(operation_id)
                    .await
                    .map(|operation| operation.map(|operation| operation.snapshot()))
            }
        },
    )
}
