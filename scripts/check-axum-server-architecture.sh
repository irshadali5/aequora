#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-axum/Cargo.toml
    crates/aequora-axum/src/lib.rs
    crates/aequora-axum/tests/http_exchange.rs
    docs/axum-server-integration-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 39 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-AXUM${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-AXUM${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-AXUM${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-AXUM${suffix}" \
        sys-arch/39-axum-server-integration-middleware-architecture.md
done

for symbol in \
    AxumConfig HttpAuthenticator PresentedCredential RequestContext ApiErrorEnvelope \
    ReadinessProbe ServerLifecycle DrainOutcome router_with_authenticator; do
    rg -q "\b${symbol}\b" crates/aequora-axum/src/lib.rs
done

for route in \
    /sync/v1/exchange /sync/v1/bootstrap /sync/v1/health /sync/v1/ready; do
    rg -q "${route}" crates/aequora-axum/src/lib.rs
done

for boundary in \
    max_body_bytes max_decompressed_bytes body_read_timeout max_in_flight_requests \
    max_in_flight_per_tenant max_rate_limit_tenants request_timeout readiness_timeout \
    max_credential_bytes authentication_timeout; do
    rg -q "\b${boundary}\b" crates/aequora-axum/src/lib.rs
done

for header in REQUEST_ID_HEADER CACHE_CONTROL x-content-type-options referrer-policy; do
    rg -q "${header}" crates/aequora-axum/src/lib.rs
done

if rg -n 'axum|tower|http-body-util' \
    crates/aequora-server/Cargo.toml crates/aequora-executor/Cargo.toml \
    crates/aequora-store/Cargo.toml; then
    echo "HTTP framework dependency leaked into a transport-neutral core crate" >&2
    exit 1
fi

if rg -n 'axum::|http::Request|AUTHORIZATION|Bearer ' \
    crates/aequora-server/src crates/aequora-executor/src crates/aequora-store/src; then
    echo "HTTP request or credential type leaked beyond the Axum transport boundary" >&2
    exit 1
fi

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-axum --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-invariants -p aequora-conformance \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --offline --bin aequora-registry \
    -- verify .

echo "Axum server integration architecture checks passed"
