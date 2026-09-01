use crate::{Connectivity, hooks::use_aequora};
use dioxus::prelude::{ReadSignal, WritableExt, use_future, use_signal};

/// Watches advisory connectivity supplied by the platform runtime.
#[must_use]
pub fn use_connectivity() -> ReadSignal<Connectivity> {
    let handle = use_aequora();
    let receiver = handle.platform_receiver();
    let mut connectivity = use_signal(|| receiver.borrow().connectivity);
    use_future(move || {
        let mut receiver = receiver.clone();
        async move {
            while receiver.changed().await.is_ok() {
                connectivity.set(receiver.borrow_and_update().connectivity);
            }
        }
    });
    connectivity.into()
}
