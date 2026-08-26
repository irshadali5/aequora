# Welcome to the Aequora Architecture Wiki 📖

The **Aequora Distributed Synchronization Engine** is a local-first, server-authoritative distributed sync system designed for high durability, zero data loss, offline capability, formal correctness, and low-latency replication.

This wiki provides the complete 30-part architecture specification, detailing all subsystem designs, formal invariants, protocols, and developer governance rules.

---

## 📚 Architectural Tiers & Specifications

### 🧩 Tier 1: Core Foundations & Correctness
*Formal state machine invariants, causal event lineage, anti-entropy self-repair, and offline queue compaction.*

- **[[Part 01: Formal Correctness, Invariants & Simulation|01-formal-correctness]]**  
  Executable invariant registry, state machine model checking, fault injection, and deterministic simulation harness.
- **[[Part 02: Causality, Dependency & Event Lineage|02-causality-provenance-lineage]]**  
  Lamport clocks, hybrid logical timestamps (HLC), dependency DAGs, causal cut consistency, and lineage tracking.
- **[[Part 03: Anti-Entropy, Divergence Detection & Self-Repair|03-anti-entropy-self-repair]]**  
  Merkle tree range exchange, cryptographic state fingerprints, divergence resolution, and continuous self-repair protocols.
- **[[Part 04: Offline Operation Compaction & Rebase|04-offline-compaction-rebase]]**  
  Local operation log compaction, redundant write coalescing, timeline rebasing, and queue footprint minimization.

---

### 🔄 Tier 2: Local Coordination & Scheduling
*Multi-process coordinator election, QoS adaptive scheduling, dynamic dataset scopes, and live push.*

- **[[Part 05: Local Multi-Process & Coordinator Election|05-local-multiprocess-coordination]]**  
  Shared storage locking, single-coordinator election across tabs/processes, IPC notification channels, and leader failover.
- **[[Part 06: Adaptive Sync Scheduler & QoS|06-adaptive-sync-scheduler-qos]]**  
  Priority queuing, dynamic rate adaptation, battery/network-aware sync policies, and latency-vs-throughput trade-offs.
- **[[Part 07: Subscription, Scope & Dynamic Datasets|07-subscription-scope-dynamic-dataset]]**  
  Partial sync subscriptions, parameterized view filters, dynamic scope expansion, and authorization boundary enforcement.
- **[[Part 08: Live Sync, Push Hints & Presence|08-live-sync-push-presence]]**  
  WebSocket/QUIC live push notifications, ephemeral presence indicators, fast invalidation hints, and fallback mechanisms.

---

### ⚡ Tier 3: Data Transfer & Execution
*Bulk migration, streaming snapshots, operation consistency profiles, and deterministic replay.*

- **[[Part 09: Bulk Import, Export & Seed Migration|09-bulk-import-export-seed-migration]]**  
  High-throughput batch ingest, transactional seeding, legacy dataset migration, and baseline verification.
- **[[Part 10: Large Snapshot & Streaming Bootstrap|10-large-snapshot-streaming-bootstrap]]**  
  Chunked snapshot transmission, out-of-band artifact transfer, resumable streaming, and cold-replica bootstrap.
- **[[Part 11: Operation Semantics & Consistency Profiles|11-operation-semantics-consistency-profiles]]**  
  CRDT vs operational transformation semantics, multi-key transaction boundaries, and customizable consistency profiles.
- **[[Part 12: Deterministic Domain Execution & Replay|12-deterministic-execution-replay]]**  
  Hermetic domain handlers, recorded input replay, time-travel debugging, and bit-identical verification.

---

### 🔐 Tier 4: Audit, Governance & Security
*Data provenance, retention/erasure lifecycle, end-to-end cryptographic encryption, and failover.*

- **[[Part 13: Data Provenance, Auditability & Explainability|13-data-provenance-auditability-explainability]]**  
  Tamper-evident audit logs, cryptographic signatures, historical state explainability, and actor attribution.
- **[[Part 14: Data Governance, Retention, Legal Hold & Erasure|14-data-governance-retention-erasure]]**  
  GDPR/CCPA right-to-be-forgotten erasure cascades, legal hold fences, tombstone expiration, and data classification.
