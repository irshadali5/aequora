# Part 10 Large Snapshot, Streaming Bootstrap, and Resumable Transfer Completion

Part 10 is implemented at Aequora's database-neutral reusable boundary. The framework defines and
tests portable manifest, transfer, staging, and activation safety; adapters provide consistent
database views, physical chunk delivery, and native generation transactions.

## Repository guarantees

| Requirement | Evidence |
|---|---|
| Durable workflow | `BootstrapJobId`, `BootstrapJob`, and the fail-closed `BootstrapState` graph |
| Exact boundary | `SnapshotBoundary` binds scope identity, version, generation, sequence, and authority epoch |
| Deterministic manifest | `SnapshotManifest::build/verify` sorts entities and binds metadata, totals, ordered descriptors, and root digest |
| Stable chunks | Content-bound `ChunkId`, deterministic ordinals/ranges, exact record counts, and independent BLAKE3 digests |
| Bounded encoding | `ChunkingConfig` enforces record, chunk, per-chunk byte, and total-byte ceilings |
| Resumable transfer | Durable `ChunkProgress` plus `BootstrapJob::record_chunk_read` persists contiguous offsets and attempt state |
| Safe ranges | `RangeResumeGuard` and object identity reject cross-object append, offset gaps, overflow, and false completion |
| Verification | `SnapshotChunk::decode_verified` checks descriptor identity, size, digest, count, and strict entity order before install |
| Delivery neutrality | `ChunkLocation` supports service, HTTP range, and opaque object-store/CDN references without storing signed URLs |
| Staging generation | `ReplicaGeneration`, `SnapshotSink::begin_staging/install_chunk/verify_staging`, and explicit chunk phases |
| Atomic activation | `verify_activation`, `VerifiedActivation`, and `SnapshotSink::activate` bind the complete generation and boundary cursor |
| Delta bridge | Activation returns the exact snapshot sequence from which normal journal replay starts at N+1 |
| Retention | `SnapshotLease` and `SnapshotLeaseStore` prove the required post-snapshot journal range remains readable |
| Pending intent | `PendingIntentPlan` commits sorted operation identities and immutable sent operations into the activation digest |
| Scope safety | Activation rechecks authorization, scope version/generation, and authority epoch; revoked staging has a quarantine hook |
| Adapter contracts | `SnapshotSource`, `SnapshotReadView`, `SnapshotChunkSource`, `SnapshotSink`, and `SnapshotInstallCapability` |
| Resource preflight | `BootstrapPreflight` checks disk, reserved space, memory, queue, and maximum decoded chunk size without overflow |
| Scheduling and telemetry | `LargeBootstrapTransfer`, `BootstrapActivation`, `LargeBootstrapEventKind`, aggregate counters, and payload-free tracing |
| Invariants | `AEQ-INV-BS001` through `AEQ-INV-BS006` in the shared invariant registry |
| CLI | `aequora bootstrap inspect`, `status`, and `explain` are read-only and report no payloads or signed locations |

## Executable evidence

`aequora-bootstrap` unit and property tests cover input-order-independent manifests, chunk/root
tampering, strict job transitions, manifest drift on resume, partial range continuation, object
identity drift, chunk phase progression, lease coverage, disk/memory preflight, and complete
activation evidence.

`aequora-testkit::large_bootstrap::FaultInjectingSnapshotSink` and
`large_bootstrap_contracts` inject failure after records but before chunk progress, chunk commit
followed by response loss, activation failure before commit, and activation commit followed by
response loss. They prove partial staging remains inactive, retries are idempotent, activation is
one generation/cursor switch, and pending operation identities survive.

The existing client still supports bounded page and streaming bootstrap through
`ClientSyncEngine::bootstrap` and `bootstrap_streaming`. The new crate supplies the scalable
manifest/object-transfer contract that adapters can place beneath those application workflows.

## Host and deployment responsibilities

A production large-bootstrap integration must still:

- implement a consistent scoped `SnapshotReadView` over its authority database and ensure every
  emitted record belongs to the declared boundary;
- publish all immutable chunks before publishing the manifest, protect object references, refresh
  short-lived signed URLs, and enforce tenant/scope authorization at every manifest/range request;
- implement `SnapshotSink` so verified records and chunk progress share one bounded native
  transaction, while activation atomically switches generation and cursor without exposing staging;
- keep the snapshot lease renewed or restart from a current snapshot when required journal history
  expires;
- preserve optimistic overlays, rebase unsent operations, and retry possibly delivered operations
  with their original `OperationId` after activation;
- stop and remove/seal staged data on revocation, scope-generation change, or incompatible authority
  epoch before activation;
- validate compression/decompression bounds for its selected codec and apply transport-specific
  timeout, retry, concurrency, network, battery, and background policies;
- prove real fresh-client N+1 catch-up, rollback/cleanup, multi-GB disk and RAM ceilings, object-store
  outage behavior, thundering-herd control, and production throughput.

Repository completion does not claim a live object store, CDN, multi-GB database snapshot, or a
particular local engine's generation swap has been exercised unless that adapter supplies its own
acceptance evidence.
