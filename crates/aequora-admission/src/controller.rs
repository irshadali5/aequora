use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use aequora_scheduler::WorkClass;
use aequora_types::TenantId;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    AdmissionPermit, AdmissionRejection, FairQueueConfig, LoadPolicy, LoadState, PolicyError,
    RateLimit, RequestLimits, ResourceBudget, ResourceDomain, WorkDescriptor,
};

const CLASSES: [WorkClass; 6] = [
    WorkClass::Critical,
    WorkClass::Interactive,
    WorkClass::Normal,
    WorkClass::Bulk,
    WorkClass::Background,
    WorkClass::Maintenance,
];

const RESOURCE_DOMAINS: [ResourceDomain; 11] = [
    ResourceDomain::HttpRequest,
    ResourceDomain::SyncExchange,
    ResourceDomain::DatabaseTransaction,
    ResourceDomain::InteractiveCpu,
    ResourceDomain::BulkCpu,
    ResourceDomain::MaintenanceCpu,
    ResourceDomain::SnapshotBuild,
    ResourceDomain::SnapshotDownload,
    ResourceDomain::BlobTransfer,
    ResourceDomain::LiveConnection,
    ResourceDomain::BackgroundJob,
];

/// One work-class ceiling plus capacity unavailable to other classes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClassBudget {
    pub budget: ResourceBudget,
    pub reserved_in_flight: usize,
}

impl Default for ClassBudget {
    fn default() -> Self {
        Self {
            budget: ResourceBudget::default(),
            reserved_in_flight: 1,
        }
    }
}

/// Default and weighted per-tenant ceilings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TenantAdmissionPolicy {
    pub budget: ResourceBudget,
    pub maximum_weight: u16,
    pub max_tracked_tenants: usize,
}

impl Default for TenantAdmissionPolicy {
    fn default() -> Self {
        Self {
            budget: ResourceBudget {
                max_in_flight: 40,
                max_queue: 64,
                max_bytes_in_flight: 64 * 1_024 * 1_024,
                max_cost_units_in_flight: 25_000,
            },
            maximum_weight: 8,
            max_tracked_tenants: 4_096,
        }
    }
}

/// Complete deterministic admission policy with conservative static defaults.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdmissionPolicy {
    pub global: ResourceBudget,
    pub tenant: TenantAdmissionPolicy,
    pub critical: ClassBudget,
    pub interactive: ClassBudget,
    pub normal: ClassBudget,
    pub bulk: ClassBudget,
    pub background: ClassBudget,
    pub maintenance: ClassBudget,
    pub resources: BTreeMap<ResourceDomain, ResourceBudget>,
    pub queue: FairQueueConfig,
    pub connection_rate: RateLimit,
    pub tenant_request_rate: RateLimit,
    pub request_limits: RequestLimits,
    pub load: LoadPolicy,
    pub retry_after_ms: u64,
}

impl Default for AdmissionPolicy {
    fn default() -> Self {
        let mut resources = BTreeMap::new();
        resources.insert(ResourceDomain::HttpRequest, budget(256, 512, 256, 100_000));
        resources.insert(ResourceDomain::SyncExchange, budget(200, 500, 256, 100_000));
        resources.insert(
            ResourceDomain::DatabaseTransaction,
            budget(100, 200, 128, 50_000),
        );
        resources.insert(ResourceDomain::InteractiveCpu, budget(16, 64, 32, 20_000));
        resources.insert(ResourceDomain::BulkCpu, budget(4, 16, 64, 20_000));
        resources.insert(ResourceDomain::MaintenanceCpu, budget(2, 8, 32, 10_000));
        resources.insert(ResourceDomain::SnapshotBuild, budget(2, 8, 128, 20_000));
        resources.insert(
            ResourceDomain::SnapshotDownload,
            budget(32, 128, 512, 40_000),
        );
        resources.insert(ResourceDomain::BlobTransfer, budget(32, 128, 512, 40_000));
        resources.insert(
            ResourceDomain::LiveConnection,
            budget(1_000, 2_000, 64, 10_000),
        );
        resources.insert(ResourceDomain::BackgroundJob, budget(16, 256, 128, 30_000));
        Self {
            global: ResourceBudget::default(),
            tenant: TenantAdmissionPolicy::default(),
            critical: class_budget(64, 16),
            interactive: class_budget(128, 32),
            normal: class_budget(192, 8),
            bulk: class_budget(32, 4),
            background: class_budget(16, 2),
            maintenance: class_budget(8, 1),
            resources,
            queue: FairQueueConfig::default(),
            connection_rate: RateLimit::default(),
            tenant_request_rate: RateLimit {
                tokens_per_second: 64,
                burst: 128,
            },
            request_limits: RequestLimits::default(),
            load: LoadPolicy::default(),
            retry_after_ms: 1_000,
        }
    }
}

