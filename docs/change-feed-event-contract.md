# Authoritative Change-Feed Event Contract

The governing architecture is
[`sys-arch/28-multi-consumer-change-feed-architecture.md`](../sys-arch/28-multi-consumer-change-feed-architecture.md).
The authoritative journal remains the source; feed events and every projection are derived.

`ChangeFeedEvent` preserves stable `EventId`, authority epoch, sequence, tenant, entity/version,
event/schema kind, payload digest, operation, correlation, and occurrence time. Event kinds `1` and
`2` currently map to canonical upsert and tombstone journal changes. Part 29 owns future numeric
registry governance. Unknown required kinds or schemas fail activation; optional unrelated events
may be filtered only through registered server-side policy.

Internal events default to `InternalOnly`. External integrations receive a separate minimized,
versioned `IntegrationEvent`; raw internal journal payloads are never automatically public. Public
and tenant integration events require explicit visibility and tenant authorization. JSON is an edge
interoperability representation; the neutral Rust contract does not make JSON authoritative.

Delivery is at least once. `EventId` plus payload digest is the dedup identity. Ordered consumers ACK
one contiguous inspected boundary only after durable processing receipts. Irrelevant filtered events
may advance the inspected boundary after the deterministic filter decision. External side effects
are materialized as Part 23 jobs/intents rather than executed inside projection checkpoints.

Schema, filter, projection, and rule versions are independent. Blue/green upgrades use another
consumer identity/cursor, rebuild from snapshot plus tail, compare projection digests and queries,
then switch downstream traffic only after verification. Retired consumer IDs are never reused.
