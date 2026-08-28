#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-COMP${suffix}" crates/aequora-invariants/src/lib.rs
done

for module in capability feature framing negotiation operation policy registry version; do
    test -f "crates/aequora-compat/src/${module}.rs"
done

test -f crates/aequora-compat/registry/compatibility.ron
rg -q 'pub fn negotiate' crates/aequora-compat/src/negotiation.rs
rg -q 'pub trait OperationUpcaster' crates/aequora-compat/src/operation.rs
rg -q 'RetryOnly' crates/aequora-compat/src/registry.rs
rg -q 'FleetCapabilityIncomplete' crates/aequora-compat/src/feature.rs
rg -q 'CompatibilityNegotiationV1' crates/aequora-protocol/src/lib.rs
rg -q 'client_hello_postcard_v1_golden_bytes_are_stable_and_decodable' crates/aequora-testkit/tests/compatibility_contracts.rs
rg -q 'required_capability_and_protocol_downgrades_fail_closed' crates/aequora-testkit/tests/compatibility_contracts.rs
rg -q 'retry_only_accepts_only_the_ledger_identical_historical_operation' crates/aequora-testkit/tests/compatibility_contracts.rs

echo "compatibility-architecture: ok (8 modules, stable registry, 9 invariants, evolution contracts)"
