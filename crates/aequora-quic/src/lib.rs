//! Optional Quinn transport using one framed request per bidirectional QUIC stream.
//!
//! TLS endpoint construction and certificate policy remain application-owned. A server must
//! authenticate the QUIC connection before supplying its [`AuthContext`] to [`QuicServer`].

use aequora_codec::{
    Compression, DecodeLimits, EncodeOptions, MessageKind, decode_frame, decode_with_limits,
    encode_with_options,
};
use aequora_executor::AuthContext;
use aequora_protocol::{
    BootstrapRequest, BootstrapResponse, Capability, PushHint, SyncRequest, SyncResponse,
};
use aequora_server::{ExchangeService, ServerError};
use aequora_store::StoreErrorKind;
use aequora_transport::{
    SnapshotPageStream, StreamingSyncTransport, SyncTransport, TransportError,
};
use aequora_types::ProtocolVersion;
use async_trait::async_trait;
use quinn::{Connection, ConnectionError, RecvStream, SendStream};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::sync::Arc;
use thiserror::Error;

/// Wire-size, decompression, and response-compression controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuicConfig {
    /// Maximum bytes accepted on one request stream before decompression.
    pub max_request_bytes: usize,
    /// Maximum request payload bytes after decompression.
    pub max_decompressed_request_bytes: usize,
    /// Maximum bytes accepted for one response or streamed page.
    pub max_response_bytes: usize,
    /// Maximum response payload bytes after decompression.
    pub max_decompressed_response_bytes: usize,
    /// Minimum serialized response bytes considered for zstd.
    pub compression_threshold: usize,
    /// zstd level selected after capability negotiation.
    pub zstd_level: i32,
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self {
            max_request_bytes: 4 * 1_024 * 1_024,
            max_decompressed_request_bytes: 4 * 1_024 * 1_024,
            max_response_bytes: 4 * 1_024 * 1_024,
            max_decompressed_response_bytes: 4 * 1_024 * 1_024,
            compression_threshold: 4_096,
            zstd_level: 3,
        }
    }
}

/// Failure in the server-side QUIC connection loop.
#[derive(Debug, Error)]
#[error("QUIC synchronization stream failed: {message}")]
pub struct QuicServerError {
    message: String,
}

impl QuicServerError {
    fn new(error: impl std::fmt::Display) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WireError {
    transient: bool,
    message: String,
}

/// Client transport over an already authenticated Quinn connection.
#[derive(Clone)]
pub struct QuicTransport {
    connection: Connection,
    config: QuicConfig,
}

impl QuicTransport {
    /// Wraps an application-configured TLS-authenticated connection.
    #[must_use]
    pub const fn new(connection: Connection, config: QuicConfig) -> Self {
        Self { connection, config }
    }

    /// Borrows the underlying connection for lifecycle and migration management.
    #[must_use]
    pub const fn connection(&self) -> &Connection {
        &self.connection
    }

    async fn request_response<Request, Response>(
        &self,
        request_kind: MessageKind,
        response_kind: MessageKind,
        request: &Request,
    ) -> Result<Response, TransportError>
    where
        Request: Serialize + Sync,
        Response: DeserializeOwned,
    {
        let frame = encode_with_options(
            ProtocolVersion::V1,
            request_kind,
            request,
            EncodeOptions::default(),
        )
        .map_err(permanent)?;
        let (mut send, mut recv) = self.connection.open_bi().await.map_err(transient)?;
        send.write_all(&frame).await.map_err(transient)?;
        send.finish().map_err(transient)?;
        let response = recv
            .read_to_end(self.config.max_response_bytes)
            .await
            .map_err(transient)?;
        decode_reply(&response, response_kind, self.config)
    }
}

#[async_trait]
impl SyncTransport for QuicTransport {
    async fn exchange(&self, request: SyncRequest) -> Result<SyncResponse, TransportError> {
        self.request_response(
            MessageKind::SyncRequest,
            MessageKind::SyncResponse,
            &request,
        )
        .await
    }

