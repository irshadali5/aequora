# Aequora Sync — Part 39

# Axum Server Integration, Middleware, Authentication, Request Pipeline, Backpressure, and HTTP Boundary Architecture

## 1. Purpose

Parts 34–38 established:

```text
workspace/crate boundaries
public Rust SDK
adapter SDK
PostgreSQL/Neon authoritative adapter
Stoolap local adapter
```

Part 39 defines the concrete HTTP server integration layer:

```text
aequora-axum
```

Its job is intentionally narrow.

> **Axum is the transport boundary around Aequora Server Core, not the location where synchronization or business semantics live.**

The server must remain usable through another transport later without rewriting:

```text
idempotency
validation
authorization
dependency planning
domain execution
journal
ledger
conflict semantics
```

---

# 2. Central Design Rule

The preferred dependency direction is:

```text
Axum
  |
  v
aequora-axum
  |
  v
aequora-server-core
  |
  +---- domain handlers
  |
  +---- authority adapter
  |
  +---- policy/security services
```

Never:

```text
domain handler
    |
    v
Axum extractor
```

and never:

```text
PostgreSQL adapter
    |
    v
HTTP request
```

---

# 3. Responsibilities of `aequora-axum`

`aequora-axum` owns:

```text
HTTP routes
request extraction
response construction
body-size enforcement
transport content negotiation
AuthContext construction
middleware composition
rate/admission integration
HTTP status mapping
CORS policy integration
CSRF policy where applicable
request IDs
tracing boundary
health/readiness endpoints
graceful shutdown integration
```

It should not own:

```text
business validation
domain rules
conflict resolution
journal mutations
ledger logic
database transactions
scope semantics
outbox semantics
```

---

# 4. Recommended Crate Layout

```text
aequora-axum/
├── src/
│   ├── lib.rs
│   ├── router.rs
│   ├── routes/
│   │   ├── exchange.rs
│   │   ├── bootstrap.rs
│   │   ├── health.rs
│   │   ├── readiness.rs
│   │   └── admin.rs
│   ├── extract/
│   │   ├── auth.rs
│   │   ├── tenant.rs
│   │   ├── request_id.rs
│   │   └── body.rs
│   ├── middleware/
│   │   ├── tracing.rs
│   │   ├── limits.rs
│   │   ├── admission.rs
│   │   ├── timeout.rs
│   │   ├── security_headers.rs
│   │   └── cors.rs
│   ├── response.rs
│   ├── errors.rs
│   ├── state.rs
│   └── shutdown.rs
└── Cargo.toml
```

---

# 5. Public Server Endpoints

Recommended v1 endpoints:

```text
POST /sync/v1/exchange
POST /sync/v1/bootstrap
GET  /sync/v1/health
GET  /sync/v1/ready
```

Potential:

```text
GET  /sync/v1/capabilities
```

Optional admin endpoints should live under a separate control-plane namespace:

```text
/admin/v1/...
```

---

# 6. Why `exchange` Is the Primary Sync Endpoint

The exchange request can carry:

```text
client hello/capabilities
pending operations
current cursors
scope state
device metadata
resource hints
```

The response can carry:

```text
operation outcomes
authoritative events
conflicts
server capabilities
cursor updates
retry hints
bootstrap instructions
```

This reduces excessive REST chatter and keeps synchronization semantics explicit.

---

# 7. Thin Route Handler

Desired shape:

```rust
async fn exchange(
    State(state): State<AppState>,
    auth: AuthContext,
    request_id: RequestId,
    body: BoundedBody,
) -> Result<impl IntoResponse, ApiError> {
    let request = decode_exchange(body)?;
    let response = state
        .sync_service
        .exchange(auth, request_id, request)
        .await?;

    encode_response(response)
}
```

The route should not implement:

```text
operation validation
idempotency
journal sequencing
conflict logic
```

---

# 8. Application State

Recommended:

```rust
pub struct AppState {
    pub sync_service: Arc<SyncService>,
    pub bootstrap_service: Arc<BootstrapService>,
    pub auth: Arc<dyn Authenticator>,
    pub admission: Arc<AdmissionController>,
    pub health: Arc<HealthService>,
}
```

Avoid a giant untyped service locator.

---

# 9. Router Construction

Concept:

```rust
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/sync/v1/exchange", post(exchange))
        .route("/sync/v1/bootstrap", post(bootstrap))
        .route("/sync/v1/health", get(health))
        .route("/sync/v1/ready", get(readiness))
        .with_state(state)
        .layer(server_layers())
}
```

---

# 10. Authentication Boundary

Authentication should happen before synchronization logic.

Axum extracts credentials and delegates to an authentication service.

Flow:

