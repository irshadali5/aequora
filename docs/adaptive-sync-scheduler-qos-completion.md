# Part 06 Adaptive Sync Scheduler and QoS Completion

This record maps `06-adaptive-sync-scheduler-qos.md` to repository-owned implementation and
verification. It covers the reusable scheduler and its client integration. It does not claim that
an application's Android, iOS, desktop, carrier, battery, or background-execution integration has
passed deployment acceptance.

## Implemented contracts

| Requirement | Repository evidence |
|---|---|
| Correctness boundary | `aequora-scheduler` is a database- and platform-neutral planner. Work descriptors carry no payload, and scheduling cannot rewrite operation identity, meaning, dependencies, authorization, authoritative order, or durable state. |
| Classification | `WorkClass`, `WorkKind`, `OperationPriorityMetadata`, and `OperationPriorityRegistry` provide trusted defaults plus bounded caller hints. Bootstrap, integrity, repair, compaction, blob, and maintenance work have conservative built-in classes. |
| Normalized context | `SchedulingContext` combines network, metering, roaming, bandwidth, power, activity, pressure, server hints, authentication, storage health, leadership, pause state, and remaining execution budget. Unknown platform signals remain explicit and normalization clamps untrusted values. |
| Eligibility | Pure policy decisions defer work for pause, offline state, invalid leadership, missing authentication, unhealthy storage, retry deadlines, unmet dependencies, constrained-network policy, low power, server background pauses, and expiring execution budgets. Only explicitly certified critical work may bypass network/power deferral. |
| Fair priority | Effective priority combines class, bounded aging, and transitive dependency inheritance. A deterministic weighted cycle reserves opportunities for every class, including background and maintenance work. |
| Adaptive execution | A count-and-byte AIMD controller grows additively after success and halves on overload. Local hard bounds always cap server preferences, bandwidth/pressure reductions, concurrency, and client push batches. Compression decisions trade CPU and bandwidth without changing payload semantics. |
| Retry and overload | Stable deterministic retry jitter, capped server retry hints, restart-safe circuit-breaker state, and bounded server backoff cooperate with the existing durable outbox retry records. Transient transport failures reduce both compatibility and scheduler batch controllers. |
| Client lifecycle | `ClientSyncEngine` gates push, bootstrap, streaming bootstrap, and retry execution through the scheduler; exposes normalized context updates, pause/resume, next-work-class requests, and a serializable scheduler snapshot; and fails closed for follower execution. Coordinator triggers distinguish interactive, normal, and periodic background work. |
| Persistence | `SchedulerState` serializes controller targets, circuit state, bounded backoff, accounting, and the fairness cursor. A builder can restore matching-version state and normalizes future deadlines. Durable work is never copied into scheduler state and remains in its owning store. |
| Profiles and configuration | Strict RON configuration embeds validated `SchedulerPolicy`. Desktop, Mobile, HighLatency, LowBandwidth, and EnterpriseLan profiles provide conservative defaults while retaining explicit hard bounds. |
| Invariants | `AEQ-INV-QOS001` through `QOS006` register semantic safety, durable-intent preservation, hard batch bounds, starvation freedom, server-hint safety, and leadership/retry safety. |
| Operations | Payload-free scheduler metrics expose selections, deferrals, selected batch count/bytes, and overload backoffs. `aequora-dev scheduler explain` and `scheduler status <state.ron>` explain policy and persisted controller state. |

## Verification evidence

- Scheduler unit/property tests cover constrained contexts, dependency inheritance, weighted
  background opportunity, restart-safe circuit state, bounded server hints, deterministic
  descriptor preservation, capped retry hints, and hard count/byte bounds.
- Client/testkit contracts prove a paused scheduler performs no transport exchange and does not
  mutate durable local state; profile policies validate; normalized context round-trips through
  the portable codec.
- Client integration retains the existing operation-byte packing limit and additionally caps every
  exchange by the current scheduler count/byte decision.
- Repository gates cover formatting, strict all-feature Clippy, workspace tests and rustdoc, Guppy
  dependency direction, and the independent database-neutrality policy.

## Deliberate host responsibilities

Hosts collect OS connectivity, metering, roaming, power, activity, and remaining-background-time
signals, normalize them into `SchedulingContext`, and persist `SchedulerState` with their local
metadata transaction strategy. Hosts also decide which domain operation kinds may request urgent
priority, obtain trusted server hints from their negotiated transport, and arrange wakeups after
deferral deadlines. Device acceptance must exercise foreground/background transitions, suspend and
resume, clock anomalies, carrier changes, data/energy budgets, process takeover, and crash/restart
on every shipped platform. Unknown signals and failed signal providers must retain conservative
defaults rather than prevent correctness-critical synchronization.
