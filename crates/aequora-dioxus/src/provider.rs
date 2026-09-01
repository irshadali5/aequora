use crate::{AequoraHandle, QueryNamespace};
use aequora_client::AequoraClient;
use dioxus::prelude::{provide_context, spawn, use_hook};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_NAMESPACE: AtomicU64 = AtomicU64::new(1);

/// Provides one shared client handle at the current Dioxus application/store boundary.
///
/// This is a hook and must be called unconditionally from the provider component. A unique cache
/// namespace is allocated for the mounted store so a replacement client cannot reuse old query
/// state accidentally.
#[must_use]
pub fn provide_aequora(client: AequoraClient) -> AequoraHandle {
    use_hook(|| {
        let id = NEXT_NAMESPACE.fetch_add(1, Ordering::Relaxed);
        install(client, QueryNamespace::new(format!("store-{id}")))
    })
}

/// Provides one shared client using an explicit stable store/account cache namespace.
#[must_use]
pub fn provide_aequora_with_namespace(
    client: AequoraClient,
    namespace: QueryNamespace,
) -> AequoraHandle {
    use_hook(|| install(client, namespace))
}

fn install(client: AequoraClient, namespace: QueryNamespace) -> AequoraHandle {
    let handle = AequoraHandle::new(client, namespace);
    let bridge = handle.clone();
    spawn(async move { bridge.bridge_events().await });
    provide_context(handle)
}