```text
HTTP request
↓
credential extraction
↓
Authenticator
↓
AuthenticatedPrincipal
↓
AuthContext
↓
server-core authorization
```

---

# 11. `AuthContext`

Canonical server-core type:

```rust
pub struct AuthContext {
    pub actor_id: ActorId,
    pub tenant_id: TenantId,
    pub device_id: Option<DeviceId>,
    pub session_id: Option<SessionId>,
    pub permissions: PermissionSet,
    pub auth_strength: AuthStrength,
}
```

The precise fields can evolve.

---

# 12. Do Not Pass Raw Bearer Tokens Downstream

Domain handlers should never receive:

```text
Authorization header
cookie string
JWT
OAuth token
```

They receive validated identity/context.

---

# 13. Authentication Providers

Potential:

```text
JWT/OIDC
session token
API key
device certificate
mTLS identity
custom enterprise SSO
```

All normalize into `AuthContext`.

---

# 14. Authorization

Authentication answers:

```text
who are you?
```

Authorization answers:

```text
may you perform this operation on this object/scope?
```

Authorization belongs primarily in server-core/domain policy, not just HTTP middleware.

---

# 15. Tenant Binding

Tenant identity must not be accepted from an arbitrary request body field without validation.

The server compares:

```text
authenticated tenant
requested tenant
operation tenant
scope tenant
```

and rejects mismatch.

---

# 16. Device Binding

If a sync operation claims:

```text
DeviceId
```

the authenticated session/device registration must permit that identity.

---

# 17. Request ID

Every incoming HTTP request should receive:

```text
RequestId
```

Use incoming trusted request ID only if validated; otherwise generate server-side.

---

# 18. Correlation vs Operation Identity

Keep separate:

```text
RequestId
CorrelationId
OperationId
```

One HTTP request can carry multiple operations.

---

# 19. Body Size Limits

Enforce hard bounds before decoding.

Example categories:

```text
exchange request
bootstrap request
admin request
```

Each can have a different maximum.

---

# 20. Why Pre-Decode Limits Matter

Postcard or any binary decoder can still be abused if given unbounded input.

Bound:

```text
HTTP body bytes
decompressed bytes
collection lengths
nested structures
operation count
dependency count
```

---

# 21. Compression

Optional:

```text
Content-Encoding: zstd
```

or another negotiated mechanism.

But enforce:

```text
compressed size limit
decompressed size limit
compression ratio limit
```

to prevent decompression bombs.

---

# 22. Content Type

Recommended internal protocol content type:

```text
application/x-aequora-postcard
```

or another explicit vendor media type.

Do not infer protocol purely from extension.

---

# 23. JSON Debug/Interoperability

Optional:

```text
application/json
```

may exist for debugging or public integrations.

It should not become the primary sync representation if Postcard is the chosen core protocol.

---

# 24. Protocol Decode Pipeline

```text
bounded raw bytes
↓
content encoding validation
↓
decompression
↓
Postcard frame decode
↓
protocol envelope validation
↓
version/capability negotiation
↓
canonical request
```

---

# 25. Decode Before Authentication?

Credential extraction may happen from headers before body decode.

But expensive body processing should happen only after cheap rejection opportunities where practical.

---

# 26. Recommended Ordering

```text
connection accepted
↓
basic header validation
↓
request size check
↓
authentication
↓
admission check
↓
body read/decompress/decode
↓
protocol validation
↓
server core
```

Exact order can vary while preserving security and resource limits.

---

# 27. Admission Control

Part 18 applies at HTTP ingress.

Admission decides whether the server has capacity for the work.

Possible result:

```text
Admitted
RateLimited
ServerBusy
TenantBusy
ResourceBudgetExceeded
```

---

# 28. Admission Before DB Pool Acquisition

Do not allow every request to pile up waiting for PostgreSQL.

Use bounded resource permits.

---

# 29. Hierarchical Admission

Concept:

```text
global
↓
tenant
↓
work class
↓
resource
```

Examples:

```text
interactive sync
bulk bootstrap
admin operation
background change feed
```

---

# 30. Work Classification

HTTP route derives an initial class:

```text
Exchange
Bootstrap
Health
Admin
```

Server-core may refine cost classification based on decoded request.

---

# 31. Rate Limiting

Rate limits can apply by:

```text
IP
tenant
actor
device
API credential
route
```

Do not rely on IP alone for authenticated multi-device systems.

---

# 32. Retry-After

For bounded temporary rejection:

```text
429 Too Many Requests
503 Service Unavailable
```

include a server retry hint where useful.

Client scheduler applies jitter.

---

# 33. HTTP Status Is Not Full Semantic Result

A sync request can return HTTP 200 while individual operations contain:

```text
accepted
rejected
conflict
superseded
```

