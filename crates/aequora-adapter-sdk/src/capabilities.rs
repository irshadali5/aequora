//! Versioned storage capabilities, manifests, and fail-closed startup validation.

use crate::AdapterError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Stable adapter identity assigned independently from a crate or database name.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct AdapterId(pub u128);

/// Adapter crate/API version, independent from protocol and schema versions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct AdapterVersion {
    /// SemVer major version.
    pub major: u16,
    /// SemVer minor version.
    pub minor: u16,
    /// SemVer patch version.
    pub patch: u16,
}

impl AdapterVersion {
    /// Constructs an adapter version.
    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

/// Stable semantic capability identifier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CapabilityId(pub u16);

impl CapabilityId {
    /// Atomic application mutation plus outbox insertion.
    pub const ATOMIC_LOCAL_OUTBOX: Self = Self(1);
    /// Atomic business mutation, version, journal, ledger, and required audit.
    pub const ATOMIC_AUTHORITATIVE_COMMIT: Self = Self(2);
    /// Compare-and-swap version transitions.
    pub const COMPARE_AND_SWAP: Self = Self(3);
    /// Durable outbox claiming and terminal disposition.
    pub const OUTBOX: Self = Self(4);
    /// Cursor compare/update coupled with local reconcile.
    pub const CURSOR: Self = Self(5);
    /// Monotonic committed journal scanning and retention floor.
    pub const JOURNAL: Self = Self(6);
    /// Operation ledger identity and payload-digest enforcement.
    pub const OPERATION_LEDGER: Self = Self(7);
    /// Ordered, checksummed physical migrations.
    pub const MIGRATIONS: Self = Self(8);
    /// Snapshot creation, publication, resume, and activation.
    pub const SNAPSHOT: Self = Self(9);
    /// Lease ownership with monotonic fencing tokens.
    pub const FENCING: Self = Self(10);
    /// Integrity digests or Merkle roots.
    pub const INTEGRITY: Self = Self(11);
    /// Retention, erasure, and legal-hold metadata.
    pub const GOVERNANCE: Self = Self(12);
    /// Required immutable audit evidence.
    pub const AUDIT: Self = Self(13);
    /// Consistent backup and restore hooks.
    pub const BACKUP: Self = Self(14);
    /// Cryptographic erase or equivalent secure deletion.
    pub const SECURE_ERASE: Self = Self(15);
    /// Immutable object publication and bounded range reads.
    pub const OBJECT_STORAGE: Self = Self(16);
}

/// Version of one semantic capability contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CapabilityVersion(pub u16);

impl CapabilityVersion {
    /// First published contract version.
    pub const V1: Self = Self(1);
}

/// Snapshot capability strength.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum SnapshotLevel {
    /// No snapshot semantics.
    None,
    /// One complete logical snapshot can be created and installed.
    Logical,
    /// Chunks can be produced and resumed without full buffering.
    Streaming,
    /// A verified immutable generation activates atomically.
    AtomicGenerationSwap,
}

/// Versioned capability strength. Future variants may be added compatibly.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[non_exhaustive]
pub enum CapabilityLevel {
    /// Capability is implemented at its base semantic level.
    Supported,
    /// Snapshot-specific level.
    Snapshot(SnapshotLevel),
}

/// One declared semantic capability.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdapterCapability {
    /// Stable capability identity.
    pub id: CapabilityId,
    /// Contract version implemented by the adapter.
    pub version: CapabilityVersion,
    /// Semantic strength implemented.
    pub level: CapabilityLevel,
}

impl AdapterCapability {
    /// Constructs a base version-one capability.
    #[must_use]
    pub const fn v1(id: CapabilityId) -> Self {
        Self {
            id,
            version: CapabilityVersion::V1,
            level: CapabilityLevel::Supported,
        }
    }
}

/// Narrow storage role implemented by an adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[non_exhaustive]
pub enum AdapterRole {
    /// Writable local replica and its domain transaction boundary.
    LocalReplicaStore,
    /// Authoritative business transaction boundary.
    AuthoritativeStore,
    /// Ordered change journal.
    JournalStore,
    /// Idempotent operation ledger.
    OperationLedgerStore,
    /// Immutable snapshot publication and installation.
    SnapshotStore,
    /// Blob/object metadata.
    BlobMetadataStore,
    /// Integrity digest persistence.
    IntegrityStore,
    /// Governance directive persistence.
    GovernanceStore,
    /// Lease and fencing coordination.
    FencingStore,
    /// Immutable artifact bytes.
    ObjectStore,
}

