# `plan.md` completion audit

This matrix treats the numbered plan as the release specification. “Implemented” requires code
and direct test or build evidence. “Boundary” means the database- or application-owned contract
exists but no concrete reference integration proves it. “Open” is required work; it is not hidden
by the broader Phase 1–4 labels.

| Sections | Requirement area | Current evidence | Status / remaining proof |
|---|---|---|---|
| 1–9 | Architecture, authority, objects, layout, dependency direction | Split core/client/server/transport/adapter crates plus four-profile database-neutrality policy | Implemented |
| 10–16 | UUIDv7 IDs, envelopes, entity refs, idempotency, journal, outbox | `aequora-types`, `aequora-protocol`, store capabilities, retry tests | Implemented |
| 17–19 | Atomic local transaction and capability-based adapters | `StoolapDatabase::transact_local_mutation` and real MVCC rollback/commit test | Implemented |
| 20–24 | Client/server/Axum architecture and validation pipeline | `ClientSyncEngine`, `SyncServer`, framed Axum routes, validated request wrapper | Implemented |
| 25–29 | Structural/business/conflict validation and compile-time execution flow | Protocol limits, typed handlers, `IncomingOperation` through `ExecutableOperation` flow | Implemented |
| 30–42 | Exchange DTOs, cursors, ledger, crash recovery, versions, HLC | Framed request/response, scoped cursor validation, atomic reconciliation, HLC tests | Implemented |
| 43–47 | Conflict strategies, financial safety boundary, deletes, tombstone GC | Policy registry, guarded `FinancialOperation` tests, field/CRDT mergers, tombstones, retention planner | Implemented |
| 48–52 | Snapshot bootstrap, consistency, partial sync, partitions | Resumable and streaming snapshots, atomic final install, scope authorization/policies | Implemented |
| 53–59 | Dependency DAG and Tokio/Rayon split | `SmallVec` dependencies, stable O(V+E) planner, dedicated thresholded Rayon pool | Implemented |
| 60–71 | Postcard/RON/JSON, framing, capabilities, compression, integrity | `AEQ1` frame, checksum, zstd bounds, optional RON and JSON diagnostic codecs | Implemented |
| 72–76 | Authentication boundary, authorization, tenant isolation, atomic server transaction | Connection-derived `AuthContext`, typed authorization, failpoints, and live PostgreSQL atomic transaction | Implemented |
| 77 | PostgreSQL adapter | Concrete `SqlxPostgresBackend`, migration, transaction, journal, snapshot, compaction, audit, and live test | Implemented |
| 78 | Stoolap adapter | Concrete `StoolapDatabase`, versioned local schema, atomic domain/outbox API, reconciliation, conflict and snapshot storage, persistent real-engine tests | Implemented |
| 79–81 | Domain integration and typed/dynamic operation registry | Typed registry, erased dispatcher, schema migration windows | Implemented |
| 82–87 | Reconciliation, accepted/modified/rejected results, conflict inbox | Atomic reconciler, terminal outbox states, durable generic conflict inbox | Implemented |
| 88–99 | Error taxonomy, retry/backoff, state machine, batching, HTTP/Axum | Typed retry semantics, coordinator, adaptive batching, bounded Reqwest/Axum transport | Implemented |
| 100–101 | Attachment/blob architecture | BLAKE3 refs/manifests plus bounded atomic reference-store publication and incomplete-upload tests | Implemented |
| 102–106 | Threat model, resource limits, bounded wire types, serialization/database safety | Runtime limits plus allocation-bounded sequence visitors and no raw SQL DTO | Implemented |
| 107–109 | Observability, trace context, metrics | Correlated observers, exact HTTP frame bytes, client gauges, validation/execution/database timings | Implemented |
| 110–115 | RON config, feature flags, builders, coordinator and UI notification | Strict config conversions, dependency-gated adapters, type-state builders, status and health watches | Implemented |
| 116–117 | Local-first reads and atomic local writes | Real Stoolap transaction API/test and runnable local repository example | Implemented |
| 118–123 | ERP examples, concurrent/same-field conflicts, referential integrity and IDs | Runnable typed attendance flow plus field-merge and UUID tests | Implemented |
| 124–125 | Database and domain schema migrations | Checksummed PostgreSQL and Stoolap ledgers, serialized server migration, crash-replay-safe local DDL, and typed payload migration windows | Implemented |
| 126–127 | Old clients and forced resynchronization | Protocol compatibility windows, typed upgrade directive, cursor-floor resync and automatic bootstrap | Implemented |
| 128 | Safe compaction | Conservative watermark planner, persistent journal floor, ledger-preservation tests | Implemented |
| 129 | Sync journal versus permanent audit log | Separate `AuditLog` capability/schema, atomic audit commit, and compaction-preservation tests | Implemented |
| 130–131 | Testkit and deterministic simulation | In-memory stores, clocks, transports, simulator, and reusable local/authority adapter conformance contracts | Implemented |
| 132 | Property invariants | Generated idempotency, cursor, outbox, applied-event, convergence, version, merge, and compaction properties | Implemented |
| 133 | Fuzzing | Four libFuzzer targets plus 128-run smoke execution for each | Implemented |
| 134 | Failure injection | Ten authoritative transaction failpoints including audit, plus lost-response and transport failures | Implemented |
| 135 | Model-based testing | 64 generated two-client histories compared with a reference authority | Implemented |
| 136–138 | Independent benchmarks, targets, memory strategy | Criterion harness executed across ten pipeline measurements; `SmallVec` and bounded decode allocations | Implemented |
| 139–143 | Async/locking/aggregate and internal/client pipelines | No global async-held mutex; database atomic boundaries; explicit processing and reconciliation phases | Implemented by architecture and tests |
| 144 | Proposed crate responsibilities | Every proposed crate exists, plus focused config/HTTP/QUIC/CRDT/partition/routing crates | Implemented |
| 145–152 | Actual ERP integration, data flow, server-originated changes, command bus reuse | Runnable typed Stoolap attendance flow and tested journal-safe server command path | Implemented |
| 153–156 | Domain semantics, SQL boundary, dependencies, dependency isolation | Raw SQL absent; adapters isolated; default facade excludes Axum/Reqwest/database crates | Implemented |
| 157–160 | Phase 1–4 features | All named phase features have corresponding crates and direct tests | Implemented |
| 161 | Recommended first API | Constructors and type-state client/server builders | Implemented |
| 162 | Thirteen permanent invariants | Named property/integration tests, guarded financial policy, and live PostgreSQL/Stoolap proof | Implemented |
| 163–165 | Architecture diagrams and final project guidance | `plan.md`, README guidance, runnable examples, and this evidence matrix | Implemented |

