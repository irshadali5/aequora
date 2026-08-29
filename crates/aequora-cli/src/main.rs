//! Payload-free diagnostics for statically linked Aequora database adapters.

use aequora_admin::{ADMIN_API_VERSION, AdminAction, AdminListenerPolicy};
use aequora_authority::{
    AuthorityController, AuthorityError, AuthorityPromotionPlan, AuthorityPromotionPolicy,
    AuthorityState, CheckpointComparison, JournalCheckpoint, PromotionEvidence,
    RecoveryVerification, compare_checkpoints,
};
use aequora_bootstrap::{BootstrapError, BootstrapJob, SnapshotManifest};
use aequora_compat::{
    ClientHello, CompatibilityError, CompatibilityPolicy, CompatibilityRegistry,
    CompatibilityResult, ServerAuthorityContext, SupportStatus, canonical_registry, negotiate,
};
use aequora_conformance::{
    CertificationArtifact, CertificationRequest, ConformanceDomain, ConformanceError,
    REFERENCE_TESTS, markdown_report,
};
use aequora_diagnostics::{
    DiagnosticError, DiagnosticPolicy, IncidentBundleManifest, OperationExplanationInput,
    explain_operation,
};
use aequora_feed::{ConsumerRegistration, FEED_INVARIANTS, FeedError};
use aequora_integrity::{
    CURRENT_HASH_SCHEMA, CURRENT_INTEGRITY_GENERATION, IntegrityError, IntegritySnapshot,
    PartitionScheme, RepairPlan,
};
use aequora_legacy::{
    BridgeHealth, CutoverReadiness, CutoverVerification, LegacyMigrationManifest,
    LegacyRetirementManifest, LegacySystemManifest, ShadowMatch, ShadowResult,
};
use aequora_migration::{
    CanonicalExport, CutoverBlocker, CutoverEvidence, ExportError, ExportLimits, ImportJob,
    verify_cutover,
};
use aequora_model::{
    ClientId, ExploreError, FailureTrace, Model, ModelAction, ModelCorrelationId, ModelEntityId,
    ModelError, ModelOperation, ModelOperationId, ReplayError, SearchBounds, explore,
};
use aequora_queue::{
    CompactionPlan as QueueCompactionPlan, MutationMutability, OptimizationRegistry, QueueEntry,
    QueueError, plan_compaction,
};
use aequora_region::{
    AuthorityLocation, RegionError, RegionalReadRequest, RegionalRouter, RegionalRouterConfig,
    ReplicaObservation,
};
use aequora_schema::{SchemaError, SchemaRegistry};
use aequora_security::{
    ATTACKER_CLASSES, SECURITY_ASSETS, SECURITY_INVARIANTS, SecurityError, SecurityPolicy,
    TRUST_BOUNDARIES,
};
use aequora_store::{
    AdapterCompatibilityError, AdapterManifest, AdapterRequirements, ProductionAdapterPair,
};
use aequora_store_postgres::POSTGRES_ADAPTER_MANIFEST;
use aequora_store_stoolap::STOOLAP_ADAPTER_MANIFEST;
use std::{env, fs, io, path::Path, process::ExitCode};
use thiserror::Error;

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.first().map(String::as_str) == Some("conform") {
        return conform_main(&arguments[1..]);
    }
    match command(arguments) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("aequora: {error}");
            ExitCode::FAILURE
        }
    }
}

fn command(arguments: impl IntoIterator<Item = String>) -> Result<String, CliError> {
    let mut arguments = arguments.into_iter();
    match arguments.next().as_deref() {
        None | Some("help" | "--help" | "-h") => Ok(help().to_owned()),
        Some("doctor") => doctor(arguments.next().as_deref()),
        Some("inspect") => inspect(arguments.next().as_deref(), arguments.next().as_deref()),
        Some("verify") => verify(
            arguments.next().as_deref(),
            arguments.next().as_deref(),
            arguments.next().as_deref(),
        ),
        Some("init") => initialize(arguments.next().as_deref(), arguments.next().as_deref()),
        Some("integrity") => integrity(arguments.next().as_deref(), arguments.next().as_deref()),
        Some("queue") => queue(
            arguments.next().as_deref(),
            arguments.next().as_deref(),
            arguments.next().as_deref(),
        ),
        Some("import") => migration_command(
            arguments.next().as_deref(),
            arguments.next().as_deref(),
            arguments.next().as_deref(),
        ),
        Some("bootstrap") => {
            bootstrap_command(arguments.next().as_deref(), arguments.next().as_deref())
        }
        Some("authority") => authority_command(
            arguments.next().as_deref(),
            arguments.next().as_deref(),
            arguments.next().as_deref(),
        ),
        Some("region") => region_command(
            arguments.next().as_deref(),
            arguments.next().as_deref(),
            arguments.next().as_deref(),
        ),
        Some("compat") => compat_command(
            arguments.next().as_deref(),
            arguments.next().as_deref(),
            arguments.next().as_deref(),
        ),
        Some("conform") => {
            conform_command(arguments.next().as_deref(), arguments.next().as_deref())
        }
        Some("admin") => admin_command(arguments.next().as_deref(), arguments.next().as_deref()),
        Some("incident") => incident_command(
            arguments.next().as_deref(),
            arguments.next().as_deref(),
            arguments.next().as_deref(),
        ),
        Some("security") => {
            security_command(arguments.next().as_deref(), arguments.next().as_deref())
        }
        Some("feed") => feed_command(arguments.next().as_deref(), arguments.next().as_deref()),
        Some("legacy") => legacy_command(arguments.next().as_deref(), arguments.next().as_deref()),
        Some(other) => Err(CliError::Usage(format!(
            "unknown command {other:?}; run `aequora help`"
        ))),
    }
}

fn help() -> &'static str {
    "aequora doctor adapters\naequora inspect adapters\naequora inspect adapter <stoolap|postgresql>\naequora verify pair <local> <authority>\naequora verify export <artifact.postcard> <schema.ron>\naequora verify model\naequora verify trace <failure.ron>\naequora integrity status\naequora integrity verify <snapshot.ron>\naequora integrity explain <repair-plan.ron>\naequora queue status <entries.ron>\naequora queue verify <entries.ron>\naequora queue compact <entries.ron> <registry.ron>\naequora queue explain <plan.ron>\naequora import plan <artifact.postcard> <schema.ron>\naequora import validate <artifact.postcard> <schema.ron>\naequora import status <job.ron>\naequora import cutover <evidence.ron>\naequora import explain\naequora bootstrap inspect <manifest.ron>\naequora bootstrap status <job.ron>\naequora bootstrap explain\naequora authority status <state.ron>\naequora authority readiness <evidence.ron>\naequora authority promote <plan.ron>\naequora authority demote <state.ron>\naequora authority restore-plan <state.ron>\naequora authority recover <state.ron> --new-epoch\naequora authority verify <verification.ron>\naequora authority fork-check <local.ron> <peer.ron>\naequora authority explain\naequora region status <topology.ron>\naequora region route <topology.ron> <request.ron>\naequora region explain\naequora compat show\naequora compat matrix\naequora compat deprecated\naequora compat check-client <hello.ron> <policy.ron>\naequora compat registry [registry.ron]\naequora conform run <request.ron>\naequora conform storage|protocol|client|server\naequora conform report <artifact.ron>\naequora conform verify <artifact.ron>\naequora admin capabilities\naequora admin validate-listener <policy.ron>\naequora admin inspect-action <action.ron>\naequora admin explain\naequora incident inspect <manifest.ron> <policy.ron>\naequora incident explain-operation <input.ron>\naequora incident explain\naequora security show\naequora security validate <policy.ron>\naequora security explain\naequora feed show\naequora feed validate <consumer.ron>\naequora feed explain\naequora legacy discover <system-manifest.ron>\naequora legacy map-verify <migration-manifest.ron>\naequora legacy bridge-status <health.ron>\naequora legacy shadow-report <results.ron>\naequora legacy cutover-plan <readiness.ron>\naequora legacy verify <verification.ron>\naequora legacy retire <retirement-manifest.ron>\naequora init <new-directory> <client|server>"
}

