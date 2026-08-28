//! Versioned, auditable, fail-closed server compatibility policy.

use crate::{
    CapabilityId, CapabilityRequirementKind, CompatibilityError, CompatibilityPolicyGeneration,
    SnapshotSchemaVersion,
};
use aequora_types::ProtocolVersion;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Stable platform family used for platform-specific minimum builds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PlatformId(pub u16);

/// Human-facing semantic build version; build policy uses the monotonic build number.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SemanticBuildVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

/// Unambiguous client release identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientBuildId {
    pub platform: PlatformId,
    pub version: SemanticBuildVersion,
    pub build_number: u64,
}

/// Inclusive emergency block for one platform's build-number range.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BlockedBuildRange {
    pub platform: PlatformId,
    pub first: u64,
    pub last: u64,
}

impl BlockedBuildRange {
    #[must_use]
    pub const fn contains(self, build: ClientBuildId) -> bool {
        self.platform.0 == build.platform.0
            && build.build_number >= self.first
            && build.build_number <= self.last
    }
}

/// Platform-specific minimum-build policy.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientBuildPolicy {
    /// Minimum build permitted to negotiate any online access.
    pub minimum: BTreeMap<PlatformId, u64>,
    /// Minimum build allowed to create new operations. Older permitted builds become read-only.
    pub minimum_for_write: BTreeMap<PlatformId, u64>,
    /// Emergency blocks override normal support windows.
    pub blocked: Vec<BlockedBuildRange>,
}

impl ClientBuildPolicy {
    /// Validates minimum-build ordering and emergency ranges.
    ///
    /// # Errors
    ///
    /// Returns [`CompatibilityError::InvalidBuildPolicy`] for inconsistent bounds.
    pub fn validate(&self) -> Result<(), CompatibilityError> {
        if self.blocked.iter().any(|range| range.first > range.last)
            || self.minimum_for_write.iter().any(|(platform, write)| {
                self.minimum
                    .get(platform)
                    .is_some_and(|minimum| write < minimum)
            })
        {
            return Err(CompatibilityError::InvalidBuildPolicy);
        }
        Ok(())
    }

    #[must_use]
    pub fn classify(&self, build: ClientBuildId) -> BuildDisposition {
        if self
            .blocked
            .iter()
            .copied()
            .any(|range| range.contains(build))
        {
            return BuildDisposition::Blocked;
        }
        if self
            .minimum
            .get(&build.platform)
            .is_some_and(|minimum| build.build_number < *minimum)
        {
            return BuildDisposition::UpgradeRequired;
        }
        if self
            .minimum_for_write
            .get(&build.platform)
            .is_some_and(|minimum| build.build_number < *minimum)
        {
            return BuildDisposition::ReadOnly;
        }
        BuildDisposition::Full
    }
}

/// Access implied by minimum-build and emergency policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildDisposition {
    Full,
    ReadOnly,
    UpgradeRequired,
    Blocked,
}

/// Ordered server protocol selection policy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolPolicy {
    pub preferred: ProtocolVersion,
    /// Ordered by server preference, not necessarily numerically descending.
    pub supported: Vec<ProtocolVersion>,
    pub deprecated: BTreeSet<ProtocolVersion>,
    pub forbidden: BTreeSet<ProtocolVersion>,
    /// Anti-downgrade floor independent of the preferred version.
    pub minimum_allowed: ProtocolVersion,
}

impl ProtocolPolicy {
    /// Validates ordered selection, uniqueness, and anti-downgrade bounds.
    ///
    /// # Errors
    ///
    /// Returns an invalid protocol-policy error for zero, duplicate, forbidden, or unavailable
    /// preferred versions.
    pub fn validate(&self) -> Result<(), CompatibilityError> {
        let unique = self.supported.iter().copied().collect::<BTreeSet<_>>();
        if self.preferred.0 == 0
            || self.minimum_allowed.0 == 0
            || unique.len() != self.supported.len()
            || self.supported.iter().any(|version| version.0 == 0)
            || unique
                .iter()
                .any(|version| self.forbidden.contains(version))
        {
            return Err(CompatibilityError::InvalidProtocolPolicy);
        }
        if !unique.contains(&self.preferred) || self.preferred < self.minimum_allowed {
            return Err(CompatibilityError::InvalidPreferredProtocol);
        }
        Ok(())
    }

    #[must_use]
    pub fn select(&self, offered: &BTreeSet<ProtocolVersion>) -> Option<ProtocolVersion> {
        self.supported.iter().copied().find(|version| {
            *version >= self.minimum_allowed
                && !self.forbidden.contains(version)
                && offered.contains(version)
        })
    }
}

/// Immutable compatibility policy snapshot suitable for atomic reload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CompatibilityPolicy {
    pub generation: CompatibilityPolicyGeneration,
    pub protocols: ProtocolPolicy,
    pub builds: ClientBuildPolicy,
    pub enabled_capabilities: BTreeSet<CapabilityId>,
    pub capability_requirements: BTreeMap<CapabilityId, CapabilityRequirementKind>,
    pub supported_snapshots: BTreeSet<SnapshotSchemaVersion>,
    pub preferred_snapshot: SnapshotSchemaVersion,
    /// Release/change identifier recorded in policy audit events. It is never sent on the hot path.
    pub change_id: u64,
}

impl CompatibilityPolicy {
    /// Validates internal consistency and, when supplied, the complete serving fleet.
    ///
    /// # Errors
    ///
    /// Returns a policy validation error when versions are inconsistent or a required capability
    /// is absent from the policy or serving fleet.
    pub fn validate(
        &self,
        fleet_capabilities: Option<&BTreeSet<CapabilityId>>,
    ) -> Result<(), CompatibilityError> {
        if self.generation.0 == 0 {
            return Err(CompatibilityError::InvalidPolicyGeneration);
        }
        self.protocols.validate()?;
        self.builds.validate()?;
        if !self.enabled_capabilities.contains(&crate::ids::POSTCARD_V1)
            || !self
                .capability_requirements
                .get(&crate::ids::POSTCARD_V1)
                .is_some_and(|kind| kind.is_required())
        {
            return Err(CompatibilityError::RequiredCapabilityUnavailable);
        }
        if self.preferred_snapshot.0 == 0
            || !self.supported_snapshots.contains(&self.preferred_snapshot)
        {
            return Err(CompatibilityError::InvalidRegistryEntry);
        }
        let required = self
            .capability_requirements
            .iter()
            .filter_map(|(id, kind)| kind.is_required().then_some(id));
        for id in required {
            if !self.enabled_capabilities.contains(id)
                || fleet_capabilities.is_some_and(|fleet| !fleet.contains(id))
            {
                return Err(CompatibilityError::RequiredCapabilityUnavailable);
            }
        }
        Ok(())
    }
}
