# Part 41 CLI and developer toolchain completion

The governing design is
[`sys-arch/41-cli-developer-toolchain-architecture.md`](../sys-arch/41-cli-developer-toolchain-architecture.md).
This report records executable repository evidence and does not replace that design.

## Implemented boundaries

`aequora-cli-core` owns stable top-level command nouns, global output/log/color options, versioned
JSON and RON envelopes, exit and error categories, redacted safe details, command safety metadata,
typed plan/apply identities, durable-submission wait semantics, migration precondition validation,
and structured doctor results. It is independent of runtimes, transports, UI frameworks, physical
databases, and server internals.

`aequora-inspect` selects authenticated agent IPC, direct read-only access, or explicitly fenced
offline maintenance from durable ownership observations. It refuses an uncontrolled second writer
and applies registry-style field classification with redaction by default.

`aequora-devtools` creates deterministic, versioned scaffold plans, distinguishes generated from
developer-owned files, reports collisions instead of overwriting, rejects destructive development
actions against explicit production markers, and excludes failpoint execution unless the
`test-failpoints` feature was built.

The existing mature commands now execute behind `aequora-cli` as a library. The binary
`src/main.rs` only enters that composition layer. Human rendering remains command-specific, while
`--output json` and `--output ron` use a stable schema envelope. Machine errors use registered exit
categories and sanitized public messages. The `version` command reports build identity without
credentials.

## Safety and conformance evidence

The implementation registers `AEQ-INV-CLI001` through `AEQ-INV-CLI010` in the normative invariant
registry and in the `CliToolchainFull` conformance profile. Focused tests cover parser stability,
machine schema versioning, secret redaction, authority plan/apply requirements, cancellation after
durable submission, migration fail-closed behavior, agent-aware store routing, fenced maintenance,
secret field rendering, deterministic scaffolding, non-overwrite behavior, and production guards.

`scripts/check-cli-toolchain-architecture.sh` verifies required artifacts and symbols, all ten
invariants across the specification/registry/conformance layers, the thin binary boundary,
database/runtime/transport neutrality of reusable tooling crates, absence of direct physical store
access in the CLI, bounded rust-analyzer settings, focused tests, registry integrity, and a real
versioned JSON doctor invocation. CI runs this gate.

Repository completion additionally requires formatting, strict Clippy, warning-denied Rustdoc,
Rust 1.87 checking, Guppy boundaries, database neutrality, and workspace tests. Run one Cargo
process at a time with `CARGO_BUILD_JOBS=1` on resource-constrained machines.

## Explicit v1 boundary

The command hierarchy and policy metadata cover the complete Part 41 noun space. Commands execute
only where a canonical reusable SDK, control-plane, registry, conformance, or adapter service
already exists; the toolchain does not invent direct-table implementations for future command
surfaces. Shell completion, generated man pages, an optional TUI/language server, package-manager
delivery, and deployment-specific authentication/credential-store integrations remain downstream
application or release work as assigned by the governing architecture.

## Completed verification

Verified on 2026-09-01 with `CARGO_BUILD_JOBS=1`: the focused Part 41 architecture gate, generated
registry Rust and Markdown snapshots, registry integrity, strict workspace Clippy, Rust 1.87
all-target/all-feature checking, warning-denied workspace Rustdoc, the complete all-feature
workspace test suite, Guppy dependency/workspace boundaries, all database-neutral composition
profiles, workspace formatting, and `git diff --check`. The loopback HTTP test was rerun with
listener permission after the restricted sandbox rejected its bind, and the complete permitted
workspace suite passed.

The semantic RAG index was rebuilt on 2026-09-01 with a writable cache under `/tmp` and one bounded
indexing process: 577 of 577 files were processed (131 new and 446 unchanged), producing 1,298
GraphRAG blocks. Follow-up semantic queries retrieved both the Part 41 architecture document and the
`CliErrorEnvelope` implementation in `aequora-cli-core`.