    async fn bootstrap(
        &self,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, TransportError> {
        self.request_response(
            MessageKind::BootstrapRequest,
            MessageKind::BootstrapResponse,
            &request,
        )
        .await
    }

    async fn next_push_hint(&self) -> Result<PushHint, TransportError> {
        let mut recv = self.connection.accept_uni().await.map_err(transient)?;
        let frame = recv
            .read_to_end(self.config.max_response_bytes)
            .await
            .map_err(transient)?;
        decode_reply(&frame, MessageKind::PushHint, self.config)
    }
}

struct QuicSnapshotStream {
    recv: RecvStream,
    config: QuicConfig,
    finished: bool,
}

#[async_trait]
impl SnapshotPageStream for QuicSnapshotStream {
    async fn next_page(&mut self) -> Result<Option<BootstrapResponse>, TransportError> {
        if self.finished {
            return Ok(None);
        }
        let frame = read_length_delimited(&mut self.recv, self.config.max_response_bytes).await?;
        let response: BootstrapResponse =
            decode_reply(&frame, MessageKind::SnapshotStreamResponse, self.config)?;
        self.finished = !response.has_more;
        Ok(Some(response))
    }
}

#[async_trait]
impl StreamingSyncTransport for QuicTransport {
    async fn bootstrap_stream(
        &self,
        request: BootstrapRequest,
    ) -> Result<Box<dyn SnapshotPageStream>, TransportError> {
        let frame = encode_with_options(
            request.protocol,
            MessageKind::SnapshotStreamRequest,
            &request,
            EncodeOptions::default(),
        )
        .map_err(permanent)?;
        let (mut send, recv) = self.connection.open_bi().await.map_err(transient)?;
        send.write_all(&frame).await.map_err(transient)?;
        send.finish().map_err(transient)?;
        Ok(Box::new(QuicSnapshotStream {
            recv,
            config: self.config,
            finished: false,
        }))
    }
}

/// Server adapter over an application-owned Quinn endpoint.
#[derive(Clone)]
pub struct QuicServer {
    service: Arc<dyn ExchangeService>,
    config: QuicConfig,
}

impl QuicServer {
    /// Creates a server-side connection handler.
    #[must_use]
    pub const fn new(service: Arc<dyn ExchangeService>, config: QuicConfig) -> Self {
        Self { service, config }
    }

    /// Accepts multiplexed request streams until the connection closes.
    /// `auth` must come from the host application's verified TLS/session identity.
    ///
    /// # Errors
    ///
    /// Returns [`QuicServerError`] when accepting a new stream fails.
    pub async fn serve_connection(
        &self,
        connection: Connection,
        auth: AuthContext,
    ) -> Result<(), QuicServerError> {
        loop {
            let (send, recv) = match connection.accept_bi().await {
                Ok(streams) => streams,
                Err(ConnectionError::ApplicationClosed(_) | ConnectionError::LocallyClosed) => {
                    return Ok(());
                }
                Err(error) => return Err(QuicServerError::new(error)),
            };
            let server = self.clone();
            drop(tokio::spawn(async move {
                let _result = server.handle_stream(send, recv, auth).await;
            }));
        }
    }

    /// Sends one advisory hint on a unidirectional stream.
    ///
    /// # Errors
    ///
    /// Returns [`QuicServerError`] for framing or stream failures.
    pub async fn send_push_hint(
        &self,
        connection: &Connection,
        hint: &PushHint,
    ) -> Result<(), QuicServerError> {
        let frame = encode_with_options(
            hint.protocol,
            MessageKind::PushHint,
            hint,
            EncodeOptions::default(),
        )
        .map_err(QuicServerError::new)?;
        let mut send = connection.open_uni().await.map_err(QuicServerError::new)?;
        send.write_all(&frame).await.map_err(QuicServerError::new)?;
        send.finish().map_err(QuicServerError::new)
    }

