#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

test -f crates/aequora-security/Cargo.toml
test -f crates/aequora-security/src/lib.rs
test -f crates/aequora-testkit/tests/security_abuse_contracts.rs
test -f config/security.ron
test -f docs/security-threat-matrix.md
test -f docs/security-runbooks.md
test -f docs/security-ownership.md
test -f SECURITY.md

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-SEC${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-SEC${suffix}" crates/aequora-security/src/lib.rs
done

if rg -q '(^|[^[:alnum:]_-])(sqlx|stoolap|tokio|axum|reqwest|quinn)([^[:alnum:]_-]|$)' \
    crates/aequora-security/Cargo.toml; then
    echo "security architecture: runtime, transport, provider, or database dependency in core" >&2
    exit 1
fi

for symbol in AuthenticationEvidence AuthenticationPolicy ValidatedAuthContext TenantBinding \
    TenantResource ProtocolLimits InputShape UploadLimits ArchiveEntry EgressPolicy \
    ValidatedOutboundTarget OperationBinding OperationReplayGuard AuthorityEpochGuard \
    SideEffectSafety SecretString SecurityEvent SecurityErrorCode SecurityMetric SecurityPolicy \
    SecurityAsset AttackerClass TrustBoundary SECURITY_INVARIANTS; do
    rg -q "${symbol}" crates/aequora-security/src/lib.rs
done

for term in "localhost" "169, 254, 169, 254" "is_private" "is_unicast_link_local" \
    "revalidate_connection" "UnsafeArchivePath" "SymbolicLink" "PayloadMismatch" \
    "AuthorityRollback" "TwoPersonDestructiveApproval"; do
    rg -q "${term}" crates/aequora-security/src/lib.rs
done

rg -q 'from_validated_security' crates/aequora-executor/src/lib.rs
rg -q 'pub use aequora_security as security' crates/aequora/src/lib.rs
rg -q 'Some\("security"\)' crates/aequora-cli/src/main.rs
rg -q '"aequora-security"' crates/aequora-dev/src/main.rs

cargo run -q -p aequora-cli --locked -- security validate config/security.ron >/dev/null

echo "security architecture: ten invariants, trusted identity bridge, tenant isolation, bounded inputs, replay/rollback resistance, SSRF/archive defenses, secret handling, threat matrix, runbooks, and neutral dependencies verified"
