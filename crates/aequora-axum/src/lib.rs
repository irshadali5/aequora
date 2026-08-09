//! Thin Axum boundary for framed Postcard exchanges.

use aequora_codec::{CodecError, Compression, DecodeLimits, EncodeOptions, MessageKind};
use aequora_executor::AuthContext;
use aequora_observability::{MetricEvent, NoopObserver, Observer};
use aequora_protocol::{BootstrapRequest, Capability, SyncRequest, SyncResponse};
use aequora_server::{ExchangeService, ServerError};
use aequora_store::StoreErrorKind;
use axum::{
    Extension, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::sync::Arc;

/// Primary synchronization media type.
pub const POSTCARD_CONTENT_TYPE: &str = "application/vnd.aequora.postcard";

#[derive(Clone)]
struct AppState {
    service: Arc<dyn ExchangeService>,
    config: AxumConfig,
    observer: Arc<dyn Observer>,
}

/// HTTP framing, decompression, and response-compression limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AxumConfig {
    /// Maximum compressed request body bytes.
    pub max_body_bytes: usize,
    /// Maximum payload bytes after decompression.
    pub max_decompressed_bytes: usize,
    /// Minimum serialized response size considered for compression.
    pub compression_threshold: usize,
    /// zstd compression level used after capability negotiation.
    pub zstd_level: i32,
}

impl AxumConfig {
    /// Creates conservative defaults around an application-selected body limit.
    #[must_use]
    pub const fn new(max_body_bytes: usize) -> Self {
        Self {
            max_body_bytes,
            max_decompressed_bytes: max_body_bytes,
            compression_threshold: 4_096,
            zstd_level: 3,
        }
    }
}

/// Builds the Phase 1 exchange and health endpoints. The host application must add an
/// `Extension<AuthContext>` from its JWT/session/mTLS authentication middleware.
pub fn router(service: Arc<dyn ExchangeService>, max_body_bytes: usize) -> Router {
    router_with_config(service, AxumConfig::new(max_body_bytes))
}

/// Builds endpoints with independent compression-bomb and wire-size controls.
pub fn router_with_config(service: Arc<dyn ExchangeService>, config: AxumConfig) -> Router {
    router_with_observer(service, config, Arc::new(NoopObserver))
}

/// Builds endpoints with exact framed-byte instrumentation at the HTTP boundary.
pub fn router_with_observer(
    service: Arc<dyn ExchangeService>,
    config: AxumConfig,
    observer: Arc<dyn Observer>,
) -> Router {
    let state = AppState {
        service,
        config,
        observer,
    };
    Router::new()
        .route("/sync/v1/exchange", post(exchange))
        .route("/sync/v1/bootstrap", post(bootstrap))
        .route("/sync/v1/health", get(health))
        .layer(DefaultBodyLimit::max(config.max_body_bytes))
        .with_state(state)
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn exchange(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if content_type != Some(POSTCARD_CONTENT_TYPE) {
        return Err(HttpError::UnsupportedMediaType);
    }
    let (frame_protocol, request) = aequora_codec::decode_with_limits::<SyncRequest>(
        &body,
        MessageKind::SyncRequest,
        DecodeLimits {
            max_wire_bytes: state.config.max_body_bytes,
            max_decompressed_bytes: state.config.max_decompressed_bytes,
        },
    )?;
    if frame_protocol != request.protocol {
        return Err(HttpError::BadRequest(
            "frame and request protocol versions differ",
        ));
    }
    let supports_zstd = request.capabilities.contains(&Capability::Zstd);
    let response = state.service.exchange(auth, request).await?;
    encode_response(
        &response,
        supports_zstd,
        state.config,
        state.observer.as_ref(),
        body.len(),
    )
}

async fn bootstrap(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if content_type != Some(POSTCARD_CONTENT_TYPE) {
        return Err(HttpError::UnsupportedMediaType);
    }
    let (frame_protocol, request) = aequora_codec::decode_with_limits::<BootstrapRequest>(
        &body,
        MessageKind::BootstrapRequest,
        DecodeLimits {
            max_wire_bytes: state.config.max_body_bytes,
            max_decompressed_bytes: state.config.max_decompressed_bytes,
        },
    )?;
    if frame_protocol != request.protocol {
        return Err(HttpError::BadRequest(
            "frame and request protocol versions differ",
        ));
    }
    let supports_zstd = request.capabilities.contains(&Capability::Zstd);
    let response = state.service.bootstrap(auth, request).await?;
    let bytes = aequora_codec::encode_with_options(
        response.protocol,
        MessageKind::BootstrapResponse,
        &response,
        compression_options(supports_zstd, state.config),
    )?;
    record_transport_bytes(state.observer.as_ref(), body.len(), bytes.len());
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static(POSTCARD_CONTENT_TYPE),
    );
    Ok((StatusCode::OK, response_headers, bytes).into_response())
}

