#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    architecture/public-rust-api-v0.1.txt
    crates/aequora-operation/Cargo.toml
    crates/aequora-operation/src/lib.rs
    crates/aequora-adapter-sdk/Cargo.toml
    crates/aequora-adapter-sdk/src/lib.rs
    crates/aequora-client/src/sdk.rs
    crates/aequora-server/src/sdk.rs
    docs/public-rust-sdk.md
    docs/sdk-versioning-and-upgrades.md
    docs/public-rust-sdk-completion.md
    .vscode/settings.json
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 35 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-SDK${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-SDK${suffix}" sys-arch/35-public-rust-api-sdk-stability-architecture.md
done

for item in \
    AequoraClient AequoraClientBuilder AequoraServer AequoraServerBuilder Operation \
    OperationId OperationKind OperationSchemaVersion MutationReceipt LocalCommitStatus \
    SyncStatus SyncResult SyncNextAction SyncEvent ConflictHandle ScopeHandle BlobHandle \
    AequoraError AequoraErrorCode; do
    rg -q "aequora::${item}$" architecture/public-rust-api-v0.1.txt
    rg -q "\b${item}\b" crates/aequora/src/lib.rs
done

for code in \
    AEQ-SDK-STORAGE-001 AEQ-SDK-TRANSPORT-001 AEQ-SDK-AUTHN-001 AEQ-SDK-AUTHZ-001 \
    AEQ-SDK-CONFLICT-001 AEQ-SDK-VALIDATION-001 AEQ-SDK-RESOURCE-001 \
    AEQ-SDK-CAPABILITY-001 AEQ-SDK-CLOSED-001 AEQ-SDK-INTERNAL-001; do
    rg -q "$code" architecture/public-rust-api-v0.1.txt
    rg -q "$code" crates/aequora-client/src/sdk.rs
done

if rg -n 'sqlx|stoolap|rusqlite|axum|reqwest|quinn|jni|windows-sys|objc2' \
    crates/aequora-operation/Cargo.toml crates/aequora-adapter-sdk/Cargo.toml \
    crates/aequora-client/Cargo.toml crates/aequora-server/Cargo.toml; then
    echo "stable SDK or extension contract gained a physical/framework dependency" >&2
    exit 1
fi

if rg -n 'sqlx::|stoolap::|rusqlite::|axum::|reqwest::|quinn::' \
    crates/aequora-operation/src crates/aequora-adapter-sdk/src \
    crates/aequora-client/src/sdk.rs crates/aequora-server/src/sdk.rs; then
    echo "physical or transport-framework type leaked into the stable SDK" >&2
    exit 1
fi

if rg -n 'pub (async )?fn (advance_cursor|set_cursor|mutate_ledger|delete_outbox|raw_transaction)' \
    crates/aequora-operation/src crates/aequora-adapter-sdk/src \
    crates/aequora-client/src/sdk.rs crates/aequora-server/src/sdk.rs; then
    echo "stable convenience API exposes an invariant-bypassing operation" >&2
    exit 1
fi

rg -q '^default = \[\]$' crates/aequora/Cargo.toml
rg -Fq 'broadcast::channel(config.event_capacity)' crates/aequora-client/src/sdk.rs
rg -q 'pub async fn status' crates/aequora-client/src/sdk.rs
rg -q 'LocalCommitStatus' crates/aequora-operation/src/lib.rs
rg -q '#\[non_exhaustive\]' crates/aequora-client/src/sdk.rs
rg -Fq 'Credential([REDACTED])' crates/aequora-adapter-sdk/src/lib.rs

rg -q 'cargo-semver-checks --version 0.49.0 --locked' .github/workflows/ci.yml
rg -q 'cargo semver-checks.*baseline-rev' .github/workflows/ci.yml
rg -q 'check-public-sdk-architecture.sh' .github/workflows/ci.yml

rg -q '"rust-analyzer.checkOnSave": false' .vscode/settings.json
rg -q '"rust-analyzer.numThreads": 1' .vscode/settings.json
rg -q '"rust-analyzer.cachePriming.enable": false' .vscode/settings.json
rg -q '"rust-analyzer.cfg.setTest": false' .vscode/settings.json
rg -q '"rust-analyzer.cargo.buildScripts.enable": false' .vscode/settings.json
rg -q '"rust-analyzer.lru.capacity": 64' .vscode/settings.json
rg -q '"rust-analyzer.procMacro.enable": false' .vscode/settings.json

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-operation -p aequora-adapter-sdk \
    -p aequora-client -p aequora-server -p aequora-invariants --locked --offline

echo "public Rust SDK architecture checks passed"
