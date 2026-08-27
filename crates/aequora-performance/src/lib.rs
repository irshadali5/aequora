//! Runtime-neutral performance and memory policy for Aequora hot paths.
//!
//! This crate deliberately owns limits, reproducible workload descriptions, and bounded data
//! structures, but no Tokio executor, Rayon pool, transport, or database adapter. Edge crates map
//! the policy into their runtime-specific implementations.

use aequora_invariants::InvariantId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use thiserror::Error;

/// Part 19 invariants in stable identifier order.
pub const PERFORMANCE_INVARIANTS: [InvariantId; 9] = [
    InvariantId::PerformanceBoundedMemory,
    InvariantId::PerformanceStreamingLargeObjects,
    InvariantId::PerformanceCpuIsolation,
    InvariantId::PerformanceCorrectnessPreservation,
    InvariantId::PerformanceImmutableHotState,
    InvariantId::PerformanceReproducibility,
    InvariantId::PerformancePagedUiState,
    InvariantId::PerformanceBlobReferences,
    InvariantId::PerformanceEarlyAdmission,
];

/// Conservative runtime shapes. A profile chooses defaults, never an unbounded mode.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum PerformanceProfile {
    /// Low-memory mobile client with one small pipeline.
    MobileLowMemory,
    /// Desktop client with modest parallelism and cache.
    Desktop,
    /// Balanced server settings.
    #[default]
    ServerStandard,
    /// Larger server limits that still require load-test evidence before production use.
    ServerHighThroughput,
}

/// Explicit peak budgets for each independently retained memory domain.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBudget {
    pub request_bytes: usize,
    pub decode_bytes: usize,
    pub response_bytes: usize,
    pub snapshot_pipeline_bytes: usize,
    pub cache_bytes: usize,
    pub ready_queue_bytes: usize,
    pub pending_decode_records: usize,
}

/// Dedicated CPU-pool and submission bounds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CpuBudget {
    pub worker_threads: usize,
    pub parallel_threshold_items: usize,
    pub max_queued_jobs: usize,
}

/// Limits for a streaming large-object pipeline.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StreamBudget {
    pub max_chunk_bytes: usize,
    pub max_in_flight_chunks: usize,
    pub max_records_per_chunk: usize,
}

impl StreamBudget {
    /// Maximum bytes retained by the chunk window, excluding adapter-owned durable buffers.
    #[must_use]
    pub const fn pipeline_bytes(self) -> usize {
        self.max_chunk_bytes
            .saturating_mul(self.max_in_flight_chunks)
    }
}

/// Bounds for query-derived reactive view state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReactiveViewBudget {
    pub max_page_items: usize,
    pub max_pending_invalidations: usize,
}

/// Complete runtime-neutral Part 19 policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PerformancePolicy {
    pub profile: PerformanceProfile,
    pub memory: MemoryBudget,
    pub cpu: CpuBudget,
    pub snapshot: StreamBudget,
    pub blob: StreamBudget,
    pub reactive_view: ReactiveViewBudget,
    pub compression_threshold_bytes: usize,
}