fn conform_main(arguments: &[String]) -> ExitCode {
    match conform_command(
        arguments.first().map(String::as_str),
        arguments.get(1).map(String::as_str),
    ) {
        Ok(output) => {
            println!("{output}");
            if output.contains("result=Failed") {
                ExitCode::from(1)
            } else if output.contains("result=Unsupported") {
                ExitCode::from(3)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("aequora: {error}");
            match error {
                CliError::Conformance(ConformanceError::UnsupportedFormat) => ExitCode::from(3),
                CliError::Conformance(_) => ExitCode::from(1),
                _ => ExitCode::from(2),
            }
        }
    }
}

fn conform_command(subject: Option<&str>, path: Option<&str>) -> Result<String, CliError> {
    match subject {
        Some("run") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora conform run <request.ron>".to_owned())
            })?;
            let artifact = read_ron::<CertificationRequest>(path, 16 * 1024 * 1024)?.execute()?;
            let encoded = ron::ser::to_string_pretty(&artifact, ron::ser::PrettyConfig::default())
                .map_err(|error| CliError::Serialize(error.to_string()))?;
            Ok(format!(
                "conformance artifact: result={:?}\n{encoded}",
                artifact.result
            ))
        }
        Some("report") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora conform report <artifact.ron>".to_owned())
            })?;
            let artifact = read_ron::<CertificationArtifact>(path, 16 * 1024 * 1024)?;
            artifact.verify_identity()?;
            Ok(markdown_report(&artifact))
        }
        Some("verify") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora conform verify <artifact.ron>".to_owned())
            })?;
            let artifact = read_ron::<CertificationArtifact>(path, 16 * 1024 * 1024)?;
            artifact.verify_identity()?;
            Ok(format!(
                "conformance: valid=true certification={} suite={} profile={:?} tier={:?} result={:?} writes=0",
                artifact.certification_id,
                artifact.suite_version,
                artifact.profile,
                artifact.tier,
                artifact.result
            ))
        }
        Some("storage" | "protocol" | "client" | "server") if path.is_none() => {
            let domain = match subject {
                Some("storage") => ConformanceDomain::StorageAdapter,
                Some("protocol") => ConformanceDomain::ProtocolImplementation,
                Some("client") => ConformanceDomain::ClientRuntime,
                Some("server") => ConformanceDomain::ServerRuntime,
                _ => unreachable!(),
            };
            let tests = REFERENCE_TESTS
                .iter()
                .filter(|test| test.domain == domain)
                .map(|test| format!("{}:{}", test.id.0, test.name))
                .collect::<Vec<_>>()
                .join(",");
            Ok(format!(
                "conformance domain: {domain:?} tests=[{tests}] writes=0"
            ))
        }
        _ => Err(CliError::Usage(
            "usage: aequora conform <run|storage|protocol|client|server|report|verify> ..."
                .to_owned(),
        )),
    }
}

fn security_command(subject: Option<&str>, path: Option<&str>) -> Result<String, CliError> {
    match subject {
        Some("show") if path.is_none() => Ok(format!(
            "security: schema=1 assets={} attackers={} boundaries={} invariants={} writes=0",
            SECURITY_ASSETS.len(),
            ATTACKER_CLASSES.len(),
            TRUST_BOUNDARIES.len(),
            SECURITY_INVARIANTS.len()
        )),
        Some("validate") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora security validate <policy.ron>".to_owned())
            })?;
            let policy: SecurityPolicy = read_ron(path, 256 * 1024)?;
            policy.validate()?;
            Ok(format!(
                "security policy: valid=true level={:?} frame_bytes={} operations={} redirects={} writes=0",
                policy.level,
                policy.protocol.max_frame_bytes,
                policy.protocol.max_operations_per_batch,
                policy.egress.max_redirects
            ))
        }
        Some("explain") if path.is_none() => Ok(
            "security: deny-by-default identity and tenant binding, bounded inputs, immutable operation semantics, rollback resistance, private admin, SSRF-safe egress, redacted secrets, and explicit side-effect reconciliation"
                .to_owned(),
        ),
        _ => Err(CliError::Usage(
            "usage: aequora security <show|validate|explain> [policy.ron]".to_owned(),
        )),
    }
}

fn feed_command(subject: Option<&str>, path: Option<&str>) -> Result<String, CliError> {
    match subject {
        Some("show") if path.is_none() => Ok(format!(
            "feed: schema=1 invariants={} source=authoritative-journal delivery=at-least-once writes=0",
            FEED_INVARIANTS.len()
        )),
        Some("validate") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora feed validate <consumer.ron>".to_owned())
            })?;
            let registration: ConsumerRegistration = read_ron(path, 256 * 1024)?;
            registration.validate()?;
            Ok(format!(
                "feed consumer: valid=true id={} status={:?} ordering={:?} retention={:?} writes=0",
                registration.consumer_id.as_uuid(),
                registration.status,
                registration.ordering,
                registration.retention
            ))
        }
        Some("explain") if path.is_none() => Ok(
            "feed: journal-derived consumers have independent epoch-bound cursors, durable-effect-before-ACK, fenced leases, explicit ordering/retention/failure policy, bounded replay, and no authority over domain state"
                .to_owned(),
        ),
        _ => Err(CliError::Usage(
            "usage: aequora feed <show|validate|explain> [consumer.ron]".to_owned(),
        )),
    }
}

