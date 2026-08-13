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

- deterministic handlers;
- capture nondeterministic inputs;
- clocks;
- random IDs;
- external results;
- replay envelope;
- historical debugging.

### Part 13 — Data Provenance, Auditability, and Explainability

- who changed data;
- source device;
- operation lineage;
- previous/new version;
- conflict resolution source;
- derived changes;
- user-visible history;
- privacy-preserving audit.

### Part 14 — Data Governance, Retention, Legal Hold, and Erasure

- journal retention;
- tombstones;
- audit retention;
- legal hold;
- per-tenant policy;
- erasure workflows;
- snapshot/blob cleanup;
- deletion evidence.

### Part 15 — Cryptographic Integrity and Optional End-to-End Protected Payloads

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
