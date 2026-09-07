# Part 43 configuration, secrets, runtime policy, and feature flags completion

The governing authority is
[`sys-arch/43-configuration-secrets-environment-profiles-runtime-policy-feature-flags-architecture.md`](../sys-arch/43-configuration-secrets-environment-profiles-runtime-policy-feature-flags-architecture.md).
This report records executable repository evidence and does not replace that design.

## Implemented scope

| Requirement | Executable evidence |
|---|---|
| Typed configuration and profiles | `aequora-config` retains its existing runtime mappings and adds a strict Part 43 deployment schema, closed environment enum, current schema version, checked `RawDeploymentConfig -> ValidatedDeploymentConfig -> EffectiveConfig` typestate, cross-field validation, and production/staging fail-closed rules. Durable authority, device, and store-generation identities are absent from the editable schema. |
| Deterministic source precedence | `ConfigLoader` applies compiled defaults, base RON, environment RON, typed environment overrides, CLI overrides, and runtime policy in fixed order independent of builder call order. Effective fields carry non-secret source provenance. Unknown RON fields and unknown `AEQUORA_` overrides fail closed. |
| Secret boundary | `aequora-secrets` defines serializable references, async provider-neutral resolution, environment/OS/external provider composition, Unix mounted-file permission enforcement, rotation classes, and non-serializable zeroizing secret byte/string wrappers whose diagnostics are always redacted. |
| Runtime policy and atomic reload | `aequora-policy` provides bounded batch/worker/duration types, mutability and change classes, semantic digests, immutable policy snapshots, monotonic generations, field-specific safe merge rules, and atomic whole-`Arc` publication. Invalid and restart-required reloads retain the previous generation. |
| Feature rollout | `aequora-feature-flags` separates Cargo compilation from runtime activation, validates safety classes, requires migration/capability evidence for semantic changes, requires differential evidence for compatible paths, rejects security-policy experimentation, and uses stable BLAKE3 assignment over feature, tenant, and rollout generation. Offline cache expiry/generation checks fail disabled. |
| Entitlement and policy authority | Feature decisions consume authoritative entitlements but never create them. Policy merge uses minimum, required-wins, and allowlist intersection rather than generic last-write-wins. |
| Adapter isolation | Logical PostgreSQL/Neon and SQLite/Stoolap selection is typed while credentials remain `SecretRef` values. Startup validation checks declared atomicity and snapshot capabilities and never silently downgrades. The three neutral Part 43 crates have no runtime, transport, UI, cloud-config-service, or physical database dependency. |
| CLI and documentation metadata | `aequora config check`, `effective`, `explain`, and `diff` validate without service startup, emit sanitized effective state, describe typed setting metadata, and classify reload/restart/forbidden/security-sensitive changes. `CONFIGURATION_REFERENCE` is the documentation source. |
| Tests, invariants, and conformance | Unit and testkit suites cover parsing, strict unknown fields, validation, precedence, production safety, redaction, provider failure, migration, capability rejection, policy merge, failed reload preservation, concurrent coherent reads, deterministic rollout, and entitlement separation. `AEQ-INV-CONFIG001`–`010`, conformance tests 114–123, and the `ConfigurationRuntimeFull` profile are registered and generated. |
| Structural gate | `scripts/check-configuration-architecture.sh` verifies crate/symbol presence, invariant parity, neutral dependencies, absence of correctness-disabling switches, focused tests, registry parity, CLI validation, and effective-output redaction. CI executes it. |

## Verification contract

The focused gate is:

```bash
bash scripts/check-configuration-architecture.sh
```

Repository completion also requires formatting, strict Clippy, Rust 1.87 all-target/all-feature
checking, warning-denied Rustdoc, locked/offline workspace tests, Guppy boundaries,
database-neutrality, registry generation/verification, and diff hygiene.

## Deployment responsibility

The repository proves the provider boundary, permission checks available on the test host, typed
rotation classification, redaction, and fail-closed behavior. A concrete application must still
register its Android/iOS/Windows/macOS/Linux or cloud secret provider, apply deployment filesystem
and process policy, and bind any remotely distributed policy to its authenticated control plane.
No external configuration service is required.

## Completed verification

Verified on 2026-09-07 with `CARGO_BUILD_JOBS=1`:

- the focused Part 43 gate, including configuration, secret, runtime-policy, feature-policy,
  testkit, invariant, conformance, registry, CLI validation, and CLI redaction tests;
- locked/offline all-feature workspace tests, after granting the existing loopback HTTP test the
  required local bind permission;
- strict workspace Clippy and all-target/all-feature checking;
- Rust 1.87 all-target/all-feature checking and warning-denied workspace Rustdoc;
- Guppy/workspace boundaries (22 rules, 94 crates, 9 layers, 12 isolated dependencies);
- database-neutral custom, Stoolap, SQLite, PostgreSQL, and combined profiles; and
- registry Rust/Markdown generation and verification, formatting, and diff hygiene.

The first sandboxed workspace test attempt failed only because the existing `aequora-http`
loopback listener received `Operation not permitted`; the permitted rerun passed. The semantic
retrieval refresh downloaded its model and indexed the new Part 43 configuration code, but its
full GraphRAG scan was stopped after prolonged processing to preserve bounded laptop resources.
This does not affect source, build, or test verification. Environment-bound production secret
stores and signed remote policy distribution remain application/deployment evidence and are not
fabricated by this repository.
