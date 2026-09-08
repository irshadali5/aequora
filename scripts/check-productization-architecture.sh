#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-release/src/productization.rs
    crates/aequora-testkit/tests/productization_ga_contracts.rs
    release/v1-scope.ron
    release/v1-readiness.ron
    release/KNOWN_ISSUES.md
    docs/v1-productization-ga-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 50 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-GA${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-GA${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-GA${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-GA${suffix}" sys-arch/50-*.md
done

for symbol in V1Scope GaReadinessManifest GaDecision GateEvidence SupportClaim KnownDefect \
    UpgradeEvidence MilestoneEvidence ReadinessReview RiskRecord; do
    rg -q "\b${symbol}\b" crates/aequora-release/src/productization.rs
done

if rg -n 'sqlx|stoolap|rusqlite|tokio|axum|reqwest|quinn|dioxus' \
    crates/aequora-release/Cargo.toml; then
    echo "runtime, transport, UI, or physical database dependency leaked into productization policy" >&2
    exit 1
fi

rg -q 'GeneralAvailabilityFull' crates/aequora-conformance/src/lib.rs
rg -q 'GeneralAvailabilityFull' registry/core/certification.ron
rg -q 'aequora compatibility check' crates/aequora-cli/src/lib.rs
rg -q 'version \[--verbose\]' crates/aequora-cli/src/lib.rs

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-release --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-conformance -p aequora-invariants --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit \
    --test productization_ga_contracts --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-cli --lib --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-release --bin aequora-release --locked --offline -- \
    v1-scope release/v1-scope.ron | rg -q 'v1-scope: frozen'
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-release --bin aequora-release --locked --offline -- \
    readiness release/v1-readiness.ron | rg -q 'eligible=false'
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- \
    compatibility check | rg -q 'compatibility: compatible'
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- \
    version --verbose | rg -q 'registry-generation='
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --offline --bin aequora-registry -- \
    verify .

echo "v1 productization and GA-readiness architecture: ok (10 invariants, 10 conformance tests)"
