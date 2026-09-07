#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-observability/src/part46.rs
    crates/aequora-testkit/tests/observability_production_contracts.rs
    deploy/observability/README.md
    deploy/observability/alerts/catalog.ron
    deploy/observability/alerts/prometheus-rules.yaml
    deploy/observability/slos/catalog.ron
    deploy/observability/dashboards/service-health.json
    deploy/observability/dashboards/sync-pipeline.json
    deploy/observability/dashboards/postgresql-authority.json
    deploy/observability/dashboards/jobs-background.json
    deploy/observability/dashboards/release-compatibility.json
    deploy/observability/runbooks/authority-commit-failure.md
    deploy/observability/runbooks/database-saturation.md
    deploy/observability/runbooks/sync-availability-burn.md
    deploy/observability/runbooks/authority-continuity.md
    docs/observability-production-telemetry-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 46 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-OBS${suffix}" crates/aequora-invariants/src/lib.rs
    rg -q "AEQ-INV-OBS${suffix}" crates/aequora-conformance/src/lib.rs
    rg -q "AEQ-INV-OBS${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-OBS${suffix}" sys-arch/46-*.md
done

for symbol in \
    MetricId MetricsSink NoopMetrics Attributes AttributeKey SeriesBudget \
    BoundedTelemetryQueue TelemetryExporter TelemetryQueueSnapshot SanitizedField \
    StructuredEvent TelemetryTraceContext SamplingPolicy OperationOutcomeClass \
    TraceParent EventRateLimiter ClientTelemetryOwner SloDefinition SloCatalog \
    AlertDefinition AlertCatalog MonotonicTimer ObservabilityConfig; do
    rg -q "\b${symbol}\b" crates/aequora-observability/src/part46.rs
done

if rg -n 'opentelemetry|prometheus|grafana|datadog|sqlx|stoolap|rusqlite|tokio|axum|reqwest|quinn|dioxus' \
    crates/aequora-observability/Cargo.toml; then
    echo "exporter, vendor, runtime, UI, or database dependency leaked into observability contracts" >&2
    exit 1
fi

for alert in AEQ-ALERT-AUTHORITY-001 AEQ-ALERT-DB-002 AEQ-ALERT-SYNC-003 AEQ-ALERT-AUTHORITY-004; do
    rg -q "$alert" deploy/observability/alerts/catalog.ron
    rg -q "$alert" deploy/observability/alerts/prometheus-rules.yaml
done

rg -q 'aequora diagnostics metrics' crates/aequora-cli/src/lib.rs
rg -q 'pub observability: ObservabilityConfig' crates/aequora-config/src/lib.rs
rg -q 'self.observability.validate()' crates/aequora-config/src/lib.rs
rg -q 'ObservabilityFull' crates/aequora-conformance/src/lib.rs
rg -q 'ObservabilityFull' registry/core/certification.ron
rg -q 'SECRET_DO_NOT_LEAK_123' crates/aequora-testkit/tests/observability_production_contracts.rs

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-observability --all-features --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-config --all-features --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit \
    --test observability_production_contracts --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-invariants -p aequora-conformance \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- \
    diagnostics metrics | rg -q 'aequora_telemetry_dropped_total'
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-registry-cli --offline --bin aequora-registry -- \
    verify .

echo "bounded metrics, tracing, redaction, SLO, alert, and telemetry export checks passed"
