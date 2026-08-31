#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-adapter-sdk/src/capabilities.rs
    crates/aequora-adapter-sdk/src/local.rs
    crates/aequora-adapter-sdk/src/authority.rs
    crates/aequora-adapter-sdk/src/journal.rs
    crates/aequora-adapter-sdk/src/ledger.rs
    crates/aequora-adapter-sdk/src/snapshot.rs
    crates/aequora-adapter-sdk/src/fencing.rs
    crates/aequora-adapter-sdk/src/migration.rs
    crates/aequora-adapter-sdk/src/errors.rs
    crates/aequora-adapter-sdk/src/conformance.rs
    crates/aequora-testkit/tests/storage_adapter_sdk_contracts.rs
    docs/storage-adapter-sdk.md
    docs/storage-adapter-sdk-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 36 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-ADAPTER${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-ADAPTER${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-ADAPTER${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-ADAPTER${suffix}" sys-arch/36-storage-adapter-sdk-official-adapter-architecture.md
done

for module in capabilities local authority journal ledger snapshot fencing migration errors conformance; do
    rg -q "pub mod ${module};" crates/aequora-adapter-sdk/src/lib.rs
done

for symbol in \
    LocalTransactionStore AuthorityTransactionStore OutboxStore CursorStore JournalStore \
    OperationLedgerStore SnapshotStore FencingStore MigrationStore ObjectStore \
    AdapterManifest AdapterRequirements CertifiedEnvironment RetryDisposition; do
    rg -q "\b${symbol}\b" crates/aequora-adapter-sdk/src
done

if rg -n 'sqlx|stoolap|rusqlite|tokio|axum|reqwest|quinn|jni|windows-sys|objc2' \
    crates/aequora-adapter-sdk/Cargo.toml; then
    echo "adapter SDK gained a database, runtime, transport, or platform dependency" >&2
    exit 1
fi

if rg -n 'sqlx::|stoolap::|rusqlite::|tokio::|axum::|reqwest::|quinn::' \
    crates/aequora-adapter-sdk/src; then
    echo "physical or runtime type leaked into the adapter SDK" >&2
    exit 1
fi

rg -q 'POSTGRES_STORAGE_ADAPTER_MANIFEST' crates/aequora-store-postgres/src/lib.rs
rg -q 'STOOLAP_STORAGE_ADAPTER_MANIFEST' crates/aequora-store-stoolap/src/lib.rs
rg -q 'AdapterSupport::Official' crates/aequora-store-postgres/src/lib.rs
rg -q 'AdapterSupport::Official' crates/aequora-store-stoolap/src/lib.rs
rg -q 'LocalAdapter' crates/aequora-conformance/src/lib.rs
rg -q 'AuthoritativeAdapter' crates/aequora-conformance/src/lib.rs
rg -q 'SnapshotAdapter' crates/aequora-conformance/src/lib.rs
rg -q 'FencingAdapter' crates/aequora-conformance/src/lib.rs
rg -q '"rust-analyzer.cargo.autoreload": false' .vscode/settings.json
rg -q '"rust-analyzer.files.watcher": "client"' .vscode/settings.json
rg -q '"\*\*/target/\*\*": true' .vscode/settings.json

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-adapter-sdk -p aequora-conformance \
    -p aequora-invariants --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit \
    --test storage_adapter_sdk_contracts --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-store-stoolap \
    part_36_manifest_is_structurally_valid_and_explicit --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-store-postgres \
    part_36_manifest_is_structurally_valid_and_explicit --locked --offline

echo "storage adapter SDK architecture checks passed"
