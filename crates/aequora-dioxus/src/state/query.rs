use crate::QueryError;
use aequora_types::SyncScopeId;
use std::sync::Arc;

/// Application-owned stable key for a bounded local query.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct QueryKey {
    value: Arc<str>,
    scope_id: Option<SyncScopeId>,
}

impl QueryKey {
    #[must_use]
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self {
            value: value.into(),
            scope_id: None,
        }
    }

    #[must_use]
    pub const fn in_scope(mut self, scope_id: SyncScopeId) -> Self {
        self.scope_id = Some(scope_id);
        self
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub const fn scope_id(&self) -> Option<SyncScopeId> {
        self.scope_id
    }
}

/// Reactive result of a bounded application-owned local query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryState<T> {
    Loading,
    Ready(T),
    /// A durable cached value remains renderable while it is being refreshed.
    Refreshing(T),
    Error(QueryError),
}
