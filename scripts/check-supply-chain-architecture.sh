#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$BASH_SOURCE")/.." && pwd)"
cd "$root"

required=(
    Cargo.lock deny.toml LICENSE SECURITY.md DEPENDENCIES.md RELEASING.md
    THIRD_PARTY_LICENSES.txt THIRD_PARTY_NOTICES.md
    crates/aequora-supply-chain/src/lib.rs
    supply-chain/policy.ron supply-chain/exceptions.ron supply-chain/risk-register.ron
    supply-chain/approved-sources.ron supply-chain/third-party-services.ron
    scripts/generate-supply-chain-artifacts.sh
    release/publish-allowlist.txt
    crates/aequora-testkit/tests/supply_chain_contracts.rs
    tests/fixtures/part49/server.spdx.ron
    tests/fixtures/part49/server-release-evidence.ron
    docs/supply-chain-governance-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 49 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-SUPPLY$suffix" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-SUPPLY$suffix" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-SUPPLY$suffix" registry/core/certification.ron
    rg -q "AEQ-INV-SUPPLY$suffix" sys-arch/49-*.md
done

for symbol in DependencyPolicy DependencyRecord PolicyException RiskAcceptance SbomDocument \
    SbomComponent ReleaseEvidence DependencyChangeEvidence ReproducibilityReport \
    ThirdPartyService; do
    rg -q "\b$symbol\b" crates/aequora-supply-chain/src/lib.rs
done

if rg -n 'sqlx|stoolap|rusqlite|tokio|axum|reqwest|quinn|dioxus|opentelemetry|prometheus' \
    crates/aequora-supply-chain/Cargo.toml; then
    echo "vendor dependency leaked into neutral supply-chain contracts" >&2
    exit 1
fi

metadata="$(mktemp)"
generated="$(mktemp -d)"
trap 'rm -f "$metadata"; rm -rf "$generated"' EXIT
CARGO_BUILD_JOBS=1 cargo metadata --format-version 1 --locked --offline >"$metadata"
python3 scripts/publish-workspace.py --plan | rg -q 'Publishable crates:       99'

if jq -e '.packages[] | select(.source != null and (.license == null or .license == ""))' \
    "$metadata" >/dev/null; then
    echo "resolved external dependency has unknown license metadata" >&2
    exit 1
fi
if jq -e '.packages[] | select(.source != null and (.source | startswith("registry+https://github.com/rust-lang/crates.io-index") | not))' \
    "$metadata" >/dev/null; then
    echo "resolved external dependency uses an unapproved source" >&2
    exit 1
fi
if rg -n '^source = "git\+.*(branch=|tag=)' Cargo.lock; then
    echo "lockfile contains a branch- or tag-based Git dependency" >&2
    exit 1
fi

CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- \
    deps verify supply-chain/policy.ron | rg -q 'dependency policy: valid'
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- \
    supply-chain verify supply-chain/policy.ron \
    tests/fixtures/part49/server.spdx.ron \
    tests/fixtures/part49/server-release-evidence.ron | rg -q 'supply-chain: verified'

bash scripts/generate-supply-chain-artifacts.sh "$generated" Cargo.lock >/dev/null
jq -e '.spdxVersion == "SPDX-2.3" and (.packages | length > 1)' \
    "$generated/aequora.spdx.json" >/dev/null
test -s "$generated/THIRD_PARTY_LICENSES.txt"
test -s "$generated/THIRD_PARTY_NOTICES.md"
jq -e '.resolved_packages > 1 and (.build_scripts | type == "array") and (.proc_macros | type == "array") and (.native_boundaries | type == "array")' \
    "$generated/dependency-report.json" >/dev/null

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-supply-chain --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-conformance --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit \
    --test supply_chain_contracts --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-cli --lib --locked --offline

echo "supply-chain architecture: ok (16 critical reviews, 10 invariants, 10 conformance tests)"
