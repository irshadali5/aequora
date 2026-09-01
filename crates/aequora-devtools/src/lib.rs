//! Deterministic development scaffolding and production safety guards.
//!
//! This crate only builds plans. The CLI composition layer owns filesystem writes and must present
//! collisions and planned changes before applying them.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EnvironmentClass {
    Development,
    Test,
    Staging,
    Production,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Template {
    RustClient,
    RustServer,
    DioxusClient,
    AxumServer,
    PostgresAuthority,
    StoolapLocal,
    FullLocalFirst,
}

impl Template {
    pub const VERSION: u16 = 1;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FileOwnership {
    Generated,
    DeveloperOwned,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlannedFile {
    pub relative_path: String,
    pub ownership: FileOwnership,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScaffoldPlan {
    pub template: Template,
    pub template_version: u16,
    pub files: Vec<PlannedFile>,
    pub collisions: Vec<String>,
}

impl ScaffoldPlan {
    #[must_use]
    pub fn can_apply_without_overwrite(&self) -> bool {
        self.collisions.is_empty()
    }
}

/// Creates a deterministic scaffold plan without touching the filesystem.
#[must_use]
pub fn plan_scaffold(template: Template, existing: &BTreeSet<String>) -> ScaffoldPlan {
    let paths = [
        ("aequora.ron", FileOwnership::Generated),
        ("registry/registry.ron", FileOwnership::Generated),
        ("migrations/README.md", FileOwnership::Generated),
        ("docs/aequora-integration.md", FileOwnership::DeveloperOwned),
        ("src/aequora_composition.rs", FileOwnership::DeveloperOwned),
    ];
    let files = paths
        .into_iter()
        .map(|(relative_path, ownership)| PlannedFile {
            relative_path: relative_path.to_owned(),
            ownership,
        })
        .collect::<Vec<_>>();
    let collisions = files
        .iter()
        .filter(|file| existing.contains(&file.relative_path))
        .map(|file| file.relative_path.clone())
        .collect();
    ScaffoldPlan {
        template,
        template_version: Template::VERSION,
        files,
        collisions,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DevelopmentAction {
    Reset,
    Seed,
    FaultInjection,
    Fixture,
    Benchmark,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DevelopmentGuardError {
    #[error("development action refused for a production environment marker")]
    ProductionTarget,
    #[error("fault injection is absent from this build")]
    FailpointsNotBuilt,
}

/// Verifies explicit environment markers and build capabilities before a development action.
///
/// # Errors
///
/// Refuses destructive/fault actions against production and failpoints absent from the build.
pub fn authorize_development_action(
    environment: EnvironmentClass,
    action: DevelopmentAction,
) -> Result<(), DevelopmentGuardError> {
    if environment == EnvironmentClass::Production
        && matches!(
            action,
            DevelopmentAction::Reset
                | DevelopmentAction::Seed
                | DevelopmentAction::FaultInjection
                | DevelopmentAction::Benchmark
        )
    {
        return Err(DevelopmentGuardError::ProductionTarget);
    }
    if action == DevelopmentAction::FaultInjection && !cfg!(feature = "test-failpoints") {
        return Err(DevelopmentGuardError::FailpointsNotBuilt);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_is_deterministic_and_never_silently_overwrites() {
        let existing = BTreeSet::from(["aequora.ron".to_owned()]);
        let left = plan_scaffold(Template::FullLocalFirst, &existing);
        let right = plan_scaffold(Template::FullLocalFirst, &existing);
        assert_eq!(left, right);
        assert_eq!(left.collisions, ["aequora.ron"]);
        assert!(!left.can_apply_without_overwrite());
    }

    #[test]
    fn production_reset_is_rejected() {
        assert_eq!(
            authorize_development_action(EnvironmentClass::Production, DevelopmentAction::Reset),
            Err(DevelopmentGuardError::ProductionTarget)
        );
    }

    #[cfg(not(feature = "test-failpoints"))]
    #[test]
    fn production_build_excludes_failpoints() {
        assert_eq!(
            authorize_development_action(
                EnvironmentClass::Development,
                DevelopmentAction::FaultInjection
            ),
            Err(DevelopmentGuardError::FailpointsNotBuilt)
        );
    }
}
