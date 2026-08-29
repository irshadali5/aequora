# Part 27 security threat model and abuse resistance completion

The detailed authority is
[`sys-arch/27-security-threat-model-abuse-resistance.md`](../sys-arch/27-security-threat-model-abuse-resistance.md).
This file records implementation locations and verification without duplicating that design.

## Implementation map

- `aequora-security` is the runtime-, transport-, provider-, and database-neutral boundary for
  validated authentication evidence, tenant/resource binding, safe security profiles, bounded
  protocol/compression/upload/archive shapes, SSRF-safe resolved egress, replay/payload binding,
  authority rollback, secret redaction/zeroization, side-effect safety, typed events/errors/metrics,
  assets, attacker classes, trust boundaries, and the ten Part 27 invariants.
- `AuthContext::from_validated_security` is the explicit bridge into authoritative sync execution;
  it cannot construct sync identity without the policy-validated active device binding.
- The shared invariant registry contains `AEQ-INV-SEC001` through `AEQ-INV-SEC010`, each with a
  model assertion, property/regression test, adapter contract, and payload-free diagnostic metric.
- `config/security.ron` is a fail-closed Standard example. Enterprise and HighAssurance constructors
  add internal mTLS, signed artifacts, hardware-backed assurance, and two-person destructive approval
  as specified. Profiles are not certifications.
- `docs/security-threat-matrix.md`, `docs/security-runbooks.md`, `docs/security-ownership.md`, and
  `SECURITY.md` maintain the threat register, incident response, review ownership, release checklist,
  and private disclosure path.
- `scripts/check-security-architecture.sh`, the testkit abuse suite, the CLI `security` commands, the
  Guppy dependency rule, and CI keep the architecture enforceable.

Deployment evidence remains deployment-owned for TLS termination, mTLS/workload identity, private
admin networking, DB roles/RLS, KMS and object ACLs, backup/crash-dump protection, SIEM/alert routing,
secret/advisory scanners, DAST, SBOM/provenance, malware scanning, and penetration testing. Core APIs
model and require these controls where applicable without claiming that an arbitrary deployment has
enabled them.

## Verification record

Verified on 2026-08-29 from the final working tree:

- focused `aequora-security`, invariant, executor, CLI, and testkit abuse contracts passed;
- strict all-workspace/all-target/all-feature Clippy passed with warnings denied;
- the architecture script, example-policy validation, 21-rule/60-crate Guppy check, and all four
  database-neutrality profiles passed;
- the sandbox-compatible all-feature workspace suite passed 415 tests with one ignored test;
- the HTTP and QUIC suites each passed their non-network test but their loopback test could not open
  a socket under this sandbox (`Operation not permitted`), an environmental limitation rather than
  a security assertion failure; and
- formatting and diff hygiene passed before handoff.
