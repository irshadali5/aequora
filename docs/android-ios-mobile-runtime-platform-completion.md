# Android and iOS mobile runtime architecture completion

This report records executable repository evidence for
[`sys-arch/31-android-ios-mobile-runtime-platform-architecture.md`](../sys-arch/31-android-ios-mobile-runtime-platform-architecture.md).
The numbered specification remains the design authority.

## Implemented reusable boundary

| Contract | Executable evidence |
|---|---|
| Shared Rust runtime | `aequora-mobile-runtime` defines lifecycle, network, power, thermal, storage, memory, execution-budget, push-hint, secure-provider, recovery, scope-eviction, event, and single-coordinator contracts without a database, transport, JNI, Apple framework, or async-runtime dependency. |
| Process-death and bounded execution | Durable startup classification rejects cursor-ahead metadata and downgrade, recovers stale in-flight work, resumes staged bootstrap, and preserves scheduler backoff. `MobileCoordinator` coalesces wake sources, fences competing execution, and accepts only checkpointed completion states. |
| Android integration | `aequora-platform-android` normalizes lifecycle, ConnectivityManager-style network facts, power/thermal state, WorkManager grants, private store placement, hardware-key policy, and auto-backup exclusion. It declares required baseline ABIs and permissions without importing Android types into the core. |
| iOS integration | `aequora-platform-ios` normalizes lifecycle, NWPathMonitor-style facts, power/thermal state, BGTask grants, protected Application Support placement, Keychain-group input, and iCloud backup exclusion. It declares baseline device/simulator targets without importing UIKit types into the core. |
| Kotlin/Swift/Dioxus binding boundary | `aequora-mobile-bindings` exposes bounded high-level async commands/outcomes and coalesced UI invalidations, stable ABI/build identity, and panic containment. Cursor and operation-ledger mutation are not representable. Dioxus can call the same Rust host directly while retaining platform service adapters. |
| Security and privacy | Opaque secure-key handles are preferred, fallback secret bytes zeroize and redact, push hints contain only bounded routing/reason data, store policy fails closed for public/traversing or backup-cloned state, and diagnostics use a sanitized export command. |
| Compatibility and conformance | `MobileClientFull` selects client, protocol, and nine mobile runtime tests. Registry test IDs 21–29 and `AEQ-INV-MOBILE001..009` are generated, locked, and covered by the invariant registry. |
| Repository acceptance | `scripts/check-mobile-architecture.sh`, focused cross-crate tests, CI wiring, Guppy dependency checks, and the database-neutrality gate enforce the reusable boundary. |

The runtime deliberately reuses Aequora's existing durable outbox, cursor, bootstrap, conflict,
compatibility, authority-epoch, local fencing, streaming, and diagnostics semantics. It does not
create a second mobile protocol or synchronization engine. Local synchronized domain data and its
outbox must remain in one certified transactional adapter; Room/CoreData may be projections but
must not independently own authoritative local mutations.

## Platform and release acceptance still required

This host repository cannot truthfully certify OS services or ship application artifacts. Each
mobile application must still:

- implement the Rust traits with real Android Keystore/ConnectivityManager/WorkManager/FCM and
  iOS Keychain/NWPathMonitor/BGTaskScheduler/APNs integrations;
- integrate a certified embedded store in the private/protected application container and verify
  atomic mutation plus outbox behavior, disk-full handling, snapshot/blob streaming, and fencing;
- generate the concrete JNI/Kotlin and Swift wrapper, build signed AAR/XCFramework/application
  artifacts for the supported ABI/target matrix, restrict exported symbols, and test ABI memory
  ownership under sanitizers/leak tooling;
- run device instrumentation for kill-before/after-commit, reconcile termination, background
  expiration, key-locked deferral, push loss/duplication, week-offline rebootstrap, backup restore,
  reinstall, app upgrade/downgrade, low storage, metered/roaming, thermal pressure, and UI main-thread
  responsiveness;
- configure actual permissions, entitlements, privacy declarations, root/jailbreak policy,
  at-rest protection, diagnostic sharing, crash/log redaction, minimum-build policy, and release
  provenance.

Passing host tests establishes the portable architecture contract; it is not an Android or iOS
store-release certification. A `MobileClientFull` claim requires exact binary, store adapter,
secure-provider, OS/device, binding, and execution-environment evidence under Part 30.
