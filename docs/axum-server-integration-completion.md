# Part 39 Axum server integration completion

The governing authority is
[`sys-arch/39-axum-server-integration-middleware-architecture.md`](../sys-arch/39-axum-server-integration-middleware-architecture.md).
This report records executable repository evidence and does not replace that design.

## Implemented surface

| Requirement | Executable evidence |
|---|---|
| Thin transport boundary | `aequora-axum` depends on `ExchangeService` and canonical protocol/core types. `aequora-server`, `aequora-executor`, and `aequora-store` have no Axum, Tower, HTTP request, or bearer-header dependency. |
| Public routes | The router owns `POST /sync/v1/exchange`, `POST /sync/v1/bootstrap`, `GET /sync/v1/health`, and canonical `GET /sync/v1/ready`. Compatibility aliases `/sync/v1/health/live` and `/sync/v1/health/ready` remain available. |
| Authentication | `HttpAuthenticator` consumes a bounded, debug-redacted `PresentedCredential` and returns only `AuthContext`. Authentication runs before admission and body decode. Existing host middleware can continue injecting `Extension<AuthContext>` during migration. |
| Identity binding | Server core validates session, operation, tenant, actor, device, and scope claims against the authenticated context before authoritative execution. Raw authorization headers are not passed into that boundary. |
| Bounded request pipeline | `DefaultBodyLimit`, incremental framed-body reads, header-declared length checks, receive deadline, compressed and decompressed byte limits, codec collection limits, server protocol limits, and server memory budgets reject work before expensive execution. |
| Admission and overload | Global in-flight, per-tenant in-flight, per-tenant token bucket, bounded tenant tracking, request deadline, readiness deadline, and `Retry-After` mapping prevent unbounded task and pool queues. Permits release on completion, cancellation, decode failure, or timeout. |
| Error and response safety | `ApiErrorEnvelope` returns stable `OperationalErrorCode`, retry hint, sanitized message, and `RequestId`. All public responses receive `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, and `x-request-id`. Provider, SQL, topology, credential, and stack details are not returned. |
| Compression and framing | Framed Postcard is explicit as `application/vnd.aequora.postcard`; zstd response compression requires negotiated client capability. Wire and decompressed limits are independent. |
| Health and shutdown | Liveness is independent of external providers. An application-owned bounded `ReadinessProbe` controls readiness. `ServerLifecycle` atomically enters draining, rejects new sync work, waits for admitted work with a deadline, and exposes exact drain outcome. |
| Horizontal scaling and delivery | Router state is correctness-stateless. Durable authority and `OperationId` idempotency remain below HTTP, so disconnect, retry, node restart, scaling, or replacement does not create another logical effect or authority epoch. |
| Bootstrap and live hints | Bootstrap uses a distinct framed request/response kind and bounded page semantics. WebSocket/SSE routes remain an application composition concern over `aequora-live`; they are advisory only and normal cursor exchange remains authoritative. |
| Admin and browser policy | The public router contains no admin route. Applications compose the Part 24 control plane on a separate router/listener. CORS and cookie-CSRF policy are deployment-owned and must use explicit origins; the native bearer sync router does not enable permissive CORS. |
| Invariants and certification | `AEQ-INV-AXUM001`–`010`, `AxumServerFull`, and conformance tests 79–88 are present in the invariant registry, canonical RON registry, and generated Rust/Markdown snapshots. |
| Structural enforcement | `scripts/check-axum-server-architecture.sh` verifies artifacts, routes, limits, symbols, invariant parity, transport-neutral dependency direction, focused tests, and registry integrity. CI runs this gate. |

## Request order and deployment boundary

The executable extractor order is request identity, authentication, global/tenant/rate admission,
bounded body receive, framed decompression/decode, protocol validation, and server-core execution.
Response construction adds stable error semantics, retry information, correlation, cache prevention,
and security headers.

Production hosts terminate TLS either directly or at a trusted ingress. `aequora-axum` does not
interpret `X-Forwarded-*` from arbitrary clients. A trusted ingress must strip untrusted forwarded
metadata before application-specific connection classification. Incoming `x-request-id` is ignored
by default and is accepted only when `trust_request_id_header` is explicitly enabled behind such an
ingress. TLS configuration, allowed hosts, explicit browser origins, cookie CSRF controls, and a
private admin listener remain deployment policy rather than synchronization semantics.

## Verification contract

The focused gate is:

```bash
bash scripts/check-axum-server-architecture.sh
```

Repository completion also requires formatting, strict Clippy, Rust 1.87 all-target/all-feature
checking, warning-denied Rustdoc, locked/offline workspace tests, Guppy workspace architecture,
database-neutrality, registry verification, and diff hygiene.

Live PostgreSQL/Neon end-to-end flows, slowloris behavior at the selected TLS/proxy, browser
CORS/CSRF policy, rolling mixed-version deployment, fuzzing, load/capacity targets, and disconnect
timing around a real database commit remain environment-bound acceptance evidence. The repository
does not claim those unexecuted deployment results.

## Completed verification

Verified on 2026-09-01 with `CARGO_BUILD_JOBS=1`:

- the focused Part 39 architecture gate, Axum route/auth/limit/lifecycle tests, and registry gate;
- the complete locked/offline all-feature workspace suite after permitting its localhost HTTP test;
- strict workspace Clippy with warnings denied;
- Rust 1.87 all-target/all-feature checking and warning-denied workspace Rustdoc;
- Guppy and workspace boundaries (22 rules, 86 crates, 9 layers), plus all database-neutral
  composition profiles; and
- formatting, generated registry integrity, and diff hygiene.

The initial sandboxed workspace suite could not bind its HTTP loopback listener; the exact test and
the complete permitted rerun passed. The post-refactor semantic index refresh was attempted with a
writable temporary model cache, but was stopped at 85/537 files when GraphRAG processing became
resource-intensive; no repository-wide index-freshness claim is made.
