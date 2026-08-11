use aequora_axum::{
    AxumConfig, DrainOutcome, POSTCARD_CONTENT_TYPE, ReadinessProbe, router_with_lifecycle,
    router_with_readiness,
};
use aequora_codec::{FLAG_ZSTD, MessageKind};
use aequora_executor::AuthContext;
use aequora_observability::AtomicMetrics;
use aequora_protocol::{
    BootstrapRequest, BootstrapResponse, Capability, ChangeKind, ClientLimits, RemoteChange,
    SessionMetadata, SnapshotLimits, SyncDirective, SyncRequest, SyncResponse,
};
use aequora_server::{ExchangeService, ServerError};
use aequora_types::{
    ActorId, Cursor, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, HybridTimestamp,
    NodeId, OperationId, ProtocolVersion, RequestId, Sequence, SessionId, SyncScopeId, TenantId,
};
use async_trait::async_trait;
use axum::{
    Extension,
    body::{Body, Bytes, to_bytes},
    http::{
        Request, StatusCode,
        header::{CONTENT_TYPE, RETRY_AFTER},
    },
};
use futures_util::stream;
use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{sync::Notify, time::sleep};
use tower::ServiceExt;

struct EchoService;

#[async_trait]
impl ExchangeService for EchoService {
    async fn exchange(
        &self,
        _auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, ServerError> {
        let compressed_response = request.capabilities.contains(&Capability::Zstd);
        let changes = if compressed_response {
            vec![RemoteChange {
                tenant_id: request.session.tenant_id,
                scope_id: request.session.scope_id,
                sequence: Sequence(1),
                operation_id: OperationId::new(),
                entity: EntityRef {
                    entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
                    entity_id: EntityId::new(),
                },
                version: EntityVersion::INITIAL,
                change_kind: ChangeKind::Upsert,
                payload: vec![42; 32 * 1_024],
                timestamp: HybridTimestamp {
                    physical_ms: 1,
                    logical: 0,
                    node: NodeId::new(),
                },
            }]
        } else {
            Vec::new()
        };
        Ok(SyncResponse {
            protocol: ProtocolVersion::V1,
            directive: SyncDirective::Continue,
            acknowledged: Vec::new(),
            rejected: Vec::new(),
            conflicts: Vec::new(),
            changes,
            next_cursor: Cursor {
                scope: request.session.scope_id,
                sequence: if compressed_response {
                    Sequence(1)
                } else {
                    Sequence(0)
                },
            },
            has_more: false,
            server_time: HybridTimestamp {
                physical_ms: 1,
                logical: 0,
                node: NodeId::new(),
            },
        })
    }

    async fn bootstrap(
        &self,
        _auth: AuthContext,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, ServerError> {
        Ok(BootstrapResponse {
            protocol: ProtocolVersion::V1,
            snapshot_id: request.snapshot_id.unwrap_or_default(),
            cursor: Cursor {
                scope: request.session.scope_id,
                sequence: Sequence(0),
            },
            offset: request.offset,
            entities: Vec::new(),
            next_offset: request.offset,
            has_more: false,
            server_time: HybridTimestamp {
                physical_ms: 1,
                logical: 0,
                node: NodeId::new(),
            },
        })
    }
}

#[tokio::test]
async fn framed_postcard_exchange_round_trips_through_axum() {
    let tenant = TenantId::new();
    let actor = ActorId::new();
    let device = DeviceId::new();
    let scope = SyncScopeId::new();
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: tenant,
        device_id: device,
    };
    let sync_request = SyncRequest {
        protocol: ProtocolVersion::V1,
        request_id: RequestId::new(),
        session: SessionMetadata {
            session_id: SessionId::new(),
            device_id: device,
            actor_id: actor,
            tenant_id: tenant,
            scope_id: scope,
            partitions: Vec::new(),
        },
        cursor: None,
        operations: Vec::new(),
        limits: ClientLimits::default(),
        capabilities: Vec::new(),
    };
    let frame = aequora_codec::encode(ProtocolVersion::V1, MessageKind::SyncRequest, &sync_request)
        .unwrap_or_else(|error| panic!("{error}"));
    let app = aequora_axum::router(Arc::new(EchoService), 1_024 * 1_024).layer(Extension(auth));
    let request = Request::post("/sync/v1/exchange")
        .header(CONTENT_TYPE, POSTCARD_CONTENT_TYPE)
        .body(Body::from(frame))
        .unwrap_or_else(|error| panic!("{error}"));

    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| match error {});

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some(POSTCARD_CONTENT_TYPE)
    );
    let body = to_bytes(response.into_body(), 1_024 * 1_024)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        aequora_codec::decode_frame(&body, 1_024 * 1_024)
            .unwrap_or_else(|error| panic!("{error}"))
            .flags
            & FLAG_ZSTD,
        0
    );
    let (_, decoded) =
        aequora_codec::decode::<SyncResponse>(&body, MessageKind::SyncResponse, 1_024 * 1_024)
            .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(decoded.next_cursor.scope, scope);
}

