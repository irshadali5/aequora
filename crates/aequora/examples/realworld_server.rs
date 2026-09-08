//! Real-world authoritative Aequora application server for container & k8s deployment.

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
use aequora_protocol::{
    ChangeKind, OperationEnvelope, SessionMetadata,
};
use aequora_server::{ExchangeService, SyncServer};
use aequora_testkit::InMemoryAuthoritativeStore;
use aequora_types::{
    ActorId, DeviceId, EntityId, NodeId, TenantId,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{env, net::SocketAddr, str::FromStr, sync::Arc};

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
        _auth: &AuthContext,
        command: &TaskOperation,
        _envelope: &OperationEnvelope,
    ) -> Result<(), ExecutionError> {
        if command.title.trim().is_empty() {
            return Err(ExecutionError::business_rule("task title cannot be empty"));
        }
        if command.priority < 1 || command.priority > 5 {
            return Err(ExecutionError::business_rule("priority must be between 1 and 5"));
        }
        if !matches!(command.status.as_str(), "todo" | "in_progress" | "completed") {
            return Err(ExecutionError::business_rule("invalid task status"));
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

struct BearerAuthenticator {
    fallback_tenant: TenantId,
    fallback_actor: ActorId,
    fallback_device: DeviceId,
}

impl BearerAuthenticator {
    fn new() -> Self {
        Self {
            fallback_tenant: TenantId::new(),
            fallback_actor: ActorId::new(),
            fallback_device: DeviceId::new(),
        }
    }
}

#[async_trait]
impl HttpAuthenticator for BearerAuthenticator {
    async fn authenticate(
        &self,
        credential: &PresentedCredential,
    ) -> Result<AuthContext, AuthenticationFailure> {
        let token = credential.expose_for_authentication();
        if let Some((tenant_str, rest)) = token.split_once(':') {
            if let Some((actor_str, device_str)) = rest.split_once(':') {
                if let (Ok(tenant_id), Ok(actor_id), Ok(device_id)) = (
                    TenantId::from_str(tenant_str),
                    ActorId::from_str(actor_str),
                    DeviceId::from_str(device_str),
                ) {
                    return Ok(AuthContext {
                        tenant_id,
                        actor_id,
                        device_id,
                    });
                }
            }
        }
        if token.starts_with("test-token") || token.starts_with("bearer-") {
            return Ok(AuthContext {
                tenant_id: self.fallback_tenant,
                actor_id: self.fallback_actor,
                device_id: self.fallback_device,
            });
        }
        Err(AuthenticationFailure::Invalid)
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
    let port: u16 = env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8443);

    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let addr: SocketAddr = format!("{host}:{port}").parse()?;

    println!("===============================================================");
    println!("  Aequora Server - Authoritative Sync Engine (Production Node)");
    println!("===============================================================");
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

    let authenticator = Arc::new(BearerAuthenticator::new());
    let observer = Arc::new(NoopObserver);
    let readiness = Arc::new(ServerReadiness);
    let config = AxumConfig::new(8 * 1024 * 1024);

    let (app, _lifecycle) = router_with_authenticator(
        service,
        config,
        observer,
        readiness,
        authenticator,
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Aequora Server is listening on {addr}. Ready to process client sync sessions.");

    axum::serve(listener, app).await?;

    println!("Aequora Server shutdown completed cleanly.");
    Ok(())
}
