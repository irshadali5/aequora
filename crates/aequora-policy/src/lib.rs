//! Runtime-operational policy, coherent generations, and field-specific safe merge rules.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt,
    num::NonZeroU16,
    sync::{Arc, RwLock},
};
use thiserror::Error;

/// Monotonically increasing identifier for one complete effective policy snapshot.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ConfigGeneration(pub u64);

impl ConfigGeneration {
    /// Returns the next generation or fails instead of wrapping.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::GenerationExhausted`] at `u64::MAX`.
    pub fn next(self) -> Result<Self, PolicyError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(PolicyError::GenerationExhausted)
    }
}

/// Digest of non-secret effective semantic policy.
#[derive(Clone, Copy, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ConfigDigest([u8; 32]);

impl ConfigDigest {
    /// Constructs a digest from a cryptographic hash computed by the configuration layer.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the digest bytes for comparison or incident metadata.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for ConfigDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ConfigDigest({self})")
    }
}

impl fmt::Display for ConfigDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Validated batch bound in the inclusive range 1..=4096.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct BatchSize(NonZeroU16);

impl BatchSize {
    /// Creates a bounded batch size.
    ///
    /// # Errors
    ///
    /// Rejects zero and values above 4096.
    pub fn new(value: u16) -> Result<Self, PolicyError> {
        if value > 4_096 {
            return Err(PolicyError::OutOfBounds("batch_size"));
        }
        NonZeroU16::new(value)
            .map(Self)
            .ok_or(PolicyError::OutOfBounds("batch_size"))
    }

    /// Returns the validated count.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

impl TryFrom<u16> for BatchSize {
    type Error = PolicyError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<BatchSize> for u16 {
    fn from(value: BatchSize) -> Self {
        value.get()
    }
}

/// Validated non-zero worker count in the inclusive range 1..=1024.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct WorkerLimit(NonZeroU16);

impl WorkerLimit {
    /// Creates a bounded worker limit.
    ///
    /// # Errors
    ///
    /// Rejects zero and values above 1024.
    pub fn new(value: u16) -> Result<Self, PolicyError> {
        if value > 1_024 {
            return Err(PolicyError::OutOfBounds("worker_limit"));
        }
        NonZeroU16::new(value)
            .map(Self)
            .ok_or(PolicyError::OutOfBounds("worker_limit"))
    }

    /// Returns the validated count.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

impl TryFrom<u16> for WorkerLimit {
    type Error = PolicyError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<WorkerLimit> for u16 {
    fn from(value: WorkerLimit) -> Self {
        value.get()
    }
}

/// Explicit millisecond duration bounded to seven days.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct TimeoutMillis(u64);

impl TimeoutMillis {
    /// Creates a non-zero duration no longer than seven days.
    ///
    /// # Errors
    ///
    /// Rejects zero and excessively large values.
    pub fn new(value: u64) -> Result<Self, PolicyError> {
        if value == 0 || value > 7 * 24 * 60 * 60 * 1_000 {
            return Err(PolicyError::OutOfBounds("timeout_ms"));
        }
        Ok(Self(value))
    }

    /// Returns milliseconds.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for TimeoutMillis {
    type Error = PolicyError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<TimeoutMillis> for u64 {
    fn from(value: TimeoutMillis) -> Self {
        value.get()
    }
}

/// Runtime settings that tune progress and resource use without changing correctness semantics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimePolicy {
    /// Maximum operations in one synchronization batch.
    pub batch_size: BatchSize,
    /// Request timeout with an explicit unit.
    pub request_timeout: TimeoutMillis,
    /// Maximum concurrent runtime workers.
    pub worker_limit: WorkerLimit,
    /// Logging filter. This is observability policy, never an authorization switch.
    pub log_level: LogLevel,
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self {
            batch_size: BatchSize::new(256).unwrap_or_else(|error| panic!("{error}")),
            request_timeout: TimeoutMillis::new(15_000).unwrap_or_else(|error| panic!("{error}")),
            worker_limit: WorkerLimit::new(8).unwrap_or_else(|error| panic!("{error}")),
            log_level: LogLevel::Info,
        }
    }
}

impl RuntimePolicy {
    /// Revalidates all bounds after deserialization or composition.
    ///
    /// # Errors
    ///
    /// Returns a precise field-bound error.
    pub fn validate(&self) -> Result<(), PolicyError> {
        BatchSize::new(self.batch_size.get())?;
        TimeoutMillis::new(self.request_timeout.get())?;
        WorkerLimit::new(self.worker_limit.get())?;
        Ok(())
    }
}

/// Safe log-level choices.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

/// Lifecycle class of one configuration setting.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum MutabilityClass {
    Reloadable,
    RestartRequired,
    StartupOnly,
    ImmutableAfterInitialization,
}

/// Safety classification applied to a candidate change.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ChangeClass {
    Reloadable,
    RestartRequired,
    Forbidden,
    SecuritySensitive,
}

/// One changed setting and its activation class.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyChange {
    pub key: &'static str,
    pub class: ChangeClass,
}

