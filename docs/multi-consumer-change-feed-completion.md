# Part 28 multi-consumer change-feed completion

Detailed design authority remains
[`sys-arch/28-multi-consumer-change-feed-architecture.md`](../sys-arch/28-multi-consumer-change-feed-architecture.md).
This document records implementation locations and verification only.

## Implementation map

- `aequora-feed` provides stable consumer/group/worker/partition IDs and numeric kinds/versions;
  independent authority-bound cursors; registration and lifecycle; server-controlled tenant filters;
  bounded batches; deterministic projector and dedup contracts; durable-effect-before-ACK; fenced
  leases; ordering/partitioning; lag; floor/epoch recovery; quarantine; reset/rebuild plans; archive
  and batch integrity; minimized external integration events; broker and storage traits; governance
  surfaces; admin/incident views; and `AEQ-INV-FEED001` through `AEQ-INV-FEED009`.
- `aequora-journal::CompactionInputs` includes the slowest justified `PinJournal` consumer without
  allowing ordinary rebuildable consumers to pin history forever.
- `config/change-feed.ron` is a validated search-consumer example. The facade, CLI, testkit, Guppy
  rule, structural gate, and CI expose and enforce the implementation without selecting PostgreSQL,
  Stoolap, Tokio, or a broker in the core.
- `docs/change-feed-event-contract.md` defines the current visibility and compatibility boundary.
  Side-effect consumers materialize Part 23 jobs; governance/storage adapters register derived copies.

Logical metadata table names and neutral `ConsumerStore`/`ChangeFeedSource` contracts are defined for
PostgreSQL or other adapters. Physical DDL, read pools/replicas, broker infrastructure, object storage,
tenant residency placement, SLO thresholds, and 10–100 consumer load acceptance remain deployment or
adapter responsibilities and cannot be truthfully claimed by the neutral core.

## Verification record

Verified on 2026-08-29 from the combined Part 27/28 working tree:

- 9 focused feed tests and 5 cross-crate contracts passed, including filtered cursor progress,
  durable ACK, duplicate identity, floor/epoch recovery, ordering, cross-tenant visibility, archive,
  governance, retention compaction, invariant registry, and destructive reset authorization;
- workspace all-target/all-feature Clippy passed with warnings denied;
- 429 sandbox-compatible all-feature tests passed with one ignored; the non-socket HTTP and QUIC
  tests passed separately, while their loopback tests remain unavailable under sandbox socket policy;
- rustdoc passed across the workspace with warnings denied;
- the change-feed architecture gate, 22-rule/61-crate Guppy boundary check, all four database-neutral
  profiles, locked standalone fuzz build, formatting, and diff hygiene passed.