fn legacy_command(subject: Option<&str>, path: Option<&str>) -> Result<String, CliError> {
    let usage = "usage: aequora legacy <discover|map-verify|bridge-status|shadow-report|cutover-plan|verify|retire> <evidence.ron>";
    let path = path.ok_or_else(|| CliError::Usage(usage.to_owned()))?;
    match subject {
        Some("discover") => {
            let manifest: LegacySystemManifest = read_ron(path, 4 * 1024 * 1024)?;
            Ok(format!(
                "legacy discover: collections={} triggers={} procedures={} writers={} writes=0",
                manifest.collections.len(),
                manifest.triggers.len(),
                manifest.procedures.len(),
                manifest.scheduled_writers.len()
            ))
        }
        Some("map-verify") => {
            let manifest: LegacyMigrationManifest = read_ron(path, 4 * 1024 * 1024)?;
            Ok(format!(
                "legacy map verify: aggregates={} mapping_version={} decoder_version={} writes=0",
                manifest.aggregate_types.len(),
                manifest.mapping_version.get(),
                manifest.decoder_version
            ))
        }
        Some("bridge-status") => {
            let health: BridgeHealth = read_ron(path, 1024 * 1024)?;
            Ok(format!(
                "legacy bridge status: state={:?} lag={} quarantine={} writes=0",
                health.state, health.lag, health.quarantine_count
            ))
        }
        Some("shadow-report") => {
            let results: Vec<ShadowResult> = read_ron(path, 8 * 1024 * 1024)?;
            let unexpected = results
                .iter()
                .filter(|result| result.match_state == ShadowMatch::UnexpectedDifference)
                .count();
            Ok(format!(
                "legacy shadow report: samples={} unexpected={} writes=0",
                results.len(),
                unexpected
            ))
        }
        Some("cutover-plan") => {
            let readiness: CutoverReadiness = read_ron(path, 4 * 1024 * 1024)?;
            let blockers = readiness.blockers();
            Ok(format!(
                "legacy cutover plan: ready={} blockers={} writes=0",
                blockers.is_empty(),
                blockers.len()
            ))
        }
        Some("verify") => {
            let verification: CutoverVerification = read_ron(path, 1024 * 1024)?;
            Ok(format!(
                "legacy verify: complete={} writes=0",
                verification.complete()
            ))
        }
        Some("retire") => {
            let manifest: LegacyRetirementManifest = read_ron(path, 1024 * 1024)?;
            Ok(format!(
                "legacy retire: archive={} retain_id_map={} writes=0",
                manifest.archive_reference, manifest.retain_id_map
            ))
        }
        _ => Err(CliError::Usage(usage.to_owned())),
    }
}

fn incident_command(
    subject: Option<&str>,
    path: Option<&str>,
    policy_path: Option<&str>,
) -> Result<String, CliError> {
    match subject {
        Some("inspect") => {
            let manifest_path = path.ok_or_else(|| {
                CliError::Usage(
                    "usage: aequora incident inspect <manifest.ron> <policy.ron>".to_owned(),
                )
            })?;
            let policy_path = policy_path.ok_or_else(|| {
                CliError::Usage(
                    "usage: aequora incident inspect <manifest.ron> <policy.ron>".to_owned(),
                )
            })?;
            let manifest: IncidentBundleManifest = read_ron(manifest_path, 2 * 1024 * 1024)?;
            let policy: DiagnosticPolicy = read_ron(policy_path, 256 * 1024)?;
            manifest.validate(&policy)?;
            Ok(format!(
                "incident bundle: id={} incident={} producer={:?} mode={:?} complete={} state={:?} writes=0",
                manifest.bundle_id.as_uuid(),
                manifest.incident_id.as_uuid(),
                manifest.producer,
                manifest.mode,
                manifest.completeness.is_complete(),
                manifest.state,
            ))
        }
        Some("explain-operation") if policy_path.is_none() => {
            let input_path = path.ok_or_else(|| {
                CliError::Usage(
                    "usage: aequora incident explain-operation <input.ron>".to_owned(),
                )
            })?;
            let input: OperationExplanationInput = read_ron(input_path, 256 * 1024)?;
            let summary = explain_operation(input);
            Ok(format!(
                "incident operation: classification={:?} confidence={:?} evidence={} writes=0",
                summary.classification,
                summary.confidence,
                summary.supporting_refs.len(),
            ))
        }
        Some("explain") if path.is_none() && policy_path.is_none() => Ok(
            "incident: collection is bounded, tenant-authorized, sanitized before export, encryption-required in production, signed in forensic mode, and replay is isolated from production side effects"
                .to_owned(),
        ),
        _ => Err(CliError::Usage(
            "usage: aequora incident <inspect|explain-operation|explain> ...".to_owned(),
        )),
    }
}

fn admin_command(subject: Option<&str>, path: Option<&str>) -> Result<String, CliError> {
    match subject {
        Some("capabilities") if path.is_none() => Ok(format!(
            "admin: api_version={ADMIN_API_VERSION} formats=postcard,ron,json mutations=require-auth writes=0"
        )),
        Some("validate-listener") => {
            let path = path.ok_or_else(|| {
                CliError::Usage(
                    "usage: aequora admin validate-listener <policy.ron>".to_owned(),
                )
            })?;
            let policy: AdminListenerPolicy = read_ron(path, 256 * 1024)?;
            policy.validate()?;
            Ok(format!(
                "admin listener: valid=true state={:?} scope={:?} destructive={:?} writes=0",
                policy.state, policy.bind_scope, policy.destructive_actions
            ))
        }
        Some("inspect-action") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora admin inspect-action <action.ron>".to_owned())
            })?;
            let action: AdminAction = read_ron(path, 512 * 1024)?;
            let digest = action.digest()?;
            Ok(format!(
                "admin action: id={} kind={:?} risk={:?} permission={:?} digest={} plan_required={} writes=0",
                action.admin_operation_id.as_uuid(),
                action.command.kind(),
                action.command.risk(),
                action.command.permission(),
                hex::encode(digest),
                aequora_admin::requires_plan(&action.command),
            ))
        }
        Some("explain") if path.is_none() => Ok(
            "admin: mutations require a private listener, strong authentication, server-side permission checks, stable AdminOperationId, exact plan digest when high risk, second-person approval when destructive, subsystem guards, verified postconditions, and durable audit"
                .to_owned(),
        ),
        _ => Err(CliError::Usage(
            "usage: aequora admin <capabilities|validate-listener|inspect-action|explain> ..."
                .to_owned(),
        )),
    }
}

fn compat_command(
    subject: Option<&str>,
    path: Option<&str>,
    policy_path: Option<&str>,
) -> Result<String, CliError> {
    match subject {
        Some("show") if path.is_none() && policy_path.is_none() => {
            let report = canonical_registry()?.report();
            Ok(format!(
                "compat: generation={} protocols={} capabilities={} operations={} writes=0",
                report.registry_generation,
                report.protocol_count,
                report.capability_count,
                report.operation_count
            ))
        }
        Some("matrix") if path.is_none() && policy_path.is_none() => {
            let registry = canonical_registry()?;
            let rows = registry
                .protocols
                .iter()
                .map(|entry| format!("v{}:{:?}", entry.version.0, entry.status))
                .collect::<Vec<_>>()
                .join(",");
            Ok(format!("compat matrix: {rows} writes=0"))
        }
        Some("deprecated") if path.is_none() && policy_path.is_none() => {
            let registry = canonical_registry()?;
            let count = registry
                .protocols
                .iter()
                .filter(|entry| entry.status == SupportStatus::Deprecated)
                .count();
            Ok(format!("compat deprecated: protocols={count} writes=0"))
        }
        Some("registry") if policy_path.is_none() => {
            let registry = match path {
                Some(path) => read_ron::<CompatibilityRegistry>(path, 2 * 1024 * 1024)?,
                None => canonical_registry()?,
            };
            registry.validate()?;
            Ok(format!(
                "compat registry: ok generation={} writes=0",
                registry.registry_generation
            ))
        }
        Some("check-client") => {
            let hello_path = path.ok_or_else(|| {
                CliError::Usage(
                    "usage: aequora compat check-client <hello.ron> <policy.ron>".to_owned(),
                )
            })?;
            let policy_path = policy_path.ok_or_else(|| {
                CliError::Usage(
                    "usage: aequora compat check-client <hello.ron> <policy.ron>".to_owned(),
                )
            })?;
            let hello: ClientHello = read_ron(hello_path, 2 * 1024 * 1024)?;
            let policy: CompatibilityPolicy = read_ron(policy_path, 2 * 1024 * 1024)?;
            let result = negotiate(
                &hello,
                &policy,
                ServerAuthorityContext {
                    authority_id: aequora_types::AuthorityId::LOCAL_DEVELOPMENT,
                    authority_epoch: aequora_types::AuthorityEpoch::INITIAL,
                },
            )?;
            let status = match result {
                CompatibilityResult::Compatible(_) => "compatible",
                CompatibilityResult::UpgradeRecommended { .. } => "upgrade-recommended",
                CompatibilityResult::UpgradeRequired(_) => "upgrade-required",
            };
            Ok(format!("compat check-client: {status} writes=0"))
        }
        _ => Err(CliError::Usage(
            "usage: aequora compat <show|matrix|deprecated|check-client|registry> ...".to_owned(),
        )),
    }
}

