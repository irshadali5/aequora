//! Stateless, server-selected negotiation and client compatibility state.

use crate::{
    BuildDisposition, CapabilityId, CapabilitySet, ClientBuildId, CompatibilityError,
    CompatibilityPolicy, CompatibilityPolicyGeneration, LocalStoreFormatVersion,
    SnapshotSchemaVersion,
};
use aequora_types::{AuthorityEpoch, AuthorityId, ProtocolVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Client's last known authority timeline; it never substitutes for authority validation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityDescriptorHint {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
}

/// Minimum security posture a high-assurance client is willing to accept.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientSecurityPolicy {
    pub minimum_protocol: ProtocolVersion,
    pub required_capabilities: BTreeSet<CapabilityId>,
}

/// Bounded client offer. The server remains authoritative for selection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientHello {
    pub client_build: ClientBuildId,
    pub supported: CapabilitySet,
    pub authority_hint: Option<AuthorityDescriptorHint>,
    pub local_store_format: LocalStoreFormatVersion,
    pub security_policy: ClientSecurityPolicy,
}

impl ClientHello {
    /// Validates all attacker-controlled hello bounds and non-zero versions.
    ///
    /// # Errors
    ///
    /// Returns a bounded-advertisement error for malformed or oversized input.
    pub fn validate(&self) -> Result<(), CompatibilityError> {
        self.supported.validate()?;
        if self.security_policy.minimum_protocol.0 == 0
            || self.local_store_format.0 == 0
            || self.security_policy.required_capabilities.len() > crate::MAX_ADVERTISED_CAPABILITIES
        {
            return Err(CompatibilityError::InvalidProtocolAdvertisement);
        }
        Ok(())
    }
}

/// Effective access granted to one negotiated session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CompatibilityMode {
    Full,
    ReadOnly,
    BootstrapOnly,
    UpgradeRequired,
}

impl CompatibilityMode {
    /// Whether new operation creation is permitted.
    #[must_use]
    pub const fn allows_writes(self) -> bool {
        matches!(self, Self::Full)
    }
}

/// Concrete server-selected interoperability profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionProfile {
    pub protocol: ProtocolVersion,
    pub codec: CapabilityId,
    pub compression: Option<CapabilityId>,
    pub snapshot_schema: SnapshotSchemaVersion,
    pub capabilities: BTreeSet<CapabilityId>,
    pub mode: CompatibilityMode,
}

/// Successful server response. Requests must still carry `profile.protocol` explicitly.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServerHello {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub profile: SessionProfile,
    pub required_capabilities: BTreeSet<CapabilityId>,
    pub policy_generation: CompatibilityPolicyGeneration,
}

/// Stable reason code for an incompatible client.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CompatibilityFailureCode {
    UnsupportedProtocol,
    RequiredCapabilityMissing,
    ClientSecurityPolicyUnsatisfied,
    ClientBuildBlocked,
    ClientBuildTooOld,
    SnapshotSchemaUnsupported,
    LocalStoreMigrationRequired,
    ProjectionIncompatible,
    OperationSchemaUnsupported,
}

/// Structured recovery steps that preserve local data and user intent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RecoveryInstruction {
    UpgradeClient,
    MigrateStore,
    Rebootstrap,
    Reauthenticate,
    ContactAdministrator,
}

/// Incompatibility details; messages remain product-owned.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CompatibilityFailure {
    pub code: CompatibilityFailureCode,
    pub policy_generation: CompatibilityPolicyGeneration,
    pub recovery: Vec<RecoveryInstruction>,
}

/// Non-blocking deprecation warning.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CompatibilityWarning {
    DeprecatedProtocol(ProtocolVersion),
    ClientBuildApproachingMinimum,
}

/// Result of policy evaluation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CompatibilityResult {
    Compatible(ServerHello),
    UpgradeRecommended {
        hello: ServerHello,
        warning: CompatibilityWarning,
    },
    UpgradeRequired(CompatibilityFailure),
}

/// Authority timeline used to construct a stateless server hello.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerAuthorityContext {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
}

