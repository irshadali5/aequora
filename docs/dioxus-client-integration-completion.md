# Part 40 Dioxus client integration completion

The governing design is
[`sys-arch/40-dioxus-client-integration-reactive-state-architecture.md`](../sys-arch/40-dioxus-client-integration-reactive-state-architecture.md).
This report records executable repository evidence and does not replace that design.

## Implemented boundary

`aequora-dioxus` provides one provider-owned `AequoraHandle` per local store, explicit
`QueryNamespace` isolation, durable-derived query hooks, local-first mutation hooks, coarse sync
status, advisory event/bootstrap/connectivity views, conflict and scope hooks, redacted diagnostics,
and platform lifecycle forwarding. `aequora-client` remains independent from Dioxus.

The Dioxus dependency enables only `hooks` and `signals`. Renderers, desktop/web/mobile runtimes,
hot reload, routing, assets, and HTML are application composition choices and are not pulled into
the integration crate.

## State and event semantics

- `QueryState` renders an existing value as `Refreshing` while a local reread is active. A query
  hook runs the application repository closure against durable local state; Aequora is not an ORM.
- `MutationState::SavedLocally` contains a `MutationReceipt` returned after the client adapter's
  atomic local commit. It never means server confirmation. Authoritative status remains queryable by
  `OperationId` through `AequoraClient::operations()`.
- Post-commit `Invalidation` notices use latest-value watch semantics. Intermediate notices may be
  coalesced; a subscriber that detects a skipped generation conservatively rereads its durable query.
  A slow component therefore cannot block or create an unbounded event queue.
- Provider event bridging converts high-level client events into scope/key/all-query hints. Event
  loss is safe because queries, operation status, conflicts, and diagnostics can be reread.
- Explicit namespaces and provider-local channels prevent state from one store/account waking or
  populating another store's query cache. Tenant/store replacement remounts the provider boundary
  with a new Dioxus key; provider context is intentionally immutable during one mount.
- Platform connectivity and lifecycle notifications are advisory. WorkManager, BGTaskScheduler,
  desktop agents, durable scheduling, cursors, retry, and reconciliation stay below Dioxus.

## Verification contract

`scripts/check-dioxus-client-architecture.sh` verifies the crate layout, public surface, all ten
Part 40 invariants, Dioxus dependency isolation, absence of physical storage/transport dependencies,
resource-bounded rust-analyzer settings, focused tests, conformance profile, and registry integrity.

The crate tests cover targeted invalidation, conservative lost-notice recovery, rapid store
isolation, a 10,000-event storm with latest-value coalescing, coarse status/bootstrap presentation,
and offline platform hints without replacing or closing the durable client.

Repository completion additionally requires formatting, strict Clippy, warning-denied Rustdoc,
Guppy boundaries, database neutrality, and the focused Part 40 gate. Commands should use
`CARGO_BUILD_JOBS=1` on resource-constrained development machines.

## Completed verification

Verified on 2026-09-01 with one Cargo build job: the Part 40 gate, strict focused Clippy, focused
warning-denied Rustdoc, Rust 1.87 all-target/all-feature checking, Guppy workspace boundaries, all
database-neutral composition profiles, registry generation/integrity, and workspace formatting.

The semantic RAG index was not rebuilt because its earlier GraphRAG pass was already observed to be
resource-intensive in this worktree and the active requirement is to avoid another system freeze.
No repository-wide fresh-index claim is made; all edited files were verified directly.
