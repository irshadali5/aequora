//! Database-, runtime-, transport-, and UI-neutral desktop integration contracts.
//!
//! Hosts translate operating-system signals into these values. Durable adapters remain
//! responsible for atomically persisting coordinator leases, outbox entries, and cursors.

use aequora_coordination::{
    CoordinationSnapshot, FencingToken, LeaseGrant, LocalStoreGeneration, LocalStoreId,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path};
use thiserror::Error;
use zeroize::Zeroize;

pub const DESKTOP_INVARIANT_IDS: [&str; 9] = [
    "AEQ-INV-DESKTOP001",
    "AEQ-INV-DESKTOP002",
    "AEQ-INV-DESKTOP003",
    "AEQ-INV-DESKTOP004",
    "AEQ-INV-DESKTOP005",
    "AEQ-INV-DESKTOP006",
    "AEQ-INV-DESKTOP007",
    "AEQ-INV-DESKTOP008",
    "AEQ-INV-DESKTOP009",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RuntimeMode {
    InProcess,
    Agent,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DesktopLifecycle {
    Active,
    Idle,
    Sleeping,
    SessionLocked,
    SessionUnlocked,
    Terminating,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum NetworkAvailability {
    #[default]
    Unavailable,
    Degraded,
    Available,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum NetworkCost {
    #[default]
    Unmetered,
    Metered,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum NetworkPath {
    #[default]
    Direct,
    Vpn,
    Proxy,
    CaptivePortal,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct NetworkContext {
    pub availability: NetworkAvailability,
    pub cost: NetworkCost,
    pub path: NetworkPath,
    pub generation: u64,
}

impl NetworkContext {
    #[must_use]
    pub const fn usable(self) -> bool {
        matches!(self.availability, NetworkAvailability::Available)
            && !matches!(self.path, NetworkPath::CaptivePortal)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ThermalState {
    #[default]
    Nominal,
    Elevated,
    Critical,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PowerContext {
    pub charging: bool,
    pub low_power: bool,
    pub thermal: ThermalState,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum StoragePressure {
    #[default]
    Healthy,
    Low,
    Critical,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WorkClass {
    Interactive,
    Background,
    Bulk,
    Maintenance,
    SecurityCritical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkDecision {
    Run,
    WaitForNetwork,
    DeferForPower,
    DeferForStorage,
    RejectNotDurable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DesktopPolicy {
    pub background_sync: bool,
    pub sync_on_resume: bool,
    pub maintenance_on_ac_power: bool,
}

impl Default for DesktopPolicy {
    fn default() -> Self {
        Self {
            background_sync: true,
            sync_on_resume: true,
            maintenance_on_ac_power: true,
        }
    }
}

impl DesktopPolicy {
    #[must_use]
    pub const fn decide(
        self,
        class: WorkClass,
        network: NetworkContext,
        power: PowerContext,
        storage: StoragePressure,
    ) -> WorkDecision {
        if matches!(
            storage,
            StoragePressure::Unavailable | StoragePressure::Critical
        ) {
            return if matches!(class, WorkClass::Interactive) {
                WorkDecision::RejectNotDurable
            } else {
                WorkDecision::DeferForStorage
            };
        }
        if !network.usable() && !matches!(class, WorkClass::Maintenance) {
            return WorkDecision::WaitForNetwork;
        }
        if matches!(class, WorkClass::Bulk | WorkClass::Maintenance)
            && (power.low_power
                || matches!(power.thermal, ThermalState::Critical)
                || (self.maintenance_on_ac_power && !power.charging))
        {
            return WorkDecision::DeferForPower;
        }
        WorkDecision::Run
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinatorAction {
    AdvanceCursor,
    CheckpointBootstrap,
    MigrateStore,
    MaintainStore,
}

#[derive(Clone, Copy, Debug)]
pub struct DesktopCoordinator {
    grant: LeaseGrant,
}

impl DesktopCoordinator {
    #[must_use]
    pub const fn new(grant: LeaseGrant) -> Self {
        Self { grant }
    }

    #[must_use]
    pub const fn fencing_token(self) -> FencingToken {
        self.grant.fencing_token
    }

    /// Authorizes coordinator-owned metadata work against the current durable lease.
    ///
    /// # Errors
    ///
    /// Rejects an expired, replaced, wrong-store, wrong-process, or wrong-generation grant.
    pub fn authorize(
        self,
        current: CoordinationSnapshot,
        now_unix_ms: u64,
        _action: CoordinatorAction,
    ) -> Result<FencingToken, DesktopError> {
        current
            .validate(self.grant, now_unix_ms)
            .map_err(|_| DesktopError::StaleCoordinator)?;
        Ok(self.grant.fencing_token)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeAction {
    None,
    WakeScheduler,
    RefreshCredentialsAndAuthority,
}

#[derive(Clone, Copy, Debug)]
pub struct DesktopSession {
    lifecycle: DesktopLifecycle,
    network_generation: u64,
    policy: DesktopPolicy,
}

impl DesktopSession {
    #[must_use]
    pub const fn new(policy: DesktopPolicy) -> Self {
        Self {
            lifecycle: DesktopLifecycle::Active,
            network_generation: 0,
            policy,
        }
    }
    #[must_use]
    pub const fn lifecycle(self) -> DesktopLifecycle {
        self.lifecycle
    }
    #[must_use]
    pub const fn network_generation(self) -> u64 {
        self.network_generation
    }

    /// Applies an OS lifecycle event and returns required recovery work.
    ///
    /// # Errors
    ///
    /// Returns an error if the network generation cannot advance on resume.
    pub fn transition(&mut self, next: DesktopLifecycle) -> Result<ResumeAction, DesktopError> {
        let was_sleeping = self.lifecycle == DesktopLifecycle::Sleeping;
        self.lifecycle = next;
        if was_sleeping
            && matches!(
                next,
                DesktopLifecycle::Active | DesktopLifecycle::SessionUnlocked
            )
        {
            self.network_generation = self
                .network_generation
                .checked_add(1)
                .ok_or(DesktopError::GenerationExhausted)?;
            return Ok(if self.policy.sync_on_resume {
                ResumeAction::RefreshCredentialsAndAuthority
            } else {
                ResumeAction::WakeScheduler
            });
        }
        Ok(ResumeAction::None)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreBinding {
    pub store_id: LocalStoreId,
    pub store_generation: LocalStoreGeneration,
    pub device_id: [u8; 16],
    pub device_binding_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreOpenDisposition {
    Open,
    RebindAsNewDevice,
    RejectRollback,
}

#[must_use]
pub fn classify_store_open(
    persisted: StoreBinding,
    installed: StoreBinding,
) -> StoreOpenDisposition {
    if persisted.store_id != installed.store_id || persisted.device_id != installed.device_id {
        StoreOpenDisposition::RebindAsNewDevice
    } else if persisted.store_generation.0 > installed.store_generation.0
        || persisted.device_binding_generation > installed.device_binding_generation
    {
        StoreOpenDisposition::RejectRollback
    } else {
        StoreOpenDisposition::Open
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// Creates zeroized secret material.
    ///
    /// # Errors
    ///
    /// Rejects empty or oversized secrets.
    pub fn new(value: Vec<u8>) -> Result<Self, DesktopError> {
        if value.is_empty() || value.len() > 64 * 1024 {
            return Err(DesktopError::InvalidInput);
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}
impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretBytes([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DesktopError {
    #[error("desktop platform input is invalid")]
    InvalidInput,
    #[error("desktop coordinator lease is stale")]
    StaleCoordinator,
    #[error("desktop generation exhausted")]
    GenerationExhausted,
    #[error("desktop secure service is unavailable")]
    SecureStoreUnavailable,
    #[error("desktop platform operation was denied")]
    Denied,
}

#[async_trait]
pub trait DesktopSecureStore: Send + Sync {
    async fn put(&self, key: &str, value: SecretBytes) -> Result<(), DesktopError>;
    async fn get(&self, key: &str) -> Result<SecretBytes, DesktopError>;
    async fn delete(&self, key: &str) -> Result<(), DesktopError>;
}
pub trait DesktopLifecycleSource: Send + Sync {
    fn current_lifecycle(&self) -> DesktopLifecycle;
}
pub trait DesktopNetworkMonitor: Send + Sync {
    fn current_network(&self) -> NetworkContext;
}
pub trait DesktopAutostart: Send + Sync {
    fn enabled(&self) -> bool;
}
pub trait DesktopNotificationHost: Send + Sync {
    /// Publishes a privacy-safe notification category.
    ///
    /// # Errors
    ///
    /// Returns a platform denial or availability error.
    fn notify_private(&self, category: &str) -> Result<(), DesktopError>;
}
pub trait DesktopFileIntegration: Send + Sync {
    /// Enqueues a host-approved path as validated durable import work.
    ///
    /// # Errors
    ///
    /// Rejects inaccessible, unsafe, or invalid import paths.
    fn enqueue_validated_import(&self, path: &Path) -> Result<(), DesktopError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_coordination::{LeaseKind, ProcessInstanceId};

    fn grant(token: u64) -> LeaseGrant {
        LeaseGrant {
            store_id: LocalStoreId::from_uuid(uuid::Uuid::nil()),
            owner_id: ProcessInstanceId::from_uuid(uuid::Uuid::nil()),
            fencing_token: FencingToken(token),
            kind: LeaseKind::SyncCoordinator,
            expires_at_unix_ms: 100,
            store_generation: LocalStoreGeneration::INITIAL,
        }
    }

    #[test]
    fn stale_process_cannot_authorize_metadata() {
        let stale = DesktopCoordinator::new(grant(1));
        let current = CoordinationSnapshot {
            store_id: grant(2).store_id,
            store_generation: LocalStoreGeneration::INITIAL,
            fencing_token: FencingToken(2),
            owner_id: Some(grant(2).owner_id),
            kind: LeaseKind::SyncCoordinator,
            expires_at_unix_ms: 100,
        };
        assert_eq!(
            stale.authorize(current, 50, CoordinatorAction::AdvanceCursor),
            Err(DesktopError::StaleCoordinator)
        );
    }

    #[test]
    fn resume_invalidates_network_assumptions() {
        let mut session = DesktopSession::new(DesktopPolicy::default());
        assert_eq!(
            session.transition(DesktopLifecycle::Sleeping),
            Ok(ResumeAction::None)
        );
        assert_eq!(
            session.transition(DesktopLifecycle::Active),
            Ok(ResumeAction::RefreshCredentialsAndAuthority)
        );
        assert_eq!(session.network_generation(), 1);
    }

    #[test]
    fn cloned_store_requires_new_device_binding() {
        let persisted = StoreBinding {
            store_id: LocalStoreId::from_uuid(uuid::Uuid::nil()),
            store_generation: LocalStoreGeneration::INITIAL,
            device_id: [1; 16],
            device_binding_generation: 1,
        };
        assert_eq!(
            classify_store_open(
                persisted,
                StoreBinding {
                    device_id: [2; 16],
                    ..persisted
                }
            ),
            StoreOpenDisposition::RebindAsNewDevice
        );
    }
}