/// Evaluates an untrusted client hello against one already validated immutable policy snapshot.
///
/// # Errors
///
/// Returns [`CompatibilityError`] only when the hello or server policy is structurally invalid.
/// Ordinary incompatibility is returned as [`CompatibilityResult::UpgradeRequired`].
#[allow(clippy::too_many_lines)]
pub fn negotiate(
    hello: &ClientHello,
    policy: &CompatibilityPolicy,
    authority: ServerAuthorityContext,
) -> Result<CompatibilityResult, CompatibilityError> {
    hello.validate()?;
    policy.validate(None)?;

    match policy.builds.classify(hello.client_build) {
        BuildDisposition::Blocked => {
            return Ok(required(
                policy,
                CompatibilityFailureCode::ClientBuildBlocked,
                vec![RecoveryInstruction::UpgradeClient],
            ));
        }
        BuildDisposition::UpgradeRequired => {
            return Ok(required(
                policy,
                CompatibilityFailureCode::ClientBuildTooOld,
                vec![RecoveryInstruction::UpgradeClient],
            ));
        }
        BuildDisposition::Full | BuildDisposition::ReadOnly => {}
    }

    let Some(protocol) = policy.protocols.select(&hello.supported.protocol_versions) else {
        return Ok(required(
            policy,
            CompatibilityFailureCode::UnsupportedProtocol,
            vec![RecoveryInstruction::UpgradeClient],
        ));
    };
    if protocol < hello.security_policy.minimum_protocol {
        return Ok(required(
            policy,
            CompatibilityFailureCode::ClientSecurityPolicyUnsatisfied,
            vec![RecoveryInstruction::ContactAdministrator],
        ));
    }

    let server_required = policy
        .capability_requirements
        .iter()
        .filter_map(|(id, kind)| kind.is_required().then_some(*id))
        .collect::<BTreeSet<_>>();
    if !server_required.is_subset(&hello.supported.capabilities) {
        return Ok(required(
            policy,
            CompatibilityFailureCode::RequiredCapabilityMissing,
            vec![RecoveryInstruction::UpgradeClient],
        ));
    }
    if !hello
        .security_policy
        .required_capabilities
        .is_subset(&policy.enabled_capabilities)
    {
        return Ok(required(
            policy,
            CompatibilityFailureCode::ClientSecurityPolicyUnsatisfied,
            vec![RecoveryInstruction::ContactAdministrator],
        ));
    }

    let snapshot_schema = if hello
        .supported
        .snapshot_versions
        .contains(&policy.preferred_snapshot)
    {
        policy.preferred_snapshot
    } else {
        hello
            .supported
            .snapshot_versions
            .intersection(&policy.supported_snapshots)
            .last()
            .copied()
            .unwrap_or(SnapshotSchemaVersion(0))
    };
    if snapshot_schema.0 == 0 {
        return Ok(required(
            policy,
            CompatibilityFailureCode::SnapshotSchemaUnsupported,
            vec![
                RecoveryInstruction::UpgradeClient,
                RecoveryInstruction::Rebootstrap,
            ],
        ));
    }

    let enabled = hello
        .supported
        .capabilities
        .intersection(&policy.enabled_capabilities)
        .copied()
        .collect::<BTreeSet<_>>();
    let mode = match policy.builds.classify(hello.client_build) {
        BuildDisposition::ReadOnly => CompatibilityMode::ReadOnly,
        BuildDisposition::Full => CompatibilityMode::Full,
        BuildDisposition::UpgradeRequired | BuildDisposition::Blocked => {
            CompatibilityMode::UpgradeRequired
        }
    };
    let response = ServerHello {
        authority_id: authority.authority_id,
        authority_epoch: authority.authority_epoch,
        profile: SessionProfile {
            protocol,
            codec: crate::ids::POSTCARD_V1,
            compression: enabled
                .contains(&crate::ids::ZSTD)
                .then_some(crate::ids::ZSTD),
            snapshot_schema,
            capabilities: enabled,
            mode,
        },
        required_capabilities: server_required,
        policy_generation: policy.generation,
    };
    if policy.protocols.deprecated.contains(&protocol) {
        Ok(CompatibilityResult::UpgradeRecommended {
            hello: response,
            warning: CompatibilityWarning::DeprecatedProtocol(protocol),
        })
    } else {
        Ok(CompatibilityResult::Compatible(response))
    }
}

