# Aequora Sync — Remaining Architecture Roadmap

## Systems and Architecture Still Required Beyond the Existing Aequora Documents

This roadmap covers major architectural areas that are not yet fully specified by the existing Aequora Sync, ACID, enterprise, plug-and-play, and universal-database interoperability documents.

The recommended approach is to design these as independent but compatible architecture documents.

## Prerequisite Implementation Order

This file is a map, not a normative subsystem specification. Implementation follows this order:

```text
1. enterprise.md
2. database-interoperability.md
3. plug-and-play.md
4. ACID.md
5. Parts 01–30 in the order below
```

The detailed numbered and named architecture documents define behavior. Supporting completion
records under `docs/` map those requirements to code, tests, host responsibilities, and explicit
remaining work. Work on Parts 01–30 must not be reported as the active implementation phase until
the prerequisite set is reconciled.

As of the current workspace implementation, all four named prerequisites are reconciled at the
reusable repository boundary. Their completion records retain the live infrastructure,
application-specific mapping, deployment, and release-acceptance gates that cannot be proven by an
offline library build. Work therefore proceeds with Part 01; no later numbered part is considered
complete merely because its architecture document exists.

Part 01 is implemented and mapped to executable evidence in `01-formal-correctness.md`: versioned
invariants, bounded distributed-model exploration, properties, Loom single-flight coverage,
failpoints, differential adapter scenarios, replayable traces, and CI tiers. Sequential work may
therefore proceed to Part 02. Part 02 is also implemented at the reusable repository boundary:
retry-stable correlation, authoritative event identity, trusted provenance, durable lineage,
tenant-bounded correlation diagnostics, adapter round-trips, and executable lineage invariants are
mapped in `docs/causality-provenance-completion.md`. Sequential work may therefore proceed to Part
03. Part 03 is implemented at the reusable repository boundary: canonical database-neutral
digests, generation-bound partition trees, Merkle localization, conservative repair planning,
pending-intent preservation, adapter verification/repair contracts, fault tests, observability,
and operator diagnostics are mapped in `docs/anti-entropy-self-repair-completion.md`. Part 04 is
also implemented at the reusable repository boundary: immutable once-possibly-delivered payloads,
bounded dependency-aware compaction, durable supersession, safe rebase, transactional Stoolap
rewrites, fail-closed finance/append semantics, properties, failpoints, metrics, and diagnostics
are mapped in `docs/offline-compaction-rebase-completion.md`. Sequential work may therefore proceed
to Part 05; live deployment validation remains an explicit environment gate. Part 05 is now
implemented at the reusable repository boundary: persistent store/process identities, atomic
Stoolap lease epochs, fencing across leader-only local commits, follower-safe domain/outbox writes,
crash takeover, maintenance generation changes, process-mode configuration, reusable adapter
compliance, Loom/property coverage, diagnostics, and metrics are mapped in
`docs/local-multiprocess-coordination-completion.md`. Sequential work may therefore proceed to Part
06; multi-process deployment acceptance remains an explicit host gate. Part 06 is now implemented
at the reusable repository boundary: normalized platform context, bounded priority metadata,
deterministic eligibility, aging and dependency inheritance, weighted fairness, adaptive
count/byte batching, retry and circuit state, safe server hints, profiles, client lifecycle gates,
invariants, metrics, and diagnostics are mapped in
`docs/adaptive-sync-scheduler-qos-completion.md`. Sequential work may therefore proceed to Part 07;
device-specific scheduling acceptance remains an explicit host gate. Part 07 is now implemented at
the reusable repository boundary: server-issued scope definitions, exact version/generation cursor
binding, filtered watermarks, projection/resource authorization contracts, independent durable
subscriptions, atomic bootstrap/expansion/contraction/revocation, shared membership tracking,
pending-intent quarantine, Stoolap migration and fencing, adapter compliance, client scheduling,
invariants, metrics, and diagnostics are mapped in
`docs/subscription-scope-dynamic-dataset-completion.md`. Sequential work may therefore proceed to
Part 08; application authorization and live revocation acceptance remain explicit host gates.
Part 08 is now implemented at the reusable repository boundary: additive payload-free live
envelopes, transport and broker contracts, post-commit best-effort publication, authenticated
tenant/scope fan-out, dynamic revocation, latest-only bounded backpressure, client wake-generation
coalescing, fenced leader ownership, reconnect catch-up, PostgreSQL NOTIFY and in-memory adapters,
TTL presence, fault injection, invariants, metrics, and diagnostics are mapped in
`docs/live-sync-push-presence-completion.md`. Sequential work may therefore proceed to Part 09;
provider-specific WebSocket/SSE/mobile integration, application presence privacy, and production
load acceptance remain explicit host gates.
Part 09 is now implemented at the reusable repository boundary: canonical checksummed artifacts,
durable import jobs and checkpoints, deterministic two-pass identity mapping, payload-minimized
quarantine, dependency ordering, baseline and journal semantics, CDC watermarks, fail-closed
cutover evidence, export manifests, CLI inspection, scheduler/telemetry integration, invariants,
and fault-injected restart/replay contracts are mapped in
`docs/bulk-import-export-seed-migration-completion.md`. Sequential work may therefore proceed to
Part 10; source-specific mapping, live CDC, native authority transactions, writer fencing,
backup/rollback rehearsal, and production-scale acceptance remain explicit host gates.
Part 10 is now implemented at the reusable repository boundary: durable bootstrap jobs,
deterministic manifests/chunks, safe range resume, bounded verification and staging, retention
leases, pending-intent commitments, fail-closed generation activation, transport-neutral source
and sink contracts, fault injection, invariants, telemetry, and diagnostics are mapped in
`docs/large-snapshot-streaming-bootstrap-completion.md`. Sequential work may therefore proceed to
Part 11; real database read views, object-store/CDN integration, native generation swaps,
multi-GB capacity, and production delta catch-up remain explicit adapter/deployment gates.

