# `next.md` implementation completion

This matrix reconciles the 184 architecture sections in `next.md` with the implemented Aequora
workspace. Later, more concrete decisions in `plan.md` take precedence where `next.md` presented
multiple policies or illustrative shapes.

## Section map

| Sections | Architecture area | Current implementation | Status |
|---|---|---|---|
| 1–5 | purpose, invariants, workspace, dependency direction, facade | 27 bounded crates, facade feature gates, Guppy rules, and database-neutrality profiles | Implemented |
| 6–13 | typed IDs, type separation, lifecycle, opaque operations, registry | newtypes, authenticated/authorized/validated/executable wrappers, immutable typed `OperationRegistry`, schema upcasters | Implemented |
| 14–19 | frame and primary request/response messages | checksummed `AEQ1` Postcard frame, typed message kind, request IDs, sync/bootstrap messages, typed directives | Implemented |
| 20–21 | cursor identity and validity | tenant/scope cursor, monotonic sequence, retained-floor expiry, ahead/incomplete-sequence rejection, typed resync | Implemented with the v1 non-resetting-sequence policy below |
| 22–28 | local state, outbox/inbox, domain transaction | Stoolap metadata, replayable state machine, applied-event ledger, `transact_local_mutation`, reconciliation transaction | Implemented |
| 29–35 | coordinator, coalescing, retry | bounded coordinator signals, debounce, connectivity as hint, bounded jittered backoff, typed retry classification, durable retry attempt/deadline | Implemented |
| 36–38 | reconciliation and authoritative changes | atomic ACK/rejection/conflict/change/cursor application, scoped applied markers, upsert/tombstone payloads | Implemented |
| 39–46 | Axum/server pipeline, authentication, validation, dedup | authenticated pre-body admission, bounded decoding, typed pipeline, indexed operation ledger and stable result replay | Implemented |
| 47–60 | dependencies, validation, execution, transaction policy | linear dependency plan, cycle/missing propagation, typed handler stages, CAS commit, one-operation transactions | Implemented with `IndependentOperations` policy |
| 61–66 | Tokio/Rayon separation | async I/O, dedicated bounded compute pool, configurable thresholds, no database transaction held across Rayon work | Implemented |
| 67–78 | capability stores, Stoolap, PostgreSQL/Neon | narrow traits, explicit transaction capabilities, native adapters, journal indexes, pool/readiness/migration behavior | Implemented |
| 79–86 | conflicts, manual resolution, tombstones | version checking, registered conflict policies/mergers, durable conflict inbox/CAS resolution, explicit superseding operation, tombstones | Implemented |
| 87–95 | snapshot bootstrap and scopes | repeatable-read captured snapshots, bounded resumable pages/streams, staged atomic install, scope authorization and re-bootstrap | Implemented |
| 96–98 | independent compatibility versions/upcasting | protocol window, operation schema window/upcasters, local and authoritative checksummed DB schema versions, snapshot capability version | Implemented |
| 99–106 | codecs, diagnostics, compression, hashing, blobs | Postcard default, RON/JSON diagnostics, bounded zstd, BLAKE3 framing/blob manifests, separate resumable blob store contract | Implemented |
| 107–119 | security, limits, tenant isolation, backpressure, paging, deadlines | bounded wire/decompressed/body/field limits, no raw SQL, authenticated tenant checks, global/tenant/rate admission, bounded pools/pages and timeouts | Implemented |
| 120–124 | telemetry and typed errors | request correlation, payload-free metrics/tracing, typed crate errors, serialized rejection/directive enums as stable machine codes | Implemented |
| 125–140 | tests, simulator, clock, failure injection, benchmarks, memory | unit/property/model/simulation/adapter/live/fuzz/failpoint suites, injectable HLC, Criterion harness, bounded ownership | Implemented |
| 141–150 | deployment, horizontal scale, UI/domain/finance/audit/privacy | stateless server correctness, centralized idempotency, UI-neutral status watch, server-command reuse, atomic audit, documented finance boundary | Implemented at library boundary |
| 151–154 | RON config, features, stability, versions | strict RON mapping, integration crates/features, additive protocol capabilities, independent crate/wire versions | Implemented |
| 155–157 | CLI and development integration | `aequora-dev` graph/policy/RAG tools plus runnable in-process and school ERP examples | Implemented for current developer workflow; packet conversion remains optional |
| 158–159 | v1 milestone and exclusions | complete Stoolap → Postcard/HTTP/Axum → PostgreSQL path; optional additions remain capability-isolated | Implemented |
| 160–171 | implementation phases 0–11 | specifications, all kernel/adapters/transports, persistent/live tests, fuzzing, metrics, migration gates and hardening | Implemented |
| 172–176 | suggested source trees | capability ownership is split by crates/public modules instead of mechanically creating one file per suggested name | Implemented by responsibility |
| 177–179 | end-to-end, multi-device, offline conflicts | runnable ERP/in-process flows and deterministic multi-client/conflict simulations | Implemented |
| 180 | production checklist | normal release gates plus conditional PostgreSQL/Neon integration; deployment backup/restore and external load acceptance remain environment-owned | Implemented at repository scope |
| 181–184 | final boundaries and recommendation | enforced dependency graph, database-neutral traits/profiles, transport-neutral engine, verified synchronization kernel | Implemented |

