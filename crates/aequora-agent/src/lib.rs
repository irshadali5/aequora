//! Runtime-neutral state machine for a current-user Aequora desktop agent.
//!
//! Socket/named-pipe servers and database adapters are supplied by application crates.

use aequora_coordination::{CoordinationSnapshot, LeaseGrant, LocalStoreId};
use aequora_desktop_runtime::{CoordinatorAction, DesktopCoordinator, DesktopError};
use aequora_ipc_protocol::{
    Command, Handshake, IpcProtocolVersion, SessionToken, VersionNegotiation, negotiate,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum AgentState {
    #[default]
    Stopped,
    Starting,
    Running,
    Idle,
    Syncing,
    Updating,
    Stopping,
    Failed,
}

#[derive(Clone, Debug)]
pub struct AgentSession {
    expected_token: SessionToken,
    minimum_registry_generation: u64,
}

impl AgentSession {
    #[must_use]
    pub const fn new(expected_token: SessionToken, minimum_registry_generation: u64) -> Self {
        Self {
            expected_token,
            minimum_registry_generation,
        }
    }

    /// Authenticates a same-user transport after its socket/pipe ACL check.
    ///
    /// # Errors
    ///
    /// Rejects malformed, unauthenticated, incompatible, or registry-stale clients.
    pub fn authenticate(
        &self,
        handshake: &Handshake,
        peer_is_current_user: bool,
    ) -> Result<IpcProtocolVersion, AgentError> {
        handshake
            .validate()
            .map_err(|_| AgentError::InvalidHandshake)?;
        if !peer_is_current_user || !handshake.session_token.matches(&self.expected_token) {
            return Err(AgentError::Unauthenticated);
        }
        if handshake.registry_generation < self.minimum_registry_generation {
            return Err(AgentError::RegistryUpgradeRequired);
        }
        match negotiate(handshake.protocol_version) {
            VersionNegotiation::Compatible(version) => Ok(version),
            VersionNegotiation::AgentUpgradeRequired => Err(AgentError::AgentUpgradeRequired),
        }
    }

    /// Validates the IPC command before routing it through application domain services.
    ///
    /// # Errors
    ///
    /// Rejects malformed commands. The protocol intentionally exposes no raw metadata mutation.
    pub fn authorize_command(&self, command: &Command) -> Result<CommandRoute, AgentError> {
        command.validate().map_err(|_| AgentError::InvalidCommand)?;
        Ok(match command {
            Command::OpenStore { .. } => CommandRoute::StoreService,
            Command::Mutate { .. } | Command::Query { .. } | Command::ResolveConflict { .. } => {
                CommandRoute::DomainService
            }
            Command::SyncNow | Command::GetStatus | Command::Subscribe { .. } => {
                CommandRoute::SyncService
            }
            Command::ExportDiagnostics => CommandRoute::DiagnosticsService,
            Command::ShutdownAgent => CommandRoute::LifecycleService,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandRoute {
    StoreService,
    DomainService,
    SyncService,
    DiagnosticsService,
    LifecycleService,
}

#[derive(Clone, Debug, Default)]
pub struct StoreRegistry {
    stores: BTreeMap<String, StoreRegistration>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreRegistration {
    pub store_id: LocalStoreId,
    pub display_label: String,
    pub last_opened_unix_ms: u64,
}

impl StoreRegistry {
    /// Registers non-secret profile metadata.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, or control-character profile metadata.
    pub fn register(
        &mut self,
        profile: String,
        registration: StoreRegistration,
    ) -> Result<(), AgentError> {
        if !valid_text(&profile) || !valid_text(&registration.display_label) {
            return Err(AgentError::InvalidProfile);
        }
        self.stores.insert(profile, registration);
        Ok(())
    }
    #[must_use]
    pub fn get(&self, profile: &str) -> Option<&StoreRegistration> {
        self.stores.get(profile)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AgentCoordinator {
    coordinator: DesktopCoordinator,
}

impl AgentCoordinator {
    #[must_use]
    pub const fn new(grant: LeaseGrant) -> Self {
        Self {
            coordinator: DesktopCoordinator::new(grant),
        }
    }

    /// Fences agent-owned work against the durable coordination snapshot.
    ///
    /// # Errors
    ///
    /// Rejects a stale or expired lease.
    pub fn authorize(
        &self,
        snapshot: CoordinationSnapshot,
        now_unix_ms: u64,
        action: CoordinatorAction,
    ) -> Result<(), AgentError> {
        self.coordinator
            .authorize(snapshot, now_unix_ms, action)
            .map(|_| ())
            .map_err(AgentError::Coordination)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentDiagnostics {
    pub app_build: String,
    pub agent_build: String,
    pub os: String,
    pub store_metadata_version: u32,
    pub pending_outbox: u64,
    pub network_state: String,
}

impl AgentDiagnostics {
    /// Validates the bounded, secret-free support summary.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, or control-character text.
    pub fn validate(&self) -> Result<(), AgentError> {
        if [
            &self.app_build,
            &self.agent_build,
            &self.os,
            &self.network_state,
        ]
        .iter()
        .all(|value| valid_text(value))
        {
            Ok(())
        } else {
            Err(AgentError::UnsafeDiagnostics)
        }
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum AgentError {
    #[error("agent IPC handshake is invalid")]
    InvalidHandshake,
    #[error("agent IPC peer is not authenticated as the current user")]
    Unauthenticated,
    #[error("agent upgrade is required")]
    AgentUpgradeRequired,
    #[error("agent registry upgrade is required")]
    RegistryUpgradeRequired,
    #[error("agent IPC command is invalid")]
    InvalidCommand,
    #[error("agent profile metadata is invalid")]
    InvalidProfile,
    #[error("agent diagnostics contain unsafe data")]
    UnsafeDiagnostics,
    #[error("agent coordination failed: {0}")]
    Coordination(DesktopError),
}

fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_ipc_protocol::CURRENT_IPC_PROTOCOL_VERSION;

    #[test]
    fn wrong_user_or_token_cannot_connect() {
        let expected = SessionToken::new(vec![1; 32]).unwrap_or_else(|_| unreachable!());
        let session = AgentSession::new(expected, 4);
        let handshake = Handshake {
            protocol_version: CURRENT_IPC_PROTOCOL_VERSION,
            build_id: "ui-1".into(),
            registry_generation: 4,
            session_token: SessionToken::new(vec![1; 32]).unwrap_or_else(|_| unreachable!()),
        };
        assert_eq!(
            session.authenticate(&handshake, false),
            Err(AgentError::Unauthenticated)
        );
    }

    #[test]
    fn protocol_has_no_raw_metadata_route() {
        let session = AgentSession::new(
            SessionToken::new(vec![1; 32]).unwrap_or_else(|_| unreachable!()),
            1,
        );
        assert_eq!(
            session.authorize_command(&Command::Mutate { operation: vec![1] }),
            Ok(CommandRoute::DomainService)
        );
    }
}
