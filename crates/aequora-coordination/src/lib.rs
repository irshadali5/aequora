//! Database-neutral identities and deterministic state transitions for local coordinator election.
//!
//! Storage adapters persist these transitions atomically. Wall-clock acquisition decisions use a
//! timestamp supplied by the adapter or host; fencing tokens protect correctness after clock skew,
//! suspension, or an ambiguous process failure.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a new identity.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wraps an existing UUID.
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// Returns the wrapped UUID.
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

uuid_id!(
    LocalStoreId,
    "Persistent identity of one local synchronization store."
);
uuid_id!(
    ProcessInstanceId,
    "Ephemeral identity of one runtime process."
);

/// Monotonic leadership epoch. A lower token is never authorized after a higher token commits.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct FencingToken(pub u64);

impl FencingToken {
    /// Allocates the next leadership epoch without wrapping.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError::TokenExhausted`] at `u64::MAX`.
    pub const fn checked_next(self) -> Result<Self, CoordinationError> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(CoordinationError::TokenExhausted),
        }
    }
}

/// Monotonic physical/logical generation of the local replica.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LocalStoreGeneration(pub u64);

impl LocalStoreGeneration {
    /// Initial installed local replica generation.
    pub const INITIAL: Self = Self(1);

    /// Allocates the next generation without wrapping.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError::GenerationExhausted`] at `u64::MAX`.
    pub const fn checked_next(self) -> Result<Self, CoordinationError> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(CoordinationError::GenerationExhausted),
        }
    }
}

/// Lease class. Maintenance excludes normal synchronization leadership.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LeaseKind {
    /// Normal network synchronization and coordinator-owned local work.
    SyncCoordinator,
    /// Exclusive migration, replacement, or destructive repair work.
    Maintenance,
}

/// Whether an adapter can safely coordinate independent processes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LocalCoordinationSupport {
    /// Durable atomic lease, fencing, and generation transitions are supported.
    Full,
    /// The adapter is safe only when one process owns the store.
    SingleProcessOnly,
}

/// Host-selected process behavior.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum LocalProcessMode {
    /// Use multi-process coordination when the adapter advertises full support.
    #[default]
    Auto,
    /// The host guarantees one runtime process owns this store.
    SingleProcess,
    /// Require full durable coordination support or fail startup.
    MultiProcess,
    /// Read-only process that never participates in election.
    Observer,
}

/// Durable lease grant returned by a successful acquisition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LeaseGrant {
    /// Persistent store identity.
    pub store_id: LocalStoreId,
    /// Runtime process holding the lease.
    pub owner_id: ProcessInstanceId,
    /// Monotonic leadership epoch.
    pub fencing_token: FencingToken,
    /// Store generation observed at acquisition.
    pub store_generation: LocalStoreGeneration,
    /// Lease class.
    pub kind: LeaseKind,
    /// Unix expiry timestamp in milliseconds.
    pub expires_at_unix_ms: u64,
}

/// One atomic acquisition request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LeaseRequest {
    /// Candidate runtime identity.
    pub owner_id: ProcessInstanceId,
    /// Requested lease class.
    pub kind: LeaseKind,
    /// Adapter/host wall-clock timestamp in milliseconds.
    pub now_unix_ms: u64,
    /// Positive lease duration in milliseconds.
    pub ttl_ms: u64,
}

impl LeaseRequest {
    /// Computes the requested deadline without wrapping.
    ///
    /// # Errors
    ///
    /// Rejects a zero TTL or timestamp overflow.
    pub const fn expires_at(self) -> Result<u64, CoordinationError> {
        if self.ttl_ms == 0 {
            return Err(CoordinationError::InvalidTtl);
        }
        match self.now_unix_ms.checked_add(self.ttl_ms) {
            Some(value) => Ok(value),
            None => Err(CoordinationError::TimestampOverflow),
        }
    }
}

/// Durable coordination state visible to followers and diagnostics.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CoordinationSnapshot {
    /// Persistent store identity.
    pub store_id: LocalStoreId,
    /// Current local replica generation.
    pub store_generation: LocalStoreGeneration,
    /// Current or most recently allocated fencing token.
    pub fencing_token: FencingToken,
    /// Current owner, absent after graceful release.
    pub owner_id: Option<ProcessInstanceId>,
    /// Current lease class.
    pub kind: LeaseKind,
    /// Lease expiry, or zero after graceful release.
    pub expires_at_unix_ms: u64,
}

