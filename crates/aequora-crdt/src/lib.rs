//! Small convergent data types and an opt-in Postcard conflict merger.

use aequora_conflict::{MergeDecision, MergeError, MergeInput, MergeStrategy};
use aequora_protocol::ChangeKind;
use aequora_types::NodeId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, marker::PhantomData};

/// State-based convergent replicated data type.
pub trait Crdt {
    /// Joins another replica state into this one. Implementations must be associative,
    /// commutative, and idempotent.
    fn merge(&mut self, other: &Self);
}

/// Grow-only counter with one monotonic component per replica.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct GCounter {
    components: BTreeMap<NodeId, u64>,
}

impl GCounter {
    /// Increases this replica's component by `amount` with saturation at `u64::MAX`.
    pub fn increment(&mut self, replica: NodeId, amount: u64) {
        let component = self.components.entry(replica).or_default();
        *component = component.saturating_add(amount);
    }

    /// Returns the convergent total with saturation at `u64::MAX`.
    #[must_use]
    pub fn value(&self) -> u64 {
        self.components
            .values()
            .copied()
            .fold(0, u64::saturating_add)
    }

    /// Returns one replica's monotonic component.
    #[must_use]
    pub fn component(&self, replica: NodeId) -> u64 {
        self.components.get(&replica).copied().unwrap_or(0)
    }
}

impl Crdt for GCounter {
    fn merge(&mut self, other: &Self) {
        for (&replica, &value) in &other.components {
            let component = self.components.entry(replica).or_default();
            *component = (*component).max(value);
        }
    }
}

/// Positive/negative counter composed from two grow-only counters.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PnCounter {
    positive: GCounter,
    negative: GCounter,
}

impl PnCounter {
    /// Adds a non-negative amount at one replica.
    pub fn increment(&mut self, replica: NodeId, amount: u64) {
        self.positive.increment(replica, amount);
    }

    /// Subtracts a non-negative amount at one replica.
    pub fn decrement(&mut self, replica: NodeId, amount: u64) {
        self.negative.increment(replica, amount);
    }

    /// Returns the signed convergent value.
    #[must_use]
    pub fn value(&self) -> i128 {
        i128::from(self.positive.value()) - i128::from(self.negative.value())
    }
}

impl Crdt for PnCounter {
    fn merge(&mut self, other: &Self) {
        self.positive.merge(&other.positive);
        self.negative.merge(&other.negative);
    }
}

/// Conflict strategy that decodes, joins, and re-encodes a state-based CRDT.
#[derive(Clone, Copy, Debug, Default)]
pub struct PostcardCrdtMerger<T> {
    marker: PhantomData<fn() -> T>,
}

impl<T> PostcardCrdtMerger<T> {
    /// Creates a stateless typed merger.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<T> MergeStrategy for PostcardCrdtMerger<T>
where
    T: Crdt + DeserializeOwned + Serialize + Send + Sync,
{
    fn merge(&self, input: MergeInput<'_>) -> Result<MergeDecision, MergeError> {
        if input.current_tombstone || input.candidate_kind == ChangeKind::Tombstone {
            return Ok(MergeDecision::Unresolved {
                message: "CRDT merge does not implicitly resolve deletion conflicts".to_owned(),
            });
        }
        let mut current: T = postcard::from_bytes(input.current_payload)
            .map_err(|_| MergeError::new("current CRDT state is malformed"))?;
        let candidate: T = postcard::from_bytes(input.candidate_payload)
            .map_err(|_| MergeError::new("candidate CRDT state is malformed"))?;
        current.merge(&candidate);
        let payload = postcard::to_stdvec(&current)
            .map_err(|_| MergeError::new("merged CRDT state could not be encoded"))?;
        Ok(MergeDecision::Merged {
            payload,
            change_kind: ChangeKind::Upsert,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_converge_independently_of_merge_order() {
        let left_node = NodeId::new();
        let right_node = NodeId::new();
        let mut left = PnCounter::default();
        left.increment(left_node, 7);
        left.decrement(left_node, 2);
        let mut right = PnCounter::default();
        right.increment(right_node, 5);
        right.decrement(right_node, 1);

        let mut left_first = left.clone();
        left_first.merge(&right);
        let mut right_first = right;
        right_first.merge(&left);
        assert_eq!(left_first, right_first);
        assert_eq!(left_first.value(), 9);

        let stable = left_first.clone();
        left_first.merge(&stable);
        assert_eq!(left_first, stable);
    }
}
