# Part 46 observability completion

Part 46 extends `aequora-observability` with a vendor-neutral production contract rather than
binding core synchronization to a collector or monitoring product. Stable metric identities and
closed attribute keys prevent unbounded labels. Structured fields are classified and sanitized
before retention. Request, trace, correlation, and operation identities remain separate.

The bounded queue uses non-blocking admission, priority-aware eviction, finite batches, and
self-metrics. Export failure drops telemetry only; it cannot roll back Tx A, Tx B, Tx C, or hold an
authority transaction. `NoopMetrics` and disabled/local-only export modes preserve zero-telemetry
operation.

Sampling policy always retains critical security hints, can retain errors/slow traces, and rejects
sensitive remote traces. Monotonic timers measure latency. SLO eligibility distinguishes accepted
and infrastructure outcomes from validation, authorization, conflicts, and business decisions.
Stable alert definitions require owner, actionable condition, bounded grouping, and runbook.
`AequoraConfig` carries and validates `ObservabilityConfig`, so the same bounded production defaults
participate in strict RON parsing and every named deployment profile.

Reference assets under `deploy/observability` provide validated SLO/alert catalogs, five initial
dashboards, Prometheus-compatible rules, collector guidance, and runbooks. The operational CLI
provides bounded summary/metric-catalog output and explicitly falls back from expired traces to the
durable ledger, journal, audit, jobs, and authority metadata.
