//! Machine-readable Part 34 workspace boundary policy and Guppy validation.

use std::{collections::BTreeMap, error::Error, io};

use guppy::graph::{DependencyDirection, PackageGraph, PackageMetadata};
use serde::Deserialize;

const POLICY_SOURCE: &str = include_str!("../../../architecture/workspace-boundaries.ron");

#[derive(Debug, Deserialize)]
struct WorkspacePolicy {
    schema_version: u32,
    layers: Vec<Layer>,
    allowed_outward_edges: Vec<AllowedEdge>,
    dependency_budgets: Vec<DependencyBudget>,
    exclusive_dependencies: Vec<ExclusiveDependency>,
}

#[derive(Debug, Deserialize)]
struct Layer {
    name: String,
    crates: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AllowedEdge {
    source: String,
    dependency: String,
}

#[derive(Debug, Deserialize)]
struct DependencyBudget {
    crate_name: String,
    max_workspace: usize,
    max_external: usize,
}

#[derive(Debug, Deserialize)]
struct ExclusiveDependency {
    dependency: String,
    owners: Vec<String>,
}

pub(super) fn check(graph: &PackageGraph) -> Result<(), Box<dyn Error>> {
    let policy = parse_policy()?;
    let assignments = assignments(&policy)?;
    let mut violations = Vec::new();
    validate_layer_edges(graph, &policy, &assignments, &mut violations);
    validate_budgets(graph, &policy, &mut violations)?;
    validate_exclusive_dependencies(graph, &policy, &mut violations);

    if violations.is_empty() {
        println!(
            "workspace-architecture: ok ({} layers, {} crates, {} budgets, {} isolated dependencies)",
            policy.layers.len(),
            assignments.len(),
            policy.dependency_budgets.len(),
            policy.exclusive_dependencies.len(),
        );
        Ok(())
    } else {
        for violation in &violations {
            eprintln!("workspace-architecture: {violation}");
        }
        Err(io::Error::other(format!(
            "{} workspace-architecture violation(s)",
            violations.len()
        ))
        .into())
    }
}

fn validate_layer_edges(
    graph: &PackageGraph,
    policy: &WorkspacePolicy,
    assignments: &BTreeMap<&str, usize>,
    violations: &mut Vec<String>,
) {
    for package in graph
        .resolve_workspace()
        .packages(DependencyDirection::Forward)
    {
        let source = package.name();
        let Some(&source_layer) = assignments.get(source) else {
            violations.push(format!(
                "workspace crate {source} is not assigned to a layer"
            ));
            continue;
        };

        for dependency in production_links(&package) {
            let target = dependency.to();
            if target.in_workspace() {
                let Some(&target_layer) = assignments.get(target.name()) else {
                    violations.push(format!(
                        "workspace dependency {} of {source} is not assigned to a layer",
                        target.name()
                    ));
                    continue;
                };
                let explicitly_allowed = policy
                    .allowed_outward_edges
                    .iter()
                    .any(|edge| edge.source == source && edge.dependency == target.name());
                if target_layer > source_layer && !explicitly_allowed {
                    violations.push(format!(
                        "outward dependency {source} ({}) -> {} ({})",
                        policy.layers[source_layer].name,
                        target.name(),
                        policy.layers[target_layer].name,
                    ));
                }
            }
        }
    }
}

fn validate_budgets(
    graph: &PackageGraph,
    policy: &WorkspacePolicy,
    violations: &mut Vec<String>,
) -> Result<(), Box<dyn Error>> {
    for budget in &policy.dependency_budgets {
        let package = graph
            .resolve_workspace()
            .packages(DependencyDirection::Forward)
            .find(|package| package.name() == budget.crate_name)
            .ok_or_else(|| missing_package(&budget.crate_name))?;
        let (workspace_count, external_count) = production_links(&package).fold(
            (0_usize, 0_usize),
            |(workspace_count, external_count), link| {
                if link.to().in_workspace() {
                    (workspace_count + 1, external_count)
                } else {
                    (workspace_count, external_count + 1)
                }
            },
        );
        if workspace_count > budget.max_workspace {
            violations.push(format!(
                "{} has {workspace_count} direct workspace dependencies; budget is {}",
                budget.crate_name, budget.max_workspace
            ));
        }
        if external_count > budget.max_external {
            violations.push(format!(
                "{} has {external_count} direct external dependencies; budget is {}",
                budget.crate_name, budget.max_external
            ));
        }
    }
    Ok(())
}

fn validate_exclusive_dependencies(
    graph: &PackageGraph,
    policy: &WorkspacePolicy,
    violations: &mut Vec<String>,
) {
    for package in graph
        .resolve_workspace()
        .packages(DependencyDirection::Forward)
    {
        for link in package.direct_links() {
            if let Some(rule) = policy
                .exclusive_dependencies
                .iter()
                .find(|rule| rule.dependency == link.to().name())
            {
                if !rule.owners.iter().any(|owner| owner == package.name()) {
                    violations.push(format!(
                        "isolated dependency {} is owned by {:?}, not {}",
                        rule.dependency,
                        rule.owners,
                        package.name()
                    ));
                }
            }
        }
    }
}

pub(super) fn print_map(graph: &PackageGraph) -> Result<(), Box<dyn Error>> {
    let policy = parse_policy()?;
    let assignments = assignments(&policy)?;
    for (index, layer) in policy.layers.iter().enumerate().rev() {
        println!("layer {index} {}", layer.name);
        for package_name in &layer.crates {
            let package = graph
                .packages()
                .find(|package| package.in_workspace() && package.name() == package_name)
                .ok_or_else(|| missing_package(package_name))?;
            let mut dependencies = production_links(&package)
                .map(|link| link.to().name())
                .filter(|name| assignments.contains_key(*name))
                .collect::<Vec<_>>();
            dependencies.sort_unstable();
            dependencies.dedup();
            let rendered = if dependencies.is_empty() {
                "-".to_owned()
            } else {
                dependencies.join(",")
            };
            println!("  {package_name} -> {rendered}");
        }
    }
    Ok(())
}

fn parse_policy() -> Result<WorkspacePolicy, Box<dyn Error>> {
    let policy: WorkspacePolicy = ron::from_str(POLICY_SOURCE)?;
    if policy.schema_version != 1 {
        return Err(io::Error::other(format!(
            "unsupported workspace policy schema {}",
            policy.schema_version
        ))
        .into());
    }
    Ok(policy)
}

fn assignments(policy: &WorkspacePolicy) -> Result<BTreeMap<&str, usize>, Box<dyn Error>> {
    let mut assignments = BTreeMap::new();
    for (index, layer) in policy.layers.iter().enumerate() {
        if layer.name.trim().is_empty() {
            return Err(io::Error::other("workspace policy contains an empty layer name").into());
        }
        for package in &layer.crates {
            if assignments.insert(package.as_str(), index).is_some() {
                return Err(io::Error::other(format!(
                    "workspace crate {package} is assigned more than once"
                ))
                .into());
            }
        }
    }
    Ok(assignments)
}

fn production_links<'graph>(
    package: &PackageMetadata<'graph>,
) -> impl Iterator<Item = guppy::graph::PackageLink<'graph>> + use<'graph> {
    package.direct_links().filter(|link| !link.dev_only())
}

fn missing_package(name: &str) -> io::Error {
    io::Error::other(format!(
        "workspace policy references missing crate {name:?}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_policy_is_well_formed_and_unique() -> Result<(), Box<dyn Error>> {
        let policy = parse_policy()?;
        let assignments = assignments(&policy)?;
        assert_eq!(assignments.len(), 91);
        assert_eq!(
            policy.layers.first().map(|layer| layer.name.as_str()),
            Some("foundation")
        );
        assert_eq!(
            policy.layers.last().map(|layer| layer.name.as_str()),
            Some("tooling-verification")
        );
        Ok(())
    }
}
