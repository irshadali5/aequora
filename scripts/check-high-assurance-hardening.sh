#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

test -f HARDENING.md
test -f .cargo/audit.toml
test -f docs/high-assurance-hardening-completion.md

rg -q '^unsafe_code = "forbid"' Cargo.toml
rg -q '^unwrap_used = \{ level = "deny"' Cargo.toml
rg -q '^expect_used = \{ level = "deny"' Cargo.toml

if rg --pcre2 -n 'uses:\s*[^[:space:]#]+@(?![0-9a-f]{40}(?:[[:space:]]|$))' \
    .github/workflows; then
    echo "high-assurance hardening: mutable GitHub Action reference" >&2
    exit 1
fi

rg -q 'cargo-audit --version 0\.22\.2 --locked' .github/workflows/ci.yml
rg -q 'audit --file Cargo\.lock' .github/workflows/ci.yml
rg -q 'audit --file fuzz/Cargo\.lock' .github/workflows/ci.yml
rg -q 'RUSTSEC-2023-0071' .cargo/audit.toml

rg -q 'fn read_bounded\(' crates/aequora-cli/src/lib.rs
rg -q 'fn read_bounded_text\(' crates/aequora-dev/src/main.rs
rg -q 'MAX_REGISTRY_FILE_BYTES' crates/aequora-registry-codegen/src/lib.rs
rg -q 'redirect::Policy::none\(\)' crates/aequora-http/src/lib.rs
rg -q 'coverage_and_future_suite_claims_cannot_be_resealed' \
    crates/aequora-conformance/src/lib.rs
rg -q 'evidence_paths_and_policy_metadata_are_bounded' \
    crates/aequora-conformance/src/lib.rs

rg -q 'AEQUORA_STRESS_AUTH_KEY_HEX' crates/aequora/examples/support/stress_auth.rs
rg -q 'blake3::keyed_hash' crates/aequora/examples/support/stress_auth.rs
rg -q 'constant_time_eq' crates/aequora/examples/support/stress_auth.rs
rg -q 'task identity mismatch' crates/aequora/examples/realworld_server.rs
rg -q 'with_graceful_shutdown' crates/aequora/examples/realworld_server.rs
rg -q 'MAX_LATENCY_SAMPLES' crates/aequora/examples/realworld_stress.rs
rg -q 'pending_outbox' crates/aequora/examples/realworld_stress.rs
rg -q 'record_unhandled' crates/aequora/examples/realworld_stress.rs
if rg -n 'test-token|starts_with\("bearer-"\)|TenantId::from_str\(tenant_str\)' \
    crates/aequora/examples; then
    echo "high-assurance hardening: forgeable stress-harness authentication" >&2
    exit 1
fi

rg -q 'take\(u64::try_from\(MAX_SECRET_BYTES \+ 1\)' crates/aequora-secrets/src/lib.rs
rg -q 'validate_secret_value' crates/aequora-secrets/src/lib.rs
if rg -n 'fs::read\(path\)' crates/aequora-secrets/src/lib.rs; then
    echo "high-assurance hardening: secret file input is not single-open and bounded" >&2
    exit 1
fi

rg -q 'MAX_CONFIG_RON_BYTES' crates/aequora-config/src/lib.rs
rg -q 'MAX_DEPLOYMENT_RON_BYTES' crates/aequora-deployment/src/lib.rs
rg -q 'MAX_CATALOG_RON_BYTES' crates/aequora-observability/src/part46.rs
rg -q 'MAX_MANIFEST_RON_BYTES' crates/aequora-release/src/productization.rs
rg -q 'MAX_POLICY_RON_BYTES' crates/aequora-supply-chain/src/lib.rs

rg -q '^FROM [^[:space:]]+@sha256:[0-9a-f]{64}$' \
    deploy/container/realworld.Containerfile
if rg -n '^FROM .*:latest|image: .*:latest' deploy; then
    echo "high-assurance hardening: mutable container image reference" >&2
    exit 1
fi
rg -q 'hostIP: 127\.0\.0\.1' deploy/kubernetes/realworld-k8s.yaml
rg -q 'automountServiceAccountToken: false' deploy/kubernetes/realworld-k8s.yaml
rg -q 'readOnlyRootFilesystem: true' deploy/kubernetes/realworld-k8s.yaml
rg -q 'drop: \["ALL"\]' deploy/kubernetes/realworld-k8s.yaml
rg -q 'terminationGracePeriodSeconds: 10' deploy/kubernetes/realworld-k8s.yaml
rg -q 'sizeLimit: 64Mi' deploy/kubernetes/realworld-k8s.yaml

echo "high-assurance hardening: immutable CI/container inputs, dependency auditing, bounded file and harness inputs, authenticated test identities, redirect isolation, and fail-closed evidence verified"
