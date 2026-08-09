//! Bounded Postcard-over-HTTP client transport for the Axum integration.

use aequora_codec::{DecodeLimits, EncodeOptions, MessageKind};
use aequora_observability::{MetricEvent, NoopObserver, Observer};
use aequora_protocol::{BootstrapRequest, BootstrapResponse, SyncRequest, SyncResponse};
use aequora_transport::{SyncTransport, TransportError};
use async_trait::async_trait;
use http::{HeaderMap, HeaderValue, header::ACCEPT, header::CONTENT_TYPE};
use reqwest::{Client, Response, StatusCode, Url};
use std::sync::Arc;
use thiserror::Error;

/// Primary synchronization media type.
pub const POSTCARD_CONTENT_TYPE: &str = "application/vnd.aequora.postcard";

/// Per-request headers supplied by the host, typically authorization and trace context.
/// Implementations may refresh credentials on every call.
pub trait RequestHeaders: Send + Sync {
    /// Returns headers for the next request.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] when credentials cannot be loaded or refreshed.
    fn headers(&self) -> Result<HeaderMap, TransportError>;
}

/// Empty headers for deployments whose client authentication is handled elsewhere.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoRequestHeaders;

impl RequestHeaders for NoRequestHeaders {
    fn headers(&self) -> Result<HeaderMap, TransportError> {
        Ok(HeaderMap::new())
    }
}

/// Cloneable immutable header set. Its debug representation intentionally omits values.
#[derive(Clone, Default)]
pub struct StaticRequestHeaders(HeaderMap);

impl StaticRequestHeaders {
    /// Wraps an application-constructed header set.
    #[must_use]
    pub const fn new(headers: HeaderMap) -> Self {
        Self(headers)
    }
}

impl std::fmt::Debug for StaticRequestHeaders {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StaticRequestHeaders")
            .field("header_count", &self.0.len())
            .finish()
    }
}

impl RequestHeaders for StaticRequestHeaders {
    fn headers(&self) -> Result<HeaderMap, TransportError> {
        Ok(self.0.clone())
    }
}

/// HTTP response and decompression limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttpTransportConfig {
    /// Maximum response bytes read from the network.
    pub max_response_bytes: usize,
    /// Maximum response payload bytes after zstd decompression.
    pub max_decompressed_response_bytes: usize,
}

impl Default for HttpTransportConfig {
    fn default() -> Self {
        Self {
            max_response_bytes: 4 * 1_024 * 1_024,
            max_decompressed_response_bytes: 4 * 1_024 * 1_024,
        }
    }
}

/// Invalid HTTP transport construction.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum HttpTransportConfigError {
    /// The supplied base URL cannot be joined with fixed synchronization paths.
    #[error("invalid synchronization base URL")]
    BaseUrl,
}

/// Cloneable HTTP transport with bounded response accumulation.
#[derive(Clone)]
pub struct HttpTransport {
    client: Client,
    exchange_url: Url,
    bootstrap_url: Url,
    headers: Arc<dyn RequestHeaders>,
    config: HttpTransportConfig,
    observer: Arc<dyn Observer>,
}

impl HttpTransport {
    /// Creates a transport from an application-configured Reqwest client and base URL.
    ///
    /// # Errors
    ///
    /// Returns [`HttpTransportConfigError`] when fixed endpoint URLs cannot be constructed.
    pub fn new<H>(
        client: Client,
        base_url: &Url,
        headers: H,
        config: HttpTransportConfig,
    ) -> Result<Self, HttpTransportConfigError>
    where
        H: RequestHeaders + 'static,
    {
        let exchange_url = base_url
            .join("/sync/v1/exchange")
            .map_err(|_| HttpTransportConfigError::BaseUrl)?;
        let bootstrap_url = base_url
            .join("/sync/v1/bootstrap")
            .map_err(|_| HttpTransportConfigError::BaseUrl)?;
        Ok(Self {
            client,
            exchange_url,
            bootstrap_url,
            headers: Arc::new(headers),
            config,
            observer: Arc::new(NoopObserver),
        })
    }

    /// Attaches payload-free exact wire-byte instrumentation.
    #[must_use]
    pub fn with_observer(mut self, observer: Arc<dyn Observer>) -> Self {
        self.observer = observer;
        self
    }