## Release gates

Completion requires all of the following from the final workspace state:

```text
cargo fmt --all -- --check
bash scripts/check-database-neutrality.sh
cargo +1.87.0 check --workspace --all-targets --all-features --offline
cargo check --workspace --all-targets --all-features --offline
cargo clippy --workspace --all-targets --all-features --offline -- -D warnings
cargo test --workspace --all-features --offline
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --all-features --no-deps --offline
cargo check --manifest-path fuzz/Cargo.toml --bins --offline
cargo bench -p aequora-testkit --bench core_pipeline --no-run --offline
cargo package --workspace --no-verify --allow-dirty --locked --offline
cargo run -p aequora --example in_process --features testkit --offline
cargo run -p aequora --example school_erp --features stoolap,testkit --offline
```

The PostgreSQL gate runs when `AEQUORA_TEST_POSTGRES_URL` is set. Neon runs the same integration
suite when both `AEQUORA_TEST_NEON_POOLED_URL` and `AEQUORA_TEST_NEON_DIRECT_URL` are set. Stoolap
real-engine tests run in every normal workspace test invocation.

## Final verification record

Verified on 2026-08-10:

- all release-gate commands above passed from the final workspace state;
- the complete all-feature workspace suite passed, including 23 sync invariants, seven generated
  properties, the model-based history test, transport tests, and real Stoolap transactions;
- the PostgreSQL adapter test passed separately against a disposable PostgreSQL 18 instance;
- the database-neutrality gate compiled and inspected custom/custom, Stoolap/custom,
  custom/PostgreSQL, and Stoolap/PostgreSQL feature profiles without cross-adapter leakage;
- a persistent Stoolap client completed an offline write, real HTTP/Axum exchange, PostgreSQL 18
  authoritative commit, and local reconciliation in one end-to-end integration test;
- public behavioral adapter contracts passed against the reference stores, Stoolap, and the live
  PostgreSQL authority;
- the PostgreSQL migration ledger recorded and revalidated version, name, 32-byte checksum, and application timestamp;
- persistent Stoolap tests adopted an unversioned legacy schema, replayed interrupted idempotent DDL,
  revalidated reopen health, and rejected checksum drift;
- every fuzz target completed a 128-input smoke run without a crash; and
- the Criterion pipeline harness both built and completed all ten measurements; and
- all 26 publishable crates produced valid package archives in dependency order.

The workspace MSRV is Rust 1.87. The lockfile intentionally holds transitive Stoolap, URL/ICU, and
SQLx dependencies to versions that compile at that baseline.

`.github/workflows/ci.yml` reproduces these gates in independent stable, MSRV, live PostgreSQL, and
artifact jobs with read-only repository permissions and per-job timeouts.
