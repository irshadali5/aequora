use crate::{LegacyRecordKey, LegacySystemId};
use aequora_types::{EntityId, EntityType};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyIdMapping {
    pub legacy_system_id: LegacySystemId,
    pub entity_type: EntityType,
    pub legacy_key: LegacyRecordKey,
    pub entity_id: EntityId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IdMapError {
    #[error("legacy or canonical identity is already mapped differently")]
    Conflict,
    #[error("legacy id mapping storage failed")]
    Storage,
}

#[async_trait]
pub trait LegacyIdMap: Send + Sync {
    async fn insert_unique(&self, mapping: LegacyIdMapping) -> Result<(), IdMapError>;
    async fn resolve_legacy(
        &self,
        system: LegacySystemId,
        key: &LegacyRecordKey,
    ) -> Result<Option<EntityId>, IdMapError>;
    async fn resolve_canonical(
        &self,
        entity: EntityId,
    ) -> Result<Option<LegacyIdMapping>, IdMapError>;
}