impl PerformancePolicy {
    /// Returns bounded defaults for a named runtime shape.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub const fn for_profile(profile: PerformanceProfile) -> Self {
        match profile {
            PerformanceProfile::MobileLowMemory => Self {
                profile,
                memory: MemoryBudget {
                    request_bytes: 256 * 1_024,
                    decode_bytes: 512 * 1_024,
                    response_bytes: 1024 * 1_024,
                    snapshot_pipeline_bytes: 2 * 1_024 * 1_024,
                    cache_bytes: 8 * 1_024 * 1_024,
                    ready_queue_bytes: 512 * 1_024,
                    pending_decode_records: 256,
                },
                cpu: CpuBudget {
                    worker_threads: 1,
                    parallel_threshold_items: 512,
                    max_queued_jobs: 2,
                },
                snapshot: StreamBudget {
                    max_chunk_bytes: 512 * 1_024,
                    max_in_flight_chunks: 2,
                    max_records_per_chunk: 256,
                },
                blob: StreamBudget {
                    max_chunk_bytes: 256 * 1_024,
                    max_in_flight_chunks: 2,
                    max_records_per_chunk: 1,
                },
                reactive_view: ReactiveViewBudget {
                    max_page_items: 100,
                    max_pending_invalidations: 64,
                },
                compression_threshold_bytes: 8 * 1_024,
            },
            PerformanceProfile::Desktop => Self {
                profile,
                memory: MemoryBudget {
                    request_bytes: 1024 * 1_024,
                    decode_bytes: 4 * 1_024 * 1_024,
                    response_bytes: 4 * 1_024 * 1_024,
                    snapshot_pipeline_bytes: 16 * 1_024 * 1_024,
                    cache_bytes: 64 * 1_024 * 1_024,
                    ready_queue_bytes: 4 * 1_024 * 1_024,
                    pending_decode_records: 1_024,
                },
                cpu: CpuBudget {
                    worker_threads: 2,
                    parallel_threshold_items: 256,
                    max_queued_jobs: 8,
                },
                snapshot: StreamBudget {
                    max_chunk_bytes: 4 * 1_024 * 1_024,
                    max_in_flight_chunks: 3,
                    max_records_per_chunk: 1_000,
                },
                blob: StreamBudget {
                    max_chunk_bytes: 1024 * 1_024,
                    max_in_flight_chunks: 4,
                    max_records_per_chunk: 1,
                },
                reactive_view: ReactiveViewBudget {
                    max_page_items: 250,
                    max_pending_invalidations: 256,
                },
                compression_threshold_bytes: 4 * 1_024,
            },
            PerformanceProfile::ServerStandard => Self {
                profile,
                memory: MemoryBudget {
                    request_bytes: 4 * 1_024 * 1_024,
                    decode_bytes: 8 * 1_024 * 1_024,
                    response_bytes: 8 * 1_024 * 1_024,
                    snapshot_pipeline_bytes: 64 * 1_024 * 1_024,
                    cache_bytes: 256 * 1_024 * 1_024,
                    ready_queue_bytes: 32 * 1_024 * 1_024,
                    pending_decode_records: 4_096,
                },
                cpu: CpuBudget {
                    worker_threads: 4,
                    parallel_threshold_items: 128,
                    max_queued_jobs: 32,
                },
                snapshot: StreamBudget {
                    max_chunk_bytes: 8 * 1_024 * 1_024,
                    max_in_flight_chunks: 4,
                    max_records_per_chunk: 2_000,
                },
                blob: StreamBudget {
                    max_chunk_bytes: 4 * 1_024 * 1_024,
                    max_in_flight_chunks: 4,
                    max_records_per_chunk: 1,
                },
                reactive_view: ReactiveViewBudget {
                    max_page_items: 500,
                    max_pending_invalidations: 1_024,
                },
                compression_threshold_bytes: 4 * 1_024,
            },
            PerformanceProfile::ServerHighThroughput => Self {
                profile,
                memory: MemoryBudget {
                    request_bytes: 8 * 1_024 * 1_024,
                    decode_bytes: 16 * 1_024 * 1_024,
                    response_bytes: 16 * 1_024 * 1_024,
                    snapshot_pipeline_bytes: 256 * 1_024 * 1_024,
                    cache_bytes: 1024 * 1_024 * 1_024,
                    ready_queue_bytes: 128 * 1_024 * 1_024,
                    pending_decode_records: 16_384,
                },
                cpu: CpuBudget {
                    worker_threads: 8,
                    parallel_threshold_items: 128,
                    max_queued_jobs: 128,
                },
                snapshot: StreamBudget {
                    max_chunk_bytes: 16 * 1_024 * 1_024,
                    max_in_flight_chunks: 8,
                    max_records_per_chunk: 4_000,
                },
                blob: StreamBudget {
                    max_chunk_bytes: 8 * 1_024 * 1_024,
                    max_in_flight_chunks: 8,
                    max_records_per_chunk: 1,
                },
                reactive_view: ReactiveViewBudget {
                    max_page_items: 1_000,
                    max_pending_invalidations: 4_096,
                },
                compression_threshold_bytes: 4 * 1_024,
            },
        }
    }

    /// Verifies non-zero limits and cross-budget retention safety.
    ///
    /// # Errors
    ///
    /// Returns [`PerformanceError::InvalidPolicy`] for inconsistent limits.
    pub const fn validate(self) -> Result<(), PerformanceError> {
        if self.memory.request_bytes == 0
            || self.memory.decode_bytes < self.memory.request_bytes
            || self.memory.response_bytes == 0
            || self.memory.snapshot_pipeline_bytes == 0
            || self.memory.cache_bytes == 0
            || self.memory.ready_queue_bytes == 0
            || self.memory.pending_decode_records == 0
            || self.cpu.worker_threads == 0
            || self.cpu.parallel_threshold_items == 0
            || self.cpu.max_queued_jobs == 0
            || self.snapshot.max_chunk_bytes == 0
            || self.snapshot.max_in_flight_chunks == 0
            || self.snapshot.max_records_per_chunk == 0
            || self.blob.max_chunk_bytes == 0
            || self.blob.max_in_flight_chunks == 0
            || self.blob.max_records_per_chunk == 0
            || self.reactive_view.max_page_items == 0
            || self.reactive_view.max_pending_invalidations == 0
            || self.compression_threshold_bytes == 0
            || self.snapshot.pipeline_bytes() > self.memory.snapshot_pipeline_bytes
            || self.blob.pipeline_bytes() > self.memory.snapshot_pipeline_bytes
        {
            return Err(PerformanceError::InvalidPolicy);
        }
        Ok(())
    }
}

