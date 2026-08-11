//! Thin Axum boundary for framed Postcard exchanges.

use aequora_codec::{CodecError, Compression, DecodeLimits, EncodeOptions, MessageKind};
use aequora_executor::AuthContext;
use aequora_observability::{MetricEvent, NoopObserver, Observer};
use aequora_protocol::{BootstrapRequest, Capability, SyncRequest, SyncResponse};
use aequora_server::{ExchangeService, ServerError};
use aequora_store::StoreErrorKind;
use aequora_types::TenantId;
use async_trait::async_trait;
use axum::{
    Extension, Router,
    body::{Bytes, to_bytes},
    extract::{DefaultBodyLimit, FromRequest, FromRequestParts, Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CONTENT_TYPE, RETRY_AFTER},
        request::Parts,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use http_body_util::LengthLimitError;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use tokio::{sync::Notify, time::timeout};

/// Primary synchronization media type.
pub const POSTCARD_CONTENT_TYPE: &str = "application/vnd.aequora.postcard";

#[derive(Clone)]
struct AppState {
    service: Arc<dyn ExchangeService>,
    config: AxumConfig,
    observer: Arc<dyn Observer>,
    readiness: Arc<dyn ReadinessProbe>,
    lifecycle: ServerLifecycle,
}

#[derive(Debug)]
struct LifecycleState {
    draining: bool,
    in_flight: usize,
    tenant_in_flight: HashMap<TenantId, usize>,
    tenant_rate_buckets: HashMap<TenantId, TenantRateBucket>,
}

#[derive(Clone, Copy)]
struct LifecycleSnapshot {
    draining: bool,
    in_flight: usize,
}

struct LifecycleInner {
    maximum_in_flight: usize,
    maximum_in_flight_per_tenant: usize,
    rate_limit: TenantRateLimitConfig,
    state: Mutex<LifecycleState>,
    observer: Arc<dyn Observer>,
    changed: Notify,
}

#[derive(Clone, Copy)]
struct TenantRateLimitConfig {
    requests_per_second: u32,
    burst: u32,
    idle_timeout: Duration,
    maximum_tracked_tenants: usize,
}

#[derive(Debug)]
struct TenantRateBucket {
    tokens: f64,
    updated_at: Instant,
    last_seen: Instant,
}

/// Cloneable, irreversible accepting-to-draining lifecycle for one Axum router instance.
#[derive(Clone)]
pub struct ServerLifecycle {
    inner: Arc<LifecycleInner>,
}

/// Result of waiting for admitted synchronization work during graceful shutdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainOutcome {
    /// Every admitted request released its lifecycle permit before the deadline.
    Drained,
    /// The deadline elapsed while requests were still admitted.
    TimedOut { remaining: usize },
}

impl ServerLifecycle {
    fn new(config: AxumConfig, observer: Arc<dyn Observer>) -> Self {
        Self {
            inner: Arc::new(LifecycleInner {
                maximum_in_flight: config.max_in_flight_requests,
                maximum_in_flight_per_tenant: config.max_in_flight_per_tenant,
                rate_limit: TenantRateLimitConfig {
                    requests_per_second: config.tenant_requests_per_second,
                    burst: config.tenant_request_burst,
                    idle_timeout: config.rate_limit_idle_timeout,
                    maximum_tracked_tenants: config.max_rate_limit_tenants,
                },
                state: Mutex::new(LifecycleState {
                    draining: false,
                    in_flight: 0,
                    tenant_in_flight: HashMap::new(),
                    tenant_rate_buckets: HashMap::new(),
                }),
                observer,
                changed: Notify::new(),
            }),
        }
    }

    /// Maximum simultaneous requests admitted by this lifecycle.
    #[must_use]
    pub fn maximum_in_flight(&self) -> usize {
        self.inner.maximum_in_flight
    }

    /// Maximum simultaneous requests admitted for one authenticated tenant.
    #[must_use]
    pub fn maximum_in_flight_per_tenant(&self) -> usize {
        self.inner.maximum_in_flight_per_tenant
    }

    /// Whether irreversible graceful draining has begun.
    #[must_use]
    pub fn is_draining(&self) -> bool {
        self.state().draining
    }