    async fn handle_stream(
        &self,
        mut send: SendStream,
        mut recv: RecvStream,
        auth: AuthContext,
    ) -> Result<(), QuicServerError> {
        let frame = recv
            .read_to_end(self.config.max_request_bytes)
            .await
            .map_err(QuicServerError::new)?;
        let decoded =
            decode_frame(&frame, self.config.max_request_bytes).map_err(QuicServerError::new)?;
        match decoded.kind {
            MessageKind::SyncRequest => {
                let (_, request) =
                    decode_request::<SyncRequest>(&frame, MessageKind::SyncRequest, self.config)?;
                let capabilities = request.capabilities.clone();
                let reply = match self.service.exchange(auth, request).await {
                    Ok(response) => encode_with_options(
                        response.protocol,
                        MessageKind::SyncResponse,
                        &response,
                        compression_options(&capabilities, self.config),
                    ),
                    Err(error) => encode_server_error(&error),
                }
                .map_err(QuicServerError::new)?;
                send.write_all(&reply).await.map_err(QuicServerError::new)?;
                send.finish().map_err(QuicServerError::new)
            }
            MessageKind::BootstrapRequest => {
                let (_, request) = decode_request::<BootstrapRequest>(
                    &frame,
                    MessageKind::BootstrapRequest,
                    self.config,
                )?;
                let capabilities = request.capabilities.clone();
                let reply = match self.service.bootstrap(auth, request).await {
                    Ok(response) => encode_with_options(
                        response.protocol,
                        MessageKind::BootstrapResponse,
                        &response,
                        compression_options(&capabilities, self.config),
                    ),
                    Err(error) => encode_server_error(&error),
                }
                .map_err(QuicServerError::new)?;
                send.write_all(&reply).await.map_err(QuicServerError::new)?;
                send.finish().map_err(QuicServerError::new)
            }
            MessageKind::SnapshotStreamRequest => {
                let (_, request) = decode_request::<BootstrapRequest>(
                    &frame,
                    MessageKind::SnapshotStreamRequest,
                    self.config,
                )?;
                self.write_snapshot_stream(send, auth, request).await
            }
            _ => {
                let reply = encode_wire_error(false, "unsupported QUIC request message")
                    .map_err(QuicServerError::new)?;
                send.write_all(&reply).await.map_err(QuicServerError::new)?;
                send.finish().map_err(QuicServerError::new)
            }
        }
    }