#[tokio::test]
async fn bootstrap_uses_its_distinct_framed_message_kind() {
    let tenant = TenantId::new();
    let actor = ActorId::new();
    let device = DeviceId::new();
    let scope = SyncScopeId::new();
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: tenant,
        device_id: device,
    };
    let bootstrap_request = BootstrapRequest {
        protocol: ProtocolVersion::V1,
        request_id: RequestId::new(),
        session: SessionMetadata {
            session_id: SessionId::new(),
            device_id: device,
            actor_id: actor,
            tenant_id: tenant,
            scope_id: scope,
            partitions: Vec::new(),
        },
        snapshot_id: None,
        offset: 0,
        limits: SnapshotLimits::default(),
        capabilities: Vec::new(),
    };
    let frame = aequora_codec::encode(
        ProtocolVersion::V1,
        MessageKind::BootstrapRequest,
        &bootstrap_request,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let app = aequora_axum::router(Arc::new(EchoService), 1_024 * 1_024).layer(Extension(auth));
    let request = Request::post("/sync/v1/bootstrap")
        .header(CONTENT_TYPE, POSTCARD_CONTENT_TYPE)
        .body(Body::from(frame))
        .unwrap_or_else(|error| panic!("{error}"));

    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1_024 * 1_024)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let (_, decoded) = aequora_codec::decode::<BootstrapResponse>(
        &body,
        MessageKind::BootstrapResponse,
        1_024 * 1_024,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(decoded.cursor.scope, scope);
}

#[tokio::test]
async fn zstd_response_requires_the_client_capability() {
    let tenant = TenantId::new();
    let actor = ActorId::new();
    let device = DeviceId::new();
    let scope = SyncScopeId::new();
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: tenant,
        device_id: device,
    };
    let sync_request = SyncRequest {
        protocol: ProtocolVersion::V1,
        request_id: RequestId::new(),
        session: SessionMetadata {
            session_id: SessionId::new(),
            device_id: device,
            actor_id: actor,
            tenant_id: tenant,
            scope_id: scope,
            partitions: Vec::new(),
        },
        cursor: None,
        operations: Vec::new(),
        limits: ClientLimits::default(),
        capabilities: vec![Capability::Zstd],
    };
    let frame = aequora_codec::encode(ProtocolVersion::V1, MessageKind::SyncRequest, &sync_request)
        .unwrap_or_else(|error| panic!("{error}"));
    let app = aequora_axum::router_with_config(
        Arc::new(EchoService),
        AxumConfig {
            max_body_bytes: 1_024 * 1_024,
            max_decompressed_bytes: 1_024 * 1_024,
            compression_threshold: 1,
            zstd_level: 1,
            zstd_enabled: true,
            ..AxumConfig::new(1_024 * 1_024)
        },
    )
    .layer(Extension(auth));
    let request = Request::post("/sync/v1/exchange")
        .header(CONTENT_TYPE, POSTCARD_CONTENT_TYPE)
        .body(Body::from(frame))
        .unwrap_or_else(|error| panic!("{error}"));

    let response = app
        .oneshot(request)
        .await
        .unwrap_or_else(|error| match error {});
    let body = to_bytes(response.into_body(), 1_024 * 1_024)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let decoded_frame =
        aequora_codec::decode_frame(&body, 1_024 * 1_024).unwrap_or_else(|error| panic!("{error}"));
    assert_ne!(decoded_frame.flags & FLAG_ZSTD, 0);
    let (_, decoded) =
        aequora_codec::decode::<SyncResponse>(&body, MessageKind::SyncResponse, 1_024 * 1_024)
            .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(decoded.changes.len(), 1);
}

struct SlowService {
    entered: Arc<Notify>,
    delay: Duration,
}