    /// Exact number of currently admitted exchange/bootstrap requests.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.state().in_flight
    }

    /// Exact number of currently admitted requests for `tenant_id`.
    #[must_use]
    pub fn tenant_in_flight(&self, tenant_id: TenantId) -> usize {
        self.state()
            .tenant_in_flight
            .get(&tenant_id)
            .copied()
            .unwrap_or(0)
    }

    /// Number of tenant counters currently retained by the lifecycle.
    #[must_use]
    pub fn active_tenants(&self) -> usize {
        self.state().tenant_in_flight.len()
    }

    /// Number of retained tenant rate buckets, bounded by the configured maximum.
    #[must_use]
    pub fn tracked_rate_limit_tenants(&self) -> usize {
        self.state().tenant_rate_buckets.len()
    }

    /// Atomically prevents new admissions and returns the exact requests already in flight.
    #[must_use]
    pub fn begin_draining(&self) -> usize {
        let snapshot = {
            let mut state = self.state();
            state.draining = true;
            state.snapshot()
        };
        self.record_state(snapshot);
        if snapshot.in_flight == 0 {
            self.inner.changed.notify_waiters();
        }
        snapshot.in_flight
    }

    /// Begins draining and waits until all admitted work exits or `deadline` elapses.
    pub async fn drain(&self, deadline: Duration) -> DrainOutcome {
        let started = Instant::now();
        let _ = self.begin_draining();
        let drained =
            self.in_flight() == 0 || timeout(deadline, self.wait_until_empty()).await.is_ok();
        let outcome = if drained {
            DrainOutcome::Drained
        } else {
            let remaining = self.in_flight();
            if remaining == 0 {
                DrainOutcome::Drained
            } else {
                DrainOutcome::TimedOut { remaining }
            }
        };
        let remaining = match outcome {
            DrainOutcome::Drained => 0,
            DrainOutcome::TimedOut { remaining } => remaining,
        };
        self.observer().record(MetricEvent::ServerDrainOutcome {
            duration_micros: duration_micros(started.elapsed()),
            remaining: usize_to_u64(remaining),
            timed_out: matches!(outcome, DrainOutcome::TimedOut { .. }),
        });
        outcome
    }

    fn try_admit(&self, tenant_id: TenantId) -> Result<LifecyclePermit, AdmissionFailure> {
        let snapshot = {
            let mut state = self.state();
            if state.draining {
                drop(state);
                self.observer().record(MetricEvent::ServerDrainingRejected);
                return Err(AdmissionFailure::Draining);
            }
            if state.in_flight >= self.inner.maximum_in_flight {
                return Err(AdmissionFailure::Saturated);
            }
            if state.tenant_in_flight.get(&tenant_id).copied().unwrap_or(0)
                >= self.inner.maximum_in_flight_per_tenant
            {
                return Err(AdmissionFailure::TenantSaturated);
            }
            if !state.rate_limit_allows(tenant_id, Instant::now(), self.inner.rate_limit) {
                return Err(AdmissionFailure::TenantRateLimited);
            }
            state.in_flight = state.in_flight.saturating_add(1);
            let tenant_count = state.tenant_in_flight.entry(tenant_id).or_default();
            *tenant_count = tenant_count.saturating_add(1);
            state.snapshot()
        };
        self.record_state(snapshot);
        Ok(LifecyclePermit {
            lifecycle: self.clone(),
            tenant_id,
        })
    }

    fn release(&self, tenant_id: TenantId) {
        let snapshot = {
            let mut state = self.state();
            state.in_flight = state.in_flight.saturating_sub(1);
            if let Some(tenant_count) = state.tenant_in_flight.get_mut(&tenant_id) {
                *tenant_count = tenant_count.saturating_sub(1);
                if *tenant_count == 0 {
                    state.tenant_in_flight.remove(&tenant_id);
                }
            }
            state.snapshot()
        };
        self.record_state(snapshot);
        self.inner.changed.notify_waiters();
    }

    async fn wait_until_empty(&self) {
        loop {
            let changed = self.inner.changed.notified();
            if self.in_flight() == 0 {
                return;
            }
            changed.await;
        }
    }

    fn state(&self) -> MutexGuard<'_, LifecycleState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn observer(&self) -> Arc<dyn Observer> {
        self.inner.observer.clone()
    }

    fn record_state(&self, state: LifecycleSnapshot) {
        self.observer().record(MetricEvent::ServerLifecycle {
            draining: state.draining,
            in_flight: usize_to_u64(state.in_flight),
        });
    }
}

impl LifecycleState {
    const fn snapshot(&self) -> LifecycleSnapshot {
        LifecycleSnapshot {
            draining: self.draining,
            in_flight: self.in_flight,
        }
    }