These are application/sync-level outcomes.

---

# 34. Transport Error Mapping

Examples:

```text
malformed body             -> 400
unauthenticated             -> 401
authenticated but denied    -> 403
unsupported media type      -> 415
body too large              -> 413
rate limited                -> 429
server overloaded           -> 503
internal unexpected failure -> 500
```

---

# 35. Compatibility Errors

Examples:

```text
client too old
required capability absent
unsupported protocol version
```

May return:

```text
409
426
400
```

depending chosen API convention.

More important than status code is a stable Aequora error code in the response body.

---

# 36. Stable Error Envelope

Concept:

```rust
pub struct ApiErrorEnvelope {
    pub code: ErrorCode,
    pub retry: RetryClass,
    pub message: String,
    pub request_id: RequestId,
    pub details: Option<SafeErrorDetails>,
}
```

Do not expose stack traces or raw SQL errors.

---

# 37. Panic Handling

Unexpected panic should:

```text
be logged
return sanitized 500 if connection still usable
```

but Aequora must not depend on panic recovery for correctness.

Expected failures use typed `Result`.

---

# 38. Tower Middleware

Axum/Tower is appropriate for generic HTTP concerns:

```text
timeouts
request IDs
tracing
body limits
compression
CORS
security headers
```

Do not push business authorization into generic middleware if it requires operation/entity semantics.

---

# 39. Middleware Ordering

Ordering matters.

Recommended conceptual order:

```text
request ID
↓
connection/request tracing
↓
security headers / protocol checks
↓
body coarse limit
↓
authentication
↓
rate/admission
↓
route
↓
server core
```

Response mapping occurs outward through layers.

---

# 40. Timeout Architecture

Use multiple timeouts, not one giant request timeout.

Possible:

```text
header/body receive timeout
authentication timeout
admission wait timeout
DB transaction timeout
response send timeout
```

---

# 41. Cancellation

When client disconnects:

```text
HTTP future may be cancelled
```

but an authoritative transaction may already have committed.

Therefore operation identity/idempotency remains the recovery mechanism.

---

# 42. Never Tie Correctness to Response Delivery

Correct flow:

```text
commit authority
↓
attempt response delivery
```

If delivery fails, client retries same OperationId.

---

# 43. Graceful Shutdown

Server shutdown sequence:

```text
stop accepting new connections
↓
mark readiness false
↓
drain bounded in-flight HTTP requests
↓
stop background workers
↓
close DB pools/resources
```

---

# 44. Shutdown Timeout

Use bounded drain.

Do not wait forever.

Any interrupted request recovers through transaction/idempotency semantics.

---

# 45. Liveness Endpoint

`GET /sync/v1/health`

Should answer whether:

```text
process is alive
```

It should be cheap and not depend on every external provider.

---

# 46. Readiness Endpoint

`GET /sync/v1/ready`

Checks whether server can safely accept synchronization work:

```text
Postgres available
schema compatible
authority writable
critical migrations complete
required capabilities loaded
```

---

# 47. Readiness During Migration

For incompatible migration state:

```text
ready = false
```

---

# 48. Readiness During Degraded Optional Dependency

If an optional analytics provider is down but sync authority is healthy:

```text
ready may remain true
```

depending dependency classification.

---

# 49. Health Detail Security

Public health endpoint should not leak:

```text
DB host
schema names
secrets
internal topology
```

Detailed diagnostics belong in authenticated admin interfaces.

---

# 50. Bootstrap Endpoint

`POST /sync/v1/bootstrap`

Responsibilities:

```text
authenticate
authorize requested scope
validate compatibility
locate/build snapshot manifest
return signed/verified bootstrap metadata
```

Large chunk transfer may use:

```text
object storage URLs
streamed HTTP response
```

depending deployment.

---

# 51. Snapshot Artifact Authorization

Pre-signed URLs or artifact tokens should be:

```text
short-lived
scope-bound
tenant-bound
```

where relevant.

---

# 52. Large Response Streaming

Avoid buffering entire large bootstrap body in server memory.

Use streaming.

---

# 53. Exchange Response Size

Keep bounded.

If more journal events remain:

```text
has_more = true
```

client performs another exchange.

---

# 54. Pagination by Cursor

Never use HTTP page number as sync ordering state.

Use Aequora cursor.

---

# 55. CORS

For native desktop/mobile clients, CORS is irrelevant to non-browser HTTP clients.

For browser integrations, configure explicit origins according to Part 34/possible browser architecture.

---

# 56. Avoid `*` with Credentials

Never combine:

```text
Access-Control-Allow-Origin: *
```

with credentialed browser security assumptions.

---

# 57. CSRF

Bearer-token native clients generally have different CSRF risks than cookie-authenticated browser clients.

