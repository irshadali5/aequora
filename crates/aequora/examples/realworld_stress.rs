//! Bounded local-only multi-client stress and chaos verification harness.

#[path = "support/stress_auth.rs"]
mod stress_auth;

use aequora_client::{ClientConfig, ClientSyncEngine};
use aequora_http::{HttpTransport, HttpTransportConfig, StaticRequestHeaders};
use aequora_protocol::{OperationEnvelope, OperationKind, OperationMetadata, SessionMetadata};
use aequora_scheduler::WorkClass;
use aequora_store::StoreError;
use aequora_store_stoolap::{StoolapDatabase, StoolapStore};
use aequora_types::{
    ActorId, DeviceId, EntityId, EntityRef, EntityType, HybridTimestamp, NodeId, OperationId,
    ProtocolVersion, SchemaVersion, SessionId, SyncScopeId, TenantId,
};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use reqwest::{Client, Url, redirect};
use serde::{Deserialize, Serialize};
use std::{
    env,
    error::Error,
    io,
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use stress_auth::StressAuthKey;
use tokio::{sync::Mutex, task::JoinHandle};

type DynError = Box<dyn Error + Send + Sync>;
type HarnessEngine = ClientSyncEngine<StoolapStore<StoolapDatabase>, HttpTransport>;

const TASK_ENTITY_TYPE: u16 = 10;
const MAX_CONCURRENCY: usize = 512;
const MAX_DURATION_SECONDS: u64 = 300;
const MAX_BATCH_SIZE: usize = 100;
const MAX_HOT_KEYS: usize = 4_096;
const MAX_ADVERSARIAL_CLIENTS: usize = 128;
const MAX_LATENCY_SAMPLES: usize = 200_000;
const FINAL_DRAIN_ATTEMPTS: usize = 20;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskOperation {
    pub task_id: EntityId,
    pub title: String,
    pub assignee: ActorId,
    pub priority: u8,
    pub status: String,
    pub notes: String,
}

impl TaskOperation {
    const KIND: u16 = 200;
}

#[derive(Clone, Debug)]
struct HarnessConfig {
    base_url: Url,
    concurrency: usize,
    duration: Duration,
    batch_size: usize,
    hot_keys: usize,
    adversarial_clients: usize,
    skip_functional: bool,
}

impl HarnessConfig {
    fn parse() -> Result<Self, DynError> {
        let mut config = Self {
            base_url: validate_target("http://127.0.0.1:8443")?,
            concurrency: 20,
            duration: Duration::from_secs(10),
            batch_size: 5,
            hot_keys: 0,
            adversarial_clients: 0,
            skip_functional: false,
        };
        let mut arguments = env::args().skip(1);
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--url" => config.base_url = validate_target(&next_value(&mut arguments, "url")?)?,
                "--concurrency" => {
                    config.concurrency = parse_bounded(
                        &next_value(&mut arguments, "concurrency")?,
                        "concurrency",
                        1,
                        MAX_CONCURRENCY,
                    )?;
                }
                "--duration" => {
                    let seconds = parse_bounded_u64(
                        &next_value(&mut arguments, "duration")?,
                        "duration",
                        1,
                        MAX_DURATION_SECONDS,
                    )?;
                    config.duration = Duration::from_secs(seconds);
                }
                "--batch-size" => {
                    config.batch_size = parse_bounded(
                        &next_value(&mut arguments, "batch size")?,
                        "batch size",
                        1,
                        MAX_BATCH_SIZE,
                    )?;
                }
                "--hot-keys" => {
                    config.hot_keys = parse_bounded(
                        &next_value(&mut arguments, "hot keys")?,
                        "hot keys",
                        0,
                        MAX_HOT_KEYS,
                    )?;
                }
                "--adversarial-clients" => {
                    config.adversarial_clients = parse_bounded(
                        &next_value(&mut arguments, "adversarial clients")?,
                        "adversarial clients",
                        0,
                        MAX_ADVERSARIAL_CLIENTS,
                    )?;
                }
                "--skip-functional" => config.skip_functional = true,
                _ => return Err(invalid_input("unknown stress-harness argument")),
            }
        }
        Ok(config)
    }
}