impl Default for PerformancePolicy {
    fn default() -> Self {
        Self::for_profile(PerformanceProfile::ServerStandard)
    }
}

/// Memory pressure is a scheduling hint, never permission to weaken correctness.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MemoryPressure {
    Normal,
    Elevated,
    Critical,
}

/// Deterministic optional-work response to memory pressure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct PressureDecision {
    pub pause_snapshots: bool,
    pub pause_maintenance: bool,
    pub reduce_optional_cache: bool,
    pub reject_new_bulk_work: bool,
}

impl MemoryPressure {
    #[must_use]
    pub const fn decision(self) -> PressureDecision {
        match self {
            Self::Normal => PressureDecision {
                pause_snapshots: false,
                pause_maintenance: false,
                reduce_optional_cache: false,
                reject_new_bulk_work: false,
            },
            Self::Elevated => PressureDecision {
                pause_snapshots: true,
                pause_maintenance: true,
                reduce_optional_cache: true,
                reject_new_bulk_work: false,
            },
            Self::Critical => PressureDecision {
                pause_snapshots: true,
                pause_maintenance: true,
                reduce_optional_cache: true,
                reject_new_bulk_work: true,
            },
        }
    }
}

/// Failure from a bounded performance-policy primitive.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PerformanceError {
    #[error("performance policy contains zero or inconsistent limits")]
    InvalidPolicy,
    #[error("item weight {actual} exceeds byte limit {maximum}")]
    ItemTooLarge { actual: usize, maximum: usize },
    #[error("bounded queue is full")]
    QueueFull,
    #[error("reactive view page contains {actual} items, limit is {maximum}")]
    ViewPageTooLarge { actual: usize, maximum: usize },
    #[error("immutable registry generation must be non-zero")]
    ZeroGeneration,
    #[error("benchmark workload is incomplete or unbounded")]
    InvalidWorkload,
    #[error("performance report contains duplicate or invalid phase measurements")]
    InvalidReport,
    #[error("regression thresholds must be at most 10000 basis points")]
    InvalidRegressionPolicy,
}

/// FIFO queue with independent item and byte caps. Callers supply retained weight explicitly.
#[derive(Clone, Debug)]
pub struct BoundedQueue<T> {
    entries: VecDeque<(T, usize)>,
    max_items: usize,
    max_bytes: usize,
    used_bytes: usize,
}

impl<T> BoundedQueue<T> {
    /// Creates a queue only when both limits are non-zero.
    ///
    /// # Errors
    ///
    /// Returns [`PerformanceError::InvalidPolicy`] for a zero item or byte limit.
    pub fn new(max_items: usize, max_bytes: usize) -> Result<Self, PerformanceError> {
        if max_items == 0 || max_bytes == 0 {
            return Err(PerformanceError::InvalidPolicy);
        }
        Ok(Self {
            entries: VecDeque::with_capacity(max_items.min(1_024)),
            max_items,
            max_bytes,
            used_bytes: 0,
        })
    }

