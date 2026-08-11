# `plan.md` completion audit

This matrix treats the numbered plan as the release specification. “Implemented” requires code
and direct test or build evidence. “Boundary” means the database- or application-owned contract
exists but no concrete reference integration proves it. “Open” is required work; it is not hidden
by the broader phase labels.

| Sections | Requirement area | Current evidence | Status / remaining proof |
|---|---|---|---|
| 1–9 | Architecture, authority, objects, layout, dependency direction | Split core/client/server/transport/adapter crates plus four-profile database-neutrality policy | Implemented |
| 10–16 | UUIDv7 IDs, envelopes, entity refs, idempotency, journal, outbox | `aequora-types`, `aequora-protocol`, store capabilities, retry tests | Implemented |
| 17–19 | Atomic local transaction and capability-based adapters | `StoolapDatabase::transact_local_mutation` and real MVCC rollback/commit test | Implemented |
| 20–24 | Client/server/Axum architecture and validation pipeline | `ClientSyncEngine`, `SyncServer`, framed Axum routes, validated request wrapper | Implemented |
| 25–29 | Structural/business/conflict validation and compile-time execution flow | Protocol limits, typed handlers, `IncomingOperation` through `ExecutableOperation` flow | Implemented |
| 30–42 | Exchange DTOs, cursors, ledger, crash recovery, versions, HLC | Framed request/response, exact cursor-completeness checks, complete terminal-result validation, atomic reconciliation, HLC tests | Implemented |
| 43–47 | Conflict strategies, financial safety boundary, deletes, tombstone GC | Policy registry, explicit timestamped LWW, guarded `FinancialOperation` tests, field/CRDT mergers, tombstones, retention planner | Implemented |
| 48–52 | Snapshot bootstrap, consistency, partial sync, partitions | Snapshot-first new-client onboarding, resumable/streaming pages, atomic final install, scope authorization/policies | Implemented |
| 53–59 | Dependency DAG, Guppy, and Tokio/Rayon split | `SmallVec` dependencies, O(V+E) planner, thresholded dedicated Rayon batch planning and parallel hashing, plus Guppy Cargo boundaries | Implemented |
| 60–71 | Postcard/RON/JSON, framing, capabilities, compression, integrity | `AEQ1` frame/checksum, append-only wire discriminants, matched frame/DTO versions, bounded negotiated HTTP/QUIC zstd in both directions, diagnostic RON/JSON | Implemented |
| 72–76 | Authentication boundary, authorization, tenant isolation, atomic server transaction | Connection-derived `AuthContext`, typed authorization, failpoints, and a live PostgreSQL 18 atomic transaction | Implemented |
| 77 | PostgreSQL adapter | Concrete `SqlxPostgresBackend`, migration, transaction, journal, snapshot, compaction, audit, adapter contracts, and current-revision PostgreSQL 18 live tests | Implemented |
| 78 | Stoolap adapter | Concrete `StoolapDatabase`, versioned local schema, atomic domain/outbox API, reconciliation, conflict and snapshot storage, persistent real-engine tests | Implemented |
| 79–81 | Domain integration and typed/dynamic operation registry | Typed registry, erased dispatcher, schema migration windows | Implemented |
| 82–87 | Reconciliation, accepted/modified/rejected results, conflict inbox | Atomic reconciler, terminal outbox states, durable generic conflict inbox | Implemented |
| 88–99 | Error taxonomy, retry/backoff, state machine, batching, HTTP/Axum | Typed retries, mutation debounce/max-wait, operation and exact framed-byte batch ceilings, adaptive RON tuning, bounded Reqwest/Axum transport | Implemented |
| 100–101 | Attachment/blob architecture | BLAKE3 refs/manifests plus bounded atomic reference-store publication and incomplete-upload tests | Implemented |
| 102–106 | Threat model, resource limits, bounded wire types, serialization/database safety | Runtime and decoder allocation limits, client-side advertised response/snapshot enforcement, and no raw SQL DTO | Implemented |
| 107–109 | Observability, trace context, metrics | Correlated observers, exact transport bytes, retry count, scoped journal lag, client gauges, validation/execution/database timings | Implemented |
| 110–115 | RON config, feature flags, builders, coordinator and UI notification | Strict conversions through client/server/HTTP/Axum/QUIC/compute/coordinator, dependency-gated adapters, type-state builders, status/health watches | Implemented |
| 116–117 | Local-first reads and atomic local writes | Real Stoolap transaction API/test and runnable local repository example | Implemented |
| 118–123 | ERP examples, concurrent/same-field conflicts, referential integrity and IDs | Runnable typed attendance flow plus field-merge and UUID tests | Implemented |
| 124–125 | Database and domain schema migrations | Checksummed PostgreSQL and Stoolap ledgers, serialized server migration, crash-replay-safe local DDL, and typed payload migration windows | Implemented |
| 126–127 | Old clients and forced resynchronization | Append-only protocol enum proof, compatibility windows, typed upgrade directive, cursor-floor resync and automatic bootstrap | Implemented |
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
| 162 | Thirteen permanent invariants | Named property/integration tests, guarded financial policy, persistent Stoolap proof, and current-revision live PostgreSQL proof | Implemented |
| 163–165 | Architecture diagrams and final project guidance | `plan.md`, README guidance, runnable examples, and this evidence matrix | Implemented |
| 166 | Phase 5 production operational resilience | Shared Axum admission permits, exchange/bootstrap deadlines, `Retry-After`, split liveness/readiness, async application probes, strict RON mapping, payload-free metrics, and deterministic saturation/timeout/readiness tests | Implemented |
| 167 | Phase 6 graceful draining and zero-downtime lifecycle | Race-free `ServerLifecycle`, exact admitted count, irreversible draining, readiness short-circuit, transient new-work rejection, bounded typed drain outcomes, RON deadline, metrics, and concurrency tests | Implemented |
| 168 | Phase 7 multi-tenant fair admission | Authenticated pre-body tenant admission, atomic global/tenant counts, bounded idle-counter cleanup, distinct transient `429`, strict RON relationships, payload-free metric, and deterministic noisy-neighbor isolation test | Implemented |
| 169 | Phase 8 authenticated tenant request-rate limiting | Per-tenant token buckets before body decoding, configurable sustained/burst limits, bounded inactive state with safe eviction, transient `429`, payload-free metric, strict RON mapping, and deterministic refill/isolation/retention tests | Implemented |
| 170 | Phase 9 bounded HTTP body ingestion | Deadline- and byte-bounded custom Axum body extraction after admission, transient `408`, permanent `413`, automatic permit release, strict RON mapping, payload-free metrics, and adversarial slow/oversized stream tests | Implemented |

