//! Ephemeral authoritative server used only by the local stress and chaos harness.

#[path = "support/stress_auth.rs"]
mod stress_auth;

use aequora_axum::{
    AuthenticationFailure, AxumConfig, HttpAuthenticator, PresentedCredential, ReadinessProbe,
    router_with_authenticator,
};
use aequora_clock::TestClock;
use aequora_conflict::RejectConflicts;
use aequora_executor::{
    AuthContext, AuthoritativeMutation, CurrentEntity, DomainOperation, ExecutionError,
    OperationHandler, OperationRegistry, ScopeAuthorizer,
};
use aequora_observability::NoopObserver;
use aequora_protocol::{ChangeKind, OperationEnvelope, SessionMetadata};
use aequora_server::{ExchangeService, SyncServer};
use aequora_testkit::InMemoryAuthoritativeStore;
use aequora_types::{ActorId, EntityId, NodeId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    env,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use stress_auth::StressAuthKey;

const DEFAULT_PORT: u16 = 8_443;
const MAX_TITLE_BYTES: usize = 256;
const MAX_STATUS_BYTES: usize = 32;
const MAX_NOTES_BYTES: usize = 4_096;
const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskOperation {
    pub task_id: EntityId,
    pub title: String,
    pub assignee: ActorId,
    pub priority: u8,
    pub status: String,
    pub notes: String,
}

impl DomainOperation for TaskOperation {
    const KIND: u16 = 200;
    const CURRENT_SCHEMA: u16 = 1;
}

struct TaskScopeAuthorizer;

#[async_trait]
impl ScopeAuthorizer for TaskScopeAuthorizer {
    async fn authorize_scope(
        &self,
        auth: &AuthContext,
        session: &SessionMetadata,
    ) -> Result<(), ExecutionError> {
        if auth.tenant_id == session.tenant_id {
            Ok(())
        } else {
            Err(ExecutionError::unauthorized("tenant mismatch"))
        }
    }
}

struct TaskOperationHandler;

#[async_trait]
impl OperationHandler<TaskOperation> for TaskOperationHandler {
    async fn authorize(
        &self,
        auth: &AuthContext,
        command: &TaskOperation,
        envelope: &OperationEnvelope,
    ) -> Result<(), ExecutionError> {
        if command.task_id != envelope.entity.entity_id || command.assignee != auth.actor_id {
            return Err(ExecutionError::unauthorized("task identity mismatch"));
        }
        if command.title.trim().is_empty() || command.title.len() > MAX_TITLE_BYTES {
            return Err(ExecutionError::business_rule("task title cannot be empty"));
        }
        if command.priority < 1 || command.priority > 5 {
            return Err(ExecutionError::business_rule(
                "priority must be between 1 and 5",
            ));
        }
        if command.status.len() > MAX_STATUS_BYTES
            || !matches!(
                command.status.as_str(),
                "todo" | "in_progress" | "completed"
            )
        {
            return Err(ExecutionError::business_rule("invalid task status"));
        }
        if command.notes.len() > MAX_NOTES_BYTES {
            return Err(ExecutionError::business_rule(
                "task notes exceed their limit",
            ));
        }
        Ok(())
    }

    async fn execute(
        &self,
        _auth: &AuthContext,
        command: &TaskOperation,
        _envelope: &OperationEnvelope,
        _current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        let payload = postcard::to_stdvec(command)
            .map_err(|_| ExecutionError::invalid_operation("failed to serialize task operation"))?;
        Ok(AuthoritativeMutation {
            payload,
            change_kind: ChangeKind::Upsert,
        })
    }
}

struct BearerAuthenticator(StressAuthKey);

#[async_trait]
impl HttpAuthenticator for BearerAuthenticator {
    async fn authenticate(
        &self,
        credential: &PresentedCredential,
    ) -> Result<AuthContext, AuthenticationFailure> {
        self.0
            .authenticate(credential.expose_for_authentication())
            .ok_or(AuthenticationFailure::Invalid)
    }
}

#[derive(Clone, Copy, Debug)]
struct ServerReadiness;

#[async_trait]
impl ReadinessProbe for ServerReadiness {
    async fn ready(&self) -> bool {
        true
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = bind_address()?;
    let authentication_key = StressAuthKey::from_environment()?;

    println!("===============================================================");
    println!("  Aequora local stress harness - ephemeral test server");
    println!("===============================================================");
    println!("WARNING: in-memory state and plaintext local HTTP; never deploy as production.");
    println!("Binding address: http://{addr}");
    println!("Protocol: Postcard/AEQ1 over HTTP/1.1");
    println!("Health live endpoint:  http://{addr}/sync/v1/health/live");
    println!("Health ready endpoint: http://{addr}/sync/v1/health/ready");
    println!("Sync exchange endpoint: http://{addr}/sync/v1/exchange");
    println!("Bootstrap endpoint:     http://{addr}/sync/v1/bootstrap");

    let node_id = NodeId::new();
    let authoritative_store = Arc::new(InMemoryAuthoritativeStore::default());

    let mut registry = OperationRegistry::new(TaskScopeAuthorizer);
    registry.register::<TaskOperation, _>(TaskOperationHandler)?;

    let service: Arc<dyn ExchangeService> = Arc::new(SyncServer::new(
        authoritative_store.clone(),
        Arc::new(registry),
        Arc::new(RejectConflicts),
        Arc::new(TestClock::new(node_id, 10_000)),
    ));

    let authenticator = Arc::new(BearerAuthenticator(authentication_key));
    let observer = Arc::new(NoopObserver);
    let readiness = Arc::new(ServerReadiness);
    let config = AxumConfig::new(MAX_BODY_BYTES);

    let (app, _lifecycle) =
        router_with_authenticator(service, config, observer, readiness, authenticator);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Aequora Server is listening on {addr}. Ready to process client sync sessions.");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    println!("Aequora Server shutdown completed cleanly.");
    Ok(())
}

async fn shutdown_signal() {
    let interrupt = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = interrupt => {
                        if let Err(error) = result {
                            eprintln!("failed to listen for Ctrl-C: {error}");
                        }
                    }
                    _ = terminate.recv() => {}
                }
            }
            Err(error) => {
                eprintln!("failed to listen for SIGTERM: {error}");
                let _ = interrupt.await;
            }
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = interrupt.await {
        eprintln!("failed to listen for Ctrl-C: {error}");
    }
}

fn bind_address() -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let port = match env::var("PORT") {
        Ok(value) => value.parse::<u16>()?,
        Err(env::VarError::NotPresent) => DEFAULT_PORT,
        Err(error) => return Err(error.into()),
    };
    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let ip = host.parse::<IpAddr>()?;
    if !ip.is_loopback() && env::var("AEQUORA_STRESS_ALLOW_CONTAINER_BIND").as_deref() != Ok("1") {
        return Err("non-loopback stress-server binding requires explicit container opt-in".into());
    }
    Ok(SocketAddr::new(ip, port))
}