    async fn write_snapshot_stream(
        &self,
        mut send: SendStream,
        auth: AuthContext,
        mut request: BootstrapRequest,
    ) -> Result<(), QuicServerError> {
        loop {
            let response = match self.service.bootstrap(auth, request.clone()).await {
                Ok(response) => response,
                Err(error) => {
                    let frame = encode_server_error(&error).map_err(QuicServerError::new)?;
                    write_length_delimited(&mut send, &frame).await?;
                    return send.finish().map_err(QuicServerError::new);
                }
            };
            let frame = encode_with_options(
                response.protocol,
                MessageKind::SnapshotStreamResponse,
                &response,
                compression_options(&request.capabilities, self.config),
            )
            .map_err(QuicServerError::new)?;
            write_length_delimited(&mut send, &frame).await?;
            if !response.has_more {
                return send.finish().map_err(QuicServerError::new);
            }
            request.snapshot_id = Some(response.snapshot_id);
            request.offset = response.next_offset;
        }
    }
}

fn decode_request<T: DeserializeOwned>(
    frame: &[u8],
    kind: MessageKind,
    config: QuicConfig,
) -> Result<(ProtocolVersion, T), QuicServerError> {
    decode_with_limits(
        frame,
        kind,
        DecodeLimits {
            max_wire_bytes: config.max_request_bytes,
            max_decompressed_bytes: config.max_decompressed_request_bytes,
        },
    )
    .map_err(QuicServerError::new)
}

fn decode_reply<T: DeserializeOwned>(
    frame: &[u8],
    expected: MessageKind,
    config: QuicConfig,
) -> Result<T, TransportError> {
    let decoded = decode_frame(frame, config.max_response_bytes).map_err(permanent)?;
    if decoded.kind == MessageKind::TransportError {
        let (_, error): (_, WireError) = decode_with_limits(
            frame,
            MessageKind::TransportError,
            DecodeLimits {
                max_wire_bytes: config.max_response_bytes,
                max_decompressed_bytes: config.max_decompressed_response_bytes,
            },
        )
        .map_err(permanent)?;
        return Err(if error.transient {
            TransportError::transient(error.message)
        } else {
            TransportError::permanent(error.message)
        });
    }
    let (_, value) = decode_with_limits(
        frame,
        expected,
        DecodeLimits {
            max_wire_bytes: config.max_response_bytes,
            max_decompressed_bytes: config.max_decompressed_response_bytes,
        },
    )
    .map_err(permanent)?;
    Ok(value)
}

fn compression_options(capabilities: &[Capability], config: QuicConfig) -> EncodeOptions {
    EncodeOptions {
        compression: if capabilities.contains(&Capability::Zstd) {
            Compression::Zstd {
                level: config.zstd_level,
            }
        } else {
            Compression::None
        },
        compression_threshold: config.compression_threshold,
    }
}

fn encode_server_error(error: &ServerError) -> Result<Vec<u8>, aequora_codec::CodecError> {
    let transient = matches!(
        error,
        ServerError::Store(store) if store.kind == StoreErrorKind::Transient
    );
    encode_wire_error(transient, &error.to_string())
}

fn encode_wire_error(transient: bool, message: &str) -> Result<Vec<u8>, aequora_codec::CodecError> {
    encode_with_options(
        ProtocolVersion::V1,
        MessageKind::TransportError,
        &WireError {
            transient,
            message: message.to_owned(),
        },
        EncodeOptions::default(),
    )
}

async fn write_length_delimited(
    send: &mut SendStream,
    frame: &[u8],
) -> Result<(), QuicServerError> {
    let length = u32::try_from(frame.len()).map_err(QuicServerError::new)?;
    send.write_all(&length.to_be_bytes())
        .await
        .map_err(QuicServerError::new)?;
    send.write_all(frame).await.map_err(QuicServerError::new)
}

async fn read_length_delimited(
    recv: &mut RecvStream,
    maximum: usize,
) -> Result<Vec<u8>, TransportError> {
    let mut length = [0_u8; 4];
    recv.read_exact(&mut length).await.map_err(transient)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > maximum {
        return Err(TransportError::permanent(format!(
            "QUIC snapshot frame length {length} exceeds {maximum} bytes"
        )));
    }
    let mut frame = vec![0; length];
    recv.read_exact(&mut frame).await.map_err(transient)?;
    Ok(frame)
}

fn permanent(error: impl std::fmt::Display) -> TransportError {
    TransportError::permanent(error.to_string())
}

fn transient(error: impl std::fmt::Display) -> TransportError {
    TransportError::transient(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_protocol::{
        ClientLimits, PushHintReason, SessionMetadata, SnapshotEntity, SnapshotLimits,
        SyncDirective,
    };
    use aequora_transport::TransportErrorKind;
    use aequora_types::{
        ActorId, Cursor, DeviceId, EntityId, EntityRef, EntityType, EntityVersion, HybridTimestamp,
        NodeId, RequestId, Sequence, SessionId, SyncScopeId, TenantId,
    };
    use quinn::{ClientConfig, Endpoint, ServerConfig};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    struct EchoService;

    #[async_trait]
    impl ExchangeService for EchoService {
        async fn exchange(
            &self,
            _auth: AuthContext,
            request: SyncRequest,
        ) -> Result<SyncResponse, ServerError> {
            Ok(SyncResponse {
                protocol: ProtocolVersion::V1,
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
            })
        }

        async fn bootstrap(
            &self,
            _auth: AuthContext,
            request: BootstrapRequest,
        ) -> Result<BootstrapResponse, ServerError> {
            let snapshot_id = request.snapshot_id.unwrap_or_default();
            Ok(BootstrapResponse {
                protocol: ProtocolVersion::V1,
                snapshot_id,
                cursor: Cursor {
                    scope: request.session.scope_id,
                    sequence: Sequence(0),
                },
                offset: request.offset,
                entities: vec![SnapshotEntity {
                    entity: EntityRef {
                        entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
                        entity_id: EntityId::new(),
                    },
                    version: EntityVersion::INITIAL,
                    payload: vec![42; 8 * 1_024],
                    tombstone: false,
                }],
                next_offset: request.offset.saturating_add(1),
                has_more: request.offset == 0,
                server_time: HybridTimestamp {
                    physical_ms: 1,
                    logical: 0,
                    node: NodeId::new(),
                },
            })
        }
    }

    #[test]
    fn typed_wire_errors_preserve_retry_semantics() {
        let frame = encode_wire_error(true, "temporarily unavailable")
            .unwrap_or_else(|error| panic!("{error}"));
        let result =
            decode_reply::<SyncResponse>(&frame, MessageKind::SyncResponse, QuicConfig::default());
        assert!(matches!(
            result,
            Err(TransportError {
                kind: TransportErrorKind::Transient,
                ..
            })
        ));
    }

    async fn loopback_connections() -> (Endpoint, Endpoint, Connection, Connection) {
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
            .unwrap_or_else(|error| panic!("{error}"));
        let certificate = certified.cert.der().clone();
        let key =
            quinn::rustls::pki_types::PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der());
        let server_config = ServerConfig::with_single_cert(vec![certificate.clone()], key.into())
            .unwrap_or_else(|error| panic!("{error}"));
        let server_endpoint = Endpoint::server(
            server_config,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let server_address = server_endpoint
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"));
        let mut roots = quinn::rustls::RootCertStore::empty();
        roots
            .add(certificate)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut client_endpoint =
            Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .unwrap_or_else(|error| panic!("{error}"));
        client_endpoint.set_default_client_config(
            ClientConfig::with_root_certificates(Arc::new(roots))
                .unwrap_or_else(|error| panic!("{error}")),
        );
        let client_connecting = client_endpoint
            .connect(server_address, "localhost")
            .unwrap_or_else(|error| panic!("{error}"));
        let incoming = server_endpoint
            .accept()
            .await
            .unwrap_or_else(|| panic!("server endpoint closed"));
        let (client_connection, server_connection) = tokio::join!(client_connecting, incoming);
        (
            server_endpoint,
            client_endpoint,
            client_connection.unwrap_or_else(|error| panic!("client connect: {error}")),
            server_connection.unwrap_or_else(|error| panic!("server connect: {error}")),
        )
    }

    #[tokio::test]
    async fn loopback_quic_exchanges_and_delivers_a_bounded_hint() {
        let (server_endpoint, client_endpoint, client_connection, server_connection) =
            loopback_connections().await;

        let tenant = TenantId::new();
        let actor = ActorId::new();
        let device = DeviceId::new();
        let scope = SyncScopeId::new();
        let auth = AuthContext {
            actor_id: actor,
            tenant_id: tenant,
            device_id: device,
        };
        let server = QuicServer::new(Arc::new(EchoService), QuicConfig::default());
        let server_task = tokio::spawn({
            let server = server.clone();
            let connection = server_connection.clone();
            async move { server.serve_connection(connection, auth).await }
        });
        let transport = QuicTransport::new(client_connection.clone(), QuicConfig::default());
        let session = SessionMetadata {
            session_id: SessionId::new(),
            device_id: device,
            actor_id: actor,
            tenant_id: tenant,
            scope_id: scope,
            partitions: Vec::new(),
        };
        let response = transport
            .exchange(SyncRequest {
                protocol: ProtocolVersion::V1,
                request_id: RequestId::new(),
                session: session.clone(),
                cursor: None,
                operations: Vec::new(),
                limits: ClientLimits::default(),
                capabilities: vec![Capability::Quic],
            })
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(response.next_cursor.scope, scope);

        let mut snapshot = transport
            .bootstrap_stream(BootstrapRequest {
                protocol: ProtocolVersion::V1,
                request_id: RequestId::new(),
                session,
                snapshot_id: None,
                offset: 0,
                limits: SnapshotLimits {
                    max_entities: 1,
                    max_payload_bytes: 16 * 1_024,
                },
                capabilities: vec![Capability::Quic, Capability::StreamingSnapshots],
            })
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let first = snapshot
            .next_page()
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("missing first snapshot page"));
        let second = snapshot
            .next_page()
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|| panic!("missing second snapshot page"));
        assert!(first.has_more);
        assert!(!second.has_more);
        assert_eq!(first.snapshot_id, second.snapshot_id);
        assert!(
            snapshot
                .next_page()
                .await
                .unwrap_or_else(|error| panic!("{error}"))
                .is_none()
        );

        let hint = PushHint {
            protocol: ProtocolVersion::V1,
            tenant_id: tenant,
            scope_id: scope,
            sequence: Sequence(9),
            reason: PushHintReason::JournalAdvanced,
            region_id: None,
        };
        server
            .send_push_hint(&server_connection, &hint)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            transport
                .next_push_hint()
                .await
                .unwrap_or_else(|error| panic!("{error}")),
            hint
        );

        client_connection.close(0_u32.into(), b"test complete");
        server_task
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .unwrap_or_else(|error| panic!("{error}"));
        server_endpoint.close(0_u32.into(), b"test complete");
        client_endpoint.close(0_u32.into(), b"test complete");
    }
}
