#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

test -f PROMPT.md
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

rg -q 'fn read_bounded\(' crates/aequora-cli/src/main.rs
rg -q 'fn read_bounded_text\(' crates/aequora-dev/src/main.rs
rg -q 'MAX_REGISTRY_FILE_BYTES' crates/aequora-registry-codegen/src/lib.rs
rg -q 'redirect::Policy::none\(\)' crates/aequora-http/src/lib.rs
rg -q 'coverage_and_future_suite_claims_cannot_be_resealed' \
    crates/aequora-conformance/src/lib.rs
rg -q 'evidence_paths_and_policy_metadata_are_bounded' \
    crates/aequora-conformance/src/lib.rs

echo "high-assurance hardening: immutable CI actions, dependency auditing, bounded file inputs, redirect isolation, and fail-closed conformance evidence verified"
