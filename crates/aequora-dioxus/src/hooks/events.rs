use crate::hooks::use_aequora;
use aequora_client::SyncEvent;
use dioxus::prelude::{ReadSignal, WritableExt, use_future, use_signal};

/// Latest advisory high-level client event. Missing intermediate events is explicitly safe.
#[must_use]
pub fn use_aequora_events() -> ReadSignal<Option<SyncEvent>> {
    let handle = use_aequora();
    let receiver = handle.event_receiver();
    let mut event = use_signal(|| receiver.borrow().clone());
    use_future(move || {
        let mut receiver = receiver.clone();
        async move {
            while receiver.changed().await.is_ok() {
                event.set(receiver.borrow_and_update().clone());
            }
        }
    });
    event.into()
}
