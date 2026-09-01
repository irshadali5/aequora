use crate::{SyncStatus, hooks::use_aequora};
use dioxus::prelude::{ReadSignal, WritableExt, use_future, use_signal};

/// Subscribes to coarse latest-value status and initializes it from durable local state.
#[must_use]
pub fn use_sync_status() -> ReadSignal<SyncStatus> {
    let handle = use_aequora();
    let receiver = handle.status_receiver();
    let mut status = use_signal(|| *receiver.borrow());
    use_future(move || {
        let handle = handle.clone();
        let mut receiver = receiver.clone();
        async move {
            if let Ok(current) = handle.refresh_status().await {
                status.set(current);
            }
            while receiver.changed().await.is_ok() {
                status.set(*receiver.borrow_and_update());
            }
        }
    });
    status.into()
}
