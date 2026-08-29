# Part 25 diagnostics, forensics, and reproducible incident bundles completion

## Implemented scope

The reusable implementation of
[`sys-arch/25-diagnostics-forensics-reproducible-incident-bundles.md`](../sys-arch/25-diagnostics-forensics-reproducible-incident-bundles.md)
is centered on the runtime-, transport-, archive-, cryptography-provider-, and database-neutral
`aequora-diagnostics` crate.

Delivered contracts include:

- UUIDv7 incident, bundle, and diagnostic-event identities; typed incident classes and bounded
  operation, entity, scope, device, job, and time selectors;
- operation, entity, scope, device, authority, and job forensic views with explicit evidence
  confidence, plus deterministic operation explanation and root-cause confidence;
- a client ring bounded simultaneously by event count, estimated bytes, and age, with typed and
  size-limited details;
- support-minimal, support-standard, forensic, and guarded developer policies with explicit file,
  record, byte, time-window, archive-depth, payload, log, encryption, and signing choices;
- classification-aware sanitization that always redacts secrets and digests restricted/personal
  values unless a stronger policy explicitly permits their inclusion;
- versioned manifests, runtime/build/adapter/protocol/schema/capability inventories, canonical
  timelines, explicit missing sections, sequence/epoch boundaries, retention/legal-hold decisions,
  and temporary incident/bundle registry contracts;
- preflight bundle plans bound by a deterministic plan digest, completeness declarations, a
  canonical BLAKE3 hash inventory, safe relative-path validation, and archive-bomb/link bounds;
- provider-neutral encryption, signing, and signature-verification boundaries that expose key IDs
  but never private-key material;
- isolated metadata/protocol/domain/full-simulation replay contracts, handler/digest mismatch
  reporting, capture-only side-effect intents, production-attachment rejection, and deterministic
  fault-script minimization;
- thin client and tenant-authorized server collection integrations, read-only CLI inspection and
  explanation, and `AEQ-INV-DIAG001` through `AEQ-INV-DIAG009` in the normative registry.

## Executable evidence

The repository contains unit scenarios for three-dimensional ring eviction, secret/PII
sanitization, path traversal and archive bombs, changed-file hash failure, explicit partial
collection, commit-with-lost-response explanation, protection requirements, production replay
rejection, minimization, and retention. The cross-crate test proves tenant authorization and
sanitization at the server boundary.

`scripts/check-diagnostics-architecture.sh` checks the neutral dependency boundary, required
client/server/CLI/testkit surfaces, absence of private-key types, and all nine invariant IDs. CI
runs the structural gate, focused core and integration tests, and read-only CLI capability output.

The final implementation run passed formatting, all eight architecture scripts including the new
Part 25 gate, all four database-neutral composition profiles, workspace all-target/all-feature
check, strict workspace Clippy, the complete all-feature workspace test suite, and rustdoc with
warnings denied. Guppy reported 20 boundary rules across 57 workspace crates. The sandboxed test
run reached the existing HTTP loopback test and was denied permission to bind; the complete suite
was then rerun with local-listener permission and passed.

## Deployment-owned boundaries

Applications supply durable diagnostic and incident registries, observability indexes, database
adapter providers, governance authorization/audit adapters, archive streaming/extraction,
protected object storage, expiry deletion jobs, KMS or recipient encryption, signing keys,
authenticated admin routes, support upload/merge transport, historical handler artifacts,
sandbox stores/providers, and production telemetry. Those integrations must keep collection
bounded, sanitize before publication, audit sensitive access, and never attach replay to production
authority, clients, or side-effect providers.