    /// Adds one caller-owned value if both hard bounds remain satisfied.
    ///
    /// # Errors
    ///
    /// Returns an item-size or queue-capacity failure without retaining `value`.
    pub fn try_push(&mut self, value: T, retained_bytes: usize) -> Result<(), PerformanceError> {
        if retained_bytes > self.max_bytes {
            return Err(PerformanceError::ItemTooLarge {
                actual: retained_bytes,
                maximum: self.max_bytes,
            });
        }
        if self.entries.len() == self.max_items
            || self.used_bytes.saturating_add(retained_bytes) > self.max_bytes
        {
            return Err(PerformanceError::QueueFull);
        }
        self.entries.push_back((value, retained_bytes));
        self.used_bytes += retained_bytes;
        Ok(())
    }

    /// Removes the oldest value and releases its retained-byte accounting.
    pub fn pop(&mut self) -> Option<T> {
        self.entries.pop_front().map(|(value, bytes)| {
            self.used_bytes = self.used_bytes.saturating_sub(bytes);
            value
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub const fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Immutable, versioned registry snapshot for lock-free shared hot-path reads.
#[derive(Clone, Debug)]
pub struct ImmutableRegistry<K, V> {
    generation: u64,
    values: Arc<BTreeMap<K, V>>,
}

impl<K, V> ImmutableRegistry<K, V>
where
    K: Ord,
{
    /// Creates one immutable generation.
    ///
    /// # Errors
    ///
    /// Returns [`PerformanceError::ZeroGeneration`] when the generation is zero.
    pub fn new(generation: u64, values: BTreeMap<K, V>) -> Result<Self, PerformanceError> {
        if generation == 0 {
            return Err(PerformanceError::ZeroGeneration);
        }
        Ok(Self {
            generation,
            values: Arc::new(values),
        })
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.values.get(key)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// One query-derived UI page. It cannot grow beyond its configured page size.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PagedView<T> {
    items: Vec<T>,
    maximum: usize,
    pub next_offset: Option<u64>,
}

impl<T> PagedView<T> {
    /// Creates an empty bounded view page.
    ///
    /// # Errors
    ///
    /// Returns [`PerformanceError::InvalidPolicy`] for a zero page size.
    pub fn new(maximum: usize) -> Result<Self, PerformanceError> {
        if maximum == 0 {
            return Err(PerformanceError::InvalidPolicy);
        }
        Ok(Self {
            items: Vec::with_capacity(maximum.min(1_024)),
            maximum,
            next_offset: None,
        })
    }

    /// Replaces the page without allowing a full-database-sized value to enter reactive state.
    ///
    /// # Errors
    ///
    /// Returns [`PerformanceError::ViewPageTooLarge`] when `items` exceeds the page bound.
    pub fn replace(
        &mut self,
        items: Vec<T>,
        next_offset: Option<u64>,
    ) -> Result<(), PerformanceError> {
        if items.len() > self.maximum {
            return Err(PerformanceError::ViewPageTooLarge {
                actual: items.len(),
                maximum: self.maximum,
            });
        }
        self.items = items;
        self.next_offset = next_offset;
        Ok(())
    }

    #[must_use]
    pub fn items(&self) -> &[T] {
        &self.items
    }
}

/// Coalesces repeated UI invalidations and falls back to one full refresh when its bound fills.
#[derive(Clone, Debug)]
pub struct InvalidationCoalescer<K> {
    keys: BTreeSet<K>,
    maximum: usize,
    full_refresh: bool,
}

impl<K: Ord> InvalidationCoalescer<K> {
    /// Creates a coalescer with a hard unique-key bound.
    ///
    /// # Errors
    ///
    /// Returns [`PerformanceError::InvalidPolicy`] when `maximum` is zero.
    pub fn new(maximum: usize) -> Result<Self, PerformanceError> {
        if maximum == 0 {
            return Err(PerformanceError::InvalidPolicy);
        }
        Ok(Self {
            keys: BTreeSet::new(),
            maximum,
            full_refresh: false,
        })
    }

    pub fn invalidate(&mut self, key: K) {
        if self.full_refresh {
            return;
        }
        if self.keys.len() == self.maximum && !self.keys.contains(&key) {
            self.keys.clear();
            self.full_refresh = true;
            return;
        }
        self.keys.insert(key);
    }

    /// Drains the current coalesced decision. `None` means refresh the whole paged query.
    pub fn drain(&mut self) -> Option<Vec<K>> {
        if self.full_refresh {
            self.full_refresh = false;
            self.keys.clear();
            return None;
        }
        Some(std::mem::take(&mut self.keys).into_iter().collect())
    }
}

/// Required database query/index characteristics for a production adapter.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HotQuery {
    JournalAfterCursor,
    PendingOutbox,
    DueRetry,
    EntityVersion,
    SnapshotPage,
}

/// Provider-neutral query shape; adapter certification maps these logical columns to its schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HotQueryContract {
    pub query: HotQuery,
    pub equality_prefix: &'static [&'static str],
    pub ordered_suffix: &'static [&'static str],
    pub bounded_result: bool,
}

pub const HOT_QUERY_CONTRACTS: [HotQueryContract; 5] = [
    HotQueryContract {
        query: HotQuery::JournalAfterCursor,
        equality_prefix: &["tenant", "scope"],
        ordered_suffix: &["sequence"],
        bounded_result: true,
    },
    HotQueryContract {
        query: HotQuery::PendingOutbox,
        equality_prefix: &["state"],
        ordered_suffix: &["priority", "local_sequence"],
        bounded_result: true,
    },
    HotQueryContract {
        query: HotQuery::DueRetry,
        equality_prefix: &["state"],
        ordered_suffix: &["next_retry", "local_sequence"],
        bounded_result: true,
    },
    HotQueryContract {
        query: HotQuery::EntityVersion,
        equality_prefix: &["tenant", "entity_type", "entity_id"],
        ordered_suffix: &[],
        bounded_result: true,
    },
    HotQueryContract {
        query: HotQuery::SnapshotPage,
        equality_prefix: &["tenant", "snapshot"],
        ordered_suffix: &["entity_order"],
        bounded_result: true,
    },
];

/// Benchmark layers are deliberately separate so microbenchmark speed cannot certify semantics.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum BenchmarkKind {
    Micro,
    Component,
    Database,
    EndToEnd,
    Soak,
}