fn next_value(
    arguments: &mut impl Iterator<Item = String>,
    name: &'static str,
) -> Result<String, DynError> {
    arguments.next().ok_or_else(|| invalid_input(name))
}

fn parse_bounded(
    value: &str,
    name: &'static str,
    minimum: usize,
    maximum: usize,
) -> Result<usize, DynError> {
    let parsed = value.parse::<usize>().map_err(|_| invalid_input(name))?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(invalid_input(name));
    }
    Ok(parsed)
}

fn parse_bounded_u64(
    value: &str,
    name: &'static str,
    minimum: u64,
    maximum: u64,
) -> Result<u64, DynError> {
    let parsed = value.parse::<u64>().map_err(|_| invalid_input(name))?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(invalid_input(name));
    }
    Ok(parsed)
}

fn invalid_input(message: &'static str) -> DynError {
    io::Error::new(io::ErrorKind::InvalidInput, message).into()
}

fn validate_target(value: &str) -> Result<Url, DynError> {
    let url = Url::parse(value)?;
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if !matches!(url.scheme(), "http" | "https")
        || !loopback
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(invalid_input(
            "the stress target must be a credential-free loopback HTTP(S) origin",
        ));
    }
    Ok(url)
}

fn read_task_entity(
    database: &StoolapDatabase,
    scope: SyncScopeId,
    entity: EntityRef,
) -> Result<(TaskOperation, bool), DynError> {
    let mut rows = database.database().query(
        "SELECT payload, provisional FROM aequora_local_entities WHERE scope_id = $1 AND entity_type = $2 AND entity_id = $3",
        (
            scope.to_string(),
            i64::from(entity.entity_type.get()),
            entity.entity_id.to_string(),
        ),
    )?;
    let row = rows
        .next()
        .ok_or_else(|| invalid_input("task row is absent"))??;
    let encoded: String = row.get(0)?;
    let provisional: bool = row.get(1)?;
    let bytes = hex::decode(encoded)?;
    let operation = postcard::from_bytes(&bytes)?;
    Ok((operation, provisional))
}

fn build_http_transport(
    base_url: &Url,
    authentication_key: &StressAuthKey,
    tenant_id: TenantId,
    actor_id: ActorId,
    device_id: DeviceId,
) -> Result<HttpTransport, DynError> {
    let token = authentication_key.issue(tenant_id, actor_id, device_id);
    let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))?;
    authorization.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, authorization);
    Ok(HttpTransport::new(
        Client::builder(),
        base_url,
        StaticRequestHeaders::new(headers),
        HttpTransportConfig::default(),
    )?)
}

fn make_session(
    tenant_id: TenantId,
    actor_id: ActorId,
    device_id: DeviceId,
    scope_id: SyncScopeId,
) -> SessionMetadata {
    SessionMetadata {
        session_id: SessionId::new(),
        device_id,
        actor_id,
        tenant_id,
        scope_id,
        partitions: Vec::new(),
    }
}

fn make_envelope(
    tenant_id: TenantId,
    actor_id: ActorId,
    device_id: DeviceId,
    entity: EntityRef,
    physical_ms: i64,
    payload: Vec<u8>,
) -> OperationEnvelope {
    OperationEnvelope {
        protocol_version: ProtocolVersion::V1,
        operation_id: OperationId::new(),
        tenant_id,
        actor_id,
        device_id,
        entity,
        base_version: None,
        created_at: HybridTimestamp {
            physical_ms,
            logical: 0,
            node: NodeId::new(),
        },
        schema_version: SchemaVersion(1),
        operation_kind: OperationKind(TaskOperation::KIND),
        payload,
        metadata: OperationMetadata::default(),
    }
}

