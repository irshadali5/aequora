use crate::{BootstrapState, hooks::use_aequora};
use dioxus::prelude::{ReadSignal, WritableExt, use_future, use_signal};

/// Watches bounded bootstrap progress hints. Durable bootstrap truth must be queried on startup.
#[must_use]
pub fn use_bootstrap_progress() -> ReadSignal<BootstrapState> {
    let handle = use_aequora();
    let receiver = handle.bootstrap_receiver();
    let mut state = use_signal(|| receiver.borrow().clone());
    use_future(move || {
        let mut receiver = receiver.clone();
        async move {
            while receiver.changed().await.is_ok() {
                state.set(receiver.borrow_and_update().clone());
            }
        }
    });
    state.into()
}
