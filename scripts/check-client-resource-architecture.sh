#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root_dir"

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-CLIENT${suffix}" crates/aequora-invariants/src/lib.rs
done

for module in context profile memory storage network power thermal admission events; do
    test -f "crates/aequora-client/src/resources/${module}.rs"
done

rg -q 'pub trait PlatformResourceMonitor' crates/aequora-client/src/resources/context.rs
rg -q 'pub trait ClientResourceAdmission' crates/aequora-client/src/resources/admission.rs
rg -q 'pub struct DurableWorkCheckpoint' crates/aequora-client/src/resources/events.rs
rg -q 'UnsyncedOutbox' crates/aequora-client/src/resources/storage.rs
rg -q 'ResourceConstrainedV1' crates/aequora-protocol/src/lib.rs
rg -q 'very_low_memory_caps_every_bounded_pipeline' crates/aequora-testkit/tests/client_resource_contracts.rs
rg -q 'disk_full_preflight_and_local_save_fail_closed' crates/aequora-testkit/tests/client_resource_contracts.rs
rg -q 'process_kill_cannot_advance_an_uncommitted_cursor' crates/aequora-testkit/tests/client_resource_contracts.rs

echo "client-resource-architecture: ok (9 modules, 9 invariants, fault-profile contracts)"

