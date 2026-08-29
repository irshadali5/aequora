# Part 26 implementation evidence

The governing design remains
[`sys-arch/26-legacy-application-compatibility-incremental-adoption.md`](../sys-arch/26-legacy-application-compatibility-incremental-adoption.md).
This report records implementation evidence only.

## Delivered surfaces

- `aequora-legacy` provides explicit discovery/manifests, canonical reader and mapper contracts,
  provenance, bounded CDC batches, atomic ledger/cursor persistence, quarantine, reconciliation,
  ID mapping, generation-checked aggregate ownership, writer and side-effect inventory, pure shadow
  planning, readiness, verified cutover, rollback refusal, governance coverage, control-plane
  permissions/routes, and retirement manifests.
- `aequora-legacy-api` confines legacy identity, request, idempotency, and error translation to the
  edge and routes typed operations through the canonical handler.
- The invariant registry contains `AEQ-INV-LEG001` through `AEQ-INV-LEG009` with model, property,
  adapter, and diagnostic evidence hooks.
- The CLI inspects bounded RON evidence for discovery, mapping, bridge health, shadow results,
  cutover readiness, final verification, and retirement without performing writes.
- `legacy_adoption_contracts` exercises duplicate delivery, crash/gap cursor safety, stale legacy
  fencing, complete cutover verification, fail-closed mapping, governance coverage, shadow
  isolation, facade retry identity, and unsupported post-cutover rollback.

## Repository enforcement

`scripts/check-legacy-architecture.sh` enforces neutral dependency direction, required modules,
invariants, tooling, and contract-test presence. CI runs it with the focused crates and contract
suite; the normal workspace, Clippy, Rustdoc, Guppy, and database-neutrality gates provide the
cross-repository checks.

## Verification

The Part 26 architecture gate, focused seven-scenario contract suite, Guppy boundary check,
database-neutrality matrix, all-target/all-feature workspace check, strict Clippy, complete
all-feature workspace tests, and warning-denied Rustdoc pass. The workspace tests were rerun with
loopback permission after the sandbox correctly blocked the HTTP transport contract. The retrieval
index was refreshed after verification.