If cookie auth exists:

```text
SameSite
CSRF token/origin validation
```

must be designed.

---

# 58. Security Headers

For browser-facing admin/web surfaces consider:

```text
Content-Security-Policy
X-Content-Type-Options
Referrer-Policy
frame restrictions
HSTS at deployment layer
```

The binary sync API itself has fewer browser-rendering concerns.

---

# 59. TLS

Production sync should use:

```text
HTTPS
```

Axum can terminate TLS directly, but deployment may instead use:

```text
reverse proxy
load balancer
service mesh ingress
```

---

# 60. Trusted Proxy Model

If behind proxy:

```text
client IP
scheme
host
```

headers must only be trusted from configured proxies.

---

# 61. `X-Forwarded-*`

Never trust forwarded headers from arbitrary internet clients.

---

# 62. Request Host Validation

Where relevant, validate expected host/domain to reduce proxy confusion and abuse.

---

# 63. HTTP/2 and HTTP/3

HTTP/2 is a natural production default.

HTTP/3/QUIC may be added later through another transport implementation.

Do not couple sync semantics to HTTP version.

---

# 64. Keepalive

Tune connection keepalive operationally.

The protocol remains request/response and retry-safe.

---

# 65. WebSocket

WebSocket may be used for:

```text
live sync hints
presence
```

from Part 08.

It should not replace durable exchange/journal semantics.

---

# 66. SSE

Also acceptable for one-way sync hints.

Same rule:

```text
hint channel != source of truth
```

---

# 67. Push Hint Route

If needed:

```text
GET /sync/v1/live
```

or a separate endpoint.

The live channel sends tiny advisory messages.

---

# 68. Server-Side Request Context

Canonical context may contain:

```rust
pub struct RequestContext {
    pub request_id: RequestId,
    pub auth: AuthContext,
    pub remote_class: RemoteClass,
    pub received_at: ServerInstant,
    pub protocol: NegotiatedProtocol,
}
```

Avoid passing `axum::http::Request` into domain code.

---

# 69. Remote Address

Remote address can aid abuse detection.

It is not identity.

---

# 70. User-Agent / Client Build

Client sends structured build metadata in protocol hello rather than relying only on generic HTTP User-Agent.

---

# 71. Capability Negotiation

Part 21 occurs inside the decoded sync protocol:

```text
ClientHello
↓
server compatibility policy
↓
SessionProfile
```

HTTP itself should remain a transport shell.

---

# 72. Minimum Client Policy

If client build/protocol is unsupported:

```text
return typed compatibility error
```

with:

```text
upgrade required
rebootstrap required
```

where appropriate.

---

# 73. Operation Count Limit

An exchange request must have hard bounds on:

```text
number of operations
dependency edges
scope cursors
requested entities
```

---

# 74. Cost Estimation

After decode, compute a bounded work cost estimate.

Examples:

```text
operation count
payload bytes
dependency complexity
expected DB work
```

Admission may reject costly bulk work under load.

---

# 75. Dependency DAG Abuse

Reject:

```text
too many nodes
too many edges
cycles
pathological depth
```

before expensive planning.

---

# 76. Validation Pipeline

Recommended:

```text
transport validity
↓
authentication
↓
protocol compatibility
↓
tenant/device binding
↓
schema decode/upcast
↓
authorization
↓
domain validation
↓
dependency planning
↓
execution
```

---

# 77. Typestate Boundary

Server-core may model:

```text
Incoming
→ Authenticated
→ Authorized
→ Validated
→ Executable
```

Axum only produces the initial canonical input and AuthContext.

---

# 78. Application Business Routes

School ERP or another product may expose ordinary REST/GraphQL endpoints.

Those writes must route into the same domain operation handlers as sync.

Never maintain:

```text
REST business logic
```

and separate:

```text
sync business logic
```

with divergent rules.

---

# 79. Example Unified Write

REST:

```text
POST /students
↓
CreateStudent operation
↓
same DomainHandler
```

Sync:

```text
ExchangeRequest(CreateStudent)
↓
same DomainHandler
```

---

# 80. REST Response vs Sync Outcome

Transport representation can differ while semantic execution is identical.

---

# 81. Idempotency for Ordinary REST

If a REST route invokes durable operations, consider requiring/generated idempotency key mapped to `OperationId`.

---

# 82. Admin API Separation

Admin/control-plane routes should use:

```text
separate router
separate auth policy
possibly separate listener
```

for high-assurance deployments.

---

# 83. Admin Listener

Possible:

```text
127.0.0.1/private network only
mTLS
VPN
```

depending deployment.

---

# 84. No Arbitrary SQL Admin Endpoint

Never expose:

