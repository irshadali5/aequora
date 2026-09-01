# Part 36 storage adapter SDK completion

The governing authority is
[`sys-arch/36-storage-adapter-sdk-official-adapter-architecture.md`](../sys-arch/36-storage-adapter-sdk-official-adapter-architecture.md).
This report records executable evidence and does not replace that design.

## Implemented scope

| Requirement | Evidence |
|---|---|
| Stable adapter SDK | `aequora-adapter-sdk` now exposes role-oriented modules for local and authority transactions, outbox/cursor, journal/ledger, snapshot/object, fencing, migration, records, capabilities, normalized errors, and conformance. |
| Atomic transaction boundaries | `LocalTransaction` couples domain mutation with outbox insertion; `AuthorityTransaction` couples business mutation, versioned journal publication, operation ledger outcome, and required audit evidence. Concrete transaction types remain associated types owned by adapters. |
| Capability and startup model | Stable numeric `AdapterId`/`CapabilityId`, independent versions, levels, roles, support labels, concurrency model, manifests, `CertifiedEnvironment`, and fail-closed `AdapterRequirements::verify`. |
| Error/retry model | Canonical categories include unavailable, conflict, disk full, read-only, corruption, constraint, serialization, timeout, and ambiguous commit. `RetryDisposition` separates retry, reopen, recovery, and ledger resolution. |
| Migration safety | Unique ordered checksummed `AdapterMigration` plans distinguish adapter and domain schema versions; privileged execution/hook traits are explicit. |
| Public conformance | Factory/probe traits and bounded local, authority, snapshot, and fencing runners emit standard `TestObservation` values. TestKit proves unique aggregating runner behavior. |
| Certification integration | New registry profiles `LocalAdapter`, `AuthoritativeAdapter`, `SnapshotAdapter`, and `FencingAdapter`; tests 49–58 cover `AEQ-INV-ADAPTER001`–`010`. Rust and Markdown registry snapshots are generated from canonical RON. |
| Official manifests | PostgreSQL and Stoolap publish Part 36 manifests with roles, versioned capabilities, engine/target baselines, concurrency, ownership, and limitations. Runtime/deployment claims still require exact environment evidence. |
| Ecosystem policy/docs | `docs/storage-adapter-sdk.md` provides adapter-author guidance, support matrix, conformance flow, security rules, and official/community release policy. SQLite is now implemented by Part 42; standalone object-store support remains explicitly unclaimed. |
| Structural/release gate | `scripts/check-storage-adapter-sdk-architecture.sh` rejects physical dependencies/type leakage, checks invariants/registry/manifests, and runs focused tests; CI invokes it. |

## Verification contract

The focused Part 36 gate is:

```bash
bash scripts/check-storage-adapter-sdk-architecture.sh
```

Repository completion additionally requires formatting, strict Clippy, Rust 1.87 all-target/all-
feature checking, warning-denied Rustdoc, workspace tests, Guppy workspace architecture,
database-neutrality, registry verification, and diff hygiene. Commands run with one Cargo build job
on constrained systems.

## Completed verification

On 2026-08-31, the focused Part 36 gate, the full locked/offline all-feature workspace test suite,
strict workspace Clippy, Rust 1.87 all-target/all-feature checking, and warning-denied workspace
Rustdoc passed with `CARGO_BUILD_JOBS=1`. The Guppy and executable workspace-boundary checks,
database-neutrality profiles, registry generation verification, public SDK, reference
implementation, local-storage, and certification architecture gates also passed. Formatting and
diff hygiene were clean. The workspace test suite was rerun with loopback-listener permission after
the restricted sandbox rejected the HTTP listener; that permitted rerun passed.

Live PostgreSQL/Neon, mobile target, forced process-kill, filesystem fault, and performance evidence
remain environment-bound certification inputs. Offline compilation or the checked-in manifest does
not claim those environments passed.
