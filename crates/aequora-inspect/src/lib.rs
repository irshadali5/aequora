//! Adapter-neutral, ownership-aware inspection policy.
//!
//! Physical adapters implement the actual reads. This crate decides whether a CLI should use
//! authenticated agent IPC, direct read-only access, or a fenced offline maintenance session.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreIdentity(pub String);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FenceToken(pub String);

/// Current durable-store ownership observed through the coordination API.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StoreOwnership {
    /// A local authenticated agent owns the embedded store.
    AgentOwned { ipc_endpoint: String },
    /// This process owns the current lease and fence.
    CliOwned { fence: FenceToken },
    /// Another process owns the store and no supported IPC route is available.
    OtherProcess,
    /// No live owner exists and the store was explicitly marked offline.
    Offline,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum InspectionMode {
    ReadOnly,
    FencedMaintenance,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InspectionRequest {
    pub store: StoreIdentity,
    pub mode: InspectionMode,
    pub ownership: StoreOwnership,
}

/// Safe route selected before opening physical storage.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum InspectionRoute {
    AuthenticatedAgentIpc { endpoint: String },
    DirectReadOnly,
    DirectFenced { fence: FenceToken },
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum InspectionError {
    #[error("direct access would create an uncontrolled second writer")]
    ConflictingWriter,
    #[error("write inspection requires explicit offline ownership and a current fence")]
    FenceRequired,
}

/// Chooses an inspection route without opening or mutating the store.
///
/// # Errors
///
/// Rejects any request that could create an uncontrolled second writer or lacks a fence.
pub fn route(request: &InspectionRequest) -> Result<InspectionRoute, InspectionError> {
    match (&request.mode, &request.ownership) {
        (_, StoreOwnership::AgentOwned { ipc_endpoint }) => {
            Ok(InspectionRoute::AuthenticatedAgentIpc {
                endpoint: ipc_endpoint.clone(),
            })
        }
        (InspectionMode::ReadOnly, StoreOwnership::Offline | StoreOwnership::CliOwned { .. }) => {
            Ok(InspectionRoute::DirectReadOnly)
        }
        (
            InspectionMode::ReadOnly | InspectionMode::FencedMaintenance,
            StoreOwnership::OtherProcess,
        ) => Err(InspectionError::ConflictingWriter),
        (InspectionMode::FencedMaintenance, StoreOwnership::CliOwned { fence }) => {
            Ok(InspectionRoute::DirectFenced {
                fence: fence.clone(),
            })
        }
        (InspectionMode::FencedMaintenance, StoreOwnership::Offline) => {
            Err(InspectionError::FenceRequired)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FieldClassification {
    Public,
    Sensitive,
    Secret,
    Pii,
    Financial,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InspectedField {
    pub name: String,
    pub classification: FieldClassification,
    pub value: String,
}

impl InspectedField {
    /// Renders registry-classified data with safe defaults.
    #[must_use]
    pub fn render(&self, reveal_sensitive: bool, authorized: bool) -> String {
        match self.classification {
            FieldClassification::Public => self.value.clone(),
            FieldClassification::Sensitive
            | FieldClassification::Pii
            | FieldClassification::Financial
                if reveal_sensitive && authorized =>
            {
                self.value.clone()
            }
            FieldClassification::Sensitive
            | FieldClassification::Secret
            | FieldClassification::Pii
            | FieldClassification::Financial => "<redacted>".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(mode: InspectionMode, ownership: StoreOwnership) -> InspectionRequest {
        InspectionRequest {
            store: StoreIdentity("store-1".to_owned()),
            mode,
            ownership,
        }
    }

    #[test]
    fn agent_owned_store_routes_through_authenticated_ipc() {
        let selected = route(&request(
            InspectionMode::ReadOnly,
            StoreOwnership::AgentOwned {
                ipc_endpoint: "local://agent".to_owned(),
            },
        ));
        assert_eq!(
            selected,
            Ok(InspectionRoute::AuthenticatedAgentIpc {
                endpoint: "local://agent".to_owned()
            })
        );
    }

    #[test]
    fn maintenance_requires_current_cli_fence() {
        assert_eq!(
            route(&request(
                InspectionMode::FencedMaintenance,
                StoreOwnership::Offline
            )),
            Err(InspectionError::FenceRequired)
        );
        let fence = FenceToken("fence-9".to_owned());
        assert_eq!(
            route(&request(
                InspectionMode::FencedMaintenance,
                StoreOwnership::CliOwned {
                    fence: fence.clone()
                }
            )),
            Ok(InspectionRoute::DirectFenced { fence })
        );
    }

    #[test]
    fn secret_is_never_revealed() {
        let field = InspectedField {
            name: "token".to_owned(),
            classification: FieldClassification::Secret,
            value: "fake-secret".to_owned(),
        };
        assert_eq!(field.render(true, true), "<redacted>");
    }
}