    async fn post<Request, Reply>(
        &self,
        url: Url,
        request_kind: MessageKind,
        response_kind: MessageKind,
        protocol: aequora_types::ProtocolVersion,
        request: &Request,
    ) -> Result<Reply, TransportError>
    where
        Request: serde::Serialize + Sync,
        Reply: serde::de::DeserializeOwned,
    {
        let frame = aequora_codec::encode_with_options(
            protocol,
            request_kind,
            request,
            EncodeOptions::default(),
        )
        .map_err(permanent)?;
        let uploaded = usize_to_u64(frame.len());
        let mut headers = self.headers.headers()?;
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static(POSTCARD_CONTENT_TYPE),
        );
        headers.insert(ACCEPT, HeaderValue::from_static(POSTCARD_CONTENT_TYPE));
        let response = self
            .client
            .post(url)
            .headers(headers)
            .body(frame)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        if content_type != Some(POSTCARD_CONTENT_TYPE) {
            return Err(TransportError::permanent(
                "HTTP response has an unsupported content type",
            ));
        }
        let body = read_bounded(response, self.config.max_response_bytes).await?;
        self.observer.record(MetricEvent::TransportBytes {
            uploaded,
            downloaded: usize_to_u64(body.len()),
        });
        let (_, reply) = aequora_codec::decode_with_limits(
            &body,
            response_kind,
            DecodeLimits {
                max_wire_bytes: self.config.max_response_bytes,
                max_decompressed_bytes: self.config.max_decompressed_response_bytes,
            },
        )
        .map_err(permanent)?;
        Ok(reply)
    }
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[async_trait]
impl SyncTransport for HttpTransport {
    async fn exchange(&self, request: SyncRequest) -> Result<SyncResponse, TransportError> {
        self.post(
            self.exchange_url.clone(),
            MessageKind::SyncRequest,
            MessageKind::SyncResponse,
            request.protocol,
            &request,
        )
        .await
    }

    async fn bootstrap(
        &self,
        request: BootstrapRequest,
    ) -> Result<BootstrapResponse, TransportError> {
        self.post(
            self.bootstrap_url.clone(),
            MessageKind::BootstrapRequest,
            MessageKind::BootstrapResponse,
            request.protocol,
            &request,
        )
        .await
    }
}

async fn read_bounded(mut response: Response, maximum: usize) -> Result<Vec<u8>, TransportError> {
    if response
        .content_length()
        .is_some_and(|length| length > u64::try_from(maximum).unwrap_or(u64::MAX))
    {
        return Err(TransportError::permanent(
            "HTTP response exceeds the configured wire limit",
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
        if body.len().saturating_add(chunk.len()) > maximum {
            return Err(TransportError::permanent(
                "HTTP response exceeds the configured wire limit",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn status_error(status: StatusCode) -> TransportError {
    let message = format!("HTTP synchronization endpoint returned status {status}");
    if status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        TransportError::transient(message)
    } else {
        TransportError::permanent(message)
    }
}

fn map_reqwest_error(error: reqwest::Error) -> TransportError {
    if error.is_builder() || error.is_redirect() {
        permanent(error)
    } else {
        TransportError::transient(error.to_string())
    }
}

fn permanent(error: impl std::fmt::Display) -> TransportError {
    TransportError::permanent(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_executor::AuthContext;
    use aequora_protocol::{ClientLimits, SessionMetadata, SyncDirective};
    use aequora_server::{ExchangeService, ServerError};
    use aequora_transport::TransportErrorKind;
    use aequora_types::{
        ActorId, Cursor, DeviceId, HybridTimestamp, NodeId, ProtocolVersion, RequestId, Sequence,
        SessionId, SyncScopeId, TenantId,
    };
    use axum::Extension;

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
    }

    #[tokio::test]
    async fn reqwest_transport_round_trips_through_the_axum_boundary() {
        let tenant = TenantId::new();
        let actor = ActorId::new();
        let device = DeviceId::new();
        let scope = SyncScopeId::new();
        let auth = AuthContext {
            actor_id: actor,
            tenant_id: tenant,
            device_id: device,
        };
        let app = aequora_axum::router(Arc::new(EchoService), 1024 * 1024).layer(Extension(auth));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("{error}"));
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let base_url = Url::parse(&format!("http://{address}/application/base/"))
            .unwrap_or_else(|error| panic!("{error}"));
        let transport = HttpTransport::new(
            Client::new(),
            &base_url,
            NoRequestHeaders,
            HttpTransportConfig::default(),
        )
        .unwrap_or_else(|error| panic!("{error}"));

        let request = SyncRequest {
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
        let response = transport
            .exchange(request.clone())
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(response.next_cursor.scope, scope);

        let bounded = HttpTransport::new(
            Client::new(),
            &base_url,
            NoRequestHeaders,
            HttpTransportConfig {
                max_response_bytes: 1,
                max_decompressed_response_bytes: 1,
            },
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let Err(error) = bounded.exchange(request).await else {
            panic!("a response larger than one byte must be rejected");
        };
        assert_eq!(error.kind, TransportErrorKind::Permanent);
        assert!(error.message.contains("wire limit"));

        server.abort();
        assert!(matches!(server.await, Err(error) if error.is_cancelled()));
    }
}