/// Stable workload scenarios from Part 19.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WorkloadProfile {
    SchoolDayMorning,
    FeePaymentPeak,
    MassReconnect,
    LargeBootstrap,
    BulkImport,
    LowBandwidthMobile,
}

/// Fixed-seed, bounded performance workload description.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadManifest {
    pub format_version: u32,
    pub name: String,
    pub profile: WorkloadProfile,
    pub benchmark_kind: BenchmarkKind,
    pub seed: u64,
    pub tenants: u32,
    pub clients_per_tenant: u32,
    pub operations: u64,
    pub payload_bytes_p50: u32,
    pub payload_bytes_p99: u32,
    pub max_dependency_edges: u64,
    pub max_runtime_seconds: u64,
    pub durable: bool,
}

impl WorkloadManifest {
    /// Verifies that every externally selected workload dimension is finite and non-zero.
    ///
    /// # Errors
    ///
    /// Returns [`PerformanceError::InvalidWorkload`] for incomplete or unbounded input.
    pub fn validate(&self) -> Result<(), PerformanceError> {
        if self.format_version != 1
            || self.name.trim().is_empty()
            || self.seed == 0
            || self.tenants == 0
            || self.clients_per_tenant == 0
            || self.operations == 0
            || self.payload_bytes_p50 == 0
            || self.payload_bytes_p99 < self.payload_bytes_p50
            || self.max_runtime_seconds == 0
        {
            return Err(PerformanceError::InvalidWorkload);
        }
        Ok(())
    }

    /// Stable content digest used to bind before/after reports to the exact workload.
    ///
    /// # Errors
    ///
    /// Returns [`PerformanceError::InvalidWorkload`] when validation fails.
    pub fn digest(&self) -> Result<[u8; 32], PerformanceError> {
        self.validate()?;
        let encoded = postcard_compatible_manifest_bytes(self);
        Ok(*blake3::hash(&encoded).as_bytes())
    }
}

