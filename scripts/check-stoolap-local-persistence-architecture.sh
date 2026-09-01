#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-store-stoolap/src/config.rs
    crates/aequora-store-stoolap/src/connection.rs
    crates/aequora-store-stoolap/src/tx.rs
    crates/aequora-store-stoolap/src/domain_bridge.rs
    crates/aequora-store-stoolap/src/outbox.rs
    crates/aequora-store-stoolap/src/cursor.rs
    crates/aequora-store-stoolap/src/entity_meta.rs
    crates/aequora-store-stoolap/src/conflict.rs
    crates/aequora-store-stoolap/src/bootstrap.rs
    crates/aequora-store-stoolap/src/snapshot.rs
    crates/aequora-store-stoolap/src/integrity.rs
    crates/aequora-store-stoolap/src/repair.rs
    crates/aequora-store-stoolap/src/scheduler.rs
    crates/aequora-store-stoolap/src/fencing.rs
    crates/aequora-store-stoolap/src/migration.rs
    crates/aequora-store-stoolap/src/storage.rs
    crates/aequora-store-stoolap/src/backup.rs
    crates/aequora-store-stoolap/src/health.rs
    crates/aequora-store-stoolap/src/errors.rs
    crates/aequora-store-stoolap/src/capabilities.rs
    docs/stoolap-local-persistence-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 38 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-STOOLAP${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-STOOLAP${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-STOOLAP${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-STOOLAP${suffix}" sys-arch/38-stoolap-embedded-local-replica-client-persistence-architecture.md
done

for symbol in \
    StoolapDatabase StoolapStore StoolapLocalIdentity ClaimedOperation \
    StoolapConfig StoolapHealth StoragePreflight SecureKeyState \
    STOOLAP_LOCAL_CORE_PROFILE STOOLAP_DESKTOP_LOCAL_FULL_PROFILE \
    STOOLAP_MOBILE_LOCAL_FULL_PROFILE; do
    rg -q "\b${symbol}\b" crates/aequora-store-stoolap/src
done

for table in \
    aequora_local_store aequora_outbox aequora_outbox_dependency aequora_entity_meta \
    aequora_cursors aequora_conflicts aequora_bootstrap aequora_snapshot_stage_meta \
    aequora_integrity_checkpoint aequora_repair_state aequora_scheduler \
    aequora_coordinator_lease aequora_schema_migrations; do
    rg -q "$table" crates/aequora-store-stoolap/src/lib.rs
done

rg -q 'payload_digest' crates/aequora-store-stoolap/src/lib.rs
rg -q 'ever_sent' crates/aequora-store-stoolap/src/lib.rs
rg -q 'claim_operations' crates/aequora-store-stoolap/src/lib.rs
rg -q 'recover_stale_claims' crates/aequora-store-stoolap/src/lib.rs
rg -q 'transact_local_mutation' crates/aequora-store-stoolap/src/lib.rs
rg -q 'reconcile_stoolap' crates/aequora-store-stoolap/src/lib.rs

if rg -n 'stoolap|aequora-store-stoolap' \
    crates/aequora-adapter-sdk/Cargo.toml crates/aequora-store/Cargo.toml; then
    echo "Stoolap dependency leaked into a neutral storage contract crate" >&2
    exit 1
fi
if rg -n 'stoolap::|ApiTransaction|StoolapDatabase' \
    crates/aequora-adapter-sdk/src crates/aequora-store/src; then
    echo "Stoolap driver type leaked into a neutral storage API" >&2
    exit 1
fi

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-store-stoolap --lib --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-invariants -p aequora-conformance --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit --test storage_adapter_sdk_contracts \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --offline --bin aequora-registry \
    -- verify .

echo "Stoolap local persistence architecture checks passed"
