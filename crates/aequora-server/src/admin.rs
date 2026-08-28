//! Thin server integration for the separately authenticated admin listener.
//!
//! Transport handlers authenticate and decode before calling this boundary. Authorization and
//! all mutation safety remain in `aequora-admin`; this module deliberately has no data-plane
//! dependency and exposes no raw metadata mutation facility.

use aequora_admin::{
    AdminAction, AdminAuditSink, AdminCommand, AdminCommandExecutor, AdminError,
    AdminListenerPolicy, AdminOperationId, AdminOperationRecord, AdminPlan, AdminPrincipal,
    AdminService, AdminStore,
};
use std::sync::Arc;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AdminPlaneConfig {
    pub listener: AdminListenerPolicy,
}

impl AdminPlaneConfig {
    /// Validates fail-closed listener security settings.
    ///
    /// # Errors
    ///
    /// Returns an admin policy error when authentication, binding, or browser security is unsafe.
    pub fn validate(&self) -> Result<(), AdminError> {
        self.listener.validate()
    }
}

pub struct AdminControlPlane<S, E, A> {
    config: AdminPlaneConfig,
    service: AdminService<S, E, A>,
}

impl<S, E, A> AdminControlPlane<S, E, A>
where
    S: AdminStore,
    E: AdminCommandExecutor,
    A: AdminAuditSink,
{
    /// Constructs the admin boundary only after listener policy validation.
    ///
    /// # Errors
    ///
    /// Returns an admin policy error when the listener configuration does not fail closed.
    pub fn new(
        config: AdminPlaneConfig,
        store: Arc<S>,
        executor: Arc<E>,
        audit: Arc<A>,
    ) -> Result<Self, AdminError> {
        config.validate()?;
        Ok(Self {
            config,
            service: AdminService::new(store, executor, audit),
        })
    }

    #[must_use]
    pub const fn config(&self) -> &AdminPlaneConfig {
        &self.config
    }

    /// Creates a bounded plan tied to the executor's current safety snapshot.
    ///
    /// # Errors
    ///
    /// Returns an authorization, storage, or subsystem-snapshot error.
    pub fn plan(
        &self,
        actor: &AdminPrincipal,
        command: &AdminCommand,
        now_unix_ms: u64,
    ) -> Result<AdminPlan, AdminError> {
        self.service.plan(actor, command, now_unix_ms)
    }

    /// Submits one typed, authenticated, idempotent admin action.
    ///
    /// # Errors
    ///
    /// Returns an authorization, plan, approval, store, execution, or audit error.
    pub fn submit(
        &self,
        action: AdminAction,
        now_unix_ms: u64,
    ) -> Result<AdminOperationRecord, AdminError> {
        self.service.submit(action, now_unix_ms)
    }

    /// Approves and executes an exact pending action under separation of duties.
    ///
    /// # Errors
    ///
    /// Returns an authorization, expiry, digest, separation, execution, or audit error.
    pub fn approve(
        &self,
        operation_id: AdminOperationId,
        approver: &AdminPrincipal,
        now_unix_ms: u64,
    ) -> Result<AdminOperationRecord, AdminError> {
        self.service.approve(operation_id, approver, now_unix_ms)
    }
}