#[derive(serde::Deserialize)]
struct RegionalTopology {
    authority: AuthorityLocation,
    authority_available: bool,
    config: RegionalRouterConfig,
    replicas: Vec<ReplicaObservation>,
}

fn region_command(
    subject: Option<&str>,
    path: Option<&str>,
    request_path: Option<&str>,
) -> Result<String, CliError> {
    match subject {
        Some("status") if request_path.is_none() => {
            let topology = read_region_topology(path, "status <topology.ron>")?;
            let current = topology
                .replicas
                .iter()
                .filter(|replica| {
                    replica.watermark.authority_id == topology.authority.authority_id
                        && replica.watermark.authority_epoch == topology.authority.authority_epoch
                })
                .count();
            let wrong_epoch = topology.replicas.len().saturating_sub(current);
            let _router = build_region_router(topology)?;
            Ok(format!(
                "region status: current_replicas={current} wrong_epoch_replicas={wrong_epoch} writes=0"
            ))
        }
        Some("route") => {
            let topology = read_region_topology(path, "route <topology.ron> <request.ron>")?;
            let request_path = request_path.ok_or_else(|| {
                CliError::Usage(
                    "usage: aequora region route <topology.ron> <request.ron>".to_owned(),
                )
            })?;
            let request: RegionalReadRequest = read_ron(request_path, 2 * 1024 * 1024)?;
            let decision = build_region_router(topology)?.route(request)?;
            Ok(format!(
                "region route: target={:?} consistency={:?} downgraded={} wait={:?} writes=0",
                decision.target,
                decision.effective_consistency,
                decision.downgraded,
                decision.wait_before_fallback.map(|wait| wait.timeout),
            ))
        }
        Some("explain") if path.is_none() && request_path.is_none() => Ok(
            "region: reads require current-epoch durable apply watermarks; session and AtLeast reads wait only within a bound then fall back by explicit policy; all writes remain authority-only"
                .to_owned(),
        ),
        _ => Err(CliError::Usage(
            "usage: aequora region <status|route|explain> ...".to_owned(),
        )),
    }
}

fn read_region_topology(path: Option<&str>, usage: &str) -> Result<RegionalTopology, CliError> {
    let path = path.ok_or_else(|| CliError::Usage(format!("usage: aequora region {usage}")))?;
    read_ron(path, 8 * 1024 * 1024)
}

fn build_region_router(topology: RegionalTopology) -> Result<RegionalRouter, CliError> {
    let mut router = RegionalRouter::new(
        topology.authority,
        topology.authority_available,
        topology.config,
    )?;
    for replica in topology.replicas {
        router.observe(replica)?;
    }
    Ok(router)
}

fn authority_command(
    subject: Option<&str>,
    path: Option<&str>,
    peer: Option<&str>,
) -> Result<String, CliError> {
    match subject {
        Some("status") => {
            let state: AuthorityState = read_authority(path, "status <state.ron>")?;
            Ok(format!(
                "authority: id={} epoch={} instance={} role={:?} mode={:?} fence={} transition={} writes=0",
                state.authority_id,
                state.epoch.get(),
                state.instance_id,
                state.role,
                state.runtime_mode,
                state.fence_token.get(),
                state.transition_id,
            ))
        }
        Some("readiness") => {
            let evidence: PromotionEvidence = read_authority(path, "readiness <evidence.ron>")?;
            Ok(format!(
                "authority readiness: status={:?} lossless_continuity={} external_fence={} lag={:?} writes=0",
                evidence.readiness(),
                evidence.proves_lossless(),
                evidence.old_primary_externally_fenced,
                evidence.replication_lag,
            ))
        }
        Some("promote") => {
            let plan: AuthorityPromotionPlan = read_authority(path, "promote <plan.ron>")?;
            let outcome = plan.evaluate()?;
            outcome.transition.verify()?;
            Ok(format!(
                "authority promotion dry-run: class={:?} old_epoch={} new_epoch={} fence={} mode={:?} approval=validated writes=0",
                outcome.transition.promotion_class,
                outcome.transition.old_epoch.get(),
                outcome.transition.new_epoch.get(),
                outcome.transition.fence_token.get(),
                outcome.state.runtime_mode,
            ))
        }
        Some("demote") => {
            let state: AuthorityState = read_authority(path, "demote <state.ron>")?;
            let controller = AuthorityController::new(state, AuthorityPromotionPolicy::default());
            let demoted = controller.demote(state.updated_at_unix_ms.saturating_add(1))?;
            Ok(format!(
                "authority demotion dry-run: old_role={:?} new_role={:?} new_fence={} writes=0",
                state.role,
                demoted.role,
                demoted.fence_token.get(),
            ))
        }
        Some("restore-plan") => authority_restore_plan(path),
        Some("recover") => authority_recover(path, peer),
        Some("verify") => {
            let verification: RecoveryVerification =
                read_authority(path, "verify <verification.ron>")?;
            if !verification.is_complete() {
                return Err(AuthorityError::RecoveryVerificationIncomplete.into());
            }
            Ok("authority recovery verification: complete=true writes=0".to_owned())
        }
        Some("fork-check") => {
            let local: JournalCheckpoint = read_authority(path, "fork-check <local.ron>")?;
            let peer: JournalCheckpoint = peer
                .ok_or_else(|| {
                    CliError::Usage(
                        "usage: aequora authority fork-check <local.ron> <peer.ron>".to_owned(),
                    )
                })
                .and_then(|path| read_ron(path, 2 * 1024 * 1024))?;
            let comparison = compare_checkpoints(local, peer);
            let action = if comparison == CheckpointComparison::ForkDetected {
                "WRITES_MUST_STOP"
            } else {
                "none"
            };
            Ok(format!(
                "authority fork-check: status={comparison:?} sequence={} action={action} writes=0",
                local.sequence.0,
            ))
        }
        Some("explain") if path.is_none() && peer.is_none() => Ok(
            "authority: commands are read-only dry-runs; the host control plane must fence the old primary and atomically persist the approved transition before recovery verification enables writes"
                .to_owned(),
        ),
        _ => Err(CliError::Usage(
            "usage: aequora authority <status|readiness|promote|demote|restore-plan|recover|verify|fork-check|explain> ..."
                .to_owned(),
        )),
    }
}

