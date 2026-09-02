//! Deterministic runtime feature rollout separated from Cargo features and business entitlement.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

macro_rules! stable_id {
    ($name:ident, $label:literal) => {
        #[doc = $label]
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            #[doc = concat!("Creates a non-empty ", $label, ".")]
            ///
            /// # Errors
            ///
            /// Rejects empty, whitespace-only, or excessively long identifiers.
            pub fn new(value: impl Into<String>) -> Result<Self, FeatureError> {
                let value = value.into();
                if value.trim().is_empty() || value.len() > 128 {
                    return Err(FeatureError::InvalidIdentifier);
                }
                Ok(Self(value))
            }

            /// Returns the governed identifier.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

stable_id!(FeatureId, "stable feature identifier");
stable_id!(TenantKey, "tenant rollout key");
stable_id!(CapabilityId, "negotiated capability identifier");
stable_id!(EntitlementId, "authoritative entitlement identifier");

/// Safety review class for a runtime feature.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FeatureSafety {
    PresentationOnly,
    Optimization,
    CompatibleBehavior,
    SemanticMigrationRequired,
    SecurityCritical,
}

/// Valid percentage in the inclusive range 0..=100.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Percentage(u8);

impl Percentage {
    /// Constructs a rollout percentage.
    ///
    /// # Errors
    ///
    /// Rejects values above 100.
    pub fn new(value: u8) -> Result<Self, FeatureError> {
        if value > 100 {
            return Err(FeatureError::InvalidPercentage);
        }
        Ok(Self(value))
    }

    /// Returns the percentage.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Percentage {
    type Error = FeatureError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Percentage> for u8 {
    fn from(value: Percentage) -> Self {
        value.get()
    }
}

/// Governed rollout stage.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RolloutStage {
    Off,
    Internal,
    Canary,
    Percentage(Percentage),
    TenantAllowlist(BTreeSet<TenantKey>),
    On,
}

/// Versioned feature declaration. It contains no business entitlement grants.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureDefinition {
    pub id: FeatureId,
    pub safety: FeatureSafety,
    pub rollout_generation: u64,
    pub stage: RolloutStage,
    pub required_capability: Option<CapabilityId>,
    pub required_entitlement: Option<EntitlementId>,
    /// Explicit semantic version or migration identifier for a semantic rollout.
    pub semantic_migration: Option<String>,
    /// Recorded differential-test evidence for a compatible implementation path.
    pub differential_evidence: bool,
}

impl FeatureDefinition {
    /// Validates that rollout mechanics cannot silently change semantics or security policy.
    ///
    /// # Errors
    ///
    /// Rejects security experimentation, unsafe semantic rollout, and unevidenced compatible
    /// behavior.
    pub fn validate(&self) -> Result<(), FeatureError> {
        if self.rollout_generation == 0 {
            return Err(FeatureError::InvalidGeneration);
        }
        match self.safety {
            FeatureSafety::SecurityCritical if self.stage != RolloutStage::On => {
                return Err(FeatureError::SecurityPolicyIsNotAFlag);
            }
            FeatureSafety::SemanticMigrationRequired => {
                if self.semantic_migration.as_deref().is_none_or(str::is_empty)
                    || self.required_capability.is_none()
                    || matches!(
                        self.stage,
                        RolloutStage::Percentage(_) | RolloutStage::Canary
                    )
                {
                    return Err(FeatureError::SemanticMigrationRequired);
                }
            }
            FeatureSafety::CompatibleBehavior if !self.differential_evidence => {
                return Err(FeatureError::DifferentialEvidenceRequired);
            }
            _ => {}
        }
        Ok(())
    }
}

/// Evaluation inputs. Entitlements are supplied from authoritative business state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FeatureContext {
    pub tenant: Option<TenantKey>,
    pub internal: bool,
    pub negotiated_capabilities: BTreeSet<CapabilityId>,
    pub entitlements: BTreeSet<EntitlementId>,
}

/// Reason for a safe enable/disable decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionReason {
    Enabled,
    StageDisabled,
    MissingTenant,
    MissingCapability,
    MissingEntitlement,
    InvalidDefinition,
}

