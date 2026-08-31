//! Platform-neutral Android and iOS lifecycle, resource, and recovery contracts.
//!
//! This crate deliberately contains no JNI, Apple framework, networking, async runtime, or
//! database dependency. Platform hosts report execution opportunities; durable Aequora storage
//! and synchronization implementations execute the resulting bounded work plan.

mod budgets;
mod context;
mod coordinator;
mod events;
mod platform;
mod policies;
mod recovery;

pub use budgets::{BudgetError, MobileMemoryBudget, MobileSyncBudget, SyncConsumption};
pub use context::{
    AppLifecycle, BatteryClass, MobileResourceContext, NetworkAvailability, NetworkConstraint,
    NetworkContext, NetworkCost, PowerContext, RoamingState, StoragePressure, ThermalState,
};
pub use coordinator::{
    Completion, CoordinatorError, CoordinatorState, MobileCoordinator, MobilePlatformEvent,
    SyncGrant, SyncTrigger,
};
pub use events::{MobileEvent, MobileStatus};
pub use platform::{
    AppLifecycleSource, BackgroundExecutionHost, BackgroundRequest, CredentialProvider,
    NetworkMonitor, PlatformError, PowerMonitor, PushHint, PushHintReason, PushHintSource,
    SecretBytes, SecureKeyHandle, SecureStore,
};
pub use policies::{MobilePolicy, PolicyDecision, ScopeCachePolicy, SyncClass, TransferKind};
pub use recovery::{
    MobileRecoveryStore, RecoveryAction, RecoveryError, RecoverySnapshot, StoreOpenDisposition,
    StoreVersions, classify_startup,
};

/// Stable native binding contract understood by Kotlin and Swift wrappers.
pub const AEQUORA_MOBILE_ABI_VERSION: u32 = 1;

/// Normative mobile invariants implemented by this boundary.
pub const MOBILE_INVARIANT_IDS: [&str; 9] = [
    "AEQ-INV-MOBILE001",
    "AEQ-INV-MOBILE002",
    "AEQ-INV-MOBILE003",
    "AEQ-INV-MOBILE004",
    "AEQ-INV-MOBILE005",
    "AEQ-INV-MOBILE006",
    "AEQ-INV-MOBILE007",
    "AEQ-INV-MOBILE008",
    "AEQ-INV-MOBILE009",
];
