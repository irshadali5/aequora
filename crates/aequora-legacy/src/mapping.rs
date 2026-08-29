use crate::{LegacyRecordKey, LegacySourcePosition, LegacySystemId};
use aequora_types::{EntityId, EntityVersion, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyRecord {
    pub key: LegacyRecordKey,
    pub payload: Vec<u8>,
    pub source_revision: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CanonicalEntity {
    pub tenant_id: TenantId,
    pub entity_id: EntityId,
    pub entity_version: EntityVersion,
    pub canonical_payload: Vec<u8>,
    pub deleted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LegacyProvenance {
    pub system_id: LegacySystemId,
    pub record_key: LegacyRecordKey,
    pub source_position: LegacySourcePosition,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MappingClassification {
    Valid,
    Repairable,
    Quarantined,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MappingOutcome {
    pub classification: MappingClassification,
    pub entity: Option<CanonicalEntity>,
    pub reason_code: Option<String>,
}

impl MappingOutcome {
    pub fn validated(self) -> Result<CanonicalEntity, MappingError> {
        match (self.classification, self.entity) {
            (MappingClassification::Valid, Some(entity)) => Ok(entity),
            (classification, _) => Err(MappingError::NotValid(classification)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MappingError {
    #[error("legacy business state is not valid canonical data: {0:?}")]
    NotValid(MappingClassification),
    #[error("legacy mapping failed: {0}")]
    Invalid(String),
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("legacy read failed: {message}")]
pub struct LegacyReadError {
    pub message: String,
}

#[async_trait]
pub trait LegacyReader: Send + Sync {
    async fn fetch_entity(&self, key: &LegacyRecordKey) -> Result<LegacyRecord, LegacyReadError>;
}

pub trait LegacyMapper: Send + Sync {
    fn map(&self, record: LegacyRecord) -> Result<MappingOutcome, MappingError>;
}
