//! Runtime-neutral overload protection for every expensive Aequora resource domain.
//!
//! Admission state is deliberately ephemeral. Required intent must remain in a durable client
//! outbox, server job table, or side-effect outbox; bounded queues in this crate are scheduling
//! aids and never a durable source of truth.

mod brownout;
mod controller;
mod errors;
mod fairness;
mod limiter;
mod load_state;
mod permit;
mod work;

pub use brownout::BrownoutPolicy;
pub use controller::{
    AdmissionController, AdmissionMetricsSnapshot, AdmissionPolicy, ClassBudget,
    HierarchicalAdmission, TenantAdmissionPolicy,
};
pub use errors::{AdmissionRejection, PolicyError, QueueRejection};
pub use fairness::{FairQueue, FairQueueConfig, QueuedWork};
pub use limiter::{RateLimit, TokenBucket};
pub use load_state::{LoadPolicy, LoadSignals, LoadState, LoadStateTracker, LoadThresholds};
pub use permit::AdmissionPermit;
pub use work::{
    CostUnits, PriorityRule, RequestLimits, RequestShape, ResourceBudget, ResourceDomain,
    ServerPriorityPolicy, WorkDescriptor,
};
