# Part 30 certification and conformance completion evidence

Authority: `sys-arch/30-certification-conformance-ecosystem-architecture.md`.

Implemented surfaces:

- `aequora-conformance`: database-neutral profiles, tiers, domains, stable reference tests,
  capability evidence, coverage, exact subject/environment binding, content-derived artifact IDs,
  bundle hashes, signature-verifier boundary, Markdown reporting, catalog lifecycle, trust labels,
  deployment enforcement, and differential comparison.
- `aequora-testkit::conformance`: deterministic reusable runner, seeds, public downstream macro,
  isolated test observations, and governed fault-injection points. Existing local/authority adapter
  contracts remain the executable storage reference suite.
- `aequora conform`: request evaluation, domain discovery, report generation, tamper verification,
  and exit codes 0 pass, 1 failure, 2 setup/usage, 3 unsupported.
- Part 29 registry domains for conformance profiles, tests, and certification tiers; nine stable
  `AEQ-INV-CERT001..009` invariants; versioned fixtures; empty-by-default ecosystem catalog; trust,
  provenance, SBOM, security-advisory, and lifecycle policy.
- Production startup policy is fail-closed and exact-match; development mode returns visible
  warnings. Certification remains evidence and never bypasses runtime checks.

The ecosystem catalog is deliberately empty until an exact built-in or third-party binary is run
against the suite and its reproducible artifact is reviewed. This avoids falsely turning repository
ownership or a fixture into an official certification.

Repository acceptance is enforced by `scripts/check-certification-architecture.sh`, the CI Part 30
step, registry verification, strict Clippy, workspace tests/docs, Guppy boundaries, and database
neutrality checks.
