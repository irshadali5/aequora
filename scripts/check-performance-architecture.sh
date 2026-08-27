#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-PERF${suffix}" crates/aequora-invariants/src/lib.rs
done

rg -q 'crates/aequora-performance' Cargo.toml
rg -q '"tokio"' crates/aequora-dev/src/main.rs
rg -q 'pub fn inspect_header' crates/aequora-codec/src/lib.rs
rg -q 'pub fn decode_borrowed' crates/aequora-codec/src/lib.rs
rg -q 'pub fn build_streaming' crates/aequora-bootstrap/src/lib.rs
rg -q 'pub async fn upload_stream' crates/aequora-blob/src/lib.rs
rg -q 'pub async fn download_stream' crates/aequora-blob/src/lib.rs
rg -q 'max_queued_jobs' crates/aequora-compute/src/lib.rs
rg -q 'pub struct WorkloadManifest' crates/aequora-performance/src/lib.rs

workload_count="$(find performance/workloads -maxdepth 1 -type f -name '*.ron' | wc -l)"
if [[ "$workload_count" -lt 4 ]]; then
    echo "performance architecture requires representative fixed-seed workload manifests" >&2
    exit 1
fi

echo "performance-architecture: ok (${workload_count} workloads, 9 invariants)"