/// One complete immutable runtime-policy generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicySnapshot {
    pub generation: ConfigGeneration,
    pub digest: ConfigDigest,
    pub policy: RuntimePolicy,
}

impl PolicySnapshot {
    /// Builds a coherent snapshot and computes its semantic digest.
    ///
    /// # Errors
    ///
    /// Returns a validation or serialization error.
    pub fn new(generation: ConfigGeneration, policy: RuntimePolicy) -> Result<Self, PolicyError> {
        policy.validate()?;
        let bytes = postcard::to_stdvec(&policy).map_err(|_| PolicyError::Encoding)?;
        Ok(Self {
            generation,
            digest: ConfigDigest(*blake3::hash(&bytes).as_bytes()),
            policy,
        })
    }
}

/// Read boundary for a coherent runtime configuration snapshot.
pub trait RuntimeConfigStore: Send + Sync {
    fn current(&self) -> Arc<PolicySnapshot>;
}

/// Lock-backed atomic publication of immutable `Arc` snapshots.
///
/// The lock guards only pointer replacement. Readers clone exactly one complete generation.
pub struct AtomicRuntimeConfigStore {
    current: RwLock<Arc<PolicySnapshot>>,
}

impl AtomicRuntimeConfigStore {
    /// Creates a store from a validated initial snapshot.
    #[must_use]
    pub fn new(initial: PolicySnapshot) -> Self {
        Self {
            current: RwLock::new(Arc::new(initial)),
        }
    }

    /// Validates and atomically publishes a reloadable candidate.
    ///
    /// On every error the previous generation remains active.
    ///
    /// # Errors
    ///
    /// Rejects invalid candidates, restart-required changes, stale generations, and poisoned
    /// synchronization state.
    pub fn reload(&self, candidate: RuntimePolicy) -> Result<Arc<PolicySnapshot>, PolicyError> {
        candidate.validate()?;
        let current = self.current();
        let changes = diff_runtime_policy(&current.policy, &candidate);
        if changes
            .iter()
            .any(|change| change.class != ChangeClass::Reloadable)
        {
            return Err(PolicyError::RestartRequired);
        }
        let next = Arc::new(PolicySnapshot::new(current.generation.next()?, candidate)?);
        let mut guard = self
            .current
            .write()
            .map_err(|_| PolicyError::StorePoisoned)?;
        if guard.generation != current.generation {
            return Err(PolicyError::StaleGeneration);
        }
        *guard = Arc::clone(&next);
        Ok(next)
    }
}

impl RuntimeConfigStore for AtomicRuntimeConfigStore {
    fn current(&self) -> Arc<PolicySnapshot> {
        self.current.read().map_or_else(
            |poisoned| Arc::clone(poisoned.get_ref()),
            |guard| Arc::clone(&guard),
        )
    }
}

/// Classifies changes between two policy values.
#[must_use]
pub fn diff_runtime_policy(
    current: &RuntimePolicy,
    candidate: &RuntimePolicy,
) -> Vec<PolicyChange> {
    let mut changes = Vec::new();
    if current.batch_size != candidate.batch_size {
        changes.push(PolicyChange {
            key: "runtime.batch_size",
            class: ChangeClass::Reloadable,
        });
    }
    if current.request_timeout != candidate.request_timeout {
        changes.push(PolicyChange {
            key: "runtime.request_timeout_ms",
            class: ChangeClass::Reloadable,
        });
    }
    if current.log_level != candidate.log_level {
        changes.push(PolicyChange {
            key: "runtime.log_level",
            class: ChangeClass::Reloadable,
        });
    }
    if current.worker_limit != candidate.worker_limit {
        changes.push(PolicyChange {
            key: "runtime.worker_limit",
            class: ChangeClass::RestartRequired,
        });
    }
    changes
}