fn store_provisional(
    database: &StoolapDatabase,
    scope: SyncScopeId,
    envelope: &OperationEnvelope,
) -> Result<(), StoreError> {
    database.transact_local_mutation(envelope, |transaction| {
        let scope_id = scope.to_string();
        let entity_type = i64::from(envelope.entity.entity_type.get());
        let entity_id = envelope.entity.entity_id.to_string();
        let payload = hex::encode(&envelope.payload);
        let updated = transaction
            .execute(
                "UPDATE aequora_local_entities SET version = version + 1, payload = $4, tombstone = 0, provisional = 1 WHERE scope_id = $1 AND entity_type = $2 AND entity_id = $3",
                (
                    scope_id.clone(),
                    entity_type,
                    entity_id.clone(),
                    payload.clone(),
                ),
            )
            .map_err(|error| StoreError::transient(error.to_string()))?;
        if updated == 0 {
            transaction
                .execute(
                    "INSERT INTO aequora_local_entities (scope_id, entity_type, entity_id, version, payload, tombstone, provisional) VALUES ($1, $2, $3, 1, $4, 0, 1)",
                    (scope_id, entity_type, entity_id, payload),
                )
                .map_err(|error| StoreError::transient(error.to_string()))?;
        }
        Ok(())
    })
}

struct FunctionalContext {
    engine: HarnessEngine,
    tenant_id: TenantId,
    actor_id: ActorId,
    device_id: DeviceId,
    scope_id: SyncScopeId,
    entity: EntityRef,
}

async fn verify_initial_reconciliation(
    config: &HarnessConfig,
    authentication_key: &StressAuthKey,
) -> Result<FunctionalContext, DynError> {
    let tenant_id = TenantId::new();
    let actor_id = ActorId::new();
    let device_id = DeviceId::new();
    let scope_id = SyncScopeId::new();
    let entity = EntityRef {
        entity_type: EntityType::new(TASK_ENTITY_TYPE)?,
        entity_id: EntityId::new(),
    };
    let operation = TaskOperation {
        task_id: entity.entity_id,
        title: "Deploy an ephemeral test container".to_owned(),
        assignee: actor_id,
        priority: 1,
        status: "todo".to_owned(),
        notes: "Synthetic local stress data".to_owned(),
    };
    let database = StoolapDatabase::open_in_memory()?;
    let envelope = make_envelope(
        tenant_id,
        actor_id,
        device_id,
        entity,
        1_000,
        postcard::to_stdvec(&operation)?,
    );
    store_provisional(&database, scope_id, &envelope)?;
    let (offline, provisional) = read_task_entity(&database, scope_id, entity)?;
    ensure(
        offline.title == operation.title && provisional,
        "offline mutation was not durable and provisional",
    )?;

    let transport = build_http_transport(
        &config.base_url,
        authentication_key,
        tenant_id,
        actor_id,
        device_id,
    )?;
    let engine = ClientSyncEngine::new(
        StoolapStore::new(database),
        transport,
        ClientConfig::new(make_session(tenant_id, actor_id, device_id, scope_id)),
    );
    let outcome = engine.run_once().await?;
    ensure(outcome.acknowledged == 1, "operation was not acknowledged")?;
    let (reconciled, provisional) = read_task_entity(engine.store().backend(), scope_id, entity)?;
    ensure(
        reconciled.title == operation.title && !provisional,
        "reconciliation did not confirm the entity",
    )?;
    Ok(FunctionalContext {
        engine,
        tenant_id,
        actor_id,
        device_id,
        scope_id,
        entity,
    })
}

