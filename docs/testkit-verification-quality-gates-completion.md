# Part 48 testkit, verification, fault injection, and quality-gate completion

Part 48 is implemented by extending the existing neutral verification layers instead of creating
parallel substitutes. `aequora-testkit` now exposes a seeded `TestContext`, simulated wall and
monotonic clocks, deterministic test-only identifiers, a one-shot fault schedule covering the
Tx A/B/C and migration/snapshot boundaries, reviewed golden-fixture digests, and bounded RON test
profiles. Production crates have no normal dependency on testkit; the public facade keeps its
legacy `testkit` feature spelling as an empty example compatibility switch backed by a
dev-dependency.

`aequora-conformance::verification` owns the database-, runtime-, and transport-neutral release
contracts: profile-specific required suites, evidence-bound gate results, release-blocking
decisions, explicit expiring waivers, seeded verification reports, and typed invariant-to-test
coverage maps. The existing `aequora-model` remains the executable semantic oracle and continues
to provide bounded exploration plus versioned RON failure-trace replay. The CLI now validates
quality manifests and reports and accepts `verify replay` as the architecture spelling for model
trace reproduction.

Executable evidence includes `AEQ-INV-VERIFY001` through `AEQ-INV-VERIFY010`, conformance profile
`VerificationFull` and tests 164–173, the four bounded configurations under `config/test/`, the
Part 48 quality-gate fixture and contract suite, and `scripts/check-verification-architecture.sh`.
The structural gate also exercises the existing property, model, Tx/fault, bootstrap, scope,
tenant-isolation, tombstone, ambiguous-commit, CLI, registry, and invariant suites.

Real PostgreSQL/Neon services, hard process kills against physical SQLite/Stoolap filesystems,
mobile devices, cross-platform runners, long fuzz campaigns, HA/failover, load/soak, backup/PITR,
air-gapped hardware, and release-candidate artifact qualification remain environment-bound
evidence. The implemented manifests represent those results explicitly; they are never reported
as passed merely because repository-only tests succeeded.