fn authority_restore_plan(path: Option<&str>) -> Result<String, CliError> {
    let state: AuthorityState = read_authority(path, "restore-plan <state.ron>")?;
    let next = state
        .epoch
        .checked_next()
        .ok_or(AuthorityError::EpochExhausted)?;
    Ok(format!(
        "authority restore plan: old_epoch={} required_new_epoch={} mode=ReadOnlyVerification governance_reconciliation=required side_effect_reconciliation=required writes=0",
        state.epoch.get(),
        next.get(),
    ))
}

fn authority_recover(path: Option<&str>, flag: Option<&str>) -> Result<String, CliError> {
    let state: AuthorityState = read_authority(path, "recover <state.ron> --new-epoch")?;
    if flag != Some("--new-epoch") {
        return Err(CliError::Usage(
            "usage: aequora authority recover <state.ron> --new-epoch".to_owned(),
        ));
    }
    let next = state
        .epoch
        .checked_next()
        .ok_or(AuthorityError::EpochExhausted)?;
    Ok(format!(
        "authority recovery dry-run: old_epoch={} new_epoch={} mode=ReadOnlyVerification verification=required writes=0",
        state.epoch.get(),
        next.get(),
    ))
}

fn read_authority<T: serde::de::DeserializeOwned>(
    path: Option<&str>,
    usage: &str,
) -> Result<T, CliError> {
    let path = path.ok_or_else(|| CliError::Usage(format!("usage: aequora authority {usage}")))?;
    read_ron(path, 2 * 1024 * 1024)
}

fn bootstrap_command(subject: Option<&str>, path: Option<&str>) -> Result<String, CliError> {
    match subject {
        Some("inspect") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora bootstrap inspect <manifest.ron>".to_owned())
            })?;
            let manifest: SnapshotManifest = read_ron(path, 32 * 1024 * 1024)?;
            manifest.verify()?;
            Ok(format!(
                "bootstrap manifest: valid=true snapshot={} chunks={} records={} bytes={} boundary_sequence={} writes=0",
                manifest.snapshot_id,
                manifest.chunks.len(),
                manifest.record_count,
                manifest.total_uncompressed_bytes,
                manifest.boundary.sequence.0,
            ))
        }
        Some("status") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora bootstrap status <job.ron>".to_owned())
            })?;
            let job: BootstrapJob = read_ron(path, 8 * 1024 * 1024)?;
            let status = job.status();
            Ok(format!(
                "bootstrap status: state={:?} chunks={}/{} bytes_downloaded={} boundary_sequence={} writes=0",
                status.state,
                status.chunks_complete,
                status.chunks_total,
                status.bytes_downloaded,
                job.boundary.sequence.0,
            ))
        }
        Some("explain") if path.is_none() => Ok(
            "bootstrap: inspect/status are read-only; transfer and staging are bounded and resumable; activation requires a current lease, scope, authority epoch, complete verified generation, and unchanged pending-intent digest"
                .to_owned(),
        ),
        _ => Err(CliError::Usage(
            "usage: aequora bootstrap <inspect|status|explain> ...".to_owned(),
        )),
    }
}

fn migration_command(
    subject: Option<&str>,
    path: Option<&str>,
    schema: Option<&str>,
) -> Result<String, CliError> {
    match subject {
        Some("plan" | "validate") => {
            let path = path.ok_or_else(|| {
                CliError::Usage(
                    "usage: aequora import <plan|validate> <artifact.postcard> <schema.ron>"
                        .to_owned(),
                )
            })?;
            let schema = schema.ok_or_else(|| {
                CliError::Usage(
                    "usage: aequora import <plan|validate> <artifact.postcard> <schema.ron>"
                        .to_owned(),
                )
            })?;
            let (records, root, chunks) = inspect_export(path, schema)?;
            Ok(format!(
                "import {}: dry_run=true records={records} chunks={chunks} root={} writes=0",
                subject.unwrap_or_default(),
                hex::encode(root)
            ))
        }
        Some("status") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora import status <job.ron>".to_owned())
            })?;
            let job: ImportJob = read_ron(path, 2 * 1024 * 1024)?;
            Ok(format!(
                "import status: state={:?} mode={:?} committed_batches={} committed_records={} source_offset={} mapping_version={} policy_version={}",
                job.state,
                job.mode,
                job.checkpoint.committed_batches,
                job.checkpoint.committed_records,
                job.checkpoint.source_offset,
                job.mapping_version,
                job.policy_version,
            ))
        }
        Some("cutover") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora import cutover <evidence.ron>".to_owned())
            })?;
            let evidence: CutoverEvidence = read_ron(path, 2 * 1024 * 1024)?;
            verify_cutover(&evidence)?;
            Ok("import cutover: ready=true activation=not-executed".to_owned())
        }
        Some("explain") if path.is_none() && schema.is_none() => Ok(
            "import: plan/validate are read-only; run/resume/cutover activation require a host authority adapter; checkpoints commit with batches; source fingerprint and single-writer fence are mandatory"
                .to_owned(),
        ),
        _ => Err(CliError::Usage(
            "usage: aequora import <plan|validate|status|cutover|explain> ...".to_owned(),
        )),
    }
}

fn queue(
    subject: Option<&str>,
    path: Option<&str>,
    registry: Option<&str>,
) -> Result<String, CliError> {
    let path = path.ok_or_else(|| {
        CliError::Usage(
            "usage: aequora queue <status|verify|compact|explain> <file.ron>".to_owned(),
        )
    })?;
    match subject {
        Some("status") => {
            let entries = read_queue_entries(path)?;
            let mutable = entries
                .iter()
                .filter(|entry| entry.mutability == MutationMutability::MutableUnsent)
                .count();
            let immutable = entries.len().saturating_sub(mutable);
            Ok(format!(
                "queue: operations={} mutable_unsent={} immutable_or_finalized={}",
                entries.len(),
                mutable,
                immutable
            ))
        }
        Some("verify") => {
            let entries = read_queue_entries(path)?;
            let plan = plan_compaction(&entries, &OptimizationRegistry::default(), entries.len())?;
            Ok(format!(
                "queue: valid operations={} dependencies=resolved immutable_hashes=valid",
                plan.operations_before
            ))
        }
        Some("compact") => {
            let registry = registry.ok_or_else(|| {
                CliError::Usage(
                    "usage: aequora queue compact <entries.ron> <registry.ron>".to_owned(),
                )
            })?;
            let entries = read_queue_entries(path)?;
            let registry: OptimizationRegistry = read_ron(registry, 4 * 1024 * 1024)?;
            let plan = plan_compaction(&entries, &registry, entries.len())?;
            Ok(format!(
                "queue dry-run: before={} after={} removed={} bytes_saved={} barriers={} sensitive_preserved={}",
                plan.operations_before,
                plan.operations_after,
                plan.remove_operations.len(),
                plan.bytes_saved,
                plan.barriers,
                plan.sensitive_preserved
            ))
        }
        Some("explain") => {
            let plan: QueueCompactionPlan = read_ron(path, 8 * 1024 * 1024)?;
            Ok(format!(
                "queue plan: removed={} supersessions={} barriers={} sensitive_preserved={}",
                plan.remove_operations.len(),
                plan.supersessions.len(),
                plan.barriers,
                plan.sensitive_preserved
            ))
        }
        _ => Err(CliError::Usage(
            "usage: aequora queue <status|verify|compact|explain> ...".to_owned(),
        )),
    }
}

