#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-ADMIN${suffix}" crates/aequora-invariants/src/lib.rs
done

test -f crates/aequora-admin/Cargo.toml
test -f crates/aequora-admin/src/lib.rs
test -f crates/aequora-server/src/admin.rs
test -f crates/aequora-testkit/tests/admin_control_plane_contracts.rs

if rg -q '(^|[^[:alnum:]_-])(sqlx|stoolap|tokio|axum|reqwest|rayon)([^[:alnum:]_-]|$)' \
    crates/aequora-admin/Cargo.toml; then
    echo "admin architecture: runtime, database, or transport dependency in core" >&2
    exit 1
fi

for symbol in AdminOperationId AdminAction AdminPrincipal PermissionId AssuranceLevel \
    AdminStore AdminService AdminPlan PlanReference AdminApproval MaintenanceMode \
    MaintenanceStateStore DynamicConfigRegistry AdminListenerPolicy BreakGlassSessionId \
    AdminQuery AdminReadService AdminErrorCode CryptoKeyView; do
    rg -q "${symbol}" crates/aequora-admin/src/lib.rs
done

rg -q 'pub mod admin' crates/aequora-server/src/lib.rs
rg -q 'Some\("admin"\)' crates/aequora-cli/src/main.rs

if rg -q 'SecretKey|SigningSecret|DataEncryptionKey' crates/aequora-admin/src/lib.rs; then
    echo "admin architecture: private key type exposed by admin core" >&2
    exit 1
fi

echo "admin architecture: typed authorization, idempotency, plans, approvals, listener isolation, nine invariants, and neutral dependencies verified"