fn required(
    policy: &CompatibilityPolicy,
    code: CompatibilityFailureCode,
    recovery: Vec<RecoveryInstruction>,
) -> CompatibilityResult {
    CompatibilityResult::UpgradeRequired(CompatibilityFailure {
        code,
        policy_generation: policy.generation,
        recovery,
    })
}

/// Cache identity. Any field change requires renegotiation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionCacheKey {
    pub authority_id: AuthorityId,
    pub authority_epoch: AuthorityEpoch,
    pub client_build: ClientBuildId,
    pub policy_generation: CompatibilityPolicyGeneration,
    pub server_instance_generation: u64,
}

/// Payload-free transcript evidence useful for anti-downgrade diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NegotiationTranscript {
    pub offered_protocols: BTreeSet<ProtocolVersion>,
    pub offered_capabilities: BTreeSet<CapabilityId>,
    pub selected_protocol: ProtocolVersion,
    pub selected_capabilities: BTreeSet<CapabilityId>,
    pub policy_generation: CompatibilityPolicyGeneration,
}

impl NegotiationTranscript {
    /// Deterministic BLAKE3 evidence hash; signing remains the Part 15 host's responsibility.
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aequora.compat.transcript.v1\0");
        for version in &self.offered_protocols {
            hasher.update(&version.0.to_be_bytes());
        }
        hasher.update(&[0xff]);
        for capability in &self.offered_capabilities {
            hasher.update(&capability.0.to_be_bytes());
        }
        hasher.update(&self.selected_protocol.0.to_be_bytes());
        for capability in &self.selected_capabilities {
            hasher.update(&capability.0.to_be_bytes());
        }
        hasher.update(&self.policy_generation.0.to_be_bytes());
        *hasher.finalize().as_bytes()
    }
}

/// Client runtime state; incompatibility never implies local-store deletion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientCompatibilityState {
    Unknown,
    Negotiating,
    Compatible(CompatibilityMode),
    UpgradeRecommended(CompatibilityMode),
    UpgradeRequired(CompatibilityFailureCode),
}

/// Payload-free durable-intent evidence retained across negotiation transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DurableIntentEvidence {
    pub pending_operations: u64,
    pub semantic_fingerprint: [u8; 32],
}

/// Compatibility state wrapper that deliberately has no API capable of mutating local intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientCompatibilityRuntime {
    state: ClientCompatibilityState,
    durable_intent: DurableIntentEvidence,
}

impl ClientCompatibilityRuntime {
    #[must_use]
    pub const fn new(durable_intent: DurableIntentEvidence) -> Self {
        Self {
            state: ClientCompatibilityState::Unknown,
            durable_intent,
        }
    }

    #[must_use]
    pub const fn state(self) -> ClientCompatibilityState {
        self.state
    }

    #[must_use]
    pub const fn durable_intent(self) -> DurableIntentEvidence {
        self.durable_intent
    }

    pub const fn begin_negotiation(&mut self) {
        self.state = ClientCompatibilityState::Negotiating;
    }

    pub fn apply(&mut self, result: &CompatibilityResult) {
        self.state = match result {
            CompatibilityResult::Compatible(hello) => {
                ClientCompatibilityState::Compatible(hello.profile.mode)
            }
            CompatibilityResult::UpgradeRecommended { hello, .. } => {
                ClientCompatibilityState::UpgradeRecommended(hello.profile.mode)
            }
            CompatibilityResult::UpgradeRequired(failure) => {
                ClientCompatibilityState::UpgradeRequired(failure.code)
            }
        };
    }
}

/// Sendability of a client feature or operation requiring server support.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityAvailability {
    Available,
    ReadOnly,
    BlockedUntilServerUpgrade,
}

/// Prevents a new operation from entering the normal sendable outbox before support is known.
#[must_use]
pub fn capability_availability(
    profile: &SessionProfile,
    required: CapabilityId,
) -> CapabilityAvailability {
    if !profile.capabilities.contains(&required) {
        CapabilityAvailability::BlockedUntilServerUpgrade
    } else if !profile.mode.allows_writes() {
        CapabilityAvailability::ReadOnly
    } else {
        CapabilityAvailability::Available
    }
}
