#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    architecture/workspace-boundaries.ron
    crates/aequora-dev/src/workspace_architecture.rs
    docs/reference-implementation-workspace-boundaries-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 34 architecture artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-IMPL${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-IMPL${suffix}" sys-arch/34-reference-implementation-workspace-crate-boundary-architecture.md
done

if rg -n 'sqlx|stoolap|rusqlite|axum|reqwest|quinn|dioxus|jni|windows-sys|objc2' \
    crates/aequora-types/Cargo.toml crates/aequora-protocol/Cargo.toml \
    crates/aequora-storage-core/Cargo.toml; then
    echo "foundation, protocol, or storage contracts gained a physical integration dependency" >&2
    exit 1
fi

if rg -n 'sqlx::|stoolap::|rusqlite::' \
    crates/aequora-storage-core/src crates/aequora-store/src; then
    echo "physical database type leaked into a storage-neutral public contract" >&2
    exit 1
fi

if rg -n 'dioxus|leptos|yew|slint' crates/aequora-client/Cargo.toml; then
    echo "client correctness crate gained a UI framework dependency" >&2
    exit 1
fi

if rg -n 'axum|tower|hyper' crates/aequora-server/Cargo.toml; then
    echo "server correctness crate gained an HTTP framework dependency" >&2
    exit 1
fi

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-dev -p aequora-invariants --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-dev --locked --offline -- architecture check
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --locked --offline \
    --bin aequora-registry -- verify .

echo "reference implementation architecture checks passed"