    fn rate_limit_allows(
        &mut self,
        tenant_id: TenantId,
        now: Instant,
        config: TenantRateLimitConfig,
    ) -> bool {
        if let Some(bucket) = self.tenant_rate_buckets.get_mut(&tenant_id) {
            return bucket.try_consume(now, config);
        }
        self.evict_expired_rate_buckets(now, config.idle_timeout);
        if self.tenant_rate_buckets.len() >= config.maximum_tracked_tenants {
            self.evict_oldest_inactive_rate_bucket();
        }
        if self.tenant_rate_buckets.len() >= config.maximum_tracked_tenants {
            return false;
        }
        let mut bucket = TenantRateBucket::new(now, config.burst);
        let allowed = bucket.try_consume(now, config);
        self.tenant_rate_buckets.insert(tenant_id, bucket);
        allowed
    }

    fn evict_expired_rate_buckets(&mut self, now: Instant, idle_timeout: Duration) {
        let active = &self.tenant_in_flight;
        self.tenant_rate_buckets.retain(|tenant_id, bucket| {
            active.contains_key(tenant_id)
                || now.saturating_duration_since(bucket.last_seen) < idle_timeout
        });
    }

    fn evict_oldest_inactive_rate_bucket(&mut self) {
        let oldest = self
            .tenant_rate_buckets
            .iter()
            .filter(|(tenant_id, _)| !self.tenant_in_flight.contains_key(tenant_id))
            .min_by_key(|(_, bucket)| bucket.last_seen)
            .map(|(tenant_id, _)| *tenant_id);
        if let Some(tenant_id) = oldest {
            self.tenant_rate_buckets.remove(&tenant_id);
        }
    }
}

impl TenantRateBucket {
    fn new(now: Instant, burst: u32) -> Self {
        Self {
            tokens: f64::from(burst),
            updated_at: now,
            last_seen: now,
        }
    }

    fn try_consume(&mut self, now: Instant, config: TenantRateLimitConfig) -> bool {
        let elapsed = now.saturating_duration_since(self.updated_at);
        let refill = elapsed.as_secs_f64() * f64::from(config.requests_per_second);
        self.tokens = (self.tokens + refill).min(f64::from(config.burst));
        self.updated_at = now;
        self.last_seen = now;
        if self.tokens < 1.0 {
            false
        } else {
            self.tokens -= 1.0;
            true
        }
    }
}

struct LifecyclePermit {
    lifecycle: ServerLifecycle,
    tenant_id: TenantId,
}