fn encode_response(
    response: &SyncResponse,
    supports_zstd: bool,
    config: AxumConfig,
    observer: &dyn Observer,
    uploaded: usize,
) -> Result<Response, HttpError> {
    let bytes = aequora_codec::encode_with_options(
        response.protocol,
        MessageKind::SyncResponse,
        response,
        compression_options(supports_zstd, config),
    )?;
    record_transport_bytes(observer, uploaded, bytes.len());
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static(POSTCARD_CONTENT_TYPE),
    );
    Ok((StatusCode::OK, headers, bytes).into_response())
}

fn record_transport_bytes(observer: &dyn Observer, uploaded: usize, downloaded: usize) {
    observer.record(MetricEvent::TransportBytes {
        uploaded: u64::try_from(uploaded).unwrap_or(u64::MAX),
        downloaded: u64::try_from(downloaded).unwrap_or(u64::MAX),
    });
}

fn compression_options(supports_zstd: bool, config: AxumConfig) -> EncodeOptions {
    EncodeOptions {
        compression: if supports_zstd {
            Compression::Zstd {
                level: config.zstd_level,
            }
        } else {
            Compression::None
        },
        compression_threshold: config.compression_threshold,
    }
}

enum HttpError {
    UnsupportedMediaType,
    BadRequest(&'static str),
    Codec(CodecError),
    Server(ServerError),
}

impl From<CodecError> for HttpError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<ServerError> for HttpError {
    fn from(error: ServerError) -> Self {
        Self::Server(error)
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported sync content type".to_owned(),
            ),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message.to_owned()),
            Self::Codec(error) => (StatusCode::BAD_REQUEST, error.to_string()),
            Self::Server(ServerError::Validation(error)) => {
                (StatusCode::BAD_REQUEST, error.to_string())
            }
            Self::Server(ServerError::Dependency(error)) => {
                (StatusCode::BAD_REQUEST, error.to_string())
            }
            Self::Server(ServerError::IdentityMismatch) => (
                StatusCode::UNAUTHORIZED,
                "authenticated identity mismatch".to_owned(),
            ),
            Self::Server(ServerError::ScopeAuthorization(_)) => (
                StatusCode::FORBIDDEN,
                "sync scope is not authorized".to_owned(),
            ),
            Self::Server(ServerError::Store(error)) if error.kind == StoreErrorKind::Transient => (
                StatusCode::SERVICE_UNAVAILABLE,
                "sync storage unavailable".to_owned(),
            ),
            Self::Server(
                ServerError::Store(_)
                | ServerError::VersionOverflow
                | ServerError::SnapshotNoProgress
                | ServerError::Compute(_)
                | ServerError::Merge(_),
            ) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "sync processing failed".to_owned(),
            ),
            Self::Server(ServerError::BootstrapUnavailable) => (
                StatusCode::NOT_IMPLEMENTED,
                "snapshot bootstrap is not available".to_owned(),
            ),
        };
        (status, message).into_response()
    }
}
