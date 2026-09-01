//! Thin Axum boundary for framed Postcard exchanges.

use aequora_codec::{
    CodecError, Compression, DecodeLimits, EncodeOptions, HEADER_LEN, MessageKind, inspect_header,
};
use aequora_executor::AuthContext;
use aequora_observability::{MetricEvent, NoopObserver, Observer};
use aequora_protocol::{BootstrapRequest, Capability, SyncRequest, SyncResponse};
use aequora_server::{ExchangeService, ServerError};
use aequora_store::StoreErrorKind;
use aequora_types::{OPERATIONAL_ERROR_CODE_HEADER, OperationalErrorCode, TenantId};
use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequest, FromRequestParts, Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER},
        request::Parts,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use bytes::BytesMut;
use http_body_util::BodyExt as _;
use std::{
    collections::HashMap,
    fmt,
    future::Future,
    str::FromStr as _,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use tokio::{sync::Notify, time::timeout};

/// Primary synchronization media type.
pub const POSTCARD_CONTENT_TYPE: &str = "application/vnd.aequora.postcard";
/// Stable JSON media type used for transport-level error envelopes.
pub const ERROR_CONTENT_TYPE: &str = "application/json";
/// Validated or server-generated request correlation header.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Opaque credential presented at the HTTP boundary.
///
/// The value is intentionally neither serializable nor printable. Authentication adapters can
/// inspect it only while producing a canonical [`AuthContext`].
#[derive(Clone)]
pub struct PresentedCredential(Arc<str>);

impl PresentedCredential {
    fn new(value: &str) -> Self {
        Self(Arc::from(value))
    }

    /// Borrows the credential for authentication only.
    #[must_use]
    pub fn expose_for_authentication(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PresentedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PresentedCredential([REDACTED])")
    }
}

/// Stable reason an HTTP authentication provider rejected or could not validate a credential.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticationFailure {
    /// Credential syntax or signature is invalid.
    Invalid,
    /// Credential is outside its validity window.
    Expired,
    /// Credential or its device binding has been revoked.
    Revoked,
    /// Authentication provider is temporarily unavailable.
    Unavailable,
}

/// Transport authentication boundary that normalizes credentials into server-core identity.
#[async_trait]
pub trait HttpAuthenticator: Send + Sync {
    /// Validates one opaque credential without passing it to domain or storage code.
    async fn authenticate(
        &self,
        credential: &PresentedCredential,
    ) -> Result<AuthContext, AuthenticationFailure>;
}

/// Canonical transport context passed to thin route orchestration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestContext {
    /// Validated or server-generated request correlation identity.
    pub request_id: aequora_types::RequestId,
    /// Server-validated actor, tenant, and device identity.
    pub auth: AuthContext,
}

#[derive(Clone)]
struct AppState {
    service: Arc<dyn ExchangeService>,
    config: AxumConfig,
    observer: Arc<dyn Observer>,
    readiness: Arc<dyn ReadinessProbe>,
    lifecycle: ServerLifecycle,
    authenticator: Option<Arc<dyn HttpAuthenticator>>,
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
    /// Maximum accepted bearer credential bytes before authentication is invoked.
    pub max_credential_bytes: usize,
    /// Maximum time allowed for the authentication provider.
    pub authentication_timeout: Duration,
    /// Whether a syntactically valid incoming `x-request-id` may be trusted.
    ///
    /// Leave disabled on an internet-facing listener unless a trusted ingress strips and sets it.
    pub trust_request_id_header: bool,
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
            max_credential_bytes: 4_096,
            authentication_timeout: Duration::from_secs(2),
            trust_request_id_header: false,
        }
    }
}

/// Builds the public sync, bootstrap, liveness, and readiness routes.
///
/// The host application must add an `Extension<AuthContext>` from trusted authentication
/// middleware. New integrations should prefer [`router_with_authenticator`].
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
    build_router(service, config, observer, readiness_probe, None)
}

