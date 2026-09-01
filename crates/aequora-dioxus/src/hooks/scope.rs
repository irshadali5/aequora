use crate::{Invalidation, ScopeState, UiError, hooks::use_aequora};
use aequora_types::SyncScopeId;
use dioxus::prelude::{ReadSignal, Signal, WritableExt, spawn, use_callback, use_signal};

/// UI actions for a durable scope subscription. Contraction is not domain deletion.
#[derive(Clone, Copy)]
pub struct ScopeMutationHandle {
    state: Signal<ScopeState>,
    subscribe: dioxus::prelude::Callback<()>,
    unsubscribe: dioxus::prelude::Callback<()>,
}

impl ScopeMutationHandle {
    #[must_use]
    pub fn state(&self) -> ReadSignal<ScopeState> {
        self.state.into()
    }

    pub fn subscribe(&self) {
        self.subscribe.call(());
    }

    pub fn unsubscribe(&self) {
        self.unsubscribe.call(());
    }
}

/// Creates durable scope expansion/contraction actions for one scope.
#[must_use]
pub fn use_aequora_scope(scope_id: SyncScopeId) -> ScopeMutationHandle {
    let handle = use_aequora();
    let mut state = use_signal(|| ScopeState::Idle);
    let subscribe_handle = handle.clone();
    let subscribe = use_callback(move |()| {
        let handle = subscribe_handle.clone();
        state.set(ScopeState::Expanding);
        spawn(async move {
            match handle.client().scopes().subscribe(scope_id).await {
                Ok(_) => {
                    state.set(ScopeState::Active);
                    handle.invalidate(Invalidation::Scope(scope_id));
                }
                Err(error) => state.set(ScopeState::Error(UiError::from(error))),
            }
        });
    });
    let unsubscribe_handle = handle;
    let unsubscribe = use_callback(move |()| {
        let handle = unsubscribe_handle.clone();
        state.set(ScopeState::Contracting);
        spawn(async move {
            match handle.client().scopes().unsubscribe(scope_id).await {
                Ok(()) => {
                    state.set(ScopeState::Idle);
                    handle.invalidate(Invalidation::Scope(scope_id));
                }
                Err(error) => state.set(ScopeState::Error(UiError::from(error))),
            }
        });
    });
    ScopeMutationHandle {
        state,
        subscribe,
        unsubscribe,
    }
}
