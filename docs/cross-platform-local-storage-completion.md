# Part 33 cross-platform local storage completion evidence

The authoritative design is [Part 33](../sys-arch/33-cross-platform-local-storage-mobile-desktop-architecture.md). This report records executable repository evidence and does not redefine that architecture.

## Implemented boundary

- `aequora-storage-core` defines the five storage classes, durability/admission/pressure states, canonical errors, safe platform path policy, format downgrade checks, secure binding classification, mandatory adapter capabilities, and verify-before-publish state transitions.
- `aequora-storage-profile` supplies mobile and desktop presets, conservative free-space preflight, read-mostly pressure transitions, and intent-safe eviction decisions.
- `aequora-storage-maintenance`, `aequora-storage-backup`, and `aequora-storage-encryption` define bounded checkpointed migration/GC, transactionally consistent manifests and restore classification, resumable key rotation, and legal-hold-aware cryptographic erase.
- `aequora-blob-store` supplies content-addressed BLAKE3 verification, pin/reconstructability classification, and manifest-last publication.
- `aequora-storage-conformance` binds full mobile/desktop certification to an adapter version, exact target, mandatory capabilities, shared tests, and platform-specific tests; missing or unsupported observations fail the claim.
- The facade, testkit, invariant registry, durable certification registry, CI, and focused architecture gate include the Part 33 surface.

## Deployment ownership

Applications select and configure an embedded engine, materialize the documented private/cache/diagnostic paths through native platform APIs, connect Android Keystore, iOS/macOS Keychain, Windows DPAPI/Credential Manager, Linux Secret Service or an explicit fallback, and establish backup inclusion/exclusion rules. Adapter maintainers run destructive crash, process-kill, low-disk, clone, locking, antivirus, sleep/resume, and key-loss tests on each supported target and publish the resulting certification record. The reusable core does not claim those deployment observations on behalf of an adapter.

SQLite and Stoolap remain adapters rather than architectural dependencies. Part 33 supplies the target-neutral contract and target-bound certification model; official adapter SDK and engine implementation work remains governed by the later adapter specification.

## Verification

The focused gate is `bash scripts/check-local-storage-architecture.sh`. It checks required artifacts, all ten invariant/profile integrations, neutral dependency manifests, focused crate tests, and cross-crate contracts. Repository-wide formatting, Clippy, tests, Rustdoc, Guppy direction, database neutrality, registry generation, and diff checks are recorded in the final task handoff after execution.