- **[[Part 15: Cryptographic Integrity, Key Management & E2E|15-cryptographic-integrity-key-management-e2e]]**  
  Zero-knowledge payload encryption, envelope encryption, asymmetric key rotation, and signed transaction envelopes.
- **[[Part 16: Authority Failover, Timeline Epochs & Fork Detection|16-authority-failover-timeline-epochs-fork-detection]]**  
  Authoritative epoch transitions, split-brain fork prevention, automated promotion protocols, and quorum fencing.

---

### 🚀 Tier 5: Scale & High Performance
*Multi-region topologies, backpressure flow control, memory zero-copy boundaries, and embedded clients.*

- **[[Part 17: Multi-Region Read & Single-Writer Global Deployment|17-multi-region-read-single-writer-global]]**  
  Edge read replicas, asynchronous authority replication, global routing, and geographic latency optimization.
- **[[Part 18: Backpressure, Admission Control & Fairness|18-backpressure-admission-fairness-overload]]**  
  Token bucket rate limiting, adaptive load shedding, fair-share multi-tenant quotas, and memory watermark throttling.
- **[[Part 19: Performance Engineering & Memory Architecture|19-performance-engineering-memory-architecture]]**  
  SIMD acceleration, memory-mapped ring buffers, arena allocation, zero-copy protocol framing, and cache efficiency.
- **[[Part 20: Resource-Constrained Client Architecture|20-resource-constrained-client-architecture]]**  
  Wasm/embedded footprints, low-power sleep modes, storage budget eviction, and mobile memory constraints.

---

### 🛠️ Tier 6: Protocols, Metadata & Workflows
*Protocol evolution, metadata persistence, durable workflows, and administrative control.*

- **[[Part 21: Protocol Negotiation & Compatibility Governance|21-protocol-negotiation-compatibility-governance]]**  
  Schema version handshakes, backward/forward protocol compatibility, semantic feature flags, and deprecation cycles.
- **[[Part 22: Sync Metadata Schema & Internal Persistence|22-sync-metadata-schema-internal-persistence]]**  
  Database storage schemas, indexes, change-log tables, state vector representations, and database neutrality.
- **[[Part 23: Background Jobs, Durable Workflows & Side Effects|23-background-jobs-durable-workflows-side-effects]]**  
  Transactional outbox patterns, idempotent task execution, external webhook triggers, and durable saga workflows.
- **[[Part 24: Operational Control Plane & Admin API|24-operational-control-plane-admin-api]]**  
  Cluster topology management, client session inspection, live configuration toggles, and metrics endpoints.

---

### 🔍 Tier 7: Diagnostics, Governance & Conformance
*Reproducible diagnostics, legacy migration, threat models, change feeds, schema governance, and certification.*

- **[[Part 25: Diagnostics, Forensics & Incident Bundles|25-diagnostics-forensics-reproducible-incident-bundles]]**  
  Flight-recorder telemetry, sanitized incident bundle export, deterministic post-mortem replay, and crash dumps.
- **[[Part 26: Legacy Application Compatibility & Migration|26-legacy-application-compatibility-incremental-adoption]]**  
  Dual-write bridge adapters, legacy SQL interception, shadow mode sync, and zero-downtime migration paths.
- **[[Part 27: Security Threat Model & Abuse Resistance|27-security-threat-model-abuse-resistance]]**  
  Threat vectors, Byzantine fault mitigations, rate exhaustion defenses, Sybil protections, and penetration test guidelines.
- **[[Part 28: Multi-Consumer Change Feed Architecture|28-multi-consumer-change-feed-architecture]]**  
  Kafka/streaming integration, consumer group offsets, at-least-once delivery semantics, and event fan-out.
- **[[Part 29: Schema & Operation Registry Governance|29-schema-operation-registry-developer-governance]]**  
  Schema registry contracts, automated linting, CI/CD breaking change detection, and developer SDK tooling.
- **[[Part 30: Certification, Conformance & Ecosystem Architecture|30-certification-conformance-ecosystem-architecture]]**  
  Automated compliance test suites, standard reference benchmarks, plug-in provider certification, and ecosystem packaging.
