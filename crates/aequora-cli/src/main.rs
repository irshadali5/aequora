//! Payload-free diagnostics for statically linked Aequora database adapters.

use aequora_authority::{
    AuthorityController, AuthorityError, AuthorityPromotionPlan, AuthorityPromotionPolicy,
    AuthorityState, CheckpointComparison, JournalCheckpoint, PromotionEvidence,
    RecoveryVerification, compare_checkpoints,
};
use aequora_bootstrap::{BootstrapError, BootstrapJob, SnapshotManifest};
use aequora_integrity::{
    CURRENT_HASH_SCHEMA, CURRENT_INTEGRITY_GENERATION, IntegrityError, IntegritySnapshot,
    PartitionScheme, RepairPlan,
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
use aequora_store::{
    AdapterCompatibilityError, AdapterManifest, AdapterRequirements, ProductionAdapterPair,
};
use aequora_store_postgres::POSTGRES_ADAPTER_MANIFEST;
use aequora_store_stoolap::STOOLAP_ADAPTER_MANIFEST;
use std::{env, fs, io, path::Path, process::ExitCode};
use thiserror::Error;

fn main() -> ExitCode {
    match command(env::args().skip(1)) {
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
        Some(other) => Err(CliError::Usage(format!(
            "unknown command {other:?}; run `aequora help`"
        ))),
    }
}

fn help() -> &'static str {
    "aequora doctor adapters\naequora inspect adapters\naequora inspect adapter <stoolap|postgresql>\naequora verify pair <local> <authority>\naequora verify export <artifact.postcard> <schema.ron>\naequora verify model\naequora verify trace <failure.ron>\naequora integrity status\naequora integrity verify <snapshot.ron>\naequora integrity explain <repair-plan.ron>\naequora queue status <entries.ron>\naequora queue verify <entries.ron>\naequora queue compact <entries.ron> <registry.ron>\naequora queue explain <plan.ron>\naequora import plan <artifact.postcard> <schema.ron>\naequora import validate <artifact.postcard> <schema.ron>\naequora import status <job.ron>\naequora import cutover <evidence.ron>\naequora import explain\naequora bootstrap inspect <manifest.ron>\naequora bootstrap status <job.ron>\naequora bootstrap explain\naequora authority status <state.ron>\naequora authority readiness <evidence.ron>\naequora authority promote <plan.ron>\naequora authority demote <state.ron>\naequora authority restore-plan <state.ron>\naequora authority recover <state.ron> --new-epoch\naequora authority verify <verification.ron>\naequora authority fork-check <local.ron> <peer.ron>\naequora authority explain\naequora region status <topology.ron>\naequora region route <topology.ron> <request.ron>\naequora region explain\naequora init <new-directory> <client|server>"
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
    #[error("unknown built-in adapter {0:?}")]
    UnknownAdapter(String),
    #[error(transparent)]
    Compatibility(#[from] AdapterCompatibilityError),
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
