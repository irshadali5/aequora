# Repository-Wide High-Assurance Hardening Completion

Status: complete on 2026-08-30 for the repository state described by
[`PROMPT.md`](../PROMPT.md).

This pass treated the existing numbered architecture specifications, security invariants, and
database-neutral boundaries as constraints. It did not replace or weaken those contracts.

## Resolved findings

| Severity | Boundary | Finding | Resolution |
| --- | --- | --- | --- |
| High | HTTP client | A caller-supplied `reqwest::Client` could retain a redirect policy while Aequora refreshed sensitive headers for each request. A cross-origin redirect could therefore forward credentials outside the configured authority. | `HttpTransport` now accepts a `ClientBuilder`, validates an HTTP(S) base URL without embedded credentials, rejects zero response/decompression limits, and applies `redirect::Policy::none()` after caller customization. |
| High | Certification | Deserialized conformance artifacts could carry non-canonical coverage claims, unknown test identifiers, malformed digest strings, or future suite versions. Evidence and report fields were insufficiently bounded, and report text could contain active Markdown/HTML content or sensitive messages. | Artifact validation now derives exact coverage from canonical observations, accepts only the current suite and known tests, validates digest shapes, bounds every attacker-controlled collection/string/signature, validates catalog and policy state, and escapes reports while omitting untrusted observation messages. |
| Medium | Evidence bundle | Evidence paths could be absolute, ambiguous, duplicated, backslash-based, or traversing, and bundle verification did not first validate all artifact semantics. | Evidence manifests now require unique normalized relative paths, bounded file/signature counts and sizes, valid digests, and a fully valid embedded artifact before unsigned or signed verification. |
| Medium | Registry generator | Registry discovery and parsing used unbounded whole-file reads and recursive traversal, accepted symlinks, and used delimiter-ambiguous semantic hashing. Generated Markdown did not escape registry-controlled text. | Registry files, directory depth, visited paths, fragment count, total entries, strings, maps, and schema history are bounded. Symlinks and non-regular files fail closed. Semantic fields are escaped before hashing, and generated Markdown is escaped. |
| Medium | CLI and developer tooling | Several local input paths used metadata checks followed by unbounded reads, leaving allocation and time-of-check/time-of-use gaps. One production command arm used `unreachable!()`. | File handles are opened once and read through `take(maximum + 1)` before UTF-8/format decoding. Oversized inputs return typed errors, and invalid command shapes fail with a usage error. |
| Medium | Supply chain | CI used mutable GitHub Action tags and did not scan the root and fuzz lockfiles against RustSec. The root lockfile also contained vulnerable `time 0.3.45`, a yanked ChaCha release, and an unmaintained PEM parser. | Every Action is pinned to an immutable commit SHA. CI installs pinned `cargo-audit 0.22.2` from its lockfile and scans both lockfiles. The test-only RCGen/Time dependency was replaced with a static localhost fixture, ChaCha is resolved to non-yanked `0.10.2`, and compatible Reqwest `0.12.28` removes `rustls-pemfile`. |
| Medium | MSRV reliability | Three let-chain expressions required Rust 1.88 even though the workspace declares and tests Rust 1.87, so the MSRV CI job could not compile. | The security, diagnostics, and client checkpoint branches now use equivalent nested conditionals. A real Rust 1.87 all-target/all-feature workspace check passes. |

Regression tests cover oversized CLI input, malformed/forged certification claims, report
injection, unsafe evidence paths, invalid policy state, non-hex differential digests, registry
delimiter collisions, invalid registry text, and invalid HTTP construction.

## Dependency advisory disposition

The final local scan used `cargo-audit 0.22.2` and RustSec advisory database commit
`b331df68b3ed0e99594d259040bdcb9de3c7c8a4` (2026-08-29). Both lockfiles report zero
unignored vulnerabilities.

- `RUSTSEC-2023-0071` (`rsa 0.9.10`) has no fixed release and is present only because SQLx records
  its optional MySQL driver in `Cargo.lock`. Aequora enables only SQLx PostgreSQL, and
  `cargo tree --workspace --all-features --target all -i rsa` returns no dependency path. The
  narrow, documented exception is in [`.cargo/audit.toml`](../.cargo/audit.toml); it must be
  removed if `rsa` ever becomes reachable or a fixed release lands.
- `RUSTSEC-2026-0253` is a warning for panic-unsound `lru::LruCache::pop`. It is reachable through
  Stoolap 0.4.0, but the current upstream integration uses `new`, `get`, `put`, and `clear`, not
  `pop`; Stoolap 0.4.0 is also the current published release. Track and upgrade the transitive
  dependency when upstream provides one.
- `atomic-polyfill 1.0.3` is an unmaintained warning, not a reported vulnerability. It remains
  transitive through Postcard/Heapless and should be removed by a compatible upstream upgrade.

Warning-only advisories are intentionally visible in CI output. They are not silently suppressed.

## Verification evidence

The following commands completed successfully with `CARGO_BUILD_JOBS=1` where Cargo was invoked:

- `cargo fmt --all -- --check`
- `cargo +1.87.0 check --workspace --all-targets --all-features --locked --offline`
- `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`
- `cargo test --workspace --all-features --locked --offline`
- `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --all-features --no-deps --locked --offline`
- `cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked --offline`
- `cargo run -q -p aequora-dev --locked --offline -- check`
- `bash scripts/check-database-neutrality.sh`
- every `scripts/check-*-architecture.sh` gate
- `cargo audit --file Cargo.lock` and `cargo audit --file fuzz/Cargo.lock`, using the advisory
  snapshot identified above after the tool's integrated Git fetch encountered a transient I/O
  failure
- `bash scripts/check-high-assurance-hardening.sh`

The restricted sandbox denied the HTTP loopback test permission to bind a socket. The complete
workspace test command was rerun with local-socket permission and passed, including HTTP and QUIC
integration tests.

## Residual deployment obligations

Repository evidence cannot establish production TLS termination, IAM and database roles, KMS/HSM
policy, secret rotation, network egress enforcement, audit-log delivery, backups, disaster
recovery, or penetration-test results. Operators must continue to apply the controls and runbooks
in [`SECURITY.md`](../SECURITY.md), [`security-ownership.md`](security-ownership.md), and
[`security-runbooks.md`](security-runbooks.md) for each deployment. No claim of absolute security is
made; this completion records the tested code and dependency state above.