/// Broad physical store class used only for diagnostics and composition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum StoreKind {
    /// Embedded local database.
    Embedded,
    /// Networked transactional database.
    NetworkDatabase,
    /// Immutable blob/object service.
    ObjectStorage,
    /// Deterministic in-memory semantic oracle.
    Reference,
}

/// Public ecosystem support label.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AdapterSupport {
    /// No production claim.
    Experimental,
    /// Community-maintained adapter with passing self-conformance evidence.
    CommunityVerified,
    /// Aequora maintainer-reviewed conformance evidence.
    MaintainerVerified,
    /// Maintainer-owned adapter subject to release gates.
    Official,
}

/// Physical concurrency model exposed to schedulers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConcurrencyModel {
    /// One writer and one process owns the store.
    SingleWriterSingleProcess,
    /// Writes serialize but independent processes may open the store.
    SingleWriterMultiProcess,
    /// Multiple writers are coordinated by the engine.
    MultiWriter,
}

/// Stable adapter identity and human-readable release information.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterDescriptor {
    /// Registry-assigned adapter identity.
    pub adapter_id: AdapterId,
    /// Display name; it is not used as identity.
    pub name: &'static str,
    /// Adapter crate/API version.
    pub version: AdapterVersion,
    /// Broad physical store class.
    pub store_kind: StoreKind,
}

/// Machine-readable adapter contract and support statement.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterManifest {
    /// Stable adapter descriptor.
    pub descriptor: AdapterDescriptor,
    /// Independently composable storage roles.
    pub roles: &'static [AdapterRole],
    /// Versioned semantic claims.
    pub capabilities: &'static [AdapterCapability],
    /// Ecosystem support label.
    pub support: AdapterSupport,
    /// Documented engine versions.
    pub supported_engine_versions: &'static [&'static str],
    /// Documented platform targets.
    pub supported_targets: &'static [&'static str],
    /// Explicit known limitations.
    pub known_limitations: &'static [&'static str],
    /// Physical concurrency behavior.
    pub concurrency: ConcurrencyModel,
    /// Whether the adapter has a named maintainer owner.
    pub maintainer_owned: bool,
    /// Whether migration, fault, performance, and support documentation is published.
    pub release_evidence_complete: bool,
}

impl AdapterManifest {
    /// Validates structural and policy-level truthfulness of this declaration.
    ///
    /// This does not prove a capability; [`CertifiedEnvironment`] supplies runtime evidence.
    ///
    /// # Errors
    ///
    /// Rejects empty identity/support fields, duplicate capability IDs, unsupported official
    /// claims, and invalid capability versions.
    pub fn validate(self) -> Result<(), AdapterError> {
        if self.descriptor.adapter_id.0 == 0
            || self.descriptor.name.trim().is_empty()
            || self.roles.is_empty()
            || self.capabilities.is_empty()
            || self.supported_engine_versions.is_empty()
            || self.supported_targets.is_empty()
        {
            return Err(AdapterError::invalid_configuration(
                "adapter manifest is incomplete",
            ));
        }
        let mut ids = BTreeSet::new();
        if self.capabilities.iter().any(|capability| {
            capability.id.0 == 0
                || capability.version.0 == 0
                || !ids.insert(capability.id)
        }) {
            return Err(AdapterError::invalid_configuration(
                "adapter capabilities must have unique non-zero IDs and versions",
            ));
        }
        if self.support == AdapterSupport::Official
            && (!self.maintainer_owned || !self.release_evidence_complete)
        {
            return Err(AdapterError::invalid_configuration(
                "official adapters require maintainer ownership and complete release evidence",
            ));
        }
        Ok(())
    }

    /// Returns whether a role is explicitly implemented.
    #[must_use]
    pub fn supports_role(self, role: AdapterRole) -> bool {
        self.roles.contains(&role)
    }

    /// Returns the declared version of a semantic capability.
    #[must_use]
    pub fn capability(self, id: CapabilityId) -> Option<AdapterCapability> {
        self.capabilities
            .iter()
            .copied()
            .find(|capability| capability.id == id)
    }
}

/// Environment-bound conformance evidence used by startup validation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CertifiedEnvironment {
    /// Adapter identity to which the evidence is bound.
    pub adapter_id: AdapterId,
    /// Exact adapter version tested.
    pub adapter_version: AdapterVersion,
    /// Physical engine version or deployment class.
    pub engine_version: String,
    /// Compilation target triple.
    pub target: String,
    /// Stable fingerprint of relevant feature/configuration values.
    pub feature_fingerprint: String,
    /// Conformance suite version.
    pub suite_version: u32,
    /// Capabilities whose corresponding tests passed.
    pub verified_capabilities: BTreeSet<CapabilityId>,
}

