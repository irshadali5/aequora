//! Deterministic test contexts, test-only fault schedules, and versioned fixtures.

use crate::conformance::FaultPoint;
use aequora_conformance::verification::TestCategory;
use aequora_types::{DeviceId, EntityId, OperationId, TenantId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use uuid::Uuid;

/// Reproduction seed printed and persisted by every randomized verification run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct TestSeed(pub u64);

/// Explicitly controlled wall and monotonic clocks; neither reads the host clock.
#[derive(Clone, Debug)]
pub struct DeterministicClock {
    state: Arc<Mutex<ClockState>>,
}

#[derive(Clone, Copy, Debug)]
struct ClockState {
    wall_unix_ms: u64,
    monotonic_ms: u64,
    frozen: bool,
}

impl DeterministicClock {
    #[must_use]
    pub fn new(wall_unix_ms: u64) -> Self {
        Self {
            state: Arc::new(Mutex::new(ClockState {
                wall_unix_ms,
                monotonic_ms: 0,
                frozen: false,
            })),
        }
    }

    fn state(&self) -> MutexGuard<'_, ClockState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[must_use]
    pub fn wall_unix_ms(&self) -> u64 {
        self.state().wall_unix_ms
    }

    #[must_use]
    pub fn monotonic_ms(&self) -> u64 {
        self.state().monotonic_ms
    }

    pub fn freeze(&self) {
        self.state().frozen = true;
    }

    pub fn resume(&self) {
        self.state().frozen = false;
    }

    /// Advances both clocks unless the clock is frozen.
    ///
    /// # Errors
    ///
    /// Returns [`TestContextError::ClockOverflow`] rather than wrapping time.
    pub fn tick(&self, duration: Duration) -> Result<(), TestContextError> {
        if self.state().frozen {
            return Ok(());
        }
        self.advance(duration)
    }

    /// Explicitly advances both clocks, including while frozen.
    ///
    /// # Errors
    ///
    /// Returns [`TestContextError::ClockOverflow`] rather than wrapping time.
    pub fn advance(&self, duration: Duration) -> Result<(), TestContextError> {
        let millis =
            u64::try_from(duration.as_millis()).map_err(|_| TestContextError::ClockOverflow)?;
        let mut state = self.state();
        state.wall_unix_ms = state
            .wall_unix_ms
            .checked_add(millis)
            .ok_or(TestContextError::ClockOverflow)?;
        state.monotonic_ms = state
            .monotonic_ms
            .checked_add(millis)
            .ok_or(TestContextError::ClockOverflow)?;
        Ok(())
    }

    /// Changes wall time without changing monotonic ordering.
    pub fn jump_wall(&self, wall_unix_ms: u64) {
        self.state().wall_unix_ms = wall_unix_ms;
    }
}

/// Deterministic test-only ID stream; production UUID generation remains unchanged.
#[derive(Debug)]
pub struct DeterministicIdSource {
    seed: TestSeed,
    counter: AtomicU64,
}

impl DeterministicIdSource {
    #[must_use]
    pub const fn new(seed: TestSeed) -> Self {
        Self {
            seed,
            counter: AtomicU64::new(0),
        }
    }

    fn next_uuid(&self) -> Uuid {
        let counter = self.counter.fetch_add(1, Ordering::Relaxed);
        let high = splitmix64(self.seed.0 ^ counter);
        let low = splitmix64(high ^ counter.rotate_left(17));
        Uuid::from_u128((u128::from(high) << 64) | u128::from(low))
    }

    #[must_use]
    pub fn operation_id(&self) -> OperationId {
        OperationId::from_uuid(self.next_uuid())
    }

    #[must_use]
    pub fn entity_id(&self) -> EntityId {
        EntityId::from_uuid(self.next_uuid())
    }

    #[must_use]
    pub fn device_id(&self) -> DeviceId {
        DeviceId::from_uuid(self.next_uuid())
    }

    #[must_use]
    pub fn tenant_id(&self) -> TenantId {
        TenantId::from_uuid(self.next_uuid())
    }
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Safe description of a fault. The controller never aborts or panics by itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultAction {
    Continue,
    ReturnError(FaultErrorClass),
    Panic,
    AbortProcess,
    Delay(Duration),
    DropMessage,
    CorruptTestArtifact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultErrorClass {
    Transient,
    Permanent,
    ResourceExhausted,
}

/// Ordered, one-shot fault schedule owned exclusively by test infrastructure.
#[derive(Clone, Debug, Default)]
pub struct FaultController {
    scheduled: Arc<Mutex<BTreeMap<FaultPoint, VecDeque<FaultAction>>>>,
}

impl FaultController {
    fn scheduled(&self) -> MutexGuard<'_, BTreeMap<FaultPoint, VecDeque<FaultAction>>> {
        self.scheduled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn schedule(&self, point: FaultPoint, action: FaultAction) {
        self.scheduled().entry(point).or_default().push_back(action);
    }

    #[must_use]
    pub fn hit(&self, point: FaultPoint) -> FaultAction {
        self.scheduled()
            .get_mut(&point)
            .and_then(VecDeque::pop_front)
            .unwrap_or(FaultAction::Continue)
    }

    pub fn clear(&self) {
        self.scheduled().clear();
    }
}

/// Reusable deterministic context binding seed, clocks, IDs, and fault schedule.
#[derive(Debug)]
pub struct TestContext {
    pub seed: TestSeed,
    pub clock: DeterministicClock,
    pub ids: DeterministicIdSource,
    pub faults: FaultController,
}

impl TestContext {
    #[must_use]
    pub fn new(seed: TestSeed, wall_unix_ms: u64) -> Self {
        Self {
            seed,
            clock: DeterministicClock::new(wall_unix_ms),
            ids: DeterministicIdSource::new(seed),
            faults: FaultController::default(),
        }
    }

    /// Stable payload-free reproduction label for logs and failure artifacts.
    #[must_use]
    pub fn reproduction_label(&self, scenario: &str, adapter: &str, build: &str) -> String {
        format!(
            "seed={} scenario={scenario} adapter={adapter} build={build}",
            self.seed.0
        )
    }
}

/// Version binding carried by durable golden fixtures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureVersion {
    pub schema_version: u32,
    pub protocol_version: u32,
    pub registry_generation: u64,
}

/// Operational duration class used to place a test in a bounded CI tier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TestRuntimeClass {
    Fast,
    Medium,
    Long,
    Soak,
}

/// Whether one harness may use infrastructure outside the current process.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExternalResources {
    Denied,
    ExplicitlyConfigured,
}

