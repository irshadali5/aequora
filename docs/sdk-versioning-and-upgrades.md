# SDK versioning and upgrade policy

The public crates use Semantic Versioning and initially release in lockstep. Before 1.0, a minor version may contain a documented public API break; patch releases must remain compatible. After 1.0, a stable API break requires a major version unless the item is explicitly under `aequora::experimental` or an unstable feature.

Crate semver is independent from:

- network protocol versions;
- local store format and metadata schema versions;
- application operation schema versions;
- adapter certification suite versions.

One crate release may support multiple protocol and store-format versions. An operation schema change does not by itself require an Aequora crate major version.

## Deprecation

Stable renames add the replacement first and mark the prior item with `#[deprecated]`. Every deprecation must name the replacement, expected removal release, and a migration example. After 1.0, common deprecated APIs remain for at least one normal release window unless a security issue requires faster removal.

## MSRV

The workspace MSRV is declared in `Cargo.toml`. Release notes must call out every MSRV increase. After 1.0, an MSRV increase follows the declared release policy and does not force consumers onto a new Rust edition unnecessarily.

## Compatibility matrix

| SDK line | Protocol | Local store format | Adapter SDK | Operation schema |
|---|---|---|---|---|
| 0.1.x | Negotiated by `aequora-compat`; currently V1 | Adapter-declared and migration-checked | 0.1.x | Application-owned; one or more versions per registry |

Stable error codes in `architecture/public-rust-api-v0.1.txt` remain interpretable throughout the supported line even when internal source errors or display messages change.

## Release gate

Pull requests run the focused SDK architecture gate and compare public crates against the pull request base with `cargo-semver-checks`. Intended pre-1.0 breaking changes require a minor version change plus migration notes. Release review also requires workspace tests, doctests, warning-denied Rustdoc, example compilation, the compatibility matrix, and deprecation review.