## Release gates

Completion requires all of the following from the final workspace state:

```text
cargo fmt --all -- --check
cargo run -p aequora-dev --locked -- check
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

Verified from the final source state on 2026-08-11:

- formatting, Rust 1.87 all-target/all-feature checking, stable strict Clippy, rustdoc warnings,
  Guppy boundaries, database-neutrality profiles, fuzz-target builds, benchmark build, packaging,
  both runnable examples, and `git diff --check` passed;
- the complete all-feature workspace suite passed, including 27 sync invariants, seven generated
  properties, the model-based history test, compressed HTTP and QUIC loopback tests, and real
  persistent Stoolap transactions;
- the added adversarial tests prove exact cursor completeness, complete terminal operation
  results, framed push/response byte ceilings, snapshot-first onboarding, mutation debounce,
  retry/journal-lag metrics, append-only conflict-policy wire discriminants, pre-body admission
  saturation, permit release after cancellation, authoritative execution deadlines, independent
  liveness/readiness semantics, and bounded dependency probes;
- Phase 5 operational failures return explicit transient `503`/`504` responses with
  `Retry-After`; strict RON rejects zero operational bounds, maps valid bounds into Axum, and
  payload-free metrics count overloads, deadlines, readiness checks, and readiness failures;
- Phase 6 proves that drain admission cannot race past a zero in-flight observation, dependency
  probes stop after draining, admitted work can finish, deadlines return the exact remaining count,
  and lifecycle gauges/outcomes remain payload-free;
- Phase 7 proves that a saturated tenant receives `429` without preventing another tenant from
  using remaining global capacity, both counts release on permit drop, idle tenant counters are
  removed, and the overload metric contains no tenant identifier;
- Phase 8 proves burst exhaustion and monotonic capped refill, rejects rate excess before decoding,
  preserves another tenant's capacity, never evicts an active bucket, removes expired inactive
  state, bounds tracked buckets, and keeps `429` transient in the HTTP client;
- Phase 9 proves a never-ending body reaches bounded `408` without service execution, releases its
  permit for a valid retry and drain, rejects streamed wire overflow with permanent `413`, and
  records both ingestion failures without body or identity data;
- the subsequent `next.md`/`ACID.md` reconciliation adds explicit adapter durability declarations,
  concurrent duplicate/version-race conformance, bounded PostgreSQL deadlock/serialization retry,
  real rollback/restart/snapshot-install proofs, transaction metrics, and durable due-only client
  retry scheduling through Stoolap schema revision 2;
- the database-neutrality gate compiled and inspected custom/custom, Stoolap/custom,
  custom/PostgreSQL, and Stoolap/PostgreSQL feature profiles without cross-adapter leakage; and
- all 26 publishable crates plus the non-publishable `aequora-dev` utility produced package
  archives in dependency order.

The current revision was also verified against a fresh disposable PostgreSQL 18.4 server on
2026-08-11. The two adapter tests passed transaction atomicity, schema/migration-ledger,
snapshot/compaction, and adapter-contract coverage. The two facade integration tests passed the
database-neutral persistent Stoolap client through HTTP/Axum to PostgreSQL, including the
bidirectional server-originated-change path. The server was then shut down and its two inspected
temporary data/socket directories were removed. Neon pooled/direct URLs were unset, so only the
Neon-specific live branch remains an environment-conditional CI gate rather than being falsely
reported as executed.

The workspace MSRV is Rust 1.87. The lockfile intentionally holds transitive Stoolap, URL/ICU, and
SQLx dependencies to versions that compile at that baseline.

`.github/workflows/ci.yml` reproduces these gates in independent stable, MSRV, live PostgreSQL, and
artifact jobs with read-only repository permissions and per-job timeouts.
