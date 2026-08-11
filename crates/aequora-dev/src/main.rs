//! Bounded, machine-readable workspace context built on Guppy's Cargo graph.

use std::{collections::BTreeSet, env, error::Error, io, process::ExitCode};

use guppy::{
    MetadataCommand,
    graph::{DependencyDirection, PackageGraph, PackageMetadata},
};

const BOUNDARY_RULES: &[(&str, &[&str])] = &[
    ("aequora-store-stoolap", &["aequora-store-postgres", "sqlx"]),
    (
        "aequora-store-postgres",
        &["aequora-store-stoolap", "stoolap"],
    ),
    (
        "aequora-client",
        &["aequora-axum", "aequora-store-postgres", "sqlx"],
    ),
    (
        "aequora-server",
        &["aequora-http", "aequora-store-stoolap", "stoolap"],
    ),
    (
        "aequora-protocol",
        &[
            "aequora-axum",
            "aequora-http",
            "aequora-store-postgres",
            "aequora-store-stoolap",
            "sqlx",
            "stoolap",
        ],
    ),
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("aequora-dev: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let graph = MetadataCommand::new().build_graph()?;
    let mut arguments = env::args().skip(1);

    match arguments.next().as_deref() {
        None | Some("summary") => {
            print_summary(&graph);
            Ok(())
        }
        Some("graph") => print_graph(&graph, arguments.next().as_deref()),
        Some("check") => check_boundaries(&graph),
        Some("help" | "--help" | "-h") => {
            print_help();
            Ok(())
        }
        Some(command) => Err(io::Error::other(format!(
            "unknown command {command:?}; run `cargo run -p aequora-dev -- help`"
        ))
        .into()),
    }
}

fn print_help() {
    println!("aequora-dev summary             workspace and edge counts");
    println!("aequora-dev graph [crate]       compact direct workspace dependencies");
    println!("aequora-dev check               enforce database and layer boundaries");
}

fn print_summary(graph: &PackageGraph) {
    let workspace = graph.resolve_workspace();
    let workspace_packages = workspace
        .packages(DependencyDirection::Forward)
        .collect::<Vec<_>>();
    let workspace_edges = workspace_packages
        .iter()
        .map(|package| workspace_dependencies(package).len())
        .sum::<usize>();
    let external_packages = graph
        .packages()
        .filter(|package| !package.in_workspace())
        .count();

    println!(
        "workspace={} internal_edges={} external_packages={external_packages}",
        workspace_packages.len(),
        workspace_edges,
    );
}

fn print_graph(graph: &PackageGraph, requested: Option<&str>) -> Result<(), Box<dyn Error>> {
    let workspace = graph.resolve_workspace();
    let mut packages = workspace
        .packages(DependencyDirection::Forward)
        .filter(|package| requested.is_none_or(|name| package.name() == name))
        .collect::<Vec<_>>();
    packages.sort_unstable_by_key(PackageMetadata::name);

    if let Some(name) = requested {
        if packages.is_empty() {
            return Err(io::Error::other(format!("workspace crate {name:?} was not found")).into());
        }
    }

    for package in packages {
        let dependencies = workspace_dependencies(&package);
        if dependencies.is_empty() {
            println!("{} -> -", package.name());
        } else {
            println!("{} -> {}", package.name(), dependencies.join(","));
        }
    }
    Ok(())
}

fn workspace_dependencies<'graph>(package: &PackageMetadata<'graph>) -> Vec<&'graph str> {
    let mut dependencies = package
        .direct_links()
        .map(|link| link.to())
        .filter(PackageMetadata::in_workspace)
        .map(|dependency| dependency.name())
        .collect::<Vec<_>>();
    dependencies.sort_unstable();
    dependencies.dedup();
    dependencies
}

fn check_boundaries(graph: &PackageGraph) -> Result<(), Box<dyn Error>> {
    let mut violations = Vec::new();

    for &(root, forbidden) in BOUNDARY_RULES {
        let dependencies = transitive_dependency_names(graph, root)?;
        for &name in forbidden {
            if dependencies.contains(name) {
                violations.push(format!(
                    "{root} transitively reaches forbidden crate {name}"
                ));
            }
        }
    }

    if violations.is_empty() {
        println!(
            "guppy-boundaries: ok ({} rules, {} workspace crates)",
            BOUNDARY_RULES.len(),
            graph.resolve_workspace().len(),
        );
        return Ok(());
    }

    for violation in &violations {
        eprintln!("guppy-boundaries: {violation}");
    }
    Err(io::Error::other(format!(
        "{} dependency-boundary violation(s)",
        violations.len()
    ))
    .into())
}

fn transitive_dependency_names(
    graph: &PackageGraph,
    root_name: &str,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let root = graph
        .packages()
        .find(|package| package.name() == root_name && package.in_workspace())
        .ok_or_else(|| io::Error::other(format!("workspace crate {root_name:?} was not found")))?;
    let query = graph.query_forward(std::iter::once(root.id()))?;

    Ok(query
        .resolve()
        .packages(DependencyDirection::Forward)
        .filter(|package| package.id() != root.id())
        .map(|package| package.name().to_owned())
        .collect())
}