```text
POST /admin/sql
```

Typed control-plane actions only.

---

# 85. Step-Up Authentication

Sensitive actions may require:

```text
recent MFA
hardware-backed credential
two-person approval
```

according to Part 24.

---

# 86. Tracing

Create one root span per HTTP request.

Useful fields:

```text
request_id
route
method
tenant opaque identifier
actor opaque identifier
device opaque identifier
protocol version
status
latency
```

---

# 87. Operation Spans

Within exchange, create child spans by:

```text
OperationId
OperationKind
```

without logging sensitive payloads.

---

# 88. Trace Sampling

High-volume success requests may use sampling.

Errors/security events should receive stronger retention according to policy.

---

# 89. Metrics

HTTP-level metrics:

```text
requests_total
request_duration
request_bytes
response_bytes
auth_failures
rate_limited
admission_rejected
body_too_large
decode_failures
```

Server-core/database metrics are separate.

---

# 90. Metric Cardinality

Never put raw:

```text
OperationId
EntityId
UserId
```

into metric labels.

---

# 91. Structured Logs

Log:

```text
error code
request ID
safe operation metadata
```

Do not dump entire operation payloads.

---

# 92. Privacy

Headers such as authorization tokens must be redacted automatically.

---

# 93. Error Reporting

External error monitoring can receive:

```text
sanitized stack/error context
```

subject to governance.

---

# 94. HTTP Access Logging

Prefer structured tracing rather than duplicating multiple access log systems.

---

# 95. Reverse Proxy Deployment

Typical:

```text
Internet
↓
Cloudflare / LB / reverse proxy
↓
Axum
↓
Aequora Server Core
↓
PostgreSQL/Neon
```

---

# 96. Direct Axum Deployment

Also valid:

```text
native TLS
Axum listener
```

for simpler environments.

---

# 97. Load Balancing

Aequora HTTP nodes should be stateless with respect to correctness-critical request state.

---

# 98. Sticky Sessions

Should not be required for ordinary sync exchange.

---

# 99. Shared Server State

Durable coordination belongs in:

```text
PostgreSQL
object storage
broker when optional
```

not in one process's memory.

---

# 100. Live Connections

WebSocket presence/hints may require connection-local state, but correctness does not.

---

# 101. Horizontal Scaling

Multiple Axum nodes can accept the same device's retries because:

```text
OperationId
+
Postgres ledger
```

provides idempotency.

---

# 102. Node Identity

Each server process may have:

```text
ServerInstanceId
```

for logs/leases/diagnostics.

It is not AuthorityId.

---

# 103. Graceful Node Replacement

Because authority lives in Postgres:

```text
drain node
terminate
start new node
```

does not change authority epoch.

---

# 104. Authority Failover

AuthorityEpoch changes are about authoritative timeline continuity, not ordinary Axum process restarts.

---

# 105. Server Configuration

Suggested RON shape:

```ron
http: (
    bind: "0.0.0.0:8443",

    limits: (
        exchange_body_bytes: 4194304,
        bootstrap_body_bytes: 1048576,
        max_operations: 512,
    ),

    timeout: (
        request_ms: 15000,
        body_ms: 5000,
    ),

    admission: (
        global_inflight: 1024,
        per_tenant_inflight: 64,
    ),
)
```

Secrets do not belong directly in RON.

---

# 106. Dynamic Configuration

Some operational limits may reload dynamically.

Correctness-critical changes require controlled validation.

---

# 107. Config Generation

Track:

```text
ConfigGeneration
```

in diagnostics when dynamic config is supported.

---

# 108. Server Start Sequence

```text
load config
↓
initialize tracing
↓
open authority store
↓
run/verify migrations
↓
load registry
↓
validate handler registry
↓
initialize auth
↓
initialize admission
↓
build server core
↓
build Axum router
↓
mark ready
↓
serve
```

---

# 109. Startup Failure

Fail startup if:

```text
required handler missing
registry incompatible
DB unavailable
critical migration failed
auth configuration invalid
required capability absent
```

---

# 110. Server Shutdown Sequence

```text
readiness false
↓
stop accepting new work
↓
drain HTTP
↓
stop sync/live workers
↓
flush bounded telemetry
↓
close DB pool
```

---

# 111. Zero-Downtime Deployment

Recommended rolling sequence:

```text
deploy compatible schema
↓
start new nodes
↓
readiness passes
↓
shift traffic
↓
drain old nodes
```

---

# 112. Compatibility Window

Old and new nodes must understand overlapping protocol/schema versions during rollout.

---

# 113. Blue-Green

Also valid where stronger rollback isolation is desired.

---

# 114. Canary

Can route small percentage of traffic to new build.

Do not split authority semantics; all nodes still use same authoritative DB/timeline.

