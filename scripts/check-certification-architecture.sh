#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

for path in \
    crates/aequora-conformance/Cargo.toml \
    crates/aequora-conformance/src/lib.rs \
    crates/aequora-testkit/src/conformance.rs \
    crates/aequora-testkit/tests/conformance_certification_contracts.rs \
    registry/core/certification.ron \
    conformance/fixtures/storage-core-passing-request.ron \
    conformance/fixtures/failure-trace.ron \
    ecosystem/catalog.ron \
    docs/conformance-invariant-coverage.md \
    config/certification.ron; do
    test -f "$path"
done

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-CERT${suffix}" crates/aequora-invariants/src/lib.rs
done

for domain in ConformanceProfile ConformanceTest CertificationTier; do
    rg -q "$domain" crates/aequora-registry-types/src/lib.rs
    rg -q "domain: $domain" registry/core/certification.ron
done

if rg -q '(^|[^[:alnum:]_-])(sqlx|stoolap|tokio|axum|quinn|reqwest)([^[:alnum:]_-]|$)' \
    crates/aequora-conformance/Cargo.toml; then
    echo "certification architecture: runtime, transport, or database dependency in neutral core" >&2
    exit 1
fi

rg -q 'aequora conform run' crates/aequora-cli/src/main.rs
rg -q 'certification_readiness' crates/aequora-config/src/lib.rs
rg -q 'conformance_certification_contracts' .github/workflows/ci.yml
rg -q 'adapter_contracts' .github/workflows/ci.yml

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-conformance --locked
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit --test conformance_certification_contracts --locked
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked -- conform storage >/dev/null
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked -- conform run conformance/fixtures/storage-core-passing-request.ron >/dev/null

echo "certification architecture: profiles, tiers, deterministic harness, fixtures, evidence integrity, lifecycle policy, ecosystem trust, CLI, CI, and nine invariants verified"
