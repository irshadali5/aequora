//! Runtime-, transport-, and database-neutral durable registry contracts.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};
use thiserror::Error;

macro_rules! numeric_id {
    ($name:ident, $repr:ty, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub $repr);
    };
}

numeric_id!(EntityTypeId, u32, "Stable entity-type identifier.");
numeric_id!(OperationKind, u32, "Stable operation-kind identifier.");
numeric_id!(EventKind, u32, "Stable event-kind identifier.");
numeric_id!(FieldId, u32, "Stable semantic field identifier.");
numeric_id!(
    CapabilityId,
    u32,
    "Stable compatibility capability identifier."
);
numeric_id!(
    ConsistencyProfileId,
    u32,
    "Stable consistency-profile family identifier."
);
numeric_id!(ErrorCode, u32, "Stable machine-readable error identifier.");
numeric_id!(JobKind, u32, "Stable durable job-kind identifier.");
numeric_id!(ConsumerKind, u32, "Stable feed consumer-kind identifier.");
numeric_id!(AuditActionId, u32, "Stable audit-action identifier.");
numeric_id!(MigrationId, u32, "Stable migration identifier.");
numeric_id!(ProtocolVersion, u32, "Stable protocol version identifier.");
numeric_id!(MessageKind, u32, "Stable wire-message identifier.");
numeric_id!(PermissionId, u32, "Stable permission identifier.");
numeric_id!(
    AdminActionId,
    u32,
    "Stable control-plane action identifier."
);
numeric_id!(
    ReasonCode,
    u32,
    "Stable machine-readable reason identifier."
);
numeric_id!(DecisionRuleId, u32, "Stable decision-rule identifier.");
numeric_id!(ArtifactFormatId, u32, "Stable artifact-format identifier.");
numeric_id!(
    ConformanceProfileId,
    u32,
    "Stable conformance-profile identifier."
);
numeric_id!(
    ConformanceTestId,
    u32,
    "Stable conformance-test identifier."
);
numeric_id!(
    CertificationTierId,
    u32,
    "Stable certification-tier identifier."
);
numeric_id!(
    VendorNamespaceId,
    u16,
    "Registered extension namespace identifier."
);

/// All compiled durable contract domains governed by Part 29.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum RegistryDomain {
    Entity,
    Operation,
    Event,
    Field,
    Capability,
    ConsistencyProfile,
    Error,
    Job,
    Consumer,
    AuditAction,
    Migration,
    Protocol,
    Message,
    Permission,
    AdminAction,
    Reason,
    DecisionRule,
    ArtifactFormat,
    ConformanceProfile,
    ConformanceTest,
    CertificationTier,
}

impl RegistryDomain {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Entity => "entity",
            Self::Operation => "operation",
            Self::Event => "event",
            Self::Field => "field",
            Self::Capability => "capability",
            Self::ConsistencyProfile => "profile",
            Self::Error => "error",
            Self::Job => "job",
            Self::Consumer => "consumer",
            Self::AuditAction => "audit-action",
            Self::Migration => "migration",
            Self::Protocol => "protocol",
            Self::Message => "message",
            Self::Permission => "permission",
            Self::AdminAction => "admin-action",
            Self::Reason => "reason",
            Self::DecisionRule => "decision-rule",
            Self::ArtifactFormat => "artifact-format",
            Self::ConformanceProfile => "conformance-profile",
            Self::ConformanceTest => "conformance-test",
            Self::CertificationTier => "certification-tier",
        }
    }
}

impl fmt::Display for RegistryDomain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Allocation class inferred from the numeric ID range.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AllocationClass {
    Core,
    RegisteredExtension,
    VendorPrivate,
    Experimental,
}

impl AllocationClass {
    #[must_use]
    pub const fn for_id(id: u32) -> Self {
        match id {
            0x0000_0000..=0x3fff_ffff => Self::Core,
            0x4000_0000..=0xbfff_ffff => Self::RegisteredExtension,
            0xc000_0000..=0xefff_ffff => Self::VendorPrivate,
            0xf000_0000..=0xffff_ffff => Self::Experimental,
        }
    }
}

/// Durable contract lifecycle. Retired IDs remain represented forever.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SupportStatus {
    Experimental,
    Current,
    Supported,
    Deprecated,
    RetryOnly,
    Reserved,
    Removed,
}

impl SupportStatus {
    #[must_use]
    pub const fn allows_new_creation(self) -> bool {
        matches!(self, Self::Experimental | Self::Current | Self::Supported)
    }

    #[must_use]
    pub const fn is_historical(self) -> bool {
        matches!(
            self,
            Self::Deprecated | Self::RetryOnly | Self::Reserved | Self::Removed
        )
    }
}

/// Governance classification for a registry change.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ChangeClassification {
    NonBreaking,
    Additive,
    Deprecated,
    BreakingWithMigration,
    SecurityRequired,
}