impl Drop for LifecyclePermit {
    fn drop(&mut self) {
        self.lifecycle.release(self.tenant_id);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdmissionFailure {
    Saturated,
    TenantSaturated,
    TenantRateLimited,
    Draining,
}

/// Application-owned dependency readiness boundary.
///
/// Implementations can check `PostgreSQL`, Neon, custom stores, or a composite dependency set
/// without coupling this transport crate to any persistence adapter. Failure details should be
/// logged by the host and are deliberately not returned to unauthenticated callers.
#[async_trait]
pub trait ReadinessProbe: Send + Sync {
    /// Returns whether the process can currently serve synchronization traffic.
    async fn ready(&self) -> bool;
}

/// Adapts an async application closure into a [`ReadinessProbe`].
///
/// This keeps adapter dependencies outside Aequora's HTTP crate while allowing concise probes
/// around methods such as `SqlxPostgresBackend::health_check`.
pub struct ReadinessFn<F>(F);

impl<F> ReadinessFn<F> {
    /// Wraps an async readiness closure.
    pub const fn new(probe: F) -> Self {
        Self(probe)
    }
}

#[async_trait]
impl<F, Fut> ReadinessProbe for ReadinessFn<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = bool> + Send,
{
    async fn ready(&self) -> bool {
        (self.0)().await
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct AlwaysReady;

#[async_trait]
impl ReadinessProbe for AlwaysReady {
    async fn ready(&self) -> bool {
        true
    }
}

/// HTTP framing, decompression, and response-compression limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AxumConfig {
    /// Maximum compressed request body bytes.
    pub max_body_bytes: usize,
    /// Maximum payload bytes after decompression.
    pub max_decompressed_bytes: usize,
    /// Maximum time allowed to receive the complete compressed request frame.
    pub body_read_timeout: Duration,
    /// Minimum serialized response size considered for compression.
    pub compression_threshold: usize,
    /// zstd compression level used after capability negotiation.
    pub zstd_level: i32,
    /// Whether this server deployment permits negotiated zstd responses.
    pub zstd_enabled: bool,
    /// Maximum exchange and bootstrap handlers executing concurrently.
    pub max_in_flight_requests: usize,
    /// Maximum exchange and bootstrap handlers executing for one authenticated tenant.
    pub max_in_flight_per_tenant: usize,
    /// Sustained admitted requests per second for one authenticated tenant.
    pub tenant_requests_per_second: u32,
    /// Maximum immediately consumable request tokens for one authenticated tenant.
    pub tenant_request_burst: u32,
    /// Maximum retained tenant rate buckets, including inactive tenants.
    pub max_rate_limit_tenants: usize,
    /// Duration after which an inactive tenant rate bucket can be discarded.
    pub rate_limit_idle_timeout: Duration,
    /// Maximum authoritative service execution time for one admitted request.
    pub request_timeout: Duration,
    /// Maximum time allowed for one dependency-readiness probe.
    pub readiness_timeout: Duration,
    /// Maximum graceful-drain wait after the host receives a shutdown signal.
    pub drain_timeout: Duration,
    /// Whole seconds advertised in `Retry-After` for overload and deadline responses.
    pub retry_after_seconds: u64,
}

impl AxumConfig {
    /// Creates conservative defaults around an application-selected body limit.
    #[must_use]
    pub const fn new(max_body_bytes: usize) -> Self {
        Self {
            max_body_bytes,
            max_decompressed_bytes: max_body_bytes,
            body_read_timeout: Duration::from_secs(15),
            compression_threshold: 4_096,
            zstd_level: 3,
            zstd_enabled: true,
            max_in_flight_requests: 256,
            max_in_flight_per_tenant: 64,
            tenant_requests_per_second: 64,
            tenant_request_burst: 128,
            max_rate_limit_tenants: 4_096,
            rate_limit_idle_timeout: Duration::from_secs(300),
            request_timeout: Duration::from_secs(30),
            readiness_timeout: Duration::from_secs(2),
            drain_timeout: Duration::from_secs(30),
            retry_after_seconds: 1,
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
    router_with_readiness(service, config, observer, Arc::new(AlwaysReady))
}

/// Builds endpoints with bounded admission/deadlines and an application-owned readiness probe.
pub fn router_with_readiness(
    service: Arc<dyn ExchangeService>,
    config: AxumConfig,
    observer: Arc<dyn Observer>,
    readiness_probe: Arc<dyn ReadinessProbe>,
) -> Router {
    router_with_lifecycle(service, config, observer, readiness_probe).0
}

/// Builds a router and returns the lifecycle handle used for graceful deployment draining.
pub fn router_with_lifecycle(
    service: Arc<dyn ExchangeService>,
    config: AxumConfig,
    observer: Arc<dyn Observer>,
    readiness_probe: Arc<dyn ReadinessProbe>,
) -> (Router, ServerLifecycle) {
    let lifecycle = ServerLifecycle::new(config, observer.clone());
    let state = AppState {
        service,
        config,
        observer,
        readiness: readiness_probe,
        lifecycle: lifecycle.clone(),
    };
    let router = Router::new()
        .route("/sync/v1/exchange", post(exchange))
        .route("/sync/v1/bootstrap", post(bootstrap))
        .route("/sync/v1/health", get(liveness))
        .route("/sync/v1/health/live", get(liveness))
        .route("/sync/v1/health/ready", get(readiness))
        .layer(DefaultBodyLimit::max(config.max_body_bytes))
        .with_state(state);
    (router, lifecycle)
}

async fn liveness() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn readiness(State(state): State<AppState>) -> StatusCode {
    let probe_ready = if state.lifecycle.is_draining() {
        false
    } else {
        timeout(state.config.readiness_timeout, state.readiness.ready())
            .await
            .unwrap_or(false)
    };
    let ready = probe_ready && !state.lifecycle.is_draining();
    state
        .observer
        .record(MetricEvent::ServerReadiness { ready });
    if ready {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

struct Admission {
    _permit: LifecyclePermit,
}

struct SyncBody {
    headers: HeaderMap,
    bytes: Bytes,
}

impl FromRequestParts<AppState> for Admission {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let tenant_id = parts
            .extensions
            .get::<AuthContext>()
            .map(|auth| auth.tenant_id)
            .ok_or(HttpError::MissingAuthentication)?;
        admission_permit(state, tenant_id).map(|permit| Self { _permit: permit })
    }
}

impl FromRequest<AppState> for SyncBody {
    type Rejection = HttpError;

    async fn from_request(request: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        let (parts, body) = request.into_parts();
        let bytes = match timeout(
            state.config.body_read_timeout,
            to_bytes(body, state.config.max_body_bytes),
        )
        .await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                let too_large = match std::error::Error::source(&error) {
                    Some(source) => source.is::<LengthLimitError>(),
                    None => false,
                };
                if too_large {
                    state.observer.record(MetricEvent::ServerBodyTooLarge);
                    return Err(HttpError::BodyTooLarge);
                }
                return Err(HttpError::BadRequest("sync request body could not be read"));
            }
            Err(_) => {
                state.observer.record(MetricEvent::ServerBodyReadTimedOut);
                return Err(HttpError::BodyReadTimedOut(
                    state.config.retry_after_seconds,
                ));
            }
        };
        Ok(Self {
            headers: parts.headers,
            bytes,
        })
    }
}

async fn exchange(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    _admission: Admission,
    SyncBody {
        headers,
        bytes: body,
    }: SyncBody,
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
    let response = timeout(
        state.config.request_timeout,
        state.service.exchange(auth, request),
    )
    .await
    .map_err(|_| {
        state.observer.record(MetricEvent::ServerDeadlineExceeded);
        HttpError::DeadlineExceeded(state.config.retry_after_seconds)
    })??;
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
    _admission: Admission,
    SyncBody {
        headers,
        bytes: body,
    }: SyncBody,
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
    let response = timeout(
        state.config.request_timeout,
        state.service.bootstrap(auth, request),
    )
    .await
    .map_err(|_| {
        state.observer.record(MetricEvent::ServerDeadlineExceeded);
        HttpError::DeadlineExceeded(state.config.retry_after_seconds)
    })??;
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

fn admission_permit(state: &AppState, tenant_id: TenantId) -> Result<LifecyclePermit, HttpError> {
    state
        .lifecycle
        .try_admit(tenant_id)
        .map_err(|failure| match failure {
            AdmissionFailure::Saturated => {
                state.observer.record(MetricEvent::ServerOverloaded);
                HttpError::Overloaded(state.config.retry_after_seconds)
            }
            AdmissionFailure::TenantSaturated => {
                state.observer.record(MetricEvent::ServerTenantOverloaded);
                HttpError::TenantOverloaded(state.config.retry_after_seconds)
            }
            AdmissionFailure::TenantRateLimited => {
                state.observer.record(MetricEvent::ServerTenantRateLimited);
                HttpError::TenantRateLimited(state.config.retry_after_seconds)
            }
            AdmissionFailure::Draining => HttpError::Draining(state.config.retry_after_seconds),
        })
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

fn duration_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn compression_options(supports_zstd: bool, config: AxumConfig) -> EncodeOptions {
    EncodeOptions {
        compression: if supports_zstd && config.zstd_enabled {
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
    BodyTooLarge,
    BodyReadTimedOut(u64),
    MissingAuthentication,
    Overloaded(u64),
    TenantOverloaded(u64),
    TenantRateLimited(u64),
    Draining(u64),
    DeadlineExceeded(u64),
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
        let (status, message, retry_after) = match self {
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported sync content type".to_owned(),
                None,
            ),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message.to_owned(), None),
            Self::BodyTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "sync request body exceeds the configured wire limit".to_owned(),
                None,
            ),
            Self::BodyReadTimedOut(seconds) => (
                StatusCode::REQUEST_TIMEOUT,
                "sync request body exceeded its receive deadline".to_owned(),
                Some(seconds),
            ),
            Self::MissingAuthentication => (
                StatusCode::UNAUTHORIZED,
                "authenticated identity is unavailable".to_owned(),
                None,
            ),
            Self::Overloaded(seconds) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "sync server is at its in-flight request limit".to_owned(),
                Some(seconds),
            ),
            Self::TenantOverloaded(seconds) => (
                StatusCode::TOO_MANY_REQUESTS,
                "tenant is at its in-flight sync request limit".to_owned(),
                Some(seconds),
            ),
            Self::TenantRateLimited(seconds) => (
                StatusCode::TOO_MANY_REQUESTS,
                "tenant sync request rate limit exceeded".to_owned(),
                Some(seconds),
            ),
            Self::Draining(seconds) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "sync server is draining".to_owned(),
                Some(seconds),
            ),
            Self::DeadlineExceeded(seconds) => (
                StatusCode::GATEWAY_TIMEOUT,
                "sync request exceeded its server execution deadline".to_owned(),
                Some(seconds),
            ),
            Self::Codec(error) => (StatusCode::BAD_REQUEST, error.to_string(), None),
            Self::Server(ServerError::Validation(error)) => {
                (StatusCode::BAD_REQUEST, error.to_string(), None)
            }
            Self::Server(ServerError::Dependency(error)) => {
                (StatusCode::BAD_REQUEST, error.to_string(), None)
            }
            Self::Server(ServerError::ResponseLimit) => (
                StatusCode::BAD_REQUEST,
                "client response limit is too small".to_owned(),
                None,
            ),
            Self::Server(ServerError::IdentityMismatch) => (
                StatusCode::UNAUTHORIZED,
                "authenticated identity mismatch".to_owned(),
                None,
            ),
            Self::Server(ServerError::ScopeAuthorization(_)) => (
                StatusCode::FORBIDDEN,
                "sync scope is not authorized".to_owned(),
                None,
            ),
            Self::Server(ServerError::Store(error)) if error.kind == StoreErrorKind::Transient => (
                StatusCode::SERVICE_UNAVAILABLE,
                "sync storage unavailable".to_owned(),
                None,
            ),
            Self::Server(
                ServerError::Store(_)
                | ServerError::VersionOverflow
                | ServerError::SnapshotNoProgress
                | ServerError::Compute(_)
                | ServerError::Codec(_)
                | ServerError::Merge(_),
            ) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "sync processing failed".to_owned(),
                None,
            ),
            Self::Server(ServerError::BootstrapUnavailable) => (
                StatusCode::NOT_IMPLEMENTED,
                "snapshot bootstrap is not available".to_owned(),
                None,
            ),
        };
        let mut response = (status, message).into_response();
        if let Some(value) =
            retry_after.and_then(|seconds| HeaderValue::from_str(&seconds.to_string()).ok())
        {
            response.headers_mut().insert(RETRY_AFTER, value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rate_config() -> TenantRateLimitConfig {
        TenantRateLimitConfig {
            requests_per_second: 2,
            burst: 2,
            idle_timeout: Duration::from_secs(5),
            maximum_tracked_tenants: 2,
        }
    }

    #[test]
    fn token_bucket_enforces_burst_refills_and_caps_capacity() {
        let now = Instant::now();
        let config = rate_config();
        let mut bucket = TenantRateBucket::new(now, config.burst);
        assert!(bucket.try_consume(now, config));
        assert!(bucket.try_consume(now, config));
        assert!(!bucket.try_consume(now, config));
        assert!(bucket.try_consume(now + Duration::from_millis(500), config));
        assert!(!bucket.try_consume(now + Duration::from_millis(500), config));
        assert!(bucket.try_consume(now + Duration::from_secs(10), config));
        assert!(bucket.try_consume(now + Duration::from_secs(10), config));
        assert!(!bucket.try_consume(now + Duration::from_secs(10), config));
    }

    #[test]
    fn rate_bucket_eviction_preserves_active_tenants_and_removes_idle_state() {
        let now = Instant::now();
        let config = rate_config();
        let active_tenant = TenantId::new();
        let inactive_tenant = TenantId::new();
        let replacement_tenant = TenantId::new();
        let post_expiry_tenant = TenantId::new();
        let mut state = LifecycleState {
            draining: false,
            in_flight: 1,
            tenant_in_flight: HashMap::from([(active_tenant, 1)]),
            tenant_rate_buckets: HashMap::new(),
        };
        assert!(state.rate_limit_allows(active_tenant, now, config));
        assert!(state.rate_limit_allows(inactive_tenant, now + Duration::from_millis(1), config,));
        assert!(state.rate_limit_allows(
            replacement_tenant,
            now + Duration::from_millis(2),
            config,
        ));
        assert!(state.tenant_rate_buckets.contains_key(&active_tenant));
        assert!(!state.tenant_rate_buckets.contains_key(&inactive_tenant));
        assert!(state.tenant_rate_buckets.contains_key(&replacement_tenant));

        state.tenant_in_flight.clear();
        state.in_flight = 0;
        assert!(state.rate_limit_allows(post_expiry_tenant, now + Duration::from_secs(6), config,));
        assert_eq!(state.tenant_rate_buckets.len(), 1);
        assert!(state.tenant_rate_buckets.contains_key(&post_expiry_tenant));
    }
}