---

# 115. HTTP Testing

Use:

```text
router-level tests
real server-core integration
property/fuzz decode tests
security tests
load tests
```

---

# 116. Route Unit Test

Test:

```text
status mapping
headers
body limits
auth extraction
```

without requiring full domain stack where possible.

---

# 117. End-to-End Test

Run:

```text
client
→ real Axum
→ server core
→ real PostgreSQL
```

for critical sync flows.

---

# 118. Malformed Postcard Test

Feed:

```text
truncated bytes
oversized lengths
unknown message kind
invalid version
```

Expected:

```text
bounded typed rejection
```

not panic/OOM.

---

# 119. Slowloris Test

Ensure header/body timeouts and server limits prevent resource exhaustion.

---

# 120. Compression Bomb Test

Compressed small input expanding beyond configured limit must abort.

---

# 121. Authentication Abuse Test

Test:

```text
invalid token flood
expired token
cross-tenant token
revoked device
```

---

# 122. Authorization Test

Authenticated actor attempts unauthorized entity/scope operation.

Must fail before authoritative mutation.

---

# 123. Disconnect Test

Disconnect client:

```text
before Tx
during Tx
after commit
before response
```

Retry must remain safe.

---

# 124. Overload Test

Exceed:

```text
global permits
tenant permits
DB pool
```

Expected:

```text
bounded 429/503
```

not cascading failure.

---

# 125. Rolling Upgrade Test

Run old/new Axum nodes concurrently against one Postgres schema within compatibility window.

---

# 126. Proxy Test

Verify trusted proxy configuration and forwarded-header handling.

---

# 127. CORS/CSRF Test

Required only for browser-facing deployment profiles.

---

# 128. Security Fuzzing

Fuzz:

```text
HTTP-to-protocol boundary
header parser assumptions
content encoding
Postcard envelope
```

---

# 129. Axum Integration Invariants

## AEQ-INV-AXUM001

```text
Axum routes contain transport orchestration only; domain and synchronization correctness remain in transport-neutral server-core.
```

## AEQ-INV-AXUM002

```text
Raw authentication credentials never reach domain handlers or storage adapters.
```

## AEQ-INV-AXUM003

```text
HTTP request bodies, decompressed bodies, operation counts, and dependency graphs are bounded before expensive processing.
```

## AEQ-INV-AXUM004

```text
Client disconnect or response-delivery failure cannot invalidate an already committed authoritative operation.
```

## AEQ-INV-AXUM005

```text
Temporary overload is rejected with bounded retry semantics before causing unbounded memory, task, or database-pool growth.
```

---

# 130. Additional Invariants

## AEQ-INV-AXUM006

```text
Tenant and device identity in protocol payloads must be validated against authenticated identity before execution.
```

## AEQ-INV-AXUM007

```text
HTTP status codes are transport-level indicators and never replace stable Aequora operation/error semantics.
```

## AEQ-INV-AXUM008

```text
Missed WebSocket/SSE/live hints cannot cause missed durable state because normal cursor exchange remains authoritative.
```

## AEQ-INV-AXUM009

```text
Ordinary Axum node restart, scaling, or replacement does not create a new AuthorityEpoch.
```

## AEQ-INV-AXUM010

```text
Detailed internal database, topology, credential, and stack-trace information is never exposed through public HTTP error responses.
```

---

# 131. Suggested Rust API

Conceptual:

```rust
pub struct AxumIntegration {
    router: Router,
}

impl AxumIntegration {
    pub fn new(server: AequoraServer) -> Self {
        // compose routes and middleware
    }

    pub fn router(self) -> Router {
        self.router
    }
}
```

---

# 132. Authentication Extractor

Concept:

```rust
pub struct AuthContextExtractor(pub AuthContext);
```

Its implementation:

```text
extract credential
↓
authenticate
↓
load device/session policy
↓
construct canonical AuthContext
```

---

# 133. Request Body Wrapper

Concept:

```rust
pub struct BoundedProtocolBody(Bytes);
```

The type should only be constructible after configured size enforcement.

---

# 134. Protocol Codec Boundary

```rust
fn decode_exchange(
    body: BoundedProtocolBody,
) -> Result<ExchangeRequest, ApiError>;
```

Codec implementation remains protocol-specific but HTTP-independent where practical.

---

# 135. Error Conversion

Use explicit conversions:

```text
AuthError
AdmissionError
ProtocolError
ServerError
```

to:

```text
ApiError
```

Avoid one catch-all conversion that maps everything to 500.

---

# 136. Response Encoding

```text
canonical server response
↓
protocol encode
↓
optional compression
↓
HTTP response
```

Encoding failure after commit is handled like response-delivery failure: client retries safely.

