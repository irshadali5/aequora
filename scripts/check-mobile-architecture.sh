#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-mobile-runtime/Cargo.toml
    crates/aequora-mobile-runtime/src/lib.rs
    crates/aequora-platform-android/Cargo.toml
    crates/aequora-platform-android/src/lib.rs
    crates/aequora-platform-ios/Cargo.toml
    crates/aequora-platform-ios/src/lib.rs
    crates/aequora-mobile-bindings/Cargo.toml
    crates/aequora-mobile-bindings/src/lib.rs
    crates/aequora-testkit/tests/mobile_runtime_contracts.rs
    docs/android-ios-mobile-runtime-platform-completion.md
)

for path in "${required[@]}"; do
    test -f "$path" || { echo "missing mobile architecture artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-MOBILE${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-MOBILE${suffix}" crates/aequora-mobile-runtime/src/lib.rs
    rg -q "AEQ-INV-MOBILE${suffix}" crates/aequora-conformance/src/lib.rs
done

if rg -n 'tokio|jni|ndk|swift|objc|uikit|android[_-]context|sqlx|stoolap|rusqlite|reqwest' \
    crates/aequora-mobile-runtime/Cargo.toml; then
    echo "platform-neutral mobile runtime gained a platform, runtime, transport, or database dependency" >&2
    exit 1
fi

rg -q 'MobileClientFull' crates/aequora-conformance/src/lib.rs
rg -q 'AEQUORA_MOBILE_ABI_VERSION' crates/aequora-mobile-runtime/src/lib.rs
rg -q 'catch_unwind' crates/aequora-mobile-bindings/src/lib.rs
rg -q 'WorkManagerGrant' crates/aequora-platform-android/src/lib.rs
rg -q 'BgTaskGrant' crates/aequora-platform-ios/src/lib.rs

CARGO_BUILD_JOBS=1 cargo test -q \
    -p aequora-mobile-runtime \
    -p aequora-platform-android \
    -p aequora-platform-ios \
    -p aequora-mobile-bindings \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit --test mobile_runtime_contracts --locked --offline

echo "mobile architecture checks passed"
