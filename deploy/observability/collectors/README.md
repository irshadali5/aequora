# Collector boundary

Use a local or regional OpenTelemetry-compatible collector with bounded memory and batch queues.
Export metrics, structured logs, and sampled traces independently. Collector outage must not fail
readiness or block synchronization. Air-gapped profiles point exporters only at internal collectors;
zero-telemetry profiles disable remote export while retaining local diagnostics and durable audit.

Never enable raw SQL statement values, HTTP authorization headers, cookies, operation payloads, or
unclassified resource attributes in automatic instrumentation.