const fn budget(in_flight: usize, queue: usize, mebibytes: u64, cost: u64) -> ResourceBudget {
    ResourceBudget {
        max_in_flight: in_flight,
        max_queue: queue,
        max_bytes_in_flight: mebibytes * 1_024 * 1_024,
        max_cost_units_in_flight: cost,
    }
}

const fn class_budget(max: usize, reserved: usize) -> ClassBudget {
    ClassBudget {
        budget: ResourceBudget {
            max_in_flight: max,
            max_queue: max,
            max_bytes_in_flight: 256 * 1_024 * 1_024,
            max_cost_units_in_flight: 100_000,
        },
        reserved_in_flight: reserved,
    }
}

impl AdmissionPolicy {
    /// Validates cross-domain, class, tenant, queue, request, and load bounds.
    ///
    /// # Errors
    ///
    /// Returns a [`PolicyError`] when a bound is missing, zero, or inconsistent.
    pub fn validate(&self) -> Result<(), PolicyError> {
        self.global.validate()?;
        self.tenant.budget.validate()?;
        self.request_limits.validate()?;
        self.queue.validate()?;
        self.connection_rate.validate()?;
        self.tenant_request_rate.validate()?;
        self.load.validate()?;
        if self.retry_after_ms == 0
            || self.tenant.maximum_weight == 0
            || self.tenant.budget.max_in_flight > self.global.max_in_flight
            || self.tenant.max_tracked_tenants < self.global.max_in_flight
        {
            return Err(PolicyError::InvalidTenantPolicy);
        }
        let mut reserved = 0usize;
        for class in CLASSES {
            let class_budget = self.class(class);
            class_budget.budget.validate()?;
            if class_budget.reserved_in_flight > class_budget.budget.max_in_flight {
                return Err(PolicyError::InvalidClassPolicy);
            }
            reserved = reserved.saturating_add(class_budget.reserved_in_flight);
        }
        if reserved > self.global.max_in_flight {
            return Err(PolicyError::InvalidClassPolicy);
        }
        for domain in RESOURCE_DOMAINS {
            self.resources
                .get(&domain)
                .ok_or(PolicyError::MissingResourceBudget)?
                .validate()?;
        }
        Ok(())
    }

