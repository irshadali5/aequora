//! Deterministic multi-region selection without weakening authoritative-write routing.

use aequora_types::{RegionId, Sequence};
use std::{collections::HashMap, time::Duration};
use thiserror::Error;

/// Serving responsibility of a deployment region.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionRole {
    /// Accepts authoritative operations.
    Primary,
    /// Serves reads only within an explicitly accepted replication lag.
    Replica,
}

/// Current routing health observation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RegionHealth {
    /// Normal routing candidate.
    Healthy,
    /// Usable only after healthy candidates.
    Degraded,
    /// Never selected.
    Unavailable,
}

/// One application-owned regional endpoint observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionState {
    /// Stable region identity.
    pub region_id: RegionId,
    /// Primary or read-replica role.
    pub role: RegionRole,
    /// Lower values are preferred after locality and health.
    pub priority: u16,
    /// Last measured request latency.
    pub latency: Duration,
    /// Approximate journal positions behind the authoritative primary.
    pub replication_lag: Sequence,
    /// Current availability observation.
    pub health: RegionHealth,
}

/// Consistency requirement for one routing decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingIntent {
    /// Must select a primary region.
    AuthoritativeWrite,
    /// Must select a primary region for a strongly consistent read.
    ConsistentRead,
    /// May select a replica within this journal-lag bound.
    BoundedStaleRead { maximum_lag: Sequence },
}

/// Why a region was selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteReason {
    /// Eligible local region won.
    Locality,
    /// Priority, health, and latency selected a remote region.
    BestAvailable,
}

/// Deterministic regional route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteDecision {
    /// Selected region.
    pub region_id: RegionId,
    /// Selection explanation.
    pub reason: RouteReason,
}

/// No region met the request's availability and consistency constraints.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("no region satisfies the requested consistency and health constraints")]
pub struct NoEligibleRegion;

/// Mutable catalog of regional health and lag observations.
#[derive(Clone, Debug, Default)]
pub struct RegionRouter {
    regions: HashMap<RegionId, RegionState>,
}

impl RegionRouter {
    /// Adds or replaces one region observation.
    pub fn observe(&mut self, state: RegionState) {
        self.regions.insert(state.region_id, state);
    }

    /// Removes a retired region.
    pub fn remove(&mut self, region_id: RegionId) -> Option<RegionState> {
        self.regions.remove(&region_id)
    }

    /// Selects an eligible route, preferring a healthy local region and then deterministic
    /// health, priority, latency, and identity ordering.
    ///
    /// # Errors
    ///
    /// Returns [`NoEligibleRegion`] when no region satisfies the requested consistency.
    pub fn select(
        &self,
        local_region: Option<RegionId>,
        intent: RoutingIntent,
    ) -> Result<RouteDecision, NoEligibleRegion> {
        let mut eligible: Vec<_> = self
            .regions
            .values()
            .copied()
            .filter(|state| eligible_for(*state, intent))
            .collect();
        eligible.sort_by_key(|state| {
            (
                state.health,
                usize::from(local_region != Some(state.region_id)),
                state.priority,
                state.latency,
                state.region_id,
            )
        });
        eligible.first().map_or(Err(NoEligibleRegion), |state| {
            Ok(RouteDecision {
                region_id: state.region_id,
                reason: if local_region == Some(state.region_id) {
                    RouteReason::Locality
                } else {
                    RouteReason::BestAvailable
                },
            })
        })
    }
}

fn eligible_for(state: RegionState, intent: RoutingIntent) -> bool {
    if state.health == RegionHealth::Unavailable {
        return false;
    }
    match intent {
        RoutingIntent::AuthoritativeWrite | RoutingIntent::ConsistentRead => {
            state.role == RegionRole::Primary
        }
        RoutingIntent::BoundedStaleRead { maximum_lag } => {
            state.role == RegionRole::Primary || state.replication_lag <= maximum_lag
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_never_route_to_a_low_latency_replica() {
        let primary = RegionId::new();
        let replica = RegionId::new();
        let mut router = RegionRouter::default();
        router.observe(RegionState {
            region_id: primary,
            role: RegionRole::Primary,
            priority: 10,
            latency: Duration::from_millis(100),
            replication_lag: Sequence(0),
            health: RegionHealth::Healthy,
        });
        router.observe(RegionState {
            region_id: replica,
            role: RegionRole::Replica,
            priority: 1,
            latency: Duration::from_millis(1),
            replication_lag: Sequence(1),
            health: RegionHealth::Healthy,
        });

        assert_eq!(
            router
                .select(Some(replica), RoutingIntent::AuthoritativeWrite)
                .unwrap_or_else(|error| panic!("{error}"))
                .region_id,
            primary
        );
        assert_eq!(
            router
                .select(
                    Some(replica),
                    RoutingIntent::BoundedStaleRead {
                        maximum_lag: Sequence(1),
                    },
                )
                .unwrap_or_else(|error| panic!("{error}"))
                .region_id,
            replica
        );
    }
}
