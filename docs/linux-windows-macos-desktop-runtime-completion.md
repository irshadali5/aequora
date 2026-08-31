# Part 32 desktop runtime completion evidence

The authoritative design is [Part 32](../sys-arch/32-linux-windows-macos-desktop-runtime-architecture.md). This report records executable repository evidence and does not redefine that architecture.

## Implemented boundary

- `aequora-desktop-runtime` defines explicit in-process/agent modes, desktop lifecycle and resume recovery, network/power/storage policy, durable lease fencing, clone detection, secure-store and OS-integration traits, and the nine Part 32 invariant IDs.
- `aequora-ipc-protocol` defines authenticated, length-bounded Postcard frames; N/N-1 version negotiation; the command/event surface; and no raw synchronization-metadata mutation command.
- `aequora-agent` defines current-user plus token authentication, registry/version compatibility, domain-service routing, fenced store ownership, minimal profile registry data, and bounded secret-free diagnostics.
- Linux, Windows, and macOS adapters validate XDG/Application Data/Application Support paths, local Unix-socket or named-pipe identities, platform secure-store choices, autostart choices, and normalized network/VPN/metered/captive-portal state.
- `aequora-update` enforces signed-manifest shape, ordered quiesce/checkpoint/install/migrate/resume transitions, pending-intent preservation, IPC compatibility windows, and rollback compatibility checks.
- The facade, testkit, invariant registry, conformance catalog (`DesktopClientFull` and `DesktopAgentFull`), durable registry, CI, and architecture gate include the desktop surface.

## Deployment ownership

Application distributions still choose their GUI framework and explicit runtime mode, provide durable store/lease adapters, connect native Secret Service/Credential Manager/DPAPI/Keychain APIs, apply owner-only socket or pipe ACLs, and build/sign/notarize AppImage/Flatpak/native Linux packages, MSIX/MSI/ZIP packages, or macOS app/DMG/PKG artifacts. Enterprise applications also supply their managed policy, proxy/private-CA integration, installer controls, and update-signature verifier. These are deployment inputs, not portable core behavior.

Portable mode is accepted only with an explicit passphrase-encrypted secure-store provider. File associations, drag/drop, and directory watchers enqueue validated durable import work; they do not write the synchronized store directly. Derived search/blob caches and UI/tray state remain rebuildable and non-authoritative.

## Verification

The focused gate is:

```text
bash scripts/check-desktop-architecture.sh
```

It verifies all required artifacts and invariant/profile wiring, checks neutral dependency manifests, runs focused crate tests, and runs the cross-crate desktop contract suite. Repository-wide formatting, Clippy, tests, Rustdoc, Guppy direction, database neutrality, registry generation, and diff checks are recorded in the final task handoff after execution.
