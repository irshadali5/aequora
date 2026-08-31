//! Physical adapter schema migration contracts.

use crate::{AdapterError, Digest};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Stable physical migration identity. IDs are never reused.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct MigrationId(pub u32);

/// Physical Aequora adapter schema version.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct AdapterSchemaVersion(pub u32);

/// Application domain schema version, intentionally distinct from adapter schema.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct DomainSchemaVersion(pub u32);

/// Immutable physical migration descriptor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdapterMigration {
    /// Stable migration identity.
    pub id: MigrationId,
    /// Digest of the immutable migration body.
    pub checksum: Digest,
    /// Exact input physical schema.
    pub from: AdapterSchemaVersion,
    /// Exact output physical schema.
    pub to: AdapterSchemaVersion,
}

/// Deterministic migration plan validated before privileged execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationPlan {
    /// Current physical schema.
    pub current: AdapterSchemaVersion,
    /// Target physical schema.
    pub target: AdapterSchemaVersion,
    /// Ordered immutable steps.
    pub steps: Vec<AdapterMigration>,
}

impl MigrationPlan {
    /// Verifies non-zero IDs/versions, contiguous ordering, and unique migration IDs.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError`] when the plan is empty for a required transition, contains reused
    /// IDs, skips a version boundary, or does not reach its target.
    pub fn validate(&self) -> Result<(), AdapterError> {
        if self.current.0 == 0 || self.target.0 == 0 || self.target < self.current {
            return Err(AdapterError::invalid_configuration(
                "invalid adapter schema migration boundary",
            ));
        }
        if self.current == self.target {
            return if self.steps.is_empty() {
                Ok(())
            } else {
                Err(AdapterError::invalid_configuration(
                    "no-op migration plan must not contain steps",
                ))
            };
        }
        let mut expected = self.current;
        let mut ids = std::collections::BTreeSet::new();
        for step in &self.steps {
            if step.id.0 == 0
                || step.from != expected
                || step.to.0 != step.from.0.saturating_add(1)
                || !ids.insert(step.id)
            {
                return Err(AdapterError::invalid_configuration(
                    "adapter migrations must be unique, ordered, and contiguous",
                ));
            }
            expected = step.to;
        }
        if expected != self.target {
            return Err(AdapterError::invalid_configuration(
                "adapter migration plan does not reach target schema",
            ));
        }
        Ok(())
    }
}

/// Lifecycle hook around privileged physical migrations.
#[async_trait]
pub trait MigrationHook: Send + Sync {
    /// Runs before the first migration after the plan has validated.
    async fn before_migration(&self, plan: &MigrationPlan) -> Result<(), AdapterError>;

    /// Runs after every migration is durably recorded.
    async fn after_migration(&self, plan: &MigrationPlan) -> Result<(), AdapterError>;
}

/// Physical migration executor owned by an adapter crate.
#[async_trait]
pub trait MigrationStore: Send + Sync {
    /// Reads the current physical adapter schema version.
    async fn schema_version(&self) -> Result<AdapterSchemaVersion, AdapterError>;

    /// Applies and durably records an already validated deterministic plan.
    async fn migrate(&self, plan: &MigrationPlan) -> Result<(), AdapterError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_plan_rejects_gaps_and_reused_ids() {
        let plan = MigrationPlan {
            current: AdapterSchemaVersion(1),
            target: AdapterSchemaVersion(3),
            steps: vec![
                AdapterMigration {
                    id: MigrationId(1),
                    checksum: [1; 32],
                    from: AdapterSchemaVersion(1),
                    to: AdapterSchemaVersion(2),
                },
                AdapterMigration {
                    id: MigrationId(1),
                    checksum: [2; 32],
                    from: AdapterSchemaVersion(2),
                    to: AdapterSchemaVersion(3),
                },
            ],
        };
        assert!(plan.validate().is_err());
    }
}