// This deliberately avoids a Postcard dependency in the policy crate's production graph. The
// fixed field framing is stable, length-delimited, and sufficient to bind a benchmark manifest.
fn postcard_compatible_manifest_bytes(manifest: &WorkloadManifest) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(128 + manifest.name.len());
    bytes.extend_from_slice(&manifest.format_version.to_le_bytes());
    bytes.extend_from_slice(&(manifest.name.len() as u64).to_le_bytes());
    bytes.extend_from_slice(manifest.name.as_bytes());
    bytes.push(manifest.profile as u8);
    bytes.push(manifest.benchmark_kind as u8);
    bytes.extend_from_slice(&manifest.seed.to_le_bytes());
    bytes.extend_from_slice(&manifest.tenants.to_le_bytes());
    bytes.extend_from_slice(&manifest.clients_per_tenant.to_le_bytes());
    bytes.extend_from_slice(&manifest.operations.to_le_bytes());
    bytes.extend_from_slice(&manifest.payload_bytes_p50.to_le_bytes());
    bytes.extend_from_slice(&manifest.payload_bytes_p99.to_le_bytes());
    bytes.extend_from_slice(&manifest.max_dependency_edges.to_le_bytes());
    bytes.extend_from_slice(&manifest.max_runtime_seconds.to_le_bytes());
    bytes.push(u8::from(manifest.durable));
    bytes
}

/// Low-cardinality measured phase.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum MeasuredPhase {
    Decode,
    Authorization,
    Validation,
    Planning,
    Database,
    Encode,
    Reconciliation,
    Snapshot,
    Blob,
}

/// Environment metadata required for reproducible reports.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkEnvironment {
    pub cpu: String,
    pub ram_bytes: u64,
    pub os: String,
    pub database: String,
    pub aequora_commit: String,
    pub durability: String,
}

/// One phase's aggregate metrics. IDs and tenant labels are intentionally absent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseMeasurement {
    pub phase: MeasuredPhase,
    pub operations: u64,
    pub elapsed_micros: u64,
    pub allocated_bytes: u64,
    pub peak_resident_bytes: u64,
    pub io_bytes: u64,
    pub database_queries: u64,
}

/// Reproducible performance result bound to one workload digest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceReport {
    pub format_version: u32,
    pub workload_digest: [u8; 32],
    pub environment: BenchmarkEnvironment,
    pub phases: Vec<PhaseMeasurement>,
}

impl PerformanceReport {
    /// Verifies environment metadata and one unique, finite measurement per phase.
    ///
    /// # Errors
    ///
    /// Returns [`PerformanceError::InvalidReport`] for incomplete or duplicate evidence.
    pub fn validate(&self) -> Result<(), PerformanceError> {
        if self.format_version != 1
            || self.workload_digest == [0; 32]
            || self.environment.cpu.trim().is_empty()
            || self.environment.ram_bytes == 0
            || self.environment.os.trim().is_empty()
            || self.environment.database.trim().is_empty()
            || self.environment.aequora_commit.trim().is_empty()
            || self.environment.durability.trim().is_empty()
            || self.phases.is_empty()
            || self.phases.len() > 9
        {
            return Err(PerformanceError::InvalidReport);
        }
        let mut phases = BTreeSet::new();
        for measurement in &self.phases {
            if measurement.operations == 0
                || measurement.elapsed_micros == 0
                || !phases.insert(measurement.phase)
            {
                return Err(PerformanceError::InvalidReport);
            }
        }
        Ok(())
    }
}

/// Noise-tolerant relative regression thresholds in basis points (`10_000` = 100%).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegressionPolicy {
    pub max_latency_increase_bps: u16,
    pub max_allocation_increase_bps: u16,
    pub max_peak_memory_increase_bps: u16,
    pub max_io_increase_bps: u16,
}

impl Default for RegressionPolicy {
    fn default() -> Self {
        Self {
            max_latency_increase_bps: 1_000,
            max_allocation_increase_bps: 1_000,
            max_peak_memory_increase_bps: 1_000,
            max_io_increase_bps: 1_000,
        }
    }
}

/// One attributed threshold failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Regression {
    pub phase: MeasuredPhase,
    pub metric: RegressionMetric,
    pub increase_bps: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegressionMetric {
    LatencyPerOperation,
    AllocationPerOperation,
    PeakResidentMemory,
    IoPerOperation,
}