## Concrete decisions where `next.md` offered alternatives

### Cursor generation

Protocol v1 never resets a sequence within an existing `SyncScopeId`. Compaction advances a
retained floor; a cursor before that floor receives a typed snapshot-resync directive. An
incompatible scope or generation change receives a new scope identity and bootstraps from zero.
Therefore a separate `CursorGeneration` wire field would duplicate the scope identity in v1. It
must be introduced only with a protocol version that supports in-place sequence reset.

### Batch transactions

The selected v1 policy is `IndependentOperations`. Dependencies control topological execution and
failure propagation but do not imply atomic group rollback. This keeps locks bounded and avoids
claiming group semantics that the public protocol cannot express. A future atomic group requires
an explicit group ID, limits, result type, and adapter compliance cases before implementation.

### Authoritative payload schemas

`SchemaVersion` versions the operation command accepted by the registered handler. Authoritative
entity/snapshot payloads remain opaque application representations; applications that require
historical decoding include their representation discriminator inside that opaque payload. Local
DB schema, authoritative DB schema, wire protocol, operation schema, and snapshot capability are
still versioned independently.

### Operation kind width

Protocol v1 uses a compact non-zero `u16` application registry key. The high/low 16-bit `u32`
layout in `next.md` was an example namespace, not a v1 compatibility requirement. Expanding it is a
wire-version decision.

### Multiple events from one command

The v1 kernel publishes one authoritative entity transition/journal event per operation. Complex
aggregate state is encoded as one opaque authoritative representation. Applications needing
multiple independently ordered events must submit explicit operations or introduce a future
bounded multi-event protocol version; the current API does not pretend to support it.

## Durable retry completion

The previous implementation treated `Sending` and `Retry` as replayable but did not persist a
backoff deadline. This is now closed:

- `RetryMetadata` records a saturating attempt count and absolute next-attempt Unix millisecond;
- `OutboxStateStore::mark_retry` persists state, attempt increment, and deadline together;
- pending scans exclude future retries without loading the full outbox;
- client retries persist the same delay used by the active bounded retry loop;
- Stoolap migration 2 adds an indexed retry schedule without rewriting published outbox rows;
- the common local-adapter contract verifies future-deadline exclusion, attempt increments, and
  deadline replacement;
- the persistent Stoolap test reopens before the deadline, proves no early selection, releases the
  retry, and then proves reconciliation durability across another reopen.

## Environment-owned acceptance

Repository correctness gates can prove bounded concurrency, persistent adapters, migrations,
failure recovery, and build artifacts. They cannot prove a particular production deployment's
backup policy, restore credentials, external TLS termination, or sustained capacity target.
Those checks remain mandatory deployment acceptance criteria and must run against the actual
PostgreSQL/Neon project and infrastructure; they are not reported as passed when credentials or
targets are absent.
