# Repository-Wide High-Assurance Hardening Completion

Status: revalidated and extended on 2026-09-09 against [`HARDENING.md`](../HARDENING.md).

This pass treated the existing numbered architecture specifications, security invariants, and
database-neutral boundaries as constraints. It did not replace or weaken those contracts.

## 2026-09-09 resolved findings

This repository-wide pass reviewed the 257 hardening-relevant files changed since the prior
completion, then exercised the workspace, architecture gates, dependency graph, local stress
boundary, and container package.

| Severity | Boundary | Finding | Resolution |
| --- | --- | --- | --- |
| Critical | Stress server authentication | The example described itself as a production node, listened on every interface, and accepted caller-selected `tenant:actor:device` values plus known token prefixes as authentication. | The example is explicitly test-only and loopback by default. Non-loopback container binding requires an opt-in, startup requires an ephemeral 256-bit key, and identities use versioned keyed-BLAKE3 tags with constant-time verification. Command entity and assignee fields are bound to the authenticated envelope and actor. |
| High | Stress evidence integrity | The stress client swallowed local-write and task failures, allowed unbounded workload arguments and latency samples, did not prove outbox drain, and reported absolute invariant preservation. Hot-key updates also attempted insert-only local mutations, making the workload itself invalid. | Workload inputs, allocations, sample retention, redirects, and runtime are bounded. Every failure affects the exit status with bounded diagnostics, shared-identity hot-key updates use atomic update-or-insert mutations, worker joins are checked, adversarial responses must all be rejected, and each local outbox must drain before the result can say that no failure was observed. |
| High | Secret loading | File secrets used a metadata-then-read sequence, while environment and custom providers could return unbounded values. Deserialized secret identifiers bypassed constructor validation. | Files are opened once, permissions are checked on the open handle, and reads stop at 1 MiB plus one byte. The same non-empty and size rules apply to every provider, while identifiers reject whitespace, controls, and values over 256 bytes. Oversized buffers are zeroized before rejection. |
| Medium | Configuration and policy parsing | Several public RON parsers decoded unbounded caller input; topology and observability collections were also unbounded. | Configuration, deployment, release, supply-chain, and observability RON inputs now fail before decoding above explicit byte limits. Topology and catalog collection counts are bounded and regression tested. |
| Medium | Test deployment packaging | The container used `archlinux:latest`; the manifest exposed an ordinary service/host port and omitted a read-only root, seccomp, service-account, capability, and graceful-shutdown controls. | The base image is digest-pinned. The test manifest is loopback-only with no Service, an ephemeral Secret placeholder, non-root execution, no service-account token, `RuntimeDefault` seccomp, read-only root, all capabilities dropped, a bounded temporary volume, and a ten-second termination grace period. The server handles Ctrl-C and SIGTERM cleanly. |
| Medium | Hardening regression gate | The hardening gate required deleted `PROMPT.md` and inspected a moved CLI file, so it no longer validated the intended controls. | The gate now anchors to `HARDENING.md`, checks current source paths, and asserts the new authentication, input-bound, single-open secret, pinned-image, and pod-security controls. |
| Low | Documentation | Historical throughput numbers were presented as proof of zero loss even though the earlier harness suppressed failures and did not verify final drain. | The README labels those figures historical and explicitly retracts the zero-loss interpretation. Current output makes only the bounded claim that no invariant failure was observed. |

## 2026-08-30 resolved findings

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

The current local scan used `cargo-audit 0.22.2` and 1,242 RustSec advisories at database commit
`bf25f6575a93a35f30796c65c0ed91bee7fa19fd` (2026-09-08). Both lockfiles report zero
unignored vulnerabilities. The offline scanner could not acquire the crates.io package-cache lock,
so this pass did not re-evaluate yanked-package status; the RustSec vulnerability scan itself
completed successfully.

- `RUSTSEC-2023-0071` (`rsa 0.9.10`) has no fixed release and is present only because SQLx records
  its optional MySQL driver in `Cargo.lock`. Aequora enables only SQLx PostgreSQL, and
  `cargo tree --workspace --all-features --target all -i rsa` returns no dependency path. The
  narrow, documented exception is in [`.cargo/audit.toml`](../.cargo/audit.toml); it must be
  removed if `rsa` ever becomes reachable or a fixed release lands.
- `RUSTSEC-2026-0253` is a warning for panic-unsound `lru::LruCache::pop`. It is reachable through
  the optional Stoolap adapter, and Stoolap 0.4.0 does call `pop` for caches whose keys are
  `String` or `u64`. Aequora cannot substitute attacker-defined key types at that call site, but
  the warning is not dismissed as unreachable: it remains visible and must be removed by upgrading
  Stoolap when its `lru 0.16` constraint changes.
- `atomic-polyfill 1.0.3` is an unmaintained warning, not a reported vulnerability. It remains
  transitive through Postcard/Heapless and should be removed by a compatible upstream upgrade.

Warning-only advisories are intentionally visible in CI output. They are not silently suppressed.

## Verification evidence

The following commands completed successfully with `CARGO_BUILD_JOBS=1` where Cargo was invoked:

- `cargo fmt --all -- --check`
- `cargo +1.87.0 check --workspace --all-targets --all-features --locked --offline`
- `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`
- `cargo test --workspace --all-features --locked --offline`
- `cargo test -p aequora --examples --all-features --locked --offline`
- `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --all-features --no-deps --locked --offline`
- `cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked --offline`
- `cargo run -q -p aequora-dev --locked --offline -- check`
- `bash scripts/check-database-neutrality.sh`
- every `scripts/check-*-architecture.sh` gate
- `cargo audit --file Cargo.lock` and `cargo audit --file fuzz/Cargo.lock`, using the advisory
  snapshot identified above after the tool's integrated Git fetch encountered a transient I/O
  failure
- `bash scripts/check-high-assurance-hardening.sh`
- release builds for all `aequora` examples
- a Podman build from the digest-pinned Containerfile
- a release-mode, loopback-only container smoke test under a non-root UID, read-only root,
  `no-new-privileges`, and zero Linux capabilities: 214 acknowledged operations, 152 expected
  conflicts, 47/47 adversarial requests rejected, zero unhandled errors, followed by a clean
  SIGTERM shutdown within the configured grace period

The restricted sandbox denied the first loopback bind with `Operation not permitted`. The complete
workspace and explicit harness tests were rerun with local-socket permission and passed; this was
an environment restriction rather than an application failure.

## Residual deployment obligations

Repository evidence cannot establish production TLS termination, IAM and database roles, KMS/HSM
policy, secret rotation, network egress enforcement, audit-log delivery, backups, disaster
recovery, or penetration-test results. Operators must continue to apply the controls and runbooks
in [`SECURITY.md`](../SECURITY.md), [`security-ownership.md`](security-ownership.md), and
[`security-runbooks.md`](security-runbooks.md) for each deployment. No claim of absolute security is
made; this completion records the tested code and dependency state above.