/// Builds a production authentication boundary and returns its graceful-drain handle.
///
/// Bearer material is bounded and consumed only by `authenticator`; route handlers and the
/// transport-neutral service receive only [`AuthContext`].
pub fn router_with_authenticator(
    service: Arc<dyn ExchangeService>,
    config: AxumConfig,
    observer: Arc<dyn Observer>,
    readiness_probe: Arc<dyn ReadinessProbe>,
    authenticator: Arc<dyn HttpAuthenticator>,
) -> (Router, ServerLifecycle) {
    build_router(
        service,
        config,
        observer,
        readiness_probe,
        Some(authenticator),
    )
}

fn build_router(
    service: Arc<dyn ExchangeService>,
    config: AxumConfig,
    observer: Arc<dyn Observer>,
    readiness_probe: Arc<dyn ReadinessProbe>,
    authenticator: Option<Arc<dyn HttpAuthenticator>>,
) -> (Router, ServerLifecycle) {
    let lifecycle = ServerLifecycle::new(config, observer.clone());
    let state = AppState {
        service,
        config,
        observer,
        readiness: readiness_probe,
        lifecycle: lifecycle.clone(),
        authenticator,
    };
    let router = Router::new()
        .route("/sync/v1/exchange", post(exchange))
        .route("/sync/v1/bootstrap", post(bootstrap))
        .route("/sync/v1/health", get(liveness))
        .route("/sync/v1/health/live", get(liveness))
        .route("/sync/v1/ready", get(readiness))
        .route("/sync/v1/health/ready", get(readiness))
        .layer(DefaultBodyLimit::max(config.max_body_bytes))
        .with_state(state);
    (router, lifecycle)
}

async fn liveness(RequestIdentity(request_id): RequestIdentity) -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    apply_public_response_headers(&mut response, request_id);
    response
}

async fn readiness(
    State(state): State<AppState>,
    RequestIdentity(request_id): RequestIdentity,
) -> Response {
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
    let mut response = if ready {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
    .into_response();
    apply_public_response_headers(&mut response, request_id);
    response
}

struct Admission {
    _permit: LifecyclePermit,
}

struct ProtocolHeaders;

struct Authentication(AuthContext);

#[derive(Clone, Copy)]
struct RequestIdentity(aequora_types::RequestId);

struct SyncBody {
    bytes: Bytes,
}

impl FromRequestParts<AppState> for ProtocolHeaders {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let request_id = request_identity(parts, state.config);
        let content_type = parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        if content_type == Some(POSTCARD_CONTENT_TYPE) {
            Ok(Self)
        } else {
            Err(HttpError::UnsupportedMediaType.with_request_id(request_id))
        }
    }
}

impl FromRequestParts<AppState> for Authentication {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let request_id = request_identity(parts, state.config);
        if let Some(auth) = parts.extensions.get::<AuthContext>().copied() {
            return Ok(Self(auth));
        }
        let authenticator = state
            .authenticator
            .as_ref()
            .ok_or(HttpError::MissingAuthentication)
            .map_err(|error| error.with_request_id(request_id))?;
        let value = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|value| !value.is_empty() && value.len() <= state.config.max_credential_bytes)
            .ok_or(HttpError::MissingAuthentication)
            .map_err(|error| error.with_request_id(request_id))?;
        let credential = PresentedCredential::new(value);
        let auth = timeout(
            state.config.authentication_timeout,
            authenticator.authenticate(&credential),
        )
        .await
        .map_err(|_| {
            HttpError::AuthenticationUnavailable(state.config.retry_after_seconds)
                .with_request_id(request_id)
        })?
        .map_err(|failure| match failure {
            AuthenticationFailure::Invalid
            | AuthenticationFailure::Expired
            | AuthenticationFailure::Revoked => {
                HttpError::AuthenticationRejected.with_request_id(request_id)
            }
            AuthenticationFailure::Unavailable => {
                HttpError::AuthenticationUnavailable(state.config.retry_after_seconds)
                    .with_request_id(request_id)
            }
        })?;
        parts.extensions.insert(auth);
        Ok(Self(auth))
    }
}

impl FromRequestParts<AppState> for RequestIdentity {
    type Rejection = HttpError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self(request_identity(parts, state.config)))
    }
}

