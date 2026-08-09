use aequora_axum::{AxumConfig, POSTCARD_CONTENT_TYPE};
use aequora_codec::{FLAG_ZSTD, MessageKind};
use aequora_executor::AuthContext;
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
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::CONTENT_TYPE},
};
use std::sync::Arc;
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
