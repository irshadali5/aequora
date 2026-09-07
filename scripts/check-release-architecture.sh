#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-release/Cargo.toml
    crates/aequora-release/src/lib.rs
    crates/aequora-release/src/bin/aequora-release.rs
    crates/aequora-update/src/lib.rs
    crates/aequora-testkit/tests/release_engineering_contracts.rs
    release/support-matrix.ron
    release/README.md
    docs/packaging-distribution-release-engineering-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 44 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-RELEASE${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-RELEASE${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-RELEASE${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-RELEASE${suffix}" sys-arch/44-*.md
done

for symbol in \
    ReleaseManifest ArtifactDescriptor ArtifactHashes BuildProvenance CompatibilityMatrix \
    MigrationBundleDescriptor SignedReleaseManifest ReleaseTrustStore UpdateMetadata \
    SignedUpdateMetadata ReleaseChannel AutomaticUpdatePolicy RollbackClass SupportMatrix \
    PromotionEvidence ImmutableReleaseIndex; do
    rg -q "\b${symbol}\b" crates/aequora-release/src/lib.rs
done
for symbol in AtomicUpdateState UpgradeCheckpoint UpdateStage; do
    rg -q "\b${symbol}\b" crates/aequora-update/src/lib.rs
done

if rg -n 'sqlx|stoolap|rusqlite|tokio|axum|reqwest|quinn|dioxus' \
    crates/aequora-release/Cargo.toml crates/aequora-update/Cargo.toml; then
    echo "runtime, transport, UI, or physical database dependency leaked into neutral release contracts" >&2
    exit 1
fi

for purpose in ReleaseArtifactSigning ReleaseManifestSigning UpdateMetadataSigning \
    ContainerSigning PlatformApplicationSigning GitTagSigning; do
    rg -q "\b${purpose}\b" crates/aequora-crypto/src/key.rs
done

rg -q 'aequora release verify' crates/aequora-cli/src/lib.rs
rg -q 'ReleaseEngineeringFull' crates/aequora-conformance/src/lib.rs
rg -q 'ReleaseEngineeringFull' registry/core/certification.ron

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-release -p aequora-update --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit \
    --test release_engineering_contracts --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-invariants -p aequora-conformance \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-release --bin aequora-release --locked --offline -- \
    support-matrix release/support-matrix.ron | rg -q 'support-matrix: ok'
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- help | \
    rg -q 'aequora release verify'
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --offline --bin aequora-registry -- \
    verify .

echo "packaging, signing, update-channel, and cross-platform release architecture checks passed"
