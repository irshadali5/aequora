//! Ordered, checksummed internal metadata migrations.

use crate::{
    LocalStoreGeneration, MetadataMigrationId, MetadataMigrationRecord, MetadataRoot,
    MetadataSchemaVersion, PersistenceError, Timestamp,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MigrationPhase {
    Expand,
    Backfill,
    Switch,
    Contract,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataMigration {
    pub id: MetadataMigrationId,
    pub from: MetadataSchemaVersion,
    pub to: MetadataSchemaVersion,
    pub phase: MigrationPhase,
    pub checksum: [u8; 32],
    pub preserves_pending_intent: bool,
    pub requires_exclusive_maintenance: bool,
}

impl MetadataMigration {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.to <= self.from || self.checksum == [0; 32] {
            return Err(PersistenceError::InvalidRecord(
                "invalid metadata migration identity".into(),
            ));
        }
        if !self.preserves_pending_intent {
            return Err(PersistenceError::ConstraintViolation(
                "metadata migration does not preserve pending user intent".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationJournal {
    pub applied: Vec<MetadataMigrationRecord>,
}

impl MigrationJournal {
    /// Rejects duplicate IDs, non-monotonic order, and changed checksums.
    pub fn validate(&self) -> Result<(), PersistenceError> {
        let mut ids = BTreeSet::new();
        let mut previous = 0;
        for record in &self.applied {
            let id = record.migration_id.get();
            if id <= previous || !ids.insert(id) || record.checksum == [0; 32] {
                return Err(PersistenceError::CorruptionDetected(
                    "migration journal is not monotonic and checksummed".into(),
                ));
            }
            previous = id;
        }
        Ok(())
    }

    pub fn verify_checksum(&self, migration: &MetadataMigration) -> Result<(), PersistenceError> {
        if self
            .applied
            .iter()
            .find(|record| record.migration_id == migration.id)
            .is_some_and(|record| record.checksum != migration.checksum)
        {
            return Err(PersistenceError::CorruptionDetected(
                "migration ID is retained with a different checksum".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreOpenDisposition {
    Ready,
    MigrationRequired,
}

/// Fails closed before normal operation when schema compatibility is unknown.
pub fn validate_store_open(
    root: MetadataRoot,
    oldest_readable: MetadataSchemaVersion,
    current: MetadataSchemaVersion,
) -> Result<StoreOpenDisposition, PersistenceError> {
    if root.schema_version > current {
        return Err(PersistenceError::SchemaTooNew);
    }
    if root.schema_version < oldest_readable {
        return Err(PersistenceError::MigrationRequired);
    }
    if root.schema_version < current {
        Ok(StoreOpenDisposition::MigrationRequired)
    } else {
        Ok(StoreOpenDisposition::Ready)
    }
}

/// Evidence that a local migration preserved unsynchronized intent or rolled back entirely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientMigrationEvidence {
    pub generation_before: LocalStoreGeneration,
    pub generation_after: LocalStoreGeneration,
    pub pending_operations_before: u64,
    pub pending_operations_after: u64,
    pub committed: bool,
}

impl ClientMigrationEvidence {
    pub fn validate(self) -> Result<(), PersistenceError> {
        if self.committed && self.pending_operations_after < self.pending_operations_before {
            return Err(PersistenceError::ConstraintViolation(
                "committed migration discarded pending operations".into(),
            ));
        }
        if !self.committed
            && (self.generation_after != self.generation_before
                || self.pending_operations_after != self.pending_operations_before)
        {
            return Err(PersistenceError::CorruptionDetected(
                "failed migration left a partial durable change".into(),
            ));
        }
        Ok(())
    }
}

#[must_use]
pub fn migration_record(
    migration: &MetadataMigration,
    applied_at: Timestamp,
    binary_version: impl Into<String>,
) -> MetadataMigrationRecord {
    MetadataMigrationRecord {
        migration_id: migration.id,
        applied_at,
        binary_version: binary_version.into(),
        checksum: migration.checksum,
    }
}
