# Part 20 resource-constrained client architecture evidence

This report maps `sys-arch/20-resource-constrained-client-architecture.md` to executable,
database-neutral repository surfaces. Resource policy changes when and how much optional work is
performed; it does not change synchronization, authorization, cursor, outbox, or governance
semantics.

| Requirement | Implementation evidence |
|---|---|
| Resource context and platform boundary | `aequora_client::resources::ClientResourceContext` models memory, storage, network, power, background budget, and thermal state. `PlatformResourceMonitor` is the Rust boundary for Android, iOS, desktop, or future browser bridges. |
| Constrained profiles | `ClientResourcePolicy` provides validated `MobileMinimal`, `MobileStandard`, `DesktopConservative`, and `DesktopStandard` profiles with hard buffer, page, worker, transfer, storage, blob-spool, network, power, and thermal caps. |
| Scheduler integration | `DefaultResourceAdmission` maps durable work kinds to `RunNow`, `RunReduced`, `Defer`, user approval, or hard-resource rejection. `ClientSyncEngine` evaluates this policy before work and applies the resource ceiling after Part 06 scheduler/server limits. |
| Bounded bootstrap/blob work | Profiles cap snapshot chunks, pipeline bytes, transfers, and CPU workers. Existing Part 10 durable chunk checkpoints and Part 19 streaming snapshot/blob APIs remain the data path; Part 20 only supplies conservative device caps. |
| Low-storage safety | `StoragePreflight` preserves a transaction reserve. `EvictionPlan` follows the specified rebuildable-data order and cannot select unsynced outbox, pending blobs, key/cursor metadata, required tombstones, required scope base, or pending-intent-pinned scopes. |
| Local-first commit truth | `LocalCommitReceipt` can be constructed only from a proven atomic domain mutation plus durable outbox append; a partial/disk-full commit cannot be reported as saved. |
| Scope/cache policy | `ScopeCachePolicy` implements required, recent, on-demand, and transient policies. Pending intent pins referenced base state under every storage state. Snapshot cache policy supports none, installed-only, and recent retention. |
| Lifecycle and crash recovery | Platform lifecycle actions flush scheduler metadata, commit checkpoints, close transient sockets, and trigger foreground catch-up. `DurableWorkCheckpoint` advances one unit only after durable apply/checkpoint proof; `RetryCheckpoint` persists storm control across process death. |
| Network, battery, and thermal behavior | Metered/roaming policy permits small interactive sync while deferring or requiring approval for bulk work. Low power and hot thermal states reduce workers/compression or defer optional maintenance; required security work is not weakened by convenience policy. |
| Capability/privacy | `ClientCapabilityProfile` discloses only coarse `LowMemory`/`Standard` class plus transport ceilings and compression support. `ResourceConstrainedV1` is transport capability, never authorization evidence. |
| Diagnostics | Bounded diagnostic rings, payload-free resource log events, local byte accounting, and sanitized user diagnostics avoid unbounded logs and domain payload disclosure. |
| Store format and upgrades | `LocalStoreFormatVersion` rejects newer incompatible stores and requires explicit migration for older formats. Resource policy does not silently open or downgrade an incompatible store. |
| Invariants and tests | `AEQ-INV-CLIENT001` through `AEQ-INV-CLIENT009`, `client_resource_contracts`, and `scripts/check-client-resource-architecture.sh` enforce intent, cursor, directive, memory, checkpoint, eviction, atomic-save, semantic-parity, and telemetry guarantees. |

## Fault profiles covered

- very-low-memory batch/decode/snapshot/worker ceilings;
- low/critical storage, protected eviction, and disk-full local-save failure;
- metered network with high RTT, interactive sync, bulk deferral, and explicit override;
- process death before durable apply/checkpoint and restart-safe continuation;
- suspended background execution and hot-device maintenance deferral;
- low-power required security work and coarse capability negotiation.

Existing Part 04, Part 10, Part 14, Part 15, and Part 19 tests continue to cover outbox compaction,
streaming bootstrap, retention/purge, crypto fail-closed behavior, and bounded large-object paths.

## Platform/deployment acceptance still required

- connect native Android/iOS/desktop monitors to their OS lifecycle, storage, network, power, and
  thermal APIs;
- run real process-memory caps, disk-full injection, OS background termination, week-offline,
  mobile data accounting, and large foreground-download tests on each supported application;
- configure adapter-specific database cache/vacuum settings and hardware-backed secure storage;
- measure application/domain entity sizes, storage watermarks, blob quotas, and battery/network
  policy on representative devices.

Those integrations are application/platform responsibilities. The reusable core remains runtime-
and database-neutral and does not claim mobile certification from host-only tests.

