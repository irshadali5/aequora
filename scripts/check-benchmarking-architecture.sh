#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-workload/src/lib.rs
    crates/aequora-benchkit/src/lib.rs
    crates/aequora-loadgen/src/lib.rs
    crates/aequora-testkit/tests/benchmarking_capacity_contracts.rs
    docs/benchmarking-capacity-planning-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 47 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-BENCH${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-BENCH${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-BENCH${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-BENCH${suffix}" sys-arch/47-*.md
done

for symbol in WorkloadSpec ScenarioId ArrivalModel LoadModel NetworkModel DatasetGenerator; do
    rg -q "\b${symbol}\b" crates/aequora-workload/src/lib.rs
done
for symbol in BenchmarkManifest EnvironmentFingerprint BuildFingerprint DatasetFingerprint \
    BenchmarkResult RegressionComparison RegressionDecision EvidenceClass CapacityEstimate \
    ProductionTargetGuard CorrectnessProfile; do
    rg -q "\b${symbol}\b" crates/aequora-benchkit/src/lib.rs
done
for symbol in LoadGenerator VirtualClientState CorrectnessOracle AdmissionOutcome LoadCounters; do
    rg -q "\b${symbol}\b" crates/aequora-loadgen/src/lib.rs crates/aequora-benchkit/src/lib.rs
done

for manifest in crates/aequora-workload/Cargo.toml crates/aequora-benchkit/Cargo.toml crates/aequora-loadgen/Cargo.toml; do
    if rg -n 'sqlx|stoolap|rusqlite|tokio|axum|reqwest|quinn|dioxus|opentelemetry|prometheus' "$manifest"; then
        echo "runtime, transport, database, UI, or telemetry vendor leaked into Part 47 core" >&2
        exit 1
    fi
done

workload_count="$(find workloads/part47 -maxdepth 1 -type f -name '*.ron' | wc -l)"
test "$workload_count" -eq 8 || { echo "Part 47 requires exactly eight recommended v1 workloads" >&2; exit 1; }

rg -q 'aequora bench list' crates/aequora-cli/src/lib.rs
rg -q 'aequora capacity estimate' crates/aequora-cli/src/lib.rs
rg -q 'evidence: Unknown' crates/aequora-cli/src/lib.rs
rg -q 'BenchmarkingFull' crates/aequora-conformance/src/lib.rs
rg -q 'BenchmarkingFull' registry/core/certification.ron

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-workload -p aequora-benchkit -p aequora-loadgen \
    --all-features --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit \
    --test benchmarking_capacity_contracts --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-invariants -p aequora-conformance \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- bench list \
    | rg -q 'reconnect_storm/v1'
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- \
    capacity estimate workloads/part47/school_standard.ron | rg -q 'certified: false'

echo "benchmarking architecture: ok (${workload_count} workloads, 10 invariants)"
