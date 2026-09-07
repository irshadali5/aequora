#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-deployment/Cargo.toml
    crates/aequora-deployment/src/lib.rs
    crates/aequora-testkit/tests/deployment_topology_contracts.rs
    deploy/profiles/small-production.ron
    deploy/container/Containerfile
    deploy/systemd/aequora-server.service
    deploy/kubernetes/aequora.yaml
    deploy/kubernetes/README.md
    deploy/air-gapped/README.md
    deploy/runbooks/rolling-upgrade.md
    deploy/runbooks/restore-and-promotion.md
    docs/deployment-topologies-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 45 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-DEPLOY${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-DEPLOY${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-DEPLOY${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-DEPLOY${suffix}" sys-arch/45-*.md
done

for symbol in \
    DeploymentDescriptor DeploymentId TopologyKind OperationalMode AuthorityBinding \
    DeploymentNode DeploymentCapability PublicDependency ConnectionBudget TimeoutBudget \
    ReadConsistency ReadTarget ContinuityEvidence EpochAction DoctorReport; do
    rg -q "\b${symbol}\b" crates/aequora-deployment/src/lib.rs
done

if rg -n 'sqlx|stoolap|rusqlite|tokio|axum|reqwest|quinn|dioxus|kube|redis|kafka|nats' \
    crates/aequora-deployment/Cargo.toml; then
    echo "database, runtime, transport, UI, or infrastructure dependency leaked into neutral deployment contracts" >&2
    exit 1
fi

rg -q 'aequora doctor deployment' crates/aequora-cli/src/lib.rs
rg -q 'DeploymentTopologyFull' crates/aequora-conformance/src/lib.rs
rg -q 'DeploymentTopologyFull' registry/core/certification.ron
rg -q 'runAsNonRoot: true' deploy/kubernetes/aequora.yaml
rg -q 'readOnlyRootFilesystem: true' deploy/kubernetes/aequora.yaml
rg -q 'NoNewPrivileges=yes' deploy/systemd/aequora-server.service

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-deployment --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit \
    --test deployment_topology_contracts --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-invariants -p aequora-conformance \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- \
    doctor deployment deploy/profiles/small-production.ron | rg -q 'deployment status: ready'
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --offline --bin aequora-registry -- \
    verify .

echo "deployment topology, readiness, recovery, air-gap, and reference environment checks passed"
