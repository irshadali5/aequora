# Part 35 public Rust SDK completion evidence

The authoritative design is [Part 35](../sys-arch/35-public-rust-api-sdk-stability-architecture.md). This report records executable repository evidence and does not redefine the architecture.

## Implemented surface

- `aequora` is now a small default facade over public client, server, operation, adapter, and identifier crates. Its prelude contains semantic application concepts only; physical adapters remain explicit dependencies.
- `aequora-operation` provides strong operation kind/schema types, the manual `Operation` contract, opaque encoded intent, local-only mutation receipts, and high-level operation state.
- `aequora-adapter-sdk` provides intentionally open, database/transport-framework-neutral extension traits for stores, transports, credentials, domain handlers, authentication, authority commit, and observability. It exposes no cursor mutation.
- `AequoraClient` is a cheap shared handle with runtime-validated construction, durable `mutate`, bounded single-flight `sync_now`, scheduler hints, durable status, bounded advisory events, explicit shutdown, and grouped operation/conflict/scope/blob/diagnostic namespaces.
- `AequoraServer` validates its authority, registry, and authenticator before serving. `DomainRegistry` rejects duplicate kinds, missing schema, and missing profiles. Business rejection/conflict remain distinct from adapter/system failure.
- `AequoraErrorCode` and retry classes form a stable error compatibility boundary. Public enums likely to grow are non-exhaustive and secrets redact in `Debug`.

## Safety and evolution evidence

- `AEQ-INV-SDK001` through `AEQ-INV-SDK010` are registered with model, property, adapter/gate, and diagnostic evidence hooks.
- `scripts/check-public-sdk-architecture.sh` rejects forbidden physical/framework leaks, cursor mutation entry points, unbounded event channels, facade drift, missing stable codes, missing CI semver automation, and incomplete public documentation.
- `architecture/public-rust-api-v0.1.txt` is the reviewed stable surface and error-code snapshot.
- [SDK versioning and upgrades](sdk-versioning-and-upgrades.md) separates crate, protocol, store, operation, and adapter versions and defines MSRV/deprecation policy.
- CI runs the focused structural gate on every quality build and `cargo-semver-checks` against the base revision for pull requests.

## Resource-bounded development

Repository-local VS Code settings disable rust-analyzer check-on-save, cache priming, build-script execution, proc-macro expansion, all-target analysis, and all-feature analysis; worker and Cargo jobs are limited to one. Terminal and CI checks remain the source of compilation truth.

## Verification

The final implementation handoff records focused tests, formatting, strict Clippy, warning-denied Rustdoc, Guppy boundaries, database-neutrality, the focused architecture gate, workspace tests, retrieval-index refresh, and diff validation actually executed in this worktree.