fn request_identity(parts: &mut Parts, config: AxumConfig) -> aequora_types::RequestId {
    if let Some(existing) = parts.extensions.get::<aequora_types::RequestId>() {
        return *existing;
    }
    let trusted = config
        .trust_request_id_header
        .then(|| parts.headers.get(REQUEST_ID_HEADER))
        .flatten()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| aequora_types::RequestId::from_str(value).ok());
    let request_id = trusted.unwrap_or_default();
    parts.extensions.insert(request_id);
    request_id
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
        let request_id = request_identity(parts, state.config);
        admission_permit(state, tenant_id)
            .map(|permit| Self { _permit: permit })
            .map_err(|error| error.with_request_id(request_id))
    }
}

impl FromRequest<AppState> for SyncBody {
    type Rejection = HttpError;

    async fn from_request(request: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        let (parts, body) = request.into_parts();
        let request_id = parts
            .extensions
            .get::<aequora_types::RequestId>()
            .copied()
            .unwrap_or_default();
        let bytes = match timeout(
            state.config.body_read_timeout,
            read_bounded_body(body, state.config.max_body_bytes),
        )
        .await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                if matches!(error, HttpError::BodyTooLarge) {
                    state.observer.record(MetricEvent::ServerBodyTooLarge);
                }
                return Err(error.with_request_id(request_id));
            }
            Err(_) => {
                state.observer.record(MetricEvent::ServerBodyReadTimedOut);
                return Err(
                    HttpError::BodyReadTimedOut(state.config.retry_after_seconds)
                        .with_request_id(request_id),
                );
            }
        };
        Ok(Self { bytes })
    }
}

async fn read_bounded_body(mut body: Body, max_body_bytes: usize) -> Result<Bytes, HttpError> {
    if max_body_bytes < HEADER_LEN {
        return Err(HttpError::BodyTooLarge);
    }
    let mut buffer = BytesMut::with_capacity(HEADER_LEN);
    let mut expected_total = None;
    while let Some(frame) = body.frame().await {
        let frame =
            frame.map_err(|_| HttpError::BadRequest("sync request body could not be read"))?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        if buffer.len().saturating_add(data.len()) > max_body_bytes {
            return Err(HttpError::BodyTooLarge);
        }
        buffer.extend_from_slice(&data);
        if expected_total.is_none() && buffer.len() >= HEADER_LEN {
            let header = inspect_header(
                &buffer[..HEADER_LEN],
                max_body_bytes.saturating_sub(HEADER_LEN),
            )
            .map_err(|error| match error {
                CodecError::PayloadTooLarge { .. } => HttpError::BodyTooLarge,
                _ => HttpError::Codec,
            })?;
            expected_total = Some(HEADER_LEN.saturating_add(header.payload_len));
        }
        if expected_total.is_some_and(|expected| buffer.len() > expected) {
            return Err(HttpError::Codec);
        }
    }
    if expected_total != Some(buffer.len()) {
        return Err(HttpError::Codec);
    }
    Ok(buffer.freeze())
}

async fn exchange(
    State(state): State<AppState>,
    _protocol_headers: ProtocolHeaders,
    Authentication(auth): Authentication,
    _admission: Admission,
    RequestIdentity(request_id): RequestIdentity,
    SyncBody { bytes: body }: SyncBody,
) -> Result<Response, HttpError> {
    let (frame_protocol, request) = aequora_codec::decode_with_limits::<SyncRequest>(
        &body,
        MessageKind::SyncRequest,
        DecodeLimits {
            max_wire_bytes: state.config.max_body_bytes.saturating_sub(HEADER_LEN),
            max_decompressed_bytes: state.config.max_decompressed_bytes,
        },
    )
    .map_err(HttpError::from)
    .map_err(|error| error.with_request_id(request_id))?;
    if frame_protocol != request.protocol {
        return Err(
            HttpError::BadRequest("frame and request protocol versions differ")
                .with_request_id(request_id),
        );
    }
    let supports_zstd = request.capabilities.contains(&Capability::Zstd);
    let response = timeout(
        state.config.request_timeout,
        state.service.exchange(auth, request),
    )
    .await
    .map_err(|_| {
        state.observer.record(MetricEvent::ServerDeadlineExceeded);
        HttpError::DeadlineExceeded(state.config.retry_after_seconds).with_request_id(request_id)
    })?
    .map_err(HttpError::from)
    .map_err(|error| error.with_request_id(request_id))?;
    encode_response(
        &response,
        supports_zstd,
        state.config,
        state.observer.as_ref(),
        body.len(),
        request_id,
    )
}

