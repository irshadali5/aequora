# Part 21 protocol negotiation and compatibility governance completion

The repository-owned implementation of
[`sys-arch/21-protocol-negotiation-compatibility-governance.md`](../sys-arch/21-protocol-negotiation-compatibility-governance.md)
is complete. The implementation is runtime-, transport-, and database-neutral; application stores,
mobile distribution, control-plane policy signing, and fleet rollout orchestration remain host
responsibilities behind these contracts.

## Delivered architecture

- `aequora-compat` separates protocol, operation, domain, snapshot, projection, payload, adapter,
  audit, governance-policy, and local-store format versions instead of using a global version.
- Bounded `ClientHello` input is evaluated by a stateless, server-selected negotiator. Ordered
  policy preference can intentionally select a stable version instead of the highest common one.
- Required safety/semantic capabilities and client minimum-security policy fail closed. Optional
  compression and transport behavior can fall back only when their declared category permits it.
- Full, read-only, bootstrap-only, upgrade-recommended, and upgrade-required states use structured
  reason/recovery types. Compatibility state transitions have no local-intent mutation API.
- The stable envelope contract carries protocol, message kind, payload version, and length before
  decode. Bounded optional extensions may be ignored; unknown required extensions are rejected.
- A pure ordered upcaster registry converts old operation payloads only toward canonical current
  semantics. Possibly-sent operations retain original schema/bytes/hash, and `RetryOnly` accepts
  only ledger-known identical retries while rejecting new creation.
- The reviewed `registry/compatibility.ron` owns protocol, capability, and operation IDs. Validation
  rejects duplicates, zero IDs, invalid version ranges, removed-ID reuse, missing retry horizons,
  and removals without structured recovery.
- Fleet manifests prevent `Enabled` or `Required` activation before every serving node supports the
  capability. Server-controlled canary sets and immediate gate rollback remain explicit.
- Adapter API/store-format/capability manifests provide database-neutral startup certification.
- `aequora compat show|matrix|deprecated|check-client|registry` provides bounded, payload-free,
  read-only release diagnostics.

## Executable evidence

`crates/aequora-testkit/tests/compatibility_contracts.rs` proves:

- server preference selection instead of numeric maximum selection;
- required-capability and protocol-strip downgrade rejection;
- read-only grace and durable-intent preservation during blocking upgrades;
- retry-only historical acceptance and possibly-sent payload immutability;
- deterministic multi-step operation upcasting;
- mixed-fleet rejection before required capability activation;
- canonical registry and required-extension validation; and
- exact Postcard bytes plus current decoding for the version-one client hello fixture.

`aequora-protocol` also pins append-only capability wire discriminants to the canonical numeric
registry IDs. `fuzz/fuzz_targets/compatibility_inputs.rs` exercises untrusted hello and extension
decoding. The invariant registry contains `AEQ-INV-COMP001` through `AEQ-INV-COMP009`, and
`scripts/check-compatibility-architecture.sh` keeps the crate layout, policy surfaces, registry,
protocol marker, invariant IDs, and focused proofs in the repository gate.

## Verification commands

```text
cargo test -p aequora-compat --locked --offline
cargo test -p aequora-testkit --test compatibility_contracts --locked --offline
cargo run -p aequora-cli --locked --offline -- compat registry
bash scripts/check-compatibility-architecture.sh
cargo run -p aequora-dev --locked --offline -- check
bash scripts/check-database-neutrality.sh
```

The workspace formatting, all-target/all-feature checking, strict Clippy, tests, rustdoc, fuzz-build,
and retrieval-index gates provide the repository-wide release evidence recorded at completion.