/// Compares reports from the same fixed workload and attributes each regression to a phase.
///
/// # Errors
///
/// Returns a report-validation or incompatible-workload/policy failure.
pub fn compare_reports(
    baseline: &PerformanceReport,
    candidate: &PerformanceReport,
    policy: RegressionPolicy,
) -> Result<Vec<Regression>, PerformanceError> {
    baseline.validate()?;
    candidate.validate()?;
    if baseline.workload_digest != candidate.workload_digest
        || policy.max_latency_increase_bps > 10_000
        || policy.max_allocation_increase_bps > 10_000
        || policy.max_peak_memory_increase_bps > 10_000
        || policy.max_io_increase_bps > 10_000
    {
        return Err(PerformanceError::InvalidRegressionPolicy);
    }
    let baseline_by_phase = baseline
        .phases
        .iter()
        .map(|measurement| (measurement.phase, measurement))
        .collect::<BTreeMap<_, _>>();
    let mut regressions = Vec::new();
    for current in &candidate.phases {
        let Some(previous) = baseline_by_phase.get(&current.phase) else {
            continue;
        };
        check_ratio(
            &mut regressions,
            current.phase,
            RegressionMetric::LatencyPerOperation,
            previous.elapsed_micros,
            previous.operations,
            current.elapsed_micros,
            current.operations,
            u64::from(policy.max_latency_increase_bps),
        );
        check_ratio(
            &mut regressions,
            current.phase,
            RegressionMetric::AllocationPerOperation,
            previous.allocated_bytes,
            previous.operations,
            current.allocated_bytes,
            current.operations,
            u64::from(policy.max_allocation_increase_bps),
        );
        check_absolute(
            &mut regressions,
            current.phase,
            RegressionMetric::PeakResidentMemory,
            previous.peak_resident_bytes,
            current.peak_resident_bytes,
            u64::from(policy.max_peak_memory_increase_bps),
        );
        check_ratio(
            &mut regressions,
            current.phase,
            RegressionMetric::IoPerOperation,
            previous.io_bytes,
            previous.operations,
            current.io_bytes,
            current.operations,
            u64::from(policy.max_io_increase_bps),
        );
    }
    Ok(regressions)
}

#[allow(clippy::too_many_arguments)]
fn check_ratio(
    output: &mut Vec<Regression>,
    phase: MeasuredPhase,
    metric: RegressionMetric,
    old_value: u64,
    old_operations: u64,
    new_value: u64,
    new_operations: u64,
    maximum_bps: u64,
) {
    let old_scaled = u128::from(old_value).saturating_mul(u128::from(new_operations));
    let new_scaled = u128::from(new_value).saturating_mul(u128::from(old_operations));
    push_if_regressed(output, phase, metric, old_scaled, new_scaled, maximum_bps);
}

fn check_absolute(
    output: &mut Vec<Regression>,
    phase: MeasuredPhase,
    metric: RegressionMetric,
    old_value: u64,
    new_value: u64,
    maximum_bps: u64,
) {
    push_if_regressed(
        output,
        phase,
        metric,
        u128::from(old_value),
        u128::from(new_value),
        maximum_bps,
    );
}