struct CountingService {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ExchangeService for CountingService {
    async fn exchange(
        &self,
        _auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, ServerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(empty_response(&request))
    }
}

#[async_trait]
impl ExchangeService for SlowService {
    async fn exchange(
        &self,
        _auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, ServerError> {
        self.entered.notify_one();
        sleep(self.delay).await;
        Ok(empty_response(&request))
    }
}

struct StaticReadiness(bool);

#[async_trait]
impl ReadinessProbe for StaticReadiness {
    async fn ready(&self) -> bool {
        self.0
    }
}

struct SlowReadiness(Duration);

#[async_trait]
impl ReadinessProbe for SlowReadiness {
    async fn ready(&self) -> bool {
        sleep(self.0).await;
        true
    }
}

struct CountingReadiness {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ReadinessProbe for CountingReadiness {
    async fn ready(&self) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        true
    }
}

struct GatedService {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

struct GatedReadiness {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl ReadinessProbe for GatedReadiness {
    async fn ready(&self) -> bool {
        self.entered.notify_one();
        self.release.notified().await;
        true
    }
}

#[async_trait]
impl ExchangeService for GatedService {
    async fn exchange(
        &self,
        _auth: AuthContext,
        request: SyncRequest,
    ) -> Result<SyncResponse, ServerError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(empty_response(&request))
    }
}

fn authenticated_exchange() -> (AuthContext, Request<Body>) {
    let tenant = TenantId::new();
    let actor = ActorId::new();
    let device = DeviceId::new();
    let auth = AuthContext {
        actor_id: actor,
        tenant_id: tenant,
        device_id: device,
    };
    (auth, exchange_for(auth))
}

fn exchange_for(auth: AuthContext) -> Request<Body> {
    let scope = SyncScopeId::new();
    let request = SyncRequest {
        protocol: ProtocolVersion::V1,
        request_id: RequestId::new(),
        session: SessionMetadata {
            session_id: SessionId::new(),
            device_id: auth.device_id,
            actor_id: auth.actor_id,
            tenant_id: auth.tenant_id,
            scope_id: scope,
            partitions: Vec::new(),
        },
        cursor: None,
        operations: Vec::new(),
        limits: ClientLimits::default(),
        capabilities: Vec::new(),
    };
    let frame = aequora_codec::encode(ProtocolVersion::V1, MessageKind::SyncRequest, &request)
        .unwrap_or_else(|error| panic!("{error}"));
    Request::post("/sync/v1/exchange")
        .header(CONTENT_TYPE, POSTCARD_CONTENT_TYPE)
        .body(Body::from(frame))
        .unwrap_or_else(|error| panic!("{error}"))
}

fn empty_response(request: &SyncRequest) -> SyncResponse {
    SyncResponse {
        protocol: request.protocol,
        directive: SyncDirective::Continue,
        acknowledged: Vec::new(),
        rejected: Vec::new(),
        conflicts: Vec::new(),
        changes: Vec::new(),
        next_cursor: Cursor {
            scope: request.session.scope_id,
            sequence: Sequence(0),
        },
        has_more: false,
        server_time: HybridTimestamp {
            physical_ms: 1,
            logical: 0,
            node: NodeId::new(),
        },
    }
}

#[tokio::test]
async fn saturated_admission_rejects_without_waiting_and_releases_permits() {
    let entered = Arc::new(Notify::new());
    let service = Arc::new(SlowService {
        entered: entered.clone(),
        delay: Duration::from_secs(30),
    });
    let metrics = Arc::new(AtomicMetrics::default());
    let config = AxumConfig {
        max_in_flight_requests: 1,
        request_timeout: Duration::from_secs(60),
        retry_after_seconds: 2,
        ..AxumConfig::new(1_024 * 1_024)
    };
    let (auth, first_request) = authenticated_exchange();
    let app = router_with_readiness(
        service,
        config,
        metrics.clone(),
        Arc::new(StaticReadiness(true)),
    )
    .layer(Extension(auth));
    let first_app = app.clone();
    let first = tokio::spawn(async move { first_app.oneshot(first_request).await });
    entered.notified().await;

    let (_, second_request) = authenticated_exchange();
    let response = app
        .clone()
        .oneshot(second_request)
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("2")
    );
    assert_eq!(metrics.snapshot().overloaded_requests, 1);

