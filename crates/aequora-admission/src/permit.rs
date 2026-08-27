use std::{fmt, sync::Arc};

use aequora_scheduler::WorkClass;
use aequora_types::TenantId;

use crate::{ResourceDomain, controller::ControllerInner};

/// RAII ownership of all counters reserved by one admitted work item.
pub struct AdmissionPermit {
    inner: Arc<ControllerInner>,
    tenant_id: TenantId,
    class: WorkClass,
    bytes: u64,
    cost: u64,
    resources: Vec<ResourceDomain>,
}

impl AdmissionPermit {
    pub(crate) fn new(
        inner: Arc<ControllerInner>,
        tenant_id: TenantId,
        class: WorkClass,
        bytes: u64,
        cost: u64,
        resources: Vec<ResourceDomain>,
    ) -> Self {
        Self {
            inner,
            tenant_id,
            class,
            bytes,
            cost,
            resources,
        }
    }
}

impl fmt::Debug for AdmissionPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmissionPermit")
            .field("class", &self.class)
            .field("bytes", &self.bytes)
            .field("cost", &self.cost)
            .field("resources", &self.resources)
            .finish_non_exhaustive()
    }
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        self.inner.release(
            self.tenant_id,
            self.class,
            self.bytes,
            self.cost,
            &self.resources,
        );
    }
}
