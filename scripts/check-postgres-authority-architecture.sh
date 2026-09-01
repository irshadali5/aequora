#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-store-postgres/src/config.rs
    crates/aequora-store-postgres/src/pool.rs
    crates/aequora-store-postgres/src/tx.rs
    crates/aequora-store-postgres/src/migration.rs
    crates/aequora-store-postgres/src/journal.rs
    crates/aequora-store-postgres/src/ledger.rs
    crates/aequora-store-postgres/src/version.rs
    crates/aequora-store-postgres/src/device.rs
    crates/aequora-store-postgres/src/scope.rs
    crates/aequora-store-postgres/src/snapshot.rs
    crates/aequora-store-postgres/src/audit.rs
    crates/aequora-store-postgres/src/governance.rs
    crates/aequora-store-postgres/src/retention.rs
    crates/aequora-store-postgres/src/health.rs
    crates/aequora-store-postgres/src/errors.rs
    crates/aequora-store-postgres/src/capabilities.rs
    crates/aequora-store-postgres/src/neon.rs
    crates/aequora-store-postgres/src/observability.rs
    docs/postgresql-neon-authority-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 37 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-PG${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-PG${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-PG${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-PG${suffix}" sys-arch/37-postgresql-neon-authoritative-adapter-detailed-architecture.md
done

for symbol in \
    PostgresAuthorityStore PostgresAdapterConfig PostgresTransactionConfig \
    PostgresSideEffectIntent PostgresDeviceRecord PostgresRetentionLease \
    PostgresReadiness PostgresMetrics POSTGRES_AUTHORITY_FULL_PROFILE NEON_OPERATIONAL_PROFILE \
    record_restore_epoch_transition; do
    rg -q "\b${symbol}\b" crates/aequora-store-postgres/src
done

for ddl in \
    payload_digest aequora_side_effect_jobs aequora_devices aequora_retention_leases \
    aequora_retention_policies \
    publication_status aequora_archive_ranges; do
    rg -q "${ddl}" crates/aequora-store-postgres/src/lib.rs
done

rg -q 'pg_advisory_xact_lock' crates/aequora-store-postgres/src/lib.rs
rg -q 'statement_timeout' crates/aequora-store-postgres/src/lib.rs
rg -q 'lock_timeout' crates/aequora-store-postgres/src/lib.rs
rg -q 'idle_in_transaction_session_timeout' crates/aequora-store-postgres/src/lib.rs
rg -q 'payload_digest.*operation_kind' crates/aequora-store-postgres/src/lib.rs
rg -q 'publication_status=.published.' crates/aequora-store-postgres/src/lib.rs

if rg -n 'sqlx|postgres|tokio' crates/aequora-adapter-sdk/Cargo.toml crates/aequora-store/Cargo.toml; then
    echo "PostgreSQL/runtime dependency leaked into a neutral storage contract crate" >&2
    exit 1
fi
if rg -n 'sqlx::|PgPool|PgRow|PgConnectOptions' \
    crates/aequora-adapter-sdk/src crates/aequora-store/src; then
    echo "PostgreSQL driver type leaked into a neutral storage API" >&2
    exit 1
fi

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-store-postgres --lib --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-store-postgres --test postgres_live --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-invariants -p aequora-conformance --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit --test storage_adapter_sdk_contracts \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --offline --bin aequora-registry \
    -- verify .

echo "PostgreSQL/Neon authority architecture checks passed"