---

# 137. Health State Separation

Recommended states:

```text
Alive
Ready
Degraded
Draining
```

Only `Ready` accepts normal synchronization traffic.

---

# 138. Draining Behavior

During graceful shutdown:

```text
ready = false
```

new load balancer traffic should stop.

Existing requests finish within drain limit.

---

# 139. Admin Control Plane

Recommended separate construction:

```rust
pub fn admin_router(
    control_plane: ControlPlane,
    policy: AdminHttpPolicy,
) -> Router;
```

---

# 140. Public vs Private Listener

Possible production topology:

```text
0.0.0.0:443
    public sync API

127.0.0.1:9443 / private subnet
    admin API
```

---

# 141. Native Client Session Security

For Dioxus desktop/mobile:

```text
TLS
short-lived access credentials
refresh/session mechanism
device registration
```

Avoid baking long-lived secrets into application binaries.

---

# 142. Device Credential Rotation

HTTP auth integration should support:

```text
expired credential
rotated credential
revoked device
```

without changing DeviceId unnecessarily.

---

# 143. Replay Protection

Operation replay is handled by OperationId/ledger.

Authentication token replay mitigation remains an auth-system concern.

---

# 144. Clock Trust

Do not rely on client HTTP `Date` header.

Server time comes from server-controlled clock/authority context.

---

# 145. HTTP Cache Headers

Sync responses should generally prevent intermediary caching unless specifically designed.

Use appropriate:

```text
Cache-Control
```

for dynamic authenticated responses.

---

# 146. CDN

CDN can safely serve immutable:

```text
snapshot chunks
blob artifacts
public static content
```

when authorization model permits.

Do not CDN-cache personalized sync exchange responses.

---

# 147. Request Replay by Proxy

Even if an intermediary retries a POST unexpectedly, OperationId idempotency protects authoritative effects.

---

# 148. Idempotency Scope

Transport retry safety depends on semantic OperationId, not only HTTP request identity.

---

# 149. Multiple Operations per Request

If the same request is retried:

```text
each operation independently resolves through ledger
```

The response can be reconstructed from authoritative state/outcomes.

---

# 150. Response Partial Failure

Avoid emitting partially encoded response streams for normal exchange unless protocol explicitly supports framing/recovery.

For bounded exchange responses, encode atomically in memory within configured size budget.

Large bootstrap artifacts use streaming separately.

---

# 151. Memory Budget

Bound:

```text
request body
decoded request
response batch
compression buffer
per-request temporary allocations
```

---

# 152. `Bytes`

Use `bytes::Bytes` or borrowed slices where useful to reduce copies, while respecting lifetime safety and decode ownership boundaries.

---

# 153. Postcard Zero-Copy

Borrowed decoding can be used only while the backing buffer remains stable.

Convert to owned canonical domain data before storing beyond request lifetime.

---

# 154. CPU-Heavy Decode/Compression

If large enough to block async runtime:

```text
bounded blocking/CPU pool
```

may be used.

Do not spawn unbounded Rayon work per request.

---

# 155. Tokio Task Ownership

The route future should not create detached correctness-critical tasks.

Durable background work belongs in the job subsystem.

---

# 156. Fire-and-Forget Warning

Never:

```rust
tokio::spawn(async move {
    perform_authoritative_mutation().await;
});
return StatusCode::ACCEPTED;
```

unless the operation was first durably enqueued into an intentional background workflow.

---

# 157. 202 Accepted

Use only for a defined durable asynchronous job contract.

Normal sync exchange should return authoritative operation outcomes when processed synchronously.

---

# 158. Maintenance Mode

Server can reject mutation traffic with:

```text
Maintenance
RetryAfter
```

while perhaps preserving health/admin access.

---

# 159. Read-Only Mode

If authority is intentionally read-only:

```text
pull may be allowed
push rejected
```

only if explicitly supported by server policy.

---

# 160. Degraded Mode

Optional noncritical features can be disabled:

```text
live hints
analytics
large exports
```

without weakening core sync security/correctness.

---

# 161. Public API Version Prefix

Use:

```text
/sync/v1/
```

for HTTP route versioning.

Remember:

```text
HTTP route version
!=
protocol schema version
```

The binary protocol still negotiates its own versions.

---

# 162. Why Both Versions Exist

Route version handles:

```text
HTTP surface shape
```

Protocol version handles:

```text
wire semantic compatibility
```

They can evolve independently.

---

# 163. Documentation

Part 39 implementation docs should include:

```text
route catalog
authentication integration
middleware order
proxy deployment
error mapping
limits
example Axum composition
```

---

# 164. Reference Integration Example