## Existing Foundation

Already covered:

1. Core database-agnostic sync engine.
2. Client/server synchronization flow.
3. Axum validator/executor architecture.
4. Postcard/RON protocol strategy.
5. Tokio/Rayon execution separation.
6. ACID transaction boundaries.
7. Idempotency and operation ledger.
8. Transactional outbox and authoritative journal.
9. Conflict framework.
10. Bootstrap and snapshots.
11. Enterprise deployment and operations.
12. Plug-and-play SDK architecture.
13. Same/different database interoperability.
14. Storage adapter/capability model.
15. Production security, observability, backup, and deployment foundations.

## Remaining Parts

### Part 01 — Formal Correctness, Invariants, Model Checking, and Deterministic Simulation

Aequora is a distributed state machine. Ordinary tests are insufficient to prove retry, crash, ordering, and concurrency properties.

Design:

- executable invariant specification;
- deterministic state-machine model;
- Rust model checking;
- Loom concurrency tests;
- proptest state-machine generation;
- failure-state exploration;
- linearizability/idempotency checks;
- convergence verification;
- deterministic replay of failures;
- invariant registry used by tests and adapters.

### Part 02 — Causality, Dependency, Provenance, and Event Lineage

- causation ID;
- correlation ID;
- parent operation;
- dependency DAG;
- authoritative event lineage;
- HLC boundaries;
- actor/device provenance;
- derived operation chains;
- replay lineage.

### Part 03 — Anti-Entropy, Integrity Verification, Divergence Detection, and Self-Repair

- canonical entity digests;
- partition digests;
- Merkle trees;
- cursor-independent verification;
- divergence localization;
- repair snapshots;
- quarantine;
- corruption detection;
- repair while preserving pending operations.

### Part 04 — Offline Operation Compaction, Coalescing, Rebase, and Queue Optimization

- safe coalescing;
- supersession;
- cancellation;
- dependency-preserving compaction;
- operation squashing;
- rebase after bootstrap;
- immutable operation exceptions;
- finance-safe rules;
- queue pressure management.

### Part 05 — Local Multi-Process / Multi-Window Coordination

- process lease;
- local coordinator election;
- database/file lease;
- fencing tokens;
- crash takeover;
- background vs foreground process;
- duplicate local sync prevention.

### Part 06 — Adaptive Sync Scheduler and QoS

- foreground/background priority;
- bandwidth awareness;
- metered networks;
- battery awareness;
- adaptive batching;
- urgent operations;
- fairness;
- server load hints;
- mobile OS constraints.

### Part 07 — Subscription, Scope, Filter, and Dynamic Dataset Architecture

- scope descriptors;
- filter identity;
- dataset expansion/contraction;
- permission changes;
- revocation;
- filtered tombstones;
- cursor invalidation;
- multi-scope clients.

### Part 08 — Live Sync, Push Hints, Presence, and Near-Real-Time Delivery

- WebSocket/SSE hints;
- platform push hints;
- reconnect;
- sequence hints;
- fan-out;
- tenant channels;
- backpressure;
- presence.

### Part 09 — Bulk Import, Export, Seed, and Initial Migration

- legacy imports;
- millions of records;
- deterministic IDs;
- restartable import;
- quarantine;
- duplicate detection;
- baseline journal/snapshot creation;
- canonical export.

### Part 10 — Large Snapshot, Streaming Bootstrap, and Resumable Transfer

