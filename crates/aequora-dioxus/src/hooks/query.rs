use crate::{QueryError, QueryKey, QueryState, hooks::use_aequora};
use dioxus::prelude::{
    ReadableExt, Resource, UseResourceState, use_future, use_resource, use_signal,
};
use std::{future::Future, marker::PhantomData};

/// Reactive handle for one application-owned, bounded local query.
pub struct QueryHandle<T: 'static> {
    resource: Resource<Result<T, QueryError>>,
    marker: PhantomData<T>,
}

impl<T> Clone for QueryHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for QueryHandle<T> {}

impl<T: Clone> QueryHandle<T> {
    /// Reads the latest state. Cached data remains visible while a refresh is pending.
    #[must_use]
    pub fn state(&self) -> QueryState<T> {
        let pending = *self.resource.state().read() == UseResourceState::Pending;
        match &*self.resource.read() {
            Some(Ok(value)) if pending => QueryState::Refreshing(value.clone()),
            Some(Ok(value)) => QueryState::Ready(value.clone()),
            Some(Err(error)) => QueryState::Error(error.clone()),
            None => QueryState::Loading,
        }
    }

    /// Explicitly rereads durable local state; useful for pull-to-refresh and lost-event recovery.
    pub fn refresh(&mut self) {
        self.resource.restart();
    }
}

/// Runs a bounded application repository query and refreshes it on matching post-commit hints.
#[must_use]
pub fn use_aequora_query<T, E, F, Fut>(key: QueryKey, query: F) -> QueryHandle<T>
where
    T: Clone + 'static,
    E: Into<QueryError> + 'static,
    F: Fn() -> Fut + Clone + 'static,
    Fut: Future<Output = Result<T, E>> + 'static,
{
    let handle = use_aequora();
    let mut revision = use_signal(|| 0_u64);
    let receiver = handle.invalidations();
    let listener_key = key;
    use_future(move || {
        let key = listener_key.clone();
        let mut receiver = receiver.clone();
        async move {
            while receiver.changed(&key).await {
                revision += 1;
            }
        }
    });
    let resource = use_resource(move || {
        let _generation = revision();
        let future = query();
        async move { future.await.map_err(Into::into) }
    });
    QueryHandle {
        resource,
        marker: PhantomData,
    }
}
