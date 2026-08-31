use crate::{
    AppLifecycle, MobilePolicy, MobileResourceContext, MobileSyncBudget, NetworkContext,
    PolicyDecision, PowerContext, PushHint, StoragePressure, TransferKind,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum SyncTrigger {
    Foreground,
    NetworkRestored,
    PushHint,
    BackgroundTask,
    Manual,
    ScheduledMaintenance,
}

/// All Android/iOS signals enter the Rust scheduler through this normalized boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MobilePlatformEvent {
    LifecycleChanged(AppLifecycle),
    NetworkChanged(NetworkContext),
    PowerChanged(PowerContext),
    StorageChanged(StoragePressure),
    SecureKeysAvailabilityChanged(bool),
    PushHint(PushHint),
    BackgroundBudgetGranted(MobileSyncBudget),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinatorState {
    Idle,
    SyncRequested,
    Running,
    Backoff,
    WaitingForNetwork,
    WaitingForBudget,
    WaitingForCredentials,
    NeedsBootstrap,
    CompatibilityBlocked,
    StorageBlocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyncGrant {
    pub generation: u64,
    pub budget: MobileSyncBudget,
    pub transfer: TransferKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion {
    Checkpointed,
    MoreWorkCheckpointed,
    RetryCheckpointed,
    NeedsBootstrap,
    CompatibilityBlocked,
}

/// Single-owner, trigger-coalescing mobile scheduler state machine.
#[derive(Clone, Debug)]
pub struct MobileCoordinator {
    state: CoordinatorState,
    generation: u64,
    pending: BTreeSet<SyncTrigger>,
    policy: MobilePolicy,
    context: MobileResourceContext,
    granted_budget: Option<MobileSyncBudget>,
}

impl MobileCoordinator {
    #[must_use]
    pub fn new(policy: MobilePolicy, context: MobileResourceContext) -> Self {
        Self {
            state: CoordinatorState::Idle,
            generation: 0,
            pending: BTreeSet::new(),
            policy,
            context,
            granted_budget: None,
        }
    }

    #[must_use]
    pub const fn state(&self) -> CoordinatorState {
        self.state
    }

    #[must_use]
    pub fn pending_trigger_count(&self) -> usize {
        self.pending.len()
    }

    pub fn update_context(&mut self, context: MobileResourceContext) {
        self.context = context;
        if !self.pending.is_empty() && self.state != CoordinatorState::Running {
            self.state = CoordinatorState::SyncRequested;
        }
    }

    /// Coalesces duplicate platform triggers without changing durable synchronization state.
    pub fn request_sync(&mut self, trigger: SyncTrigger) {
        self.pending.insert(trigger);
        if self.state != CoordinatorState::Running {
            self.state = CoordinatorState::SyncRequested;
        }
    }

    /// Normalizes platform signals into context updates or coalesced wake requests.
    ///
    /// # Errors
    ///
    /// Rejects malformed push hints and invalid platform budgets.
    pub fn apply_platform_event(
        &mut self,
        event: MobilePlatformEvent,
    ) -> Result<(), CoordinatorError> {
        match event {
            MobilePlatformEvent::LifecycleChanged(lifecycle) => {
                self.context.lifecycle = lifecycle;
                if lifecycle == AppLifecycle::Foreground {
                    self.request_sync(SyncTrigger::Foreground);
                }
            }
            MobilePlatformEvent::NetworkChanged(network) => {
                let restored = !self.context.network.available() && network.available();
                self.context.network = network;
                if restored {
                    self.request_sync(SyncTrigger::NetworkRestored);
                }
            }
            MobilePlatformEvent::PowerChanged(power) => self.context.power = power,
            MobilePlatformEvent::StorageChanged(storage) => self.context.storage = storage,
            MobilePlatformEvent::SecureKeysAvailabilityChanged(available) => {
                self.context.secure_keys_available = available;
            }
            MobilePlatformEvent::PushHint(hint) => {
                hint.validate()
                    .map_err(|_| CoordinatorError::InvalidPlatformEvent)?;
                self.request_sync(SyncTrigger::PushHint);
            }
            MobilePlatformEvent::BackgroundBudgetGranted(budget) => {
                budget
                    .validate()
                    .map_err(|_| CoordinatorError::InvalidBudget)?;
                self.granted_budget = Some(budget);
                self.request_sync(SyncTrigger::BackgroundTask);
            }
        }
        if !self.pending.is_empty() && self.state != CoordinatorState::Running {
            self.state = CoordinatorState::SyncRequested;
        }
        Ok(())
    }

    /// Grants at most one bounded execution to the Rust synchronization engine.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinatorError::AlreadyRunning`] for competing schedulers or a budget error.
    pub fn grant(
        &mut self,
        transfer: TransferKind,
        platform_budget: Option<MobileSyncBudget>,
    ) -> Result<Option<SyncGrant>, CoordinatorError> {
        if self.state == CoordinatorState::Running {
            return Err(CoordinatorError::AlreadyRunning);
        }
        if self.pending.is_empty() {
            self.state = CoordinatorState::Idle;
            return Ok(None);
        }
        let platform_budget = platform_budget.or(self.granted_budget.take());
        let decision = self.policy.decide(self.context, transfer, platform_budget);
        let budget = match decision {
            PolicyDecision::Run(budget) => budget,
            PolicyDecision::WaitForNetwork => {
                self.state = CoordinatorState::WaitingForNetwork;
                return Ok(None);
            }
            PolicyDecision::WaitForBudget | PolicyDecision::DeferForResources => {
                self.state = CoordinatorState::WaitingForBudget;
                return Ok(None);
            }
            PolicyDecision::WaitForCredentials => {
                self.state = CoordinatorState::WaitingForCredentials;
                return Ok(None);
            }
            PolicyDecision::StorageUnavailable => {
                self.state = CoordinatorState::StorageBlocked;
                return Ok(None);
            }
        };
        budget
            .validate()
            .map_err(|_| CoordinatorError::InvalidBudget)?;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(CoordinatorError::GenerationExhausted)?;
        self.pending.clear();
        self.state = CoordinatorState::Running;
        Ok(Some(SyncGrant {
            generation: self.generation,
            budget,
            transfer,
        }))
    }

    /// Records only a durably checkpointed outcome. Process death before this call leaves durable
    /// outbox/cursor state as the source of truth for restart.
    ///
    /// # Errors
    ///
    /// Rejects stale grants and completion outside a running execution.
    pub fn complete(
        &mut self,
        grant: SyncGrant,
        completion: Completion,
    ) -> Result<(), CoordinatorError> {
        if self.state != CoordinatorState::Running || grant.generation != self.generation {
            return Err(CoordinatorError::StaleGrant);
        }
        self.state = match completion {
            Completion::Checkpointed => CoordinatorState::Idle,
            Completion::MoreWorkCheckpointed => {
                self.pending.insert(SyncTrigger::ScheduledMaintenance);
                CoordinatorState::SyncRequested
            }
            Completion::RetryCheckpointed => {
                self.pending.insert(SyncTrigger::ScheduledMaintenance);
                CoordinatorState::Backoff
            }
            Completion::NeedsBootstrap => CoordinatorState::NeedsBootstrap,
            Completion::CompatibilityBlocked => CoordinatorState::CompatibilityBlocked,
        };
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum CoordinatorError {
    #[error("a mobile sync execution already owns this coordinator")]
    AlreadyRunning,
    #[error("mobile sync grant is stale")]
    StaleGrant,
    #[error("mobile sync budget is invalid")]
    InvalidBudget,
    #[error("mobile coordinator generation exhausted")]
    GenerationExhausted,
    #[error("mobile platform event is invalid")]
    InvalidPlatformEvent,
}