    first.abort();
    let _ = first.await;
    let (_, third_request) = authenticated_exchange();
    let third = tokio::spawn(async move { app.oneshot(third_request).await });
    entered.notified().await;
    third.abort();
    let _ = third.await;
}

#[tokio::test]
async fn tenant_admission_prevents_a_noisy_neighbor_from_starving_other_tenants() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let metrics = Arc::new(AtomicMetrics::default());
    let config = AxumConfig {
        max_in_flight_requests: 2,
        max_in_flight_per_tenant: 1,
        retry_after_seconds: 3,
        ..AxumConfig::new(1_024 * 1_024)
    };
    let (router, lifecycle) = router_with_lifecycle(
        Arc::new(GatedService {
            entered: entered.clone(),
            release: release.clone(),
        }),
        config,
        metrics.clone(),
        Arc::new(StaticReadiness(true)),
    );
    let (tenant_a, first_a_request) = authenticated_exchange();
    let (tenant_b, tenant_b_request) = authenticated_exchange();
    let tenant_b_id = tenant_b.tenant_id;
    let app_a = router.clone().layer(Extension(tenant_a));
    let app_b = router.layer(Extension(tenant_b));

    let first_a_app = app_a.clone();
    let first_a = tokio::spawn(async move { first_a_app.oneshot(first_a_request).await });
    entered.notified().await;

    let rejected_a = app_a
        .oneshot(exchange_for(tenant_a))
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(rejected_a.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        rejected_a
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("3")
    );

    let tenant_b_task = tokio::spawn(async move { app_b.oneshot(tenant_b_request).await });
    entered.notified().await;
    assert_eq!(lifecycle.maximum_in_flight(), 2);
    assert_eq!(lifecycle.maximum_in_flight_per_tenant(), 1);
    assert_eq!(lifecycle.in_flight(), 2);
    assert_eq!(lifecycle.tenant_in_flight(tenant_a.tenant_id), 1);
    assert_eq!(lifecycle.tenant_in_flight(tenant_b_id), 1);
    assert_eq!(lifecycle.active_tenants(), 2);
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.tenant_overloaded_requests, 1);
    assert_eq!(snapshot.overloaded_requests, 0);

    release.notify_one();
    release.notify_one();
    let first_a_response = first_a
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|error| match error {});
    let tenant_b_response = tenant_b_task
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|error| match error {});
    assert_eq!(first_a_response.status(), StatusCode::OK);
    assert_eq!(tenant_b_response.status(), StatusCode::OK);
    assert_eq!(lifecycle.in_flight(), 0);
    assert_eq!(lifecycle.tenant_in_flight(tenant_a.tenant_id), 0);
    assert_eq!(lifecycle.tenant_in_flight(tenant_b_id), 0);
    assert_eq!(lifecycle.active_tenants(), 0);
}

#[tokio::test]
async fn tenant_rate_limit_is_pre_body_isolated_and_memory_bounded() {
    let metrics = Arc::new(AtomicMetrics::default());
    let config = AxumConfig {
        max_in_flight_requests: 2,
        max_in_flight_per_tenant: 2,
        tenant_requests_per_second: 1,
        tenant_request_burst: 1,
        max_rate_limit_tenants: 2,
        retry_after_seconds: 4,
        ..AxumConfig::new(1_024 * 1_024)
    };
    let (router, lifecycle) = router_with_lifecycle(
        Arc::new(EchoService),
        config,
        metrics.clone(),
        Arc::new(StaticReadiness(true)),
    );
    let (limited_auth, initial_request) = authenticated_exchange();
    let (isolated_auth, isolated_request) = authenticated_exchange();
    let (replacement_auth, replacement_request) = authenticated_exchange();
    let limited_app = router.clone().layer(Extension(limited_auth));
    let initial_response = limited_app
        .clone()
        .oneshot(initial_request)
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(initial_response.status(), StatusCode::OK);

    let rejected = limited_app
        .oneshot(
            Request::post("/sync/v1/exchange")
                .header(CONTENT_TYPE, POSTCARD_CONTENT_TYPE)
                .body(Body::from("not a postcard frame"))
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(rejected.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        rejected
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("4")
    );

    let isolated_response = router
        .clone()
        .layer(Extension(isolated_auth))
        .oneshot(isolated_request)
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(isolated_response.status(), StatusCode::OK);
    assert_eq!(lifecycle.tracked_rate_limit_tenants(), 2);

    let replacement_response = router
        .layer(Extension(replacement_auth))
        .oneshot(replacement_request)
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(replacement_response.status(), StatusCode::OK);
    assert_eq!(lifecycle.tracked_rate_limit_tenants(), 2);
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.tenant_rate_limited_requests, 1);
    assert_eq!(snapshot.tenant_overloaded_requests, 0);
    assert_eq!(snapshot.overloaded_requests, 0);
}