fn read_queue_entries(path: &str) -> Result<Vec<QueueEntry>, CliError> {
    read_ron(path, 16 * 1024 * 1024)
}

fn read_ron<T: serde::de::DeserializeOwned>(path: &str, limit: usize) -> Result<T, CliError> {
    let bytes = read_bounded(Path::new(path), limit)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| CliError::QueueEncoding)?;
    ron::from_str(text).map_err(|error| CliError::QueueRon(error.to_string()))
}

fn integrity(subject: Option<&str>, path: Option<&str>) -> Result<String, CliError> {
    match subject {
        Some("status") if path.is_none() => Ok(format!(
            "integrity: supported generation={} hash_schema={} max_partitions={}",
            CURRENT_INTEGRITY_GENERATION.0,
            CURRENT_HASH_SCHEMA.0,
            PartitionScheme::MAX_BUCKETS
        )),
        Some("verify") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora integrity verify <snapshot.ron>".to_owned())
            })?;
            let bytes = read_bounded(Path::new(path), 16 * 1024 * 1024)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| CliError::IntegrityEncoding)?;
            let snapshot: IntegritySnapshot =
                ron::from_str(text).map_err(|error| CliError::IntegrityRon(error.to_string()))?;
            snapshot.verify_structure()?;
            Ok(format!(
                "integrity: ok generation={} partitions={} entities={} root={}",
                snapshot.manifest.generation.0,
                snapshot.manifest.partition_count,
                snapshot.manifest.entity_count,
                snapshot.manifest.root_hash
            ))
        }
        Some("explain") => {
            let path = path.ok_or_else(|| {
                CliError::Usage("usage: aequora integrity explain <repair-plan.ron>".to_owned())
            })?;
            let bytes = read_bounded(Path::new(path), 4 * 1024 * 1024)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| CliError::IntegrityEncoding)?;
            let plan: RepairPlan =
                ron::from_str(text).map_err(|error| CliError::IntegrityRon(error.to_string()))?;
            Ok(format!(
                "integrity repair: strategy={:?} affected_entities={} boundary_sequence={} scope={}",
                plan.strategy,
                plan.affected_entities.len(),
                plan.boundary.sequence.0,
                plan.boundary.scope
            ))
        }
        _ => Err(CliError::Usage(
            "usage: aequora integrity <status|verify|explain> ...".to_owned(),
        )),
    }
}

fn doctor(subject: Option<&str>) -> Result<String, CliError> {
    if subject.is_some_and(|value| value != "adapters") {
        return Err(CliError::Usage(
            "doctor currently supports only `aequora doctor adapters`".to_owned(),
        ));
    }
    AdapterRequirements::PRODUCTION_LOCAL.verify(STOOLAP_ADAPTER_MANIFEST)?;
    AdapterRequirements::PRODUCTION_AUTHORITATIVE.verify(POSTGRES_ADAPTER_MANIFEST)?;
    ProductionAdapterPair::verify(STOOLAP_ADAPTER_MANIFEST, POSTGRES_ADAPTER_MANIFEST)?;
    Ok(format!(
        "adapter status: ok\nlocal: {} {} tier={:?} db={}\nauthority: {} {} tier={:?} db={}\npair: database-neutral production requirements satisfied",
        STOOLAP_ADAPTER_MANIFEST.name,
        STOOLAP_ADAPTER_MANIFEST.adapter_version,
        STOOLAP_ADAPTER_MANIFEST.tier,
        STOOLAP_ADAPTER_MANIFEST.tested_database_versions.join(","),
        POSTGRES_ADAPTER_MANIFEST.name,
        POSTGRES_ADAPTER_MANIFEST.adapter_version,
        POSTGRES_ADAPTER_MANIFEST.tier,
        POSTGRES_ADAPTER_MANIFEST.tested_database_versions.join(","),
    ))
}

fn inspect(subject: Option<&str>, name: Option<&str>) -> Result<String, CliError> {
    let pretty = ron::ser::PrettyConfig::default();
    match subject {
        Some("adapters") if name.is_none() => {
            let manifests = [STOOLAP_ADAPTER_MANIFEST, POSTGRES_ADAPTER_MANIFEST];
            for manifest in manifests {
                manifest.validate()?;
            }
            ron::ser::to_string_pretty(&manifests, pretty)
                .map_err(|error| CliError::Serialize(error.to_string()))
        }
        Some("adapter") => {
            let manifest = named_manifest(name)?;
            manifest.validate()?;
            ron::ser::to_string_pretty(&manifest, pretty)
                .map_err(|error| CliError::Serialize(error.to_string()))
        }
        _ => Err(CliError::Usage(
            "usage: aequora inspect adapters | aequora inspect adapter <stoolap|postgresql>"
                .to_owned(),
        )),
    }
}

fn verify(
    subject: Option<&str>,
    local: Option<&str>,
    authority: Option<&str>,
) -> Result<String, CliError> {
    match subject {
        Some("pair") => {
            let local = named_manifest(local)?;
            let authority = named_manifest(authority)?;
            ProductionAdapterPair::verify(local, authority)?;
            Ok(format!(
                "pair: ok local={} authority={}",
                local.name, authority.name
            ))
        }
        Some("export") => verify_export(local, authority),
        Some("model") if local.is_none() && authority.is_none() => verify_model(),
        Some("trace") if authority.is_none() => verify_trace(local),
        _ => Err(CliError::Usage(
            "usage: aequora verify <pair|export|model|trace> ...".to_owned(),
        )),
    }
}

fn verify_model() -> Result<String, CliError> {
    let mut model = Model::new(1);
    model.apply(ModelAction::LocalMutate {
        client: ClientId(0),
        operation: ModelOperation {
            id: ModelOperationId(1),
            correlation_id: ModelCorrelationId(1),
            entity: ModelEntityId(1),
            expected_version: None,
            value: 1,
        },
    })?;
    let report = explore(model, SearchBounds::default())?;
    Ok(format!(
        "model: ok version={} states={} transitions={} depth={}",
        aequora_model::MODEL_VERSION,
        report.visited_states,
        report.explored_transitions,
        report.maximum_depth
    ))
}

fn verify_trace(path: Option<&str>) -> Result<String, CliError> {
    let path = path
        .ok_or_else(|| CliError::Usage("usage: aequora verify trace <failure.ron>".to_owned()))?;
    let bytes = read_bounded(Path::new(path), 16 * 1024 * 1024)?;
    let encoded = std::str::from_utf8(&bytes).map_err(|_| CliError::TraceEncoding)?;
    let trace =
        FailureTrace::from_ron(encoded).map_err(|error| CliError::TraceRon(error.to_string()))?;
    let violation = trace.replay()?;
    Ok(format!(
        "trace: reproduced invariant={} actions={}",
        violation.invariant,
        trace.actions.len()
    ))
}

