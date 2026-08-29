use aequora_types::{EntityType, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

uuid_id!(
    LegacySystemId,
    "Stable identity of an external legacy system."
);
uuid_id!(
    LegacyMigrationId,
    "Stable identity of one legacy migration."
);

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LegacyRecordKey(Vec<u8>);

impl LegacyRecordKey {
    #[must_use]
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LegacySourcePosition(Vec<u8>);

impl LegacySourcePosition {
    #[must_use]
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct LegacyMappingVersion(u32);

impl LegacyMappingVersion {
    pub const INITIAL: Self = Self(1);
    pub fn new(value: u32) -> Result<Self, ManifestError> {
        (value > 0)
            .then_some(Self(value))
            .ok_or(ManifestError::ZeroMappingVersion)
    }
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LegacyDatabaseKind {
    Postgres,
    MySql,
    SqlServer,
    ApplicationLog,
    Polling,
    Other,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AdoptionStage {
    Observe,
    CanonicalRead,
    CdcBridge,
    SelectedOperations,
    AequoraAuthoritative,
    LegacyFacade,
    Retired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct LegacyCredentialRef(pub String);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyCollection {
    pub name: String,
    pub candidate_aggregate: Option<EntityType>,
    pub primary_keys: Vec<String>,
    pub timestamp_fields: Vec<String>,
    pub soft_delete_semantics: Option<String>,
    pub transaction_boundary: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacySystemManifest {
    pub system_id: LegacySystemId,
    pub database_kind: LegacyDatabaseKind,
    pub collections: Vec<LegacyCollection>,
    pub triggers: Vec<String>,
    pub procedures: Vec<String>,
    pub scheduled_writers: Vec<String>,
    pub schema_digest: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyMigrationManifest {
    pub migration_id: LegacyMigrationId,
    pub legacy_system_id: LegacySystemId,
    pub tenant_id: TenantId,
    pub aggregate_types: Vec<EntityType>,
    pub source_schema_digest: [u8; 32],
    pub mapping_version: LegacyMappingVersion,
    pub decoder_version: u32,
    pub cutover_position: Option<LegacySourcePosition>,
    pub ownership_generation: u64,
    pub created_at_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyRetirementManifest {
    pub migration_id: LegacyMigrationId,
    pub final_source_position: LegacySourcePosition,
    pub archive_reference: String,
    pub final_ownership_generation: u64,
    pub retain_id_map: bool,
    pub retired_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SchemaDriftPolicy {
    Exact,
    AdditiveColumnsAllowed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaDrift {
    None,
    CompatibleAdditive,
    Breaking,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ManifestError {
    #[error("mapping version must be non-zero")]
    ZeroMappingVersion,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LegacyRegistryError {
    #[error("legacy registry storage failed")]
    Storage,
    #[error("legacy stage transition is invalid")]
    InvalidStageTransition,
}

#[async_trait]
pub trait LegacyDiscovery: Send + Sync {
    async fn discover(&self) -> Result<LegacySystemManifest, LegacyRegistryError>;
}

#[async_trait]
pub trait LegacyRegistryStore: Send + Sync {
    async fn register_system(
        &self,
        manifest: &LegacySystemManifest,
    ) -> Result<(), LegacyRegistryError>;
    async fn save_migration(
        &self,
        manifest: &LegacyMigrationManifest,
    ) -> Result<(), LegacyRegistryError>;
    async fn transition_stage(
        &self,
        migration: LegacyMigrationId,
        expected: AdoptionStage,
        next: AdoptionStage,
    ) -> Result<(), LegacyRegistryError>;
    async fn retire(&self, manifest: &LegacyRetirementManifest) -> Result<(), LegacyRegistryError>;
}
