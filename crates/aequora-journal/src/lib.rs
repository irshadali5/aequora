//! Cursor watermarks and conservative tombstone-retention decisions.

use aequora_protocol::{ChangeKind, RemoteChange};
use aequora_types::{Cursor, DeviceId, Sequence, SyncScopeId};
use std::{collections::HashMap, time::Duration};
use thiserror::Error;

/// Conservative tombstone garbage-collection policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TombstoneRetention {
    /// Minimum wall-clock age before a tombstone is eligible.
    pub minimum_age: Duration,
}

/// Returns whether a tombstone is old enough and observed by every active device.
///
/// A `None` watermark is deliberately unsafe: without an active-device lower bound the
/// caller cannot prove that offline clients have observed the deletion.
#[must_use]
pub fn tombstone_collectable(
    change: &RemoteChange,
    now_ms: i64,
    retention: TombstoneRetention,
    minimum_active_cursor: Option<Sequence>,
) -> bool {
    if change.change_kind != ChangeKind::Tombstone || change.timestamp.physical_ms > now_ms {
        return false;
    }
    let age_ms = now_ms.saturating_sub(change.timestamp.physical_ms);
    let required_ms = i64::try_from(retention.minimum_age.as_millis()).unwrap_or(i64::MAX);
    age_ms >= required_ms && minimum_active_cursor.is_some_and(|cursor| cursor >= change.sequence)
}

#[derive(Clone, Copy, Debug)]
struct DeviceWatermark {
    cursor: Cursor,
    last_seen_ms: i64,
}

/// Tracks per-device scoped cursors for retention and resynchronization decisions.
#[derive(Default)]
pub struct CursorWatermarks {
    devices: HashMap<(DeviceId, SyncScopeId), DeviceWatermark>,
}

impl CursorWatermarks {
    /// Records durable device progress, rejecting cursor regression.
    ///
    /// # Errors
    ///
    /// Returns [`WatermarkError::Regression`] if the new cursor moves backward.
    pub fn update(
        &mut self,
        device: DeviceId,
        cursor: Cursor,
        last_seen_ms: i64,
    ) -> Result<(), WatermarkError> {
        let key = (device, cursor.scope);
        if self
            .devices
            .get(&key)
            .is_some_and(|existing| cursor.sequence < existing.cursor.sequence)
        {
            return Err(WatermarkError::Regression);
        }
        self.devices.insert(
            key,
            DeviceWatermark {
                cursor,
                last_seen_ms,
            },
        );
        Ok(())
    }

    /// Returns the lowest cursor among devices still considered active in a scope.
    #[must_use]
    pub fn minimum_active_cursor(
        &self,
        scope: SyncScopeId,
        now_ms: i64,
        inactive_after: Duration,
    ) -> Option<Sequence> {
        let active_since =
            now_ms.saturating_sub(i64::try_from(inactive_after.as_millis()).unwrap_or(i64::MAX));
        self.devices
            .values()
            .filter(|watermark| {
                watermark.cursor.scope == scope && watermark.last_seen_ms >= active_since
            })
            .map(|watermark| watermark.cursor.sequence)
            .min()
    }
}

/// Invalid device cursor transition.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum WatermarkError {
    /// A durable cursor must never move backward.
    #[error("device cursor cannot move backward")]
    Regression,
}

/// Inputs proving how far a synchronization journal may be compacted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompactionInputs {
    /// Latest sequence represented by a retained consistent snapshot.
    pub snapshot_sequence: Sequence,
    /// Lowest cursor held by any active device. Missing means compaction is unsafe.
    pub minimum_active_cursor: Option<Sequence>,
    /// Highest sequence old enough under synchronization retention policy.
    pub retention_sequence: Sequence,
    /// Optional upper bound when the sync journal also serves audit retention.
    pub audit_sequence: Option<Sequence>,
}

/// Conservative inclusive journal-deletion boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompactionPlan {
    /// Entries at or below this sequence may be removed from the sync journal.
    pub through: Sequence,
}

/// Plans compaction only when a snapshot and every active device make deletion safe.
#[must_use]
pub fn plan_compaction(inputs: CompactionInputs) -> Option<CompactionPlan> {
    let active = inputs.minimum_active_cursor?;
    let mut through = inputs
        .snapshot_sequence
        .min(active)
        .min(inputs.retention_sequence);
    if let Some(audit) = inputs.audit_sequence {
        through = through.min(audit);
    }
    (through.0 > 0).then_some(CompactionPlan { through })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_types::{
        EntityId, EntityRef, EntityType, EntityVersion, HybridTimestamp, NodeId, OperationId,
        TenantId,
    };

    #[test]
    fn slowest_active_device_controls_tombstone_collection() {
        let scope = SyncScopeId::new();
        let mut watermarks = CursorWatermarks::default();
        assert!(
            watermarks
                .update(
                    DeviceId::new(),
                    Cursor {
                        scope,
                        sequence: Sequence(10)
                    },
                    10_000,
                )
                .is_ok()
        );
        assert!(
            watermarks
                .update(
                    DeviceId::new(),
                    Cursor {
                        scope,
                        sequence: Sequence(5)
                    },
                    10_000,
                )
                .is_ok()
        );
        let change = RemoteChange {
            tenant_id: TenantId::new(),
            scope_id: scope,
            sequence: Sequence(6),
            operation_id: OperationId::new(),
            entity: EntityRef {
                entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
                entity_id: EntityId::new(),
            },
            version: EntityVersion::INITIAL,
            change_kind: ChangeKind::Tombstone,
            payload: Vec::new(),
            timestamp: HybridTimestamp {
                physical_ms: 1_000,
                logical: 0,
                node: NodeId::new(),
            },
        };
        let minimum = watermarks.minimum_active_cursor(scope, 10_000, Duration::from_secs(60));
        assert!(!tombstone_collectable(
            &change,
            10_000,
            TombstoneRetention {
                minimum_age: Duration::from_secs(1)
            },
            minimum,
        ));
    }

    #[test]
    fn compaction_uses_the_most_conservative_boundary() {
        let plan = plan_compaction(CompactionInputs {
            snapshot_sequence: Sequence(100),
            minimum_active_cursor: Some(Sequence(80)),
            retention_sequence: Sequence(90),
            audit_sequence: Some(Sequence(70)),
        });
        assert_eq!(
            plan,
            Some(CompactionPlan {
                through: Sequence(70)
            })
        );
        assert_eq!(
            plan_compaction(CompactionInputs {
                snapshot_sequence: Sequence(100),
                minimum_active_cursor: None,
                retention_sequence: Sequence(90),
                audit_sequence: None,
            }),
            None
        );
    }
}
