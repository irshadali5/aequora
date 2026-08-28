#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-META${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-META${suffix}" crates/aequora-metadata/src/invariants.rs
done

for module in version client server records indexes migrations invariants export; do
    test -f "crates/aequora-metadata/src/${module}.rs"
done

for symbol in MetadataSchemaVersion MetadataRoot OutboxRecord ScopeCursorRecord \
    OperationLedgerRecord JournalRecord SnapshotRecord MetadataMigration \
    AdapterMappingContract LocalTransaction AuthoritativeTransaction; do
    rg -q "${symbol}" crates/aequora-metadata/src
done

if rg -q '(^|[^[:alnum:]_-])(sqlx|stoolap|tokio|axum|reqwest|rayon)([^[:alnum:]_-]|$)' \
    crates/aequora-metadata/Cargo.toml; then
    echo "metadata architecture: runtime, database, transport, or CPU-pool dependency detected" >&2
    exit 1
fi

echo "metadata architecture: typed schema, nine invariants, and neutral dependencies verified"
