//! Platform-neutral resource policy for constrained Aequora clients.
//!
//! The policy in this module may reduce or defer bounded work. It never owns authoritative
//! validity, idempotency, authorization, cursor commits, or durable outbox semantics.

mod admission;
mod context;
mod events;
mod memory;
mod network;
mod power;
mod profile;
mod storage;
mod thermal;

pub use admission::{
    AdmissionLimits, ClientResourceAdmission, ClientWork, ClientWorkKind, CompressionPreference,
    CpuClass, DefaultResourceAdmission, ResourceDecision, ResourceReason, WorkEstimate,
};
pub use context::{
    BackgroundBudget, ClientResourceContext, MemoryClass, NetworkContext, PlatformResourceMonitor,
    PowerContext, StorageState, ThermalState,
};
pub use events::{
    AppLifecycleEvent, CheckpointError, ClientResourceLogEvent, ClientResourceMetrics,
    ClientStatus, ClientUserDiagnostics, DiagnosticRing, DurableWorkCheckpoint, LifecycleActions,
    ResourceEvent, ResourceEventCoalescer, RetryCheckpoint,
};
pub use memory::ClientMemoryLimits;
pub use network::{
    ClientNetworkPolicy, DataBudget, DataBudgetUsage, NetworkAdmission, NetworkWorkClass,
};
pub use power::ClientPowerPolicy;
pub use profile::{
    ClientCapabilityClass, ClientCapabilityProfile, ClientResourcePolicy, ClientResourceProfile,
    PolicyError,
};
pub use storage::{
    CacheRetention, EvictionCandidate, EvictionPlan, EvictionReason, LocalCommitReceipt,
    LocalCommitReceiptError, LocalStoreFormatVersion, ScopeCachePolicy, SnapshotCachePolicy,
    StorageAssetKind, StoragePolicy, StoragePreflight, StoragePreflightDecision, StoreOpenDecision,
};
pub use thermal::ClientThermalPolicy;
