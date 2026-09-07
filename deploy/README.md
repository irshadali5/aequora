# Aequora reference deployments

These examples implement Part 45 without making an orchestrator part of Aequora correctness.
All production profiles require an external authoritative PostgreSQL/Neon service and external
object storage. API nodes are replaceable and never store authoritative state on local disks.

Before deployment, copy and complete `profiles/small-production.ron`, then run:

```bash
aequora doctor deployment deploy/profiles/small-production.ron
```

The descriptor is secret-free. Supply database, TLS, identity, object-store, and KMS credentials
through the Part 43 secret-provider boundary. Never place secret values in these files.

Reference targets:

- `local/` — developer setup guidance;
- `container/` — hardened non-root OCI image;
- `systemd/` — native Linux unit;
- `kubernetes/` — API, worker, disruption, and deny-by-default network examples;
- `air-gapped/` — signed offline import procedure;
- `runbooks/` — upgrades, restore, and authority promotion.