/// Whether a harness may hard-exit a child process at a failpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ProcessFaultMode {
    ReturnedErrorsOnly,
    IsolatedChildProcess,
}

/// Bounded, versioned RON configuration for one verification tier.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TestConfiguration {
    pub schema_version: u32,
    pub category: TestCategory,
    pub runtime_class: TestRuntimeClass,
    pub seed: TestSeed,
    pub maximum_actions: usize,
    pub maximum_artifact_bytes: usize,
    pub external_resources: ExternalResources,
    pub process_fault_mode: ProcessFaultMode,
}

impl TestConfiguration {
    /// Rejects unversioned, unseeded, or unbounded test execution.
    ///
    /// # Errors
    ///
    /// Returns [`TestContextError::InvalidConfiguration`] for invalid bounds or schema.
    pub fn validate(&self) -> Result<(), TestContextError> {
        if self.schema_version != 1
            || self.maximum_actions == 0
            || self.maximum_actions > 1_000_000
            || self.maximum_artifact_bytes == 0
            || self.maximum_artifact_bytes > 64 * 1024 * 1024
        {
            Err(TestContextError::InvalidConfiguration)
        } else {
            Ok(())
        }
    }
}

/// Reviewed golden fixture identity and digest. Regeneration is intentionally not provided.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoldenFixture {
    pub name: String,
    pub version: FixtureVersion,
    pub digest: [u8; 32],
}

impl GoldenFixture {
    #[must_use]
    pub fn from_reviewed_bytes(
        name: impl Into<String>,
        version: FixtureVersion,
        bytes: &[u8],
    ) -> Self {
        Self {
            name: name.into(),
            version,
            digest: *blake3::hash(bytes).as_bytes(),
        }
    }

    #[must_use]
    pub fn verify(&self, bytes: &[u8]) -> bool {
        self.digest == *blake3::hash(bytes).as_bytes()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TestContextError {
    #[error("deterministic clock overflow")]
    ClockOverflow,
    #[error("test configuration is unversioned or exceeds verification bounds")]
    InvalidConfiguration,
}
