# Vendor-neutral observability reference

This directory contains the initial Part 46 operational surface. The RON catalogs are validated by
`aequora-observability`; Prometheus-compatible rules and Grafana-compatible dashboards are adapters
around the stable semantic metric names. Replace deployment labels and data-source identities at
promotion time without changing alert IDs or SLO definitions.

Metrics endpoints must remain private or authenticated. Remote exporters use bounded queues and
may be disabled; durable audit, operation ledger, journal, jobs, authority metadata, and local
diagnostics remain the forensic truth in every mode.