async fn verify_functional(
    config: &HarnessConfig,
    authentication_key: &StressAuthKey,
) -> Result<(), DynError> {
    println!("\n>>> RUNNING FUNCTIONAL VERIFICATION");
    let context = verify_initial_reconciliation(config, authentication_key).await?;
    let repeated = context.engine.run_once().await?;
    ensure(
        repeated.acknowledged == 0,
        "idempotent repeat acknowledged a duplicate",
    )?;

    let invalid = TaskOperation {
        task_id: EntityId::new(),
        title: "Invalid synthetic priority".to_owned(),
        assignee: context.actor_id,
        priority: 99,
        status: "todo".to_owned(),
        notes: String::new(),
    };
    let invalid_envelope = make_envelope(
        context.tenant_id,
        context.actor_id,
        context.device_id,
        EntityRef {
            entity_type: context.entity.entity_type,
            entity_id: invalid.task_id,
        },
        1_005,
        postcard::to_stdvec(&invalid)?,
    );
    context
        .engine
        .store()
        .backend()
        .transact_local_mutation(&invalid_envelope, |_| Ok(()))?;
    ensure(
        context.engine.run_once().await?.rejected == 1,
        "invalid domain operation was not rejected",
    )?;

    let actor_b = ActorId::new();
    let device_b = DeviceId::new();
    let transport_b = build_http_transport(
        &config.base_url,
        authentication_key,
        context.tenant_id,
        actor_b,
        device_b,
    )?;
    let engine_b = ClientSyncEngine::new(
        StoolapStore::new(StoolapDatabase::open_in_memory()?),
        transport_b,
        ClientConfig::new(make_session(
            context.tenant_id,
            actor_b,
            device_b,
            context.scope_id,
        )),
    );
    ensure(
        engine_b.run_once().await?.changes > 0,
        "second device did not observe authoritative state",
    )?;
    println!(">>> FUNCTIONAL VERIFICATION PASSED");
    Ok(())
}

fn ensure(condition: bool, message: &'static str) -> Result<(), DynError> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}

#[derive(Default)]
struct Counters {
    operations: AtomicU64,
    exchanges: AtomicU64,
    unhandled_errors: AtomicU64,
    transient_errors: AtomicU64,
    conflicts: AtomicU64,
    attacks: AtomicU64,
    blocked_attacks: AtomicU64,
    dropped_latency_samples: AtomicU64,
}

fn spawn_legitimate_worker(
    client_index: usize,
    config: HarnessConfig,
    authentication_key: StressAuthKey,
    shared: WorkerShared,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        if run_legitimate_worker(client_index, &config, &authentication_key, &shared)
            .await
            .is_err()
        {
            shared
                .counters
                .unhandled_errors
                .fetch_add(1, Ordering::Relaxed);
        }
    })
}

async fn run_legitimate_worker(
    client_index: usize,
    config: &HarnessConfig,
    authentication_key: &StressAuthKey,
    shared: &WorkerShared,
) -> Result<(), ()> {
    let identity = WorkerIdentity {
        tenant: shared.population.tenant,
        actor: shared.population.actor,
        device: DeviceId::new(),
        scope: shared.population.scope,
        entity_type: shared.population.entity_type,
    };
    let database = StoolapDatabase::open_in_memory().map_err(|_| ())?;
    let transport = build_http_transport(
        &config.base_url,
        authentication_key,
        identity.tenant,
        identity.actor,
        identity.device,
    )
    .map_err(|_| ())?;
    let engine = ClientSyncEngine::new(
        StoolapStore::new(database),
        transport,
        ClientConfig::new(make_session(
            identity.tenant,
            identity.actor,
            identity.device,
            identity.scope,
        )),
    );

    let mut sequence = 1_u64;
    while Instant::now() < shared.stop_time {
        offer_batch(
            &engine,
            identity,
            client_index,
            config,
            &shared.hot_keys,
            &mut sequence,
            &shared.counters,
        )?;
        exchange_once(&engine, &shared.counters, &shared.latencies).await;
        tokio::time::sleep(Duration::from_millis(3)).await;
    }
    drain_worker(&engine, &shared.counters).await
}

#[derive(Clone, Copy)]
struct WorkerIdentity {
    tenant: TenantId,
    actor: ActorId,
    device: DeviceId,
    scope: SyncScopeId,
    entity_type: EntityType,
}

#[derive(Clone, Copy)]
struct WorkerPopulation {
    tenant: TenantId,
    actor: ActorId,
    scope: SyncScopeId,
    entity_type: EntityType,
}