```rust
let authority = PostgresAuthorityStore::connect(pg_config).await?;

let server = AequoraServer::builder()
    .authority_store(authority)
    .domain(domain_registry)
    .authenticator(authenticator.clone())
    .build()?;

let app = aequora_axum::router(
    server,
    AxumConfig::default(),
);

let listener = tokio::net::TcpListener::bind(addr).await?;

axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await?;
```

---

# 165. Product Server Composition

Example School ERP:

```text
Public Router
├── /sync/v1/*
├── /api/v1/students/*
├── /api/v1/fees/*
└── /api/v1/auth/*

All durable business writes
        |
        v
same domain operation registry
        |
        v
Aequora Server Core
```

---

# 166. Avoid Duplicate Business Semantics

Bad:

```text
REST CreateStudent SQL
+
sync CreateStudent handler
```

Good:

```text
REST CreateStudent
      \
       → CreateStudentOperation → shared handler
      /
Sync CreateStudent
```

---

# 167. Future QUIC Integration

Part 39 intentionally preserves:

```text
aequora-server-core
```

so a future:

```text
aequora-quic
```

can call the same service.

---

# 168. Future gRPC

Likewise possible for enterprise interoperability.

Do not let protobuf/gRPC types become canonical domain types.

---

# 169. Local IPC Server

The desktop `aequora-agent` may expose IPC using the same architectural principle:

```text
IPC framing
↓
canonical SDK/service request
↓
client runtime
```

---

# 170. HTTP Boundary Threat Model

Primary threats:

```text
credential theft
request floods
cross-tenant access
oversized bodies
compression bombs
malformed binary payloads
slowloris
header spoofing
proxy confusion
protocol downgrade
resource amplification
```

Part 39 mitigates these before they reach domain execution.

---

# 171. Security Review Checklist

```text
[ ] TLS deployment defined
[ ] trusted proxy list explicit
[ ] body limits active
[ ] decompression limits active
[ ] auth before expensive work where practical
[ ] tenant binding checked
[ ] device binding checked
[ ] stable error sanitization
[ ] rate/admission limits
[ ] no token logging
[ ] malformed Postcard fuzzed
[ ] admin routes separated
```

---

# 172. Performance Review Checklist

```text
[ ] bounded request allocation
[ ] bounded response allocation
[ ] no blocking DB call on Tokio reactor
[ ] pool wait measured
[ ] middleware overhead measured
[ ] compression threshold measured
[ ] operation batch limit measured
[ ] live connections isolated from sync requests
```

---

# 173. Deployment Review Checklist

```text
[ ] readiness configured
[ ] graceful shutdown configured
[ ] proxy/TLS behavior verified
[ ] connection pool sized
[ ] admission limits configured
[ ] health endpoints protected appropriately
[ ] rolling compatibility tested
```

---

# 174. Completion Criteria

Part 39 is complete when:

```text
[ ] aequora-axum responsibilities defined
[ ] route set defined
[ ] AuthContext extraction defined
[ ] tenant/device binding defined
[ ] body/decompression limits defined
[ ] protocol decode pipeline defined
[ ] admission/backpressure defined
[ ] HTTP error mapping defined
[ ] timeout/cancellation semantics defined
[ ] health/readiness defined
[ ] bootstrap/live integration defined
[ ] REST/shared-domain-handler rule defined
[ ] observability defined
[ ] proxy/TLS security defined
[ ] graceful shutdown defined
[ ] horizontal scaling defined
[ ] tests/fuzzing defined
[ ] AXUM invariants defined
```

---

# 175. Final Architecture

```text
                    Native / Web Client
                            |
                          HTTPS
                            |
                            v
                     Axum / Tower
          +-----------------+------------------+
          |                 |                  |
          v                 v                  v
      AuthContext      Limits/Admission     Tracing
          |                 |                  |
          +-----------------+------------------+
                            |
                            v
                    Protocol Decode
                            |
                            v
                 Aequora Server Core
          +-----------------+------------------+
          |                 |                  |
          v                 v                  v
       AuthZ             Planner           Domain
                                               |
                                               v
                                   PostgreSQL Authority Tx
                                               |
                             +-----------------+----------------+
                             |                 |                |
                             v                 v                v
                          Journal           Ledger            Audit
```

---

# 176. Final Recommendation

Keep `aequora-axum` deliberately thin.

The HTTP layer should be very good at:

```text
accepting requests
authenticating
bounding resources
decoding protocol
mapping errors
observing traffic
```

and deliberately bad at owning business semantics because it should own none.

The key principle is:

> **Aequora should be able to replace Axum with another transport without changing how an operation is authenticated, authorized, validated, executed, journaled, deduplicated, or reconciled.**

That separation makes Axum a robust production transport rather than an architectural dependency of the synchronization engine.
