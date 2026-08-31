#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-desktop-runtime/Cargo.toml
    crates/aequora-desktop-runtime/src/lib.rs
    crates/aequora-ipc-protocol/Cargo.toml
    crates/aequora-agent/Cargo.toml
    crates/aequora-platform-linux/Cargo.toml
    crates/aequora-platform-windows/Cargo.toml
    crates/aequora-platform-macos/Cargo.toml
    crates/aequora-update/Cargo.toml
    crates/aequora-testkit/tests/desktop_runtime_contracts.rs
    docs/linux-windows-macos-desktop-runtime-completion.md
)

for path in "${required[@]}"; do
    test -f "$path" || { echo "missing desktop architecture artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-DESKTOP${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-DESKTOP${suffix}" crates/aequora-desktop-runtime/src/lib.rs
    rg -q "AEQ-INV-DESKTOP${suffix}" crates/aequora-conformance/src/lib.rs
done

if rg -n 'tokio|async-std|sqlx|stoolap|rusqlite|reqwest|windows-sys|objc|appkit|gtk' \
    crates/aequora-desktop-runtime/Cargo.toml crates/aequora-ipc-protocol/Cargo.toml crates/aequora-update/Cargo.toml; then
    echo "neutral desktop crates gained a runtime, transport, database, or OS-framework dependency" >&2
    exit 1
fi

rg -q 'DesktopClientFull' crates/aequora-conformance/src/lib.rs
rg -q 'DesktopAgentFull' crates/aequora-conformance/src/lib.rs
rg -q 'CoordinationSnapshot' crates/aequora-desktop-runtime/src/lib.rs
rg -q 'SessionToken' crates/aequora-ipc-protocol/src/lib.rs
rg -q 'peer_is_current_user' crates/aequora-agent/src/lib.rs
rg -q 'SecretService' crates/aequora-platform-linux/src/lib.rs
rg -q 'CredentialManager' crates/aequora-platform-windows/src/lib.rs
rg -q 'Keychain' crates/aequora-platform-macos/src/lib.rs

CARGO_BUILD_JOBS=1 cargo test -q \
    -p aequora-desktop-runtime \
    -p aequora-ipc-protocol \
    -p aequora-agent \
    -p aequora-platform-linux \
    -p aequora-platform-windows \
    -p aequora-platform-macos \
    -p aequora-update \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit --test desktop_runtime_contracts --locked --offline

echo "desktop architecture checks passed"
