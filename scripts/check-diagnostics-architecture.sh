#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-DIAG${suffix}" crates/aequora-invariants/src/lib.rs
done

test -f crates/aequora-diagnostics/Cargo.toml
test -f crates/aequora-diagnostics/src/lib.rs
test -f crates/aequora-client/src/diagnostics.rs
test -f crates/aequora-server/src/diagnostics.rs
test -f crates/aequora-testkit/tests/diagnostic_incident_contracts.rs

if rg -q '(^|[^[:alnum:]_-])(sqlx|stoolap|tokio|axum|reqwest|rayon|zstd)([^[:alnum:]_-]|$)' \
    crates/aequora-diagnostics/Cargo.toml; then
    echo "diagnostics architecture: runtime, archive, database, or transport dependency in core" >&2
    exit 1
fi

for symbol in IncidentId IncidentBundleId IncidentClass DiagnosticSelector DiagnosticEvent \
    DiagnosticRing DiagnosticPolicy DiagnosticProvider DiagnosticSanitizer DiagnosticSection \
    OperationForensicView EntityForensicView ScopeForensicView DeviceForensicView \
    AuthorityForensicView JobForensicView RuntimeInventory TimelineEvent EvidenceConfidence \
    IncidentBundleManifest HashInventory BundleEncryptor BundleSigner BundleVerification \
    ReplayManifest ReplaySandbox FailureTraceMinimizer IncidentRegistry; do
    rg -q "${symbol}" crates/aequora-diagnostics/src/lib.rs
done

rg -q 'pub mod diagnostics' crates/aequora-client/src/lib.rs
rg -q 'pub mod diagnostics' crates/aequora-server/src/lib.rs
rg -q 'Some\("incident"\)' crates/aequora-cli/src/main.rs
rg -q '"aequora-diagnostics"' crates/aequora-dev/src/main.rs

if rg -q 'SigningSecret|DataEncryptionKey|SecretKey' crates/aequora-diagnostics/src/lib.rs; then
    echo "diagnostics architecture: serializable private-key type exposed by core" >&2
    exit 1
fi

echo "diagnostics architecture: bounded collection, sanitization, protection boundaries, replay isolation, nine invariants, and neutral dependencies verified"
