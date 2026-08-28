//! Stable capability identifiers and bounded peer advertisements.

use crate::{CompatibilityError, SnapshotSchemaVersion};
use aequora_types::ProtocolVersion;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Maximum protocol versions accepted from one untrusted hello.
pub const MAX_ADVERTISED_PROTOCOLS: usize = 16;
/// Maximum capability IDs accepted from one untrusted hello.
pub const MAX_ADVERTISED_CAPABILITIES: usize = 256;
/// Maximum snapshot schema versions accepted from one untrusted hello.
pub const MAX_ADVERTISED_SNAPSHOT_VERSIONS: usize = 16;

/// Stable numeric interoperability behavior identifier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CapabilityId(pub u32);

/// Core capability IDs. Once allocated, these values are never reused.
pub mod ids {
    use super::CapabilityId;

    pub const POSTCARD_V1: CapabilityId = CapabilityId(1);
    pub const ZSTD: CapabilityId = CapabilityId(2);
    pub const SNAPSHOT_V1: CapabilityId = CapabilityId(3);
    pub const TOMBSTONES: CapabilityId = CapabilityId(4);
    pub const STREAMING_SNAPSHOTS: CapabilityId = CapabilityId(5);
    pub const PUSH_HINTS: CapabilityId = CapabilityId(6);
    pub const QUIC: CapabilityId = CapabilityId(7);
    pub const MULTI_REGION: CapabilityId = CapabilityId(8);
    pub const LINEAGE_V1: CapabilityId = CapabilityId(9);
    pub const INTEGRITY_V1: CapabilityId = CapabilityId(10);
    pub const SCOPE_V1: CapabilityId = CapabilityId(11);
    pub const LIVE_V1: CapabilityId = CapabilityId(12);
    pub const SIGNED_SNAPSHOT_V1: CapabilityId = CapabilityId(13);
    pub const ENCRYPTED_SNAPSHOT_V1: CapabilityId = CapabilityId(14);
    pub const DEVICE_SIGNATURE_V1: CapabilityId = CapabilityId(15);
    pub const AUTHORITY_EPOCH_V1: CapabilityId = CapabilityId(16);
    pub const RESOURCE_CONSTRAINED_V1: CapabilityId = CapabilityId(17);
    pub const NEGOTIATION_V1: CapabilityId = CapabilityId(18);
}

/// Broad capability ownership category used in generated reports.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CapabilityCategory {
    Protocol,
    Codec,
    Compression,
    Snapshot,
    LiveTransport,
    Crypto,
    Scope,
    Replay,
    Bulk,
    Regional,
    Resource,
}

/// Whether absence permits fallback or must fail closed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CapabilityRequirementKind {
    OptionalWithFallback,
    OptionalOptimization,
    RequiredForSafety,
    RequiredForSemantics,
}

impl CapabilityRequirementKind {
    /// Returns true when negotiation must reject a peer lacking this capability.
    #[must_use]
    pub const fn is_required(self) -> bool {
        matches!(self, Self::RequiredForSafety | Self::RequiredForSemantics)
    }
}

/// Bounded, deduplicated interoperability advertisement.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilitySet {
    pub protocol_versions: BTreeSet<ProtocolVersion>,
    pub capabilities: BTreeSet<CapabilityId>,
    pub snapshot_versions: BTreeSet<SnapshotSchemaVersion>,
}

impl CapabilitySet {
    /// Validates attacker-controlled collection sizes and disallows version zero.
    ///
    /// # Errors
    ///
    /// Returns a bounded-advertisement error for empty, oversized, or zero-valued input.
    pub fn validate(&self) -> Result<(), CompatibilityError> {
        if self.protocol_versions.is_empty()
            || self.protocol_versions.len() > MAX_ADVERTISED_PROTOCOLS
            || self.protocol_versions.iter().any(|version| version.0 == 0)
        {
            return Err(CompatibilityError::InvalidProtocolAdvertisement);
        }
        if self.capabilities.len() > MAX_ADVERTISED_CAPABILITIES
            || self.capabilities.iter().any(|capability| capability.0 == 0)
        {
            return Err(CompatibilityError::CapabilityAdvertisementTooLarge);
        }
        if self.snapshot_versions.is_empty()
            || self.snapshot_versions.len() > MAX_ADVERTISED_SNAPSHOT_VERSIONS
            || self.snapshot_versions.iter().any(|version| version.0 == 0)
        {
            return Err(CompatibilityError::InvalidSnapshotAdvertisement);
        }
        Ok(())
    }
}
