# Kubernetes reference profile

Replace the image placeholder with a verified immutable digest and create the secret-free
`aequora-config` ConfigMap. Supply secrets through a secret-provider integration, not a ConfigMap.
The checked-in `NetworkPolicy` is intentionally deny-by-default; add narrowly scoped ingress from
the trusted gateway and egress to internal DNS, the PostgreSQL writer, object storage, identity,
KMS, and telemetry collectors used by your profile. API pods require no persistent volume.

Readiness must validate configuration, registry, migration compatibility, database connectivity,
authority metadata, and adapter capabilities. Liveness should remain process-local so a temporary
remote outage does not create a restart storm. Configure graceful termination to mark unready and
drain bounded requests before the platform sends its final termination signal.