/// Required role/capabilities for one deployment composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterRequirements {
    /// Required logical roles.
    pub roles: BTreeSet<AdapterRole>,
    /// Required capability contract versions.
    pub capabilities: BTreeSet<(CapabilityId, CapabilityVersion)>,
    /// Lowest accepted ecosystem support label.
    pub minimum_support: AdapterSupport,
    /// Expected engine version or deployment class.
    pub engine_version: String,
    /// Expected target triple.
    pub target: String,
    /// Expected relevant feature/configuration fingerprint.
    pub feature_fingerprint: String,
    /// Minimum accepted conformance suite version.
    pub minimum_suite_version: u32,
}

impl AdapterRequirements {
    /// Verifies a manifest and its exact environment evidence without silent downgrade.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError`] when identity, version, environment, support, role, capability, or
    /// conformance evidence does not satisfy the deployment requirements.
    pub fn verify(
        &self,
        manifest: AdapterManifest,
        evidence: &CertifiedEnvironment,
    ) -> Result<(), AdapterError> {
        manifest.validate()?;
        let environment_matches = evidence.adapter_id == manifest.descriptor.adapter_id
            && evidence.adapter_version == manifest.descriptor.version
            && evidence.engine_version == self.engine_version
            && evidence.target == self.target
            && evidence.feature_fingerprint == self.feature_fingerprint
            && evidence.suite_version >= self.minimum_suite_version;
        if !environment_matches {
            return Err(AdapterError::new(
                crate::AdapterErrorKind::UnsupportedCapability,
                "adapter certification does not match the selected environment",
            ));
        }
        if manifest.support < self.minimum_support
            || self
                .roles
                .iter()
                .any(|role| !manifest.supports_role(*role))
        {
            return Err(AdapterError::new(
                crate::AdapterErrorKind::UnsupportedCapability,
                "adapter does not satisfy required support level or role",
            ));
        }
        for (id, version) in &self.capabilities {
            let Some(claim) = manifest.capability(*id) else {
                return Err(AdapterError::new(
                    crate::AdapterErrorKind::UnsupportedCapability,
                    "adapter is missing a required capability",
                ));
            };
            if claim.version < *version || !evidence.verified_capabilities.contains(id) {
                return Err(AdapterError::new(
                    crate::AdapterErrorKind::UnsupportedCapability,
                    "required adapter capability is not verified",
                ));
            }
        }
        Ok(())
    }
}

/// Implemented by adapters that expose a stable storage capability manifest.
pub trait AdapterCapabilities: Send + Sync {
    /// Returns the immutable manifest for this adapter build.
    fn adapter_manifest(&self) -> AdapterManifest;
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLES: &[AdapterRole] = &[AdapterRole::LocalReplicaStore];
    const CAPS: &[AdapterCapability] = &[AdapterCapability::v1(
        CapabilityId::ATOMIC_LOCAL_OUTBOX,
    )];
    const ENGINES: &[&str] = &["reference-1"];
    const TARGETS: &[&str] = &["test-target"];

    fn manifest() -> AdapterManifest {
        AdapterManifest {
            descriptor: AdapterDescriptor {
                adapter_id: AdapterId(1),
                name: "reference",
                version: AdapterVersion::new(1, 0, 0),
                store_kind: StoreKind::Reference,
            },
            roles: ROLES,
            capabilities: CAPS,
            support: AdapterSupport::MaintainerVerified,
            supported_engine_versions: ENGINES,
            supported_targets: TARGETS,
            known_limitations: &[],
            concurrency: ConcurrencyModel::SingleWriterSingleProcess,
            maintainer_owned: true,
            release_evidence_complete: true,
        }
    }

    #[test]
    fn unverified_claim_fails_startup() {
        let requirements = AdapterRequirements {
            roles: BTreeSet::from([AdapterRole::LocalReplicaStore]),
            capabilities: BTreeSet::from([(
                CapabilityId::ATOMIC_LOCAL_OUTBOX,
                CapabilityVersion::V1,
            )]),
            minimum_support: AdapterSupport::MaintainerVerified,
            engine_version: "reference-1".to_owned(),
            target: "test-target".to_owned(),
            feature_fingerprint: "default".to_owned(),
            minimum_suite_version: 1,
        };
        let evidence = CertifiedEnvironment {
            adapter_id: AdapterId(1),
            adapter_version: AdapterVersion::new(1, 0, 0),
            engine_version: "reference-1".to_owned(),
            target: "test-target".to_owned(),
            feature_fingerprint: "default".to_owned(),
            suite_version: 1,
            verified_capabilities: BTreeSet::new(),
        };
        assert_eq!(
            requirements.verify(manifest(), &evidence).map_err(|error| error.kind()),
            Err(crate::AdapterErrorKind::UnsupportedCapability)
        );
    }
}