impl CoordinationSnapshot {
    /// Whether an unexpired lease exists at `now_unix_ms`.
    #[must_use]
    pub fn is_active(self, now_unix_ms: u64) -> bool {
        self.owner_id.is_some() && self.expires_at_unix_ms > now_unix_ms
    }

    /// Validates a leader-exclusive commit against the current durable epoch and generation.
    ///
    /// # Errors
    ///
    /// Rejects an owner, token, kind, generation, or expiry mismatch.
    pub fn validate(self, grant: LeaseGrant, now_unix_ms: u64) -> Result<(), CoordinationError> {
        if self.store_id != grant.store_id {
            return Err(CoordinationError::StoreMismatch);
        }
        if self.store_generation != grant.store_generation {
            return Err(CoordinationError::GenerationChanged);
        }
        if self.owner_id != Some(grant.owner_id)
            || self.fencing_token != grant.fencing_token
            || self.kind != grant.kind
            || !self.is_active(now_unix_ms)
        {
            return Err(CoordinationError::StaleFence);
        }
        Ok(())
    }
}

/// Pure reference lease state used by adapters, simulations, and property tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseState {
    snapshot: CoordinationSnapshot,
}

impl LeaseState {
    /// Initializes an unowned store at generation one and token zero.
    #[must_use]
    pub const fn new(store_id: LocalStoreId) -> Self {
        Self {
            snapshot: CoordinationSnapshot {
                store_id,
                store_generation: LocalStoreGeneration::INITIAL,
                fencing_token: FencingToken(0),
                owner_id: None,
                kind: LeaseKind::SyncCoordinator,
                expires_at_unix_ms: 0,
            },
        }
    }

    /// Returns the current durable state.
    #[must_use]
    pub const fn snapshot(self) -> CoordinationSnapshot {
        self.snapshot
    }

    /// Atomically acquires an absent/expired lease and increments its fencing token.
    ///
    /// # Errors
    ///
    /// Rejects an active lease, invalid deadline, or exhausted token.
    pub fn acquire(&mut self, request: LeaseRequest) -> Result<LeaseGrant, CoordinationError> {
        if self.snapshot.is_active(request.now_unix_ms) {
            return Err(CoordinationError::LeaseHeld);
        }
        let grant = LeaseGrant {
            store_id: self.snapshot.store_id,
            owner_id: request.owner_id,
            fencing_token: self.snapshot.fencing_token.checked_next()?,
            store_generation: self.snapshot.store_generation,
            kind: request.kind,
            expires_at_unix_ms: request.expires_at()?,
        };
        self.snapshot.owner_id = Some(grant.owner_id);
        self.snapshot.fencing_token = grant.fencing_token;
        self.snapshot.kind = grant.kind;
        self.snapshot.expires_at_unix_ms = grant.expires_at_unix_ms;
        Ok(grant)
    }