fn push_if_regressed(
    output: &mut Vec<Regression>,
    phase: MeasuredPhase,
    metric: RegressionMetric,
    old_value: u128,
    new_value: u128,
    maximum_bps: u64,
) {
    if old_value == 0 || new_value <= old_value {
        return;
    }
    let increase = new_value.saturating_sub(old_value).saturating_mul(10_000) / old_value;
    let increase_bps = u64::try_from(increase).unwrap_or(u64::MAX);
    if increase_bps > maximum_bps {
        output.push(Regression {
            phase,
            metric,
            increase_bps,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_profile_is_bounded_and_cross_budget_safe() {
        for profile in [
            PerformanceProfile::MobileLowMemory,
            PerformanceProfile::Desktop,
            PerformanceProfile::ServerStandard,
            PerformanceProfile::ServerHighThroughput,
        ] {
            PerformancePolicy::for_profile(profile)
                .validate()
                .unwrap_or_else(|error| panic!("invalid profile: {error}"));
        }
    }

    #[test]
    fn bounded_queue_releases_item_and_byte_capacity() -> Result<(), PerformanceError> {
        let mut queue = BoundedQueue::new(2, 10)?;
        queue.try_push("one", 6)?;
        assert_eq!(queue.try_push("two", 5), Err(PerformanceError::QueueFull));
        queue.try_push("two", 4)?;
        assert_eq!(queue.try_push("three", 0), Err(PerformanceError::QueueFull));
        assert_eq!(queue.pop(), Some("one"));
        assert_eq!(queue.used_bytes(), 4);
        queue.try_push("three", 6)?;
        assert_eq!(queue.len(), 2);
        Ok(())
    }

    #[test]
    fn immutable_registry_clones_share_one_snapshot() -> Result<(), PerformanceError> {
        let registry = ImmutableRegistry::new(7, BTreeMap::from([(1_u16, "handler")]))?;
        let clone = registry.clone();
        assert_eq!(clone.generation(), 7);
        assert_eq!(clone.get(&1), Some(&"handler"));
        assert_eq!(registry.len(), 1);
        Ok(())
    }

    #[test]
    fn view_state_is_paged_and_invalidations_are_coalesced() -> Result<(), PerformanceError> {
        let mut view = PagedView::new(2)?;
        view.replace(vec![1, 2], Some(2))?;
        assert!(matches!(
            view.replace(vec![1, 2, 3], None),
            Err(PerformanceError::ViewPageTooLarge { .. })
        ));
        let mut invalidations = InvalidationCoalescer::new(2)?;
        invalidations.invalidate(1);
        invalidations.invalidate(1);
        assert_eq!(invalidations.drain(), Some(vec![1]));
        invalidations.invalidate(1);
        invalidations.invalidate(2);
        invalidations.invalidate(3);
        assert_eq!(invalidations.drain(), None);
        Ok(())
    }

    fn report(elapsed: u64, allocated: u64) -> PerformanceReport {
        PerformanceReport {
            format_version: 1,
            workload_digest: [7; 32],
            environment: BenchmarkEnvironment {
                cpu: "test-cpu".into(),
                ram_bytes: 1024,
                os: "test-os".into(),
                database: "test-db".into(),
                aequora_commit: "test-commit".into(),
                durability: "durable".into(),
            },
            phases: vec![PhaseMeasurement {
                phase: MeasuredPhase::Planning,
                operations: 100,
                elapsed_micros: elapsed,
                allocated_bytes: allocated,
                peak_resident_bytes: 100,
                io_bytes: 100,
                database_queries: 0,
            }],
        }
    }

    #[test]
    fn regression_is_attributed_to_a_measured_phase() -> Result<(), PerformanceError> {
        let regressions = compare_reports(
            &report(1_000, 1_000),
            &report(1_200, 1_000),
            RegressionPolicy::default(),
        )?;
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].phase, MeasuredPhase::Planning);
        assert_eq!(regressions[0].metric, RegressionMetric::LatencyPerOperation);
        assert_eq!(regressions[0].increase_bps, 2_000);
        Ok(())
    }

    #[test]
    fn workload_manifest_is_fixed_seed_and_digest_bound() -> Result<(), PerformanceError> {
        let manifest = WorkloadManifest {
            format_version: 1,
            name: "planner-10k".into(),
            profile: WorkloadProfile::SchoolDayMorning,
            benchmark_kind: BenchmarkKind::Component,
            seed: 42,
            tenants: 10,
            clients_per_tenant: 10,
            operations: 10_000,
            payload_bytes_p50: 256,
            payload_bytes_p99: 4_096,
            max_dependency_edges: 20_000,
            max_runtime_seconds: 60,
            durable: true,
        };
        assert_eq!(manifest.digest()?, manifest.digest()?);
        assert_eq!(PERFORMANCE_INVARIANTS.len(), 9);
        Ok(())
    }

    #[test]
    fn repository_workload_manifests_are_valid_and_distinct() {
        let inputs = [
            include_str!("../../../performance/workloads/school-day-morning.ron"),
            include_str!("../../../performance/workloads/mass-reconnect.ron"),
            include_str!("../../../performance/workloads/large-bootstrap.ron"),
            include_str!("../../../performance/workloads/low-bandwidth-mobile.ron"),
        ];
        let mut digests = BTreeSet::new();
        for input in inputs {
            let manifest: WorkloadManifest =
                ron::from_str(input).unwrap_or_else(|error| panic!("{error}"));
            let digest = manifest.digest().unwrap_or_else(|error| panic!("{error}"));
            assert!(digests.insert(digest));
        }
    }
}