    #[must_use]
    pub const fn class(&self, class: WorkClass) -> ClassBudget {
        match class {
            WorkClass::Critical => self.critical,
            WorkClass::Interactive => self.interactive,
            WorkClass::Normal => self.normal,
            WorkClass::Bulk => self.bulk,
            WorkClass::Background => self.background,
            WorkClass::Maintenance => self.maintenance,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Usage {
    in_flight: usize,
    bytes: u64,
    cost: u64,
}

#[derive(Debug)]
struct AdmissionState {
    global: Usage,
    tenants: BTreeMap<TenantId, Usage>,
    classes: BTreeMap<WorkClass, Usage>,
    resources: BTreeMap<ResourceDomain, Usage>,
    load_state: LoadState,
    allowed_total: u64,
    rejected_total: [u64; 8],
    rejected_by_class: [u64; 6],
}

impl Default for AdmissionState {
    fn default() -> Self {
        Self {
            global: Usage::default(),
            tenants: BTreeMap::new(),
            classes: BTreeMap::new(),
            resources: BTreeMap::new(),
            load_state: LoadState::Healthy,
            allowed_total: 0,
            rejected_total: [0; 8],
            rejected_by_class: [0; 6],
        }
    }
}

pub(crate) struct ControllerInner {
    policy: AdmissionPolicy,
    state: Mutex<AdmissionState>,
}

impl ControllerInner {
    fn state(&self) -> MutexGuard<'_, AdmissionState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn release(
        &self,
        tenant_id: TenantId,
        class: WorkClass,
        bytes: u64,
        cost: u64,
        resources: &[ResourceDomain],
    ) {
        let mut state = self.state();
        subtract_usage(&mut state.global, bytes, cost);
        if let Some(usage) = state.tenants.get_mut(&tenant_id) {
            subtract_usage(usage, bytes, cost);
            if usage.in_flight == 0 {
                state.tenants.remove(&tenant_id);
            }
        }
        if let Some(usage) = state.classes.get_mut(&class) {
            subtract_usage(usage, bytes, cost);
        }
        for domain in resources {
            if let Some(usage) = state.resources.get_mut(domain) {
                subtract_usage(usage, bytes, cost);
            }
        }
    }
}

fn subtract_usage(usage: &mut Usage, bytes: u64, cost: u64) {
    usage.in_flight = usage.in_flight.saturating_sub(1);
    usage.bytes = usage.bytes.saturating_sub(bytes);
    usage.cost = usage.cost.saturating_sub(cost);
}

fn add_usage(usage: &mut Usage, bytes: u64, cost: u64) {
    usage.in_flight = usage.in_flight.saturating_add(1);
    usage.bytes = usage.bytes.saturating_add(bytes);
    usage.cost = usage.cost.saturating_add(cost);
}

/// Object-safe admission boundary. Implementations reject before protected work begins.
#[async_trait]
pub trait AdmissionController: Send + Sync {
    async fn admit(&self, request: &WorkDescriptor) -> Result<AdmissionPermit, AdmissionRejection>;
}

/// Lock-atomic global, tenant, class, and resource admission with RAII release.
#[derive(Clone)]
pub struct HierarchicalAdmission {
    inner: Arc<ControllerInner>,
}

impl HierarchicalAdmission {
    /// Creates an admission controller from a fully validated policy.
    ///
    /// # Errors
    ///
    /// Returns a [`PolicyError`] when the supplied policy is unsafe.
    pub fn new(policy: AdmissionPolicy) -> Result<Self, PolicyError> {
        policy.validate()?;
        Ok(Self {
            inner: Arc::new(ControllerInner {
                policy,
                state: Mutex::new(AdmissionState::default()),
            }),
        })
    }

    #[must_use]
    pub fn policy(&self) -> &AdmissionPolicy {
        &self.inner.policy
    }

    /// Installs the state derived by [`crate::LoadStateTracker`].
    pub fn set_load_state(&self, load_state: LoadState) {
        self.inner.state().load_state = load_state;
    }

    /// Synchronous fast path for middleware and deterministic tests.
    ///
    /// # Errors
    ///
    /// Returns a typed [`AdmissionRejection`] before any counter changes when capacity, request
    /// shape, load state, or resource policy refuses the work.
    #[allow(clippy::too_many_lines)]
    pub fn try_admit(
        &self,
        request: &WorkDescriptor,
    ) -> Result<AdmissionPermit, AdmissionRejection> {
        if let Err(rejection) = self.inner.policy.request_limits.check(request.shape) {
            self.record_rejection(rejection, request.class);
            return Err(rejection);
        }
        let bytes = request.estimated_bytes();
        let cost = u64::from(request.cost.get());
        let resources: Vec<_> = request
            .resources
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut state = self.inner.state();
        let retry_after_ms = self.inner.policy.retry_after_ms;
        if !self
            .inner
            .policy
            .load
            .allows(state.load_state, request.class)
        {
            let rejection = AdmissionRejection::LoadShed { retry_after_ms };
            record_rejection_locked(&mut state, rejection, request.class);
            return Err(rejection);
        }

        let capacity_percent = self.inner.policy.load.capacity_percent(state.load_state);
        let global_limit = scaled_limit(self.inner.policy.global.max_in_flight, capacity_percent);
        let unused_other_reservations = CLASSES
            .into_iter()
            .filter(|class| *class != request.class)
            .map(|class| {
                let reserved = self.inner.policy.class(class).reserved_in_flight;
                let used = state.classes.get(&class).map_or(0, |usage| usage.in_flight);
                reserved.saturating_sub(used)
            })
            .sum::<usize>();
        let class_aware_global_limit = global_limit.saturating_sub(unused_other_reservations);
        if state.global.in_flight >= class_aware_global_limit
            || !fits(state.global, self.inner.policy.global, bytes, cost)
        {
            let rejection = AdmissionRejection::ServerBusy { retry_after_ms };
            record_rejection_locked(&mut state, rejection, request.class);
            return Err(rejection);
        }

        let weight = usize::from(
            request
                .tenant_weight
                .max(1)
                .min(self.inner.policy.tenant.maximum_weight),
        );
        let tenant_budget = weighted_tenant_budget(
            self.inner.policy.tenant.budget,
            self.inner.policy.global,
            weight,
        );
        let tenant_usage = state
            .tenants
            .get(&request.tenant_id)
            .copied()
            .unwrap_or_default();
        if !fits(tenant_usage, tenant_budget, bytes, cost) {
            let rejection = AdmissionRejection::TenantBusy { retry_after_ms };
            record_rejection_locked(&mut state, rejection, request.class);
            return Err(rejection);
        }
        if !state.tenants.contains_key(&request.tenant_id)
            && state.tenants.len() >= self.inner.policy.tenant.max_tracked_tenants
        {
            let rejection = AdmissionRejection::TenantBusy { retry_after_ms };
            record_rejection_locked(&mut state, rejection, request.class);
            return Err(rejection);
        }

        let class_usage = state
            .classes
            .get(&request.class)
            .copied()
            .unwrap_or_default();
        if !fits(
            class_usage,
            self.inner.policy.class(request.class).budget,
            bytes,
            cost,
        ) {
            let rejection = AdmissionRejection::ResourceBudgetExceeded { retry_after_ms };
            record_rejection_locked(&mut state, rejection, request.class);
            return Err(rejection);
        }
        for domain in &resources {
            let usage = state.resources.get(domain).copied().unwrap_or_default();
            let Some(budget) = self.inner.policy.resources.get(domain).copied() else {
                let rejection = AdmissionRejection::ResourceBudgetExceeded { retry_after_ms };
                record_rejection_locked(&mut state, rejection, request.class);
                return Err(rejection);
            };
            if !fits(usage, budget, bytes, cost) {
                let rejection = AdmissionRejection::ResourceBudgetExceeded { retry_after_ms };
                record_rejection_locked(&mut state, rejection, request.class);
                return Err(rejection);
            }
        }

        add_usage(&mut state.global, bytes, cost);
        add_usage(
            state.tenants.entry(request.tenant_id).or_default(),
            bytes,
            cost,
        );
        add_usage(state.classes.entry(request.class).or_default(), bytes, cost);
        for domain in &resources {
            add_usage(state.resources.entry(*domain).or_default(), bytes, cost);
        }
        state.allowed_total = state.allowed_total.saturating_add(1);
        drop(state);
        Ok(AdmissionPermit::new(
            Arc::clone(&self.inner),
            request.tenant_id,
            request.class,
            bytes,
            cost,
            resources,
        ))
    }

    fn record_rejection(&self, rejection: AdmissionRejection, class: WorkClass) {
        record_rejection_locked(&mut self.inner.state(), rejection, class);
    }

    #[must_use]
    pub fn snapshot(&self) -> AdmissionMetricsSnapshot {
        let state = self.inner.state();
        let mut class_in_flight = [0; 6];
        for (class, usage) in &state.classes {
            class_in_flight[class_index(*class)] = usage.in_flight;
        }
        AdmissionMetricsSnapshot {
            load_state: state.load_state,
            global_in_flight: state.global.in_flight,
            bytes_in_flight: state.global.bytes,
            cost_units_in_flight: state.global.cost,
            tracked_tenants: state.tenants.len(),
            class_in_flight,
            resource_in_flight: state
                .resources
                .iter()
                .map(|(domain, usage)| (*domain, usage.in_flight))
                .collect(),
            allowed_total: state.allowed_total,
            rejected_total: state.rejected_total,
            rejected_by_class: state.rejected_by_class,
        }
    }
}

#[async_trait]
impl AdmissionController for HierarchicalAdmission {
    async fn admit(&self, request: &WorkDescriptor) -> Result<AdmissionPermit, AdmissionRejection> {
        self.try_admit(request)
    }
}

fn weighted_tenant_budget(
    tenant: ResourceBudget,
    global: ResourceBudget,
    weight: usize,
) -> ResourceBudget {
    let weight_u64 = u64::try_from(weight).unwrap_or(u64::MAX);
    ResourceBudget {
        max_in_flight: tenant
            .max_in_flight
            .saturating_mul(weight)
            .min(global.max_in_flight),
        max_queue: tenant
            .max_queue
            .saturating_mul(weight)
            .min(global.max_queue),
        max_bytes_in_flight: tenant
            .max_bytes_in_flight
            .saturating_mul(weight_u64)
            .min(global.max_bytes_in_flight),
        max_cost_units_in_flight: tenant
            .max_cost_units_in_flight
            .saturating_mul(weight_u64)
            .min(global.max_cost_units_in_flight),
    }
}

const fn scaled_limit(limit: usize, percent: u8) -> usize {
    let scaled = limit.saturating_mul(percent as usize).div_ceil(100);
    if scaled == 0 { 1 } else { scaled }
}

fn fits(current: Usage, budget: ResourceBudget, bytes: u64, cost: u64) -> bool {
    current.in_flight < budget.max_in_flight
        && current.bytes.saturating_add(bytes) <= budget.max_bytes_in_flight
        && current.cost.saturating_add(cost) <= budget.max_cost_units_in_flight
}

fn record_rejection_locked(
    state: &mut AdmissionState,
    rejection: AdmissionRejection,
    class: WorkClass,
) {
    let index = match rejection {
        AdmissionRejection::ServerBusy { .. } => 0,
        AdmissionRejection::TenantBusy { .. } => 1,
        AdmissionRejection::RateLimited { .. } => 2,
        AdmissionRejection::QueueFull { .. } => 3,
        AdmissionRejection::ResourceBudgetExceeded { .. } => 4,
        AdmissionRejection::RequestTooLarge => 5,
        AdmissionRejection::TooManyDependencies => 6,
        AdmissionRejection::LoadShed { .. } | AdmissionRejection::Brownout { .. } => 7,
    };
    state.rejected_total[index] = state.rejected_total[index].saturating_add(1);
    let class_index = class_index(class);
    state.rejected_by_class[class_index] = state.rejected_by_class[class_index].saturating_add(1);
}

const fn class_index(class: WorkClass) -> usize {
    match class {
        WorkClass::Critical => 0,
        WorkClass::Interactive => 1,
        WorkClass::Normal => 2,
        WorkClass::Bulk => 3,
        WorkClass::Background => 4,
        WorkClass::Maintenance => 5,
    }
}

/// Payload-free, bounded-cardinality operational view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmissionMetricsSnapshot {
    pub load_state: LoadState,
    pub global_in_flight: usize,
    pub bytes_in_flight: u64,
    pub cost_units_in_flight: u64,
    pub tracked_tenants: usize,
    pub class_in_flight: [usize; 6],
    pub resource_in_flight: BTreeMap<ResourceDomain, usize>,
    pub allowed_total: u64,
    pub rejected_total: [u64; 8],
    pub rejected_by_class: [u64; 6],
}

#[cfg(test)]
mod tests {
    use aequora_scheduler::WorkKind;