#[derive(Clone)]
struct WorkerShared {
    population: WorkerPopulation,
    stop_time: Instant,
    hot_keys: Arc<Vec<EntityId>>,
    counters: Arc<Counters>,
    latencies: Arc<Mutex<Vec<f64>>>,
}

fn offer_batch(
    engine: &HarnessEngine,
    identity: WorkerIdentity,
    client_index: usize,
    config: &HarnessConfig,
    hot_keys: &[EntityId],
    sequence: &mut u64,
    counters: &Counters,
) -> Result<(), ()> {
    for batch_index in 0..config.batch_size {
        let task_id = if config.hot_keys > 0 && batch_index.is_multiple_of(2) {
            let length = u64::try_from(hot_keys.len()).map_err(|_| ())?;
            let index = usize::try_from(*sequence % length).map_err(|_| ())?;
            *hot_keys.get(index).ok_or(())?
        } else {
            EntityId::new()
        };
        let priority = u8::try_from((*sequence % 5) + 1).map_err(|_| ())?;
        let operation = TaskOperation {
            task_id,
            title: format!("Synthetic task {client_index}-{sequence}"),
            assignee: identity.actor,
            priority,
            status: if sequence.is_multiple_of(2) {
                "todo".to_owned()
            } else {
                "in_progress".to_owned()
            },
            notes: "Synthetic Aequora stress payload".to_owned(),
        };
        *sequence = sequence.checked_add(1).ok_or(())?;
        let physical_ms = i64::try_from(*sequence)
            .map_err(|_| ())?
            .saturating_add(1_000);
        let envelope = make_envelope(
            identity.tenant,
            identity.actor,
            identity.device,
            EntityRef {
                entity_type: identity.entity_type,
                entity_id: task_id,
            },
            physical_ms,
            postcard::to_stdvec(&operation).map_err(|_| ())?,
        );
        if let Err(error) = store_provisional(engine.store().backend(), identity.scope, &envelope) {
            record_unhandled(counters, "local provisional write", &error);
        }
    }
    Ok(())
}

