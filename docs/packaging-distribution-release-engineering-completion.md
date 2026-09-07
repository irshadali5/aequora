# Part 44 packaging, distribution, and release engineering completion

The governing authority is
[`sys-arch/44-packaging-distribution-release-engineering-artifact-signing-update-channels-cross-platform-delivery-architecture.md`](../sys-arch/44-packaging-distribution-release-engineering-artifact-signing-update-channels-cross-platform-delivery-architecture.md).
This report records executable repository evidence and does not replace that design.

## Implemented scope

| Requirement | Executable evidence |
|---|---|
| Release and compatibility dimensions | `aequora-release` separates SemVer from protocol, operation, configuration, snapshot, registry, store-format, adapter, and migration-set dimensions. Strict manifest validation binds source revision, deterministic build identity, toolchain, target, features, workflow, registry, migrations, and every artifact. |
| Artifact integrity and signatures | Final bytes receive SHA-256 and BLAKE3 hashes plus an Ed25519 signature. Release manifests and update metadata have separate domain-separated signatures. The crypto key model adds distinct Git-tag, artifact, manifest, update, container, and platform signing purposes with lifecycle and validity checks. |
| Immutable publishing | `ImmutableReleaseIndex` rejects a semantic version rebound to different manifest bytes. `PromotionEvidence` requires all nine Part 44 release-gate categories, identical candidate/promoted digests, approval policy, immutable source identity, and absence of production credentials from build jobs. |
| Update trust and rollout | Signed metadata carries generation, channel, target, artifact hashes/size, minimum compatible version, criticality, availability/halt/revocation, rollout basis points, and validity window. Decisions check channel, policy, downgrade, protocol, config, registry, store formats, and a stable privacy-preserving cohort. Enterprise-pinned installations remain externally managed. |
| Upgrade and rollback | `aequora-update` adds an atomic sequence with a durable checkpoint over pending operations, cursor, conflicts, store identity, intent digest, and store format. Only the migration stage may advance format. Rollback classes reject forward-only state and require verified rollback migration evidence where declared. |
| Cross-platform delivery | [`release/support-matrix.ron`](../release/support-matrix.ron) is a validated machine-readable matrix for Linux x86_64/arm64, Windows, macOS Intel/Apple Silicon, Android, iOS, OCI amd64/arm64, and crates.io, with package formats, native-signing/native-runner requirements, and target smoke-test identities. |
| Release tooling and offline verification | The `aequora-release` binary streams SHA-256/BLAKE3 calculation, validates the support matrix, and authorizes promotion evidence. `aequora release verify` validates signed manifests and locally supplied artifacts against an independently supplied public trust store, enabling air-gapped and enterprise-mirror verification. |
| Tests, invariants, and conformance | Unit suites cover byte tampering, signing-purpose separation, immutable versions, compatibility decisions, support matrices, stable promotion, durable upgrade state, and rollback. Testkit adds cross-crate release contracts. `AEQ-INV-RELEASE001`–`010`, conformance tests 124–133, and `ReleaseEngineeringFull` are registered and generated. |
| Structural and CI gates | `scripts/check-release-architecture.sh` checks required surfaces, invariant parity, key-purpose separation, neutral dependencies, focused tests, registry parity, support-matrix tooling, and offline verification CLI exposure. The normal CI quality job executes it. |

## Verification contract

The focused gate is:

```bash
bash scripts/check-release-architecture.sh
```

Repository completion also requires formatting, strict Clippy, Rust 1.87 all-target/all-feature
checking, warning-denied Rustdoc, locked/offline workspace tests, Guppy boundaries,
database-neutrality, registry generation/verification, and diff hygiene.

## Deployment-bound evidence

The repository defines and verifies the portable trust and policy boundary. Production
Authenticode, Apple Developer ID/notarization/stapling, Android/iOS app signing, OCI signing,
HSM/KMS custody, app-store/MDM delivery, target install/uninstall tests, second-party reproducible
build comparison, real canary metrics, and immutable artifact-repository controls require the
corresponding protected CI runners and deployment accounts. An official release cannot claim
those gates until their evidence is attached; no local test substitutes for it.

## Completed verification

Verified on 2026-09-07 with `CARGO_BUILD_JOBS=1`:

- the focused Part 44 gate, including release/update unit tests, cross-crate testkit contracts,
  invariant and conformance parity, generated registry verification, support-matrix validation,
  and CLI exposure;
- locked/offline all-feature workspace tests, after granting the existing loopback HTTP test the
  required local bind permission;
- strict workspace Clippy and all-target/all-feature checking;
- Rust 1.87 all-target/all-feature checking and warning-denied workspace Rustdoc;
- Guppy/workspace boundaries (22 rules, 95 crates, 9 layers, 12 isolated dependencies);
- database-neutral custom, Stoolap, SQLite, PostgreSQL, and combined profiles; and
- registry Rust/Markdown generation and verification, formatting, package-order planning, and
  diff hygiene.

The first sandboxed workspace test attempt failed only because the existing `aequora-http`
loopback listener received `Operation not permitted`; the permitted rerun passed. Protected
platform signing, external package/app stores, real target smoke tests, production canaries, and
artifact-store immutability remain deployment evidence as described above.

The semantic retrieval refresh could not initialize its FastEmbed model in the sandbox; the
permitted rerun produced no progress for two minutes and was stopped to preserve bounded laptop
resources. A focused query remained unavailable for the same model-download restriction. This
does not affect the source, registry, build, package, test, documentation, or architecture-gate
evidence above; the working tree remains newer than the retrieval index.