/// Layered product/runtime policy. Merge semantics are field-specific and deny-wins.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LayerPolicy {
    pub maximum_batch: BatchSize,
    pub require_mfa: bool,
    pub allowed_modules: BTreeSet<String>,
}

/// Computes the safe intersection of a higher and lower policy layer.
#[must_use]
pub fn merge_policy(higher: &LayerPolicy, lower: &LayerPolicy) -> LayerPolicy {
    LayerPolicy {
        maximum_batch: if higher.maximum_batch <= lower.maximum_batch {
            higher.maximum_batch
        } else {
            lower.maximum_batch
        },
        require_mfa: higher.require_mfa || lower.require_mfa,
        allowed_modules: higher
            .allowed_modules
            .intersection(&lower.allowed_modules)
            .cloned()
            .collect(),
    }
}

/// Runtime policy validation or publication failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PolicyError {
    #[error("policy field {0} is outside its safe bound")]
    OutOfBounds(&'static str),
    #[error("policy encoding failed")]
    Encoding,
    #[error("configuration generation exhausted")]
    GenerationExhausted,
    #[error("candidate contains a restart-required change")]
    RestartRequired,
    #[error("candidate was based on a stale generation")]
    StaleGeneration,
    #[error("runtime configuration store is unavailable")]
    StorePoisoned,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn batch(value: u16) -> BatchSize {
        BatchSize::new(value).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn invalid_bounds_never_construct_validated_types() {
        assert!(BatchSize::new(0).is_err());
        assert!(BatchSize::new(4_097).is_err());
        assert!(WorkerLimit::new(0).is_err());
        assert!(TimeoutMillis::new(0).is_err());
    }

    #[test]
    fn merge_uses_minimum_intersection_and_required_wins() {
        let higher = LayerPolicy {
            maximum_batch: batch(256),
            require_mfa: true,
            allowed_modules: ["core".to_owned(), "reports".to_owned()].into(),
        };
        let lower = LayerPolicy {
            maximum_batch: batch(1_000),
            require_mfa: false,
            allowed_modules: ["core".to_owned(), "experimental".to_owned()].into(),
        };
        let effective = merge_policy(&higher, &lower);
        assert_eq!(effective.maximum_batch, batch(256));
        assert!(effective.require_mfa);
        assert_eq!(effective.allowed_modules, ["core".to_owned()].into());
    }

    #[test]
    fn failed_reload_keeps_previous_generation() {
        let initial = PolicySnapshot::new(ConfigGeneration(7), RuntimePolicy::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let store = AtomicRuntimeConfigStore::new(initial);
        let restart = RuntimePolicy {
            worker_limit: WorkerLimit::new(9).unwrap_or_else(|error| panic!("{error}")),
            ..RuntimePolicy::default()
        };
        assert_eq!(store.reload(restart), Err(PolicyError::RestartRequired));
        assert_eq!(store.current().generation, ConfigGeneration(7));
    }

    #[test]
    fn concurrent_readers_only_observe_complete_generations() {
        let initial = PolicySnapshot::new(ConfigGeneration(1), RuntimePolicy::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let store = Arc::new(AtomicRuntimeConfigStore::new(initial));
        let readers = (0..8)
            .map(|_| {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    for _ in 0..1_000 {
                        let snapshot = store.current();
                        let pair = (snapshot.generation.0, snapshot.policy.batch_size.get());
                        assert!(matches!(pair, (1, 256) | (2, 128)));
                    }
                })
            })
            .collect::<Vec<_>>();
        let candidate = RuntimePolicy {
            batch_size: batch(128),
            ..RuntimePolicy::default()
        };
        store
            .reload(candidate)
            .unwrap_or_else(|error| panic!("{error}"));
        for reader in readers {
            assert!(reader.join().is_ok());
        }
    }
}
