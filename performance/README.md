# Aequora performance evidence

Part 19 performance work is accepted only when it preserves the semantic test suite and is tied to
a fixed workload under `performance/workloads/`. Throughput alone is not a correctness or adapter
certification result.

## Measurement layers

- Micro: Postcard framing/borrowed decode, canonical hashing, dependency planning, compaction, and
  routing.
- Component: bounded 10k-operation planning, large reconciliation, snapshot construction, and blob
  transfer.
- Database: real adapter transaction throughput, journal cursor reads, due outbox scans, entity
  version lookups, and snapshot pages.
- End to end: local mutate, transport, authoritative commit, response, and local reconciliation.
- Soak: bounded queues, RSS/heap divergence, connections, tasks, and throughput drift over hours.

Shared CI compiles benchmark targets and checks policy contracts. Noise-sensitive regression gates
belong on a stable dedicated runner. The default comparison threshold is 10 percent per measured
phase; a project-specific SLO may be stricter after a baseline exists.

## Reproduction and profiling

Record CPU, RAM, OS, database/version, Aequora commit, workload digest, and durability mode in every
report. Use `cargo build --profile profiling` for optimized symbols, capture a flamegraph or heap
profile while replaying the exact fixed seed, attribute the change to `decode`, `authorization`,
`validation`, `planning`, `database`, `encode`, `reconciliation`, `snapshot`, or `blob`, then compare
before and after reports:

```text
cargo run -p aequora-dev -- performance workload-verify performance/workloads/school-day-morning.ron
cargo run -p aequora-dev -- performance report-verify report.ron
cargo run -p aequora-dev -- performance compare baseline.ron candidate.ron
```

Database investigations use `EXPLAIN (ANALYZE, BUFFERS)` on representative data. Hot contracts are
tenant/scope/sequence journal pages, state/priority/local-sequence outbox pages, state/next-retry
retry pages, tenant/entity/version lookup, and tenant/snapshot/entity-order pages. Fetch only required
columns, cap result counts and bytes, use prepared adapter queries, and keep transactions bounded.

## Ownership and copy budget

```text
socket -> bounded frame -> immutable Bytes -> borrowed validation view
       -> owned typed execution fields -> durable commit -> immutable/streamed response

object source -> one bounded chunk -> hash/compress/encrypt -> durable staging -> release
```

Snapshots, blobs, and exports use the second path. Compatibility helpers that accept a complete byte
slice remain useful for tests and small inputs but are not the production large-object contract.
Normal operations carry `BlobRef`, never an unbounded binary payload.

The system allocator remains the portable default. Buffer pools, arenas, alternative allocators,
weak hashers, NUMA affinity, and hand-written SIMD require workload evidence and a measured win.
Workspace core crates forbid unsafe Rust; an exception requires an isolated documented module,
fuzzing/tests, review, and before/after evidence. BLAKE3 and zstd provide their own optimized code.

UI integrations retain a bounded query-derived page plus coalesced invalidations and re-query the
local database. They do not mirror the complete synchronized database in reactive state.
