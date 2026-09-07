# Part 45 deployment topology completion

Part 45 is implemented by `aequora-deployment`, a secret-free descriptor and validation crate that
keeps deployment choices separate from synchronization semantics. It provides topology profiles,
single-writer scope validation, production and air-gap readiness, connection and timeout budgets,
regional consistency routing, failover epoch decisions, and deterministic fleet-drift diagnostics.

`aequora doctor deployment <descriptor.ron>` validates a bounded descriptor without connecting to
infrastructure or printing secrets. Reference assets cover local, OCI, systemd, Kubernetes, and
air-gapped operation. The restore/promotion and rolling-upgrade runbooks make epoch continuity,
fencing, immutable promotion, and migration ownership explicit.

The implementation is intentionally free of SQL drivers, cloud SDKs, container/Kubernetes clients,
Redis, Kafka, NATS, service-mesh APIs, and runtime transports. PostgreSQL remains the authoritative
writer; SQLite and Stoolap remain client-local adapters.