async fn bootstrap(
    State(state): State<AppState>,
    _protocol_headers: ProtocolHeaders,
    Authentication(auth): Authentication,
    _admission: Admission,
    RequestIdentity(request_id): RequestIdentity,
    SyncBody { bytes: body }: SyncBody,
) -> Result<Response, HttpError> {
    let (frame_protocol, request) = aequora_codec::decode_with_limits::<BootstrapRequest>(
        &body,
        MessageKind::BootstrapRequest,
        DecodeLimits {
            max_wire_bytes: state.config.max_body_bytes.saturating_sub(HEADER_LEN),
            max_decompressed_bytes: state.config.max_decompressed_bytes,
        },
    )
    .map_err(HttpError::from)
    .map_err(|error| error.with_request_id(request_id))?;
    if frame_protocol != request.protocol {
        return Err(
            HttpError::BadRequest("frame and request protocol versions differ")
                .with_request_id(request_id),
        );
    }
    let supports_zstd = request.capabilities.contains(&Capability::Zstd);
    let response = timeout(
        state.config.request_timeout,
        state.service.bootstrap(auth, request),
    )
    .await
    .map_err(|_| {
        state.observer.record(MetricEvent::ServerDeadlineExceeded);
        HttpError::DeadlineExceeded(state.config.retry_after_seconds).with_request_id(request_id)
    })?
    .map_err(HttpError::from)
    .map_err(|error| error.with_request_id(request_id))?;
    let bytes = aequora_codec::encode_bytes_with_options(
        response.protocol,
        MessageKind::BootstrapResponse,
        &response,
        compression_options(supports_zstd, state.config),
    )
    .map_err(HttpError::from)
    .map_err(|error| error.with_request_id(request_id))?;
    record_transport_bytes(state.observer.as_ref(), body.len(), bytes.len());
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static(POSTCARD_CONTENT_TYPE),
    );
    let mut response = (StatusCode::OK, response_headers, bytes).into_response();
    apply_public_response_headers(&mut response, request_id);
    Ok(response)
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
    request_id: aequora_types::RequestId,
) -> Result<Response, HttpError> {
    let bytes = aequora_codec::encode_bytes_with_options(
        response.protocol,
        MessageKind::SyncResponse,
        response,
        compression_options(supports_zstd, config),
    )
    .map_err(HttpError::from)
    .map_err(|error| error.with_request_id(request_id))?;
    record_transport_bytes(observer, uploaded, bytes.len());
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static(POSTCARD_CONTENT_TYPE),
    );
    let mut response = (StatusCode::OK, headers, bytes).into_response();
    apply_public_response_headers(&mut response, request_id);
    Ok(response)
}

fn apply_public_response_headers(response: &mut Response, request_id: aequora_types::RequestId) {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    if let Ok(value) = HeaderValue::from_str(&request_id.to_string()) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
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

/// Sanitized, stable transport error returned to an HTTP client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiErrorEnvelope {
    /// Machine-stable Aequora error category.
    pub code: OperationalErrorCode,
    /// Bounded retry hint for temporary failures.
    pub retry_after_seconds: Option<u64>,
    /// Safe public message without provider, topology, credential, or stack details.
    pub message: &'static str,
    /// Correlation identity shared with the response header and server telemetry.
    pub request_id: aequora_types::RequestId,
}

impl ApiErrorEnvelope {
    fn json(self) -> String {
        let retry = self
            .retry_after_seconds
            .map_or_else(|| "null".to_owned(), |seconds| seconds.to_string());
        format!(
            "{{\"code\":\"{}\",\"retry_after_seconds\":{},\"message\":\"{}\",\"request_id\":\"{}\"}}",
            self.code.as_str(),
            retry,
            self.message,
            self.request_id
        )
    }
}

