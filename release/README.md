# Aequora release operations

The typed authority for release manifests, final-byte verification, update decisions, rollback,
and promotion is `aequora-release`. The architectural authority is `sys-arch/44-*`. This directory
contains operational inputs rather than a second architecture specification.

Part 50 adds `v1-scope.ron` and `v1-readiness.ron`. The former is the machine-readable v1 scope
freeze. The latter is a truthful development readiness snapshot and intentionally evaluates as
ineligible for GA until exact release, deployment, upgrade, target, ownership, and pilot evidence
is attached. `KNOWN_ISSUES.md` defines the companion issue-recording policy.

## Required bundle

An official bundle contains `release-manifest.ron`, its detached signature, `SHA256SUMS`,
`BLAKE3SUMS`, purpose-specific artifact signatures, per-artifact SBOM/provenance references,
adapter migration bundles, the registry snapshot, configuration schema, conformance report,
compatibility matrix, release notes, artifacts, and separately retained debug symbols. Names use
`<product>-<semver>-<target>.<format>` and existing versioned objects are never overwritten.

## Build and promotion

Candidate builds use the pinned Rust toolchain, committed `Cargo.lock`, `--locked`, an explicit
feature set, a clean source revision, normalized timestamps/paths, and the checked-in support
matrix. The correctness, compatibility, security, migration, conformance, performance,
documentation, packaging, and provenance gates must all pass before promotion.

Build jobs have read-only source access and no production signing or publishing credentials.
Platform-native/HSM signing jobs receive final packaged bytes, return purpose-bound signatures,
and cannot change the package afterward. Publish jobs promote those same bytes through immutable
RC/canary/stable object identities. `PromotionEvidence::authorize_stable` rejects missing gates,
changed bytes, insufficient approval, and evidence that build jobs possessed production secrets.

## Platform responsibility

`support-matrix.ron` declares the v1 target/package/smoke-test contract. Linux archives and OCI
images are built on controlled Linux runners; Windows MSI/Authenticode work runs on Windows;
macOS/iOS signing, notarization, stapling, provisioning, and packaging run on protected macOS
infrastructure; Android app signing runs in protected Android release tooling. Rust core artifacts
ship inside the native mobile application and never self-update independently.

Production signing keys, app-store credentials, notarization identities, package repositories,
OCI registries, HSM/KMS integrations, real devices, and enterprise MDM systems are deployment
inputs. Their evidence must be attached to the release audit record; the repository does not
fabricate it.

## Verification and recovery

`aequora release verify <manifest.ron> <trust.ron> <artifact-directory>` performs offline
manifest, trust-lifecycle, size, SHA-256, BLAKE3, and signature checks. Enterprise mirrors may
redistribute the same bytes but cannot replace the independent trust root. Update metadata is
signed separately, expires, carries a monotonic generation, supports rollout halt/revocation,
and is checked before download/install policy.

Rollback follows the declared class. Forward-only releases cannot roll back; migration-required
releases need verified reversible migration evidence. Debug symbols, manifests, signatures,
SBOMs, provenance, stable/security releases, and audit records follow protected long-term
retention; nightlies may use a shorter documented retention window.