/// Review strength required for a durable contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub enum GovernanceLevel {
    InternalImplementation,
    PublicApi,
    DurableContract,
    CriticalSemanticContract,
}

/// Stable ownership metadata; owners are groups or modules, not individuals.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OwnerRef {
    pub team: String,
    pub crate_name: String,
    pub module: String,
}

/// A typed cross-registry reference.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistryRef {
    pub domain: RegistryDomain,
    pub id: u32,
}

/// Canonical source entry. Domain-specific semantics live in named, validated attributes rather
/// than in runtime reflection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistryEntry {
    pub domain: RegistryDomain,
    pub id: u32,
    pub name: String,
    pub owner: OwnerRef,
    pub status: SupportStatus,
    pub introduced_in: String,
    pub description: String,
    #[serde(default)]
    pub schema_version: Option<u32>,
    #[serde(default)]
    pub supported_schema_versions: Vec<u32>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    #[serde(default)]
    pub references: Vec<RegistryRef>,
    #[serde(default)]
    pub security_sensitive: bool,
    #[serde(default)]
    pub change_proposal_ref: Option<String>,
    #[serde(default)]
    pub migration_ref: Option<String>,
    #[serde(default)]
    pub replacement: Option<RegistryRef>,
}

impl RegistryEntry {
    #[must_use]
    pub const fn allocation_class(&self) -> AllocationClass {
        AllocationClass::for_id(self.id)
    }

    #[must_use]
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }
}

/// One source file fragment. Fragments are merged at build time.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistryFragment {
    pub entries: Vec<RegistryEntry>,
}

/// Canonical registry-set metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistryManifest {
    pub generation: u64,
    pub release: String,
    pub namespace: String,
}

/// Published semantic identity retained in `registry.lock`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LockEntry {
    pub domain: RegistryDomain,
    pub id: u32,
    pub name: String,
    pub semantic_digest: String,
    pub status: SupportStatus,
}

/// Immutable published-ID ledger.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistryLock {
    pub generation: u64,
    pub entries: Vec<LockEntry>,
}

/// Fully merged registry used by build tooling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrySet {
    pub manifest: RegistryManifest,
    pub entries: Vec<RegistryEntry>,
}

/// Required evidence for a Level 2 or Level 3 durable-contract proposal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistryChangeProposal {
    pub change_id: String,
    pub title: String,
    pub affected_entries: Vec<RegistryRef>,
    pub current_semantics: String,
    pub proposed_semantics: String,
    pub compatibility_impact: String,
    pub migration_plan: String,
    pub replay_impact: String,
    pub offline_client_impact: String,
    pub security_impact: String,
    pub governance_impact: String,
    pub rollback_plan: String,
    pub test_plan: String,
    pub level: GovernanceLevel,
}

/// Payload-free release evidence generated from reviewed registry changes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegistryReleaseManifest {
    pub release: String,
    pub generation: u64,
    pub digest: String,
    pub new_ids: Vec<RegistryRef>,
    pub deprecated_ids: Vec<RegistryRef>,
    pub schema_bumps: Vec<RegistryRef>,
    pub required_capability_changes: Vec<RegistryRef>,
    pub migration_ids: Vec<MigrationId>,
}

/// Compile-time extension namespace contract. Runtime mutation is deliberately unsupported.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExtensionManifest {
    pub namespace: VendorNamespaceId,
    pub name: String,
    pub version: String,
    pub required_protocol: ProtocolVersion,
    pub registry_generation: u64,
}

/// Stable validation failure categories suitable for CI output.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RegistryError {
    #[error("registry generation must be non-zero")]
    ZeroGeneration,
    #[error("registry manifest metadata is invalid")]
    InvalidManifest,
    #[error("registry exceeds its bounded entry limit")]
    RegistryLimit,
    #[error("duplicate registry key {domain}:{id}")]
    DuplicateId { domain: RegistryDomain, id: u32 },
    #[error("duplicate canonical name {domain}:{name}")]
    DuplicateName {
        domain: RegistryDomain,
        name: String,
    },
    #[error("invalid registry entry {domain}:{id}: {reason}")]
    InvalidEntry {
        domain: RegistryDomain,
        id: u32,
        reason: String,
    },
    #[error("unresolved registry reference {domain}:{id}")]
    UnresolvedReference { domain: RegistryDomain, id: u32 },
    #[error("published registry ID {domain}:{id} was deleted or reused")]
    PublishedIdChanged { domain: RegistryDomain, id: u32 },
    #[error("schema version regressed for {domain}:{id}")]
    SchemaRegression { domain: RegistryDomain, id: u32 },
    #[error("security-sensitive change {domain}:{id} lacks a proposal")]
    MissingSecurityReview { domain: RegistryDomain, id: u32 },
}