enum HttpError {
    UnsupportedMediaType,
    BadRequest(&'static str),
    BodyTooLarge,
    BodyReadTimedOut(u64),
    MissingAuthentication,
    AuthenticationRejected,
    AuthenticationUnavailable(u64),
    Overloaded(u64),
    TenantOverloaded(u64),
    TenantRateLimited(u64),
    Draining(u64),
    DeadlineExceeded(u64),
    Codec,
    Server(ServerError),
    WithRequestId(aequora_types::RequestId, Box<Self>),
}

impl HttpError {
    fn with_request_id(self, request_id: aequora_types::RequestId) -> Self {
        match self {
            Self::WithRequestId(_, _) => self,
            error => Self::WithRequestId(request_id, Box::new(error)),
        }
    }

    fn split_request_id(self) -> (aequora_types::RequestId, Self) {
        match self {
            Self::WithRequestId(request_id, error) => (request_id, *error),
            error => (aequora_types::RequestId::new(), error),
        }
    }
}

impl From<CodecError> for HttpError {
    fn from(_error: CodecError) -> Self {
        Self::Codec
    }
}

impl From<ServerError> for HttpError {
    fn from(error: ServerError) -> Self {
        Self::Server(error)
    }
}

impl IntoResponse for HttpError {
    #[allow(clippy::too_many_lines)]
    fn into_response(self) -> Response {
        let (request_id, error) = self.split_request_id();
        let (status, message, retry_after, code) = match error {
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported sync content type",
                None,
                OperationalErrorCode::Protocol,
            ),
            Self::BadRequest(message) => (
                StatusCode::BAD_REQUEST,
                message,
                None,
                OperationalErrorCode::Validation,
            ),
            Self::BodyTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "sync request body exceeds the configured wire limit",
                None,
                OperationalErrorCode::PayloadLimit,
            ),
            Self::BodyReadTimedOut(seconds) => (
                StatusCode::REQUEST_TIMEOUT,
                "sync request body exceeded its receive deadline",
                Some(seconds),
                OperationalErrorCode::Deadline,
            ),
            Self::MissingAuthentication => (
                StatusCode::UNAUTHORIZED,
                "authentication is required",
                None,
                OperationalErrorCode::Authentication,
            ),
            Self::AuthenticationRejected => (
                StatusCode::UNAUTHORIZED,
                "authentication was rejected",
                None,
                OperationalErrorCode::Authentication,
            ),
            Self::AuthenticationUnavailable(seconds) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "authentication service is temporarily unavailable",
                Some(seconds),
                OperationalErrorCode::Authentication,
            ),
            Self::Overloaded(seconds) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "sync server is at its in-flight request limit",
                Some(seconds),
                OperationalErrorCode::Overloaded,
            ),
            Self::TenantOverloaded(seconds) => (
                StatusCode::TOO_MANY_REQUESTS,
                "tenant is at its in-flight sync request limit",
                Some(seconds),
                OperationalErrorCode::Overloaded,
            ),
            Self::TenantRateLimited(seconds) => (
                StatusCode::TOO_MANY_REQUESTS,
                "tenant sync request rate limit exceeded",
                Some(seconds),
                OperationalErrorCode::Overloaded,
            ),
            Self::Draining(seconds) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "sync server is draining",
                Some(seconds),
                OperationalErrorCode::Draining,
            ),
            Self::DeadlineExceeded(seconds) => (
                StatusCode::GATEWAY_TIMEOUT,
                "sync request exceeded its server execution deadline",
                Some(seconds),
                OperationalErrorCode::Deadline,
            ),
            Self::Codec => (
                StatusCode::BAD_REQUEST,
                "sync protocol frame is invalid",
                None,
                OperationalErrorCode::Protocol,
            ),
            Self::Server(ServerError::Admission(rejection)) => {
                let status = match rejection {
                    aequora_admission::AdmissionRejection::TenantBusy { .. }
                    | aequora_admission::AdmissionRejection::RateLimited { .. } => {
                        StatusCode::TOO_MANY_REQUESTS
                    }
                    aequora_admission::AdmissionRejection::RequestTooLarge => {
                        StatusCode::PAYLOAD_TOO_LARGE
                    }
                    aequora_admission::AdmissionRejection::TooManyDependencies => {
                        StatusCode::BAD_REQUEST
                    }
                    _ => StatusCode::SERVICE_UNAVAILABLE,
                };
                let code = if rejection.retryable() {
                    OperationalErrorCode::Overloaded
                } else {
                    OperationalErrorCode::PayloadLimit
                };
                let retry_after = rejection
                    .retry_after_ms()
                    .map(|milliseconds| milliseconds.div_ceil(1_000).max(1));
                (status, "sync request was not admitted", retry_after, code)
            }
            Self::Server(ServerError::Validation(_)) => (
                StatusCode::BAD_REQUEST,
                "sync request failed protocol validation",
                None,
                OperationalErrorCode::Protocol,
            ),
            Self::Server(ServerError::Dependency(_)) => (
                StatusCode::BAD_REQUEST,
                "sync operation dependencies are invalid",
                None,
                OperationalErrorCode::Validation,
            ),
            Self::Server(ServerError::ResponseLimit) => (
                StatusCode::BAD_REQUEST,
                "client response limit is too small",
                None,
                OperationalErrorCode::PayloadLimit,
            ),
            Self::Server(ServerError::MemoryBudget { .. }) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "typed sync request exceeds the configured memory budget",
                None,
                OperationalErrorCode::PayloadLimit,
            ),
            Self::Server(ServerError::IdentityMismatch) => (
                StatusCode::UNAUTHORIZED,
                "authenticated identity mismatch",
                None,
                OperationalErrorCode::Authentication,
            ),
            Self::Server(ServerError::ScopeAuthorization(_)) => (
                StatusCode::FORBIDDEN,
                "sync scope is not authorized",
                None,
                OperationalErrorCode::Authentication,
            ),
            Self::Server(ServerError::Store(error)) if error.kind == StoreErrorKind::Transient => (
                StatusCode::SERVICE_UNAVAILABLE,
                "sync storage unavailable",
                None,
                OperationalErrorCode::Storage,
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
                "sync processing failed",
                None,
                OperationalErrorCode::Storage,
            ),
            Self::Server(ServerError::BootstrapUnavailable) => (
                StatusCode::NOT_IMPLEMENTED,
                "snapshot bootstrap is not available",
                None,
                OperationalErrorCode::Protocol,
            ),
            Self::Server(ServerError::Maintenance {
                retry_after_seconds,
                ..
            }) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "sync is temporarily unavailable due to maintenance",
                Some(retry_after_seconds),
                OperationalErrorCode::Maintenance,
            ),
            Self::Server(ServerError::Authority(_)) => (
                StatusCode::CONFLICT,
                "authority state rejected the sync request",
                None,
                OperationalErrorCode::Authority,
            ),
            Self::WithRequestId(_, _) => unreachable!("request identity wrapper is removed first"),
        };
        let envelope = ApiErrorEnvelope {
            code,
            retry_after_seconds: retry_after,
            message,
            request_id,
        };
        let mut response = (status, envelope.json()).into_response();
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(ERROR_CONTENT_TYPE));
        if let Some(value) =
            retry_after.and_then(|seconds| HeaderValue::from_str(&seconds.to_string()).ok())
        {
            response.headers_mut().insert(RETRY_AFTER, value);
        }
        response.headers_mut().insert(
            OPERATIONAL_ERROR_CODE_HEADER,
            HeaderValue::from_static(code.as_str()),
        );
        apply_public_response_headers(&mut response, request_id);
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_failures_expose_stable_codes_without_payloads() {
        let response = HttpError::Server(ServerError::Maintenance {
            mode: aequora_server::MaintenanceMode::SyncPaused,
            retry_after_seconds: 17,
        })
        .into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(OPERATIONAL_ERROR_CODE_HEADER),
            Some(&HeaderValue::from_static("AEQ-MAINT-001"))
        );
        assert_eq!(
            response.headers().get(RETRY_AFTER),
            Some(&HeaderValue::from_static("17"))
        );
    }

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
