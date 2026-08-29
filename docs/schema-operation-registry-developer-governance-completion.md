# Part 29 completion: schema, operation registry, and developer governance

## Delivered boundary

Part 29 is implemented as a compile-time, database-neutral durable-contract registry. Canonical RON fragments under `registry/` are merged in lexical order, validated, checked against the immutable published-ID lock, and compiled into static Rust constants and lookup tables. Production lookup does not parse RON or query a database.

The implementation separates:

- `aequora-registry-types`: ID newtypes, lifecycle/governance metadata, source and release contracts;
- `aequora-registry-codegen`: bounded loading, lint, semantic digest, lock verification, compatibility diff, Rust generation, and Markdown generation;
- `aequora-registry-generated`: immutable runtime IDs/lookups, fail-closed admission, and startup binding checks;
- `aequora-registry-cli`: `verify`, `lint`, `diff`, `compatibility`, `explain`, `reserve`, `docs`, and `lock` developer workflows.

Core, application, and extension fragments share collision validation. Numeric ranges distinguish core, registered-extension, vendor-private, and experimental allocations. Application-owned entity, operation, event, field, job, consumer, audit, and migration sources intentionally begin empty: Aequora does not claim application business IDs. Adding them through `registry/app` or `registry/extensions` activates the same generated and compatibility gates.

## Existing-contract integration

The initial registry records the existing protocol/message kinds, all Part 21 capabilities, built-in consistency profile families, operational error categories, control-plane permissions, admin actions/reasons, and artifact formats. Operational errors now have explicit numeric discriminants and a stable `registry_id()` rather than relying on declaration order. Cross-crate contract tests prove generated IDs match the existing codec, compatibility, error, and control-plane surfaces.

Authenticated diagnostics expose the embedded registry generation, digest, and entry count. The umbrella crate exports both generated lookup APIs and registry contract types.

## Governance and compatibility enforcement

Validation rejects zero/duplicate IDs, duplicate names, missing ownership or descriptions, invalid ranges, experimental-range misuse, unresolved references, missing domain metadata, unsafe capability fallback, and security-sensitive entries without a proposal reference. The lock rejects deletion or semantic reuse of published IDs. Compatibility diff reports additions, deprecation, schema regression, semantic breakage, and security-required changes. Generated documentation drift fails `verify`.

The checked-in proposal template covers semantics, compatibility, offline clients, migration/upcast, replay, security/governance, rollback, and testing. `RegistryChangeProposal`, `RegistryReleaseManifest`, governance levels, change classifications, extension manifests, and vendor namespace IDs provide typed automation boundaries.

Runtime admission rejects unknown IDs and lifecycle-forbidden new operations. Experimental durable creation requires explicit opt-in. Retry-only contracts remain resolvable for historical retries, while removed/reserved entries remain explainable but non-executable. Startup validation rejects unregistered handlers and requires handlers for every compiled current, supported, deprecated, or retry-only operation; configured consumer projectors must resolve canonically.

## Invariants and acceptance evidence

`AEQ-INV-REGISTRY001` through `AEQ-INV-REGISTRY009` are registered in `aequora-invariants` with model, property, adapter, and diagnostic evidence names. Focused tests cover duplicate IDs, removed-ID reuse, missing owner, schema regression, unresolved references, required-safety fallback, unknown ID admission, retry-only lifecycle, generated lookup resolution, startup handler validation, ID parity, and reservation ranges.

The repository gate is `scripts/check-registry-architecture.sh`; CI runs it, all four registry crate suites, and the cross-crate registry contracts. The gate also enforces that runtime registry layers remain free of database, transport, and async-runtime dependencies.

## Explicit v1 decisions

- RON remains source-controlled input; runtime mutation of compiled semantics is unsupported.
- The lock and Git/release history are the historical registry database; no production database copy is required.
- Reservation reports the next free ID but does not mutate source files, preserving reviewable merge conflicts.
- External signed registry packages, a hosted namespace allocation service, derive macros, and a schema-explorer UI remain future ecosystem features, not prerequisites for the compile-time v1 governance boundary.
