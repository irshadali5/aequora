#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-store-sqlite/Cargo.toml
    crates/aequora-store-sqlite/src/lib.rs
    crates/aequora-store-sqlite/src/config.rs
    crates/aequora-store-sqlite/src/backup.rs
    crates/aequora-store-sqlite/src/capabilities.rs
    docs/sqlite-local-adapter-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 42 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005; do
    rg -q "AEQ-INV-SQLITE${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-SQLITE${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-SQLITE${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-SQLITE${suffix}" sys-arch/42-sqlite-embedded-local-adapter-architecture.md
done

for table in \
    operation_outbox sync_cursor conflict_ledger scope_cache blob_manifest metadata; do
    rg -q "CREATE TABLE IF NOT EXISTS ${table}" crates/aequora-store-sqlite/src/lib.rs
done

for pragma in journal_mode synchronous foreign_keys busy_timeout; do
    rg -q "$pragma" crates/aequora-store-sqlite/src/lib.rs crates/aequora-store-sqlite/src/config.rs
done

for symbol in \
    SQLiteDatabase SQLiteConfig SQLiteHealth SQLiteBlobManifest SQLiteProjectionHook \
    SQLITE_LOCAL_CORE_PROFILE SQLITE_DESKTOP_LOCAL_FULL_PROFILE \
    SQLITE_MOBILE_LOCAL_FULL_PROFILE; do
    rg -q "\b${symbol}\b" crates/aequora-store-sqlite/src
done

rg -q 'transact_local_mutation' crates/aequora-store-sqlite/src/lib.rs
rg -q 'transaction_with_behavior\(TransactionBehavior::Immediate\)' \
    crates/aequora-store-sqlite/src/lib.rs
rg -q 'Backup::new' crates/aequora-store-sqlite/src/lib.rs
rg -q 'integrity_check' crates/aequora-store-sqlite/src/lib.rs

if rg -n 'rusqlite|aequora-store-sqlite' \
    crates/aequora-adapter-sdk/Cargo.toml crates/aequora-store/Cargo.toml; then
    echo "SQLite dependency leaked into a neutral storage contract crate" >&2
    exit 1
fi
if rg -n 'rusqlite::|SQLiteDatabase|SQLiteProjectionHook' \
    crates/aequora-adapter-sdk/src crates/aequora-store/src; then
    echo "SQLite physical types leaked into a neutral storage API" >&2
    exit 1
fi

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-store-sqlite --lib --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-invariants -p aequora-conformance --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora --test adapter_manifests \
    --features postgres,sqlite,stoolap --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --offline --bin aequora-registry \
    -- verify .

echo "SQLite embedded local adapter architecture checks passed"
