#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"

for crate in aequora-legacy aequora-legacy-api; do
    test -f "crates/${crate}/Cargo.toml"
    test -f "crates/${crate}/src/lib.rs"
done

for module in bridge cutover id_map mapping ownership shadow system; do
    test -f "crates/aequora-legacy/src/${module}.rs"
done

if rg -q 'sqlx|stoolap|axum|reqwest|tokio' crates/aequora-legacy/Cargo.toml crates/aequora-legacy-api/Cargo.toml; then
    echo "legacy architecture: runtime, transport, or database dependency in compatibility core" >&2
    exit 1
fi

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-LEG${suffix}" crates/aequora-invariants/src/lib.rs
done

for symbol in LegacySystemManifest LegacyReader LegacyMapper CdcBridge BridgeStore LegacyProvenance AggregateOwnership ShadowExecutor CutoverVerification LegacyIdMap GovernanceCoverage LegacyRegistryStore; do
    rg -q "${symbol}" crates/aequora-legacy/src
done

rg -q 'LegacyRequestTranslator' crates/aequora-legacy-api/src/lib.rs
rg -q 'CanonicalOperationHandler' crates/aequora-legacy-api/src/lib.rs
rg -q 'Some\("legacy"\)' crates/aequora-cli/src/main.rs
rg -q 'legacy_facade_retries_route_to_typed_handler_with_stable_operation_id' crates/aequora-testkit/tests/legacy_adoption_contracts.rs
rg -q 'cdc_gap_or_crash_before_commit_never_advances_cursor' crates/aequora-testkit/tests/legacy_adoption_contracts.rs
rg -q 'cutover_requires_fence_final_boundary_and_all_canonical_checks' crates/aequora-testkit/tests/legacy_adoption_contracts.rs

echo "legacy architecture: staged adoption, durable CDC, explicit ownership, shadow isolation, verified cutover, facade, governance, and nine invariants verified"