    /// Renews only the current owner/token/generation.
    ///
    /// # Errors
    ///
    /// Rejects stale grants and invalid deadlines.
    pub fn renew(
        &mut self,
        grant: LeaseGrant,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> Result<LeaseGrant, CoordinationError> {
        self.snapshot.validate(grant, now_unix_ms)?;
        let expires_at_unix_ms = LeaseRequest {
            owner_id: grant.owner_id,
            kind: grant.kind,
            now_unix_ms,
            ttl_ms,
        }
        .expires_at()?;
        self.snapshot.expires_at_unix_ms = expires_at_unix_ms;
        Ok(LeaseGrant {
            expires_at_unix_ms,
            ..grant
        })
    }

    /// Gracefully releases only the current owner/token/generation.
    ///
    /// # Errors
    ///
    /// Rejects a stale grant.
    pub fn release(
        &mut self,
        grant: LeaseGrant,
        now_unix_ms: u64,
    ) -> Result<(), CoordinationError> {
        self.snapshot.validate(grant, now_unix_ms)?;
        self.snapshot.owner_id = None;
        self.snapshot.expires_at_unix_ms = 0;
        Ok(())
    }

    /// Advances the store generation under the current maintenance fence.
    ///
    /// # Errors
    ///
    /// Rejects non-maintenance/stale grants or generation exhaustion.
    pub fn advance_generation(
        &mut self,
        grant: LeaseGrant,
        now_unix_ms: u64,
    ) -> Result<LocalStoreGeneration, CoordinationError> {
        self.snapshot.validate(grant, now_unix_ms)?;
        if grant.kind != LeaseKind::Maintenance {
            return Err(CoordinationError::MaintenanceRequired);
        }
        let generation = self.snapshot.store_generation.checked_next()?;
        self.snapshot.store_generation = generation;
        Ok(generation)
    }
}

/// Stable coordination failure classification.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum CoordinationError {
    /// Another unexpired lease already owns the store.
    #[error("local coordinator lease is already held")]
    LeaseHeld,
    /// The presented leadership epoch is no longer current.
    #[error("stale local coordinator fencing token")]
    StaleFence,
    /// The lease belongs to another local store.
    #[error("local coordinator lease belongs to another store")]
    StoreMismatch,
    /// The local replica generation changed after acquisition.
    #[error("local store generation changed")]
    GenerationChanged,
    /// This transition requires an exclusive maintenance lease.
    #[error("local store generation changes require a maintenance lease")]
    MaintenanceRequired,
    /// Lease durations must be positive.
    #[error("local coordinator lease TTL must be positive")]
    InvalidTtl,
    /// Lease deadline arithmetic overflowed.
    #[error("local coordinator lease timestamp overflow")]
    TimestampOverflow,
    /// Every fencing-token value has been consumed.
    #[error("local coordinator fencing token exhausted")]
    TokenExhausted,
    /// Every local-store generation value has been consumed.
    #[error("local store generation exhausted")]
    GenerationExhausted,
    /// Requested process mode is unsupported by the adapter.
    #[error("local adapter does not support multi-process coordination")]
    UnsupportedProcessMode,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn crash_takeover_allocates_a_new_token_and_fences_the_old_owner() {
        let mut state = LeaseState::new(LocalStoreId::new());
        let first = state
            .acquire(LeaseRequest {
                owner_id: ProcessInstanceId::new(),
                kind: LeaseKind::SyncCoordinator,
                now_unix_ms: 10,
                ttl_ms: 10,
            })
            .unwrap_or_else(|error| panic!("{error}"));
        let second = state
            .acquire(LeaseRequest {
                owner_id: ProcessInstanceId::new(),
                kind: LeaseKind::SyncCoordinator,
                now_unix_ms: 20,
                ttl_ms: 10,
            })
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(second.fencing_token > first.fencing_token);
        assert_eq!(
            state.snapshot().validate(first, 20),
            Err(CoordinationError::StaleFence)
        );
    }

    #[test]
    fn loom_atomic_epoch_allows_one_winner_per_compare_exchange() {
        loom::model(|| {
            use loom::sync::{Arc, atomic::AtomicU64};
            use loom::thread;
            use std::sync::atomic::Ordering;

            let epoch = Arc::new(AtomicU64::new(0));
            let left = Arc::clone(&epoch);
            let right = Arc::clone(&epoch);
            let first = thread::spawn(move || {
                left.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            });
            let second = thread::spawn(move || {
                right
                    .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            });
            let winners = usize::from(first.join().unwrap_or(false))
                + usize::from(second.join().unwrap_or(false));
            assert_eq!(winners, 1);
        });
    }

    proptest! {
        #[test]
        fn successful_takeovers_strictly_increase_tokens(ttls in prop::collection::vec(1_u64..1000, 1..64)) {
            let mut state = LeaseState::new(LocalStoreId::new());
            let mut now = 0_u64;
            let mut prior = FencingToken(0);
            for ttl in ttls {
                let grant = state.acquire(LeaseRequest {
                    owner_id: ProcessInstanceId::new(),
                    kind: LeaseKind::SyncCoordinator,
                    now_unix_ms: now,
                    ttl_ms: ttl,
                })?;
                prop_assert!(grant.fencing_token > prior);
                prior = grant.fencing_token;
                now = now.saturating_add(ttl);
            }
        }
    }
}
