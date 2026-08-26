//! Bounded, machine-readable workspace context built on Guppy's Cargo graph.

use std::{collections::BTreeSet, env, error::Error, fmt::Write as _, fs, io, process::ExitCode};

use aequora_audit::{ChainedAuditRecord, verify_chain};
use aequora_coordination::CoordinationSnapshot;
use aequora_crypto::{CryptoPolicy, KeyRegistryManifest, PublicKeyBytes, TrustedKeyRegistry};
use aequora_governance::ErasurePlan;
use aequora_profile::{ConsistencyProfile, ConsistencyProfileKind, ProfileManifest};
use aequora_replay::{ReplayBundle, ReplayStateRef};
use aequora_scheduler::SchedulerState;
use aequora_scope::LocalScopeState;

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
    (
        "aequora-replay",
        &[
            "aequora-axum",
            "aequora-client",
            "aequora-http",
            "aequora-quic",
            "aequora-server",
            "aequora-store-postgres",
            "aequora-store-stoolap",
            "axum",
            "reqwest",
            "sqlx",
            "stoolap",
            "tokio",
        ],
    ),
    (
        "aequora-audit",
        &[
            "aequora-axum",
            "aequora-client",
            "aequora-http",
            "aequora-quic",
            "aequora-server",
            "aequora-store-postgres",
            "aequora-store-stoolap",
            "axum",
            "reqwest",
            "sqlx",
            "stoolap",
            "tokio",
        ],
    ),
    (
        "aequora-governance",
        &[
            "aequora-axum",
            "aequora-client",
            "aequora-http",
            "aequora-quic",
            "aequora-server",
            "aequora-store-postgres",
            "aequora-store-stoolap",
            "axum",
            "reqwest",
            "sqlx",
            "stoolap",
            "tokio",
        ],
    ),
    (
        "aequora-crypto",
        &[
            "aequora-axum",
            "aequora-client",
            "aequora-http",
            "aequora-quic",
            "aequora-server",
            "aequora-store-postgres",
            "aequora-store-stoolap",
            "axum",
            "reqwest",
            "sqlx",
            "stoolap",
            "tokio",
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
        Some("coordination") => print_coordination(arguments.next().as_deref(), arguments.next()),
        Some("scheduler") => print_scheduler(arguments.next().as_deref(), arguments.next()),
        Some("scope") => print_scope(arguments.next().as_deref(), arguments.next()),
        Some("live") => print_live(arguments.next().as_deref()),
        Some("profile") => print_profile(
            arguments.next().as_deref(),
            arguments.next(),
            arguments.next(),
        ),
        Some("replay") => print_replay(arguments.next().as_deref(), arguments.next()),
        Some("audit") => print_audit(arguments.next().as_deref(), arguments.next()),
        Some("governance") => print_governance(arguments.next().as_deref(), arguments.next()),
        Some("crypto") => print_crypto(arguments.next().as_deref(), arguments.next()),
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
    println!("aequora-dev coordination explain");
    println!("aequora-dev coordination status <snapshot.ron> [now_unix_ms]");
    println!("aequora-dev scheduler explain");
    println!("aequora-dev scheduler status <state.ron>");
    println!("aequora-dev scope explain");
    println!("aequora-dev scope status <state.ron>");
    println!("aequora-dev live explain");
    println!("aequora-dev profile list");
    println!("aequora-dev profile explain <kind>");
    println!("aequora-dev profile verify <manifest.ron>");
    println!("aequora-dev profile compare <old.ron> <new.ron>");
    println!("aequora-dev replay explain");
    println!("aequora-dev replay verify <bundle.ron>");
    println!("aequora-dev replay inspect <bundle.ron>");
    println!("aequora-dev audit explain");
    println!("aequora-dev audit verify <chain.ron>");
    println!("aequora-dev audit inspect <chain.ron>");
    println!("aequora-dev governance explain");
    println!("aequora-dev governance verify <erasure-plan.ron>");
    println!("aequora-dev crypto policy");
    println!("aequora-dev crypto registry-verify <root-and-registry.ron>");
}

fn print_crypto(action: Option<&str>, argument: Option<String>) -> Result<(), Box<dyn Error>> {
    match action {
        Some("policy") => {
            let policy = CryptoPolicy::standard();
            println!(
                "crypto-policy version={} digest={:?} signature={:?} encryption={:?}",
                policy.version.0,
                policy.allowed_digests,
                policy.allowed_signatures,
                policy.allowed_encryption
            );
            println!(
                "profiles: standard default; enterprise requires signed and encrypted durable artifacts"
            );
            println!("e2e: application-managed opaque domains only; server decisions stay visible");
            Ok(())
        }
        Some("registry-verify") => {
            let path = argument
                .ok_or_else(|| io::Error::other("crypto registry-verify requires a bundle path"))?;
            let (root, manifest): (PublicKeyBytes, KeyRegistryManifest) =
                ron::from_str(&fs::read_to_string(path)?)?;
            let mut registry = TrustedKeyRegistry::new(manifest.signature.key_id, root);
            let accepted = registry.accept(&manifest)?;
            println!(
                "key-registry verified generation={} keys={}",
                accepted.accepted.0, accepted.keys
            );
            Ok(())
        }
        _ => Err(io::Error::other("crypto command must be `policy` or `registry-verify`").into()),
    }
}

fn print_governance(action: Option<&str>, argument: Option<String>) -> Result<(), Box<dyn Error>> {
    match action {
        Some("explain") => {
            println!("precedence: active-hold > erasure/retention; required evidence is minimized");
            println!("sync-safety: active watermarks + bootstrap floor + retired identity guard");
            println!("copies: primary+journal+audit+snapshot+blob+export+replay+import+backup");
            println!("execution: dry-run + separate approval + bounded idempotent store plans");
            println!("completion: every required surface executes and verifies; otherwise partial");
            Ok(())
        }
        Some("verify") => {
            let path = argument
                .ok_or_else(|| io::Error::other("governance verify requires a plan path"))?;
            let plan: ErasurePlan = ron::from_str(&fs::read_to_string(path)?)?;
            plan.verify()?;
            println!(
                "erasure-plan verified actions={} blockers={} executable={}",
                plan.actions.len(),
                plan.blockers.len(),
                plan.executable()
            );
            Ok(())
        }
        _ => Err(io::Error::other("governance command must be `explain` or `verify`").into()),
    }
}

fn print_audit(action: Option<&str>, argument: Option<String>) -> Result<(), Box<dyn Error>> {
    match action {
        Some("explain") => {
            println!("histories: sync-journal != operation-ledger != business-audit != logs");
            println!("atomic: required audit declarations commit with mutation+journal+ledger");
            println!("privacy: stable IDs and policy-filtered values; no payload telemetry");
            println!("integrity: tenant-partition chains plus externally anchored checkpoints");
            println!("access: tenant-bounded authorized queries with hard result limits");
            Ok(())
        }
        Some("verify" | "inspect") => {
            let path =
                argument.ok_or_else(|| io::Error::other("audit command requires a chain path"))?;
            let records: Vec<ChainedAuditRecord> = ron::from_str(&fs::read_to_string(path)?)?;
            let root = verify_chain(&records)?;
            let (tenant, partition, sequence) = records.last().map_or_else(
                || ("empty".to_owned(), "none".to_owned(), 0),
                |record| {
                    (
                        record.event.tenant_id.to_string(),
                        format!("{:?}", record.partition),
                        record.sequence.0,
                    )
                },
            );
            println!(
                "audit-chain verified tenant={} partition={} records={} sequence={} root={}",
                tenant,
                partition,
                records.len(),
                sequence,
                hex_prefix(root),
            );
            Ok(())
        }
        _ => {
            Err(io::Error::other("audit command must be `explain`, `verify`, or `inspect`").into())
        }
    }
}

fn hex_prefix(bytes: [u8; 32]) -> String {
    let mut output = String::with_capacity(16);
    for byte in &bytes[..8] {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn print_replay(action: Option<&str>, argument: Option<String>) -> Result<(), Box<dyn Error>> {
    match action {
        Some("explain") => {
            println!("decision: validated-operation + canonical-pre-state + captured-inputs");
            println!(
                "nondeterminism: one captured clock, labelled IDs/randomness, versioned policy"
            );
            println!("commit: validated plan is separate and atomic; retries retain inputs+digest");
            println!("side-effects: durable intents only; replay never invokes external systems");
            println!(
                "sandbox: embedded canonical state, hard limits, no database or network handle"
            );
            Ok(())
        }
        Some("verify" | "inspect") => {
            let path = argument
                .ok_or_else(|| io::Error::other("replay command requires a bundle path"))?;
            let bundle: ReplayBundle = ron::from_str(&fs::read_to_string(path)?)?;
            bundle.verify()?;
            let state_kind = match bundle.pre_state {
                ReplayStateRef::Embedded { .. } => "embedded",
                ReplayStateRef::Snapshot { .. } => "snapshot-reference",
                ReplayStateRef::JournalRange { .. } => "journal-range",
            };
            println!(
                "replay-bundle verified handler_version={} profile_version={} operation_kind={} state={} captured_ids={} external_results={}",
                bundle.handler_version.get(),
                bundle.profile_version,
                bundle.operation.envelope.operation_kind.0,
                state_kind,
                bundle.execution_inputs.allocated_ids.len(),
                bundle.execution_inputs.external_results.len(),
            );
            Ok(())
        }
        _ => {
            Err(io::Error::other("replay command must be `explain`, `verify`, or `inspect`").into())
        }
    }
}

fn print_profile(
    action: Option<&str>,
    first: Option<String>,
    second: Option<String>,
) -> Result<(), Box<dyn Error>> {
    match action {
        Some("list") => {
            for kind in profile_kinds() {
                println!("{kind:?}");
            }
            Ok(())
        }
        Some("explain") => {
            let name = first.ok_or_else(|| io::Error::other("profile explain requires a kind"))?;
            let kind = parse_profile_kind(&name)?;
            println!("{:#?}", ConsistencyProfile::built_in(kind));
            Ok(())
        }
        Some("verify") => {
            let path =
                first.ok_or_else(|| io::Error::other("profile verify requires a manifest path"))?;
            let manifest: ProfileManifest = ron::from_str(&fs::read_to_string(path)?)?;
            manifest.verify()?;
            println!(
                "profile-manifest verified aggregates={} operations={}",
                manifest.aggregates.len(),
                manifest.operations.len()
            );
            Ok(())
        }
        Some("compare") => {
            let old_path = first
                .ok_or_else(|| io::Error::other("profile compare requires an old manifest"))?;
            let new_path = second
                .ok_or_else(|| io::Error::other("profile compare requires a new manifest"))?;
            let old: ProfileManifest = ron::from_str(&fs::read_to_string(old_path)?)?;
            let new: ProfileManifest = ron::from_str(&fs::read_to_string(new_path)?)?;
            old.verify_compatible_successor(&new)?;
            println!("profile-manifest successor is compatible");
            Ok(())
        }
        _ => Err(io::Error::other(
            "profile command must be `list`, `explain`, `verify`, or `compare`",
        )
        .into()),
    }
}

const fn profile_kinds() -> [ConsistencyProfileKind; 9] {
    [
        ConsistencyProfileKind::ImmutableAppendOnly,
        ConsistencyProfileKind::OptimisticVersioned,
        ConsistencyProfileKind::Commutative,
        ConsistencyProfileKind::LastWriterWins,
        ConsistencyProfileKind::ManualConflict,
        ConsistencyProfileKind::StrongAggregate,
        ConsistencyProfileKind::ServerOnly,
        ConsistencyProfileKind::DeviceLocal,
        ConsistencyProfileKind::DerivedProjection,
    ]
}

fn parse_profile_kind(name: &str) -> Result<ConsistencyProfileKind, io::Error> {
    profile_kinds()
        .into_iter()
        .find(|kind| format!("{kind:?}").eq_ignore_ascii_case(name))
        .ok_or_else(|| io::Error::other(format!("unknown consistency profile {name:?}")))
}

fn print_live(action: Option<&str>) -> Result<(), Box<dyn Error>> {
    match action {
        Some("explain") => {
            println!("authority: none; hints only wake durable authenticated reconciliation");
            println!("delivery: best-effort, unordered, duplicateable, lossy, non-durable");
            println!("routing: authenticated tenant+authorized scope; revoke before fan-out");
            println!("backpressure: latest-only bounded queues; drop or disconnect safely");
            println!("leadership: one fenced local leader; acquisition requires cursor catch-up");
            println!("presence: ephemeral, privacy-filtered, ttl-expiring, never a lock");
            Ok(())
        }
        _ => Err(io::Error::other("live command must be `explain`").into()),
    }
}

fn print_scope(action: Option<&str>, argument: Option<String>) -> Result<(), Box<dyn Error>> {
    match action {
        Some("explain") => {
            println!("authority: server-issued definition+canonical parameters+current auth");
            println!("cursor-binding: scope-id+scope-version+authority-generation+sequence");
            println!("safe-fallback: full per-scope bootstrap on incompatible change");
            println!("removal: membership-only; domain tombstones remain distinct");
            println!("revocation: deactivate first, quarantine pending intent, then clean up");
            Ok(())
        }
        Some("status") => {
            let path = argument.ok_or_else(|| {
                io::Error::other("scope status requires a RON LocalScopeState path")
            })?;
            let input = fs::read_to_string(path)?;
            let state: LocalScopeState = ron::from_str(&input)?;
            println!(
                "active_subscriptions={} pending_transitions={} membership_references={} quarantined_operations={}",
                state.active_subscription_count(),
                state.pending_transition_count(),
                state.membership_reference_count(),
                state.quarantined_operation_count(),
            );
            Ok(())
        }
        _ => Err(io::Error::other("scope command must be `explain` or `status`").into()),
    }
}

fn print_scheduler(action: Option<&str>, argument: Option<String>) -> Result<(), Box<dyn Error>> {
    match action {
        Some("explain") => {
            println!(
                "correctness-first: qos changes timing/order/bounds, never semantics or intent"
            );
            println!("classes: critical,interactive,normal,bulk,background,maintenance");
            println!("durable-sources: outbox,bootstrap,repair,blob,maintenance");
            println!(
                "constraints: dependency,retry,network,power,activity,auth,storage,leadership"
            );
            Ok(())
        }
        Some("status") => {
            let path = argument.ok_or_else(|| {
                io::Error::other("scheduler status requires a RON SchedulerState path")
            })?;
            let input = fs::read_to_string(path)?;
            let state: SchedulerState = ron::from_str(&input)?;
            println!(
                "policy_version={} target_ops={} target_bytes={} success_streak={} circuit={:?} server_backoff_until_unix_ms={:?} background_bytes_today={}",
                state.policy_version,
                state.batch.target_ops,
                state.batch.target_bytes,
                state.batch.success_streak,
                state.circuit.state,
                state.server_backoff_until_unix_ms,
                state.background_bytes_today,
            );
            Ok(())
        }
        _ => Err(io::Error::other("scheduler command must be `explain` or `status`").into()),
    }
}

fn print_coordination(
    action: Option<&str>,
    argument: Option<String>,
) -> Result<(), Box<dyn Error>> {
    match action {
        Some("explain") => {
            println!("leader-only: sync,reconciliation,bootstrap,rebase,compaction,repair");
            println!("follower-safe: domain-mutation,outbox-append,repository-read");
            println!("fence: owner+token+store-generation+kind+unexpired");
            Ok(())
        }
        Some("status") => {
            let path = argument.ok_or_else(|| {
                io::Error::other("coordination status requires a RON snapshot path")
            })?;
            let input = fs::read_to_string(path)?;
            let snapshot: CoordinationSnapshot = ron::from_str(&input)?;
            let now = env::args()
                .nth(4)
                .map(|value| value.parse::<u64>())
                .transpose()?
                .unwrap_or(0);
            let role = if snapshot.is_active(now) {
                "leased"
            } else {
                "unowned-or-expired"
            };
            println!(
                "store_id={} generation={} token={} owner={} kind={:?} expires_at_unix_ms={} status={role}",
                snapshot.store_id,
                snapshot.store_generation.0,
                snapshot.fencing_token.0,
                snapshot
                    .owner_id
                    .map_or_else(|| "-".to_owned(), |owner| owner.to_string()),
                snapshot.kind,
                snapshot.expires_at_unix_ms,
            );
            Ok(())
        }
        _ => Err(io::Error::other("coordination command must be `explain` or `status`").into()),
    }
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
