use aequora_types::{EntityType, TenantId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WriteOwner {
    Legacy,
    Aequora,
    Migrating,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AggregateOwnership {
    pub tenant_id: TenantId,
    pub aggregate_type: EntityType,
    pub owner: WriteOwner,
    pub generation: u64,
    pub updated_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WritePath {
    LegacyDirect,
    LegacyFacade,
    AequoraNative,
    ReverseBridge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OwnershipError {
    #[error("write path does not own this aggregate")]
    NotOwner,
    #[error("ownership generation is stale")]
    StaleGeneration,
    #[error("a direct legacy write occurred after cutover")]
    LegacyWriteAfterCutover,
    #[error("ownership storage failed")]
    Storage,
}

impl AggregateOwnership {
    pub fn authorize(
        &self,
        path: WritePath,
        assumed_generation: u64,
    ) -> Result<(), OwnershipError> {
        if assumed_generation != self.generation {
            return Err(OwnershipError::StaleGeneration);
        }
        match (self.owner, path) {
            (WriteOwner::Legacy | WriteOwner::Migrating, WritePath::LegacyDirect)
            | (WriteOwner::Aequora, WritePath::LegacyFacade | WritePath::AequoraNative) => Ok(()),
            (WriteOwner::Aequora, WritePath::LegacyDirect) => {
                Err(OwnershipError::LegacyWriteAfterCutover)
            }
            _ => Err(OwnershipError::NotOwner),
        }
    }
}

#[async_trait]
pub trait OwnershipStore: Send + Sync {
    async fn load(
        &self,
        tenant: TenantId,
        aggregate: EntityType,
    ) -> Result<AggregateOwnership, OwnershipError>;
    async fn compare_and_set(
        &self,
        expected: &AggregateOwnership,
        next: AggregateOwnership,
    ) -> Result<(), OwnershipError>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SideEffectOwner {
    Legacy,
    Aequora,
    DisabledDuringShadow,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WriterInventoryEntry {
    pub writer_id: String,
    pub kind: WriterKind,
    pub classification: WriterClassification,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WriterKind {
    Api,
    Cron,
    AdminScript,
    Trigger,
    BatchImport,
    SupportTool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WriterClassification {
    Fenced,
    RewiredToFacade,
    RemainsLegacy,
    Unknown,
}
