//! Fleet-aware rolling activation and adapter compatibility manifests.

use crate::{AdapterApiVersion, CapabilityId, CompatibilityError, LocalStoreFormatVersion};
use aequora_types::{NodeId, TenantId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Rollout state for one server capability.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FeatureState {
    Disabled,
    Shadow,
    EnabledForCanary,
    Enabled,
    Required,
}

/// One serving node's runtime interoperability manifest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NodeCapabilities {
    pub node_id: NodeId,
    pub build_number: u64,
    pub serving: bool,
    pub capabilities: BTreeSet<CapabilityId>,
}

/// Atomic control-plane observation used before feature activation.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct FleetCapabilities {
    pub nodes: Vec<NodeCapabilities>,
}

impl FleetCapabilities {
    /// Capabilities present on every currently serving path.
    #[must_use]
    pub fn common_serving_capabilities(&self) -> BTreeSet<CapabilityId> {
        let mut serving = self.nodes.iter().filter(|node| node.serving);
        let Some(first) = serving.next() else {
            return BTreeSet::new();
        };
        serving.fold(first.capabilities.clone(), |common, node| {
            common.intersection(&node.capabilities).copied().collect()
        })
    }

    #[must_use]
    pub fn all_serving_support(&self, capability: CapabilityId) -> bool {
        let serving = self.nodes.iter().filter(|node| node.serving);
        let mut count = 0_usize;
        for node in serving {
            count = count.saturating_add(1);
            if !node.capabilities.contains(&capability) {
                return false;
            }
        }
        count > 0
    }
}

/// Server-controlled rollout; clients cannot self-select a safety-sensitive canary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServerFeatureGate {
    pub capability: CapabilityId,
    pub state: FeatureState,
    pub canary_tenants: BTreeSet<TenantId>,
}

impl ServerFeatureGate {
    /// Applies a rollout or rollback state after fleet-safety validation.
    ///
    /// # Errors
    ///
    /// Returns [`CompatibilityError::FleetCapabilityIncomplete`] when an enabled state would
    /// reach a serving node that lacks the capability.
    pub fn transition(
        &mut self,
        next: FeatureState,
        fleet: &FleetCapabilities,
    ) -> Result<(), CompatibilityError> {
        if matches!(next, FeatureState::Enabled | FeatureState::Required)
            && !fleet.all_serving_support(self.capability)
        {
            return Err(CompatibilityError::FleetCapabilityIncomplete);
        }
        self.state = next;
        Ok(())
    }

    #[must_use]
    pub fn enabled_for(&self, tenant_id: TenantId) -> bool {
        match self.state {
            FeatureState::Disabled | FeatureState::Shadow => false,
            FeatureState::EnabledForCanary => self.canary_tenants.contains(&tenant_id),
            FeatureState::Enabled | FeatureState::Required => true,
        }
    }
}

/// Runtime adapter interoperability metadata, separate from crate `SemVer`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdapterCompatibilityManifest {
    pub api_version: AdapterApiVersion,
    pub store_formats: BTreeSet<LocalStoreFormatVersion>,
    pub capabilities: BTreeSet<CapabilityId>,
}

impl AdapterCompatibilityManifest {
    /// Startup certification against the selected deployment profile.
    ///
    /// # Errors
    ///
    /// Returns [`CompatibilityError::RequiredCapabilityUnavailable`] for an API, store-format,
    /// or capability mismatch.
    pub fn certify(
        &self,
        required_api: AdapterApiVersion,
        required_store_format: LocalStoreFormatVersion,
        required_capabilities: &BTreeSet<CapabilityId>,
    ) -> Result<(), CompatibilityError> {
        if self.api_version != required_api
            || !self.store_formats.contains(&required_store_format)
            || !required_capabilities.is_subset(&self.capabilities)
        {
            return Err(CompatibilityError::RequiredCapabilityUnavailable);
        }
        Ok(())
    }
}