async fn exchange_once(engine: &HarnessEngine, counters: &Counters, latencies: &Mutex<Vec<f64>>) {
    engine.request_work_class(WorkClass::Interactive);
    let started = Instant::now();
    match engine.run_once().await {
        Ok(outcome) => {
            counters.operations.fetch_add(
                u64::try_from(outcome.acknowledged).unwrap_or(u64::MAX),
                Ordering::Relaxed,
            );
            counters.exchanges.fetch_add(1, Ordering::Relaxed);
            counters.conflicts.fetch_add(
                u64::try_from(outcome.conflicts).unwrap_or(u64::MAX),
                Ordering::Relaxed,
            );
            let mut samples = latencies.lock().await;
            if samples.len() < MAX_LATENCY_SAMPLES {
                samples.push(started.elapsed().as_secs_f64() * 1_000.0);
            } else {
                counters
                    .dropped_latency_samples
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        Err(error) if error.is_transient() => {
            counters.transient_errors.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        Err(error) => {
            record_unhandled(counters, "sync exchange", &error);
        }
    }
}

async fn drain_worker(engine: &HarnessEngine, counters: &Counters) -> Result<(), ()> {
    for _ in 0..FINAL_DRAIN_ATTEMPTS {
        engine.request_work_class(WorkClass::Interactive);
        match engine.run_once().await {
            Ok(outcome) => {
                counters.operations.fetch_add(
                    u64::try_from(outcome.acknowledged).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
            }
            Err(error) if error.is_transient() => {
                counters.transient_errors.fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => {
                record_unhandled(counters, "final outbox drain", &error);
            }
        }
        if pending_outbox(engine.store().backend()).map_err(|_| ())? == 0 {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err(())
}

fn pending_outbox(database: &StoolapDatabase) -> Result<i64, DynError> {
    Ok(database.database().query_one::<i64, _>(
        "SELECT COUNT(*) FROM aequora_outbox WHERE state IN ('pending', 'sending', 'retry')",
        (),
    )?)
}

fn record_unhandled(counters: &Counters, context: &str, error: &impl std::fmt::Display) {
    let previous = counters.unhandled_errors.fetch_add(1, Ordering::Relaxed);
    if previous < 5 {
        eprintln!("unhandled harness failure during {context}: {error}");
    }
}

fn spawn_adversarial_worker(
    config: HarnessConfig,
    client: Client,
    valid_authorization: HeaderValue,
    stop_time: Instant,
    counters: Arc<Counters>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Ok(exchange_url) = config.base_url.join("sync/v1/exchange") else {
            counters.unhandled_errors.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let mut sequence = 0_u64;
        while Instant::now() < stop_time {
            counters.attacks.fetch_add(1, Ordering::Relaxed);
            sequence = sequence.saturating_add(1);
            let response = match sequence % 3 {
                0 => {
                    client
                        .post(exchange_url.clone())
                        .header(AUTHORIZATION, "Bearer invalid-forged-token")
                        .header("Content-Type", "application/vnd.aequora.postcard")
                        .body(vec![0xAA; 128])
                        .send()
                        .await
                }
                1 => {
                    client
                        .post(exchange_url.clone())
                        .header(AUTHORIZATION, valid_authorization.clone())
                        .header("Content-Type", "application/vnd.aequora.postcard")
                        .body(vec![0xFF, 0x00, 0xDE, 0xAD, 0xBE, 0xEF, 0x42])
                        .send()
                        .await
                }
                _ => {
                    client
                        .post(exchange_url.clone())
                        .header(AUTHORIZATION, valid_authorization.clone())
                        .header("Content-Type", "application/json")
                        .body(r#"{"attack":"malformed_json_payload"}"#)
                        .send()
                        .await
                }
            };
            match response {
                Ok(response) if response.status().is_client_error() => {
                    counters.blocked_attacks.fetch_add(1, Ordering::Relaxed);
                }
                Ok(_) | Err(_) => {
                    counters.unhandled_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
}

async fn run_stress(
    config: &HarnessConfig,
    authentication_key: &StressAuthKey,
) -> Result<(), DynError> {
    println!("\n>>> RUNNING BOUNDED STRESS AND CHAOS VERIFICATION");
    let counters = Arc::new(Counters::default());
    let latencies = Arc::new(Mutex::new(Vec::with_capacity(
        MAX_LATENCY_SAMPLES.min(config.concurrency.saturating_mul(1_024)),
    )));
    let hot_keys = Arc::new(
        (0..config.hot_keys.max(1))
            .map(|_| EntityId::new())
            .collect::<Vec<_>>(),
    );
    let started = Instant::now();
    let stop_time = started + config.duration;
    let handle_count = config
        .concurrency
        .checked_add(config.adversarial_clients)
        .ok_or_else(|| invalid_input("worker count overflow"))?;
    let mut handles = Vec::new();
    handles.try_reserve_exact(handle_count)?;
    let population = WorkerPopulation {
        tenant: TenantId::new(),
        actor: ActorId::new(),
        scope: SyncScopeId::new(),
        entity_type: EntityType::new(TASK_ENTITY_TYPE)?,
    };
    let shared = WorkerShared {
        population,
        stop_time,
        hot_keys,
        counters: counters.clone(),
        latencies: latencies.clone(),
    };

    for client_index in 0..config.concurrency {
        handles.push(spawn_legitimate_worker(
            client_index,
            config.clone(),
            authentication_key.clone(),
            shared.clone(),
        ));
    }
    if config.adversarial_clients > 0 {
        let client = Client::builder()
            .redirect(redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()?;
        let token = authentication_key.issue(TenantId::new(), ActorId::new(), DeviceId::new());
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))?;
        authorization.set_sensitive(true);
        for _ in 0..config.adversarial_clients {
            handles.push(spawn_adversarial_worker(
                config.clone(),
                client.clone(),
                authorization.clone(),
                stop_time,
                counters.clone(),
            ));
        }
    }
    for handle in handles {
        if handle.await.is_err() {
            counters.unhandled_errors.fetch_add(1, Ordering::Relaxed);
        }
    }
    report_and_validate(started.elapsed(), &counters, &latencies).await
}

async fn report_and_validate(
    elapsed: Duration,
    counters: &Counters,
    latencies: &Mutex<Vec<f64>>,
) -> Result<(), DynError> {
    let operations = counters.operations.load(Ordering::Relaxed);
    let exchanges = counters.exchanges.load(Ordering::Relaxed);
    let errors = counters.unhandled_errors.load(Ordering::Relaxed);
    let transient = counters.transient_errors.load(Ordering::Relaxed);
    let conflicts = counters.conflicts.load(Ordering::Relaxed);
    let attacks = counters.attacks.load(Ordering::Relaxed);
    let blocked = counters.blocked_attacks.load(Ordering::Relaxed);
    let dropped_samples = counters.dropped_latency_samples.load(Ordering::Relaxed);
    let mut samples = latencies.lock().await.clone();
    samples.sort_by(f64::total_cmp);
    let percentile = |percent: usize| {
        samples
            .get(samples.len().saturating_mul(percent) / 100)
            .copied()
            .unwrap_or(0.0)
    };
    let elapsed_seconds = elapsed.as_secs_f64();

    println!("==========================================================================");
    println!("Elapsed: {elapsed_seconds:.2}s; exchanges: {exchanges}; operations: {operations}");
    println!("Errors: {errors}; expected transient faults: {transient}; conflicts: {conflicts}");
    println!("Attacks rejected: {blocked}/{attacks}; dropped latency samples: {dropped_samples}");
    println!(
        "Throughput: {:.2} ops/s; latency p50/p95/p99: {:.2}/{:.2}/{:.2} ms",
        u64_as_f64(operations) / elapsed_seconds,
        percentile(50),
        percentile(95),
        percentile(99),
    );
    println!("==========================================================================");

    ensure(errors == 0, "the harness observed unhandled failures")?;
    ensure(operations > 0, "the harness committed no operations")?;
    ensure(
        attacks == blocked,
        "one or more adversarial requests were not rejected",
    )?;
    println!(">>> STRESS RESULT: VERIFIED WITH NO OBSERVED INVARIANT FAILURE");
    Ok(())
}

fn u64_as_f64(value: u64) -> f64 {
    let high = u32::try_from(value >> 32).unwrap_or_default();
    let low = u32::try_from(value & u64::from(u32::MAX)).unwrap_or_default();
    f64::from(high) * 4_294_967_296.0 + f64::from(low)
}

fn print_configuration(config: &HarnessConfig) {
    println!("==========================================================================");
    println!("  Aequora bounded local stress and chaos verification harness");
    println!("==========================================================================");
    println!(
        "Target: {}; clients: {}; duration: {:?}",
        config.base_url, config.concurrency, config.duration
    );
    println!(
        "Batch: {}; hot keys: {}; adversarial clients: {}",
        config.batch_size, config.hot_keys, config.adversarial_clients
    );
    println!("Synthetic data only; the target is restricted to a loopback origin.");
}

#[tokio::main]
async fn main() -> Result<(), DynError> {
    let config = HarnessConfig::parse()?;
    let authentication_key = StressAuthKey::from_environment()?;
    print_configuration(&config);
    if !config.skip_functional {
        verify_functional(&config, &authentication_key).await?;
    }
    run_stress(&config, &authentication_key).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_and_numeric_bounds_fail_closed() {
        assert!(validate_target("http://127.0.0.1:8443").is_ok());
        assert!(validate_target("https://localhost:8443").is_ok());
        assert!(validate_target("http://example.com:8443").is_err());
        assert!(validate_target("http://user:secret@127.0.0.1:8443").is_err());
        assert!(parse_bounded("0", "clients", 1, MAX_CONCURRENCY).is_err());
        assert!(parse_bounded("513", "clients", 1, MAX_CONCURRENCY).is_err());
    }
}
