#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-cli-core/Cargo.toml
    crates/aequora-cli-core/src/lib.rs
    crates/aequora-inspect/Cargo.toml
    crates/aequora-inspect/src/lib.rs
    crates/aequora-devtools/Cargo.toml
    crates/aequora-devtools/src/lib.rs
    crates/aequora-cli/src/lib.rs
    crates/aequora-cli/src/main.rs
    docs/cli-developer-toolchain-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 41 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-CLI${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-CLI${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-CLI${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-CLI${suffix}" sys-arch/41-cli-developer-toolchain-architecture.md
done

for symbol in \
    CliRequest Command CommandName GlobalOptions OutputFormat MachineEnvelope CliErrorEnvelope \
    CliExitCode ErrorCode RetryClass SafeDetails CommandPolicy SafetyClass ApplyIntent \
    SubmissionState MigrationIdentity MigrationPreconditions DoctorReport; do
    rg -q "\b${symbol}\b" crates/aequora-cli-core/src/lib.rs
done

for symbol in InspectionRequest InspectionMode StoreOwnership InspectionRoute FieldClassification; do
    rg -q "\b${symbol}\b" crates/aequora-inspect/src/lib.rs
done

for symbol in ScaffoldPlan Template FileOwnership EnvironmentClass DevelopmentAction; do
    rg -q "\b${symbol}\b" crates/aequora-devtools/src/lib.rs
done

test "$(wc -l < crates/aequora-cli/src/main.rs)" -le 12
rg -q 'aequora_cli::run_main()' crates/aequora-cli/src/main.rs
rg -q 'CliRequest::parse' crates/aequora-cli/src/lib.rs
rg -q 'render_machine' crates/aequora-cli/src/lib.rs

if rg -n 'sqlx|stoolap|tokio|axum|reqwest|quinn|dioxus' \
    crates/aequora-cli-core/Cargo.toml crates/aequora-inspect/Cargo.toml \
    crates/aequora-devtools/Cargo.toml \
    || rg -n 'sqlx::|stoolap::|tokio::|axum::|reqwest::|quinn::|dioxus::' \
    crates/aequora-cli-core/src crates/aequora-inspect/src crates/aequora-devtools/src; then
    echo "runtime, transport, UI, or physical database dependency leaked into neutral CLI contracts" >&2
    exit 1
fi

if rg -n '(^|[^[:alnum:]_])(sqlx|stoolap)::|DELETE[[:space:]]+FROM|UPDATE[[:space:]]+aequora_' \
    crates/aequora-cli/src; then
    echo "CLI directly accesses physical storage internals" >&2
    exit 1
fi

rg -q 'feature = "test-failpoints"' crates/aequora-devtools/src/lib.rs
rg -q 'EnvironmentClass::Production' crates/aequora-devtools/src/lib.rs
rg -q 'StoreOwnership::AgentOwned' crates/aequora-inspect/src/lib.rs
rg -q 'InspectionMode::FencedMaintenance' crates/aequora-inspect/src/lib.rs
rg -q 'MACHINE_SCHEMA_VERSION: u16 = 1' crates/aequora-cli-core/src/lib.rs
rg -q 'RetrySameIdentity' crates/aequora-cli-core/src/lib.rs

rg -q '"rust-analyzer.checkOnSave": false' .vscode/settings.json
rg -q '"rust-analyzer.numThreads": 1' .vscode/settings.json
rg -q '"rust-analyzer.cachePriming.enable": false' .vscode/settings.json
rg -q '"rust-analyzer.procMacro.enable": false' .vscode/settings.json

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-cli-core -p aequora-inspect \
    -p aequora-devtools -p aequora-cli --all-features --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-invariants -p aequora-conformance \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --offline --bin aequora-registry \
    -- verify .

machine_output="$(CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- \
    --output json doctor adapters)"
rg -q '"schema_version": 1' <<<"$machine_output"
rg -q '"command": "doctor"' <<<"$machine_output"
if rg -q 'postgres://' <<<"$machine_output"; then
    echo "machine output exposed a database credential" >&2
    exit 1
fi

set +e
machine_error="$(CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- \
    --output json unknown-command 2>&1)"
machine_status=$?
set -e
test "$machine_status" -eq 2
rg -q '"schema_version": 1' <<<"$machine_error"
rg -q '"code": "InvalidUsage"' <<<"$machine_error"

echo "CLI and developer toolchain architecture checks passed"
