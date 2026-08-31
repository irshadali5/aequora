#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-storage-core/src/lib.rs
    crates/aequora-storage-profile/src/lib.rs
    crates/aequora-storage-maintenance/src/lib.rs
    crates/aequora-storage-backup/src/lib.rs
    crates/aequora-storage-encryption/src/lib.rs
    crates/aequora-storage-conformance/src/lib.rs
    crates/aequora-blob-store/src/lib.rs
    crates/aequora-testkit/tests/local_storage_contracts.rs
    docs/cross-platform-local-storage-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing local storage artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-STORAGE${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-STORAGE${suffix}" crates/aequora-storage-core/src/lib.rs
    rg -q "AEQ-INV-STORAGE${suffix}" crates/aequora-conformance/src/lib.rs
done

if rg -n 'tokio|async-std|sqlx|stoolap|rusqlite|reqwest|windows-sys|objc|appkit|gtk' \
    crates/aequora-storage-{core,profile,maintenance,backup,encryption,conformance}/Cargo.toml \
    crates/aequora-blob-store/Cargo.toml; then
    echo "neutral storage crates gained a runtime, transport, database, or OS-framework dependency" >&2
    exit 1
fi

rg -q 'MobileLocalStoreFull' crates/aequora-storage-conformance/src/lib.rs
rg -q 'DesktopLocalStoreFull' crates/aequora-storage-conformance/src/lib.rs
rg -q 'RebindAndRebase' crates/aequora-storage-backup/src/lib.rs
rg -q 'ReadMostly' crates/aequora-storage-core/src/lib.rs
rg -q 'SecureProvider' crates/aequora-storage-encryption/src/lib.rs

CARGO_BUILD_JOBS=1 cargo test -q \
    -p aequora-storage-core -p aequora-storage-profile -p aequora-storage-maintenance \
    -p aequora-storage-backup -p aequora-storage-encryption \
    -p aequora-storage-conformance -p aequora-blob-store --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit --test local_storage_contracts --locked --offline

echo "local storage architecture checks passed"
