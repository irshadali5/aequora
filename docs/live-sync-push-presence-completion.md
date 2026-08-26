# Part 08 Live Sync, Push Hints, and Presence Completion

Part 08 is implemented at Aequora's reusable repository boundary. Live delivery accelerates the
normal durable exchange; it is not a second replication protocol and it is never required for
convergence.

## Repository guarantees

| Requirement | Evidence |
|---|---|
| Additive hint-only protocol | `aequora-live::SyncHint`, `LiveMessage`, `LIVE_PROTOCOL_V1`, and additive `Capability::LiveV1` |
| Transport independence | `LiveHintTransport` and `LiveHintStream`; WebSocket, SSE, mobile push, IPC, and QUIC notification adapters stay outside the core |
| Broker independence | `HintBroker`, `HintSubscription`, `InMemoryHintBroker`, and `PostCommitHintPublisher` |
| Post-commit degradation | `publish_best_effort` returns `Published` or `Degraded`; broker failure is not a transaction error |
| Scope isolation | `LiveRouter` authorizes every initial/dynamic subscription and matches exact tenant plus scope |
| Revocation | `revoke_scope` removes routing eligibility before later delivery; durable request authorization remains authoritative |
| Bounded backpressure | `LatestHintQueue` retains at most one monotonically merged hint per scope and enforces a hard scope bound |
| Duplicate/reorder/loss safety | `HintWakeTracker` coalesces stale hints and changes only a wake generation; it owns no cursor/store API |
| Sync race safety | `begin_exchange`/`needs_follow_up` detect hints received during a normal exchange |
| Reconnect and leadership | `ReconnectPolicy` caps exponential delay; `LiveLeadership` binds socket ownership to the current fencing epoch; acquisition requires durable catch-up |
| Cross-node initial adapter | `PostgresNotifyHintBroker` implements ephemeral hex/Postcard `LISTEN/NOTIFY` without entering core crates |
| Presence | `PresenceDirectory` is bounded, tenant-isolated, privacy-filtered by host policy, TTL-expiring, and intentionally restart-empty |
| Scheduler | `SyncCoordinatorHandle::observe_live_hint`, `SyncTrigger::PushHint`, and `WorkKind::LiveCatchUp` schedule the established reconciliation path |
| Diagnostics | `MetricEvent::Live`, aggregate live/presence metrics, and `aequora-dev live explain` |
| Normative invariants | `AEQ-INV-LIVE001` through `AEQ-INV-LIVE005` in `aequora-invariants` |

The existing `SyncTransport` HTTP/QUIC exchange request and response bodies are unchanged. The
new live capability is append-only and separately negotiated, so peers without Part 08 continue
polling through protocol v1.

## Executable evidence

`aequora-live` tests prove compact payload-free encoding, hard queue bounds, 100,000-event
slow-consumer coalescing, tenant/scope isolation, revocation routing removal, duplicate and reordered
hint behavior, exchange-race follow-up, optional broker behavior, and presence expiry. The testkit
`live_contracts` suite injects loss, duplication, reordering, delay, and disconnect. Existing full
workspace HTTP and QUIC suites prove the durable fallback path remains compatible.

## Host and deployment responsibilities

A host must still:

- authenticate each WebSocket/SSE/mobile connection and implement `LiveScopeAuthorizer` from its
  real authorization model;
- publish only after the authoritative journal transaction commits;
- retain periodic safety polling and run immediate cursor catch-up after connect or leadership
  acquisition;
- map WebSocket binary frames, SSE base64/text frames, and provider-specific silent mobile pushes
  without adding domain payloads;
- refresh credentials by reconnecting and close revoked sessions promptly, while continuing to
  enforce authorization on every durable request;
- define presence visibility, TTL, activity-rate limits, and approximate UI language;
- configure tenant/device/connection/scope quotas and prove reconnect bursts, broker outages,
  memory, and latency under expected production load;
- run live PostgreSQL NOTIFY acceptance when PostgreSQL is the selected broker. Redis, NATS, or
  another broker can replace it without protocol changes.

These are deployment acceptance gates, not missing database-neutral engine behavior.