- multi-GB bootstraps;
- chunk manifests;
- resumable downloads;
- object storage;
- parallel chunks;
- checksums;
- staged install;
- delta-after-snapshot;
- throttling.

### Part 11 — Operation Semantics, Aggregate Policies, and Consistency Profiles

**Repository status:** Complete. The profile registry, manifests, derives/builders, capability
validation, invariants, telemetry, CLI, and compliance evidence are mapped in
[`docs/operation-semantics-consistency-profiles-completion.md`](docs/operation-semantics-consistency-profiles-completion.md).

Profiles:

- ImmutableAppendOnly;
- OptimisticVersioned;
- Commutative;
- LastWriterWins;
- ManualConflict;
- StrongAggregate;
- ServerOnly;
- DeviceLocal;
- DerivedProjection.

Each profile defines version, conflict, retry, compaction, delete, and snapshot rules.

### Part 12 — Deterministic Domain Execution and Replay

**Repository status:** Complete. Captured execution inputs, pure decision plans, handler/policy
versions, integrity-bound replay bundles, isolated and differential replay, commit/side-effect
boundaries, invariants, telemetry, diagnostics, and fault contracts are mapped in
[`docs/deterministic-execution-replay-completion.md`](docs/deterministic-execution-replay-completion.md).

- deterministic handlers;
- capture nondeterministic inputs;
- clocks;
- random IDs;
- external results;
- replay envelope;
- historical debugging.

### Part 13 — Data Provenance, Auditability, and Explainability

**Repository status:** Complete. Canonical business-audit events, truthful attribution, stable
actions/fields/reasons, sensitivity and durability policies, atomic execution-plan declarations,
tamper-evident chains, checkpoints/anchors, bounded authorized queries, field provenance,
origin-specific explanations, invariants, telemetry, diagnostics, and fault contracts are mapped
in [`docs/data-provenance-auditability-explainability-completion.md`](docs/data-provenance-auditability-explainability-completion.md).

- who changed data;
- source device;
- operation lineage;
- previous/new version;
- conflict resolution source;
- derived changes;
- user-visible history;
- privacy-preserving audit.

### Part 14 — Data Governance, Retention, Legal Hold, and Erasure

**Repository status:** Complete. Versioned retention/deletion policy, legal holds, subject/copy
graphs, erasure and purge plans, tombstone/journal/ledger safety, tenant offboarding, restore gates,
client purge directives, storage-surface verification, invariants, diagnostics, and fault contracts
are mapped in [`docs/data-governance-retention-erasure-completion.md`](docs/data-governance-retention-erasure-completion.md).

- journal retention;
- tombstones;
- audit retention;
- legal hold;
- per-tenant policy;
- erasure workflows;
- snapshot/blob cleanup;
- deletion evidence.

### Part 15 — Cryptographic Integrity and Optional End-to-End Protected Payloads

**Repository status:** Complete. Provider-neutral BLAKE3 domain-separated digests, Ed25519
artifact/checkpoint/device signatures, purpose-bound rollback-protected key registries,
XChaCha20-Poly1305 tenant envelopes, Argon2id export keys, E2E compatibility rules, governance
destruction evidence, invariants, telemetry, diagnostics, contract/property/tamper tests, and fuzz
coverage are mapped in
[the Part 15 completion report](docs/cryptographic-integrity-key-management-e2e-completion.md).

- device keypairs;
- signed operations;
- key rotation;
- replay protection;
- tamper evidence;
- optional encrypted fields;
- limitations with server validation.

### Part 16 — Authority Failover, Timeline Epochs, Fork Detection, and Disaster Promotion

- authority epoch;
- fencing;
- primary promotion;
- old-primary rejection;
- split-brain detection;
- timeline fork handling;
- PITR promotion;
- client behavior during authority changes.

### Part 17 — Multi-Region Read Architecture and Future Single-Writer Global Deployment

- one write region;
- regional Axum edges;
- read replicas;
- request routing;
- replica lag;
- bootstrap locality;
- future multi-region path.

### Part 18 — Backpressure, Admission Control, Fairness, and Overload Safety

- global budgets;
- per-tenant budgets;
- per-device budgets;
- priority classes;
- queue deadlines;
- DB/Rayon admission;
- retry-after;
- thundering-herd control.

### Part 19 — Performance Engineering and Memory Architecture

- allocation strategy;
- Bytes/Arc reuse;
- zero-copy boundaries;
- streaming serialization;
- batching;
- transaction sizing;
- prepared statements;
- journal query shapes;
- Rayon thresholds;
- performance regression gates.

### Part 20 — Resource-Constrained Client Architecture

