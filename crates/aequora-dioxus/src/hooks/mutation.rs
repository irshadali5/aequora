use crate::{Invalidation, MutationState, QueryKey, UiError, hooks::use_aequora};
use aequora_operation::MutationReceipt;
use dioxus::prelude::{Callback, ReadSignal, Signal, WritableExt, spawn, use_callback, use_signal};
use std::{future::Future, marker::PhantomData};

const MAX_TARGETED_INVALIDATIONS: usize = 64;

/// Cancellation-safe submission handle. Dropping the component only drops presentation updates;
/// a mutation already committed by the client remains durable and continues synchronizing.
pub struct MutationHandle<I: 'static> {
    state: Signal<MutationState>,
    submit: Callback<I>,
    marker: PhantomData<I>,
}

impl<I> Clone for MutationHandle<I> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<I> Copy for MutationHandle<I> {}

impl<I> MutationHandle<I> {
    #[must_use]
    pub fn state(&self) -> ReadSignal<MutationState> {
        self.state.into()
    }

    pub fn submit(&self, input: I) {
        self.submit.call(input);
    }

    pub fn reset(&mut self) {
        self.state.set(MutationState::Idle);
    }
}

/// Creates an explicit local-first mutation hook. The closure must return only after Tx A commits.
/// Matching query keys are invalidated after that receipt is returned, never before.
#[must_use]
pub fn use_aequora_mutation<I, E, F, Fut>(
    invalidates: Vec<QueryKey>,
    mutation: F,
) -> MutationHandle<I>
where
    I: 'static,
    E: Into<UiError> + 'static,
    F: Fn(I) -> Fut + Clone + 'static,
    Fut: Future<Output = Result<MutationReceipt, E>> + 'static,
{
    let handle = use_aequora();
    let invalidates = if invalidates.len() > MAX_TARGETED_INVALIDATIONS {
        None
    } else {
        Some(invalidates)
    };
    let mut state = use_signal(|| MutationState::Idle);
    let submit = use_callback(move |input: I| {
        let future = mutation(input);
        let handle = handle.clone();
        let invalidates = invalidates.clone();
        state.set(MutationState::SubmittingLocal);
        spawn(async move {
            match future.await {
                Ok(receipt) => {
                    state.set(MutationState::SavedLocally(receipt));
                    if let Some(invalidates) = invalidates {
                        for key in invalidates {
                            handle.invalidate(Invalidation::Key(key));
                        }
                    } else {
                        handle.invalidate(Invalidation::All);
                    }
                    let _accepted = handle
                        .client()
                        .request_sync(aequora_client::SyncReason::UserInitiated);
                }
                Err(error) => state.set(MutationState::Error(error.into())),
            }
        });
    });
    MutationHandle {
        state,
        submit,
        marker: PhantomData,
    }
}
