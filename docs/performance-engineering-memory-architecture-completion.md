# Part 19 performance engineering and memory architecture evidence

This document maps `sys-arch/19-performance-engineering-memory-architecture.md` to executable
repository surfaces. It characterizes bounded behavior; it does not claim universal throughput or
replace deployment-specific load, soak, database-plan, mobile, and capacity acceptance.

| Requirement | Implementation evidence |
|---|---|
| Memory profiles and budgets | `aequora-performance::PerformancePolicy` supplies validated mobile, desktop, standard-server, and high-throughput profiles with request, decode, response, snapshot-pipeline, cache, ready-queue, pending-record, CPU, stream, and reactive-view bounds. `AequoraConfig` validates wire/snapshot settings against those budgets. |
| Low-copy framing | `aequora-codec` constructs `BytesMut`, freezes to `Bytes`, exposes a stable borrowed frame view, and supports borrowed uncompressed Postcard decode whose lifetime is tied to the frame. HTTP request/response handoff uses immutable `Bytes`. |
| Early frame limits | `inspect_header` checks magic, flags, kind, and declared length from the fixed header. Axum reads incrementally after authenticated admission and rejects an excessive declaration or actual body before domain decode/execution. |
| Bounded CPU work | `ComputePool` owns a dedicated fixed-size Rayon pool, a measured parallel threshold, and a non-waiting hard submission semaphore. Saturation rejects before Rayon entry and RAII releases capacity after completion or abandonment. |
| Snapshot streaming | `SnapshotManifest::build_streaming` accepts sorted records and emits one bounded chunk to a durable sink while retaining only the current chunk plus the bounded descriptor manifest. The existing complete-result helper remains for tests and small inputs. |
| Blob streaming | `BlobUploadSource`, `StreamingBlobStore`, and `BlobDownloadSink` hash, upload, publish, read, and verify one immutable bounded chunk at a time. Normal sync carries `BlobRef`; staged chunks stay invisible until commit. |
| Client/UI memory | `PagedView` enforces a maximum page and `InvalidationCoalescer` coalesces duplicate keys, falling back to one full query refresh at its bound instead of growing reactive state. |
| Immutable hot state | `ImmutableRegistry` builds one non-zero generation in a shared `Arc<BTreeMap<_, _>>`; reads do not take a mutable lock and replacement constructs another version. |
| Database/query shape | `HOT_QUERY_CONTRACTS` declares bounded provider-neutral journal cursor, pending/due outbox, entity-version, and snapshot-page index shapes. PostgreSQL and Stoolap retain adapter-owned prepared/indexed implementations; performance characterization remains separate from semantic certification. |
| Benchmarks and regression | Versioned fixed-seed RON workloads cover school-day, mass-reconnect, large-bootstrap, and low-bandwidth profiles. Reports require CPU/RAM/OS/database/commit/durability metadata and unique phase metrics. `compare_reports` uses noise-tolerant relative thresholds and attributes failures to one phase. |
| Profiling and allocator policy | The `profiling` Cargo profile keeps optimized symbols. `performance/README.md` defines flamegraph, heap, database explain, copy-budget, allocator, unsafe, and SIMD approval policy. |
| Invariants and architecture gates | `AEQ-INV-PERF001` through `AEQ-INV-PERF009` are stable registry entries. `scripts/check-performance-architecture.sh`, focused tests, Guppy, database-neutrality, strict Clippy, and workspace tests enforce the implementation. |

## Ownership boundary

```text
admitted bounded frame -> immutable bytes / borrowed typed validation
                       -> owned execution fields -> dense dependency plan
                       -> bounded I/O or CPU resource -> durable commit
                       -> immutable response or bounded large-object stream
                       -> bounded reconciliation -> local database -> paged UI query
```

Performance pressure may pause snapshots, maintenance, cache retention, or new bulk work. It cannot
skip authorization, integrity, audit, idempotency, transaction, cursor, epoch, or governance work.

## Deployment acceptance still required

- capture baselines and SLOs on each supported client/server platform and durability mode;
- run PostgreSQL/Stoolap query-plan and representative-data benchmarks;
- execute low-memory Android/desktop tests, mass reconnect, large bootstrap, and multi-hour soak;
- record peak RSS, allocator retention, p95/p99 latency, throughput, I/O, query count, and failures;
- approve higher limits only from measured capacity and keep semantic certification independent.