/// Complete feature decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeatureDecision {
    pub enabled: bool,
    pub reason: DecisionReason,
    pub rollout_generation: u64,
}

/// Stable feature evaluation boundary.
pub trait FeatureEvaluator {
    fn evaluate(&self, feature: &FeatureId, context: &FeatureContext) -> FeatureDecision;
}

/// Immutable map-backed evaluator suitable for one feature-policy generation.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct FeaturePolicy(BTreeMap<FeatureId, FeatureDefinition>);

impl FeaturePolicy {
    /// Builds a policy and rejects duplicate IDs or unsafe declarations.
    ///
    /// # Errors
    ///
    /// Returns [`FeatureError`] for duplicates or invalid declarations.
    pub fn new(
        definitions: impl IntoIterator<Item = FeatureDefinition>,
    ) -> Result<Self, FeatureError> {
        let mut policy = BTreeMap::new();
        for definition in definitions {
            definition.validate()?;
            if policy.insert(definition.id.clone(), definition).is_some() {
                return Err(FeatureError::DuplicateFeature);
            }
        }
        Ok(Self(policy))
    }

    /// Returns the governed definitions in stable identifier order.
    pub fn definitions(&self) -> impl Iterator<Item = &FeatureDefinition> {
        self.0.values()
    }
}

impl FeatureEvaluator for FeaturePolicy {
    fn evaluate(&self, feature: &FeatureId, context: &FeatureContext) -> FeatureDecision {
        let Some(definition) = self.0.get(feature) else {
            return disabled(DecisionReason::StageDisabled, 0);
        };
        if definition.validate().is_err() {
            return disabled(
                DecisionReason::InvalidDefinition,
                definition.rollout_generation,
            );
        }
        if definition
            .required_capability
            .as_ref()
            .is_some_and(|capability| !context.negotiated_capabilities.contains(capability))
        {
            return disabled(
                DecisionReason::MissingCapability,
                definition.rollout_generation,
            );
        }
        if definition
            .required_entitlement
            .as_ref()
            .is_some_and(|entitlement| !context.entitlements.contains(entitlement))
        {
            return disabled(
                DecisionReason::MissingEntitlement,
                definition.rollout_generation,
            );
        }
        let enabled = match &definition.stage {
            RolloutStage::Off => false,
            RolloutStage::Internal => context.internal,
            RolloutStage::Canary => context.tenant.as_ref().is_some_and(|tenant| {
                stable_bucket(&definition.id, tenant, definition.rollout_generation) < 1
            }),
            RolloutStage::Percentage(percentage) => context.tenant.as_ref().is_some_and(|tenant| {
                stable_bucket(&definition.id, tenant, definition.rollout_generation)
                    < u64::from(percentage.get())
            }),
            RolloutStage::TenantAllowlist(tenants) => context
                .tenant
                .as_ref()
                .is_some_and(|tenant| tenants.contains(tenant)),
            RolloutStage::On => true,
        };
        if enabled {
            FeatureDecision {
                enabled: true,
                reason: DecisionReason::Enabled,
                rollout_generation: definition.rollout_generation,
            }
        } else {
            disabled(
                if context.tenant.is_none()
                    && matches!(
                        definition.stage,
                        RolloutStage::Canary
                            | RolloutStage::Percentage(_)
                            | RolloutStage::TenantAllowlist(_)
                    )
                {
                    DecisionReason::MissingTenant
                } else {
                    DecisionReason::StageDisabled
                },
                definition.rollout_generation,
            )
        }
    }
}

fn disabled(reason: DecisionReason, rollout_generation: u64) -> FeatureDecision {
    FeatureDecision {
        enabled: false,
        reason,
        rollout_generation,
    }
}