- bounded RAM;
- bounded disk;
- low-storage mode;
- queue preservation;
- snapshot staging limits;
- mobile background limits;
- incremental compaction.

### Part 21 — Protocol Negotiation and Compatibility Governance

- capability bits;
- mandatory/optional features;
- protocol epochs;
- manifests;
- compatibility CI;
- retirement policy;
- downgrade prevention;
- long-offline clients.

### Part 22 — Sync Metadata Schema and Internal Persistence Specification

Normative logical schema for:

- client state;
- devices;
- outbox;
- inbox;
- conflicts;
- journal;
- operation ledger;
- snapshots;
- scopes;
- leases;
- authority epochs;
- repair state.

### Part 23 — Background Jobs, Durable Workflows, and Side-Effect Engine

- durable jobs;
- retries;
- job IDs;
- scheduling;
- webhooks;
- notifications;
- snapshot jobs;
- cleanup;
- dead-letter/quarantine;
- worker leases.

### Part 24 — Operational Control Plane and Admin API

- tenant health;
- device health/revoke;
- operation inspection;
- journal inspection;
- resync request;
- maintenance mode;
- RBAC;
- safe admin mutation policy;
- audit.

### Part 25 — Diagnostics, Forensics, and Reproducible Incident Bundles

- lifecycle reconstruction;
- sanitized bundles;
- protocol capture;
- deterministic replay;
- trace correlation;
- adapter diagnostics;
- incident fingerprints.

### Part 26 — Compatibility With Existing / Legacy Applications

Implementation evidence: [Part 26 completion report](docs/legacy-application-compatibility-incremental-adoption-completion.md).

- shadow mode;
- read-only observation;
- CDC bridge;
- dual-write migration;
- transactional outbox introduction;
- cutover;
- comparison mode;
- rollback.

### Part 27 — Dedicated Security Threat Model and Abuse Resistance

- malicious clients;
- tenant probing;
- operation/dependency bombs;
- decompression bombs;
- auth replay;
- cursor manipulation;
- compromised devices;
- compromised server nodes;
- resource starvation;
- containment.

### Part 28 — Multi-Consumer Change Feed Architecture

- independent consumer IDs;
- analytics;
- search;
- warehouse;
- notification consumers;
- consumer cursor;
- replay;
- poison-event quarantine;
- lag monitoring.

### Part 29 — Schema / Operation Registry Service and Developer Governance

- operation ID allocation;
- entity/field ID allocation;
- ownership;
- deprecation;
- compatibility manifests;
- CI validation;
- module merges;
- preventing ID reuse;
- generated docs.

### Part 30 — Certification, Conformance, and Ecosystem Architecture

- adapter certification;
- transport certification;
- official vs community support;
- test vectors;
- fixtures;
- benchmark suite;
- support lifecycle;
- vulnerability policy.

## Recommended Order

```text
01 Formal Correctness / Model Checking
02 Causality / Provenance
03 Anti-Entropy / Self-Repair
04 Offline Compaction / Rebase
05 Multi-Process Local Coordination
06 Adaptive Scheduler / QoS
07 Dynamic Scopes
08 Live Sync Hints
09 Bulk Import
10 Streaming Bootstrap
11 Consistency Profiles
12 Deterministic Replay
13 Audit / Provenance
14 Data Governance
15 Cryptographic Integrity
16 Authority Failover
17 Multi-Region Read Architecture
18 Admission Control
19 Performance Architecture
20 Resource-Constrained Clients
21 Protocol Governance
22 Metadata Persistence Specification
23 Durable Jobs
24 Admin Control Plane
25 Forensics
26 Legacy Adoption
27 Threat Model
28 Multi-Consumer Feed
29 Registry Governance
30 Certification / Ecosystem
```

The first five should be completed before calling the Aequora architecture substantially complete.

## Architectural Completion Criterion

Aequora should eventually have explicit, testable answers to:

```text
What happens if the same operation arrives twice?
What happens if the server commits but the response disappears?
What happens if a client is offline for six months?
What happens if a cursor is corrupted?
What happens if client data silently diverges?
What happens if two app processes use the same local DB?
What happens if the database engine is migrated?
What happens after PITR?
What happens if an old primary returns?
What happens if 100,000 clients reconnect simultaneously?
What happens if an operation schema changes?
What happens if a revoked device returns with pending work?
What happens if a client runs out of disk?
What happens if a snapshot is interrupted?
What happens if an adapter violates transaction guarantees?
How can one OperationId be reconstructed end-to-end?
How can corruption be detected before a user reports it?
```

When the architecture answers these precisely, Aequora has moved from a sync library toward a resilient synchronization platform.