fn verify_export(artifact: Option<&str>, schema: Option<&str>) -> Result<String, CliError> {
    let artifact = artifact.ok_or_else(|| {
        CliError::Usage("usage: aequora verify export <artifact.postcard> <schema.ron>".to_owned())
    })?;
    let schema = schema.ok_or_else(|| {
        CliError::Usage("usage: aequora verify export <artifact.postcard> <schema.ron>".to_owned())
    })?;
    let limits = ExportLimits::default();
    let schema_bytes = read_bounded(Path::new(schema), 8 * 1024 * 1024)?;
    let schema_text = std::str::from_utf8(&schema_bytes).map_err(|_| CliError::SchemaEncoding)?;
    let registry: SchemaRegistry =
        ron::from_str(schema_text).map_err(|error| CliError::SchemaRon(error.to_string()))?;
    registry.validate()?;
    let artifact_bytes = read_bounded(Path::new(artifact), limits.max_bytes)?;
    let artifact = CanonicalExport::decode(&artifact_bytes, limits)?;
    let verified = artifact.verify(&registry, limits)?;
    Ok(format!(
        "export: ok records={} root={}",
        verified.len(),
        hex::encode(verified.root_digest())
    ))
}

fn inspect_export(artifact: &str, schema: &str) -> Result<(u64, [u8; 32], usize), CliError> {
    let limits = ExportLimits::default();
    let registry: SchemaRegistry = read_ron(schema, 8 * 1024 * 1024)?;
    registry.validate()?;
    let artifact_bytes = read_bounded(Path::new(artifact), limits.max_bytes)?;
    let artifact = CanonicalExport::decode(&artifact_bytes, limits)?;
    let chunks = artifact.chunks.len();
    let verified = artifact.verify(&registry, limits)?;
    Ok((verified.len(), verified.root_digest(), chunks))
}

fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, CliError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(CliError::InputLimit {
            path: path.display().to_string(),
            maximum,
        });
    }
    let bytes = fs::read(path)?;
    if bytes.len() > maximum {
        return Err(CliError::InputLimit {
            path: path.display().to_string(),
            maximum,
        });
    }
    Ok(bytes)
}

fn named_manifest(name: Option<&str>) -> Result<AdapterManifest, CliError> {
    match name {
        Some("stoolap") => Ok(STOOLAP_ADAPTER_MANIFEST),
        Some("postgres" | "postgresql") => Ok(POSTGRES_ADAPTER_MANIFEST),
        Some(other) => Err(CliError::UnknownAdapter(other.to_owned())),
        None => Err(CliError::Usage("adapter name is required".to_owned())),
    }
}

fn initialize(target: Option<&str>, profile: Option<&str>) -> Result<String, CliError> {
    let target = target.ok_or_else(|| {
        CliError::Usage("usage: aequora init <new-directory> <client|server>".to_owned())
    })?;
    let profile = match profile {
        Some("client") => StarterProfile::Client,
        Some("server") => StarterProfile::Server,
        Some(other) => return Err(CliError::UnknownProfile(other.to_owned())),
        None => {
            return Err(CliError::Usage(
                "usage: aequora init <new-directory> <client|server>".to_owned(),
            ));
        }
    };
    let target = Path::new(target);
    if target.exists() {
        return Err(CliError::TargetExists(target.display().to_string()));
    }
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err(CliError::MissingParent(parent.display().to_string()));
    }
    let package_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| valid_package_name(name))
        .ok_or_else(|| CliError::InvalidPackageName(target.display().to_string()))?;

    let temporary = tempfile::Builder::new()
        .prefix(".aequora-init-")
        .tempdir_in(parent)?;
    fs::create_dir(temporary.path().join("src"))?;
    fs::write(
        temporary.path().join("Cargo.toml"),
        profile.cargo_toml(package_name),
    )?;
    fs::write(temporary.path().join("src/main.rs"), profile.main_rs())?;
    fs::write(temporary.path().join("README.md"), profile.readme())?;
    fs::rename(temporary.path(), target)?;

    Ok(format!(
        "created {} starter at {} without overwriting existing files",
        profile.name(),
        target.display()
    ))
}

fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Clone, Copy)]
enum StarterProfile {
    Client,
    Server,
}