fn stable_bucket(feature: &FeatureId, tenant: &TenantKey, generation: u64) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aequora-feature-assignment-v1\0");
    hasher.update(feature.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(tenant.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(&generation.to_le_bytes());
    let mut prefix = [0_u8; 8];
    prefix.copy_from_slice(&hasher.finalize().as_bytes()[..8]);
    u64::from_le_bytes(prefix) % 100
}

/// Offline cache metadata. Expired or mismatched caches must fail to disabled decisions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureCache {
    pub schema_version: u16,
    pub policy_generation: u64,
    pub expires_at_unix_ms: u64,
    pub policy: FeaturePolicy,
}

impl FeatureCache {
    /// Returns the cached policy only while its schema, generation, and expiry are valid.
    #[must_use]
    pub fn valid_policy(
        &self,
        expected_generation: u64,
        now_unix_ms: u64,
    ) -> Option<&FeaturePolicy> {
        (self.schema_version == 1
            && self.policy_generation == expected_generation
            && now_unix_ms < self.expires_at_unix_ms)
            .then_some(&self.policy)
    }
}

/// Feature definition or policy error.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum FeatureError {
    #[error("feature identifier is invalid")]
    InvalidIdentifier,
    #[error("feature percentage is outside 0..=100")]
    InvalidPercentage,
    #[error("feature rollout generation must be non-zero")]
    InvalidGeneration,
    #[error("security policy cannot be weakened through a runtime feature flag")]
    SecurityPolicyIsNotAFlag,
    #[error("semantic feature rollout requires an explicit migration and capability")]
    SemanticMigrationRequired,
    #[error("compatible behavior rollout requires differential-test evidence")]
    DifferentialEvidenceRequired,
    #[error("feature identifier is duplicated")]
    DuplicateFeature,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(value: &str) -> FeatureId {
        FeatureId::new(value).unwrap_or_else(|error| panic!("{error}"))
    }

    fn tenant(value: &str) -> TenantKey {
        TenantKey::new(value).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn percentage_assignment_is_stable_and_generation_bound() {
        let definition = FeatureDefinition {
            id: feature("new-read-path"),
            safety: FeatureSafety::Optimization,
            rollout_generation: 7,
            stage: RolloutStage::Percentage(
                Percentage::new(37).unwrap_or_else(|error| panic!("{error}")),
            ),
            required_capability: None,
            required_entitlement: None,
            semantic_migration: None,
            differential_evidence: false,
        };
        let policy = FeaturePolicy::new([definition]).unwrap_or_else(|error| panic!("{error}"));
        let context = FeatureContext {
            tenant: Some(tenant("tenant-a")),
            ..FeatureContext::default()
        };
        let first = policy.evaluate(&feature("new-read-path"), &context);
        for _ in 0..100 {
            assert_eq!(policy.evaluate(&feature("new-read-path"), &context), first);
        }
    }

    #[test]
    fn entitlement_is_required_but_never_granted_by_the_flag() {
        let entitlement =
            EntitlementId::new("advanced-accounting").unwrap_or_else(|error| panic!("{error}"));
        let definition = FeatureDefinition {
            id: feature("advanced-ui"),
            safety: FeatureSafety::PresentationOnly,
            rollout_generation: 1,
            stage: RolloutStage::On,
            required_capability: None,
            required_entitlement: Some(entitlement.clone()),
            semantic_migration: None,
            differential_evidence: false,
        };
        let policy = FeaturePolicy::new([definition]).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            policy
                .evaluate(&feature("advanced-ui"), &FeatureContext::default())
                .reason,
            DecisionReason::MissingEntitlement
        );
        let context = FeatureContext {
            entitlements: [entitlement].into(),
            ..FeatureContext::default()
        };
        assert!(policy.evaluate(&feature("advanced-ui"), &context).enabled);
    }

    #[test]
    fn unsafe_semantic_and_security_flags_are_rejected() {
        let security = FeatureDefinition {
            id: feature("disable-auth"),
            safety: FeatureSafety::SecurityCritical,
            rollout_generation: 1,
            stage: RolloutStage::Off,
            required_capability: None,
            required_entitlement: None,
            semantic_migration: None,
            differential_evidence: false,
        };
        assert_eq!(
            security.validate(),
            Err(FeatureError::SecurityPolicyIsNotAFlag)
        );
    }
}