#[tokio::test]
async fn slow_body_times_out_releases_admission_and_does_not_block_drain() {
    let calls = Arc::new(AtomicUsize::new(0));
    let metrics = Arc::new(AtomicMetrics::default());
    let config = AxumConfig {
        body_read_timeout: Duration::from_millis(1),
        retry_after_seconds: 6,
        ..AxumConfig::new(1_024 * 1_024)
    };
    let (router, lifecycle) = router_with_lifecycle(
        Arc::new(CountingService {
            calls: calls.clone(),
        }),
        config,
        metrics.clone(),
        Arc::new(StaticReadiness(true)),
    );
    let (auth, valid_request) = authenticated_exchange();
    let app = router.layer(Extension(auth));
    let never_ending_body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
    let timed_out = app
        .clone()
        .oneshot(
            Request::post("/sync/v1/exchange")
                .header(CONTENT_TYPE, POSTCARD_CONTENT_TYPE)
                .body(never_ending_body)
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(timed_out.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(
        timed_out
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("6")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(lifecycle.in_flight(), 0);

    let valid = app
        .oneshot(valid_request)
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(valid.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        lifecycle.drain(Duration::from_millis(10)).await,
        DrainOutcome::Drained
    );
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.body_read_timeouts, 1);
    assert_eq!(snapshot.oversized_request_bodies, 0);
}

#[tokio::test]
async fn oversized_streamed_body_is_rejected_before_service_execution() {
    let calls = Arc::new(AtomicUsize::new(0));
    let metrics = Arc::new(AtomicMetrics::default());
    let config = AxumConfig::new(32);
    let (router, lifecycle) = router_with_lifecycle(
        Arc::new(CountingService {
            calls: calls.clone(),
        }),
        config,
        metrics.clone(),
        Arc::new(StaticReadiness(true)),
    );
    let (auth, _) = authenticated_exchange();
    let body = Body::from_stream(stream::iter([
        Ok::<_, Infallible>(Bytes::from_static(&[0; 24])),
        Ok::<_, Infallible>(Bytes::from_static(&[0; 24])),
    ]));
    let response = router
        .layer(Extension(auth))
        .oneshot(
            Request::post("/sync/v1/exchange")
                .header(CONTENT_TYPE, POSTCARD_CONTENT_TYPE)
                .body(body)
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(lifecycle.in_flight(), 0);
    assert_eq!(metrics.snapshot().oversized_request_bodies, 1);
}

#[tokio::test]
async fn execution_deadline_is_transient_and_observable() {
    let service = Arc::new(SlowService {
        entered: Arc::new(Notify::new()),
        delay: Duration::from_millis(100),
    });
    let metrics = Arc::new(AtomicMetrics::default());
    let config = AxumConfig {
        request_timeout: Duration::from_millis(1),
        retry_after_seconds: 4,
        ..AxumConfig::new(1_024 * 1_024)
    };
    let (auth, request) = authenticated_exchange();
    let app = router_with_readiness(
        service,
        config,
        metrics.clone(),
        Arc::new(StaticReadiness(true)),
    )
    .layer(Extension(auth));
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(
        response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("4")
    );
    assert_eq!(metrics.snapshot().timed_out_requests, 1);

    let (_, retry_request) = authenticated_exchange();
    let retry_response = app
        .oneshot(retry_request)
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(retry_response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(metrics.snapshot().timed_out_requests, 2);
}

#[tokio::test]
async fn liveness_is_independent_from_dependency_readiness() {
    let metrics = Arc::new(AtomicMetrics::default());
    let app = router_with_readiness(
        Arc::new(EchoService),
        AxumConfig::new(1_024 * 1_024),
        metrics.clone(),
        Arc::new(StaticReadiness(false)),
    );
    let live = app
        .clone()
        .oneshot(
            Request::get("/sync/v1/health/live")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| match error {});
    let legacy_live = app
        .clone()
        .oneshot(
            Request::get("/sync/v1/health")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| match error {});
    let ready = app
        .oneshot(
            Request::get("/sync/v1/health/ready")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(live.status(), StatusCode::NO_CONTENT);
    assert_eq!(legacy_live.status(), StatusCode::NO_CONTENT);
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.readiness_checks, 1);
    assert_eq!(snapshot.readiness_failures, 1);
}

#[tokio::test]
async fn readiness_probe_is_bounded_by_its_own_deadline() {
    let metrics = Arc::new(AtomicMetrics::default());
    let config = AxumConfig {
        readiness_timeout: Duration::from_millis(1),
        ..AxumConfig::new(1_024 * 1_024)
    };
    let app = router_with_readiness(
        Arc::new(EchoService),
        config,
        metrics.clone(),
        Arc::new(SlowReadiness(Duration::from_secs(30))),
    );
    let response = app
        .oneshot(
            Request::get("/sync/v1/health/ready")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(metrics.snapshot().readiness_failures, 1);
}

#[tokio::test]
async fn graceful_drain_rejects_new_work_and_waits_for_admitted_work() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let readiness_calls = Arc::new(AtomicUsize::new(0));
    let metrics = Arc::new(AtomicMetrics::default());
    let config = AxumConfig {
        max_in_flight_requests: 1,
        retry_after_seconds: 5,
        ..AxumConfig::new(1_024 * 1_024)
    };
    let (router, lifecycle) = router_with_lifecycle(
        Arc::new(GatedService {
            entered: entered.clone(),
            release: release.clone(),
        }),
        config,
        metrics.clone(),
        Arc::new(CountingReadiness {
            calls: readiness_calls.clone(),
        }),
    );
    let (auth, admitted_request) = authenticated_exchange();
    let app = router.layer(Extension(auth));
    let admitted_app = app.clone();
    let admitted = tokio::spawn(async move { admitted_app.oneshot(admitted_request).await });
    entered.notified().await;
    assert_eq!(lifecycle.begin_draining(), 1);

    let ready = app
        .clone()
        .oneshot(
            Request::get("/sync/v1/health/ready")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(readiness_calls.load(Ordering::SeqCst), 0);

    let (_, rejected_request) = authenticated_exchange();
    let rejected = app
        .oneshot(rejected_request)
        .await
        .unwrap_or_else(|error| match error {});
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        rejected
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("5")
    );

    let drain_lifecycle = lifecycle.clone();
    let drain = tokio::spawn(async move { drain_lifecycle.drain(Duration::from_secs(1)).await });
    release.notify_one();
    let admitted_response = admitted
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|error| match error {});
    assert_eq!(admitted_response.status(), StatusCode::OK);
    assert_eq!(
        drain.await.unwrap_or_else(|error| panic!("{error}")),
        DrainOutcome::Drained
    );
    assert_eq!(lifecycle.in_flight(), 0);
    let snapshot = metrics.snapshot();
    assert!(snapshot.server_draining);
    assert_eq!(snapshot.server_in_flight, 0);
    assert_eq!(snapshot.draining_rejections, 1);
    assert_eq!(snapshot.drains_completed, 1);
}

#[tokio::test]
async fn graceful_drain_deadline_reports_exact_remaining_work() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let metrics = Arc::new(AtomicMetrics::default());
    let (router, lifecycle) = router_with_lifecycle(
        Arc::new(GatedService {
            entered: entered.clone(),
            release: release.clone(),
        }),
        AxumConfig::new(1_024 * 1_024),
        metrics.clone(),
        Arc::new(StaticReadiness(true)),
    );
    let (auth, request) = authenticated_exchange();
    let request_task =
        tokio::spawn(async move { router.layer(Extension(auth)).oneshot(request).await });
    entered.notified().await;
    assert_eq!(
        lifecycle.drain(Duration::from_millis(1)).await,
        DrainOutcome::TimedOut { remaining: 1 }
    );
    release.notify_one();
    let response = request_task
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|error| match error {});
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        lifecycle.drain(Duration::from_secs(1)).await,
        DrainOutcome::Drained
    );
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.drains_timed_out, 1);
    assert_eq!(snapshot.drains_completed, 1);
    assert_eq!(snapshot.drain_remaining, 0);
}

#[tokio::test]
async fn readiness_cannot_race_a_drain_and_return_healthy() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (router, lifecycle) = router_with_lifecycle(
        Arc::new(EchoService),
        AxumConfig::new(1_024 * 1_024),
        Arc::new(AtomicMetrics::default()),
        Arc::new(GatedReadiness {
            entered: entered.clone(),
            release: release.clone(),
        }),
    );
    let readiness = tokio::spawn(async move {
        router
            .oneshot(
                Request::get("/sync/v1/health/ready")
                    .body(Body::empty())
                    .unwrap_or_else(|error| panic!("{error}")),
            )
            .await
    });
    entered.notified().await;
    assert_eq!(lifecycle.begin_draining(), 0);
    release.notify_one();
    let response = readiness
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .unwrap_or_else(|error| match error {});
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