impl StarterProfile {
    const fn name(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
        }
    }

    fn cargo_toml(self, package_name: &str) -> String {
        let features = match self {
            Self::Client => "stoolap,http-client",
            Self::Server => "postgres,axum",
        };
        format!(
            "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\naequora = {{ version = \"0.1\", features = [\"{}\", \"{}\"] }}\n",
            features
                .split_once(',')
                .map_or(features, |(first, _)| first),
            features
                .split_once(',')
                .map_or(features, |(_, second)| second),
        )
    }

    const fn main_rs(self) -> &'static str {
        match self {
            Self::Client => include_str!("../templates/client.rs"),
            Self::Server => include_str!("../templates/server.rs"),
        }
    }

    const fn readme(self) -> &'static str {
        match self {
            Self::Client => {
                "# Aequora client starter\n\nThis non-destructive starter wires no credentials. Add application authentication, a Stoolap domain transaction, and an HTTP transport explicitly.\n"
            }
            Self::Server => {
                "# Aequora server starter\n\nThis non-destructive starter includes no allow-all authentication. Add the application identity boundary, handlers, PostgreSQL configuration, and Axum routes explicitly.\n"
            }
        }
    }
}

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error(transparent)]
    Conformance(#[from] ConformanceError),
    #[error("unknown built-in adapter {0:?}")]
    UnknownAdapter(String),
    #[error(transparent)]
    Admin(#[from] aequora_admin::AdminError),
    #[error(transparent)]
    Diagnostic(#[from] DiagnosticError),
    #[error(transparent)]
    Security(#[from] SecurityError),
    #[error(transparent)]
    Feed(#[from] FeedError),
    #[error(transparent)]
    Compatibility(#[from] AdapterCompatibilityError),
    #[error(transparent)]
    ProtocolCompatibility(#[from] CompatibilityError),
    #[error("manifest serialization failed: {0}")]
    Serialize(String),
    #[error("schema RON is malformed: {0}")]
    SchemaRon(String),
    #[error("schema file must contain UTF-8 RON")]
    SchemaEncoding,
    #[error("model trace is malformed: {0}")]
    TraceRon(String),
    #[error("model trace file must contain UTF-8 RON")]
    TraceEncoding,
    #[error("integrity evidence is malformed: {0}")]
    IntegrityRon(String),
    #[error("integrity evidence must contain UTF-8 RON")]
    IntegrityEncoding,
    #[error("input {path} exceeds the {maximum}-byte safety limit")]
    InputLimit { path: String, maximum: usize },
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(transparent)]
    Export(#[from] ExportError),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Explore(#[from] ExploreError),
    #[error(transparent)]
    Replay(#[from] ReplayError),
    #[error(transparent)]
    Integrity(#[from] IntegrityError),
    #[error("queue evidence is malformed: {0}")]
    QueueRon(String),
    #[error("queue evidence must contain UTF-8 RON")]
    QueueEncoding,
    #[error(transparent)]
    Queue(#[from] QueueError),
    #[error(transparent)]
    Cutover(#[from] CutoverBlocker),
    #[error(transparent)]
    Bootstrap(#[from] BootstrapError),
    #[error(transparent)]
    Authority(#[from] AuthorityError),
    #[error(transparent)]
    Region(#[from] RegionError),
    #[error("starter target already exists: {0}")]
    TargetExists(String),
    #[error("starter parent directory does not exist: {0}")]
    MissingParent(String),
    #[error("starter directory name is not a valid Cargo package name: {0}")]
    InvalidPackageName(String),
    #[error("unknown starter profile {0:?}; expected client or server")]
    UnknownProfile(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_schema::{
        CanonicalEntityId, CanonicalFieldId, CanonicalRecord, CanonicalType, CanonicalValue,
        EntitySchema, FieldSchema,
    };
    use std::collections::BTreeMap;

    fn args<'a>(values: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
        values.iter().map(ToString::to_string)
    }

    #[test]
    fn doctor_verifies_built_in_pair_without_secrets() {
        let output = command(args(&["doctor", "adapters"]))
            .unwrap_or_else(|error| panic!("doctor failed: {error}"));
        assert!(output.contains("adapter status: ok"));
        assert!(!output.contains("postgres://"));
    }

    #[test]
    fn inspect_emits_serialized_manifest() {
        let output = command(args(&["inspect", "adapter", "stoolap"]))
            .unwrap_or_else(|error| panic!("inspect failed: {error}"));
        assert!(output.contains("name: \"stoolap\""));
        assert!(output.contains("LocalWritable"));
    }

    #[test]
    fn inspect_adapters_generates_complete_static_support_data() {
        let output = command(args(&["inspect", "adapters"]))
            .unwrap_or_else(|error| panic!("support data generation failed: {error}"));
        assert!(output.contains("name: \"stoolap\""));
        assert!(output.contains("name: \"postgresql\""));
        assert!(!output.contains("postgres://"));
    }

    #[test]
    fn reversed_pair_fails_closed() {
        assert!(command(args(&["verify", "pair", "postgresql", "stoolap"])).is_err());
    }

    #[test]
    fn verify_model_runs_the_bounded_failure_interleaving_search() {
        let output = command(args(&["verify", "model"]))
            .unwrap_or_else(|error| panic!("model verification failed: {error}"));
        assert!(output.contains("model: ok version="));
        assert!(output.contains("states="));
    }

    #[test]
    fn integrity_status_is_payload_free_and_reports_supported_generation() {
        let output = command(args(&["integrity", "status"]))
            .unwrap_or_else(|error| panic!("integrity status failed: {error}"));
        assert!(output.contains("integrity: supported generation=1"));
        assert!(output.contains("max_partitions=4096"));
    }

    #[test]
    fn compatibility_registry_command_is_read_only_and_validated() {
        let output = command(args(&["compat", "registry"]))
            .unwrap_or_else(|error| panic!("compatibility registry failed: {error}"));
        assert!(output.contains("compat registry: ok generation=1"));
        assert!(output.contains("writes=0"));
    }

    #[test]
    fn incident_explain_describes_fail_closed_read_only_boundary() {
        let output = command(args(&["incident", "explain"]))
            .unwrap_or_else(|error| panic!("incident explanation failed: {error}"));
        assert!(output.contains("tenant-authorized"));
        assert!(output.contains("replay is isolated"));
    }

    #[test]
    fn import_explain_keeps_mutation_host_owned() {
        let output = command(args(&["import", "explain"]))
            .unwrap_or_else(|error| panic!("import explanation failed: {error}"));
        assert!(output.contains("plan/validate are read-only"));
        assert!(output.contains("host authority adapter"));
    }

    #[test]
    fn bootstrap_explain_is_read_only_and_fail_closed() {
        let output = command(args(&["bootstrap", "explain"]))
            .unwrap_or_else(|error| panic!("bootstrap explanation failed: {error}"));
        assert!(output.contains("inspect/status are read-only"));
        assert!(output.contains("unchanged pending-intent digest"));
    }

    #[test]
    fn init_is_non_destructive_and_writes_a_safe_starter() {
        let root = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
        let target = root.path().join("client-app");
        let target_text = target.to_string_lossy().into_owned();
        command(vec![
            "init".to_owned(),
            target_text.clone(),
            "client".to_owned(),
        ])
        .unwrap_or_else(|error| panic!("starter generation failed: {error}"));
        let main = fs::read_to_string(target.join("src/main.rs"))
            .unwrap_or_else(|error| panic!("starter read failed: {error}"));
        assert!(main.contains("AequoraClient::builder"));
        assert!(command(vec!["init".to_owned(), target_text, "client".to_owned()]).is_err());
    }

    #[test]
    fn verify_export_checks_schema_records_and_digests() {
        let entity = CanonicalEntityId::new(1)
            .unwrap_or_else(|error| panic!("entity fixture failed: {error}"));
        let field = CanonicalFieldId::new(1)
            .unwrap_or_else(|error| panic!("field fixture failed: {error}"));
        let mut registry = SchemaRegistry::new();
        registry
            .register(EntitySchema {
                id: entity,
                name: "student".to_owned(),
                fields: vec![FieldSchema {
                    id: field,
                    name: "name".to_owned(),
                    field_type: CanonicalType::Text,
                    nullable: false,
                }],
            })
            .unwrap_or_else(|error| panic!("schema fixture failed: {error}"));
        let records = [CanonicalRecord {
            entity,
            key: CanonicalValue::Unsigned(1),
            fields: BTreeMap::from([(field, CanonicalValue::Text("Ada".to_owned()))]),
        }];
        let artifact = CanonicalExport::build(&registry, "fixture", 1, &records, 1)
            .unwrap_or_else(|error| panic!("artifact fixture failed: {error}"));
        let root = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("temporary directory failed: {error}"));
        let artifact_path = root.path().join("export.postcard");
        let schema_path = root.path().join("schema.ron");
        fs::write(
            &artifact_path,
            artifact
                .encode(ExportLimits::default())
                .unwrap_or_else(|error| panic!("artifact encoding failed: {error}")),
        )
        .unwrap_or_else(|error| panic!("artifact write failed: {error}"));
        fs::write(
            &schema_path,
            ron::ser::to_string_pretty(&registry, ron::ser::PrettyConfig::default())
                .unwrap_or_else(|error| panic!("schema encoding failed: {error}")),
        )
        .unwrap_or_else(|error| panic!("schema write failed: {error}"));
        let output = command(vec![
            "verify".to_owned(),
            "export".to_owned(),
            artifact_path.to_string_lossy().into_owned(),
            schema_path.to_string_lossy().into_owned(),
        ])
        .unwrap_or_else(|error| panic!("export verification failed: {error}"));
        assert!(output.contains("export: ok records=1"));
        let plan = command(vec![
            "import".to_owned(),
            "plan".to_owned(),
            artifact_path.to_string_lossy().into_owned(),
            schema_path.to_string_lossy().into_owned(),
        ])
        .unwrap_or_else(|error| panic!("import planning failed: {error}"));
        assert!(plan.contains("dry_run=true records=1 chunks=1"));
        assert!(plan.contains("writes=0"));
    }
}