    use super::*;
    use crate::{CostUnits, RequestShape};

    fn work(tenant_id: TenantId, class: WorkClass) -> WorkDescriptor {
        WorkDescriptor {
            tenant_id,
            tenant_weight: 1,
            kind: WorkKind::PushOperations,
            class,
            cost: CostUnits::new(1),
            shape: RequestShape {
                operations: 1,
                encoded_bytes: 1,
                decompressed_bytes: 1,
                ..RequestShape::default()
            },
            scope: None,
            estimated_response_bytes: 1,
            resources: vec![
                ResourceDomain::SyncExchange,
                ResourceDomain::DatabaseTransaction,
            ],
        }
    }

    fn small_policy() -> AdmissionPolicy {
        let mut policy = AdmissionPolicy::default();
        policy.global.max_in_flight = 4;
        policy.tenant.budget.max_in_flight = 2;
        policy.tenant.max_tracked_tenants = 4;
        policy.critical.reserved_in_flight = 0;
        policy.interactive.reserved_in_flight = 1;
        policy.normal.reserved_in_flight = 0;
        policy.bulk.reserved_in_flight = 0;
        policy.background.reserved_in_flight = 0;
        policy.maintenance.reserved_in_flight = 0;
        policy
    }

    #[test]
    fn hot_tenant_cannot_consume_other_tenant_capacity() {
        let controller =
            HierarchicalAdmission::new(small_policy()).unwrap_or_else(|error| panic!("{error}"));
        let noisy = TenantId::new();
        let quiet = TenantId::new();
        let _first = controller
            .try_admit(&work(noisy, WorkClass::Normal))
            .unwrap_or_else(|error| panic!("{error}"));
        let _second = controller
            .try_admit(&work(noisy, WorkClass::Normal))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            controller.try_admit(&work(noisy, WorkClass::Normal)),
            Err(AdmissionRejection::TenantBusy { .. })
        ));
        let _quiet = controller
            .try_admit(&work(quiet, WorkClass::Normal))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(controller.snapshot().global_in_flight, 3);
    }

    #[test]
    fn reserved_interactive_capacity_survives_bulk_saturation() {
        let controller =
            HierarchicalAdmission::new(small_policy()).unwrap_or_else(|error| panic!("{error}"));
        let tenants = [TenantId::new(), TenantId::new(), TenantId::new()];
        let permits: Vec<_> = tenants
            .into_iter()
            .map(|tenant| {
                controller
                    .try_admit(&work(tenant, WorkClass::Bulk))
                    .unwrap_or_else(|error| panic!("{error}"))
            })
            .collect();
        assert!(matches!(
            controller.try_admit(&work(TenantId::new(), WorkClass::Bulk)),
            Err(AdmissionRejection::ServerBusy { .. })
        ));
        let _interactive = controller
            .try_admit(&work(TenantId::new(), WorkClass::Interactive))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(controller.snapshot().global_in_flight, 4);
        drop(permits);
    }

    #[test]
    fn permit_drop_releases_every_hierarchical_counter() {
        let controller =
            HierarchicalAdmission::new(small_policy()).unwrap_or_else(|error| panic!("{error}"));
        let tenant = TenantId::new();
        let permit = controller
            .try_admit(&work(tenant, WorkClass::Normal))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(controller.snapshot().global_in_flight, 1);
        drop(permit);
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.global_in_flight, 0);
        assert_eq!(snapshot.tracked_tenants, 0);
        assert!(
            snapshot
                .resource_in_flight
                .values()
                .all(|count| *count == 0)
        );
    }

    #[test]
    fn reconnect_storm_stays_bounded_and_recovers() {
        let controller =
            HierarchicalAdmission::new(small_policy()).unwrap_or_else(|error| panic!("{error}"));
        let tenants = [TenantId::new(), TenantId::new()];
        let mut permits = Vec::new();
        let mut rejected = 0usize;
        for attempt in 0..100_000 {
            let request = work(tenants[attempt % tenants.len()], WorkClass::Interactive);
            match controller.try_admit(&request) {
                Ok(permit) => permits.push(permit),
                Err(_) => rejected += 1,
            }
        }
        assert_eq!(permits.len(), 4);
        assert_eq!(rejected, 99_996);
        assert_eq!(controller.snapshot().global_in_flight, 4);
        drop(permits);
        assert_eq!(controller.snapshot().global_in_flight, 0);
    }

    #[test]
    fn overload_sheds_bulk_before_interactive() {
        let controller =
            HierarchicalAdmission::new(small_policy()).unwrap_or_else(|error| panic!("{error}"));
        controller.set_load_state(LoadState::Overloaded);
        assert!(matches!(
            controller.try_admit(&work(TenantId::new(), WorkClass::Bulk)),
            Err(AdmissionRejection::LoadShed { .. })
        ));
        let _permit = controller
            .try_admit(&work(TenantId::new(), WorkClass::Interactive))
            .unwrap_or_else(|error| panic!("{error}"));
    }
}
