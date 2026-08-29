#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

for path in \
    crates/aequora-registry-types/Cargo.toml \
    crates/aequora-registry-codegen/Cargo.toml \
    crates/aequora-registry-generated/Cargo.toml \
    crates/aequora-registry-cli/Cargo.toml \
    registry/manifest.ron registry/registry.lock \
    docs/registry/generated-registry.md; do
    test -f "$path"
done

for directory in registry/core registry/app registry/extensions; do
    test -d "$directory"
done

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-REGISTRY${suffix}" crates/aequora-invariants/src/lib.rs
done

if rg -q '(^|[^[:alnum:]_-])(sqlx|stoolap|tokio|axum|quinn|reqwest)([^[:alnum:]_-]|$)' \
    crates/aequora-registry-types/Cargo.toml crates/aequora-registry-generated/Cargo.toml; then
    echo "registry architecture: runtime, transport, or database dependency in registry runtime layers" >&2
    exit 1
fi

for domain in Entity Operation Event Field Capability ConsistencyProfile Error Job Consumer \
    AuditAction Migration Protocol Message Permission AdminAction Reason DecisionRule ArtifactFormat \
    ConformanceProfile ConformanceTest CertificationTier; do
    rg -q "${domain}" crates/aequora-registry-types/src/lib.rs
done

rg -q 'pub use aequora_registry_generated as registry' crates/aequora/src/lib.rs
rg -q 'durable_registry_provenance' crates/aequora-diagnostics/src/lib.rs
rg -q 'cargo .*aequora-registry-cli.*verify' .github/workflows/ci.yml

CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --locked --bin aequora-registry -- verify . >/dev/null

echo "registry architecture: canonical RON, immutable lock, code generation, namespace/range checks, compatibility tooling, historical resolution, diagnostics provenance, and nine invariants verified"
