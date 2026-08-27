//! Deterministic live-hint fault injection and reusable compliance checks.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard},
};

use aequora_live::{HintBroker, HintSubscription, LiveError, SyncHint};
use async_trait::async_trait;

#[derive(Debug, Default)]
struct FakeState {
    queue: VecDeque<SyncHint>,
    delayed: VecDeque<SyncHint>,
    held_for_reorder: Option<SyncHint>,
    drop_next: usize,
    duplicate_next: usize,
    delay_next: usize,
    reorder_next: bool,
    disconnected: bool,
}

/// One-subscriber deterministic fake capable of loss, duplication, reordering, delay, and close.
#[derive(Clone, Debug, Default)]
pub struct FakeHintBroker {
    state: Arc<Mutex<FakeState>>,
}

impl FakeHintBroker {
    pub fn drop_next(&self, count: usize) {
        lock(&self.state).drop_next = count;
    }

    pub fn duplicate_next(&self, extra_copies: usize) {
        lock(&self.state).duplicate_next = extra_copies;
    }

    pub fn delay_next(&self, count: usize) {
        lock(&self.state).delay_next = count;
    }

    pub fn reorder_next_pair(&self) {
        lock(&self.state).reorder_next = true;
    }

    pub fn release_delayed(&self) {
        let mut state = lock(&self.state);
        while let Some(hint) = state.delayed.pop_front() {
            state.queue.push_back(hint);
        }
    }

    pub fn disconnect(&self) {
        lock(&self.state).disconnected = true;
    }

    #[must_use]
    pub fn queued(&self) -> usize {
        lock(&self.state).queue.len()
    }
}

struct FakeSubscription {
    state: Arc<Mutex<FakeState>>,
}

#[async_trait]
impl HintSubscription for FakeSubscription {
    async fn next_hint(&mut self) -> Result<Option<SyncHint>, LiveError> {
        let mut state = lock(&self.state);
        if state.disconnected {
            return Ok(None);
        }
        Ok(state.queue.pop_front())
    }
}

#[async_trait]
impl HintBroker for FakeHintBroker {
    async fn publish(&self, hint: SyncHint) -> Result<(), LiveError> {
        let mut state = lock(&self.state);
        if state.drop_next > 0 {
            state.drop_next -= 1;
            return Ok(());
        }
        if state.delay_next > 0 {
            state.delay_next -= 1;
            state.delayed.push_back(hint);
            return Ok(());
        }
        if state.reorder_next && state.held_for_reorder.is_none() {
            state.held_for_reorder = Some(hint);
            return Ok(());
        }
        state.queue.push_back(hint);
        if let Some(held) = state.held_for_reorder.take() {
            state.queue.push_back(held);
            state.reorder_next = false;
        }
        let duplicates = state.duplicate_next;
        state.duplicate_next = 0;
        state.queue.extend(std::iter::repeat_n(hint, duplicates));
        Ok(())
    }

    async fn subscribe(&self) -> Result<Box<dyn HintSubscription>, LiveError> {
        Ok(Box::new(FakeSubscription {
            state: Arc::clone(&self.state),
        }))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
